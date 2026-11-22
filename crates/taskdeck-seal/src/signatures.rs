//! Login, admin and backup signatures (seal.md §16, §26).

use crate::{BlobId, BoardId, Chain, DevId, DeviceKeys, Entropy, FrameHash, SealError, Signer};

/// The login message bytes (seal.md §16.1, §26).
pub fn login_message(
    origin: &str,
    board: &BoardId,
    dev: &DevId,
    r#gen: u32, // `gen` is reserved in edition 2024
    relay_nonce: &[u8],
    client_nonce: &[u8; 32],
) -> Result<Vec<u8>, SealError> {
    unimplemented!()
}

/// Signs a login: `(client_nonce, sig)` (seal.md §16.1, §26).
pub fn sign_login(
    keys: &DeviceKeys,
    origin: &str,
    board: &BoardId,
    r#gen: u32, // `gen` is reserved in edition 2024
    relay_nonce: &[u8],
    rng: &mut dyn Entropy,
) -> Result<([u8; 32], [u8; 64]), SealError> {
    unimplemented!()
}

/// An admin operation of the registry (seal.md §16.2, §26).
pub enum AdminOp {
    Register {
        dev: DevId,
        sign_pk: [u8; 32],
        frame_hash: FrameHash,
    },
    Revoke(DevId),
    Reinstate {
        dev: DevId,
        first_epoch: u32,
        wraps: Vec<[u8; 96]>,
    },
    RecoveryRegister {
        dev: DevId,
        sign_pk: [u8; 32],
        frame_hash: FrameHash,
    },
    Fence {
        host: String,
        door: [u8; 5],
        home_gen: u32,
    },
    Seed(SeedArgs),
    Compact(CompactArgs),
    RestoreTail {
        first_seq: u64,
        frames: Vec<FrameHash>,
    },
    Purge {
        before_seq: u64,
        epoch: u32,
    },
    MintReadToken {
        dev: DevId,
        epoch: u32,
    },
    RevokeReadToken(DevId),
}

impl AdminOp {
    /// The registry's operation name (seal.md §16.2).
    pub fn name(&self) -> &'static str {
        unimplemented!()
    }

    /// The `args` bytes as the registry lays them out (seal.md §16.2, SEAL-SIG-004).
    pub fn args(&self) -> Vec<u8> {
        unimplemented!()
    }
}

/// The `args` of `seed` (seal.md §16.2).
pub struct SeedArgs {
    pub home_gen: u32,
    pub first_seq: u64,
    pub chain_before: Chain,
    pub last_seq: u64,
    pub chain_last: Chain,
    pub blob_id: BlobId,
    pub entries: Vec<SeedEntry>,
}

/// One `dev ‖ sign_pk ‖ role ‖ status ‖ last_ctr` entry of `seed` (seal.md §16.2).
pub struct SeedEntry {
    pub dev: DevId,
    pub sign_pk: [u8; 32],
    pub role: u8,
    pub status: u8,
    pub last_ctr: u64,
}

/// The `args` of `compact` (seal.md §16.2).
pub struct CompactArgs {
    pub below_seq: u64,
    pub snap_entry_seq: u64,
    pub blob_id: BlobId,
    pub keep_seqs: Vec<u64>,
}

/// The admin message bytes (seal.md §16.2, SEAL-SIG-003, §26).
pub fn admin_message(
    op: &AdminOp,
    board: &BoardId,
    signer: &DevId,
    relay_nonce: &[u8],
) -> Result<Vec<u8>, SealError> {
    unimplemented!()
}

/// Signs an admin operation (seal.md §16.2, SEAL-SIG-003, §26).
pub fn sign_admin(
    signer: &Signer,
    op: &AdminOp,
    board: &BoardId,
    relay_nonce: &[u8],
) -> Result<[u8; 64], SealError> {
    unimplemented!()
}
