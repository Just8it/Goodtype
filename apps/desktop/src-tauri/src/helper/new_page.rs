//! Review-only page creation; no assistant tool can publish or execute a page.
use super::*;
use goodtype_core::storage::{self, NotebookSnapshot, PagePosition};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{io::Read, path::Path};

#[cfg(test)]
mod tests {
    use super::*;
    const TIME: &str = "2026-11-06T10:00:00Z";
    struct Fixture {
        _temp: tempfile::TempDir,
        root: std::path::PathBuf,
        anchor: String,
        bridge: bridge::Bridge,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("book");
            let from =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/notebooks/minimal");
            let initial = storage::open_notebook(&from).unwrap();
            std::fs::create_dir_all(root.join("references")).unwrap();
            std::fs::copy(
                from.join("references/lecture.pdf"),
                root.join("references/lecture.pdf"),
            )
            .unwrap();
            storage::create_notebook(&root, &initial).unwrap();
            let root = root.canonicalize().unwrap();
            let roots = Arc::new(crate::workspace::AllowedRoots::default());
            crate::workspace::allow_root(&roots, &root).unwrap();
            let helper = Helper::default();
            let mut session = bridge::new_session("page-test".into());
            session.info.approved = true;
            session.conversation = Some("conversation".into());
            helper
                .inner
                .lock()
                .unwrap()
                .sessions
                .insert("page-test".into(), session);
            let f = Self {
                _temp: temp,
                root,
                anchor: initial.page.id,
                bridge: bridge::Bridge {
                    helper,
                    app: None,
                    id: "page-test".into(),
                    roots,
                    histories: Default::default(),
                },
            };
            f.install(false);
            f
        }
        fn install(&self, names: bool) {
            self.install_with(names, None);
        }
        fn install_with(&self, names: bool, selection: Option<SharePayload>) {
            let grant = snapshot::capture_locked(
                &self.bridge.roots,
                &self.bridge.histories,
                RequestContextIntent {
                    share_page_names: names,
                    cells: None,
                    destination: RequestDestination {
                        root: self.root.to_string_lossy().into(),
                        page_id: self.anchor.clone(),
                    },
                    selection,
                    page_ids: vec![self.anchor.clone()],
                    presets: false,
                    intent_version: "page-test".into(),
                },
                Usage::Detailed,
            )
            .unwrap();
            let mut grant = grant;
            grant.conversation_id = "conversation".into();
            self.bridge
                .helper
                .inner
                .lock()
                .unwrap()
                .sessions
                .get_mut(&self.bridge.id)
                .unwrap()
                .install_request(grant);
        }
        async fn propose(&self, kind: &str, target: Option<&str>) -> proposals::Proposal {
            let mut args = json!({"pageType":kind,"title":"Solution","position":{"kind":"after","pageId":self.anchor},"summary":"Solve the task"});
            if kind == "python" {
                args["cells"] = json!([{"cellType":"markdown","source":"## Result"},{"cellType":"code","source":"answer = 6 * 7\nprint(answer)"}]);
            }
            if kind == "page_text" {
                args["source"] = json!("= Solution\n$ F = m a $");
            }
            if let Some(target) = target {
                args["targetPageId"] = json!(target);
            }
            let result = self.bridge.propose("propose_page", args).await.unwrap();
            self.bridge.helper.inner.lock().unwrap().sessions[&self.bridge.id].proposals
                [result["id"].as_str().unwrap()]
            .clone()
        }
        fn apply(
            &self,
            p: &proposals::Proposal,
            position: PagePosition,
        ) -> Result<ApplyResult, crate::notebook::NotebookError> {
            crate::notebook::with_notebook_lock(&self.bridge.histories, |store| {
                apply(
                    &self.bridge.helper,
                    None,
                    &self.root,
                    store,
                    ApplyInput {
                        proposal_id: p.id.clone(),
                        proposal_version: p.version,
                        position,
                        modified_at: TIME.into(),
                    },
                )
            })
        }
    }
    #[tokio::test]
    async fn new_pages_publish_initial_content_once_and_undo_redo_as_one_action() {
        for kind in ["blank", "page_text", "python"] {
            let f = Fixture::new();
            let before = storage::open_notebook(&f.root).unwrap();
            let p = f.propose(kind, None).await;
            assert_eq!(
                storage::open_notebook(&f.root)
                    .unwrap()
                    .manifest
                    .pages
                    .len(),
                before.manifest.pages.len()
            );
            let result = f
                .apply(
                    &p,
                    PagePosition::After {
                        page_id: f.anchor.clone(),
                    },
                )
                .unwrap();
            let id = result.structure.snapshot.page.id.clone();
            assert!(result.structure.can_undo);
            assert_eq!(result.structure.snapshot.manifest.pages[1].id, id);
            if kind == "python" {
                let b = &result.structure.snapshot;
                let doc = goodtype_core::calculation::Calculation::parse(&b.calculations[0].bytes)
                    .unwrap();
                assert_eq!(doc.cells.len(), 2);
                assert!(
                    doc.cells
                        .iter()
                        .all(|c| c.execution_count.is_none() && c.outputs.is_empty())
                );
            } else if kind == "page_text" {
                assert_eq!(
                    result
                        .structure
                        .snapshot
                        .blocks
                        .iter()
                        .find(|b| {
                            result
                                .structure
                                .snapshot
                                .page
                                .objects
                                .iter()
                                .any(|o| o.searchable_source() == Some(b.path.as_str()))
                        })
                        .unwrap()
                        .bytes,
                    b"= Solution\n$ F = m a $"
                );
            }
            assert!(f.apply(&p, PagePosition::Last).is_err());
            let undone = crate::notebook::with_notebook_lock(&f.bridge.histories, |store| {
                crate::notebook::restore_structure_result(store, &f.root, TIME, true)
            })
            .unwrap();
            assert!(!undone.snapshot.manifest.pages.iter().any(|p| p.id == id));
            let redone = crate::notebook::with_notebook_lock(&f.bridge.histories, |store| {
                crate::notebook::restore_structure_result(store, &f.root, TIME, false)
            })
            .unwrap();
            assert_eq!(redone.snapshot.page.id, id);
            assert_eq!(
                storage::open_page(&f.root, &id).unwrap().blocks.len(),
                result.structure.snapshot.blocks.len()
            );
        }
    }
    #[tokio::test]
    async fn selection_on_an_attached_page_does_not_make_the_page_proposal_stale() {
        let f = Fixture::new();
        let share = |object_ids: Vec<String>| SharePayload {
            root: f.root.to_string_lossy().into(),
            page_id: f.anchor.clone(),
            object_ids,
            stroke_ids: vec![],
            crop_pt: Rect {
                x: 0.,
                y: 0.,
                width: 20.,
                height: 20.,
            },
            session_ids: None,
        };
        let first = storage::open_page(&f.root, &f.anchor).unwrap().page.objects[0]
            .fields()
            .id
            .clone();
        for selection in [share(vec![]), share(vec![first])] {
            f.install_with(false, Some(selection));
            let p = f.propose("blank", None).await;
            f.apply(&p, PagePosition::Last).unwrap();
        }
    }
    #[tokio::test]
    async fn user_can_change_placement_but_stale_deleted_or_revoked_proposals_cannot_write() {
        let f = Fixture::new();
        let p = f.propose("python", None).await;
        assert!(
            f.apply(
                &p,
                PagePosition::After {
                    page_id: "foreign".into()
                }
            )
            .is_err()
        );
        let before = storage::open_notebook(&f.root)
            .unwrap()
            .manifest
            .pages
            .len();
        let result = f
            .apply(
                &p,
                PagePosition::Before {
                    page_id: f.anchor.clone(),
                },
            )
            .unwrap();
        assert_eq!(
            result.structure.snapshot.manifest.pages[0].id,
            result.structure.snapshot.page.id
        );
        assert_eq!(result.structure.snapshot.manifest.pages.len(), before + 1);
        let f = Fixture::new();
        let p = f.propose("blank", None).await;
        storage::create_page(&f.root, TIME, &PagePosition::Last, None, None).unwrap();
        let count = storage::open_notebook(&f.root)
            .unwrap()
            .manifest
            .pages
            .len();
        assert!(f.apply(&p, PagePosition::Last).is_err());
        assert_eq!(
            storage::open_notebook(&f.root)
                .unwrap()
                .manifest
                .pages
                .len(),
            count
        );
        let f = Fixture::new();
        let p = f.propose("blank", None).await;
        f.bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .sessions
            .remove(&f.bridge.id);
        assert!(f.apply(&p, PagePosition::Last).is_err());
        assert_eq!(
            storage::open_notebook(&f.root)
                .unwrap()
                .manifest
                .pages
                .len(),
            1
        );
    }
    #[tokio::test]
    async fn python_reference_request_can_append_without_reading_or_replacing_existing_code() {
        let f = Fixture::new();
        let target = storage::create_calculation_page(
            &f.root,
            TIME,
            &PagePosition::Last,
            "Python results",
            None,
        )
        .unwrap();
        f.install(true);
        let original =
            goodtype_core::calculation::Calculation::parse(&target.calculations[0].bytes).unwrap();
        let p = f.propose("python", Some(&target.page.id)).await;
        assert_eq!(p.kind, "cells_append");
        let encoded = serde_json::to_value(&p).unwrap();
        assert!(encoded["newPage"].get("appendBase").is_none());
        assert!(encoded["newPage"].get("captured").is_none());
        let result = f.apply(&p, PagePosition::Last).unwrap();
        let doc = &result.calculation.as_ref().unwrap().document;
        assert_eq!(doc.cells.len(), original.cells.len() + 2);
        assert_eq!(doc.cells[0], original.cells[0]);
        assert!(
            doc.cells
                .iter()
                .all(|c| c.execution_count.is_none() && c.outputs.is_empty())
        );
        assert!(f.apply(&p, PagePosition::Last).is_err());
        assert_eq!(
            storage::open_notebook(&f.root)
                .unwrap()
                .manifest
                .pages
                .len(),
            2
        );
    }
    #[tokio::test]
    async fn failed_source_revision_keeps_last_valid_page_proposal() {
        let f = Fixture::new();
        let p = f.propose("page_text", None).await;
        assert!(
            edit(
                &f.bridge.helper,
                None,
                p.clone(),
                p.version,
                vec!["#let =".into()]
            )
            .await
            .is_err()
        );
        let stored = &f.bridge.helper.inner.lock().unwrap().sessions[&f.bridge.id].proposals[&p.id];
        assert_eq!(stored.version, p.version);
        assert_eq!(
            stored.new_page.as_ref().unwrap().draft.source,
            "= Solution\n$ F = m a $"
        );
    }
    #[tokio::test]
    async fn followup_keeps_reviewed_page_authority_but_revocation_still_refuses() {
        let f = Fixture::new();
        let p = f.propose("python", None).await;
        f.install(true);
        let edited = edit(
            &f.bridge.helper,
            None,
            p.clone(),
            p.version,
            vec!["## Reviewed answer".into(), "print(42)".into()],
        )
        .await
        .unwrap();
        assert!(f.apply(&edited, PagePosition::Last).is_ok());
        f.install(true);
        let pending = f.propose("python", None).await;
        f.bridge.helper.revoke_conversation("conversation");
        assert!(f.apply(&pending, PagePosition::Last).is_err());
    }
    #[tokio::test]
    async fn page_names_are_opt_in_metadata_and_do_not_grant_other_page_contents() {
        let f = Fixture::new();
        let mut pdf = storage::open_page(&f.root, &f.anchor).unwrap();
        pdf.page.background = goodtype_core::PageBackground::Pdf {
            source_path: "references/Aircraft.pdf".into(),
            page: 3,
        };
        assert_eq!(page_label(&pdf), ("Aircraft · 3".into(), "pdf"));
        let target = storage::create_calculation_page(
            &f.root,
            TIME,
            &PagePosition::Last,
            "Named results",
            None,
        )
        .unwrap();
        for enabled in [false, true] {
            f.install(enabled);
            let blocks = f
                .bridge
                .run(
                    "list_pages",
                    json!({}),
                    &mut Default::default(),
                    &mut String::new(),
                )
                .await
                .unwrap();
            let value = serde_json::to_value(blocks).unwrap();
            let text = value[0]["text"].as_str().unwrap();
            let value: Value = serde_json::from_str(text).unwrap();
            let target_meta = value["pages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["id"] == target.page.id)
                .unwrap();
            assert_eq!(target_meta.get("quotedTitle").is_some(), enabled);
            if enabled {
                assert_eq!(target_meta["quotedTitle"], "Named results");
                assert_eq!(target_meta["kind"], "python");
            }
            assert!(
                f.bridge
                    .run(
                        "read_page",
                        json!({"page":target.page.id}),
                        &mut Default::default(),
                        &mut String::new()
                    )
                    .await
                    .is_err()
            );
        }
    }
    #[tokio::test]
    async fn page_name_requests_follow_the_settings_permission() {
        let f = Fixture::new();
        let shared = |f: &Fixture| {
            f.bridge.helper.inner.lock().unwrap().sessions[&f.bridge.id]
                .request
                .as_ref()
                .unwrap()
                .share_page_names
        };
        let set = |f: &Fixture, p: Permission| {
            f.bridge
                .helper
                .inner
                .lock()
                .unwrap()
                .settings
                .permissions
                .page_names = p;
        };
        let ask = |f: &Fixture| {
            let bridge = f.bridge.clone();
            tokio::spawn(async move {
                bridge
                    .run(
                        "request_scope",
                        json!({"scope":{"kind":"page_names"},"reason":"Place the new page"}),
                        &mut Default::default(),
                        &mut String::new(),
                    )
                    .await
            })
        };
        // Never refuses without a card.
        f.install(false);
        set(&f, Permission::Never);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        f.bridge.helper.inner.lock().unwrap().scope_requests = Some(tx);
        assert!(ask(&f).await.unwrap().is_err());
        assert!(rx.try_recv().is_err());
        assert!(!shared(&f));
        // Allow grants without a card, for this request only.
        set(&f, Permission::Allow);
        assert!(ask(&f).await.unwrap().is_ok());
        assert!(rx.try_recv().is_err());
        assert!(shared(&f));
        // Ask shows a card; a denial leaves names unshared.
        f.install(false);
        set(&f, Permission::Ask);
        let task = ask(&f);
        let event = rx.recv().await.unwrap();
        assert_eq!(event["scope"], json!({"kind":"page_names"}));
        let (_, answer) = f
            .bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .scopes
            .remove(event["requestId"].as_str().unwrap())
            .unwrap();
        answer.send("deny".into()).unwrap();
        assert!(task.await.unwrap().is_err());
        assert!(!shared(&f));
    }
    #[tokio::test]
    async fn cells_requests_attach_a_python_page_without_moving_the_destination() {
        let f = Fixture::new();
        let target =
            storage::create_calculation_page(&f.root, TIME, &PagePosition::Last, "Solution", None)
                .unwrap();
        f.install(false);
        let number = f.bridge.helper.inner.lock().unwrap().sessions[&f.bridge.id]
            .request
            .as_ref()
            .unwrap()
            .destination
            .manifest
            .pages
            .iter()
            .position(|p| p.id == target.page.id)
            .unwrap()
            + 1;
        f.bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .settings
            .permissions
            .pages = Permission::Allow;
        let ask = |page: usize| {
            let bridge = f.bridge.clone();
            async move {
                bridge
                    .run(
                        "request_scope",
                        json!({"scope":{"kind":"cells","page":page},"reason":"Read the existing solution"}),
                        &mut Default::default(),
                        &mut String::new(),
                    )
                    .await
            }
        };
        // "cells" is a page request: an ordinary page is attached as content, a Python page as cells.
        let before = f.bridge.helper.inner.lock().unwrap().sessions[&f.bridge.id]
            .request
            .as_ref()
            .unwrap()
            .destination
            .page
            .id
            .clone();
        assert!(ask(1).await.is_ok());
        assert!(ask(number).await.is_ok());
        let s = f.bridge.helper.inner.lock().unwrap();
        let grant = s.sessions[&f.bridge.id].request.as_ref().unwrap();
        assert_eq!(grant.destination.page.id, before);
        assert!(
            grant
                .cells
                .get(&target.page.id)
                .is_some_and(|c| !c.cells.is_empty())
        );
        assert!(grant.pages.contains_key(&f.anchor));
        assert!(!grant.pages.contains_key(&target.page.id));
    }
    #[tokio::test]
    async fn ordinary_pdf_material_reference_and_destination_cells_share_one_request() {
        let f = Fixture::new();
        let target = storage::create_calculation_page(
            &f.root,
            TIME,
            &PagePosition::Last,
            "Calculation",
            None,
        )
        .unwrap();
        let reference = target.page.calculation.as_ref().unwrap();
        let doc =
            goodtype_core::calculation::Calculation::parse(&target.calculations[0].bytes).unwrap();
        let cell = doc.cells[0].id.clone();
        let mut grant = snapshot::capture_locked(
            &f.bridge.roots,
            &f.bridge.histories,
            RequestContextIntent {
                share_page_names: true,
                cells: Some(CellGrantIntent {
                    calculation_id: reference.id.clone(),
                    part_id: reference.part_id.clone(),
                    cell_ids: vec![cell.clone()],
                    include_outputs: true,
                }),
                destination: RequestDestination {
                    root: f.root.to_string_lossy().into(),
                    page_id: target.page.id.clone(),
                },
                selection: None,
                page_ids: vec![f.anchor.clone()],
                presets: false,
                intent_version: "mixed".into(),
            },
            Usage::Detailed,
        )
        .unwrap();
        grant.conversation_id = "conversation".into();
        f.bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .sessions
            .get_mut(&f.bridge.id)
            .unwrap()
            .install_request(grant);
        let context = f
            .bridge
            .run(
                "get_context",
                json!({}),
                &mut Default::default(),
                &mut String::new(),
            )
            .await
            .unwrap();
        let context = serde_json::to_value(context).unwrap();
        let text = context[0]["text"].as_str().unwrap();
        assert!(text.contains(&f.anchor));
        assert!(text.contains(&cell));
        let rendered = f
            .bridge
            .run(
                "render",
                json!({"page":f.anchor,"scale":0.5}),
                &mut Default::default(),
                &mut String::new(),
            )
            .await
            .unwrap();
        assert!(
            serde_json::to_value(rendered)
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .any(|b| b["type"] == "image")
        );
        let proposal = f
            .bridge
            .propose(
                "propose_cell_edit",
                json!({"cellId":cell,"source":"print(42)","summary":"Use the reference"}),
            )
            .await
            .unwrap();
        assert_eq!(proposal["kind"], "cell_edit");
        assert_eq!(
            storage::open_page(&f.root, &target.page.id)
                .unwrap()
                .calculations[0]
                .bytes,
            target.calculations[0].bytes
        );
    }
    #[tokio::test]
    async fn two_attached_python_pages_share_one_context_and_proposals_follow_the_anchor() {
        let f = Fixture::new();
        let python = |title: &str| {
            let b =
                storage::create_calculation_page(&f.root, TIME, &PagePosition::Last, title, None)
                    .unwrap();
            let doc =
                goodtype_core::calculation::Calculation::parse(&b.calculations[0].bytes).unwrap();
            (b.page.id, doc.cells[0].id.clone())
        };
        let (first, first_cell) = python("First");
        let (second, second_cell) = python("Second");
        let mut grant = snapshot::capture_locked(
            &f.bridge.roots,
            &f.bridge.histories,
            RequestContextIntent {
                share_page_names: false,
                cells: None,
                destination: RequestDestination {
                    root: f.root.to_string_lossy().into(),
                    page_id: f.anchor.clone(),
                },
                selection: None,
                page_ids: vec![f.anchor.clone(), first.clone(), second.clone()],
                presets: false,
                intent_version: "unified".into(),
            },
            Usage::Detailed,
        )
        .unwrap();
        grant.conversation_id = "conversation".into();
        f.bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .sessions
            .get_mut(&f.bridge.id)
            .unwrap()
            .install_request(grant);
        let context = f
            .bridge
            .run(
                "get_context",
                json!({}),
                &mut Default::default(),
                &mut String::new(),
            )
            .await
            .unwrap();
        let text = serde_json::to_value(context).unwrap()[0]["text"]
            .as_str()
            .unwrap()
            .to_owned();
        for id in [&f.anchor, &first_cell, &second_cell] {
            assert!(text.contains(id.as_str()), "{id}");
        }
        let proposal = f
            .bridge
            .propose(
                "propose_cell_edit",
                json!({"cellId":second_cell,"source":"print(2)","summary":"Fix"}),
            )
            .await
            .unwrap();
        let id = proposal["id"].as_str().unwrap();
        let s = f.bridge.helper.inner.lock().unwrap();
        assert_eq!(s.sessions[&f.bridge.id].proposals[id].page_id, second);
    }
}

/// Navigation metadata only. Never serialize an arbitrary source line as an ungranted title.
pub(super) fn page_label(b: &NotebookSnapshot) -> (String, &'static str) {
    let clean = |s: &str| {
        s.chars()
            .filter(|c| !c.is_control())
            .take(120)
            .collect::<String>()
    };
    if let Some(reference) = &b.page.calculation {
        let title = b
            .calculations
            .iter()
            .find(|f| f.path == reference.source_path)
            .and_then(|f| serde_json::from_slice::<Value>(&f.bytes).ok())
            .and_then(|v| v["metadata"]["goodtype"]["title"].as_str().map(clean))
            .unwrap_or_default();
        return (title, "python");
    }
    if let goodtype_core::PageBackground::Pdf { source_path, page } = &b.page.background {
        return (
            format!(
                "{} · {}",
                clean(
                    &Path::new(source_path)
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
                page
            ),
            "pdf",
        );
    }
    let title = b
        .blocks
        .iter()
        .filter_map(|f| std::str::from_utf8(&f.bytes).ok())
        .flat_map(str::lines)
        .find_map(|line| {
            let text = line.trim_start().trim_start_matches('=');
            (text.len() < line.trim_start().len() && text.starts_with(' '))
                .then(|| clean(text.trim()))
        })
        .unwrap_or_default();
    (
        title,
        if b.page
            .objects
            .iter()
            .any(|o| matches!(o, goodtype_core::PageObject::PageTypst { .. }))
        {
            "page_text"
        } else {
            "page"
        },
    )
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitialCell {
    pub cell_type: String,
    pub source: String,
}
fn last_position() -> PagePosition {
    PagePosition::Last
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Draft {
    pub page_type: String,
    #[serde(default)]
    pub target_page_id: Option<String>,
    pub title: String,
    #[serde(default = "last_position")]
    pub position: PagePosition,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub cells: Vec<InitialCell>,
    pub summary: String,
    #[serde(default)]
    pub assumptions: Vec<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageProposal {
    #[serde(flatten)]
    pub draft: Draft,
    pub svg: Option<String>,
    #[serde(skip)]
    pub grant: Option<Arc<CapturedRequestGrant>>,
    #[serde(skip)]
    pub generation: u64,
    #[serde(skip)]
    pub manifest: Vec<u8>,
    #[serde(skip)]
    pub append_base: Option<(String, String, u64, String)>,
    #[serde(skip)]
    pub captured: Arc<CapturedRequestGrant>,
}

fn signature(snapshot: &NotebookSnapshot) -> Result<Vec<u8>, String> {
    let mut manifest = snapshot.manifest.clone();
    manifest.modified_at.clear();
    serde_json::to_vec(&manifest).map_err(|e| e.to_string())
}

// A follow-up replaces read authority, but an already published review keeps its
// captured inputs. Explicit removal/Stop/End marks proposals stale or revokes the
// session. Match existing block review semantics; never give the model old reads.
fn review_authorized(state: &Inner, proposal: &proposals::Proposal) -> bool {
    let Some(page) = &proposal.new_page else {
        return false;
    };
    let Some(grant) = &page.grant else {
        return state.grant_current(&proposal.session_id, page.generation, None);
    };
    state
        .sessions
        .get(&proposal.session_id)
        .is_some_and(|session| {
            !session
                .launch_cancelled
                .as_ref()
                .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
                && session.conversation.as_deref() == Some(grant.conversation_id.as_str())
                && session
                    .request
                    .as_ref()
                    .is_some_and(|current| current.root == grant.root)
        })
}
fn validate(d: &Draft, snapshot: &NotebookSnapshot) -> Result<(), String> {
    if !matches!(d.page_type.as_str(), "blank" | "page_text" | "python")
        || d.title.trim().is_empty()
        || d.title.len() > 240
        || d.summary.trim().is_empty()
        || d.summary.len() > 4000
        || d.source.len() > 65536
        || d.cells.len() > 100
        || d.cells
            .iter()
            .any(|c| !matches!(c.cell_type.as_str(), "code" | "markdown") || c.source.len() > 65536)
        || d.cells.iter().map(|c| c.source.len()).sum::<usize>() > 128 * 1024
        || d.assumptions.len() > 20
        || d.assumptions.iter().any(|s| s.len() > 2000)
        || d.page_type == "python" && (d.cells.is_empty() || !d.source.is_empty())
        || d.page_type != "python" && !d.cells.is_empty()
        || d.page_type == "blank" && !d.source.is_empty()
    {
        return Err("invalid or oversized page proposal".into());
    }
    if let Some(target) = &d.target_page_id
        && (d.page_type != "python" || !snapshot.manifest.pages.iter().any(|p| &p.id == target))
    {
        return Err("invalid Python destination: targetPageId must name an existing Python page; for other pages omit it and use position {kind:\"after\",pageId}".into());
    }
    match &d.position {
        PagePosition::Before { page_id } | PagePosition::After { page_id }
            if !snapshot.manifest.pages.iter().any(|p| &p.id == page_id) =>
        {
            Err("not shared: placement page is outside this notebook".into())
        }
        _ => Ok(()),
    }
}
fn preview(mut p: proposals::Proposal) -> Result<proposals::Proposal, String> {
    let page = p.new_page.as_ref().ok_or("missing page draft")?;
    if page.draft.page_type == "page_text" {
        let mut snapshot = p.page_snapshot.clone().ok_or("missing preview page")?;
        snapshot.page.calculation = None;
        snapshot.page.geometry = p.geometry.clone();
        snapshot.page.background = snapshot.manifest.default_page.background.clone();
        snapshot.page.objects.clear();
        snapshot.page.reading_order.clear();
        snapshot
            .blocks
            .retain(|f| p.files.contains_key(&format!("/{}", f.path)));
        snapshot.assets.clear();
        snapshot.calculations.clear();
        for layer in &mut snapshot.ink_layers {
            layer.strokes.clear();
        }
        let (svg, _, _, overflow) =
            proposals::compile_page(&snapshot, &p.root, &page.draft.source, &p.files, false)?;
        p.overflow = overflow;
        p.new_page.as_mut().unwrap().svg = Some(svg);
    }
    Ok(p)
}
impl bridge::Bridge {
    pub(super) async fn propose_new_page(
        &self,
        args: Value,
        stamp: (u64, Option<Arc<CapturedRequestGrant>>),
    ) -> Result<Value, String> {
        let draft: Draft =
            serde_json::from_value(args).map_err(|_| "invalid page proposal fields")?;
        let captured = if let Some(grant) = &stamp.1 {
            grant.clone()
        } else {
            let (share, usage) = {
                let s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
                let (share, info) = s
                    .share_for(&self.id)
                    .ok_or("not shared: select material first")?;
                if !info.session_ids.contains(&self.id) {
                    return Err("not shared: selection".into());
                }
                (share.clone(), s.settings.usage)
            };
            Arc::new(
                snapshot::capture(
                    self.roots.clone(),
                    self.histories.clone(),
                    RequestContextIntent {
                        share_page_names: false,
                        destination: RequestDestination {
                            root: share.root.clone(),
                            page_id: share.page_id.clone(),
                        },
                        selection: Some(SharePayload {
                            session_ids: None,
                            ..share
                        }),
                        page_ids: vec![],
                        presets: false,
                        cells: None,
                        intent_version: "terminal-page-proposal".into(),
                    },
                    usage,
                )
                .await
                .map_err(|p| p.code)?,
            )
        };
        validate(&draft, &captured.destination)?;
        let (own, global) = {
            let s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            if !s.grant_current(&self.id, stamp.0, stamp.1.as_deref()) {
                return Err("changed: request revoked".into());
            }
            let session = s.sessions.get(&self.id).ok_or("session ended")?;
            if proposals::pending_count(session) >= 20 {
                return Err("limit: 20 pending proposals".into());
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
        let append_base = if let Some(target) = &draft.target_page_id {
            let b = snapshot::read_with(
                self.roots.clone(),
                self.histories.clone(),
                &captured.root,
                target,
            )
            .await?;
            if signature(&b)? != signature(&captured.destination)? {
                return Err("changed: notebook structure changed".into());
            }
            let reference = b
                .page
                .calculation
                .as_ref()
                .ok_or("destination is not a Python page: omit targetPageId and propose a new Python page with position {kind:\"after\",pageId} instead")?;
            let bytes = &b
                .calculations
                .iter()
                .find(|f| f.path == reference.source_path)
                .ok_or("missing calculation")?
                .bytes;
            let document =
                goodtype_core::calculation::Calculation::parse(bytes).map_err(|e| e.to_string())?;
            Some((
                reference.id.clone(),
                reference.part_id.clone(),
                document.revision(),
                goodtype_core::calculation::source_hash(
                    std::str::from_utf8(bytes).map_err(|_| "invalid calculation")?,
                ),
            ))
        } else {
            None
        };
        let id = random_hex(16)?;
        let p = proposals::Proposal {
            new_page: Some(PageProposal {
                draft: draft.clone(),
                svg: None,
                grant: stamp.1.clone(),
                generation: stamp.0,
                manifest: signature(&captured.destination)?,
                append_base,
                captured: captured.clone(),
            }),
            cell: None,
            preset: None,
            id: id.clone(),
            session_id: self.id.clone(),
            root: captured.root.clone(),
            page_id: captured.destination.page.id.clone(),
            version: 1,
            status: "pending".into(),
            text: None,
            changed: vec![],
            summary: draft.summary,
            assumptions: draft.assumptions,
            edit: false,
            kind: if draft.target_page_id.is_some() {
                "cells_append"
            } else {
                "page_create"
            }
            .into(),
            page_number: captured.info.page_number,
            overflow: false,
            managed_header: None,
            page_snapshot: Some(captured.destination.clone()),
            dependencies: vec![],
            baseline_grid: false,
            items: vec![],
            base: captured.destination_base.clone(),
            files: captured.files(),
            style: String::new(),
            geometry: captured.destination.manifest.default_page.geometry.clone(),
            anchor: captured.anchor.crop_pt,
        };
        let p = tauri::async_runtime::spawn_blocking(move || {
            let _permits = (own, global);
            preview(p)
        })
        .await
        .map_err(|e| e.to_string())??;
        let mut s = self.helper.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !s.grant_current(&self.id, stamp.0, stamp.1.as_deref()) {
            return Err("changed: request revoked".into());
        }
        let session = s.sessions.get_mut(&self.id).ok_or("session ended")?;
        if proposals::pending_count(session) >= 20 {
            return Err("limit: 20 pending proposals".into());
        }
        session.proposals.insert(id.clone(), p.clone());
        proposals::emit(self.app.as_ref(), &p);
        Ok(
            json!({"id":id,"status":"pending","version":1,"kind":p.kind,"position":p.new_page.as_ref().unwrap().draft.position,"overflow":p.overflow}),
        )
    }
}

pub(super) async fn edit(
    helper: &Helper,
    app: Option<&tauri::AppHandle>,
    mut p: proposals::Proposal,
    version: u64,
    sources: Vec<String>,
) -> Result<proposals::Proposal, String> {
    let page = p.new_page.as_mut().ok_or("missing page draft")?;
    if p.status != "pending" || p.version != version {
        return Err("proposal changed".into());
    }
    if page.draft.page_type == "python" {
        if sources.len() != page.draft.cells.len() {
            return Err("invalid page sources".into());
        }
        for (cell, source) in page.draft.cells.iter_mut().zip(sources) {
            cell.source = source;
        }
    } else {
        if sources.len() != 1 {
            return Err("invalid page sources".into());
        }
        page.draft.source = sources[0].clone();
    }
    validate(&page.draft, p.page_snapshot.as_ref().ok_or("missing page")?)?;
    let (own, global) = {
        let s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !review_authorized(&s, &p) {
            return Err("changed: request revoked".into());
        }
        (
            s.sessions
                .get(&p.session_id)
                .ok_or("session ended")?
                .compile
                .clone()
                .try_acquire_owned()
                .map_err(|_| "busy")?,
            helper
                .compile
                .clone()
                .try_acquire_owned()
                .map_err(|_| "busy")?,
        )
    };
    p = tauri::async_runtime::spawn_blocking(move || {
        let _permits = (own, global);
        preview(p)
    })
    .await
    .map_err(|e| e.to_string())??;
    let mut s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
    if !review_authorized(&s, &p) {
        return Err("changed: request revoked".into());
    }
    let stored = proposals::find_mut(&mut s, &p.id)?;
    if stored.version != version || stored.status != "pending" {
        return Err("proposal changed".into());
    }
    p.version += 1;
    *stored = p.clone();
    proposals::emit(app, &p);
    Ok(p)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyInput {
    pub proposal_id: String,
    pub proposal_version: u64,
    pub position: PagePosition,
    pub modified_at: String,
}

/// Caller holds the notebook lock. Returns a new page only after the complete draft is persisted.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    #[serde(flatten)]
    pub structure: storage::StructureHistoryResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calculation: Option<storage::CalculationResult>,
}

pub(crate) fn apply(
    helper: &Helper,
    app: Option<&tauri::AppHandle>,
    root: &Path,
    store: &mut crate::notebook::HistoryStore,
    input: ApplyInput,
) -> Result<ApplyResult, crate::notebook::NotebookError> {
    let mut s = helper
        .inner
        .lock()
        .map_err(crate::notebook::NotebookError::other)?;
    let p = s
        .sessions
        .values()
        .find_map(|session| session.proposals.get(&input.proposal_id))
        .cloned()
        .ok_or(storage::StorageError::ProposalStale(vec![]))?;
    let page = p.new_page.as_ref().ok_or("wrong proposal kind")?;
    if p.version != input.proposal_version || p.status != "pending" {
        return Err("proposal changed or already inserted".into());
    }
    if !review_authorized(&s, &p) {
        return Err(storage::StorageError::ProposalStale(vec![]).into());
    }
    if Path::new(&p.root) != root {
        return Err("wrong_page".into());
    }
    let current = storage::open_page(root, &p.page_id)?;
    if signature(&current).map_err(crate::notebook::NotebookError::other)? != page.manifest {
        return Err(storage::StorageError::ProposalStale(vec![]).into());
    }
    storage::check_proposal_base(root, &current, &p.base)?;
    if page
        .captured
        .cells
        .iter()
        .any(|(id, cells)| !super::cells::current(root, id, cells))
    {
        return Err(storage::StorageError::ProposalStale(vec![]).into());
    }
    for (path, expected) in &page.captured.preset_files {
        let file = goodtype_core::paths::resolve_file(root, path.trim_start_matches('/'))
            .map_err(crate::notebook::NotebookError::other)?;
        let mut actual = Vec::new();
        std::fs::File::open(file)?
            .take(expected.len() as u64 + 1)
            .read_to_end(&mut actual)?;
        if actual != *expected {
            return Err(storage::StorageError::ProposalStale(vec![]).into());
        }
    }
    // The selection entry holds filtered material even when its page is also attached whole,
    // so it is re-filtered by role, not by whether its page id appears among the attachments.
    for (source, selected) in page
        .captured
        .pages
        .values()
        .map(|p| (p, false))
        .chain(page.captured.selection.iter().map(|p| (p, true)))
    {
        let actual = storage::open_page(root, &source.snapshot.page.id)?;
        let actual = if selected {
            if page.captured.anchor.object_ids.is_empty()
                && page.captured.anchor.stroke_ids.is_empty()
            {
                snapshot::region_filtered(&actual, page.captured.anchor.crop_pt)
            } else {
                snapshot::shared_filtered(
                    &actual,
                    &source
                        .snapshot
                        .page
                        .objects
                        .iter()
                        .map(|o| o.fields().id.clone())
                        .collect(),
                    &page.captured.anchor.stroke_ids,
                )
            }
        } else {
            actual
        };
        if serde_json::to_vec(&(&actual.page, &actual.ink_layers))
            .map_err(crate::notebook::NotebookError::other)?
            != serde_json::to_vec(&(&source.snapshot.page, &source.snapshot.ink_layers))
                .map_err(crate::notebook::NotebookError::other)?
        {
            return Err(storage::StorageError::ProposalStale(vec![]).into());
        }
        for (path, expected) in source.files.iter().chain(source.render_files.iter()) {
            let file = goodtype_core::paths::resolve_file(root, path.trim_start_matches('/'))
                .map_err(|_| {
                    crate::notebook::NotebookError::from(storage::StorageError::ProposalStale(
                        vec![],
                    ))
                })?;
            let mut actual = Vec::new();
            std::fs::File::open(file)?
                .take(expected.len() as u64 + 1)
                .read_to_end(&mut actual)?;
            if actual != *expected {
                return Err(storage::StorageError::ProposalStale(vec![]).into());
            }
        }
    }
    let mut draft = page.draft.clone();
    draft.position = input.position;
    validate(&draft, &current).map_err(crate::notebook::NotebookError::other)?;
    if p.overflow {
        return Err("does_not_fit".into());
    }
    if let Some(target) = &draft.target_page_id {
        let base = page
            .append_base
            .as_ref()
            .ok_or("missing calculation base")?;
        let mut cells = Vec::new();
        for source in &draft.cells {
            let mut cell = goodtype_core::calculation::Cell::new(
                format!(
                    "cell-{}",
                    random_hex(16).map_err(crate::notebook::NotebookError::other)?
                ),
                &base.1,
                &source.cell_type,
            );
            cell.source = source.source.clone();
            cells.push(cell);
        }
        let result = store.append_helper_cells(
            root,
            target,
            (&base.0, &base.1, base.2, &base.3),
            cells,
            &input.modified_at,
        )?;
        let snapshot = storage::open_page(root, target)?;
        let structure = storage::StructureHistoryResult {
            snapshot,
            can_undo: result.can_undo,
            can_redo: result.can_redo,
        };
        let stored = proposals::find_mut(&mut s, &input.proposal_id)
            .map_err(crate::notebook::NotebookError::other)?;
        stored.status = "inserted".into();
        proposals::emit(app, stored);
        return Ok(ApplyResult {
            structure,
            calculation: Some(result),
        });
    }
    crate::notebook::observe_structure(store, root, &p.page_id)?;
    let snapshot = match draft.page_type.as_str() {
        "python" => {
            let cells: Vec<_> = draft.cells.iter().map(|c| json!({"cell_type":c.cell_type,"source":c.source,"metadata":{},"execution_count":null,"outputs":[]})).collect();
            let bytes = serde_json::to_vec(&json!({"nbformat":4,"nbformat_minor":5,"metadata":{"language_info":{"name":"python"}},"cells":cells})).map_err(crate::notebook::NotebookError::other)?;
            storage::import_calculation_page(
                root,
                &input.modified_at,
                &draft.position,
                &draft.title,
                &bytes,
            )?
        }
        "page_text" => storage::create_page_with_text(
            root,
            &input.modified_at,
            &draft.position,
            Some(&current.manifest.default_page.background),
            Some(&p.geometry),
            &draft.source,
        )?,
        _ => storage::create_page(
            root,
            &input.modified_at,
            &draft.position,
            None,
            Some(&p.geometry),
        )?,
    };
    let result = crate::notebook::advance_structure_result(store, root, snapshot)?;
    let stored = proposals::find_mut(&mut s, &input.proposal_id)
        .map_err(crate::notebook::NotebookError::other)?;
    stored.status = "inserted".into();
    proposals::emit(app, stored);
    Ok(ApplyResult {
        structure: result,
        calculation: None,
    })
}
