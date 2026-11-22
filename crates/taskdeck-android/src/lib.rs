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
#[cfg(feature = "backend-device")]
use {rustls_platform_verifier as _, taskdeck_core as _, taskdeck_device as _};

pub mod envelope;
mod jni_entry;

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

/// The device backend: the facade and the request-key cache (bridge.md §11.3, §21).
#[cfg(feature = "backend-device")]
#[derive(Clone)]
pub struct DeviceBackend {}

#[cfg(feature = "backend-device")]
impl DeviceBackend {
    /// Opens the facade; the JSON is `open`'s result (bridge.md §11.3).
    pub fn open(config: &OpenConfig) -> Result<(Self, serde_json::Value), BridgeError> {
        unimplemented!()
    }
}

/// What the page's routes read from the facade (bridge.md §6, §21).
#[cfg(feature = "backend-device")]
#[derive(Clone)]
pub struct PageFacade {}

#[cfg(feature = "backend-device")]
impl PageFacade {
    /// The `/api/state` body: the snapshot JSON of `core.md` §14.3 (bridge.md §6.4.1, §21).
    pub fn answer_snapshot(&self, from: chrono::NaiveDate, days: u32, now_ms: i64) -> serde_json::Value {
        unimplemented!()
    }

    /// Today in the device's zone at `now_ms` (bridge.md §6.4.1, §21).
    pub fn today(&self, now_ms: i64) -> chrono::NaiveDate {
        unimplemented!()
    }

    /// The weather route's body (bridge.md §6.4.2).
    pub fn weather(&self) -> Option<serde_json::Value> {
        unimplemented!()
    }

    /// The weather place (bridge.md §6.4.5).
    pub fn weather_place(&self) -> (taskdeck_device::WeatherSource, Option<taskdeck_core::board::Home>) {
        unimplemented!()
    }

    /// The id of the picture shown (bridge.md §7.4).
    pub fn picture_id(&self) -> Option<String> {
        unimplemented!()
    }

    /// Submits an intent given as JSON (bridge.md §6.3).
    pub fn submit_json(&self, text: &str) -> Result<taskdeck_device::Submitted, taskdeck_device::DeviceError> {
        unimplemented!()
    }

    /// Sets the look dials (bridge.md §6.4.3).
    pub fn set_look_dials(&self, blur: u8, light: u8) -> Result<Option<String>, taskdeck_device::DeviceError> {
        unimplemented!()
    }

    /// Sets the phone's own picture (bridge.md §6.4.4).
    pub fn set_own_picture(&self, bytes: Vec<u8>) -> Result<Option<String>, taskdeck_device::DeviceError> {
        unimplemented!()
    }

    /// Clears the phone's own picture (bridge.md §6.4.4).
    pub fn clear_own_picture(&self) -> Result<Option<String>, taskdeck_device::DeviceError> {
        unimplemented!()
    }

    /// Dismisses a notice (bridge.md §6.4.6).
    pub fn dismiss_notice(&self, seq: u64, index: u32) -> Result<(), taskdeck_device::DeviceError> {
        unimplemented!()
    }

    /// The pending notes merge (bridge.md §6.4.6).
    pub fn notes_merge(&self) -> Option<taskdeck_device::NotesMerge> {
        unimplemented!()
    }

    /// Resolves the notes merge (bridge.md §6.4.6).
    pub fn resolve_notes(&self, text: String) -> Result<taskdeck_device::Submitted, taskdeck_device::DeviceError> {
        unimplemented!()
    }

    /// Discards the notes draft (bridge.md §6.4.6).
    pub fn discard_notes_draft(&self) -> Result<(), taskdeck_device::DeviceError> {
        unimplemented!()
    }

    /// The change counters (bridge.md §11.2).
    pub fn changes(&self) -> taskdeck_device::Changes {
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

/// The key store over the Android Keystore (bridge.md §11.5, §12.5).
#[cfg(feature = "backend-device")]
pub struct AndroidKeyStore {}

#[cfg(feature = "backend-device")]
impl taskdeck_device::KeyStore for AndroidKeyStore {
    fn kind(&self) -> u8 {
        unimplemented!()
    }
    fn create(
        &self,
        board: &[u8; 16],
        dev: &[u8; 16],
        key: &zeroize::Zeroizing<[u8; 32]>,
    ) -> Result<(), taskdeck_device::KeyStoreError> {
        unimplemented!()
    }
    fn load(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<zeroize::Zeroizing<[u8; 32]>, taskdeck_device::KeyStoreError> {
        unimplemented!()
    }
    fn delete(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<(), taskdeck_device::KeyStoreError> {
        unimplemented!()
    }
}

/// The reminder records the shell schedules (bridge.md §15.2).
// Behind `backend-device`: `taskdeck-core` is a dependency only there (bridge.md §12.4).
#[cfg(feature = "backend-device")]
pub mod reminders {
    /// One reminder as a §15.2 record (bridge.md §15.2).
    pub fn record_json(r: &taskdeck_core::reminders::Reminder, zone: &dyn taskdeck_core::time::Zone) -> serde_json::Value {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_shell_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "taskdeck-android");
    }
}
