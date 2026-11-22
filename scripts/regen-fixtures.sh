#!/usr/bin/env bash
# Regenerates every generated file under fixtures/ (SYNC_PLAN.md §6.1 item 5;
# fixtures/README.md): the boards, the legacy formats, the snapshots, the
# transcripts and MANIFEST.sha256. fixtures/README.md and fixtures/generator/
# are not generated and are left alone.
#
#   bash scripts/regen-fixtures.sh [<dir>]    default: this repository's fixtures/
#
# Into <dir> instead when given (scripts/check-fixtures.sh uses a temporary
# one). The generator runs twice, each time under the TZ it requires:
# Europe/Berlin, then America/New_York. Honours CARGO_TARGET_DIR.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="${1:-$root/fixtures}"
cd "$root"
cargo build --locked --quiet -p taskdeck-fixtures
mkdir -p "$out"
for generated in boards legacy-main legacy-other snapshots transcripts MANIFEST.sha256; do
    rm -rf -- "${out:?}/$generated"
done
TZ=Europe/Berlin cargo run --locked --quiet -p taskdeck-fixtures -- --zone berlin --out "$out"
TZ=America/New_York cargo run --locked --quiet -p taskdeck-fixtures -- --zone new-york --out "$out"
