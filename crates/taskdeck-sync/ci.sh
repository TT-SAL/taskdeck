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
# The regression files and the simulator's default 50 seeds run in the
# `cargo test -p taskdeck-sync` of stage.sh step 3. The simulator at 1,000
# seeds (the rows `simulator` and `ci-seeds` of scripts/gates/engine.gate,
# `env SIM_SEEDS=1000 cargo test --locked -p taskdeck-sync --test simulator`;
# `base` is TD_SIM_BASE when set, else the first 16 hex digits of the commit
# id; spec: ENG-SIM-004) becomes a step here once it passes; until then
# `bash scripts/gate.sh engine` runs it (the owner's decision of 2026-11-28,
# docs/sync/status/engine.md).
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
