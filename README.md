# Learning Companion

A native Rust/GPUI PDF reader with a reading companion agent: Pi Durable 1.4.0
is compiled from TypeScript into a WASM component and hosted by Wasmtime in the
app, not a sidecar. The reader and durable conversations share SQLite;
OpenRouter supplies model responses and optional Brave Search supplies web
results. The agent can read the current document, follow your explicit page
navigation, answer selected passages, and navigate or highlight through PDF tools.

Implemented through PR0–PR11. See the [product vision](docs/VISION.md),
[design](docs/design.md), and implementation differences below.

## Prerequisites

- **Rust 1.95.0**, pinned in `rust-toolchain.toml` with rustfmt and clippy.
  With rustup installed, invoking Cargo in this repository installs the pinned
  toolchain automatically. This version is needed by the PDF/GPUI dependencies.
- **Node 22.19.0 or newer** for building the agent (`agent/package.json`);
  CI uses Node **22**. Node is not needed at runtime or for reader-only builds.
- **Linux:** CI runs Ubuntu 24.04. Install its exact GPUI/keychain build package
  set below, and run in an X11/Wayland desktop with a working Vulkan driver.
  Native key storage also needs an accessible, unlocked Secret Service over D-Bus.
- **macOS:** install Xcode Command Line Tools and the Rust/Node prerequisites.
  macOS uses the native Keychain. macOS (Apple Silicon) is built and release-smoke-tested in CI.

Linux packages (from `.github/workflows/ci.yml`):

```sh
sudo apt-get update
sudo apt-get install -y \
  pkg-config libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev \
  libvulkan-dev libasound2-dev libssl-dev libzstd-dev libsqlite3-dev libdbus-1-dev
```

## Quick start (Linux / macOS)

From the repository root, after installing the prerequisites and setting an
OpenRouter key as described below. The commands are the same on both platforms;
both Linux checks and a macOS release launch are CI-verified. Use a **release
build** for normal reading: debug builds are slow for this GUI app, even with
the optimised dev profile described below.

```sh
cd agent
npm ci
npm run build
npm run componentize
cd ..
cargo build --release -p companion --features agent --locked
cargo run --release -p companion --bin learning-companion --features agent -- path/to/book.pdf
```

Replace `path/to/book.pdf` with your PDF, or omit `-- path/to/book.pdf` to reopen
the most recent document. With no recent document, click **Open PDF…**.
The window remains usable as a reader when credentials are missing, but the chat
pane explains key setup and will not submit.

## Build and run

The agent build is a **separate step**: `npm run build` bundles
`agent/dist/guest.js`; `npm run componentize` writes
`crates/companion-agent-host/src/agent-core.wasm`. The component is generated,
ignored by Git, and embedded by the Cargo **`agent`** feature. Rebuild it after
changing guest code, then rebuild the Rust app. Enabling `agent` without the
artifact fails with build instructions.

Recommended release build/run (from the root, after generating the component):

```sh
cargo build --release -p companion --features agent --locked
cargo run --release -p companion --bin learning-companion --features agent -- path/to/book.pdf
```

For development/debugging, omit `--release`. Debug builds are slow for a GUI
app, especially PDF rendering and agent startup. The dev profile now optimises
workspace code at level 1 and dependencies (including GPUI, hayro and Wasmtime)
at level 3 to keep development usable; release is still recommended for reading.

```sh
cargo build --workspace --locked --features agent
cargo run -p companion --bin learning-companion --features agent -- path/to/book.pdf
```

Without the feature, the default workspace build needs neither Node nor WASM
and has **no agent**:

```sh
cargo build --release --workspace --locked
cargo run --release -p companion --bin learning-companion -- path/to/book.pdf
```

Omit `--release` here too for a development/debugging build.

Only one PDF is open at a time. **Open PDF…** replaces it; **Recent** reopens a
managed copy. Switching documents cancels an active turn before attaching the
new document's conversation.

## API keys, models and optional search

### Host-only credentials

Store keys in OS keychain service **`learning-companion`**, with account
**`openrouter`** for model access and **`brave`** for optional search. The Rust
host reads them and injects authorization into outbound requests; real keys are
never passed into the WASM guest's config, environment or credential store.
The guest has no ambient filesystem, shell, environment or preopened directories.

macOS examples (replace placeholders; these commands can leave keys in shell
history/process arguments):

```sh
security add-generic-password -s learning-companion -a openrouter -w '<openrouter-key>'
security add-generic-password -s learning-companion -a brave -w '<brave-key>'
```

Linux examples using `secret-tool` (provided separately by `libsecret-tools`;
enter the key at its prompt):

```sh
secret-tool store --label='Learning Companion OpenRouter' service learning-companion username openrouter target default
secret-tool store --label='Learning Companion Brave' service learning-companion username brave target default
```

The locked **keyring 3.6.3** Secret Service backend searches attributes `service`
and **`username`**, not `account`, with `target=default` for these entries. It also
has a compatibility fallback for items without `target` in the default collection.

Alternatively set **`COMPANION_OPENROUTER_API_KEY`** and optionally
**`COMPANION_BRAVE_API_KEY`** in the launching process's environment. These are
host-only fallbacks **when keychain lookup fails**, not overrides of an existing
keychain value (even an empty one). Restart after changing credentials. Tests use
explicit in-memory stores and never read native keys.

The chat pane's **Set up key** action shows the environment/keychain commands.
Missing keys, key-store failures, startup, document preparation and disabled-agent
builds have explicit status messages. Send, Ask, Follow, canned questions and Model
are dimmed while unavailable; the composer remains editable and keeps your draft.
On macOS, unsigned rebuilds may require Keychain approval again; interactive
Keychain approval is not covered by the no-key CI smoke.

If sending reports a DNS address in **198.18.0.0/15**, a fake-IP VPN/proxy such as
Surge or Clash is likely intercepting DNS. Exclude **openrouter.ai** from fake-IP
mode or use real DNS, then retry. The host deliberately still refuses connections
to this special-purpose address range; it does not bypass public-IP protections.

### Model selection

Click **Model** above the chat pane to choose Claude Haiku (the default), Claude
Sonnet, GPT-4o mini or Gemini 2.5 Flash. **Custom…** accepts another OpenRouter
model id. Ids are trimmed, must contain 1–128 ASCII letters, digits or `~._:/-`,
and cannot contain whitespace. Invalid input shows an inline error and is not
saved. The JSON string setting `model_id` persists in SQLite; unset means
`~anthropic/claude-haiku-latest`. Legacy `provider`/`modelId` preferences are not
used by this picker. There are no network calls to list models.

The next submitted turn (including a queued follow-up) configures the existing
durable conversation with the chosen OpenRouter id. A run already in progress
keeps its model; changing the preference does not restart the agent or app.
Custom model availability and tool support are ultimately checked by OpenRouter.
For ids absent from the bundled catalog, the guest uses a local text-only
OpenAI-compatible definition with a conservative 32,768-token context and
4,096-token output cap. Pricing metadata is zero/unknown, not a free-model claim.
Known models keep their bundled capabilities, limits and pricing.

### Web search

With a nonempty Brave key and no opt-out, `web_search` is registered automatically;
ask in chat when you want external information. Without that key the PDF/chat
features still work. To disable search, omit the Brave credential or persist
`websearch_enabled=false` using `SqliteHub::set_websearch_enabled(false)` (the
`settings` row's `value_json` is the JSON boolean `false`). There is no UI toggle
or environment flag for this setting. Restart the agent to refresh tool
registration; the host also checks the setting on every search request.

Brave access is restricted to HTTPS GET `/res/v1/web/search`, with a 10-second
total deadline and 1 MiB response cap. Other provider requests have a 120-second
deadline and 32 MiB cap. Host allowlisting, public-IP DNS pinning, no redirects,
and authorization replacement apply to both. See [web search](docs/websearch.md).
Document/search delimiters are best-effort prompt-injection guidance, not a
sandbox against malicious model instructions.

## Using the reader and companion

- **Outline:** click a destination in the left rail to jump to its page.
- **Find:** **Cmd-F / Ctrl-F** or toolbar **Find** opens the embedded viewer's
  search bar, including when the chat composer has focus.
- **Reading progress:** app **Previous/Next**, **Page Up/Page Down**, outline,
  quote chips and agent navigation use the tracked explicit-page path. Progress
  saves after a 500 ms debounce, flushes on normal quit, and restores on reopen.
  Wheel scrolling and the embedded viewer's own toolbar, links and search-result
  jumps are **not tracked**.
- **Quote highlights:** drag text, then click **Highlight** to persist and paint
  the quote; highlights restore on reopen, even without an agent. Repeating a
  highlight does not create another row or change the original color.
- **Select-to-ask:** drag text and click **Ask** to submit “Explain this passage”
  with its page/quote, without saving a highlight. User turns show a tappable
  **p. N · “quote”** chip that returns to the cited page.
- **Chat:** type in the composer; **Enter** sends, **Shift-Enter** inserts a line
  break. Clipboard paste is supported. Sends during a turn queue follow-ups.
  **Esc** cancels the active turn and discards queued questions. Tokens stream
  into the pane, alongside short PDF/search tool traces.
- **Follow:** the pane's **Follow: on/off** toggle persists per conversation
  (default on). When on, the next request includes a bounded reading section with
  the last explicit page, excerpt and selection. It does not know your wheel
  position. Page turns/scrolling never submit a question automatically.
  **What did I just read?** submits a question independently of the Follow flag.
- **Scanned pages:** a “This page has no text” banner above the viewer describes
  the last explicit page. No OCR is implemented; tools report `no_text_layer`.
- **Password PDFs:** enter the password in the masked modal and press **Enter**
  or **Unlock**. Wrong/missing passwords leave it open. Passwords are memory-only,
  never stored in SQLite or passed to the agent; reopening after launch asks again.
- **Crash recovery:** launch without a path to reopen the most recent document.
  Unfinished runnable work shows **Resuming…** while `resume-wait` resumes and
  waits on-stack. `session_failed`, `task_faulted` and `task_parked` reports appear
  in chat; **Retry parked task** retries the durable task without duplicating the
  submission. Esc cancels recovery using abort, not another resume.

The agent has a read-only virtual `/doc`, durable SQLite-backed `/notes`,
session-local `/tmp`, a read tool and seven `pdf_*` tools. There is no general
shell/CodingTools access. See [chat](docs/chat.md), [reader UX](docs/reader-ux.md)
and [virtual document filesystem](docs/virtual-doc-fs.md) for details.

### Appearance and zoomed pages

Launch with `COMPANION_THEME=dark` to use dark chrome and dark PDF pages even
on a light desktop, or `COMPANION_THEME=light` for original page colours.
Dark pages invert lightness while preserving hue (photographs retain their hue,
but their tones invert). Theme changes select cached pixels without reloading
the document. Highlights and search emphasis remain coloured overlays.

At high zoom, pan horizontally with **Shift + wheel**, a trackpad's horizontal
scroll gesture, or the draggable scrollbar along the viewer's bottom edge.
The right-edge scrollbar continues to navigate vertically.

## Data and configuration

PDFs are copied to **`docs/<uuid>.pdf`** under the app data directory before
parsing; SHA-256 identity deduplicates identical bytes. **`companion.sqlite`**
alongside them holds the catalog, progress, highlights, settings, notes and
Pi Durable's `pi_` tables on the same SQLite hub (WAL).

The default root is
`directories::ProjectDirs::from("", "LearningCompanion", "learning-companion")`:
on Linux, `$XDG_DATA_HOME/learning-companion` or
`~/.local/share/learning-companion`; on macOS, the platform mapping is
`~/Library/Application Support/LearningCompanion.learning-companion`.
`COMPANION_DATA_DIR` overrides the entire root.

| Environment variable | Behavior |
| --- | --- |
| `COMPANION_AGENT` | `wasm` (default) or `off`; other values are errors. WASM still requires the `agent` feature. |
| `COMPANION_DATA_DIR` | Override the data root, useful for isolated/headless runs. |
| `COMPANION_THEME` | `dark`, `light`, or `system` (default). Forces the whole window and PDF page palette; `system` follows live OS appearance changes. Invalid values warn and use `system`. |
| `COMPANION_CACHE_DIR` | Override the OS app cache root (separate from data). Compiled agent artifacts live in `components/`. |
| `COMPANION_COMPONENT_CACHE` | `off` disables persistent component caching; otherwise enabled on Unix. |
| `COMPANION_OPENROUTER_API_KEY` | Host keychain fallback for OpenRouter. |
| `COMPANION_BRAVE_API_KEY` | Host keychain fallback for optional Brave Search. |
| `COMPANION_OPENAI_API_KEY`, `COMPANION_ANTHROPIC_API_KEY`, `COMPANION_GOOGLE_API_KEY` | Additional host SecretStore fallbacks; not a desktop provider/model switch. |
| `RUST_LOG` | Logging filter; default `companion=info,gpui_pdf=warn`. |
| `COMPANION_TEST_PROVIDER_BASE_URL` | **Non-shipping `test-mock-provider` feature only**: port-scoped `http://127.0.0.1:PORT` mock routing and in-memory test keys. Not compiled into normal/`agent` builds. |

For build resource limits, CI and `scripts/spike-sse.sh` set `CARGO_BUILD_JOBS=1`;
the script also defaults `CARGO_PROFILE_DEV_DEBUG` and `CARGO_PROFILE_TEST_DEBUG`
to `0`. These are Cargo build controls, not app settings.

## Development and CI

For repeated local agent builds, `scripts/build-agent.sh` skips npm installation,
bundling and componentization when the nonempty Wasm is newer than the guest
sources, WIT, package lock and build configuration. Use `--force` after restoring
files with preserved timestamps or removing inputs. Normal `npm run componentize`
always rebuilds. See [build performance](docs/build-performance.md) for CI caching,
measurement commands and the debug-information tradeoff.

Reader-only Rust checks need no generated component; parsing/store tests need no
display:

```sh
cargo fmt --all -- --check
cargo build --workspace --locked
cargo test --workspace
cargo tree -i gpui-pre --depth 0
```

Agent checks (build the component before the feature-gated Rust tests):

```sh
cd agent
npm ci
npm test
npm run typecheck
npm run build
npm run componentize
npm run test:mock
cd ..
scripts/spike-sse.sh --built
cargo test --workspace --features agent -- --test-threads=1
cargo test -p companion --features test-mock-provider -- --test-threads=1
```

`spike-sse.sh --built` records component imports and runs
`cargo test -p companion-agent-host --features agent -- --nocapture --test-threads=1`.
Without `--built` it also installs/bundles/componentizes the guest.

CI has two **independent Ubuntu 24.04 jobs** and a macOS consumer of the agent artifact:

- **rust:** apt dependencies, pinned toolchain, formatting, locked workspace
  build, workspace tests, Clippy (`-D warnings`), and a guard against ambiguous
  GPUI snapshots.
- **agent:** Node 22, npm tests/typecheck, content-cached build/componentize and
  WASI import artifact, Node local-provider mock, pinned Rust, agent/mock-feature
  Clippy (`-D warnings`), host component harness and headless bridge tests.
  It uploads the WASM and generated WIT description even on cache hits.
- **macos:** Apple Silicon (`macos-15`), pinned Rust, a separate Rust cache,
  locked release build and Clippy with `agent`. Downloads `agent-core` from the
  agent job without rebuilding JavaScript, then launches for 15 seconds with
  fresh data/cache directories, the tiny `hello.pdf` fixture and no keys.
  The `macos-release-smoke` artifact contains the app log and a best-effort screenshot.
  This job waits for agent; the Linux Rust job remains independent.

Locally on a logged-in macOS GUI session, after the release build, run
`scripts/smoke-macos.sh [release-binary] [artifact-directory]`. It checks process
liveness and the window-open INFO log, then terminates the app and removes its
isolated data/cache. It unsets credential environment variables and refuses an
existing app Keychain credential (never modifies your keys). This is a launch
smoke, not a PDF-rendering, menu-interaction or live-provider test. Cold agent
compilation may still be running when the observation period ends.

CI disables Rust debug information and incremental compilation to keep cached
artifacts small and use consistent profiles across checks and the component harness.
Local dev builds use the optimised profile above, with Cargo's default debug
information. CI overrides dev optimisation to level 0 for workspace code and
dependencies via its Cargo config: correctness checks do not need interactive
GUI performance, and optimising every dependency makes cold checks much slower.
Its Rust cache has an explicit profile namespace to avoid mixing those artifacts.
The CI formatting command is the one
shown above. Mock-provider testing exercises the real component without external
requests or credentials; [chat.md](docs/chat.md) describes the local UI smoke.

## Known limitations

- No live run against real OpenRouter or Brave has been verified; provider tests
  and recorded UI smokes use local mock servers. A live run needs your keys and
  outbound HTTPS.
- CI's macOS release smoke checks no-key window creation and liveness, not
  interactive Keychain approval, app-menu actions or file-dialog interaction.
- Scans have no OCR. Progress, Follow and the scanned-page banner describe
  explicit navigation, not the live scroll viewport. Upstream accessors/events
  are still missing; the upstream issue was drafted, not posted.
- The first agent launch still compiles the component (roughly **70–80 seconds**
  on the small build VM). Subsequent Unix launches use a private on-disk native
  component cache, keyed by Wasm content and engine compatibility; invalid or
  unwritable caches fall back to compilation. Other platforms currently compile
  without caching. See [cache details and measurements](docs/component-cache.md).
- The pinned ComponentizeJS/weval build-tool chain has a reported **critical npm
  audit archive-extraction advisory** (`decompress`), plus moderate build-time
  advisories. Do not use it to process untrusted archives.
- The composer and custom model input are minimal (no rich editing/IME support),
  and there is no provider selector or search-settings UI. One document is attached at a
  time. Virtual filesystem watchers do not fire; shell/git-worktree semantics are
  deliberately unsupported.
- A crash between conversation creation and its document-map insertion can leave
  an unused ownerless conversation, not a mapping to nonexistent data.

## Differences from `docs/design.md`

These summarize implementation adjustments across PR0–PR11, not unfinished
features from intermediate PRs:

- Rust moved from the 1.85 floor to **1.95.0**; GPUI/platform are both pinned at
  **0.3.10** with the component family at **0.8.1**. Vendored gpui-pdf 0.6.1
  includes the modifier/scroll API adaptations recorded in its `VENDORED.md`.
- The PDF is parsed separately for the viewer and our snapshot. Missing viewport
  and overlay hooks mean explicit-navigation tracking and an adjacent banner,
  rather than scroll-aware tracking or per-page overlays.
- `GlyphBox` payloads contain normalized **line rectangles**, not raw glyph boxes.
- `COMPANION_DATA_DIR` was added for isolation; native keys remain host-only.
- StarlingMonkey requires a reader-based stream adapter, a replacement for broken
  native `AbortSignal.any`, and build-time Unicode-regex lowering for TypeBox.
  Provider bundles use browser export conditions, with no Node runtime shim.
- Wasmtime uses explicit synchronous bindings; WASI component imports are 0.2.10
  versus the host's 0.2.6 interfaces (semver linking is tested). With no CLI/FS
  componentizer toggles, the host supplies empty env/args and no preopens.
- Durable 1.4.0 uses branded ids, `output` tool results, `current`/`initial` fork
  semantics and an enum thinking level. Idle `pi.live={}` is not a busy signal;
  FollowMode is read separately from structural views. Dummy credential `modify`
  is a true no-op, not the sketch's proposed-key return.
- SQLite WIT parameters are typed; JSON result columns tag integers as decimal
  strings and blobs as byte arrays to preserve bigint/binary values.
- The read tool gets explicit safe replay; goto and web search omit replay to
  avoid repeating navigation or billed searches. Highlights replay silently.
- Selected turns use supported entry data for durable display chips, separate
  from delimited model input. The minimal GPUI chat uses structural commits plus
  on-stream snapshots, with explicit queue/cancellation/recovery states.
- Cold component compilation misses the intended startup target; persistent
  component caching speeds subsequent launches. Live-provider validation remains
  a follow-up.

## Repository layout

- `crates/companion/` — `learning-companion` binary, GPUI reader/chat and bridge.
- `crates/companion-pdf/` — viewer wrapper, text/snapshot/selection and fixtures.
- `crates/pdf-view/` — vendored gpui-pdf 0.6.1; see [provenance and local changes](crates/pdf-view/VENDORED.md).
- `crates/companion-store/` — SQLite hub, migrations, catalog and preferences.
- `crates/companion-agent-host/` — Wasmtime host, HTTP policy, secrets and SQLite WIT.
- `crates/companion-env/` — virtual document filesystem and PDF command bus.
- `agent/` — Pi Durable guest, extensions, bundling/componentizing and tests;
  see its [README](agent/README.md) and [spike evidence](agent/SPIKE-RESULTS.md).
- `wit/` — component interface contract.
- `scripts/` — SSE harness runner and local chat/search mock servers.
- `docs/` — vision/design and implementation notes (some earlier PR notes are
  historical; this README describes the integrated app).
