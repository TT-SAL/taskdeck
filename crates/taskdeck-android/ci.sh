#!/usr/bin/env bash
# The hook of CI's android job (scripts/ci/stage.sh, contract point 4): the
# skeletons compile for the Android targets (SYNC_PLAN.md §6.1 gate).
#
# On Linux (CI_OS=linux) it installs what the check needs, `bash
# scripts/dev-setup.sh android-targets` (the toolchain, cargo-ndk, and the NDK
# unless the runner names a usable one), takes the exports of
# `bash scripts/dev-setup.sh --env` and runs scripts/check-android-targets.sh,
# whose status it exits with. On another system it says the check runs on Linux
# only and exits 0. Run from the repository root.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

if [[ "${CI_OS:-}" != linux ]]; then
    echo "taskdeck-android/ci.sh: the Android-target check runs on Linux only (CI_OS=${CI_OS:-unset}); nothing to do"
    exit 0
fi
bash scripts/dev-setup.sh android-targets
eval "$(bash scripts/dev-setup.sh --env)"
exec bash scripts/check-android-targets.sh
