use goodtype_typst::{Completion, Hover};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};
use std::time::Duration;

const TINYMIST_VERSION: &str = "0.15.2";
const TINYMIST_SHA256: &str = "8780ed5076135f5afb11a03e6215d43659c193bc649634aff9b9dc91d77200a8";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const EDITOR_FILE: &str = "__goodtype_editor.typ";
type Pending = Arc<Mutex<HashMap<u64, mpsc::Sender<Result<Value, String>>>>>;
type PublishedDiagnostics = Arc<(Mutex<Option<Value>>, Condvar)>;

#[derive(Clone, Default)]
pub struct Tinymist(Arc<Mutex<Option<Client>>>);

#[derive(Debug, Serialize)]
pub struct Highlight {
    pub kind: String,
    pub modifiers: Vec<String>,
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub severity: &'static str,
    pub message: String,
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub highlights: Vec<Highlight>,
    pub diagnostics: Vec<Diagnostic>,
    /// Whether the analyzer actually answered.
    ///
    /// Completion, hover and formatting all fall back to the in-process `typst-ide` path when
    /// the sidecar is unavailable. Semantic highlighting has no fallback — `typst-ide` has no
    /// equivalent — so an empty analysis is genuinely ambiguous: it means either "this source
    /// has no tokens" or "there is no analyzer on this platform". Saying which is what lets the
    /// editor keep the colours it already has instead of blanking the document.
    pub available: bool,
}

impl Analysis {
    /// No analyzer answered. Distinct from an empty analysis, which is a real answer.
    pub fn unavailable() -> Self {
        Self {
            highlights: Vec::new(),
            diagnostics: Vec::new(),
            available: false,
        }
    }
}

impl Default for Analysis {
    fn default() -> Self {
        Self {
            highlights: Vec::new(),
            diagnostics: Vec::new(),
            available: true,
        }
    }
}

impl Tinymist {
    pub fn reset(&self) {
        if let Ok(mut slot) = self.0.lock() {
            slot.take();
        }
    }

    pub fn complete(
        &self,
        root: &Path,
        source: &str,
        cursor: usize,
        _explicit: bool,
    ) -> Result<Vec<Completion>, String> {
        self.run(root, |client| {
            client.sync(source)?;
            let position = byte_offset_to_position(source, cursor);
            let response = client.request(
                "textDocument/completion",
                json!({
                    "textDocument": { "uri": client.document_uri },
                    "position": position,
                    "context": { "triggerKind": 1 },
                }),
            )?;
            parse_completions(source, cursor, response)
        })
    }

    pub fn hover(&self, root: &Path, source: &str, cursor: usize) -> Result<Option<Hover>, String> {
        self.run(root, |client| {
            client.sync(source)?;
            let response = client.request(
                "textDocument/hover",
                json!({
                    "textDocument": { "uri": client.document_uri },
                    "position": byte_offset_to_position(source, cursor),
                }),
            )?;
            Ok(parse_hover(response))
        })
    }

    pub fn format(&self, root: &Path, source: &str) -> Result<String, String> {
        self.run(root, |client| {
            client.sync(source)?;
            let response = client.request(
                "textDocument/formatting",
                json!({
                    "textDocument": { "uri": client.document_uri },
                    "options": { "tabSize": 2, "insertSpaces": true },
                }),
            )?;
            apply_text_edits(source, response)
        })
    }

    pub fn analyze(&self, root: &Path, source: &str) -> Result<Analysis, String> {
        self.run(root, |client| {
            client.sync(source)?;
            let response = client.request(
                "textDocument/semanticTokens/full",
                json!({ "textDocument": { "uri": client.document_uri } }),
            )?;
            Ok(Analysis {
                highlights: parse_semantic_tokens(
                    source,
                    &client.semantic_legend,
                    &client.semantic_modifiers,
                    response,
                )?,
                diagnostics: client.diagnostics(source)?,
                available: true,
            })
        })
    }

    fn run<T>(
        &self,
        root: &Path,
        mut operation: impl FnMut(&mut Client) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut slot = self.0.lock().map_err(|_| "Tinymist state is unavailable")?;
        let mut last_error = String::new();
        for _ in 0..2 {
            if slot.as_ref().is_none_or(|client| client.root != root) {
                *slot = Some(Client::start(root.to_owned())?);
            }
            match operation(slot.as_mut().expect("Tinymist client was just created")) {
                Ok(value) => return Ok(value),
                Err(error) => {
                    last_error = error;
                    slot.take();
                }
            }
        }
        Err(last_error)
    }
}

struct Client {
    root: PathBuf,
    document_uri: String,
    source: String,
    version: u64,
    opened: bool,
    semantic_legend: Vec<String>,
    semantic_modifiers: Vec<String>,
    diagnostics: PublishedDiagnostics,
    child: Child,
    input: Arc<Mutex<ChildStdin>>,
    pending: Pending,
    next_id: AtomicU64,
    proxy_alive: Arc<AtomicBool>,
}

impl Client {
    fn start(root: PathBuf) -> Result<Self, String> {
        let binary = verified_binary()?;
        let root_uri = tauri::Url::from_directory_path(&root)
            .map_err(|_| "Notebook root cannot be represented as a file URL")?
            .to_string();
        let document_uri = tauri::Url::from_file_path(root.join(EDITOR_FILE))
            .map_err(|_| "Typst editor path cannot be represented as a file URL")?
            .to_string();
        let settings = settings(&root);
        let (proxy_url, proxy_alive) = local_proxy_sink()?;

        let mut child = Command::new(binary)
            .arg("lsp")
            .current_dir(&root)
            .env("TINYMIST_LOG", "error")
            .env("HTTP_PROXY", &proxy_url)
            .env("HTTPS_PROXY", &proxy_url)
            .env("ALL_PROXY", &proxy_url)
            .env("NO_PROXY", "")
            .env("http_proxy", &proxy_url)
            .env("https_proxy", &proxy_url)
            .env("all_proxy", &proxy_url)
            .env("no_proxy", "")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("Could not start Tinymist {TINYMIST_VERSION}: {error}"))?;

        let input = Arc::new(Mutex::new(
            child.stdin.take().ok_or("Tinymist stdin is unavailable")?,
        ));
        let output = child
            .stdout
            .take()
            .ok_or("Tinymist stdout is unavailable")?;
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let diagnostics = Arc::new((Mutex::new(None), Condvar::new()));
        spawn_reader(
            output,
            input.clone(),
            pending.clone(),
            diagnostics.clone(),
            settings.clone(),
            root_uri.clone(),
        );
        if let Some(stderr) = child.stderr.take() {
            std::thread::spawn(move || for _ in BufReader::new(stderr).lines() {});
        }

        let mut client = Self {
            root,
            document_uri,
            source: String::new(),
            version: 0,
            opened: false,
            semantic_legend: Vec::new(),
            semantic_modifiers: Vec::new(),
            diagnostics,
            child,
            input,
            pending,
            next_id: AtomicU64::new(1),
            proxy_alive,
        };
        let initialize = client.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "workspaceFolders": [{ "uri": root_uri, "name": "Goodtype notebook" }],
                "capabilities": {
                    "workspace": { "configuration": true, "workspaceFolders": true },
                    "textDocument": {
                        "completion": { "completionItem": { "snippetSupport": true } },
                        "hover": { "contentFormat": ["markdown", "plaintext"] },
                        "semanticTokens": {
                            "requests": { "full": true },
                            "tokenTypes": [
                                "comment", "string", "keyword", "operator", "number",
                                "function", "decorator", "type", "namespace", "bool",
                                "punct", "escape", "link", "raw", "label", "ref",
                                "heading", "marker", "term", "delim", "pol", "error", "text"
                            ],
                            "tokenModifiers": ["strong", "emph", "math", "readonly", "static", "defaultLibrary"],
                            "formats": ["relative"]
                        }
                    }
                },
                "initializationOptions": settings,
            }),
        )?;
        client.semantic_legend = initialize
            .pointer("/capabilities/semanticTokensProvider/legend/tokenTypes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        client.semantic_modifiers = initialize
            .pointer("/capabilities/semanticTokensProvider/legend/tokenModifiers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        if client.semantic_legend.is_empty() {
            return Err("Tinymist did not provide a semantic-token legend".into());
        }
        client.notify("initialized", json!({}))?;
        client.notify(
            "workspace/didChangeConfiguration",
            json!({ "settings": settings }),
        )?;
        Ok(client)
    }

    fn sync(&mut self, source: &str) -> Result<(), String> {
        if !self.opened {
            self.clear_diagnostics();
            self.version = 1;
            self.notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": self.document_uri,
                        "languageId": "typst",
                        "version": self.version,
                        "text": source,
                    }
                }),
            )?;
            self.opened = true;
            self.source = source.to_owned();
        } else if self.source != source {
            self.clear_diagnostics();
            self.version += 1;
            self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": self.document_uri, "version": self.version },
                    "contentChanges": [{ "text": source }],
                }),
            )?;
            self.source = source.to_owned();
        }
        Ok(())
    }

    fn clear_diagnostics(&self) {
        if let Ok(mut diagnostics) = self.diagnostics.0.lock() {
            *diagnostics = None;
        }
    }

    fn diagnostics(&self, source: &str) -> Result<Vec<Diagnostic>, String> {
        let (slot, ready) = &*self.diagnostics;
        let diagnostics = slot
            .lock()
            .map_err(|_| "Tinymist diagnostic state is unavailable")?;
        let (diagnostics, _) = ready
            .wait_timeout_while(diagnostics, Duration::from_secs(1), |value| value.is_none())
            .map_err(|_| "Tinymist diagnostic state is unavailable")?;
        diagnostics
            .as_ref()
            .map(|params| parse_diagnostics(source, params))
            .transpose()
            .map(Option::unwrap_or_default)
    }

    fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| "Tinymist request state is unavailable")?
            .insert(id, sender);
        if let Err(error) = write_message(
            &self.input,
            &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        ) {
            self.pending
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&id));
            return Err(error);
        }
        self.receive(method, id, receiver, REQUEST_TIMEOUT)
    }

    fn receive(
        &self,
        method: &str,
        id: u64,
        receiver: mpsc::Receiver<Result<Value, String>>,
        timeout: Duration,
    ) -> Result<Value, String> {
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(_) => {
                self.pending
                    .lock()
                    .ok()
                    .and_then(|mut pending| pending.remove(&id));
                let _ = self.notify("$/cancelRequest", json!({ "id": id }));
                Err(format!("Tinymist request {method} timed out"))
            }
        }
    }

    fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        write_message(
            &self.input,
            &json!({ "jsonrpc": "2.0", "method": method, "params": params }),
        )
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.proxy_alive.store(false, Ordering::Relaxed);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id, sender);
        }
        let _ = write_message(
            &self.input,
            &json!({ "jsonrpc": "2.0", "id": id, "method": "shutdown", "params": null }),
        );
        let _ = self.receive("shutdown", id, receiver, Duration::from_millis(250));
        let _ = self.notify("exit", json!({}));
        for _ in 0..10 {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn settings(root: &Path) -> Value {
    json!({
        "rootPath": root,
        "projectResolution": "singleFile",
        "exportPdf": "never",
        "systemFonts": false,
        "semanticTokens": "enable",
        "lint": { "enabled": false },
    })
}

fn local_proxy_sink() -> Result<(String, Arc<AtomicBool>), String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("Could not contain Tinymist network access: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("Could not configure Tinymist network containment: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("Could not read Tinymist proxy address: {error}"))?;
    let alive = Arc::new(AtomicBool::new(true));
    let thread_alive = alive.clone();
    std::thread::spawn(move || {
        while thread_alive.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => drop(stream),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(_) => break,
            }
        }
    });
    Ok((format!("http://{address}"), alive))
}

fn spawn_reader(
    output: impl Read + Send + 'static,
    input: Arc<Mutex<ChildStdin>>,
    pending: Pending,
    diagnostics: PublishedDiagnostics,
    settings: Value,
    root_uri: String,
) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(output);
        while let Ok(Some(message)) = read_message(&mut reader) {
            if let (Some(method), Some(id)) = (
                message.get("method").and_then(Value::as_str),
                message.get("id").cloned(),
            ) {
                let result = match method {
                    "workspace/configuration" => {
                        let count = message["params"]["items"].as_array().map_or(0, Vec::len);
                        Value::Array((0..count).map(|_| settings.clone()).collect())
                    }
                    "workspace/workspaceFolders" => {
                        json!([{ "uri": root_uri, "name": "Goodtype notebook" }])
                    }
                    _ => Value::Null,
                };
                let _ = write_message(
                    &input,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                );
            } else if message.get("method").and_then(Value::as_str)
                == Some("textDocument/publishDiagnostics")
            {
                if let Some(params) = message.get("params") {
                    let (slot, ready) = &*diagnostics;
                    if let Ok(mut published) = slot.lock() {
                        *published = Some(params.clone());
                        ready.notify_all();
                    }
                }
            } else if let Some(id) = message.get("id").and_then(Value::as_u64) {
                let sender = pending
                    .lock()
                    .ok()
                    .and_then(|mut pending| pending.remove(&id));
                if let Some(sender) = sender {
                    let response = if let Some(error) = message.get("error") {
                        Err(format!("Tinymist returned an error: {error}"))
                    } else {
                        Ok(message.get("result").cloned().unwrap_or(Value::Null))
                    };
                    let _ = sender.send(response);
                }
            }
        }
        if let Ok(mut pending) = pending.lock() {
            for (_, sender) in pending.drain() {
                let _ = sender.send(Err("Tinymist stopped unexpectedly".into()));
            }
        }
    });
}

fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut length = None;
    loop {
        let mut header = String::new();
        let read = reader
            .read_line(&mut header)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Ok(None);
        }
        if header == "\r\n" || header == "\n" {
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length:") {
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| "Tinymist sent an invalid Content-Length")?,
            );
        }
    }
    let length = length.ok_or("Tinymist sent an LSP frame without Content-Length")?;
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| format!("Tinymist sent invalid JSON: {error}"))
}

fn write_message(input: &Arc<Mutex<ChildStdin>>, message: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(message).map_err(|error| error.to_string())?;
    let mut input = input.lock().map_err(|_| "Tinymist stdin is unavailable")?;
    write!(input, "Content-Length: {}\r\n\r\n", body.len()).map_err(|error| error.to_string())?;
    input.write_all(&body).map_err(|error| error.to_string())?;
    input.flush().map_err(|error| error.to_string())
}

fn parse_completions(
    source: &str,
    cursor: usize,
    response: Value,
) -> Result<Vec<Completion>, String> {
    if response.is_null() {
        return Ok(Vec::new());
    }
    let items = response
        .as_array()
        .or_else(|| response.get("items").and_then(Value::as_array))
        .ok_or("Tinymist returned an invalid completion list")?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?.to_owned();
            let edit = item.get("textEdit");
            let apply = edit
                .and_then(|edit| edit.get("newText"))
                .or_else(|| item.get("insertText"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let offset = edit
                .and_then(|edit| edit.get("range"))
                .and_then(|range| range.get("start"))
                .and_then(|position| position_to_byte_offset(source, position))
                .unwrap_or_else(|| word_start(source, cursor));
            let detail = item
                .get("detail")
                .and_then(Value::as_str)
                .or_else(|| {
                    item.pointer("/labelDetails/description")
                        .and_then(Value::as_str)
                })
                .map(str::to_owned);
            Some(Completion {
                kind: completion_kind(item.get("kind").and_then(Value::as_u64)),
                symbol: None,
                label,
                apply,
                detail,
                offset,
            })
        })
        .collect())
}

fn parse_hover(response: Value) -> Option<Hover> {
    let contents = response.get("contents")?;
    let markdown = contents
        .as_str()
        .or_else(|| contents.get("value").and_then(Value::as_str))?;
    let value = markdown
        .split("\n\n")
        .filter(|part| part.trim() != "---")
        .take(3)
        .collect::<Vec<_>>()
        .join("\n\n")
        .replace("```typc\n", "")
        .replace("```typ\n", "")
        .replace("```", "");
    Some(Hover {
        value: value.chars().take(1600).collect(),
        code: false,
    })
}

fn parse_semantic_tokens(
    source: &str,
    legend: &[String],
    modifier_legend: &[String],
    response: Value,
) -> Result<Vec<Highlight>, String> {
    let data = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or("Tinymist returned invalid semantic tokens")?;
    if data.len() % 5 != 0 {
        return Err("Tinymist returned a partial semantic token".into());
    }

    let mut line = 0usize;
    let mut character = 0usize;
    let mut highlights = Vec::with_capacity(data.len() / 5);
    for token in data.chunks_exact(5) {
        let delta_line = json_usize(&token[0])?;
        let delta_start = json_usize(&token[1])?;
        let length = json_usize(&token[2])?;
        let kind = legend
            .get(json_usize(&token[3])?)
            .ok_or("Tinymist semantic token uses an unknown type")?;
        let modifier_bits = token[4]
            .as_u64()
            .ok_or("Tinymist semantic token uses invalid modifiers")?;
        if delta_line == 0 {
            character += delta_start;
        } else {
            line += delta_line;
            character = delta_start;
        }
        // Incomplete Typst can briefly produce a token that extends past its line. One stale
        // assist must not blank every other color or suppress the independent diagnostics.
        let Some(from) = line_character_to_byte_offset(source, line, character) else {
            continue;
        };
        let Some(to) = line_character_to_byte_offset(source, line, character + length) else {
            continue;
        };
        if from < to {
            highlights.push(Highlight {
                kind: kind.clone(),
                modifiers: modifier_legend
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| modifier_bits & (1_u64 << index) != 0)
                    .map(|(_, modifier)| modifier.clone())
                    .collect(),
                from,
                to,
            });
        }
    }
    Ok(highlights)
}

fn parse_diagnostics(source: &str, params: &Value) -> Result<Vec<Diagnostic>, String> {
    let diagnostics = params
        .get("diagnostics")
        .and_then(Value::as_array)
        .ok_or("Tinymist returned invalid diagnostics")?
        .iter()
        .filter_map(|diagnostic| {
            let from = position_to_byte_offset(source, diagnostic.pointer("/range/start")?)?;
            let to = position_to_byte_offset(source, diagnostic.pointer("/range/end")?)?;
            let message = diagnostic.get("message")?.as_str()?.to_owned();
            Some(Diagnostic {
                severity: match diagnostic.get("severity").and_then(Value::as_u64) {
                    Some(1) => "error",
                    Some(2) => "warning",
                    _ => "info",
                },
                message,
                from,
                to,
            })
        })
        .collect();
    Ok(diagnostics)
}

fn json_usize(value: &Value) -> Result<usize, String> {
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| "Tinymist semantic token is not a valid integer".into())
}

fn apply_text_edits(source: &str, response: Value) -> Result<String, String> {
    if response.is_null() {
        return Ok(source.to_owned());
    }
    let edits = response
        .as_array()
        .ok_or("Tinymist returned invalid formatting edits")?;
    let mut parsed = edits
        .iter()
        .map(|edit| {
            let start = position_to_byte_offset(
                source,
                edit.pointer("/range/start").ok_or("Missing edit start")?,
            )
            .ok_or("Invalid edit start")?;
            let end = position_to_byte_offset(
                source,
                edit.pointer("/range/end").ok_or("Missing edit end")?,
            )
            .ok_or("Invalid edit end")?;
            let text = edit
                .get("newText")
                .and_then(Value::as_str)
                .ok_or("Missing edit text")?;
            if start > end {
                return Err("Tinymist returned a reversed text edit".to_owned());
            }
            Ok((start, end, text))
        })
        .collect::<Result<Vec<_>, String>>()?;
    parsed.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    let mut formatted = source.to_owned();
    let mut previous_start = source.len();
    for (start, end, text) in parsed {
        if end > previous_start {
            return Err("Tinymist returned overlapping text edits".into());
        }
        formatted.replace_range(start..end, text);
        previous_start = start;
    }
    Ok(formatted)
}

fn byte_offset_to_position(source: &str, byte_offset: usize) -> Value {
    let mut offset = byte_offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &source[..offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count();
    let column_source = before
        .rsplit_once('\n')
        .map_or(before, |(_, column)| column);
    json!({ "line": line, "character": column_source.encode_utf16().count() })
}

fn position_to_byte_offset(source: &str, position: &Value) -> Option<usize> {
    let target_line = usize::try_from(position.get("line")?.as_u64()?).ok()?;
    let target_column = usize::try_from(position.get("character")?.as_u64()?).ok()?;
    line_character_to_byte_offset(source, target_line, target_column)
}

fn line_character_to_byte_offset(
    source: &str,
    target_line: usize,
    target_column: usize,
) -> Option<usize> {
    let line_start = if target_line == 0 {
        0
    } else {
        source
            .match_indices('\n')
            .nth(target_line - 1)
            .map(|(offset, _)| offset + 1)?
    };
    let line = source[line_start..]
        .split_once('\n')
        .map_or(&source[line_start..], |(line, _)| line);
    let mut utf16 = 0;
    for (byte, character) in line.char_indices() {
        if utf16 == target_column {
            return Some(line_start + byte);
        }
        utf16 += character.len_utf16();
        if utf16 > target_column {
            return None;
        }
    }
    (utf16 == target_column).then_some(line_start + line.len())
}

fn word_start(source: &str, cursor: usize) -> usize {
    let mut offset = cursor.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    for (index, character) in source[..offset].char_indices().rev() {
        if !(character.is_alphanumeric() || matches!(character, '_' | '-' | '.')) {
            return index + character.len_utf8();
        }
    }
    0
}

fn completion_kind(kind: Option<u64>) -> &'static str {
    match kind {
        Some(2..=3) => "function",
        Some(4..=6) => "variable",
        Some(7 | 8 | 22) => "class",
        Some(9 | 10 | 17 | 18 | 19 | 20 | 21) => "constant",
        Some(14) => "keyword",
        Some(15) => "text",
        Some(16) => "color",
        Some(23 | 24) => "property",
        _ => "text",
    }
}

/// The bundled sidecar, once its checksum matches.
///
/// Only success is cached. Caching the `Result` meant one transient failure — a file lock during
/// an upgrade, a slow volume at startup — disabled editor intelligence for the rest of the
/// process, with no way back short of a restart. Re-verifying after a failure costs one hash of
/// a file that is almost certainly in the page cache.
fn verified_binary() -> Result<PathBuf, String> {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    if let Some(verified) = BINARY.get() {
        return Ok(verified.clone());
    }
    let verify = || -> Result<PathBuf, String> {
        {
            let path = if cfg!(debug_assertions) {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("binaries/tinymist-x86_64-pc-windows-msvc.exe")
            } else {
                std::env::current_exe()
                    .map_err(|error| error.to_string())?
                    .parent()
                    .ok_or("Goodtype executable has no parent directory")?
                    .join("tinymist.exe")
            };
            let mut file = File::open(&path)
                .map_err(|error| format!("Tinymist sidecar is missing: {error}"))?;
            let mut hasher = Sha256::new();
            let mut buffer = [0; 64 * 1024];
            loop {
                let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            let actual = format!("{:x}", hasher.finalize());
            if actual != TINYMIST_SHA256 {
                return Err(format!(
                    "Tinymist checksum mismatch: expected {TINYMIST_SHA256}, got {actual}"
                ));
            }
            Ok(path)
        }
    };
    let verified = verify()?;
    Ok(BINARY.get_or_init(|| verified).clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_positions_and_edits_preserve_unicode() {
        let source = "a\u{1d55c}\n\u{2211} x";
        let prefix = "a\u{1d55c}\n\u{2211}";
        let position = byte_offset_to_position(source, prefix.len());
        assert_eq!(position, json!({ "line": 1, "character": 1 }));
        assert_eq!(
            position_to_byte_offset(source, &position),
            Some(prefix.len())
        );

        let edits = json!([{
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 1 }
            },
            "newText": "\u{03a3}"
        }]);
        assert_eq!(
            apply_text_edits(source, edits).unwrap(),
            "a\u{1d55c}\n\u{03a3} x"
        );

        let highlights = parse_semantic_tokens(
            source,
            &["keyword".into()],
            &["math".into()],
            json!({ "data": [0, 1, 2, 0, 1] }),
        )
        .unwrap();
        assert_eq!((highlights[0].from, highlights[0].to), (1, 5));
        assert_eq!(highlights[0].modifiers, ["math"]);

        let diagnostics = parse_diagnostics(
            source,
            &json!({ "diagnostics": [{
                "range": {
                    "start": { "line": 0, "character": 1 },
                    "end": { "line": 0, "character": 3 }
                },
                "severity": 1,
                "message": "unknown variable"
            }] }),
        )
        .unwrap();
        assert_eq!((diagnostics[0].from, diagnostics[0].to), (1, 5));
        assert_eq!(diagnostics[0].severity, "error");
    }

    #[test]
    fn bundled_tinymist_completes_without_owning_the_document() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/typst")
            .canonicalize()
            .unwrap();
        let tinymist = Tinymist::default();
        let items = tinymist.complete(&root, "#image", 1, true).unwrap();
        assert!(items.iter().any(|item| item.label == "image"));
        let analysis = tinymist
            .analyze(&root, "#let answer = 42\n$ pi * 4 $\n#aasd")
            .unwrap();
        assert!(
            analysis
                .highlights
                .iter()
                .any(|token| token.kind == "keyword")
        );
        assert!(
            analysis
                .highlights
                .iter()
                .any(|token| token.kind == "number")
        );
        assert!(
            analysis
                .highlights
                .iter()
                .any(|token| token.modifiers.iter().any(|modifier| modifier == "math"))
        );
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == "error" && diagnostic.from < diagnostic.to)
        );
        assert!(!root.join(EDITOR_FILE).exists());
    }
}
