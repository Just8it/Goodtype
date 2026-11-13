//! Restricted, optional Python sessions. Saved notebook content never depends on a live process.
mod acquisition;
pub mod local;

#[cfg(windows)]
mod windows;

use crate::{
    notebook::{NotebookError, NotebookHistories, on_notebook},
    workspace::{AllowedRoots, ensure_allowed},
};
use goodtype_core::{calculation::Calculation, storage};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::Manager;

pub struct RuntimeState {
    pub downloads: AtomicBool,
    candidates: Mutex<Vec<local::Candidate>>,
    lifecycle: AtomicU64,
    installing: AtomicBool,
    cancelled: AtomicBool,
    progress: Mutex<String>,
    sessions: Mutex<HashMap<(PathBuf, String), Arc<Session>>>,
}
impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            candidates: Mutex::new(Vec::new()),
            lifecycle: AtomicU64::new(0),
            downloads: AtomicBool::new(true),
            installing: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            progress: Mutex::new(String::new()),
            sessions: Mutex::new(HashMap::new()),
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    installed: bool,
    installing: bool,
    downloads_allowed: bool,
    progress: String,
    download_bytes: u64,
}
fn runtime_base(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let data = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&data).map_err(|error| error.to_string())?;
    let data = data.canonicalize().map_err(|error| error.to_string())?;
    goodtype_core::paths::ensure_dir(&data, &data.join("python"), "python")
        .map_err(|error| error.to_string())
}
fn installed_runtime(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let base = runtime_base(app)?;
    if let Some(path) = local::selected(&base) {
        return Ok(path);
    }
    let path = base.join(acquisition::runtime_id());
    if !acquisition::installed(&path) {
        return Err("Python runtime is not installed".into());
    }
    goodtype_core::paths::resolve_dir(&base, &path, "Python runtime")
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn python_runtime_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> Result<RuntimeStatus, String> {
    Ok(RuntimeStatus {
        installed: installed_runtime(&app).is_ok(),
        installing: state.installing.load(Ordering::SeqCst),
        downloads_allowed: state.downloads.load(Ordering::SeqCst),
        progress: state
            .progress
            .lock()
            .map_err(|error| error.to_string())?
            .clone(),
        download_bytes: acquisition::download_bytes(),
    })
}
#[tauri::command]
pub async fn python_runtime_install(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> Result<(), String> {
    if !state.downloads.load(Ordering::SeqCst) {
        return Err("Python package downloads are disabled".into());
    }
    if state.installing.swap(true, Ordering::SeqCst) {
        return Err("Python installation is already running".into());
    }
    state.cancelled.store(false, Ordering::SeqCst);
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = handle.state::<RuntimeState>();
        acquisition::install(&runtime_base(&handle)?, &state)
    })
    .await
    .map_err(|error| error.to_string());
    state.installing.store(false, Ordering::SeqCst);
    result?
}
/// Forget the selected local Python so the next run asks again. Live sessions use that copy, so
/// they end first; saved outputs stay. Returns whether a copy was removed.
#[tauri::command]
pub async fn python_runtime_forget_local(
    app: tauri::AppHandle,
    state: tauri::State<'_, RuntimeState>,
) -> Result<bool, String> {
    if state.installing.load(Ordering::SeqCst) {
        return Err("Python installation is already running".into());
    }
    let removed = {
        let mut sessions = state.sessions.lock().map_err(|error| error.to_string())?;
        state.lifecycle.fetch_add(1, Ordering::SeqCst);
        sessions
            .drain()
            .map(|(_, session)| session)
            .collect::<Vec<_>>()
    };
    for session in &removed {
        session.stop();
    }
    drop(removed);
    let base = runtime_base(&app)?;
    tauri::async_runtime::spawn_blocking(move || local::forget(&base))
        .await
        .map_err(|error| error.to_string())?
}
#[tauri::command]
pub fn python_runtime_cancel(state: tauri::State<'_, RuntimeState>) {
    state.cancelled.store(true, Ordering::SeqCst);
}

struct Session {
    #[cfg(windows)]
    process: windows::Process,
    input: PathBuf,
    output: PathBuf,
    provenance: Value,
    sequence: AtomicU64,
    generation: AtomicU64,
    running: AtomicBool,
    _directory: tempfile::TempDir,
}
impl Session {
    #[cfg(windows)]
    fn new(runtime: &Path) -> Result<Self, String> {
        Self::new_cancellable(runtime, None)
    }
    #[cfg(windows)]
    fn new_cancellable(runtime: &Path, cancelled: Option<&AtomicBool>) -> Result<Self, String> {
        let directory = tempfile::Builder::new()
            .prefix("goodtype-python-")
            .tempdir()
            .map_err(|error| error.to_string())?;
        let input = directory.path().join("input");
        let output = directory.path().join("output");
        std::fs::create_dir(&input).map_err(|error| error.to_string())?;
        std::fs::create_dir(&output).map_err(|error| error.to_string())?;
        std::fs::write(input.join("runner.py"), include_str!("runner.py"))
            .map_err(|error| error.to_string())?;
        let process = windows::Process::start(runtime, &input, &output)?;
        let started = Instant::now();
        let ready = output.join("ready.json");
        while !ready.exists() {
            if cancelled.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
                return Err("cancelled".into());
            }
            directory_bytes(&output, 64 * 1024 * 1024)?;
            if !process.running() || started.elapsed() > Duration::from_secs(30) {
                return Err("Restricted Python startup failed or timed out".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let bytes = loop {
            match storage::files::read_bounded(&ready, 64 * 1024) {
                Ok(bytes) => break bytes,
                Err(error)
                    if matches!(error.raw_os_error(), Some(5 | 32))
                        && started.elapsed() < Duration::from_secs(30) =>
                {
                    std::thread::sleep(Duration::from_millis(15))
                }
                Err(error) => return Err(error.to_string()),
            }
        };
        let provenance: Value =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if provenance.get("protocol") != Some(&json!(1)) || provenance.get("error").is_some() {
            return Err(format!("Python startup: {provenance}"));
        }
        Ok(Self {
            process,
            input,
            output,
            provenance,
            sequence: AtomicU64::new(0),
            generation: AtomicU64::new(1),
            running: AtomicBool::new(false),
            _directory: directory,
        })
    }
    #[cfg(not(windows))]
    fn new(_runtime: &Path) -> Result<Self, String> {
        Err("Restricted Python currently requires Windows".into())
    }
    fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        #[cfg(windows)]
        self.process.stop();
    }
    fn alive(&self) -> bool {
        #[cfg(windows)]
        {
            self.process.running()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
    fn execute(&self, cell: &str, source: &str, german: bool) -> Result<(Value, u64, u64), String> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err("This calculation is already running".into());
        }
        let result = self.execute_inner(cell, source, german);
        self.running.store(false, Ordering::SeqCst);
        result
    }
    fn execute_inner(
        &self,
        cell: &str,
        source: &str,
        german: bool,
    ) -> Result<(Value, u64, u64), String> {
        let generation = self.generation.load(Ordering::SeqCst);
        let request = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let response = self.output.join("response.json");
        if response.exists() {
            std::fs::remove_file(&response).map_err(|error| error.to_string())?;
        }
        let shortened_msg = if german {
            "Ausgabe wegen Größenlimit gekürzt."
        } else {
            "Output shortened at the size limit."
        };
        if source.len() > 1024 * 1024 {
            return Err("Cell source exceeds the protocol limit".into());
        }
        let payload =
            serde_json::to_vec(&json!({"requestId":request,"cellId":cell,"source":source,
            "shortenedMessage":shortened_msg}))
            .map_err(|error| error.to_string())?;
        storage::files::persist_atomic(&self.input.join("request.json"), &payload, true)
            .map_err(|error| error.to_string())?;
        let started = Instant::now();
        loop {
            if self.generation.load(Ordering::SeqCst) != generation || !self.alive() {
                return Err(
                    "Python session ended. Variables were lost. Saved outputs remain.".into(),
                );
            }
            if started.elapsed() > Duration::from_secs(300) {
                self.stop();
                return Err(
                    "Python exceeded the five-minute cell limit. Variables were lost.".into(),
                );
            }
            if directory_bytes(&self.output, 64 * 1024 * 1024).is_err() {
                self.stop();
                return Err("Python exceeded the scratch size limit. Variables were lost.".into());
            }
            if response.exists() {
                let mut attempts = 0;
                let bytes = loop {
                    match storage::files::read_bounded(&response, 16 * 1024 * 1024) {
                        Ok(bytes) => break Ok(bytes),
                        Err(error)
                            if attempts < 10 && matches!(error.raw_os_error(), Some(5 | 32)) =>
                        {
                            attempts += 1;
                            std::thread::sleep(Duration::from_millis(15));
                        }
                        Err(error) => break Err(error),
                    }
                }
                .map_err(|error| {
                    self.stop();
                    error.to_string()
                })?;
                let result: Value = serde_json::from_slice(&bytes).map_err(|error| {
                    self.stop();
                    error.to_string()
                })?;
                if result.get("protocol") != Some(&json!(1))
                    || result.get("requestId") != Some(&json!(request))
                    || result.get("cellId").and_then(Value::as_str) != Some(cell)
                    || result.get("fatal").is_some()
                {
                    self.stop();
                    return Err("Python returned an invalid response. Session ended.".into());
                }
                let sequence = result
                    .get("sequence")
                    .and_then(Value::as_u64)
                    .filter(|value| *value > 0)
                    .ok_or("Python returned an invalid execution count")?;
                return Ok((result, sequence, generation));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
fn directory_bytes(root: &Path, limit: u64) -> Result<u64, String> {
    let mut total = 0u64;
    let mut count = 0;
    let mut pending = vec![(root.to_path_buf(), 0)];
    while let Some((directory, depth)) = pending.pop() {
        if depth > 16 {
            return Err("Python scratch directory nesting exceeds the limit".into());
        }
        for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
            count += 1;
            if count > 5000 {
                return Err("Python scratch contains too many entries".into());
            }
            let path = entry.map_err(|error| error.to_string())?.path();
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.to_string()),
            };
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err("Python scratch contains a reparse point".into());
                }
            }
            if metadata.file_type().is_symlink() {
                return Err("Python scratch contains a symbolic link".into());
            }
            if metadata.is_dir() {
                pending.push((path, depth + 1));
            } else {
                total = total
                    .checked_add(metadata.len())
                    .ok_or("Python scratch size overflow")?;
            }
            if total > limit {
                return Err("Python scratch exceeds the size limit".into());
            }
        }
    }
    Ok(total)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri injects the privileged app state beside the cell request.
pub async fn run_calculation_cell(
    app: tauri::AppHandle,
    roots: tauri::State<'_, AllowedRoots>,
    histories: tauri::State<'_, NotebookHistories>,
    root: String,
    page_id: String,
    cell_id: String,
    modified_at: String,
    ui_language: Option<String>,
) -> Result<storage::CalculationResult, NotebookError> {
    let lifecycle = app.state::<RuntimeState>().lifecycle.load(Ordering::SeqCst);
    let target = root.clone();
    let page = page_id.clone();
    let identity = cell_id.clone();
    let (path, calculation_id, source) = on_notebook(&roots, &histories, target, move |root, _| {
        let snapshot = storage::open_page(root, &page)?;
        let reference = snapshot
            .page
            .calculation
            .ok_or("page is not computational")?;
        let file = snapshot
            .calculations
            .iter()
            .find(|file| file.path == reference.source_path)
            .ok_or("calculation source is missing")?;
        let document = Calculation::parse(&file.bytes)?;
        let cell = document
            .cells
            .iter()
            .find(|cell| {
                cell.id == identity
                    && cell.part_id() == Some(&reference.part_id)
                    && cell.cell_type == "code"
            })
            .ok_or("code cell does not belong to this page")?;
        Ok((root.to_path_buf(), reference.id, cell.source.clone()))
    })
    .await?;
    let handle = app.clone();
    let calc = calculation_id.clone();
    let expected_root = path.clone();
    let (session, response, sequence, generation) =
        tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
            let state = handle.state::<RuntimeState>();
            if state.lifecycle.load(Ordering::SeqCst) != lifecycle {
                return Err("Notebook session lifecycle changed before Python startup".into());
            }
            let key = (path, calc);
            let existing = {
                let mut sessions = state.sessions.lock().map_err(|error| error.to_string())?;
                sessions.retain(|_, s| s.alive());
                sessions.get(&key).filter(|s| s.alive()).cloned()
            };
            let session = if let Some(s) = existing {
                s
            } else {
                let runtime_dir = installed_runtime(&handle)?;
                let new_session = Arc::new(Session::new(&runtime_dir)?);
                let mut sessions = state.sessions.lock().map_err(|error| error.to_string())?;
                if state.lifecycle.load(Ordering::SeqCst) != lifecycle {
                    return Err("Notebook session lifecycle changed during Python startup".into());
                }
                sessions.retain(|_, s| s.alive());
                if let Some(s) = sessions.get(&key).filter(|s| s.alive()) {
                    s.clone()
                } else {
                    if sessions.len() >= 4 {
                        return Err(
                            "Four Python sessions are active. End one before starting another."
                                .into(),
                        );
                    }
                    sessions.insert(key, new_session.clone());
                    new_session
                }
            };
            let (response, sequence, generation) =
                session.execute(&cell_id, &source, ui_language.as_deref() == Some("de"))?;
            Ok((session, (response, cell_id, source), sequence, generation))
        })
        .await
        .map_err(|error| error.to_string())??;
    let (response, cell_id, source) = response;
    let outputs = response
        .get("outputs")
        .and_then(Value::as_array)
        .ok_or("Python response has no outputs")?
        .clone();
    on_notebook(&roots, &histories, root, move |root, store| {
        if root != expected_root || session.generation.load(Ordering::SeqCst) != generation {
            return Err("Python session changed before its output could be saved".into());
        }
        store.commit_calculation_outputs(
            root,
            &page_id,
            &calculation_id,
            &cell_id,
            &source,
            outputs,
            sequence,
            session.provenance.clone(),
            &modified_at,
        )
    })
    .await
}

/// Call with the old canonical root before close, access revocation, or rename.
/// Also use this before replacing an environment if existing sessions must restart.
pub fn end_root_sessions(state: &RuntimeState, root: &Path) -> Result<(), String> {
    let removed = {
        let mut sessions = state.sessions.lock().map_err(|error| error.to_string())?;
        state.lifecycle.fetch_add(1, Ordering::SeqCst);
        let keys: Vec<_> = sessions
            .keys()
            .filter(|(path, _)| path == root)
            .cloned()
            .collect();
        keys.into_iter()
            .filter_map(|key| sessions.remove(&key))
            .collect::<Vec<_>>()
    };
    for session in removed {
        session.stop();
    }
    Ok(())
}
#[tauri::command]
pub async fn end_calculation_session(
    roots: tauri::State<'_, AllowedRoots>,
    state: tauri::State<'_, RuntimeState>,
    root: String,
    calculation_id: String,
) -> Result<(), String> {
    let root = ensure_allowed(&roots, &root)?;
    if let Some(session) = state
        .sessions
        .lock()
        .map_err(|error| error.to_string())?
        .remove(&(root, calculation_id))
    {
        session.stop();
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn root_cleanup_invalidates_pending_startups() {
        let state = RuntimeState::default();
        let epoch = state.lifecycle.load(Ordering::SeqCst);
        end_root_sessions(&state, Path::new("unused-root")).unwrap();
        assert_ne!(state.lifecycle.load(Ordering::SeqCst), epoch);
    }
    #[test]
    #[ignore = "requires an explicitly prepared scientific runtime in GOODTYPE_PYTHON_TEST_RUNTIME"]
    fn restricted_python_runs_assistant_csv_plot_recipe() {
        let runtime = std::env::var_os("GOODTYPE_PYTHON_TEST_RUNTIME")
            .expect("set GOODTYPE_PYTHON_TEST_RUNTIME");
        let skill = include_str!("../../../../../crates/goodtype-mcp/skill/goodtype/SKILL.md")
            .replace("\r\n", "\n");
        let section = skill.split("### CSV data and Python plots").nth(1).unwrap();
        let recipe = section
            .split("```python\n")
            .nth(1)
            .unwrap()
            .split("```")
            .next()
            .unwrap();
        for _ in 0..2 {
            // The saved source must recreate its CSV and plot even after the old session ends.
            let session = Session::new(Path::new(&runtime)).unwrap();
            let (result, _, _) = session.execute("csv-plot", recipe, false).unwrap();
            let outputs = result["outputs"].as_array().unwrap();
            assert!(
                !outputs
                    .iter()
                    .any(|output| output["output_type"] == "error"),
                "{result}"
            );
            assert!(
                outputs
                    .iter()
                    .any(|output| output["data"]["image/png"].is_string()),
                "{result}"
            );
            assert_eq!(
                std::fs::read_to_string(session.output.join("plot_data/example.csv"))
                    .unwrap()
                    .replace("\r\n", "\n"),
                "x,y\n0,0\n1,2\n2,4\n"
            );
            let (loaded, _, _) = session
                .execute(
                    "csv-read",
                    "assert data['y'].tolist() == [0, 2, 4]\nassert '__file__' not in globals()",
                    false,
                )
                .unwrap();
            assert!(
                !loaded["outputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|output| output["output_type"] == "error"),
                "{loaded}"
            );
            session.stop();
        }
    }

    #[test]
    #[ignore = "requires an explicitly prepared portable runtime in GOODTYPE_PYTHON_TEST_RUNTIME"]
    fn restricted_python_protocol_preserves_state_isolates_sessions_and_bounds_output() {
        let runtime = std::env::var_os("GOODTYPE_PYTHON_TEST_RUNTIME")
            .expect("set GOODTYPE_PYTHON_TEST_RUNTIME");
        let first = Arc::new(Session::new(Path::new(&runtime)).unwrap());
        let (result, count, _) = first.execute("one", "x = 2\nprint(x)", false).unwrap();
        assert_eq!(count, 1);
        assert!(result["outputs"].to_string().contains('2'));
        let (result, count, _) = first.execute("two", "x + 3", false).unwrap();
        assert_eq!(count, 2);
        assert!(result["outputs"].to_string().contains('5'));
        let second = Session::new(Path::new(&runtime)).unwrap();
        let (result, _, _) = second.execute("independent", "print(x)", false).unwrap();
        assert!(result["outputs"].to_string().contains("NameError"));
        first.execute("failure", "x = 7\n1 / 0", false).unwrap();
        let (result, _, _) = first.execute("after", "print(x)", false).unwrap();
        assert!(result["outputs"].to_string().contains('7'));
        let (result, _, _) = first
            .execute("large", "print('a' * 2_000_000)", false)
            .unwrap();
        assert!(result["outputs"].to_string().contains("Output shortened"));
        assert!(serde_json::to_vec(&result).unwrap().len() < 1024 * 1024 + 4096);
        let (result_de, _, _) = first
            .execute("large_de", "print('b' * 2_000_000)", true)
            .unwrap();
        assert!(
            result_de["outputs"]
                .to_string()
                .contains("Ausgabe wegen Größenlimit gekürzt.")
        );
        let (unsupported, _, _) = first
            .execute("input_test", "input('enter value:')", false)
            .unwrap();
        assert!(
            unsupported["outputs"]
                .to_string()
                .contains("Interactive input() is not supported")
        );
        let (plot, _, _) = first.execute("plot", "import numpy as np\nimport matplotlib.pyplot as plt\nx = np.linspace(0, 6, 100)\nplt.plot(x, np.sin(x))", false).unwrap();

        assert!(
            plot["outputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|output| output["data"]["image/png"].is_string())
        );
        let running = first.clone();
        let worker = std::thread::spawn(move || running.execute("loop", "while True: pass", false));
        std::thread::sleep(Duration::from_millis(150));
        first.stop();
        assert!(worker.join().unwrap().is_err());
        assert!(!first.alive());
    }
}
