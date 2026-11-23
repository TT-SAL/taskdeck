//! Today's overlay tests (subscriptions.rs:794-1255), ported, and the
//! `FetchStatus.at` change of core.md §14.6.
//!
//! Ported: every test of the items here. Not ported, because what they test
//! is not: `checked_url` (task D), the fetch tests (`serving`, `fetch_all`),
//! the cache file round trip through a folder (`save_overlay`,
//! `read_overlay`; its serde half is the fixture test below) and
//! `without_repeated_location`, a fetch-side helper outside
//! subscriptions.rs:184-484.

use super::*;
use chrono::NaiveDate;

const SUBSCRIBED_CACHE: &str = include_str!("../../../../fixtures/boards/small/subscribed_cache.json");

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("a real date")
}

fn event(subscription: u64, day: NaiveDate, start: i32, end: i32, summary: &str) -> OverlayEvent {
    OverlayEvent {
        subscription,
        day,
        start,
        end,
        all_day: false,
        free: false,
        summary: summary.to_string(),
        location: String::new(),
        description: String::new(),
    }
}

/// Today's tests used `Local::now()` for `FetchStatus.at`; no test here reads
/// a clock (CORE-GEN-003), so a fixed time stands in. None of them depends on
/// its value.
fn at(text: &str) -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::parse_from_rfc3339(text).expect("an RFC 3339 time")
}

#[test]
fn a_calendar_is_named_after_its_host_until_something_better_turns_up() {
    assert_eq!(name_from_url("https://sisu.helsinki.fi:443/ilmo/x.ics").as_deref(), Some("sisu.helsinki.fi"));
    assert_eq!(name_from_url("https://www.example.com/a.ics").as_deref(), Some("example.com"));
    assert_eq!(name_from_url("https://user:pw@cal.example.com/a.ics").as_deref(), Some("cal.example.com"));
    assert_eq!(name_from_url("not a url").as_deref(), None);

    let host_named = Subscription {
        id: 150,
        name: "sisu.helsinki.fi".into(),
        url: "https://sisu.helsinki.fi:443/ilmo/x.ics".into(),
        color: [1, 2, 3, 255],
        enabled: true,
    };
    // A name nobody chose, so a feed that does introduce itself still may.
    assert!(is_unnamed(&host_named));
    assert!(is_unnamed(&Subscription { name: placeholder_name(150), ..host_named.clone() }));
    // A name a person typed is left alone.
    assert!(!is_unnamed(&Subscription { name: "Uni".into(), ..host_named }));
}

#[test]
fn better_name_prefers_the_feed_then_the_host_and_never_overrides_a_typed_name() {
    // Not one of today's tests: `better_name` had none of its own.
    let sub = Subscription {
        id: 7,
        name: placeholder_name(7),
        url: "https://www.example.com/a.ics".into(),
        color: [0, 0, 0, 255],
        enabled: true,
    };
    let titled = Overlay::sealed(
        Vec::new(),
        vec![FetchStatus {
            subscription: 7,
            at: at("2026-10-20T09:50:00+02:00"),
            outcome: FetchOutcome::Ok { events: 0, problems: Vec::new(), title: Some("  Work  ".into()) },
        }],
    );
    assert_eq!(better_name(&sub, &titled).as_deref(), Some("Work"));
    assert_eq!(better_name(&sub, &Overlay::default()).as_deref(), Some("example.com"));
    let host_named = Subscription { name: "example.com".into(), ..sub.clone() };
    assert_eq!(better_name(&host_named, &Overlay::default()), None, "already the best there is");
    let typed = Subscription { name: "Mine".into(), ..sub };
    assert_eq!(better_name(&typed, &titled), None);
}

#[test]
fn an_overlay_is_bounded_and_sorted_however_it_was_handed_over() {
    let long = "x".repeat(SUMMARY_MAX_CHARS + 40);
    let overlay = Overlay::sealed(
        vec![
            event(1, day(2026, 9, 10), 600, 660, &long),
            event(1, day(2026, 9, 9), 540, 600, "earlier"),
            // Ends before it starts: not an event.
            event(1, day(2026, 9, 11), 600, 300, "backwards"),
            // Past the end of a day: clamped, not refused.
            event(1, day(2026, 9, 12), 0, 99_999, "long"),
        ],
        Vec::new(),
    );
    let days: Vec<NaiveDate> = overlay.events.iter().map(|e| e.day).collect();
    assert_eq!(days, vec![day(2026, 9, 9), day(2026, 9, 10), day(2026, 9, 12)]);
    assert_eq!(overlay.events.iter().find(|e| e.day == day(2026, 9, 12)).map(|e| e.end), Some(planner::DAY_MINUTES));
    assert!(overlay.events.iter().all(|e| e.summary.chars().count() <= SUMMARY_MAX_CHARS));
}

#[test]
fn the_digest_ignores_the_order_a_feed_listed_things_in_and_ignores_the_clock() {
    let one = Overlay::sealed(
        vec![event(1, day(2026, 9, 10), 600, 660, "A"), event(1, day(2026, 9, 10), 540, 600, "B")],
        Vec::new(),
    );
    let other = Overlay::sealed(
        vec![event(1, day(2026, 9, 10), 540, 600, "B"), event(1, day(2026, 9, 10), 600, 660, "A")],
        Vec::new(),
    );
    assert_eq!(one.digest(), other.digest());

    // The same events fetched at a different moment are the same events.
    let later = Overlay::sealed(
        one.events.clone(),
        vec![FetchStatus {
            subscription: 1,
            at: at("2026-09-10T12:00:00+03:00"),
            outcome: FetchOutcome::Ok { events: 2, problems: Vec::new(), title: None },
        }],
    );
    assert_eq!(one.digest(), later.digest(), "the time of the last attempt is not a change");

    // A real change is a change.
    let changed = Overlay::sealed(vec![event(1, day(2026, 9, 10), 600, 660, "A renamed")], Vec::new());
    assert_ne!(one.digest(), changed.digest());

    // Including a room that moved.
    let mut relocated = one.events.clone();
    if let Some(first) = relocated.first_mut() {
        first.location = "Chemicum, sali A110".into();
    }
    assert_ne!(one.digest(), Overlay::sealed(relocated, Vec::new()).digest());
}

#[test]
fn the_digest_is_today_fnv_1a_over_the_events_byte_for_byte() {
    // Worked by hand from today's `digest`: FNV-1a 64 over, per event, the
    // subscription (8 bytes LE), the day as "%Y%m%d", start and end (4 bytes
    // LE each), all_day and free (one byte each), then summary, location and
    // description. No events: the offset basis itself.
    assert_eq!(Overlay::default().digest(), 0xcbf2_9ce4_8422_2325);
    let one = Overlay::sealed(vec![event(1, day(2026, 9, 10), 600, 660, "A")], Vec::new());
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend(1u64.to_le_bytes());
    bytes.extend(b"20260910");
    bytes.extend(600i32.to_le_bytes());
    bytes.extend(660i32.to_le_bytes());
    bytes.extend([0, 0]);
    bytes.extend(b"A");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    assert_eq!(one.digest(), hash);
}

#[test]
fn the_digest_day_is_written_as_chrono_writes_percent_y_m_d() {
    // `ymd` stands in for today's `day.format("%Y%m%d")`, which the core's
    // chrono (no `alloc`) lacks; the tests' chrono has it.
    for d in [
        day(2026, 9, 10),
        day(1, 1, 1),
        day(999, 12, 31),
        day(9999, 12, 31),
        day(10_000, 1, 1),
        day(0, 2, 29),
        day(-1, 3, 4),
        day(-12_345, 6, 7),
        NaiveDate::MIN,
        NaiveDate::MAX,
    ] {
        assert_eq!(ymd(d), d.format("%Y%m%d").to_string(), "{d:?}");
    }
}

#[test]
fn an_all_day_band_and_a_free_marker_are_drawn_but_never_planned_around() {
    let mut all_day = event(1, day(2026, 9, 10), 0, planner::DAY_MINUTES, "Conference");
    all_day.all_day = true;
    let mut free = event(1, day(2026, 9, 10), 540, 600, "due: Tax return");
    free.free = true;
    let busy = event(1, day(2026, 9, 10), 720, 780, "Dentist");
    let overlay = Overlay::sealed(vec![all_day, free, busy], Vec::new());

    assert_eq!(overlay.events_on(day(2026, 9, 10)).count(), 3);
    assert_eq!(overlay.anchors_on(day(2026, 9, 10)), vec![(720, 780)]);
    assert!(overlay.anchors_on(day(2026, 9, 11)).is_empty());
}

#[test]
fn forgetting_a_subscription_takes_its_events_and_its_status_with_it() {
    let mut overlay = Overlay::sealed(
        vec![event(1, day(2026, 9, 10), 600, 660, "mine"), event(2, day(2026, 9, 10), 700, 760, "theirs")],
        vec![
            FetchStatus {
                subscription: 1,
                at: at("2026-09-10T12:00:00+03:00"),
                outcome: FetchOutcome::Ok { events: 1, problems: Vec::new(), title: None },
            },
            FetchStatus {
                subscription: 2,
                at: at("2026-09-10T12:00:00+03:00"),
                outcome: FetchOutcome::Failed { why: "no".into() },
            },
        ],
    );
    overlay.forget(2);
    assert_eq!(overlay.events.len(), 1);
    assert_eq!(overlay.events.first().map(|e| e.subscription), Some(1));
    assert!(overlay.status_for(2).is_none());
    assert!(overlay.status_for(1).is_some());
}

#[test]
fn a_failure_reason_is_bounded_when_sealed() {
    // Not one of today's tests: the `why` bound of today's `sealed`.
    let overlay = Overlay::sealed(
        Vec::new(),
        vec![FetchStatus {
            subscription: 1,
            at: at("2026-09-10T12:00:00+03:00"),
            outcome: FetchOutcome::Failed { why: "w".repeat(WHY_MAX_CHARS + 5) },
        }],
    );
    match overlay.status_for(1).map(|s| &s.outcome) {
        Some(FetchOutcome::Failed { why }) => assert_eq!(why.chars().count(), WHY_MAX_CHARS),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_span_long_enough_to_describe_the_day_is_drawn_but_never_planned_around() {
    // Today's test used `Local::now().date_naive()`; any day will do, and no
    // test reads the clock (CORE-GEN-003).
    let today = day(2026, 9, 10);
    let period = event(1, today, 8 * 60, 20 * 60, "CS-C3240, Machine Learning, Lähiopetus 1.9.–30.11.");
    let lecture = event(1, today, 10 * 60 + 15, 12 * 60, "MOLE-101, Biokemia");
    let overlay = Overlay::sealed(vec![period, lecture], Vec::new());

    assert_eq!(overlay.events_on(today).count(), 2, "both are still shown");
    assert_eq!(overlay.anchors_on(today), vec![(10 * 60 + 15, 12 * 60)], "only the lecture takes an hour out of the day");
    let long = overlay.events.iter().find(|e| e.minutes() >= LONG_EVENT_MINUTES).expect("the period");
    assert!(long.is_background() && !long.is_busy());

    for (start, end, what) in [
        (16 * 60, 22 * 60, "board game night, six hours"),
        (12 * 60, 20 * 60, "tournament, eight hours"),
        (14 * 60, 24 * 60, "the long one, ten hours"),
        (9 * 60, 13 * 60, "lab, four hours"),
    ] {
        assert!(event(1, today, start, end, what).is_busy(), "{what} is an appointment");
    }
    assert!(event(1, today, 0, 24 * 60, "conference").is_background());
}

#[test]
fn the_span_is_not_part_of_what_counts_as_a_change() {
    let events = vec![event(1, day(2026, 9, 4), 600, 660, "Lecture")];
    let monday = Overlay::sealed(events.clone(), Vec::new())
        .covering(Some(Covered { from: day(2026, 7, 7), until: day(2027, 10, 10) }));
    let tuesday = Overlay::sealed(events, Vec::new())
        .covering(Some(Covered { from: day(2026, 7, 8), until: day(2027, 10, 11) }));
    assert_eq!(monday.digest(), tuesday.digest());
}

#[test]
fn a_day_is_inside_the_span_at_both_of_its_ends() {
    let covers = Covered { from: day(2026, 7, 7), until: day(2027, 10, 10) };
    assert!(covers.holds(day(2026, 7, 7)));
    assert!(covers.holds(day(2027, 10, 10)));
    assert!(!covers.holds(day(2026, 7, 6)));
    assert!(!covers.holds(day(2027, 10, 11)));
}

#[test]
fn the_span_the_calendars_were_read_for_survives_serde_and_an_older_cache_still_reads() {
    // The serde half of today's `the_span_the_calendars_were_read_for_survives_the_cache`
    // (its folder half is the device's).
    let covers = Covered { from: day(2026, 7, 7), until: day(2027, 10, 10) };
    let overlay =
        Overlay::sealed(vec![event(1, day(2026, 9, 4), 600, 660, "Lecture")], Vec::new()).covering(Some(covers));
    let text = serde_json::to_string(&overlay).expect("writes");
    let back: Overlay = serde_json::from_str(&text).expect("reads");
    assert_eq!(back.covers, Some(covers));
    let older: Overlay = serde_json::from_str(r#"{"events":[],"status":[]}"#).expect("reads");
    assert_eq!(older.covers, None);
}

// brief c2 item 2 (core.md §14.6): `FetchStatus.at` at a fixed offset reads
// and writes today's `subscribed_cache.json` unchanged.
#[test]
fn todays_subscribed_cache_reads_and_writes_back_unchanged() {
    let overlay: Overlay = serde_json::from_str(SUBSCRIBED_CACHE).expect("today's cache reads");
    assert!(!overlay.events.is_empty() && !overlay.status.is_empty());
    for status in &overlay.status {
        // The fixture's times are at +02:00, and the offset is kept.
        assert_eq!(status.at.offset().local_minus_utc(), 2 * 3600);
        assert_eq!(status.at.to_rfc3339(), "2026-10-20T09:50:00+02:00");
    }
    // Written back, it is today's file byte for byte: the fixture is compact
    // JSON in field order, as today's `serde_json::to_string` wrote it.
    let written = serde_json::to_string(&overlay).expect("writes");
    assert_eq!(written, SUBSCRIBED_CACHE.trim_end());
    // And what it wrote reads back equal.
    let again: Overlay = serde_json::from_str(&written).expect("reads");
    assert_eq!(again, overlay);
}
