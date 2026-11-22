#!/usr/bin/env bash
# Spec coverage (SYNC_PLAN.md §3.3; docs/sync/spec/README.md §4.3): every live
# clause a stage owns must be cited by at least one of that stage's test files.
#
#   bash scripts/spec-coverage.sh <stage>            uncovered clauses, one per line
#   bash scripts/spec-coverage.sh --list <stage>     every clause of the stage and the files citing it
#   bash scripts/spec-coverage.sh --paths [<stage>]  the test-path table below
#   options: --root <dir> (the tree to read, default this repository)
#
# Exit 0 when every clause is covered and every citation is real, 1 otherwise,
# 2 on a usage error. A stage that owns no clause (foundation) passes.
# rc owns no clause but runs every other stage's check, each on that stage's
# test files, then its own on every path (the stricter reading of §3.3 for a
# release candidate: no clause may have lost its tests since its stage exited).
#
# A test file is a source file (.rs .js .mjs .cjs .ts .kt .kts .java .html,
# and .sh .py for the shell and Python tests of release) that matches the
# stage's patterns below; a data file, README or note under a test directory
# cites nothing.
#
# A clause is covered when its id stands, as a whole token, anywhere in a test
# file of the stage (SYNC_PLAN.md §3.3: "fails if a clause owned by the stage
# appears in no test"): in the marker `spec: <ID>` (`// spec: CORE-APPLY-012`),
# in a test's name, or elsewhere. The script does not judge whether the test
# around it runs: a skipped test is the no-skipped-tests scan's business, a
# commented-out one is review's (lead ruling 9 of the tooling task).
# Ids are compared as whole tokens: `CORE-APPLY-0012` does not cite
# CORE-APPLY-001. Every token shaped like an id (a spec prefix and two more
# hyphen-separated parts, or whatever follows `spec:`) must name a live
# clause: one that is no clause, or a withdrawn clause, is an error, since a
# test that cites nothing real proves nothing.
#
# There is no option that makes a stage pass without tests.
set -uo pipefail
# The patterns are expanded as words, never as globs of the shell.
set -f

# ---------------------------------------------------------------------------
# Test files per stage: the source of truth (README §4.3). Globs relative to
# the repository root; `**` is any depth, `*` stays within one name, a pattern
# starting with `!` removes files. Build output is never searched: directories
# named target, node_modules, .git or .gradle, and a build directory next to a
# Gradle build or settings file (speclib.is_output_dir).
# TOOL_TESTS are foundation's tests of this script and of spec-lint, whose
# made-up specs and test files cite made-up ids: they are no stage's tests.
# ---------------------------------------------------------------------------
TOOL_TESTS='!scripts/tests/test-spec-*.sh !scripts/tests/spec-fixture.sh'
rust_tests() {  # the Rust pattern under a crate directory
    echo "$1/tests/** $1/fuzz/fuzz_targets/** $1/src/**/tests.rs $1/src/**/*_tests.rs"
}
declare -A STAGE_TEST_PATHS=(
    [foundation]=""
    [core]="$(rust_tests crates/taskdeck-core)"
    [seal]="$(rust_tests crates/taskdeck-seal)"
    [engine]="$(rust_tests crates/taskdeck-sync)"
    [device]="$(rust_tests crates/taskdeck-device) tests/e2e/**"
    [recovery]="$(rust_tests crates/taskdeck-backup) $(rust_tests crates/taskdeck-recover)"
    [relay]="relay/test/**"
    [android-shell]="android/**/src/test/** android/**/src/androidTest/** android/page/test/** $(rust_tests crates/taskdeck-android)"
    [android]="android/**/src/test/** android/**/src/androidTest/** android/page/test/** $(rust_tests crates/taskdeck-android)"
    [desktop]="src/** tests/** !tests/e2e/**"
    [release]="scripts/release/** scripts/tests/** $TOOL_TESTS"
)
# rc: every path above (the desktop exclusion does not carry over: tests/e2e is
# device's), without TOOL_TESTS.
STAGE_TEST_PATHS[rc]="$(for s in core seal engine device recovery relay android-shell desktop release; do
    for p in ${STAGE_TEST_PATHS[$s]}; do [[ "$p" == '!'* ]] || echo "$p"; done
done | sort -u | tr '\n' ' ')$TOOL_TESTS"
STAGE_ORDER="foundation core seal relay engine android-shell device recovery desktop android release rc"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
usage() { sed -n '5,9p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }

mode=check stage="" root="$(dirname "$here")"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --list) mode=list ;;
        --paths) mode=paths ;;
        --root) [[ $# -ge 2 ]] || usage; root="$2"; shift ;;
        -h|--help) usage ;;
        -*) echo "spec-coverage: unknown option $1" >&2; usage ;;
        *) [[ -z "$stage" ]] || usage; stage="$1" ;;
    esac
    shift
done

if [[ -n "$stage" && -z "${STAGE_TEST_PATHS[$stage]+x}" ]]; then
    echo "spec-coverage: unknown stage '$stage' (stages: $STAGE_ORDER)" >&2
    exit 2
fi
if [[ "$mode" == paths ]]; then
    for s in $STAGE_ORDER; do
        [[ -z "$stage" || "$s" == "$stage" ]] || continue
        printf '%-14s %s\n' "$s" "${STAGE_TEST_PATHS[$s]:-(none: the stage has no test files)}"
    done
    echo "(source files only: .rs .js .mjs .cjs .ts .kt .kts .java .html .sh .py)"
    [[ -n "$stage" && "$stage" != rc ]] || echo "(rc first runs every other stage's check, each on that stage's row)"
    exit 0
fi
[[ -n "$stage" ]] || usage
command -v python3 >/dev/null || { echo "spec-coverage: error: python3 is needed" >&2; exit 2; }
check() {  # check <stage>
    # shellcheck disable=SC2086 # the patterns are words on purpose
    python3 -I -B "$here/spec-coverage.py" "$mode" "$1" "$root" ${STAGE_TEST_PATHS[$1]}
}
[[ "$stage" == rc ]] || { check "$stage"; exit; }
status=0
for s in $STAGE_ORDER; do
    check "$s"
    r=$?
    (( r > status )) && status=$r
done
exit "$status"
