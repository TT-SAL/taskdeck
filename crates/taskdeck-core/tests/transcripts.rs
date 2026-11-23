//! The transcript test (SYNC_PLAN.md §6.2; core.md §13.5, §18.4, §18.7):
//! every recorded transcript of today's server replayed through
//! `parse_intent`, `compile` and `apply`, starting from `import` of the
//! transcript's board folder, and compared with the recorded replies and
//! states while every step so far is classified "same" by §13.5.
//!
//! Per step: the recorded command's ids are translated through the id map
//! `M` (a recorded `session` index becomes the sid of that item's i-th block
//! in ascending sid); the text is parsed as an intent and compiled with the
//! step's `now` truncated to milliseconds, device 1, the replay zone and the
//! overlay of the recorded state before the step; the command is applied
//! with `Ctx { seq: k + 1, device_no: 1, at: now, based_on: k }` at step `k`.
//! The replay zone is the transcript's zone that reads a local time in a fold
//! as the later instant alone (§13.5). A step is "changed" only by a rule of
//! §13.5 or §18.7, each a function below that looks at the step and the state
//! before it; from a changed step on, the transcript is replayed without
//! comparing. The steps compared and not compared are asserted per
//! transcript, so that none can drop out silently.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{boards, transcripts, zones};
use serde_json::Value;
use taskdeck_core::CompileError;
use taskdeck_core::apply::{Ctx, apply};
use taskdeck_core::board::{ArchiveRow, BoardData, Item, Kind, Outcome};
use taskdeck_core::compile::{CompileCtx, compile, detab};
use taskdeck_core::ids::IdMinter;
use taskdeck_core::intent::{Intent, WireTime, parse_intent};
use taskdeck_core::legacy::{FailureKind, IdList, LegacyFolder, import};
use taskdeck_core::overlay::Overlay;
use taskdeck_core::planner::drop_length_for;
use taskdeck_core::read::reflow_inputs;
use taskdeck_core::time::{Instant, LocalTime, TableZone, Zone, clamp_block, resolve_on_day};

/// Today's server prefixes every parse failure with this (fixtures/README.md,
/// "Transcript files"); what follows is serde's wording and is not compared.
const UNREADABLE: &str = "Could not read the command: ";

/// 2^40: the first id with a high part (core.md §7.1).
const HIGH_ONE: u64 = 1 << 40;
/// Today's temporary ids start here (core.md §15.3 step 9.1).
const TEMPORARY_FLOOR: u64 = 1 << 62;

/// The ops whose `id` names a subscription (core.md §12.3).
const SUBSCRIPTION_OPS: [&str; 4] =
    ["remove_subscription", "rename_subscription", "set_subscription_color", "set_subscription_enabled"];

// ---------------------------------------------------------------- the replay zone

/// The transcript's zone, reading a local time in a fold as the later instant
/// alone: the recording took chrono's `.earliest()` of `Local`, which is the
/// later one (core.md §13.5).
struct ReplayZone(TableZone);

impl Zone for ReplayZone {
    fn offset_at(&self, utc: Instant) -> i32 {
        self.0.offset_at(utc)
    }
    fn from_local(&self, local: chrono::NaiveDateTime) -> LocalTime {
        match self.0.from_local(local) {
            LocalTime::Ambiguous(_, later) => LocalTime::Single(later),
            other => other,
        }
    }
}

// ---------------------------------------------------------------- the files

/// One step of a transcript (fixtures/README.md, "Transcript files").
struct Step {
    number: u64,
    now: Instant,
    wire: String,
    status: u16,
    body: Value,
    state: Value,
}

/// The start line and the steps of a transcript, each line parsed once.
fn parse_transcript(text: &str) -> (Value, Vec<Step>) {
    let mut lines = text.split('\n').map(|l| l.trim_end_matches('\r')).filter(|l| !l.is_empty());
    let start: Value = serde_json::from_str(lines.next().expect("a start line")).expect("JSON");
    assert_eq!(start["kind"], "start");
    let steps = lines
        .map(|line| {
            let v: Value = serde_json::from_str(line).expect("JSON");
            assert_eq!(v["kind"], "step");
            let now = instant(v["now"].as_str().expect("now"));
            Step {
                number: v["step"].as_u64().expect("step"),
                // core.md §13.5: the pinned clock, truncated to milliseconds.
                now: Instant::from_at_ms(now.as_unix_ms()).expect("in range"),
                wire: v["wire"].as_str().expect("wire").to_owned(),
                status: u16::try_from(v["status"].as_u64().expect("status")).expect("u16"),
                body: v["body"].clone(),
                state: v["state"].clone(),
            }
        })
        .collect();
    (start, steps)
}

/// A today's `DateTime<Local>` text as a UTC chrono time.
fn utc(text: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(text).unwrap_or_else(|e| panic!("RFC 3339 {text}: {e}")).to_utc()
}

/// A today's `DateTime<Local>` text as an instant of §5.1.
fn instant(text: &str) -> Instant {
    Instant::from_chrono(utc(text)).unwrap_or_else(|| panic!("{text} in range"))
}

// ---------------------------------------------------------------- the import (§18.7)

/// What §18.7 removed from a folder before importing it: item ids, and the
/// keys `(id, archived_at)` of archive rows.
#[derive(Default)]
struct Removed {
    items: BTreeSet<u64>,
    rows: BTreeSet<(u64, chrono::DateTime<chrono::Utc>)>,
}

/// The board a transcript starts from, or the refusal of its folder.
enum Start {
    Board(Box<BoardData>, Removed, IdMap),
    /// Refused for a reason other than `instant_range` (§18.7): not replayed.
    Refused,
}

/// Whether a today's `DateTime<Local>` text denotes an instant before
/// 1970-01-01T00:00:00Z.
fn holds_early_instant(v: &Value) -> bool {
    v.as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).is_some_and(|t| t.timestamp() < 0)
}

/// Whether an item or row holds an instant outside §5.1's range.
fn out_of_range(v: &Value) -> bool {
    ["created", "deadline", "planned_start", "inactivated"].iter().any(|k| holds_early_instant(&v[*k]))
        || v["sessions"].as_array().is_some_and(|s| s.iter().any(|s| holds_early_instant(&s["start"])))
}

/// The JSON Lines of an archive file that parse as JSON.
fn json_lines(archived: &[u8]) -> Vec<Value> {
    archived.split(|&c| c == b'\n').filter_map(|l| serde_json::from_slice::<Value>(l).ok()).collect()
}

/// Every id of a list that §15.3 step 9.4 names: from 2^40 to 2^62 − 1 with
/// a high part of 8192 or more or a low part of 0.
fn out_of_range_ids(list: &[Value]) -> Vec<u64> {
    list.iter()
        .filter_map(|v| v["id"].as_u64())
        .filter(|&id| (HIGH_ONE..TEMPORARY_FLOOR).contains(&id) && (id / HIGH_ONE > 8191 || id % HIGH_ONE == 0))
        .collect()
}

/// `import` of a board folder, treated as §18.7 states.
fn start_board(name: &str) -> Start {
    let mut folder: LegacyFolder = boards::board(name);
    let (board, report) = import(&folder);
    if let Some(board) = board {
        return Start::Board(Box::new(board), Removed::default(), IdMap::from_report(&report.renumbered));
    }
    let failure = report.failure.expect("a refusal names its failure");
    match failure.kind {
        FailureKind::InstantRange => {
            // Remove exactly the items and rows holding an instant outside
            // §5.1's range, check that their ids are the failure's, import
            // the repaired folder.
            let items: Vec<Value> =
                serde_json::from_slice(folder.read_at_startup.as_deref().expect("items")).expect("JSON");
            let archived = folder.archived.clone().unwrap_or_default();
            let rows = json_lines(&archived);
            let removed_items: BTreeSet<u64> =
                items.iter().filter(|v| out_of_range(v)).filter_map(|v| v["id"].as_u64()).collect();
            let removed_rows: BTreeSet<(u64, chrono::DateTime<chrono::Utc>)> = rows
                .iter()
                .filter(|v| out_of_range(v))
                .map(|v| (v["id"].as_u64().expect("id"), utc(v["inactivated"].as_str().expect("inactivated"))))
                .collect();
            let listed: BTreeSet<u64> = failure.ids.iter().copied().collect();
            let ids: BTreeSet<u64> = removed_items.iter().copied().chain(removed_rows.iter().map(|r| r.0)).collect();
            assert_eq!(ids, listed, "{name}: the removed records are the failure's");
            let kept: Vec<Value> =
                items.into_iter().filter(|v| !v["id"].as_u64().is_some_and(|id| removed_items.contains(&id))).collect();
            folder.read_at_startup = Some(serde_json::to_vec(&kept).expect("JSON"));
            let kept_rows: Vec<&[u8]> = archived
                .split(|&c| c == b'\n')
                .filter(|line| !serde_json::from_slice::<Value>(line).is_ok_and(|v| out_of_range(&v)))
                .collect();
            folder.archived = Some(kept_rows.join(&b'\n'));
            let (board, report) = import(&folder);
            let board = board.unwrap_or_else(|| panic!("{name}, repaired: {:?}", report.failure));
            let ids = IdMap::from_report(&report.renumbered);
            Start::Board(Box::new(board), Removed { items: removed_items, rows: removed_rows }, ids)
        }
        FailureKind::IdRange if name == "id-limits" => {
            // Not replayed (§18.7, "any other failure"); the failure lists
            // exactly the ids that §15.3 step 9.4 names.
            let mut expected = Vec::new();
            let items: Vec<Value> =
                serde_json::from_slice(folder.read_at_startup.as_deref().expect("items")).expect("JSON");
            expected.extend(out_of_range_ids(&items));
            expected.extend(out_of_range_ids(&json_lines(folder.archived.as_deref().unwrap_or_default())));
            if let Some(subs) = folder.subscriptions.as_deref() {
                let subs: Vec<Value> = serde_json::from_slice(subs).expect("JSON");
                expected.extend(out_of_range_ids(&subs));
            }
            expected.sort_unstable();
            expected.dedup();
            assert!(!expected.is_empty());
            let mut listed = failure.ids.clone();
            listed.sort_unstable();
            assert_eq!(listed, expected, "{name}: the ids of step 9.4");
            Start::Refused
        }
        other => panic!("{name} refused with {other:?}"),
    }
}

// ---------------------------------------------------------------- the id map (§18.4)

/// The id map `M` of core.md §18.4, initially the identity. Items and
/// archive rows share one map (a `restore` turns a row back into an item);
/// subscriptions have their own.
// SPEC-QUESTION: §18.4 names one map M, but today's item and subscription ids repeat across the two lists (`small`'s subscriptions 31–34 and the items 30–38 that every-command adds), so M is kept per list.
#[derive(Default)]
struct IdMap {
    items: BTreeMap<u64, u64>,
    subscriptions: BTreeMap<u64, u64>,
}

impl IdMap {
    /// The identity plus the import's renumberings (core.md §15.4).
    fn from_report(renumbered: &[taskdeck_core::legacy::Renumbered]) -> IdMap {
        let mut ids = IdMap::default();
        for r in renumbered {
            let map = match r.list {
                IdList::Items | IdList::Archive => &mut ids.items,
                IdList::Subscriptions => &mut ids.subscriptions,
            };
            assert!(map.insert(r.old, r.new).is_none_or(|n| n == r.new), "one renumbering per id");
        }
        ids
    }
    fn item(&self, recorded: u64) -> u64 {
        self.items.get(&recorded).copied().unwrap_or(recorded)
    }
    fn subscription(&self, recorded: u64) -> u64 {
        self.subscriptions.get(&recorded).copied().unwrap_or(recorded)
    }
}

/// The sids of an item's blocks, ascending.
fn sids(item: &Item) -> Vec<u64> {
    let mut sids: Vec<u64> = item.blocks.iter().map(|b| b.sid).collect();
    sids.sort_unstable();
    sids
}

/// The recorded command with its ids translated through `M`, and its
/// `session` index `i` as the sid of the item's `i`-th block in ascending
/// sid (core.md §18.4). A text that changes nothing is passed on as recorded.
fn translate(wire: &str, ids: &IdMap, board: &BoardData) -> String {
    let Ok(Value::Object(mut fields)) = serde_json::from_str::<Value>(wire) else {
        return wire.to_owned();
    };
    let op = fields.get("op").and_then(Value::as_str).unwrap_or_default().to_owned();
    let Some(recorded) = fields.get("id").and_then(Value::as_u64) else {
        return wire.to_owned();
    };
    let id = if SUBSCRIPTION_OPS.contains(&op.as_str()) { ids.subscription(recorded) } else { ids.item(recorded) };
    let mut changed = id != recorded;
    fields.insert("id".to_owned(), Value::from(id));
    if let Some(index) = fields.get("session").and_then(Value::as_u64) {
        // An index past the item's blocks (or of an item not there) is no
        // block: sid 0, which no block has (core.md §7.3).
        let sid = board
            .item(id)
            .and_then(|item| usize::try_from(index).ok().and_then(|i| sids(item).get(i).copied()))
            .unwrap_or(0);
        changed |= sid != index;
        fields.insert("session".to_owned(), Value::from(sid));
    }
    if changed { serde_json::to_string(&Value::Object(fields)).expect("JSON") } else { wire.to_owned() }
}

// ---------------------------------------------------------------- the projection (§18.4)

/// The compared fields of an item or row (core.md §18.4).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Fields {
    name: String,
    created: Instant,
    deadline: Option<Instant>,
    kind: &'static str,
    importance: Option<u8>,
    time_importance: Option<u8>,
    duration_minutes: Option<u32>,
    recurrence: Option<(u8, chrono::NaiveDate, i32, u32)>,
    blocks: Vec<(Instant, u32)>,
}

/// An archive row: the fields of an item plus `archived_at` and `outcome`.
/// The row's id is not among them (§18.4 lists the fields; the archive is a
/// multiset).
// SPEC-QUESTION: §18.4 compares "the archive as a multiset of rows with the same fields plus archived_at and outcome"; read as the item fields after "keyed by id", so a row's id is not compared.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Row {
    fields: Fields,
    archived_at: Instant,
    outcome: &'static str,
}

/// A subscription's compared fields: name, URL, colour, enabled.
type Sub = (String, String, [u8; 4], bool);

#[derive(PartialEq, Debug)]
struct Projection {
    items: BTreeMap<u64, Fields>,
    archive: Vec<Row>,
    notes: String,
    subscriptions: BTreeMap<u64, Sub>,
}

/// `created` truncated to milliseconds (core.md §18.4).
fn to_ms(t: Instant) -> Instant {
    Instant::from_at_ms(t.as_unix_ms()).expect("in range")
}

fn small(v: &Value) -> Option<u8> {
    v.as_u64().map(|n| u8::try_from(n).expect("u8"))
}

fn minutes(v: &Value) -> u32 {
    u32::try_from(v.as_u64().expect("minutes")).expect("u32")
}

/// The fields of a recorded item or row.
fn recorded_fields(v: &Value) -> Fields {
    let recurrence = (!v["recurrence"].is_null()).then(|| {
        let r = &v["recurrence"];
        (
            small(&r["days"]).expect("days"),
            chrono::NaiveDate::parse_from_str(r["anchor"].as_str().expect("anchor"), "%Y-%m-%d").expect("date"),
            i32::try_from(r["start_minutes"].as_i64().expect("start")).expect("i32"),
            minutes(&r["minutes"]),
        )
    });
    let kind = if recurrence.is_some() {
        "routine"
    } else if v["is_event"].as_bool().unwrap_or(false) {
        "event"
    } else {
        "task"
    };
    Fields {
        name: v["name"].as_str().expect("name").to_owned(),
        created: to_ms(instant(v["created"].as_str().expect("created"))),
        deadline: v["deadline"].as_str().map(instant),
        kind,
        importance: small(&v["importance"]),
        time_importance: small(&v["time_importance"]),
        duration_minutes: v["duration_minutes"].as_u64().map(|m| u32::try_from(m).expect("u32")),
        recurrence,
        blocks: v["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .map(|s| (instant(s["start"].as_str().expect("start")), minutes(&s["minutes"])))
            .collect(),
    }
}

/// A recorded state through the projection, ids translated through `M`, the
/// records §18.7 removed left out.
fn recorded(state: &Value, ids: &IdMap, removed: &Removed) -> Projection {
    let mut items = BTreeMap::new();
    for v in state["items"].as_array().expect("items") {
        let id = v["id"].as_u64().expect("id");
        if removed.items.contains(&id) {
            continue;
        }
        assert!(items.insert(ids.item(id), recorded_fields(v)).is_none(), "item {id} recorded once");
    }
    let mut archive: Vec<Row> = state["archive"]
        .as_array()
        .expect("archive")
        .iter()
        .filter(|v| {
            let key = (v["id"].as_u64().expect("id"), utc(v["inactivated"].as_str().expect("inactivated")));
            !removed.rows.contains(&key)
        })
        .map(|v| Row {
            fields: recorded_fields(v),
            archived_at: instant(v["inactivated"].as_str().expect("inactivated")),
            outcome: match v["outcome"].as_str().unwrap_or("finished") {
                "finished" => "finished",
                "dropped" => "dropped",
                other => panic!("outcome {other}"),
            },
        })
        .collect();
    archive.sort();
    let mut subscriptions = BTreeMap::new();
    for v in state["subscriptions"].as_array().expect("subscriptions") {
        let id = v["id"].as_u64().expect("id");
        let color: Vec<u8> = v["color"].as_array().expect("color").iter().filter_map(small).collect();
        let sub = (
            v["name"].as_str().expect("name").to_owned(),
            v["url"].as_str().expect("url").to_owned(),
            <[u8; 4]>::try_from(color).expect("four channels"),
            v["enabled"].as_bool().expect("enabled"),
        );
        assert!(subscriptions.insert(ids.subscription(id), sub).is_none(), "subscription {id} recorded once");
    }
    Projection { items, archive, notes: state["notes"].as_str().expect("notes").to_owned(), subscriptions }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Task => "task",
        Kind::Event => "event",
        Kind::Routine => "routine",
    }
}

/// The fields of a replayed archive row.
fn row_fields(r: &ArchiveRow) -> Fields {
    Fields {
        name: r.name.clone(),
        created: to_ms(r.created),
        deadline: r.deadline,
        kind: kind_name(r.kind),
        importance: r.importance,
        time_importance: r.time_importance,
        duration_minutes: r.duration_minutes,
        recurrence: r.recurrence.map(|x| (x.days, x.anchor, x.start_minutes, x.minutes)),
        blocks: r.blocks.iter().map(|b| (b.start, b.minutes)).collect(),
    }
}

/// The replayed board through the projection.
fn replayed(board: &BoardData) -> Projection {
    let items = board
        .items
        .iter()
        .map(|i| {
            let mut blocks: Vec<(u64, Instant, u32)> = i.blocks.iter().map(|b| (b.sid, b.start, b.minutes)).collect();
            blocks.sort_unstable();
            let fields = Fields {
                name: i.name.clone(),
                created: to_ms(i.created),
                deadline: i.deadline,
                kind: kind_name(i.kind),
                importance: i.importance,
                time_importance: i.time_importance,
                duration_minutes: i.duration_minutes,
                recurrence: i.recurrence.map(|r| (r.days, r.anchor, r.start_minutes, r.minutes)),
                blocks: blocks.into_iter().map(|(_, s, m)| (s, m)).collect(),
            };
            (i.id, fields)
        })
        .collect();
    let mut archive: Vec<Row> = board
        .archive
        .iter()
        .map(|r| Row {
            fields: row_fields(r),
            archived_at: r.archived_at,
            outcome: match r.outcome {
                Outcome::Finished => "finished",
                Outcome::Dropped => "dropped",
            },
        })
        .collect();
    archive.sort();
    let subscriptions =
        board.subscriptions.iter().map(|s| (s.id, (s.name.clone(), s.url.clone(), s.color, s.enabled))).collect();
    Projection { items, archive, notes: board.notes.clone(), subscriptions }
}

/// The first difference of two maps, as text.
fn map_difference<V: PartialEq + std::fmt::Debug>(what: &str, a: &BTreeMap<u64, V>, b: &BTreeMap<u64, V>) -> Option<String> {
    let keys: BTreeSet<&u64> = a.keys().chain(b.keys()).collect();
    keys.into_iter().find_map(|k| {
        let (x, y) = (a.get(k), b.get(k));
        (x != y).then(|| format!("{what} {k}: recorded {x:?}, replayed {y:?}"))
    })
}

/// The first difference of the recorded and the replayed projection, as text.
fn difference(rec: &Projection, rep: &Projection) -> Option<String> {
    if let Some(d) = map_difference("item", &rec.items, &rep.items) {
        return Some(d);
    }
    if rec.archive != rep.archive {
        let only_rec: Vec<&Row> = rec.archive.iter().filter(|r| !rep.archive.contains(r)).collect();
        let only_rep: Vec<&Row> = rep.archive.iter().filter(|r| !rec.archive.contains(r)).collect();
        return Some(format!(
            "archive ({} recorded, {} replayed): only recorded {only_rec:?}; only replayed {only_rep:?}",
            rec.archive.len(),
            rep.archive.len()
        ));
    }
    if rec.notes != rep.notes {
        return Some(format!("notes: recorded {} chars, replayed {} chars", rec.notes.chars().count(), rep.notes.chars().count()));
    }
    map_difference("subscription", &rec.subscriptions, &rep.subscriptions)
}

// ---------------------------------------------------------------- the rules of §13.5 and §18.7

/// What a rule looks at: the step, the intent parsed from its translated
/// text, the replayed board and the recorded state before the step.
struct Before<'a> {
    wire: &'a Value,
    intent: Option<&'a Intent>,
    board: &'a BoardData,
    state: &'a Value,
    overlay: &'a Overlay,
    ids: &'a IdMap,
    removed: &'a Removed,
    zone: &'a ReplayZone,
}

/// §18.7: a recorded command that names a removed item's id or a removed
/// row's key.
fn names_a_removed_record(b: &Before) -> bool {
    let op = b.wire["op"].as_str().unwrap_or_default();
    let Some(id) = b.wire["id"].as_u64() else { return false };
    match op {
        "restore" | "forget_archived" => b.wire["archived_at"]
            .as_str()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .is_some_and(|t| b.removed.rows.contains(&(id, t.to_utc()))),
        _ if SUBSCRIPTION_OPS.contains(&op) => false,
        _ => b.removed.items.contains(&id),
    }
}

/// §13.5, `create`: a routine whose `resolve_on_day(day, start)` is none in
/// the pinned zone (today 400, now created).
fn routine_created_in_a_gap(b: &Before) -> bool {
    let Some(Intent::Create { kind: Kind::Routine, day, start, minutes, .. }) = b.intent else { return false };
    let (start, _) = clamp_block(i64::from(*start), i64::from(*minutes));
    resolve_on_day(*day, start, b.zone).is_none()
}

/// §13.5, `move_block`: a routine whose `resolve_on_day(day, start)` is none
/// (today 400).
fn routine_moved_into_a_gap(b: &Before) -> bool {
    let Some(Intent::MoveBlock { id, day, start, minutes, .. }) = b.intent else { return false };
    if b.board.item(*id).is_none_or(|i| i.kind != Kind::Routine) {
        return false;
    }
    let (start, _) = clamp_block(i64::from(*start), i64::from(*minutes));
    resolve_on_day(*day, start, b.zone).is_none()
}

/// §13.5, `reflow`: two flowable blocks on the day have equal starts and
/// today's `Vec` order differs from ascending `(id, sid)` (ties). Today's
/// order is the items' order in the recorded state, each item's sessions in
/// index order, which is ascending sid (§18.4).
fn reflow_ties(b: &Before) -> bool {
    let Some(Intent::Reflow { day, .. }) = b.intent else { return false };
    let (flowables, _) = reflow_inputs(b.board, b.overlay, *day, b.zone);
    let position: BTreeMap<u64, usize> = b.state["items"]
        .as_array()
        .expect("items")
        .iter()
        .enumerate()
        .map(|(p, v)| (b.ids.item(v["id"].as_u64().expect("id")), p))
        .collect();
    let mut ours: Vec<(i32, (u64, u64))> = flowables.iter().map(|f| (f.start, f.key)).collect();
    let mut todays = ours.clone();
    todays.sort_by_key(|&(_, (id, _))| position.get(&id).copied().unwrap_or(usize::MAX));
    // Reflow orders by start, stably (planner::reflow).
    ours.sort_by_key(|&(start, _)| start);
    todays.sort_by_key(|&(start, _)| start);
    ours != todays
}

/// §13.5, `set_notes`: `chars(text) ≤ 20,000 < chars(detab(text))` (the cap
/// after detab).
fn notes_over_the_cap_after_detab(b: &Before) -> bool {
    let Some(Intent::SetNotes { text, .. }) = b.intent else { return false };
    text.chars().count() <= 20_000 && detab(text).chars().count() > 20_000
}

/// §13.5, `restore` and `forget_archived`: two rows share the key and
/// today's memory order (the recorded archive, first row with the key) and
/// the archive order (the board's first row with the key) pick different
/// rows.
fn archive_key_ties(b: &Before) -> bool {
    let (Some(Intent::Restore { id, archived_at }) | Some(Intent::ForgetArchived { id, archived_at })) = b.intent
    else {
        return false;
    };
    let at = archived_at.to_utc();
    let recorded_id = b.wire["id"].as_u64().expect("id");
    let todays: Vec<&Value> = b.state["archive"]
        .as_array()
        .expect("archive")
        .iter()
        .filter(|v| v["id"].as_u64() == Some(recorded_id) && utc(v["inactivated"].as_str().expect("inactivated")) == at)
        .collect();
    let ours: Vec<usize> = (0..b.board.archive.len())
        .filter(|&r| b.board.archive[r].id == *id && b.board.archive[r].archived_at.to_chrono() == at)
        .collect();
    if todays.len() < 2 && ours.len() < 2 {
        return false;
    }
    let today_picks = todays.first().map(|v| recorded_fields(v));
    let we_pick = ours.first().map(|&r| row_fields(&b.board.archive[r]));
    today_picks != we_pick
}

/// Whether a local year and a UTC year fall on different sides of 1970 or
/// 9999 (core.md §13.5, V2-4).
fn on_different_sides(local_year: i32, utc_year: i32) -> bool {
    (local_year < 1970) != (utc_year < 1970) || (local_year > 9999) != (utc_year > 9999)
}

/// The offset of the replay zone at a UTC time, also outside §5.1's range,
/// where both fixture zones keep their standard time.
fn offset_at(zone: &ReplayZone, t: chrono::DateTime<chrono::Utc>) -> i32 {
    Instant::from_chrono(t).map_or(zone.0.initial, |t| zone.offset_at(t))
}

/// Whether an exact time's local and UTC years are on different sides.
fn exact_crosses(zone: &ReplayZone, t: &chrono::DateTime<chrono::FixedOffset>) -> bool {
    use chrono::Datelike;
    let t = t.to_utc();
    let local = t.naive_utc() + chrono::TimeDelta::seconds(i64::from(offset_at(zone, t)));
    on_different_sides(local.year(), t.year())
}

/// Whether a local time on a day, resolved as the compile step resolves it,
/// gives a UTC year on the other side of 1970 or 9999 from the local one.
fn local_crosses(zone: &ReplayZone, day: chrono::NaiveDate, minutes: i32) -> bool {
    use chrono::Datelike;
    let utc_year = match resolve_on_day(day, minutes, zone) {
        Some(t) => t.to_chrono().year(),
        None => {
            // Outside the instant range: the local time minus the zone's
            // standard offset, which both fixture zones keep there.
            let m = minutes.clamp(0, 1439);
            let hour = u32::try_from(m / 60).expect("hour");
            let minute = u32::try_from(m % 60).expect("minute");
            let local = day.and_hms_opt(hour, minute, 0).expect("a time");
            (local - chrono::TimeDelta::seconds(i64::from(zone.0.initial))).year()
        }
    };
    on_different_sides(day.year(), utc_year)
}

fn wire_crosses(zone: &ReplayZone, t: Option<&WireTime>) -> bool {
    use chrono::Timelike;
    match t {
        None => false,
        Some(WireTime::Exact(t)) => exact_crosses(zone, t),
        Some(WireTime::Naive(n, _)) => {
            let minutes = i32::try_from(n.hour() * 60 + n.minute()).expect("minutes");
            local_crosses(zone, n.date(), minutes)
        }
    }
}

/// §13.5, under the table: a recorded command that carries or resolves an
/// instant whose local year and UTC year fall on different sides of 1970 or
/// 9999 (V2-4).
fn year_sides_differ(b: &Before) -> bool {
    let Some(intent) = b.intent else { return false };
    let z = b.zone;
    let item = |id: &u64| b.board.item(*id);
    match intent {
        Intent::Add { deadline, .. } | Intent::QuickAdd { deadline, .. } | Intent::SetDeadline { deadline, .. } => {
            wire_crosses(z, deadline.as_ref())
        }
        Intent::Complete { at, .. } | Intent::Delete { at, .. } => wire_crosses(z, at.as_ref()),
        Intent::Restore { archived_at, .. } | Intent::ForgetArchived { archived_at, .. } => {
            exact_crosses(z, archived_at)
        }
        Intent::Create { kind, day, start, minutes, .. } if *kind != Kind::Routine => {
            let (start, _) = clamp_block(i64::from(*start), i64::from(*minutes));
            local_crosses(z, *day, start)
        }
        Intent::MoveBlock { id, day, start, minutes, .. } => item(id).is_some_and(|i| {
            let (start, _) = clamp_block(i64::from(*start), i64::from(*minutes));
            i.kind != Kind::Routine && local_crosses(z, *day, start)
        }),
        Intent::AddBlock { id, day, start, minutes } => item(id).is_some_and(|i| {
            let length = minutes.unwrap_or_else(|| drop_length_for(i));
            let (start, _) = clamp_block(i64::from(*start), i64::from(length));
            local_crosses(z, *day, start)
        }),
        _ => false,
    }
}

/// A rule of §13.5 or §18.7: whether it classifies the step changed.
type Rule = fn(&Before) -> bool;

/// The rules, each with the name the counts table reports.
const RULES: [(&str, Rule); 7] = [
    ("§18.7 removed record", names_a_removed_record),
    ("§13.5 create: routine in a gap", routine_created_in_a_gap),
    ("§13.5 move_block: routine in a gap", routine_moved_into_a_gap),
    ("§13.5 reflow: ties", reflow_ties),
    ("§13.5 set_notes: cap after detab", notes_over_the_cap_after_detab),
    ("§13.5 restore: key ties", archive_key_ties),
    ("§13.5 year sides", year_sides_differ),
];

// ---------------------------------------------------------------- the replay

/// What the replay of one transcript found.
struct Replay {
    compared: u32,
    not_compared: u32,
    /// The first step a rule classified changed, and the rule; step 0 for a
    /// folder §18.7 does not replay.
    first_changed: Option<(u64, &'static str)>,
    /// Each difference with its step (0 for the start state).
    failures: Vec<(u64, String)>,
}

/// A recorded refusal against the compile step's error: the status and the
/// message character for character, or, for an unparseable body, the status,
/// the prefix and the code `core/intent_json` (fixtures/README.md).
fn refusal_difference(step: &Step, e: &CompileError) -> Option<String> {
    let recorded = step.body["error"].as_str().unwrap_or_default();
    let same = if step.status == 200 {
        false
    } else if recorded.starts_with(UNREADABLE) {
        e.code == "core/intent_json" && e.status == step.status
    } else {
        e.status == step.status && e.message == recorded
    };
    (!same).then(|| format!("recorded {} {}, compiled {e:?}", step.status, step.body))
}

/// A recorded 200 against a compiled command or a "nothing to do" reply:
/// the reply carries an id exactly when the recorded one does, and the same
/// `moved`.
fn reply_difference(step: &Step, compiled: &taskdeck_core::compile::Compiled) -> Option<String> {
    let reply = &compiled.reply;
    let same = step.status == 200
        && step.body["id"].is_u64() == reply.id.is_some()
        && step.body["moved"].as_u64() == reply.moved.map(u64::from);
    (!same).then(|| format!("recorded {} {}, compiled {reply:?}", step.status, step.body))
}

/// Replays one transcript (core.md §13.5, §18.4).
fn replay(zone_dir: &str, name: &str, text: &str) -> Replay {
    let (start, steps) = parse_transcript(text);
    assert_eq!(start["transcript"], name);
    let count = u32::try_from(steps.len()).expect("u32");
    for (k, step) in (1..).zip(&steps) {
        assert_eq!(step.number, k, "{zone_dir}/{name}: steps numbered from 1");
    }
    let zone = ReplayZone(zones::of_directory(zone_dir));
    let Start::Board(board, removed, mut ids) = start_board(start["board"].as_str().expect("board")) else {
        return Replay { compared: 0, not_compared: count, first_changed: Some((0, "§18.7 refused folder")), failures: vec![] };
    };
    let mut board = *board;
    let mut out = Replay { compared: 0, not_compared: 0, first_changed: None, failures: vec![] };
    if let Some(d) = difference(&recorded(&start["state"], &ids, &removed), &replayed(&board)) {
        out.failures.push((0, format!("{zone_dir}/{name} start: {d}")));
    }
    let mut minter = IdMinter::new(1);
    let mut before = &start["state"];
    for step in &steps {
        let k = step.number;
        let overlay: Overlay = serde_json::from_value(before["overlay"].clone()).expect("the recorded overlay");
        let wire: Value = serde_json::from_str(&step.wire).unwrap_or(Value::Null);
        let parsed = parse_intent(&translate(&step.wire, &ids, &board));
        if out.first_changed.is_none() {
            let b = Before {
                wire: &wire,
                intent: parsed.as_ref().ok(),
                board: &board,
                state: before,
                overlay: &overlay,
                ids: &ids,
                removed: &removed,
                zone: &zone,
            };
            out.first_changed = RULES.iter().find(|(_, rule)| rule(&b)).map(|(rule, _)| (k, *rule));
        }
        let comparing = out.first_changed.is_none();
        let cx = CompileCtx { view: &board, zone: &zone, now: step.now, device_no: 1, overlay: &overlay };
        let compiled = parsed.and_then(|intent| compile(&intent, &cx, &mut minter));
        let mut problems = Vec::new();
        match compiled {
            Err(e) => problems.extend(refusal_difference(step, &e)),
            Ok(c) => {
                problems.extend(reply_difference(step, &c));
                if let (Some(r), Some(id)) = (step.body["id"].as_u64(), c.reply.id) {
                    let map = if wire["op"] == "add_subscription" { &mut ids.subscriptions } else { &mut ids.items };
                    map.insert(r, id);
                }
                if let Some(command) = &c.command {
                    let ctx = Ctx { seq: k + 1, device_no: 1, at: step.now, based_on: k };
                    if let Err(refusal) = apply(&mut board, command, &ctx) {
                        problems.push(format!("apply refused {command:?}: {refusal:?}"));
                    }
                }
            }
        }
        if comparing {
            out.compared += 1;
            problems.extend(difference(&recorded(&step.state, &ids, &removed), &replayed(&board)));
            out.failures.extend(problems.into_iter().map(|p| (k, format!("{zone_dir}/{name} step {k} {}: {p}", step.wire))));
        } else {
            out.not_compared += 1;
        }
        before = &step.state;
    }
    out
}

/// Per transcript: the steps compared, the steps not compared, and the first
/// changed step with its rule.
type Expected = (&'static str, &'static str, u32, u32, Option<(u64, &'static str)>);

const EXPECTED: [Expected; 20] = [
    ("america-new-york", "dst-edges", 14, 3, Some((15, "§13.5 year sides"))),
    ("america-new-york", "every-command", 51, 0, None),
    ("america-new-york", "id-limits", 0, 6, Some((0, "§18.7 refused folder"))),
    ("america-new-york", "refusals", 19, 55, Some((20, "§13.5 year sides"))),
    ("america-new-york", "seeded-01", 30, 0, None),
    ("america-new-york", "seeded-02", 30, 0, None),
    ("america-new-york", "seeded-03", 30, 0, None),
    ("america-new-york", "seeded-04", 30, 0, None),
    ("america-new-york", "seeded-05", 20, 0, None),
    ("america-new-york", "seeded-06", 20, 0, None),
    ("europe-berlin", "dst-edges", 13, 4, Some((14, "§13.5 year sides"))),
    ("europe-berlin", "every-command", 51, 0, None),
    ("europe-berlin", "id-limits", 0, 6, Some((0, "§18.7 refused folder"))),
    ("europe-berlin", "refusals", 73, 1, Some((74, "§13.5 set_notes: cap after detab"))),
    ("europe-berlin", "seeded-01", 30, 0, None),
    ("europe-berlin", "seeded-02", 30, 0, None),
    ("europe-berlin", "seeded-03", 30, 0, None),
    ("europe-berlin", "seeded-04", 30, 0, None),
    ("europe-berlin", "seeded-05", 20, 0, None),
    ("europe-berlin", "seeded-06", 20, 0, None),
];

/// Steps that differ from the recording although no rule of §13.5 or §18.7
/// covers them, and the difference looks intended by the spec: left failing
/// for the lead to rule on (brief f1, settled point 1), and listed exactly,
/// so that neither a new difference nor a fix goes unseen.
///
/// `refusals` step 10, `{"op":"add","name":"x","deadline":"1969-12-31T12:00"}`:
/// today answers `400 Could not read the command: not a time: …`; the core
/// answers `core/date_range` (400, "That date is out of range (1970–9999).")
/// because §13.3 G3 makes a naive local time outside the range (§5.4)
/// `core/date_range`, while §13.5 classifies no such step changed (its year
/// rule needs the local and the UTC year on different sides; both are 1969).
const AWAITING_THE_LEAD: [(&str, &str, u64); 2] =
    [("america-new-york", "refusals", 10), ("europe-berlin", "refusals", 10)];

// Every transcript replayed from the import of its board folder: each step's
// outcome, reply id and `moved`, and the state after it compared with the
// recording through §18.4's projection while every step so far is "same";
// the counts of compared and not compared steps are fixed per transcript.
// spec: CORE-COMP-001
// spec: CORE-COMP-003
// spec: CORE-COMP-005
// spec: CORE-COMP-006
// spec: CORE-INT-001
// spec: CORE-IMP-005
// spec: CORE-CMD-004
// spec: CORE-READ-001
#[test]
fn every_transcript_replays_as_recorded() {
    let mut failures = Vec::new();
    let mut awaiting = Vec::new();
    let mut counts = Vec::new();
    for (zone_dir, name, text) in transcripts::FILES {
        let r = replay(zone_dir, name, text);
        for (step, failure) in r.failures {
            if AWAITING_THE_LEAD.contains(&(zone_dir, name, step)) {
                awaiting.push((zone_dir, name, step));
            } else {
                failures.push(failure);
            }
        }
        counts.push((zone_dir, name, r.compared, r.not_compared, r.first_changed));
    }
    let table: Vec<String> = counts.iter().map(|c| format!("{c:?},")).collect();
    assert!(failures.is_empty(), "{} differences:\n{}\ncounts:\n{}", failures.len(), failures.join("\n"), table.join("\n"));
    assert_eq!(awaiting, AWAITING_THE_LEAD, "the steps awaiting the lead differ, and differ only there");
    assert_eq!(counts, EXPECTED, "counts:\n{}", table.join("\n"));
}

// The rules that classify a step changed fire on their own cases and not on
// their neighbours': most of them never fire in the fixtures, where an
// earlier changed step or no such command comes first.
// spec: CORE-COMP-003
#[test]
fn the_rules_fire_on_their_cases() {
    let berlin = ReplayZone(zones::berlin());
    let new_york = ReplayZone(zones::new_york());
    let board = BoardData::empty();
    let state = serde_json::json!({"items": [], "archive": []});
    let overlay = Overlay::default();
    let ids = IdMap::default();
    let removed = Removed { items: BTreeSet::from([21]), rows: BTreeSet::new() };
    let fires = |rule: Rule, wire: &str, zone: &ReplayZone| {
        let parsed = parse_intent(wire);
        let wire: Value = serde_json::from_str(wire).expect("JSON");
        let b = Before {
            wire: &wire,
            intent: parsed.as_ref().ok(),
            board: &board,
            state: &state,
            overlay: &overlay,
            ids: &ids,
            removed: &removed,
            zone,
        };
        rule(&b)
    };
    // §18.7: an item op naming a removed id.
    assert!(fires(names_a_removed_record, r#"{"op":"rename","id":21,"name":"x"}"#, &berlin));
    assert!(!fires(names_a_removed_record, r#"{"op":"rename","id":22,"name":"x"}"#, &berlin));
    assert!(!fires(names_a_removed_record, r#"{"op":"remove_subscription","id":21}"#, &berlin));
    // `create` of a routine at a local time outside the range: no instant.
    let routine = r#"{"op":"create","kind":"routine","name":"r","day":"1970-01-01","start":0,"minutes":30}"#;
    assert!(fires(routine_created_in_a_gap, routine, &berlin));
    assert!(!fires(routine_created_in_a_gap, routine, &new_york));
    let task = r#"{"op":"create","kind":"task","name":"t","day":"1970-01-01","start":0,"minutes":30}"#;
    assert!(!fires(routine_created_in_a_gap, task, &berlin));
    // The year rule: 00:30 on 1970-01-01 in Berlin is 1969 in UTC.
    assert!(fires(year_sides_differ, task, &berlin));
    assert!(!fires(year_sides_differ, task, &new_york));
    let exact = r#"{"op":"add","name":"x","deadline":"1970-01-01T00:30:00+01:00"}"#;
    assert!(fires(year_sides_differ, exact, &berlin));
    assert!(!fires(year_sides_differ, exact, &new_york));
    let late = r#"{"op":"complete","id":1,"at":"9999-12-31T23:30:00-05:00"}"#;
    assert!(fires(year_sides_differ, late, &new_york));
    assert!(!fires(year_sides_differ, late, &berlin));
    let naive = r#"{"op":"quick_add","name":"x","deadline":"1969-12-31T12:00"}"#;
    assert!(!fires(year_sides_differ, naive, &berlin));
    // `set_notes`: within the cap before detab, over it after.
    let notes = |text: String| format!(r#"{{"op":"set_notes","text":{}}}"#, Value::String(text));
    let tabbed = notes(format!("{}\t", "x".repeat(19_999)));
    assert!(fires(notes_over_the_cap_after_detab, &tabbed, &berlin));
    assert!(!fires(notes_over_the_cap_after_detab, &notes("x".repeat(20_000)), &berlin));
    assert!(!fires(notes_over_the_cap_after_detab, &notes(format!("{}\t", "x".repeat(20_000))), &berlin));
}
