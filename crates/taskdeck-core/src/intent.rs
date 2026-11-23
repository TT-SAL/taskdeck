//! Intents (core.md §12, §20).
//!
//! `parse_intent` reads today's wire shapes (today's `Command`, board.rs:111-280)
//! with `serde` derive on private wire structs, so an unknown field is
//! ignored, a missing field takes today's default and a wrong type is an
//! error, with the messages `serde_json` gives today (core.md §12.2 step 3).
//! The wire enum keeps today's name and variant order, so those messages read
//! as today's. Nothing is resolved here: a naive time stays naive
//! (CORE-INT-002) and `session` is a sid (CORE-INT-003).

use serde::de::{Deserializer, Error as _};
use serde::Deserialize;

use crate::CompileError;
use crate::board::{Kind, PictureRef};
use crate::compile::trim;

/// A wire time: exact, or naive with its text (core.md §12.2 step 4).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WireTime {
    Exact(chrono::DateTime<chrono::FixedOffset>),
    Naive(chrono::NaiveDateTime, String),
}

/// An intent, one variant per row of the table (core.md §12.3).
#[derive(Clone, PartialEq, Debug)]
pub enum Intent {
    Add {
        name: String,
        deadline: Option<WireTime>,
        importance: Option<u8>,
        is_event: bool,
        time_importance: Option<u8>,
    },
    QuickAdd { name: String, deadline: Option<WireTime> },
    Create { kind: Kind, name: String, day: chrono::NaiveDate, start: i32, minutes: u32 },
    Rename { id: u64, name: String },
    Complete { id: u64, at: Option<WireTime> },
    Delete { id: u64, at: Option<WireTime> },
    Forget { id: u64 },
    MoveBlock { id: u64, session: Option<u64>, day: chrono::NaiveDate, start: i32, minutes: u32 },
    AddBlock { id: u64, day: chrono::NaiveDate, start: i32, minutes: Option<u32> },
    RemoveBlock { id: u64, session: u64 },
    Unplan { id: u64 },
    SetEstimate { id: u64, minutes: u32 },
    SetDeadline { id: u64, deadline: Option<WireTime> },
    SetSeverity { id: u64, level: u8 },
    SetHorizon { id: u64, level: u8 },
    SetRepeat { id: u64, days: u8, day: chrono::NaiveDate },
    Reflow { day: chrono::NaiveDate, from: Option<i32> },
    SetNotes { text: String, base: Option<u64> },
    Restore { id: u64, archived_at: chrono::DateTime<chrono::FixedOffset> },
    ForgetArchived { id: u64, archived_at: chrono::DateTime<chrono::FixedOffset> },
    AddSubscription { name: String, url: String, color: Option<[u8; 4]> },
    RemoveSubscription { id: u64 },
    RenameSubscription { id: u64, name: String },
    SetSubscriptionColor { id: u64, color: [u8; 4] },
    SetSubscriptionEnabled { id: u64, enabled: bool },
    SetPalette { colors: [[u8; 4]; 6] },
    SetHome { lat: f64, lon: f64 },
    SetPicture { picture: Option<PictureRef> },
}

/// Today's `MAX_BODY_BYTES` (phone.rs:136; core.md §12.2 step 1, §17).
pub(crate) const INTENT_LIMIT: usize = 84_096;

/// `core/intent_json` with status 400 and the parser's message (core.md §12.2, §16).
pub(crate) fn intent_json(message: impl Into<String>) -> CompileError {
    CompileError { code: "core/intent_json", status: 400, message: message.into() }
}

/// The message of a wire time that is none of the accepted forms (core.md §12.2 step 4).
pub(crate) fn not_a_time(text: &str) -> String {
    format!("not a time: `{text}`")
}

/// Parses intent JSON (core.md §12.2).
///
/// The six steps of §12.2: the length before anything is read, then today's
/// `serde` reading (an object with a known `op`, today's fields and
/// defaults, unknown fields ignored), the wire times as §12.2 steps 4 and 5
/// read them, and `day` fields as `chrono::NaiveDate`'s `serde` reads them.
/// Every failure is `core/intent_json` with status 400 (CORE-INT-001). A
/// query (`snapshot`, `feed`, `board`) parses as today and is answered
/// `core/query`, as today's server answers it (phone.rs:892).
pub fn parse_intent(text: &str) -> Result<Intent, CompileError> {
    // Step 1: the length, before anything is allocated (CORE-GEN-009).
    if text.len() > INTENT_LIMIT {
        return Err(intent_json("That request is too large."));
    }
    // Step 2: an object. The derive of an internally tagged enum also reads
    // a JSON array as `[op, field, ...]`, a second wire form the spec refuses.
    // After leading JSON whitespace (RFC 8259 §2) a value that starts with
    // `[` is an array; any other non-object is refused by the derive itself.
    if text.trim_start_matches([' ', '\t', '\n', '\r']).starts_with('[') {
        return Err(intent_json("invalid type: sequence, expected internally tagged enum Command"));
    }
    // Steps 2 to 6.
    let command: Command = serde_json::from_str(text).map_err(|e| intent_json(e.to_string()))?;
    command.into_intent()
}

// ---------------------------------------------------------------- the wire (today's `Command`)

/// Today's `Command` (board.rs:111-280), field for field, with `session` a
/// sid and the three new intents of §12.3 after today's variants. Its name is
/// today's, since `serde`'s messages carry it ("expected internally tagged
/// enum Command").
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Command {
    Snapshot {
        #[serde(rename = "from")]
        _from: chrono::NaiveDate,
        #[serde(default = "one", rename = "days")]
        _days: u32,
    },
    Feed,
    Board,
    Add {
        name: String,
        #[serde(default, deserialize_with = "wire_time")]
        deadline: Option<WireTime>,
        #[serde(default)]
        importance: Option<u8>,
        #[serde(default)]
        is_event: bool,
        #[serde(default)]
        time_importance: Option<u8>,
    },
    QuickAdd {
        name: String,
        #[serde(default, deserialize_with = "wire_time")]
        deadline: Option<WireTime>,
    },
    Rename {
        id: u64,
        name: String,
    },
    Complete {
        id: u64,
        #[serde(default, deserialize_with = "wire_time")]
        at: Option<WireTime>,
    },
    Delete {
        id: u64,
        #[serde(default, deserialize_with = "wire_time")]
        at: Option<WireTime>,
    },
    Forget {
        id: u64,
    },
    MoveBlock {
        id: u64,
        #[serde(default)]
        session: Option<u64>,
        day: chrono::NaiveDate,
        start: i32,
        minutes: u32,
    },
    AddBlock {
        id: u64,
        day: chrono::NaiveDate,
        start: i32,
        #[serde(default)]
        minutes: Option<u32>,
    },
    RemoveBlock {
        id: u64,
        session: u64,
    },
    Unplan {
        id: u64,
    },
    SetEstimate {
        id: u64,
        minutes: u32,
    },
    SetDeadline {
        id: u64,
        #[serde(default, deserialize_with = "wire_time")]
        deadline: Option<WireTime>,
    },
    SetSeverity {
        id: u64,
        level: u8,
    },
    SetHorizon {
        id: u64,
        level: u8,
    },
    SetRepeat {
        id: u64,
        days: u8,
        day: chrono::NaiveDate,
    },
    Create {
        kind: WireKind,
        name: String,
        day: chrono::NaiveDate,
        start: i32,
        minutes: u32,
    },
    Reflow {
        day: chrono::NaiveDate,
        #[serde(default)]
        from: Option<i32>,
    },
    SetNotes {
        text: String,
        #[serde(default)]
        base: Option<u64>,
    },
    Restore {
        id: u64,
        #[serde(deserialize_with = "exact_time")]
        archived_at: chrono::DateTime<chrono::FixedOffset>,
    },
    ForgetArchived {
        id: u64,
        #[serde(deserialize_with = "exact_time")]
        archived_at: chrono::DateTime<chrono::FixedOffset>,
    },
    AddSubscription {
        name: String,
        url: String,
        #[serde(default)]
        color: Option<[u8; 4]>,
    },
    RemoveSubscription {
        id: u64,
    },
    RenameSubscription {
        id: u64,
        name: String,
    },
    SetSubscriptionColor {
        id: u64,
        color: [u8; 4],
    },
    SetSubscriptionEnabled {
        id: u64,
        enabled: bool,
    },
    SetPalette {
        colors: [[u8; 4]; 6],
    },
    SetHome {
        lat: f64,
        lon: f64,
    },
    SetPicture {
        #[serde(default, deserialize_with = "wire_picture")]
        picture: Option<PictureRef>,
    },
}

/// Today's `Kind` on the wire (board.rs:78-84).
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum WireKind {
    Task,
    Event,
    Routine,
}

fn one() -> u32 {
    1
}

impl Command {
    fn into_intent(self) -> Result<Intent, CompileError> {
        Ok(match self {
            Command::Snapshot { .. } | Command::Feed | Command::Board => {
                return Err(CompileError {
                    code: "core/query",
                    status: 400,
                    message: "That is a query, not a command.".to_owned(),
                });
            }
            Command::Add { name, deadline, importance, is_event, time_importance } => {
                Intent::Add { name, deadline, importance, is_event, time_importance }
            }
            Command::QuickAdd { name, deadline } => Intent::QuickAdd { name, deadline },
            Command::Rename { id, name } => Intent::Rename { id, name },
            Command::Complete { id, at } => Intent::Complete { id, at },
            Command::Delete { id, at } => Intent::Delete { id, at },
            Command::Forget { id } => Intent::Forget { id },
            Command::MoveBlock { id, session, day, start, minutes } => Intent::MoveBlock { id, session, day, start, minutes },
            Command::AddBlock { id, day, start, minutes } => Intent::AddBlock { id, day, start, minutes },
            Command::RemoveBlock { id, session } => Intent::RemoveBlock { id, session },
            Command::Unplan { id } => Intent::Unplan { id },
            Command::SetEstimate { id, minutes } => Intent::SetEstimate { id, minutes },
            Command::SetDeadline { id, deadline } => Intent::SetDeadline { id, deadline },
            Command::SetSeverity { id, level } => Intent::SetSeverity { id, level },
            Command::SetHorizon { id, level } => Intent::SetHorizon { id, level },
            Command::SetRepeat { id, days, day } => Intent::SetRepeat { id, days, day },
            Command::Create { kind, name, day, start, minutes } => {
                let kind = match kind {
                    WireKind::Task => Kind::Task,
                    WireKind::Event => Kind::Event,
                    WireKind::Routine => Kind::Routine,
                };
                Intent::Create { kind, name, day, start, minutes }
            }
            Command::Reflow { day, from } => Intent::Reflow { day, from },
            Command::SetNotes { text, base } => Intent::SetNotes { text, base },
            Command::Restore { id, archived_at } => Intent::Restore { id, archived_at },
            Command::ForgetArchived { id, archived_at } => Intent::ForgetArchived { id, archived_at },
            Command::AddSubscription { name, url, color } => Intent::AddSubscription { name, url, color },
            Command::RemoveSubscription { id } => Intent::RemoveSubscription { id },
            Command::RenameSubscription { id, name } => Intent::RenameSubscription { id, name },
            Command::SetSubscriptionColor { id, color } => Intent::SetSubscriptionColor { id, color },
            Command::SetSubscriptionEnabled { id, enabled } => Intent::SetSubscriptionEnabled { id, enabled },
            Command::SetPalette { colors } => Intent::SetPalette { colors },
            Command::SetHome { lat, lon } => Intent::SetHome { lat, lon },
            Command::SetPicture { picture } => Intent::SetPicture { picture },
        })
    }
}

// ---------------------------------------------------------------- wire times (§12.2 steps 4 and 5)

/// RFC 3339 as `chrono::DateTime::parse_from_rfc3339` reads it, a leap
/// second excluded (core.md §12.2 step 4).
fn exact(text: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    use chrono::Timelike;
    chrono::DateTime::parse_from_rfc3339(text).ok().filter(|t| t.nanosecond() < 1_000_000_000)
}

/// A trimmed, non-empty wire-time text as §12.2 step 4 reads it.
fn read_wire_time(text: &str) -> Option<WireTime> {
    if let Some(t) = exact(text) {
        return Some(WireTime::Exact(t));
    }
    chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S"))
        .ok()
        .map(|naive| WireTime::Naive(naive, text.to_owned()))
}

/// A wire-time field (core.md §12.2 step 4): absent, `null` or empty after
/// trim is none; trimmed per G1 (core.md §13.3), as today's
/// `wire::optional_local` trims (board.rs:398).
fn wire_time<'de, D: Deserializer<'de>>(d: D) -> Result<Option<WireTime>, D::Error> {
    let text: Option<String> = Option::deserialize(d)?;
    match text.as_deref().map(trim).filter(|t| !t.is_empty()) {
        None => Ok(None),
        Some(t) => read_wire_time(t).map(Some).ok_or_else(|| D::Error::custom(not_a_time(t))),
    }
}

/// `archived_at` (core.md §12.2 step 5): RFC 3339 as in step 4, exact only.
// Lead ruling, spec change 5, §12.2 step 5: a bad `archived_at` has step 4's
// message, `not a time: `<text>``.
fn exact_time<'de, D: Deserializer<'de>>(d: D) -> Result<chrono::DateTime<chrono::FixedOffset>, D::Error> {
    let text = String::deserialize(d)?;
    let t = trim(&text);
    exact(t).ok_or_else(|| D::Error::custom(not_a_time(t)))
}

/// A `PictureRef` on the wire: the §6.9 record's fields, ranges not yet
/// checked (the compile step checks them, `core/picture`, core.md §13.4).
#[derive(Deserialize)]
struct WirePicture {
    blob: String,
    epoch: u32,
    parts: u32,
    sha256: String,
    size: u64,
}

fn wire_picture<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PictureRef>, D::Error> {
    let Some(p) = Option::<WirePicture>::deserialize(d)? else {
        return Ok(None);
    };
    let blob = hex::<16>(&p.blob).ok_or_else(|| D::Error::custom("`blob` is not 32 lowercase hex digits"))?;
    let sha256 = hex::<32>(&p.sha256).ok_or_else(|| D::Error::custom("`sha256` is not 64 lowercase hex digits"))?;
    Ok(Some(PictureRef { blob, epoch: p.epoch, parts: p.parts, sha256, size: p.size }))
}

/// A `hexN` (core.md §4.4): exactly `2N` lowercase hexadecimal digits.
fn hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    crate::canon::hex::<N>(&serde_json::Value::String(text.to_owned())).ok()
}

#[cfg(test)]
mod tests;
