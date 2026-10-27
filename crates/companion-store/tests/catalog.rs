use companion_store::{
    SqliteHub, documents::Document, highlights::Highlight, progress::ReadingProgress,
};
use tempfile::TempDir;

fn setup() -> (TempDir, SqliteHub, Document) {
    let dir = tempfile::tempdir().unwrap();
    let hub = SqliteHub::open(dir.path().join("companion.sqlite")).unwrap();
    let source = dir.path().join("book.pdf");
    std::fs::write(&source, b"stable PDF bytes").unwrap();
    let document = hub.import_pdf(&source, dir.path()).unwrap();
    (dir, hub, document)
}

#[test]
fn migrations_are_idempotent_and_preserve_data() {
    let (dir, hub, document) = setup();
    hub.write(|connection| {
        connection.execute_batch(
            "CREATE TABLE pi_test(value TEXT); INSERT INTO pi_test VALUES ('untouched');",
        )?;
        Ok(())
    })
    .unwrap();
    drop(hub);
    let hub = SqliteHub::open(dir.path().join("companion.sqlite")).unwrap();
    assert_eq!(
        hub.document_by_sha256(&document.sha256).unwrap(),
        Some(document)
    );
    let connection = hub.open_read_only().unwrap();
    assert_eq!(
        connection
            .query_row("SELECT value FROM pi_test", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "untouched"
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM schema_migrations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    for table in [
        "documents",
        "highlights",
        "reading_progress",
        "conversations_map",
        "settings",
        "notes",
    ] {
        assert!(
            connection
                .prepare(&format!("SELECT * FROM {table}"))
                .is_ok()
        );
    }
}

#[test]
fn copy_on_open_deduplicates_bytes_and_recent_reuses_identity() {
    let (dir, hub, first) = setup();
    let source = dir.path().join("renamed.pdf");
    std::fs::write(&source, b"stable PDF bytes").unwrap();
    let second = hub.import_pdf(&source, dir.path()).unwrap();
    assert_eq!(first, second);
    assert_ne!(first.stored_path, source.to_string_lossy());
    std::fs::write(&source, b"changed original").unwrap();
    assert_eq!(
        std::fs::read(&first.stored_path).unwrap(),
        b"stable PDF bytes"
    );
    assert!(hub.recent(10).unwrap().is_empty());
    hub.touch_last_opened(&first.id).unwrap();
    std::fs::remove_file(&source).unwrap();
    assert_eq!(hub.recent(1).unwrap(), vec![first.clone()]);
    assert!(std::path::Path::new(&first.stored_path).is_file());
    assert!(hub.recent(0).unwrap().is_empty());
    let mut duplicate = first.clone();
    duplicate.id = "different-uuid".into();
    assert_eq!(hub.upsert_document(&duplicate).unwrap(), first);
}

#[test]
fn missing_managed_copy_is_repaired_from_identical_bytes() {
    let (dir, hub, document) = setup();
    std::fs::remove_file(&document.stored_path).unwrap();
    let repaired = hub
        .import_pdf(&dir.path().join("book.pdf"), dir.path())
        .unwrap();
    assert_eq!(document, repaired);
    assert_eq!(
        std::fs::read(&repaired.stored_path).unwrap(),
        b"stable PDF bytes"
    );
}

#[test]
fn progress_round_trips_zero_based_page_and_excerpt() {
    let (_dir, hub, document) = setup();
    assert_eq!(hub.progress(&document.id).unwrap(), None);
    for page_0 in [6, 0] {
        let progress = ReadingProgress {
            page_0,
            scroll_frac: 0.25,
            viewport_excerpt: Some("quote".into()),
        };
        hub.set_progress(&document.id, &progress).unwrap();
        assert_eq!(hub.progress(&document.id).unwrap(), Some(progress));
    }
}

#[test]
fn progress_rejects_invalid_fractions_and_unknown_documents() {
    let (_dir, hub, document) = setup();
    for scroll_frac in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
        let progress = ReadingProgress {
            page_0: 0,
            scroll_frac,
            viewport_excerpt: None,
        };
        assert!(hub.set_progress(&document.id, &progress).is_err());
    }
    assert!(hub.progress(&document.id).unwrap().is_none());
    assert!(
        hub.set_progress(
            "missing",
            &ReadingProgress {
                page_0: 0,
                scroll_frac: 0.0,
                viewport_excerpt: None,
            }
        )
        .is_err()
    );
}

#[test]
fn highlight_locator_is_unique() {
    let (_dir, hub, document) = setup();
    let first = Highlight {
        id: "one".into(),
        document_id: document.id.clone(),
        page_0: 0,
        quote_text: "quote".into(),
        occurrence: 0,
        color: "yellow".into(),
        conversation_entry_id: None,
    };
    assert!(hub.insert_highlight(&first).unwrap());
    let mut second = first.clone();
    second.id = "two".into();
    second.color = "green".into();
    assert!(!hub.insert_highlight(&second).unwrap());
    assert_eq!(hub.highlights(&document.id).unwrap(), vec![first]);
    second.occurrence = 1;
    assert!(hub.insert_highlight(&second).unwrap());
}

#[test]
fn read_only_rejects_writes_and_wal_is_active() {
    let (_dir, hub, _) = setup();
    let connection = hub.open_read_only().unwrap();
    assert!(
        connection
            .execute("INSERT INTO settings VALUES ('test','true')", [])
            .is_err()
    );
    assert_eq!(
        connection
            .query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    assert_eq!(
        connection
            .query_row("PRAGMA query_only", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    hub.write(|connection| {
        for (pragma, expected) in [
            ("synchronous", 1),
            ("foreign_keys", 1),
            ("busy_timeout", 5000),
            ("wal_autocheckpoint", 1000),
        ] {
            assert_eq!(
                connection.query_row(&format!("PRAGMA {pragma}"), [], |r| r.get::<_, i64>(0))?,
                expected
            );
        }
        Ok(())
    })
    .unwrap();
}
