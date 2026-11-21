#!/usr/bin/env bash
# The supply-chain gate (SYNC.md §8 "Supply chain"; R4 §4.2).
#
#   bash scripts/supply-chain.sh
#
# Runs, in order and always all three:
#   1. cargo deny check advisories bans sources   (deny.toml)
#   2. cargo audit                                 (.cargo/audit.toml)
#   3. the seven-day cooldown against its default base (scripts/cooldown.sh)
# and exits non-zero if any of them failed. cargo-deny and cargo-audit must be
# the versions scripts/dev-setup.sh pins, installed by it; the script runs
# those binaries from the directory dev-setup installs into.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$here")"
cd "$root" || exit 2
TOOLS_DIR="${TASKDECK_TOOLS_DIR:-${CARGO_HOME:-$HOME/.cargo}/bin}"

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

banner "3/3 cooldown (7 days, new packages since the base)"
if bash "$here/cooldown.sh"; then
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
