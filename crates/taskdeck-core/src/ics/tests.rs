//! Today's ICS parser tests (ics.rs:1418-1775), ported with the zone
//! argument, and the zone clauses of core.md §14.6 (CORE-READ-004).

use super::*;
use crate::time::{self, FixedZone, TableZone};

/// Today's tests ran in the zone of the process that ran `cargo test`. Every
/// ported test passes this zone where that one was read (`read` and every
/// `parse` call): +03:00, as in the ported planner tests. No expected value
/// depends on which zone it is except the defined-`TZID` test, which computes
/// its expectation in `ZONE` as today's computed it in the process zone.
const ZONE: FixedZone = FixedZone(3 * 3600);


fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("a real date")
}

fn wrap(body: &str) -> String {
    format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{body}\r\nEND:VCALENDAR\r\n")
}

fn read(body: &str) -> Parsed {
    parse(&wrap(body), day(2026, 1, 1), day(2027, 1, 1), Limits::default(), &ZONE).expect("a calendar")
}

fn event(body: &str) -> String {
    format!("BEGIN:VEVENT\r\nUID:x@test\r\n{body}\r\nEND:VEVENT")
}

#[test]
fn a_folded_line_is_one_property_again_whether_it_broke_on_crlf_or_lf() {
    let crlf = unfold("SUMMARY:one\r\n  two\r\n", 100).expect("unfolds");
    let lf = unfold("SUMMARY:one\n  two\n", 100).expect("unfolds");
    // One space of the fold belongs to the fold; the second is the value's.
    assert_eq!(crlf, vec!["SUMMARY:one two"]);
    assert_eq!(crlf, lf);
    assert!(unfold("A:1\nB:2\nC:3\n", 2).is_err(), "a longer file than allowed is refused");
}

#[test]
fn a_byte_order_mark_does_not_hide_the_begin_line() {
    // Outlook writes one, and `\u{feff}BEGIN` is not `BEGIN`.
    let text = format!("\u{feff}{}", wrap(&event("SUMMARY:Standup\r\nDTSTART:20260910T090000Z")));
    let parsed = parse(&text, day(2026, 1, 1), day(2027, 1, 1), Limits::default(), &ZONE).expect("a calendar");
    assert_eq!(parsed.occurrences.len(), 1);
}

#[test]
fn a_colon_inside_a_quoted_parameter_does_not_end_the_property_name() {
    let line = split_line("DTSTART;TZID=\"Europe/Helsinki\":19980118T230000").expect("splits");
    assert_eq!(line.name, "DTSTART");
    assert_eq!(line.params, vec![("TZID".to_string(), "Europe/Helsinki".to_string())]);
    assert_eq!(line.value, "19980118T230000");
    assert!(split_line("no colon here").is_none());
}

#[test]
fn a_summary_comes_back_the_way_our_own_feed_would_have_escaped_it() {
    // The exact inverse of `phone::escape_text`.
    assert_eq!(unescape_text(r"a\;b\,c\\d\ne"), "a;b,c\\d\ne");
    // An escape this does not know is kept as written rather than eaten.
    assert_eq!(unescape_text(r"50\% and a trailing \"), r"50\% and a trailing \");
}

#[test]
fn an_imported_name_is_detabbed_flattened_and_bounded_like_every_other_name() {
    assert_eq!(imported_name("a\tb"), "a    b");
    assert_eq!(imported_name(r"one\ntwo"), "one two");
    let long = "x".repeat(NAME_MAX_CHARS + 50);
    assert_eq!(imported_name(&long).chars().count(), NAME_MAX_CHARS);
    // Cut on a character, never on a byte.
    let wide = "日".repeat(NAME_MAX_CHARS + 10);
    assert_eq!(imported_name(&wide).chars().count(), NAME_MAX_CHARS);
}

#[test]
fn a_file_of_nothing_but_begin_lines_is_refused_rather_than_recursed_into() {
    // A recursive parser meets this with a stack overflow, which
    // `panic = "abort"` cannot catch. This is the reason for the shape of
    // `harvest`, and the reason the obvious crate was not taken.
    let body = "BEGIN:X\r\n".repeat(50_000);
    let text = format!("BEGIN:VCALENDAR\r\n{body}");
    let error = parse(&text, day(2026, 1, 1), day(2027, 1, 1), Limits::default(), &ZONE).unwrap_err();
    assert!(error.contains("deep"), "{error}");
}

#[test]
fn a_file_that_is_not_a_calendar_at_all_is_refused_and_one_bad_event_is_only_skipped() {
    assert!(parse("<html>404</html>", day(2026, 1, 1), day(2027, 1, 1), Limits::default(), &ZONE).is_err());
    // But one unreadable event costs only itself.
    let parsed = read(&format!(
        "{}\r\n{}",
        event("SUMMARY:No start at all"),
        event("SUMMARY:Fine\r\nDTSTART:20260910T090000Z")
    ));
    assert_eq!(parsed.occurrences.len(), 1);
    assert_eq!(parsed.occurrences.first().map(|o| o.name.as_str()), Some("Fine"));
    assert!(parsed.problems.iter().any(|p| p.contains("no start")), "{:?}", parsed.problems);
}

#[test]
fn a_value_this_parser_half_understands_is_skipped_rather_than_guessed_at() {
    // An hourly rule is not read, and the event does not silently become
    // a one-off drawn at the wrong cadence.
    let parsed = read(&event("SUMMARY:Ping\r\nDTSTART:20260910T090000Z\r\nRRULE:FREQ=HOURLY;INTERVAL=2"));
    assert!(parsed.problems.iter().any(|p| p.contains("repeat rule")), "{:?}", parsed.problems);
}

#[test]
fn an_event_that_runs_past_midnight_is_cut_at_midnight() {
    let parsed = read(&event("SUMMARY:Night shift\r\nDTSTART:20260910T230000\r\nDTEND:20260911T010000"));
    assert_eq!(parsed.occurrences.len(), 2);
    let first = parsed.occurrences.first().expect("a first");
    let second = parsed.occurrences.get(1).expect("a second");
    assert_eq!((first.day, first.start, first.end), (day(2026, 9, 10), 23 * 60, planner::DAY_MINUTES));
    assert_eq!((second.day, second.start, second.end), (day(2026, 9, 11), 0, 60));
}

#[test]
fn an_all_day_event_covers_its_days_exclusive_of_dtend_and_anchors_none_of_them() {
    // RFC 5545's DTEND is exclusive for a DATE value: this is one day.
    let parsed = read(&event("SUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20260907\r\nDTEND;VALUE=DATE:20260908"));
    assert_eq!(parsed.occurrences.len(), 1);
    let only = parsed.occurrences.first().expect("one");
    assert_eq!(only.day, day(2026, 9, 7));
    assert!(only.all_day);
    assert!(!only.is_busy(), "an all-day band is shown, never planned around");
}

#[test]
fn a_duration_stands_in_for_a_missing_dtend_and_a_negative_one_is_refused() {
    assert_eq!(read_duration_minutes("PT45M"), Some(45));
    assert_eq!(read_duration_minutes("P2DT3H30M"), Some(2 * 24 * 60 + 3 * 60 + 30));
    assert_eq!(read_duration_minutes("P1W"), Some(7 * 24 * 60));
    // Seconds round up, so a ninety-second event still takes a minute.
    assert_eq!(read_duration_minutes("PT90S"), Some(2));
    assert_eq!(read_duration_minutes("-PT1H"), None);
    assert_eq!(read_duration_minutes("P999999999999999999999D"), None);

    let parsed = read(&event("SUMMARY:Call\r\nDTSTART:20260910T090000\r\nDURATION:PT45M"));
    let only = parsed.occurrences.first().expect("one");
    assert_eq!((only.start, only.end), (9 * 60, 9 * 60 + 45));
}

#[test]
fn an_event_with_neither_dtend_nor_duration_gets_the_length_our_own_feed_gives_one() {
    let parsed = read(&event("SUMMARY:Marker\r\nDTSTART:20260910T090000"));
    let only = parsed.occurrences.first().expect("one");
    assert_eq!(only.end - only.start, planner::SNAP_MINUTES);
}

#[test]
fn a_weekly_rule_lands_on_every_day_its_byday_names_and_stops_at_until() {
    let parsed = read(&event(
        "SUMMARY:Standup\r\nDTSTART:20260907T090000\r\nDTEND:20260907T091500\r\n\
         RRULE:FREQ=WEEKLY;BYDAY=MO,WE;UNTIL=20260917T000000Z",
    ));
    let days: Vec<NaiveDate> = parsed.occurrences.iter().map(|o| o.day).collect();
    assert_eq!(
        days,
        vec![day(2026, 9, 7), day(2026, 9, 9), day(2026, 9, 14), day(2026, 9, 16)],
        "Mondays and Wednesdays, and nothing past UNTIL"
    );
}

#[test]
fn count_is_counted_from_dtstart_not_from_the_window() {
    // The window opens after the series did: COUNT has already been spent
    // on the occurrences before it, so only the tail shows.
    let text = wrap(&event("SUMMARY:Five\r\nDTSTART:20260101T090000\r\nRRULE:FREQ=DAILY;COUNT=5"));
    let parsed = parse(&text, day(2026, 1, 3), day(2026, 12, 31), Limits::default(), &ZONE).expect("a calendar");
    let days: Vec<NaiveDate> = parsed.occurrences.iter().map(|o| o.day).collect();
    assert_eq!(days, vec![day(2026, 1, 3), day(2026, 1, 4), day(2026, 1, 5)]);
}

#[test]
fn a_monthly_rule_on_the_thirty_first_skips_february_rather_than_clamping_into_it() {
    let parsed = read(&event(
        "SUMMARY:Rent\r\nDTSTART:20260131T090000\r\nRRULE:FREQ=MONTHLY;BYMONTHDAY=31;COUNT=4",
    ));
    let days: Vec<NaiveDate> = parsed.occurrences.iter().map(|o| o.day).collect();
    // January, March, May, July — never the 28th of February.
    assert!(days.contains(&day(2026, 1, 31)));
    assert!(days.contains(&day(2026, 3, 31)));
    assert!(!days.iter().any(|d| d.month() == 2), "{days:?}");
}

#[test]
fn a_last_weekday_of_the_month_rule_lands_on_the_last_one() {
    let parsed = read(&event(
        "SUMMARY:Retro\r\nDTSTART:20260130T160000\r\nRRULE:FREQ=MONTHLY;BYDAY=-1FR;COUNT=3",
    ));
    let days: Vec<NaiveDate> = parsed.occurrences.iter().map(|o| o.day).collect();
    assert_eq!(days, vec![day(2026, 1, 30), day(2026, 2, 27), day(2026, 3, 27)]);
}

#[test]
fn a_yearly_rule_on_the_twenty_ninth_of_february_recurs_only_in_leap_years() {
    let text = wrap(&event("SUMMARY:Leap\r\nDTSTART:20240229T090000\r\nRRULE:FREQ=YEARLY"));
    let parsed = parse(&text, day(2024, 1, 1), day(2029, 12, 31), Limits::default(), &ZONE).expect("a calendar");
    let days: Vec<NaiveDate> = parsed.occurrences.iter().map(|o| o.day).collect();
    assert_eq!(days, vec![day(2024, 2, 29), day(2028, 2, 29)]);
}

#[test]
fn exdate_removes_the_occurrence_it_names() {
    let parsed = read(&event(
        "SUMMARY:Standup\r\nDTSTART:20260907T090000\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEXDATE:20260908T090000",
    ));
    let days: Vec<NaiveDate> = parsed.occurrences.iter().map(|o| o.day).collect();
    assert_eq!(days, vec![day(2026, 9, 7), day(2026, 9, 9)]);
}

#[test]
fn a_moved_occurrence_replaces_the_one_its_recurrence_id_names_rather_than_joining_it() {
    // Without this the meeting is drawn twice: once where it was and once
    // where it went. It happens on the first real calendar anyone adds.
    let body = String::from(
        "BEGIN:VEVENT\r\nUID:s@test\r\nSUMMARY:Standup\r\nDTSTART:20260907T090000\r\n\
         RRULE:FREQ=DAILY;COUNT=2\r\nEND:VEVENT\r\n\
         BEGIN:VEVENT\r\nUID:s@test\r\nRECURRENCE-ID:20260908T090000\r\nSUMMARY:Standup (late)\r\n\
         DTSTART:20260908T140000\r\nEND:VEVENT"
    );
    let parsed = read(&body);
    let times: Vec<(NaiveDate, i32, &str)> =
        parsed.occurrences.iter().map(|o| (o.day, o.start, o.name.as_str())).collect();
    assert_eq!(
        times,
        vec![(day(2026, 9, 7), 9 * 60, "Standup"), (day(2026, 9, 8), 14 * 60, "Standup (late)")]
    );
}

#[test]
fn a_cancelled_override_removes_its_occurrence_and_leaves_the_series_standing() {
    let body = String::from(
        "BEGIN:VEVENT\r\nUID:s@test\r\nSUMMARY:Standup\r\nDTSTART:20260907T090000\r\n\
         RRULE:FREQ=DAILY;COUNT=3\r\nEND:VEVENT\r\n\
         BEGIN:VEVENT\r\nUID:s@test\r\nRECURRENCE-ID:20260908T090000\r\nSTATUS:CANCELLED\r\n\
         DTSTART:20260908T090000\r\nEND:VEVENT"
    );
    let days: Vec<NaiveDate> = read(&body).occurrences.iter().map(|o| o.day).collect();
    assert_eq!(days, vec![day(2026, 9, 7), day(2026, 9, 9)]);
}

#[test]
fn a_tzid_the_file_defines_is_read_from_that_definition_and_not_from_this_machine() {
    // A zone two hours ahead of UTC all year. 09:00 there is 07:00 UTC.
    let body = format!(
        "BEGIN:VTIMEZONE\r\nTZID:Test/Fixed\r\nBEGIN:STANDARD\r\nDTSTART:19700101T000000\r\n\
         TZOFFSETFROM:+0200\r\nTZOFFSETTO:+0200\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n{}",
        event("SUMMARY:Abroad\r\nDTSTART;TZID=Test/Fixed:20260910T090000\r\nDURATION:PT1H")
    );
    let parsed = read(&body);
    let only = parsed.occurrences.first().expect("one");
    // Today: 07:00 UTC converted to the process zone; here the local day and
    // minutes of that instant in `ZONE` (core.md §5.3).
    let expected =
        Instant::from_chrono(Utc.with_ymd_and_hms(2026, 9, 10, 7, 0, 0).single().expect("an instant")).expect("in range");
    assert_eq!(only.day, time::local_day(expected, &ZONE));
    assert_eq!(only.start, time::local_minutes(expected, &ZONE));
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
}

// Today's test was `..._and_says_which_one_it_was`, and asserted the line
// quoted the TZID. Brief c3 (bound 5, CORE-GEN-010) forbids quoting a TZID:
// the expected start is today's; the line now says that a zone was guessed at
// without naming it.
// spec: CORE-GEN-010
#[test]
fn a_tzid_the_file_never_defined_falls_back_to_local_and_says_so_without_quoting_it() {
    let parsed = read(&event(
        "SUMMARY:Somewhere\r\nDTSTART;TZID=W. Europe Standard Time:20260910T090000\r\nDURATION:PT1H",
    ));
    let only = parsed.occurrences.first().expect("one");
    assert_eq!(only.start, 9 * 60, "read as this computer's wall time");
    assert_eq!(parsed.problems, vec![UNDEFINED_ZONE.to_string()], "the calendar admits it guessed at a zone");
    assert!(parsed.problems.iter().all(|p| !p.contains("Europe")), "{:?}", parsed.problems);
}

#[test]
fn a_rule_anchored_centuries_back_stops_at_the_step_budget_and_reports_it() {
    let limits = Limits { max_rule_steps: 500, ..Limits::default() };
    let text = wrap(&event("SUMMARY:Ancient\r\nDTSTART:16010101T090000\r\nRRULE:FREQ=DAILY"));
    let parsed = parse(&text, day(2026, 1, 1), day(2026, 12, 31), limits, &ZONE).expect("a calendar");
    assert!(parsed.problems.iter().any(|p| p.contains("further")), "{:?}", parsed.problems);
}

#[test]
fn a_date_at_the_end_of_what_chrono_can_hold_ends_the_walk_instead_of_the_process() {
    let text = wrap(&event("SUMMARY:Forever\r\nDTSTART:99991230T090000\r\nRRULE:FREQ=DAILY"));
    let parsed = parse(&text, day(9999, 1, 1), day(9999, 12, 31), Limits::default(), &ZONE).expect("a calendar");
    assert!(parsed.occurrences.len() <= 2, "the walk ended at the edge of the calendar");
}

#[test]
fn two_fetches_of_a_calendar_that_did_not_change_parse_equal_whatever_order_it_listed() {
    // This is the whole of the rule that an unchanged refresh must not
    // move the board version.
    let one = read(&format!(
        "{}\r\n{}",
        event("SUMMARY:A\r\nDTSTART:20260910T090000"),
        event("SUMMARY:B\r\nDTSTART:20260910T100000")
    ));
    let other = read(&format!(
        "{}\r\n{}",
        event("SUMMARY:B\r\nDTSTART:20260910T100000"),
        event("SUMMARY:A\r\nDTSTART:20260910T090000")
    ));
    assert_eq!(one.occurrences, other.occurrences);
}

#[test]
fn a_thousand_unreadable_events_produce_one_line_of_problems_not_a_thousand() {
    let body = (0..1000).map(|_| event("SUMMARY:Nothing")).collect::<Vec<_>>().join("\r\n");
    let parsed = read(&body);
    assert_eq!(parsed.problems.len(), 1);
    assert!(parsed.problems.first().is_some_and(|p| p.contains("1000 times")), "{:?}", parsed.problems);
}

#[test]
fn where_it_is_comes_off_its_own_field_rather_than_the_end_of_the_summary() {
    // The shape a real university feed sends, verbatim but for the names:
    // the room is both buried at the end of a hundred-character summary
    // and sitting in its own short field. Reading LOCATION is what stops
    // the room being the first thing an ellipsis eats.
    let parsed = read(&event(
        "SUMMARY:MS-A0301\\, Differentiaali- ja integraalilaskenta 3\\, Luento-opetus 23.2.–15.4.2026 - Luento - L01 - E-sali - Y124\r\n\
         DTSTART:20260910T061500Z\r\nDURATION:PT1H45M\r\nLOCATION:E-sali - Y124\r\nTRANSP:OPAQUE",
    ));
    let only = parsed.occurrences.first().expect("one");
    assert_eq!(only.location, "E-sali - Y124");
    assert!(only.name.starts_with("MS-A0301, Differentiaali"), "{}", only.name);
    assert_eq!(only.end - only.start, 105, "PT1H45M");
    assert!(only.description.is_empty(), "that feed sends none, and none is fine");

    // A description is flattened to one line and bounded: it is a hover,
    // not a wall. `\t` is not one of RFC 5545's escapes, so it survives as
    // written rather than being guessed at — the same rule `unescape_text`
    // keeps for `\%`.
    let long = read(&event(
        "SUMMARY:Standup\r\nDTSTART:20260910T090000\r\nDESCRIPTION:line one\\nline two\\n\\nand  spaces",
    ));
    let noted = long.occurrences.first().expect("one");
    assert_eq!(noted.description, "line one line two and spaces");
    assert!(!noted.description.contains('\n'), "a hover is one line");
}

#[test]
fn a_transparent_event_is_shown_and_never_planned_around() {
    // Our own feed writes TRANSP:TRANSPARENT on a due marker, and a
    // household may well subscribe TaskDeck to a calendar TaskDeck feeds.
    let parsed = read(&event(
        "SUMMARY:due: Tax return\r\nDTSTART:20260910T090000\r\nDURATION:PT15M\r\nTRANSP:TRANSPARENT",
    ));
    let only = parsed.occurrences.first().expect("one");
    assert!(only.free);
    assert!(!only.is_busy());
}

#[test]
fn the_calendar_names_itself_when_the_file_says_so() {
    let parsed = read(&format!(
        "X-WR-CALNAME:Work\r\n{}",
        event("SUMMARY:Standup\r\nDTSTART:20260910T090000")
    ));
    assert_eq!(parsed.name.as_deref(), Some("Work"));
}

/* ───────────── New: the zone argument (CORE-READ-004, brief c2) ───────────── */

fn utc_ms(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> i64 {
    Utc.with_ymd_and_hms(y, m, d, hour, minute, 0).single().expect("an instant").timestamp_millis()
}

/// A zone like central Europe's in 2026: +01:00, +02:00 from 01:00 UTC on
/// 29 March, +01:00 again from 01:00 UTC on 25 October.
fn europe_2026() -> TableZone {
    TableZone::new(3600, vec![(utc_ms(2026, 3, 29, 1, 0), 7200), (utc_ms(2026, 10, 25, 1, 0), 3600)])
        .expect("a valid table")
}

/// The day and start minute of the one occurrence called `name`.
fn when(parsed: &Parsed, name: &str) -> (NaiveDate, i32) {
    let found: Vec<&Occurrence> = parsed.occurrences.iter().filter(|o| o.name == name).collect();
    assert_eq!(found.len(), 1, "{name}: {:?}", parsed.occurrences);
    found.first().map(|o| (o.day, o.start)).unwrap_or_default()
}

/// A file with a zone it defines as UTC all year, and one event of each kind:
/// UTC, `TZID`, floating, and UTC on both sides of both 2026 transitions.
fn mixed_calendar() -> String {
    let events = [
        "SUMMARY:utc-mar-28\r\nDTSTART:20260328T120000Z",
        "SUMMARY:utc-mar-30\r\nDTSTART:20260330T120000Z",
        "SUMMARY:tzid-mar-28\r\nDTSTART;TZID=Test/UTC:20260328T120000",
        "SUMMARY:tzid-mar-30\r\nDTSTART;TZID=Test/UTC:20260330T120000",
        "SUMMARY:floating-mar-28\r\nDTSTART:20260328T120000",
        "SUMMARY:floating-mar-30\r\nDTSTART:20260330T120000",
        "SUMMARY:spring-before\r\nDTSTART:20260329T003000Z",
        "SUMMARY:spring-after\r\nDTSTART:20260329T013000Z",
        "SUMMARY:autumn-before\r\nDTSTART:20261025T003000Z",
        "SUMMARY:autumn-after\r\nDTSTART:20261025T013000Z",
    ];
    let body: Vec<String> = events.iter().enumerate().map(|(n, e)| {
        format!("BEGIN:VEVENT\r\nUID:{n}@test\r\n{e}\r\nDURATION:PT30M\r\nEND:VEVENT")
    }).collect();
    wrap(&format!(
        "BEGIN:VTIMEZONE\r\nTZID:Test/UTC\r\nBEGIN:STANDARD\r\nDTSTART:19700101T000000\r\n\
         TZOFFSETFROM:+0000\r\nTZOFFSETTO:+0000\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n{}",
        body.join("\r\n")
    ))
}

fn parse_in(text: &str, zone: &(impl Zone + ?Sized)) -> Parsed {
    parse(text, day(2026, 1, 1), day(2027, 1, 1), Limits::default(), zone).expect("a calendar")
}

// spec: CORE-READ-004
#[test]
fn utc_and_tzid_times_are_read_in_the_zone_given_and_floating_ones_in_none() {
    let text = mixed_calendar();
    let utc = parse_in(&text, &FixedZone(0));
    let new_york = parse_in(&text, &FixedZone(-5 * 3600));
    assert!(utc.problems.is_empty(), "{:?}", utc.problems);

    // UTC: 12:00Z is 12:00 at +00:00 and 07:00 at -05:00.
    assert_eq!(when(&utc, "utc-mar-28"), (day(2026, 3, 28), 12 * 60));
    assert_eq!(when(&new_york, "utc-mar-28"), (day(2026, 3, 28), 7 * 60));
    // TZID: the file's zone gives the instant, the zone given reads it.
    assert_eq!(when(&utc, "tzid-mar-30"), (day(2026, 3, 30), 12 * 60));
    assert_eq!(when(&new_york, "tzid-mar-30"), (day(2026, 3, 30), 7 * 60));
    // Floating: the wall time as written, whatever the zone.
    assert_eq!(when(&utc, "floating-mar-28"), (day(2026, 3, 28), 12 * 60));
    assert_eq!(when(&new_york, "floating-mar-28"), (day(2026, 3, 28), 12 * 60));
    // A UTC time just after midnight is the evening before at -05:00.
    assert_eq!(when(&utc, "spring-before"), (day(2026, 3, 29), 30));
    assert_eq!(when(&new_york, "spring-before"), (day(2026, 3, 28), 19 * 60 + 30));
}

// spec: CORE-READ-004
#[test]
fn a_table_zone_moves_utc_and_tzid_times_across_its_transitions() {
    let parsed = parse_in(&mixed_calendar(), &europe_2026());
    // Before the spring transition +01:00, after it +02:00.
    assert_eq!(when(&parsed, "utc-mar-28"), (day(2026, 3, 28), 13 * 60));
    assert_eq!(when(&parsed, "utc-mar-30"), (day(2026, 3, 30), 14 * 60));
    assert_eq!(when(&parsed, "tzid-mar-28"), (day(2026, 3, 28), 13 * 60));
    assert_eq!(when(&parsed, "tzid-mar-30"), (day(2026, 3, 30), 14 * 60));
    // Floating times do not move.
    assert_eq!(when(&parsed, "floating-mar-28"), (day(2026, 3, 28), 12 * 60));
    assert_eq!(when(&parsed, "floating-mar-30"), (day(2026, 3, 30), 12 * 60));
    // 00:30Z is 01:30 at +01:00; 01:30Z is 03:30 at +02:00: 02:xx is skipped.
    assert_eq!(when(&parsed, "spring-before"), (day(2026, 3, 29), 90));
    assert_eq!(when(&parsed, "spring-after"), (day(2026, 3, 29), 3 * 60 + 30));
    // In the autumn fold both read 02:30: 00:30Z at +02:00, 01:30Z at +01:00.
    assert_eq!(when(&parsed, "autumn-before"), (day(2026, 10, 25), 2 * 60 + 30));
    assert_eq!(when(&parsed, "autumn-after"), (day(2026, 10, 25), 2 * 60 + 30));
}

// spec: CORE-READ-004
#[test]
fn a_utc_until_is_read_in_the_zone_given() {
    // 23:00Z on 12 September is still the 12th at +00:00 and the 13th at +03:00.
    let text = wrap(&event("SUMMARY:Daily\r\nDTSTART:20260910T090000\r\nRRULE:FREQ=DAILY;UNTIL=20260912T230000Z"));
    let days = |zone: &dyn Zone| -> Vec<NaiveDate> {
        parse_in(&text, zone).occurrences.iter().map(|o| o.day).collect()
    };
    assert_eq!(days(&FixedZone(0)), vec![day(2026, 9, 10), day(2026, 9, 11), day(2026, 9, 12)]);
    assert_eq!(days(&ZONE), vec![day(2026, 9, 10), day(2026, 9, 11), day(2026, 9, 12), day(2026, 9, 13)]);
}

// spec: CORE-READ-004
// spec: CORE-GEN-010
#[test]
fn an_undefined_tzid_is_read_as_wall_time_in_every_zone() {
    let text = wrap(&event("SUMMARY:Somewhere\r\nDTSTART;TZID=Nowhere/Undefined:20260910T090000\r\nDURATION:PT1H"));
    for zone in [&FixedZone(0) as &dyn Zone, &FixedZone(-5 * 3600), &europe_2026()] {
        let parsed = parse_in(&text, zone);
        assert_eq!(when(&parsed, "Somewhere"), (day(2026, 9, 10), 9 * 60));
        // One line that says so without quoting the TZID (brief c3, CORE-GEN-010).
        assert_eq!(parsed.problems, vec![UNDEFINED_ZONE.to_string()]);
    }
}

// spec: CORE-READ-004
#[test]
fn a_utc_time_before_1970_is_read_with_the_offset_at_the_start_of_the_range() {
    // Outside core.md §5.1's instants (lead ruling, spec change 5, §14.6): the
    // zone's offset at 1970-01-01T00:00Z is taken.
    let text = wrap(&event("SUMMARY:Old\r\nDTSTART:19600101T120000Z\r\nDURATION:PT1H"));
    let parsed = parse(&text, day(1959, 1, 1), day(1961, 1, 1), Limits::default(), &europe_2026()).expect("a calendar");
    assert_eq!(when(&parsed, "Old"), (day(1960, 1, 1), 13 * 60));
}

/* ─────────────── New: the bounds of brief c3 (core.md §14.6) ─────────────── */

#[test]
fn a_by_list_is_read_without_duplicates_and_holds_at_most_sixty_four_values() {
    let rule = read_recurrence("FREQ=MONTHLY;BYDAY=MO,MO,+1MO,1MO,-1FR;BYMONTH=1,1,2;BYSETPOS=1,1;BYMONTHDAY=3,3")
        .expect("a rule");
    assert_eq!(rule.by_day, vec![(None, Weekday::Mon), (Some(1), Weekday::Mon), (Some(-1), Weekday::Fri)]);
    assert_eq!(rule.by_month, vec![1, 2]);
    assert_eq!(rule.by_set_pos, vec![1]);
    assert_eq!(rule.by_month_day, vec![3]);

    let list = |n: i32| (1..=n).map(|v| v.to_string()).collect::<Vec<_>>().join(",");
    assert_eq!(read_recurrence(&format!("FREQ=YEARLY;BYSETPOS={}", list(MAX_BY_VALUES as i32))).map(|r| r.by_set_pos.len()), Some(64));
    assert_eq!(read_recurrence(&format!("FREQ=YEARLY;BYSETPOS={}", list(MAX_BY_VALUES as i32 + 1))), None);
    // Every month day counted both ways is 62 values, the longest real list.
    let both: Vec<String> = (1..=31).map(|d| d.to_string()).chain((1..=31).map(|d| format!("-{d}"))).collect();
    assert_eq!(read_recurrence(&format!("FREQ=MONTHLY;BYMONTHDAY={}", both.join(","))).map(|r| r.by_month_day.len()), Some(62));
}

#[test]
fn the_nth_weekday_is_todays_filter_of_every_day_of_the_month() {
    let weekdays = [Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri, Weekday::Sat, Weekday::Sun];
    for year in [1, 1900, 2024, 2026, 2100, 9999] {
        for month in 1..=12 {
            for weekday in weekdays {
                let today: Vec<NaiveDate> = (1..=days_in(year, month))
                    .filter_map(|d| NaiveDate::from_ymd_opt(year, month, d))
                    .filter(|date| date.weekday() == weekday)
                    .collect();
                assert_eq!(nth_weekday(year, month, weekday, None), today, "{year}-{month} {weekday:?}");
                for n in [-6i32, -5, -1, 1, 4, 5, 6] {
                    let picked = if n > 0 {
                        today.get(n as usize - 1).copied()
                    } else {
                        today.len().checked_sub(n.unsigned_abs() as usize).and_then(|i| today.get(i)).copied()
                    };
                    assert_eq!(nth_weekday(year, month, weekday, Some(n)), picked.into_iter().collect::<Vec<_>>());
                }
            }
        }
    }
}

#[test]
fn problems_hold_one_line_per_kind_and_at_most_sixty_four() {
    let mut problems = Problems::default();
    for _ in 0..3 {
        problems.note(NO_START);
    }
    problems.note(NO_END);
    assert_eq!(problems.lines(), vec![format!("{NO_START} (3 times)"), NO_END.to_string()]);

    // Should there ever be more kinds than the bound, the lines stop at it.
    let kinds: Vec<&'static str> =
        (0..MAX_PROBLEM_LINES + 10).map(|i| -> &'static str { format!("kind {i}").leak() }).collect();
    let mut problems = Problems::default();
    for &kind in &kinds {
        problems.note(kind);
        problems.note(kind);
    }
    assert_eq!(problems.lines().len(), MAX_PROBLEM_LINES);
}

/// A zone `Z` whose points are the observances given, as `(DTSTART, offset
/// in hours)`, and the parse window of `read`.
fn zones_of(observances: &[(&str, i64)]) -> Vec<(NaiveDateTime, TimeDelta)> {
    let body: String = observances
        .iter()
        .map(|(at, hours)| {
            format!(
                "BEGIN:STANDARD\r\nDTSTART:{at}\r\nTZOFFSETFROM:+0000\r\nTZOFFSETTO:+{hours:02}00\r\nEND:STANDARD\r\n"
            )
        })
        .collect();
    let text = wrap(&format!("BEGIN:VTIMEZONE\r\nTZID:Z\r\n{body}END:VTIMEZONE"));
    let lines: Vec<Line> = unfold(&text, 1000).expect("unfolds").iter().filter_map(|l| split_line(l)).collect();
    let harvest = harvest(lines, &Limits::default()).expect("a calendar");
    let zones = Zones::build(harvest.zones, 2025, 2027, &Limits::default(), &mut StepBudget::for_file(&Limits::default()), &ZONE);
    zones.transitions.get("Z").cloned().unwrap_or_default()
}

#[test]
fn transitions_are_found_by_search_as_todays_scan_found_them() {
    // Out of order, and two at the same wall time: the first one listed wins,
    // as today's stable sort and dedup kept it.
    let points = zones_of(&[("20260601T000000", 3), ("20260101T000000", 1), ("20260601T000000", 5), ("20261001T000000", 2)]);
    let offsets: Vec<i64> = points.iter().map(|(_, o)| o.num_hours()).collect();
    assert_eq!(offsets, vec![1, 3, 2]);

    let text = wrap("BEGIN:VTIMEZONE\r\nTZID:Z\r\nBEGIN:STANDARD\r\nDTSTART:20260101T000000\r\nTZOFFSETFROM:+0000\r\n\
                     TZOFFSETTO:+0100\r\nEND:STANDARD\r\nBEGIN:STANDARD\r\nDTSTART:20260601T000000\r\n\
                     TZOFFSETFROM:+0100\r\nTZOFFSETTO:+0300\r\nEND:STANDARD\r\nEND:VTIMEZONE");
    let lines: Vec<Line> = unfold(&text, 1000).expect("unfolds").iter().filter_map(|l| split_line(l)).collect();
    let harvest = harvest(lines, &Limits::default()).expect("a calendar");
    let zones = Zones::build(harvest.zones, 2025, 2027, &Limits::default(), &mut StepBudget::for_file(&Limits::default()), &FixedZone(0));
    let at = |text: &str| NaiveDateTime::parse_from_str(text, "%Y%m%dT%H%M%S").expect("a time");
    // Before every point: the offset before the earliest observance (+00:00).
    assert_eq!(zones.resolve("Z", at("20251231T120000")), Some(at("20251231T120000")));
    // On a point and between points: the last one at or before it.
    assert_eq!(zones.resolve("Z", at("20260101T000000")), Some(at("20251231T230000")));
    assert_eq!(zones.resolve("Z", at("20260531T235959")), Some(at("20260531T225959")));
    assert_eq!(zones.resolve("Z", at("20260601T000000")), Some(at("20260531T210000")));
    assert_eq!(zones.resolve("Z", at("20270101T000000")), Some(at("20261231T210000")));
    assert_eq!(zones.resolve("Nowhere", at("20270101T000000")), None);
}

#[test]
fn a_span_whose_end_overflows_is_none_and_every_other_ends_at_or_after_its_start() {
    let event = RawEvent::default();
    let (from, until) = (day(2026, 1, 1), day(2027, 1, 1));
    assert_eq!(spread(day(2026, 10, 1), 540, i64::MAX, false, &event, from, until), None);
    let span = spread(day(2026, 10, 1), 540, i64::MAX - 540, false, &event, from, until).expect("computes");
    assert_eq!(span.len(), 93, "to the window's edge: 1 October to 1 January");
    assert!(span.iter().all(|o| o.start <= o.end));
    let span = spread(day(2026, 10, 1), 0, i64::MAX, true, &event, from, until).expect("computes");
    assert!(span.iter().all(|o| o.start == 0 && o.end == planner::DAY_MINUTES));
}
