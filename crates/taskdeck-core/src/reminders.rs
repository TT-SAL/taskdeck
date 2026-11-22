//! Reminders (core.md §14.5, §20).

use crate::board::BoardData;
use crate::time::Instant;

/// The lead of each reminder kind in minutes (core.md §14.5).
pub struct ReminderDefaults {
    pub event_lead_minutes: u32,
    pub block_lead_minutes: u32,
    pub due_lead_minutes: u32,
}

/// Leads of 10, 0 and 0 minutes (core.md §14.5, CORE-REM-001).
impl Default for ReminderDefaults {
    fn default() -> Self {
        ReminderDefaults { event_lead_minutes: 10, block_lead_minutes: 0, due_lead_minutes: 0 }
    }
}

/// What a reminder is for (core.md §14.5).
pub enum ReminderKind {
    Event,
    Block,
    Due,
}

/// One reminder (core.md §14.5).
pub struct Reminder {
    pub key: String,
    pub kind: ReminderKind,
    pub item: u64,
    pub sid: Option<u64>,
    pub at: Instant,
    pub fire_at: Instant,
    pub minutes: Option<u32>,
    pub name: String,
}

impl Reminder {
    /// The reminder's canonical bytes (core.md §14.5).
    pub fn to_canonical(&self) -> Vec<u8> {
        unimplemented!()
    }
}

/// The reminders firing in `[from, until)`, by `(fire_at, key)` (core.md §14.5).
pub fn reminders(board: &BoardData, from: Instant, until: Instant, defaults: &ReminderDefaults) -> Vec<Reminder> {
    unimplemented!()
}
