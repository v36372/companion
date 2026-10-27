# Chat pane / select-to-ask

Build the guest first (see [agent README](../agent/README.md)), then:

```sh
cargo run -p companion --features agent -- path/to/document.pdf
```

`COMPANION_AGENT=wasm` is implicit. `off`, or a build without the `agent`
feature, leaves a useful reader with an agent-off explanation. Missing OpenRouter
credentials show keychain/environment setup instructions and disable submission.
The host alone reads `COMPANION_OPENROUTER_API_KEY` or the `learning-companion` /
`openrouter` keychain entry; no key is sent to WASI. Startup checks credentials
on a background thread, never on the GPUI thread. No real credentials are needed
for the tests below.

Select PDF text and click **Ask** to submit “Explain this passage” with the
1-based page, occurrence, and an explicitly delimited untrusted quote. This does
not save a highlight. The composer supports Enter, Shift+Enter, Backspace and
clipboard paste. While a turn is running, sends queue follow-ups with a UUIDv7
requestId allocated once, at enqueue. Esc cancels the active turn and discards
queued follow-ups. Follow can be toggled independently of submitting a question.

The pane header's **Follow** toggle commits the conversation's durable FollowMode.
When on, the next model request includes a bounded, delimited reading section:
last explicit page, excerpt and selection. When off, that section is absent.
The flag and `/doc/follow.json` are restored on reattach. **What did I just read?**
is an ordinary canned submission, independent of the flag. Neither wheel scroll
nor explicit page turns submit anything. `/doc/viewport.json` reports
`source: "explicit_nav"`; wheel position is unknown. Existing explicit-navigation
progress writes are coalesced for 500 ms, without Durable commits.

Quoted user turns display a compact **p. N · “quote”** chip followed by the
question, not the model's injection delimiters. Clicking the chip takes the
tracked explicit-navigation path back to that page. New selected turns store
page, quote, occurrence and question as a `companion.quote` entry's data,
separately from its delimited model message, so reopening preserves the chip.
Delimiters are best-effort guidance, not a prompt-injection sandbox.

The guest awaits `viewState()` and subscribes before submit. Structural commits
carry transcript snapshots; on-stream carries replaceable pi.live snapshots.
The UI awaits an async event channel (no token polling timer), so a local user
commit wakes GPUI immediately instead of waiting for the PDF-command drain.
`session_failed`, `task_parked`, `task_faulted`, other reports, and short spatial
tool traces are shown in the pane.

The bridge immediately invalidates the old conversation on document switch.
It requests the HTTP cancellation atomic **and** pollable wakeup, waits for the
on-stack submit and subsequent abort export to settle, then sends SetDocument
and OpenDocument. It never uses resume for Esc. Queued old-document questions
are discarded. Shutdown queues close after cancellation and app quit awaits
worker completion, so the guest disposes its attached view. Conversation switch
also disposes the previous view. See [virtual document lifecycle](virtual-doc-fs.md).

The model id is selected from the guest's OpenRouter catalog, preferring the
saved setting (then the catalog's Haiku alias, then its first available model).
The selected provider/model is saved without credentials. There is no model
picker in this pane yet.

## Window-independent verification

```sh
cargo test --workspace
cargo test --workspace --features agent -- --test-threads=1
cargo test -p companion --features test-mock-provider -- --test-threads=1
```

The last command runs the actual bridge, component and local SSE provider with
an explicit memory SecretStore. It covers streamed snapshots → committed reply,
a queued follow-up, Esc mid-body (including abort-export completion), and
opening another document while streaming. Default tests also cover missing-key
submission refusal, stale events and UUIDv7/selection metadata.

## Local headless smoke (no API keys)

`test-mock-provider` is an explicit non-shipping feature. Only it compiles the
`COMPANION_TEST_PROVIDER_BASE_URL` override. It accepts **only**
`http://127.0.0.1:PORT`, substitutes an in-memory test credential, and reroutes
catalog OpenRouter requests to that exact local port. Normal builds cannot
recognize the override or enable loopback.

```sh
cargo build -p companion --features test-mock-provider
python3 scripts/chat-mock.py --port 18596 &
COMPANION_DATA_DIR="$PWD/.smoke-data" \
COMPANION_TEST_PROVIDER_BASE_URL=http://127.0.0.1:18596 \
DISPLAY=:96 WAYLAND_DISPLAY= \
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
  /home/exedev/codes/.coord/target-companion/debug/learning-companion \
  crates/companion-pdf/tests/fixtures/hello.pdf
```

Start Xvfb with the PR3 sysroot/bubblewrap recipe when needed; source the sysroot
environment first. Wait for “Ready” (cold component compilation is expensive),
select the fixture's text and click Ask. Capture while “Running” and streamed
words are visible with `import -window root -display :96 screenshot.png`.
Never point smoke traffic at a real provider. A live run needs a user-supplied
OpenRouter key and outbound HTTPS.
