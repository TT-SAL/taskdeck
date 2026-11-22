//! Board data (core.md §6, §20).

use crate::CoreError;
use crate::time::Instant;

/// An item's kind (core.md §6.2).
pub enum Kind {
    Task,
    Event,
    Routine,
}

/// How an archive row left the board (core.md §6.6).
pub enum Outcome {
    Finished,
    Dropped,
}

/// A field group's stamp (core.md §6.5, §10.1).
pub struct Stamp {
    pub seq: u64,
    pub device_no: u16,
}

/// An item's stamps per field group (core.md §6.5).
pub struct ItemStamps {
    pub estimate: Stamp,
    pub levels: Stamp,
    pub name: Stamp,
    pub repeat: Stamp,
    pub time: Stamp,
}

/// A routine's rule (core.md §6.4).
pub struct Recurrence {
    pub days: u8,
    pub anchor: chrono::NaiveDate,
    pub start_minutes: i32,
    pub minutes: u32,
}

/// A planned block of a live item (core.md §6.3).
pub struct Block {
    pub sid: u64,
    pub start: Instant,
    pub minutes: u32,
    pub stamp: Stamp,
}

/// A block of an archive row (core.md §6.6).
pub struct RowBlock {
    pub sid: u64,
    pub start: Instant,
    pub minutes: u32,
}

/// A live item (core.md §6.2).
pub struct Item {
    pub id: u64,
    pub kind: Kind,
    pub name: String,
    pub created: Instant,
    pub deadline: Option<Instant>,
    pub importance: Option<u8>,
    pub time_importance: Option<u8>,
    pub duration_minutes: Option<u32>,
    pub recurrence: Option<Recurrence>,
    pub blocks: Vec<Block>,
    pub stamps: ItemStamps,
}

/// An archived item (core.md §6.6).
pub struct ArchiveRow {
    pub id: u64,
    pub kind: Kind,
    pub name: String,
    pub created: Instant,
    pub deadline: Option<Instant>,
    pub importance: Option<u8>,
    pub time_importance: Option<u8>,
    pub duration_minutes: Option<u32>,
    pub recurrence: Option<Recurrence>,
    pub blocks: Vec<RowBlock>,
    pub archived_at: Instant,
    pub outcome: Outcome,
}

/// The archive key (core.md §6.11).
pub struct ArchiveKey {
    pub id: u64,
    pub archived_at: Instant,
}

/// A calendar subscription (core.md §6.8).
pub struct Subscription {
    pub id: u64,
    pub name: String,
    pub url: String,
    pub color: [u8; 4],
    pub enabled: bool,
}

/// The home coordinates setting (core.md §6.9).
pub struct Home {
    pub lat: i32,
    pub lon: i32,
}

/// The picture setting (core.md §6.9).
pub struct PictureRef {
    pub blob: [u8; 16],
    pub epoch: u32,
    pub parts: u32,
    pub sha256: [u8; 32],
    pub size: u64,
}

/// A device's id mark (core.md §6.10, §7.2).
pub struct Mark {
    pub device_no: u16,
    pub item: u64,
    pub block: u64,
}

/// The board (core.md §6.1).
pub struct BoardData {
    pub rules: u32,
    pub items: Vec<Item>,
    pub archive: Vec<ArchiveRow>,
    pub unreadable: Vec<Vec<u8>>,
    pub notes: String,
    pub notes_rev: u64,
    pub subscriptions: Vec<Subscription>,
    pub palette: [[u8; 4]; 6],
    pub home: Home,
    pub picture: Option<PictureRef>,
    pub marks: Vec<Mark>,
}

impl BoardData {
    /// The empty board `{"rules":1}` (core.md §15.3).
    pub fn empty() -> Self {
        unimplemented!()
    }

    /// Canonical board bytes (core.md §6.13).
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        unimplemented!()
    }

    /// Loads a board from canonical bytes (core.md §6.14).
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, CoreError> {
        unimplemented!()
    }

    /// Checks the invariants (core.md §6.12).
    pub fn check(&self) -> Result<(), CoreError> {
        unimplemented!()
    }

    /// The live item with this id (core.md §6.1).
    pub fn item(&self, id: u64) -> Option<&Item> {
        unimplemented!()
    }

    /// The subscription with this id (core.md §6.8).
    pub fn subscription(&self, id: u64) -> Option<&Subscription> {
        unimplemented!()
    }

    /// A device's mark, zero when absent (core.md §6.10).
    pub fn mark(&self, device_no: u16) -> Mark {
        unimplemented!()
    }
}
