//! The write path, and the fingerprint that decides whether a later write is still safe.
//!
//! [`FileSet`] is why this is one module. A page's canonical files have to be hashed the same way
//! whether the bytes came from disk or from the write that just happened, and the two answers
//! have to be equal — otherwise every later commit reports a change that never occurred.

use std::{collections::HashMap, path::Path};

use crate::{SourceRole, layout};

use super::{
    MAX_IMAGE_BYTES, MAX_INK_BYTES, MAX_JSON_BYTES, NotebookSnapshot, StorageError, files::*,
    invalid_error, paths::*, recovery::*, validate::*,
};

/// The canonical files of one page: everything the store rewrites, and therefore everything a
/// change fingerprint has to cover.
///
/// There is one definition of this set because there are two ways to reach a fingerprint — hash
/// the bytes read back from disk, or hash the bytes just written — and they have to agree
/// exactly. When they were derived separately, a file added to one and not the other would make
/// every later commit report an external change and refuse to save.
struct FileSet<'a> {
    files: Vec<(&'a str, usize)>,
}

impl<'a> FileSet<'a> {
    fn of(snapshot: &'a NotebookSnapshot) -> Result<Self, StorageError> {
        let reference = page_reference(&snapshot.manifest, &snapshot.page.id)?;
        let mut files = vec![
            (layout::MANIFEST, MAX_JSON_BYTES),
            (reference.path.as_str(), MAX_JSON_BYTES),
        ];
        files.extend(
            referenced_paths(&snapshot.manifest, &snapshot.page, SourceRole::Block)
                .into_iter()
                .map(|path| (path, SourceRole::Block.max_bytes())),
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
        Ok(Self { files })
    }

    fn paths(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.files.iter().map(|(path, _)| *path)
    }

    /// Hash the set as it currently sits on disk.
    fn fingerprint_on_disk(&self, root: &Path) -> Result<u64, StorageError> {
        let bytes = self
            .files
            .iter()
            .map(|(relative, maximum)| {
                Ok((
                    *relative,
                    read_limited(resolve_existing(root, relative)?, *maximum)?,
                ))
            })
            .collect::<Result<Vec<(&str, Vec<u8>)>, StorageError>>()?;
        Ok(fingerprint_files(
            &bytes
                .iter()
                .map(|(path, value)| (*path, value.as_slice()))
                .collect::<Vec<_>>(),
        ))
    }

    /// Hash the set from bytes just persisted, saving a full read-back per commit. Every path in
    /// the set must be present in `written`, which is what makes this equal to
    /// [`Self::fingerprint_on_disk`] rather than merely intended to be.
    fn fingerprint_written(&self, written: &HashMap<String, Vec<u8>>) -> Result<u64, StorageError> {
        let entries = self
            .paths()
            .map(|path| {
                written
                    .get(path)
                    .map(|bytes| (path, bytes.as_slice()))
                    .ok_or_else(|| invalid_error("a fingerprinted file was not written"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(fingerprint_files(&entries))
    }
}

pub(crate) fn canonical_fingerprint(
    root: &Path,
    snapshot: &NotebookSnapshot,
) -> Result<u64, StorageError> {
    FileSet::of(snapshot)?.fingerprint_on_disk(root)
}

/// Writes `candidate` transactionally and returns it as the frontend will see it — the written
/// canonical state plus its originals — together with the fingerprint of what was written.
/// The canonical files equal `candidate` after the write, so only assets are read back rather
/// than re-parsing and re-hashing the whole page.
pub(crate) fn save_and_return(
    root: &Path,
    previous: &NotebookSnapshot,
    candidate: &NotebookSnapshot,
) -> Result<(NotebookSnapshot, u64), StorageError> {
    let fingerprint = save_transactional(root, previous, candidate)?;
    let assets = read_stored_files(
        root,
        &referenced_paths(&candidate.manifest, &candidate.page, SourceRole::Asset),
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

    stage_transaction(root, previous, candidate)?;
    let fingerprint = save_to_root(root, candidate)?;
    remove_pending_transaction(root)?;
    Ok(fingerprint)
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
    let relative = layout::asset_path(filename);
    write_once(&root, &relative, bytes)?;
    Ok(relative)
}

/// Persists every file in the snapshot and returns the canonical fingerprint of the files
/// it wrote. Computing the fingerprint here, from the bytes just written, saves reading every
/// canonical file back a second time on the commit and undo/redo paths.
pub(crate) fn save_to_root(root: &Path, snapshot: &NotebookSnapshot) -> Result<u64, StorageError> {
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
        layout::MANIFEST.to_owned(),
        write_json(root, layout::MANIFEST, &snapshot.manifest)?,
    );

    FileSet::of(snapshot)?.fingerprint_written(&written)
}

#[cfg(test)]
mod tests {
    use crate::storage::{fixtures::*, *};
    use std::fs;

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
}
