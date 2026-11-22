#!/usr/bin/env bash
# Installs the tools a stage of SYNC_PLAN.md needs in a fresh container.
#
#   bash scripts/dev-setup.sh <stage>           install what <stage> needs (`all`: everything)
#   bash scripts/dev-setup.sh --check <stage>   only report what is missing
#   bash scripts/dev-setup.sh --list            print the stage table
#   bash scripts/dev-setup.sh --env             print the exports for what is installed
#
# Every tool is pinned here: a version, the official download URL and a SHA-256
# written in this file, never fetched from where the download comes from
# (SYNC_PLAN.md §3.4, "install tools from their official sources at pinned
# versions"; V1-4). A checksum mismatch stops the script and installs nothing.
# Versions follow the project's seven-day rule: each was published at least
# seven days before it was pinned, except wrangler (see its pin).
#
# Where things go:
#   - single binaries (cargo-deny, cargo-audit, cargo-ndk) into one directory:
#     $TASKDECK_TOOLS_DIR if set, else ${CARGO_HOME:-~/.cargo}/bin, which rustup
#     already puts on PATH (when that directory is not on PATH, --env exports
#     it). A tool whose installed binary already has the pinned
#     hash (or, for cargo-ndk, which is built here, reports the pinned version)
#     is skipped;
#   - the Android SDK and NDK, and wrangler, under $TASKDECK_TOOLS_HOME, else
#     ~/.taskdeck-tools: android-sdk/ (ANDROID_HOME), android-sdk/ndk/<rev>
#     (ANDROID_NDK_HOME) and wrangler-<version>/. The script writes the exports
#     to $TASKDECK_TOOLS_HOME/env.sh and prints them; `--env` prints them again
#     (eval "$(bash scripts/dev-setup.sh --env)");
#   - the Rust toolchain of rust-toolchain.toml through rustup
#     (scripts/ci/toolchain.sh, which CI uses too).
#
# Options:
#   --from-dir <dir>       take release archives from <dir> (named as in the URL)
#                          instead of downloading them; the checksum still applies.
#   --i-am-in-a-container  allow wrangler outside CI and a cloud session (below).
#
# Linux x86_64 only for now.
set -euo pipefail

# ---------------------------------------------------------------------------
# Which stage needs which tool (SYNC_PLAN.md §6.1 item 6). `all` installs
# everything. Tools install in the order written: rust-toolchain before
# cargo-ndk (which it builds), android-cmdline-tools before android-sdk-packages
# (whose sdkmanager it is).
#   foundation   the supply-chain tools, and what android-targets needs (its gate
#                runs both)
#   supply-chain cargo-deny and cargo-audit: CI's supply-chain job (scripts/ci/supply-chain.sh)
#   android-targets  the toolchain, cargo-ndk and an NDK for the Android-target
#                check of the crates with C code (scripts/check-android-targets.sh;
#                CI's android job through crates/taskdeck-android/ci.sh).
#                android-ndk-env is the NDK that ANDROID_NDK_HOME or
#                ANDROID_NDK_LATEST_HOME names (GitHub's Ubuntu runners carry
#                one) when it is usable, else the pinned android-ndk
#   relay, device, rc   wrangler, and Node 22 (checked, not installed: the
#                container image carries it, SYNC_PLAN.md §5)
#   android-shell, android   the SDK, its platform and build-tools, the NDK, cargo-ndk
#   release      everything: it builds every artifact in its dry run
# ---------------------------------------------------------------------------
ALL_TOOLS="rust-toolchain cargo-deny cargo-audit android-cmdline-tools android-sdk-packages android-ndk cargo-ndk node wrangler"
ANDROID_TOOLS="rust-toolchain android-cmdline-tools android-sdk-packages android-ndk cargo-ndk"
declare -A STAGE_TOOLS=(
    [foundation]="cargo-deny cargo-audit rust-toolchain cargo-ndk android-ndk-env"
    [supply-chain]="cargo-deny cargo-audit"
    [android-targets]="rust-toolchain cargo-ndk android-ndk-env"
    [core]="rust-toolchain"
    [seal]="rust-toolchain"
    [relay]="node wrangler"
    [engine]="rust-toolchain"
    [android-shell]="$ANDROID_TOOLS"
    [device]="rust-toolchain node wrangler"
    [recovery]="rust-toolchain"
    [desktop]="rust-toolchain"
    [android]="$ANDROID_TOOLS"
    [release]="$ALL_TOOLS"
    [rc]="$ALL_TOOLS"
    [all]="$ALL_TOOLS"
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

# The Android command-line tools 22.0 (build 15859902, `cmdline-tools;22.0`),
# the archive Android Studio's download page offers. Google publishes its
# SHA-256 there (developer.android.com/studio, "Command line tools only"); the
# value below is that one, and the archive matched it when it was pinned.
CMDLINE_TOOLS_VERSION="22.0"
CMDLINE_TOOLS_URL="https://dl.google.com/android/repository/commandlinetools-linux-15859902_latest.zip"
CMDLINE_TOOLS_ARCHIVE_SHA256="4e4c464f145a7512b57d088ac6c278c03c9eea610886b35a5e0804e74eedf583"

# What sdkmanager installs (SYNC.md §8: compileSdk 37, build-tools to match).
# sdkmanager downloads these itself and checks each archive against the SHA-1
# of Google's signed-over-HTTPS repository manifest (repository2-3.xml); the
# script does not see those archives. It pins the package path and revision,
# and fails if what is installed is not that revision.
SDK_PLATFORM_PACKAGE="platforms;android-37.0"
SDK_PLATFORM_REVISION="2"
SDK_BUILD_TOOLS_PACKAGE="build-tools;37.0.0"
SDK_BUILD_TOOLS_REVISION="37.0.0"

# NDK r28c (28.2.13676358): r28, the first with 16 KB pages by default
# (SYNC.md §8). Google publishes a SHA-1 for it, in the repository manifest
# (repository2-3.xml, `ndk;28.2.13676358`); the archive matched that SHA-1 when
# it was pinned, and the SHA-256 below is the same archive's. Both are checked.
NDK_RELEASE="r28c"
NDK_REVISION="28.2.13676358"
NDK_URL="https://dl.google.com/android/repository/android-ndk-r28c-linux.zip"
NDK_ARCHIVE_SHA256="dfb20d396df28ca02a8c708314b814a4d961dc9074f9a161932746f815aa552f"
NDK_ARCHIVE_SHA1="a7b54a5de87fecd125a17d54f73c446199e72a64"

# cargo-ndk 4.1.2, 2025-08-09, built from its crates.io source with the
# Cargo.lock it ships (`cargo install --locked`). The SHA-256 is the crate
# file's, as crates.io's index records it; cargo checks every dependency's
# against the index too. The binary is built here, so it has no pinned hash:
# it must report the pinned version.
CARGO_NDK_VERSION="4.1.2"
CARGO_NDK_URL="https://static.crates.io/crates/cargo-ndk/cargo-ndk-4.1.2.crate"
CARGO_NDK_CRATE_SHA256="903cc87cda6ab7a2ff82a74065e1c2ae5baa869546b32eb4aacabcc6ed5a670f"

# wrangler 4.166.0, the version SYNC_PLAN.md §5 checked. Published on
# 2026-11-20, so younger than seven days when it was pinned (2026-11-22): the
# plan names this version. It is installed from scripts/wrangler/ (package.json
# and package-lock.json, resolved with npm's --before 2026-11-21) by `npm ci`,
# which refuses any package whose tarball does not match the lockfile's
# integrity hash (SHA-512); the script also refuses a lockfile whose wrangler
# entry is not this version with this hash (npm's dist.integrity of 4.166.0).
# No install scripts run (--ignore-scripts).
WRANGLER_VERSION="4.166.0"
WRANGLER_INTEGRITY="sha512-qMz0sHGXqEC0Va8T6dAaG0ggp9tCcuHHYMUQzRKbtZg6+MPAPb6oBu1757IaeugNJHPAz+twHiA+zDyXWQLsLw=="

# Node: checked, not installed.
NODE_MAJOR_MIN="22"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
TOOLS_DIR="${TASKDECK_TOOLS_DIR:-${CARGO_HOME:-$HOME/.cargo}/bin}"
TOOLS_HOME="${TASKDECK_TOOLS_HOME:-$HOME/.taskdeck-tools}"
SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FROM_DIR=""
IN_CONTAINER=0  # --i-am-in-a-container
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

# fetch_verified <name> <version> <url> <sha256> <sha1 or ""> <out file>
# Downloads (or takes from --from-dir) one archive and refuses it unless its
# SHA-256 (and SHA-1, when Google publishes only that) is the pinned one.
fetch_verified() {
    local name="$1" version="$2" url="$3" sha="$4" sha1="$5" out="$6"
    mkdir -p "$(dirname "$out")"
    if [[ -n "$FROM_DIR" ]]; then
        local local_file
        local_file="$FROM_DIR/$(basename "${url//%2F//}")"
        [[ -f "$local_file" ]] || die "$name: $local_file not found (--from-dir)"
        cp "$local_file" "$out"
    else
        say "downloading $name $version from $url"
        local t0=$SECONDS
        curl --proto '=https' --tlsv1.2 -sSfL --retry 3 -o "$out" "$url" \
            || die "$name: download failed: $url"
        say "downloaded $(du -h "$out" | cut -f1) in $((SECONDS - t0)) s"
    fi
    local got
    got="$(sha256_of "$out")"
    if [[ "$got" != "$sha" ]]; then
        rm -f "$out"
        echo "dev-setup: CHECKSUM MISMATCH for $name $version" >&2
        echo "dev-setup:   expected $sha" >&2
        echo "dev-setup:   got      $got" >&2
        die "$name: refusing to install an archive whose SHA-256 is not the pinned one"
    fi
    if [[ -n "$sha1" ]]; then
        got="$(sha1sum "$out" | cut -d' ' -f1)"
        if [[ "$got" != "$sha1" ]]; then
            rm -f "$out"
            echo "dev-setup: CHECKSUM MISMATCH for $name $version (Google's SHA-1)" >&2
            echo "dev-setup:   expected $sha1" >&2
            echo "dev-setup:   got      $got" >&2
            die "$name: refusing to install an archive whose SHA-1 is not the one Google publishes"
        fi
    fi
}

# prop_of <source.properties> <key>: the value of one key, spaces trimmed.
prop_of() {
    [[ -f "$1" ]] || return 0
    sed -n "s/^[[:space:]]*$2[[:space:]]*=[[:space:]]*//p" "$1" | head -n 1 | tr -d '\r' | sed 's/[[:space:]]*$//'
}

# dir_status <dir> <pinned archive sha256> <expected Pkg.Revision>
# A directory installed from an archive carries a .taskdeck-pin file with the
# archive's SHA-256; the revision its source.properties names must match too.
dir_status() {
    local dir="$1" sha="$2" rev="$3"
    if [[ ! -e "$dir" ]]; then
        echo missing
    elif [[ "$(cat "$dir/.taskdeck-pin" 2>/dev/null)" == "$sha" \
            && "$(prop_of "$dir/source.properties" Pkg.Revision)" == "$rev" ]]; then
        echo ok
    else
        echo differs
    fi
}

# install_zip_dir <name> <version> <url> <sha256> <sha1 or ""> <top dir in the
#                 archive> <destination> <expected Pkg.Revision>
# Fetches and verifies a zip archive, unpacks it next to the destination,
# checks the revision it names, and only then puts it in place.
install_zip_dir() {
    local name="$1" version="$2" url="$3" sha="$4" sha1="$5" top="$6" dest="$7" rev="$8"
    local status
    status="$(dir_status "$dest" "$sha" "$rev")"
    if [[ "$status" == ok ]]; then
        say "$name $version already installed in $dest"
        return 0
    fi
    command -v unzip >/dev/null || die "$name: unzip is needed"
    local parent partial archive
    parent="$(dirname "$dest")"
    mkdir -p "$parent"
    partial="$parent/.partial-$(basename "$dest")-$$"
    archive="$TOOLS_HOME/.downloads/$(basename "${url//%2F//}")"
    rm -rf "$partial"
    fetch_verified "$name" "$version" "$url" "$sha" "$sha1" "$archive"
    local t0=$SECONDS
    mkdir -p "$partial"
    unzip -q "$archive" -d "$partial" || { rm -rf "$partial"; die "$name: cannot unpack $archive"; }
    rm -f "$archive"
    [[ -d "$partial/$top" ]] || { rm -rf "$partial"; die "$name: the archive has no $top/"; }
    local got
    got="$(prop_of "$partial/$top/source.properties" Pkg.Revision)"
    [[ "$got" == "$rev" ]] || { rm -rf "$partial"; die "$name: the archive says revision '$got', pinned $rev"; }
    echo "$sha" >"$partial/$top/.taskdeck-pin"
    rm -rf "$dest"
    mv "$partial/$top" "$dest"
    rm -rf "$partial"
    say "installed $name $version into $dest ($(du -sh "$dest" | cut -f1), unpacked in $((SECONDS - t0)) s)"
}

ANDROID_SDK_DIR() { echo "$TOOLS_HOME/android-sdk"; }
NDK_DIR() { echo "$TOOLS_HOME/android-sdk/ndk/$NDK_REVISION"; }
CMDLINE_TOOLS_DIR() { echo "$TOOLS_HOME/android-sdk/cmdline-tools/$CMDLINE_TOOLS_VERSION"; }
WRANGLER_DIR() { echo "$TOOLS_HOME/wrangler-$WRANGLER_VERSION"; }

# in_container: true in GitHub Actions, in a Claude Code cloud session, or when
# --i-am-in-a-container was given.
in_container() {
    [[ "$IN_CONTAINER" -eq 1 || "${GITHUB_ACTIONS:-}" == true || "${CLAUDE_CODE_REMOTE:-}" == true ]]
}

# env_lines: the exports for what is installed now: the tool directory when it
# holds a tool and is not on PATH; the pinned NDK, else a usable one that the
# environment names (env_ndk).
env_lines() {
    local b ndk
    case ":$PATH:" in
        *":$TOOLS_DIR:"*) ;;
        *) for b in cargo-deny cargo-audit cargo-ndk; do
               if [[ -x "$TOOLS_DIR/$b" ]]; then echo "export PATH=\"$TOOLS_DIR:\$PATH\""; break; fi
           done ;;
    esac
    if [[ "$(tool_android_cmdline_tools_check)" == ok || "$(tool_android_ndk_check)" == ok ]]; then
        echo "export ANDROID_HOME=\"$(ANDROID_SDK_DIR)\""
    fi
    if [[ "$(tool_android_ndk_check)" == ok ]]; then ndk="$(NDK_DIR)"; else ndk="$(env_ndk)"; fi
    if [[ -n "$ndk" ]]; then
        echo "export ANDROID_NDK_HOME=\"$ndk\""
    fi
    if [[ "$(tool_android_cmdline_tools_check)" == ok ]]; then
        echo "export PATH=\"$(CMDLINE_TOOLS_DIR)/bin:\$PATH\""
    fi
    if [[ "$(tool_wrangler_check)" == ok ]]; then
        echo "export TASKDECK_WRANGLER=\"$(WRANGLER_DIR)/node_modules/.bin/wrangler\""
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

# The Rust toolchain of rust-toolchain.toml, with its profile, components and
# targets, through rustup: scripts/ci/toolchain.sh installs and verifies it.
toolchain_field() {  # channel | components | targets
    (
        # shellcheck source=ci/toolchain.sh
        source "$SCRIPTS_DIR/ci/toolchain.sh"
        parse_toolchain_file "$SCRIPTS_DIR/../rust-toolchain.toml" >/dev/null
        case "$1" in
            channel) echo "$TC_CHANNEL" ;;
            components) echo "${TC_COMPONENTS[*]+${TC_COMPONENTS[*]}}" ;;
            targets) echo "${TC_TARGETS[*]+${TC_TARGETS[*]}}" ;;
        esac
    )
}
tool_rust_toolchain_check() {
    command -v rustup >/dev/null || { echo missing; return; }
    local ch c t have
    ch="$(toolchain_field channel)" || { echo missing; return; }
    (cd / && rustup toolchain list) 2>/dev/null | grep -q "^$ch-" || { echo missing; return; }
    have="$(cd / && rustup component list --installed --toolchain "$ch" 2>/dev/null)"
    for c in $(toolchain_field components); do
        grep -q "^$c-" <<<"$have" || { echo missing; return; }
    done
    have="$(cd / && rustup target list --installed --toolchain "$ch" 2>/dev/null)"
    for t in $(toolchain_field targets); do
        grep -qx "$t" <<<"$have" || { echo missing; return; }
    done
    echo ok
}
tool_rust_toolchain_install() {
    if [[ "$(tool_rust_toolchain_check)" == ok ]]; then
        say "rust toolchain $(toolchain_field channel) already installed with its components and targets"
        return 0
    fi
    command -v rustup >/dev/null || die "rustup is not on PATH: install it from https://rustup.rs first"
    bash "$SCRIPTS_DIR/ci/toolchain.sh" || die "the toolchain of rust-toolchain.toml did not install"
}
tool_rust_toolchain_version() { toolchain_field channel; }

tool_android_cmdline_tools_check() {
    dir_status "$(CMDLINE_TOOLS_DIR)" "$CMDLINE_TOOLS_ARCHIVE_SHA256" "$CMDLINE_TOOLS_VERSION"
}
tool_android_cmdline_tools_install() {
    install_zip_dir android-cmdline-tools "$CMDLINE_TOOLS_VERSION" "$CMDLINE_TOOLS_URL" \
        "$CMDLINE_TOOLS_ARCHIVE_SHA256" "" cmdline-tools "$(CMDLINE_TOOLS_DIR)" "$CMDLINE_TOOLS_VERSION"
}
tool_android_cmdline_tools_version() { echo "$CMDLINE_TOOLS_VERSION"; }

# package_dir <sdkmanager package path>: where sdkmanager puts it.
package_dir() { echo "$(ANDROID_SDK_DIR)/${1//;//}"; }
tool_android_sdk_packages_check() {
    local p r pkg rev
    for p in "$SDK_PLATFORM_PACKAGE=$SDK_PLATFORM_REVISION" "$SDK_BUILD_TOOLS_PACKAGE=$SDK_BUILD_TOOLS_REVISION"; do
        pkg="${p%=*}" rev="${p##*=}"
        r="$(package_dir "$pkg")/source.properties"
        [[ -f "$r" ]] || { echo missing; return; }
        [[ "$(prop_of "$r" Pkg.Revision)" == "$rev" ]] || { echo differs; return; }
    done
    echo ok
}
tool_android_sdk_packages_install() {
    if [[ "$(tool_android_sdk_packages_check)" == ok ]]; then
        say "$SDK_PLATFORM_PACKAGE and $SDK_BUILD_TOOLS_PACKAGE already installed in $(ANDROID_SDK_DIR)"
        return 0
    fi
    [[ "$(tool_android_cmdline_tools_check)" == ok ]] \
        || die "android-sdk-packages: the command-line tools are not installed (android-cmdline-tools first)"
    command -v java >/dev/null || die "android-sdk-packages: sdkmanager needs Java 17 or later on PATH"
    local sdkm
    sdkm="$(CMDLINE_TOOLS_DIR)/bin/sdkmanager"
    local t0=$SECONDS
    # sdkmanager will not install before the SDK licences are accepted. This
    # accepts them for this SDK directory, as every CI setup of the SDK does.
    say "accepting the Android SDK licences for $(ANDROID_SDK_DIR) (sdkmanager --licenses)"
    # (`yes` ends on SIGPIPE when sdkmanager stops reading: only sdkmanager's
    # status counts.)
    (set +o pipefail; yes 2>/dev/null | "$sdkm" --sdk_root="$(ANDROID_SDK_DIR)" --licenses >/dev/null) \
        || die "android-sdk-packages: sdkmanager --licenses failed"
    say "sdkmanager: installing $SDK_PLATFORM_PACKAGE and $SDK_BUILD_TOOLS_PACKAGE (it checks Google's SHA-1 of each archive)"
    "$sdkm" --sdk_root="$(ANDROID_SDK_DIR)" --install "$SDK_PLATFORM_PACKAGE" "$SDK_BUILD_TOOLS_PACKAGE" </dev/null \
        | tr '\r' '\n' | grep -Ev '^[[:space:]]*$|\] [0-9]+%' || true
    local status
    status="$(tool_android_sdk_packages_check)"
    [[ "$status" == ok ]] || die "android-sdk-packages: after sdkmanager the packages are $status: Google may serve another revision now; compare $(package_dir "$SDK_PLATFORM_PACKAGE")/source.properties with the pins"
    say "installed $SDK_PLATFORM_PACKAGE (r$SDK_PLATFORM_REVISION) and $SDK_BUILD_TOOLS_PACKAGE in $((SECONDS - t0)) s ($(du -sh "$(package_dir "$SDK_PLATFORM_PACKAGE")" | cut -f1), $(du -sh "$(package_dir "$SDK_BUILD_TOOLS_PACKAGE")" | cut -f1))"
}
tool_android_sdk_packages_version() { echo "$SDK_PLATFORM_PACKAGE r$SDK_PLATFORM_REVISION, $SDK_BUILD_TOOLS_PACKAGE"; }

tool_android_ndk_check() { dir_status "$(NDK_DIR)" "$NDK_ARCHIVE_SHA256" "$NDK_REVISION"; }
tool_android_ndk_install() {
    install_zip_dir android-ndk "$NDK_RELEASE ($NDK_REVISION)" "$NDK_URL" "$NDK_ARCHIVE_SHA256" \
        "$NDK_ARCHIVE_SHA1" "android-ndk-$NDK_RELEASE" "$(NDK_DIR)" "$NDK_REVISION"
}
tool_android_ndk_version() { echo "$NDK_RELEASE ($NDK_REVISION)"; }

# env_ndk: the NDK that ANDROID_NDK_HOME, else ANDROID_NDK_LATEST_HOME, names,
# if it is usable for the Android-target check: of the pinned major release or
# later (16 KB pages, SYNC.md §8) and with its Linux clang. Prints its
# directory, or nothing.
env_ndk() {
    local d major
    for d in "${ANDROID_NDK_HOME:-}" "${ANDROID_NDK_LATEST_HOME:-}"; do
        [[ -n "$d" && -x "$d/toolchains/llvm/prebuilt/linux-x86_64/bin/clang" ]] || continue
        major="$(prop_of "$d/source.properties" Pkg.Revision)"
        major="${major%%.*}"
        if [[ "$major" =~ ^[0-9]+$ && "$major" -ge "${NDK_REVISION%%.*}" ]]; then
            echo "$d"
            return 0
        fi
    done
}
tool_android_ndk_env_check() {
    if [[ -n "$(env_ndk)" ]]; then echo ok; else tool_android_ndk_check; fi
}
tool_android_ndk_env_install() {
    local d
    d="$(env_ndk)"
    if [[ -n "$d" ]]; then
        say "using the NDK $(prop_of "$d/source.properties" Pkg.Revision) that the environment names: $d"
        return 0
    fi
    tool_android_ndk_install
}
tool_android_ndk_env_version() {
    echo "named by ANDROID_NDK_HOME or ANDROID_NDK_LATEST_HOME (r${NDK_REVISION%%.*} or later), else $(tool_android_ndk_version)"
}

tool_cargo_ndk_check() {
    local path="$TOOLS_DIR/cargo-ndk"
    if [[ ! -x "$path" ]]; then
        echo missing
    # cargo-ndk runs only as a cargo subcommand: cargo sets CARGO and passes "ndk".
    elif [[ "$(CARGO="${CARGO:-cargo}" "$path" ndk --version 2>/dev/null | head -n 1)" == "cargo-ndk $CARGO_NDK_VERSION" ]]; then
        echo ok
    else
        echo differs
    fi
}
tool_cargo_ndk_install() {
    local status
    status="$(tool_cargo_ndk_check)"
    if [[ "$status" == ok ]]; then
        say "cargo-ndk $CARGO_NDK_VERSION already installed in $TOOLS_DIR"
        return 0
    fi
    local work="${WORK_DIR:?}/cargo-ndk"
    rm -rf "$work"
    mkdir -p "$work/src"
    fetch_verified cargo-ndk "$CARGO_NDK_VERSION" "$CARGO_NDK_URL" "$CARGO_NDK_CRATE_SHA256" "" "$work/crate.tar.gz"
    tar -xzf "$work/crate.tar.gz" -C "$work/src" || die "cargo-ndk: cannot unpack the crate"
    local ch t0=$SECONDS
    ch="$(toolchain_field channel)"
    say "building cargo-ndk $CARGO_NDK_VERSION with its Cargo.lock (cargo install --locked, toolchain $ch)"
    (cd "$work" && rustup run "$ch" cargo install --locked --quiet --root "$work/root" \
        --path "$work/src/cargo-ndk-$CARGO_NDK_VERSION") || die "cargo-ndk: cargo install failed"
    mkdir -p "$TOOLS_DIR"
    install -m 0755 "$work/root/bin/cargo-ndk" "$TOOLS_DIR/cargo-ndk.tmp.$$"
    mv -f "$TOOLS_DIR/cargo-ndk.tmp.$$" "$TOOLS_DIR/cargo-ndk"
    [[ "$(tool_cargo_ndk_check)" == ok ]] || die "cargo-ndk: the built binary does not report version $CARGO_NDK_VERSION"
    say "installed cargo-ndk $CARGO_NDK_VERSION into $TOOLS_DIR (built in $((SECONDS - t0)) s)"
}
tool_cargo_ndk_version() { echo "$CARGO_NDK_VERSION"; }

tool_node_check() {
    command -v node >/dev/null || { echo missing; return; }
    local major
    major="$(node --version 2>/dev/null | sed -n 's/^v\([0-9][0-9]*\)\..*/\1/p')"
    if [[ -n "$major" && "$major" -ge "$NODE_MAJOR_MIN" ]]; then echo ok; else echo differs; fi
}
tool_node_install() {
    local status
    status="$(tool_node_check)"
    [[ "$status" == ok ]] && { say "node $(node --version) is there"; return 0; }
    die "node $NODE_MAJOR_MIN or later is needed ($status) and this script does not install it: use the container image's, or nodejs.org's"
}
tool_node_version() { echo ">= $NODE_MAJOR_MIN (checked only)"; }

# The lockfile's own record of wrangler: "<version> <integrity>".
lock_wrangler() {
    python3 -I -c '
import json, sys
p = json.load(open(sys.argv[1])).get("packages", {}).get("node_modules/wrangler", {})
print(p.get("version", ""), p.get("integrity", ""))
' "$1"
}
tool_wrangler_check() {
    local dir
    dir="$(WRANGLER_DIR)"
    if [[ ! -e "$dir" ]]; then
        echo missing
    elif cmp -s "$dir/package-lock.json" "$SCRIPTS_DIR/wrangler/package-lock.json" \
            && [[ -f "$dir/.taskdeck-pin" && -x "$dir/node_modules/.bin/wrangler" ]] \
            && grep -q "\"version\": \"$WRANGLER_VERSION\"" "$dir/node_modules/wrangler/package.json" 2>/dev/null; then
        echo ok
    else
        echo differs
    fi
}
tool_wrangler_install() {
    if [[ "$(tool_wrangler_check)" == ok ]]; then
        say "wrangler $WRANGLER_VERSION already installed in $(WRANGLER_DIR)"
        return 0
    fi
    # SYNC_PLAN.md §3.4: wrangler is "used only in cloud sessions and CI".
    in_container || die "wrangler: refusing to install outside CI or a cloud session (GITHUB_ACTIONS or CLAUDE_CODE_REMOTE is not true); pass --i-am-in-a-container if this is one"
    command -v npm >/dev/null || die "wrangler: npm is needed"
    local lock="$SCRIPTS_DIR/wrangler/package-lock.json"
    [[ -f "$lock" && -f "$SCRIPTS_DIR/wrangler/package.json" ]] || die "wrangler: scripts/wrangler/package.json and package-lock.json are needed"
    local got
    got="$(lock_wrangler "$lock")"
    [[ "$got" == "$WRANGLER_VERSION $WRANGLER_INTEGRITY" ]] \
        || die "wrangler: the lockfile records '$got', pinned '$WRANGLER_VERSION $WRANGLER_INTEGRITY'"
    local dir partial t0=$SECONDS
    dir="$(WRANGLER_DIR)"
    partial="$dir.partial-$$"
    rm -rf "$partial"
    mkdir -p "$partial"
    cp "$SCRIPTS_DIR/wrangler/package.json" "$lock" "$partial/"
    say "npm ci: wrangler $WRANGLER_VERSION and its lockfile's packages, each checked against its SHA-512"
    (cd "$partial" && npm ci --ignore-scripts --no-audit --no-fund --loglevel=error \
        --registry=https://registry.npmjs.org/) || { rm -rf "$partial"; die "wrangler: npm ci failed"; }
    # wrangler prints warnings (a proxy in the environment, say) before the version line.
    got="$("$partial/node_modules/.bin/wrangler" --version 2>/dev/null | grep -Ex '[0-9]+\.[0-9]+\.[0-9]+' | tail -n 1)"
    [[ "$got" == "$WRANGLER_VERSION" ]] || { rm -rf "$partial"; die "wrangler: the installed wrangler reports '$got'"; }
    sha256_of "$lock" >"$partial/.taskdeck-pin"
    rm -rf "$dir"
    mv "$partial" "$dir"
    say "installed wrangler $WRANGLER_VERSION into $dir ($(du -sh "$dir" | cut -f1), in $((SECONDS - t0)) s)"
}
tool_wrangler_version() { echo "$WRANGLER_VERSION"; }

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
usage() {
    sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//' >&2
    exit 2
}

write_env_file() {
    local lines
    lines="$(env_lines)"
    [[ -n "$lines" ]] || return 0
    mkdir -p "$TOOLS_HOME"
    { echo "# Written by scripts/dev-setup.sh. Source it: . $TOOLS_HOME/env.sh"; echo "$lines"; } >"$TOOLS_HOME/env.sh"
    say "exports, also in $TOOLS_HOME/env.sh (source it, or eval \"\$(bash scripts/dev-setup.sh --env)\"):"
    sed 's/^/    /' <<<"$lines"
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
            --env) mode=env ;;
            --i-am-in-a-container) IN_CONTAINER=1 ;;
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
    if [[ "$mode" == env ]]; then
        env_lines
        exit 0
    fi
    [[ -n "$stage" ]] || usage
    if [[ -z "${STAGE_TOOLS[$stage]+x}" ]]; then
        die "no tools listed for stage '$stage' (known: $(printf '%s\n' "${!STAGE_TOOLS[@]}" | sort | tr '\n' ' '))"
    fi

    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) ;;
        *) die "only Linux x86_64 is supported for now (this is $(uname -s) $(uname -m))" ;;
    esac

    say "tool directory: $TOOLS_DIR"
    say "tools home (Android SDK and NDK, wrangler): $TOOLS_HOME"
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
    write_env_file
    say "done: $stage"
}

# Sourcing the file (as scripts/tests/test-dev-setup.sh does) defines the
# functions without running anything.
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
