//! Today's tests of the snapshot builders (phone.rs:2774-2835 and
//! 3228-3325), ported with explicit zones, sids in place of session indices
//! and the changes of core.md §14.3; and the clauses of §14.3.
//!
//! Not ported: the parts of those tests about commands (`Command` parsing in
//! `behind_is_zero_on_any_day_but_today`), which the intent tests cover, and
//! the HTTP, picture and feed tests of phone.rs, which are not the core's.

use super::*;
use crate::board::{Block, ItemStamps, Outcome, Recurrence, Stamp};
use crate::intent::{Intent, parse_intent};
use crate::time::FixedZone;

/// Today's tests ran in `Local`; a fixed zone at +03:00 stands in for it:
/// none of these times is near a transition.
const ZONE: FixedZone = FixedZone(3 * 3600);
const ONE: Stamp = Stamp { seq: 1, device_no: 0 };
const STAMPS: ItemStamps = ItemStamps { estimate: ONE, levels: ONE, name: ONE, repeat: ONE, time: ONE };

fn date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default()
}

/// `Local.with_ymd_and_hms(y, m, d, hour, minute, 0)` of today's tests, in `ZONE`.
fn at(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> Instant {
    let minutes = i32::try_from(hour * 60 + minute).unwrap_or(0);
    time::resolve_on_day(date(y, m, d), minutes, &ZONE).unwrap_or(Instant::MIN)
}

/// Today's `task(id, name)`: a task with horizon 1, created 2026-09-01 09:00.
fn task(id: u64, name: &str) -> Item {
    Item {
        id,
        kind: Kind::Task,
        name: name.to_string(),
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
    Block { sid, start, minutes, stamp: ONE }
}

/// Today's `Archived::retire(item, outcome, at)`.
fn retire(item: Item, outcome: Outcome, archived_at: Instant) -> ArchiveRow {
    ArchiveRow {
        id: item.id,
        kind: item.kind,
        name: item.name,
        created: item.created,
        deadline: item.deadline,
        importance: item.importance,
        time_importance: item.time_importance,
        duration_minutes: item.duration_minutes,
        recurrence: item.recurrence,
        blocks: item.blocks.iter().map(|b| RowBlock { sid: b.sid, start: b.start, minutes: b.minutes }).collect(),
        archived_at,
        outcome,
    }
}

fn subscription(id: u64, enabled: bool) -> Subscription {
    Subscription { id, name: "Timetable".into(), url: "https://example.invalid/a.ics".into(), color: [200, 120, 40, 255], enabled }
}

fn input<'a>(board: &'a BoardData, overlay: &'a Overlay, now: Instant, from: chrono::NaiveDate, days: u32) -> SnapshotInput<'a, FixedZone> {
    SnapshotInput {
        board,
        overlay,
        zone: &ZONE,
        now,
        from,
        days,
        version: 0,
        ranked: Some(Vec::new()),
        look: None,
        dials: Dials { blur: 11, light: 39 },
    }
}

// The window the calendars were read for is declared only when some
// subscription is enabled; switched off is the same as absent.
// spec: CORE-SNAP-001
#[test]
fn the_snapshot_declares_how_far_the_calendars_were_read_only_when_there_are_any() {
    let now = at(2026, 9, 5, 12, 0);
    let from = date(2026, 9, 5);
    let covers = Covered { from: date(2026, 7, 7), until: date(2027, 10, 10) };
    let overlay = Overlay::sealed(Vec::new(), Vec::new()).covering(Some(covers));

    // Nothing subscribed: no window, because there is nothing to be ignorant
    // about, and a day with nothing on it really is free.
    let mut board = BoardData::empty();
    assert_eq!(snapshot(&input(&board, &overlay, now, from, 1)).known, None);

    board.subscriptions.push(subscription(1, true));
    assert_eq!(snapshot(&input(&board, &overlay, now, from, 1)).known, Some(covers));

    // Switched off: nothing was read, so nothing is claimed to have been.
    board.subscriptions[0].enabled = false;
    assert_eq!(snapshot(&input(&board, &overlay, now, from, 1)).known, None);
}

// More days than the server builds are cut down quietly to 31; none is
// raised to 1.
// spec: CORE-SNAP-001
#[test]
fn a_request_for_more_days_than_the_server_builds_is_cut_down_quietly() {
    let board = BoardData::empty();
    let overlay = Overlay::default();
    let now = at(2026, 9, 5, 12, 0);
    let from = date(2026, 9, 5);
    for asked in [MAX_SNAPSHOT_DAYS + 1, 100, 400, u32::MAX] {
        let snap = snapshot(&input(&board, &overlay, now, from, asked));
        assert_eq!(snap.days.len(), 31, "asked for {asked}");
    }
    let exact = snapshot(&input(&board, &overlay, now, from, MAX_SNAPSHOT_DAYS));
    assert_eq!(exact.days.len(), 31);
    assert_eq!(exact.days.last().map(|d| d.date), Some(date(2026, 10, 5)));
    assert_eq!(snapshot(&input(&board, &overlay, now, from, 0)).days.len(), 1);
    // The last day chrono holds: no panic, the days that exist.
    let end = snapshot(&input(&board, &overlay, now, chrono::NaiveDate::MAX, 3));
    assert_eq!(end.days.len(), 1);
}

// Live entries and ghosts are laid out together; the entry's `session` is
// the block's sid.
// spec: CORE-SNAP-002
// spec: CORE-READ-001
#[test]
fn a_day_lays_out_live_entries_and_ghosts_together() {
    let day = date(2026, 9, 4);
    let mut planned = task(1, "report");
    planned.blocks.push(block(7, at(2026, 9, 4, 9, 0), 60));
    planned.deadline = Some(at(2026, 9, 4, 17, 0));
    let mut sleep = task(2, "sleep");
    sleep.kind = Kind::Routine;
    sleep.recurrence = Some(Recurrence { days: crate::tasks::EVERY_DAY, anchor: day, start_minutes: 23 * 60, minutes: 60 });

    let mut finished = task(3, "finished");
    finished.blocks.push(block(8, at(2026, 9, 4, 9, 0), 60));
    let archived = retire(finished, Outcome::Finished, at(2026, 9, 4, 10, 0));
    let ghosts: Vec<(&ArchiveRow, &RowBlock)> = archived.blocks.iter().map(|b| (&archived, b)).collect();

    let snapshot = build_day(&[planned, sleep], &ghosts, day, at(2026, 9, 4, 12, 0), &ZONE);
    assert!(snapshot.is_today);
    assert_eq!(snapshot.weekday, "Friday");
    assert_eq!(snapshot.label, "4 September 2026");
    // A block, a due marker, and a routine block.
    assert_eq!(snapshot.entries.len(), 3);
    let block = &snapshot.entries[0];
    assert_eq!((block.id, block.session, block.start, block.minutes, block.marker), (1, Some(7), 540, 60, false));
    // The ghost shares the nine o'clock hour, so both take a column.
    assert_eq!(block.columns, 2);
    assert_eq!(snapshot.ghosts.len(), 1);
    assert_eq!(snapshot.ghosts[0].columns, 2);
    assert!(snapshot.ghosts[0].finished);
    let due = snapshot.entries.iter().find(|e| e.due).map(|e| (e.start, e.marker));
    assert_eq!(due, Some((17 * 60, true)));
    let routine = snapshot.entries.iter().find(|e| e.kind == "routine").map(|e| e.start);
    assert_eq!(routine, Some(23 * 60));
    assert_eq!(snapshot.summary, "1h planned · 1 block · 1 due · 1h routine · 1h done");
    // The nine o'clock block is wholly behind a noon now-line.
    assert_eq!(snapshot.behind_minutes, 60);
}

// Behind is zero on any day but today; and today is the zone's.
// spec: CORE-READ-001
#[test]
fn behind_is_zero_on_any_day_but_today() {
    let day = date(2026, 9, 4);
    let mut planned = task(1, "report");
    planned.blocks.push(block(1, at(2026, 9, 4, 9, 0), 60));
    let snapshot = build_day(std::slice::from_ref(&planned), &[], day, at(2026, 9, 5, 12, 0), &ZONE);
    assert!(!snapshot.is_today);
    assert_eq!(snapshot.behind_minutes, 0);
    // 2026-09-04T22:30Z is 9-5 at +03:00 but 9-4 in UTC, where the block,
    // at 06:00 UTC, is behind.
    let late = Instant::parse_canonical("2026-09-04T22:30:00.000000000Z").unwrap_or(Instant::MIN);
    assert!(!build_day(std::slice::from_ref(&planned), &[], day, late, &ZONE).is_today);
    let utc = build_day(std::slice::from_ref(&planned), &[], day, late, &FixedZone(0));
    assert!(utc.is_today);
    assert_eq!((utc.entries[0].start, utc.behind_minutes), (360, 60));
}

// A done row's key round-trips through JSON: `archived_at` is the canonical
// instant text, which the page's `restore` hands back and §12.2 parses.
// spec: CORE-SNAP-002
#[test]
fn done_rows_round_trip_their_key_through_json() {
    let mut finished = task(3, "finished");
    finished.blocks.push(block(1, at(2026, 9, 4, 9, 0), 60));
    let archived = retire(finished, Outcome::Finished, at(2026, 9, 4, 10, 0));
    let rows = done_rows(std::slice::from_ref(&archived), &ZONE);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].finished);
    assert_eq!(rows[0].kind, "task");
    assert_eq!(rows[0].when, "Fri 4 Sep 10:00");
    assert_eq!(rows[0].archived_at, "2026-09-04T07:00:00.000000000Z");

    // The key the page hands back is the one the archive knows.
    let json = serde_json::to_value(&rows[0]).unwrap_or_default();
    let restore = serde_json::json!({"op": "restore", "id": json["id"], "archived_at": json["archived_at"]});
    match parse_intent(&restore.to_string()) {
        Ok(Intent::Restore { id, archived_at }) => {
            let at = Instant::from_chrono(archived_at.with_timezone(&chrono::Utc));
            assert_eq!((id, at), (archived.id, Some(archived.archived_at)));
        }
        other => panic!("parsed {other:?}"),
    }
}

// The tray splits by the shown day: owed on or before it, and the rest.
// spec: CORE-READ-001
#[test]
fn the_tray_splits_by_the_shown_day() {
    let day = date(2026, 9, 4);
    let now = at(2026, 9, 4, 12, 0);
    let mut owed = task(1, "owed");
    owed.deadline = Some(at(2026, 9, 3, 17, 0));
    owed.importance = Some(2);
    owed.duration_minutes = Some(120);
    owed.blocks.push(block(1, at(2026, 9, 2, 9, 0), 60));
    let later = task(2, "later");
    let mut event = task(3, "event");
    event.kind = Kind::Event;
    event.deadline = Some(now);

    let tray = build_tray(&[owed, later, event], day, now, 7, &ZONE);
    assert_eq!(tray.due.len(), 1);
    assert_eq!(tray.due[0].name, "owed");
    assert_eq!(tray.due[0].due_in.as_deref(), Some("overdue"));
    assert_eq!(tray.due[0].deadline.as_deref(), Some("2026-09-03T17:00"));
    assert_eq!((tray.due[0].planned, tray.due[0].drop_minutes), (60, 60));
    assert_eq!(tray.later.len(), 1);
    assert_eq!(tray.later[0].name, "later");
    assert_eq!(tray.later[0].drop_minutes, planner::DEFAULT_BLOCK_MINUTES);
}

// An item's detail: sessions in ascending sid with the sid as `index`, local
// days and minutes, deadline and creation day in the zone.
// spec: CORE-SNAP-002
// spec: CORE-READ-001
#[test]
fn an_item_detail_gives_sids_and_local_times() {
    let mut item = task(5, "detail");
    item.deadline = Some(at(2026, 9, 6, 1, 30));
    item.duration_minutes = Some(240);
    item.blocks = vec![block(3, at(2026, 9, 4, 9, 0), 60), block(9, at(2026, 9, 5, 2, 0), 45)];
    let detail = item_detail(&item, &ZONE);
    let sessions: Vec<(u64, chrono::NaiveDate, i32, u32)> =
        detail.sessions.iter().map(|s| (s.index, s.day, s.start, s.minutes)).collect();
    assert_eq!(sessions, vec![(3, date(2026, 9, 4), 540, 60), (9, date(2026, 9, 5), 120, 45)]);
    assert_eq!(detail.deadline.as_deref(), Some("2026-09-06T01:30"));
    assert_eq!((detail.planned, detail.remaining), (105, Some(135)));
    assert_eq!(detail.created, date(2026, 9, 1));
    // In UTC the second block is on 9-4 at 23:00 and the deadline on 9-5.
    let utc = item_detail(&item, &FixedZone(0));
    assert_eq!((utc.sessions[1].day, utc.sessions[1].start), (date(2026, 9, 4), 23 * 60));
    assert_eq!(utc.deadline.as_deref(), Some("2026-09-05T22:30"));
}

// The snapshot's own changes: `version`, `palette`, `notes_base`, items in
// ascending id, `ranked` computed when the caller gives none, `done` newest
// first in reverse archive order with canonical `archived_at`, the caller's
// look and dials, `today` and `now_minutes` in the zone; the board is not
// changed.
// spec: CORE-SNAP-001
// spec: CORE-SNAP-002
// spec: CORE-READ-002
// spec: CORE-READ-005
#[test]
fn the_snapshot_takes_its_changes_from_the_input() {
    let mut board = BoardData::empty();
    board.items = vec![task(1, "one"), task(2, "two"), task(4, "four")];
    board.items[2].kind = Kind::Event;
    board.items[2].deadline = Some(at(2026, 9, 5, 9, 0));
    let t = at(2026, 9, 4, 10, 0);
    board.archive = vec![
        retire(task(6, "six"), Outcome::Finished, t),
        retire(task(5, "five"), Outcome::Dropped, at(2026, 9, 4, 11, 0)),
        retire(task(7, "seven"), Outcome::Finished, at(2026, 9, 4, 11, 0)),
    ];
    board.notes = "milk".into();
    board.notes_rev = 9;
    board.palette[0] = [1, 2, 3, 4];
    let copy = board.clone();
    let overlay = Overlay::default();
    let now = Instant::parse_canonical("2026-09-04T22:30:00.000000000Z").unwrap_or(Instant::MIN);
    let mut request = input(&board, &overlay, now, date(2026, 9, 5), 2);
    request.version = 42;
    request.ranked = None;
    request.look = Some(Look { id: "p".into(), aspect: 0.5, top: "#000000".into() });
    let snap = snapshot(&request);
    assert_eq!(board, copy);
    assert_eq!(snap.version, 42);
    assert_eq!(snap.palette, board.palette);
    assert_eq!((snap.notes.as_str(), snap.notes_base), ("milk", 9));
    assert_eq!(snap.today, date(2026, 9, 5));
    assert_eq!(snap.now_minutes, 90);
    assert_eq!(snap.items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![1, 2, 4]);
    assert_eq!(snap.items[2].kind, "event");
    let mut ranked = snap.ranked.clone();
    ranked.sort_unstable();
    assert_eq!(ranked, vec![1, 2]);
    assert_eq!(snap.ranked, read::ranked_ids(&board, now, &ZONE));
    assert_eq!(snap.done.iter().map(|d| d.id).collect::<Vec<_>>(), vec![7, 5, 6]);
    assert_eq!(snap.done[2].archived_at, "2026-09-04T07:00:00.000000000Z");
    assert_eq!((snap.dials.blur, snap.dials.light), (11, 39));
    let json = answer_snapshot(&request);
    assert_eq!(json["look"]["id"], "p");
    assert_eq!(json["notes_base"], 9);
    assert_eq!(json["days"].as_array().map(Vec::len), Some(2));
    assert_eq!(json["days"][0]["entries"][0]["id"], 4);
    // Today's field order, `notes_base` right after `notes`.
    let text = serde_json::to_string(&snap).unwrap_or_default();
    let order = ["\"version\"", "\"today\"", "\"now_minutes\"", "\"palette\"", "\"labels\"", "\"days\"", "\"tray\"",
        "\"items\"", "\"ranked\"", "\"done\"", "\"notes\"", "\"notes_base\"", "\"look\"", "\"dials\""];
    let positions: Vec<usize> = order.iter().map(|k| text.rfind(k).unwrap_or(0)).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
}

// Ghosts come from `archive_by_day`: placed at their local minutes, at least
// a quarter-hour, in start order, newest first among equal starts.
// spec: CORE-SNAP-003
#[test]
fn ghosts_are_placed_from_their_buckets() {
    let mut board = BoardData::empty();
    let mut a = task(1, "a");
    a.blocks = vec![block(1, at(2026, 9, 4, 14, 0), 5), block(2, at(2026, 9, 4, 9, 0), 30)];
    let mut b = task(2, "b");
    b.blocks = vec![block(3, at(2026, 9, 4, 9, 0), 60)];
    board.archive = vec![retire(a, Outcome::Finished, at(2026, 9, 4, 15, 0)), retire(b, Outcome::Dropped, at(2026, 9, 4, 16, 0))];
    let buckets = read::archive_by_day(&board, date(2026, 9, 4), 1, &ZONE);
    let day = build_day(&[], &buckets[0], date(2026, 9, 4), at(2026, 9, 4, 8, 0), &ZONE);
    let ghosts: Vec<(&str, i32, u32, bool)> =
        day.ghosts.iter().map(|g| (g.name.as_str(), g.start, g.minutes, g.finished)).collect();
    assert_eq!(ghosts, vec![("b", 540, 60, false), ("a", 540, 30, true), ("a", 840, 15, true)]);
}

// The subscribed calendars' events, from the caller's overlay; a disabled
// calendar's are not drawn; background spans take the full width.
// spec: CORE-SNAP-001
#[test]
fn subscribed_events_come_from_the_overlay() {
    let d = date(2026, 9, 4);
    let event = |subscription, start, end, all_day| crate::overlay::OverlayEvent {
        subscription,
        day: d,
        start,
        end,
        all_day,
        free: false,
        summary: "meeting".into(),
        location: String::new(),
        description: "note".into(),
    };
    let overlay = Overlay::sealed(
        vec![event(1, 600, 660, false), event(1, 630, 690, false), event(1, 0, 1440, true), event(2, 600, 660, false)],
        Vec::new(),
    );
    let subs = [subscription(1, true), subscription(2, false)];
    let days = with_subscribed(vec![build_day(&[], &[], d, at(2026, 9, 4, 8, 0), &ZONE)], &subs, &overlay, &ZONE);
    let shown: Vec<(i32, usize, usize, bool)> =
        days[0].subscribed.iter().map(|s| (s.start, s.column, s.columns, s.background)).collect();
    assert_eq!(shown.len(), 3);
    assert!(shown.contains(&(0, 0, 1, true)));
    assert!(shown.contains(&(600, 0, 2, false)) && shown.contains(&(630, 1, 2, false)));
    assert!(days[0].subscribed.iter().all(|s| s.color == "#c87828" && s.calendar == "Timetable"));
    let json = serde_json::to_value(&days[0].subscribed[0]).unwrap_or_default();
    assert!(json.get("where").is_none() && json["note"] == "note");
}

// The labels are today's tables.
#[test]
fn the_labels_are_today_s() {
    let labels = Labels::current();
    assert_eq!(labels.severity[4], "Lethally important");
    assert_eq!(labels.horizon, ["Within a month", "Within a week", "Within days", "Whenever"]);
    assert_eq!(labels.horizon_order, [2, 1, 0, 3]);
    assert_eq!(labels.weekdays[0], "Mon");
}
