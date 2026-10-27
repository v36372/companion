use super::*;

#[test]
fn set_follow_survives_store_and_filesystem_reopen() {
    let _serial = serial();
    let runtime = runtime();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("follow.sqlite");
    let hub = Arc::new(companion_store::SqliteHub::open(&path).unwrap());
    hub.write(|c| {
        c.execute("INSERT INTO documents(id,title,source_path,stored_path,sha256) VALUES('follow-test','Follow PDF','','','follow')", [])?;
        Ok(())
    }).unwrap();
    let config = r#"{"provider":"openrouter","modelId":"~anthropic/claude-haiku-latest"}"#;
    let document = r#"{"documentId":"follow-test","title":"Follow PDF","pageCount":1,"provider":"openrouter","modelId":"~anthropic/claude-haiku-latest"}"#;
    let (events, received) = crossbeam_channel::unbounded();
    let (mut store, guest) = wasm::instantiate(
        &runtime,
        AgentHostState::with_secrets(
            Abort::default(),
            Policy::default(),
            events,
            Arc::new(MemorySecretStore::default()),
            hub.clone(),
        ),
    )
    .unwrap();
    guest.call_open(&mut store, config).unwrap().unwrap();
    let opened = guest
        .call_open_document(&mut store, document)
        .unwrap()
        .unwrap();
    let id = serde_json::from_str::<serde_json::Value>(&opened).unwrap()["conversationId"]
        .as_i64()
        .unwrap();
    received.try_iter().for_each(drop);
    guest
        .call_set_follow(&mut store, id, false)
        .unwrap()
        .unwrap();
    assert!(received.try_iter().any(|event| matches!(event, Event::Commit(json) if serde_json::from_str::<serde_json::Value>(&json).unwrap()["followEnabled"] == false)), "follow-only commits must be published");
    assert!(
        !store
            .data()
            .fs
            .follow
            .load(std::sync::atomic::Ordering::Relaxed)
    );
    guest.call_close(&mut store).unwrap();
    drop(store);
    drop(hub);

    let hub = Arc::new(companion_store::SqliteHub::open(path).unwrap());
    let (events, received) = crossbeam_channel::unbounded();
    let (mut store, guest) = wasm::instantiate(
        &runtime,
        AgentHostState::with_secrets(
            Abort::default(),
            Policy::default(),
            events,
            Arc::new(MemorySecretStore::default()),
            hub,
        ),
    )
    .unwrap();
    guest.call_open(&mut store, config).unwrap().unwrap();
    let reopened = guest
        .call_open_document(&mut store, document)
        .unwrap()
        .unwrap();
    assert_eq!(reopened, opened);
    assert!(received.try_iter().any(|event| matches!(event, Event::Commit(json) if serde_json::from_str::<serde_json::Value>(&json).unwrap()["followEnabled"] == false)));
    assert!(
        !store
            .data()
            .fs
            .follow
            .load(std::sync::atomic::Ordering::Relaxed)
    );
    guest.call_close(&mut store).unwrap();
}
