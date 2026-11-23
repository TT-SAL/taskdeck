//! The calendar parser's cost is bounded (brief c3, to become core.md §14.6
//! with spec change 5). Each finding of the security pass on the ported
//! parser is a test here, at the full 8 MiB body a fetch may carry where the
//! reproduction scales.
//!
//! The crate's tests cannot read a clock (CORE-GEN-003), so bounded time is
//! proven by proptest's own `timeout`: each `timed` test is one case that
//! must finish inside `CASE_TIMEOUT_MS` or fail. They are timing harnesses,
//! not properties, so they run one case each; the properties below them run
//! proptest's default case count.

use chrono::{Datelike, NaiveDate, Weekday};
use proptest::prelude::*;
use taskdeck_core::ics::{Limits, Parsed, parse};
use taskdeck_core::time::FixedZone;

/// The largest body a fetch hands the parser (`device.md`, DEV-ICS-004).
const BODY_MAX: usize = 8 * 1024 * 1024;

/// Each timing case must finish within this many milliseconds.
const CASE_TIMEOUT_MS: u32 = 30_000;

/// The phone's window around 2026-11-23: 60 days back, 400 forward.
fn window() -> (NaiveDate, NaiveDate) {
    (
        NaiveDate::from_ymd_opt(2026, 9, 24).unwrap_or(NaiveDate::MIN),
        NaiveDate::from_ymd_opt(2027, 12, 29).unwrap_or(NaiveDate::MAX),
    )
}

fn read(text: &str) -> Parsed {
    read_with(text, Limits::default())
}

fn read_with(text: &str, limits: Limits) -> Parsed {
    let (from, until) = window();
    parse(text, from, until, limits, &FixedZone(0)).expect("a calendar")
}

/// `head`, then `items` cycled and joined by commas for as long as the whole
/// stays within `BODY_MAX`, then `tail`.
fn fill(head: &str, items: &[String], tail: &str) -> String {
    let mut text = String::with_capacity(BODY_MAX);
    text.push_str(head);
    let mut first = true;
    for item in items.iter().cycle() {
        if text.len() + item.len() + 1 + tail.len() > BODY_MAX {
            break;
        }
        if !first {
            text.push(',');
        }
        first = false;
        text.push_str(item);
    }
    text.push_str(tail);
    text
}

/// The text a body was built to stay inside, and close to.
fn assert_full(text: &str) {
    assert!(text.len() <= BODY_MAX, "{}", text.len());
    assert!(text.len() > BODY_MAX - 64 * 1024, "{}", text.len());
}

fn has(parsed: &Parsed, words: &str) -> bool {
    parsed.problems.iter().any(|p| p.contains(words))
}

/// The Mondays of `window()`: 28 September 2026 to 27 December 2027, 455
/// days apart, so 455 / 7 + 1 = 66 of them (worked by hand).
const MONDAYS_IN_WINDOW: usize = 66;

fn all_mondays(parsed: &Parsed) -> bool {
    parsed.occurrences.iter().all(|o| o.day.weekday() == Weekday::Mon)
}

/// 64 distinct `BYDAY` values: the 1st to 32nd and the last to 32nd-last Monday.
fn sixty_four_mondays() -> Vec<String> {
    (1..=32).map(|n| format!("{n}MO")).chain((1..=32).map(|n| format!("-{n}MO"))).collect()
}

fn one_event(body: &str) -> String {
    format!("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\n{body}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n")
}

/* ──────────── 1. A BY… list is read without duplicates, at most 64 ──────────── */

proptest! {
    #![proptest_config(ProptestConfig { timeout: CASE_TIMEOUT_MS, cases: 1, ..ProptestConfig::default() })]

    // spec: CORE-GEN-003
    #[test]
    fn timed_a_repeated_byday_at_eight_mib_is_one_value(_case in Just(())) {
        // Security finding 1: 40,000 `MO`s took 109 s in release; 8 MiB of
        // them would have taken hours. Read without duplicates, it is `MO`.
        let text = fill(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:00010101\r\nRRULE:FREQ=MONTHLY;BYDAY=",
            &["MO".to_string()],
            "\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        assert_full(&text);
        let parsed = read(&text);
        prop_assert_eq!(parsed.occurrences.len(), MONDAYS_IN_WINDOW);
        prop_assert!(all_mondays(&parsed));
        prop_assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
        // The parse depends on its arguments alone (CORE-GEN-003).
        prop_assert_eq!(read(&text), parsed);
    }

    #[test]
    fn timed_sixty_four_distinct_values_at_eight_mib_are_followed(_case in Just(())) {
        // 64 distinct BYDAY values, repeated to 8 MiB, walked monthly from
        // year 1: every Monday, once.
        let text = fill(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:00010101\r\nRRULE:FREQ=MONTHLY;BYDAY=",
            &sixty_four_mondays(),
            "\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        assert_full(&text);
        let parsed = read(&text);
        prop_assert_eq!(parsed.occurrences.len(), MONDAYS_IN_WINDOW);
        prop_assert!(all_mondays(&parsed));

        // 64 distinct BYSETPOS values, repeated to 8 MiB: positions 1 to 5
        // pick every Monday of a month, the rest pick none.
        let positions: Vec<String> = (1..=64).map(|n| n.to_string()).collect();
        let text = fill(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:00010101\r\nRRULE:FREQ=MONTHLY;BYDAY=MO;BYSETPOS=",
            &positions,
            "\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        assert_full(&text);
        let parsed = read(&text);
        prop_assert_eq!(parsed.occurrences.len(), MONDAYS_IN_WINDOW);

        // A weekly walk from year 1 over 64 BYDAY values runs out of steps
        // (100,000 weeks) before the window, and says so.
        let text = fill(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:00010101\r\nRRULE:FREQ=WEEKLY;BYDAY=",
            &sixty_four_mondays(),
            "\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        let parsed = read(&text);
        prop_assert!(has(&parsed, "further"), "{:?}", parsed.problems);
    }
}

#[test]
fn a_list_of_sixty_five_distinct_values_is_a_rule_this_cannot_follow() {
    // 64 distinct values are followed.
    let byday = sixty_four_mondays().join(",");
    let parsed = read(&one_event(&format!("DTSTART;VALUE=DATE:20260901\r\nRRULE:FREQ=MONTHLY;BYDAY={byday}")));
    assert_eq!(parsed.occurrences.len(), MONDAYS_IN_WINDOW);
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);

    // A 65th is a rule the parser cannot follow, treated as today's parser
    // treats one: counted, and the event read without its rule (a single
    // occurrence at DTSTART, here before the window).
    let parsed =
        read(&one_event(&format!("DTSTART;VALUE=DATE:20260901\r\nRRULE:FREQ=MONTHLY;BYDAY={byday},33MO")));
    assert!(parsed.occurrences.is_empty(), "{:?}", parsed.occurrences);
    assert!(has(&parsed, "repeat rule"), "{:?}", parsed.problems);

    let positions: Vec<String> = (1..=65).map(|n| n.to_string()).collect();
    let parsed = read(&one_event(&format!(
        "DTSTART;VALUE=DATE:20260901\r\nRRULE:FREQ=MONTHLY;BYDAY=MO;BYSETPOS={}",
        positions.join(",")
    )));
    assert!(parsed.occurrences.is_empty());
    assert!(has(&parsed, "repeat rule"), "{:?}", parsed.problems);

    // Duplicates do not count towards the 64: 65 values, 64 of them distinct.
    let parsed = read(&one_event(&format!("DTSTART;VALUE=DATE:20260901\r\nRRULE:FREQ=MONTHLY;BYDAY={byday},1MO")));
    assert_eq!(parsed.occurrences.len(), MONDAYS_IN_WINDOW);
}

/* ─────── 2. A VTIMEZONE: at most 512 observances and 4,096 transitions ─────── */

/// A calendar with a zone `Z` of the given observances, and one event at
/// 09:00 in `Z` on 1 October 2026.
fn zoned(observances: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nBEGIN:VTIMEZONE\r\nTZID:Z\r\n{observances}END:VTIMEZONE\r\n\
         BEGIN:VEVENT\r\nSUMMARY:Meet\r\nDTSTART;TZID=Z:20261001T090000\r\nDURATION:PT30M\r\nEND:VEVENT\r\n\
         END:VCALENDAR\r\n"
    )
}

/// An observance at +01:00 starting `second` seconds into `day` (YYYYMMDD),
/// with `rule` as its RRULE when not empty.
fn observance(day: &str, second: u32, rule: &str) -> String {
    let (h, m, s) = (second / 3600, second / 60 % 60, second % 60);
    let rule = if rule.is_empty() { String::new() } else { format!("RRULE:{rule}\r\n") };
    format!(
        "BEGIN:STANDARD\r\nDTSTART:{day}T{h:02}{m:02}{s:02}\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0100\r\n{rule}END:STANDARD\r\n"
    )
}

/// The start minute of the one occurrence: 480 when `Z` was read (+01:00,
/// 08:00 at UTC), 540 when it was treated as undefined (wall time).
fn start_of_meet(parsed: &Parsed) -> i32 {
    assert_eq!(parsed.occurrences.len(), 1, "{:?}", parsed.occurrences);
    parsed.occurrences.first().map(|o| o.start).unwrap_or(-1)
}

#[test]
fn a_zone_of_512_observances_is_read_and_one_of_513_is_an_undefined_zone() {
    let many = |n: u32| (0..n).map(|i| observance("20260101", i, "")).collect::<String>();
    let parsed = read(&zoned(&many(512)));
    assert_eq!(start_of_meet(&parsed), 8 * 60);
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);

    let parsed = read(&zoned(&many(513)));
    assert_eq!(start_of_meet(&parsed), 9 * 60, "read as wall time, as an undefined zone is");
    assert!(has(&parsed, "time zone"), "{:?}", parsed.problems);
}

#[test]
fn a_zone_of_4096_transitions_in_its_window_is_read_and_one_of_4097_is_an_undefined_zone() {
    // The zone's window for `window()` is 2025-01-01 to 2028-12-31. A DAILY
    // observance from 2025-01-01 gives 1,000 points there (the per-event
    // limit; its DTSTART is the first of them), so four give 4,000; each
    // single observance in 2026 adds one more at its own second.
    let daily: String = (0..4).map(|i| observance("20250101", i, "FREQ=DAILY")).collect();
    let singles = |n: u32| (0..n).map(|i| observance("20260101", 43_200 + i, "")).collect::<String>();

    let parsed = read(&zoned(&format!("{daily}{}", singles(96))));
    assert_eq!(start_of_meet(&parsed), 8 * 60, "4,096 points");
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);

    let parsed = read(&zoned(&format!("{daily}{}", singles(97))));
    assert_eq!(start_of_meet(&parsed), 9 * 60, "4,097 points: an undefined zone");
    assert!(has(&parsed, "time zone"), "{:?}", parsed.problems);
}

proptest! {
    #![proptest_config(ProptestConfig { timeout: CASE_TIMEOUT_MS, cases: 1, ..ProptestConfig::default() })]

    // spec: CORE-GEN-003
    #[test]
    fn timed_a_zone_of_thirty_thousand_observances_and_its_exdates_at_eight_mib(_case in Just(())) {
        // Security finding 2: 10,000 DAILY observances and 4,000 EXDATEs
        // (1.1 MB) took 178 s in release, and 10^7 transition points were
        // held. Here 30,000 observances (180,000 lines, under max_lines) and
        // EXDATEs to fill 8 MiB.
        let observances: String = (0..30_000).map(|i| observance("20250101", i, "FREQ=DAILY")).collect();
        let text = fill(
            &format!(
                "BEGIN:VCALENDAR\r\nBEGIN:VTIMEZONE\r\nTZID:Z\r\n{observances}END:VTIMEZONE\r\n\
                 BEGIN:VEVENT\r\nDTSTART;TZID=Z:20261001T090000\r\nEXDATE;TZID=Z:"
            ),
            &["20000101T090000".to_string()],
            "\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        assert_full(&text);
        let parsed = read(&text);
        prop_assert_eq!(start_of_meet(&parsed), 9 * 60);
        prop_assert!(has(&parsed, "time zone"), "{:?}", parsed.problems);
        prop_assert!(has(&parsed, "excluded dates"), "{:?}", parsed.problems);
    }

    #[test]
    fn timed_zoned_times_are_found_by_search_in_a_zone_of_4000_transitions(_case in Just(())) {
        // A defined zone of 4,000 points, and as many events with 1,000
        // zoned EXDATEs each as fit in 8 MiB: about 500,000 lookups, which a
        // scan per lookup turns into 2 * 10^9 comparisons.
        let observances: String = (0..4).map(|i| observance("20250101", i, "FREQ=DAILY")).collect();
        let exdates = vec!["20000101T090000"; 1000].join(",");
        let one = format!(
            "BEGIN:VEVENT\r\nSUMMARY:Meet\r\nDTSTART;TZID=Z:20261001T090000\r\nEXDATE;TZID=Z:{exdates}\r\nEND:VEVENT\r\n"
        );
        let head = format!("BEGIN:VCALENDAR\r\nBEGIN:VTIMEZONE\r\nTZID:Z\r\n{observances}END:VTIMEZONE\r\n");
        let tail = "END:VCALENDAR\r\n";
        let events = (BODY_MAX - head.len() - tail.len()) / one.len();
        let text = format!("{head}{}{tail}", one.repeat(events));
        assert_full(&text);
        let parsed = read(&text);
        prop_assert_eq!(parsed.occurrences.len(), events);
        prop_assert!(parsed.occurrences.iter().all(|o| o.start == 8 * 60));
        prop_assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    }
}

/* ─────── 3. EXDATE, RDATE and overrides: each at most the per-event limit ─────── */

#[test]
fn exdates_and_rdates_past_the_per_event_limit_are_dropped_and_noted_once() {
    let limits = Limits { max_occurrences_per_event: 5, ..Limits::default() };
    // Starts: DTSTART on 1 October and the first five RDATEs (2 to 6
    // October); the 7th and 8th are past the limit. Exclusions: the first
    // five EXDATEs (1 and 3 October, then three days that are no start); the
    // 2nd and 4th October come sixth and seventh, past the limit, and so do
    // not exclude.
    let text = one_event(
        "SUMMARY:Meet\r\nDTSTART:20261001T090000\r\n\
         RDATE:20261002T090000,20261003T090000,20261004T090000,20261005T090000,20261006T090000,\
         20261007T090000,20261008T090000\r\n\
         EXDATE:20261001T090000,20261003T090000,20261020T090000,20261021T090000,20261022T090000,\
         20261002T090000,20261004T090000\r\n",
    );
    let parsed = read_with(&text, limits);
    let days: Vec<u32> = parsed.occurrences.iter().map(|o| o.day.day()).collect();
    assert_eq!(days, vec![2, 4, 5, 6]);
    assert!(parsed.problems.iter().any(|p| p.contains("excluded dates") && !p.contains("times")), "{:?}", parsed.problems);
    assert!(parsed.problems.iter().any(|p| p.contains("extra dates") && !p.contains("times")), "{:?}", parsed.problems);
}

#[test]
fn overrides_past_the_per_event_limit_are_dropped_and_noted_once() {
    let limits = Limits { max_occurrences_per_event: 5, ..Limits::default() };
    // A series on 1 and 2 October at 09:00, and seven overrides of it: five
    // of November days the series never falls on, then moves of 1 and 2
    // October to 10:00. Under a limit of 5 the first five are kept, the two
    // moves are past it and dropped, and the series stays where its rule put
    // it.
    let mut text = String::from(
        "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:u\r\nSUMMARY:Standup\r\nDTSTART:20261001T090000\r\n\
         RRULE:FREQ=DAILY;COUNT=2\r\nEND:VEVENT\r\n",
    );
    for (month, d) in [(11, 1), (11, 2), (11, 3), (11, 4), (11, 5), (10, 1), (10, 2)] {
        text.push_str(&format!(
            "BEGIN:VEVENT\r\nUID:u\r\nSUMMARY:Standup\r\nRECURRENCE-ID:2026{month}0{d}T090000\r\n\
             DTSTART:2026{month}0{d}T100000\r\nEND:VEVENT\r\n"
        ));
    }
    text.push_str("END:VCALENDAR\r\n");
    let starts = |parsed: &Parsed| parsed.occurrences.iter().map(|o| (o.day.day(), o.start)).collect::<Vec<_>>();

    let parsed = read_with(&text, Limits { max_occurrences_per_event: 7, ..limits });
    assert_eq!(starts(&parsed), vec![(1, 600), (2, 600)], "the limit does not bite at 7");
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);

    let parsed = read_with(&text, limits);
    assert_eq!(starts(&parsed), vec![(1, 540), (2, 540)]);
    assert!(parsed.problems.iter().any(|p| p.contains("moved occurrences") && !p.contains("times")), "{:?}", parsed.problems);
}

#[test]
fn overrides_do_not_multiply_past_the_per_event_limit() {
    // Security finding 3, as reproduced: two series with UID u and five
    // overrides each with a DAILY rule of their own gave 50 occurrences
    // under a per-event limit of 5.
    let limits = Limits { max_occurrences_per_event: 5, max_occurrences: 1_000_000, ..Limits::default() };
    let mut text = String::from("BEGIN:VCALENDAR\r\n");
    for _ in 0..2 {
        text.push_str("BEGIN:VEVENT\r\nUID:u\r\nDTSTART:20261001T090000\r\nRRULE:FREQ=DAILY\r\nEND:VEVENT\r\n");
    }
    for d in 1..=5 {
        text.push_str(&format!(
            "BEGIN:VEVENT\r\nUID:u\r\nRECURRENCE-ID:2026100{d}T090000\r\nDTSTART:20261001T100000\r\n\
             DURATION:PT1M\r\nRRULE:FREQ=DAILY\r\nEND:VEVENT\r\n"
        ));
    }
    text.push_str("END:VCALENDAR\r\n");
    let parsed = read_with(&text, limits);
    // Each series stops at its limit, its overrides included.
    assert_eq!(parsed.occurrences.len(), 2 * limits.max_occurrences_per_event);
}

/// A span is cut into at most this many days (`spread`'s guard), so one
/// start adds at most this many occurrences past the per-event limit.
const SPREAD_DAYS: usize = 400;

proptest! {
    #![proptest_config(ProptestConfig { timeout: CASE_TIMEOUT_MS, cases: 1, ..ProptestConfig::default() })]

    #[test]
    fn timed_overrides_at_eight_mib_stay_inside_the_per_event_limit(_case in Just(())) {
        // Security finding 3 at scale: a 1 MB file of 461 overrides, each
        // with a DAILY rule, DURATION:P400D and 500 four-byte characters,
        // peaked at 1.16 GB. Here 8 MiB of them.
        let wide = "\u{10348}";
        let head = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:u\r\nDTSTART:20261001T090000\r\nRRULE:FREQ=DAILY\r\nEND:VEVENT\r\n";
        let tail = "END:VCALENDAR\r\n";
        let one = |n: usize| {
            let day = NaiveDate::from_ymd_opt(2026, 10, 1).and_then(|d| d.checked_add_days(chrono::Days::new((n % 450) as u64)));
            let day = day.map(|d| format!("{:04}{:02}{:02}", d.year(), d.month(), d.day())).unwrap_or_default();
            format!(
                "BEGIN:VEVENT\r\nUID:u\r\nRECURRENCE-ID:{day}T090000\r\nDTSTART:20261001T100000\r\nDURATION:P400D\r\n\
                 RRULE:FREQ=DAILY\r\nSUMMARY:{}\r\nDESCRIPTION:{}\r\nEND:VEVENT\r\n",
                wide.repeat(200),
                wide.repeat(300)
            )
        };
        let mut text = String::from(head);
        let mut n = 0;
        loop {
            let next = one(n);
            if text.len() + next.len() + tail.len() > BODY_MAX {
                break;
            }
            text.push_str(&next);
            n += 1;
        }
        text.push_str(tail);
        assert_full(&text);
        let limits = Limits::default();
        let parsed = read_with(&text, limits);
        prop_assert!(
            parsed.occurrences.len() < limits.max_occurrences_per_event + SPREAD_DAYS,
            "{}",
            parsed.occurrences.len()
        );
        prop_assert!(has(&parsed, "moved occurrences"), "{:?}", parsed.problems);
    }
}

/* ──────────────── 4. Durations, ends and spans: checked arithmetic ──────────────── */

#[test]
fn a_duration_whose_end_cannot_be_computed_drops_the_occurrence_and_says_so() {
    // Security finding 4: a debug build panicked on `offset + remaining`,
    // a release build returned end 0 for start 540.
    let parsed = read(&one_event("SUMMARY:Forever\r\nDTSTART:20261001T090000\r\nDURATION:PT9223372036854775807M"));
    assert!(parsed.occurrences.is_empty(), "{:?}", parsed.occurrences);
    assert!(has(&parsed, "end could not be worked out"), "{:?}", parsed.problems);

    // Just short of overflowing at 09:00, it is a span to the window's edge.
    let minutes = i64::MAX - 9 * 60;
    let parsed = read(&one_event(&format!("SUMMARY:Long\r\nDTSTART:20261001T090000\r\nDURATION:PT{minutes}M")));
    assert_eq!(parsed.occurrences.first().map(|o| (o.start, o.end)), Some((9 * 60, 24 * 60)));
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
}

fn duration_minutes() -> impl Strategy<Value = i64> {
    prop_oneof![0i64..=10_000, (i64::MAX - 3_000)..=i64::MAX, 0i64..=i64::MAX]
}

proptest! {
    #[test]
    fn no_occurrence_ends_before_it_starts_whatever_its_duration(
        minutes in duration_minutes(),
        hour in 0u32..24,
        minute in 0u32..60,
        all_day in any::<bool>(),
    ) {
        let start = if all_day { "DTSTART;VALUE=DATE:20261001".to_string() } else { format!("DTSTART:20261001T{hour:02}{minute:02}00") };
        let text = one_event(&format!("SUMMARY:x\r\n{start}\r\nDURATION:PT{minutes}M\r\nRRULE:FREQ=WEEKLY;COUNT=3"));
        let parsed = read(&text);
        for o in &parsed.occurrences {
            prop_assert!(0 <= o.start && o.start <= o.end && o.end <= 24 * 60, "{o:?}");
        }
        prop_assert!(!parsed.occurrences.is_empty() || has(&parsed, "end could not be worked out"));
    }
}

/* ─────── 5. Problems: one line per kind, at most 64, quoting nothing ─────── */

// spec: CORE-GEN-010
#[test]
fn undefined_tzids_give_one_problem_line_that_quotes_none_of_them() {
    // Security finding 5: each undefined TZID was its own line, quoting it.
    let mut text = String::from("BEGIN:VCALENDAR\r\n");
    for i in 0..3 {
        text.push_str(&format!("BEGIN:VEVENT\r\nDTSTART;TZID=\"Zone {i}\":20261001T090000\r\nEND:VEVENT\r\n"));
    }
    text.push_str("END:VCALENDAR\r\n");
    let parsed = read(&text);
    assert_eq!(parsed.problems.len(), 1, "{:?}", parsed.problems);
    assert!(parsed.problems.iter().all(|p| !p.contains("Zone") && p.contains("3 times")), "{:?}", parsed.problems);
}

// spec: CORE-GEN-010
#[test]
fn a_calendar_scale_this_cannot_read_is_refused_without_quoting_it() {
    let (from, until) = window();
    let error = parse("BEGIN:VCALENDAR\r\nCALSCALE:Secret-Scale\r\nEND:VCALENDAR\r\n", from, until, Limits::default(), &FixedZone(0))
        .expect_err("refused");
    assert!(!error.to_ascii_uppercase().contains("SECRET-SCALE"), "{error}");
    assert!(error.contains("calendar"), "{error}");
}

// spec: CORE-GEN-010
#[test]
fn every_kind_of_problem_at_once_is_one_line_each_and_quotes_nothing_of_the_file() {
    let limits = Limits { max_occurrences_per_event: 2, max_occurrences: 3, max_rule_steps: 50, ..Limits::default() };
    let events = [
        "SUMMARY:Private one",
        "SUMMARY:Private two\r\nDTSTART:20261001T090000\r\nRRULE:FREQ=HOURLY",
        "SUMMARY:Private three\r\nDTSTART:16010101T090000\r\nRRULE:FREQ=DAILY",
        "SUMMARY:Private four\r\nDTSTART;TZID=Private/Zone:20261001T090000",
        "SUMMARY:Private five\r\nDTSTART:20261001T090000\r\nDURATION:PT9223372036854775807M",
        "UID:p\r\nSUMMARY:Private six\r\nDTSTART:20261001T090000\r\nRDATE:20261002T090000,20261003T090000,20261004T090000\r\n\
         EXDATE:20261102T090000,20261103T090000,20261104T090000",
        "UID:p\r\nRECURRENCE-ID:20261002T090000\r\nDTSTART:20261002T100000",
        "UID:p\r\nRECURRENCE-ID:20261003T090000\r\nDTSTART:20261003T100000",
        "UID:p\r\nRECURRENCE-ID:20261004T090000\r\nDTSTART:20261004T100000",
        "SUMMARY:Private seven\r\nDTSTART:20261005T090000",
        "SUMMARY:Private eight\r\nDTSTART:20261006T090000",
    ];
    let body: String = events.iter().map(|e| format!("BEGIN:VEVENT\r\n{e}\r\nEND:VEVENT\r\n")).collect();
    let text = format!("BEGIN:VCALENDAR\r\nX-WR-CALNAME:Private name\r\n{body}END:VCALENDAR\r\n");
    let parsed = read_with(&text, limits);
    assert!(parsed.problems.len() >= 8, "{:?}", parsed.problems);
    assert!(parsed.problems.len() <= 64);
    let mut kinds = parsed.problems.clone();
    kinds.sort();
    kinds.dedup();
    assert_eq!(kinds.len(), parsed.problems.len(), "one line per kind: {:?}", parsed.problems);
    for p in &parsed.problems {
        assert!(!p.contains("Private") && !p.contains("2026") && !p.contains("1601"), "{p}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig { timeout: CASE_TIMEOUT_MS, cases: 1, ..ProptestConfig::default() })]

    // spec: CORE-GEN-010
    #[test]
    fn timed_twenty_thousand_distinct_tzids_give_one_line(_case in Just(())) {
        // The finding at scale: 20,000 events with distinct TZIDs sharing a
        // 300-character prefix (7.2 MB) took 2.45 s and gave 20,000 lines.
        let prefix = "p".repeat(300);
        let mut text = String::from("BEGIN:VCALENDAR\r\n");
        for i in 0..20_000 {
            text.push_str(&format!("BEGIN:VEVENT\r\nDTSTART;TZID=\"{prefix}{i}\":20261001T090000\r\nEND:VEVENT\r\n"));
        }
        text.push_str("END:VCALENDAR\r\n");
        prop_assert!(text.len() <= BODY_MAX);
        let parsed = read(&text);
        prop_assert_eq!(parsed.occurrences.len(), 20_000);
        prop_assert_eq!(parsed.problems.len(), 1);
        prop_assert!(parsed.problems.iter().all(|p| p.contains("20000 times") && !p.contains("ppp")));
    }
}

/* ─────────── 6. Ties on (day, start, end, name) ordered by the rest ─────────── */

#[test]
fn the_listing_order_does_not_change_the_parse() {
    // Security finding 6, as reproduced.
    let a = "BEGIN:VEVENT\r\nSUMMARY:x\r\nLOCATION:one\r\nDTSTART:20261001T090000\r\nEND:VEVENT\r\n";
    let b = "BEGIN:VEVENT\r\nSUMMARY:x\r\nLOCATION:two\r\nDTSTART:20261001T090000\r\nEND:VEVENT\r\n";
    let ab = format!("BEGIN:VCALENDAR\r\n{a}{b}END:VCALENDAR\r\n");
    let ba = format!("BEGIN:VCALENDAR\r\n{b}{a}END:VCALENDAR\r\n");
    assert_eq!(read(&ab), read(&ba));
}

/// Events that tie on (day, start, end, name) and differ in every other
/// field: location, description, all day (an all-day band and a timed
/// 00:00-24:00 span both read 0 to 1440) and free.
const TIES: [&str; 6] = [
    "SUMMARY:x\r\nDTSTART;VALUE=DATE:20261001",
    "SUMMARY:x\r\nDTSTART:20261001T000000\r\nDURATION:P1D",
    "SUMMARY:x\r\nDTSTART:20261001T000000\r\nDURATION:P1D\r\nTRANSP:TRANSPARENT",
    "SUMMARY:x\r\nDTSTART:20261001T000000\r\nDURATION:P1D\r\nLOCATION:b",
    "SUMMARY:x\r\nDTSTART:20261001T000000\r\nDURATION:P1D\r\nLOCATION:a",
    "SUMMARY:x\r\nDTSTART:20261001T000000\r\nDURATION:P1D\r\nLOCATION:a\r\nDESCRIPTION:d",
];

fn listed(order: &[usize]) -> String {
    let body: String = order.iter().map(|&i| format!("BEGIN:VEVENT\r\n{}\r\nEND:VEVENT\r\n", TIES[i])).collect();
    format!("BEGIN:VCALENDAR\r\n{body}END:VCALENDAR\r\n")
}

proptest! {
    #[test]
    fn a_calendar_listed_in_any_order_parses_equal(order in Just((0..TIES.len()).collect::<Vec<usize>>()).prop_shuffle()) {
        let natural: Vec<usize> = (0..TIES.len()).collect();
        let parsed = read(&listed(&order));
        prop_assert_eq!(parsed.occurrences.len(), TIES.len());
        prop_assert_eq!(parsed, read(&listed(&natural)));
    }
}
