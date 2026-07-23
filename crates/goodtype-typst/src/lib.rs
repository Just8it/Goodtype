//! Restricted Typst process boundary for the Phase 0B prototype.

pub mod export;

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_WIDTH_PT: f64 = 10_000.0;

#[derive(Debug, Clone, PartialEq)]
pub struct CompileRequest {
    pub source: String,
    pub width_pt: f64,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompileResult {
    pub generation: u64,
    pub svg: Option<String>,
    pub width_pt: Option<f64>,
    pub height_pt: Option<f64>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug)]
pub enum CompileError {
    InvalidRoot(PathBuf),
    InvalidWidth(f64),
    SourceTooLarge(usize),
    Io(std::io::Error),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRoot(path) => {
                write!(
                    formatter,
                    "Typst root is not a directory: {}",
                    path.display()
                )
            }
            Self::InvalidWidth(width) => write!(formatter, "invalid Typst width: {width}"),
            Self::SourceTooLarge(size) => {
                write!(
                    formatter,
                    "Typst source exceeds {MAX_SOURCE_BYTES} bytes: {size}"
                )
            }
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CompileError {}

impl From<std::io::Error> for CompileError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn compile_block(
    notebook_root: &Path,
    request: &CompileRequest,
) -> Result<CompileResult, CompileError> {
    let root = notebook_root
        .canonicalize()
        .map_err(|_| CompileError::InvalidRoot(notebook_root.to_path_buf()))?;
    if !root.is_dir() {
        return Err(CompileError::InvalidRoot(root));
    }
    if !request.width_pt.is_finite() || request.width_pt <= 0.0 || request.width_pt > MAX_WIDTH_PT {
        return Err(CompileError::InvalidWidth(request.width_pt));
    }
    if request.source.len() > MAX_SOURCE_BYTES {
        return Err(CompileError::SourceTooLarge(request.source.len()));
    }

    let workspace = tempfile::Builder::new()
        .prefix(".goodtype-typst-")
        .tempdir_in(&root)?;
    let input = workspace.path().join("block.typ");
    let output = workspace.path().join("block.svg");
    let wrapper = format!(
        "#set page(width: {}pt, height: auto, margin: 0pt)\n{}",
        request.width_pt, request.source
    );
    fs::write(&input, wrapper)?;

    let result = Command::new(typst_compiler())
        .arg("compile")
        .arg("--root")
        .arg(&root)
        .arg("--diagnostic-format")
        .arg("short")
        .arg(&input)
        .arg(&output)
        .output()?;
    let diagnostics = parse_diagnostics(&String::from_utf8_lossy(&result.stderr));

    if !result.status.success() {
        return Ok(CompileResult {
            generation: request.generation,
            svg: None,
            width_pt: None,
            height_pt: None,
            diagnostics,
        });
    }

    let svg = fs::read_to_string(output)?;
    let (width_pt, height_pt) = svg_dimensions_pt(&svg);
    Ok(CompileResult {
        generation: request.generation,
        svg: Some(svg),
        width_pt,
        height_pt,
        diagnostics,
    })
}

pub(crate) fn typst_compiler() -> PathBuf {
    if let Some(path) = env::var_os("GOODTYPE_TYPST_BIN") {
        return PathBuf::from(path);
    }
    let development_cache =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tools/typst.exe");
    if development_cache.is_file() {
        development_cache
    } else {
        PathBuf::from("typst")
    }
}

fn parse_diagnostics(stderr: &str) -> Vec<Diagnostic> {
    stderr
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (severity, message) = if let Some(message) = line
                .strip_prefix("error: ")
                .or_else(|| line.split_once(": error: ").map(|(_, message)| message))
            {
                (DiagnosticSeverity::Error, message)
            } else {
                let message = line
                    .strip_prefix("warning: ")
                    .or_else(|| line.split_once(": warning: ").map(|(_, message)| message))?;
                (DiagnosticSeverity::Warning, message)
            };
            Some(Diagnostic {
                severity,
                message: message.to_owned(),
            })
        })
        .collect()
}

fn svg_dimensions_pt(svg: &str) -> (Option<f64>, Option<f64>) {
    (
        svg_attribute(svg, "width").and_then(svg_length_to_pt),
        svg_attribute(svg, "height").and_then(svg_length_to_pt),
    )
}

fn svg_attribute<'a>(svg: &'a str, name: &str) -> Option<&'a str> {
    let start = svg.find("<svg")?;
    let tag = &svg[start..svg[start..].find('>')? + start];
    let marker = format!("{name}=\"");
    let value = &tag[tag.find(&marker)? + marker.len()..];
    Some(&value[..value.find('"')?])
}

fn svg_length_to_pt(value: &str) -> Option<f64> {
    if let Some(value) = value.strip_suffix("pt") {
        return value.parse().ok();
    }
    if let Some(value) = value.strip_suffix("px") {
        return value.parse::<f64>().ok().map(|pixels| pixels * 0.75);
    }
    value.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiler() -> Option<PathBuf> {
        env::var_os("GOODTYPE_TYPST_BIN").map(PathBuf::from)
    }

    #[test]
    fn compiles_valid_source_and_reports_invalid_source() {
        let Some(compiler) = compiler() else {
            eprintln!("GOODTYPE_TYPST_BIN is not set; skipping Typst process test");
            return;
        };
        assert!(compiler.is_file(), "Typst binary does not exist");

        let root = tempfile::tempdir().unwrap();
        let valid_source = include_str!("../../../fixtures/typst/valid.typ").to_owned();
        let valid = compile_block(
            root.path(),
            &CompileRequest {
                source: valid_source,
                width_pt: 240.0,
                generation: 7,
            },
        )
        .unwrap();
        assert_eq!(valid.generation, 7);
        assert!(valid.svg.as_deref().is_some_and(|svg| svg.contains("<svg")));
        assert!(valid.width_pt.is_some_and(|width| width > 0.0));
        assert!(valid.height_pt.is_some_and(|height| height > 0.0));
        assert!(valid.diagnostics.is_empty());

        let invalid_source = include_str!("../../../fixtures/typst/invalid.typ").to_owned();
        let invalid = compile_block(
            root.path(),
            &CompileRequest {
                source: invalid_source,
                width_pt: 240.0,
                generation: 8,
            },
        )
        .unwrap();
        assert_eq!(invalid.generation, 8);
        assert!(invalid.svg.is_none());
        assert!(
            invalid
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        );
    }

    #[test]
    fn rejects_untrusted_root_and_width() {
        let missing = Path::new("does-not-exist");
        assert!(matches!(
            compile_block(
                missing,
                &CompileRequest {
                    source: String::new(),
                    width_pt: 240.0,
                    generation: 0,
                }
            ),
            Err(CompileError::InvalidRoot(_))
        ));

        let root = tempfile::tempdir().unwrap();
        assert!(matches!(
            compile_block(
                root.path(),
                &CompileRequest {
                    source: String::new(),
                    width_pt: f64::NAN,
                    generation: 0,
                }
            ),
            Err(CompileError::InvalidWidth(_))
        ));
    }
}
