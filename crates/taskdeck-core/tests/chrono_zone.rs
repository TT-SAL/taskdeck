//! `ChronoZone::from_local` against chrono zones that list an ambiguous
//! local time later instant first, and that report a wall time just past a
//! fall-back as ambiguous although only one of its readings is real.

use chrono::{FixedOffset, LocalResult, NaiveDate, NaiveDateTime, TimeZone};
use taskdeck_core::time::{self, ChronoZone, Instant, LocalTime, Zone};

/// Berlin's rules around 2026-10-25 (+02:00 until 01:00Z, then +01:00),
/// answered the way chrono's `Local` answers under `TZ=Europe/Berlin`:
/// `Ambiguous(+01:00, +02:00)` for 02:00 to 03:00 inclusive.
#[derive(Clone, Copy, Debug)]
struct LaterFirst;

fn fall_back_utc() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, 25).unwrap().and_hms_opt(1, 0, 0).unwrap()
}

impl TimeZone for LaterFirst {
    type Offset = FixedOffset;
    fn from_offset(_: &FixedOffset) -> Self {
        LaterFirst
    }
    fn offset_from_local_date(&self, _: &NaiveDate) -> LocalResult<FixedOffset> {
        LocalResult::None
    }
    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<FixedOffset> {
        let (winter, summer) = (FixedOffset::east_opt(3600).unwrap(), FixedOffset::east_opt(7200).unwrap());
        let start = fall_back_utc() + chrono::Duration::hours(1);
        let end = fall_back_utc() + chrono::Duration::hours(2);
        if *local < start {
            LocalResult::Single(summer)
        } else if *local <= end {
            LocalResult::Ambiguous(winter, summer)
        } else {
            LocalResult::Single(winter)
        }
    }
    fn offset_from_utc_date(&self, _: &NaiveDate) -> FixedOffset {
        FixedOffset::east_opt(3600).unwrap()
    }
    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
        FixedOffset::east_opt(if *utc < fall_back_utc() { 7200 } else { 3600 }).unwrap()
    }
}

fn at(s: &str) -> Instant {
    Instant::parse_canonical(s).unwrap()
}

// spec: CORE-TIME-003
#[test]
fn chrono_zone_keeps_both_instants_of_a_fall_back_time_in_order() {
    let z = ChronoZone(LaterFirst);
    let day = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
    let earlier = at("2026-10-25T00:30:00.000000000Z");
    let later = at("2026-10-25T01:30:00.000000000Z");
    assert_eq!(z.from_local(day.and_hms_opt(2, 30, 0).unwrap()), LocalTime::Ambiguous(earlier, later));
    assert_eq!(time::resolve_on_day(day, 150, &z), Some(earlier));
}

// spec: CORE-TIME-003
#[test]
fn chrono_zone_drops_a_reading_that_does_not_round_trip() {
    let z = ChronoZone(LaterFirst);
    let l = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap().and_hms_opt(3, 0, 0).unwrap();
    // 03:00+02:00 is 01:00Z, whose offset is already +01:00: not a reading of 03:00.
    assert_eq!(z.from_local(l), LocalTime::Single(at("2026-10-25T02:00:00.000000000Z")));
}

// spec: CORE-TIME-003
#[test]
fn process_zone_readings_round_trip_and_ascend() {
    let z = taskdeck_core::time::ChronoZone(chrono::Local);
    let day = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
    for minutes in (0..24 * 60).step_by(15) {
        let l = day.and_hms_opt(minutes / 60, minutes % 60, 0).unwrap();
        let back = |t: Instant| t.to_chrono().naive_utc() + chrono::Duration::seconds(i64::from(z.offset_at(t)));
        match z.from_local(l) {
            LocalTime::None => {}
            LocalTime::Single(t) => assert_eq!(back(t), l),
            LocalTime::Ambiguous(a, b) => {
                assert!(a < b, "{l}");
                assert_eq!((back(a), back(b)), (l, l));
            }
        }
    }
}
