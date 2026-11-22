#!/usr/bin/env bash
# Tests for scripts/ci/changes.sh: every path rule, the fall-backs to
# "everything", [ci full], wide and oses, and the git modes on throwaway
# repositories. Offline; touches nothing outside a temporary directory.
set -uo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
CHANGES="$SCRIPTS_DIR/ci/changes.sh"
unset GITHUB_OUTPUT CI_EVENT CI_BEFORE CI_AFTER CI_HEAD_MESSAGE
ALL="root supply foundation core seal engine device recovery relay android"

verdict() {   # verdict <case> <got> <want>
    if [[ "$2" == "$3" ]]; then
        PASSES=$((PASSES + 1)); echo "ok    $1"
    else
        FAILS=$((FAILS + 1)); echo "FAIL  $1: got [$2], wanted [$3]"
    fi
}

# The areas set true in changes.sh's output, in its order, or "none".
on_areas() {
    local got
    got="$(sed -n 's/^\([a-z-]*\)=true$/\1/p' <<<"$1" | grep -vx wide | tr '\n' ' ' | sed 's/ $//')"
    echo "${got:-none}"
}
value_of() { sed -n "s/^$1=//p" <<<"$2"; }

# paths <case> <expected areas> <path>...: test mode, paths on standard input.
# EV and MSG set the event and the head commit message (default: a push).
paths() {
    local name="$1" want="$2" out
    shift 2
    if ! out="$(printf '%s\n' "$@" | CI_EVENT="${EV-push}" CI_HEAD_MESSAGE="${MSG-}" \
        bash "$CHANGES" --paths-from - 2>/dev/null)"; then
        FAILS=$((FAILS + 1)); echo "FAIL  $name: changes.sh exited non-zero"; return
    fi
    LAST="$out"
    verdict "$name" "$(on_areas "$out")" "$want"
}

echo "-- shared files set every area"
for p in Cargo.lock Cargo.toml rust-toolchain.toml .github/workflows/ci.yml \
    .github/workflows/release.yml scripts/ci/changes.sh scripts/ci/README.md \
    .cargo/config.toml .cargo/config .cargo/other/x.toml \
    scripts/dev-setup.sh scripts/wrangler/package.json scripts/wrangler/package-lock.json; do
    paths "shared: $p" "$ALL" "$p"
done

echo "-- root: what the root package compiles, embeds or reads"
for p in src/main.rs src/phone.html src/notes.md resources.rc icon.ico icon.png noback.png \
    1920px-Blue_Marble_2002.png fonts/DejaVuSans.ttf fonts/LICENSES/dejavu.txt \
    images/pexels-francesco-ungaro-1525041.jpg weather_svgs_2/04.svg; do
    paths "root (and so foundation): $p" "root foundation" "$p"
done
paths "root and foundation: build.rs (check-build-date.sh checks it)" "root foundation" build.rs
paths "root: the release-panic check (desktop runs it on every OS)" "root foundation" scripts/check-release-panic.sh
paths "root: the panic probe" "root foundation" crates/panic-probe/src/main.rs
paths "root: the panic probe's manifest" "root supply foundation" crates/panic-probe/Cargo.toml

echo "-- supply"
paths "supply: deny.toml" "supply" deny.toml
paths "supply: .cargo/audit.toml" "supply" .cargo/audit.toml
paths "supply: a member's Cargo.toml" "supply foundation core device recovery android" crates/taskdeck-core/Cargo.toml
paths "supply: a Cargo.toml outside crates/" "supply device relay android" relay/Cargo.toml
paths "supply: scripts/supply-chain.sh" "supply foundation" scripts/supply-chain.sh
paths "supply: scripts/cooldown.sh" "supply foundation" scripts/cooldown.sh
paths "supply: scripts/cooldown.py" "supply foundation" scripts/cooldown.py
paths "supply: scripts/cooldown-exemptions.txt" "supply foundation" scripts/cooldown-exemptions.txt

echo "-- foundation"
paths "foundation: a spec" "foundation" docs/sync/spec/core.md
# The foundation job's spec lint checks the requirement map against both.
paths "foundation: SYNC.md (spec lint reads it)" "foundation" SYNC.md
paths "foundation: SYNC_PLAN.md (spec lint reads it)" "foundation" SYNC_PLAN.md
paths "foundation: fixtures also mean core" "foundation core device recovery android" fixtures/boards/small.json
paths "foundation only: the fixture generator's code" "foundation" fixtures/generator/src/main.rs
paths "foundation only: the fixture generator's case variant" "foundation" Fixtures/Generator/src/main.rs
paths "foundation and supply: the fixture generator's manifest" "supply foundation" fixtures/generator/Cargo.toml
paths "the generator's code next to fixture data marks core again" "foundation core device recovery android" \
    fixtures/generator/src/main.rs fixtures/snapshots/x.json
# Whatever marks root marks foundation: the generator links the root package,
# and the foundation job checks the fixtures against what it writes.
paths "root marks foundation: src/board.rs" "root foundation" src/board.rs
paths "foundation: a script" "foundation" scripts/check-build-date.sh
paths "foundation: a script test" "foundation" scripts/tests/run.sh
paths "foundation: any crate" "foundation" crates/other/src/main.rs
# Clippy and rustfmt read their config from every parent of a crate's manifest.
paths "shared: crates/clippy.toml (every crate below reads it)" "$ALL" crates/clippy.toml
paths "shared: crates/.clippy.toml" "$ALL" crates/.clippy.toml
paths "shared: any file directly under crates/" "$ALL" crates/rustfmt.toml

echo "-- one path per stage area, with what builds on it"
paths "core" "foundation core device recovery android" crates/taskdeck-core/src/lib.rs
paths "seal" "foundation seal device recovery android" crates/taskdeck-seal/src/lib.rs
paths "seal: vectors also mean relay" "seal device recovery relay android" docs/sync/vectors/frame.json
paths "engine, and the recovery tool whose device links it" "foundation engine device recovery android" crates/taskdeck-sync/src/lib.rs
paths "device, and the recovery tool that links it" "foundation device recovery android" crates/taskdeck-device/src/lib.rs
paths "device: e2e tests" "device android" tests/e2e/pairing.rs
paths "recovery: backup, and the device that links it" "foundation device recovery android" crates/taskdeck-backup/src/lib.rs
paths "recovery: recover, and the device" "foundation device recovery android" crates/taskdeck-recover/src/main.rs
paths "relay, and the device whose e2e tests use it" "device relay android" relay/relay-core.js
paths "android" "android" android/app/build.gradle
paths "android: the JNI crate" "foundation android" crates/taskdeck-android/src/lib.rs
paths "android: the check its hook runs" "foundation android" scripts/check-android-targets.sh
paths "android: the forked page" "android" android/page/index.html

echo "-- paths that need no CI"
for p in docs/sync/status/foundation.md docs/sync/status/foundation/briefs/ci.md \
    docs/sync/research/R1.md docs/sync/runbooks/restore.md README.md \
    LICENSE taskdeck_example_main.png .gitignore deploy/install.sh deploy/taskdeck-server.service \
    deploy/NOTES.md; do
    paths "no CI: $p" "none" "$p"
done
paths "docs-only change: nothing but changes and gate" "none" \
    README.md docs/sync/status/foundation.md docs/sync/research/R4.md LICENSE .gitignore deploy/install.sh
paths "no changed path at all" "none"

echo "-- unknown paths are fail safe"
for p in .github/dependabot.yml docs/README.md docs/sync/other.md \
    LICENSE.txt Makefile src.rs tests/other/x.rs scripts deploy .cargo deployment/x.sh; do
    paths "unknown: $p" "$ALL" "$p"
done
paths "a nested .md is not a root .md" "$ALL" tests/NOTES.md
paths "a nested .png is not a root .png" "$ALL" docs/shot.png
paths "docs plus one unknown path" "$ALL" README.md docs/sync/status/x.md deploy/install.sh Makefile
paths "mixed areas add up" "root foundation device relay android" README.md src/ui.rs relay/x.js
paths "a CRLF line still matches" "root foundation" $'src/ui.rs\r'

echo "-- case variants: one file on Windows and macOS file systems"
for p in 1920px-blue_marble_2002.png 1920px-blue_Marble_2002.png ICON.png Noback.PNG Fonts/x.ttf SRC/main.rs; do
    paths "root: the case variant $p" "root foundation" "$p"
done
paths "shared: the case variant SCRIPTS/DEV-SETUP.SH" "$ALL" SCRIPTS/DEV-SETUP.SH
for p in Deploy/install.sh docs/sync/Status/x.md README.MD license; do
    paths "the no-CI list keeps case: $p" "$ALL" "$p"
done

echo "-- wide, oses and [ci full]"
EV=push paths "push: narrow" "root foundation" src/ui.rs
verdict "push: wide=false" "$(value_of wide "$LAST")" "false"
verdict "push: Linux only" "$(value_of oses "$LAST")" '["ubuntu-latest"]'
EV=pull_request paths "pull request: still only what changed" "root foundation" src/ui.rs
verdict "pull request: wide=true" "$(value_of wide "$LAST")" "true"
verdict "pull request: three systems" "$(value_of oses "$LAST")" '["ubuntu-latest","windows-latest","macos-latest"]'
EV=pull_request paths "pull request, docs only: nothing" "none" README.md
EV=push MSG=$'fix the planner\n\n[ci full]' paths "[ci full]: everything" "$ALL" README.md
verdict "[ci full]: wide" "$(value_of wide "$LAST")" "true"
verdict "[ci full]: three systems" "$(value_of oses "$LAST")" '["ubuntu-latest","windows-latest","macos-latest"]'
EV=push MSG='[ci full] first' paths "[ci full] at the start" "$ALL" README.md
EV=push MSG='[CI FULL]' paths "[CI FULL] is [ci full] (any case, as GitHub's contains())" "$ALL" README.md
EV=push MSG='Fix the planner [CI full]' paths "[CI full] is [ci full]" "$ALL" src/ui.rs
verdict "[CI full]: wide" "$(value_of wide "$LAST")" "true"
EV=push MSG='ci full' paths "ci full without brackets is not [ci full]" "none" README.md
EV=push MSG='$(touch '"$TMP"'/pwned) `touch '"$TMP"'/pwned2`' paths "a hostile message is data" "none" README.md
verdict "the hostile message ran nothing" "$(ls "$TMP" | grep -c pwned)" "0"

echo "-- outputs"
out_file="$TMP/github_output"
: >"$out_file"
stdout="$(printf 'relay/x.js\n' | CI_EVENT=push GITHUB_OUTPUT="$out_file" bash "$CHANGES" --paths-from - 2>/dev/null)"
verdict "GITHUB_OUTPUT gets exactly the printed outputs" "$(cat "$out_file")" "$stdout"
verdict "twelve outputs" "$(wc -l <"$out_file" | tr -d ' ')" "12"
verdict "outputs in order" "$(cut -d= -f1 "$out_file" | tr '\n' ' ')" "$ALL wide oses "
printf 'src/ui.rs\n' >"$TMP/paths.txt"
expect "--paths-from <file>" 0 env CI_EVENT=push bash "$CHANGES" --paths-from "$TMP/paths.txt"
output_has "--paths-from <file> reads the file" "root=true"
expect "an unreadable paths file is an error" 2 bash "$CHANGES" --paths-from "$TMP/nope.txt"
expect "an unknown argument is an error" 2 bash "$CHANGES" --frobnicate
expect "--paths-from without a value is an error" 2 bash "$CHANGES" --paths-from

echo "-- git modes, on throwaway repositories"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
g() { git -C "$REPO" -c user.name=test -c user.email= -c commit.gpgsign=false "$@"; }
REPO="$TMP/repo"
git init -q -b day-planner "$REPO"
echo one >"$REPO/README.md"
g add -A && g commit -qm one
A="$(g rev-parse HEAD)"
mkdir -p "$REPO/src" && echo 'fn a() {}' >"$REPO/src/a.rs"
g add -A && g commit -qm two
B="$(g rev-parse HEAD)"
echo two >>"$REPO/README.md"
g add -A && g commit -qm three
C="$(g rev-parse HEAD)"

# git_case <case> <expected areas> <env...>: CI mode in $REPO.
git_case() {
    local name="$1" want="$2" out
    shift 2
    if ! out="$(cd "$REPO" && env "$@" bash "$CHANGES" 2>/dev/null)"; then
        FAILS=$((FAILS + 1)); echo "FAIL  $name: changes.sh exited non-zero"; return
    fi
    LAST="$out"
    verdict "$name" "$(on_areas "$out")" "$want"
}
git_case "push: the diff before..after" "root foundation" CI_EVENT=push CI_BEFORE="$A" CI_AFTER="$B"
git_case "push: after defaults to HEAD" "root foundation" CI_EVENT=push CI_BEFORE="$A"
git_case "push: a docs-only push runs nothing" "none" CI_EVENT=push CI_BEFORE="$B" CI_AFTER="$C"
git_case "push: before all zeros means everything" "$ALL" \
    CI_EVENT=push CI_BEFORE=0000000000000000000000000000000000000000 CI_AFTER="$C"
git_case "push: no before means everything" "$ALL" CI_EVENT=push CI_AFTER="$C"
git_case "push: a before not in the clone means everything" "$ALL" \
    CI_EVENT=push CI_BEFORE=1234567890abcdef1234567890abcdef12345678 CI_AFTER="$C"
git_case "push: a before that is not a revision means everything" "$ALL" \
    CI_EVENT=push CI_BEFORE='--output=x' CI_AFTER="$C"
git_case "push: a ref name instead of a commit id means everything" "$ALL" \
    CI_EVENT=push CI_BEFORE=day-planner CI_AFTER="$C"
git_case "push: an after not in the clone means everything" "$ALL" \
    CI_EVENT=push CI_BEFORE="$A" CI_AFTER=1234567890abcdef1234567890abcdef12345678
git_case "push: [ci full] on a docs-only push" "$ALL" \
    CI_EVENT=push CI_BEFORE="$B" CI_AFTER="$C" CI_HEAD_MESSAGE='docs [ci full]'
verdict "push: [ci full] is wide" "$(value_of wide "$LAST")" "true"
git_case "no event means everything" "$ALL" CI_BEFORE="$A" CI_AFTER="$B"
git_case "another event means everything" "$ALL" CI_EVENT=workflow_dispatch CI_BEFORE="$A"

# A move out of src/ into a path that needs no CI still runs root (both names count).
g mv src/a.rs docs-a.md 2>/dev/null || mv "$REPO/src/a.rs" "$REPO/docs-a.md"
g add -A && g commit -qm move
D="$(g rev-parse HEAD)"
git_case "push: a rename counts its old path too" "root foundation" CI_EVENT=push CI_BEFORE="$C" CI_AFTER="$D"
# Odd file names are neither quoted nor split.
mkdir -p "$REPO/src/odd dir" && echo x >"$REPO/src/odd dir/ü näme.rs"
g add -A && g commit -qm odd
E="$(g rev-parse HEAD)"
git_case "push: a path with spaces and non-ASCII letters" "root foundation" CI_EVENT=push CI_BEFORE="$D" CI_AFTER="$E"

# Pull requests: the checkout is the merge commit, base first.
g checkout -q -b feature "$C"
mkdir -p "$REPO/relay" && echo x >"$REPO/relay/x.js"
g add -A && g commit -qm feature
g checkout -q day-planner
g merge -q --no-ff -m merge feature
git_case "pull request: the merge commit's changes against its base" "device relay android" CI_EVENT=pull_request
verdict "pull request: wide" "$(value_of wide "$LAST")" "true"
g checkout -q feature
git_case "pull request: a HEAD with one parent means everything" "$ALL" CI_EVENT=pull_request

# A file name with a newline must not put a workflow command ("::add-mask::true",
# which would blank the job outputs and skip every job) at the start of a log
# line, and such a name means everything.
no_command_lines() {   # no_command_lines <case> <env...>: CI mode in $REPO
    local name="$1" err
    shift
    err="$(cd "$REPO" && env "$@" bash "$CHANGES" 2>&1 >/dev/null)"
    if grep -q '^::' <<<"$err"; then
        FAILS=$((FAILS + 1)); echo "FAIL  $name: a log line starts with '::'"; sed 's/^/        /' <<<"$err"
    else
        PASSES=$((PASSES + 1)); echo "ok    $name"
    fi
}
g checkout -q -b hostile day-planner
mkdir -p "$REPO/docs/sync/status" && echo x >"$REPO/docs/sync/status/note"$'\n''::add-mask::true'
echo 'fn b() {}' >"$REPO/src/b.rs"
g add -A && g commit -qm hostile
g checkout -q day-planner
g merge -q --no-ff -m merge hostile
git_case "pull request: a path with a newline means everything" "$ALL" CI_EVENT=pull_request
no_command_lines "pull request: a path with a newline prints no workflow command" CI_EVENT=pull_request
no_command_lines "push: a before with a newline prints no workflow command" \
    CI_EVENT=push CI_BEFORE=$'x\n::add-mask::true'
paths "test mode: a path with a control character means everything" "$ALL" $'docs/sync/status/a\033b'

finish
