#!/usr/bin/env bash
# The hook of CI's core job (scripts/ci/stage.sh, contract point 4): the
# library built alone, without the dev-dependencies, on the host and for
# wasm32-unknown-unknown (SYNC_PLAN.md §6.2 purity).
#
# stage.sh's `cargo test` and `cargo clippy --all-targets` build the library
# with the dev-dependencies' features, and the tests' chrono has `clock`, so
# there `chrono::Local` exists and clippy does not flag it used as a value
# (`Local.offset_from_utc_datetime(..)`). These builds have no `clock`: any use
# of `Local` or of chrono's `now` in the library fails to compile here
# (CORE-GEN-003, CORE-GEN-004). The core stage adds its own checks below.
# Run from the repository root.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

cargo check --locked -p taskdeck-core --lib
cargo check --locked -p taskdeck-core --lib --target wasm32-unknown-unknown
