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

fn editor_source(root: &std::path::Path, source: String) -> Result<(String, usize), String> {
    if !goodtype_typst::has_managed_preset(&source) {
        return Ok((source, 0));
    }
    let prepared = goodtype_typst::page_source::prepare(root, &source)?;
    let (prefix, body) = if let Some(template) = prepared.preamble {
        let prefix = format!(
            "{PAGE_TEXT_EDITOR_PRELUDE}#let goodtype_page = (width: auto, height: auto, margin: 0pt)\n{template}\n"
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
    source: String,
    #[serde(default)]
    shared_style: Option<String>,
    width_pt: f64,
    generation: u64,
    #[serde(default)]
    page_text: Option<PageTextRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageTextRequest {
    geometry: goodtype_core::PageGeometry,
    background: goodtype_core::PageBackground,
    snap_blocks_to_grid: bool,
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
        let source = if let Some(page_text) = request.page_text {
            let page = goodtype_typst::export::ExportPage {
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
            goodtype_typst::export::prepared_page_text(&root, &page, &request.source)?
        } else {
            match request.shared_style {
                Some(style) if !style.is_empty() => format!("{style}\n{}", request.source),
                _ => request.source,
            }
        };
        compile_typst_block(
            &root,
            &CompileRequest {
                source,
                width_pt: request.width_pt,
                generation: request.generation,
                allow_remote_packages,
            },
        )
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
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
        let (source, prefix) = editor_source(&root, source)?;
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
        let (source, prefix) = editor_source(&root, source)?;
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
    tinymist: tauri::State<'_, crate::tinymist::Tinymist>,
    root: String,
    source: String,
) -> Result<crate::tinymist::Analysis, String> {
    let root = ensure_allowed(&roots, &root)?;
    let tinymist = tinymist.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (source, prefix) = editor_source(&root, source)?;
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
    fn preamble_analysis_keeps_original_offsets_and_definitions() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("styles")).unwrap();
        std::fs::write(
            root.path().join("styles/a.typ"),
            "#let custom(body) = strong(body)",
        )
        .unwrap();
        let source = "#import \"/styles/a.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n\n#custom[Ω]";
        let (prepared, prefix) = editor_source(root.path(), source.into()).unwrap();
        let original = source.find("#custom").unwrap();
        assert_eq!(prepared.find("#custom[Ω]"), Some(original + prefix));
        assert_eq!(prepared.len(), prefix + source.len());
        assert!(prepared.contains("#let custom(body) = strong(body)"));
        assert!(!prepared.contains("#import \"/styles/a.typ\": preset"));
    }
}
