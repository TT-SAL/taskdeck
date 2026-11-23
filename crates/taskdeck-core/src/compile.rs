//! The compile step (core.md §13, §20).
//!
//! All policy lives here (§13.2): defaults, trimming, detab, snapping and
//! clamping, zone conversion, Reflow and Unplan, ids. `compile` runs each row
//! of §13.4 in the order written and returns the first failure with the code,
//! status and message of §16 (CORE-COMP-001). It mints on a copy of the
//! minter and stores the copy back only on `Ok` (CORE-COMP-004). Every check
//! `apply` would make is made on `view` first (G6), so a returned command is
//! accepted by `apply` on that view (CORE-COMP-008).

use crate::CompileError;
use crate::board::{BoardData, Item, Kind, PictureRef};
use crate::command::{BlockMove, CreateEvent, CreateRoutine, CreateTask, LogCommand, NewBlock};
use crate::ids::IdMinter;
use crate::intent::{Intent, WireTime, intent_json, not_a_time};
use crate::overlay::{self, Overlay};
use crate::planner;
use crate::read;
use crate::time::{self, Instant, Zone};

/// The origin's inputs to the compile step (core.md §13.1).
pub struct CompileCtx<'a, Z: Zone + ?Sized> {
    pub view: &'a BoardData,
    pub zone: &'a Z,
    pub now: Instant,
    pub device_no: u16,
    pub overlay: &'a Overlay,
}

/// The compiled command, if any, and the reply (core.md §13.1).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Compiled {
    pub command: Option<LogCommand>,
    pub reply: Reply,
}

/// Today's reply, `session` a sid (core.md §13.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Reply {
    pub id: Option<u64>,
    pub session: Option<u64>,
    pub moved: Option<u32>,
}

// ---------------------------------------------------------------- limits (§13.4, §17)

/// A name after trim: at most 200 scalars (G2).
const NAME_SCALARS: usize = 200;
/// Notes after detab: at most 20,000 scalars (§13.4 `set_notes`).
const NOTES_SCALARS: usize = 20_000;
/// A URL after trim: at most 2,000 scalars (`checked_url`).
const URL_SCALARS: usize = 2000;
const MAX_ITEMS: usize = 10_000;
const MAX_BLOCKS_PER_ITEM: usize = 1000;
const MAX_LIVE_BLOCKS: usize = 50_000;
const MAX_ARCHIVE: usize = 200_000;
const MAX_SUBSCRIPTIONS: usize = 20;
/// Moves of one `move_blocks` (core.md §9.11).
const MAX_MOVES: usize = 1000;
/// Years a day or a naive time may fall in (G3, G4).
const FIRST_YEAR: i32 = 1970;
const LAST_YEAR: i32 = 9999;
/// The largest `u53`, which the canonical form can write (core.md §4.2 rule 5).
const MAX_U53: u64 = (1 << 53) - 1;
/// One part of a picture blob (core.md §6.9).
const PICTURE_PART: u64 = 1_048_576;
/// Today's `NEW_TASK_HORIZON` and `NEW_TASK_IMPORTANCE` (board.rs).
const NEW_TASK_HORIZON: u8 = 1;
const NEW_TASK_IMPORTANCE: u8 = 2;

/// Today's `subscriptions::DEFAULT_COLORS` (subscriptions.rs:60-67).
pub const DEFAULT_COLORS: [[u8; 4]; 6] = [
    [90, 140, 200, 255],
    [200, 120, 90, 255],
    [120, 175, 110, 255],
    [175, 120, 190, 255],
    [200, 175, 90, 255],
    [110, 175, 185, 255],
];

// ---------------------------------------------------------------- errors (§16)

/// The error of a code of §16 whose message is fixed: its status and today's text.
fn fail(code: &'static str) -> CompileError {
    let (status, message) = match code {
        "core/query" => (400, "That is a query, not a command."),
        "core/name_long" => (400, "A name is at most 200 characters."),
        "core/event_time" => (400, "An event needs a time."),
        "core/date_range" => (400, "That date is out of range (1970–9999)."),
        "core/severity" => (400, "No such severity."),
        "core/horizon" => (400, "No such horizon."),
        "core/item_gone" => (410, "That item is no longer on the board."),
        "core/block_gone" => (410, "That block is no longer there."),
        "core/bad_time" => (400, "That time doesn't exist on this day (daylight saving)."),
        "core/finish_kind" => (400, "Only a task can be finished — an event or a routine is deleted instead."),
        "core/block_kind" => (400, "Only a task can be given a block of time."),
        "core/unplan_kind" => (400, "Only a task can be unplanned."),
        "core/length_range" => (400, "A length is between fifteen minutes and a day."),
        "core/deadline_kind" => (400, "Only a task has a due date; an event's time is its time."),
        "core/severity_kind" => (400, "Only a task has a severity."),
        "core/horizon_kind" => (400, "Only a task has a horizon."),
        "core/repeat_kind" => (400, "Only a routine repeats."),
        "core/notes_long" => (400, "Those notes are too long for the notepad."),
        // SPEC-QUESTION: §16 has `core/notes_stale` only for apply (§9.20); compile refuses an intent `base` above 2^53 − 1 with it, status 400 and a new message.
        "core/notes_stale" => (400, "Those notes have changed since you started editing."),
        "core/archive_gone" => (410, "That is no longer in the archive."),
        "core/calendars_full" => (400, "That is already 20 calendars, which is as many as this keeps."),
        "core/url_empty" => (400, "An address is needed."),
        "core/url_long" => (400, "That address is too long."),
        "core/url_http" => (400, "Use https — a calendar link is a password, and http sends it in clear."),
        "core/url_scheme" => (400, "An address should start with https://"),
        "core/url_host" => (400, "That address has no host in it."),
        "core/url_duplicate" => (400, "That calendar is already subscribed to."),
        "core/calendar_gone" => (410, "That calendar is no longer subscribed to."),
        "core/cap_items" => (400, "That is as many items as a board can hold."),
        "core/cap_blocks" => (400, "That is as many blocks as a board can hold."),
        "core/cap_archive" => (400, "The archive is full."),
        "core/place" => (400, "That place is off the map."),
        "core/picture" => (400, "That picture could not be shared."),
        // SPEC-QUESTION: §16 has no compile code for a restore whose row's sid a live block holds (apply's `core/id_taken`, §9.21 step 4); this keeps apply's code with status 400 and a new message.
        "core/id_taken" => (400, "That item's blocks are already on the board."),
        // SPEC-QUESTION: §16 has no compile code for an op outside `view.rules`' op table (CORE-RULES-004); this uses apply's `core/rules` with status 400 and a new message.
        "core/rules" => (400, "This board's rules do not allow that yet."),
        // SPEC-QUESTION: §16 has no compile code for a command apply would refuse for size (§8.4 step 4, CORE-APPLY-005) though G6 and CORE-COMP-008 require compile to refuse it on `view`; this uses apply's `core/cap` with status 400 and a new message.
        "core/cap" => (400, "The board is full."),
        _ => (400, ""),
    };
    CompileError { code, status, message: message.to_owned() }
}

/// `core/name_empty`, whose message is the intent's (core.md §13.4, §16).
fn name_empty(message: &str) -> CompileError {
    CompileError { code: "core/name_empty", status: 400, message: message.to_owned() }
}

type R<T> = Result<T, CompileError>;

// ---------------------------------------------------------------- compile

/// Compiles an intent into a log command (core.md §13.3, §13.4).
pub fn compile<Z: Zone + ?Sized>(
    intent: &Intent,
    cx: &CompileCtx<Z>,
    minter: &mut IdMinter,
) -> Result<Compiled, CompileError> {
    // Minted on a copy, stored back only on `Ok` (CORE-COMP-004).
    let mut m = *minter;
    let out = Rows { cx, minter: &mut m }.row(intent)?;
    // G8: never an op outside the op table of `view.rules` (CORE-RULES-004).
    if let Some(c) = &out.command
        && !crate::apply::op_table(cx.view.rules).contains(&c.op())
    {
        return Err(fail("core/rules"));
    }
    // G6: apply's size check (§8.4 step 4) on `view`, so `apply` on `view`
    // does not refuse the command `core/cap` (CORE-COMP-008). The entry's
    // `seq` is not known here; the largest one writes the longest stamps, so
    // a command that fits under it fits under every `seq`, and `based_on` just
    // below it means the origin saw every change of `view`.
    if let Some(c) = &out.command {
        let seq = crate::apply::LAST_SEQ;
        let ctx = crate::apply::Ctx { seq, device_no: cx.device_no, at: cx.now, based_on: seq - 1 };
        if crate::apply::refused_for_size(cx.view, c, &ctx) {
            return Err(fail("core/cap"));
        }
    }
    *minter = m;
    Ok(out)
}

fn done(command: LogCommand) -> R<Compiled> {
    Ok(Compiled { command: Some(command), reply: Reply::default() })
}

fn nothing() -> R<Compiled> {
    Ok(Compiled { command: None, reply: Reply::default() })
}

fn with_id(command: LogCommand, id: u64) -> R<Compiled> {
    Ok(Compiled { command: Some(command), reply: Reply { id: Some(id), ..Reply::default() } })
}

/// The rows of §13.4 over one context and the minter's copy.
struct Rows<'c, 'a, Z: Zone + ?Sized> {
    cx: &'c CompileCtx<'a, Z>,
    minter: &'c mut IdMinter,
}

impl<'a, Z: Zone + ?Sized> Rows<'_, 'a, Z> {
    fn view(&self) -> &'a BoardData {
        self.cx.view
    }

    // ------------------------------------------------------------ the rules of §13.3

    /// G2: trimmed (G1); over 200 scalars is `core/name_long`. May be empty.
    fn name(&self, raw: &str) -> R<String> {
        let name = trim(raw);
        if name.chars().count() > NAME_SCALARS {
            return Err(fail("core/name_long"));
        }
        Ok(name.to_owned())
    }

    /// G4: a day whose year is outside 1970 to 9999 is `core/date_range`.
    fn day(&self, day: chrono::NaiveDate) -> R<chrono::NaiveDate> {
        use chrono::Datelike;
        if (FIRST_YEAR..=LAST_YEAR).contains(&day.year()) { Ok(day) } else { Err(fail("core/date_range")) }
    }

    /// G5 for an exact time: within §5.1's range on the UTC instant, else
    /// `core/date_range`.
    fn exact(&self, t: &chrono::DateTime<chrono::FixedOffset>) -> R<Instant> {
        Instant::from_chrono(t.with_timezone(&chrono::Utc)).ok_or_else(|| fail("core/date_range"))
    }

    /// G3 and G5 for a wire time (core.md §13.3): `Exact` converts; `Naive`
    /// resolves as today's `parse_local_input` in the origin's zone, seconds
    /// dropped (CORE-COMP-003).
    fn wire(&self, w: Option<&WireTime>) -> R<Option<Instant>> {
        use chrono::{Datelike, Timelike};
        match w {
            None => Ok(None),
            Some(WireTime::Exact(t)) => self.exact(t).map(Some),
            Some(WireTime::Naive(naive, text)) => {
                let minutes = i32::try_from(naive.hour() * 60 + naive.minute()).unwrap_or(0);
                time::resolve_on_day(naive.date(), minutes, self.cx.zone)
                    .filter(|&t| (FIRST_YEAR..=LAST_YEAR).contains(&time::local_day(t, self.cx.zone).year()))
                    .map(Some)
                    .ok_or_else(|| intent_json(not_a_time(text)))
            }
        }
    }

    /// `resolve_on_day` in the origin's zone; none is `core/bad_time`. Its
    /// instant is within §5.1's range by construction (G5).
    fn resolve(&self, day: chrono::NaiveDate, minutes: i32) -> R<Instant> {
        time::resolve_on_day(day, minutes, self.cx.zone).ok_or_else(|| fail("core/bad_time"))
    }

    /// "Missing" (core.md §13.4): the live item of `view` with `id`.
    fn item(&self, id: u64) -> R<&'a Item> {
        self.view().item(id).ok_or_else(|| fail("core/item_gone"))
    }

    /// The live-item cap (core.md §9.5): `core/cap_items`.
    fn cap_items(&self) -> R<()> {
        if self.view().items.len() < MAX_ITEMS { Ok(()) } else { Err(fail("core/cap_items")) }
    }

    fn live_blocks(&self) -> usize {
        self.view().items.iter().map(|i| i.blocks.len()).sum()
    }

    // SPEC-QUESTION: §13.4 says nothing of a minter with no number left (§7.1); this answers the cap of the kind minted, `core/cap_items` or `core/cap_blocks`.
    fn mint_item(&mut self) -> R<u64> {
        self.minter.mint_item(self.cx.view).ok_or_else(|| fail("core/cap_items"))
    }

    fn mint_block(&mut self) -> R<u64> {
        self.minter.mint_block(self.cx.view).ok_or_else(|| fail("core/cap_blocks"))
    }

    /// `at'` of `complete` and `delete`: `min(at, now + 1 day)`, or `now`
    /// when none (G9).
    fn archived_at(&self, at: Option<Instant>) -> Instant {
        let latest = self.cx.now.plus_seconds(86_400).unwrap_or(Instant::MAX);
        at.map_or(self.cx.now, |at| at.min(latest))
    }

    fn cap_archive(&self) -> R<()> {
        if self.view().archive.len() < MAX_ARCHIVE { Ok(()) } else { Err(fail("core/cap_archive")) }
    }

    // ------------------------------------------------------------ the rows of §13.4

    fn row(&mut self, intent: &Intent) -> R<Compiled> {
        match intent {
            Intent::Add { name, deadline, importance, is_event, time_importance } => {
                let name = self.name(name)?;
                if name.is_empty() {
                    return Err(name_empty("It needs a name."));
                }
                if *is_event && deadline.is_none() {
                    return Err(fail("core/event_time"));
                }
                let deadline = self.wire(deadline.as_ref())?;
                let time_importance = if !is_event && deadline.is_none() && importance.is_none() && time_importance.is_none()
                {
                    Some(NEW_TASK_HORIZON)
                } else {
                    *time_importance
                };
                if importance.is_some_and(|l| l >= 5) {
                    return Err(fail("core/severity"));
                }
                if time_importance.is_some_and(|l| l >= 4) {
                    return Err(fail("core/horizon"));
                }
                self.cap_items()?;
                let id = self.mint_item()?;
                let command = match deadline {
                    // An event has a time (`core/event_time` above).
                    Some(start) if *is_event => LogCommand::CreateEvent(CreateEvent {
                        id,
                        name,
                        start,
                        minutes: None,
                        importance: *importance,
                        time_importance,
                    }),
                    _ => LogCommand::CreateTask(CreateTask {
                        id,
                        name,
                        deadline,
                        importance: *importance,
                        time_importance,
                        duration_minutes: None,
                        blocks: Vec::new(),
                    }),
                };
                with_id(command, id)
            }

            Intent::QuickAdd { name, deadline } => {
                let name = self.name(name)?;
                if name.is_empty() {
                    return Err(name_empty("A task needs a name."));
                }
                let deadline = self.wire(deadline.as_ref())?;
                self.cap_items()?;
                let id = self.mint_item()?;
                let (importance, time_importance) =
                    if deadline.is_some() { (Some(NEW_TASK_IMPORTANCE), None) } else { (None, Some(NEW_TASK_HORIZON)) };
                let command = LogCommand::CreateTask(CreateTask {
                    id,
                    name,
                    deadline,
                    importance,
                    time_importance,
                    duration_minutes: None,
                    blocks: Vec::new(),
                });
                with_id(command, id)
            }

            Intent::Create { kind, name, day, start, minutes } => {
                let day = self.day(*day)?;
                let (start, minutes) = time::clamp_block(i64::from(*start), i64::from(*minutes));
                let when = match kind {
                    Kind::Task | Kind::Event => Some(self.resolve(day, start)?),
                    Kind::Routine => None,
                };
                let name = self.name(name)?;
                let name = if name.is_empty() {
                    match kind {
                        Kind::Task => "New task",
                        Kind::Event => "New event",
                        Kind::Routine => "New routine",
                    }
                    .to_owned()
                } else {
                    name
                };
                self.cap_items()?;
                if *kind == Kind::Task && self.live_blocks() >= MAX_LIVE_BLOCKS {
                    return Err(fail("core/cap_blocks"));
                }
                let id = self.mint_item()?;
                let command = match (kind, when) {
                    (Kind::Task, Some(when)) => {
                        let sid = self.mint_block()?;
                        LogCommand::CreateTask(CreateTask {
                            id,
                            name,
                            deadline: None,
                            importance: None,
                            time_importance: Some(NEW_TASK_HORIZON),
                            duration_minutes: Some(minutes),
                            blocks: vec![NewBlock { sid, start: when, minutes }],
                        })
                    }
                    (Kind::Event, Some(when)) => LogCommand::CreateEvent(CreateEvent {
                        id,
                        name,
                        start: when,
                        minutes: Some(minutes),
                        importance: None,
                        time_importance: None,
                    }),
                    _ => LogCommand::CreateRoutine(CreateRoutine {
                        id,
                        name,
                        days: 0,
                        anchor: day,
                        start_minutes: start,
                        minutes,
                    }),
                };
                with_id(command, id)
            }

            Intent::Rename { id, name } => {
                let name = self.name(name)?;
                if name.is_empty() {
                    return Err(name_empty("A name cannot be empty."));
                }
                self.item(*id)?;
                done(LogCommand::Rename { id: *id, name })
            }

            Intent::Complete { id, at } => {
                let item = self.item(*id)?;
                if item.kind != Kind::Task {
                    return Err(fail("core/finish_kind"));
                }
                let at = self.wire(at.as_ref())?;
                self.cap_archive()?;
                done(LogCommand::Complete { id: *id, at: self.archived_at(at) })
            }

            Intent::Delete { id, at } => {
                self.item(*id)?;
                let at = self.wire(at.as_ref())?;
                self.cap_archive()?;
                done(LogCommand::Delete { id: *id, at: self.archived_at(at) })
            }

            Intent::Forget { id } => {
                self.item(*id)?;
                done(LogCommand::Forget { id: *id })
            }

            Intent::MoveBlock { id, session, day, start, minutes } => {
                let day = self.day(*day)?;
                let item = self.item(*id)?;
                let kind = item.kind;
                let sid = match kind {
                    Kind::Task => match session {
                        Some(sid) if item.blocks.iter().any(|b| b.sid == *sid) => Some(*sid),
                        _ => return Err(fail("core/block_gone")),
                    },
                    Kind::Event | Kind::Routine => None,
                };
                let (start, minutes) = time::clamp_block(i64::from(*start), i64::from(*minutes));
                match (kind, sid) {
                    (Kind::Task, Some(sid)) => {
                        let when = self.resolve(day, start)?;
                        done(LogCommand::MoveBlock { id: *id, sid, start: when, minutes })
                    }
                    (Kind::Event, _) => {
                        let when = self.resolve(day, start)?;
                        done(LogCommand::SetEventSpan { id: *id, start: when, minutes })
                    }
                    _ => done(LogCommand::MoveRoutine { id: *id, start_minutes: start.clamp(0, 1439), minutes }),
                }
            }

            Intent::AddBlock { id, day, start, minutes } => {
                let day = self.day(*day)?;
                let item = self.item(*id)?;
                if item.kind != Kind::Task {
                    return Err(fail("core/block_kind"));
                }
                let length = minutes.unwrap_or_else(|| planner::drop_length_for(item));
                let (start, length) = time::clamp_block(i64::from(*start), i64::from(length));
                let when = self.resolve(day, start)?;
                if item.blocks.len() >= MAX_BLOCKS_PER_ITEM || self.live_blocks() >= MAX_LIVE_BLOCKS {
                    return Err(fail("core/cap_blocks"));
                }
                let sid = self.mint_block()?;
                let command = LogCommand::AddBlock { id: *id, sid, start: when, minutes: length.max(15) };
                Ok(Compiled { command: Some(command), reply: Reply { session: Some(sid), ..Reply::default() } })
            }

            Intent::RemoveBlock { id, session } => {
                let item = self.item(*id)?;
                if item.kind != Kind::Task || !item.blocks.iter().any(|b| b.sid == *session) {
                    return Err(fail("core/block_gone"));
                }
                done(LogCommand::RemoveBlocks { blocks: vec![(*id, *session)] })
            }

            Intent::Unplan { id } => {
                let item = self.item(*id)?;
                if item.kind != Kind::Task {
                    return Err(fail("core/unplan_kind"));
                }
                if item.blocks.is_empty() {
                    return nothing();
                }
                // Every block of the item in `view`, ascending by sid
                // (CORE-COMP-006): the order `blocks` is stored in (§6.12).
                let mut blocks: Vec<(u64, u64)> = item.blocks.iter().map(|b| (*id, b.sid)).collect();
                blocks.sort_by_key(|p| p.1);
                done(LogCommand::RemoveBlocks { blocks })
            }

            Intent::SetEstimate { id, minutes } => {
                if !(15..=1440).contains(minutes) {
                    return Err(fail("core/length_range"));
                }
                let item = self.item(*id)?;
                match (item.kind, item.recurrence, item.deadline) {
                    (Kind::Routine, Some(rule), _) => {
                        let (s, l) = time::clamp_block(i64::from(rule.start_minutes), i64::from(*minutes));
                        done(LogCommand::MoveRoutine { id: *id, start_minutes: s, minutes: l })
                    }
                    (Kind::Event, _, Some(d)) => {
                        let zone = self.cx.zone;
                        let (s, l) = time::clamp_block(i64::from(time::local_minutes(d, zone)), i64::from(*minutes));
                        let when = time::resolve_on_day(time::local_day(d, zone), s, zone).unwrap_or(d);
                        done(LogCommand::SetEventSpan { id: *id, start: when, minutes: l })
                    }
                    _ => done(LogCommand::SetEstimate { id: *id, minutes: *minutes }),
                }
            }

            Intent::SetDeadline { id, deadline } => {
                let deadline = self.wire(deadline.as_ref())?;
                let item = self.item(*id)?;
                if item.kind != Kind::Task {
                    return Err(fail("core/deadline_kind"));
                }
                let seed = (deadline.is_some() && item.importance.is_none()).then_some(NEW_TASK_IMPORTANCE);
                done(LogCommand::SetDeadline { id: *id, deadline, seed })
            }

            Intent::SetSeverity { id, level } => {
                if *level >= 5 {
                    return Err(fail("core/severity"));
                }
                if self.item(*id)?.kind != Kind::Task {
                    return Err(fail("core/severity_kind"));
                }
                done(LogCommand::SetSeverity { id: *id, level: *level })
            }

            Intent::SetHorizon { id, level } => {
                if *level >= 4 {
                    return Err(fail("core/horizon"));
                }
                if self.item(*id)?.kind != Kind::Task {
                    return Err(fail("core/horizon_kind"));
                }
                done(LogCommand::SetHorizon { id: *id, level: *level })
            }

            Intent::SetRepeat { id, days, day } => {
                let day = self.day(*day)?;
                let item = self.item(*id)?;
                let (Kind::Routine, Some(rule)) = (item.kind, item.recurrence) else {
                    return Err(fail("core/repeat_kind"));
                };
                let days = days & 127;
                let anchor = if days == 0 { day } else { rule.anchor };
                done(LogCommand::SetRepeat { id: *id, days, anchor })
            }

            Intent::Reflow { day, from } => {
                let day = self.day(*day)?;
                let f = match from {
                    Some(from) => (*from).clamp(0, 1440),
                    None => match time::now_marker(day, self.cx.now, self.cx.zone) {
                        Some(f) => f,
                        None => {
                            return Ok(Compiled { command: None, reply: Reply { moved: Some(0), ..Reply::default() } });
                        }
                    },
                };
                let moves = self.reflow(day, f);
                let moved = u32::try_from(moves.len()).unwrap_or(u32::MAX);
                let command = (!moves.is_empty()).then_some(LogCommand::MoveBlocks { moves });
                Ok(Compiled { command, reply: Reply { moved: Some(moved), ..Reply::default() } })
            }

            Intent::SetNotes { text, base } => {
                let t = detab(text);
                if t.chars().count() > NOTES_SCALARS {
                    return Err(fail("core/notes_long"));
                }
                // SPEC-QUESTION: an intent `base` other than `view.notes_rev` is emitted as given (§13.4), so `apply` on `view` refuses it `core/notes_stale` (§9.20), against CORE-COMP-008; and `view.notes_rev` at 2^53 − 1 has no compile code (apply: `core/cap`).
                let base = base.unwrap_or(self.view().notes_rev);
                // A base above 2^53 − 1 has no canonical encoding (§4.2 rule 5),
                // and it can never be `view.notes_rev` (a `u53`), so apply on
                // `view` would refuse it `core/notes_stale` (§9.20): refuse it here
                // with that code, so the command returned always encodes (CORE-COMP-008).
                // SPEC-QUESTION: §13.4 and §16 give no compile refusal for an intent `base` above 2^53 − 1; this uses apply's `core/notes_stale`.
                if base > MAX_U53 {
                    return Err(fail("core/notes_stale"));
                }
                done(LogCommand::SetNotes { text: t, base })
            }

            Intent::Restore { id, archived_at } => {
                let archived_at = self.exact(archived_at)?;
                // The first row in archive order with the key (§6.11): the
                // archive is stored in that order (§6.12).
                let row = self
                    .view()
                    .archive
                    .iter()
                    .find(|r| r.id == *id && r.archived_at == archived_at)
                    .ok_or_else(|| fail("core/archive_gone"))?;
                self.cap_items()?;
                // G6: the rest of apply's checks of §9.21 on `view`.
                if self.live_blocks() + row.blocks.len() > MAX_LIVE_BLOCKS {
                    return Err(fail("core/cap_blocks"));
                }
                let view = self.view();
                if row.blocks.iter().any(|rb| view.items.iter().any(|i| i.blocks.iter().any(|b| b.sid == rb.sid))) {
                    return Err(fail("core/id_taken"));
                }
                let own = (row.id != 0 && view.item(row.id).is_none()).then_some(row.id);
                let fallback = self.mint_item()?;
                let command = LogCommand::Restore { id: *id, archived_at, fallback };
                with_id(command, own.unwrap_or(fallback))
            }

            Intent::ForgetArchived { id, archived_at } => {
                let archived_at = self.exact(archived_at)?;
                if !self.view().archive.iter().any(|r| r.id == *id && r.archived_at == archived_at) {
                    return Err(fail("core/archive_gone"));
                }
                done(LogCommand::ForgetArchived { id: *id, archived_at })
            }

            Intent::AddSubscription { name, url, color } => {
                let n = self.view().subscriptions.len();
                if n >= MAX_SUBSCRIPTIONS {
                    return Err(fail("core/calendars_full"));
                }
                let u = checked_url(url)?;
                if self.view().subscriptions.iter().any(|s| s.url == u) {
                    return Err(fail("core/url_duplicate"));
                }
                let name = self.name(name)?;
                let id = self.mint_item()?;
                let name = if name.is_empty() {
                    overlay::name_from_url(&u)
                        .filter(|n| n.chars().count() <= NAME_SCALARS)
                        .unwrap_or_else(|| overlay::placeholder_name(id))
                } else {
                    name
                };
                let color = color.unwrap_or(DEFAULT_COLORS[n % DEFAULT_COLORS.len()]);
                with_id(LogCommand::AddSubscription { id, name, url: u, color }, id)
            }

            Intent::RemoveSubscription { id } => {
                self.subscription(*id)?;
                done(LogCommand::RemoveSubscription { id: *id })
            }

            Intent::RenameSubscription { id, name } => {
                let name = self.name(name)?;
                self.subscription(*id)?;
                if name.is_empty() {
                    return nothing();
                }
                done(LogCommand::RenameSubscription { id: *id, name })
            }

            Intent::SetSubscriptionColor { id, color } => {
                self.subscription(*id)?;
                done(LogCommand::SetSubscriptionColor { id: *id, color: *color })
            }

            Intent::SetSubscriptionEnabled { id, enabled } => {
                self.subscription(*id)?;
                done(LogCommand::SetSubscriptionEnabled { id: *id, enabled: *enabled })
            }

            Intent::SetPalette { colors } => done(LogCommand::SetPalette { colors: *colors }),

            Intent::SetHome { lat, lon } => {
                let ok = lat.is_finite() && lon.is_finite() && (-90.0..=90.0).contains(lat) && (-180.0..=180.0).contains(lon);
                if !ok {
                    return Err(fail("core/place"));
                }
                // Hundredths, rounded half away from zero (`f64::round`); within
                // ±9,000 and ±18,000 by the check above.
                let lat = (lat * 100.0).round() as i32;
                let lon = (lon * 100.0).round() as i32;
                done(LogCommand::SetHome { lat, lon })
            }

            Intent::SetPicture { picture } => {
                if let Some(p) = picture
                    && !picture_ok(p)
                {
                    return Err(fail("core/picture"));
                }
                done(LogCommand::SetPicture { picture: *picture })
            }
        }
    }

    fn subscription(&self, id: u64) -> R<()> {
        self.view().subscription(id).map(|_| ()).ok_or_else(|| fail("core/calendar_gone"))
    }

    /// Reflow's moves after `f` is known (core.md §13.4, steps 1 to 4).
    fn reflow(&self, day: chrono::NaiveDate, f: i32) -> Vec<BlockMove> {
        let zone = self.cx.zone;
        // Steps 1 and 2.
        let (flowables, anchors) = read::reflow_inputs(self.view(), self.cx.overlay, day, zone);
        // Step 3: a new start per flowable, in input order; ties in start
        // keep the input order, ascending `(id, sid)` (§13.5).
        let placed = planner::reflow(&flowables, &anchors, f);
        // Step 4.
        let mut moves = Vec::new();
        for (flowable, ((id, sid), m)) in flowables.iter().zip(placed) {
            if m == flowable.start {
                continue;
            }
            let Some(w) = time::resolve_on_day(day, m, zone) else { continue };
            let block = self.view().item(id).and_then(|i| i.blocks.iter().find(|b| b.sid == sid));
            if let Some(block) = block
                && w != block.start
            {
                moves.push(BlockMove { id, sid, start: w, minutes: block.minutes });
            }
        }
        // SPEC-QUESTION: `move_blocks` holds at most 1,000 moves (§9.11) and §13.4 does not say what a day with more does; this keeps the first 1,000 in input order.
        moves.truncate(MAX_MOVES);
        moves
    }
}

/// A `PictureRef` in the ranges of §6.9 with `parts = ceil(size / 1,048,576)`.
fn picture_ok(p: &PictureRef) -> bool {
    p.epoch >= 1
        && (1..=64).contains(&p.parts)
        && (67..=67_108_864).contains(&p.size)
        && u64::from(p.parts) == p.size.div_ceil(PICTURE_PART)
}

/// Today's subscription URL check (core.md §13.4).
///
/// Today's `subscriptions::checked_url` (subscriptions.rs:156-180), trimmed per
/// G1: trim; empty, over 2,000 scalars, `webcal://` to `https://`, `http://`,
/// not `https://`, no host or a host without a `.`.
pub fn checked_url(url: &str) -> Result<String, CompileError> {
    let url = trim(url);
    if url.is_empty() {
        return Err(fail("core/url_empty"));
    }
    if url.chars().count() > URL_SCALARS {
        return Err(fail("core/url_long"));
    }
    let url = match url.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_owned(),
    };
    if url.starts_with("http://") {
        return Err(fail("core/url_http"));
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return Err(fail("core/url_scheme"));
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() || !host.contains('.') {
        return Err(fail("core/url_host"));
    }
    Ok(url)
}

/// The 25 `White_Space` code points of core.md §2, by an explicit list: not
/// `str::trim`, whose table follows the toolchain's Unicode version
/// (CORE-COMP-002).
const WHITE_SPACE: [char; 25] = [
    '\u{0009}', '\u{000A}', '\u{000B}', '\u{000C}', '\u{000D}', '\u{0020}', '\u{0085}', '\u{00A0}',
    '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}',
    '\u{2007}', '\u{2008}', '\u{2009}', '\u{200A}', '\u{2028}', '\u{2029}', '\u{202F}', '\u{205F}',
    '\u{3000}',
];

/// Trims Unicode `White_Space` only (core.md §2, §13.3): every leading and
/// trailing code point of `WHITE_SPACE`, and nothing else (CORE-COMP-002).
pub fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| WHITE_SPACE.contains(&c))
}

/// Replaces every U+0009 by four U+0020 (core.md §2, §13.3), as today's
/// `utilities::detab` does.
pub fn detab(text: &str) -> String {
    text.replace('\t', "    ")
}

#[cfg(test)]
mod tests;
