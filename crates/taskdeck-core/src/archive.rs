//! The archive's verdicts, filter, summary and months, copied from today's
//! `src/archive.rs` (core.md §14.2, §20).

use std::ops::Range;

use crate::board::ArchiveRow;
use crate::time::Zone;

/// How an archive row met its deadline (today's archive; core.md §14.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    Undated,
    Met(chrono::TimeDelta),
    Missed(chrono::TimeDelta),
}

impl ArchiveRow {
    /// The row's verdict text, in the zone (core.md §14.2).
    pub fn verdict(&self, zone: &(impl Zone + ?Sized)) -> String {
        unimplemented!()
    }

    /// How the row met its deadline (core.md §14.2).
    pub fn timing(&self) -> Timing {
        unimplemented!()
    }

    /// Whether the row was finished (core.md §14.2).
    pub fn was_finished(&self) -> bool {
        unimplemented!()
    }

    /// Whether the row has a verdict (core.md §14.2).
    pub fn was_judged(&self) -> bool {
        unimplemented!()
    }

    /// The row's mark (core.md §14.2).
    pub fn mark(&self) -> &'static str {
        unimplemented!()
    }

    /// From creation to archiving (core.md §14.2).
    pub fn lifetime(&self) -> chrono::TimeDelta {
        unimplemented!()
    }

    /// The minutes of the row's blocks (core.md §14.2).
    pub fn booked_minutes(&self) -> u32 {
        unimplemented!()
    }

    /// The number of the row's blocks (core.md §14.2).
    pub fn sittings(&self) -> usize {
        unimplemented!()
    }

    /// The row's palette index (core.md §14.2).
    pub fn color_id(&self) -> usize {
        unimplemented!()
    }

    /// Whether the row is a routine (core.md §14.2).
    pub fn is_routine(&self) -> bool {
        unimplemented!()
    }
}

/// A span as text (today's archive; core.md §14.2).
pub fn format_span(span: chrono::TimeDelta) -> String {
    unimplemented!()
}

/// Total minutes as text (today's archive; core.md §14.2).
pub fn format_total(minutes: u64) -> String {
    unimplemented!()
}

/// The archive view's filter (today's archive; core.md §14.2).
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub query: String,
    pub outcome: OutcomeFilter,
    pub kind: KindFilter,
}

/// The filter by outcome (today's archive; core.md §14.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutcomeFilter {
    #[default]
    Any,
    Finished,
    Dropped,
}

/// The filter by kind (today's archive; core.md §14.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KindFilter {
    #[default]
    Any,
    Tasks,
    Events,
}

impl Filter {
    /// Whether the filter admits a row (core.md §14.2).
    pub fn admits(&self, row: &ArchiveRow) -> bool {
        unimplemented!()
    }

    /// Whether the filter narrows anything (core.md §14.2).
    pub fn is_open(&self) -> bool {
        unimplemented!()
    }
}

/// Counts over archive rows (today's archive; core.md §14.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub finished: usize,
    pub dropped: usize,
    pub events: usize,
    pub routines: usize,
    pub judged: usize,
    pub met: usize,
    pub booked_minutes: u64,
    pub median_lifetime: Option<chrono::TimeDelta>,
}

impl Summary {
    /// The summary line (core.md §14.2).
    pub fn headline(&self) -> String {
        unimplemented!()
    }
}

/// Summarizes rows (core.md §14.2).
pub fn summarize<'a>(rows: impl IntoIterator<Item = &'a ArchiveRow>) -> Summary {
    unimplemented!()
}

/// A run of rows in one month (today's archive; core.md §14.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthRun {
    pub label: String,
    pub rows: Range<usize>,
}

/// Groups rows, newest first, by local month (core.md §14.2).
pub fn group_by_month(rows: &[&ArchiveRow], zone: &(impl Zone + ?Sized)) -> Vec<MonthRun> {
    unimplemented!()
}
