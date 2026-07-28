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
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use goodtype_core::{PageDefaults, layout, paths};
use serde::{Deserialize, Serialize};
use tauri_plugin_dialog::DialogExt;

use crate::settings::{StoredLibrary, read_library, write_library};
use crate::workspace::{AllowedRoots, allow_root};

/// A notebook's manifest can be large; a listing only needs the page count out of it. Reading
/// more than this means the file is not a manifest we wrote.
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;

/// One row in a folder listing. Either a folder to descend into or a notebook to open — the
/// frontend never has to guess which from the shape of the fields.
/// `rename_all` governs the variant names; the fields of a struct variant need
/// `rename_all_fields` of their own, or `modified_ms` reaches the frontend still in snake_case.
/// Same shape as `PagePosition` in `goodtype_core::storage`.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
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
        /// The paper this notebook is written on, so a tile can draw its real ruling before any
        /// cover of its contents exists. It costs nothing: the manifest is already open for the
        /// page count, and ruling is geometry rather than ink.
        paper: Option<PageDefaults>,
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
    let canonical_root = paths::canonical_root(root).map_err(|error| error.to_string())?;
    if relative.is_empty() {
        return Ok(canonical_root);
    }
    let candidate = paths::validate_library_relative(relative)
        .map_err(|_| "that path leaves the library".to_owned())?;
    paths::contained(&canonical_root, &canonical_root.join(candidate), relative).map_err(|error| {
        match error {
            paths::PathError::Io(error) => error.to_string(),
            _ => "that path leaves the library".to_owned(),
        }
    })
}

/// Whether a directory is a notebook rather than an ordinary folder.
pub fn is_notebook(path: &Path) -> bool {
    path.join(layout::MANIFEST).is_file()
}

/// What a shelf needs out of a notebook's manifest: how long it is, and what paper it uses.
///
/// Deliberately not `goodtype_core::storage::open`: that loads every page and its strokes, which
/// is the right thing when opening one notebook and the wrong thing forty times over to draw a
/// shelf. `IgnoredAny` counts the pages without building any of them.
///
/// `default_page` is optional so a manifest that has drifted still yields a page count. A tile
/// with no paper falls back to plain; a notebook that vanishes from the shelf has no fallback.
fn peek_manifest(notebook: &Path) -> (Option<usize>, Option<PageDefaults>) {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ManifestPeek {
        pages: Vec<serde::de::IgnoredAny>,
        #[serde(default)]
        default_page: Option<PageDefaults>,
    }
    let manifest = notebook.join(layout::MANIFEST);
    let within_ceiling = fs::metadata(&manifest)
        .map(|metadata| metadata.len() <= MAX_MANIFEST_BYTES)
        .unwrap_or(false);
    if !within_ceiling {
        return (None, None);
    }
    let Ok(bytes) = fs::read(manifest) else {
        return (None, None);
    };
    match serde_json::from_slice::<ManifestPeek>(&bytes) {
        Ok(peek) => (Some(peek.pages.len()), peek.default_page),
        Err(_) => (None, None),
    }
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
            let (page_count, paper) = peek_manifest(&path);
            notebooks.push(LibraryEntry::Notebook {
                name,
                path: child_path,
                modified_ms: modified_ms(&path.join(layout::MANIFEST)),
                page_count,
                paper,
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

/// Names Goodtype will create a folder or notebook under.
///
/// The mirror of `nameProblem` in `apps/desktop/src/lib/library/library.ts`, and the one that
/// counts: the frontend's copy makes the message immediate, this one makes it true. A frontend
/// can be wrong or bypassed, and either way the name is about to become a directory.
///
/// Deliberately narrower than what this platform accepts. A library is synced and shared, so
/// these names land on Windows, macOS and Linux alike, and the rule is what all three take.
/// Trailing dots and spaces are refused because Windows silently strips them, which would turn
/// two names the writer sees as distinct into one directory.
pub fn validate_name(name: &str) -> Result<(), String> {
    const FORBIDDEN: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
    const RESERVED: [&str; 6] = ["con", "prn", "aux", "nul", "com", "lpt"];

    if name.is_empty() || name.len() > 80 {
        return Err("names are 1 to 80 characters".to_owned());
    }
    if name.trim() != name {
        return Err("names cannot start or end with a space".to_owned());
    }
    if name.starts_with('.') {
        return Err("a name starting with a dot would be hidden".to_owned());
    }
    if name.ends_with('.') {
        return Err("names cannot end with a dot".to_owned());
    }
    if name.contains(FORBIDDEN) || name.chars().any(|c| c.is_control()) {
        return Err(r#"a name cannot contain \ / : * ? " < > |"#.to_owned());
    }
    let lowered = name.to_ascii_lowercase();
    if RESERVED.iter().any(|reserved| {
        lowered == *reserved
            || (lowered.len() == 4
                && lowered.starts_with(reserved)
                && lowered.ends_with(|c: char| c.is_ascii_digit()))
    }) {
        return Err("that name is reserved by Windows".to_owned());
    }
    Ok(())
}

/// Where a new entry will go, refusing a name already taken.
///
/// `create_dir` would refuse a collision on its own, but not before the caller has decided what
/// to tell the writer. Checking first is what turns "os error 183" into a sentence.
fn free_child(root: &Path, parent: &str, name: &str) -> Result<PathBuf, String> {
    validate_name(name)?;
    let parent = resolve(root, parent)?;
    if is_notebook(&parent) {
        return Err("notebook contents cannot be changed from the library".to_owned());
    }
    reject_notebook_internals(root, &parent)?;
    let target = parent.join(name);
    if target.exists() {
        return Err(format!("`{name}` already exists here"));
    }
    Ok(target)
}

/// Library commands may move a notebook as one shelf entry, but never mutate its internals.
fn reject_notebook_internals(root: &Path, path: &Path) -> Result<(), String> {
    let root = paths::canonical_root(root).map_err(|error| error.to_string())?;
    if path.starts_with(root.join(INTERNAL_DIR)) {
        return Err("library metadata cannot be changed from the shelf".to_owned());
    }
    let mut ancestor = path.parent();
    while let Some(candidate) = ancestor {
        if candidate == root {
            break;
        }
        if is_notebook(candidate) {
            return Err("notebook contents cannot be changed from the library".to_owned());
        }
        ancestor = candidate.parent();
    }
    Ok(())
}

fn relative_child(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

/// Make a folder, and answer with the path that lists it.
#[tauri::command]
pub fn create_library_folder(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    parent: String,
    name: String,
) -> Result<String, String> {
    let root = current_root(&app, &roots)?;
    fs::create_dir(free_child(&root, &parent, &name)?).map_err(|error| error.to_string())?;
    Ok(relative_child(&parent, &name))
}

/// Make an empty directory for a notebook and admit it, handing back the absolute root.
///
/// The manifest is not written here. The frontend already knows how to fill an empty directory —
/// it is the same path that created a notebook from the old start screen — and duplicating it in
/// Rust would give the store two authors for the same file.
#[tauri::command]
pub fn create_library_notebook(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    parent: String,
    name: String,
) -> Result<String, String> {
    let root = current_root(&app, &roots)?;
    let target = free_child(&root, &parent, &name)?;
    fs::create_dir(&target).map_err(|error| error.to_string())?;
    allow_root(&roots, &target)?
        .into_os_string()
        .into_string()
        .map_err(|_| "that path is not valid Unicode".to_owned())
}

/// Rename in place. Answers with the new path, since the old one has stopped existing.
#[tauri::command]
pub fn rename_library_entry(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    path: String,
    name: String,
) -> Result<String, String> {
    let root = current_root(&app, &roots)?;
    let source = resolve(&root, &path)?;
    reject_notebook_internals(&root, &source)?;
    let parent = parent_of(&path);
    let target = free_child(&root, parent, &name)?;
    fs::rename(&source, &target).map_err(|error| error.to_string())?;
    forget_favourite(&root, &path)?;
    Ok(relative_child(parent, &name))
}

/// Move an entry into another folder of the library.
#[tauri::command]
pub fn move_library_entry(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    path: String,
    destination: String,
) -> Result<String, String> {
    let root = current_root(&app, &roots)?;
    let source = resolve(&root, &path)?;
    let into = resolve(&root, &destination)?;
    reject_notebook_internals(&root, &source)?;
    reject_notebook_internals(&root, &into)?;
    if !into.is_dir() || is_notebook(&into) {
        return Err("that destination is not a folder".to_owned());
    }
    // Moving a folder inside itself would carry the tree out of reach of the shelf, and on most
    // filesystems it half-succeeds rather than failing cleanly.
    if into.starts_with(&source) {
        return Err("a folder cannot be moved into itself".to_owned());
    }
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("that entry has no name")?
        .to_owned();
    let target = free_child(&root, &destination, &name)?;
    fs::rename(&source, &target).map_err(|error| error.to_string())?;
    forget_favourite(&root, &path)?;
    Ok(relative_child(&destination, &name))
}

/// Move an entry to the library's trash.
///
/// Not a permanent delete. A misclick here costs a semester of coursework, and the shelf is the
/// one surface where a whole notebook is a single click away. The trash lives at
/// `.goodtype/trash/` inside the library: hidden, so it never appears on a shelf; inside the
/// library, so it travels with it and needs no platform-specific recycle bin; timestamped, so
/// two deletions of the same name do not collide.
///
/// This is a change of mind. The plan said the OS recycle bin, on the grounds that an in-app
/// trash is a second place where truth lives. That argument holds against a *Trash view* in the
/// sidebar competing with the filesystem; it does not justify deleting a notebook outright, and
/// the recycle bin would mean a new dependency for behaviour a hidden folder already gives.
#[tauri::command]
pub fn delete_library_entry(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    path: String,
) -> Result<(), String> {
    let root = current_root(&app, &roots)?;
    let source = resolve(&root, &path)?;
    reject_notebook_internals(&root, &source)?;
    if source == root {
        return Err("the library itself cannot be deleted".to_owned());
    }
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("that entry has no name")?;
    let internal = paths::ensure_dir(&root, &root.join(INTERNAL_DIR), INTERNAL_DIR)
        .map_err(|_| "the library metadata directory leaves the library".to_owned())?;
    let trash = paths::ensure_dir(&root, &internal.join(TRASH_DIR), TRASH_DIR)
        .map_err(|_| "the library trash leaves the library".to_owned())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis())
        .unwrap_or(0);
    let target = trash.join(format!("{stamp}-{name}"));
    if target.exists() {
        return Err("a trash entry with that name already exists".to_owned());
    }
    fs::rename(&source, target).map_err(|error| error.to_string())?;
    forget_favourite(&root, &path)?;
    Ok(())
}

fn parent_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(cut) => &path[..cut],
        None => "",
    }
}

/// Store-owned state for this library, reusing the name a notebook already keeps its own
/// bookkeeping under. Hidden, so it never lists; inside the library, so it travels with it.
const INTERNAL_DIR: &str = layout::INTERNAL_DIR;
const TRASH_DIR: &str = "trash";
const SHELF_FILE: &str = "shelf.json";
const MAX_SHELF_BYTES: u64 = 1024 * 1024;

fn internal_file(root: &Path, name: &str, create: bool) -> Result<PathBuf, String> {
    let root = paths::canonical_root(root).map_err(|error| error.to_string())?;
    let directory = root.join(INTERNAL_DIR);
    let directory = if create {
        paths::ensure_dir(&root, &directory, INTERNAL_DIR)
    } else {
        paths::resolve_dir(&root, &directory, INTERNAL_DIR)
    }
    .map_err(|_| "the metadata directory leaves its root".to_owned())?;
    let target = directory.join(name);
    if target
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err("the metadata file cannot be a symbolic link".to_owned());
    }
    Ok(target)
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct Shelf {
    /// Library-relative paths. A favourite is app state, not a property of the directory, which
    /// is why it lives here rather than in the notebook.
    favourites: Vec<String>,
}

fn read_shelf(root: &Path) -> Shelf {
    let Ok(path) = internal_file(root, SHELF_FILE, false) else {
        return Shelf::default();
    };
    let within_ceiling = fs::metadata(&path)
        .map(|metadata| metadata.len() <= MAX_SHELF_BYTES)
        .unwrap_or(false);
    if !within_ceiling {
        return Shelf::default();
    }
    // A preferences file that will not parse must not stop the library opening.
    fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_shelf(root: &Path, shelf: &Shelf) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(shelf).map_err(|error| error.to_string())?;
    fs::write(internal_file(root, SHELF_FILE, true)?, bytes).map_err(|error| error.to_string())
}

/// Drop a favourite whose entry has just moved, been renamed, or been thrown away.
///
/// A favourite is a path, so any of those three leaves it pointing at nothing. Rather than
/// rewrite the entry — which would also have to rewrite every favourite *below* a renamed
/// folder — the star is dropped. Losing a star on a rename is a small surprise; a Favoriten view
/// full of entries that no longer exist is a broken one.
fn forget_favourite(root: &Path, path: &str) -> Result<(), String> {
    let mut shelf = read_shelf(root);
    let before = shelf.favourites.len();
    shelf
        .favourites
        .retain(|favourite| favourite != path && !favourite.starts_with(&format!("{path}/")));
    if shelf.favourites.len() == before {
        return Ok(());
    }
    write_shelf(root, &shelf)
}

/// A notebook's cover lives in its own store-owned directory, beside the pending transaction and
/// the recovery copies. It is derived, regenerable state rather than part of the document, which
/// is the split `goodtype_core::layout` already draws with `.goodtype/`.
const COVER_FILE: &str = "cover.png";
/// Wide enough for a 304px raster of a busy page, far short of anything that is not a thumbnail.
const MAX_COVER_BYTES: usize = 2 * 1024 * 1024;

/// Store a rendered cover for an open notebook.
///
/// Takes the notebook's absolute root, because this is called from the notebook side, where the
/// library is not necessarily what the writer is looking at — and the allowlist already vouches
/// for a root that has been opened. The bytes are checked to be a PNG rather than trusted: this
/// writes a file inside a notebook, and "the frontend said so" is not a reason to.
#[tauri::command]
pub fn write_notebook_cover(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
    png: Vec<u8>,
) -> Result<(), String> {
    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if png.len() > MAX_COVER_BYTES {
        return Err("that cover is too large".to_owned());
    }
    if !png.starts_with(&PNG_MAGIC) {
        return Err("a cover must be a PNG".to_owned());
    }
    let notebook = crate::workspace::ensure_allowed(&roots, &root)?;
    let path = internal_file(&notebook, COVER_FILE, true)?;
    fs::write(path, png).map_err(|error| error.to_string())
}

/// A notebook's cover as a data URL, or `None` when it has never been saved with one.
///
/// Fetched per tile rather than folded into the listing, so a folder of a hundred notebooks does
/// not put a hundred rasters into one IPC reply for the sake of the dozen that are on screen.
#[tauri::command]
pub fn library_cover(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    path: String,
) -> Result<Option<String>, String> {
    let root = current_root(&app, &roots)?;
    let notebook = resolve(&root, &path)?;
    if !is_notebook(&notebook) {
        return Ok(None);
    }
    let Ok(cover) = internal_file(&notebook, COVER_FILE, false) else {
        return Ok(None);
    };
    let within_ceiling = fs::metadata(&cover)
        .map(|metadata| metadata.len() as usize <= MAX_COVER_BYTES)
        .unwrap_or(false);
    if !within_ceiling {
        return Ok(None);
    }
    let Ok(bytes) = fs::read(&cover) else {
        return Ok(None);
    };
    Ok(Some(format!("data:image/png;base64,{}", base64(&bytes))))
}

/// Base64 without a dependency. The alphabet is four lines and the alternative is a crate in the
/// lockfile for one call site.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let block = chunk.iter().enumerate().fold(0u32, |block, (index, byte)| {
            block | (*byte as u32) << (16 - 8 * index)
        });
        for slot in 0..4 {
            if slot <= chunk.len() {
                out.push(ALPHABET[(block >> (18 - 6 * slot) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[tauri::command]
pub fn library_favourites(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
) -> Result<Vec<String>, String> {
    Ok(read_shelf(&current_root(&app, &roots)?).favourites)
}

#[tauri::command]
pub fn set_library_favourite(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    path: String,
    favourite: bool,
) -> Result<Vec<String>, String> {
    let root = current_root(&app, &roots)?;
    // Resolve even when un-starring: a path the library cannot vouch for has no business being
    // written into its preferences.
    resolve(&root, &path)?;
    let mut shelf = read_shelf(&root);
    shelf.favourites.retain(|existing| existing != &path);
    if favourite {
        shelf.favourites.push(path);
    }
    write_shelf(&root, &shelf)?;
    Ok(shelf.favourites)
}

/// Every favourite that still exists, as full entries ready for the shelf.
///
/// Reads each one rather than trusting the list, so a folder deleted in Explorer simply stops
/// appearing instead of becoming a tile that cannot be opened.
#[tauri::command]
pub fn list_library_favourites(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
) -> Result<Vec<LibraryEntry>, String> {
    let root = current_root(&app, &roots)?;
    let mut entries = Vec::new();
    for path in read_shelf(&root).favourites {
        let Ok(resolved) = resolve(&root, &path) else {
            continue;
        };
        if !resolved.is_dir() {
            continue;
        }
        let name = parent_of(&path);
        let name = path[if name.is_empty() { 0 } else { name.len() + 1 }..].to_owned();
        if is_notebook(&resolved) {
            let (page_count, paper) = peek_manifest(&resolved);
            entries.push(LibraryEntry::Notebook {
                name,
                path,
                modified_ms: modified_ms(&resolved.join(layout::MANIFEST)),
                page_count,
                paper,
            });
        } else {
            entries.push(LibraryEntry::Folder {
                name,
                path,
                modified_ms: modified_ms(&resolved),
                child_count: child_count(&resolved),
            });
        }
    }
    Ok(entries)
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

    /// The frontend reads these fields by name, and a serde attribute in the wrong place is
    /// invisible until a tile renders `undefined`. Pin the wire shape here instead.
    #[test]
    fn an_entry_reaches_the_frontend_in_camel_case() {
        let root = library();
        let entries = read_folder(root.path(), "Semester 3").unwrap();
        let json = serde_json::to_value(&entries).unwrap();

        assert_eq!(json[0]["kind"], "folder");
        assert_eq!(json[0]["childCount"], 0);
        assert!(json[0].get("modifiedMs").is_some(), "{json}");
        assert_eq!(json[1]["kind"], "notebook");
        assert_eq!(json[1]["pageCount"], 3);
    }

    /// Hand-rolled, so the padding cases are pinned against the RFC 4648 vectors rather than
    /// assumed. The one-and two-byte tails are where a home-made encoder goes wrong.
    #[test]
    fn base64_matches_the_reference_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // High bytes and the two characters that only appear at the top of the alphabet.
        assert_eq!(base64(&[0xff, 0xef, 0xfe]), "/+/+");
    }

    #[test]
    fn names_are_checked_against_every_platform_not_this_one() {
        for good in [
            "Semester 3",
            "Thermodynamik",
            "Serie_07",
            "Übung 2 – Kinematik",
        ] {
            assert!(validate_name(good).is_ok(), "`{good}` should be allowed");
        }
        // Linux would take most of these. They still have to be refused, because a library is
        // synced and shared and these become directory names on someone else's disk.
        for bad in [
            "",
            "a/b",
            "a:b",
            "a*b",
            "a?b",
            ".hidden",
            "trailing.",
            " leading",
            "trailing ",
            "con",
            "COM1",
            "lpt9",
        ] {
            assert!(validate_name(bad).is_err(), "`{bad}` should be refused");
        }
        assert!(validate_name(&"x".repeat(81)).is_err());
    }

    #[test]
    fn a_folder_cannot_be_moved_into_itself() {
        let root = library();
        let outer = fs::canonicalize(root.path().join("Semester 3")).unwrap();
        let inner = fs::canonicalize(root.path().join("Semester 3/Altklausuren")).unwrap();
        // The shape `move_library_entry` guards with, checked directly so the guard is pinned
        // even though the command itself needs a running Tauri app.
        assert!(inner.starts_with(&outer));
        assert!(!outer.starts_with(&inner));
    }

    #[test]
    fn library_mutations_stop_at_notebook_boundaries() {
        let root = library();
        let notebook = resolve(root.path(), "Semester 3/Thermodynamik").unwrap();
        let internal = notebook.join("blocks");
        fs::create_dir(&internal).unwrap();

        assert!(reject_notebook_internals(root.path(), &notebook).is_ok());
        assert!(reject_notebook_internals(root.path(), &internal).is_err());
        assert!(
            reject_notebook_internals(
                root.path(),
                &fs::canonicalize(root.path().join(INTERNAL_DIR)).unwrap(),
            )
            .is_err()
        );
        assert!(free_child(root.path(), "Semester 3/Thermodynamik", "intruder").is_err());
    }

    #[test]
    fn store_owned_files_refuse_symbolic_links() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("library");
        let outside = temporary.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();

        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside, root.join(INTERNAL_DIR)).is_ok();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, root.join(INTERNAL_DIR)).is_ok();

        if linked {
            assert!(internal_file(&root, SHELF_FILE, true).is_err());
            assert!(!outside.join(SHELF_FILE).exists());
        }
    }

    /// A favourite is a path, so renaming, moving or deleting the entry leaves it pointing at
    /// nothing — including every favourite *below* a renamed folder.
    #[test]
    fn favourites_below_a_moved_folder_are_forgotten_with_it() {
        let root = library();
        write_shelf(
            root.path(),
            &Shelf {
                favourites: vec![
                    "Semester 3".to_owned(),
                    "Semester 3/Thermodynamik".to_owned(),
                    "Semester 4".to_owned(),
                ],
            },
        )
        .unwrap();

        forget_favourite(root.path(), "Semester 3").unwrap();

        assert_eq!(read_shelf(root.path()).favourites, vec!["Semester 4"]);
    }

    /// A `Semester 30` must not be dropped when `Semester 3` is, which a plain prefix test would
    /// get wrong.
    #[test]
    fn forgetting_a_favourite_respects_folder_boundaries() {
        let root = library();
        write_shelf(
            root.path(),
            &Shelf {
                favourites: vec!["Semester 3".to_owned(), "Semester 30".to_owned()],
            },
        )
        .unwrap();

        forget_favourite(root.path(), "Semester 3").unwrap();

        assert_eq!(read_shelf(root.path()).favourites, vec!["Semester 30"]);
    }

    /// Preferences must never be the reason a library will not open.
    #[test]
    fn an_unparsable_shelf_reads_as_empty() {
        let root = library();
        let internal = root.path().join(INTERNAL_DIR);
        fs::create_dir_all(&internal).unwrap();
        fs::write(internal.join(SHELF_FILE), b"{ not json").unwrap();

        assert!(read_shelf(root.path()).favourites.is_empty());
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
