#!/usr/bin/env bash
# scripts/check-build-date.sh must give the same verdict under CI's
# CARGO_TERM_COLOR=always: its rerun check reads cargo's human-readable
# output, which colour codes broke once. Builds the headless library (a warm
# target dir makes it quick); honours CARGO_TARGET_DIR.
set -uo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

expect "check-build-date passes with CARGO_TERM_COLOR=always" 0 \
    env CARGO_TERM_COLOR=always bash "$SCRIPTS_DIR/check-build-date.sh"
output_has "its rerun checks saw build.rs run again" "build.rs ran again"
output_lacks "no rerun check failed" "did not run again"

finish
