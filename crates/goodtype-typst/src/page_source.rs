//! Effective page-template source shared by preview, export and editor analysis.
use std::path::Path;
use typst::syntax::{ast, parse};

pub struct PageSource {
    pub body: String,
    pub preamble: Option<String>,
    pub header_bytes: usize,
    pub managed: bool,
}

pub fn managed_header(source: &str) -> Option<(&str, usize)> {
    let mut lines = source.split_inclusive('\n');
    let import = lines.next()?;
    let path = import
        .trim_end_matches(['\r', '\n'])
        .strip_prefix("#import \"/styles/")?
        .strip_suffix("\": preset")?;
    if !path.ends_with(".typ")
        || path.is_empty()
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return None;
    }
    let show = lines.next()?;
    if show.trim_end_matches(['\r', '\n']) != "#show: preset.with(rhythm: goodtype_rhythm)" {
        return None;
    }
    Some((path, import.len() + show.len()))
}

pub fn prepare(root: &Path, source: &str) -> Result<PageSource, String> {
    let Some((filename, header_bytes)) = managed_header(source) else {
        return Ok(PageSource {
            body: source.into(),
            preamble: None,
            header_bytes: 0,
            managed: false,
        });
    };
    let root = goodtype_core::paths::canonical_root(root).map_err(|e| e.to_string())?;
    let path = goodtype_core::paths::resolve_file(&root, &format!("styles/{filename}"))
        .map_err(|e| e.to_string())?;
    let bytes = goodtype_core::storage::files::read_bounded(&path, crate::MAX_SOURCE_BYTES)
        .map_err(|e| format!("Page template: {e}"))?;
    let template = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    let tree = parse(&template);
    let defines_preset = tree.children().filter_map(|node| node.cast::<ast::LetBinding>())
        .any(|binding| matches!(binding.kind(), ast::LetBindingKind::Closure(name) if name.as_str() == "preset"));
    Ok(PageSource {
        body: if defines_preset {
            source.into()
        } else {
            source[header_bytes..].into()
        },
        preamble: (!defines_preset).then(|| without_page_settings(&template)),
        header_bytes,
        managed: true,
    })
}

// Fixed page geometry belongs to Goodtype. A template's document page settings cannot be
// applied inside export's placed content, and preview must use the same effective preamble.
fn without_page_settings(source: &str) -> String {
    let tree = parse(source);
    let mut result = String::new();
    let mut copied = 0;
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
            result.push_str(&source[copied..start]);
            copied = end;
        }
        offset = end;
    }
    result.push_str(&source[copied..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguishes_real_function_bindings_from_examples_and_preserves_literals() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("styles")).unwrap();
        let header =
            "#import \"/styles/a.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n\n";
        for template in [
            "#set text(fill: blue)",
            "```typ\n#let preset(body) = body\n```",
            "/* #let preset(body) = body */",
        ] {
            std::fs::write(root.path().join("styles/a.typ"), template).unwrap();
            let page = prepare(root.path(), &format!("{header}Body")).unwrap();
            assert_eq!(page.preamble.as_deref(), Some(template));
            assert_eq!(page.body, "\nBody");
        }
        std::fs::write(
            root.path().join("styles/a.typ"),
            "#let preset(body, rhythm: 16pt) = body",
        )
        .unwrap();
        assert!(
            prepare(root.path(), &format!("{header}Body"))
                .unwrap()
                .preamble
                .is_none()
        );
        assert!(
            !prepare(root.path(), &format!("```typ\n{header}```"))
                .unwrap()
                .managed
        );
    }
}
