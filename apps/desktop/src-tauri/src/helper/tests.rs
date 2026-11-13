use super::*;
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};

#[tokio::test]
async fn rmcp_client_enforces_approval_grants_context_diagnostics_limits_and_disconnect() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fixtures/notebooks/minimal")
        .canonicalize()
        .unwrap();
    let root_string = root.to_string_lossy().into_owned();
    let roots = Arc::new(crate::workspace::AllowedRoots::default());
    crate::workspace::allow_root(&roots, &root).unwrap();
    let helper = Helper::default();
    let id = "test-session".to_owned();
    helper
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .sessions
        .insert(id.clone(), bridge::new_session(id.clone()));
    let stop = helper.inner.lock().unwrap().sessions[&id].stop.subscribe();
    let bridge = bridge::Bridge {
        helper: helper.clone(),
        app: None,
        id: id.clone(),
        roots,
        histories: Default::default(),
    };
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
        serde_json::to_value(result).unwrap()
    }
    assert_ne!(call(&client, "status", json!({})).await["isError"], true);
    let waiting = call(&client, "get_context", json!({})).await;
    assert_eq!(waiting["isError"], true);
    assert!(waiting.to_string().contains("Waiting for approval"));
    let (requests, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    {
        let mut s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.sessions.get_mut(&id).unwrap().info.approved = true;
        s.settings.enabled = true;
        s.settings.usage = Usage::Balanced;
        s.sessions.get_mut(&id).unwrap().notebook = true;
        s.context_requests = Some(requests);
        let crop = Rect {
            x: 50.0,
            y: 50.0,
            width: 240.0,
            height: 200.0,
        };
        s.share = Some((
            SharePayload {
                root: root_string.clone(),
                page_id: "page-001".into(),
                object_ids: vec!["group-001".into()],
                stroke_ids: vec!["stroke-001".into()],
                crop_pt: crop,
                session_ids: Some(vec![id.clone()]),
            },
            ShareInfo {
                page_id: "page-001".into(),
                page_number: 1,
                object_count: 1,
                crop_pt: crop,
                session_ids: vec![id.clone()],
                shared_at: now(),
            },
        ));
    }
    let h = helper.clone();
    let root_reply = root_string.clone();
    let frontend = tokio::spawn(async move {
        let request = receiver.recv().await.unwrap();
        h.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contexts
            .remove(&request)
            .unwrap()
            .1
            .send(ContextReply {
                request_id: request,
                status: "ok".into(),
                root: Some(root_reply),
                page_id: Some("page-001".into()),
                edit_generation: Some(1),
            })
            .unwrap();
    });
    let context = call(&client, "get_context", json!({})).await;
    assert_ne!(context["isError"], true, "{context}");
    assert!(context.to_string().contains("proposalBase"));
    assert!(
        !context.to_string().contains("image-001"),
        "get_context widened beyond selection: {context}"
    );
    {
        let mut s = helper.inner.lock().unwrap();
        let image = &s.activity.front().unwrap().handed_over.images[0];
        assert_eq!(
            (image.width, image.height),
            (480, 400),
            "context must use shared crop even with notebook grant"
        );
        s.sessions.get_mut(&id).unwrap().notebook = false;
    }
    frontend.await.unwrap();
    assert!(
        call(&client, "read_page", json!({"page":1}))
            .await
            .to_string()
            .contains("page scope required")
    );
    helper
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .settings
        .usage = Usage::Balanced;
    assert!(
        call(
            &client,
            "render",
            json!({"page":1,"rect":{"x":0,"y":0,"width":500,"height":500}})
        )
        .await
        .to_string()
        .contains("outside approved crop")
    );
    assert!(
        call(
            &client,
            "transcribe",
            json!({"kind":"text","strokeIds":["foreign"]})
        )
        .await
        .to_string()
        .contains("foreign stroke")
    );
    let compiled = call(
        &client,
        "typst_compile",
        json!({"source":"#unknown_goodtype_function()","target":{"kind":"block","widthPt":200}}),
    )
    .await;
    assert_ne!(compiled["isError"], true, "{compiled}");
    assert!(compiled.to_string().contains("line"));
    assert!(compiled.to_string().contains("column"));
    let diagnostics: Value =
        serde_json::from_str(compiled["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(diagnostics["diagnostics"][0]["line"], 1);
    assert!(diagnostics["diagnostics"][0]["column"].as_u64().unwrap() >= 1);
    let rendered = call(&client, "render", json!({"page":1})).await;
    assert_ne!(rendered["isError"], true, "{rendered}");
    assert!(rendered["structuredContent"].is_null());
    assert!(
        rendered["content"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["type"] == "image")
    );
    helper
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .settings
        .usage = Usage::Low;
    assert!(
        call(
            &client,
            "typst_compile",
            json!({"source":"hi","target":{"kind":"block","widthPt":200}})
        )
        .await
        .to_string()
        .contains("limit_reached")
    );
    client.cancel().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    let s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
    assert!(s.sessions.is_empty());
    assert!(s.share.as_ref().unwrap().1.session_ids.is_empty());
}

#[tokio::test]
async fn revoked_session_before_serve_does_not_poison_helper() {
    let helper = Helper::default();
    let session = bridge::new_session("gone".into());
    let stop = session.stop.subscribe();
    helper
        .inner
        .lock()
        .unwrap()
        .sessions
        .insert("gone".into(), session);
    helper.stop();
    let bridge = bridge::Bridge {
        helper: helper.clone(),
        app: None,
        id: "gone".into(),
        roots: Arc::new(crate::workspace::AllowedRoots::default()),
        histories: Default::default(),
    };
    let (_, stream) = tokio::io::duplex(1024);
    bridge.serve_stream(stream, stop).await;
    assert!(!helper.inner.is_poisoned());
    assert!(helper.status().sessions.is_empty());
}
#[test]
fn helper_recovers_poisoned_mutex() {
    let helper = Helper::default();
    let h = helper.clone();
    let _ = std::thread::spawn(move || {
        let _guard = h.inner.lock().unwrap();
        panic!("injected poison");
    })
    .join();
    assert!(!helper.status().enabled);
    helper.stop();
}

// P6: a preamble preset's relative include resolves beside the preset, from granted bytes.
#[test]
fn attached_page_preamble_relative_import_requires_module_origin() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fixtures/notebooks/minimal")
        .canonicalize()
        .unwrap();
    let bundle = goodtype_core::storage::open_page(&root, "page-001").unwrap();
    let mut page = snapshot::export(&bundle).unwrap();
    page.blocks.clear();
    page.strokes.clear();
    page.images.clear();
    page.shapes.clear();
    page.shared_style = None;
    page.background = goodtype_core::PageBackground::Plain {
        color: "#ffffff".into(),
    };
    let source =
        "#import \"/styles/a.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n\nBody";
    let files = goodtype_typst::GrantedFiles::from([
        ("/styles/a.typ".into(), b"#include \"nested.typ\"".to_vec()),
        (
            "/styles/nested.typ".into(),
            b"Granted nested material".to_vec(),
        ),
    ]);
    let result = goodtype_typst::compile_page_text_granted(
        &root,
        &page,
        &goodtype_typst::CompileRequest {
            source: source.into(),
            width_pt: page.width_pt,
            generation: 0,
            allow_remote_packages: false,
        },
        &files,
    )
    .unwrap();
    assert!(
        result.svg.is_some(),
        "fully granted preset must compile: {:?}",
        result.diagnostics
    );
}

mod request_grants {
    use super::*;
    use goodtype_core::{PageObject, storage};

    async fn approval(
        f: &Fixture,
        kind: &str,
    ) -> (
        tokio::task::JoinHandle<Result<Vec<rmcp::model::ContentBlock>, String>>,
        Value,
    ) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        f.bridge.helper.inner.lock().unwrap().scope_requests = Some(tx);
        let bridge = f.bridge.clone();
        let kind = kind.to_owned();
        let task = tokio::spawn(async move {
            bridge
                .run(
                    "request_scope",
                    json!({"scope":{"kind":kind,"pages":[3]},"reason":"Exact context needed"}),
                    &mut Default::default(),
                    &mut String::new(),
                )
                .await
        });
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        (task, event)
    }
    fn answer(f: &Fixture, event: &Value, value: &str) {
        let (_, tx) = f
            .bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .scopes
            .remove(event["requestId"].as_str().unwrap())
            .unwrap();
        tx.send(value.into()).unwrap();
    }
    #[tokio::test]
    async fn in_app_approval_real_allow_epoch_and_stale_reply_race() {
        let f = Fixture::new(3);
        f.install(&[0]);
        let old = f.bridge.helper.inner.lock().unwrap().sessions["app"]
            .request
            .clone()
            .unwrap();
        let (task, event) = approval(&f, "pages").await;
        assert_eq!(event["pageIds"], json!([f.pages[2]]));
        answer(&f, &event, "request");
        assert!(task.await.unwrap().is_ok());
        assert!(
            !f.bridge
                .helper
                .inner
                .lock()
                .unwrap()
                .grant_current("app", 0, Some(&old))
        );
        assert!(
            f.call("read_page", json!({"page":f.pages[2]}))
                .await
                .is_ok()
        );
        assert!(
            f.call("read_page", json!({"page":f.pages[1]}))
                .await
                .is_err()
        );
        // Real old read held at publication while a second approval changes epoch.
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        f.bridge.helper.inner.lock().unwrap().before_return =
            Some((entered.clone(), release.clone()));
        let bridge = f.bridge.clone();
        let page = f.pages[0].clone();
        let read = tokio::spawn(async move {
            bridge
                .run(
                    "read_page",
                    json!({"page":page}),
                    &mut Default::default(),
                    &mut String::new(),
                )
                .await
        });
        entered.notified().await;
        f.bridge.helper.inner.lock().unwrap().before_return = None;
        std::fs::write(f.root.join("styles/extra.typ"), "#let preset(body) = body").unwrap();
        let (task, event) = approval(&f, "presets").await;
        answer(&f, &event, "request");
        assert!(task.await.unwrap().is_ok());
        release.notify_one();
        assert!(read.await.unwrap().is_err());
        assert!(
            f.call("preset_read", json!({"id":"/styles/extra.typ"}))
                .await
                .is_ok()
        );
        let id = f.bridge.helper.inner.lock().unwrap().sessions["app"]
            .request
            .as_ref()
            .unwrap()
            .request_id
            .clone();
        f.bridge.helper.inner.lock().unwrap().requests.clear();
        f.bridge
            .helper
            .revoke_attachment("conversation", &id, "@presets")
            .unwrap();
        assert!(
            f.call("preset_read", json!({"id":"/styles/extra.typ"}))
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn in_app_approval_own_receipt_cannot_publish_after_followup() {
        let f = Fixture::new(3);
        f.install(&[0]);
        let (task, event) = approval(&f, "pages").await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        f.bridge.helper.inner.lock().unwrap().before_return =
            Some((entered.clone(), release.clone()));
        answer(&f, &event, "request");
        entered.notified().await;
        f.install(&[1]);
        f.bridge.helper.inner.lock().unwrap().before_return = None;
        release.notify_one();
        assert!(task.await.unwrap().is_err());
        assert!(
            !f.bridge.helper.inner.lock().unwrap().sessions["app"]
                .request
                .as_ref()
                .unwrap()
                .pages
                .contains_key(&f.pages[2])
        );
    }
    #[tokio::test]
    async fn in_app_approval_deny_and_changes_during_pending() {
        for change in ["deny", "followup", "revoke", "notebook", "end"] {
            let f = Fixture::new(3);
            f.install(&[0]);
            let (task, event) = approval(&f, "pages").await;
            match change {
                "followup" => {
                    f.install(&[1]);
                }
                "notebook" => f.bridge.helper.revoke_notebook(&f.root.to_string_lossy()),
                "end" => f.bridge.helper.revoke_conversation("conversation"),
                "revoke" => f
                    .bridge
                    .helper
                    .inner
                    .lock()
                    .unwrap()
                    .sessions
                    .get_mut("app")
                    .unwrap()
                    .revoke_request(),
                _ => {}
            }
            answer(
                &f,
                &event,
                if change == "deny" { "deny" } else { "request" },
            );
            assert!(task.await.unwrap().is_err(), "{change}");
            if change == "deny" {
                assert!(
                    f.call("read_page", json!({"page":f.pages[0]}))
                        .await
                        .is_ok()
                );
            }
        }
    }
    const TIME: &str = "2026-11-04T10:00:00Z";
    struct Fixture {
        _temp: tempfile::TempDir,
        root: std::path::PathBuf,
        pages: Vec<String>,
        bridge: bridge::Bridge,
    }
    impl Fixture {
        fn new(count: usize) -> Self {
            let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../fixtures/notebooks/minimal");
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("book");
            std::fs::create_dir_all(root.join("references")).unwrap();
            std::fs::copy(
                fixture.join("references/lecture.pdf"),
                root.join("references/lecture.pdf"),
            )
            .unwrap();
            storage::create_notebook(&root, &storage::open_notebook(&fixture).unwrap()).unwrap();
            let root = root.canonicalize().unwrap();
            let mut pages = vec!["page-001".into()];
            std::fs::create_dir_all(root.join("styles")).unwrap();
            std::fs::write(
                root.join("styles/context.typ"),
                "#let preset(body, rhythm: 16pt) = body\n// GRANTED_PRESET",
            )
            .unwrap();
            for i in 1..count {
                let mut b =
                    storage::create_page(&root, TIME, &storage::PagePosition::Last, None, None)
                        .unwrap();
                let mut fields = storage::open_page(&root, "page-001").unwrap().page.objects[0]
                    .fields()
                    .clone();
                fields.id = format!("page-text-{i}");
                fields.group_id = None;
                fields.x = 0.;
                fields.y = 0.;
                fields.reading_order = 0;
                let path = format!("blocks/context-{i}.typ");
                b.page.objects.push(PageObject::PageTypst {
                    fields: fields.clone(),
                    source_path: path.clone(),
                });
                b.page.reading_order.push(fields.id);
                b.blocks.push(storage::StoredFile { path, bytes:format!("#import \"/styles/context.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n\n= SOURCE_{i}\nAttached equation $F = m a$").into_bytes() });
                storage::save_notebook(&root, &b).unwrap();
                pages.push(b.page.id);
            }
            let roots = Arc::new(crate::workspace::AllowedRoots::default());
            crate::workspace::allow_root(&roots, &root).unwrap();
            let helper = Helper::default();
            let mut session = bridge::new_session("app".into());
            session.info.approved = true;
            session.conversation = Some("conversation".into());
            helper
                .inner
                .lock()
                .unwrap()
                .sessions
                .insert("app".into(), session);
            Self {
                _temp: temp,
                root,
                pages,
                bridge: bridge::Bridge {
                    helper,
                    app: None,
                    id: "app".into(),
                    roots,
                    histories: Default::default(),
                },
            }
        }
        fn intent(&self, pages: &[usize]) -> RequestContextIntent {
            RequestContextIntent {
                share_page_names: false,
                cells: None,
                destination: RequestDestination {
                    root: self.root.to_string_lossy().into_owned(),
                    page_id: self.pages[0].clone(),
                },
                selection: None,
                page_ids: pages.iter().map(|i| self.pages[*i].clone()).collect(),
                presets: false,
                intent_version: "1".into(),
            }
        }
        fn capture(&self, intent: RequestContextIntent) -> Result<CapturedRequestGrant, UiProblem> {
            snapshot::capture_locked(
                &self.bridge.roots,
                &self.bridge.histories,
                intent,
                Usage::Detailed,
            )
        }
        fn install(&self, pages: &[usize]) -> String {
            let mut g = self.capture(self.intent(pages)).unwrap();
            g.conversation_id = "conversation".into();
            let id = g.request_id.clone();
            let mut s = self.bridge.helper.inner.lock().unwrap();
            s.requests
                .retain(|_, g| g.conversation_id != "conversation");
            s.requests.insert(id.clone(), g.clone());
            s.sessions.get_mut("app").unwrap().install_request(g);
            s.generation += 1;
            id
        }
        async fn call(&self, tool: &str, args: Value) -> Result<Value, String> {
            let content = self
                .bridge
                .run(tool, args, &mut Default::default(), &mut String::new())
                .await?;
            serde_json::to_value(content).map_err(|e| e.to_string())
        }
        fn proposal_args(&self) -> Value {
            let s = self.bridge.helper.inner.lock().unwrap();
            let g = s.sessions["app"].request.as_ref().unwrap();
            json!({"page":self.pages[0],"proposalBase":proposals::base_to_wire(&g.destination_base),"items":[{"source":"A reviewed summary","widthPt":180,"placement":"below_selection"}],"summary":"Summary"})
        }
        fn hook(&self) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
            let ready = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            self.bridge.helper.inner.lock().unwrap().before_return =
                Some((ready.clone(), release.clone()));
            (ready, release)
        }
    }

    #[test]
    fn edge_crop_with_same_page_and_presets_is_clipped() {
        let f = Fixture::new(1);
        let mut intent = f.intent(&[0]);
        intent.presets = true;
        intent.selection = Some(SharePayload {
            root: intent.destination.root.clone(),
            page_id: f.pages[0].clone(),
            object_ids: vec![],
            stroke_ids: vec!["stroke-001".into()],
            crop_pt: Rect {
                x: -2.,
                y: -3.,
                width: 10000.,
                height: 10000.,
            },
            session_ids: None,
        });
        let grant = f.capture(intent.clone()).unwrap();
        assert_eq!(grant.anchor.crop_pt.x, 0.);
        assert_eq!(
            grant.anchor.crop_pt.width,
            grant.destination.page.geometry.width_pt
        );
        assert!(grant.pages.contains_key(&f.pages[0]));
        assert!(grant.selection.is_some());
        intent.selection.as_mut().unwrap().crop_pt.x = 10000.;
        assert_eq!(
            f.capture(intent.clone()).err().unwrap().code,
            "selection_crop_invalid"
        );
        intent.selection.as_mut().unwrap().crop_pt.x = f64::NAN;
        assert_eq!(
            f.capture(intent.clone()).err().unwrap().code,
            "selection_crop_invalid"
        );
        intent.selection.as_mut().unwrap().crop_pt = Rect {
            x: 0.,
            y: 0.,
            width: 20.,
            height: 20.,
        };
        intent.selection.as_mut().unwrap().stroke_ids = vec!["missing".into()];
        assert_eq!(
            f.capture(intent.clone()).err().unwrap().code,
            "selection_changed"
        );
        intent.selection.as_mut().unwrap().stroke_ids = vec!["stroke-001".into(); 10001];
        assert_eq!(f.capture(intent).err().unwrap().code, "selection_too_large");
    }

    /// AI helper step 4 wave 3: reviewed edits of existing Page text (contract §4, §10).
    mod page_text {
        use super::*;
        use sha2::Digest;
        const HEADER: &str = "#import \"/styles/context.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n";

        fn source_path(i: usize) -> String {
            format!("blocks/context-{i}.typ")
        }
        impl Fixture {
            fn write_page_text(&self, i: usize, source: &str) {
                let mut b = storage::open_page(&self.root, &self.pages[i]).unwrap();
                b.blocks
                    .iter_mut()
                    .find(|f| f.path == source_path(i))
                    .unwrap()
                    .bytes = source.as_bytes().to_vec();
                storage::save_notebook(&self.root, &b).unwrap();
            }
            /// Attach page `i` and make it the request destination, as Summarize/Page text does.
            fn attach_destination(&self, i: usize) -> Value {
                let mut intent = self.intent(&[i]);
                intent.destination.page_id = self.pages[i].clone();
                let mut g = self.capture(intent).unwrap();
                g.conversation_id = "conversation".into();
                let base = proposals::base_to_wire(&g.pages[&self.pages[i]].base);
                let mut s = self.bridge.helper.inner.lock().unwrap();
                s.requests.insert(g.request_id.clone(), g.clone());
                s.sessions.get_mut("app").unwrap().install_request(g);
                s.generation += 1;
                base
            }
            fn page_text_args(&self, i: usize, base: &Value, body: &str) -> Value {
                json!({"page":self.pages[i],"pageTextId":format!("page-text-{i}"),"proposalBase":base,"source":body,"summary":"Clarify the definitions","assumptions":["Constant mass"]})
            }
            async fn propose_page_text(&self, i: usize, body: &str) -> proposals::Proposal {
                let base = self.attach_destination(i);
                let reply = self
                    .call(
                        "propose_page_text_edit",
                        self.page_text_args(i, &base, body),
                    )
                    .await
                    .unwrap();
                let id = serde_json::from_str::<Value>(reply[0]["text"].as_str().unwrap()).unwrap()
                    ["id"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                self.bridge.helper.inner.lock().unwrap().sessions["app"].proposals[&id].clone()
            }
            fn apply_page_text(
                &self,
                i: usize,
                p: &proposals::Proposal,
                history: &mut storage::NotebookHistory,
            ) -> Result<storage::HistoryResult, storage::StorageError> {
                proposals::insert(
                    &self.bridge.helper,
                    None,
                    &self.root,
                    &self.pages[i],
                    history,
                    proposals::ProposalInsert {
                        proposal_id: p.id.clone(),
                        proposal_version: p.version,
                    },
                )
            }
            fn page_text_bytes(&self, i: usize) -> Vec<u8> {
                std::fs::read(self.root.join(source_path(i))).unwrap()
            }
            fn stored(&self, id: &str) -> proposals::Proposal {
                self.bridge.helper.inner.lock().unwrap().sessions["app"].proposals[id].clone()
            }
        }
        /// Every file in the notebook directory, to prove that review writes nothing.
        pub(super) fn tree(
            root: &std::path::Path,
        ) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
            let mut out = std::collections::BTreeMap::new();
            let mut stack = vec![root.to_path_buf()];
            while let Some(dir) = stack.pop() {
                for entry in std::fs::read_dir(dir).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        stack.push(path);
                    } else {
                        out.insert(path.clone(), std::fs::read(&path).unwrap());
                    }
                }
            }
            out
        }
        fn history(f: &Fixture, i: usize) -> storage::NotebookHistory {
            let mut history = storage::NotebookHistory::default();
            storage::observe_page(&f.root, &f.pages[i], &mut history).unwrap();
            history
        }

        #[tokio::test]
        async fn function_preamble_default_and_shared_presets_compile_like_page_text() {
            let f = Fixture::new(2);
            // Function form: the preset wraps the body.
            std::fs::write(
                f.root.join("styles/context.typ"),
                "#let preset(body, rhythm: 16pt) = { set text(fill: rgb(\"#aa0011\")); body }",
            )
            .unwrap();
            let p = f.propose_page_text(1, "= Function\nBody").await;
            assert_eq!(p.managed_header.as_deref(), Some(HEADER));
            assert!(
                p.items[0].svg.contains("#aa0011"),
                "function preset applied"
            );
            // Preamble form: no preset function, the module is inlined before the body.
            std::fs::write(
                f.root.join("styles/context.typ"),
                "#show heading: set text(fill: rgb(\"#00aa22\"))",
            )
            .unwrap();
            let p = f.propose_page_text(1, "= Preamble\nBody").await;
            assert!(
                p.items[0].svg.contains("#00aa22"),
                "preamble preset applied"
            );
            // The notebook default preset is an ordinary managed import of styles/default.typ.
            std::fs::write(
                f.root.join("styles/default.typ"),
                "#let preset(body, rhythm: 16pt) = { set text(fill: rgb(\"#3300bb\")); body }",
            )
            .unwrap();
            let default_header = "#import \"/styles/default.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\n";
            f.write_page_text(1, &format!("{default_header}= Default\nBody"));
            let p = f.propose_page_text(1, "= Default\nRevised").await;
            assert_eq!(p.managed_header.as_deref(), Some(default_header));
            assert!(p.items[0].svg.contains("#3300bb"), "default preset applied");
            // Unmanaged Page text uses the notebook's shared style and is reviewed whole.
            std::fs::write(
                f.root.join("style.typ"),
                "#show heading: set text(fill: rgb(\"#cc5500\"))",
            )
            .unwrap();
            f.write_page_text(1, "= Shared\nBody");
            let p = f.propose_page_text(1, "= Shared\nRevised").await;
            assert_eq!(p.managed_header, None);
            assert_eq!(p.items[0].before_source.as_deref(), Some("= Shared\nBody"));
            assert!(p.items[0].svg.contains("#cc5500"), "shared style applied");
        }

        #[tokio::test]
        async fn preview_uses_full_page_geometry_not_block_width() {
            let f = Fixture::new(2);
            let p = f.propose_page_text(1, "= Geometry\nBody").await;
            let page = storage::open_page(&f.root, &f.pages[1]).unwrap();
            let (w, h) = (page.page.geometry.width_pt, page.page.geometry.height_pt);
            assert_eq!((p.items[0].width_pt, p.items[0].height_pt), (w, h));
            for svg in [&p.items[0].svg, p.items[0].before_svg.as_ref().unwrap()] {
                assert!(svg.contains(&format!("viewBox=\"0 0 {w} {h}\"")));
            }
            let export = snapshot::export(&page).unwrap();
            let (x, y, width) = goodtype_typst::export::page_text_preview_geometry(&export);
            assert!(p.items[0].svg.contains(&format!(
                "translate({},{})",
                x - goodtype_typst::PREVIEW_PAD_PT,
                y - goodtype_typst::PREVIEW_PAD_PT
            )));
            assert!(
                width > 300.0,
                "page text width, not a movable-block heuristic"
            );
            assert!(!p.overflow);
            assert_eq!(p.page_number, 2);
            assert_eq!(p.kind, "page_text_edit");
        }

        #[tokio::test]
        async fn managed_header_with_comments_and_crlf_is_preserved_exactly_through_undo_redo() {
            let f = Fixture::new(2);
            let header = "// keep this note\r\n#import \"/styles/context.typ\": preset\r\n// preset comment\r\n#show: preset.with(rhythm: goodtype_rhythm)\r\n";
            f.write_page_text(1, &format!("{header}= Old\r\nBody"));
            let original = f.page_text_bytes(1);
            let p = f.propose_page_text(1, "= New\r\nRevised body").await;
            assert_eq!(p.managed_header.as_deref(), Some(header));
            assert_eq!(p.items[0].before_source.as_deref(), Some("= Old\r\nBody"));
            let objects = storage::open_page(&f.root, &f.pages[1]).unwrap().page;
            let mut history = history(&f, 1);
            f.apply_page_text(1, &p, &mut history).unwrap();
            let applied = format!("{header}= New\r\nRevised body").into_bytes();
            assert_eq!(f.page_text_bytes(1), applied);
            // Target identity, transform and reading order are untouched.
            let after = storage::open_page(&f.root, &f.pages[1]).unwrap().page;
            assert_eq!(
                serde_json::to_value(&after.objects).unwrap(),
                serde_json::to_value(&objects.objects).unwrap()
            );
            assert_eq!(after.reading_order, objects.reading_order);
            // One undo restores the exact original bytes; one redo the exact reviewed bytes.
            storage::undo_notebook(&f.root, &mut history).unwrap();
            assert_eq!(f.page_text_bytes(1), original);
            storage::redo_notebook(&f.root, &mut history).unwrap();
            assert_eq!(f.page_text_bytes(1), applied);
        }

        #[tokio::test]
        async fn header_changes_removal_or_addition_are_refused() {
            let f = Fixture::new(2);
            let base = f.attach_destination(1);
            for body in [
                format!("{HEADER}= Replaced header"),
                "#import \"/styles/other.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\nBody".into(),
                "// sneaky\n#import \"/styles/context.typ\": preset\n#show: preset.with(rhythm: goodtype_rhythm)\nBody".into(),
            ] {
                let error = f
                    .call("propose_page_text_edit", f.page_text_args(1, &base, &body))
                    .await
                    .unwrap_err();
                assert!(error.contains("managed header"), "{error}");
            }
            // Unmanaged Page text cannot gain a managed header through this tool.
            f.write_page_text(1, "Plain body");
            let base = f.attach_destination(1);
            let error = f
                .call(
                    "propose_page_text_edit",
                    f.page_text_args(1, &base, &format!("{HEADER}Body")),
                )
                .await
                .unwrap_err();
            assert!(error.contains("managed header"), "{error}");
            // A header without a trailing newline stays byte-identical; the body starts a new line.
            assert_eq!(
                proposals::page_text_source(Some(HEADER.trim_end()), "Body").unwrap(),
                format!("{}\nBody", HEADER.trim_end())
            );
        }

        #[tokio::test]
        async fn review_edit_recompiles_advances_version_and_keeps_last_good_preview() {
            let f = Fixture::new(2);
            let p = f.propose_page_text(1, "= First\nBody").await;
            let before_files = tree(&f.root);
            let invalid = proposals::edit(
                &f.bridge.helper,
                None,
                &p.id,
                p.version,
                vec!["#undefined_function(".into()],
                false,
            )
            .await
            .err()
            .unwrap();
            let diagnostics: Value = serde_json::from_str(&invalid).unwrap();
            assert_eq!(diagnostics[0]["line"], 1, "line maps to the editable body");
            let kept = f.stored(&p.id);
            assert_eq!((kept.version, &kept.items[0].svg), (1, &p.items[0].svg));
            assert_eq!(kept.items[0].source, "= First\nBody");
            let edited = proposals::edit(
                &f.bridge.helper,
                None,
                &p.id,
                p.version,
                vec!["= Second\nBody".into()],
                false,
            )
            .await
            .unwrap();
            assert_eq!(edited.version, 2);
            assert_eq!(
                edited.items[0].hash,
                format!(
                    "{:x}",
                    sha2::Sha256::digest(format!("{HEADER}= Second\nBody").as_bytes())
                )
            );
            assert_ne!(edited.items[0].svg, p.items[0].svg);
            assert_eq!(f.stored(&p.id).version, 2);
            // Review, failed compile and edit write nothing.
            assert_eq!(tree(&f.root), before_files);
            // Apply binds to the reviewed version.
            let mut history = history(&f, 1);
            assert!(f.apply_page_text(1, &p, &mut history).is_err());
            f.apply_page_text(1, &edited, &mut history).unwrap();
            assert_eq!(
                f.page_text_bytes(1),
                format!("{HEADER}= Second\nBody").into_bytes()
            );
        }

        #[tokio::test]
        async fn invalid_proposed_source_creates_no_candidate_and_maps_lines_to_body() {
            let f = Fixture::new(2);
            let base = f.attach_destination(1);
            let error = f
                .call(
                    "propose_page_text_edit",
                    f.page_text_args(1, &base, "Fine\n#undefined_function("),
                )
                .await
                .unwrap_err();
            let diagnostics: Value = serde_json::from_str(&error).unwrap();
            assert_eq!(diagnostics[0]["line"], 2);
            assert!(
                f.bridge.helper.inner.lock().unwrap().sessions["app"]
                    .proposals
                    .is_empty()
            );
        }

        #[tokio::test]
        async fn no_writes_before_apply_unrelated_edits_pass_and_duplicate_apply_is_refused() {
            let f = Fixture::new(2);
            let before = tree(&f.root);
            let p = f.propose_page_text(1, "= Reviewed\nBody").await;
            assert_eq!(tree(&f.root), before, "proposing writes nothing");
            // An unrelated movable block added later does not stale Page text.
            let mut page = storage::open_page(&f.root, &f.pages[1]).unwrap();
            let mut block = storage::open_page(&f.root, "page-001")
                .unwrap()
                .page
                .objects[0]
                .clone();
            block.fields_mut().id = "unrelated".into();
            block.fields_mut().group_id = None;
            block.fields_mut().reading_order = page.page.reading_order.len() as u32;
            if let PageObject::Typst { source_path, .. } = &mut block {
                *source_path = "blocks/unrelated.typ".into();
            }
            page.page.reading_order.push("unrelated".into());
            page.page.objects.push(block);
            page.blocks.push(storage::StoredFile {
                path: "blocks/unrelated.typ".into(),
                bytes: b"Unrelated".to_vec(),
            });
            storage::save_notebook(&f.root, &page).unwrap();
            let mut history = history(&f, 1);
            f.apply_page_text(1, &p, &mut history).unwrap();
            assert_eq!(f.stored(&p.id).status, "inserted");
            let applied = f.page_text_bytes(1);
            assert!(f.apply_page_text(1, &p, &mut history).is_err(), "duplicate");
            assert_eq!(f.page_text_bytes(1), applied);
            assert!(
                storage::open_page(&f.root, &f.pages[1])
                    .unwrap()
                    .page
                    .objects
                    .iter()
                    .any(|o| o.fields().id == "unrelated")
            );
        }

        #[tokio::test]
        async fn stale_dependencies_refuse_without_writing_and_name_the_change() {
            for change in [
                "source",
                "geometry",
                "paper",
                "preset",
                "shared_style",
                "removed_target",
                "import",
            ] {
                let f = Fixture::new(2);
                std::fs::create_dir_all(f.root.join("lib")).unwrap();
                std::fs::write(f.root.join("lib/defs.typ"), "#let g = 9.81").unwrap();
                f.write_page_text(
                    1,
                    &format!("{HEADER}#import \"/lib/defs.typ\": g\n= Old\n$g$"),
                );
                let p = f
                    .propose_page_text(1, "#import \"/lib/defs.typ\": g\n= New\n$g$")
                    .await;
                assert!(
                    p.dependencies.contains(&"/lib/defs.typ".to_owned()),
                    "{change}"
                );
                let mut history = history(&f, 1);
                let mut page = storage::open_page(&f.root, &f.pages[1]).unwrap();
                match change {
                    "source" => {
                        f.write_page_text(1, &format!("{HEADER}= Edited elsewhere"));
                    }
                    "geometry" => {
                        page.page.geometry.width_pt += 10.;
                        storage::save_notebook(&f.root, &page).unwrap();
                    }
                    "paper" => {
                        page.page.background = goodtype_core::PageBackground::Plain {
                            color: "#fff8e0".into(),
                        };
                        storage::save_notebook(&f.root, &page).unwrap();
                    }
                    "preset" => std::fs::write(
                        f.root.join("styles/context.typ"),
                        "#let preset(body, rhythm: 16pt) = strong(body)",
                    )
                    .unwrap(),
                    "shared_style" => {
                        std::fs::write(f.root.join("style.typ"), "#set text(size: 14pt)").unwrap()
                    }
                    "removed_target" => {
                        page.page.objects.retain(|o| o.fields().id != "page-text-1");
                        page.page.reading_order.retain(|id| id != "page-text-1");
                        page.blocks.retain(|b| b.path != source_path(1));
                        storage::save_notebook(&f.root, &page).unwrap();
                    }
                    _ => std::fs::write(f.root.join("lib/defs.typ"), "#let g = 10").unwrap(),
                }
                let files = tree(&f.root);
                let error = f.apply_page_text(1, &p, &mut history).unwrap_err();
                assert!(
                    matches!(error, storage::StorageError::ProposalStale(_)),
                    "{change}: {error}"
                );
                assert_eq!(tree(&f.root), files, "{change}: nothing written");
                let stale = f.stored(&p.id);
                assert_eq!(stale.status, "stale", "{change}");
                let named = serde_json::to_value(&stale.changed).unwrap();
                let expected = match change {
                    "source" | "removed_target" => "pageText",
                    "geometry" | "paper" => "page",
                    "preset" => "preset",
                    "shared_style" => "sharedStyle",
                    _ => "file",
                };
                assert!(
                    named
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|c| c["dependency"] == expected),
                    "{change}: {named}"
                );
                assert!(f.apply_page_text(1, &p, &mut history).is_err(), "{change}");
            }
        }

        /// proposal_status reports the same Page text staleness Apply finds, before any Apply.
        #[tokio::test]
        async fn status_reports_page_guidance_and_import_staleness_like_apply() {
            for (change, expected) in [("geometry", "page"), ("paper", "page"), ("import", "file")]
            {
                let f = Fixture::new(2);
                std::fs::create_dir_all(f.root.join("lib")).unwrap();
                std::fs::write(f.root.join("lib/defs.typ"), "#let g = 9.81").unwrap();
                f.write_page_text(
                    1,
                    &format!("{HEADER}#import \"/lib/defs.typ\": g\n= Old\n$g$"),
                );
                let p = f
                    .propose_page_text(1, "#import \"/lib/defs.typ\": g\n= New\n$g$")
                    .await;
                assert_eq!(
                    f.bridge.proposal_status(&p.id).await.unwrap()["status"],
                    "pending"
                );
                let mut page = storage::open_page(&f.root, &f.pages[1]).unwrap();
                match change {
                    "geometry" => {
                        page.page.geometry.width_pt += 10.;
                        storage::save_notebook(&f.root, &page).unwrap();
                    }
                    "paper" => {
                        page.page.background = goodtype_core::PageBackground::Plain {
                            color: "#fff8e0".into(),
                        };
                        storage::save_notebook(&f.root, &page).unwrap();
                    }
                    _ => std::fs::write(f.root.join("lib/defs.typ"), "#let g = 10").unwrap(),
                }
                let files = tree(&f.root);
                let status = f.bridge.proposal_status(&p.id).await.unwrap();
                assert_eq!(status["status"], "stale", "{change}");
                assert!(
                    status["changed"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|c| c["dependency"] == expected),
                    "{change}: {status}"
                );
                assert_eq!(tree(&f.root), files, "{change}: status writes nothing");
            }
        }

        #[tokio::test]
        async fn external_conflict_refuses_without_writing() {
            let f = Fixture::new(2);
            let p = f.propose_page_text(1, "= Reviewed\nBody").await;
            let mut history = history(&f, 1);
            // An outside program rewrites the page file after Goodtype observed it.
            let page_file = f.root.join(format!("pages/{}.json", f.pages[1]));
            let mut json: Value =
                serde_json::from_slice(&std::fs::read(&page_file).unwrap()).unwrap();
            json["revision"] = json!(json["revision"].as_u64().unwrap() + 7);
            std::fs::write(&page_file, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
            let files = tree(&f.root);
            assert!(f.apply_page_text(1, &p, &mut history).is_err());
            assert_eq!(tree(&f.root), files);
            assert_eq!(f.stored(&p.id).status, "pending");
        }

        #[tokio::test]
        async fn overflow_is_reported_and_refused_on_apply() {
            let f = Fixture::new(2);
            let long = (0..120)
                .map(|i| format!("Line {i} of a long derivation.\n\n"))
                .collect::<String>();
            let p = f.propose_page_text(1, &long).await;
            assert!(p.overflow);
            let before = f.page_text_bytes(1);
            let mut history = history(&f, 1);
            assert!(f.apply_page_text(1, &p, &mut history).is_err());
            assert_eq!(f.page_text_bytes(1), before);
        }

        #[tokio::test]
        async fn retained_history_refusal_writes_nothing() {
            let f = Fixture::new(2);
            let p = f.propose_page_text(1, "= Reviewed\nBody").await;
            // Unrelated material larger than the 128 MiB undo budget, added after review.
            let mut page = storage::open_page(&f.root, &f.pages[1]).unwrap();
            let template = storage::open_page(&f.root, "page-001")
                .unwrap()
                .page
                .objects[0]
                .clone();
            for n in 0..130 {
                let mut block = template.clone();
                let id = format!("bulk-{n}");
                block.fields_mut().id = id.clone();
                block.fields_mut().group_id = None;
                block.fields_mut().reading_order = page.page.reading_order.len() as u32;
                if let PageObject::Typst { source_path, .. } = &mut block {
                    *source_path = format!("blocks/{id}.typ");
                }
                page.page.reading_order.push(id.clone());
                page.page.objects.push(block);
                page.blocks.push(storage::StoredFile {
                    path: format!("blocks/{id}.typ"),
                    bytes: vec![b'x'; goodtype_core::object::MAX_BLOCK_BYTES],
                });
            }
            storage::save_notebook(&f.root, &page).unwrap();
            let before = f.page_text_bytes(1);
            let mut history = history(&f, 1);
            assert!(matches!(
                f.apply_page_text(1, &p, &mut history),
                Err(storage::StorageError::HistoryNotRetained)
            ));
            assert_eq!(f.page_text_bytes(1), before);
            assert_eq!(f.stored(&p.id).status, "pending");
        }

        #[tokio::test]
        async fn authority_is_explicit_destination_page_text_only() {
            let f = Fixture::new(3);
            // No request at all.
            let error = f
                .call(
                    "propose_page_text_edit",
                    json!({"page":f.pages[1],"pageTextId":"page-text-1","proposalBase":{},"source":"B","summary":"S"}),
                )
                .await
                .unwrap_err();
            assert!(error.contains("not shared"), "{error}");
            // Attached as context, but the destination is another page.
            let base = {
                let id = f.install(&[1]);
                let s = f.bridge.helper.inner.lock().unwrap();
                proposals::base_to_wire(&s.requests[&id].pages[&f.pages[1]].base)
            };
            let error = f
                .call("propose_page_text_edit", f.page_text_args(1, &base, "B"))
                .await
                .unwrap_err();
            assert!(error.contains("destination"), "{error}");
            // Destination without being attached: a selection alone is not page authority.
            let mut intent = f.intent(&[2]);
            intent.destination.page_id = f.pages[1].clone();
            let mut g = f.capture(intent).unwrap();
            g.conversation_id = "conversation".into();
            let dest_base = proposals::base_to_wire(&g.destination_base);
            {
                let mut s = f.bridge.helper.inner.lock().unwrap();
                s.sessions.get_mut("app").unwrap().install_request(g);
                s.generation += 1;
            }
            let error = f
                .call(
                    "propose_page_text_edit",
                    f.page_text_args(1, &dest_base, "B"),
                )
                .await
                .unwrap_err();
            assert!(error.contains("attach"), "{error}");
            // Correct authority, but a movable block ID or a rounded base is refused.
            let base = f.attach_destination(1);
            let mut wrong = f.page_text_args(1, &base, "B");
            wrong["pageTextId"] = json!("typst-001");
            let error = f.call("propose_page_text_edit", wrong).await.unwrap_err();
            assert!(error.contains("propose_block_edit"), "{error}");
            let mut rounded = f.page_text_args(1, &base, "B");
            rounded["proposalBase"]["pageText"] = json!(1.0e19);
            assert!(f.call("propose_page_text_edit", rounded).await.is_err());
            let mut changed = f.page_text_args(1, &base, "B");
            changed["proposalBase"]["revision"] = json!("999");
            assert!(f.call("propose_page_text_edit", changed).await.is_err());
            assert!(
                f.bridge.helper.inner.lock().unwrap().sessions["app"]
                    .proposals
                    .is_empty()
            );
        }

        #[tokio::test]
        async fn revoking_during_compile_publishes_nothing_and_revoked_proposals_cannot_apply() {
            let f = Fixture::new(2);
            let base = f.attach_destination(1);
            let (ready, release) = f.hook();
            let args = f.page_text_args(1, &base, "= Reviewed");
            let call = f.bridge.propose("propose_page_text_edit", args);
            let revoke = async {
                ready.notified().await;
                f.bridge
                    .helper
                    .inner
                    .lock()
                    .unwrap()
                    .sessions
                    .get_mut("app")
                    .unwrap()
                    .request = None;
                release.notify_one();
            };
            let (result, ()) = tokio::join!(call, revoke);
            assert!(result.unwrap_err().contains("changed"));
            assert!(
                f.bridge.helper.inner.lock().unwrap().sessions["app"]
                    .proposals
                    .is_empty()
            );
            f.bridge.helper.inner.lock().unwrap().before_return = None;
            // A finished proposal whose request is later revoked cannot be applied.
            let p = f.propose_page_text(1, "= Reviewed").await;
            f.bridge
                .helper
                .inner
                .lock()
                .unwrap()
                .sessions
                .get_mut("app")
                .unwrap()
                .request = None;
            let before = f.page_text_bytes(1);
            let mut history = history(&f, 1);
            assert!(f.apply_page_text(1, &p, &mut history).is_err());
            assert_eq!(f.page_text_bytes(1), before);
        }
    }

    #[test]
    fn capture_refuses_cross_page_and_computational_dependencies_without_bytes() {
        let f = Fixture::new(2);
        for source in [
            "#include \"/blocks/equation.typ\"",
            "#image(\"/assets/diagram.svg\")",
            "#read(\"/calculations/private.ipynb\")",
            "#read(\"/calculations/outputs/private.txt\")",
            "#read(\"/support/private.ipynb\")",
        ] {
            for path in [
                "calculations/private.ipynb",
                "calculations/outputs/private.txt",
                "support/private.ipynb",
            ] {
                std::fs::create_dir_all(f.root.join(path).parent().unwrap()).unwrap();
                std::fs::write(f.root.join(path), "PRIVATE_BYTES").unwrap();
            }
            std::fs::write(f.root.join("blocks/context-1.typ"), source).unwrap();
            let error = f.capture(f.intent(&[1])).err().unwrap();
            assert_eq!(error.code, "context_dependencies_unavailable");
            assert!(error.params.is_empty());
        }
    }
    #[test]
    fn capture_preserves_preset_and_transitive_support_closure() {
        let f = Fixture::new(2);
        std::fs::create_dir_all(f.root.join("support")).unwrap();
        std::fs::write(f.root.join("support/outer.typ"), "#include \"inner.typ\"").unwrap();
        std::fs::write(f.root.join("support/inner.typ"), "Allowed support").unwrap();
        let path = f.root.join("blocks/context-1.typ");
        let source = std::fs::read_to_string(&path).unwrap();
        std::fs::write(path, format!("{source}\n#include \"/support/outer.typ\"")).unwrap();
        let grant = f.capture(f.intent(&[1])).unwrap();
        let files = &grant.pages[&f.pages[1]].files;
        for path in [
            "/styles/context.typ",
            "/support/outer.typ",
            "/support/inner.typ",
        ] {
            assert!(files.contains_key(path));
        }
    }
    #[tokio::test]
    async fn transcribe_attached_page_uses_full_strokes_but_destination_alone_is_private() {
        let f = Fixture::new(2);
        let mut intent = f.intent(&[0]);
        intent.destination.page_id = f.pages[1].clone();
        let mut captured = f.capture(intent).unwrap();
        captured.conversation_id = "conversation".into();
        f.bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .sessions
            .get_mut("app")
            .unwrap()
            .install_request(captured);
        assert_eq!(
            f.call("transcribe", json!({"page":f.pages[0],"kind":"math"}))
                .await
                .unwrap_err(),
            "no_window"
        );
        assert!(
            f.call("render", json!({"page":f.pages[0],"scale":0.1}))
                .await
                .unwrap()
                .to_string()
                .contains("image/png")
        );
        f.install(&[0]);
        // Math reaches recognition's window requirement, beyond stroke authorization.
        assert_eq!(
            f.call(
                "transcribe",
                json!({"page":f.pages[0],"kind":"math","strokeIds":["stroke-001"]})
            )
            .await
            .unwrap_err(),
            "no_window"
        );
        f.install(&[1]);
        assert!(
            f.call("transcribe", json!({"page":f.pages[0],"kind":"math"}))
                .await
                .unwrap_err()
                .contains("not shared")
        );
        assert!(
            f.call(
                "transcribe",
                json!({"page":f.pages[1],"kind":"math","strokeIds":["foreign"]})
            )
            .await
            .unwrap_err()
            .contains("foreign stroke")
        );
    }
    mod terminal_presets {
        use super::*;
        fn terminal() -> Fixture {
            let f = Fixture::new(2);
            let mut s = f.bridge.helper.inner.lock().unwrap();
            s.settings.usage = Usage::Detailed;
            let session = s.sessions.get_mut("app").unwrap();
            session.conversation = None;
            let share = SharePayload {
                root: f.root.to_string_lossy().into_owned(),
                page_id: f.pages[1].clone(),
                object_ids: vec!["page-text-1".into()],
                stroke_ids: vec![],
                crop_pt: Rect {
                    x: 0.,
                    y: 0.,
                    width: 200.,
                    height: 200.,
                },
                session_ids: None,
            };
            let info = ShareInfo {
                page_id: share.page_id.clone(),
                page_number: 2,
                object_count: 1,
                crop_pt: share.crop_pt,
                session_ids: vec!["app".into()],
                shared_at: now(),
            };
            s.share = Some((share, info));
            drop(s);
            std::fs::write(
                f.root.join("styles/private.typ"),
                "#set text(size: 13pt)\n// PRIVATE",
            )
            .unwrap();
            f
        }
        async fn approve(f: &Fixture, answer: &str) {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            f.bridge.helper.inner.lock().unwrap().scope_requests = Some(tx);
            let bridge = f.bridge.clone();
            let task = tokio::spawn(async move {
                bridge
                    .run(
                        "request_scope",
                        json!({"scope":{"kind":"presets"},"reason":"Review notebook styles"}),
                        &mut Default::default(),
                        &mut String::new(),
                    )
                    .await
            });
            let event = rx.recv().await.unwrap();
            assert_eq!(event["scope"], json!({"kind":"presets"}));
            assert!(event.get("conversationId").is_none());
            let (_, tx) = f
                .bridge
                .helper
                .inner
                .lock()
                .unwrap()
                .scopes
                .remove(event["requestId"].as_str().unwrap())
                .unwrap();
            tx.send(answer.into()).unwrap();
            task.await.unwrap().unwrap();
        }
        #[tokio::test]
        async fn terminal_presets_selection_once_session_and_isolation() {
            let f = terminal();
            let list = f.call("presets_list", json!({})).await.unwrap().to_string();
            assert!(list.contains("context.typ"));
            assert!(!list.contains("private.typ"));
            assert!(!list.contains("SOURCE_1"));
            assert!(
                f.call("preset_read", json!({"id":"styles/private.typ"}))
                    .await
                    .is_err()
            );
            approve(&f, "once").await;
            assert!(
                f.call("preset_read", json!({"id":"styles/private.typ"}))
                    .await
                    .unwrap()
                    .to_string()
                    .contains("PRIVATE")
            );
            assert!(
                f.call("preset_read", json!({"id":"styles/private.typ"}))
                    .await
                    .is_err()
            );
            approve(&f, "session").await;
            for _ in 0..2 {
                assert!(
                    f.call("preset_read", json!({"id":"styles/private.typ"}))
                        .await
                        .is_ok()
                );
            }
            let mut other = f.bridge.clone();
            other.id = "other".into();
            let mut session = bridge::new_session("other".into());
            session.info.approved = true;
            f.bridge
                .helper
                .inner
                .lock()
                .unwrap()
                .sessions
                .insert("other".into(), session);
            assert!(
                other
                    .run(
                        "preset_read",
                        json!({"id":"styles/private.typ"}),
                        &mut Default::default(),
                        &mut String::new()
                    )
                    .await
                    .is_err()
            );
        }
        #[tokio::test]
        async fn terminal_presets_once_preview_continues_to_ui_install_without_page_writes() {
            let f = terminal();
            approve(&f, "once").await;
            let before = page_text::tree(&f.root);
            let p = f.preset_proposal("#include \"private.typ\"").await;
            assert!(
                f.call("preset_read", json!({"id":"styles/private.typ"}))
                    .await
                    .is_err()
            );
            assert_eq!(page_text::tree(&f.root), before);
            let installed = f.preset_install(&p).unwrap();
            assert!(installed.wrote);
            assert!(!f.preset_install(&p).unwrap().wrote);
            let mut after = page_text::tree(&f.root);
            after.remove(&f.root.join(installed.path));
            assert_eq!(after, before);
            f.bridge.helper.inner.lock().unwrap().generation += 1;
            assert!(f.preset_install(&p).is_err());
        }
        #[tokio::test]
        async fn terminal_presets_page_once_and_capture_bounds() {
            let f = terminal();
            {
                let mut s = f.bridge.helper.inner.lock().unwrap();
                s.share.as_mut().unwrap().1.session_ids.clear();
                let session = s.sessions.get_mut("app").unwrap();
                session.scope_root = Some(f.root.to_string_lossy().into_owned());
                session.pages.insert(f.pages[1].clone(), true);
            }
            assert!(
                f.call("preset_read", json!({"id":"styles/context.typ"}))
                    .await
                    .is_ok()
            );
            assert!(
                f.call("preset_read", json!({"id":"styles/context.typ"}))
                    .await
                    .is_err()
            );
            {
                let mut s = f.bridge.helper.inner.lock().unwrap();
                s.share.as_mut().unwrap().1.session_ids.push("app".into());
            }
            for i in 0..257 {
                std::fs::write(
                    f.root.join(format!("styles/bounded-{i}.typ")),
                    "#set text(size:12pt)",
                )
                .unwrap();
            }
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            f.bridge.helper.inner.lock().unwrap().scope_requests = Some(tx);
            let bridge = f.bridge.clone();
            let task = tokio::spawn(async move {
                bridge
                    .run(
                        "request_scope",
                        json!({"scope":{"kind":"presets"},"reason":"Styles"}),
                        &mut Default::default(),
                        &mut String::new(),
                    )
                    .await
            });
            let event = rx.recv().await.unwrap();
            let (_, tx) = f
                .bridge
                .helper
                .inner
                .lock()
                .unwrap()
                .scopes
                .remove(event["requestId"].as_str().unwrap())
                .unwrap();
            tx.send("session".into()).unwrap();
            assert!(task.await.unwrap().is_err());
            assert!(
                f.bridge.helper.inner.lock().unwrap().sessions["app"]
                    .terminal_presets
                    .is_none()
            );
        }
        #[tokio::test]
        async fn terminal_presets_captured_reads_stale_dependencies_versions_and_collision() {
            let f = terminal();
            approve(&f, "session").await;
            std::fs::write(f.root.join("styles/private.typ"), "CHANGED_AFTER_CAPTURE").unwrap();
            let read = f
                .call("preset_read", json!({"id":"styles/private.typ"}))
                .await
                .unwrap()
                .to_string();
            assert!(read.contains("PRIVATE"));
            assert!(!read.contains("CHANGED_AFTER_CAPTURE"));
            let preview = f
                .call(
                    "preset_preview",
                    json!({"source":"#include \"private.typ\""}),
                )
                .await
                .unwrap();
            let preview: Value =
                serde_json::from_str(preview[0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(preview["rendered"], true);
            assert!(f.call("propose_preset", json!({"source":"#include \"private.typ\"","name":"Review","candidateHash":preview["candidateHash"]})).await.unwrap_err().contains("changed"));
            approve(&f, "session").await;
            let p = f.preset_proposal("#set text(size:12pt)").await;
            let before = page_text::tree(&f.root);
            let mut wrong = p.clone();
            wrong.version += 1;
            assert!(f.preset_install(&wrong).is_err());
            wrong = p.clone();
            wrong.preset.as_mut().unwrap().candidate_hash = "wrong".into();
            assert!(f.preset_install(&wrong).is_err());
            assert_eq!(page_text::tree(&f.root), before);
            let target = f
                .root
                .join("styles")
                .join(&p.preset.as_ref().unwrap().filename);
            std::fs::write(&target, "ORIGINAL_COLLISION").unwrap();
            assert!(f.preset_install(&p).is_err());
            assert_eq!(
                std::fs::read_to_string(&target).unwrap(),
                "ORIGINAL_COLLISION"
            );
            // A new approved preset request invalidates the previous review epoch.
            approve(&f, "session").await;
            assert!(f.preset_install(&p).is_err());
        }
        #[tokio::test]
        async fn terminal_presets_revoke_during_preview_cannot_publish_or_install() {
            let f = terminal();
            approve(&f, "once").await;
            let (ready, release) = f.hook();
            let bridge = f.bridge.clone();
            let task = tokio::spawn(async move {
                bridge
                    .run(
                        "preset_preview",
                        json!({"source":"#set text(size:12pt)"}),
                        &mut Default::default(),
                        &mut String::new(),
                    )
                    .await
            });
            ready.notified().await;
            f.bridge.helper.inner.lock().unwrap().generation += 1;
            release.notify_one();
            assert!(task.await.unwrap().unwrap_err().contains("changed"));
            f.bridge.helper.inner.lock().unwrap().before_return = None;
            assert!(
                f.call(
                    "propose_preset",
                    json!({"source":"#set text(size:12pt)","name":"Old","candidateHash":"old"})
                )
                .await
                .is_err()
            );
        }
        #[tokio::test]
        async fn terminal_presets_pending_allow_cannot_restore_replaced_share() {
            let f = terminal();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            f.bridge.helper.inner.lock().unwrap().scope_requests = Some(tx);
            let bridge = f.bridge.clone();
            let task = tokio::spawn(async move {
                bridge
                    .run(
                        "request_scope",
                        json!({"scope":{"kind":"presets"},"reason":"Styles"}),
                        &mut Default::default(),
                        &mut String::new(),
                    )
                    .await
            });
            let event = rx.recv().await.unwrap();
            let (_, tx) = {
                let mut s = f.bridge.helper.inner.lock().unwrap();
                s.generation += 1;
                s.scopes
                    .remove(event["requestId"].as_str().unwrap())
                    .unwrap()
            };
            tx.send("session".into()).unwrap();
            assert!(task.await.unwrap().unwrap_err().contains("changed"));
            assert!(
                f.bridge.helper.inner.lock().unwrap().sessions["app"]
                    .terminal_presets
                    .is_none()
            );
        }
    }

    impl Fixture {
        async fn preset_proposal(&self, source: &str) -> proposals::Proposal {
            let preview = self
                .call("preset_preview", json!({"source":source}))
                .await
                .unwrap();
            let preview: Value =
                serde_json::from_str(preview[0]["text"].as_str().unwrap()).unwrap();
            let result=self.call("propose_preset",json!({"source":source,"name":"Reviewed notes","candidateHash":preview["candidateHash"],"sourceUrl":"https://example.invalid/display-only"})).await.unwrap();
            let result: Value = serde_json::from_str(result[0]["text"].as_str().unwrap()).unwrap();
            self.bridge.helper.inner.lock().unwrap().sessions["app"].proposals
                [result["id"].as_str().unwrap()]
            .clone()
        }
        fn preset_install(
            &self,
            p: &proposals::Proposal,
        ) -> Result<super::preset_install::InstallResult, String> {
            let input = super::preset_install::InstallPreset {
                proposal_id: p.id.clone(),
                proposal_version: p.version,
                candidate_hash: p.preset.as_ref().unwrap().candidate_hash.clone(),
                session_id: p.session_id.clone(),
                root: p.root.clone(),
            };
            crate::notebook::with_notebook(&self.bridge.histories, || {
                super::preset_install::install(
                    &self.bridge.helper,
                    None,
                    &self.root.canonicalize().unwrap(),
                    input,
                )
            })
        }
    }
    #[tokio::test]
    async fn preset_install_transaction_is_reviewed_new_only_and_idempotent() {
        let f = Fixture::new(1);
        f.install(&[0]);
        let before = page_text::tree(&f.root);
        let p = f.preset_proposal("#set text(size: 12pt)").await;
        assert_eq!(
            page_text::tree(&f.root),
            before,
            "preview/propose must never write"
        );
        assert_eq!(p.kind, "preset_install");
        assert!(p.items.is_empty());
        let first = f.preset_install(&p).unwrap();
        assert!(first.wrote);
        assert_eq!(first.status, "installed");
        let mut after = page_text::tree(&f.root);
        assert!(after.remove(&f.root.join(&first.path)).is_some());
        assert_eq!(after, before, "no page/default/history changes");
        let again = f.preset_install(&p).unwrap();
        assert!(!again.wrote);
        assert_eq!(again.path, first.path);
        assert_eq!(
            std::fs::read_to_string(f.root.join(first.path)).unwrap(),
            p.preset.unwrap().source
        );
    }
    #[tokio::test]
    async fn preset_install_refuses_wrong_hash_version_owner_and_changed_dependencies() {
        let f = Fixture::new(1);
        f.install(&[0]);
        let p = f.preset_proposal("#set text(size: 12pt)").await;
        let before = page_text::tree(&f.root);
        for variant in 0..4 {
            let mut bad = p.clone();
            match variant {
                0 => bad.version += 1,
                1 => bad.preset.as_mut().unwrap().candidate_hash = "wrong".into(),
                2 => bad.session_id = "foreign".into(),
                _ => bad.root = f.root.join("other").to_string_lossy().into_owned(),
            }
            assert!(f.preset_install(&bad).is_err());
        }
        assert_eq!(page_text::tree(&f.root), before);
        let file = p.preset.as_ref().unwrap().files.keys().next().unwrap();
        std::fs::write(f.root.join(file.trim_start_matches('/')), "changed").unwrap();
        assert!(f.preset_install(&p).is_err());
        assert!(
            !f.root
                .join(format!("styles/{}", p.preset.as_ref().unwrap().filename))
                .exists()
        );
        assert_eq!(
            f.bridge.proposal_status(&p.id).await.unwrap()["status"],
            "stale"
        );
    }
    #[tokio::test]
    async fn preset_install_collision_and_identical_existing_are_explicit() {
        for exact in [false, true] {
            let f = Fixture::new(1);
            f.install(&[0]);
            let p = f.preset_proposal("#set text(size: 12pt)").await;
            let preset = p.preset.as_ref().unwrap();
            let target = f.root.join(format!("styles/{}", preset.filename));
            let bytes = if exact {
                preset.source.as_str()
            } else {
                "original unrelated content"
            };
            std::fs::write(&target, bytes).unwrap();
            let result = f.preset_install(&p);
            if exact {
                let result = result.unwrap();
                assert!(!result.wrote);
                assert_eq!(result.status, "already_installed");
            } else {
                assert!(result.is_err());
                assert_eq!(
                    f.bridge.helper.inner.lock().unwrap().sessions["app"].proposals[&p.id].status,
                    "pending"
                );
            }
            assert_eq!(std::fs::read_to_string(target).unwrap(), bytes);
        }
    }
    #[tokio::test]
    async fn preset_source_edits_invalidate_old_install_and_revocation_denies_install() {
        let f = Fixture::new(1);
        f.install(&[0]);
        let p = f.preset_proposal("#set text(size: 12pt)").await;
        let updated = proposals::edit(
            &f.bridge.helper,
            None,
            &p.id,
            p.version,
            vec!["#set text(size: 13pt)".into()],
            false,
        )
        .await
        .unwrap();
        assert_eq!(updated.version, p.version + 1);
        assert_ne!(
            updated.preset.as_ref().unwrap().candidate_hash,
            p.preset.as_ref().unwrap().candidate_hash
        );
        assert!(f.preset_install(&p).is_err());
        let mut bad = updated.clone();
        bad.preset.as_mut().unwrap().source = "changed".into();
        f.bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .sessions
            .get_mut("app")
            .unwrap()
            .request = None;
        assert!(f.preset_install(&updated).is_err());
        assert!(
            !f.root
                .join(format!("styles/{}", bad.preset.unwrap().filename))
                .exists()
        );
    }
    #[tokio::test]
    async fn preset_propose_requires_matching_preview_and_never_shadows_granted_candidate() {
        let f = Fixture::new(1);
        f.install(&[0]);
        assert!(f.call("propose_preset",json!({"source":"#set text(size: 12pt)","name":"Notes","candidateHash":"unreviewed"})).await.is_err());
        let preview = f
            .call("preset_preview", json!({"source":"#set text(size: 12pt)"}))
            .await
            .unwrap();
        let preview: Value = serde_json::from_str(preview[0]["text"].as_str().unwrap()).unwrap();
        assert!(f.call("propose_preset",json!({"source":"#set text(size: 13pt)","name":"Notes","candidateHash":preview["candidateHash"]})).await.is_err());
        let p = f.preset_proposal("#set text(size: 12pt)").await;
        assert!(
            proposals::edit(
                &f.bridge.helper,
                None,
                &p.id,
                p.version,
                vec!["#let broken =".into()],
                false
            )
            .await
            .is_err()
        );
        assert_eq!(
            f.bridge.helper.inner.lock().unwrap().sessions["app"].proposals[&p.id].version,
            p.version
        );
    }

    #[tokio::test]
    async fn preset_preview_uses_granted_world_and_returns_mcp_image() {
        let f = Fixture::new(1);
        f.install(&[0]);
        let before = page_text::tree(&f.root);
        let result = f
            .call(
                "preset_preview",
                json!({"source":"#set text(font: \"No such Goodtype font\")"}),
            )
            .await
            .unwrap();
        let content = result.as_array().unwrap();
        assert!(
            content
                .iter()
                .any(|c| c["type"] == "image" && c["mimeType"] == "image/png"),
            "{result}"
        );
        let text = content
            .iter()
            .filter_map(|c| c["text"].as_str())
            .collect::<String>();
        assert!(
            text.contains("candidateHash") && text.contains("preamble") && text.contains("font"),
            "{text}"
        );
        assert_eq!(page_text::tree(&f.root), before);
        let denied = f
            .call(
                "preset_preview",
                json!({"source":"#include \"/secret.typ\""}),
            )
            .await
            .unwrap();
        assert!(
            !denied
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["type"] == "image")
        );
        assert_eq!(page_text::tree(&f.root), before);
    }

    #[tokio::test]
    async fn preset_reads_are_request_local_and_revoke_without_changing_page_authority() {
        let f = Fixture::new(2);
        storage::write_typst_style(&f.root, "private.typ", "#let private = 17").unwrap();
        f.install(&[0]);
        assert!(
            f.call("preset_read", json!({"id":"styles/private.typ"}))
                .await
                .unwrap_err()
                .contains("not shared")
        );
        let mut intent = f.intent(&[0]);
        intent.presets = true;
        let mut g = f.capture(intent).unwrap();
        g.conversation_id = "conversation".into();
        let id = g.request_id.clone();
        {
            let mut state = f.bridge.helper.inner.lock().unwrap();
            state.requests.insert(id.clone(), g.clone());
            state.sessions.get_mut("app").unwrap().install_request(g);
            state.generation += 1;
        }
        assert!(
            f.call("preset_read", json!({"id":"styles/private.typ"}))
                .await
                .unwrap()
                .to_string()
                .contains("private = 17")
        );
        let listed = f.call("presets_list", json!({})).await.unwrap().to_string();
        assert!(listed.contains("private.typ"));
        f.bridge
            .helper
            .revoke_attachment("conversation", &id, "@presets")
            .unwrap();
        assert!(
            f.call("preset_read", json!({"id":"styles/private.typ"}))
                .await
                .unwrap_err()
                .contains("not shared")
        );
        assert!(
            f.call("read_page", json!({"page":f.pages[0]}))
                .await
                .is_ok()
        );
        f.install(&[0]);
        assert!(
            f.call("preset_read", json!({"id":"styles/private.typ"}))
                .await
                .is_err()
        );
    }

    #[test]
    fn request_capture_deduplicates_caps_refuses_missing_and_foreign_and_shares_python_cells() {
        let f = Fixture::new(10);
        assert_eq!(f.capture(f.intent(&[1, 1, 1])).unwrap().pages.len(), 1);
        assert_eq!(
            f.capture(f.intent(&(0..9).collect::<Vec<_>>()))
                .err()
                .unwrap()
                .code,
            "attachment_limit"
        );
        assert_eq!(
            f.capture(f.intent(&(0..8).collect::<Vec<_>>()))
                .unwrap()
                .pages
                .len(),
            8
        );
        let mut intent = f.intent(&[]);
        intent.page_ids = vec!["missing".into()];
        assert_eq!(
            f.capture(intent).err().unwrap().code,
            "context_page_unavailable"
        );
        let mut intent = f.intent(&[]);
        intent.destination.page_id = "../outside".into();
        assert_eq!(
            f.capture(intent).err().unwrap().code,
            "context_page_unavailable"
        );
        let mut intent = f.intent(&[]);
        intent.destination.root = f._temp.path().to_string_lossy().into_owned();
        assert_eq!(f.capture(intent).err().unwrap().code, "context_root_denied");
        let mut intent = f.intent(&[]);
        intent.presets = true;
        assert!(f.capture(intent).is_ok());
        let calc = storage::create_calculation_page(
            &f.root,
            TIME,
            &storage::PagePosition::Last,
            "Private Python",
            None,
        )
        .unwrap();
        // An attached Python page shares its cells, never its files or an ordinary page grant.
        let mut intent = f.intent(&[]);
        intent.page_ids.push(calc.page.id.clone());
        let grant = f.capture(intent).unwrap();
        assert!(!grant.pages.contains_key(&calc.page.id));
        assert!(grant.cells[&calc.page.id].include_outputs);
        assert!(!grant.cells[&calc.page.id].cells.is_empty());
        assert!(grant.files().keys().all(|path| !path.ends_with(".ipynb")));
        // Selections stay on ordinary pages.
        let mut intent = f.intent(&[]);
        intent.selection = Some(SharePayload {
            root: intent.destination.root.clone(),
            page_id: calc.page.id,
            object_ids: vec![],
            stroke_ids: vec![],
            crop_pt: Rect {
                x: 0.,
                y: 0.,
                width: 100.,
                height: 100.,
            },
            session_ids: None,
        });
        assert_eq!(
            f.capture(intent).err().unwrap().code,
            "computational_context_unavailable"
        );
    }
    #[test]
    fn request_capture_refuses_invalid_selection_and_aggregate_source_budget() {
        let f = Fixture::new(3);
        let mut intent = f.intent(&[]);
        intent.selection = Some(SharePayload {
            root: intent.destination.root.clone(),
            page_id: f.pages[0].clone(),
            object_ids: vec!["foreign".into()],
            stroke_ids: vec![],
            crop_pt: Rect {
                x: 0.,
                y: 0.,
                width: 100.,
                height: 100.,
            },
            session_ids: None,
        });
        assert_eq!(
            f.capture(intent.clone()).err().unwrap().code,
            "selection_changed"
        );
        intent.selection.as_mut().unwrap().root = "foreign".into();
        assert_eq!(f.capture(intent).err().unwrap().code, "context_root_denied");
        std::fs::write(
            f.root.join("blocks/context-2.typ"),
            "x".repeat(Usage::Detailed.limits().0 + 1),
        )
        .unwrap();
        assert_eq!(
            f.capture(f.intent(&[2])).err().unwrap().code,
            "context_limit"
        );
    }
    #[tokio::test]
    async fn attached_page_text_presets_titles_search_and_render_are_confined() {
        let f = Fixture::new(3);
        f.install(&[2]);
        let text = f
            .call("read_page", json!({"page":f.pages[2]}))
            .await
            .unwrap()
            .to_string();
        assert!(
            text.contains("SOURCE_2")
                && text.contains("GRANTED_PRESET")
                && text.contains("page_typst")
        );
        assert!(!text.contains("SOURCE_1"));
        for id in [&f.pages[0], &f.pages[1], &"../private.typ".into()] {
            assert!(f.call("read_page", json!({"page":id})).await.is_err());
        }
        let listed = f.call("list_pages", json!({})).await.unwrap().to_string();
        assert!(listed.contains("quotedTitle"));
        assert!(!listed.contains("SOURCE_1"));
        let search = f
            .call("search", json!({"query":"SOURCE"}))
            .await
            .unwrap()
            .to_string();
        assert!(search.contains("SOURCE_2"));
        assert!(!search.contains("SOURCE_1"));
        assert!(f.call("render", json!({"page":f.pages[1]})).await.is_err());
        assert!(
            f.call(
                "render",
                json!({"page":f.pages[2],"rect":{"x":0,"y":0,"width":100000,"height":100000}})
            )
            .await
            .is_err()
        );
        let render = f
            .call("render", json!({"page":f.pages[2],"scale":0.2}))
            .await
            .unwrap();
        assert!(render.to_string().contains("image/png"));
        assert!(
            f.call(
                "typst_compile",
                json!({"page":f.pages[1],"source":"hi","target":{"kind":"pageText"}})
            )
            .await
            .is_err()
        );
        assert!(
            f.call(
                "typst_compile",
                json!({"page":f.pages[2],"source":"= Page text","target":{"kind":"pageText"}})
            )
            .await
            .is_ok()
        );
        for source in [
            "#read(\"/blocks/equation.typ\")",
            "#image(\"/assets/diagram.svg\")",
            "#include \"/styles/private.typ\"",
        ] {
            let result = f
                .call(
                    "typst_compile",
                    json!({"source":source,"target":{"kind":"block","widthPt":200}}),
                )
                .await
                .unwrap();
            assert!(result.to_string().contains("not shared"));
        }
        assert!(
            f.call(
                "request_scope",
                json!({"scope":{"kind":"notebook"},"reason":"more"})
            )
            .await
            .unwrap_err()
            .contains("scope unavailable")
        );
        assert!(
            f.call("read_page", json!({"page":f.pages[2]}))
                .await
                .is_ok()
        );
    }
    #[tokio::test]
    async fn attached_page_render_obeys_low_usage_and_call_budget() {
        let f = Fixture::new(2);
        let mut captured = f.capture(f.intent(&[1])).unwrap();
        captured.usage = Usage::Low;
        captured.conversation_id = "conversation".into();
        f.bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .sessions
            .get_mut("app")
            .unwrap()
            .install_request(captured);
        assert!(
            f.call("render", json!({"page":f.pages[1]}))
                .await
                .unwrap_err()
                .contains("limit_reached")
        );
        f.install(&[1]);
        {
            let mut state = f.bridge.helper.inner.lock().unwrap();
            let session = state.sessions.get_mut("app").unwrap();
            for _ in session.calls..Usage::Detailed.limits().3 {
                session.admit(Usage::Detailed, "status").unwrap();
            }
        }
        assert!(
            f.call("render", json!({"page":f.pages[1]}))
                .await
                .unwrap_err()
                .contains("limit_reached")
        );
    }
    #[tokio::test]
    async fn page_only_context_keeps_destination_private_and_later_requests_replace_authority() {
        let f = Fixture::new(3);
        f.install(&[2]);
        let context = f.call("get_context", json!({})).await.unwrap().to_string();
        assert!(context.contains("proposalBase") && context.contains(&f.pages[0]));
        assert!(!context.contains("F ="));
        let epoch = f.bridge.helper.inner.lock().unwrap().sessions["app"].epoch;
        f.install(&[1]);
        assert!(f.bridge.helper.inner.lock().unwrap().sessions["app"].epoch > epoch);
        assert!(
            f.call("read_page", json!({"page":f.pages[2]}))
                .await
                .is_err()
        );
        assert!(
            f.call("read_page", json!({"page":f.pages[1]}))
                .await
                .is_ok()
        );
        let mut s = f.bridge.helper.inner.lock().unwrap();
        let app = s.sessions.get_mut("app").unwrap();
        assert_eq!(app.compiles, 0);
        for _ in app.calls..Usage::Detailed.limits().3 {
            app.admit(Usage::Detailed, "status").unwrap();
        }
        assert!(app.admit(Usage::Detailed, "status").is_err());
        app.last_call = Instant::now() - Duration::from_secs(121);
        assert!(app.admit(Usage::Detailed, "status").is_err());
        drop(s);
        f.install(&[]);
        assert_eq!(
            f.bridge.helper.inner.lock().unwrap().sessions["app"].calls,
            0
        );
    }
    #[tokio::test]
    async fn attachment_revoke_invalidates_inflight_read_render_compile_and_propose() {
        for tool in ["read_page", "render", "typst_compile", "propose_blocks"] {
            let f = Fixture::new(3);
            let request = f.install(&[2]);
            let (ready, release) = f.hook();
            let args = match tool {
                "read_page" | "render" => json!({"page":f.pages[2],"scale":0.1}),
                "typst_compile" => {
                    json!({"source":"Generated text","target":{"kind":"block","widthPt":180}})
                }
                _ => f.proposal_args(),
            };
            let bridge = f.bridge.clone();
            let task = tokio::spawn(async move {
                if tool == "propose_blocks" {
                    bridge.propose(tool, args).await.map(|_| ())
                } else {
                    bridge
                        .run(tool, args, &mut Default::default(), &mut String::new())
                        .await
                        .map(|_| ())
                }
            });
            tokio::time::timeout(Duration::from_secs(20), ready.notified())
                .await
                .unwrap();
            f.bridge
                .helper
                .revoke_attachment("conversation", &request, &f.pages[2])
                .unwrap();
            f.bridge.helper.inner.lock().unwrap().before_return = None;
            release.notify_one();
            assert!(task.await.unwrap().unwrap_err().contains("changed"));
            assert!(
                f.bridge.helper.inner.lock().unwrap().sessions["app"]
                    .proposals
                    .is_empty()
            );
            assert!(
                f.call("read_page", json!({"page":f.pages[2]}))
                    .await
                    .is_err()
            );
        }
    }
    #[tokio::test]
    async fn notebook_close_end_and_disconnect_revoke_and_do_not_resurrect() {
        let f = Fixture::new(3);
        let id = f.install(&[2]);
        assert!(
            f.bridge
                .helper
                .revoke_attachment("foreign", &id, &f.pages[2])
                .is_err()
        );
        assert!(
            f.bridge
                .helper
                .revoke_attachment("conversation", "old-request", &f.pages[2])
                .is_err()
        );
        f.bridge.helper.revoke_notebook(&f.root.to_string_lossy());
        assert!(
            f.call("read_page", json!({"page":f.pages[2]}))
                .await
                .is_err()
        );
        assert!(f.bridge.helper.inner.lock().unwrap().requests.is_empty());
        f.install(&[2]);
        let stop = f.bridge.helper.inner.lock().unwrap().sessions["app"]
            .stop
            .subscribe();
        let (client, server) = tokio::io::duplex(1024);
        drop(client);
        f.bridge.serve_stream(server, stop).await;
        assert!(f.bridge.helper.inner.lock().unwrap().sessions.is_empty());
        assert!(f.bridge.helper.inner.lock().unwrap().requests.is_empty());
        assert!(
            f.call("read_page", json!({"page":f.pages[2]}))
                .await
                .is_err()
        );
        f.bridge.helper.stop();
        assert!(
            f.bridge
                .helper
                .inner
                .lock()
                .unwrap()
                .pending_launches
                .is_empty()
        );
    }
    #[tokio::test]
    async fn s4_proposal_destination_survives_page_and_notebook_switch_mid_compile() {
        let f = Fixture::new(3);
        let request = f.install(&[2]);
        let args = f.proposal_args();
        let (ready, release) = f.hook();
        let bridge = f.bridge.clone();
        let task = tokio::spawn(async move { bridge.propose("propose_blocks", args).await });
        ready.notified().await;
        // Terminal/current-view selection is deliberately unrelated to the in-app grant.
        let mut foreign = f.intent(&[]);
        foreign.destination.root = "another-notebook".into();
        {
            let mut s = f.bridge.helper.inner.lock().unwrap();
            let g = s.sessions["app"].request.as_ref().unwrap();
            let mut share = g.anchor.clone();
            share.root = foreign.destination.root;
            share.page_id = f.pages[1].clone();
            s.share = Some((share, g.info.clone()));
            s.generation += 1;
            s.before_return = None;
        }
        release.notify_one();
        let result = task.await.unwrap().unwrap();
        let p = f.bridge.helper.inner.lock().unwrap().sessions["app"].proposals
            [result["id"].as_str().unwrap()]
        .clone();
        assert_eq!(p.page_id, f.pages[0]);
        assert_eq!(p.root, f.root.to_string_lossy());
        assert!(
            storage::open_page(&f.root, &f.pages[0])
                .unwrap()
                .page
                .objects
                .iter()
                .all(|o| !o.fields().id.starts_with("helper-"))
        );
        let mut history = storage::NotebookHistory::default();
        storage::observe_notebook(&f.root, &mut history).unwrap();
        assert!(
            proposals::insert(
                &f.bridge.helper,
                None,
                &f.root,
                &f.pages[1],
                &mut history,
                proposals::ProposalInsert {
                    proposal_id: p.id.clone(),
                    proposal_version: p.version
                }
            )
            .is_err()
        );
        assert!(
            proposals::insert(
                &f.bridge.helper,
                None,
                &f.root,
                &f.pages[0],
                &mut history,
                proposals::ProposalInsert {
                    proposal_id: p.id.clone(),
                    proposal_version: p.version
                }
            )
            .is_ok()
        );
        f.bridge
            .helper
            .revoke_attachment("conversation", &request, &f.pages[2])
            .unwrap();
    }
    #[tokio::test]
    async fn terminal_and_other_inapp_sessions_never_inherit_request_pages() {
        let f = Fixture::new(3);
        f.install(&[2]);
        let mut other = bridge::new_session("other".into());
        other.info.approved = true;
        other.conversation = Some("foreign-conversation".into());
        f.bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .sessions
            .insert("other".into(), other);
        let mut bridge = f.bridge.clone();
        bridge.id = "other".into();
        assert!(
            bridge
                .run(
                    "read_page",
                    json!({"page":f.pages[2]}),
                    &mut Default::default(),
                    &mut String::new()
                )
                .await
                .is_err()
        );
        let mut terminal = bridge::new_session("terminal".into());
        terminal.info.approved = true;
        f.bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .sessions
            .insert("terminal".into(), terminal);
        bridge.id = "terminal".into();
        assert!(
            bridge
                .run(
                    "read_page",
                    json!({"page":f.pages[2]}),
                    &mut Default::default(),
                    &mut String::new()
                )
                .await
                .is_err()
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn launch_admission_and_followup_install_equivalent_grants_and_revoke_pending() {
        use tokio::io::AsyncWriteExt;
        let f = Fixture::new(3);
        let mut captured = f.capture(f.intent(&[2])).unwrap();
        captured.conversation_id = "conversation".into();
        let request = captured.request_id.clone();
        let launch = |grant: CapturedRequestGrant| codex::Launch {
            cancelled: Default::default(),
            created: Instant::now(),
            conversation: "conversation".into(),
            share: grant.anchor.clone(),
            info: grant.info.clone(),
            grant: Some(grant),
        };
        {
            let mut state = f.bridge.helper.inner.lock().unwrap();
            state.settings.enabled = true;
            state.requests.insert(request.clone(), captured.clone());
            state
                .pending_launches
                .insert("launch".into(), launch(captured.clone()));
        }
        let record = pipe::Record {
            protocol: 1,
            instance_id: "instance".into(),
            pipe: "unused".into(),
            token: "discovery".into(),
            pid: 1,
            started_at: 1,
        };
        let (mut client, mut server) = tokio::io::duplex(8192);
        client.write_all(b"{\"type\":\"goodtype-relay-hello\",\"protocol\":1,\"instanceId\":\"instance\",\"token\":\"discovery\",\"launchToken\":\"launch\"}\n").await.unwrap();
        let mut session = bridge::new_session("launched".into());
        assert!(
            pipe::handshake_bound(
                &mut server,
                &record,
                true,
                true,
                Some((&f.bridge.helper, &mut session))
            )
            .await
            .unwrap()
        );
        assert_eq!(session.request.as_ref().unwrap().pages.len(), 1);
        assert_eq!(session.request.as_ref().unwrap().session_id, "launched");
        assert!(pipe::admit_session(&f.bridge.helper, 0, session).is_some());
        {
            let mut state = f.bridge.helper.inner.lock().unwrap();
            let app = state.sessions.get_mut("app").unwrap();
            app.pages.insert("residue".into(), true);
            app.once_notebook = true;
            app.notebook = true;
            app.scope_root = Some("old".into());
            app.calls = 60;
        }
        codex::bind(&f.bridge.helper, "conversation", captured.clone());
        {
            let state = f.bridge.helper.inner.lock().unwrap();
            let a = &state.sessions["app"];
            let b = &state.sessions["launched"];
            assert!(
                a.pages.is_empty() && !a.once_notebook && !a.notebook && a.scope_root.is_none()
            );
            assert_eq!(a.calls, 0);
            assert_eq!(
                a.request.as_ref().unwrap().pages.keys().collect::<Vec<_>>(),
                b.request.as_ref().unwrap().pages.keys().collect::<Vec<_>>()
            );
            assert_eq!(
                a.request.as_ref().unwrap().destination_base,
                b.request.as_ref().unwrap().destination_base
            );
            assert_eq!(
                a.request.as_ref().unwrap().files(),
                b.request.as_ref().unwrap().files()
            );
            assert_eq!(a.request.as_ref().unwrap().epoch, 1);
        }
        f.bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .pending_launches
            .insert("pending".into(), launch(captured.clone()));
        f.bridge
            .helper
            .revoke_attachment("conversation", &request, &f.pages[2])
            .unwrap();
        assert!(
            f.bridge.helper.inner.lock().unwrap().requests[&request]
                .pages
                .is_empty()
        );
        let (mut client, mut server) = tokio::io::duplex(8192);
        client.write_all(b"{\"type\":\"goodtype-relay-hello\",\"protocol\":1,\"instanceId\":\"instance\",\"token\":\"discovery\",\"launchToken\":\"pending\"}\n").await.unwrap();
        let mut session = bridge::new_session("late".into());
        assert!(
            pipe::handshake_bound(
                &mut server,
                &record,
                true,
                true,
                Some((&f.bridge.helper, &mut session))
            )
            .await
            .unwrap()
        );
        assert!(session.request.as_ref().unwrap().pages.is_empty());
        f.bridge.helper.revoke_conversation("conversation");
        assert!(pipe::admit_session(&f.bridge.helper, 0, session).is_none());
    }
    #[tokio::test]
    async fn stop_end_disable_and_close_invalidate_inflight_work_and_proposal_apply() {
        for action in ["stop", "end", "disable", "close"] {
            let f = Fixture::new(3);
            f.install(&[2]);
            let proposal = f
                .bridge
                .propose("propose_blocks", f.proposal_args())
                .await
                .unwrap();
            let id = proposal["id"].as_str().unwrap().to_owned();
            let (ready, release) = f.hook();
            let bridge = f.bridge.clone();
            let page = f.pages[2].clone();
            let task = tokio::spawn(async move {
                bridge
                    .run(
                        "read_page",
                        json!({"page":page}),
                        &mut Default::default(),
                        &mut String::new(),
                    )
                    .await
            });
            ready.notified().await;
            match action {
                "stop" => f.bridge.helper.revoke_conversation("conversation"),
                "end" => codex::cleanup(&f.bridge.helper, "conversation"),
                "disable" => f.bridge.helper.stop(),
                _ => f.bridge.helper.revoke_notebook(&f.root.to_string_lossy()),
            }
            f.bridge.helper.inner.lock().unwrap().before_return = None;
            release.notify_one();
            assert!(task.await.unwrap().is_err());
            let mut history = storage::NotebookHistory::default();
            storage::observe_notebook(&f.root, &mut history).unwrap();
            assert!(
                proposals::insert(
                    &f.bridge.helper,
                    None,
                    &f.root,
                    &f.pages[0],
                    &mut history,
                    proposals::ProposalInsert {
                        proposal_id: id,
                        proposal_version: 1
                    }
                )
                .is_err()
            );
            assert!(f.bridge.helper.inner.lock().unwrap().requests.is_empty());
        }
    }
    #[tokio::test]
    async fn new_request_retains_review_authorization_but_revocation_removes_it() {
        let f = Fixture::new(3);
        f.install(&[2]);
        let proposal = f
            .bridge
            .propose("propose_blocks", f.proposal_args())
            .await
            .unwrap();
        let id = proposal["id"].as_str().unwrap().to_owned();
        f.install(&[1]);
        let mut history = storage::NotebookHistory::default();
        storage::observe_notebook(&f.root, &mut history).unwrap();
        assert!(
            proposals::insert(
                &f.bridge.helper,
                None,
                &f.root,
                &f.pages[0],
                &mut history,
                proposals::ProposalInsert {
                    proposal_id: id,
                    proposal_version: 1
                }
            )
            .is_ok()
        );
        assert!(
            f.call("read_page", json!({"page":f.pages[2]}))
                .await
                .is_err()
        );
        let proposal = f
            .bridge
            .propose("propose_blocks", f.proposal_args())
            .await
            .unwrap();
        let pending_id = proposal["id"].as_str().unwrap().to_owned();
        let before = storage::open_page(&f.root, &f.pages[0]).unwrap();
        f.bridge.helper.revoke_conversation("conversation");
        assert!(
            proposals::insert(
                &f.bridge.helper,
                None,
                &f.root,
                &f.pages[0],
                &mut history,
                proposals::ProposalInsert {
                    proposal_id: pending_id,
                    proposal_version: 1
                }
            )
            .is_err()
        );
        let after = storage::open_page(&f.root, &f.pages[0]).unwrap();
        assert_eq!(
            serde_json::to_value(&before.page).unwrap(),
            serde_json::to_value(&after.page).unwrap()
        );
        assert_eq!(
            before
                .blocks
                .iter()
                .map(|f| (&f.path, &f.bytes))
                .collect::<Vec<_>>(),
            after
                .blocks
                .iter()
                .map(|f| (&f.path, &f.bytes))
                .collect::<Vec<_>>()
        );
    }
    #[tokio::test]
    async fn transcribe_selection_preserves_exact_stroke_grant() {
        let f = Fixture::new(2);
        let mut b = storage::open_page(&f.root, &f.pages[0]).unwrap();
        let mut private = b.ink_layers[0].strokes[0].clone();
        private.id = "private-stroke".into();
        b.ink_layers[0].strokes.push(private);
        for object in &mut b.page.objects {
            if let PageObject::InkGroup { stroke_ids, .. } = object {
                stroke_ids.push("private-stroke".into());
            }
        }
        storage::save_notebook(&f.root, &b).unwrap();
        let mut intent = f.intent(&[]);
        intent.selection = Some(SharePayload {
            root: intent.destination.root.clone(),
            page_id: f.pages[0].clone(),
            object_ids: vec![],
            stroke_ids: vec!["stroke-001".into()],
            crop_pt: Rect {
                x: 0.,
                y: 0.,
                width: b.page.geometry.width_pt,
                height: b.page.geometry.height_pt,
            },
            session_ids: None,
        });
        let mut captured = f.capture(intent).unwrap();
        captured.conversation_id = "conversation".into();
        f.bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .sessions
            .get_mut("app")
            .unwrap()
            .install_request(captured);
        assert_eq!(
            f.call("transcribe", json!({"kind":"math"}))
                .await
                .unwrap_err(),
            "no_window"
        );
        assert!(
            f.call(
                "transcribe",
                json!({"kind":"math","strokeIds":["private-stroke"]})
            )
            .await
            .unwrap_err()
            .contains("foreign stroke")
        );
    }
    #[tokio::test]
    async fn ink_on_pdf_balanced_has_pixels_low_has_only_notice() {
        let f = Fixture::new(2);
        let mut b = storage::open_page(&f.root, &f.pages[0]).unwrap();
        b.page.background = goodtype_core::PageBackground::Pdf {
            source_path: "references/lecture.pdf".into(),
            page: 1,
        };
        storage::save_notebook(&f.root, &b).unwrap();
        for usage in [Usage::Balanced, Usage::Low] {
            let mut intent = f.intent(&[]);
            intent.selection = Some(SharePayload {
                root: intent.destination.root.clone(),
                page_id: f.pages[0].clone(),
                object_ids: vec![],
                stroke_ids: vec!["stroke-001".into()],
                crop_pt: Rect {
                    x: 0.,
                    y: 0.,
                    width: 150.,
                    height: 100.,
                },
                session_ids: None,
            });
            let mut captured =
                snapshot::capture_locked(&f.bridge.roots, &f.bridge.histories, intent, usage)
                    .unwrap();
            assert!(!captured.files().contains_key("/references/lecture.pdf"));
            captured.conversation_id = "conversation".into();
            f.bridge
                .helper
                .inner
                .lock()
                .unwrap()
                .sessions
                .get_mut("app")
                .unwrap()
                .install_request(captured);
            let result = f.call("get_context", json!({})).await.unwrap().to_string();
            assert_eq!(result.contains("image/png"), usage == Usage::Balanced);
            if usage == Usage::Low {
                assert!(result.contains("Image context is off"));
            }
        }
    }

    #[tokio::test]
    async fn region_only_has_no_sources_and_cannot_render_outside_crop() {
        let f = Fixture::new(2);
        let mut page = storage::open_page(&f.root, &f.pages[0]).unwrap();
        let mut fields = page.page.objects[0].fields().clone();
        fields.id = "region-material".into();
        fields.reading_order = page.page.reading_order.len() as u32;
        fields.group_id = None;
        fields.x = 80.;
        fields.y = 100.;
        fields.scale = 1.;
        fields.rotation = 0.;
        page.page.reading_order.push(fields.id.clone());
        page.page.objects.push(PageObject::PdfMaterial {
            fields,
            source_path: "references/lecture.pdf".into(),
            page: 0,
            source_width_pt: 100.,
            source_height_pt: 60.,
        });
        storage::save_notebook(&f.root, &page).unwrap();
        let mut intent = f.intent(&[]);
        intent.selection = Some(SharePayload {
            root: intent.destination.root.clone(),
            page_id: f.pages[0].clone(),
            object_ids: vec![],
            stroke_ids: vec![],
            crop_pt: Rect {
                x: 70.,
                y: 90.,
                width: 100.,
                height: 80.,
            },
            session_ids: None,
        });
        let mut captured = f.capture(intent).unwrap();
        assert!(captured.pages.is_empty());
        assert_eq!(
            captured
                .selection
                .as_ref()
                .unwrap()
                .snapshot
                .page
                .objects
                .len(),
            1
        );
        assert!(!captured.files().contains_key("/blocks/equation.typ"));
        assert!(!captured.files().contains_key("/references/lecture.pdf"));
        assert!(
            captured
                .selection
                .as_ref()
                .unwrap()
                .render_files
                .contains_key("/references/lecture.pdf")
        );
        assert_eq!(snapshot::context_text(&captured)["quotedData"], json!([]));
        assert!(
            !snapshot::context_text(&captured)
                .to_string()
                .contains("region-material")
        );
        captured.conversation_id = "conversation".into();
        f.bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .sessions
            .get_mut("app")
            .unwrap()
            .install_request(captured);
        assert!(
            f.call(
                "render",
                json!({"rect":{"x":0,"y":0,"width":10,"height":10}})
            )
            .await
            .unwrap_err()
            .contains("outside approved crop")
        );
        assert!(
            f.call("get_context", json!({}))
                .await
                .unwrap()
                .to_string()
                .contains("image/png")
        );
    }

    #[tokio::test]
    async fn selection_crop_and_identity_are_enforced_without_attachment_widening() {
        let f = Fixture::new(3);
        let mut intent = f.intent(&[]);
        intent.selection = Some(SharePayload {
            root: intent.destination.root.clone(),
            page_id: f.pages[0].clone(),
            object_ids: vec!["typst-001".into()],
            stroke_ids: vec![],
            crop_pt: Rect {
                x: 70.,
                y: 90.,
                width: 240.,
                height: 120.,
            },
            session_ids: None,
        });
        let mut captured = f.capture(intent).unwrap();
        captured.conversation_id = "conversation".into();
        f.bridge
            .helper
            .inner
            .lock()
            .unwrap()
            .sessions
            .get_mut("app")
            .unwrap()
            .install_request(captured);
        let context = f.call("get_context", json!({})).await.unwrap().to_string();
        assert!(context.contains("typst-001"));
        assert!(
            context.contains("image/png"),
            "selected context retains its cropped preview"
        );
        assert!(!context.contains("image-001"));
        assert!(f.call("render", json!({"page":f.pages[0],"rect":{"x":70,"y":90,"width":240,"height":120},"scale":0.1})).await.unwrap().to_string().contains("image/png"));
        assert!(
            f.call(
                "render",
                json!({"rect":{"x":0,"y":0,"width":595,"height":842}})
            )
            .await
            .is_err()
        );
        assert!(
            f.call("read_page", json!({"page":f.pages[0]}))
                .await
                .is_err()
        );
        assert!(
            f.call(
                "transcribe",
                json!({"strokeIds":["stroke-001"],"kind":"text"})
            )
            .await
            .is_err()
        );
        {
            let mut state = f.bridge.helper.inner.lock().unwrap();
            state.sessions.get_mut("app").unwrap().conversation = Some("wrong-conversation".into());
        }
        assert!(
            f.call("get_context", json!({}))
                .await
                .unwrap_err()
                .contains("changed")
        );
    }

    #[tokio::test]
    async fn untrusted_tools_cannot_write_or_supply_their_own_limits_or_inherit_once_scope() {
        let f = Fixture::new(3);
        f.install(&[2]);
        let before = std::fs::read(f.root.join("pages/page-001.json")).unwrap();
        assert!(
            f.call(
                "save_page",
                json!({"root":f.root,"page":f.pages[0],"source":"ATTACK"})
            )
            .await
            .is_err()
        );
        assert_eq!(
            std::fs::read(f.root.join("pages/page-001.json")).unwrap(),
            before
        );
        {
            let mut state = f.bridge.helper.inner.lock().unwrap();
            let session = state.sessions.get_mut("app").unwrap();
            session.once_notebook = true;
            session.notebook = true;
            session.pages.insert(f.pages[1].clone(), true);
        }
        assert!(
            f.call(
                "read_page",
                json!({"page":f.pages[1],"usage":"unlimited","sessionId":"other"})
            )
            .await
            .is_err()
        );
        let intent = json!({"destination":{"root":f.root,"pageId":f.pages[0]},"selection":null,"pageIds":[],"presets":false,"intentVersion":"1","usage":"unlimited"});
        assert!(serde_json::from_value::<RequestContextIntent>(intent).is_err());
        assert!(serde_json::from_value::<Usage>(json!("unlimited")).is_err());
        assert!(serde_json::from_value::<RequestContextIntent>(json!({"pageIds":[]})).is_err());
    }
    #[test]
    fn send_refuses_deleted_pages_and_missing_destination_before_admission() {
        let f = Fixture::new(3);
        let bundle = storage::open_page(&f.root, &f.pages[2]).unwrap();
        let path = bundle
            .manifest
            .pages
            .iter()
            .find(|p| p.id == f.pages[2])
            .unwrap()
            .path
            .clone();
        std::fs::remove_file(f.root.join(path)).unwrap();
        assert_eq!(
            f.capture(f.intent(&[2])).err().unwrap().code,
            "context_page_unavailable"
        );
        let mut intent = f.intent(&[]);
        intent.destination.page_id = "deleted-destination".into();
        assert_eq!(
            f.capture(intent).err().unwrap().code,
            "context_page_unavailable"
        );
    }

    #[tokio::test]
    async fn captured_bytes_do_not_follow_later_disk_edits_and_mcp_u64s_stay_decimal_strings() {
        let f = Fixture::new(3);
        f.install(&[2]);
        std::fs::write(f.root.join("blocks/context-2.typ"), "UNSENT_SECRET").unwrap();
        let data = f
            .call("read_page", json!({"page":f.pages[2]}))
            .await
            .unwrap()
            .to_string();
        assert!(data.contains("SOURCE_2"));
        assert!(!data.contains("UNSENT_SECRET"));
        {
            let mut state = f.bridge.helper.inner.lock().unwrap();
            let session = state.sessions.get_mut("app").unwrap();
            let mut grant = (**session.request.as_ref().unwrap()).clone();
            session.epoch = 9_007_199_254_740_993;
            grant.epoch = session.epoch;
            grant.destination_base.revision = u64::MAX;
            session.request = Some(Arc::new(grant));
        }
        let status = f.call("status", json!({})).await.unwrap();
        let status: Value = serde_json::from_str(status[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(status["epoch"], "9007199254740993");
        let context = f.call("get_context", json!({})).await.unwrap();
        let context: Value = serde_json::from_str(context[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(context["proposalBase"]["revision"], "18446744073709551615");
    }

    #[tokio::test]
    async fn terminal_grant_changes_do_not_invalidate_an_inapp_request_epoch() {
        let f = Fixture::new(3);
        f.install(&[2]);
        let (ready, release) = f.hook();
        let bridge = f.bridge.clone();
        let page = f.pages[2].clone();
        let task = tokio::spawn(async move {
            bridge
                .run(
                    "read_page",
                    json!({"page":page}),
                    &mut Default::default(),
                    &mut String::new(),
                )
                .await
        });
        ready.notified().await;
        {
            let mut state = f.bridge.helper.inner.lock().unwrap();
            state.share = None;
            state.generation += 1;
            state.before_return = None;
        }
        release.notify_one();
        assert!(task.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn proposal_dispatch_cannot_adopt_a_new_request_between_admission_and_compile() {
        let f = Fixture::new(3);
        f.install(&[2]);
        let args = f.proposal_args();
        let stamp = {
            let state = f.bridge.helper.inner.lock().unwrap();
            (state.generation, state.sessions["app"].request.clone())
        };
        f.install(&[1]);
        assert!(
            f.bridge
                .propose_scoped("propose_blocks", args, stamp)
                .await
                .unwrap_err()
                .contains("changed")
        );
        assert!(
            f.bridge.helper.inner.lock().unwrap().sessions["app"]
                .proposals
                .is_empty()
        );
    }
}
