use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use goodtype_core::storage::{self, HistoryResult, NotebookHistory, NotebookSnapshot};

#[derive(Default)]
pub struct NotebookHistories(Arc<Mutex<HashMap<(PathBuf, String), NotebookHistory>>>);

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
pub async fn create_notebook(
    root: String,
    snapshot: NotebookSnapshot,
    histories: tauri::State<'_, NotebookHistories>,
) -> Result<(), String> {
    let histories = histories.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        storage::create_notebook(Path::new(&root), &snapshot).map_err(message)?;
        let root = fs::canonicalize(root).map_err(message)?;
        let page_id = snapshot.page.id;
        let mut histories = histories.lock().map_err(message)?;
        storage::observe_page(
            &root,
            &page_id,
            histories
                .entry((root.clone(), page_id.clone()))
                .or_default(),
        )
        .map(|_| ())
        .map_err(message)
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn open_notebook(
    root: String,
    histories: tauri::State<'_, NotebookHistories>,
) -> Result<NotebookSnapshot, String> {
    let histories = histories.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = fs::canonicalize(root).map_err(message)?;
        let snapshot = storage::open_notebook(&root).map_err(message)?;
        let page_id = snapshot.page.id;
        let mut histories = histories.lock().map_err(message)?;
        storage::observe_page(
            &root,
            &page_id,
            histories
                .entry((root.clone(), page_id.clone()))
                .or_default(),
        )
        .map_err(message)
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn open_page(
    root: String,
    page_id: String,
    histories: tauri::State<'_, NotebookHistories>,
) -> Result<NotebookSnapshot, String> {
    let histories = histories.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = fs::canonicalize(root).map_err(message)?;
        let mut histories = histories.lock().map_err(message)?;
        storage::observe_page(
            &root,
            &page_id,
            histories
                .entry((root.clone(), page_id.clone()))
                .or_default(),
        )
        .map_err(message)
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn create_page(
    root: String,
    modified_at: String,
    histories: tauri::State<'_, NotebookHistories>,
) -> Result<NotebookSnapshot, String> {
    let histories = histories.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = fs::canonicalize(root).map_err(message)?;
        let snapshot = storage::create_page(&root, &modified_at).map_err(message)?;
        let page_id = snapshot.page.id;
        let mut histories = histories.lock().map_err(message)?;
        storage::observe_page(
            &root,
            &page_id,
            histories
                .entry((root.clone(), page_id.clone()))
                .or_default(),
        )
        .map_err(message)
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn commit_notebook(
    root: String,
    snapshot: NotebookSnapshot,
    histories: tauri::State<'_, NotebookHistories>,
) -> Result<HistoryResult, String> {
    let histories = histories.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = fs::canonicalize(root).map_err(message)?;
        let page_id = snapshot.page.id.clone();
        // ponytail: one lock is enough for one open notebook; use per-root locks with multi-window work.
        let mut histories = histories.lock().map_err(message)?;
        storage::commit_notebook(
            &root,
            histories.entry((root.clone(), page_id)).or_default(),
            snapshot,
        )
        .map_err(message)
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn undo_notebook(
    root: String,
    page_id: String,
    histories: tauri::State<'_, NotebookHistories>,
) -> Result<HistoryResult, String> {
    let histories = histories.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = fs::canonicalize(root).map_err(message)?;
        let mut histories = histories.lock().map_err(message)?;
        storage::undo_notebook(&root, histories.entry((root.clone(), page_id)).or_default())
            .map_err(message)
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn redo_notebook(
    root: String,
    page_id: String,
    histories: tauri::State<'_, NotebookHistories>,
) -> Result<HistoryResult, String> {
    let histories = histories.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = fs::canonicalize(root).map_err(message)?;
        let mut histories = histories.lock().map_err(message)?;
        storage::redo_notebook(&root, histories.entry((root.clone(), page_id)).or_default())
            .map_err(message)
    })
    .await
    .map_err(message)?
}

#[tauri::command]
pub async fn store_pasted_image(
    root: String,
    filename: String,
    bytes: Vec<u8>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        storage::store_pasted_image(Path::new(&root), &filename, &bytes)
    })
    .await
    .map_err(message)?
    .map_err(message)
}
