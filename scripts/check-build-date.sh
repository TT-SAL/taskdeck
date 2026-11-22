#!/usr/bin/env bash
# Proves build.rs takes BUILD_DATE from SOURCE_DATE_EPOCH when it is set
# (https://reproducible-builds.org/specs/source-date-epoch/), refuses a set
# but unusable value, uses today's UTC date when it is unset, and runs again
# when a file under src/ changes (so that date does not go stale).
#
# Uses `cargo check` on the headless library, the cheapest build that runs the
# build script. The date is read from cargo's JSON messages (the
# build-script-executed message lists the environment the script set), which
# cargo prints even when the script did not need to run again.
#
# Runs from any directory, on Linux, macOS and Git-Bash on Windows. Honours
# CARGO_TARGET_DIR (a relative value is taken from the caller's directory, as
# cargo would) and cargo's own target-dir configuration.
set -euo pipefail
# The rerun check below greps cargo's human-readable output, which colour
# would break (CI sets CARGO_TERM_COLOR=always): colour off for every call.
export CARGO_TERM_COLOR=never

# Make a relative target dir from the environment absolute against the
# caller's directory before moving to the repository root. `pwd -W` gives the
# Windows form under Git-Bash, which native cargo understands.
caller="$(pwd -W 2>/dev/null || pwd)"
for var in CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR; do
    value="${!var-}"
    case "$value" in
        "" | /* | [A-Za-z]:[\\/]* | \\\\*) ;;
        *) export "$var=$caller/$value" ;;
    esac
done

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

failures=0
fail() {
    echo "check-build-date: FAIL: $*" >&2
    failures=$((failures + 1))
}

# For the host, as the build script always runs there: a CARGO_BUILD_TARGET
# or build.target the libraries cannot build for would fail every case for a
# reason unrelated to the date. TZ is a far-from-UTC POSIX zone (no tzdata
# needed), so a build.rs that used local time instead of UTC gives a wrong
# date here even on a UTC machine. chrono ignores TZ on Windows; the Linux
# and macOS runs catch it.
host="$(rustc -vV | sed -n 's/^host: //p')"
[ -n "$host" ] || { echo "check-build-date: could not read the host from rustc -vV" >&2; exit 1; }
tz=XXX-14   # UTC+14: 1700000000 (22:13 UTC) is already the next day there
check() {
    TZ="$tz" cargo check --locked --no-default-features --lib --target "$host" \
        --message-format=json "$@"
}

# Prints the BUILD_DATE the root package's build script emitted, or nothing.
# Cargo writes the root package's id as `path+file:///.../dir#task_deck@1.0.0`,
# or as `path+file:///.../task_deck#1.0.0` when the directory is named after
# the package. Never fails, so a missing date reaches the caller's fail().
build_date() {
    { grep '"reason":"build-script-executed"' \
        | grep -E '"package_id":"[^"]*(#task_deck@|/task_deck#)' \
        | sed -n 's/.*\["BUILD_DATE","\([^"]*\)"\].*/\1/p' \
        | tail -n 1; } || true
}

# build_date's own check, on made-up messages: both package-id forms are
# found, another package's date is not, and no match prints nothing.
self_test() {
    local msg='{"reason":"build-script-executed","package_id":"%s","env":[["BUILD_DATE","%s"]]}\n'
    local got
    got="$(printf "$msg" 'path+file:///x/checkout#task_deck@1.0.0' 2001-01-01 | build_date)"
    [ "$got" = 2001-01-01 ] || fail "build_date missed the name@version package id (got \"$got\")"
    got="$(printf "$msg" 'path+file:///x/task_deck#1.0.0' 2002-02-02 | build_date)"
    [ "$got" = 2002-02-02 ] || fail "build_date missed the bare-version package id (got \"$got\")"
    got="$(printf "$msg" 'path+file:///x/task_deck/crates/panic-probe#0.0.0' 2003-03-03 | build_date)"
    [ -z "$got" ] || fail "build_date took another package's date (got \"$got\")"
    got="$(printf 'no messages\n' | build_date)"
    [ -z "$got" ] || fail "build_date printed \"$got\" for no messages"
}

# expect_refused VALUE: the build must fail, and say why.
# The log is searched through a here-string, not a pipe: with `pipefail`, a
# `printf | grep -q` fails by SIGPIPE when grep stops reading a long log early.
expect_refused() {
    local value="$1" log
    echo "check-build-date: SOURCE_DATE_EPOCH=\"$value\" must fail the build"
    if log="$(SOURCE_DATE_EPOCH="$value" check 2>&1)"; then
        fail "SOURCE_DATE_EPOCH=\"$value\" built; it must be refused"
    elif ! grep -q 'SOURCE_DATE_EPOCH is set but unusable' <<<"$log"; then
        printf '%s\n' "$log" | tail -n 20 >&2
        fail "SOURCE_DATE_EPOCH=\"$value\" failed, but not with build.rs's message"
    else
        echo "check-build-date:   refused as expected"
    fi
}

echo "check-build-date: build_date finds the root package's date"
self_test

# Cargo names the build-script unit by the package's path inside the
# workspace and judges freshness by mtime, so in a target dir shared with
# another checkout it may reuse that checkout's compiled build.rs. Removing
# this package's own units (dependencies stay) makes every case below test
# this tree's build.rs.
cargo clean --locked -p task_deck --target "$host"

expect_refused "abc"
expect_refused ""
expect_refused "99999999999999999"   # fits in i64, far past chrono's last year
# Outside i64, either side; parsed wider and cast, each wraps to 1700000000.
expect_refused "18446744075409551616"    # 2^64 + 1700000000
expect_refused "-18446744072009551616"   # -2^64 + 1700000000
expect_refused "253402300800"        # 10000-01-01, not a four-digit year
expect_refused "-62167219201"        # -0001-12-31, not a four-digit year
expect_refused "+1700000000"         # digits and an optional '-' only
# Whitespace is not trimmed, at either end: each of space, tab, LF and CR.
for ws in ' ' $'\t' $'\n' $'\r'; do
    expect_refused "${ws}1700000000"
    expect_refused "1700000000${ws}"
done
expect_refused "1700000000.5"        # not a float
# Not UTF-8: must be refused, not taken as unset nor read with the bad byte
# dropped (digits around it, so either mistake builds). Windows keeps the
# environment as UTF-16, so the byte may not survive there.
case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) ;;
    *) expect_refused $'1700000000\xff' ;;
esac

# expect_date VALUE DATE: the build must succeed with BUILD_DATE=DATE.
expect_date() {
    local value="$1" want="$2" out got
    echo "check-build-date: SOURCE_DATE_EPOCH=$value must give $want"
    if out="$(SOURCE_DATE_EPOCH="$value" check)"; then
        got="$(printf '%s\n' "$out" | build_date)"
        if [ "$got" = "$want" ]; then
            echo "check-build-date:   BUILD_DATE=$got"
        else
            fail "SOURCE_DATE_EPOCH=$value gave BUILD_DATE=\"$got\", expected $want"
        fi
    else
        fail "the build with SOURCE_DATE_EPOCH=$value failed"
    fi
}

expect_date 1700000000 2023-11-14
expect_date 4102444800 2100-01-01     # past 2038: does not fit in an i32
expect_date 253402300799 9999-12-31   # the last second accepted
expect_date -1 1969-12-31             # before 1970

echo "check-build-date: SOURCE_DATE_EPOCH unset must give today's UTC date"
# Pick the zone whose date differs from UTC's at this hour: UTC-12 before
# 12:00 UTC, UTC+14 from 10:00 UTC. One zone at every hour would miss a
# local-time fallback for part of the day. Only one run: TZ is not a rerun
# trigger, so a second run would reuse this one's output.
read -r before hour <<<"$(date -u '+%Y-%m-%d %H')"
if [ "${hour#0}" -lt 12 ]; then tz=XXX+12; else tz=XXX-14; fi
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

# build.rs lists the inputs that run it again; if one dropped out of that
# list, or `src` was narrowed to some of its files, a change to it would keep
# a stale BUILD_DATE. Each listed file is touched in turn, then a new file is
# made under src/ (only the directory entry covers it), and each change must
# run the script again (cargo -v prints the run on stderr). The new file is
# not a module, so the build ignores it otherwise. It comes last: removing it
# changes src/, which would run the script again for any later change.
# expect_rerun WHAT: the next build must run build.rs again.
expect_rerun() {
    local what="$1" log
    echo "check-build-date: $what must run build.rs again"
    if log="$(unset SOURCE_DATE_EPOCH; check -v 2>&1 >/dev/null)"; then
        if grep -qE 'Running `[^`]*task_deck-[0-9a-f]+[/\\]build-script-build(\.exe)?`' <<<"$log"; then
            echo "check-build-date:   build.rs ran again"
        else
            fail "build.rs did not run again after: $what (is it in its rerun-if-changed list?)"
        fi
    else
        printf '%s\n' "$log" | tail -n 20 >&2
        fail "the build after this change failed: $what"
    fi
}

for input in resources.rc icon.ico Cargo.toml Cargo.lock; do
    touch "$input"
    expect_rerun "a change to $input"
done
probe=src/check-build-date.probe
trap 'rm -f "$probe"' EXIT
: >"$probe"
expect_rerun "a new file under src/"
rm -f "$probe"

if [ "$failures" -ne 0 ]; then
    echo "check-build-date: $failures check(s) failed" >&2
    exit 1
fi
echo "check-build-date: OK"
