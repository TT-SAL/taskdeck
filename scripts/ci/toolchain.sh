#!/usr/bin/env bash
# Installs exactly the toolchain rust-toolchain.toml pins, with its profile,
# components and targets, and proves it is the one cargo will use
# (scripts/ci/README.md). Every CI job that runs cargo calls it first.
#
#   bash scripts/ci/toolchain.sh             install, print versions, verify
#   bash scripts/ci/toolchain.sh --dry-run   print what it would run, change nothing
#
# Works on GitHub's Linux, Windows (Git Bash) and macOS runners whatever rustup
# they carry. It does not rely on rustup installing the override by itself
# (rustup 1.28 stopped doing that on first use): it reads the file and asks for
# that toolchain by name. It never runs `rustup default` or `rustup update`,
# and it fails unless `rustc --version`, run in the repository, reports the
# channel the file names.
#
# The file is read with a small parser of our own (no python or toml tool is
# assumed on every runner). It understands what rust-toolchain.toml uses: a
# [toolchain] table with `key = "string"` and `key = ["array", ...]` values,
# arrays over several lines, and # comments.
set -euo pipefail
# Plain bash 3.2 too (macOS): empty arrays are expanded as ${a[@]+"${a[@]}"}.

TC_CHANNEL=""
TC_PROFILE=""
TC_COMPONENTS=()
TC_TARGETS=()

die() { echo "toolchain: error: $*" >&2; exit 1; }

# parse_toolchain_file <file>: sets TC_CHANNEL, TC_PROFILE, TC_COMPONENTS, TC_TARGETS.
parse_toolchain_file() {
    local file="$1" line key value in_table=0 pending_key="" pending=""
    TC_CHANNEL="" TC_PROFILE="" TC_COMPONENTS=() TC_TARGETS=()
    [[ -r "$file" ]] || die "cannot read $file"
    while IFS= read -r line || [[ -n "$line" ]]; do
        line="${line%$'\r'}"
        # Drop a comment. No value this file uses contains '#'.
        line="${line%%#*}"
        if [[ -n "$pending_key" ]]; then
            pending+=" $line"
            if [[ "$pending" == *"]"* ]]; then
                _tc_set "$pending_key" "$pending"
                pending_key="" pending=""
            fi
            continue
        fi
        if [[ "$line" =~ ^[[:space:]]*\[([^]]*)\][[:space:]]*$ ]]; then
            if [[ "${BASH_REMATCH[1]// /}" == toolchain ]]; then in_table=1; else in_table=0; fi
            continue
        fi
        [[ $in_table -eq 1 ]] || continue
        [[ "$line" =~ ^[[:space:]]*([A-Za-z_-]+)[[:space:]]*=[[:space:]]*(.*)$ ]] || continue
        key="${BASH_REMATCH[1]}"
        value="${BASH_REMATCH[2]}"
        if [[ "$value" == "["* && "$value" != *"]"* ]]; then
            pending_key="$key" pending="$value"
            continue
        fi
        _tc_set "$key" "$value"
    done <"$file"
    [[ -z "$pending_key" ]] || die "$file: the array '$pending_key' is not closed"
    [[ -n "$TC_CHANNEL" ]] || die "$file: no channel in [toolchain]"
    [[ "$TC_CHANNEL" =~ ^[A-Za-z0-9._-]+$ ]] || die "$file: unexpected channel '$TC_CHANNEL'"
    return 0
}

# _tc_set <key> <raw value>
_tc_set() {
    local key="$1" raw="$2" item re='"([^"]*)"'
    local -a items=()
    while [[ "$raw" =~ $re ]]; do
        items+=("${BASH_REMATCH[1]}")
        raw="${raw#*"${BASH_REMATCH[0]}"}"
    done
    for item in ${items[@]+"${items[@]}"}; do
        [[ "$item" =~ ^[A-Za-z0-9._-]+$ ]] || die "unexpected value '$item' for $key"
    done
    case "$key" in
        channel) TC_CHANNEL="${items[0]:-}" ;;
        profile) TC_PROFILE="${items[0]:-}" ;;
        components) TC_COMPONENTS=(${items[@]+"${items[@]}"}) ;;
        targets) TC_TARGETS=(${items[@]+"${items[@]}"}) ;;
        *) ;;   # path and anything newer: not used here
    esac
}

# rustc_matches_channel <channel> <output of rustc --version>
# A version channel (1.97.0, or 1.97 for its newest patch) must be the version
# rustc reports; a named channel (stable, beta, nightly, nightly-2026-01-01)
# must at least be the kind of release rustc reports.
rustc_matches_channel() {
    local channel="$1" got="$2"
    if [[ "$channel" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        [[ "$got" == "rustc $channel "* || "$got" == "rustc $channel" ]]
    elif [[ "$channel" =~ ^[0-9]+\.[0-9]+$ ]]; then
        [[ "$got" =~ ^rustc\ ${channel//./\\.}\.[0-9]+(\ |$) ]]
    elif [[ "$channel" == stable ]]; then
        [[ "$got" =~ ^rustc\ [0-9]+\.[0-9]+\.[0-9]+(\ |$) ]]
    elif [[ "$channel" == beta* ]]; then
        [[ "$got" == *"-beta"* ]]
    elif [[ "$channel" == nightly* ]]; then
        [[ "$got" == *"-nightly"* ]]
    else
        return 1
    fi
}

# install_args: the arguments of `rustup toolchain install`, one per line.
install_args() {
    local c t
    echo toolchain
    echo install
    echo "$TC_CHANNEL"
    echo --no-self-update
    if [[ -n "$TC_PROFILE" ]]; then echo --profile; echo "$TC_PROFILE"; fi
    for c in ${TC_COMPONENTS[@]+"${TC_COMPONENTS[@]}"}; do echo --component; echo "$c"; done
    for t in ${TC_TARGETS[@]+"${TC_TARGETS[@]}"}; do echo --target; echo "$t"; done
}

cpu_count() {
    nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo "${NUMBER_OF_PROCESSORS:-unknown}"
}

main() {
    local dry=0
    case "${1:-}" in
        "") ;;
        --dry-run) dry=1 ;;
        *) echo "usage: bash scripts/ci/toolchain.sh [--dry-run]" >&2; exit 2 ;;
    esac
    local root
    root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
    parse_toolchain_file "$root/rust-toolchain.toml"
    echo "toolchain: rust-toolchain.toml pins channel $TC_CHANNEL, profile ${TC_PROFILE:-default}"
    echo "toolchain:   components: ${TC_COMPONENTS[*]+${TC_COMPONENTS[*]}}"
    echo "toolchain:   targets: ${TC_TARGETS[*]+${TC_TARGETS[*]}}"

    local -a args=()
    local a
    while IFS= read -r a; do args+=("$a"); done < <(install_args)
    if [[ $dry -eq 1 ]]; then
        echo "toolchain: would run (outside the repository): rustup ${args[*]}"
        if [[ ${#TC_COMPONENTS[@]} -gt 0 ]]; then
            echo "toolchain: would run: rustup component add --toolchain $TC_CHANNEL ${TC_COMPONENTS[*]}"
        fi
        if [[ ${#TC_TARGETS[@]} -gt 0 ]]; then
            echo "toolchain: would run: rustup target add --toolchain $TC_CHANNEL ${TC_TARGETS[*]}"
        fi
        return 0
    fi

    command -v rustup >/dev/null || die "rustup is not on PATH"
    rustup --version 2>/dev/null | head -n 1 || true

    # Outside the repository, so no rustup version consults (or trips over)
    # the override while the toolchain it names is not yet installed.
    (
        cd "${HOME:-/}" 2>/dev/null || cd /
        rustup "${args[@]}"
        # An older rustup may leave an already installed toolchain without a
        # requested component or target; asking again is a no-op otherwise.
        if [[ ${#TC_COMPONENTS[@]} -gt 0 ]]; then
            rustup component add --toolchain "$TC_CHANNEL" ${TC_COMPONENTS[@]+"${TC_COMPONENTS[@]}"}
        fi
        if [[ ${#TC_TARGETS[@]} -gt 0 ]]; then
            rustup target add --toolchain "$TC_CHANNEL" ${TC_TARGETS[@]+"${TC_TARGETS[@]}"}
        fi
    ) || die "rustup could not install toolchain $TC_CHANNEL"

    cd "$root"
    echo "toolchain: active in the repository: $(rustup show active-toolchain 2>&1 | head -n 1)"
    local rustc_v cargo_v
    rustc_v="$(rustc --version)" || die "rustc --version failed in the repository"
    cargo_v="$(cargo --version)" || die "cargo --version failed in the repository"
    echo "$rustc_v"
    echo "$cargo_v"
    echo "toolchain: $(cpu_count) CPUs"
    if ! rustc_matches_channel "$TC_CHANNEL" "$rustc_v"; then
        die "rustc reports '$rustc_v', but rust-toolchain.toml pins channel $TC_CHANNEL"
    fi
    echo "toolchain: ok, rustc matches channel $TC_CHANNEL"
}

# Sourcing the file (as scripts/tests/test-ci-toolchain.sh does) defines the
# functions without running anything.
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
