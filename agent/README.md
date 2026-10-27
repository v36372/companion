# Component agent (PR6)

The guest runs the **pi-durable 1.4.0 Harness with SqliteStorage** and
pi-ai 1.4.0 providers. OpenRouter is the default. `COMPANION_AGENT=wasm` is
implicit; `off` prevents component loading (there is no sidecar runtime).
The caller supplies the app's `Arc<SqliteHub>` to the agent thread: production
uses the same `companion.sqlite`, not a second connection or `agent.sqlite`.
The host document ExecutionEnv, stock read tool, and seven PDF tools are installed:
no ambient FS/shell. See [virtual-doc-fs.md](../docs/virtual-doc-fs.md) for the
single-snapshot lifecycle, replay policy, and command channel integration.
PR4's streaming/abort evidence remains in [SPIKE-RESULTS.md](SPIKE-RESULTS.md).

## Build and test (no real credentials)

Node 22.19+ and the pinned Rust toolchain are required. Linux keyring needs
`pkg-config` and `libdbus-1-dev` (the supplied VM sysroot includes these).

```sh
cd agent
npm ci
npm test
npm run typecheck
npm run build
npm run componentize
cd ..
cargo test -p companion-agent-host --features agent -- --nocapture --test-threads=1
```

Default workspace builds/tests need neither Node nor the generated Wasm. The
`agent` feature includes `crates/companion-agent-host/src/agent-core.wasm` (ignored
by git). `build.rs` reports how to create it when missing. The independent CI
agent job componentizes and runs all host tests. Tests compile the component
**once per test binary** using OnceLock, then create fresh Stores for each case.
Uncached component compilation is expensive; see the PR4 evidence for timings.

The local mock test goes through the actual Wasm Harness, catalog-selected
OpenRouter provider, fetch, host SecretStore overwrite and streaming response.
Only `cfg(test)` or the explicit non-shipping `test-mock-provider` feature may
reroute the catalog's OpenRouter HTTPS URL to an explicitly selected 127.0.0.1
port. See [chat.md](../docs/chat.md) for the GPUI pane and headless smoke. Default
and shipping `agent` builds do not compile the smoke environment override. The mock verifies the injected test key, not the dummy.
It verifies incremental pi.live, done settlement and mid-stream Esc followed by
the abort export. PR4 raw SSE tests additionally cover TTFB/stalled-body abort,
epoch interruption, byte caps, policy and cancellation wakeups. No real keys
are used, read or searched for by tests.

## Boundary and lifecycle

`src/protocol.ts` defines the JSON payloads. `open`, `open-document`, `close`,
`submit`, `resume-wait`, `abort`, `set-follow` and `configure` are real exports.
`submit` awaits `submission.wait(BACKGROUND_CONTEXT)` **on-stack**; there is no
poll pump. The view is subscribed before submit. Live changes use on-stream;
on-commit carries only head-entry / busy transitions plus initial attachment.
`models.getModels()` is sent once per open via on-report; choose a model id from
that catalog rather than assuming the design's example id exists.

The host thread owns its synchronous Store. Send HostToGuest commands, consume
GuestToHost events. Call `Abort::request()` for Esc (not just the atomic): this
wakes pending HTTP headers/body pollables. After submit unwinds the host calls
the abort export. Retries are disabled and afterFailedRun is withdraw. Crash
recovery uses open + resume-wait, never Esc. A trapped Store/resource table is
dropped, a new instance is opened, then abort/resume-wait is attempted. With
SQLite the last committed checkpoint survives the lost Store.

## Storage / recovery

`HostSqliteDatabase` implements chord's promise-based facade. WIT parameters use
`sqlite-value` (never JSON parameters). Returned JSON rows contain tagged column
values: integers are decimal strings and blobs are byte arrays; the facade
restores safe integers to numbers, larger s64 values to bigint, and blobs to
Uint8Array. Text is never interpreted as an encoding marker.

`transaction(cb)` permits only the transaction handle. Database operations,
including nested transaction/close, never settle inside the callback, per chord.
Expired handles reject. Host transaction ownership is per Store; a Condvar keeps
app writers and other Stores out until commit/rollback. Store drop rolls back
an abandoned transaction and wakes writers. Do not hold a hub lock while waiting
on an agent command. WAL readers on query-only connections see committed state.

Only pi-durable creates/migrates `pi_` tables; app migrations never recreate them.
Open-document requires an existing catalog document, creates an ownerless
conversation with a model, seeds OpenDoc/FollowMode in conversationCreated, then
stores `conversations_map`. Reopening the document attaches that conversation.
There is a small crash window between conversation creation and map insertion:
a crash there can leave an unused ownerless conversation, never a map pointing
to a missing conversation. Notes remain blobs in the app `notes` table.

Crash recovery is **open(config), open-document (attach), resume-wait**. The last
step calls resume + waitForIdle on-stack. Esc instead cancels HTTP, lets submit
unwind, then calls abort; it must not resume the cancelled generation. Unexpected
traps drop the Store before opening a new instance on the same hub. The mock
crash test interrupts the actual streaming submit, drops the Store without
close/abort, reopens the SQLite file (including app migrations), reattaches and
finishes via resume-wait. No real credentials or external requests are needed.

## Host secrets / production HTTP

`SecretStore` supports native keychain read/write/delete. `KeychainStore` uses
service `learning-companion`, account `openrouter` (also openai, anthropic, brave,
google). Host-only fallback variables are `COMPANION_OPENROUTER_API_KEY`,
`COMPANION_OPENAI_API_KEY`, `COMPANION_ANTHROPIC_API_KEY`,
`COMPANION_BRAVE_API_KEY`, `COMPANION_GOOGLE_API_KEY`. Tests explicitly pass a
MemorySecretStore and never instantiate/read the native store for requests.
A live run needs a user-supplied OpenRouter key in that keychain or host variable,
a model id chosen from the printed catalog, and outbound HTTPS. No live request
has been made. Settings persist only provider + modelId in companion-store.

Production HTTP allows only HTTPS:443 to api.openai.com, api.anthropic.com,
openrouter.ai, generativelanguage.googleapis.com and api.search.brave.com. DNS is
resolved once; all results must be public; TCP connects to a validated IP while
TLS validates the original hostname. Redirects are not followed. Authorization,
API-key and provider identity headers are stripped and replaced from host data;
missing keys fail closed. Responses are header-first, piped through a 32 MiB
running cap and a 120 s total deadline. Native keychain reads run on the internal
runtime's blocking pool, inside the cancellable header/deadline envelope. Logs contain method/host/status/bytes,
not headers, paths, keys or bodies. The host builds EMPTY WASI env/args with no
preopens. No ambient guest auth context is consulted.

## Actual 1.4.0 / engine differences from the design sketch

- Conversation ids are branded types, so JSON ids are validated then cast at
  the boundary. ReportInfo.task exists only on some union arms.
- pi.live persists as `{}` when idle! Presence is not busy; use run/generation /
  compaction state. View entries are readonly. Streaming emits live snapshots
  only when their immutable value changes.
- FollowMode's initial return type must widen its boolean literal. Latest-doc fork
  semantics are `current` / `initial`, not `copy`. The 1.4.0 structural view mounts
  built-in docs only: attachment reads FollowMode through `harness.snapshot`,
  and set-follow publishes its committed value explicitly. The host uses those
  events to restore `/doc/follow.json`, including after Store/filesystem reopen.
- Selected input uses the supported `entry: { kind, data }` submission option.
  `companion.quote` entry data holds display locators/question; its model message
  retains the untrusted quote delimiters. Structural snapshots carry both, and
  the reader renders only the display parts as a tappable page chip.
- AgentChange.thinkingLevel is an enum, not an arbitrary string; the wire type
  remains the design's subset, cast to Durable's type at configure.
- Dummy modify is a true no-op: unlike the sketch it never returns a callback's
  proposed key. read/list/modify/delete cannot install real credentials.
- WIT s64 arguments arrive as bigint, but Durable ids are JS numbers. component.ts
  converts abort/configure/set-follow ids before validating their safe range.
- StarlingMonkey 0.22's native AbortSignal.any returned an already-aborted signal
  (reason `5e-324`) for fresh inputs. OpenAI's SDK composes signals before fetch,
  causing an immediate connection error. abort-signals.ts replaces only `any`
  with listener-based composition, preserving native signals/reasons/cancellation.
  It does NOT strip fetch signals or alter the SDK; Node unit tests cover it and
  the real Wasm submission exercises it end-to-end.
- StarlingMonkey bodies have getReader but no async iterator: streams.ts adapts
  each reader read directly. WIT result errors are lowered to strings.
- The bundled TypeBox uses Unicode-property regexes absent from StarlingMonkey's
  small ICU. esbuild.mjs uses acorn + regexpu-core to lower their exact Unicode
  sets at build time (no ASCII approximation, dependency patch or eval shim).
- Browser export conditions are required when bundling provider SDKs; neutral
  selects Anthropic's Node-only modules. No Node shim ships in the component.
- Wasmtime 36.0.19 uses explicit synchronous bindgen imports/exports flags, not
  `async: false`. The component imports WASI @0.2.10 while Wasmtime uses @0.2.6;
  semver-aware linking is exercised by tests (see PR4).

ComponentizeJS/Wizer runs with `env: {}` and fetch-event disabled. Its pinned
build-tool dependency chain has an existing npm-audit archive-extraction
advisory: do not use it to process untrusted archives (PR4 caveat).
