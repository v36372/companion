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

The examples are copied unchanged. The manifest uses this repository's workspace
dependencies and disables publishing. The upstream
Cargo.lock and rust-toolchain.toml are not included.

- Local changes: cached hue-preserving dark page palette and two-axis page scrolling with a draggable horizontal scrollbar.
