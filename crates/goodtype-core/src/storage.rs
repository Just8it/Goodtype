use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt, fs,
    hash::{DefaultHasher, Hash, Hasher},
    io,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tempfile::NamedTempFile;

use crate::{
    InkLayer, InkLayerReference, NotebookManifest, Page, PageBackground, PageObject, PageReference,
    SCHEMA_VERSION,
};

pub const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;
// Ink grows with handwriting rather than with structure, so it has its own ceiling.
// MAX_INK_POINTS_PER_LAYER is the bound that is actually enforced before a write;
// MAX_INK_BYTES only has to stay above what that many quantized samples can serialize to.
const MAX_INK_BYTES: usize = 64 * 1024 * 1024;
const MAX_INK_STROKES_PER_LAYER: usize = 20_000;
const MAX_INK_POINTS_PER_LAYER: usize = 750_000;
const MAX_BLOCK_BYTES: usize = 1024 * 1024;
const MAX_RECOVERY_BYTES: usize = 192 * 1024 * 1024;
const HISTORY_LIMIT: usize = 100;
const RECOVERY_CANDIDATE_LIMIT: usize = 10;
const PENDING_TRANSACTION_PATH: &str = ".goodtype/pending-transaction.json";

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

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryIntent {
    version: u32,
    previous: NotebookSnapshot,
    candidate: NotebookSnapshot,
}

#[derive(Debug, Default)]
pub struct NotebookHistory {
    undo: Vec<NotebookSnapshot>,
    redo: Vec<NotebookSnapshot>,
    current_page_id: Option<String>,
    current_revision: Option<u64>,
    current_fingerprint: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryResult {
    pub snapshot: NotebookSnapshot,
    pub can_undo: bool,
    pub can_redo: bool,
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

pub fn create_notebook(
    selected_root: &Path,
    snapshot: &NotebookSnapshot,
) -> Result<(), StorageError> {
    fs::create_dir_all(selected_root)?;
    let root = canonical_root(selected_root)?;
    if root.join("goodtype.json").exists() {
        return Err(StorageError::AlreadyExists("goodtype.json".into()));
    }
    save_to_root(&root, snapshot).map(|_| ())
}

pub fn save_notebook(
    selected_root: &Path,
    snapshot: &NotebookSnapshot,
) -> Result<(), StorageError> {
    let root = canonical_root(selected_root)?;
    if resolve_existing(&root, "goodtype.json").is_err() {
        return Err(StorageError::InvalidNotebook(
            "goodtype.json does not exist".into(),
        ));
    }
    save_to_root(&root, snapshot).map(|_| ())
}

pub fn open_notebook(selected_root: &Path) -> Result<NotebookSnapshot, StorageError> {
    let root = canonical_root(selected_root)?;
    recover_interrupted_transaction(&root)?;
    let manifest: NotebookManifest = read_json(&root, "goodtype.json")?;
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
    let manifest: NotebookManifest = read_json(&root, "goodtype.json")?;
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

    let block_paths = referenced_block_paths(&manifest, &page);
    let asset_paths = referenced_asset_paths(&page);
    let blocks = read_stored_files(root, &block_paths, MAX_BLOCK_BYTES)?;
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
    if history.current_page_id.as_deref() != Some(snapshot.page.id.as_str())
        || history.current_revision != Some(snapshot.page.revision)
        || history.current_fingerprint != Some(fingerprint)
    {
        history.undo.clear();
        history.redo.clear();
    }
    history.current_page_id = Some(snapshot.page.id.clone());
    history.current_revision = Some(snapshot.page.revision);
    history.current_fingerprint = Some(fingerprint);
    Ok(snapshot)
}

pub fn create_page(
    selected_root: &Path,
    modified_at: &str,
) -> Result<NotebookSnapshot, StorageError> {
    if modified_at.is_empty() || modified_at.len() > 64 {
        return invalid("page timestamp must be present and bounded");
    }
    let root = canonical_root(selected_root)?;
    recover_interrupted_transaction(&root)?;
    let manifest_bytes = read_limited(resolve_existing(&root, "goodtype.json")?, MAX_JSON_BYTES)?;
    let mut manifest: NotebookManifest = serde_json::from_slice(&manifest_bytes)?;
    validate_manifest(&manifest)?;
    validate_manifest_files(&root, &manifest)?;

    let (id, page_path, ink_id, ink_path) = fresh_page_identifiers(&root, &manifest);

    manifest.pages.push(PageReference {
        id: id.clone(),
        path: page_path,
    });
    manifest.modified_at = modified_at.to_owned();
    let page = Page {
        schema_version: SCHEMA_VERSION,
        id: id.clone(),
        revision: 1,
        geometry: manifest.default_page.geometry.clone(),
        background: manifest.default_page.background.clone(),
        objects: Vec::new(),
        reading_order: Vec::new(),
        ink_layers: vec![InkLayerReference {
            id: ink_id.clone(),
            path: ink_path,
        }],
    };
    let snapshot = NotebookSnapshot {
        manifest,
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
    if read_limited(resolve_existing(&root, "goodtype.json")?, MAX_JSON_BYTES)? != manifest_bytes {
        return invalid("external change detected; reopen the notebook before adding a page");
    }
    save_to_root(&root, &snapshot)?;
    open_page(&root, &snapshot.page.id)
}

fn fresh_page_identifiers(
    root: &Path,
    manifest: &NotebookManifest,
) -> (String, String, String, String) {
    let mut number = manifest.pages.len() + 1;
    loop {
        let id = format!("page-{number:03}");
        let page_path = format!("pages/{id}.json");
        let ink_id = format!("{id}-ink-001");
        let ink_path = format!("ink/{id}-layer-001.json");
        if !manifest
            .pages
            .iter()
            .any(|page| page.id == id || page.path == page_path)
            && !root.join(&page_path).exists()
            && !root.join(&ink_path).exists()
        {
            return (id, page_path, ink_id, ink_path);
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
    if modified_at.is_empty() || modified_at.len() > 64 {
        return invalid("page timestamp must be present and bounded");
    }
    let root = canonical_root(selected_root)?;
    recover_interrupted_transaction(&root)?;
    let manifest_bytes = read_limited(resolve_existing(&root, "goodtype.json")?, MAX_JSON_BYTES)?;
    let source = open_page(&root, page_id)?;
    let mut manifest = source.manifest.clone();

    let (new_id, new_page_path, _, _) = fresh_page_identifiers(&root, &manifest);

    // Remap every ID the page owns. Assets and the shared style are shared references and
    // keep their canonical paths; everything else gets a fresh identity.
    let mut object_ids: HashMap<&str, String> = HashMap::new();
    for (index, object) in source.page.objects.iter().enumerate() {
        object_ids.insert(
            object.fields().id.as_str(),
            format!("{new_id}-obj-{:03}", index + 1),
        );
    }
    let mut layer_ids: HashMap<&str, String> = HashMap::new();
    let mut layer_paths: HashMap<&str, String> = HashMap::new();
    for (index, reference) in source.page.ink_layers.iter().enumerate() {
        layer_ids.insert(
            reference.id.as_str(),
            format!("{new_id}-ink-{:03}", index + 1),
        );
        layer_paths.insert(
            reference.path.as_str(),
            format!("ink/{new_id}-layer-{:03}.json", index + 1),
        );
    }
    let mut block_paths: HashMap<&str, String> = HashMap::new();
    let mut block_number = 0usize;
    for object in &source.page.objects {
        if let PageObject::Typst { source_path, .. } = object
            && !block_paths.contains_key(source_path.as_str())
        {
            block_number += 1;
            block_paths.insert(
                source_path.as_str(),
                format!("blocks/{new_id}-block-{block_number:03}.typ"),
            );
        }
    }
    let remap = |id: &str| -> Result<String, StorageError> {
        object_ids
            .get(id)
            .cloned()
            .ok_or_else(|| invalid_error("page references an unknown object"))
    };

    let mut stroke_counter = 0usize;
    let ink_layers = source
        .ink_layers
        .iter()
        .map(|layer| {
            let strokes = layer
                .strokes
                .iter()
                .map(|stroke| {
                    stroke_counter += 1;
                    Ok(crate::Stroke {
                        id: format!("{new_id}-stroke-{stroke_counter:04}"),
                        group_id: stroke.group_id.as_deref().map(&remap).transpose()?,
                        ..stroke.clone()
                    })
                })
                .collect::<Result<Vec<_>, StorageError>>()?;
            Ok(InkLayer {
                schema_version: layer.schema_version,
                id: layer_ids
                    .get(layer.id.as_str())
                    .cloned()
                    .ok_or_else(|| invalid_error("page references an unknown ink layer"))?,
                page_id: new_id.clone(),
                strokes,
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;

    // Stroke IDs changed, so remap ink-group membership by position within each layer.
    let stroke_id_map: HashMap<&str, &str> = source
        .ink_layers
        .iter()
        .zip(&ink_layers)
        .flat_map(|(old, new)| {
            old.strokes
                .iter()
                .zip(&new.strokes)
                .map(|(old, new)| (old.id.as_str(), new.id.as_str()))
        })
        .collect();

    let objects = source
        .page
        .objects
        .iter()
        .map(|object| {
            let mut fields = object.fields().clone();
            fields.id = remap(&fields.id)?;
            fields.group_id = fields.group_id.as_deref().map(&remap).transpose()?;
            fields.created_at = modified_at.to_owned();
            fields.modified_at = modified_at.to_owned();
            Ok(match object {
                PageObject::Typst {
                    source_path,
                    layout_width_pt,
                    measured_width_pt,
                    measured_height_pt,
                    ..
                } => PageObject::Typst {
                    fields,
                    source_path: block_paths
                        .get(source_path.as_str())
                        .cloned()
                        .expect("every Typst path was mapped"),
                    layout_width_pt: *layout_width_pt,
                    measured_width_pt: *measured_width_pt,
                    measured_height_pt: *measured_height_pt,
                },
                PageObject::Image {
                    source_path,
                    width_pt,
                    height_pt,
                    alt_text,
                    ..
                } => PageObject::Image {
                    fields,
                    source_path: source_path.clone(),
                    width_pt: *width_pt,
                    height_pt: *height_pt,
                    alt_text: alt_text.clone(),
                },
                PageObject::PdfMaterial {
                    source_path,
                    page,
                    source_width_pt,
                    source_height_pt,
                    ..
                } => PageObject::PdfMaterial {
                    fields,
                    source_path: source_path.clone(),
                    page: *page,
                    source_width_pt: *source_width_pt,
                    source_height_pt: *source_height_pt,
                },
                PageObject::InkGroup {
                    ink_layer_id,
                    stroke_ids,
                    ..
                } => PageObject::InkGroup {
                    fields,
                    ink_layer_id: layer_ids
                        .get(ink_layer_id.as_str())
                        .cloned()
                        .ok_or_else(|| invalid_error("ink group references an unknown layer"))?,
                    stroke_ids: stroke_ids
                        .iter()
                        .map(|id| {
                            stroke_id_map
                                .get(id.as_str())
                                .map(|new| (*new).to_owned())
                                .ok_or_else(|| {
                                    invalid_error("ink group references an unknown stroke")
                                })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                },
                PageObject::Group { child_ids, .. } => PageObject::Group {
                    fields,
                    child_ids: child_ids
                        .iter()
                        .map(|id| remap(id))
                        .collect::<Result<Vec<_>, _>>()?,
                },
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;

    let page = Page {
        schema_version: SCHEMA_VERSION,
        id: new_id.clone(),
        revision: 1,
        geometry: source.page.geometry.clone(),
        background: source.page.background.clone(),
        objects,
        reading_order: source
            .page
            .reading_order
            .iter()
            .map(|id| remap(id))
            .collect::<Result<Vec<_>, _>>()?,
        ink_layers: source
            .page
            .ink_layers
            .iter()
            .map(|reference| {
                Ok(InkLayerReference {
                    id: layer_ids
                        .get(reference.id.as_str())
                        .cloned()
                        .ok_or_else(|| invalid_error("unknown ink layer reference"))?,
                    path: layer_paths
                        .get(reference.path.as_str())
                        .cloned()
                        .ok_or_else(|| invalid_error("unknown ink layer path"))?,
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()?,
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
        },
    );
    manifest.modified_at = modified_at.to_owned();

    let blocks = source
        .blocks
        .iter()
        .map(|file| StoredFile {
            path: block_paths
                .get(file.path.as_str())
                .cloned()
                .unwrap_or_else(|| file.path.clone()),
            bytes: file.bytes.clone(),
        })
        .collect();

    let snapshot = NotebookSnapshot {
        manifest,
        page,
        blocks,
        assets: Vec::new(),
        ink_layers,
    };
    validate_snapshot(&root, &snapshot)?;
    if read_limited(resolve_existing(&root, "goodtype.json")?, MAX_JSON_BYTES)? != manifest_bytes {
        return invalid("external change detected; reopen the notebook before duplicating a page");
    }
    save_to_root(&root, &snapshot)?;
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
    if modified_at.is_empty() || modified_at.len() > 64 {
        return invalid("page timestamp must be present and bounded");
    }
    let root = canonical_root(selected_root)?;
    recover_interrupted_transaction(&root)?;
    let manifest_bytes = read_limited(resolve_existing(&root, "goodtype.json")?, MAX_JSON_BYTES)?;
    let mut manifest: NotebookManifest = serde_json::from_slice(&manifest_bytes)?;
    validate_manifest(&manifest)?;
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

    if read_limited(resolve_existing(&root, "goodtype.json")?, MAX_JSON_BYTES)? != manifest_bytes {
        return invalid("external change detected; reopen the notebook before deleting a page");
    }
    write_json(&root, "goodtype.json", &manifest)?;
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
    if modified_at.is_empty() || modified_at.len() > 64 {
        return invalid("page timestamp must be present and bounded");
    }
    let root = canonical_root(selected_root)?;
    recover_interrupted_transaction(&root)?;
    let manifest_bytes = read_limited(resolve_existing(&root, "goodtype.json")?, MAX_JSON_BYTES)?;
    let mut manifest: NotebookManifest = serde_json::from_slice(&manifest_bytes)?;
    validate_manifest(&manifest)?;

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

    if read_limited(resolve_existing(&root, "goodtype.json")?, MAX_JSON_BYTES)? != manifest_bytes {
        return invalid("external change detected; reopen the notebook before reordering pages");
    }
    write_json(&root, "goodtype.json", &manifest)?;
    open_page(&root, active_page_id)
}

pub fn commit_notebook(
    selected_root: &Path,
    history: &mut NotebookHistory,
    mut snapshot: NotebookSnapshot,
) -> Result<HistoryResult, StorageError> {
    let root = canonical_root(selected_root)?;
    let current = open_page(selected_root, &snapshot.page.id)?;
    ensure_current(&root, history, &current, snapshot.page.revision)?;
    snapshot.page.revision = current.page.revision + 1;
    let (saved, fingerprint) = save_and_return(&root, &current, &snapshot)?;
    push_history(&mut history.undo, history_snapshot(current));
    history.redo.clear();
    history.current_page_id = Some(saved.page.id.clone());
    history.current_revision = Some(saved.page.revision);
    history.current_fingerprint = Some(fingerprint);
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
        push_history(&mut history.redo, history_snapshot(current));
    } else {
        history.redo.pop();
        push_history(&mut history.undo, history_snapshot(current));
    }
    history.current_page_id = Some(restored.page.id.clone());
    history.current_revision = Some(restored.page.revision);
    history.current_fingerprint = Some(fingerprint);
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
                history.undo.clear();
                history.redo.clear();
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
    history.undo.clear();
    history.redo.clear();
    Err(StorageError::InvalidNotebook(format!(
        "revision conflict: expected {expected}, found {}",
        current.page.revision
    )))
}

fn canonical_fingerprint(root: &Path, snapshot: &NotebookSnapshot) -> Result<u64, StorageError> {
    let reference = page_reference(&snapshot.manifest, &snapshot.page.id)?;
    let mut files = vec![
        ("goodtype.json", MAX_JSON_BYTES),
        (reference.path.as_str(), MAX_JSON_BYTES),
    ];
    files.extend(
        referenced_block_paths(&snapshot.manifest, &snapshot.page)
            .into_iter()
            .map(|path| (path, MAX_BLOCK_BYTES)),
    );
    files.extend(
        snapshot
            .page
            .ink_layers
            .iter()
            .map(|reference| (reference.path.as_str(), MAX_INK_BYTES)),
    );
    files.sort_unstable_by_key(|(path, _)| *path);
    files.dedup_by_key(|(path, _)| *path);

    let bytes = files
        .into_iter()
        .map(|(relative, maximum)| {
            Ok((
                relative,
                read_limited(resolve_existing(root, relative)?, maximum)?,
            ))
        })
        .collect::<Result<Vec<(&str, Vec<u8>)>, StorageError>>()?;
    let entries = bytes
        .iter()
        .map(|(path, value)| (*path, value.as_slice()))
        .collect::<Vec<_>>();
    Ok(fingerprint_files(&entries))
}

/// Writes `candidate` transactionally and returns it as the frontend will see it — the written
/// canonical state plus its originals — together with the fingerprint of what was written.
/// The canonical files equal `candidate` after the write, so only assets are read back rather
/// than re-parsing and re-hashing the whole page.
fn save_and_return(
    root: &Path,
    previous: &NotebookSnapshot,
    candidate: &NotebookSnapshot,
) -> Result<(NotebookSnapshot, u64), StorageError> {
    let fingerprint = save_transactional(root, previous, candidate)?;
    let assets = read_stored_files(
        root,
        &referenced_asset_paths(&candidate.page),
        MAX_IMAGE_BYTES,
    )?;
    let saved = NotebookSnapshot {
        assets,
        ..candidate.clone()
    };
    Ok((saved, fingerprint))
}

fn save_transactional(
    root: &Path,
    previous: &NotebookSnapshot,
    candidate: &NotebookSnapshot,
) -> Result<u64, StorageError> {
    validate_snapshot(root, candidate)?;
    ensure_recovery_capacity(root)?;
    for asset in &candidate.assets {
        write_once(root, &asset.path, &asset.bytes)?;
    }

    // ponytail: reuse canonical snapshots until measured recovery size requires file staging.
    let intent = RecoveryIntent {
        version: 1,
        previous: history_snapshot(previous.clone()),
        candidate: history_snapshot(candidate.clone()),
    };
    write_recovery_intent(root, &intent)?;
    let fingerprint = save_to_root(root, candidate)?;
    remove_pending_transaction(root)?;
    Ok(fingerprint)
}

fn write_recovery_intent(root: &Path, intent: &RecoveryIntent) -> Result<(), StorageError> {
    // Internal, disposable, machine-read only: it carries two full page snapshots and is the
    // largest write per commit on a heavy page, so it is stored compact rather than pretty.
    let mut bytes = serde_json::to_vec(intent)?;
    bytes.push(b'\n');
    if bytes.len() > MAX_RECOVERY_BYTES {
        return Err(StorageError::InvalidNotebook(format!(
            "recovery data is {} bytes; maximum is {MAX_RECOVERY_BYTES}",
            bytes.len()
        )));
    }
    write_atomic(root, PENDING_TRANSACTION_PATH, &bytes)
}

fn ensure_recovery_capacity(root: &Path) -> Result<(), StorageError> {
    let recovery = root.join(".goodtype").join("recovery");
    if !recovery.exists() {
        return Ok(());
    }
    let recovery = fs::canonicalize(recovery)?;
    if !recovery.starts_with(root) || !recovery.is_dir() {
        return Err(StorageError::InvalidPath(".goodtype/recovery".into()));
    }
    if fs::read_dir(recovery)?.count() >= RECOVERY_CANDIDATE_LIMIT {
        return Err(StorageError::InvalidNotebook(format!(
            "recovery contains {RECOVERY_CANDIDATE_LIMIT} unresolved candidates"
        )));
    }
    Ok(())
}

fn recover_interrupted_transaction(root: &Path) -> Result<(), StorageError> {
    if !root.join(PENDING_TRANSACTION_PATH).exists() {
        return Ok(());
    }
    let bytes = read_limited(
        resolve_existing(root, PENDING_TRANSACTION_PATH)?,
        MAX_RECOVERY_BYTES,
    )?;
    let intent: RecoveryIntent = serde_json::from_slice(&bytes)?;
    if intent.version != 1 {
        return invalid("unsupported recovery intent version");
    }
    validate_snapshot(root, &intent.previous)?;
    validate_snapshot(root, &intent.candidate)?;
    if intent.candidate.page.revision != intent.previous.page.revision + 1 {
        return invalid("recovery revisions are not consecutive");
    }

    let candidate_reference =
        page_reference(&intent.candidate.manifest, &intent.candidate.page.id)?;
    let current_revision = read_json::<Page>(root, &candidate_reference.path)
        .map(|page| page.revision)
        .ok();
    if current_revision == Some(intent.candidate.page.revision) {
        save_to_root(root, &intent.candidate)?;
        return remove_pending_transaction(root);
    }

    save_to_root(root, &intent.previous)?;
    archive_pending_transaction(root, intent.candidate.page.revision)
}

fn remove_pending_transaction(root: &Path) -> Result<(), StorageError> {
    let pending = resolve_existing(root, PENDING_TRANSACTION_PATH)?;
    fs::remove_file(pending)?;
    Ok(())
}

fn archive_pending_transaction(root: &Path, revision: u64) -> Result<(), StorageError> {
    let pending = resolve_existing(root, PENDING_TRANSACTION_PATH)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for suffix in 0..1000 {
        let relative =
            format!(".goodtype/recovery/interrupted-r{revision}-{timestamp}-{suffix}.json");
        let target = prepare_target(root, &relative)?;
        if !target.exists() {
            fs::rename(&pending, target)?;
            return Ok(());
        }
    }
    Err(StorageError::InvalidNotebook(
        "too many recovery candidates; preserve or remove old candidates before reopening".into(),
    ))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCandidate {
    pub file_name: String,
    pub page_id: String,
    pub confirmed_revision: u64,
    pub candidate_revision: u64,
}

fn validate_candidate_file_name(file_name: &str) -> Result<(), StorageError> {
    let valid = file_name.strip_prefix("interrupted-r").is_some_and(|rest| {
        rest.strip_suffix(".json").is_some_and(|stem| {
            let mut parts = stem.split('-');
            parts.clone().count() == 3 && parts.all(|part| part.bytes().all(|b| b.is_ascii_digit()))
        })
    });
    if valid {
        Ok(())
    } else {
        Err(StorageError::InvalidPath(file_name.into()))
    }
}

fn read_candidate_intent(root: &Path, file_name: &str) -> Result<RecoveryIntent, StorageError> {
    validate_candidate_file_name(file_name)?;
    let bytes = read_limited(
        resolve_existing(root, &format!(".goodtype/recovery/{file_name}"))?,
        MAX_RECOVERY_BYTES,
    )?;
    let intent: RecoveryIntent = serde_json::from_slice(&bytes)?;
    if intent.version != 1 {
        return invalid("unsupported recovery intent version");
    }
    Ok(intent)
}

/// List retained interrupted-transaction candidates, oldest first. Unreadable entries are
/// skipped rather than blocking the readable ones.
pub fn list_recovery_candidates(
    selected_root: &Path,
) -> Result<Vec<RecoveryCandidate>, StorageError> {
    let root = canonical_root(selected_root)?;
    let recovery = root.join(".goodtype").join("recovery");
    if !recovery.is_dir() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = fs::read_dir(recovery)?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| validate_candidate_file_name(name).is_ok())
        .collect();
    names.sort_unstable();
    Ok(names
        .into_iter()
        .filter_map(|file_name| {
            let intent = read_candidate_intent(&root, &file_name).ok()?;
            Some(RecoveryCandidate {
                file_name,
                page_id: intent.candidate.page.id,
                confirmed_revision: intent.previous.page.revision,
                candidate_revision: intent.candidate.page.revision,
            })
        })
        .collect())
}

/// Restore a retained candidate as a new committed revision of its page. The current manifest
/// is kept — restore never resurrects an old page list — and the restored state becomes an
/// ordinary undoable commit. The candidate file is removed after a successful restore.
pub fn restore_recovery_candidate(
    selected_root: &Path,
    history: &mut NotebookHistory,
    file_name: &str,
) -> Result<HistoryResult, StorageError> {
    let root = canonical_root(selected_root)?;
    let intent = read_candidate_intent(&root, file_name)?;
    let mut candidate = intent.candidate;
    let current = observe_page(&root, &candidate.page.id, history)?;

    let current_reference = page_reference(&current.manifest, &candidate.page.id)?;
    let candidate_reference = page_reference(&candidate.manifest, &candidate.page.id)?;
    if current_reference.path != candidate_reference.path {
        return invalid("the recovery candidate no longer matches the notebook layout");
    }
    // The candidate carries the manifest as it looked when interrupted; pages added or
    // reordered since then must survive a restore.
    candidate.manifest = current.manifest.clone();
    candidate.page.revision = current.page.revision + 1;

    let (restored, fingerprint) = save_and_return(&root, &current, &candidate)?;
    push_history(&mut history.undo, history_snapshot(current));
    history.redo.clear();
    history.current_page_id = Some(restored.page.id.clone());
    history.current_revision = Some(restored.page.revision);
    history.current_fingerprint = Some(fingerprint);
    discard_recovery_candidate(&root, file_name)?;
    Ok(history_result(restored, history))
}

/// Remove one retained candidate after the user confirms it is no longer needed.
pub fn discard_recovery_candidate(
    selected_root: &Path,
    file_name: &str,
) -> Result<(), StorageError> {
    let root = canonical_root(selected_root)?;
    validate_candidate_file_name(file_name)?;
    let path = resolve_existing(&root, &format!(".goodtype/recovery/{file_name}"))?;
    fs::remove_file(path)?;
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub page_id: String,
    pub page_number: usize,
    pub object_id: String,
    pub excerpt: String,
}

const MAX_SEARCH_RESULTS: usize = 200;

/// Case-insensitive search across the notebook's Typst block sources, in manifest page order.
/// The index is the canonical files themselves; nothing is cached or persisted.
pub fn search_notebook(selected_root: &Path, query: &str) -> Result<Vec<SearchHit>, StorageError> {
    let trimmed = query.trim();
    if trimmed.is_empty() || trimmed.len() > 200 {
        return invalid("search text must be between 1 and 200 characters");
    }
    let root = canonical_root(selected_root)?;
    let manifest: NotebookManifest = read_json(&root, "goodtype.json")?;
    validate_manifest(&manifest)?;
    let needle = trimmed.to_lowercase();

    let mut hits = Vec::new();
    for (page_index, reference) in manifest.pages.iter().enumerate() {
        let Ok(page) = read_json::<Page>(&root, &reference.path) else {
            continue;
        };
        for object in &page.objects {
            let PageObject::Typst {
                fields,
                source_path,
                ..
            } = object
            else {
                continue;
            };
            let Ok(bytes) = read_limited(
                match resolve_existing(&root, source_path) {
                    Ok(path) => path,
                    Err(_) => continue,
                },
                MAX_BLOCK_BYTES,
            ) else {
                continue;
            };
            let source = String::from_utf8_lossy(&bytes);
            let lowered = source.to_lowercase();
            let Some(position) = lowered.find(&needle) else {
                continue;
            };
            hits.push(SearchHit {
                page_id: page.id.clone(),
                page_number: page_index + 1,
                object_id: fields.id.clone(),
                excerpt: excerpt_around(&source, position, needle.len()),
            });
            if hits.len() >= MAX_SEARCH_RESULTS {
                return Ok(hits);
            }
        }
    }
    Ok(hits)
}

fn excerpt_around(source: &str, position: usize, length: usize) -> String {
    const CONTEXT: usize = 40;
    // The match position was found in a lowercased copy whose byte offsets can differ from the
    // original for non-ASCII text; clamp and slice only at original char boundaries.
    let position = position.min(source.len());
    let start = source
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= position.saturating_sub(CONTEXT))
        .last()
        .unwrap_or(0);
    let end = source
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| *index >= (position + length + CONTEXT).min(source.len()))
        .unwrap_or(source.len());
    source[start..end]
        .replace(['\n', '\r'], " ")
        .trim()
        .to_owned()
}

fn history_snapshot(mut snapshot: NotebookSnapshot) -> NotebookSnapshot {
    snapshot.assets.clear();
    snapshot
}

fn push_history(stack: &mut Vec<NotebookSnapshot>, snapshot: NotebookSnapshot) {
    if stack.len() == HISTORY_LIMIT {
        stack.remove(0);
    }
    stack.push(snapshot);
}

fn history_result(snapshot: NotebookSnapshot, history: &NotebookHistory) -> HistoryResult {
    HistoryResult {
        snapshot,
        can_undo: !history.undo.is_empty(),
        can_redo: !history.redo.is_empty(),
    }
}

pub fn store_pasted_image(
    selected_root: &Path,
    filename: &str,
    bytes: &[u8],
) -> Result<String, StorageError> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return Err(StorageError::ImageTooLarge {
            size: bytes.len(),
            maximum: MAX_IMAGE_BYTES,
        });
    }
    validate_safe_filename(filename)?;

    let root = canonical_root(selected_root)?;
    let relative = format!("assets/{filename}");
    write_once(&root, &relative, bytes)?;
    Ok(relative)
}

/// Persists every file in the snapshot and returns the canonical fingerprint of the files
/// it wrote. Computing the fingerprint here, from the bytes just written, saves reading every
/// canonical file back a second time on the commit and undo/redo paths.
fn save_to_root(root: &Path, snapshot: &NotebookSnapshot) -> Result<u64, StorageError> {
    validate_snapshot(root, snapshot)?;
    let page_path = page_reference(&snapshot.manifest, &snapshot.page.id)?
        .path
        .clone();

    // Retain the exact bytes of each canonical (non-asset) file so the fingerprint matches what
    // reading the file back would produce.
    let mut written: HashMap<String, Vec<u8>> = HashMap::new();

    for file in &snapshot.blocks {
        write_atomic(root, &file.path, &file.bytes)?;
        written.insert(file.path.clone(), file.bytes.clone());
    }
    for file in &snapshot.assets {
        write_once(root, &file.path, &file.bytes)?;
    }
    for reference in &snapshot.page.ink_layers {
        let layer = snapshot
            .ink_layers
            .iter()
            .find(|layer| layer.id == reference.id)
            .expect("validated ink reference");
        let bytes = write_json_compact(root, &reference.path, layer)?;
        written.insert(reference.path.clone(), bytes);
    }
    written.insert(
        page_path.clone(),
        write_json(root, &page_path, &snapshot.page)?,
    );
    written.insert(
        "goodtype.json".to_owned(),
        write_json(root, "goodtype.json", &snapshot.manifest)?,
    );

    // The fingerprint file set must match `canonical_fingerprint` exactly, so derive its paths
    // the same way and hash the bytes just persisted.
    let mut paths = vec!["goodtype.json", page_path.as_str()];
    paths.extend(referenced_block_paths(&snapshot.manifest, &snapshot.page));
    paths.extend(
        snapshot
            .page
            .ink_layers
            .iter()
            .map(|reference| reference.path.as_str()),
    );
    let entries = paths
        .into_iter()
        .map(|path| {
            written
                .get(path)
                .map(|bytes| (path, bytes.as_slice()))
                .ok_or_else(|| invalid_error("a fingerprinted file was not written"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(fingerprint_files(&entries))
}

fn validate_manifest(manifest: &NotebookManifest) -> Result<(), StorageError> {
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(StorageError::InvalidNotebook(format!(
            "unsupported schema version {}",
            manifest.schema_version
        )));
    }
    if manifest.pages.is_empty() {
        return invalid("notebook must contain at least one page");
    }
    let mut ids = HashSet::new();
    let mut paths = HashSet::new();
    for page in &manifest.pages {
        if page.id.is_empty() || !ids.insert(page.id.as_str()) || !paths.insert(page.path.as_str())
        {
            return invalid("manifest page IDs and paths must be non-empty and unique");
        }
        validate_relative(&page.path)?;
        if !is_in_directory(&page.path, "pages") {
            return Err(StorageError::InvalidPath(page.path.clone()));
        }
    }
    if let Some(style) = &manifest.shared_style_path {
        validate_relative(style)?;
    }
    Ok(())
}

fn validate_manifest_files(root: &Path, manifest: &NotebookManifest) -> Result<(), StorageError> {
    for page in &manifest.pages {
        resolve_existing(root, &page.path)?;
    }
    Ok(())
}

fn page_reference<'a>(
    manifest: &'a NotebookManifest,
    page_id: &str,
) -> Result<&'a PageReference, StorageError> {
    manifest
        .pages
        .iter()
        .find(|page| page.id == page_id)
        .ok_or_else(|| invalid_error("page is not referenced by the notebook manifest"))
}

fn validate_snapshot(root: &Path, snapshot: &NotebookSnapshot) -> Result<(), StorageError> {
    validate_manifest(&snapshot.manifest)?;
    prepare_target(root, "goodtype.json")?;
    let page_reference = page_reference(&snapshot.manifest, &snapshot.page.id)?;
    prepare_target(root, &page_reference.path)?;
    if snapshot.page.schema_version != SCHEMA_VERSION {
        return Err(StorageError::InvalidNotebook(format!(
            "unsupported page schema version {}",
            snapshot.page.schema_version
        )));
    }
    validate_page_content(snapshot)?;

    let block_paths = referenced_block_paths(&snapshot.manifest, &snapshot.page);
    let asset_paths = referenced_asset_paths(&snapshot.page);
    validate_files(root, &snapshot.blocks, &block_paths, false)?;
    validate_files(root, &snapshot.assets, &asset_paths, true)?;
    validate_background(root, &snapshot.manifest.default_page.background)?;
    validate_background(root, &snapshot.page.background)?;

    for object in &snapshot.page.objects {
        if let PageObject::PdfMaterial { source_path, .. } = object {
            validate_relative(source_path)?;
            if !is_in_directory(source_path, "references") {
                return Err(StorageError::InvalidPath(source_path.clone()));
            }
            resolve_existing(root, source_path)?;
        }
    }

    if snapshot.ink_layers.len() != snapshot.page.ink_layers.len() {
        return Err(StorageError::InvalidNotebook(
            "ink layer references and payloads differ".into(),
        ));
    }
    for reference in &snapshot.page.ink_layers {
        validate_relative(&reference.path)?;
        if !is_in_directory(&reference.path, "ink") {
            return Err(StorageError::InvalidPath(reference.path.clone()));
        }
        prepare_target(root, &reference.path)?;
        let matches = snapshot
            .ink_layers
            .iter()
            .filter(|layer| layer.id == reference.id && layer.page_id == snapshot.page.id)
            .count();
        if matches != 1 {
            return Err(StorageError::InvalidNotebook(format!(
                "ink layer {} does not match its reference",
                reference.id
            )));
        }
    }
    Ok(())
}

fn validate_page_content(snapshot: &NotebookSnapshot) -> Result<(), StorageError> {
    let page = &snapshot.page;
    if page.id.is_empty() || !positive(page.geometry.width_pt) || !positive(page.geometry.height_pt)
    {
        return invalid("page ID and geometry must be valid");
    }

    let mut objects = HashMap::new();
    for object in &page.objects {
        let fields = object.fields();
        if fields.id.is_empty()
            || objects.insert(fields.id.as_str(), object).is_some()
            || !fields.x.is_finite()
            || !fields.y.is_finite()
            || !fields.rotation.is_finite()
            || !positive(fields.scale)
        {
            return invalid("object IDs must be unique and transforms must be finite");
        }
        match object {
            PageObject::Typst {
                layout_width_pt,
                measured_width_pt,
                measured_height_pt,
                ..
            } if !positive(*layout_width_pt)
                || !nonnegative(*measured_width_pt)
                || !nonnegative(*measured_height_pt) =>
            {
                return invalid("Typst dimensions must be finite and non-negative");
            }
            PageObject::Image {
                width_pt,
                height_pt,
                ..
            } if !positive(*width_pt) || !positive(*height_pt) => {
                return invalid("image dimensions must be finite and positive");
            }
            PageObject::PdfMaterial {
                source_width_pt,
                source_height_pt,
                ..
            } if !positive(*source_width_pt) || !positive(*source_height_pt) => {
                return invalid("PDF dimensions must be finite and positive");
            }
            _ => {}
        }
    }

    let top_level = page
        .objects
        .iter()
        .filter(|object| object.fields().group_id.is_none())
        .map(|object| object.fields().id.as_str())
        .collect::<HashSet<_>>();
    let reading_order = page
        .reading_order
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if reading_order.len() != page.reading_order.len() || reading_order != top_level {
        return invalid("reading order must contain every top-level object exactly once");
    }
    for (index, id) in page.reading_order.iter().enumerate() {
        if objects[id.as_str()].fields().reading_order != index as u32 {
            return invalid("object reading-order values must match the page reading order");
        }
    }

    validate_groups(&objects)?;
    validate_ink(snapshot, &objects)
}

fn validate_groups(objects: &HashMap<&str, &PageObject>) -> Result<(), StorageError> {
    for object in objects.values() {
        if let PageObject::Group { fields, child_ids } = object {
            let children = child_ids.iter().map(String::as_str).collect::<HashSet<_>>();
            if children.is_empty() || children.len() != child_ids.len() {
                return invalid("groups must contain unique child IDs");
            }
            for child_id in child_ids {
                let child = objects
                    .get(child_id.as_str())
                    .ok_or_else(|| invalid_error("group child does not exist"))?;
                if child.fields().group_id.as_deref() != Some(fields.id.as_str()) {
                    return invalid("group membership must agree in parent and child");
                }
            }
        }

        let mut seen = HashSet::new();
        let mut current = object.fields();
        while let Some(parent_id) = current.group_id.as_deref() {
            if !seen.insert(parent_id) {
                return invalid("groups cannot contain a cycle");
            }
            let parent = objects
                .get(parent_id)
                .ok_or_else(|| invalid_error("group parent does not exist"))?;
            if !matches!(parent, PageObject::Group { .. }) {
                return invalid("group parent must reference a group object");
            }
            current = parent.fields();
        }
    }
    Ok(())
}

fn validate_ink(
    snapshot: &NotebookSnapshot,
    objects: &HashMap<&str, &PageObject>,
) -> Result<(), StorageError> {
    let reference_ids = snapshot
        .page
        .ink_layers
        .iter()
        .map(|reference| reference.id.as_str())
        .collect::<HashSet<_>>();
    let reference_paths = snapshot
        .page
        .ink_layers
        .iter()
        .map(|reference| reference.path.as_str())
        .collect::<HashSet<_>>();
    if reference_ids.len() != snapshot.page.ink_layers.len()
        || reference_paths.len() != snapshot.page.ink_layers.len()
    {
        return invalid("ink-layer references must be unique");
    }

    let mut layers = HashMap::new();
    let mut strokes = HashMap::new();
    for layer in &snapshot.ink_layers {
        if layer.id.is_empty() || layers.insert(layer.id.as_str(), layer).is_some() {
            return invalid("ink-layer IDs must be non-empty and unique");
        }
        // Refuse an oversized layer here, before `save_to_root` writes anything. Relying on the
        // read ceiling alone would let a commit write a layer it can never read back, which
        // leaves the notebook unopenable.
        if layer.strokes.len() > MAX_INK_STROKES_PER_LAYER {
            return Err(StorageError::InvalidNotebook(format!(
                "ink layer {} holds {} strokes; maximum is {MAX_INK_STROKES_PER_LAYER}",
                layer.id,
                layer.strokes.len(),
            )));
        }
        let points = layer
            .strokes
            .iter()
            .map(|stroke| stroke.points.len())
            .sum::<usize>();
        if points > MAX_INK_POINTS_PER_LAYER {
            return Err(StorageError::InvalidNotebook(format!(
                "ink layer {} holds {points} samples; maximum is {MAX_INK_POINTS_PER_LAYER}",
                layer.id,
            )));
        }
        for stroke in &layer.strokes {
            if stroke.id.is_empty()
                || strokes
                    .insert(stroke.id.as_str(), (layer.id.as_str(), stroke))
                    .is_some()
                || !positive(stroke.width_pt)
                || !(0.0..=1.0).contains(&stroke.taper)
                || !(0.0..=1.0).contains(&stroke.opacity)
                || stroke.points.is_empty()
                || !positive(stroke.transform.scale_x)
                || !positive(stroke.transform.scale_y)
                || !stroke.transform.translate_x.is_finite()
                || !stroke.transform.translate_y.is_finite()
                || !stroke.transform.rotation.is_finite()
            {
                return invalid("stroke IDs, points, widths, tapers, and transforms must be valid");
            }
            if stroke.points.iter().any(|point| {
                !point.x.is_finite()
                    || !point.y.is_finite()
                    || !(0.0..=1.0).contains(&point.pressure)
                    || !nonnegative(point.time_ms)
                    || !(-90.0..=90.0).contains(&point.tilt_x)
                    || !(-90.0..=90.0).contains(&point.tilt_y)
            }) {
                return invalid("stroke samples must be finite and calibrated");
            }
        }
    }

    for object in objects.values() {
        if let PageObject::InkGroup {
            fields,
            ink_layer_id,
            stroke_ids,
        } = object
        {
            if !layers.contains_key(ink_layer_id.as_str()) {
                return invalid("ink group references an unknown layer");
            }
            let unique = stroke_ids
                .iter()
                .map(String::as_str)
                .collect::<HashSet<_>>();
            if unique.is_empty() || unique.len() != stroke_ids.len() {
                return invalid("ink groups must contain unique stroke IDs");
            }
            for stroke_id in stroke_ids {
                let Some((layer_id, stroke)) = strokes.get(stroke_id.as_str()) else {
                    return invalid("ink group references an unknown stroke");
                };
                if *layer_id != ink_layer_id || stroke.group_id.as_deref() != Some(&fields.id) {
                    return invalid("ink group membership must agree in object and stroke");
                }
            }
        }
    }

    for (layer_id, stroke) in strokes.values() {
        if let Some(group_id) = stroke.group_id.as_deref() {
            let Some(PageObject::InkGroup {
                ink_layer_id,
                stroke_ids,
                ..
            }) = objects.get(group_id)
            else {
                return invalid("stroke group does not exist");
            };
            if ink_layer_id != layer_id || !stroke_ids.iter().any(|id| id == &stroke.id) {
                return invalid("stroke group membership is incomplete");
            }
        }
    }
    Ok(())
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn invalid<T>(message: &str) -> Result<T, StorageError> {
    Err(invalid_error(message))
}

fn invalid_error(message: &str) -> StorageError {
    StorageError::InvalidNotebook(message.into())
}

fn validate_background(root: &Path, background: &PageBackground) -> Result<(), StorageError> {
    if let PageBackground::Pdf { source_path, .. } = background {
        validate_relative(source_path)?;
        if !is_in_directory(source_path, "references") {
            return Err(StorageError::InvalidPath(source_path.clone()));
        }
        resolve_existing(root, source_path)?;
    }
    Ok(())
}

fn validate_files(
    root: &Path,
    files: &[StoredFile],
    expected: &[&str],
    assets: bool,
) -> Result<(), StorageError> {
    for (index, file) in files.iter().enumerate() {
        validate_relative(&file.path)?;
        if !expected.contains(&file.path.as_str())
            || files[index + 1..]
                .iter()
                .any(|other| other.path == file.path)
        {
            return Err(StorageError::InvalidNotebook(format!(
                "unexpected or duplicate file {}",
                file.path
            )));
        }
        let target = prepare_target(root, &file.path)?;
        if assets {
            validate_asset_path(&file.path)?;
            if file.bytes.is_empty() || file.bytes.len() > MAX_IMAGE_BYTES {
                return Err(StorageError::ImageTooLarge {
                    size: file.bytes.len(),
                    maximum: MAX_IMAGE_BYTES,
                });
            }
            if target.exists() && fs::read(target)? != file.bytes {
                return Err(StorageError::AlreadyExists(file.path.clone()));
            }
        }
    }
    for path in expected {
        validate_relative(path)?;
        if assets {
            validate_asset_path(path)?;
        } else if *path != "style.typ" && !is_in_directory(path, "blocks") {
            return Err(StorageError::InvalidPath((*path).into()));
        }
        if !files.iter().any(|file| file.path == *path) {
            resolve_existing(root, path)?;
        }
    }
    Ok(())
}

fn referenced_block_paths<'a>(manifest: &'a NotebookManifest, page: &'a Page) -> Vec<&'a str> {
    let mut paths = Vec::new();
    if let Some(path) = manifest.shared_style_path.as_deref() {
        paths.push(path);
    }
    for object in &page.objects {
        if let PageObject::Typst { source_path, .. } = object
            && !paths.contains(&source_path.as_str())
        {
            paths.push(source_path);
        }
    }
    paths
}

fn referenced_asset_paths(page: &Page) -> Vec<&str> {
    let mut paths = Vec::new();
    for object in &page.objects {
        if let PageObject::Image { source_path, .. } = object
            && !paths.contains(&source_path.as_str())
        {
            paths.push(source_path);
        }
    }
    paths
}

fn read_stored_files(
    root: &Path,
    paths: &[&str],
    maximum: usize,
) -> Result<Vec<StoredFile>, StorageError> {
    paths
        .iter()
        .map(|path| {
            Ok(StoredFile {
                path: (*path).into(),
                bytes: read_limited(resolve_existing(root, path)?, maximum)?,
            })
        })
        .collect()
}

fn read_json<T: DeserializeOwned>(root: &Path, relative: &str) -> Result<T, StorageError> {
    read_json_limited(root, relative, MAX_JSON_BYTES)
}

fn read_json_limited<T: DeserializeOwned>(
    root: &Path,
    relative: &str,
    maximum: usize,
) -> Result<T, StorageError> {
    let bytes = read_limited(resolve_existing(root, relative)?, maximum)?;
    Ok(serde_json::from_slice(&bytes)?)
}

// Returns the exact bytes persisted, so a caller writing a set of canonical files can
// fingerprint them without reading them all back from disk.
fn write_json<T: Serialize>(
    root: &Path,
    relative: &str,
    value: &T,
) -> Result<Vec<u8>, StorageError> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write_atomic(root, relative, &bytes)?;
    Ok(bytes)
}

// Ink is machine-written sample data, not hand-edited structure. Pretty-printing it costs
// roughly 1.5x the bytes of every stroke on every commit for no inspection benefit that
// `jq` does not already provide.
fn write_json_compact<T: Serialize>(
    root: &Path,
    relative: &str,
    value: &T,
) -> Result<Vec<u8>, StorageError> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    write_atomic(root, relative, &bytes)?;
    Ok(bytes)
}

// One hashing procedure shared by the read path (`canonical_fingerprint`) and the write path
// (`save_to_root`) so a fingerprint computed from freshly written bytes equals one computed by
// reading the same files back. The fingerprint is process-local change detection, not a
// security signature.
fn fingerprint_files(entries: &[(&str, &[u8])]) -> u64 {
    let mut entries = entries.to_vec();
    entries.sort_unstable_by_key(|(path, _)| *path);
    entries.dedup_by_key(|(path, _)| *path);
    let mut fingerprint = DefaultHasher::new();
    for (path, bytes) in entries {
        path.hash(&mut fingerprint);
        bytes.hash(&mut fingerprint);
    }
    fingerprint.finish()
}

fn write_atomic(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), StorageError> {
    let target = prepare_target(root, relative)?;
    let mut temporary = NamedTempFile::new_in(target.parent().expect("target parent"))?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(&target)
        .map_err(|error| StorageError::Io(error.error))?;
    Ok(())
}

fn write_once(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), StorageError> {
    let target = prepare_target(root, relative)?;
    if target.exists() {
        if fs::read(&target)? == bytes {
            return Ok(());
        }
        return Err(StorageError::AlreadyExists(relative.into()));
    }

    let mut temporary = NamedTempFile::new_in(target.parent().expect("target parent"))?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist_noclobber(&target).map_err(|error| {
        if error.error.kind() == io::ErrorKind::AlreadyExists {
            StorageError::AlreadyExists(relative.into())
        } else {
            StorageError::Io(error.error)
        }
    })?;
    Ok(())
}

fn canonical_root(root: &Path) -> Result<PathBuf, StorageError> {
    let canonical = fs::canonicalize(root)?;
    if !canonical.is_dir() {
        return Err(StorageError::InvalidPath(root.display().to_string()));
    }
    Ok(canonical)
}

fn validate_relative(value: &str) -> Result<&Path, StorageError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains(['\\', ':'])
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(StorageError::InvalidPath(value.into()));
    }
    Ok(path)
}

fn resolve_existing(root: &Path, relative: &str) -> Result<PathBuf, StorageError> {
    let relative = validate_relative(relative)?;
    let resolved = fs::canonicalize(root.join(relative))?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err(StorageError::InvalidPath(relative.display().to_string()));
    }
    Ok(resolved)
}

fn prepare_target(root: &Path, relative: &str) -> Result<PathBuf, StorageError> {
    let relative = validate_relative(relative)?;
    let parent = ensure_directory(root, relative.parent().unwrap_or_else(|| Path::new("")))?;
    let target = parent.join(
        relative
            .file_name()
            .ok_or_else(|| StorageError::InvalidPath(relative.display().to_string()))?,
    );
    if target.exists() {
        let metadata = fs::symlink_metadata(&target)?;
        if metadata.file_type().is_symlink() {
            return Err(StorageError::InvalidPath(relative.display().to_string()));
        }
        let canonical = fs::canonicalize(&target)?;
        if !canonical.starts_with(root) || !canonical.is_file() {
            return Err(StorageError::InvalidPath(relative.display().to_string()));
        }
    }
    Ok(target)
}

fn ensure_directory(root: &Path, relative: &Path) -> Result<PathBuf, StorageError> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(StorageError::InvalidPath(relative.display().to_string()));
        };
        let candidate = current.join(name);
        if !candidate.exists() {
            fs::create_dir(&candidate)?;
        }
        current = fs::canonicalize(candidate)?;
        if !current.starts_with(root) || !current.is_dir() {
            return Err(StorageError::InvalidPath(relative.display().to_string()));
        }
    }
    Ok(current)
}

fn read_limited(path: PathBuf, maximum: usize) -> Result<Vec<u8>, StorageError> {
    let file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    if length > maximum as u64 {
        return Err(StorageError::InvalidNotebook(format!(
            "file is {length} bytes; maximum is {maximum}"
        )));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(StorageError::InvalidNotebook(format!(
            "file exceeds {maximum} bytes"
        )));
    }
    Ok(bytes)
}

fn is_in_directory(value: &str, directory: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(
        components.next(),
        Some(Component::Normal(component)) if component == directory
    ) && components.next().is_some()
}

fn validate_asset_path(value: &str) -> Result<(), StorageError> {
    let path = validate_relative(value)?;
    if path.parent() != Some(Path::new("assets")) {
        return Err(StorageError::InvalidPath(value.into()));
    }
    validate_safe_filename(
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(""),
    )
}

fn validate_safe_filename(filename: &str) -> Result<(), StorageError> {
    let safe_characters = filename
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
    let stem = filename
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if filename.is_empty()
        || filename.len() > 120
        || filename.starts_with('.')
        || filename.ends_with('.')
        || !filename.contains('.')
        || !safe_characters
        || reserved
    {
        return Err(StorageError::InvalidPath(filename.into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        InkLayerReference, ObjectFields, PageDefaults, PageGeometry, PageReference, Stroke,
        StrokePoint, StrokeTool, Transform,
    };

    const IMAGE_BYTES: &[u8] =
        include_bytes!("../../../fixtures/notebooks/phase0b/original-image.bin");

    fn fields(id: &str, reading_order: u32) -> ObjectFields {
        ObjectFields {
            id: id.into(),
            x: 72.0,
            y: 96.0,
            rotation: 0.0,
            scale: 1.0,
            z_index: reading_order as i32,
            reading_order,
            group_id: None,
            created_at: "2026-07-23T00:00:00Z".into(),
            modified_at: "2026-07-23T00:00:00Z".into(),
        }
    }

    fn snapshot() -> NotebookSnapshot {
        NotebookSnapshot {
            manifest: NotebookManifest {
                schema_version: SCHEMA_VERSION,
                id: "notebook-phase0b".into(),
                title: "Phase 0B persistence".into(),
                pages: vec![PageReference {
                    id: "page-001".into(),
                    path: "pages/page-001.json".into(),
                }],
                default_page: PageDefaults {
                    geometry: PageGeometry {
                        width_pt: 595.0,
                        height_pt: 842.0,
                    },
                    background: PageBackground::Plain {
                        color: "#ffffff".into(),
                    },
                },
                shared_style_path: None,
                created_at: "2026-07-23T00:00:00Z".into(),
                modified_at: "2026-07-23T00:00:00Z".into(),
            },
            page: Page {
                schema_version: SCHEMA_VERSION,
                id: "page-001".into(),
                revision: 1,
                geometry: PageGeometry {
                    width_pt: 595.0,
                    height_pt: 842.0,
                },
                background: PageBackground::Plain {
                    color: "#ffffff".into(),
                },
                objects: vec![
                    PageObject::Typst {
                        fields: fields("typst-001", 0),
                        source_path: "blocks/equation.typ".into(),
                        layout_width_pt: 220.0,
                        measured_width_pt: 180.0,
                        measured_height_pt: 32.0,
                    },
                    PageObject::Image {
                        fields: fields("image-001", 1),
                        source_path: "assets/diagram.png".into(),
                        width_pt: 120.0,
                        height_pt: 80.0,
                        alt_text: "Original pasted diagram".into(),
                    },
                ],
                reading_order: vec!["typst-001".into(), "image-001".into()],
                ink_layers: vec![InkLayerReference {
                    id: "ink-layer-001".into(),
                    path: "ink/page-001-layer-001.json".into(),
                }],
            },
            blocks: vec![StoredFile {
                path: "blocks/equation.typ".into(),
                bytes: b"$ F = m a $".to_vec(),
            }],
            assets: vec![StoredFile {
                path: "assets/diagram.png".into(),
                bytes: IMAGE_BYTES.to_vec(),
            }],
            ink_layers: vec![InkLayer {
                schema_version: SCHEMA_VERSION,
                id: "ink-layer-001".into(),
                page_id: "page-001".into(),
                strokes: vec![Stroke {
                    id: "stroke-001".into(),
                    tool: StrokeTool::Pen,
                    color: "#111111".into(),
                    width_pt: 2.0,
                    pressure: true,
                    taper: 0.0,
                    opacity: 1.0,
                    group_id: None,
                    points: vec![StrokePoint {
                        x: 80.0,
                        y: 140.0,
                        pressure: 0.75,
                        time_ms: 0.0,
                        tilt_x: 0.0,
                        tilt_y: 0.0,
                    }],
                    transform: Transform {
                        translate_x: 0.0,
                        translate_y: 0.0,
                        scale_x: 1.0,
                        scale_y: 1.0,
                        rotation: 0.0,
                    },
                }],
            }],
        }
    }

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

        let second = create_page(&notebook_root, "2026-07-23T18:00:00Z").unwrap();
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
    fn recovers_each_interrupted_replacement_boundary() {
        for completed_writes in 0..=4 {
            let temporary = tempfile::tempdir().unwrap();
            let notebook_root = temporary.path().join("notebook");
            create_notebook(&notebook_root, &snapshot()).unwrap();
            let root = canonical_root(&notebook_root).unwrap();
            let previous = open_notebook(&root).unwrap();
            let mut candidate = previous.clone();
            candidate.page.revision = 2;
            candidate.manifest.modified_at = "2026-07-23T12:00:00Z".into();
            candidate.blocks[0].bytes = b"$ interrupted = true $".to_vec();
            candidate.ink_layers[0].strokes[0].points[0].x = 160.0;
            let PageObject::Image { fields, .. } = &mut candidate.page.objects[1] else {
                panic!("image object missing");
            };
            fields.x = 144.0;
            let intent = RecoveryIntent {
                version: 1,
                previous: history_snapshot(previous),
                candidate: history_snapshot(candidate.clone()),
            };

            write_recovery_intent(&root, &intent).unwrap();
            if completed_writes >= 1 {
                write_atomic(&root, &candidate.blocks[0].path, &candidate.blocks[0].bytes).unwrap();
            }
            if completed_writes >= 2 {
                write_json(
                    &root,
                    &candidate.page.ink_layers[0].path,
                    &candidate.ink_layers[0],
                )
                .unwrap();
            }
            if completed_writes >= 3 {
                write_json(&root, &candidate.manifest.pages[0].path, &candidate.page).unwrap();
            }
            if completed_writes >= 4 {
                write_json(&root, "goodtype.json", &candidate.manifest).unwrap();
            }

            let recovered = open_notebook(&root).unwrap();
            assert!(!root.join(PENDING_TRANSACTION_PATH).exists());
            if completed_writes < 3 {
                assert_eq!(recovered.page.revision, 1);
                assert_eq!(recovered.blocks[0].bytes, b"$ F = m a $");
                assert_eq!(recovered.ink_layers[0].strokes[0].points[0].x, 80.0);
                assert_eq!(recovered.manifest.modified_at, "2026-07-23T00:00:00Z");
                assert_eq!(
                    fs::read_dir(root.join(".goodtype").join("recovery"))
                        .unwrap()
                        .count(),
                    1
                );
            } else {
                assert_eq!(recovered.page.revision, 2);
                assert_eq!(recovered.blocks[0].bytes, candidate.blocks[0].bytes);
                assert_eq!(recovered.ink_layers[0].strokes[0].points[0].x, 160.0);
                assert_eq!(
                    recovered.manifest.modified_at,
                    candidate.manifest.modified_at
                );
                assert!(!root.join(".goodtype").join("recovery").exists());
            }
        }
    }

    #[test]
    fn blocks_new_commits_when_recovery_is_full() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        let recovery = notebook_root.join(".goodtype").join("recovery");
        fs::create_dir_all(&recovery).unwrap();
        for index in 0..RECOVERY_CANDIDATE_LIMIT {
            fs::write(
                recovery.join(format!("candidate-{index}.json")),
                b"preserved",
            )
            .unwrap();
        }
        let mut history = NotebookHistory::default();
        let mut changed = observe_notebook(&notebook_root, &mut history).unwrap();
        let PageObject::Image { fields, .. } = &mut changed.page.objects[1] else {
            panic!("image object missing");
        };
        fields.x = 144.0;

        let error = commit_notebook(&notebook_root, &mut history, changed).unwrap_err();
        assert!(matches!(
            error,
            StorageError::InvalidNotebook(message) if message.contains("unresolved candidates")
        ));
        assert_eq!(open_notebook(&notebook_root).unwrap().page.revision, 1);
    }

    #[test]
    fn rejects_inconsistent_canonical_page_content() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let mut duplicate_id = open_notebook(&notebook_root).unwrap();
        let PageObject::Image { fields, .. } = &mut duplicate_id.page.objects[1] else {
            panic!("image object missing");
        };
        fields.id = "typst-001".into();
        assert!(matches!(
            save_notebook(&notebook_root, &duplicate_id),
            Err(StorageError::InvalidNotebook(_))
        ));

        let mut bad_order = open_notebook(&notebook_root).unwrap();
        bad_order.page.reading_order.pop();
        assert!(matches!(
            save_notebook(&notebook_root, &bad_order),
            Err(StorageError::InvalidNotebook(_))
        ));

        let mut bad_ink = open_notebook(&notebook_root).unwrap();
        bad_ink.ink_layers[0].strokes[0].points[0].pressure = 1.5;
        assert!(matches!(
            save_notebook(&notebook_root, &bad_ink),
            Err(StorageError::InvalidNotebook(_))
        ));
    }

    #[test]
    fn rejects_a_path_escape() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        let mut invalid = snapshot();
        let PageObject::Typst { source_path, .. } = &mut invalid.page.objects[0] else {
            unreachable!();
        };
        *source_path = "../escape.typ".into();
        invalid.blocks[0].path = "../escape.typ".into();

        let error = create_notebook(&notebook_root, &invalid).unwrap_err();
        assert!(matches!(error, StorageError::InvalidPath(_)));
        assert!(!temporary.path().join("escape.typ").exists());
    }

    #[test]
    fn pasted_image_preserves_bytes_and_never_overwrites() {
        let temporary = tempfile::tempdir().unwrap();
        let relative =
            store_pasted_image(temporary.path(), "pasted-image.png", IMAGE_BYTES).unwrap();
        assert_eq!(relative, "assets/pasted-image.png");
        assert_eq!(
            fs::read(temporary.path().join(&relative)).unwrap(),
            IMAGE_BYTES
        );

        let error =
            store_pasted_image(temporary.path(), "pasted-image.png", b"different").unwrap_err();
        assert!(matches!(error, StorageError::AlreadyExists(_)));
        assert_eq!(
            fs::read(temporary.path().join(relative)).unwrap(),
            IMAGE_BYTES
        );
    }

    /// Samples shaped like the frontend writes them: quantized to the precision in
    /// `apps/desktop/src/lib/ink/pipeline.ts`, so the size this test measures is the
    /// size a real notebook reaches.
    fn handwriting_layer(strokes: usize, points_per_stroke: usize) -> InkLayer {
        InkLayer {
            schema_version: SCHEMA_VERSION,
            id: "ink-layer-001".into(),
            page_id: "page-001".into(),
            strokes: (0..strokes)
                .map(|stroke| Stroke {
                    id: format!("stroke-{stroke:06}"),
                    tool: StrokeTool::Pen,
                    color: "#101418".into(),
                    width_pt: 1.6,
                    pressure: true,
                    taper: 0.0,
                    opacity: 1.0,
                    group_id: None,
                    points: (0..points_per_stroke)
                        .map(|point| {
                            let step = (stroke * points_per_stroke + point) as f64;
                            StrokePoint {
                                x: ((step * 7.31) % 59500.0).round() / 100.0,
                                y: ((step * 11.17) % 84200.0).round() / 100.0,
                                pressure: ((step % 1000.0) / 1000.0 * 1000.0).round() / 1000.0,
                                time_ms: (step * 83.0).round() / 10.0,
                                tilt_x: ((step % 900.0) / 10.0 - 45.0).round(),
                                tilt_y: ((step % 700.0) / 10.0 - 35.0).round(),
                            }
                        })
                        .collect(),
                    transform: Transform {
                        translate_x: 0.0,
                        translate_y: 0.0,
                        scale_x: 1.0,
                        scale_y: 1.0,
                        rotation: 0.0,
                    },
                })
                .collect(),
        }
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

    /// Locks the optimization that computes the post-write fingerprint from the bytes just
    /// written instead of reading every canonical file back. If that fingerprint ever diverged
    /// from what the files hash to on disk, the next commit's `ensure_current` would falsely
    /// report an external change — so five commits in a row, each reusing the returned
    /// snapshot, prove the two agree.
    #[test]
    fn commits_repeatedly_without_a_false_external_change() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let mut history = NotebookHistory::default();
        let mut working = observe_notebook(&notebook_root, &mut history).unwrap();
        for revision in 2..=6 {
            let mut stroke = snapshot().ink_layers[0].strokes[0].clone();
            stroke.id = format!("stroke-{revision:03}");
            working.ink_layers[0].strokes.push(stroke);
            let result = commit_notebook(&notebook_root, &mut history, working).unwrap();
            assert_eq!(result.snapshot.page.revision, revision);
            working = result.snapshot;
        }

        let reopened = open_notebook(&notebook_root).unwrap();
        assert_eq!(reopened.page.revision, 6);
        assert_eq!(reopened.ink_layers[0].strokes.len(), 6);
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
    fn deletes_and_reorders_pages_with_guards() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        create_page(&notebook_root, "2026-07-23T19:00:00Z").unwrap();
        create_page(&notebook_root, "2026-07-23T19:01:00Z").unwrap();

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
    fn lists_restores_and_discards_recovery_candidates() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        let root = fs::canonicalize(&notebook_root).unwrap();

        // Simulate an interrupted transaction whose candidate was never written: reopen rolls
        // back to the confirmed state and archives the candidate.
        let previous = open_notebook(&notebook_root).unwrap();
        let mut candidate = history_snapshot(previous.clone());
        candidate.page.revision = 2;
        let mut extra = candidate.ink_layers[0].strokes[0].clone();
        extra.id = "stroke-recovered".into();
        candidate.ink_layers[0].strokes.push(extra);
        write_recovery_intent(
            &root,
            &RecoveryIntent {
                version: 1,
                previous: history_snapshot(previous.clone()),
                candidate,
            },
        )
        .unwrap();
        let reopened = open_notebook(&notebook_root).unwrap();
        assert_eq!(reopened.page.revision, 1);

        let candidates = list_recovery_candidates(&notebook_root).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].page_id, "page-001");
        assert_eq!(candidates[0].candidate_revision, 2);

        // Restoring lands the candidate's work as a fresh committed revision.
        let mut history = NotebookHistory::default();
        let restored =
            restore_recovery_candidate(&notebook_root, &mut history, &candidates[0].file_name)
                .unwrap();
        assert_eq!(restored.snapshot.page.revision, 2);
        assert_eq!(restored.snapshot.ink_layers[0].strokes.len(), 2);
        assert!(restored.can_undo);
        assert!(list_recovery_candidates(&notebook_root).unwrap().is_empty());

        // Discard removes without applying.
        let previous = open_notebook(&notebook_root).unwrap();
        let mut candidate = history_snapshot(previous.clone());
        candidate.page.revision = 3;
        write_recovery_intent(
            &root,
            &RecoveryIntent {
                version: 1,
                previous: history_snapshot(previous),
                candidate,
            },
        )
        .unwrap();
        open_notebook(&notebook_root).unwrap();
        let candidates = list_recovery_candidates(&notebook_root).unwrap();
        assert_eq!(candidates.len(), 1);
        discard_recovery_candidate(&notebook_root, &candidates[0].file_name).unwrap();
        assert!(list_recovery_candidates(&notebook_root).unwrap().is_empty());
        assert!(matches!(
            discard_recovery_candidate(&notebook_root, "../../goodtype.json"),
            Err(StorageError::InvalidPath(_))
        ));
    }

    #[test]
    fn searches_typst_sources_across_pages() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        duplicate_page(&notebook_root, "page-001", "2026-07-23T19:00:00Z").unwrap();

        let hits = search_notebook(&notebook_root, "F = m a").unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(
            (hits[0].page_number, hits[0].page_id.as_str()),
            (1, "page-001")
        );
        assert_eq!(
            (hits[1].page_number, hits[1].page_id.as_str()),
            (2, "page-002")
        );
        assert!(hits[0].excerpt.contains("F = m a"));
        // Case-insensitive; no hit for absent text.
        assert_eq!(search_notebook(&notebook_root, "f = M A").unwrap().len(), 2);
        assert!(
            search_notebook(&notebook_root, "entropy")
                .unwrap()
                .is_empty()
        );
        assert!(search_notebook(&notebook_root, "   ").is_err());
    }

    /// An over-budget layer must be refused before anything reaches disk, so the previous
    /// confirmed revision stays openable.
    #[test]
    fn refuses_an_oversized_ink_layer_without_writing_it() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();

        let mut history = NotebookHistory::default();
        let mut oversized = observe_notebook(&notebook_root, &mut history).unwrap();
        oversized.ink_layers = vec![handwriting_layer(MAX_INK_STROKES_PER_LAYER + 1, 1)];

        let error = commit_notebook(&notebook_root, &mut history, oversized).unwrap_err();
        assert!(matches!(error, StorageError::InvalidNotebook(_)));

        let confirmed = open_notebook(&notebook_root).unwrap();
        assert_eq!(confirmed.page.revision, 1);
        assert_eq!(confirmed.ink_layers[0].strokes.len(), 1);
        assert!(!notebook_root.join(PENDING_TRANSACTION_PATH).exists());
    }
}
