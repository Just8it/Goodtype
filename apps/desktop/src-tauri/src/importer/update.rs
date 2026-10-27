//! Explicit stable-channel discovery. Only signed metadata can authorize a new executable.
use std::io::Read;

use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;

use super::{
    MAX_PACKAGE_BYTES, PROTOCOL_VERSION, PinnedRelease, ReleaseView, SCHEMA_VERSION, TARGET, Tool,
    github_asset,
};

const MAX_DESCRIPTOR: u64 = 64 * 1024;

#[derive(Clone, Debug)]
pub(super) struct VerifiedUpdate {
    pub version: String,
    pub size: u64,
    url: String,
    sha256: String,
    decoder_commit: String,
}

impl VerifiedUpdate {
    pub fn pin(&self) -> PinnedRelease<'_> {
        PinnedRelease {
            version: &self.version,
            size: self.size,
            url: &self.url,
            sha256: &self.sha256,
            decoder_commit: &self.decoder_commit,
        }
    }

    pub fn view(&self) -> ReleaseView {
        ReleaseView {
            version: self.version.clone(),
            size_bytes: self.size,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Descriptor {
    version: String,
    protocol_version: u64,
    notebook_schema_version: u32,
    target: String,
    source_dirty: bool,
    source_commit: String,
    decoder_commit: String,
    archive: Archive,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Archive {
    file_name: String,
    size_bytes: u64,
    sha256: String,
    url: String,
}

fn stable_version(version: &str) -> Result<[u32; 3], String> {
    let parts = version
        .split('.')
        .map(|part| {
            if part.is_empty()
                || part.len() > 9
                || !part.bytes().all(|c| c.is_ascii_digit())
                || part.len() > 1 && part.starts_with('0')
            {
                return Err("not a stable importer version".to_owned());
            }
            part.parse::<u32>()
                .map_err(|_| "invalid importer version".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    parts
        .try_into()
        .map_err(|_| "not a three-part importer version".into())
}

pub(super) fn require_newer(version: &str, installed: &str) -> Result<(), String> {
    if stable_version(version)? <= stable_version(installed)? {
        return Err("a newer or equal importer version is already installed".into());
    }
    Ok(())
}

fn is_hash(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Authenticates `bytes` with `key` and checks they describe a compatible, clean, stable release
/// of `tool` from its own repository.
fn verify(
    tool: &Tool,
    bytes: &[u8],
    signature: &[u8],
    key: &[u8; 32],
) -> Result<VerifiedUpdate, String> {
    if bytes.len() as u64 > MAX_DESCRIPTOR {
        return Err("importer update metadata is too large".into());
    }
    VerifyingKey::from_bytes(key)
        .map_err(|_| "invalid importer verification key")?
        .verify_strict(
            bytes,
            &Signature::from_slice(signature).map_err(|_| "invalid importer signature length")?,
        )
        .map_err(|_| "the importer update signature is invalid; the installed version was kept")?;
    let descriptor: Descriptor = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid importer update metadata ({error})"))?;
    stable_version(&descriptor.version)?;
    if descriptor.source_dirty
        || !is_hash(&descriptor.source_commit, 40)
        || !is_hash(&descriptor.decoder_commit, 40)
        || !is_hash(&descriptor.archive.sha256, 64)
    {
        return Err("importer update is not a clean, identified release".into());
    }
    if descriptor.protocol_version != PROTOCOL_VERSION
        || descriptor.notebook_schema_version != SCHEMA_VERSION
        || descriptor.target != TARGET
    {
        return Err("the newest importer requires a different Goodtype version; the installed version was kept".into());
    }
    let name = format!("{}-{}-windows-x64.zip", tool.package, descriptor.version);
    if descriptor.archive.file_name != name
        || descriptor.archive.url
            != format!("{}/v{}/{name}", tool.release_url(), descriptor.version)
        || descriptor.archive.size_bytes == 0
        || descriptor.archive.size_bytes > MAX_PACKAGE_BYTES
    {
        return Err("unexpected importer update package URL or size".into());
    }
    Ok(VerifiedUpdate {
        version: descriptor.version,
        size: descriptor.archive.size_bytes,
        url: descriptor.archive.url,
        sha256: descriptor.archive.sha256,
        decoder_commit: descriptor.decoder_commit,
    })
}

fn fetch(tool: &Tool, url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    github_asset(tool, url, true)?
        .into_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("the importer update check was interrupted ({error})"))?;
    if bytes.len() as u64 > limit {
        return Err("importer update metadata exceeds its size limit".into());
    }
    Ok(bytes)
}

pub(super) fn check(
    tool: &Tool,
    installed: Option<&str>,
) -> Result<Option<VerifiedUpdate>, String> {
    let bytes = fetch(tool, &tool.latest_descriptor(), MAX_DESCRIPTOR)?;
    // This untrusted version is used only to form a fixed-repository signature URL.
    let untrusted: Descriptor =
        serde_json::from_slice(&bytes).map_err(|_| "malformed importer release descriptor")?;
    stable_version(&untrusted.version)?;
    let signature = fetch(
        tool,
        &format!(
            "{}/v{}/importer-update.json.sig",
            tool.release_url(),
            untrusted.version
        ),
        64,
    )?;
    let verified = verify(tool, &bytes, &signature, &tool.public_key)?;
    if let Some(version) = installed.or_else(|| tool.pinned.as_ref().map(|pin| pin.version)) {
        let ordering = stable_version(&verified.version)?.cmp(&stable_version(version)?);
        if !ordering.is_gt() {
            return Ok(None);
        }
    }
    Ok(Some(verified))
}

#[cfg(test)]
mod tests {
    use super::super::{GOODNOTES, ONENOTE};
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    fn descriptor(tool: &Tool) -> serde_json::Value {
        let name = format!("{}-0.1.1-windows-x64.zip", tool.package);
        json!({"version":"0.1.1", "protocolVersion":1, "notebookSchemaVersion":SCHEMA_VERSION,
            "target":TARGET, "sourceDirty":false, "sourceCommit":"1".repeat(40), "decoderCommit":"2".repeat(40),
            "archive":{"fileName":name, "sizeBytes":1234,
                "sha256":"3".repeat(64), "url":format!("{}/v0.1.1/{name}", tool.release_url())}})
    }

    #[test]
    fn authenticated_update_rejects_tampering_and_wrong_keys() {
        let key = SigningKey::from_bytes(&[42; 32]);
        for tool in [&GOODNOTES, &ONENOTE] {
            let bytes = serde_json::to_vec(&descriptor(tool)).unwrap();
            let signature = key.sign(&bytes).to_bytes();
            let ours = key.verifying_key();
            let verified = verify(tool, &bytes, &signature, ours.as_bytes()).unwrap();
            assert_eq!(verified.pin().version, "0.1.1");
            let mut changed = bytes.clone();
            changed.push(b' ');
            assert!(verify(tool, &changed, &signature, ours.as_bytes()).is_err());
            assert!(verify(tool, &bytes, &signature, &tool.public_key).is_err());
            assert!(verify(tool, &bytes, &[0; 63], ours.as_bytes()).is_err());
        }
    }

    #[test]
    fn one_importers_release_never_authorizes_the_other() {
        assert_ne!(GOODNOTES.public_key, ONENOTE.public_key);
        let key = SigningKey::from_bytes(&[42; 32]);
        for (named, used) in [(&GOODNOTES, &ONENOTE), (&ONENOTE, &GOODNOTES)] {
            // Even correctly signed, a descriptor for one importer's package and repository is
            // refused when checked as the other's.
            let bytes = serde_json::to_vec(&descriptor(named)).unwrap();
            let signature = key.sign(&bytes).to_bytes();
            assert!(verify(used, &bytes, &signature, key.verifying_key().as_bytes()).is_err());
        }
    }

    #[test]
    fn signed_metadata_still_needs_compatible_clean_bounded_assets() {
        let key = SigningKey::from_bytes(&[42; 32]);
        for tool in [&GOODNOTES, &ONENOTE] {
            for (pointer, value) in [
                ("/protocolVersion", json!(2)),
                ("/notebookSchemaVersion", json!(999)),
                ("/target", json!("wrong-target")),
                ("/sourceDirty", json!(true)),
                ("/sourceCommit", json!("bad")),
                ("/version", json!("0.1.2-rc.1")),
                ("/archive/url", json!("https://evil.invalid/tool.zip")),
                ("/archive/sha256", json!("bad")),
                ("/archive/sizeBytes", json!(MAX_PACKAGE_BYTES + 1)),
            ] {
                let mut data = descriptor(tool);
                *data.pointer_mut(pointer).unwrap() = value;
                let bytes = serde_json::to_vec(&data).unwrap();
                let signature = key.sign(&bytes).to_bytes();
                assert!(
                    verify(tool, &bytes, &signature, key.verifying_key().as_bytes()).is_err(),
                    "{} {pointer}",
                    tool.id
                );
            }
        }
    }

    #[test]
    fn versions_are_numeric_stable_and_cannot_downgrade() {
        assert!(require_newer("0.1.10", "0.1.9").is_ok());
        assert!(require_newer("0.1.1", "0.1.1").is_err());
        assert!(require_newer("0.1.1", "0.2.0").is_err());
        for version in [
            "../1",
            "v1.2.3",
            "1.2",
            "01.2.3",
            "1.2.3-rc.1",
            "1.2.3/../4",
            "9999999999.1.2",
        ] {
            assert!(stable_version(version).is_err(), "{version}");
        }
    }

    fn tool_from_env() -> &'static Tool {
        match std::env::var("IMPORTER_TOOL").as_deref() {
            Ok("onenote") => &ONENOTE,
            _ => &GOODNOTES,
        }
    }

    /// `IMPORTER_TOOL=onenote|goodnotes cargo test live_update -- --ignored --nocapture`
    #[test]
    #[ignore = "explicit live GitHub update discovery check"]
    fn live_update_check_authenticates_the_published_stable_release() {
        let tool = tool_from_env();
        let release = check(tool, Some("0.0.0"))
            .unwrap()
            .expect("a signed stable release");
        require_newer(&release.version, "0.0.0").unwrap();
        assert!(release.url.starts_with(&tool.release_url()));
        println!(
            "Live update discovery authenticated {} importer {} ({} bytes).",
            tool.id, release.version, release.size
        );
    }

    /// `IMPORTER_TOOL=onenote|goodnotes IMPORTER_UPDATE_FIXTURE=<downloaded release assets>`
    #[test]
    #[ignore = "needs a real signed release bundle in IMPORTER_UPDATE_FIXTURE"]
    fn real_signed_release_installs_with_the_production_key() {
        use super::super::{InstallDir, install_archive, probe_tool, verify_package};
        use crate::store::read_bounded;
        let tool = tool_from_env();
        let root = std::path::PathBuf::from(std::env::var_os("IMPORTER_UPDATE_FIXTURE").unwrap());
        let bytes =
            read_bounded(&root.join("importer-update.json"), MAX_DESCRIPTOR as usize).unwrap();
        let signature = read_bounded(&root.join("importer-update.json.sig"), 64).unwrap();
        let release = verify(tool, &bytes, &signature, &tool.public_key).unwrap();
        let archive = root.join(format!(
            "{}-{}-windows-x64.zip",
            tool.package, release.version
        ));
        let temporary = tempfile::tempdir().unwrap();
        let dir = InstallDir(temporary.path().to_path_buf(), tool);
        let previous = temporary.path().join("versions/0.0.9");
        std::fs::create_dir_all(&previous).unwrap();
        std::fs::write(previous.join("placeholder"), b"retired importer fixture").unwrap();
        let installed = install_archive(&dir, &archive, &release.pin(), &|path| {
            probe_tool(tool, path)
        })
        .unwrap();
        assert_eq!(installed, release.version);
        assert!(!previous.exists());
        let (_, active) = dir.active().unwrap();
        verify_package(tool, &active).unwrap();
        println!(
            "Verified the {} signature, installed {installed}, passed its self-check and removed the inactive version.",
            tool.id
        );
    }
}
