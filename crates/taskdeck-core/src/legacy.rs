//! Legacy import (core.md §15, §20).
//!
//! One standalone desktop folder, as bytes the caller read, becomes the
//! genesis `BoardData` and a report of counts, ids and kinds. The fifteen
//! steps of §15.3 run in the order written, and the first failing step ends
//! the import with its failure kind (CORE-IMP-001). This is the one place
//! where normalisation runs (CORE-IMP-008): ids assigned, plans migrated,
//! notes detabbed, today's defaults filled in.
//!
//! The files are read with private `serde` structs that mirror today's
//! (`tasks.rs` `Active`, `Session`, `Recurrence`; `archive.rs` `Archived`;
//! `subscriptions.rs` `Subscription`; `color.rs` `ColorScheme`): the same field
//! names, the same defaults, unknown fields ignored. A time is read as
//! `DateTime<FixedOffset>`, which accepts exactly what today's
//! `DateTime<Local>` reads, and becomes the instant it denotes.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::Deserialize;

use crate::board::{ArchiveRow, Block, BoardData, Home, Item, ItemStamps, Kind, Mark, Outcome, Recurrence, RowBlock, Stamp, Subscription};
use crate::time::Instant;

/// Today's files, read by the caller (core.md §15.2).
#[derive(Clone, PartialEq, Debug)]
pub struct LegacyFolder {
    pub read_at_startup: Option<Vec<u8>>,
    pub archived: Option<Vec<u8>>,
    pub notepad: Option<Vec<u8>>,
    pub subscriptions: Option<Vec<u8>>,
    pub colorschemes: Option<Vec<u8>>,
    pub config: LegacyConfig,
}

/// What the caller read from `userconfig.toml` (core.md §15.2).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LegacyConfig {
    pub coordinates: Option<[f64; 2]>,
    pub selected_colorscheme_id: Option<u32>,
}

/// The import report: counts, ids and kinds only (core.md §15.4).
///
/// A failed import's report holds the failure and nothing else: every count
/// is 0, every flag `false` and every list empty.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct ImportReport {
    pub items_task: u32,
    pub items_event: u32,
    pub items_routine: u32,
    pub blocks: u32,
    pub archive_rows: u32,
    pub unreadable_lines: u32,
    pub subscriptions: u32,
    pub notes_scalars: u32,
    pub palette_set: bool,
    pub home_set: bool,
    pub renumbered: Vec<Renumbered>,
    pub live_and_archived: Vec<u64>,
    pub warnings: Vec<ImportWarning>,
    pub failure: Option<ImportFailure>,
}

/// One renumbered id (core.md §15.3 step 9, §15.4).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Renumbered {
    pub list: IdList,
    pub old: u64,
    pub new: u64,
}

/// The list a renumbered id was in (core.md §15.4).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum IdList {
    Items,
    Archive,
    Subscriptions,
}

/// A warning of the import (core.md §15.4).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ImportWarning {
    PaletteUnreadable,
    HomeOutOfRange,
    NotesOverCap,
}

/// The step an import failed at (core.md §15.3, §15.4).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FailureKind {
    TooLarge,
    ItemsUnreadable,
    NotesUnreadable,
    SubscriptionsUnreadable,
    IdRange,
    InstantRange,
    FieldRange,
    Cap,
    Invariant,
}

/// A failed import and the ids it names (core.md §15.4).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ImportFailure {
    pub kind: FailureKind,
    pub ids: Vec<u64>,
}

/// The genesis board and the report (core.md §15.3).
///
/// A pure function of its bytes (CORE-IMP-002); it never panics
/// (CORE-GEN-007). The report holds no text from the board (CORE-IMP-003).
pub fn import(folder: &LegacyFolder) -> (Option<BoardData>, ImportReport) {
    match run(folder) {
        Ok((board, report)) => (Some(board), report),
        Err(failure) => (None, ImportReport { failure: Some(failure), ..ImportReport::default() }),
    }
}

// ---------------------------------------------------------------- limits (§15, §17)

/// A legacy input file: 256 MiB (core.md §15.2, §17).
const FILE_BYTES: usize = 268_435_456;
/// Today's temporary ids start here (`TEMPORARY_ID_FLOOR`, board.rs).
const TEMPORARY_FLOOR: u64 = 1 << 62;
/// 2^40: the first id with a high part (core.md §7.1).
const HIGH_ONE: u64 = 1 << 40;
const MAX_DEVICE_NO: u64 = 8191;
const NAME_BYTES: usize = 4096;
const NOTES_BYTES: usize = 1_048_576;
const URL_BYTES: usize = 8000;
const LINE_BYTES: usize = 65_536;
const NOTES_SCALARS: usize = 20_000;
const MAX_ITEMS: usize = 10_000;
const MAX_BLOCKS_PER_ITEM: usize = 1000;
const MAX_LIVE_BLOCKS: usize = 50_000;
const MAX_ARCHIVE: usize = 200_000;
const MAX_UNREADABLE: usize = 10_000;
const MAX_SUBSCRIPTIONS: usize = 20;
/// Today's `planner::DEFAULT_BLOCK_MINUTES` and `MIN_BLOCK_MINUTES`.
const DEFAULT_BLOCK_MINUTES: u32 = 30;
const MIN_BLOCK_MINUTES: u32 = 15;

// ---------------------------------------------------------------- today's shapes

type LocalTime = DateTime<FixedOffset>;

/// Today's `Session` (tasks.rs).
#[derive(Deserialize)]
struct LegacySession {
    start: LocalTime,
    minutes: u32,
}

fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or_default()
}

/// Today's `Recurrence` (tasks.rs), `anchor` defaulting to 1970-01-01.
#[derive(Deserialize, Clone, Copy)]
struct LegacyRecurrence {
    days: u8,
    #[serde(default = "epoch")]
    anchor: NaiveDate,
    start_minutes: i32,
    minutes: u32,
}

/// Today's `Active` (tasks.rs:169-237); the `main` build's is a subset.
#[derive(Deserialize)]
struct LegacyActive {
    #[serde(default)]
    id: u64,
    importance: Option<u8>,
    time_importance: Option<u8>,
    name: String,
    created: LocalTime,
    deadline: Option<LocalTime>,
    is_event: bool,
    #[serde(default)]
    sessions: Vec<LegacySession>,
    #[serde(default)]
    planned_start: Option<LocalTime>,
    #[serde(default)]
    duration_minutes: Option<u32>,
    #[serde(default)]
    recurrence: Option<LegacyRecurrence>,
}

/// Today's `Outcome` (archive.rs), `finished` by default.
#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum LegacyOutcome {
    #[default]
    Finished,
    Dropped,
}

/// Today's `Archived` (archive.rs:114-155); the `main` build's `InActive` is
/// a subset.
#[derive(Deserialize)]
struct LegacyArchived {
    #[serde(default)]
    id: u64,
    name: String,
    created: LocalTime,
    deadline: Option<LocalTime>,
    is_event: bool,
    importance: Option<u8>,
    #[serde(default)]
    time_importance: Option<u8>,
    #[serde(default)]
    sessions: Vec<LegacySession>,
    #[serde(default)]
    duration_minutes: Option<u32>,
    #[serde(default)]
    recurrence: Option<LegacyRecurrence>,
    #[serde(rename = "inactivated")]
    archived_at: LocalTime,
    #[serde(default)]
    outcome: LegacyOutcome,
}

/// Today's `Subscription` (subscriptions.rs:137-147).
#[derive(Deserialize)]
struct LegacySubscription {
    id: u64,
    name: String,
    url: String,
    color: [u8; 4],
    enabled: bool,
}

/// Today's `ColorScheme` (color.rs:11-16).
#[derive(Deserialize)]
struct LegacyScheme {
    #[serde(rename = "name")]
    _name: String,
    colors: [[u8; 4]; 6],
    #[serde(rename = "is_user_configurable")]
    _is_user_configurable: bool,
}

fn fail(kind: FailureKind, ids: impl IntoIterator<Item = u64>) -> ImportFailure {
    let ids: BTreeSet<u64> = ids.into_iter().collect();
    ImportFailure { kind, ids: ids.into_iter().collect() }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn kind_of(recurrence: bool, is_event: bool) -> Kind {
    if recurrence {
        Kind::Routine
    } else if is_event {
        Kind::Event
    } else {
        Kind::Task
    }
}

/// A device-minted id of step 9.4: high part 1 to 8191, low part at least 1.
fn device_minted(id: u64) -> bool {
    (1..=MAX_DEVICE_NO).contains(&(id >> 40)) && id & (HIGH_ONE - 1) != 0
}

/// The instant a legacy time denotes, every fraction digit kept; none
/// outside §5.1's range.
// Lead ruling, spec change 5, §15.3: a leap second (`:60`, which chrono reads) is
// no instant of §5.1, so it is `instant_range`.
fn instant(t: &LocalTime) -> Option<Instant> {
    Instant::from_parts(t.timestamp(), t.timestamp_subsec_nanos())
}

// ---------------------------------------------------------------- the steps

/// One live item between the steps.
struct Live {
    raw: LegacyActive,
}

/// The id state of step 9: the shared `next` and what it found.
struct Ids {
    next: u64,
    out_of_range: Vec<u64>,
    overflowed: Vec<u64>,
    renumbered: Vec<Renumbered>,
}

impl Ids {
    /// Gives `old` the next legacy id (step 9.1 to 9.3), or records that
    /// `next` reached 2^40.
    fn renumber(&mut self, list: IdList, old: u64) -> u64 {
        if self.next >= HIGH_ONE {
            self.overflowed.push(old);
            return old;
        }
        let new = self.next;
        self.next += 1;
        self.renumbered.push(Renumbered { list, old, new });
        new
    }

    /// Step 9.4 for a kept id.
    fn check_kept(&mut self, id: u64) {
        if (HIGH_ONE..TEMPORARY_FLOOR).contains(&id) && !device_minted(id) {
            self.out_of_range.push(id);
        }
    }
}

fn run(folder: &LegacyFolder) -> Result<(BoardData, ImportReport), ImportFailure> {
    // 1. Sizes, before anything is parsed.
    let inputs = [&folder.read_at_startup, &folder.archived, &folder.notepad, &folder.subscriptions, &folder.colorschemes];
    if inputs.iter().any(|b| b.as_ref().is_some_and(|b| b.len() > FILE_BYTES)) {
        return Err(fail(FailureKind::TooLarge, []));
    }

    // 2. Items.
    let mut items: Vec<Live> = match &folder.read_at_startup {
        None => Vec::new(),
        Some(b) => serde_json::from_slice::<Vec<LegacyActive>>(b)
            .map_err(|_| fail(FailureKind::ItemsUnreadable, []))?
            .into_iter()
            .map(|raw| Live { raw })
            .collect(),
    };

    // 3. Archive: lines split at every 0x0A, in file order.
    let mut rows: Vec<LegacyArchived> = Vec::new();
    let mut unreadable: Vec<Vec<u8>> = Vec::new();
    if let Some(b) = &folder.archived {
        for line in b.split(|&c| c == b'\n') {
            match std::str::from_utf8(line) {
                Err(_) => unreadable.push(line.to_vec()),
                Ok(text) if crate::compile::trim(text).is_empty() => {}
                Ok(text) => match serde_json::from_str::<LegacyArchived>(text) {
                    Ok(row) => rows.push(row),
                    Err(_) => unreadable.push(line.to_vec()),
                },
            }
        }
    }

    // 4. Notes: one JSON string, then detab.
    let notes = match &folder.notepad {
        None => String::new(),
        Some(b) => serde_json::from_slice::<String>(b).map_err(|_| fail(FailureKind::NotesUnreadable, []))?,
    };
    let notes = crate::compile::detab(&notes);

    // 5. Subscriptions.
    let subscriptions: Vec<LegacySubscription> = match &folder.subscriptions {
        None => Vec::new(),
        Some(b) if std::str::from_utf8(b).is_ok_and(|t| crate::compile::trim(t).is_empty()) => Vec::new(),
        Some(b) => serde_json::from_slice(b).map_err(|_| fail(FailureKind::SubscriptionsUnreadable, []))?,
    };

    let mut warnings = Vec::new();

    // 6. Palette: the scheme under the selected id (default 0).
    let selected = folder.config.selected_colorscheme_id.unwrap_or(0);
    let mut palette = [[0u8; 4]; 6];
    let mut palette_set = false;
    if let Some(b) = &folder.colorschemes {
        match serde_json::from_slice::<BTreeMap<u32, LegacyScheme>>(b) {
            Err(_) => warnings.push(ImportWarning::PaletteUnreadable),
            Ok(schemes) => {
                if let Some(scheme) = schemes.get(&selected) {
                    palette = scheme.colors;
                    palette_set = true;
                }
            }
        }
    }

    // 7. Home: hundredths of a degree, rounded half away from zero.
    let mut home = Home { lat: 0, lon: 0 };
    let mut home_set = false;
    if let Some([lat, lon]) = folder.config.coordinates {
        let (lat, lon) = ((lat * 100.0).round(), (lon * 100.0).round());
        if (-9000.0..=9000.0).contains(&lat) && (-18000.0..=18000.0).contains(&lon) {
            // Both are integers within the i32 range here, so `as` is exact.
            home = Home { lat: lat as i32, lon: lon as i32 };
            home_set = true;
        } else {
            warnings.push(ImportWarning::HomeOutOfRange);
        }
    }

    // 8. Plans: `migrate_legacy_plans` exactly as today.
    for item in &mut items {
        let raw = &mut item.raw;
        let Some(start) = raw.planned_start.take() else { continue };
        if raw.is_event || !raw.sessions.is_empty() {
            continue;
        }
        let minutes = raw.duration_minutes.unwrap_or(DEFAULT_BLOCK_MINUTES).max(MIN_BLOCK_MINUTES);
        raw.sessions.push(LegacySession { start, minutes });
    }

    // 9. Ids.
    let largest = items
        .iter()
        .map(|i| i.raw.id)
        .chain(rows.iter().map(|r| r.id))
        .chain(subscriptions.iter().map(|s| s.id))
        .filter(|&id| id < HIGH_ONE)
        .max()
        .unwrap_or(0);
    let mut ids = Ids { next: largest + 1, out_of_range: Vec::new(), overflowed: Vec::new(), renumbered: Vec::new() };
    let mut item_ids: Vec<u64> = Vec::with_capacity(items.len());
    {
        let mut seen = BTreeSet::new();
        for item in &items {
            let old = item.raw.id;
            let id = if old == 0 || seen.contains(&old) || old >= TEMPORARY_FLOOR {
                ids.renumber(IdList::Items, old)
            } else {
                ids.check_kept(old);
                old
            };
            seen.insert(id);
            item_ids.push(id);
        }
    }
    let mut row_ids: Vec<u64> = Vec::with_capacity(rows.len());
    for row in &rows {
        let id = if row.id >= TEMPORARY_FLOOR {
            ids.renumber(IdList::Archive, row.id)
        } else {
            ids.check_kept(row.id);
            row.id
        };
        row_ids.push(id);
    }
    let mut sub_ids: Vec<u64> = Vec::with_capacity(subscriptions.len());
    {
        let mut seen = BTreeSet::new();
        for sub in &subscriptions {
            let old = sub.id;
            let id = if old == 0 || seen.contains(&old) || old >= TEMPORARY_FLOOR {
                ids.renumber(IdList::Subscriptions, old)
            } else {
                ids.check_kept(old);
                old
            };
            seen.insert(id);
            sub_ids.push(id);
        }
    }
    // Lead ruling, spec change 5, §15.3 step 9.4: `id_range` lists the
    // out-of-range ids, ascending and distinct, else (only when a renumbering
    // would assign 2^40 or more) the ids left without a number.
    if !ids.out_of_range.is_empty() {
        return Err(fail(FailureKind::IdRange, ids.out_of_range));
    }
    if !ids.overflowed.is_empty() {
        return Err(fail(FailureKind::IdRange, ids.overflowed));
    }
    let live_and_archived: Vec<u64> = {
        let live: BTreeSet<u64> = item_ids.iter().copied().collect();
        let both: BTreeSet<u64> = row_ids.iter().copied().filter(|id| live.contains(id)).collect();
        both.into_iter().collect()
    };

    // 10. Kinds and sids: one counter, live items by ascending id, then rows
    // in file line order. 11. Instants, collected as the records are built.
    let mut order: Vec<(u64, Live)> = item_ids.into_iter().zip(items).collect();
    order.sort_by_key(|(id, _)| *id);
    let mut next_sid: u64 = 1;
    let mut bad_instants: Vec<u64> = Vec::new();
    let mut at = |t: &LocalTime, id: u64| -> Instant {
        instant(t).unwrap_or_else(|| {
            bad_instants.push(id);
            Instant::MIN
        })
    };
    let stamp = Stamp { seq: 1, device_no: 0 };
    let stamps = ItemStamps { estimate: stamp, levels: stamp, name: stamp, repeat: stamp, time: stamp };
    let mut board_items: Vec<Item> = Vec::with_capacity(order.len());
    for (id, Live { raw }) in order {
        let blocks = raw
            .sessions
            .iter()
            .map(|s| {
                let sid = next_sid;
                next_sid += 1;
                Block { sid, start: at(&s.start, id), minutes: s.minutes, stamp }
            })
            .collect();
        board_items.push(Item {
            id,
            kind: kind_of(raw.recurrence.is_some(), raw.is_event),
            created: at(&raw.created, id),
            deadline: raw.deadline.as_ref().map(|t| at(t, id)),
            name: raw.name,
            importance: raw.importance,
            time_importance: raw.time_importance,
            duration_minutes: raw.duration_minutes,
            recurrence: raw.recurrence.map(recurrence),
            blocks,
            stamps,
        });
    }
    let mut archive: Vec<ArchiveRow> = Vec::with_capacity(rows.len());
    for (id, raw) in row_ids.into_iter().zip(rows) {
        let blocks = raw
            .sessions
            .iter()
            .map(|s| {
                let sid = next_sid;
                next_sid += 1;
                RowBlock { sid, start: at(&s.start, id), minutes: s.minutes }
            })
            .collect();
        archive.push(ArchiveRow {
            id,
            kind: kind_of(raw.recurrence.is_some(), raw.is_event),
            created: at(&raw.created, id),
            deadline: raw.deadline.as_ref().map(|t| at(t, id)),
            archived_at: at(&raw.archived_at, id),
            name: raw.name,
            importance: raw.importance,
            time_importance: raw.time_importance,
            duration_minutes: raw.duration_minutes,
            recurrence: raw.recurrence.map(recurrence),
            blocks,
            outcome: match raw.outcome {
                LegacyOutcome::Finished => Outcome::Finished,
                LegacyOutcome::Dropped => Outcome::Dropped,
            },
        });
    }
    let last_sid = next_sid - 1;
    let subs: Vec<Subscription> = sub_ids
        .into_iter()
        .zip(subscriptions)
        .map(|(id, s)| Subscription { id, name: s.name, url: s.url, color: s.color, enabled: s.enabled })
        .collect();
    if !bad_instants.is_empty() {
        return Err(fail(FailureKind::InstantRange, bad_instants));
    }

    // 12. Fields, then the caps of §17.
    let mut bad_fields: Vec<u64> = Vec::new();
    for i in &board_items {
        if !fields_ok(&i.name, i.importance, i.time_importance, i.duration_minutes, i.recurrence.as_ref(), i.blocks.iter().map(|b| b.minutes)) {
            bad_fields.push(i.id);
        }
    }
    for r in &archive {
        if !fields_ok(&r.name, r.importance, r.time_importance, r.duration_minutes, r.recurrence.as_ref(), r.blocks.iter().map(|b| b.minutes)) {
            bad_fields.push(r.id);
        }
    }
    let mut by_url: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for s in &subs {
        by_url.entry(s.url.as_str()).or_default().push(s.id);
        if s.name.len() > NAME_BYTES || !(1..=URL_BYTES).contains(&s.url.len()) {
            bad_fields.push(s.id);
        }
    }
    bad_fields.extend(by_url.values().filter(|ids| ids.len() > 1).flatten());
    // Lead ruling, spec change 5, §15.3 step 12: more than 20 subscriptions is
    // `field_range`, listing the ids past the twentieth.
    bad_fields.extend(subs.iter().skip(MAX_SUBSCRIPTIONS).map(|s| s.id));
    let notes_long = notes.len() > NOTES_BYTES;
    if !bad_fields.is_empty() || notes_long {
        return Err(fail(FailureKind::FieldRange, bad_fields));
    }
    let crowded = board_items
        .iter()
        .filter(|i| i.blocks.len() > MAX_BLOCKS_PER_ITEM)
        .map(|i| i.id)
        .chain(archive.iter().filter(|r| r.blocks.len() > MAX_BLOCKS_PER_ITEM).map(|r| r.id));
    let crowded: Vec<u64> = crowded.collect();
    let live_blocks: usize = board_items.iter().map(|i| i.blocks.len()).sum();
    if !crowded.is_empty()
        || board_items.len() > MAX_ITEMS
        || live_blocks > MAX_LIVE_BLOCKS
        || archive.len() > MAX_ARCHIVE
        || unreadable.len() > MAX_UNREADABLE
        || unreadable.iter().any(|l| l.len() > LINE_BYTES)
    {
        return Err(fail(FailureKind::Cap, crowded));
    }

    // 13. Notes cap: kept, with a warning.
    let notes_scalars = notes.chars().count();
    if notes_scalars > NOTES_SCALARS {
        warnings.push(ImportWarning::NotesOverCap);
    }

    // 14. Assembly.
    let mut highest: BTreeMap<u64, u64> = BTreeMap::new();
    for id in board_items.iter().map(|i| i.id).chain(archive.iter().map(|r| r.id)).chain(subs.iter().map(|s| s.id)) {
        let low = highest.entry(id >> 40).or_insert(0);
        *low = (*low).max(id & (HIGH_ONE - 1));
    }
    let mut marks = Vec::new();
    let legacy = Mark { device_no: 0, item: highest.get(&0).copied().unwrap_or(0), block: last_sid };
    if (legacy.item, legacy.block) != (0, 0) {
        marks.push(legacy);
    }
    for (&h, &item) in highest.range(1..) {
        if item != 0 {
            // h is at most 8191 here (step 9.4).
            marks.push(Mark { device_no: u16::try_from(h).unwrap_or(u16::MAX), item, block: 0 });
        }
    }
    archive.sort_by(crate::board::archive_cmp);
    unreadable.sort();
    let mut subs = subs;
    subs.sort_by_key(|s| s.id);
    let report = ImportReport {
        items_task: count(board_items.iter().filter(|i| i.kind == Kind::Task).count()),
        items_event: count(board_items.iter().filter(|i| i.kind == Kind::Event).count()),
        items_routine: count(board_items.iter().filter(|i| i.kind == Kind::Routine).count()),
        blocks: count(live_blocks),
        archive_rows: count(archive.len()),
        unreadable_lines: count(unreadable.len()),
        subscriptions: count(subs.len()),
        notes_scalars: count(notes_scalars),
        palette_set,
        home_set,
        renumbered: ids.renumbered,
        live_and_archived,
        warnings,
        failure: None,
    };
    let board = BoardData {
        rules: 1,
        items: board_items,
        archive,
        unreadable,
        notes,
        notes_rev: 0,
        subscriptions: subs,
        palette,
        home,
        picture: None,
        marks,
    };

    // 15. Check, and the size of the canonical bytes.
    if board.check().is_err() {
        return Err(fail(FailureKind::Invariant, []));
    }
    if board.to_canonical_bytes().len() > crate::apply::SIZE_CAP {
        return Err(fail(FailureKind::Cap, []));
    }
    Ok((board, report))
}

fn recurrence(r: LegacyRecurrence) -> Recurrence {
    Recurrence { days: r.days, anchor: r.anchor, start_minutes: r.start_minutes, minutes: r.minutes }
}

/// Step 12 for one item or row: the schema of §6.2 to §6.6.
fn fields_ok(
    name: &str,
    importance: Option<u8>,
    time_importance: Option<u8>,
    duration: Option<u32>,
    recurrence: Option<&Recurrence>,
    mut block_minutes: impl Iterator<Item = u32>,
) -> bool {
    use chrono::Datelike;
    name.len() <= NAME_BYTES
        && importance.is_none_or(|x| x <= 4)
        && time_importance.is_none_or(|x| x <= 3)
        && duration.is_none_or(|x| x <= 1440)
        && recurrence.is_none_or(|r| {
            r.days <= 127 && r.minutes <= 1440 && (0..=1439).contains(&r.start_minutes) && (1970..=9999).contains(&r.anchor.year())
        })
        && block_minutes.all(|m| m <= 1440)
}

#[cfg(test)]
mod tests;
