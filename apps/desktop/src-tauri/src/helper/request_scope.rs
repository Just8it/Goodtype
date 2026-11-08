//! In-app approvals never mutate terminal grants or reset per-message usage.
use super::*;
use serde_json::{Value, json};

pub(super) struct Receipt {
    pub grant: Arc<CapturedRequestGrant>,
    pub event: Value,
}

pub(super) const SCOPE_SHAPE: &str = "use {kind:\"pages\",pages:[<page number>]} (at most 8; a Python page shares its cells and saved outputs), {kind:\"presets\"} or {kind:\"page_names\"}";

/// A scope request normalized from the shapes assistants actually send.
#[derive(Debug, PartialEq)]
pub(super) enum Scope {
    /// Page ids and their 1-based numbers, in request order.
    Pages(Vec<String>, Vec<usize>),
    Presets,
    PageNames,
    Notebook,
}
impl Scope {
    /// The canonical form shown on approval cards and in scope events.
    pub fn wire(&self) -> Value {
        match self {
            Self::Pages(_, numbers) => json!({"kind":"pages","pages":numbers}),
            Self::Presets => json!({"kind":"presets"}),
            Self::PageNames => json!({"kind":"page_names"}),
            Self::Notebook => json!({"kind":"notebook"}),
        }
    }
}
/// Accepts page numbers or page IDs under `pages`, `page`, `pageIds` or `pageId`, and treats
/// `page`, `cells` and `python_cells` as page requests: attached Python pages share cells anyway.
pub(super) fn parse_scope(
    scope: &Value,
    b: &goodtype_core::storage::NotebookSnapshot,
) -> Result<Scope, String> {
    let invalid = || format!("invalid scope: {SCOPE_SHAPE}");
    let refs: Vec<&Value> = ["pages", "pageIds", "page", "pageId", "pageNumber"]
        .iter()
        .flat_map(|key| match &scope[*key] {
            Value::Array(items) => items.iter().collect(),
            Value::Null => vec![],
            item => vec![item],
        })
        .collect();
    let kind = scope["kind"]
        .as_str()
        .unwrap_or(if refs.is_empty() { "" } else { "pages" });
    match kind {
        "presets" | "preset" | "styles" => return Ok(Scope::Presets),
        "page_names" | "pageNames" | "names" => return Ok(Scope::PageNames),
        "notebook" => return Ok(Scope::Notebook),
        "pages" | "page" | "cells" | "python" | "python_cells" | "python_page" => {}
        _ => return Err(invalid()),
    }
    let (mut ids, mut numbers) = (Vec::new(), Vec::new());
    for item in refs {
        let number = match item {
            Value::Number(n) => n.as_u64().map(|n| n as usize),
            Value::String(s) => s.trim().parse::<usize>().ok().or_else(|| {
                b.manifest
                    .pages
                    .iter()
                    .position(|p| p.id == *s)
                    .map(|i| i + 1)
            }),
            _ => None,
        };
        let number = number.ok_or_else(|| format!("invalid page {item}: {SCOPE_SHAPE}"))?;
        let id = number
            .checked_sub(1)
            .and_then(|i| b.manifest.pages.get(i))
            .ok_or_else(|| {
                format!(
                    "invalid page number {number}: the notebook has {} pages",
                    b.manifest.pages.len()
                )
            })?
            .id
            .clone();
        if !ids.contains(&id) {
            ids.push(id);
            numbers.push(number);
        }
    }
    if ids.is_empty() || ids.len() > 8 {
        return Err(invalid());
    }
    Ok(Scope::Pages(ids, numbers))
}
impl bridge::Bridge {
    pub(super) async fn request_scope(
        &self,
        old: Arc<CapturedRequestGrant>,
        args: Value,
    ) -> Result<Receipt, String> {
        let reason = args["reason"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.chars().count() <= 200)
            .ok_or("invalid scope reason")?
            .to_owned();
        let parsed = parse_scope(&args["scope"], &old.destination)?;
        let presets = parsed == Scope::Presets;
        let names = parsed == Scope::PageNames;
        let ids = match &parsed {
            Scope::Pages(ids, _) => ids.clone(),
            Scope::Notebook => {
                return Err(format!(
                    "scope unavailable: Goodtype does not hand over the whole notebook in the app; {SCOPE_SHAPE}"
                ));
            }
            _ => vec![],
        };
        let mut args = args;
        args["scope"] = parsed.wire();
        if names && old.share_page_names {
            return Err("already granted: page names".into());
        }
        let permission = {
            let state = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            let p = state.settings.permissions;
            if presets {
                p.presets
            } else if names {
                p.page_names
            } else {
                p.pages
            }
        };
        if permission == Permission::Never {
            return Err("scope denied: blocked in Goodtype settings".into());
        }
        let bridge = self.clone();
        let root = old.root.clone();
        let targets = if presets {
            tauri::async_runtime::spawn_blocking(move || {
                crate::notebook::with_notebook(&bridge.histories, || {
                    let root = crate::workspace::ensure_allowed(&bridge.roots, &root)?;
                    goodtype_core::storage::list_typst_style_entries(&root)
                        .map(|v| v.into_iter().map(|e| e.path).collect::<Vec<_>>())
                        .map_err(|e| e.to_string())
                })
            })
            .await
            .map_err(|e| e.to_string())??
        } else {
            vec![]
        };
        let id = random_hex(16)?;
        let (event, answer) = if permission == Permission::Allow {
            let state = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            if !state.grant_current(&self.id, 0, Some(&old)) {
                return Err("changed: request changed".into());
            }
            let event = json!({"requestId":id,"sessionId":self.id,"clientName":state.sessions[&self.id].info.client_name,"conversationId":old.conversation_id,"contextRequestId":old.request_id,"root":old.root,"epoch":old.epoch.to_string(),"pageIds":ids,"scope":args["scope"],"targets":targets,"reason":reason,"automatic":true});
            (event, "request".to_owned())
        } else {
            self.ask_scope(&old, &id, &ids, &args, &targets, &reason)
                .await?
        };
        if answer != "request" {
            return Err("scope denied by the user".into());
        }
        self.install_scope(old, event, ids, targets, presets, names)
            .await
    }

    /// Shows the approval card and waits for the user's answer (deny after two minutes).
    async fn ask_scope(
        &self,
        old: &Arc<CapturedRequestGrant>,
        id: &str,
        ids: &[String],
        args: &Value,
        targets: &[String],
        reason: &str,
    ) -> Result<(Value, String), String> {
        let id = id.to_owned();
        let (tx, rx) = oneshot::channel();
        let event = {
            let mut state = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            if !state.grant_current(&self.id, 0, Some(old)) {
                return Err("changed: request changed".into());
            }
            let event = json!({"requestId":id,"sessionId":self.id,"clientName":state.sessions[&self.id].info.client_name,"conversationId":old.conversation_id,"contextRequestId":old.request_id,"root":old.root,"epoch":old.epoch.to_string(),"pageIds":ids,"scope":args["scope"],"targets":targets,"reason":reason});
            state.scopes.insert(id.clone(), (self.id.clone(), tx));
            #[cfg(test)]
            if let Some(sender) = &state.scope_requests {
                let _ = sender.send(event.clone());
            }
            event
        };
        if let Some(app) = &self.app
            && let Err(e) = app.emit_to("main", "helper://scope-request", &event)
        {
            self.helper
                .inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .scopes
                .remove(&id);
            return Err(e.to_string());
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
        Ok((event, answer))
    }

    /// Captures the approved context and installs it as the request's new grant.
    async fn install_scope(
        &self,
        old: Arc<CapturedRequestGrant>,
        event: Value,
        ids: Vec<String>,
        targets: Vec<String>,
        presets: bool,
        names: bool,
    ) -> Result<Receipt, String> {
        let bridge = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::notebook::with_notebook(&bridge.histories, || {
                // Lock order is notebook -> helper. Keep both through capture and installation.
                let mut state = bridge
                    .helper
                    .inner
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if !state.grant_current(&bridge.id, 0, Some(&old)) {
                    return Err("changed: request changed during approval".into());
                }
                let root = crate::workspace::ensure_allowed(&bridge.roots, &old.root)?;
                if presets {
                    let current = goodtype_core::storage::list_typst_style_entries(&root)
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .map(|e| e.path)
                        .collect::<Vec<_>>();
                    if current != targets {
                        return Err("changed: preset targets changed".into());
                    }
                }
                // Python pages are captured as cells; every other page as ordinary content.
                let mut pages = old.pages.keys().cloned().collect::<Vec<_>>();
                pages.extend(
                    old.cells
                        .keys()
                        .filter(|id| **id != old.destination.page.id)
                        .cloned(),
                );
                pages.extend(ids.clone());
                pages.sort();
                pages.dedup();
                let cells = old
                    .cells
                    .get(&old.destination.page.id)
                    .map(|c| CellGrantIntent {
                        calculation_id: c.calculation_id.clone(),
                        part_id: c.part_id.clone(),
                        cell_ids: c.cells.iter().map(|c| c.id.clone()).collect(),
                        include_outputs: c.include_outputs,
                    });
                let intent = RequestContextIntent {
                    share_page_names: old.share_page_names || names,
                    cells,
                    destination: RequestDestination {
                        root: old.root.clone(),
                        page_id: old.destination.page.id.clone(),
                    },
                    selection: None,
                    page_ids: pages,
                    presets,
                    intent_version: old.intent_version.clone(),
                };
                let captured = snapshot::capture_under_lock(&bridge.roots, intent, old.usage)
                    .map_err(|e| format!("scope denied: {}", e.code))?;
                let previous_files = old.files();
                if captured
                    .files()
                    .iter()
                    .any(|(path, bytes)| previous_files.get(path).is_some_and(|old| old != bytes))
                {
                    return Err("changed: previously granted dependencies changed".into());
                }
                let mut next = (*old).clone();
                for (id, page) in captured.pages {
                    next.pages.entry(id).or_insert(page);
                }
                if presets {
                    next.preset_files = captured.preset_files;
                }
                for (id, cells) in captured.cells {
                    next.cells.entry(id).or_insert(cells);
                }
                next.share_page_names = next.share_page_names || names;
                let bytes = next
                    .pages
                    .values()
                    .chain(next.selection.iter())
                    .map(|p| {
                        serde_json::to_vec(&snapshot::captured_text(p))
                            .map_or(usize::MAX, |s| s.len())
                    })
                    .sum::<usize>();
                let files = next.files();
                let render_bytes = next
                    .pages
                    .values()
                    .chain(next.selection.iter())
                    .map(|p| {
                        p.render_files.values().map(Vec::len).sum::<usize>()
                            + serde_json::to_vec(&p.snapshot.ink_layers)
                                .map_or(usize::MAX / 512, |v| v.len())
                    })
                    .sum::<usize>();
                if bytes > next.usage.limits().0
                    || files.len() > 256
                    || files.values().map(Vec::len).sum::<usize>() + render_bytes + bytes
                        > 32 * 1024 * 1024
                {
                    return Err("scope denied: context_limit".into());
                }
                let session = state.sessions.get_mut(&bridge.id).ok_or("session ended")?;
                session.epoch += 1;
                next.epoch = session.epoch;
                let next = Arc::new(next);
                session.request = Some(next.clone());
                for p in session.proposals.values_mut() {
                    if p.status == "pending" {
                        p.status = "stale".into();
                    }
                }
                // Do not reset call/compile counters or modify global/terminal grants.
                if let Some(pending) = state.requests.get_mut(&old.request_id) {
                    *pending = (*next).clone();
                }
                Ok(Receipt { grant: next, event })
            })
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_shapes_assistants_send_resolve_to_pages() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/notebooks/minimal");
        let b = goodtype_core::storage::open_page(&fixture, "page-001").unwrap();
        let id = b.manifest.pages[0].id.clone();
        let pages = |numbers: Vec<usize>| Scope::Pages(vec![id.clone(); numbers.len()], numbers);
        // Shapes recorded from real Codex sessions that used to fail as "notebook expansion".
        for scope in [
            json!({"kind":"page","page":1}),
            json!({"kind":"python_cells","pageId":id}),
            json!({"kind":"cells","page":1}),
            json!({"kind":"pages","pages":[1,1]}),
            json!({"pages":["1"]}),
        ] {
            assert_eq!(parse_scope(&scope, &b).unwrap(), pages(vec![1]), "{scope}");
        }
        assert_eq!(
            parse_scope(&json!({"kind":"page","page":1}), &b)
                .unwrap()
                .wire(),
            json!({"kind":"pages","pages":[1]})
        );
        assert_eq!(
            parse_scope(&json!({"kind":"presets"}), &b).unwrap(),
            Scope::Presets
        );
        assert_eq!(
            parse_scope(&json!({"kind":"notebook"}), &b).unwrap(),
            Scope::Notebook
        );
        for scope in [
            json!({"kind":"page","page":99}),
            json!({"kind":"pages","pages":[]}),
            json!({"kind":"pages","pages":["missing"]}),
            json!({"kind":"everything"}),
            json!({}),
        ] {
            let error = parse_scope(&scope, &b).unwrap_err();
            assert!(error.starts_with("invalid"), "{scope}: {error}");
        }
    }
}
