//! Time, zones and planner arithmetic (core.md §5, §20).
//!
//! All arithmetic is on integers (CORE-TIME-004, CORE-GEN-006); chrono is used
//! only for calendar conversions, which read no clock.

use chrono::{Datelike, Offset, Timelike};

use crate::CoreError;

/// A UTC instant with nanosecond resolution (core.md §5.1).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Instant {
    secs: i64,
    nanos: u32,
}

const NANOS_PER_SEC: u32 = 1_000_000_000;
const MAX_SECS: i64 = 253_402_300_799;
/// The largest `at_ms` (`seal.md` §10.1): 9999-12-31T23:59:59.999Z.
const MAX_AT_MS: i64 = 253_402_300_799_999;

// The spec fixes `&self` receivers on this `Copy` type (core.md §20).
#[allow(clippy::wrong_self_convention)]
impl Instant {
    /// 1970-01-01T00:00:00.000000000Z (core.md §5.1).
    pub const MIN: Instant = Instant { secs: 0, nanos: 0 };
    /// 9999-12-31T23:59:59.999999999Z (core.md §5.1).
    pub const MAX: Instant = Instant { secs: MAX_SECS, nanos: 999_999_999 };

    /// An instant from seconds and nanoseconds, within range (core.md §5.1).
    pub fn from_parts(secs: i64, nanos: u32) -> Option<Instant> {
        if (0..=MAX_SECS).contains(&secs) && nanos < NANOS_PER_SEC {
            Some(Instant { secs, nanos })
        } else {
            None
        }
    }

    /// An instant from a plaintext's `at_ms` (core.md §5.1).
    pub fn from_at_ms(at_ms: i64) -> Option<Instant> {
        if !(0..=MAX_AT_MS).contains(&at_ms) {
            return None;
        }
        let ms = u32::try_from(at_ms % 1000).ok()?;
        Instant::from_parts(at_ms / 1000, ms * 1_000_000)
    }

    /// The 30-byte canonical instant text (core.md §4.4).
    pub fn to_canonical(&self) -> String {
        let t = self.to_chrono();
        let mut out = String::with_capacity(30);
        push_digits(&mut out, i64::from(t.year()), 4);
        out.push('-');
        push_digits(&mut out, i64::from(t.month()), 2);
        out.push('-');
        push_digits(&mut out, i64::from(t.day()), 2);
        out.push('T');
        push_digits(&mut out, i64::from(t.hour()), 2);
        out.push(':');
        push_digits(&mut out, i64::from(t.minute()), 2);
        out.push(':');
        push_digits(&mut out, i64::from(t.second()), 2);
        out.push('.');
        push_digits(&mut out, i64::from(self.nanos), 9);
        out.push('Z');
        out
    }

    /// Parses the canonical instant text (core.md §4.4).
    pub fn parse_canonical(text: &str) -> Option<Instant> {
        let b = text.as_bytes();
        if b.len() != 30
            || b[10] != b'T'
            || b[13] != b':'
            || b[16] != b':'
            || b[19] != b'.'
            || b[29] != b'Z'
        {
            return None;
        }
        let day = parse_ymd(&b[..10])?;
        let hour = digits(&b[11..13])?;
        let minute = digits(&b[14..16])?;
        let second = digits(&b[17..19])?;
        let nanos = digits(&b[20..29])?;
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        let secs = day.and_hms_opt(u32::try_from(hour).ok()?, u32::try_from(minute).ok()?, u32::try_from(second).ok()?)?;
        Instant::from_parts(secs.and_utc().timestamp(), u32::try_from(nanos).ok()?)
    }

    /// An instant from a chrono UTC time, within range (core.md §5.1).
    pub fn from_chrono(t: chrono::DateTime<chrono::Utc>) -> Option<Instant> {
        // A chrono leap second (nanos of 10^9 or more) is outside the range.
        Instant::from_parts(t.timestamp(), t.timestamp_subsec_nanos())
    }

    /// The instant as a chrono UTC time (core.md §5.1).
    pub fn to_chrono(&self) -> chrono::DateTime<chrono::Utc> {
        // Every instant of the range is a chrono time.
        chrono::DateTime::from_timestamp(self.secs, self.nanos).unwrap_or_default()
    }

    /// Adds seconds, saturating at `MAX` (core.md §5.1).
    pub fn plus_seconds(&self, s: i64) -> Option<Instant> {
        match self.secs.checked_add(s) {
            Some(secs) if secs > MAX_SECS => Some(Instant::MAX),
            Some(secs) => Instant::from_parts(secs, self.nanos),
            None if s > 0 => Some(Instant::MAX),
            None => None,
        }
    }

    /// Unix milliseconds, floored (core.md §5.1).
    pub fn as_unix_ms(&self) -> i64 {
        self.secs * 1000 + i64::from(self.nanos / 1_000_000)
    }
}

/// Appends `v` (non-negative) in decimal, zero-padded to `width` digits.
pub(crate) fn push_digits(out: &mut String, v: i64, width: usize) {
    let s = v.to_string();
    for _ in s.len()..width {
        out.push('0');
    }
    out.push_str(&s);
}

/// The value of a run of ASCII digits, or none.
fn digits(b: &[u8]) -> Option<i64> {
    let mut v: i64 = 0;
    for &c in b {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v * 10 + i64::from(c - b'0');
    }
    Some(v)
}

/// `YYYY-MM-DD` with the rules of a `date` (core.md §4.4): year 1970 to
/// 9999, a day that exists in its month.
pub(crate) fn parse_ymd(b: &[u8]) -> Option<chrono::NaiveDate> {
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let year = digits(&b[..4])?;
    let month = digits(&b[5..7])?;
    let day = digits(&b[8..10])?;
    if !(1970..=9999).contains(&year) {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, u32::try_from(month).ok()?, u32::try_from(day).ok()?)
}

/// The instants of a local wall time, ascending (core.md §5.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

/// The largest offset magnitude a zone reports (core.md §5.2).
const MAX_OFFSET: i32 = 86_399;

/// A constant offset in seconds (core.md §5.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FixedZone(pub i32);

/// An initial offset and its transitions `(utc_ms, offset_s)` (core.md §5.2).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TableZone {
    pub initial: i32,
    pub transitions: Vec<(i64, i32)>,
}

impl TableZone {
    /// A table zone; transitions strictly ascending (core.md §5.2).
    pub fn new(initial: i32, transitions: Vec<(i64, i32)>) -> Result<Self, CoreError> {
        let offset_ok = |o: i32| (-MAX_OFFSET..=MAX_OFFSET).contains(&o);
        if !offset_ok(initial)
            || transitions.iter().any(|&(_, o)| !offset_ok(o))
            || transitions.windows(2).any(|w| w[0].0 >= w[1].0)
        {
            // A bad table is `core/schema` (core.md §5.2).
            return Err(crate::canon::schema());
        }
        Ok(TableZone { initial, transitions })
    }

    /// The period of `t`: 0 before the first transition, `k` from transition `k − 1` on.
    fn period(&self, t: Instant) -> usize {
        let ms = t.as_unix_ms();
        self.transitions.partition_point(|&(at, _)| at <= ms)
    }

    fn period_offset(&self, p: usize) -> i32 {
        match p.checked_sub(1) {
            None => self.initial,
            Some(k) => self.transitions.get(k).map_or(self.initial, |&(_, o)| o),
        }
    }
}

/// A zone delegated to chrono (core.md §5.2).
pub struct ChronoZone<Z: chrono::TimeZone>(pub Z);

/// The instant whose UTC wall time is `local` minus `offset` seconds, if in range.
fn local_minus(local: chrono::NaiveDateTime, offset: i32) -> Option<Instant> {
    let utc = local.and_utc();
    let secs = utc.timestamp().checked_sub(i64::from(offset))?;
    Instant::from_parts(secs, utc.timestamp_subsec_nanos())
}

fn local_time(found: &[Instant]) -> LocalTime {
    match found {
        [] => LocalTime::None,
        [one] => LocalTime::Single(*one),
        // Three or more instants: the first and the last (core.md §5.2).
        [first, .., last] => LocalTime::Ambiguous(*first, *last),
    }
}

impl Zone for FixedZone {
    fn offset_at(&self, _utc: Instant) -> i32 {
        self.0.clamp(-MAX_OFFSET, MAX_OFFSET)
    }
    fn from_local(&self, local: chrono::NaiveDateTime) -> LocalTime {
        match local_minus(local, self.0.clamp(-MAX_OFFSET, MAX_OFFSET)) {
            Some(t) => LocalTime::Single(t),
            None => LocalTime::None,
        }
    }
}

impl Zone for TableZone {
    fn offset_at(&self, utc: Instant) -> i32 {
        self.period_offset(self.period(utc)).clamp(-MAX_OFFSET, MAX_OFFSET)
    }
    fn from_local(&self, local: chrono::NaiveDateTime) -> LocalTime {
        // Every period p with offset o gives the candidate l − o, kept when
        // it falls inside p (core.md §5.2). Periods are in time order, so the
        // kept candidates are ascending. An offset is below a day, so only
        // the periods that meet the day on either side of `l` can hold one.
        let l_ms = local.and_utc().timestamp_millis();
        let first = self.transitions.partition_point(|&(at, _)| at <= l_ms.saturating_sub(86_400_000));
        let last = self.transitions.partition_point(|&(at, _)| at <= l_ms.saturating_add(86_400_000));
        let mut found = Vec::new();
        for p in first..=last {
            let o = self.period_offset(p).clamp(-MAX_OFFSET, MAX_OFFSET);
            if let Some(t) = local_minus(local, o)
                && self.period(t) == p
                && found.last().is_none_or(|&prev| prev < t)
            {
                found.push(t);
            }
        }
        local_time(&found)
    }
}

impl<Z: chrono::TimeZone> Zone for ChronoZone<Z> {
    fn offset_at(&self, utc: Instant) -> i32 {
        let o = self.0.offset_from_utc_datetime(&utc.to_chrono().naive_utc()).fix().local_minus_utc();
        o.clamp(-MAX_OFFSET, MAX_OFFSET)
    }
    fn from_local(&self, local: chrono::NaiveDateTime) -> LocalTime {
        let conv = |t: chrono::DateTime<Z>| Instant::from_chrono(t.with_timezone(&chrono::Utc));
        // `earliest` and `latest` are the two instants of an ambiguous
        // result, the one of a single result twice, and none for a gap.
        // chrono does not promise their order (`Local` lists the later
        // instant first in a fall-back), and `Local` also reports readings
        // that are not real (03:00+02:00 just after a fall-back to +01:00).
        // So each candidate is kept only when it reads back as `local`, and
        // the kept ones are sorted (core.md §5.2: every instant, ascending).
        let r = self.0.from_local_datetime(&local);
        let mut found: Vec<Instant> = [r.clone().earliest(), r.latest()]
            .into_iter()
            .flatten()
            .filter_map(conv)
            .filter(|&t| local_minus(local, self.offset_at(t)) == Some(t))
            .collect();
        found.sort();
        found.dedup();
        local_time(&found)
    }
}

/// The local wall time of `t` in `z`, as seconds of a naive time line.
fn local_secs(t: Instant, z: &(impl Zone + ?Sized)) -> i64 {
    t.secs + i64::from(z.offset_at(t).clamp(-MAX_OFFSET, MAX_OFFSET))
}

/// The local day of an instant (core.md §5.3).
pub fn local_day(t: Instant, z: &(impl Zone + ?Sized)) -> chrono::NaiveDate {
    let days = local_secs(t, z).div_euclid(86_400);
    // Days from 0001-01-01 (day 1 of the common era) to 1970-01-01: 719,163.
    i32::try_from(days + 719_163)
        .ok()
        .and_then(chrono::NaiveDate::from_num_days_from_ce_opt)
        .unwrap_or_default()
}

/// The local minutes since midnight of an instant (core.md §5.3).
pub fn local_minutes(t: Instant, z: &(impl Zone + ?Sized)) -> i32 {
    // Within 0 to 1439, so the conversion cannot fail.
    i32::try_from(local_secs(t, z).rem_euclid(86_400) / 60).unwrap_or(0)
}

/// The naive date-time of `local_at(d, x)` (core.md §5.4 step 2): `d` at
/// hour `min(x div 60, 23)`, minute `x mod 60`, second 0.
fn local_naive(day: chrono::NaiveDate, x: i32) -> Option<chrono::NaiveDateTime> {
    let hour = u32::try_from((x / 60).min(23)).ok()?;
    let minute = u32::try_from(x % 60).ok()?;
    day.and_hms_opt(hour, minute, 0)
}

/// `local_at(d, x)` of core.md §5.4 step 2: on a fold the earlier instant
/// (CORE-TIME-003), which `from_local` lists first (§5.2).
fn local_at(day: chrono::NaiveDate, x: i32, z: &(impl Zone + ?Sized)) -> Option<Instant> {
    match z.from_local(local_naive(day, x)?) {
        LocalTime::None => None,
        LocalTime::Single(t) | LocalTime::Ambiguous(t, _) => Some(t),
    }
}

/// Whether a local wall time lies outside the instant range (core.md §5.4;
/// lead ruling, spec change 5, §5.4): its seconds since
/// 1970-01-01T00:00:00 minus `z.offset_at(Instant::MIN)` are below 0, or
/// minus `z.offset_at(Instant::MAX)` are above 253,402,300,799.
fn naive_outside_range(local: chrono::NaiveDateTime, z: &(impl Zone + ?Sized)) -> bool {
    let secs = local.and_utc().timestamp();
    secs - i64::from(z.offset_at(Instant::MIN)) < 0 || secs - i64::from(z.offset_at(Instant::MAX)) > MAX_SECS
}

/// Whether the local time `resolve_on_day(day, minutes, z)` reads first,
/// `local_at(day, clamp(minutes, 0, 1439))`, lies outside the instant range
/// (lead ruling, spec change 5, §5.4). `resolve_on_day` gives none for such a
/// time without the 60-minute step; `compile` answers a day and minutes there
/// `core/date_range`, not as a gap, and a naive wire time there `not a time`
/// (§13.3 G3 and G5).
pub(crate) fn outside_range(day: chrono::NaiveDate, minutes: i32, z: &(impl Zone + ?Sized)) -> bool {
    local_naive(day, minutes.clamp(0, 1439)).is_some_and(|local| naive_outside_range(local, z))
}

/// The local date-time of `t` in `z` (core.md §5.3): `t + z.offset_at(t)`
/// read as a naive date-time, for the read queries that print one.
pub(crate) fn local_date_time(t: Instant, z: &(impl Zone + ?Sized)) -> chrono::NaiveDateTime {
    let secs = local_secs(t, z);
    // Within a day of the instant range, so chrono holds it.
    chrono::DateTime::from_timestamp(secs, t.nanos).map(|d| d.naive_utc()).unwrap_or_default()
}

/// `to − from` as a chrono span: a difference of two instants needs no zone.
pub(crate) fn between(from: Instant, to: Instant) -> chrono::TimeDelta {
    to.to_chrono() - from.to_chrono()
}

/// English month names, January first (chrono's `%B`, without locales).
const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
    "November", "December",
];

/// chrono's `%B`: the month's full English name.
pub(crate) fn month_name(month: u32) -> &'static str {
    let index = usize::try_from(month.saturating_sub(1)).unwrap_or(0);
    MONTH_NAMES.get(index).copied().unwrap_or("")
}

/// chrono's `%b`: the month's first three letters.
pub(crate) fn month_abbrev(month: u32) -> &'static str {
    let name = month_name(month);
    name.get(..3).unwrap_or(name)
}

/// Resolves a local time on a day (core.md §5.4).
///
/// A local time outside the instant range is no gap: the result is none, and
/// step 3 does not try 60 minutes later (lead ruling, spec change 5, §5.4).
pub fn resolve_on_day(day: chrono::NaiveDate, minutes: i32, z: &(impl Zone + ?Sized)) -> Option<Instant> {
    let m = minutes.clamp(0, 1439);
    if outside_range(day, m, z) {
        return None;
    }
    local_at(day, m, z).or_else(|| local_at(day, (m + 60).min(1439), z))
}

/// The now-line's minutes on `day`, if today (core.md §5.5).
pub fn now_marker(day: chrono::NaiveDate, now: Instant, z: &(impl Zone + ?Sized)) -> Option<i32> {
    (local_day(now, z) == day).then(|| local_minutes(now, z))
}

/// Snaps minutes to the grid (core.md §5.5).
pub fn snap(m: i64) -> i32 {
    if m <= 0 {
        return 0;
    }
    // At most 96 × 15 = 1440.
    i32::try_from((m.saturating_add(7) / 15).min(96) * 15).unwrap_or(1440)
}

/// Clamps a block to the day (core.md §5.5).
pub fn clamp_block(start: i64, minutes: i64) -> (i32, u32) {
    let length = snap(minutes.max(15)).max(15);
    let start = snap(start).clamp(0, 1440 - length);
    (start, length.unsigned_abs())
}

/// Snaps minutes up to the grid (core.md §5.5).
pub fn snap_up(m: i32) -> i32 {
    let up = (i64::from(m) + 14).div_euclid(15) * 15;
    // Clamped into 0 to 1440, so the conversion cannot fail.
    i32::try_from(up.clamp(0, 1440)).unwrap_or(0)
}

#[cfg(test)]
mod tests;
