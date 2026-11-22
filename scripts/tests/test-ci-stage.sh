#!/usr/bin/env bash
# Tests for scripts/ci/stage.sh, on a throwaway copy of the script with a fake
# cargo on PATH: the stage-to-directory map, missing directories skipped,
# cargo test and clippy per package, the ci.sh hooks and their variables, and
# that every step runs before the script fails.
set -uo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

R="$TMP/repo"
mkdir -p "$R/scripts/ci" "$TMP/bin"
cp "$SCRIPTS_DIR/ci/stage.sh" "$R/scripts/ci/stage.sh"
LOG="$TMP/log"
# The fake cargo logs its arguments and fails for a package named in CARGO_FAIL.
cat >"$TMP/bin/cargo" <<'EOF'
#!/usr/bin/env bash
echo "cargo $*" >>"$FAKE_LOG"
for a in "$@"; do [[ -n "${CARGO_FAIL:-}" && "$a" == "$CARGO_FAIL" ]] && exit 101; done
exit 0
EOF
chmod +x "$TMP/bin/cargo"
export FAKE_LOG="$LOG" PATH="$TMP/bin:$PATH"
unset CI_WIDE CI_OS CI_STAGE CARGO_FAIL

crate() {   # crate <dir> <package name>
    mkdir -p "$R/$1/src"
    printf '[package]\nname = "%s"\nversion = "0.1.0"\n\n[dependencies]\nname-like = "1"\n' "$2" >"$R/$1/Cargo.toml"
}
hook() {    # hook <dir> [exit code]: a ci.sh that logs its variables
    mkdir -p "$R/$1"
    printf 'echo "hook %s stage=$CI_STAGE os=$CI_OS wide=$CI_WIDE pwd=$(pwd)" >>"$FAKE_LOG"\nexit %s\n' "$1" "${2:-0}" >"$R/$1/ci.sh"
}
stage() { : >"$LOG"; bash "$R/scripts/ci/stage.sh" "$@"; }
log_is() {  # log_is <case> <expected log>
    if [[ "$(cat "$LOG")" == "$2" ]]; then
        PASSES=$((PASSES + 1)); echo "ok    $1"
    else
        FAILS=$((FAILS + 1)); echo "FAIL  $1"; echo "        got:"; sed 's/^/          /' "$LOG"
        echo "        wanted:"; sed 's/^/          /' <<<"$2"
    fi
}

echo "-- nothing there yet"
for s in core seal engine device recovery relay android; do
    expect "$s: missing directories are skipped, not an error" 0 env RUNNER_OS=Linux bash "$R/scripts/ci/stage.sh" "$s"
    output_has "$s: says it skipped" "not in this checkout yet; skipped"
done
expect "an unknown stage is a usage error" 2 stage frob
expect "no stage is a usage error" 2 stage
expect "two stages is a usage error" 2 stage core seal

echo "-- the directories of each stage"
crate crates/taskdeck-core taskdeck-core
expect "core: test and clippy" 0 env RUNNER_OS=Linux bash "$R/scripts/ci/stage.sh" core
: >"$LOG"; RUNNER_OS=Linux bash "$R/scripts/ci/stage.sh" core >/dev/null
log_is "core: the two cargo commands" "cargo test --locked -p taskdeck-core
cargo clippy --locked -p taskdeck-core --all-targets -- -D warnings"
crate crates/taskdeck-seal taskdeck-seal
crate crates/taskdeck-sync taskdeck-sync
crate crates/taskdeck-device taskdeck-device
crate crates/taskdeck-backup taskdeck-backup
crate crates/taskdeck-recover taskdeck-recover
crate crates/taskdeck-android taskdeck-android
for pair in seal:taskdeck-seal engine:taskdeck-sync device:taskdeck-device \
    recovery:"taskdeck-backup taskdeck-recover" android:taskdeck-android; do
    s="${pair%%:*}"
    want=""
    for p in ${pair#*:}; do
        want+="cargo test --locked -p $p"$'\n'"cargo clippy --locked -p $p --all-targets -- -D warnings"$'\n'
    done
    RUNNER_OS=Linux stage "$s" >/dev/null
    log_is "$s: its packages" "${want%$'\n'}"
done
crate tests/e2e taskdeck-e2e
RUNNER_OS=Linux stage device >/dev/null
log_is "device: tests/e2e is a package too when it has a Cargo.toml" "cargo test --locked -p taskdeck-device
cargo clippy --locked -p taskdeck-device --all-targets -- -D warnings
cargo test --locked -p taskdeck-e2e
cargo clippy --locked -p taskdeck-e2e --all-targets -- -D warnings"
rm -rf "$R/tests/e2e"

echo "-- hooks"
hook relay
expect "relay: the hook runs" 0 env RUNNER_OS=Linux CI_WIDE=true bash "$R/scripts/ci/stage.sh" relay
: >"$LOG"; RUNNER_OS=Linux CI_WIDE=true bash "$R/scripts/ci/stage.sh" relay >/dev/null
log_is "relay: hook variables, from the repository root" "hook relay stage=relay os=linux wide=true pwd=$R"
hook android
hook crates/taskdeck-android
: >"$LOG"; RUNNER_OS=Windows bash "$R/scripts/ci/stage.sh" android >/dev/null
log_is "android: cargo first, then hooks in directory order; CI_WIDE defaults to false" "cargo test --locked -p taskdeck-android
cargo clippy --locked -p taskdeck-android --all-targets -- -D warnings
hook android stage=android os=windows wide=false pwd=$R
hook crates/taskdeck-android stage=android os=windows wide=false pwd=$R"
: >"$LOG"; RUNNER_OS=macOS CI_WIDE=yes bash "$R/scripts/ci/stage.sh" relay >/dev/null
log_is "macOS; a CI_WIDE other than true is false" "hook relay stage=relay os=macos wide=false pwd=$R"
: >"$LOG"; (cd "$TMP" && RUNNER_OS=Linux bash "$R/scripts/ci/stage.sh" relay >/dev/null)
log_is "runs from the repository root whatever the caller's directory" "hook relay stage=relay os=linux wide=false pwd=$R"

echo "-- failures"
hook relay 3
expect "a failing hook fails the stage" 1 env RUNNER_OS=Linux bash "$R/scripts/ci/stage.sh" relay
output_has "the summary names it" "FAILED  bash relay/ci.sh"
hook relay 0
expect "a failing test fails the stage" 1 env RUNNER_OS=Linux CARGO_FAIL=test bash "$R/scripts/ci/stage.sh" android
: >"$LOG"; RUNNER_OS=Linux CARGO_FAIL=test bash "$R/scripts/ci/stage.sh" android >/dev/null
log_is "after a failure every other step still runs" "cargo test --locked -p taskdeck-android
cargo clippy --locked -p taskdeck-android --all-targets -- -D warnings
hook android stage=android os=linux wide=false pwd=$R
hook crates/taskdeck-android stage=android os=linux wide=false pwd=$R"
expect "failing clippy fails the stage" 1 env RUNNER_OS=Linux CARGO_FAIL=clippy bash "$R/scripts/ci/stage.sh" core
rm -rf "$R/crates/taskdeck-sync" && mkdir -p "$R/crates/taskdeck-sync/src"
expect "a crate directory without Cargo.toml is an error" 1 env RUNNER_OS=Linux bash "$R/scripts/ci/stage.sh" engine
output_has "says so" "crates/taskdeck-sync exists but has no Cargo.toml"
printf '[lib]\nname = "nope"\n' >"$R/crates/taskdeck-sync/Cargo.toml"
expect "a Cargo.toml without a [package] name is an error" 1 env RUNNER_OS=Linux bash "$R/scripts/ci/stage.sh" engine
mkdir -p "$R/relay"
expect "a directory with neither crate nor hook is fine" 0 env RUNNER_OS=Linux bash -c 'rm -f "$1/relay/ci.sh"; bash "$1/scripts/ci/stage.sh" relay' _ "$R"

finish
