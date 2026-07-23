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

use crate::{InkLayer, NotebookManifest, Page, PageBackground, PageObject, SCHEMA_VERSION};

pub const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;
const MAX_BLOCK_BYTES: usize = 1024 * 1024;
const MAX_RECOVERY_BYTES: usize = 64 * 1024 * 1024;
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
    save_to_root(&root, snapshot)
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
    save_to_root(&root, snapshot)
}

pub fn open_notebook(selected_root: &Path) -> Result<NotebookSnapshot, StorageError> {
    let root = canonical_root(selected_root)?;
    recover_interrupted_transaction(&root)?;
    let manifest: NotebookManifest = read_json(&root, "goodtype.json")?;
    validate_manifest(&manifest)?;

    let page: Page = read_json(&root, &manifest.pages[0].path)?;
    let ink_layers = page
        .ink_layers
        .iter()
        .map(|reference| read_json(&root, &reference.path))
        .collect::<Result<Vec<_>, _>>()?;

    let block_paths = referenced_block_paths(&manifest, &page);
    let asset_paths = referenced_asset_paths(&page);
    let blocks = read_stored_files(&root, &block_paths, MAX_BLOCK_BYTES)?;
    let assets = read_stored_files(&root, &asset_paths, MAX_IMAGE_BYTES)?;
    let snapshot = NotebookSnapshot {
        manifest,
        page,
        blocks,
        assets,
        ink_layers,
    };
    validate_snapshot(&root, &snapshot)?;
    Ok(snapshot)
}

pub fn observe_notebook(
    selected_root: &Path,
    history: &mut NotebookHistory,
) -> Result<NotebookSnapshot, StorageError> {
    let root = canonical_root(selected_root)?;
    let snapshot = open_notebook(&root)?;
    let fingerprint = canonical_fingerprint(&root, &snapshot)?;
    if history.current_revision != Some(snapshot.page.revision)
        || history.current_fingerprint != Some(fingerprint)
    {
        history.undo.clear();
        history.redo.clear();
    }
    history.current_revision = Some(snapshot.page.revision);
    history.current_fingerprint = Some(fingerprint);
    Ok(snapshot)
}

pub fn commit_notebook(
    selected_root: &Path,
    history: &mut NotebookHistory,
    mut snapshot: NotebookSnapshot,
) -> Result<HistoryResult, StorageError> {
    let root = canonical_root(selected_root)?;
    let current = open_notebook(selected_root)?;
    ensure_current(&root, history, &current, snapshot.page.revision)?;
    snapshot.page.revision = current.page.revision + 1;
    save_transactional(&root, &current, &snapshot)?;
    let saved = open_notebook(&root)?;
    push_history(&mut history.undo, history_snapshot(current));
    history.redo.clear();
    history.current_revision = Some(saved.page.revision);
    history.current_fingerprint = Some(canonical_fingerprint(&root, &saved)?);
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
    let current = open_notebook(&root)?;
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
    save_transactional(&root, &current, &target)?;
    let restored = open_notebook(&root)?;

    if undo {
        history.undo.pop();
        push_history(&mut history.redo, history_snapshot(current));
    } else {
        history.redo.pop();
        push_history(&mut history.undo, history_snapshot(current));
    }
    history.current_revision = Some(restored.page.revision);
    history.current_fingerprint = Some(canonical_fingerprint(&root, &restored)?);
    Ok(history_result(restored, history))
}

fn ensure_current(
    root: &Path,
    history: &mut NotebookHistory,
    current: &NotebookSnapshot,
    expected: u64,
) -> Result<(), StorageError> {
    if current.page.revision == expected
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
    let mut files = vec![
        ("goodtype.json", MAX_JSON_BYTES),
        (snapshot.manifest.pages[0].path.as_str(), MAX_JSON_BYTES),
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
            .map(|reference| (reference.path.as_str(), MAX_JSON_BYTES)),
    );
    files.sort_unstable_by_key(|(path, _)| *path);
    files.dedup_by_key(|(path, _)| *path);

    // ponytail: this process-local fingerprint detects changes; it is not a security signature.
    let mut fingerprint = DefaultHasher::new();
    for (relative, maximum) in files {
        relative.hash(&mut fingerprint);
        read_limited(resolve_existing(root, relative)?, maximum)?.hash(&mut fingerprint);
    }
    Ok(fingerprint.finish())
}

fn save_transactional(
    root: &Path,
    previous: &NotebookSnapshot,
    candidate: &NotebookSnapshot,
) -> Result<(), StorageError> {
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
    save_to_root(root, candidate)?;
    remove_pending_transaction(root)
}

fn write_recovery_intent(root: &Path, intent: &RecoveryIntent) -> Result<(), StorageError> {
    let mut bytes = serde_json::to_vec_pretty(intent)?;
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

    let current_revision = read_json::<Page>(root, &intent.candidate.manifest.pages[0].path)
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

fn save_to_root(root: &Path, snapshot: &NotebookSnapshot) -> Result<(), StorageError> {
    validate_snapshot(root, snapshot)?;

    for file in &snapshot.blocks {
        write_atomic(root, &file.path, &file.bytes)?;
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
        write_json(root, &reference.path, layer)?;
    }
    write_json(root, &snapshot.manifest.pages[0].path, &snapshot.page)?;
    write_json(root, "goodtype.json", &snapshot.manifest)
}

fn validate_manifest(manifest: &NotebookManifest) -> Result<(), StorageError> {
    if manifest.schema_version != SCHEMA_VERSION {
        return Err(StorageError::InvalidNotebook(format!(
            "unsupported schema version {}",
            manifest.schema_version
        )));
    }
    if manifest.pages.len() != 1 {
        return Err(StorageError::InvalidNotebook(
            "Phase 0 supports exactly one page".into(),
        ));
    }
    validate_relative(&manifest.pages[0].path)?;
    if !is_in_directory(&manifest.pages[0].path, "pages") {
        return Err(StorageError::InvalidPath(manifest.pages[0].path.clone()));
    }
    if let Some(style) = &manifest.shared_style_path {
        validate_relative(style)?;
    }
    Ok(())
}

fn validate_snapshot(root: &Path, snapshot: &NotebookSnapshot) -> Result<(), StorageError> {
    validate_manifest(&snapshot.manifest)?;
    prepare_target(root, "goodtype.json")?;
    prepare_target(root, &snapshot.manifest.pages[0].path)?;
    if snapshot.page.schema_version != SCHEMA_VERSION {
        return Err(StorageError::InvalidNotebook(format!(
            "unsupported page schema version {}",
            snapshot.page.schema_version
        )));
    }
    if snapshot.manifest.pages[0].id != snapshot.page.id {
        return Err(StorageError::InvalidNotebook(
            "manifest and page IDs differ".into(),
        ));
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
        for stroke in &layer.strokes {
            if stroke.id.is_empty()
                || strokes
                    .insert(stroke.id.as_str(), (layer.id.as_str(), stroke))
                    .is_some()
                || !positive(stroke.width_pt)
                || stroke.points.is_empty()
                || !positive(stroke.transform.scale_x)
                || !positive(stroke.transform.scale_y)
                || !stroke.transform.translate_x.is_finite()
                || !stroke.transform.translate_y.is_finite()
                || !stroke.transform.rotation.is_finite()
            {
                return invalid("stroke IDs, points, widths, and transforms must be valid");
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
    let bytes = read_limited(resolve_existing(root, relative)?, MAX_JSON_BYTES)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn write_json<T: Serialize>(root: &Path, relative: &str, value: &T) -> Result<(), StorageError> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write_atomic(root, relative, &bytes)
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
}
