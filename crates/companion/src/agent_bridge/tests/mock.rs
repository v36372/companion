//! Actual bridge -> sync component -> local SSE, no window or machine secrets.
use super::*;
use companion_agent_host::{
    http_proxy::{Abort, Policy},
    secrets::{MemorySecretStore, SecretStore},
    wasm::Runtime,
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

fn runtime() -> Runtime {
    static RUNTIME: std::sync::OnceLock<Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::load().unwrap()).clone()
}

fn pump(
    bridge: &mut Bridge,
    events: &async_channel::Receiver<Event>,
    until: impl Fn(&Bridge) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !until(bridge) {
        assert!(
            Instant::now() < deadline,
            "bridge timed out: {} {:?}",
            bridge.status,
            bridge.notices
        );
        if let Ok(event) = events.try_recv() {
            bridge.event(event);
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn local_sse_tokens_commit_queue_escape_and_document_switch() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        for turn in 0..4 {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_nodelay(true).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(20)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut buf = [0; 4096];
                let n = socket.read(&mut buf).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buf[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                    assert!(headers.starts_with("post /api/v1/chat/completions "));
                    assert!(headers.contains("authorization: bearer test-openrouter-key"));
                    let size: usize = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .unwrap()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + size {
                        let body: Value =
                            serde_json::from_slice(&request[end + 4..end + 4 + size]).unwrap();
                        let expected = match turn {
                            0 => "openai/gpt-4o-mini",
                            1 => "~anthropic/claude-sonnet-latest",
                            _ => "vendor/custom-model:v1",
                        };
                        assert_eq!(body["model"], expected);
                        if turn == 0 {
                            let body = String::from_utf8_lossy(&request[end + 4..]);
                            assert!(body.contains("BEGIN SELECTED PDF QUOTE"));
                            assert!(body.contains("page: 3"));
                            assert!(body.contains("occurrence: 2"));
                        }
                        break;
                    }
                }
            }
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
            for i in 0..12 {
                let data = format!(
                    "data: {}\n\n",
                    json!({"id":"mock","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":format!("turn{turn}-token{i} ")},"finish_reason":null}]})
                );
                if write!(socket, "{:x}\r\n{}\r\n", data.len(), data).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let data = "data: {\"id\":\"mock\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            let _ = write!(socket, "{:x}\r\n{}\r\n0\r\n\r\n", data.len(), data);
        }
    });
    let hub = Arc::new(companion_store::SqliteHub::open(":memory:").unwrap());
    hub.write(|c| {
        c.execute("INSERT INTO documents(id,title,source_path,stored_path,sha256) VALUES ('A','A','','','a'),('B','B','','','b')", [])?;
        Ok(())
    }).unwrap();
    hub.set_model_id("openai/gpt-4o-mini").unwrap();
    let preferences = hub.clone();
    let abort = Abort::default();
    let (tx, commands) = crossbeam_channel::unbounded();
    let (out, events) = async_channel::unbounded();
    let secrets = Arc::new(MemorySecretStore::default());
    secrets.write("openrouter", "test-openrouter-key").unwrap();
    let runtime = runtime(); // shared compile for this test binary
    let run_abort = abort.clone();
    let worker = std::thread::spawn(move || {
        run_with(
            hub,
            runtime,
            run_abort,
            Policy::mock_provider(&format!("http://127.0.0.1:{port}")).unwrap(),
            secrets,
            commands,
            &out,
        )
        .unwrap()
    });
    let mut bridge = Bridge::off();
    bridge.tx = Some(tx);
    let cancel = abort.clone();
    bridge.cancel = Arc::new(move || cancel.request());
    bridge.prepare = Arc::new(move || abort.reset());
    pump(&mut bridge, &events, |b| {
        b.availability == Availability::Ready
    });
    bridge.switch_document(attachment("A"));
    pump(&mut bridge, &events, |b| b.conversation.is_some());
    bridge.submit(
        "Explain".into(),
        Some(json!({"page":3,"quote":"local quote","occurrence":2})),
    );
    pump(&mut bridge, &events, |b| b.live.contains("token"));
    assert!(bridge.running);
    // The active HTTP request keeps its original model. The queued next turn
    // must configure the same durable conversation, without a restart.
    preferences
        .set_model_id("~anthropic/claude-sonnet-latest")
        .unwrap();
    bridge.submit("follow-up".into(), None);
    assert_eq!(bridge.queued(), 1);
    pump(&mut bridge, &events, |b| !b.running && b.queued() == 0);
    assert!(
        bridge
            .transcript
            .iter()
            .any(|s| s.text.contains("turn0-token11"))
    );
    assert!(
        bridge
            .transcript
            .iter()
            .any(|s| s.text.contains("turn1-token11"))
    );
    assert_eq!(
        bridge
            .transcript
            .iter()
            .find_map(|s| s.quote.as_ref())
            .unwrap()
            .label(),
        "p. 3 · “local quote”"
    );
    assert!(
        bridge
            .transcript
            .iter()
            .all(|s| !s.text.contains("---BEGIN"))
    );
    preferences.set_model_id("vendor/custom-model:v1").unwrap();
    bridge.submit("cancel".into(), None);
    pump(&mut bridge, &events, |b| b.live.contains("turn2"));
    let at = Instant::now();
    bridge.abort();
    pump(&mut bridge, &events, |b| !b.running);
    assert!(at.elapsed() < Duration::from_secs(1));
    assert!(bridge.notices.iter().any(|s| s.contains("abort completed")));
    bridge.submit("switch away".into(), None);
    pump(&mut bridge, &events, |b| b.live.contains("turn3"));
    let old = bridge.conversation;
    bridge.switch_document(attachment("B"));
    assert!(bridge.conversation.is_none());
    pump(&mut bridge, &events, |b| b.conversation.is_some());
    assert_ne!(old, bridge.conversation);
    assert!(!bridge.running);
    assert!(bridge.notices.iter().any(|s| s.contains("abort completed")));
    bridge.switch_document(attachment("A"));
    pump(&mut bridge, &events, |b| b.conversation == old);
    assert_eq!(
        bridge
            .transcript
            .iter()
            .find_map(|s| s.quote.as_ref())
            .unwrap()
            .label(),
        "p. 3 · “local quote”"
    );
    assert!(
        bridge
            .transcript
            .iter()
            .all(|s| !s.text.contains("---BEGIN"))
    );
    bridge.close();
    worker.join().unwrap();
    server.join().unwrap();
}

#[test]
fn launch_reattaches_checkpoint_shows_resuming_and_finishes() {
    use companion_agent_host::wasm::{self, AgentHostState, Event as WasmEvent};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(20)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut buf = [0; 4096];
                let n = socket.read(&mut buf).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buf[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                    let size: usize = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .unwrap()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + size {
                        break;
                    }
                }
            }
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
            for i in 0..12 {
                let data = format!(
                    "data: {}\n\n",
                    json!({"id":"resume","choices":[{"index":0,"delta":{"content":format!("recovered{i} ")},"finish_reason":null}]})
                );
                if write!(socket, "{:x}\r\n{}\r\n", data.len(), data).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(60));
            }
            let data = "data: {\"id\":\"resume\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            let _ = write!(socket, "{:x}\r\n{}\r\n0\r\n\r\n", data.len(), data);
        }
    });
    let runtime = runtime();
    let hub = Arc::new(companion_store::SqliteHub::open(":memory:").unwrap());
    hub.write(|c| { c.execute("INSERT INTO documents(id,title,source_path,stored_path,sha256) VALUES('A','A','','','a')", [])?; Ok(()) }).unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    secrets.write("openrouter", "test-openrouter-key").unwrap();
    let policy = Policy::mock_provider(&format!("http://127.0.0.1:{port}")).unwrap();
    let (out, incoming) = crossbeam_channel::unbounded();
    let crash_runtime = runtime.clone();
    let state = AgentHostState::with_secrets(
        Abort::default(),
        policy.clone(),
        out,
        secrets.clone(),
        hub.clone(),
    );
    let crashed = std::thread::spawn(move || {
        let (mut store, guest) = wasm::instantiate(&crash_runtime, state).unwrap();
        guest
            .call_open(
                &mut store,
                r#"{"provider":"openrouter","modelId":"~anthropic/claude-haiku-latest"}"#,
            )
            .unwrap()
            .unwrap();
        let attached = guest.call_open_document(&mut store, r#"{"documentId":"A","title":"A","pageCount":1,"provider":"openrouter","modelId":"~anthropic/claude-haiku-latest"}"#).unwrap().unwrap();
        let id = serde_json::from_str::<Value>(&attached).unwrap()["conversationId"]
            .as_i64()
            .unwrap();
        assert!(guest.call_submit(&mut store, &json!({"conversationId":id,"requestId":"crashed-request","content":"Recover this"}).to_string()).is_err());
        drop(store); // process loss: no close/abort
        id
    });
    loop {
        if let WasmEvent::Chunk { json, .. } =
            incoming.recv_timeout(Duration::from_secs(300)).unwrap()
            && json.contains("recovered")
        {
            runtime.engine.increment_epoch();
            break;
        }
    }
    let id = crashed.join().unwrap();
    let abort = Abort::default();
    let cancel = abort.clone();
    let prepare = abort.clone();
    let (tx, commands) = crossbeam_channel::unbounded();
    let (out, events) = async_channel::unbounded();
    let worker = std::thread::spawn(move || {
        run_with(hub, runtime, abort, policy, secrets, commands, &out).unwrap()
    });
    let mut bridge = Bridge::off();
    bridge.tx = Some(tx);
    bridge.cancel = Arc::new(move || cancel.request());
    bridge.prepare = Arc::new(move || prepare.reset());
    pump(&mut bridge, &events, |b| {
        b.availability == Availability::Ready
    });
    bridge.switch_document(attachment("A"));
    pump(&mut bridge, &events, |b| b.status.contains("Resuming"));
    assert!(bridge.running);
    pump(&mut bridge, &events, |b| {
        !b.running && b.conversation == Some(id)
    });
    assert!(
        bridge
            .transcript
            .iter()
            .any(|t| t.text.contains("recovered11"))
    );
    assert_eq!(bridge.status, "Ready");
    bridge.close();
    worker.join().unwrap();
    server.join().unwrap();
}
