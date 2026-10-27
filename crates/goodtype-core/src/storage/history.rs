//! Undo, redo, and the guarantee that a commit lands on the revision it was made against.
//!
//! The stacks hold whole page snapshots. They stay replayable only while the page on disk is
//! still the one they were built from, which is what the recorded revision and fingerprint
//! establish — any disagreement drops both stacks rather than writing over someone else's work.

use std::path::Path;

use serde::Serialize;

use super::{
    HISTORY_BYTES_LIMIT, HISTORY_LIMIT, NotebookSnapshot, StorageError, invalid_error,
    pages::*,
    paths::*,
    validate::{raise_schema_version, validate_page_bundle_versions},
    write::*,
};

#[derive(Debug)]
struct HistoryEntry {
    snapshot: NotebookSnapshot,
    bytes: usize,
}

#[derive(Debug, Default)]
pub struct NotebookHistory {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    undo_bytes: usize,
    redo_bytes: usize,
    current_page_id: Option<String>,
    current_revision: Option<u64>,
    current_fingerprint: Option<u64>,
}

impl NotebookHistory {
    /// Adopt `snapshot` as the state the next operation is checked against.
    ///
    /// The three fields move together or not at all: a page ID without its matching fingerprint
    /// would let a stale commit through. Setting them by hand at each call site is what made that
    /// possible, so it is no longer possible.
    fn record(&mut self, snapshot: &NotebookSnapshot, fingerprint: u64) {
        self.current_page_id = Some(snapshot.page.id.clone());
        self.current_revision = Some(snapshot.page.revision);
        self.current_fingerprint = Some(fingerprint);
    }

    /// Land a new revision: `previous` becomes what undo returns to, and the redo stack goes,
    /// because a fresh commit makes anything that was redoable unreachable.
    ///
    /// Commit and recovery-restore both do exactly this, so they do it through one method rather
    /// than each reaching into the stacks.
    pub(crate) fn advance(
        &mut self,
        previous: NotebookSnapshot,
        saved: &NotebookSnapshot,
        fingerprint: u64,
    ) {
        push_history(
            &mut self.undo,
            &mut self.undo_bytes,
            previous.without_assets(),
        );
        self.redo.clear();
        self.redo_bytes = 0;
        self.record(saved, fingerprint);
    }

    /// Drop both stacks. They describe a page that is no longer the one on disk, so replaying
    /// either would write over whatever replaced it.
    fn forget(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.undo_bytes = 0;
        self.redo_bytes = 0;
    }

    fn describes(&self, snapshot: &NotebookSnapshot, fingerprint: u64) -> bool {
        self.current_page_id.as_deref() == Some(snapshot.page.id.as_str())
            && self.current_revision == Some(snapshot.page.revision)
            && self.current_fingerprint == Some(fingerprint)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryResult {
    pub snapshot: NotebookSnapshot,
    pub can_undo: bool,
    pub can_redo: bool,
}

pub fn observe_notebook(
    selected_root: &Path,
    history: &mut NotebookHistory,
) -> Result<NotebookSnapshot, StorageError> {
    let snapshot = open_notebook(selected_root)?;
    observe_snapshot(selected_root, history, snapshot)
}

pub fn observe_page(
    selected_root: &Path,
    page_id: &str,
    history: &mut NotebookHistory,
) -> Result<NotebookSnapshot, StorageError> {
    let snapshot = open_page(selected_root, page_id)?;
    observe_snapshot(selected_root, history, snapshot)
}

/// Observe a page and report its undo/redo availability. Used when focus moves between pages so
/// the frontend can target the viewed page and show its real history state. A page's history is
/// preserved across focus changes as long as its files are unchanged, so returning to a page you
/// edited keeps its undo stack.
pub fn focus_page(
    selected_root: &Path,
    page_id: &str,
    history: &mut NotebookHistory,
) -> Result<HistoryResult, StorageError> {
    let snapshot = observe_page(selected_root, page_id, history)?;
    Ok(history_result(snapshot, history))
}

fn observe_snapshot(
    selected_root: &Path,
    history: &mut NotebookHistory,
    snapshot: NotebookSnapshot,
) -> Result<NotebookSnapshot, StorageError> {
    let root = canonical_root(selected_root)?;
    let fingerprint = canonical_fingerprint(&root, &snapshot)?;
    // A page that is not the one the stacks were built against — or one whose files moved since —
    // makes those stacks unreplayable.
    if !history.describes(&snapshot, fingerprint) {
        history.forget();
    }
    history.record(&snapshot, fingerprint);
    Ok(snapshot)
}

pub fn commit_notebook(
    selected_root: &Path,
    history: &mut NotebookHistory,
    snapshot: NotebookSnapshot,
) -> Result<HistoryResult, StorageError> {
    let root = canonical_root(selected_root)?;
    let current = open_page(selected_root, &snapshot.page.id)?;
    ensure_current(&root, history, &current, snapshot.page.revision)?;
    commit_current(&root, history, current, snapshot)
}

pub(super) fn commit_current(
    root: &Path,
    history: &mut NotebookHistory,
    current: NotebookSnapshot,
    mut snapshot: NotebookSnapshot,
) -> Result<HistoryResult, StorageError> {
    validate_page_bundle_versions(&snapshot)?;
    // A preloaded neighbouring page may carry a timestamp from before another page saved.
    // Keep the current manifest and only advance its bookkeeping time.
    let requested_modified_at = snapshot.manifest.modified_at.clone();
    snapshot.manifest = current.manifest.clone();
    if requested_modified_at > snapshot.manifest.modified_at {
        snapshot.manifest.modified_at = requested_modified_at;
    }
    // A notebook rises to the version its content needs and never merely for being edited, so
    // this is the one place that can decide it: it holds both the notebook as it exists on disk
    // and the page about to be written. The manifest states the version and the page and its ink
    // layers move with it, so a partially upgraded notebook is not representable.
    raise_schema_version(&mut snapshot);
    snapshot.page.revision = current.page.revision + 1;
    let (saved, fingerprint) = save_and_return(root, &current, snapshot)?;
    history.advance(current, &saved, fingerprint);
    Ok(history_result(saved, history))
}

pub fn undo_notebook(
    selected_root: &Path,
    history: &mut NotebookHistory,
) -> Result<HistoryResult, StorageError> {
    restore_notebook(selected_root, history, true)
}

pub fn redo_notebook(
    selected_root: &Path,
    history: &mut NotebookHistory,
) -> Result<HistoryResult, StorageError> {
    restore_notebook(selected_root, history, false)
}

fn restore_notebook(
    selected_root: &Path,
    history: &mut NotebookHistory,
    undo: bool,
) -> Result<HistoryResult, StorageError> {
    let root = canonical_root(selected_root)?;
    let page_id = history
        .current_page_id
        .clone()
        .ok_or_else(|| invalid_error("page must be observed before undo or redo"))?;
    let current = open_page(&root, &page_id)?;
    ensure_current(
        &root,
        history,
        &current,
        history.current_revision.unwrap_or(current.page.revision),
    )?;
    let mut target = if undo {
        history.undo.last()
    } else {
        history.redo.last()
    }
    .map(|entry| entry.snapshot.clone())
    .ok_or_else(|| StorageError::InvalidNotebook("nothing to undo or redo".into()))?;
    // Undo restores this page's content. Page ordering and the notebook-wide schema maximum may
    // have advanced since the entry was retained and must never be resurrected from old history.
    target.manifest = current.manifest.clone();
    target.page.revision = current.page.revision + 1;
    let (restored, fingerprint) = save_and_return(&root, &current, target)?;

    if undo {
        history.undo_bytes -= history.undo.pop().expect("checked undo entry").bytes;
        push_history(
            &mut history.redo,
            &mut history.redo_bytes,
            current.without_assets(),
        );
    } else {
        history.redo_bytes -= history.redo.pop().expect("checked redo entry").bytes;
        push_history(
            &mut history.undo,
            &mut history.undo_bytes,
            current.without_assets(),
        );
    }
    history.record(&restored, fingerprint);
    Ok(history_result(restored, history))
}

pub(super) fn ensure_current(
    root: &Path,
    history: &mut NotebookHistory,
    current: &NotebookSnapshot,
    expected: u64,
) -> Result<(), StorageError> {
    if history.current_page_id.as_deref() == Some(current.page.id.as_str())
        && current.page.revision == expected
        && history
            .current_revision
            .is_none_or(|revision| revision == current.page.revision)
    {
        let fingerprint = canonical_fingerprint(root, current)?;
        match history.current_fingerprint {
            Some(observed) if observed == fingerprint => return Ok(()),
            Some(_) => {
                history.forget();
                return Err(StorageError::ExternalChange(
                    "reopen the notebook before saving".into(),
                ));
            }
            None => {
                return Err(StorageError::InvalidNotebook(
                    "notebook must be observed before saving".into(),
                ));
            }
        }
    }
    history.forget();
    Err(StorageError::RevisionConflict {
        expected,
        found: current.page.revision,
    })
}

fn push_history(stack: &mut Vec<HistoryEntry>, total: &mut usize, snapshot: NotebookSnapshot) {
    push_history_bounded(stack, total, snapshot, HISTORY_BYTES_LIMIT);
}

fn push_history_bounded(
    stack: &mut Vec<HistoryEntry>,
    total: &mut usize,
    snapshot: NotebookSnapshot,
    byte_limit: usize,
) {
    // This is a bounded payload proxy, not a claim about allocator or `String` capacity. Counting
    // encoded ink avoids turning every retained sample into JSON merely to enforce the budget.
    let Ok(bytes) = retained_payload_bytes(&snapshot) else {
        return;
    };
    if bytes > byte_limit {
        return;
    }
    while stack.len() == HISTORY_LIMIT || *total + bytes > byte_limit {
        *total -= stack.remove(0).bytes;
    }
    *total += bytes;
    stack.push(HistoryEntry { snapshot, bytes });
}

fn retained_payload_bytes(snapshot: &NotebookSnapshot) -> Result<usize, StorageError> {
    let mut bytes = serde_json::to_vec(&snapshot.manifest)?.len();
    bytes = bytes
        .checked_add(serde_json::to_vec(&snapshot.page)?.len())
        .ok_or_else(|| invalid_error("history payload size overflow"))?;
    for file in snapshot.blocks.iter().chain(&snapshot.assets) {
        bytes = bytes
            .checked_add(file.path.len())
            .and_then(|value| value.checked_add(file.bytes.len()))
            .ok_or_else(|| invalid_error("history payload size overflow"))?;
    }
    for layer in &snapshot.ink_layers {
        bytes = bytes
            .checked_add(crate::ink_codec::encoded_len(layer)?)
            .ok_or_else(|| invalid_error("history payload size overflow"))?;
    }
    Ok(bytes)
}

pub(crate) fn history_result(
    snapshot: NotebookSnapshot,
    history: &NotebookHistory,
) -> HistoryResult {
    HistoryResult {
        snapshot,
        can_undo: !history.undo.is_empty(),
        can_redo: !history.redo.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::push_history_bounded;
    use crate::{
        PageObject, layout,
        storage::{fixtures::*, *},
    };
    use std::fs;

    #[test]
    fn history_evicts_old_snapshots_at_its_byte_budget() {
        let value = snapshot().without_assets();
        let one_snapshot = super::retained_payload_bytes(&value).unwrap();
        let mut stack = Vec::new();
        let mut bytes = 0;
        push_history_bounded(&mut stack, &mut bytes, value.clone(), one_snapshot + 1);
        push_history_bounded(&mut stack, &mut bytes, value, one_snapshot + 1);
        assert_eq!(stack.len(), 1);
        assert!(bytes <= one_snapshot + 1);
    }

    #[test]
    fn committed_change_undoes_and_redoes_with_monotonic_revisions() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        let mut history = NotebookHistory::default();
        let mut changed = observe_notebook(&notebook_root, &mut history).unwrap();
        let PageObject::Image { fields, .. } = &mut changed.page.objects[1] else {
            panic!("image object missing");
        };
        fields.x = 144.0;

        let committed = commit_notebook(&notebook_root, &mut history, changed).unwrap();
        assert_eq!(committed.snapshot.page.revision, 2);
        assert!(committed.can_undo && !committed.can_redo);

        let undone = undo_notebook(&notebook_root, &mut history).unwrap();
        let PageObject::Image { fields, .. } = &undone.snapshot.page.objects[1] else {
            panic!("image object missing");
        };
        assert_eq!((undone.snapshot.page.revision, fields.x), (3, 72.0));
        assert!(!undone.can_undo && undone.can_redo);

        let redone = redo_notebook(&notebook_root, &mut history).unwrap();
        let PageObject::Image { fields, .. } = &redone.snapshot.page.objects[1] else {
            panic!("image object missing");
        };
        assert_eq!((redone.snapshot.page.revision, fields.x), (4, 144.0));
        assert!(redone.can_undo && !redone.can_redo);
    }

    #[test]
    fn rejects_an_external_change_without_overwriting_it() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        let mut history = NotebookHistory::default();
        let mut changed = observe_notebook(&notebook_root, &mut history).unwrap();
        let PageObject::Image { fields, .. } = &mut changed.page.objects[1] else {
            panic!("image object missing");
        };
        fields.x = 144.0;

        let external_source = b"$ external = true $";
        fs::write(
            notebook_root.join("blocks").join("equation.typ"),
            external_source,
        )
        .unwrap();

        let error = commit_notebook(&notebook_root, &mut history, changed).unwrap_err();
        assert!(matches!(error, StorageError::ExternalChange(_)));
        assert_eq!(
            fs::read(notebook_root.join("blocks").join("equation.typ")).unwrap(),
            external_source
        );
        assert_eq!(open_notebook(&notebook_root).unwrap().page.revision, 1);
    }

    #[test]
    fn commit_rejects_invalid_submitted_versions_before_normalizing() {
        let cases = [
            (0, 0),
            (crate::SCHEMA_VERSION + 1, crate::SCHEMA_VERSION + 1),
            (3, 2),
        ];
        for (page_version, layer_version) in cases {
            let temporary = tempfile::tempdir().unwrap();
            let root = temporary.path().join("notebook");
            create_notebook(&root, &snapshot()).unwrap();
            let ink_path = root.join("ink/page-001-layer-001.ink");
            let original_ink = fs::read(&ink_path).unwrap();
            let mut history = NotebookHistory::default();
            let mut candidate = observe_notebook(&root, &mut history).unwrap();
            candidate.page.schema_version = page_version;
            candidate.ink_layers[0].schema_version = layer_version;

            assert!(matches!(
                commit_notebook(&root, &mut history, candidate),
                Err(StorageError::InvalidNotebook(_))
            ));
            assert_eq!(open_notebook(&root).unwrap().page.revision, 1);
            assert_eq!(fs::read(&ink_path).unwrap(), original_ink);
        }
    }

    #[test]
    fn different_pages_can_commit_after_the_shared_modified_time_changes() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        create_page(
            &notebook_root,
            "2026-07-30T10:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();

        let mut first_history = NotebookHistory::default();
        let mut second_history = NotebookHistory::default();
        let mut first = observe_page(&notebook_root, "page-001", &mut first_history).unwrap();
        let mut second = observe_page(&notebook_root, "page-002", &mut second_history).unwrap();

        first.manifest.modified_at = "2026-07-30T10:01:00Z".into();
        first.ink_layers[0].strokes[0].points[0].x = 81.0;
        commit_notebook(&notebook_root, &mut first_history, first).unwrap();

        let mut stroke = snapshot().ink_layers[0].strokes[0].clone();
        stroke.id = "page-002-stroke-001".into();
        second.ink_layers[0].strokes.push(stroke);
        let committed = commit_notebook(&notebook_root, &mut second_history, second).unwrap();

        assert_eq!(committed.snapshot.page.revision, 2);
        assert_eq!(
            committed.snapshot.manifest.modified_at,
            "2026-07-30T10:01:00Z"
        );
    }

    #[test]
    fn observed_legacy_sibling_can_commit_after_another_page_migrates() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &legacy_snapshot()).unwrap();
        create_page(
            &root,
            "2026-09-10T08:00:00Z",
            &PagePosition::Last,
            None,
            None,
        )
        .unwrap();

        let mut first_history = NotebookHistory::default();
        let mut second_history = NotebookHistory::default();
        let first = observe_page(&root, "page-001", &mut first_history).unwrap();
        let second = observe_page(&root, "page-002", &mut second_history).unwrap();
        commit_notebook(&root, &mut first_history, first).unwrap();
        let second = commit_notebook(&root, &mut second_history, second)
            .unwrap()
            .snapshot;

        assert_eq!(second.manifest.schema_version, crate::SCHEMA_VERSION);
        assert_eq!(second.page.schema_version, crate::SCHEMA_VERSION);
        assert!(second.page.ink_layers[0].path.ends_with(".ink"));
    }

    #[test]
    fn undo_after_legacy_migration_keeps_schema_and_binary_reference() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &legacy_snapshot()).unwrap();
        let mut history = NotebookHistory::default();
        let mut candidate = observe_notebook(&root, &mut history).unwrap();
        candidate.ink_layers[0].strokes[0].points[0].x = 123.0;
        commit_notebook(&root, &mut history, candidate).unwrap();

        let undone = undo_notebook(&root, &mut history).unwrap().snapshot;
        assert_eq!(undone.manifest.schema_version, crate::SCHEMA_VERSION);
        assert_eq!(undone.page.schema_version, crate::SCHEMA_VERSION);
        assert!(undone.page.ink_layers[0].path.ends_with(".ink"));
        assert_eq!(undone.ink_layers[0].strokes[0].points[0].x, 80.0);
        assert!(root.join("ink/page-001-layer-001.json").is_file());
    }

    #[test]
    fn changing_manifest_content_is_still_an_external_change() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        let mut history = NotebookHistory::default();
        let mut changed = observe_notebook(&notebook_root, &mut history).unwrap();
        changed.ink_layers[0].strokes[0].points[0].x = 81.0;

        let manifest_path = notebook_root.join(layout::MANIFEST);
        let mut manifest: crate::NotebookManifest =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest.title = "Changed outside Goodtype".into();
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let error = commit_notebook(&notebook_root, &mut history, changed).unwrap_err();
        assert!(matches!(error, StorageError::ExternalChange(_)));
        assert_eq!(
            open_notebook(&notebook_root).unwrap().manifest.title,
            "Changed outside Goodtype"
        );
    }

    /// The Phase 1 gate: a page carrying five thousand strokes must commit, reopen, and
    /// still be readable. This is the check that the 8 MiB `MAX_JSON_BYTES` ceiling used
    /// to fail — after the write had already landed, leaving the notebook unopenable.
    #[test]
    fn commits_and_reopens_a_five_thousand_stroke_page() {
        const STROKES: usize = 5_000;
        const POINTS: usize = 60;

        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let mut history = NotebookHistory::default();
        let mut heavy = observe_notebook(&notebook_root, &mut history).unwrap();
        heavy.ink_layers = vec![handwriting_layer(STROKES, POINTS)];
        commit_notebook(&notebook_root, &mut history, heavy).unwrap();

        let reopened = open_notebook(&notebook_root).unwrap();
        assert_eq!(reopened.page.revision, 2);
        assert_eq!(reopened.ink_layers[0].strokes.len(), STROKES);
        assert_eq!(reopened.ink_layers[0].strokes[0].points.len(), POINTS);
        assert_eq!(
            reopened.ink_layers[0].strokes[STROKES - 1].id,
            format!("stroke-{:06}", STROKES - 1)
        );

        let written = fs::metadata(notebook_root.join("ink/page-001-layer-001.ink"))
            .unwrap()
            .len();
        assert!(
            written < MAX_INK_BYTES as u64,
            "a {STROKES}-stroke layer serialized to {written} bytes, at or above the \
             {MAX_INK_BYTES}-byte read ceiling; it would reopen as a broken notebook"
        );
        assert!(
            written > MAX_JSON_BYTES as u64,
            "a {STROKES}-stroke layer now serializes to {written} bytes, inside the \
             {MAX_JSON_BYTES}-byte structural ceiling. If the ink format got smaller, \
             retune the ink ceilings and drop this assertion."
        );
    }
}
