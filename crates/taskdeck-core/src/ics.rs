//! Today's ICS parser with the zone parameter (core.md §14.6, §20).

use serde::{Deserialize, Serialize};

use crate::time::Zone;

/// One occurrence of a calendar event (today's ics; core.md §14.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occurrence {
    pub day: chrono::NaiveDate,
    pub start: i32,
    pub end: i32,
    pub all_day: bool,
    pub free: bool,
    pub name: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub description: String,
}

impl Occurrence {
    /// Whether the occurrence makes its time busy (today's ics; core.md §14.6).
    pub fn is_busy(&self) -> bool {
        unimplemented!()
    }
}

/// What a parse produced (today's ics; core.md §14.6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    pub occurrences: Vec<Occurrence>,
    pub name: Option<String>,
    pub problems: Vec<String>,
}

/// The parser's limits (today's ics; core.md §14.6).
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_depth: usize,
    pub max_lines: usize,
    pub max_events: usize,
    pub max_occurrences_per_event: usize,
    pub max_occurrences: usize,
    pub max_rule_steps: usize,
}

/// Today's `Limits::default()`, unchanged (core.md §14.6).
impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_depth: 8,
            max_lines: 200_000,
            max_events: 20_000,
            max_occurrences_per_event: 1_000,
            max_occurrences: 50_000,
            max_rule_steps: 100_000,
        }
    }
}

/// Parses calendar text for `[from, until]` in the zone (core.md §14.6).
pub fn parse(
    text: &str,
    from: chrono::NaiveDate,
    until: chrono::NaiveDate,
    limits: Limits,
    zone: &(impl Zone + ?Sized),
) -> Result<Parsed, String> {
    unimplemented!()
}
