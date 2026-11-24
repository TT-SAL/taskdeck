#!/usr/bin/env bash
# Stage engine's own CI checks for taskdeck-sync (the hook of point 4 of the
# contract at the top of scripts/ci/stage.sh). Run from the repository root,
# by stage.sh with CI_STAGE, CI_OS and CI_WIDE set, or by hand. Honours
# CARGO_TARGET_DIR. Every step runs even after one fails; exits 1 if any did.
#
#   1. the crate compiles for wasm32-unknown-unknown with its default features
#      and with the feature `sim`: proof that it does no I/O (SYNC_PLAN.md
#      §6.5); spec: ENG-GEN-002
#
# The simulator's 1,000 seeds and the regression files (the gate of
# scripts/gates/engine.gate) are added here when the simulator lands.
set -uo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)" || exit 2
cd "$root" || exit 2

failed=0
step() {
    echo
    echo "==== taskdeck-sync ci.sh: $1"
    shift
    if ! "$@"; then
        failed=1
    fi
}

step "wasm32, default features" cargo check --locked -p taskdeck-sync --target wasm32-unknown-unknown
step "wasm32, feature sim" cargo check --locked -p taskdeck-sync --features sim --target wasm32-unknown-unknown

exit "$failed"
