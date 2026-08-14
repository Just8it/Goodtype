use goodtype_typst::{
    CompileRequest, CompileResult, Completion, Hover, compile_block as compile_typst_block,
};
use serde::Deserialize;

use crate::settings::RemotePackages;
use crate::workspace::{AllowedRoots, ensure_allowed};

const PAGE_TEXT_EDITOR_PRELUDE: &str = "#let goodtype_rhythm = 16pt\n";

fn editor_source(source: String) -> (String, usize) {
    if source.starts_with("#import \"/styles/")
        && source.lines().nth(1) == Some("#show: preset.with(rhythm: goodtype_rhythm)")
    {
        (
            format!("{PAGE_TEXT_EDITOR_PRELUDE}{source}"),
            PAGE_TEXT_EDITOR_PRELUDE.len(),
        )
    } else {
        (source, 0)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileBlockRequest {
    source: String,
    #[serde(default)]
    shared_style: Option<String>,
    width_pt: f64,
    generation: u64,
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
        let source = match request.shared_style {
            Some(style) if !style.is_empty() => format!("{style}\n{}", request.source),
            _ => request.source,
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
        let (source, prefix) = editor_source(source);
        let cursor = cursor + prefix;
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
        let (source, prefix) = editor_source(source);
        let cursor = cursor + prefix;
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
        let (source, prefix) = editor_source(source);
        let mut analysis = tinymist.analyze(&root, &source).unwrap_or_default();
        analysis.highlights.retain_mut(|item| {
            item.from = item.from.saturating_sub(prefix);
            item.to = item.to.saturating_sub(prefix);
            item.to > item.from
        });
        analysis.diagnostics.retain_mut(|item| {
            item.from = item.from.saturating_sub(prefix);
            item.to = item.to.saturating_sub(prefix);
            item.to > item.from
        });
        Ok(analysis)
    })
    .await
    .map_err(|error| error.to_string())?
}
