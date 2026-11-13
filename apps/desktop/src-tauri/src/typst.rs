use goodtype_typst::{
    CompileRequest, CompileResult, Completion, Hover, compile_block as compile_typst_block,
};
use serde::Deserialize;

use crate::settings::RemotePackages;
use crate::workspace::{AllowedRoots, ensure_allowed};

const PAGE_TEXT_EDITOR_PRELUDE: &str = "#let goodtype_rhythm = 16pt\n";

#[tauri::command]
pub async fn typst_font_families() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(goodtype_typst::font_families)
        .await
        .map_err(|error| error.to_string())
}

fn editor_source(
    root: &std::path::Path,
    source: String,
    allow_remote_packages: bool,
) -> Result<(String, usize), String> {
    if !goodtype_typst::has_managed_preset(&source) {
        return Ok((source, 0));
    }
    // The same preparation as preview and export, so a preamble's relative imports resolve from
    // the preset file here too.
    let prepared = goodtype_typst::page_source::prepare(root, &source, allow_remote_packages)?;
    let (prefix, body) = if let Some(template) = prepared.preamble {
        let prefix = format!(
            "{PAGE_TEXT_EDITOR_PRELUDE}#let goodtype_page = (width: auto, height: auto, margin: 0pt)\n{}\n",
            template.text
        );
        // Blank the owned header without changing any original byte offsets.
        let padding: String = source[..prepared.header_bytes]
            .chars()
            .map(|c| if c == '\n' || c == '\r' { c } else { ' ' })
            .collect();
        (prefix, format!("{padding}{}", prepared.body))
    } else {
        (PAGE_TEXT_EDITOR_PRELUDE.to_owned(), source)
    };
    let offset = prefix.len();
    Ok((format!("{prefix}{body}"), offset))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileBlockRequest {
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) shared_style: Option<String>,
    pub(crate) width_pt: f64,
    pub(crate) generation: u64,
    #[serde(default)]
    pub(crate) page_text: Option<PageTextRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageTextRequest {
    pub(crate) geometry: goodtype_core::PageGeometry,
    pub(crate) background: goodtype_core::PageBackground,
    pub(crate) snap_blocks_to_grid: bool,
}

#[tauri::command]
pub async fn compile_typst(
    roots: tauri::State<'_, AllowedRoots>,
    packages: tauri::State<'_, RemotePackages>,
    root: String,
    request: CompileBlockRequest,
) -> Result<CompileResult, String> {
    let root = ensure_allowed(&roots, &root)?;
    // The package policy is Rust's, read from settings — never supplied by the frontend.
    let allow_remote_packages = packages.allowed();
    tauri::async_runtime::spawn_blocking(move || {
        compile_for_target(&root, request, allow_remote_packages)
    })
    .await
    .map_err(|error| error.to_string())?
}

pub(crate) fn compile_for_target(
    root: &std::path::Path,
    request: CompileBlockRequest,
    allow_remote_packages: bool,
) -> Result<CompileResult, String> {
    if let Some(page_text) = request.page_text {
        let page = goodtype_typst::export::ExportPage {
            calculation: None,
            width_pt: page_text.geometry.width_pt,
            height_pt: page_text.geometry.height_pt,
            background: page_text.background,
            page_text_baseline_grid: page_text.snap_blocks_to_grid,
            shared_style: request.shared_style,
            page_typst: Some(request.source.clone()),
            blocks: vec![],
            strokes: vec![],
            shapes: vec![],
            images: vec![],
        };
        return goodtype_typst::compile_page_text(
            root,
            &page,
            &CompileRequest {
                source: request.source,
                width_pt: request.width_pt,
                generation: request.generation,
                allow_remote_packages,
            },
        );
    }
    let prefix = request
        .shared_style
        .as_ref()
        .filter(|s| !s.is_empty())
        .map_or(0, |s| s.len() + 1);
    let source = match request.shared_style {
        Some(style) if !style.is_empty() => format!("{style}\n{}", request.source),
        _ => request.source.clone(),
    };
    let mut result = compile_typst_block(
        root,
        &CompileRequest {
            source,
            width_pt: request.width_pt,
            generation: request.generation,
            allow_remote_packages,
        },
    )
    .map_err(|error| error.to_string())?;
    goodtype_typst::remap_diagnostics(
        &mut result,
        &request.source,
        prefix,
        0,
        request.source.len(),
    );
    Ok(result)
}

/// Complete at a caret inside a Typst block. Analysis uses the same root-scoped
/// world as compilation, so results cannot reach outside the notebook or a resolved package.
#[tauri::command]
pub async fn complete_typst(
    roots: tauri::State<'_, AllowedRoots>,
    packages: tauri::State<'_, RemotePackages>,
    tinymist: tauri::State<'_, crate::tinymist::Tinymist>,
    root: String,
    source: String,
    cursor: usize,
    explicit: bool,
) -> Result<Vec<Completion>, String> {
    let root = ensure_allowed(&roots, &root)?;
    let allow_remote_packages = packages.allowed();
    let tinymist = tinymist.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (source, prefix) = editor_source(&root, source, allow_remote_packages)?;
        let cursor = cursor
            .checked_add(prefix)
            .ok_or("Typst cursor offset overflow")?;
        if let Ok(mut items) = tinymist.complete(&root, &source, cursor, explicit) {
            for item in &mut items {
                item.offset = item.offset.saturating_sub(prefix);
            }
            return Ok(items);
        }
        let mut items =
            goodtype_typst::complete(&root, source, cursor, explicit, allow_remote_packages)
                .map_err(|error| error.to_string())?;
        for item in &mut items {
            item.offset = item.offset.saturating_sub(prefix);
        }
        Ok(items)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn hover_typst(
    roots: tauri::State<'_, AllowedRoots>,
    packages: tauri::State<'_, RemotePackages>,
    tinymist: tauri::State<'_, crate::tinymist::Tinymist>,
    root: String,
    source: String,
    cursor: usize,
) -> Result<Option<Hover>, String> {
    let root = ensure_allowed(&roots, &root)?;
    let allow_remote_packages = packages.allowed();
    let tinymist = tinymist.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (source, prefix) = editor_source(&root, source, allow_remote_packages)?;
        let cursor = cursor
            .checked_add(prefix)
            .ok_or("Typst cursor offset overflow")?;
        if let Ok(result) = tinymist.hover(&root, &source, cursor) {
            return Ok(result);
        }
        goodtype_typst::hover(&root, source, cursor, allow_remote_packages)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn format_typst(
    roots: tauri::State<'_, AllowedRoots>,
    tinymist: tauri::State<'_, crate::tinymist::Tinymist>,
    root: String,
    source: String,
) -> Result<String, String> {
    let root = ensure_allowed(&roots, &root)?;
    let tinymist = tinymist.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Ok(formatted) = tinymist.format(&root, &source) {
            return Ok(formatted);
        }
        goodtype_typst::format_source(source).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn analyze_typst(
    roots: tauri::State<'_, AllowedRoots>,
    packages: tauri::State<'_, RemotePackages>,
    tinymist: tauri::State<'_, crate::tinymist::Tinymist>,
    root: String,
    source: String,
) -> Result<crate::tinymist::Analysis, String> {
    let root = ensure_allowed(&roots, &root)?;
    let allow_remote_packages = packages.allowed();
    let tinymist = tinymist.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (source, prefix) = editor_source(&root, source, allow_remote_packages)?;
        // An unavailable analyzer is reported as such rather than as an empty answer. There is
        // no in-process fallback for semantic tokens, so silently returning nothing left the
        // editor looking broken on any platform without the sidecar.
        let Ok(mut analysis) = tinymist.analyze(&root, &source) else {
            return Ok(crate::tinymist::Analysis::unavailable());
        };
        analysis
            .highlights
            .retain_mut(|item| shift_range(&mut item.from, &mut item.to, prefix));
        analysis
            .diagnostics
            .retain_mut(|item| shift_range(&mut item.from, &mut item.to, prefix));
        Ok(analysis)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Move a range from the analysed source back onto the canonical source the writer sees.
///
/// The generated prelude is not part of the document, so anything inside it collapses to an
/// empty range and is dropped by the caller's `retain`. Written once because the same shift was
/// applied at four call sites, twice for offsets and twice for ranges.
fn shift_range(from: &mut usize, to: &mut usize, prefix: usize) -> bool {
    *from = from.saturating_sub(prefix);
    *to = to.saturating_sub(prefix);
    *to > *from
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_style_diagnostics_do_not_claim_caller_spans() {
        let root = tempfile::tempdir().unwrap();
        let result = compile_for_target(
            root.path(),
            CompileBlockRequest {
                source: "Hello".into(),
                shared_style: Some("#unknown_style".into()),
                width_pt: 300.0,
                generation: 1,
                page_text: None,
            },
            false,
        )
        .unwrap();
        assert!(result.svg.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.line.is_none() && d.snippet.is_none())
        );
    }

    #[test]
    fn compile_service_maps_shared_style_and_page_text_spans() {
        let root = tempfile::tempdir().unwrap();
        for style in [None, Some("#set text(size: 12pt)\n#let foo = 1".into())] {
            let r = compile_for_target(
                root.path(),
                CompileBlockRequest {
                    source: "Hello\n\u{e4} #unknown_name".into(),
                    shared_style: style,
                    width_pt: 300.0,
                    generation: 9,
                    page_text: None,
                },
                false,
            )
            .unwrap();
            let d = r
                .diagnostics
                .iter()
                .find(|d| d.message.contains("unknown_name"))
                .unwrap();
            assert_eq!((d.line, d.column), (Some(2), Some(4)));
        }
    }

    #[test]
    fn compile_diagnostics_map_editor_prelude() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("styles")).unwrap();
        std::fs::write(root.path().join("styles/a.typ"), "#set text(fill: blue)").unwrap();
        let original = "#import \"/styles/a.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n\n\u{e4} #unknown_name";
        let (source, prefix) = editor_source(root.path(), original.into(), false).unwrap();
        let mut result = compile_typst_block(
            root.path(),
            &CompileRequest {
                source,
                width_pt: 300.0,
                generation: 0,
                allow_remote_packages: false,
            },
        )
        .unwrap();
        goodtype_typst::remap_diagnostics(&mut result, original, prefix, 0, original.len());
        let d = result
            .diagnostics
            .iter()
            .find(|d| d.message.contains("unknown_name"))
            .unwrap();
        assert_eq!((d.line, d.column), (Some(4), Some(4)));
    }

    #[test]
    fn preamble_analysis_keeps_original_offsets_and_definitions() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("styles")).unwrap();
        std::fs::write(
            root.path().join("styles/a.typ"),
            "#let custom(body) = strong(body)",
        )
        .unwrap();
        let source = "#import \"/styles/a.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n\n#custom[Ω]";
        let (prepared, prefix) = editor_source(root.path(), source.into(), false).unwrap();
        let original = source.find("#custom").unwrap();
        assert_eq!(prepared.find("#custom[Ω]"), Some(original + prefix));
        assert_eq!(prepared.len(), prefix + source.len());
        assert!(prepared.contains("#let custom(body) = strong(body)"));
        assert!(!prepared.contains("#import \"/styles/a.typ\": preset"));
    }

    /// P6 in the editor: completion and analysis see a preamble's relative import resolved from
    /// the preset file, the same text preview and export compile.
    #[test]
    fn editor_preamble_relative_imports_resolve_from_the_preset() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("styles/parts")).unwrap();
        std::fs::write(
            root.path().join("styles/a.typ"),
            "#import \"parts/defs.typ\": custom",
        )
        .unwrap();
        std::fs::write(
            root.path().join("styles/parts/defs.typ"),
            "#let custom(body) = strong(body)",
        )
        .unwrap();
        let source = "#import \"/styles/a.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n\n#custom[Ω]";
        let (prepared, prefix) = editor_source(root.path(), source.into(), false).unwrap();
        assert!(prepared.contains("#import \"/styles/parts/defs.typ\": custom"));
        let mut result = compile_typst_block(
            root.path(),
            &CompileRequest {
                source: prepared,
                width_pt: 300.0,
                generation: 0,
                allow_remote_packages: false,
            },
        )
        .unwrap();
        goodtype_typst::remap_diagnostics(&mut result, source, prefix, 0, source.len());
        assert!(result.svg.is_some(), "{:?}", result.diagnostics);
    }
}
