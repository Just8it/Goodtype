//! P1–P9 preset hardening regressions for the shared compiler boundary.
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use typst_kit::downloader::Downloader;
use typst_kit::packages::{FsPackages, SystemPackages, UniversePackages};

use super::*;
use crate::page_source::PresetForm;

const FIXTURE_ARCHIVE: &[u8] =
    include_bytes!("../../../fixtures/typst/packages/goodtype-fixture-0.1.0.tar.gz");
const PACKAGE_PRESET: &str = "#import \"@preview/goodtype-fixture:0.1.0\": shade\n\
#let preset(body, rhythm: 16pt) = {\n  show heading: set text(fill: shade)\n  body\n}\n";

/// Serves the fixture archive and counts every request, so a test proves exactly how often the
/// network would have been reached.
struct FixtureDownloader(Arc<AtomicUsize>);

impl Downloader for FixtureDownloader {
    fn stream(
        &self,
        _key: &dyn std::any::Any,
        url: &str,
    ) -> std::io::Result<(Option<usize>, Box<dyn Read>)> {
        self.0.fetch_add(1, Ordering::SeqCst);
        if url.ends_with("/preview/goodtype-fixture-0.1.0.tar.gz") {
            Ok((Some(FIXTURE_ARCHIVE.len()), Box::new(FIXTURE_ARCHIVE)))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no fixture",
            ))
        }
    }
}

/// An isolated, initially empty package cache with a counting fixture registry.
fn isolated_packages() -> (tempfile::TempDir, &'static SystemPackages, Arc<AtomicUsize>) {
    let cache = tempfile::tempdir().unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let packages = Box::leak(Box::new(SystemPackages::from_parts(
        None,
        Some(FsPackages::new(cache.path())),
        UniversePackages::with_url(
            FixtureDownloader(requests.clone()),
            "https://fixture.invalid",
        ),
    )));
    (cache, packages, requests)
}

/// The same isolated cache, read without any registry: the "downloads off" policy.
fn offline_packages(cache: &Path) -> &'static SystemPackages {
    Box::leak(Box::new(SystemPackages::from_parts(
        None,
        Some(FsPackages::new(cache)),
        UniversePackages::with_url(DeniedDownloader, "https://denied.invalid"),
    )))
}

struct DeniedDownloader;

impl Downloader for DeniedDownloader {
    fn stream(
        &self,
        _key: &dyn std::any::Any,
        _url: &str,
    ) -> std::io::Result<(Option<usize>, Box<dyn Read>)> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "downloads off",
        ))
    }
}

fn notebook(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = root.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    root
}

const STARTER_LIKE: &str = "// Name: Template 1\n#set text(font: \"New Computer Modern\")\n#set par(justify: false)\n#show heading.where(level: 1): set text(weight: \"bold\")\n";

/// P3/P5b: both forms validate through the same composition, including an editor-style starter
/// (a preamble), an alias and a re-export; a broken draft is diagnosed, not accepted.
#[test]
fn function_preamble_starter_alias_and_reexport_presets_validate() {
    let root = notebook(&[
        (
            "styles/function.typ",
            "#let preset(body, rhythm: 16pt) = { set par(justify: true); body }",
        ),
        ("styles/starter.typ", STARTER_LIKE),
        (
            "styles/alias.typ",
            "#let notes(body, rhythm: 16pt) = body\n#let preset = notes",
        ),
        (
            "styles/base.typ",
            "#let preset(body, rhythm: 16pt) = { set text(size: 10pt); body }",
        ),
        ("styles/reexport.typ", "#import \"base.typ\": preset"),
        ("styles/draft.typ", "#set text(size: \n"),
    ]);
    for (file, form) in [
        ("function.typ", PresetForm::Function),
        ("starter.typ", PresetForm::Preamble),
        ("alias.typ", PresetForm::Function),
        ("reexport.typ", PresetForm::Function),
    ] {
        let check = check_notebook_preset(root.path(), file, false).unwrap();
        assert!(check.rendered, "{file}: {:?}", check.diagnostics);
        assert_eq!(check.form, Some(form), "{file}");
        assert!(check.svg.is_some());
    }
    let draft = check_notebook_preset(root.path(), "draft.typ", false).unwrap();
    assert!(!draft.rendered);
    assert!(draft.error().is_some());
    // The error inside the inlined preamble points at the preset file, with a line.
    let error = draft
        .diagnostics
        .iter()
        .find(|d| d.severity == DiagnosticSeverity::Error)
        .unwrap();
    assert_eq!(
        error.file.as_deref(),
        Some("/styles/draft.typ"),
        "{error:?}"
    );
    assert_eq!(error.line, Some(1));
    // A candidate import is checked as a single file in the same way.
    assert!(
        check_preset_candidate(STARTER_LIKE, false)
            .unwrap()
            .rendered
    );
    assert!(
        !check_preset_candidate("#let preset(body) = ", false)
            .unwrap()
            .rendered
    );
}

/// P3 (wave 3 finding): a preamble's `#set text(fill: …)` wins over Goodtype's own Page text
/// prelude, as a function preset's `#show` rule already did.
#[test]
fn a_preamble_presets_text_styling_wins_over_the_page_text_prelude() {
    let root = notebook(&[
        (
            "styles/red.typ",
            "#set text(fill: rgb(\"#c81e1e\"), size: 14pt)",
        ),
        ("styles/plain.typ", "#set par(justify: false)"),
    ]);
    let page = preset_sample_page();
    let compile = |file: &str| {
        compile_page_text(
            root.path(),
            &page,
            &CompileRequest {
                source: format!("#import \"/styles/{file}\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n\nHello"),
                width_pt: export::page_text_width(&page),
                generation: 1,
                allow_remote_packages: false,
            },
        )
        .unwrap()
    };
    let red = compile("red.typ");
    assert!(
        red.svg
            .as_deref()
            .unwrap()
            .to_ascii_lowercase()
            .contains("#c81e1e"),
        "{:?}",
        red.diagnostics
    );
    let plain = compile("plain.typ");
    assert!(
        !plain
            .svg
            .as_deref()
            .unwrap()
            .to_ascii_lowercase()
            .contains("#c81e1e")
    );
    assert!(
        red.height_pt.unwrap() > plain.height_pt.unwrap(),
        "the preset's 14pt applies"
    );
}

/// P4: ordinary validation follows the package policy on a cold cache; nothing real is fetched.
#[test]
fn cold_cache_package_presets_follow_the_download_policy() {
    let (cache, allowed, requests) = isolated_packages();
    let denied = offline_packages(cache.path());
    let refused = check_candidate_with(PACKAGE_PRESET, false, denied).unwrap();
    assert!(!refused.rendered);
    assert!(refused.error().is_some());
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    let fetched = check_candidate_with(PACKAGE_PRESET, true, allowed).unwrap();
    assert!(fetched.rendered, "{:?}", fetched.diagnostics);
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    // Now cached: the same preset passes with downloads off, without another request.
    let cached = check_candidate_with(PACKAGE_PRESET, false, denied).unwrap();
    assert!(cached.rendered, "{:?}", cached.diagnostics);
    assert_eq!(requests.load(Ordering::SeqCst), 1);
}

/// P4 helper boundary: granted checks read only granted bytes and cached packages. A denied
/// project file on disk and an uncached package never reach disk or network, even when the
/// loader handed in could download.
#[test]
fn granted_checks_never_fall_back_to_disk_or_network() {
    let root = notebook(&[("styles/secret.typ", "#let secret = \"SECRET_UNSHARED\"")]);
    let (_cache, downloading, requests) = isolated_packages();
    let files = GrantedFiles::from([(
        "/styles/a.typ".to_owned(),
        b"#import \"secret.typ\": secret\n#let preset(body, rhythm: 16pt) = [#secret #body]"
            .to_vec(),
    )]);
    let check = check_preset(root.path(), "a.typ", Some(&files), true, downloading).unwrap();
    assert!(!check.rendered);
    assert!(format!("{:?}", check.diagnostics).contains("not shared: /styles/secret.typ"));
    assert!(!format!("{:?}", check.diagnostics).contains("SECRET_UNSHARED"));
    let files = GrantedFiles::from([(
        "/styles/a.typ".to_owned(),
        PACKAGE_PRESET.as_bytes().to_vec(),
    )]);
    let check = check_preset(root.path(), "a.typ", Some(&files), true, downloading).unwrap();
    assert!(!check.rendered);
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    assert!(!check_preset_granted(root.path(), "missing.typ", &files).is_ok_and(|c| c.rendered));
}

/// P6: relative imports and includes in a preamble preset resolve beside the preset file, both
/// from disk and from a granted map, through nested importers.
#[test]
fn nested_relative_dependencies_resolve_from_each_importer_on_disk_and_granted() {
    let preset =
        "#import \"parts/defs.typ\": unit\n#include \"parts/intro.typ\"\n#let speed = [3 #unit]";
    let defs = "#import \"names.typ\": metre\n#let unit = metre";
    let names = "#let metre = \"m/s\"";
    let intro = "#include \"../note.typ\"";
    let note = "Nested material";
    let root = notebook(&[
        ("styles/a.typ", preset),
        ("styles/parts/defs.typ", defs),
        ("styles/parts/names.typ", names),
        ("styles/parts/intro.typ", intro),
        ("styles/note.typ", note),
        // Decoys at the notebook root: resolving from there would silently pick these up.
        ("parts/defs.typ", "#let unit = \"WRONG\""),
    ]);
    let check = check_notebook_preset(root.path(), "a.typ", false).unwrap();
    assert!(check.rendered, "{:?}", check.diagnostics);
    assert_eq!(check.form, Some(PresetForm::Preamble));
    let files: GrantedFiles = [
        ("/styles/a.typ", preset),
        ("/styles/parts/defs.typ", defs),
        ("/styles/parts/names.typ", names),
        ("/styles/parts/intro.typ", intro),
        ("/styles/note.typ", note),
    ]
    .into_iter()
    .map(|(path, text)| (path.to_owned(), text.as_bytes().to_vec()))
    .collect();
    let empty = tempfile::tempdir().unwrap();
    let granted = check_preset_granted(empty.path(), "a.typ", &files).unwrap();
    assert!(granted.rendered, "{:?}", granted.diagnostics);
    let page = preset_sample_page();
    let body =
        "#import \"/styles/a.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n\n#speed";
    let preview = compile_page_text(
        root.path(),
        &page,
        &CompileRequest {
            source: body.into(),
            width_pt: export::page_text_width(&page),
            generation: 1,
            allow_remote_packages: false,
        },
    )
    .unwrap();
    assert!(preview.svg.is_some(), "{:?}", preview.diagnostics);
    assert!(!preview.svg.unwrap().contains("WRONG"));
}

/// P8: a missing font is a warning that survives a successful check.
#[test]
fn a_missing_font_warning_is_retained_on_a_rendered_preset() {
    let check =
        check_preset_candidate("#set text(font: \"Goodtype Missing Font\")", false).unwrap();
    assert!(check.rendered);
    assert!(
        check
            .diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Warning
                && d.message.to_lowercase().contains("goodtype missing font")
                && d.file.as_deref() == Some("/styles/preset.typ")),
        "{:?}",
        check.diagnostics
    );
    let json = serde_json::to_value(&check).unwrap();
    assert_eq!(json["form"], "preamble");
    assert_eq!(json["sampleRevision"], PRESET_SAMPLE_REVISION);
    assert_eq!(json["diagnostics"][0]["severity"], "warning");
    assert!(json.get("svg").is_none());
}

/// P9: one deterministic sample with every representative element, pinned to its revision.
#[test]
fn the_representative_sample_is_shared_deterministic_and_pinned() {
    for element in [
        "= Damped",
        "\n== ",
        "$ ",
        "$omega_0",
        "thin \"kg\"",
        "\n- ",
        "#table(",
        "curve(",
    ] {
        assert!(PRESET_SAMPLE.contains(element), "{element}");
    }
    assert!(
        !PRESET_SAMPLE.contains("@preview"),
        "the sample uses only the standard library"
    );
    let first = check_preset_candidate("#set par(justify: true)", false).unwrap();
    let second = check_preset_candidate("#set par(justify: true)", false).unwrap();
    assert!(first.rendered, "{:?}", first.diagnostics);
    assert!(first.diagnostics.is_empty(), "{:?}", first.diagnostics);
    assert_eq!(first.svg, second.svg);
    assert!(first.height_pt.unwrap() > 200.0);
    // Changing the sample must come with a new revision; update both together.
    let digest = PRESET_SAMPLE
        .bytes()
        .filter(|byte| *byte != b'\r')
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
        });
    assert_eq!(
        (PRESET_SAMPLE_REVISION, digest),
        ("goodtype-preset-sample-1", SAMPLE_DIGEST),
        "sample text changed: bump PRESET_SAMPLE_REVISION and SAMPLE_DIGEST"
    );
}

const SAMPLE_DIGEST: u64 = 17418189714946132605;

#[test]
fn reviewed_package_capture_replays_exact_consumed_bytes_and_denies_new_reads() {
    let (cache, downloading, requests) = isolated_packages();
    assert!(
        check_candidate_with(PACKAGE_PRESET, true, downloading)
            .unwrap()
            .rendered
    );
    let offline = offline_packages(cache.path());
    let root = notebook(&[]);
    let files = GrantedFiles::from([("/styles/a.typ".into(), PACKAGE_PRESET.as_bytes().to_vec())]);
    let (first, captured) = with_preset_packages(None, || {
        check_preset(root.path(), "a.typ", Some(&files), false, offline)
    })
    .unwrap();
    assert!(first.rendered);
    assert!(!captured.0.is_empty());
    for (path, _) in captured.0.values() {
        std::fs::write(path, b"corrupted cached package").unwrap();
    }
    let (replayed, _) = with_preset_packages(Some(&captured), || {
        check_preset(root.path(), "a.typ", Some(&files), false, offline)
    })
    .unwrap();
    assert!(replayed.rendered, "{:?}", replayed.diagnostics);
    let (denied, _) = with_preset_packages(Some(&PresetPackageBytes::default()), || {
        check_preset(root.path(), "a.typ", Some(&files), false, offline)
    })
    .unwrap();
    assert!(!denied.rendered);
    assert_eq!(requests.load(Ordering::SeqCst), 1);
}
