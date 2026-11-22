#!/usr/bin/env bash
# scripts/check-fixtures.sh passes on the committed fixtures and fails on a
# changed, a stray and a missing file; the generator refuses a TZ other than
# the one its set is written in. Builds the generator (a warm target dir makes
# it quick); honours CARGO_TARGET_DIR.
set -uo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

expect "check-fixtures passes on the committed fixtures" 0 bash "$SCRIPTS_DIR/check-fixtures.sh"
output_has "it says so" "fixtures/ is exactly what the generator writes"

cp -R "$REPO_DIR/fixtures" "$TMP/changed"
printf 'x' >>"$TMP/changed/transcripts/europe-berlin/seeded-01.jsonl"
expect "a changed file fails" 1 bash "$SCRIPTS_DIR/check-fixtures.sh" "$TMP/changed"
output_has "and is named" "fixtures/transcripts/europe-berlin/seeded-01.jsonl differs"

cp -R "$REPO_DIR/fixtures" "$TMP/stray"
printf '{}\n' >"$TMP/stray/boards/small/stray.json"
expect "a file the generator does not write fails" 1 bash "$SCRIPTS_DIR/check-fixtures.sh" "$TMP/stray"
output_has "and is named" "fixtures/boards/small/stray.json is not written by the generator"

cp -R "$REPO_DIR/fixtures" "$TMP/missing"
rm "$TMP/missing/boards/unreadable/archived.jsonl"
expect "a missing file fails" 1 bash "$SCRIPTS_DIR/check-fixtures.sh" "$TMP/missing"
output_has "and is named" "fixtures/boards/unreadable/archived.jsonl is missing"

expect "the generator refuses another TZ" 1 \
    env TZ=UTC cargo run --locked --quiet --manifest-path "$REPO_DIR/Cargo.toml" -p taskdeck-fixtures -- --zone berlin --out "$TMP/out"
output_has "and says which it wants" "TZ=Europe/Berlin"

finish
