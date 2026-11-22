//! Sealed local files (seal.md §19, §26).

use crate::{BoardId, DevId, Entropy, SealError};

/// A sealed file's kind byte (seal.md §19.2, §26).
#[repr(u8)]
pub enum FileKind {
    Log = 1,
    Checkpoint = 2,
    Base = 3,
    Hashes = 4,
    Outbox = 5,
    Overlay = 6,
    Identity = 7,
    Notices = 8,
    Meta = 9,
    Marker = 10,
    History = 11,
    Device = 12,
    Picture = 13,
    Weather = 14,
    Draft = 15,
    Feed = 16,
    Bootstrap = 17,
    Unsent = 18,
    OwnPicture = 19,
}

/// The storage key `L_d` (seal.md §6.6, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct StorageKey {
    key: [u8; 32],
}

impl StorageKey {
    /// Draws a new `L_d` (seal.md §6.6).
    pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError> {
        unimplemented!()
    }

    /// `L_d` from its bytes (seal.md §6.6).
    pub fn from_bytes(b: [u8; 32]) -> Self {
        unimplemented!()
    }
}

/// Seals the records of one file: its header and next record index (seal.md §19.1, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct FileSealer {
    file_key: [u8; 32],
    header: Vec<u8>,
    next: u64,
}

impl FileSealer {
    /// Starts a new file and returns its header bytes (seal.md §19.1).
    pub fn create(
        key: &StorageKey,
        kind: FileKind,
        board: &BoardId,
        dev: &DevId,
        rng: &mut dyn Entropy,
    ) -> Result<(Self, Vec<u8>), SealError> {
        unimplemented!()
    }

    /// Continues a file after `records` records (seal.md §19.1).
    pub fn resume(key: &StorageKey, header: &[u8], records: u64) -> Result<Self, SealError> {
        unimplemented!()
    }

    /// Seals the next record (seal.md §19.1, §19.3).
    pub fn seal_record(
        &mut self,
        plaintext: &[u8],
        rng: &mut dyn Entropy,
    ) -> Result<Vec<u8>, SealError> {
        unimplemented!()
    }
}

/// An opened file's records and tail (seal.md §19.4, §26).
pub struct OpenedFile {
    pub records: Vec<zeroize::Zeroizing<Vec<u8>>>,
    pub tail: FileTail,
}

/// How a file ends (seal.md §19.4, §26).
pub enum FileTail {
    Clean,
    Torn { valid_bytes: u64 },
    Bad { valid_bytes: u64 },
}

/// Reads a sealed file (seal.md §19.4, §26).
pub fn open_file(
    key: &StorageKey,
    kind: FileKind,
    board: &BoardId,
    dev: &DevId,
    bytes: &[u8],
) -> Result<OpenedFile, SealError> {
    unimplemented!()
}
