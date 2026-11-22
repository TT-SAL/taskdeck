#!/usr/bin/env bash
# Tests for crates/taskdeck-android/ci.sh, the android job's hook, on a
# throwaway tree with stand-ins for scripts/dev-setup.sh and
# scripts/check-android-targets.sh that log how they were run.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

R="$TMP/tree"
LOG="$TMP/log"
export FAKE_LOG="$LOG"
mkdir -p "$R/crates/taskdeck-android" "$R/scripts"
cp "$REPO_DIR/crates/taskdeck-android/ci.sh" "$R/crates/taskdeck-android/ci.sh"
cat >"$R/scripts/dev-setup.sh" <<'EOF'
echo "dev-setup $*" >>"$FAKE_LOG"
if [ "$1" = --env ]; then echo 'export ANDROID_NDK_HOME="/made/up/ndk"'; exit 0; fi
exit "${DS_EXIT:-0}"
EOF
cat >"$R/scripts/check-android-targets.sh" <<'EOF'
echo "check ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-unset} in $(basename "$PWD")" >>"$FAKE_LOG"
exit "${CHECK_EXIT:-0}"
EOF

# Run from another directory: the hook finds the root itself.
run() { : >"$LOG"; (cd "$TMP" && env -u ANDROID_NDK_HOME "$@" bash "$R/crates/taskdeck-android/ci.sh"); }
log_is() {
    if [[ "$(cat "$LOG")" == "$2" ]]; then
        PASSES=$((PASSES + 1)); echo "ok    $1"
    else
        FAILS=$((FAILS + 1)); echo "FAIL  $1"; sed 's/^/        got: /' "$LOG"; sed 's/^/        wanted: /' <<<"$2"
    fi
}

expect "linux: the check passes" 0 run CI_OS=linux
log_is "linux: install the row, take the exports, run the check" "dev-setup android-targets
dev-setup --env
check ANDROID_NDK_HOME=/made/up/ndk in tree"
expect "linux: a failing check fails the hook" 1 run CI_OS=linux CHECK_EXIT=1
expect "linux: a failed install fails the hook before the check" 1 run CI_OS=linux DS_EXIT=1
log_is "and the check does not run" "dev-setup android-targets"
for os in windows macos ""; do
    expect "'$os': nothing to do" 0 run CI_OS="$os"
    output_has "'$os': it says why" "runs on Linux only"
    log_is "'$os': nothing runs" ""
done

finish
