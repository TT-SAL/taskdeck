//! The store of synced mode (device.md §5 to §8, §25): the data folder and
//! its lock, the sealed files of an identity directory and their encodings,
//! `Persist` execution, and the reading at open.
//!
//! The file kinds of §6.1 are `taskdeck_seal::FileKind` (seal.md §19.2, §26):
//! the store passes those variants to `FileSealer::create` and `open_file`.
//! These constants are their numbers, never defined a second time.

use taskdeck_seal::FileKind;

/// The kind of `meta` (device.md §6.1).
pub const KIND_META: u8 = FileKind::Meta as u8;
/// The kind of the marker (device.md §6.1).
pub const KIND_MARKER: u8 = FileKind::Marker as u8;
/// The kind of a history record (device.md §6.1).
pub const KIND_HISTORY: u8 = FileKind::History as u8;
/// The kind of the device file (device.md §6.1).
pub const KIND_DEVICE: u8 = FileKind::Device as u8;
/// The kind of the picture (device.md §6.1).
pub const KIND_PICTURE: u8 = FileKind::Picture as u8;
/// The kind of the weather (device.md §6.1).
pub const KIND_WEATHER: u8 = FileKind::Weather as u8;
/// The kind of the notes draft (device.md §6.1).
pub const KIND_DRAFT: u8 = FileKind::Draft as u8;
/// The kind of a feed body (device.md §6.1).
pub const KIND_FEED: u8 = FileKind::Feed as u8;
/// The kind of the bootstrap (device.md §6.1).
pub const KIND_BOOTSTRAP: u8 = FileKind::Bootstrap as u8;
/// The kind of the unsent file (device.md §6.1).
pub const KIND_UNSENT: u8 = FileKind::Unsent as u8;
/// The kind of the phone's own picture (device.md §6.1).
pub const KIND_OWN_PICTURE: u8 = FileKind::OwnPicture as u8;

mod codec;
mod folder;
mod identity_dir;
mod os_files;
mod rjson;
mod settings;
#[cfg(test)]
mod tests;

pub use codec::{
    ACTIVE_MAGIC, PENDING_MAGIC, StoreDescriptor, decode_active, decode_checkpoint, decode_log_page, encode_active,
    encode_checkpoint, encode_log_page,
};
pub use folder::{
    FolderState, IdentityName, Lock, activate, create_identity, identities, identity_dir, is_cloud_folder, open_folder,
    read_active, read_pending, read_store_descriptor, sync_dir, take_lock, write_pending,
};
pub use identity_dir::{IdentityStore, NewIdentity, Opened, base_name, segment_name};
pub use settings::DeviceSettings;

/// Encodes a value as record JSON (device.md §6.5): the canonical encoding
/// (`core.md` §4.2) of its `serde` value. The engine's records have no
/// `serde` implementations; the store maps those itself (`rjson`, finding
/// f2-1). This is for the device's own records.
pub fn record_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, super::DeviceError> {
    let v = serde_json::to_value(value).map_err(|_| codec::err(rjson::RECORD_RANGE))?;
    taskdeck_core::canon::encode(&v).map_err(|_| codec::err(rjson::RECORD_RANGE))
}

/// Decodes record JSON (device.md §6.5): the canonical parser (depth 32),
/// then the type; any refusal is `device/record_format`.
pub fn from_record_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, super::DeviceError> {
    let v = taskdeck_core::canon::parse(bytes, rjson::META_MAX).map_err(|_| codec::fmt())?;
    serde_json::from_value(v).map_err(|_| codec::fmt())
}

/// Reads an identity directory: the engine's `Loaded` and the history
/// records (device.md §7.4), through `OsFiles`, with the board and `dev` of
/// its `store.tds`. As at open, leftover `*.tmp` files are removed and torn
/// tails truncated. `Loaded.build_id` is empty: the caller sets it.
pub fn read_identity_dir(
    dir: &std::path::Path,
    l_d: &zeroize::Zeroizing<[u8; 32]>,
) -> Result<(taskdeck_sync::Loaded, Vec<taskdeck_seal::HistoryRecord>), super::DeviceError> {
    let files: std::sync::Arc<dyn super::FileLayer> = std::sync::Arc::new(super::OsFiles);
    let d = read_store_descriptor(&*files, dir)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |t| i64::try_from(t.as_millis()).unwrap_or(i64::MAX));
    let o = IdentityStore::open(files, dir, d.board_id, d.dev, l_d, "", now)?;
    let history = o
        .history
        .iter()
        .map(|b| taskdeck_seal::HistoryRecord::from_bytes(b).map_err(|_| codec::fmt()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((o.loaded, history))
}

/// Writes the legacy files of a board (device.md §9.3, §18.7).
pub fn write_legacy_files(
    dir: &std::path::Path,
    board: &taskdeck_core::board::BoardData,
    zone: &dyn taskdeck_core::time::Zone,
) -> Result<(), super::DeviceError> {
    unimplemented!()
}
