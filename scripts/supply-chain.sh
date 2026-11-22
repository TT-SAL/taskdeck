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
# unknown argument; --base is the only option). A crate whose registry index
# entry cargo-deny or cargo-audit cannot fetch or read fails that tool's check,
# so a yanked crate cannot pass through an index failure. Step 1 also fails
# when a cargo configuration replaces or overrides crates.io code (see there).
# cargo-deny and cargo-audit must be
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
# All of dev-setup's report: it names what is missing and how to install it,
# or why it cannot check (an unsupported platform, say).
missing_hint() {
    echo "supply-chain: $1, the pinned version, is not usable from $TOOLS_DIR; dev-setup.sh --check says:"
    sed 's/^/supply-chain:   /' <<<"$check_out"
}

banner "1/3 cargo deny check advisories bans sources"
# Cargo.lock still says crates.io when a cargo configuration replaces crates.io
# with a mirror or a vendored directory ([source]), or overrides its crates
# ([patch], paths), and cargo-deny and cargo-audit read only the lockfile. So
# any of those keys in a configuration file cargo reads here (.cargo/config and
# .cargo/config.toml in this directory, every parent and CARGO_HOME), or the
# environment variables that set them, fails this step; so does an `include`,
# which makes cargo read further files this check does not follow.
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
configs=("$cargo_home/config" "$cargo_home/config.toml")
d="$root"
while :; do
    configs+=("$d/.cargo/config" "$d/.cargo/config.toml")
    [[ "$d" == / ]] && break
    d="$(dirname "$d")"
done
replaced="$(python3 -I -c '
import os, re, sys, tomllib
for p in sys.argv[1:]:
    try:
        with open(p, "rb") as f:
            config = tomllib.load(f)
    except FileNotFoundError:
        continue
    except Exception as e:
        print(f"{p}: cannot read: {e}")
        continue
    for k in ("source", "patch", "paths", "include"):
        if k in config:
            print(f"{p}: sets {k}")
for k in sorted(os.environ):
    if re.match(r"CARGO_(SOURCE|PATCH|PATHS|INCLUDE)(_|$)", k):
        print(f"the environment sets {k}")
' "${configs[@]}")"
# Only python's own output is a finding; if python3 itself fails (missing, or
# older than 3.11 and without tomllib), the configuration was not checked.
if [[ $? -ne 0 ]]; then
    echo "supply-chain: cannot check the cargo configuration: python3 3.11 or later is needed (for tomllib)"
    failed+=("cargo configuration (not checked)")
elif [[ -n "$replaced" ]]; then
    sed 's/^/supply-chain: crates.io replaced or overridden: /' <<<"$replaced"
    echo "supply-chain: cargo configuration: FAILED (only crates.io code may be built)"
    failed+=("cargo configuration (crates.io replaced)")
fi
if tool_ok cargo-deny; then
    # A crate whose index entry cannot be fetched or read is only a warning to
    # cargo-deny (its yanked check is then skipped): -D makes it an error, so
    # an index failure fails closed. cargo-deny makes a notice advisory an
    # error and has no config key for it: -W notice prints it without failing,
    # as cargo-audit does (the lead's rulings for task baseline-1b, ruling 4).
    if "$TOOLS_DIR/cargo-deny" --workspace --locked --all-features \
        check -D index-failure -D index-cache-load-failure -W notice \
        advisories bans sources; then
        echo "supply-chain: cargo deny: passed"
    else
        echo "supply-chain: cargo deny: FAILED"
        failed+=("cargo deny")
    fi
else
    missing_hint cargo-deny
    failed+=("cargo deny (pinned tool not usable)")
fi

banner "2/3 cargo audit"
if tool_ok cargo-audit; then
    # cargo-audit exits 0 when it cannot check a crate for yanking (it prints
    # "error: couldn't check if the package is yanked"), and when it cannot
    # update or open the crates.io index at all (it prints only "warning:
    # couldn't update/open crates.io index" and checks no crate), so either
    # line fails the check too: an index failure fails closed.
    audit_out="$("$TOOLS_DIR/cargo-audit" audit --color never 2>&1)"
    audit_rc=$?
    printf '%s\n' "$audit_out"
    if [[ $audit_rc -eq 0 ]] && ! grep -Eq "^(error:|warning: couldn't (update|open) crates\.io index)" <<<"$audit_out"; then
        echo "supply-chain: cargo audit: passed"
    else
        echo "supply-chain: cargo audit: FAILED"
        failed+=("cargo audit")
    fi
else
    missing_hint cargo-audit
    failed+=("cargo audit (pinned tool not usable)")
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
