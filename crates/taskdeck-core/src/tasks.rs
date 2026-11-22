//! Scoring and the item and rule queries, copied from today's `src/tasks.rs`
//! (core.md §14.2, §20).

use crate::board::{Item, Recurrence};
use crate::time::Instant;

/// Weekday names, Monday first (today's tasks; core.md §14.2).
pub const WEEKDAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
/// The rule bits of every day (today's tasks; core.md §6.4).
pub const EVERY_DAY: u8 = 0b0111_1111;
/// The rule bits of Monday to Friday (today's tasks; core.md §6.4).
pub const WEEKDAYS: u8 = 0b0001_1111;
/// The severity labels (today's tasks; core.md §14.2).
pub const IMPORTANCE_LABELS: [&str; 5] =
    ["Not important", "Mildly important", "Important", "Highly important", "Lethally important"];
/// The horizon labels (today's tasks; core.md §14.2).
pub const HORIZON_LABELS: [&str; 4] = ["Within a month", "Within a week", "Within days", "Whenever"];
/// The order the horizons are shown in (today's tasks; core.md §14.2).
pub const HORIZON_DISPLAY_ORDER: [u8; 4] = [2, 1, 0, 3];
/// The palette index of routines (today's tasks; core.md §14.2).
pub const ROUTINE_COLOR_INDEX: usize = 0;
/// The palette's length (today's tasks; core.md §6.9).
pub const PALETTE_LEN: usize = 6;
/// The horizon level "whenever" (today's tasks; core.md §14.2).
pub const HORIZON_WHENEVER: u8 = 3;

/// Score weight by importance (today's scoring table; core.md §14.2).
pub const WEIGHT_BY_IMPORTANCE: [f32; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];
/// Lead days by importance (today's scoring table; core.md §14.2).
pub const LEAD_DAYS_BY_IMPORTANCE: [f32; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];
/// Score weight by horizon (today's scoring table; core.md §14.2).
pub const WEIGHT_BY_HORIZON: [f32; 4] = [1.0, 2.0, 4.0, 0.5];
/// Ripening days by horizon (today's scoring table; core.md §14.2).
pub const RIPEN_DAYS_BY_HORIZON: [f32; 4] = [30.0, 10.0, 3.0, 90.0];
/// Lead days of a planned task (today's scoring table; core.md §14.2).
pub const PLANNED_LEAD_DAYS: f32 = 0.5;
/// Days in which overdue pressure doubles (today's scoring table; core.md §14.2).
pub const OVERDUE_DOUBLING_DAYS: f32 = 1.0;
/// The cap on overdue pressure (today's scoring table; core.md §14.2).
pub const OVERDUE_PRESSURE_CAP: f32 = 4.0;
/// The importance assumed when none is set (today's scoring table; core.md §14.2).
pub const ASSUMED_IMPORTANCE: u8 = 2;
/// The score of a malformed item (today's scoring table; core.md §14.2).
pub const MALFORMED_SCORE: f32 = 1.0e6;
/// The size of the tie-break jitter (today's scoring table; core.md §14.2).
pub const JITTER: f32 = 0.08;

/// The rule bit of a day's weekday (today's tasks; core.md §6.4).
pub fn weekday_bit(day: chrono::NaiveDate) -> u8 {
    unimplemented!()
}

/// The palette index of an item's levels (today's tasks; core.md §14.2).
pub fn calendar_item_color(is_event: bool, importance: Option<u8>, time_importance: Option<u8>) -> usize {
    unimplemented!()
}

impl Recurrence {
    /// Whether the rule fires on a day (core.md §14.2).
    pub fn falls_on(&self, day: chrono::NaiveDate) -> bool {
        unimplemented!()
    }

    /// Whether the rule repeats (core.md §14.2).
    pub fn repeats(&self) -> bool {
        unimplemented!()
    }

    /// Whether the rule includes a weekday index (core.md §14.2).
    pub fn includes(&self, index: u32) -> bool {
        unimplemented!()
    }

    /// The rule as text (core.md §14.2).
    pub fn summary(&self) -> String {
        unimplemented!()
    }

    /// The rule as text with its date (core.md §14.2).
    pub fn summary_dated(&self) -> String {
        unimplemented!()
    }
}

impl Item {
    /// Today's score, including its tables and jitter (core.md §14.2).
    pub fn importance_score(&self, now: Instant, seed: u64) -> f32 {
        unimplemented!()
    }

    /// Today's tie-break factor in `[1, 1 + JITTER)` (core.md §14.2).
    pub fn tie_break_jitter(&self, seed: u64) -> f32 {
        unimplemented!()
    }

    /// Whether the item is a routine (core.md §14.2).
    pub fn is_routine(&self) -> bool {
        unimplemented!()
    }

    /// The item's palette index (core.md §14.2).
    pub fn calendar_item_color(&self) -> usize {
        unimplemented!()
    }

    /// The instant the planner anchors the item at (core.md §14.2).
    pub fn planner_anchor(&self) -> Option<Instant> {
        unimplemented!()
    }

    /// Whether the item has a block (core.md §14.2).
    pub fn is_planned(&self) -> bool {
        unimplemented!()
    }

    /// The minutes of the item's blocks (core.md §14.2).
    pub fn planned_minutes(&self) -> u32 {
        unimplemented!()
    }

    /// The estimate not yet planned (core.md §14.2).
    pub fn remaining_minutes(&self) -> Option<u32> {
        unimplemented!()
    }

    /// Whether the item waits to be planned (core.md §14.2).
    pub fn wants_planning(&self) -> bool {
        unimplemented!()
    }
}
