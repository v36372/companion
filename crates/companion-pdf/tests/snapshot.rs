use std::sync::Arc;

use companion_pdf::{
    page_index::{page_0_to_1, page_1_to_0},
    snapshot::{DocMeta, DocSnapshot},
    text_layer::{ask_quote, live_selection, locate_quote},
};
use companion_store::{SqliteHub, documents::Document, highlights::Highlight};

fn meta() -> DocMeta {
    DocMeta {
        id: "doc".into(),
        title: "Fixture".into(),
        source_path: "fixture.pdf".into(),
        sha256: "fixture".into(),
        page_count: 0,
    }
}

#[test]
fn snapshot_of_committed_text_fixture() {
    let snapshot = DocSnapshot::build(
        Arc::new(include_bytes!("fixtures/hello.pdf").to_vec()),
        meta(),
    )
    .unwrap();
    assert_eq!(snapshot.meta.page_count, 1);
    assert!(!snapshot.pages[0].json.no_text_layer);
    assert!(
        snapshot.pages[0]
            .text
            .as_deref()
            .unwrap()
            .contains("Companion")
    );
    assert_eq!(snapshot.pages[0].json.page, 1);
    assert!(!snapshot.pages[0].json.boxes.is_empty());
    let page = gpui_pdf::extract_page_text(&snapshot.hayro, 0).unwrap();
    assert!(!locate_quote(&page, "Companion", 0).is_empty());
}

#[test]
fn image_only_page_is_not_empty_text() {
    let snapshot = DocSnapshot::build(
        Arc::new(include_bytes!("fixtures/mixed.pdf").to_vec()),
        meta(),
    )
    .unwrap();
    assert_eq!(snapshot.meta.page_count, 2);
    assert!(!snapshot.pages[0].json.no_text_layer);
    assert!(snapshot.pages[0].text.is_some());
    let scanned = &snapshot.pages[1];
    assert!(scanned.text.is_none());
    assert!(scanned.json.text.is_none());
    assert!(scanned.json.no_text_layer);
    assert_eq!(scanned.json.page, 2);
    assert!(scanned.json.boxes.is_empty());
    snapshot.record_navigation(1, 0.0);
    let viewport = snapshot.viewport.read().unwrap();
    assert_eq!(viewport.page, 2);
    assert_eq!(viewport.source, "explicit_nav");
}

#[test]
fn ui_page_one_sql_zero_tool_goto_one_viewer_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.sqlite");
    let hub = SqliteHub::open(&path).unwrap();
    hub.upsert_document(&Document {
        id: "doc".into(),
        title: "Fixture".into(),
        source_path: "fixture.pdf".into(),
        stored_path: "copy.pdf".into(),
        sha256: "fixture".into(),
        page_count: None,
    })
    .unwrap();
    hub.set_page_count("doc", 2).unwrap();
    assert_eq!(
        hub.document_by_sha256("fixture")
            .unwrap()
            .unwrap()
            .page_count,
        Some(2)
    );
    // Viewer selection on the first page (UI label 1, callback index 0).
    let selection = live_selection(0, "A Companion quote.", "Companion".into(), 0);
    assert_eq!(selection.page, 1);
    assert_eq!(selection.prefix, "A ");
    assert_eq!(selection.suffix, " quote.");
    assert!(ask_quote(&selection).contains("Page 1\n<selected_quote>"));
    // Asking alone is a pure formatting operation, not a persistence action.
    assert!(hub.highlights("doc").unwrap().is_empty());
    let highlight = Highlight {
        id: "highlight".into(),
        document_id: "doc".into(),
        page_0: page_1_to_0(selection.page).unwrap(),
        quote_text: selection.quote,
        occurrence: 0,
        color: "yellow".into(),
        conversation_entry_id: None,
    };
    assert!(hub.insert_highlight(&highlight).unwrap());
    let mut duplicate = highlight.clone();
    duplicate.id = "different-id".into();
    duplicate.color = "blue".into();
    assert!(!hub.insert_highlight(&duplicate).unwrap());
    let rows = hub.highlights_with_rowids("doc").unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].0 > 0);
    assert_eq!(
        hub.open_read_only()
            .unwrap()
            .query_row("SELECT page FROM highlights", [], |row| row
                .get::<_, u32>(0))
            .unwrap(),
        0
    );
    drop(hub);
    let reopened = SqliteHub::open(&path).unwrap();
    let restored = reopened.highlights("doc").unwrap();
    assert_eq!(restored, vec![highlight]);
    assert_eq!(reopened.highlights_with_rowids("doc").unwrap(), rows);
    let pdf_goto_page_1 = page_0_to_1(restored[0].page_0);
    assert_eq!(pdf_goto_page_1, 1);
    let go_to_page_0 = page_1_to_0(pdf_goto_page_1).unwrap();
    assert_eq!(go_to_page_0, 0);
}

#[test]
fn encrypted_snapshot_requires_password_and_keeps_outline_zero_based() {
    let bytes = Arc::new(include_bytes!("fixtures/encrypted.pdf").to_vec());
    assert!(matches!(
        DocSnapshot::build(bytes.clone(), meta()),
        Err(gpui_pdf::LoadError::Locked)
    ));
    assert!(matches!(
        DocSnapshot::build_with_password(bytes.clone(), meta(), "wrong"),
        Err(gpui_pdf::LoadError::Locked)
    ));
    let snapshot = DocSnapshot::build_with_password(bytes, meta(), "companion").unwrap();
    assert_eq!(snapshot.meta.page_count, 2);
    assert!(
        snapshot.pages[0]
            .text
            .as_deref()
            .unwrap()
            .contains("Companion")
    );
    assert!(snapshot.pages[1].json.no_text_layer);
    assert_eq!(
        snapshot
            .outline
            .iter()
            .map(|item| item.page)
            .collect::<Vec<_>>(),
        vec![Some(0), Some(1)]
    );
    assert_eq!(snapshot.outline[0].title, "Text chapter");
    assert!(snapshot.outline_md.contains("Text chapter (page 1)"));
    assert!(snapshot.outline_md.contains("Scanned chapter (page 2)"));
    // Rail navigation passes the unmodified 0-based target to the tracked path.
    snapshot.record_navigation(snapshot.outline[1].page.unwrap() as u32, 0.0);
    assert_eq!(snapshot.viewport.read().unwrap().page, 2);
}
