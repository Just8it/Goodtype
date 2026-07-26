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

use super::{MAX_JSON_BYTES, StorageError, StoredFile, paths::*};

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
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write_atomic(root, relative, &bytes)?;
    Ok(bytes)
}

// Ink is machine-written sample data, not hand-edited structure. Pretty-printing it costs
// roughly 1.5x the bytes of every stroke on every commit for no inspection benefit that
// `jq` does not already provide.
pub(crate) fn write_json_compact<T: Serialize>(
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
    let mut temporary = NamedTempFile::new_in(target.parent().expect("target parent"))?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(&target)
        .map_err(|error| StorageError::Io(error.error))?;
    Ok(())
}

pub(crate) fn write_once(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), StorageError> {
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

pub(crate) fn read_limited(path: PathBuf, maximum: usize) -> Result<Vec<u8>, StorageError> {
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
