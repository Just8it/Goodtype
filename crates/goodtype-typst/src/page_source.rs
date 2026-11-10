//! Effective page-template source shared by preview, export, editor analysis and validation.
//!
//! A managed Page text names a preset file under `/styles/`. That file has one of two forms:
//!
//! - **Function**: it binds `preset` to a function (directly, through a `let` alias, or by
//!   importing/re-exporting one). The page's own header imports and applies it, so the module is
//!   evaluated at its real location and its relative paths resolve from it.
//! - **Preamble**: anything else — an ordinary Typst document header of `#import`, `#let`,
//!   `#set` and `#show` lines. Its rules must reach the page text, so it is inlined above the
//!   body. Its relative paths are rewritten to resolve from the preset file, as they would if
//!   Typst evaluated it in place, and its page settings are blanked (fixed geometry is Goodtype's).
//!
//! [`detect_form`] is the one place that decides between them.
use std::ops::Range;
use std::path::Path;

use serde::Serialize;
use typst::syntax::{LinkedNode, ast, ast::AstNode, parse};

pub struct PageSource {
    pub body: String,
    /// The inlined preamble, for a preamble-form preset only.
    pub preamble: Option<Preamble>,
    pub header_bytes: usize,
    pub managed: bool,
    /// The preset's detected form, for a managed source whose preset could be read.
    pub form: Option<PresetForm>,
}

/// A preamble-form preset as inlined, with what is needed to point diagnostics back at it.
pub struct Preamble {
    /// The text inlined above the body.
    pub text: String,
    /// The preset's virtual path, e.g. `/styles/notes.typ`.
    pub path: String,
    /// The preset file's own text.
    pub original: String,
    /// Each rewritten span: (range in `original`, range in `text`). Sorted, non-overlapping.
    edits: Vec<(Range<usize>, Range<usize>)>,
}

impl Preamble {
    /// Map an offset in [`Self::text`] back to the preset file. Offsets inside a rewritten span
    /// map to the start of that span in the original.
    pub fn original_offset(&self, offset: usize) -> Option<usize> {
        let mut shift: isize = 0;
        for (original, text) in &self.edits {
            if offset < text.start {
                break;
            }
            if offset < text.end {
                return Some(original.start);
            }
            shift = text.end as isize - original.end as isize;
        }
        let mapped = offset as isize - shift;
        (mapped >= 0 && (mapped as usize) <= self.original.len()).then_some(mapped as usize)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetForm {
    Function,
    Preamble,
}

pub use goodtype_core::storage::managed_preset_header as managed_header;

/// Prepare a Page text source against the notebook on disk. The package policy only matters
/// when the preset's form has to be decided by evaluating it (an alias or re-export).
pub fn prepare(
    root: &Path,
    source: &str,
    allow_remote_packages: bool,
) -> Result<PageSource, String> {
    prepare_with(
        source,
        |filename| {
            let root = goodtype_core::paths::canonical_root(root).map_err(|e| e.to_string())?;
            let path = goodtype_core::paths::resolve_file(&root, &format!("styles/{filename}"))
                .map_err(|e| e.to_string())?;
            let bytes = goodtype_core::storage::files::read_bounded(&path, crate::MAX_SOURCE_BYTES)
                .map_err(|e| format!("Page template: {e}"))?;
            String::from_utf8(bytes).map_err(|e| e.to_string())
        },
        |path| {
            crate::embedded::binds_preset_function(
                root,
                path,
                crate::embedded::package_loader(allow_remote_packages),
                None,
            )
        },
    )
}

/// Prepare from granted project bytes only; package imports follow the caller's policy.
pub(crate) fn prepare_granted(
    root: &Path,
    source: &str,
    files: &crate::GrantedFiles,
    allow_remote_packages: bool,
) -> Result<PageSource, String> {
    prepare_with(
        source,
        |filename| {
            let path = format!("/styles/{filename}");
            let bytes = files
                .get(&path)
                .ok_or_else(|| format!("not shared: {path}"))?;
            if bytes.len() > crate::MAX_SOURCE_BYTES {
                return Err("Page template exceeds source limit".into());
            }
            String::from_utf8(bytes.clone()).map_err(|e| e.to_string())
        },
        |path| {
            crate::embedded::binds_preset_function(
                root,
                path,
                crate::embedded::package_loader(allow_remote_packages),
                Some(files),
            )
        },
    )
}

fn prepare_with(
    source: &str,
    load: impl FnOnce(&str) -> Result<String, String>,
    evaluate: impl FnOnce(&str) -> Option<bool>,
) -> Result<PageSource, String> {
    let Some((filename, header_bytes)) = managed_header(source) else {
        return Ok(PageSource {
            body: source.into(),
            preamble: None,
            header_bytes: 0,
            managed: false,
            form: None,
        });
    };
    let path = format!("/styles/{filename}");
    let template = load(filename)?;
    let form = detect_form(&template, || evaluate(&path));
    Ok(PageSource {
        body: match form {
            PresetForm::Function => source.into(),
            PresetForm::Preamble => source[header_bytes..].into(),
        },
        preamble: (form == PresetForm::Preamble).then(|| relocated_preamble(&path, template)),
        header_bytes,
        managed: true,
        form: Some(form),
    })
}

/// The one preset form detector.
///
/// A top-level `#let preset(..) = ..` is a function without evaluating anything. A top-level
/// binding that *might* bind `preset` to a function — `#let preset = other`, an import naming
/// `preset` (also `x as preset`), or a wildcard import — is decided by `evaluate`, which
/// evaluates the module and reports whether `preset` is a function value. Nothing else is
/// inferred: a template without such a binding is a preamble. When evaluation fails, an explicit
/// binding is treated as a function (so its real error surfaces from the import), a wildcard as
/// a preamble.
pub fn detect_form(template: &str, evaluate: impl FnOnce() -> Option<bool>) -> PresetForm {
    let tree = parse(template);
    let mut explicit = false;
    let mut wildcard = false;
    for node in tree.children() {
        if let Some(binding) = node.cast::<ast::LetBinding>() {
            match binding.kind() {
                ast::LetBindingKind::Closure(name) if name.as_str() == "preset" => {
                    return PresetForm::Function;
                }
                ast::LetBindingKind::Normal(ast::Pattern::Normal(ast::Expr::Ident(name)))
                    if name.as_str() == "preset" =>
                {
                    explicit = true;
                }
                _ => {}
            }
        } else if let Some(import) = node.cast::<ast::ModuleImport>() {
            match import.imports() {
                Some(ast::Imports::Wildcard) => wildcard = true,
                Some(ast::Imports::Items(items))
                    if items
                        .iter()
                        .any(|item| item.bound_name().as_str() == "preset") =>
                {
                    explicit = true;
                }
                _ => {}
            }
        }
    }
    if !explicit && !wildcard {
        return PresetForm::Preamble;
    }
    match evaluate() {
        Some(true) => PresetForm::Function,
        Some(false) => PresetForm::Preamble,
        None if explicit => PresetForm::Function,
        None => PresetForm::Preamble,
    }
}

/// Functions whose first positional string argument is a path resolved from the calling file.
const PATH_FUNCTIONS: &[&str] = &[
    "read",
    "image",
    "json",
    "csv",
    "yaml",
    "toml",
    "xml",
    "cbor",
    "plugin",
    "bibliography",
];

fn relocated_preamble(path: &str, original: String) -> Preamble {
    let directory = path.rsplit_once('/').map_or("", |(directory, _)| directory);
    let mut replacements: Vec<(Range<usize>, String)> = Vec::new();
    let tree = parse(&original);
    collect_relocations(&LinkedNode::new(&tree), directory, &mut replacements);
    collect_page_settings(&original, &tree, &mut replacements);
    replacements.sort_by_key(|(range, _)| range.start);
    let mut text = String::with_capacity(original.len());
    let mut edits = Vec::new();
    let mut copied = 0;
    for (range, replacement) in replacements {
        if range.start < copied {
            continue;
        }
        text.push_str(&original[copied..range.start]);
        let start = text.len();
        text.push_str(&replacement);
        if replacement.len() != range.len() {
            edits.push((range.clone(), start..text.len()));
        }
        copied = range.end;
    }
    text.push_str(&original[copied..]);
    Preamble {
        text,
        path: path.into(),
        original,
        edits,
    }
}

fn collect_relocations(
    node: &LinkedNode,
    directory: &str,
    output: &mut Vec<(Range<usize>, String)>,
) {
    let path_expr = if let Some(import) = node.cast::<ast::ModuleImport>() {
        Some(import.source())
    } else if let Some(include) = node.cast::<ast::ModuleInclude>() {
        Some(include.source())
    } else if let Some(call) = node.cast::<ast::FuncCall>() {
        match call.callee() {
            ast::Expr::Ident(name) if PATH_FUNCTIONS.contains(&name.as_str()) => {
                call.args().items().find_map(|arg| match arg {
                    ast::Arg::Pos(expr) => Some(expr),
                    _ => None,
                })
            }
            _ => None,
        }
    } else {
        None
    };
    if let Some(ast::Expr::Str(literal)) = path_expr
        && let Some(found) = descendant(node, literal.to_untyped())
        && let Some(absolute) = absolute_path(directory, literal.get().as_str())
    {
        output.push((found.range(), format!("\"{absolute}\"")));
    }
    for child in node.children() {
        collect_relocations(&child, directory, output);
    }
}

/// The linked (positioned) node for `target`, found by identity: a freshly parsed tree has
/// detached spans, so span lookup cannot locate it.
fn descendant<'a>(
    node: &LinkedNode<'a>,
    target: &typst::syntax::SyntaxNode,
) -> Option<LinkedNode<'a>> {
    if std::ptr::eq(node.get(), target) {
        return Some(node.clone());
    }
    node.children().find_map(|child| descendant(&child, target))
}

/// `helpers.typ` beside `/styles/x.typ` is `/styles/helpers.typ`; `..` is resolved. Absolute,
/// package and unusual paths stay as written, so Typst reports them exactly as before.
fn absolute_path(directory: &str, path: &str) -> Option<String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with('@')
        || path.contains(['\\', '"', '\n', '\r'])
    {
        return None;
    }
    let mut parts: Vec<&str> = directory
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(format!("/{}", parts.join("/")))
}

// Fixed page geometry belongs to Goodtype. A template's document page settings cannot be
// applied inside export's placed content, and preview must use the same effective preamble.
// They are blanked rather than removed, keeping every other offset in place.
fn collect_page_settings(
    source: &str,
    tree: &typst::syntax::SyntaxNode,
    output: &mut Vec<(Range<usize>, String)>,
) {
    let mut offset = 0;
    for node in tree.children() {
        let end = offset + node.len();
        if let Some(rule) = node.cast::<ast::SetRule>()
            && matches!(rule.target(), ast::Expr::Ident(name) if name.as_str() == "page")
        {
            let start = if source[..offset].ends_with('#') {
                offset - 1
            } else {
                offset
            };
            let blank = source[start..end]
                .chars()
                .map(|c| if c == '\n' || c == '\r' { c } else { ' ' })
                .collect();
            output.push((start..end, blank));
        }
        offset = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const HEADER: &str =
        "#import \"/styles/a.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n\n";

    fn styles(template: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("styles")).unwrap();
        std::fs::write(root.path().join("styles/a.typ"), template).unwrap();
        root
    }

    #[test]
    fn distinguishes_real_function_bindings_from_examples_and_preserves_literals() {
        for template in [
            "#set text(fill: blue)",
            "```typ\n#let preset(body) = body\n```",
            "/* #let preset(body) = body */",
        ] {
            let root = styles(template);
            let page = prepare(root.path(), &format!("{HEADER}Body"), false).unwrap();
            assert_eq!(
                page.preamble.as_ref().map(|p| p.text.as_str()),
                Some(template)
            );
            assert_eq!(page.form, Some(PresetForm::Preamble));
            assert_eq!(page.body, "\nBody");
        }
        let root = styles("#let preset(body, rhythm: 16pt) = body");
        let page = prepare(root.path(), &format!("{HEADER}Body"), false).unwrap();
        assert!(page.preamble.is_none());
        assert_eq!(page.form, Some(PresetForm::Function));
        assert!(
            !prepare(root.path(), &format!("```typ\n{HEADER}```"), false)
                .unwrap()
                .managed
        );
    }

    /// P5b: the form comes from what `preset` evaluates to, not from how the file spells it.
    #[test]
    fn aliases_and_reexports_are_functions_other_bindings_are_not() {
        type FormCase<'a> = (&'a str, &'a [(&'a str, &'a str)], PresetForm);
        let cases: &[FormCase<'_>] = &[
            (
                "#let style(body, rhythm: 16pt) = body\n#let preset = style",
                &[],
                PresetForm::Function,
            ),
            (
                "#import \"base.typ\": preset",
                &[("base.typ", "#let preset(body, rhythm: 16pt) = body")],
                PresetForm::Function,
            ),
            (
                "#import \"base.typ\": notes as preset",
                &[("base.typ", "#let notes(body, rhythm: 16pt) = body")],
                PresetForm::Function,
            ),
            (
                "#import \"base.typ\": *",
                &[("base.typ", "#let preset(body, rhythm: 16pt) = body")],
                PresetForm::Function,
            ),
            (
                "#import \"base.typ\": *\n#set text(size: 12pt)",
                &[("base.typ", "#let helper = 1")],
                PresetForm::Preamble,
            ),
            (
                "#let preset = 1\n#set text(size: 12pt)",
                &[],
                PresetForm::Preamble,
            ),
            ("#let preset = (body) => body", &[], PresetForm::Function),
        ];
        for (template, extra, expected) in cases {
            let root = styles(template);
            for (name, text) in *extra {
                std::fs::write(root.path().join("styles").join(name), text).unwrap();
            }
            let page = prepare(root.path(), &format!("{HEADER}Body"), false).unwrap();
            assert_eq!(page.form, Some(*expected), "{template}");
        }
        // Detection never guesses: without a candidate binding nothing is evaluated.
        let mut evaluated = false;
        assert_eq!(
            detect_form("#let notes(body) = body", || {
                evaluated = true;
                Some(true)
            }),
            PresetForm::Preamble
        );
        assert!(!evaluated);
    }

    /// P6: a preamble's relative paths resolve from the preset file, not the notebook root.
    #[test]
    fn preamble_relative_paths_resolve_from_the_preset_and_map_back() {
        let template = "#import \"lib/defs.typ\": unit\n#include \"../shared/intro.typ\"\n#let logo = read(\"logo.svg\")\n#import \"@preview/x:1.0.0\": y\n#import \"/abs.typ\": z\n#set page(paper: \"a4\")\n#set text(size: 12pt)";
        let preamble = relocated_preamble("/styles/a.typ", template.into());
        assert!(
            preamble
                .text
                .contains("#import \"/styles/lib/defs.typ\": unit")
        );
        assert!(preamble.text.contains("#include \"/shared/intro.typ\""));
        assert!(preamble.text.contains("read(\"/styles/logo.svg\")"));
        assert!(preamble.text.contains("\"@preview/x:1.0.0\""));
        assert!(preamble.text.contains("\"/abs.typ\""));
        assert!(!preamble.text.contains("set page"));
        let set_text = preamble.text.find("#set text").unwrap();
        assert_eq!(
            preamble.original_offset(set_text),
            template.find("#set text")
        );
        let rewritten = preamble.text.find("\"/styles/logo.svg\"").unwrap();
        assert_eq!(
            preamble.original_offset(rewritten + 3),
            template.find("\"logo.svg\"")
        );
        assert_eq!(absolute_path("/styles", "../../escape.typ"), None);
    }
}
