#!/usr/bin/env bash
# The `supply-chain` job (.github/workflows/ci.yml; scripts/ci/README.md):
# runs scripts/supply-chain.sh, and the seven-day cooldown against the base the
# event names.
#
#   bash scripts/ci/supply-chain.sh
#
# Environment (set by the job through `env:`):
#   CI_EVENT    push or pull_request
#   CI_PR_BASE  pull_request: the pull request's base commit
#   CI_BEFORE   push: the commit before the push
#
# scripts/supply-chain.sh takes no options: its cooldown always compares with
# its default base, the merge base of HEAD and origin/day-planner (the job
# checks out with full history so that ref exists). The CI design wants the
# cooldown measured from the pull request's base or the push's `before`. When
# that commit is known, is in the clone, and differs from the default base
# (a push to day-planner itself, whose merge base with origin/day-planner is
# HEAD), this script runs `scripts/cooldown.sh --base <that commit>` as well.
# An all-zero, empty or unknown commit (or anything but a commit id) leaves
# the default base alone.
# Exits non-zero if either run failed, after running both.
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
    say "the event names no base commit (${base:-empty}); the cooldown uses its default base only"
    base=""
elif [[ ! "$base" =~ ^[0-9a-f]{40}([0-9a-f]{24})?$ ]] \
    || ! git rev-parse --verify --quiet "${base}^{commit}" >/dev/null; then
    say "the base commit $base is not in the clone; the cooldown uses its default base only"
    base=""
fi

status=0
bash "$root/scripts/supply-chain.sh" || status=1

if [[ -n "$base" ]]; then
    base="$(git rev-parse "${base}^{commit}")"
    default="$(git merge-base HEAD origin/day-planner 2>/dev/null || true)"
    if [[ "$base" == "$default" ]]; then
        say "the event's base ${base:0:12} is the default base; the cooldown above covered it"
    else
        printf '\n==== ci/supply-chain: cooldown against the event base %s ====\n' "${base:0:12}"
        if bash "$root/scripts/cooldown.sh" --base "$base"; then
            say "cooldown against ${base:0:12}: passed"
        else
            say "cooldown against ${base:0:12}: FAILED"
            status=1
        fi
    fi
fi
exit $status
