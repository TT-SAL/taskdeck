//! Reading somebody else's calendar.
//!
//! The inverse of `phone::calendar_feed`, and deliberately not its equal. That
//! function writes a format this program controls; this one reads a file
//! fetched from an https URL a user pasted, which is to say **input nobody in
//! this repository wrote**. So it is built the other way round: everything is
//! bounded before it is read, an unreadable event is skipped and counted
//! rather than argued with, and only a file that is structurally not a
//! calendar is refused whole.
//!
//! Nothing here knows about egui, windows, threads or HTTP — the same rule
//! `board.rs` keeps, and for the same reason: it can be tested against a
//! `&str` and nothing else.
//!
//! ## Why a parser of our own
//!
//! The crates that read iCalendar read all of it. This needs a *tenth* of it,
//! and needs that tenth to be bounded, to refuse rather than guess, and to
//! resolve a `TZID` from the file's own `VTIMEZONE` rather than from a copy of
//! the IANA database — half the calendars in the world come out of Outlook,
//! whose `TZID`s (`W. Europe Standard Time`) are not IANA names and would not
//! be found in one. RFC 5545 §3.6.5 requires a file that uses a `TZID` to
//! carry its definition, so the answer is already in the file.
//!
//! What is deliberately left out is listed on `Recurrence`.

use std::collections::HashMap;

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeDelta, TimeZone, Timelike, Utc, Weekday};
use serde::{Deserialize, Serialize};

use crate::{board::NAME_MAX_CHARS, planner, utilities};

/* ────────────────────────────── What comes out ───────────────────────────── */

/// One occurrence of a subscribed event, already cut to a single day.
///
/// A span is cut at midnight on the way in rather than at every draw: the
/// calendar asks "what is on this day" once per cell, on an uncapped render
/// loop (§14.1), and the planner asks it again for every reflow. Neither
/// should have to know that an event can run past midnight. `start` and `end`
/// are minutes from midnight on `day`; `end` may be `planner::DAY_MINUTES` and
/// is never below `start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occurrence {
    pub day: NaiveDate,
    pub start: i32,
    pub end: i32,
    /// A `VALUE=DATE` event: no clock. Drawn as a band and **never** a reflow
    /// anchor — "we are away this week" is a fact about the week, not a reason
    /// to refuse to plan Tuesday.
    pub all_day: bool,
    /// `TRANSP:TRANSPARENT`: the owner said this does not occupy them. Drawn,
    /// never anchored. This app's own feed writes it on a due marker, and a
    /// household is quite likely to subscribe TaskDeck to a calendar TaskDeck
    /// feeds — reading our own politeness back as somebody's meeting would
    /// wall off the day with our own due dates.
    pub free: bool,
    pub name: String,
    /// `LOCATION` — where it is. Its own field in the file, and worth its own
    /// line on screen: a university feed writes a room here in a dozen
    /// characters while the summary runs to a hundred, so this is the part
    /// that survives being read at a glance.
    #[serde(default)]
    pub location: String,
    /// `DESCRIPTION`, bounded. Too long to draw in a calendar block, so it is
    /// what a hover or a long press says rather than something painted.
    #[serde(default)]
    pub description: String,
}

impl Occurrence {
    /// Whether this occurrence is a thing the day has to be planned around.
    ///
    /// An all-day band and a `TRANSPARENT` marker are shown and not obeyed;
    /// everything else is time somebody else has already taken.
    pub fn is_busy(&self) -> bool {
        !self.all_day && !self.free && self.end > self.start
    }
}

/// What one fetched file amounted to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    /// Every occurrence inside the window, sorted by `(day, start, end, name)`.
    /// Sorted so that two fetches of a calendar that did not change compare
    /// equal whatever order the file listed its events in — which is the whole
    /// of the rule that an unchanged refresh must not move the board version.
    pub occurrences: Vec<Occurrence>,
    /// `X-WR-CALNAME`, when the file gives one: what a new subscription is
    /// called before anyone names it.
    pub name: Option<String>,
    /// What was skipped, and why — **one line per reason, with a count**, never
    /// one per event. A calendar of three thousand things this parser does not
    /// read must produce one line, not three thousand.
    pub problems: Vec<String>,
}

/// The bounds a fetched file is read inside.
///
/// Not configuration. These are the line between "a calendar" and "a way to
/// spend this thread's afternoon", and nothing on the far end of a URL gets to
/// move them. Every one is a count the walk spends down, so the parser's cost
/// is a function of these numbers and the window, never of what the file
/// claims about itself.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Deepest `BEGIN:`/`END:` nesting read before the file is refused whole.
    /// A real calendar is three deep: VCALENDAR > VTIMEZONE > STANDARD.
    pub max_depth: usize,
    /// Most unfolded content lines read.
    pub max_lines: usize,
    /// Most `VEVENT`s read from one file.
    pub max_events: usize,
    /// Most occurrences one event may put in the window.
    pub max_occurrences_per_event: usize,
    /// Most occurrences one file may put in the window, everything together.
    pub max_occurrences: usize,
    /// Most candidate periods a rule is walked before the event is given up
    /// on. A `FREQ=DAILY` rule anchored in 1970 is twenty thousand steps from
    /// today; one anchored by a broken exporter is unbounded, and this is
    /// where that stops.
    pub max_rule_steps: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 8,
            max_lines: 200_000,
            max_events: 20_000,
            max_occurrences_per_event: 1_000,
            max_occurrences: 50_000,
            max_rule_steps: 100_000,
        }
    }
}

/// Reasons things were skipped, each counted once however often it happened.
#[derive(Debug, Default)]
struct Problems {
    counts: Vec<(String, usize)>,
}

impl Problems {
    fn note(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        match self.counts.iter_mut().find(|(seen, _)| *seen == reason) {
            Some((_, count)) => *count += 1,
            None => self.counts.push((reason, 1)),
        }
    }

    fn lines(self) -> Vec<String> {
        self.counts
            .into_iter()
            .map(|(reason, count)| if count == 1 { reason } else { format!("{reason} ({count} times)") })
            .collect()
    }
}

/* ───────────────────────────── The one entry point ───────────────────────── */

/// Read one fetched iCalendar file, keeping what falls in `[from, until)`.
///
/// The window is days in local time, and it is deliberately not the calendar's
/// display range: the wall calendar shows up to ten years, and expanding a
/// daily rule across ten years for every subscription is hundreds of thousands
/// of occurrences to hold, to compare on every refresh, and to hand to a
/// client. Past the window the overlay is simply empty.
///
/// `Err` only for a file that is structurally not a calendar. Anything else —
/// an event with no `DTSTART`, a rule this parser does not read, a `TZID` the
/// file never defined — is a skipped event and a counted line in
/// `Parsed::problems`, because one bad `VEVENT` must not cost the other four
/// hundred.
pub fn parse(text: &str, from: NaiveDate, until: NaiveDate, limits: Limits) -> Result<Parsed, String> {
    let lines = unfold(text, limits.max_lines)?;
    let lines: Vec<Line> = lines.iter().filter_map(|line| split_line(line)).collect();
    let harvest = harvest(lines, &limits)?;

    let mut problems = Problems::default();
    let zones = Zones::build(harvest.zones, from.year().saturating_sub(1), until.year().saturating_add(1), &limits);

    let raw: Vec<RawEvent> = harvest.events.iter().map(|lines| read_event(lines)).collect();
    let (series, overrides) = apply_overrides(raw);

    let mut occurrences: Vec<Occurrence> = Vec::new();
    for event in &series {
        if occurrences.len() >= limits.max_occurrences {
            problems.note("more events than can be shown at once; the rest of the file was not read");
            break;
        }
        let room = limits.max_occurrences.saturating_sub(occurrences.len());
        occurrences.extend(
            occurrences_of(event, &overrides, &zones, from, until, &limits, &mut problems).into_iter().take(room),
        );
    }

    // Sorted so a calendar that did not change parses equal however it was
    // listed. This is what makes `Parsed == Parsed` the answer to "did this
    // change", which is what keeps an unchanged refresh from waking every
    // phone and every client.
    occurrences.sort_by(|a, b| {
        (a.day, a.start, a.end, &a.name).cmp(&(b.day, b.start, b.end, &b.name))
    });

    Ok(Parsed { occurrences, name: harvest.calendar_name, problems: problems.lines() })
}

/* ──────────────────────────────── The lexer ──────────────────────────────── */

/// One unfolded content line, split where RFC 5545 says to split it.
///
/// Owned rather than borrowed: the body is capped long before it reaches here,
/// so the copy is bounded, and a lifetime threaded through every stage below
/// would buy nothing but noise.
#[derive(Debug, Clone)]
struct Line {
    /// Upper-cased, so `dtstart` and `DtStart` are the same property.
    name: String,
    /// Parameter names upper-cased, values with their quotes taken off.
    params: Vec<(String, String)>,
    /// Still escaped. Whether `\n` is a line break depends on whether the
    /// property is TEXT, and only the reader of a given property knows that.
    value: String,
}

/// Unfold `text` into logical content lines.
///
/// RFC 5545 §3.1 breaks a long line and starts the next with one space or tab;
/// the break *and* that one whitespace belong to the fold, not to the value.
/// The RFC says CRLF and a good half of the files in the wild use bare LF, so
/// both are accepted and a stray CR at the end of a line is dropped rather
/// than living on inside a value nothing can then match. A UTF-8 byte order
/// mark at the very start is dropped too: Outlook writes one, and
/// `\u{feff}BEGIN` is not `BEGIN`.
///
/// Refuses past `max_lines` rather than growing to fit.
fn unfold(text: &str, max_lines: usize) -> Result<Vec<String>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out: Vec<String> = Vec::new();

    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        // A continuation carries exactly one space or tab of fold, and the
        // rest of the line is value — including any further spaces, which are
        // somebody's indentation inside a DESCRIPTION and not ours to trim.
        let folded = raw.strip_prefix(' ').or_else(|| raw.strip_prefix('\t'));
        match folded {
            Some(rest) => match out.last_mut() {
                Some(line) => line.push_str(rest),
                // A fold with nothing to fold into: a file that begins with a
                // space. Not worth refusing a calendar over.
                None => continue,
            },
            None => {
                if raw.is_empty() {
                    continue;
                }
                if out.len() >= max_lines {
                    return Err(format!("that calendar is longer than {max_lines} lines"));
                }
                out.push(raw.to_string());
            }
        }
    }
    Ok(out)
}

/// Split one content line into `NAME;PARAM=value:VALUE`.
///
/// The value begins at the first colon **outside** double quotes. Apple writes
/// `DTSTART;TZID="Europe/Helsinki":19980118T230000`, and a quoted parameter is
/// allowed to hold the colon that would otherwise have ended the name —
/// splitting on the first colon puts the meeting in a zone called `"Europe`
/// and the rest of the line in the value.
///
/// `None` for a line with no colon at all: a line this parser has nothing to
/// do with, not a reason to refuse the file.
fn split_line(line: &str) -> Option<Line> {
    let mut quoted = false;
    let mut split_at = None;
    for (index, ch) in line.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            ':' if !quoted => {
                split_at = Some(index);
                break;
            }
            _ => {}
        }
    }
    let at = split_at?;
    let head = line.get(..at)?;
    let value = line.get(at + 1..).unwrap_or_default().to_string();

    // The name runs to the first semicolon outside quotes; the parameters are
    // what follows, split the same way.
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in head.chars() {
        match ch {
            '"' => quoted = !quoted,
            ';' if !quoted => parts.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    parts.push(current);

    let mut parts = parts.into_iter();
    let name = parts.next().unwrap_or_default().trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    let params = parts
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            let value = value.trim();
            let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value);
            Some((key.trim().to_ascii_uppercase(), value.to_string()))
        })
        .collect();

    Some(Line { name, params, value })
}

/// Reverse RFC 5545 §3.3.11 escaping in a TEXT value — the inverse of
/// `phone::escape_text`.
///
/// An unknown escape is kept exactly as written: a name is a label on a
/// calendar cell, and a stray backslash on it is better than a mangled word.
/// A trailing lone backslash is kept for the same reason.
fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(';') => out.push(';'),
            Some(',') => out.push(','),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// A `SUMMARY` as a name this app can paint.
///
/// Unescaped, then flattened and bounded like every other name that reaches
/// the board. The app is set in Fixedsys, which has no tab glyph and whose
/// missing-glyph box is what `utilities::detab` exists to prevent; an imported
/// summary can carry both tabs and — once `\n` is unescaped — real line
/// breaks, and a calendar cell is one line. Bounded at `NAME_MAX_CHARS`
/// because that is what a name is here, and a subscribed event's name is not
/// exempt from it.
/// A free-text field, flattened and bounded like a name but to its own length.
fn bounded_text(value: &str, max: usize) -> String {
    let text = utilities::detab(&unescape_text(value));
    let flat = text.replace(['\n', '\r'], " ");
    // Runs of space are what a folded, unescaped description turns into.
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(max).collect::<String>().trim().to_string()
}

fn imported_name(value: &str) -> String {
    let text = utilities::detab(&unescape_text(value));
    let flat = text.replace(['\n', '\r'], " ");
    // By characters, never by bytes: a name cut in the middle of a codepoint
    // is a panic, and this one came off the network.
    flat.trim().chars().take(NAME_MAX_CHARS).collect::<String>().trim().to_string()
}

/* ─────────────────────────── The component walk ──────────────────────────── */

/// One `STANDARD` or `DAYLIGHT` block of a `VTIMEZONE`.
#[derive(Debug, Clone)]
struct Observance {
    /// Its `DTSTART`: a floating wall time, in the offset that was in force
    /// just before it — which is `from`, not `to`.
    start: NaiveDateTime,
    from: TimeDelta,
    to: TimeDelta,
    rule: Option<Recurrence>,
}

/// A `VTIMEZONE`, as the file defined it.
#[derive(Debug, Clone)]
struct ZoneDefinition {
    tzid: String,
    observances: Vec<Observance>,
}

/// The `BEGIN:`/`END:` tree, flattened to the three things TaskDeck reads.
#[derive(Debug, Default)]
struct Harvest {
    events: Vec<Vec<Line>>,
    zones: Vec<ZoneDefinition>,
    calendar_name: Option<String>,
}

/// Walk the components with a `Vec` for a stack rather than with recursion.
///
/// A recursive walk is the obvious way to write this and exactly the reason
/// not to. Nothing stops a file from being a hundred thousand `BEGIN:X` lines,
/// and a recursive descent meets that with a stack overflow — which
/// `panic = "abort"` cannot catch, which no care in the caller can prevent,
/// and which is the one failure this module exists not to have. It is also
/// what decided against the obvious crate: `icalendar`'s component parser
/// recurses with no depth of any kind.
///
/// An `END:` naming something other than the innermost open component closes
/// the innermost one anyway: mismatched nesting is a broken exporter, not an
/// attack, and losing the rest of a calendar over it is the wrong trade.
fn harvest(lines: Vec<Line>, limits: &Limits) -> Result<Harvest, String> {
    let mut out = Harvest::default();
    let mut stack: Vec<String> = Vec::new();
    let mut saw_calendar = false;

    // Whatever component is being filled at the moment, if it is one we keep.
    let mut event: Option<Vec<Line>> = None;
    let mut zone: Option<ZoneDefinition> = None;
    let mut observance: Option<Vec<Line>> = None;

    for line in lines {
        match line.name.as_str() {
            "BEGIN" => {
                let kind = line.value.trim().to_ascii_uppercase();
                if stack.len() >= limits.max_depth {
                    return Err(format!("that calendar nests more than {} components deep", limits.max_depth));
                }
                match kind.as_str() {
                    "VCALENDAR" => saw_calendar = true,
                    "VEVENT" => {
                        if out.events.len() >= limits.max_events {
                            return Err(format!("that calendar holds more than {} events", limits.max_events));
                        }
                        event = Some(Vec::new());
                    }
                    "VTIMEZONE" => zone = Some(ZoneDefinition { tzid: String::new(), observances: Vec::new() }),
                    "STANDARD" | "DAYLIGHT" => observance = Some(Vec::new()),
                    _ => {}
                }
                stack.push(kind);
            }
            "END" => {
                let kind = stack.pop().unwrap_or_default();
                match kind.as_str() {
                    "VEVENT" => {
                        if let Some(lines) = event.take() {
                            out.events.push(lines);
                        }
                    }
                    "VTIMEZONE" => {
                        if let Some(zone) = zone.take()
                            && !zone.tzid.is_empty()
                        {
                            out.zones.push(zone);
                        }
                    }
                    "STANDARD" | "DAYLIGHT" => {
                        if let Some(lines) = observance.take()
                            && let Some(zone) = zone.as_mut()
                            && let Some(read) = read_observance(&lines)
                        {
                            zone.observances.push(read);
                        }
                    }
                    _ => {}
                }
            }
            "CALSCALE" => {
                let scale = line.value.trim().to_ascii_uppercase();
                if !scale.is_empty() && scale != "GREGORIAN" {
                    return Err(format!("that calendar is written in the {scale} calendar, which this cannot read"));
                }
            }
            _ => {
                if let Some(lines) = observance.as_mut() {
                    lines.push(line);
                } else if let Some(lines) = event.as_mut() {
                    lines.push(line);
                } else if let Some(zone) = zone.as_mut() {
                    if line.name == "TZID" {
                        zone.tzid = line.value.trim().to_string();
                    }
                } else if line.name == "X-WR-CALNAME" && out.calendar_name.is_none() {
                    let name = imported_name(&line.value);
                    if !name.is_empty() {
                        out.calendar_name = Some(name);
                    }
                }
            }
        }
    }

    if !saw_calendar {
        return Err("that address did not answer with a calendar".to_string());
    }
    Ok(out)
}

fn read_observance(lines: &[Line]) -> Option<Observance> {
    let mut start = None;
    let mut from = None;
    let mut to = None;
    let mut rule = None;
    for line in lines {
        match line.name.as_str() {
            "DTSTART" => {
                start = match read_time(&line.value, &line.params) {
                    Some(IcsTime::Floating(at)) => Some(at),
                    Some(IcsTime::Utc(at)) => Some(at.naive_utc()),
                    Some(IcsTime::Zoned { at, .. }) => Some(at),
                    Some(IcsTime::Date(day)) => day.and_hms_opt(0, 0, 0),
                    None => None,
                }
            }
            "TZOFFSETFROM" => from = read_utc_offset(&line.value),
            "TZOFFSETTO" => to = read_utc_offset(&line.value),
            "RRULE" => rule = read_recurrence(&line.value),
            _ => {}
        }
    }
    Some(Observance { start: start?, from: from?, to: to?, rule })
}

/* ─────────────────────────────── Value readers ───────────────────────────── */

/// A `DATE-TIME` or `DATE` as the file wrote it, before a zone is chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
enum IcsTime {
    /// `19980118` — a whole day, no clock.
    Date(NaiveDate),
    /// `19980118T230000Z` — an instant, and the only unambiguous form.
    Utc(DateTime<Utc>),
    /// `19980118T230000` with no `TZID`: whatever the wall clock says wherever
    /// the calendar is read, which for TaskDeck is here.
    Floating(NaiveDateTime),
    /// `TZID=Europe/Helsinki:19980118T230000` — a wall time in a named zone.
    Zoned { at: NaiveDateTime, tzid: String },
}

/// Read a `DTSTART`/`DTEND`/`EXDATE`/`RDATE`/`RECURRENCE-ID` value.
///
/// Strict about shape on purpose: eight digits, or eight, `T`, six, and an
/// optional `Z`. A value this parser half-understood would put a meeting at
/// the wrong hour, and a meeting at the wrong hour is worse than one the
/// overlay does not show — the overlay's whole job is to be believed.
fn read_time(value: &str, params: &[(String, String)]) -> Option<IcsTime> {
    let value = value.trim();
    let is_date = params
        .iter()
        .any(|(name, value)| name == "VALUE" && value.eq_ignore_ascii_case("DATE"));
    let tzid = params.iter().find(|(name, _)| name == "TZID").map(|(_, value)| value.clone());

    if is_date || value.len() == 8 {
        return NaiveDate::parse_from_str(value, "%Y%m%d").ok().map(IcsTime::Date);
    }
    if let Some(body) = value.strip_suffix('Z') {
        let naive = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
        return Some(IcsTime::Utc(Utc.from_utc_datetime(&naive)));
    }
    let naive = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
    Some(match tzid {
        Some(tzid) if !tzid.is_empty() => IcsTime::Zoned { at: naive, tzid },
        _ => IcsTime::Floating(naive),
    })
}

/// A comma-separated list of them, as `EXDATE` and `RDATE` are allowed to be.
fn read_times(line: &Line) -> Vec<IcsTime> {
    line.value.split(',').filter_map(|part| read_time(part, &line.params)).collect()
}

/// Read an RFC 5545 §3.3.6 `DURATION` into minutes.
///
/// `P2DT3H30M`, `PT45M`, `P1W`. Seconds round **up** to the minute the planner
/// works in, so a ninety-second event still takes a minute of the day rather
/// than none. A negative duration is refused: legal on a `TRIGGER`, meaningless
/// as an event's length. Every step is checked, so a file full of nines is a
/// `None` here rather than an overflow — and note the release profile leaves
/// overflow checks off, so an unchecked one would have wrapped in silence.
fn read_duration_minutes(value: &str) -> Option<i64> {
    let value = value.trim();
    let body = value.strip_prefix('P').or_else(|| value.strip_prefix("+P"))?;
    if value.starts_with('-') {
        return None;
    }
    let (date_part, time_part) = match body.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => (body, None),
    };

    let mut minutes: i64 = 0;
    let mut digits = String::new();
    for ch in date_part.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        let count: i64 = digits.parse().ok()?;
        digits.clear();
        let per = match ch {
            'W' => 7 * 24 * 60,
            'D' => 24 * 60,
            _ => return None,
        };
        minutes = minutes.checked_add(count.checked_mul(per)?)?;
    }
    if !digits.is_empty() {
        return None;
    }

    if let Some(time_part) = time_part {
        let mut seconds: i64 = 0;
        for ch in time_part.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
                continue;
            }
            let count: i64 = digits.parse().ok()?;
            digits.clear();
            match ch {
                'H' => minutes = minutes.checked_add(count.checked_mul(60)?)?,
                'M' => minutes = minutes.checked_add(count)?,
                'S' => seconds = seconds.checked_add(count)?,
                _ => return None,
            }
        }
        if !digits.is_empty() {
            return None;
        }
        // Rounded up: a ninety-second event takes a minute, not none.
        minutes = minutes.checked_add(seconds.checked_add(59)? / 60)?;
    }
    Some(minutes)
}

/// `+0200` / `-0530` / `+020000`, as `TZOFFSETFROM` and `TZOFFSETTO` write it.
fn read_utc_offset(value: &str) -> Option<TimeDelta> {
    let value = value.trim();
    let (sign, body) = match value.strip_prefix('-') {
        Some(body) => (-1i64, body),
        None => (1i64, value.strip_prefix('+').unwrap_or(value)),
    };
    if body.len() != 4 && body.len() != 6 {
        return None;
    }
    if !body.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = body.get(..2)?.parse().ok()?;
    let minutes: i64 = body.get(2..4)?.parse().ok()?;
    let seconds: i64 = match body.get(4..6) {
        Some(text) => text.parse().ok()?,
        None => 0,
    };
    let total = hours.checked_mul(3600)?.checked_add(minutes.checked_mul(60)?)?.checked_add(seconds)?;
    TimeDelta::try_seconds(sign.checked_mul(total)?)
}

/* ─────────────────────────────── Time zones ──────────────────────────────── */

/// Every zone the file defined, and the transitions they work out to over the
/// years the window touches.
///
/// TaskDeck resolves a `TZID` from the file's own definition rather than from
/// a copy of the IANA database, for two reasons and not one. The database is
/// seven megabytes of generated tables for a wall calendar that needs one
/// household's zone — and it would not answer the question anyway, because
/// Outlook's `TZID`s are names like `W. Europe Standard Time` and are not in
/// it. RFC 5545 §3.6.5 requires a file that uses a `TZID` to define it, so the
/// answer is already in the file.
#[derive(Debug, Default)]
pub struct Zones {
    /// Per zone, the wall time each offset takes effect at, ascending.
    transitions: HashMap<String, Vec<(NaiveDateTime, TimeDelta)>>,
    /// The offset in force before any of them.
    earliest: HashMap<String, TimeDelta>,
}

impl Zones {
    fn build(definitions: Vec<ZoneDefinition>, first_year: i32, last_year: i32, limits: &Limits) -> Self {
        let mut zones = Zones::default();
        let from = NaiveDate::from_ymd_opt(first_year.clamp(1, 9999), 1, 1);
        let until = NaiveDate::from_ymd_opt(last_year.clamp(1, 9999), 12, 31);
        let (Some(from), Some(until)) = (from, until) else { return zones };

        for definition in definitions {
            let mut points: Vec<(NaiveDateTime, TimeDelta)> = Vec::new();
            let mut earliest: Option<(NaiveDateTime, TimeDelta)> = None;

            for observance in &definition.observances {
                match &earliest {
                    Some((at, _)) if *at <= observance.start => {}
                    _ => earliest = Some((observance.start, observance.from)),
                }
                points.push((observance.start, observance.to));

                let Some(rule) = &observance.rule else { continue };
                let expansion = expand(rule, observance.start.date(), from, until, limits);
                for day in expansion.days {
                    points.push((day.and_time(observance.start.time()), observance.to));
                }
            }

            points.sort_by_key(|(at, _)| *at);
            points.dedup_by_key(|(at, _)| *at);
            if let Some((_, offset)) = earliest {
                zones.earliest.insert(definition.tzid.clone(), offset);
            }
            zones.transitions.insert(definition.tzid, points);
        }
        zones
    }

    /// The instant a wall time in `tzid` names, as this machine's clock reads
    /// it.
    ///
    /// `None` when the file used a `TZID` it never defined — the one case this
    /// cannot answer. The caller then reads the wall time as local and says so
    /// in `problems`, because a meeting an hour out is much easier to live with
    /// when the calendar admits which meeting it is.
    fn resolve(&self, tzid: &str, at: NaiveDateTime) -> Option<DateTime<Local>> {
        let points = self.transitions.get(tzid)?;
        let offset = points
            .iter()
            .rev()
            .find(|(when, _)| *when <= at)
            .map(|(_, offset)| *offset)
            .or_else(|| self.earliest.get(tzid).copied())?;
        let utc = at.checked_sub_signed(offset)?;
        Some(Utc.from_utc_datetime(&utc).with_timezone(&Local))
    }
}

/* ──────────────────────────────── Recurrence ─────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// The part of `RRULE` (RFC 5545 §3.3.10) TaskDeck reads.
///
/// A restricted grammar, on purpose. Everything here turns up in calendars
/// people actually subscribe to — a weekly standup, a "last Friday of the
/// month", a school's term dates, a national holiday feed. What is left out
/// does not: `BYYEARDAY`, `BYWEEKNO`, `BYHOUR` and below, `EXRULE` (deprecated
/// in RFC 5545 itself), and `FREQ=SECONDLY|MINUTELY|HOURLY`. A rule using one
/// of those is skipped with a counted line rather than half-honoured: an event
/// drawn on the wrong day is worse than an event not drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Recurrence {
    freq: Freq,
    /// At least 1. `INTERVAL=0` is not a rule, it is a loop.
    interval: u32,
    count: Option<u32>,
    until: Option<IcsTime>,
    /// `(None, Mon)` for `MO`; `(Some(-1), Sun)` for `-1SU`.
    by_day: Vec<(Option<i32>, Weekday)>,
    /// Negative counts back from the end of the month, as the RFC says.
    by_month_day: Vec<i32>,
    by_month: Vec<u32>,
    by_set_pos: Vec<i32>,
}

fn read_recurrence(value: &str) -> Option<Recurrence> {
    let mut freq = None;
    let mut rule = Recurrence {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
        by_month: Vec::new(),
        by_set_pos: Vec::new(),
    };

    for part in value.split(';') {
        let Some((key, body)) = part.split_once('=') else { continue };
        let key = key.trim().to_ascii_uppercase();
        let body = body.trim();
        match key.as_str() {
            "FREQ" => {
                freq = match body.to_ascii_uppercase().as_str() {
                    "DAILY" => Some(Freq::Daily),
                    "WEEKLY" => Some(Freq::Weekly),
                    "MONTHLY" => Some(Freq::Monthly),
                    "YEARLY" => Some(Freq::Yearly),
                    // SECONDLY, MINUTELY, HOURLY: not read, and not guessed at.
                    _ => return None,
                }
            }
            "INTERVAL" => rule.interval = body.parse::<u32>().ok().filter(|n| *n > 0)?,
            "COUNT" => rule.count = body.parse::<u32>().ok(),
            "UNTIL" => rule.until = read_time(body, &[]),
            "BYDAY" => {
                for entry in body.split(',') {
                    let entry = entry.trim();
                    if entry.is_empty() {
                        continue;
                    }
                    let split = entry.len().saturating_sub(2);
                    let (nth, day) = (entry.get(..split)?, entry.get(split..)?);
                    let weekday = match day.to_ascii_uppercase().as_str() {
                        "MO" => Weekday::Mon,
                        "TU" => Weekday::Tue,
                        "WE" => Weekday::Wed,
                        "TH" => Weekday::Thu,
                        "FR" => Weekday::Fri,
                        "SA" => Weekday::Sat,
                        "SU" => Weekday::Sun,
                        _ => return None,
                    };
                    let nth = if nth.is_empty() { None } else { Some(nth.parse::<i32>().ok()?) };
                    rule.by_day.push((nth, weekday));
                }
            }
            "BYMONTHDAY" => {
                for entry in body.split(',') {
                    let day = entry.trim().parse::<i32>().ok()?;
                    if day == 0 || !(-31..=31).contains(&day) {
                        return None;
                    }
                    rule.by_month_day.push(day);
                }
            }
            "BYMONTH" => {
                for entry in body.split(',') {
                    let month = entry.trim().parse::<u32>().ok().filter(|m| (1..=12).contains(m))?;
                    rule.by_month.push(month);
                }
            }
            "BYSETPOS" => {
                for entry in body.split(',') {
                    rule.by_set_pos.push(entry.trim().parse::<i32>().ok()?);
                }
            }
            "WKST" => {}
            // A rule part this parser does not read makes the whole rule
            // unreadable rather than partly obeyed.
            "BYYEARDAY" | "BYWEEKNO" | "BYHOUR" | "BYMINUTE" | "BYSECOND" => return None,
            _ => {}
        }
    }

    rule.freq = freq?;
    Some(rule)
}

struct Expansion {
    days: Vec<NaiveDate>,
    exhausted_budget: bool,
}

/// The days a rule falls on inside `[from, until]`, walked forward from
/// `start`.
///
/// Walked rather than solved. The window is a few hundred days wide, so the
/// walk is a few thousand date additions on a background thread — nothing —
/// and it cannot be talked into a wrong answer the way a closed-form jump can.
/// `max_rule_steps` bounds it regardless: a rule whose `DTSTART` is in 1601
/// stops being expanded and says so rather than becoming the fetch thread's
/// afternoon. `COUNT` is counted from `start`, including occurrences before
/// the window, because that is what `COUNT` means.
///
/// **Every date step is checked.** `board.rs` already carries the scar of
/// chrono panicking past its last representable day, so the walk asks
/// `from_ymd_opt` and `checked_add_signed` and treats `None` as the end of the
/// walk rather than the end of the process.
///
/// Monthly and yearly step by *calendar position*, not by adding months: a
/// rule anchored on the 31st must **skip** February, and adding a month would
/// clamp it to the 28th and put a meeting on a day the rule never named.
fn expand(rule: &Recurrence, start: NaiveDate, from: NaiveDate, until: NaiveDate, limits: &Limits) -> Expansion {
    let mut days: Vec<NaiveDate> = Vec::new();
    let mut produced: u32 = 0;
    let mut steps: usize = 0;
    let interval = rule.interval.max(1) as i64;

    let until_day = match &rule.until {
        Some(IcsTime::Date(day)) => Some(*day),
        Some(IcsTime::Utc(at)) => Some(at.with_timezone(&Local).date_naive()),
        Some(IcsTime::Floating(at)) => Some(at.date()),
        Some(IcsTime::Zoned { at, .. }) => Some(at.date()),
        None => None,
    };
    let last = match until_day {
        Some(day) => day.min(until),
        None => until,
    };

    // Where the walk stands: a day for daily and weekly, a (year, month) for
    // monthly and yearly.
    let mut cursor = start;
    let (mut year, mut month) = (start.year(), start.month());

    loop {
        steps += 1;
        if steps > limits.max_rule_steps {
            return Expansion { days, exhausted_budget: true };
        }
        if let Some(count) = rule.count
            && produced >= count
        {
            break;
        }

        let period: Vec<NaiveDate> = match rule.freq {
            Freq::Daily => vec![cursor],
            Freq::Weekly => week_days(cursor, rule, start),
            Freq::Monthly => month_days(year, month, rule, start),
            Freq::Yearly => year_days(year, rule, start),
        };

        let period = apply_set_pos(period, rule);
        let mut past_the_end = false;
        for day in period {
            if day < start {
                continue;
            }
            if day > last {
                past_the_end = true;
                continue;
            }
            produced = produced.saturating_add(1);
            if let Some(count) = rule.count
                && produced > count
            {
                past_the_end = true;
                break;
            }
            if day >= from {
                days.push(day);
            }
            if days.len() >= limits.max_occurrences_per_event {
                return Expansion { days, exhausted_budget: true };
            }
        }

        // Advance. The period start moving past the window ends the walk;
        // a single period whose days all fell past the end does not, because
        // BYSETPOS can put a later period's days earlier than this one's.
        match rule.freq {
            Freq::Daily => {
                let Some(next) = cursor.checked_add_signed(TimeDelta::try_days(interval).unwrap_or_default()) else {
                    break;
                };
                cursor = next;
                if cursor > last {
                    break;
                }
            }
            Freq::Weekly => {
                let Some(next) = cursor.checked_add_signed(TimeDelta::try_days(interval * 7).unwrap_or_default())
                else {
                    break;
                };
                cursor = next;
                if week_start_of(cursor) > last {
                    break;
                }
            }
            Freq::Monthly => {
                let step = interval.clamp(1, 1200) as u32;
                let total = year as i64 * 12 + (month as i64 - 1) + step as i64;
                year = (total.div_euclid(12)) as i32;
                month = (total.rem_euclid(12) + 1) as u32;
                let Some(first) = NaiveDate::from_ymd_opt(year, month, 1) else { break };
                if first > last {
                    break;
                }
            }
            Freq::Yearly => {
                let Some(next) = year.checked_add(interval.clamp(1, 1000) as i32) else { break };
                year = next;
                let Some(first) = NaiveDate::from_ymd_opt(year, 1, 1) else { break };
                if first > last {
                    break;
                }
            }
        }
        if past_the_end && rule.by_set_pos.is_empty() && matches!(rule.freq, Freq::Daily | Freq::Weekly) {
            break;
        }
    }

    Expansion { days, exhausted_budget: false }
}

fn week_start_of(day: NaiveDate) -> NaiveDate {
    let back = day.weekday().num_days_from_monday() as i64;
    day.checked_sub_signed(TimeDelta::try_days(back).unwrap_or_default()).unwrap_or(day)
}

fn week_days(cursor: NaiveDate, rule: &Recurrence, start: NaiveDate) -> Vec<NaiveDate> {
    let base = week_start_of(cursor);
    let wanted: Vec<Weekday> = if rule.by_day.is_empty() {
        vec![start.weekday()]
    } else {
        rule.by_day.iter().map(|(_, day)| *day).collect()
    };
    let mut out: Vec<NaiveDate> = wanted
        .into_iter()
        .filter_map(|weekday| {
            let forward = weekday.num_days_from_monday() as i64;
            base.checked_add_signed(TimeDelta::try_days(forward)?)
        })
        .filter(|day| rule.by_month.is_empty() || rule.by_month.contains(&day.month()))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn days_in(year: i32, month: u32) -> u32 {
    crate::utilities::days_in_month(year, month)
}

fn month_days(year: i32, month: u32, rule: &Recurrence, start: NaiveDate) -> Vec<NaiveDate> {
    if !rule.by_month.is_empty() && !rule.by_month.contains(&month) {
        return Vec::new();
    }
    let length = days_in(year, month);
    let mut out: Vec<NaiveDate> = Vec::new();

    if !rule.by_month_day.is_empty() {
        for &wanted in &rule.by_month_day {
            // Negative counts back from the end, as the RFC says. Asking
            // `from_ymd_opt` rather than clamping is what makes a rule on the
            // 31st skip February instead of landing on the 28th.
            let day = if wanted > 0 { wanted } else { length as i32 + 1 + wanted };
            if day >= 1
                && let Some(date) = NaiveDate::from_ymd_opt(year, month, day as u32)
            {
                out.push(date);
            }
        }
    } else if !rule.by_day.is_empty() {
        for &(nth, weekday) in &rule.by_day {
            out.extend(nth_weekday(year, month, weekday, nth));
        }
    } else if let Some(date) = NaiveDate::from_ymd_opt(year, month, start.day()) {
        out.push(date);
    }

    out.sort_unstable();
    out.dedup();
    out
}

/// The days of `month` that are `weekday`, or just the `nth` of them.
fn nth_weekday(year: i32, month: u32, weekday: Weekday, nth: Option<i32>) -> Vec<NaiveDate> {
    let length = days_in(year, month);
    let all: Vec<NaiveDate> = (1..=length)
        .filter_map(|day| NaiveDate::from_ymd_opt(year, month, day))
        .filter(|date| date.weekday() == weekday)
        .collect();
    match nth {
        None => all,
        Some(n) if n > 0 => all.get(n as usize - 1).copied().into_iter().collect(),
        Some(n) if n < 0 => {
            let back = n.unsigned_abs() as usize;
            all.len().checked_sub(back).and_then(|index| all.get(index)).copied().into_iter().collect()
        }
        Some(_) => Vec::new(),
    }
}

fn year_days(year: i32, rule: &Recurrence, start: NaiveDate) -> Vec<NaiveDate> {
    let months: Vec<u32> = if rule.by_month.is_empty() { vec![start.month()] } else { rule.by_month.clone() };
    let mut out: Vec<NaiveDate> = Vec::new();
    for month in months {
        if !rule.by_month_day.is_empty() || !rule.by_day.is_empty() {
            let mut inner = rule.clone();
            inner.by_month = Vec::new();
            out.extend(month_days(year, month, &inner, start));
        } else if let Some(date) = NaiveDate::from_ymd_opt(year, month, start.day()) {
            // A yearly rule on the 29th of February recurs only in leap years,
            // which falls out of asking rather than clamping.
            out.push(date);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn apply_set_pos(period: Vec<NaiveDate>, rule: &Recurrence) -> Vec<NaiveDate> {
    if rule.by_set_pos.is_empty() {
        return period;
    }
    let mut out: Vec<NaiveDate> = Vec::new();
    for &position in &rule.by_set_pos {
        let picked = if position > 0 {
            period.get(position as usize - 1)
        } else if position < 0 {
            period.len().checked_sub(position.unsigned_abs() as usize).and_then(|index| period.get(index))
        } else {
            None
        };
        if let Some(day) = picked {
            out.push(*day);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/* ─────────────────────────────── One VEVENT ──────────────────────────────── */

/// One `VEVENT` as read, before the series is worked out.
#[derive(Debug, Clone, Default)]
struct RawEvent {
    uid: Option<String>,
    name: String,
    location: String,
    description: String,
    start: Option<IcsTime>,
    end: Option<IcsTime>,
    duration_minutes: Option<i64>,
    rule: Option<Recurrence>,
    rule_unreadable: bool,
    exdates: Vec<IcsTime>,
    rdates: Vec<IcsTime>,
    /// Present on a single occurrence that was moved or cancelled.
    recurrence_id: Option<IcsTime>,
    cancelled: bool,
    /// `TRANSP:TRANSPARENT` — the owner said this does not occupy them.
    transparent: bool,
}

fn read_event(lines: &[Line]) -> RawEvent {
    let mut event = RawEvent::default();
    for line in lines {
        match line.name.as_str() {
            "UID" => event.uid = Some(line.value.trim().to_string()),
            "SUMMARY" => event.name = imported_name(&line.value),
            // Where it is, which in a course feed is the room — its own field,
            // and short, while the summary runs past a hundred characters.
            "LOCATION" => event.location = bounded_text(&line.value, 120),
            "DESCRIPTION" => event.description = bounded_text(&line.value, 300),
            "DTSTART" => event.start = read_time(&line.value, &line.params),
            "DTEND" => event.end = read_time(&line.value, &line.params),
            "DURATION" => event.duration_minutes = read_duration_minutes(&line.value),
            "RRULE" => match read_recurrence(&line.value) {
                Some(rule) => event.rule = Some(rule),
                None => event.rule_unreadable = true,
            },
            "EXDATE" => event.exdates.extend(read_times(line)),
            "RDATE" => event.rdates.extend(read_times(line)),
            "RECURRENCE-ID" => event.recurrence_id = read_time(&line.value, &line.params),
            "STATUS" => event.cancelled = line.value.trim().eq_ignore_ascii_case("CANCELLED"),
            "TRANSP" => event.transparent = line.value.trim().eq_ignore_ascii_case("TRANSPARENT"),
            _ => {}
        }
    }
    event
}

/// One occurrence that replaces another, named by the instant it replaces.
#[derive(Debug, Clone)]
struct Override {
    uid: String,
    replaces: IcsTime,
    event: RawEvent,
}

/// Take the overrides out of the list of events.
///
/// Google and Outlook do not rewrite a series when one occurrence moves: they
/// emit a second `VEVENT` with the same `UID` and a `RECURRENCE-ID` naming the
/// instant it replaces. Without this the overlay draws the meeting twice, once
/// where it was and once where it went, which is the most visible way this
/// feature could be wrong and happens on the first real calendar anyone
/// subscribes to.
fn apply_overrides(events: Vec<RawEvent>) -> (Vec<RawEvent>, Vec<Override>) {
    let mut series = Vec::new();
    let mut overrides = Vec::new();
    for event in events {
        match (&event.recurrence_id, &event.uid) {
            (Some(replaces), Some(uid)) => {
                overrides.push(Override { uid: uid.clone(), replaces: replaces.clone(), event: event.clone() })
            }
            _ => series.push(event),
        }
    }
    (series, overrides)
}

/// The day and minute a time falls on, read here.
fn local_of(time: &IcsTime, zones: &Zones, problems: &mut Problems) -> Option<(NaiveDate, Option<i32>)> {
    match time {
        IcsTime::Date(day) => Some((*day, None)),
        IcsTime::Utc(at) => {
            let local = at.with_timezone(&Local);
            Some((local.date_naive(), Some(minutes_of(local))))
        }
        IcsTime::Floating(at) => Some((at.date(), Some(at.hour() as i32 * 60 + at.minute() as i32))),
        IcsTime::Zoned { at, tzid } => match zones.resolve(tzid, *at) {
            Some(local) => Some((local.date_naive(), Some(minutes_of(local)))),
            None => {
                problems.note(format!("a time zone the file never defined ({tzid}); read as this computer's"));
                Some((at.date(), Some(at.hour() as i32 * 60 + at.minute() as i32)))
            }
        },
    }
}

fn minutes_of(at: DateTime<Local>) -> i32 {
    at.hour() as i32 * 60 + at.minute() as i32
}

/// A key that identifies one occurrence across the forms a file may write it
/// in: an `EXDATE` may be written `Z` while its `DTSTART` carries a `TZID`.
fn instant_key(time: &IcsTime, zones: &Zones, problems: &mut Problems) -> Option<(NaiveDate, i32)> {
    let (day, minutes) = local_of(time, zones, problems)?;
    Some((day, minutes.unwrap_or(0)))
}

/// One event's occurrences, cut to days and clipped to the window.
///
/// The length is `DTEND - DTSTART`, or `DURATION`, or — when the file gives
/// neither — `planner::SNAP_MINUTES`, which is what the outbound feed already
/// gives a length-less event. One span becomes one `Occurrence` per day it
/// touches, each clamped inside the day; a zero-length span still gets
/// `MIN_BLOCK_MINUTES` so it is something a person can see.
///
/// An all-day event covers each of its days, and RFC 5545's exclusive `DTEND`
/// for a `DATE` value means the last day is **not** included: a one-day event
/// is `DTSTART:20260907`, `DTEND:20260908`, and drawing two days there is the
/// other classic off-by-one in this format.
fn occurrences_of(
    event: &RawEvent,
    overrides: &[Override],
    zones: &Zones,
    from: NaiveDate,
    until: NaiveDate,
    limits: &Limits,
    problems: &mut Problems,
) -> Vec<Occurrence> {
    if event.cancelled {
        return Vec::new();
    }
    if event.rule_unreadable {
        problems.note("a repeat rule this cannot read; that event was left out");
    }
    let Some(start) = &event.start else {
        problems.note("an event with no start; left out");
        return Vec::new();
    };
    let Some((first_day, start_minutes)) = local_of(start, zones, problems) else {
        problems.note("an event whose start could not be read; left out");
        return Vec::new();
    };
    let all_day = start_minutes.is_none();

    // How long it runs, in minutes for a timed event and in days for an
    // all-day one.
    let length_minutes = span_minutes(event, zones, problems, all_day);

    // Which days the series begins on.
    let mut starts: Vec<NaiveDate> = Vec::new();
    match &event.rule {
        Some(rule) => {
            let expansion = expand(rule, first_day, from, until, limits);
            if expansion.exhausted_budget {
                problems.note("a repeat that goes back further than can be worked out; shown only in part");
            }
            starts.extend(expansion.days);
        }
        None => {
            if first_day >= from && first_day <= until {
                starts.push(first_day);
            }
        }
    }
    for rdate in &event.rdates {
        if let Some((day, _)) = local_of(rdate, zones, problems)
            && day >= from
            && day <= until
        {
            starts.push(day);
        }
    }
    starts.sort_unstable();
    starts.dedup();

    // Which of them were taken out again.
    let excluded: Vec<(NaiveDate, i32)> =
        event.exdates.iter().filter_map(|time| instant_key(time, zones, problems)).collect();
    let uid = event.uid.clone().unwrap_or_default();
    let moved: Vec<&Override> = overrides.iter().filter(|over| over.uid == uid && !uid.is_empty()).collect();

    let mut out: Vec<Occurrence> = Vec::new();
    for day in starts {
        let minutes = start_minutes.unwrap_or(0);
        if excluded.iter().any(|(exday, exmin)| *exday == day && (all_day || *exmin == minutes)) {
            continue;
        }
        // An occurrence that was moved or cancelled is drawn where the
        // override put it, not where the rule did.
        if let Some(over) = moved.iter().find(|over| {
            instant_key(&over.replaces, zones, problems).is_some_and(|(rday, rmin)| {
                rday == day && (all_day || rmin == minutes)
            })
        }) {
            if over.event.cancelled {
                continue;
            }
            out.extend(occurrences_of(&over.event, &[], zones, from, until, limits, problems));
            continue;
        }
        out.extend(spread(day, minutes, length_minutes, all_day, event, from, until));
        if out.len() >= limits.max_occurrences_per_event {
            break;
        }
    }
    out
}

/// How long one occurrence runs: minutes for a timed event, whole days for an
/// all-day one.
fn span_minutes(event: &RawEvent, zones: &Zones, problems: &mut Problems, all_day: bool) -> i64 {
    if let Some(minutes) = event.duration_minutes {
        return minutes.max(0);
    }
    let (Some(start), Some(end)) = (&event.start, &event.end) else {
        // What the outbound feed gives a length-less event.
        return if all_day { planner::DAY_MINUTES as i64 } else { planner::SNAP_MINUTES as i64 };
    };
    let (Some((start_day, start_min)), Some((end_day, end_min))) =
        (local_of(start, zones, problems), local_of(end, zones, problems))
    else {
        return planner::SNAP_MINUTES as i64;
    };
    let days = (end_day - start_day).num_days();
    let minutes = days.saturating_mul(planner::DAY_MINUTES as i64)
        + (end_min.unwrap_or(0) - start_min.unwrap_or(0)) as i64;
    minutes.max(0)
}

/// Cut one span into one occurrence per day it touches.
#[allow(clippy::too_many_arguments)]
fn spread(
    day: NaiveDate,
    start_minutes: i32,
    length_minutes: i64,
    all_day: bool,
    event: &RawEvent,
    from: NaiveDate,
    until: NaiveDate,
) -> Vec<Occurrence> {
    let mut out = Vec::new();
    let day_minutes = planner::DAY_MINUTES as i64;
    // An all-day DTEND is exclusive, so a length of exactly one day covers one
    // day. A zero-length timed event still gets a minimum so it can be seen.
    let length = if all_day {
        length_minutes.max(day_minutes)
    } else {
        length_minutes.max(planner::MIN_BLOCK_MINUTES as i64)
    };

    let mut remaining = length;
    let mut cursor = day;
    let mut offset = start_minutes as i64;
    // Bounded by the window: a span longer than the window stops at its edge
    // rather than running to the end of the calendar.
    let mut guard = 0;
    while remaining > 0 && guard < 400 {
        guard += 1;
        let end = (offset + remaining).min(day_minutes);
        if cursor > until {
            break;
        }
        if cursor >= from {
            out.push(Occurrence {
                day: cursor,
                start: offset.clamp(0, day_minutes) as i32,
                end: end.clamp(0, day_minutes) as i32,
                all_day,
                free: event.transparent,
                name: event.name.clone(),
                location: event.location.clone(),
                description: event.description.clone(),
            });
        }
        remaining -= end - offset;
        offset = 0;
        let Some(next) = cursor.checked_add_signed(TimeDelta::try_days(1).unwrap_or_default()) else { break };
        cursor = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("a real date")
    }

    fn wrap(body: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{body}\r\nEND:VCALENDAR\r\n")
    }

    fn read(body: &str) -> Parsed {
        parse(&wrap(body), day(2026, 1, 1), day(2027, 1, 1), Limits::default()).expect("a calendar")
    }

    fn event(body: &str) -> String {
        format!("BEGIN:VEVENT\r\nUID:x@test\r\n{body}\r\nEND:VEVENT")
    }

    #[test]
    fn a_folded_line_is_one_property_again_whether_it_broke_on_crlf_or_lf() {
        let crlf = unfold("SUMMARY:one\r\n  two\r\n", 100).expect("unfolds");
        let lf = unfold("SUMMARY:one\n  two\n", 100).expect("unfolds");
        // One space of the fold belongs to the fold; the second is the value's.
        assert_eq!(crlf, vec!["SUMMARY:one two"]);
        assert_eq!(crlf, lf);
        assert!(unfold("A:1\nB:2\nC:3\n", 2).is_err(), "a longer file than allowed is refused");
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_begin_line() {
        // Outlook writes one, and `\u{feff}BEGIN` is not `BEGIN`.
        let text = format!("\u{feff}{}", wrap(&event("SUMMARY:Standup\r\nDTSTART:20260910T090000Z")));
        let parsed = parse(&text, day(2026, 1, 1), day(2027, 1, 1), Limits::default()).expect("a calendar");
        assert_eq!(parsed.occurrences.len(), 1);
    }

    #[test]
    fn a_colon_inside_a_quoted_parameter_does_not_end_the_property_name() {
        let line = split_line("DTSTART;TZID=\"Europe/Helsinki\":19980118T230000").expect("splits");
        assert_eq!(line.name, "DTSTART");
        assert_eq!(line.params, vec![("TZID".to_string(), "Europe/Helsinki".to_string())]);
        assert_eq!(line.value, "19980118T230000");
        assert!(split_line("no colon here").is_none());
    }

    #[test]
    fn a_summary_comes_back_the_way_our_own_feed_would_have_escaped_it() {
        // The exact inverse of `phone::escape_text`.
        assert_eq!(unescape_text(r"a\;b\,c\\d\ne"), "a;b,c\\d\ne");
        // An escape this does not know is kept as written rather than eaten.
        assert_eq!(unescape_text(r"50\% and a trailing \"), r"50\% and a trailing \");
    }

    #[test]
    fn an_imported_name_is_detabbed_flattened_and_bounded_like_every_other_name() {
        assert_eq!(imported_name("a\tb"), "a    b");
        assert_eq!(imported_name(r"one\ntwo"), "one two");
        let long = "x".repeat(NAME_MAX_CHARS + 50);
        assert_eq!(imported_name(&long).chars().count(), NAME_MAX_CHARS);
        // Cut on a character, never on a byte.
        let wide = "日".repeat(NAME_MAX_CHARS + 10);
        assert_eq!(imported_name(&wide).chars().count(), NAME_MAX_CHARS);
    }

    #[test]
    fn a_file_of_nothing_but_begin_lines_is_refused_rather_than_recursed_into() {
        // A recursive parser meets this with a stack overflow, which
        // `panic = "abort"` cannot catch. This is the reason for the shape of
        // `harvest`, and the reason the obvious crate was not taken.
        let body = "BEGIN:X\r\n".repeat(50_000);
        let text = format!("BEGIN:VCALENDAR\r\n{body}");
        let error = parse(&text, day(2026, 1, 1), day(2027, 1, 1), Limits::default()).unwrap_err();
        assert!(error.contains("deep"), "{error}");
    }

    #[test]
    fn a_file_that_is_not_a_calendar_at_all_is_refused_and_one_bad_event_is_only_skipped() {
        assert!(parse("<html>404</html>", day(2026, 1, 1), day(2027, 1, 1), Limits::default()).is_err());
        // But one unreadable event costs only itself.
        let parsed = read(&format!(
            "{}\r\n{}",
            event("SUMMARY:No start at all"),
            event("SUMMARY:Fine\r\nDTSTART:20260910T090000Z")
        ));
        assert_eq!(parsed.occurrences.len(), 1);
        assert_eq!(parsed.occurrences.first().map(|o| o.name.as_str()), Some("Fine"));
        assert!(parsed.problems.iter().any(|p| p.contains("no start")), "{:?}", parsed.problems);
    }

    #[test]
    fn a_value_this_parser_half_understands_is_skipped_rather_than_guessed_at() {
        // An hourly rule is not read, and the event does not silently become
        // a one-off drawn at the wrong cadence.
        let parsed = read(&event("SUMMARY:Ping\r\nDTSTART:20260910T090000Z\r\nRRULE:FREQ=HOURLY;INTERVAL=2"));
        assert!(parsed.problems.iter().any(|p| p.contains("repeat rule")), "{:?}", parsed.problems);
    }

    #[test]
    fn an_event_that_runs_past_midnight_is_cut_at_midnight() {
        let parsed = read(&event("SUMMARY:Night shift\r\nDTSTART:20260910T230000\r\nDTEND:20260911T010000"));
        assert_eq!(parsed.occurrences.len(), 2);
        let first = parsed.occurrences.first().expect("a first");
        let second = parsed.occurrences.get(1).expect("a second");
        assert_eq!((first.day, first.start, first.end), (day(2026, 9, 10), 23 * 60, planner::DAY_MINUTES));
        assert_eq!((second.day, second.start, second.end), (day(2026, 9, 11), 0, 60));
    }

    #[test]
    fn an_all_day_event_covers_its_days_exclusive_of_dtend_and_anchors_none_of_them() {
        // RFC 5545's DTEND is exclusive for a DATE value: this is one day.
        let parsed = read(&event("SUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20260907\r\nDTEND;VALUE=DATE:20260908"));
        assert_eq!(parsed.occurrences.len(), 1);
        let only = parsed.occurrences.first().expect("one");
        assert_eq!(only.day, day(2026, 9, 7));
        assert!(only.all_day);
        assert!(!only.is_busy(), "an all-day band is shown, never planned around");
    }

    #[test]
    fn a_duration_stands_in_for_a_missing_dtend_and_a_negative_one_is_refused() {
        assert_eq!(read_duration_minutes("PT45M"), Some(45));
        assert_eq!(read_duration_minutes("P2DT3H30M"), Some(2 * 24 * 60 + 3 * 60 + 30));
        assert_eq!(read_duration_minutes("P1W"), Some(7 * 24 * 60));
        // Seconds round up, so a ninety-second event still takes a minute.
        assert_eq!(read_duration_minutes("PT90S"), Some(2));
        assert_eq!(read_duration_minutes("-PT1H"), None);
        assert_eq!(read_duration_minutes("P999999999999999999999D"), None);

        let parsed = read(&event("SUMMARY:Call\r\nDTSTART:20260910T090000\r\nDURATION:PT45M"));
        let only = parsed.occurrences.first().expect("one");
        assert_eq!((only.start, only.end), (9 * 60, 9 * 60 + 45));
    }

    #[test]
    fn an_event_with_neither_dtend_nor_duration_gets_the_length_our_own_feed_gives_one() {
        let parsed = read(&event("SUMMARY:Marker\r\nDTSTART:20260910T090000"));
        let only = parsed.occurrences.first().expect("one");
        assert_eq!(only.end - only.start, planner::SNAP_MINUTES);
    }

    #[test]
    fn a_weekly_rule_lands_on_every_day_its_byday_names_and_stops_at_until() {
        let parsed = read(&event(
            "SUMMARY:Standup\r\nDTSTART:20260907T090000\r\nDTEND:20260907T091500\r\n\
             RRULE:FREQ=WEEKLY;BYDAY=MO,WE;UNTIL=20260917T000000Z",
        ));
        let days: Vec<NaiveDate> = parsed.occurrences.iter().map(|o| o.day).collect();
        assert_eq!(
            days,
            vec![day(2026, 9, 7), day(2026, 9, 9), day(2026, 9, 14), day(2026, 9, 16)],
            "Mondays and Wednesdays, and nothing past UNTIL"
        );
    }

    #[test]
    fn count_is_counted_from_dtstart_not_from_the_window() {
        // The window opens after the series did: COUNT has already been spent
        // on the occurrences before it, so only the tail shows.
        let text = wrap(&event("SUMMARY:Five\r\nDTSTART:20260101T090000\r\nRRULE:FREQ=DAILY;COUNT=5"));
        let parsed = parse(&text, day(2026, 1, 3), day(2026, 12, 31), Limits::default()).expect("a calendar");
        let days: Vec<NaiveDate> = parsed.occurrences.iter().map(|o| o.day).collect();
        assert_eq!(days, vec![day(2026, 1, 3), day(2026, 1, 4), day(2026, 1, 5)]);
    }

    #[test]
    fn a_monthly_rule_on_the_thirty_first_skips_february_rather_than_clamping_into_it() {
        let parsed = read(&event(
            "SUMMARY:Rent\r\nDTSTART:20260131T090000\r\nRRULE:FREQ=MONTHLY;BYMONTHDAY=31;COUNT=4",
        ));
        let days: Vec<NaiveDate> = parsed.occurrences.iter().map(|o| o.day).collect();
        // January, March, May, July — never the 28th of February.
        assert!(days.contains(&day(2026, 1, 31)));
        assert!(days.contains(&day(2026, 3, 31)));
        assert!(!days.iter().any(|d| d.month() == 2), "{days:?}");
    }

    #[test]
    fn a_last_weekday_of_the_month_rule_lands_on_the_last_one() {
        let parsed = read(&event(
            "SUMMARY:Retro\r\nDTSTART:20260130T160000\r\nRRULE:FREQ=MONTHLY;BYDAY=-1FR;COUNT=3",
        ));
        let days: Vec<NaiveDate> = parsed.occurrences.iter().map(|o| o.day).collect();
        assert_eq!(days, vec![day(2026, 1, 30), day(2026, 2, 27), day(2026, 3, 27)]);
    }

    #[test]
    fn a_yearly_rule_on_the_twenty_ninth_of_february_recurs_only_in_leap_years() {
        let text = wrap(&event("SUMMARY:Leap\r\nDTSTART:20240229T090000\r\nRRULE:FREQ=YEARLY"));
        let parsed = parse(&text, day(2024, 1, 1), day(2029, 12, 31), Limits::default()).expect("a calendar");
        let days: Vec<NaiveDate> = parsed.occurrences.iter().map(|o| o.day).collect();
        assert_eq!(days, vec![day(2024, 2, 29), day(2028, 2, 29)]);
    }

    #[test]
    fn exdate_removes_the_occurrence_it_names() {
        let parsed = read(&event(
            "SUMMARY:Standup\r\nDTSTART:20260907T090000\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEXDATE:20260908T090000",
        ));
        let days: Vec<NaiveDate> = parsed.occurrences.iter().map(|o| o.day).collect();
        assert_eq!(days, vec![day(2026, 9, 7), day(2026, 9, 9)]);
    }

    #[test]
    fn a_moved_occurrence_replaces_the_one_its_recurrence_id_names_rather_than_joining_it() {
        // Without this the meeting is drawn twice: once where it was and once
        // where it went. It happens on the first real calendar anyone adds.
        let body = format!(
            "BEGIN:VEVENT\r\nUID:s@test\r\nSUMMARY:Standup\r\nDTSTART:20260907T090000\r\n\
             RRULE:FREQ=DAILY;COUNT=2\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:s@test\r\nRECURRENCE-ID:20260908T090000\r\nSUMMARY:Standup (late)\r\n\
             DTSTART:20260908T140000\r\nEND:VEVENT"
        );
        let parsed = read(&body);
        let times: Vec<(NaiveDate, i32, &str)> =
            parsed.occurrences.iter().map(|o| (o.day, o.start, o.name.as_str())).collect();
        assert_eq!(
            times,
            vec![(day(2026, 9, 7), 9 * 60, "Standup"), (day(2026, 9, 8), 14 * 60, "Standup (late)")]
        );
    }

    #[test]
    fn a_cancelled_override_removes_its_occurrence_and_leaves_the_series_standing() {
        let body = format!(
            "BEGIN:VEVENT\r\nUID:s@test\r\nSUMMARY:Standup\r\nDTSTART:20260907T090000\r\n\
             RRULE:FREQ=DAILY;COUNT=3\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:s@test\r\nRECURRENCE-ID:20260908T090000\r\nSTATUS:CANCELLED\r\n\
             DTSTART:20260908T090000\r\nEND:VEVENT"
        );
        let days: Vec<NaiveDate> = read(&body).occurrences.iter().map(|o| o.day).collect();
        assert_eq!(days, vec![day(2026, 9, 7), day(2026, 9, 9)]);
    }

    #[test]
    fn a_tzid_the_file_defines_is_read_from_that_definition_and_not_from_this_machine() {
        // A zone two hours ahead of UTC all year. 09:00 there is 07:00 UTC.
        let body = format!(
            "BEGIN:VTIMEZONE\r\nTZID:Test/Fixed\r\nBEGIN:STANDARD\r\nDTSTART:19700101T000000\r\n\
             TZOFFSETFROM:+0200\r\nTZOFFSETTO:+0200\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n{}",
            event("SUMMARY:Abroad\r\nDTSTART;TZID=Test/Fixed:20260910T090000\r\nDURATION:PT1H")
        );
        let parsed = read(&body);
        let only = parsed.occurrences.first().expect("one");
        let expected = Utc
            .with_ymd_and_hms(2026, 9, 10, 7, 0, 0)
            .single()
            .expect("an instant")
            .with_timezone(&Local);
        assert_eq!(only.day, expected.date_naive());
        assert_eq!(only.start, minutes_of(expected));
        assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    }

    #[test]
    fn a_tzid_the_file_never_defined_falls_back_to_local_and_says_which_one_it_was() {
        let parsed = read(&event(
            "SUMMARY:Somewhere\r\nDTSTART;TZID=W. Europe Standard Time:20260910T090000\r\nDURATION:PT1H",
        ));
        let only = parsed.occurrences.first().expect("one");
        assert_eq!(only.start, 9 * 60, "read as this computer's wall time");
        assert!(
            parsed.problems.iter().any(|p| p.contains("W. Europe Standard Time")),
            "the calendar has to admit which meeting it guessed at: {:?}",
            parsed.problems
        );
    }

    #[test]
    fn a_rule_anchored_centuries_back_stops_at_the_step_budget_and_reports_it() {
        let limits = Limits { max_rule_steps: 500, ..Limits::default() };
        let text = wrap(&event("SUMMARY:Ancient\r\nDTSTART:16010101T090000\r\nRRULE:FREQ=DAILY"));
        let parsed = parse(&text, day(2026, 1, 1), day(2026, 12, 31), limits).expect("a calendar");
        assert!(parsed.problems.iter().any(|p| p.contains("further")), "{:?}", parsed.problems);
    }

    #[test]
    fn a_date_at_the_end_of_what_chrono_can_hold_ends_the_walk_instead_of_the_process() {
        let text = wrap(&event("SUMMARY:Forever\r\nDTSTART:99991230T090000\r\nRRULE:FREQ=DAILY"));
        let parsed = parse(&text, day(9999, 1, 1), day(9999, 12, 31), Limits::default()).expect("a calendar");
        assert!(parsed.occurrences.len() <= 2, "the walk ended at the edge of the calendar");
    }

    #[test]
    fn two_fetches_of_a_calendar_that_did_not_change_parse_equal_whatever_order_it_listed() {
        // This is the whole of the rule that an unchanged refresh must not
        // move the board version.
        let one = read(&format!(
            "{}\r\n{}",
            event("SUMMARY:A\r\nDTSTART:20260910T090000"),
            event("SUMMARY:B\r\nDTSTART:20260910T100000")
        ));
        let other = read(&format!(
            "{}\r\n{}",
            event("SUMMARY:B\r\nDTSTART:20260910T100000"),
            event("SUMMARY:A\r\nDTSTART:20260910T090000")
        ));
        assert_eq!(one.occurrences, other.occurrences);
    }

    #[test]
    fn a_thousand_unreadable_events_produce_one_line_of_problems_not_a_thousand() {
        let body = (0..1000).map(|_| event("SUMMARY:Nothing")).collect::<Vec<_>>().join("\r\n");
        let parsed = read(&body);
        assert_eq!(parsed.problems.len(), 1);
        assert!(parsed.problems.first().is_some_and(|p| p.contains("1000 times")), "{:?}", parsed.problems);
    }

    #[test]
    fn where_it_is_comes_off_its_own_field_rather_than_the_end_of_the_summary() {
        // The shape a real university feed sends, verbatim but for the names:
        // the room is both buried at the end of a hundred-character summary
        // and sitting in its own short field. Reading LOCATION is what stops
        // the room being the first thing an ellipsis eats.
        let parsed = read(&event(
            "SUMMARY:MS-A0301\\, Differentiaali- ja integraalilaskenta 3\\, Luento-opetus 23.2.–15.4.2026 - Luento - L01 - E-sali - Y124\r\n\
             DTSTART:20260910T061500Z\r\nDURATION:PT1H45M\r\nLOCATION:E-sali - Y124\r\nTRANSP:OPAQUE",
        ));
        let only = parsed.occurrences.first().expect("one");
        assert_eq!(only.location, "E-sali - Y124");
        assert!(only.name.starts_with("MS-A0301, Differentiaali"), "{}", only.name);
        assert_eq!(only.end - only.start, 105, "PT1H45M");
        assert!(only.description.is_empty(), "that feed sends none, and none is fine");

        // A description is flattened to one line and bounded: it is a hover,
        // not a wall. `\t` is not one of RFC 5545's escapes, so it survives as
        // written rather than being guessed at — the same rule `unescape_text`
        // keeps for `\%`.
        let long = read(&event(
            "SUMMARY:Standup\r\nDTSTART:20260910T090000\r\nDESCRIPTION:line one\\nline two\\n\\nand  spaces",
        ));
        let noted = long.occurrences.first().expect("one");
        assert_eq!(noted.description, "line one line two and spaces");
        assert!(!noted.description.contains('\n'), "a hover is one line");
    }

    #[test]
    fn a_transparent_event_is_shown_and_never_planned_around() {
        // Our own feed writes TRANSP:TRANSPARENT on a due marker, and a
        // household may well subscribe TaskDeck to a calendar TaskDeck feeds.
        let parsed = read(&event(
            "SUMMARY:due: Tax return\r\nDTSTART:20260910T090000\r\nDURATION:PT15M\r\nTRANSP:TRANSPARENT",
        ));
        let only = parsed.occurrences.first().expect("one");
        assert!(only.free);
        assert!(!only.is_busy());
    }

    #[test]
    fn the_calendar_names_itself_when_the_file_says_so() {
        let parsed = read(&format!(
            "X-WR-CALNAME:Work\r\n{}",
            event("SUMMARY:Standup\r\nDTSTART:20260910T090000")
        ));
        assert_eq!(parsed.name.as_deref(), Some("Work"));
    }
}
