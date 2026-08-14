//! Writing a small JSON file where it will survive losing power.
//!
//! The store crate already does this for canonical notebook content
//! ([`goodtype_core::storage`]); this is the same guarantee for the files the desktop app owns
//! itself — settings, recents, the library shelf, phase-0 metrics. They were written three
//! different ways: one flushed without syncing, one synced, and one went through a plain
//! `fs::write` that could leave a truncated file behind. None of that was deliberate, and the
//! shelf losing its favourites to a hard power-off is what it cost.
//!
//! Every write here lands through a temporary file in the destination's own directory, is
//! flushed and synced before it is renamed, and refuses a destination that has become a
//! symbolic link.

use std::{fs, io::Write, path::Path};

use serde::Serialize;

/// Refuse a destination that is a symbolic link.
///
/// A link is followed by the write, so a store-owned file replaced by one would put the app's
/// own state wherever the link pointed. Checked before writing rather than after, and checked on
/// the link itself rather than through it, which is why this is `symlink_metadata`.
pub fn reject_symlink(target: &Path) -> Result<(), String> {
    if target
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err("that file cannot be a symbolic link".to_owned());
    }
    Ok(())
}

/// Serialize `value` as the pretty JSON these files are stored in, with the trailing newline a
/// text file ought to end with, refusing anything past `maximum`.
pub fn json_bytes<T: Serialize>(value: &T, maximum: usize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    if bytes.len() > maximum {
        return Err(format!(
            "that file is {} bytes; maximum is {maximum}",
            bytes.len()
        ));
    }
    Ok(bytes)
}

/// Replace `target` with `bytes`, or leave it exactly as it was.
///
/// The `sync_all` is the part that is easy to leave out and hard to notice missing: without it
/// the rename can reach the disk before the contents do, and an interrupted write leaves a file
/// that exists, parses, and is empty.
pub fn write_atomic(target: &Path, bytes: &[u8]) -> Result<(), String> {
    reject_symlink(target)?;
    let parent = target.parent().ok_or("that file has no parent directory")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temporary
        .write_all(bytes)
        .and_then(|_| temporary.flush())
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| error.to_string())?;
    temporary
        .persist(target)
        .map_err(|error| error.error.to_string())?;
    Ok(())
}

/// Serialize and write in one step, which is what every caller actually wants.
pub fn write_json<T: Serialize>(target: &Path, value: &T, maximum: usize) -> Result<(), String> {
    write_atomic(target, &json_bytes(value, maximum)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replaced_file_is_never_left_half_written() {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("shelf.json");
        write_json(&target, &serde_json::json!({ "favourites": ["a"] }), 4096).unwrap();
        assert!(fs::read_to_string(&target).unwrap().contains("\"a\""));

        // A second write replaces the first outright rather than overwriting it in place.
        write_json(&target, &serde_json::json!({ "favourites": ["b"] }), 4096).unwrap();
        let written = fs::read_to_string(&target).unwrap();
        assert!(written.contains("\"b\"") && !written.contains("\"a\""));
        assert!(
            written.ends_with('\n'),
            "a text file should end in a newline"
        );
    }

    #[test]
    fn an_oversized_payload_is_refused_before_it_reaches_the_file() {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("settings.json");
        write_json(&target, &"small", 4096).unwrap();

        let big = "x".repeat(4096);
        assert!(write_json(&target, &big, 4096).is_err());
        // The refusal must leave the previous contents exactly as they were.
        assert_eq!(fs::read_to_string(&target).unwrap().trim(), "\"small\"");
    }

    #[test]
    fn a_destination_that_became_a_link_is_refused() {
        let temporary = tempfile::tempdir().unwrap();
        let outside = temporary.path().join("outside.json");
        let target = temporary.path().join("linked.json");
        fs::write(&outside, b"untouched").unwrap();

        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&outside, &target).is_ok();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, &target).is_ok();

        if linked {
            assert!(write_json(&target, &"replacement", 4096).is_err());
            assert_eq!(fs::read_to_string(&outside).unwrap(), "untouched");
        }
    }
}
