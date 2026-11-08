//! Compile the actual bundled skill recipes, never a second copy of their source.
use super::*;
use std::io::Read;
use typst_kit::downloader::Downloader;
use typst_kit::packages::{FsPackages, SystemPackages, UniversePackages};

struct RecipeFixtures;
impl Downloader for RecipeFixtures {
    fn stream(
        &self,
        _key: &dyn std::any::Any,
        url: &str,
    ) -> std::io::Result<(Option<usize>, Box<dyn Read>)> {
        let bytes: &'static [u8] = match url.strip_prefix("https://fixture.invalid/preview/") {
            Some("lilaq-0.6.0.tar.gz") => {
                include_bytes!("../../../fixtures/typst/packages/lilaq-0.6.0.tar.gz")
            }
            Some("elembic-1.1.1.tar.gz") => {
                include_bytes!("../../../fixtures/typst/packages/elembic-1.1.1.tar.gz")
            }
            Some("zero-0.6.1.tar.gz") => {
                include_bytes!("../../../fixtures/typst/packages/zero-0.6.1.tar.gz")
            }
            Some("tiptoe-0.4.0.tar.gz") => {
                include_bytes!("../../../fixtures/typst/packages/tiptoe-0.4.0.tar.gz")
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "unpinned recipe package",
                ));
            }
        };
        Ok((Some(bytes.len()), Box::new(bytes)))
    }
}

#[test]
fn bundled_skill_recipes_compile_offline() {
    let skill = include_str!("../../goodtype-mcp/skill/goodtype/SKILL.md").replace("\r\n", "\n");
    let cases = [
        "Function plot",
        "Table with units",
        "Annotated multistep",
        "Dimensional check",
        "Concise explanation",
        "Practice with separate",
        "Page summary preserving",
    ];
    let root = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let packages = Box::leak(Box::new(SystemPackages::from_parts(
        None,
        Some(FsPackages::new(cache.path())),
        UniversePackages::with_url(RecipeFixtures, "https://fixture.invalid"),
    )));
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/w6/recipes");
    std::fs::create_dir_all(&output).unwrap();
    for (index, heading) in cases.iter().enumerate() {
        let section = skill
            .lines()
            .find(|s| s.trim_start_matches('#').trim_start().starts_with(heading))
            .unwrap_or_else(|| panic!("missing skill recipe: {heading}"));
        let section = &skill[skill.find(section).unwrap()..];
        let source = section
            .split_once("```typst\n")
            .and_then(|(_, s)| s.split_once("\n```"))
            .unwrap_or_else(|| panic!("missing Typst fence: {heading}"))
            .0;
        let result = compile_block_with(
            root.path(),
            &CompileRequest {
                source: source.to_owned(),
                width_pt: 360.0,
                generation: index as u64,
                allow_remote_packages: false,
            },
            None,
            packages,
        )
        .unwrap();
        assert!(
            result.diagnostics.is_empty(),
            "{heading}: {:?}. Recipe dependencies must resolve solely from the pinned test fixtures.",
            result.diagnostics
        );
        let svg = result.svg.expect("recipe must produce SVG");
        assert!(
            svg.contains("<svg") && svg.len() > 100,
            "{heading}: empty SVG"
        );
        assert!(
            result.width_pt.is_some_and(|n| n.is_finite() && n > 0.0),
            "{heading}: width"
        );
        assert!(
            result.height_pt.is_some_and(|n| n.is_finite() && n > 0.0),
            "{heading}: height"
        );
        std::fs::write(output.join(format!("recipe-{}.svg", index + 1)), svg).unwrap();
        println!(
            "{heading}: {:?} x {:?} pt, zero diagnostics",
            result.width_pt, result.height_pt
        );
    }
}
