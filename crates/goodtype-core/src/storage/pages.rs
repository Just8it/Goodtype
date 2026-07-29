//! Opening a notebook, and the operations that change which pages it has.
//!
//! [`ManifestGuard`] is what makes those operations safe to interleave with another writer: it
//! records the manifest bytes a decision was made from, and refuses to commit if they moved.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    IdRemap, InkLayer, InkLayerReference, NotebookManifest, Page, PageBackground, PageGeometry,
    PageReference, SCHEMA_VERSION, SourceRole, layout,
};

use super::{
    HISTORY_LIMIT, MAX_IMAGE_BYTES, MAX_INK_BYTES, MAX_JSON_BYTES, NotebookSnapshot, StorageError,
    StoredFile, files::*, invalid, invalid_error, paths::*, recovery::*, validate::*, write::*,
};

pub fn create_notebook(
    selected_root: &Path,
    snapshot: &NotebookSnapshot,
) -> Result<(), StorageError> {
    fs::create_dir_all(selected_root)?;
    let root = canonical_root(selected_root)?;
    if root.join(layout::MANIFEST).exists() {
        return Err(StorageError::AlreadyExists(layout::MANIFEST.into()));
    }
    save_to_root(&root, snapshot).map(|_| ())
}

pub fn save_notebook(
    selected_root: &Path,
    snapshot: &NotebookSnapshot,
) -> Result<(), StorageError> {
    let root = canonical_root(selected_root)?;
    if resolve_existing(&root, layout::MANIFEST).is_err() {
        return Err(StorageError::InvalidNotebook(
            "goodtype.json does not exist".into(),
        ));
    }
    save_to_root(&root, snapshot).map(|_| ())
}

pub fn open_notebook(selected_root: &Path) -> Result<NotebookSnapshot, StorageError> {
    let root = canonical_root(selected_root)?;
    recover_interrupted_transaction(&root)?;
    let manifest: NotebookManifest = read_json(&root, layout::MANIFEST)?;
    validate_manifest(&manifest)?;
    validate_manifest_files(&root, &manifest)?;
    let page_id = manifest
        .pages
        .first()
        .expect("validated non-empty page list")
        .id
        .clone();
    load_page(&root, manifest, &page_id)
}

pub fn open_page(selected_root: &Path, page_id: &str) -> Result<NotebookSnapshot, StorageError> {
    let root = canonical_root(selected_root)?;
    recover_interrupted_transaction(&root)?;
    let manifest: NotebookManifest = read_json(&root, layout::MANIFEST)?;
    validate_manifest(&manifest)?;
    validate_manifest_files(&root, &manifest)?;
    load_page(&root, manifest, page_id)
}

fn load_page(
    root: &Path,
    manifest: NotebookManifest,
    page_id: &str,
) -> Result<NotebookSnapshot, StorageError> {
    let reference = page_reference(&manifest, page_id)?;
    let page: Page = read_json(root, &reference.path)?;
    let ink_layers = page
        .ink_layers
        .iter()
        .map(|reference| read_json_limited(root, &reference.path, MAX_INK_BYTES))
        .collect::<Result<Vec<_>, _>>()?;

    let block_paths = referenced_paths(&manifest, &page, SourceRole::Block);
    let asset_paths = referenced_paths(&manifest, &page, SourceRole::Asset);
    let blocks = read_stored_files(root, &block_paths, SourceRole::Block.max_bytes())?;
    let assets = read_stored_files(root, &asset_paths, MAX_IMAGE_BYTES)?;
    let snapshot = NotebookSnapshot {
        manifest,
        page,
        blocks,
        assets,
        ink_layers,
    };
    validate_snapshot(root, &snapshot)?;
    Ok(snapshot)
}

/// Where a new page lands in the manifest.
///
/// Not part of the on-disk format — it is an argument to the operation. Appending was never
/// enough: inserting before the current page is how a cover or a correction gets added, and
/// inserting after it is how you keep writing without walking back to the end.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PagePosition {
    Before { page_id: String },
    After { page_id: String },
    Last,
}

/// A read-modify-write of the manifest that will not land on top of someone else's change.
///
/// Opening records the exact bytes the decision is being made from; committing re-reads them and
/// refuses if they moved. Every operation that edits the page list goes through this, so one
/// added later cannot quietly skip the check — which, with the sequence written out by hand at
/// four call sites, was only a matter of time.
struct ManifestGuard {
    root: PathBuf,
    bytes: Vec<u8>,
    manifest: NotebookManifest,
}

impl ManifestGuard {
    fn read(selected_root: &Path) -> Result<Self, StorageError> {
        let root = canonical_root(selected_root)?;
        recover_interrupted_transaction(&root)?;
        let bytes = read_limited(resolve_existing(&root, layout::MANIFEST)?, MAX_JSON_BYTES)?;
        let manifest: NotebookManifest = serde_json::from_slice(&bytes)?;
        validate_manifest(&manifest)?;
        Ok(Self {
            root,
            bytes,
            manifest,
        })
    }

    /// `modified_at` is checked here because every operation that reaches the manifest stamps it,
    /// and every one of them used to validate it separately.
    fn open(selected_root: &Path, modified_at: &str) -> Result<Self, StorageError> {
        if modified_at.is_empty() || modified_at.len() > 64 {
            return invalid("page timestamp must be present and bounded");
        }
        Self::read(selected_root)
    }

    /// As [`Self::open`], and additionally resolve every page file the manifest names. Operations
    /// that build on the existing pages need this; ones that only reorder references do not.
    fn open_resolved(selected_root: &Path, modified_at: &str) -> Result<Self, StorageError> {
        let guard = Self::open(selected_root, modified_at)?;
        validate_manifest_files(&guard.root, &guard.manifest)?;
        Ok(guard)
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn ensure_unchanged(&self, operation: &str) -> Result<(), StorageError> {
        let current = read_limited(
            resolve_existing(&self.root, layout::MANIFEST)?,
            MAX_JSON_BYTES,
        )?;
        if current != self.bytes {
            return Err(invalid_error(&format!(
                "external change detected; reopen the notebook before {operation}"
            )));
        }
        Ok(())
    }

    /// Persist the manifest alone, for operations that only move references around.
    fn commit(self, operation: &str) -> Result<NotebookManifest, StorageError> {
        self.ensure_unchanged(operation)?;
        write_json(&self.root, layout::MANIFEST, &self.manifest)?;
        Ok(self.manifest)
    }

    /// Persist a whole snapshot, for operations that also write new page content.
    fn commit_snapshot(
        self,
        snapshot: &NotebookSnapshot,
        operation: &str,
    ) -> Result<(), StorageError> {
        self.ensure_unchanged(operation)?;
        save_to_root(&self.root, snapshot)?;
        Ok(())
    }
}

pub fn create_page(
    selected_root: &Path,
    modified_at: &str,
    position: &PagePosition,
    background: Option<&PageBackground>,
    geometry: Option<&PageGeometry>,
) -> Result<NotebookSnapshot, StorageError> {
    let mut guard = ManifestGuard::open_resolved(selected_root, modified_at)?;
    let root = guard.root().to_path_buf();
    let manifest = &mut guard.manifest;

    let FreshPage {
        id,
        page_path,
        ink_id,
        ink_path,
    } = fresh_page_identifiers(&root, manifest);

    let insert_at = match position {
        PagePosition::Last => manifest.pages.len(),
        PagePosition::Before { page_id } => neighbour_index(manifest, page_id)?,
        PagePosition::After { page_id } => neighbour_index(manifest, page_id)? + 1,
    };
    // The notebook default is the fallback, not the rule: picking a size or a template for one
    // page should not change what the next page comes out as.
    let geometry = geometry
        .cloned()
        .unwrap_or_else(|| manifest.default_page.geometry.clone());
    manifest.pages.insert(
        insert_at,
        PageReference {
            id: id.clone(),
            path: page_path,
            geometry: geometry.clone(),
        },
    );
    manifest.modified_at = modified_at.to_owned();
    let page = Page {
        schema_version: SCHEMA_VERSION,
        id: id.clone(),
        revision: 1,
        geometry,
        // The notebook default is the fallback, not the rule: picking a template for one page
        // should not change what the next blank page looks like.
        background: background
            .cloned()
            .unwrap_or_else(|| manifest.default_page.background.clone()),
        objects: Vec::new(),
        reading_order: Vec::new(),
        ink_layers: vec![InkLayerReference {
            id: ink_id.clone(),
            path: ink_path,
        }],
    };
    let snapshot = NotebookSnapshot {
        manifest: guard.manifest.clone(),
        page,
        blocks: Vec::new(),
        assets: Vec::new(),
        ink_layers: vec![InkLayer {
            schema_version: SCHEMA_VERSION,
            id: ink_id,
            page_id: id,
            strokes: Vec::new(),
        }],
    };
    validate_snapshot(&root, &snapshot)?;
    guard.commit_snapshot(&snapshot, "adding a page")?;
    open_page(&root, &snapshot.page.id)
}

/// Allocate a fresh identity for everything a duplicated page owns.
///
/// Assets and the shared style are deliberately absent from the source map: anything left
/// unmapped passes through [`IdRemap::source`] unchanged, which is exactly what "original
/// assets stay shared" means. Stroke IDs are keyed by their old ID rather than by position,
/// which the uniqueness rule in `validate_ink` already guarantees.
fn duplication_remap(source: &NotebookSnapshot, new_id: &str, modified_at: &str) -> IdRemap {
    let mut remap = IdRemap::new(modified_at);
    for (index, object) in source.page.objects.iter().enumerate() {
        remap.map_object(&object.fields().id, layout::object_id(new_id, index + 1));
    }
    for (index, reference) in source.page.ink_layers.iter().enumerate() {
        remap.map_ink_layer(
            &reference.id,
            layout::ink_layer_id(new_id, index + 1),
            &reference.path,
            layout::ink_layer_path(new_id, index + 1),
        );
    }
    let mut blocks = 0usize;
    for object in &source.page.objects {
        if let Some(file) = object.source()
            && file.role == SourceRole::Block
            && !remap.has_source(file.path)
        {
            blocks += 1;
            remap.map_source(file.path, layout::block_path(new_id, blocks));
        }
    }
    let mut strokes = 0usize;
    for layer in &source.ink_layers {
        for stroke in &layer.strokes {
            strokes += 1;
            remap.map_stroke(&stroke.id, layout::stroke_id(new_id, strokes));
        }
    }
    remap
}

/// Position of the page a new one is being placed next to. A stale id means the caller's view of
/// the notebook no longer matches the file, which is a refusal rather than a silent append.
fn neighbour_index(manifest: &NotebookManifest, page_id: &str) -> Result<usize, StorageError> {
    manifest
        .pages
        .iter()
        .position(|page| page.id == page_id)
        .ok_or_else(|| invalid_error("cannot place a page next to one this notebook does not have"))
}

/// A page identity nothing in the notebook is using yet.
struct FreshPage {
    id: String,
    page_path: String,
    ink_id: String,
    ink_path: String,
}

fn fresh_page_identifiers(root: &Path, manifest: &NotebookManifest) -> FreshPage {
    let mut number = manifest.pages.len() + 1;
    loop {
        let id = layout::page_id(number);
        let fresh = FreshPage {
            page_path: layout::page_path(&id),
            ink_id: layout::ink_layer_id(&id, 1),
            ink_path: layout::ink_layer_path(&id, 1),
            id,
        };
        if !manifest
            .pages
            .iter()
            .any(|page| page.id == fresh.id || page.path == fresh.page_path)
            && !root.join(&fresh.page_path).exists()
            && !root.join(&fresh.ink_path).exists()
        {
            return fresh;
        }
        number += 1;
    }
}

/// Duplicate one page with fresh page, object, group, ink-layer, stroke, and block IDs.
/// Original assets stay shared by reference; the shared style stays shared. The new page is
/// inserted directly after its source in the manifest.
pub fn duplicate_page(
    selected_root: &Path,
    page_id: &str,
    modified_at: &str,
) -> Result<NotebookSnapshot, StorageError> {
    let mut guard = ManifestGuard::open(selected_root, modified_at)?;
    let root = guard.root().to_path_buf();
    let source = open_page(&root, page_id)?;
    let manifest = &mut guard.manifest;

    let fresh = fresh_page_identifiers(&root, manifest);
    let (new_id, new_page_path) = (fresh.id, fresh.page_path);

    let remap = duplication_remap(&source, &new_id, modified_at);

    let ink_layers = source
        .ink_layers
        .iter()
        .map(|layer| {
            let strokes = layer
                .strokes
                .iter()
                .map(|stroke| {
                    Ok(crate::Stroke {
                        id: remap.stroke(&stroke.id)?,
                        group_id: remap.optional_object(stroke.group_id.as_deref())?,
                        ..stroke.clone()
                    })
                })
                .collect::<Result<Vec<_>, &'static str>>()?;
            Ok(InkLayer {
                schema_version: layer.schema_version,
                id: remap.ink_layer(&layer.id)?,
                page_id: new_id.clone(),
                strokes,
            })
        })
        .collect::<Result<Vec<_>, &'static str>>()
        .map_err(invalid_error)?;

    let page = Page {
        schema_version: SCHEMA_VERSION,
        id: new_id.clone(),
        revision: 1,
        geometry: source.page.geometry.clone(),
        background: source.page.background.clone(),
        objects: source
            .page
            .objects
            .iter()
            .map(|object| object.remapped(&remap))
            .collect::<Result<Vec<_>, _>>()
            .map_err(invalid_error)?,
        reading_order: source
            .page
            .reading_order
            .iter()
            .map(|id| remap.object(id))
            .collect::<Result<Vec<_>, _>>()
            .map_err(invalid_error)?,
        ink_layers: source
            .page
            .ink_layers
            .iter()
            .map(|reference| {
                Ok(InkLayerReference {
                    id: remap.ink_layer(&reference.id)?,
                    path: remap.ink_layer_path(&reference.path)?,
                })
            })
            .collect::<Result<Vec<_>, &'static str>>()
            .map_err(invalid_error)?,
    };

    let source_index = manifest
        .pages
        .iter()
        .position(|reference| reference.id == page_id)
        .ok_or_else(|| invalid_error("page is not referenced by the notebook manifest"))?;
    manifest.pages.insert(
        source_index + 1,
        PageReference {
            id: new_id.clone(),
            path: new_page_path,
            geometry: source.page.geometry.clone(),
        },
    );
    manifest.modified_at = modified_at.to_owned();

    let blocks = source
        .blocks
        .iter()
        .map(|file| StoredFile {
            path: remap.source(&file.path),
            bytes: file.bytes.clone(),
        })
        .collect();

    let snapshot = NotebookSnapshot {
        manifest: guard.manifest.clone(),
        page,
        blocks,
        assets: Vec::new(),
        ink_layers,
    };
    validate_snapshot(&root, &snapshot)?;
    guard.commit_snapshot(&snapshot, "duplicating a page")?;
    open_page(&root, &new_id)
}

/// Remove one page's manifest reference transactionally. Canonical page, ink, block, and asset
/// files are retained for recovery; only the reference disappears. Returns the bundle of the
/// nearest remaining page.
pub fn delete_page(
    selected_root: &Path,
    page_id: &str,
    modified_at: &str,
) -> Result<NotebookSnapshot, StorageError> {
    let mut guard = ManifestGuard::open(selected_root, modified_at)?;
    let root = guard.root().to_path_buf();
    let manifest = &mut guard.manifest;
    if manifest.pages.len() < 2 {
        return invalid("the final remaining page cannot be deleted");
    }
    let index = manifest
        .pages
        .iter()
        .position(|reference| reference.id == page_id)
        .ok_or_else(|| invalid_error("page is not referenced by the notebook manifest"))?;
    manifest.pages.remove(index);
    manifest.modified_at = modified_at.to_owned();

    let manifest = guard.commit("deleting a page")?;
    let neighbor = manifest.pages[index.min(manifest.pages.len() - 1)]
        .id
        .clone();
    open_page(&root, &neighbor)
}

/// Reorder the manifest page list. `ordered_ids` must be a permutation of the current page IDs;
/// page files, IDs, and paths are untouched.
pub fn reorder_pages(
    selected_root: &Path,
    ordered_ids: &[String],
    modified_at: &str,
    active_page_id: &str,
) -> Result<NotebookSnapshot, StorageError> {
    let mut guard = ManifestGuard::open(selected_root, modified_at)?;
    let root = guard.root().to_path_buf();
    let manifest = &mut guard.manifest;

    let current: HashSet<&str> = manifest.pages.iter().map(|page| page.id.as_str()).collect();
    let requested: HashSet<&str> = ordered_ids.iter().map(String::as_str).collect();
    if ordered_ids.len() != manifest.pages.len() || current != requested {
        return invalid("reorder must list every current page exactly once");
    }

    let mut by_id: HashMap<&str, PageReference> = manifest
        .pages
        .iter()
        .map(|page| (page.id.as_str(), page.clone()))
        .collect();
    manifest.pages = ordered_ids
        .iter()
        .map(|id| by_id.remove(id.as_str()).expect("validated permutation"))
        .collect();
    manifest.modified_at = modified_at.to_owned();

    guard.commit("reordering pages")?;
    open_page(&root, active_page_id)
}

#[derive(Clone, Debug)]
struct StructureSnapshot {
    manifest: NotebookManifest,
    active_page_id: String,
}

/// Session-only history for changes to the manifest page list.
///
/// Page files are deliberately absent: create and duplicate write fresh immutable identities,
/// while delete only removes a manifest reference. Restoring the manifest is therefore enough
/// to undo every page-list operation without copying page payloads into memory.
#[derive(Debug, Default)]
pub struct NotebookStructureHistory {
    undo: Vec<StructureSnapshot>,
    redo: Vec<StructureSnapshot>,
    current: Option<StructureSnapshot>,
    current_manifest_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureHistoryResult {
    pub snapshot: NotebookSnapshot,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// Observe the page list before a structure operation. An outside manifest change invalidates
/// the session history rather than becoming something undo may overwrite.
pub fn observe_structure(
    selected_root: &Path,
    active_page_id: &str,
    history: &mut NotebookStructureHistory,
) -> Result<(), StorageError> {
    let guard = ManifestGuard::read(selected_root)?;
    page_reference(&guard.manifest, active_page_id)?;
    if history
        .current_manifest_bytes
        .as_ref()
        .is_some_and(|bytes| bytes != &guard.bytes)
    {
        history.undo.clear();
        history.redo.clear();
    }
    history.current = Some(StructureSnapshot {
        manifest: guard.manifest,
        active_page_id: active_page_id.to_owned(),
    });
    history.current_manifest_bytes = Some(guard.bytes);
    Ok(())
}

/// Record the manifest produced by a successful page-list operation.
pub fn advance_structure(
    selected_root: &Path,
    snapshot: &NotebookSnapshot,
    history: &mut NotebookStructureHistory,
) -> Result<StructureHistoryResult, StorageError> {
    let previous = history
        .current
        .take()
        .ok_or_else(|| invalid_error("page structure must be observed before changing it"))?;
    push_structure(&mut history.undo, previous);
    history.redo.clear();
    let guard = ManifestGuard::read(selected_root)?;
    history.current = Some(StructureSnapshot {
        manifest: guard.manifest,
        active_page_id: snapshot.page.id.clone(),
    });
    history.current_manifest_bytes = Some(guard.bytes);
    Ok(structure_result(snapshot.clone(), history))
}

pub fn undo_structure(
    selected_root: &Path,
    modified_at: &str,
    history: &mut NotebookStructureHistory,
) -> Result<StructureHistoryResult, StorageError> {
    restore_structure(selected_root, modified_at, history, true)
}

pub fn redo_structure(
    selected_root: &Path,
    modified_at: &str,
    history: &mut NotebookStructureHistory,
) -> Result<StructureHistoryResult, StorageError> {
    restore_structure(selected_root, modified_at, history, false)
}

fn restore_structure(
    selected_root: &Path,
    modified_at: &str,
    history: &mut NotebookStructureHistory,
    undo: bool,
) -> Result<StructureHistoryResult, StorageError> {
    let mut guard = ManifestGuard::open(selected_root, modified_at)?;
    if history.current_manifest_bytes.as_ref() != Some(&guard.bytes) {
        history.undo.clear();
        history.redo.clear();
        return Err(invalid_error(
            "external change detected; reopen the notebook before changing page history",
        ));
    }
    let source = if undo {
        &mut history.undo
    } else {
        &mut history.redo
    };
    let mut target = source
        .pop()
        .ok_or_else(|| invalid_error("nothing to undo or redo in page structure"))?;
    let current = history
        .current
        .take()
        .ok_or_else(|| invalid_error("page structure must be observed before undo or redo"))?;
    target.manifest.modified_at = modified_at.to_owned();
    validate_manifest_files(guard.root(), &target.manifest)?;
    guard.manifest = target.manifest.clone();
    let root = guard.root().to_path_buf();
    guard.commit(if undo {
        "undoing a page-list change"
    } else {
        "redoing a page-list change"
    })?;
    let snapshot = open_page(&root, &target.active_page_id)?;

    if undo {
        push_structure(&mut history.redo, current);
    } else {
        push_structure(&mut history.undo, current);
    }
    let observed = ManifestGuard::read(&root)?;
    history.current = Some(StructureSnapshot {
        manifest: observed.manifest,
        active_page_id: target.active_page_id,
    });
    history.current_manifest_bytes = Some(observed.bytes);
    Ok(structure_result(snapshot, history))
}

fn push_structure(stack: &mut Vec<StructureSnapshot>, snapshot: StructureSnapshot) {
    if stack.len() == HISTORY_LIMIT {
        stack.remove(0);
    }
    stack.push(snapshot);
}

fn structure_result(
    snapshot: NotebookSnapshot,
    history: &NotebookStructureHistory,
) -> StructureHistoryResult {
    StructureHistoryResult {
        snapshot,
        can_undo: !history.undo.is_empty(),
        can_redo: !history.redo.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        PageObject,
        storage::{fixtures::*, *},
    };
    use std::path::Path;

    #[test]
    fn creates_saves_and_reopens_one_page() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let mut reopened = open_notebook(&notebook_root).unwrap();
        assert_eq!(reopened.page.revision, 1);
        assert_eq!(reopened.ink_layers[0].strokes.len(), 1);
        reopened.page.revision = 2;
        if let PageObject::Image { fields, .. } = &mut reopened.page.objects[1] {
            fields.x = 144.0;
            fields.scale = 1.5;
        }
        save_notebook(&notebook_root, &reopened).unwrap();

        let saved = open_notebook(&notebook_root).unwrap();
        assert_eq!(saved.page.revision, 2);
        let PageObject::Image { fields, .. } = &saved.page.objects[1] else {
            panic!("image object missing");
        };
        assert_eq!((fields.x, fields.scale), (144.0, 1.5));
    }

    #[test]
    fn creates_and_commits_pages_independently() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let second = create_page(
            &notebook_root,
            "2026-07-23T18:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        assert_eq!(second.manifest.pages.len(), 2);
        assert_eq!(second.page.id, "page-002");
        assert!(second.page.objects.is_empty());

        let mut history = NotebookHistory::default();
        let mut changed = observe_page(&notebook_root, "page-002", &mut history).unwrap();
        let mut stroke = snapshot().ink_layers[0].strokes[0].clone();
        stroke.id = "page-002-stroke-001".into();
        changed.ink_layers[0].strokes.push(stroke);
        commit_notebook(&notebook_root, &mut history, changed).unwrap();

        let first = open_page(&notebook_root, "page-001").unwrap();
        let second = open_page(&notebook_root, "page-002").unwrap();
        assert_eq!(first.page.revision, 1);
        assert_eq!(first.ink_layers[0].strokes.len(), 1);
        assert_eq!(second.page.revision, 2);
        assert_eq!(second.ink_layers[0].strokes.len(), 1);
        assert_eq!(
            second
                .manifest
                .pages
                .iter()
                .map(|page| page.id.as_str())
                .collect::<Vec<_>>(),
            ["page-001", "page-002"]
        );
    }

    #[test]
    fn multipage_fixture_opens_every_page_without_loading_all() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/notebooks/multipage");

        let first = open_notebook(&fixture).unwrap();
        assert_eq!(first.manifest.pages.len(), 3);
        assert_eq!(first.page.id, "page-001");
        assert_eq!(first.ink_layers[0].strokes.len(), 1);

        let second = open_page(&fixture, "page-002").unwrap();
        assert!(matches!(second.page.objects[0], PageObject::Image { .. }));
        assert_eq!(second.ink_layers[0].strokes[0].id, "page-002-stroke-0001");

        let third = open_page(&fixture, "page-003").unwrap();
        assert!(third.ink_layers[0].strokes.is_empty());

        // Search reaches every page's Typst source in manifest order.
        let hits = search_notebook(&fixture, "momentum").unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.page_number).collect::<Vec<_>>(),
            [1, 3]
        );
    }

    #[test]
    fn opens_edges_of_a_hundred_page_notebook() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        for minute in 1..100 {
            create_page(
                &notebook_root,
                &format!("2026-07-29T10:{:02}:00Z", minute % 60),
                &PagePosition::Last,
                None,
                None,
            )
            .unwrap();
        }

        let opened = open_notebook(&notebook_root).unwrap();
        assert_eq!(opened.manifest.pages.len(), 100);
        assert_eq!(
            open_page(&notebook_root, "page-050").unwrap().page.id,
            "page-050"
        );
        assert_eq!(
            open_page(&notebook_root, "page-100").unwrap().page.id,
            "page-100"
        );
    }

    #[test]
    fn duplicates_a_page_with_fresh_ids_and_shared_originals() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let copy = duplicate_page(&notebook_root, "page-001", "2026-07-23T19:00:00Z").unwrap();
        assert_eq!(copy.page.id, "page-002");
        assert_eq!(copy.page.revision, 1);
        assert_eq!(copy.page.objects.len(), 2);
        assert_eq!(copy.ink_layers[0].strokes.len(), 1);
        // Fresh identity everywhere the page owns one.
        assert!(
            copy.page
                .objects
                .iter()
                .all(|object| { object.fields().id.starts_with("page-002-obj-") })
        );
        assert!(copy.ink_layers[0].id.starts_with("page-002-ink-"));
        assert!(
            copy.ink_layers[0].strokes[0]
                .id
                .starts_with("page-002-stroke-")
        );
        let PageObject::Typst { source_path, .. } = &copy.page.objects[0] else {
            panic!("typst object missing");
        };
        assert!(source_path.starts_with("blocks/page-002-block-"));
        // Shared originals stay shared.
        let PageObject::Image { source_path, .. } = &copy.page.objects[1] else {
            panic!("image object missing");
        };
        assert_eq!(source_path, "assets/diagram.png");

        // The source page is untouched and both pages reopen.
        let original = open_page(&notebook_root, "page-001").unwrap();
        assert_eq!(original.page.revision, 1);
        assert_eq!(original.ink_layers[0].strokes[0].id, "stroke-001");
        assert_eq!(
            copy.manifest
                .pages
                .iter()
                .map(|page| page.id.as_str())
                .collect::<Vec<_>>(),
            ["page-001", "page-002"]
        );
    }

    #[test]
    fn new_pages_land_where_the_writer_put_them() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let order = |snapshot: &NotebookSnapshot| -> Vec<String> {
            snapshot
                .manifest
                .pages
                .iter()
                .map(|page| page.id.clone())
                .collect()
        };

        let appended = create_page(
            &notebook_root,
            "2026-07-25T09:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        assert_eq!(order(&appended), ["page-001", "page-002"]);

        let before = create_page(
            &notebook_root,
            "2026-07-25T09:01:00Z",
            &PagePosition::Before {
                page_id: "page-001".into(),
            },
            None,
            None,
        )
        .unwrap();
        assert_eq!(order(&before), ["page-003", "page-001", "page-002"]);
        assert_eq!(before.page.id, "page-003");

        let after = create_page(
            &notebook_root,
            "2026-07-25T09:02:00Z",
            &PagePosition::After {
                page_id: "page-001".into(),
            },
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            order(&after),
            ["page-003", "page-001", "page-004", "page-002"]
        );

        // A neighbour the notebook does not have is a stale view, not a reason to append.
        assert!(
            create_page(
                &notebook_root,
                "2026-07-25T09:03:00Z",
                &PagePosition::After {
                    page_id: "page-404".into(),
                },
                None,
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn deletes_and_reorders_pages_with_guards() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        create_page(
            &notebook_root,
            "2026-07-23T19:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        create_page(
            &notebook_root,
            "2026-07-23T19:01:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();

        let reordered = reorder_pages(
            &notebook_root,
            &["page-003".into(), "page-001".into(), "page-002".into()],
            "2026-07-23T19:02:00Z",
            "page-001",
        )
        .unwrap();
        assert_eq!(
            reordered
                .manifest
                .pages
                .iter()
                .map(|page| page.id.as_str())
                .collect::<Vec<_>>(),
            ["page-003", "page-001", "page-002"]
        );

        let error = reorder_pages(
            &notebook_root,
            &["page-001".into(), "page-002".into()],
            "2026-07-23T19:03:00Z",
            "page-001",
        )
        .unwrap_err();
        assert!(matches!(error, StorageError::InvalidNotebook(_)));

        let after_delete = delete_page(&notebook_root, "page-001", "2026-07-23T19:04:00Z").unwrap();
        assert_eq!(after_delete.manifest.pages.len(), 2);
        // Deletion removes the reference, never the canonical files.
        assert!(notebook_root.join("pages/page-001.json").is_file());
        assert!(notebook_root.join("ink/page-001-layer-001.json").is_file());

        delete_page(&notebook_root, "page-002", "2026-07-23T19:05:00Z").unwrap();
        let error = delete_page(&notebook_root, "page-003", "2026-07-23T19:06:00Z").unwrap_err();
        assert!(matches!(error, StorageError::InvalidNotebook(_)));
    }

    #[test]
    fn page_structure_changes_undo_and_redo_without_deleting_page_files() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        let mut history = NotebookStructureHistory::default();

        observe_structure(&notebook_root, "page-001", &mut history).unwrap();
        let added = create_page(
            &notebook_root,
            "2026-07-29T10:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();
        let added = advance_structure(&notebook_root, &added, &mut history).unwrap();
        assert_eq!(added.snapshot.manifest.pages.len(), 2);
        assert!(added.can_undo && !added.can_redo);

        let undone = undo_structure(&notebook_root, "2026-07-29T10:01:00Z", &mut history).unwrap();
        assert_eq!(undone.snapshot.page.id, "page-001");
        assert_eq!(undone.snapshot.manifest.pages.len(), 1);
        assert!(notebook_root.join("pages/page-002.json").is_file());
        assert!(!undone.can_undo && undone.can_redo);

        let redone = redo_structure(&notebook_root, "2026-07-29T10:02:00Z", &mut history).unwrap();
        assert_eq!(redone.snapshot.page.id, "page-002");
        assert_eq!(redone.snapshot.manifest.pages.len(), 2);
        assert!(redone.can_undo && !redone.can_redo);
    }
}
