//! The membership state (seal.md §13, §26).

use crate::{BoardId, DevId, EpochRoot, FrameHash, SealError};

/// The membership state `M` (seal.md §13.1, §26).
pub struct Membership {
    board_id: BoardId,
    salt: [u8; 16],
    rules: u32,
    epoch: u32,
    epoch_seq: u64,
    epoch_clock_ms: i64,
    clock_ms: i64,
    home_gen: u32,
    home_seq: u64,
    home_host: String,
    home_door: [u8; 5],
    purge_seq: u64,
    next_device_no: u16,
    /// `(e, seq, c_e)`, ascending by `e`.
    commitments: Vec<(u32, u64, [u8; 32])>,
    recovery: Vec<RecoveryRecord>,
    devices: Vec<DeviceRecord>,
    pending_recovery: Option<PendingRecovery>,
    /// `(rules, voter, seq)`.
    pending_override: Option<(u32, DevId, u64)>,
    /// `(at_seq, target, hash_version, state_hash)`.
    last_reset: Option<(u64, u64, u32, [u8; 32])>,
    /// `(t, frame_hash)`, ascending by `t`.
    skips: Vec<(u64, FrameHash)>,
    ctl_seqs: Vec<u64>,
}

impl Membership {
    /// The canonical membership bytes (seal.md §13.2, SEAL-MEM-001).
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        unimplemented!()
    }

    /// Parses canonical membership bytes (seal.md §13.2, SEAL-MEM-002).
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, SealError> {
        unimplemented!()
    }

    /// `board_id` (seal.md §13.1).
    pub fn board(&self) -> BoardId {
        unimplemented!()
    }

    /// The current epoch `E` (seal.md §13.1).
    pub fn epoch(&self) -> u32 {
        unimplemented!()
    }

    /// The active rules version (seal.md §13.1).
    pub fn rules(&self) -> u32 {
        unimplemented!()
    }

    /// Every device ever added, ascending by `dev` (seal.md §13.1).
    pub fn devices(&self) -> &[DeviceRecord] {
        unimplemented!()
    }

    /// The recovery record with `until_seq = 0` (seal.md §13.1).
    pub fn current_recovery(&self) -> &RecoveryRecord {
        unimplemented!()
    }

    /// The proposed recovery-key change, if any (seal.md §13.1, §13.11).
    pub fn pending_recovery(&self) -> Option<&PendingRecovery> {
        unimplemented!()
    }

    /// A device's `device_no` (seal.md §13.7).
    pub fn device_no(&self, dev: &DevId) -> Option<u16> {
        unimplemented!()
    }
}

/// A device record (seal.md §13.2, §26).
pub struct DeviceRecord {
    pub dev: DevId,
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
    pub device_no: u16,
    pub status: DeviceStatus,
    pub added_seq: u64,
    pub added_by: DevId,
    pub bootstrap_seq: u64,
    pub status_seq: u64,
    pub status_by: DevId,
    pub last_ctr: u64,
    pub beat_ms: i64,
    pub rules: Vec<u32>,
    pub envelopes: Vec<(u8, u8)>,
    pub hash_versions: Vec<u32>,
    pub name: String,
}

/// A device record's `status` byte (seal.md §13.2).
pub enum DeviceStatus {
    Current = 1,
    Revoked = 2,
}

/// A recovery record (seal.md §13.2, §26).
pub struct RecoveryRecord {
    pub dev: DevId,
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
    pub since_seq: u64,
    pub until_seq: u64,
    pub last_ctr: u64,
}

/// A proposed recovery-key change (seal.md §13.2, §26).
pub struct PendingRecovery {
    pub proposer: DevId,
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
    pub seq: u64,
    pub start_ms: i64,
}

/// The caller's anchor for seq 1 (seal.md §13.3, §26).
pub struct GenesisAnchor {
    pub rsign_pk: [u8; 32],
    pub salt: [u8; 16],
    pub root1: EpochRoot,
}
