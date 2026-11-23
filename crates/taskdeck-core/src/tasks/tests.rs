//! Today's scoring and rule tests (tasks.rs:780-1511), ported, and the zone and
//! read-query clauses of core.md §5.3 and §14.
//!
//! Not ported, because what they test is not: `Recurrence::toggle` (stays in
//! the UI), the serde shape of a legacy rule, `migrate_legacy_plans`,
//! `bucket_by_deadline_day`, `assign_missing_ids` and the file quarantine.

use std::collections::BTreeSet;

use super::*;
use crate::board::{Block, BoardData, ItemStamps, Stamp};
use crate::time::{FixedZone, TableZone, Zone};

/// Today's tests ran in `Local`, the zone of the process that ran
/// `cargo test`; the recorded data was made at +03:00. A fixed zone at +03:00
/// stands in for it here. Scoring reads only differences of instants, so the
/// zone only decides which instants the test's wall times name.
const ZONE: FixedZone = FixedZone(3 * 3600);

fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default()
}

fn local(y: i32, m: u32, d: u32, hour: u32, minute: u32, zone: &dyn Zone) -> Instant {
    let minutes = i32::try_from(hour * 60 + minute).unwrap_or(0);
    time::resolve_on_day(date(y, m, d), minutes, zone).unwrap_or(Instant::MIN)
}

/// `Local.with_ymd_and_hms(y, m, d, hour, minute, 0)` of today's tests, in `ZONE`.
fn at(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> Instant {
    local(y, m, d, hour, minute, &ZONE)
}

/// `t + minutes` (today's `t + Duration::minutes(minutes)`).
fn shift(t: Instant, minutes: i64) -> Instant {
    t.plus_seconds(minutes * 60).unwrap_or(Instant::MIN)
}

const ZERO: Stamp = Stamp { seq: 0, device_no: 0 };
const STAMPS: ItemStamps = ItemStamps { estimate: ZERO, levels: ZERO, name: ZERO, repeat: ZERO, time: ZERO };

fn active(importance: Option<u8>, time_importance: Option<u8>, is_event: bool, deadline: Option<Instant>) -> Item {
    Item {
        id: 0,
        kind: if is_event { Kind::Event } else { Kind::Task },
        name: "test".to_string(),
        created: at(2025, 1, 1, 0, 0),
        deadline,
        importance,
        time_importance,
        duration_minutes: None,
        recurrence: None,
        blocks: Vec::new(),
        stamps: STAMPS,
    }
}

/// Today's `Session { start, minutes }`, with a sid.
fn session(sid: u64, start: Instant, minutes: u32) -> Block {
    Block { sid, start, minutes, stamp: ZERO }
}

fn a_friday() -> chrono::NaiveDate {
    date(2026, 8, 14)
}

/// A routine: sleep, 23:00, eight hours, on the given days.
fn routine(days: u8) -> Item {
    Item {
        kind: Kind::Routine,
        recurrence: Some(Recurrence { days, anchor: a_friday(), start_minutes: 23 * 60, minutes: 8 * 60 }),
        ..active(None, None, false, None)
    }
}

#[test]
fn a_routine_is_neither_ranked_nor_waiting_to_be_planned() {
    let now = noon();
    let sleep = routine(EVERY_DAY);

    // The shape a routine has — no deadline, no importance, no horizon — is
    // exactly what the scorer calls malformed and shoves to the top of the
    // list. It is not work you owe, so it scores nothing at all.
    assert_eq!(sleep.base_score(now), 0.0);
    assert!(sleep.base_score(now) < MALFORMED_SCORE);

    // And it is not waiting for a slot: it is already at one, every week.
    assert!(!sleep.wants_planning(), "a routine must never reach the tray");
    assert!(sleep.is_planned());
    assert!(sleep.is_routine());

    // Nothing is owed, so nothing goes on the calendar grid. (Today's
    // `bucket_by_deadline_day` line is not ported.)
    assert_eq!(sleep.planner_anchor(), None);
}

#[test]
fn a_rule_fires_on_the_days_it_names() {
    // 14 Aug 2026 is a Friday.
    let friday = a_friday();
    let saturday = friday.succ_opt().unwrap_or_default();

    let weekdays = Recurrence { days: WEEKDAYS, anchor: friday, start_minutes: 8 * 60, minutes: 30 };
    assert!(weekdays.falls_on(friday));
    assert!(!weekdays.falls_on(saturday));

    let daily = Recurrence { days: EVERY_DAY, ..weekdays };
    assert!(daily.falls_on(friday) && daily.falls_on(saturday));
}

#[test]
fn no_days_is_a_one_off_not_a_broken_rule() {
    // "Tomorrow at two I walk the dog" is the same *kind* of thing as
    // sleeping every night — time spoken for, owed to nobody — and it happens
    // once. Repeating is a property of this kind, not its definition.
    let friday = a_friday();
    let once = Recurrence { days: 0, anchor: friday, start_minutes: 14 * 60, minutes: 30 };

    assert!(!once.repeats());
    assert!(once.falls_on(friday));
    assert!(!once.falls_on(friday.succ_opt().unwrap_or_default()));
    assert!(!once.falls_on(friday - chrono::TimeDelta::days(7)), "not the same weekday a week back");
}

#[test]
fn a_rule_says_what_it_amounts_to() {
    let at = |days| Recurrence { days, anchor: a_friday(), start_minutes: 0, minutes: 60 };
    assert_eq!(at(EVERY_DAY).summary(), "every day");
    assert_eq!(at(WEEKDAYS).summary(), "weekdays");
    assert_eq!(at(0b0110_0000).summary(), "weekends");
    assert_eq!(at(0b0001_0101).summary(), "Mon · Wed · Fri");
    assert_eq!(at(0).summary(), "just this day");
    // The archive has no "this day" to point at, so it names the date.
    assert_eq!(at(0).summary_dated(), "just 14 Aug 2026");
    assert_eq!(at(EVERY_DAY).summary_dated(), "every day");
    assert!(at(0b0001_0101).includes(2) && !at(0b0001_0101).includes(1));
    assert_eq!(weekday_bit(a_friday()), 0b0001_0000);
}

#[test]
fn calendar_item_color_mapping() {
    let dl = Some(at(2025, 6, 1, 12, 0));
    // Events always map to palette index 5, regardless of importance.
    assert_eq!(active(None, None, true, dl).calendar_item_color(), 5);
    // Deadline tasks map by importance.
    assert_eq!(active(Some(3), None, false, dl).calendar_item_color(), 3);
    // Horizon tasks map by time_importance.
    assert_eq!(active(None, Some(2), false, None).calendar_item_color(), 2);
    // "Whenever" is index 3 for save-compatibility but wears the calmest
    // colour, not the highly-important one.
    assert_eq!(active(None, Some(HORIZON_WHENEVER), false, None).calendar_item_color(), 0);
    // Nothing set falls back to 0.
    assert_eq!(active(None, None, false, None).calendar_item_color(), 0);
}

#[test]
fn a_palette_index_is_always_inside_the_palette() {
    let dl = Some(at(2025, 6, 1, 12, 0));
    // The UI writes 0–4, but this is a `u8` from outside and the calendar
    // indexes a six-entry array with it. An out-of-range value used to panic
    // on the next frame that drew the day.
    for importance in 0..=u8::MAX {
        let index = active(Some(importance), None, false, dl).calendar_item_color();
        assert!(index < PALETTE_LEN, "importance {importance} gave index {index}");
    }
    for horizon in 0..=u8::MAX {
        let index = active(None, Some(horizon), false, None).calendar_item_color();
        assert!(index < PALETTE_LEN, "horizon {horizon} gave index {index}");
    }
}

/// A dated task, `days` from its deadline (negative = overdue).
fn dated(importance: u8, days: f64, now: Instant) -> Item {
    Item { deadline: Some(shift(now, (days * 1440.0) as i64)), ..active(Some(importance), None, false, None) }
}

/// An undated task of the given horizon, created `age` days ago.
fn undated(horizon: u8, age: f64, now: Instant) -> Item {
    Item { created: shift(now, -((age * 1440.0) as i64)), ..active(None, Some(horizon), false, None) }
}

/// A task with one planned block starting at `start`.
fn planned(importance: Option<u8>, start: Instant, minutes: u32) -> Item {
    Item { blocks: vec![session(1, start, minutes)], ..active(importance, None, false, None) }
}

fn noon() -> Instant {
    at(2026, 8, 14, 12, 0)
}

#[test]
fn score_rises_as_a_deadline_approaches() {
    let now = noon();
    for importance in 0..=4 {
        let far = dated(importance, 30.0, now).base_score(now);
        let near = dated(importance, 1.0, now).base_score(now);
        let due = dated(importance, 0.0, now).base_score(now);
        let late = dated(importance, -2.0, now).base_score(now);

        assert!(far < near, "importance {importance}: {far} !< {near}");
        assert!(near < due, "importance {importance}: {near} !< {due}");
        assert!(due < late, "importance {importance}: {due} !< {late}");
    }
}

#[test]
fn pressure_is_exactly_one_at_the_deadline() {
    let now = noon();
    // At the deadline a task is worth precisely its weight, which is what
    // makes weights comparable across the two models.
    for importance in 0..=4u8 {
        let score = dated(importance, 0.0, now).base_score(now);
        assert!((score - WEIGHT_BY_IMPORTANCE[importance as usize]).abs() < 0.001, "importance {importance} scored {score}");
    }
}

#[test]
fn importance_separates_tasks_sharing_a_deadline() {
    let now = noon();
    let scores: Vec<f32> = (0..=4).map(|i| dated(i, 3.0, now).base_score(now)).collect();
    for pair in scores.windows(2) {
        assert!(pair[0] < pair[1], "importance should break the tie: {scores:?}");
    }
}

#[test]
fn importance_stops_out_arguing_urgency_eventually() {
    let now = noon();
    // Importance wins near-term: a lethally important task a fortnight out
    // is above a trivial one due today.
    let trivial_due_now = dated(0, 0.0, now).base_score(now);
    assert!(dated(4, 14.0, now).base_score(now) > trivial_due_now);

    // But its reach is bounded. Far enough out, the thing actually due today
    // takes the top.
    assert!(dated(4, 45.0, now).base_score(now) < trivial_due_now);
}

#[test]
fn a_distant_deadline_still_ranks_by_importance() {
    let now = noon();
    assert!(dated(4, 30.0, now).base_score(now) > dated(0, 30.0, now).base_score(now));
}

#[test]
fn overdue_pressure_levels_off_at_the_cap() {
    let now = noon();
    for importance in 0..=4u8 {
        let capped = WEIGHT_BY_IMPORTANCE[importance as usize] * OVERDUE_PRESSURE_CAP;
        let a_week = dated(importance, -7.0, now).base_score(now);
        let a_year = dated(importance, -365.0, now).base_score(now);
        assert!((a_week - capped).abs() < 0.001, "{a_week} != {capped}");
        assert_eq!(a_week, a_year, "lateness past the cap should stop counting");
    }
}

#[test]
fn a_late_trivial_task_does_not_bury_important_upcoming_work() {
    let now = noon();
    let maximally_late_trivial = dated(0, -365.0, now).base_score(now);
    let important_upcoming = dated(4, 14.0, now).base_score(now);
    assert!(important_upcoming > maximally_late_trivial, "{important_upcoming} should beat {maximally_late_trivial}");
}

#[test]
fn undated_tasks_ripen_towards_their_weight_and_stop() {
    let now = noon();
    for horizon in 0..=3u8 {
        let weight = WEIGHT_BY_HORIZON[horizon as usize];
        let fresh = undated(horizon, 0.0, now).base_score(now);
        let ripening = undated(horizon, f64::from(RIPEN_DAYS_BY_HORIZON[horizon as usize]), now).base_score(now);
        let ancient = undated(horizon, 3650.0, now).base_score(now);

        assert!(fresh < ripening && ripening < ancient, "horizon {horizon} should ripen");
        // Half weight at the ripening time, by construction.
        assert!((ripening - weight / 2.0).abs() < 0.01, "{ripening} != half of {weight}");
        // And never past its weight, however long it sits.
        assert!(ancient <= weight, "{ancient} exceeded its weight {weight}");
    }
}

#[test]
fn a_ripe_undated_task_never_outranks_comparable_overdue_work() {
    let now = noon();
    let ripest = undated(2, 3650.0, now).base_score(now);
    let ripest_weight = WEIGHT_BY_HORIZON[2];
    for importance in 0..=4u8 {
        if WEIGHT_BY_IMPORTANCE[importance as usize] < ripest_weight {
            continue;
        }
        let overdue = dated(importance, -1.0, now).base_score(now);
        assert!(overdue > ripest, "overdue {overdue} should beat ripe {ripest}");
    }
}

#[test]
fn scores_stay_finite_at_absurd_distances() {
    let now = noon();
    // Far-future deadlines used to overflow to +inf. Now they underflow
    // towards zero instead, which orders correctly and can't produce NaN.
    //
    // Today's "late" case put the deadline 200 years before 2026, before the
    // instant range of core.md §5.1. The score reads only `deadline − now`, so
    // the same 200 years overdue is a deadline at 2026 scored from 2226.
    let later = shift(now, 365 * 200 * 1440);
    for importance in 0..=4 {
        let far = dated(importance, 365.0 * 200.0, now).base_score(now);
        let late = dated(importance, 0.0, now).base_score(later);
        assert!(far.is_finite() && far >= 0.0, "far score was {far}");
        assert!(late.is_finite(), "late score was {late}");
    }
}

#[test]
fn a_planned_task_rises_as_its_slot_approaches() {
    let now = noon();
    // A task dragged out on the planner gets a block but no deadline — a plan
    // is not a due date. Without the planned term it scored purely on age.
    let planned_at = |hours_ahead: i64| planned(Some(2), shift(now, hours_ahead * 60), 60);

    let tomorrow = planned_at(24).base_score(now);
    let this_evening = planned_at(6).base_score(now);
    let imminent = planned_at(1).base_score(now);

    assert!(tomorrow < this_evening, "{tomorrow} !< {this_evening}");
    assert!(this_evening < imminent, "{this_evening} !< {imminent}");
}

#[test]
fn a_slipped_plan_does_not_escalate_like_a_missed_deadline() {
    let now = noon();
    let slipped = |days_ago: i64| planned(Some(2), shift(now, -days_ago * 1440), 60);

    // A plan you didn't keep tops out at the task's weight and stays there.
    // A missed *deadline* keeps climbing past it.
    let weight = WEIGHT_BY_IMPORTANCE[2];
    assert!((slipped(1).base_score(now) - weight).abs() < 0.001);
    assert_eq!(slipped(1).base_score(now), slipped(100).base_score(now));
    assert!(dated(2, -1.0, now).base_score(now) > slipped(100).base_score(now));
}

#[test]
fn planning_a_dated_task_lifts_it_on_the_day() {
    let now = noon();
    // Due in three days, and set aside for right now. The plan is the more
    // pressing of the two reasons, so it wins.
    let due_friday = dated(3, 3.0, now);
    let due_friday_planned_now = Item { blocks: vec![session(1, shift(now, 30), 60)], ..due_friday.clone() };
    assert!(due_friday_planned_now.base_score(now) > due_friday.base_score(now), "planning a task for now should lift it");

    // But a plan never *lowers* a task: the deadline still applies if it is
    // the more pressing of the two.
    let due_today_planned_next_week = Item { blocks: vec![session(1, shift(now, 7 * 1440), 60)], ..dated(3, 0.0, now) };
    assert!((due_today_planned_next_week.base_score(now) - dated(3, 0.0, now).base_score(now)).abs() < 0.001);
}

#[test]
fn a_planned_task_with_no_importance_is_still_scored() {
    let now = noon();
    let bare = planned(None, now, 60);
    let score = bare.base_score(now);
    assert!((score - WEIGHT_BY_IMPORTANCE[ASSUMED_IMPORTANCE as usize]).abs() < 0.001);
    assert!(score < MALFORMED_SCORE);
}

#[test]
fn a_dated_task_missing_its_importance_is_scored_as_middling() {
    let now = noon();
    let no_importance = Item { deadline: Some(now), ..active(None, None, false, None) };
    let middling = dated(ASSUMED_IMPORTANCE, 0.0, now).base_score(now);
    assert!((no_importance.base_score(now) - middling).abs() < 0.001);
}

#[test]
fn the_most_pressing_session_drives_planned_pressure() {
    let now = noon();
    let near_only = planned(Some(2), shift(now, 2 * 60), 60);
    let both = Item {
        blocks: vec![session(1, shift(now, 2 * 60), 60), session(2, shift(now, 7 * 1440), 60)],
        ..active(Some(2), None, false, None)
    };
    assert!((both.base_score(now) - near_only.base_score(now)).abs() < 0.001);
}

#[test]
fn whenever_surfaces_eventually_but_out_argues_nothing() {
    let now = noon();
    let fresh = undated(HORIZON_WHENEVER, 0.0, now).base_score(now);
    let old = undated(HORIZON_WHENEVER, 365.0, now).base_score(now);
    assert!(fresh < old, "{fresh} !< {old}");
    let month = undated(0, 365.0, now).base_score(now);
    assert!(old < month, "{old} !< {month}");
    assert!(old < dated(0, 0.0, now).base_score(now));
}

#[test]
fn sessions_report_planned_and_remaining_time() {
    let now = noon();
    let mut task = active(Some(2), None, false, None);
    task.duration_minutes = Some(120);
    assert_eq!(task.planned_minutes(), 0);
    assert_eq!(task.remaining_minutes(), Some(120));
    assert!(task.wants_planning(), "an unplanned estimated task is a tray card");

    task.blocks.push(session(1, now, 45));
    assert_eq!(task.planned_minutes(), 45);
    assert_eq!(task.remaining_minutes(), Some(75));
    assert!(task.wants_planning(), "45m of a 2h estimate planned: still half a card");

    task.blocks.push(session(2, shift(now, 1440), 90));
    assert_eq!(task.remaining_minutes(), Some(0), "over-planning clamps to zero");
    assert!(!task.wants_planning(), "estimate covered: the card is done");

    // No estimate: one block is enough to leave the tray.
    let mut unsized_task = active(Some(2), None, false, None);
    assert!(unsized_task.wants_planning());
    unsized_task.blocks.push(session(1, now, 30));
    assert!(!unsized_task.wants_planning());
    assert_eq!(unsized_task.remaining_minutes(), None);
    // The earliest block anchors it on the planner.
    assert_eq!(task.planner_anchor(), Some(now));
    assert!(task.is_planned());
}

#[test]
fn an_item_with_nothing_to_go_on_is_surfaced_at_the_top() {
    let now = noon();
    let nothing = active(None, None, false, None).base_score(now);
    let highest_real = WEIGHT_BY_IMPORTANCE[4] * OVERDUE_PRESSURE_CAP;
    assert!(nothing > highest_real, "{nothing} should stand out from {highest_real}");
}

#[test]
fn an_event_closer_in_time_scores_higher() {
    let now = noon();
    let soon = active(None, None, true, Some(shift(now, 1440)));
    let later = active(None, None, true, Some(shift(now, 10 * 1440)));
    assert!(soon.importance_score(now, 0) > later.importance_score(now, 0), "nearer event should score higher");
}

#[test]
fn jitter_is_small_bounded_and_actually_varies_per_task() {
    let factors: Vec<f32> =
        (1..=64u64).map(|id| Item { id, ..active(Some(2), None, false, None) }.tie_break_jitter(7)).collect();

    for factor in &factors {
        assert!((1.0..1.0 + JITTER).contains(factor), "factor {factor} out of range");
    }

    // Distinct ids must give distinct factors.
    let distinct = factors.iter().map(|f| f.to_bits()).collect::<BTreeSet<_>>().len();
    assert!(distinct > 60, "only {distinct} distinct factors out of 64");
}

#[test]
fn jitter_holds_all_day_and_turns_over_at_midnight() {
    let task = Item { id: 7, ..active(Some(2), None, false, None) };
    // The seed is the *day*, so the order is fixed for as long as anyone is
    // looking at it, and moves once, tomorrow.
    let today = 739_000;
    assert_eq!(task.tie_break_jitter(today), task.tie_break_jitter(today));
    assert_ne!(task.tie_break_jitter(today), task.tie_break_jitter(today + 1));
}

#[test]
fn a_days_worth_of_edits_cannot_reorder_the_list() {
    let now = noon();
    let tasks: Vec<Item> = (1..=3).map(|id| Item { id, ..dated(2, 5.0, now) }).collect();

    let order_at = |seed: u64| {
        let mut scored: Vec<(f32, u64)> = tasks.iter().map(|t| (t.importance_score(now, seed), t.id)).collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.into_iter().map(|(_, id)| id).collect::<Vec<_>>()
    };

    let settled = order_at(739_000);
    for _ in 0..50 {
        assert_eq!(order_at(739_000), settled);
    }
}

#[test]
fn jitter_cannot_reorder_tasks_that_genuinely_differ() {
    let now = noon();
    let lower = dated(2, 0.0, now);
    let higher = dated(3, 0.0, now);
    for seed in 0..64 {
        assert!(higher.importance_score(now, seed) > lower.importance_score(now, seed));
    }
}

// ---------------------------------------------------------------- zones (core.md §5.3, §14)

/// Central Europe in 2026: +01:00, +02:00 from 29 March 01:00 UTC (spring
/// forward), +01:00 again from 25 October 01:00 UTC (fall back).
fn europe() -> TableZone {
    let utc = |m, d| local(2026, m, d, 1, 0, &FixedZone(0)).as_unix_ms();
    TableZone::new(3600, vec![(utc(3, 29), 7200), (utc(10, 25), 3600)]).unwrap_or(TableZone { initial: 0, transitions: Vec::new() })
}

// spec: CORE-TIME-002
// spec: CORE-READ-001
#[test]
fn a_score_reads_instants_so_a_clock_change_shortens_the_wait() {
    // Nothing here takes a zone: a score is a function of instants. A zone
    // only decides which instants a wall time names. "Due at noon tomorrow",
    // written on the Saturday before the spring-forward, is 23 hours away,
    // and scores as 23 hours away, not 24.
    let zone = europe();
    let now = local(2026, 3, 28, 12, 0, &zone);
    let due = local(2026, 3, 29, 12, 0, &zone);
    assert_eq!(time::between(now, due), chrono::TimeDelta::hours(23));
    let in_23h = Item { deadline: Some(shift(now, 23 * 60)), ..active(Some(2), None, false, None) };
    assert_eq!(dated(2, 0.0, due).base_score(now), in_23h.base_score(now));
    assert!(dated(2, 0.0, due).base_score(now) > dated(2, 1.0, now).base_score(now));

    // The fall-back: noon to noon across it is 25 hours.
    let now = local(2026, 10, 24, 12, 0, &zone);
    let due = local(2026, 10, 25, 12, 0, &zone);
    assert_eq!(time::between(now, due), chrono::TimeDelta::hours(25));
    assert!(dated(2, 0.0, due).base_score(now) < dated(2, 1.0, now).base_score(now));
}

// spec: CORE-READ-002
// spec: CORE-READ-005
#[test]
fn the_item_queries_leave_the_board_as_it_was() {
    let now = noon();
    let mut board = BoardData::empty();
    board.items = vec![
        Item { id: 1, blocks: vec![session(4, shift(now, 60), 30), session(9, now, 30)], ..dated(3, 2.0, now) },
        Item { id: 2, ..active(None, None, true, Some(now)) },
        Item { id: 3, ..routine(WEEKDAYS) },
        Item { id: 4, ..undated(1, 5.0, now) },
    ];
    let before = board.clone();
    for item in &board.items {
        let _ = item.importance_score(now, 739_000);
        let _ = (item.is_routine(), item.calendar_item_color(), item.planner_anchor(), item.is_planned());
        let _ = (item.planned_minutes(), item.remaining_minutes(), item.wants_planning());
        if let Some(rule) = &item.recurrence {
            let _ = (rule.falls_on(a_friday()), rule.summary(), rule.summary_dated());
        }
    }
    assert_eq!(board, before, "no read query changes the board");
    // The anchor is the earliest block, found without sorting the blocks.
    assert_eq!(board.items[0].planner_anchor(), Some(now));
    assert_eq!(board.items[0].blocks.iter().map(|b| b.sid).collect::<Vec<_>>(), vec![4, 9]);
}
