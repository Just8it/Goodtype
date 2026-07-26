//! The library: one directory the writer points Goodtype at, and everything below it.
//!
//! Folders nest arbitrarily. Any folder holding `goodtype.json` is a notebook — a leaf that is
//! opened, never browsed into. That single rule is what keeps the tree unambiguous: without it,
//! `Semester 3/Thermodynamik/pages/` could be either a subfolder or a notebook's insides.
//!
//! **The filesystem is the model.** There is no index, no registry and no import step, so a
//! folder made in Explorer is a folder here, and nothing can drift out of agreement with the
//! disk. The cost is that every listing is a `read_dir`, which is why this reads one level at a
//! time and never walks the tree.
//!
//! The frontend never handles an absolute path. It names a path relative to the library root and
//! [`resolve`] is the only place one becomes real — see there for why that is a boundary rather
//! than a convention.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use tauri_plugin_dialog::DialogExt;

use crate::settings::{StoredLibrary, read_library, write_library};
use crate::workspace::{AllowedRoots, allow_root};

/// A notebook's manifest can be large; a listing only needs the page count out of it. Reading
/// more than this means the file is not a manifest we wrote.
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;

/// One row in a folder listing. Either a folder to descend into or a notebook to open — the
/// frontend never has to guess which from the shape of the fields.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LibraryEntry {
    Folder {
        name: String,
        path: String,
        modified_ms: Option<u64>,
        /// Folders and notebooks directly inside, so a tile can say "4 Elemente" without the
        /// frontend asking for a listing it will not otherwise use.
        child_count: usize,
    },
    Notebook {
        name: String,
        path: String,
        modified_ms: Option<u64>,
        /// `None` when the manifest could not be read. The notebook still lists — a tile that
        /// cannot state its length is better than a notebook that vanishes from the shelf.
        page_count: Option<usize>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryListing {
    /// Echoed back so a listing that arrives late cannot be mistaken for the current folder.
    pub path: String,
    pub entries: Vec<LibraryEntry>,
}

/// Resolve a library-relative path to a real one inside the library.
///
/// Rejecting `..`, absolute paths and backslashes is not enough on its own: a symlink inside the
/// library can still point anywhere on the disk. So the resolved path is canonicalised — which
/// follows every link — and then required to sit under the canonical root. That second check is
/// what makes the library a boundary rather than a suggestion, and it is the same shape as the
/// containment check the PDF export already applies to image assets.
///
/// An empty string is the library root itself.
pub fn resolve(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    if relative.is_empty() {
        return Ok(canonical_root);
    }
    if relative.contains('\\') {
        return Err("library paths use forward slashes".to_owned());
    }
    let candidate = Path::new(relative);
    if candidate.is_absolute()
        || !candidate
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err("that path leaves the library".to_owned());
    }
    let resolved =
        fs::canonicalize(canonical_root.join(candidate)).map_err(|error| error.to_string())?;
    if !resolved.starts_with(&canonical_root) {
        return Err("that path leaves the library".to_owned());
    }
    Ok(resolved)
}

/// Whether a directory is a notebook rather than an ordinary folder.
pub fn is_notebook(path: &Path) -> bool {
    path.join("goodtype.json").is_file()
}

/// Pages in a notebook, read from its manifest.
///
/// Deliberately not `goodtype_core::storage::open`: that loads every page and its strokes, which
/// is the right thing when opening one notebook and the wrong thing forty times over to draw a
/// shelf. Only the length of `pages` is needed here.
fn page_count(notebook: &Path) -> Option<usize> {
    #[derive(Deserialize)]
    struct PageCountOnly {
        pages: Vec<serde::de::IgnoredAny>,
    }
    let manifest = notebook.join("goodtype.json");
    if fs::metadata(&manifest).ok()?.len() > MAX_MANIFEST_BYTES {
        return None;
    }
    let bytes = fs::read(manifest).ok()?;
    serde_json::from_slice::<PageCountOnly>(&bytes)
        .ok()
        .map(|manifest| manifest.pages.len())
}

fn modified_ms(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|since| since.as_millis() as u64)
}

/// Directly contained folders and notebooks, ignoring anything hidden.
fn child_count(folder: &Path) -> usize {
    let Ok(entries) = fs::read_dir(folder) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_dir() && !is_hidden(&entry.file_name()))
        .count()
}

/// A leading dot hides an entry, which is how `.goodtype/` stays out of the shelf on every
/// platform without a per-OS attribute check.
fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

/// One level of the library.
///
/// Only directories appear. A loose file inside the library is not a notebook and not a folder,
/// so showing it would invite the writer to click something Goodtype cannot open.
pub fn read_folder(root: &Path, relative: &str) -> Result<Vec<LibraryEntry>, String> {
    let folder = resolve(root, relative)?;
    if !folder.is_dir() {
        return Err("that folder is not in the library any more".to_owned());
    }
    let prefix = if relative.is_empty() {
        String::new()
    } else {
        format!("{relative}/")
    };

    let mut folders = Vec::new();
    let mut notebooks = Vec::new();
    for entry in fs::read_dir(&folder).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if !path.is_dir() || is_hidden(&entry.file_name()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_path = format!("{prefix}{name}");
        if is_notebook(&path) {
            notebooks.push(LibraryEntry::Notebook {
                name,
                path: child_path,
                modified_ms: modified_ms(&path.join("goodtype.json")),
                page_count: page_count(&path),
            });
        } else {
            folders.push(LibraryEntry::Folder {
                name,
                path: child_path,
                modified_ms: modified_ms(&path),
                child_count: child_count(&path),
            });
        }
    }

    // Folders first, then notebooks, each by name. The design bands them separately, and a
    // stable order here means the two bands never reshuffle between listings of the same folder.
    folders.sort_by(|a, b| sort_key(a).cmp(sort_key(b)));
    notebooks.sort_by(|a, b| sort_key(a).cmp(sort_key(b)));
    folders.append(&mut notebooks);
    Ok(folders)
}

fn sort_key(entry: &LibraryEntry) -> &str {
    match entry {
        LibraryEntry::Folder { name, .. } | LibraryEntry::Notebook { name, .. } => name,
    }
}

/// The library the writer chose, if they have chosen one.
///
/// Re-admits it to the allowlist on the way out: the path outlives the process, the allowlist
/// does not, so the first read after a restart is what puts it back.
#[tauri::command]
pub fn library_root(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
) -> Result<Option<String>, String> {
    let Some(root) = read_library(&app)?.root else {
        return Ok(None);
    };
    let path = Path::new(&root);
    if !path.is_dir() {
        return Ok(None);
    }
    allow_root(&roots, path)?;
    Ok(Some(root))
}

/// Choose the library with the native dialog.
///
/// Any directory will do — unlike opening a single notebook, there is nothing to verify. An empty
/// folder is a perfectly good empty library, and requiring a marker file would mean an import
/// step, which is the thing this design is built to avoid.
#[tauri::command]
pub async fn pick_library_root(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
) -> Result<Option<String>, String> {
    let Some(picked) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let picked = picked.into_path().map_err(|error| error.to_string())?;
    let canonical = allow_root(&roots, &picked)?;
    let root = canonical
        .into_os_string()
        .into_string()
        .map_err(|_| "that path is not valid Unicode".to_owned())?;
    write_library(
        &app,
        &StoredLibrary {
            root: Some(root.clone()),
        },
    )?;
    Ok(Some(root))
}

/// List one folder of the library. `path` is relative to the root; empty means the root.
#[tauri::command]
pub fn list_library(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    path: String,
) -> Result<LibraryListing, String> {
    let root = current_root(&app, &roots)?;
    let entries = read_folder(&root, &path)?;
    Ok(LibraryListing { path, entries })
}

/// Admit a notebook inside the library and hand back its absolute root, which is what every
/// existing notebook command already takes. Nothing downstream needs to know about the library.
#[tauri::command]
pub fn open_library_notebook(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    path: String,
) -> Result<String, String> {
    let root = current_root(&app, &roots)?;
    let notebook = resolve(&root, &path)?;
    if !is_notebook(&notebook) {
        return Err("that folder is not a notebook".to_owned());
    }
    allow_root(&roots, &notebook)?
        .into_os_string()
        .into_string()
        .map_err(|_| "that path is not valid Unicode".to_owned())
}

/// The chosen library, verified against the allowlist so a stale settings file cannot widen what
/// this process will touch.
fn current_root(app: &tauri::AppHandle, roots: &AllowedRoots) -> Result<PathBuf, String> {
    let root = read_library(app)?
        .root
        .ok_or("no library has been chosen yet")?;
    crate::workspace::ensure_allowed(roots, &root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let semester = root.path().join("Semester 3");
        fs::create_dir(&semester).unwrap();
        let notebook = semester.join("Thermodynamik");
        fs::create_dir(&notebook).unwrap();
        fs::write(
            notebook.join("goodtype.json"),
            br#"{"pages":[{"id":"a"},{"id":"b"},{"id":"c"}]}"#,
        )
        .unwrap();
        fs::create_dir(semester.join("Altklausuren")).unwrap();
        fs::create_dir(root.path().join(".goodtype")).unwrap();
        root
    }

    #[test]
    fn a_folder_holding_a_manifest_is_a_notebook_and_the_rest_are_folders() {
        let root = library();
        let entries = read_folder(root.path(), "Semester 3").unwrap();

        // Folders sort ahead of notebooks, so `Altklausuren` leads despite the alphabet.
        assert!(matches!(
            &entries[0],
            LibraryEntry::Folder { name, child_count: 0, .. } if name == "Altklausuren"
        ));
        assert!(matches!(
            &entries[1],
            LibraryEntry::Notebook { name, page_count: Some(3), path, .. }
                if name == "Thermodynamik" && path == "Semester 3/Thermodynamik"
        ));
        assert_eq!(entries.len(), 2);
    }

    /// `.goodtype/` holds the library's own preferences and must never appear on the shelf.
    #[test]
    fn hidden_directories_stay_out_of_the_listing() {
        let root = library();
        let entries = read_folder(root.path(), "").unwrap();
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert!(
            matches!(&entries[0], LibraryEntry::Folder { name, child_count: 2, .. } if name == "Semester 3")
        );
    }

    /// The frontend supplies these strings, so every way out of the library has to be closed
    /// here rather than trusted not to be attempted.
    #[test]
    fn a_path_cannot_climb_out_of_the_library() {
        let root = library();
        for escape in [
            "..",
            "Semester 3/../..",
            "/etc",
            "Semester 3\\Thermodynamik",
        ] {
            assert!(
                resolve(root.path(), escape).is_err(),
                "`{escape}` should have been refused"
            );
        }
        assert!(resolve(root.path(), "Semester 3/Thermodynamik").is_ok());
        assert_eq!(
            resolve(root.path(), "").unwrap(),
            fs::canonicalize(root.path()).unwrap()
        );
    }

    /// A manifest that cannot be parsed must not remove the notebook from the shelf — it is
    /// still a notebook, it just cannot say how long it is.
    #[test]
    fn an_unreadable_manifest_still_lists_the_notebook() {
        let root = library();
        let broken = root.path().join("Kaputt");
        fs::create_dir(&broken).unwrap();
        fs::write(broken.join("goodtype.json"), b"{ not json").unwrap();

        let entries = read_folder(root.path(), "").unwrap();
        assert!(entries.iter().any(|entry| matches!(
            entry,
            LibraryEntry::Notebook { name, page_count: None, .. } if name == "Kaputt"
        )));
    }
}
