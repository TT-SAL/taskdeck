//! `taskdeck-device`: the device runtime. The sealed store, the transport, the
//! key stores, the fetchers, blobs, backup scheduling, the facade, and the
//! adapters that implement `taskdeck_sync::StateMachine` over `taskdeck-core`
//! and `taskdeck_sync::Sealer` over `taskdeck-seal` (decision D6).
//!
//! Spec: `docs/sync/spec/device.md`. Owner: stage `device` (SYNC_PLAN.md §4).
//! Created as a shell by stage `foundation`; the skeleton types come from the
//! spec's Public API (§25). Every body is `unimplemented!()` until stage
//! `device` fills it: `grep -rn 'unimplemented!()' crates/taskdeck-device`
//! lists what is left.
//!
//! `unsafe` is denied: the device stage allows it in `platform::windows` alone
//! (DEV-GEN-001). There is none yet.
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
// Skeleton (task 4): the bodies are `unimplemented!()`, so the parameters the
// spec names and the private fields of the opaque types are not read yet.
// The owning stage removes this.
#![allow(unused_variables, dead_code)]

// Declared for the stage that fills this crate: each dependency is named once
// so that it is resolved and compiled now, and a wrong feature set fails today
// rather than in wave 1.
use base64 as _;
use flate2 as _;
use serde_json as _;
use subtle as _;
use taskdeck_backup as _;
use taskdeck_core as _;
use taskdeck_seal as _;
use taskdeck_sync as _;
use url as _;
use zeroize as _;
const _: () = {
    #[allow(dead_code)] // never built: it only proves the derives compile
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Declared(chrono::DateTime<chrono::FixedOffset>);
    // `clock`: the device reads the clock and the local zone.
    let _ = chrono::Local::now;
    let _ = <sha2::Sha256 as sha2::Digest>::new;
    // `jpeg` and `png`.
    let _ = image::ImageFormat::Jpeg;
    let _ = image::codecs::jpeg::JpegEncoder::<Vec<u8>>::new_with_quality;
    let _ = image::codecs::png::PngDecoder::<std::io::Cursor<Vec<u8>>>::new;
    // The ring provider, and TLS 1.2 next to 1.3.
    let _ = rustls::crypto::ring::default_provider;
    let _ = &rustls::version::TLS12;
    let _ = rustls_platform_verifier::Verifier::new;
    // ureq over rustls with the platform verifier.
    let _ = ureq::tls::RootCerts::PlatformVerifier;
    let _ = ureq::tls::TlsProvider::Rustls;
    // The client handshake over the device's own rustls stream.
    let _ = tungstenite::client::client::<
        rustls::StreamOwned<rustls::ClientConnection, std::net::TcpStream>,
        tungstenite::handshake::client::Request,
    >;
};
#[cfg(windows)]
const _: () = {
    let _ = windows::Win32::Security::Cryptography::CryptProtectData::<windows::core::PCWSTR>;
    let _ = windows::Win32::Security::Cryptography::CryptUnprotectData;
    let _ = windows::Win32::Foundation::LocalFree;
    let _ = windows::Win32::Storage::FileSystem::MoveFileExW::<windows::core::PCWSTR, windows::core::PCWSTR>;
};
#[cfg(target_os = "macos")]
const _: () = {
    let _ = security_framework::passwords::get_generic_password;
};

pub mod recovery_online;
pub mod store;

// ---------- opening (§5, §17.2)

/// The facade: one open data folder (device.md §17.2, §4.5).
#[derive(Clone)]
pub struct Device {
    inner: std::sync::Arc<Inner>,
}

/// What a `Device` shares between its clones (device.md §4.5).
struct Inner {}

/// The platform the facade runs on (device.md §17.2).
pub enum Platform {
    Windows,
    MacOs,
    Linux,
    Android { api: u32 },
}

/// Where the local zone comes from (device.md §17.2).
pub enum ZoneSource {
    System,
    Table(taskdeck_core::time::TableZone),
}

/// The TLS trust roots (device.md §10.1, §11.1 item 2).
pub enum TrustRoots {
    Platform,
    Bundled,
    #[cfg(feature = "test-trust")]
    Extra(Vec<u8>),
}

/// The arguments of `Device::open` (device.md §17.2).
pub struct OpenOptions {
    pub data_dir: std::path::PathBuf,
    pub platform: Platform,
    pub key_store: std::sync::Arc<dyn KeyStore>,
    pub zone: ZoneSource,
    pub trust: TrustRoots,
    pub build_id: String,
    pub default_device_name: String,
    #[cfg(feature = "test-trust")]
    pub clock: Option<std::sync::Arc<dyn TestClock>>,
    #[cfg(feature = "test-trust")]
    pub files: Option<std::sync::Arc<dyn FileLayer>>,
}

/// The mode of an open data folder (device.md §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    LocalOnly,
    Synced,
    KeyLost,
}

/// The active identity as the facade reports it (device.md §17.2).
pub struct IdentityInfo {
    pub board_id: [u8; 16],
    pub dev: [u8; 16],
    pub device_name: String,
    pub relay_address: Option<String>,
    pub key_fp: Option<String>,
}

/// An identity recorded in `store.tds` (device.md §17.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredIdentity {
    pub board_id: [u8; 16],
    pub dev: [u8; 16],
    pub key_store: u8,
    pub created_ms: i64,
    pub active: bool,
}

/// A warning `open` reports (device.md §17.2, §7.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenWarning {
    FolderLocked,
    FolderUnlockable,
    TornTail { kind: u8 },
    BadTail { kind: u8 },
    Quarantined { kind: u8, file: String },
    SetAside { folder: String },
    PendingCreate,
}

/// What `open` found (device.md §17.2).
pub struct OpenReport {
    pub mode: Mode,
    pub warnings: Vec<OpenWarning>,
    pub import: Option<taskdeck_core::legacy::ImportReport>,
    pub identity: Option<IdentityInfo>,
}

/// A facade error: a code of the catalogue (device.md §21, §17.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceError {
    pub code: &'static str,
    pub status: Option<u16>,
    pub message: Option<String>,
}

impl Device {
    /// Opens a data folder (device.md §17.2).
    pub fn open(options: OpenOptions) -> Result<(Device, OpenReport), DeviceError> {
        unimplemented!()
    }

    /// The current mode (device.md §17.2).
    pub fn mode(&self) -> Mode {
        unimplemented!()
    }

    /// Stops the runtime (device.md §17.2).
    pub fn shutdown(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Removes an inactive identity (device.md §17.2).
    pub fn forget_identity(&self, board: [u8; 16], dev: [u8; 16]) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// The identities of `store.tds` (device.md §17.2).
    pub fn identities(&self) -> Vec<StoredIdentity> {
        unimplemented!()
    }

    // ---------- change notification (§17.3)

    /// Sets the waker the runtime calls on a change (device.md §17.3).
    pub fn set_waker(&self, waker: std::sync::Arc<dyn Fn() + Send + Sync>) {
        unimplemented!()
    }

    /// The change counters (device.md §17.3).
    pub fn changes(&self) -> Changes {
        unimplemented!()
    }

    /// Waits until `any` passes `since_any` or the timeout (device.md §17.3).
    pub fn wait_for_change(&self, since_any: u64, timeout_ms: u32) -> Changes {
        unimplemented!()
    }

    // ---------- intents (§17.4)

    /// Submits an intent (device.md §17.4).
    pub fn submit(&self, intent: taskdeck_core::intent::Intent) -> Result<Submitted, DeviceError> {
        unimplemented!()
    }

    /// Submits a named intent, held for coalescing (device.md §17.4).
    pub fn submit_named(&self, intent: taskdeck_core::intent::Intent) -> Result<Submitted, DeviceError> {
        unimplemented!()
    }

    /// Releases the held intent (device.md §17.4).
    pub fn release_hold(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Submits an intent given as JSON (device.md §17.4).
    pub fn submit_json(&self, text: &str) -> Result<Submitted, DeviceError> {
        unimplemented!()
    }

    // ---------- reads (§17.5)

    /// The published view (device.md §17.5, §4.4).
    pub fn view(&self) -> std::sync::Arc<Published> {
        unimplemented!()
    }

    /// The device status (device.md §17.8).
    pub fn status(&self) -> DeviceStatus {
        unimplemented!()
    }

    // ---------- notes (§17.6)

    /// The notes draft (device.md §17.6).
    pub fn notes_draft(&self) -> Option<Draft> {
        unimplemented!()
    }

    /// Replaces the notes draft (device.md §17.6).
    pub fn set_notes_draft(&self, text: String) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Submits the notes draft (device.md §17.6).
    pub fn flush_notes_draft(&self) -> Result<Option<Submitted>, DeviceError> {
        unimplemented!()
    }

    /// The pending notes merge (device.md §17.6).
    pub fn notes_merge(&self) -> Option<NotesMerge> {
        unimplemented!()
    }

    /// Resolves the notes merge (device.md §17.6).
    pub fn resolve_notes(&self, text: String) -> Result<Submitted, DeviceError> {
        unimplemented!()
    }

    /// Discards the notes draft (device.md §17.6).
    pub fn discard_notes_draft(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    // ---------- notices, banners, alarms, halted view (§17.7)

    /// Dismisses a notice (device.md §17.7).
    pub fn dismiss_notice(&self, seq: u64, index: u32) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Acknowledges a banner (device.md §17.7).
    pub fn ack_banner(&self, id: (u64, u8)) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Dismisses an alarm (device.md §17.7).
    pub fn dismiss_alarm(&self, id: u64) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Confirms the clock (device.md §17.7).
    pub fn confirm_clock(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Confirms a rejoin (device.md §17.7).
    pub fn confirm_rejoin(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Acknowledges a continuity warning (device.md §17.7).
    pub fn ack_continuity(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Acknowledges a restored copy (device.md §17.7).
    pub fn ack_restored_copy(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Acknowledges a base that was not checked (device.md §17.7).
    pub fn ack_based_on_unchecked(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Keeps this copy in the halted view (device.md §17.7).
    pub fn use_this_copy(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Adopts a device; the result is a ticket (device.md §17.7).
    pub fn adopt(&self, dev: [u8; 16], typed_fp: &str) -> Result<u64, DeviceError> {
        unimplemented!()
    }

    /// Skips an entry; the result is a ticket (device.md §17.7).
    pub fn skip(&self, seq: u64) -> Result<u64, DeviceError> {
        unimplemented!()
    }

    // ---------- devices and operations (§17.8)

    /// Revokes a device; the result is a ticket (device.md §17.8).
    pub fn revoke(&self, dev: [u8; 16], replace: bool) -> Result<u64, DeviceError> {
        unimplemented!()
    }

    /// The restore targets `(seq, ms)` (device.md §17.8).
    pub fn restore_targets(&self) -> Vec<(u64, i64)> {
        unimplemented!()
    }

    /// Restores to a target; the result is a ticket (device.md §17.8).
    pub fn restore_to(&self, seq: u64) -> Result<u64, DeviceError> {
        unimplemented!()
    }

    /// What a purge would reach (device.md §17.8).
    pub fn purge_preview(&self) -> Result<PurgePreview, DeviceError> {
        unimplemented!()
    }

    /// Purges; the result is a ticket (device.md §17.8).
    pub fn purge(&self) -> Result<u64, DeviceError> {
        unimplemented!()
    }

    /// Overrides the rules version; the result is a ticket (device.md §17.8).
    pub fn rules_override(&self, version: u32) -> Result<u64, DeviceError> {
        unimplemented!()
    }

    /// Moves the board to another relay; the result is a ticket (device.md §17.8).
    pub fn move_board(&self, host: &str, create_secret: zeroize::Zeroizing<String>) -> Result<u64, DeviceError> {
        unimplemented!()
    }

    /// The state of a move (device.md §17.8).
    pub fn move_status(&self) -> MoveStatus {
        unimplemented!()
    }

    /// Enters a move code (device.md §17.8).
    pub fn enter_move_code(&self, text: &str) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Follows a moved board; the result is a ticket (device.md §17.8).
    pub fn follow_address(&self, address: &str, code: &str) -> Result<u64, DeviceError> {
        unimplemented!()
    }

    /// Waits for an operation (device.md §17.8).
    pub fn wait_op(&self, ticket: u64, timeout_ms: u32) -> Option<Result<(), &'static str>> {
        unimplemented!()
    }

    /// The running operations (device.md §17.8).
    pub fn ops(&self) -> Vec<OpView> {
        unimplemented!()
    }

    // ---------- platform (§17.9)

    /// Reports whether the UI is visible (device.md §17.9).
    pub fn set_foreground(&self, visible: bool) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Reports a resume (device.md §17.9).
    pub fn notify_resume(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Syncs within a budget (device.md §17.9).
    pub fn background_sync(&self, budget_ms: u32) -> Result<BackgroundOutcome, DeviceError> {
        unimplemented!()
    }

    /// Replaces the zone (device.md §17.9).
    pub fn set_zone(&self, zone: taskdeck_core::time::TableZone) -> Result<(), DeviceError> {
        unimplemented!()
    }

    // ---------- flows (§17.10, §18)

    /// Begins creating a board (device.md §17.10, §18.1).
    pub fn begin_create(&self, request: CreateRequest) -> Result<CreateFlow, DeviceError> {
        unimplemented!()
    }

    /// Resumes a pending create (device.md §17.10, §18.1).
    pub fn resume_create(&self, create_secret: zeroize::Zeroizing<String>) -> Result<CreatedBoard, DeviceError> {
        unimplemented!()
    }

    /// Abandons a pending create (device.md §17.10, §18.1).
    pub fn abandon_create(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Resets trust (device.md §17.10, §18.6).
    pub fn reset_trust(&self, request: CreateRequest) -> Result<CreateFlow, DeviceError> {
        unimplemented!()
    }

    /// Starts pairing a new device (device.md §17.10, §18.3).
    pub fn pair_start(&self) -> Result<PairSheet, DeviceError> {
        unimplemented!()
    }

    /// Confirms the pairing (device.md §17.10, §18.3).
    pub fn pair_confirm(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Cancels the pairing (device.md §17.10, §18.3).
    pub fn pair_cancel(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Joins a board (device.md §17.10, §18.3).
    pub fn join(&self, address: &str, code: zeroize::Zeroizing<String>, device_name: &str) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Enters the SAS digits while joining (device.md §17.10, §18.3).
    pub fn join_sas(&self, digits: &str) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Pairs this device again (device.md §17.10, §18.4).
    pub fn re_pair(&self, address: &str, code: zeroize::Zeroizing<String>, device_name: &str) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Begins the recovery-key change (device.md §17.10, §18.5).
    pub fn begin_recovery_change(&self) -> Result<RecoveryChangeFlow, DeviceError> {
        unimplemented!()
    }

    /// Confirms the new recovery key; the result is a ticket (device.md §17.10, §18.5).
    pub fn confirm_recovery_key(&self) -> Result<u64, DeviceError> {
        unimplemented!()
    }

    /// Cancels the recovery-key change; the result is a ticket (device.md §17.10, §18.5).
    pub fn cancel_recovery_key(&self) -> Result<u64, DeviceError> {
        unimplemented!()
    }

    // ---------- backups (§17.11)

    /// The backup destinations (device.md §17.11, §15.6).
    pub fn backup_destinations(&self) -> Vec<Destination> {
        unimplemented!()
    }

    /// Replaces the backup destinations (device.md §17.11, §15.6).
    pub fn set_backup_destinations(&self, list: Vec<Destination>) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// The backup status (device.md §17.11, §15.6).
    pub fn backup_status(&self) -> BackupStatus {
        unimplemented!()
    }

    /// Writes a backup now (device.md §17.11, §15.2).
    pub fn backup_now(&self) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Pins the recovery fingerprint (device.md §17.11, §15.5).
    pub fn pin_recovery_fingerprint(&self, typed: &str) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Deletes backups older than `before_ms` (device.md §17.11, §15.6).
    pub fn delete_backups_before(&self, before_ms: i64) -> Result<u32, DeviceError> {
        unimplemented!()
    }

    // ---------- picture, settings, export (§17.12)

    /// Sets the shared picture (device.md §17.12, §14.1).
    pub fn set_shared_picture(&self, bytes: Vec<u8>) -> Result<Submitted, DeviceError> {
        unimplemented!()
    }

    /// Clears the shared picture (device.md §17.12, §14.1).
    pub fn clear_shared_picture(&self) -> Result<Submitted, DeviceError> {
        unimplemented!()
    }

    /// Sets the look dials; the result is the id of the look shown (device.md §17.12).
    pub fn set_look_dials(&self, blur: u8, light: u8) -> Result<Option<String>, DeviceError> {
        unimplemented!()
    }

    /// Sets the phone's own picture (device.md §17.12, §14.3).
    pub fn set_own_picture(&self, bytes: Vec<u8>) -> Result<Option<String>, DeviceError> {
        unimplemented!()
    }

    /// Clears the phone's own picture (device.md §17.12, §14.3).
    pub fn clear_own_picture(&self) -> Result<Option<String>, DeviceError> {
        unimplemented!()
    }

    /// The weather place (device.md §17.12, §16).
    pub fn weather_place(&self) -> (WeatherSource, Option<taskdeck_core::board::Home>) {
        unimplemented!()
    }

    /// Sets the weather place (device.md §17.12, §16).
    pub fn set_weather_place(
        &self,
        source: WeatherSource,
        own_place: Option<taskdeck_core::board::Home>,
    ) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Sets the reminder leads (device.md §17.12, §16).
    pub fn set_reminder_leads(&self, leads: taskdeck_core::reminders::ReminderDefaults) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Writes the board as the legacy files (device.md §17.12, §18.7).
    pub fn export_legacy(&self, target: &std::path::Path) -> Result<(), DeviceError> {
        unimplemented!()
    }
}

/// Checks a folder for import (device.md §17.10, §18.2).
pub fn import_check(folder: &std::path::Path) -> Result<ImportCheck, DeviceError> {
    unimplemented!()
}

/// The result of `import_check` (device.md §17.10).
pub struct ImportCheck {
    pub report: taskdeck_core::legacy::ImportReport,
    pub genesis_sha256: String,
}

// ---------- change counters, results, views (§17.3 to §17.8)

/// The change counters (device.md §17.3).
pub struct Changes {
    pub any: u64,
    pub view: u64,
    pub status: u64,
    pub notices: u64,
    pub banners: u64,
    pub overlay: u64,
    pub weather: u64,
    pub picture: u64,
    pub backup: u64,
    pub pairing: u64,
    pub ops: u64,
}

/// The result of a submitted intent (device.md §17.4).
pub struct Submitted {
    pub reply: taskdeck_core::compile::Reply,
    pub entry_id: Option<[u8; 16]>,
    pub provisional_seq: Option<u64>,
    pub view_version: u64,
}

/// A published view (device.md §4.4, §17.5).
pub struct Published {}

impl Published {
    /// The board (device.md §17.5).
    pub fn board(&self) -> &taskdeck_core::board::BoardData {
        unimplemented!()
    }

    /// The view's version (device.md §17.5).
    pub fn version(&self) -> u64 {
        unimplemented!()
    }

    /// The zone the view was built with (device.md §17.5).
    pub fn zone(&self) -> &dyn taskdeck_core::time::Zone {
        unimplemented!()
    }

    /// The ranked item ids (device.md §17.5).
    pub fn ranked_ids(&self, now: taskdeck_core::time::Instant) -> Vec<u64> {
        unimplemented!()
    }

    /// The shuffle seed (device.md §17.5).
    pub fn shuffle_seed(&self, now: taskdeck_core::time::Instant) -> u64 {
        unimplemented!()
    }

    /// The minutes behind on a day (device.md §17.5).
    pub fn behind_minutes(&self, day: chrono::NaiveDate, now: taskdeck_core::time::Instant) -> i32 {
        unimplemented!()
    }

    /// The next free start on a day (device.md §17.5).
    pub fn next_free_start(&self, day: chrono::NaiveDate) -> i32 {
        unimplemented!()
    }

    /// The inputs of a reflow on a day (device.md §17.5).
    #[allow(clippy::type_complexity)] // the spec's signature (device.md §25)
    pub fn reflow_inputs(
        &self,
        day: chrono::NaiveDate,
    ) -> (Vec<taskdeck_core::planner::Flowable<(u64, u64)>>, Vec<(i32, i32)>) {
        unimplemented!()
    }

    /// An item's placements on a day; empty when it is not live (device.md §17.5).
    pub fn placements_for(&self, item: u64, day: chrono::NaiveDate) -> Vec<taskdeck_core::planner::DayPlacement> {
        unimplemented!()
    }

    /// The archive, newest first (device.md §17.5).
    pub fn archive_newest_first(&self) -> Vec<&taskdeck_core::board::ArchiveRow> {
        unimplemented!()
    }

    /// The archive by day (device.md §17.5).
    #[allow(clippy::type_complexity)] // the spec's signature (device.md §25)
    pub fn archive_by_day(
        &self,
        from: chrono::NaiveDate,
        days: u32,
    ) -> Vec<Vec<(&taskdeck_core::board::ArchiveRow, &taskdeck_core::board::RowBlock)>> {
        unimplemented!()
    }

    /// A snapshot of days (device.md §17.5).
    pub fn snapshot(
        &self,
        from: chrono::NaiveDate,
        days: u32,
        now: taskdeck_core::time::Instant,
        ranked: Option<Vec<u64>>,
    ) -> taskdeck_core::snapshot::Snapshot {
        unimplemented!()
    }

    /// A snapshot as the page's answer JSON (device.md §17.5).
    pub fn answer_snapshot(
        &self,
        from: chrono::NaiveDate,
        days: u32,
        now: taskdeck_core::time::Instant,
    ) -> serde_json::Value {
        unimplemented!()
    }

    /// The reminders in a window (device.md §17.5).
    pub fn reminders(
        &self,
        from: taskdeck_core::time::Instant,
        until: taskdeck_core::time::Instant,
    ) -> Vec<taskdeck_core::reminders::Reminder> {
        unimplemented!()
    }

    /// The overlay (device.md §17.5).
    pub fn overlay(&self) -> &taskdeck_core::overlay::Overlay {
        unimplemented!()
    }

    /// The weather: the report, when it was fetched and its place (device.md §17.5).
    pub fn weather(&self) -> Option<(&taskdeck_core::weather::Report, i64, WeatherSource)> {
        unimplemented!()
    }

    /// Each subscription's last good fetch, ascending id (device.md §25, §13.1.2 step 8).
    pub fn overlay_fetched(&self) -> &[(u64, i64)] {
        unimplemented!()
    }

    /// The picture shown (device.md §17.5, §14.2).
    pub fn picture(&self) -> Option<PictureView> {
        unimplemented!()
    }

    /// The pending marks (device.md §17.5).
    pub fn pending_marks(&self) -> &[taskdeck_sync::PendingMark] {
        unimplemented!()
    }

    /// The notices (device.md §17.7).
    pub fn notices(&self) -> &[NoticeView] {
        unimplemented!()
    }
}

/// A picture ready to show (device.md §17.5, §14.2).
pub struct PictureView {
    pub id: String,
    pub jpeg: std::sync::Arc<Vec<u8>>,
    pub top: [u8; 3],
    pub aspect_milli: u32,
}

/// A notice as the facade shows it, engine.md's `NoticeEntry` (device.md §17.7).
pub struct NoticeView {
    pub seq: u64,
    pub index: u32,
    pub dismissed: bool,
    pub record: NoticeRecord,
}

/// A notice or a refusal (device.md §17.7).
pub enum NoticeRecord {
    Notice(taskdeck_core::apply::Notice),
    Refusal(taskdeck_core::apply::RefusalRecord),
}

/// The device status (device.md §17.8).
pub struct DeviceStatus {
    pub engine: Option<taskdeck_sync::Status>,
    pub mode: Mode,
    pub relay_address: Option<String>,
    pub socket_open: bool,
    pub requests_today: u32,
    pub weather_age_ms: Option<i64>,
    pub picture: PictureStatus,
    pub backup: BackupStatus,
    pub pairing: PairState,
    pub device_name: String,
}

/// The state of the shared picture (device.md §17.8, §14.2).
pub enum PictureStatus {
    None,
    Ready,
    Failed { code: &'static str },
}

/// The notes draft (device.md §17.6).
pub struct Draft {
    pub text: String,
    pub base_rev: u64,
    pub edited_ms: i64,
}

/// A notes merge to resolve (device.md §17.6).
pub struct NotesMerge {
    pub mine: String,
    pub theirs: String,
    pub base_rev: u64,
    pub current_rev: u64,
}

/// What a purge would reach (device.md §17.8).
pub struct PurgePreview {
    pub reach_seq: u64,
    pub reach_ms: i64,
}

/// The state of a move (device.md §17.8).
pub struct MoveStatus {
    pub code: Option<String>,
    pub carried_through: Option<u64>,
    pub old_relay_answered: Option<bool>,
}

/// A running operation (device.md §17.8).
pub struct OpView {
    pub ticket: u64,
    pub kind: &'static str,
    pub stage: u8,
    pub started_ms: i64,
}

/// The result of a background sync (device.md §17.9).
pub struct BackgroundOutcome {
    pub synced: bool,
    pub queued: u32,
    pub head: u64,
}

/// The place the weather is for (device.md §16, §13.2).
pub enum WeatherSource {
    Home,
    Own,
}

// ---------- flows (§18)

/// What a new board starts from (device.md §18.1, §18.2).
pub enum CreateSource {
    Empty,
    ThisFolder,
    Folder(std::path::PathBuf),
    CurrentBoard,
}

/// The arguments of a create or a trust reset (device.md §18.1, §18.6).
pub struct CreateRequest {
    pub relay_host: String,
    pub create_secret: zeroize::Zeroizing<String>,
    pub source: CreateSource,
    pub device_name: String,
}

/// A board being created; holds the seed, keys and genesis (device.md §18.1).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct CreateFlow {}

impl CreateFlow {
    /// The recovery sheet (device.md §18.1).
    pub fn sheet(&self) -> SheetData {
        unimplemented!()
    }

    /// The import report of the source (device.md §18.1, §18.2).
    pub fn import_report(&self) -> Option<&taskdeck_core::legacy::ImportReport> {
        unimplemented!()
    }

    /// Checks the typed sheet (device.md §18.1).
    pub fn confirm_sheet(&mut self, typed: &str) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Creates the board (device.md §18.1).
    pub fn finish(self) -> Result<CreatedBoard, DeviceError> {
        unimplemented!()
    }
}

/// What the recovery sheet shows (device.md §18.1, §18.5).
pub struct SheetData {
    pub seed_groups: zeroize::Zeroizing<String>,
    pub relay_address: String,
    pub recovery_fp: String,
    pub release_fp: String,
    pub date: chrono::NaiveDate,
}

/// A created board (device.md §18.1).
pub struct CreatedBoard {
    pub address: String,
}

/// A recovery-key change in progress (device.md §18.5).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct RecoveryChangeFlow {}

impl RecoveryChangeFlow {
    /// The new recovery sheet (device.md §18.5).
    pub fn sheet(&self) -> SheetData {
        unimplemented!()
    }

    /// Checks the typed sheet (device.md §18.5).
    pub fn confirm_sheet(&mut self, typed: &str) -> Result<(), DeviceError> {
        unimplemented!()
    }

    /// Proposes the change; the result is the proposal's ticket (device.md §18.5).
    pub fn finish(self) -> Result<u64, DeviceError> {
        unimplemented!()
    }
}

/// What the pairing device shows (device.md §18.3).
pub struct PairSheet {
    pub address_display: String,
    pub code_display: zeroize::Zeroizing<String>,
}

/// The state of a pairing (device.md §18.3).
pub enum PairState {
    Idle,
    ShowingCode,
    ShowSas { digits: String },
    Registering,
    AskSas,
    Accepted,
    Joining,
    Done,
    Aborted { code: &'static str },
}

// ---------- backups (§15)

/// The kind of a backup destination (device.md §15.6).
pub enum DestinationKind {
    Folder,
    Offline,
}

/// A backup destination (device.md §15.6).
pub struct Destination {
    pub path: std::path::PathBuf,
    pub kind: DestinationKind,
    pub label: String,
}

/// The status of one destination (device.md §15.6).
pub struct DestinationStatus {
    pub path: std::path::PathBuf,
    pub last_good_ms: Option<i64>,
    pub last_attempt_ms: Option<i64>,
    pub last_error: Option<&'static str>,
    pub files: u32,
    pub bytes: u64,
}

/// The backup status (device.md §15.6).
pub struct BackupStatus {
    pub off_reason: Option<&'static str>,
    pub destinations: Vec<DestinationStatus>,
    pub last_nightly_day: Option<chrono::NaiveDate>,
    pub last_segment: Option<(String, u64)>,
    pub alarms: Vec<&'static str>,
    pub pin_needed: Option<String>,
}

// ---------- key stores (§12)

/// A store of storage keys `L_d` (device.md §12.1).
pub trait KeyStore: Send + Sync {
    /// The store's kind number (device.md §12.2).
    fn kind(&self) -> u8;
    /// Stores a key (device.md §12.1).
    fn create(&self, board: &[u8; 16], dev: &[u8; 16], key: &zeroize::Zeroizing<[u8; 32]>) -> Result<(), KeyStoreError>;
    /// Loads a key (device.md §12.1).
    fn load(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<zeroize::Zeroizing<[u8; 32]>, KeyStoreError>;
    /// Deletes a key (device.md §12.1).
    fn delete(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<(), KeyStoreError>;
}

/// A key store failure (device.md §12.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStoreError {
    NotFound,
    CannotUnwrap,
    Unavailable,
    Failed,
}

/// The DPAPI key store (device.md §12.3).
#[cfg(windows)]
pub struct DpapiKeyStore;

#[cfg(windows)]
impl KeyStore for DpapiKeyStore {
    fn kind(&self) -> u8 {
        unimplemented!()
    }
    fn create(&self, board: &[u8; 16], dev: &[u8; 16], key: &zeroize::Zeroizing<[u8; 32]>) -> Result<(), KeyStoreError> {
        unimplemented!()
    }
    fn load(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<zeroize::Zeroizing<[u8; 32]>, KeyStoreError> {
        unimplemented!()
    }
    fn delete(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<(), KeyStoreError> {
        unimplemented!()
    }
}

/// The login Keychain key store (device.md §12.4).
#[cfg(target_os = "macos")]
pub struct KeychainKeyStore;

#[cfg(target_os = "macos")]
impl KeyStore for KeychainKeyStore {
    fn kind(&self) -> u8 {
        unimplemented!()
    }
    fn create(&self, board: &[u8; 16], dev: &[u8; 16], key: &zeroize::Zeroizing<[u8; 32]>) -> Result<(), KeyStoreError> {
        unimplemented!()
    }
    fn load(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<zeroize::Zeroizing<[u8; 32]>, KeyStoreError> {
        unimplemented!()
    }
    fn delete(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<(), KeyStoreError> {
        unimplemented!()
    }
}

// The test key stores `FileKeyStore` and `MemoryKeyStore` (device.md §12.6,
// §11.1 item 4) live in `keystore`.
#[cfg(feature = "test-trust")]
pub mod keystore;
#[cfg(feature = "test-trust")]
pub use keystore::{FileKeyStore, MemoryKeyStore};

// AndroidKeyStore lives in taskdeck-android (device.md §12.5) and implements KeyStore there (bridge.md §21).

// ---------- the store and its file layer (§6, §7)

/// The file system calls of the store, its write boundaries (device.md §7.1).
pub trait FileLayer: Send + Sync {
    /// Creates a directory (device.md §7.1).
    fn create_dir(&self, path: &std::path::Path) -> std::io::Result<()>;
    /// Creates or truncates a file and writes all of `bytes` (device.md §7.1).
    fn write_new(&self, path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()>;
    /// Appends to a file (device.md §7.1).
    fn append(&self, path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()>;
    /// Syncs a file (device.md §7.1).
    fn sync(&self, path: &std::path::Path) -> std::io::Result<()>;
    /// Renames a file (device.md §7.1).
    fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()>;
    /// Removes a file (device.md §7.1).
    fn remove(&self, path: &std::path::Path) -> std::io::Result<()>;
    /// Truncates a file to `len` (device.md §7.1).
    fn truncate(&self, path: &std::path::Path, len: u64) -> std::io::Result<()>;
    /// Syncs a directory (device.md §7.1).
    fn sync_dir(&self, dir: &std::path::Path) -> std::io::Result<()>;
    /// Reads a file (device.md §7.1).
    fn read(&self, path: &std::path::Path) -> std::io::Result<Vec<u8>>;
    /// Lists a directory (device.md §7.1).
    fn list(&self, dir: &std::path::Path) -> std::io::Result<Vec<std::ffi::OsString>>;
    /// Whether a path exists (device.md §7.1).
    fn exists(&self, path: &std::path::Path) -> bool;
}

/// The real file layer (device.md §7.1).
pub struct OsFiles;

// `impl FileLayer for OsFiles` is in `store::os_files` (device.md §7.1).

/// The file layer of the crash tests (device.md §7.6, §11.1 item 5).
#[cfg(feature = "test-trust")]
pub struct CrashFiles {
    pub inner: OsFiles,
    pub crash_at: u64,
}

#[cfg(feature = "test-trust")]
impl FileLayer for CrashFiles {
    fn create_dir(&self, path: &std::path::Path) -> std::io::Result<()> {
        unimplemented!()
    }
    fn write_new(&self, path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
        unimplemented!()
    }
    fn append(&self, path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
        unimplemented!()
    }
    fn sync(&self, path: &std::path::Path) -> std::io::Result<()> {
        unimplemented!()
    }
    fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
        unimplemented!()
    }
    fn remove(&self, path: &std::path::Path) -> std::io::Result<()> {
        unimplemented!()
    }
    fn truncate(&self, path: &std::path::Path, len: u64) -> std::io::Result<()> {
        unimplemented!()
    }
    fn sync_dir(&self, dir: &std::path::Path) -> std::io::Result<()> {
        unimplemented!()
    }
    fn read(&self, path: &std::path::Path) -> std::io::Result<Vec<u8>> {
        unimplemented!()
    }
    fn list(&self, dir: &std::path::Path) -> std::io::Result<Vec<std::ffi::OsString>> {
        unimplemented!()
    }
    fn exists(&self, path: &std::path::Path) -> bool {
        unimplemented!()
    }
}

// ---------- adapters (decision D6, §4.1)

/// `CoreMachine` (`taskdeck_sync::StateMachine` over `taskdeck-core`) and
/// `SealSealer` (`taskdeck_sync::Sealer` over `taskdeck-seal`), device.md §25.
pub mod adapt;
pub use adapt::{CoreMachine, SealSealer};

// ---------- test-only (§11)

/// The marker every build with `test-trust` contains (device.md §11.1 item 7, DEV-TRUST-002).
#[cfg(feature = "test-trust")]
pub const TEST_TRUST_MARKER: &str = "taskdeck-test-trust-enabled-6f1c2a";

/// The injectable clock (device.md §11.1 item 5).
#[cfg(feature = "test-trust")]
pub trait TestClock: Send + Sync {
    /// The wall clock in milliseconds (device.md §11.1 item 5).
    fn wall_ms(&self) -> i64;
    /// The monotonic clock in milliseconds (device.md §11.1 item 5).
    fn mono_ms(&self) -> u64;
}

#[cfg(test)]
mod tests {
    use getrandom as _;
    use tempfile as _;

    #[test]
    fn the_shell_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "taskdeck-device");
    }

    /// Pins the signature of `Published::overlay_fetched` (device.md §25, DEV-ICS-011);
    /// it compiles only while the method exists as written. Nothing is called.
    #[test]
    fn overlay_fetched_has_the_spec_signature() {
        let _: for<'a> fn(&'a super::Published) -> &'a [(u64, i64)] =
            super::Published::overlay_fetched;
    }
}
