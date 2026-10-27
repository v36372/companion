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

Click **Open PDF…** to choose or replace the single open document. The outline
and agent columns are placeholders; the center viewer provides navigation,
zoom, text selection, and search. PR1 opens the source file directly (managed
copies arrive in PR2; document snapshots in PR3).

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
