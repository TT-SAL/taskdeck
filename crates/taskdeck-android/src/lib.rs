//! `taskdeck-android`: `libtaskdeck.so`, the JNI library of the Android app.
//! The bridge envelope, the backends behind it, the Keystore key store and the
//! JNI entry points.
//!
//! Spec: `docs/sync/spec/bridge.md` (§11, §21). Owner: stage `android-shell`,
//! then `android` (SYNC_PLAN.md §4). Created as a shell by stage
//! `foundation`; the skeleton types come from the spec's Public API (§21).
//! Every body is `unimplemented!()` until those stages fill it:
//! `grep -rn 'unimplemented!()' crates/taskdeck-android` lists what is left.
//!
//! `unsafe` is denied: the android stages allow it in `jni_entry` alone
//! (BRIDGE-GEN-001). There is none yet.
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
// Skeleton (task 4): the bodies are `unimplemented!()`, so the parameters the
// spec names are not read yet. The owning stage removes this.
#![allow(unused_variables, dead_code)]

// Exactly one backend (bridge.md §11.1, BRIDGE-GEN-011, ruling S32).
#[cfg(all(feature = "backend-echo", feature = "backend-device"))]
compile_error!("choose one backend");
#[cfg(not(any(feature = "backend-echo", feature = "backend-device")))]
compile_error!("choose a backend");

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

pub mod envelope;
mod jni_entry;
// Stage `android`'s part of the crate, in a file of its own (lead ruling 10 of
// the tooling task), with every item at its bridge.md §21 path.
#[cfg(feature = "backend-device")]
mod device_backend;
#[cfg(feature = "backend-device")]
pub use device_backend::{AndroidKeyStore, DeviceBackend, PageFacade, reminders};

/// What answers the page and the shell (bridge.md §6, §11.2, §21).
pub trait Backend: Send + Sync {
    /// Answers a page request: the Rust rows of the route table (bridge.md §6.2).
    fn request(&self, request: &envelope::Request) -> envelope::Reply;
    /// Answers a `call` (bridge.md §11.4).
    fn call(&self, name: &str, args: &serde_json::Value) -> Result<serde_json::Value, BridgeError>;
    /// Answers a `callSecret` (bridge.md §11.2, §11.4).
    fn call_secret(
        &self,
        name: &str,
        args: &serde_json::Value,
        code: zeroize::Zeroizing<String>,
    ) -> Result<serde_json::Value, BridgeError>;
    /// The change counters as JSON (bridge.md §11.2).
    fn changes(&self) -> serde_json::Value;
    /// Waits for a change; the counters as JSON (bridge.md §11.2).
    fn wait_for_change(&self, since_any: u64, timeout_ms: u32) -> serde_json::Value;
    /// The `look` bytes of a picture (bridge.md §11.2, §7.4).
    fn picture(&self, id: &str) -> Option<std::sync::Arc<Vec<u8>>>;
}

/// A bridge error: a code of the catalogue (bridge.md §17).
pub struct BridgeError {
    pub code: &'static str,
    pub message: Option<String>,
}

// impl Backend for EchoBackend (stage android-shell, §6.7) and impl Backend for DeviceBackend (stage android) are
// those stages' to write; the skeleton carries the two structs without them (bridge.md §21).

/// The echo backend (bridge.md §6.7).
#[cfg(feature = "backend-echo")]
#[derive(Clone)]
pub struct EchoBackend {}

#[cfg(feature = "backend-echo")]
impl EchoBackend {
    /// The echo backend over the fixtures (bridge.md §6.7).
    pub fn from_fixtures() -> Self {
        unimplemented!()
    }
}

/// The configuration of `open` (bridge.md §11.3).
pub struct OpenConfig {
    pub data_dir: std::path::PathBuf,
    pub api: u32,
    pub build_id: String,
    pub device_name: String,
    pub trust: Trust,
    pub zone: ZoneJson,
}

/// The trust roots `open` asks for (bridge.md §11.3, §12.6).
pub enum Trust {
    Platform,
    Bundled,
}

/// A zone as Kotlin builds it (bridge.md §12.7).
pub struct ZoneJson {
    pub initial: i32,
    pub transitions: Vec<(i64, i32)>,
}

/// Parses `open`'s configuration (bridge.md §11.3).
pub fn parse_open_config(text: &str) -> Result<OpenConfig, BridgeError> {
    unimplemented!()
}

/// Parses a zone (bridge.md §12.7 step 4).
pub fn parse_zone(value: &serde_json::Value) -> Result<ZoneJson, BridgeError> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_shell_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "taskdeck-android");
    }

    // The items of device_backend.rs keep the paths bridge.md §21 gives them.
    #[cfg(feature = "backend-device")]
    #[test]
    fn the_device_items_keep_their_paths() {
        let _ = crate::DeviceBackend::open;
        let _ = crate::PageFacade::changes;
        let _ = <crate::AndroidKeyStore as taskdeck_device::KeyStore>::kind;
        let _ = crate::reminders::record_json;
    }
}
