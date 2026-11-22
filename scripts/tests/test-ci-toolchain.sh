#!/usr/bin/env bash
# Tests for scripts/ci/toolchain.sh: its reading of rust-toolchain.toml, the
# rustup command it builds, and the check that rustc reports the pinned
# channel. Offline; nothing is installed. (The script's real run is in CI.)
set -uo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
TOOLCHAIN="$SCRIPTS_DIR/ci/toolchain.sh"
# shellcheck source=../ci/toolchain.sh
source "$TOOLCHAIN"
set +e   # the sourced script turned errexit on; these tests count failures instead

verdict() {
    if [[ "$2" == "$3" ]]; then
        PASSES=$((PASSES + 1)); echo "ok    $1"
    else
        FAILS=$((FAILS + 1)); echo "FAIL  $1: got [$2], wanted [$3]"
    fi
}
parsed() { echo "$TC_CHANNEL|$TC_PROFILE|${TC_COMPONENTS[*]+${TC_COMPONENTS[*]}}|${TC_TARGETS[*]+${TC_TARGETS[*]}}"; }

echo "-- the repository's own file"
parse_toolchain_file "$REPO_DIR/rust-toolchain.toml"
verdict "channel is read" "$TC_CHANNEL" "$(sed -n 's/^channel *= *"\(.*\)".*/\1/p' "$REPO_DIR/rust-toolchain.toml")"
verdict "clippy is a component" "$(printf '%s\n' "${TC_COMPONENTS[@]}" | grep -cx clippy)" "1"
verdict "every target line is read" "${#TC_TARGETS[@]}" \
    "$(sed -n '/^targets/,/]/p' "$REPO_DIR/rust-toolchain.toml" | grep -o '"[^"]*"' | wc -l | tr -d ' ')"

echo "-- other shapes of the file"
cat >"$TMP/a.toml" <<'EOF'
# a comment
[toolchain]
channel = "1.90.1"   # trailing comment
profile = "minimal"
components = [ "clippy", "rustfmt" ]
targets = [
    "wasm32-unknown-unknown", # one
    "x86_64-linux-android",
]
EOF
parse_toolchain_file "$TMP/a.toml"
verdict "one-line and multi-line arrays, comments" "$(parsed)" \
    "1.90.1|minimal|clippy rustfmt|wasm32-unknown-unknown x86_64-linux-android"
printf '[toolchain]\r\nchannel = "1.91.0"\r\ncomponents = ["clippy"]\r\n' >"$TMP/crlf.toml"
parse_toolchain_file "$TMP/crlf.toml"
verdict "CRLF line ends" "$(parsed)" "1.91.0||clippy|"
printf '[other]\nchannel = "nightly"\n[toolchain]\nchannel = "1.92.0"\n' >"$TMP/tables.toml"
parse_toolchain_file "$TMP/tables.toml"
verdict "only the [toolchain] table counts" "$TC_CHANNEL" "1.92.0"
printf '[toolchain]\nprofile = "minimal"\n' >"$TMP/nochannel.toml"
expect "no channel is an error" 1 bash -c 'source "$1"; parse_toolchain_file "$2"' _ "$TOOLCHAIN" "$TMP/nochannel.toml"
printf '[toolchain]\nchannel = "1.0; rm -rf /"\n' >"$TMP/bad.toml"
expect "a channel with odd characters is an error" 1 bash -c 'source "$1"; parse_toolchain_file "$2"' _ "$TOOLCHAIN" "$TMP/bad.toml"
printf '[toolchain]\nchannel = "1.93.0"\ntargets = ["a",\n' >"$TMP/open.toml"
expect "an unclosed array is an error" 1 bash -c 'source "$1"; parse_toolchain_file "$2"' _ "$TOOLCHAIN" "$TMP/open.toml"
expect "a missing file is an error" 1 bash -c 'source "$1"; parse_toolchain_file "$2"' _ "$TOOLCHAIN" "$TMP/none.toml"

echo "-- the rustup command"
parse_toolchain_file "$TMP/a.toml"
verdict "install arguments" "$(install_args | tr '\n' ' ')" \
    "toolchain install 1.90.1 --no-self-update --profile minimal --component clippy --component rustfmt --target wasm32-unknown-unknown --target x86_64-linux-android "
expect "--dry-run changes nothing and names the pinned toolchain" 0 bash "$TOOLCHAIN" --dry-run
output_has "dry run: the install command" "rustup toolchain install $(sed -n 's/^channel *= *"\(.*\)".*/\1/p' "$REPO_DIR/rust-toolchain.toml") --no-self-update"
output_lacks "never rustup default" "rustup default"
output_lacks "never rustup update" "rustup update"
expect "an unknown option is an error" 2 bash "$TOOLCHAIN" --frobnicate
verdict "the script never runs rustup default or update" \
    "$(grep -v '^ *#' "$TOOLCHAIN" | grep -cE 'rustup +(default|update)\b')" "0"

echo "-- rustc must report the pinned channel"
m() { if rustc_matches_channel "$1" "$2"; then echo yes; else echo no; fi; }
verdict "1.97.0 matches rustc 1.97.0" "$(m 1.97.0 'rustc 1.97.0 (2d8144b78 2026-07-07)')" yes
verdict "1.97.0 does not match rustc 1.97.1" "$(m 1.97.0 'rustc 1.97.1 (abc 2026-08-01)')" no
verdict "1.97.0 does not match rustc 1.97.01" "$(m 1.97.0 'rustc 1.97.01 (abc)')" no
verdict "1.9.0 does not match rustc 1.90.0" "$(m 1.9.0 'rustc 1.90.0 (abc)')" no
verdict "1.97.0 does not match a nightly 1.97.0" "$(m 1.97.0 'rustc 1.97.0-nightly (abc)')" no
verdict "1.97 matches rustc 1.97.2" "$(m 1.97 'rustc 1.97.2 (abc)')" yes
verdict "1.97 does not match rustc 1.971.0" "$(m 1.97 'rustc 1.971.0 (abc)')" no
verdict "stable matches a release" "$(m stable 'rustc 1.97.0 (abc)')" yes
verdict "stable does not match a nightly" "$(m stable 'rustc 1.98.0-nightly (abc)')" no
verdict "nightly-2026-01-01 matches a nightly" "$(m nightly-2026-01-01 'rustc 1.95.0-nightly (abc)')" yes
verdict "an empty answer matches nothing" "$(m 1.97.0 '')" no

finish
