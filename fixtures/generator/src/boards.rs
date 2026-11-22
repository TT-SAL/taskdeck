//! The made-up boards, each a data directory as today's build keeps it.
//!
//! Wherever today's code has a way to make a thing, the thing is made that way
//! (`Board::apply` with a wire command at a pinned `now`, `ArchiveLog::record`,
//! `Board::save_*`), so the files are what today's code writes. Where a board
//! needs data no command can make — ten thousand old rows, ids at the limits,
//! unreadable lines — the data is built from today's own types and written by
//! today's own writers; only the `unreadable` archive is assembled byte by byte,
//! because it stands for a file damaged outside the program.

use std::path::Path;

use chrono::{DateTime, Duration, Local};
use serde_json::json;
use task_deck::archive::{ARCHIVE_FILE, Archived, Outcome};
use task_deck::board::{NAME_MAX_CHARS, NOTES_MAX_CHARS, TEMPORARY_ID_FLOOR};
use task_deck::subscriptions::{Covered, FetchOutcome, FetchStatus, Overlay, OverlayEvent, SUBSCRIPTIONS_MAX};
use task_deck::tasks::{Active, EVERY_DAY, Recurrence, Session};

use crate::clock::{day, text, utc};
use crate::common::{Builder, write_settings};
use crate::out;
use crate::rng::SplitMix64;

/// Every board, in the order they are written.
pub const BOARDS: [&str; 6] = ["empty", "small", "archive-10k", "dst-edges", "id-limits", "unreadable"];

pub fn write_all(boards: &Path) -> Result<(), String> {
    empty(&boards.join("empty"))?;
    small(&boards.join("small"))?;
    archive_10k(&boards.join("archive-10k"))?;
    dst_edges(&boards.join("dst-edges"))?;
    id_limits(&boards.join("id-limits"))?;
    unreadable(&boards.join("unreadable"))?;
    Ok(())
}

/// What a first start leaves: the seeded `[]` and `""`, the built-in palettes
/// and the default settings (with the fixtures' home place and palette).
fn empty(dir: &Path) -> Result<(), String> {
    Builder::new(dir, utc(2026, 10, 1, 6, 0, 0))?;
    write_settings(dir)
}

/// A few dozen items covering every kind and every field.
fn small(dir: &Path) -> Result<(), String> {
    let mut b = Builder::new(dir, utc(2026, 10, 1, 6, 0, 0))?;
    // Dated tasks at every severity, undated ones at every horizon.
    let mut dated = Vec::new();
    for level in 0..5u8 {
        dated.push(b.id(json!({"op":"add","name":format!("Fixture task, severity {level}: polish the pretend trophy"),
            "deadline":format!("2026-10-{:02}T17:00", 21 + level),"importance":level}))?);
        b.tick(7);
    }
    let mut undated = Vec::new();
    for level in 0..4u8 {
        undated.push(b.id(json!({"op":"add","name":format!("Fixture task, horizon {level}: sort the imaginary buttons"),
            "time_importance":level}))?);
        b.tick(5);
    }
    let bare = b.id(json!({"op":"add","name":"Fixture task with nothing said about it"}))?;
    let quick = b.id(json!({"op":"quick_add","name":"Quick add: oil the made-up bicycle"}))?;
    let quick_dated = b.id(json!({"op":"quick_add","name":"Quick add, dated: return the invented book","deadline":"2026-10-24T12:00"}))?;
    b.id(json!({"op":"add","name":"Fixture event: widget guild meetup","deadline":"2026-10-21T18:30:00+02:00","is_event":true}))?;
    b.tick(30);
    // Planned work: several blocks per task, with and without estimates.
    let planned = b.id(json!({"op":"create","kind":"task","name":"Fixture plan: draft the moon-garden layout","day":"2026-10-20","start":540,"minutes":90}))?;
    b.send(json!({"op":"add_block","id":planned,"day":"2026-10-21","start":600,"minutes":60}))?;
    b.send(json!({"op":"set_estimate","id":planned,"minutes":240}))?;
    b.send(json!({"op":"add_block","id":planned,"day":"2026-10-22","start":780}))?;
    b.send(json!({"op":"add_block","id":dated[2],"day":"2026-10-20","start":660,"minutes":45}))?;
    b.send(json!({"op":"add_block","id":dated[2],"day":"2026-10-23","start":900,"minutes":30}))?;
    b.send(json!({"op":"set_estimate","id":undated[1],"minutes":120}))?;
    b.send(json!({"op":"move_block","id":dated[2],"session":1,"day":"2026-10-23","start":930,"minutes":45}))?;
    let unplanned = b.id(json!({"op":"create","kind":"task","name":"Fixture plan that will be unplanned","day":"2026-10-20","start":1200,"minutes":30}))?;
    b.send(json!({"op":"unplan","id":unplanned}))?;
    let removed = b.id(json!({"op":"create","kind":"task","name":"Fixture plan with a block removed","day":"2026-10-21","start":480,"minutes":30}))?;
    b.send(json!({"op":"add_block","id":removed,"day":"2026-10-22","start":480,"minutes":30}))?;
    b.send(json!({"op":"remove_block","id":removed,"session":0}))?;
    b.tick(30);
    // Events drawn on the timeline, moved and resized.
    let tea = b.id(json!({"op":"create","kind":"event","name":"Fictional event: tea with the robot","day":"2026-10-20","start":1020,"minutes":45}))?;
    b.send(json!({"op":"move_block","id":tea,"day":"2026-10-20","start":1035,"minutes":60}))?;
    let talk = b.id(json!({"op":"create","kind":"event","name":"Fictional event: lantern talk","day":"2026-10-22","start":1140,"minutes":90}))?;
    b.send(json!({"op":"set_estimate","id":talk,"minutes":120}))?;
    // Routines: every day, weekdays, and a one-off.
    let sleep = b.id(json!({"op":"create","kind":"routine","name":"Example routine: sleep","day":"2026-10-01","start":1380,"minutes":480}))?;
    b.send(json!({"op":"set_repeat","id":sleep,"days":EVERY_DAY,"day":"2026-10-01"}))?;
    let commute = b.id(json!({"op":"create","kind":"routine","name":"Example routine: commute","day":"2026-10-01","start":480,"minutes":45}))?;
    b.send(json!({"op":"set_repeat","id":commute,"days":0b0001_1111,"day":"2026-10-01"}))?;
    b.send(json!({"op":"move_block","id":commute,"day":"2026-10-02","start":465,"minutes":40}))?;
    b.id(json!({"op":"create","kind":"routine","name":"Example one-off: the window cleaner visits","day":"2026-10-23","start":600,"minutes":60}))?;
    b.tick(60);
    // Changes after the fact.
    b.send(json!({"op":"set_deadline","id":undated[0],"deadline":"2026-11-05T09:00"}))?;
    b.send(json!({"op":"set_deadline","id":quick_dated,"deadline":null}))?;
    b.send(json!({"op":"set_severity","id":bare,"level":4}))?;
    b.send(json!({"op":"set_horizon","id":quick,"level":3}))?;
    b.send(json!({"op":"rename","id":quick,"name":"Quick add, renamed: oil the made-up tricycle"}))?;
    let forgotten = b.id(json!({"op":"create","kind":"task","name":"","day":"2026-10-20","start":300,"minutes":30}))?;
    b.send(json!({"op":"forget","id":forgotten}))?;
    b.send(json!({"op":"set_notes","text":"Fixture notes (invented).\n\tA tab, which the notepad turns into spaces.\nUnicode: \u{e9}t\u{e9}, \u{2603}, \u{1f9ea}."}))?;
    // The archive: every verdict, a week and a half on (a retirement may
    // not be dated more than a day past `now`).
    b.now = utc(2026, 10, 12, 8, 0, 0);
    let done = |b: &mut Builder, name: &str, deadline: Option<&str>| -> Result<u64, String> {
        let mut wire = json!({"op":"add","name":name,"importance":2});
        if let Some(deadline) = deadline {
            wire["deadline"] = json!(deadline);
        }
        b.id(wire)
    };
    let on_time = done(&mut b, "Archived fixture: finished before its deadline", Some("2026-10-10T12:00"))?;
    b.send(json!({"op":"add_block","id":on_time,"day":"2026-10-08","start":600,"minutes":60}))?;
    b.send(json!({"op":"complete","id":on_time,"at":"2026-10-08T11:00"}))?;
    let late = done(&mut b, "Archived fixture: finished after its deadline", Some("2026-10-02T12:00"))?;
    b.send(json!({"op":"complete","id":late,"at":"2026-10-05T09:15:00+02:00"}))?;
    let whenever = b.id(json!({"op":"add","name":"Archived fixture: undated and finished","time_importance":2}))?;
    b.tick(90);
    b.send(json!({"op":"complete","id":whenever}))?;
    let dropped = done(&mut b, "Archived fixture: dropped task", Some("2026-10-30T12:00"))?;
    b.send(json!({"op":"delete","id":dropped}))?;
    let gone_event = b.id(json!({"op":"create","kind":"event","name":"Archived fixture: removed event","day":"2026-10-03","start":600,"minutes":60}))?;
    b.send(json!({"op":"delete","id":gone_event,"at":"2026-10-03T12:00"}))?;
    let gone_routine = b.id(json!({"op":"create","kind":"routine","name":"Archived fixture: dropped routine","day":"2026-10-01","start":360,"minutes":30}))?;
    b.send(json!({"op":"set_repeat","id":gone_routine,"days":EVERY_DAY,"day":"2026-10-01"}))?;
    b.send(json!({"op":"delete","id":gone_routine}))?;
    let restored = done(&mut b, "Archived fixture: finished, then put back", None)?;
    b.send(json!({"op":"complete","id":restored,"at":"2026-10-04T08:00"}))?;
    b.board.load_archive()?;
    let key = b.board.archive.entries().iter().find(|row| row.id == restored).map(|row| text(row.archived_at));
    b.send(json!({"op":"restore","id":restored,"archived_at":key.ok_or("the restored row")?}))?;
    let erased = done(&mut b, "Archived fixture: forgotten for good", None)?;
    b.send(json!({"op":"complete","id":erased}))?;
    let key = b.board.archive.entries().iter().find(|row| row.id == erased).map(|row| text(row.archived_at));
    b.send(json!({"op":"forget_archived","id":erased,"archived_at":key.ok_or("the forgotten row")?}))?;
    b.tick(60);
    // Subscribed calendars, at made-up addresses that resolve nowhere.
    let team = b.id(json!({"op":"add_subscription","name":"Fixture team calendar","url":"https://example.invalid/fixture/team.ics"}))?;
    let holidays = b.id(json!({"op":"add_subscription","name":"Fixture holidays","url":"webcal://calendar.example.com/made-up/holidays.ics","color":[90,140,200,255]}))?;
    let club = b.id(json!({"op":"add_subscription","name":"","url":"https://example.com/synthetic/club.ics"}))?;
    let quiet = b.id(json!({"op":"add_subscription","name":"Fixture calendar, switched off","url":"https://example.invalid/fixture/off.ics"}))?;
    b.send(json!({"op":"rename_subscription","id":club,"name":"Fixture club (renamed)"}))?;
    b.send(json!({"op":"set_subscription_color","id":team,"color":[200,120,40,255]}))?;
    b.send(json!({"op":"set_subscription_enabled","id":quiet,"enabled":false}))?;
    let event = |subscription, on, start, end, all_day, free, summary: &str| OverlayEvent {
        subscription,
        day: on,
        start,
        end,
        all_day,
        free,
        summary: summary.to_string(),
        location: if all_day { String::new() } else { "Room 0 (invented)".to_string() },
        description: "Synthetic description for a fixture event.".to_string(),
    };
    let mut events = Vec::new();
    for offset in 0..14 {
        let on = day(2026, 10, 19) + Duration::days(offset);
        events.push(event(team, on, 600, 660, false, false, "Fixture stand-up"));
        if offset % 3 == 0 {
            events.push(event(club, on, 480, 1200, false, false, "Fixture club open day (a twelve-hour span)"));
        }
    }
    events.push(event(holidays, day(2026, 10, 26), 0, 1440, true, false, "Fixture holiday"));
    events.push(event(team, day(2026, 10, 20), 840, 900, false, true, "Fixture free-time block"));
    events.push(event(quiet, day(2026, 10, 20), 700, 760, false, false, "Never drawn: its calendar is off"));
    let at = utc(2026, 10, 20, 7, 50, 0);
    let status = vec![
        FetchStatus { subscription: team, at, outcome: FetchOutcome::Ok { events: 15, problems: Vec::new(), title: Some("Team (feed title)".into()) } },
        FetchStatus { subscription: holidays, at, outcome: FetchOutcome::Ok { events: 1, problems: vec!["Fixture problem: one event had no end".into()], title: None } },
        FetchStatus { subscription: club, at, outcome: FetchOutcome::Failed { why: "Fixture failure: the made-up server answered 404".into() } },
    ];
    let overlay = Overlay::sealed(events, status).covering(Some(Covered { from: day(2026, 8, 21), until: day(2027, 11, 24) }));
    b.board.adopt_overlay(overlay);
    b.board.save_overlay();
    // A reflow of the planned day, from mid-morning.
    b.send(json!({"op":"reflow","day":"2026-10-20","from":600}))?;
    write_settings(dir)
}

pub const WORDS: [&str; 16] = [
    "amber", "basalt", "cobalt", "dune", "ember", "fjord", "garnet", "harbor", "indigo", "juniper", "kelp", "lumen",
    "marble", "nectar", "opal", "pebble",
];

/// About ten thousand archive rows behind a realistic live set: the budget
/// fixture.
fn archive_10k(dir: &Path) -> Result<(), String> {
    let mut rng = SplitMix64::new(10_000);
    let mut b = Builder::new(dir, utc(2026, 10, 1, 6, 0, 0))?;
    let rows = 10_000u64;
    // The live ids sit above the archived ones, as on a board that has run
    // for years.
    b.board.number_from(rows + 1);
    let mut archive = Vec::with_capacity(rows as usize);
    // The rows run up to just before the snapshots' `now` (2026-10-20
    // 08:15Z), as on a board in daily use, so the budget snapshots place
    // ghosts: the gaps (about 3.5 hours on average, about four years in all)
    // are drawn first and the walk is anchored at its last row.
    let gaps: Vec<i64> = {
        let mut gap_rng = SplitMix64::new(10_001);
        (0..rows).map(|_| gap_rng.between(60, 357)).collect()
    };
    let mut at = utc(2026, 10, 20, 7, 0, 0) - Duration::minutes(gaps.iter().sum());
    for (id, gap) in (1..=rows).zip(gaps) {
        at += Duration::minutes(gap);
        let name = format!("Synthetic archived #{id}: {} {}", rng.pick(&WORDS), rng.pick(&WORDS));
        let created = at - Duration::hours(rng.between(1, 24 * 40));
        let kind = rng.below(20);
        let start = |rng: &mut SplitMix64| created + Duration::minutes(rng.between(60, 24 * 60 * 3) / 15 * 15);
        let mut item = Active {
            id,
            importance: None,
            time_importance: None,
            name,
            created,
            deadline: None,
            is_event: false,
            sessions: Vec::new(),
            planned_start: None,
            duration_minutes: None,
            recurrence: None,
        };
        let outcome = match kind {
            0..=1 => {
                item.is_event = true;
                item.deadline = Some(start(&mut rng));
                item.duration_minutes = Some(rng.between(2, 8) as u32 * 15);
                Outcome::Dropped
            }
            2 => {
                item.recurrence = Some(Recurrence {
                    days: rng.between(1, 127) as u8,
                    anchor: created.date_naive(),
                    start_minutes: rng.between(0, 95) as i32 * 15,
                    minutes: rng.between(2, 32) as u32 * 15,
                });
                Outcome::Dropped
            }
            _ => {
                if rng.chance(2, 3) {
                    item.deadline = Some(at + Duration::hours(rng.between(-72, 96)));
                    item.importance = Some(rng.below(5) as u8);
                } else {
                    item.time_importance = Some(rng.below(4) as u8);
                }
                // Work sessions in the three days before the row was archived
                // (not before the task existed).
                for _ in 0..rng.below(4) {
                    let minutes = rng.between(1, 8) * 15;
                    let start = (at - Duration::minutes(minutes + rng.between(0, 24 * 4 * 3) * 15)).max(created);
                    item.sessions.push(Session { start, minutes: minutes as u32 });
                }
                if rng.chance(1, 2) {
                    item.duration_minutes = Some(rng.between(1, 16) as u32 * 15);
                }
                if kind < 5 { Outcome::Dropped } else { Outcome::Finished }
            }
        };
        archive.push(Archived::retire(item, outcome, at));
    }
    b.board.archive.replace_with(archive);
    b.board.archive.persist(dir).map_err(|e| e.to_string())?;
    // The live set: about 150 items, built through the wire.
    b.now = utc(2026, 10, 1, 6, 0, 0);
    for n in 0..110 {
        let name = format!("Synthetic live task {n}: {} {}", rng.pick(&WORDS), rng.pick(&WORDS));
        let id = if rng.chance(1, 2) {
            let due = day(2026, 10, 1) + Duration::days(rng.between(0, 60));
            b.id(json!({"op":"add","name":name,"deadline":format!("{due}T{:02}:{:02}", rng.between(8, 20), rng.below(4) * 15),"importance":rng.below(5)}))?
        } else {
            b.id(json!({"op":"add","name":name,"time_importance":rng.below(4)}))?
        };
        if rng.chance(1, 3) {
            b.send(json!({"op":"set_estimate","id":id,"minutes":rng.between(1, 16) * 15}))?;
        }
        for _ in 0..rng.below(3) {
            let on = day(2026, 10, 15) + Duration::days(rng.between(0, 30));
            b.send(json!({"op":"add_block","id":id,"day":on.to_string(),"start":rng.between(28, 80) * 15,"minutes":rng.between(1, 8) * 15}))?;
        }
        b.tick(rng.between(5, 300));
    }
    for n in 0..35 {
        let on = day(2026, 10, 10) + Duration::days(rng.between(0, 50));
        b.id(json!({"op":"create","kind":"event","name":format!("Synthetic live event {n}: {}", rng.pick(&WORDS)),"day":on.to_string(),"start":rng.between(32, 84) * 15,"minutes":rng.between(2, 8) * 15}))?;
        b.tick(rng.between(5, 120));
    }
    for (n, (start, minutes, days)) in [(1380, 480, EVERY_DAY), (480, 45, 0b0001_1111), (720, 60, EVERY_DAY), (1080, 90, 0b0110_0000), (420, 30, 0b0001_0101)].into_iter().enumerate() {
        let id = b.id(json!({"op":"create","kind":"routine","name":format!("Synthetic routine {n}"),"day":"2026-10-01","start":start,"minutes":minutes}))?;
        b.send(json!({"op":"set_repeat","id":id,"days":days,"day":"2026-10-01"}))?;
    }
    b.send(json!({"op":"set_notes","text":"Synthetic notes for the ten-thousand-row board."}))?;
    write_settings(dir)
}

/// Daylight-saving edges in the pinned zone (Berlin: 2026-03-29 02:00 skips to
/// 03:00; 2026-10-25 03:00 falls back to 02:00) and at New York's transitions
/// (2026-03-08 07:00Z, 2026-11-01 06:00Z), midnight, year ends, and the first
/// and last years today's code accepts (1970, 9999).
fn dst_edges(dir: &Path) -> Result<(), String> {
    let mut b = Builder::new(dir, utc(2026, 11, 2, 10, 0, 0))?;
    // Berlin's gap and fold, through the timeline (day + minutes) ...
    let gap_task = b.id(json!({"op":"create","kind":"task","name":"DST fixture: block in the spring-forward gap","day":"2026-03-29","start":120,"minutes":60}))?;
    b.send(json!({"op":"add_block","id":gap_task,"day":"2026-03-29","start":150,"minutes":90}))?;
    b.id(json!({"op":"create","kind":"event","name":"DST fixture: event at 02:30 on the spring-forward day","day":"2026-03-29","start":150,"minutes":60}))?;
    b.id(json!({"op":"create","kind":"event","name":"DST fixture: event at 02:30 on the fall-back day","day":"2026-10-25","start":150,"minutes":60}))?;
    let fold_task = b.id(json!({"op":"create","kind":"task","name":"DST fixture: block across the repeated hour","day":"2026-10-25","start":120,"minutes":120}))?;
    b.send(json!({"op":"add_block","id":fold_task,"day":"2026-10-24","start":1410,"minutes":120}))?;
    // ... and through naive local wire times, and exact instants.
    b.id(json!({"op":"add","name":"DST fixture: due at a naive time in the gap","deadline":"2026-03-29T02:30","importance":1}))?;
    b.id(json!({"op":"add","name":"DST fixture: due at a naive time in the fold","deadline":"2026-10-25T02:30","importance":1}))?;
    b.id(json!({"op":"add","name":"DST fixture: due at the first 02:30 of the fold","deadline":"2026-10-25T02:30:00+02:00","importance":2}))?;
    b.id(json!({"op":"add","name":"DST fixture: due at the second 02:30 of the fold","deadline":"2026-10-25T02:30:00+01:00","importance":2}))?;
    b.id(json!({"op":"add","name":"DST fixture: due at New York's spring-forward instant","deadline":"2026-03-08T02:00:00-05:00","importance":3}))?;
    b.id(json!({"op":"add","name":"DST fixture: due at New York's first 01:30 of the fold","deadline":"2026-11-01T01:30:00-04:00","importance":3}))?;
    b.id(json!({"op":"add","name":"DST fixture: due at New York's second 01:30 of the fold","deadline":"2026-11-01T01:30:00-05:00","importance":3}))?;
    b.id(json!({"op":"create","kind":"event","name":"DST fixture: event at 01:30 on New York's fall-back day","day":"2026-11-01","start":90,"minutes":60}))?;
    b.id(json!({"op":"create","kind":"event","name":"DST fixture: event at 02:30 on New York's spring-forward day","day":"2026-03-08","start":150,"minutes":60}))?;
    // A routine whose start falls in the gap on one day of the year.
    let routine = b.id(json!({"op":"create","kind":"routine","name":"DST fixture: routine at 02:30 every day","day":"2026-03-01","start":150,"minutes":30}))?;
    b.send(json!({"op":"set_repeat","id":routine,"days":EVERY_DAY,"day":"2026-03-01"}))?;
    // Midnight and year ends.
    b.id(json!({"op":"create","kind":"task","name":"Edge fixture: block running past midnight","day":"2026-10-20","start":1410,"minutes":120}))?;
    b.id(json!({"op":"create","kind":"event","name":"Edge fixture: event at midnight","day":"2026-10-21","start":0,"minutes":30}))?;
    b.id(json!({"op":"create","kind":"event","name":"Edge fixture: last minute of the day","day":"2026-10-21","start":1439,"minutes":15}))?;
    b.id(json!({"op":"add","name":"Edge fixture: due at the last minute of 2026","deadline":"2026-12-31T23:59","importance":2}))?;
    b.id(json!({"op":"create","kind":"event","name":"Edge fixture: new year's first minute","day":"2027-01-01","start":0,"minutes":60}))?;
    b.id(json!({"op":"add","name":"Edge fixture: due in New York's 2026, Berlin's 2027","deadline":"2026-12-31T23:30:00-05:00","importance":2}))?;
    // The first and last years the wire accepts.
    b.id(json!({"op":"create","kind":"event","name":"Edge fixture: the first day of 1970","day":"1970-01-01","start":0,"minutes":60}))?;
    b.id(json!({"op":"add","name":"Edge fixture: due at local 1970-01-01 00:30 (1969 in UTC)","deadline":"1970-01-01T00:30","importance":0}))?;
    b.id(json!({"op":"create","kind":"event","name":"Edge fixture: the last block of 9999","day":"9999-12-31","start":1425,"minutes":15}))?;
    b.id(json!({"op":"add","name":"Edge fixture: due at 9999-12-31 23:59 local","deadline":"9999-12-31T23:59","importance":4}))?;
    // Archive rows at the edges, through `complete` and `delete` ...
    for (name, at) in [
        ("DST archive fixture: finished in the first 02:30 of Berlin's fold", "2026-10-25T02:30:00+02:00"),
        ("DST archive fixture: finished in the second 02:30 of Berlin's fold", "2026-10-25T02:30:00+01:00"),
        ("DST archive fixture: finished just after Berlin's gap", "2026-03-29T03:00:00+02:00"),
        ("DST archive fixture: finished in New York's first 01:30", "2026-11-01T01:30:00-04:00"),
        ("DST archive fixture: finished in New York's second 01:30", "2026-11-01T01:30:00-05:00"),
        ("Edge archive fixture: finished at the turn of the year", "2027-01-01T00:00:00+01:00"),
        ("Edge archive fixture: finished on 1970-01-01 local", "1970-01-01T00:15:00+01:00"),
    ] {
        let id = b.id(json!({"op":"add","name":name,"time_importance":0}))?;
        b.send(json!({"op":"complete","id":id,"at":at}))?;
    }
    let event = b.id(json!({"op":"create","kind":"event","name":"DST archive fixture: event deleted in the fold","day":"2026-10-25","start":150,"minutes":30}))?;
    b.send(json!({"op":"delete","id":event,"at":"2026-10-25T02:45:00+01:00"}))?;
    // ... and one the wire's one-day clamp cannot make: archived in 9999,
    // filed the way every row is filed.
    let late = Active {
        id: 9_000,
        importance: Some(1),
        time_importance: None,
        name: "Edge archive fixture: archived at 9999-12-31 23:59 local".into(),
        created: b.now,
        deadline: Some(utc(9999, 12, 31, 22, 0, 0)),
        is_event: false,
        sessions: vec![Session { start: utc(9999, 12, 31, 21, 0, 0), minutes: 45 }],
        planned_start: None,
        duration_minutes: Some(45),
        recurrence: None,
    };
    b.board.load_archive()?;
    b.board.archive.record(dir, Archived::retire(late, Outcome::Finished, utc(9999, 12, 31, 22, 59, 0))).map_err(|e| e.to_string())?;
    b.send(json!({"op":"set_notes","text":"DST and edge fixture notes."}))?;
    write_settings(dir)
}

/// Ids at and next to every boundary the design names, the largest values the
/// fields hold, names and notes at their limits, the most subscriptions, and
/// the duplicate subscription ids a reopen hands out (R1 §2 item 4).
fn id_limits(dir: &Path) -> Result<(), String> {
    let now = utc(2026, 10, 1, 6, 0, 0);
    // Ten calendars on an empty board, then a reopen and ten more: the
    // counter is seeded from items only, so the second ten get ids 1 to 10
    // again. Twenty is also the most a board keeps.
    for round in 0..2 {
        let mut b = Builder::new(dir, now)?;
        for n in 0..SUBSCRIPTIONS_MAX as u64 / 2 {
            let name = if round == 0 && n == 0 { "\u{e9}".repeat(NAME_MAX_CHARS) } else { format!("Limit fixture calendar {round}-{n}") };
            let url = if round == 1 && n == 9 {
                let head = "https://example.invalid/";
                format!("{head}{}", "x".repeat(2000 - head.len()))
            } else {
                format!("https://example.invalid/limits/{round}/{n}.ics")
            };
            b.id(json!({"op":"add_subscription","name":name,"url":url}))?;
        }
    }
    let mut b = Builder::new(dir, now)?;
    let limits: [u64; 15] = [
        1,
        (1 << 40) - 1,
        1 << 40,
        (1 << 40) + 1,
        (1 << 53) - 1,
        1 << 53,
        (1 << 53) + 1,
        TEMPORARY_ID_FLOOR - 1,
        TEMPORARY_ID_FLOOR,
        TEMPORARY_ID_FLOOR + 1,
        TEMPORARY_ID_FLOOR + 2,
        (1 << 63) - 1,
        1 << 63,
        u64::MAX - 1,
        u64::MAX,
    ];
    let mut items = Vec::new();
    for (n, id) in limits.into_iter().enumerate() {
        let name = match n {
            0 => "a".repeat(NAME_MAX_CHARS),
            1 => "\u{1f9ea}".repeat(NAME_MAX_CHARS),
            // 200 code points (today's limit counts `chars()`), 199 graphemes.
            2 => "\u{e9}".repeat(NAME_MAX_CHARS - 2) + "e\u{301}",
            _ => format!("Limit fixture item {id}"),
        };
        let created = utc(2026, 9, 1, 8, 0, 0) + Duration::nanoseconds(999_999_999) + Duration::minutes(n as i64);
        let mut item = Active {
            id,
            importance: Some((n % 5) as u8),
            time_importance: Some(3),
            name,
            created,
            deadline: Some(utc(2026, 10, 20, 10, 0, 0) + Duration::hours(n as i64)),
            is_event: false,
            sessions: Vec::new(),
            planned_start: None,
            duration_minutes: Some(1440),
            recurrence: None,
        };
        match n % 3 {
            0 => {
                for k in 0..24 {
                    item.sessions.push(Session { start: utc(2026, 10, 19, 22, 0, 0) + Duration::hours(k), minutes: 60 });
                }
            }
            1 => {
                item.is_event = true;
                item.importance = None;
                item.time_importance = None;
            }
            _ => {
                item.deadline = None;
                item.importance = None;
                item.duration_minutes = None;
                item.recurrence = Some(Recurrence { days: EVERY_DAY, anchor: day(1970, 1, 1), start_minutes: 1439, minutes: 1440 });
            }
        }
        items.push(item);
    }
    // Set directly, past the wire's own check: hold them to it here.
    if let Some(long) = items.iter().find(|item| item.name.chars().count() > NAME_MAX_CHARS) {
        return Err(format!("id-limits: item {} has a name past {NAME_MAX_CHARS} characters", long.id));
    }
    b.board.items = items;
    let mut notes: String = "Limit fixture notes. ".chars().cycle().take(NOTES_MAX_CHARS - 2).collect();
    notes.push_str("\u{1f9ea}\u{e9}");
    b.board.notes = notes;
    b.board.save_items_and_notes()?;
    b.board.load_archive()?;
    for (n, id) in [(1u64 << 40) + 1, (1 << 53) + 1, TEMPORARY_ID_FLOOR + 3, u64::MAX - 2].into_iter().enumerate() {
        let item = Active {
            id,
            importance: Some(4),
            time_importance: None,
            name: format!("Limit archive fixture {id}"),
            created: utc(2026, 9, 2, 8, 0, 0),
            deadline: Some(utc(2026, 9, 3, 8, 0, 0)),
            is_event: false,
            sessions: Vec::new(),
            planned_start: None,
            duration_minutes: None,
            recurrence: None,
        };
        let at = utc(2026, 9, 4, 8, 0, 0) + Duration::nanoseconds(123_456_789 * (n as i64 + 1));
        b.board.archive.record(dir, Archived::retire(item, Outcome::Finished, at)).map_err(|e| e.to_string())?;
    }
    write_settings(dir)
}

/// An archive with lines today's code cannot read, kept as bytes among good
/// ones, and an id that is both live and archived (V2-11: `retire` appends
/// the row before it saves the items, so a copy taken between the two holds
/// both).
fn unreadable(dir: &Path) -> Result<(), String> {
    let mut b = Builder::new(dir, utc(2026, 10, 1, 6, 0, 0))?;
    let both = b.id(json!({"op":"add","name":"Unreadable fixture: live, and archived too","deadline":"2026-10-22T12:00","importance":2}))?;
    b.id(json!({"op":"create","kind":"task","name":"Unreadable fixture: a planned task","day":"2026-10-20","start":600,"minutes":60}))?;
    b.id(json!({"op":"create","kind":"event","name":"Unreadable fixture: an event","day":"2026-10-21","start":720,"minutes":60}))?;
    b.send(json!({"op":"set_notes","text":"Unreadable fixture notes."}))?;
    let row = |id, name: &str, at: DateTime<Local>| {
        let item = Active {
            id,
            importance: Some(1),
            time_importance: None,
            name: name.to_string(),
            created: utc(2026, 9, 1, 8, 0, 0),
            deadline: None,
            is_event: false,
            sessions: Vec::new(),
            planned_start: None,
            duration_minutes: None,
            recurrence: None,
        };
        let mut line = serde_json::to_vec(&Archived::retire(item, Outcome::Finished, at)).expect("a row serialises");
        line.push(b'\n');
        line
    };
    let the_live_one = b.board.item(both).cloned().ok_or("the live item")?;
    let mut both_line = serde_json::to_vec(&Archived::retire(the_live_one, Outcome::Finished, utc(2026, 10, 1, 9, 0, 0))).expect("a row serialises");
    both_line.push(b'\n');
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend(row(901, "Unreadable fixture: good row before the damage", utc(2026, 9, 10, 8, 0, 0)));
    bytes.extend(b"{this is not json\n");
    bytes.extend(row(902, "Unreadable fixture: good row between bad ones", utc(2026, 9, 11, 8, 0, 0)));
    bytes.extend(b"[1,2,3]\n");
    bytes.extend(b"\"a string, not a row\"\n");
    bytes.extend(b"{\"id\":903,\"name\":\"Unreadable fixture: a row with fields missing\"}\n");
    bytes.extend(b"{\"id\":904,\"name\":\"Unreadable fixture: unknown outcome\",\"created\":\"2026-09-01T10:00:00+02:00\",\"deadline\":null,\"is_event\":false,\"importance\":1,\"inactivated\":\"2026-09-12T10:00:00+02:00\",\"outcome\":\"exploded\"}\n");
    bytes.extend(b"\n");
    bytes.extend(b"   \t \n");
    bytes.extend(b"\xff\xfe invalid UTF-8 \xc3\x28 in a line\n");
    // A good row behind a byte-order mark: today's reader does not skip one.
    bytes.extend(b"\xef\xbb\xbf");
    bytes.extend(row(905, "Unreadable fixture: a good row behind a byte-order mark", utc(2026, 9, 12, 8, 0, 0)));
    let mut crlf = row(906, "Unreadable fixture: a good row ending in CRLF", utc(2026, 9, 13, 8, 0, 0));
    crlf.insert(crlf.len() - 1, b'\r');
    bytes.extend(crlf);
    bytes.extend(row(907, &format!("Unreadable fixture: a readable row with a name far past the wire limit {}", "n".repeat(50_000)), utc(2026, 9, 14, 8, 0, 0)));
    bytes.extend(format!("{{\"over_long\":\"{}\"}}\n", "z".repeat(70_000)).as_bytes());
    bytes.extend(both_line);
    let mut last = row(908, "Unreadable fixture: last row, no final newline", utc(2026, 9, 15, 8, 0, 0));
    last.pop();
    bytes.extend(last);
    out::write(&dir.join(ARCHIVE_FILE), bytes)?;
    write_settings(dir)
}
