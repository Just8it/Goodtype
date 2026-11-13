//! Proposal dependency checks over a committed snapshot. No persistence occurs here.
use super::{
    NotebookSnapshot, StorageError,
    files::{fingerprint_files, read_limited},
    invalid_error,
    paths::{canonical_root, resolve_existing},
};
use crate::PageObject;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

/// Process-local fingerprints, not cryptographic signatures or durable notebook data.
/// Each object hash covers its complete serialized object JSON (including geometry, style,
/// transforms and metadata), plus exact source/asset bytes, or the JSON of its referenced
/// strokes in reference order. Group hashes recursively cover their children.
/// Page text covers each PageTypst object's JSON and exact source bytes. Preset hashes cover
/// the managed import's relative path and exact file bytes. Shared style covers its path and
/// exact bytes. Absent page dependencies are represented by None, so additions are detected.
/// Revision is informational: unrelated page edits must not invalidate a proposal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalBase {
    pub page_id: String,
    pub revision: u64,
    pub objects: BTreeMap<String, u64>,
    pub page_text: Option<u64>,
    pub preset: Option<u64>,
    pub shared_style: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalPrecondition {
    pub base: ProposalBase,
    pub require_retained_history: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DependencyChange {
    Modified,
    Removed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "dependency", rename_all = "camelCase")]
pub enum ChangedDependency {
    Object {
        id: String,
        change: DependencyChange,
    },
    Page {
        change: DependencyChange,
    },
    PageText {
        change: DependencyChange,
    },
    Preset {
        change: DependencyChange,
    },
    SharedStyle {
        change: DependencyChange,
    },
    /// A notebook-relative file a compiled proposal read, such as a literal import.
    File {
        id: String,
        change: DependencyChange,
    },
}

fn file_bytes(
    root: &Path,
    snapshot: &NotebookSnapshot,
    path: &str,
    maximum: usize,
) -> Result<Vec<u8>, StorageError> {
    if let Some(file) = snapshot
        .blocks
        .iter()
        .chain(&snapshot.assets)
        .find(|file| file.path == path)
    {
        return Ok(file.bytes.clone());
    }
    read_limited(resolve_existing(root, path)?, maximum)
}

fn object_hash(
    root: &Path,
    snapshot: &NotebookSnapshot,
    id: &str,
    visiting: &mut Vec<String>,
) -> Result<u64, StorageError> {
    if visiting.iter().any(|seen| seen == id) {
        return Err(invalid_error("cyclic proposal target group"));
    }
    let object = snapshot
        .page
        .objects
        .iter()
        .find(|object| object.fields().id == id)
        .ok_or_else(|| invalid_error("unknown proposal target object"))?;
    visiting.push(id.into());
    let mut parts = vec![("object".to_owned(), serde_json::to_vec(object)?)];
    if let Some(source) = object.source() {
        parts.push((
            source.path.into(),
            file_bytes(root, snapshot, source.path, source.role.max_bytes())?,
        ));
    }
    match object {
        PageObject::InkGroup {
            ink_layer_id,
            stroke_ids,
            ..
        } => {
            let layer = snapshot
                .ink_layers
                .iter()
                .find(|layer| &layer.id == ink_layer_id)
                .ok_or_else(|| invalid_error("missing proposal ink layer"))?;
            for id in stroke_ids {
                let stroke = layer
                    .strokes
                    .iter()
                    .find(|stroke| &stroke.id == id)
                    .ok_or_else(|| invalid_error("missing proposal stroke"))?;
                parts.push((format!("stroke/{id}"), serde_json::to_vec(stroke)?));
            }
        }
        PageObject::Group { child_ids, .. } => {
            for id in child_ids {
                parts.push((
                    format!("child/{id}"),
                    object_hash(root, snapshot, id, visiting)?
                        .to_le_bytes()
                        .to_vec(),
                ));
            }
        }
        _ => {}
    }
    visiting.pop();
    Ok(fingerprint_files(
        &parts
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
            .collect::<Vec<_>>(),
    ))
}

/// The managed Page text header, located in its source.
///
/// Byte ranges are into the original source: `import` and `show` cover the two directive lines
/// (including their line endings), `path` the quoted preset path without quotes, and `end` is
/// where the editable body begins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedHeader<'a> {
    pub filename: &'a str,
    pub path: std::ops::Range<usize>,
    pub import: std::ops::Range<usize>,
    pub show: std::ops::Range<usize>,
    pub end: usize,
}

/// The managed Page text header: `#import "/styles/<name>.typ": preset` followed by
/// `#show: preset.with(rhythm: goodtype_rhythm)`, in that order, as the first directives.
/// Blank lines, `//` comment lines, single-line `/* */` comments, spaces/tabs around tokens,
/// a trailing `//` comment and CRLF endings are tolerated. This is the single parser;
/// `goodtype-typst` page preparation uses it too.
pub fn parse_managed_header(source: &str) -> Option<ManagedHeader<'_>> {
    let mut offset = 0;
    let mut lines = source.split_inclusive('\n').map(|line| {
        let start = offset;
        offset += line.len();
        (start, line)
    });
    let mut next_directive = || lines.find(|(_, line)| !ignorable_line(line));
    let (import_start, import) = next_directive()?;
    let path = import_path(import)?;
    let filename = path.strip_prefix("/styles/")?;
    if !filename.ends_with(".typ")
        || filename.len() <= 4
        || !filename
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return None;
    }
    let (show_start, show) = next_directive()?;
    if !is_managed_show(show) {
        return None;
    }
    let path_start = import_start + (path.as_ptr() as usize - import.as_ptr() as usize);
    let filename_start = path_start + "/styles/".len();
    Some(ManagedHeader {
        filename: &source[filename_start..filename_start + filename.len()],
        path: path_start..path_start + path.len(),
        import: import_start..import_start + import.len(),
        show: show_start..show_start + show.len(),
        end: show_start + show.len(),
    })
}

/// Returns the preset file name and the header's byte length. See [`parse_managed_header`].
pub fn managed_preset_header(source: &str) -> Option<(&str, usize)> {
    parse_managed_header(source).map(|header| (header.filename, header.end))
}

fn ignorable_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty()
        || trimmed.starts_with("//")
        || (trimmed.starts_with("/*")
            && trimmed.ends_with("*/")
            && trimmed.len() >= 4
            && !trimmed[2..trimmed.len() - 2].contains("*/")
            && !trimmed[2..trimmed.len() - 2].contains("/*"))
}

/// A line's content without its line ending, surrounding blanks or a trailing `//` comment.
fn directive_text(line: &str) -> &str {
    let line = line.trim_end_matches(['\r', '\n']);
    let code = match line.find("//") {
        // A `//` inside the quoted path would be invalid anyway; only strip outside quotes.
        Some(index) if line[..index].matches('"').count().is_multiple_of(2) => &line[..index],
        _ => line,
    };
    code.trim_matches([' ', '\t'])
}

fn token<'a>(text: &'a str, expected: &str) -> Option<&'a str> {
    text.trim_start_matches([' ', '\t']).strip_prefix(expected)
}

fn import_path(line: &str) -> Option<&str> {
    let rest = directive_text(line).strip_prefix("#import")?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let rest = token(rest, "\"")?;
    let close = rest.find('"')?;
    let path = &rest[..close];
    let rest = token(&rest[close + 1..], ":")?;
    let rest = token(rest, "preset")?;
    rest.trim_matches([' ', '\t']).is_empty().then_some(path)
}

fn is_managed_show(line: &str) -> bool {
    let check = || {
        let rest = directive_text(line).strip_prefix("#show")?;
        let rest = token(rest, ":")?;
        let rest = token(rest, "preset")?;
        let rest = token(rest, ".with")?;
        let rest = token(rest, "(")?;
        let rest = token(rest, "rhythm")?;
        let rest = token(rest, ":")?;
        let rest = token(rest, "goodtype_rhythm")?;
        let rest = token(rest, ",").unwrap_or(rest);
        let rest = token(rest, ")")?;
        rest.trim_matches([' ', '\t']).is_empty().then_some(())
    };
    check().is_some()
}

fn preset_path(bytes: &[u8]) -> Option<String> {
    let (name, _) = managed_preset_header(std::str::from_utf8(bytes).ok()?)?;
    Some(format!("styles/{name}"))
}

/// Capture under the caller's notebook lock, using a complete committed snapshot.
pub fn proposal_base(
    root: &Path,
    snapshot: &NotebookSnapshot,
    targets: &[String],
) -> Result<ProposalBase, StorageError> {
    proposal_base_inner(root, snapshot, targets, false)
}

fn proposal_base_inner(
    root: &Path,
    snapshot: &NotebookSnapshot,
    targets: &[String],
    allow_missing: bool,
) -> Result<ProposalBase, StorageError> {
    let root = canonical_root(root)?;
    let mut objects = BTreeMap::new();
    for id in targets {
        objects.insert(
            id.clone(),
            object_hash(&root, snapshot, id, &mut Vec::new())?,
        );
    }
    let mut text = Vec::new();
    let mut presets = Vec::new();
    for object in &snapshot.page.objects {
        if let PageObject::PageTypst { source_path, .. } = object {
            text.push((
                object.fields().id.clone(),
                object_hash(&root, snapshot, &object.fields().id, &mut Vec::new())?
                    .to_le_bytes()
                    .to_vec(),
            ));
            let bytes = file_bytes(&root, snapshot, source_path, crate::object::MAX_BLOCK_BYTES)?;
            if let Some(path) = preset_path(&bytes) {
                match file_bytes(&root, snapshot, &path, crate::object::MAX_BLOCK_BYTES) {
                    Ok(bytes) => presets.push((path, bytes)),
                    Err(StorageError::Io(error))
                        if allow_missing && error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    let hash = |parts: Vec<(String, Vec<u8>)>| {
        (!parts.is_empty()).then(|| {
            fingerprint_files(
                &parts
                    .iter()
                    .map(|(p, b)| (p.as_str(), b.as_slice()))
                    .collect::<Vec<_>>(),
            )
        })
    };
    let shared_style = snapshot
        .manifest
        .shared_style_path
        .as_ref()
        .map(|path| {
            Ok::<_, StorageError>(fingerprint_files(&[(
                path,
                &file_bytes(&root, snapshot, path, crate::object::MAX_BLOCK_BYTES)?,
            )]))
        })
        .transpose()?;
    Ok(ProposalBase {
        page_id: snapshot.page.id.clone(),
        revision: snapshot.page.revision,
        objects,
        page_text: hash(text),
        preset: hash(presets),
        shared_style,
    })
}

/// Check loaded committed content without reopening the page. Revision differences alone pass.
pub fn check_proposal_base(
    root: &Path,
    current: &NotebookSnapshot,
    base: &ProposalBase,
) -> Result<(), StorageError> {
    let mut changed = Vec::new();
    if current.page.id != base.page_id {
        changed.push(ChangedDependency::Page {
            change: DependencyChange::Modified,
        });
    }
    let targets: Vec<_> = base
        .objects
        .keys()
        .filter(|id| current.page.objects.iter().any(|o| &o.fields().id == *id))
        .cloned()
        .collect();
    let now = proposal_base_inner(root, current, &targets, true)?;
    for (id, hash) in &base.objects {
        match now.objects.get(id) {
            None => changed.push(ChangedDependency::Object {
                id: id.clone(),
                change: DependencyChange::Removed,
            }),
            Some(value) if value != hash => changed.push(ChangedDependency::Object {
                id: id.clone(),
                change: DependencyChange::Modified,
            }),
            _ => {}
        }
    }
    let change = |now: Option<u64>| {
        if now.is_none() {
            DependencyChange::Removed
        } else {
            DependencyChange::Modified
        }
    };
    if base.page_text != now.page_text {
        changed.push(ChangedDependency::PageText {
            change: change(now.page_text),
        });
    }
    if base.preset != now.preset {
        changed.push(ChangedDependency::Preset {
            change: change(now.preset),
        });
    }
    if base.shared_style != now.shared_style {
        changed.push(ChangedDependency::SharedStyle {
            change: change(now.shared_style),
        });
    }
    if changed.is_empty() {
        Ok(())
    } else {
        Err(StorageError::ProposalStale(changed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{fixtures::*, *};

    #[test]
    fn target_hashes_cover_objects_sources_ink_assets_and_shapes() {
        let temp = tempfile::tempdir().unwrap();
        let mut value = snapshot();
        value.page.objects.push(PageObject::InkGroup {
            fields: fields("ink", 2),
            ink_layer_id: "ink-layer-001".into(),
            stroke_ids: vec!["stroke-001".into()],
        });
        let shape: PageObject = serde_json::from_value(serde_json::json!({
            "type": "shape", "id": "shape", "x": 0, "y": 0, "rotation": 0, "scale": 1,
            "zIndex": 3, "readingOrder": 3, "groupId": null, "createdAt": "now", "modifiedAt": "now",
            "geometry": {"kind": "line", "start": {"x": 0, "y": 0}, "end": {"x": 10, "y": 10}},
            "style": {"strokeColor": "#000000", "strokeWidthPt": 1, "fillColor": null, "opacity": 1}
        })).unwrap();
        value.page.objects.push(shape);
        let ids = vec![
            "typst-001".into(),
            "image-001".into(),
            "ink".into(),
            "shape".into(),
        ];
        let base = proposal_base(temp.path(), &value, &ids).unwrap();
        assert_eq!(base.objects.len(), 4);
        assert_eq!(
            serde_json::from_slice::<ProposalBase>(&serde_json::to_vec(&base).unwrap()).unwrap(),
            base
        );
        for index in 0..4 {
            let mut changed = value.clone();
            changed.page.objects[index].fields_mut().x += 1.0;
            assert!(matches!(
                check_proposal_base(temp.path(), &changed, &base),
                Err(StorageError::ProposalStale(_))
            ));
        }
        for kind in 0..3 {
            let mut changed = value.clone();
            match kind {
                0 => changed.blocks[0].bytes.push(b' '),
                1 => changed.assets[0].bytes.push(0),
                _ => changed.ink_layers[0].strokes[0].points[0].x += 1.0,
            }
            assert!(matches!(
                check_proposal_base(temp.path(), &changed, &base),
                Err(StorageError::ProposalStale(_))
            ));
        }
        assert!(proposal_base(temp.path(), &value, &["unknown".into()]).is_err());
    }

    #[test]
    fn unrelated_edits_pass_targets_modified_and_removed_are_stale() {
        let temp = tempfile::tempdir().unwrap();
        let mut value = snapshot();
        let base = proposal_base(temp.path(), &value, &["typst-001".into()]).unwrap();
        value.page.revision += 1;
        value.page.objects[1].fields_mut().x += 1.0;
        check_proposal_base(temp.path(), &value, &base).unwrap();
        value.page.objects[0].fields_mut().x += 1.0;
        assert!(
            matches!(check_proposal_base(temp.path(), &value, &base), Err(StorageError::ProposalStale(changed)) if changed == vec![ChangedDependency::Object {id: "typst-001".into(), change: DependencyChange::Modified}])
        );
        value.page.objects.remove(0);
        assert!(
            matches!(check_proposal_base(temp.path(), &value, &base), Err(StorageError::ProposalStale(changed)) if changed == vec![ChangedDependency::Object {id: "typst-001".into(), change: DependencyChange::Removed}])
        );
    }

    #[test]
    fn page_text_preset_and_shared_style_changes_are_stale() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("styles")).unwrap();
        let preset = temp.path().join("styles/test.typ");
        std::fs::write(&preset, b"first").unwrap();
        let mut value = snapshot();
        value.page.objects.push(PageObject::PageTypst {
            fields: fields("page-text", 2),
            source_path: "blocks/page.typ".into(),
        });
        value.blocks.push(StoredFile {path: "blocks/page.typ".into(), bytes: b"#import \"/styles/test.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\nText".to_vec()});
        value.manifest.shared_style_path = Some("style.typ".into());
        value.blocks.push(StoredFile {
            path: "style.typ".into(),
            bytes: b"style".to_vec(),
        });
        let base = proposal_base(temp.path(), &value, &[]).unwrap();
        std::fs::write(&preset, b"second").unwrap();
        assert!(
            matches!(check_proposal_base(temp.path(), &value, &base), Err(StorageError::ProposalStale(changed)) if changed.contains(&ChangedDependency::Preset { change: DependencyChange::Modified }))
        );
        std::fs::remove_file(&preset).unwrap();
        assert!(
            matches!(check_proposal_base(temp.path(), &value, &base), Err(StorageError::ProposalStale(changed)) if changed.contains(&ChangedDependency::Preset { change: DependencyChange::Removed }))
        );
        std::fs::write(&preset, b"first").unwrap();
        value.blocks[1].bytes.push(b'!');
        assert!(
            matches!(check_proposal_base(temp.path(), &value, &base), Err(StorageError::ProposalStale(changed)) if changed.contains(&ChangedDependency::PageText { change: DependencyChange::Modified }))
        );
        value.blocks[1].bytes.pop();
        value.blocks[2].bytes.push(b'!');
        assert!(
            matches!(check_proposal_base(temp.path(), &value, &base), Err(StorageError::ProposalStale(changed)) if changed.contains(&ChangedDependency::SharedStyle { change: DependencyChange::Modified }))
        );
    }

    #[test]
    fn stale_precondition_writes_nothing_and_none_matches_ordinary_commit() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let mut history = NotebookHistory::default();
        let mut value = observe_notebook(&root, &mut history).unwrap();
        let base = proposal_base(&root, &value, &["typst-001".into()]).unwrap();
        value.page.objects[0].fields_mut().x += 1.0;
        let value = commit_notebook(&root, &mut history, value)
            .unwrap()
            .snapshot;
        let root = super::super::paths::canonical_root(&root).unwrap();
        let before = super::super::write::canonical_fingerprint(&root, &value).unwrap();
        assert!(matches!(
            commit_notebook_with_precondition(
                &root,
                &mut history,
                value.clone(),
                Some(ProposalPrecondition {
                    base,
                    require_retained_history: true
                })
            ),
            Err(StorageError::ProposalStale(_))
        ));
        assert_eq!(
            before,
            super::super::write::canonical_fingerprint(&root, &open_notebook(&root).unwrap())
                .unwrap()
        );
        let first = commit_notebook_with_precondition(&root, &mut history, value, None).unwrap();
        let second = commit_notebook(&root, &mut history, first.snapshot.clone()).unwrap();
        assert_eq!(
            second.snapshot.page.revision,
            first.snapshot.page.revision + 1
        );
        assert!(first.can_undo && second.can_undo);
    }
}

#[cfg(test)]
mod managed_header_review_tests {
    #[test]
    fn comments_crlf_and_exact_header_bytes_are_preserved() {
        let header = "// user comment\r\n\r\n#import \"/styles/example.typ\": preset\r\n// preset comment\r\n#show: preset.with(rhythm: goodtype_rhythm)\r\n";
        let source = format!("{header}Body");
        let (name, bytes) = super::managed_preset_header(&source).unwrap();
        assert_eq!(name, "example.typ");
        assert_eq!(&source[..bytes], header);
        assert_eq!(&source[bytes..], "Body");
    }

    #[test]
    fn whitespace_trailing_comments_and_block_comment_lines_are_tolerated_with_ranges() {
        let source = "/* page */
  #import  \"/styles/a-b.typ\" : preset // chosen\r
	#show :preset .with( rhythm : goodtype_rhythm, ) // managed
Body";
        let header = super::parse_managed_header(source).unwrap();
        assert_eq!(header.filename, "a-b.typ");
        assert_eq!(&source[header.path.clone()], "/styles/a-b.typ");
        assert!(source[header.import.clone()].contains("#import"));
        assert!(source[header.import.clone()].ends_with("\r\n"));
        assert!(source[header.show.clone()].starts_with("	#show"));
        assert_eq!(&source[header.end..], "Body");
    }

    #[test]
    fn non_managed_shapes_are_refused() {
        for source in [
            "#import \"/styles/a.typ\": preset
#show: preset.with(rhythm: 12pt)
",
            "#import \"/styles/a.typ\": preset, other
#show: preset.with(rhythm: goodtype_rhythm)
",
            "#import \"/styles/../a.typ\": preset
#show: preset.with(rhythm: goodtype_rhythm)
",
            "#import \"/styles/sub/a.typ\": preset
#show: preset.with(rhythm: goodtype_rhythm)
",
            "#import \"/other/a.typ\": preset
#show: preset.with(rhythm: goodtype_rhythm)
",
            "Text
#import \"/styles/a.typ\": preset
#show: preset.with(rhythm: goodtype_rhythm)
",
            "#import\"/styles/a.typ\": preset
#show: preset.with(rhythm: goodtype_rhythm)
",
            "/* open
#import \"/styles/a.typ\": preset
#show: preset.with(rhythm: goodtype_rhythm)
",
            "#show: preset.with(rhythm: goodtype_rhythm)
#import \"/styles/a.typ\": preset
",
        ] {
            assert!(super::parse_managed_header(source).is_none(), "{source:?}");
        }
    }
}
