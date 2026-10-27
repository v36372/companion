use std::sync::Arc;

#[test]
fn parses_fixture_and_extracts_text_without_a_window() {
    // Hand-written one-page PDF with a standard Helvetica text object; no GUI
    // platform or display server is initialized by this test.
    let bytes = Arc::new(include_bytes!("fixtures/hello.pdf").to_vec());
    let document = gpui_pdf::parse(bytes).expect("parse fixture");
    assert_eq!(gpui_pdf::page_dims(&document).len(), 1);
    let text = gpui_pdf::extract_page_text(&document, 0).expect("first page");
    assert!(!text.is_empty());
    assert!(text.text().contains("Companion"), "{}", text.text());
    assert!(gpui_pdf::extract_page_text(&document, 1).is_none());
}

#[test]
fn rejects_invalid_pdf_without_a_window() {
    assert!(gpui_pdf::parse(Arc::new(b"not a PDF".to_vec())).is_err());
}
