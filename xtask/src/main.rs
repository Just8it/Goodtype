use std::{env, fs, path::Path, process::Command};

fn main() {
    let result = match env::args().nth(1).as_deref() {
        Some("doctor") => doctor(),
        Some("verify") => verify(),
        _ => Err("usage: cargo xtask <doctor|verify>".to_owned()),
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
    let typst = read_toml_value(&root.join("tools/versions.toml"), "typst");
    let tinymist = read_toml_value(&root.join("tools/versions.toml"), "tinymist");

    println!("Goodtype toolchain doctor");
    let mut required_ok = true;
    required_ok &= check_tool("Rust", "rustc", rust.as_deref(), true);
    required_ok &= check_tool("Cargo", "cargo", None, true);
    required_ok &= check_tool("Node", "node", node.as_deref(), true);
    required_ok &= check_tool("pnpm", pnpm_command(), pnpm.as_deref(), true);
    check_tool("Typst (Phase 0B)", "typst", typst.as_deref(), false);
    check_tool("Tinymist (Phase 2)", "tinymist", tinymist.as_deref(), false);

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
