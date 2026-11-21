#!/usr/bin/env bash
# Proves that the workspace's release profile unwinds on panic, so the sync
# design's catch_unwind guards (SYNC.md 4.5; the JNI entry points) work.
#
# `cargo test --release` cannot show this: cargo builds test harnesses with
# unwinding whatever the profile says. So this builds the tiny `panic-probe`
# binary with the real release profile and runs it. It exits 0 only if a panic
# inside catch_unwind was caught and execution continued.
#
# Runs from any directory, on Linux, macOS and Git-Bash on Windows. Honours
# CARGO_TARGET_DIR and cargo's own target-dir configuration.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

fail() {
    echo >&2
    echo "check-release-panic: FAIL: $*" >&2
    echo "check-release-panic: is [profile.release] panic = \"unwind\" in $root/Cargo.toml?" >&2
    exit 1
}

echo "check-release-panic: building panic-probe with the release profile"
# The JSON messages name the built executable, wherever the target dir is.
messages="$(cargo build --release --locked -p panic-probe \
    --message-format=json-render-diagnostics)" || fail "the release build failed"

bin="$(printf '%s\n' "$messages" \
    | grep '"reason":"compiler-artifact"' \
    | grep '"name":"panic-probe"' \
    | sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' \
    | tail -n 1)"
# On Windows the path arrives JSON-escaped (C:\\a\\b); Git-Bash runs C:/a/b.
bin="${bin//\\\\//}"
[ -n "$bin" ] || fail "cargo did not report a panic-probe executable"
[ -f "$bin" ] || fail "the reported executable does not exist: $bin"

echo "check-release-panic: running $bin"
set +e
"$bin"
status=$?
set -e

if [ "$status" -ne 0 ]; then
    fail "panic-probe exited with status $status, so the panic was not caught (134 is SIGABRT on Linux and macOS; on Windows an abort shows as a large fail-fast code)"
fi
echo "check-release-panic: OK: release builds unwind, and catch_unwind caught the panic"
