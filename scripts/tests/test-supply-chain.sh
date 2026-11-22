#!/usr/bin/env bash
# Tests for scripts/supply-chain.sh's command line, with fake dev-setup.sh,
# cooldown.sh, cargo-deny and cargo-audit that log how they were run: --base
# <rev> reaches the cooldown unchanged, no option means the cooldown's default
# base, any other argument is refused before anything runs, and all three
# checks run even when one fails, and an "error:" or an index-failure warning
# from cargo-audit fails the run even when it exits 0. A cargo configuration
# that replaces or overrides crates.io, or includes another file, fails the run;
# a python3 that cannot run the check fails it for that reason; and a tool that
# is not usable is reported with dev-setup's own words.
set -uo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

R="$TMP/repo"
LOG="$TMP/log"
export FAKE_LOG="$LOG"
mkdir -p "$R/scripts" "$TMP/tools"
cp "$SCRIPTS_DIR/supply-chain.sh" "$R/scripts/supply-chain.sh"
# It checks the supply-chain row only: the job installs nothing else.
printf '[ "$*" = "--check supply-chain" ] || { echo "fake dev-setup: called with $*"; exit 1; }\n[ -z "${DEVSETUP_OUT:-}" ] || { printf "%%s\\n" "$DEVSETUP_OUT"; exit 1; }\necho "ok       cargo-deny 0.20.2"\necho "ok       cargo-audit 0.23.0"\n' >"$R/scripts/dev-setup.sh"
printf 'echo "cooldown${*:+ $*}" >>"$FAKE_LOG"\nexit "${CD_EXIT:-0}"\n' >"$R/scripts/cooldown.sh"
printf '#!/bin/sh\necho "cargo-deny $*" >>"$FAKE_LOG"\nexit "${DENY_EXIT:-0}"\n' >"$TMP/tools/cargo-deny"
printf '#!/bin/sh\necho "cargo-audit $*" >>"$FAKE_LOG"\n[ -z "${AUDIT_ERR:-}" ] || echo "$AUDIT_ERR" >&2\nexit "${AUDIT_EXIT:-0}"\n' >"$TMP/tools/cargo-audit"
chmod +x "$TMP/tools/cargo-deny" "$TMP/tools/cargo-audit"

HOME_C="$TMP/cargo-home"
mkdir -p "$HOME_C"
run() { : >"$LOG"; env TASKDECK_TOOLS_DIR="$TMP/tools" CARGO_HOME="$HOME_C" "$@" bash "$R/scripts/supply-chain.sh" "${ARGS[@]}"; }
log_is() {
    if [[ "$(cat "$LOG")" == "$2" ]]; then
        PASSES=$((PASSES + 1)); echo "ok    $1"
    else
        FAILS=$((FAILS + 1)); echo "FAIL  $1"; sed 's/^/        got: /' "$LOG"; sed 's/^/        wanted: /' <<<"$2"
    fi
}
# -W notice: a notice advisory is printed but does not fail cargo-deny, as with
# cargo-audit (ruling 4); without it cargo-deny 0.20.2 fails on a notice.
ALL3="cargo-deny --workspace --locked --all-features check -D index-failure -D index-cache-load-failure -W notice advisories bans sources
cargo-audit audit --color never"

echo "-- the cooldown's base"
ARGS=()
expect "no option" 0 run
log_is "no option: the cooldown runs without --base" "$ALL3
cooldown"
output_has "says the default base" "the default base"
ARGS=(--base 0123456789abcdef0123456789abcdef01234567)
expect "--base <rev>" 0 run
log_is "--base reaches the cooldown" "$ALL3
cooldown --base 0123456789abcdef0123456789abcdef01234567"
output_has "names the base" "new packages since 0123456789abcdef0123456789abcdef01234567"

echo "-- anything else is refused before any check runs"
ARGS=(--base "")
expect "refused: --base with an empty value" 2 run
log_is "nothing ran: --base with an empty value" ""
for a in "--all" "--base" "--base|a|--base|b" "--base|a|extra" "extra" "--now|2026-11-22T00:00:00Z"; do
    IFS='|' read -r -a ARGS <<<"$a"
    expect "refused: ${a//|/ }" 2 run
    log_is "nothing ran: ${a//|/ }" ""
done
output_has "prints the usage" "usage: bash scripts/supply-chain.sh [--base <rev>]"

echo "-- all three run, and any failure fails the run"
ARGS=(--base abc)
expect "cargo deny failing" 1 run DENY_EXIT=1
log_is "cargo deny failing: the other two still ran" "$ALL3
cooldown --base abc"
expect "cargo audit failing" 1 run AUDIT_EXIT=1
expect "the cooldown failing" 1 run CD_EXIT=1
output_has "says which failed" "FAILED: cooldown"

echo "-- an index failure fails closed"
# cargo-audit 0.23.0 exits 0 when it cannot read a crate's index entry; the
# gate must still fail. (cargo-deny's index-failure warnings are made errors
# by the -D options the log checks above.)
expect "cargo audit printing an error but exiting 0" 1 run \
    AUDIT_ERR="error: couldn't check if the package is yanked: registry: invalid type"
output_has "names cargo audit" "FAILED: cargo audit"
# cargo-audit 0.23.0 also exits 0, with only a warning, when it cannot update
# or open the crates.io index at all; it then checks no crate for yanking.
expect "cargo audit unable to update the index, exiting 0" 1 run \
    AUDIT_ERR="warning: couldn't update crates.io index: unable to acquire filesystem lock"
output_has "names cargo audit (update)" "FAILED: cargo audit"
expect "cargo audit unable to open the index, exiting 0" 1 run \
    AUDIT_ERR="warning: couldn't open crates.io index: not found"
output_has "names cargo audit (open)" "FAILED: cargo audit"
expect "cargo audit's lock-wait warning alone passes" 0 run \
    AUDIT_ERR="warning: directory /x/advisory-db is locked, waiting for up to 300 seconds for it to become available"

echo "-- a cargo configuration that replaces or overrides crates.io fails"
ARGS=()
mkdir -p "$R/.cargo"
replaces() { # <case> <file> <content>: the file fails the run, all three still run
    printf '%s\n' "$3" >"$2"
    expect "$1 fails" 1 run
    log_is "$1: all three still ran" "$ALL3
cooldown"
    output_has "$1 is named" "supply-chain: crates.io replaced or overridden: $2: "
    output_has "$1: the failure is listed" "FAILED: cargo configuration (crates.io replaced)"
    rm -f "$2"
}
replaces "a vendored directory in .cargo/config.toml" "$R/.cargo/config.toml" '[source.crates-io]
replace-with = "elsewhere"
[source.elsewhere]
directory = "vendor"'
replaces "a dotted source key in .cargo/config" "$R/.cargo/config" 'source.crates-io.replace-with = "m"'
replaces "a [patch] in CARGO_HOME" "$HOME_C/config.toml" '[patch.crates-io]
itoa = { path = "x" }'
replaces "a paths override" "$R/.cargo/config.toml" 'paths = ["x"]'
replaces "a configuration that is not TOML" "$R/.cargo/config.toml" '[source'
# cargo (1.97, stable) reads the files an `include` names: one that sets
# [source] would otherwise pass unseen, so any include fails.
printf '[source.crates-io]\nreplace-with = "m"\n' >"$R/.cargo/mirror.toml"
replaces "an include of a file that sets source" "$R/.cargo/config.toml" 'include = ["mirror.toml"]'
replaces "an include in CARGO_HOME" "$HOME_C/config.toml" 'include = [{ path = "mirror.toml" }]'
rm -f "$R/.cargo/mirror.toml"
expect "the environment variable fails" 1 run CARGO_SOURCE_CRATES_IO_REPLACE_WITH=m
output_has "the environment variable is named" "the environment sets CARGO_SOURCE_CRATES_IO_REPLACE_WITH"

echo "-- without python3 3.11 the configuration check fails for that reason"
mkdir -p "$TMP/oldpy"
printf '#!/bin/sh\necho "ModuleNotFoundError: No module named tomllib" >&2\nexit 1\n' >"$TMP/oldpy/python3"
chmod +x "$TMP/oldpy/python3"
expect "an old python3 fails" 1 run PATH="$TMP/oldpy:$PATH"
output_has "the reason is python3" "supply-chain: cannot check the cargo configuration: python3 3.11 or later is needed"
output_lacks "it does not blame a replacement" "crates.io replaced"
log_is "an old python3: all three still ran" "$ALL3
cooldown"
printf '[build]\njobs = 2\n[registries.crates-io]\nprotocol = "sparse"\n' >"$R/.cargo/config.toml"
expect "a configuration that leaves crates.io alone passes" 0 run
rm -f "$R/.cargo/config.toml"

echo "-- a tool that is not usable is reported in dev-setup's words"
expect "an unsupported platform fails" 1 run DEVSETUP_OUT="dev-setup: error: only Linux x86_64 is supported for now (this is Linux aarch64)"
output_has "the reason is printed" "supply-chain:   dev-setup: error: only Linux x86_64 is supported"
output_lacks "it does not say not installed" "not installed"
log_is "the cooldown still ran" "cooldown"
expect "a missing tool fails" 1 run DEVSETUP_OUT="ok       cargo-deny 0.20.2
missing  cargo-audit 0.23.0
dev-setup: run: bash scripts/dev-setup.sh supply-chain"
output_has "what to install is printed" "supply-chain:   dev-setup: run: bash scripts/dev-setup.sh supply-chain"
output_has "only the missing tool fails" "FAILED: cargo audit (pinned tool not usable)"

finish
