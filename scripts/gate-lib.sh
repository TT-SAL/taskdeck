#!/usr/bin/env bash
# Checks the stage gates (scripts/gates/*.gate) call by name. Each prints what
# it found and exits 0 (clean), 1 (found something) or 2 (usage error).
#
#   bash scripts/gate-lib.sh [--root <dir>] <check> [<args>]
#
#   owned-paths <stage>        the stage's owned paths (SYNC_PLAN.md §4), as below
#   no-unimplemented <stage>   no unimplemented!( , todo!( or unreachable!("TODO in them
#   no-skipped-tests <stage>   no skipped or ignored test in them (the list below)
#   workspace-sanity           cargo metadata --locked works and the workspace has
#                              the root package and every crate of §4
#   clippy-config <clippy.toml> <required list>
#                              every `<table> <path>` of the list is still disallowed,
#                              and the crate of <clippy.toml> turns none of those
#                              lints off (allow/expect, [lints], .cargo rustflags)
#   vectors <dir>              test vectors present and well formed (spec README §7)
#   npm-no-deps <package.json> no runtime dependency (§3.4, §6.4)
#   removed-modules            no reference left to what `desktop` removes (§6.9)
#
# --root runs the check on another tree (the tests use it); the default is this
# repository. Paths in the output are relative to the root.
set -uo pipefail

# ---------------------------------------------------------------------------
# Owned paths per stage, from SYNC_PLAN.md §4. Where §4 lists two stages for a
# path ("ownership passes when the first one merges"), the path is in both
# stages' lists: the stricter reading, since the later stage's gate runs after
# the hand-over. `desktop` also gets its test files of spec-coverage.sh
# (tests/ without tests/e2e). `!` removes a path (a directory or a file).
# `android-shell` leaves out crates/taskdeck-android/src/device_backend.rs,
# which holds what only the `backend-device` flavour compiles: that is stage
# `android`'s to fill (lead ruling 10 of the tooling task), and `android`'s
# list keeps it. `rc` may touch any path; its
# scans leave out docs/, which holds records and data, not code (the workflow
# records under docs/sync/status/ are .js files that quote the patterns).
# ---------------------------------------------------------------------------
declare -A STAGE_OWNED=(
    [foundation]="docs/sync/spec fixtures scripts .github/workflows/ci.yml Cargo.toml build.rs"
    [core]="crates/taskdeck-core"
    [seal]="crates/taskdeck-seal docs/sync/vectors"
    [relay]="relay"
    [engine]="crates/taskdeck-sync"
    [android-shell]="android crates/taskdeck-android !crates/taskdeck-android/src/device_backend.rs"
    [device]="crates/taskdeck-device tests/e2e crates/taskdeck-sync crates/taskdeck-backup crates/taskdeck-recover"
    [recovery]="crates/taskdeck-backup crates/taskdeck-recover"
    [desktop]="src tests !tests/e2e"
    [android]="android crates/taskdeck-android"
    [release]=".github/workflows/release.yml .github/workflows/nightly.yml scripts/release scripts/mac-bundle.sh docs/sync/runbooks *.md"
    [rc]=". !docs"
)

# The scans (gate_checks.py scan) read source files only: Rust, JavaScript and
# TypeScript, the page's HTML, Kotlin and Java. Markdown, shell and Python
# files are not scanned, so docs and these scripts may name the patterns. Build
# output is not scanned (speclib.is_output_dir). The patterns are Python regular
# expressions matched against whole files, so `\s` also spans line ends and a
# macro, an attribute or an option split over several lines is found.
# SYNC_PLAN.md §3.3 and §6.1 item 4: bodies may be unimplemented!() in
# foundation "and nowhere later". Any spelling of the macros: `todo ! ()`,
# `todo!{}`, `todo![]`, a path before them, and the parts on separate lines.
UNIMPLEMENTED_RE='\b(unimplemented|todo)\s*!\s*[({\[]|\bunreachable\s*!\s*[({\[]\s*"TODO'
# §3.3: "No skipped, ignored or loosened tests". Rust: #[ignore] and #![ignore]
# in any spacing, and an ignore inside #[cfg_attr(...)]. JavaScript (node:test
# and the like): any .skip() or .todo() called with no argument or a string,
# whatever its receiver (Rust's Iterator::skip takes a number, so `it.skip(1)`
# in Rust passes), and in JavaScript and HTML files any it/test/describe/suite
# .skip( or .todo( whatever its argument (a title held in a variable, as a
# table-driven suite writes it); ['skip'] or ['todo'] on it/test/describe/suite
# or on a test context named t, ctx or context; xit/xtest/xdescribe; and a skip or todo key (plain, quoted or shorthand) in an
# options object of a test call, that is an object before the callback (the
# text before its brace holds no => and no function). Kotlin and Java: @Ignore,
# @Disabled and its @DisabledOn... forms, with or without their package,
# assumeTrue(false) and assumeFalse(true), with or without a message first.
# \x27 is ', \x60 is `.
SKIPPED_RE='#\s*!?\s*\[\s*ignore\b|#\s*!?\s*\[\s*cfg_attr\b[^\]]*\bignore\b'
SKIPPED_RE+='|\b(it|test|describe|suite|t|ctx|context)\s*\[\s*[\x27"\x60](skip|todo)[\x27"\x60]\s*\]'
SKIPPED_RE+='|\.\s*(skip|todo)\s*\(\s*[)\x27"\x60]|\bx(it|test|describe)\s*\('
SKIPPED_RE+='|\b(it|test|describe|suite)\s*\((?:\x27[^\x27\n]*\x27|"[^"\n]*"|\x60[^\x60]*\x60|\([^()]*\)|(?!function\b)[^(){}=>\x27"\x60])*'
SKIPPED_RE+='\{(?:[^}]*?,)?\s*[\x27"]?(skip|todo)[\x27"]?\s*[:,}]'
SKIPPED_RE+='|@(?:[\w.]+\.)?(?:Ignore\b|Disabled)'
SKIPPED_RE+='|\bassumeTrue\s*\((?:\s*"[^"]*"\s*,)?\s*false\b|\bassumeFalse\s*\((?:\s*"[^"]*"\s*,)?\s*true\b'
SKIPPED_JS_RE='\b(it|test|describe|suite)\s*\.\s*(skip|todo)\s*\('
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$here")"

usage() { sed -n '5,21p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
need_stage() {
    [[ -n "${1:-}" ]] || usage
    [[ -n "${STAGE_OWNED[$1]+x}" ]] || { echo "gate-lib: unknown stage '$1'" >&2; exit 2; }
}

# owned <stage>: prints "include" lines and "!exclude" lines, globs expanded
# against the root, missing paths kept (the scan reports them).
owned() {
    local p
    for p in ${STAGE_OWNED[$1]}; do
        if [[ "$p" == *'*'* && "$p" != '!'* ]]; then
            ( cd "$ROOT" && compgen -G "$p" ) || true
        else
            echo "$p"
        fi
    done
}

# scan <name> <stage> <regex> [<regex for JavaScript and HTML files too>]
scan() {
    local name="$1" stage="$2" re="$3" p
    local -a js=()
    [[ -n "${4:-}" ]] && js=(--js "$4")
    local -a inc=() exc=() missing=()
    while IFS= read -r p; do
        if [[ "$p" == '!'* ]]; then exc+=("${p#!}")
        elif [[ -e "$ROOT/$p" ]]; then inc+=("$p")
        else missing+=("$p")
        fi
    done < <(set -f; owned "$stage")
    if [[ ${#missing[@]} -gt 0 ]]; then
        echo "$name $stage: not present yet: ${missing[*]}"
    fi
    if [[ ${#inc[@]} -eq 0 ]]; then
        echo "$name $stage: ok (no owned path exists yet)"
        return 0
    fi
    local hits
    hits="$(checks scan "$re" ${js[@]+"${js[@]}"} "${inc[@]}" ${exc[@]+"${exc[@]/#/!}"})"
    case $? in
        0) ;;
        1)  printf '%s\n' "$hits"
            echo "$name $stage: FAILED: $(grep -c '' <<<"$hits") found in ${inc[*]}${exc[*]+ (without ${exc[*]})}"
            return 1 ;;
        *)  echo "$name $stage: FAILED: the scan did not run"
            return 1 ;;
    esac
    echo "$name $stage: ok (none in ${inc[*]}${exc[*]+ without ${exc[*]}})"
}

checks() { (cd "$ROOT" && python3 -I -B "$here/gate_checks.py" "$@"); }

workspace_sanity() {
    local meta
    meta="$(cd "$ROOT" && cargo metadata --locked --format-version 1 --no-deps)" \
        || { echo "workspace-sanity: FAILED: cargo metadata --locked failed (is Cargo.lock out of date?)"; return 1; }
    printf '%s' "$meta" | checks workspace-sanity
}

# What `desktop` removes (SYNC_PLAN.md §6.9 item 5): the taskdeck-server
# binary, the phone server, src/phone.html and its service worker, sync.rs,
# tiny_http, qrcode, and the fixture generator.
REMOVED_FILES="src/phone.rs src/phone.html src/phone_sw.js src/sync.rs src/server_main.rs fixtures/generator"
REMOVED_ANYWHERE_RE='taskdeck-server|phone\.html|phone_sw\.js|tiny_http|\bqrcode\b|fixtures/generator|taskdeck-fixtures'
REMOVED_ROOT_RE='\bmod (phone|sync|server_main)\b|\b(crate|super|task_deck)::(phone|sync)\b|phone\.rs|\bsync\.rs\b|server_main'

removed_modules() {
    local bad=0 f files
    for f in $REMOVED_FILES; do
        if [[ -e "$ROOT/$f" ]]; then echo "removed-modules: $f still exists"; bad=1; fi
    done
    # Every tracked file (every file when the root is not a git checkout),
    # except documentation (docs/ and Markdown, which may tell the history)
    # and the gate's own files, which name what they look for.
    if git -C "$ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        files="$(git -C "$ROOT" ls-files)"
    else
        files="$(cd "$ROOT" && find . -type d \( -name target -o -name node_modules -o -name .git \) -prune -o -type f -print | sed 's|^\./||')"
    fi
    local -a list=()
    while IFS= read -r f; do
        case "$f" in
            ''|docs/*|*.md|scripts/gate-lib.sh|scripts/gates/*|scripts/tests/test-gate-lib.sh) continue ;;
        esac
        [[ -f "$ROOT/$f" ]] && list+=("$f")
    done <<<"$files"
    local hits=""
    if [[ ${#list[@]} -gt 0 ]]; then
        hits="$(cd "$ROOT" && printf '%s\0' "${list[@]}" | xargs -0 grep -nIE -- "$REMOVED_ANYWHERE_RE" 2>/dev/null)"
        local -a rootfiles=()
        for f in "${list[@]}"; do
            case "$f" in src/*|build.rs|Cargo.toml) rootfiles+=("$f") ;; esac
        done
        if [[ ${#rootfiles[@]} -gt 0 ]]; then
            local h2
            h2="$(cd "$ROOT" && grep -nIHE -- "$REMOVED_ROOT_RE" "${rootfiles[@]}" 2>/dev/null)"
            [[ -n "$h2" ]] && hits+="${hits:+$'\n'}$h2"
        fi
    fi
    if [[ -n "$hits" ]]; then
        printf '%s\n' "$hits" | sort -u
        bad=1
    fi
    if [[ $bad -ne 0 ]]; then
        echo "removed-modules: FAILED: references to removed modules remain"
        return 1
    fi
    echo "removed-modules: ok"
}

main() {
    if [[ "${1:-}" == --root ]]; then
        [[ $# -ge 2 ]] || usage
        ROOT="$(cd "$2" && pwd)" || exit 2
        shift 2
    fi
    local check="${1:-}"
    [[ $# -gt 0 ]] && shift
    case "$check" in
        owned-paths) need_stage "${1:-}"; (set -f; owned "$1") ;;
        no-unimplemented) need_stage "${1:-}"; scan no-unimplemented "$1" "$UNIMPLEMENTED_RE" ;;
        no-skipped-tests) need_stage "${1:-}"; scan no-skipped-tests "$1" "$SKIPPED_RE" "$SKIPPED_JS_RE" ;;
        workspace-sanity) workspace_sanity ;;
        clippy-config|vectors|npm-no-deps) checks "$check" "$@" ;;
        removed-modules) removed_modules ;;
        *) usage ;;
    esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
