//! Blob requests and their answers (engine.md §19.6).

use crate::{Dev, Hash32, SealedBlob};

/// A blob request (engine.md §19.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlobRequest {
    Upload { purpose: u8, seq: u64, blob: SealedBlob },
    Fetch { id: [u8; 16], parts: u32 },
    List,
    Meta { id: [u8; 16] },
}

/// A blob's metadata (engine.md §19.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobMeta {
    pub id: [u8; 16],
    pub purpose: u8,
    pub seq: u64,
    pub author: Dev,
    pub size: u64,
    pub parts: u32,
    pub sha256: Hash32,
    pub complete: bool,
}

/// A successful blob answer (engine.md §19.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlobOk {
    Uploaded,
    Parts(Vec<Vec<u8>>),
    List(Vec<BlobMeta>),
    Meta(BlobMeta),
}
