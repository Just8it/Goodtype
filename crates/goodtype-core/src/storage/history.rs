//! Undo, redo, and the guarantee that a commit lands on the revision it was made against.
//!
//! The stacks hold whole page snapshots. They stay replayable only while the page on disk is
//! still the one they were built from, which is what the recorded revision and fingerprint
//! establish — any disagreement drops both stacks rather than writing over someone else's work.

use std::path::Path;

use serde::Serialize;

use super::{
    HISTORY_LIMIT, NotebookSnapshot, StorageError, invalid_error, pages::*, paths::*,
    validate::raise_schema_version, write::*,
};

#[derive(Debug, Default)]
pub struct NotebookHistory {
    undo: Vec<NotebookSnapshot>,
    redo: Vec<NotebookSnapshot>,
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
        push_history(&mut self.undo, previous.without_assets());
        self.redo.clear();
        self.record(saved, fingerprint);
    }

    /// Drop both stacks. They describe a page that is no longer the one on disk, so replaying
    /// either would write over whatever replaced it.
    fn forget(&mut self) {
        self.undo.clear();
        self.redo.clear();
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
    mut snapshot: NotebookSnapshot,
) -> Result<HistoryResult, StorageError> {
    let root = canonical_root(selected_root)?;
    let current = open_page(selected_root, &snapshot.page.id)?;
    ensure_current(&root, history, &current, snapshot.page.revision)?;
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
    let (saved, fingerprint) = save_and_return(&root, &current, &snapshot)?;
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
    let source = if undo { &history.undo } else { &history.redo };
    let mut target = source
        .last()
        .cloned()
        .ok_or_else(|| StorageError::InvalidNotebook("nothing to undo or redo".into()))?;
    target.page.revision = current.page.revision + 1;
    let (restored, fingerprint) = save_and_return(&root, &current, &target)?;

    if undo {
        history.undo.pop();
        push_history(&mut history.redo, current.without_assets());
    } else {
        history.redo.pop();
        push_history(&mut history.undo, current.without_assets());
    }
    history.record(&restored, fingerprint);
    Ok(history_result(restored, history))
}

fn ensure_current(
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
                return Err(StorageError::InvalidNotebook(
                    "external change detected; reopen the notebook before saving".into(),
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
    Err(StorageError::InvalidNotebook(format!(
        "revision conflict: expected {expected}, found {}",
        current.page.revision
    )))
}

pub(crate) fn push_history(stack: &mut Vec<NotebookSnapshot>, snapshot: NotebookSnapshot) {
    if stack.len() == HISTORY_LIMIT {
        stack.remove(0);
    }
    stack.push(snapshot);
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
    use crate::{
        PageObject, layout,
        storage::{fixtures::*, *},
    };
    use std::fs;

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
        assert!(matches!(
            error,
            StorageError::InvalidNotebook(message) if message.contains("external change")
        ));
        assert_eq!(
            fs::read(notebook_root.join("blocks").join("equation.typ")).unwrap(),
            external_source
        );
        assert_eq!(open_notebook(&notebook_root).unwrap().page.revision, 1);
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
        assert!(matches!(
            error,
            StorageError::InvalidNotebook(message) if message.contains("external change")
        ));
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

        let written = fs::metadata(notebook_root.join("ink/page-001-layer-001.json"))
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
