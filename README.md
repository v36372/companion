# Learning Companion

A native desktop app for reading books to learn: a GPUI PDF reader with an embedded Pi Durable agent that shares the page, the selection, and the work of understanding.

- Product vision: [docs/VISION.md](docs/VISION.md)
- Technical design (rev 5, PR plan PR0–PR11): [docs/design.md](docs/design.md)

Status: under construction, built PR by PR following the design's PR Plan.

## Run the PDF shell

```sh
cargo run -p companion --bin learning-companion
# Or open a document directly:
cargo run -p companion --bin learning-companion -- path/to/book.pdf
```

Click **Open PDF…** to choose or replace the single open document, or reopen
one from **Recent**. PDFs are copied before parsing to `docs/<uuid>.pdf` under
the application data directory; SHA-256 identity deduplicates identical bytes.
The SQLite catalog lives alongside these copies as `companion.sqlite`.

Paths use `directories::ProjectDirs::from("", "LearningCompanion", "learning-companion")`
(on Linux, `$XDG_DATA_HOME/learning-companion` or `~/.local/share/learning-companion`).
For tests/headless runs, `COMPANION_DATA_DIR` overrides the entire data directory.

The app's **Previous/Next** buttons and **Page Up/Page Down** keys save the last
explicit page after a 500 ms debounce and restore it when reopening a document.
Pending navigation is flushed on normal shutdown. Wheel scrolling does not update progress. The upstream viewer's own toolbar,
outline, links, and search have no navigation callback in gpui-pdf 0.6.1 and are
not tracked; use the app controls for restorable progress. The agent column is
still a placeholder; document snapshots/outline integration arrive in PR3.

Rust 1.95.0 is pinned: hayro requires 1.92, and GPUI uses `slice::as_array`
and `hint::cold_path`, stabilized in 1.93 and 1.95. GPUI and its platform
crate are pinned together at 0.3.2: gpui-pdf 0.6.1 does not compile against
0.3.10's changed `Modifiers::secondary` API. Linux needs the GUI development
libraries listed in `.github/workflows/ci.yml`, plus a working X11/Wayland display
and Vulkan driver. Set `RUST_LOG` to override the default
`companion=info,gpui_pdf=warn` logging filter.

```sh
cargo build --workspace
cargo test --workspace  # PDF parsing/text tests do not need a display
cargo fmt --all -- --check
cargo tree -i gpui-pre  # must resolve exactly one GPUI snapshot
```
