use std::{fs, io::Write, path::Path};

use serde_json::Value;
use tauri::Manager;

#[tauri::command]
pub fn phase0_notebook_root(app: tauri::AppHandle) -> Result<String, String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("phase0-notebook");
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    root.into_os_string()
        .into_string()
        .map_err(|_| "the notebook path is not valid Unicode".to_owned())
}

#[tauri::command]
pub fn write_phase0_metrics(root: String, metrics: Value) -> Result<(), String> {
    let root = fs::canonicalize(Path::new(&root)).map_err(|error| error.to_string())?;
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
