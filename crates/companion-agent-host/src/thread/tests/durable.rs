use super::*;

const CONFIG: &str = r#"{"provider":"openrouter","modelId":"~anthropic/claude-haiku-latest"}"#;
const DOCUMENT: &str = r#"{"documentId":"crash-test","title":"Crash PDF","pageCount":2,"provider":"openrouter","modelId":"~anthropic/claude-haiku-latest"}"#;

fn state(hub: Arc<companion_store::SqliteHub>, port: u16, events: Sender<Event>) -> AgentHostState {
    let secrets = Arc::new(MemorySecretStore::default());
    secrets.write("openrouter", "test-openrouter-key").unwrap();
    AgentHostState::with_secrets(
        Abort::default(),
        Policy {
            loopback_port: Some(port),
        },
        events,
        secrets,
        hub,
    )
}

/// Real Wasm/Harness checkpoints survive loss of the Store mid-generation.
/// No close/abort export is called on the crashed instance.
#[test]
fn crash_checkpoint_resume_and_conversation_reattach() {
    let _serial = serial();
    let runtime = runtime();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("companion.sqlite");
    let hub = Arc::new(companion_store::SqliteHub::open(&path).unwrap());
    hub.write(|c| {
        c.execute("INSERT INTO documents(id,title,source_path,stored_path,sha256) VALUES('crash-test','Crash PDF','','','crash')", [])?;
        c.execute("INSERT INTO notes(document_id,path,content,mtime_ms) VALUES('crash-test','test',?,1)", [vec![0u8,255]])?;
        Ok(())
    }).unwrap();
    let (port, server) = model_server();
    let (events, received) = crossbeam_channel::unbounded();
    let worker_runtime = runtime.clone();
    let worker_hub = hub.clone();
    let worker = std::thread::spawn(move || {
        let (mut store, guest) =
            wasm::instantiate(&worker_runtime, state(worker_hub, port, events)).unwrap();
        guest.call_open(&mut store, CONFIG).unwrap().unwrap();
        let json = guest
            .call_open_document(&mut store, DOCUMENT)
            .unwrap()
            .unwrap();
        let id = serde_json::from_str::<serde_json::Value>(&json).unwrap()["conversationId"]
            .as_i64()
            .unwrap();
        let req = serde_json::json!({"conversationId":id,"requestId":"crash-request","content":"Hello crash test"}).to_string();
        let error = guest
            .call_submit(&mut store, &req)
            .expect_err("epoch must trap submit");
        assert!(format!("{error:#}").contains("interrupt"));
        // Drop resource table and release any abandoned transaction, no exports.
        drop(store);
        id
    });
    let mut token_snapshots = 0;
    loop {
        match received.recv_timeout(Duration::from_secs(300)).unwrap() {
            Event::Chunk { json, .. } if json.contains("token") => {
                token_snapshots += 1;
                if token_snapshots == 4 {
                    runtime.engine.increment_epoch();
                    break;
                }
            }
            Event::Report(json) => eprintln!("CRASH REPORT {json}"),
            _ => (),
        }
    }
    let id = worker.join().unwrap();
    server.join().unwrap();
    // Snapshot every pi_ table before running app migrations on a fresh hub.
    let snapshot = |hub: &companion_store::SqliteHub| -> Vec<(String, Vec<String>)> {
        hub.write(|c| {
            let names = c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name GLOB 'pi_*' ORDER BY name")?
                .query_map([], |r| r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
            names.into_iter().map(|name| {
                let mut statement = c.prepare(&format!("SELECT * FROM \"{name}\""))?;
                let columns = statement.column_count();
                let mut rows = statement.query_map([], |r| {
                    let values = (0..columns).map(|i| r.get::<_, rusqlite::types::Value>(i)).collect::<Result<Vec<_>,_>>()?;
                    Ok(format!("{values:?}"))
                })?.collect::<Result<Vec<_>,_>>()?;
                rows.sort();
                Ok((name, rows))
            }).collect()
        }).unwrap()
    };
    let before = snapshot(&hub);
    assert!(
        !before.is_empty(),
        "pi-durable must create its own pi_ schema"
    );
    drop(hub);
    let hub = Arc::new(companion_store::SqliteHub::open(&path).unwrap());
    assert_eq!(
        snapshot(&hub),
        before,
        "app migrations must not recreate pi_ tables"
    );
    hub.write(|c| {
        let blob: Vec<u8> = c.query_row("SELECT content FROM notes", [], |r| r.get(0))?;
        assert_eq!(blob, vec![0, 255]);
        assert_eq!(
            c.query_row(
                "SELECT conversation_id FROM conversations_map WHERE document_id='crash-test'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            id
        );
        Ok(())
    })
    .unwrap();
    let (port, resumed_server) = model_server();
    let (events, resumed) = crossbeam_channel::unbounded();
    let (mut store, guest) = wasm::instantiate(&runtime, state(hub.clone(), port, events)).unwrap();
    guest.call_open(&mut store, CONFIG).unwrap().unwrap();
    let json = guest
        .call_open_document(&mut store, DOCUMENT)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap()["conversationId"],
        id
    );
    guest.call_resume_wait(&mut store).unwrap().unwrap();
    let mut commits = Vec::new();
    let chunks = resumed
        .try_iter()
        .filter_map(|event| match event {
            Event::Chunk { json, .. } => Some(json),
            Event::Commit(json) => {
                commits.push(json);
                None
            }
            Event::Report(json) => {
                eprintln!("RESUME REPORT {json}");
                None
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        // Live snapshots may coalesce the final token into the committed reply.
        !chunks.is_empty() && commits.iter().any(|json| json.contains("token15")),
        "resume-wait must commit the finished checkpointed generation: {commits:?}"
    );
    // Exactly-once request id is retained through crash recovery: submitting it
    // again settles from storage without contacting a third HTTP server.
    let req = serde_json::json!({"conversationId":id,"requestId":"crash-request","content":"Hello crash test"}).to_string();
    let settled = guest.call_submit(&mut store, &req).unwrap().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&settled).unwrap()["status"],
        "done"
    );
    guest.call_close(&mut store).unwrap();
    resumed_server.join().unwrap();
    assert!(snapshot(&hub).iter().any(|(_, rows)| !rows.is_empty()));
}
