//! `taskdeck-seal`: frames, contexts, envelope, keys, membership, chain,
//! pairing, HPKE wraps, sealed files and backups, as pure functions and
//! sans-IO machines.
//!
//! Spec: `docs/sync/spec/seal.md`. Owner: stage `seal` (SYNC_PLAN.md §4).
//! Created as a shell by stage `foundation`; the skeleton types come from the
//! spec's Public API (§26). Every body is `unimplemented!()` until stage
//! `seal` fills it: `grep -rn 'unimplemented!()' crates/taskdeck-seal` lists
//! what is left.
#![forbid(unsafe_code)]
// Skeleton (task 4): the bodies are `unimplemented!()`, so the parameters the
// spec names and the private fields of the opaque types are not read yet.
// Stage `seal` removes this line.
#![allow(unused_variables, dead_code)]

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

// Declared for the stage that fills this crate: each dependency is named once
// so that it is resolved and compiled now, and a wrong feature set fails today
// rather than in wave 1.
use chacha20poly1305 as _;
use ed25519_dalek as _;
use flate2 as _;
use getrandom as _;
use hkdf as _;
use hpke as _;
use serde_json as _;
use sha2 as _;
use subtle as _;
use x25519_dalek as _;
const _: () = {
    // The features seal.md needs, named so that turning one off fails here.
    let _ = <x25519_dalek::StaticSecret as From<[u8; 32]>>::from;
    // `alloc`: XChaCha20-Poly1305 into a `Vec`.
    type Encrypt = fn(
        &chacha20poly1305::XChaCha20Poly1305,
        &chacha20poly1305::XNonce,
        &'static [u8],
    ) -> Result<Vec<u8>, chacha20poly1305::Error>;
    let _: Encrypt = <chacha20poly1305::XChaCha20Poly1305 as chacha20poly1305::aead::Aead>::encrypt;
    let _ = ed25519_dalek::VerifyingKey::verify_strict;
    let _ = <hpke::kem::X25519HkdfSha256 as hpke::Kem>::derive_keypair;
    let _ = hpke::single_shot_open::<hpke::aead::ChaCha20Poly1305, hpke::kdf::HkdfSha256, hpke::kem::X25519HkdfSha256>;
    // hpke's `*_with_rng` sealing functions take `&mut impl CryptoRng` from
    // rand_core 0.10, which hpke re-exports as `hpke::rand_core`: `Entropy`
    // (SEAL-GEN-004) reaches hpke through a wrapper like this one, so seal
    // declares no rand_core of its own.
    #[allow(dead_code)] // never built: it only proves the wrapper type-checks
    struct EntropyRng<F: FnMut(&mut [u8])>(F);
    impl<F: FnMut(&mut [u8])> hpke::rand_core::TryRng for EntropyRng<F> {
        type Error = core::convert::Infallible;
        fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
            let mut b = [0u8; 4];
            (self.0)(&mut b);
            Ok(u32::from_le_bytes(b))
        }
        fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
            let mut b = [0u8; 8];
            (self.0)(&mut b);
            Ok(u64::from_le_bytes(b))
        }
        fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
            (self.0)(dst);
            Ok(())
        }
    }
    impl<F: FnMut(&mut [u8])> hpke::rand_core::TryCryptoRng for EntropyRng<F> {}
    type Kem = hpke::kem::X25519HkdfSha256;
    #[allow(dead_code)] // never called: it only proves hpke accepts the wrapper
    fn seal_with_entropy(
        rng: &mut EntropyRng<fn(&mut [u8])>,
        pk: &<Kem as hpke::Kem>::PublicKey,
    ) -> Result<(<Kem as hpke::Kem>::EncappedKey, Vec<u8>), hpke::HpkeError> {
        hpke::single_shot_seal_with_rng::<hpke::aead::ChaCha20Poly1305, hpke::kdf::HkdfSha256, Kem>(
            &hpke::OpModeS::Base,
            pk,
            b"",
            b"",
            b"",
            rng,
        )
    }
    #[allow(dead_code)] // never built: it only proves the derives compile
    #[derive(serde::Serialize, serde::Deserialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
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
