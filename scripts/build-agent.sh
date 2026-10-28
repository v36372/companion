#!/usr/bin/env bash
# Fast local rebuild; CI uses an exact content-keyed component cache instead.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $# -gt 1 || ( $# -eq 1 && $1 != --force ) ]]; then
  echo "Usage: $0 [--force]" >&2
  exit 2
fi
wasm=crates/companion-agent-host/src/agent-core.wasm
stale=true
if [[ ${1:-} != --force && -s $wasm ]]; then
  stale=false
  # Include this script, build config and lockfile, not node_modules or dist.
  for input in scripts/build-agent.sh agent/*.mjs agent/*.json; do
    if [[ $input -nt $wasm ]]; then stale=true; break; fi
  done
  if [[ -n $(find agent/src wit -newer "$wasm" -print -quit) ]]; then
    stale=true
  fi
fi
if [[ $stale == false ]]; then
  echo "Agent component is up to date; skipping npm install/build/componentize."
  exit 0
fi
(cd agent && npm ci && npm run build && npm run componentize)
