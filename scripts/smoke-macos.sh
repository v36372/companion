#!/usr/bin/env bash
# Run from any directory on a logged-in macOS GUI session. No build or network.
# Usage: scripts/smoke-macos.sh [release-binary] [artifact-directory]
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${1:-$root/target/release/learning-companion}"
artifacts="${2:-$(mktemp -d "${TMPDIR:-/tmp}/companion-smoke-logs.XXXXXX")}"
seconds="${COMPANION_SMOKE_SECONDS:-15}"
[[ "$(uname -s)" == Darwin ]] || { echo 'This smoke requires a macOS GUI session.' >&2; exit 2; }
[[ "$seconds" =~ ^[1-9][0-9]*$ ]] || { echo 'COMPANION_SMOKE_SECONDS must be a positive integer.' >&2; exit 2; }
[[ -x "$binary" ]] || { echo "Release binary is not executable: $binary" >&2; exit 2; }

# Do not read, remove or replace a developer's credentials. CI runners have none;
# locally refuse an existing credential rather than claiming a no-key smoke.
for account in openrouter brave; do
    if security find-generic-password -s learning-companion -a "$account" >/dev/null 2>&1; then
        echo "Use a clean macOS account: learning-companion/$account exists in Keychain." >&2
        exit 2
    fi
done
for provider in OPENROUTER BRAVE OPENAI ANTHROPIC GOOGLE; do
    unset "COMPANION_${provider}_API_KEY"
done
unset COMPANION_TEST_PROVIDER_BASE_URL

mkdir -p "$artifacts"
artifacts="$(cd "$artifacts" && pwd)"
state="$(mktemp -d "${TMPDIR:-/tmp}/companion-smoke-state.XXXXXX")"
pid=''
cleanup() {
    if [[ -n "$pid" ]]; then
        kill "$pid" 2>/dev/null || true
        # Bound teardown even if the agent is still compiling its cold component.
        for ((i = 0; i < 5; i++)); do
            kill -0 "$pid" 2>/dev/null || break
            sleep 1
        done
        kill -KILL "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    fi
    rm -rf "$state"
    echo "Smoke artifacts: $artifacts"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

export COMPANION_DATA_DIR="$state/data"
export COMPANION_CACHE_DIR="$state/cache"
export COMPANION_AGENT=wasm
export RUST_LOG=companion=info
"$binary" "$root/crates/companion-pdf/tests/fixtures/hello.pdf" >"$artifacts/app.log" 2>&1 &
pid=$!

# Stay alive for the whole observation period, not just until the first log line.
for ((i = 0; i < seconds; i++)); do
    if ! kill -0 "$pid" 2>/dev/null; then
        echo 'Learning Companion exited before the smoke observation period ended.' >&2
        tail -100 "$artifacts/app.log" >&2
        exit 1
    fi
    sleep 1
done
# Screenshot is diagnostic only (Screen Recording permissions can prevent it).
if ! screencapture -x "$artifacts/screenshot.png" 2>"$artifacts/screencapture.log"; then
    echo 'Screenshot unavailable; see screencapture.log.' >&2
fi
kill -0 "$pid" || { echo 'Learning Companion exited during smoke.' >&2; exit 1; }
if ! grep -Fq 'Learning Companion window opened' "$artifacts/app.log"; then
    echo 'Missing window-open INFO log.' >&2
    tail -100 "$artifacts/app.log" >&2
    exit 1
fi
echo "PASS: release process survived ${seconds}s and logged window creation."
