use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use goodtype_core::storage::{
    self, HistoryResult, NotebookHistory, NotebookSnapshot, NotebookStructureHistory, PagePosition,
    RecoveryCandidate, SearchHit, StructureHistoryResult,
};

use goodtype_core::{PageBackground, PageGeometry};
use tauri_plugin_dialog::DialogExt;

use crate::workspace::{AllowedRoots, ensure_allowed};

#[derive(Clone, Default)]
// ponytail: one global lock is sufficient for one notebook window; split by root when multiple
// independent notebook windows need concurrent storage throughput.
pub struct NotebookHistories(Arc<Mutex<HistoryStore>>);

#[derive(Default)]
struct HistoryStore {
    pages: HashMap<(PathBuf, String), NotebookHistory>,
    structure: HashMap<PathBuf, NotebookStructureHistory>,
}

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn with_notebook_lock<T>(
    histories: &NotebookHistories,
    operation: impl FnOnce(&mut HistoryStore) -> Result<T, String>,
) -> Result<T, String> {
    let mut histories = histories.0.lock().map_err(message)?;
    operation(&mut histories)
}

pub(crate) fn with_notebook<T>(
    histories: &NotebookHistories,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    with_notebook_lock(histories, |_| operation())
}

/// Observe a page under the shared history map. Every history-bearing command funnels through
/// here so the (root, page) keying stays in one place.
fn with_history<T>(
    histories: &NotebookHistories,
    root: PathBuf,
    page_id: String,
    operation: impl FnOnce(&PathBuf, &mut NotebookHistory) -> Result<T, storage::StorageError>,
) -> Result<T, String> {
    with_notebook_lock(histories, |histories| {
        let history = histories.pages.entry((root.clone(), page_id)).or_default();
        operation(&root, history).map_err(message)
    })
}

/// A manifest change invalidates every page fingerprint because the manifest is part of each
/// page's canonical file set. Record the structure action, drop those histories, and observe the
/// page returned by the change.
fn advance_structure_result(
    histories: &mut HistoryStore,
    root: &Path,
    snapshot: NotebookSnapshot,
) -> Result<StructureHistoryResult, String> {
    let result = storage::advance_structure(
        root,
        &snapshot,
        histories.structure.entry(root.to_path_buf()).or_default(),
    )
    .map_err(message)?;
    histories
        .pages
        .retain(|(known_root, _), _| known_root != root);
    let page_id = result.snapshot.page.id.clone();
    let history = histories
        .pages
        .entry((root.to_path_buf(), page_id.clone()))
        .or_default();
    let snapshot = storage::observe_page(root, &page_id, history).map_err(message)?;
    Ok(StructureHistoryResult { snapshot, ..result })
}

fn observe_structure(
    histories: &mut HistoryStore,
    root: &Path,
    active_page_id: &str,
) -> Result<(), String> {
    storage::observe_structure(
        root,
        active_page_id,
        histories.structure.entry(root.to_path_buf()).or_default(),
    )
    .map_err(message)
}

fn restore_structure_result(
    histories: &mut HistoryStore,
    root: &Path,
    modified_at: &str,
    undo: bool,
) -> Result<StructureHistoryResult, String> {
    let structure = histories.structure.entry(root.to_path_buf()).or_default();
    let result = if undo {
        storage::undo_structure(root, modified_at, structure)
    } else {
        storage::redo_structure(root, modified_at, structure)
    }
    .map_err(message)?;
    histories
        .pages
        .retain(|(known_root, _), _| known_root != root);
    let page_id = result.snapshot.page.id.clone();
    let history = histories
        .pages
        .entry((root.to_path_buf(), page_id.clone()))
        .or_default();
    let snapshot = storage::observe_page(root, &page_id, history).map_err(message)?;
    Ok(StructureHistoryResult { snapshot, ..result })
}

#[tauri::command]
pub async fn create_notebook(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    snapshot: NotebookSnapshot,
) -> Result<(), String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook_lock(&histories, |history_map| {
            storage::create_notebook(&root, &snapshot).map_err(message)?;
            observe_structure(history_map, &root, &snapshot.page.id)?;
            let history = history_map
                .pages
                .entry((root.clone(), snapshot.page.id.clone()))
                .or_default();
            storage::observe_page(&root, &snapshot.page.id, history)
                .map(|_| ())
                .map_err(message)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn open_notebook(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
) -> Result<NotebookSnapshot, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook_lock(&histories, |history_map| {
            let snapshot = storage::open_notebook(&root).map_err(message)?;
            observe_structure(history_map, &root, &snapshot.page.id)?;
            let history = history_map
                .pages
                .entry((root.clone(), snapshot.page.id.clone()))
                .or_default();
            storage::observe_page(&root, &snapshot.page.id, history).map_err(message)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn open_page(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
) -> Result<NotebookSnapshot, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_history(&histories, root, page_id.clone(), |root, history| {
            storage::observe_page(root, &page_id, history)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn focus_page(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
) -> Result<HistoryResult, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_history(&histories, root, page_id.clone(), |root, history| {
            storage::focus_page(root, &page_id, history)
        })
    })
    .await
    .map_err(message)?
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePageRequest {
    modified_at: String,
    position: PagePosition,
    background: Option<PageBackground>,
    geometry: Option<PageGeometry>,
    active_page_id: String,
}

#[tauri::command]
pub async fn create_page(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    request: CreatePageRequest,
) -> Result<StructureHistoryResult, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook_lock(&histories, |history_map| {
            observe_structure(history_map, &root, &request.active_page_id)?;
            let snapshot = storage::create_page(
                &root,
                &request.modified_at,
                &request.position,
                request.background.as_ref(),
                request.geometry.as_ref(),
            )
            .map_err(message)?;
            advance_structure_result(history_map, &root, snapshot)
        })
    })
    .await
    .map_err(message)?
}

fn safe_pdf_filename(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    let mut safe = stem
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    safe.truncate(90);
    let safe = safe.trim_matches('-');
    format!("{}.pdf", if safe.is_empty() { "document" } else { safe })
}

/// Pick and preserve a PDF under the notebook's `references/` directory. The frontend receives
/// only the notebook-relative path, never general filesystem access.
#[tauri::command]
pub async fn pick_pdf_reference(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
) -> Result<Option<String>, String> {
    let root = ensure_allowed(&roots, &root)?;
    let Some(picked) = app
        .dialog()
        .file()
        .add_filter("PDF document", &["pdf"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let picked = picked.into_path().map_err(message)?;
    let length = fs::metadata(&picked).map_err(message)?.len();
    if length > storage::MAX_PDF_BYTES as u64 {
        return Err(format!(
            "PDF is {length} bytes; maximum is {}",
            storage::MAX_PDF_BYTES
        ));
    }
    let bytes = fs::read(&picked).map_err(message)?;
    let filename = safe_pdf_filename(&picked);
    let stem = filename.trim_end_matches(".pdf");
    for suffix in 0..1_000 {
        let candidate = if suffix == 0 {
            filename.clone()
        } else {
            format!("{stem}-{suffix}.pdf")
        };
        match storage::store_pdf_reference(&root, &candidate, &bytes) {
            Ok(relative) => return Ok(Some(relative)),
            Err(storage::StorageError::AlreadyExists(_)) => {}
            Err(error) => return Err(message(error)),
        }
    }
    Err("could not allocate a PDF reference name".to_owned())
}

/// Read one already-contained reference as a raw IPC response. This avoids serialising a lecture
/// deck into a JSON number array before PDF.js can consume it.
#[tauri::command]
pub async fn read_pdf_reference(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
    source_path: String,
) -> Result<tauri::ipc::Response, String> {
    let root = ensure_allowed(&roots, &root)?;
    tauri::async_runtime::spawn_blocking(move || {
        storage::read_pdf_reference(&root, &source_path)
            .map(tauri::ipc::Response::new)
            .map_err(message)
    })
    .await
    .map_err(message)?
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPdfPagesRequest {
    modified_at: String,
    position: PagePosition,
    source_path: String,
    geometries: Vec<PageGeometry>,
    active_page_id: String,
}

#[tauri::command]
pub async fn import_pdf_pages(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    request: ImportPdfPagesRequest,
) -> Result<StructureHistoryResult, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook_lock(&histories, |history_map| {
            observe_structure(history_map, &root, &request.active_page_id)?;
            let snapshot = storage::import_pdf_pages(
                &root,
                &request.modified_at,
                &request.position,
                &request.source_path,
                &request.geometries,
            )
            .map_err(message)?;
            advance_structure_result(history_map, &root, snapshot)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn commit_notebook(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    snapshot: NotebookSnapshot,
) -> Result<HistoryResult, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let page_id = snapshot.page.id.clone();
        // ponytail: one lock is enough for one open notebook; use per-root locks with multi-window work.
        with_history(&histories, root, page_id, |root, history| {
            storage::commit_notebook(root, history, snapshot)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn undo_notebook(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
) -> Result<HistoryResult, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_history(&histories, root, page_id, |root, history| {
            storage::undo_notebook(root, history)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn redo_notebook(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
) -> Result<HistoryResult, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_history(&histories, root, page_id, |root, history| {
            storage::redo_notebook(root, history)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn duplicate_page(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
    modified_at: String,
) -> Result<StructureHistoryResult, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook_lock(&histories, |history_map| {
            observe_structure(history_map, &root, &page_id)?;
            let snapshot =
                storage::duplicate_page(&root, &page_id, &modified_at).map_err(message)?;
            advance_structure_result(history_map, &root, snapshot)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn delete_page(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
    modified_at: String,
) -> Result<StructureHistoryResult, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook_lock(&histories, |history_map| {
            observe_structure(history_map, &root, &page_id)?;
            let snapshot = storage::delete_page(&root, &page_id, &modified_at).map_err(message)?;
            advance_structure_result(history_map, &root, snapshot)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn reorder_pages(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    ordered_ids: Vec<String>,
    modified_at: String,
    active_page_id: String,
) -> Result<StructureHistoryResult, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook_lock(&histories, |history_map| {
            observe_structure(history_map, &root, &active_page_id)?;
            let snapshot =
                storage::reorder_pages(&root, &ordered_ids, &modified_at, &active_page_id)
                    .map_err(message)?;
            advance_structure_result(history_map, &root, snapshot)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn undo_page_structure(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    modified_at: String,
) -> Result<StructureHistoryResult, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook_lock(&histories, |history_map| {
            restore_structure_result(history_map, &root, &modified_at, true)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn redo_page_structure(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    modified_at: String,
) -> Result<StructureHistoryResult, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook_lock(&histories, |history_map| {
            restore_structure_result(history_map, &root, &modified_at, false)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn search_notebook(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
    query: String,
) -> Result<Vec<SearchHit>, String> {
    let root = ensure_allowed(&roots, &root)?;
    tauri::async_runtime::spawn_blocking(move || {
        storage::search_notebook(&root, &query).map_err(message)
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn list_recovery_candidates(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
) -> Result<Vec<RecoveryCandidate>, String> {
    let root = ensure_allowed(&roots, &root)?;
    tauri::async_runtime::spawn_blocking(move || {
        storage::list_recovery_candidates(&root).map_err(message)
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn restore_recovery_candidate(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    file_name: String,
) -> Result<HistoryResult, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook_lock(&histories, |history_map| {
            let candidates = storage::list_recovery_candidates(&root).map_err(message)?;
            let page_id = candidates
                .iter()
                .find(|candidate| candidate.file_name == file_name)
                .map(|candidate| candidate.page_id.clone())
                .ok_or_else(|| "unknown recovery candidate".to_owned())?;
            let history = history_map
                .pages
                .entry((root.clone(), page_id))
                .or_default();
            storage::restore_recovery_candidate(&root, history, &file_name).map_err(message)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn discard_recovery_candidate(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    file_name: String,
) -> Result<(), String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook_lock(&histories, |_| {
            storage::discard_recovery_candidate(&root, &file_name).map_err(message)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn store_pasted_image(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    filename: String,
    bytes: Vec<u8>,
) -> Result<String, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook_lock(&histories, |_| {
            storage::store_pasted_image(&root, &filename, &bytes).map_err(message)
        })
    })
    .await
    .map_err(message)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        thread,
        time::Duration,
    };

    #[test]
    fn notebook_operations_do_not_overlap() {
        let histories = NotebookHistories::default();
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        thread::scope(|scope| {
            for _ in 0..4 {
                let histories = NotebookHistories(histories.0.clone());
                let active = active.clone();
                let peak = peak.clone();
                scope.spawn(move || {
                    with_notebook_lock(&histories, |_| {
                        let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(count, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(10));
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(())
                    })
                    .unwrap();
                });
            }
        });

        assert_eq!(peak.load(Ordering::SeqCst), 1);
    }
}
