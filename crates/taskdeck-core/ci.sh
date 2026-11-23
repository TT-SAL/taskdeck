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

# The rest runs on Linux only (CI_OS, set by scripts/ci/stage.sh; unset, as
# in the gate's run of this file, it is skipped): the fuzz targets compile,
# and the budgets.
if [[ "${CI_OS:-}" != linux ]]; then
    echo "ci.sh: the fuzz check and the budgets run on Linux only (CI_OS=${CI_OS:-unset}): skipped"
    exit 0
fi
target_dir="${CARGO_TARGET_DIR:-target}"

# The fuzz targets of core.md §18.6 compile on the pinned stable toolchain.
# They live in their own package with its own Cargo.lock (crates/taskdeck-core/fuzz);
# stage `release`'s nightly job runs them.
cargo check --locked --manifest-path crates/taskdeck-core/fuzz/Cargo.toml --target-dir "$target_dir/fuzz"

# The budgets (core.md §17, CORE-PERF-001 to -003; SYNC_PLAN.md §3.3). The
# crate reads no clock, so the shell times the examples: each reads `n` from
# standard input, does its setup once (importing the 10,000-row fixture) and
# its workload `n` times. A run's time per workload is (T(n) - T(0)) / n, from
# `date +%s%N`; five runs; the median is printed on a line starting `budget:`
# and must be within the limit, and within 25% of the recorded baseline once
# there is one.
#
# Baselines in ms per workload, from the first CI run of this job (the lead
# records them here). Empty: no baseline yet.
BASELINE_SNAPSHOT_MS=""
BASELINE_APPLY_MS=""
BASELINE_CHECKPOINT_MS=""

cargo build --locked --release -p taskdeck-core \
    --example budget_snapshot --example budget_apply --example budget_checkpoint
budget_failed=0

# budget <name> <example> <n> <limit ms> <baseline ms or empty>
budget() {
    local name="$1" bin="$target_dir/release/examples/$2" n="$3" limit="$4" baseline="$5"
    local i t0 t1 t2 runs=() median ms
    for i in 1 2 3 4 5; do
        t0="$(date +%s%N)"
        echo "$n" | "$bin" >/dev/null
        t1="$(date +%s%N)"
        echo 0 | "$bin" >/dev/null
        t2="$(date +%s%N)"
        runs+=("$(( ((t1 - t0) - (t2 - t1)) / n ))")
    done
    median="$(printf '%s\n' "${runs[@]}" | sort -n | sed -n 3p)"
    ms="$(awk -v ns="$median" 'BEGIN { printf "%.3f", ns / 1e6 }')"
    echo "budget: $name $ms ms (median of five, n = $n; limit $limit ms; baseline ${baseline:-none}; runs in ns: ${runs[*]})"
    if awk -v ms="$ms" -v limit="$limit" 'BEGIN { exit !(ms > limit) }'; then
        echo "budget: FAILED: $name takes $ms ms, over its limit of $limit ms"
        budget_failed=1
    fi
    if [[ -n "$baseline" ]] && awk -v ms="$ms" -v b="$baseline" 'BEGIN { exit !(ms > b * 1.25) }'; then
        echo "budget: FAILED: $name takes $ms ms, more than 25% over its baseline of $baseline ms"
        budget_failed=1
    fi
}

budget snapshot budget_snapshot 500 40 "$BASELINE_SNAPSHOT_MS"       # CORE-PERF-001: 31-day snapshot
budget apply budget_apply 10 1000 "$BASELINE_APPLY_MS"               # CORE-PERF-002: 5,000 commands
budget checkpoint budget_checkpoint 20 150 "$BASELINE_CHECKPOINT_MS" # CORE-PERF-003: canonical bytes loaded
exit "$budget_failed"
