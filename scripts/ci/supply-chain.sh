#!/usr/bin/env bash
# The `supply-chain` job (.github/workflows/ci.yml; scripts/ci/README.md):
# works out the cooldown's base from the event and runs scripts/supply-chain.sh
# once.
#
#   bash scripts/ci/supply-chain.sh
#
# Environment (set by the job through `env:`):
#   CI_EVENT    push or pull_request
#   CI_PR_BASE  pull_request: the pull request's base commit
#   CI_BEFORE   push: the commit before the push
#
# The CI design wants the cooldown measured from the pull request's base or the
# push's `before`. When that commit is a commit id that is in the clone, this
# script runs `scripts/supply-chain.sh --base <that commit>`, which passes it
# to scripts/cooldown.sh. An all-zero, empty or unknown commit (or anything but
# a commit id) gives no usable base: the script then runs
# `scripts/supply-chain.sh` without it, and the cooldown uses its default base,
# the merge base of HEAD and origin/day-planner (the job checks out with full
# history so that ref exists). Exits with scripts/supply-chain.sh's status.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
cd "$root" || exit 2

say() { echo "ci/supply-chain: $*"; }

base=""
case "${CI_EVENT:-}" in
    pull_request) base="${CI_PR_BASE:-}" ;;
    push) base="${CI_BEFORE:-}" ;;
esac
if [[ -z "$base" || "$base" =~ ^0+$ ]]; then
    say "the event names no base commit (${base:-empty}); the cooldown uses its default base"
    base=""
elif [[ ! "$base" =~ ^[0-9a-f]{40}([0-9a-f]{24})?$ ]] \
    || ! git rev-parse --verify --quiet "${base}^{commit}" >/dev/null; then
    say "the base commit $base is not in the clone; the cooldown uses its default base"
    base=""
fi

if [[ -n "$base" ]]; then
    base="$(git rev-parse "${base}^{commit}")"
    say "the cooldown's base is the event's base ${base:0:12}"
    exec bash "$root/scripts/supply-chain.sh" --base "$base"
fi
exec bash "$root/scripts/supply-chain.sh"
