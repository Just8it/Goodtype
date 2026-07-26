//! Surviving an interruption in the middle of a write.
//!
//! Before the canonical files are touched, both the previous and the intended state go into a
//! pending-transaction file. Reopening finds it and decides which way to go: forward if the page
//! file already carries the new revision, back otherwise. A rolled-back candidate is archived
//! rather than discarded, so the work can still be recovered deliberately.

use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{Page, layout};

use super::{
    MAX_RECOVERY_BYTES, NotebookSnapshot, RECOVERY_CANDIDATE_LIMIT, StorageError, files::*,
    history::*, invalid, paths::*, validate::*, write::*,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecoveryIntent {
    version: u32,
    previous: NotebookSnapshot,
    candidate: NotebookSnapshot,
}

/// Record what a write is about to do, before it touches a canonical file.
///
/// The shape of that record is this module's business alone — the write path says which two
/// states are involved and nothing more, so the on-disk recovery format can change without the
/// write path knowing.
///
// ponytail: reuse canonical snapshots until measured recovery size requires file staging.
pub(crate) fn stage_transaction(
    root: &Path,
    previous: &NotebookSnapshot,
    candidate: &NotebookSnapshot,
) -> Result<(), StorageError> {
    write_recovery_intent(
        root,
        &RecoveryIntent {
            version: 1,
            previous: previous.clone().without_assets(),
            candidate: candidate.clone().without_assets(),
        },
    )
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
    write_atomic(root, layout::PENDING_TRANSACTION, &bytes)
}

pub(crate) fn ensure_recovery_capacity(root: &Path) -> Result<(), StorageError> {
    let recovery = root.join(layout::RECOVERY_DIR);
    if !recovery.exists() {
        return Ok(());
    }
    let recovery = fs::canonicalize(recovery)?;
    if !recovery.starts_with(root) || !recovery.is_dir() {
        return Err(StorageError::InvalidPath(layout::RECOVERY_DIR.into()));
    }
    if fs::read_dir(recovery)?.count() >= RECOVERY_CANDIDATE_LIMIT {
        return Err(StorageError::InvalidNotebook(format!(
            "recovery contains {RECOVERY_CANDIDATE_LIMIT} unresolved candidates"
        )));
    }
    Ok(())
}

pub(crate) fn recover_interrupted_transaction(root: &Path) -> Result<(), StorageError> {
    if !root.join(layout::PENDING_TRANSACTION).exists() {
        return Ok(());
    }
    let bytes = read_limited(
        resolve_existing(root, layout::PENDING_TRANSACTION)?,
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

pub(crate) fn remove_pending_transaction(root: &Path) -> Result<(), StorageError> {
    let pending = resolve_existing(root, layout::PENDING_TRANSACTION)?;
    fs::remove_file(pending)?;
    Ok(())
}

fn archive_pending_transaction(root: &Path, revision: u64) -> Result<(), StorageError> {
    let pending = resolve_existing(root, layout::PENDING_TRANSACTION)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for suffix in 0..1000 {
        let relative =
            layout::recovery_path(&layout::interrupted_candidate(revision, timestamp, suffix));
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
    if layout::is_candidate_name(file_name) {
        Ok(())
    } else {
        Err(StorageError::InvalidPath(file_name.into()))
    }
}

fn read_candidate_intent(root: &Path, file_name: &str) -> Result<RecoveryIntent, StorageError> {
    validate_candidate_file_name(file_name)?;
    let bytes = read_limited(
        resolve_existing(root, &layout::recovery_path(file_name))?,
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
    let recovery = root.join(layout::RECOVERY_DIR);
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
    history.advance(current, &restored, fingerprint);
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
    let path = resolve_existing(&root, &layout::recovery_path(file_name))?;
    fs::remove_file(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PageObject, layout,
        storage::{fixtures::*, *},
    };
    use std::fs;

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
                previous: previous.without_assets(),
                candidate: candidate.clone().without_assets(),
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
                write_json(&root, layout::MANIFEST, &candidate.manifest).unwrap();
            }

            let recovered = open_notebook(&root).unwrap();
            assert!(!root.join(layout::PENDING_TRANSACTION).exists());
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
    fn lists_restores_and_discards_recovery_candidates() {
        let temporary = tempfile::tempdir().unwrap();
        let notebook_root = temporary.path().join("notebook");
        create_notebook(&notebook_root, &snapshot()).unwrap();
        let root = fs::canonicalize(&notebook_root).unwrap();

        // Simulate an interrupted transaction whose candidate was never written: reopen rolls
        // back to the confirmed state and archives the candidate.
        let previous = open_notebook(&notebook_root).unwrap();
        let mut candidate = previous.clone().without_assets();
        candidate.page.revision = 2;
        let mut extra = candidate.ink_layers[0].strokes[0].clone();
        extra.id = "stroke-recovered".into();
        candidate.ink_layers[0].strokes.push(extra);
        write_recovery_intent(
            &root,
            &RecoveryIntent {
                version: 1,
                previous: previous.clone().without_assets(),
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
        let mut candidate = previous.clone().without_assets();
        candidate.page.revision = 3;
        write_recovery_intent(
            &root,
            &RecoveryIntent {
                version: 1,
                previous: previous.without_assets(),
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
}
