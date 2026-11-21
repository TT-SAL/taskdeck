#!/usr/bin/env bash
# Proves that the workspace's release profile unwinds on panic, so the sync
# design's catch_unwind guards (SYNC.md 4.5; the JNI entry points) work.
#
# Two checks:
# 1. Cargo.toml's [profile.release] table says panic = "unwind" (read from the
#    file, so no environment or cargo config can stand in for it).
# 2. The release profile really unwinds. `cargo test --release` cannot show
#    this: cargo builds test harnesses with unwinding whatever the profile
#    says. So this builds the tiny `panic-probe` binary with the real release
#    profile, for the host, and runs it. It exits 0 only if a panic inside
#    catch_unwind was caught and execution continued. The build ignores the
#    caller's RUSTFLAGS-style variables, CARGO_PROFILE_RELEASE_PANIC and
#    CARGO_BUILD_TARGET, which would otherwise change what is being tested.
#
# Runs from any directory, on Linux, macOS and Git-Bash on Windows. Honours
# CARGO_TARGET_DIR (a relative value is taken from the caller's directory, as
# cargo would) and cargo's own target-dir configuration.
set -euo pipefail

# Make a relative target dir from the environment absolute against the
# caller's directory before moving to the repository root. `pwd -W` gives the
# Windows form under Git-Bash, which native cargo understands.
caller="$(pwd -W 2>/dev/null || pwd)"
for var in CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR; do
    value="${!var-}"
    case "$value" in
        "" | /* | [A-Za-z]:[\\/]* | \\\\*) ;;
        *) export "$var=$caller/$value" ;;
    esac
done

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

fail() {
    echo >&2
    echo "check-release-panic: FAIL: $*" >&2
    exit 1
}

# For failures that point at the panic setting itself.
fail_unwind() {
    echo >&2
    echo "check-release-panic: FAIL: $*" >&2
    echo "check-release-panic: is [profile.release] panic = \"unwind\" in $root/Cargo.toml?" >&2
    exit 1
}

echo "check-release-panic: reading [profile.release] in Cargo.toml"
# Prints the value of `panic` in the [profile.release] table, or nothing.
manifest_panic="$(awk '
    /^[[:space:]]*\[/ {
        in_release = ($0 ~ /^[[:space:]]*\[profile\.release\][[:space:]]*(#.*)?$/)
        next
    }
    in_release && /^[[:space:]]*panic[[:space:]]*=/ {
        line = $0
        sub(/^[[:space:]]*panic[[:space:]]*=[[:space:]]*/, "", line)
        sub(/[[:space:]]*(#.*)?$/, "", line)
        print line
    }
' Cargo.toml)"
[ "$manifest_panic" = '"unwind"' ] \
    || fail_unwind "Cargo.toml's [profile.release] has panic = ${manifest_panic:-<not set>}, not \"unwind\""

host="$(rustc -vV | sed -n 's/^host: //p')"
[ -n "$host" ] || fail "could not read the host target from rustc -vV"

# Variables that would change the panic strategy or the target of this build.
unset_vars=(RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_BUILD_RUSTFLAGS
    CARGO_PROFILE_RELEASE_PANIC CARGO_BUILD_TARGET)
while IFS= read -r var; do
    case "$var" in CARGO_TARGET_*_RUSTFLAGS) unset_vars+=("$var") ;; esac
done < <(compgen -e)
unset "${unset_vars[@]}"

echo "check-release-panic: building panic-probe with the release profile for $host"
# The JSON messages name the built executable, wherever the target dir is.
messages="$(cargo build --release --locked -p panic-probe --target "$host" \
    --message-format=json-render-diagnostics)" || fail "the release build failed"

# The executable's path as a JSON string body, escapes and all.
raw="$(printf '%s\n' "$messages" \
    | grep '"reason":"compiler-artifact"' \
    | grep '"name":"panic-probe"' \
    | sed -nE 's/.*"executable":"(([^"\\]|\\.)*)".*/\1/p' \
    | tail -n 1)" || true
[ -n "$raw" ] || fail "cargo did not report a panic-probe executable"

# Decode the JSON escapes a path can carry: \\ \" \/. Anything else (\uXXXX
# for a control character) is left as is and fails the existence check below.
bin="${raw//\\\\/$'\001'}"
bin="${bin//\\\"/\"}"
bin="${bin//\\\//\/}"
bin="${bin//$'\001'/\\}"
# Git-Bash runs C:/a/b; cargo reports C:\a\b.
case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) bin="${bin//\\//}" ;;
esac
[ -f "$bin" ] || fail "could not find the executable cargo reported (JSON: $raw, decoded: $bin)"

echo "check-release-panic: running $bin"
set +e
"$bin"
status=$?
set -e

case "$status" in
    0) ;;
    2) fail "panic-probe returned without panicking, so the probe itself is broken" ;;
    126 | 127) fail "could not run panic-probe (status $status) at $bin" ;;
    *) fail_unwind "panic-probe exited with status $status, so the panic was not caught (134 is SIGABRT on Linux and macOS; on Windows an abort shows as a large fail-fast code)" ;;
esac
echo "check-release-panic: OK: Cargo.toml says unwind, release builds unwind, and catch_unwind caught the panic"
