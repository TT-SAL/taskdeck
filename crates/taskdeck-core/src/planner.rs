//! The day planner's model, copied from today's `src/planner.rs` with the
//! zone parameter and sids (core.md §14.2, §20). Its arithmetic in integers
//! (`snap`, `clamp_block`, `snap_up`, `resolve_on_day`, `now_marker`) is in
//! `time` (core.md §5.4, §5.5).
//!
//! Everything here is pure. The parts of a planner that are easy to get
//! subtly wrong (clamping a block inside its day, packing overlapping blocks
//! side by side, sliding work past now) live here so they can be unit-tested
//! directly.
//!
//! ## The two kinds of time
//!
//! A planner has to distinguish *when something is due* from *when you will do
//! it*. `Item::deadline` answers the first; `Item::blocks` answers the second
//! — a list, because the answer is not always one block. That is why a task
//! can appear several times in a week: a worked-on block per block, plus a due
//! marker on the day it is owed. Events are simpler — an event's `deadline` is
//! when it happens — so they are planned by moving that.

use crate::board::{Block, Item, Kind, Recurrence};
use crate::time::{self, Instant, Zone};

/// Minutes in a day; the timeline's full extent.
pub const DAY_MINUTES: i32 = 24 * 60;

/// Everything the planner does snaps to this grid. Fifteen minutes is fine
/// enough to plan a real day and coarse enough that a hand-held drag lands where
/// the user meant.
pub const SNAP_MINUTES: i32 = 15;

/// Shortest block the user can make. Below this the title is unreadable and the
/// resize handle has nothing to grab.
pub const MIN_BLOCK_MINUTES: u32 = 15;

/// Length given to a block created by a click (rather than a drag) — dropping a
/// task from the backlog, or a click that didn't travel far enough to be a drag.
pub const DEFAULT_BLOCK_MINUTES: u32 = 30;

/// Where an item sits on the timeline for a given day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Occupies a span: a planned block, drawn as a filled rectangle.
    /// `start` is minutes from midnight; `minutes` is its length.
    Block { start: i32, minutes: u32 },
    /// A point in time, drawn as a thin pill: an event with no length yet
    /// (`due == false`), or a task's due time (`due == true`).
    Marker { at: i32, due: bool },
}

impl Placement {
    /// First minute of the day this placement touches.
    pub fn start(&self) -> i32 {
        match *self {
            Self::Block { start, .. } => start,
            Self::Marker { at, .. } => at,
        }
    }

    /// One past the last minute it touches. A marker is given a nominal length
    /// so overlap packing can treat both kinds uniformly.
    pub fn end(&self) -> i32 {
        match *self {
            Self::Block { start, minutes } => start + minutes as i32,
            Self::Marker { at, .. } => at + SNAP_MINUTES,
        }
    }
}

/// Minutes from midnight, if `at` falls on `day` in `zone` (core.md §5.3,
/// CORE-TIME-002).
fn minutes_into_day(at: Instant, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> Option<i32> {
    time::now_marker(day, at, zone)
}

/// One thing an item puts on a day's timeline, with enough identity for the
/// UI's gestures to say *which* thing they grabbed.
///
/// `session` is the `sid` of the block when the placement is a planned work
/// block; `None` for an event's block, a routine's block and due markers,
/// which are not stored blocks (core.md §14.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayPlacement {
    pub session: Option<u64>,
    pub placement: Placement,
}

/// Everything `item` puts on `day`'s timeline — possibly nothing, possibly
/// several things.
///
/// A task legitimately shows up more than once: once per block that lands on
/// this day (two hours of homework split across the morning and the evening is
/// two blocks), plus a due marker on the day it is owed. The marker is shown
/// even when a block shares its day — "worked on Friday morning, due Friday
/// 17:00" is exactly the day where seeing the deadline next to the work
/// matters most.
pub fn placements_for(item: &Item, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> Vec<DayPlacement> {
    placements_of(Placeable::of(item), day, zone)
}

/// Everything about an item that decides where it lands on a day.
///
/// A struct rather than five positional arguments, so that something that is
/// not an `Item` can be placed by the same rule; taking the parts *named*
/// keeps the call sites from reading `(false, None, None, …)`.
#[derive(Debug, Clone, Copy)]
pub struct Placeable<'a> {
    pub is_event: bool,
    pub deadline: Option<Instant>,
    pub duration_minutes: Option<u32>,
    pub blocks: &'a [Block],
    pub recurrence: Option<&'a Recurrence>,
}

impl<'a> Placeable<'a> {
    /// The placeable view of an item; its `kind` decides (core.md §6.2).
    pub fn of(item: &'a Item) -> Self {
        Self {
            is_event: item.kind == Kind::Event,
            deadline: item.deadline,
            duration_minutes: item.duration_minutes,
            blocks: &item.blocks,
            recurrence: item.recurrence.as_ref().filter(|_| item.kind == Kind::Routine),
        }
    }
}

/// The same, from the parts rather than from an `Item`. Blocks come in the
/// order they are stored, ascending by sid (core.md §6.3, §14.2).
pub fn placements_of(item: Placeable, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> Vec<DayPlacement> {
    let Placeable { is_event, deadline, duration_minutes, blocks, recurrence } = item;
    let mut placements = Vec::new();

    // A routine is checked first and answers alone. It is a rule, so it has no
    // deadline and no blocks to consider — and if a legacy routine carries
    // some anyway, the rule is what the user set here and it wins rather than
    // the item appearing twice on its own day.
    if let Some(rule) = recurrence {
        if rule.falls_on(day) {
            placements.push(DayPlacement {
                session: None,
                placement: Placement::Block {
                    start: rule.start_minutes.clamp(0, DAY_MINUTES - 1),
                    minutes: rule.minutes.max(MIN_BLOCK_MINUTES),
                },
            });
        }
        return placements;
    }

    if is_event {
        if let Some(at) = deadline.and_then(|deadline| minutes_into_day(deadline, day, zone)) {
            placements.push(DayPlacement {
                session: None,
                placement: match duration_minutes {
                    Some(minutes) => Placement::Block { start: at, minutes: minutes.max(MIN_BLOCK_MINUTES) },
                    None => Placement::Marker { at, due: false },
                },
            });
        }
        return placements;
    }

    for block in blocks {
        if let Some(start) = minutes_into_day(block.start, day, zone) {
            placements.push(DayPlacement {
                session: Some(block.sid),
                placement: Placement::Block { start, minutes: block.minutes.max(MIN_BLOCK_MINUTES) },
            });
        }
    }

    if let Some(due) = deadline.and_then(|deadline| minutes_into_day(deadline, day, zone)) {
        placements.push(DayPlacement { session: None, placement: Placement::Marker { at: due, due: true } });
    }

    placements
}

/// Whether `item` puts anything at all on `day`'s timeline.
///
/// The same question `placements_for` answers in detail, without building the
/// list — used to decide whether a tray card has a block to host its title
/// editor or has to host it itself.
pub fn appears_on(item: &Item, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> bool {
    if item.kind == Kind::Routine {
        return item.recurrence.is_some_and(|rule| rule.falls_on(day));
    }
    let falls_on = |at: Instant| time::local_day(at, zone) == day;
    if item.kind == Kind::Event {
        return item.deadline.is_some_and(falls_on);
    }
    item.blocks.iter().any(|block| falls_on(block.start)) || item.deadline.is_some_and(falls_on)
}

/// Format minutes past midnight as `HH:MM`.
pub fn format_minutes(minutes: i32) -> String {
    let minutes = minutes.clamp(0, DAY_MINUTES);
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// Format a length as `1h 30m` / `45m` / `2h`.
pub fn format_duration(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// Column assignment for one laid-out placement: which lane it draws in, and how
/// many lanes its cluster of overlapping neighbours needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lane {
    pub column: usize,
    pub columns: usize,
}

/// Lay overlapping placements out side by side.
///
/// Placements are grouped into *clusters* — runs that transitively overlap — and
/// within a cluster each is given the first column free at its start time. Every
/// member of a cluster reports the same `columns`, so they divide the timeline's
/// width evenly and line up, the way a week view does.
///
/// The returned vector is parallel to `placements`; input order is preserved so
/// the caller can zip it back against its own items.
pub fn lay_out(placements: &[Placement]) -> Vec<Lane> {
    let mut order: Vec<usize> = (0..placements.len()).collect();
    // Earliest first; longer first on a tie, so the big block takes column 0 and
    // the short ones stack to its right.
    order.sort_by_key(|&i| (placements[i].start(), -(placements[i].end())));

    let mut lanes = vec![Lane { column: 0, columns: 1 }; placements.len()];

    // One cluster at a time: `column_ends[c]` is when column `c` last became
    // free, and `cluster` collects the indices to backfill `columns` into.
    let mut column_ends: Vec<i32> = Vec::new();
    let mut cluster: Vec<usize> = Vec::new();
    let mut cluster_end = i32::MIN;

    for &i in &order {
        let placement = placements[i];

        // A placement starting at or after every active column's end begins a
        // new cluster: nothing before it can share its rows.
        if placement.start() >= cluster_end && !cluster.is_empty() {
            let columns = column_ends.len().max(1);
            for &member in &cluster {
                lanes[member].columns = columns;
            }
            cluster.clear();
            column_ends.clear();
        }

        let column = column_ends.iter().position(|&end| end <= placement.start()).unwrap_or_else(|| {
            column_ends.push(i32::MIN);
            column_ends.len() - 1
        });
        column_ends[column] = placement.end();

        lanes[i].column = column;
        cluster.push(i);
        cluster_end = cluster_end.max(placement.end());
    }

    let columns = column_ends.len().max(1);
    for &member in &cluster {
        lanes[member].columns = columns;
    }

    lanes
}

/// What the planner header reports about a day.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DaySummary {
    /// Total minutes covered by blocks, counting overlapping time once.
    pub planned_minutes: i32,
    /// How many blocks are on the day.
    pub blocks: usize,
    /// How many due markers are on the day.
    pub due: usize,
}

/// Summarize a day's placements. Overlapping blocks are unioned rather than
/// summed, so a double-booked hour is reported as one hour of the day spent, not
/// two — "how much of my day is committed" is the useful number.
pub fn summarize(placements: &[Placement]) -> DaySummary {
    let mut spans: Vec<(i32, i32)> = placements
        .iter()
        .filter_map(|p| match *p {
            Placement::Block { start, minutes } => Some((start, start + minutes as i32)),
            Placement::Marker { .. } => None,
        })
        .collect();
    spans.sort_unstable();

    let mut planned_minutes = 0;
    let mut merged: Option<(i32, i32)> = None;
    for (start, end) in spans {
        match merged {
            Some((_, current_end)) if start <= current_end => {
                merged = merged.map(|(s, e)| (s, e.max(end)));
            }
            Some((s, e)) => {
                planned_minutes += e - s;
                merged = Some((start, end));
            }
            None => merged = Some((start, end)),
        }
    }
    if let Some((s, e)) = merged {
        planned_minutes += e - s;
    }

    DaySummary {
        planned_minutes,
        blocks: placements.iter().filter(|p| matches!(p, Placement::Block { .. })).count(),
        due: placements.iter().filter(|p| matches!(p, Placement::Marker { due: true, .. })).count(),
    }
}

/// The masthead's line for a day — `2h 30m planned · 3 blocks · 1 due · 9h
/// routine · 2h done` — each part only when it has something to say.
///
/// Routines are counted apart from planned work: folding nine hours of sleep
/// and meals into "planned" would make every day read as full, and leaving
/// them out makes a day with four free hours claim thirteen. Finished work is
/// counted apart from both, or a finished day would look like a day with
/// everything still ahead of it. One function, because the desktop and the
/// phone both say this and must say it the same way.
pub fn summary_text(planned: DaySummary, routine: DaySummary, done: DaySummary) -> String {
    if planned.blocks == 0 && planned.due == 0 && done.blocks == 0 && routine.blocks == 0 {
        return "nothing on this day".to_string();
    }
    let mut parts = Vec::new();
    if planned.blocks > 0 || (done.blocks == 0 && routine.blocks == 0) {
        parts.push(format!("{} planned", format_duration(planned.planned_minutes.max(0) as u32)));
    }
    if planned.blocks > 0 {
        parts.push(format!("{} block{}", planned.blocks, if planned.blocks == 1 { "" } else { "s" }));
    }
    if planned.due > 0 {
        parts.push(format!("{} due", planned.due));
    }
    if routine.blocks > 0 {
        parts.push(format!("{} routine", format_duration(routine.planned_minutes.max(0) as u32)));
    }
    if done.blocks > 0 {
        parts.push(format!("{} done", format_duration(done.planned_minutes.max(0) as u32)));
    }
    parts.join(" · ")
}

/// How long a block dropped from the tray should run: the task's *unplanned
/// remainder* when it has an estimate, else the default.
///
/// This is what makes an estimate worth setting, twice over. "Physics homework
/// takes two hours" lands a two-hour block rather than a default half-hour to
/// be stretched by hand — and once an hour of it is booked, the next drop
/// lands the remaining hour, so planning a task across days is just dragging
/// the same card until it runs out.
pub fn drop_length_for(item: &Item) -> u32 {
    item.remaining_minutes().filter(|left| *left > 0).unwrap_or(DEFAULT_BLOCK_MINUTES).max(MIN_BLOCK_MINUTES)
}

/// The lengths the duration picker offers. Quarter-hours while the numbers are
/// small, then coarser: nobody plans a five-hour block to the nearest fifteen
/// minutes, and a list you have to scroll is worse than one that rounds.
const DURATION_CHOICES: [u32; 14] = [15, 30, 45, 60, 90, 120, 150, 180, 240, 300, 360, 420, 480, 600];

/// Durations to offer for an item currently `minutes` long.
///
/// `current` is folded into the list so a length dragged out by hand — 1h 05m,
/// say — is still shown as the selection rather than silently reading as the
/// nearest preset, and picking something else and coming back doesn't quietly
/// round it.
pub fn duration_options(current: Option<u32>) -> Vec<u32> {
    let mut options: Vec<u32> = DURATION_CHOICES.to_vec();
    if let Some(current) = current {
        let current = current.max(MIN_BLOCK_MINUTES);
        if !options.contains(&current) {
            options.push(current);
            options.sort_unstable();
        }
    }
    options
}

/// Which tray section an unplanned task belongs to on the day being planned.
///
/// A task owed today with no time set aside for it is the single most useful
/// thing a day planner can point at, so it gets its own group at the top of
/// the tray rather than being ranked in amongst everything else by score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacklogGroup {
    /// Owed on or before the day being planned — wants a slot on it.
    Due,
    /// Owed later, or not owed at all.
    Later,
}

/// Group an unplanned task by its deadline's local day relative to `day`
/// (core.md §14.2: `Due` when `D(deadline) ≤ day`).
///
/// "On or before" rather than "on": something that was due on Wednesday and is
/// still unplanned belongs at the top of Friday's tray too. Planning is about
/// what still needs doing, not about which day the deadline happened to fall on.
pub fn backlog_group(deadline: Option<Instant>, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> BacklogGroup {
    match deadline {
        Some(deadline) if time::local_day(deadline, zone) <= day => BacklogGroup::Due,
        _ => BacklogGroup::Later,
    }
}

/// A human "in 2h 10m" / "3d" hint for a task's deadline, used on backlog cards
/// so the tray conveys urgency without opening anything. A difference of two
/// instants, so no zone.
pub fn relative_due(deadline: Instant, now: Instant) -> String {
    let delta = time::between(now, deadline);
    if delta.num_minutes() < 0 {
        return "overdue".to_string();
    }
    let days = delta.num_days();
    if days >= 1 {
        return format!("{days}d");
    }
    let hours = delta.num_hours();
    if hours >= 1 { format!("{hours}h") } else { format!("{}m", delta.num_minutes().max(0)) }
}

/* ─────────────────────────────── Reflow ───────────────────────────────
 *
 * A plan made of clock times is over-specified: dropping a block at 14:00
 * asserts both "an hour on this today" (usually known) and "that hour is
 * 14:00" (almost never known), and it is the second claim that breaks and
 * takes everything below it with it. The cheap answer is a button: when work
 * is booked before the now-line and not ticked off, say how far behind the day
 * is, and offer to slide what is left down past now — in order, around the
 * things that are genuinely fixed.
 */

/// One work block reflow may move. `key` is whatever the caller needs to find
/// the block again — `(item id, sid)` in practice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flowable<K> {
    pub key: K,
    pub start: i32,
    pub minutes: u32,
}

/// Minutes of booked work already gone by: the part of each block that lies
/// before `now`. This is the masthead's "1h 20m behind" — it answers "how much
/// of what I planned is behind me and not ticked off" — and is zero while
/// everything is still ahead. Pass only work blocks: eight hours of sleep that
/// have passed are not eight hours behind.
///
/// Overlapping time counts once, as `summarize` counts it: two blocks over
/// the same hour are one hour behind, and the figure never exceeds the
/// "planned" figure it sits beside.
pub fn behind_minutes(blocks: &[Placement], now: i32) -> i32 {
    let clipped: Vec<Placement> = blocks
        .iter()
        .filter_map(|placement| match *placement {
            Placement::Block { start, minutes } => {
                let end = now.min(start + minutes as i32);
                (end > start).then_some(Placement::Block { start, minutes: (end - start) as u32 })
            }
            Placement::Marker { .. } => None,
        })
        .collect();
    summarize(&clipped).planned_minutes
}

/// Slide the day's work down past `now`, keeping its order, around `anchors`
/// — the `(start, end)` spans that are genuinely fixed: events and routines.
///
/// Returns a new start for every block, in the order the blocks were given;
/// the caller applies the ones that changed. Blocks keep their length and
/// their **order**: this is a slide, not a re-plan — the report you meant to
/// write first is still first, it just starts now. A block still ahead stays
/// where it was put unless one landing in front of it pushes it on, which is
/// the "my schedule exploded" the button exists for; nothing is ever pulled
/// *earlier*, so on a day with nothing behind, nothing moves.
///
/// Nothing is dropped. A day that runs out of room has its last blocks clamped
/// inside it and left overlapping, which is what an over-booked day should
/// look like rather than something to hide.
pub fn reflow<K: Copy>(blocks: &[Flowable<K>], anchors: &[(i32, i32)], now: i32) -> Vec<(K, i32)> {
    let mut order: Vec<usize> = (0..blocks.len()).collect();
    order.sort_by_key(|&index| blocks[index].start);

    let mut anchors: Vec<(i32, i32)> = anchors.iter().copied().filter(|(start, end)| end > start).collect();
    anchors.sort_unstable();

    let mut placed: Vec<Option<(K, i32)>> = vec![None; blocks.len()];
    let mut cursor = time::snap_up(now);
    for &index in &order {
        let length = blocks[index].minutes.max(MIN_BLOCK_MINUTES) as i32;
        // Behind now, or behind the block before it: moved to the cursor.
        // Ahead of both: left where it is.
        let mut start = cursor.max(blocks[index].start);
        // Anchors are sorted by start and `start` only ever grows, so one pass
        // steps past every anchor the block would land in — including one
        // that begins inside the anchor just stepped past.
        for &(anchor_start, anchor_end) in &anchors {
            if start < anchor_end && anchor_start < start + length {
                start = time::snap_up(anchor_end);
            }
        }
        let start = start.min(DAY_MINUTES - length).max(0);
        placed[index] = Some((blocks[index].key, start));
        cursor = start + length;
    }
    placed.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests;
