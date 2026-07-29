//! Reading and writing a notebook on disk.
//!
//! The module is layered, bottom to top: [`paths`] decides whether a path may be touched at all,
//! [`files`] moves bytes without tearing them, [`validate`] decides whether a snapshot is a
//! notebook, [`write`] persists one and fingerprints what it wrote, and [`pages`], [`history`],
//! [`recovery`], and [`search`] are the operations built on top of those. Callers outside the
//! crate see only the re-exports below.

use std::{error::Error, fmt, io};

use serde::{Deserialize, Serialize};

use crate::{InkLayer, NotebookManifest, Page};

mod files;
mod history;
mod pages;
mod paths;
mod recovery;
mod search;
mod validate;
mod write;

#[cfg(test)]
mod fixtures;

pub use crate::object::{MAX_IMAGE_BYTES, MAX_PDF_BYTES};
pub use history::{
    HistoryResult, NotebookHistory, commit_notebook, focus_page, observe_notebook, observe_page,
    redo_notebook, undo_notebook,
};
pub use pages::{
    NotebookStructureHistory, PagePosition, StructureHistoryResult, advance_structure,
    create_notebook, create_page, delete_page, duplicate_page, import_pdf_pages, observe_structure,
    open_notebook, open_page, redo_structure, reorder_pages, save_notebook, undo_structure,
};
pub use recovery::{
    RecoveryCandidate, discard_recovery_candidate, list_recovery_candidates,
    restore_recovery_candidate,
};
pub use search::{SearchHit, search_notebook};
pub use write::{read_pdf_reference, store_pasted_image, store_pdf_reference};

/// The store's size budget, in one place.
///
/// Structure and handwriting grow for different reasons, so they get separate ceilings.
/// `MAX_JSON_BYTES` bounds a manifest or page file. Ink is bounded by
/// `MAX_INK_POINTS_PER_LAYER` — the limit actually enforced before a write — with
/// `MAX_INK_BYTES` only having to stay above what that many quantized samples serialize to.
pub(crate) const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_INK_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_INK_STROKES_PER_LAYER: usize = 20_000;
pub(crate) const MAX_INK_POINTS_PER_LAYER: usize = 750_000;
pub(crate) const MAX_RECOVERY_BYTES: usize = 192 * 1024 * 1024;
pub(crate) const HISTORY_LIMIT: usize = 100;
pub(crate) const RECOVERY_CANDIDATE_LIMIT: usize = 10;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookSnapshot {
    pub manifest: NotebookManifest,
    pub page: Page,
    pub blocks: Vec<StoredFile>,
    pub assets: Vec<StoredFile>,
    pub ink_layers: Vec<InkLayer>,
}

impl NotebookSnapshot {
    /// The form kept in an undo stack or a recovery intent.
    ///
    /// Assets are write-once and still on disk, so carrying their bytes in every retained
    /// snapshot would multiply a notebook's memory by its undo depth for no gain — they are read
    /// back from their canonical paths whenever a snapshot is handed out again.
    pub(crate) fn without_assets(mut self) -> Self {
        self.assets.clear();
        self
    }
}

#[derive(Debug)]
pub enum StorageError {
    Io(io::Error),
    Json(serde_json::Error),
    InvalidPath(String),
    InvalidNotebook(String),
    AlreadyExists(String),
    ImageTooLarge { size: usize, maximum: usize },
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
            Self::InvalidPath(path) => write!(formatter, "invalid notebook path: {path}"),
            Self::InvalidNotebook(message) => write!(formatter, "invalid notebook: {message}"),
            Self::AlreadyExists(path) => write!(formatter, "file already exists: {path}"),
            Self::ImageTooLarge { size, maximum } => {
                write!(formatter, "image is {size} bytes; maximum is {maximum}")
            }
        }
    }
}

impl Error for StorageError {}

impl From<io::Error> for StorageError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub(crate) fn invalid<T>(message: &str) -> Result<T, StorageError> {
    Err(invalid_error(message))
}

pub(crate) fn invalid_error(message: &str) -> StorageError {
    StorageError::InvalidNotebook(message.into())
}
