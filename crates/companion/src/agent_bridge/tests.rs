use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn bridge() -> (
    Bridge,
    crossbeam_channel::Receiver<Command>,
    Arc<AtomicUsize>,
) {
    let (tx, rx) = crossbeam_channel::unbounded();
    let cancelled = Arc::new(AtomicUsize::new(0));
    let counter = cancelled.clone();
    let mut bridge = Bridge::off();
    bridge.tx = Some(tx);
    bridge.cancel = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    bridge.event(Event::Ready);
    bridge.conversation = Some(10);
    (bridge, rx, cancelled)
}
fn attachment(id: &str) -> Attachment {
    let hub = Arc::new(companion_store::SqliteHub::open(":memory:").unwrap());
    Attachment {
        fs: Arc::new(VirtualDocFs::new(None, hub).unwrap()),
        pdf_tx: crossbeam_channel::unbounded().0,
        document_id: id.into(),
        title: id.into(),
        page_count: 1,
    }
}
fn submitted(rx: &crossbeam_channel::Receiver<Command>) -> Value {
    match rx.recv().unwrap() {
        Command::Submit { json, .. } => serde_json::from_str(&json).unwrap(),
        _ => panic!("expected submit"),
    }
}
#[test]
fn submit_stream_commit_and_uuid_v7() {
    let (mut bridge, rx, _) = bridge();
    assert!(bridge.submit(
        "Explain".into(),
        Some(json!({"page":4,"quote":"quote","occurrence":2}))
    ));
    let req = submitted(&rx);
    assert_eq!(
        uuid::Uuid::parse_str(req["requestId"].as_str().unwrap())
            .unwrap()
            .get_version_num(),
        7
    );
    assert_eq!(req["selection"]["page"], 4);
    assert!(bridge.running);
    bridge.event(Event::Stream(json!({"conversationId":10,"live":{"generation":{"message":{"content":[{"type":"text","text":"Hello"}]}}}})));
    assert_eq!(bridge.live, "Hello");
    bridge.event(Event::Stream(json!({"conversationId":10,"live":{"generation":{"message":{"content":[{"type":"text","text":"Hello world"}]}}}})));
    assert_eq!(
        bridge.live, "Hello world",
        "snapshots must replace, not append"
    );
    bridge.event(Event::Commit(json!({"conversationId":10,"entries":[{"kind":"pi.assistant","model":[{"content":[{"type":"text","text":"Hello world"}]}]}],"busy":false})));
    bridge.event(Event::Settled);
    assert!(!bridge.running);
    assert_eq!(bridge.transcript, ["Agent\nHello world"]);
}
#[test]
fn follow_up_is_queued_and_escape_drops_queue() {
    let (mut bridge, rx, cancelled) = bridge();
    bridge.submit("first".into(), None);
    let first = submitted(&rx);
    bridge.submit("next".into(), None);
    assert_eq!(bridge.queued(), 1);
    assert!(rx.try_recv().is_err());
    bridge.event(Event::Settled);
    let second = submitted(&rx);
    assert_ne!(first["requestId"], second["requestId"]);
    bridge.submit("discard".into(), None);
    bridge.abort();
    assert_eq!(cancelled.load(Ordering::SeqCst), 1);
    assert_eq!(bridge.queued(), 0);
    assert!(bridge.running, "Esc must wait for submit unwind + abort");
    bridge.event(Event::Settled);
    assert!(!bridge.running);
    assert!(rx.try_recv().is_err());
}
#[test]
fn document_switch_waits_for_abort_then_replaces_tree() {
    let (mut bridge, rx, cancelled) = bridge();
    bridge.submit("old".into(), None);
    submitted(&rx);
    bridge.submit("old followup".into(), None);
    bridge.switch_document(attachment("B"));
    assert_eq!(cancelled.load(Ordering::SeqCst), 1);
    assert!(
        rx.try_recv().is_err(),
        "must not SetDocument while old submit is on stack"
    );
    assert!(!bridge.submit("too soon".into(), None));
    bridge.event(Event::Stream(
        json!({"conversationId":10,"live":{"generation":{"message":{"content":"stale"}}}}),
    ));
    assert!(bridge.live.is_empty());
    bridge.event(Event::Settled);
    assert!(matches!(rx.recv().unwrap(), Command::Attach(doc) if doc.document_id == "B"));
    bridge.event(Event::Attached(20));
    assert_eq!(bridge.conversation, Some(20));
    assert!(rx.try_recv().is_err());
}
#[test]
fn missing_key_does_not_submit_and_reports_survive_commit() {
    let (mut bridge, rx, _) = bridge();
    bridge.available = false;
    bridge.event(Event::Error(
        "OpenRouter key missing. Set COMPANION_OPENROUTER_API_KEY".into(),
    ));
    assert!(!bridge.submit("must not send".into(), None));
    assert!(rx.try_recv().is_err());
    bridge.event(Event::Report("task_parked: interrupted".into()));
    bridge.event(Event::Commit(json!({"conversationId":10,"entries":[]})));
    assert!(bridge.notices.iter().any(|s| s.contains("task_parked")));
}

#[test]
fn extraction_switch_and_close_invalidate_old_questions() {
    let (mut bridge, rx, cancelled) = bridge();
    bridge.submit("old".into(), None);
    submitted(&rx);
    bridge.begin_switch();
    assert!(!bridge.submit("must not hit old PDF during extraction".into(), None));
    bridge.event(Event::Settled);
    assert!(rx.try_recv().is_err());
    bridge.close();
    assert!(matches!(rx.recv().unwrap(), Command::Close));
    assert_eq!(cancelled.load(Ordering::SeqCst), 1);
}

#[cfg(feature = "test-mock-provider")]
mod mock;
