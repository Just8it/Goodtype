//! Notebook paths, in the store's own error type.
//!
//! The name rules and the containment check live in [`crate::paths`], which the Typst export and
//! the desktop library share. What is left here is the notebook flavour of them: the store's
//! error type, the `blocks/`-style directory checks, and the write-target preparation that only
//! a writer needs.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use crate::{
    SourceRole,
    paths::{self, PathError},
};

use super::StorageError;

/// The store reports a refused path as an invalid notebook path however it was refused: a caller
/// cannot act differently on the distinction, and an I/O error while resolving still means the
/// path is unusable.
impl From<PathError> for StorageError {
    fn from(error: PathError) -> Self {
        match error {
            PathError::Io(error) => Self::Io(error),
            PathError::Invalid(value) | PathError::Escapes(value) => Self::InvalidPath(value),
        }
    }
}

pub(crate) fn canonical_root(root: &Path) -> Result<PathBuf, StorageError> {
    Ok(paths::canonical_root(root)?)
}

pub(crate) fn validate_relative(value: &str) -> Result<&Path, StorageError> {
    Ok(paths::validate_relative(value)?)
}

pub(crate) fn resolve_existing(root: &Path, relative: &str) -> Result<PathBuf, StorageError> {
    Ok(paths::resolve_file(root, relative)?)
}

/// Where a file is about to be written: parent directories created, and an existing target
/// confirmed to be an ordinary contained file.
///
/// The symlink check is the part that does not belong in the shared module. Reading through a
/// link is refused there by canonicalising; *writing* has to refuse the link itself, before it is
/// followed, or the write lands wherever the link points.
pub(crate) fn prepare_target(root: &Path, relative: &str) -> Result<PathBuf, StorageError> {
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
        paths::contained(root, &target, &relative.display().to_string())?;
        if !target.is_file() {
            return Err(StorageError::InvalidPath(relative.display().to_string()));
        }
    }
    Ok(target)
}

/// Create each missing component of `relative` under `root`, confirming containment at every
/// level rather than only at the end — a link introduced part-way down would otherwise be
/// followed while a final check still passed.
pub(crate) fn ensure_directory(root: &Path, relative: &Path) -> Result<PathBuf, StorageError> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(StorageError::InvalidPath(relative.display().to_string()));
        };
        current = paths::ensure_dir(root, &current.join(name), &relative.display().to_string())?;
    }
    Ok(current)
}

pub(crate) fn is_in_directory(value: &str, directory: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(
        components.next(),
        Some(Component::Normal(component)) if component == directory
    ) && components.next().is_some()
}

pub(crate) fn validate_asset_path(value: &str) -> Result<(), StorageError> {
    let path = validate_relative(value)?;
    if path.parent() != Some(Path::new(SourceRole::Asset.directory())) {
        return Err(StorageError::InvalidPath(value.into()));
    }
    validate_safe_filename(
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(""),
    )
}

pub(crate) fn validate_safe_filename(filename: &str) -> Result<(), StorageError> {
    let safe_characters = filename
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
    let stem = filename.split('.').next().unwrap_or("");
    let reserved = paths::is_windows_reserved(stem);
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
    use crate::{
        PageObject,
        storage::{fixtures::*, *},
    };

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
}
