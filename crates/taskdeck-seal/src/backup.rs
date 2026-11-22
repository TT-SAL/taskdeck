//! Backups and segments (seal.md §21, §26).

use crate::{
    BoardId, Chain, DevId, DeviceKeys, DevicePublic, Entropy, EntryId, EpochRoot, FrameHash, Head,
    Membership, Opened, RecoveryKeys, RecoveryPublic, SealError,
};

/// One history record of an accepted control entry (seal.md §21.5, §26).
pub struct HistoryRecord {
    seq: u64,
    dev: DevId,
    ctr: u64,
    entry_id: EntryId,
    at_ms: i64,
    frame_hash: FrameHash,
    items: Vec<u8>,
    sig: [u8; 64],
}

impl HistoryRecord {
    /// Builds the record of an accepted `ctl` entry, or `seal/not_control` (SEAL-BKP-010).
    pub fn from_accepted(seq: u64, frame: &[u8], opened: &Opened) -> Result<Self, SealError> {
        unimplemented!()
    }

    /// The record's bytes (seal.md §21.5).
    pub fn to_bytes(&self) -> Vec<u8> {
        unimplemented!()
    }

    /// Reads exactly one record, or `seal/backup_history` (seal.md §21.5).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SealError> {
        unimplemented!()
    }

    /// The entry's seq (seal.md §21.5).
    pub fn seq(&self) -> u64 {
        unimplemented!()
    }
}

/// A nightly backup's or a segment's content (seal.md §21.5, §26).
pub enum BackupContent {
    Nightly {
        membership: Vec<u8>,
        board: Vec<u8>,
        head: Head,
        hash_version: u32,
        state_hash: [u8; 32],
    },
    Segment {
        membership: Vec<u8>,
        first_seq: u64,
        chain_before: Chain,
        frames: Vec<Vec<u8>>,
    },
}

/// What a writer puts in a backup file (seal.md §21, §26).
pub struct BackupInput<'a> {
    pub board: BoardId,
    pub seq: u64,
    pub written_ms: i64,
    pub history: &'a [HistoryRecord],
    pub content: BackupContent,
    pub roots: Vec<(u32, EpochRoot)>,
    pub read_token: Option<String>,
    pub meta: Vec<u8>,
    pub pinned_recovery: RecoveryPublic,
}

/// Writes a backup or segment file (seal.md §21.2 to §21.5, §26).
pub fn write_backup(
    writer: &DeviceKeys,
    input: BackupInput,
    rng: &mut dyn Entropy,
) -> Result<Vec<u8>, SealError> {
    unimplemented!()
}

/// A backup file that passed every step of §21.6 (seal.md §26).
pub struct VerifiedBackup {
    pub writer: DevicePublic,
    pub seq: u64,
    pub file_type: u8,
    pub content: BackupContent,
    pub roots: Vec<(u32, EpochRoot)>,
    pub read_token: Option<String>,
    pub meta: Vec<u8>,
    pub projection: Membership,
}

/// Reads a backup file with a typed sheet's keys (seal.md §21.6, §26).
pub fn read_backup(
    bytes: &[u8],
    sheet_board: &BoardId,
    recovery: &RecoveryKeys,
) -> Result<VerifiedBackup, SealError> {
    unimplemented!()
}

/// A backup file's header (seal.md §21.2, §26).
pub struct BackupHeader {
    pub file_type: u8,
    pub board: BoardId,
    pub writer_dev: DevId,
    pub writer_sign_pk: [u8; 32],
    pub recipient: DevId,
    pub seq: u64,
    pub written_ms: i64,
    pub ct_len: u64,
}

/// Checks a file's header and writer signature: §21.6 steps 1, 2, 5 (SEAL-BKP-009; seal.md §26).
pub fn verify_backup_file(
    bytes: &[u8],
    writer_sign_pk: &[u8; 32],
) -> Result<BackupHeader, SealError> {
    unimplemented!()
}
