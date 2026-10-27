# PDF text and selection integration (PR3)

`companion-pdf::snapshot::DocSnapshot::build` runs on GPUI's background executor,
reading the managed copy. It owns the first `gpui_pdf::parse`; `PdfView::new`
performs the accepted second parse of the same stable file. Four scoped extraction
workers share our `Arc<Document>`, with at most four `extract_page_text` calls in
flight. The snapshot is published once complete; an open-generation token prevents
an older document's completion from replacing the current snapshot. Successful
extraction also updates the catalog's `page_count`.

The snapshot includes agent-facing metadata, Markdown outline, text/JSON pages,
explicit-navigation viewport, and live selection. All agent/chat page numbers are
1-based; viewer and SQL indices are 0-based. `page_index` contains the boundary
conversions, including rejection of tool page zero.

## Viewer API accommodations

- gpui-pdf 0.6.1 has no public per-page overlay hook or viewport callback. The
  scanned-page banner is **adjacent above the viewer**, scoped to the last explicitly
  identified page, and says `Page N: This page has no text. (Last explicit navigation)`.
  `None` extraction and `PageText::is_empty()` both produce `text: None` and
  `no_text_layer: true`, never an empty text string.
- App Previous/Next, Page Up/Down, our outline links, and selection pages update
  the tracked explicit-navigation path and snapshot viewport. Wheel scroll and
  the embedded viewer's own toolbar/outline/link navigation are not observable.
  They must not be presented as live viewport tracking.
- The public text API exposes normalized quote/line rectangles, not raw glyph
  geometry. `PageJson.boxes` therefore contains normalized **line boxes** from
  `PageText::locate`, despite the design's `GlyphBox` type name.
- Outline entries are extracted off-thread with the Markdown and cached for the
  app's left rail; their clicks use the same progress tracking as app navigation.

## Selection behavior

Text-selection mode starts enabled. Dragging text invokes
`set_on_create_highlight`, storing only a pending `LiveSelection` plus picked
color and showing the Ask/Highlight bar. Prefix/suffix context is matched using
the viewer's lowercase, whitespace-insensitive normalization while preserving
UTF-8 boundaries. Selection identifies a page without scrolling it under the drag.

**Ask** puts `Page N` and a `<selected_quote>` block into the agent placeholder.
It does not insert a highlight. **Highlight** uses the existing `INSERT OR IGNORE`
quote locator and restores the markup list. Only one color per quote is allowed;
repeating Highlight with another color leaves the original row unchanged.
`highlights_with_rowids` reads locators and SQLite rowids together; the viewer's
`Highlight.id` is that rowid as `u64`, not the UUID or a UUID hash. Reopening restores
markup without needing an agent runtime.

## Verification

- `cargo test --workspace` covers the original text fixture, the reproducibly
  generated `tests/fixtures/mixed.pdf` (text page plus image-only page), selection
  context, quote lookup, one-color uniqueness, rowid/restore stability, catalog
  page counts, and UI page 1 → SQL 0 → tool goto 1 → viewer goto 0 without a window.
- Headless Xvfb smoke: app Next on `mixed.pdf` displayed the page-2 no-text banner.
  Screenshot: coordinator artifact `shots/pr3.png`.
- Headless drag/Ask/Highlight smoke: Ask left the SQL highlight count at zero;
  Highlight inserted `(page=0, quote_text='Companion text page', color='yellow')`
  and painted the quote. A subsequent launch restored the yellow markup.

VirtualDocFs and LLM submission remain later PRs; this PR only prepares their
snapshot and agent-pane selection input.
