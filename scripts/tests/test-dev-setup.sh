#!/usr/bin/env bash
# Tests for scripts/dev-setup.sh. No network: archives come from --from-dir,
# and the install path is exercised with a made-up tool archive whose hashes
# the test computes itself.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

DEV="$SCRIPTS_DIR/dev-setup.sh"

# --- the command line --------------------------------------------------------
mkdir -p "$TMP/empty-tools"
expect "--check reports missing tools" 1 env TASKDECK_TOOLS_DIR="$TMP/empty-tools" bash "$DEV" --check foundation
output_has "--check names cargo-deny as missing" "missing  cargo-deny 0.20.2"
output_has "--check names cargo-audit as missing" "missing  cargo-audit 0.23.0"
output_has "--check says how to install" "run: bash scripts/dev-setup.sh foundation"
output_has "--check prints the tool directory" "tool directory: $TMP/empty-tools"

mkdir -p "$TMP/other-tools"
printf '#!/bin/sh\necho "cargo-deny 0.20.2"\n' >"$TMP/other-tools/cargo-deny"
chmod +x "$TMP/other-tools/cargo-deny"
expect "--check refuses a binary that is not the pinned one" 1 env TASKDECK_TOOLS_DIR="$TMP/other-tools" bash "$DEV" --check foundation
output_has "--check reports the differing binary" "differs  cargo-deny"

# A wrong archive offered through --from-dir is refused before anything is
# installed, without any download.
mkdir -p "$TMP/bad-archives" "$TMP/bad-tools"
echo "not the real archive" >"$TMP/bad-archives/cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz"
echo "not the real archive" >"$TMP/bad-archives/cargo-audit-x86_64-unknown-linux-musl-v0.23.0.tgz"
expect "a wrong checksum is refused" 1 env TASKDECK_TOOLS_DIR="$TMP/bad-tools" bash "$DEV" --from-dir "$TMP/bad-archives" foundation
output_has "the mismatch is reported" "CHECKSUM MISMATCH for cargo-deny 0.20.2"
output_has "the expected hash is printed" "expected 9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f"
if [[ -e "$TMP/bad-tools/cargo-deny" || -e "$TMP/bad-tools/cargo-audit" ]]; then
    FAILS=$((FAILS + 1)); echo "FAIL  nothing installed after a mismatch"
else
    PASSES=$((PASSES + 1)); echo "ok    nothing installed after a mismatch"
fi

expect "--from-dir without the archive fails" 1 env TASKDECK_TOOLS_DIR="$TMP/bad-tools" bash "$DEV" --from-dir "$TMP/empty-tools" foundation
output_has "the missing archive is named" "cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz not found"

expect "an unknown stage fails" 1 bash "$DEV" --check no-such-stage
output_has "the known stages are listed" "known: foundation"
expect "--list prints the stage table" 0 bash "$DEV" --list
output_has "foundation needs cargo-deny and cargo-audit" "foundation: cargo-deny cargo-audit"
expect "no stage is a usage error" 2 bash "$DEV" --check

# --- the install path, with a made-up tool -----------------------------------
# Sourcing defines the functions without running main.
source "$DEV"
set +eu
WORK_DIR="$TMP/work"
mkdir -p "$WORK_DIR" "$TMP/fake-src/fake-1.0" "$TMP/archives"
printf '#!/bin/sh\necho fake-tool 1.0\n' >"$TMP/fake-src/fake-1.0/fake-tool"
chmod +x "$TMP/fake-src/fake-1.0/fake-tool"
tar -czf "$TMP/archives/fake-1.0.tar.gz" -C "$TMP/fake-src" fake-1.0
ASHA="$(sha256_of "$TMP/archives/fake-1.0.tar.gz")"
BSHA="$(sha256_of "$TMP/fake-src/fake-1.0/fake-tool")"
URL="https://example.invalid/releases/download/fake%2Fv1.0/fake-1.0.tar.gz"
FROM_DIR="$TMP/archives"

TOOLS_DIR="$TMP/tools"
expect "a matching archive installs" 0 install_release_tarball fake-tool 1.0 "$URL" "$ASHA" fake-1.0/fake-tool "$BSHA"
output_has "install says where" "installed fake-tool 1.0 into $TMP/tools"
expect "the installed binary has the pinned hash" 0 test "$(status_of fake-tool "$BSHA")" = ok
expect "a second run is a no-op" 0 install_release_tarball fake-tool 1.0 "$URL" "$ASHA" fake-1.0/fake-tool "$BSHA"
output_has "the second run skips" "fake-tool 1.0 already installed"

echo "tampered" >>"$TMP/tools/fake-tool"
expect "a changed binary reads as differs" 0 test "$(status_of fake-tool "$BSHA")" = differs
expect "a changed binary is replaced" 0 install_release_tarball fake-tool 1.0 "$URL" "$ASHA" fake-1.0/fake-tool "$BSHA"
output_has "the replacement is reported" "replaced a different fake-tool"
expect "the replacement has the pinned hash" 0 test "$(status_of fake-tool "$BSHA")" = ok

TOOLS_DIR="$TMP/tools2"
WRONG="0000000000000000000000000000000000000000000000000000000000000000"
expect "a wrong archive hash is refused" 1 install_release_tarball fake-tool 1.0 "$URL" "$WRONG" fake-1.0/fake-tool "$BSHA"
output_has "the refusal is loud" "CHECKSUM MISMATCH for fake-tool 1.0"
expect "nothing is installed after the refusal" 0 test ! -e "$TMP/tools2/fake-tool"
expect "a wrong binary hash is refused" 1 install_release_tarball fake-tool 1.0 "$URL" "$ASHA" fake-1.0/fake-tool "$WRONG"
output_has "the binary mismatch is reported" "extracted binary has SHA-256 $BSHA"
expect "nothing is installed after the binary refusal" 0 test ! -e "$TMP/tools2/fake-tool"

# --- --check when the pinned binaries are there ------------------------------
# Stand-in binaries, with the pins pointed at their hashes; main runs in the
# subshell expect opens, so its exit does not end this file.
mkdir -p "$TMP/ok-tools"
printf '#!/bin/sh\necho stand-in deny\n' >"$TMP/ok-tools/cargo-deny"
printf '#!/bin/sh\necho stand-in audit\n' >"$TMP/ok-tools/cargo-audit"
CARGO_DENY_BINARY_SHA256="$(sha256_of "$TMP/ok-tools/cargo-deny")"
CARGO_AUDIT_BINARY_SHA256="$(sha256_of "$TMP/ok-tools/cargo-audit")"
TOOLS_DIR="$TMP/ok-tools"
FROM_DIR=""
expect "--check passes when every pinned binary is there" 0 main --check foundation
output_has "--check reports cargo-deny ok" "ok       cargo-deny $CARGO_DENY_VERSION"
output_has "--check reports cargo-audit ok" "ok       cargo-audit $CARGO_AUDIT_VERSION"
output_lacks "--check asks for no install" "run: bash scripts/dev-setup.sh"
rm "$TMP/ok-tools/cargo-audit"
expect "--check fails when one of two is missing" 1 main --check foundation
output_has "the present tool is still ok" "ok       cargo-deny $CARGO_DENY_VERSION"
output_has "the absent tool is missing" "missing  cargo-audit $CARGO_AUDIT_VERSION"
expect "an install run skips the present tool" 1 main --from-dir "$TMP/empty-tools" foundation
output_has "the present tool is skipped" "cargo-deny $CARGO_DENY_VERSION already installed"
output_has "the absent one is looked for" "cargo-audit-x86_64-unknown-linux-musl-v0.23.0.tgz not found"

finish
