#!/usr/bin/env bash
# Tests for scripts/check-android-targets.sh on a throwaway tree, with stand-ins
# for scripts/dev-setup.sh (it prints $DS_ENV for --env), cargo and cargo-ndk:
# the NDK it uses is the one dev-setup.sh exports, never a stale
# ANDROID_NDK_HOME of the caller.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

R="$TMP/tree"
LOG="$TMP/log"
export FAKE_LOG="$LOG"
mkdir -p "$R/scripts" "$TMP/bin" "$TMP/ndk"
cp "$REPO_DIR/scripts/check-android-targets.sh" "$R/scripts/"
cat >"$R/scripts/dev-setup.sh" <<'EOF'
[ "$1" = --env ] && printf '%s\n' "${DS_ENV:-}"
EOF
cat >"$TMP/bin/cargo" <<'EOF'
case "$1" in
    tree) case " $* " in *" -p taskdeck-device "*) echo "cc v1.0.0" ;; *) echo "made-up v0.1.0" ;; esac ;;
    ndk) echo "ndk ANDROID_NDK_HOME=$ANDROID_NDK_HOME" >>"$FAKE_LOG" ;;
    *) echo "$1" >>"$FAKE_LOG" ;;
esac
EOF
printf '#!/bin/sh\n' >"$TMP/bin/cargo-ndk"
chmod +x "$TMP/bin/cargo" "$TMP/bin/cargo-ndk"

run() { : >"$LOG"; env PATH="$TMP/bin:$PATH" "$@" bash "$R/scripts/check-android-targets.sh"; }

expect "the NDK dev-setup exports is used" 0 run ANDROID_NDK_HOME=/made/up/stale \
    DS_ENV="export ANDROID_NDK_HOME=\"$TMP/ndk\""
expect "and not the caller's stale one" 0 grep -qxF "ndk ANDROID_NDK_HOME=$TMP/ndk" "$LOG"
expect "a stale ANDROID_NDK_HOME that dev-setup rejects fails" 1 run ANDROID_NDK_HOME=/made/up/stale DS_ENV=
output_has "it names the variable" "ANDROID_NDK_HOME=/made/up/stale names no NDK that dev-setup.sh accepts"
output_has "and the setup to run" "run: bash scripts/dev-setup.sh android-targets"
expect "and cargo ndk does not run" 1 grep -q '^ndk' "$LOG"

finish
