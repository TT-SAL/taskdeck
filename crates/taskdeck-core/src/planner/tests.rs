//! Today's planner tests (planner.rs:724-1318), ported, and the zone and
//! read-query clauses of core.md §5.3 and §14.
//!
//! Not ported, because what they test is not: the geometry, drag and preview
//! tests (`TimelineGeometry`, `Drag`, `preview`, `block_from_drag`,
//! `CreateKind`) stay in the UI; `snap`, `clamp_block` and `resolve_on_day`
//! are in `time` and tested there.

use super::*;
use crate::board::{BoardData, ItemStamps, Stamp};
use crate::time::{FixedZone, TableZone};

/// Today's tests ran in `Local`, the zone of the process that ran
/// `cargo test`; the recorded data was made at +03:00. A fixed zone at +03:00
/// stands in for it here: none of these times is near a transition.
const ZONE: FixedZone = FixedZone(3 * 3600);

fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default()
}

/// `Local.with_ymd_and_hms(y, m, d, hour, minute, 0)` of today's tests, in `ZONE`.
fn at(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> Instant {
    let minutes = i32::try_from(hour * 60 + minute).unwrap_or(0);
    time::resolve_on_day(date(y, m, d), minutes, &ZONE).unwrap_or(Instant::MIN)
}

fn block(start: i32, minutes: u32) -> Placement {
    Placement::Block { start, minutes }
}

fn flowable(key: u32, start: i32, minutes: u32) -> Flowable<u32> {
    Flowable { key, start, minutes }
}

#[test]
fn behind_counts_only_the_part_of_a_block_that_has_passed() {
    let blocks = [block(540, 60), block(840, 60), Placement::Marker { at: 600, due: true }];
    assert_eq!(behind_minutes(&blocks, 500), 0);
    assert_eq!(behind_minutes(&blocks, 570), 30);
    assert_eq!(behind_minutes(&blocks, 600), 60);
    assert_eq!(behind_minutes(&blocks, 900), 120);
    assert_eq!(behind_minutes(&[], 900), 0);
    // Overlapping blocks count once, like the planned figure they sit beside.
    assert_eq!(behind_minutes(&[block(540, 60), block(540, 60), block(570, 60)], 720), 90);
}

#[test]
fn summary_text_says_only_what_there_is() {
    let none = DaySummary::default();
    assert_eq!(summary_text(none, none, none), "nothing on this day");
    let planned = DaySummary { planned_minutes: 150, blocks: 3, due: 1 };
    let routine = DaySummary { planned_minutes: 540, blocks: 2, due: 0 };
    let done = DaySummary { planned_minutes: 60, blocks: 1, due: 0 };
    assert_eq!(summary_text(planned, routine, done), "2h 30m planned · 3 blocks · 1 due · 9h routine · 1h done");
    // A day with only routines on it does not claim "0m planned".
    assert_eq!(summary_text(none, routine, none), "9h routine");
    // But a day with a due date and nothing planned says so.
    let due_only = DaySummary { planned_minutes: 0, blocks: 0, due: 2 };
    assert_eq!(summary_text(due_only, none, none), "0m planned · 2 due");
    let one = DaySummary { planned_minutes: 30, blocks: 1, due: 0 };
    assert_eq!(summary_text(one, none, none), "30m planned · 1 block");
}

#[test]
fn reflow_slides_work_past_now_in_order_and_around_anchors() {
    // A and B are behind at 10:20; C is still ahead but gets pushed on by
    // them. The dentist at 12:00–13:00 is an anchor nothing may land in.
    let blocks = [flowable(1, 540, 60), flowable(2, 600, 30), flowable(3, 900, 60)];
    let moved = reflow(&blocks, &[(720, 780)], 620);
    // Now rounds up to the grid (10:30); A starts there, B follows; C at
    // 15:00 is ahead of both and stays exactly where it was put.
    assert_eq!(moved, vec![(1, 630), (2, 690), (3, 900)]);
    // A now that is already on the grid is used as it is.
    let moved = reflow(&blocks, &[(720, 780)], 615);
    assert_eq!(moved, vec![(1, 615), (2, 675), (3, 900)]);
    // Pushed on when the ones in front reach it: B grows to two hours, so
    // it ends at 12:30 inside the dentist... and C moves to after it.
    let long = [flowable(1, 540, 60), flowable(2, 600, 120), flowable(3, 780, 60)];
    assert_eq!(reflow(&long, &[(720, 780)], 620), vec![(1, 630), (2, 780), (3, 900)]);
}

#[test]
fn reflow_moves_nothing_when_nothing_is_behind() {
    // The R key is safe to press on reflex: a day still ahead is left alone.
    let blocks = [flowable(1, 840, 60), flowable(2, 960, 30)];
    assert_eq!(reflow(&blocks, &[(720, 780)], 600), vec![(1, 840), (2, 960)]);
    assert_eq!(reflow(&blocks, &[], 0), vec![(1, 840), (2, 960)]);
}

#[test]
fn reflow_keeps_input_order_in_its_answer_and_steps_over_stacked_anchors() {
    // Given out of time order; answered in the order given.
    let blocks = [flowable(9, 700, 60), flowable(4, 500, 60)];
    // Two overlapping anchors: stepping past the first lands in the
    // second. The second ends off the grid (13:20), so the block that
    // follows it lands on the next grid step (13:30), not at 13:20.
    let moved = reflow(&blocks, &[(600, 720), (700, 800)], 600);
    assert_eq!(moved, vec![(9, 870), (4, 810)]);
}

#[test]
fn reflow_clamps_at_the_end_of_the_day_rather_than_dropping_anything() {
    let blocks = [flowable(1, 1380, 60), flowable(2, 1400, 60)];
    let moved = reflow(&blocks, &[], 1400);
    // Both end up in the last hour, overlapping — an over-booked day looks
    // over-booked.
    assert_eq!(moved, vec![(1, 1380), (2, 1380)]);
}

#[test]
fn reflow_ignores_empty_anchors_and_respects_the_minimum_length() {
    let blocks = [flowable(1, 60, 5)];
    let moved = reflow(&blocks, &[(100, 100)], 90);
    assert_eq!(moved, vec![(1, 90)]);
    // Today's private `planner::snap_up` is `time::snap_up` (core.md §5.5).
    assert_eq!(time::snap_up(0), 0);
    assert_eq!(time::snap_up(1), 15);
    assert_eq!(time::snap_up(DAY_MINUTES + 30), DAY_MINUTES);
}

fn day() -> chrono::NaiveDate {
    date(2026, 8, 14)
}

const ZERO: Stamp = Stamp { seq: 0, device_no: 0 };
const STAMPS: ItemStamps = ItemStamps { estimate: ZERO, levels: ZERO, name: ZERO, repeat: ZERO, time: ZERO };

/// A task with planned `blocks`, a `deadline`, and an estimate.
fn task(blocks: Vec<Block>, deadline: Option<Instant>, minutes: Option<u32>) -> Item {
    Item {
        id: 1,
        kind: Kind::Task,
        name: "t".into(),
        created: at(2026, 8, 1, 9, 0),
        deadline,
        importance: Some(2),
        time_importance: None,
        duration_minutes: minutes,
        recurrence: None,
        blocks,
        stamps: STAMPS,
    }
}

/// Today's `Session { start, minutes }`, with the sid that replaces its index.
fn session(sid: u64, start: Instant, minutes: u32) -> Block {
    Block { sid, start, minutes, stamp: ZERO }
}

fn event(deadline: Instant, minutes: Option<u32>) -> Item {
    Item {
        id: 2,
        kind: Kind::Event,
        name: "e".into(),
        created: at(2026, 8, 1, 9, 0),
        deadline: Some(deadline),
        importance: None,
        time_importance: None,
        duration_minutes: minutes,
        recurrence: None,
        blocks: Vec::new(),
        stamps: STAMPS,
    }
}

fn routine(days: u8, start_minutes: i32, minutes: u32) -> Item {
    Item {
        kind: Kind::Routine,
        recurrence: Some(Recurrence { days, anchor: day(), start_minutes, minutes }),
        ..task(Vec::new(), None, None)
    }
}

#[test]
fn a_routine_places_a_block_on_every_day_its_rule_names() {
    // `day()` is Friday 14 Aug 2026.
    let friday = day();
    let saturday = friday.succ_opt().unwrap_or_default();
    let sleep = routine(crate::tasks::EVERY_DAY, 23 * 60, 8 * 60);

    for on in [friday, saturday] {
        let placed = placements_for(&sleep, on, &ZONE);
        assert_eq!(placed.len(), 1, "one block a day, generated from the rule");
        assert_eq!(placed[0].placement, Placement::Block { start: 23 * 60, minutes: 8 * 60 });
        // Not a stored block: there is nothing to address, so a gesture aims
        // at the rule itself.
        assert_eq!(placed[0].session, None);
        assert!(appears_on(&sleep, on, &ZONE));
    }

    let weekdays_only = routine(0b0001_1111, 8 * 60, 30);
    assert!(placements_for(&weekdays_only, friday, &ZONE).len() == 1);
    assert!(placements_for(&weekdays_only, saturday, &ZONE).is_empty());
    assert!(!appears_on(&weekdays_only, saturday, &ZONE));
}

#[test]
fn a_routine_ignores_a_deadline_and_sessions_it_should_not_have() {
    // Not a shape the UI makes, but a legacy routine can carry them (core.md
    // §6.2). The rule is what the user set here, so it answers alone rather
    // than the item showing up twice on the day it was hand-given.
    let confused = Item {
        kind: Kind::Routine,
        recurrence: Some(Recurrence { days: crate::tasks::EVERY_DAY, anchor: day(), start_minutes: 60, minutes: 30 }),
        deadline: Some(at(2026, 8, 14, 9, 0)),
        blocks: vec![session(1, at(2026, 8, 14, 15, 0), 60)],
        ..task(Vec::new(), None, None)
    };
    let placed = placements_for(&confused, day(), &ZONE);
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].placement, Placement::Block { start: 60, minutes: 30 });
}

#[test]
fn a_routine_block_is_never_shorter_than_a_block_can_be() {
    let sliver = routine(crate::tasks::EVERY_DAY, 0, 1);
    assert_eq!(placements_for(&sliver, day(), &ZONE)[0].placement, Placement::Block { start: 0, minutes: MIN_BLOCK_MINUTES });
}

#[test]
fn placements_distinguish_planned_work_from_a_due_date() {
    let planned_day = day();
    let due_day = date(2026, 8, 16);

    // Worked on Friday 09:00 for an hour, due Sunday 17:00.
    let item = task(vec![session(1, at(2026, 8, 14, 9, 0), 60)], Some(at(2026, 8, 16, 17, 0)), Some(60));

    // Today's `session: Some(0)` (an index) is the block's sid, 1 (core.md §14.2).
    assert_eq!(
        placements_for(&item, planned_day, &ZONE),
        vec![DayPlacement { session: Some(1), placement: Placement::Block { start: 9 * 60, minutes: 60 } }],
        "the planned day shows the work block"
    );
    assert_eq!(
        placements_for(&item, due_day, &ZONE),
        vec![DayPlacement { session: None, placement: Placement::Marker { at: 17 * 60, due: true } }],
        "the due day still shows the deadline"
    );
    assert_eq!(placements_for(&item, date(2026, 8, 15), &ZONE), vec![], "an unrelated day shows nothing");
}

#[test]
fn a_task_split_across_a_day_shows_every_session_and_its_deadline() {
    // Two blocks on the same day the task is due: 09:00–10:00,
    // 14:00–15:30, owed at 17:00. All three appear — the due marker is not
    // suppressed by the work blocks, because seeing the deadline next to
    // the work is the point of showing a day whole.
    let item = task(
        vec![session(1, at(2026, 8, 14, 9, 0), 60), session(2, at(2026, 8, 14, 14, 0), 90)],
        Some(at(2026, 8, 14, 17, 0)),
        Some(150),
    );

    let placements = placements_for(&item, day(), &ZONE);
    assert_eq!(placements.len(), 3, "{placements:?}");
    // Today's indices 0 and 1 are the sids 1 and 2, in ascending sid.
    assert_eq!(placements[0].session, Some(1));
    assert_eq!(placements[1].session, Some(2));
    assert_eq!(placements[1].placement, Placement::Block { start: 14 * 60, minutes: 90 });
    assert_eq!(placements[2], DayPlacement { session: None, placement: Placement::Marker { at: 17 * 60, due: true } });

    // A block on another day stays on that day.
    assert_eq!(placements_for(&item, date(2026, 8, 13), &ZONE), vec![]);
}

#[test]
fn events_are_blocks_once_they_have_a_length() {
    let without = event(at(2026, 8, 14, 14, 30), None);
    assert_eq!(
        placements_for(&without, day(), &ZONE),
        vec![DayPlacement { session: None, placement: Placement::Marker { at: 14 * 60 + 30, due: false } }]
    );

    let with = event(at(2026, 8, 14, 14, 30), Some(45));
    assert_eq!(
        placements_for(&with, day(), &ZONE),
        vec![DayPlacement { session: None, placement: Placement::Block { start: 14 * 60 + 30, minutes: 45 } }]
    );
}

#[test]
fn appears_on_agrees_with_placements_for() {
    let day = day();
    let elsewhere = date(2026, 8, 20);

    // Every shape, checked against the list it summarises: a bare task, one
    // with only a block here, one with only its deadline here, and an event.
    let cases = [
        task(vec![], None, None),
        task(vec![session(1, at(2026, 8, 14, 9, 0), 60)], None, Some(60)),
        task(vec![], Some(at(2026, 8, 14, 17, 0)), None),
        task(vec![session(1, at(2026, 8, 20, 9, 0), 60)], Some(at(2026, 8, 14, 17, 0)), Some(60)),
        event(at(2026, 8, 14, 14, 0), Some(45)),
        event(at(2026, 8, 20, 14, 0), Some(45)),
    ];
    for item in &cases {
        for probe in [day, elsewhere] {
            assert_eq!(
                appears_on(item, probe, &ZONE),
                !placements_for(item, probe, &ZONE).is_empty(),
                "{} on {probe}",
                item.name
            );
        }
    }
}

#[test]
fn unplanned_deadlineless_task_has_no_placement() {
    assert_eq!(placements_for(&task(vec![], None, None), day(), &ZONE), vec![]);
}

#[test]
fn lay_out_gives_disjoint_blocks_the_full_width() {
    let placements = [Placement::Block { start: 540, minutes: 60 }, Placement::Block { start: 660, minutes: 60 }];
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
    let placements = [Placement::Block { start: 540, minutes: 60 }, Placement::Block { start: 780, minutes: 30 }];
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
    let half_planned = task(vec![session(1, at(2026, 8, 13, 9, 0), 60)], None, Some(120));
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

    assert_eq!(backlog_group(Some(due_today), day, &ZONE), BacklogGroup::Due);
    // Still unplanned two days after it was owed: it wants a slot today.
    assert_eq!(backlog_group(Some(overdue), day, &ZONE), BacklogGroup::Due);
    assert_eq!(backlog_group(Some(later), day, &ZONE), BacklogGroup::Later);
    // Nothing owed at all is ordinary backlog, not a call to action.
    assert_eq!(backlog_group(None, day, &ZONE), BacklogGroup::Later);
}

#[test]
fn relative_due_reads_as_a_countdown() {
    let now = at(2026, 8, 14, 12, 0);
    assert_eq!(relative_due(at(2026, 8, 17, 12, 0), now), "3d");
    assert_eq!(relative_due(at(2026, 8, 14, 15, 0), now), "3h");
    assert_eq!(relative_due(at(2026, 8, 14, 12, 30), now), "30m");
    assert_eq!(relative_due(at(2026, 8, 14, 11, 0), now), "overdue");
}

// ---------------------------------------------------------------- zones (core.md §5.3, §14)

/// A UTC instant.
fn utc(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> Instant {
    let minutes = i32::try_from(hour * 60 + minute).unwrap_or(0);
    time::resolve_on_day(date(y, m, d), minutes, &FixedZone(0)).unwrap_or(Instant::MIN)
}

/// Central Europe in 2026: +01:00, +02:00 from 29 March 01:00 UTC (spring
/// forward), +01:00 again from 25 October 01:00 UTC (fall back).
fn europe() -> TableZone {
    let spring = utc(2026, 3, 29, 1, 0).as_unix_ms();
    let fall = utc(2026, 10, 25, 1, 0).as_unix_ms();
    TableZone::new(3600, vec![(spring, 7200), (fall, 3600)]).unwrap_or(TableZone { initial: 0, transitions: Vec::new() })
}

// spec: CORE-TIME-002
// spec: CORE-READ-001
#[test]
fn placements_take_the_local_day_and_minutes_from_the_zone() {
    let zone = europe();
    let utc_zone = FixedZone(0);
    // Two blocks at 22:30 UTC: on 28 March (+01:00, so 23:30 that day) and
    // on 29 March, after the spring-forward (+02:00, so 00:30 on 30 March).
    let item = task(vec![session(1, utc(2026, 3, 28, 22, 30), 60), session(2, utc(2026, 3, 29, 22, 30), 60)], None, None);

    let on = |d: chrono::NaiveDate, z: &dyn Zone| placements_for(&item, d, z);
    assert_eq!(on(date(2026, 3, 28), &zone), vec![DayPlacement { session: Some(1), placement: block(23 * 60 + 30, 60) }]);
    assert_eq!(on(date(2026, 3, 29), &zone), vec![]);
    assert_eq!(on(date(2026, 3, 30), &zone), vec![DayPlacement { session: Some(2), placement: block(30, 60) }]);
    // In UTC the same blocks sit at 22:30 on the 28th and the 29th.
    assert_eq!(on(date(2026, 3, 29), &utc_zone), vec![DayPlacement { session: Some(2), placement: block(22 * 60 + 30, 60) }]);
    assert!(!appears_on(&item, date(2026, 3, 29), &zone));
    assert!(appears_on(&item, date(2026, 3, 29), &utc_zone));

    // The fall-back fold: 00:30 UTC (+02:00) and 01:30 UTC (+01:00) are both
    // 02:30 local on 25 October.
    for t in [utc(2026, 10, 25, 0, 30), utc(2026, 10, 25, 1, 30)] {
        assert_eq!(
            placements_for(&event(t, None), date(2026, 10, 25), &zone),
            vec![DayPlacement { session: None, placement: Placement::Marker { at: 150, due: false } }]
        );
    }

    // A deadline at 22:30 UTC on 29 March is owed on the 30th locally.
    let deadline = Some(utc(2026, 3, 29, 22, 30));
    assert_eq!(backlog_group(deadline, date(2026, 3, 29), &zone), BacklogGroup::Later);
    assert_eq!(backlog_group(deadline, date(2026, 3, 29), &utc_zone), BacklogGroup::Due);
}

// spec: CORE-READ-002
// spec: CORE-READ-005
#[test]
fn the_planner_queries_leave_the_board_as_it_was() {
    let mut board = BoardData::empty();
    board.items = vec![
        task(vec![session(3, at(2026, 8, 14, 14, 0), 60), session(4, at(2026, 8, 14, 9, 0), 30)], Some(at(2026, 8, 14, 17, 0)), Some(120)),
        Item { id: 5, ..event(at(2026, 8, 14, 10, 0), Some(45)) },
        Item { id: 7, ..routine(crate::tasks::EVERY_DAY, 23 * 60, 8 * 60) },
    ];
    let before = board.clone();
    let zone = europe();

    let mut placements = Vec::new();
    for item in &board.items {
        placements.extend(placements_for(item, day(), &zone).into_iter().map(|p| p.placement));
        let _ = appears_on(item, day(), &zone);
        let _ = drop_length_for(item);
        let _ = backlog_group(item.deadline, day(), &zone);
    }
    let lanes = lay_out(&placements);
    let _ = summary_text(summarize(&placements), DaySummary::default(), DaySummary::default());
    let _ = behind_minutes(&placements, 12 * 60);
    let flow: Vec<Flowable<usize>> =
        placements.iter().enumerate().map(|(key, p)| Flowable { key, start: p.start(), minutes: 30 }).collect();
    let moved = reflow(&flow, &[], 12 * 60);

    assert_eq!(board, before, "no read query changes the board");
    // Each answers in a list of its own, parallel to or in the order of its input.
    assert_eq!(lanes.len(), placements.len());
    assert_eq!(moved.iter().map(|&(k, _)| k).collect::<Vec<_>>(), (0..flow.len()).collect::<Vec<_>>());
    // The blocks keep their stored order; the placements follow it (ascending sid).
    assert_eq!(placements_for(&board.items[0], day(), &zone).iter().map(|p| p.session).collect::<Vec<_>>(), vec![Some(3), Some(4), None]);
}
