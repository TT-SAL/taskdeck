//! Ids and small values (seal.md §5, §8, §15, §26).

/// A board id, `H(ctx("board") ‖ rsign_pk ‖ salt)[0..16]` (seal.md §6.2, §26).
pub struct BoardId(pub [u8; 16]);

/// A device's or the recovery identity's `dev` (seal.md §6.3, §6.4, §26).
pub struct DevId(pub [u8; 16]);

/// An entry id of a plaintext (seal.md §10.1, §26).
pub struct EntryId(pub [u8; 16]);

/// A blob id (seal.md §20.1, §26).
pub struct BlobId(pub [u8; 16]);

/// A frame hash (seal.md §8.2, §26).
pub struct FrameHash(pub [u8; 32]);

/// A hash-chain value `C_s` (seal.md §15.1, §26).
pub struct Chain(pub [u8; 32]);

/// A log head: a seq and its chain (seal.md §15.2, §26).
pub struct Head {
    pub seq: u64,
    pub chain: Chain,
}

/// An epoch number (seal.md §6.5, §26).
pub struct Epoch(pub u32);
