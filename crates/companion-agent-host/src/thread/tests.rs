mod durable;
mod pdf;

use super::*;
use crate::{secrets::MemorySecretStore, wasm::Event};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

fn test_hub() -> Arc<companion_store::SqliteHub> {
    Arc::new(companion_store::SqliteHub::open(":memory:").unwrap())
}

fn server(delay_headers: bool, delay_body: bool) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.set_nodelay(true).unwrap();
        let mut request = [0; 4096];
        let _ = socket.read(&mut request);
        if delay_headers {
            std::thread::sleep(Duration::from_secs(5));
        }
        if socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").is_err() { return; }
        if delay_body {
            std::thread::sleep(Duration::from_secs(5));
        }
        for i in 0..30 {
            let data = format!("data: {{\"sequence\":{i}}}\n\n");
            let chunk = format!("{:x}\r\n{}\r\n", data.len(), data);
            if socket.write_all(chunk.as_bytes()).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = socket.write_all(b"0\r\n\r\n");
    });
    port
}

fn run_case(
    runtime: &wasm::Runtime,
    name: &str,
    abort_chunks: Option<usize>,
    delayed_headers: bool,
    delayed_body: bool,
) {
    let port = server(delayed_headers, delayed_body);
    let abort = Abort::default();
    let (tx, rx) = crossbeam_channel::unbounded();
    let (events, received) = crossbeam_channel::unbounded();
    let secrets = Arc::new(MemorySecretStore::default());
    secrets.write("openrouter", "test-key").unwrap();
    let thread = spawn_with_secrets(
        runtime.clone(),
        abort.clone(),
        Policy {
            loopback_port: Some(port),
        },
        secrets,
        rx,
        events,
        test_hub(),
    );
    match received.recv_timeout(Duration::from_secs(300)).unwrap() {
        Event::Ready {
            cold_start_ms,
            compile_ms,
        } => println!(
            "COLD_START {name} instantiate={cold_start_ms:.3} ms compile={compile_ms:.3} ms"
        ),
        e => panic!("expected ready: {e:?}"),
    }
    let start = Instant::now();
    tx.send(Command::Sse(format!("http://127.0.0.1:{port}/sse")))
        .unwrap();
    let timer = if delayed_headers || delayed_body {
        let abort = abort.clone();
        Some(std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            abort.request();
            Instant::now()
        }))
    } else {
        None
    };
    let mut stamps = Vec::new();
    let mut aborted_at = None;
    let result = loop {
        match received
            .recv_timeout(Duration::from_secs(10))
            .expect("export must return promptly")
        {
            Event::Chunk { at, json } => {
                let value: serde_json::Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["sequence"].as_u64().unwrap(), stamps.len() as u64);
                let ms = at.duration_since(start).as_secs_f64() * 1000.;
                println!("CHUNK {name} {} {ms:.3} ms", stamps.len());
                stamps.push(at);
                if abort_chunks == Some(stamps.len()) {
                    aborted_at = Some(Instant::now());
                    abort.request();
                }
            }
            Event::Done(result) => break result,
            e => panic!("unexpected: {e:?}"),
        }
    };
    let elapsed = start.elapsed();
    if let Some(timer) = timer {
        aborted_at = Some(timer.join().unwrap());
    }
    println!(
        "DONE {name} {:.3} ms result={result:?}",
        elapsed.as_secs_f64() * 1000.
    );
    if let Some(at) = aborted_at {
        assert!(result.is_err(), "abort must fail fetch/body");
        let latency = at.elapsed();
        println!("ABORT {name} {:.3} ms", latency.as_secs_f64() * 1000.);
        assert!(latency < Duration::from_secs(1));
        if delayed_headers || delayed_body {
            assert!(elapsed < Duration::from_secs(1));
        }
    } else {
        assert!(result.is_ok());
        assert!(stamps.len() >= 10, "must see incremental chunks");
        assert!(stamps[0].duration_since(start) < Duration::from_secs(1));
        assert!(stamps.last().unwrap().duration_since(stamps[0]) > Duration::from_secs(2));
        let good_gaps = stamps
            .windows(2)
            .filter(|w| {
                let gap = w[1].duration_since(w[0]);
                gap > Duration::from_millis(50) && gap < Duration::from_millis(250)
            })
            .count();
        assert!(good_gaps >= 10, "chunks must arrive ~100ms apart");
    }
    let _ = tx.send(Command::Close);
    thread.join().unwrap().unwrap();
}

fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn runtime() -> wasm::Runtime {
    static RUNTIME: std::sync::OnceLock<wasm::Runtime> = std::sync::OnceLock::new();
    RUNTIME
        .get_or_init(|| wasm::Runtime::load().unwrap())
        .clone()
}

#[test]
fn spike_sse() {
    let _serial = serial();
    let runtime = runtime();
    run_case(&runtime, "incremental", None, false, false);
    run_case(&runtime, "mid-body", Some(5), false, false);
    run_case(&runtime, "ttfb", None, true, false);
    run_case(&runtime, "headers-no-body", None, false, true);

    let engine = runtime.engine.clone();
    let abort = Abort::default();
    let (tx, rx) = crossbeam_channel::unbounded();
    let (events, received) = crossbeam_channel::unbounded();
    let thread = spawn(
        runtime,
        abort.clone(),
        Policy::default(),
        rx,
        events,
        test_hub(),
    );
    assert!(matches!(
        received.recv_timeout(Duration::from_secs(300)).unwrap(),
        Event::Ready { .. }
    ));
    tx.send(Command::Busy).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let at = Instant::now();
    abort.request();
    engine.increment_epoch();
    match received.recv_timeout(Duration::from_secs(1)).unwrap() {
        Event::Done(Err(error)) => {
            println!("EPOCH {:.3} ms {error}", at.elapsed().as_secs_f64() * 1000.);
            assert!(error.contains("interrupt"));
        }
        e => panic!("expected epoch trap: {e:?}"),
    }
    thread.join().unwrap().unwrap();
    println!("VERDICT: PASS (on-stack incremental fetch, mid-body/TTFB abort, epoch busy-loop)");
}

fn model_server() -> (u16, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let thread = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.set_nodelay(true).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut request = Vec::new();
        loop {
            let mut buf = [0; 4096];
            let n = socket.read(&mut buf).unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buf[..n]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                assert!(
                    headers.starts_with("post /api/v1/chat/completions "),
                    "{headers}"
                );
                assert!(headers.contains("authorization: bearer test-openrouter-key"));
                assert!(!headers.contains("guest-dummy-not-a-secret"));
                if let Some(length) = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                {
                    if request.len() >= end + 4 + length.parse::<usize>().unwrap() {
                        break;
                    }
                } else {
                    assert!(headers.contains("transfer-encoding: chunked"));
                    if request[end + 4..].ends_with(b"0\r\n\r\n") {
                        break;
                    }
                }
            }
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
        for i in 0..16 {
            let event = serde_json::json!({"id":"mock", "object":"chat.completion.chunk", "choices":[{"index":0,"delta":{"content":format!("token{i} ")},"finish_reason":null}]});
            let data = format!("data: {event}\n\n");
            if write!(socket, "{:x}\r\n{}\r\n", data.len(), data).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let data = "data: {\"id\":\"mock\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        let _ = write!(socket, "{:x}\r\n{}\r\n0\r\n\r\n", data.len(), data);
    });
    (port, thread)
}

fn real_submit(abort_mid_stream: bool) {
    let (port, server) = model_server();
    let hub = test_hub();
    hub.write(|c| {
        c.execute("INSERT INTO documents(id,title,source_path,stored_path,sha256) VALUES ('local-test','Mock PDF','','','test')", [])?;
        Ok(())
    }).unwrap();
    let (tx, rx) = crossbeam_channel::unbounded();
    let (events, received) = crossbeam_channel::unbounded();
    let secrets = Arc::new(MemorySecretStore::default());
    secrets.write("openrouter", "test-openrouter-key").unwrap();
    let abort = Abort::default();
    let thread = spawn_with_secrets(
        runtime(),
        abort.clone(),
        Policy {
            loopback_port: Some(port),
        },
        secrets.clone(),
        rx,
        events,
        hub.clone(),
    );
    assert!(matches!(
        received.recv_timeout(Duration::from_secs(300)).unwrap(),
        Event::Ready { .. }
    ));
    tx.send(Command::Open { config_json: serde_json::json!({"provider":"openrouter", "modelId":"~anthropic/claude-haiku-latest", "prefix":"pi_"}).to_string() }).unwrap();
    loop {
        match received.recv_timeout(Duration::from_secs(10)).unwrap() {
            Event::Done(result) => {
                result.unwrap();
                break;
            }
            Event::Report(json) => assert!(json.contains("models")),
            other => panic!("open: {other:?}"),
        }
    }
    tx.send(Command::OpenDocument { req_json: serde_json::json!({"documentId":"local-test", "title":"Mock PDF", "pageCount":1, "provider":"openrouter", "modelId":"~anthropic/claude-haiku-latest"}).to_string() }).unwrap();
    let mut id = 0;
    loop {
        match received.recv_timeout(Duration::from_secs(10)).unwrap() {
            Event::OpenDocumentDone(json) => {
                id = serde_json::from_str::<serde_json::Value>(&json).unwrap()["conversationId"]
                    .as_i64()
                    .unwrap();
            }
            Event::Done(result) => {
                result.unwrap();
                break;
            }
            Event::Commit(_) => (),
            other => panic!("open document: {other:?}"),
        }
    }
    assert!(id > 0);
    for command in [
        Command::SetFollow { conversation_id: id, enabled: false },
        Command::Configure { conversation_id: id, agent_json: serde_json::json!({"model":{"provider":"openrouter","modelId":"~anthropic/claude-haiku-latest"}}).to_string() },
        Command::ResumeWait,
    ] {
        tx.send(command).unwrap();
        loop {
            match received.recv_timeout(Duration::from_secs(10)).unwrap() {
                Event::Done(result) => { result.unwrap(); break; }
                Event::Commit(_) | Event::Report(_) => (),
                other => panic!("configuration: {other:?}"),
            }
        }
    }
    let started = Instant::now();
    tx.send(Command::Submit { conversation_id: id, req_json: serde_json::json!({"conversationId":id, "content":"Hello local mock", "requestId":"mock-request-1"}).to_string() }).unwrap();
    let mut stamps = Vec::new();
    let mut last_tokens = 0;
    let mut aborted_at = None;
    let mut abort_export = false;
    let mut commits = 0;
    let mut submission_done = false;
    loop {
        match received
            .recv_timeout(Duration::from_secs(10))
            .expect("on-stack submit must finish")
        {
            Event::Chunk { at, json } => {
                let tokens = json.matches("token").count();
                if tokens > last_tokens {
                    last_tokens = tokens;
                    stamps.push(at);
                    if abort_mid_stream && stamps.len() == 4 {
                        aborted_at = Some(Instant::now());
                        abort.request();
                    }
                }
            }
            Event::Commit(_) => {
                commits += 1;
            }
            Event::Report(json) => eprintln!("SUBMIT REPORT {json}"),
            Event::Aborted(conv) => {
                assert_eq!(conv, id);
                abort_export = true;
            }
            Event::SubmitDone(json) => {
                let res: serde_json::Value = serde_json::from_str(&json).unwrap();
                assert_eq!(res["conversationId"], id);
                if !abort_mid_stream {
                    assert_eq!(res["status"], "done");
                }
                submission_done = true;
            }
            Event::Done(result) => {
                result.unwrap();
                break;
            }
            other => panic!("submit: {other:?}"),
        }
    }
    assert!(submission_done);
    println!(
        "HARNESS abort={abort_mid_stream} snapshots={} tokens={last_tokens} elapsed_ms={:.3}",
        stamps.len(),
        started.elapsed().as_secs_f64() * 1000.
    );
    assert!(stamps.len() >= 4, "real pi.live must stream incrementally");
    assert!(stamps.last().unwrap().duration_since(stamps[0]) > Duration::from_millis(200));
    if let Some(at) = aborted_at {
        assert!(at.elapsed() < Duration::from_secs(1));
        assert!(abort_export, "abort export must run after submit unwinds");
    } else {
        // Durable's 100 ms partial policy may coalesce adjacent 100 ms SSE
        // chunks. Assert growing text over time, not one event per raw chunk.
        assert!(stamps[0].duration_since(started) < Duration::from_secs(1));
        assert!(stamps.last().unwrap().duration_since(stamps[0]) > Duration::from_millis(800));
        assert!(commits < stamps.len(), "commits must not be per-token");
    }
    tx.send(Command::Close).unwrap();
    thread.join().unwrap().unwrap();
    server.join().unwrap();
    if abort_mid_stream {
        // A completed Esc is durable too: restart must not resurrect the run.
        let (events, received) = crossbeam_channel::unbounded();
        let state = AgentHostState::with_secrets(
            Abort::default(),
            Policy {
                loopback_port: Some(port),
            },
            events,
            secrets,
            hub,
        );
        let (mut store, guest) = wasm::instantiate(&runtime(), state).unwrap();
        guest
            .call_open(
                &mut store,
                r#"{"provider":"openrouter","modelId":"~anthropic/claude-haiku-latest"}"#,
            )
            .unwrap()
            .unwrap();
        let json = guest.call_open_document(&mut store, r#"{"documentId":"local-test","title":"Mock PDF","pageCount":1,"provider":"openrouter","modelId":"~anthropic/claude-haiku-latest"}"#).unwrap().unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).unwrap()["conversationId"],
            id
        );
        guest.call_resume_wait(&mut store).unwrap().unwrap();
        for event in received.try_iter() {
            match event {
                Event::Chunk { json, .. } => {
                    assert!(!json.contains("token"), "Esc must not resume: {json}")
                }
                Event::Report(json) => {
                    assert!(json.contains("models"), "no model retry after Esc: {json}")
                }
                _ => (),
            }
        }
        guest.call_close(&mut store).unwrap();
    }
}

#[test]
fn sqlite_harness_openrouter_submit_and_escape() {
    let _serial = serial();
    real_submit(false);
    real_submit(true);
}

#[test]
fn wasi_environment_is_empty() {
    let (events, _) = crossbeam_channel::unbounded();
    let mut state = AgentHostState::new(Abort::default(), Policy::default(), events, test_hub());
    assert!(state.wasi.cli().environment.is_empty());
}

#[test]
fn trap_reinstantiates_and_opens_before_resume() {
    let _serial = serial();
    let runtime = runtime();
    let engine = runtime.engine.clone();
    let (tx, rx) = crossbeam_channel::unbounded();
    let (events, received) = crossbeam_channel::unbounded();
    let thread = spawn(
        runtime,
        Abort::default(),
        Policy::default(),
        rx,
        events,
        test_hub(),
    );
    assert!(matches!(
        received.recv_timeout(Duration::from_secs(300)).unwrap(),
        Event::Ready { .. }
    ));
    tx.send(Command::Open {
        config_json: "{\"provider\":\"openrouter\",\"modelId\":\"catalog-choice\"}".into(),
    })
    .unwrap();
    loop {
        match received.recv_timeout(Duration::from_secs(10)).unwrap() {
            Event::Report(_) => (),
            Event::Done(result) => {
                result.unwrap();
                break;
            }
            other => panic!("open: {other:?}"),
        }
    }
    tx.send(Command::Busy).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    engine.increment_epoch();
    let mut reopened = false;
    loop {
        match received.recv_timeout(Duration::from_secs(10)).unwrap() {
            Event::Ready { .. } => (),
            Event::Report(json) => {
                assert!(json.contains("models"));
                reopened = true;
            }
            Event::Trapped {
                message,
                abort_requested,
            } => {
                assert!(message.contains("interrupt"));
                assert!(!abort_requested);
                break;
            }
            other => panic!("recovery: {other:?}"),
        }
    }
    assert!(reopened, "new guest must open(config) before resume-wait");
    tx.send(Command::Close).unwrap();
    thread.join().unwrap().unwrap();
}
