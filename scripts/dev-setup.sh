#!/usr/bin/env bash
# Installs the tools a stage of SYNC_PLAN.md needs in a fresh container.
#
#   bash scripts/dev-setup.sh <stage>           install what <stage> needs
#   bash scripts/dev-setup.sh --check <stage>   only report what is missing
#   bash scripts/dev-setup.sh --list            print the stage table
#
# Every tool is pinned here: a version, the official download URL and a SHA-256
# written in this file, never fetched from where the download comes from
# (SYNC_PLAN.md §3.4, "install tools from their official sources at pinned
# versions"; V1-4). A checksum mismatch stops the script and installs nothing.
# Versions follow the project's seven-day rule: each was published at least
# seven days before it was pinned.
#
# Tools go into one directory, printed at the start: $TASKDECK_TOOLS_DIR if set,
# else ${CARGO_HOME:-~/.cargo}/bin, which rustup already puts on PATH. Running the
# script again skips a tool whose installed binary already has the pinned hash.
#
# Options for tests and offline use:
#   --from-dir <dir>   take release archives from <dir> (named as in the URL)
#                      instead of downloading them; the checksum still applies.
#
# Linux x86_64 only for now. A later foundation task adds the Android SDK and
# NDK, cargo-ndk and wrangler: add a row to STAGE_TOOLS and a tool_<name>
# function pair for each.
set -euo pipefail

# ---------------------------------------------------------------------------
# Which stage needs which tool. Stages not listed need nothing from here yet.
# ---------------------------------------------------------------------------
declare -A STAGE_TOOLS=(
    [foundation]="cargo-deny cargo-audit"
)

# ---------------------------------------------------------------------------
# Pins. Dates are the crates.io publish dates of the same versions.
# ---------------------------------------------------------------------------

# cargo-deny 0.20.2, 2026-07-09. 0.21.0 (2026-11-16) and 0.22.0 (2026-11-19)
# were younger than seven days on 2026-11-21. The archive's hash matched the
# .sha256 file the release publishes next to it when it was pinned.
CARGO_DENY_VERSION="0.20.2"
CARGO_DENY_URL="https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz"
CARGO_DENY_ARCHIVE_SHA256="9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f"
CARGO_DENY_MEMBER="cargo-deny-0.20.2-x86_64-unknown-linux-musl/cargo-deny"
CARGO_DENY_BINARY_SHA256="b329e25933d01c36dd7c47d84ea5716694f9b7caf53a5003d45674703a8ed54a"

# cargo-audit 0.23.0, 2026-11-10. The rustsec release publishes no checksum
# file; the hash below is the archive's as downloaded when it was pinned.
CARGO_AUDIT_VERSION="0.23.0"
CARGO_AUDIT_URL="https://github.com/rustsec/rustsec/releases/download/cargo-audit%2Fv0.23.0/cargo-audit-x86_64-unknown-linux-musl-v0.23.0.tgz"
CARGO_AUDIT_ARCHIVE_SHA256="4f01c74ed84b2dffd0b1d37012a62cdf2a92c96e25bdf8eec7fcc0d4d3eb7811"
CARGO_AUDIT_MEMBER="cargo-audit-x86_64-unknown-linux-musl-v0.23.0/cargo-audit"
CARGO_AUDIT_BINARY_SHA256="ae133cc7bdfc6d61d1f9d000da3259423f1a232e6275e9ade45b4b4c67207b64"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
TOOLS_DIR="${TASKDECK_TOOLS_DIR:-${CARGO_HOME:-$HOME/.cargo}/bin}"
FROM_DIR=""
WORK_DIR=""   # set by main; a test that sources this file sets its own

die() { echo "dev-setup: error: $*" >&2; exit 1; }
say() { echo "dev-setup: $*"; }

sha256_of() { sha256sum "$1" | cut -d' ' -f1; }

# status_of <binary-name> <pinned-binary-sha256>: prints ok, missing or differs.
status_of() {
    local path="$TOOLS_DIR/$1"
    if [[ ! -e "$path" ]]; then
        echo missing
    elif [[ "$(sha256_of "$path")" == "$2" ]]; then
        echo ok
    else
        echo differs
    fi
}

# install_release_tarball <binary-name> <version> <url> <archive-sha256>
#                         <member-path> <binary-sha256>
# Downloads (or takes from --from-dir) a release archive, refuses it unless its
# SHA-256 is the pinned one, extracts the one member and installs it.
install_release_tarball() {
    local name="$1" version="$2" url="$3" archive_sha="$4" member="$5" binary_sha="$6"
    local status
    status="$(status_of "$name" "$binary_sha")"
    if [[ "$status" == ok ]]; then
        say "$name $version already installed in $TOOLS_DIR"
        return 0
    fi

    local work="${WORK_DIR:?}/$name"
    rm -rf "$work"
    mkdir -p "$work"
    local archive="$work/archive"
    if [[ -n "$FROM_DIR" ]]; then
        local local_file
        local_file="$FROM_DIR/$(basename "${url//%2F//}")"
        [[ -f "$local_file" ]] || die "$name: $local_file not found (--from-dir)"
        cp "$local_file" "$archive"
    else
        say "downloading $name $version from $url"
        curl --proto '=https' --tlsv1.2 -sSfL --retry 3 -o "$archive" "$url" \
            || die "$name: download failed: $url"
    fi

    local got
    got="$(sha256_of "$archive")"
    if [[ "$got" != "$archive_sha" ]]; then
        echo "dev-setup: CHECKSUM MISMATCH for $name $version" >&2
        echo "dev-setup:   expected $archive_sha" >&2
        echo "dev-setup:   got      $got" >&2
        die "$name: refusing to install an archive whose SHA-256 is not the pinned one"
    fi

    mkdir -p "$work/x"
    tar -xzf "$archive" -C "$work/x" "$member" || die "$name: $member not in the archive"
    got="$(sha256_of "$work/x/$member")"
    [[ "$got" == "$binary_sha" ]] \
        || die "$name: extracted binary has SHA-256 $got, pinned $binary_sha"

    mkdir -p "$TOOLS_DIR"
    install -m 0755 "$work/x/$member" "$TOOLS_DIR/$name.tmp.$$"
    mv -f "$TOOLS_DIR/$name.tmp.$$" "$TOOLS_DIR/$name"
    if [[ "$status" == differs ]]; then
        say "replaced a different $name in $TOOLS_DIR with $version"
    else
        say "installed $name $version into $TOOLS_DIR"
    fi
}

# ---------------------------------------------------------------------------
# Tools: one tool_<name>_check (prints ok/missing/differs) and one
# tool_<name>_install per tool.
# ---------------------------------------------------------------------------
tool_cargo_deny_check() { status_of cargo-deny "$CARGO_DENY_BINARY_SHA256"; }
tool_cargo_deny_install() {
    install_release_tarball cargo-deny "$CARGO_DENY_VERSION" "$CARGO_DENY_URL" \
        "$CARGO_DENY_ARCHIVE_SHA256" "$CARGO_DENY_MEMBER" "$CARGO_DENY_BINARY_SHA256"
}
tool_cargo_deny_version() { echo "$CARGO_DENY_VERSION"; }

tool_cargo_audit_check() { status_of cargo-audit "$CARGO_AUDIT_BINARY_SHA256"; }
tool_cargo_audit_install() {
    install_release_tarball cargo-audit "$CARGO_AUDIT_VERSION" "$CARGO_AUDIT_URL" \
        "$CARGO_AUDIT_ARCHIVE_SHA256" "$CARGO_AUDIT_MEMBER" "$CARGO_AUDIT_BINARY_SHA256"
}
tool_cargo_audit_version() { echo "$CARGO_AUDIT_VERSION"; }

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
usage() {
    sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//' >&2
    exit 2
}

fn_of() { echo "tool_${1//-/_}"; }

main() {
    WORK_DIR="$(mktemp -d)"
    trap 'rm -rf "$WORK_DIR"' EXIT

    local mode=install stage="" tool fn status version missing
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --check) mode=check ;;
            --list) mode=list ;;
            --from-dir) [[ $# -ge 2 ]] || usage; FROM_DIR="$2"; shift ;;
            -h|--help) usage ;;
            -*) echo "dev-setup: unknown option $1" >&2; usage ;;
            *) [[ -z "$stage" ]] || usage; stage="$1" ;;
        esac
        shift
    done

    if [[ "$mode" == list ]]; then
        for s in "${!STAGE_TOOLS[@]}"; do echo "$s: ${STAGE_TOOLS[$s]}"; done | sort
        exit 0
    fi
    [[ -n "$stage" ]] || usage
    if [[ -z "${STAGE_TOOLS[$stage]+x}" ]]; then
        die "no tools listed for stage '$stage' (known: $(printf '%s ' "${!STAGE_TOOLS[@]}"))"
    fi

    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) ;;
        *) die "only Linux x86_64 is supported for now (this is $(uname -s) $(uname -m))" ;;
    esac

    say "tool directory: $TOOLS_DIR"
    case ":$PATH:" in
        *":$TOOLS_DIR:"*) ;;
        *) say "note: $TOOLS_DIR is not on PATH; add it to use the tools by name" ;;
    esac

    if [[ "$mode" == check ]]; then
        missing=0
        for tool in ${STAGE_TOOLS[$stage]}; do
            fn="$(fn_of "$tool")"
            status="$("${fn}_check")"
            version="$("${fn}_version")"
            case "$status" in
                ok) echo "ok       $tool $version" ;;
                missing) echo "missing  $tool $version"; missing=1 ;;
                differs) echo "differs  $tool (installed binary is not the pinned $version)"; missing=1 ;;
            esac
        done
        if [[ "$missing" -ne 0 ]]; then
            say "run: bash scripts/dev-setup.sh $stage"
            exit 1
        fi
        exit 0
    fi

    for tool in ${STAGE_TOOLS[$stage]}; do
        "$(fn_of "$tool")_install"
    done
    say "done: $stage"
}

# Sourcing the file (as scripts/tests/test-dev-setup.sh does) defines the
# functions without running anything.
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
