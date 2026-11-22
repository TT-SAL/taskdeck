#!/usr/bin/env bash
# A stage's gate (SYNC_PLAN.md §6: "scripts/gate.sh <stage> exits 0 for
# everything a container can run, and the stage's CI jobs are green for the
# rest. The script prints the items only CI can check.").
#
#   bash scripts/gate.sh <stage>               run every `run` item, print every item
#   bash scripts/gate.sh --list <stage>        print the items, run nothing
#   bash scripts/gate.sh --only <id> <stage>   run one item
#   option: --root <dir>  the repository whose scripts/gates/ and commands are used
#                         (the tests use it; default this repository)
#
# The gate of each stage is data, in scripts/gates/<stage>.gate (format and
# conventions: scripts/gates/README.md), one item per line:
#   run | <id> | <what the plan asks, with its § reference> | <command, run from the root>
#   ci  | <id> | <what the plan asks, with its § reference> | <the CI job that checks it>
# Every `run` item runs, also after one failed, with bash -o pipefail and stdin
# from /dev/null; its output is shown as it runs. At the end every item is
# listed with PASS, FAIL or CI-ONLY. Exit 0 only if every `run` item that ran
# passed; 1 if one failed; 2 for an unknown stage, a missing or empty gate
# file, a malformed line, or a gate without the items every stage has, last,
# in this order (§3.3):
#   spec-coverage     bash scripts/spec-coverage.sh <stage>
#   no-unimplemented  bash scripts/gate-lib.sh no-unimplemented <stage>   (not foundation's)
#   no-skipped-tests  bash scripts/gate-lib.sh no-skipped-tests <stage>
set -uo pipefail

STAGES="foundation core seal relay engine android-shell device recovery desktop android release rc"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$here")"

usage() { sed -n '6,11p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
err() { echo "gate: error: $*" >&2; exit 2; }
trim() { local s="$1"; s="${s#"${s%%[![:space:]]*}"}"; s="${s%"${s##*[![:space:]]}"}"; printf '%s' "$s"; }

KINDS=() IDS=() DESCS=() CMDS=()

# load <gate file> <stage>: fills the arrays, or exits 2 naming the problem.
load() {
    local file="$1" stage="$2" line n=0 kind id desc cmd extra i
    [[ -f "$file" ]] || err "no gate file $file for stage $stage"
    local bad=0
    while IFS= read -r line || [[ -n "$line" ]]; do
        n=$((n + 1))
        line="${line%$'\r'}"
        [[ "$(trim "$line")" == "" || "$(trim "$line")" == '#'* ]] && continue
        IFS='|' read -r kind id desc cmd <<<"$line"
        kind="$(trim "$kind")" id="$(trim "$id")" desc="$(trim "$desc")" cmd="$(trim "$cmd")"
        extra=""
        if [[ "$kind" != run && "$kind" != ci ]]; then
            extra="the first field must be 'run' or 'ci', not '$kind'"
        elif [[ ! "$id" =~ ^[a-z0-9][a-z0-9-]*$ ]]; then
            extra="the id '$id' must be lowercase letters, digits and hyphens"
        elif [[ -z "$desc" || "$desc" != *"§"* ]]; then
            extra="the description must say what the plan asks, with its § reference"
        elif [[ -z "$cmd" ]]; then
            extra="the fourth field (the command, or the CI job) is empty"
        else
            for i in "${IDS[@]+"${IDS[@]}"}"; do
                [[ "$i" == "$id" ]] && extra="the id '$id' is used twice"
            done
        fi
        if [[ -n "$extra" ]]; then
            echo "gate: error: $file:$n: malformed line: $extra" >&2
            bad=1
            continue
        fi
        KINDS+=("$kind") IDS+=("$id") DESCS+=("$desc") CMDS+=("$cmd")
    done <"$file"
    [[ $bad -eq 0 ]] || exit 2
    [[ ${#IDS[@]} -gt 0 ]] || err "$file holds no item"

    # The items every stage's gate has, last, in this order.
    local -a want_ids=(spec-coverage) want_cmds=("bash scripts/spec-coverage.sh $stage")
    if [[ "$stage" != foundation ]]; then
        want_ids+=(no-unimplemented) want_cmds+=("bash scripts/gate-lib.sh no-unimplemented $stage")
    fi
    want_ids+=(no-skipped-tests) want_cmds+=("bash scripts/gate-lib.sh no-skipped-tests $stage")
    local k=${#want_ids[@]} total=${#IDS[@]} j at
    for ((j = 0; j < k; j++)); do
        at=$((total - k + j))
        if [[ $at -lt 0 || "${KINDS[$at]}" != run || "${IDS[$at]}" != "${want_ids[$j]}" || "${CMDS[$at]}" != "${want_cmds[$j]}" ]]; then
            err "$file: the last items must be: $(for ((i = 0; i < k; i++)); do printf "'run | %s | ... | %s' " "${want_ids[$i]}" "${want_cmds[$i]}"; done)(SYNC_PLAN.md §3.3)"
        fi
    done
}

main() {
    local mode=run only="" stage=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --list) mode=list ;;
            --only) [[ $# -ge 2 ]] || usage; only="$2"; shift ;;
            --root) [[ $# -ge 2 ]] || usage; ROOT="$(cd "$2" && pwd)" || exit 2; shift ;;
            -h|--help) usage ;;
            -*) echo "gate: unknown option $1" >&2; usage ;;
            *) [[ -z "$stage" ]] || usage; stage="$1" ;;
        esac
        shift
    done
    [[ -n "$stage" ]] || usage
    [[ " $STAGES " == *" $stage "* ]] || err "unknown stage '$stage' (stages: $STAGES)"
    local file="$ROOT/scripts/gates/$stage.gate"
    load "$file" "$stage"
    local n=${#IDS[@]} i
    if [[ -n "$only" ]]; then
        local found=""
        for ((i = 0; i < n; i++)); do [[ "${IDS[$i]}" == "$only" ]] && found=$i; done
        [[ -n "$found" ]] || err "stage $stage has no item '$only' (items: ${IDS[*]})"
    fi

    local w=0
    for ((i = 0; i < n; i++)); do [[ ${#IDS[$i]} -gt $w ]] && w=${#IDS[$i]}; done

    if [[ "$mode" == list ]]; then
        local runs=0
        for ((i = 0; i < n; i++)); do [[ "${KINDS[$i]}" == run ]] && runs=$((runs + 1)); done
        echo "gate $stage: $n items ($runs run here, $((n - runs)) CI-only), from scripts/gates/$stage.gate"
        for ((i = 0; i < n; i++)); do
            [[ -n "$only" && "${IDS[$i]}" != "$only" ]] && continue
            if [[ "${KINDS[$i]}" == run ]]; then
                printf 'RUN      %-*s  %s\n%*s$ %s\n' "$w" "${IDS[$i]}" "${DESCS[$i]}" $((w + 11)) "" "${CMDS[$i]}"
            else
                printf 'CI-ONLY  %-*s  %s\n%*sCI: %s\n' "$w" "${IDS[$i]}" "${DESCS[$i]}" $((w + 11)) "" "${CMDS[$i]}"
            fi
        done
        exit 0
    fi

    local -a result=()
    local failed=() status t0
    for ((i = 0; i < n; i++)); do
        result[$i]=""
        [[ -n "$only" && "${IDS[$i]}" != "$only" ]] && continue
        if [[ "${KINDS[$i]}" == ci ]]; then
            result[$i]=CI-ONLY
            continue
        fi
        echo "==== gate $stage: ${IDS[$i]}: ${DESCS[$i]}"
        echo "==== \$ ${CMDS[$i]}"
        t0=$SECONDS
        (cd "$ROOT" && bash -o pipefail -c "${CMDS[$i]}" </dev/null) 2>&1
        status=$?
        if [[ $status -eq 0 ]]; then
            result[$i]=PASS
            echo "==== PASS ${IDS[$i]} ($((SECONDS - t0)) s)"
        else
            result[$i]=FAIL
            failed+=("${IDS[$i]}")
            echo "==== FAIL ${IDS[$i]} (exit $status, $((SECONDS - t0)) s)"
        fi
    done

    echo
    echo "gate $stage:"
    local passed=0 cionly=0
    for ((i = 0; i < n; i++)); do
        [[ -z "${result[$i]}" ]] && continue
        printf '  %-8s %-*s  %s\n' "${result[$i]}" "$w" "${IDS[$i]}" "${DESCS[$i]}"
        if [[ "${result[$i]}" == CI-ONLY ]]; then
            printf '  %-8s %-*s  CI: %s\n' "" "$w" "" "${CMDS[$i]}"
            cionly=$((cionly + 1))
        elif [[ "${result[$i]}" == PASS ]]; then
            passed=$((passed + 1))
        fi
    done
    echo
    if [[ ${#failed[@]} -gt 0 ]]; then
        echo "gate $stage: FAILED: ${failed[*]} ($passed passed; $cionly CI-only items not checked here)"
        exit 1
    fi
    echo "gate $stage: passed: $passed run items; $cionly CI-only items: check the CI jobs named above"
}

main "$@"
