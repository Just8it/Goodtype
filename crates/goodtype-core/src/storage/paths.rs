//! Turning a caller's relative path into a real one, or refusing to.
//!
//! Nothing here knows what a notebook is. It exists so that the rules keeping a write inside the
//! notebook directory — no absolute path, no `..`, no symlink leading out, no reserved Windows
//! name — sit in one place that can be read and tested on its own. Everything else in `storage`
//! reaches the filesystem through these functions.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use crate::SourceRole;

use super::StorageError;

pub(crate) fn canonical_root(root: &Path) -> Result<PathBuf, StorageError> {
    let canonical = fs::canonicalize(root)?;
    if !canonical.is_dir() {
        return Err(StorageError::InvalidPath(root.display().to_string()));
    }
    Ok(canonical)
}

pub(crate) fn validate_relative(value: &str) -> Result<&Path, StorageError> {
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

pub(crate) fn resolve_existing(root: &Path, relative: &str) -> Result<PathBuf, StorageError> {
    let relative = validate_relative(relative)?;
    let resolved = fs::canonicalize(root.join(relative))?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err(StorageError::InvalidPath(relative.display().to_string()));
    }
    Ok(resolved)
}

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
