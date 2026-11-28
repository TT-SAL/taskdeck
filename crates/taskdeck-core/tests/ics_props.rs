//! Property test of `ics::parse` (brief c2; core.md §14.6, §18.6): on random
//! text and on mutated calendars, within `Limits::default()`, it returns (an
//! `Ok` or an `Err`, never a panic), and it does so in bounded time: each case
//! runs under proptest's timeout, so a case that ran away fails the test.
//! The zones are a fixed offset and a table zone with transitions.

use chrono::NaiveDate;
use proptest::prelude::*;
use taskdeck_core::ics::{Limits, parse};
use taskdeck_core::time::{FixedZone, TableZone, Zone};

/// Calendars to mutate: every construct the parser reads.
const SEEDS: [&str; 4] = [
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:Work\r\nBEGIN:VEVENT\r\nUID:a@x\r\nSUMMARY:Standup\\, daily\r\n\
     DTSTART:20260907T090000\r\nDTEND:20260907T091500\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;UNTIL=20261217T000000Z\r\n\
     EXDATE:20260909T090000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
    "BEGIN:VCALENDAR\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Test\r\nBEGIN:STANDARD\r\nDTSTART:19701025T030000\r\n\
     TZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nRRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\nEND:STANDARD\r\n\
     BEGIN:DAYLIGHT\r\nDTSTART:19700329T020000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\n\
     RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:b@x\r\n\
     SUMMARY:Zoned\r\nDTSTART;TZID=Europe/Test:20260329T013000\r\nDURATION:P1DT2H\r\n\
     RRULE:FREQ=MONTHLY;BYMONTHDAY=31,-1;BYSETPOS=1;COUNT=40\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:b@x\r\n\
     RECURRENCE-ID;TZID=Europe/Test:20260331T013000\r\nSTATUS:CANCELLED\r\nDTSTART:20260331T013000\r\n\
     END:VEVENT\r\nEND:VCALENDAR\r\n",
    "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:All day\nDTSTART;VALUE=DATE:20260907\nDTEND;VALUE=DATE:20260910\n\
     TRANSP:TRANSPARENT\nRDATE;VALUE=DATE:20261001,20261101\nLOCATION:Room\n  continued\nDESCRIPTION:a\\nb\n\
     END:VEVENT\nBEGIN:VEVENT\nSUMMARY:Ancient\nDTSTART:16010101T090000Z\nRRULE:FREQ=DAILY;INTERVAL=1\nEND:VEVENT\n\
     END:VCALENDAR\n",
    "BEGIN:VCALENDAR\r\nCALSCALE:GREGORIAN\r\nBEGIN:VEVENT\r\nSUMMARY:Leap\r\nDTSTART:20240229T235959Z\r\n\
     RRULE:FREQ=YEARLY;INTERVAL=4\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:Edge\r\nDTSTART:99991231T230000\r\n\
     DURATION:PT999999H\r\nRRULE:FREQ=DAILY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
];

/// Lines a mutation may insert: the property names and values the parser
/// branches on, malformed ones included.
const LINES: [&str; 24] = [
    "BEGIN:VEVENT",
    "END:VEVENT",
    "BEGIN:VTIMEZONE",
    "END:VTIMEZONE",
    "BEGIN:STANDARD",
    "END:STANDARD",
    "BEGIN:VCALENDAR",
    "END:VCALENDAR",
    "DTSTART:20260910T090000Z",
    "DTSTART;TZID=\"Europe/Test\":20260910T090000",
    "DTSTART;VALUE=DATE:20260910",
    "DTEND:19700101T000000",
    "DURATION:P99999999W",
    "DURATION:-PT1H",
    "RRULE:FREQ=DAILY;COUNT=4294967295",
    "RRULE:FREQ=MONTHLY;BYDAY=5FR,-5MO;BYSETPOS=-1,1,0",
    "RRULE:FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=29,30,-31",
    "RRULE:FREQ=WEEKLY;INTERVAL=0",
    "RRULE:FREQ=HOURLY",
    "EXDATE:20260911T090000Z,bad",
    "TZOFFSETFROM:+2400",
    "TZOFFSETTO:-000001",
    "CALSCALE:CHINESE",
    "RECURRENCE-ID:20260910T090000Z",
];

#[derive(Debug, Clone)]
enum Mutation {
    InsertLine(usize, usize),
    DeleteLine(usize),
    DuplicateLine(usize),
    InsertText(usize, String),
    Truncate(usize),
}

fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        (any::<usize>(), 0..LINES.len()).prop_map(|(at, l)| Mutation::InsertLine(at, l)),
        any::<usize>().prop_map(Mutation::DeleteLine),
        any::<usize>().prop_map(Mutation::DuplicateLine),
        (any::<usize>(), "\\PC{0,12}").prop_map(|(at, s)| Mutation::InsertText(at, s)),
        any::<usize>().prop_map(Mutation::Truncate),
    ]
}

fn mutate(seed: &str, mutations: &[Mutation]) -> String {
    let mut lines: Vec<String> = seed.split('\n').map(str::to_string).collect();
    for m in mutations {
        let n = lines.len().max(1);
        match m {
            Mutation::InsertLine(at, l) => lines.insert(at % (lines.len() + 1), LINES[*l].to_string()),
            Mutation::DeleteLine(at) if !lines.is_empty() => {
                lines.remove(at % n);
            }
            Mutation::DuplicateLine(at) if !lines.is_empty() => {
                let line = lines[at % n].clone();
                lines.insert(at % n, line);
            }
            Mutation::InsertText(at, s) if !lines.is_empty() => {
                let line = &mut lines[at % n];
                let cut = line.char_indices().map(|(i, _)| i).nth(at % (line.chars().count() + 1)).unwrap_or(line.len());
                line.insert_str(cut, s);
            }
            Mutation::Truncate(at) if !lines.is_empty() => {
                let line = &mut lines[at % n];
                let keep = line.char_indices().map(|(i, _)| i).nth(at % (line.chars().count() + 1)).unwrap_or(line.len());
                line.truncate(keep);
            }
            _ => {}
        }
    }
    lines.join("\n")
}

/// A window of up to 800 days: mostly around the seeds' years, sometimes
/// anywhere in years 1 to 9999.
fn window() -> impl Strategy<Value = (NaiveDate, NaiveDate)> {
    let year = prop_oneof![3 => 2023i32..=2027, 1 => 1i32..=9999];
    (year, 1u32..=366, 0i64..=800).prop_map(|(year, ordinal, span)| {
        let from = NaiveDate::from_yo_opt(year, ordinal.min(365)).unwrap_or(NaiveDate::MIN);
        let until = from.checked_add_signed(chrono::TimeDelta::days(span)).unwrap_or(from);
        (from, until)
    })
}

/// A zone for one case: a fixed offset or a table zone with transitions.
#[derive(Debug, Clone)]
enum TestZone {
    Fixed(FixedZone),
    Table(TableZone),
}

impl TestZone {
    fn as_zone(&self) -> &dyn Zone {
        match self {
            TestZone::Fixed(z) => z,
            TestZone::Table(z) => z,
        }
    }
}

fn zone() -> impl Strategy<Value = TestZone> {
    prop_oneof![
        (-86_399i32..=86_399).prop_map(|o| TestZone::Fixed(FixedZone(o))),
        // +01:00, +02:00 from 2026-03-29T01:00Z, +01:00 from 2026-10-25T01:00Z.
        Just(TestZone::Table(TableZone { initial: 3600, transitions: vec![(1_774_746_000_000, 7200), (1_792_890_000_000, 3600)] })),
    ]
}

/// Each case must finish within this many milliseconds (proptest's timeout).
const CASE_TIMEOUT_MS: u32 = 30_000;

proptest! {
    #![proptest_config(ProptestConfig { timeout: CASE_TIMEOUT_MS, ..ProptestConfig::default() })]

    #[test]
    fn parse_returns_on_random_text(text in "\\PC{0,400}", (from, until) in window(), zone in zone()) {
        let _ = parse(&text, from, until, Limits::default(), zone.as_zone());
        let wrapped = format!("BEGIN:VCALENDAR\r\n{text}\r\nEND:VCALENDAR\r\n");
        let _ = parse(&wrapped, from, until, Limits::default(), zone.as_zone());
    }

    #[test]
    fn parse_returns_on_mutated_calendars(
        seed in 0..SEEDS.len(),
        mutations in prop::collection::vec(mutation(), 0..12),
        (from, until) in window(),
        zone in zone(),
    ) {
        let text = mutate(SEEDS[seed], &mutations);
        let limits = Limits::default();
        if let Ok(parsed) = parse(&text, from, until, limits, zone.as_zone()) {
            prop_assert!(parsed.occurrences.len() <= limits.max_occurrences);
            for o in &parsed.occurrences {
                prop_assert!(from <= o.day && o.day <= until, "{:?} outside {from}..={until}", o.day);
                prop_assert!(0 <= o.start && o.start <= o.end && o.end <= 24 * 60, "{o:?}");
            }
        }
    }

    #[test]
    fn the_seed_calendars_parse_in_any_zone_and_window((from, until) in window(), zone in zone()) {
        for seed in SEEDS {
            prop_assert!(parse(seed, from, until, Limits::default(), zone.as_zone()).is_ok());
        }
    }
}

/// The seeds are read at all: the property above is not vacuous.
#[test]
fn the_seeds_give_occurrences_in_2026() {
    let from = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap_or(NaiveDate::MIN);
    let until = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap_or(NaiveDate::MAX);
    for seed in &SEEDS[..3] {
        let parsed = parse(seed, from, until, Limits::default(), &FixedZone(0)).expect("a calendar");
        assert!(!parsed.occurrences.is_empty(), "{seed}");
    }
}
