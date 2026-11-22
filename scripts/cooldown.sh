#!/usr/bin/env bash
# The seven-day dependency cooldown (SYNC.md §8 "Supply chain"; R4 §4.2).
#
#   bash scripts/cooldown.sh [--base <rev>] [--all] [--now <RFC 3339>]
#                            [--index-dir <dir>] [--lockfile <file>]
#                            [--exemptions <file>] [--base-lockfile <file>]
#
# Compares the working tree's Cargo.lock with Cargo.lock at the base revision
# (default: the merge base of HEAD and origin/day-planner). Every crates.io
# package version that is new must have been published at least 7 days before
# the check, must not be yanked, must be in the index and must have the
# index's checksum (a changed checksum makes a package new); --all checks every
# crates.io package in the lockfile. Exemptions: scripts/cooldown-exemptions.txt.
# --index-dir reads index files laid out like https://index.crates.io/ from a
# local directory, and --base-lockfile compares with a file instead of a
# revision, for offline tests. Prints one line per violation and a
# summary; exits 0 only when clean (1: violations, 2: could not check).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
command -v python3 >/dev/null || { echo "cooldown: error: python3 is needed" >&2; exit 2; }
python3 -I -c 'import tomllib' 2>/dev/null \
    || { echo "cooldown: error: python3 3.11 or later is needed (for tomllib)" >&2; exit 2; }
command -v curl >/dev/null || { echo "cooldown: error: curl is needed" >&2; exit 2; }
exec python3 -I "$here/cooldown.py" "$@"
