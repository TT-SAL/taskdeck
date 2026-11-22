#!/usr/bin/env bash
# scripts/check-fixtures.sh passes on the committed fixtures and fails on a
# changed, a stray, a symlinked and a missing file; regen-fixtures.sh writes a
# relative <dir> under the caller's directory; an autocrlf checkout keeps the
# bytes; the generator refuses a TZ other than the one its set is written in. Builds the generator (a warm target dir makes
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

cp -R "$REPO_DIR/fixtures" "$TMP/link"
ln -s ../small/read_at_startup.json "$TMP/link/boards/empty/stray.json"
expect "an extra symlink fails" 1 bash "$SCRIPTS_DIR/check-fixtures.sh" "$TMP/link"
output_has "and is named" "fixtures/boards/empty/stray.json is not a regular file"

cp -R "$REPO_DIR/fixtures" "$TMP/swapped"
mv "$TMP/swapped/boards/small/notepad_text.json" "$TMP/swapped/notepad-copy"
ln -s ../../notepad-copy "$TMP/swapped/boards/small/notepad_text.json"
expect "a file replaced by a symlink to the same bytes fails" 1 bash "$SCRIPTS_DIR/check-fixtures.sh" "$TMP/swapped"
output_has "and is named" "fixtures/boards/small/notepad_text.json is not a regular file"

cp -R "$REPO_DIR/fixtures" "$TMP/missing"
rm "$TMP/missing/boards/unreadable/archived.jsonl"
expect "a missing file fails" 1 bash "$SCRIPTS_DIR/check-fixtures.sh" "$TMP/missing"
output_has "and is named" "fixtures/boards/unreadable/archived.jsonl is missing"

mkdir "$TMP/caller"
expect "regen-fixtures writes a relative <dir> under the caller's directory, not the repository" 0 \
    bash -c 'cd "$1" && bash "$2/regen-fixtures.sh" rel && test -f rel/MANIFEST.sha256 && test ! -e "$3/rel"' \
    _ "$TMP/caller" "$SCRIPTS_DIR" "$REPO_DIR"

# fixtures/.gitattributes keeps the bytes on a checkout that converts line
# endings (core.autocrlf=true, Git's default on Windows).
mkdir "$TMP/crlf"
cp -R "$REPO_DIR/fixtures" "$TMP/crlf/"
expect "an autocrlf checkout gives the committed bytes" 0 bash -c '
    cd "$1" && git init -q && git add -A && rm -rf fixtures &&
    git -c core.autocrlf=true checkout-index -a && diff -r "$2/fixtures" fixtures' _ "$TMP/crlf" "$REPO_DIR"

expect "the generator refuses another TZ" 1 \
    env TZ=UTC cargo run --locked --quiet --manifest-path "$REPO_DIR/Cargo.toml" -p taskdeck-fixtures -- --zone berlin --out "$TMP/out"
output_has "and says which it wants" "TZ=Europe/Berlin"

finish
