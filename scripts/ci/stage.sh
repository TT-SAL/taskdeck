#!/usr/bin/env bash
# One later stage's CI checks (the core, seal, engine, device, recovery, relay
# and android jobs of .github/workflows/ci.yml), so those stages add checks
# without editing the shared workflow file.
#
#   bash scripts/ci/stage.sh <stage>
#
# THE CONTRACT
#
# 1. Each stage owns directories (SYNC_PLAN.md §4):
#      core      crates/taskdeck-core
#      seal      crates/taskdeck-seal
#      engine    crates/taskdeck-sync
#      device    crates/taskdeck-device  tests/e2e
#      recovery  crates/taskdeck-backup  crates/taskdeck-recover
#      relay     relay
#      android   android  crates/taskdeck-android
# 2. A directory that does not exist yet is reported and skipped, not an error.
# 3. A directory with a Cargo.toml is a Rust package. For each, in the order
#    above, the script runs, from the repository root:
#      cargo test --locked -p <package>
#      cargo clippy --locked -p <package> --all-targets -- -D warnings
#    where <package> is the `name` in that Cargo.toml's [package] table. (A
#    directory under crates/ that exists without a Cargo.toml is an error.)
# 4. Then, for each of the stage's directories that holds a file `ci.sh`, it
#    runs `bash <dir>/ci.sh` from the repository root with these variables set:
#      CI_STAGE  the stage name
#      CI_OS     linux, windows or macos
#      CI_WIDE   true on the wide runs (pull requests, "[ci full]"), else false
#    That hook is the stage's own: wasm32 builds, budgets, `node --test`,
#    Gradle, anything else. It exits non-zero to fail the job. The toolchain
#    from rust-toolchain.toml is installed before it runs; anything else it
#    needs it installs itself (scripts/dev-setup.sh <stage>).
# 5. Every step runs even after one fails; the script prints a summary and
#    exits 1 if any step failed (2 for a usage error).
#
# CI_WIDE is taken from the environment (the job passes the `changes` job's
# `wide` output); unset means false. CI_OS comes from RUNNER_OS on GitHub's
# runners, else from uname. See scripts/ci/README.md.
set -uo pipefail
# Plain bash 3.2 too (macOS): empty arrays are expanded as ${a[@]+"${a[@]}"}.

stage_dirs() {
    case "$1" in
        core) echo crates/taskdeck-core ;;
        seal) echo crates/taskdeck-seal ;;
        engine) echo crates/taskdeck-sync ;;
        device) echo crates/taskdeck-device tests/e2e ;;
        recovery) echo crates/taskdeck-backup crates/taskdeck-recover ;;
        relay) echo relay ;;
        android) echo android crates/taskdeck-android ;;
        *) return 1 ;;
    esac
}

ci_os() {
    case "${RUNNER_OS:-}" in
        Linux) echo linux; return ;;
        Windows) echo windows; return ;;
        macOS) echo macos; return ;;
    esac
    case "$(uname -s)" in
        Linux*) echo linux ;;
        Darwin*) echo macos ;;
        MINGW* | MSYS* | CYGWIN* | Windows*) echo windows ;;
        *) echo unknown ;;
    esac
}

# package_name <Cargo.toml>: the name in its [package] table, or nothing.
package_name() {
    # A basic ("x") or a literal ('x') TOML string.
    local line in_pkg=0 re='^[[:space:]]*name[[:space:]]*=[[:space:]]*("([^"]+)"|'"'"'([^'"'"']+)'"'"')'
    while IFS= read -r line || [[ -n "$line" ]]; do
        line="${line%$'\r'}"
        if [[ "$line" =~ ^[[:space:]]*\[ ]]; then
            if [[ "$line" =~ ^[[:space:]]*\[package\][[:space:]]*(#.*)?$ ]]; then in_pkg=1; else in_pkg=0; fi
            continue
        fi
        if [[ $in_pkg -eq 1 && "$line" =~ $re ]]; then
            echo "${BASH_REMATCH[2]}${BASH_REMATCH[3]}"
            return 0
        fi
    done <"$1"
}

STEPS=()
FAILED=()

run_step() {
    local label="$1"
    shift
    echo
    echo "==== stage.sh: $label"
    if "$@"; then
        STEPS+=("ok      $label")
    else
        STEPS+=("FAILED  $label")
        FAILED+=("$label")
    fi
}

note_failure() {
    echo "stage.sh: error: $1" >&2
    STEPS+=("FAILED  $1")
    FAILED+=("$1")
}

main() {
    if [[ $# -ne 1 ]]; then
        echo "usage: bash scripts/ci/stage.sh <core|seal|engine|device|recovery|relay|android>" >&2
        exit 2
    fi
    local stage="$1" dirs
    if ! dirs="$(stage_dirs "$stage")"; then
        echo "stage.sh: error: unknown stage '$stage'" >&2
        exit 2
    fi
    local root
    root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
    cd "$root" || exit 2

    export CI_STAGE="$stage"
    CI_OS="$(ci_os)"
    export CI_OS
    case "${CI_WIDE:-false}" in
        true) CI_WIDE=true ;;
        *) CI_WIDE=false ;;
    esac
    export CI_WIDE
    echo "stage.sh: stage $CI_STAGE on $CI_OS, wide=$CI_WIDE; directories: $dirs"

    local d pkg present=()
    for d in $dirs; do
        if [[ -d "$d" ]]; then
            present+=("$d")
        else
            echo "stage.sh: $d is not in this checkout yet; skipped"
            STEPS+=("skipped $d (not in this checkout yet)")
        fi
    done

    for d in ${present[@]+"${present[@]}"}; do
        if [[ -f "$d/Cargo.toml" ]]; then
            pkg="$(package_name "$d/Cargo.toml")"
            if [[ -z "$pkg" ]]; then
                note_failure "$d/Cargo.toml has no [package] name"
                continue
            fi
            run_step "cargo test -p $pkg" cargo test --locked -p "$pkg"
            run_step "cargo clippy -p $pkg" cargo clippy --locked -p "$pkg" --all-targets -- -D warnings
        elif [[ "$d" == crates/* ]]; then
            note_failure "$d exists but has no Cargo.toml"
        fi
    done

    for d in ${present[@]+"${present[@]}"}; do
        if [[ -f "$d/ci.sh" ]]; then
            run_step "bash $d/ci.sh" bash "$d/ci.sh"
        fi
    done

    echo
    echo "==== stage.sh: summary for stage $stage"
    if [[ ${#STEPS[@]} -eq 0 ]]; then
        echo "nothing to do"
    else
        printf '  %s\n' "${STEPS[@]}"
    fi
    if [[ ${#FAILED[@]} -gt 0 ]]; then
        echo "stage.sh: FAILED: ${#FAILED[@]} step(s)"
        exit 1
    fi
    echo "stage.sh: stage $stage passed"
}

main "$@"
