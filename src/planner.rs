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
//! it*. `Active::deadline` answers the first; `Active::planned_start` answers
//! the second. That is why a task can appear twice in a week: as a due marker on
//! Friday and as a worked-on block on Tuesday morning. Events are simpler —
//! an event's `deadline` is when it happens — so they are planned by moving that.

use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Timelike};

use crate::tasks::Active;

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

/// Minutes from midnight, if `at` falls on `day`.
fn minutes_into_day(at: DateTime<Local>, day: NaiveDate) -> Option<i32> {
    (at.date_naive() == day).then(|| at.hour() as i32 * 60 + at.minute() as i32)
}

/// Where `item` belongs on `day`'s timeline, or `None` if it doesn't belong on
/// that day at all.
///
/// A task with both a `planned_start` and a `deadline` legitimately produces a
/// placement on **two** days — a block where it is worked on and a due marker
/// where it is owed — so this is asked per-day rather than answered once.
pub fn placement_for(item: &Active, day: NaiveDate) -> Option<Placement> {
    if item.is_event {
        let at = minutes_into_day(item.deadline?, day)?;
        return Some(match item.duration_minutes {
            Some(minutes) => Placement::Block { start: at, minutes: minutes.max(MIN_BLOCK_MINUTES) },
            None => Placement::Marker { at, due: false },
        });
    }

    // A planned task shows as a block on the day it is worked on...
    if let Some(planned) = item.planned_start {
        if let Some(start) = minutes_into_day(planned, day) {
            return Some(Placement::Block {
                start,
                minutes: item.duration_minutes.unwrap_or(DEFAULT_BLOCK_MINUTES).max(MIN_BLOCK_MINUTES),
            });
        }
    }

    // ...and as a due marker on the day it is owed, which may be a different day.
    let due = minutes_into_day(item.deadline?, day)?;
    Some(Placement::Marker { at: due, due: true })
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
    /// Moving a block. `grab_offset` is how far into the block the press landed,
    /// so it doesn't jump to centre itself under the pointer.
    Move { id: u64, grab_offset: i32, minutes: u32 },
    /// Dragging a block's bottom edge; `start` is held fixed.
    Resize { id: u64, start: i32 },
    /// Dragging a card out of the backlog. Nothing is committed until release.
    FromBacklog { id: u64 },
}

/// Id reported for the block a [`Drag::Create`] is pulling out: it has no
/// `Active` behind it until the pointer is released, but the preview still flows
/// through the same layout as real blocks so neighbours move aside for it.
pub const PENDING_ID: u64 = u64::MAX;

/// Where `drag` currently places a block, as `(id, start, minutes)`.
///
/// `default_minutes` is the length to use for a backlog card being dropped —
/// the caller resolves it from the item, since the gesture itself doesn't carry
/// one. Both the live preview and the commit-on-release go through this, so what
/// the user sees while dragging is exactly what gets saved.
pub fn preview(drag: &Drag, minutes_at_pointer: f32, default_minutes: u32) -> (u64, i32, u32) {
    match *drag {
        Drag::Create { anchor } => {
            let (start, minutes) = block_from_drag(anchor as f32, minutes_at_pointer);
            (PENDING_ID, start, minutes)
        }
        Drag::Move { id, grab_offset, minutes } => {
            let (start, minutes) = clamp_block(minutes_at_pointer - grab_offset as f32, minutes as f32);
            (id, start, minutes)
        }
        Drag::Resize { id, start } => {
            let (start, minutes) = clamp_block(start as f32, minutes_at_pointer - start as f32);
            (id, start, minutes)
        }
        Drag::FromBacklog { id } => {
            let (start, minutes) = clamp_block(minutes_at_pointer, default_minutes as f32);
            (id, start, minutes)
        }
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

/// How long a freshly planned item should run when the user didn't drag out a
/// length: its own duration if it has one, else the default.
pub fn default_length_for(item: &Active) -> u32 {
    item.duration_minutes.unwrap_or(DEFAULT_BLOCK_MINUTES).max(MIN_BLOCK_MINUTES)
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
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, hour, minute, 0).unwrap()
    }

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, 14).unwrap()
    }

    fn task(planned: Option<DateTime<Local>>, deadline: Option<DateTime<Local>>, minutes: Option<u32>) -> Active {
        Active {
            id: 1,
            importance: Some(2),
            time_importance: None,
            name: "t".into(),
            created: at(2026, 8, 1, 9, 0),
            deadline,
            is_event: false,
            planned_start: planned,
            duration_minutes: minutes,
        }
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
    fn placement_distinguishes_planned_work_from_a_due_date() {
        let planned_day = day();
        let due_day = NaiveDate::from_ymd_opt(2026, 8, 16).unwrap();

        // Worked on Friday 09:00 for an hour, due Sunday 17:00.
        let item = task(
            Some(at(2026, 8, 14, 9, 0)),
            Some(at(2026, 8, 16, 17, 0)),
            Some(60),
        );

        assert_eq!(
            placement_for(&item, planned_day),
            Some(Placement::Block { start: 9 * 60, minutes: 60 }),
            "the planned day shows the work block"
        );
        assert_eq!(
            placement_for(&item, due_day),
            Some(Placement::Marker { at: 17 * 60, due: true }),
            "the due day still shows the deadline"
        );
        assert_eq!(
            placement_for(&item, NaiveDate::from_ymd_opt(2026, 8, 15).unwrap()),
            None,
            "an unrelated day shows nothing"
        );
    }

    #[test]
    fn events_are_blocks_once_they_have_a_length() {
        let without = event(at(2026, 8, 14, 14, 30), None);
        assert_eq!(
            placement_for(&without, day()),
            Some(Placement::Marker { at: 14 * 60 + 30, due: false })
        );

        let with = event(at(2026, 8, 14, 14, 30), Some(45));
        assert_eq!(
            placement_for(&with, day()),
            Some(Placement::Block { start: 14 * 60 + 30, minutes: 45 })
        );
    }

    #[test]
    fn unplanned_deadlineless_task_has_no_placement() {
        assert_eq!(placement_for(&task(None, None, None), day()), None);
    }

    #[test]
    fn create_drag_previews_the_pulled_out_block() {
        // 09:00 anchor, pointer at 10:30.
        assert_eq!(preview(&Drag::Create { anchor: 540 }, 630.0, 30), (PENDING_ID, 540, 90));
        // Pulling upwards builds the same block rather than an inverted one.
        assert_eq!(preview(&Drag::Create { anchor: 630 }, 540.0, 30), (PENDING_ID, 540, 90));
        // A press that barely moves still yields a legal, minimum-length block.
        assert_eq!(preview(&Drag::Create { anchor: 540 }, 542.0, 30), (PENDING_ID, 540, MIN_BLOCK_MINUTES));
    }

    #[test]
    fn move_drag_keeps_the_grab_point_under_the_pointer() {
        // Grabbed 30 minutes into a 60-minute block; pointer now at 14:00, so
        // the block's start should sit 30 minutes earlier, at 13:30.
        let drag = Drag::Move { id: 7, grab_offset: 30, minutes: 60 };
        assert_eq!(preview(&drag, 14.0 * 60.0, 30), (7, 13 * 60 + 30, 60));
        // Length is preserved by a move, not recomputed from the pointer.
        let (_, _, minutes) = preview(&drag, 9.0 * 60.0, 30);
        assert_eq!(minutes, 60);
    }

    #[test]
    fn move_drag_cannot_push_a_block_out_of_the_day() {
        let drag = Drag::Move { id: 7, grab_offset: 0, minutes: 90 };
        // Dragged past midnight: the block stops flush with the end of the day.
        let (_, start, minutes) = preview(&drag, (DAY_MINUTES + 300) as f32, 30);
        assert_eq!((start, minutes), (DAY_MINUTES - 90, 90));
        // Dragged above midnight: it stops at 00:00.
        let (_, start, _) = preview(&drag, -200.0, 30);
        assert_eq!(start, 0);
    }

    #[test]
    fn resize_drag_holds_the_start_and_respects_the_minimum() {
        let drag = Drag::Resize { id: 3, start: 600 };
        // Pulled down to 11:45.
        assert_eq!(preview(&drag, 11.0 * 60.0 + 45.0, 30), (3, 600, 105));
        // Pulled up above its own start: collapses to the minimum, and the start
        // does not move.
        assert_eq!(preview(&drag, 300.0, 30), (3, 600, MIN_BLOCK_MINUTES));
    }

    #[test]
    fn backlog_drop_uses_the_supplied_length_at_the_pointer() {
        let drag = Drag::FromBacklog { id: 12 };
        assert_eq!(preview(&drag, 9.0 * 60.0 + 7.0, 45), (12, 540, 45));
        // Dropped near midnight, it is pulled back to fit rather than truncated.
        let (_, start, minutes) = preview(&drag, (DAY_MINUTES - 10) as f32, 60);
        assert_eq!(minutes, 60);
        assert_eq!(start, DAY_MINUTES - 60);
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
    fn relative_due_reads_as_a_countdown() {
        let now = at(2026, 8, 14, 12, 0);
        assert_eq!(relative_due(at(2026, 8, 17, 12, 0), now), "3d");
        assert_eq!(relative_due(at(2026, 8, 14, 15, 0), now), "3h");
        assert_eq!(relative_due(at(2026, 8, 14, 12, 30), now), "30m");
        assert_eq!(relative_due(at(2026, 8, 14, 11, 0), now), "overdue");
    }
}
