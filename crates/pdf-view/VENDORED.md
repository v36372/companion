# Vendored gpui-pdf

- Upstream: https://github.com/packetThrower/zorite
- Published crate: `gpui-pdf` 0.6.1
- License: MIT; Copyright (c) 2026 Will Lehnertz (see [LICENSE](LICENSE)).
- Vendored: 2026-10-29, from the published crates.io source archive.

## Local changes

- Ported to gpui-pre 0.3.10: pass `cx` to `Modifiers::secondary(cx)` in the
  keyboard handler (`src/lib.rs`).
- Ported the scroll-container notification listener from `on_scroll_wheel` to
  `on_scroll` (`src/lib.rs`).
- Scroll-driven page/fraction events via an out-of-layout 150ms sampler and public
  `current_page_index` (`src/page_events.rs`).
- In-page links: bounded off-thread annotation parsing, named/explicit/GoTo/URI
  targets, cropped/rotated hit rectangles, selection-safe click events and
  destination y-offset navigation (`src/links.rs`).
- Cached hue-preserving dark page palette and two-axis page scrolling with a
  draggable horizontal scrollbar; overflowing pages are left-aligned so every
  edge stays reachable at high zoom (`src/recolor.rs`).
- Viewer toolbar/find bar: gpui-component buttons, native inputs, themed chrome and
  an attached highlight palette popover; removed unused forms/lopdf and box-drag
  area creation while preserving quote highlights and host navigation APIs
  (`src/toolbar.rs`, `src/find_bar.rs`).

The manifest uses this repository's workspace dependencies and disables
publishing. Examples are copied unchanged, except `dump_outline` (updated for the
links API) and `forms_check` (removed with forms). The upstream Cargo.lock and
rust-toolchain.toml are not included.
