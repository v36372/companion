# Virtual document environment (PR7)

`companion-env::VirtualDocFs` owns exactly one optional `Arc<DocSnapshot>` and the
app's `Arc<SqliteHub>`. It never opens an OS path. The optional pre-document state
fails document reads closed. Extraction is already complete when PR3 publishes
the snapshot, so there is no pending extraction state to poll or time out.

- `/doc` is read-only; metadata, outline and page slots come from our parse.
  Viewport/selection/follow are read at request time. Scanned `.txt` entries exist
  with size zero but reads return `not_supported` / `no_text_layer`; their JSON
  has `text: null`. Legitimately empty extracted text succeeds with `""`.
- `/notes` stores binary blobs in SQLite, scoped to the snapshot document id.
  Mutations commit atomically; reopen restores files and their parent directories.
  Empty directories are session-local (there is no directory row in `notes`).
- `/tmp`, open reader handles, and no-op watcher registrations are session-local.
  Cleanup clears temporary files/handles, never notes.
- All paths normalize POSIX components. NUL, `file://`, and climbing above `/`
  fail `invalid`. There are no symlinks, preopens, or shell.
- Host line scanning uses Durable's 0-based half-open **line** ranges (distinct
  from the 1-based **page** boundary), UTF-8 replacement sizes and BOM handling.
  LF readers preserve CR, report termination and do not manufacture a final line.
- `listFiles` is bounded, depth first, name ordered, returns relative paths and
  directories, skips `.git`, and honors hidden/excluded names. Gitignore is a
  no-op: the virtual tree is never a git worktree.

`HostExecutionEnv` forwards WIT return values/structured thrown errors to Durable
Results and converts WIT u64 bigint metadata to JS numbers. It passes Context to
every reader method and rejects already-aborted calls. `watch` returns a polling
watcher that never fires. Named notification conformance cases are expected
failures, as are successful shell/real-git-worktree cases. The independent Rust
suite tests the actual virtual backend, not the conformance fake host.

## PDF tools and UI bus

`createPdfExtension` installs all seven `pdf_*` tools. Page text, outline,
selection, viewport and search text are wrapped in untrusted-document delimiters
(best effort, not a sandbox). Page reads are capped at eight pages / 24,000 text
characters; missing pages are errors, scans return the explicit `no_text_layer`
record. `createSnapshotReadTool` retains stock `createReadTool()` execution and
sets `replay: "safe"` explicitly: stock 1.4.0 read omits it. CodingTools is never
installed. Every PDF tool is safe except `pdf_goto`, which omits replay.

`PdfCommands` converts pages once (WIT 1-based → SQL/viewer 0-based). Search reads
the snapshot without UI side effects. Goto queues navigation. Highlight uses
`INSERT OR IGNORE`, returns the existing SQLite rowid on replay and queues a
markup refresh only for a newly inserted row. The GPUI consumer rejects stale
document identities, navigates with the existing explicit-progress path and
restores persisted markup without flashing an already-existing highlight.

The UI prepares `doc_env` and a command channel when extraction completes.
`HostToGuest::SetDocument` attaches these to the agent Store and preserves them
across trap recovery. PR8's agent bridge must first request Esc, await submit
unwinding/abort, then enqueue SetDocument and OpenDocument; never swap while a
generation is running. This PR does not add a chat pane or start a second agent
runtime in the viewer. No visual UI layout changes were made.

## Verification

- Rust virtual backend tests: path containment, scanned/empty/missing pages,
  read-only doc operations, SQLite blob reopen, LF/BOM scanning, listing,
  one-time page conversion and silent idempotent highlight replay.
- Vitest `registerEnvConformance` against a synchronous fake WIT host, with named
  expected failures for unsupported watch notifications, shell and real git.
  Unit tests assert all D9 replay flags and document delimiters.
- Feature-gated actual component test: local OpenRouter-compatible SSE model
  emits pdf_read_pages/pdf_goto/pdf_highlight calls, verifies tool results and
  delimiters in its next request, and asserts the resulting host commands and
  zero-based SQL highlight. It shares the runtime compilation cache with the
  other agent tests; no keys or external model traffic are needed.
