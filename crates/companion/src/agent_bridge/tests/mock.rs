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
    let abort = Abort::default();
    let (tx, commands) = crossbeam_channel::unbounded();
    let (out, events) = async_channel::unbounded();
    let secrets = Arc::new(MemorySecretStore::default());
    secrets.write("openrouter", "test-openrouter-key").unwrap();
    let runtime = Runtime::load().unwrap(); // once for all four scenarios
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
    pump(&mut bridge, &events, |b| b.available);
    bridge.switch_document(attachment("A"));
    pump(&mut bridge, &events, |b| b.conversation.is_some());
    bridge.submit(
        "Explain".into(),
        Some(json!({"page":3,"quote":"local quote","occurrence":2})),
    );
    pump(&mut bridge, &events, |b| b.live.contains("token"));
    assert!(bridge.running);
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
