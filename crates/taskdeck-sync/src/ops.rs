//! User-started operations (engine.md §28).

use crate::Dev;

/// An operation the user starts (engine.md §28).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    RestoreTo { seq: u64 },
    Purge,
    Move { host: String },
    FollowAddress { host: String, door: [u8; 5], code: String },
    Revoke { dev: Dev, replace: bool },
    Skip { seq: u64 },
    RulesOverride { version: u32 },
    CloseBoard { host: String, door: [u8; 5] },
    Adopt { dev: Dev, typed_fp: String },
}

/// An operation in progress (engine.md §28, §7.8 `ops`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpRecord {
    pub ticket: u64,
    pub op: Operation,
    pub stage: u8,
    pub attempts: u8,
    pub started_ms: i64,
}
