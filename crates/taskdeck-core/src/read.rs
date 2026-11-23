//! Read queries over the board (core.md §14.2, §14.4, §20).

use crate::board::{ArchiveRow, BoardData, RowBlock};
use crate::overlay::Overlay;
use crate::planner;
use crate::time::{Instant, Zone};

/// Task ids by score, ties in ascending id (core.md §14.2, CORE-READ-003).
pub fn ranked_ids(board: &BoardData, now: Instant, zone: &(impl Zone + ?Sized)) -> Vec<u64> {
    unimplemented!()
}

/// The tie-break seed of the local day (core.md §14.2).
pub fn shuffle_seed(now: Instant, zone: &(impl Zone + ?Sized)) -> u64 {
    unimplemented!()
}

/// Minutes of planned work behind `now` on a day (core.md §14.2).
pub fn behind_minutes(board: &BoardData, day: chrono::NaiveDate, now: Instant, zone: &(impl Zone + ?Sized)) -> i32 {
    unimplemented!()
}

/// The first free start minute of a day (core.md §14.2).
pub fn next_free_start(board: &BoardData, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> i32 {
    unimplemented!()
}

/// The blocks and anchors reflow takes (core.md §14.2, §13.4 steps 1 and 2).
#[allow(clippy::type_complexity)] // the spec's signature (core.md §20)
pub fn reflow_inputs(
    board: &BoardData,
    overlay: &Overlay,
    day: chrono::NaiveDate,
    zone: &(impl Zone + ?Sized),
) -> (Vec<planner::Flowable<(u64, u64)>>, Vec<(i32, i32)>) {
    let mut flowables = Vec::new();
    let mut anchors = Vec::new();
    // Step 1: live items in ascending id (`items` ascends by id, §6.12), each
    // item's placements in `placements_for`'s order, its blocks ascending by
    // sid. A block placement with a sid is a task's block and flows; every
    // other block placement (an event's, a routine's) is an anchor; markers
    // are dropped.
    for item in &board.items {
        for placed in planner::placements_for(item, day, zone) {
            let planner::Placement::Block { start, minutes } = placed.placement else { continue };
            match placed.session {
                Some(sid) => flowables.push(planner::Flowable { key: (item.id, sid), start, minutes }),
                None => anchors.push((start, start.saturating_add(i32::try_from(minutes).unwrap_or(i32::MAX)))),
            }
        }
    }
    // Step 2: the overlay's anchors of the day, appended.
    anchors.extend(overlay.anchors_on(day));
    (flowables, anchors)
}

/// The archive in reverse archive order (core.md §14.2, §6.12).
// Skeleton: `!` is no iterator, so the opaque type needs a concrete one after
// the `unimplemented!()`.
#[allow(unreachable_code)]
pub fn archive_newest_first(board: &BoardData) -> impl Iterator<Item = &ArchiveRow> {
    unimplemented!();
    std::iter::empty()
}

/// The ghost sources of each day from `from` (core.md §14.4).
pub fn archive_by_day<'a>(
    board: &'a BoardData,
    from: chrono::NaiveDate,
    days: u32,
    zone: &(impl Zone + ?Sized),
) -> Vec<Vec<(&'a ArchiveRow, &'a RowBlock)>> {
    unimplemented!()
}
