#!/usr/bin/env bash
# Stage seal's own CI checks for taskdeck-seal (the hook of point 4 of the
# contract at the top of scripts/ci/stage.sh). Run from the repository root,
# by stage.sh with CI_STAGE, CI_OS and CI_WIDE set, or by hand. Honours
# CARGO_TARGET_DIR. Every step runs even after one fails; exits 1 if any did.
#
#   1. the suite with the feature `test-entropy` (FixedEntropy, the vectors);
#   2. clippy over every target with the feature;
#   3. on Linux only: the three Android targets build, and the library
#      built with `test-entropy` contains TEST_ENTROPY_MARKER while the one
#      built without it does not (searching the built .rlib);
#      spec: SEAL-GEN-014
#   4. on Linux only (the check needs python3, which a Windows runner's
#      bash may lack; the files are the same on every system): the format of
#      the published vectors (README §7);
#   5. on Linux only: the fuzz targets compile on the pinned toolchain
#      (`fuzz/`, a crate of its own with its own lockfile; stage `release`
#      runs them, nightly);
#   6. on Linux only: the budget job (SYNC_PLAN.md §3.3, §6.3): 256 entries
#      verified and opened within 100 ms, the median of five runs, in a
#      release build, and within 25% of `budget-baseline.txt`
#      (`examples/budget.rs`).
set -uo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)" || exit 2
cd "$root" || exit 2

os="${CI_OS:-}"
if [[ -z "$os" ]]; then
    case "$(uname -s)" in
        Linux) os=linux ;;
        Darwin) os=macos ;;
        MINGW* | MSYS* | CYGWIN*) os=windows ;;
        *) os=unknown ;;
    esac
fi

failed=()
step() {
    local name="$1"
    shift
    echo "==> seal ci: $name"
    if "$@"; then
        echo "==> seal ci: $name: ok"
    else
        echo "==> seal ci: $name: FAILED"
        failed+=("$name")
    fi
}

# Whether the .rlib cargo reports for the library built with the given
# flags contains the marker: 0 yes, 1 no, 2 no library found. Each build is
# searched before the next, since cargo copies both to the same path.
rlib_has_marker() {
    local marker="taskdeck-test-entropy-enabled-3b9e5d" path
    path="$(cargo build --locked -p taskdeck-seal --lib --message-format=json "$@" 2>/dev/null |
        grep -o '"[^"]*libtaskdeck_seal[^"]*\.rlib"' | tail -n 1 | tr -d '"')"
    if [[ -z "$path" || ! -f "$path" ]]; then
        echo "marker: no library reported for: cargo build $*"
        return 2
    fi
    echo "marker: searching $path (flags: ${*:-none})"
    grep -qaF "$marker" "$path"
}

marker_check() {
    local with without
    rlib_has_marker --features test-entropy
    with=$?
    rlib_has_marker
    without=$?
    if [[ $with -ne 0 ]]; then
        echo "marker: the build with test-entropy does not contain it"
        return 1
    fi
    if [[ $without -ne 1 ]]; then
        echo "marker: the build without test-entropy contains it, or was not found"
        return 1
    fi
    echo "marker: present with test-entropy, absent without"
}

step "test (test-entropy)" cargo test --locked -p taskdeck-seal --features test-entropy
step "clippy (test-entropy)" cargo clippy --locked -p taskdeck-seal --all-targets --features test-entropy -- -D warnings
if [[ "$os" == linux ]]; then
    step "android targets" cargo check --locked -p taskdeck-seal \
        --target aarch64-linux-android --target armv7-linux-androideabi --target x86_64-linux-android
    step "test-entropy marker (SEAL-GEN-014)" marker_check
    step "vectors format" bash scripts/gate-lib.sh vectors docs/sync/vectors
    step "fuzz targets build" cargo check --locked --manifest-path crates/taskdeck-seal/fuzz/Cargo.toml
    step "budget" cargo run --release --locked -p taskdeck-seal --example budget
else
    echo "==> seal ci: android targets, marker, vectors format, fuzz targets and budget: Linux only, skipped on $os"
fi

if [[ ${#failed[@]} -gt 0 ]]; then
    echo "seal ci: ${#failed[@]} step(s) failed: ${failed[*]}"
    exit 1
fi
echo "seal ci: all steps passed"
