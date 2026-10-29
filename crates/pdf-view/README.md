# gpui-pdf

[![crates.io](https://img.shields.io/crates/v/gpui-pdf.svg)](https://crates.io/crates/gpui-pdf) [![docs.rs](https://docs.rs/gpui-pdf/badge.svg)](https://docs.rs/gpui-pdf) [![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A PDF viewer for [GPUI](https://www.gpui.rs/), rendered in pure Rust with
[`hayro`](https://crates.io/crates/hayro). There are no native libraries and no
system-font dependency, so it builds and behaves the same on macOS, Linux, and
Windows.

Use the ready-made `PdfView`, which handles loading, scrolling, rendering, and
memory on its own, or build your own viewer on the lower-level functions
(`parse`, `page_dims`, `render_page`, `keep_window`). Text search and quote
highlights are optional features. Form filling and box-drag area mode are not supported.

The complete API reference is in [API.md](API.md).

## Overview

- **Bounded memory.** `PdfView` is page-virtualized: every page gets a correctly
  sized slot up front (so the scrollbar reflects the whole document), but only the
  pages near the viewport are rasterized. Pages scrolled away are freed — CPU pixel
  buffer *and* GPU atlas texture — so an 800-page document stays as light as a
  one-pager.
- **Zoom & navigation, no flicker.** Built-in zoom and page navigation (header
  controls, a native page-number input, keyboard shortcuts, a scroll-to-top
  button). On a zoom or quality change the page never blanks — the current bitmap
  stays on screen, rescaled, until the crisp re-render lands.
- **Off-thread everything.** The file is read, parsed *once*, and measured on a
  background thread; pages rasterize on the background executor and paint as they
  land. The UI never blocks.
- **DPI-aware, host-settable quality.** Pages rasterize at the display's pixel
  ratio × zoom × a quality multiplier the host supplies — read reactively, like the
  theme, so a settings slider re-renders every open viewer automatically.
- **Password-protected PDFs.** An encrypted file doesn't fail to load — `PdfView`
  enters a *locked* state and emits an event so the host can render its own
  password prompt; `unlock(password)` retries (RC4 / AES-128 / AES-256 via hayro's
  standard security handler — the exact table is in [API.md](API.md)).
- **Outline & links.** A table-of-contents side panel from the document's outline,
  and clickable link annotations (internal → jump to page, external → open URL),
  both also exposed as plain functions (`outline`, `page_links`) for custom UIs.
- **Theme-reactive.** The toolbar and find bar use gpui-component widgets and the
  host's `Root`/`Theme`. Page/placeholder styling still comes from `PdfStyleFn`.
  Initialize gpui-component and icon assets in the host before rendering the viewer.

## Adding the dependency

This is the workspace-vendored fork (see [VENDORED.md](VENDORED.md)):

```toml
[dependencies]
# Optional features:
#   markup — text layer + quote-anchored highlights (adds only `kurbo`)
#   search — find-in-PDF bar; implies markup
gpui-pdf = { path = "crates/pdf-view", features = ["search"] }
```
> **gpui version:** the crate depends on GPUI as published on crates.io — the
> `gpui-pre` family, consumed under the name `gpui` (`gpui = { package = "gpui-pre",
> version = "0.3" }`). Every `gpui-pre` release is a different Zed snapshot, so your
> app must resolve to the **same** `gpui-pre` version as this crate (one gpui graph);
> pin it in your `Cargo.lock` and move both together.


## Quick start

```rust
use std::rc::Rc;
use std::path::PathBuf;
use gpui_pdf::{PdfView, PdfStyle};

// Create the viewer (kicks off the off-thread load):
let view = cx.new(|cx| {
    PdfView::new(
        path,                                  // PathBuf to a local .pdf
        Rc::new(|| PdfStyle {                  // map your theme onto the chrome
            bg: my_theme::bg(),
            border: my_theme::border(),
            placeholder_bg: my_theme::muted_bg(),
            placeholder_fg: my_theme::muted_fg(),
            header_fg: my_theme::text(),
            header_muted: my_theme::muted_fg(),
        }),
        Rc::new(|| 1.0),                        // render-quality multiplier (1.0 = native DPI)
        cx,
    )
});

// Render it like any child view:
div().child(view.clone())

// Free its GPU textures before dropping it (e.g. when its tab closes):
view.update(cx, |v, cx| v.release(window, cx));
```

The exact contracts (locked-state flow, texture lifetime, zoom clamps, the
low-level primitives) are in [API.md](API.md).

## Markup & search (optional features)

`markup` adds a text layer — extracted by a custom hayro `Device`, no heavyweight
PDF dependency — and quote-anchored highlights on top of it: the host stores quotes
however it likes, hands them to the viewer to locate and draw, and gets callbacks
for clicks and drag-to-create (with a color picker). Coordinates are normalized, so
highlights track zoom and DPI for free. `search` builds a browser-style find-in-PDF
bar (⌘F, match highlighting, next/prev) on the same text layer.

## Toolbar and find

The fixed-height toolbar provides outline, page jump (1-based, Enter to commit,
Esc to cancel), zoom/reset, fit width/page, selection and text highlighting.
The pen opens an attached palette popover; it stays below the find-bar lane when
both are open. Neither overlay changes the document's layout.

**Cmd/Ctrl-F** opens/focuses the native find input, including IME, selection and
clipboard support. The counter reads `3/17`; **Enter / Shift-Enter** cycle matches,
**Cmd/Ctrl-G / Cmd/Ctrl-Shift-G** also cycle, and **Esc** closes. PageUp/PageDown
remain viewer navigation shortcuts when no native input is focused.

The unused `forms` feature, form mutation/event APIs, and `toggle_area_mode` /
`set_on_create_area` / `CreateAreaFn` have been removed. `Highlight.region` remains
accepted for existing host payload compatibility; there is no box-drag creation tool.

## Status

Early, but solid for scroll-to-read viewing. Password-protected PDFs open behind a
host-rendered prompt. Markup and find-in-PDF are available behind their features.
Scanned pages have no selectable text; OCR and form filling are out of scope.

## License

MIT. (The Zorite app itself is GPL-3.0-or-later.)
