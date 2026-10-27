#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_BUILD_JOBS=1
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}"
export CARGO_PROFILE_TEST_DEBUG="${CARGO_PROFILE_TEST_DEBUG:-0}"
if [[ "${1:-}" != --built ]]; then
  (cd agent && npm ci && npm run build && npm run componentize)
fi
(cd agent && npx jco wit ../crates/companion-agent-host/src/agent-core.wasm)
cargo test -p companion-agent-host --features agent -- --nocapture --test-threads=1
