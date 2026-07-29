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

/// Slack compiled around a preview so it is not clipped by its own page frame.
///
/// A Typst page with `height: auto` fits the content's *layout* box, and a line box runs from cap
/// height to baseline — so descenders drop below the frame, and accents, tall delimiters and
/// inline math rise above it. An SVG's viewBox is that frame, so everything outside it is cut
/// off. The PDF export never had the problem: there a block is placed on a page far larger than
/// itself, and Typst does not clip a block's overflow.
///
/// Compiling into a page this much wider with a margin to match keeps the text measure identical
/// to the export's `block(width:)` — the same line breaks — and gives the overflow somewhere to
/// land. [`CompileResult::width_pt`] and [`CompileResult::height_pt`] stay the size of the
/// content alone, so a caller draws the SVG at its full size offset by `-pad_pt` and the content
/// lands exactly where the export puts it.
pub const PREVIEW_PAD_PT: f64 = 16.0;

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
    /// Width of the content, excluding the [`PREVIEW_PAD_PT`] slack the SVG carries on each side.
    pub width_pt: Option<f64>,
    /// Height of the content, excluding the [`PREVIEW_PAD_PT`] slack the SVG carries on each side.
    pub height_pt: Option<f64>,
    /// How far the SVG extends past the content on every side. Draw it offset by `-pad_pt` to put
    /// the content back where the export places it.
    pub pad_pt: f64,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    pub value: String,
    pub code: bool,
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
    let root = checked_root_and_source(notebook_root, &source)?;
    Ok(embedded::complete(
        &root,
        source,
        cursor,
        explicit,
        allow_remote_packages,
    ))
}

pub fn hover(
    notebook_root: &Path,
    source: String,
    cursor: usize,
    allow_remote_packages: bool,
) -> Result<Option<Hover>, CompileError> {
    let root = checked_root_and_source(notebook_root, &source)?;
    Ok(embedded::hover(
        &root,
        source,
        cursor,
        allow_remote_packages,
    ))
}

pub fn format_source(source: String) -> Result<String, CompileError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(CompileError::SourceTooLarge(source.len()));
    }
    typstyle_core::Typstyle::default()
        .format_text(source)
        .render()
        .map_err(|error| CompileError::Format(error.to_string()))
}

fn checked_root_and_source(notebook_root: &Path, source: &str) -> Result<PathBuf, CompileError> {
    let root = notebook_root
        .canonicalize()
        .map_err(|_| CompileError::InvalidRoot(notebook_root.to_path_buf()))?;
    if !root.is_dir() {
        return Err(CompileError::InvalidRoot(root));
    }
    if source.len() > MAX_SOURCE_BYTES {
        return Err(CompileError::SourceTooLarge(source.len()));
    }
    Ok(root)
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
    Format(String),
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
            Self::Format(error) => write!(formatter, "Typst formatting failed: {error}"),
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

    // `fill: none` rather than the default white: the padding is meant to be see-through, or
    // every block would wear a 16pt white halo over whatever it sits on.
    let wrapper = format!(
        "#set page(width: {}pt, height: auto, margin: {PREVIEW_PAD_PT}pt, fill: none)\n{}",
        request.width_pt + 2.0 * PREVIEW_PAD_PT,
        request.source
    );

    Ok(embedded::compile_block(
        &root,
        request.generation,
        wrapper,
        PREVIEW_PAD_PT,
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

    /// The preview SVG must extend past the content on every side, and the reported size must
    /// still be the content alone.
    ///
    /// Both halves matter. Without the slack, a line box running cap-height to baseline clips
    /// every descender and every accent; without the content size staying exact, the block's
    /// footprint on the page would no longer be what the PDF prints.
    #[test]
    fn a_preview_is_padded_without_changing_the_size_it_reports() {
        let root = tempfile::tempdir().unwrap();
        // Descenders below the baseline, an accent and inline math above the cap height: all of
        // it used to fall outside the frame.
        let result = compile_block(
            root.path(),
            &CompileRequest {
                source: "Jaqjpy Ä $1/2$".to_owned(),
                width_pt: 240.0,
                generation: 1,
                allow_remote_packages: false,
            },
        )
        .unwrap();

        assert_eq!(result.pad_pt, PREVIEW_PAD_PT);
        // The measure is untouched, so the block breaks its lines exactly as the export does.
        assert_eq!(result.width_pt, Some(240.0));
        let height = result.height_pt.unwrap();
        assert!(height > 0.0, "content should have a height");

        let svg = result.svg.unwrap();
        let (frame_width, frame_height) = svg_view_box(&svg);
        assert!(
            (frame_width - (240.0 + 2.0 * PREVIEW_PAD_PT)).abs() < 1e-6,
            "frame {frame_width} should clear the content by the pad on each side"
        );
        assert!(
            (frame_height - (height + 2.0 * PREVIEW_PAD_PT)).abs() < 1e-6,
            "frame {frame_height} should clear the {height}pt content by the pad on each side"
        );
        // The slack has to be see-through, or every block wears a white halo over the page.
        assert!(
            !svg.contains("fill=\"#ffffff\""),
            "the page fill should not be painted"
        );
    }

    fn svg_view_box(svg: &str) -> (f64, f64) {
        let box_start = svg
            .find("viewBox=\"")
            .expect("svg should declare a viewBox")
            + 9;
        let box_end = box_start + svg[box_start..].find('"').unwrap();
        let values: Vec<f64> = svg[box_start..box_end]
            .split_whitespace()
            .map(|value| value.parse().unwrap())
            .collect();
        (values[2], values[3])
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
    fn explains_and_formats_typst_in_process() {
        let root = tempfile::tempdir().unwrap();
        let source = "#box".to_owned();
        let help = hover(root.path(), source, 2, false)
            .unwrap()
            .expect("box should have built-in documentation");
        assert!(help.value.contains("container"));

        let formatted = format_source("#let x=(1,2,3)\n#x".to_owned()).unwrap();
        assert_eq!(formatted, "#let x = (1, 2, 3)\n#x\n");
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
