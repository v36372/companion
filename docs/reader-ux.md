# Reader UX: outline, Find, encrypted PDFs and recovery

The 220px outline rail comes from `outline(&snapshot.hayro)`, our independently
owned parse of the managed copy. Targets stay **0-based** through
`goto_explicit` → viewer/progress; only the agent-facing Markdown/viewport uses
1-based pages. Outline entries without a destination do not navigate.

**Cmd-F / Ctrl-F**, including while the composer has focus, and the toolbar
**Find** button call `PdfView::toggle_search`. This is the embedded viewer's find
bar, not a separate search implementation. As with the viewer's other internal
navigation, search-result movement does not pretend to be live viewport tracking.

## Passwords

`PdfEvent::LockChanged` and `PdfView::is_locked()` display a host modal. Use its
masked input (focused automatically), enter the password and press Enter (or click Unlock). Missing and
wrong passwords keep the prompt open. Unlock sends the same in-memory string to
`PdfView::unlock` and our background `DocSnapshot::build_with_password`, which
calls `parse_with_password` against the managed copy. A generation token rejects
stale extraction results. No document environment/conversation is attached until
our parse succeeds. The input clears immediately after submission and on document
switch; the password is never part of the snapshot, SQLite, guest configuration,
chat or logs. The upstream parser/viewer may retain decryption data in memory
until release. No password is retained for the next application launch.

`tests/fixtures/generate_password.py` (pypdf 6.21.3) generates a two-page outline
fixture and its RC4-128 encrypted twin. **companion** is a public test password,
not a credential. Tests cover missing/wrong/correct passwords, extracted text,
scanned-page detection and the outline targets `[0, 1]` / Markdown pages `[1, 2]`.

## Crash resume and reports

With no path argument, launch reopens the most recent managed document (encrypted
files ask again). On attachment, the guest inspects unfinished tasks for the mapped conversation.
If there is runnable unfinished work, `open-document` returns `resumable: true`.
The bridge marks the chat **Resuming…** before accepting queued submissions,
then calls `resume-wait` on the agent thread: `resume()` + `waitForIdle()` stay
on-stack. When it settles, the committed transcript replaces live text and queued
follow-ups may run. Opening an idle conversation makes no model request.

Esc during recovery uses the normal HTTP cancellation/pollable path and then the
abort export after unwind, never another resume. Retry has the same cancellation
behavior. Switching documents waits for recovery/cancellation before swapping
`VirtualDocFs`. Reports remain visible across transcript commits:

- `session_failed`: storage/session failure;
- `task_faulted`: task failure;
- `task_parked`: restart limit reached; **Retry parked task** calls the real
  `Harness.retryTask(taskId)` and then awaits resume/idle, not a duplicate submit.

Already parked tasks also emit an actionable report on reattach. Task ids and
conversation ownership survive the report boundary; Retry is scoped
to the current conversation and is disabled by the state machine while busy.
Quote turns render speaker, tappable quote chip, then question.

## Verification

See [chat.md](chat.md) for the build, local provider and Xvfb recipe. PR11's
headless capture uses display `:95` and the generated `outline.pdf`, with outline,
embedded Find and chat in one window. The bridge's mock-provider test interrupts
a real Wasm submit at a checkpoint without close/abort, creates a fresh Store,
reattaches through the actual bridge and observes Resuming → streamed recovery →
Ready. Unit tests cover queue/cancel and conversation-scoped parked-task Retry.
No test requires or reads real API credentials.
