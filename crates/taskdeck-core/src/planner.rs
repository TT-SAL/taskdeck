//! The planner, copied from today's `src/planner.rs` with the zone parameter
//! and sids (core.md §14.2, §20). Its arithmetic in integers is in `time`
//! (core.md §5.5).

use crate::board::{Block, Item, Recurrence};
use crate::time::{Instant, Zone};

/// Minutes in a day (today's planner; core.md §14.2).
pub const DAY_MINUTES: i32 = 24 * 60;
/// The snapping grid in minutes (today's planner; core.md §5.5).
pub const SNAP_MINUTES: i32 = 15;
/// The shortest block (today's planner; core.md §5.5).
pub const MIN_BLOCK_MINUTES: u32 = 15;
/// A new block's length (today's planner; core.md §14.2).
pub const DEFAULT_BLOCK_MINUTES: u32 = 30;

/// What an item draws on a day (today's planner; core.md §14.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Block { start: i32, minutes: u32 },
    Marker { at: i32, due: bool },
}

impl Placement {
    /// The placement's start minute (today's planner; core.md §14.2).
    pub fn start(&self) -> i32 {
        unimplemented!()
    }

    /// The placement's end minute (today's planner; core.md §14.2).
    pub fn end(&self) -> i32 {
        unimplemented!()
    }
}

/// A placement and the sid of its block (core.md §14.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayPlacement {
    pub session: Option<u64>,
    pub placement: Placement,
}

/// An item's placements on a day, blocks in ascending sid (core.md §14.2).
pub fn placements_for(item: &Item, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> Vec<DayPlacement> {
    unimplemented!()
}

/// The fields of an item that placing reads (today's planner; core.md §14.2).
#[derive(Clone, Copy)]
pub struct Placeable<'a> {
    pub is_event: bool,
    pub deadline: Option<Instant>,
    pub duration_minutes: Option<u32>,
    pub blocks: &'a [Block],
    pub recurrence: Option<&'a Recurrence>,
}

impl<'a> Placeable<'a> {
    /// The placeable view of an item (today's planner; core.md §14.2).
    pub fn of(item: &'a Item) -> Self {
        unimplemented!()
    }
}

/// The placements of a placeable on a day (core.md §14.2).
pub fn placements_of(item: Placeable, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> Vec<DayPlacement> {
    unimplemented!()
}

/// Whether an item appears on a day (core.md §14.2).
pub fn appears_on(item: &Item, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> bool {
    unimplemented!()
}

/// A placement's column among overlapping ones (today's planner; core.md §14.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lane {
    pub column: usize,
    pub columns: usize,
}

/// Lays overlapping placements out in lanes (core.md §14.2).
pub fn lay_out(placements: &[Placement]) -> Vec<Lane> {
    unimplemented!()
}

/// A day's planned minutes, blocks and due markers (today's planner; core.md §14.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DaySummary {
    pub planned_minutes: i32,
    pub blocks: usize,
    pub due: usize,
}

/// Summarizes a day's placements (core.md §14.2).
pub fn summarize(placements: &[Placement]) -> DaySummary {
    unimplemented!()
}

/// The day's summary line (core.md §14.2).
pub fn summary_text(planned: DaySummary, routine: DaySummary, done: DaySummary) -> String {
    unimplemented!()
}

/// Minutes as a clock time (core.md §14.2).
pub fn format_minutes(minutes: i32) -> String {
    unimplemented!()
}

/// A duration as text (core.md §14.2).
pub fn format_duration(minutes: u32) -> String {
    unimplemented!()
}

/// The length a dropped item gets (core.md §14.2).
pub fn drop_length_for(item: &Item) -> u32 {
    unimplemented!()
}

/// The estimate choices around the current one (core.md §14.2).
pub fn duration_options(current: Option<u32>) -> Vec<u32> {
    unimplemented!()
}

/// The tray group of a task (today's planner; core.md §14.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacklogGroup {
    Due,
    Later,
}

/// `Due` when the deadline's local day is on or before `day` (core.md §14.2).
pub fn backlog_group(deadline: Option<Instant>, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> BacklogGroup {
    unimplemented!()
}

/// How far off a deadline is, as text (core.md §14.2).
pub fn relative_due(deadline: Instant, now: Instant) -> String {
    unimplemented!()
}

/// A block that reflow may move (today's planner; core.md §14.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flowable<K> {
    pub key: K,
    pub start: i32,
    pub minutes: u32,
}

/// Minutes of planned work already behind `now` (core.md §14.2).
pub fn behind_minutes(blocks: &[Placement], now: i32) -> i32 {
    unimplemented!()
}

/// Reflows blocks around anchors from `now` (core.md §14.2, §13.4).
pub fn reflow<K: Copy>(blocks: &[Flowable<K>], anchors: &[(i32, i32)], now: i32) -> Vec<(K, i32)> {
    unimplemented!()
}
