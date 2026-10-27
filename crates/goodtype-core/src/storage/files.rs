//! Reading and writing bytes, with a ceiling and without a torn file.
//!
//! Also notebook-agnostic. Every write lands atomically through a temporary file in the same
//! directory, and every read refuses a file larger than the caller's limit before allocating for
//! it. `fingerprint_files` lives here because it consumes the same bytes.

use std::{
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Serialize, de::DeserializeOwned};
use tempfile::NamedTempFile;

use super::{MAX_INK_BYTES, MAX_JSON_BYTES, StorageError, StoredFile, invalid_error, paths::*};
use crate::InkLayer;

pub(crate) fn read_stored_files(
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

pub(crate) fn read_json<T: DeserializeOwned>(
    root: &Path,
    relative: &str,
) -> Result<T, StorageError> {
    read_json_limited(root, relative, MAX_JSON_BYTES)
}

pub(crate) fn read_json_limited<T: DeserializeOwned>(
    root: &Path,
    relative: &str,
    maximum: usize,
) -> Result<T, StorageError> {
    let bytes = read_limited(resolve_existing(root, relative)?, maximum)?;
    Ok(serde_json::from_slice(&bytes)?)
}

// Returns the exact bytes persisted, so a caller writing a set of canonical files can
// fingerprint them without reading them all back from disk.
pub(crate) fn write_json<T: Serialize>(
    root: &Path,
    relative: &str,
    value: &T,
) -> Result<Vec<u8>, StorageError> {
    let bytes = json_bytes(value, true, MAX_JSON_BYTES)?;
    write_atomic(root, relative, &bytes)?;
    Ok(bytes)
}

pub(crate) fn read_ink(root: &Path, relative: &str) -> Result<InkLayer, StorageError> {
    let bytes = read_limited(resolve_existing(root, relative)?, MAX_INK_BYTES)?;
    if relative.ends_with(".ink") {
        crate::ink_codec::decode(&bytes)
    } else if relative.ends_with(".json") {
        Ok(serde_json::from_slice(&bytes)?)
    } else {
        Err(invalid_error("ink layer path must end in .ink or .json"))
    }
}

pub(crate) fn ink_bytes(relative: &str, layer: &InkLayer) -> Result<Vec<u8>, StorageError> {
    if relative.ends_with(".ink") {
        crate::ink_codec::encode(layer)
    } else if relative.ends_with(".json") {
        json_bytes(layer, false, MAX_INK_BYTES)
    } else {
        Err(invalid_error("ink layer path must end in .ink or .json"))
    }
}

pub(crate) fn write_ink(
    root: &Path,
    relative: &str,
    layer: &InkLayer,
) -> Result<Vec<u8>, StorageError> {
    let bytes = ink_bytes(relative, layer)?;
    write_atomic(root, relative, &bytes)?;
    Ok(bytes)
}

pub(crate) fn write_ink_new(
    root: &Path,
    relative: &str,
    layer: &InkLayer,
) -> Result<Vec<u8>, StorageError> {
    let bytes = ink_bytes(relative, layer)?;
    write_new(root, relative, &bytes)?;
    Ok(bytes)
}

pub fn json_bytes<T: Serialize>(
    value: &T,
    pretty: bool,
    maximum: usize,
) -> Result<Vec<u8>, StorageError> {
    let mut bytes = if pretty {
        serde_json::to_vec_pretty(value)?
    } else {
        serde_json::to_vec(value)?
    };
    bytes.push(b'\n');
    if bytes.len() > maximum {
        return Err(StorageError::InvalidNotebook(format!(
            "serialized file is {} bytes; maximum is {maximum}",
            bytes.len()
        )));
    }
    Ok(bytes)
}

// One hashing procedure shared by the read path (`canonical_fingerprint`) and the write path
// (`save_to_root`) so a fingerprint computed from freshly written bytes equals one computed by
// reading the same files back. The fingerprint is process-local change detection, not a
// security signature.
pub(crate) fn fingerprint_files(entries: &[(&str, &[u8])]) -> u64 {
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

pub(crate) fn write_atomic(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), StorageError> {
    let target = prepare_target(root, relative)?;
    Ok(persist_atomic(&target, bytes, true)?)
}

pub(crate) fn write_once(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), StorageError> {
    let target = prepare_target(root, relative)?;
    if target.exists() {
        if has_contents(&target, bytes)? {
            return Ok(());
        }
        return Err(StorageError::AlreadyExists(relative.into()));
    }

    write_new(root, relative, bytes)
}

/// Create a new file without adopting any pre-existing bytes, even when they happen to match.
pub(crate) fn write_new(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), StorageError> {
    let target = prepare_target(root, relative)?;
    persist_atomic(&target, bytes, false).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            StorageError::AlreadyExists(relative.into())
        } else {
            StorageError::Io(error)
        }
    })
}

/// Persist bytes through a synced sibling temporary file. The caller must validate the root
/// and create/contain the parent directory; this primitive never authorizes a path.
/// `replace` chooses atomic replacement or creation without overwriting an existing file.
pub fn persist_atomic(target: &Path, bytes: &[u8], replace: bool) -> io::Result<()> {
    reject_symlink(target)?;
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "write target has no parent"))?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    let mut attempt = 0;
    loop {
        reject_symlink(target)?;
        let result = if replace {
            temporary.persist(target)
        } else {
            temporary.persist_noclobber(target)
        };
        match result {
            Ok(_) => return Ok(()),
            Err(error) if attempt < PERSIST_RETRIES && briefly_locked(&error.error) => {
                // The temporary file comes back with the error, so the next attempt still
                // replaces the target in one atomic rename.
                temporary = error.file;
                attempt += 1;
                std::thread::sleep(PERSIST_RETRY_DELAY * attempt);
            }
            Err(error) => return Err(error.error),
        }
    }
}

/// Refuse a path that is itself a symbolic link (or junction); a missing path is fine.
///
/// A write follows a link, so a file replaced by one would land wherever it points. Checked on the
/// link rather than through it, which is why this is `symlink_metadata`. Every writer — the store,
/// the desktop app's own files, the export — uses this one check.
pub fn reject_symlink(target: &Path) -> io::Result<()> {
    match target.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "that file cannot be a symbolic link",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Retries spread over roughly a second, longer than an on-access virus scan or a sync client
/// usually holds a file it just saw change.
const PERSIST_RETRIES: u32 = 8;
const PERSIST_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(25);

/// On Windows a rename onto a file that another process holds open without delete sharing fails
/// with "access denied" (5) or a sharing violation (32). Defender, the search indexer and cloud
/// sync open freshly written files exactly like that for a moment, which surfaced as "could not
/// be saved" on an ordinary pen stroke. Elsewhere a rename is not blocked by readers.
fn briefly_locked(error: &io::Error) -> bool {
    cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32))
}

/// Read a whole file, refusing one larger than `maximum` before allocating for it.
///
/// The size is taken from the open handle, not from a separate `metadata` call on the path, and
/// the read itself stops one byte past the ceiling, so a file that grows or is swapped between the
/// check and the read is still refused. Every bounded read in Goodtype goes through here; an
/// oversized file is reported as [`io::ErrorKind::InvalidData`].
pub fn read_bounded(path: &Path, maximum: usize) -> io::Result<Vec<u8>> {
    let too_large = |length: u64| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file is {length} bytes; maximum is {maximum}"),
        )
    };
    let file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    if length > maximum as u64 {
        return Err(too_large(length));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(too_large(bytes.len() as u64));
    }
    Ok(bytes)
}

/// Whether the file at `target` holds exactly `bytes`. Reads no more than `bytes.len()` of it, so
/// a large file at the target is refused by its size rather than read whole to be compared.
pub(crate) fn has_contents(target: &Path, bytes: &[u8]) -> Result<bool, StorageError> {
    match read_bounded(target, bytes.len()) {
        Ok(existing) => Ok(existing == bytes),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// [`read_bounded`] in the store's error type: an oversized file is an invalid notebook.
pub(crate) fn read_limited(path: PathBuf, maximum: usize) -> Result<Vec<u8>, StorageError> {
    read_bounded(&path, maximum).map_err(|error| {
        if error.kind() == io::ErrorKind::InvalidData {
            StorageError::InvalidNotebook(error.to_string())
        } else {
            StorageError::Io(error)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_creation_never_overwrites_and_failed_replacement_preserves_state() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("state.json");
        persist_atomic(&target, b"original", false).unwrap();
        let error = persist_atomic(&target, b"replacement", false).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"original");
        persist_atomic(&target, b"replacement", true).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"replacement");
        let blocked = directory.path().join("directory");
        fs::create_dir(&blocked).unwrap();
        assert!(persist_atomic(&blocked, b"bytes", true).is_err());
        assert!(blocked.is_dir());
        assert_eq!(fs::read(&target).unwrap(), b"replacement");
    }

    /// Reproduces "Access is denied (os error 5)": another process holds the file open without
    /// delete sharing, as a virus scanner or sync client does right after a save.
    #[cfg(windows)]
    #[test]
    fn replacement_waits_out_a_brief_external_lock() {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("layer.ink");
        persist_atomic(&target, b"original", true).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&target)
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            drop(lock);
        });
        persist_atomic(&target, b"replacement", true).unwrap();
        release.join().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"replacement");
        let leftovers = fs::read_dir(directory.path()).unwrap().count();
        assert_eq!(leftovers, 1, "no temporary file may be left behind");
    }
}
