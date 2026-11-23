//! Board data (core.md §6, §20).
//!
//! Every record of §6 with its schema (`Record`), the canonical board bytes
//! (§6.13), the invariants of §6.12 and the order helpers later tasks use to
//! keep every list in its spec order.

use std::cmp::Ordering;

use serde_json::{Map, Value};

use crate::CoreError;
use crate::canon::{self, Obj, Record, schema};
use crate::time::Instant;

/// An item's kind (core.md §6.2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Task,
    Event,
    Routine,
}

/// How an archive row left the board (core.md §6.6).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Outcome {
    Finished,
    Dropped,
}

/// A field group's stamp (core.md §6.5, §10.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Stamp {
    pub seq: u64,
    pub device_no: u16,
}

/// An item's stamps per field group (core.md §6.5).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ItemStamps {
    pub estimate: Stamp,
    pub levels: Stamp,
    pub name: Stamp,
    pub repeat: Stamp,
    pub time: Stamp,
}

/// A routine's rule (core.md §6.4).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Recurrence {
    pub days: u8,
    pub anchor: chrono::NaiveDate,
    pub start_minutes: i32,
    pub minutes: u32,
}

/// A planned block of a live item (core.md §6.3).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Block {
    pub sid: u64,
    pub start: Instant,
    pub minutes: u32,
    pub stamp: Stamp,
}

/// A block of an archive row (core.md §6.6).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RowBlock {
    pub sid: u64,
    pub start: Instant,
    pub minutes: u32,
}

/// A live item (core.md §6.2).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Item {
    pub id: u64,
    pub kind: Kind,
    pub name: String,
    pub created: Instant,
    pub deadline: Option<Instant>,
    pub importance: Option<u8>,
    pub time_importance: Option<u8>,
    pub duration_minutes: Option<u32>,
    pub recurrence: Option<Recurrence>,
    pub blocks: Vec<Block>,
    pub stamps: ItemStamps,
}

/// An archived item (core.md §6.6).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ArchiveRow {
    pub id: u64,
    pub kind: Kind,
    pub name: String,
    pub created: Instant,
    pub deadline: Option<Instant>,
    pub importance: Option<u8>,
    pub time_importance: Option<u8>,
    pub duration_minutes: Option<u32>,
    pub recurrence: Option<Recurrence>,
    pub blocks: Vec<RowBlock>,
    pub archived_at: Instant,
    pub outcome: Outcome,
}

/// The archive key (core.md §6.11).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ArchiveKey {
    pub id: u64,
    pub archived_at: Instant,
}

/// A calendar subscription (core.md §6.8).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Subscription {
    pub id: u64,
    pub name: String,
    pub url: String,
    pub color: [u8; 4],
    pub enabled: bool,
}

/// The home coordinates setting (core.md §6.9).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Home {
    pub lat: i32,
    pub lon: i32,
}

/// The picture setting (core.md §6.9).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PictureRef {
    pub blob: [u8; 16],
    pub epoch: u32,
    pub parts: u32,
    pub sha256: [u8; 32],
    pub size: u64,
}

/// A device's id mark (core.md §6.10, §7.2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Mark {
    pub device_no: u16,
    pub item: u64,
    pub block: u64,
}

/// The board (core.md §6.1).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct BoardData {
    pub rules: u32,
    pub items: Vec<Item>,
    pub archive: Vec<ArchiveRow>,
    pub unreadable: Vec<Vec<u8>>,
    pub notes: String,
    pub notes_rev: u64,
    pub subscriptions: Vec<Subscription>,
    pub palette: [[u8; 4]; 6],
    pub home: Home,
    pub picture: Option<PictureRef>,
    pub marks: Vec<Mark>,
}

// ---------------------------------------------------------------- limits (§6, §17)

const MAX_U53: u64 = (1 << 53) - 1;
/// The largest low part of an id, and the largest mark: 2^40 − 1 (core.md §6.10, §7.1).
const MAX_LOW: u64 = (1 << 40) - 1;
const MAX_DEVICE_NO: u16 = 8191;
const NAME_BYTES: usize = 4096;
const NOTES_BYTES: usize = 1_048_576;
const URL_BYTES: usize = 8000;
const LINE_BYTES: usize = 65_536;
const MAX_ITEMS: usize = 10_000;
const MAX_BLOCKS_PER_ITEM: usize = 1000;
const MAX_LIVE_BLOCKS: usize = 50_000;
const MAX_ARCHIVE: usize = 200_000;
const MAX_UNREADABLE: usize = 10_000;
const MAX_SUBSCRIPTIONS: usize = 20;
const PICTURE_PART: u64 = 1_048_576;
const DEFAULT_PALETTE: [[u8; 4]; 6] = [[0; 4]; 6];

// ---------------------------------------------------------------- field readers

/// An `int[lo..hi]` field converted to its Rust type.
fn small<T: TryFrom<i64>>(v: &Value, lo: i64, hi: i64) -> Result<T, CoreError> {
    T::try_from(canon::int(v, lo, hi)?).map_err(|_| schema())
}

fn opt_small<T: TryFrom<i64>>(o: &mut Obj<'_>, key: &str, lo: i64, hi: i64) -> Result<Option<T>, CoreError> {
    o.opt(key).map(|v| small(v, lo, hi)).transpose()
}

fn list<T>(o: &mut Obj<'_>, key: &str, f: impl Fn(&Value) -> Result<T, CoreError>) -> Result<Vec<T>, CoreError> {
    match o.opt(key) {
        None => Ok(Vec::new()),
        Some(v) => canon::array(v)?.iter().map(f).collect(),
    }
}

fn kind_from(v: &Value) -> Result<Kind, CoreError> {
    match canon::string(v)? {
        "task" => Ok(Kind::Task),
        "event" => Ok(Kind::Event),
        "routine" => Ok(Kind::Routine),
        _ => Err(schema()),
    }
}

fn kind_text(k: Kind) -> &'static str {
    match k {
        Kind::Task => "task",
        Kind::Event => "event",
        Kind::Routine => "routine",
    }
}

fn stamp_from(v: &Value) -> Result<Stamp, CoreError> {
    match canon::array(v)?.as_slice() {
        [seq, device_no] => Ok(Stamp { seq: canon::u53(seq)?, device_no: small(device_no, 0, 8191)? }),
        _ => Err(schema()),
    }
}

fn stamp_value(s: Stamp) -> Value {
    Value::Array(vec![Value::from(s.seq), Value::from(s.device_no)])
}

fn instant_value(t: Instant) -> Value {
    Value::String(t.to_canonical())
}

fn put(m: &mut Map<String, Value>, key: &str, v: Value) {
    m.insert(key.to_owned(), v);
}

fn put_opt<T>(m: &mut Map<String, Value>, key: &str, v: Option<T>, f: impl Fn(T) -> Value) {
    if let Some(x) = v {
        put(m, key, f(x));
    }
}

fn put_list<T>(m: &mut Map<String, Value>, key: &str, v: &[T], f: impl Fn(&T) -> Value) {
    if !v.is_empty() {
        put(m, key, Value::Array(v.iter().map(f).collect()));
    }
}

// ---------------------------------------------------------------- records

impl Record for ItemStamps {
    /// It occurs only inside a board: the board's limit (core.md §4.5, §17).
    const LIMIT: usize = canon::BOARD_LIMIT;

    fn to_value(&self) -> Value {
        let mut m = Map::new();
        put(&mut m, "estimate", stamp_value(self.estimate));
        put(&mut m, "levels", stamp_value(self.levels));
        put(&mut m, "name", stamp_value(self.name));
        put(&mut m, "repeat", stamp_value(self.repeat));
        put(&mut m, "time", stamp_value(self.time));
        Value::Object(m)
    }

    fn from_value(v: &Value) -> Result<Self, CoreError> {
        let mut o = Obj::new(v)?;
        let s = ItemStamps {
            estimate: stamp_from(o.req("estimate")?)?,
            levels: stamp_from(o.req("levels")?)?,
            name: stamp_from(o.req("name")?)?,
            repeat: stamp_from(o.req("repeat")?)?,
            time: stamp_from(o.req("time")?)?,
        };
        o.done()?;
        Ok(s)
    }
}

impl Record for Recurrence {
    /// It occurs only inside a board: the board's limit (core.md §4.5, §17).
    const LIMIT: usize = canon::BOARD_LIMIT;

    fn to_value(&self) -> Value {
        let mut m = Map::new();
        put(&mut m, "anchor", Value::String(canon::date_text(self.anchor)));
        put(&mut m, "days", Value::from(self.days));
        put(&mut m, "minutes", Value::from(self.minutes));
        put(&mut m, "start_minutes", Value::from(self.start_minutes));
        Value::Object(m)
    }

    fn from_value(v: &Value) -> Result<Self, CoreError> {
        let mut o = Obj::new(v)?;
        let r = Recurrence {
            anchor: canon::date(o.req("anchor")?)?,
            days: small(o.req("days")?, 0, 127)?,
            minutes: small(o.req("minutes")?, 0, 1440)?,
            start_minutes: small(o.req("start_minutes")?, 0, 1439)?,
        };
        o.done()?;
        Ok(r)
    }
}

impl Record for Block {
    /// It occurs only inside a board: the board's limit (core.md §4.5, §17).
    const LIMIT: usize = canon::BOARD_LIMIT;

    fn to_value(&self) -> Value {
        let mut m = Map::new();
        put(&mut m, "minutes", Value::from(self.minutes));
        put(&mut m, "sid", Value::from(self.sid));
        put(&mut m, "stamp", stamp_value(self.stamp));
        put(&mut m, "start", instant_value(self.start));
        Value::Object(m)
    }

    fn from_value(v: &Value) -> Result<Self, CoreError> {
        let mut o = Obj::new(v)?;
        let b = Block {
            minutes: small(o.req("minutes")?, 0, 1440)?,
            sid: canon::u53(o.req("sid")?)?,
            stamp: stamp_from(o.req("stamp")?)?,
            start: canon::instant(o.req("start")?)?,
        };
        o.done()?;
        Ok(b)
    }
}

impl Record for RowBlock {
    /// It occurs only inside a board: the board's limit (core.md §4.5, §17).
    const LIMIT: usize = canon::BOARD_LIMIT;

    fn to_value(&self) -> Value {
        let mut m = Map::new();
        put(&mut m, "minutes", Value::from(self.minutes));
        put(&mut m, "sid", Value::from(self.sid));
        put(&mut m, "start", instant_value(self.start));
        Value::Object(m)
    }

    fn from_value(v: &Value) -> Result<Self, CoreError> {
        let mut o = Obj::new(v)?;
        let b = RowBlock {
            minutes: small(o.req("minutes")?, 0, 1440)?,
            sid: canon::u53(o.req("sid")?)?,
            start: canon::instant(o.req("start")?)?,
        };
        o.done()?;
        Ok(b)
    }
}

impl Record for Item {
    /// It occurs only inside a board: the board's limit (core.md §4.5, §17).
    const LIMIT: usize = canon::BOARD_LIMIT;

    fn to_value(&self) -> Value {
        let mut m = Map::new();
        put_list(&mut m, "blocks", &self.blocks, Block::to_value);
        put(&mut m, "created", instant_value(self.created));
        put_opt(&mut m, "deadline", self.deadline, instant_value);
        put_opt(&mut m, "duration_minutes", self.duration_minutes, Value::from);
        put(&mut m, "id", Value::from(self.id));
        put_opt(&mut m, "importance", self.importance, Value::from);
        put(&mut m, "kind", Value::from(kind_text(self.kind)));
        put(&mut m, "name", Value::String(self.name.clone()));
        put_opt(&mut m, "recurrence", self.recurrence.as_ref(), Recurrence::to_value);
        put(&mut m, "stamps", self.stamps.to_value());
        put_opt(&mut m, "time_importance", self.time_importance, Value::from);
        Value::Object(m)
    }

    fn from_value(v: &Value) -> Result<Self, CoreError> {
        let mut o = Obj::new(v)?;
        let item = Item {
            blocks: list(&mut o, "blocks", Block::from_value)?,
            created: canon::instant(o.req("created")?)?,
            deadline: o.opt("deadline").map(canon::instant).transpose()?,
            duration_minutes: opt_small(&mut o, "duration_minutes", 0, 1440)?,
            id: canon::u53(o.req("id")?)?,
            importance: opt_small(&mut o, "importance", 0, 4)?,
            kind: kind_from(o.req("kind")?)?,
            name: canon::string_bytes(o.req("name")?, 0, NAME_BYTES)?.to_owned(),
            recurrence: o.opt("recurrence").map(Recurrence::from_value).transpose()?,
            stamps: ItemStamps::from_value(o.req("stamps")?)?,
            time_importance: opt_small(&mut o, "time_importance", 0, 3)?,
        };
        o.done()?;
        Ok(item)
    }
}

impl Record for ArchiveRow {
    /// It occurs only inside a board: the board's limit (core.md §4.5, §17).
    const LIMIT: usize = canon::BOARD_LIMIT;

    fn to_value(&self) -> Value {
        let mut m = Map::new();
        put(&mut m, "archived_at", instant_value(self.archived_at));
        put_list(&mut m, "blocks", &self.blocks, RowBlock::to_value);
        put(&mut m, "created", instant_value(self.created));
        put_opt(&mut m, "deadline", self.deadline, instant_value);
        put_opt(&mut m, "duration_minutes", self.duration_minutes, Value::from);
        put(&mut m, "id", Value::from(self.id));
        put_opt(&mut m, "importance", self.importance, Value::from);
        put(&mut m, "kind", Value::from(kind_text(self.kind)));
        put(&mut m, "name", Value::String(self.name.clone()));
        if self.outcome == Outcome::Dropped {
            put(&mut m, "outcome", Value::from("dropped"));
        }
        put_opt(&mut m, "recurrence", self.recurrence.as_ref(), Recurrence::to_value);
        put_opt(&mut m, "time_importance", self.time_importance, Value::from);
        Value::Object(m)
    }

    fn from_value(v: &Value) -> Result<Self, CoreError> {
        let mut o = Obj::new(v)?;
        let row = ArchiveRow {
            archived_at: canon::instant(o.req("archived_at")?)?,
            blocks: list(&mut o, "blocks", RowBlock::from_value)?,
            created: canon::instant(o.req("created")?)?,
            deadline: o.opt("deadline").map(canon::instant).transpose()?,
            duration_minutes: opt_small(&mut o, "duration_minutes", 0, 1440)?,
            id: canon::u53(o.req("id")?)?,
            importance: opt_small(&mut o, "importance", 0, 4)?,
            kind: kind_from(o.req("kind")?)?,
            name: canon::string_bytes(o.req("name")?, 0, NAME_BYTES)?.to_owned(),
            outcome: match o.opt("outcome").map(canon::string).transpose()? {
                None | Some("finished") => Outcome::Finished,
                Some("dropped") => Outcome::Dropped,
                Some(_) => return Err(schema()),
            },
            recurrence: o.opt("recurrence").map(Recurrence::from_value).transpose()?,
            time_importance: opt_small(&mut o, "time_importance", 0, 3)?,
        };
        o.done()?;
        Ok(row)
    }
}

impl Record for Subscription {
    /// It occurs only inside a board: the board's limit (core.md §4.5, §17).
    const LIMIT: usize = canon::BOARD_LIMIT;

    fn to_value(&self) -> Value {
        let mut m = Map::new();
        put(&mut m, "color", canon::rgba_value(self.color));
        if !self.enabled {
            put(&mut m, "enabled", Value::Bool(false));
        }
        put(&mut m, "id", Value::from(self.id));
        put(&mut m, "name", Value::String(self.name.clone()));
        put(&mut m, "url", Value::String(self.url.clone()));
        Value::Object(m)
    }

    fn from_value(v: &Value) -> Result<Self, CoreError> {
        let mut o = Obj::new(v)?;
        let s = Subscription {
            color: canon::rgba(o.req("color")?)?,
            enabled: o.opt("enabled").map(canon::boolean).transpose()?.unwrap_or(true),
            id: canon::u53(o.req("id")?)?,
            name: canon::string_bytes(o.req("name")?, 0, NAME_BYTES)?.to_owned(),
            url: canon::string_bytes(o.req("url")?, 1, URL_BYTES)?.to_owned(),
        };
        o.done()?;
        Ok(s)
    }
}

impl Record for Home {
    /// It occurs only inside a board: the board's limit (core.md §4.5, §17).
    const LIMIT: usize = canon::BOARD_LIMIT;

    fn to_value(&self) -> Value {
        let mut m = Map::new();
        put(&mut m, "lat", Value::from(self.lat));
        put(&mut m, "lon", Value::from(self.lon));
        Value::Object(m)
    }

    fn from_value(v: &Value) -> Result<Self, CoreError> {
        let mut o = Obj::new(v)?;
        let h = Home { lat: small(o.req("lat")?, -9000, 9000)?, lon: small(o.req("lon")?, -18000, 18000)? };
        o.done()?;
        Ok(h)
    }
}

impl Record for PictureRef {
    /// It occurs only inside a board: the board's limit (core.md §4.5, §17).
    const LIMIT: usize = canon::BOARD_LIMIT;

    fn to_value(&self) -> Value {
        let mut m = Map::new();
        put(&mut m, "blob", Value::String(canon::hex_text(&self.blob)));
        put(&mut m, "epoch", Value::from(self.epoch));
        put(&mut m, "parts", Value::from(self.parts));
        put(&mut m, "sha256", Value::String(canon::hex_text(&self.sha256)));
        put(&mut m, "size", Value::from(self.size));
        Value::Object(m)
    }

    fn from_value(v: &Value) -> Result<Self, CoreError> {
        let mut o = Obj::new(v)?;
        let p = PictureRef {
            blob: canon::hex::<16>(o.req("blob")?)?,
            epoch: small(o.req("epoch")?, 1, 4_294_967_295)?,
            parts: small(o.req("parts")?, 1, 64)?,
            sha256: canon::hex::<32>(o.req("sha256")?)?,
            size: small(o.req("size")?, 67, 67_108_864)?,
        };
        o.done()?;
        Ok(p)
    }
}

impl Record for Mark {
    /// It occurs only inside a board: the board's limit (core.md §4.5, §17).
    const LIMIT: usize = canon::BOARD_LIMIT;

    fn to_value(&self) -> Value {
        let mut m = Map::new();
        put(&mut m, "block", Value::from(self.block));
        put(&mut m, "device_no", Value::from(self.device_no));
        put(&mut m, "item", Value::from(self.item));
        Value::Object(m)
    }

    fn from_value(v: &Value) -> Result<Self, CoreError> {
        let mut o = Obj::new(v)?;
        let low = i64::try_from(MAX_LOW).map_err(|_| schema())?;
        let mk = Mark {
            block: small(o.req("block")?, 0, low)?,
            device_no: small(o.req("device_no")?, 0, 8191)?,
            item: small(o.req("item")?, 0, low)?,
        };
        o.done()?;
        Ok(mk)
    }
}

impl Record for BoardData {
    /// The canonical board bytes: 62,914,560 (core.md §17).
    const LIMIT: usize = canon::BOARD_LIMIT;

    fn to_value(&self) -> Value {
        let mut m = Map::new();
        put_list(&mut m, "archive", &self.archive, ArchiveRow::to_value);
        if self.home != (Home { lat: 0, lon: 0 }) {
            put(&mut m, "home", self.home.to_value());
        }
        put_list(&mut m, "items", &self.items, Item::to_value);
        put_list(&mut m, "marks", &self.marks, Mark::to_value);
        if !self.notes.is_empty() {
            put(&mut m, "notes", Value::String(self.notes.clone()));
        }
        if self.notes_rev != 0 {
            put(&mut m, "notes_rev", Value::from(self.notes_rev));
        }
        if self.palette != DEFAULT_PALETTE {
            put(&mut m, "palette", Value::Array(self.palette.iter().map(|&c| canon::rgba_value(c)).collect()));
        }
        put_opt(&mut m, "picture", self.picture.as_ref(), PictureRef::to_value);
        put(&mut m, "rules", Value::from(self.rules));
        put_list(&mut m, "subscriptions", &self.subscriptions, Subscription::to_value);
        put_list(&mut m, "unreadable", &self.unreadable, |l| Value::String(canon::base64_text(l)));
        Value::Object(m)
    }

    fn from_value(v: &Value) -> Result<Self, CoreError> {
        let mut o = Obj::new(v)?;
        let board = BoardData {
            archive: list(&mut o, "archive", ArchiveRow::from_value)?,
            home: o.opt("home").map(Home::from_value).transpose()?.unwrap_or(Home { lat: 0, lon: 0 }),
            items: list(&mut o, "items", Item::from_value)?,
            marks: list(&mut o, "marks", Mark::from_value)?,
            notes: o.opt("notes").map(|v| canon::string_bytes(v, 0, NOTES_BYTES)).transpose()?.unwrap_or("").to_owned(),
            notes_rev: o.opt("notes_rev").map(canon::u53).transpose()?.unwrap_or(0),
            palette: match o.opt("palette") {
                None => DEFAULT_PALETTE,
                Some(p) => {
                    let slots = canon::array(p)?;
                    if slots.len() != 6 {
                        return Err(schema());
                    }
                    let mut palette = DEFAULT_PALETTE;
                    for (slot, c) in palette.iter_mut().zip(slots) {
                        *slot = canon::rgba(c)?;
                    }
                    palette
                }
            },
            picture: o.opt("picture").map(PictureRef::from_value).transpose()?,
            rules: small(o.req("rules")?, 1, 2_147_483_647)?,
            subscriptions: list(&mut o, "subscriptions", Subscription::from_value)?,
            unreadable: list(&mut o, "unreadable", |l| canon::base64(l, 1, LINE_BYTES))?,
        };
        o.done()?;
        Ok(board)
    }
}

// ---------------------------------------------------------------- orders

/// The archive order of core.md §6.12: `archived_at`, then `id`, then the
/// canonical bytes of the rows as unsigned bytes (a proper prefix first).
pub(crate) fn archive_cmp(a: &ArchiveRow, b: &ArchiveRow) -> Ordering {
    (a.archived_at, a.id).cmp(&(b.archived_at, b.id)).then_with(|| a.to_canonical().cmp(&b.to_canonical()))
}

/// Whether the high part (0 to 8191) and the non-zero low part make a minted
/// or legacy id (core.md §6.12 rule 3, §7.1).
fn id_form_ok(id: u64) -> bool {
    id & MAX_LOW != 0 && id >> 40 <= u64::from(MAX_DEVICE_NO)
}

fn stamp_ok(s: &Stamp) -> bool {
    s.seq <= MAX_U53 && s.device_no <= MAX_DEVICE_NO
}

fn date_ok(d: chrono::NaiveDate) -> bool {
    use chrono::Datelike;
    (1970..=9999).contains(&d.year())
}

fn recurrence_ok(r: &Recurrence) -> bool {
    r.days <= 127 && r.minutes <= 1440 && (0..=1439).contains(&r.start_minutes) && date_ok(r.anchor)
}

fn levels_ok(importance: Option<u8>, time_importance: Option<u8>, duration: Option<u32>) -> bool {
    importance.is_none_or(|x| x <= 4) && time_importance.is_none_or(|x| x <= 3) && duration.is_none_or(|x| x <= 1440)
}

fn strictly_ascending<T, K: Ord>(v: &[T], key: impl Fn(&T) -> K) -> bool {
    v.windows(2).all(|w| key(&w[0]) < key(&w[1]))
}

impl BoardData {
    /// The empty board `{"rules":1}` (core.md §15.3).
    pub fn empty() -> Self {
        BoardData {
            rules: 1,
            items: Vec::new(),
            archive: Vec::new(),
            unreadable: Vec::new(),
            notes: String::new(),
            notes_rev: 0,
            subscriptions: Vec::new(),
            palette: DEFAULT_PALETTE,
            home: Home { lat: 0, lon: 0 },
            picture: None,
            marks: Vec::new(),
        }
    }

    /// Canonical board bytes (core.md §6.13).
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        self.to_canonical()
    }

    /// Loads a board from canonical bytes (core.md §6.14).
    ///
    /// §4.5 steps 1 to 5 in order (`core/too_large`, `core/not_json`,
    /// `core/schema`, `core/not_canonical`), then `check` (`core/invariant`
    /// with the first failing rule of §6.12). Nothing is normalised.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, CoreError> {
        let board: BoardData = canon::record_from_canonical(bytes, canon::BOARD_LIMIT)?;
        board.check()?;
        Ok(board)
    }

    /// Checks the invariants (core.md §6.12).
    pub fn check(&self) -> Result<(), CoreError> {
        let rules: [fn(&BoardData) -> bool; 9] = [
            BoardData::rule_1_schema_and_order,
            BoardData::rule_2_caps,
            BoardData::rule_3_id_form,
            BoardData::rule_4_marks,
            BoardData::rule_5_live_sids,
            BoardData::rule_6_kinds,
            |b| !b.notes.contains('\t'),
            |b| b.picture.is_none_or(|p| u64::from(p.parts) == p.size.div_ceil(PICTURE_PART)),
            |b| crate::apply::SUPPORTED_RULES.contains(&b.rules),
        ];
        for (k, rule) in (1u32..).zip(rules) {
            if !rule(self) {
                return Err(CoreError::invariant(k));
            }
        }
        Ok(())
    }

    /// Rule 1: every field within its schema (§6.1 to §6.10), every array in
    /// its order without forbidden duplicates.
    fn rule_1_schema_and_order(&self) -> bool {
        let items_ok = self.items.iter().all(|i| {
            i.id <= MAX_U53
                && i.name.len() <= NAME_BYTES
                && levels_ok(i.importance, i.time_importance, i.duration_minutes)
                && i.recurrence.as_ref().is_none_or(recurrence_ok)
                && i.blocks.iter().all(|b| b.sid <= MAX_U53 && b.minutes <= 1440 && stamp_ok(&b.stamp))
                && strictly_ascending(&i.blocks, |b| b.sid)
                && [i.stamps.estimate, i.stamps.levels, i.stamps.name, i.stamps.repeat, i.stamps.time].iter().all(stamp_ok)
        }) && strictly_ascending(&self.items, |i| i.id);
        let rows_ok = self.archive.iter().all(|r| {
            r.id <= MAX_U53
                && r.name.len() <= NAME_BYTES
                && levels_ok(r.importance, r.time_importance, r.duration_minutes)
                && r.recurrence.as_ref().is_none_or(recurrence_ok)
                && r.blocks.iter().all(|b| b.sid <= MAX_U53 && b.minutes <= 1440)
                && strictly_ascending(&r.blocks, |b| b.sid)
        }) && self.archive.windows(2).all(|w| archive_cmp(&w[0], &w[1]) != Ordering::Greater);
        let lines_ok = self.unreadable.iter().all(|l| (1..=LINE_BYTES).contains(&l.len()))
            && self.unreadable.windows(2).all(|w| w[0] <= w[1]);
        let subs_ok = self.subscriptions.iter().all(|s| {
            s.id <= MAX_U53 && s.name.len() <= NAME_BYTES && (1..=URL_BYTES).contains(&s.url.len())
        }) && strictly_ascending(&self.subscriptions, |s| s.id)
            && {
                let mut urls: Vec<&str> = self.subscriptions.iter().map(|s| s.url.as_str()).collect();
                urls.sort_unstable();
                urls.windows(2).all(|w| w[0] != w[1])
            };
        let settings_ok = (1..=2_147_483_647).contains(&self.rules)
            && self.notes.len() <= NOTES_BYTES
            && self.notes_rev <= MAX_U53
            && (-9000..=9000).contains(&self.home.lat)
            && (-18000..=18000).contains(&self.home.lon)
            && self.picture.is_none_or(|p| p.epoch >= 1 && (1..=64).contains(&p.parts) && (67..=67_108_864).contains(&p.size));
        // A mark with both numbers 0 is not stored (§6.10); one that is
        // breaks rule 1 (§6.12 rule 1).
        let marks_ok = self
            .marks
            .iter()
            .all(|m| m.device_no <= MAX_DEVICE_NO && m.item <= MAX_LOW && m.block <= MAX_LOW && (m.item, m.block) != (0, 0))
            && strictly_ascending(&self.marks, |m| m.device_no);
        items_ok && rows_ok && lines_ok && subs_ok && settings_ok && marks_ok
    }

    /// Rule 2: every count within its cap (§17).
    fn rule_2_caps(&self) -> bool {
        self.items.len() <= MAX_ITEMS
            && self.archive.len() <= MAX_ARCHIVE
            && self.unreadable.len() <= MAX_UNREADABLE
            && self.subscriptions.len() <= MAX_SUBSCRIPTIONS
            && self.items.iter().all(|i| i.blocks.len() <= MAX_BLOCKS_PER_ITEM)
            && self.archive.iter().all(|r| r.blocks.len() <= MAX_BLOCKS_PER_ITEM)
            && self.items.iter().map(|i| i.blocks.len()).sum::<usize>() <= MAX_LIVE_BLOCKS
    }

    /// Every id of rules 3 and 4 with its kind: `true` for a sid.
    fn ids_and_sids(&self) -> impl Iterator<Item = (u64, bool)> + '_ {
        let items = self.items.iter().map(|i| (i.id, false));
        let subs = self.subscriptions.iter().map(|s| (s.id, false));
        let rows = self.archive.iter().filter(|r| r.id != 0).map(|r| (r.id, false));
        let live_sids = self.items.iter().flat_map(|i| i.blocks.iter().map(|b| (b.sid, true)));
        let row_sids = self.archive.iter().flat_map(|r| r.blocks.iter().map(|b| (b.sid, true)));
        items.chain(subs).chain(rows).chain(live_sids).chain(row_sids)
    }

    /// Rule 3: the form of every id and sid (§7.1).
    fn rule_3_id_form(&self) -> bool {
        self.ids_and_sids().all(|(id, _)| id_form_ok(id))
    }

    /// Rule 4: every id and sid at or below its device's mark (§7.2).
    fn rule_4_marks(&self) -> bool {
        self.ids_and_sids().all(|(id, sid)| {
            let (h, n) = crate::ids::split(id);
            let mark = self.mark(h);
            if sid { mark.block >= n } else { mark.item >= n }
        })
    }

    /// Rule 5: the sids of all live blocks pairwise distinct.
    fn rule_5_live_sids(&self) -> bool {
        let mut sids: Vec<u64> = self.items.iter().flat_map(|i| i.blocks.iter().map(|b| b.sid)).collect();
        sids.sort_unstable();
        sids.windows(2).all(|w| w[0] != w[1])
    }

    /// Rule 6: `kind = routine` exactly when `recurrence` is present.
    fn rule_6_kinds(&self) -> bool {
        self.items.iter().all(|i| (i.kind == Kind::Routine) == i.recurrence.is_some())
            && self.archive.iter().all(|r| (r.kind == Kind::Routine) == r.recurrence.is_some())
    }

    /// The live item with this id (core.md §6.1).
    pub fn item(&self, id: u64) -> Option<&Item> {
        self.item_index(id).ok().and_then(|k| self.items.get(k))
    }

    /// The subscription with this id (core.md §6.8).
    pub fn subscription(&self, id: u64) -> Option<&Subscription> {
        self.subscription_index(id).ok().and_then(|k| self.subscriptions.get(k))
    }

    /// A device's mark, zero when absent (core.md §6.10).
    pub fn mark(&self, device_no: u16) -> Mark {
        match self.mark_index(device_no) {
            Ok(k) => self.marks[k],
            Err(_) => Mark { device_no, item: 0, block: 0 },
        }
    }

    // ------------------------------------------------------------ order helpers

    /// The position of the live item `id`, or where it would be inserted
    /// (`items` ascends by id).
    pub(crate) fn item_index(&self, id: u64) -> Result<usize, usize> {
        self.items.binary_search_by_key(&id, |i| i.id)
    }

    /// The position of the subscription `id`, or where it would be inserted.
    pub(crate) fn subscription_index(&self, id: u64) -> Result<usize, usize> {
        self.subscriptions.binary_search_by_key(&id, |s| s.id)
    }

    /// The position of the mark of `device_no`, or where it would be inserted.
    pub(crate) fn mark_index(&self, device_no: u16) -> Result<usize, usize> {
        self.marks.binary_search_by_key(&device_no, |m| m.device_no)
    }

    /// Raises the mark of `device_no` to at least `item` and `block`,
    /// inserting it at its place; a mark that would stay all zero is not
    /// stored (core.md §6.10, §7.2).
    pub(crate) fn raise_mark(&mut self, device_no: u16, item: u64, block: u64) {
        match self.mark_index(device_no) {
            Ok(k) => {
                let m = &mut self.marks[k];
                m.item = m.item.max(item);
                m.block = m.block.max(block);
            }
            Err(k) => {
                if (item, block) != (0, 0) {
                    self.marks.insert(k, Mark { device_no, item, block });
                }
            }
        }
    }

    /// Inserts an archive row at the position the archive order gives it,
    /// after every row that does not follow it (core.md §6.12, CORE-DATA-003).
    /// Returns its index.
    pub(crate) fn insert_archive_row(&mut self, row: ArchiveRow) -> usize {
        let key = (row.archived_at, row.id);
        // Rows with a smaller key go first without encoding anything; only
        // rows with an equal key are compared by their bytes.
        let lo = self.archive.partition_point(|r| (r.archived_at, r.id) < key);
        let hi = self.archive.partition_point(|r| (r.archived_at, r.id) <= key);
        let at = match hi - lo {
            0 => lo,
            _ => {
                let enc = row.to_canonical();
                lo + self.archive[lo..hi].partition_point(|r| r.to_canonical() <= enc)
            }
        };
        self.archive.insert(at, row);
        at
    }

    /// The index of the first row in the archive order with this key
    /// (core.md §6.11).
    pub(crate) fn find_archive_row(&self, key: &ArchiveKey) -> Option<usize> {
        let k = (key.archived_at, key.id);
        let at = self.archive.partition_point(|r| (r.archived_at, r.id) < k);
        self.archive.get(at).filter(|r| (r.archived_at, r.id) == k).map(|_| at)
    }
}

#[cfg(test)]
mod tests;
