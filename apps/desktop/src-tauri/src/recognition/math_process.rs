//! One serialized, warm helper. Blocking pipe reads/writes stay off the UI and have deadlines.
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

const LINE_LIMIT: usize = 4 * 1024 * 1024;
const IDLE: Duration = Duration::from_secs(600);

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MathStroke {
    pub width_pt: f64,
    pub points: Vec<[f64; 2]>,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct MathSelection {
    pub strokes: Vec<MathStroke>,
}
impl MathSelection {
    pub fn validate(&self) -> Result<(), String> {
        if self.strokes.is_empty()
            || self.strokes.len() > super::MAX_STROKES
            || self.strokes.iter().map(|s| s.points.len()).sum::<usize>() > super::MAX_POINTS
            || self.strokes.iter().any(|s| {
                s.points.is_empty()
                    || !s.width_pt.is_finite()
                    || s.width_pt <= 0.0
                    || s.width_pt > 100.0
                    || s.points
                        .iter()
                        .flatten()
                        .any(|v| !v.is_finite() || v.abs() > super::MAX_COORDINATE_PT)
            })
        {
            return Err("invalid or oversized math selection".into());
        }
        Ok(())
    }
}
fn request(id: u64, selection: &MathSelection) -> Result<Vec<u8>, String> {
    selection.validate()?;
    let mut bytes = serde_json::to_vec(&serde_json::json!({"id":id,"strokes":selection.strokes}))
        .map_err(|e| e.to_string())?;
    if bytes.len() > LINE_LIMIT {
        return Err("math request exceeds 4 MiB".into());
    }
    bytes.push(b'\n');
    Ok(bytes)
}
/// The outer error means the helper broke the protocol; the inner one is its answer to this
/// request, which leaves a healthy helper running.
fn response(line: &str, expected: u64) -> Result<Result<String, String>, String> {
    #[derive(Deserialize)]
    struct Response {
        id: u64,
        latex: Option<String>,
        error: Option<String>,
    }
    let reply: Response =
        serde_json::from_str(line).map_err(|e| format!("invalid math response: {e}"))?;
    if reply.id != expected {
        return Err("math response id does not match request".into());
    }
    match (reply.latex, reply.error) {
        (Some(latex), None) => Ok(Ok(latex)),
        (None, Some(error)) => Ok(Err(error)),
        _ => Err("math response must contain latex or error".into()),
    }
}
fn read_line(reader: &mut impl BufRead) -> Result<String, String> {
    let mut bytes = Vec::new();
    let n = reader
        .take((LINE_LIMIT + 2) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|e| e.to_string())?;
    if n == 0 || bytes.len() > LINE_LIMIT + 1 || bytes.last() != Some(&b'\n') {
        return Err("invalid or oversized math response line".into());
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}
fn idle(elapsed: Duration) -> bool {
    elapsed >= IDLE
}

pub struct MathProcess {
    session: Arc<Mutex<Option<Session>>>,
}
impl Default for MathProcess {
    fn default() -> Self {
        let session: Arc<Mutex<Option<Session>>> = Arc::new(Mutex::new(None));
        let weak = Arc::downgrade(&session);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let Some(shared) = weak.upgrade() else {
                    break;
                };
                if let Ok(mut slot) = shared.try_lock()
                    && slot.as_ref().is_some_and(|s| idle(s.used.elapsed()))
                {
                    slot.take();
                }
            }
        });
        Self { session }
    }
}
impl MathProcess {
    pub fn shutdown(&self) {
        self.session.lock().unwrap().take();
    }
    pub fn prepare(&self, app: &tauri::AppHandle) -> Result<(), String> {
        let mut slot = self.session.lock().unwrap();
        self.ensure(app, &mut slot)
    }
    fn ensure(&self, app: &tauri::AppHandle, slot: &mut Option<Session>) -> Result<(), String> {
        if let Some(session) = slot.as_mut()
            && (idle(session.used.elapsed())
                || session
                    .child
                    .try_wait()
                    .map_err(|e| e.to_string())?
                    .is_some())
        {
            slot.take();
        }
        if slot.is_none()
            && let Some(executable) = crate::importer::math_executable(app)?
        {
            *slot = Some(Session::start(&executable)?);
        }
        Ok(())
    }
    pub fn recognize(
        &self,
        app: &tauri::AppHandle,
        selection: MathSelection,
    ) -> Result<String, String> {
        let mut slot = self.session.lock().unwrap();
        self.ensure(app, &mut slot)?;
        let session = slot.as_mut().ok_or("math recognizer is not installed")?;
        session.id = session
            .id
            .checked_add(1)
            .ok_or("math request id exhausted")?;
        let bytes = request(session.id, &selection)?;
        let reply = session
            .input
            .send(bytes)
            .map_err(|_| "math recognizer stopped".to_owned())
            .and_then(|_| session.line(Duration::from_secs(30)))
            .and_then(|line| response(&line, session.id));
        session.used = Instant::now();
        // A stopped, late or garbled helper is restarted next time; its 4 s model load is only
        // paid again when something is actually wrong with the process.
        if reply.is_err() {
            slot.take();
        }
        reply?
    }
}
struct Session {
    child: Child,
    #[cfg(windows)]
    _job: crate::importer::job::Job,
    input: mpsc::Sender<Vec<u8>>,
    output: mpsc::Receiver<Result<String, String>>,
    id: u64,
    used: Instant,
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Session {
    fn line(&self, timeout: Duration) -> Result<String, String> {
        self.output
            .recv_timeout(timeout)
            .map_err(|_| "math recognizer stopped or timed out".to_owned())?
    }
    fn start(executable: &std::path::Path) -> Result<Self, String> {
        if !crate::importer::platform_supported() {
            return Err("math recognition requires Windows x64".into());
        }
        let mut command = Command::new(executable);
        command
            .arg("--serve")
            .current_dir(executable.parent().ok_or("missing helper directory")?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        #[cfg(windows)]
        let job =
            crate::importer::job::Job::new(3 * 1024 * 1024 * 1024).map_err(|e| e.to_string())?;
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        #[cfg(windows)]
        if let Err(error) = job.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.to_string());
        }
        let mut stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let (input, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            for bytes in rx {
                if stdin.write_all(&bytes).and_then(|_| stdin.flush()).is_err() {
                    break;
                }
            }
        });
        let (tx, output) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let result = read_line(&mut reader);
                let failed = result.is_err();
                if tx.send(result).is_err() || failed {
                    break;
                }
            }
        });
        let session = Self {
            child,
            #[cfg(windows)]
            _job: job,
            input,
            output,
            id: 0,
            used: Instant::now(),
        };
        let ready: serde_json::Value =
            serde_json::from_str(&session.line(Duration::from_secs(60))?)
                .map_err(|e| e.to_string())?;
        if ready.get("ready").and_then(|v| v.as_bool()) != Some(true) {
            return Err("math recognizer did not become ready".into());
        }
        Ok(session)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn selection() -> MathSelection {
        MathSelection {
            strokes: vec![MathStroke {
                width_pt: 2.0,
                points: vec![[1.0, 2.0]],
            }],
        }
    }
    #[test]
    fn bounds_and_serialization() {
        let mut s = selection();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&request(7, &s).unwrap()).unwrap(),
            serde_json::json!({"id":7,"strokes":[{"widthPt":2.0,"points":[[1.0,2.0]]}]})
        );
        for width in [0.0, -1.0, 100.1, f64::NAN, f64::INFINITY] {
            s.strokes[0].width_pt = width;
            assert!(s.validate().is_err());
        }
        s = selection();
        s.strokes[0].points[0][0] = 100001.0;
        assert!(s.validate().is_err());
        s = selection();
        s.strokes[0].points.clear();
        assert!(s.validate().is_err());
        assert!(MathSelection { strokes: vec![] }.validate().is_err());
        s = selection();
        s.strokes[0].points = vec![[0.0, 0.0]; 200001];
        assert!(s.validate().is_err());
        s = selection();
        s.strokes[0].points = vec![[99999.123456789, 99999.123456789]; 200000];
        assert!(request(1, &s).is_err());
    }
    #[test]
    fn bounded_protocol_lines() {
        use std::io::Cursor;
        assert_eq!(
            read_line(&mut Cursor::new(b"{\"ready\":true}\n")).unwrap(),
            "{\"ready\":true}\n"
        );
        assert!(read_line(&mut Cursor::new(b"")).is_err());
        assert!(read_line(&mut Cursor::new(b"truncated")).is_err());
        assert!(read_line(&mut Cursor::new(vec![b'x'; LINE_LIMIT + 2])).is_err());
        assert!(read_line(&mut Cursor::new([0xff, b'\n'])).is_err());
    }

    #[test]
    fn responses_and_idle_deadline() {
        assert_eq!(
            response(r#"{"id":4,"latex":"x"}"#, 4).unwrap(),
            Ok("x".into())
        );
        assert_eq!(
            response(r#"{"id":4,"error":"failed"}"#, 4).unwrap(),
            Err("failed".into())
        );
        for line in [r#"{"id":5,"latex":"x"}"#, r#"{"id":4}"#, "bad"] {
            assert!(response(line, 4).is_err());
        }
        assert!(!idle(Duration::from_secs(599)));
        assert!(idle(Duration::from_secs(600)));
    }
}
