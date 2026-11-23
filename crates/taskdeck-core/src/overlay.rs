//! The overlay types, copied from today's `src/subscriptions.rs` with
//! `FetchStatus.at` at a fixed offset (core.md §14.6, §20).
//!
//! Ported from today's `src/subscriptions.rs` (lines 184-484): what every
//! function here returns is today's, byte for byte, except two changes of
//! brief c3: `OverlayEvent::minutes` saturates, and `Overlay::sealed` orders
//! events that tie on today's sort key by their remaining fields. Not ported: the fetches,
//! the feeds thread, the cache file and `checked_url` (task D). None of these
//! functions reads a clock (CORE-GEN-003): `FetchStatus.at` is filled by the
//! caller.

use serde::{Deserialize, Serialize};

use crate::board::Subscription;
use crate::planner;

/// Today's `subscriptions::SUMMARY_MAX_CHARS`.
const SUMMARY_MAX_CHARS: usize = 200;
/// Today's `subscriptions::EVENTS_MAX`.
const EVENTS_MAX: usize = 5_000;
/// Today's `subscriptions::LOCATION_MAX_CHARS`.
const LOCATION_MAX_CHARS: usize = 120;
/// Today's `subscriptions::DESCRIPTION_MAX_CHARS`.
const DESCRIPTION_MAX_CHARS: usize = 300;
/// Today's `subscriptions::WHY_MAX_CHARS`.
const WHY_MAX_CHARS: usize = 200;

/// The span the subscribed calendars were read for (today's overlay; core.md §14.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Covered {
    pub from: chrono::NaiveDate,
    pub until: chrono::NaiveDate,
}

impl Covered {
    /// Whether a day lies in the span (today's overlay; core.md §14.6).
    pub fn holds(&self, day: chrono::NaiveDate) -> bool {
        self.from <= day && day <= self.until
    }
}

/// An event of a subscribed calendar on one day (today's overlay; core.md §14.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayEvent {
    pub subscription: u64,
    pub day: chrono::NaiveDate,
    pub start: i32,
    pub end: i32,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub free: bool,
    pub summary: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub description: String,
}

/// Events at least this long are background (today's overlay; core.md §14.6).
pub const LONG_EVENT_MINUTES: i32 = 12 * 60;

impl OverlayEvent {
    /// The event's length in minutes (today's overlay; core.md §14.6).
    ///
    /// Saturating (brief c3, bound 4): an event read straight from a cache
    /// file and never sealed can hold any `i32`s, and today's subtraction
    /// overflowed on them (a panic in a debug build, a wrapped length in a
    /// release one).
    pub fn minutes(&self) -> i32 {
        self.end.saturating_sub(self.start).max(0)
    }

    /// Whether the event is drawn as background (today's overlay; core.md §14.6).
    pub fn is_background(&self) -> bool {
        self.all_day || self.minutes() >= LONG_EVENT_MINUTES
    }

    /// Whether the event makes its time busy (today's overlay; core.md §14.6).
    pub fn is_busy(&self) -> bool {
        !self.is_background() && !self.free && self.end > self.start
    }
}

/// The outcome of one fetch (today's overlay; core.md §14.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FetchOutcome {
    Ok {
        events: usize,
        problems: Vec<String>,
        #[serde(default)]
        title: Option<String>,
    },
    Failed {
        why: String,
    },
}

/// A subscription's placeholder name (today's overlay; core.md §14.6).
pub fn placeholder_name(id: u64) -> String {
    format!("Calendar {id}")
}

/// A name taken from a feed's URL (today's overlay; core.md §14.6).
pub fn name_from_url(url: &str) -> Option<String> {
    let host = url.split("://").nth(1)?;
    let host = host.split(['/', '?', '#']).next()?;
    // A userinfo prefix belongs to nobody's calendar name.
    let host = host.rsplit('@').next()?;
    let host = host.split(':').next()?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    (!host.is_empty()).then(|| host.to_string())
}

/// A better name for a subscription from its feed (today's overlay; core.md §14.6).
pub fn better_name(subscription: &Subscription, overlay: &Overlay) -> Option<String> {
    if !is_unnamed(subscription) {
        return None;
    }
    let from_feed = match overlay.status_for(subscription.id).map(|status| &status.outcome) {
        Some(FetchOutcome::Ok { title: Some(title), .. }) if !title.trim().is_empty() => {
            Some(title.trim().to_string())
        }
        _ => None,
    };
    let better = from_feed.or_else(|| name_from_url(&subscription.url))?;
    (better != subscription.name).then_some(better)
}

/// Whether a subscription still has its placeholder name (today's overlay; core.md §14.6).
pub fn is_unnamed(subscription: &Subscription) -> bool {
    subscription.name == placeholder_name(subscription.id)
        || Some(&subscription.name) == name_from_url(&subscription.url).as_ref()
}

/// The last fetch of a subscription; `at` at a fixed offset (core.md §14.6).
///
/// Today's `at` is a date-time in the process zone; its serde form, RFC 3339 with the
/// offset, is the same as this one's, so today's `subscribed_cache.json`
/// reads and writes unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchStatus {
    pub subscription: u64,
    pub at: chrono::DateTime<chrono::FixedOffset>,
    pub outcome: FetchOutcome,
}

/// The subscribed calendars' events and fetch states (today's overlay; core.md §14.6).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overlay {
    #[serde(default)]
    pub events: Vec<OverlayEvent>,
    #[serde(default)]
    pub status: Vec<FetchStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub covers: Option<Covered>,
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fold(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// `day.format("%Y%m%d")` of today's `digest`, written out: the core's chrono
/// has no `alloc` feature and so no `format`. chrono writes a year outside 0 to
/// 9999 with its sign and at least four digits.
pub(crate) fn ymd(day: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    let y = day.year();
    let year = if (0..10_000).contains(&y) { format!("{y:04}") } else { format!("{y:+05}") };
    format!("{year}{:02}{:02}", day.month(), day.day())
}

impl Overlay {
    /// The overlay with its covered span (today's overlay; core.md §14.6).
    pub fn covering(mut self, covers: Option<Covered>) -> Overlay {
        self.covers = covers;
        self
    }

    /// An overlay from events and states, sorted (today's overlay; core.md §14.6).
    ///
    /// Bounds the strings and the counts, drops an event that ends before it
    /// starts, and sorts (today's `sealed`).
    pub fn sealed(events: Vec<OverlayEvent>, status: Vec<FetchStatus>) -> Overlay {
        let mut events: Vec<OverlayEvent> = events
            .into_iter()
            .filter(|event| event.end >= event.start)
            .map(|mut event| {
                event.start = event.start.clamp(0, planner::DAY_MINUTES);
                event.end = event.end.clamp(event.start, planner::DAY_MINUTES);
                event.summary = event.summary.chars().take(SUMMARY_MAX_CHARS).collect();
                event.location = event.location.chars().take(LOCATION_MAX_CHARS).collect();
                event.description = event.description.chars().take(DESCRIPTION_MAX_CHARS).collect();
                event
            })
            .take(EVENTS_MAX)
            .collect();
        // Sorted so a feed free to list its events in a different order each
        // time does not read as a change. Events that tie on today's key are
        // ordered by their remaining fields (brief c3, bound 6), so the order
        // they were handed over in shows nowhere.
        events.sort_by(|a, b| {
            (a.day, a.start, a.end, a.subscription, &a.summary, &a.location, &a.description, a.all_day, a.free).cmp(&(
                b.day,
                b.start,
                b.end,
                b.subscription,
                &b.summary,
                &b.location,
                &b.description,
                b.all_day,
                b.free,
            ))
        });
        let status = status
            .into_iter()
            .map(|mut status| {
                if let FetchOutcome::Failed { why } = &mut status.outcome {
                    *why = why.chars().take(WHY_MAX_CHARS).collect();
                }
                status
            })
            .collect();
        Overlay { events, status, covers: None }
    }

    /// A digest of the overlay (today's overlay; core.md §14.6).
    ///
    /// FNV-1a over the events only: not the status, which carries the time of
    /// the last attempt, and not the covered span (today's `digest`).
    pub fn digest(&self) -> u64 {
        let mut hash = FNV_OFFSET;
        for event in &self.events {
            hash = fold(hash, &event.subscription.to_le_bytes());
            hash = fold(hash, &ymd(event.day).into_bytes());
            hash = fold(hash, &event.start.to_le_bytes());
            hash = fold(hash, &event.end.to_le_bytes());
            hash = fold(hash, &[event.all_day as u8, event.free as u8]);
            hash = fold(hash, event.summary.as_bytes());
            hash = fold(hash, event.location.as_bytes());
            hash = fold(hash, event.description.as_bytes());
        }
        hash
    }

    /// The busy spans of a day, for reflow (today's overlay; core.md §14.6).
    ///
    /// An all-day event, a background span and a `TRANSPARENT` marker are not
    /// anchors (today's `anchors_on`).
    pub fn anchors_on(&self, day: chrono::NaiveDate) -> Vec<(i32, i32)> {
        self.events
            .iter()
            .filter(|event| event.day == day && event.is_busy())
            .map(|event| (event.start, event.end))
            .collect()
    }

    /// The events of a day (today's overlay; core.md §14.6).
    pub fn events_on(&self, day: chrono::NaiveDate) -> impl Iterator<Item = &OverlayEvent> {
        self.events.iter().filter(move |event| event.day == day)
    }

    /// Drops a subscription's events and state (today's overlay; core.md §14.6).
    pub fn forget(&mut self, subscription: u64) {
        self.events.retain(|event| event.subscription != subscription);
        self.status.retain(|status| status.subscription != subscription);
    }

    /// A subscription's last fetch (today's overlay; core.md §14.6).
    pub fn status_for(&self, subscription: u64) -> Option<&FetchStatus> {
        self.status.iter().find(|status| status.subscription == subscription)
    }
}

#[cfg(test)]
mod tests;
