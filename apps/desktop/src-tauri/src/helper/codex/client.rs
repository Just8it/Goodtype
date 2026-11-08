use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
pub const LINE_LIMIT: usize = 4 * 1024 * 1024;
pub struct Client {
    pub cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    child: Option<Child>,
    input: Box<dyn Write + Send>,
    pub incoming: mpsc::Receiver<Result<Value, String>>,
    next: u64,
    responses: HashMap<u64, Value>,
    pub cwd: tempfile::TempDir,
    #[cfg(windows)]
    job: Option<Job>,
}
impl Client {
    pub fn spawn(path: &Path, overrides: &[String]) -> Result<Self, String> {
        let cwd = tempfile::tempdir().map_err(|e| e.to_string())?;
        let mut command = Command::new(path);
        for value in overrides {
            command.arg("-c").arg(value);
        }
        command
            .args(["app-server", "--listen", "stdio://"])
            .current_dir(cwd.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        #[cfg(windows)]
        let job = match Job::new(&child) {
            Ok(job) => job,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        let input = child.stdin.take().ok_or("missing stdin")?;
        let output = child.stdout.take().ok_or("missing stdout")?;
        let stderr = child.stderr.take().ok_or("missing stderr")?;
        let (tx, incoming) = mpsc::sync_channel(128);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let result = read_line(&mut reader)
                    .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()));
                let failed = result.is_err();
                if tx.send(result).is_err() || failed {
                    break;
                }
            }
        });
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut total = 0;
            while let Ok(line) = read_line(&mut reader) {
                if total < 8192 {
                    total += line.len();
                    eprintln!("Codex stderr: {} bytes (content withheld)", line.len());
                }
            }
        });
        Ok(Self {
            cancelled: Default::default(),
            child: Some(child),
            input: Box::new(input),
            incoming,
            next: 0,
            responses: HashMap::new(),
            cwd,
            #[cfg(windows)]
            job: Some(job),
        })
    }
    pub fn send(&mut self, value: Value) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
        if bytes.len() > LINE_LIMIT {
            return Err("oversized request".into());
        }
        bytes.push(b'\n');
        self.input
            .write_all(&bytes)
            .and_then(|()| self.input.flush())
            .map_err(|e| e.to_string())
    }
    pub fn call(
        &mut self,
        method: &str,
        params: Value,
        event: &mut impl FnMut(&Value),
    ) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        self.send(json!({"id":id,"method":method,"params":params}))?;
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            if self.cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                return Err("cancelled".into());
            }
            if let Some(value) = self.responses.remove(&id) {
                return response(value);
            }
            let value = match self.incoming.recv_timeout(
                until
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(100)),
            ) {
                Ok(value) => value?,
                Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < until => continue,
                Err(e) => return Err(e.to_string()),
            };
            if value.get("method").is_some() {
                self.event(value, event)?;
            } else if let Some(id) = value["id"].as_u64() {
                self.responses.insert(id, value);
            }
            if Instant::now() >= until {
                return Err("app-server response timed out".into());
            }
        }
    }
    pub fn thread_inventory(
        &mut self,
        thread: &str,
        names: &[String],
        event: &mut impl FnMut(&Value),
    ) -> Result<Value, String> {
        let mut data = Vec::new();
        // In 0.166 only a named-server query reuses the thread's connection. A bulk query
        // starts another relay and consumes its single-use launch token a second time.
        for name in std::iter::once("goodtype").chain(names.iter().map(String::as_str)) {
            let result = self.call(
                "mcpServerStatus/list",
                json!({"threadId":thread,"serverName":name,"detail":"toolsAndAuthOnly","limit":100}),
                event,
            )?;
            let servers = result["data"].as_array().ok_or("missing MCP inventory")?;
            if !result["nextCursor"].is_null() || servers.len() != 1 || servers[0]["name"] != name {
                return Err(format!("incomplete MCP inventory for {name}"));
            }
            data.push(servers[0].clone());
        }
        Ok(json!({"data":data,"nextCursor":null}))
    }
    pub fn event(&mut self, value: Value, event: &mut impl FnMut(&Value)) -> Result<(), String> {
        if let Some(id) = value.get("id") {
            if let Some(result) = decline(value["method"].as_str().unwrap_or("")) {
                self.send(json!({"id":id,"result":result}))?;
            } else {
                self.send(
                    json!({"id":id,"error":{"code":-32601,"message":"Unsupported server request"}}),
                )?;
            }
        }
        event(&value);
        Ok(())
    }
    pub fn initialize(&mut self, event: &mut impl FnMut(&Value)) -> Result<String, String> {
        self.call("initialize",json!({"clientInfo":{"name":"goodtype","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}),event)?;
        self.send(json!({"method":"initialized","params":{}}))?;
        let account = self.call("account/read", json!({"refreshToken":false}), event)?;
        Ok(match account["account"]["type"].as_str() {
            Some("chatgpt") => "chatgpt",
            Some(_) => "other",
            None => "none",
        }
        .into())
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn response(value: Value) -> Result<Value, String> {
    if let Some(e) = value.get("error") {
        Err(e.to_string())
    } else {
        value.get("result").cloned().ok_or("missing result".into())
    }
}
pub fn read_line(reader: &mut impl BufRead) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let n = reader
        .take((LINE_LIMIT + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|e| e.to_string())?;
    if n == 0 || n > LINE_LIMIT || bytes.last() != Some(&b'\n') {
        return Err("closed or oversized JSONL line".into());
    }
    Ok(bytes)
}
pub fn decline(method: &str) -> Option<Value> {
    match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            Some(json!({"decision":"decline"}))
        }
        "item/permissions/requestApproval" => Some(json!({"permissions":{},"scope":"turn"})),
        "mcpServer/elicitation/request" => {
            Some(json!({"action":"decline","content":null,"_meta":null}))
        }
        "item/tool/requestUserInput" => Some(json!({"answers":{}})),
        _ => None,
    }
}
pub fn resolve(configured: Option<&str>) -> Result<PathBuf, String> {
    if let Some(path) = configured {
        let path = Path::new(path);
        if !path.is_absolute() {
            return Err("configured executable must be absolute".into());
        }
        return path.canonicalize().map_err(|e| e.to_string());
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let path = dir.join(if cfg!(windows) { "codex.exe" } else { "codex" });
        if path.is_file() {
            return path.canonicalize().map_err(|e| e.to_string());
        }
    }
    Err("codex executable not found".into())
}
pub fn version(path: &Path) -> Result<String, String> {
    let mut cmd = Command::new(path);
    cmd.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() || out.stdout.len() > 1024 {
        return Err("version check failed".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
#[cfg(windows)]
struct Job(std::os::windows::io::OwnedHandle);
#[cfg(windows)]
impl Job {
    fn new(child: &Child) -> Result<Self, String> {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if raw.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let job = Self(OwnedHandle::from_raw_handle(raw));
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                raw,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(raw, child.as_raw_handle()) == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(job)
        }
    }
    fn terminate(&self) {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0.as_raw_handle(), 1);
        }
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    #[derive(Clone)]
    struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl Write for Capture {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn inventory_queries_each_existing_thread_connection_and_rejects_incomplete_results() {
        for bad in [
            None,
            Some(json!({"data":[],"nextCursor":null})),
            Some(json!({"data":[{"name":"other"}],"nextCursor":null})),
            Some(json!({"data":[{"name":"goodtype"}],"nextCursor":"more"})),
        ] {
            let output = Capture(Default::default());
            let (tx, incoming) = mpsc::sync_channel(4);
            tx.send(Ok(json!({"id":1,"result":bad.clone().unwrap_or(json!({"data":[{"name":"goodtype","tools":{"status":{}}}],"nextCursor":null}))}))).unwrap();
            tx.send(Ok(json!({"id":2,"result":{"data":[{"name":"github","tools":{},"runtimeStatus":"disabled"}],"nextCursor":null}}))).unwrap();
            let mut c = Client {
                cancelled: Default::default(),
                child: None,
                input: Box::new(output.clone()),
                incoming,
                next: 0,
                responses: HashMap::new(),
                cwd: tempfile::tempdir().unwrap(),
                #[cfg(windows)]
                job: None,
            };
            let result = c.thread_inventory("thread", &["github".into()], &mut |_| {});
            if bad.is_some() {
                assert!(result.is_err());
                continue;
            }
            assert_eq!(result.unwrap()["data"].as_array().unwrap().len(), 2);
            let written = output.0.lock().unwrap();
            let requests = written
                .split(|b| *b == b'\n')
                .filter(|line| !line.is_empty())
                .map(|line| serde_json::from_slice::<Value>(line).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(requests[0]["params"]["serverName"], "goodtype");
            assert_eq!(requests[1]["params"]["serverName"], "github");
            assert!(requests.iter().all(|r| r["params"]["threadId"] == "thread"));
        }
    }
    #[test]
    fn fake_app_server_routes_responses_approvals_and_interrupt() {
        let output = Capture(Default::default());
        let (tx, incoming) = mpsc::sync_channel(16);
        for message in [
            json!({"id":2,"result":{}}),
            json!({"id":"approval","method":"item/commandExecution/requestApproval","params":{}}),
            json!({"method":"item/agentMessage/delta","params":{"delta":"hello"}}),
            json!({"id":1,"result":{"thread":{"id":"thread"}}}),
            json!({"method":"turn/completed","params":{"turn":{"status":"interrupted"}}}),
        ] {
            tx.send(Ok(message)).unwrap();
        }
        let mut c = Client {
            cancelled: Default::default(),
            child: None,
            input: Box::new(output.clone()),
            incoming,
            next: 0,
            responses: HashMap::new(),
            cwd: tempfile::tempdir().unwrap(),
            #[cfg(windows)]
            job: None,
        };
        let mut events = Vec::new();
        assert_eq!(
            c.call("thread/start", json!({}), &mut |v| events.push(v.clone()))
                .unwrap()["thread"]["id"],
            "thread"
        );
        c.call(
            "turn/interrupt",
            json!({"threadId":"thread","turnId":"turn"}),
            &mut |v| events.push(v.clone()),
        )
        .unwrap();
        let terminal = c.incoming.recv().unwrap().unwrap();
        c.event(terminal, &mut |v| events.push(v.clone())).unwrap();
        assert_eq!(
            events.last().unwrap()["params"]["turn"]["status"],
            "interrupted"
        );
        let messages = output
            .0
            .lock()
            .unwrap()
            .split(|b| *b == b'\n')
            .filter(|b| !b.is_empty())
            .map(|b| serde_json::from_slice::<Value>(b).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            messages[1],
            json!({"id":"approval","result":{"decision":"decline"}})
        );
        assert_eq!(messages[2]["method"], "turn/interrupt");
    }
}
