#!/usr/bin/env bash
# The supply-chain gate (SYNC.md §8 "Supply chain"; R4 §4.2).
#
#   bash scripts/supply-chain.sh [--base <rev>]
#
# Runs, in order and always all three:
#   1. cargo deny check advisories bans sources   (deny.toml)
#   2. cargo audit                                 (.cargo/audit.toml)
#   3. the seven-day cooldown (scripts/cooldown.sh), against <rev> when --base
#      is given (passed through as `cooldown.sh --base <rev>`), otherwise
#      against the cooldown's default base
# and exits non-zero if any of them failed (2, before running anything, on an
# unknown argument; --base is the only option). cargo-deny and cargo-audit must be
# the versions scripts/dev-setup.sh pins, installed by it; the script runs
# those binaries from the directory dev-setup installs into.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$here")"
cd "$root" || exit 2
TOOLS_DIR="${TASKDECK_TOOLS_DIR:-${CARGO_HOME:-$HOME/.cargo}/bin}"

usage() { echo "usage: bash scripts/supply-chain.sh [--base <rev>]" >&2; exit 2; }
cooldown_args=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        --base)
            [[ $# -ge 2 && -n "$2" && ${#cooldown_args[@]} -eq 0 ]] || usage
            cooldown_args=(--base "$2")
            shift 2
            ;;
        *)
            echo "supply-chain: unknown argument: $1" >&2
            usage
            ;;
    esac
done

failed=()
banner() { printf '\n==== supply-chain: %s ====\n' "$1"; }

# Which pinned tools are installed, by hash (dev-setup.sh --check).
check_out="$(bash "$here/dev-setup.sh" --check foundation 2>&1)"
tool_ok() { grep -q "^ok  *$1 " <<<"$check_out"; }
missing_hint() {
    echo "supply-chain: $1 (the pinned version) is not installed in $TOOLS_DIR:"
    grep " $1" <<<"$check_out" | sed 's/^/supply-chain:   /'
    echo "supply-chain: install it with: bash scripts/dev-setup.sh foundation"
}

banner "1/3 cargo deny check advisories bans sources"
if tool_ok cargo-deny; then
    if "$TOOLS_DIR/cargo-deny" --workspace --locked --all-features \
        check advisories bans sources; then
        echo "supply-chain: cargo deny: passed"
    else
        echo "supply-chain: cargo deny: FAILED"
        failed+=("cargo deny")
    fi
else
    missing_hint cargo-deny
    failed+=("cargo deny (not installed)")
fi

banner "2/3 cargo audit"
if tool_ok cargo-audit; then
    if "$TOOLS_DIR/cargo-audit" audit; then
        echo "supply-chain: cargo audit: passed"
    else
        echo "supply-chain: cargo audit: FAILED"
        failed+=("cargo audit")
    fi
else
    missing_hint cargo-audit
    failed+=("cargo audit (not installed)")
fi

if [[ ${#cooldown_args[@]} -gt 0 ]]; then
    banner "3/3 cooldown (7 days, new packages since ${cooldown_args[1]})"
else
    banner "3/3 cooldown (7 days, new packages since the default base)"
fi
if bash "$here/cooldown.sh" ${cooldown_args[@]+"${cooldown_args[@]}"}; then
    echo "supply-chain: cooldown: passed"
else
    echo "supply-chain: cooldown: FAILED"
    failed+=("cooldown")
fi

echo
if [[ ${#failed[@]} -eq 0 ]]; then
    echo "supply-chain: all three checks passed"
    exit 0
fi
printf 'supply-chain: FAILED: %s\n' "$(IFS=,; echo "${failed[*]}" | sed 's/,/, /g')"
exit 1
