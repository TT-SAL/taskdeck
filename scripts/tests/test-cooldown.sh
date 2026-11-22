#!/usr/bin/env bash
# Tests for scripts/cooldown.sh, offline: made-up lockfiles and index files,
# a fixed --now. No network.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

COOL="$SCRIPTS_DIR/cooldown.sh"
NOW="2026-11-21T12:00:00Z"
REG="registry+https://github.com/rust-lang/crates.io-index"
SPARSE="sparse+https://index.crates.io/"
NO_EXEMPTIONS="$TMP/no-exemptions.txt"
: >"$NO_EXEMPTIONS"

# pkg <name> <version> [source]: one [[package]] table. An empty source writes
# a path package (no source line).
pkg() {
    printf '\n[[package]]\nname = "%s"\nversion = "%s"\n' "$1" "$2"
    local src="${3-$REG}"
    if [[ -n "$src" ]]; then
        printf 'source = "%s"\n' "$src"
        [[ "$src" == git+* ]] || printf 'checksum = "%064d"\n' 0
    fi
}

# lock <file> <pkg lines...>: write a lockfile from pkg outputs given on stdin.
lock() { { echo 'version = 4'; cat; } >"$1"; }

# index_line <name> <version> <pubtime> [yanked]
index_line() {
    printf '{"name":"%s","vers":"%s","deps":[],"cksum":"%064d","features":{},"yanked":%s,"pubtime":"%s"}\n' \
        "$1" "$2" 0 "${4:-false}" "$3"
}

# index_put <dir> <name> <lines on stdin>: write <dir>/<sparse path of name>.
index_put() {
    local dir="$1" name="$2" n path
    n="$(tr '[:upper:]' '[:lower:]' <<<"$name")"
    case ${#n} in
        1) path="1/$n" ;;
        2) path="2/$n" ;;
        3) path="3/${n:0:1}/$n" ;;
        *) path="${n:0:2}/${n:2:2}/$n" ;;
    esac
    mkdir -p "$dir/$(dirname "$path")"
    cat >"$dir/$path"
}

run_cool() { bash "$COOL" --now "$NOW" --exemptions "$NO_EXEMPTIONS" "$@"; }

# The base lockfile every case starts from: one old crate that is not in any
# index directory below, so looking it up would fail.
BASE="$TMP/base.lock"
pkg oldie 0.1.0 | lock "$BASE"

# --- a new crate younger than 7 days fails ---------------------------------
d="$TMP/idx-young"
index_put "$d" alpha <<<"$(index_line alpha 1.0.0 2025-01-01T00:00:00Z; index_line alpha 1.1.0 2026-11-18T12:00:00Z)"
{ pkg oldie 0.1.0; pkg alpha 1.1.0; } | lock "$TMP/young.lock"
expect "young crate fails" 1 run_cool --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$d"
output_has "young crate is named" "VIOLATION: alpha 1.1.0: published 2026-11-18T12:00:00Z, 3.0 days"
output_has "summary counts one violation" "1 violation(s)"

# --- one second short of 7 days fails; exactly 7 days passes ----------------
d="$TMP/idx-edge"
index_put "$d" alpha <<<"$(index_line alpha 1.1.0 2026-11-14T12:00:01Z)"
expect "7 days minus one second fails" 1 run_cool --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$d"
d="$TMP/idx-seven"
index_put "$d" alpha <<<"$(index_line alpha 1.1.0 2026-11-14T12:00:00Z)"
expect "exactly 7 days passes" 0 run_cool --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$d"
output_has "summary counts one check" "1 crates.io package(s) checked"

# --- an unchanged old crate is not looked up ---------------------------------
# oldie is in both lockfiles and in no index file: a lookup would report it.
expect "unchanged crate not looked up" 0 run_cool --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$d"
output_lacks "unchanged crate not reported" "oldie"
# ... but --all checks it, and there it is missing.
expect "--all looks up unchanged crates" 1 run_cool --all --lockfile "$TMP/young.lock" --index-dir "$d"
output_has "--all reports the missing crate" "VIOLATION: oldie 0.1.0: crate not found"

# --- a new version of an existing crate is checked ---------------------------
{ pkg oldie 0.2.0; } | lock "$TMP/bump.lock"
d="$TMP/idx-bump"
index_put "$d" oldie <<<"$(index_line oldie 0.1.0 2020-01-01T00:00:00Z; index_line oldie 0.2.0 2026-11-20T00:00:00Z)"
expect "new version of a known crate is checked" 1 run_cool --base-lockfile "$BASE" --lockfile "$TMP/bump.lock" --index-dir "$d"
output_has "the new version is named" "VIOLATION: oldie 0.2.0"

# --- a yanked version fails, however old ------------------------------------
d="$TMP/idx-yanked"
index_put "$d" alpha <<<"$(index_line alpha 1.1.0 2024-01-01T00:00:00Z true)"
expect "yanked version fails" 1 run_cool --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$d"
output_has "yanked is reported" "VIOLATION: alpha 1.1.0: yanked"

# --- a missing version, or a missing crate, fails ----------------------------
d="$TMP/idx-missing"
index_put "$d" alpha <<<"$(index_line alpha 1.0.0 2024-01-01T00:00:00Z)"
expect "missing version fails" 1 run_cool --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$d"
output_has "missing version is reported" "VIOLATION: alpha 1.1.0: version not found"
mkdir -p "$TMP/idx-empty"
expect "missing crate fails" 1 run_cool --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$TMP/idx-empty"
output_has "missing crate is reported" "VIOLATION: alpha 1.1.0: crate not found"

# --- a version without a publish time fails ----------------------------------
d="$TMP/idx-nopub"
index_put "$d" alpha <<<'{"name":"alpha","vers":"1.1.0","deps":[],"yanked":false}'
expect "no publish time fails" 1 run_cool --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$d"
output_has "no publish time is reported" "no publish time"

# --- exemptions --------------------------------------------------------------
d="$TMP/idx-young"
ex="$TMP/ex-valid.txt"
printf '# comment\n\nalpha 1.1.0 2026-12-01 advisory fix, approved by the lead (made-up)\n' >"$ex"
expect "valid exemption passes" 0 bash "$COOL" --now "$NOW" --exemptions "$ex" --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$d"
output_has "exemption is noted" "note: alpha 1.1.0: exempt (advisory fix, approved by the lead (made-up))"
ex="$TMP/ex-today.txt"
printf 'alpha 1.1.0 2026-11-21 holds through its expiry day\n' >"$ex"
expect "exemption on its expiry day passes" 0 bash "$COOL" --now "$NOW" --exemptions "$ex" --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$d"
ex="$TMP/ex-other.txt"
printf 'alpha 1.0.0 2026-12-01 a different version\n' >"$ex"
expect "exemption for another version does not apply" 1 bash "$COOL" --now "$NOW" --exemptions "$ex" --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$d"
ex="$TMP/ex-expired.txt"
printf 'alpha 1.1.0 2026-11-20 expired yesterday\n' >"$ex"
expect "expired exemption is an error" 2 bash "$COOL" --now "$NOW" --exemptions "$ex" --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$d"
output_has "expiry is reported" "expired on 2026-11-20"
ex="$TMP/ex-noreason.txt"
printf 'alpha 1.1.0 2026-12-01\n' >"$ex"
expect "exemption without a reason is an error" 2 bash "$COOL" --now "$NOW" --exemptions "$ex" --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$d"
output_has "missing field is reported" "needs name, version, expiry date and a reason"
ex="$TMP/ex-baddate.txt"
printf 'alpha 1.1.0 01-12-2026 date in the wrong order\n' >"$ex"
expect "exemption with a bad date is an error" 2 bash "$COOL" --now "$NOW" --exemptions "$ex" --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$d"
ex="$TMP/ex-yanked.txt"
printf 'alpha 1.1.0 2026-12-01 does not cover yanked\n' >"$ex"
expect "exemption does not cover a yanked version" 1 bash "$COOL" --now "$NOW" --exemptions "$ex" --base-lockfile "$BASE" --lockfile "$TMP/young.lock" --index-dir "$TMP/idx-yanked"
expect "the shipped exemptions file is clean" 0 bash "$COOL" --now "$NOW" --base-lockfile "$BASE" --lockfile "$BASE" --index-dir "$TMP/idx-empty"

# --- git and path packages are ignored; the sparse source counts -------------
d="$TMP/idx-sources"
index_put "$d" xyz <<<"$(index_line xyz 2.0.0 2025-06-01T00:00:00Z)"
index_put "$d" ab <<<"$(index_line ab 0.3.0 2025-06-01T00:00:00Z)"
index_put "$d" q <<<"$(index_line q 1.0.0 2025-06-01T00:00:00Z)"
{
    pkg oldie 0.1.0
    pkg fromgit 0.1.0 "git+https://example.invalid/fromgit?rev=abc#0123456789abcdef"
    pkg local-crate 0.1.0 ""
    pkg xyz 2.0.0 "$SPARSE"
    pkg ab 0.3.0
    pkg q 1.0.0
} | lock "$TMP/sources.lock"
expect "git and path packages are ignored" 0 run_cool --base-lockfile "$BASE" --lockfile "$TMP/sources.lock" --index-dir "$d"
output_has "three registry packages checked, two skipped" "3 crates.io package(s) checked (new since $BASE), 0 violation(s), 0 exempted, 2 non-crates.io package(s) skipped"

# --- the git base path, read-only: HEAD against a copy of HEAD's lockfile ----
git -C "$REPO_DIR" show HEAD:Cargo.lock >"$TMP/head.lock"
expect "--base HEAD with HEAD's lockfile looks up nothing" 0 run_cool --base HEAD --lockfile "$TMP/head.lock" --index-dir "$TMP/idx-empty"
output_has "nothing new since HEAD" "0 crates.io package(s) checked"
expect "an unknown base revision is an error" 2 run_cool --base no-such-rev-xyz --lockfile "$TMP/head.lock" --index-dir "$TMP/idx-empty"

# --- bad input -----------------------------------------------------------------
expect "--now without an offset is an error" 2 bash "$COOL" --now 2026-11-21T12:00:00 --exemptions "$NO_EXEMPTIONS" --all --lockfile "$BASE" --index-dir "$TMP/idx-empty"
expect "a missing lockfile is an error, not a violation" 2 run_cool --all --lockfile "$TMP/no-such.lock" --index-dir "$TMP/idx-empty"
output_has "the missing lockfile is named" "no-such.lock"

finish
