#!/usr/bin/env bash
# Tests for scripts/ci/supply-chain.sh, on a throwaway repository with a fake
# scripts/supply-chain.sh that logs how it was run: which base the event
# names, that the script is called exactly once (with --base <commit> when the
# event gives a usable base commit, without it otherwise), and that its exit
# status is the job's.
set -uo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
unset CI_EVENT CI_PR_BASE CI_BEFORE

R="$TMP/repo"
LOG="$TMP/log"
export FAKE_LOG="$LOG"
mkdir -p "$R/scripts/ci"
cp "$SCRIPTS_DIR/ci/supply-chain.sh" "$R/scripts/ci/supply-chain.sh"
printf 'echo "supply-chain${*:+ $*}" >>"$FAKE_LOG"\nexit "${SC_EXIT:-0}"\n' >"$R/scripts/supply-chain.sh"
printf 'echo "cooldown $*" >>"$FAKE_LOG"\nexit 0\n' >"$R/scripts/cooldown.sh"
g() { git -C "$R" -c user.name=test -c user.email= -c commit.gpgsign=false "$@"; }
git init -q -b day-planner "$R"
g add -A && g commit -qm one
A="$(g rev-parse HEAD)"
echo x >"$R/Cargo.lock"
g add -A && g commit -qm two
B="$(g rev-parse HEAD)"
g update-ref refs/remotes/origin/day-planner "$B"

run() { : >"$LOG"; (cd "$R" && env "$@" bash scripts/ci/supply-chain.sh); }
log_is() {
    if [[ "$(cat "$LOG")" == "$2" ]]; then
        PASSES=$((PASSES + 1)); echo "ok    $1"
    else
        FAILS=$((FAILS + 1)); echo "FAIL  $1"; sed 's/^/        got: /' "$LOG"; sed 's/^/        wanted: /' <<<"$2"
    fi
}

echo "-- a usable base: one call, with --base"
expect "push" 0 run CI_EVENT=push CI_BEFORE="$A"
log_is "push: supply-chain --base before, once" "supply-chain --base $A"
output_has "says which base" "the event's base ${A:0:12}"
expect "push whose before is the default base" 0 run CI_EVENT=push CI_BEFORE="$B"
log_is "push: still one call, with --base" "supply-chain --base $B"
expect "pull request" 0 run CI_EVENT=pull_request CI_PR_BASE="$A"
log_is "pull request: supply-chain --base the pull request's base, once" "supply-chain --base $A"

echo "-- no usable base: one call, without --base"
for b in "" 0000000000000000000000000000000000000000 1234567890abcdef1234567890abcdef12345678 \
    day-planner HEAD~1 '--all'; do
    expect "push with before '$b'" 0 run CI_EVENT=push CI_BEFORE="$b"
    log_is "push with before '$b': supply-chain without --base" "supply-chain"
done
output_has "says the default base is used" "uses its default base"
expect "an unknown event" 0 run CI_EVENT=schedule CI_BEFORE="$A"
log_is "an unknown event: supply-chain without --base" "supply-chain"
expect "a pull request without a base" 0 run CI_EVENT=pull_request CI_BEFORE="$A"
log_is "a pull request ignores before" "supply-chain"

echo "-- the job's status is supply-chain.sh's"
expect "supply-chain.sh failing fails the job" 1 run CI_EVENT=push CI_BEFORE="$A" SC_EXIT=1
log_is "and it ran once" "supply-chain --base $A"
expect "supply-chain.sh failing without a base fails the job" 1 run CI_EVENT=push CI_BEFORE= SC_EXIT=1
log_is "and it ran once, without --base" "supply-chain"

finish
