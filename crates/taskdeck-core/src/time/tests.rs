use super::*;

fn at(text: &str) -> Instant {
    Instant::parse_canonical(text).unwrap_or(Instant::MIN)
}

fn naive(y: i32, m: u32, d: u32, h: u32, mi: u32) -> chrono::NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(y, m, d).and_then(|x| x.and_hms_opt(h, mi, 0)).unwrap_or_default()
}

// spec: CORE-TIME-005
#[test]
fn table_zone_periods() {
    let z = TableZone::new(3600, vec![(1_774_746_000_000, 7200), (1_792_890_000_000, 3600)]).unwrap_or_else(|_| panic!("table"));
    assert_eq!(z.offset_at(Instant::MIN), 3600);
    assert_eq!(z.offset_at(Instant::MAX), 3600);
    assert_eq!(z.from_local(naive(2026, 7, 1, 12, 0)), LocalTime::Single(at("2026-07-01T10:00:00.000000000Z")));
    // An empty table is a fixed zone.
    let e = TableZone::new(-7200, vec![]).unwrap_or_else(|_| panic!("table"));
    assert_eq!(e.offset_at(at("2026-07-01T10:00:00.000000000Z")), -7200);
    assert_eq!(e.from_local(naive(2026, 7, 1, 8, 0)), LocalTime::Single(at("2026-07-01T10:00:00.000000000Z")));
}

#[test]
fn three_candidates_keep_the_first_and_the_last() {
    // Two fall-backs of an hour within an hour of UTC wall time: 00:30 local
    // exists in three periods.
    let z = TableZone::new(7200, vec![(0, 3600), (3_600_000, 0)]).unwrap_or_else(|_| panic!("table"));
    match z.from_local(naive(1970, 1, 1, 1, 30)) {
        LocalTime::Ambiguous(a, b) => assert!(a < b),
        other => panic!("{other:?}"),
    }
}

// (zone conversions never produce an out-of-range instant)
// spec: CORE-TIME-001
#[test]
fn zone_conversions_stay_in_range() {
    assert_eq!(FixedZone(3600).from_local(naive(1970, 1, 1, 0, 30)), LocalTime::None);
    assert_eq!(FixedZone(-3600).from_local(naive(9999, 12, 31, 23, 30)), LocalTime::None);
    assert_eq!(FixedZone(0).from_local(naive(9999, 12, 31, 23, 59)), LocalTime::Single(at("9999-12-31T23:59:00.000000000Z")));
    let c = ChronoZone(chrono::FixedOffset::east_opt(3600).unwrap_or(chrono::FixedOffset::east_opt(0).unwrap_or_else(|| panic!("offset"))));
    assert_eq!(c.from_local(naive(1970, 1, 1, 0, 30)), LocalTime::None);
    assert_eq!(c.offset_at(Instant::MIN), 3600);
    assert_eq!(c.from_local(naive(2026, 9, 4, 10, 0)), LocalTime::Single(at("2026-09-04T09:00:00.000000000Z")));
}

// spec: CORE-TIME-002
#[test]
fn local_day_and_minutes_come_from_the_offset() {
    let t = at("2026-09-04T23:30:00.000000000Z");
    assert_eq!(local_day(t, &FixedZone(0)), chrono::NaiveDate::from_ymd_opt(2026, 9, 4).unwrap_or_default());
    assert_eq!(local_minutes(t, &FixedZone(0)), 1410);
    assert_eq!(local_day(t, &FixedZone(1800)), chrono::NaiveDate::from_ymd_opt(2026, 9, 5).unwrap_or_default());
    assert_eq!(local_minutes(t, &FixedZone(1800)), 0);
    let dz: &dyn Zone = &FixedZone(-60);
    assert_eq!(local_minutes(t, dz), 1409);
    assert_eq!(now_marker(local_day(t, dz), t, dz), Some(1409));
}

// spec: CORE-TIME-004
#[test]
fn snap_matches_rounding_half_away_from_zero() {
    for m in -50i64..1600 {
        // Today's arithmetic (planner.rs: snap) worked in integers: round(m / 15)
        // with m / 15 never exactly half, clamped to 0..=96 steps.
        let steps = if m <= 0 { 0 } else { (2 * m + 15) / 30 };
        assert_eq!(i64::from(snap(m)), steps.clamp(0, 96) * 15, "{m}");
    }
}
