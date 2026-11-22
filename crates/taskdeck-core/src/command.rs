//! Log commands v1 (core.md §9, §20).

use crate::CoreError;
use crate::board::PictureRef;
use crate::time::Instant;

/// A log command (core.md §9.3).
pub enum LogCommand {
    CreateTask(CreateTask),
    CreateEvent(CreateEvent),
    CreateRoutine(CreateRoutine),
    Rename { id: u64, name: String },
    Complete { id: u64, at: Instant },
    Delete { id: u64, at: Instant },
    Forget { id: u64 },
    MoveBlock { id: u64, sid: u64, start: Instant, minutes: u32 },
    MoveBlocks { moves: Vec<BlockMove> },
    AddBlock { id: u64, sid: u64, start: Instant, minutes: u32 },
    RemoveBlocks { blocks: Vec<(u64, u64)> },
    SetEstimate { id: u64, minutes: u32 },
    SetDeadline { id: u64, deadline: Option<Instant>, seed: Option<u8> },
    SetSeverity { id: u64, level: u8 },
    SetHorizon { id: u64, level: u8 },
    SetRepeat { id: u64, days: u8, anchor: chrono::NaiveDate },
    MoveRoutine { id: u64, start_minutes: i32, minutes: u32 },
    SetEventSpan { id: u64, start: Instant, minutes: u32 },
    SetNotes { text: String, base: u64 },
    Restore { id: u64, archived_at: Instant, fallback: u64 },
    ForgetArchived { id: u64, archived_at: Instant },
    AddSubscription { id: u64, name: String, url: String, color: [u8; 4] },
    RemoveSubscription { id: u64 },
    RenameSubscription { id: u64, name: String },
    SetSubscriptionColor { id: u64, color: [u8; 4] },
    SetSubscriptionEnabled { id: u64, enabled: bool },
    SetPalette { colors: [[u8; 4]; 6] },
    SetHome { lat: i32, lon: i32 },
    SetPicture { picture: Option<PictureRef> },
}

/// `create_task` (core.md §9.4).
pub struct CreateTask {
    pub id: u64,
    pub name: String,
    pub deadline: Option<Instant>,
    pub importance: Option<u8>,
    pub time_importance: Option<u8>,
    pub duration_minutes: Option<u32>,
    pub blocks: Vec<NewBlock>,
}

/// `create_event` (core.md §9.5).
pub struct CreateEvent {
    pub id: u64,
    pub name: String,
    pub start: Instant,
    pub minutes: Option<u32>,
    pub importance: Option<u8>,
    pub time_importance: Option<u8>,
}

/// `create_routine` (core.md §9.6).
pub struct CreateRoutine {
    pub id: u64,
    pub name: String,
    pub days: u8,
    pub anchor: chrono::NaiveDate,
    pub start_minutes: i32,
    pub minutes: u32,
}

/// A block of `create_task` (core.md §9.2, §9.4).
pub struct NewBlock {
    pub sid: u64,
    pub start: Instant,
    pub minutes: u32,
}

/// One move of `move_blocks` (core.md §9.11).
pub struct BlockMove {
    pub id: u64,
    pub sid: u64,
    pub start: Instant,
    pub minutes: u32,
}

impl LogCommand {
    /// The command's `op` string (core.md §9.1).
    pub fn op(&self) -> &'static str {
        unimplemented!()
    }

    /// The command's canonical bytes (core.md §9.1, §4.2).
    pub fn to_canonical(&self) -> Vec<u8> {
        unimplemented!()
    }

    /// A `create_*` with another name, else `Err(self)` (core.md §20, V2-17).
    pub fn with_name(self, name: String) -> Result<LogCommand, LogCommand> {
        unimplemented!()
    }

    /// The item or subscription the command targets (core.md §9.3).
    pub fn target(&self) -> Option<u64> {
        unimplemented!()
    }
}

/// Parses a command; `apply` checks the rules (core.md §9.1).
pub fn parse_command(bytes: &[u8]) -> Result<LogCommand, CoreError> {
    unimplemented!()
}
