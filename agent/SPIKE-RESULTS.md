# PR4 spike evidence — 2026-10-27

**VERDICT: PASS** — actual ComponentizeJS Wasm fetch, on-stack throughout the
stream, incremental `on-stream` calls, mid-body and TTFB cancellation, and epoch
interruption. This is not a real remote LLM request.

Validated locally on Node 24.22.1, ComponentizeJS 0.22.0, jco 1.15.1,
Wasmtime/WASI/HTTP 36.0.19. CI selects Node 22. All Rust commands used one Cargo
build job and disabled dev/test debug information. Linux workspace commands used
the supplied user-level sysroot.

## Commands / outcomes

- `cd agent && npm ci && npm run build && npm run componentize`: PASS. All three
  Earendil dependencies installed at 1.4.0; none is bundled into the raw SSE guest.
- `scripts/spike-sse.sh --built`: PASS, all three streaming/abort/epoch and
  policy/cap/wakeup/timeout tests (final rerun: 3 passed, 84.14 s). The timestamp
  table below records the first successful run.
- `cargo test -p companion-agent-host --features agent http_proxy::tests -- --nocapture --test-threads=1`:
  PASS, two policy/running-cap/abort-wakeup/timeout tests.
- `cd agent && node mock-provider.mjs`: PASS. The actual pi-ai OpenRouter provider
  parsed 12 local OpenAI-compatible SSE text deltas, first at 130.971 ms and last
  at 1227.199 ms. **This extra provider check runs in Node, not Wasm.**
- With `agent-core.wasm` temporarily absent: `cargo build --workspace`,
  `cargo test --workspace`, `cargo fmt --all -- --check`: PASS. Two existing PDF
  integration tests passed; all unit/doc test targets passed. The file was then
  restored. `cargo build -p companion-agent-host --features agent` with it absent
  failed as expected with `agent-core.wasm is missing: cd agent && npm ci && npm
  run build && npm run componentize (or build without --features agent)`.

## On-stack chunks

Milliseconds from issuing `Command::Sse`, measured by `Instant` in the host import:

| Chunk | ms | Chunk | ms | Chunk | ms |
|---:|---:|---:|---:|---:|---:|
| 0 | 7.166 | 10 | 1008.842 | 20 | 2011.289 |
| 1 | 106.869 | 11 | 1109.184 | 21 | 2111.434 |
| 2 | 206.824 | 12 | 1209.343 | 22 | 2211.684 |
| 3 | 308.594 | 13 | 1309.554 | 23 | 2311.902 |
| 4 | 407.070 | 14 | 1409.778 | 24 | 2412.453 |
| 5 | 507.512 | 15 | 1510.137 | 25 | 2512.626 |
| 6 | 607.804 | 16 | 1610.316 | 26 | 2612.616 |
| 7 | 708.043 | 17 | 1710.512 | 27 | 2712.798 |
| 8 | 808.322 | 18 | 1810.876 | 28 | 2813.116 |
| 9 | 908.613 | 19 | 1911.028 | 29 | 2913.275 |

Export returned at **3013.700 ms**, reporting 30 chunks / 680 bytes. Tests enforce
at least ten 50–250 ms gaps and first chunk before one second.

## Cancellation

| Case | Last chunk / abort trigger | Export completion | Error after abort |
|---|---|---:|---:|
| Mid-body | chunk 4 at 406.434 ms | 408.389 ms | 1.811 ms |
| TTFB (headers delayed 5 s) | timer at ~500 ms | 503.212 ms | 2.643 ms |
| Headers ready, body delayed 5 s | timer at ~500 ms | 502.795 ms | 2.552 ms |
| Busy loop | atomic + epoch after 100 ms | interrupt trap | 0.413 ms |

Mid-body / stalled-body returned `TypeError: IO error reading from incoming
response body`; delayed headers returned `TypeError: NetworkError when attempting
to fetch resource`. HTTP abort cases deliberately do **not** increment the epoch,
proving that cancellation wakes the HTTP future/body pollable independently.

## Startup and compatibility caveats

First instantiation: **5.408 ms**, later fresh instances: 2.084 / 2.063 / 1.875 ms.
Compiling the component once with Cranelift `OptLevel::None`: **69236.862 ms**.
The <2 s instantiation target passes; **uncached end-to-end startup does not**.
A safe compilation cache/precompiled artifact strategy is follow-up work.

`npx jco wit ../crates/companion-agent-host/src/agent-core.wasm` reports
`wasi:io`, `cli`, `clocks`, `filesystem`, `random`, `http` **@0.2.10**.
Wasmtime 36.0.19 declares **@0.2.6**. These do not match exactly; the semver-aware
linker successfully instantiated and exercised all interfaces used by this spike.
No WASI version was relabeled or binary-patched.

See README for the getReader async-iterator adapter, sync bindgen option change,
empty environment/no preopens, fail-closed external policy and npm audit caveat.
Real OpenRouter streaming remains **blocked-on-key**. No credentials were sought
or used. No Bun sidecar was implemented.
