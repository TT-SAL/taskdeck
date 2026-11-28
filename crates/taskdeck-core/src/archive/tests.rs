//! Today's archive tests (archive.rs:819-1283), ported, and the zone and
//! read-query clauses of core.md §5.3 and §14.
//!
//! Not ported, because what they test is not: `Archived::retire` and
//! `to_active` (retiring and restoring are `apply`'s, core.md §9.8, §9.21),
//! the serde shape of a legacy row, and the archive log and its file.

use super::*;
use crate::board::{BoardData, Recurrence, RowBlock};
use crate::time::{FixedZone, Instant, TableZone};

/// Today's tests ran in `Local`, the zone of the process that ran
/// `cargo test`; the recorded data was made at +03:00. A fixed zone at +03:00
/// stands in for it here: none of these times is near a transition.
const ZONE: FixedZone = FixedZone(3 * 3600);

fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default()
}

fn local(y: i32, m: u32, d: u32, hour: u32, minute: u32, zone: &dyn Zone) -> Instant {
    let minutes = i32::try_from(hour * 60 + minute).unwrap_or(0);
    time::resolve_on_day(date(y, m, d), minutes, zone).unwrap_or(Instant::MIN)
}

/// `Local.with_ymd_and_hms(year, month, day, hour, minute, 0)` of today's tests, in `ZONE`.
fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Instant {
    local(year, month, day, hour, minute, &ZONE)
}

/// `Archived::retire(task(name), outcome, archived_at)` of today's tests: a
/// task (id 1, importance 2, created 1 Aug 09:00) retired as `outcome`.
fn retired(name: &str, outcome: Outcome, archived_at: Instant) -> ArchiveRow {
    ArchiveRow {
        id: 1,
        kind: Kind::Task,
        name: name.to_string(),
        created: at(2026, 8, 1, 9, 0),
        deadline: None,
        importance: Some(2),
        time_importance: None,
        duration_minutes: None,
        recurrence: None,
        blocks: Vec::new(),
        archived_at,
        outcome,
    }
}

fn archived(name: &str, archived_at: Instant) -> ArchiveRow {
    retired(name, Outcome::Finished, archived_at)
}

/// Today's `Session { start, minutes }`, with a sid.
fn session(sid: u64, start: Instant, minutes: u32) -> RowBlock {
    RowBlock { sid, start, minutes }
}

fn canonical(text: &str) -> Instant {
    Instant::parse_canonical(text).unwrap_or(Instant::MIN)
}

#[test]
fn timing_reads_the_deadline_against_the_finish() {
    let dated = |finished_at| ArchiveRow {
        deadline: Some(at(2026, 8, 20, 17, 0)),
        ..retired("report", Outcome::Finished, finished_at)
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
    let event = ArchiveRow {
        kind: Kind::Event,
        deadline: Some(at(2026, 8, 10, 9, 0)),
        ..retired("dentist", Outcome::Dropped, at(2026, 8, 20, 9, 0))
    };
    assert_eq!(event.timing(), Timing::Undated);
    assert!(!event.was_judged());
    assert!(event.verdict(&ZONE).contains("event removed"), "{}", event.verdict(&ZONE));
}

#[test]
fn a_legacy_event_row_does_not_claim_to_have_been_finished() {
    // Found in a real log: an event archived by a build that had no
    // `outcome` field, so it loads as `Finished`. Today's test parsed the
    // line {"id":119,"importance":null,"name":"saunas",
    // "created":"2026-08-15T22:09:51.933405+03:00",
    // "deadline":"2026-08-14T01:30:00+03:00","is_event":true,
    // "inactivated":"2026-08-15T22:10:22.233122+03:00"}; here is the row it
    // is (the parsing is the import's, core.md §15).
    let row = ArchiveRow {
        id: 119,
        kind: Kind::Event,
        name: "saunas".to_string(),
        created: canonical("2026-08-15T19:09:51.933405000Z"),
        deadline: Some(canonical("2026-08-13T22:30:00.000000000Z")),
        importance: None,
        time_importance: None,
        duration_minutes: None,
        recurrence: None,
        blocks: Vec::new(),
        archived_at: canonical("2026-08-15T19:10:22.233122000Z"),
        outcome: Outcome::Finished,
    };

    assert_eq!(row.outcome, Outcome::Finished, "the stored field is what it is");
    assert!(!row.was_finished(), "but an event is never finished");
    assert_eq!(row.mark(), glyph(Outcome::Dropped));

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
    let row = ArchiveRow {
        kind: Kind::Routine,
        recurrence: Some(Recurrence {
            days: crate::tasks::EVERY_DAY,
            anchor: date(2026, 8, 14),
            start_minutes: 23 * 60,
            minutes: 8 * 60,
        }),
        importance: None,
        ..retired("sleep", Outcome::Dropped, at(2026, 8, 20, 9, 0))
    };

    // (Today's lines on `to_active` and the restored item's score are not
    // ported: restoring is `apply`'s.)
    assert!(row.is_routine());

    // It is neither finished work nor a broken promise: it was an
    // arrangement, and the row says what the arrangement was.
    assert!(!row.was_finished());
    assert!(!row.was_judged());
    assert_eq!(row.timing(), Timing::Undated);
    assert_eq!(row.mark(), glyph(Outcome::Dropped));
    assert_eq!(row.color_id(), crate::tasks::ROUTINE_COLOR_INDEX);
    let verdict = row.verdict(&ZONE);
    assert!(verdict.contains("routine dropped"), "{verdict}");
    assert!(verdict.contains("every day at 23:00 for 8h"), "{verdict}");

    let summary = summarize([&row]);
    assert_eq!((summary.finished, summary.dropped, summary.events, summary.routines), (0, 0, 0, 1));
    assert!(summary.headline().contains("1 routine dropped"), "{}", summary.headline());
}

#[test]
fn the_verdict_holds_the_estimate_against_the_booking() {
    let row = ArchiveRow {
        deadline: Some(at(2026, 8, 20, 17, 0)),
        duration_minutes: Some(120),
        blocks: vec![session(1, at(2026, 8, 18, 10, 0), 90), session(2, at(2026, 8, 19, 10, 0), 90)],
        ..retired("physics homework", Outcome::Finished, at(2026, 8, 18, 17, 0))
    };

    let verdict = row.verdict(&ZONE);
    assert!(verdict.contains("finished 2d early"), "{verdict}");
    assert!(verdict.contains("2h estimated, 3h over 2 sittings"), "{verdict}");
    assert!(verdict.contains("on the board"), "{verdict}");
    assert_eq!((row.booked_minutes(), row.sittings()), (180, 2));
}

#[test]
fn a_deadline_met_to_the_minute_is_neither_early_nor_late() {
    let row = ArchiveRow {
        deadline: Some(at(2026, 8, 20, 17, 0)),
        ..retired("report", Outcome::Finished, at(2026, 8, 20, 16, 50))
    };
    assert!(row.verdict(&ZONE).contains("right on the deadline"), "{}", row.verdict(&ZONE));
}

#[test]
fn an_unbooked_task_says_so() {
    let never = retired("tidy the shed", Outcome::Dropped, at(2026, 8, 5, 9, 0));
    assert!(never.verdict(&ZONE).contains("no time set aside"), "{}", never.verdict(&ZONE));

    let promised = ArchiveRow { duration_minutes: Some(45), ..retired("tidy the shed", Outcome::Dropped, at(2026, 8, 5, 9, 0)) };
    assert!(promised.verdict(&ZONE).contains("never booked"), "{}", promised.verdict(&ZONE));
}

#[test]
fn spans_are_one_coarse_unit() {
    use chrono::TimeDelta as Duration;
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
    let met = ArchiveRow {
        deadline: Some(at(2026, 8, 20, 17, 0)),
        blocks: vec![session(1, at(2026, 8, 18, 10, 0), 60)],
        ..retired("met", Outcome::Finished, at(2026, 8, 19, 9, 0))
    };
    let missed = ArchiveRow {
        deadline: Some(at(2026, 8, 20, 17, 0)),
        ..retired("missed", Outcome::Finished, at(2026, 8, 25, 9, 0))
    };
    // Dropped with a date: withdrawn, not missed.
    let dropped = ArchiveRow {
        deadline: Some(at(2026, 8, 20, 17, 0)),
        ..retired("dropped", Outcome::Dropped, at(2026, 8, 21, 9, 0))
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
    let dropped_event = ArchiveRow { kind: Kind::Event, ..retired("dentist", Outcome::Dropped, at(2026, 8, 20, 9, 0)) };

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
    let rows_owned = [
        archived("c", at(2026, 8, 20, 9, 0)),
        archived("b", at(2026, 8, 2, 9, 0)),
        archived("a", at(2026, 7, 30, 9, 0)),
    ];
    let rows: Vec<&ArchiveRow> = rows_owned.iter().collect();

    let runs = group_by_month(&rows, &ZONE);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].rows, 0..2);
    assert_eq!(runs[1].rows, 2..3);
    assert!(runs[0].label.starts_with("August"), "{}", runs[0].label);
    assert!(runs[1].label.starts_with("July"), "{}", runs[1].label);
    // chrono's `%B %Y`.
    assert_eq!(runs[0].label, "August 2026");
}

#[test]
fn grouping_nothing_produces_nothing() {
    assert!(group_by_month(&[], &ZONE).is_empty());
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
fn months_and_event_times_are_read_in_the_zone() {
    let zone = europe();
    let utc = FixedZone(0);

    // Newest first: 31 March 22:30 UTC is 1 April 00:30 after the
    // spring-forward (+02:00), 21:30 UTC is still 31 March 23:30.
    let rows_owned = [
        archived("april", local(2026, 3, 31, 22, 30, &utc)),
        archived("march", local(2026, 3, 31, 21, 30, &utc)),
    ];
    let rows: Vec<&ArchiveRow> = rows_owned.iter().collect();
    let runs = group_by_month(&rows, &zone);
    assert_eq!(
        runs,
        vec![
            MonthRun { label: "April 2026".to_string(), rows: 0..1 },
            MonthRun { label: "March 2026".to_string(), rows: 1..2 },
        ]
    );
    assert_eq!(group_by_month(&rows, &utc), vec![MonthRun { label: "March 2026".to_string(), rows: 0..2 }]);

    // The fall-back fold: 00:30 UTC (+02:00) and 01:30 UTC (+01:00) on
    // 25 October are both 02:30 local; in UTC they read as they are.
    let event = |t| ArchiveRow { kind: Kind::Event, deadline: Some(t), ..retired("dentist", Outcome::Dropped, local(2026, 10, 26, 9, 0, &utc)) };
    for (t, in_utc) in [(local(2026, 10, 25, 0, 30, &utc), "00:30"), (local(2026, 10, 25, 1, 30, &utc), "01:30")] {
        assert!(event(t).verdict(&zone).contains("was set for 25 Oct 02:30"), "{}", event(t).verdict(&zone));
        assert!(event(t).verdict(&utc).contains(&format!("was set for 25 Oct {in_utc}")), "{}", event(t).verdict(&utc));
    }
}

// spec: CORE-READ-002
// spec: CORE-READ-005
#[test]
fn the_archive_queries_leave_the_board_as_it_was() {
    let mut board = BoardData::empty();
    // In the archive order (ascending archived_at, core.md §6.12).
    board.archive = vec![
        archived("a", at(2026, 7, 30, 9, 0)),
        ArchiveRow { kind: Kind::Event, deadline: Some(at(2026, 8, 1, 9, 0)), ..archived("b", at(2026, 8, 2, 9, 0)) },
        archived("c", at(2026, 8, 20, 9, 0)),
    ];
    let before = board.clone();
    let zone = europe();

    let newest_first: Vec<&ArchiveRow> = board.archive.iter().rev().collect();
    let runs = group_by_month(&newest_first, &zone);
    let summary = summarize(board.archive.iter());
    let filter = Filter { query: "b".to_string(), ..Filter::default() };
    let admitted: Vec<&str> = board.archive.iter().filter(|r| filter.admits(r)).map(|r| r.name.as_str()).collect();
    for row in &board.archive {
        let _ = (row.verdict(&zone), row.timing(), row.mark(), row.lifetime(), row.color_id());
    }

    assert_eq!(board, before, "no read query changes the board");
    // The runs index the caller's own list, newest first, and leave the board's order alone.
    assert_eq!(runs.iter().map(|r| r.rows.clone()).collect::<Vec<_>>(), vec![0..2, 2..3]);
    assert_eq!(board.archive.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c"]);
    assert_eq!((summary.finished, summary.events), (2, 1));
    assert_eq!(admitted, vec!["b"]);
}
