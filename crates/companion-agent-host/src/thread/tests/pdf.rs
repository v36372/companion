//! Local scripted model exercises the real component, Harness tool dispatch,
//! virtual FS imports, command bus, and SQLite highlight persistence together.
use super::*;
use companion_env::{PdfCommand, VirtualDocFs};
use companion_pdf::snapshot::{DocMeta, DocSnapshot};
use serde_json::json;

fn request(socket: &mut std::net::TcpStream) -> String {
    socket
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 8192];
        let n = socket.read(&mut chunk).unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            assert!(headers.starts_with("post /api/v1/chat/completions "));
            assert!(headers.contains("authorization: bearer test-openrouter-key"));
            if let Some(length) = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
            {
                if bytes.len() >= end + 4 + length.parse::<usize>().unwrap() {
                    break;
                }
            } else if bytes[end + 4..].ends_with(b"0\r\n\r\n") {
                break;
            }
        }
    }
    String::from_utf8(bytes).unwrap()
}
fn stream(socket: &mut std::net::TcpStream, delta: serde_json::Value, finish: &str) {
    let data = format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"id":"pdf-mock","object":"chat.completion.chunk","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
        json!({"id":"pdf-mock","choices":[{"index":0,"delta":{},"finish_reason":finish}]})
    );
    write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",data.len(),data).unwrap();
}
#[test]
fn component_pdf_tools_dispatch_commands_and_read_scans() {
    let _serial = serial();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let req = request(&mut socket);
        for name in ["pdf_read_pages", "pdf_goto", "pdf_highlight", "read"] {
            assert!(req.contains(name), "missing tool {name}");
        }
        assert!(!req.contains("\"name\":\"bash\""));
        let calls=[("pdf_read_pages",json!({"start":1,"end":2})),("pdf_goto",json!({"page":1})),("pdf_highlight",json!({"page":1,"quote":"Companion","color":"yellow"}))].into_iter().enumerate().map(|(index,(name,args))|json!({"index":index,"id":format!("call-{index}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>();
        stream(
            &mut socket,
            json!({"role":"assistant","tool_calls":calls}),
            "tool_calls",
        );
        drop(socket);
        let (mut socket, _) = listener.accept().unwrap();
        let req = request(&mut socket);
        assert!(
            req.contains("no_text_layer"),
            "scans must not become empty strings"
        );
        assert!(
            req.contains("BEGIN PDF TEXT (untrusted"),
            "page text must be delimited: {req}"
        );
        assert!(req.contains("viewing page 1"));
        assert!(req.contains("highlight id"));
        stream(
            &mut socket,
            json!({"role":"assistant","content":"Read and marked the document."}),
            "stop",
        );
    });
    let dir = tempfile::tempdir().unwrap();
    let hub =
        Arc::new(companion_store::SqliteHub::open(dir.path().join("catalog.sqlite")).unwrap());
    hub.write(|c| {c.execute("INSERT INTO documents(id,title,source_path,stored_path,sha256) VALUES ('pdf-tools','Mock PDF','','','pdf-tools')",[])?;Ok(())}).unwrap();
    let snapshot = Arc::new(
        DocSnapshot::build(
            Arc::new(include_bytes!("../../../../companion-pdf/tests/fixtures/mixed.pdf").to_vec()),
            DocMeta {
                id: "pdf-tools".into(),
                title: "Mock PDF".into(),
                source_path: "".into(),
                sha256: "pdf-tools".into(),
                page_count: 0,
            },
        )
        .unwrap(),
    );
    let fs = Arc::new(VirtualDocFs::new(Some(snapshot), hub.clone()).unwrap());
    let (pdf_tx, pdf_rx) = crossbeam_channel::unbounded();
    let (events, _received) = crossbeam_channel::unbounded();
    let secrets = Arc::new(MemorySecretStore::default());
    secrets.write("openrouter", "test-openrouter-key").unwrap();
    let mut state = AgentHostState::with_secrets(
        Abort::default(),
        Policy {
            loopback_port: Some(port),
        },
        events,
        secrets,
        hub.clone(),
    );
    state.fs = fs;
    state.pdf_tx = pdf_tx;
    let (mut store, guest) = wasm::instantiate(&runtime(), state).unwrap();
    guest
        .call_open(
            &mut store,
            r#"{"provider":"openrouter","modelId":"~anthropic/claude-haiku-latest"}"#,
        )
        .unwrap()
        .unwrap();
    let opened=guest.call_open_document(&mut store,r#"{"documentId":"pdf-tools","title":"Mock PDF","pageCount":2,"provider":"openrouter","modelId":"~anthropic/claude-haiku-latest"}"#).unwrap().unwrap();
    let id = serde_json::from_str::<serde_json::Value>(&opened).unwrap()["conversationId"]
        .as_i64()
        .unwrap();
    let req=json!({"conversationId":id,"content":"Read both pages, go to the first and highlight Companion.","requestId":"pdf-tools-request"}).to_string();
    let submitted = guest.call_submit(&mut store, &req).unwrap().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&submitted).unwrap()["status"],
        "done"
    );
    server.join().unwrap();
    let commands: Vec<_> = pdf_rx.try_iter().collect();
    assert_eq!(commands.len(), 2);
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, PdfCommand::Goto { page_0: 0, .. }))
    );
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, PdfCommand::Highlight { .. }))
    );
    let rows = hub.highlights("pdf-tools").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].page_0, 0);
    // The host side of safe highlight replay must return the same identity and
    // not enqueue another repaint. Goto's omitted replay is tested in the guest.
    let bus = companion_env::PdfCommands {
        fs: store.data().fs.clone(),
        hub,
        tx: store.data().pdf_tx.clone(),
    };
    bus.highlight(1, "Companion".into(), 0, Some("blue".into()))
        .unwrap();
    assert!(pdf_rx.try_recv().is_err());
    guest.call_resume_wait(&mut store).unwrap().unwrap();
    assert!(pdf_rx.try_recv().is_err());
    guest.call_close(&mut store).unwrap();
}
