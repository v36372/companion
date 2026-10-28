#!/usr/bin/env bash
# Portable smoke-harness regression tests with fake platform/GUI commands.
# This does NOT test the app or replace the real macOS CI launch.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/test-smoke-macos.XXXXXX")"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
printf '#!/bin/sh\necho Darwin\n' >"$work/bin/uname"
printf '#!/bin/sh\nexit "${FAKE_KEYCHAIN_STATUS:-1}"\n' >"$work/bin/security"
printf '#!/bin/sh\nexit 1\n' >"$work/bin/screencapture"
cat >"$work/bin/app" <<'APP'
#!/usr/bin/env bash
set -eu
[[ "$1" == */hello.pdf && -s "$1" ]]
[[ "$COMPANION_AGENT" == wasm && "$RUST_LOG" == companion=info ]]
[[ ! -e "$COMPANION_DATA_DIR" && ! -e "$COMPANION_CACHE_DIR" ]]
[[ -z "${COMPANION_OPENROUTER_API_KEY+x}" && -z "${COMPANION_BRAVE_API_KEY+x}" ]]
printf '%s\n' "${COMPANION_DATA_DIR%/data}" >"$FAKE_STATE_RECORD"
if [[ "${FAKE_APP_MODE:-ok}" == crash ]]; then exit 1; fi
if [[ "${FAKE_APP_MODE:-ok}" != silent ]]; then echo 'Learning Companion window opened'; fi
exec sleep 30
APP
chmod +x "$work/bin/"*
export PATH="$work/bin:$PATH" COMPANION_SMOKE_SECONDS=1
export FAKE_STATE_RECORD="$work/state-path"
export COMPANION_OPENROUTER_API_KEY=must-be-unset COMPANION_BRAVE_API_KEY=must-be-unset

"$root/scripts/smoke-macos.sh" "$work/bin/app" "$work/success" >"$work/success.log" 2>&1
[[ -s "$work/success/app.log" && -f "$work/success/screencapture.log" ]]
read -r state <"$FAKE_STATE_RECORD"
[[ ! -e "$state" ]]
for mode in crash silent; do
    if FAKE_APP_MODE="$mode" "$root/scripts/smoke-macos.sh" "$work/bin/app" "$work/$mode" >"$work/$mode.log" 2>&1; then
        echo "FAIL: $mode app incorrectly passed" >&2
        exit 1
    fi
    read -r state <"$FAKE_STATE_RECORD"
    [[ ! -e "$state" ]]
done
if FAKE_KEYCHAIN_STATUS=0 "$root/scripts/smoke-macos.sh" "$work/bin/app" "$work/keychain" >"$work/keychain.log" 2>&1; then
    echo 'FAIL: existing credential incorrectly passed' >&2
    exit 1
fi
[[ ! -e "$work/keychain" ]]
echo 'PASS: smoke harness success, early exit, missing log, credential refusal and state cleanup.'
