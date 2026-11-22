//! `taskdeck-android`: `libtaskdeck.so`, the JNI library of the Android app.
//! The bridge envelope, the backends behind it, the Keystore key store and the
//! JNI entry points.
//!
//! Spec: `docs/sync/spec/bridge.md` (§11, §21). Owner: stage `android-shell`,
//! then `android` (SYNC_PLAN.md §4). Created as a shell by stage
//! `foundation`; the skeleton types come from the spec's Public API (§21).
//!
//! `unsafe` is denied: the android stages allow it in `jni_entry` alone
//! (BRIDGE-GEN-001). There is none yet.
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

// Declared for the stage that fills this crate: each dependency is named once
// so that it is resolved and compiled now, and a wrong feature set fails today
// rather than in wave 1.
use base64 as _;
use jni as _;
use serde_json as _;
use zeroize as _;
const _: () = {
    #[allow(dead_code)] // never built: it only proves the derives compile
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Declared(u64);
    let _ = <sha2::Sha256 as sha2::Digest>::new;
};
#[cfg(feature = "backend-device")]
use {rustls_platform_verifier as _, taskdeck_core as _, taskdeck_device as _};

#[cfg(test)]
mod tests {
    #[test]
    fn the_shell_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "taskdeck-android");
    }
}
