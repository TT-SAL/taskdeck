#!/usr/bin/env bash
# Tests for scripts/ci/supply-chain.sh, on a throwaway repository with fake
# scripts/supply-chain.sh and scripts/cooldown.sh that log how they were run:
# which base the event names, when the extra cooldown run happens, and that
# both runs happen and either failing fails the job.
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
printf 'echo "cooldown $*" >>"$FAKE_LOG"\nexit "${CD_EXIT:-0}"\n' >"$R/scripts/cooldown.sh"
g() { git -C "$R" -c user.name=test -c user.email= -c commit.gpgsign=false "$@"; }
git init -q -b day-planner "$R"
g add -A && g commit -qm one
A="$(g rev-parse HEAD)"
echo x >"$R/Cargo.lock"
g add -A && g commit -qm two
B="$(g rev-parse HEAD)"

run() { : >"$LOG"; (cd "$R" && env "$@" bash scripts/ci/supply-chain.sh); }
log_is() {
    if [[ "$(cat "$LOG")" == "$2" ]]; then
        PASSES=$((PASSES + 1)); echo "ok    $1"
    else
        FAILS=$((FAILS + 1)); echo "FAIL  $1"; sed 's/^/        got: /' "$LOG"; sed 's/^/        wanted: /' <<<"$2"
    fi
}

echo "-- a push to day-planner: the default base is HEAD, so the event's base runs too"
g update-ref refs/remotes/origin/day-planner "$B"
expect "push to day-planner passes" 0 run CI_EVENT=push CI_BEFORE="$A"
log_is "push to day-planner: supply-chain, then cooldown --base before" "supply-chain
cooldown --base $A"

echo "-- a branch push or a pull request whose base is the default base: one run"
g update-ref refs/remotes/origin/day-planner "$A"
expect "branch push" 0 run CI_EVENT=push CI_BEFORE="$A"
log_is "branch push: supply-chain only" "supply-chain"
output_has "says the default base covered it" "is the default base"
expect "pull request" 0 run CI_EVENT=pull_request CI_PR_BASE="$A"
log_is "pull request: supply-chain only" "supply-chain"
g update-ref refs/remotes/origin/day-planner "$B"
expect "pull request whose base differs from the merge base" 0 run CI_EVENT=pull_request CI_PR_BASE="$A"
log_is "pull request: the event's base runs too" "supply-chain
cooldown --base $A"

echo "-- no usable base: the default base only"
for b in "" 0000000000000000000000000000000000000000 1234567890abcdef1234567890abcdef12345678 \
    day-planner HEAD~1 '--all'; do
    expect "push with before '$b'" 0 run CI_EVENT=push CI_BEFORE="$b"
    log_is "push with before '$b': supply-chain only" "supply-chain"
done
expect "an unknown event" 0 run CI_EVENT=schedule CI_BEFORE="$A"
log_is "an unknown event: supply-chain only" "supply-chain"
expect "a pull request without a base" 0 run CI_EVENT=pull_request CI_BEFORE="$A"
log_is "a pull request ignores before" "supply-chain"

echo "-- failures"
expect "supply-chain.sh failing fails the job" 1 run CI_EVENT=push CI_BEFORE="$A" SC_EXIT=1
log_is "and the extra cooldown still ran" "supply-chain
cooldown --base $A"
expect "the extra cooldown failing fails the job" 1 run CI_EVENT=push CI_BEFORE="$A" CD_EXIT=1
output_has "says which" "FAILED"

finish
