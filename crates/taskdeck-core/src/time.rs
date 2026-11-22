//! Time, zones and planner arithmetic (core.md §5, §20).

use crate::CoreError;

/// A UTC instant with nanosecond resolution (core.md §5.1).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Instant {
    secs: i64,
    nanos: u32,
}

// The spec fixes `&self` receivers on this `Copy` type (core.md §20).
#[allow(clippy::wrong_self_convention)]
impl Instant {
    /// 1970-01-01T00:00:00.000000000Z (core.md §5.1).
    pub const MIN: Instant = Instant { secs: 0, nanos: 0 };
    /// 9999-12-31T23:59:59.999999999Z (core.md §5.1).
    pub const MAX: Instant = Instant { secs: 253_402_300_799, nanos: 999_999_999 };

    /// An instant from seconds and nanoseconds, within range (core.md §5.1).
    pub fn from_parts(secs: i64, nanos: u32) -> Option<Instant> {
        unimplemented!()
    }

    /// An instant from a plaintext's `at_ms` (core.md §5.1).
    pub fn from_at_ms(at_ms: i64) -> Option<Instant> {
        unimplemented!()
    }

    /// The 30-byte canonical instant text (core.md §4.4).
    pub fn to_canonical(&self) -> String {
        unimplemented!()
    }

    /// Parses the canonical instant text (core.md §4.4).
    pub fn parse_canonical(text: &str) -> Option<Instant> {
        unimplemented!()
    }

    /// An instant from a chrono UTC time, within range (core.md §5.1).
    pub fn from_chrono(t: chrono::DateTime<chrono::Utc>) -> Option<Instant> {
        unimplemented!()
    }

    /// The instant as a chrono UTC time (core.md §5.1).
    pub fn to_chrono(&self) -> chrono::DateTime<chrono::Utc> {
        unimplemented!()
    }

    /// Adds seconds, saturating at `MAX` (core.md §5.1).
    pub fn plus_seconds(&self, s: i64) -> Option<Instant> {
        unimplemented!()
    }

    /// Unix milliseconds, floored (core.md §5.1).
    pub fn as_unix_ms(&self) -> i64 {
        unimplemented!()
    }
}

/// The instants of a local wall time, ascending (core.md §5.2).
pub enum LocalTime {
    None,
    Single(Instant),
    Ambiguous(Instant, Instant),
}

/// The caller's zone (core.md §5.2).
pub trait Zone {
    /// Seconds east of UTC in force at `utc` (core.md §5.2).
    fn offset_at(&self, utc: Instant) -> i32;
    /// Every instant whose local wall time is `local` (core.md §5.2).
    #[allow(clippy::wrong_self_convention)] // the spec's name (core.md §5.2)
    fn from_local(&self, local: chrono::NaiveDateTime) -> LocalTime;
}

/// A constant offset in seconds (core.md §5.2).
pub struct FixedZone(pub i32);

/// An initial offset and its transitions `(utc_ms, offset_s)` (core.md §5.2).
pub struct TableZone {
    pub initial: i32,
    pub transitions: Vec<(i64, i32)>,
}

impl TableZone {
    /// A table zone; transitions strictly ascending (core.md §5.2).
    pub fn new(initial: i32, transitions: Vec<(i64, i32)>) -> Result<Self, CoreError> {
        unimplemented!()
    }
}

/// A zone delegated to chrono (core.md §5.2).
pub struct ChronoZone<Z: chrono::TimeZone>(pub Z);

impl Zone for FixedZone {
    fn offset_at(&self, utc: Instant) -> i32 {
        unimplemented!()
    }
    fn from_local(&self, local: chrono::NaiveDateTime) -> LocalTime {
        unimplemented!()
    }
}

impl Zone for TableZone {
    fn offset_at(&self, utc: Instant) -> i32 {
        unimplemented!()
    }
    fn from_local(&self, local: chrono::NaiveDateTime) -> LocalTime {
        unimplemented!()
    }
}

impl<Z: chrono::TimeZone> Zone for ChronoZone<Z> {
    fn offset_at(&self, utc: Instant) -> i32 {
        unimplemented!()
    }
    fn from_local(&self, local: chrono::NaiveDateTime) -> LocalTime {
        unimplemented!()
    }
}

/// The local day of an instant (core.md §5.3).
pub fn local_day(t: Instant, z: &(impl Zone + ?Sized)) -> chrono::NaiveDate {
    unimplemented!()
}

/// The local minutes since midnight of an instant (core.md §5.3).
pub fn local_minutes(t: Instant, z: &(impl Zone + ?Sized)) -> i32 {
    unimplemented!()
}

/// Resolves a local time on a day (core.md §5.4).
pub fn resolve_on_day(day: chrono::NaiveDate, minutes: i32, z: &(impl Zone + ?Sized)) -> Option<Instant> {
    unimplemented!()
}

/// The now-line's minutes on `day`, if today (core.md §5.5).
pub fn now_marker(day: chrono::NaiveDate, now: Instant, z: &(impl Zone + ?Sized)) -> Option<i32> {
    unimplemented!()
}

/// Snaps minutes to the grid (core.md §5.5).
pub fn snap(m: i64) -> i32 {
    unimplemented!()
}

/// Clamps a block to the day (core.md §5.5).
pub fn clamp_block(start: i64, minutes: i64) -> (i32, u32) {
    unimplemented!()
}

/// Snaps minutes up to the grid (core.md §5.5).
pub fn snap_up(m: i32) -> i32 {
    unimplemented!()
}
