//! Explicit stable-channel discovery. Only signed metadata can authorize a new executable.
use std::io::Read;

use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;

use super::{
    MAX_PACKAGE_BYTES, PINNED, PROTOCOL_VERSION, PinnedRelease, ReleaseView, SCHEMA_VERSION,
    TARGET, github_asset,
};

pub(super) const LATEST_DESCRIPTOR: &str = "https://github.com/Just8it/Goodtype-Goodnodes-importer/releases/latest/download/importer-update.json";
const REPOSITORY: &str = "https://github.com/Just8it/Goodtype-Goodnodes-importer/releases/download";
// Matches the importer's release-public-key.pem. Private key lives only in the publisher secret.
const PUBLIC_KEY: [u8; 32] = [
    0x42, 0x8c, 0xc6, 0x8f, 0x6d, 0x0b, 0x38, 0xea, 0xe7, 0x4d, 0xcb, 0x21, 0x1e, 0x48, 0x77, 0xc6,
    0xeb, 0x51, 0x54, 0x8d, 0xaa, 0xbf, 0x24, 0xde, 0x64, 0xa1, 0x85, 0x88, 0x5c, 0x82, 0x5b, 0x3c,
];
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

fn verify(bytes: &[u8], signature: &[u8], key: &[u8; 32]) -> Result<VerifiedUpdate, String> {
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
    let name = format!(
        "goodtype-goodnotes-importer-{}-windows-x64.zip",
        descriptor.version
    );
    if descriptor.archive.file_name != name
        || descriptor.archive.url != format!("{REPOSITORY}/v{}/{name}", descriptor.version)
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

fn fetch(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    github_asset(url, true)?
        .into_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("the importer update check was interrupted ({error})"))?;
    if bytes.len() as u64 > limit {
        return Err("importer update metadata exceeds its size limit".into());
    }
    Ok(bytes)
}

pub(super) fn check(installed: Option<&str>) -> Result<Option<VerifiedUpdate>, String> {
    let bytes = fetch(LATEST_DESCRIPTOR, MAX_DESCRIPTOR)?;
    // This untrusted version is used only to form a fixed-repository signature URL.
    let untrusted: Descriptor =
        serde_json::from_slice(&bytes).map_err(|_| "malformed importer release descriptor")?;
    stable_version(&untrusted.version)?;
    let signature = fetch(
        &format!(
            "{REPOSITORY}/v{}/importer-update.json.sig",
            untrusted.version
        ),
        64,
    )?;
    let verified = verify(&bytes, &signature, &PUBLIC_KEY)?;
    if let Some(version) = installed.or_else(|| PINNED.as_ref().map(|pin| pin.version)) {
        let ordering = stable_version(&verified.version)?.cmp(&stable_version(version)?);
        if !ordering.is_gt() {
            return Ok(None);
        }
    }
    Ok(Some(verified))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    fn descriptor() -> serde_json::Value {
        json!({"version":"0.1.1", "protocolVersion":1, "notebookSchemaVersion":SCHEMA_VERSION,
            "target":TARGET, "sourceDirty":false, "sourceCommit":"1".repeat(40), "decoderCommit":"2".repeat(40),
            "archive":{"fileName":"goodtype-goodnotes-importer-0.1.1-windows-x64.zip", "sizeBytes":1234,
                "sha256":"3".repeat(64), "url":format!("{REPOSITORY}/v0.1.1/goodtype-goodnotes-importer-0.1.1-windows-x64.zip")}})
    }

    #[test]
    fn authenticated_update_rejects_tampering_and_wrong_keys() {
        let key = SigningKey::from_bytes(&[42; 32]);
        let bytes = serde_json::to_vec(&descriptor()).unwrap();
        let signature = key.sign(&bytes).to_bytes();
        let verified = verify(&bytes, &signature, key.verifying_key().as_bytes()).unwrap();
        assert_eq!(verified.pin().version, "0.1.1");
        let mut changed = bytes.clone();
        changed.push(b' ');
        assert!(verify(&changed, &signature, key.verifying_key().as_bytes()).is_err());
        assert!(verify(&bytes, &signature, &PUBLIC_KEY).is_err());
        assert!(verify(&bytes, &[0; 63], key.verifying_key().as_bytes()).is_err());
    }

    #[test]
    fn signed_metadata_still_needs_compatible_clean_bounded_assets() {
        let key = SigningKey::from_bytes(&[42; 32]);
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
            let mut data = descriptor();
            *data.pointer_mut(pointer).unwrap() = value;
            let bytes = serde_json::to_vec(&data).unwrap();
            let signature = key.sign(&bytes).to_bytes();
            assert!(
                verify(&bytes, &signature, key.verifying_key().as_bytes()).is_err(),
                "{pointer}"
            );
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

    #[test]
    #[ignore = "explicit live GitHub update discovery check"]
    fn live_update_check_authenticates_the_published_stable_release() {
        let release = check(Some("0.0.0"))
            .unwrap()
            .expect("a signed stable release");
        require_newer(&release.version, "0.0.0").unwrap();
        assert!(release.url.starts_with(REPOSITORY));
        println!(
            "Live update discovery authenticated importer {} ({} bytes).",
            release.version, release.size
        );
    }

    #[test]
    #[ignore = "needs a real signed release bundle in GOODNOTES_UPDATE_FIXTURE"]
    fn real_signed_release_installs_with_the_production_key() {
        use super::super::{InstallDir, install_archive, probe_tool, read_bounded, verify_package};
        let root = std::path::PathBuf::from(std::env::var_os("GOODNOTES_UPDATE_FIXTURE").unwrap());
        let bytes = read_bounded(&root.join("importer-update.json"), MAX_DESCRIPTOR).unwrap();
        let signature = read_bounded(&root.join("importer-update.json.sig"), 64).unwrap();
        let release = verify(&bytes, &signature, &PUBLIC_KEY).unwrap();
        let archive = root.join(format!(
            "goodtype-goodnotes-importer-{}-windows-x64.zip",
            release.version
        ));
        let temporary = tempfile::tempdir().unwrap();
        let dir = InstallDir(temporary.path().to_path_buf());
        let previous = temporary.path().join("versions/0.1.0");
        std::fs::create_dir_all(&previous).unwrap();
        std::fs::write(previous.join("placeholder"), b"retired importer fixture").unwrap();
        let installed = install_archive(&dir, &archive, &release.pin(), &probe_tool).unwrap();
        assert_eq!(installed, release.version);
        assert!(!previous.exists());
        let (_, active) = dir.active().unwrap();
        verify_package(&active).unwrap();
        println!(
            "Verified signature, installed {installed}, passed runtime self-check and removed inactive version."
        );
    }
}
