//! The write path, and the fingerprint that decides whether a later write is still safe.
//!
//! [`FileSet`] is why this is one module. A page's canonical files have to be hashed the same way
//! whether the bytes came from disk or from the write that just happened, and the two answers
//! have to be equal — otherwise every later commit reports a change that never occurred.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use crate::{SourceRole, layout};

use super::{
    MAX_IMAGE_BYTES, MAX_INK_BYTES, MAX_JSON_BYTES, NotebookSnapshot, StorageError, StoredFile,
    files::*, invalid_error, paths::*, recovery::*, validate::*,
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
                    fingerprint_file(
                        relative,
                        read_limited(resolve_existing(root, relative)?, *maximum)?,
                    )?,
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
        let manifest = written
            .get(layout::MANIFEST)
            .cloned()
            .ok_or_else(|| invalid_error("the written manifest is missing"))?;
        let manifest = fingerprint_file(layout::MANIFEST, manifest)?;
        let entries = self
            .paths()
            .map(|path| {
                if path == layout::MANIFEST {
                    return Ok((path, manifest.as_slice()));
                }
                written
                    .get(path)
                    .map(|bytes| (path, bytes.as_slice()))
                    .ok_or_else(|| invalid_error("a fingerprinted file was not written"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(fingerprint_files(&entries))
    }
}

/// `modifiedAt` and the manifest schema maximum are Goodtype-owned notebook bookkeeping shared by
/// every page. Hashing either would make saving or migrating one page look like an outside edit on
/// every other loaded page. Page schemas and ink references remain fingerprinted in each page
/// file, while every other manifest field remains protected here.
fn fingerprint_file(relative: &str, bytes: Vec<u8>) -> Result<Vec<u8>, StorageError> {
    if relative != layout::MANIFEST {
        return Ok(bytes);
    }
    let manifest: crate::NotebookManifest = serde_json::from_slice(&bytes)?;
    manifest_fingerprint_bytes(&manifest)
}

pub(crate) fn manifest_fingerprint_bytes(
    manifest: &crate::NotebookManifest,
) -> Result<Vec<u8>, StorageError> {
    let mut manifest = manifest.clone();
    manifest.modified_at.clear();
    manifest.schema_version = 0;
    Ok(serde_json::to_vec(&manifest)?)
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
    mut candidate: NotebookSnapshot,
) -> Result<(NotebookSnapshot, u64), StorageError> {
    let missing_blocks = referenced_paths(&candidate.manifest, &candidate.page, SourceRole::Block)
        .into_iter()
        .filter(|path| !candidate.blocks.iter().any(|file| file.path == *path))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let existing = read_stored_files(
        root,
        &missing_blocks
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        SourceRole::Block.max_bytes(),
    )?;
    candidate.blocks.extend(existing);
    prepare_binary_ink_write(root, previous, &mut candidate)?;
    let fingerprint = save_transactional(root, previous, &candidate)?;
    let assets = read_stored_files(
        root,
        &referenced_paths(&candidate.manifest, &candidate.page, SourceRole::Asset),
        MAX_IMAGE_BYTES,
    )?;
    let saved = NotebookSnapshot {
        assets,
        ..candidate
    };
    Ok((saved, fingerprint))
}

/// Upgrade only the page bundle being confirmed. Existing binary references are retained; a
/// legacy reference receives a fresh sibling path and its JSON original is deliberately left
/// untouched. An occupied target is preserved and a bounded unused sibling is selected.
fn prepare_binary_ink_write(
    root: &Path,
    previous: &NotebookSnapshot,
    candidate: &mut NotebookSnapshot,
) -> Result<(), StorageError> {
    for reference in &mut candidate.page.ink_layers {
        let previous_reference = previous
            .page
            .ink_layers
            .iter()
            .find(|current| current.id == reference.id);
        if let Some(previous_reference) = previous_reference
            && previous_reference.path.ends_with(".ink")
        {
            reference.path = previous_reference.path.clone();
            continue;
        }

        let proposed = previous_reference
            .map(|current| current.path.as_str())
            .unwrap_or(reference.path.as_str());
        let mut binary = if let Some(stem) = proposed.strip_suffix(".json") {
            format!("{stem}.ink")
        } else if previous_reference.is_none() && proposed.ends_with(".ink") {
            proposed.to_owned()
        } else {
            return Err(invalid_error(
                "ink layer path must migrate from .json or reuse its current .ink path",
            ));
        };
        if root.join(&binary).exists() {
            let stem = binary.strip_suffix(".ink").expect("binary path suffix");
            binary = (1..=1000)
                .map(|suffix| format!("{stem}-migrated-{suffix:03}.ink"))
                .find(|path| !root.join(path).exists())
                .ok_or_else(|| invalid_error("no unused migration ink path remains"))?;
        }
        reference.path = binary;
    }

    candidate.manifest.schema_version = candidate
        .manifest
        .schema_version
        .max(previous.manifest.schema_version)
        .max(crate::BINARY_INK_SCHEMA_VERSION);
    candidate.page.schema_version = candidate
        .page
        .schema_version
        .max(crate::BINARY_INK_SCHEMA_VERSION);
    for layer in &mut candidate.ink_layers {
        layer.schema_version = candidate.page.schema_version;
    }
    Ok(())
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
    let previous_paths = previous
        .page
        .ink_layers
        .iter()
        .map(|reference| reference.path.as_str())
        .collect::<HashSet<_>>();
    let new_ink_paths = candidate
        .page
        .ink_layers
        .iter()
        .filter(|reference| !previous_paths.contains(reference.path.as_str()))
        .map(|reference| reference.path.clone())
        .collect::<HashSet<_>>();
    let fingerprint = save_to_root_inner(root, candidate, None, Some(&new_ink_paths))?;
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

pub fn store_pdf_reference(
    selected_root: &Path,
    filename: &str,
    bytes: &[u8],
) -> Result<String, StorageError> {
    if bytes.is_empty() || bytes.len() > crate::object::MAX_PDF_BYTES {
        return Err(StorageError::InvalidNotebook(format!(
            "PDF is {} bytes; maximum is {}",
            bytes.len(),
            crate::object::MAX_PDF_BYTES
        )));
    }
    if !bytes[..bytes.len().min(1024)]
        .windows(5)
        .any(|window| window == b"%PDF-")
    {
        return Err(StorageError::InvalidNotebook(
            "selected file is not a PDF".into(),
        ));
    }
    validate_safe_filename(filename)?;
    if !filename.to_ascii_lowercase().ends_with(".pdf") {
        return Err(StorageError::InvalidPath(filename.into()));
    }

    let root = canonical_root(selected_root)?;
    let relative = layout::reference_path(filename);
    write_once(&root, &relative, bytes)?;
    Ok(relative)
}

pub fn read_pdf_reference(selected_root: &Path, relative: &str) -> Result<Vec<u8>, StorageError> {
    if !relative.to_ascii_lowercase().ends_with(".pdf") {
        return Err(StorageError::InvalidPath(relative.into()));
    }
    let root = canonical_root(selected_root)?;
    validate_stored_path(relative, SourceRole::Reference)?;
    read_limited(
        resolve_existing(&root, relative)?,
        crate::object::MAX_PDF_BYTES,
    )
}

const MAX_TYPST_STYLE_BYTES: usize = 1024 * 1024;

/// Update an observed template; the caller serializes notebook writes as for page commits.
pub fn update_typst_style(
    selected_root: &Path,
    filename: &str,
    source: &str,
    expected_source: &str,
) -> Result<String, StorageError> {
    validate_safe_filename(filename)?;
    let root = canonical_root(selected_root)?;
    let relative = format!("styles/{filename}");
    let current = read_limited(resolve_existing(&root, &relative)?, MAX_TYPST_STYLE_BYTES)?;
    if current != expected_source.as_bytes() {
        return Err(StorageError::ExternalChange(
            "the template was not overwritten. Reopen it after preserving your unsaved text".into(),
        ));
    }
    write_typst_style(&root, filename, source)
}

/// Write one ordinary UTF-8 Typst source below `styles/`. The caller owns language-level
/// validation; storage owns the filename, containment, symlink, size, and atomic-write rules.
pub fn write_typst_style(
    selected_root: &Path,
    filename: &str,
    source: &str,
) -> Result<String, StorageError> {
    persist_typst_style(selected_root, filename, source, true)
}

/// Install a preset without replacing an edited file. Identical bytes may be reused.
pub fn install_typst_style(
    selected_root: &Path,
    filename: &str,
    source: &str,
) -> Result<String, StorageError> {
    persist_typst_style(selected_root, filename, source, false)
}

fn persist_typst_style(
    selected_root: &Path,
    filename: &str,
    source: &str,
    replace: bool,
) -> Result<String, StorageError> {
    validate_safe_filename(filename)?;
    if !filename.to_ascii_lowercase().ends_with(".typ") {
        return Err(StorageError::InvalidPath(filename.into()));
    }
    if source.is_empty() || source.len() > MAX_TYPST_STYLE_BYTES {
        return Err(StorageError::InvalidNotebook(format!(
            "Typst style is {} bytes; maximum is {MAX_TYPST_STYLE_BYTES}",
            source.len()
        )));
    }
    let root = canonical_root(selected_root)?;
    let relative = format!("styles/{filename}");
    if replace {
        write_atomic(&root, &relative, source.as_bytes())?;
    } else {
        write_once(&root, &relative, source.as_bytes())?;
    }
    Ok(relative)
}

pub fn list_typst_styles(selected_root: &Path) -> Result<Vec<StoredFile>, StorageError> {
    let root = canonical_root(selected_root)?;
    let directory = root.join("styles");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut files = fs::read_dir(&directory)?
        .map(|entry| {
            let entry = entry?;
            let filename = entry.file_name().to_string_lossy().into_owned();
            validate_safe_filename(&filename)?;
            if !filename.to_ascii_lowercase().ends_with(".typ") || !entry.file_type()?.is_file() {
                return Err(StorageError::InvalidPath(filename));
            }
            let relative = format!("styles/{filename}");
            Ok(StoredFile {
                path: relative.clone(),
                bytes: read_limited(resolve_existing(&root, &relative)?, MAX_TYPST_STYLE_BYTES)?,
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    files.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

/// Exact cleanup for a failed notebook-creation transaction. Never accepts a general path.
pub fn remove_typst_style(selected_root: &Path, filename: &str) -> Result<(), StorageError> {
    validate_safe_filename(filename)?;
    if !filename.to_ascii_lowercase().ends_with(".typ") {
        return Err(StorageError::InvalidPath(filename.into()));
    }
    let root = canonical_root(selected_root)?;
    let relative = format!("styles/{filename}");
    match fs::remove_file(resolve_existing(&root, &relative)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Persists every file in the snapshot and returns the canonical fingerprint of the files
/// it wrote. Computing the fingerprint here, from the bytes just written, saves reading every
/// canonical file back a second time on the commit and undo/redo paths.
pub(crate) fn save_to_root(root: &Path, snapshot: &NotebookSnapshot) -> Result<u64, StorageError> {
    save_to_root_inner(root, snapshot, None, None)
}

/// Roll back a transaction whose page file never crossed the commit boundary. A legacy migration
/// writes a new `.ink` sibling and never touches the referenced `.json`; keeping those exact bytes
/// avoids changing an original merely because recovery rewrote the other mutable files.
pub(crate) fn restore_previous_to_root(
    root: &Path,
    previous: &NotebookSnapshot,
    candidate: &NotebookSnapshot,
) -> Result<u64, StorageError> {
    let untouched_legacy = previous
        .page
        .ink_layers
        .iter()
        .filter(|reference| {
            reference.path.ends_with(".json")
                && candidate
                    .page
                    .ink_layers
                    .iter()
                    .find(|candidate| candidate.id == reference.id)
                    .is_some_and(|candidate| candidate.path != reference.path)
        })
        .map(|reference| reference.path.clone())
        .collect::<HashSet<_>>();
    save_to_root_inner(root, previous, Some(&untouched_legacy), None)
}

fn save_to_root_inner(
    root: &Path,
    snapshot: &NotebookSnapshot,
    preserved_ink: Option<&HashSet<String>>,
    protected_new_ink: Option<&HashSet<String>>,
) -> Result<u64, StorageError> {
    validate_snapshot(root, snapshot)?;
    let page_path = page_reference(&snapshot.manifest, &snapshot.page.id)?
        .path
        .clone();

    // Retain the exact bytes of each canonical (non-asset) file so the fingerprint matches what
    // reading the file back would produce.
    let mut written: HashMap<String, Vec<u8>> = HashMap::new();

    // Existing sources may be omitted from a batch (notably the shared style on a blank page).
    // Read them before replacing anything so fingerprinting cannot fail after the save lands.
    for path in referenced_paths(&snapshot.manifest, &snapshot.page, SourceRole::Block) {
        if !snapshot.blocks.iter().any(|file| file.path == path) {
            written.insert(
                path.to_owned(),
                read_limited(resolve_existing(root, path)?, SourceRole::Block.max_bytes())?,
            );
        }
    }

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
        let bytes = if preserved_ink.is_some_and(|paths| paths.contains(&reference.path))
            && root.join(&reference.path).is_file()
        {
            read_limited(resolve_existing(root, &reference.path)?, MAX_INK_BYTES)?
        } else if protected_new_ink.is_some_and(|paths| paths.contains(&reference.path)) {
            write_ink_new(root, &reference.path, layer)?
        } else {
            write_ink(root, &reference.path, layer)?
        };
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
    fn preset_installation_reuses_identical_bytes_but_never_replaces_edits() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        install_typst_style(root, "clean-notes.typ", "original").unwrap();
        install_typst_style(root, "clean-notes.typ", "original").unwrap();
        update_typst_style(root, "clean-notes.typ", "edited", "original").unwrap();
        assert!(matches!(
            install_typst_style(root, "clean-notes.typ", "original"),
            Err(StorageError::AlreadyExists(_))
        ));
        assert_eq!(
            fs::read_to_string(root.join("styles/clean-notes.typ")).unwrap(),
            "edited"
        );
    }

    #[test]
    fn edited_templates_refuse_stale_sources_and_keep_the_confirmed_file() {
        let temporary = tempfile::tempdir().unwrap();
        write_typst_style(temporary.path(), "a.typ", "original").unwrap();
        update_typst_style(temporary.path(), "a.typ", "first", "original").unwrap();
        assert!(update_typst_style(temporary.path(), "a.typ", "stale", "original").is_err());
        assert_eq!(
            fs::read_to_string(temporary.path().join("styles/a.typ")).unwrap(),
            "first"
        );
        fs::write(temporary.path().join("styles/a.typ"), "external").unwrap();
        assert!(update_typst_style(temporary.path(), "a.typ", "second", "first").is_err());
        assert_eq!(
            fs::read_to_string(temporary.path().join("styles/a.typ")).unwrap(),
            "external"
        );
        assert!(update_typst_style(temporary.path(), "a.typ", "", "external").is_err());
        assert_eq!(
            fs::read_to_string(temporary.path().join("styles/a.typ")).unwrap(),
            "external"
        );
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
    fn confirmed_write_migrates_ink_without_touching_the_legacy_original() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &legacy_snapshot()).unwrap();
        let legacy_path = root.join("ink/page-001-layer-001.json");
        let layer = open_notebook(&root).unwrap().ink_layers.remove(0);
        let mut original = serde_json::to_vec_pretty(&layer).unwrap();
        original.extend_from_slice(b"\n");
        fs::write(&legacy_path, &original).unwrap();

        let mut history = NotebookHistory::default();
        let candidate = observe_notebook(&root, &mut history).unwrap();
        let saved = commit_notebook(&root, &mut history, candidate)
            .unwrap()
            .snapshot;

        assert_eq!(fs::read(&legacy_path).unwrap(), original);
        assert_eq!(saved.page.ink_layers[0].path, "ink/page-001-layer-001.ink");
        assert!(
            fs::read(root.join(&saved.page.ink_layers[0].path))
                .unwrap()
                .starts_with(b"GTINK\0\r\n")
        );
    }

    #[test]
    fn first_migration_preserves_an_occupied_target_and_uses_a_sibling() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &legacy_snapshot()).unwrap();
        let occupied = root.join("ink/page-001-layer-001.ink");
        fs::write(&occupied, b"unrelated").unwrap();

        let mut history = NotebookHistory::default();
        let candidate = observe_notebook(&root, &mut history).unwrap();
        let saved = commit_notebook(&root, &mut history, candidate)
            .unwrap()
            .snapshot;

        assert_eq!(fs::read(occupied).unwrap(), b"unrelated");
        assert_eq!(saved.page.revision, 2);
        assert_eq!(
            saved.page.ink_layers[0].path,
            "ink/page-001-layer-001-migrated-001.ink"
        );
        assert!(root.join(&saved.page.ink_layers[0].path).is_file());
    }
}
