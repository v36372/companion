#!/usr/bin/env bash
# Debug-profile, fresh-process load + instantiate measurements; no credentials.
# Source the platform's GPUI link environment before invoking this script.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_BUILD_JOBS=1
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
export COMPANION_AGENT=wasm
unset COMPANION_COMPONENT_CACHE
cache=$(mktemp -d "$PWD/.component-cache-bench.XXXXXX")
trap 'rm -rf "$cache"' EXIT
export COMPANION_CACHE_DIR="$cache"
measure() {
    cargo test -p companion-agent-host --features agent "$1" -- \
        --exact --ignored --nocapture --test-threads=1
}
for run in 1 2 3; do
    echo "BEFORE $run (main e5e8239 compile path)"
    measure wasm::measurement::component_time_to_ready_before
done
for run in 1 2 3; do
    rm -rf "$cache/components"
    echo "AFTER COLD $run"
    measure wasm::measurement::component_time_to_ready
done
for run in 1 2 3; do
    echo "AFTER WARM $run"
    measure wasm::measurement::component_time_to_ready
done
