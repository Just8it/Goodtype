use super::proposals::*;
use super::*;
use goodtype_core::storage;
use serde_json::{Value, json};

struct Test {
    temp: tempfile::TempDir,
    root: std::path::PathBuf,
    bridge: bridge::Bridge,
    base: storage::ProposalBase,
}
impl Test {
    fn new() -> Self {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/notebooks/minimal");
        let snapshot = storage::open_notebook(&fixture).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("book");
        std::fs::create_dir_all(root.join("references")).unwrap();
        std::fs::copy(
            fixture.join("references/lecture.pdf"),
            root.join("references/lecture.pdf"),
        )
        .unwrap();
        storage::create_notebook(&root, &snapshot).unwrap();
        let root = root.canonicalize().unwrap();
        let snapshot = storage::open_notebook(&root).unwrap();
        let base = storage::proposal_base(&root, &snapshot, &["typst-001".into()]).unwrap();
        let helper = Helper::default();
        let mut session = bridge::new_session("test".into());
        session.info.approved = true;
        let crop = Rect {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 200.0,
        };
        {
            let mut s = helper.inner.lock().unwrap();
            s.settings.enabled = true;
            s.settings.usage = Usage::Detailed;
            s.sessions.insert("test".into(), session);
            s.share = Some((
                SharePayload {
                    root: root.to_string_lossy().into(),
                    page_id: snapshot.page.id.clone(),
                    object_ids: vec!["typst-001".into()],
                    stroke_ids: vec![],
                    crop_pt: crop,
                    session_ids: None,
                },
                ShareInfo {
                    page_id: snapshot.page.id.clone(),
                    page_number: 1,
                    object_count: 1,
                    crop_pt: crop,
                    session_ids: vec!["test".into()],
                    shared_at: now(),
                },
            ));
        }
        let roots = Arc::new(crate::workspace::AllowedRoots::default());
        crate::workspace::allow_root(&roots, &root).unwrap();
        Self {
            temp,
            root,
            bridge: bridge::Bridge {
                helper,
                app: None,
                id: "test".into(),
                roots,
                histories: Default::default(),
            },
            base,
        }
    }
    fn args(&self, source: &str) -> Value {
        json!({"blockId":"typst-001","proposalBase":base_to_wire(&self.base),"source":source,"summary":"Fix","assumptions":["positive mass"]})
    }
    async fn propose(&self) -> Proposal {
        let result = self
            .bridge
            .propose("propose_block_edit", self.args("$ F = m a $"))
            .await
            .unwrap();
        self.bridge.helper.inner.lock().unwrap().sessions["test"].proposals
            [result["id"].as_str().unwrap()]
        .clone()
    }
    fn insert(
        &self,
        p: &Proposal,
        history: &mut storage::NotebookHistory,
    ) -> Result<storage::HistoryResult, storage::StorageError> {
        insert(
            &self.bridge.helper,
            None,
            &self.root,
            &p.page_id,
            history,
            ProposalInsert {
                proposal_id: p.id.clone(),
                proposal_version: p.version,
            },
        )
    }
}
#[tokio::test]
async fn proposal_rejects_compile_failure_foreign_target_and_twenty_first_pending() {
    let t = Test::new();
    let before = storage::open_notebook(&t.root).unwrap();
    assert!(
        t.bridge
            .propose("propose_block_edit", t.args("#not_defined( "))
            .await
            .is_err()
    );
    let mut args = t.args("Hello");
    args["blockId"] = json!("image-001");
    assert!(
        t.bridge
            .propose("propose_block_edit", args)
            .await
            .unwrap_err()
            .contains("not shared")
    );
    let foreign = json!({"page":"other-page","proposalBase":t.base,"items":[{"source":"Text","placement":"below_selection"}],"summary":"Foreign"});
    assert!(
        t.bridge
            .propose("propose_blocks", foreign)
            .await
            .unwrap_err()
            .contains("not shared")
    );
    let p = t.propose().await;
    {
        let mut s = t.bridge.helper.inner.lock().unwrap();
        let session = s.sessions.get_mut("test").unwrap();
        for n in 1..20 {
            let mut p = p.clone();
            p.id = format!("p{n}");
            session.proposals.insert(p.id.clone(), p);
        }
        assert_eq!(pending_count(session), 20);
    }
    assert!(
        t.bridge
            .propose("propose_block_edit", t.args("Hello"))
            .await
            .unwrap_err()
            .contains("20 pending")
    );
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(storage::open_notebook(&t.root).unwrap()).unwrap()
    );
    assert!(t.temp.path().exists());
}
#[tokio::test]
async fn insert_undo_redo_duplicate_and_unrelated_edit() {
    let t = Test::new();
    let mut history = storage::NotebookHistory::default();
    let mut current = storage::observe_notebook(&t.root, &mut history).unwrap();
    let p = t.propose().await;
    current
        .page
        .objects
        .iter_mut()
        .find(|o| o.fields().id == "image-001")
        .unwrap()
        .fields_mut()
        .x += 5.0;
    storage::commit_notebook(&t.root, &mut history, current).unwrap();
    let before = storage::open_notebook(&t.root).unwrap();
    let result = t.insert(&p, &mut history).unwrap();
    assert!(result.can_undo);
    assert!(t.insert(&p, &mut history).is_err());
    let undone = storage::undo_notebook(&t.root, &mut history).unwrap();
    assert_eq!(
        serde_json::to_value(undone.snapshot.blocks).unwrap(),
        serde_json::to_value(before.blocks).unwrap()
    );
    let redone = storage::redo_notebook(&t.root, &mut history).unwrap();
    assert_eq!(
        serde_json::to_value(redone.snapshot.blocks).unwrap(),
        serde_json::to_value(result.snapshot.blocks).unwrap()
    );
}
#[tokio::test]
async fn stale_target_writes_nothing_and_marks_dependency() {
    let t = Test::new();
    let mut history = storage::NotebookHistory::default();
    let mut current = storage::observe_notebook(&t.root, &mut history).unwrap();
    let p = t.propose().await;
    current
        .page
        .objects
        .iter_mut()
        .find(|o| o.fields().id == "typst-001")
        .unwrap()
        .fields_mut()
        .x += 5.0;
    storage::commit_notebook(&t.root, &mut history, current).unwrap();
    let before = storage::open_notebook(&t.root).unwrap();
    assert!(matches!(
        t.insert(&p, &mut history),
        Err(storage::StorageError::ProposalStale(_))
    ));
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(storage::open_notebook(&t.root).unwrap()).unwrap()
    );
    let s = t.bridge.helper.inner.lock().unwrap();
    let p = &s.sessions["test"].proposals[&p.id];
    assert_eq!(p.status, "stale");
    assert!(!p.changed.is_empty());
}
#[tokio::test]
async fn new_blocks_compile_all_and_keep_creation_identity() {
    let t = Test::new();
    let args = json!({"page":"page-001","proposalBase":t.base,"items":[{"source":"First","placement":"beside_selection"},{"source":"#unknown()","placement":"below_selection"}],"summary":"Two"});
    assert!(
        t.bridge
            .propose("propose_blocks", args.clone())
            .await
            .is_err()
    );
    assert!(
        t.bridge.helper.inner.lock().unwrap().sessions["test"]
            .proposals
            .is_empty()
    );
    let mut args = args;
    args["items"][1]["source"] = json!("Second");
    let result = t.bridge.propose("propose_blocks", args).await.unwrap();
    let p = t.bridge.helper.inner.lock().unwrap().sessions["test"].proposals
        [result["id"].as_str().unwrap()]
    .clone();
    let mut history = storage::NotebookHistory::default();
    storage::observe_notebook(&t.root, &mut history).unwrap();
    let saved = t.insert(&p, &mut history).unwrap();
    for item in p.items {
        assert!(
            saved
                .snapshot
                .page
                .objects
                .iter()
                .any(|o| o.fields().id == item.object.fields().id)
        );
    }
}
#[tokio::test]
async fn rmcp_propose_status_and_session_end_discard() {
    use rmcp::{ServiceExt, model::CallToolRequestParams};
    let t = Test::new();
    let p = t.propose().await;
    // Keep the first proposal too, to check that disconnect drops all pending state.
    let stop = t.bridge.helper.inner.lock().unwrap().sessions["test"]
        .stop
        .subscribe();
    let bridge = t.bridge.clone();
    let (client, server) = tokio::io::duplex(1024 * 1024);
    let task = tokio::spawn(async move {
        bridge.serve_stream(server, stop).await;
    });
    let client = ().serve(client).await.unwrap();
    async fn call(
        client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
        name: &str,
        args: Value,
    ) -> Value {
        let result = client
            .call_tool(
                CallToolRequestParams::new(name.to_owned())
                    .with_arguments(args.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        assert_ne!(result.is_error, Some(true), "{result:?}");
        let v = serde_json::to_value(result).unwrap();
        serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap()
    }
    let result = call(&client, "propose_block_edit", t.args("Correct")).await;
    assert_eq!(
        call(&client, "proposal_status", json!({"id":result["id"]})).await["status"],
        "pending"
    );
    assert_eq!(
        call(&client, "proposal_status", json!({"id":p.id})).await["status"],
        "pending"
    );
    client.cancel().await.unwrap();
    task.await.unwrap();
    assert!(t.bridge.helper.inner.lock().unwrap().sessions.is_empty());
}

#[tokio::test]
async fn destination_and_review_version_are_enforced_before_writing() {
    let t = Test::new();
    let p = t.propose().await;
    let mut history = storage::NotebookHistory::default();
    storage::observe_notebook(&t.root, &mut history).unwrap();
    let before = serde_json::to_vec(&storage::open_notebook(&t.root).unwrap()).unwrap();
    assert!(
        insert(
            &t.bridge.helper,
            None,
            &t.root,
            "wrong-page",
            &mut history,
            ProposalInsert {
                proposal_id: p.id.clone(),
                proposal_version: p.version
            }
        )
        .is_err()
    );
    assert!(
        insert(
            &t.bridge.helper,
            None,
            &t.root,
            &p.page_id,
            &mut history,
            ProposalInsert {
                proposal_id: p.id.clone(),
                proposal_version: p.version + 1
            }
        )
        .is_err()
    );
    assert_eq!(
        before,
        serde_json::to_vec(&storage::open_notebook(&t.root).unwrap()).unwrap()
    );
}

/// Opposite lock acquisition would deadlock: reads queue behind a notebook owner which
/// then inserts (notebook -> helper). The timeout releases the notebook even on regression.
#[tokio::test]
async fn proposal_insert_and_helper_reads_obey_lock_order() {
    use rmcp::{ServiceExt, model::CallToolRequestParams};
    let t = Test::new();
    let p = t.propose().await;
    {
        let mut state = t.bridge.helper.inner.lock().unwrap();
        state.sessions.get_mut("test").unwrap().notebook = true;
    }
    let bridge = t.bridge.clone();
    let stop = bridge.helper.inner.lock().unwrap().sessions["test"]
        .stop
        .subscribe();
    let (client, server) = tokio::io::duplex(1024 * 1024);
    let server = tokio::spawn(async move { bridge.serve_stream(server, stop).await });
    let client = ().serve(client).await.unwrap();
    let (held_tx, held_rx) = tokio::sync::oneshot::channel();
    let histories = t.bridge.histories.clone();
    let helper = t.bridge.helper.clone();
    let root = t.root.clone();
    let owner = std::thread::spawn(move || {
        crate::notebook::with_notebook(&histories, || {
            held_tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(200));
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if let Ok(guard) = helper.inner.try_lock() {
                    drop(guard);
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("helper lock held by a read waiting for notebook".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let mut history = storage::NotebookHistory::default();
            storage::observe_notebook(&root, &mut history).map_err(|e| e.to_string())?;
            insert(
                &helper,
                None,
                &root,
                &p.page_id,
                &mut history,
                ProposalInsert {
                    proposal_id: p.id,
                    proposal_version: p.version,
                },
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    });
    held_rx.await.unwrap();
    let call = |name: &'static str, args: Value| {
        client.call_tool(
            CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
        )
    };
    tokio::time::timeout(Duration::from_secs(15), async {
        let results = tokio::join!(
            call("get_context", json!({"include_image":false})),
            call("read_page", json!({"page":1})),
            call("render", json!({"page":1})),
            call("search", json!({"query":"force"})),
            call("propose_block_edit", t.args("Correct")),
        );
        // Context can report no_window and propose can become stale after insertion;
        // neither is allowed to keep the helper lock while waiting for the notebook.
        assert!(
            results.0.is_ok()
                && results.1.is_ok()
                && results.2.is_ok()
                && results.3.is_ok()
                && results.4.is_ok()
        );
    })
    .await
    .expect("helper reads timed out");
    owner.join().unwrap().unwrap();
    client.cancel().await.unwrap();
    server.await.unwrap();
}
