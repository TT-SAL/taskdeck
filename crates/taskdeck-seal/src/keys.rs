//! Keys and epoch-root wraps (seal.md §6, §7, §26).

use crate::{BoardId, DevId, Entropy, SealError};

/// The 32-byte recovery seed (seal.md §6.1, SEAL-KEY-001, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct RecoverySeed {
    seed: [u8; 32],
}

impl RecoverySeed {
    /// Draws a new seed (seal.md §6.1, SEAL-KEY-001).
    pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError> {
        unimplemented!()
    }

    /// The sheet text (seal.md §18.1, SEAL-KEY-004).
    pub fn to_sheet(&self) -> zeroize::Zeroizing<String> {
        unimplemented!()
    }

    /// Reads a typed sheet (seal.md §18.2).
    pub fn from_sheet(text: &str) -> Result<Self, SealError> {
        unimplemented!()
    }

    /// The recovery keys derived from the seed (seal.md §6.1).
    pub fn keys(&self) -> RecoveryKeys {
        unimplemented!()
    }
}

/// The recovery signing key and HPKE secret (seal.md §6.1, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct RecoveryKeys {
    sign: ed25519_dalek::SigningKey,
    hpke: x25519_dalek::StaticSecret,
}

impl RecoveryKeys {
    /// The recovery public key (seal.md §6.1).
    pub fn public(&self) -> RecoveryPublic {
        unimplemented!()
    }
}

/// The recovery public key `(rsign_pk, rhpke_pk)` (seal.md §6.1, §26).
pub struct RecoveryPublic {
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
}

impl RecoveryPublic {
    /// The recovery identity's `dev_R` on a board (seal.md §6.3).
    pub fn dev(&self, board: &BoardId) -> DevId {
        unimplemented!()
    }

    /// The key fingerprint for banners (seal.md §6.8).
    pub fn fingerprint(&self) -> String {
        unimplemented!()
    }
}

/// `board_id` from the recovery signing key and the salt (seal.md §6.2, §26).
pub fn board_id(rsign_pk: &[u8; 32], salt: &[u8; 16]) -> BoardId {
    unimplemented!()
}

/// A device's `dev`, signing key and X25519 key (seal.md §6.4, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct DeviceKeys {
    #[zeroize(skip)]
    dev: DevId,
    sign: ed25519_dalek::SigningKey,
    hpke: x25519_dalek::StaticSecret,
}

impl DeviceKeys {
    /// Draws new device keys (seal.md §6.4).
    pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError> {
        unimplemented!()
    }

    /// The device's public keys (seal.md §6.4).
    pub fn public(&self) -> DevicePublic {
        unimplemented!()
    }

    /// The identity file contents (seal.md §19.2 kind 7).
    pub fn to_identity_bytes(&self) -> zeroize::Zeroizing<Vec<u8>> {
        unimplemented!()
    }

    /// Reads identity file contents (seal.md §19.2 kind 7).
    pub fn from_identity_bytes(bytes: &[u8]) -> Result<Self, SealError> {
        unimplemented!()
    }
}

/// A device's public keys (seal.md §6.4, §26).
pub struct DevicePublic {
    pub dev: DevId,
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
}

impl DevicePublic {
    /// The key fingerprint for banners (seal.md §6.8).
    pub fn fingerprint(&self) -> String {
        unimplemented!()
    }
}

/// Who signs: a device, or the recovery identity on a board (seal.md §8.3, §16, §26).
pub enum Signer<'a> {
    Device(&'a DeviceKeys),
    Recovery(&'a RecoveryKeys, BoardId),
}

/// An epoch root `R_e` (seal.md §6.5, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct EpochRoot {
    root: [u8; 32],
}

impl EpochRoot {
    /// Draws a new root (seal.md §6.5).
    pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError> {
        unimplemented!()
    }

    /// A root from its bytes (seal.md §26).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        unimplemented!()
    }

    /// The root's bytes, for §19.2 kind 7 only (SEAL-KEY-009).
    pub fn to_bytes(&self) -> zeroize::Zeroizing<[u8; 32]> {
        unimplemented!()
    }

    /// The commitment `c_e` (seal.md §6.5).
    pub fn commitment(&self, board: &BoardId, epoch: u32) -> [u8; 32] {
        unimplemented!()
    }
}

/// Which per-kind key of an epoch (seal.md §6.5, §26).
pub enum KeyKind {
    Entry,
    Snap,
    Blob,
}

/// A derived symmetric key (seal.md §6.5, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct SymKey {
    key: [u8; 32],
}

/// `K_entry,e`, `K_snap,e` or `K_blob,e` (seal.md §6.5, §26).
pub fn derive_epoch_key(root: &EpochRoot, board: &BoardId, epoch: u32, kind: KeyKind) -> SymKey {
    unimplemented!()
}

/// Wraps a root to one recipient (seal.md §7, §26).
pub fn wrap_root(
    root: &EpochRoot,
    board: &BoardId,
    epoch: u32,
    commit: &[u8; 32],
    recipient: DevId,
    recipient_pk: &[u8; 32],
    rng: &mut dyn Entropy,
) -> Result<[u8; 96], SealError> {
    unimplemented!()
}

/// Opens a wrap addressed to this identity (seal.md §7, §26).
pub fn unwrap_root(
    wrap: &[u8; 96],
    board: &BoardId,
    epoch: u32,
    commit: &[u8; 32],
    own: &Recipient,
) -> Result<EpochRoot, SealError> {
    unimplemented!()
}

/// Whose wrap is opened: a device, or the recovery identity on a board (seal.md §7, §26).
pub enum Recipient<'a> {
    Device(&'a DeviceKeys),
    Recovery(&'a RecoveryKeys, BoardId),
}
