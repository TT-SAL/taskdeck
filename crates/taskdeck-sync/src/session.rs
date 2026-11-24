//! The session's relay messages (engine.md §11.1, §11.7).

use crate::{Dev, Hash32, Head};

/// A socket welcome or a `hello` response (engine.md §11.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Welcome {
    pub head: Head,
    pub last_ctr: Option<u64>,
    pub last_seq: Option<u64>,
    pub relay_now: i64,
    pub floor: u64,
    pub purge_floor: u64,
    pub fenced: bool,
    pub fence: Option<(String, [u8; 5], u32)>,
    pub appends_left: Option<u32>,
    pub push: bool,
}

/// An admin request (engine.md §11.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdminRequest {
    Register { frame: Vec<u8>, dev: Dev, sign_pk: Hash32 },
    RegisterSeq { seq: u64, dev: Dev, sign_pk: Hash32, frame_hash: Hash32 },
    RecoveryRegisterSeq { seq: u64, dev_r: Dev, sign_pk: Hash32, frame_hash: Hash32 },
    Revoke { dev: Dev },
    Fence { host: String, door: [u8; 5], home_gen: u32, frame: Option<Vec<u8>> },
    Door { host: String },
    Seed {
        host: String,
        home_gen: u32,
        first_seq: u64,
        chain_before: Hash32,
        last_seq: u64,
        chain_last: Hash32,
        blob_id: [u8; 16],
        devices: Vec<SeedDevice>,
    },
    SeedTail { host: String, first: u64, frames: Vec<Vec<u8>> },
    Compact { below_seq: u64, snap_entry_seq: u64, blob_id: [u8; 16], keep: Vec<u64> },
    RestoreTail { first_seq: u64, chain_before: Hash32, frames: Vec<Vec<u8>> },
    Purge { before_seq: u64, epoch: u32 },
}

/// A device row of a `Seed` request (engine.md §11.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeedDevice {
    pub dev: Dev,
    pub sign_pk: Hash32,
    pub role: u8,
    pub status: u8,
    pub last_ctr: u64,
}

/// A successful admin answer (engine.md §11.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdminOk {
    pub seq: Option<u64>,
    pub chain: Option<Hash32>,
    pub token: bool,
    pub deleted: Option<u64>,
    pub floor: Option<u64>,
    pub more: Option<bool>,
    pub purge_floor: Option<u64>,
    pub done: Option<u64>,
    pub head: Option<Head>,
    pub door: Option<[u8; 5]>,
}

/// A relay refusal (engine.md §11.7, §11.9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayRefusal {
    pub status: u16,
    pub code: String,
    pub until: Option<i64>,
    pub head: Option<Head>,
    pub index: Option<u32>,
}
