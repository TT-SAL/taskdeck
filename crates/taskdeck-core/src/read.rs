//! Read queries over the board (core.md §14.2, §14.4, §20).

use chrono::Datelike;

use crate::board::{ArchiveRow, BoardData, Kind, RowBlock};
use crate::overlay::Overlay;
use crate::planner;
use crate::time::{self, Instant, Zone};

/// Where a new block goes on an empty day: today's `DEFAULT_MORNING_HOUR`
/// (board.rs:68).
const DEFAULT_MORNING_HOUR: i32 = 7;

/// Task ids by score, ties in ascending id (core.md §14.2, CORE-READ-003).
///
/// Today's `Board::ranked_ids` (board.rs:860-870): every task, by score, most
/// pressing first. Events and routines are not ranked. The items are taken in
/// the board's order, ascending id (§6.12), and the sort is stable, so equal
/// scores keep ascending id. The board is not reordered (CORE-READ-005).
pub fn ranked_ids(board: &BoardData, now: Instant, zone: &(impl Zone + ?Sized)) -> Vec<u64> {
    let seed = shuffle_seed(now, zone);
    let mut scored: Vec<(f32, u64)> = board
        .items
        .iter()
        .filter(|item| item.kind == Kind::Task)
        .map(|item| (item.importance_score(now, seed), item.id))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().map(|(_, id)| id).collect()
}

/// The tie-break seed of the local day (core.md §14.2).
///
/// Today's `Board::shuffle_seed` (board.rs:854-856): `num_days_from_ce` of
/// the day `now` falls on in `zone`.
pub fn shuffle_seed(now: Instant, zone: &(impl Zone + ?Sized)) -> u64 {
    // A local day of the instant range is after the common era began.
    u64::try_from(time::local_day(now, zone).num_days_from_ce()).unwrap_or(0)
}

/// Minutes of planned work behind `now` on a day (core.md §14.2).
///
/// Today's `Board::behind_minutes` (board.rs:912-920): zero on any day but
/// the local day of `now`; otherwise the part of the day's task blocks
/// (reflow's flowables) that lies before the now-line.
pub fn behind_minutes(board: &BoardData, day: chrono::NaiveDate, now: Instant, zone: &(impl Zone + ?Sized)) -> i32 {
    let Some(now_minutes) = time::now_marker(day, now, zone) else { return 0 };
    // Only the flowables are read; the overlay's anchors play no part.
    let (blocks, _) = reflow_inputs(board, &Overlay::default(), day, zone);
    let placements: Vec<planner::Placement> = blocks
        .iter()
        .map(|block| planner::Placement::Block { start: block.start, minutes: block.minutes })
        .collect();
    planner::behind_minutes(&placements, now_minutes)
}

/// The first free start minute of a day (core.md §14.2).
///
/// Today's `Board::next_free_start` (board.rs:875-884): after the last thing
/// already on the day, or the default morning hour on an empty day, clamped
/// so a default-length block still fits.
pub fn next_free_start(board: &BoardData, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> i32 {
    let day_end = planner::DAY_MINUTES - i32::try_from(planner::DEFAULT_BLOCK_MINUTES).unwrap_or(0);
    board
        .items
        .iter()
        .flat_map(|item| planner::placements_for(item, day, zone))
        .map(|placed| placed.placement.end())
        .max()
        .unwrap_or(DEFAULT_MORNING_HOUR * 60)
        .clamp(0, day_end)
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
///
/// `BoardData.archive` is kept in the archive order (CORE-DATA-003), so
/// newest first is the list read backwards.
pub fn archive_newest_first(board: &BoardData) -> impl Iterator<Item = &ArchiveRow> {
    board.archive.iter().rev()
}

/// The most days `archive_by_day` buckets (lead ruling, spec change 5, §14.4).
const MAX_DAYS: u32 = 366;

/// The ghost sources of each day from `from` (core.md §14.4).
///
/// One pass over the archive newest first (CORE-SNAP-003): for each row
/// whose `kind` is not `event`, each of its blocks in ascending sid goes to
/// the bucket of its local day when that day is one of the `days` days from
/// `from`. The result has one bucket per day, the first for `from`; a `days`
/// above 366 is read as 366 (lead ruling, spec change 5, §14.4).
pub fn archive_by_day<'a>(
    board: &'a BoardData,
    from: chrono::NaiveDate,
    days: u32,
    zone: &(impl Zone + ?Sized),
) -> Vec<Vec<(&'a ArchiveRow, &'a RowBlock)>> {
    // A `days` above 366 is read as 366 (lead ruling, spec change 5, §14.4),
    // so no caller can ask for billions of buckets.
    let count = usize::try_from(days.min(MAX_DAYS)).unwrap_or(0);
    let mut buckets: Vec<Vec<(&'a ArchiveRow, &'a RowBlock)>> = vec![Vec::new(); count];
    for row in archive_newest_first(board) {
        // An archived event places nothing: today's ghost placeable has no
        // deadline (phone.rs:2409-2421).
        if row.kind == Kind::Event {
            continue;
        }
        for block in &row.blocks {
            let offset = (time::local_day(block.start, zone) - from).num_days();
            if let Some(bucket) = usize::try_from(offset).ok().and_then(|k| buckets.get_mut(k)) {
                bucket.push((row, block));
            }
        }
    }
    buckets
}

#[cfg(test)]
mod tests;
