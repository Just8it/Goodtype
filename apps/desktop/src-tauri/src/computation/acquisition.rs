//! Fixed artifact acquisition. Wheel extraction executes no installer, pip, .pth or shell code.
use super::RuntimeState;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    sync::atomic::Ordering,
    time::Duration,
};

const MANIFEST: &str = include_str!("runtime.json");
const MAX_ARCHIVE: usize = 80 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 768 * 1024 * 1024;
#[derive(Deserialize)]
struct Manifest {
    id: String,
    archives: Vec<Archive>,
}
#[derive(Deserialize)]
struct Archive {
    name: String,
    url: String,
    sha256: String,
    target: String,
    bytes: u64,
}
fn manifest() -> Manifest {
    serde_json::from_str(MANIFEST).expect("bundled runtime manifest")
}
pub(super) fn runtime_id() -> String {
    manifest().id
}
pub(super) fn download_bytes() -> u64 {
    manifest()
        .archives
        .iter()
        .map(|archive| archive.bytes)
        .sum()
}
fn manifest_hash() -> String {
    format!("{:x}", Sha256::digest(MANIFEST.as_bytes()))
}
pub(super) fn installed(path: &Path) -> bool {
    path.join("python.exe").is_file()
        && crate::store::read_bounded(&path.join("installed.json"), 1024)
            .is_ok_and(|bytes| bytes == manifest_hash().as_bytes())
}
fn permitted(state: &RuntimeState) -> Result<(), String> {
    if state.cancelled.load(Ordering::SeqCst) {
        return Err("Python installation cancelled".into());
    }
    if !state.downloads.load(Ordering::SeqCst) {
        return Err("Python package downloads are disabled".into());
    }
    Ok(())
}
fn valid_url(url: &str) -> bool {
    (url.starts_with("https://www.python.org/ftp/python/")
        || url.starts_with("https://files.pythonhosted.org/packages/"))
        && !url.chars().any(char::is_control)
        && !url.contains('?')
        && !url.contains('#')
        && !url.contains('@')
}
pub(super) fn install(base: &Path, state: &RuntimeState) -> Result<(), String> {
    let manifest = manifest();
    let destination = base.join(&manifest.id);
    if installed(&destination) {
        return Ok(());
    }
    if destination.exists() {
        return Err("An incomplete Python installation already occupies its path".into());
    }
    let cache =
        goodtype_core::paths::ensure_dir(base, &base.join("archives"), "Python archive cache")
            .map_err(|error| error.to_string())?;
    let stage = tempfile::Builder::new()
        .prefix("install-")
        .tempdir_in(base)
        .map_err(|error| error.to_string())?;
    // ureq is built without a default TLS backend; HTTPS needs the system connector, as in the importer.
    let tls = native_tls::TlsConnector::new().map_err(|error| error.to_string())?;
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(10))
        .user_agent(concat!("goodtype/", env!("CARGO_PKG_VERSION")))
        .tls_connector(std::sync::Arc::new(tls))
        .build();
    let mut extracted = 0u64;
    let mut entries = 0;
    for archive in &manifest.archives {
        permitted(state)?;
        if !valid_url(&archive.url)
            || archive.sha256.len() != 64
            || !archive.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !matches!(archive.target.as_str(), "." | "site-packages")
        {
            return Err("Invalid bundled Python artifact manifest".into());
        }
        *state.progress.lock().map_err(|error| error.to_string())? = archive.name.clone();
        let cached = cache.join(format!("{}.zip", archive.sha256));
        if !cached.exists() {
            let response = agent
                .get(&archive.url)
                .call()
                .map_err(|error| format!("{}: {error}", archive.name))?;
            if response.status() != 200 {
                return Err("Python artifact download did not return HTTP 200".into());
            }
            let mut reader = response.into_reader();
            let mut temporary =
                tempfile::NamedTempFile::new_in(&cache).map_err(|error| error.to_string())?;
            let mut hasher = Sha256::new();
            let mut total = 0usize;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                permitted(state)?;
                let count = reader
                    .read(&mut buffer)
                    .map_err(|error| error.to_string())?;
                if count == 0 {
                    break;
                }
                total += count;
                if total > MAX_ARCHIVE || total as u64 > archive.bytes {
                    return Err("Python artifact exceeds its pinned size".into());
                }
                hasher.update(&buffer[..count]);
                temporary
                    .write_all(&buffer[..count])
                    .map_err(|error| error.to_string())?;
            }
            if total as u64 != archive.bytes || format!("{:x}", hasher.finalize()) != archive.sha256
            {
                return Err("Python artifact integrity check failed".into());
            }
            temporary
                .as_file()
                .sync_all()
                .map_err(|error| error.to_string())?;
            temporary
                .persist_noclobber(&cached)
                .map_err(|error| error.to_string())?;
        }
        let bytes = crate::store::read_bounded(&cached, MAX_ARCHIVE)?;
        if bytes.len() as u64 != archive.bytes
            || format!("{:x}", Sha256::digest(&bytes)) != archive.sha256
        {
            return Err("Cached Python artifact integrity check failed".into());
        }
        let mut bundle =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|error| error.to_string())?;
        for index in 0..bundle.len() {
            permitted(state)?;
            entries += 1;
            if entries > 30_000 {
                return Err("Python archive contains too many entries".into());
            }
            let mut entry = bundle.by_index(index).map_err(|error| error.to_string())?;
            if entry.is_dir() {
                continue;
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err("Python archive contains a symbolic link".into());
            }
            let mut name = entry.name().to_owned();
            if let Some((_, relative)) = name.split_once(".data/") {
                let Some((category, relative)) = relative.split_once('/') else {
                    return Err("Invalid wheel data path".into());
                };
                if !matches!(category, "purelib" | "platlib") {
                    continue;
                }
                name = relative.to_owned();
            }
            goodtype_core::paths::validate_relative(&name).map_err(|error| error.to_string())?;
            extracted = extracted
                .checked_add(entry.size())
                .ok_or("Python extraction size overflow")?;
            if extracted > MAX_EXTRACTED {
                return Err("Python extraction exceeds the size limit".into());
            }
            let target = stage.path().join(&archive.target).join(&name);
            let parent = target.parent().ok_or("Invalid Python archive path")?;
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&target)
                .map_err(|error| format!("{}: {error}", target.display()))?;
            let count = std::io::copy(&mut (&mut entry).take(MAX_EXTRACTED + 1), &mut file)
                .map_err(|error| error.to_string())?;
            if count != entry.size() {
                return Err("Python archive entry size mismatch".into());
            }
        }
    }
    let mut pth_written = false;
    if let Ok(entries) = fs::read_dir(stage.path()) {
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with("python") && file_name.ends_with(".zip") {
                let pth_name = file_name.replace(".zip", "._pth");
                let content = format!("{file_name}\n.\nsite-packages\n");
                fs::write(stage.path().join(pth_name), content.as_bytes())
                    .map_err(|error| error.to_string())?;
                pth_written = true;
            }
        }
    }
    if !pth_written {
        fs::write(
            stage.path().join("python314._pth"),
            b"python314.zip\n.\nsite-packages\n",
        )
        .map_err(|error| error.to_string())?;
    }
    crate::store::write_atomic(
        &stage.path().join("installed.json"),
        manifest_hash().as_bytes(),
    )?;
    permitted(state)?;
    fs::rename(stage.path(), &destination).map_err(|error| error.to_string())?;
    *state.progress.lock().map_err(|error| error.to_string())? = String::new();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_sources_and_disabled_policy_reject_before_network() {
        assert!(valid_url(
            "https://www.python.org/ftp/python/3.14.10/runtime.zip"
        ));
        for url in [
            "http://www.python.org/x",
            "https://www.python.org.evil/x",
            "https://user@files.pythonhosted.org/packages/x",
            "https://files.pythonhosted.org/packages/x?redirect=1",
        ] {
            assert!(!valid_url(url));
        }
        let state = RuntimeState::default();
        state.downloads.store(false, Ordering::SeqCst);
        let temp = tempfile::tempdir().unwrap();
        assert!(install(temp.path(), &state).is_err());
        assert!(!temp.path().join(runtime_id()).exists());
    }
}
