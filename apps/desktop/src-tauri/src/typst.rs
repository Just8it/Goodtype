use std::path::Path;

use goodtype_typst::{CompileRequest, DiagnosticSeverity, compile_block as compile_typst_block};
use serde::{Deserialize, Serialize};

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
    root: String,
    request: CompileBlockRequest,
) -> Result<CompileBlockResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        compile_typst_block(
            Path::new(&root),
            &CompileRequest {
                source: request.source,
                width_pt: request.width_pt,
                generation: request.generation,
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
