//! The day planner's model and geometry.
//!
//! Everything here is pure: no egui, no `TaskApp`. The parts of a planner that
//! are easy to get subtly wrong — mapping time to pixels and back, snapping,
//! clamping a block inside its day, and packing overlapping blocks side by side
//! — live here so they can be unit-tested directly. `ui.rs` owns the drawing and
//! the pointer handling and calls into this.
//!
//! ## The two kinds of time
//!
//! A planner has to distinguish *when something is due* from *when you will do
//! it*. `Active::deadline` answers the first; `Active::sessions` answers the
//! second — a list, because the answer is not always one block. That is why a
//! task can appear several times in a week: a worked-on block per session, plus
//! a due marker on the day it is owed. Events are simpler — an event's
//! `deadline` is when it happens — so they are planned by moving that.

use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Timelike};

use crate::tasks::{Active, Session};

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

/// What a drag (or double-click) on empty timeline makes.
///
/// Two kinds, distinguished by which time field the gesture fills in: a `Task`
/// gets a work session (a plan is not a due date, so its deadline is left
/// empty), an `Event` gets its `deadline`, because an event's deadline *is*
/// when it happens.
///
/// There used to be a third kind, `Deadline`, which dropped a due marker with
/// no session behind it. It existed because a deadline could only be *created*,
/// never attached — with the footer's due editor able to put a deadline on any
/// task, a due time is an attribute you set, not a thing you draw, and the mode
/// went away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CreateKind {
    /// Time set aside to work on something. The default: the planner's job.
    #[default]
    Task,
    /// Something that happens at a time.
    Event,
}

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

/// Minutes from midnight, if `at` falls on `day`.
fn minutes_into_day(at: DateTime<Local>, day: NaiveDate) -> Option<i32> {
    (at.date_naive() == day).then(|| at.hour() as i32 * 60 + at.minute() as i32)
}

/// One thing an item puts on a day's timeline, with enough identity for the
/// UI's gestures to say *which* thing they grabbed.
///
/// `session` is the index into the task's `sessions` when the placement is a
/// planned work block; `None` for an event's block and for due markers, which
/// are not sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayPlacement {
    pub session: Option<usize>,
    pub placement: Placement,
}

/// Everything `item` puts on `day`'s timeline — possibly nothing, possibly
/// several things.
///
/// A task legitimately shows up more than once: once per session that lands on
/// this day (two hours of homework split across the morning and the evening is
/// two blocks), plus a due marker on the day it is owed. The marker is shown
/// even when a session shares its day — earlier versions suppressed it, but
/// "worked on Friday morning, due Friday 17:00" is exactly the day where
/// seeing the deadline next to the work matters most.
pub fn placements_for(item: &Active, day: NaiveDate) -> Vec<DayPlacement> {
    placements_of(
        item.is_event,
        item.deadline,
        item.duration_minutes,
        &item.sessions,
        day,
    )
}

/// The same, from the fields rather than from an `Active`.
///
/// An archived item puts blocks on a day too — the planner draws what a past
/// day was actually spent on behind what is still planned for it — and it is
/// not an `Active` any more. Taking the parts keeps one copy of the placement
/// rule instead of a second one drifting alongside it in the archive.
pub fn placements_of(
    is_event: bool,
    deadline: Option<DateTime<Local>>,
    duration_minutes: Option<u32>,
    sessions: &[Session],
    day: NaiveDate,
) -> Vec<DayPlacement> {
    let mut placements = Vec::new();

    if is_event {
        if let Some(at) = deadline.and_then(|deadline| minutes_into_day(deadline, day)) {
            placements.push(DayPlacement {
                session: None,
                placement: match duration_minutes {
                    Some(minutes) => {
                        Placement::Block { start: at, minutes: minutes.max(MIN_BLOCK_MINUTES) }
                    }
                    None => Placement::Marker { at, due: false },
                },
            });
        }
        return placements;
    }

    for (index, session) in sessions.iter().enumerate() {
        if let Some(start) = minutes_into_day(session.start, day) {
            placements.push(DayPlacement {
                session: Some(index),
                placement: Placement::Block {
                    start,
                    minutes: session.minutes.max(MIN_BLOCK_MINUTES),
                },
            });
        }
    }

    if let Some(due) = deadline.and_then(|deadline| minutes_into_day(deadline, day)) {
        placements.push(DayPlacement {
            session: None,
            placement: Placement::Marker { at: due, due: true },
        });
    }

    placements
}

/// Whether `item` puts anything at all on `day`'s timeline.
///
/// The same question `placements_for` answers in detail, without building the
/// list — used to decide whether a tray card has a block to host its title
/// editor or has to host it itself.
pub fn appears_on(item: &Active, day: NaiveDate) -> bool {
    if item.is_event {
        return item.deadline.is_some_and(|at| at.date_naive() == day);
    }
    item.sessions.iter().any(|session| session.start.date_naive() == day)
        || item.deadline.is_some_and(|at| at.date_naive() == day)
}

/// Vertical geometry of the timeline: where midnight sits and how tall an hour
/// is. Kept as data so the mapping is testable without a `Ui`.
#[derive(Debug, Clone, Copy)]
pub struct TimelineGeometry {
    /// Screen y of 00:00.
    pub top: f32,
    /// Pixels per hour.
    pub hour_height: f32,
}

impl TimelineGeometry {
    pub fn new(top: f32, hour_height: f32) -> Self {
        Self { top, hour_height: hour_height.max(1.0) }
    }

    /// Screen y for a number of minutes past midnight.
    pub fn y_for(&self, minutes: f32) -> f32 {
        self.top + minutes / 60.0 * self.hour_height
    }

    /// Minutes past midnight for a screen y. Not clamped — callers that need a
    /// legal time run the result through `snap` or `clamp_block`.
    pub fn minutes_at(&self, y: f32) -> f32 {
        (y - self.top) / self.hour_height * 60.0
    }

    /// Full height of a 24-hour timeline.
    pub fn full_height(&self) -> f32 {
        self.hour_height * 24.0
    }
}

/// Round to the nearest snap step and clamp into the day.
pub fn snap(minutes: f32) -> i32 {
    let step = SNAP_MINUTES as f32;
    let snapped = (minutes / step).round() as i32 * SNAP_MINUTES;
    snapped.clamp(0, DAY_MINUTES)
}

/// Snap and clamp a `(start, length)` pair so the block is at least
/// `MIN_BLOCK_MINUTES` long and lies wholly inside the day. Used by create,
/// move, and resize alike, so all three agree on what a legal block is.
pub fn clamp_block(start: f32, minutes: f32) -> (i32, u32) {
    let length = snap(minutes.max(MIN_BLOCK_MINUTES as f32)).max(MIN_BLOCK_MINUTES as i32);
    // Never let a block run past midnight: pull the start back instead of
    // silently shortening what the user dragged.
    let start = snap(start).clamp(0, DAY_MINUTES - length);
    (start, length as u32)
}

/// Build a `(start, length)` from the two ends of a drag, in either direction.
pub fn block_from_drag(anchor_minutes: f32, cursor_minutes: f32) -> (i32, u32) {
    let (from, to) = if cursor_minutes < anchor_minutes {
        (cursor_minutes, anchor_minutes)
    } else {
        (anchor_minutes, cursor_minutes)
    };
    clamp_block(from, to - from)
}

/// Turn minutes-past-midnight on `day` back into a local timestamp.
///
/// Returns `None` only for a time that does not exist locally — the hour a
/// spring-forward DST jump skips. Callers nudge past it rather than fail: see
/// `resolve_on_day`.
fn local_at(day: NaiveDate, minutes: i32) -> Option<DateTime<Local>> {
    let time = day.and_hms_opt(
        (minutes / 60).clamp(0, 23) as u32,
        (minutes % 60) as u32,
        0,
    )?;
    Local.from_local_datetime(&time).earliest()
}

/// Local timestamp for `minutes` past midnight on `day`, stepping over a DST
/// gap the way `utilities::parse_time_input` does so planning on a
/// spring-forward morning doesn't silently fail.
pub fn resolve_on_day(day: NaiveDate, minutes: i32) -> Option<DateTime<Local>> {
    let minutes = minutes.clamp(0, DAY_MINUTES - 1);
    local_at(day, minutes).or_else(|| local_at(day, (minutes + 60).min(DAY_MINUTES - 1)))
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

/// A pointer gesture in progress on the timeline.
///
/// The gesture lives here, next to the maths that interprets it, so the mapping
/// from "where the pointer is" to "where the block lands" can be tested without
/// a window. `ui.rs` only decides *which* gesture a press begins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drag {
    /// Pulling a new block out of empty timeline. `anchor` is the minute the
    /// press landed on; the block spans from there to the pointer, in either
    /// direction.
    Create { anchor: i32 },
    /// Moving a block: session `session` of task `id`, or an event's block when
    /// `session` is `None`. `grab_offset` is how far into the block the press
    /// landed, so it doesn't jump to centre itself under the pointer.
    Move { id: u64, session: Option<usize>, grab_offset: i32, minutes: u32 },
    /// Dragging a block's bottom edge; `start` is held fixed. Same addressing
    /// as [`Drag::Move`].
    Resize { id: u64, session: Option<usize>, start: i32 },
    /// Dragging a card out of the tray. On release this *adds* a session — the
    /// task may already have others. Nothing is committed until release.
    FromBacklog { id: u64 },
}

impl Drag {
    /// The existing block this gesture is moving or resizing, if it is one —
    /// `(task id, session index)`. `Create` and `FromBacklog` make a *new*
    /// block, so they address nothing yet.
    pub fn target(&self) -> Option<(u64, Option<usize>)> {
        match *self {
            Self::Move { id, session, .. } | Self::Resize { id, session, .. } => {
                Some((id, session))
            }
            Self::Create { .. } | Self::FromBacklog { .. } => None,
        }
    }
}

/// Where `drag` currently puts its block, as `(start, minutes)`. Which block
/// that is, the drag itself says (`Drag::target`, or a new pending block).
///
/// `default_minutes` is the length to use for a tray card being dropped — the
/// caller resolves it from the item, since the gesture itself doesn't carry
/// one. Both the live preview and the commit-on-release go through this, so
/// what the user sees while dragging is exactly what gets saved.
pub fn preview(drag: &Drag, minutes_at_pointer: f32, default_minutes: u32) -> (i32, u32) {
    match *drag {
        Drag::Create { anchor } => block_from_drag(anchor as f32, minutes_at_pointer),
        Drag::Move { grab_offset, minutes, .. } => {
            clamp_block(minutes_at_pointer - grab_offset as f32, minutes as f32)
        }
        Drag::Resize { start, .. } => {
            clamp_block(start as f32, minutes_at_pointer - start as f32)
        }
        Drag::FromBacklog { .. } => clamp_block(minutes_at_pointer, default_minutes as f32),
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

        let column = column_ends
            .iter()
            .position(|&end| end <= placement.start())
            .unwrap_or_else(|| {
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
        blocks: placements
            .iter()
            .filter(|p| matches!(p, Placement::Block { .. }))
            .count(),
        due: placements
            .iter()
            .filter(|p| matches!(p, Placement::Marker { due: true, .. }))
            .count(),
    }
}

/// Minutes past midnight for "right now", or `None` when `day` isn't today —
/// the caller draws the now-line only when there is one to draw.
pub fn now_marker(day: NaiveDate, now: DateTime<Local>) -> Option<i32> {
    minutes_into_day(now, day)
}

/// How long a block dropped from the tray should run: the task's *unplanned
/// remainder* when it has an estimate, else the default.
///
/// This is what makes an estimate worth setting, twice over. "Physics homework
/// takes two hours" lands a two-hour block rather than a default half-hour to
/// be stretched by hand — and once an hour of it is booked, the next drop
/// lands the remaining hour, so planning a task across days is just dragging
/// the same card until it runs out.
pub fn drop_length_for(item: &Active) -> u32 {
    item.remaining_minutes()
        .filter(|left| *left > 0)
        .unwrap_or(DEFAULT_BLOCK_MINUTES)
        .max(MIN_BLOCK_MINUTES)
}

/// The lengths the duration picker offers. Quarter-hours while the numbers are
/// small, then coarser: nobody plans a five-hour block to the nearest fifteen
/// minutes, and a list you have to scroll is worse than one that rounds.
const DURATION_CHOICES: [u32; 14] = [
    15, 30, 45, 60, 90, 120, 150, 180, 240, 300, 360, 420, 480, 600,
];

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
/// This is the seam where the old day popup's question ("what is on this day?")
/// turns into the planner's ("when will I do it?"). A task owed today with no
/// time set aside for it is the single most useful thing a day planner can point
/// at, so it gets its own group at the top of the tray rather than being ranked
/// in amongst everything else by score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacklogGroup {
    /// Owed on or before the day being planned — wants a slot on it.
    Due,
    /// Owed later, or not owed at all.
    Later,
}

/// Group an unplanned task by its deadline relative to `day`.
///
/// "On or before" rather than "on": something that was due on Wednesday and is
/// still unplanned belongs at the top of Friday's tray too. Planning is about
/// what still needs doing, not about which day the deadline happened to fall on.
pub fn backlog_group(deadline: Option<DateTime<Local>>, day: NaiveDate) -> BacklogGroup {
    match deadline {
        Some(deadline) if deadline.date_naive() <= day => BacklogGroup::Due,
        _ => BacklogGroup::Later,
    }
}

/// A human "in 2h 10m" / "3d" hint for a task's deadline, used on backlog cards
/// so the tray conveys urgency without opening anything.
pub fn relative_due(deadline: DateTime<Local>, now: DateTime<Local>) -> String {
    let delta: Duration = deadline - now;
    if delta.num_minutes() < 0 {
        return "overdue".to_string();
    }
    let days = delta.num_days();
    if days >= 1 {
        return format!("{days}d");
    }
    let hours = delta.num_hours();
    if hours >= 1 {
        format!("{hours}h")
    } else {
        format!("{}m", delta.num_minutes().max(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::Session;
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, hour, minute, 0).unwrap()
    }

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, 14).unwrap()
    }

    /// A task with `sessions` planned blocks, a `deadline`, and an estimate.
    fn task(sessions: Vec<Session>, deadline: Option<DateTime<Local>>, minutes: Option<u32>) -> Active {
        Active {
            id: 1,
            importance: Some(2),
            time_importance: None,
            name: "t".into(),
            created: at(2026, 8, 1, 9, 0),
            deadline,
            is_event: false,
            sessions,
            planned_start: None,
            duration_minutes: minutes,
        }
    }

    fn session(start: DateTime<Local>, minutes: u32) -> Session {
        Session { start, minutes }
    }

    fn event(deadline: DateTime<Local>, minutes: Option<u32>) -> Active {
        Active {
            id: 2,
            importance: None,
            time_importance: None,
            name: "e".into(),
            created: at(2026, 8, 1, 9, 0),
            deadline: Some(deadline),
            is_event: true,
            sessions: Vec::new(),
            planned_start: None,
            duration_minutes: minutes,
        }
    }

    #[test]
    fn geometry_round_trips_minutes_through_pixels() {
        let geom = TimelineGeometry::new(100.0, 60.0);
        assert_eq!(geom.y_for(0.0), 100.0);
        assert_eq!(geom.y_for(60.0), 160.0);
        assert_eq!(geom.minutes_at(160.0), 60.0);
        // Round-trip at an arbitrary point.
        let minutes = geom.minutes_at(geom.y_for(487.0));
        assert!((minutes - 487.0).abs() < 0.001, "got {minutes}");
        assert_eq!(geom.full_height(), 1440.0);
    }

    #[test]
    fn snap_rounds_to_the_quarter_hour_and_stays_in_the_day() {
        assert_eq!(snap(0.0), 0);
        assert_eq!(snap(7.0), 0);
        assert_eq!(snap(8.0), 15);
        assert_eq!(snap(22.0), 15);
        assert_eq!(snap(23.0), 30);
        // Out-of-range drags clamp rather than producing an impossible time.
        assert_eq!(snap(-500.0), 0);
        assert_eq!(snap(5000.0), DAY_MINUTES);
    }

    #[test]
    fn clamp_block_enforces_a_minimum_and_keeps_blocks_inside_the_day() {
        // A too-short drag becomes the minimum length.
        assert_eq!(clamp_block(600.0, 1.0), (600, MIN_BLOCK_MINUTES));
        // A normal drag snaps both ends independently, each to its nearest step:
        // 604 -> 600 (4 below vs 11 above), 52 -> 45 (7 vs 8).
        assert_eq!(clamp_block(604.0, 52.0), (600, 45));
        // And rounds up when that is nearer: 608 is 8 past 600 but only 7 short
        // of 615.
        assert_eq!(clamp_block(608.0, 52.0), (615, 45));
        // A block that would run past midnight is pulled back, not truncated.
        let (start, minutes) = clamp_block(1430.0, 120.0);
        assert_eq!(minutes, 120);
        assert_eq!(start, DAY_MINUTES - 120);
        assert!(start + minutes as i32 <= DAY_MINUTES);
    }

    #[test]
    fn block_from_drag_works_in_both_directions() {
        let downward = block_from_drag(540.0, 630.0);
        let upward = block_from_drag(630.0, 540.0);
        assert_eq!(downward, (540, 90));
        assert_eq!(downward, upward, "dragging up should build the same block");
    }

    #[test]
    fn placements_distinguish_planned_work_from_a_due_date() {
        let planned_day = day();
        let due_day = NaiveDate::from_ymd_opt(2026, 8, 16).unwrap();

        // Worked on Friday 09:00 for an hour, due Sunday 17:00.
        let item = task(
            vec![session(at(2026, 8, 14, 9, 0), 60)],
            Some(at(2026, 8, 16, 17, 0)),
            Some(60),
        );

        assert_eq!(
            placements_for(&item, planned_day),
            vec![DayPlacement { session: Some(0), placement: Placement::Block { start: 9 * 60, minutes: 60 } }],
            "the planned day shows the work block"
        );
        assert_eq!(
            placements_for(&item, due_day),
            vec![DayPlacement { session: None, placement: Placement::Marker { at: 17 * 60, due: true } }],
            "the due day still shows the deadline"
        );
        assert_eq!(
            placements_for(&item, NaiveDate::from_ymd_opt(2026, 8, 15).unwrap()),
            vec![],
            "an unrelated day shows nothing"
        );
    }

    #[test]
    fn a_task_split_across_a_day_shows_every_session_and_its_deadline() {
        // Two sessions on the same day the task is due: 09:00–10:00,
        // 14:00–15:30, owed at 17:00. All three appear — the due marker is not
        // suppressed by the work blocks, because seeing the deadline next to
        // the work is the point of showing a day whole.
        let item = task(
            vec![
                session(at(2026, 8, 14, 9, 0), 60),
                session(at(2026, 8, 14, 14, 0), 90),
            ],
            Some(at(2026, 8, 14, 17, 0)),
            Some(150),
        );

        let placements = placements_for(&item, day());
        assert_eq!(placements.len(), 3, "{placements:?}");
        assert_eq!(placements[0].session, Some(0));
        assert_eq!(placements[1].session, Some(1));
        assert_eq!(placements[1].placement, Placement::Block { start: 14 * 60, minutes: 90 });
        assert_eq!(
            placements[2],
            DayPlacement { session: None, placement: Placement::Marker { at: 17 * 60, due: true } }
        );

        // A session on another day stays on that day.
        let elsewhere = NaiveDate::from_ymd_opt(2026, 8, 13).unwrap();
        assert_eq!(placements_for(&item, elsewhere), vec![]);
    }

    #[test]
    fn events_are_blocks_once_they_have_a_length() {
        let without = event(at(2026, 8, 14, 14, 30), None);
        assert_eq!(
            placements_for(&without, day()),
            vec![DayPlacement { session: None, placement: Placement::Marker { at: 14 * 60 + 30, due: false } }]
        );

        let with = event(at(2026, 8, 14, 14, 30), Some(45));
        assert_eq!(
            placements_for(&with, day()),
            vec![DayPlacement { session: None, placement: Placement::Block { start: 14 * 60 + 30, minutes: 45 } }]
        );
    }

    #[test]
    fn appears_on_agrees_with_placements_for() {
        let day = day();
        let elsewhere = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();

        // Every shape, checked against the list it summarises: a bare task, one
        // with only a session here, one with only its deadline here, and an
        // event.
        let cases = [
            task(vec![], None, None),
            task(vec![session(at(2026, 8, 14, 9, 0), 60)], None, Some(60)),
            task(vec![], Some(at(2026, 8, 14, 17, 0)), None),
            task(vec![session(at(2026, 8, 20, 9, 0), 60)], Some(at(2026, 8, 14, 17, 0)), Some(60)),
            event(at(2026, 8, 14, 14, 0), Some(45)),
            event(at(2026, 8, 20, 14, 0), Some(45)),
        ];
        for item in &cases {
            for probe in [day, elsewhere] {
                assert_eq!(
                    appears_on(item, probe),
                    !placements_for(item, probe).is_empty(),
                    "{} on {probe}", item.name
                );
            }
        }
    }

    #[test]
    fn unplanned_deadlineless_task_has_no_placement() {
        assert_eq!(placements_for(&task(vec![], None, None), day()), vec![]);
    }

    #[test]
    fn create_drag_previews_the_pulled_out_block() {
        // 09:00 anchor, pointer at 10:30.
        assert_eq!(preview(&Drag::Create { anchor: 540 }, 630.0, 30), (540, 90));
        // Pulling upwards builds the same block rather than an inverted one.
        assert_eq!(preview(&Drag::Create { anchor: 630 }, 540.0, 30), (540, 90));
        // A press that barely moves still yields a legal, minimum-length block.
        assert_eq!(preview(&Drag::Create { anchor: 540 }, 542.0, 30), (540, MIN_BLOCK_MINUTES));
        // A create addresses no existing block.
        assert_eq!(Drag::Create { anchor: 540 }.target(), None);
    }

    #[test]
    fn move_drag_keeps_the_grab_point_under_the_pointer() {
        // Grabbed 30 minutes into a 60-minute block; pointer now at 14:00, so
        // the block's start should sit 30 minutes earlier, at 13:30.
        let drag = Drag::Move { id: 7, session: Some(1), grab_offset: 30, minutes: 60 };
        assert_eq!(preview(&drag, 14.0 * 60.0, 30), (13 * 60 + 30, 60));
        // Length is preserved by a move, not recomputed from the pointer.
        let (_, minutes) = preview(&drag, 9.0 * 60.0, 30);
        assert_eq!(minutes, 60);
        // The gesture knows which block it holds: task 7's second session.
        assert_eq!(drag.target(), Some((7, Some(1))));
    }

    #[test]
    fn move_drag_cannot_push_a_block_out_of_the_day() {
        let drag = Drag::Move { id: 7, session: Some(0), grab_offset: 0, minutes: 90 };
        // Dragged past midnight: the block stops flush with the end of the day.
        let (start, minutes) = preview(&drag, (DAY_MINUTES + 300) as f32, 30);
        assert_eq!((start, minutes), (DAY_MINUTES - 90, 90));
        // Dragged above midnight: it stops at 00:00.
        let (start, _) = preview(&drag, -200.0, 30);
        assert_eq!(start, 0);
    }

    #[test]
    fn resize_drag_holds_the_start_and_respects_the_minimum() {
        // `session: None` addresses an event's block.
        let drag = Drag::Resize { id: 3, session: None, start: 600 };
        // Pulled down to 11:45.
        assert_eq!(preview(&drag, 11.0 * 60.0 + 45.0, 30), (600, 105));
        // Pulled up above its own start: collapses to the minimum, and the start
        // does not move.
        assert_eq!(preview(&drag, 300.0, 30), (600, MIN_BLOCK_MINUTES));
        assert_eq!(drag.target(), Some((3, None)));
    }

    #[test]
    fn backlog_drop_uses_the_supplied_length_at_the_pointer() {
        let drag = Drag::FromBacklog { id: 12 };
        assert_eq!(preview(&drag, 9.0 * 60.0 + 7.0, 45), (540, 45));
        // Dropped near midnight, it is pulled back to fit rather than truncated.
        let (start, minutes) = preview(&drag, (DAY_MINUTES - 10) as f32, 60);
        assert_eq!(minutes, 60);
        assert_eq!(start, DAY_MINUTES - 60);
        // A tray drop adds a new session; it addresses no existing block.
        assert_eq!(drag.target(), None);
    }

    #[test]
    fn lay_out_gives_disjoint_blocks_the_full_width() {
        let placements = [
            Placement::Block { start: 540, minutes: 60 },
            Placement::Block { start: 660, minutes: 60 },
        ];
        let lanes = lay_out(&placements);
        assert_eq!(lanes[0], Lane { column: 0, columns: 1 });
        assert_eq!(lanes[1], Lane { column: 0, columns: 1 });
    }

    #[test]
    fn lay_out_splits_overlapping_blocks_into_columns() {
        let placements = [
            Placement::Block { start: 540, minutes: 120 }, // 09:00–11:00
            Placement::Block { start: 600, minutes: 60 },  // 10:00–11:00, overlaps it
            Placement::Block { start: 570, minutes: 30 },  // 09:30–10:00, also overlaps it
        ];
        let lanes = lay_out(&placements);

        // All three are one cluster (the long block ties them together) so they
        // agree on the width...
        assert!(lanes.iter().all(|l| l.columns == 2), "{lanes:?}");
        // ...but only two columns are needed: the two short blocks are back to
        // back, so the second reuses the column the first vacated at 10:00.
        assert_eq!(lanes[0].column, 0, "the long block holds column 0");
        assert_eq!(lanes[1].column, 1);
        assert_eq!(lanes[2].column, 1);
    }

    #[test]
    fn lay_out_uses_a_third_column_only_for_a_real_three_way_overlap() {
        let placements = [
            Placement::Block { start: 540, minutes: 120 }, // 09:00–11:00
            Placement::Block { start: 570, minutes: 90 },  // 09:30–11:00
            Placement::Block { start: 600, minutes: 30 },  // 10:00–10:30
        ];
        let lanes = lay_out(&placements);

        // 10:00–10:30 is genuinely triple-booked, so nothing can share a column.
        assert!(lanes.iter().all(|l| l.columns == 3), "{lanes:?}");
        let mut columns: Vec<usize> = lanes.iter().map(|l| l.column).collect();
        columns.sort_unstable();
        assert_eq!(columns, vec![0, 1, 2], "each block gets its own column");
    }

    #[test]
    fn lay_out_reuses_a_column_once_it_is_free() {
        let placements = [
            Placement::Block { start: 540, minutes: 180 }, // 09:00–12:00
            Placement::Block { start: 540, minutes: 60 },  // 09:00–10:00
            Placement::Block { start: 600, minutes: 60 },  // 10:00–11:00, can reuse column 1
        ];
        let lanes = lay_out(&placements);
        assert_eq!(lanes.iter().map(|l| l.columns).max(), Some(2), "{lanes:?}");
        assert_eq!(lanes[1].column, lanes[2].column, "the freed column is reused");
    }

    #[test]
    fn summarize_counts_overlapping_time_once() {
        let placements = [
            Placement::Block { start: 540, minutes: 120 }, // 09:00–11:00
            Placement::Block { start: 600, minutes: 120 }, // 10:00–12:00
            Placement::Marker { at: 900, due: true },
        ];
        let summary = summarize(&placements);
        // 09:00–12:00 is three hours of the day committed, not four.
        assert_eq!(summary.planned_minutes, 180);
        assert_eq!(summary.blocks, 2);
        assert_eq!(summary.due, 1);
    }

    #[test]
    fn summarize_adds_disjoint_spans() {
        let placements = [
            Placement::Block { start: 540, minutes: 60 },
            Placement::Block { start: 780, minutes: 30 },
        ];
        assert_eq!(summarize(&placements).planned_minutes, 90);
    }

    #[test]
    fn formatting_is_human_readable() {
        assert_eq!(format_minutes(0), "00:00");
        assert_eq!(format_minutes(9 * 60 + 5), "09:05");
        assert_eq!(format_minutes(23 * 60 + 45), "23:45");
        assert_eq!(format_duration(45), "45m");
        assert_eq!(format_duration(60), "1h");
        assert_eq!(format_duration(90), "1h 30m");
    }

    #[test]
    fn resolve_on_day_returns_the_requested_wall_clock_time() {
        // Noon is never inside a DST gap, so this holds in any local timezone.
        let resolved = resolve_on_day(day(), 12 * 60 + 30).expect("noon resolves");
        assert_eq!(resolved.date_naive(), day());
        assert_eq!(resolved.hour(), 12);
        assert_eq!(resolved.minute(), 30);
    }

    #[test]
    fn a_plain_drag_plans() {
        // Planning is the planner's job, so the default create kind is a task.
        assert_eq!(CreateKind::default(), CreateKind::Task);
    }

    #[test]
    fn duration_options_keep_a_hand_dragged_length_selectable() {
        // A preset length adds nothing to the list.
        assert_eq!(duration_options(Some(120)), duration_options(None));
        // An odd length dragged out by hand is folded in, in order, so the
        // picker shows it as the selection instead of the nearest preset.
        let options = duration_options(Some(65));
        assert!(options.contains(&65), "{options:?}");
        assert!(options.windows(2).all(|pair| pair[0] < pair[1]), "{options:?}");
        // Nothing shorter than a legal block is ever offered.
        assert_eq!(duration_options(Some(1)).first(), Some(&MIN_BLOCK_MINUTES));
    }

    #[test]
    fn drop_length_is_the_unplanned_remainder() {
        // "This takes two hours" is a fact about the task, so dropping it on
        // the timeline lands two hours, not the default half-hour.
        let estimated = task(vec![], None, Some(120));
        assert_eq!(drop_length_for(&estimated), 120);

        // Half of it already booked: the next drop is the other half. This is
        // what lets one card be dragged out day after day until it runs out.
        let half_planned = task(vec![session(at(2026, 8, 13, 9, 0), 60)], None, Some(120));
        assert_eq!(drop_length_for(&half_planned), 60);

        // With no estimate, the default.
        assert_eq!(drop_length_for(&task(vec![], None, None)), DEFAULT_BLOCK_MINUTES);
        // A nonsense estimate still yields a legal block.
        assert_eq!(drop_length_for(&task(vec![], None, Some(1))), MIN_BLOCK_MINUTES);
    }

    #[test]
    fn backlog_group_puts_everything_still_owed_at_the_top() {
        let day = day(); // 2026-08-14
        let due_today = at(2026, 8, 14, 17, 0);
        let overdue = at(2026, 8, 12, 9, 0);
        let later = at(2026, 8, 20, 9, 0);

        assert_eq!(backlog_group(Some(due_today), day), BacklogGroup::Due);
        // Still unplanned two days after it was owed: it wants a slot today.
        assert_eq!(backlog_group(Some(overdue), day), BacklogGroup::Due);
        assert_eq!(backlog_group(Some(later), day), BacklogGroup::Later);
        // Nothing owed at all is ordinary backlog, not a call to action.
        assert_eq!(backlog_group(None, day), BacklogGroup::Later);
    }

    #[test]
    fn relative_due_reads_as_a_countdown() {
        let now = at(2026, 8, 14, 12, 0);
        assert_eq!(relative_due(at(2026, 8, 17, 12, 0), now), "3d");
        assert_eq!(relative_due(at(2026, 8, 14, 15, 0), now), "3h");
        assert_eq!(relative_due(at(2026, 8, 14, 12, 30), now), "30m");
        assert_eq!(relative_due(at(2026, 8, 14, 11, 0), now), "overdue");
    }
}
