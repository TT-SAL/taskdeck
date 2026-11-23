// The fixtures' process zones as `TableZone`s (core.md §5.2, §18.5).
//
// Tests cannot set `TZ`, so the two zones the fixtures were recorded in are
// generated here from today's rules for the years 2000 to 2100, with
// standard time outside them. The fixture data lies in 2022 to 2027 plus
// winter days of 1970 and 9999, where both zones are on standard time.

use chrono::{Datelike, NaiveDate, Weekday};
use taskdeck_core::time::TableZone;

const YEARS: std::ops::RangeInclusive<i32> = 2000..=2100;

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("a date")
}

/// The last Sunday of a month.
fn last_sunday(y: i32, m: u32) -> NaiveDate {
    let mut d = if m == 12 { date(y + 1, 1, 1) } else { date(y, m + 1, 1) }.pred_opt().expect("a date");
    while d.weekday() != Weekday::Sun {
        d = d.pred_opt().expect("a date");
    }
    d
}

/// The `n`-th Sunday of a month, from 1.
fn nth_sunday(y: i32, m: u32, n: u32) -> NaiveDate {
    let mut d = date(y, m, 1);
    while d.weekday() != Weekday::Sun {
        d = d.succ_opt().expect("a date");
    }
    d + chrono::Duration::weeks(i64::from(n - 1))
}

/// Unix milliseconds of `hour`:00 UTC on a day.
fn utc_ms(d: NaiveDate, hour: u32) -> i64 {
    d.and_hms_opt(hour, 0, 0).expect("a time").and_utc().timestamp_millis()
}

/// `Europe/Berlin`: +1 h, and +2 h from 01:00 UTC on the last Sunday of March
/// to 01:00 UTC on the last Sunday of October.
pub fn berlin() -> TableZone {
    let mut transitions = Vec::new();
    for y in YEARS {
        transitions.push((utc_ms(last_sunday(y, 3), 1), 7200));
        transitions.push((utc_ms(last_sunday(y, 10), 1), 3600));
    }
    TableZone::new(3600, transitions).expect("a valid table")
}

/// `America/New_York`: −5 h, and −4 h from 02:00 local (07:00 UTC) on the
/// second Sunday of March to 02:00 local (06:00 UTC) on the first Sunday of
/// November.
pub fn new_york() -> TableZone {
    let mut transitions = Vec::new();
    for y in YEARS {
        transitions.push((utc_ms(nth_sunday(y, 3, 2), 7), -14_400));
        transitions.push((utc_ms(nth_sunday(y, 11, 1), 6), -18_000));
    }
    TableZone::new(-18_000, transitions).expect("a valid table")
}

/// The zone of a directory under `fixtures/snapshots/`.
pub fn of_directory(name: &str) -> TableZone {
    match name {
        "europe-berlin" => berlin(),
        "america-new-york" => new_york(),
        _ => panic!("no zone for {name}"),
    }
}
