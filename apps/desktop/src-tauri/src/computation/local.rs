//! Local candidates are copied before any interpreter code runs. Host files are never changed.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub executable: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub code: String,
    pub missing: Vec<String>,
    pub provenance: Value,
    pub selected: bool,
}
fn candidate(path: PathBuf) -> Option<Candidate> {
    for ancestor in path.ancestors() {
        plain(ancestor).ok()?;
    }
    let path = path.canonicalize().ok()?;
    if !path.is_file() || path.file_name()?.to_str()?.to_lowercase() != "python.exe" {
        return None;
    }
    let executable = path.to_str()?.to_owned();
    let id = format!(
        "local-{:x}",
        Sha256::digest(executable.to_lowercase().as_bytes())
    );
    Some(Candidate { id, executable })
}
#[tauri::command]
pub fn python_runtime_discover(
    state: tauri::State<'_, RuntimeState>,
) -> Result<Vec<Candidate>, String> {
    let found = discover();
    *state.candidates.lock().map_err(|e| e.to_string())? = found.clone();
    Ok(found)
}
fn discover() -> Vec<Candidate> {
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(
            std::env::split_paths(&path)
                .take(256)
                .map(|p| p.join("python.exe")),
        );
    }
    for key in [
        "LOCALAPPDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "USERPROFILE",
    ] {
        if let Some(root) = std::env::var_os(key) {
            let root = PathBuf::from(root);
            for base in [root.clone(), root.join("Programs/Python")] {
                paths.push(base.join("python.exe"));
                if let Ok(entries) = fs::read_dir(base) {
                    for entry in entries.take(256).flatten() {
                        let name = entry.file_name().to_string_lossy().to_lowercase();
                        if name.starts_with("python") || name.contains("conda") {
                            paths.push(entry.path().join("python.exe"));
                        }
                    }
                }
            }
        }
    }
    #[cfg(windows)]
    paths.extend(registry_paths());
    let mut found: Vec<_> = paths.into_iter().filter_map(candidate).collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found.dedup_by(|a, b| a.id == b.id);
    found.truncate(64);
    found
}
#[cfg(windows)]
fn registry_paths() -> Vec<PathBuf> {
    use std::ffi::c_void;
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegOpenKeyExW(
            h: *mut c_void,
            sub: *const u16,
            options: u32,
            access: u32,
            out: *mut *mut c_void,
        ) -> i32;
        fn RegEnumKeyExW(
            h: *mut c_void,
            index: u32,
            name: *mut u16,
            len: *mut u32,
            res: *mut u32,
            class: *mut u16,
            classlen: *mut u32,
            time: *mut c_void,
        ) -> i32;
        fn RegQueryValueExW(
            h: *mut c_void,
            name: *const u16,
            res: *mut u32,
            kind: *mut u32,
            data: *mut u8,
            len: *mut u32,
        ) -> i32;
        fn RegCloseKey(h: *mut c_void) -> i32;
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    unsafe fn children(root: *mut c_void, path: &str, view: u32) -> Vec<String> {
        let mut h = std::ptr::null_mut();
        let mut result = Vec::new();
        unsafe {
            if RegOpenKeyExW(root, wide(path).as_ptr(), 0, 0x20019 | view, &mut h) != 0 {
                return result;
            }
            for i in 0..128 {
                let mut name = [0u16; 256];
                let mut len = 256;
                if RegEnumKeyExW(
                    h,
                    i,
                    name.as_mut_ptr(),
                    &mut len,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ) != 0
                {
                    break;
                }
                result.push(String::from_utf16_lossy(&name[..len as usize]));
            }
            RegCloseKey(h);
        }
        result
    }
    let mut paths = Vec::new();
    unsafe {
        for root in [0x80000001u32, 0x80000002u32] {
            let root = root as i32 as isize as *mut c_void;
            for view in [0x100, 0x200] {
                for company in children(root, "Software\\Python", view) {
                    let base = format!("Software\\Python\\{company}");
                    for tag in children(root, &base, view) {
                        let key = format!("{base}\\{tag}\\InstallPath");
                        let mut h = std::ptr::null_mut();
                        if RegOpenKeyExW(root, wide(&key).as_ptr(), 0, 0x20019 | view, &mut h) == 0
                        {
                            let mut data = [0u16; 2048];
                            let mut bytes = 4096;
                            let mut kind = 0;
                            if RegQueryValueExW(
                                h,
                                std::ptr::null(),
                                std::ptr::null_mut(),
                                &mut kind,
                                data.as_mut_ptr().cast(),
                                &mut bytes,
                            ) == 0
                                && kind == 1
                                && bytes <= 4096
                            {
                                let s = String::from_utf16_lossy(&data[..bytes as usize / 2]);
                                paths.push(
                                    PathBuf::from(s.trim_end_matches('\0')).join("python.exe"),
                                );
                            }
                            RegCloseKey(h);
                        }
                    }
                }
            }
        }
    }
    paths
}
fn plain(path: &Path) -> Result<fs::Metadata, String> {
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err("unsafe_candidate".into());
        }
    }
    if m.file_type().is_symlink() || (!m.is_file() && !m.is_dir()) {
        return Err("unsafe_candidate".into());
    }
    Ok(m)
}
fn copy_tree(source: &Path, target: &Path, state: &RuntimeState) -> Result<(), String> {
    // Validate every ancestor as well as each copied entry. No links are followed deliberately.
    for ancestor in source.ancestors() {
        plain(ancestor)?;
    }
    let mut pending = vec![(source.to_path_buf(), target.to_path_buf(), 0)];
    let mut files = 0;
    let mut bytes = 0u64;
    while let Some((src, dst, depth)) = pending.pop() {
        if state.cancelled.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        if depth > 24 {
            return Err("copy_limit".into());
        }
        let m = plain(&src)?;
        files += 1;
        if files > 60000 {
            return Err("copy_limit".into());
        }
        if m.is_dir() {
            fs::create_dir_all(&dst).map_err(|e| e.to_string())?;
            for entry in fs::read_dir(&src).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                if pending.len() >= 60000 {
                    return Err("copy_limit".into());
                }
                pending.push((entry.path(), dst.join(entry.file_name()), depth + 1));
            }
        } else {
            bytes = bytes.checked_add(m.len()).ok_or("copy_limit")?;
            if bytes > 2 * 1024 * 1024 * 1024 {
                return Err("copy_limit".into());
            }
            let mut input = fs::File::open(&src).map_err(|e| e.to_string())?;
            let mut output = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&dst)
                .map_err(|e| e.to_string())?;
            let mut remaining = m.len();
            let mut buffer = [0u8; 65536];
            while remaining > 0 {
                if state.cancelled.load(Ordering::SeqCst) {
                    return Err("cancelled".into());
                }
                let n = input
                    .read(&mut buffer[..remaining.min(65536) as usize])
                    .map_err(|e| e.to_string())?;
                if n == 0 {
                    return Err("candidate_changed".into());
                }
                output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
                remaining -= n as u64;
            }
            if input.read(&mut buffer[..1]).map_err(|e| e.to_string())? != 0 {
                return Err("candidate_changed".into());
            }
        }
    }
    Ok(())
}
#[derive(Deserialize, Serialize)]
struct Selection {
    directory: String,
    provenance: Value,
}
pub(super) fn selected(base: &Path) -> Option<PathBuf> {
    let canonical = base.canonicalize().ok()?;
    let base = canonical.as_path();
    let bytes = crate::store::read_bounded(&base.join("selected-local.json"), 65536).ok()?;
    let selection: Selection = serde_json::from_slice(&bytes).ok()?;
    if !selection.directory.starts_with("local-")
        || selection.directory.len() != 70
        || !selection.directory[6..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    let path = base.join(selection.directory);
    goodtype_core::paths::resolve_dir(base, &path, "local Python").ok()
}
/// Drop the selected local Python: the selection first, so a half-deleted copy is never used, then
/// Goodtype's own copy. The user's installation was only ever read and is not touched.
pub(super) fn forget(base: &Path) -> Result<bool, String> {
    let Some(copy) = selected(base) else {
        return Ok(false);
    };
    let base = base.canonicalize().map_err(|e| e.to_string())?;
    fs::remove_file(base.join("selected-local.json")).map_err(|e| e.to_string())?;
    // `selected` only returns a validated `local-<hash>` directory contained in `base`.
    fs::remove_dir_all(&copy).map_err(|e| e.to_string())?;
    Ok(true)
}
#[tauri::command]
pub async fn python_runtime_probe(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
    candidate_id: String,
    select: bool,
) -> Result<ProbeResult, String> {
    let candidate = state
        .candidates
        .lock()
        .map_err(|e| e.to_string())?
        .iter()
        .find(|c| c.id == candidate_id)
        .cloned()
        .ok_or("unknown_candidate")?;
    if state.installing.swap(true, Ordering::SeqCst) {
        return Err("busy".into());
    }
    state.cancelled.store(false, Ordering::SeqCst);
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        probe(
            &runtime_base(&handle)?,
            &handle.state::<RuntimeState>(),
            candidate,
            select,
        )
    })
    .await
    .map_err(|e| e.to_string());
    state.installing.store(false, Ordering::SeqCst);
    *state.progress.lock().map_err(|e| e.to_string())? = String::new();
    result?
}
fn probe(
    base: &Path,
    state: &RuntimeState,
    candidate: Candidate,
    select: bool,
) -> Result<ProbeResult, String> {
    *state.progress.lock().map_err(|e| e.to_string())? = "local-copy".into();
    let source = PathBuf::from(&candidate.executable);
    let root = source.parent().ok_or("unsafe_candidate")?;
    let stage = tempfile::Builder::new()
        .prefix("reuse-")
        .tempdir_in(base)
        .map_err(|e| e.to_string())?;
    copy_tree(root, stage.path(), state)?;
    // Explicit isolated paths: do not import site, execute .pth files, or consult host registry paths.
    let mut dlls = Vec::new();
    for entry in fs::read_dir(stage.path()).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("python3") && name.ends_with(".dll") && name != "python3.dll" {
            dlls.push(name);
        }
    }
    if dlls.len() != 1 {
        return Ok(ProbeResult {
            code: "incompatible".into(),
            missing: vec![],
            provenance: json!({}),
            selected: false,
        });
    }
    let stem = dlls[0].trim_end_matches(".dll");
    fs::write(
        stage.path().join(format!("{stem}._pth")),
        format!("{stem}.zip\n.\nLib\nDLLs\nLib/site-packages\nsite-packages\n"),
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        stage.path().join("python._pth"),
        format!("{stem}.zip\n.\nLib\nDLLs\nLib/site-packages\nsite-packages\n"),
    )
    .map_err(|e| e.to_string())?;
    if state.cancelled.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    *state.progress.lock().map_err(|e| e.to_string())? = "local-probe".into();
    let missing_on_disk: Vec<String> =
        ["IPython", "numpy", "scipy", "sympy", "pandas", "matplotlib"]
            .into_iter()
            .filter(|name| {
                !["site-packages", "Lib/site-packages"]
                    .iter()
                    .any(|directory| {
                        stage
                            .path()
                            .join(directory)
                            .join(name)
                            .join("__init__.py")
                            .is_file()
                    })
            })
            .map(str::to_owned)
            .collect();
    #[cfg(windows)]
    let started = Session::new_cancellable(stage.path(), Some(&state.cancelled));
    #[cfg(not(windows))]
    let started = Session::new(stage.path());
    if state.cancelled.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    let session = match started {
        Ok(s) => s,
        Err(_) => {
            return Ok(ProbeResult {
                code: "incompatible".into(),
                missing: vec![],
                provenance: json!({}),
                selected: false,
            });
        }
    };
    let done = AtomicBool::new(false);
    let execution = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !done.load(Ordering::SeqCst) {
                if state.cancelled.load(Ordering::SeqCst) {
                    session.stop();
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        });
        let result = session.execute("probe", "import math\nprint(math.isqrt(1764))", false);
        done.store(true, Ordering::SeqCst);
        result
    });
    let (response, _, _) = execution?;
    let probe_output = response["outputs"]
        .as_array()
        .and_then(|a| a.iter().find(|o| o["output_type"] == "stream"))
        .and_then(|o| o["text"].as_str())
        .ok_or("incompatible")?;
    if probe_output.trim() != "42" {
        return Err("incompatible".into());
    }
    let provenance = json!({"source":candidate.executable,"runtime":session.provenance});
    session.stop();
    drop(session);
    if state.cancelled.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    if select {
        let destination = base.join(&candidate.id);
        if destination.exists() {
            return Err("environment_exists".into());
        }
        fs::rename(stage.path(), &destination).map_err(|e| e.to_string())?;
        crate::store::write_atomic(
            &base.join("selected-local.json"),
            &serde_json::to_vec(&Selection {
                directory: candidate.id,
                provenance: provenance.clone(),
            })
            .map_err(|e| e.to_string())?,
        )?;
    }
    Ok(ProbeResult {
        code: "compatible".into(),
        missing: missing_on_disk,
        provenance,
        selected: select,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copying_is_bounded_and_cancelled() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        fs::write(src.path().join("x"), b"original").unwrap();
        let state = RuntimeState::default();
        copy_tree(src.path(), dst.path(), &state).unwrap();
        assert_eq!(fs::read(dst.path().join("x")).unwrap(), b"original");
        state.cancelled.store(true, Ordering::SeqCst);
        assert_eq!(
            copy_tree(src.path(), dst.path(), &state).unwrap_err(),
            "cancelled"
        );
    }
    #[test]
    #[ignore = "requires an explicitly selected plain installation in GOODTYPE_LOCAL_PYTHON_TEST_RUNTIME"]
    fn plain_local_python_runs_cells_without_the_scientific_bundle() {
        let root = PathBuf::from(std::env::var_os("GOODTYPE_LOCAL_PYTHON_TEST_RUNTIME").unwrap());
        let candidate = candidate(root.join("python.exe")).unwrap();
        let before = fs::read(root.join("python.exe")).unwrap();
        let base = tempfile::tempdir().unwrap();
        let state = RuntimeState::default();
        state.downloads.store(false, Ordering::SeqCst);
        let result = probe(base.path(), &state, candidate, true).unwrap();
        assert!(result.selected);
        assert_eq!(result.provenance["runtime"]["engine"], "python");
        assert!(result.missing.iter().any(|name| name == "numpy"));
        let session = Session::new(&selected(base.path()).unwrap()).unwrap();
        let (response, count, _) = session
            .execute("first", "import math as m\nx = m.sqrt(81)\nx", false)
            .unwrap();
        assert_eq!(count, 1);
        assert!(response["outputs"].to_string().contains("9.0"));
        let (response, count, _) = session.execute("next", "x * 2", false).unwrap();
        assert_eq!(count, 2);
        assert!(response["outputs"].to_string().contains("18.0"));
        let (response, _, _) = session
            .execute(
                "missing",
                "import goodtype_deliberately_missing_library",
                false,
            )
            .unwrap();
        assert!(
            response["outputs"]
                .to_string()
                .contains("ModuleNotFoundError")
        );
        assert_eq!(fs::read(root.join("python.exe")).unwrap(), before);
        session.stop();
    }
    #[test]
    #[ignore = "requires GOODTYPE_PYTHON_TEST_RUNTIME"]
    fn prepared_runtime_can_be_reused_offline() {
        let runtime = PathBuf::from(std::env::var_os("GOODTYPE_PYTHON_TEST_RUNTIME").unwrap());
        let candidate = candidate(runtime.join("python.exe")).unwrap();
        let base = tempfile::tempdir().unwrap();
        let state = RuntimeState::default();
        state.downloads.store(false, Ordering::SeqCst);
        let result = probe(base.path(), &state, candidate, true).unwrap();
        assert_eq!(result.code, "compatible");
        assert!(result.selected);
        assert!(result.missing.is_empty());
        assert!(selected(base.path()).unwrap().join("python.exe").is_file());
        assert!(result.provenance["runtime"]["packages"]["numpy"].is_string());
    }
    #[test]
    fn nesting_is_bounded() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let mut path = src.path().to_path_buf();
        for _ in 0..26 {
            path = path.join("d");
            fs::create_dir(&path).unwrap();
        }
        assert_eq!(
            copy_tree(src.path(), dst.path(), &RuntimeState::default()).unwrap_err(),
            "copy_limit"
        );
    }
    #[test]
    fn selection_rejects_traversal() {
        let base = tempfile::tempdir().unwrap();
        fs::write(
            base.path().join("selected-local.json"),
            br#"{"directory":"local-../../escape","provenance":{}}"#,
        )
        .unwrap();
        assert!(selected(base.path()).is_none());
    }
    #[test]
    fn forgetting_removes_only_the_selected_copy() {
        let base = tempfile::tempdir().unwrap();
        let name = format!("local-{}", "a".repeat(64));
        fs::create_dir(base.path().join(&name)).unwrap();
        fs::write(base.path().join(&name).join("python.exe"), b"copy").unwrap();
        fs::create_dir(base.path().join("keep")).unwrap();
        let select = |directory: &str| {
            fs::write(
                base.path().join("selected-local.json"),
                serde_json::to_vec(&json!({"directory": directory, "provenance": {}})).unwrap(),
            )
            .unwrap()
        };
        select("keep");
        assert!(!forget(base.path()).unwrap());
        assert!(base.path().join("keep").is_dir());
        select(&name);
        assert!(forget(base.path()).unwrap());
        assert!(!base.path().join(&name).exists());
        assert!(!base.path().join("selected-local.json").exists());
        assert!(base.path().join("keep").is_dir());
        assert!(!forget(base.path()).unwrap());
    }
}
