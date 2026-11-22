//! What only the `backend-device` flavour compiles (bridge.md §11.1, §21):
//! the device backend, the page facade, the Android key store and the
//! reminder records. Stage `android` owns this file; stage `android-shell`
//! owns the rest of the crate (lead ruling 10 of the tooling task: the
//! `android-shell` gate's scans leave this file out). `lib.rs` declares it
//! behind the feature and re-exports every item at the public path bridge.md
//! §21 gives it (`taskdeck::DeviceBackend`, `taskdeck::reminders`, ...).

use crate::{BridgeError, OpenConfig};
// Declared for stage `android`, as in lib.rs: each dependency of the feature
// is named once so that it is resolved and compiled now.
use {rustls_platform_verifier as _, taskdeck_core as _, taskdeck_device as _};

/// The device backend: the facade and the request-key cache (bridge.md §11.3, §21).
#[derive(Clone)]
pub struct DeviceBackend {}

impl DeviceBackend {
    /// Opens the facade; the JSON is `open`'s result (bridge.md §11.3).
    pub fn open(config: &OpenConfig) -> Result<(Self, serde_json::Value), BridgeError> {
        unimplemented!()
    }
}

/// What the page's routes read from the facade (bridge.md §6, §21).
#[derive(Clone)]
pub struct PageFacade {}

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

/// The key store over the Android Keystore (bridge.md §11.5, §12.5).
pub struct AndroidKeyStore {}

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
pub mod reminders {
    /// One reminder as a §15.2 record (bridge.md §15.2).
    pub fn record_json(r: &taskdeck_core::reminders::Reminder, zone: &dyn taskdeck_core::time::Zone) -> serde_json::Value {
        unimplemented!()
    }
}
