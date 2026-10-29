# gpui-pre-macos stable-Rust compatibility patch

- Source: the published crates.io `gpui-pre-macos` 0.3.10 archive
  (https://crates.io/crates/gpui-pre-macos/0.3.10).
- Upstream: https://github.com/zed-industries/zed, snapshot
  `08cbb88a152c3751a43e853425a70e9f631a5918` (`gpui_macos`).
- License: Apache-2.0; Copyright 2022–2025 Zed Industries, Inc.
  See the preserved `LICENSE-APACHE`.
- Vendored: 2026-10-29. The normalized published `Cargo.toml` and sources are
  retained; the upstream lockfile and original workspace manifest are omitted.

## Local changes

In `src/window.rs`, replace the three `Option::map_or_default` calls (screen
visible frame, content-view size, and target-screen frame) with the equivalent
stable `map_or_else(Default::default, ...)`. A file-header notice identifies the
local patch. No other sources or dependencies are changed.

The root `[patch.crates-io]` applies this only where the dependency graph uses
`gpui-pre-macos` (macOS, not Linux). The application remains on Rust 1.95.0;
`map_or_default` is not stable on that toolchain.

**Drop when the toolchain is >= 1.98**, where `map_or_default` is stable (or when
an adopted upstream snapshot no longer needs the workaround). Remove the root
patch entry and this directory together.
