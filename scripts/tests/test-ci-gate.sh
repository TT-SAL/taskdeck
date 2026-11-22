#!/usr/bin/env bash
# Tests for scripts/ci/gate.sh: skipped passes, failure and cancelled fail,
# `changes` must succeed, and unreadable input fails.
set -uo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
GATE="$SCRIPTS_DIR/ci/gate.sh"

gate() { NEEDS_JSON="$1" bash "$GATE"; }
j() {   # j changes=success linux=skipped ... : a needs object
    local out="{" sep="" kv
    for kv in "$@"; do
        out+="$sep\"${kv%%=*}\":{\"result\":\"${kv#*=}\",\"outputs\":{}}"
        sep=","
    done
    echo "$out}"
}

expect "everything passed" 0 gate "$(j changes=success linux=success desktop=success)"
output_has "prints a table" "linux    success  ok"
output_has "says it passed" "gate: passed"
expect "skipped counts as passed" 0 gate "$(j changes=success linux=skipped emulator=skipped core=skipped)"
expect "a docs-only run: changes passed, the rest skipped" 0 \
    gate "$(j changes=success linux=skipped desktop=skipped emulator=skipped supply-chain=skipped \
        foundation=skipped core=skipped seal=skipped engine=skipped device=skipped recovery=skipped \
        relay=skipped android=skipped)"
expect "a failed job fails the gate" 1 gate "$(j changes=success linux=failure desktop=skipped)"
output_has "names the failed job" "gate: FAILED: linux"
expect "a cancelled job fails the gate" 1 gate "$(j changes=success linux=success core=cancelled)"
output_has "names the cancelled job" "core     cancelled  FAIL"
expect "every bad job is named" 1 gate "$(j changes=success relay=failure seal=cancelled)"
output_has "both named" "gate: FAILED: relay, seal"
expect "changes failed" 1 gate "$(j changes=failure linux=skipped)"
expect "changes cancelled" 1 gate "$(j changes=cancelled linux=skipped)"
expect "changes skipped is not enough" 1 gate "$(j changes=skipped linux=skipped)"
output_has "says why" "FAIL: must succeed"
expect "changes missing" 1 gate "$(j linux=success)"
output_has "says changes is missing" "(missing)"
expect "an unknown result fails" 1 gate "$(j changes=success linux=neutral)"
expect "a job without a result fails" 1 gate '{"changes":{"result":"success"},"linux":{}}'
expect "empty input fails" 1 gate ""
expect "an empty object fails" 1 gate "{}"
expect "not JSON fails" 1 gate "success"
expect "a JSON array fails" 1 gate '[{"result":"success"}]'
expect "NEEDS_JSON unset fails" 1 env -u NEEDS_JSON bash "$GATE"

finish
