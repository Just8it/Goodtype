//! Transaction recovery with compact binary ink and legacy JSON compatibility.

use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{Page, ink_codec, layout};

use super::{
    MAX_RECOVERY_BYTES, NotebookSnapshot, RECOVERY_CANDIDATE_LIMIT, StorageError, files::*,
    history::*, invalid, invalid_error, paths::*, validate::*, write::*,
};

const MAGIC: &[u8; 8] = b"GTREC\0\r\n";
const FORMAT_VERSION: u32 = 2;
const HEADER_BYTES: usize = 16;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BinaryMetadata {
    previous: StoredSnapshot,
    candidate: StoredSnapshot,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredSnapshot {
    snapshot: NotebookSnapshot,
    ink_layer_lengths: Vec<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyIntent {
    version: u32,
    previous: NotebookSnapshot,
    candidate: NotebookSnapshot,
}

#[derive(Clone, Copy)]
enum IntentFormat {
    Binary,
    Legacy,
}

struct RecoveryIntent {
    previous: NotebookSnapshot,
    candidate: NotebookSnapshot,
}

/// Record both states before a transactional write touches canonical files.
pub(crate) fn stage_transaction(
    root: &Path,
    previous: &NotebookSnapshot,
    candidate: &NotebookSnapshot,
) -> Result<(), StorageError> {
    if root.join(layout::PENDING_TRANSACTION).exists()
        || root.join(layout::LEGACY_PENDING_TRANSACTION).exists()
    {
        return invalid("a pending recovery transaction already exists");
    }
    let bytes = encode_intent(previous, candidate)?;
    write_atomic(root, layout::PENDING_TRANSACTION, &bytes)
}

fn encode_intent(
    previous: &NotebookSnapshot,
    candidate: &NotebookSnapshot,
) -> Result<Vec<u8>, StorageError> {
    let mut payload_bytes = 0usize;
    let (previous, previous_layers) = store_snapshot(previous, &mut payload_bytes)?;
    let (candidate, candidate_layers) = store_snapshot(candidate, &mut payload_bytes)?;
    let metadata = serde_json::to_vec(&BinaryMetadata {
        previous,
        candidate,
    })?;
    let metadata_len = u32::try_from(metadata.len())
        .map_err(|_| invalid_error("recovery metadata length exceeds u32"))?;
    let total_len = HEADER_BYTES
        .checked_add(metadata.len())
        .and_then(|value| value.checked_add(payload_bytes))
        .ok_or_else(|| invalid_error("recovery data length overflow"))?;
    if total_len > MAX_RECOVERY_BYTES {
        return Err(StorageError::InvalidNotebook(format!(
            "recovery data is {total_len} bytes; maximum is {MAX_RECOVERY_BYTES}"
        )));
    }

    let mut bytes = Vec::with_capacity(total_len);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&metadata_len.to_le_bytes());
    bytes.extend_from_slice(&metadata);
    for layer in previous_layers.into_iter().chain(candidate_layers) {
        bytes.extend_from_slice(&layer);
    }
    debug_assert_eq!(bytes.len(), total_len);
    Ok(bytes)
}

fn store_snapshot(
    snapshot: &NotebookSnapshot,
    payload_bytes: &mut usize,
) -> Result<(StoredSnapshot, Vec<Vec<u8>>), StorageError> {
    let metadata_snapshot = NotebookSnapshot {
        manifest: snapshot.manifest.clone(),
        page: snapshot.page.clone(),
        blocks: snapshot.blocks.clone(),
        assets: Vec::new(),
        ink_layers: Vec::new(),
    };
    let mut layers = Vec::with_capacity(snapshot.ink_layers.len());
    let mut lengths = Vec::with_capacity(snapshot.ink_layers.len());
    for layer in &snapshot.ink_layers {
        let encoded_len = ink_codec::encoded_len(layer)?;
        let next_payload = payload_bytes
            .checked_add(encoded_len)
            .ok_or_else(|| invalid_error("recovery payload length overflow"))?;
        if next_payload > MAX_RECOVERY_BYTES {
            return Err(StorageError::InvalidNotebook(format!(
                "recovery ink payload is {next_payload} bytes; maximum is {MAX_RECOVERY_BYTES}"
            )));
        }
        let encoded = ink_codec::encode(layer)?;
        if encoded.len() != encoded_len {
            return invalid("binary ink encoded length changed during recovery staging");
        }
        *payload_bytes = next_payload;
        lengths.push(
            u32::try_from(encoded.len())
                .map_err(|_| invalid_error("recovery ink layer length exceeds u32"))?,
        );
        layers.push(encoded);
    }
    Ok((
        StoredSnapshot {
            snapshot: metadata_snapshot,
            ink_layer_lengths: lengths,
        },
        layers,
    ))
}

fn decode_binary(bytes: &[u8]) -> Result<RecoveryIntent, StorageError> {
    if bytes.len() > MAX_RECOVERY_BYTES {
        return Err(StorageError::InvalidNotebook(format!(
            "recovery data is {} bytes; maximum is {MAX_RECOVERY_BYTES}",
            bytes.len()
        )));
    }
    if bytes.len() < HEADER_BYTES {
        return invalid("recovery header is truncated");
    }
    if &bytes[..MAGIC.len()] != MAGIC {
        return invalid("recovery magic is invalid");
    }
    let version = read_u32(bytes, 8)?;
    if version != FORMAT_VERSION {
        return Err(StorageError::InvalidNotebook(format!(
            "unsupported recovery format version {version}"
        )));
    }
    let metadata_len = usize::try_from(read_u32(bytes, 12)?)
        .map_err(|_| invalid_error("recovery metadata length is unsupported"))?;
    let metadata_end = HEADER_BYTES
        .checked_add(metadata_len)
        .ok_or_else(|| invalid_error("recovery metadata offset overflow"))?;
    let metadata_slice = bytes
        .get(HEADER_BYTES..metadata_end)
        .ok_or_else(|| invalid_error("recovery metadata is truncated"))?;
    let metadata: BinaryMetadata = serde_json::from_slice(metadata_slice)?;

    // Prove every declared blob fits and the envelope has no trailing bytes before the codec
    // allocates any point arrays.
    let expected_end = metadata
        .previous
        .ink_layer_lengths
        .iter()
        .chain(&metadata.candidate.ink_layer_lengths)
        .try_fold(metadata_end, |offset, length| {
            let length = usize::try_from(*length)
                .map_err(|_| invalid_error("recovery ink layer length is unsupported"))?;
            offset
                .checked_add(length)
                .ok_or_else(|| invalid_error("recovery ink layer offset overflow"))
        })?;
    if expected_end != bytes.len() {
        return Err(StorageError::InvalidNotebook(format!(
            "recovery data is {} bytes; metadata requires {expected_end}",
            bytes.len()
        )));
    }

    let mut offset = metadata_end;
    let previous = load_snapshot(metadata.previous, bytes, &mut offset)?;
    let candidate = load_snapshot(metadata.candidate, bytes, &mut offset)?;
    debug_assert_eq!(offset, bytes.len());
    Ok(RecoveryIntent {
        previous,
        candidate,
    })
}

fn load_snapshot(
    mut stored: StoredSnapshot,
    bytes: &[u8],
    offset: &mut usize,
) -> Result<NotebookSnapshot, StorageError> {
    if !stored.snapshot.ink_layers.is_empty() || !stored.snapshot.assets.is_empty() {
        return invalid("recovery metadata must not embed ink layers or asset bytes");
    }
    if stored.ink_layer_lengths.len() != stored.snapshot.page.ink_layers.len() {
        return invalid("recovery ink layer lengths do not match page references");
    }
    let mut layers = Vec::with_capacity(stored.ink_layer_lengths.len());
    for length in stored.ink_layer_lengths {
        let length = usize::try_from(length)
            .map_err(|_| invalid_error("recovery ink layer length is unsupported"))?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| invalid_error("recovery ink layer offset overflow"))?;
        let encoded = bytes
            .get(*offset..end)
            .ok_or_else(|| invalid_error("recovery ink layer is truncated"))?;
        layers.push(ink_codec::decode(encoded)?);
        *offset = end;
    }
    stored.snapshot.ink_layers = layers;
    Ok(stored.snapshot)
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, StorageError> {
    let raw = bytes
        .get(offset..offset + size_of::<u32>())
        .ok_or_else(|| invalid_error("recovery header is truncated"))?;
    Ok(u32::from_le_bytes(raw.try_into().map_err(|_| {
        invalid_error("recovery header field width is invalid")
    })?))
}

fn decode_legacy(bytes: &[u8]) -> Result<RecoveryIntent, StorageError> {
    let intent: LegacyIntent = serde_json::from_slice(bytes)?;
    if intent.version != 1 {
        return invalid("unsupported legacy recovery intent version");
    }
    Ok(RecoveryIntent {
        previous: intent.previous,
        candidate: intent.candidate,
    })
}

fn validate_intent(intent: &RecoveryIntent) -> Result<(), StorageError> {
    validate_snapshot_versions(&intent.previous)?;
    validate_snapshot_versions(&intent.candidate)?;
    if intent.previous.manifest.id != intent.candidate.manifest.id
        || intent.previous.page.id != intent.candidate.page.id
        || page_reference(&intent.previous.manifest, &intent.previous.page.id)?.path
            != page_reference(&intent.candidate.manifest, &intent.candidate.page.id)?.path
    {
        return invalid("recovery states must describe the same notebook and page path");
    }
    let expected_revision = intent
        .previous
        .page
        .revision
        .checked_add(1)
        .ok_or_else(|| invalid_error("recovery revision overflow"))?;
    if intent.candidate.page.revision != expected_revision {
        return invalid("recovery revisions are not consecutive");
    }
    Ok(())
}

fn read_intent(
    root: &Path,
    relative: &str,
    format: IntentFormat,
) -> Result<RecoveryIntent, StorageError> {
    let bytes = read_limited(resolve_existing(root, relative)?, MAX_RECOVERY_BYTES)?;
    let intent = match format {
        IntentFormat::Binary => decode_binary(&bytes)?,
        IntentFormat::Legacy => decode_legacy(&bytes)?,
    };
    validate_intent(&intent)?;
    Ok(intent)
}

pub(crate) fn ensure_recovery_capacity(root: &Path) -> Result<(), StorageError> {
    if candidate_names(root)?.len() >= RECOVERY_CANDIDATE_LIMIT {
        return Err(StorageError::RecoveryLimit {
            maximum: RECOVERY_CANDIDATE_LIMIT,
        });
    }
    Ok(())
}

pub(crate) fn recover_interrupted_transaction(root: &Path) -> Result<(), StorageError> {
    let binary = root.join(layout::PENDING_TRANSACTION).exists();
    let legacy = root.join(layout::LEGACY_PENDING_TRANSACTION).exists();
    if binary && legacy {
        return invalid("both binary and legacy pending recovery transactions exist");
    }
    let (pending, format) = if binary {
        (layout::PENDING_TRANSACTION, IntentFormat::Binary)
    } else if legacy {
        (layout::LEGACY_PENDING_TRANSACTION, IntentFormat::Legacy)
    } else {
        return Ok(());
    };
    let intent = read_intent(root, pending, format)?;
    validate_snapshot(root, &intent.previous)?;
    validate_snapshot(root, &intent.candidate)?;

    let candidate_reference =
        page_reference(&intent.candidate.manifest, &intent.candidate.page.id)?;
    let current_revision = read_json::<Page>(root, &candidate_reference.path)
        .map(|page| page.revision)
        .ok();
    if current_revision == Some(intent.candidate.page.revision) {
        save_to_root(root, &intent.candidate)?;
        return remove_pending(root, pending);
    }

    restore_previous_to_root(root, &intent.previous, &intent.candidate)?;
    archive_pending(root, pending, format, intent.candidate.page.revision)
}

pub(crate) fn remove_pending_transaction(root: &Path) -> Result<(), StorageError> {
    remove_pending(root, layout::PENDING_TRANSACTION)
}

fn remove_pending(root: &Path, relative: &str) -> Result<(), StorageError> {
    fs::remove_file(resolve_existing(root, relative)?)?;
    Ok(())
}

fn archive_pending(
    root: &Path,
    pending_relative: &str,
    format: IntentFormat,
    revision: u64,
) -> Result<(), StorageError> {
    let pending = resolve_existing(root, pending_relative)?;
    // Recovery is an internal store-owned directory. General notebook-relative paths reject
    // hidden components, so resolve this fixed location directly under the canonical root.
    let recovery =
        crate::paths::ensure_dir(root, &root.join(layout::RECOVERY_DIR), layout::RECOVERY_DIR)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for suffix in 0..1000 {
        let name = match format {
            IntentFormat::Binary => layout::interrupted_candidate(revision, timestamp, suffix),
            IntentFormat::Legacy => {
                layout::legacy_interrupted_candidate(revision, timestamp, suffix)
            }
        };
        let target = recovery.join(&name);
        if !target.exists() {
            fs::rename(&pending, target)?;
            return Ok(());
        }
    }
    invalid("too many recovery candidates; preserve or remove old candidates before reopening")
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCandidate {
    pub file_name: String,
    pub page_id: Option<String>,
    pub confirmed_revision: Option<u64>,
    pub candidate_revision: Option<u64>,
    pub error: Option<String>,
}

fn candidate_format(file_name: &str) -> Result<IntentFormat, StorageError> {
    if !layout::is_candidate_name(file_name) {
        return Err(StorageError::InvalidPath(file_name.into()));
    }
    if file_name.ends_with(".bin") {
        Ok(IntentFormat::Binary)
    } else if file_name.ends_with(".json") {
        Ok(IntentFormat::Legacy)
    } else {
        Err(StorageError::InvalidPath(file_name.into()))
    }
}

fn read_candidate(root: &Path, file_name: &str) -> Result<RecoveryIntent, StorageError> {
    let format = candidate_format(file_name)?;
    read_intent(root, &layout::recovery_path(file_name), format)
}

pub fn list_recovery_candidates(
    selected_root: &Path,
) -> Result<Vec<RecoveryCandidate>, StorageError> {
    let root = canonical_root(selected_root)?;
    Ok(candidate_names(&root)?
        .into_iter()
        .map(|file_name| match read_candidate(&root, &file_name) {
            Ok(intent) => RecoveryCandidate {
                file_name,
                page_id: Some(intent.candidate.page.id),
                confirmed_revision: Some(intent.previous.page.revision),
                candidate_revision: Some(intent.candidate.page.revision),
                error: None,
            },
            Err(error) => RecoveryCandidate {
                file_name,
                page_id: None,
                confirmed_revision: None,
                candidate_revision: None,
                error: Some(error.to_string()),
            },
        })
        .collect())
}

fn candidate_names(root: &Path) -> Result<Vec<String>, StorageError> {
    let recovery = root.join(layout::RECOVERY_DIR);
    if !recovery.exists() {
        return Ok(Vec::new());
    }
    let recovery = crate::paths::resolve_dir(root, &recovery, layout::RECOVERY_DIR)?;
    let mut names = Vec::new();
    for entry in fs::read_dir(recovery)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir()
            && let Some(name) = entry.file_name().to_str()
            && layout::is_candidate_name(name)
        {
            names.push(name.to_owned());
        }
    }
    names.sort_unstable();
    Ok(names)
}

pub fn restore_recovery_candidate(
    selected_root: &Path,
    history: &mut NotebookHistory,
    file_name: &str,
) -> Result<HistoryResult, StorageError> {
    let root = canonical_root(selected_root)?;
    let intent = read_candidate(&root, file_name)?;
    let mut candidate = intent.candidate;
    let current = observe_page(&root, &candidate.page.id, history)?;

    let current_reference = page_reference(&current.manifest, &candidate.page.id)?;
    let candidate_reference = page_reference(&candidate.manifest, &candidate.page.id)?;
    if current_reference.path != candidate_reference.path {
        return invalid("the recovery candidate no longer matches the notebook layout");
    }
    candidate.manifest = current.manifest.clone();
    candidate.page.revision = current
        .page
        .revision
        .checked_add(1)
        .ok_or_else(|| invalid_error("recovery restore revision overflow"))?;

    let (restored, fingerprint) = save_and_return(&root, &current, candidate)?;
    history.advance(current, &restored, fingerprint);
    discard_recovery_candidate(&root, file_name)?;
    Ok(history_result(restored, history))
}

pub fn discard_recovery_candidate(
    selected_root: &Path,
    file_name: &str,
) -> Result<(), StorageError> {
    let root = canonical_root(selected_root)?;
    candidate_format(file_name)?;
    // Unlink the candidate itself, including an unreadable/broken symlink, never its target.
    let recovery = crate::paths::resolve_dir(
        &root,
        &root.join(layout::RECOVERY_DIR),
        layout::RECOVERY_DIR,
    )?;
    fs::remove_file(recovery.join(file_name))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BINARY_INK_SCHEMA_VERSION, PageObject,
        storage::{fixtures::*, *},
    };

    fn changed(previous: &NotebookSnapshot) -> NotebookSnapshot {
        let mut next = previous.clone().without_assets();
        next.page.revision = previous.page.revision + 1;
        next.manifest.modified_at = "2026-09-10T12:00:00Z".into();
        next.blocks[0].bytes = b"$ interrupted = true $".to_vec();
        next.ink_layers[0].strokes[0].points[0].x = 160.0;
        let PageObject::Image { fields, .. } = &mut next.page.objects[1] else {
            panic!("image object missing");
        };
        fields.x = 144.0;
        next
    }

    #[test]
    fn binary_envelope_round_trips_and_rejects_bad_bounds() {
        let previous = snapshot().without_assets();
        let candidate = changed(&previous);
        let encoded = encode_intent(&previous, &candidate).unwrap();
        let decoded = decode_binary(&encoded).unwrap();
        assert_eq!(
            decoded.candidate.ink_layers[0].strokes[0].points[0].x,
            160.0
        );
        for end in [0, 7, 15, encoded.len() - 1] {
            assert!(decode_binary(&encoded[..end]).is_err());
        }
        let mut future = encoded.clone();
        future[8..12].copy_from_slice(&3_u32.to_le_bytes());
        assert!(decode_binary(&future).is_err());
        let mut trailing = encoded;
        trailing.push(0);
        assert!(decode_binary(&trailing).is_err());
    }

    #[test]
    fn recovers_binary_transactions_at_each_write_boundary() {
        for completed in 0..=4 {
            let temporary = tempfile::tempdir().unwrap();
            let root = temporary.path().join("notebook");
            create_notebook(&root, &snapshot()).unwrap();
            let canonical = canonical_root(&root).unwrap();
            let previous = open_notebook(&root).unwrap();
            let candidate = changed(&previous);
            stage_transaction(&canonical, &previous, &candidate).unwrap();
            if completed >= 1 {
                write_atomic(
                    &canonical,
                    &candidate.blocks[0].path,
                    &candidate.blocks[0].bytes,
                )
                .unwrap();
            }
            if completed >= 2 {
                write_ink(
                    &canonical,
                    &candidate.page.ink_layers[0].path,
                    &candidate.ink_layers[0],
                )
                .unwrap();
            }
            if completed >= 3 {
                write_json(
                    &canonical,
                    &candidate.manifest.pages[0].path,
                    &candidate.page,
                )
                .unwrap();
            }
            if completed >= 4 {
                write_json(&canonical, layout::MANIFEST, &candidate.manifest).unwrap();
            }
            let recovered = open_notebook(&root).unwrap();
            if completed < 3 {
                assert_eq!(recovered.page.revision, previous.page.revision);
                assert_eq!(recovered.ink_layers[0].strokes[0].points[0].x, 80.0);
                assert_eq!(
                    fs::read_dir(root.join(layout::RECOVERY_DIR))
                        .unwrap()
                        .count(),
                    1
                );
            } else {
                assert_eq!(recovered.page.revision, candidate.page.revision);
                assert_eq!(recovered.ink_layers[0].strokes[0].points[0].x, 160.0);
                assert!(!root.join(layout::RECOVERY_DIR).exists());
            }
        }
    }

    #[test]
    fn malformed_or_ambiguous_pending_state_changes_nothing() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let manifest = fs::read(root.join(layout::MANIFEST)).unwrap();
        fs::create_dir_all(root.join(layout::INTERNAL_DIR)).unwrap();
        fs::write(root.join(layout::PENDING_TRANSACTION), b"broken").unwrap();
        assert!(open_notebook(&root).is_err());
        assert_eq!(fs::read(root.join(layout::MANIFEST)).unwrap(), manifest);
        fs::write(root.join(layout::LEGACY_PENDING_TRANSACTION), b"{}").unwrap();
        assert!(
            open_notebook(&root)
                .unwrap_err()
                .to_string()
                .contains("both binary and legacy")
        );
        assert_eq!(fs::read(root.join(layout::MANIFEST)).unwrap(), manifest);
    }

    #[test]
    fn migration_rollback_preserves_unchanged_legacy_ink_bytes() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        let mut legacy = snapshot();
        let version = BINARY_INK_SCHEMA_VERSION - 1;
        legacy.manifest.schema_version = version;
        legacy.page.schema_version = version;
        legacy.ink_layers[0].schema_version = version;
        legacy.page.ink_layers[0].path = layout::legacy_ink_layer_path("page-001", 1);
        create_notebook(&root, &legacy).unwrap();
        let canonical = canonical_root(&root).unwrap();
        let ink_path = root.join(&legacy.page.ink_layers[0].path);
        let mut exact = serde_json::to_vec_pretty(&legacy.ink_layers[0]).unwrap();
        exact.extend_from_slice(b"\r\n");
        fs::write(&ink_path, &exact).unwrap();
        let previous = open_notebook(&root).unwrap();
        let mut candidate = changed(&previous);
        candidate.manifest.schema_version = BINARY_INK_SCHEMA_VERSION;
        candidate.page.schema_version = BINARY_INK_SCHEMA_VERSION;
        candidate.ink_layers[0].schema_version = BINARY_INK_SCHEMA_VERSION;
        candidate.page.ink_layers[0].path = layout::ink_layer_path("page-001", 1);
        stage_transaction(&canonical, &previous, &candidate).unwrap();
        write_ink_new(
            &canonical,
            &candidate.page.ink_layers[0].path,
            &candidate.ink_layers[0],
        )
        .unwrap();
        assert_eq!(
            open_notebook(&root).unwrap().page.revision,
            previous.page.revision
        );
        assert_eq!(fs::read(ink_path).unwrap(), exact);
        assert!(
            list_recovery_candidates(&root).unwrap()[0]
                .file_name
                .ends_with(".bin")
        );
        let orphan = root.join(&candidate.page.ink_layers[0].path);
        let orphan_bytes = fs::read(&orphan).unwrap();
        let archived = list_recovery_candidates(&root).unwrap().remove(0);
        discard_recovery_candidate(&root, &archived.file_name).unwrap();
        let mut history = NotebookHistory::default();
        let observed = observe_notebook(&root, &mut history).unwrap();
        let saved = commit_notebook(&root, &mut history, observed)
            .unwrap()
            .snapshot;
        assert_eq!(fs::read(orphan).unwrap(), orphan_bytes);
        assert_eq!(
            saved.page.ink_layers[0].path,
            "ink/page-001-layer-001-migrated-001.ink"
        );
    }

    #[test]
    fn legacy_same_path_ink_is_rolled_back_and_candidate_can_restore() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        let mut legacy = snapshot();
        let version = BINARY_INK_SCHEMA_VERSION - 1;
        legacy.manifest.schema_version = version;
        legacy.page.schema_version = version;
        legacy.ink_layers[0].schema_version = version;
        legacy.page.ink_layers[0].path = layout::legacy_ink_layer_path("page-001", 1);
        create_notebook(&root, &legacy).unwrap();
        let canonical = canonical_root(&root).unwrap();
        let previous = open_notebook(&root).unwrap();
        let candidate = changed(&previous);
        let intent = LegacyIntent {
            version: 1,
            previous: previous.clone().without_assets(),
            candidate: candidate.clone(),
        };
        fs::create_dir_all(root.join(layout::INTERNAL_DIR)).unwrap();
        fs::write(
            root.join(layout::LEGACY_PENDING_TRANSACTION),
            serde_json::to_vec(&intent).unwrap(),
        )
        .unwrap();
        write_ink(
            &canonical,
            &candidate.page.ink_layers[0].path,
            &candidate.ink_layers[0],
        )
        .unwrap();

        let rolled_back = open_notebook(&root).unwrap();
        assert_eq!(rolled_back.ink_layers[0].strokes[0].points[0].x, 80.0);
        let candidates = list_recovery_candidates(&root).unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].file_name.ends_with(".json"));
        let mut history = NotebookHistory::default();
        let restored =
            restore_recovery_candidate(&root, &mut history, &candidates[0].file_name).unwrap();
        assert_eq!(
            restored.snapshot.ink_layers[0].strokes[0].points[0].x,
            160.0
        );
        assert!(restored.snapshot.page.schema_version >= BINARY_INK_SCHEMA_VERSION);

        let previous = open_notebook(&root).unwrap();
        let intent = LegacyIntent {
            version: 1,
            previous: previous.clone().without_assets(),
            candidate: changed(&previous),
        };
        fs::write(
            root.join(layout::LEGACY_PENDING_TRANSACTION),
            serde_json::to_vec(&intent).unwrap(),
        )
        .unwrap();
        open_notebook(&root).unwrap();
        let candidates = list_recovery_candidates(&root).unwrap();
        discard_recovery_candidate(&root, &candidates[0].file_name).unwrap();
        assert!(list_recovery_candidates(&root).unwrap().is_empty());
    }

    #[test]
    fn binary_candidates_list_restore_and_discard() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let canonical = canonical_root(&root).unwrap();
        let previous = open_notebook(&root).unwrap();
        let mut candidate = changed(&previous);
        let mut extra = candidate.ink_layers[0].strokes[0].clone();
        extra.id = "stroke-recovered".into();
        candidate.ink_layers[0].strokes.push(extra);
        stage_transaction(&canonical, &previous, &candidate).unwrap();
        open_notebook(&root).unwrap();
        let candidates = list_recovery_candidates(&root).unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].file_name.ends_with(".bin"));
        let mut history = NotebookHistory::default();
        let restored =
            restore_recovery_candidate(&root, &mut history, &candidates[0].file_name).unwrap();
        assert_eq!(restored.snapshot.ink_layers[0].strokes.len(), 2);
        assert!(restored.can_undo);

        let previous = open_notebook(&root).unwrap();
        stage_transaction(&canonical, &previous, &changed(&previous)).unwrap();
        open_notebook(&root).unwrap();
        let candidates = list_recovery_candidates(&root).unwrap();
        discard_recovery_candidate(&root, &candidates[0].file_name).unwrap();
        assert!(list_recovery_candidates(&root).unwrap().is_empty());
    }

    #[test]
    fn full_recovery_directory_blocks_a_commit() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let recovery = root.join(layout::RECOVERY_DIR);
        fs::create_dir_all(&recovery).unwrap();
        for index in 0..RECOVERY_CANDIDATE_LIMIT {
            fs::write(
                recovery.join(layout::interrupted_candidate(2, index as u128, 0)),
                b"preserved",
            )
            .unwrap();
        }
        let mut history = NotebookHistory::default();
        let mut changed = observe_notebook(&root, &mut history).unwrap();
        let PageObject::Image { fields, .. } = &mut changed.page.objects[1] else {
            panic!("image object missing");
        };
        fields.x = 144.0;
        let candidates = list_recovery_candidates(&root).unwrap();
        assert_eq!(candidates.len(), RECOVERY_CANDIDATE_LIMIT);
        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.error.is_some() && candidate.page_id.is_none())
        );
        assert!(commit_notebook(&root, &mut history, changed.clone()).is_err());
        assert_eq!(open_notebook(&root).unwrap().page.revision, 1);
        discard_recovery_candidate(&root, &candidates[0].file_name).unwrap();
        assert_eq!(
            list_recovery_candidates(&root).unwrap().len(),
            RECOVERY_CANDIDATE_LIMIT - 1
        );
        fs::write(recovery.join("notes.txt"), b"keep").unwrap();
        commit_notebook(&root, &mut history, changed).unwrap();
        assert_eq!(fs::read(recovery.join("notes.txt")).unwrap(), b"keep");
    }
}
