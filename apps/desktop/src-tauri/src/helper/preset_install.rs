//! Review-only preset candidates; only the UI Install command can persist a new style.
use super::*;
use goodtype_core::storage;
use goodtype_typst::{GrantedFiles, PresetPackageBytes};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedPreset {
    pub source: String,
    pub name: String,
    pub description: Option<String>,
    pub source_url: Option<String>,
    pub candidate_hash: String,
    pub dependency_hashes: BTreeMap<String, String>,
    pub check: goodtype_typst::PresetCheck,
    pub svg: String,
    pub png: String,
    pub filename: String,
    pub epoch: u64,
    pub request_id: String,
    #[serde(skip)]
    pub generation: u64,
    #[serde(skip)]
    pub files: GrantedFiles,
    #[serde(skip)]
    pub packages: PresetPackageBytes,
}
fn digest(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}
fn optional(args: &Value, key: &str, limit: usize) -> Result<Option<String>, String> {
    match &args[key] {
        Value::Null => Ok(None),
        Value::String(s) if s.len() <= limit => Ok(Some(s.clone())),
        _ => Err(format!("invalid {key}")),
    }
}
fn filename(files: &GrantedFiles) -> String {
    // Never shadow a real granted path. Stable within an unchanged grant.
    (0..)
        .map(|i| format!("helper-candidate-{i}.typ"))
        .find(|n| !files.contains_key(&format!("/styles/{n}")))
        .unwrap()
}
fn compile(
    root: &Path,
    source: String,
    files: GrantedFiles,
    replay: Option<PresetPackageBytes>,
    edge: u32,
    allow_remote_packages: bool,
) -> Result<(ReviewedPreset, Vec<u8>, u32, u32), String> {
    let filename = filename(&files);
    let mut world_files = files.clone();
    world_files.insert(format!("/styles/{filename}"), source.as_bytes().to_vec());
    let ((check, image), packages) = goodtype_typst::with_preset_packages(replay.as_ref(), || {
        let check = goodtype_typst::check_preset_granted(
            root,
            &filename,
            &world_files,
            allow_remote_packages && replay.is_none(),
        )?;
        if !check.rendered {
            return Err(serde_json::to_string(&check).map_err(|e| e.to_string())?);
        }
        let mut page = goodtype_typst::preset_sample_page();
        page.page_typst = Some(format!(
            "#import \"/styles/{filename}\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n{}",
            goodtype_typst::PRESET_SAMPLE
        ));
        let scale = f64::from(edge) / page.width_pt.max(page.height_pt);
        let image = goodtype_typst::export::render_page_png_granted(
            root,
            &page,
            &world_files,
            None,
            Some(scale),
        )
        .map_err(|e| e.to_string())?;
        Ok((check, image))
    })?;
    let mut dependencies: BTreeMap<_, _> =
        files.iter().map(|(p, b)| (p.clone(), digest(b))).collect();
    for (key, (_, bytes)) in &packages.0 {
        dependencies.insert(format!("package:{key}"), digest(bytes));
    }
    let hash = digest(serde_json::to_vec(&json!({"source":source,"filename":filename,"form":check.form,"sampleRevision":check.sample_revision,"dependencies":dependencies})).map_err(|e|e.to_string())?);
    let svg = check.svg.clone().ok_or("missing sample SVG")?;
    let size = svg.len()
        + image.png.len()
        + files.values().map(Vec::len).sum::<usize>()
        + packages.0.values().map(|(_, b)| b.len()).sum::<usize>();
    if size > 24 * 1024 * 1024 {
        return Err("limit: preset review exceeds 24 MiB".into());
    }
    let reviewed = ReviewedPreset {
        source,
        name: String::new(),
        description: None,
        source_url: None,
        candidate_hash: hash,
        dependency_hashes: dependencies,
        check,
        svg,
        png: super::bridge::base64(&image.png),
        filename,
        epoch: 0,
        request_id: String::new(),
        generation: 0,
        files,
        packages,
    };
    Ok((reviewed, image.png, image.width_px, image.height_px))
}
pub(super) fn current(state: &Inner, owner: &str, p: &ReviewedPreset) -> bool {
    let Some(session) = state.sessions.get(owner) else {
        return false;
    };
    if session.conversation.is_none() {
        return p.request_id.is_empty()
            && p.epoch == session.epoch
            && state.grant_current(owner, p.generation, None);
    }
    let Some(request) = session.request.as_deref() else {
        return false;
    };
    request.request_id == p.request_id
        && request.epoch == p.epoch
        && state.grant_current(owner, p.generation, Some(request))
}
impl bridge::Bridge {
    pub(super) async fn preset_tool(
        &self,
        tool: &str,
        args: Value,
        request: Arc<CapturedRequestGrant>,
        generation: u64,
        edge: u32,
        max_text: usize,
    ) -> Result<(Value, Option<(Vec<u8>, u32, u32)>), String> {
        let source = args["source"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= max_text.min(65536))
            .ok_or("invalid source")?
            .to_owned();
        let name = optional(&args, "name", 120)?.unwrap_or_default();
        let description = optional(&args, "description", 2000)?;
        let source_url = optional(&args, "sourceUrl", 2048)?;
        let matched = if tool == "propose_preset" {
            if name.trim().is_empty() {
                return Err("invalid name".into());
            }
            let hash = args["candidateHash"]
                .as_str()
                .ok_or("missing candidateHash")?;
            let s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            Some(
                s.sessions
                    .get(&self.id)
                    .ok_or("session ended")?
                    .preset_previews
                    .iter()
                    .find(|p| {
                        p.candidate_hash == hash && p.source == source && current(&s, &self.id, p)
                    })
                    .cloned()
                    .ok_or("changed: successful matching preview required")?,
            )
        } else {
            None
        };
        if let Some(p) = &matched {
            validate_dependencies(Path::new(&request.root), p)?;
        }
        let root = request.root.clone();
        let files = matched
            .as_ref()
            .map_or_else(|| request.files(), |p| p.files.clone());
        let replay = matched.as_ref().map(|p| p.packages.clone());
        let allow_remote_packages = self
            .app
            .as_ref()
            .is_some_and(|app| app.state::<crate::settings::RemotePackages>().allowed());
        let checked = tauri::async_runtime::spawn_blocking(move || {
            compile(
                Path::new(&root),
                source,
                files,
                replay,
                edge,
                allow_remote_packages,
            )
        })
        .await
        .map_err(|e| e.to_string())?;
        let (mut reviewed, png, width, height) = match checked {
            Ok(value) => value,
            Err(error) if tool == "preset_preview" => {
                let mut value=serde_json::from_str::<Value>(&error).unwrap_or_else(|_|json!({"rendered":false,"diagnostics":[{"severity":"error","message":error}],"sampleRevision":goodtype_typst::PRESET_SAMPLE_REVISION}));
                value["importOrigin"] = json!("/styles/");
                return Ok((value, None));
            }
            Err(error) => return Err(error),
        };
        if matched
            .as_ref()
            .is_some_and(|p| p.candidate_hash != reviewed.candidate_hash)
        {
            return Err("changed: preview dependencies".into());
        }
        reviewed.name = name;
        reviewed.description = description;
        reviewed.source_url = source_url;
        reviewed.request_id = request.request_id.clone();
        reviewed.epoch = request.epoch;
        reviewed.generation = generation;
        let mut s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !current(&s, &self.id, &reviewed) {
            return Err("changed: request revoked".into());
        }
        if tool == "propose_preset" {
            let session = s.sessions.get_mut(&self.id).ok_or("session ended")?;
            if proposals::pending_count(session) >= 20
                || session
                    .proposals
                    .values()
                    .filter(|p| p.kind == "preset_install")
                    .count()
                    >= 20
            {
                return Err("limit: 20 pending proposals".into());
            }
            let p = proposals::Proposal {
                new_page: None,
                cell: None,
                preset: Some(reviewed),
                id: random_hex(16)?,
                session_id: self.id.clone(),
                root: request.root.clone(),
                page_id: request.destination.page.id.clone(),
                version: 1,
                status: "pending".into(),
                text: None,
                changed: vec![],
                summary: args["name"].as_str().unwrap().into(),
                assumptions: vec![],
                edit: false,
                kind: "preset_install".into(),
                page_number: 0,
                overflow: false,
                managed_header: None,
                page_snapshot: None,
                dependencies: vec![],
                baseline_grid: false,
                items: vec![],
                base: request.destination_base.clone(),
                files: Default::default(),
                style: String::new(),
                geometry: request.destination.page.geometry.clone(),
                anchor: request.anchor.crop_pt,
            };
            let result = json!({"id":p.id,"version":p.version,"status":p.status,"kind":p.kind,"candidateHash":p.preset.as_ref().unwrap().candidate_hash});
            proposals::emit(self.app.as_ref(), &p);
            session.proposals.insert(p.id.clone(), p);
            Ok((result, None))
        } else {
            let value = json!({"candidateHash":reviewed.candidate_hash,"dependencyHashes":reviewed.dependency_hashes,"form":reviewed.check.form,"sampleRevision":reviewed.check.sample_revision,"rendered":true,"diagnostics":reviewed.check.diagnostics,"widthPt":reviewed.check.width_pt,"heightPt":reviewed.check.height_pt,"importOrigin":"/styles/"});
            if value.to_string().len() > max_text {
                return Err("limit_reached: preview metadata exceeds usage budget".into());
            }
            let session = s.sessions.get_mut(&self.id).ok_or("session ended")?;
            session.preset_previews.push_front(reviewed);
            session.preset_previews.truncate(4);
            Ok((value, Some((png, width, height))))
        }
    }
}

pub async fn edit(
    helper: &Helper,
    app: Option<&tauri::AppHandle>,
    mut p: proposals::Proposal,
    version: u64,
    sources: Vec<String>,
) -> Result<proposals::Proposal, String> {
    if p.version != version
        || p.status != "pending"
        || sources.len() != 1
        || sources[0].len() > 65536
    {
        return Err("proposal changed".into());
    }
    let old = p.preset.clone().ok_or("missing preset")?;
    let (own, global) = {
        let s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !current(&s, &p.session_id, &old) {
            return Err("changed: request revoked".into());
        }
        (
            s.sessions[&p.session_id]
                .compile
                .clone()
                .try_acquire_owned()
                .map_err(|_| "busy")?,
            helper
                .compile
                .clone()
                .try_acquire_owned()
                .map_err(|_| "busy")?,
        )
    };
    let root = p.root.clone();
    let source = sources[0].clone();
    let saved = old.clone();
    let mut updated = tauri::async_runtime::spawn_blocking(move || {
        let _permits = (own, global);
        compile(
            Path::new(&root),
            source,
            saved.files,
            Some(saved.packages),
            1000,
            false,
        )
        .map(|v| v.0)
    })
    .await
    .map_err(|e| e.to_string())??;
    updated.name = old.name;
    updated.description = old.description;
    updated.source_url = old.source_url;
    updated.request_id = old.request_id;
    updated.epoch = old.epoch;
    updated.generation = old.generation;
    p.preset = Some(updated);
    p.version += 1;
    let mut s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
    if !current(&s, &p.session_id, p.preset.as_ref().unwrap()) {
        return Err("changed: request revoked".into());
    }
    let stored = proposals::find_mut(&mut s, &p.id)?;
    if stored.version != version || stored.status != "pending" {
        return Err("proposal changed".into());
    }
    *stored = p.clone();
    proposals::emit(app, &p);
    Ok(p)
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallPreset {
    pub proposal_id: String,
    pub proposal_version: u64,
    pub candidate_hash: String,
    pub session_id: String,
    pub root: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub status: String,
    pub path: String,
    pub wrote: bool,
}

// Caller holds notebook lock; hold helper lock through validation and new-only persistence.
pub fn install(
    helper: &Helper,
    app: Option<&tauri::AppHandle>,
    root: &Path,
    input: InstallPreset,
) -> Result<InstallResult, String> {
    let mut s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
    let p = s
        .sessions
        .get(&input.session_id)
        .and_then(|s| s.proposals.get(&input.proposal_id))
        .ok_or("unknown proposal owner")?
        .clone();
    let reviewed = p.preset.as_ref().ok_or("not a preset proposal")?;
    if p.kind != "preset_install"
        || p.session_id != input.session_id
        || p.root != input.root
        || Path::new(&p.root)
            .canonicalize()
            .map_err(|e| e.to_string())?
            != root
        || p.version != input.proposal_version
        || reviewed.candidate_hash != input.candidate_hash
        || !current(&s, &p.session_id, reviewed)
    {
        return Err("changed: preset review or authorization".into());
    }
    if p.status == "installed" || p.status == "already_installed" {
        return Ok(InstallResult {
            status: p.status,
            path: format!("styles/{}", reviewed.filename),
            wrote: false,
        });
    }
    if p.status != "pending" {
        return Err("proposal is final or stale".into());
    }
    let _own = s.sessions[&p.session_id]
        .compile
        .clone()
        .try_acquire_owned()
        .map_err(|_| "busy")?;
    let _global = helper
        .compile
        .clone()
        .try_acquire_owned()
        .map_err(|_| "busy")?;
    let _pinned = pin_dependencies(root, reviewed)?;
    let (checked, _, _, _) = compile(
        root,
        reviewed.source.clone(),
        reviewed.files.clone(),
        Some(reviewed.packages.clone()),
        1000,
        false,
    )?;
    if checked.candidate_hash != reviewed.candidate_hash {
        return Err("changed: preset compile".into());
    }
    // Re-check after compile; compilation itself uses only captured bytes.
    validate_dependencies(root, reviewed)?;
    let (path, wrote) =
        storage::install_reviewed_typst_style(root, &reviewed.filename, &reviewed.source)
            .map_err(|e| e.to_string())?;
    let stored = proposals::find_mut(&mut s, &p.id)?;
    stored.status = if wrote {
        "installed"
    } else {
        "already_installed"
    }
    .into();
    proposals::emit(app, stored);
    if wrote && let Some(app) = app {
        crate::preset::notify(
            app,
            &[crate::preset::preset_change(
                &input.root,
                &path,
                reviewed.source.as_bytes(),
            )],
        );
    }
    Ok(InstallResult {
        status: stored.status.clone(),
        path,
        wrote,
    })
}
// Windows dependency handles deny writes/deletion through the new-file commit, closing the
// external-editor precheck/write gap as well as serializing Goodtype writers with notebook lock.
fn pin_dependencies(root: &Path, p: &ReviewedPreset) -> Result<Vec<std::fs::File>, String> {
    use std::io::Read;
    let mut paths = Vec::new();
    for (path, bytes) in &p.files {
        let path = root.join(path.trim_start_matches('/'));
        let path = goodtype_core::paths::contained(root, &path, "preset dependency")
            .map_err(|e| e.to_string())?;
        paths.push((path, bytes));
    }
    for (path, bytes) in p.packages.0.values() {
        paths.push((path.clone(), bytes));
    }
    let mut handles = Vec::new();
    for (path, expected) in paths {
        storage::files::reject_symlink(&path).map_err(|e| e.to_string())?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1); // FILE_SHARE_READ only.
        }
        let mut file = options.open(&path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(expected.len() as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes != *expected {
            return Err("changed: preset dependency".into());
        }
        handles.push(file);
    }
    Ok(handles)
}
pub(super) fn validate_dependencies(root: &Path, p: &ReviewedPreset) -> Result<(), String> {
    for (path, bytes) in &p.files {
        let path = root.join(path.trim_start_matches('/'));
        let path = goodtype_core::paths::contained(root, &path, "preset dependency")
            .map_err(|e| e.to_string())?;
        if goodtype_core::storage::files::read_bounded(&path, bytes.len())
            .map_err(|e| e.to_string())?
            != *bytes
        {
            return Err("changed: preset dependency".into());
        }
    }
    for (path, bytes) in p.packages.0.values() {
        goodtype_core::storage::files::reject_symlink(path).map_err(|e| e.to_string())?;
        if goodtype_core::storage::files::read_bounded(path, bytes.len())
            .map_err(|e| e.to_string())?
            != *bytes
        {
            return Err("changed: cached package dependency".into());
        }
    }
    Ok(())
}
#[tauri::command]
pub async fn helper_install_preset(
    app: tauri::AppHandle,
    helper: tauri::State<'_, Helper>,
    roots: tauri::State<'_, crate::workspace::AllowedRoots>,
    histories: tauri::State<'_, crate::notebook::NotebookHistories>,
    input: InstallPreset,
) -> Result<InstallResult, String> {
    let root = crate::workspace::ensure_allowed(&roots, &input.root)?;
    let helper = helper.inner().clone();
    let histories = histories.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::notebook::with_notebook(&histories, || install(&helper, Some(&app), &root, input))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn install_command_uses_registered_notebook_root_state() {
        // Tauri resolves managed state by exact type; main registers AllowedRoots, not Arc<AllowedRoots>.
        fn accepts_registered_state<'a, F, Fut>(_: F)
        where
            F: FnOnce(
                tauri::AppHandle,
                tauri::State<'a, Helper>,
                tauri::State<'a, crate::workspace::AllowedRoots>,
                tauri::State<'a, crate::notebook::NotebookHistories>,
                InstallPreset,
            ) -> Fut,
        {
        }
        accepts_registered_state(helper_install_preset);
    }

    #[test]
    fn candidate_never_overwrites_granted_path_and_package_changes_are_stale() {
        let root = tempfile::tempdir().unwrap();
        let files = GrantedFiles::from([(
            "/styles/helper-candidate-0.typ".into(),
            b"Granted original".to_vec(),
        )]);
        let source = "#include \"helper-candidate-0.typ\"";
        let (mut p, _, _, _) =
            compile(root.path(), source.into(), files, None, 400, false).unwrap();
        assert_eq!(p.filename, "helper-candidate-1.typ");
        assert_eq!(
            p.files["/styles/helper-candidate-0.typ"],
            b"Granted original"
        );
        std::fs::create_dir(root.path().join("styles")).unwrap();
        std::fs::write(
            root.path().join("styles/helper-candidate-0.typ"),
            b"Granted original",
        )
        .unwrap();
        let path = root.path().join("cache-file");
        std::fs::write(&path, b"cached package").unwrap();
        p.packages
            .0
            .insert("fixture".into(), (path.clone(), b"cached package".to_vec()));
        assert!(validate_dependencies(&root.path().canonicalize().unwrap(), &p).is_ok());
        #[cfg(windows)]
        {
            let pinned = pin_dependencies(&root.path().canonicalize().unwrap(), &p).unwrap();
            assert!(std::fs::write(&path, b"concurrent edit").is_err());
            assert!(std::fs::remove_file(&path).is_err());
            drop(pinned);
        }
        std::fs::write(path, b"changed").unwrap();
        assert!(validate_dependencies(&root.path().canonicalize().unwrap(), &p).is_err());
    }
}
