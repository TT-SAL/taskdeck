//! Reminders (core.md §14.5, §20).
//!
//! The core only computes them: the `android` stage schedules them as exact
//! alarms and recomputes them before notifying (SYNC_PLAN.md §6.10).

use serde_json::{Map, Value};

use crate::board::{BoardData, Kind};
use crate::canon;
use crate::time::Instant;

/// The longest lead of a kind, in minutes (core.md §14.5: each 0 to 1,440).
const MAX_LEAD_MINUTES: u32 = 1440;

/// The lead of each reminder kind in minutes (core.md §14.5).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ReminderDefaults {
    pub event_lead_minutes: u32,
    pub block_lead_minutes: u32,
    pub due_lead_minutes: u32,
}

/// Leads of 10, 0 and 0 minutes (core.md §14.5, CORE-REM-001).
impl Default for ReminderDefaults {
    fn default() -> Self {
        ReminderDefaults { event_lead_minutes: 10, block_lead_minutes: 0, due_lead_minutes: 0 }
    }
}

/// What a reminder is for (core.md §14.5).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ReminderKind {
    Event,
    Block,
    Due,
}

impl ReminderKind {
    /// The kind's text in the record and in the key (core.md §14.5).
    fn text(self) -> &'static str {
        match self {
            ReminderKind::Event => "event",
            ReminderKind::Block => "block",
            ReminderKind::Due => "due",
        }
    }
}

/// One reminder (core.md §14.5).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Reminder {
    pub key: String,
    pub kind: ReminderKind,
    pub item: u64,
    pub sid: Option<u64>,
    pub at: Instant,
    pub fire_at: Instant,
    pub minutes: Option<u32>,
    pub name: String,
}

impl Reminder {
    /// The reminder's canonical bytes (core.md §14.5).
    ///
    /// The record of §14.5 in the canonical encoding (§4.2): `at`,
    /// `fire_at`, `item`, `key`, `kind`, `name` always; `minutes` and `sid`
    /// when present.
    pub fn to_canonical(&self) -> Vec<u8> {
        let mut m = Map::new();
        m.insert("at".to_owned(), Value::String(self.at.to_canonical()));
        m.insert("fire_at".to_owned(), Value::String(self.fire_at.to_canonical()));
        m.insert("item".to_owned(), Value::from(self.item));
        m.insert("key".to_owned(), Value::String(self.key.clone()));
        m.insert("kind".to_owned(), Value::from(self.kind.text()));
        if let Some(minutes) = self.minutes {
            m.insert("minutes".to_owned(), Value::from(minutes));
        }
        m.insert("name".to_owned(), Value::String(self.name.clone()));
        if let Some(sid) = self.sid {
            m.insert("sid".to_owned(), Value::from(sid));
        }
        // No null and no float, so `encode` cannot fail.
        canon::encode(&Value::Object(m)).unwrap_or_default()
    }
}

/// `<kind>:<item>:<sid or 0>:<at>` (core.md §14.5, CORE-REM-002).
fn key(kind: ReminderKind, item: u64, sid: Option<u64>, at: Instant) -> String {
    format!("{}:{}:{}:{}", kind.text(), item, sid.unwrap_or(0), at.to_canonical())
}

/// The reminders firing in `[from, until)`, by `(fire_at, key)` (core.md §14.5).
///
/// From live items only: an event with a `deadline` (`kind` `event`, at the
/// deadline, its `duration_minutes`); each block of a task (`block`, at the
/// block's start, its minutes); a task with a `deadline` (`due`, at the
/// deadline). Routines give none. `fire_at = at − lead`; a source is kept
/// when `from ≤ fire_at < until` (CORE-REM-001).
pub fn reminders(board: &BoardData, from: Instant, until: Instant, defaults: &ReminderDefaults) -> Vec<Reminder> {
    // SPEC-QUESTION: §14.5 gives each lead the range 0 to 1,440 but not what a larger value does; a lead above 1,440 is taken as 1,440.
    let lead = |minutes: u32| -i64::from(minutes.min(MAX_LEAD_MINUTES)) * 60;
    let mut out = Vec::new();
    let mut add = |kind: ReminderKind, item: &crate::board::Item, sid: Option<u64>, at: Instant, minutes: Option<u32>| {
        let lead_minutes = match kind {
            ReminderKind::Event => defaults.event_lead_minutes,
            ReminderKind::Block => defaults.block_lead_minutes,
            ReminderKind::Due => defaults.due_lead_minutes,
        };
        // A `fire_at` before the instant range lies before every `from`.
        let Some(fire_at) = at.plus_seconds(lead(lead_minutes)) else { return };
        if from <= fire_at && fire_at < until {
            out.push(Reminder {
                key: key(kind, item.id, sid, at),
                kind,
                item: item.id,
                sid,
                at,
                fire_at,
                minutes,
                name: item.name.clone(),
            });
        }
    };
    for item in &board.items {
        match item.kind {
            Kind::Event => {
                if let Some(at) = item.deadline {
                    add(ReminderKind::Event, item, None, at, item.duration_minutes);
                }
            }
            Kind::Task => {
                for block in &item.blocks {
                    add(ReminderKind::Block, item, Some(block.sid), block.start, Some(block.minutes));
                }
                if let Some(at) = item.deadline {
                    add(ReminderKind::Due, item, None, at, None);
                }
            }
            // A routine is a habit, not an appointment (core.md §14.5).
            Kind::Routine => {}
        }
    }
    out.sort_by(|a, b| a.fire_at.cmp(&b.fire_at).then_with(|| a.key.as_bytes().cmp(b.key.as_bytes())));
    out
}
