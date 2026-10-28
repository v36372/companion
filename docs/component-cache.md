# Compiled agent component cache

`companion-agent-host::wasm::Runtime::load` is the single loading path for the
app and component integration tests. Compilation already runs on the bridge's
background OS thread, not the UI thread. Fresh Stores and trapped-instance
recovery continue to reuse the immutable in-process component.

On Unix, the disk cache defaults to the **cache directory** of
`ProjectDirs::from("", "LearningCompanion", "learning-companion")` (not the data
directory). For example, Linux uses `$XDG_CACHE_HOME/learning-companion` or
`~/.cache/learning-companion`. `COMPANION_CACHE_DIR` overrides that root.
`COMPANION_COMPONENT_CACHE=off` disables reading and writing the cache.

Files are `components/agent-core-<sha256>.cwasm`. The digest includes the embedded
Wasm, the pinned Wasmtime version, and the engine compatibility fingerprint via
its `Hash` implementation. Wasmtime guarantees that matching fingerprints cover
compatible code-generation settings; this includes our epoch interruption and
Cranelift optimization settings. A unit test guards the explicit version against
the workspace pin.

## Trust and failure handling

Serialized components contain **native machine code**. They are not validated
untrusted Wasm. We only deserialize user-owned regular files with mode **0600**
inside a user-owned, non-symlink **0700** components directory. The cache root must
also be owned by the user and not writable by other users. Other processes
running as the same user are trusted, as they are for the app executable itself.
Do not share the directory with untrusted processes. Arbitrary existing caches
with more permissive modes are not trusted or silently chmodded into trust.

Missing artifacts are compiled normally. A failed deserialize logs a warning,
removes the bad file, and recompiles. Successful compilation serializes to a
0600 temporary file in the same directory, syncs it, and renames it atomically.
Only after a successful write are other `agent-core-*.cwasm` keys removed. Read,
write, or cleanup failures log warnings and never prevent agent startup; genuine
Wasm compilation errors still fail normally. Cache removal is always safe.

Platforms without Unix owner/mode verification currently use uncached
compilation rather than loading unverified executable artifacts. Windows ACL
support is a possible follow-up.

## Tests and timings

Focused tests use tiny valid components and temporary directories. They verify
miss/write/name/modes, hit without invoking the compile hook, corrupt-file
recompilation/replacement, byte/config key changes, stale-key cleanup, symlink
and insecure-directory rejection, nonfatal write failures, and a subprocess
kill-switch test (no process-global environment mutation).

For isolated integration tests, set `COMPANION_CACHE_DIR` to a private directory.
The existing test helpers still compile/load once per binary via `OnceLock`,
and now share the same on-disk cache path as production. At least one focused
test explicitly bypasses the cache.

Reproduce timings from the repository root (generate `agent-core.wasm` first):

```sh
. ~/.cargo/env
source ~/.local/sysroot-env.sh # this VM's GPUI linking environment only
export CARGO_TARGET_DIR=/home/exedev/codes/.coord/target-companion
scripts/component-cache-bench.sh
```

The script launches a fresh debug-profile test process for every sample. It
measures component **load + instantiate**, including engine/linker/state setup,
without opening a provider or reading credentials. The baseline helper reproduces
main `e5e8239`'s original `Engine` + `Component::from_binary` path verbatim; the
after helper calls production `Runtime::load`. Both call the same production
`instantiate`. Three baseline and three empty-cache samples precede three warm
samples. Cargo build time is excluded. The temporary cache is removed on exit.

Measured on this Linux x86_64 VM (2 vCPUs, ~3.9 GiB RAM), Rust 1.95.0,
**debug/unoptimized** profile with debug symbols and incremental compilation
disabled, using the exact command above:

| Path / run | Load (ms) | Instantiate/setup (ms) | Time to ready (ms) |
| --- | ---: | ---: | ---: |
| Before 1 | 202845.7 | 55.0 | 202900.7 |
| Before 2 | 132457.9 | 12.6 | 132470.5 |
| Before 3 | 91027.9 | 14.7 | 91042.6 |
| After cold 1 | 165219.3 | 38.4 | 165257.7 |
| After cold 2 | 83048.3 | 11.9 | 83060.2 |
| After cold 3 | 78051.9 | 11.2 | 78063.0 |
| After warm 1 | 1090.1 | 3.1 | 1093.3 |
| After warm 2 | 1220.6 | 3.1 | 1223.7 |
| After warm 3 | 1090.9 | 3.2 | 1094.1 |

Median time to ready: before **132.471 s**, cold **83.060 s**, warm **1.094 s**.
Other workers were building/testing on this shared VM during earlier samples,
so the compile times vary substantially. Do not infer a cold-path speedup from
these numbers; cold launches still compile. Debug numbers also overstate compile
time versus a release build. The reliable improvement is that warm launches
skip compilation, confirmed independently by the unit-test compile counter.
No release build, display, real API keys, or provider requests were used.
