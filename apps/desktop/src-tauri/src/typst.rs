use goodtype_typst::{CompileRequest, DiagnosticSeverity, compile_block as compile_typst_block};
use serde::{Deserialize, Serialize};

use crate::settings::RemotePackages;
use crate::workspace::{AllowedRoots, ensure_allowed};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileBlockRequest {
    source: String,
    #[serde(default)]
    shared_style: Option<String>,
    width_pt: f64,
    generation: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileBlockResult {
    generation: u64,
    svg: Option<String>,
    /// The content's own size; the SVG bleeds `pad_pt` past it on every side.
    width_pt: Option<f64>,
    height_pt: Option<f64>,
    pad_pt: f64,
    diagnostics: Vec<CompileDiagnostic>,
}

#[derive(Serialize)]
pub struct CompileDiagnostic {
    severity: &'static str,
    message: String,
}

#[tauri::command]
pub async fn compile_typst(
    roots: tauri::State<'_, AllowedRoots>,
    packages: tauri::State<'_, RemotePackages>,
    root: String,
    request: CompileBlockRequest,
) -> Result<CompileBlockResult, String> {
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
        .map(|result| CompileBlockResult {
            generation: result.generation,
            svg: result.svg,
            width_pt: result.width_pt,
            height_pt: result.height_pt,
            pad_pt: result.pad_pt,
            diagnostics: result
                .diagnostics
                .into_iter()
                .map(|diagnostic| CompileDiagnostic {
                    severity: match diagnostic.severity {
                        DiagnosticSeverity::Error => "error",
                        DiagnosticSeverity::Warning => "warning",
                    },
                    message: diagnostic.message,
                })
                .collect(),
        })
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    kind: &'static str,
    symbol: Option<String>,
    label: String,
    apply: Option<String>,
    detail: Option<String>,
    offset: usize,
}

#[derive(Serialize)]
pub struct HoverResult {
    value: String,
    code: bool,
}

/// Complete at a caret inside a Typst block. Analysis uses the same root-scoped
/// world as compilation, so results cannot reach outside the notebook or a resolved package.
#[tauri::command]
pub async fn complete_typst(
    roots: tauri::State<'_, AllowedRoots>,
    packages: tauri::State<'_, RemotePackages>,
    root: String,
    source: String,
    cursor: usize,
    explicit: bool,
) -> Result<Vec<CompletionItem>, String> {
    let root = ensure_allowed(&roots, &root)?;
    let allow_remote_packages = packages.allowed();
    tauri::async_runtime::spawn_blocking(move || {
        goodtype_typst::complete(&root, source, cursor, explicit, allow_remote_packages)
            .map(|items| {
                items
                    .into_iter()
                    .map(|item| CompletionItem {
                        kind: item.kind,
                        symbol: item.symbol,
                        label: item.label,
                        apply: item.apply,
                        detail: item.detail,
                        offset: item.offset,
                    })
                    .collect()
            })
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn hover_typst(
    roots: tauri::State<'_, AllowedRoots>,
    packages: tauri::State<'_, RemotePackages>,
    root: String,
    source: String,
    cursor: usize,
) -> Result<Option<HoverResult>, String> {
    let root = ensure_allowed(&roots, &root)?;
    let allow_remote_packages = packages.allowed();
    tauri::async_runtime::spawn_blocking(move || {
        goodtype_typst::hover(&root, source, cursor, allow_remote_packages)
            .map(|result| {
                result.map(|hover| HoverResult {
                    value: hover.value,
                    code: hover.code,
                })
            })
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn format_typst(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
    source: String,
) -> Result<String, String> {
    let _root = ensure_allowed(&roots, &root)?;
    tauri::async_runtime::spawn_blocking(move || {
        goodtype_typst::format_source(source).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}
