use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde_json::Value;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

use crate::settings::record_recent;

/// Notebook roots the user has explicitly selected in this process, plus the default local
/// notebook. Commands only operate on member roots, so the frontend can never point Rust at an
/// arbitrary directory (Pillar 4).
#[derive(Default)]
pub struct AllowedRoots(pub Arc<Mutex<HashSet<PathBuf>>>);

pub fn allow_root(roots: &AllowedRoots, root: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(root).map_err(|error| error.to_string())?;
    roots
        .0
        .lock()
        .map_err(|error| error.to_string())?
        .insert(canonical.clone());
    Ok(canonical)
}

pub fn ensure_allowed(roots: &AllowedRoots, root: &str) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(Path::new(root)).map_err(|error| error.to_string())?;
    let allowed = roots.0.lock().map_err(|error| error.to_string())?;
    if allowed.contains(&canonical) {
        Ok(canonical)
    } else {
        Err("this directory was not selected as a notebook root".to_owned())
    }
}

fn path_string(path: PathBuf) -> Result<String, String> {
    path.into_os_string()
        .into_string()
        .map_err(|_| "the notebook path is not valid Unicode".to_owned())
}

#[tauri::command]
pub fn phase0_notebook_root(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
) -> Result<String, String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("phase0-notebook");
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    path_string(allow_root(&roots, &root)?)
}

/// Let the user pick an existing notebook directory with the native dialog. The chosen
/// directory joins the allowlist only when it actually contains a notebook manifest.
#[tauri::command]
pub async fn pick_notebook_root(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
) -> Result<Option<String>, String> {
    let Some(picked) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let picked = picked.into_path().map_err(|error| error.to_string())?;
    if !picked.join("goodtype.json").is_file() {
        return Err("that folder does not contain a Goodtype notebook".to_owned());
    }
    path_string(allow_root(&roots, &picked)?).map(Some)
}

/// Re-admit a notebook chosen in an earlier session (from the recents list). The path must
/// still contain a manifest; missing notebooks stay visible in the list but cannot open.
#[tauri::command]
pub fn open_recent_root(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
) -> Result<String, String> {
    let path = Path::new(&root);
    if !path.join("goodtype.json").is_file() {
        return Err("this notebook is missing or was moved".to_owned());
    }
    path_string(allow_root(&roots, path)?)
}

/// Record a successful open in the recents list.
#[tauri::command]
pub fn record_notebook_opened(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
    title: String,
    opened_at: String,
) -> Result<(), String> {
    let canonical = ensure_allowed(&roots, &root)?;
    record_recent(&app, &path_string(canonical)?, &title, &opened_at)
}

#[tauri::command]
pub fn write_phase0_metrics(
    roots: tauri::State<'_, AllowedRoots>,
    root: String,
    metrics: Value,
) -> Result<(), String> {
    let root = ensure_allowed(&roots, &root)?;
    if !root.is_dir() {
        return Err("the notebook root is not a directory".to_owned());
    }

    let metrics_dir = root.join(".goodtype");
    fs::create_dir_all(&metrics_dir).map_err(|error| error.to_string())?;
    let metrics_dir = fs::canonicalize(metrics_dir).map_err(|error| error.to_string())?;
    if !metrics_dir.starts_with(&root) {
        return Err("the metrics directory escapes the notebook root".to_owned());
    }

    let target = metrics_dir.join("phase0-metrics.json");
    if target
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err("the metrics file cannot be a symbolic link".to_owned());
    }
    let mut bytes = serde_json::to_vec_pretty(&metrics).map_err(|error| error.to_string())?;
    if bytes.len() > 64 * 1024 {
        return Err("the metrics payload is too large".to_owned());
    }
    bytes.push(b'\n');
    let mut temporary =
        tempfile::NamedTempFile::new_in(&metrics_dir).map_err(|error| error.to_string())?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.flush())
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| error.to_string())?;
    temporary
        .persist(target)
        .map_err(|error| error.error.to_string())?;
    Ok(())
}
