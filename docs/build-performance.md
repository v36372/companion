# Build performance

## Fast paths

- Local: run `scripts/build-agent.sh` from any directory. It installs pinned npm
  dependencies, bundles and componentizes only if the nonempty Wasm is missing or
  older than an input. Inputs include `agent/src`, WIT, root agent `.mjs`/JSON
  files and the helper itself. No Node dependencies are needed on a hit. This is
  a timestamp heuristic: use `--force` after restoring old timestamps or deleting
  inputs. Direct `npm run build && npm run componentize` remains unconditional.
- CI: an exact `actions/cache` key covers guest sources, root agent build scripts,
  package manifest/lock, TypeScript config and all WIT files, scoped to Linux,
  runner architecture, Node 22 and a versioned component recipe. There is **no
  partial-key restore**. Both Wasm and its WIT dump are cached. On a hit only
  bundling/componentizing/WIT generation are skipped: npm installation, unit
  tests, typechecking, provider mock, Clippy, component harness and bridge tests
  still run, and both generated artifacts are uploaded.
- CI Rust uses `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0` and
  `CARGO_INCREMENTAL=0`. This matches `spike-sse.sh`, reducing debug-info overhead
  and avoiding additional profile variants. Clippy still needs check metadata;
  it does not replace the code generation performed by build/test. The tradeoff is no Rust
  debug information in CI; local Cargo defaults and release builds are unchanged.
  Rust caches remain job-specific (the agent job has a different feature graph)
  and can save on failure. Both Rust and component caches can be saved on PRs as
  well as main; GitHub's normal branch cache isolation applies, and a PR entry
  does not populate main's cache. A main-only save policy was considered but not
  added: after a profile/dependency change it would force every revision of a PR
  to rebuild dependencies until main catches up.

The explicit workspace build remains: `cargo test --no-run` alone does not
replace checking all normal binary targets. Formatting, both `-D warnings`
Clippy feature sets, tests and the single-GPUI guard are unchanged.

## Measurements

Local environment: 2 vCPU/~3.9 GiB VM, Rust 1.95.0, Node 24.22.1, Linux x86_64,
GPUI sysroot environment. Shared target; no clean build or second target.
`CARGO_BUILD_JOBS=1 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
CARGO_INCREMENTAL=0`, `CARGO_TARGET_DIR=/home/exedev/codes/.coord/target-companion`.
Each entry is wall-clock seconds, median of three runs.

| Scenario | Before | After |
| --- | ---: | ---: |
| Repeated agent build, unchanged inputs | 10.22 | <0.01 |
| `cargo build -p companion`, no source change | 2.97 | 2.11 |
| Touch `crates/companion/src/main.rs`, build companion | 2.71 | 6.45 |
| Touch `crates/companion-agent-host/src/lib.rs`, build companion | 5.14 | 2.84 |
| `cargo test --workspace --no-run` | 3.29 | 0.39 |

Before the agent fast path: `(cd agent && npm run build && npm run componentize)`;
after: `scripts/build-agent.sh` following one successful rebuild. Raw before
samples: 10.77/9.27/10.22; after: 0.01/0.00/0.00 (GNU time's 0.01 s resolution).
The helper's input invalidation, missing/empty artifact, forced rebuild and
argument handling were also tested with a fake npm fixture.

Rust reproduction: warm once with `cargo build -p companion`, then run each
scenario three times under `/usr/bin/time -f %e`; touch the named file immediately
before each leaf/host sample. No Rust source or local profile settings changed.
**The Rust deltas are noise, not claimed improvements:** concurrent workers
rebuild the same package ids in the shared target, and npm checks also competed
for CPU. First test samples may include test-binary rebuilds. Raw before/after
samples: no-op 3.45/2.97/2.73 vs 9.11/2.11/0.64; leaf 2.71/2.55/3.71 vs
6.45/7.06/3.71; host 4.78/5.14/7.68 vs 3.18/2.84/2.75; tests 17.20/3.29/0.33
vs 2.03/0.39/0.31. CI is the reference for dependency-build costs, not these
contended local samples.

CI job durations and cache-hit measurements are recorded in the PR body.
The last-green main run before opening the PR was 40666979499 (79057c7), with
both Clippy gates: rust **254 s**, agent **568 s**. Both Rust caches were exact
hits (~939 MiB rust/~1686 MiB agent); restore took 25/54 s. Clippy took
180/270 s; componentization took 7 s. The earlier starting baseline was
40613554497 (e5e8239): rust 69 s, agent 324 s, but it predates Clippy and is
not a like-for-like comparison. These are warm dependency builds, not clean
build measurements; no local clean build was performed.

## Evaluated but not added

- `cargo rustc -p companion --bin learning-companion -- --print link-args`
  confirmed `-fuse-ld=lld` on the pinned toolchain. `command -v mold` found no
  mold. No linker override or new system dependency was added: there is no
  measured mold advantage here, and an unconditional override would break local
  machines without it.
- No workspace dev/test profile changes, per-dependency optimization,
  split-debuginfo or codegen-unit tuning: this VM already requires debug=0 and
  incremental=0, so changing defaults would force expensive dependency rebuilds
  without a controlled baseline. Optimizing heavy dependencies also risks making
  edit/build cycles slower. CI-only environment alignment is the narrow change.
- No merged build/test step or skipped gate. The existing Rust cache is working;
  the focus is avoiding duplicate artifact variants and generated guest work.
