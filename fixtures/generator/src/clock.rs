//! The pinned zones and the pinned clock.
//!
//! Today's code reads the zone through `chrono::Local`, which follows `TZ`.
//! The generator writes each set under one zone and refuses to run under any
//! other, and it checks that `Local` really is that zone (a machine without the
//! tz database would otherwise quietly give UTC). Every instant it hands to
//! today's code is built from UTC here, so the same instant is meant in both
//! zones; only its printed offset differs.

use chrono::{DateTime, Local, NaiveDate, Offset, TimeZone, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// The pinned zone every board is written in.
    Berlin,
    /// The second zone the snapshots and transcripts are written in again.
    NewYork,
}

pub const ZONES: [Zone; 2] = [Zone::Berlin, Zone::NewYork];

impl Zone {
    /// What `TZ` must be.
    pub fn tz(self) -> &'static str {
        match self {
            Zone::Berlin => "Europe/Berlin",
            Zone::NewYork => "America/New_York",
        }
    }

    /// The directory name under `snapshots/` and `transcripts/`.
    pub fn label(self) -> &'static str {
        match self {
            Zone::Berlin => "europe-berlin",
            Zone::NewYork => "america-new-york",
        }
    }

    /// The command-line name.
    pub fn parse(name: &str) -> Option<Zone> {
        match name {
            "berlin" => Some(Zone::Berlin),
            "new-york" => Some(Zone::NewYork),
            _ => None,
        }
    }

    /// Seconds east of UTC in mid-January and mid-July 2026.
    fn offsets(self) -> (i32, i32) {
        match self {
            Zone::Berlin => (3600, 7200),
            Zone::NewYork => (-5 * 3600, -4 * 3600),
        }
    }
}

/// Why the generator must not run for `zone` with this `TZ`, if it must not.
pub fn zone_refusal(zone: Zone, tz: Option<&str>) -> Option<String> {
    match tz {
        Some(value) if value == zone.tz() => None,
        Some(value) => Some(format!("TZ is `{value}`; this set is written with TZ={}", zone.tz())),
        None => Some(format!("TZ is not set; this set is written with TZ={}", zone.tz())),
    }
}

/// Refuse unless `TZ` names `zone` and `chrono::Local` agrees.
pub fn check_zone(zone: Zone) -> Result<(), String> {
    if let Some(why) = zone_refusal(zone, std::env::var("TZ").ok().as_deref()) {
        return Err(why);
    }
    let offset = |month| {
        Utc.with_ymd_and_hms(2026, month, 15, 12, 0, 0).unwrap().with_timezone(&Local).offset().fix().local_minus_utc()
    };
    let seen = (offset(1), offset(7));
    if seen != zone.offsets() {
        return Err(format!(
            "TZ={} but the local offsets are {seen:?} seconds (expected {:?}): is the tz database installed?",
            zone.tz(),
            zone.offsets()
        ));
    }
    Ok(())
}

/// An instant given in UTC, as today's code holds it.
pub fn utc(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Local> {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second).unwrap().with_timezone(&Local)
}

pub fn day(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).unwrap()
}

/// An instant as today's serialisation writes it (RFC 3339, local offset).
pub fn text(at: DateTime<Local>) -> String {
    serde_json::to_value(at).unwrap().as_str().unwrap().to_string()
}

/// An instant for a file name: UTC, `YYYYMMDDTHHMMZ`.
pub fn compact(at: DateTime<Local>) -> String {
    at.with_timezone(&Utc).format("%Y%m%dT%H%MZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zone_must_be_named_exactly() {
        assert_eq!(zone_refusal(Zone::Berlin, Some("Europe/Berlin")), None);
        assert!(zone_refusal(Zone::Berlin, Some("America/New_York")).is_some());
        assert!(zone_refusal(Zone::NewYork, Some("EST5EDT")).is_some());
        assert!(zone_refusal(Zone::NewYork, None).is_some());
        assert_eq!(Zone::parse("new-york"), Some(Zone::NewYork));
        assert_eq!(Zone::parse("Europe/Berlin"), None);
    }

    #[test]
    fn compact_names_are_utc() {
        assert_eq!(compact(utc(2026, 10, 25, 1, 30, 0)), "20261025T0130Z");
    }
}
