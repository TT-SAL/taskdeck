//! Snapshots and blobs (seal.md §20, §26).

use crate::{BlobId, BoardId, Chain, DevId, Entropy, EpochRoot, HistoryRecord, SealError, SymKey};

/// A blob kind (seal.md §20.2, §26).
pub enum BlobKind {
    Snapshot = 1,
    Picture = 2,
}

/// A sealed blob: id, bytes, hash and part count (seal.md §20.2, §26).
pub struct SealedBlob {
    pub id: BlobId,
    pub bytes: Vec<u8>,
    pub sha256: [u8; 32],
    pub parts: u32,
}

/// What a reader expects of a blob before decrypting (seal.md §20.5, §26).
pub struct BlobRef {
    pub id: BlobId,
    pub sha256: [u8; 32],
    pub size: u64,
    pub parts: u32,
    pub inflated: u64,
    pub epoch: u32,
}

/// A snapshot body (seal.md §20.4, §26).
pub struct SnapshotBody {
    pub seq: u64,
    pub chain: Chain,
    pub hash_version: u32,
    pub state_hash: [u8; 32],
    pub board: Vec<u8>,
    pub membership: Vec<u8>,
    pub history: Vec<HistoryRecord>,
}

/// Seals a snapshot, blob kind 1 (seal.md §20.2 to §20.4, §26).
pub fn seal_snapshot(
    body: &SnapshotBody,
    key: &SymKey,
    board: &BoardId,
    epoch: u32,
    writer: &DevId,
    rng: &mut dyn Entropy,
) -> Result<SealedBlob, SealError> {
    unimplemented!()
}

/// Opens a snapshot against its reference (seal.md §20.5, §26).
pub fn open_snapshot(
    parts: &[&[u8]],
    expected: &BlobRef,
    s: u64,
    writer: &DevId,
    key: &SymKey,
    board: &BoardId,
) -> Result<SnapshotBody, SealError> {
    unimplemented!()
}

/// Opens a snapshot in declared mode: `(body, epoch, inflated)` (seal.md §20.5, §26).
// `<'a>`: the spec's `Option<&EpochRoot>` needs a named lifetime to compile.
pub fn open_snapshot_declared<'a>(
    parts: &[&[u8]],
    sha256: &[u8; 32],
    size: u64,
    s: u64,
    writer: &DevId,
    roots: &dyn Fn(u32) -> Option<&'a EpochRoot>,
    board: &BoardId,
) -> Result<(SnapshotBody, u32, u64), SealError> {
    unimplemented!()
}

/// Seals a blob of kind 2 (seal.md §20.2, §20.3, §26).
pub fn seal_blob(
    body: &[u8],
    key: &SymKey,
    board: &BoardId,
    epoch: u32,
    writer: &DevId,
    rng: &mut dyn Entropy,
) -> Result<SealedBlob, SealError> {
    unimplemented!()
}

/// Opens a blob of kind 2; `expected.inflated` is not read (seal.md §20.5, §26).
pub fn open_blob(
    parts: &[&[u8]],
    expected: &BlobRef,
    writer: &DevId,
    key: &SymKey,
    board: &BoardId,
) -> Result<Vec<u8>, SealError> {
    unimplemented!()
}

/// Splits sealed bytes into parts of 1,048,576 bytes (seal.md §20.2, §26).
pub fn split_parts(sealed: &[u8]) -> Vec<&[u8]> {
    unimplemented!()
}
