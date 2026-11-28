//! Text through `ics::parse` with today's limits and the phone's window
//! (core.md §14.6, §18.6): never a panic (CORE-GEN-007), its cost bounded
//! (CORE-READ-007; libFuzzer's `-timeout` and `-rss_limit_mb` catch a breach).
#![no_main]

use chrono::NaiveDate;
use libfuzzer_sys::fuzz_target;
use taskdeck_core::ics::{Limits, parse};
use taskdeck_core::time::FixedZone;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let from = NaiveDate::from_ymd_opt(2026, 9, 24).expect("a date");
    let until = NaiveDate::from_ymd_opt(2027, 12, 29).expect("a date");
    let _ = parse(text, from, until, Limits::default(), &FixedZone(3600));
});
