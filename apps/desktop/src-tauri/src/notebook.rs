use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use goodtype_core::storage::{
    self, HistoryResult, NotebookHistory, NotebookSnapshot, PagePosition, RecoveryCandidate,
    SearchHit,
};

use crate::workspace::{AllowedRoots, ensure_allowed};

#[derive(Default)]
pub struct NotebookHistories(Arc<Mutex<HashMap<(PathBuf, String), NotebookHistory>>>);

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Observe a page under the shared history map. Every history-bearing command funnels through
/// here so the (root, page) keying stays in one place.
fn with_history<T>(
    histories: &NotebookHistories,
    root: PathBuf,
    page_id: String,
    operation: impl FnOnce(&PathBuf, &mut NotebookHistory) -> Result<T, storage::StorageError>,
) -> Result<T, String> {
    let mut histories = histories.0.lock().map_err(message)?;
    let history = histories.entry((root.clone(), page_id)).or_default();
    operation(&root, history).map_err(message)
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
        storage::create_notebook(&root, &snapshot).map_err(message)?;
        let page_id = snapshot.page.id;
        with_history(&histories, root, page_id.clone(), |root, history| {
            storage::observe_page(root, &page_id, history).map(|_| ())
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
        let snapshot = storage::open_notebook(&root).map_err(message)?;
        let page_id = snapshot.page.id.clone();
        with_history(&histories, root, page_id.clone(), |root, history| {
            storage::observe_page(root, &page_id, history)
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

#[tauri::command]
pub async fn create_page(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    modified_at: String,
    position: PagePosition,
) -> Result<NotebookSnapshot, String> {
    let root = ensure_allowed(&roots, &root)?;
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = storage::create_page(&root, &modified_at, &position).map_err(message)?;
        let page_id = snapshot.page.id;
        with_history(&histories, root, page_id.clone(), |root, history| {
            storage::observe_page(root, &page_id, history)
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
    root: String,
    page_id: String,
    modified_at: String,
) -> Result<NotebookSnapshot, String> {
    let root = ensure_allowed(&roots, &root)?;
    tauri::async_runtime::spawn_blocking(move || {
        storage::duplicate_page(&root, &page_id, &modified_at).map_err(message)
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn delete_page(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
    page_id: String,
    modified_at: String,
) -> Result<NotebookSnapshot, String> {
    let root = ensure_allowed(&roots, &root)?;
    tauri::async_runtime::spawn_blocking(move || {
        storage::delete_page(&root, &page_id, &modified_at).map_err(message)
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn reorder_pages(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
    ordered_ids: Vec<String>,
    modified_at: String,
    active_page_id: String,
) -> Result<NotebookSnapshot, String> {
    let root = ensure_allowed(&roots, &root)?;
    tauri::async_runtime::spawn_blocking(move || {
        storage::reorder_pages(&root, &ordered_ids, &modified_at, &active_page_id).map_err(message)
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
        let candidates = storage::list_recovery_candidates(&root).map_err(message)?;
        let page_id = candidates
            .iter()
            .find(|candidate| candidate.file_name == file_name)
            .map(|candidate| candidate.page_id.clone())
            .ok_or_else(|| "unknown recovery candidate".to_owned())?;
        with_history(&histories, root, page_id, |root, history| {
            storage::restore_recovery_candidate(root, history, &file_name)
        })
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn discard_recovery_candidate(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
    file_name: String,
) -> Result<(), String> {
    let root = ensure_allowed(&roots, &root)?;
    tauri::async_runtime::spawn_blocking(move || {
        storage::discard_recovery_candidate(&root, &file_name).map_err(message)
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn store_pasted_image(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
    filename: String,
    bytes: Vec<u8>,
) -> Result<String, String> {
    let root = ensure_allowed(&roots, &root)?;
    tauri::async_runtime::spawn_blocking(move || {
        storage::store_pasted_image(&root, &filename, &bytes)
    })
    .await
    .map_err(message)?
    .map_err(message)
}
