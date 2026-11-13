//! The write path, and the fingerprint that decides whether a later write is still safe.
//!
//! [`FileSet`] is why this is one module. A page's canonical files have to be hashed the same way
//! whether the bytes came from disk or from the write that just happened, and the two answers
//! have to be equal — otherwise every later commit reports a change that never occurred.

use std::{
    borrow::Cow,
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
            referenced_paths(&snapshot.manifest, &snapshot.page, SourceRole::Calculation)
                .into_iter()
                .map(|path| (path, SourceRole::Calculation.max_bytes())),
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
    fn fingerprint_written(&self, written: &HashMap<&str, Cow<[u8]>>) -> Result<u64, StorageError> {
        let manifest = written
            .get(layout::MANIFEST)
            .map(|bytes| bytes.to_vec())
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
                    .map(|bytes| (path, bytes.as_ref()))
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
///
/// Everything fallible happens before the transaction commits: the originals returned are read
/// and checked first, so once the new revision is durable the command can only succeed. A read
/// after the commit could fail while the revision is already on disk, leaving history a step
/// behind and turning the retry into a conflict.
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
    let missing = referenced_paths(
        &candidate.manifest,
        &candidate.page,
        SourceRole::Calculation,
    )
    .into_iter()
    .filter(|path| !candidate.calculations.iter().any(|file| file.path == *path))
    .map(str::to_owned)
    .collect::<Vec<_>>();
    candidate.calculations.extend(read_stored_files(
        root,
        &missing.iter().map(String::as_str).collect::<Vec<_>>(),
        SourceRole::Calculation.max_bytes(),
    )?);
    prepare_binary_ink_write(root, previous, &mut candidate)?;
    validate_snapshot(root, &candidate)?;
    // Originals the batch does not carry are already on disk and write-once; read them now.
    // Carried ones are written by `write_once`, which refuses anything but these exact bytes.
    let referenced = referenced_paths(&candidate.manifest, &candidate.page, SourceRole::Asset)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut loaded = read_stored_files(
        root,
        &referenced
            .iter()
            .map(String::as_str)
            .filter(|path| !candidate.assets.iter().any(|file| file.path == *path))
            .collect::<Vec<_>>(),
        MAX_IMAGE_BYTES,
    )?
    .into_iter()
    .map(|file| (file.path, file.bytes))
    .collect::<HashMap<_, _>>();

    let fingerprint = commit_transaction(root, previous, &candidate)?;

    let mut carried = std::mem::take(&mut candidate.assets)
        .into_iter()
        .map(|file| (file.path, file.bytes))
        .collect::<HashMap<_, _>>();
    let assets = referenced
        .into_iter()
        .filter_map(|path| {
            let bytes = carried.remove(&path).or_else(|| loaded.remove(&path))?;
            Some(StoredFile { path, bytes })
        })
        .collect();
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

pub(super) fn save_transactional(
    root: &Path,
    previous: &NotebookSnapshot,
    candidate: &NotebookSnapshot,
) -> Result<u64, StorageError> {
    validate_snapshot(root, candidate)?;
    commit_transaction(root, previous, candidate)
}

/// The transaction proper. `candidate` has passed [`validate_snapshot`] in this call chain and is
/// borrowed immutably from there to here, so its content checks are not repeated; the
/// filesystem checks run again at the point of writing.
fn commit_transaction(
    root: &Path,
    previous: &NotebookSnapshot,
    candidate: &NotebookSnapshot,
) -> Result<u64, StorageError> {
    ensure_recovery_capacity(root)?;
    for asset in &candidate.assets {
        write_once(root, &asset.path, &asset.bytes)?;
    }

    let staged = stage_transaction(root, previous, candidate)?;
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
    let unchanged = unchanged_paths(previous, candidate, &staged);
    // The previous state's encodings were needed only for that comparison.
    let StagedInk {
        previous: prior,
        candidate: encoded_ink,
    } = staged;
    drop(prior);
    let fingerprint = save_to_root_inner(
        root,
        candidate,
        WriteOptions {
            content_validated: true,
            protected_new_ink: Some(&new_ink_paths),
            encoded_ink: Some(&encoded_ink),
            unchanged: Some(&unchanged),
            ..WriteOptions::default()
        },
    )?;
    remove_pending_transaction(root)?;
    Ok(fingerprint)
}

fn encoded_layer<'a>(
    layers: &[crate::InkLayer],
    bytes: &'a [Vec<u8>],
    id: &str,
) -> Option<&'a [u8]> {
    layers
        .iter()
        .position(|layer| layer.id == id)
        .and_then(|index| bytes.get(index))
        .map(Vec::as_slice)
}

/// Mutable files whose candidate bytes equal the previous state's at the same path. Only a
/// candidate for skipping: the write is skipped when the file on disk also holds exactly those
/// bytes at the moment of writing, so an external change made after loading is still replaced
/// as before.
fn unchanged_paths(
    previous: &NotebookSnapshot,
    candidate: &NotebookSnapshot,
    staged: &StagedInk,
) -> HashSet<String> {
    let same_file = |earlier: &[StoredFile], file: &StoredFile| {
        earlier
            .iter()
            .any(|old| old.path == file.path && old.bytes == file.bytes)
    };
    let mut unchanged = candidate
        .blocks
        .iter()
        .filter(|file| same_file(&previous.blocks, file))
        .chain(
            candidate
                .calculations
                .iter()
                .filter(|file| same_file(&previous.calculations, file)),
        )
        .map(|file| file.path.clone())
        .collect::<HashSet<_>>();
    for reference in &candidate.page.ink_layers {
        let same_reference = previous
            .page
            .ink_layers
            .iter()
            .any(|old| old.id == reference.id && old.path == reference.path);
        let before = encoded_layer(&previous.ink_layers, &staged.previous, &reference.id);
        let after = encoded_layer(&candidate.ink_layers, &staged.candidate, &reference.id);
        if same_reference && reference.path.ends_with(".ink") && before.is_some() && before == after
        {
            unchanged.insert(reference.path.clone());
        }
    }
    unchanged
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

/// One `.typ` entry in `styles/`: its notebook-relative path and either its bytes or why they
/// could not be read. Non-Typst files, directories, symlinks and unsafe names are never entries.
#[derive(Debug)]
pub struct TypstStyleEntry {
    pub path: String,
    pub bytes: Result<Vec<u8>, StorageError>,
}

/// Every Typst style entry, sorted by path. One unreadable or oversized file is reported on its
/// own entry instead of failing the whole listing, and unrelated files are skipped.
pub fn list_typst_style_entries(
    selected_root: &Path,
) -> Result<Vec<TypstStyleEntry>, StorageError> {
    let root = canonical_root(selected_root)?;
    let directory = root.join("styles");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(&directory)? {
        let Ok(entry) = entry else { continue };
        let filename = entry.file_name().to_string_lossy().into_owned();
        // `file_type` does not follow links, so a symlink is never an ordinary file here.
        let is_file = entry.file_type().is_ok_and(|kind| kind.is_file());
        if !is_file
            || !filename.to_ascii_lowercase().ends_with(".typ")
            || validate_safe_filename(&filename).is_err()
        {
            continue;
        }
        let relative = format!("styles/{filename}");
        let bytes = resolve_existing(&root, &relative)
            .and_then(|path| read_limited(path, MAX_TYPST_STYLE_BYTES));
        entries.push(TypstStyleEntry {
            path: relative,
            bytes,
        });
    }
    entries.sort_unstable_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

/// The readable Typst styles. Unrelated and unreadable entries are left out; callers that must
/// show per-file problems use [`list_typst_style_entries`].
pub fn list_typst_styles(selected_root: &Path) -> Result<Vec<StoredFile>, StorageError> {
    Ok(list_typst_style_entries(selected_root)?
        .into_iter()
        .filter_map(|entry| {
            entry.bytes.ok().map(|bytes| StoredFile {
                path: entry.path,
                bytes,
            })
        })
        .collect())
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
    save_to_root_inner(root, snapshot, WriteOptions::default())
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
    save_to_root_inner(
        root,
        previous,
        WriteOptions {
            preserved_ink: Some(&untouched_legacy),
            ..WriteOptions::default()
        },
    )
}

/// How [`save_to_root_inner`] writes one snapshot. The default is a plain full write with full
/// validation, which is what every non-transactional caller gets.
#[derive(Default)]
struct WriteOptions<'a> {
    /// The snapshot passed [`validate_snapshot_content`] earlier in this transaction and has been
    /// borrowed immutably since. Filesystem checks run regardless.
    content_validated: bool,
    /// Legacy ink originals kept byte-for-byte instead of being rewritten.
    preserved_ink: Option<&'a HashSet<String>>,
    /// Ink paths new to this transaction, created without replacing anything already there.
    protected_new_ink: Option<&'a HashSet<String>>,
    /// Codec bytes for each `snapshot.ink_layers` entry, encoded once for the recovery intent.
    encoded_ink: Option<&'a [Vec<u8>]>,
    /// Mutable files equal to the previous state; see [`unchanged_paths`].
    unchanged: Option<&'a HashSet<String>>,
}

/// Whether `relative` already holds exactly `bytes`. Any read failure answers no, so the caller
/// falls back to the ordinary atomic write.
fn already_on_disk(root: &Path, relative: &str, bytes: &[u8], maximum: usize) -> bool {
    resolve_existing(root, relative)
        .and_then(|path| read_limited(path, maximum))
        .is_ok_and(|current| current == bytes)
}

fn save_to_root_inner<'a>(
    root: &Path,
    snapshot: &'a NotebookSnapshot,
    options: WriteOptions<'a>,
) -> Result<u64, StorageError> {
    if !options.content_validated {
        validate_snapshot_content(snapshot)?;
    }
    validate_snapshot_files(root, snapshot)?;
    let page_path = page_reference(&snapshot.manifest, &snapshot.page.id)?
        .path
        .as_str();
    let unchanged = |path: &str| options.unchanged.is_some_and(|paths| paths.contains(path));

    // Retain the exact bytes of each canonical (non-asset) file so the fingerprint matches what
    // reading the file back would produce.
    let mut written: HashMap<&str, Cow<[u8]>> = HashMap::new();

    // Existing sources may be omitted from a batch (notably the shared style on a blank page).
    // Read them before replacing anything so fingerprinting cannot fail after the save lands.
    for path in referenced_paths(&snapshot.manifest, &snapshot.page, SourceRole::Block) {
        if !snapshot.blocks.iter().any(|file| file.path == path) {
            written.insert(
                path,
                Cow::Owned(read_limited(
                    resolve_existing(root, path)?,
                    SourceRole::Block.max_bytes(),
                )?),
            );
        }
    }

    for path in referenced_paths(&snapshot.manifest, &snapshot.page, SourceRole::Calculation) {
        if !snapshot.calculations.iter().any(|file| file.path == path) {
            written.insert(
                path,
                Cow::Owned(read_limited(
                    resolve_existing(root, path)?,
                    SourceRole::Calculation.max_bytes(),
                )?),
            );
        }
    }
    for (file, maximum) in snapshot
        .blocks
        .iter()
        .map(|file| (file, SourceRole::Block.max_bytes()))
        .chain(
            snapshot
                .calculations
                .iter()
                .map(|file| (file, SourceRole::Calculation.max_bytes())),
        )
    {
        if !(unchanged(&file.path) && already_on_disk(root, &file.path, &file.bytes, maximum)) {
            write_atomic(root, &file.path, &file.bytes)?;
        }
        written.insert(&file.path, Cow::Borrowed(&file.bytes));
    }
    for file in &snapshot.assets {
        write_once(root, &file.path, &file.bytes)?;
    }
    for reference in &snapshot.page.ink_layers {
        let index = snapshot
            .ink_layers
            .iter()
            .position(|layer| layer.id == reference.id)
            .expect("validated ink reference");
        let bytes = if options
            .preserved_ink
            .is_some_and(|paths| paths.contains(&reference.path))
            && root.join(&reference.path).is_file()
        {
            Cow::Owned(read_limited(
                resolve_existing(root, &reference.path)?,
                MAX_INK_BYTES,
            )?)
        } else {
            let bytes = match options.encoded_ink.and_then(|encoded| encoded.get(index)) {
                Some(encoded) if reference.path.ends_with(".ink") => Cow::Borrowed(&encoded[..]),
                _ => Cow::Owned(ink_bytes(&reference.path, &snapshot.ink_layers[index])?),
            };
            if options
                .protected_new_ink
                .is_some_and(|paths| paths.contains(&reference.path))
            {
                write_new(root, &reference.path, &bytes)?;
            } else if !(unchanged(&reference.path)
                && already_on_disk(root, &reference.path, &bytes, MAX_INK_BYTES))
            {
                write_atomic(root, &reference.path, &bytes)?;
            }
            bytes
        };
        written.insert(&reference.path, bytes);
    }
    written.insert(
        page_path,
        Cow::Owned(write_json(root, page_path, &snapshot.page)?),
    );
    written.insert(
        layout::MANIFEST,
        Cow::Owned(write_json(root, layout::MANIFEST, &snapshot.manifest)?),
    );

    FileSet::of(snapshot)?.fingerprint_written(&written)
}

/// Atomic new-only install with an explicit no-write result for identical existing bytes.
pub fn install_reviewed_typst_style(
    root: &Path,
    filename: &str,
    source: &str,
) -> Result<(String, bool), StorageError> {
    validate_safe_filename(filename)?;
    if !filename.ends_with(".typ") || source.is_empty() || source.len() > MAX_TYPST_STYLE_BYTES {
        return Err(StorageError::InvalidPath(filename.into()));
    }
    let root = canonical_root(root)?;
    let relative = format!("styles/{filename}");
    match super::files::write_new(&root, &relative, source.as_bytes()) {
        Ok(()) => Ok((relative, true)),
        Err(StorageError::AlreadyExists(_)) => {
            let bytes = read_limited(resolve_existing(&root, &relative)?, MAX_TYPST_STYLE_BYTES)?;
            if bytes == source.as_bytes() {
                Ok((relative, false))
            } else {
                Err(StorageError::AlreadyExists(relative))
            }
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::{fixtures::*, *};
    use std::fs;

    /// P7: one stray file or one bad preset must not empty the list.
    #[test]
    fn style_listing_skips_unrelated_entries_and_reports_bad_typ_files_individually() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write_typst_style(root, "good.typ", "#set text(size: 12pt)").unwrap();
        let styles = root.join("styles");
        fs::write(styles.join("README.md"), "notes").unwrap();
        fs::write(styles.join("thumbs.db"), [0u8, 1, 2]).unwrap();
        fs::create_dir(styles.join("folder.typ")).unwrap();
        fs::write(
            styles.join("huge.typ"),
            vec![b'x'; super::MAX_TYPST_STYLE_BYTES + 1],
        )
        .unwrap();
        fs::write(styles.join("latin1.typ"), [0xe4u8, b'\n']).unwrap();
        let entries = list_typst_style_entries(root).unwrap();
        let paths: Vec<_> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(
            paths,
            ["styles/good.typ", "styles/huge.typ", "styles/latin1.typ"]
        );
        assert!(entries[0].bytes.is_ok());
        assert!(entries[1].bytes.is_err());
        // Bytes are returned as stored; deciding about UTF-8 belongs to the preset reader.
        assert!(entries[2].bytes.is_ok());
        let readable = list_typst_styles(root).unwrap();
        assert_eq!(readable.len(), 2);
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            fs::write(outside.path().join("secret.typ"), "SECRET").unwrap();
            std::os::unix::fs::symlink(outside.path().join("secret.typ"), styles.join("link.typ"))
                .unwrap();
            assert!(
                list_typst_style_entries(root)
                    .unwrap()
                    .iter()
                    .all(|e| e.path != "styles/link.typ")
            );
        }
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(root.join("styles/good.typ"), styles.join("link.typ"))
            .is_ok()
        {
            assert!(
                list_typst_style_entries(root)
                    .unwrap()
                    .iter()
                    .all(|e| e.path != "styles/link.typ")
            );
        }
    }

    #[test]
    fn reviewed_install_atomic_collision_has_exactly_one_writer() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("styles")).unwrap();
        let barrier = std::sync::Barrier::new(2);
        let results = std::thread::scope(|scope| {
            let threads = ["first", "second"].map(|source| {
                let barrier = &barrier;
                let root = temp.path();
                scope.spawn(move || {
                    barrier.wait();
                    install_reviewed_typst_style(root, "new.typ", source)
                })
            });
            threads.map(|t| t.join().unwrap())
        });
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        let source = std::fs::read_to_string(temp.path().join("styles/new.typ")).unwrap();
        assert!(source == "first" || source == "second");
        assert_eq!(
            install_reviewed_typst_style(temp.path(), "new.typ", &source).unwrap(),
            ("styles/new.typ".into(), false)
        );
    }

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

    fn old_mtime(path: &std::path::Path) -> std::time::SystemTime {
        let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        old
    }

    fn modified(path: &std::path::Path) -> std::time::SystemTime {
        fs::metadata(path).unwrap().modified().unwrap()
    }

    fn delta(
        snapshot: &NotebookSnapshot,
        strokes: serde_json::Value,
        blocks: serde_json::Value,
    ) -> PageDelta {
        serde_json::from_value(serde_json::json!({
            "page": snapshot.page, "modifiedAt": snapshot.manifest.modified_at,
            "blocks": blocks, "assets": [], "knownAssets": {},
            "inkLayers": [{ "schemaVersion": snapshot.page.schema_version,
                "id": snapshot.ink_layers[0].id, "pageId": snapshot.page.id, "strokes": strokes }]
        }))
        .unwrap()
    }

    /// Finding 16: an original that passes path validation but cannot be read back is refused
    /// before the revision becomes durable, instead of after it.
    #[test]
    fn unreadable_original_refuses_the_commit_before_anything_changes() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        fs::write(root.join("assets/huge.png"), vec![0u8; MAX_IMAGE_BYTES + 1]).unwrap();
        let mut history = NotebookHistory::default();
        let observed = observe_notebook(&root, &mut history).unwrap();
        let mut candidate = observed.clone();
        let mut image = candidate.page.objects[1].clone();
        if let crate::PageObject::Image {
            fields,
            source_path,
            ..
        } = &mut image
        {
            fields.id = "image-002".into();
            fields.reading_order = 2;
            *source_path = "assets/huge.png".into();
        }
        candidate.page.objects.push(image);
        candidate.page.reading_order.push("image-002".into());
        let before = tree_bytes(&root);
        assert!(commit_notebook(&root, &mut history, candidate.clone()).is_err());
        assert_eq!(tree_bytes(&root), before);
        assert!(!root.join(crate::layout::PENDING_TRANSACTION).exists());

        // The same history is still current: once the original is readable the retry lands on
        // the next revision rather than conflicting with a write the caller never saw.
        fs::write(root.join("assets/huge.png"), IMAGE_BYTES).unwrap();
        let saved = commit_notebook(&root, &mut history, candidate).unwrap();
        assert_eq!(saved.snapshot.page.revision, observed.page.revision + 1);
        let returned = saved
            .snapshot
            .assets
            .iter()
            .map(|file| (file.path.as_str(), file.bytes.as_slice()))
            .collect::<Vec<_>>();
        assert_eq!(
            returned,
            [
                ("assets/diagram.png", IMAGE_BYTES),
                ("assets/huge.png", IMAGE_BYTES)
            ]
        );
    }

    /// Finding 6: the pure checks run once per transaction, but every public entry still
    /// refuses malformed content before writing, and the filesystem checks still run at the
    /// point of writing.
    #[test]
    fn malformed_snapshots_fail_at_every_public_entry_before_any_write() {
        let malformed = |mut value: NotebookSnapshot| {
            let duplicate = value.page.objects[0].clone();
            value.page.objects.push(duplicate);
            value
        };

        let temporary = tempfile::tempdir().unwrap();
        let fresh = temporary.path().join("fresh");
        assert!(create_notebook(&fresh, &malformed(snapshot())).is_err());
        assert!(tree_bytes(&fresh).is_empty());

        let root = temporary.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let mut history = NotebookHistory::default();
        let observed = observe_notebook(&root, &mut history).unwrap();
        let before = tree_bytes(&root);
        assert!(save_notebook(&root, &malformed(observed.clone())).is_err());
        assert_eq!(tree_bytes(&root), before);
        assert!(commit_notebook(&root, &mut history, malformed(observed.clone())).is_err());
        assert_eq!(tree_bytes(&root), before);
        let mut bad_delta = delta(&observed, serde_json::json!([]), serde_json::json!([]));
        bad_delta.page = malformed(observed.clone()).page;
        assert!(commit_page_delta(&root, &mut history, bad_delta).is_err());
        assert_eq!(tree_bytes(&root), before);

        // A recovery intent carrying malformed content is refused before recovery writes.
        let canonical = super::canonical_root(&root).unwrap();
        let mut candidate = malformed(observed.clone()).without_assets();
        candidate.page.revision += 1;
        super::super::recovery::stage_transaction(&canonical, &observed, &candidate).unwrap();
        let staged = tree_bytes(&root);
        assert!(open_notebook(&root).is_err());
        assert_eq!(tree_bytes(&root), staged);
    }

    #[test]
    fn a_symlinked_destination_is_refused_at_the_point_of_writing() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let canonical = super::canonical_root(&root).unwrap();
        let previous = open_notebook(&root).unwrap();
        let mut candidate = previous.clone();
        candidate.page.revision += 1;
        candidate.blocks[0].bytes = b"$ replaced $".to_vec();
        // The content half accepts it: only the disk is wrong.
        super::validate_snapshot_content(&candidate).unwrap();

        let skip_content = || super::WriteOptions {
            content_validated: true,
            ..super::WriteOptions::default()
        };
        // Runs everywhere: a directory swapped in where the source file was.
        let block = root.join(&candidate.blocks[0].path);
        let original = fs::read(&block).unwrap();
        fs::remove_file(&block).unwrap();
        fs::create_dir(&block).unwrap();
        let swapped = tree_bytes(&root);
        assert!(super::save_to_root_inner(&canonical, &candidate, skip_content()).is_err());
        assert_eq!(tree_bytes(&root), swapped);
        assert!(block.is_dir());
        fs::remove_dir(&block).unwrap();
        fs::write(&block, original).unwrap();

        let outside = temporary.path().join("outside.typ");
        fs::write(&outside, "OUTSIDE").unwrap();
        fs::remove_file(&block).unwrap();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, &block).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&outside, &block).is_ok();
        if !linked {
            eprintln!("symlinks unavailable here; skipping the swapped-destination check");
            return;
        }
        let before = tree_bytes(&root);
        let manifest = fs::read(root.join(crate::layout::MANIFEST)).unwrap();
        assert!(super::save_to_root_inner(&canonical, &candidate, skip_content()).is_err());
        assert!(super::save_transactional(&canonical, &previous, &candidate).is_err());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "OUTSIDE");
        assert_eq!(
            fs::read(root.join(crate::layout::MANIFEST)).unwrap(),
            manifest
        );
        assert_eq!(tree_bytes(&root), before);
    }

    /// Finding 2: a delta commit leaves files whose bytes did not change untouched on disk,
    /// while the fingerprint still covers them (a following commit sees no external change).
    #[test]
    fn delta_commits_skip_rewriting_unchanged_files() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let mut history = NotebookHistory::default();
        let before = observe_notebook(&root, &mut history).unwrap();
        let block = root.join(&before.blocks[0].path);
        let ink = root.join(&before.page.ink_layers[0].path);

        // Transform only: the ink layer changes, the Typst source does not.
        let block_time = old_mtime(&block);
        let ink_time = old_mtime(&ink);
        let mut transform = before.ink_layers[0].strokes[0].transform.clone();
        transform.translate_x = 30.0;
        let moved = commit_page_delta(
            &root,
            &mut history,
            delta(
                &before,
                serde_json::json!([{ "kind": "reuse", "id": "stroke-001", "transform": transform }]),
                serde_json::json!([]),
            ),
        )
        .unwrap();
        assert_eq!(modified(&block), block_time);
        assert_ne!(modified(&ink), ink_time);

        // Typst only: the source changes, the ink layer does not.
        let block_time = old_mtime(&block);
        let ink_time = old_mtime(&ink);
        let edited = commit_page_delta(
            &root,
            &mut history,
            delta(
                &moved.snapshot,
                serde_json::json!([{ "kind": "reuse", "id": "stroke-001" }]),
                serde_json::json!([{ "path": "blocks/equation.typ", "bytes": b"$ E = m c^2 $" }]),
            ),
        )
        .unwrap();
        assert_ne!(modified(&block), block_time);
        assert_eq!(modified(&ink), ink_time);
        assert_eq!(fs::read(&block).unwrap(), b"$ E = m c^2 $");

        // The skipped files are still fingerprinted exactly: the next commit is not refused,
        // and undo restores the earlier state.
        let undone = undo_notebook(&root, &mut history).unwrap();
        assert_eq!(undone.snapshot.blocks[0].bytes, b"$ F = m a $");
        assert_eq!(fs::read(&block).unwrap(), b"$ F = m a $");
        let redone = redo_notebook(&root, &mut history).unwrap();
        assert_eq!(
            redone.snapshot.blocks[0].bytes,
            edited.snapshot.blocks[0].bytes
        );
        commit_page_delta(
            &root,
            &mut history,
            delta(
                &redone.snapshot,
                serde_json::json!([{ "kind": "reuse", "id": "stroke-001" }]),
                serde_json::json!([]),
            ),
        )
        .unwrap();
    }

    fn canonical_fingerprint_of(root: &std::path::Path) -> u64 {
        super::canonical_fingerprint(
            &super::canonical_root(root).unwrap(),
            &open_notebook(root).unwrap(),
        )
        .unwrap()
    }

    /// A file that changed on disk after the previous state was loaded is rewritten exactly as
    /// before, even though the candidate's bytes equal the previous state's.
    #[test]
    fn unchanged_candidate_still_replaces_a_file_that_changed_on_disk() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("notebook");
        create_notebook(&root, &snapshot()).unwrap();
        let canonical = super::canonical_root(&root).unwrap();
        let previous = open_notebook(&root).unwrap();
        let mut candidate = previous.clone();
        candidate.page.revision += 1;
        let block = root.join(&candidate.blocks[0].path);
        let ink = root.join(&candidate.page.ink_layers[0].path);
        let canonical_ink = fs::read(&ink).unwrap();
        fs::write(&block, "external").unwrap();
        let mut other = previous.ink_layers[0].clone();
        other.strokes[0].points[0].x = 1.0;
        fs::write(&ink, crate::ink_codec::encode(&other).unwrap()).unwrap();

        let fingerprint = super::save_transactional(&canonical, &previous, &candidate).unwrap();
        assert_eq!(fs::read(&block).unwrap(), candidate.blocks[0].bytes);
        assert_eq!(fs::read(&ink).unwrap(), canonical_ink);
        assert_eq!(fingerprint, canonical_fingerprint_of(&root));
    }

    /// Skipping writes must not weaken conflict detection: a same-revision external edit to any
    /// protected file still refuses the next commit and keeps the outside bytes.
    #[test]
    fn same_revision_external_edits_to_protected_files_are_still_refused() {
        type Edit = fn(&std::path::Path, &NotebookSnapshot);
        let edits: [(&str, Edit); 4] = [
            ("blocks/equation.typ", |path, _| {
                fs::write(path, "$ external $").unwrap()
            }),
            ("ink/page-001-layer-001.ink", |path, observed| {
                let mut layer = observed.ink_layers[0].clone();
                layer.strokes[0].points[0].x = 5.0;
                fs::write(path, crate::ink_codec::encode(&layer).unwrap()).unwrap()
            }),
            ("pages/page-001.json", |path, observed| {
                let mut page = observed.page.clone();
                page.objects[0].fields_mut().x = 9.0;
                fs::write(path, serde_json::to_vec_pretty(&page).unwrap()).unwrap()
            }),
            (crate::layout::MANIFEST, |path, observed| {
                let mut manifest = observed.manifest.clone();
                manifest.title = "Renamed outside".into();
                fs::write(path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap()
            }),
        ];
        for (relative, edit) in edits {
            let temporary = tempfile::tempdir().unwrap();
            let root = temporary.path().join("notebook");
            create_notebook(&root, &snapshot()).unwrap();
            let mut history = NotebookHistory::default();
            let observed = observe_notebook(&root, &mut history).unwrap();
            let path = root.join(relative);
            edit(&path, &observed);
            let outside = fs::read(&path).unwrap();
            let error = commit_page_delta(
                &root,
                &mut history,
                delta(
                    &observed,
                    serde_json::json!([{ "kind": "reuse", "id": "stroke-001" }]),
                    serde_json::json!([]),
                ),
            )
            .unwrap_err();
            assert!(
                error.to_string().contains("external change"),
                "{relative}: {error}"
            );
            assert_eq!(fs::read(&path).unwrap(), outside, "{relative}");
        }
    }
}
