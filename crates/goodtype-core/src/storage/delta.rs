//! Compact transport only. Reconstruction still passes the complete canonical validator/save path.
use super::{
    HistoryResult, MAX_INK_STROKES_PER_LAYER, NotebookHistory, StorageError, StoredFile,
    invalid_error, open_page, paths::canonical_root,
};
use crate::{InkLayer, Page, Stroke, Transform};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageDelta {
    pub page: Page,
    pub modified_at: String,
    pub blocks: Vec<StoredFile>,
    pub assets: Vec<StoredFile>,
    pub known_assets: HashMap<String, String>,
    pub ink_layers: Vec<InkLayerDelta>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InkLayerDelta {
    pub schema_version: u32,
    pub id: String,
    pub page_id: String,
    pub strokes: Vec<StrokeDelta>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum StrokeDelta {
    Reuse {
        id: String,
        transform: Option<Transform>,
    },
    Put {
        stroke: Stroke,
    },
}

pub fn commit_page_delta(
    root: &Path,
    history: &mut NotebookHistory,
    delta: PageDelta,
) -> Result<HistoryResult, StorageError> {
    let root = canonical_root(root)?;
    let current = open_page(&root, &delta.page.id)?;
    super::history::ensure_current(&root, history, &current, delta.page.revision)?;
    let mut candidate = super::NotebookSnapshot {
        manifest: current.manifest.clone(),
        page: current.page.clone(),
        blocks: current.blocks.clone(),
        ink_layers: current.ink_layers.clone(),
        assets: delta.assets,
    };
    let mut layers: HashMap<_, _> = std::mem::take(&mut candidate.ink_layers)
        .into_iter()
        .map(|layer| {
            (
                layer.id,
                layer
                    .strokes
                    .into_iter()
                    .map(|stroke| (stroke.id.clone(), stroke))
                    .collect::<HashMap<_, _>>(),
            )
        })
        .collect();
    candidate.ink_layers = delta
        .ink_layers
        .into_iter()
        .map(|layer| {
            if layer.strokes.len() > MAX_INK_STROKES_PER_LAYER {
                return Err(invalid_error("too many strokes in delta"));
            }
            let mut existing = layers.remove(&layer.id).unwrap_or_default();
            let strokes = layer
                .strokes
                .into_iter()
                .map(|change| match change {
                    StrokeDelta::Put { stroke } => Ok(stroke),
                    StrokeDelta::Reuse { id, transform } => {
                        let mut stroke = existing
                            .remove(&id)
                            .ok_or_else(|| invalid_error("unknown or repeated stroke reference"))?;
                        if let Some(transform) = transform {
                            stroke.transform = transform;
                        }
                        Ok(stroke)
                    }
                })
                .collect::<Result<Vec<_>, StorageError>>()?;
            Ok(InkLayer {
                schema_version: layer.schema_version,
                id: layer.id,
                page_id: layer.page_id,
                strokes,
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    candidate.page = delta.page;
    candidate.manifest.modified_at = delta.modified_at;
    let referenced: HashSet<String> = super::validate::referenced_paths(
        &candidate.manifest,
        &candidate.page,
        crate::SourceRole::Block,
    )
    .into_iter()
    .map(str::to_owned)
    .collect();
    candidate
        .blocks
        .retain(|file| referenced.contains(&file.path));
    let mut supplied = HashSet::new();
    for file in delta.blocks {
        if !referenced.contains(&file.path) || !supplied.insert(file.path.clone()) {
            return Err(invalid_error("unexpected or duplicate delta block"));
        }
        candidate.blocks.retain(|old| old.path != file.path);
        candidate.blocks.push(file);
    }
    super::history::commit_current(&root, history, current, candidate)
}

#[cfg(test)]
mod tests {
    use super::super::{fixtures::snapshot, *};
    use super::*;
    use serde_json::json;

    fn delta(snapshot: &NotebookSnapshot, strokes: serde_json::Value) -> PageDelta {
        serde_json::from_value(json!({
            "page": snapshot.page, "modifiedAt": snapshot.manifest.modified_at,
            "blocks": [], "assets": [], "knownAssets": {},
            "inkLayers": [{ "schemaVersion": snapshot.page.schema_version,
                "id": snapshot.ink_layers[0].id, "pageId": snapshot.page.id, "strokes": strokes }]
        }))
        .unwrap()
    }

    #[test]
    fn compact_changes_preserve_samples_order_originals_and_history() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let mut history = NotebookHistory::default();
        let before = observe_notebook(&root, &mut history).unwrap();
        let mut added = before.ink_layers[0].strokes[0].clone();
        added.id = "added".into();
        let mut transform = added.transform.clone();
        transform.translate_x = 45.0;
        let changed = commit_page_delta(
            &root,
            &mut history,
            delta(
                &before,
                json!([
                    { "kind": "put", "stroke": added },
                    { "kind": "reuse", "id": "stroke-001", "transform": transform }
                ]),
            ),
        )
        .unwrap();
        assert_eq!(changed.snapshot.ink_layers[0].strokes[0].id, "added");
        assert_eq!(
            changed.snapshot.ink_layers[0].strokes[1]
                .transform
                .translate_x,
            45.0
        );
        assert_eq!(
            serde_json::to_value(&changed.snapshot.ink_layers[0].strokes[1].points).unwrap(),
            serde_json::to_value(&before.ink_layers[0].strokes[0].points).unwrap()
        );
        assert_eq!(changed.snapshot.assets[0].bytes, before.assets[0].bytes);
        let undone = undo_notebook(&root, &mut history).unwrap();
        assert_eq!(undone.snapshot.ink_layers[0].strokes.len(), 1);
        let redone = redo_notebook(&root, &mut history).unwrap();
        assert_eq!(redone.snapshot.ink_layers[0].strokes.len(), 2);
        let removed = commit_page_delta(
            &root,
            &mut history,
            delta(
                &redone.snapshot,
                json!([{ "kind": "reuse", "id": "added" }]),
            ),
        )
        .unwrap();
        assert_eq!(removed.snapshot.ink_layers[0].strokes.len(), 1);
        assert_eq!(
            open_page(&root, &before.page.id).unwrap().ink_layers[0].strokes[0].id,
            "added"
        );
    }

    #[test]
    fn bad_references_versions_and_external_changes_never_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let mut history = NotebookHistory::default();
        let before = observe_notebook(&root, &mut history).unwrap();
        for changes in [
            json!([{ "kind": "reuse", "id": "missing" }]),
            json!([{ "kind": "reuse", "id": "stroke-001" }, { "kind": "reuse", "id": "stroke-001" }]),
            json!([{ "kind": "reuse", "id": "stroke-001", "transform": {
                "translateX": 0, "translateY": 0, "scaleX": -1, "scaleY": 1, "rotation": 0 } }]),
        ] {
            assert!(commit_page_delta(&root, &mut history, delta(&before, changes)).is_err());
            assert_eq!(
                open_page(&root, &before.page.id).unwrap().page.revision,
                before.page.revision
            );
        }
        let mut stale = delta(&before, json!([]));
        stale.page.revision += 1;
        assert!(
            commit_page_delta(&root, &mut history, stale)
                .unwrap_err()
                .to_string()
                .contains("revision conflict")
        );
        let block = root.join(&before.blocks[0].path);
        std::fs::write(&block, "external").unwrap();
        assert!(
            commit_page_delta(&root, &mut history, delta(&before, json!([])))
                .unwrap_err()
                .to_string()
                .contains("external change")
        );
        assert_eq!(std::fs::read_to_string(block).unwrap(), "external");
    }

    #[test]
    fn removing_a_block_drops_its_transport_bytes_but_preserves_its_original_file() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let mut history = NotebookHistory::default();
        let before = observe_notebook(&root, &mut history).unwrap();
        let mut change = delta(&before, json!([{ "kind": "reuse", "id": "stroke-001" }]));
        change.page.objects.remove(0);
        change.page.reading_order.remove(0);
        if let crate::PageObject::Image { fields, .. } = &mut change.page.objects[0] {
            fields.reading_order = 0;
        }
        let saved = commit_page_delta(&root, &mut history, change).unwrap();
        assert!(saved.snapshot.blocks.is_empty());
        assert!(root.join(&before.blocks[0].path).exists());
    }
}
