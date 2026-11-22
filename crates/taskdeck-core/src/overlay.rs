//! The overlay types, copied from today's `src/subscriptions.rs` with
//! `FetchStatus.at` at a fixed offset (core.md §14.6, §20).

use serde::{Deserialize, Serialize};

use crate::board::Subscription;

/// The span the subscribed calendars were read for (today's overlay; core.md §14.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Covered {
    pub from: chrono::NaiveDate,
    pub until: chrono::NaiveDate,
}

impl Covered {
    /// Whether a day lies in the span (today's overlay; core.md §14.6).
    pub fn holds(&self, day: chrono::NaiveDate) -> bool {
        unimplemented!()
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
    pub fn minutes(&self) -> i32 {
        unimplemented!()
    }

    /// Whether the event is drawn as background (today's overlay; core.md §14.6).
    pub fn is_background(&self) -> bool {
        unimplemented!()
    }

    /// Whether the event makes its time busy (today's overlay; core.md §14.6).
    pub fn is_busy(&self) -> bool {
        unimplemented!()
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
    unimplemented!()
}

/// A name taken from a feed's URL (today's overlay; core.md §14.6).
pub fn name_from_url(url: &str) -> Option<String> {
    unimplemented!()
}

/// A better name for a subscription from its feed (today's overlay; core.md §14.6).
pub fn better_name(subscription: &Subscription, overlay: &Overlay) -> Option<String> {
    unimplemented!()
}

/// Whether a subscription still has its placeholder name (today's overlay; core.md §14.6).
pub fn is_unnamed(subscription: &Subscription) -> bool {
    unimplemented!()
}

/// The last fetch of a subscription; `at` at a fixed offset (core.md §14.6).
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

impl Overlay {
    /// The overlay with its covered span (today's overlay; core.md §14.6).
    pub fn covering(self, covers: Option<Covered>) -> Overlay {
        unimplemented!()
    }

    /// An overlay from events and states, sorted (today's overlay; core.md §14.6).
    pub fn sealed(events: Vec<OverlayEvent>, status: Vec<FetchStatus>) -> Overlay {
        unimplemented!()
    }

    /// A digest of the overlay (today's overlay; core.md §14.6).
    pub fn digest(&self) -> u64 {
        unimplemented!()
    }

    /// The busy spans of a day, for reflow (today's overlay; core.md §14.6).
    pub fn anchors_on(&self, day: chrono::NaiveDate) -> Vec<(i32, i32)> {
        unimplemented!()
    }

    /// The events of a day (today's overlay; core.md §14.6).
    // Skeleton: `!` is no iterator, so the opaque type needs a concrete one
    // after the `unimplemented!()`.
    #[allow(unreachable_code)]
    pub fn events_on(&self, day: chrono::NaiveDate) -> impl Iterator<Item = &OverlayEvent> {
        unimplemented!();
        std::iter::empty()
    }

    /// Drops a subscription's events and state (today's overlay; core.md §14.6).
    pub fn forget(&mut self, subscription: u64) {
        unimplemented!()
    }

    /// A subscription's last fetch (today's overlay; core.md §14.6).
    pub fn status_for(&self, subscription: u64) -> Option<&FetchStatus> {
        unimplemented!()
    }
}
