# PR4: on-stack SSE spike

This is a capability/abort spike, not a working Durable harness. Production WIT
exports and fs/sqlite/pdf imports are present but return explicit stub errors.
There is no dependency on the PDF crates.

## Reproduce

Requires Node 22.19+ (CI uses 22; local validation uses 24), the pinned Rust
toolchain and enough space for Wasmtime/Cranelift.

```sh
scripts/spike-sse.sh
# Already componentized:
scripts/spike-sse.sh --built
# Extra policy / 32 MiB running-cap / cancellation wakeup tests:
cargo test -p companion-agent-host --features agent
# Optional real pi-ai provider against ONLY a local mock (Node, not Wasm):
cd agent && npm run test:mock
```

The script runs a loopback chunked SSE server, 30 events at 100 ms intervals.
`spikeSse` awaits fetch and iterates `response.body` inside the export, calling
`on-stream` per raw byte chunk (not per parsed event). Tests require at least ten
incremental chunks, an early first chunk and at least ten 50–250 ms gaps.
StarlingMonkey's body lacks `Symbol.asyncIterator`; a small standards adapter
uses `getReader().read()` to yield each chunk directly. There is no buffering
or timer-generated data. Guest exceptions are converted to string payloads for
WIT `result<_, string>` (raw DOMException/Error payloads cannot be lowered).
Other cases abort after five chunks, during delayed headers, and after headers
with a stalled body. Each abort must return an error within one second;
delayed-header/body cases abort at 500 ms and must finish within one second of
starting. A final busy-loop export must trap after an epoch increment.

`Abort::request()` sets an AtomicBool **and wakes registered header/body futures**.
Setting only an atomic or incrementing the epoch cannot wake a blocked WASI poll.
The host owns its Store on a plain std::thread, blocking on crossbeam `recv`.
The HTTP async tasks run on Wasmtime-WASI's internal runtime; the agent thread
never creates a runtime or calls block_on. Wasmtime 36.0.19 no longer accepts
`bindgen async: false`; `imports/exports: { default: ignore_wit }` explicitly
produces synchronous bindings instead. Trapped instances are dropped, never
reused. Full reinstantiate + open + abort/resume is PR5, not implemented here.

## Feature/artifact boundary

Default features are empty. `agent` enables all Wasmtime/HTTP code and the
`include_bytes!` of `crates/companion-agent-host/src/agent-core.wasm`. build.rs
checks the file **only if CARGO_FEATURE_AGENT is set**, with an actionable
`npm run componentize` error. Default workspace build/test needs neither Node
nor a Wasm file. The generated artifact and node_modules/dist are gitignored.
The independent CI agent job uploads the component and extracted WIT, then runs
the spike; the Rust/PDF job does not depend on it.

## Security / intentional spike limits

- No keys were used or searched for. A real OpenRouter request is blocked-on-key.
- All three Earendil packages installed successfully at exactly 1.4.0. The raw
  Wasm SSE spike is deliberately independent of them. `test:mock` exercises the
  actual pi-ai OpenRouter provider and OpenAI-compatible parser against localhost
  using an explicit dummy CredentialStore and empty AuthContext, in Node only.
- The production HTTP policy checks the normative HTTPS hostname/port allowlist,
  then **denies external traffic** until PR5 supplies keychain injection and
  DNS/private-address pinning. It never leaks guest Authorization. This is a
  deliberate fail-closed deviation, not a shipping provider proxy.
- Only `#[cfg(test)]` can enable HTTP 127.0.0.1, scoped to one server port. No env
  switch enables it in production. No response/request bodies are logged.
- The response is header-first and piped through a running 32 MiB cap and a
  120-second total deadline; it is never collected. Guest timeouts are capped.
- ComponentizeJS 0.22.0 supports `stdio`, `random`, `clocks`, `http`, `fetch-event`
  feature toggles, not `cli` or `filesystem`. We disable fetch-event and pass
  `env: {}` to Wizer. The host builds a fresh EMPTY WasiCtx: no inherited env,
  args or preopened directories (tests inherit stderr only for diagnostics). Thus retained filesystem/CLI imports
  grant no host capabilities. No `disable: cli` environ trap workaround is needed.
- npm audit reports a critical `decompress` archive-extraction advisory through
  the pinned componentize-js/weval build-tool chain (plus two moderate entries).
  Do not use these pinned build tools on untrusted archives; upgrading the
  mandated componentize-js pin needs a separate dependency review.

## WASI versions

`cd agent && npx jco wit ../crates/companion-agent-host/src/agent-core.wasm`
reports `wasi:{io,cli,clocks,filesystem,random,http}@0.2.10`. The pinned
wasmtime-wasi / wasmtime-wasi-http 36.0.19 sources declare **0.2.6**, not 0.2.10.
These are not exact version matches. Wasmtime's component linker consults
semver-compatible interface names. Successful instantiation and the streaming
harness are the compatibility check for the interfaces actually used here;
do not infer that every new 0.2.10 interface is available in 36.0.19.

The spike sets Cranelift `OptLevel::None` and compiles the immutable component
once, then instantiates fresh Stores for every case (never reuse a trapped one).
The test prints both `instantiate=...` cold-start milliseconds and separate
`compile=...` Cranelift milliseconds. Compilation is not silently included in
or hidden behind the instantiation measurement.
