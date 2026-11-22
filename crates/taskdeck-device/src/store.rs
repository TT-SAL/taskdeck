//! The store's encodings (device.md §6, §25).
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

/// Encodes a log page (device.md §6.4.1).
pub fn encode_log_page(records: &[taskdeck_sync::LogRecord]) -> Vec<u8> {
    unimplemented!()
}

/// Decodes a log page (device.md §6.4.1).
pub fn decode_log_page(bytes: &[u8]) -> Result<Vec<taskdeck_sync::LogRecord>, super::DeviceError> {
    unimplemented!()
}

/// Encodes a checkpoint (device.md §6.4.2).
pub fn encode_checkpoint(c: &taskdeck_sync::Checkpoint) -> Result<Vec<u8>, super::DeviceError> {
    unimplemented!()
}

/// Decodes a checkpoint (device.md §6.4.2).
pub fn decode_checkpoint(bytes: &[u8]) -> Result<taskdeck_sync::Checkpoint, super::DeviceError> {
    unimplemented!()
}

/// Encodes a value as record JSON (device.md §6.5).
pub fn record_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, super::DeviceError> {
    unimplemented!()
}

/// Decodes record JSON (device.md §6.5).
pub fn from_record_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, super::DeviceError> {
    unimplemented!()
}

/// Reads an identity directory: the engine's `Loaded` and the history records (device.md §7.4).
pub fn read_identity_dir(
    dir: &std::path::Path,
    l_d: &zeroize::Zeroizing<[u8; 32]>,
) -> Result<(taskdeck_sync::Loaded, Vec<taskdeck_seal::HistoryRecord>), super::DeviceError> {
    unimplemented!()
}

/// Writes the legacy files of a board (device.md §9.3, §18.7).
pub fn write_legacy_files(
    dir: &std::path::Path,
    board: &taskdeck_core::board::BoardData,
    zone: &dyn taskdeck_core::time::Zone,
) -> Result<(), super::DeviceError> {
    unimplemented!()
}
