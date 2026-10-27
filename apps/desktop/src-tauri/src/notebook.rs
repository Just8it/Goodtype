use std::{
    collections::{HashMap, HashSet},
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

impl NotebookHistories {
    pub(crate) fn forget_root(&self, root: &Path) -> Result<(), String> {
        let mut store = self.0.lock().map_err(|error| error.to_string())?;
        store.pages.retain(|(known_root, _), _| known_root != root);
        store.structure.remove(root);
        Ok(())
    }

    pub(crate) fn retain_roots(&self, roots: &HashSet<PathBuf>) -> Result<(), String> {
        let mut store = self.0.lock().map_err(|error| error.to_string())?;
        store.pages.retain(|(root, _), _| roots.contains(root));
        store.structure.retain(|root, _| roots.contains(root));
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct HistoryStore {
    pages: HashMap<(PathBuf, String), NotebookHistory>,
    structure: HashMap<PathBuf, NotebookStructureHistory>,
}

impl HistoryStore {
    pub(crate) fn relocate_roots(&mut self, moves: &[(PathBuf, PathBuf)]) {
        let relocate = |root: PathBuf| {
            moves
                .iter()
                .find_map(|(from, to)| root.strip_prefix(from).ok().map(|suffix| to.join(suffix)))
                .unwrap_or(root)
        };
        let pages = self
            .pages
            .extract_if(|(root, _), _| moves.iter().any(|(from, _)| root.starts_with(from)))
            .collect::<Vec<_>>();
        for ((root, page), history) in pages {
            self.pages.insert((relocate(root), page), history);
        }
        let structure = self
            .structure
            .extract_if(|root, _| moves.iter().any(|(from, _)| root.starts_with(from)))
            .collect::<Vec<_>>();
        for (root, history) in structure {
            self.structure.insert(relocate(root), history);
        }
    }
}

/// Codes carry behavior across IPC; details stay available without language-dependent parsing.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookError {
    code: &'static str,
    details: String,
}

impl NotebookError {
    fn other(error: impl std::fmt::Display) -> Self {
        Self {
            code: "operation_failed",
            details: error.to_string(),
        }
    }
}

impl std::fmt::Display for NotebookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.details)
    }
}

impl From<String> for NotebookError {
    fn from(details: String) -> Self {
        Self::other(details)
    }
}

impl From<&str> for NotebookError {
    fn from(details: &str) -> Self {
        Self::other(details)
    }
}

impl From<std::io::Error> for NotebookError {
    fn from(error: std::io::Error) -> Self {
        Self::other(error)
    }
}

impl From<storage::StorageError> for NotebookError {
    fn from(error: storage::StorageError) -> Self {
        let code = match &error {
            storage::StorageError::ExternalChange(_) => "external_change",
            storage::StorageError::RevisionConflict { .. } => "revision_conflict",
            storage::StorageError::RecoveryLimit { .. } => "recovery_limit",
            _ => "operation_failed",
        };
        Self {
            code,
            details: error.to_string(),
        }
    }
}

pub(crate) fn with_notebook_lock<T>(
    histories: &NotebookHistories,
    operation: impl FnOnce(&mut HistoryStore) -> Result<T, NotebookError>,
) -> Result<T, NotebookError> {
    let mut histories = histories.0.lock().map_err(NotebookError::other)?;
    operation(&mut histories)
}

pub(crate) fn with_notebook<T>(
    histories: &NotebookHistories,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    with_notebook_lock(histories, |_| operation().map_err(NotebookError::from))
        .map_err(|error| error.details)
}

/// The shape every notebook command has: admit the root, take the notebook lock, and do the work
/// off the UI thread.
///
/// Written out by hand at fifteen call sites, it was four lines of ceremony around three lines of
/// work — and one of those four is `ensure_allowed`, which is the whole reason the frontend
/// cannot point Rust at an arbitrary directory. A command that forgot it would look exactly like
/// its neighbours. Now it cannot be forgotten, because there is nowhere to forget it.
pub(crate) async fn on_notebook<T: Send + 'static>(
    roots: &tauri::State<'_, AllowedRoots>,
    histories: &tauri::State<'_, NotebookHistories>,
    root: String,
    operation: impl FnOnce(&Path, &mut HistoryStore) -> Result<T, NotebookError> + Send + 'static,
) -> Result<T, NotebookError> {
    ensure_allowed(roots, &root)?;
    let roots = AllowedRoots(roots.0.clone());
    let histories = NotebookHistories(histories.0.clone());
    tauri::async_runtime::spawn_blocking(move || {
        with_notebook_lock(&histories, |store| {
            // A shelf move may have changed root identity while this operation was queued.
            let root = ensure_allowed(&roots, &root)?;
            operation(&root, store)
        })
    })
    .await
    .map_err(NotebookError::other)?
}

/// As [`on_notebook`], for the operations scoped to one page: resolves the `(root, page)` history
/// entry so that keying stays in one place.
async fn on_page<T: Send + 'static>(
    roots: &tauri::State<'_, AllowedRoots>,
    histories: &tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
    operation: impl FnOnce(&Path, &str, &mut NotebookHistory) -> Result<T, storage::StorageError>
    + Send
    + 'static,
) -> Result<T, NotebookError> {
    on_notebook(roots, histories, root, move |root, store| {
        let history = store
            .pages
            .entry((root.to_path_buf(), page_id.clone()))
            .or_default();
        operation(root, &page_id, history).map_err(NotebookError::from)
    })
    .await
}

/// For the read-only operations that touch no history at all, and so need no lock.
async fn on_root<T: Send + 'static>(
    roots: &tauri::State<'_, AllowedRoots>,
    root: String,
    operation: impl FnOnce(&Path) -> Result<T, NotebookError> + Send + 'static,
) -> Result<T, NotebookError> {
    let root = ensure_allowed(roots, &root)?;
    tauri::async_runtime::spawn_blocking(move || operation(&root))
        .await
        .map_err(NotebookError::other)?
}

/// A structural manifest change invalidates every page fingerprint, because the manifest is part
/// of each page's canonical file set. Drop those histories and re-observe the page the change
/// landed on.
///
/// Both the advance and the undo/redo path need exactly this, and each carried its own copy.
fn reobserve_after_structure(
    histories: &mut HistoryStore,
    root: &Path,
    result: StructureHistoryResult,
) -> Result<StructureHistoryResult, NotebookError> {
    histories
        .pages
        .retain(|(known_root, _), _| known_root != root);
    let page_id = result.snapshot.page.id.clone();
    let history = histories
        .pages
        .entry((root.to_path_buf(), page_id.clone()))
        .or_default();
    let snapshot = storage::observe_page(root, &page_id, history).map_err(NotebookError::from)?;
    Ok(StructureHistoryResult { snapshot, ..result })
}

/// Record the structure action, then re-observe the page it produced.
fn advance_structure_result(
    histories: &mut HistoryStore,
    root: &Path,
    snapshot: NotebookSnapshot,
) -> Result<StructureHistoryResult, NotebookError> {
    let result = storage::advance_structure(
        root,
        &snapshot,
        histories.structure.entry(root.to_path_buf()).or_default(),
    )
    .map_err(NotebookError::from)?;
    reobserve_after_structure(histories, root, result)
}

fn observe_structure(
    histories: &mut HistoryStore,
    root: &Path,
    active_page_id: &str,
) -> Result<(), NotebookError> {
    storage::observe_structure(
        root,
        active_page_id,
        histories.structure.entry(root.to_path_buf()).or_default(),
    )
    .map_err(NotebookError::from)
}

fn restore_structure_result(
    histories: &mut HistoryStore,
    root: &Path,
    modified_at: &str,
    undo: bool,
) -> Result<StructureHistoryResult, NotebookError> {
    let structure = histories.structure.entry(root.to_path_buf()).or_default();
    let result = if undo {
        storage::undo_structure(root, modified_at, structure)
    } else {
        storage::redo_structure(root, modified_at, structure)
    }
    .map_err(NotebookError::from)?;
    reobserve_after_structure(histories, root, result)
}

#[tauri::command]
pub async fn create_notebook(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    snapshot: NotebookSnapshot,
    preset_choice: Option<crate::preset::PresetChoice>,
) -> Result<(), NotebookError> {
    on_notebook(&roots, &histories, root, move |root, store| {
        let preset_choice = preset_choice.unwrap_or_default();
        if !matches!(preset_choice, crate::preset::PresetChoice::None)
            && root.join("styles/default.typ").exists()
        {
            return Err("styles/default.typ already exists".into());
        }
        let installed = crate::preset::install_default(root, &preset_choice, None)?.is_some();
        if let Err(error) = storage::create_notebook(root, &snapshot) {
            if installed {
                let _ = storage::remove_typst_style(root, "default.typ");
            }
            return Err(error.into());
        }
        observe_structure(store, root, &snapshot.page.id)?;
        let history = store
            .pages
            .entry((root.to_path_buf(), snapshot.page.id.clone()))
            .or_default();
        storage::observe_page(root, &snapshot.page.id, history)
            .map(|_| ())
            .map_err(NotebookError::from)
    })
    .await
}

#[tauri::command]
pub async fn open_notebook(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
) -> Result<NotebookSnapshot, NotebookError> {
    on_notebook(&roots, &histories, root, |root, store| {
        let snapshot = storage::open_notebook(root).map_err(NotebookError::from)?;
        observe_structure(store, root, &snapshot.page.id)?;
        let history = store
            .pages
            .entry((root.to_path_buf(), snapshot.page.id.clone()))
            .or_default();
        storage::observe_page(root, &snapshot.page.id, history).map_err(NotebookError::from)
    })
    .await
}

#[tauri::command]
pub async fn open_page(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
) -> Result<NotebookSnapshot, NotebookError> {
    on_page(&roots, &histories, root, page_id, storage::observe_page).await
}

#[tauri::command]
pub async fn focus_page(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
) -> Result<HistoryResult, NotebookError> {
    on_page(&roots, &histories, root, page_id, storage::focus_page).await
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
) -> Result<StructureHistoryResult, NotebookError> {
    on_notebook(&roots, &histories, root, move |root, store| {
        observe_structure(store, root, &request.active_page_id)?;
        let snapshot = storage::create_page(
            root,
            &request.modified_at,
            &request.position,
            request.background.as_ref(),
            request.geometry.as_ref(),
        )
        .map_err(NotebookError::from)?;
        advance_structure_result(store, root, snapshot)
    })
    .await
}

fn safe_pdf_filename(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy())
        .unwrap_or_default();
    format!(
        "{}.pdf",
        goodtype_core::paths::safe_stem(&stem, 90, "document")
    )
}

/// Pick and preserve a PDF under the notebook's `references/` directory. The frontend receives
/// only the notebook-relative path, never general filesystem access.
#[tauri::command]
pub async fn pick_pdf_reference(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    ui_language: Option<String>,
) -> Result<Option<String>, NotebookError> {
    ensure_allowed(&roots, &root)?;
    let Some(picked) = app
        .dialog()
        .file()
        .set_title(crate::settings::dialog_label(
            ui_language.as_deref(),
            "Import PDF",
            "PDF importieren",
        ))
        .add_filter(
            crate::settings::dialog_label(ui_language.as_deref(), "PDF document", "PDF-Dokument"),
            &["pdf"],
        )
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let picked = picked.into_path().map_err(NotebookError::other)?;
    let bytes = storage::files::read_bounded(&picked, storage::MAX_PDF_BYTES)
        .map_err(|error| NotebookError::other(format!("PDF: {error}")))?;
    let filename = safe_pdf_filename(&picked);
    on_notebook(&roots, &histories, root, move |root, _| {
        let stem = filename.trim_end_matches(".pdf");
        for suffix in 0..1_000 {
            let candidate = if suffix == 0 {
                filename.clone()
            } else {
                format!("{stem}-{suffix}.pdf")
            };
            match storage::store_pdf_reference(root, &candidate, &bytes) {
                Ok(relative) => return Ok(Some(relative)),
                Err(storage::StorageError::AlreadyExists(_)) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err("could not allocate a PDF reference name".into())
    })
    .await
}

/// Read one already-contained reference as a raw IPC response. This avoids serialising a lecture
/// deck into a JSON number array before PDF.js can consume it.
#[tauri::command]
pub async fn read_pdf_reference(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
    source_path: String,
) -> Result<tauri::ipc::Response, NotebookError> {
    on_root(&roots, root, move |root| {
        storage::read_pdf_reference(root, &source_path)
            .map(tauri::ipc::Response::new)
            .map_err(NotebookError::from)
    })
    .await
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
) -> Result<StructureHistoryResult, NotebookError> {
    on_notebook(&roots, &histories, root, move |root, store| {
        observe_structure(store, root, &request.active_page_id)?;
        let snapshot = storage::import_pdf_pages(
            root,
            &request.modified_at,
            &request.position,
            &request.source_path,
            &request.geometries,
        )
        .map_err(NotebookError::from)?;
        advance_structure_result(store, root, snapshot)
    })
    .await
}

#[tauri::command]
pub async fn commit_notebook(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    snapshot: Option<NotebookSnapshot>,
    delta: Option<storage::PageDelta>,
) -> Result<CommitResult, NotebookError> {
    let page_id = match (&snapshot, &delta) {
        (Some(snapshot), None) => snapshot.page.id.clone(),
        (None, Some(delta)) => delta.page.id.clone(),
        _ => return Err("Supply exactly one snapshot or page delta".into()),
    };
    // ponytail: one lock is enough for one open notebook; use per-root locks with multi-window work.
    on_page(
        &roots,
        &histories,
        root,
        page_id,
        move |root, _, history| {
            let (result, asset_paths) = if let Some(delta) = delta {
                let known = delta.known_assets.clone();
                let mut result = storage::commit_page_delta(root, history, delta)?;
                let paths = result
                    .snapshot
                    .assets
                    .iter()
                    .map(|file| file.path.clone())
                    .collect();
                result
                    .snapshot
                    .assets
                    .retain(|file| !known_asset(file, &known));
                result.snapshot.ink_layers.clear();
                (result, Some(paths))
            } else {
                (
                    storage::commit_notebook(root, history, snapshot.expect("checked snapshot"))?,
                    None,
                )
            };
            Ok(CommitResult {
                result,
                asset_paths,
            })
        },
    )
    .await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitResult {
    #[serde(flatten)]
    result: HistoryResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    asset_paths: Option<Vec<String>>,
}

fn known_asset(file: &storage::StoredFile, known: &HashMap<String, String>) -> bool {
    use sha2::{Digest, Sha256};
    known
        .get(&file.path)
        .is_some_and(|hash| *hash == format!("{:x}", Sha256::digest(&file.bytes)))
}

#[tauri::command]
pub async fn undo_notebook(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
) -> Result<HistoryResult, NotebookError> {
    on_page(&roots, &histories, root, page_id, |root, _, history| {
        storage::undo_notebook(root, history)
    })
    .await
}

#[tauri::command]
pub async fn redo_notebook(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
) -> Result<HistoryResult, NotebookError> {
    on_page(&roots, &histories, root, page_id, |root, _, history| {
        storage::redo_notebook(root, history)
    })
    .await
}

#[tauri::command]
pub async fn duplicate_page(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
    modified_at: String,
) -> Result<StructureHistoryResult, NotebookError> {
    on_notebook(&roots, &histories, root, move |root, store| {
        observe_structure(store, root, &page_id)?;
        let snapshot =
            storage::duplicate_page(root, &page_id, &modified_at).map_err(NotebookError::from)?;
        advance_structure_result(store, root, snapshot)
    })
    .await
}

#[tauri::command]
pub async fn delete_page(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
    modified_at: String,
) -> Result<StructureHistoryResult, NotebookError> {
    on_notebook(&roots, &histories, root, move |root, store| {
        observe_structure(store, root, &page_id)?;
        let snapshot =
            storage::delete_page(root, &page_id, &modified_at).map_err(NotebookError::from)?;
        advance_structure_result(store, root, snapshot)
    })
    .await
}

#[tauri::command]
pub async fn reorder_pages(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    ordered_ids: Vec<String>,
    modified_at: String,
    active_page_id: String,
) -> Result<StructureHistoryResult, NotebookError> {
    on_notebook(&roots, &histories, root, move |root, store| {
        observe_structure(store, root, &active_page_id)?;
        let snapshot = storage::reorder_pages(root, &ordered_ids, &modified_at, &active_page_id)
            .map_err(NotebookError::from)?;
        advance_structure_result(store, root, snapshot)
    })
    .await
}

#[tauri::command]
pub async fn undo_page_structure(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    modified_at: String,
) -> Result<StructureHistoryResult, NotebookError> {
    on_notebook(&roots, &histories, root, move |root, store| {
        restore_structure_result(store, root, &modified_at, true)
    })
    .await
}

#[tauri::command]
pub async fn redo_page_structure(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    modified_at: String,
) -> Result<StructureHistoryResult, NotebookError> {
    on_notebook(&roots, &histories, root, move |root, store| {
        restore_structure_result(store, root, &modified_at, false)
    })
    .await
}

#[tauri::command]
pub async fn search_notebook(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
    query: String,
) -> Result<Vec<SearchHit>, NotebookError> {
    on_root(&roots, root, move |root| {
        storage::search_notebook(root, &query).map_err(NotebookError::from)
    })
    .await
}

#[tauri::command]
pub async fn list_recovery_candidates(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
) -> Result<Vec<RecoveryCandidate>, NotebookError> {
    on_root(&roots, root, |root| {
        storage::list_recovery_candidates(root).map_err(NotebookError::from)
    })
    .await
}

#[tauri::command]
pub async fn restore_recovery_candidate(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    file_name: String,
) -> Result<HistoryResult, NotebookError> {
    on_notebook(&roots, &histories, root, move |root, store| {
        let candidates = storage::list_recovery_candidates(root).map_err(NotebookError::from)?;
        let page_id = candidates
            .iter()
            .find(|candidate| candidate.file_name == file_name)
            .and_then(|candidate| candidate.page_id.clone())
            .ok_or_else(|| "recovery candidate is unknown or unreadable".to_owned())?;
        let history = store
            .pages
            .entry((root.to_path_buf(), page_id))
            .or_default();
        storage::restore_recovery_candidate(root, history, &file_name).map_err(NotebookError::from)
    })
    .await
}

#[tauri::command]
pub async fn discard_recovery_candidate(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    file_name: String,
) -> Result<(), NotebookError> {
    on_notebook(&roots, &histories, root, move |root, _| {
        storage::discard_recovery_candidate(root, &file_name).map_err(NotebookError::from)
    })
    .await
}

#[tauri::command]
pub async fn store_pasted_image(
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    filename: String,
    bytes: Vec<u8>,
) -> Result<String, NotebookError> {
    on_notebook(&roots, &histories, root, move |root, _| {
        storage::store_pasted_image(root, &filename, &bytes).map_err(NotebookError::from)
    })
    .await
}

#[cfg(test)]
mod tests {
    #[test]
    fn storage_failure_codes_survive_ipc_without_parsing_details() {
        for (error, code) in [
            (
                storage::StorageError::ExternalChange("arbitrary details".into()),
                "external_change",
            ),
            (
                storage::StorageError::RevisionConflict {
                    expected: 1,
                    found: 2,
                },
                "revision_conflict",
            ),
            (
                storage::StorageError::RecoveryLimit { maximum: 10 },
                "recovery_limit",
            ),
            (
                storage::StorageError::InvalidNotebook(
                    "revision conflict in an unrelated diagnostic".into(),
                ),
                "operation_failed",
            ),
        ] {
            let payload = serde_json::to_value(NotebookError::from(error)).unwrap();
            assert_eq!(payload["code"], code);
            assert!(
                payload["details"]
                    .as_str()
                    .is_some_and(|details| !details.is_empty())
            );
        }
    }
    #[test]
    fn compact_confirmations_omit_only_identical_asset_contents() {
        use sha2::{Digest, Sha256};
        let mut file = super::storage::StoredFile {
            path: "assets/original.png".into(),
            bytes: vec![1, 2, 3],
        };
        let known = std::collections::HashMap::from([(
            file.path.clone(),
            format!("{:x}", Sha256::digest(&file.bytes)),
        )]);
        assert!(super::known_asset(&file, &known));
        file.bytes[0] = 9;
        assert!(!super::known_asset(&file, &known));
    }
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

    #[test]
    fn closed_notebook_histories_are_released() {
        let histories = NotebookHistories::default();
        let kept = PathBuf::from("kept");
        let closed = PathBuf::from("closed");
        with_notebook_lock(&histories, |store| {
            store
                .pages
                .insert((kept.clone(), "page-1".into()), NotebookHistory::default());
            store.pages.insert(
                (closed.clone(), "page-1".into()),
                NotebookHistory::default(),
            );
            store
                .structure
                .insert(closed.clone(), NotebookStructureHistory::default());
            Ok(())
        })
        .unwrap();

        histories
            .retain_roots(&HashSet::from([kept.clone()]))
            .unwrap();
        let store = histories.0.lock().unwrap();
        assert!(store.pages.keys().all(|(root, _)| root == &kept));
        assert!(store.structure.is_empty());
    }

    #[test]
    fn moved_notebooks_keep_page_and_structure_history_under_the_new_root() {
        let histories = NotebookHistories::default();
        let from = PathBuf::from("library").join("semester");
        let old = from.join("notes");
        let to = PathBuf::from("library").join("archive").join("semester");
        let new = to.join("notes");
        let sibling = PathBuf::from("library").join("semester-other");
        with_notebook_lock(&histories, |store| {
            for root in [&old, &sibling] {
                store
                    .pages
                    .insert((root.clone(), "page-1".into()), NotebookHistory::default());
                store
                    .structure
                    .insert(root.clone(), NotebookStructureHistory::default());
            }
            store.relocate_roots(&[(from, to)]);
            assert!(store.pages.contains_key(&(new.clone(), "page-1".into())));
            assert!(store.structure.contains_key(&new));
            assert!(!store.pages.contains_key(&(old.clone(), "page-1".into())));
            assert!(!store.structure.contains_key(&old));
            assert!(
                store
                    .pages
                    .contains_key(&(sibling.clone(), "page-1".into()))
            );
            assert!(store.structure.contains_key(&sibling));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn page_undo_survives_a_move_even_with_stale_destination_history() {
        let temporary = tempfile::tempdir().unwrap();
        let from = temporary.path().join("notes");
        let to = temporary.path().join("moved");
        std::fs::create_dir(&from).unwrap();
        let mut manifest: goodtype_core::NotebookManifest = serde_json::from_str(include_str!(
            "../../../../fixtures/notebooks/minimal/goodtype.json"
        ))
        .unwrap();
        manifest.shared_style_path = None;
        let mut page: goodtype_core::Page = serde_json::from_str(include_str!(
            "../../../../fixtures/notebooks/minimal/pages/page-001.json"
        ))
        .unwrap();
        page.objects.clear();
        page.reading_order.clear();
        page.ink_layers.clear();
        let mut snapshot = NotebookSnapshot {
            manifest,
            page,
            blocks: vec![],
            assets: vec![],
            ink_layers: vec![],
        };
        storage::create_notebook(&from, &snapshot).unwrap();
        let mut history = NotebookHistory::default();
        storage::observe_notebook(&from, &mut history).unwrap();
        snapshot.page.background = PageBackground::Plain {
            color: "#123456".into(),
        };
        storage::commit_notebook(&from, &mut history, snapshot).unwrap();
        let mut store = HistoryStore::default();
        store
            .pages
            .insert((from.clone(), "page-001".into()), history);
        store
            .pages
            .insert((to.clone(), "page-001".into()), NotebookHistory::default());
        std::fs::rename(&from, &to).unwrap();
        store.relocate_roots(&[(from.clone(), to.clone())]);
        let history = store
            .pages
            .get_mut(&(to.clone(), "page-001".into()))
            .unwrap();
        let undone = storage::undo_notebook(&to, history).unwrap();
        assert!(
            matches!(&undone.snapshot.page.background, PageBackground::Plain { color } if color == "#ffffff")
        );
        let redone = storage::redo_notebook(&to, history).unwrap();
        assert!(
            matches!(&redone.snapshot.page.background, PageBackground::Plain { color } if color == "#123456")
        );
        assert!(!from.exists());
    }
}
