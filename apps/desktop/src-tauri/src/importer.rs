//! The optional Goodnotes and OneNote importers: fetch the pinned release, install it, run it,
//! publish what it produced (ADR 0029, 0031, 0032).
//!
//! Everything about the importers lives here, behind a handful of commands. The frontend names
//! which importer it means and can ask for the status, start or cancel a download, pick a source
//! with the native dialog, and start or cancel an import into the Library folder it is showing.
//! It never sees a URL, an executable path or a process. Goodtype runs, and imported notebooks
//! open, with none of this installed. Both importers share every check below; they differ only
//! in the fixed facts of their [`Tool`].

use std::{
    collections::HashSet,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use goodtype_core::{SCHEMA_VERSION, layout, paths, storage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

use crate::{library, store::read_bounded, workspace::AllowedRoots};

mod update;

// ---------------------------------------------------------------------------------------------
// The release Goodtype trusts
// ---------------------------------------------------------------------------------------------

/// One tested importer release, pinned in Goodtype's own source. The checksum published beside
/// the release is not trusted on its own: a compromised release could replace both.
#[derive(Clone, Copy, Debug)]
pub struct PinnedRelease<'a> {
    pub version: &'a str,
    pub url: &'a str,
    pub size: u64,
    pub sha256: &'a str,
    pub decoder_commit: &'a str,
}

/// The fixed facts of one optional importer. Nothing here comes from the frontend or a download.
#[derive(Debug)]
pub struct Tool {
    /// The name the frontend uses, and the install folder's suffix.
    pub id: &'static str,
    /// The name its `--version --json` reports; the executable is this plus `.exe`.
    pub name: &'static str,
    /// `goodtype-<source>-importer`: release ZIPs are `<package>-<version>-windows-x64.zip`.
    pub package: &'static str,
    /// The GitHub repository path, `/owner/name`, exactly as its release URLs spell it.
    pub repository: &'static str,
    /// Verifies signed update metadata. Private halves live only in each repository's publisher.
    pub public_key: [u8; 32],
    /// The release this build downloads. `None` means no release is published and tested; the
    /// Library then says the importer is not available yet and nothing is ever requested.
    pub pinned: Option<PinnedRelease<'static>>,
    /// The native file dialog's filter.
    pub source_filter: (&'static str, &'static [&'static str]),
    /// `<batch> 2026-10-22` is the Library folder an import goes into.
    pub batch: &'static str,
}

impl Tool {
    fn executable(&self) -> String {
        format!("{}.exe", self.name)
    }

    /// The release path every download starts at.
    fn release_path(&self) -> String {
        format!("{}/releases/download/", self.repository)
    }

    fn release_url(&self) -> String {
        format!(
            "https://github.com{}",
            self.release_path().trim_end_matches('/')
        )
    }

    /// The fixed latest-release update descriptor, only ever read by an explicit update check.
    fn latest_descriptor(&self) -> String {
        format!(
            "https://github.com{}/releases/latest/download/importer-update.json",
            self.repository
        )
    }
}

/// v0.1.1, published 2026-10-21 from commit bb93582f (clean tree). The hash was computed from an
/// independent download and agrees with the release's `SHA256SUMS`, its descriptor JSON and
/// GitHub's asset digest. The package then installed through `install_archive`, passed its own
/// `--check`, and imported the private sample notebooks (see the opt-in test below). Re-pin only
/// after repeating that.
pub static GOODNOTES: Tool = Tool {
    id: "goodnotes",
    name: "goodtype-goodnotes-import",
    package: "goodtype-goodnotes-importer",
    repository: "/Just8it/Goodtype-Goodnodes-importer",
    // Matches that repository's release-public-key.pem.
    public_key: [
        0x42, 0x8c, 0xc6, 0x8f, 0x6d, 0x0b, 0x38, 0xea, 0xe7, 0x4d, 0xcb, 0x21, 0x1e, 0x48, 0x77,
        0xc6, 0xeb, 0x51, 0x54, 0x8d, 0xaa, 0xbf, 0x24, 0xde, 0x64, 0xa1, 0x85, 0x88, 0x5c, 0x82,
        0x5b, 0x3c,
    ],
    pinned: Some(PinnedRelease {
        version: "0.1.1",
        url: "https://github.com/Just8it/Goodtype-Goodnodes-importer/releases/download/v0.1.1/goodtype-goodnotes-importer-0.1.1-windows-x64.zip",
        size: 14_881_982,
        sha256: "ef8bfd150626eab885cf1479131e5945fcfd2a9da4152aab3a03311507db5742",
        decoder_commit: "66028455fd7fc61b7931679915d811a25485e3ad",
    }),
    source_filter: ("Goodnotes", &["goodnotes", "zip"]),
    batch: "Goodnotes Import",
};

/// v0.1.0, published 2026-10-22 from commit 9806bbd2 (clean tree). The hash was computed from an
/// independent download and agrees with the release's `SHA256SUMS`, its signed descriptor and
/// GitHub's asset digest; the package installed through `install_archive`, passed `--check` and
/// imported OneNote sample sections (see the opt-in test below). Re-pin only after repeating that.
pub static ONENOTE: Tool = Tool {
    id: "onenote",
    name: "goodtype-onenote-import",
    package: "goodtype-onenote-importer",
    repository: "/Just8it/Goodtype-OneNote-Importer",
    // Matches that repository's release-public-key.pem; distinct from the Goodnotes key.
    public_key: [
        0x73, 0xbc, 0xe6, 0x98, 0x80, 0x1a, 0x59, 0xfe, 0x2d, 0x42, 0x3f, 0xd9, 0xc6, 0xdb, 0xa8,
        0x68, 0x7f, 0xe3, 0xb9, 0x2f, 0x60, 0xbb, 0x9f, 0x27, 0x49, 0x9e, 0x72, 0x42, 0xc2, 0x33,
        0x6c, 0x3b,
    ],
    pinned: Some(PinnedRelease {
        version: "0.1.0",
        url: "https://github.com/Just8it/Goodtype-OneNote-Importer/releases/download/v0.1.0/goodtype-onenote-importer-0.1.0-windows-x64.zip",
        size: 1_927_930,
        sha256: "a09f7899eef1280819a7da0b12e1355a76e2a71c774e738b0945e07bac9a63dc",
        decoder_commit: "727389a674665dc07e0c31818cd4b54b64b9c684",
    }),
    source_filter: ("OneNote", &["one", "onetoc2", "onepkg", "zip"]),
    batch: "OneNote Import",
};

const PROTOCOL_VERSION: u64 = 1;
const PACKAGE_MANIFEST: &str = "importer-manifest.json";
const TARGET: &str = "x86_64-pc-windows-msvc";
/// The only hosts a release download may be redirected to.
const ASSET_HOSTS: [&str; 3] = [
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "github-releases.githubusercontent.com",
];

const MIB: u64 = 1024 * 1024;
/// Absolute ceilings, whatever a pin says.
const MAX_PACKAGE_BYTES: u64 = 256 * MIB;
const MAX_ARCHIVE_ENTRIES: usize = 20_000;
const MAX_UNPACKED_BYTES: u64 = 512 * MIB;
const MAX_PATH_DEPTH: usize = 24;
const MAX_JSON_BYTES: usize = 4 * MIB as usize;
const MAX_REDIRECTS: usize = 5;
const DOWNLOAD_DEADLINE: Duration = Duration::from_secs(15 * 60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const CHECK_TIMEOUT: Duration = Duration::from_secs(180);
const IMPORT_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);
/// The importer limits its own decoder to 1.5 GiB; this bounds the converter and decoder together.
const JOB_MEMORY_BYTES: usize = 4 * 1024 * 1024 * 1024;
const MAX_LISTED_WARNINGS: usize = 12;

pub fn platform_supported() -> bool {
    cfg!(all(windows, target_arch = "x86_64"))
}

// ---------------------------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------------------------

/// Splits an absolute `https://host/path` URL. Anything else — another scheme, credentials, a port,
/// a relative redirect — is refused rather than interpreted.
fn https_parts(url: &str) -> Option<(String, &str)> {
    let rest = url.strip_prefix("https://")?;
    let slash = rest.find('/')?;
    let (host, path) = rest.split_at(slash);
    if host.is_empty() || host.contains(['@', ':', '\\', '?', '#']) || path.contains('\\') {
        return None;
    }
    Some((host.to_ascii_lowercase(), path))
}

/// The first request must be the tool's own releases on github.com; redirects may also go to
/// GitHub's release-asset hosts, and nowhere else.
pub fn download_url_allowed(tool: &Tool, url: &str, redirected: bool) -> bool {
    let Some((host, path)) = https_parts(url) else {
        return false;
    };
    (host == "github.com" && path.starts_with(&tool.release_path()))
        || (redirected && ASSET_HOSTS.contains(&host.as_str()))
}

fn https_agent() -> Result<ureq::Agent, String> {
    let tls = native_tls::TlsConnector::new().map_err(|error| error.to_string())?;
    Ok(ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .user_agent(concat!("goodtype/", env!("CARGO_PKG_VERSION")))
        .tls_connector(Arc::new(tls))
        .build())
}

fn github_asset(
    tool: &Tool,
    url: &str,
    allow_update_pointer: bool,
) -> Result<ureq::Response, String> {
    let agent = https_agent()?;
    let mut url = url.to_owned();
    for redirects in 0..=MAX_REDIRECTS {
        if !(download_url_allowed(tool, &url, redirects > 0)
            || allow_update_pointer && url == tool.latest_descriptor())
        {
            return Err("refused an unexpected importer release URL".into());
        }
        let response = agent
            .get(&url)
            .call()
            .map_err(|error| format!("cannot read the GitHub release ({error})"))?;
        if !(300..400).contains(&response.status()) {
            return Ok(response);
        }
        url = response
            .header("location")
            .ok_or("GitHub redirected without a location")?
            .to_owned();
    }
    Err("GitHub redirected too many times".into())
}

/// Streams the pinned archive to `into`, hashing as it goes. Returns the SHA-256; the caller
/// compares it with the pin before anything is extracted.
fn download(
    tool: &Tool,
    release: &PinnedRelease,
    into: &Path,
    cancel: &AtomicBool,
    progress: &dyn Fn(u64, u64),
) -> Result<String, String> {
    if release.size == 0 || release.size > MAX_PACKAGE_BYTES {
        return Err("the pinned importer package size is out of range".into());
    }
    let started = Instant::now();
    let response = github_asset(tool, release.url, false)?;
    if let Some(length) = response
        .header("content-length")
        .and_then(|value| value.parse::<u64>().ok())
        && length != release.size
    {
        return Err(format!(
            "the package is {length} bytes; this Goodtype version expects {}",
            release.size
        ));
    }
    receive(
        response.into_reader(),
        release.size,
        into,
        cancel,
        started,
        progress,
    )
}

/// Writes exactly `size` bytes from `reader` to `into`, hashing them. Fails on cancellation, the
/// deadline, an interrupted stream, or a body shorter or longer than expected.
fn receive(
    reader: impl Read,
    size: u64,
    into: &Path,
    cancel: &AtomicBool,
    started: Instant,
    progress: &dyn Fn(u64, u64),
) -> Result<String, String> {
    let mut reader = reader.take(size + 1);
    let mut file = fs::File::create(into).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut received = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(CANCELLED.into());
        }
        if started.elapsed() > DOWNLOAD_DEADLINE {
            return Err("the download timed out".into());
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("the download was interrupted ({error})"))?;
        if read == 0 {
            break;
        }
        received += read as u64;
        if received > size {
            return Err("the package is larger than expected".into());
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])
            .map_err(|error| error.to_string())?;
        progress(received, size);
    }
    if received != size {
        return Err("the download ended early".into());
    }
    file.sync_all().map_err(|error| error.to_string())?;
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

const CANCELLED: &str = "cancelled";

// ---------------------------------------------------------------------------------------------
// Archive and package checks
// ---------------------------------------------------------------------------------------------

/// The package's one top-level folder for a version.
fn package_folder(tool: &Tool, version: &str) -> String {
    format!("{}-{version}-windows-x64", tool.package)
}

/// A relative path inside a package or report, as `/`-separated components that are safe on
/// every platform. Returns the components.
fn safe_components(path: &str) -> Result<Vec<&str>, String> {
    if path.is_empty() || path.starts_with('/') || path.contains(['\\', ':', '\0']) {
        return Err(format!("unsafe path `{path}`"));
    }
    let components: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    if components.len() > MAX_PATH_DEPTH {
        return Err(format!("path `{path}` is nested too deeply"));
    }
    for component in &components {
        let stem = component.split('.').next().unwrap_or(component);
        if component.is_empty()
            || *component == "."
            || *component == ".."
            || component.ends_with(['.', ' '])
            || component.chars().any(char::is_control)
            || component.contains(['*', '?', '"', '<', '>', '|'])
            || paths::is_windows_reserved(stem)
        {
            return Err(format!("unsafe path `{path}`"));
        }
    }
    Ok(components)
}

/// Unpacks `archive` into `into`, which must not exist yet. Only the expected top folder, no
/// links, no collisions; sizes are counted as bytes are written, not taken from headers.
pub fn extract_package(archive: &Path, top: &str, into: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|error| error.to_string())?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|error| format!("not a valid ZIP ({error})"))?;
    if zip.len() > MAX_ARCHIVE_ENTRIES {
        return Err("the package has too many files".into());
    }
    fs::create_dir(into).map_err(|error| error.to_string())?;
    let mut seen = HashSet::new();
    let mut written = 0_u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|error| error.to_string())?;
        if entry.is_symlink() {
            return Err(format!("the package contains a link `{}`", entry.name()));
        }
        let name = entry.name().to_owned();
        let components = safe_components(&name)?;
        if components[0] != top {
            return Err(format!("unexpected entry `{name}` outside {top}/"));
        }
        let inner = &components[1..];
        if inner.is_empty() {
            continue;
        }
        if !seen.insert(inner.join("/").to_lowercase()) {
            return Err(format!("the package lists `{name}` twice"));
        }
        let target = inner
            .iter()
            .fold(into.to_path_buf(), |path, part| path.join(part));
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|error| format!("cannot write `{name}` ({error})"))?;
        let budget = MAX_UNPACKED_BYTES - written;
        let copied = io::copy(&mut (&mut entry).take(budget + 1), &mut output)
            .map_err(|error| format!("cannot unpack `{name}` ({error})"))?;
        written += copied;
        if written > MAX_UNPACKED_BYTES {
            return Err("the package unpacks to more than 512 MiB".into());
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageManifest {
    pub version: String,
    pub protocol_version: u64,
    pub notebook_schema_version: u32,
    pub target: String,
    pub decoder_commit: String,
    pub files: Vec<PackageFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageFile {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).map_err(|error| error.to_string())?;
    Ok(hex(&hasher.finalize()))
}

/// Every file under `dir`, as `/`-separated relative paths. Links are refused outright.
fn package_files(dir: &Path) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    let mut pending = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = pending.pop() {
        for entry in fs::read_dir(&folder).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "a package file name is not Unicode".to_owned())?;
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if kind.is_symlink() {
                return Err(format!("the package contains a link `{relative}`"));
            } else if kind.is_dir() {
                pending.push((entry.path(), relative));
            } else {
                files.push(relative);
            }
        }
    }
    Ok(files)
}

/// Python's own bytecode cache. The bundled interpreter writes `__pycache__/*.pyc` beside the
/// sources it runs; those files are not in the manifest and must not be trusted either (Python can
/// prefer a `.pyc` to its `.py`), so they are deleted before every check and rebuilt from the
/// verified sources.
fn is_bytecode_cache(relative: &str) -> bool {
    relative
        .split('/')
        .any(|part| part.eq_ignore_ascii_case("__pycache__"))
        && relative.to_ascii_lowercase().ends_with(".pyc")
}

/// Checks an unpacked package against its own manifest: every listed file present with the
/// listed size and hash, nothing unlisted apart from Python bytecode caches, which are removed.
/// Returns the manifest.
pub fn verify_package(tool: &Tool, dir: &Path) -> Result<PackageManifest, String> {
    let manifest: PackageManifest =
        serde_json::from_slice(&read_bounded(&dir.join(PACKAGE_MANIFEST), MAX_JSON_BYTES)?)
            .map_err(|error| format!("the package manifest is malformed ({error})"))?;
    let mut listed = HashSet::new();
    for file in &manifest.files {
        safe_components(&file.path)?;
        if !listed.insert(file.path.to_lowercase()) {
            return Err(format!("the manifest lists `{}` twice", file.path));
        }
        let path = file
            .path
            .split('/')
            .fold(dir.to_path_buf(), |path, part| path.join(part));
        let size = fs::metadata(&path)
            .map_err(|_| format!("`{}` is missing from the package", file.path))?
            .len();
        if size != file.size_bytes || !sha256_file(&path)?.eq_ignore_ascii_case(&file.sha256) {
            return Err(format!(
                "`{}` does not match the package manifest",
                file.path
            ));
        }
    }
    for file in package_files(dir)? {
        if file == PACKAGE_MANIFEST || listed.contains(&file.to_lowercase()) {
            continue;
        }
        if is_bytecode_cache(&file) {
            let path = file
                .split('/')
                .fold(dir.to_path_buf(), |path, part| path.join(part));
            fs::remove_file(&path).map_err(|error| format!("cannot clear `{file}` ({error})"))?;
            continue;
        }
        return Err(format!("`{file}` is not listed in the package manifest"));
    }
    if !listed.contains(&tool.executable()) {
        return Err("the package has no importer executable".into());
    }
    Ok(manifest)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolVersion {
    pub name: String,
    pub version: String,
    pub protocol_version: u64,
    pub notebook_schema_version: u32,
    pub decoder_commit: String,
}

/// Whether a package (and its own version report) is one this Goodtype can drive. With a pin,
/// the version and decoder must be exactly the pinned ones.
pub fn check_contract(
    expected: &Tool,
    manifest: &PackageManifest,
    tool: Option<&ToolVersion>,
    pin: Option<&PinnedRelease>,
) -> Result<(), String> {
    if manifest.protocol_version != PROTOCOL_VERSION
        || manifest.notebook_schema_version != SCHEMA_VERSION
        || manifest.target != TARGET
    {
        return Err(format!(
            "importer {} speaks protocol {} for notebook schema {} on {}; this Goodtype needs \
             protocol {PROTOCOL_VERSION}, schema {SCHEMA_VERSION} on {TARGET}",
            manifest.version,
            manifest.protocol_version,
            manifest.notebook_schema_version,
            manifest.target
        ));
    }
    if let Some(tool) = tool
        && (tool.name != expected.name
            || tool.version != manifest.version
            || tool.protocol_version != manifest.protocol_version
            || tool.notebook_schema_version != manifest.notebook_schema_version
            || tool.decoder_commit != manifest.decoder_commit)
    {
        return Err("the importer reports a different version than its package".into());
    }
    if let Some(pin) = pin
        && (manifest.version != pin.version || manifest.decoder_commit != pin.decoder_commit)
    {
        return Err(format!(
            "importer {} is not the tested version {}",
            manifest.version, pin.version
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Installation
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
struct Active {
    version: String,
}

/// `<app-local-data>/<tool>-importer`: `versions/<version>/`, `active.json`, and transient
/// `staging-*` / `download-*.part` entries that a failed install leaves nothing else of.
pub struct InstallDir(pub PathBuf, pub &'static Tool);

impl InstallDir {
    fn versions(&self) -> PathBuf {
        self.0.join("versions")
    }

    fn version_dir(&self, version: &str) -> Result<PathBuf, String> {
        safe_components(version)?;
        if version.contains('/') {
            return Err("invalid importer version".into());
        }
        Ok(self.versions().join(version))
    }

    /// The installed version that `active.json` names, if its folder still holds the tool.
    pub fn active(&self) -> Option<(String, PathBuf)> {
        let bytes = read_bounded(&self.0.join("active.json"), 4096).ok()?;
        let active: Active = serde_json::from_slice(&bytes).ok()?;
        let dir = self.version_dir(&active.version).ok()?;
        dir.join(self.1.executable())
            .is_file()
            .then_some((active.version, dir))
    }

    fn activate(&self, version: &str) -> Result<(), String> {
        let bytes = serde_json::to_vec(&Active {
            version: version.to_owned(),
        })
        .map_err(|error| error.to_string())?;
        crate::store::write_atomic(&self.0.join("active.json"), &bytes)
    }

    fn scratch(&self, prefix: &str) -> PathBuf {
        self.0.join(format!("{prefix}{}", unique_suffix()))
    }
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{:x}-{:x}", std::process::id(), nanos)
}

/// Installs an archive that has already been downloaded to `archive`. Verifies the pinned hash,
/// unpacks, checks the manifest and the tool's own contract, then switches `active.json`. Any
/// failure removes what this attempt created and leaves the previous version active.
pub fn install_archive(
    dir: &InstallDir,
    archive: &Path,
    release: &PinnedRelease,
    probe: &dyn Fn(&Path) -> Result<ToolVersion, String>,
) -> Result<String, String> {
    let hash = sha256_file(archive)?;
    if !hash.eq_ignore_ascii_case(release.sha256) {
        return Err("the downloaded package does not match the tested release".into());
    }
    fs::create_dir_all(dir.versions()).map_err(|error| error.to_string())?;
    let staging = dir.scratch("staging-");
    let mut retired = None;
    let mut installed_target = None;
    let result = (|| {
        extract_package(archive, &package_folder(dir.1, release.version), &staging)?;
        let manifest = verify_package(dir.1, &staging)?;
        let tool = probe(&staging)?;
        check_contract(dir.1, &manifest, Some(&tool), Some(release))?;
        let target = dir.version_dir(release.version)?;
        if target.exists() {
            // Same version reinstalled: the verified copy replaces the old one.
            let previous = dir.scratch("retired-");
            fs::rename(&target, &previous).map_err(|error| error.to_string())?;
            retired = Some(previous);
        }
        fs::rename(&staging, &target).map_err(|error| error.to_string())?;
        installed_target = Some(target);
        dir.activate(release.version)?;
        Ok(release.version.to_owned())
    })();
    if result.is_err() {
        if let Some(target) = installed_target {
            let _ = fs::remove_dir_all(target);
        }
        if let Some(previous) = retired {
            fs::rename(previous, dir.version_dir(release.version)?)
                .map_err(|error| format!("could not restore the previous importer ({error})"))?;
        }
    } else {
        if let Some(previous) = retired {
            let _ = fs::remove_dir_all(previous);
        }
        prune_versions(dir, release.version);
    }
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

/// Only installer-owned version directories are cleanup targets, after successful activation.
fn prune_versions(dir: &InstallDir, active: &str) {
    let Ok(root) = fs::canonicalize(dir.versions()) else {
        return;
    };
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() == active
            || !entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
        {
            continue;
        }
        let Ok(path) = fs::canonicalize(entry.path()) else {
            continue;
        };
        if path.starts_with(&root) && path != root {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// Asks the installed tool for its contract and runs its own decoder check.
fn probe_tool(tool: &Tool, dir: &Path) -> Result<ToolVersion, String> {
    let executable = dir.join(tool.executable());
    let output = run_contained(
        Command::new(&executable)
            .args(["--version", "--json"])
            .current_dir(dir),
        PROBE_TIMEOUT,
        &AtomicBool::new(false),
        &|| {},
    )?;
    if !output.success {
        return Err(format!(
            "the importer did not start ({})",
            output.stderr.trim()
        ));
    }
    let tool: ToolVersion = serde_json::from_str(output.stdout.trim())
        .map_err(|error| format!("the importer's version report is malformed ({error})"))?;
    let check = run_contained(
        Command::new(&executable).arg("--check").current_dir(dir),
        CHECK_TIMEOUT,
        &AtomicBool::new(false),
        &|| {},
    )?;
    if !check.success {
        return Err(format!(
            "the importer's self-check failed ({})",
            check.stderr.trim()
        ));
    }
    Ok(tool)
}

// ---------------------------------------------------------------------------------------------
// Running the tool
// ---------------------------------------------------------------------------------------------

struct Finished {
    success: bool,
    cancelled: bool,
    stdout: String,
    stderr: String,
}

/// Keeps the last part of a stream; output is for diagnostics only and never parsed for progress.
fn tail_reader(mut stream: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        const KEEP: usize = 16 * 1024;
        let mut kept = Vec::new();
        let mut buffer = [0_u8; 4096];
        while let Ok(read) = stream.read(&mut buffer) {
            if read == 0 {
                break;
            }
            kept.extend_from_slice(&buffer[..read]);
            if kept.len() > 2 * KEEP {
                kept.drain(..kept.len() - KEEP);
            }
        }
        String::from_utf8_lossy(&kept).into_owned()
    })
}

/// Runs the fixed tool without a shell or console window, inside a Job Object that ends the whole
/// process tree when it is cancelled, times out or Goodtype exits. `tick` runs while waiting.
fn run_contained(
    command: &mut Command,
    timeout: Duration,
    cancel: &AtomicBool,
    tick: &dyn Fn(),
) -> Result<Finished, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(windows)]
    let job = job::Job::new(JOB_MEMORY_BYTES).map_err(|error| error.to_string())?;
    let mut child = command
        .spawn()
        .map_err(|error| format!("the importer could not start ({error})"))?;
    #[cfg(windows)]
    if let Err(error) = job.assign(&child) {
        let _ = child.kill();
        return Err(format!("the importer could not be contained ({error})"));
    }
    let stdout = tail_reader(child.stdout.take().expect("piped"));
    let stderr = tail_reader(child.stderr.take().expect("piped"));
    let started = Instant::now();
    let mut cancelled = false;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if cancel.load(Ordering::SeqCst) || started.elapsed() > timeout {
            cancelled = cancel.load(Ordering::SeqCst);
            #[cfg(windows)]
            job.terminate();
            let _ = child.kill();
            break child.wait().map_err(|error| error.to_string())?;
        }
        tick();
        std::thread::sleep(Duration::from_millis(250));
    };
    let timed_out = !cancelled && !status.success() && started.elapsed() > timeout;
    let mut stderr = stderr.join().unwrap_or_default();
    if timed_out {
        stderr.push_str("\nthe importer ran past its time limit");
    }
    Ok(Finished {
        success: status.success(),
        cancelled,
        stdout: stdout.join().unwrap_or_default(),
        stderr,
    })
}

#[cfg(windows)]
mod job {
    //! A Job Object with kill-on-close and a memory ceiling. Closing the handle (cancel, or this
    //! process exiting) ends every process in it. Lifetime and resource containment only — the
    //! tool still runs with the user's file-system rights.

    use std::{io, os::windows::io::AsRawHandle, process::Child, ptr};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_JOB_MEMORY,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
        },
    };

    pub struct Job(HANDLE);

    // SAFETY: a job handle is a kernel handle, usable from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn new(memory: usize) -> io::Result<Self> {
            // SAFETY: plain Win32 calls with owned, initialised arguments.
            unsafe {
                let handle = CreateJobObjectW(ptr::null(), ptr::null());
                if handle.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let job = Self(handle);
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags =
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_JOB_MEMORY;
                limits.JobMemoryLimit = memory;
                if SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) == 0
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(job)
            }
        }

        pub fn assign(&self, child: &Child) -> io::Result<()> {
            // SAFETY: the child's handle is valid while `child` is alive.
            if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub fn terminate(&self) {
            // SAFETY: valid job handle owned by `self`.
            unsafe { TerminateJobObject(self.0, 1) };
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: closing the handle we created; kill-on-close ends anything still running.
            unsafe { CloseHandle(self.0) };
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Reports and publication
// ---------------------------------------------------------------------------------------------

/// One line of the tool's `import-report.json`.
#[derive(Clone, Debug, Deserialize)]
pub struct ReportEntry {
    pub status: String,
    pub source: String,
    pub target: Option<String>,
    #[serde(default)]
    pub pages: usize,
    pub error: Option<String>,
}

pub fn read_report(path: &Path) -> Result<Vec<ReportEntry>, String> {
    let entries: Vec<ReportEntry> = serde_json::from_slice(&read_bounded(path, MAX_JSON_BYTES)?)
        .map_err(|error| format!("the import report is malformed ({error})"))?;
    if entries.len() > 500 {
        return Err("the import report lists too many notebooks".into());
    }
    Ok(entries)
}

/// What became of one notebook from the source.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotebookOutcome {
    /// Name or folder path inside the Goodnotes export.
    pub source: String,
    /// `imported`, `failed`, `needsArtwork`, `cancelled` or `alreadyThere`.
    pub status: &'static str,
    /// Library-relative path of the imported notebook.
    pub path: Option<String>,
    pub pages: usize,
    pub error: Option<String>,
    /// Page-level notes from the importer, e.g. a feature it could not carry over.
    pub warnings: Vec<String>,
    /// Pages whose content became vector artwork with reduced editability.
    pub artwork_pages: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    /// Library-relative batch folder, if anything was imported into it.
    pub batch: Option<String>,
    pub notebooks: Vec<NotebookOutcome>,
    pub cancelled: bool,
    /// Process or report problem not tied to one notebook.
    pub problem: Option<String>,
}

/// A notebook folder the importer says it completed must still be an ordinary, valid notebook:
/// no interrupted transaction (opening would "recover" it, i.e. write), manifest and every page
/// readable through the same core the app uses.
pub fn validate_notebook(dir: &Path) -> Result<(), String> {
    if dir.join(layout::PENDING_TRANSACTION).exists()
        || dir.join(layout::LEGACY_PENDING_TRANSACTION).exists()
    {
        return Err("the notebook has an unfinished transaction".into());
    }
    let snapshot = storage::open_notebook(dir).map_err(|error| error.to_string())?;
    for page in &snapshot.manifest.pages {
        storage::open_page(dir, &page.id).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[derive(Deserialize)]
struct NotebookReport {
    #[serde(default)]
    warnings: Vec<String>,
    #[serde(default)]
    pages: Vec<PageReport>,
}

#[derive(Deserialize)]
struct PageReport {
    page: usize,
    #[serde(default)]
    derived: Vec<serde_json::Value>,
    #[serde(default)]
    warnings: Vec<String>,
}

/// Bounded page notes from a notebook's own report, for the result list.
fn notebook_notes(dir: &Path) -> (Vec<String>, usize) {
    let Ok(bytes) = read_bounded(&dir.join("import-report.json"), MAX_JSON_BYTES) else {
        return (Vec::new(), 0);
    };
    let Ok(report) = serde_json::from_slice::<NotebookReport>(&bytes) else {
        return (Vec::new(), 0);
    };
    let mut notes: Vec<String> = report.warnings;
    for page in &report.pages {
        notes.extend(
            page.warnings
                .iter()
                .map(|warning| format!("Page {}: {warning}", page.page)),
        );
    }
    notes.truncate(MAX_LISTED_WARNINGS);
    let artwork = report
        .pages
        .iter()
        .filter(|page| !page.derived.is_empty())
        .count();
    (notes, artwork)
}

fn needs_artwork(error: &str) -> bool {
    error.contains("--allow-derived")
}

/// Moves validated, completed notebooks from the tool's batch into the Library batch folder.
/// `library_batch` is `(absolute folder, library-relative path)`; it is created on the first
/// published notebook. Never replaces anything already there.
pub fn publish(
    entries: &[ReportEntry],
    tool_batch: &Path,
    library_batch: (&Path, &str),
    cancelled: bool,
) -> Vec<NotebookOutcome> {
    let (folder, relative_folder) = library_batch;
    entries
        .iter()
        .map(|entry| {
            let outcome = |status, error: Option<String>| NotebookOutcome {
                source: entry.source.clone(),
                status,
                path: None,
                pages: entry.pages,
                error,
                warnings: Vec::new(),
                artwork_pages: 0,
            };
            match entry.status.as_str() {
                "completed" => {}
                "failed" => {
                    let error = entry.error.clone().unwrap_or_default();
                    let status = if needs_artwork(&error) {
                        "needsArtwork"
                    } else {
                        "failed"
                    };
                    return outcome(status, Some(error));
                }
                _ => {
                    return outcome(
                        if cancelled { "cancelled" } else { "failed" },
                        (!cancelled).then(|| "the importer stopped before this notebook".into()),
                    );
                }
            }
            let Some(target) = entry.target.as_deref() else {
                return outcome("failed", Some("the report names no folder".into()));
            };
            let components = match safe_components(target) {
                Ok(components) => components,
                Err(error) => return outcome("failed", Some(error)),
            };
            let produced = components
                .iter()
                .fold(tool_batch.to_path_buf(), |path, part| path.join(part));
            let destination = components
                .iter()
                .fold(folder.to_path_buf(), |path, part| path.join(part));
            if destination.exists() {
                let mut kept = outcome("alreadyThere", None);
                kept.path = Some(format!("{relative_folder}/{}", components.join("/")));
                return kept;
            }
            if let Err(error) = validate_notebook(&produced) {
                return outcome(
                    "failed",
                    Some(format!("Goodtype could not open the result: {error}")),
                );
            }
            let moved = destination
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| fs::rename(&produced, &destination));
            if let Err(error) = moved {
                return outcome(
                    "failed",
                    Some(format!("could not place the notebook: {error}")),
                );
            }
            let (warnings, artwork_pages) = notebook_notes(&destination);
            NotebookOutcome {
                source: entry.source.clone(),
                status: "imported",
                path: Some(format!("{relative_folder}/{}", components.join("/"))),
                pages: entry.pages,
                error: None,
                warnings,
                artwork_pages,
            }
        })
        .collect()
}

/// `Goodnotes Import 2026-10-21`, or the first free ` (2)`, ` (3)`… beside it.
fn batch_name(tool: &Tool, parent: &Path, today: &str) -> Result<String, String> {
    let base = format!("{} {today}", tool.batch);
    (1..1000)
        .map(|number| {
            if number == 1 {
                base.clone()
            } else {
                format!("{base} ({number})")
            }
        })
        .find(|name| !parent.join(name).exists())
        .ok_or_else(|| "no free folder name for the import".into())
}

/// Today's date (UTC) as `YYYY-MM-DD`, without a calendar dependency.
fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

// ---------------------------------------------------------------------------------------------
// State and commands
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Activity {
    Idle,
    Checking,
    #[serde(rename_all = "camelCase")]
    Downloading {
        received: u64,
        total: u64,
    },
    Installing,
    #[serde(rename_all = "camelCase")]
    InstallFailed {
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    Importing {
        done: usize,
        total: usize,
        cancelling: bool,
    },
    Publishing,
    #[serde(rename_all = "camelCase")]
    Finished {
        result: ImportResult,
    },
    #[serde(rename_all = "camelCase")]
    ImportFailed {
        message: String,
    },
}

impl Activity {
    fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Checking
                | Self::Downloading { .. }
                | Self::Installing
                | Self::Importing { .. }
                | Self::Publishing
        )
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Default)]
struct State {
    activity: Option<Activity>,
    source: Option<PathBuf>,
    update: Option<update::VerifiedUpdate>,
    update_checked: bool,
    update_problem: Option<String>,
}

/// One importer's job. One download or import at a time per importer.
#[derive(Default)]
pub struct Importer {
    state: Arc<Mutex<State>>,
    cancel: Arc<AtomicBool>,
}

impl Importer {
    fn set(&self, activity: Activity) {
        self.state.lock().unwrap().activity = Some(activity);
    }

    fn busy(&self) -> bool {
        self.state
            .lock()
            .unwrap()
            .activity
            .as_ref()
            .is_some_and(Activity::is_busy)
    }

    fn begin(&self, activity: Activity) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if state.activity.as_ref().is_some_and(Activity::is_busy) {
            return Err("the importer is busy".into());
        }
        self.cancel.store(false, Ordering::SeqCst);
        state.activity = Some(activity);
        Ok(())
    }
}

/// Managed by Tauri: each importer keeps its own job, chosen source and update state.
#[derive(Default)]
pub struct Importers {
    goodnotes: Importer,
    onenote: Importer,
}

impl Importers {
    /// The importer the frontend named. Only the fixed ids exist.
    fn get(&self, id: &str) -> Result<(&Importer, &'static Tool), String> {
        match id {
            "goodnotes" => Ok((&self.goodnotes, &GOODNOTES)),
            "onenote" => Ok((&self.onenote, &ONENOTE)),
            _ => Err("unknown importer".into()),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseView {
    pub version: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImporterStatus {
    /// Windows x64; elsewhere the importer does not exist.
    pub platform_supported: bool,
    /// The release this build would download, or none while no release is published and tested.
    pub release: Option<ReleaseView>,
    /// Installed version, when its files still match its manifest and contract.
    pub installed: Option<String>,
    /// Why an installed copy cannot be used, if it cannot.
    pub installed_problem: Option<String>,
    pub source: Option<SourceView>,
    pub activity: Activity,
    pub update: Option<ReleaseView>,
    pub update_checked: bool,
    pub update_problem: Option<String>,
}

fn install_dir(app: &tauri::AppHandle, tool: &'static Tool) -> Result<InstallDir, String> {
    let base = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?;
    let dir = base.join(format!("{}-importer", tool.id));
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(InstallDir(dir, tool))
}

/// The installed copy if it is usable: files match its manifest, and its contract fits. Hashes
/// every file. The app runs the same check in the import thread right before a launch.
#[cfg(test)]
fn usable_installation(dir: &InstallDir) -> Result<Option<(String, PathBuf)>, String> {
    let Some((version, path)) = dir.active() else {
        return Ok(None);
    };
    let manifest = verify_package(dir.1, &path)?;
    check_contract(dir.1, &manifest, None, None)?;
    Ok(Some((version, path)))
}

/// The cheap half of [`usable_installation`] for status: the manifest's contract, no hashing.
fn installed_contract(dir: &InstallDir) -> Result<Option<(String, PathBuf)>, String> {
    let Some((version, path)) = dir.active() else {
        return Ok(None);
    };
    let manifest: PackageManifest =
        serde_json::from_slice(&read_bounded(&path.join(PACKAGE_MANIFEST), MAX_JSON_BYTES)?)
            .map_err(|error| format!("the installed importer's manifest is malformed ({error})"))?;
    check_contract(dir.1, &manifest, None, None)?;
    Ok(Some((version, path)))
}

fn source_view(path: &Path) -> Option<SourceView> {
    Some(SourceView {
        name: path.file_name()?.to_string_lossy().into_owned(),
        size_bytes: fs::metadata(path).ok()?.len(),
    })
}

#[tauri::command]
pub async fn importer_status(
    app: tauri::AppHandle,
    importers: tauri::State<'_, Importers>,
    tool: String,
) -> Result<ImporterStatus, String> {
    let (importer, tool) = importers.get(&tool)?;
    let (activity, source, update, update_checked, update_problem) = {
        let state = importer.state.lock().unwrap();
        (
            state.activity.clone(),
            state.source.clone(),
            state.update.as_ref().map(|release| release.view()),
            state.update_checked,
            state.update_problem.clone(),
        )
    };
    let (installed, installed_problem) = if platform_supported() {
        match installed_contract(&install_dir(&app, tool)?) {
            Ok(found) => (found.map(|(version, _)| version), None),
            Err(problem) => (None, Some(problem)),
        }
    } else {
        (None, None)
    };
    Ok(ImporterStatus {
        platform_supported: platform_supported(),
        release: tool.pinned.map(|pin| ReleaseView {
            version: pin.version.into(),
            size_bytes: pin.size,
        }),
        installed,
        installed_problem,
        source: source.as_deref().and_then(source_view),
        activity: activity.unwrap_or(Activity::Idle),
        update,
        update_checked,
        update_problem,
    })
}

/// Downloads and installs the pinned release. Only ever called from the user's explicit action.
#[tauri::command]
pub fn importer_install(
    app: tauri::AppHandle,
    importers: tauri::State<'_, Importers>,
    tool: String,
) -> Result<(), String> {
    let (importer, tool) = importers.get(&tool)?;
    if !platform_supported() {
        return Err("the importer is available for Windows x64 only".into());
    }
    let Some(release) = tool.pinned else {
        return Err("no tested importer release is available yet".into());
    };
    if importer.busy() {
        return Err("the importer is busy".into());
    }
    let dir = install_dir(&app, tool)?;
    importer.begin(Activity::Downloading {
        received: 0,
        total: release.size,
    })?;
    let state = importer.state.clone();
    let cancel = importer.cancel.clone();
    std::thread::spawn(move || {
        let set = |activity| state.lock().unwrap().activity = Some(activity);
        let archive = dir.scratch("download-").with_extension("part");
        let result = download(tool, &release, &archive, &cancel, &|received, total| {
            set(Activity::Downloading { received, total });
        })
        .and_then(|_| {
            set(Activity::Installing);
            install_archive(&dir, &archive, &release, &|path| probe_tool(tool, path))
        });
        let _ = fs::remove_file(&archive);
        set(match result {
            Ok(_) => Activity::Idle,
            Err(message) => Activity::InstallFailed { message },
        });
    });
    Ok(())
}

#[tauri::command]
pub fn importer_check_update(
    app: tauri::AppHandle,
    importers: tauri::State<'_, Importers>,
    tool: String,
) -> Result<(), String> {
    let (importer, tool) = importers.get(&tool)?;
    if !platform_supported() || importer.busy() {
        return Err("the importer cannot check for updates right now".into());
    }
    let dir = install_dir(&app, tool)?;
    let installed = installed_contract(&dir)?.map(|(version, _)| version);
    importer.begin(Activity::Checking)?;
    let state = importer.state.clone();
    std::thread::spawn(move || {
        let result = update::check(tool, installed.as_deref());
        let mut state = state.lock().unwrap();
        state.update_checked = true;
        state.update = None;
        state.update_problem = None;
        match result {
            Ok(release) => state.update = release,
            Err(message) => state.update_problem = Some(message),
        }
        state.activity = Some(Activity::Idle);
    });
    Ok(())
}

#[tauri::command]
pub fn importer_update(
    app: tauri::AppHandle,
    importers: tauri::State<'_, Importers>,
    tool: String,
) -> Result<(), String> {
    let (importer, tool) = importers.get(&tool)?;
    if !platform_supported() || importer.busy() {
        return Err("the importer cannot update right now".into());
    }
    let dir = install_dir(&app, tool)?;
    let candidate = importer
        .state
        .lock()
        .unwrap()
        .update
        .clone()
        .ok_or("check for an importer update first")?;
    if let Some((installed, _)) = installed_contract(&dir)? {
        update::require_newer(&candidate.version, &installed)?;
    }
    importer.begin(Activity::Downloading {
        received: 0,
        total: candidate.size,
    })?;
    let state = importer.state.clone();
    let cancel = importer.cancel.clone();
    std::thread::spawn(move || {
        let set = |activity| state.lock().unwrap().activity = Some(activity);
        let archive = dir.scratch("download-").with_extension("part");
        let release = candidate.pin();
        let result = download(tool, &release, &archive, &cancel, &|received, total| {
            set(Activity::Downloading { received, total })
        })
        .and_then(|_| {
            set(Activity::Installing);
            install_archive(&dir, &archive, &release, &|path| probe_tool(tool, path))
        });
        let _ = fs::remove_file(&archive);
        let mut state = state.lock().unwrap();
        state.activity = Some(match result {
            Ok(_) => {
                state.update = None;
                Activity::Idle
            }
            Err(message) => Activity::InstallFailed { message },
        });
    });
    Ok(())
}

/// Stops a download or an import. An import keeps every notebook it had finished.
#[tauri::command]
pub fn importer_cancel(importers: tauri::State<'_, Importers>, tool: String) -> Result<(), String> {
    let (importer, _) = importers.get(&tool)?;
    importer.cancel.store(true, Ordering::SeqCst);
    let mut state = importer.state.lock().unwrap();
    if let Some(Activity::Importing { cancelling, .. }) = state.activity.as_mut() {
        *cancelling = true;
    }
    Ok(())
}

/// Clears a finished or failed result so the dialog can start over.
#[tauri::command]
pub fn importer_dismiss(
    importers: tauri::State<'_, Importers>,
    tool: String,
) -> Result<(), String> {
    let (importer, _) = importers.get(&tool)?;
    if !importer.busy() {
        importer.state.lock().unwrap().activity = Some(Activity::Idle);
    }
    Ok(())
}

/// Native file picker for the importer's source files. The path stays here.
#[tauri::command]
pub async fn import_pick_source(
    app: tauri::AppHandle,
    importers: tauri::State<'_, Importers>,
    tool: String,
    ui_language: Option<String>,
) -> Result<Option<SourceView>, String> {
    let (importer, tool) = importers.get(&tool)?;
    let (filter, extensions) = tool.source_filter;
    let Some(picked) = app
        .dialog()
        .file()
        .set_title(crate::settings::dialog_label(
            ui_language.as_deref(),
            "Choose source file",
            "Quelldatei auswählen",
        ))
        .add_filter(filter, extensions)
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|error| error.to_string())?;
    let accepted = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(extension))
        });
    if !path.is_file() || !accepted {
        return Err(format!(
            "choose a {filter} file ({})",
            extensions
                .iter()
                .map(|extension| format!(".{extension}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let view = source_view(&path).ok_or("that file cannot be read")?;
    importer.state.lock().unwrap().source = Some(path);
    Ok(Some(view))
}

/// Imports the picked source into a new batch folder inside the Library folder `parent`, or,
/// with `into`, adds what is missing to an earlier batch (a retry with vector artwork allowed).
#[tauri::command]
pub fn import_start(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    importers: tauri::State<'_, Importers>,
    tool: String,
    parent: String,
    allow_derived: bool,
    into: Option<String>,
) -> Result<(), String> {
    let (importer, tool) = importers.get(&tool)?;
    if importer.busy() {
        return Err("the importer is busy".into());
    }
    let Some((_, installed)) = installed_contract(&install_dir(&app, tool)?)? else {
        return Err("the importer is not installed".into());
    };
    let source = importer
        .state
        .lock()
        .unwrap()
        .source
        .clone()
        .ok_or("choose a file first")?;
    let root = library::current_root(&app, &roots)?;
    let parent_dir = library::resolve(&root, &parent)?;
    if library::is_notebook(&parent_dir) {
        return Err("choose a Library folder, not a notebook".into());
    }
    library::reject_notebook_internals(&root, &parent_dir)?;
    let (batch_dir, batch_relative) = match into {
        Some(existing) => {
            let folder = library::resolve(&root, &existing)?;
            if !folder.is_dir() || library::is_notebook(&folder) {
                return Err("the earlier import folder is gone".into());
            }
            library::reject_notebook_internals(&root, &folder)?;
            (folder, existing)
        }
        None => {
            let name = batch_name(tool, &parent_dir, &today())?;
            library::validate_name(&name)?;
            let relative = if parent.is_empty() {
                name.clone()
            } else {
                format!("{parent}/{name}")
            };
            (parent_dir.join(&name), relative)
        }
    };
    // Hidden from the Library listing (dot name) and on the destination volume, so publication is
    // a rename. The tool creates its own scratch folder beside its batch, i.e. inside this one.
    importer.begin(Activity::Importing {
        done: 0,
        total: 0,
        cancelling: false,
    })?;
    let staging = parent_dir.join(format!(".goodtype-import-{}", unique_suffix()));
    if let Err(error) = fs::create_dir(&staging) {
        importer.set(Activity::Idle);
        return Err(error.to_string());
    }
    let state = importer.state.clone();
    let cancel = importer.cancel.clone();
    std::thread::spawn(move || {
        let result = run_import(
            tool,
            &installed,
            &source,
            &staging,
            (&batch_dir, &batch_relative),
            allow_derived,
            &cancel,
            &state,
        );
        let _ = fs::remove_dir_all(&staging);
        state.lock().unwrap().activity = Some(match result {
            Ok(result) => Activity::Finished { result },
            Err(message) => Activity::ImportFailed { message },
        });
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_import(
    tool: &Tool,
    installed: &Path,
    source: &Path,
    staging: &Path,
    library_batch: (&Path, &str),
    allow_derived: bool,
    cancel: &AtomicBool,
    state: &Mutex<State>,
) -> Result<ImportResult, String> {
    // The files on disk are checked again right before they run.
    verify_package(tool, installed).map_err(|error| {
        format!("the installed importer is damaged ({error}); download it again")
    })?;
    let tool_batch = staging.join("batch");
    let report = tool_batch.join("import-report.json");
    let mut command = Command::new(installed.join(tool.executable()));
    command.arg(source).arg(&tool_batch).current_dir(staging);
    if allow_derived {
        command.arg("--allow-derived");
    }
    let finished = run_contained(&mut command, IMPORT_TIMEOUT, cancel, &|| {
        if let Ok(entries) = read_report(&report) {
            let done = entries
                .iter()
                .filter(|entry| entry.status != "pending")
                .count();
            if let Some(Activity::Importing {
                done: shown, total, ..
            }) = state.lock().unwrap().activity.as_mut()
            {
                *shown = done;
                *total = entries.len();
            }
        }
    })?;
    state.lock().unwrap().activity = Some(Activity::Publishing);
    let entries = match read_report(&report) {
        Ok(entries) => entries,
        Err(_) if finished.cancelled => Vec::new(),
        Err(error) => {
            let detail = finished.stderr.trim();
            return Err(if detail.is_empty() {
                format!("the importer produced no usable report ({error})")
            } else {
                detail.lines().last().unwrap_or(detail).to_owned()
            });
        }
    };
    let created = !library_batch.0.exists();
    let notebooks = publish(&entries, &tool_batch, library_batch, finished.cancelled);
    let published = notebooks
        .iter()
        .any(|notebook| notebook.status == "imported");
    if created && !published && library_batch.0.exists() {
        let _ = fs::remove_dir(library_batch.0);
    }
    let problem = (!finished.success
        && !finished.cancelled
        && entries.iter().all(|entry| entry.status != "failed"))
    .then(|| {
        finished
            .stderr
            .trim()
            .lines()
            .last()
            .unwrap_or("the importer stopped unexpectedly")
            .to_owned()
    });
    Ok(ImportResult {
        batch: library_batch.0.exists().then(|| library_batch.1.to_owned()),
        notebooks,
        cancelled: finished.cancelled,
        problem,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zip::write::SimpleFileOptions;

    const VERSION: &str = "9.9.9";
    /// Every importer must pass every check below; they share one code path.
    const TOOLS: [&Tool; 2] = [&GOODNOTES, &ONENOTE];

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            if name.ends_with('/') {
                writer
                    .add_directory(*name, SimpleFileOptions::default())
                    .unwrap();
            } else {
                writer
                    .start_file(*name, SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(bytes).unwrap();
            }
        }
        writer.finish().unwrap().into_inner()
    }

    fn sha(bytes: &[u8]) -> String {
        hex(&Sha256::digest(bytes))
    }

    /// A small but complete package: the executable stand-in, a runtime file, and a manifest
    /// listing both with their hashes.
    fn package(tool: &Tool, version: &str, tamper: bool, extra: Option<(&str, &[u8])>) -> Vec<u8> {
        let top = package_folder(tool, version);
        let executable = tool.executable();
        let exe: &[u8] = b"not really an executable";
        let runtime: &[u8] = b"print('decoder')";
        let manifest = serde_json::json!({
            "version": version, "protocolVersion": 1, "notebookSchemaVersion": SCHEMA_VERSION,
            "target": TARGET, "decoderCommit": "abc",
            "files": [
                {"path": executable, "sizeBytes": exe.len(), "sha256": sha(exe)},
                {"path": "runtime/decode.py", "sizeBytes": runtime.len(), "sha256": if tamper { "00".repeat(32) } else { sha(runtime) }},
            ]
        })
        .to_string();
        let mut entries: Vec<(String, Vec<u8>)> = vec![
            (format!("{top}/{executable}"), exe.to_vec()),
            (format!("{top}/runtime/decode.py"), runtime.to_vec()),
            (format!("{top}/{PACKAGE_MANIFEST}"), manifest.into_bytes()),
        ];
        if let Some((name, bytes)) = extra {
            entries.push((format!("{top}/{name}"), bytes.to_vec()));
        }
        let borrowed: Vec<(&str, &[u8])> = entries
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        zip_bytes(&borrowed)
    }

    fn pin(tool: &Tool, bytes: &[u8]) -> PinnedRelease<'static> {
        PinnedRelease {
            version: VERSION,
            url: Box::leak(format!("{}/v9.9.9/x.zip", tool.release_url()).into_boxed_str()),
            size: bytes.len() as u64,
            sha256: Box::leak(sha(bytes).into_boxed_str()),
            decoder_commit: "abc",
        }
    }

    fn good_probe(tool: &Tool) -> Result<ToolVersion, String> {
        Ok(ToolVersion {
            name: tool.name.into(),
            version: VERSION.into(),
            protocol_version: 1,
            notebook_schema_version: SCHEMA_VERSION,
            decoder_commit: "abc".into(),
        })
    }

    #[test]
    fn downloads_only_from_the_tools_own_releases_and_github_asset_hosts() {
        for tool in TOOLS {
            let release = format!("{}/v0.1.0/a.zip", tool.release_url());
            let repository = tool.repository;
            assert!(download_url_allowed(tool, &release, false));
            assert!(download_url_allowed(
                tool,
                "https://objects.githubusercontent.com/x?sig=1",
                true
            ));
            for refused in [
                "https://objects.githubusercontent.com/x".to_owned(),
                release.replace("https", "http"),
                "https://github.com/someone/else/releases/download/a.zip".to_owned(),
                format!("https://github.com@evil.example{repository}/releases/download/a"),
                format!("https://github.com:8443{repository}/releases/download/a"),
                // The repository's other pages are not release downloads.
                format!("https://github.com{repository}/archive/main.zip"),
            ] {
                assert!(!download_url_allowed(tool, &refused, false), "{refused}");
            }
            assert!(!download_url_allowed(tool, "https://evil.example/x", true));
            assert!(!download_url_allowed(tool, "/relative/redirect", true));
        }
        // One importer's releases are never a permitted download for the other.
        for (tool, other) in [(&GOODNOTES, &ONENOTE), (&ONENOTE, &GOODNOTES)] {
            let release = format!("{}/v0.1.0/a.zip", other.release_url());
            assert!(!download_url_allowed(tool, &release, false));
            assert!(!download_url_allowed(tool, &release, true));
        }
    }

    /// A stream that delivers some bytes and then fails, like a dropped connection.
    struct Interrupted(usize);

    impl Read for Interrupted {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.0 == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "connection reset",
                ));
            }
            let count = self.0.min(buffer.len());
            buffer[..count].fill(7);
            self.0 -= count;
            Ok(count)
        }
    }

    #[test]
    fn interrupted_short_long_and_cancelled_downloads_fail() {
        let home = tempfile::tempdir().unwrap();
        let into = home.path().join("download.part");
        let quiet = |_: u64, _: u64| {};
        let go = AtomicBool::new(false);
        let now = Instant::now();
        let bytes = vec![1_u8; 1000];
        let expected = sha(&bytes);
        assert_eq!(
            receive(bytes.as_slice(), 1000, &into, &go, now, &quiet).unwrap(),
            expected
        );
        assert!(
            receive(Interrupted(500), 1000, &into, &go, now, &quiet)
                .unwrap_err()
                .contains("interrupted")
        );
        assert!(
            receive(&bytes[..999], 1000, &into, &go, now, &quiet)
                .unwrap_err()
                .contains("early")
        );
        assert!(
            receive(vec![1_u8; 1001].as_slice(), 1000, &into, &go, now, &quiet)
                .unwrap_err()
                .contains("larger")
        );
        let stop = AtomicBool::new(true);
        assert_eq!(
            receive(bytes.as_slice(), 1000, &into, &stop, now, &quiet).unwrap_err(),
            CANCELLED
        );
        let late = Instant::now() - DOWNLOAD_DEADLINE - Duration::from_secs(1);
        assert!(
            receive(bytes.as_slice(), 1000, &into, &go, late, &quiet)
                .unwrap_err()
                .contains("timed out")
        );
    }

    #[test]
    fn installs_a_verified_package_and_keeps_it_when_a_later_one_is_bad() {
        for tool in TOOLS {
            let home = tempfile::tempdir().unwrap();
            let dir = InstallDir(home.path().to_path_buf(), tool);
            let good = package(tool, VERSION, false, None);
            let archive = home.path().join("good.zip");
            fs::write(&archive, &good).unwrap();
            let probe = |_: &Path| good_probe(tool);
            assert_eq!(
                install_archive(&dir, &archive, &pin(tool, &good), &probe).unwrap(),
                VERSION
            );
            assert_eq!(dir.active().unwrap().0, VERSION);
            assert!(usable_installation(&dir).unwrap().is_some());

            // Wrong checksum, tampered file, unlisted file, failing probe, the other importer's
            // package or version report: each refused, the installed version untouched and no
            // staging left behind.
            let other = if tool.id == "goodnotes" {
                &ONENOTE
            } else {
                &GOODNOTES
            };
            let tampered = package(tool, VERSION, true, None);
            let unlisted = package(
                tool,
                VERSION,
                false,
                Some(("runtime/extra.dll", b"surprise")),
            );
            let foreign = package(other, VERSION, false, None);
            let foreign_probe = |_: &Path| good_probe(other);
            type Probe<'a> = &'a dyn Fn(&Path) -> Result<ToolVersion, String>;
            let cases: Vec<(Vec<u8>, PinnedRelease, Probe)> = vec![
                (
                    good.clone(),
                    PinnedRelease {
                        sha256: "00",
                        ..pin(tool, &good)
                    },
                    &probe,
                ),
                (tampered.clone(), pin(tool, &tampered), &probe),
                (unlisted.clone(), pin(tool, &unlisted), &probe),
                (foreign.clone(), pin(tool, &foreign), &probe),
                (good.clone(), pin(tool, &good), &foreign_probe),
                (good.clone(), pin(tool, &good), &|_| {
                    Err("self-check failed".into())
                }),
            ];
            for (bytes, release, probe) in cases {
                fs::write(&archive, &bytes).unwrap();
                assert!(install_archive(&dir, &archive, &release, probe).is_err());
                assert_eq!(dir.active().unwrap().0, VERSION);
                assert!(usable_installation(&dir).unwrap().is_some());
                let leftovers = fs::read_dir(home.path())
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_name().to_string_lossy().starts_with("staging-"))
                    .count();
                assert_eq!(leftovers, 0);
            }
        }
    }

    #[test]
    fn activation_failure_restores_same_version_and_success_cleans_inactive_versions() {
        let home = tempfile::tempdir().unwrap();
        let dir = InstallDir(home.path().to_path_buf(), &GOODNOTES);
        let good = package(&GOODNOTES, VERSION, false, None);
        let archive = home.path().join("good.zip");
        fs::write(&archive, &good).unwrap();
        install_archive(&dir, &archive, &pin(&GOODNOTES, &good), &|_| {
            good_probe(&GOODNOTES)
        })
        .unwrap();
        let installed = dir.active().unwrap().1;
        // Refuse replacement while keeping the previous pointer readable.
        let active = home.path().join("active.json");
        #[cfg(windows)]
        let active_lock = {
            use std::os::windows::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(&active)
                .unwrap()
        };
        #[cfg(unix)]
        {
            fs::rename(&active, home.path().join("active-before.json")).unwrap();
            std::os::unix::fs::symlink(home.path().join("active-before.json"), &active).unwrap();
        }
        assert!(
            install_archive(&dir, &archive, &pin(&GOODNOTES, &good), &|_| good_probe(
                &GOODNOTES
            ))
            .is_err()
        );
        assert_eq!(dir.active().unwrap().1, installed);
        assert!(usable_installation(&dir).unwrap().is_some());
        #[cfg(windows)]
        drop(active_lock);
        #[cfg(unix)]
        {
            fs::remove_file(&active).unwrap();
            fs::rename(home.path().join("active-before.json"), &active).unwrap();
        }
        let old = dir.version_dir("0.1.0").unwrap();
        fs::create_dir(&old).unwrap();
        fs::write(old.join("old.exe"), b"old importer only").unwrap();
        let notebook = home.path().join("notebooks");
        fs::create_dir(&notebook).unwrap();
        fs::write(notebook.join("goodtype.json"), b"preserve").unwrap();
        install_archive(&dir, &archive, &pin(&GOODNOTES, &good), &|_| {
            good_probe(&GOODNOTES)
        })
        .unwrap();
        assert!(!old.exists());
        assert!(installed.exists());
        assert!(notebook.join("goodtype.json").is_file());
    }

    #[test]
    fn python_caches_are_cleared_but_other_additions_still_fail() {
        let home = tempfile::tempdir().unwrap();
        let dir = InstallDir(home.path().to_path_buf(), &GOODNOTES);
        let good = package(&GOODNOTES, VERSION, false, None);
        let archive = home.path().join("good.zip");
        fs::write(&archive, &good).unwrap();
        install_archive(&dir, &archive, &pin(&GOODNOTES, &good), &|_| {
            good_probe(&GOODNOTES)
        })
        .unwrap();
        let installed = dir.active().unwrap().1;
        let cache = installed.join("runtime/__pycache__/decode.cpython-314.pyc");
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, b"compiled, possibly tampered").unwrap();
        assert!(verify_package(&GOODNOTES, &installed).is_ok());
        assert!(!cache.exists(), "the cache must be removed, not trusted");
        fs::write(
            installed.join("runtime/__pycache__/sneaky.py"),
            b"import os",
        )
        .unwrap();
        assert!(verify_package(&GOODNOTES, &installed).is_err());
    }

    #[test]
    fn refuses_unsafe_archives() {
        for tool in TOOLS {
            let top = package_folder(tool, VERSION);
            let other = package_folder(
                if tool.id == "goodnotes" {
                    &ONENOTE
                } else {
                    &GOODNOTES
                },
                VERSION,
            );
            let home = tempfile::tempdir().unwrap();
            let cases: Vec<Vec<(String, &[u8])>> = vec![
                vec![(format!("{top}/../escape.txt"), b"x")],
                vec![("C:/Windows/evil.dll".into(), b"x")],
                vec![("/abs.txt".into(), b"x")],
                vec![(format!("{top}\\back\\slash.txt"), b"x")],
                vec![("other-folder/a.txt".into(), b"x")],
                vec![(format!("{other}/a.txt"), b"x")],
                vec![(format!("{top}/CON.txt"), b"x")],
                vec![
                    (format!("{top}/a.txt"), b"x"),
                    (format!("{top}/A.TXT"), b"y"),
                ],
            ];
            for (index, entries) in cases.iter().enumerate() {
                let borrowed: Vec<(&str, &[u8])> =
                    entries.iter().map(|(n, b)| (n.as_str(), *b)).collect();
                let archive = home.path().join(format!("{index}.zip"));
                fs::write(&archive, zip_bytes(&borrowed)).unwrap();
                let into = home.path().join(format!("out-{index}"));
                assert!(
                    extract_package(&archive, &top, &into).is_err(),
                    "{} case {index} was accepted",
                    tool.id
                );
            }
        }
    }

    #[test]
    fn refuses_links_and_decompression_beyond_the_budget() {
        let top = package_folder(&GOODNOTES, VERSION);
        let home = tempfile::tempdir().unwrap();
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .add_symlink(
                format!("{top}/link"),
                "../../outside",
                SimpleFileOptions::default(),
            )
            .unwrap();
        let archive = home.path().join("link.zip");
        fs::write(&archive, writer.finish().unwrap().into_inner()).unwrap();
        assert!(extract_package(&archive, &top, &home.path().join("a")).is_err());

        // 513 MiB of zeros compresses to well under a megabyte.
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                format!("{top}/bomb.bin"),
                SimpleFileOptions::default().large_file(true),
            )
            .unwrap();
        let chunk = vec![0_u8; MIB as usize];
        for _ in 0..513 {
            writer.write_all(&chunk).unwrap();
        }
        let bytes = writer.finish().unwrap().into_inner();
        assert!((bytes.len() as u64) < 4 * MIB);
        let archive = home.path().join("bomb.zip");
        fs::write(&archive, bytes).unwrap();
        let error = extract_package(&archive, &top, &home.path().join("b")).unwrap_err();
        assert!(error.contains("512 MiB"), "{error}");
    }

    #[test]
    fn contract_needs_matching_protocol_schema_and_pin() {
        let manifest = |protocol, schema| PackageManifest {
            version: VERSION.into(),
            protocol_version: protocol,
            notebook_schema_version: schema,
            target: TARGET.into(),
            decoder_commit: "abc".into(),
            files: Vec::new(),
        };
        for tool in TOOLS {
            let release = pin(tool, b"x");
            let check = |manifest: &PackageManifest, version, pin| {
                check_contract(tool, manifest, version, pin)
            };
            let reported = good_probe(tool).unwrap();
            assert!(
                check(
                    &manifest(1, SCHEMA_VERSION),
                    Some(&reported),
                    Some(&release)
                )
                .is_ok()
            );
            assert!(check(&manifest(2, SCHEMA_VERSION), None, None).is_err());
            assert!(check(&manifest(1, SCHEMA_VERSION + 1), None, None).is_err());
            let other_release = PinnedRelease {
                version: "1.0.0",
                ..release
            };
            assert!(check(&manifest(1, SCHEMA_VERSION), None, Some(&other_release)).is_err());
            let mut lying = good_probe(tool).unwrap();
            lying.version = "0.0.1".into();
            assert!(check(&manifest(1, SCHEMA_VERSION), Some(&lying), None).is_err());
            // A package whose executable reports the other importer's name is not this one.
            let other = if tool.id == "goodnotes" {
                &ONENOTE
            } else {
                &GOODNOTES
            };
            let impostor = good_probe(other).unwrap();
            assert!(check(&manifest(1, SCHEMA_VERSION), Some(&impostor), None).is_err());
        }
    }

    fn fixture_notebook(into: &Path) {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/notebooks/multipage");
        copy_dir(&fixture, into);
    }

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    fn entry(status: &str, source: &str, target: Option<&str>, error: Option<&str>) -> ReportEntry {
        ReportEntry {
            status: status.into(),
            source: source.into(),
            target: target.map(Into::into),
            pages: 2,
            error: error.map(Into::into),
        }
    }

    #[test]
    fn publishes_only_validated_completed_notebooks_without_replacing_anything() {
        let home = tempfile::tempdir().unwrap();
        let tool_batch = home.path().join("staging/batch");
        fixture_notebook(&tool_batch.join("Mechanics/Lecture"));
        fixture_notebook(&tool_batch.join("Existing"));
        // Completed but broken (manifest without pages), and a cancelled half-written folder.
        fs::create_dir_all(tool_batch.join("Broken")).unwrap();
        fs::write(tool_batch.join("Broken/goodtype.json"), "{}").unwrap();
        fs::create_dir_all(tool_batch.join("Half")).unwrap();
        let library_batch = home.path().join("Library/Goodnotes-Import");
        fixture_notebook(&library_batch.join("Existing"));
        let before = fs::read(library_batch.join("Existing/goodtype.json")).unwrap();

        let entries = vec![
            entry(
                "completed",
                "Mechanics/Lecture",
                Some("Mechanics/Lecture"),
                None,
            ),
            entry("completed", "Existing", Some("Existing"), None),
            entry("completed", "Broken", Some("Broken"), None),
            entry("completed", "Escape", Some("../../outside"), None),
            entry(
                "failed",
                "Sketches",
                None,
                Some("page 3 needs vector artwork — use --allow-derived"),
            ),
            entry("failed", "Audio", None, Some("unsupported audio")),
            entry("pending", "Half", Some("Half"), None),
        ];
        let outcomes = publish(
            &entries,
            &tool_batch,
            (&library_batch, "Goodnotes-Import"),
            true,
        );
        let statuses: Vec<_> = outcomes.iter().map(|outcome| outcome.status).collect();
        assert_eq!(
            statuses,
            [
                "imported",
                "alreadyThere",
                "failed",
                "failed",
                "needsArtwork",
                "failed",
                "cancelled"
            ]
        );
        assert_eq!(
            outcomes[0].path.as_deref(),
            Some("Goodnotes-Import/Mechanics/Lecture")
        );
        assert!(validate_notebook(&library_batch.join("Mechanics/Lecture")).is_ok());
        assert_eq!(
            fs::read(library_batch.join("Existing/goodtype.json")).unwrap(),
            before
        );
        assert!(!library_batch.join("Broken").exists());
        assert!(!library_batch.join("Half").exists());
        assert!(!home.path().join("outside").exists());
    }

    #[test]
    fn a_pending_transaction_is_refused_rather_than_recovered() {
        let home = tempfile::tempdir().unwrap();
        let notebook = home.path().join("n");
        fixture_notebook(&notebook);
        fs::create_dir_all(notebook.join(".goodtype")).unwrap();
        fs::write(notebook.join(layout::PENDING_TRANSACTION), b"partial").unwrap();
        assert!(validate_notebook(&notebook).is_err());
        assert_eq!(
            fs::read(notebook.join(layout::PENDING_TRANSACTION)).unwrap(),
            b"partial"
        );
    }

    #[test]
    fn malformed_or_oversized_reports_are_refused() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("import-report.json");
        fs::write(&path, b"{not json").unwrap();
        assert!(read_report(&path).is_err());
        fs::write(&path, vec![b' '; MAX_JSON_BYTES + 1]).unwrap();
        assert!(read_report(&path).is_err());
        fs::write(
            &path,
            br#"[{"status":"completed","source":"a","target":"a","pages":1,"error":null}]"#,
        )
        .unwrap();
        assert_eq!(read_report(&path).unwrap().len(), 1);
    }

    /// Opt-in check against a real release package and real Goodnotes exports, which never belong
    /// in the repository:
    ///
    /// ```text
    /// GOODNOTES_RELEASE_ZIP=…zip GOODNOTES_RELEASE_SHA256=… GOODNOTES_SMALL=….goodnotes \
    /// GOODNOTES_BACKUP=….zip GOODNOTES_LARGE=….goodnotes cargo test real_release -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "needs a downloaded release package and private Goodnotes exports"]
    fn real_release_installs_imports_and_cancels() {
        let env = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
        let archive = PathBuf::from(env("GOODNOTES_RELEASE_ZIP"));
        let release = PinnedRelease {
            version: "0.1.0",
            url: "https://github.com/Just8it/Goodtype-Goodnodes-importer/releases/download/v0.1.0/goodtype-goodnotes-importer-0.1.0-windows-x64.zip",
            size: fs::metadata(&archive).unwrap().len(),
            sha256: Box::leak(env("GOODNOTES_RELEASE_SHA256").into_boxed_str()),
            decoder_commit: "66028455fd7fc61b7931679915d811a25485e3ad",
        };
        let home = tempfile::tempdir().unwrap();
        let dir = InstallDir(home.path().join("installed"), &GOODNOTES);
        fs::create_dir_all(&dir.0).unwrap();
        let started = Instant::now();
        assert_eq!(
            install_archive(&dir, &archive, &release, &|path| probe_tool(
                &GOODNOTES, path
            ))
            .unwrap(),
            "0.1.0"
        );
        println!("installed and self-checked in {:?}", started.elapsed());
        let (_, tool) = usable_installation(&dir).unwrap().unwrap();

        let library = home.path().join("Library");
        fs::create_dir_all(&library).unwrap();
        let state = Mutex::new(State::default());
        let run = |source: &str, batch: &str, derived: bool, cancel: &AtomicBool| {
            let staging = library.join(format!(".goodtype-import-{batch}-{derived}"));
            fs::create_dir(&staging).unwrap();
            let folder = library.join(batch);
            let started = Instant::now();
            let result = run_import(
                &GOODNOTES,
                &tool,
                Path::new(source),
                &staging,
                (&folder, batch),
                derived,
                cancel,
                &state,
            );
            let _ = fs::remove_dir_all(&staging);
            println!(
                "{batch} (derived={derived}) in {:?}: {result:#?}",
                started.elapsed()
            );
            result.unwrap()
        };
        let go = AtomicBool::new(false);

        // Without vector artwork the small notebook needs it; with it, the retry fills the gap.
        let first = run(&env("GOODNOTES_SMALL"), "Small", false, &go);
        assert_eq!(first.notebooks[0].status, "needsArtwork");
        assert!(
            first.batch.is_none(),
            "an empty batch folder must not be left behind"
        );
        let second = run(&env("GOODNOTES_SMALL"), "Small", true, &go);
        assert_eq!(second.notebooks[0].status, "imported");
        assert!(second.notebooks[0].artwork_pages > 0);

        let backup = run(&env("GOODNOTES_BACKUP"), "Backup", true, &go);
        assert!(
            backup
                .notebooks
                .iter()
                .all(|notebook| notebook.status == "imported")
        );
        for notebook in &backup.notebooks {
            let path = library.join(notebook.path.as_ref().unwrap());
            validate_notebook(&path).unwrap();
            assert!(path.join("originals/source.goodnotes").is_file());
        }

        // Cancelling the large notebook mid-conversion publishes nothing half-written.
        let cancel = Arc::new(AtomicBool::new(false));
        let trigger = cancel.clone();
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(4));
            trigger.store(true, Ordering::SeqCst);
        });
        let cancelled = run(&env("GOODNOTES_LARGE"), "Large", true, &cancel);
        timer.join().unwrap();
        assert!(cancelled.cancelled);
        assert!(
            cancelled
                .notebooks
                .iter()
                .all(|notebook| notebook.status != "imported")
        );
        assert!(!library.join("Large").exists());
        let leftovers: Vec<_> = fs::read_dir(&library)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        println!("library now holds {leftovers:?}");
        assert!(
            leftovers
                .iter()
                .all(|name| !name.starts_with(".goodtype-import"))
        );

        // Without the importer installed, an imported notebook edits, saves, moves and reopens.
        fs::remove_dir_all(&dir.0).unwrap();
        let notebook = library.join(backup.notebooks[0].path.as_ref().unwrap());
        let mut snapshot = storage::open_notebook(&notebook).unwrap();
        snapshot.manifest.title = "Edited after import".into();
        storage::save_notebook(&notebook, &snapshot).unwrap();
        let moved = library.join("Moved notebook");
        fs::rename(&notebook, &moved).unwrap();
        validate_notebook(&moved).unwrap();
        assert_eq!(
            storage::open_notebook(&moved).unwrap().manifest.title,
            "Edited after import"
        );
        println!("edited, saved, moved and reopened without the importer installed");
    }

    /// Opt-in: the pinned OneNote release, installed and run exactly as the app does it, on a
    /// OneNote export that never belongs in the repository:
    ///
    /// ```text
    /// ONENOTE_RELEASE_ZIP=…zip ONENOTE_EXPORT=….onepkg cargo test real_onenote -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "needs the downloaded OneNote release package and a OneNote export"]
    fn real_onenote_release_installs_and_imports() {
        let env = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
        let archive = PathBuf::from(env("ONENOTE_RELEASE_ZIP"));
        let release = ONENOTE.pinned.expect("a pinned OneNote release");
        let home = tempfile::tempdir().unwrap();
        let dir = InstallDir(home.path().join("installed"), &ONENOTE);
        fs::create_dir_all(&dir.0).unwrap();
        install_archive(&dir, &archive, &release, &|path| probe_tool(&ONENOTE, path)).unwrap();
        let (_, installed) = usable_installation(&dir).unwrap().unwrap();

        let library = home.path().join("Library");
        let staging = library.join(".goodtype-import-test");
        fs::create_dir_all(&staging).unwrap();
        let name = batch_name(&ONENOTE, &library, &today()).unwrap();
        let folder = library.join(&name);
        let state = Mutex::new(State::default());
        let result = run_import(
            &ONENOTE,
            &installed,
            Path::new(&env("ONENOTE_EXPORT")),
            &staging,
            (&folder, &name),
            false,
            &AtomicBool::new(false),
            &state,
        )
        .unwrap();
        fs::remove_dir_all(&staging).unwrap();
        println!("{result:#?}");
        assert!(result.problem.is_none());
        assert!(!result.notebooks.is_empty());
        for notebook in &result.notebooks {
            assert_eq!(notebook.status, "imported", "{notebook:?}");
            let path = library.join(notebook.path.as_ref().unwrap());
            validate_notebook(&path).unwrap();
            assert!(path.join("originals/source.one").is_file());
        }
    }

    #[test]
    fn the_pinned_releases_are_permitted_downloads() {
        for tool in TOOLS {
            let pin = tool.pinned.expect("a tested release is pinned");
            assert!(download_url_allowed(tool, pin.url, false), "{}", tool.id);
            assert!(pin.size > 0 && pin.size <= MAX_PACKAGE_BYTES);
            assert_eq!(pin.sha256.len(), 64);
            assert_eq!(pin.decoder_commit.len(), 40);
            assert!(
                pin.url
                    .ends_with(&format!("{}.zip", package_folder(tool, pin.version)))
            );
        }
        assert_ne!(GOODNOTES.id, ONENOTE.id);
        assert_ne!(GOODNOTES.repository, ONENOTE.repository);
    }

    #[test]
    fn batch_folders_never_reuse_a_name() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            batch_name(&GOODNOTES, home.path(), "2026-10-21").unwrap(),
            "Goodnotes Import 2026-10-21"
        );
        fs::create_dir(home.path().join("Goodnotes Import 2026-10-21")).unwrap();
        assert_eq!(
            batch_name(&GOODNOTES, home.path(), "2026-10-21").unwrap(),
            "Goodnotes Import 2026-10-21 (2)"
        );
        assert_eq!(
            batch_name(&ONENOTE, home.path(), "2026-10-21").unwrap(),
            "OneNote Import 2026-10-21"
        );
        assert_eq!(today().len(), 10);
        library::validate_name("Goodnotes Import 2026-10-21 (2)").unwrap();
        library::validate_name("OneNote Import 2026-10-21").unwrap();
    }

    #[test]
    fn only_one_update_or_import_can_claim_the_importer() {
        let importer = Arc::new(Importer::default());
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let importer = importer.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    importer.begin(Activity::Checking).is_ok()
                })
            })
            .collect();
        assert_eq!(
            workers
                .into_iter()
                .map(|thread| usize::from(thread.join().unwrap()))
                .sum::<usize>(),
            1
        );
        assert!(
            importer
                .begin(Activity::Importing {
                    done: 0,
                    total: 0,
                    cancelling: false
                })
                .is_err()
        );
        importer.set(Activity::Idle);
        assert!(importer.begin(Activity::Installing).is_ok());
        // The other importer keeps its own job; only the fixed ids exist.
        let importers = Importers::default();
        let (goodnotes, tool) = importers.get("goodnotes").unwrap();
        assert_eq!(tool.id, "goodnotes");
        goodnotes.begin(Activity::Installing).unwrap();
        let (onenote, tool) = importers.get("onenote").unwrap();
        assert_eq!(tool.id, "onenote");
        assert!(onenote.begin(Activity::Installing).is_ok());
        assert!(importers.get("../goodnotes").is_err());
        assert!(importers.get("").is_err());
    }
}
