//! The snapshot builders, copied from today's `src/phone.rs` with the zone,
//! sids and the changes of core.md §14.3 (core.md §14.2, §14.3, §20).

use chrono::{Datelike, Timelike};
use serde::Serialize;

use crate::board::{ArchiveRow, BoardData, Item, Kind, RowBlock, Subscription};
use crate::overlay::{Covered, Overlay, OverlayEvent};
use crate::planner;
use crate::read;
use crate::tasks::{HORIZON_DISPLAY_ORDER, HORIZON_LABELS, IMPORTANCE_LABELS, WEEKDAY_NAMES};
use crate::time::{self, Instant, Zone};

/// The most days one snapshot builds; more are cut down quietly (today's
/// `MAX_SNAPSHOT_DAYS`, phone.rs:116; core.md §17).
const MAX_SNAPSHOT_DAYS: u32 = 31;

/// The most rows `done` carries (today's `DONE_ROWS_MAX`, phone.rs:167).
const DONE_ROWS_MAX: usize = 40;

/// chrono's `%A`, Monday first.
const WEEKDAY_FULL: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/// The wire name of a kind (core.md §14.3 item 14; today's `kind_name`).
fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Task => "task",
        Kind::Event => "event",
        Kind::Routine => "routine",
    }
}

/// chrono's `%Y`: the year, at least four digits, a sign outside 0 to 9999.
fn year_text(year: i32) -> String {
    if (0..10_000).contains(&year) { format!("{year:04}") } else { format!("{year:+05}") }
}

/// Today's `local_input` (board.rs:422): `%Y-%m-%dT%H:%M` of the local time
/// in `zone`, the shape a `datetime-local` input takes.
fn local_input(at: Instant, zone: &(impl Zone + ?Sized)) -> String {
    let local = time::local_date_time(at, zone);
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}",
        year_text(local.year()),
        local.month(),
        local.day(),
        local.hour(),
        local.minute()
    )
}

/// chrono's `%A` of a day.
fn weekday_full(day: chrono::NaiveDate) -> &'static str {
    let index = usize::try_from(day.weekday().num_days_from_monday()).unwrap_or(0);
    WEEKDAY_FULL.get(index).copied().unwrap_or("")
}

/// chrono's `%a` of a day.
fn weekday_abbrev(day: chrono::NaiveDate) -> &'static str {
    let index = usize::try_from(day.weekday().num_days_from_monday()).unwrap_or(0);
    WEEKDAY_NAMES.get(index).copied().unwrap_or("")
}

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
        Self {
            severity: IMPORTANCE_LABELS,
            horizon: HORIZON_LABELS,
            horizon_order: HORIZON_DISPLAY_ORDER,
            weekdays: WEEKDAY_NAMES,
        }
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
///
/// Today's `phone::snapshot` (phone.rs:218-272) with the fourteen changes of
/// §14.3: `days` clamped to 1 to 31; one `build_day` per day from `from`,
/// its ghosts from one pass of `archive_by_day`; the subscribed calendars
/// from the caller's overlay; the tray of `from`; every live item in
/// ascending id; `ranked` the caller's or `ranked_ids`; `done` newest first;
/// `notes` and `notes_base`; `known`; the caller's `look` and `dials`. Every
/// local date and minute is taken in `input.zone` (CORE-READ-001). The board
/// is read, never changed or cloned (CORE-READ-002).
pub fn snapshot<Z: Zone + ?Sized>(input: &SnapshotInput<Z>) -> Snapshot {
    let SnapshotInput { board, overlay, zone, now, from, .. } = *input;
    let days = input.days.clamp(1, MAX_SNAPSHOT_DAYS);
    // One pass over the archive serves every day (§14.4, CORE-PERF-001).
    let ghosts = read::archive_by_day(board, from, days, zone);
    // The days stop at the last date chrono holds, where today's `from +
    // days(offset)` would panic.
    let day_snapshots: Vec<DaySnapshot> = (0..days)
        .map_while(|offset| from.checked_add_days(chrono::Days::new(u64::from(offset))))
        .zip(ghosts.iter())
        .map(|(day, bucket)| build_day(&board.items, bucket, day, now, zone))
        .collect();
    let day_snapshots = with_subscribed(day_snapshots, &board.subscriptions, overlay, zone);

    Snapshot {
        version: input.version,
        today: time::local_day(now, zone),
        now_minutes: time::local_minutes(now, zone),
        palette: board.palette,
        labels: Labels::current(),
        days: day_snapshots,
        tray: build_tray(&board.items, from, now, read::shuffle_seed(now, zone), zone),
        items: board.items.iter().map(|item| item_detail(item, zone)).collect(),
        ranked: input.ranked.clone().unwrap_or_else(|| read::ranked_ids(board, now, zone)),
        done: done_rows(read::archive_newest_first(board), zone),
        notes: board.notes.clone(),
        notes_base: board.notes_rev,
        // Only when there is something to be ignorant about.
        known: if board.subscriptions.iter().any(|s| s.enabled) { overlay.covers } else { None },
        look: input.look.clone(),
        dials: input.dials.clone(),
    }
}

/// The snapshot as JSON (core.md §14.3).
///
/// `serde_json::to_value(snapshot(input))`. Every key is a string, so the
/// conversion cannot fail; a non-finite `Look.aspect` is written as `null`.
pub fn answer_snapshot<Z: Zone + ?Sized>(input: &SnapshotInput<Z>) -> serde_json::Value {
    serde_json::to_value(snapshot(input)).unwrap_or_default()
}

/// One day, its ghosts from `archive_by_day` (core.md §14.2, §14.4).
///
/// Today's `build_day` (phone.rs:2346-2470). Live entries and ghosts are laid
/// out together. Entries sort by `(start, id, session)` with `session` a sid
/// (§14.3 item 5); each ghost is placed at its block's local minutes with at
/// least 15 minutes and the ghosts sort stably by start (§14.4).
pub fn build_day(
    items: &[Item],
    ghosts: &[(&ArchiveRow, &RowBlock)],
    day: chrono::NaiveDate,
    now: Instant,
    zone: &(impl Zone + ?Sized),
) -> DaySnapshot {
    struct Live<'a> {
        item: &'a Item,
        placed: planner::DayPlacement,
    }
    let mut live: Vec<Live> = Vec::new();
    for item in items {
        for placed in planner::placements_for(item, day, zone) {
            live.push(Live { item, placed });
        }
    }
    live.sort_by_key(|entry| (entry.placed.placement.start(), entry.item.id, entry.placed.session));

    struct Ghost<'a> {
        row: &'a ArchiveRow,
        placement: planner::Placement,
    }
    let mut ghosts: Vec<Ghost> = ghosts
        .iter()
        .map(|&(row, block)| Ghost {
            row,
            placement: planner::Placement::Block {
                start: time::local_minutes(block.start, zone),
                minutes: block.minutes.max(planner::MIN_BLOCK_MINUTES),
            },
        })
        .collect();
    ghosts.sort_by_key(|ghost| ghost.placement.start());

    let placements: Vec<planner::Placement> = live
        .iter()
        .map(|entry| entry.placed.placement)
        .chain(ghosts.iter().map(|ghost| ghost.placement))
        .collect();
    let lanes = planner::lay_out(&placements);
    let (live_lanes, ghost_lanes) = lanes.split_at(live.len().min(lanes.len()));

    let entries: Vec<EntrySnapshot> = live
        .iter()
        .zip(live_lanes)
        .map(|(entry, lane)| {
            let (start, minutes, marker, due) = match entry.placed.placement {
                planner::Placement::Block { start, minutes } => (start, minutes, false, false),
                planner::Placement::Marker { at, due } => (at, 0, true, due),
            };
            EntrySnapshot {
                id: entry.item.id,
                session: entry.placed.session,
                name: entry.item.name.clone(),
                kind: kind_name(entry.item.kind),
                color: entry.item.calendar_item_color(),
                start,
                minutes,
                marker,
                due,
                column: lane.column,
                columns: lane.columns,
            }
        })
        .collect();

    let ghost_snapshots: Vec<GhostSnapshot> = ghosts
        .iter()
        .zip(ghost_lanes)
        .filter_map(|(ghost, lane)| match ghost.placement {
            planner::Placement::Block { start, minutes } => Some(GhostSnapshot {
                name: ghost.row.name.clone(),
                finished: ghost.row.was_finished(),
                color: ghost.row.color_id(),
                start,
                minutes,
                column: lane.column,
                columns: lane.columns,
            }),
            planner::Placement::Marker { .. } => None,
        })
        .collect();

    // The masthead's figure, counted the way the desktop counts it: routines
    // apart from planned work, and finished work apart from both.
    let (routine, planned): (Vec<&Live>, Vec<&Live>) = live.iter().partition(|entry| entry.item.is_routine());
    let planned_summary = planner::summarize(&planned.iter().map(|e| e.placed.placement).collect::<Vec<_>>());
    let routine_summary = planner::summarize(&routine.iter().map(|e| e.placed.placement).collect::<Vec<_>>());
    let done = planner::summarize(&ghosts.iter().map(|g| g.placement).collect::<Vec<_>>());
    let summary = planner::summary_text(planned_summary, routine_summary, done);

    // Only work blocks count as behind: an event that has passed happened,
    // and a routine that has passed was never owed.
    let behind_minutes = time::now_marker(day, now, zone)
        .map(|now_minutes| {
            let work: Vec<planner::Placement> = planned
                .iter()
                .filter(|entry| entry.item.kind != Kind::Event && entry.placed.session.is_some())
                .map(|entry| entry.placed.placement)
                .collect();
            planner::behind_minutes(&work, now_minutes)
        })
        .unwrap_or(0);

    DaySnapshot {
        date: day,
        weekday: weekday_full(day).to_string(),
        label: format!("{} {} {}", day.day(), time::month_name(day.month()), year_text(day.year())),
        is_today: day == time::local_day(now, zone),
        summary,
        behind_minutes,
        entries,
        ghosts: ghost_snapshots,
        // Filled by `with_subscribed`: this function is the board's own things.
        subscribed: Vec::new(),
    }
}

/// The tray of a day (core.md §14.2, §14.3 item 4).
///
/// Today's `build_tray` (phone.rs:2474-2502): every task still wanting a
/// slot, most pressing first, split into owed by `day` (its deadline's local
/// day on or before it) and the rest. The items come in ascending id and the
/// sort is stable, so equal scores keep ascending id.
pub fn build_tray(
    items: &[Item],
    day: chrono::NaiveDate,
    now: Instant,
    shuffle_seed: u64,
    zone: &(impl Zone + ?Sized),
) -> Tray {
    let mut wanting: Vec<(f32, &Item)> = items
        .iter()
        .filter(|item| item.wants_planning())
        .map(|item| (item.importance_score(now, shuffle_seed), item))
        .collect();
    wanting.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut due = Vec::new();
    let mut later = Vec::new();
    for (_, item) in wanting {
        let card = TrayCard {
            id: item.id,
            name: item.name.clone(),
            color: item.calendar_item_color(),
            deadline: item.deadline.map(|deadline| local_input(deadline, zone)),
            due_in: item.deadline.map(|deadline| planner::relative_due(deadline, now)),
            duration: item.duration_minutes,
            planned: item.planned_minutes(),
            drop_minutes: planner::drop_length_for(item),
        };
        match planner::backlog_group(item.deadline, day, zone) {
            planner::BacklogGroup::Due => due.push(card),
            planner::BacklogGroup::Later => later.push(card),
        }
    }
    Tray { due, later }
}

/// The detail of an item (core.md §14.2, §14.3 item 6).
///
/// Today's `item_detail` (phone.rs:2504-2534); `sessions` are the blocks in
/// ascending sid, each with its sid as `index` and its local day and minutes
/// in `zone`.
pub fn item_detail(item: &Item, zone: &(impl Zone + ?Sized)) -> ItemDetail {
    ItemDetail {
        id: item.id,
        name: item.name.clone(),
        kind: kind_name(item.kind),
        color: item.calendar_item_color(),
        deadline: item.deadline.map(|deadline| local_input(deadline, zone)),
        duration: item.duration_minutes,
        planned: item.planned_minutes(),
        remaining: item.remaining_minutes(),
        severity: item.importance,
        horizon: item.time_importance,
        repeat: item.recurrence.map(|rule| RepeatDetail {
            days: rule.days,
            summary: rule.summary(),
            start: rule.start_minutes,
            minutes: rule.minutes,
        }),
        sessions: item
            .blocks
            .iter()
            .map(|block| SessionDetail {
                index: block.sid,
                day: time::local_day(block.start, zone),
                start: time::local_minutes(block.start, zone),
                minutes: block.minutes,
            })
            .collect(),
        created: time::local_day(item.created, zone),
    }
}

/// The `done` rows, newest first (core.md §14.2, §14.3 items 7 and 8).
///
/// Today's `done_rows` (phone.rs:2131-2146): the first 40 rows given, which
/// the snapshot gives newest first (`archive_newest_first`). `archived_at`
/// is the canonical instant text (§4.4), `when` chrono's
/// `%a %-d %b %H:%M` of the local time in `zone`.
pub fn done_rows<'a>(rows: impl IntoIterator<Item = &'a ArchiveRow>, zone: &(impl Zone + ?Sized)) -> Vec<DoneRow> {
    rows.into_iter()
        .take(DONE_ROWS_MAX)
        .map(|row| {
            let local = time::local_date_time(row.archived_at, zone);
            DoneRow {
                id: row.id,
                archived_at: row.archived_at.to_canonical(),
                name: row.name.clone(),
                kind: kind_name(row.kind),
                color: row.color_id(),
                finished: row.was_finished(),
                when: format!(
                    "{} {} {} {:02}:{:02}",
                    weekday_abbrev(local.date()),
                    local.day(),
                    time::month_abbrev(local.month()),
                    local.hour(),
                    local.minute()
                ),
                verdict: row.verdict(zone),
            }
        })
        .collect()
}

/// Adds the subscribed calendars' events to the days (core.md §14.2, §14.3 item 11).
///
/// Today's `with_subscribed` (phone.rs:2335-2395). The overlay's events are
/// already bucketed by the local day they were read for, so `zone` is not
/// read here: it is in the signature so that every builder takes the caller's
/// zone alike (§20).
pub fn with_subscribed(
    mut days: Vec<DaySnapshot>,
    subscriptions: &[Subscription],
    overlay: &Overlay,
    zone: &(impl Zone + ?Sized),
) -> Vec<DaySnapshot> {
    let _ = zone;
    for day in &mut days {
        let showing: Vec<(&OverlayEvent, &Subscription)> = overlay
            .events_on(day.date)
            .filter_map(|event| {
                // A calendar switched off is not drawn.
                let subscription = subscriptions.iter().find(|s| s.id == event.subscription)?;
                subscription.enabled.then_some((event, subscription))
            })
            .collect();

        // Laid out among themselves; a background span takes no part in the
        // column packing and is drawn behind them at full width.
        let placements: Vec<planner::Placement> = showing
            .iter()
            .map(|(event, _)| planner::Placement::Block {
                start: event.start,
                minutes: if event.is_background() { 0 } else { u32::try_from(event.minutes().max(1)).unwrap_or(1) },
            })
            .collect();
        let lanes = planner::lay_out(&placements);

        day.subscribed = showing
            .iter()
            .zip(lanes.iter())
            .map(|((event, subscription), lane)| SubscribedSnapshot {
                start: event.start,
                end: event.end,
                column: if event.is_background() { 0 } else { lane.column },
                columns: if event.is_background() { 1 } else { lane.columns.max(1) },
                all_day: event.all_day,
                background: event.is_background(),
                where_: event.location.clone(),
                note: event.description.clone(),
                name: event.summary.clone(),
                color: format!(
                    "#{:02x}{:02x}{:02x}",
                    subscription.color[0], subscription.color[1], subscription.color[2]
                ),
                calendar: subscription.name.clone(),
            })
            .collect();
    }
    days
}

#[cfg(test)]
mod tests;
