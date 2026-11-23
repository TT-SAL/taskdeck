//! The archive's verdicts, filter, summary and months, copied from today's
//! `src/archive.rs` (core.md §14.2, §20).
//!
//! The archive keeps the **whole item**, plus how it left — finished, or
//! dropped. With the deadline and the finishing time you can say whether the
//! date was met; with the estimate and the blocks you can say whether the guess
//! was any good. `ArchiveRow::verdict` writes that line.
//!
//! The log file, its loading and its rewrites are not here: the rows live in
//! `BoardData.archive` (core.md §6.6).

use std::ops::Range;

use chrono::{Datelike, Timelike};

use crate::board::{ArchiveRow, Kind, Outcome};
use crate::planner::{format_duration, format_minutes};
use crate::tasks::{ROUTINE_COLOR_INDEX, calendar_item_color};
use crate::time::{self, Zone};

/// Beat a deadline (or miss it) by less than this and it reads as neither: the
/// date was met on the nose. Half an hour is about the resolution at which
/// "early" and "late" stop being interesting facts about a day's work.
const ON_THE_NOSE_MINUTES: i64 = 30;

/// The mark that stands in front of the name in the ledger (today's
/// `Outcome::glyph`).
fn glyph(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Finished => "✓",
        Outcome::Dropped => "✗",
    }
}

/// The verb the verdict line starts from (today's `Outcome::verb`).
fn verb(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Finished => "finished",
        Outcome::Dropped => "dropped",
    }
}

/// Whether a deadline was kept, and by how much.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    /// Nothing was owed, so nothing was met or missed.
    Undated,
    /// Done with this much of the deadline still to go.
    Met(chrono::TimeDelta),
    /// Done this long after it.
    Missed(chrono::TimeDelta),
}

impl ArchiveRow {
    /// The one line the ledger prints under the name: what happened, what it
    /// cost, and how long it had been hanging around. An event's time is
    /// printed in `zone` (core.md §5.3, CORE-TIME-002).
    pub fn verdict(&self, zone: &(impl Zone + ?Sized)) -> String {
        let mut parts: Vec<String> = Vec::new();

        if let Some(rule) = self.recurrence.filter(|_| self.is_routine()) {
            // A routine was never work, so there is nothing to be early or late
            // about and no time to hold against an estimate. What it *was* is
            // the standing arrangement itself, so that is what the row reports.
            parts.push(if rule.repeats() { "routine dropped" } else { "time dropped" }.to_string());
            parts.push(format!(
                "{} at {} for {}",
                rule.summary_dated(),
                format_minutes(rule.start_minutes),
                format_duration(rule.minutes)
            ));
        } else if self.kind == Kind::Event {
            // An event is never "finished" — it is a time that arrives and
            // passes on its own, which is why the planner offers no ✓ on one.
            // So the only thing to report is that it was taken off the
            // calendar, and when it had been set for.
            parts.push("event removed".to_string());
            if let Some(when) = self.deadline {
                // chrono's `%-d %b %H:%M` of the local time.
                let local = time::local_date_time(when, zone);
                parts.push(format!(
                    "was set for {} {} {:02}:{:02}",
                    local.day(),
                    time::month_abbrev(local.month()),
                    local.hour(),
                    local.minute()
                ));
            }
        } else {
            parts.push(self.outcome_phrase());
            parts.push(self.time_phrase());
        }

        parts.push(format!("{} on the board", format_span(self.lifetime())));
        parts.join("  ·  ")
    }

    /// "finished 2d early" / "dropped" / "finished 3h late".
    fn outcome_phrase(&self) -> String {
        let verb = verb(self.outcome);
        match self.timing() {
            Timing::Undated => verb.to_string(),
            Timing::Met(spare) | Timing::Missed(spare) if spare.num_minutes() <= ON_THE_NOSE_MINUTES => {
                format!("{verb} right on the deadline")
            }
            Timing::Met(spare) => format!("{verb} {} early", format_span(spare)),
            Timing::Missed(over) => format!("{verb} {} late", format_span(over)),
        }
    }

    /// What it cost, and what it was said it would cost: the estimate is a
    /// promise made before the work, the blocks are the time actually set
    /// aside for it.
    fn time_phrase(&self) -> String {
        let booked = self.booked_minutes();
        let spent = match self.sittings() {
            0 => None,
            1 => Some(format!("{} in one sitting", format_duration(booked))),
            sittings => Some(format!("{} over {sittings} sittings", format_duration(booked))),
        };

        match (self.duration_minutes, spent) {
            (Some(estimate), Some(spent)) => format!("{} estimated, {spent}", format_duration(estimate)),
            (Some(estimate), None) => format!("{} estimated, never booked", format_duration(estimate)),
            (None, Some(spent)) => spent,
            (None, None) => "no time set aside".to_string(),
        }
    }

    /// Whether the deadline was kept. Events are `Undated` by construction:
    /// an event's "deadline" is when it happened, not a promise it could break.
    pub fn timing(&self) -> Timing {
        // Neither an event nor a routine can be early or late: an event's
        // "deadline" is when it happened, and a routine never had one.
        if self.kind == Kind::Event || self.is_routine() {
            return Timing::Undated;
        }
        match self.deadline {
            None => Timing::Undated,
            Some(deadline) if self.archived_at <= deadline => Timing::Met(time::between(self.archived_at, deadline)),
            Some(deadline) => Timing::Missed(time::between(deadline, self.archived_at)),
        }
    }

    /// True when this is a task that was genuinely ticked off.
    ///
    /// Not simply `outcome == Finished`. An event is never finished — it is a
    /// time that arrives and passes on its own — and neither is a routine,
    /// which is a standing arrangement rather than a piece of work. Legacy rows
    /// written before `outcome` existed *all* default to `Finished`, events
    /// among them. So the kind of thing decides first and the stored outcome
    /// only breaks the tie.
    pub fn was_finished(&self) -> bool {
        self.kind != Kind::Event && !self.is_routine() && self.outcome == Outcome::Finished
    }

    /// True when this row counts towards the "deadlines met" figure: a finished
    /// task that actually carried a date. A dropped task broke no promise it
    /// had made — it was withdrawn — and an event never made one.
    pub fn was_judged(&self) -> bool {
        self.was_finished() && self.deadline.is_some()
    }

    /// The mark the ledger puts in front of the name: ✓ for work that was
    /// done, ✗ for everything that merely left.
    pub fn mark(&self) -> &'static str {
        if self.was_finished() { glyph(Outcome::Finished) } else { glyph(Outcome::Dropped) }
    }

    /// How long it sat on the board before leaving it.
    pub fn lifetime(&self) -> chrono::TimeDelta {
        time::between(self.created, self.archived_at).max(chrono::TimeDelta::zero())
    }

    /// Total minutes booked across every block.
    pub fn booked_minutes(&self) -> u32 {
        self.blocks.iter().map(|block| block.minutes).sum()
    }

    /// How many separate blocks of time it was given.
    pub fn sittings(&self) -> usize {
        self.blocks.len()
    }

    /// Palette index, by the same rule the calendar and planner use, so a
    /// ghost on the timeline wears the colour the item wore in life.
    pub fn color_id(&self) -> usize {
        if self.is_routine() {
            return ROUTINE_COLOR_INDEX;
        }
        calendar_item_color(self.kind == Kind::Event, self.importance, self.time_importance)
    }

    /// True when this was a standing commitment: its `kind` is `Routine`
    /// (core.md §6.6).
    pub fn is_routine(&self) -> bool {
        self.kind == Kind::Routine
    }
}

/// A span as one coarse unit — `40m`, `6h`, `12d`, `4mo`.
///
/// Deliberately one unit and rounded down: a ledger is skimmed, and "finished
/// 2d early" is the fact, while "2d 3h 14m early" is the fact buried in noise.
pub fn format_span(span: chrono::TimeDelta) -> String {
    let minutes = span.num_minutes().max(0);
    if minutes < 1 {
        return "a moment".to_string();
    }
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = span.num_hours();
    if hours < 24 {
        return format!("{hours}h");
    }
    let days = span.num_days();
    if days < 90 {
        return format!("{days}d");
    }
    format!("{}mo", days / 30)
}

/// A running total of minutes, in the unit a total is read in. Separate from
/// `planner::format_duration` because that formats *one block* (`1h 30m`) and a
/// column of them adds up to numbers where the odd minutes are noise.
pub fn format_total(minutes: u64) -> String {
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    match minutes % 60 {
        0 => format!("{hours}h"),
        rest if hours < 10 => format!("{hours}h {rest}m"),
        // Past ten hours the leftover minutes stop carrying information.
        _ => format!("{hours}h"),
    }
}

/// What to show. Every field narrows; an empty filter admits everything.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Case-insensitive substring of the name.
    pub query: String,
    pub outcome: OutcomeFilter,
    pub kind: KindFilter,
}

/// The filter by outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutcomeFilter {
    #[default]
    Any,
    Finished,
    Dropped,
}

/// The filter by kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KindFilter {
    #[default]
    Any,
    Tasks,
    Events,
}

impl Filter {
    /// Whether the filter admits a row.
    pub fn admits(&self, row: &ArchiveRow) -> bool {
        // Keyed on `was_finished` rather than on the stored outcome, so the two
        // branches between them still cover every row: an event has no
        // "finished" state, and folding it under "dropped" reads correctly —
        // it was taken off the calendar.
        match self.outcome {
            OutcomeFilter::Any => {}
            OutcomeFilter::Finished if !row.was_finished() => return false,
            OutcomeFilter::Dropped if row.was_finished() => return false,
            _ => {}
        }
        let is_event = row.kind == Kind::Event;
        match self.kind {
            KindFilter::Any => {}
            KindFilter::Tasks if is_event => return false,
            KindFilter::Events if !is_event => return false,
            _ => {}
        }

        let query = self.query.trim();
        if query.is_empty() {
            return true;
        }
        row.name.to_lowercase().contains(&query.to_lowercase())
    }

    /// True when the filter is doing nothing, so the window can say "47 rows"
    /// rather than "47 of 47 rows".
    pub fn is_open(&self) -> bool {
        self.query.trim().is_empty() && self.outcome == OutcomeFilter::Any && self.kind == KindFilter::Any
    }
}

/// What a set of rows adds up to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    /// Tasks ticked off.
    pub finished: usize,
    /// Tasks deleted.
    pub dropped: usize,
    /// Events taken off the calendar. Counted apart from the two above rather
    /// than lumped in with them: an event is neither finished nor abandoned,
    /// and folding it into "finished" would inflate the one figure in the
    /// headline that is supposed to mean work done.
    pub events: usize,
    /// Routines that were given up. Same argument again: a standing arrangement
    /// you stopped keeping is not a task you failed.
    pub routines: usize,
    /// Finished, dated tasks — the ones a deadline could be kept or broken on.
    pub judged: usize,
    /// How many of those were done by their date.
    pub met: usize,
    /// Total booked time. `u64` because this is the one figure that adds up
    /// across years and has no business overflowing.
    pub booked_minutes: u64,
    /// Median time from being written down to leaving the board. The median
    /// rather than the mean: one task forgotten for two years should not become
    /// the headline figure for how you work.
    pub median_lifetime: Option<chrono::TimeDelta>,
}

impl Summary {
    /// The masthead line: "41 finished · 6 dropped · 29 of 34 deadlines met · …"
    pub fn headline(&self) -> String {
        if self.finished == 0 && self.dropped == 0 && self.events == 0 && self.routines == 0 {
            return "nothing here yet".to_string();
        }

        let mut parts = Vec::new();
        if self.finished > 0 {
            parts.push(format!("{} finished", self.finished));
        }
        if self.dropped > 0 {
            parts.push(format!("{} dropped", self.dropped));
        }
        if self.events > 0 {
            parts.push(format!("{} event{} removed", self.events, if self.events == 1 { "" } else { "s" }));
        }
        if self.routines > 0 {
            parts.push(format!("{} routine{} dropped", self.routines, if self.routines == 1 { "" } else { "s" }));
        }
        if self.judged > 0 {
            parts.push(format!("{} of {} deadlines met", self.met, self.judged));
        }
        if self.booked_minutes > 0 {
            parts.push(format!("{} booked", format_total(self.booked_minutes)));
        }
        if let Some(median) = self.median_lifetime {
            parts.push(format!("typically {} on the board", format_span(median)));
        }
        parts.join("  ·  ")
    }
}

/// Fold a set of rows into what they add up to.
pub fn summarize<'a>(rows: impl IntoIterator<Item = &'a ArchiveRow>) -> Summary {
    let mut summary = Summary::default();
    let mut lifetimes: Vec<chrono::TimeDelta> = Vec::new();

    for row in rows {
        if row.is_routine() {
            summary.routines += 1;
        } else if row.kind == Kind::Event {
            summary.events += 1;
        } else if row.was_finished() {
            summary.finished += 1;
        } else {
            summary.dropped += 1;
        }
        if row.was_judged() {
            summary.judged += 1;
            if matches!(row.timing(), Timing::Met(_)) {
                summary.met += 1;
            }
        }
        summary.booked_minutes += u64::from(row.booked_minutes());
        lifetimes.push(row.lifetime());
    }

    if !lifetimes.is_empty() {
        lifetimes.sort();
        summary.median_lifetime = Some(lifetimes[lifetimes.len() / 2]);
    }
    summary
}

/// A run of consecutive rows sharing a calendar month, with the heading it
/// wants. The ledger is read as a journal, and a journal has months in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthRun {
    pub label: String,
    /// Indices into the slice that was grouped.
    pub rows: Range<usize>,
}

/// Break already-ordered rows into runs by the local month of `archived_at`
/// in `zone` (core.md §5.3, CORE-TIME-002).
///
/// Runs, not buckets: the input is newest-first and stays that way, so a month
/// that somehow appears twice draws two headings rather than silently merging
/// out-of-order rows.
pub fn group_by_month(rows: &[&ArchiveRow], zone: &(impl Zone + ?Sized)) -> Vec<MonthRun> {
    let month_of = |row: &ArchiveRow| {
        let day = time::local_day(row.archived_at, zone);
        (day.year(), day.month())
    };
    let mut runs: Vec<MonthRun> = Vec::new();

    for (index, row) in rows.iter().enumerate() {
        let month = month_of(row);
        let same_as_last = runs.last().is_some_and(|run| month_of(rows[run.rows.end - 1]) == month);

        match runs.last_mut() {
            Some(run) if same_as_last => run.rows.end = index + 1,
            _ => runs.push(MonthRun {
                // chrono's `%B %Y` of the local month.
                label: format!("{} {}", time::month_name(month.1), month.0),
                rows: index..index + 1,
            }),
        }
    }
    runs
}

#[cfg(test)]
mod tests;
