//! `Ctx`, `apply`, conflict rules and rules versions (core.md §8, §10, §11, §20).
//!
//! `apply` runs the five steps of §8.4: the context (`core/bad_ctx`), the op
//! table of `board.rules` (`core/rules`), the command's checks, the size check
//! (`core/cap`, CORE-APPLY-005) and the command's effects under that rules
//! version. Every check of a command runs before its first change to the
//! board, so a refusal leaves the board as it was (CORE-APPLY-002). Rules version 1 is `apply_v1`; a later version sits
//! beside it and version 1's code never changes (§11.1).
//!
//! Nothing here reads a clock, a zone, a float or a hash-map order
//! (CORE-GEN-006): the only instants are `ctx.at` and the command's.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::CoreError;
use crate::board::{ArchiveKey, ArchiveRow, Block, BoardData, Item, ItemStamps, Kind, Outcome, Recurrence, RowBlock, Stamp};
use crate::canon::{self, Obj, schema};
use crate::command::{CreateEvent, CreateRoutine, CreateTask, LogCommand};
use crate::ids;
use crate::time::Instant;

/// The context of one `apply` (core.md §8.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Ctx {
    pub seq: u64,
    pub device_no: u16,
    pub at: Instant,
    pub based_on: u64,
}

/// The result of an accepted command (core.md §8.3).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Applied {
    pub benign: bool,
    pub changes: ChangeSet,
    pub notices: Vec<Notice>,
    pub moved: u32,
    pub skipped: u32,
    pub restored_as: Option<u64>,
}

/// What an accepted command changed (core.md §8.3).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ChangeSet {
    pub items: BTreeSet<u64>,
    pub archive_inserted: Vec<ArchiveKey>,
    pub archive_removed: Vec<ArchiveKey>,
    pub notes: bool,
    pub subscriptions: BTreeSet<u64>,
    pub overlay_forget: BTreeSet<u64>,
    pub settings: bool,
    pub marks: bool,
}

/// A refused command (core.md §8.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Refusal {
    pub code: &'static str,
    pub op: &'static str,
    pub target: Option<u64>,
    pub notes: Option<NotesConflict>,
}

/// A `set_notes` whose base is not current (core.md §8.3, §10.5).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NotesConflict {
    pub base: u64,
    pub current: u64,
}

/// The field group of an overwrite notice (core.md §10.2, §10.7).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Group {
    Name,
    Time,
    Levels,
    Estimate,
    Repeat,
    Block,
    Item,
}

/// A notice record (core.md §10.7).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Notice {
    Overwrite { seq: u64, author: u16, other: u16, other_seq: u64, item: u64, sid: Option<u64>, group: Group },
    Skipped { seq: u64, author: u16, item: u64, sid: u64, reason: SkipReason, other: Option<(u16, u64)> },
}

/// Why a `move_blocks` move was skipped (core.md §10.3).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SkipReason {
    Changed,
    Gone,
}

/// A refusal record (core.md §10.7).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct RefusalRecord {
    pub seq: u64,
    pub author: u16,
    pub code: String,
    pub op: Option<String>,
    pub target: Option<u64>,
    pub base: Option<u64>,
    pub current: Option<u64>,
}

// ---------------------------------------------------------------- codes and limits (§16, §17)

const BAD_CTX: &str = "core/bad_ctx";
const RULES: &str = "core/rules";
const GONE: &str = "core/gone";
const KIND: &str = "core/kind";
const BAD_ID: &str = "core/bad_id";
const ID_TAKEN: &str = "core/id_taken";
const CAP: &str = "core/cap";
const DUPLICATE_URL: &str = "core/duplicate_url";
const NOTES_STALE: &str = "core/notes_stale";
const UNKNOWN_RULES: &str = "core/unknown_rules";

const MAX_U53: u64 = (1 << 53) - 1;
const MAX_DEVICE_NO: u16 = 8191;
const MAX_ITEMS: usize = 10_000;
const MAX_BLOCKS_PER_ITEM: usize = 1000;
const MAX_LIVE_BLOCKS: usize = 50_000;
const MAX_ARCHIVE: usize = 200_000;
const MAX_SUBSCRIPTIONS: usize = 20;
/// The archive clamp: `archived_at` is at most a day after `ctx.at` (core.md §9.8, §17).
const ARCHIVE_CLAMP_S: i64 = 86_400;

/// The rules versions this build applies (core.md §11.1).
pub const SUPPORTED_RULES: &[u32] = &[1];

/// The state-hash versions this build computes (core.md §6.13, §11).
pub const HASH_VERSIONS: &[u32] = &[1];

/// The op table of rules version 1: the 29 ops of core.md §9.3 (§11.1).
const OPS_V1: [&str; 29] = [
    "create_task",
    "create_event",
    "create_routine",
    "rename",
    "complete",
    "delete",
    "forget",
    "move_block",
    "move_blocks",
    "add_block",
    "remove_blocks",
    "set_estimate",
    "set_deadline",
    "set_severity",
    "set_horizon",
    "set_repeat",
    "move_routine",
    "set_event_span",
    "set_notes",
    "restore",
    "forget_archived",
    "add_subscription",
    "remove_subscription",
    "rename_subscription",
    "set_subscription_color",
    "set_subscription_enabled",
    "set_palette",
    "set_home",
    "set_picture",
];

/// The op table of a rules version; empty for a version this build does not
/// know (core.md §11.1, CORE-RULES-001).
fn op_table(rules: u32) -> &'static [&'static str] {
    match rules {
        1 => &OPS_V1,
        _ => &[],
    }
}

// ---------------------------------------------------------------- records (§10.7)

fn group_text(g: Group) -> &'static str {
    match g {
        Group::Name => "name",
        Group::Time => "time",
        Group::Levels => "levels",
        Group::Estimate => "estimate",
        Group::Repeat => "repeat",
        Group::Block => "block",
        Group::Item => "item",
    }
}

fn group_from(s: &str) -> Result<Group, CoreError> {
    Ok(match s {
        "name" => Group::Name,
        "time" => Group::Time,
        "levels" => Group::Levels,
        "estimate" => Group::Estimate,
        "repeat" => Group::Repeat,
        "block" => Group::Block,
        "item" => Group::Item,
        _ => return Err(schema()),
    })
}

fn put(m: &mut Map<String, Value>, key: &str, v: Value) {
    m.insert(key.to_owned(), v);
}

/// An `int[1..8191]` device number (core.md §10.7).
fn device(v: &Value) -> Result<u16, CoreError> {
    u16::try_from(canon::int(v, 1, i64::from(MAX_DEVICE_NO))?).map_err(|_| schema())
}

/// The byte limit of a notice or refusal record: 4,096 (core.md §10.7, §17).
pub(crate) const RECORD_LIMIT: usize = 4096;

/// §4.5 steps 1 to 3 here (the 4,096-byte limit of §10.7 first), 4 in
/// `read`, 5 by encoding the result once and comparing (CORE-CONF-006).
fn record_from<T>(b: &[u8], read: impl Fn(&Value) -> Result<T, CoreError>, enc: impl Fn(&T) -> Vec<u8>) -> Result<T, CoreError> {
    let value = canon::parse_value(b, RECORD_LIMIT)?;
    let record = read(&value)?;
    drop(value);
    if enc(&record) != b {
        return Err(CoreError::code(canon::NOT_CANONICAL));
    }
    Ok(record)
}

impl Notice {
    /// The notice's canonical bytes (core.md §10.7).
    pub fn to_canonical(&self) -> Vec<u8> {
        let mut m = Map::new();
        match *self {
            Notice::Overwrite { seq, author, other, other_seq, item, sid, group } => {
                put(&mut m, "author", Value::from(author));
                put(&mut m, "group", Value::from(group_text(group)));
                put(&mut m, "item", Value::from(item));
                put(&mut m, "kind", Value::from("overwrite"));
                put(&mut m, "other", Value::from(other));
                put(&mut m, "other_seq", Value::from(other_seq));
                put(&mut m, "seq", Value::from(seq));
                if let Some(sid) = sid {
                    put(&mut m, "sid", Value::from(sid));
                }
            }
            Notice::Skipped { seq, author, item, sid, reason, other } => {
                put(&mut m, "author", Value::from(author));
                put(&mut m, "group", Value::from("block"));
                put(&mut m, "item", Value::from(item));
                put(&mut m, "kind", Value::from("skipped"));
                if let Some((d, s)) = other {
                    put(&mut m, "other", Value::from(d));
                    put(&mut m, "other_seq", Value::from(s));
                }
                let reason = match reason {
                    SkipReason::Changed => "changed",
                    SkipReason::Gone => "gone",
                };
                put(&mut m, "reason", Value::from(reason));
                put(&mut m, "seq", Value::from(seq));
                put(&mut m, "sid", Value::from(sid));
            }
        }
        // The value holds no null and no float, so `encode` cannot fail.
        canon::encode(&Value::Object(m)).unwrap_or_default()
    }

    /// A notice from canonical bytes (core.md §10.7).
    pub fn from_canonical(b: &[u8]) -> Result<Self, CoreError> {
        record_from(b, Notice::from_value, Notice::to_canonical)
    }

    fn from_value(v: &Value) -> Result<Notice, CoreError> {
        let mut o = Obj::new(v)?;
        let author = device(o.req("author")?)?;
        let group = group_from(canon::string(o.req("group")?)?)?;
        let item = canon::u53(o.req("item")?)?;
        let kind = canon::string(o.req("kind")?)?;
        let other = o.opt("other").map(device).transpose()?;
        let other_seq = o.opt("other_seq").map(canon::u53).transpose()?;
        let reason = o.opt("reason").map(canon::string).transpose()?;
        let seq = canon::u53(o.req("seq")?)?;
        let sid = o.opt("sid").map(canon::u53).transpose()?;
        o.done()?;
        // `sid` exactly when `group` is `block`; `other_seq` exactly when `other`.
        if sid.is_some() != (group == Group::Block) {
            return Err(schema());
        }
        let other = match (other, other_seq) {
            (Some(d), Some(s)) => Some((d, s)),
            (None, None) => None,
            _ => return Err(schema()),
        };
        match (kind, reason) {
            ("overwrite", None) => match other {
                Some((other, other_seq)) => Ok(Notice::Overwrite { seq, author, other, other_seq, item, sid, group }),
                None => Err(schema()),
            },
            ("skipped", Some(r)) => {
                let reason = match (r, other.is_some()) {
                    ("changed", true) => SkipReason::Changed,
                    ("gone", false) => SkipReason::Gone,
                    _ => return Err(schema()),
                };
                // A skip names a block (core.md §10.3), so its group is `block`.
                match sid {
                    Some(sid) => Ok(Notice::Skipped { seq, author, item, sid, reason, other }),
                    None => Err(schema()),
                }
            }
            _ => Err(schema()),
        }
    }

    /// Whether the notice concerns a device (core.md §10.7).
    ///
    /// An overwrite concerns its author and the `other` device; a skip
    /// concerns its author alone (§10.7, §10.3).
    pub fn concerns(&self, device_no: u16) -> bool {
        match *self {
            Notice::Overwrite { author, other, .. } => author == device_no || other == device_no,
            Notice::Skipped { author, .. } => author == device_no,
        }
    }
}

impl RefusalRecord {
    /// The record's canonical bytes (core.md §10.7).
    pub fn to_canonical(&self) -> Vec<u8> {
        let mut m = Map::new();
        put(&mut m, "author", Value::from(self.author));
        if let Some(b) = self.base {
            put(&mut m, "base", Value::from(b));
        }
        put(&mut m, "code", Value::from(self.code.as_str()));
        if let Some(c) = self.current {
            put(&mut m, "current", Value::from(c));
        }
        if let Some(op) = &self.op {
            put(&mut m, "op", Value::from(op.as_str()));
        }
        put(&mut m, "seq", Value::from(self.seq));
        if let Some(t) = self.target {
            put(&mut m, "target", Value::from(t));
        }
        canon::encode(&Value::Object(m)).unwrap_or_default()
    }

    /// A record from canonical bytes (core.md §10.7).
    pub fn from_canonical(b: &[u8]) -> Result<Self, CoreError> {
        record_from(b, RefusalRecord::from_value, RefusalRecord::to_canonical)
    }

    fn from_value(v: &Value) -> Result<RefusalRecord, CoreError> {
        let mut o = Obj::new(v)?;
        // `code` and `op` are read as any string, so that a newer build's
        // record still reads (core.md §10.7).
        let r = RefusalRecord {
            author: device(o.req("author")?)?,
            base: o.opt("base").map(canon::u53).transpose()?,
            code: canon::string(o.req("code")?)?.to_owned(),
            current: o.opt("current").map(canon::u53).transpose()?,
            op: o.opt("op").map(canon::string).transpose()?.map(str::to_owned),
            seq: canon::u53(o.req("seq")?)?,
            target: o.opt("target").map(canon::u53).transpose()?,
        };
        o.done()?;
        // `base` and `current` only with `op` `set_notes` (core.md §10.7).
        if (r.base.is_some() || r.current.is_some()) && r.op.as_deref() != Some("set_notes") {
            return Err(schema());
        }
        Ok(r)
    }
}

// ---------------------------------------------------------------- apply (§8)

/// Whether `ctx` satisfies core.md §8.1 (CORE-CTX-001). `at` is an
/// `Instant`, which is within §5.1's range by construction.
fn ctx_ok(ctx: &Ctx) -> bool {
    (1..=MAX_DEVICE_NO).contains(&ctx.device_no) && (2..=MAX_U53).contains(&ctx.seq) && ctx.based_on < ctx.seq
}

/// Applies one command (core.md §8.4).
pub fn apply(board: &mut BoardData, command: &LogCommand, ctx: &Ctx) -> Result<Applied, Refusal> {
    let refuse = |code| Refusal { code, op: command.op(), target: command.target(), notes: None };
    // Step 1: the context.
    if !ctx_ok(ctx) {
        return Err(refuse(BAD_CTX));
    }
    // Step 2: the op table of the board's rules version.
    if !op_table(board.rules).contains(&command.op()) {
        return Err(refuse(RULES));
    }
    // Steps 3 to 5 under the rules of `board.rules` (CORE-RULES-001).
    if board.rules != 1 {
        return Err(refuse(RULES));
    }
    // Step 4, the size check (CORE-APPLY-005). The length the effects leave
    // is at most the length before plus `growth_bound`; when even that is
    // within the cap, the command cannot be refused for size and runs on the
    // board itself. The length before is first bounded from lengths alone
    // (`size_bound`, a pass over the lists), and only when that is not enough
    // counted exactly (`canonical_len`, no encoding).
    let growth = growth_bound(command, board);
    if size_bound(board).saturating_add(growth) <= SIZE_CAP {
        return V1 { board, ctx, command, out: Applied::default() }.run();
    }
    let before = canonical_len(board);
    if before.saturating_add(growth) <= SIZE_CAP {
        return V1 { board, ctx, command, out: Applied::default() }.run();
    }
    // At the cap: the checks and effects run on a copy and its length is
    // counted. A refusal of step 3 comes first.
    let mut copy = board.clone();
    let out = V1 { board: &mut copy, ctx, command, out: Applied::default() }.run()?;
    if out.benign {
        // A benign repeat changes nothing (CORE-APPLY-004).
        return Ok(out);
    }
    let after = canonical_len(&copy);
    if after > SIZE_CAP && after > before {
        return Err(refuse(CAP));
    }
    *board = copy;
    Ok(out)
}

// ---------------------------------------------------------------- the size check (§8.4 step 4)

/// The size cap: a command may not leave canonical board bytes over
/// 61,865,984 (59 MiB) that are longer than before it (core.md §8.4 step 4,
/// §17, CORE-APPLY-005).
pub(crate) const SIZE_CAP: usize = 61_865_984;

/// The most bytes one byte of a string takes in canonical bytes: a control
/// character is written `\u00XX` (core.md §4.2 rule 4); every other byte is
/// written as itself or as a two-byte escape.
pub(crate) const ESC: usize = 6;

// Bounds of what each part of a board takes in canonical bytes, its comma
// included, without the bytes of its strings (counted with `ESC`) and of its
// blocks. Each is above the record with every optional field present at its
// longest; the unit tests encode those records and compare.

/// Everything of a board but its lists' elements and its notes text: every
/// key, the six lists' brackets, `home`, `notes_rev`, `palette`, `picture`
/// and `rules` at their longest.
pub(crate) const BOARD_BOUND: usize = 1024;
/// A live item, its `blocks` key and brackets included.
pub(crate) const ITEM_BOUND: usize = 640;
/// A live block.
pub(crate) const BLOCK_BOUND: usize = 128;
/// An archive row, its `blocks` key and brackets included.
pub(crate) const ROW_BOUND: usize = 576;
/// A block of an archive row.
pub(crate) const ROW_BLOCK_BOUND: usize = 96;
/// A subscription.
pub(crate) const SUB_BOUND: usize = 160;
/// A mark.
pub(crate) const MARK_BOUND: usize = 64;
/// A list key with its brackets and comma, which a list's first element
/// adds (`"subscriptions":[],`, the longest, is 19).
const LIST_BOUND: usize = 32;
/// What raising a mark adds: a new mark and the `marks` key, or the digits
/// of an existing one.
const MARK_GROWTH: usize = MARK_BOUND + LIST_BOUND;
/// What a changed field group of a live item adds: a field that was absent
/// at its longest (`"deadline":"…",`, 44) or longer digits, and its stamp's
/// digits (`[1,0]` to `[9007199254740991,8191]`, 18).
const FIELD_GROWTH: usize = 128;
/// What a block a command moves adds: the digits of `minutes` and of its
/// stamp (`start` is always 30 bytes).
const MOVE_GROWTH: usize = 32;
/// What `complete` and `delete` add: an archive row holds the item's fields
/// but its stamps, plus `archived_at` (46) and `outcome` (20), and the
/// `archive` key.
const RETIRE_GROWTH: usize = 128 + LIST_BOUND;
/// What `restore` adds beyond its row's blocks: an item holds the row's
/// fields but `archived_at` and `outcome`, plus five stamps, an id of up to
/// 16 digits where the row's was 0, the `items` key and a mark.
const RESTORE_GROWTH: usize = 512 + MARK_GROWTH;
/// What `restore` adds per block of its row: a `stamp` (`"stamp":[…,…],`, 32).
const RESTORE_BLOCK_GROWTH: usize = 48;
/// What a settings command adds: a palette, home or picture at its longest.
const SETTINGS_GROWTH: usize = 512;

/// A bound of a board's canonical length, never below it, from lengths and
/// counts alone (no encoding): a pass over the lists.
pub(crate) fn size_bound(b: &BoardData) -> usize {
    let strings = |s: &str| ESC.saturating_mul(s.len());
    let items = b.items.iter().fold(0usize, |n, i| {
        n.saturating_add(ITEM_BOUND).saturating_add(strings(&i.name)).saturating_add(BLOCK_BOUND.saturating_mul(i.blocks.len()))
    });
    let rows = b.archive.iter().fold(0usize, |n, r| {
        n.saturating_add(ROW_BOUND).saturating_add(strings(&r.name)).saturating_add(ROW_BLOCK_BOUND.saturating_mul(r.blocks.len()))
    });
    // A line is written as base64 with padding, in quotes, with a comma.
    let lines = b.unreadable.iter().fold(0usize, |n, l| n.saturating_add(l.len().div_ceil(3).saturating_mul(4)).saturating_add(3));
    let subs = b.subscriptions.iter().fold(0usize, |n, s| {
        n.saturating_add(SUB_BOUND).saturating_add(strings(&s.name)).saturating_add(strings(&s.url))
    });
    BOARD_BOUND
        .saturating_add(strings(&b.notes))
        .saturating_add(items)
        .saturating_add(rows)
        .saturating_add(lines)
        .saturating_add(subs)
        .saturating_add(MARK_BOUND.saturating_mul(b.marks.len()))
}

// ------------------------------------------------ the exact canonical length

/// The length of a canonical integer (core.md §4.2 rule 5).
fn int_len(i: i64) -> usize {
    let mut m = i.unsigned_abs();
    let mut n = usize::from(i < 0) + 1;
    while m >= 10 {
        m /= 10;
        n += 1;
    }
    n
}

fn uint_len(u: u64) -> usize {
    int_len(i64::try_from(u).unwrap_or(i64::MAX))
}

/// The length of a canonical string, quotes included: the escapes of the
/// table in core.md §4.2 rule 4 take two bytes, other control characters six.
fn str_len(s: &str) -> usize {
    s.bytes().fold(2usize, |n, c| {
        n.saturating_add(match c {
            b'"' | b'\\' | 0x08 | 0x09 | 0x0a | 0x0c | 0x0d => 2,
            0x00..=0x1f => 6,
            _ => 1,
        })
    })
}

/// An instant's text in quotes: 30 bytes and two (core.md §4.4).
const INSTANT_LEN: usize = 32;
/// A date's text in quotes (core.md §4.4).
const DATE_LEN: usize = 12;

/// The length of a canonical object, member by member: `{`, `"key":value`
/// joined by commas, `}` (core.md §4.2).
struct ObjLen {
    members: usize,
    len: usize,
}

impl ObjLen {
    fn new() -> ObjLen {
        ObjLen { members: 0, len: 2 }
    }

    fn field(mut self, key: &str, value: usize) -> ObjLen {
        // `"key":` and the comma before every member but the first.
        self.len = self.len.saturating_add(key.len() + 3 + usize::from(self.members > 0)).saturating_add(value);
        self.members += 1;
        self
    }

    fn opt(self, key: &str, value: Option<usize>) -> ObjLen {
        match value {
            Some(v) => self.field(key, v),
            None => self,
        }
    }

    fn done(self) -> usize {
        self.len
    }
}

/// The length of a canonical array of elements of these lengths.
fn array_len(elements: impl Iterator<Item = usize>) -> usize {
    let (n, sum) = elements.fold((0usize, 0usize), |(n, s), e| (n + 1, s.saturating_add(e)));
    sum.saturating_add(2 + n.saturating_sub(1))
}

/// A list field, absent when empty (core.md §4.3).
fn list_len<T>(v: &[T], f: impl Fn(&T) -> usize) -> Option<usize> {
    (!v.is_empty()).then(|| array_len(v.iter().map(f)))
}

fn stamp_len(s: Stamp) -> usize {
    3 + uint_len(s.seq) + uint_len(u64::from(s.device_no))
}

fn kind_len(k: Kind) -> usize {
    match k {
        Kind::Task => 6,
        Kind::Event => 7,
        Kind::Routine => 9,
    }
}

fn recurrence_len(r: &Recurrence) -> usize {
    ObjLen::new()
        .field("anchor", DATE_LEN)
        .field("days", uint_len(u64::from(r.days)))
        .field("minutes", uint_len(u64::from(r.minutes)))
        .field("start_minutes", int_len(i64::from(r.start_minutes)))
        .done()
}

fn block_len(b: &Block) -> usize {
    ObjLen::new()
        .field("minutes", uint_len(u64::from(b.minutes)))
        .field("sid", uint_len(b.sid))
        .field("stamp", stamp_len(b.stamp))
        .field("start", INSTANT_LEN)
        .done()
}

fn row_block_len(b: &RowBlock) -> usize {
    ObjLen::new()
        .field("minutes", uint_len(u64::from(b.minutes)))
        .field("sid", uint_len(b.sid))
        .field("start", INSTANT_LEN)
        .done()
}

fn item_len(i: &Item) -> usize {
    let s = &i.stamps;
    let stamps = ObjLen::new()
        .field("estimate", stamp_len(s.estimate))
        .field("levels", stamp_len(s.levels))
        .field("name", stamp_len(s.name))
        .field("repeat", stamp_len(s.repeat))
        .field("time", stamp_len(s.time))
        .done();
    ObjLen::new()
        .opt("blocks", list_len(&i.blocks, block_len))
        .field("created", INSTANT_LEN)
        .opt("deadline", i.deadline.map(|_| INSTANT_LEN))
        .opt("duration_minutes", i.duration_minutes.map(|m| uint_len(u64::from(m))))
        .field("id", uint_len(i.id))
        .opt("importance", i.importance.map(|x| uint_len(u64::from(x))))
        .field("kind", kind_len(i.kind))
        .field("name", str_len(&i.name))
        .opt("recurrence", i.recurrence.as_ref().map(recurrence_len))
        .field("stamps", stamps)
        .opt("time_importance", i.time_importance.map(|x| uint_len(u64::from(x))))
        .done()
}

fn row_len(r: &ArchiveRow) -> usize {
    ObjLen::new()
        .field("archived_at", INSTANT_LEN)
        .opt("blocks", list_len(&r.blocks, row_block_len))
        .field("created", INSTANT_LEN)
        .opt("deadline", r.deadline.map(|_| INSTANT_LEN))
        .opt("duration_minutes", r.duration_minutes.map(|m| uint_len(u64::from(m))))
        .field("id", uint_len(r.id))
        .opt("importance", r.importance.map(|x| uint_len(u64::from(x))))
        .field("kind", kind_len(r.kind))
        .field("name", str_len(&r.name))
        .opt("outcome", (r.outcome == Outcome::Dropped).then_some(9))
        .opt("recurrence", r.recurrence.as_ref().map(recurrence_len))
        .opt("time_importance", r.time_importance.map(|x| uint_len(u64::from(x))))
        .done()
}

fn rgba_len(c: [u8; 4]) -> usize {
    array_len(c.iter().map(|&x| uint_len(u64::from(x))))
}

/// The length of a board's canonical bytes (core.md §6.13), counted from
/// its fields without encoding anything; equal to
/// `to_canonical_bytes().len()` (the tests compare them on generated boards).
pub(crate) fn canonical_len(b: &BoardData) -> usize {
    let home = (b.home != crate::board::Home { lat: 0, lon: 0 })
        .then(|| ObjLen::new().field("lat", int_len(i64::from(b.home.lat))).field("lon", int_len(i64::from(b.home.lon))).done());
    let palette = (b.palette != [[0u8; 4]; 6]).then(|| array_len(b.palette.iter().map(|&c| rgba_len(c))));
    let picture = b.picture.map(|p| {
        ObjLen::new()
            .field("blob", 34)
            .field("epoch", uint_len(u64::from(p.epoch)))
            .field("parts", uint_len(u64::from(p.parts)))
            .field("sha256", 66)
            .field("size", uint_len(p.size))
            .done()
    });
    let mark = |m: &crate::board::Mark| {
        ObjLen::new()
            .field("block", uint_len(m.block))
            .field("device_no", uint_len(u64::from(m.device_no)))
            .field("item", uint_len(m.item))
            .done()
    };
    let sub = |s: &crate::board::Subscription| {
        ObjLen::new()
            .field("color", rgba_len(s.color))
            .opt("enabled", (!s.enabled).then_some(5))
            .field("id", uint_len(s.id))
            .field("name", str_len(&s.name))
            .field("url", str_len(&s.url))
            .done()
    };
    ObjLen::new()
        .opt("archive", list_len(&b.archive, row_len))
        .opt("home", home)
        .opt("items", list_len(&b.items, item_len))
        .opt("marks", list_len(&b.marks, mark))
        .opt("notes", (!b.notes.is_empty()).then(|| str_len(&b.notes)))
        .opt("notes_rev", (b.notes_rev != 0).then(|| uint_len(b.notes_rev)))
        .opt("palette", palette)
        .opt("picture", picture)
        .field("rules", uint_len(u64::from(b.rules)))
        .opt("subscriptions", list_len(&b.subscriptions, sub))
        // A line is base64 with padding, in quotes (core.md §6.7).
        .opt("unreadable", list_len(&b.unreadable, |l| l.len().div_ceil(3).saturating_mul(4).saturating_add(2)))
        .done()
}

/// A bound of how many bytes a command's effects can add to a board's
/// canonical length, never below what they add.
pub(crate) fn growth_bound(c: &LogCommand, board: &BoardData) -> usize {
    let strings = |s: &str| ESC.saturating_mul(s.len());
    let new_item = ITEM_BOUND + LIST_BOUND + MARK_GROWTH;
    match c {
        LogCommand::CreateTask(c) => new_item.saturating_add(strings(&c.name)).saturating_add(BLOCK_BOUND.saturating_mul(c.blocks.len())),
        LogCommand::CreateEvent(CreateEvent { name, .. }) | LogCommand::CreateRoutine(CreateRoutine { name, .. }) => {
            new_item.saturating_add(strings(name))
        }
        LogCommand::Rename { name, .. } => FIELD_GROWTH.saturating_add(strings(name)),
        LogCommand::Complete { .. } | LogCommand::Delete { .. } => RETIRE_GROWTH,
        // These only take something away.
        LogCommand::Forget { .. }
        | LogCommand::RemoveBlocks { .. }
        | LogCommand::ForgetArchived { .. }
        | LogCommand::RemoveSubscription { .. } => 0,
        LogCommand::MoveBlock { .. } => MOVE_GROWTH,
        LogCommand::MoveBlocks { moves } => MOVE_GROWTH.saturating_mul(moves.len()),
        LogCommand::AddBlock { .. } => BLOCK_BOUND + LIST_BOUND + MARK_GROWTH,
        LogCommand::SetEstimate { .. }
        | LogCommand::SetDeadline { .. }
        | LogCommand::SetSeverity { .. }
        | LogCommand::SetHorizon { .. }
        | LogCommand::SetRepeat { .. }
        | LogCommand::MoveRoutine { .. }
        | LogCommand::SetEventSpan { .. } => FIELD_GROWTH,
        // `"notes":"…",` and `"notes_rev":…,` at their longest.
        LogCommand::SetNotes { text, .. } => FIELD_GROWTH.saturating_add(strings(text)),
        LogCommand::Restore { id, archived_at, .. } => {
            let key = ArchiveKey { id: *id, archived_at: *archived_at };
            let blocks = board.find_archive_row(&key).map_or(0, |r| board.archive[r].blocks.len());
            RESTORE_GROWTH.saturating_add(RESTORE_BLOCK_GROWTH.saturating_mul(blocks))
        }
        LogCommand::AddSubscription { name, url, .. } => {
            (SUB_BOUND + LIST_BOUND + MARK_GROWTH).saturating_add(strings(name)).saturating_add(strings(url))
        }
        LogCommand::RenameSubscription { name, .. } => strings(name),
        LogCommand::SetSubscriptionColor { .. } | LogCommand::SetSubscriptionEnabled { .. } => MOVE_GROWTH,
        LogCommand::SetPalette { .. } | LogCommand::SetHome { .. } | LogCommand::SetPicture { .. } => SETTINGS_GROWTH,
    }
}

/// The stamp an entry writes (core.md §10.1).
fn entry_stamp(ctx: &Ctx) -> Stamp {
    Stamp { seq: ctx.seq, device_no: ctx.device_no }
}

/// Whether a stamp is another device's change since the origin's view
/// (core.md §10.1).
fn others_since(s: Stamp, ctx: &Ctx) -> bool {
    s.seq > ctx.based_on && s.device_no != ctx.device_no && s.device_no != 0
}

/// An item's latest stamp (core.md §10.1): the first, in the order
/// `estimate`, `levels`, `name`, `repeat`, `time`, then the blocks by sid,
/// with the greatest `seq` (§10.1 settles that tie order).
fn latest_stamp(item: &Item) -> Stamp {
    let s = &item.stamps;
    let mut best = s.estimate;
    for x in [s.levels, s.name, s.repeat, s.time].into_iter().chain(item.blocks.iter().map(|b| b.stamp)) {
        if x.seq > best.seq {
            best = x;
        }
    }
    best
}

fn all_stamps(s: Stamp) -> ItemStamps {
    ItemStamps { estimate: s, levels: s, name: s, repeat: s, time: s }
}

fn live_blocks(board: &BoardData) -> usize {
    board.items.iter().map(|i| i.blocks.len()).sum()
}

/// One `apply` under rules version 1 (core.md §9, §10).
struct V1<'a> {
    board: &'a mut BoardData,
    ctx: &'a Ctx,
    command: &'a LogCommand,
    out: Applied,
}

type Step = Result<(), Refusal>;

impl V1<'_> {
    fn refuse(&self, code: &'static str) -> Refusal {
        Refusal { code, op: self.command.op(), target: self.command.target(), notes: None }
    }

    fn check(&self, ok: bool, code: &'static str) -> Step {
        if ok { Ok(()) } else { Err(self.refuse(code)) }
    }

    /// CORE-ID-002: a newly minted id's high part is the author's and its low
    /// part is above the author's mark (`block` for a sid).
    fn mint_check(&self, id: u64, block: bool) -> Step {
        let (h, n) = ids::split(id);
        let mark = self.board.mark(self.ctx.device_no);
        let above = if block { n > mark.block } else { n > mark.item };
        self.check(h == self.ctx.device_no && above, BAD_ID)
    }

    /// The live item, else `core/gone` (core.md §9.3).
    fn live(&self, id: u64) -> Result<usize, Refusal> {
        self.board.item_index(id).map_err(|_| self.refuse(GONE))
    }

    /// The kind check (core.md §9.3).
    fn kind(&self, k: usize, kinds: &[Kind]) -> Step {
        self.check(kinds.contains(&self.board.items[k].kind), KIND)
    }

    /// An overwrite notice when `old` is another device's change since the
    /// origin's view (core.md §10.2, CORE-CONF-002).
    fn overwrite(&mut self, old: Stamp, item: u64, sid: Option<u64>, group: Group) {
        if others_since(old, self.ctx) {
            self.out.notices.push(Notice::Overwrite {
                seq: self.ctx.seq,
                author: self.ctx.device_no,
                other: old.device_no,
                other_seq: old.seq,
                item,
                sid,
                group,
            });
        }
    }

    fn benign() -> Result<Applied, Refusal> {
        Ok(Applied { benign: true, ..Applied::default() })
    }

    /// Raises the author's marks (CORE-ID-003) and records the change.
    fn raise_mark(&mut self, item_id: Option<u64>, sid: Option<u64>) {
        let before = self.board.marks.clone();
        let item = item_id.map_or(0, |i| ids::split(i).1);
        let block = sid.map_or(0, |s| ids::split(s).1);
        self.board.raise_mark(self.ctx.device_no, item, block);
        self.out.changes.marks |= self.board.marks != before;
    }

    fn insert_item(&mut self, item: Item) {
        let id = item.id;
        match self.board.item_index(id) {
            Err(k) => self.board.items.insert(k, item),
            // The mint check keeps a created id off the live items; on a board
            // that breaks rule 4 of §6.12 the item is replaced rather than
            // duplicated.
            Ok(k) => self.board.items[k] = item,
        }
        self.out.changes.items.insert(id);
    }

    fn run(mut self) -> Result<Applied, Refusal> {
        let command = self.command;
        match command {
            LogCommand::CreateTask(c) => self.create_task(c)?,
            LogCommand::CreateEvent(c) => self.create_event(c)?,
            LogCommand::CreateRoutine(c) => self.create_routine(c)?,
            LogCommand::Rename { id, name } => {
                let k = self.live(*id)?;
                let old = self.board.items[k].stamps.name;
                self.overwrite(old, *id, None, Group::Name);
                let st = entry_stamp(self.ctx);
                let item = &mut self.board.items[k];
                item.name.clone_from(name);
                item.stamps.name = st;
                self.out.changes.items.insert(*id);
            }
            LogCommand::Complete { id, at } => return self.retire(*id, *at, Outcome::Finished),
            LogCommand::Delete { id, at } => return self.retire(*id, *at, Outcome::Dropped),
            LogCommand::Forget { id } => {
                let Ok(k) = self.board.item_index(*id) else { return Self::benign() };
                let item = self.board.items.remove(k);
                self.overwrite(latest_stamp(&item), *id, None, Group::Item);
                self.out.changes.items.insert(*id);
            }
            LogCommand::MoveBlock { id, sid, start, minutes } => {
                let k = self.live(*id)?;
                self.kind(k, &[Kind::Task])?;
                let b = self.board.items[k].blocks.binary_search_by_key(sid, |b| b.sid).map_err(|_| self.refuse(GONE))?;
                let old = self.board.items[k].blocks[b].stamp;
                self.overwrite(old, *id, Some(*sid), Group::Block);
                let st = entry_stamp(self.ctx);
                let block = &mut self.board.items[k].blocks[b];
                block.start = *start;
                block.minutes = *minutes;
                block.stamp = st;
                self.out.changes.items.insert(*id);
            }
            LogCommand::MoveBlocks { moves } => self.move_blocks(moves)?,
            LogCommand::AddBlock { id, sid, start, minutes } => {
                let k = self.live(*id)?;
                self.kind(k, &[Kind::Task])?;
                self.mint_check(*sid, true)?;
                let room = self.board.items[k].blocks.len() < MAX_BLOCKS_PER_ITEM && live_blocks(self.board) < MAX_LIVE_BLOCKS;
                self.check(room, CAP)?;
                let block = Block { sid: *sid, start: *start, minutes: *minutes, stamp: entry_stamp(self.ctx) };
                let blocks = &mut self.board.items[k].blocks;
                if let Err(b) = blocks.binary_search_by_key(sid, |b| b.sid) {
                    blocks.insert(b, block);
                }
                self.out.changes.items.insert(*id);
                self.raise_mark(None, Some(*sid));
            }
            LogCommand::RemoveBlocks { blocks } => return self.remove_blocks(blocks),
            LogCommand::SetEstimate { id, minutes } => {
                let k = self.live(*id)?;
                self.kind(k, &[Kind::Task, Kind::Event])?;
                let st = entry_stamp(self.ctx);
                let item = &self.board.items[k];
                let (old, group) = match item.kind {
                    Kind::Task => (item.stamps.estimate, Group::Estimate),
                    _ => (item.stamps.time, Group::Time),
                };
                self.overwrite(old, *id, None, group);
                let item = &mut self.board.items[k];
                item.duration_minutes = Some(*minutes);
                match group {
                    Group::Estimate => item.stamps.estimate = st,
                    _ => item.stamps.time = st,
                }
                self.out.changes.items.insert(*id);
            }
            LogCommand::SetDeadline { id, deadline, seed } => {
                let k = self.live(*id)?;
                self.kind(k, &[Kind::Task])?;
                let old = self.board.items[k].stamps.time;
                self.overwrite(old, *id, None, Group::Time);
                let st = entry_stamp(self.ctx);
                let item = &mut self.board.items[k];
                item.deadline = *deadline;
                item.stamps.time = st;
                // CORE-CMD-009: the seed only when present and no importance
                // is set at the time of apply; no notice for it.
                if seed.is_some() && item.importance.is_none() {
                    item.importance = *seed;
                    item.stamps.levels = st;
                }
                self.out.changes.items.insert(*id);
            }
            LogCommand::SetSeverity { id, level } | LogCommand::SetHorizon { id, level } => {
                let k = self.live(*id)?;
                self.kind(k, &[Kind::Task])?;
                let old = self.board.items[k].stamps.levels;
                self.overwrite(old, *id, None, Group::Levels);
                let st = entry_stamp(self.ctx);
                let item = &mut self.board.items[k];
                if matches!(command, LogCommand::SetSeverity { .. }) {
                    item.importance = Some(*level);
                } else {
                    item.time_importance = Some(*level);
                }
                item.stamps.levels = st;
                self.out.changes.items.insert(*id);
            }
            LogCommand::SetRepeat { id, days, anchor } => {
                let k = self.live(*id)?;
                self.kind(k, &[Kind::Routine])?;
                let old = self.board.items[k].stamps.repeat;
                self.overwrite(old, *id, None, Group::Repeat);
                let st = entry_stamp(self.ctx);
                let item = &mut self.board.items[k];
                // A routine has a recurrence (rule 6 of §6.12). No zone is
                // consulted (CORE-CMD-007).
                if let Some(r) = item.recurrence.as_mut() {
                    r.days = *days;
                    r.anchor = *anchor;
                }
                item.stamps.repeat = st;
                self.out.changes.items.insert(*id);
            }
            LogCommand::MoveRoutine { id, start_minutes, minutes } => {
                let k = self.live(*id)?;
                self.kind(k, &[Kind::Routine])?;
                let old = self.board.items[k].stamps.time;
                self.overwrite(old, *id, None, Group::Time);
                let st = entry_stamp(self.ctx);
                let item = &mut self.board.items[k];
                if let Some(r) = item.recurrence.as_mut() {
                    r.start_minutes = *start_minutes;
                    r.minutes = *minutes;
                }
                item.stamps.time = st;
                self.out.changes.items.insert(*id);
            }
            LogCommand::SetEventSpan { id, start, minutes } => {
                let k = self.live(*id)?;
                self.kind(k, &[Kind::Event])?;
                let old = self.board.items[k].stamps.time;
                self.overwrite(old, *id, None, Group::Time);
                let st = entry_stamp(self.ctx);
                let item = &mut self.board.items[k];
                item.deadline = Some(*start);
                item.duration_minutes = Some(*minutes);
                item.stamps.time = st;
                self.out.changes.items.insert(*id);
            }
            LogCommand::SetNotes { text, base } => {
                let current = self.board.notes_rev;
                if *base != current {
                    return Err(Refusal { notes: Some(NotesConflict { base: *base, current }), ..self.refuse(NOTES_STALE) });
                }
                // Check 2: `notes_rev` below 2^53 − 1, else `core/cap` (core.md §9.20).
                self.check(current < MAX_U53, CAP)?;
                self.board.notes.clone_from(text);
                self.board.notes_rev = current + 1;
                self.out.changes.notes = true;
            }
            LogCommand::Restore { id, archived_at, fallback } => return self.restore(*id, *archived_at, *fallback),
            LogCommand::ForgetArchived { id, archived_at } => {
                let key = ArchiveKey { id: *id, archived_at: *archived_at };
                let Some(r) = self.board.find_archive_row(&key) else { return Self::benign() };
                self.board.archive.remove(r);
                self.out.changes.archive_removed.push(key);
            }
            LogCommand::AddSubscription { id, name, url, color } => {
                self.mint_check(*id, false)?;
                self.check(self.board.subscriptions.len() < MAX_SUBSCRIPTIONS, CAP)?;
                self.check(!self.board.subscriptions.iter().any(|s| s.url.as_bytes() == url.as_bytes()), DUPLICATE_URL)?;
                let sub = crate::board::Subscription { id: *id, name: name.clone(), url: url.clone(), color: *color, enabled: true };
                if let Err(k) = self.board.subscription_index(*id) {
                    self.board.subscriptions.insert(k, sub);
                }
                self.out.changes.subscriptions.insert(*id);
                self.raise_mark(Some(*id), None);
            }
            LogCommand::RemoveSubscription { id } => {
                let Ok(k) = self.board.subscription_index(*id) else { return Self::benign() };
                self.board.subscriptions.remove(k);
                self.out.changes.subscriptions.insert(*id);
                self.out.changes.overlay_forget.insert(*id);
            }
            LogCommand::RenameSubscription { id, .. }
            | LogCommand::SetSubscriptionColor { id, .. }
            | LogCommand::SetSubscriptionEnabled { id, .. } => {
                let k = self.board.subscription_index(*id).map_err(|_| self.refuse(GONE))?;
                let sub = &mut self.board.subscriptions[k];
                let before = sub.clone();
                match command {
                    LogCommand::RenameSubscription { name, .. } => sub.name.clone_from(name),
                    LogCommand::SetSubscriptionColor { color, .. } => sub.color = *color,
                    LogCommand::SetSubscriptionEnabled { enabled, .. } => {
                        sub.enabled = *enabled;
                        if !*enabled {
                            self.out.changes.overlay_forget.insert(*id);
                        }
                    }
                    _ => {}
                }
                if self.board.subscriptions[k] != before {
                    self.out.changes.subscriptions.insert(*id);
                }
            }
            LogCommand::SetPalette { colors } => {
                self.out.changes.settings = self.board.palette != *colors;
                self.board.palette = *colors;
            }
            LogCommand::SetHome { lat, lon } => {
                let home = crate::board::Home { lat: *lat, lon: *lon };
                self.out.changes.settings = self.board.home != home;
                self.board.home = home;
            }
            LogCommand::SetPicture { picture } => {
                self.out.changes.settings = self.board.picture != *picture;
                self.board.picture = *picture;
            }
        }
        Ok(self.out)
    }

    /// The item cap of §9.5: fewer than 10,000 live items.
    fn item_room(&self) -> Step {
        self.check(self.board.items.len() < MAX_ITEMS, CAP)
    }

    fn create_task(&mut self, c: &CreateTask) -> Step {
        self.mint_check(c.id, false)?;
        for b in &c.blocks {
            self.mint_check(b.sid, true)?;
        }
        self.item_room()?;
        self.check(live_blocks(self.board) + c.blocks.len() <= MAX_LIVE_BLOCKS, CAP)?;
        let st = entry_stamp(self.ctx);
        self.insert_item(Item {
            id: c.id,
            kind: Kind::Task,
            name: c.name.clone(),
            created: self.ctx.at,
            deadline: c.deadline,
            importance: c.importance,
            time_importance: c.time_importance,
            duration_minutes: c.duration_minutes,
            recurrence: None,
            blocks: c.blocks.iter().map(|b| Block { sid: b.sid, start: b.start, minutes: b.minutes, stamp: st }).collect(),
            stamps: all_stamps(st),
        });
        // `blocks` ascend by sid (§9.4), so the last has the largest.
        self.raise_mark(Some(c.id), c.blocks.last().map(|b| b.sid));
        Ok(())
    }

    fn create_event(&mut self, c: &CreateEvent) -> Step {
        self.mint_check(c.id, false)?;
        self.item_room()?;
        let st = entry_stamp(self.ctx);
        self.insert_item(Item {
            id: c.id,
            kind: Kind::Event,
            name: c.name.clone(),
            created: self.ctx.at,
            deadline: Some(c.start),
            importance: c.importance,
            time_importance: c.time_importance,
            duration_minutes: c.minutes,
            recurrence: None,
            blocks: Vec::new(),
            stamps: all_stamps(st),
        });
        self.raise_mark(Some(c.id), None);
        Ok(())
    }

    fn create_routine(&mut self, c: &CreateRoutine) -> Step {
        self.mint_check(c.id, false)?;
        self.item_room()?;
        let st = entry_stamp(self.ctx);
        // The start floats: no zone is consulted (CORE-CMD-007, V2-4).
        self.insert_item(Item {
            id: c.id,
            kind: Kind::Routine,
            name: c.name.clone(),
            created: self.ctx.at,
            deadline: None,
            importance: None,
            time_importance: None,
            duration_minutes: None,
            recurrence: Some(Recurrence { days: c.days, anchor: c.anchor, start_minutes: c.start_minutes, minutes: c.minutes }),
            blocks: Vec::new(),
            stamps: all_stamps(st),
        });
        self.raise_mark(Some(c.id), None);
        Ok(())
    }

    /// `complete` and `delete` (core.md §9.8).
    fn retire(mut self, id: u64, at: Instant, outcome: Outcome) -> Result<Applied, Refusal> {
        let Ok(k) = self.board.item_index(id) else {
            if self.board.archive.iter().any(|r| r.id == id) {
                return Self::benign();
            }
            return Err(self.refuse(GONE));
        };
        if outcome == Outcome::Finished {
            self.kind(k, &[Kind::Task])?;
        }
        self.check(self.board.archive.len() < MAX_ARCHIVE, CAP)?;
        // CORE-CMD-005: `min(at, ctx.at + 1 day)`, the sum saturating at the
        // end of the range.
        let clamp = self.ctx.at.plus_seconds(ARCHIVE_CLAMP_S).unwrap_or(Instant::MAX);
        let archived_at = at.min(clamp);
        let item = self.board.items.remove(k);
        let latest = latest_stamp(&item);
        // Every field copied, as today's `Archived::retire` (archive.rs:186-201).
        let row = ArchiveRow {
            id: item.id,
            kind: item.kind,
            name: item.name,
            created: item.created,
            deadline: item.deadline,
            importance: item.importance,
            time_importance: item.time_importance,
            duration_minutes: item.duration_minutes,
            recurrence: item.recurrence,
            blocks: item.blocks.iter().map(|b| RowBlock { sid: b.sid, start: b.start, minutes: b.minutes }).collect(),
            archived_at,
            outcome,
        };
        self.board.insert_archive_row(row);
        self.overwrite(latest, id, None, Group::Item);
        self.out.changes.items.insert(id);
        self.out.changes.archive_inserted.push(ArchiveKey { id, archived_at });
        Ok(self.out)
    }

    /// `move_blocks` (core.md §9.11, §10.3).
    fn move_blocks(&mut self, moves: &[crate::command::BlockMove]) -> Step {
        for mv in moves {
            if let Some(item) = self.board.item(mv.id) {
                self.check(item.kind == Kind::Task, KIND)?;
            }
        }
        let (seq, author) = (self.ctx.seq, self.ctx.device_no);
        for mv in moves {
            let found = self
                .board
                .item_index(mv.id)
                .ok()
                .and_then(|k| self.board.items[k].blocks.binary_search_by_key(&mv.sid, |b| b.sid).ok().map(|b| (k, b)));
            let Some((k, b)) = found else {
                self.out.skipped += 1;
                let reason = SkipReason::Gone;
                self.out.notices.push(Notice::Skipped { seq, author, item: mv.id, sid: mv.sid, reason, other: None });
                continue;
            };
            let block = &mut self.board.items[k].blocks[b];
            if others_since(block.stamp, self.ctx) {
                self.out.skipped += 1;
                let other = Some((block.stamp.device_no, block.stamp.seq));
                let reason = SkipReason::Changed;
                self.out.notices.push(Notice::Skipped { seq, author, item: mv.id, sid: mv.sid, reason, other });
                continue;
            }
            block.start = mv.start;
            block.minutes = mv.minutes;
            block.stamp = Stamp { seq, device_no: author };
            self.out.moved += 1;
            self.out.changes.items.insert(mv.id);
        }
        Ok(())
    }

    /// `remove_blocks` (core.md §9.13, §10.4).
    fn remove_blocks(mut self, pairs: &[(u64, u64)]) -> Result<Applied, Refusal> {
        for &(id, _) in pairs {
            if let Some(item) = self.board.item(id) {
                self.check(item.kind == Kind::Task, KIND)?;
            }
        }
        let mut removed = false;
        for &(id, sid) in pairs {
            let Ok(k) = self.board.item_index(id) else { continue };
            let Ok(b) = self.board.items[k].blocks.binary_search_by_key(&sid, |b| b.sid) else { continue };
            let block = self.board.items[k].blocks.remove(b);
            self.overwrite(block.stamp, id, Some(sid), Group::Block);
            self.out.changes.items.insert(id);
            removed = true;
        }
        if !removed {
            return Self::benign();
        }
        Ok(self.out)
    }

    /// `restore` (core.md §9.21, §7.4).
    fn restore(mut self, id: u64, archived_at: Instant, fallback: u64) -> Result<Applied, Refusal> {
        self.mint_check(fallback, false)?;
        let key = ArchiveKey { id, archived_at };
        let Some(r) = self.board.find_archive_row(&key) else {
            let live = |x: u64| self.board.item(x).is_some();
            if (id != 0 && live(id)) || live(fallback) {
                return Self::benign();
            }
            return Err(self.refuse(GONE));
        };
        let row_blocks = self.board.archive[r].blocks.len();
        self.item_room()?;
        self.check(live_blocks(self.board) + row_blocks <= MAX_LIVE_BLOCKS, CAP)?;
        let mut live_sids: Vec<u64> = self.board.items.iter().flat_map(|i| i.blocks.iter().map(|b| b.sid)).collect();
        live_sids.sort_unstable();
        let taken = self.board.archive[r].blocks.iter().any(|b| live_sids.binary_search(&b.sid).is_ok());
        self.check(!taken, ID_TAKEN)?;
        // CORE-ID-005: the row's own id unless it is 0 or held; no range
        // check on it.
        let target = if id != 0 && self.board.item(id).is_none() { id } else { fallback };
        let row = self.board.archive.remove(r);
        let st = entry_stamp(self.ctx);
        // Every field of the row, as today's `Archived::to_active`
        // (archive.rs:210-226); `created` is the row's.
        self.insert_item(Item {
            id: target,
            kind: row.kind,
            name: row.name,
            created: row.created,
            deadline: row.deadline,
            importance: row.importance,
            time_importance: row.time_importance,
            duration_minutes: row.duration_minutes,
            recurrence: row.recurrence,
            blocks: row.blocks.iter().map(|b| Block { sid: b.sid, start: b.start, minutes: b.minutes, stamp: st }).collect(),
            stamps: all_stamps(st),
        });
        self.out.changes.archive_removed.push(key);
        // CORE-ID-003: the mark rises to `fallback` whether or not it was used.
        self.raise_mark(Some(fallback), None);
        self.out.restored_as = Some(target);
        Ok(self.out)
    }
}

// ---------------------------------------------------------------- rules versions (§11)

/// Moves the board to rules version `v` (core.md §11.2).
pub fn upgrade_rules(board: &mut BoardData, v: u32) -> Result<(), CoreError> {
    // Steps 1 and 2, before anything changes (CORE-RULES-002).
    if v <= board.rules || !SUPPORTED_RULES.contains(&v) {
        return Err(CoreError::code(UNKNOWN_RULES));
    }
    // Step 3: the version, then the migration it defines.
    board.rules = v;
    migrate_to(board, v);
    Ok(())
}

/// The migration rules version `v` defines, a pure function of the board
/// (core.md §11.2). Version 1, the only one in `SUPPORTED_RULES`, defines
/// none; a later version adds its arm here.
fn migrate_to(_board: &mut BoardData, _v: u32) {}

/// The reset board (core.md §11.3).
pub fn reset(target: &BoardData, current: &BoardData) -> Result<BoardData, CoreError> {
    // Step 1.
    if target.rules > current.rules {
        return Err(CoreError::code(UNKNOWN_RULES));
    }
    // Step 2: `target` with the elementwise maximum of the marks, ascending by
    // `device_no`, and the notes revision one above both.
    let mut b = target.clone();
    let mut marks = target.marks.clone();
    for m in &current.marks {
        match marks.binary_search_by_key(&m.device_no, |x| x.device_no) {
            Ok(k) => {
                marks[k].item = marks[k].item.max(m.item);
                marks[k].block = marks[k].block.max(m.block);
            }
            Err(k) => marks.insert(k, *m),
        }
    }
    b.marks = marks;
    // The revision cannot be raised past 2^53 − 1: `core/cap`, and nothing is
    // returned (CORE-RULES-005).
    let rev = target.notes_rev.max(current.notes_rev);
    if rev >= MAX_U53 {
        return Err(CoreError::code(CAP));
    }
    b.notes_rev = rev + 1;
    // Step 3.
    if b.rules < current.rules {
        upgrade_rules(&mut b, current.rules)?;
    }
    Ok(b)
}

#[cfg(test)]
mod tests;
