use goodtype_typst::{CompileRequest, DiagnosticSeverity, compile_block as compile_typst_block};
use serde::{Deserialize, Serialize};

use crate::settings::RemotePackages;
use crate::workspace::{AllowedRoots, ensure_allowed};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileBlockRequest {
    source: String,
    width_pt: f64,
    generation: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileBlockResult {
    generation: u64,
    svg: Option<String>,
    width_pt: Option<f64>,
    height_pt: Option<f64>,
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
        compile_typst_block(
            &root,
            &CompileRequest {
                source: request.source,
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
