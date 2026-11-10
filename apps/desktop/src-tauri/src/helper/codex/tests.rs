use super::*;
#[test]
fn connection_failure_is_distinct_from_safety_checks_and_turn_failures() {
    let detail =
        json!({"code":-32603,"message":"workspace routing discovery timed out"}).to_string();
    assert_eq!(
        problem("confinement_failed", &detail).code,
        "provider_unreachable"
    );
    assert_eq!(
        problem("confinement_failed", "app-server response timed out").code,
        "provider_unreachable"
    );
    assert_eq!(
        problem("confinement_failed", "unexpected shell permission").code,
        "confinement_failed"
    );
    assert_eq!(problem("codex_crashed", &detail).code, "codex_crashed");
}

#[test]
fn approval_responses_deny_authority() {
    for method in [
        "item/commandExecution/requestApproval",
        "item/fileChange/requestApproval",
    ] {
        assert_eq!(client::decline(method), Some(json!({"decision":"decline"})));
    }
    assert_eq!(
        client::decline("item/permissions/requestApproval").unwrap()["permissions"],
        json!({})
    );
    assert_eq!(
        client::decline("mcpServer/elicitation/request").unwrap()["action"],
        "decline"
    );
    assert_eq!(
        client::decline("item/tool/requestUserInput").unwrap()["answers"],
        json!({})
    );
}
#[test]
fn jsonl_bounds_and_fake_routing() {
    let bytes = b"{\"id\":7,\"result\":{}}\n{\"method\":\"turn/completed\",\"params\":{}}\n";
    let mut reader = std::io::Cursor::new(bytes);
    let first: Value = serde_json::from_slice(&client::read_line(&mut reader).unwrap()).unwrap();
    assert_eq!(first["id"], 7);
    let second: Value = serde_json::from_slice(&client::read_line(&mut reader).unwrap()).unwrap();
    assert_eq!(second["method"], "turn/completed");
    assert!(client::read_line(&mut reader).is_err());
    assert!(
        client::read_line(&mut std::io::Cursor::new(vec![
            b'x';
            client::LINE_LIMIT + 2
        ]))
        .is_err()
    );
}
#[test]
fn shares_are_session_scoped() {
    let mut inner = Inner::default();
    let share = SharePayload {
        root: "root".into(),
        page_id: "p".into(),
        object_ids: vec!["terminal".into()],
        stroke_ids: vec![],
        crop_pt: Rect {
            x: 0.,
            y: 0.,
            width: 1.,
            height: 1.,
        },
        session_ids: None,
    };
    let info = ShareInfo {
        page_id: "p".into(),
        page_number: 1,
        object_count: 1,
        crop_pt: share.crop_pt,
        session_ids: vec!["terminal".into()],
        shared_at: 0,
    };
    inner.share = Some((share.clone(), info.clone()));
    inner
        .sessions
        .insert("terminal".into(), bridge::new_session("terminal".into()));
    let mut s = bridge::new_session("app".into());
    s.conversation = Some("conversation".into());
    assert!(s.run_share.is_none());
    inner.sessions.insert("app".into(), s);
    assert!(inner.share_for("app").is_none());
    let mut app_share = share;
    app_share.object_ids = vec!["app-only".into()];
    inner.sessions.get_mut("app").unwrap().run_share = Some((app_share, info));
    assert_eq!(
        inner.share_for("terminal").unwrap().0.object_ids,
        ["terminal"]
    );
    assert_eq!(inner.share_for("app").unwrap().0.object_ids, ["app-only"]);
}
#[test]
#[ignore = "Read-only installed Codex probe; never starts a thread or model turn"]
fn codex_real() {
    let path = client::resolve(None).unwrap();
    let version = client::version(&path).unwrap();
    println!("version: {version}");
    let mut discovery = client::Client::spawn(&path, &[]).unwrap();
    let account = discovery.initialize(&mut |_| {}).unwrap();
    println!("account type: {account} (all account details redacted)");
    let read = discovery
        .call("config/read", json!({"includeLayers":true}), &mut |_| {})
        .unwrap();
    let names = config::names(&read);
    drop(discovery);
    let relay = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("goodtype-mcp.exe");
    let args = config::overrides(
        &relay.to_string_lossy(),
        &"0".repeat(64),
        &names,
        &[],
        false,
    );
    let mut c = match client::Client::spawn(&path, &args) {
        Ok(c) => c,
        Err(e) => panic!("override process rejected: {e}"),
    };
    c.initialize(&mut |_| {}).unwrap();
    let read = c
        .call("config/read", json!({"includeLayers":true}), &mut |_| {})
        .unwrap();
    println!("config read-back: {}", config::evidence(&read));
    let inventory = c
        .call("mcpServerStatus/list", json!({}), &mut |_| {})
        .unwrap();
    println!(
        "MCP inventory (names/tool counts only): {}",
        json!({"servers":inventory["data"].as_array().map(|a|a.iter().map(|s|json!({"name":s["name"],"tools":s["tools"].as_object().map(|t|t.len())})).collect::<Vec<_>>()),"nextCursor":inventory["nextCursor"]})
    );
    assert_eq!(read["config"]["features"]["shell_tool"], false);
    // Without the Goodtype app running, the goodtype server cannot be ready, so this probe checks
    // the effective config and that every server Goodtype did not select stays inactive.
    config::check_config(&read, &[], false).expect("effective config matches the launch overrides");
    config::check_inventory(&inventory, &[], false).expect("unselected MCP servers stay inactive");
}

#[test]
#[ignore = "Installed Codex thread-inventory probe; disables every MCP server and sends no model turn"]
fn codex_real_thread_inventory() {
    let path = client::resolve(None).unwrap();
    let mut discovery = client::Client::spawn(&path, &[]).unwrap();
    discovery.initialize(&mut |_| {}).unwrap();
    let read = discovery
        .call("config/read", json!({"includeLayers":true}), &mut |_| {})
        .unwrap();
    let names = config::names(&read);
    drop(discovery);
    let mut args = config::overrides("unused.exe", &"0".repeat(64), &names, &[], false);
    args.push("mcp_servers.goodtype.enabled=false".into());
    let mut c = client::Client::spawn(&path, &args).unwrap();
    c.initialize(&mut |_| {}).unwrap();
    let thread = c.call("thread/start", json!({"cwd":c.cwd.path(),"ephemeral":true,"approvalPolicy":"never","sandbox":"read-only"}), &mut |_| {}).unwrap();
    let inventory = c
        .call(
            "mcpServerStatus/list",
            json!({"threadId":thread["thread"]["id"],"detail":"toolsAndAuthOnly","limit":100}),
            &mut |_| {},
        )
        .unwrap();
    for server in inventory["data"].as_array().unwrap() {
        println!(
            "{}: runtime={}, tools={}",
            server["name"],
            server["runtimeStatus"],
            server["tools"].as_object().unwrap().len()
        );
    }
    config::check_inventory(&inventory, &[], false).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Installed Codex and real relay launch-token preflight; isolated discovery, no notebook or model turn"]
async fn codex_real_launch_preflight() {
    use super::super::{pipe, security};
    let path = client::resolve(None).unwrap();
    let relay = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("goodtype-mcp.exe");
    let mut discovery = client::Client::spawn(&path, &[]).unwrap();
    discovery.initialize(&mut |_| {}).unwrap();
    let read = discovery
        .call("config/read", json!({"includeLayers":true}), &mut |_| {})
        .unwrap();
    let names = config::names(&read);
    drop(discovery);
    let root = tempfile::tempdir().unwrap();
    let instance_id = random_hex(16).unwrap();
    let record = pipe::Record {
        protocol: 1,
        pipe: format!(r"\\.\pipe\goodtype-helper-{instance_id}"),
        instance_id,
        token: random_hex(32).unwrap(),
        pid: std::process::id(),
        started_at: security::process_started_at().unwrap(),
    };
    pipe::write_record(&root.path().join("Goodtype/helper/instance.json"), &record).unwrap();
    let mut listener = pipe::instance(&record, true).unwrap();
    let helper = Helper::default();
    let token = random_hex(32).unwrap();
    let share = SharePayload {
        root: "test-only".into(),
        page_id: "p".into(),
        object_ids: vec!["test".into()],
        stroke_ids: vec![],
        crop_pt: Rect {
            x: 0.,
            y: 0.,
            width: 1.,
            height: 1.,
        },
        session_ids: None,
    };
    let info = ShareInfo {
        page_id: "p".into(),
        page_number: 1,
        object_count: 1,
        crop_pt: share.crop_pt,
        session_ids: vec![],
        shared_at: 0,
    };
    {
        let mut s = helper.inner.lock().unwrap();
        s.settings.enabled = true;
        s.pending_launches.insert(
            token.clone(),
            Launch {
                grant: None,
                cancelled: Default::default(),
                created: Instant::now(),
                conversation: "test".into(),
                share,
                info,
            },
        );
    }
    let task = tokio::spawn(async move {
        listener.connect().await.unwrap();
        let mut session = bridge::new_session("test-session".into());
        assert!(
            pipe::handshake_bound(
                &mut listener,
                &record,
                true,
                true,
                Some((&helper, &mut session))
            )
            .await
            .unwrap()
        );
        assert!(session.info.approved);
        assert_eq!(session.conversation.as_deref(), Some("test"));
        println!("Real relay launch-token handshake accepted");
        let stop = session.stop.subscribe();
        helper
            .inner
            .lock()
            .unwrap()
            .sessions
            .insert("test-session".into(), session);
        let bridge = bridge::Bridge {
            helper,
            app: None,
            id: "test-session".into(),
            roots: Default::default(),
            histories: Default::default(),
        };
        bridge.serve_stream(listener, stop).await;
    });
    let mut args = config::overrides(&relay.to_string_lossy(), &token, &names, &[], false);
    args.push(format!(
        "mcp_servers.goodtype.env.LOCALAPPDATA={}",
        serde_json::to_string(&root.path().to_string_lossy()).unwrap()
    ));
    let mut c = client::Client::spawn(&path, &args).unwrap();
    c.initialize(&mut |_| {}).unwrap();
    let thread = c.call("thread/start", json!({"cwd":c.cwd.path(),"ephemeral":true,"approvalPolicy":"never","sandbox":"read-only"}), &mut |_| {}).unwrap();
    let read = c
        .call("config/read", json!({"includeLayers":true}), &mut |_| {})
        .unwrap();
    let inventory = c
        .thread_inventory(
            thread["thread"]["id"].as_str().unwrap(),
            &names,
            &mut |_| {},
        )
        .unwrap();
    config::preflight(&read, &inventory, &[], false).unwrap();
    println!("Launch-token handshake and preflight passed; no model turn sent");
    drop(c);
    task.abort();
}

#[test]
fn settings_change_releases_helper_lock_before_closing_conversations() {
    let h = Helper::default();
    let conversations = h.codex.conversations.lock().unwrap();
    let worker = h.clone();
    let task = std::thread::spawn(move || {
        worker.apply_settings(
            HelperSettings {
                codex_path: Some("changed".into()),
                ..Default::default()
            },
            || Ok(()),
        )
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut observed = false;
    while Instant::now() < deadline {
        if let Ok(state) = h.inner.try_lock()
            && state.settings.codex_path.as_deref() == Some("changed")
        {
            observed = true;
            break;
        }
        std::thread::yield_now();
    }
    drop(conversations);
    task.join().unwrap().unwrap();
    assert!(
        observed,
        "settings must release helper lock before waiting for conversations"
    );
}

#[test]
fn app_model_defaults_and_explicit_choices() {
    for request in [None, Some(""), Some("  ")] {
        for saved in [None, Some(""), Some("  ")] {
            assert_eq!(resolve_model(request, saved), "gpt-6-luna");
        }
        assert_eq!(resolve_model(request, Some("gpt-6-terra")), "gpt-6-terra");
    }
    assert_eq!(
        resolve_model(Some("gpt-6-sol"), Some("gpt-6-terra")),
        "gpt-6-sol"
    );
    let request: CodexRunRequest =
        serde_json::from_value(json!({"prompt":"hello","webSearch":false})).unwrap();
    assert_eq!(resolve_model(request.model.as_deref(), None), DEFAULT_MODEL);
    let request: CodexRunRequest =
        serde_json::from_value(json!({"prompt":"hello","webSearch":false,"model":null})).unwrap();
    assert_eq!(resolve_model(request.model.as_deref(), None), DEFAULT_MODEL);
}
#[test]
fn unavailable_default_refuses_instead_of_substituting_inventory_model() {
    let mut models = vec![Model {
        id: "gpt-6-sol".into(),
        label: "Sol".into(),
        image: true,
    }];
    let error = require_model(&models, DEFAULT_MODEL).unwrap_err();
    assert_eq!(error.code, "model_unavailable");
    assert_eq!(error.params["detail"], DEFAULT_MODEL);
    assert!(require_model(&models, "gpt-6-sol").is_ok());
    models.push(Model {
        id: DEFAULT_MODEL.into(),
        label: "Luna".into(),
        image: true,
    });
    assert!(require_model(&models, DEFAULT_MODEL).is_ok());
}
#[test]
fn initial_and_followup_payloads_pin_model_and_medium_effort() {
    for model in [
        resolve_model(None, None),
        resolve_model(None, Some("gpt-6-terra")),
    ] {
        for prompt in ["initial request", "followup request"] {
            let params = turn_params("thread-1", &model, prompt);
            assert_eq!(params["threadId"], "thread-1");
            assert_eq!(params["model"], model);
            assert_eq!(params["effort"], "medium");
            assert!(
                params["input"][0]["text"]
                    .as_str()
                    .unwrap()
                    .starts_with(prompt)
            );
            assert!(
                params["input"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("image(block)")
            );
        }
    }
}
