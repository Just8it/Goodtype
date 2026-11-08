mod client;
mod config;
#[cfg(test)]
mod tests;
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::mpsc};
use tauri::Manager as _;

pub struct Launch {
    pub grant: Option<CapturedRequestGrant>,
    pub cancelled: Arc<std::sync::atomic::AtomicBool>,
    pub created: Instant,
    pub conversation: String,
    pub share: SharePayload,
    pub info: ShareInfo,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexStatus {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub account: String,
    pub problem: Option<UiProblem>,
    pub models: Vec<Model>,
    pub mcp_servers: Vec<McpServer>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Model {
    pub id: String,
    pub label: String,
    pub image: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct McpServer {
    pub name: String,
    pub allowed: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRunRequest {
    pub usage_override: Option<Usage>,
    pub prompt: String,
    pub share: Option<SharePayload>,
    pub intent: Option<RequestContextIntent>,
    pub model: Option<String>,
    pub web_search: bool,
    pub conversation_id: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    pub run_id: String,
    pub request_id: String,
    pub conversation_id: String,
}
#[derive(Default)]
pub struct Manager {
    cache: Mutex<Option<(HelperSettings, CodexStatus)>>,
    conversations: Mutex<HashMap<String, Conversation>>,
}
struct Conversation {
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    run: String,
    busy: bool,
    model: Option<String>,
    web: bool,
    control: mpsc::Sender<Control>,
}
enum Control {
    Run {
        run: String,
        prompt: String,
        share: Box<SharePayload>,
        info: ShareInfo,
        grant: Box<CapturedRequestGrant>,
    },
    Stop,
    Close,
}
impl Manager {
    pub fn close_all(&self) {
        for (_, c) in self
            .conversations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
        {
            c.cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = c.control.send(Control::Close);
        }
    }
}
fn problem(code: &str, detail: impl Into<String>) -> UiProblem {
    UiProblem {
        code: code.into(),
        params: HashMap::from([("detail".into(), detail.into())]),
    }
}
fn state(
    app: &tauri::AppHandle,
    run: &str,
    conversation: &str,
    state: &str,
    problem: Option<UiProblem>,
) {
    let _ = app.emit_to(
        "main",
        "helper://codex-run",
        json!({"runId":run,"conversationId":conversation,"state":state,"problem":problem}),
    );
}
fn probe(settings: &HelperSettings) -> CodexStatus {
    let mut s = CodexStatus {
        available: false,
        path: None,
        version: None,
        account: "unknown".into(),
        problem: None,
        models: vec![],
        mcp_servers: vec![],
    };
    let result = (|| -> Result<(), UiProblem> {
        let path = client::resolve(settings.codex_path.as_deref())
            .map_err(|e| problem("codex_not_found", e))?;
        s.path = Some(path.to_string_lossy().into_owned());
        let version =
            client::version(&path).map_err(|e| problem("codex_version_unsupported", e))?;
        s.version = Some(version.clone());
        if version != "codex-cli 0.166.0" {
            return Err(problem("codex_version_unsupported", version));
        }
        let mut c = client::Client::spawn(&path, &[]).map_err(|e| problem("codex_crashed", e))?;
        s.account = c
            .initialize(&mut |_| {})
            .map_err(|e| problem("codex_crashed", e))?;
        let config = c
            .call("config/read", json!({"includeLayers":true}), &mut |_| {})
            .map_err(|e| problem("codex_crashed", e))?;
        s.mcp_servers = config::names(&config)
            .into_iter()
            .map(|name| McpServer {
                allowed: settings.allowed_mcp_servers.contains(&name),
                name,
            })
            .collect();
        let mut cursor = Value::Null;
        loop {
            let models = c
                .call("model/list", json!({"cursor":cursor}), &mut |_| {})
                .map_err(|e| problem("codex_crashed", e))?;
            for model in models["data"]
                .as_array()
                .ok_or_else(|| problem("codex_crashed", "missing models"))?
            {
                if let Some(id) = model["id"].as_str() {
                    s.models.push(Model {
                        id: id.into(),
                        label: model["displayName"].as_str().unwrap_or(id).into(),
                        image: model["inputModalities"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|v| v == "image")),
                    });
                }
            }
            cursor = models["nextCursor"].clone();
            if cursor.is_null() {
                break;
            }
            if s.models.len() > 1000 {
                return Err(problem("codex_crashed", "model list limit"));
            }
        }
        if s.account != "chatgpt" {
            return Err(problem(
                if s.account == "none" {
                    "codex_signed_out"
                } else {
                    "codex_not_chatgpt"
                },
                "",
            ));
        }
        s.available = true;
        Ok(())
    })();
    s.problem = result.err();
    s
}
#[tauri::command]
pub async fn helper_codex_status(
    helper: tauri::State<'_, Helper>,
    refresh: Option<bool>,
) -> Result<CodexStatus, UiProblem> {
    let helper = helper.inner().clone();
    tauri::async_runtime::spawn_blocking(move || status(&helper, refresh.unwrap_or(false)))
        .await
        .map_err(|e| problem("codex_crashed", e.to_string()))
}
fn status(helper: &Helper, refresh: bool) -> CodexStatus {
    let settings = helper
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .settings
        .clone();
    let mut cache = helper.codex.cache.lock().unwrap_or_else(|e| e.into_inner());
    if !refresh
        && let Some((old, s)) = &*cache
        && serde_json::to_value(old).ok() == serde_json::to_value(&settings).ok()
    {
        return s.clone();
    }
    let s = probe(&settings);
    *cache = Some((settings, s.clone()));
    s
}
#[tauri::command]
pub async fn helper_codex_run(
    app: tauri::AppHandle,
    helper: tauri::State<'_, Helper>,
    request: CodexRunRequest,
) -> Result<RunInfo, UiProblem> {
    if request.prompt.trim().is_empty() || request.prompt.chars().count() > 4000 {
        return Err(problem(
            "share_required",
            "prompt must contain 1 to 4000 characters",
        ));
    }
    let intent = match (request.intent, request.share) {
        (Some(intent), None) => intent,
        (None, Some(share)) => RequestContextIntent {
            share_page_names: false,
            cells: None,
            destination: RequestDestination {
                root: share.root.clone(),
                page_id: share.page_id.clone(),
            },
            selection: Some(share),
            page_ids: vec![],
            presets: false,
            intent_version: "legacy".into(),
        },
        _ => {
            return Err(problem(
                "invalid_context_intent",
                "send exactly one intent or legacy share",
            ));
        }
    };
    let (usage, capture_generation) = {
        let state = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
        (state.settings.usage, state.generation)
    };
    let override_applied = request_usage(usage, request.usage_override)? != usage;
    let usage = request_usage(usage, request.usage_override)?;
    let mut captured = snapshot::capture(
        Arc::new(crate::workspace::AllowedRoots(
            app.state::<crate::workspace::AllowedRoots>().0.clone(),
        )),
        app.state::<crate::notebook::NotebookHistories>()
            .inner()
            .clone(),
        intent,
        usage,
    )
    .await?;
    captured.usage_override = override_applied;
    let info = captured.info.clone();
    let share = captured.anchor.clone();
    let request_id = captured.request_id.clone();
    let h = helper.inner().clone();
    let s = tauri::async_runtime::spawn_blocking(move || status(&h, false))
        .await
        .map_err(|e| problem("codex_crashed", e.to_string()))?;
    if !s.available {
        return Err(s.problem.unwrap_or_else(|| problem("codex_not_found", "")));
    }
    let settings = helper
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .settings
        .clone();
    if !settings.enabled || !helper.status().listening {
        return Err(problem("share_required", "helper disabled"));
    }
    let run = random_hex(16).map_err(|e| problem("codex_crashed", e))?;
    let conversation = request
        .conversation_id
        .clone()
        .unwrap_or(random_hex(16).map_err(|e| problem("codex_crashed", e))?);
    let mut conversations = helper
        .codex
        .conversations
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let selected_model = Some(resolve_model(
        request.model.as_deref(),
        settings.codex_model.as_deref(),
    ));
    require_model(&s.models, selected_model.as_deref().unwrap())?;
    captured.conversation_id = conversation.clone();
    // Validate the conversation before replacing any live authority.
    if let Some(c) = conversations.get(&conversation) {
        if c.busy {
            return Err(problem("run_busy", "conversation is running"));
        }
        if c.web != request.web_search || c.model != selected_model {
            return Err(problem(
                "confinement_failed",
                "close the conversation before changing model or web search",
            ));
        }
    } else if request.conversation_id.is_some() || conversations.len() >= 2 {
        return Err(problem(
            "run_busy",
            "two conversation limit or unknown conversation",
        ));
    }
    {
        let mut state = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
        if state.generation != capture_generation || !state.settings.enabled || !state.listening {
            return Err(problem(
                "context_changed",
                "context changed before admission",
            ));
        }
        state
            .requests
            .retain(|_, g| g.conversation_id != conversation);
        state.requests.insert(request_id.clone(), captured.clone());
        for session in state
            .sessions
            .values_mut()
            .filter(|s| s.conversation.as_deref() == Some(&conversation))
        {
            session.install_request(captured.clone());
        }
        state.generation += 1;
    }
    let control = Control::Run {
        run: run.clone(),
        prompt: request.prompt,
        share: Box::new(share),
        grant: Box::new(captured),
        info,
    };
    if let Some(c) = conversations.get_mut(&conversation) {
        c.busy = true;
        c.run = run.clone();
        state(&app, &run, &conversation, "starting", None);
        c.control
            .send(control)
            .map_err(|e| problem("codex_crashed", e.to_string()))?;
    } else {
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let actor_cancelled = cancelled.clone();
        let (tx, rx) = mpsc::channel();
        tx.send(control)
            .map_err(|e| problem("codex_crashed", e.to_string()))?;
        conversations.insert(
            conversation.clone(),
            Conversation {
                cancelled,
                model: selected_model.clone(),
                web: request.web_search,
                run: run.clone(),
                busy: true,
                control: tx,
            },
        );
        let h = helper.inner().clone();
        let a = app.clone();
        let id = conversation.clone();
        let model = selected_model;
        let web = request.web_search;
        state(&app, &run, &conversation, "starting", None);
        std::thread::spawn(move || {
            actor(&a, &h, &id, &s, &settings, model, web, rx, actor_cancelled);
            cleanup(&h, &id);
            h.codex
                .conversations
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            h.notify(&a);
        });
    }
    drop(conversations);
    Ok(RunInfo {
        request_id,
        run_id: run,
        conversation_id: conversation,
    })
}
pub(super) fn cleanup(h: &Helper, id: &str) {
    let mut s = h.inner.lock().unwrap_or_else(|e| e.into_inner());
    s.pending_launches.retain(|_, l| l.conversation != id);
    s.requests.retain(|_, g| g.conversation_id != id);
    s.sessions.retain(|_, session| {
        if session.conversation.as_deref() == Some(id) {
            let _ = session.stop.send(true);
            false
        } else {
            true
        }
    });
    s.generation += 1;
}
#[tauri::command]
pub fn helper_codex_stop(
    helper: tauri::State<'_, Helper>,
    run_id: String,
) -> Result<(), UiProblem> {
    let all = helper
        .codex
        .conversations
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (conversation, c) = all
        .iter()
        .find(|(_, c)| c.run == run_id)
        .ok_or_else(|| problem("codex_crashed", "unknown run"))?;
    c.cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
    helper.revoke_conversation(conversation);
    c.control
        .send(Control::Stop)
        .map_err(|e| problem("codex_crashed", e.to_string()))
}
#[tauri::command]
pub fn helper_codex_close(
    app: tauri::AppHandle,
    helper: tauri::State<'_, Helper>,
    conversation_id: String,
) -> Result<(), UiProblem> {
    if let Some(c) = helper
        .codex
        .conversations
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&conversation_id)
    {
        c.cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = c.control.send(Control::Close);
    }
    cleanup(&helper, &conversation_id);
    helper.notify(&app);
    Ok(())
}
pub(super) fn bind(h: &Helper, id: &str, grant: CapturedRequestGrant) {
    let mut s = h.inner.lock().unwrap_or_else(|e| e.into_inner());
    let Some(grant) = s.requests.get(&grant.request_id).cloned() else {
        return;
    };
    for session in s
        .sessions
        .values_mut()
        .filter(|s| s.conversation.as_deref() == Some(id))
    {
        if session
            .request
            .as_ref()
            .is_none_or(|g| g.request_id != grant.request_id)
        {
            session.install_request(grant.clone());
        }
    }
}
#[allow(clippy::too_many_arguments)] // One process actor; these are its immutable launch inputs.
fn actor(
    app: &tauri::AppHandle,
    h: &Helper,
    id: &str,
    status: &CodexStatus,
    settings: &HelperSettings,
    model: Option<String>,
    web: bool,
    controls: mpsc::Receiver<Control>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
) {
    let mut active = String::new();
    let mut turn = String::new();
    let mut thread = String::new();
    let mut client = None;
    let mut stopping = None;
    loop {
        match controls.recv_timeout(Duration::from_millis(20)) {
            Ok(Control::Close) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Control::Stop) => {
                if let Some(c) = &mut client {
                    let c: &mut client::Client = c;
                    let _ = c.send(json!({"id":"interrupt","method":"turn/interrupt","params":{"threadId":thread,"turnId":turn}}));
                    stopping = Some(Instant::now());
                } else {
                    break;
                }
            }
            Ok(Control::Run {
                run,
                prompt,
                share,
                info,
                grant,
            }) => {
                active = run;
                let result = (|| -> Result<(), UiProblem> {
                    if client.is_none() {
                        let token = random_hex(32).map_err(|e| problem("codex_crashed", e))?;
                        let relay = {
                            let mut s = h.inner.lock().unwrap_or_else(|e| e.into_inner());
                            if !s.settings.enabled {
                                return Err(problem("share_required", "helper disabled"));
                            }
                            let relay = s.relay_path.clone().ok_or_else(|| {
                                problem("confinement_failed", "relay unavailable")
                            })?;
                            let current_grant = s
                                .requests
                                .get(&grant.request_id)
                                .cloned()
                                .ok_or_else(|| problem("stale_request", "request revoked"))?;
                            s.pending_launches.insert(
                                token.clone(),
                                Launch {
                                    grant: Some(current_grant),
                                    cancelled: cancelled.clone(),
                                    created: Instant::now(),
                                    conversation: id.into(),
                                    share: (*share).clone(),
                                    info: info.clone(),
                                },
                            );
                            relay
                        };
                        let names = status
                            .mcp_servers
                            .iter()
                            .map(|s| s.name.clone())
                            .collect::<Vec<_>>();
                        let args = config::overrides(
                            &relay,
                            &token,
                            &names,
                            &settings.allowed_mcp_servers,
                            web,
                        );
                        let executable = PathBuf::from(
                            status
                                .path
                                .as_ref()
                                .ok_or_else(|| problem("codex_not_found", ""))?,
                        );
                        let version = client::version(&executable)
                            .map_err(|e| problem("codex_version_unsupported", e))?;
                        if version != "codex-cli 0.166.0" {
                            return Err(problem("codex_version_unsupported", version));
                        }
                        let mut c = client::Client::spawn(
                            &PathBuf::from(
                                status
                                    .path
                                    .as_ref()
                                    .ok_or_else(|| problem("codex_not_found", ""))?,
                            ),
                            &args,
                        )
                        .map_err(|e| problem("confinement_failed", e))?;
                        c.cancelled = cancelled.clone();
                        let account = c
                            .initialize(&mut |v| event(app, h, id, &active, v))
                            .map_err(|e| problem("confinement_failed", e))?;
                        if account != "chatgpt" {
                            return Err(problem(
                                if account == "none" {
                                    "codex_signed_out"
                                } else {
                                    "codex_not_chatgpt"
                                },
                                "",
                            ));
                        }
                        let started=c.call("thread/start",json!({"cwd":c.cwd.path(),"approvalPolicy":"never","sandbox":"read-only","developerInstructions":config::instructions(),"model":model}),&mut |v|event(app,h,id,&active,v)).map_err(|e|problem("confinement_failed",e))?;
                        thread = started["thread"]["id"]
                            .as_str()
                            .ok_or_else(|| problem("codex_crashed", "missing thread id"))?
                            .into();
                        client = Some(c);
                    } else {
                        bind(h, id, *grant);
                    }
                    let c = client.as_mut().unwrap();
                    let config = c
                        .call("config/read", json!({"includeLayers":true}), &mut |v| {
                            event(app, h, id, &active, v)
                        })
                        .map_err(|e| problem("confinement_failed", e))?;
                    let inventory = c
                        .thread_inventory(&thread, &config::names(&config), &mut |v| {
                            event(app, h, id, &active, v)
                        })
                        .map_err(|e| problem("confinement_failed", e))?;
                    config::preflight(&config, &inventory, &settings.allowed_mcp_servers, web)
                        .map_err(|e| problem("confinement_failed", e))?;
                    let result = c
                        .call(
                            "turn/start",
                            turn_params(
                                &thread,
                                model.as_deref().unwrap_or(DEFAULT_MODEL),
                                &prompt,
                            ),
                            &mut |v| event(app, h, id, &active, v),
                        )
                        .map_err(|e| problem("codex_crashed", e))?;
                    turn = result["turn"]["id"]
                        .as_str()
                        .ok_or_else(|| problem("codex_crashed", "missing turn id"))?
                        .into();
                    Ok(())
                })();
                if let Err(e) = result {
                    state(
                        app,
                        &active,
                        id,
                        if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
                            "stopped"
                        } else {
                            "failed"
                        },
                        Some(e),
                    );
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Some(c) = &mut client {
            while let Ok(value) = c.incoming.try_recv() {
                match value {
                    Ok(v) => {
                        let terminal = v["method"] == "turn/completed";
                        let _ = c.event(v, &mut |v| event(app, h, id, &active, v));
                        if terminal {
                            if let Some(conv) = h
                                .codex
                                .conversations
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .get_mut(id)
                            {
                                conv.busy = false;
                            }
                            if stopping.is_some() {
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        state(
                            app,
                            &active,
                            id,
                            "failed",
                            Some(problem("codex_crashed", e)),
                        );
                        return;
                    }
                }
            }
        }
        if stopping.is_some_and(|t: Instant| t.elapsed() >= Duration::from_secs(5)) {
            state(app, &active, id, "stopped", None);
            break;
        }
    }
}
fn event(app: &tauri::AppHandle, h: &Helper, id: &str, run: &str, v: &Value) {
    let method = v["method"].as_str().unwrap_or("");
    let p = &v["params"];
    if v.get("id").is_some() {
        let entry = ActivityEntry {
            usage_override: false,
            at: now(),
            session_id: id.into(),
            client_name: "Codex".into(),
            tool: method.into(),
            grant: "none".into(),
            outcome: "denied".into(),
            handed_over: Default::default(),
            detail_code: Some("denied".into()),
        };
        let mut s = h.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.activity.push_front(entry.clone());
        s.activity.truncate(500);
        drop(s);
        let _ = app.emit_to("main", "helper://activity", entry);
    }
    if method == "item/agentMessage/delta" {
        let _ = app.emit_to(
            "main",
            "helper://codex-delta",
            json!({"runId":run,"text":p["delta"],"itemId":p["itemId"]}),
        );
    }
    if matches!(method, "item/started" | "item/completed") && p["item"]["type"] == "mcpToolCall" {
        let item = &p["item"];
        let server = item["server"].as_str().unwrap_or("");
        let tool = item["tool"].as_str().unwrap_or("");
        let phase = if method == "item/started" {
            "start"
        } else if item["status"] == "failed" {
            "error"
        } else {
            "done"
        };
        let _=app.emit_to("main","helper://codex-tool",json!({"runId":run,"server":server,"tool":tool,"phase":phase,"external":server!="goodtype"}));
        if server == "goodtype" {
            let next = match tool {
                "get_context" | "read_page" | "render" => Some("reading"),
                "typst_compile" => Some("checking"),
                "propose_blocks"
                | "propose_block_edit"
                | "propose_cell_edit"
                | "propose_cell_insert" => Some(
                    if phase == "done"
                        && h.inner
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .sessions
                            .values()
                            .filter(|s| s.conversation.as_deref() == Some(id))
                            .any(|s| s.proposals.values().any(|p| p.status == "pending"))
                    {
                        "ready"
                    } else {
                        "preparing"
                    },
                ),
                _ => None,
            };
            if let Some(next) = next {
                state(app, run, id, next, None);
            }
        }
    }
    if method == "turn/completed" {
        if let Some(conversation) = h
            .codex
            .conversations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(id)
        {
            conversation.busy = false;
        }
        let status = p["turn"]["status"].as_str().unwrap_or("failed");
        state(
            app,
            run,
            id,
            match status {
                "completed" => "done",
                "interrupted" => "stopped",
                _ => "failed",
            },
            if status == "failed" {
                Some(problem("codex_crashed", "turn failed"))
            } else {
                None
            },
        );
    }
}

// Null/missing/blank means the app default, never the user's inherited Codex model.
const DEFAULT_MODEL: &str = "gpt-6-luna";
fn resolve_model(request: Option<&str>, saved: Option<&str>) -> String {
    request
        .filter(|v| !v.trim().is_empty())
        .or(saved.filter(|v| !v.trim().is_empty()))
        .unwrap_or(DEFAULT_MODEL)
        .into()
}
fn require_model(models: &[Model], selected: &str) -> Result<(), UiProblem> {
    if models.iter().any(|m| m.id == selected) {
        Ok(())
    } else {
        Err(problem("model_unavailable", selected))
    }
}
fn turn_params(thread: &str, model: &str, prompt: &str) -> Value {
    json!({"threadId":thread,"model":model,"effort":"medium","input":[{"type":"text","text":format!("{prompt}\nThe user granted request-scoped context in Goodtype; call get_context first and use its captured destination. Inspect returned image content blocks. When using JavaScript tools, forward them with image(block); text(result) or JSON.stringify(result) only displays base64 text, not the image. Do not request a reupload merely because you printed image data as text.")}]})
}

fn request_usage(current: Usage, requested: Option<Usage>) -> Result<Usage, UiProblem> {
    match (current, requested) {
        (Usage::Low, Some(Usage::Balanced)) => Ok(Usage::Balanced),
        (_, None) => Ok(current),
        _ => Err(problem(
            "invalid_usage_override",
            "only Low to Balanced is permitted",
        )),
    }
}
#[cfg(test)]
mod usage_override_tests {
    use super::*;
    #[test]
    fn override_is_only_low_to_balanced_and_does_not_change_settings() {
        assert_eq!(
            request_usage(Usage::Low, Some(Usage::Balanced)).unwrap(),
            Usage::Balanced
        );
        assert_eq!(request_usage(Usage::Low, None).unwrap(), Usage::Low);
        for current in [Usage::Low, Usage::Balanced, Usage::Detailed] {
            for requested in [Usage::Low, Usage::Balanced, Usage::Detailed] {
                assert_eq!(
                    request_usage(current, Some(requested)).is_ok(),
                    current == Usage::Low && requested == Usage::Balanced
                );
            }
        }
    }
}
