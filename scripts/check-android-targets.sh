#!/usr/bin/env bash
# The skeletons compile for the Android targets (SYNC_PLAN.md §6.1 gate: "skeletons
# compile on Windows, macOS, Linux and for the Android targets").
#
#   bash scripts/check-android-targets.sh
#
# For each crate of SYNC_PLAN.md §4 it asks cargo whether the crate's
# dependency tree for the Android targets builds C code (holds the `cc` crate,
# as ring does):
#   - a crate without C code is checked with plain `cargo check --target` for
#     the three targets, which needs no NDK;
#   - a crate with C code needs the NDK's C compiler: it is checked with
#     `cargo ndk` for the three ABIs at the API level of minSdk (26,
#     SYNC_PLAN.md §2). Without an NDK and cargo-ndk this FAILS and names the
#     dev-setup command; it never skips.
# It takes the exports of scripts/dev-setup.sh --env (the tool directory on
# PATH, the NDK it installed or found) and only those: an ANDROID_NDK_HOME that
# dev-setup finds unusable is dropped and named. Install what it needs with:
#   bash scripts/dev-setup.sh android-targets
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$here")"
cd "$root" || exit 2

CRATES="taskdeck-core taskdeck-seal taskdeck-sync taskdeck-device taskdeck-backup taskdeck-recover taskdeck-android"
TARGETS="aarch64-linux-android armv7-linux-androideabi x86_64-linux-android"
ABIS="arm64-v8a armeabi-v7a x86_64"
API_LEVEL=26

ndk_given="${ANDROID_NDK_HOME:-}"
exports="$(bash "$here/dev-setup.sh" --env)" || { echo "check-android-targets: FAILED: dev-setup.sh --env"; exit 1; }
unset ANDROID_NDK_HOME
eval "$exports"

target_args=() abi_args=()
for t in $TARGETS; do target_args+=(--target "$t"); done
for a in $ABIS; do abi_args+=(-t "$a"); done

plain=() with_c=()
for c in $CRATES; do
    tree="$(cargo tree --locked -q -p "$c" "${target_args[@]}" -e normal,build --prefix none --format '{p}')" \
        || { echo "check-android-targets: FAILED: cargo tree -p $c"; exit 1; }
    if grep -q '^cc v' <<<"$tree"; then with_c+=("$c"); else plain+=("$c"); fi
done
echo "check-android-targets: without C code (cargo check): ${plain[*]:-none}"
echo "check-android-targets: with C code (cargo ndk, API $API_LEVEL): ${with_c[*]:-none}"

status=0
if [[ ${#plain[@]} -gt 0 ]]; then
    pargs=()
    for c in "${plain[@]}"; do pargs+=(-p "$c"); done
    echo "+ cargo check --locked ${pargs[*]} ${target_args[*]}"
    cargo check --locked "${pargs[@]}" "${target_args[@]}" || status=1
fi
if [[ ${#with_c[@]} -gt 0 ]]; then
    if [[ -z "${ANDROID_NDK_HOME:-}" || ! -d "${ANDROID_NDK_HOME:-}" ]] || ! command -v cargo-ndk >/dev/null; then
        if [[ -n "$ndk_given" && -z "${ANDROID_NDK_HOME:-}" ]]; then
            echo "check-android-targets: ANDROID_NDK_HOME=$ndk_given names no NDK that dev-setup.sh accepts (env_ndk: the pinned major release or later, with its Linux clang)"
        fi
        echo "check-android-targets: FAILED: ${with_c[*]} need the NDK and cargo-ndk, which are not installed"
        echo "check-android-targets: run: bash scripts/dev-setup.sh android-targets"
        exit 1
    fi
    cargs=()
    for c in "${with_c[@]}"; do cargs+=(-p "$c"); done
    echo "+ cargo ndk ${abi_args[*]} --platform $API_LEVEL check --locked ${cargs[*]}   (ANDROID_NDK_HOME=$ANDROID_NDK_HOME)"
    cargo ndk "${abi_args[@]}" --platform "$API_LEVEL" check --locked "${cargs[@]}" || status=1
fi
if [[ $status -ne 0 ]]; then
    echo "check-android-targets: FAILED"
    exit 1
fi
echo "check-android-targets: ok"
