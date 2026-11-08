//! Session-local, compiled proposals. Never canonical until a validated commit succeeds.
use super::*;
use goodtype_core::{
    PageObject,
    storage::{self, NotebookSnapshot, ProposalBase},
};
use goodtype_typst::{CompileRequest, GrantedFiles};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

// MCP clients commonly parse JSON through JavaScript; u64 fingerprints exceed its exact range.
pub(super) fn base_to_wire(base: &ProposalBase) -> Value {
    json!({
        "pageId":base.page_id,
        "revision":base.revision.to_string(),
        "objects":base.objects.iter().map(|(id, hash)| (id.clone(), hash.to_string())).collect::<std::collections::BTreeMap<_, _>>(),
        "pageText":base.page_text.map(|n|n.to_string()),
        "preset":base.preset.map(|n|n.to_string()),
        "sharedStyle":base.shared_style.map(|n|n.to_string()),
    })
}

fn base_from_wire(value: &Value) -> Result<ProposalBase, String> {
    let number = |v: &Value| -> Result<u64, String> {
        v.as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| v.as_u64())
            .ok_or_else(|| "invalid proposalBase".into())
    };
    let optional = |v: &Value| {
        if v.is_null() {
            Ok(None)
        } else {
            number(v).map(Some)
        }
    };
    Ok(ProposalBase {
        page_id: value["pageId"]
            .as_str()
            .ok_or("invalid proposalBase")?
            .to_owned(),
        revision: number(&value["revision"])?,
        objects: value["objects"]
            .as_object()
            .ok_or("invalid proposalBase")?
            .iter()
            .map(|(id, v)| Ok((id.clone(), number(v)?)))
            .collect::<Result<_, String>>()?,
        page_text: optional(&value["pageText"])?,
        preset: optional(&value["preset"])?,
        shared_style: optional(&value["sharedStyle"])?,
    })
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    #[test]
    fn proposal_fingerprints_survive_javascript_json_roundtrip() {
        let base = ProposalBase {
            page_id: "p".into(),
            revision: u64::MAX,
            objects: std::collections::BTreeMap::from([(
                "block".into(),
                4_289_289_204_561_653_629,
            )]),
            page_text: Some(u64::MAX),
            preset: None,
            shared_style: Some(0),
        };
        let wire = base_to_wire(&base);
        assert_eq!(wire["objects"]["block"], "4289289204561653629");
        assert_eq!(base_from_wire(&wire).unwrap(), base);
        // Legacy exact integer clients still work; rounded or changed values stay stale.
        assert_eq!(
            base_from_wire(&serde_json::to_value(&base).unwrap()).unwrap(),
            base
        );
        for invalid in [
            json!("18446744073709551616"),
            json!("4.2"),
            json!(-1),
            json!(1.5),
        ] {
            let mut bad = wire.clone();
            bad["objects"]["block"] = invalid;
            assert!(base_from_wire(&bad).is_err());
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_page: Option<super::new_page::PageProposal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<super::cells::CellProposal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<super::preset_install::ReviewedPreset>,
    pub id: String,
    pub session_id: String,
    pub root: String,
    pub page_id: String,
    pub version: u64,
    pub status: String,
    pub text: Option<String>,
    pub changed: Vec<storage::ChangedDependency>,
    pub summary: String,
    pub assumptions: Vec<String>,
    pub edit: bool,
    /// `blocks`, `block_edit` or `page_text_edit`.
    pub kind: String,
    /// One-based destination page number for display; 0 when not captured.
    pub page_number: usize,
    /// Page text content extends past the page bottom. Apply refuses until revised.
    pub overflow: bool,
    /// The exact read-only managed preset header of a Page text target.
    pub managed_header: Option<String>,
    /// The captured destination page a Page text proposal compiles against.
    #[serde(skip)]
    pub page_snapshot: Option<NotebookSnapshot>,
    /// Granted file paths a Page text compile reads besides its own source.
    #[serde(skip)]
    pub dependencies: Vec<String>,
    #[serde(skip)]
    pub baseline_grid: bool,
    pub items: Vec<ProposalItem>,
    #[serde(skip)]
    pub base: ProposalBase,
    #[serde(skip)]
    pub files: GrantedFiles,
    #[serde(skip)]
    pub style: String,
    #[serde(skip)]
    pub geometry: goodtype_core::PageGeometry,
    pub anchor: Rect,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalItem {
    pub object: PageObject,
    pub source: String,
    pub hash: String,
    pub svg: String,
    pub before_source: Option<String>,
    pub before_svg: Option<String>,
    pub width_pt: f64,
    pub height_pt: f64,
    pub placement: Value,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposalInsert {
    pub proposal_id: String,
    pub proposal_version: u64,
}

pub fn emit(app: Option<&tauri::AppHandle>, p: &Proposal) {
    if let Some(app) = app {
        let _ = app.emit_to("main", "helper://proposal", p);
    }
}
fn compile(
    root: &std::path::Path,
    source: &str,
    width: f64,
    files: &GrantedFiles,
    style: &str,
    remote: bool,
) -> Result<(String, f64, f64), String> {
    let result = goodtype_typst::compile_block_granted(
        root,
        &CompileRequest {
            source: if style.is_empty() {
                source.into()
            } else {
                format!("{style}\n{source}")
            },
            width_pt: width,
            generation: 0,
            allow_remote_packages: remote,
        },
        files,
    )
    .map_err(|e| e.to_string())?;
    let svg = result
        .svg
        .ok_or_else(|| serde_json::to_string(&result.diagnostics).unwrap_or_default())?;
    Ok((
        svg,
        result.width_pt.unwrap_or(width),
        result.height_pt.unwrap_or(1.0),
    ))
}
pub fn pending_count(session: &Session) -> usize {
    session
        .proposals
        .values()
        .filter(|p| p.status == "pending")
        .count()
}
impl bridge::Bridge {
    pub async fn proposal_status(&self, id: &str) -> Result<Value, String> {
        let p = {
            let mut s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            s.sessions
                .get_mut(&self.id)
                .and_then(|s| s.proposals.get_mut(id))
                .ok_or("unknown proposal")?
                .clone()
        };
        if p.status == "pending"
            && let Some(preset) = &p.preset
        {
            let root = crate::workspace::ensure_allowed(&self.roots, &p.root)?;
            let stale = super::preset_install::validate_dependencies(&root, preset).is_err();
            let mut s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            if stale || !super::preset_install::current(&s, &self.id, preset) {
                let stored = find_mut(&mut s, id)?;
                if stored.version == p.version && stored.status == "pending" {
                    stored.status = "stale".into();
                    emit(self.app.as_ref(), stored);
                }
            }
        }
        if p.status == "pending"
            && let Some(cell) = &p.cell
        {
            let bridge = self.clone();
            let proposal = p.clone();
            let stale = tauri::async_runtime::spawn_blocking(move || {
                crate::notebook::with_notebook(&bridge.histories, || {
                    let root = crate::workspace::ensure_allowed(&bridge.roots, &proposal.root)?;
                    let granted = proposal
                        .cell
                        .as_ref()
                        .unwrap()
                        .grant
                        .cells
                        .get(&proposal.page_id);
                    Ok::<_, String>(
                        granted.is_none_or(|c| !super::cells::current(&root, &proposal.page_id, c)),
                    )
                })
            })
            .await
            .map_err(|e| e.to_string())??;
            let mut state = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            let invalid = !state.grant_current(&self.id, 0, Some(&cell.grant));
            let stored = find_mut(&mut state, id)?;
            if (stale || invalid) && stored.version == p.version && stored.status == "pending" {
                stored.status = "stale".into();
                emit(self.app.as_ref(), stored);
            }
        }
        if p.status == "pending"
            && p.kind != "preset_install"
            && p.cell.is_none()
            && p.new_page.is_none()
        {
            let current = super::snapshot::read_with(
                self.roots.clone(),
                self.histories.clone(),
                &p.root,
                &p.page_id,
            )
            .await?;
            let root = crate::workspace::ensure_allowed(&self.roots, &p.root)?;
            // The same staleness Apply would find: ProposalBase, plus for Page text the page
            // geometry/paper and imported files the fingerprints do not cover.
            let stale = match storage::check_proposal_base(&root, &current, &p.base) {
                Err(storage::StorageError::ProposalStale(changed)) => Some(changed),
                _ if p.kind == "page_text_edit" => {
                    Some(page_text_changes(&p, &root, &current)).filter(|c| !c.is_empty())
                }
                _ => None,
            };
            if let Some(changed) = stale {
                let mut s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
                let current = s
                    .sessions
                    .get_mut(&self.id)
                    .and_then(|s| s.proposals.get_mut(id))
                    .ok_or("session ended")?;
                if current.status == "pending" && current.version == p.version {
                    current.status = "stale".into();
                    current.changed = changed;
                    emit(self.app.as_ref(), current);
                }
            }
        }
        let s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
        let p = s
            .sessions
            .get(&self.id)
            .and_then(|s| s.proposals.get(id))
            .ok_or("session ended")?;
        Ok(json!({"id":p.id,"status":p.status,"text":p.text,"changed":p.changed}))
    }
    #[cfg(test)]
    pub async fn propose(&self, tool: &str, args: Value) -> Result<Value, String> {
        let stamp = {
            let s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            (
                s.generation,
                s.sessions
                    .get(&self.id)
                    .ok_or("session ended")?
                    .request
                    .clone(),
            )
        };
        self.propose_scoped(tool, args, stamp).await
    }
    pub(super) async fn propose_scoped(
        &self,
        tool: &str,
        args: Value,
        stamp: (u64, Option<Arc<CapturedRequestGrant>>),
    ) -> Result<Value, String> {
        if tool == "propose_page" {
            return self.propose_new_page(args, stamp).await;
        }
        if matches!(tool, "propose_cell_edit" | "propose_cell_insert") {
            return self.propose_cell(tool, args, stamp);
        }
        // Blocks and page text cannot land on a Python page; attached Python pages elsewhere are fine.
        if stamp
            .1
            .as_ref()
            .is_some_and(|g| g.destination.page.calculation.is_some())
        {
            return Err(
                "not shared: the destination is a Python page; use cell proposals or propose_page"
                    .into(),
            );
        }
        if tool == "propose_page_text_edit" {
            return self.propose_page_text(args, stamp).await;
        }
        let (share, generation, own, request) = {
            let s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            if !s.grant_current(&self.id, stamp.0, stamp.1.as_deref()) {
                return Err("changed: request changed before proposal dispatch".into());
            }
            let session = s.sessions.get(&self.id).ok_or("session ended")?;
            if pending_count(session) >= 20 {
                return Err("limit: 20 pending proposals; review or dismiss first".into());
            }
            let (share, info) = s
                .share_for(&self.id)
                .ok_or("not shared: selection required")?;
            if !info.session_ids.contains(&self.id) {
                return Err("not shared: selection required".into());
            }
            let request = session.request.clone();
            if session.conversation.is_some() && request.is_none() {
                return Err("not shared: request revoked".into());
            }
            let mut share = share.clone();
            if let Some(request) = &request {
                share.page_id = request.destination.page.id.clone();
                if request.anchor.page_id != share.page_id {
                    share.object_ids.clear();
                    share.stroke_ids.clear();
                    share.crop_pt = Rect {
                        x: 12.,
                        y: 12.,
                        width: 230_f64.min(request.destination.page.geometry.width_pt - 24.),
                        height: 1.,
                    };
                }
            }
            (share, s.generation, session.compile.clone(), request)
        };
        let own = own.try_acquire_owned().map_err(|_| "busy")?;
        let global = self
            .helper
            .compile
            .clone()
            .try_acquire_owned()
            .map_err(|_| "busy")?;
        let b = if let Some(request) = &request {
            request.destination.clone()
        } else {
            super::snapshot::read_with(
                self.roots.clone(),
                self.histories.clone(),
                &share.root,
                &share.page_id,
            )
            .await?
        };
        if tool == "propose_blocks"
            && !args["page"].is_null()
            && args["page"] != share.page_id
            && args["page"].as_u64()
                != Some(
                    b.manifest
                        .pages
                        .iter()
                        .position(|p| p.id == share.page_id)
                        .unwrap_or(usize::MAX) as u64
                        + 1,
                )
        {
            return Err("not shared: destination page".into());
        }
        let selected = super::snapshot::shared_selection(&b, &share.object_ids, &share.stroke_ids)?;
        let base = base_from_wire(&args["proposalBase"])?;
        if base.page_id != share.page_id
            || base.objects.keys().any(|id| !selected.contains(id))
            || selected.iter().any(|id| !base.objects.contains_key(id))
        {
            return Err("not shared: proposal base targets differ from selection".into());
        }
        if let Some(request) = &request
            && base_to_wire(&base) != base_to_wire(&request.destination_base)
        {
            return Err("changed: destination base".into());
        }
        let root = crate::workspace::ensure_allowed(&self.roots, &share.root)?;
        storage::check_proposal_base(&root, &b, &base).map_err(|e| e.to_string())?;
        let filtered = super::snapshot::shared_filtered(&b, &selected, &share.stroke_ids);
        let files = if let Some(request) = &request {
            request.files()
        } else {
            super::snapshot::files(&root, &filtered, &b)?
        };
        let style = if request
            .as_ref()
            .is_some_and(|r| r.source(&r.destination.page.id).is_none())
        {
            String::new()
        } else {
            super::snapshot::export(&filtered)?
                .shared_style
                .unwrap_or_default()
        };
        let edit = tool == "propose_block_edit";
        let summary = args["summary"]
            .as_str()
            .filter(|s| s.len() <= 4000)
            .ok_or("invalid summary")?
            .to_owned();
        let assumptions: Vec<String> = if args["assumptions"].is_null() {
            vec![]
        } else {
            serde_json::from_value(args["assumptions"].clone())
                .map_err(|_| "invalid assumptions")?
        };
        if assumptions.len() > 20 || assumptions.iter().any(|s| s.len() > 2000) {
            return Err("oversized assumptions".into());
        }
        let inputs = if edit {
            vec![json!({"source":args["source"],"placement":"below_selection"})]
        } else {
            args["items"]
                .as_array()
                .filter(|v| !v.is_empty() && v.len() <= 20)
                .ok_or("invalid items")?
                .clone()
        };
        let target = if edit {
            let id = args["blockId"].as_str().ok_or("missing blockId")?;
            if !selected.contains(id) {
                return Err("not shared: target block".into());
            }
            Some(
                b.page
                    .objects
                    .iter()
                    .find(|o| o.fields().id == id && matches!(o, PageObject::Typst { .. }))
                    .ok_or("target is not a block")?
                    .clone(),
            )
        } else {
            None
        };
        let id = random_hex(16)?;
        let remote = self
            .app
            .as_ref()
            .is_some_and(|app| app.state::<crate::settings::RemotePackages>().allowed());
        let root_string = share.root.clone();
        let p = tauri::async_runtime::spawn_blocking(move || {
            let _permits = (own, global);
            let mut items = Vec::new();
            let z = b.page.objects.iter().map(|o| o.fields().z_index).max().unwrap_or(0);
            let order = b.page.reading_order.len() as u32;
            for (i, input) in inputs.iter().enumerate() {
                let source = input["source"].as_str().filter(|s| s.len() <= 65536).ok_or("invalid source")?.to_owned();
                let width = target.as_ref().and_then(|o| if let PageObject::Typst {layout_width_pt,..}=o {Some(*layout_width_pt)}else{None}).unwrap_or(input["widthPt"].as_f64().unwrap_or(share.crop_pt.width.clamp(120.0_f64.min(b.page.geometry.width_pt),b.page.geometry.width_pt.min(230.0))));
                if !width.is_finite() || width <= 0.0 || width > b.page.geometry.width_pt { return Err("invalid widthPt".into()); }
                let (svg, w, h) = compile(&root, &source, width, &files, &style, remote)?;
                let (object, before_source, before_svg) = if let Some(target) = &target {
                    let path = target.searchable_source().ok_or("missing source")?;
                    let before = String::from_utf8(b.blocks.iter().find(|f| f.path == path).ok_or("missing source")?.bytes.clone()).map_err(|e| e.to_string())?;
                    let preview = compile(&root, &before, width, &files, &style, remote).ok().map(|v|v.0);
                    (target.clone(), Some(before), preview)
                } else {
                    let object_id = format!("helper-{id}-{i}");
                    let placement = &input["placement"];
                    if placement != "beside_selection" && placement != "below_selection" && !(placement["x"].is_number() && placement["y"].is_number()) { return Err("invalid placement".into()); }
                    let below_x=share.crop_pt.x.clamp(12.0,(b.page.geometry.width_pt-132.0).max(12.0));
                    let below_y=if share.crop_pt.y+share.crop_pt.height+6.0 < b.page.geometry.height_pt-24.0 {share.crop_pt.y+share.crop_pt.height+6.0}else{(share.crop_pt.y-30.0).max(12.0)};
                    let beside=placement=="beside_selection" && share.crop_pt.x+share.crop_pt.width+6.0+w<=b.page.geometry.width_pt;
                    let x=placement["x"].as_f64().unwrap_or(if beside{share.crop_pt.x+share.crop_pt.width+6.0}else{below_x});
                    let y=placement["y"].as_f64().unwrap_or(if beside{share.crop_pt.y}else{below_y}+i as f64*(h+6.0));
                    if w > b.page.geometry.width_pt || h > b.page.geometry.height_pt { return Err("proposal does not fit page".into()); }
                    let object = serde_json::from_value(json!({"type":"typst","id":object_id,"x":x.clamp(0.0,b.page.geometry.width_pt-w),"y":y.clamp(0.0,b.page.geometry.height_pt-h),"rotation":0,"scale":1,"zIndex":z+1+i as i32,"readingOrder":order+i as u32,"groupId":null,"createdAt":now().to_string(),"modifiedAt":now().to_string(),"sourcePath":format!("blocks/{object_id}.typ"),"layoutWidthPt":width,"measuredWidthPt":w,"measuredHeightPt":h})).map_err(|e|format!("{e}"))?;
                    (object, None, None)
                };
                items.push(ProposalItem { object, hash: format!("{:x}",Sha256::digest(source.as_bytes())), source, svg, before_source, before_svg, width_pt:w, height_pt:h, placement:input["placement"].clone() });
            }
            Ok::<_,String>(Proposal { new_page:None, cell:None, preset:None, overflow:false, page_number:0, kind:if edit {"block_edit"} else {"blocks"}.into(), managed_header:None, page_snapshot:None, dependencies:vec![], baseline_grid:false, id, session_id:String::new(), root:root_string, page_id:share.page_id, version:1, status:"pending".into(), text:None, changed:vec![], summary, assumptions, edit, items, base, files, style, geometry:b.page.geometry.clone(), anchor:share.crop_pt })
        }).await.map_err(|e|e.to_string())??;
        #[cfg(test)]
        self.helper.pause_before_return().await;
        let mut p = p;
        p.session_id = self.id.clone();
        let mut s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !s.grant_current(&self.id, generation, request.as_deref()) {
            return Err("changed: grant changed".into());
        }
        let session = s.sessions.get_mut(&self.id).ok_or("session ended")?;
        if pending_count(session) >= 20 {
            return Err("limit: 20 pending proposals".into());
        }
        session.proposals.insert(p.id.clone(), p.clone());
        emit(self.app.as_ref(), &p);
        drop(s);
        Ok(json!({"id":p.id,"status":p.status,"version":p.version}))
    }
}

pub fn apply(p: &Proposal, snapshot: &mut NotebookSnapshot) -> Result<(), String> {
    if p.kind == "page_text_edit" {
        // Only the PageTypst source changes: same object, transform, geometry and header.
        let captured = p.page_snapshot.as_ref().ok_or("missing captured page")?;
        let item = p.items.first().ok_or("missing Page text")?;
        if snapshot.page.id != p.page_id || page_guidance(captured) != page_guidance(snapshot) {
            return Err("page geometry or writing guidance changed".into());
        }
        let target = snapshot
            .page
            .objects
            .iter()
            .find(|o| o.fields().id == item.object.fields().id)
            .ok_or("missing target")?;
        if !matches!(target, PageObject::PageTypst { .. })
            || serde_json::to_value(target).ok() != serde_json::to_value(&item.object).ok()
        {
            return Err("Page text target changed".into());
        }
        let source = page_text_source(p.managed_header.as_deref(), &item.source)?;
        if format!("{:x}", Sha256::digest(source.as_bytes())) != item.hash {
            return Err("reviewed source changed".into());
        }
        let path = target
            .searchable_source()
            .ok_or("missing source")?
            .to_owned();
        let file = snapshot
            .blocks
            .iter_mut()
            .find(|f| f.path == path)
            .ok_or("missing source")?;
        file.bytes = source.into_bytes();
        return Ok(());
    }
    if snapshot.page.id != p.page_id {
        return Err("wrong destination page".into());
    }
    for item in &p.items {
        let path = item
            .object
            .searchable_source()
            .ok_or("missing source path")?
            .to_owned();
        if p.edit {
            let object = snapshot
                .page
                .objects
                .iter_mut()
                .find(|o| o.fields().id == item.object.fields().id)
                .ok_or("missing target")?;
            *object = item.object.clone();
            let file = snapshot
                .blocks
                .iter_mut()
                .find(|f| f.path == path)
                .ok_or("missing target source")?;
            file.bytes = item.source.as_bytes().to_vec();
        } else {
            if snapshot
                .page
                .objects
                .iter()
                .any(|o| o.fields().id == item.object.fields().id)
            {
                return Err("duplicate proposal object".into());
            }
            snapshot
                .page
                .reading_order
                .push(item.object.fields().id.clone());
            snapshot.page.objects.push(item.object.clone());
            snapshot.blocks.push(storage::StoredFile {
                path,
                bytes: item.source.as_bytes().to_vec(),
            });
        }
    }
    Ok(())
}
pub fn find_mut<'a>(s: &'a mut Inner, id: &str) -> Result<&'a mut Proposal, String> {
    s.sessions
        .values_mut()
        .find_map(|session| session.proposals.get_mut(id))
        .ok_or("proposal session ended".into())
}
#[tauri::command]
pub fn helper_proposals(helper: tauri::State<'_, Helper>) -> Vec<Proposal> {
    helper
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .sessions
        .values()
        .flat_map(|s| s.proposals.values().cloned())
        .collect()
}
fn change(
    helper: &Helper,
    app: &tauri::AppHandle,
    id: &str,
    status: &str,
    text: Option<String>,
) -> Result<(), String> {
    let mut s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
    let p = find_mut(&mut s, id)?;
    if !["pending", "stale", "change_requested"].contains(&p.status.as_str()) {
        return Err("proposal is final".into());
    }
    p.status = status.into();
    p.text = text;
    emit(Some(app), p);
    Ok(())
}
#[tauri::command]
pub fn helper_dismiss_proposal(
    app: tauri::AppHandle,
    helper: tauri::State<'_, Helper>,
    id: String,
) -> Result<(), String> {
    change(&helper, &app, &id, "dismissed", None)
}
#[tauri::command]
pub fn helper_request_change(
    app: tauri::AppHandle,
    helper: tauri::State<'_, Helper>,
    id: String,
    text: String,
) -> Result<(), String> {
    if text.chars().count() > 2000 {
        return Err("change request exceeds 2000 characters".into());
    }
    change(&helper, &app, &id, "change_requested", Some(text))
}
#[tauri::command]
pub async fn helper_edit_proposal(
    app: tauri::AppHandle,
    helper: tauri::State<'_, Helper>,
    id: String,
    proposal_version: u64,
    sources: Vec<String>,
) -> Result<Proposal, String> {
    let remote = app.state::<crate::settings::RemotePackages>().allowed();
    edit(&helper, Some(&app), &id, proposal_version, sources, remote).await
}

/// Recompile reviewed sources and advance the version. A failed compile leaves the stored
/// proposal, its version and its last valid preview untouched.
pub async fn edit(
    helper: &Helper,
    app: Option<&tauri::AppHandle>,
    id: &str,
    proposal_version: u64,
    sources: Vec<String>,
    remote: bool,
) -> Result<Proposal, String> {
    let mut p = {
        let mut s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
        find_mut(&mut s, id)?.clone()
    };
    if p.new_page.is_some() {
        return super::new_page::edit(helper, app, p, proposal_version, sources).await;
    }
    if let Some(cell) = &p.cell {
        if p.status != "pending"
            || p.version != proposal_version
            || sources.len() != 1
            || sources[0].len() > 65536
        {
            return Err("proposal changed".into());
        }
        let mut state = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !state.grant_current(&p.session_id, 0, Some(&cell.grant)) {
            return Err("changed: grant revoked".into());
        }
        let stored = find_mut(&mut state, id)?;
        if stored.version != proposal_version || stored.status != "pending" {
            return Err("proposal changed".into());
        }
        stored.cell.as_mut().unwrap().new_source = sources[0].clone();
        stored.version += 1;
        emit(app, stored);
        return Ok(stored.clone());
    }
    if p.kind == "preset_install" {
        return super::preset_install::edit(helper, app, p, proposal_version, sources).await;
    }
    if p.status != "pending"
        || p.version != proposal_version
        || sources.len() != p.items.len()
        || sources.iter().any(|s| s.len() > 65536)
    {
        return Err("proposal changed".into());
    }
    let own = {
        let s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.sessions
            .get(&p.session_id)
            .ok_or("session ended")?
            .compile
            .clone()
            .try_acquire_owned()
            .map_err(|_| "busy")?
    };
    let global = helper
        .compile
        .clone()
        .try_acquire_owned()
        .map_err(|_| "busy")?;
    p = tauri::async_runtime::spawn_blocking(move || {
        let _permits = (own, global);
        for (item, source) in p.items.iter_mut().zip(sources) {
            if p.kind == "page_text_edit" {
                // The reviewer edits only the body; an error keeps the last valid version.
                let full = page_text_source(p.managed_header.as_deref(), &source)?;
                let (svg, w, h, overflow) = compile_page(
                    p.page_snapshot.as_ref().ok_or("missing page")?,
                    &p.root,
                    &full,
                    &p.files,
                    p.baseline_grid,
                )?;
                p.overflow = overflow;
                item.hash = format!("{:x}", Sha256::digest(full.as_bytes()));
                item.source = source;
                item.svg = svg;
                item.width_pt = w;
                item.height_pt = h;
                continue;
            }
            let width = if let PageObject::Typst {
                layout_width_pt, ..
            } = &item.object
            {
                *layout_width_pt
            } else {
                return Err("invalid block".into());
            };
            let (svg, w, h) = compile(
                std::path::Path::new(&p.root),
                &source,
                width,
                &p.files,
                &p.style,
                remote,
            )?;
            if !p.edit
                && (item.object.fields().x + w > p.geometry.width_pt
                    || item.object.fields().y + h > p.geometry.height_pt)
            {
                return Err("edited proposal no longer fits page".into());
            }
            if let PageObject::Typst {
                measured_width_pt,
                measured_height_pt,
                ..
            } = &mut item.object
            {
                *measured_width_pt = w;
                *measured_height_pt = h;
            }
            item.hash = format!("{:x}", Sha256::digest(source.as_bytes()));
            item.source = source;
            item.svg = svg;
            item.width_pt = w;
            item.height_pt = h;
        }
        p.version += 1;
        Ok::<_, String>(p)
    })
    .await
    .map_err(|e| e.to_string())??;
    let mut s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
    let current = find_mut(&mut s, id)?;
    if current.version != proposal_version || current.status != "pending" {
        return Err("proposal changed".into());
    }
    *current = p.clone();
    emit(app, &p);
    Ok(p)
}

// Notebook lock is acquired before the helper lock. Keep the session/proposal stable through
// persistence so revoke, edited-source compilation and a second insert cannot race acceptance.
pub fn insert(
    helper: &Helper,
    app: Option<&tauri::AppHandle>,
    root: &std::path::Path,
    page_id: &str,
    history: &mut storage::NotebookHistory,
    insert: ProposalInsert,
) -> Result<storage::HistoryResult, storage::StorageError> {
    let mut state = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
    if state.sessions.values().any(|session| {
        session.proposals.contains_key(&insert.proposal_id)
            && (session
                .launch_cancelled
                .as_ref()
                .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
                || session.conversation.is_some() && session.request.is_none())
    }) {
        return Err(storage::StorageError::InvalidNotebook(
            "proposal authorization revoked".into(),
        ));
    }
    let p = find_mut(&mut state, &insert.proposal_id)
        .map_err(storage::StorageError::InvalidNotebook)?;
    if p.cell.is_some()
        || p.kind == "preset_install"
        || p.version != insert.proposal_version
        || p.status != "pending"
        || p.page_id != page_id
        || std::path::Path::new(&p.root).canonicalize()? != root
    {
        return Err(storage::StorageError::InvalidNotebook(
            "proposal changed, wrong destination or already inserted".into(),
        ));
    }
    let current = storage::open_page(root, page_id)?;
    if let Err(error) = storage::check_proposal_base(root, &current, &p.base) {
        if let storage::StorageError::ProposalStale(changed) = &error {
            p.status = "stale".into();
            p.changed = changed.clone();
            emit(app, p);
        }
        return Err(error);
    }
    if p.kind == "page_text_edit" {
        let changed = page_text_changes(p, root, &current);
        if !changed.is_empty() {
            p.status = "stale".into();
            p.changed = changed.clone();
            emit(app, p);
            return Err(storage::StorageError::ProposalStale(changed));
        }
        if p.overflow {
            return Err(storage::StorageError::InvalidNotebook(
                "Page text exceeds the page".into(),
            ));
        }
    }
    let mut candidate = current;
    apply(p, &mut candidate).map_err(storage::StorageError::InvalidNotebook)?;
    let result = storage::commit_notebook_with_precondition(
        root,
        history,
        candidate,
        Some(storage::ProposalPrecondition {
            base: p.base.clone(),
            require_retained_history: true,
        }),
    )?;
    p.status = "inserted".into();
    emit(app, p);
    Ok(result)
}

/// Rebuild the reviewed Page text once. A managed preset header stays byte-identical and
/// read-only; the assistant and the reviewer edit only the body. An unmanaged source is reviewed
/// whole and cannot gain a managed header through this path.
pub(super) fn page_text_source(header: Option<&str>, body: &str) -> Result<String, String> {
    if body.len() > 65536 {
        return Err("invalid source: Page text exceeds 64 KiB".into());
    }
    if storage::managed_preset_header(body).is_some() {
        return Err(if header.is_some() {
            "managed header is read-only: supply only the editable body"
        } else {
            "managed header cannot be added through a Page text edit"
        }
        .into());
    }
    let Some(header) = header else {
        return Ok(body.into());
    };
    let separator = if header.ends_with('\n') || body.is_empty() {
        ""
    } else {
        "\n"
    };
    let full = format!("{header}{separator}{body}");
    match storage::managed_preset_header(&full) {
        Some((_, n)) if full.starts_with(header) && n == header.len() + separator.len() => Ok(full),
        _ => Err("managed header changed".into()),
    }
}

/// The page writing guidance a Page text preview depends on: geometry and paper. Captured with
/// the proposal; a change makes Apply stale.
fn page_guidance(snapshot: &NotebookSnapshot) -> Value {
    json!({"geometry":snapshot.page.geometry,"background":snapshot.page.background})
}

/// Files the Page text compile can read besides its own source: preset, shared style and literal
/// imports. Sources owned by page objects are excluded, so editing another object is unrelated.
fn page_text_dependencies(snapshot: &NotebookSnapshot, files: &GrantedFiles) -> Vec<String> {
    let owned: std::collections::HashSet<String> = snapshot
        .page
        .objects
        .iter()
        .filter_map(|o| o.source().map(|s| format!("/{}", s.path)))
        .collect();
    let mut paths: Vec<String> = files
        .keys()
        .filter(|path| !owned.contains(*path))
        .cloned()
        .collect();
    paths.sort();
    paths
}

/// Changes since capture that the fingerprints in ProposalBase do not cover.
fn page_text_changes(
    p: &Proposal,
    root: &std::path::Path,
    current: &NotebookSnapshot,
) -> Vec<storage::ChangedDependency> {
    use storage::{ChangedDependency, DependencyChange};
    let mut changed = Vec::new();
    if let Some(captured) = &p.page_snapshot
        && page_guidance(captured) != page_guidance(current)
    {
        changed.push(ChangedDependency::Page {
            change: DependencyChange::Modified,
        });
    }
    for path in &p.dependencies {
        let Some(bytes) = p.files.get(path) else {
            continue;
        };
        let now = goodtype_core::paths::resolve_file(root, path.trim_start_matches('/'))
            .ok()
            .and_then(|file| std::fs::read(file).ok());
        match now {
            Some(now) if &now == bytes => {}
            now => changed.push(ChangedDependency::File {
                id: path.clone(),
                change: if now.is_none() {
                    DependencyChange::Removed
                } else {
                    DependencyChange::Modified
                },
            }),
        }
    }
    changed
}

fn svg_color(color: &str) -> &str {
    if goodtype_core::valid_hex_color(color) {
        color
    } else {
        "#ffffff"
    }
}

/// Compile the full reviewed source through the same granted Page text path as preview/export and
/// place it on a page-sized sheet with the page's paper and writing guidance. Diagnostics are
/// mapped to the editable body; header diagnostics name the preset. Errors yield no candidate.
pub(super) fn compile_page(
    snapshot: &NotebookSnapshot,
    root: &str,
    source: &str,
    files: &GrantedFiles,
    baseline_grid: bool,
) -> Result<(String, f64, f64, bool), String> {
    let mut page = super::snapshot::export(snapshot)?;
    page.page_text_baseline_grid = baseline_grid;
    page.page_typst = Some(source.into());
    let (x, y, width) = goodtype_typst::export::page_text_preview_geometry(&page);
    let mut result = goodtype_typst::compile_page_text_granted(
        std::path::Path::new(root),
        &page,
        &CompileRequest {
            source: source.into(),
            width_pt: width,
            generation: 0,
            allow_remote_packages: false,
        },
        files,
    )?;
    if let Some((preset, bytes)) = storage::managed_preset_header(source) {
        let lines = source[..bytes].bytes().filter(|b| *b == b'\n').count();
        for d in &mut result.diagnostics {
            if d.file.is_some() {
                continue;
            }
            match d.line {
                Some(line) if line > lines => {
                    d.line = Some(line - lines);
                    d.end_line = d.end_line.map(|end| end.saturating_sub(lines).max(1));
                }
                Some(_) => d.file = Some(format!("/styles/{preset}")),
                None => {}
            }
        }
    }
    let svg = result
        .svg
        .ok_or_else(|| serde_json::to_string(&result.diagnostics).unwrap_or_default())?;
    let overflow = y + result.height_pt.unwrap_or(0.) > page.height_pt;
    let paper = match &snapshot.page.background {
        goodtype_core::PageBackground::Plain { color } => svg_color(color),
        goodtype_core::PageBackground::Template { template } => {
            svg_color(&template.background_color)
        }
        goodtype_core::PageBackground::Pdf { .. } => "#ffffff",
    };
    let mut guidance = String::new();
    if let goodtype_core::PageBackground::Template { template } = &snapshot.page.background {
        for shape in goodtype_core::template::resolve(template, &snapshot.page.geometry) {
            match shape {
                goodtype_core::template::TemplateShape::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                    weight_pt,
                } => guidance.push_str(&format!(
                    r#"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{}" stroke-width="{weight_pt}"/>"#,
                    svg_color(color)
                )),
                goodtype_core::template::TemplateShape::Dot {
                    cx,
                    cy,
                    radius_pt,
                    color,
                } => guidance.push_str(&format!(
                    r#"<circle cx="{cx}" cy="{cy}" r="{radius_pt}" fill="{}"/>"#,
                    svg_color(color)
                )),
            }
        }
    }
    let (w, h, pad) = (page.width_pt, page.height_pt, result.pad_pt);
    let full_svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}pt" height="{h}pt" viewBox="0 0 {w} {h}"><rect width="{w}" height="{h}" fill="{paper}"/>{guidance}<g transform="translate({},{})">{svg}</g></svg>"#,
        x - pad,
        y - pad
    );
    Ok((full_svg, w, h, overflow))
}

impl bridge::Bridge {
    /// Everything the proposal depends on is captured from the request grant before any await:
    /// root, destination, PageTypst target and source path, managed header, base fingerprints,
    /// page guidance, granted dependency bytes and the grant epoch.
    async fn propose_page_text(
        &self,
        args: Value,
        stamp: (u64, Option<Arc<CapturedRequestGrant>>),
    ) -> Result<Value, String> {
        let request = stamp
            .1
            .clone()
            .ok_or("not shared: attach the destination page to edit its Page text")?;
        let page_id = request.destination.page.id.clone();
        if args["page"].as_str() != Some(page_id.as_str()) {
            return Err("not shared: Page text edits target the request's destination page".into());
        }
        let captured = request
            .pages
            .get(&page_id)
            .ok_or("not shared: attach the destination page to edit its Page text")?;
        let snapshot = captured.snapshot.clone();
        let mut base = base_from_wire(&args["proposalBase"])?;
        if base_to_wire(&base) != base_to_wire(&captured.base) {
            return Err("changed: proposalBase differs from the attached page's sourceBase".into());
        }
        // Page text depends on its own fingerprint, presets and shared style, not on unrelated
        // movable objects.
        base.objects.clear();
        let target_id = args["pageTextId"].as_str().ok_or("missing pageTextId")?;
        let target = snapshot
            .page
            .objects
            .iter()
            .find(|o| matches!(o, PageObject::PageTypst { .. }) && o.fields().id == target_id)
            .ok_or("missing existing Page text: use propose_block_edit for movable blocks")?
            .clone();
        let path = target
            .searchable_source()
            .ok_or("missing Page text source")?;
        let before = String::from_utf8(
            snapshot
                .blocks
                .iter()
                .find(|f| f.path == path)
                .ok_or("missing Page text source")?
                .bytes
                .clone(),
        )
        .map_err(|e| e.to_string())?;
        let header = storage::managed_preset_header(&before).map(|(_, n)| before[..n].to_owned());
        let source = args["source"].as_str().ok_or("invalid source")?.to_owned();
        let full = page_text_source(header.as_deref(), &source)?;
        let summary = args["summary"]
            .as_str()
            .filter(|s| s.len() <= 4000)
            .ok_or("invalid summary")?
            .to_owned();
        let assumptions: Vec<String> = if args["assumptions"].is_null() {
            vec![]
        } else {
            serde_json::from_value(args["assumptions"].clone())
                .map_err(|_| "invalid assumptions")?
        };
        if assumptions.len() > 20 || assumptions.iter().any(|s| s.len() > 2000) {
            return Err("oversized assumptions".into());
        }
        let root = request.root.clone();
        crate::workspace::ensure_allowed(&self.roots, &root)?;
        let files = captured.files.clone();
        let dependencies = page_text_dependencies(&snapshot, &files);
        // The same preference ordinary Page text preview receives; the default when unavailable.
        let baseline_grid = self.app.as_ref().is_none_or(|app| {
            crate::settings::load_app_settings(app.clone())
                .map_or(true, |s| s.page_text_baseline_grid)
        });
        let page_number = snapshot
            .manifest
            .pages
            .iter()
            .position(|p| p.id == snapshot.page.id)
            .ok_or("not shared: destination page")?
            + 1;
        let (own, global) = {
            let s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            if !s.grant_current(&self.id, stamp.0, stamp.1.as_deref()) {
                return Err("changed: request changed before proposal dispatch".into());
            }
            let session = s.sessions.get(&self.id).ok_or("session ended")?;
            if pending_count(session) >= 20 {
                return Err("limit: 20 pending proposals; review or dismiss first".into());
            }
            (
                session
                    .compile
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| "busy")?,
                self.helper
                    .compile
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| "busy")?,
            )
        };
        let id = random_hex(16)?;
        let session_id = self.id.clone();
        let anchor = request.anchor.crop_pt;
        let p = tauri::async_runtime::spawn_blocking(move || {
            let _permits = (own, global);
            let (svg, w, h, overflow) =
                compile_page(&snapshot, &root, &full, &files, baseline_grid)?;
            let before_svg = compile_page(&snapshot, &root, &before, &files, baseline_grid)
                .ok()
                .map(|v| v.0);
            let before_source = Some(before[header.as_ref().map_or(0, String::len)..].to_owned());
            Ok::<_, String>(Proposal {
                new_page: None,
                cell: None,
                preset: None,
                overflow,
                page_number,
                id,
                session_id,
                root,
                page_id: snapshot.page.id.clone(),
                version: 1,
                status: "pending".into(),
                text: None,
                changed: vec![],
                summary,
                assumptions,
                edit: true,
                kind: "page_text_edit".into(),
                managed_header: header,
                geometry: snapshot.page.geometry.clone(),
                page_snapshot: Some(snapshot),
                dependencies,
                baseline_grid,
                anchor,
                base,
                files,
                style: String::new(),
                items: vec![ProposalItem {
                    object: target,
                    source,
                    hash: format!("{:x}", Sha256::digest(full.as_bytes())),
                    svg,
                    before_source,
                    before_svg,
                    width_pt: w,
                    height_pt: h,
                    placement: Value::Null,
                }],
            })
        })
        .await
        .map_err(|e| e.to_string())??;
        #[cfg(test)]
        self.helper.pause_before_return().await;
        let mut s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !s.grant_current(&self.id, stamp.0, stamp.1.as_deref()) {
            return Err("changed: grant changed".into());
        }
        let session = s.sessions.get_mut(&self.id).ok_or("session ended")?;
        if pending_count(session) >= 20 {
            return Err("limit: 20 pending proposals".into());
        }
        session.proposals.insert(p.id.clone(), p.clone());
        emit(self.app.as_ref(), &p);
        drop(s);
        Ok(
            json!({"id":p.id,"status":p.status,"version":p.version,"kind":p.kind,"overflow":p.overflow}),
        )
    }
}
