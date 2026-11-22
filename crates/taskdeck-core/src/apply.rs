//! `Ctx`, `apply`, conflict rules and rules versions (core.md §8, §10, §11, §20).

use std::collections::BTreeSet;

use crate::CoreError;
use crate::board::{ArchiveKey, BoardData};
use crate::command::LogCommand;
use crate::time::Instant;

/// The context of one `apply` (core.md §8.1).
pub struct Ctx {
    pub seq: u64,
    pub device_no: u16,
    pub at: Instant,
    pub based_on: u64,
}

/// The result of an accepted command (core.md §8.3).
pub struct Applied {
    pub benign: bool,
    pub changes: ChangeSet,
    pub notices: Vec<Notice>,
    pub moved: u32,
    pub skipped: u32,
    pub restored_as: Option<u64>,
}

/// What an accepted command changed (core.md §8.3).
pub struct ChangeSet {
    pub items: BTreeSet<u64>,
    pub archive_inserted: Vec<ArchiveKey>,
    pub archive_removed: Vec<ArchiveKey>,
    pub notes: bool,
    pub subscriptions: BTreeSet<u64>,
    pub overlay_forget: BTreeSet<u64>,
    pub settings: bool,
    pub marks: bool,
}

/// A refused command (core.md §8.3).
pub struct Refusal {
    pub code: &'static str,
    pub op: &'static str,
    pub target: Option<u64>,
    pub notes: Option<NotesConflict>,
}

/// A `set_notes` whose base is not current (core.md §8.3, §10.5).
pub struct NotesConflict {
    pub base: u64,
    pub current: u64,
}

/// The field group of an overwrite notice (core.md §10.2, §10.7).
pub enum Group {
    Name,
    Time,
    Levels,
    Estimate,
    Repeat,
    Block,
    Item,
}

/// A notice record (core.md §10.7).
pub enum Notice {
    Overwrite { seq: u64, author: u16, other: u16, other_seq: u64, item: u64, sid: Option<u64>, group: Group },
    Skipped { seq: u64, author: u16, item: u64, sid: u64, reason: SkipReason, other: Option<(u16, u64)> },
}

/// Why a `move_blocks` move was skipped (core.md §10.3).
pub enum SkipReason {
    Changed,
    Gone,
}

/// A refusal record (core.md §10.7).
pub struct RefusalRecord {
    pub seq: u64,
    pub author: u16,
    pub code: String,
    pub op: Option<String>,
    pub target: Option<u64>,
    pub base: Option<u64>,
    pub current: Option<u64>,
}

impl Notice {
    /// The notice's canonical bytes (core.md §10.7).
    pub fn to_canonical(&self) -> Vec<u8> {
        unimplemented!()
    }

    /// A notice from canonical bytes (core.md §10.7).
    pub fn from_canonical(b: &[u8]) -> Result<Self, CoreError> {
        unimplemented!()
    }

    /// Whether the notice concerns a device (core.md §10.7).
    pub fn concerns(&self, device_no: u16) -> bool {
        unimplemented!()
    }
}

impl RefusalRecord {
    /// The record's canonical bytes (core.md §10.7).
    pub fn to_canonical(&self) -> Vec<u8> {
        unimplemented!()
    }

    /// A record from canonical bytes (core.md §10.7).
    pub fn from_canonical(b: &[u8]) -> Result<Self, CoreError> {
        unimplemented!()
    }
}

/// The rules versions this build applies (core.md §11.1).
pub const SUPPORTED_RULES: &[u32] = &[1];

/// The state-hash versions this build computes (core.md §6.13, §11).
pub const HASH_VERSIONS: &[u32] = &[1];

/// Applies one command (core.md §8.4).
pub fn apply(board: &mut BoardData, command: &LogCommand, ctx: &Ctx) -> Result<Applied, Refusal> {
    unimplemented!()
}

/// Moves the board to rules version `v` (core.md §11.2).
pub fn upgrade_rules(board: &mut BoardData, v: u32) -> Result<(), CoreError> {
    unimplemented!()
}

/// The reset board (core.md §11.3).
pub fn reset(target: &BoardData, current: &BoardData) -> Result<BoardData, CoreError> {
    unimplemented!()
}
