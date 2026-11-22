//! Hash chain, head and fingerprints (seal.md §15, §26).

use crate::{BoardId, Chain, FrameHash, Head, SealError};

/// `C_0` (seal.md §15.1, §26).
pub fn chain_zero(board: &BoardId) -> Chain {
    unimplemented!()
}

/// `C_s` from `C_{s−1}` (seal.md §15.1, §26).
pub fn chain_step(prev: &Chain, seq: u64, frame: &[u8]) -> Chain {
    unimplemented!()
}

/// The sync fingerprint: a seq and four words (seal.md §15.3, §26).
pub struct SyncFingerprint {
    pub seq: u64,
    pub words: [&'static str; 4],
}

/// The sync fingerprint of a head (seal.md §15.3, §26).
pub fn sync_fingerprint(board: &BoardId, head: &Head) -> SyncFingerprint {
    unimplemented!()
}

/// The move code of a Fence (seal.md §15.4, §26).
pub fn move_code(board: &BoardId, fence_seq: u64, fence_hash: &FrameHash) -> String {
    unimplemented!()
}

/// Checks a typed move code, or `seal/move_code` (seal.md §15.4, §26).
pub fn check_move_code(
    board: &BoardId,
    fence_seq: u64,
    fence_hash: &FrameHash,
    typed: &str,
) -> Result<(), SealError> {
    unimplemented!()
}

/// A key fingerprint for banners (seal.md §6.8, §26).
pub fn key_fingerprint(sign_pk: &[u8; 32], hpke_pk: &[u8; 32]) -> String {
    unimplemented!()
}

/// The state hash of ENG-HASH-001 (engine.md §18.3; seal.md §25, §26).
pub fn state_hash(hash_version: u32, board_hash: &[u8; 32], membership_bytes: &[u8]) -> [u8; 32] {
    unimplemented!()
}
