#!/usr/bin/env bash
# Tests for scripts/dev-setup.sh. No network: archives come from --from-dir,
# and the install path is exercised with a made-up tool archive whose hashes
# the test computes itself.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

DEV="$SCRIPTS_DIR/dev-setup.sh"
# The SDK, the NDK and wrangler go under TASKDECK_TOOLS_HOME: never the real one here.
export TASKDECK_TOOLS_HOME="$TMP/tools-home"
# An NDK the environment names is used by the android-targets row: none here
# unless a case sets one.
unset ANDROID_NDK_HOME ANDROID_NDK_LATEST_HOME

# --- the command line --------------------------------------------------------
mkdir -p "$TMP/empty-tools"
expect "--check reports missing tools" 1 env TASKDECK_TOOLS_DIR="$TMP/empty-tools" bash "$DEV" --check foundation
output_has "--check names cargo-deny as missing" "missing  cargo-deny 0.20.2"
output_has "--check names cargo-audit as missing" "missing  cargo-audit 0.23.0"
output_has "--check says how to install" "run: bash scripts/dev-setup.sh foundation"
output_has "--check prints the tool directory" "tool directory: $TMP/empty-tools"

mkdir -p "$TMP/other-tools"
printf '#!/bin/sh\necho "cargo-deny 0.20.2"\n' >"$TMP/other-tools/cargo-deny"
chmod +x "$TMP/other-tools/cargo-deny"
expect "--check refuses a binary that is not the pinned one" 1 env TASKDECK_TOOLS_DIR="$TMP/other-tools" bash "$DEV" --check foundation
output_has "--check reports the differing binary" "differs  cargo-deny"

# A wrong archive offered through --from-dir is refused before anything is
# installed, without any download.
mkdir -p "$TMP/bad-archives" "$TMP/bad-tools"
echo "not the real archive" >"$TMP/bad-archives/cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz"
echo "not the real archive" >"$TMP/bad-archives/cargo-audit-x86_64-unknown-linux-musl-v0.23.0.tgz"
expect "a wrong checksum is refused" 1 env TASKDECK_TOOLS_DIR="$TMP/bad-tools" bash "$DEV" --from-dir "$TMP/bad-archives" foundation
output_has "the mismatch is reported" "CHECKSUM MISMATCH for cargo-deny 0.20.2"
output_has "the expected hash is printed" "expected 9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f"
if [[ -e "$TMP/bad-tools/cargo-deny" || -e "$TMP/bad-tools/cargo-audit" ]]; then
    FAILS=$((FAILS + 1)); echo "FAIL  nothing installed after a mismatch"
else
    PASSES=$((PASSES + 1)); echo "ok    nothing installed after a mismatch"
fi

expect "--from-dir without the archive fails" 1 env TASKDECK_TOOLS_DIR="$TMP/bad-tools" bash "$DEV" --from-dir "$TMP/empty-tools" foundation
output_has "the missing archive is named" "cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz not found"

expect "an unknown stage fails" 1 bash "$DEV" --check no-such-stage
output_has "the known stages are listed" "known: all android android-shell android-targets core desktop device engine foundation rc recovery relay release seal supply-chain"
expect "--list prints the stage table" 0 bash "$DEV" --list
output_has "foundation needs cargo-deny and cargo-audit" "foundation: cargo-deny cargo-audit"
output_has "foundation also needs what android-targets needs" "foundation: cargo-deny cargo-audit rust-toolchain cargo-ndk android-ndk-env"
output_has "CI's supply-chain job needs the two tools only" "supply-chain: cargo-deny cargo-audit"
output_has "the Android-target check needs the toolchain, cargo-ndk and an NDK" "android-targets: rust-toolchain cargo-ndk android-ndk-env"
output_has "android needs the SDK" "android: rust-toolchain android-cmdline-tools android-sdk-packages android-ndk cargo-ndk"
output_has "relay needs node and wrangler" "relay: node wrangler"
output_has "all installs everything" "all: rust-toolchain cargo-deny cargo-audit android-cmdline-tools android-sdk-packages android-ndk cargo-ndk node wrangler"
expect "no stage is a usage error" 2 bash "$DEV" --check

# --- the install path, with a made-up tool -----------------------------------
# Sourcing defines the functions without running main.
source "$DEV"
set +eu
TOOLS_HOME="$TMP/tools-home"
WORK_DIR="$TMP/work"
mkdir -p "$WORK_DIR" "$TMP/fake-src/fake-1.0" "$TMP/archives"
printf '#!/bin/sh\necho fake-tool 1.0\n' >"$TMP/fake-src/fake-1.0/fake-tool"
chmod +x "$TMP/fake-src/fake-1.0/fake-tool"
tar -czf "$TMP/archives/fake-1.0.tar.gz" -C "$TMP/fake-src" fake-1.0
ASHA="$(sha256_of "$TMP/archives/fake-1.0.tar.gz")"
BSHA="$(sha256_of "$TMP/fake-src/fake-1.0/fake-tool")"
URL="https://example.invalid/releases/download/fake%2Fv1.0/fake-1.0.tar.gz"
FROM_DIR="$TMP/archives"

TOOLS_DIR="$TMP/tools"
expect "a matching archive installs" 0 install_release_tarball fake-tool 1.0 "$URL" "$ASHA" fake-1.0/fake-tool "$BSHA"
output_has "install says where" "installed fake-tool 1.0 into $TMP/tools"
expect "the installed binary has the pinned hash" 0 test "$(status_of fake-tool "$BSHA")" = ok
expect "a second run is a no-op" 0 install_release_tarball fake-tool 1.0 "$URL" "$ASHA" fake-1.0/fake-tool "$BSHA"
output_has "the second run skips" "fake-tool 1.0 already installed"

echo "tampered" >>"$TMP/tools/fake-tool"
expect "a changed binary reads as differs" 0 test "$(status_of fake-tool "$BSHA")" = differs
expect "a changed binary is replaced" 0 install_release_tarball fake-tool 1.0 "$URL" "$ASHA" fake-1.0/fake-tool "$BSHA"
output_has "the replacement is reported" "replaced a different fake-tool"
expect "the replacement has the pinned hash" 0 test "$(status_of fake-tool "$BSHA")" = ok

TOOLS_DIR="$TMP/tools2"
WRONG="0000000000000000000000000000000000000000000000000000000000000000"
expect "a wrong archive hash is refused" 1 install_release_tarball fake-tool 1.0 "$URL" "$WRONG" fake-1.0/fake-tool "$BSHA"
output_has "the refusal is loud" "CHECKSUM MISMATCH for fake-tool 1.0"
expect "nothing is installed after the refusal" 0 test ! -e "$TMP/tools2/fake-tool"
expect "a wrong binary hash is refused" 1 install_release_tarball fake-tool 1.0 "$URL" "$ASHA" fake-1.0/fake-tool "$WRONG"
output_has "the binary mismatch is reported" "extracted binary has SHA-256 $BSHA"
expect "nothing is installed after the binary refusal" 0 test ! -e "$TMP/tools2/fake-tool"

# --- --check when the pinned binaries are there ------------------------------
# Stand-in binaries, with the pins pointed at their hashes; main runs in the
# subshell expect opens, so its exit does not end this file.
mkdir -p "$TMP/ok-tools"
printf '#!/bin/sh\necho stand-in deny\n' >"$TMP/ok-tools/cargo-deny"
printf '#!/bin/sh\necho stand-in audit\n' >"$TMP/ok-tools/cargo-audit"
CARGO_DENY_BINARY_SHA256="$(sha256_of "$TMP/ok-tools/cargo-deny")"
CARGO_AUDIT_BINARY_SHA256="$(sha256_of "$TMP/ok-tools/cargo-audit")"
TOOLS_DIR="$TMP/ok-tools"
FROM_DIR=""
# The other foundation tools have their own tests below: stand-ins here.
real_rust_check="$(declare -f tool_rust_toolchain_check)"
real_ndk_check="$(declare -f tool_android_ndk_check)"
real_cargo_ndk_check="$(declare -f tool_cargo_ndk_check)"
tool_rust_toolchain_check() { echo ok; }
tool_android_ndk_check() { echo ok; }
tool_cargo_ndk_check() { echo ok; }
expect "--check passes when every pinned binary is there" 0 main --check foundation
output_has "--check reports cargo-deny ok" "ok       cargo-deny $CARGO_DENY_VERSION"
output_has "--check reports cargo-audit ok" "ok       cargo-audit $CARGO_AUDIT_VERSION"
output_lacks "--check asks for no install" "run: bash scripts/dev-setup.sh"
rm "$TMP/ok-tools/cargo-audit"
expect "--check fails when one of two is missing" 1 main --check foundation
output_has "the present tool is still ok" "ok       cargo-deny $CARGO_DENY_VERSION"
output_has "the absent tool is missing" "missing  cargo-audit $CARGO_AUDIT_VERSION"
expect "an install run skips the present tool" 1 main --from-dir "$TMP/empty-tools" foundation
output_has "the present tool is skipped" "cargo-deny $CARGO_DENY_VERSION already installed"
output_has "the absent one is looked for" "cargo-audit-x86_64-unknown-linux-musl-v0.23.0.tgz not found"
eval "$real_rust_check"
eval "$real_ndk_check"
eval "$real_cargo_ndk_check"

# --- archives unpacked into a directory (the command-line tools, the NDK) -----
# A made-up zip with a source.properties, as Google's archives have.
mkdir -p "$TMP/zips"
python3 -I - "$TMP/zips/fake-sdk-1.2.zip" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1], "w") as z:
    z.writestr("fake-top/source.properties", "Pkg.Desc = Made up\nPkg.Revision = 1.2\n")
    z.writestr("fake-top/bin/tool", "#!/bin/sh\necho made-up\n")
PY
ZSHA="$(sha256_of "$TMP/zips/fake-sdk-1.2.zip")"
ZSHA1="$(sha1sum "$TMP/zips/fake-sdk-1.2.zip" | cut -d' ' -f1)"
ZURL="https://example.invalid/android/repository/fake-sdk-1.2.zip"
FROM_DIR="$TMP/zips"
TOOLS_HOME="$TMP/zip-home"
ZDEST="$TMP/zip-home/sdk/fake/1.2"
expect "a verified zip installs" 0 install_zip_dir fake-sdk 1.2 "$ZURL" "$ZSHA" "$ZSHA1" fake-top "$ZDEST" 1.2
output_has "it says where" "installed fake-sdk 1.2 into $ZDEST"
expect "the directory reads as ok" 0 test "$(dir_status "$ZDEST" "$ZSHA" 1.2)" = ok
expect "its files are in place" 0 test -f "$ZDEST/bin/tool"
expect "a second run is a no-op" 0 install_zip_dir fake-sdk 1.2 "$ZURL" "$ZSHA" "$ZSHA1" fake-top "$ZDEST" 1.2
output_has "the second run skips" "fake-sdk 1.2 already installed"
echo "0000" >"$ZDEST/.taskdeck-pin"
expect "a directory from another archive reads as differs" 0 test "$(dir_status "$ZDEST" "$ZSHA" 1.2)" = differs
expect "a directory with another revision reads as differs" 0 test "$(dir_status "$ZDEST" "$ZSHA" 1.3)" = differs
expect "a missing directory reads as missing" 0 test "$(dir_status "$TMP/zip-home/none" "$ZSHA" 1.2)" = missing
ZDEST2="$TMP/zip-home/sdk/fake/other"
expect "a wrong SHA-256 is refused" 1 install_zip_dir fake-sdk 1.2 "$ZURL" "$WRONG" "$ZSHA1" fake-top "$ZDEST2" 1.2
output_has "the refusal is loud" "CHECKSUM MISMATCH for fake-sdk 1.2"
expect "nothing is installed after it" 0 test ! -e "$ZDEST2"
expect "a wrong SHA-1 is refused" 1 install_zip_dir fake-sdk 1.2 "$ZURL" "$ZSHA" 0000000000000000000000000000000000000000 fake-top "$ZDEST2" 1.2
output_has "Google's SHA-1 is named" "CHECKSUM MISMATCH for fake-sdk 1.2 (Google's SHA-1)"
expect "nothing is installed after that either" 0 test ! -e "$ZDEST2"
expect "an archive of another revision is refused" 1 install_zip_dir fake-sdk 1.2 "$ZURL" "$ZSHA" "" fake-top "$ZDEST2" 1.3
output_has "the revision is named" "the archive says revision '1.2', pinned 1.3"
expect "and not installed" 0 test ! -e "$ZDEST2"
expect "an archive without the expected top directory is refused" 1 install_zip_dir fake-sdk 1.2 "$ZURL" "$ZSHA" "" other-top "$ZDEST2" 1.2
expect "the real pins name Google's archives" 0 test "$(basename "$NDK_URL")" = "android-ndk-$NDK_RELEASE-linux.zip"
expect "a wrong crate checksum is refused" 1 fetch_verified cargo-ndk 4.1.2 "https://example.invalid/fake-sdk-1.2.zip" "$WRONG" "" "$TMP/crate"

# --- what sdkmanager installs: checked by revision -------------------------------
TOOLS_HOME="$TMP/pkg-home"
expect "no packages read as missing" 0 test "$(tool_android_sdk_packages_check)" = missing
mkdir -p "$(package_dir "$SDK_PLATFORM_PACKAGE")" "$(package_dir "$SDK_BUILD_TOOLS_PACKAGE")"
echo "Pkg.Revision=$SDK_PLATFORM_REVISION" >"$(package_dir "$SDK_PLATFORM_PACKAGE")/source.properties"
echo "Pkg.Revision=$SDK_BUILD_TOOLS_REVISION" >"$(package_dir "$SDK_BUILD_TOOLS_PACKAGE")/source.properties"
expect "the pinned revisions read as ok" 0 test "$(tool_android_sdk_packages_check)" = ok
expect "the platform sits where AGP looks" 0 test -d "$TMP/pkg-home/android-sdk/platforms/android-37.0"
echo "Pkg.Revision=99" >"$(package_dir "$SDK_PLATFORM_PACKAGE")/source.properties"
expect "another platform revision reads as differs" 0 test "$(tool_android_sdk_packages_check)" = differs
expect "the packages need the command-line tools first" 1 tool_android_sdk_packages_install
output_has "it says so" "the command-line tools are not installed"

# --- an NDK the environment names (android-ndk-env) -------------------------------
TOOLS_HOME="$TMP/no-ndk-home"
fake_ndk() {  # fake_ndk <dir> <Pkg.Revision>
    mkdir -p "$1/toolchains/llvm/prebuilt/linux-x86_64/bin"
    printf '#!/bin/sh\n' >"$1/toolchains/llvm/prebuilt/linux-x86_64/bin/clang"
    chmod +x "$1/toolchains/llvm/prebuilt/linux-x86_64/bin/clang"
    echo "Pkg.Revision = $2" >"$1/source.properties"
}
fake_ndk "$TMP/ndk27" 27.3.13750724
fake_ndk "$TMP/ndk29" 29.0.14206865
mkdir -p "$TMP/ndk-empty"
expect "without an NDK in the environment the pinned one is missing" 0 test "$(tool_android_ndk_env_check)" = missing
expect "an NDK older than the pinned release is not used" 0 test "$(ANDROID_NDK_HOME="$TMP/ndk27" tool_android_ndk_env_check)" = missing
expect "nor a directory without clang" 0 test -z "$(ANDROID_NDK_HOME="$TMP/ndk-empty" env_ndk)"
expect "ANDROID_NDK_LATEST_HOME is taken when ANDROID_NDK_HOME is too old" 0 \
    test "$(ANDROID_NDK_HOME="$TMP/ndk27" ANDROID_NDK_LATEST_HOME="$TMP/ndk29" env_ndk)" = "$TMP/ndk29"
expect "a usable NDK in the environment reads as ok" 0 test "$(ANDROID_NDK_LATEST_HOME="$TMP/ndk29" tool_android_ndk_env_check)" = ok
expect "and is not downloaded" 0 env ANDROID_NDK_LATEST_HOME="$TMP/ndk29" bash -c "source '$DEV'; set +e; TOOLS_HOME='$TMP/no-ndk-home'; tool_android_ndk_env_install"
output_has "it says which NDK it uses" "using the NDK 29.0.14206865 that the environment names: $TMP/ndk29"
expect "the exports name it" 0 env ANDROID_NDK_LATEST_HOME="$TMP/ndk29" TASKDECK_TOOLS_HOME="$TMP/no-ndk-home" bash "$DEV" --env
output_has "as ANDROID_NDK_HOME" "export ANDROID_NDK_HOME=\"$TMP/ndk29\""
expect "without one the pinned NDK would be installed" 1 env TASKDECK_TOOLS_HOME="$TMP/no-ndk-home" \
    bash -c "source '$DEV'; set +e; TOOLS_HOME='$TMP/no-ndk-home'; FROM_DIR='$TMP/empty-tools'; tool_android_ndk_env_install"
output_has "it looks for the pinned archive" "android-ndk-$NDK_RELEASE-linux.zip not found"
expect "--check android-targets reports the environment's NDK" 0 env ANDROID_NDK_LATEST_HOME="$TMP/ndk29" \
    bash -c "source '$DEV'; set +e; tool_rust_toolchain_check() { echo ok; }; tool_cargo_ndk_check() { echo ok; }; TOOLS_HOME='$TMP/no-ndk-home'; main --check android-targets"
output_has "it is ok" "ok       android-ndk-env named by ANDROID_NDK_HOME or ANDROID_NDK_LATEST_HOME (r28 or later)"
expect "--check supply-chain looks at the two tools only" 1 env TASKDECK_TOOLS_DIR="$TMP/empty-tools" bash "$DEV" --check supply-chain
output_has "cargo-deny" "missing  cargo-deny"
output_lacks "no NDK" "android-ndk"
output_lacks "no cargo-ndk" "cargo-ndk"
output_has "it names its own row" "run: bash scripts/dev-setup.sh supply-chain"

# --- the exports ----------------------------------------------------------------
TOOLS_HOME="$TMP/env-home"
TOOLS_DIR="$TMP/empty-tools"
expect "--env prints nothing when nothing is installed" 0 test -z "$(env_lines)"
# A tool directory that holds a tool and is not on PATH is put on it, so the
# scripts that eval the exports find cargo-ndk.
mkdir -p "$TMP/off-path"
printf '#!/bin/sh\n' >"$TMP/off-path/cargo-ndk"
chmod +x "$TMP/off-path/cargo-ndk"
expect "a tool directory off PATH is exported" 0 env TASKDECK_TOOLS_DIR="$TMP/off-path" TASKDECK_TOOLS_HOME="$TMP/env-home" bash "$DEV" --env
output_has "on PATH" "export PATH=\"$TMP/off-path:\$PATH\""
expect "one on PATH is not" 0 env PATH="$TMP/off-path:$PATH" TASKDECK_TOOLS_DIR="$TMP/off-path" TASKDECK_TOOLS_HOME="$TMP/env-home" bash "$DEV" --env
output_lacks "no PATH line" "off-path:"
mkdir -p "$(NDK_DIR)"
echo "$NDK_ARCHIVE_SHA256" >"$(NDK_DIR)/.taskdeck-pin"
echo "Pkg.Revision = $NDK_REVISION" >"$(NDK_DIR)/source.properties"
expect "an installed NDK is exported" 0 env_lines
output_has "as ANDROID_NDK_HOME" "export ANDROID_NDK_HOME=\"$TMP/env-home/android-sdk/ndk/$NDK_REVISION\""
output_has "with ANDROID_HOME" "export ANDROID_HOME=\"$TMP/env-home/android-sdk\""
expect "the command line prints the same" 0 env TASKDECK_TOOLS_HOME="$TMP/env-home" bash "$DEV" --env
output_has "the NDK export" "export ANDROID_NDK_HOME="

# --- wrangler -------------------------------------------------------------------
TOOLS_HOME="$TMP/wr-home"
expect "the lockfile records the pinned wrangler" 0 test "$(lock_wrangler "$SCRIPTS_DIR/wrangler/package-lock.json")" = "$WRANGLER_VERSION $WRANGLER_INTEGRITY"
expect "package.json asks for exactly that version" 0 grep -q "\"wrangler\": \"$WRANGLER_VERSION\"" "$SCRIPTS_DIR/wrangler/package.json"
expect "wrangler is refused outside CI and a cloud session" 1 env -u CLAUDE_CODE_REMOTE -u GITHUB_ACTIONS bash -c "source '$DEV'; set +e; TOOLS_HOME='$TMP/wr-home'; IN_CONTAINER=0; tool_wrangler_install"
output_has "the refusal names the way out" "refusing to install outside CI or a cloud session"
expect "nothing is installed after the refusal" 0 test ! -e "$TMP/wr-home/wrangler-$WRANGLER_VERSION"
expect "the flag allows it, but a lockfile that is not the pinned one is refused" 1 env -u CLAUDE_CODE_REMOTE -u GITHUB_ACTIONS bash -c "source '$DEV'; set +e; TOOLS_HOME='$TMP/wr-home'; IN_CONTAINER=1; WRANGLER_INTEGRITY=sha512-made-up; tool_wrangler_install"
output_has "the lockfile mismatch is named" "the lockfile records '$WRANGLER_VERSION $WRANGLER_INTEGRITY', pinned '$WRANGLER_VERSION sha512-made-up'"
expect "an absent wrangler reads as missing" 0 test "$(tool_wrangler_check)" = missing
mkdir -p "$TMP/wr-home/wrangler-$WRANGLER_VERSION"
expect "a wrangler directory without the lockfile reads as differs" 0 test "$(tool_wrangler_check)" = differs

finish
