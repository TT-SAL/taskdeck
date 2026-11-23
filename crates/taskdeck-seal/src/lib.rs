//! `taskdeck-seal`: frames, contexts, envelope, keys, membership, chain,
//! pairing, HPKE wraps, sealed files and backups, as pure functions and
//! sans-IO machines.
//!
//! Spec: `docs/sync/spec/seal.md`. Owner: stage `seal` (SYNC_PLAN.md §4).
//! Created as a shell by stage `foundation`; the types are the spec's Public
//! API (§26), and stage `seal` filled every body.
#![forbid(unsafe_code)]

mod backup;
pub mod base32;
mod blob;
mod canon;
mod chain;
pub mod ctx;
mod encodings;
mod entries;
mod entropy;
mod error;
mod file;
mod ids;
mod keys;
mod membership;
pub mod pair;
mod payloads;
mod prim;
mod receipt;
mod signatures;

pub use backup::{
    BackupContent, BackupHeader, BackupInput, HistoryRecord, VerifiedBackup, read_backup,
    verify_backup_file, write_backup,
};
pub use blob::{
    BlobKind, BlobRef, SealedBlob, SnapshotBody, open_blob, open_snapshot, open_snapshot_declared,
    seal_blob, seal_snapshot, split_parts,
};
pub use chain::{
    SyncFingerprint, chain_step, chain_zero, check_move_code, key_fingerprint, move_code,
    state_hash, sync_fingerprint,
};
pub use encodings::relay_origin;
pub use entries::{
    AckVerdict, Beat, BeatAck, BeatState, CtlItem, DeviceAdded, EpochRotated, Fence, FrameHeader,
    Genesis, GenesisDevice, GenesisSnapshot, Kind, Plain, RecoveryKeyChange, RecoveryKeyMode,
    ResetTo, RulesUpgrade, RulesUpgradeMode, SkipItem, SnapAnnounce, beat_payload, ctl_payload,
    frame_hash, frame_header, new_entry_id, padded_len, seal_frame, snap_payload,
};
#[cfg(feature = "test-entropy")]
pub use entropy::{FixedEntropy, TEST_ENTROPY_MARKER};
pub use entropy::{Entropy, OsEntropy};
pub use error::SealError;
pub use file::{FileKind, FileSealer, FileTail, OpenedFile, StorageKey, open_file};
pub use ids::{BlobId, BoardId, Chain, DevId, EntryId, Epoch, FrameHash, Head};
pub use keys::{
    DeviceKeys, DevicePublic, EpochRoot, KeyKind, Recipient, RecoveryKeys, RecoveryPublic,
    RecoverySeed, Signer, SymKey, board_id, derive_epoch_key, unwrap_root, wrap_root,
};
pub use membership::{
    DeviceRecord, DeviceStatus, GenesisAnchor, Membership, PendingRecovery, RecoveryRecord,
};
pub use receipt::{Class, Halt, Opened, OwnIdentity, Receipt, ReceiptInput, classify};
pub use signatures::{
    AdminOp, CompactArgs, SeedArgs, SeedEntry, admin_message, login_message, sign_admin,
    sign_login,
};

// `serde` is declared in Cargo.toml but used by no module; this keeps it
// resolved and compiled with the features the crate asks for.
const _: () = {
    #[allow(dead_code)] // never built: it only proves the derive compiles
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Declared([u8; 32]);
};

#[cfg(test)]
mod tests {
    use proptest as _;

    #[test]
    fn the_shell_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "taskdeck-seal");
    }
}
