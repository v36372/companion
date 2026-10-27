use companion_env::{ListFilesOptions, PdfCommand, PdfCommands, VirtualDocFs, absolute_path};
use companion_pdf::snapshot::{DocMeta, DocSnapshot};
use companion_store::{SqliteHub, documents::Document};
use std::sync::Arc;

fn setup() -> (tempfile::TempDir, Arc<SqliteHub>, DocSnapshot) {
    let dir = tempfile::tempdir().unwrap();
    let hub = Arc::new(SqliteHub::open(dir.path().join("catalog.sqlite")).unwrap());
    hub.upsert_document(&Document {
        id: "doc".into(),
        title: "Fixture".into(),
        source_path: "source.pdf".into(),
        stored_path: "copy.pdf".into(),
        sha256: "fixture".into(),
        page_count: Some(2),
    })
    .unwrap();
    let doc = DocSnapshot::build(
        Arc::new(include_bytes!("../../companion-pdf/tests/fixtures/mixed.pdf").to_vec()),
        DocMeta {
            id: "doc".into(),
            title: "Fixture".into(),
            source_path: "source.pdf".into(),
            sha256: "fixture".into(),
            page_count: 0,
        },
    )
    .unwrap();
    (dir, hub, doc)
}
#[test]
fn viewport_and_follow_files_publish_host_state() {
    let (_dir, hub, doc) = setup();
    let doc = Arc::new(doc);
    let fs = VirtualDocFs::new(Some(doc.clone()), hub).unwrap();
    doc.record_navigation(0, 0.0);
    let viewport: serde_json::Value =
        serde_json::from_str(&fs.read_text_file("/doc/viewport.json").unwrap()).unwrap();
    assert_eq!(viewport["page"], 1);
    assert_eq!(viewport["page_count"], 2);
    assert_eq!(viewport["source"], "explicit_nav");
    assert!(viewport["excerpt"].as_str().unwrap().contains("Companion"));
    doc.record_navigation(1, 0.0);
    let viewport: serde_json::Value =
        serde_json::from_str(&fs.read_text_file("/doc/viewport.json").unwrap()).unwrap();
    assert_eq!(viewport["page"], 2);
    assert_eq!(viewport["source"], "explicit_nav");
    assert_eq!(viewport["excerpt"], ""); // scanned page, not invented text
    fs.follow.store(false, std::sync::atomic::Ordering::Relaxed);
    let follow: serde_json::Value =
        serde_json::from_str(&fs.read_text_file("/doc/follow.json").unwrap()).unwrap();
    assert_eq!(follow, serde_json::json!({"enabled":false}));
}

#[test]
fn paths_never_escape_to_host() {
    assert_eq!(
        absolute_path("/doc/pages", ".././meta.json").unwrap(),
        "/doc/meta.json"
    );
    for path in [
        "../../../etc/passwd",
        "/../../x",
        "file:///etc/passwd",
        "a\0b",
    ] {
        assert_eq!(absolute_path("/doc", path).unwrap_err().code, "invalid");
    }
    assert_eq!(
        absolute_path("/notes", "/doc/meta.json").unwrap(),
        "/doc/meta.json"
    );
}
#[test]
fn scanned_empty_missing_and_readonly() {
    let (_dir, hub, mut doc) = setup();
    doc.pages[0].text = Some(Arc::from(""));
    Arc::make_mut(&mut doc.pages[0].json).text = Some("".into());
    let fs = VirtualDocFs::new(Some(Arc::new(doc)), hub).unwrap();
    assert_eq!(fs.read_text_file("pages/0001.txt").unwrap(), "");
    let scan = fs.read_text_file("pages/0002.txt").unwrap_err();
    assert_eq!(
        (scan.code.as_str(), scan.message.as_str()),
        ("not_supported", "no_text_layer")
    );
    assert_eq!(fs.file_info("pages/0002.txt").unwrap().size, 0);
    let meta: serde_json::Value =
        serde_json::from_str(&fs.read_text_file("pages/0002.json").unwrap()).unwrap();
    assert_eq!(meta["text"], serde_json::Value::Null);
    assert_eq!(meta["no_text_layer"], true);
    assert_eq!(fs.list_dir("pages").unwrap().len(), 4);
    assert_eq!(
        fs.read_text_file("pages/0003.txt").unwrap_err().code,
        "not_found"
    );
    for result in [
        fs.write_file("meta.json", vec![]),
        fs.append_file("meta.json", vec![]),
        fs.truncate_file("meta.json", 0),
        fs.flush_file("meta.json"),
        fs.remove("pages", true, true),
        fs.create_dir("other", true),
        fs.rename_file("meta.json", "/notes/meta"),
    ] {
        assert_eq!(result.unwrap_err().code, "permission_denied");
    }
}
#[test]
fn notes_are_blobs_and_tmp_is_ephemeral() {
    let (_dir, hub, doc) = setup();
    let doc = Arc::new(doc);
    let fs = VirtualDocFs::new(Some(doc.clone()), hub.clone()).unwrap();
    assert_eq!(
        fs.join_path(vec!["a".into(), "b/../c".into()]).unwrap(),
        "a/c"
    );
    assert_eq!(
        fs.join_path(vec!["/notes".into(), "./sub".into()]).unwrap(),
        "/notes/sub"
    );
    assert_eq!(fs.join_path(vec![]).unwrap(), ".");
    assert_eq!(
        fs.join_path(vec!["".into(), "child".into()]).unwrap(),
        "child"
    );
    fs.create_dir("/notes/sub", true).unwrap();
    let bytes = vec![0, 255, 1, 13, 10];
    fs.write_file("/notes/sub/blob", bytes.clone()).unwrap();
    assert_eq!(
        fs.create_dir("/notes/sub/blob/child", true)
            .unwrap_err()
            .code,
        "not_directory"
    );
    assert!(!fs.exists("/notes/sub/blob/child").unwrap());
    fs.write_file("/tmp/foo", vec![1]).unwrap();
    fs.rename_file("/notes/sub/blob", "/notes/sub/renamed")
        .unwrap();
    drop(fs);
    let fs = VirtualDocFs::new(Some(doc), hub.clone()).unwrap();
    assert_eq!(fs.read_binary_file("/notes/sub/renamed").unwrap(), bytes);
    assert!(!fs.exists("/tmp/foo").unwrap());
    assert_eq!(
        hub.open_read_only()
            .unwrap()
            .query_row("SELECT typeof(content) FROM notes", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "blob"
    );
    fs.remove("/notes/sub", true, false).unwrap();
    assert!(!fs.exists("/notes/sub/renamed").unwrap());
}
#[test]
fn line_contract_and_listing() {
    let (_dir, hub, doc) = setup();
    let fs = VirtualDocFs::new(Some(Arc::new(doc)), hub).unwrap();
    fs.write_file("/notes/lines", b"\xef\xbb\xbfa\r\nb\n\nlast".to_vec())
        .unwrap();
    assert_eq!(
        fs.read_text_lines("/notes/lines", Some(2)).unwrap(),
        vec!["a\r", "b"]
    );
    let handle = fs.open_binary("/notes/lines", false).unwrap();
    let scan = fs.scan_lines(handle, 0, Some(1)).unwrap();
    assert_eq!(
        (scan.newlines, scan.start, scan.end, scan.selected_bytes),
        (3, 0, 5, 2)
    );
    assert_eq!(
        fs.scan_lines(handle, 1, Some(1)).unwrap_err().code,
        "invalid"
    );
    fs.close_handle(handle);
    fs.close_handle(handle);
    assert_eq!(fs.pread(handle, 0, 1).unwrap_err().code, "invalid");
    let listing: serde_json::Value = serde_json::from_str(
        &fs.list_files("/notes", ListFilesOptions::default())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(listing["entries"][0]["path"], "lines");
}
#[test]
fn commands_convert_once_and_highlight_replay_does_not_flash() {
    let (_dir, hub, doc) = setup();
    let (tx, rx) = crossbeam_channel::unbounded();
    let bus = PdfCommands {
        fs: Arc::new(VirtualDocFs::new(Some(Arc::new(doc)), hub.clone()).unwrap()),
        hub: hub.clone(),
        tx,
    };
    assert!(bus.goto(0, None, 0).is_err());
    assert!(bus.goto(3, None, 0).is_err());
    bus.goto(1, None, 0).unwrap();
    assert!(matches!(
        rx.recv().unwrap(),
        PdfCommand::Goto { page_0: 0, .. }
    ));
    let id = bus.highlight(1, "Companion".into(), 0, None).unwrap();
    assert!(matches!(rx.recv().unwrap(),PdfCommand::Highlight{id:got,..} if got==id));
    assert_eq!(
        bus.highlight(1, "Companion".into(), 0, Some("blue".into()))
            .unwrap(),
        id
    );
    assert!(rx.try_recv().is_err());
    let rows = hub.highlights("doc").unwrap();
    assert_eq!(rows[0].page_0, 0);
    assert_eq!(rows[0].color, "yellow");
    let hits: serde_json::Value =
        serde_json::from_str(&bus.search("Companion", 20).unwrap()).unwrap();
    assert_eq!(hits[0]["page"], 1);
}
