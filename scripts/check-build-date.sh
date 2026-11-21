#!/usr/bin/env bash
# Proves build.rs takes BUILD_DATE from SOURCE_DATE_EPOCH when it is set
# (https://reproducible-builds.org/specs/source-date-epoch/), refuses a set
# but unusable value, and uses today's UTC date when it is unset.
#
# Uses `cargo check` on the headless library, the cheapest build that runs the
# build script. The date is read from cargo's JSON messages (the
# build-script-executed message lists the environment the script set), which
# cargo prints even when the script did not need to run again.
#
# Runs from any directory, on Linux, macOS and Git-Bash on Windows. Honours
# CARGO_TARGET_DIR and cargo's own target-dir configuration.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

failures=0
fail() {
    echo "check-build-date: FAIL: $*" >&2
    failures=$((failures + 1))
}

check() {
    cargo check --locked --no-default-features --lib --message-format=json "$@"
}

# Prints the BUILD_DATE the root package's build script emitted.
build_date() {
    grep '"reason":"build-script-executed"' \
        | grep '"package_id":"[^"]*#task_deck@' \
        | sed -n 's/.*\["BUILD_DATE","\([^"]*\)"\].*/\1/p' \
        | tail -n 1
}

# expect_refused VALUE: the build must fail, and say why.
expect_refused() {
    local value="$1" log
    echo "check-build-date: SOURCE_DATE_EPOCH=\"$value\" must fail the build"
    if log="$(SOURCE_DATE_EPOCH="$value" check 2>&1)"; then
        fail "SOURCE_DATE_EPOCH=\"$value\" built; it must be refused"
    elif ! printf '%s\n' "$log" | grep -q 'SOURCE_DATE_EPOCH is set but unusable'; then
        printf '%s\n' "$log" | tail -n 20 >&2
        fail "SOURCE_DATE_EPOCH=\"$value\" failed, but not with build.rs's message"
    else
        echo "check-build-date:   refused as expected"
    fi
}

expect_refused "abc"
expect_refused ""
expect_refused "99999999999999999"   # fits in i64, far past chrono's last year

echo "check-build-date: SOURCE_DATE_EPOCH=1700000000 must give 2023-11-14"
if out="$(SOURCE_DATE_EPOCH=1700000000 check)"; then
    got="$(printf '%s\n' "$out" | build_date)"
    if [ "$got" = "2023-11-14" ]; then
        echo "check-build-date:   BUILD_DATE=$got"
    else
        fail "SOURCE_DATE_EPOCH=1700000000 gave BUILD_DATE=\"$got\", expected 2023-11-14"
    fi
else
    fail "the build with SOURCE_DATE_EPOCH=1700000000 failed"
fi

echo "check-build-date: SOURCE_DATE_EPOCH unset must give today's UTC date"
before="$(date -u +%Y-%m-%d)"
if out="$(unset SOURCE_DATE_EPOCH; check)"; then
    got="$(printf '%s\n' "$out" | build_date)"
    # The script just ran again (the variable changed since the last build),
    # so the date is fresh. Either side of a midnight UTC during the build is
    # accepted.
    after="$(date -u +%Y-%m-%d)"
    if [ "$got" = "$before" ] || [ "$got" = "$after" ]; then
        echo "check-build-date:   BUILD_DATE=$got"
    else
        fail "with SOURCE_DATE_EPOCH unset BUILD_DATE=\"$got\", expected $after"
    fi
else
    fail "the build with SOURCE_DATE_EPOCH unset failed"
fi

if [ "$failures" -ne 0 ]; then
    echo "check-build-date: $failures check(s) failed" >&2
    exit 1
fi
echo "check-build-date: OK"
