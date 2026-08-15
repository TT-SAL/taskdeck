//! The archive: what became of everything that has left the board.
//!
//! # Why this is more than a bin
//!
//! TaskDeck's one real claim is that **due is not the same as planned**: a
//! deadline says when something is owed, sessions say when you will actually
//! sit down and do it, and the app stores those separately (`DOCUMENTATION.md`
//! §16.1). Those two facts only become *checkable* at one moment — the moment
//! the thing is finished. Which is precisely the moment the old archive threw
//! them away: it kept a name, a created date, a deadline and a timestamp, and
//! dropped the sessions, the estimate and the horizon on the floor.
//!
//! So the archive here keeps the **whole item**, plus one new fact the old
//! record could not express: *how* it left — finished, or dropped. Three things
//! follow from that, and they are the whole point of the module:
//!
//! 1. **A verdict.** With the deadline and the finishing time you can say
//!    whether the date was met; with the estimate and the sessions you can say
//!    whether the guess was any good. `Archived::verdict` writes that line.
//! 2. **Restore.** A record that lost nothing can be turned back into the task
//!    it came from — `to_active` is the exact inverse of `retire` — so ticking
//!    something off by mistake is no longer permanent.
//! 3. **Ghosts.** The same completeness lets the day planner draw what you
//!    *actually* spent a past day on, behind what is still planned for it. The
//!    archive stops being a separate room and becomes part of the calendar.
//!
//! # The store
//!
//! One append-only JSONL file, `archived.jsonl`, read **once** into memory and
//! kept there for the session. That replaces the old paged reader, which
//! re-opened the file and reverse-scanned past `offset` lines on every "Show
//! more" — O(n) a page, O(n²) to walk the log — and which counted *raw* lines
//! while displaying *parsed* ones, so a single unreadable line silently skipped
//! or duplicated rows across page boundaries (`CODE_REVIEW.md` B4).
//!
//! Reading it whole is what makes searching, grouping and summarising possible
//! at all: you cannot compute "29 of 34 deadlines met" from a 15-row window.
//! The write path that runs constantly is still one appended line; the O(n)
//! rewrite happens only when a row is deliberately removed.
//!
//! Lines that fail to parse are **kept verbatim** rather than dropped, and
//! written back out on a rewrite. A log is not allowed to lose something merely
//! because this version of the program failed to understand it.

use std::{
    error::Error,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    ops::Range,
    path::Path,
};

use chrono::{DateTime, Datelike, Duration, Local};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{
    planner::{format_duration, format_minutes},
    tasks::{Active, Recurrence, Session, calendar_item_color, ROUTINE_COLOR_INDEX},
};

/// The log's file name inside `taskdeck_data/`.
pub const ARCHIVE_FILE: &str = "archived.jsonl";

/// Beat a deadline (or miss it) by less than this and it reads as neither: the
/// date was met on the nose. Half an hour is about the resolution at which
/// "early" and "late" stop being interesting facts about a day's work.
const ON_THE_NOSE_MINUTES: i64 = 30;

/* ─────────────────────────────── The record ─────────────────────────────── */

/// How an item left the board.
///
/// The old archive had no such field because it had no such concept: only
/// *completing* wrote a row, and deleting simply dropped the item — which made
/// the README's "completed and deleted items are not thrown away" untrue, and
/// meant a task you abandoned left no trace that it had ever existed. Both
/// paths archive now, and this is what tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Ticked off. `Default` because every row written before this field
    /// existed came from the complete path, so an absent `outcome` means
    /// exactly this.
    #[default]
    Finished,
    /// Deleted — abandoned, or never really a task in the first place.
    Dropped,
}

impl Outcome {
    /// The mark that stands in front of the name in the ledger.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Finished => "✓",
            Self::Dropped => "✗",
        }
    }

    /// The verb the verdict line starts from.
    pub fn verb(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::Dropped => "dropped",
        }
    }
}

/// One item, as it was when it left the board, and what became of it.
///
/// Every field of `Active` that carries meaning is here — including the
/// `sessions` and `duration_minutes` the old `InActive` discarded — so the row
/// is a faithful record rather than a receipt, and `to_active` can put it back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Archived {
    /// The `Active::id` this came from. `#[serde(default)]` for rows written
    /// before ids existed; `0` means "unidentified", and restoring one hands it
    /// a fresh id rather than trusting the sentinel.
    #[serde(default)]
    pub id: u64,
    pub name: String,
    pub created: DateTime<Local>,
    pub deadline: Option<DateTime<Local>>,
    pub is_event: bool,
    pub importance: Option<u8>,
    /// The horizon of an undated task. Dropped by the old record, which made a
    /// restored task look corrupt to the priority scorer (no deadline, no
    /// importance, no horizon — `MALFORMED_SCORE`).
    #[serde(default)]
    pub time_importance: Option<u8>,
    /// The time that was actually set aside for this. The single most
    /// interesting thing the archive holds, and the thing the old record threw
    /// away first.
    #[serde(default)]
    pub sessions: Vec<Session>,
    /// The estimate — what the user said it would take. Kept so the archive can
    /// hold the guess up against the booking.
    #[serde(default)]
    pub duration_minutes: Option<u32>,
    /// The weekly rule, when this was a routine. Kept for the same reason as
    /// everything else here: without it, putting a routine back would produce a
    /// nameless task with no deadline, no importance and no horizon — the exact
    /// shape the scorer reads as corrupt.
    #[serde(default)]
    pub recurrence: Option<Recurrence>,
    /// When it left the board.
    ///
    /// The wire name stays `inactivated`: that is what every line written
    /// before this redesign calls it, and an archive you cannot read is not an
    /// archive. The Rust name says what it means.
    #[serde(rename = "inactivated")]
    pub archived_at: DateTime<Local>,
    #[serde(default)]
    pub outcome: Outcome,
}

/// Identity of one row in the log.
///
/// `id` alone is not enough: rows written before ids existed all carry the
/// sentinel `0`, and a hand-edited log can repeat one. The instant it was
/// archived separates them, and together the pair is stable across the
/// rewrites that restoring and forgetting cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveKey {
    pub id: u64,
    pub archived_at: DateTime<Local>,
}

/// Whether a deadline was kept, and by how much.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    /// Nothing was owed, so nothing was met or missed.
    Undated,
    /// Done with this much of the deadline still to go.
    Met(Duration),
    /// Done this long after it.
    Missed(Duration),
}

impl Archived {
    /// Retire a live item into a record of itself.
    ///
    /// Takes the item by value: retiring is the end of its life as an `Active`,
    /// and consuming it means no caller can archive a copy and go on editing
    /// the original.
    pub fn retire(item: Active, outcome: Outcome, at: DateTime<Local>) -> Self {
        Self {
            id: item.id,
            name: item.name,
            created: item.created,
            deadline: item.deadline,
            is_event: item.is_event,
            importance: item.importance,
            time_importance: item.time_importance,
            sessions: item.sessions,
            duration_minutes: item.duration_minutes,
            recurrence: item.recurrence,
            archived_at: at,
            outcome,
        }
    }

    /// Turn the record back into the task it came from — the exact inverse of
    /// `retire`, which is only possible because nothing was dropped on the way
    /// in.
    ///
    /// The id comes back too, but the caller owns the question of whether it is
    /// still free (see `TaskApp::restore_archived`): the archive cannot know
    /// what the live set is holding.
    pub fn to_active(&self) -> Active {
        Active {
            id: self.id,
            importance: self.importance,
            time_importance: self.time_importance,
            name: self.name.clone(),
            created: self.created,
            deadline: self.deadline,
            is_event: self.is_event,
            sessions: self.sessions.clone(),
            // Spent at load time, and nothing here was ever written by a
            // pre-sessions build: a restored item is already migrated.
            planned_start: None,
            duration_minutes: self.duration_minutes,
            recurrence: self.recurrence,
        }
    }

    pub fn key(&self) -> ArchiveKey {
        ArchiveKey { id: self.id, archived_at: self.archived_at }
    }

    /// Palette index, by the same rule the calendar and planner use, so a
    /// ghost on the timeline wears the colour the item wore in life.
    pub fn color_id(&self) -> usize {
        if self.is_routine() {
            return ROUTINE_COLOR_INDEX;
        }
        calendar_item_color(self.is_event, self.importance, self.time_importance)
    }

    /// True when this was a standing weekly commitment. See
    /// `Active::recurrence`.
    pub fn is_routine(&self) -> bool {
        self.recurrence.is_some()
    }

    /// How long it sat on the board before leaving it.
    pub fn lifetime(&self) -> Duration {
        (self.archived_at - self.created).max(Duration::zero())
    }

    /// Total minutes booked across every session.
    pub fn booked_minutes(&self) -> u32 {
        self.sessions.iter().map(|session| session.minutes).sum()
    }

    /// How many separate blocks of time it was given.
    pub fn sittings(&self) -> usize {
        self.sessions.len()
    }

    /// Whether the deadline was kept. Events are `Undated` by construction:
    /// an event's "deadline" is when it happened, not a promise it could break.
    pub fn timing(&self) -> Timing {
        // Neither an event nor a routine can be early or late: an event's
        // "deadline" is when it happened, and a routine never had one.
        if self.is_event || self.is_routine() {
            return Timing::Undated;
        }
        match self.deadline {
            None => Timing::Undated,
            Some(deadline) if self.archived_at <= deadline => {
                Timing::Met(deadline - self.archived_at)
            }
            Some(deadline) => Timing::Missed(self.archived_at - deadline),
        }
    }

    /// True when this is a task that was genuinely ticked off.
    ///
    /// Not simply `outcome == Finished`. An event is never finished — it is a
    /// time that arrives and passes on its own, which is why the planner offers
    /// no ✓ on one — and neither is a routine, which is a standing arrangement
    /// rather than a piece of work. Rows written before `outcome` existed *all*
    /// default to `Finished`, events among them. So the kind of thing decides
    /// first and the stored outcome only breaks the tie.
    pub fn was_finished(&self) -> bool {
        !self.is_event && !self.is_routine() && self.outcome == Outcome::Finished
    }

    /// The mark the ledger puts in front of the name: ✓ for work that was
    /// done, ✗ for everything that merely left.
    pub fn mark(&self) -> &'static str {
        if self.was_finished() {
            Outcome::Finished.glyph()
        } else {
            Outcome::Dropped.glyph()
        }
    }

    /// True when this row counts towards the "deadlines met" figure: a finished
    /// task that actually carried a date. A dropped task broke no promise it
    /// had made — it was withdrawn — and an event never made one.
    pub fn was_judged(&self) -> bool {
        self.was_finished() && self.deadline.is_some()
    }

    /// The one line the ledger prints under the name: what happened, what it
    /// cost, and how long it had been hanging around.
    pub fn verdict(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        if let Some(rule) = self.recurrence {
            // A routine was never work, so there is nothing to be early or late
            // about and no time to hold against an estimate. What it *was* is
            // the standing arrangement itself, so that is what the row reports.
            parts.push("routine dropped".to_string());
            parts.push(format!(
                "{} at {} for {}",
                rule.summary(),
                format_minutes(rule.start_minutes),
                format_duration(rule.minutes)
            ));
        } else if self.is_event {
            // An event is never "finished" — it is a time that arrives and
            // passes on its own, which is why the planner offers no ✓ on one
            // (§16.3). So the only thing to report is that it was taken off the
            // calendar, and when it had been set for.
            parts.push("event removed".to_string());
            if let Some(when) = self.deadline {
                parts.push(format!("was set for {}", when.format("%-d %b %H:%M")));
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
        let verb = self.outcome.verb();
        match self.timing() {
            Timing::Undated => verb.to_string(),
            Timing::Met(spare) | Timing::Missed(spare)
                if spare.num_minutes() <= ON_THE_NOSE_MINUTES =>
            {
                format!("{verb} right on the deadline")
            }
            Timing::Met(spare) => format!("{verb} {} early", format_span(spare)),
            Timing::Missed(over) => format!("{verb} {} late", format_span(over)),
        }
    }

    /// What it cost, and what it was said it would cost.
    ///
    /// This is the sentence the whole redesign exists to be able to write: the
    /// estimate is a promise made before the work, the sessions are the time
    /// actually set aside for it, and putting the two next to each other is
    /// something only an archive that kept both can do.
    fn time_phrase(&self) -> String {
        let booked = self.booked_minutes();
        let spent = match self.sittings() {
            0 => None,
            1 => Some(format!("{} in one sitting", format_duration(booked))),
            sittings => Some(format!("{} over {sittings} sittings", format_duration(booked))),
        };

        match (self.duration_minutes, spent) {
            (Some(estimate), Some(spent)) => {
                format!("{} estimated, {spent}", format_duration(estimate))
            }
            (Some(estimate), None) => {
                format!("{} estimated, never booked", format_duration(estimate))
            }
            (None, Some(spent)) => spent,
            (None, None) => "no time set aside".to_string(),
        }
    }
}

/* ────────────────────────────── Reading it back ─────────────────────────── */

/// A span as one coarse unit — `40m`, `6h`, `12d`, `4mo`.
///
/// Deliberately one unit and rounded down: a ledger is skimmed, and "finished
/// 2d early" is the fact, while "2d 3h 14m early" is the fact buried in noise.
pub fn format_span(span: Duration) -> String {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutcomeFilter {
    #[default]
    Any,
    Finished,
    Dropped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KindFilter {
    #[default]
    Any,
    Tasks,
    Events,
}

impl Filter {
    pub fn admits(&self, row: &Archived) -> bool {
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
        match self.kind {
            KindFilter::Any => {}
            KindFilter::Tasks if row.is_event => return false,
            KindFilter::Events if !row.is_event => return false,
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
        self.query.trim().is_empty()
            && self.outcome == OutcomeFilter::Any
            && self.kind == KindFilter::Any
    }
}

/// What a set of rows adds up to.
///
/// Computable only because the whole log is in memory: the old 15-row window
/// could not have told you any of this.
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
    pub median_lifetime: Option<Duration>,
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
            parts.push(format!(
                "{} event{} removed",
                self.events,
                if self.events == 1 { "" } else { "s" }
            ));
        }
        if self.routines > 0 {
            parts.push(format!(
                "{} routine{} dropped",
                self.routines,
                if self.routines == 1 { "" } else { "s" }
            ));
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
pub fn summarize<'a>(rows: impl IntoIterator<Item = &'a Archived>) -> Summary {
    let mut summary = Summary::default();
    let mut lifetimes: Vec<Duration> = Vec::new();

    for row in rows {
        if row.is_routine() {
            summary.routines += 1;
        } else if row.is_event {
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

/// Break already-ordered rows into month runs.
///
/// Runs, not buckets: the input is newest-first and stays that way, so a month
/// that somehow appears twice (a hand-edited log) draws two headings rather
/// than silently merging out-of-order rows.
pub fn group_by_month(rows: &[&Archived]) -> Vec<MonthRun> {
    let mut runs: Vec<MonthRun> = Vec::new();

    for (index, row) in rows.iter().enumerate() {
        let month = (row.archived_at.year(), row.archived_at.month());
        let same_as_last = runs.last().is_some_and(|run| {
            let previous = rows[run.rows.end - 1].archived_at;
            (previous.year(), previous.month()) == month
        });

        match runs.last_mut() {
            Some(run) if same_as_last => run.rows.end = index + 1,
            _ => runs.push(MonthRun {
                label: row.archived_at.format("%B %Y").to_string(),
                rows: index..index + 1,
            }),
        }
    }
    runs
}

/* ──────────────────────────────── The store ─────────────────────────────── */

/// The log, and the session's copy of it.
///
/// Loaded lazily — nothing reads the archive until something asks to see it —
/// and then kept, so opening and closing the window is free and the planner can
/// consult it without touching the disk again.
#[derive(Debug, Default)]
pub struct ArchiveLog {
    /// Newest first, which is the order everything reads it in.
    entries: Vec<Archived>,
    /// Lines that would not parse, kept verbatim. They are written back out on
    /// a rewrite: failing to understand a line is not grounds for deleting it.
    unreadable: Vec<String>,
    loaded: bool,
}

impl ArchiveLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Every row, newest first.
    pub fn entries(&self) -> &[Archived] {
        &self.entries
    }

    /// How many lines of the log this build could not read. Surfaced in the
    /// window rather than swallowed, because a number that is not zero is
    /// something the user should be told about their own data.
    pub fn unreadable(&self) -> usize {
        self.unreadable.len()
    }

    /// Read the whole log, once. Later calls are free.
    pub fn load(&mut self, dir: &Path) -> Result<(), Box<dyn Error>> {
        if self.loaded {
            return Ok(());
        }

        let path = dir.join(ARCHIVE_FILE);
        let mut entries: Vec<Archived> = Vec::new();
        let mut unreadable: Vec<String> = Vec::new();

        match File::open(&path) {
            Ok(file) => {
                for line in BufReader::new(file).lines() {
                    let line = line?;
                    if line.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Archived>(&line) {
                        Ok(row) => entries.push(row),
                        Err(_) => unreadable.push(line),
                    }
                }
            }
            // No log yet is what a fresh install looks like, not a failure.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Box::new(error)),
        }

        // Sorted rather than merely reversed: appends happen in time order, so
        // file order is usually already right, but a hand-edited or
        // hand-merged log has no such guarantee and the ledger's month runs
        // depend on the order being real.
        entries.sort_by(|a, b| b.archived_at.cmp(&a.archived_at));

        self.entries = entries;
        self.unreadable = unreadable;
        self.loaded = true;
        Ok(())
    }

    /// File one record. One appended line — the write path that actually runs
    /// often stays O(1).
    pub fn record(&mut self, dir: &Path, row: Archived) -> Result<(), Box<dyn Error>> {
        append(dir, &row)?;
        // Only mirror into memory when memory is authoritative. Pushing onto an
        // unloaded log would have the next `load` read the same row off disk
        // and hold it twice.
        if self.loaded {
            self.entries.insert(0, row);
        }
        Ok(())
    }

    /// Take one row out of the log, returning it.
    ///
    /// Both removals go through here: restoring (the row becomes a task again,
    /// so the archive must stop claiming it is done) and forgetting (the user
    /// wants it gone). The file is rewritten whole, which is O(n) — acceptable
    /// because it happens only when someone deliberately asks, and never in the
    /// path that runs every time an item is ticked off.
    pub fn take(&mut self, dir: &Path, key: ArchiveKey) -> Result<Option<Archived>, Box<dyn Error>> {
        self.load(dir)?;

        let Some(index) = self.entries.iter().position(|row| row.key() == key) else {
            return Ok(None);
        };
        let removed = self.entries.remove(index);

        if let Err(error) = self.rewrite(dir) {
            // The file is unchanged, so memory must be too — otherwise the row
            // is gone from the window and still on disk.
            self.entries.insert(index, removed);
            return Err(error);
        }
        Ok(Some(removed))
    }

    /// Write the log out whole and atomically: temp file in the same directory,
    /// flushed, fsynced, renamed over the original — the same discipline
    /// `tasks::oversafe_activesave` uses for the live set.
    fn rewrite(&self, dir: &Path) -> Result<(), Box<dyn Error>> {
        fs::create_dir_all(dir)?;

        let mut body = String::new();
        // Unreadable lines first, out of the way of the chronological tail that
        // appends land on, and preserved exactly as they were found.
        for line in &self.unreadable {
            body.push_str(line);
            body.push('\n');
        }
        for row in self.entries.iter().rev() {
            body.push_str(&serde_json::to_string(row)?);
            body.push('\n');
        }

        let mut temp = NamedTempFile::new_in(dir)?;
        {
            let mut writer = BufWriter::new(&mut temp);
            writer.write_all(body.as_bytes())?;
            writer.flush()?;
        }
        temp.as_file_mut().sync_all()?;
        temp.persist(dir.join(ARCHIVE_FILE))?;
        Ok(())
    }
}

fn append(dir: &Path, row: &Archived) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(dir)?;

    // Serialized before the file is opened, so a value that cannot be written
    // never leaves a half-line in the log.
    let mut json = serde_json::to_string(row)?;
    json.push('\n');

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(ARCHIVE_FILE))?;
    file.write_all(json.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(year, month, day, hour, minute, 0).unwrap()
    }

    fn task(name: &str) -> Active {
        Active {
            id: 1,
            importance: Some(2),
            time_importance: None,
            name: name.to_string(),
            created: at(2026, 8, 1, 9, 0),
            deadline: None,
            is_event: false,
            sessions: Vec::new(),
            planned_start: None,
            duration_minutes: None,
            recurrence: None,
        }
    }

    fn archived(name: &str, archived_at: DateTime<Local>) -> Archived {
        Archived::retire(task(name), Outcome::Finished, archived_at)
    }

    #[test]
    fn retiring_and_restoring_lose_nothing() {
        let original = Active {
            deadline: Some(at(2026, 8, 20, 17, 0)),
            time_importance: Some(1),
            duration_minutes: Some(120),
            sessions: vec![
                Session { start: at(2026, 8, 18, 10, 0), minutes: 60 },
                Session { start: at(2026, 8, 19, 10, 0), minutes: 60 },
            ],
            ..task("physics homework")
        };

        let round_tripped = Archived::retire(original.clone(), Outcome::Finished, at(2026, 8, 19, 11, 0))
            .to_active();

        // The whole point of the new record: nothing the old one dropped —
        // sessions, the estimate, the horizon — is lost on the way through.
        assert_eq!(round_tripped.id, original.id);
        assert_eq!(round_tripped.name, original.name);
        assert_eq!(round_tripped.created, original.created);
        assert_eq!(round_tripped.deadline, original.deadline);
        assert_eq!(round_tripped.importance, original.importance);
        assert_eq!(round_tripped.time_importance, original.time_importance);
        assert_eq!(round_tripped.sessions, original.sessions);
        assert_eq!(round_tripped.duration_minutes, original.duration_minutes);
        assert_eq!(round_tripped.is_event, original.is_event);
    }

    #[test]
    fn a_restored_undated_task_is_not_scored_as_corrupt() {
        // The old record dropped `time_importance`, so an undated task came
        // back with no deadline, no importance and no horizon — the exact shape
        // `Active::base_score` treats as malformed and shoves to the top of the
        // list.
        let undated = Active { importance: None, time_importance: Some(2), ..task("book a dentist") };
        let restored = Archived::retire(undated, Outcome::Finished, at(2026, 8, 10, 12, 0)).to_active();

        let now = at(2026, 8, 11, 12, 0);
        assert!(restored.importance_score(now, 0) < 1000.0, "restored task scored as malformed");
    }

    #[test]
    fn legacy_rows_load_and_mean_what_they_meant() {
        // Exactly the shape the old `save_inactive` wrote: no outcome, no
        // sessions, no horizon, and `inactivated` as the timestamp's name.
        let line = r#"{"id":97,"importance":2,"name":"old row","created":"2026-08-15T20:28:41.882008+03:00","deadline":"2026-08-29T15:45:00+03:00","is_event":false,"inactivated":"2026-08-15T21:35:10.023652+03:00"}"#;

        let row: Archived = serde_json::from_str(line).expect("legacy row must still load");
        assert_eq!(row.name, "old row");
        assert_eq!(row.id, 97);
        // Only the complete path ever wrote a row, so an absent outcome is a
        // completion — not an unknown.
        assert_eq!(row.outcome, Outcome::Finished);
        assert!(row.sessions.is_empty());
        assert_eq!(row.time_importance, None);
    }

    #[test]
    fn the_timestamp_keeps_its_name_on_disk() {
        let json = serde_json::to_string(&archived("a thing", at(2026, 8, 16, 9, 0))).unwrap();
        // A build that writes `archived_at` produces a log the previous build
        // cannot read at all, which is not a trade worth making for a nicer
        // field name.
        assert!(json.contains("\"inactivated\""), "wire name changed: {json}");
        assert!(!json.contains("archived_at"), "wire name changed: {json}");
    }

    #[test]
    fn timing_reads_the_deadline_against_the_finish() {
        let dated = |finished_at| Archived {
            deadline: Some(at(2026, 8, 20, 17, 0)),
            ..Archived::retire(task("report"), Outcome::Finished, finished_at)
        };

        assert!(matches!(dated(at(2026, 8, 18, 17, 0)).timing(), Timing::Met(_)));
        assert!(matches!(dated(at(2026, 8, 22, 17, 0)).timing(), Timing::Missed(_)));
        // On the deadline exactly counts as met — the promise was to be done by
        // then, and being done at then is being done by then.
        assert!(matches!(dated(at(2026, 8, 20, 17, 0)).timing(), Timing::Met(_)));
        // Undated work cannot be late.
        assert_eq!(archived("no date", at(2026, 8, 20, 9, 0)).timing(), Timing::Undated);
    }

    #[test]
    fn an_event_is_never_late() {
        // An event's deadline is when it happened, not a promise it could
        // break. Reading it as one would have every deleted past appointment
        // report itself as a missed deadline.
        let event = Archived {
            is_event: true,
            deadline: Some(at(2026, 8, 10, 9, 0)),
            ..Archived::retire(task("dentist"), Outcome::Dropped, at(2026, 8, 20, 9, 0))
        };
        assert_eq!(event.timing(), Timing::Undated);
        assert!(!event.was_judged());
        assert!(event.verdict().contains("event removed"), "{}", event.verdict());
    }

    #[test]
    fn a_legacy_event_row_does_not_claim_to_have_been_finished() {
        // Found in a real log: an event archived by a build that had no
        // `outcome` field, so it loads as `Finished` — and a dentist
        // appointment wearing a ✓ reads as though the user had *done* it. The
        // kind of thing decides the mark, not the defaulted outcome.
        let line = r#"{"id":119,"importance":null,"name":"saunas","created":"2026-08-15T22:09:51.933405+03:00","deadline":"2026-08-14T01:30:00+03:00","is_event":true,"inactivated":"2026-08-15T22:10:22.233122+03:00"}"#;
        let row: Archived = serde_json::from_str(line).unwrap();

        assert_eq!(row.outcome, Outcome::Finished, "the stored field is what it is");
        assert!(!row.was_finished(), "but an event is never finished");
        assert_eq!(row.mark(), Outcome::Dropped.glyph());

        // And it counts as neither work done nor work abandoned.
        let summary = summarize([&row]);
        assert_eq!((summary.finished, summary.dropped, summary.events), (0, 0, 1));
        assert!(summary.headline().contains("1 event removed"), "{}", summary.headline());

        // "Finished" must not offer it; "Dropped" must, so the two filters
        // between them still show everything.
        assert!(!Filter { outcome: OutcomeFilter::Finished, ..Filter::default() }.admits(&row));
        assert!(Filter { outcome: OutcomeFilter::Dropped, ..Filter::default() }.admits(&row));
    }

    #[test]
    fn a_dropped_routine_comes_back_as_a_routine() {
        let sleep = Active {
            recurrence: Some(Recurrence {
                days: crate::tasks::EVERY_DAY,
                start_minutes: 23 * 60,
                minutes: 8 * 60,
            }),
            importance: None,
            ..task("sleep")
        };
        let row = Archived::retire(sleep, Outcome::Dropped, at(2026, 8, 20, 9, 0));

        // The rule survives, so putting it back restores the arrangement rather
        // than a nameless task with nothing the scorer can read.
        assert!(row.is_routine());
        let restored = row.to_active();
        assert!(restored.is_routine());
        assert_eq!(restored.recurrence, row.recurrence);
        assert_eq!(restored.importance_score(at(2026, 8, 21, 9, 0), 0), 0.0);

        // It is neither finished work nor a broken promise: it was an
        // arrangement, and the row says what the arrangement was.
        assert!(!row.was_finished());
        assert!(!row.was_judged());
        assert_eq!(row.timing(), Timing::Undated);
        assert_eq!(row.mark(), Outcome::Dropped.glyph());
        let verdict = row.verdict();
        assert!(verdict.contains("routine dropped"), "{verdict}");
        assert!(verdict.contains("every day at 23:00 for 8h"), "{verdict}");

        let summary = summarize([&row]);
        assert_eq!((summary.finished, summary.dropped, summary.events, summary.routines), (0, 0, 0, 1));
        assert!(summary.headline().contains("1 routine dropped"), "{}", summary.headline());
    }

    #[test]
    fn the_verdict_holds_the_estimate_against_the_booking() {
        let row = Archived {
            deadline: Some(at(2026, 8, 20, 17, 0)),
            duration_minutes: Some(120),
            sessions: vec![
                Session { start: at(2026, 8, 18, 10, 0), minutes: 90 },
                Session { start: at(2026, 8, 19, 10, 0), minutes: 90 },
            ],
            ..Archived::retire(task("physics homework"), Outcome::Finished, at(2026, 8, 18, 17, 0))
        };

        let verdict = row.verdict();
        assert!(verdict.contains("finished 2d early"), "{verdict}");
        assert!(verdict.contains("2h estimated, 3h over 2 sittings"), "{verdict}");
        assert!(verdict.contains("on the board"), "{verdict}");
    }

    #[test]
    fn a_deadline_met_to_the_minute_is_neither_early_nor_late() {
        let row = Archived {
            deadline: Some(at(2026, 8, 20, 17, 0)),
            ..Archived::retire(task("report"), Outcome::Finished, at(2026, 8, 20, 16, 50))
        };
        assert!(row.verdict().contains("right on the deadline"), "{}", row.verdict());
    }

    #[test]
    fn an_unbooked_task_says_so() {
        let never = Archived::retire(task("tidy the shed"), Outcome::Dropped, at(2026, 8, 5, 9, 0));
        assert!(never.verdict().contains("no time set aside"), "{}", never.verdict());

        let promised = Archived {
            duration_minutes: Some(45),
            ..Archived::retire(task("tidy the shed"), Outcome::Dropped, at(2026, 8, 5, 9, 0))
        };
        assert!(promised.verdict().contains("never booked"), "{}", promised.verdict());
    }

    #[test]
    fn spans_are_one_coarse_unit() {
        assert_eq!(format_span(Duration::seconds(20)), "a moment");
        assert_eq!(format_span(Duration::minutes(40)), "40m");
        assert_eq!(format_span(Duration::hours(6)), "6h");
        assert_eq!(format_span(Duration::days(12)), "12d");
        assert_eq!(format_span(Duration::days(200)), "6mo");
        // A negative span is a clock that went backwards, not a negative age.
        assert_eq!(format_span(Duration::minutes(-5)), "a moment");
    }

    #[test]
    fn totals_drop_the_minutes_once_they_stop_mattering() {
        assert_eq!(format_total(45), "45m");
        assert_eq!(format_total(120), "2h");
        assert_eq!(format_total(150), "2h 30m");
        assert_eq!(format_total(60 * 52 + 30), "52h");
    }

    #[test]
    fn the_summary_counts_only_promises_that_were_actually_made() {
        let met = Archived {
            deadline: Some(at(2026, 8, 20, 17, 0)),
            sessions: vec![Session { start: at(2026, 8, 18, 10, 0), minutes: 60 }],
            ..Archived::retire(task("met"), Outcome::Finished, at(2026, 8, 19, 9, 0))
        };
        let missed = Archived {
            deadline: Some(at(2026, 8, 20, 17, 0)),
            ..Archived::retire(task("missed"), Outcome::Finished, at(2026, 8, 25, 9, 0))
        };
        // Dropped with a date: withdrawn, not missed. Counting it would make
        // abandoning a task look like failing at it.
        let dropped = Archived {
            deadline: Some(at(2026, 8, 20, 17, 0)),
            ..Archived::retire(task("dropped"), Outcome::Dropped, at(2026, 8, 21, 9, 0))
        };
        // Undated: no promise to keep.
        let undated = archived("undated", at(2026, 8, 21, 9, 0));

        let rows = [&met, &missed, &dropped, &undated];
        let summary = summarize(rows.iter().copied());

        assert_eq!(summary.finished, 3);
        assert_eq!(summary.dropped, 1);
        assert_eq!(summary.judged, 2, "only finished, dated tasks are judged");
        assert_eq!(summary.met, 1);
        assert_eq!(summary.booked_minutes, 60);
        assert!(summary.median_lifetime.is_some());
        assert!(summary.headline().contains("1 of 2 deadlines met"), "{}", summary.headline());
    }

    #[test]
    fn an_empty_summary_says_nothing_rather_than_zeroes() {
        assert_eq!(summarize([]).headline(), "nothing here yet");
    }

    #[test]
    fn filters_narrow_independently() {
        let finished_task = archived("write the report", at(2026, 8, 20, 9, 0));
        let dropped_event = Archived {
            is_event: true,
            ..Archived::retire(task("dentist"), Outcome::Dropped, at(2026, 8, 20, 9, 0))
        };

        let mut filter = Filter::default();
        assert!(filter.is_open());
        assert!(filter.admits(&finished_task) && filter.admits(&dropped_event));

        filter.outcome = OutcomeFilter::Finished;
        assert!(filter.admits(&finished_task) && !filter.admits(&dropped_event));

        filter = Filter { kind: KindFilter::Events, ..Filter::default() };
        assert!(!filter.admits(&finished_task) && filter.admits(&dropped_event));

        // Case-insensitive substring, so searching is typing part of the name.
        filter = Filter { query: "  REPORT ".to_string(), ..Filter::default() };
        assert!(filter.admits(&finished_task) && !filter.admits(&dropped_event));
        assert!(!filter.is_open());
    }

    #[test]
    fn months_become_runs_in_the_order_given() {
        let rows_owned = vec![
            archived("c", at(2026, 8, 20, 9, 0)),
            archived("b", at(2026, 8, 2, 9, 0)),
            archived("a", at(2026, 7, 30, 9, 0)),
        ];
        let rows: Vec<&Archived> = rows_owned.iter().collect();

        let runs = group_by_month(&rows);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].rows, 0..2);
        assert_eq!(runs[1].rows, 2..3);
        assert!(runs[0].label.starts_with("August"), "{}", runs[0].label);
        assert!(runs[1].label.starts_with("July"), "{}", runs[1].label);
    }

    #[test]
    fn grouping_nothing_produces_nothing() {
        assert!(group_by_month(&[]).is_empty());
    }

    #[test]
    fn the_log_round_trips_through_a_file_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = ArchiveLog::new();

        // A log that does not exist yet is an empty one, not an error.
        log.load(dir.path()).unwrap();
        assert!(log.entries().is_empty());

        log.record(dir.path(), archived("first", at(2026, 8, 1, 9, 0))).unwrap();
        log.record(dir.path(), archived("second", at(2026, 8, 2, 9, 0))).unwrap();

        let mut reread = ArchiveLog::new();
        reread.load(dir.path()).unwrap();
        let names: Vec<&str> = reread.entries().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["second", "first"], "the ledger reads backwards from now");
    }

    #[test]
    fn recording_into_an_unloaded_log_does_not_double_the_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = ArchiveLog::new();

        // Ticking something off before the archive has ever been opened: the
        // file is authoritative, memory is not yet.
        log.record(dir.path(), archived("only once", at(2026, 8, 1, 9, 0))).unwrap();
        assert!(!log.is_loaded());

        log.load(dir.path()).unwrap();
        assert_eq!(log.entries().len(), 1);
    }

    #[test]
    fn taking_a_row_out_rewrites_the_file_without_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = ArchiveLog::new();
        log.load(dir.path()).unwrap();

        let keeper = archived("keep me", at(2026, 8, 1, 9, 0));
        let goner = archived("take me back", at(2026, 8, 2, 9, 0));
        let key = goner.key();
        log.record(dir.path(), keeper).unwrap();
        log.record(dir.path(), goner).unwrap();

        let taken = log.take(dir.path(), key).unwrap().expect("row should be found");
        assert_eq!(taken.name, "take me back");
        assert_eq!(log.entries().len(), 1);

        // Gone from disk too, not just from the window — otherwise a restored
        // task would come back again on the next launch.
        let mut reread = ArchiveLog::new();
        reread.load(dir.path()).unwrap();
        assert_eq!(reread.entries().len(), 1);
        assert_eq!(reread.entries()[0].name, "keep me");

        // Taking the same row twice is a no-op, not an error.
        assert!(log.take(dir.path(), key).unwrap().is_none());
    }

    #[test]
    fn unreadable_lines_are_counted_and_never_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ARCHIVE_FILE);
        let good = serde_json::to_string(&archived("readable", at(2026, 8, 2, 9, 0))).unwrap();
        let doomed = archived("doomed", at(2026, 8, 3, 9, 0));
        let key = doomed.key();
        let doomed_line = serde_json::to_string(&doomed).unwrap();
        fs::write(&path, format!("{{ not json at all\n{good}\n\n{doomed_line}\n")).unwrap();

        let mut log = ArchiveLog::new();
        log.load(dir.path()).unwrap();
        assert_eq!(log.entries().len(), 2, "blank lines are skipped, bad ones set aside");
        assert_eq!(log.unreadable(), 1);

        // A rewrite must not quietly take the opportunity to delete what it
        // failed to parse.
        log.take(dir.path(), key).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("{ not json at all"), "unreadable line was dropped:\n{written}");

        let mut reread = ArchiveLog::new();
        reread.load(dir.path()).unwrap();
        assert_eq!(reread.entries().len(), 1);
        assert_eq!(reread.unreadable(), 1);
    }

    #[test]
    fn a_log_out_of_order_on_disk_still_reads_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ARCHIVE_FILE);
        let lines: Vec<String> = [
            archived("middle", at(2026, 8, 10, 9, 0)),
            archived("newest", at(2026, 8, 20, 9, 0)),
            archived("oldest", at(2026, 8, 1, 9, 0)),
        ]
        .iter()
        .map(|row| serde_json::to_string(row).unwrap())
        .collect();
        fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();

        let mut log = ArchiveLog::new();
        log.load(dir.path()).unwrap();
        let names: Vec<&str> = log.entries().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["newest", "middle", "oldest"]);
    }
}
