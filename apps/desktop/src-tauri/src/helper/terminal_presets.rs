//! Terminal-only preset grants. Once reservations are consumed before work starts;
//! successful previews independently retain their exact reviewed continuation.
use super::*;
use serde_json::{Value, json};

pub struct Authority {
    generation: u64,
    grant: Arc<CapturedRequestGrant>,
    once: bool,
}
impl bridge::Bridge {
    pub(super) async fn terminal_preset_authority(
        &self,
        tool: &str,
        args: &Value,
        generation: u64,
    ) -> Result<Arc<CapturedRequestGrant>, String> {
        let bridge = self.clone();
        let tool = tool.to_owned();
        let args = args.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::notebook::with_notebook(&bridge.histories, || {
                let mut state = bridge
                    .helper
                    .inner
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if !state.grant_current(&bridge.id, generation, None) {
                    return Err("changed: terminal grants".into());
                }
                let (share, info) = state.share.clone().ok_or("not shared: selection")?;
                let usage = state.settings.usage;
                let session = state.sessions.get_mut(&bridge.id).ok_or("session ended")?;
                if session.conversation.is_some() {
                    return Err("not shared: terminal authority".into());
                }
                // A matched preview authorizes only propose's exact source/hash, never a new read.
                let continuation = tool == "propose_preset"
                    && session.preset_previews.iter().any(|p| {
                        p.generation == generation
                            && p.epoch == session.epoch
                            && p.request_id.is_empty()
                            && args["candidateHash"].as_str() == Some(&p.candidate_hash)
                            && args["source"].as_str() == Some(&p.source)
                    });
                crate::workspace::ensure_allowed(&bridge.roots, &share.root)?;
                if !continuation
                    && let Some(a) = &session.terminal_presets
                    && a.generation == generation
                {
                    let grant = a.grant.clone();
                    if a.once {
                        session.terminal_presets = None;
                    }
                    return Ok(grant);
                }
                let root = crate::workspace::ensure_allowed(&bridge.roots, &share.root)?;
                let same_root = session.scope_root.as_deref() == Some(&share.root);
                let notebook = same_root && (session.notebook || session.once_notebook);
                let mut pages = if same_root {
                    session.pages.keys().cloned().collect::<Vec<_>>()
                } else {
                    vec![]
                };
                if notebook {
                    pages = goodtype_core::storage::open_page(&root, &share.page_id)
                        .map_err(|_| "context_page_unavailable")?
                        .manifest
                        .pages
                        .iter()
                        .map(|p| p.id.clone())
                        .collect();
                }
                let selection = info.session_ids.contains(&bridge.id).then(|| {
                    let mut selection = share.clone();
                    selection.session_ids = None;
                    selection
                });
                if selection.is_none() && pages.is_empty() && !continuation {
                    return Err("not shared: selection or page or presets required".into());
                }
                if !continuation {
                    session.pages.retain(|_, once| !*once);
                    session.once_notebook = false;
                }
                let intent = RequestContextIntent {
                    share_page_names: false,
                    cells: None,
                    destination: RequestDestination {
                        root: share.root,
                        page_id: share.page_id,
                    },
                    selection: if continuation { None } else { selection },
                    page_ids: if continuation { vec![] } else { pages },
                    presets: false,
                    intent_version: generation.to_string(),
                };
                let mut grant = snapshot::capture_under_lock(&bridge.roots, intent, usage)
                    .map_err(|e| e.code)?;
                grant.request_id.clear();
                grant.epoch = session.epoch;
                Ok(Arc::new(grant))
            })
        })
        .await
        .map_err(|e| e.to_string())?
    }
    pub(super) async fn terminal_preset_scope(
        &self,
        args: Value,
        generation: u64,
    ) -> Result<Vec<rmcp::model::ContentBlock>, String> {
        let reason = args["reason"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.chars().count() <= 200)
            .ok_or("invalid scope reason")?;
        if args["scope"] != json!({"kind":"presets"}) {
            return Err("invalid scope".into());
        }
        let (root, page, epoch) = {
            let s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            if !s.grant_current(&self.id, generation, None) {
                return Err("changed: terminal grants".into());
            }
            let (share, info) = s.share.as_ref().ok_or("not shared: selection")?;
            let session = &s.sessions[&self.id];
            if !info.session_ids.contains(&self.id)
                && !(session.scope_root.as_deref() == Some(&share.root)
                    && (session.notebook || session.once_notebook || !session.pages.is_empty()))
            {
                return Err("not shared: selection or page".into());
            }
            (
                share.root.clone(),
                share.page_id.clone(),
                s.sessions[&self.id].epoch,
            )
        };
        let bridge = self.clone();
        let root2 = root.clone();
        let targets = tauri::async_runtime::spawn_blocking(move || {
            crate::notebook::with_notebook(&bridge.histories, || {
                let root = crate::workspace::ensure_allowed(&bridge.roots, &root2)?;
                goodtype_core::storage::list_typst_style_entries(&root)
                    .map(|v| v.into_iter().map(|e| e.path).collect::<Vec<_>>())
                    .map_err(|e| e.to_string())
            })
        })
        .await
        .map_err(|e| e.to_string())??;
        let id = random_hex(16)?;
        let (tx, rx) = oneshot::channel();
        let event = {
            let mut s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            if !s.grant_current(&self.id, generation, None) {
                return Err("changed: terminal grants".into());
            }
            let session = &s.sessions[&self.id];
            if session.conversation.is_some() {
                return Err("not shared: terminal authority".into());
            }
            let event = json!({"requestId":id,"sessionId":self.id,"clientName":session.info.client_name,"scope":args["scope"],"reason":reason,"targets":targets});
            s.scopes.insert(id.clone(), (self.id.clone(), tx));
            #[cfg(test)]
            if let Some(tx) = &s.scope_requests {
                let _ = tx.send(event.clone());
            }
            event
        };
        if let Some(app) = &self.app
            && let Err(error) = app.emit_to("main", "helper://scope-request", event)
        {
            self.helper
                .inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .scopes
                .remove(&id);
            return Err(error.to_string());
        }
        let answer = tokio::time::timeout(Duration::from_secs(120), rx)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or("deny".into());
        self.helper
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .scopes
            .remove(&id);
        if let Some(app) = &self.app {
            let _ = app.emit_to("main", "helper://scope-expired", json!({"requestId":id}));
        }
        if answer != "once" && answer != "session" {
            return Err("scope denied".into());
        }
        let bridge = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::notebook::with_notebook(&bridge.histories, || {
                let mut s = bridge
                    .helper
                    .inner
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if !s.grant_current(&bridge.id, generation, None)
                    || s.sessions[&bridge.id].epoch != epoch
                {
                    return Err("changed: terminal approval".into());
                }
                let allowed = crate::workspace::ensure_allowed(&bridge.roots, &root)?;
                let current = goodtype_core::storage::list_typst_style_entries(&allowed)
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .map(|e| e.path)
                    .collect::<Vec<_>>();
                if targets != current {
                    return Err("changed: preset targets".into());
                }
                let mut grant = snapshot::capture_under_lock(
                    &bridge.roots,
                    RequestContextIntent {
                        share_page_names: false,
                        cells: None,
                        destination: RequestDestination {
                            root,
                            page_id: page,
                        },
                        selection: None,
                        page_ids: vec![],
                        presets: true,
                        intent_version: generation.to_string(),
                    },
                    s.settings.usage,
                )
                .map_err(|e| e.code)?;
                let session = s.sessions.get_mut(&bridge.id).ok_or("session ended")?;
                session.epoch += 1;
                grant.request_id.clear();
                grant.epoch = session.epoch;
                session.terminal_presets = Some(Authority {
                    generation,
                    grant: Arc::new(grant),
                    once: answer == "once",
                });
                Ok(vec![rmcp::model::ContentBlock::text("Scope approved")])
            })
        })
        .await
        .map_err(|e| e.to_string())?
    }
}
