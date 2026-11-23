//! Log commands v1 (core.md §9, §20).
//!
//! `parse_command` runs the seven steps of §9.1 in order: the length, JSON,
//! duplicate keys (canon's `parse_value_steps`), `op`, the command's schema
//! (one reader per command over `canon::Obj`), canonical bytes (the parsed
//! command encoded once and compared), then the "Schema rule"s. A command is a
//! canonical record (§4.3): a field at its default and an optional field that
//! is none are absent, so a default written out fails step 6.

use serde_json::{Map, Value};

use crate::CoreError;
use crate::board::PictureRef;
use crate::canon::{self, Obj, Record, schema};
use crate::time::Instant;

/// The byte limit of a log command (core.md §9.1, §17; `seal.md` §10.2).
pub(crate) const COMMAND_LIMIT: usize = 257_979;

const UNKNOWN_OP: &str = "core/unknown_op";

/// A log command (core.md §9.3).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum LogCommand {
    CreateTask(CreateTask),
    CreateEvent(CreateEvent),
    CreateRoutine(CreateRoutine),
    Rename { id: u64, name: String },
    Complete { id: u64, at: Instant },
    Delete { id: u64, at: Instant },
    Forget { id: u64 },
    MoveBlock { id: u64, sid: u64, start: Instant, minutes: u32 },
    MoveBlocks { moves: Vec<BlockMove> },
    AddBlock { id: u64, sid: u64, start: Instant, minutes: u32 },
    RemoveBlocks { blocks: Vec<(u64, u64)> },
    SetEstimate { id: u64, minutes: u32 },
    SetDeadline { id: u64, deadline: Option<Instant>, seed: Option<u8> },
    SetSeverity { id: u64, level: u8 },
    SetHorizon { id: u64, level: u8 },
    SetRepeat { id: u64, days: u8, anchor: chrono::NaiveDate },
    MoveRoutine { id: u64, start_minutes: i32, minutes: u32 },
    SetEventSpan { id: u64, start: Instant, minutes: u32 },
    SetNotes { text: String, base: u64 },
    Restore { id: u64, archived_at: Instant, fallback: u64 },
    ForgetArchived { id: u64, archived_at: Instant },
    AddSubscription { id: u64, name: String, url: String, color: [u8; 4] },
    RemoveSubscription { id: u64 },
    RenameSubscription { id: u64, name: String },
    SetSubscriptionColor { id: u64, color: [u8; 4] },
    SetSubscriptionEnabled { id: u64, enabled: bool },
    SetPalette { colors: [[u8; 4]; 6] },
    SetHome { lat: i32, lon: i32 },
    SetPicture { picture: Option<PictureRef> },
}

/// `create_task` (core.md §9.4).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct CreateTask {
    pub id: u64,
    pub name: String,
    pub deadline: Option<Instant>,
    pub importance: Option<u8>,
    pub time_importance: Option<u8>,
    pub duration_minutes: Option<u32>,
    pub blocks: Vec<NewBlock>,
}

/// `create_event` (core.md §9.5).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct CreateEvent {
    pub id: u64,
    pub name: String,
    pub start: Instant,
    pub minutes: Option<u32>,
    pub importance: Option<u8>,
    pub time_importance: Option<u8>,
}

/// `create_routine` (core.md §9.6).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct CreateRoutine {
    pub id: u64,
    pub name: String,
    pub days: u8,
    pub anchor: chrono::NaiveDate,
    pub start_minutes: i32,
    pub minutes: u32,
}

/// A block of `create_task` (core.md §9.2, §9.4).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NewBlock {
    pub sid: u64,
    pub start: Instant,
    pub minutes: u32,
}

/// One move of `move_blocks` (core.md §9.11).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BlockMove {
    pub id: u64,
    pub sid: u64,
    pub start: Instant,
    pub minutes: u32,
}

// ---------------------------------------------------------------- limits (§9, §17)

/// `name`: 1 to 200 scalars (core.md §9.2).
const NAME_SCALARS: usize = 200;
/// `url` of `add_subscription`: 9 to 2,000 scalars (core.md §9.23).
const URL_MIN_SCALARS: usize = 9;
const URL_MAX_SCALARS: usize = 2000;
/// `text` of `set_notes`: at most 20,000 scalars (core.md §9.20).
const NOTES_SCALARS: usize = 20_000;
/// Blocks of `create_task`, moves, pairs: at most 1,000 (core.md §9.4, §9.11, §9.13).
const MAX_ENTRIES: usize = 1000;
/// `len`: 15 to 1,440 minutes (core.md §9.2).
const LEN_MIN: i64 = 15;
const LEN_MAX: i64 = 1440;
/// `day_start`: 0 to 1,425 (core.md §9.2).
const DAY_START_MAX: i64 = 1425;
/// The minutes of a day, the bound of the routine schema rule (core.md §9.6, §9.18).
const DAY_MINUTES: i64 = 1440;
/// One part of a picture blob (core.md §9.24, §6.9).
const PICTURE_PART: u64 = 1_048_576;

impl LogCommand {
    /// The command's `op` string (core.md §9.1).
    pub fn op(&self) -> &'static str {
        match self {
            LogCommand::CreateTask(_) => "create_task",
            LogCommand::CreateEvent(_) => "create_event",
            LogCommand::CreateRoutine(_) => "create_routine",
            LogCommand::Rename { .. } => "rename",
            LogCommand::Complete { .. } => "complete",
            LogCommand::Delete { .. } => "delete",
            LogCommand::Forget { .. } => "forget",
            LogCommand::MoveBlock { .. } => "move_block",
            LogCommand::MoveBlocks { .. } => "move_blocks",
            LogCommand::AddBlock { .. } => "add_block",
            LogCommand::RemoveBlocks { .. } => "remove_blocks",
            LogCommand::SetEstimate { .. } => "set_estimate",
            LogCommand::SetDeadline { .. } => "set_deadline",
            LogCommand::SetSeverity { .. } => "set_severity",
            LogCommand::SetHorizon { .. } => "set_horizon",
            LogCommand::SetRepeat { .. } => "set_repeat",
            LogCommand::MoveRoutine { .. } => "move_routine",
            LogCommand::SetEventSpan { .. } => "set_event_span",
            LogCommand::SetNotes { .. } => "set_notes",
            LogCommand::Restore { .. } => "restore",
            LogCommand::ForgetArchived { .. } => "forget_archived",
            LogCommand::AddSubscription { .. } => "add_subscription",
            LogCommand::RemoveSubscription { .. } => "remove_subscription",
            LogCommand::RenameSubscription { .. } => "rename_subscription",
            LogCommand::SetSubscriptionColor { .. } => "set_subscription_color",
            LogCommand::SetSubscriptionEnabled { .. } => "set_subscription_enabled",
            LogCommand::SetPalette { .. } => "set_palette",
            LogCommand::SetHome { .. } => "set_home",
            LogCommand::SetPicture { .. } => "set_picture",
        }
    }

    /// The command's canonical bytes (core.md §9.1, §4.2).
    pub fn to_canonical(&self) -> Vec<u8> {
        // The value holds no null and no float, so `encode` cannot fail.
        canon::encode(&self.to_value()).unwrap_or_default()
    }

    /// A `create_*` with another name, else `Err(self)` (core.md §20, V2-17).
    pub fn with_name(self, name: String) -> Result<LogCommand, LogCommand> {
        match self {
            LogCommand::CreateTask(c) => Ok(LogCommand::CreateTask(CreateTask { name, ..c })),
            LogCommand::CreateEvent(c) => Ok(LogCommand::CreateEvent(CreateEvent { name, ..c })),
            LogCommand::CreateRoutine(c) => Ok(LogCommand::CreateRoutine(CreateRoutine { name, ..c })),
            other => Err(other),
        }
    }

    /// The item or subscription the command targets (core.md §9.3): the
    /// command's `id` field where it has one (for a create, the id created),
    /// else none.
    pub fn target(&self) -> Option<u64> {
        match self {
            LogCommand::CreateTask(c) => Some(c.id),
            LogCommand::CreateEvent(c) => Some(c.id),
            LogCommand::CreateRoutine(c) => Some(c.id),
            LogCommand::Rename { id, .. }
            | LogCommand::Complete { id, .. }
            | LogCommand::Delete { id, .. }
            | LogCommand::Forget { id }
            | LogCommand::MoveBlock { id, .. }
            | LogCommand::AddBlock { id, .. }
            | LogCommand::SetEstimate { id, .. }
            | LogCommand::SetDeadline { id, .. }
            | LogCommand::SetSeverity { id, .. }
            | LogCommand::SetHorizon { id, .. }
            | LogCommand::SetRepeat { id, .. }
            | LogCommand::MoveRoutine { id, .. }
            | LogCommand::SetEventSpan { id, .. }
            // `restore` and `forget_archived`: their `id`, the archive row's
            // (core.md §10.7 `target`).
            | LogCommand::Restore { id, .. }
            | LogCommand::ForgetArchived { id, .. }
            | LogCommand::AddSubscription { id, .. }
            | LogCommand::RemoveSubscription { id }
            | LogCommand::RenameSubscription { id, .. }
            | LogCommand::SetSubscriptionColor { id, .. }
            | LogCommand::SetSubscriptionEnabled { id, .. } => Some(*id),
            LogCommand::MoveBlocks { .. }
            | LogCommand::RemoveBlocks { .. }
            | LogCommand::SetNotes { .. }
            | LogCommand::SetPalette { .. }
            | LogCommand::SetHome { .. }
            | LogCommand::SetPicture { .. } => None,
        }
    }

    /// The object the command's schema maps it to (core.md §4.3, §9.4–§9.24):
    /// defaults and nones absent.
    fn to_value(&self) -> Value {
        let mut m = Map::new();
        put(&mut m, "op", Value::from(self.op()));
        match self {
            LogCommand::CreateTask(c) => {
                put(&mut m, "id", Value::from(c.id));
                put(&mut m, "name", Value::from(c.name.as_str()));
                put_opt(&mut m, "deadline", c.deadline.map(instant_value));
                put_opt(&mut m, "importance", c.importance.map(Value::from));
                put_opt(&mut m, "time_importance", c.time_importance.map(Value::from));
                put_opt(&mut m, "duration_minutes", c.duration_minutes.map(Value::from));
                if !c.blocks.is_empty() {
                    let blocks = c.blocks.iter().map(|b| {
                        let mut o = Map::new();
                        put(&mut o, "minutes", Value::from(b.minutes));
                        put(&mut o, "sid", Value::from(b.sid));
                        put(&mut o, "start", instant_value(b.start));
                        Value::Object(o)
                    });
                    put(&mut m, "blocks", Value::Array(blocks.collect()));
                }
            }
            LogCommand::CreateEvent(c) => {
                put(&mut m, "id", Value::from(c.id));
                put(&mut m, "name", Value::from(c.name.as_str()));
                put(&mut m, "start", instant_value(c.start));
                put_opt(&mut m, "minutes", c.minutes.map(Value::from));
                put_opt(&mut m, "importance", c.importance.map(Value::from));
                put_opt(&mut m, "time_importance", c.time_importance.map(Value::from));
            }
            LogCommand::CreateRoutine(c) => {
                put(&mut m, "id", Value::from(c.id));
                put(&mut m, "name", Value::from(c.name.as_str()));
                put(&mut m, "days", Value::from(c.days));
                put(&mut m, "anchor", Value::from(canon::date_text(c.anchor)));
                put(&mut m, "start_minutes", Value::from(c.start_minutes));
                put(&mut m, "minutes", Value::from(c.minutes));
            }
            LogCommand::Rename { id, name } | LogCommand::RenameSubscription { id, name } => {
                put(&mut m, "id", Value::from(*id));
                put(&mut m, "name", Value::from(name.as_str()));
            }
            LogCommand::Complete { id, at } | LogCommand::Delete { id, at } => {
                put(&mut m, "id", Value::from(*id));
                put(&mut m, "at", instant_value(*at));
            }
            LogCommand::Forget { id } | LogCommand::RemoveSubscription { id } => {
                put(&mut m, "id", Value::from(*id));
            }
            LogCommand::MoveBlock { id, sid, start, minutes } | LogCommand::AddBlock { id, sid, start, minutes } => {
                put(&mut m, "id", Value::from(*id));
                put(&mut m, "sid", Value::from(*sid));
                put(&mut m, "start", instant_value(*start));
                put(&mut m, "minutes", Value::from(*minutes));
            }
            LogCommand::MoveBlocks { moves } => {
                let moves = moves.iter().map(|mv| {
                    let mut o = Map::new();
                    put(&mut o, "id", Value::from(mv.id));
                    put(&mut o, "minutes", Value::from(mv.minutes));
                    put(&mut o, "sid", Value::from(mv.sid));
                    put(&mut o, "start", instant_value(mv.start));
                    Value::Object(o)
                });
                put(&mut m, "moves", Value::Array(moves.collect()));
            }
            LogCommand::RemoveBlocks { blocks } => {
                let pairs = blocks.iter().map(|&(id, sid)| {
                    let mut o = Map::new();
                    put(&mut o, "id", Value::from(id));
                    put(&mut o, "sid", Value::from(sid));
                    Value::Object(o)
                });
                put(&mut m, "blocks", Value::Array(pairs.collect()));
            }
            LogCommand::SetEstimate { id, minutes } => {
                put(&mut m, "id", Value::from(*id));
                put(&mut m, "minutes", Value::from(*minutes));
            }
            LogCommand::SetDeadline { id, deadline, seed } => {
                put(&mut m, "id", Value::from(*id));
                put_opt(&mut m, "deadline", deadline.map(instant_value));
                put_opt(&mut m, "seed", seed.map(Value::from));
            }
            LogCommand::SetSeverity { id, level } | LogCommand::SetHorizon { id, level } => {
                put(&mut m, "id", Value::from(*id));
                put(&mut m, "level", Value::from(*level));
            }
            LogCommand::SetRepeat { id, days, anchor } => {
                put(&mut m, "id", Value::from(*id));
                put(&mut m, "days", Value::from(*days));
                put(&mut m, "anchor", Value::from(canon::date_text(*anchor)));
            }
            LogCommand::MoveRoutine { id, start_minutes, minutes } => {
                put(&mut m, "id", Value::from(*id));
                put(&mut m, "start_minutes", Value::from(*start_minutes));
                put(&mut m, "minutes", Value::from(*minutes));
            }
            LogCommand::SetEventSpan { id, start, minutes } => {
                put(&mut m, "id", Value::from(*id));
                put(&mut m, "start", instant_value(*start));
                put(&mut m, "minutes", Value::from(*minutes));
            }
            LogCommand::SetNotes { text, base } => {
                if !text.is_empty() {
                    put(&mut m, "text", Value::from(text.as_str()));
                }
                if *base != 0 {
                    put(&mut m, "base", Value::from(*base));
                }
            }
            LogCommand::Restore { id, archived_at, fallback } => {
                put(&mut m, "id", Value::from(*id));
                put(&mut m, "archived_at", instant_value(*archived_at));
                put(&mut m, "fallback", Value::from(*fallback));
            }
            LogCommand::ForgetArchived { id, archived_at } => {
                put(&mut m, "id", Value::from(*id));
                put(&mut m, "archived_at", instant_value(*archived_at));
            }
            LogCommand::AddSubscription { id, name, url, color } => {
                put(&mut m, "id", Value::from(*id));
                put(&mut m, "name", Value::from(name.as_str()));
                put(&mut m, "url", Value::from(url.as_str()));
                put(&mut m, "color", canon::rgba_value(*color));
            }
            LogCommand::SetSubscriptionColor { id, color } => {
                put(&mut m, "id", Value::from(*id));
                put(&mut m, "color", canon::rgba_value(*color));
            }
            LogCommand::SetSubscriptionEnabled { id, enabled } => {
                put(&mut m, "id", Value::from(*id));
                put(&mut m, "enabled", Value::Bool(*enabled));
            }
            LogCommand::SetPalette { colors } => {
                put(&mut m, "colors", Value::Array(colors.iter().map(|&c| canon::rgba_value(c)).collect()));
            }
            LogCommand::SetHome { lat, lon } => {
                put(&mut m, "lat", Value::from(*lat));
                put(&mut m, "lon", Value::from(*lon));
            }
            LogCommand::SetPicture { picture } => {
                put_opt(&mut m, "picture", picture.map(|p| p.to_value()));
            }
        }
        Value::Object(m)
    }

    /// Step 7 of §9.1: the cross-field "Schema rule"s (§9.6, §9.18, §9.24).
    fn check_schema_rules(&self) -> Result<(), CoreError> {
        match self {
            LogCommand::CreateRoutine(CreateRoutine { start_minutes, minutes, .. })
            | LogCommand::MoveRoutine { start_minutes, minutes, .. } => {
                if i64::from(*start_minutes) + i64::from(*minutes) <= DAY_MINUTES { Ok(()) } else { Err(schema()) }
            }
            LogCommand::SetPicture { picture: Some(p) } => {
                if u64::from(p.parts) == p.size.div_ceil(PICTURE_PART) { Ok(()) } else { Err(schema()) }
            }
            _ => Ok(()),
        }
    }
}

fn put(m: &mut Map<String, Value>, key: &str, v: Value) {
    m.insert(key.to_owned(), v);
}

fn put_opt(m: &mut Map<String, Value>, key: &str, v: Option<Value>) {
    if let Some(v) = v {
        put(m, key, v);
    }
}

fn instant_value(t: Instant) -> Value {
    Value::String(t.to_canonical())
}

/// Parses a command; `apply` checks the rules (core.md §9.1).
pub fn parse_command(bytes: &[u8]) -> Result<LogCommand, CoreError> {
    // Steps 1 to 3: the length before anything is allocated (CORE-GEN-009),
    // then JSON, then duplicate keys. The step-5 value findings (`null`, a
    // float, an integer outside ±(2^53 − 1)) are held back as `bad`.
    let (value, bad) = canon::parse_value_steps(bytes, COMMAND_LIMIT)?;
    // Step 4: an object with a string `op` naming a command of §9.3.
    let op = match &value {
        Value::Object(m) => m.get("op").and_then(Value::as_str),
        _ => None,
    };
    let reader = op.and_then(reader_for).ok_or(CoreError::code(UNKNOWN_OP))?;
    // Step 5: the command's schema.
    if bad {
        return Err(schema());
    }
    let mut o = Obj::new(&value)?;
    o.req("op")?;
    let command = reader(&mut o)?;
    o.done()?;
    drop(value);
    // Step 6: the canonical bytes.
    if command.to_canonical() != bytes {
        return Err(CoreError::code(canon::NOT_CANONICAL));
    }
    // Step 7: the schema rules.
    command.check_schema_rules()?;
    Ok(command)
}

// ---------------------------------------------------------------- step 5 readers

type Reader = fn(&mut Obj<'_>) -> Result<LogCommand, CoreError>;

/// The schema reader of the command `op` names, or none (§9.3, CORE-CMD-002).
fn reader_for(op: &str) -> Option<Reader> {
    let r: Reader = match op {
        "create_task" => read_create_task,
        "create_event" => read_create_event,
        "create_routine" => read_create_routine,
        "rename" => |o| Ok(LogCommand::Rename { id: id(o, "id")?, name: name(o.req("name")?)? }),
        "complete" => |o| Ok(LogCommand::Complete { id: id(o, "id")?, at: canon::instant(o.req("at")?)? }),
        "delete" => |o| Ok(LogCommand::Delete { id: id(o, "id")?, at: canon::instant(o.req("at")?)? }),
        "forget" => |o| Ok(LogCommand::Forget { id: id(o, "id")? }),
        "move_block" => |o| {
            Ok(LogCommand::MoveBlock {
                id: id(o, "id")?,
                sid: id(o, "sid")?,
                start: canon::instant(o.req("start")?)?,
                minutes: len(o.req("minutes")?)?,
            })
        },
        "move_blocks" => read_move_blocks,
        "add_block" => |o| {
            Ok(LogCommand::AddBlock {
                id: id(o, "id")?,
                sid: id(o, "sid")?,
                start: canon::instant(o.req("start")?)?,
                minutes: len(o.req("minutes")?)?,
            })
        },
        "remove_blocks" => read_remove_blocks,
        "set_estimate" => |o| Ok(LogCommand::SetEstimate { id: id(o, "id")?, minutes: len(o.req("minutes")?)? }),
        "set_deadline" => |o| {
            Ok(LogCommand::SetDeadline {
                id: id(o, "id")?,
                deadline: o.opt("deadline").map(canon::instant).transpose()?,
                seed: o.opt("seed").map(|v| small(v, 0, 4)).transpose()?,
            })
        },
        "set_severity" => |o| Ok(LogCommand::SetSeverity { id: id(o, "id")?, level: small(o.req("level")?, 0, 4)? }),
        "set_horizon" => |o| Ok(LogCommand::SetHorizon { id: id(o, "id")?, level: small(o.req("level")?, 0, 3)? }),
        "set_repeat" => |o| {
            Ok(LogCommand::SetRepeat {
                id: id(o, "id")?,
                days: small(o.req("days")?, 0, 127)?,
                anchor: canon::date(o.req("anchor")?)?,
            })
        },
        "move_routine" => |o| {
            Ok(LogCommand::MoveRoutine {
                id: id(o, "id")?,
                start_minutes: small(o.req("start_minutes")?, 0, DAY_START_MAX)?,
                minutes: len(o.req("minutes")?)?,
            })
        },
        "set_event_span" => |o| {
            Ok(LogCommand::SetEventSpan {
                id: id(o, "id")?,
                start: canon::instant(o.req("start")?)?,
                minutes: len(o.req("minutes")?)?,
            })
        },
        "set_notes" => read_set_notes,
        "restore" => |o| {
            Ok(LogCommand::Restore {
                id: id(o, "id")?,
                archived_at: canon::instant(o.req("archived_at")?)?,
                fallback: id(o, "fallback")?,
            })
        },
        "forget_archived" => |o| {
            Ok(LogCommand::ForgetArchived { id: id(o, "id")?, archived_at: canon::instant(o.req("archived_at")?)? })
        },
        "add_subscription" => |o| {
            Ok(LogCommand::AddSubscription {
                id: id(o, "id")?,
                name: name(o.req("name")?)?,
                url: url(o.req("url")?)?,
                color: canon::rgba(o.req("color")?)?,
            })
        },
        "remove_subscription" => |o| Ok(LogCommand::RemoveSubscription { id: id(o, "id")? }),
        "rename_subscription" => |o| Ok(LogCommand::RenameSubscription { id: id(o, "id")?, name: name(o.req("name")?)? }),
        "set_subscription_color" => {
            |o| Ok(LogCommand::SetSubscriptionColor { id: id(o, "id")?, color: canon::rgba(o.req("color")?)? })
        }
        "set_subscription_enabled" => {
            |o| Ok(LogCommand::SetSubscriptionEnabled { id: id(o, "id")?, enabled: canon::boolean(o.req("enabled")?)? })
        }
        "set_palette" => read_set_palette,
        "set_home" => |o| {
            Ok(LogCommand::SetHome { lat: small(o.req("lat")?, -9000, 9000)?, lon: small(o.req("lon")?, -18000, 18000)? })
        },
        "set_picture" => |o| Ok(LogCommand::SetPicture { picture: o.opt("picture").map(PictureRef::from_value).transpose()? }),
        _ => return None,
    };
    Some(r)
}

fn read_create_task(o: &mut Obj<'_>) -> Result<LogCommand, CoreError> {
    let item = id(o, "id")?;
    let name = name(o.req("name")?)?;
    let deadline = o.opt("deadline").map(canon::instant).transpose()?;
    let importance = o.opt("importance").map(|v| small(v, 0, 4)).transpose()?;
    let time_importance = o.opt("time_importance").map(|v| small(v, 0, 3)).transpose()?;
    let duration_minutes = o.opt("duration_minutes").map(len).transpose()?;
    let blocks = match o.opt("blocks") {
        None => Vec::new(),
        Some(v) => {
            // `blocks` of 0 to 1,000, ascending by `sid`, distinct (§9.4). An
            // empty array present is valid here and fails step 6.
            let a = entries(v, 0)?;
            let mut out: Vec<NewBlock> = Vec::with_capacity(a.len());
            for e in a {
                let mut b = Obj::new(e)?;
                let block =
                    NewBlock { minutes: len(b.req("minutes")?)?, sid: id(&mut b, "sid")?, start: canon::instant(b.req("start")?)? };
                b.done()?;
                if out.last().is_some_and(|p| p.sid >= block.sid) {
                    return Err(schema());
                }
                out.push(block);
            }
            out
        }
    };
    Ok(LogCommand::CreateTask(CreateTask { id: item, name, deadline, importance, time_importance, duration_minutes, blocks }))
}

fn read_create_event(o: &mut Obj<'_>) -> Result<LogCommand, CoreError> {
    Ok(LogCommand::CreateEvent(CreateEvent {
        id: id(o, "id")?,
        name: name(o.req("name")?)?,
        start: canon::instant(o.req("start")?)?,
        minutes: o.opt("minutes").map(len).transpose()?,
        importance: o.opt("importance").map(|v| small(v, 0, 4)).transpose()?,
        time_importance: o.opt("time_importance").map(|v| small(v, 0, 3)).transpose()?,
    }))
}

fn read_create_routine(o: &mut Obj<'_>) -> Result<LogCommand, CoreError> {
    Ok(LogCommand::CreateRoutine(CreateRoutine {
        id: id(o, "id")?,
        name: name(o.req("name")?)?,
        days: small(o.req("days")?, 0, 127)?,
        anchor: canon::date(o.req("anchor")?)?,
        start_minutes: small(o.req("start_minutes")?, 0, DAY_START_MAX)?,
        minutes: len(o.req("minutes")?)?,
    }))
}

fn read_move_blocks(o: &mut Obj<'_>) -> Result<LogCommand, CoreError> {
    // `moves` of 1 to 1,000 in the origin's order, `sid`s distinct (§9.11).
    let a = entries(o.req("moves")?, 1)?;
    let mut moves = Vec::with_capacity(a.len());
    for e in a {
        let mut m = Obj::new(e)?;
        let mv = BlockMove {
            id: id(&mut m, "id")?,
            minutes: small(m.req("minutes")?, 0, DAY_MINUTES)?,
            sid: id(&mut m, "sid")?,
            start: canon::instant(m.req("start")?)?,
        };
        m.done()?;
        moves.push(mv);
    }
    let mut sids: Vec<u64> = moves.iter().map(|m| m.sid).collect();
    sids.sort_unstable();
    if sids.windows(2).any(|w| w[0] == w[1]) {
        return Err(schema());
    }
    Ok(LogCommand::MoveBlocks { moves })
}

fn read_remove_blocks(o: &mut Obj<'_>) -> Result<LogCommand, CoreError> {
    // `blocks` of 1 to 1,000 `{id, sid}`, ascending by `sid`, distinct (§9.13).
    let a = entries(o.req("blocks")?, 1)?;
    let mut blocks: Vec<(u64, u64)> = Vec::with_capacity(a.len());
    for e in a {
        let mut p = Obj::new(e)?;
        let pair = (id(&mut p, "id")?, id(&mut p, "sid")?);
        p.done()?;
        if blocks.last().is_some_and(|q| q.1 >= pair.1) {
            return Err(schema());
        }
        blocks.push(pair);
    }
    Ok(LogCommand::RemoveBlocks { blocks })
}

fn read_set_notes(o: &mut Obj<'_>) -> Result<LogCommand, CoreError> {
    let base = o.opt("base").map(canon::u53).transpose()?.unwrap_or(0);
    let text = match o.opt("text") {
        None => String::new(),
        Some(v) => {
            // At most 20,000 scalars and no U+0009 (§9.20). A present `""` is
            // valid here and fails step 6.
            let s = scalars(v, 0, NOTES_SCALARS)?;
            if s.contains('\t') {
                return Err(schema());
            }
            s.to_owned()
        }
    };
    Ok(LogCommand::SetNotes { text, base })
}

fn read_set_palette(o: &mut Obj<'_>) -> Result<LogCommand, CoreError> {
    let a = canon::array(o.req("colors")?)?;
    if a.len() != 6 {
        return Err(schema());
    }
    let mut colors = [[0u8; 4]; 6];
    for (c, v) in colors.iter_mut().zip(a) {
        *c = canon::rgba(v)?;
    }
    Ok(LogCommand::SetPalette { colors })
}

// ---------------------------------------------------------------- field types (§4.4, §9.2)

/// An `id` field (core.md §4.4): 0 to 2^53 − 1.
fn id(o: &mut Obj<'_>, key: &str) -> Result<u64, CoreError> {
    canon::u53(o.req(key)?)
}

/// An `int[lo..hi]` converted to its Rust type.
fn small<T: TryFrom<i64>>(v: &Value, lo: i64, hi: i64) -> Result<T, CoreError> {
    T::try_from(canon::int(v, lo, hi)?).map_err(|_| schema())
}

/// A `len` (core.md §9.2): 15 to 1,440 minutes.
fn len(v: &Value) -> Result<u32, CoreError> {
    small(v, LEN_MIN, LEN_MAX)
}

/// A string of `lo` to `hi` scalars. The byte length bounds the scalar count
/// from both sides (1 to 4 bytes a scalar), so a string far over `hi` is
/// rejected without counting.
fn scalars(v: &Value, lo: usize, hi: usize) -> Result<&str, CoreError> {
    let s = canon::string(v)?;
    if s.len() < lo || s.len() > hi.saturating_mul(4) {
        return Err(schema());
    }
    if (lo..=hi).contains(&s.chars().count()) { Ok(s) } else { Err(schema()) }
}

/// A `name` (core.md §9.2): `text[1..200]`, nothing else checked (V2-1).
fn name(v: &Value) -> Result<String, CoreError> {
    scalars(v, 1, NAME_SCALARS).map(str::to_owned)
}

/// The `url` of `add_subscription` (core.md §9.23): 9 to 2,000 scalars
/// beginning with the 8 bytes `https://`.
fn url(v: &Value) -> Result<String, CoreError> {
    let s = scalars(v, URL_MIN_SCALARS, URL_MAX_SCALARS)?;
    if s.as_bytes().starts_with(b"https://") { Ok(s.to_owned()) } else { Err(schema()) }
}

/// An array of `lo` to 1,000 entries (core.md §9.4, §9.11, §9.13).
fn entries(v: &Value, lo: usize) -> Result<&Vec<Value>, CoreError> {
    let a = canon::array(v)?;
    if (lo..=MAX_ENTRIES).contains(&a.len()) { Ok(a) } else { Err(schema()) }
}

#[cfg(test)]
mod tests;
