#!/usr/bin/env bash
# Tests for scripts/supply-chain.sh's command line, with fake dev-setup.sh,
# cooldown.sh, cargo-deny and cargo-audit that log how they were run: --base
# <rev> reaches the cooldown unchanged, no option means the cooldown's default
# base, any other argument is refused before anything runs, and all three
# checks run even when one fails.
set -uo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

R="$TMP/repo"
LOG="$TMP/log"
export FAKE_LOG="$LOG"
mkdir -p "$R/scripts" "$TMP/tools"
cp "$SCRIPTS_DIR/supply-chain.sh" "$R/scripts/supply-chain.sh"
printf 'echo "ok       cargo-deny 0.20.2"\necho "ok       cargo-audit 0.23.0"\n' >"$R/scripts/dev-setup.sh"
printf 'echo "cooldown${*:+ $*}" >>"$FAKE_LOG"\nexit "${CD_EXIT:-0}"\n' >"$R/scripts/cooldown.sh"
printf '#!/bin/sh\necho "cargo-deny $*" >>"$FAKE_LOG"\nexit "${DENY_EXIT:-0}"\n' >"$TMP/tools/cargo-deny"
printf '#!/bin/sh\necho "cargo-audit $*" >>"$FAKE_LOG"\nexit "${AUDIT_EXIT:-0}"\n' >"$TMP/tools/cargo-audit"
chmod +x "$TMP/tools/cargo-deny" "$TMP/tools/cargo-audit"

run() { : >"$LOG"; env TASKDECK_TOOLS_DIR="$TMP/tools" "$@" bash "$R/scripts/supply-chain.sh" "${ARGS[@]}"; }
log_is() {
    if [[ "$(cat "$LOG")" == "$2" ]]; then
        PASSES=$((PASSES + 1)); echo "ok    $1"
    else
        FAILS=$((FAILS + 1)); echo "FAIL  $1"; sed 's/^/        got: /' "$LOG"; sed 's/^/        wanted: /' <<<"$2"
    fi
}
ALL3="cargo-deny --workspace --locked --all-features check advisories bans sources
cargo-audit audit"

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

finish
