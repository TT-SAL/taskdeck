//! The snapshot builders, copied from today's `src/phone.rs` with the zone,
//! sids and the changes of core.md §14.3 (core.md §14.2, §14.3, §20).

use serde::Serialize;

use crate::board::{ArchiveRow, BoardData, Item, RowBlock, Subscription};
use crate::overlay::{Covered, Overlay};
use crate::time::{Instant, Zone};

/// The inputs of a snapshot (core.md §14.3).
pub struct SnapshotInput<'a, Z: Zone + ?Sized> {
    pub board: &'a BoardData,
    pub overlay: &'a Overlay,
    pub zone: &'a Z,
    pub now: Instant,
    pub from: chrono::NaiveDate,
    pub days: u32,
    pub version: u64,
    pub ranked: Option<Vec<u64>>,
    pub look: Option<Look>,
    pub dials: Dials,
}

/// Today's snapshot, plus `notes_base` (core.md §14.3).
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub version: u64,
    pub today: chrono::NaiveDate,
    pub now_minutes: i32,
    pub palette: [[u8; 4]; 6],
    pub labels: Labels,
    pub days: Vec<DaySnapshot>,
    pub tray: Tray,
    pub items: Vec<ItemDetail>,
    pub ranked: Vec<u64>,
    pub done: Vec<DoneRow>,
    pub notes: String,
    pub notes_base: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub known: Option<Covered>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub look: Option<Look>,
    pub dials: Dials,
}

/// The page's background picture (core.md §14.3 item 10).
#[derive(Debug, Clone, Serialize)]
pub struct Look {
    pub id: String,
    pub aspect: f32,
    pub top: String,
}

/// The page's picture dials (core.md §14.3 item 10).
#[derive(Debug, Clone, Serialize)]
pub struct Dials {
    pub blur: u32,
    pub light: u32,
}

/// One row of `done`; `archived_at` the canonical instant text (core.md §14.3 item 7).
#[derive(Debug, Clone, Serialize)]
pub struct DoneRow {
    pub id: u64,
    pub archived_at: String,
    pub name: String,
    pub kind: &'static str,
    pub color: usize,
    pub finished: bool,
    pub when: String,
    pub verdict: String,
}

/// Today's label tables (core.md §14.3).
#[derive(Debug, Clone, Serialize)]
pub struct Labels {
    pub severity: [&'static str; 5],
    pub horizon: [&'static str; 4],
    pub horizon_order: [u8; 4],
    pub weekdays: [&'static str; 7],
}

impl Labels {
    /// Today's labels (core.md §14.3).
    pub fn current() -> Self {
        unimplemented!()
    }
}

/// One day of the snapshot (core.md §14.3).
#[derive(Debug, Clone, Serialize)]
pub struct DaySnapshot {
    pub date: chrono::NaiveDate,
    pub weekday: String,
    pub label: String,
    pub is_today: bool,
    pub summary: String,
    pub behind_minutes: i32,
    pub entries: Vec<EntrySnapshot>,
    pub ghosts: Vec<GhostSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subscribed: Vec<SubscribedSnapshot>,
}

/// A subscribed calendar's event on a day (core.md §14.3 item 11).
#[derive(Debug, Clone, Serialize)]
pub struct SubscribedSnapshot {
    pub start: i32,
    pub end: i32,
    pub column: usize,
    pub columns: usize,
    pub all_day: bool,
    pub background: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    #[serde(rename = "where")]
    pub where_: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
    pub name: String,
    pub color: String,
    pub calendar: String,
}

/// A live item on a day; `session` a sid (core.md §14.3 item 5).
#[derive(Debug, Clone, Serialize)]
pub struct EntrySnapshot {
    pub id: u64,
    pub session: Option<u64>,
    pub name: String,
    pub kind: &'static str,
    pub color: usize,
    pub start: i32,
    pub minutes: u32,
    pub marker: bool,
    pub due: bool,
    pub column: usize,
    pub columns: usize,
}

/// An archived block on a day (core.md §14.3, §14.4).
#[derive(Debug, Clone, Serialize)]
pub struct GhostSnapshot {
    pub name: String,
    pub finished: bool,
    pub color: usize,
    pub start: i32,
    pub minutes: u32,
    pub column: usize,
    pub columns: usize,
}

/// The tray of unplanned tasks (core.md §14.3 item 4).
#[derive(Debug, Clone, Serialize)]
pub struct Tray {
    pub due: Vec<TrayCard>,
    pub later: Vec<TrayCard>,
}

/// One card of the tray (core.md §14.3).
#[derive(Debug, Clone, Serialize)]
pub struct TrayCard {
    pub id: u64,
    pub name: String,
    pub color: usize,
    pub deadline: Option<String>,
    pub due_in: Option<String>,
    pub duration: Option<u32>,
    pub planned: u32,
    pub drop_minutes: u32,
}

/// Full details of a live item (core.md §14.3 item 6).
#[derive(Debug, Clone, Serialize)]
pub struct ItemDetail {
    pub id: u64,
    pub name: String,
    pub kind: &'static str,
    pub color: usize,
    pub deadline: Option<String>,
    pub duration: Option<u32>,
    pub planned: u32,
    pub remaining: Option<u32>,
    pub severity: Option<u8>,
    pub horizon: Option<u8>,
    pub repeat: Option<RepeatDetail>,
    pub sessions: Vec<SessionDetail>,
    pub created: chrono::NaiveDate,
}

/// A routine's rule in the detail (core.md §14.3).
#[derive(Debug, Clone, Serialize)]
pub struct RepeatDetail {
    pub days: u8,
    pub summary: String,
    pub start: i32,
    pub minutes: u32,
}

/// One block in the detail; `index` the sid (core.md §14.3 item 6).
#[derive(Debug, Clone, Serialize)]
pub struct SessionDetail {
    pub index: u64,
    pub day: chrono::NaiveDate,
    pub start: i32,
    pub minutes: u32,
}

/// Builds the snapshot (core.md §14.3).
pub fn snapshot<Z: Zone + ?Sized>(input: &SnapshotInput<Z>) -> Snapshot {
    unimplemented!()
}

/// The snapshot as JSON (core.md §14.3).
pub fn answer_snapshot<Z: Zone + ?Sized>(input: &SnapshotInput<Z>) -> serde_json::Value {
    unimplemented!()
}

/// One day, its ghosts from `archive_by_day` (core.md §14.2, §14.4).
pub fn build_day(
    items: &[Item],
    ghosts: &[(&ArchiveRow, &RowBlock)],
    day: chrono::NaiveDate,
    now: Instant,
    zone: &(impl Zone + ?Sized),
) -> DaySnapshot {
    unimplemented!()
}

/// The tray of a day (core.md §14.2, §14.3 item 4).
pub fn build_tray(
    items: &[Item],
    day: chrono::NaiveDate,
    now: Instant,
    shuffle_seed: u64,
    zone: &(impl Zone + ?Sized),
) -> Tray {
    unimplemented!()
}

/// The detail of an item (core.md §14.2, §14.3 item 6).
pub fn item_detail(item: &Item, zone: &(impl Zone + ?Sized)) -> ItemDetail {
    unimplemented!()
}

/// The `done` rows, newest first (core.md §14.2, §14.3 items 7 and 8).
pub fn done_rows<'a>(rows: impl IntoIterator<Item = &'a ArchiveRow>, zone: &(impl Zone + ?Sized)) -> Vec<DoneRow> {
    unimplemented!()
}

/// Adds the subscribed calendars' events to the days (core.md §14.2, §14.3 item 11).
pub fn with_subscribed(
    days: Vec<DaySnapshot>,
    subscriptions: &[Subscription],
    overlay: &Overlay,
    zone: &(impl Zone + ?Sized),
) -> Vec<DaySnapshot> {
    unimplemented!()
}
