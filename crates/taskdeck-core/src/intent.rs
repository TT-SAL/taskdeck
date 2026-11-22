//! Intents (core.md §12, §20).

use crate::CompileError;
use crate::board::{Kind, PictureRef};

/// A wire time: exact, or naive with its text (core.md §12.2 step 4).
pub enum WireTime {
    Exact(chrono::DateTime<chrono::FixedOffset>),
    Naive(chrono::NaiveDateTime, String),
}

/// An intent, one variant per row of the table (core.md §12.3).
pub enum Intent {
    Add {
        name: String,
        deadline: Option<WireTime>,
        importance: Option<u8>,
        is_event: bool,
        time_importance: Option<u8>,
    },
    QuickAdd { name: String, deadline: Option<WireTime> },
    Create { kind: Kind, name: String, day: chrono::NaiveDate, start: i32, minutes: u32 },
    Rename { id: u64, name: String },
    Complete { id: u64, at: Option<WireTime> },
    Delete { id: u64, at: Option<WireTime> },
    Forget { id: u64 },
    MoveBlock { id: u64, session: Option<u64>, day: chrono::NaiveDate, start: i32, minutes: u32 },
    AddBlock { id: u64, day: chrono::NaiveDate, start: i32, minutes: Option<u32> },
    RemoveBlock { id: u64, session: u64 },
    Unplan { id: u64 },
    SetEstimate { id: u64, minutes: u32 },
    SetDeadline { id: u64, deadline: Option<WireTime> },
    SetSeverity { id: u64, level: u8 },
    SetHorizon { id: u64, level: u8 },
    SetRepeat { id: u64, days: u8, day: chrono::NaiveDate },
    Reflow { day: chrono::NaiveDate, from: Option<i32> },
    SetNotes { text: String, base: Option<u64> },
    Restore { id: u64, archived_at: chrono::DateTime<chrono::FixedOffset> },
    ForgetArchived { id: u64, archived_at: chrono::DateTime<chrono::FixedOffset> },
    AddSubscription { name: String, url: String, color: Option<[u8; 4]> },
    RemoveSubscription { id: u64 },
    RenameSubscription { id: u64, name: String },
    SetSubscriptionColor { id: u64, color: [u8; 4] },
    SetSubscriptionEnabled { id: u64, enabled: bool },
    SetPalette { colors: [[u8; 4]; 6] },
    SetHome { lat: f64, lon: f64 },
    SetPicture { picture: Option<PictureRef> },
}

/// Parses intent JSON (core.md §12.2).
pub fn parse_intent(text: &str) -> Result<Intent, CompileError> {
    unimplemented!()
}
