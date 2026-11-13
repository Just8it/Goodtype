use super::{
    Helper, random_hex,
    security::{UserSecurity, process_started_at},
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
    sync::Semaphore,
};

use tauri::Manager;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub protocol: u32,
    pub instance_id: String,
    pub pipe: String,
    pub token: String,
    pub pid: u32,
    pub started_at: u64,
}
pub fn record_path() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
        .join("Goodtype/helper/instance.json")
}
pub(super) fn write_record(path: &std::path::Path, record: &Record) -> Result<(), String> {
    let security = UserSecurity::new()?;
    let parent = path.parent().ok_or("no record parent")?;
    security.directory(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    security.apply(temp.path())?;
    std::io::Write::write_all(
        &mut temp,
        &serde_json::to_vec(record).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
pub(super) fn instance(record: &Record, first: bool) -> Result<NamedPipeServer, String> {
    let security = UserSecurity::new()?;
    let mut attributes = security.attributes();
    unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                &record.pipe,
                (&mut attributes as *mut windows_sys::Win32::Security::SECURITY_ATTRIBUTES).cast(),
            )
    }
    .map_err(|e| e.to_string())
}
pub fn start(helper: Helper, app: tauri::AppHandle) -> Result<(), String> {
    if std::env::var_os("LOCALAPPDATA").is_none() {
        return Err("LOCALAPPDATA missing".into());
    }
    let id = random_hex(16)?;
    let record = Record {
        protocol: 1,
        pipe: format!(r"\\.\pipe\goodtype-helper-{id}"),
        instance_id: id,
        token: random_hex(32)?,
        pid: std::process::id(),
        started_at: process_started_at()?,
    };
    let generation = helper
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .enable_generation;
    let first = instance(&record, true)?;
    let security = UserSecurity::new()?;
    security.directory(record_path().parent().unwrap())?;
    use std::os::windows::fs::OpenOptionsExt;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(record_path().with_extension("lock"))
        .map_err(|e| format!("another helper instance owns discovery: {e}"))?;
    security.apply(&record_path().with_extension("lock"))?;
    write_record(&record_path(), &record)?;
    let h = helper.clone();
    let task = tauri::async_runtime::spawn(async move {
        let slots = std::sync::Arc::new(Semaphore::new(4));
        let mut listener = first;
        let mut connections = tokio::task::JoinSet::new();
        loop {
            if listener.connect().await.is_err() {
                break;
            }
            let next = match instance(&record, false) {
                Ok(p) => p,
                Err(_) => break,
            };
            let mut stream = std::mem::replace(&mut listener, next);
            let permit = slots.clone().try_acquire_owned();
            let record = record.clone();
            let helper = h.clone();
            let app = app.clone();
            while connections.try_join_next().is_some() {}
            connections.spawn(async move {
                let enabled = helper
                    .inner
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .settings
                    .enabled;
                let id = match random_hex(16) {
                    Ok(id) => id,
                    Err(_) => return,
                };
                let mut session = super::bridge::new_session(id.clone());
                if !matches!(
                    tokio::time::timeout(
                        Duration::from_secs(5),
                        handshake_bound(
                            &mut stream,
                            &record,
                            enabled,
                            permit.is_ok(),
                            Some((&helper, &mut session))
                        )
                    )
                    .await,
                    Ok(Ok(true))
                ) {
                    return;
                }
                let Ok(_permit) = permit else { return };
                let Some(stop) = admit_session(&helper, generation, session) else {
                    return;
                };
                let server = super::bridge::Bridge {
                    helper: helper.clone(),
                    app: Some(app.clone()),
                    id: id.clone(),
                    roots: std::sync::Arc::new(crate::workspace::AllowedRoots(
                        app.state::<crate::workspace::AllowedRoots>().0.clone(),
                    )),
                    histories: app
                        .state::<crate::notebook::NotebookHistories>()
                        .inner()
                        .clone(),
                };
                server.serve_stream(stream, stop).await;
            });
        }
        {
            let mut s = h.inner.lock().unwrap_or_else(|e| e.into_inner());
            if s.enable_generation == generation {
                s.listening = false;
            }
        }
        h.notify(&app);
    });
    let mut s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
    s.discovery_lock = Some(lock);
    s.listener = Some(task);
    s.listening = true;
    Ok(())
}
#[cfg(test)]
pub async fn handshake<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    record: &Record,
    enabled: bool,
    available: bool,
) -> Result<bool, String> {
    handshake_bound(stream, record, enabled, available, None).await
}
pub(super) async fn handshake_bound<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    record: &Record,
    enabled: bool,
    available: bool,
    binding: Option<(&Helper, &mut super::Session)>,
) -> Result<bool, String> {
    let mut bytes = Vec::new();
    loop {
        let b = stream.read_u8().await.map_err(|e| e.to_string())?;
        if b == b'\n' {
            break;
        }
        bytes.push(b);
        if bytes.len() > 4096 {
            return Err("oversized handshake".into());
        }
    }
    let hello: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let mut reason = if !enabled {
        Some("disabled")
    } else if !available {
        Some("busy")
    } else if hello["protocol"] != 1 {
        Some("protocol_mismatch")
    } else if hello["type"] != "goodtype-relay-hello"
        || hello["instanceId"] != record.instance_id
        || hello["token"] != record.token
    {
        Some("bad_token")
    } else {
        None
    };
    if reason.is_none() && hello.get("launchToken").is_some() {
        let accepted = if let (Some(token), Some((helper, session))) =
            (hello["launchToken"].as_str(), binding)
        {
            let mut s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(launch) = s.pending_launches.remove(token).filter(|l| {
                l.created.elapsed() < Duration::from_secs(60)
                    && !l.cancelled.load(std::sync::atomic::Ordering::SeqCst)
            }) {
                session.launch_cancelled = Some(launch.cancelled);
                session.conversation = Some(launch.conversation);
                session.info.approved = true;
                let mut info = launch.info;
                info.session_ids = vec![session.info.id.clone()];
                session.run_share = Some((launch.share, info));
                if let Some(grant) = launch.grant {
                    let Some(grant) = s.requests.get(&grant.request_id).cloned() else {
                        return Err("request revoked".into());
                    };
                    session.install_request(grant);
                }
                s.generation += 1;
                true
            } else {
                false
            }
        } else {
            false
        };
        if !accepted {
            reason = Some("bad_token");
        }
    }
    let reply = if let Some(reason) = reason {
        serde_json::json!({"type":"goodtype-relay-refused","reason":reason,"message":format!("Goodtype helper refused the connection: {reason}")})
    } else {
        serde_json::json!({"type":"goodtype-relay-welcome","protocol":1})
    };
    let mut reply = serde_json::to_vec(&reply).map_err(|e| e.to_string())?;
    reply.push(b'\n');
    stream.write_all(&reply).await.map_err(|e| e.to_string())?;
    Ok(reason.is_none())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_acl_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("helper/instance.json");
        let r = Record {
            protocol: 1,
            instance_id: random_hex(16).unwrap(),
            pipe: "test".into(),
            token: random_hex(32).unwrap(),
            pid: std::process::id(),
            started_at: process_started_at().unwrap(),
        };
        write_record(&path, &r).unwrap();
        assert_eq!(
            serde_json::from_slice::<Record>(&std::fs::read(&path).unwrap()).unwrap(),
            r
        );
        let security = UserSecurity::new().unwrap();
        assert!(security.sddl().starts_with("D:P(A;OICI;GA;;;"));
        use std::os::windows::io::AsRawHandle;
        let file = std::fs::File::open(&path).unwrap();
        assert_eq!(
            super::super::security::handle_sddl(file.as_raw_handle()),
            security.sddl().replace(";GA;", ";FA;")
        );
    }
    #[tokio::test]
    async fn real_pipe_has_current_user_only_acl() {
        use std::os::windows::io::AsRawHandle;
        let r = Record {
            protocol: 1,
            instance_id: random_hex(16).unwrap(),
            pipe: format!(r"\\.\pipe\goodtype-helper-test-{}", random_hex(16).unwrap()),
            token: random_hex(32).unwrap(),
            pid: std::process::id(),
            started_at: process_started_at().unwrap(),
        };
        let pipe = instance(&r, true).unwrap();
        // Pipe objects do not carry directory inheritance flags.
        let actual = super::super::security::handle_sddl(pipe.as_raw_handle());
        assert_eq!(
            actual.replace("OICI", ""),
            UserSecurity::new()
                .unwrap()
                .sddl()
                .replace("OICI", "")
                .replace(";GA;", ";FA;")
        );
    }
    #[tokio::test]
    async fn handshake_accepts_only_matching_token_and_protocol() {
        let r = Record {
            protocol: 1,
            instance_id: "id".into(),
            pipe: "test".into(),
            token: "secret".into(),
            pid: 0,
            started_at: 0,
        };
        for (token, protocol, ok) in [("secret", 1, true), ("bad", 1, false), ("secret", 2, false)]
        {
            let (mut client, mut server) = tokio::io::duplex(8192);
            let r = r.clone();
            let task =
                tokio::spawn(async move { handshake(&mut server, &r, true, true).await.unwrap() });
            client.write_all(format!("{{\"type\":\"goodtype-relay-hello\",\"instanceId\":\"id\",\"token\":\"{token}\",\"protocol\":{protocol}}}\n").as_bytes()).await.unwrap();
            assert_eq!(task.await.unwrap(), ok);
        }
    }
}

pub(super) fn admit_session(
    helper: &Helper,
    generation: u64,
    mut session: super::Session,
) -> Option<tokio::sync::watch::Receiver<bool>> {
    let mut s = helper.inner.lock().unwrap_or_else(|e| e.into_inner());
    if !s.settings.enabled
        || s.enable_generation != generation
        || session
            .launch_cancelled
            .as_ref()
            .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
    {
        return None;
    }
    if let Some(old) = &session.request {
        let mut current = s.requests.get(&old.request_id)?.clone();
        if current.conversation_id != old.conversation_id {
            return None;
        }
        current.session_id = session.info.id.clone();
        current.epoch = session.epoch;
        current.info.session_ids = vec![session.info.id.clone()];
        session.run_share = Some((current.anchor.clone(), current.info.clone()));
        session.request = Some(std::sync::Arc::new(current));
    }
    let stop = session.stop.subscribe();
    s.sessions.insert(session.info.id.clone(), session);
    Some(stop)
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    #[test]
    fn enabled_settings_from_plain_thread_create_live_pipe() {
        std::thread::spawn(|| {
            assert!(tokio::runtime::Handle::try_current().is_err());
            let helper = Helper::default();
            helper
                .apply_settings(
                    super::super::HelperSettings {
                        enabled: true,
                        ..Default::default()
                    },
                    || {
                        let record = Record {
                            protocol: 1,
                            instance_id: "test".into(),
                            pipe: format!(r"\\.\pipe\goodtype-runtime-test-{}", random_hex(16)?),
                            token: "test".into(),
                            pid: 0,
                            started_at: 0,
                        };
                        let server = instance(&record, true)?;
                        let client = tokio::net::windows::named_pipe::ClientOptions::new()
                            .open(&record.pipe)
                            .map_err(|e| e.to_string())?;
                        tauri::async_runtime::block_on(server.connect())
                            .map_err(|e| e.to_string())?;
                        drop(client);
                        Ok(())
                    },
                )
                .unwrap();
        })
        .join()
        .unwrap();
    }
    #[test]
    fn old_listener_generation_cannot_admit_after_disable_reenable() {
        let helper = Helper::default();
        helper.inner.lock().unwrap().settings.enabled = true;
        let old = helper.inner.lock().unwrap().enable_generation;
        helper.stop();
        helper.inner.lock().unwrap().settings.enabled = true;
        assert!(
            admit_session(
                &helper,
                old,
                super::super::bridge::new_session("old".into())
            )
            .is_none()
        );
        let generation = helper.inner.lock().unwrap().enable_generation;
        let stop = admit_session(
            &helper,
            generation,
            super::super::bridge::new_session("new".into()),
        )
        .unwrap();
        helper.stop();
        assert!(*stop.borrow());
    }
}

#[cfg(test)]
mod launch_tests {
    use super::*;
    #[tokio::test]
    async fn launch_tokens_single_use_expiry_and_binding() {
        use super::super::*;
        let helper = Helper::default();
        let share = SharePayload {
            root: "run-root".into(),
            page_id: "p".into(),
            object_ids: vec!["run-object".into()],
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
        for (token, age) in [("valid", 0), ("expired", 61)] {
            helper.inner.lock().unwrap().pending_launches.insert(
                token.into(),
                codex::Launch {
                    grant: None,
                    cancelled: Default::default(),
                    created: std::time::Instant::now() - Duration::from_secs(age),
                    conversation: "conversation".into(),
                    share: share.clone(),
                    info: info.clone(),
                },
            );
        }
        let record = Record {
            protocol: 1,
            instance_id: "instance".into(),
            pipe: "unused".into(),
            token: "discovery".into(),
            pid: 1,
            started_at: 1,
        };
        for (token, accepted) in [
            ("valid", true),
            ("valid", false),
            ("expired", false),
            ("unknown", false),
        ] {
            let (mut client, mut stream) = tokio::io::duplex(8192);
            client.write_all(format!("{{\"type\":\"goodtype-relay-hello\",\"protocol\":1,\"instanceId\":\"instance\",\"token\":\"discovery\",\"launchToken\":\"{token}\"}}\n").as_bytes()).await.unwrap();
            let mut session = bridge::new_session("session".into());
            assert_eq!(
                handshake_bound(
                    &mut stream,
                    &record,
                    true,
                    true,
                    Some((&helper, &mut session))
                )
                .await
                .unwrap(),
                accepted
            );
            if accepted {
                assert!(session.info.approved);
                assert_eq!(session.conversation.as_deref(), Some("conversation"));
                let (bound, info) = session.run_share.unwrap();
                assert_eq!(bound.object_ids, ["run-object"]);
                assert_eq!(info.session_ids, ["session"]);
            }
        }
        assert!(helper.inner.lock().unwrap().pending_launches.is_empty());
    }
}

#[cfg(test)]
mod close_race_tests {
    use super::*;
    #[test]
    fn close_between_hello_and_admission_cannot_resurrect_grants() {
        let h = Helper::default();
        h.inner.lock().unwrap().settings.enabled = true;
        let mut session = super::super::bridge::new_session("closed".into());
        session.conversation = Some("conversation".into());
        session.info.approved = true;
        session.launch_cancelled = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            true,
        )));
        assert!(
            session
                .admit(super::super::Usage::Low, "get_context")
                .is_err()
        );
        assert!(admit_session(&h, 0, session).is_none());
        assert!(h.inner.lock().unwrap().sessions.is_empty());
    }
}
