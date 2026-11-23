//! `Ctx`, `apply`, conflict rules and rules versions (core.md §8, §10, §11, §20).
//!
//! `apply` runs the four steps of §8.4: the context (`core/bad_ctx`), the op
//! table of `board.rules` (`core/rules`), then the command's checks and its
//! effects under that rules version. Every check of a command runs before its
//! first change to the board, so a refusal leaves the board as it was
//! (CORE-APPLY-002). Rules version 1 is `apply_v1`; a later version sits
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

/// §4.5 steps 1 to 3 here, 4 in `read`, 5 by encoding the result once and
/// comparing (CORE-CONF-006).
// SPEC-QUESTION: §10.7 gives the records no byte limit; taken as the default limit of `canon::Record` (60 MiB).
fn record_from<T>(b: &[u8], read: impl Fn(&Value) -> Result<T, CoreError>, enc: impl Fn(&T) -> Vec<u8>) -> Result<T, CoreError> {
    let value = canon::parse_value(b, canon::BOARD_LIMIT)?;
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
    /// An overwrite concerns its author and the `other` device (§10.2); a
    /// skip concerns its author alone (§10.3).
    // SPEC-QUESTION: §10.7 says a device keeps a notice when it is the `author` or `other`, §10.3 that a `skipped` notice is for the author alone; taken as §10.3 for skips.
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
        // SPEC-QUESTION: §10.7 lets `code` be any code of §16, of seal.md §22 or two engine codes, and `op` any op (a newer build's too); taken as any string for both.
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
        // `base` and `current` belong to `set_notes` only.
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
    // Steps 3 and 4 under the rules of `board.rules` (CORE-RULES-001).
    match board.rules {
        1 => V1 { board, ctx, command, out: Applied::default() }.run(),
        _ => Err(refuse(RULES)),
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
/// with the greatest `seq`.
// SPEC-QUESTION: §10.1 does not say which stamp is the latest when two with the greatest seq differ in device_no (only a hand-made board has that); taken as the first in field-name order, then blocks.
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
                // SPEC-QUESTION: §9.20 has no check for a `notes_rev` of 2^53 − 1, whose increment would leave a board that fails `check` (CORE-DATA-006); taken as `core/cap`.
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
    b.notes_rev = target.notes_rev.max(current.notes_rev).saturating_add(1);
    // Step 3.
    if b.rules < current.rules {
        upgrade_rules(&mut b, current.rules)?;
    }
    Ok(b)
}

#[cfg(test)]
mod tests;
