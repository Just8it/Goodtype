mod bridge;
pub(crate) mod cells;
pub mod codex;
pub(crate) mod new_page;
#[cfg(windows)]
mod pipe;
pub(crate) mod preset_install;
#[cfg(test)]
mod proposal_tests;
pub(crate) mod proposals;
mod request_scope;
#[cfg(windows)]
mod security;
mod snapshot;
mod terminal_presets;
#[cfg(test)]
mod tests;
mod types;
pub use types::*;

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};
use tokio::sync::{Semaphore, oneshot};

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn random_hex(bytes: usize) -> Result<String, String> {
    let mut value = vec![0; bytes];
    getrandom::fill(&mut value).map_err(|e| e.to_string())?;
    Ok(value.iter().map(|b| format!("{b:02x}")).collect())
}
pub struct Session {
    pub launch_cancelled: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub conversation: Option<String>,
    pub run_share: Option<(SharePayload, ShareInfo)>,
    pub request: Option<Arc<CapturedRequestGrant>>,
    pub epoch: u64,
    pub terminal_presets: Option<terminal_presets::Authority>,
    pub preset_previews: VecDeque<preset_install::ReviewedPreset>,
    pub proposals: HashMap<String, proposals::Proposal>,
    pub info: SessionInfo,
    pub pages: HashMap<String, bool>,
    pub notebook: bool,
    pub scope_root: Option<String>,
    pub once_notebook: bool,
    pub calls: usize,
    pub compiles: usize,
    pub last_call: Instant,
    pub compile: Arc<Semaphore>,
    pub render: Arc<Semaphore>,
    pub stop: tokio::sync::watch::Sender<bool>,
}
impl Session {
    fn reset(&mut self) {
        self.calls = 0;
        self.compiles = 0;
        self.last_call = Instant::now();
    }
    pub fn admit(&mut self, usage: Usage, tool: &str) -> Result<(), String> {
        if self
            .launch_cancelled
            .as_ref()
            .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
        {
            return Err("session ended".into());
        }
        if self.request.as_ref().is_some_and(|g| {
            g.session_id != self.info.id
                || self.conversation.as_deref() != Some(&g.conversation_id)
                || g.epoch != self.epoch
        }) {
            return Err("not shared: request identity mismatch".into());
        }
        if tool != "status" && !self.info.approved {
            return Err("Waiting for approval in Goodtype".into());
        }
        if self.conversation.is_none() && self.last_call.elapsed() >= Duration::from_secs(120) {
            self.reset();
        }
        self.last_call = Instant::now();
        let (_, _, compile, all, _) = usage.limits();
        if self.calls >= all
            || (matches!(tool, "typst_compile" | "preset_preview" | "propose_preset")
                && self.compiles >= compile)
        {
            return Err("limit_reached: ask the user before continuing".into());
        }
        self.calls += 1;
        if matches!(tool, "typst_compile" | "preset_preview" | "propose_preset") {
            self.compiles += 1;
        }
        Ok(())
    }
}
#[derive(Default)]
pub struct Inner {
    pub pending_launches: HashMap<String, codex::Launch>,
    pub requests: HashMap<String, CapturedRequestGrant>,
    pub settings: HelperSettings,
    pub listening: bool,
    pub relay_path: Option<String>,
    pub relay_problem: Option<UiProblem>,
    pub skill_path: Option<String>,
    pub sessions: HashMap<String, Session>,
    pub share: Option<(SharePayload, ShareInfo)>,
    pub generation: u64,
    pub enable_generation: u64,
    pub contexts: HashMap<String, (String, oneshot::Sender<ContextReply>)>,
    pub scopes: HashMap<String, (String, oneshot::Sender<String>)>,
    pub activity: VecDeque<ActivityEntry>,
    #[cfg(test)]
    pub scope_requests: Option<tokio::sync::mpsc::UnboundedSender<serde_json::Value>>,
    #[cfg(test)]
    pub context_requests: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    #[cfg(test)]
    pub before_return: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
    pub listener: Option<tauri::async_runtime::JoinHandle<()>>,
    #[cfg(windows)]
    pub discovery_lock: Option<std::fs::File>,
}
#[derive(Clone)]
pub struct Helper {
    pub inner: Arc<Mutex<Inner>>,
    pub compile: Arc<Semaphore>,
    pub render: Arc<Semaphore>,
    pub codex: Arc<codex::Manager>,
}
impl Default for Helper {
    fn default() -> Self {
        Self {
            codex: Default::default(),
            inner: Default::default(),
            compile: Arc::new(Semaphore::new(2)),
            render: Arc::new(Semaphore::new(2)),
        }
    }
}
impl Helper {
    pub fn status(&self) -> HelperStatus {
        let s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        HelperStatus {
            enabled: s.settings.enabled,
            listening: s.listening,
            relay_path: s.relay_path.clone(),
            relay_problem: s.relay_problem.clone(),
            skill_path: s.skill_path.clone(),
            usage: s.settings.usage,
            sessions: s.sessions.values().map(|s| s.info.clone()).collect(),
            share: s.share.as_ref().map(|(_, i)| i.clone()),
        }
    }
    pub fn notify(&self, app: &tauri::AppHandle) {
        let _ = app.emit_to("main", "helper://status", self.status());
    }
    pub fn stop(&self) {
        self.codex.close_all();
        let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.pending_launches.clear();
        s.requests.clear();
        let owned = s.listener.is_some();
        if let Some(task) = s.listener.take() {
            task.abort();
        }
        for (_, session) in s.sessions.drain() {
            let _ = session.stop.send(true);
        }
        s.contexts.clear();
        s.scopes.clear();
        s.share = None;
        s.listening = false;
        s.generation += 1;
        s.enable_generation += 1;
        #[cfg(windows)]
        {
            s.discovery_lock = None;
            if owned {
                let _ = std::fs::remove_file(pipe::record_path());
            }
        }
    }
    pub fn apply(&self, app: &tauri::AppHandle, settings: HelperSettings) -> Result<(), String> {
        self.apply_settings(settings, || {
            self.install_relay();
            #[cfg(windows)]
            pipe::start(self.clone(), app.clone())?;
            Ok(())
        })?;
        self.notify(app);
        Ok(())
    }
    // Shared by startup and settings saves; callers need no ambient Tokio runtime.
    fn apply_settings(
        &self,
        settings: HelperSettings,
        start: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let (listening, close_conversations) = {
            let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let changed = s.settings.codex_path != settings.codex_path
                || s.settings.allowed_mcp_servers != settings.allowed_mcp_servers;
            s.settings = settings.clone();
            (s.listening, changed)
        };
        // Send/Stop take conversation then helper locks. Never take the inverse order here.
        if close_conversations {
            self.codex.close_all();
        }
        if settings.enabled && !listening {
            let runtime = tauri::async_runtime::handle();
            let _entered = runtime.inner().enter();
            start()?;
        } else if !settings.enabled {
            self.stop();
        }
        Ok(())
    }
    fn install_relay(&self) {
        // Packaged skill sits beside the executable; dev uses the checked-in skill folder.
        let result = (|| -> Result<(String, Option<String>), String> {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let parent = exe.parent().ok_or("app executable has no parent")?;
            let source = parent.join("goodtype-mcp.exe");
            let target = std::path::PathBuf::from(
                std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA missing")?,
            )
            .join("Goodtype/bin/goodtype-mcp.exe");
            let bytes = std::fs::read(&source)
                .map_err(|e| format!("relay not found next to the app: {e}"))?;
            std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
            if std::fs::read(&target).ok().as_ref() != Some(&bytes) {
                use sha2::{Digest, Sha256};
                use std::io::Write;
                let metadata = target.with_extension("protocol.json");
                let mut temp = tempfile::NamedTempFile::new_in(target.parent().unwrap())
                    .map_err(|e| e.to_string())?;
                temp.write_all(&bytes).map_err(|e| e.to_string())?;
                temp.as_file().sync_all().map_err(|e| e.to_string())?;
                if let Err(error) = temp.persist(&target) {
                    let compatible=std::fs::read(&target).ok().zip(std::fs::read(&metadata).ok()).is_some_and(|(old,meta)| {
                        let expected=serde_json::json!({"protocol":1,"sha256":format!("{:x}",Sha256::digest(old))});
                        serde_json::from_slice::<serde_json::Value>(&meta).ok()==Some(expected)
                    });
                    if !compatible {
                        return Err(format!(
                            "relay update failed; restart the assistant: {error}"
                        ));
                    }
                } else {
                    crate::store::write_json(&metadata,&serde_json::json!({"protocol":1,"sha256":format!("{:x}",Sha256::digest(&bytes))}),4096).map_err(|e|e.to_string())?;
                }
            }
            let packaged = parent.join("skill/goodtype");
            let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../crates/goodtype-mcp/skill/goodtype");
            let skill = if packaged.is_dir() {
                Some(packaged)
            } else if cfg!(debug_assertions) && dev.is_dir() {
                Some(dev)
            } else {
                None
            };
            Ok((
                target.to_string_lossy().into_owned(),
                skill.map(|p| p.to_string_lossy().into_owned()),
            ))
        })();
        let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok((p, k)) => {
                s.relay_path = Some(p);
                s.skill_path = k;
                s.relay_problem = None;
            }
            Err(_) => {
                s.relay_problem = Some(UiProblem {
                    code: "relay_unavailable".into(),
                    params: HashMap::new(),
                })
            }
        }
    }
    pub async fn context(
        &self,
        app: Option<&tauri::AppHandle>,
        session_id: &str,
    ) -> Result<ContextReply, String> {
        let id = random_hex(16)?;
        let (tx, rx) = oneshot::channel();
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contexts
            .insert(id.clone(), (session_id.to_owned(), tx));
        let emitted = app
            .map(|app| {
                app.emit_to(
                    "main",
                    "helper://context-request",
                    serde_json::json!({"requestId":id}),
                )
            })
            .transpose();
        #[cfg(test)]
        if let Some(tx) = &self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .context_requests
        {
            let _ = tx.send(id.clone());
        }
        let reply = if emitted.is_ok() {
            tokio::time::timeout(Duration::from_secs(5), rx)
                .await
                .ok()
                .and_then(Result::ok)
        } else {
            None
        };
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contexts
            .remove(&id);
        let reply = reply.ok_or("busy: context reply timed out")?;
        if reply.status != "ok" {
            return Err(reply.status);
        }
        if reply.edit_generation.is_none() {
            return Err("busy: missing edit generation".into());
        }
        Ok(reply)
    }
}

#[tauri::command]
pub fn helper_status(helper: tauri::State<'_, Helper>) -> HelperStatus {
    helper.status()
}
#[tauri::command]
pub fn helper_install_relay(
    app: tauri::AppHandle,
    helper: tauri::State<'_, Helper>,
) -> HelperStatus {
    helper.install_relay();
    helper.notify(&app);
    helper.status()
}
#[tauri::command]
pub async fn helper_share(
    app: tauri::AppHandle,
    helper: tauri::State<'_, Helper>,
    share: SharePayload,
) -> Result<ShareInfo, String> {
    if !share.crop_pt.valid() || share.object_ids.len() + share.stroke_ids.len() > 10_000 {
        return Err("invalid selection".into());
    }
    let bundle = snapshot::read(&app, &share.root, &share.page_id).await?;
    if !(Rect {
        x: 0.0,
        y: 0.0,
        width: bundle.page.geometry.width_pt,
        height: bundle.page.geometry.height_pt,
    })
    .contains(share.crop_pt)
    {
        return Err("crop outside page".into());
    }
    snapshot::shared_selection(&bundle, &share.object_ids, &share.stroke_ids)?;
    let mut s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
    if !s.settings.enabled || !s.listening {
        return Err("AI helpers are disabled".into());
    }
    let ids = share.session_ids.clone().unwrap_or_else(|| {
        s.sessions
            .values()
            .filter(|s| s.info.approved && s.conversation.is_none())
            .map(|s| s.info.id.clone())
            .collect()
    });
    if ids.iter().any(|id| {
        !s.sessions
            .get(id)
            .is_some_and(|s| s.info.approved && s.conversation.is_none())
    }) {
        return Err("session not approved".into());
    }
    let info = ShareInfo {
        page_id: share.page_id.clone(),
        page_number: bundle
            .manifest
            .pages
            .iter()
            .position(|p| p.id == share.page_id)
            .ok_or("page missing")?
            + 1,
        object_count: share.object_ids.len() + share.stroke_ids.len(),
        crop_pt: share.crop_pt,
        session_ids: ids,
        shared_at: now(),
    };
    for session in s.sessions.values_mut() {
        if session.conversation.is_some() {
            continue;
        }
        if session.scope_root.as_deref() != Some(&share.root) {
            session.pages.clear();
            session.notebook = false;
            session.once_notebook = false;
            session.scope_root = None;
        }
    }
    s.share = Some((share, info.clone()));
    s.generation += 1;
    for session in s.sessions.values_mut().filter(|s| s.conversation.is_none()) {
        session.terminal_presets = None;
        session.preset_previews.clear();
        session.reset();
    }
    drop(s);
    helper.notify(&app);
    Ok(info)
}
#[tauri::command]
pub fn helper_unshare(app: tauri::AppHandle, helper: tauri::State<'_, Helper>) {
    let mut s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
    s.share = None;
    s.generation += 1;
    for session in s.sessions.values_mut().filter(|s| s.conversation.is_none()) {
        session.terminal_presets = None;
        session.preset_previews.clear();
    }
    drop(s);
    helper.notify(&app);
}
#[tauri::command]
pub fn helper_context_reply(
    helper: tauri::State<'_, Helper>,
    reply: ContextReply,
) -> Result<(), String> {
    let (_, tx) = helper
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contexts
        .remove(&reply.request_id)
        .ok_or("unknown context request")?;
    tx.send(reply).map_err(|_| "context expired".into())
}
#[tauri::command]
pub fn helper_answer_connection(
    app: tauri::AppHandle,
    helper: tauri::State<'_, Helper>,
    session_id: String,
    allow: bool,
) -> Result<(), String> {
    if !allow {
        return helper_revoke(app, helper, session_id);
    }
    helper
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .sessions
        .get_mut(&session_id)
        .ok_or("unknown session")?
        .info
        .approved = true;
    helper.notify(&app);
    Ok(())
}
#[tauri::command]
pub fn helper_answer_scope(
    helper: tauri::State<'_, Helper>,
    request_id: String,
    answer: String,
) -> Result<(), String> {
    if !["once", "session", "deny", "request"].contains(&answer.as_str()) {
        return Err("invalid scope answer".into());
    }
    let (_, tx) = helper
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .scopes
        .remove(&request_id)
        .ok_or("unknown scope request")?;
    tx.send(answer).map_err(|_| "scope request expired".into())
}
#[tauri::command]
pub fn helper_revoke(
    app: tauri::AppHandle,
    helper: tauri::State<'_, Helper>,
    session_id: String,
) -> Result<(), String> {
    let mut s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
    let session = s.sessions.remove(&session_id).ok_or("unknown session")?;
    let _ = session.stop.send(true);
    s.scopes.retain(|_, (id, _)| id != &session_id);
    s.contexts.retain(|_, (id, _)| id != &session_id);
    if let Some((_, share)) = &mut s.share {
        share.session_ids.retain(|id| id != &session_id);
    }
    s.generation += 1;
    drop(s);
    helper.notify(&app);
    Ok(())
}
#[tauri::command]
pub fn helper_activity(helper: tauri::State<'_, Helper>) -> Vec<ActivityEntry> {
    helper
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .activity
        .iter()
        .cloned()
        .collect()
}

impl Inner {
    /// Request epochs are session-local. Terminal scope still uses its existing generation.
    pub fn grant_current(
        &self,
        id: &str,
        generation: u64,
        request: Option<&CapturedRequestGrant>,
    ) -> bool {
        let Some(session) = self.sessions.get(id) else {
            return false;
        };
        if session
            .launch_cancelled
            .as_ref()
            .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
        {
            return false;
        }
        match request {
            Some(request) => {
                request.session_id == id
                    && session.epoch == request.epoch
                    && session.conversation.as_deref() == Some(request.conversation_id.as_str())
                    && session.request.as_ref().is_some_and(|current| {
                        current.request_id == request.request_id
                            && current.session_id == id
                            && current.epoch == request.epoch
                    })
            }
            None => self.generation == generation,
        }
    }
    pub fn share_for(&self, id: &str) -> Option<&(SharePayload, ShareInfo)> {
        match self.sessions.get(id) {
            Some(s) if s.conversation.is_some() => s.run_share.as_ref(),
            Some(_) => self.share.as_ref(),
            None => None,
        }
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    #[test]
    fn preset_limits_and_epoch_reset() {
        assert_eq!(Usage::Low.limits(), (16384, 5, 1, 10, 0));
        assert_eq!(Usage::Balanced.limits(), (65536, 10, 3, 30, 1024));
        assert_eq!(Usage::Detailed.limits(), (262144, 25, 6, 60, 2048));
        let mut s = bridge::new_session("test".into());
        s.info.approved = true;
        for _ in 0..10 {
            s.admit(Usage::Low, "status").unwrap();
        }
        assert!(s.admit(Usage::Low, "status").is_err());
        s.last_call = Instant::now() - Duration::from_secs(121);
        s.admit(Usage::Low, "status").unwrap();
        assert_eq!(s.calls, 1);
    }
}

impl Session {
    pub fn install_request(&mut self, mut grant: CapturedRequestGrant) {
        self.epoch += 1;
        grant.session_id = self.info.id.clone();
        grant.epoch = self.epoch;
        grant.info.session_ids = vec![self.info.id.clone()];
        self.run_share = Some((grant.anchor.clone(), grant.info.clone()));
        self.request = Some(Arc::new(grant));
        self.pages.clear();
        self.notebook = false;
        self.once_notebook = false;
        self.scope_root = None;
        self.reset();
    }
    pub fn revoke_request(&mut self) {
        self.epoch += 1;
        self.request = None;
        self.run_share = None;
        self.pages.clear();
        self.notebook = false;
        self.once_notebook = false;
        self.scope_root = None;
        for proposal in self.proposals.values_mut() {
            if proposal.status == "pending" {
                proposal.status = "stale".into();
            }
        }
    }
}
impl Helper {
    pub fn revoke_attachment(
        &self,
        conversation: &str,
        request: &str,
        page: &str,
    ) -> Result<(), UiProblem> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let owns_pending = state
            .requests
            .get(request)
            .is_some_and(|g| g.conversation_id == conversation);
        let owns_live = state.sessions.values().any(|s| {
            s.conversation.as_deref() == Some(conversation)
                && s.request.as_ref().is_some_and(|g| g.request_id == request)
        });
        if !owns_pending && !owns_live {
            return Err(UiProblem {
                code: "stale_request".into(),
                params: Default::default(),
            });
        }
        if let Some(pending) = state
            .requests
            .get_mut(request)
            .filter(|g| g.conversation_id == conversation)
        {
            pending.pages.remove(page);
            if page == "@page_names" {
                pending.share_page_names = false;
            }
            // "@cells" is the active-cell chip, which always belongs to the destination page.
            let cells_page = if page == "@cells" {
                pending.destination.page.id.clone()
            } else {
                page.to_owned()
            };
            pending.cells.remove(&cells_page);
            if page == "@presets" {
                pending.preset_files.clear();
            }
        }
        // Pending launches also contain bytes and must be invalidated before admission.
        for launch in state
            .pending_launches
            .values_mut()
            .filter(|l| l.conversation == conversation)
        {
            if let Some(grant) = &mut launch.grant {
                if grant.request_id != request {
                    return Err(UiProblem {
                        code: "stale_request".into(),
                        params: Default::default(),
                    });
                }
                grant.pages.remove(page);
                if page == "@page_names" {
                    grant.share_page_names = false;
                }
                // "@cells" is the active-cell chip, which always belongs to the destination page.
                let cells_page = if page == "@cells" {
                    grant.destination.page.id.clone()
                } else {
                    page.to_owned()
                };
                grant.cells.remove(&cells_page);
                if page == "@presets" {
                    grant.preset_files.clear();
                }
            }
        }
        let mut found = false;
        for session in state
            .sessions
            .values_mut()
            .filter(|s| s.conversation.as_deref() == Some(conversation))
        {
            let Some(old) = session.request.as_ref() else {
                continue;
            };
            if old.request_id != request {
                return Err(UiProblem {
                    code: "stale_request".into(),
                    params: Default::default(),
                });
            }
            let mut grant = (**old).clone();
            grant.pages.remove(page);
            if page == "@page_names" {
                grant.share_page_names = false;
            }
            // "@cells" is the active-cell chip, which always belongs to the destination page.
            let cells_page = if page == "@cells" {
                grant.destination.page.id.clone()
            } else {
                page.to_owned()
            };
            grant.cells.remove(&cells_page);
            if page == "@presets" {
                grant.preset_files.clear();
            }
            session.epoch += 1;
            grant.epoch = session.epoch;
            session.request = Some(Arc::new(grant));
            for proposal in session.proposals.values_mut() {
                if proposal.status == "pending" {
                    proposal.status = "stale".into();
                }
            }
            found = true;
        }
        state.generation += 1;
        if found || owns_pending {
            Ok(())
        } else {
            Err(UiProblem {
                code: "stale_request".into(),
                params: Default::default(),
            })
        }
    }
    pub fn revoke_notebook(&self, root: &str) {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.requests.retain(|_, g| g.root != root);
        state
            .pending_launches
            .retain(|_, l| l.grant.as_ref().is_none_or(|g| g.root != root));
        if state
            .share
            .as_ref()
            .is_some_and(|(share, _)| share.root == root)
        {
            state.share = None;
        }
        for session in state.sessions.values_mut() {
            if session.scope_root.as_deref() == Some(root) {
                session.pages.clear();
                session.notebook = false;
                session.once_notebook = false;
                session.scope_root = None;
            }
            for proposal in session
                .proposals
                .values_mut()
                .filter(|p| p.root == root && p.status == "pending")
            {
                proposal.status = "stale".into();
            }
            if session.request.as_ref().is_some_and(|g| g.root == root) {
                session.revoke_request();
            }
        }
        state.generation += 1;
    }
}
#[tauri::command]
pub fn helper_revoke_attachment(
    helper: tauri::State<'_, Helper>,
    conversation_id: String,
    request_id: String,
    page_id: String,
) -> Result<(), UiProblem> {
    helper.revoke_attachment(&conversation_id, &request_id, &page_id)
}
#[tauri::command]
pub fn helper_revoke_notebook(helper: tauri::State<'_, Helper>, root: String) {
    helper.revoke_notebook(&root);
}

#[cfg(test)]
impl Helper {
    pub async fn pause_before_return(&self) {
        let hook = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .before_return
            .clone();
        if let Some((ready, release)) = hook {
            ready.notify_one();
            release.notified().await;
        }
    }
}
// Preserve the existing close IPC while revoking helper authority synchronously.
#[tauri::command(rename = "close_notebook_session")]
pub fn helper_close_notebook_session(
    app: tauri::AppHandle,
    roots: tauri::State<'_, crate::workspace::AllowedRoots>,
    histories: tauri::State<'_, crate::notebook::NotebookHistories>,
    computation: tauri::State<'_, crate::computation::RuntimeState>,
    helper: tauri::State<'_, Helper>,
    root: String,
) -> Result<(), String> {
    let canonical = crate::workspace::ensure_allowed(&roots, &root)?;
    helper.revoke_notebook(&canonical.to_string_lossy());
    crate::workspace::close_notebook_session(app, roots, histories, computation, root)
}

impl Helper {
    pub(super) fn revoke_conversation(&self, conversation: &str) {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state
            .requests
            .retain(|_, g| g.conversation_id != conversation);
        state
            .pending_launches
            .retain(|_, l| l.conversation != conversation);
        for session in state
            .sessions
            .values_mut()
            .filter(|s| s.conversation.as_deref() == Some(conversation))
        {
            session.revoke_request();
        }
        state.generation += 1;
    }
}
