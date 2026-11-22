#!/usr/bin/env bash
# Decides which areas of the repository a CI run must test (the `changes` job
# of .github/workflows/ci.yml; SYNC_PLAN.md §3.2 and §6.1; scripts/ci/README.md).
#
#   bash scripts/ci/changes.sh                       CI mode: the changed paths come from git
#   bash scripts/ci/changes.sh --paths-from <file>   test mode: one changed path per line
#   bash scripts/ci/changes.sh --paths-from -        test mode: the paths on standard input
#
# Inputs are environment variables, never shell text built from event data:
#   CI_EVENT         push or pull_request (github.event_name)
#   CI_BEFORE        push only: the commit before the push (github.event.before)
#   CI_AFTER         push only: the commit after the push (github.sha); default HEAD
#   CI_HEAD_MESSAGE  push only: the head commit's message (may be empty)
#
# Outputs, one key=value per line on standard output, and appended to
# $GITHUB_OUTPUT when that is set:
#   root supply foundation core seal engine device recovery relay android
#                    true or false: does that area need CI
#   wide             true on pull_request, and on a push whose head commit
#                    message contains "[ci full]"
#   oses             the JSON array the per-stage matrices run on
# A human-readable account of the decision goes to standard error.
#
# Changed paths in CI mode:
#   pull_request  git diff HEAD^1 HEAD: the default checkout is the merge
#                 commit, whose first parent is the base. If HEAD does not have
#                 two parents, "everything".
#   push          git diff <before> <after>. If <before> is all zeros, is not a
#                 commit id in the clone, or the diff fails for any reason,
#                 "everything".
#   other events  "everything".
# A head commit message containing "[ci full]", in any case, means "everything" too.
# "Everything" sets every area true. So does a changed path that no rule below
# knows, or that holds a control character such as a newline (fail safe).
# In test mode git is not used at all.
set -euo pipefail

AREAS=(root supply foundation core seal engine device recovery relay android)
ALL_OSES='["ubuntu-latest","windows-latest","macos-latest"]'
LINUX_OS='["ubuntu-latest"]'

declare -A ON=()
for a in "${AREAS[@]}"; do ON[$a]=false; done
EVERYTHING=""   # the reason, once "everything" has been decided
HIT=0           # did the path being classified match an area rule

# The runner reads workflow commands ("::add-mask::...") from any line of a
# step's output, and a changed path can hold a newline: never print a control
# character.
say() { local m="$*"; echo "changes: ${m//[[:cntrl:]]/?}" >&2; }

everything() {
    if [[ -z "$EVERYTHING" ]]; then
        EVERYTHING="$1"
        say "everything: $1"
    fi
}

mark() { ON[$1]=true; HIT=1; }

# Paths that set every area true, because every job reads them. Under .cargo/,
# only audit.toml is the supply area's alone: any other file there (a
# .cargo/config.toml) changes every build. scripts/dev-setup.sh installs the
# tools of the supply-chain job and of every stage's ci.sh hook (stage.sh).
# A file directly under crates/ (crates/clippy.toml) is read by tools from
# every crate below it, whatever the crate's area.
is_shared() {
    case "$1" in
        .cargo/audit.toml | crates/*/*) return 1 ;;
        crates/*) return 0 ;;
        Cargo.lock | Cargo.toml | rust-toolchain.toml | .github/workflows/* | scripts/ci/* | .cargo/* \
            | scripts/dev-setup.sh) return 0 ;;
    esac
    return 1
}

# Paths that need no CI at all, when no area rule matched them first. Matched
# case-sensitively, unlike the area rules.
is_no_ci() {
    shopt -u nocasematch
    case "$1" in
        docs/sync/status/* | docs/sync/research/* | docs/sync/runbooks/*) return 0 ;;
        deploy/*) return 0 ;;     # today's server's install script and unit file; no job reads them
        LICENSE | .gitignore) return 0 ;;
        */*) return 1 ;;          # below: files at the repository root only
        *.md | *.png) return 0 ;;
    esac
    return 1
}

# classify <path>: marks the areas the path belongs to.
classify() {
    local p="$1"
    if [[ "$p" == *[[:cntrl:]]* ]]; then
        everything "a path holds a control character: $p"
        return
    fi
    # The area rules ignore case: on Windows and macOS file systems a case
    # variant (ICON.png) is the same file as the path a rule names (icon.png).
    shopt -s nocasematch
    if is_shared "$p"; then
        shopt -u nocasematch
        everything "shared file $p"
        return
    fi
    HIT=0
    # root: what the root package compiles, embeds (include_bytes!, include_str!,
    # egui::include_image!, resources.rc) or reads in its tests.
    # Also the release-panic check and its probe: the desktop job runs that
    # check on every OS.
    case "$p" in
        src/* | build.rs | resources.rc | icon.ico | icon.png | noback.png | 1920px-Blue_Marble_2002.png \
            | fonts/* | images/* | weather_svgs_2/* \
            | scripts/check-release-panic.sh | crates/panic-probe/*) mark root ;;
    esac
    case "$p" in
        Cargo.lock | Cargo.toml | */Cargo.toml | deny.toml | .cargo/audit.toml \
            | scripts/supply-chain.sh | scripts/cooldown*) mark supply ;;
    esac
    case "$p" in
        # build.rs too: the foundation job's check-build-date.sh checks it.
        docs/sync/spec/* | fixtures/* | scripts/* | crates/* | build.rs) mark foundation ;;
    esac
    case "$p" in crates/taskdeck-core/* | fixtures/*) mark core ;; esac
    case "$p" in crates/taskdeck-seal/* | docs/sync/vectors/*) mark seal ;; esac
    case "$p" in crates/taskdeck-sync/*) mark engine ;; esac
    # relay too: the relay's suite takes seal's published vectors.
    case "$p" in relay/* | docs/sync/vectors/*) mark relay ;; esac
    case "$p" in crates/taskdeck-device/* | tests/e2e/*) mark device ;; esac
    # The device too, and the engine it links: the recovery tool's online steps
    # link the device runtime (device.md §15.8).
    case "$p" in
        crates/taskdeck-backup/* | crates/taskdeck-recover/* | crates/taskdeck-device/* | crates/taskdeck-sync/*) mark recovery ;;
    esac
    case "$p" in android/* | crates/taskdeck-android/*) mark android ;; esac
    shopt -u nocasematch

    if [[ $HIT -eq 0 ]]; then
        if is_no_ci "$p"; then
            say "no CI needed: $p"
        else
            everything "no rule knows $p"
        fi
    fi
}

PATHS=()

paths_from_git() {
    local event="${CI_EVENT:-}" before after list
    # -z output in a file: NUL-separated (bash drops NULs from $(...)), so no
    # path is quoted, split or joined.
    list="$(mktemp)"
    # shellcheck disable=SC2064
    trap "rm -f '$list'" EXIT
    case "$event" in
        pull_request)
            if ! git rev-parse --verify --quiet 'HEAD^2^{commit}' >/dev/null; then
                everything "HEAD is not a merge commit with two parents"
                return
            fi
            if ! git -c core.quotepath=off diff --no-renames --name-only -z 'HEAD^1' HEAD >"$list"; then
                everything "git diff HEAD^1 HEAD failed"
                return
            fi
            ;;
        push)
            before="${CI_BEFORE:-}"
            after="${CI_AFTER:-HEAD}"
            if [[ -z "$before" || "$before" =~ ^0+$ ]]; then
                everything "the push has no previous commit (before is ${before:-empty})"
                return
            fi
            # Commit ids only: nothing that git could read as an option or a ref.
            if [[ ! "$before" =~ ^[0-9a-f]{40}([0-9a-f]{24})?$ ]] \
                || ! git rev-parse --verify --quiet "${before}^{commit}" >/dev/null; then
                everything "the previous commit $before is not in the clone"
                return
            fi
            if [[ "$after" != HEAD && ! "$after" =~ ^[0-9a-f]{40}([0-9a-f]{24})?$ ]] \
                || ! git rev-parse --verify --quiet "${after}^{commit}" >/dev/null; then
                everything "the pushed commit $after is not in the clone"
                return
            fi
            if ! git -c core.quotepath=off diff --no-renames --name-only -z "$before" "$after" >"$list"; then
                everything "git diff $before $after failed"
                return
            fi
            ;;
        *)
            everything "event '${event}' is neither push nor pull_request"
            return
            ;;
    esac
    local p
    while IFS= read -r -d '' p; do
        if [[ -n "$p" ]]; then PATHS+=("$p"); fi
    done <"$list"
}

paths_from_file() {
    local src="$1" p
    if [[ "$src" == - ]]; then
        src=/dev/stdin
    elif [[ ! -r "$src" ]]; then
        echo "changes: error: cannot read $src" >&2
        exit 2
    fi
    while IFS= read -r p || [[ -n "$p" ]]; do
        p="${p%$'\r'}"
        if [[ -n "$p" ]]; then PATHS+=("$p"); fi
    done <"$src"
}

main() {
    local paths_from=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --paths-from)
                [[ $# -ge 2 ]] || { echo "changes: error: --paths-from needs a file or -" >&2; exit 2; }
                paths_from="$2"
                shift
                ;;
            -h | --help) sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2 ;;
            *) echo "changes: error: unknown argument $1" >&2; exit 2 ;;
        esac
        shift
    done

    local wide=false
    if [[ "${CI_EVENT:-}" == pull_request ]]; then
        wide=true
    fi
    # In any case, as GitHub's contains() matched it before: [CI full] too.
    local msg="${CI_HEAD_MESSAGE:-}"
    if [[ "${msg,,}" == *"[ci full]"* ]]; then
        wide=true
        everything "the head commit message asks for [ci full]"
    fi

    if [[ -z "$EVERYTHING" ]]; then
        if [[ -n "$paths_from" ]]; then
            paths_from_file "$paths_from"
        else
            paths_from_git
        fi
    fi

    if [[ -z "$EVERYTHING" ]]; then
        say "${#PATHS[@]} changed path(s) (event ${CI_EVENT:-unset})"
        local p
        for p in "${PATHS[@]}"; do
            classify "$p"
            if [[ -n "$EVERYTHING" ]]; then break; fi
        done
    fi

    local a
    if [[ -n "$EVERYTHING" ]]; then
        for a in "${AREAS[@]}"; do ON[$a]=true; done
    else
        # What builds on what (SYNC_PLAN.md §4), in this order: recovery reads
        # core's board and seal's files; the device runtime links core, seal,
        # the engine and the backup crate, and its end-to-end tests run against
        # the relay; the phone links the device runtime.
        if [[ ${ON[core]} == true || ${ON[seal]} == true ]]; then ON[recovery]=true; fi
        local d
        for d in core seal engine relay recovery; do
            if [[ ${ON[$d]} == true ]]; then ON[device]=true; fi
        done
        if [[ ${ON[device]} == true ]]; then ON[android]=true; fi
    fi

    local oses="$LINUX_OS"
    if [[ "$wide" == true ]]; then oses="$ALL_OSES"; fi

    local out=""
    for a in "${AREAS[@]}"; do out+="$a=${ON[$a]}"$'\n'; done
    out+="wide=$wide"$'\n'
    out+="oses=$oses"$'\n'
    printf '%s' "$out"
    if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
        printf '%s' "$out" >>"$GITHUB_OUTPUT"
    fi
    local on_list=""
    for a in "${AREAS[@]}"; do if [[ ${ON[$a]} == true ]]; then on_list+=" $a"; fi; done
    say "areas that need CI:${on_list:- none}; wide=$wide"
}

main "$@"
