//! Restricted Typst compiler boundary.
//!
//! The Typst compiler runs in-process via [`embedded`], with a Goodtype-owned
//! `World`: embedded fonts, notebook-root-scoped file access, and a fixed clock. Remote
//! packages are gated separately (see the `World` implementation).

pub mod export;

mod embedded;

use std::path::{Path, PathBuf};

const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_WIDTH_PT: f64 = 10_000.0;

#[derive(Debug, Clone, PartialEq)]
pub struct CompileRequest {
    pub source: String,
    pub width_pt: f64,
    pub generation: u64,
    /// Allow downloading a Typst Universe package on a cache miss. Cached packages
    /// resolve either way; this only governs whether a request may be made.
    pub allow_remote_packages: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompileResult {
    pub generation: u64,
    pub svg: Option<String>,
    pub width_pt: Option<f64>,
    pub height_pt: Option<f64>,
    pub diagnostics: Vec<Diagnostic>,
}

/// One completion candidate at a caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// What is being completed: `function`, `parameter`, `symbol`, `package`, …
    pub kind: &'static str,
    /// For a math symbol, the glyph it renders as (`∑`), so it can be shown beside the name.
    pub symbol: Option<String>,
    pub label: String,
    /// Replacement text when it differs from the label; may contain `${…}` placeholders.
    pub apply: Option<String>,
    pub detail: Option<String>,
    /// Byte offset where the replacement starts, so the caller replaces the partial word.
    pub offset: usize,
}

/// Complete at `cursor` (a byte offset) inside a block's source.
///
/// `explicit` marks a completion the user asked for (Ctrl+Space) rather than one triggered by
/// typing; Typst offers broader candidates in that case.
pub fn complete(
    notebook_root: &Path,
    source: String,
    cursor: usize,
    explicit: bool,
    allow_remote_packages: bool,
) -> Result<Vec<Completion>, CompileError> {
    let root = notebook_root
        .canonicalize()
        .map_err(|_| CompileError::InvalidRoot(notebook_root.to_path_buf()))?;
    if !root.is_dir() {
        return Err(CompileError::InvalidRoot(root));
    }
    if source.len() > MAX_SOURCE_BYTES {
        return Err(CompileError::SourceTooLarge(source.len()));
    }
    Ok(embedded::complete(
        &root,
        source,
        cursor,
        explicit,
        allow_remote_packages,
    ))
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

    let wrapper = format!(
        "#set page(width: {}pt, height: auto, margin: 0pt)\n{}",
        request.width_pt, request.source
    );

    Ok(embedded::compile_block(
        &root,
        request.generation,
        wrapper,
        request.allow_remote_packages,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_valid_source_and_reports_invalid_source() {
        let root = tempfile::tempdir().unwrap();

        let valid = compile_block(
            root.path(),
            &CompileRequest {
                source: include_str!("../../../fixtures/typst/valid.typ").to_owned(),
                width_pt: 240.0,
                generation: 7,
                allow_remote_packages: false,
            },
        )
        .unwrap();
        assert_eq!(valid.generation, 7);
        assert!(valid.svg.as_deref().is_some_and(|svg| svg.contains("<svg")));
        assert!(valid.width_pt.is_some_and(|width| width > 0.0));
        assert!(valid.height_pt.is_some_and(|height| height > 0.0));
        assert!(
            valid
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != DiagnosticSeverity::Error)
        );

        let invalid = compile_block(
            root.path(),
            &CompileRequest {
                source: include_str!("../../../fixtures/typst/invalid.typ").to_owned(),
                width_pt: 240.0,
                generation: 8,
                allow_remote_packages: false,
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

    /// An uncached package with downloads off must fail as a diagnostic without reaching the
    /// network, so `cargo xtask verify` stays offline.
    #[test]
    fn uncached_package_is_reported_when_downloads_are_off() {
        let root = tempfile::tempdir().unwrap();
        let result = compile_block(
            root.path(),
            &CompileRequest {
                // A namespace/name that cannot exist locally, so this never hits a warm cache.
                source: "#import \"@preview/goodtype-does-not-exist:9.9.9\": *".to_owned(),
                width_pt: 240.0,
                generation: 1,
                allow_remote_packages: false,
            },
        )
        .unwrap();

        assert!(result.svg.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        );
    }

    /// Notebook files stay reachable only through the notebook root: a package path must not
    /// resolve into the notebook, and a notebook path must not escape it.
    #[test]
    fn source_outside_the_notebook_root_is_rejected() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.typ"), "= Secret").unwrap();
        let root = tempfile::tempdir().unwrap();

        let result = compile_block(
            root.path(),
            &CompileRequest {
                source: "#include \"../secret.typ\"".to_owned(),
                width_pt: 240.0,
                generation: 1,
                allow_remote_packages: false,
            },
        )
        .unwrap();

        assert!(result.svg.is_none());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        );
    }

    /// Completion must come from the compiler, not a word list: a caret after `#` offers real
    /// library functions, and a math symbol resolves to its glyph.
    #[test]
    fn completes_library_functions_and_math_symbols() {
        let root = tempfile::tempdir().unwrap();

        let source = "#".to_owned();
        let functions = complete(root.path(), source.clone(), source.len(), false, false).unwrap();
        assert!(
            functions.iter().any(|item| item.label == "image"),
            "expected a compiler-derived function completion, got {:?}",
            functions.iter().map(|item| &item.label).collect::<Vec<_>>()
        );

        let math = "$ sum".to_owned();
        let symbols = complete(root.path(), math.clone(), math.len(), false, false).unwrap();
        let sum = symbols.iter().find(|item| item.label == "sum");
        assert!(sum.is_some(), "expected the `sum` math symbol");
        assert_eq!(sum.and_then(|item| item.symbol.as_deref()), Some("∑"));
    }

    /// A caret at an impossible offset must return nothing rather than panic the analyzer.
    #[test]
    fn completion_tolerates_an_out_of_range_cursor() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            complete(root.path(), "= Title".to_owned(), 999, false, false)
                .unwrap()
                .is_empty()
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
                    allow_remote_packages: false,
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
                    allow_remote_packages: false,
                }
            ),
            Err(CompileError::InvalidWidth(_))
        ));
    }
}
