use std::{env, fs, path::Path, process::Command};

fn main() {
    let result = match env::args().nth(1).as_deref() {
        Some("doctor") => doctor(),
        Some("verify") => verify(),
        Some("icons") => icons(),
        _ => Err("usage: cargo xtask <doctor|verify|icons>".to_owned()),
    };

    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn doctor() -> Result<(), String> {
    let root = workspace_root();
    let rust = read_toml_value(&root.join("rust-toolchain.toml"), "channel");
    let node = read_first_line(&root.join(".node-version"))
        .or_else(|| read_first_line(&root.join(".nvmrc")));
    let pnpm = read_json_string(&root.join("package.json"), "packageManager")
        .and_then(|value| value.strip_prefix("pnpm@").map(str::to_owned));
    println!("Goodtype toolchain doctor");
    let mut required_ok = true;
    required_ok &= check_tool("Rust", "rustc", rust.as_deref(), true);
    required_ok &= check_tool("Cargo", "cargo", None, true);
    required_ok &= check_tool("Node", "node", node.as_deref(), true);
    required_ok &= check_tool("pnpm", pnpm_command(), pnpm.as_deref(), true);
    if node.is_none() {
        println!("WARN Node: no .node-version or .nvmrc project pin found");
    }

    if required_ok {
        Ok(())
    } else {
        Err("doctor found missing or mismatched required tools".to_owned())
    }
}

fn check_tool(name: &str, command: &str, expected: Option<&str>, required: bool) -> bool {
    match Command::new(command).arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let matches = expected.is_none_or(|expected| version_matches(&version, expected));
            let note = expected.map_or(String::new(), |value| format!("; expected {value}"));

            if matches {
                println!("OK   {name}: {version}{note}");
                true
            } else {
                let level = if required { "ERROR" } else { "WARN " };
                println!("{level} {name}: {version}{note}");
                !required
            }
        }
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr);
            let level = if required { "ERROR" } else { "WARN " };
            println!(
                "{level} {name}: `{command} --version` failed: {}",
                detail.trim()
            );
            !required
        }
        Err(error) => {
            let level = if required { "ERROR" } else { "WARN " };
            println!("{level} {name}: not available ({error}); install it or add it to PATH");
            !required
        }
    }
}

fn verify() -> Result<(), String> {
    let root = workspace_root();
    let commands: &[(&str, &str, &[&str])] = &[
        ("Rust format", "cargo", &["fmt", "--all", "--", "--check"]),
        (
            "Rust Clippy",
            "cargo",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("Rust tests", "cargo", &["test", "--workspace"]),
        (
            "Frontend unit tests",
            pnpm_command(),
            &["--filter", "@goodtype/desktop", "test"],
        ),
        (
            "Svelte, TypeScript, and frontend build",
            pnpm_command(),
            &["--filter", "@goodtype/desktop", "build"],
        ),
    ];

    for (label, program, args) in commands {
        println!("\n==> {label}");
        let status = Command::new(program)
            .args(*args)
            .current_dir(&root)
            .status()
            .map_err(|error| format!("failed to run `{program}`: {error}"))?;

        if !status.success() {
            return Err(format!("{label} failed with {status}"));
        }
    }

    println!("\nVerification passed.");
    Ok(())
}

/// Rasterise `brand/icon.svg` into the bitmaps Tauri embeds and bundles.
///
/// The icons are generated rather than committed by hand so the SVG stays the single source of
/// truth: changing the mark means editing one file and re-running this, not reconciling six
/// bitmaps that may or may not have come from the same artwork. It runs offline and needs no
/// design tool — `resvg` is already in the lockfile as a Typst dependency.
fn icons() -> Result<(), String> {
    let root = workspace_root();
    let destination = root.join("apps/desktop/src-tauri/icons");
    fs::create_dir_all(&destination).map_err(|error| error.to_string())?;

    // Detail is dropped as the icon shrinks rather than downsampling one artwork everywhere: at
    // 32px the two set lines collapse into a grey smear, and a smudged page reads worse than no
    // page. Each level is its own SVG so the choice is visible and editable, not buried here.
    let artwork = |size: u32| -> &'static str {
        match size {
            0..=32 => "brand/icon-small.svg",
            33..=64 => "brand/icon-medium.svg",
            _ => "brand/icon.svg",
        }
    };

    let load = |size: u32| -> Result<resvg::usvg::Tree, String> {
        let path = root.join(artwork(size));
        let svg =
            fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())
            .map_err(|error| format!("{} is not valid SVG: {error}", path.display()))
    };

    // The names Tauri looks for. `icon.png` is the one `generate_context!` embeds on every
    // platform that is not Windows; without it the build fails rather than falling back.
    let targets: &[(&str, u32)] = &[
        ("32x32.png", 32),
        ("128x128.png", 128),
        ("128x128@2x.png", 256),
        ("icon.png", 1024),
    ];

    for (name, size) in targets {
        let pixmap = render(&load(*size)?, *size)?;
        let path = destination.join(name);
        pixmap
            .save_png(&path)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        println!("  {name} ({size}px, {})", artwork(*size));
    }

    // Windows wants one .ico holding every size the shell might ask for, so it never has to
    // scale a mismatched bitmap itself.
    let mut ico = Vec::new();
    let encoder = image::codecs::ico::IcoEncoder::new(std::io::Cursor::new(&mut ico));
    let frames = [16u32, 24, 32, 48, 64, 128, 256]
        .into_iter()
        .map(|size| {
            let pixmap = render(&load(size)?, size)?;
            // Each frame is stored as PNG inside the container, which is what modern Windows
            // reads and what keeps the file small at 256px.
            image::codecs::ico::IcoFrame::as_png(
                pixmap.data(),
                size,
                size,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|error| format!("could not build a {size}px frame: {error}"))
        })
        .collect::<Result<Vec<_>, String>>()?;
    encoder
        .encode_images(&frames)
        .map_err(|error| format!("cannot encode icon.ico: {error}"))?;
    let ico_path = destination.join("icon.ico");
    fs::write(&ico_path, &ico).map_err(|error| format!("cannot write {ico_path:?}: {error}"))?;
    println!("  icon.ico (16-256px)");

    println!("\nIcons written to {}.", destination.display());
    Ok(())
}

fn render(tree: &resvg::usvg::Tree, size: u32) -> Result<resvg::tiny_skia::Pixmap, String> {
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)
        .ok_or_else(|| format!("cannot allocate a {size}px canvas"))?;
    let scale = size as f32 / tree.size().width();
    resvg::render(
        tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must be inside the workspace root")
        .to_owned()
}

fn read_first_line(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()?
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

fn read_toml_value(path: &Path, key: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()?
        .lines()
        .map(str::trim)
        .find_map(|line| {
            let (candidate, value) = line.split_once('=')?;
            (candidate.trim() == key).then(|| value.trim().trim_matches('"').to_owned())
        })
}

fn read_json_string(path: &Path, key: &str) -> Option<String> {
    let marker = format!("\"{key}\"");
    fs::read_to_string(path)
        .ok()?
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(&marker))?
        .split_once(':')
        .map(|(_, value)| {
            value
                .trim()
                .trim_end_matches(',')
                .trim_matches('"')
                .to_owned()
        })
}

fn version_matches(actual: &str, expected: &str) -> bool {
    actual
        .split_whitespace()
        .map(|part| part.trim_start_matches('v'))
        .any(|part| part == expected.trim_start_matches('v'))
}

const fn pnpm_command() -> &'static str {
    if cfg!(windows) { "pnpm.cmd" } else { "pnpm" }
}

#[cfg(test)]
mod tests {
    use super::version_matches;

    #[test]
    fn compares_exact_version_tokens() {
        assert!(version_matches("rustc 1.97.1 (abc)", "1.97.1"));
        assert!(version_matches("v25.2.1", "25.2.1"));
        assert!(!version_matches("rustc 1.97.10 (abc)", "1.97.1"));
    }
}
