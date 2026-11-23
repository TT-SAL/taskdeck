//! Today's tests of the board queries (board.rs:1886-1913, the read part of
//! `reflow_and_notes_and_queries`), ported with explicit zones, and the
//! clauses of core.md §14.1, §14.2 and §14.4.
//!
//! Not ported: the `apply` half of that test (reflow, notes, `Feed`), which
//! the command and golden tests cover.

use proptest::prelude::*;

use super::*;
use crate::board::{Block, Item, ItemStamps, Outcome, Recurrence, Stamp};
use crate::planner::{Placeable, Placement};
use crate::time::{FixedZone, TableZone};

/// Today's tests ran in `Local`; a fixed zone at +03:00 stands in for it:
/// none of these times is near a transition.
const ZONE: FixedZone = FixedZone(3 * 3600);
const ZERO: Stamp = Stamp { seq: 1, device_no: 0 };
const STAMPS: ItemStamps = ItemStamps { estimate: ZERO, levels: ZERO, name: ZERO, repeat: ZERO, time: ZERO };

fn day(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default()
}

/// `Local.with_ymd_and_hms(y, m, d, hour, minute, 0)` of today's tests, in `ZONE`.
fn at(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> Instant {
    let minutes = i32::try_from(hour * 60 + minute).unwrap_or(0);
    time::resolve_on_day(day(y, m, d), minutes, &ZONE).unwrap_or(Instant::MIN)
}

fn item(id: u64, kind: Kind) -> Item {
    Item {
        id,
        kind,
        name: format!("item {id}"),
        created: at(2026, 9, 1, 9, 0),
        deadline: None,
        importance: None,
        time_importance: Some(1),
        duration_minutes: None,
        recurrence: None,
        blocks: Vec::new(),
        stamps: STAMPS,
    }
}

fn block(sid: u64, start: Instant, minutes: u32) -> Block {
    Block { sid, start, minutes, stamp: ZERO }
}

fn row(id: u64, kind: Kind, archived_at: Instant, blocks: Vec<RowBlock>) -> ArchiveRow {
    ArchiveRow {
        id,
        kind,
        name: format!("row {id}"),
        created: at(2026, 9, 1, 9, 0),
        deadline: None,
        importance: None,
        time_importance: Some(1),
        duration_minutes: None,
        recurrence: (kind == Kind::Routine).then_some(Recurrence {
            days: 0,
            anchor: day(2026, 9, 1),
            start_minutes: 0,
            minutes: 30,
        }),
        blocks,
        archived_at,
        outcome: Outcome::Finished,
    }
}

fn board(items: Vec<Item>, archive: Vec<ArchiveRow>) -> BoardData {
    BoardData { items, archive, ..BoardData::empty() }
}

/// The board of today's `reflow_and_notes_and_queries` after its reflow: the
/// task `a` (id 1) with its hour moved to 13:00, the event "fixed" (id 2) at
/// 12:00 for an hour, on 2026-09-04.
fn reflowed() -> BoardData {
    let mut a = item(1, Kind::Task);
    a.blocks.push(block(1, at(2026, 9, 4, 13, 0), 60));
    let mut fixed = item(2, Kind::Event);
    fixed.deadline = Some(at(2026, 9, 4, 12, 0));
    fixed.duration_minutes = Some(60);
    board(vec![a, fixed], Vec::new())
}

// Today's `reflow_and_notes_and_queries`, its queries: before the reflow the
// nine o'clock hour is wholly behind a 12:05 now-line; after it nothing is;
// the task list ranks the one task; the next free start follows the last
// block, or is the morning hour on an empty day.
// spec: CORE-READ-001
#[test]
fn reflow_and_notes_and_queries() {
    let d = day(2026, 9, 4);
    let noon = at(2026, 9, 4, 12, 5);
    let mut before = reflowed();
    before.items[0].blocks[0].start = at(2026, 9, 4, 9, 0);
    assert_eq!(behind_minutes(&before, d, noon, &ZONE), 60);

    let after = reflowed();
    assert_eq!(behind_minutes(&after, d, noon, &ZONE), 0);
    assert_eq!(ranked_ids(&after, noon, &ZONE), vec![1]);
    assert_eq!(next_free_start(&after, d, &ZONE), 14 * 60);
    assert_eq!(next_free_start(&after, day(2026, 9, 5), &ZONE), DEFAULT_MORNING_HOUR * 60);
}

// Behind is zero on any day but the local day of `now`, and that day is the
// caller's zone's: 2026-09-04T21:30Z is 9-5 at +03:00 and 9-4 in UTC.
// spec: CORE-READ-001
#[test]
fn behind_is_counted_on_the_local_day_of_now_only() {
    let mut b = board(vec![item(1, Kind::Task)], Vec::new());
    b.items[0].blocks.push(block(1, at(2026, 9, 4, 9, 0), 60));
    let late = Instant::parse_canonical("2026-09-04T21:30:00.000000000Z").unwrap_or(Instant::MIN);
    assert_eq!(behind_minutes(&b, day(2026, 9, 4), late, &ZONE), 0);
    assert_eq!(behind_minutes(&b, day(2026, 9, 4), late, &FixedZone(0)), 60);
    // An event's hour is never behind, only a task's block.
    let mut e = item(2, Kind::Event);
    e.deadline = Some(at(2026, 9, 4, 9, 0));
    e.duration_minutes = Some(60);
    let events = board(vec![e], Vec::new());
    assert_eq!(behind_minutes(&events, day(2026, 9, 4), at(2026, 9, 4, 12, 0), &ZONE), 0);
}

// The seed is `num_days_from_ce` of the local day, so it turns over at the
// caller's midnight, not UTC's.
// spec: CORE-READ-001
#[test]
fn the_seed_is_the_local_day() {
    let late = Instant::parse_canonical("2026-09-04T21:30:00.000000000Z").unwrap_or(Instant::MIN);
    let utc = shuffle_seed(late, &FixedZone(0));
    assert_eq!(utc, u64::try_from(day(2026, 9, 4).num_days_from_ce()).unwrap_or(0));
    assert_eq!(shuffle_seed(late, &ZONE), utc + 1);
    assert_eq!(shuffle_seed(late, &FixedZone(-3 * 3600)), utc);
}

// The next free start is the latest end of anything placed on the day, a
// due marker counting a quarter-hour, clamped so a default block still fits.
// spec: CORE-READ-001
#[test]
fn the_next_free_start_follows_everything_on_the_day() {
    let mut late = item(1, Kind::Task);
    late.deadline = Some(at(2026, 9, 4, 18, 0));
    let b = board(vec![late], Vec::new());
    assert_eq!(next_free_start(&b, day(2026, 9, 4), &ZONE), 18 * 60 + 15);
    let mut evening = item(2, Kind::Task);
    evening.blocks.push(block(1, at(2026, 9, 4, 23, 0), 60));
    let b = board(vec![evening], Vec::new());
    assert_eq!(next_free_start(&b, day(2026, 9, 4), &ZONE), 24 * 60 - 30);
    // In UTC that block is on 9-4 at 20:00.
    assert_eq!(next_free_start(&b, day(2026, 9, 4), &FixedZone(0)), 21 * 60);
}

// Tasks only, by score descending, the board's items taken in ascending id;
// the board is neither changed nor reordered.
// spec: CORE-READ-003
// spec: CORE-READ-002
// spec: CORE-READ-005
#[test]
fn ranked_ids_are_the_tasks_by_score_with_ties_in_ascending_id() {
    let now = at(2026, 9, 4, 12, 0);
    let mut items: Vec<Item> = (1..=12).map(|id| item(id, Kind::Task)).collect();
    for (k, it) in items.iter_mut().enumerate() {
        let k = u32::try_from(k).unwrap_or(0);
        it.importance = Some(u8::try_from(k % 5).unwrap_or(0));
        if k % 3 == 0 {
            it.deadline = Some(at(2026, 9, 5 + k, 17, 0));
        }
    }
    items[4].kind = Kind::Event;
    items[7].kind = Kind::Routine;
    items[7].recurrence = Some(Recurrence { days: 127, anchor: day(2026, 9, 1), start_minutes: 0, minutes: 30 });
    let b = board(items, Vec::new());
    let copy = b.clone();
    let ranked = ranked_ids(&b, now, &ZONE);
    assert_eq!(b, copy);
    assert_eq!(ranked.len(), 10);
    assert!(!ranked.contains(&5) && !ranked.contains(&8));
    let seed = shuffle_seed(now, &ZONE);
    let score = |id: u64| b.item(id).map_or(0.0, |i| i.importance_score(now, seed));
    for pair in ranked.windows(2) {
        let (x, y) = (score(pair[0]), score(pair[1]));
        assert!(x > y || (x == y && pair[0] < pair[1]), "{pair:?}");
    }
}

// The archive read backwards: the reverse of the archive order of §6.12.
// spec: CORE-READ-002
#[test]
fn the_archive_newest_first_is_the_archive_read_backwards() {
    let t = |h| at(2026, 9, 4, h, 0);
    let b = board(
        Vec::new(),
        vec![row(3, Kind::Task, t(9), Vec::new()), row(1, Kind::Task, t(10), Vec::new()), row(2, Kind::Task, t(10), Vec::new())],
    );
    let ids: Vec<u64> = archive_newest_first(&b).map(|r| r.id).collect();
    assert_eq!(ids, vec![2, 1, 3]);
}

// One bucket per day; an archived event places nothing; every other row's
// blocks go to their local day in ascending sid, rows newest first; a block
// outside the range is dropped.
// spec: CORE-SNAP-003
#[test]
fn archive_by_day_buckets_blocks_by_their_local_day() {
    let rb = |sid, start, minutes| RowBlock { sid, start, minutes };
    let older = row(
        10,
        Kind::Task,
        at(2026, 9, 6, 9, 0),
        vec![rb(1, at(2026, 9, 4, 9, 0), 60), rb(2, at(2026, 9, 5, 1, 0), 30), rb(3, at(2026, 9, 8, 9, 0), 30)],
    );
    let event = row(11, Kind::Event, at(2026, 9, 6, 10, 0), vec![rb(4, at(2026, 9, 4, 11, 0), 30)]);
    let routine = row(12, Kind::Routine, at(2026, 9, 6, 11, 0), vec![rb(5, at(2026, 9, 4, 8, 0), 30)]);
    let b = board(Vec::new(), vec![older, event, routine]);
    let buckets = archive_by_day(&b, day(2026, 9, 4), 3, &ZONE);
    let keys: Vec<Vec<(u64, u64)>> =
        buckets.iter().map(|bucket| bucket.iter().map(|(r, blk)| (r.id, blk.sid)).collect()).collect();
    assert_eq!(keys, vec![vec![(12, 5), (10, 1)], vec![(10, 2)], vec![]]);
    // In UTC the 01:00 block (+03:00) falls on 9-4.
    let buckets = archive_by_day(&b, day(2026, 9, 4), 1, &FixedZone(0));
    let keys: Vec<(u64, u64)> = buckets[0].iter().map(|(r, blk)| (r.id, blk.sid)).collect();
    assert_eq!(keys, vec![(12, 5), (10, 1), (10, 2)]);
    assert_eq!(archive_by_day(&b, day(2026, 9, 4), 0, &ZONE).len(), 0);
}

/// Today's per-day ghost scan (phone.rs:2409-2421): every row newest first,
/// placed by `placements_of` with no deadline, no estimate and no rule.
fn per_day_scan(b: &BoardData, d: chrono::NaiveDate, zone: &TableZone) -> Vec<(u64, u64, Placement)> {
    let mut out = Vec::new();
    for r in archive_newest_first(b) {
        let blocks: Vec<Block> = r.blocks.iter().map(|x| block(x.sid, x.start, x.minutes)).collect();
        let placeable = Placeable {
            is_event: r.kind == Kind::Event,
            deadline: None,
            duration_minutes: None,
            blocks: &blocks,
            recurrence: None,
        };
        for placed in planner::placements_of(placeable, d, zone) {
            out.push((r.id, placed.session.unwrap_or(0), placed.placement));
        }
    }
    out
}

proptest! {
    // One pass gives, day for day, exactly what today's per-day scan gives,
    // in a zone with a transition inside the range.
    // spec: CORE-SNAP-003
    #[test]
    fn archive_by_day_equals_the_per_day_scan(
        rows in proptest::collection::vec(
            (0u8..3, 0i64..400, proptest::collection::vec((0i64..(10 * 24 * 60), 0u32..200), 0..4)),
            0..12,
        ),
        days in 0u32..9,
    ) {
        let base = Instant::parse_canonical("2026-10-20T00:00:00.000000000Z").unwrap_or(Instant::MIN);
        let zone = TableZone::new(7200, vec![(base.as_unix_ms() + 5 * 86_400_000 + 3_600_000, 3600)])
            .unwrap_or_else(|_| TableZone { initial: 0, transitions: Vec::new() });
        let mut sid = 0;
        let mut archive: Vec<ArchiveRow> = rows
            .iter()
            .enumerate()
            .map(|(k, (kind, archived, blocks))| {
                let kind = [Kind::Task, Kind::Event, Kind::Routine][usize::from(*kind)];
                let blocks = blocks
                    .iter()
                    .map(|&(m, minutes)| {
                        sid += 1;
                        RowBlock { sid, start: base.plus_seconds(m * 60).unwrap_or(base), minutes }
                    })
                    .collect();
                row(u64::try_from(k).unwrap_or(0) + 1, kind, base.plus_seconds(archived * 3600).unwrap_or(base), blocks)
            })
            .collect();
        archive.sort_by_key(|r| (r.archived_at, r.id));
        let b = board(Vec::new(), archive);
        let from = day(2026, 10, 21);
        let buckets = archive_by_day(&b, from, days, &zone);
        prop_assert_eq!(buckets.len(), usize::try_from(days).unwrap_or(0));
        for (d, bucket) in from.iter_days().zip(&buckets) {
            let got: Vec<(u64, u64, Placement)> = bucket
                .iter()
                .map(|(r, blk)| {
                    let placement = Placement::Block {
                        start: time::local_minutes(blk.start, &zone),
                        minutes: blk.minutes.max(planner::MIN_BLOCK_MINUTES),
                    };
                    (r.id, blk.sid, placement)
                })
                .collect();
            prop_assert_eq!(got, per_day_scan(&b, d, &zone));
        }
    }
}
