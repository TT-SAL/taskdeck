//! The board: everything TaskDeck knows, and every way it changes.
//!
//! The live items, the archive, the notepad and the id counter live here, with
//! **one** entry point for changing any of it: [`Board::apply`] takes a
//! [`Command`] and either carries it out — and saves — or says why not. The
//! desktop GUI, the phone view and the headless server all go through it, so
//! there is exactly one place that knows what "book another block" or "put it
//! back" means, and a change made anywhere is the same change.
//!
//! Nothing in this module knows about egui, windows, threads or HTTP. That is
//! what lets it be unit-tested against a temporary directory, and what lets
//! `taskdeck-server` run it on a machine with no screen.
//!
//! ## Why commands
//!
//! A command is a small, named, serialisable intent — "move block 2 of task 17
//! to 14:30" — rather than a new copy of the file. That shape is what makes
//! everything downstream simple: the phone sends them over HTTP, a desktop that
//! talks to a server sends the same ones, an offline desktop can queue them
//! and replay them later, and a conflict is resolved per command rather than
//! by merging two files. It also means the complete list of things that can
//! happen to the data is the variants of one enum.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, Timelike};
use serde::{Deserialize, Serialize};

use crate::{
    archive::{ArchiveKey, ArchiveLog, Archived, Outcome},
    planner::{self, CreateKind},
    subscriptions::{self, Overlay, Subscription},
    tasks::{self, Active, Session, HORIZON_LABELS, IMPORTANCE_LABELS},
    utilities,
};

/// Severity seeded onto a task the moment it gains its first deadline.
/// Mid-scale: a dated task is scored by how bad missing the date is, the user
/// hasn't said yet, and the footer is right there to adjust it.
pub const NEW_TASK_IMPORTANCE: u8 = 2;

/// Horizon given to a task created on the timeline or in the quick-add:
/// "within a week", the middle of the road.
pub const NEW_TASK_HORIZON: u8 = 1;

/// Longest notes accepted. The notepad is a card on a wall, not a document;
/// this is far past anything that fits it.
pub const NOTES_MAX_CHARS: usize = 20_000;

/// Longest name an item may have, in characters. Nothing bounded a name before
/// the board did; a name is a line on a calendar cell, and two hundred
/// characters is already three of them. The phone page's inputs stop here too.
pub const NAME_MAX_CHARS: usize = 200;

/// First id a client hands out to things it creates before the server has
/// confirmed them (`sync::CLIENT_ID_FLOOR` is this). Nothing the board itself
/// numbers ever reaches it, and a restored row carrying one is renumbered.
pub const TEMPORARY_ID_FLOOR: u64 = 1 << 62;

/// The years a date on the wire may fall in. Everything the calendar does
/// with a date adds days to it, and `chrono` panics past its last
/// representable day; a client cannot be allowed to hand the board one of
/// those, and nothing anyone plans is a thousand years out.
const FIRST_YEAR: i32 = 1970;
const LAST_YEAR: i32 = 9999;

/// Hour a block lands at on an empty day when nothing says otherwise.
const DEFAULT_MORNING_HOUR: i32 = 7;

/// What reflow works from: the day's movable work blocks, addressed by
/// `(task id, session index)`, and the `(start, end)` spans it flows around.
type ReflowInputs = (Vec<planner::Flowable<(u64, usize)>>, Vec<(i32, i32)>);

/* ─────────────────────────────── Commands ─────────────────────────────── */

/// What a create makes. Mirrors `planner::CreateKind`, with the serialisation
/// the wire uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Task,
    Event,
    Routine,
}

impl Kind {
    pub fn to_create_kind(self) -> CreateKind {
        match self {
            Kind::Task => CreateKind::Task,
            Kind::Event => CreateKind::Event,
            Kind::Routine => CreateKind::Routine,
        }
    }

    pub fn from_create_kind(kind: CreateKind) -> Self {
        match kind {
            CreateKind::Task => Kind::Task,
            CreateKind::Event => Kind::Event,
            CreateKind::Routine => Kind::Routine,
        }
    }
}

/// Every way the board changes, and the two questions the phone asks of it.
///
/// Times on the wire: a day is `YYYY-MM-DD`, a time of day is minutes from
/// midnight, and an instant is either RFC 3339 (what this program writes) or
/// the naive local `YYYY-MM-DDTHH:MM` a phone's `datetime-local` input
/// produces — both are accepted, and resolved on this machine's clock like
/// every other time in the app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    /// *Query.* The state of `days` days starting at `from`, plus the tray.
    Snapshot {
        from: NaiveDate,
        #[serde(default = "one")]
        days: u32,
    },
    /// *Query.* The iCalendar feed.
    Feed,
    /// *Query.* The whole board — items, archive, notes, version — for a
    /// desktop that keeps a replica of a server's board.
    Board,
    /// A task or event as the desktop's New Task / New Event dialogs make it.
    Add {
        name: String,
        #[serde(default, deserialize_with = "wire::optional_local")]
        deadline: Option<DateTime<Local>>,
        #[serde(default)]
        importance: Option<u8>,
        #[serde(default)]
        is_event: bool,
        #[serde(default)]
        time_importance: Option<u8>,
    },
    /// A task from the tray's quick-add: a name, undated unless one is given.
    QuickAdd {
        name: String,
        #[serde(default, deserialize_with = "wire::optional_local")]
        deadline: Option<DateTime<Local>>,
    },
    Rename {
        id: u64,
        name: String,
    },
    /// Finish a task. `at` is when: the client that applied it first says,
    /// so the archive row it made and the one the server makes share a key
    /// (`archived_at`); absent, it is now.
    Complete {
        id: u64,
        #[serde(default, deserialize_with = "wire::optional_local")]
        at: Option<DateTime<Local>>,
    },
    /// Delete an item, which archives it. `at` as for `Complete`.
    Delete {
        id: u64,
        #[serde(default, deserialize_with = "wire::optional_local")]
        at: Option<DateTime<Local>>,
    },
    /// Drop an item without recording it — Escape on a block just dragged out
    /// and not yet named. Anything older is deleted, which archives.
    Forget {
        id: u64,
    },
    /// Move or resize one block: session `session` of a task, or an event's or
    /// routine's block when `session` is absent. A routine's block is its
    /// rule, so this moves it on every day it falls on.
    MoveBlock {
        id: u64,
        #[serde(default)]
        session: Option<usize>,
        day: NaiveDate,
        start: i32,
        minutes: u32,
    },
    /// Book a (further) session for a task on `day`. Without `minutes` it
    /// takes the task's remaining estimate, as a drop from the tray does.
    AddBlock {
        id: u64,
        day: NaiveDate,
        start: i32,
        #[serde(default)]
        minutes: Option<u32>,
    },
    RemoveBlock {
        id: u64,
        session: usize,
    },
    Unplan {
        id: u64,
    },
    /// A task's estimate, an event's length, or a routine's length.
    SetEstimate {
        id: u64,
        minutes: u32,
    },
    /// Set or clear a task's deadline. `null` clears.
    SetDeadline {
        id: u64,
        #[serde(default, deserialize_with = "wire::optional_local")]
        deadline: Option<DateTime<Local>>,
    },
    SetSeverity {
        id: u64,
        level: u8,
    },
    SetHorizon {
        id: u64,
        level: u8,
    },
    /// Which weekdays a routine repeats on (bit 0 = Monday); zero means just
    /// `day`.
    SetRepeat {
        id: u64,
        days: u8,
        day: NaiveDate,
    },
    /// A new item drawn on `day`'s timeline.
    Create {
        kind: Kind,
        name: String,
        day: NaiveDate,
        start: i32,
        minutes: u32,
    },
    /// Slide `day`'s remaining work past `from` — minutes from midnight — in
    /// order, around events and routines (`planner::reflow`). A client that
    /// applied it fills `from` with its own now, so the copy replayed later on
    /// the server slides the day exactly as the client did; absent, `from` is
    /// now on the board's own clock, and nothing happens on a day that is not
    /// today.
    Reflow {
        day: NaiveDate,
        #[serde(default)]
        from: Option<i32>,
    },
    /// Replace the notepad's text.
    SetNotes {
        text: String,
    },
    /// Put an archived item back on the board, addressed by the archive's key.
    Restore {
        id: u64,
        archived_at: DateTime<Local>,
    },
    /// Delete an archived row for good — the one irreversible act.
    ForgetArchived {
        id: u64,
        archived_at: DateTime<Local>,
    },

    /* The subscribed calendars (§23). These change the *list*, which is board
     * data like anything else; nothing here touches the events themselves,
     * which are fetched, never authored. */
    /// Subscribe to a calendar at an https address.
    AddSubscription {
        name: String,
        url: String,
        #[serde(default)]
        color: Option<[u8; 4]>,
    },
    /// Stop subscribing, and forget what it had said.
    RemoveSubscription {
        id: u64,
    },
    RenameSubscription {
        id: u64,
        name: String,
    },
    SetSubscriptionColor {
        id: u64,
        color: [u8; 4],
    },
    /// Switch one off without forgetting the address.
    SetSubscriptionEnabled {
        id: u64,
        enabled: bool,
    },
}

fn one() -> u32 {
    1
}

impl Command {
    /// Whether this only asks — a snapshot, the feed, the board — rather than
    /// changes.
    pub fn is_query(&self) -> bool {
        matches!(self, Command::Snapshot { .. } | Command::Feed | Command::Board)
    }

    /// Whether carrying this out touches the archive — the planner's ghosts
    /// are rebuilt from it, and only then.
    pub fn touches_archive(&self) -> bool {
        matches!(
            self,
            Command::Complete { .. }
                | Command::Delete { .. }
                | Command::Restore { .. }
                | Command::ForgetArchived { .. }
        )
    }

    /// Whether carrying this out changes the list of subscribed calendars —
    /// and so whether whoever is fetching has to be told.
    pub fn touches_calendars(&self) -> bool {
        matches!(
            self,
            Command::AddSubscription { .. }
                | Command::RemoveSubscription { .. }
                | Command::RenameSubscription { .. }
                | Command::SetSubscriptionColor { .. }
                | Command::SetSubscriptionEnabled { .. }
        )
    }

    /// The item this command addresses, if it addresses one — live, or by
    /// its archive key. What an offline queue remaps when a locally-created
    /// item gets its real id: an item finished and then put back while
    /// offline is addressed by that same id in the archive.
    pub fn item_id(&self) -> Option<u64> {
        match self {
            Command::Rename { id, .. }
            | Command::Complete { id, .. }
            | Command::Delete { id, .. }
            | Command::Forget { id }
            | Command::MoveBlock { id, .. }
            | Command::AddBlock { id, .. }
            | Command::RemoveBlock { id, .. }
            | Command::Unplan { id }
            | Command::SetEstimate { id, .. }
            | Command::SetDeadline { id, .. }
            | Command::SetSeverity { id, .. }
            | Command::SetHorizon { id, .. }
            | Command::SetRepeat { id, .. }
            | Command::Restore { id, .. }
            | Command::ForgetArchived { id, .. } => Some(*id),
            _ => None,
        }
    }

    /// The same, for writing: re-point the command at another item.
    pub fn set_item_id(&mut self, new_id: u64) {
        match self {
            Command::Rename { id, .. }
            | Command::Complete { id, .. }
            | Command::Delete { id, .. }
            | Command::Forget { id }
            | Command::MoveBlock { id, .. }
            | Command::AddBlock { id, .. }
            | Command::RemoveBlock { id, .. }
            | Command::Unplan { id }
            | Command::SetEstimate { id, .. }
            | Command::SetDeadline { id, .. }
            | Command::SetSeverity { id, .. }
            | Command::SetHorizon { id, .. }
            | Command::SetRepeat { id, .. }
            | Command::Restore { id, .. }
            | Command::ForgetArchived { id, .. } => *id = new_id,
            _ => {}
        }
    }

    /// The command with its clock filled in where it says nothing: when a
    /// completion or deletion happened (`at`) and where a reflow slides from
    /// (`from`). The host that applies a command first stamps it before
    /// keeping a copy for a server, so the server applies the same instant
    /// rather than its own — which is what makes the archive rows on both
    /// sides share a key, and a replayed reflow slide the day as it slid here.
    pub fn stamped(self, now: DateTime<Local>) -> Command {
        match self {
            Command::Complete { id, at: None } => Command::Complete { id, at: Some(now) },
            Command::Delete { id, at: None } => Command::Delete { id, at: Some(now) },
            Command::Reflow { day, from: None } => Command::Reflow { day, from: planner::now_marker(day, now) },
            other => other,
        }
    }

    /// Whether carrying this out makes a new live item — whose id the reply
    /// carries, and which an offline queue has to learn.
    pub fn creates_item(&self) -> bool {
        matches!(
            self,
            Command::Add { .. } | Command::QuickAdd { .. } | Command::Create { .. } | Command::Restore { .. }
        )
    }
}

/// Instants on the wire: RFC 3339 as this program writes them, or the naive
/// local `YYYY-MM-DDTHH:MM[:SS]` a phone's `datetime-local` input produces.
mod wire {
    use super::*;
    use serde::Deserializer;

    pub fn optional_local<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<DateTime<Local>>, D::Error> {
        let text: Option<String> = Option::deserialize(deserializer)?;
        match text.as_deref().map(str::trim).filter(|text| !text.is_empty()) {
            None => Ok(None),
            Some(text) => parse_local_input(text)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom(format!("not a time: `{text}`"))),
        }
    }
}

/// Parse an instant off the wire: RFC 3339, or naive local `YYYY-MM-DDTHH:MM`
/// (seconds optional), stepping over a DST gap like the rest of the app.
pub fn parse_local_input(text: &str) -> Option<DateTime<Local>> {
    let text = text.trim();
    if let Ok(exact) = DateTime::parse_from_rfc3339(text) {
        return Some(exact.with_timezone(&Local));
    }
    let naive = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S"))
        .ok()?;
    let minutes = naive.hour() as i32 * 60 + naive.minute() as i32;
    planner::resolve_on_day(naive.date(), minutes).filter(|at| sane_day(at.date_naive()).is_ok())
}

/// Naive local `YYYY-MM-DDTHH:MM`, the shape a `datetime-local` input takes.
pub fn local_input(at: DateTime<Local>) -> String {
    at.format("%Y-%m-%dT%H:%M").to_string()
}

/// A name off the wire: trimmed, and no longer than `NAME_MAX_CHARS`. Whether
/// an empty one is allowed is the command's own question (a timeline create
/// names the thing itself; the others refuse).
fn checked_name(name: &str) -> Result<String, BoardError> {
    let name = name.trim();
    if name.chars().count() > NAME_MAX_CHARS {
        return Err(BoardError::bad_request(format!("A name is at most {NAME_MAX_CHARS} characters.")));
    }
    Ok(name.to_string())
}

/// Whether a read failed for want of permission rather than for what the
/// file holds — the one failure that must not be answered by quarantining it.
fn is_permission_denied(error: &(dyn std::error::Error + 'static)) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|io| io.kind() == std::io::ErrorKind::PermissionDenied)
}

/// A day off the wire, if it is one the calendar can work with.
fn sane_day(day: NaiveDate) -> Result<NaiveDate, BoardError> {
    if (FIRST_YEAR..=LAST_YEAR).contains(&day.year()) {
        Ok(day)
    } else {
        Err(BoardError::bad_request(format!("That date is out of range ({FIRST_YEAR}–{LAST_YEAR}).")))
    }
}

/// The same for an instant, which may be absent.
fn sane_instant(at: Option<DateTime<Local>>) -> Result<Option<DateTime<Local>>, BoardError> {
    match at {
        Some(at) => sane_day(at.date_naive()).map(|_| Some(at)),
        None => Ok(None),
    }
}

/// When a retirement happened: the client's `at` when it gave one — its
/// clock may run a little ahead of this one, which is fine — but never more
/// than a day into the future, which is a clock that is wrong and would pin
/// the row to the top of the ledger for years.
fn archived_at(at: Option<DateTime<Local>>, now: DateTime<Local>) -> Result<DateTime<Local>, BoardError> {
    let latest = now + chrono::Duration::days(1);
    Ok(sane_instant(at)?.map_or(now, |at| at.min(latest)))
}

/// The whole board on the wire — what `Command::Board` answers with, and what
/// a desktop replica is built from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardState {
    pub version: u64,
    pub items: Vec<Active>,
    pub archive: Vec<Archived>,
    pub notes: String,
    /// The subscribed calendars, which are board data (§23).
    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
    /// What those calendars said last time the board's owner asked. Derived,
    /// and sent with the board because a client never fetches for itself.
    #[serde(default)]
    pub overlay: Overlay,
}

/* ─────────────────────────────── Replies ─────────────────────────────── */

/// Why a command could not be carried out, with the HTTP status it maps to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardError {
    pub status: u16,
    pub message: String,
}

impl BoardError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self { status: 400, message: message.into() }
    }
    /// The item is gone — finished or deleted elsewhere, or the caller is
    /// looking at a stale picture. Callers refresh on this.
    pub fn gone(message: impl Into<String>) -> Self {
        Self { status: 410, message: message.into() }
    }
    pub fn failed(message: impl Into<String>) -> Self {
        Self { status: 500, message: message.into() }
    }
    /// Whether this is the caller's mistake (a bad request, a stale id)
    /// rather than the board's failure — what decides whether an offline
    /// queue drops the command or keeps trying.
    pub fn is_rejection(&self) -> bool {
        (400..500).contains(&self.status)
    }
}

impl std::fmt::Display for BoardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// What a carried-out command has to say for itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    /// The id of an item this created or brought back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// The index of a session this booked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<usize>,
    /// How many blocks a reflow moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved: Option<usize>,
}

/* ─────────────────────────────── The board ─────────────────────────────── */

/// The live set, the archive, the notepad, and the counter that names things.
pub struct Board {
    /// Every live task, event and routine. Public because the GUI reads it
    /// every frame and re-orders it when it rebuilds the calendar; changing
    /// what is *in* it goes through `apply`.
    pub items: Vec<Active>,
    /// Everything that has left the board. Read from disk the first time
    /// something asks and kept from then on (§17.2).
    pub archive: ArchiveLog,
    /// The notepad. Public because the desktop's text field edits it live;
    /// the debounced save goes through `apply(SetNotes)`.
    pub notes: String,
    /// The subscribed calendars (§23). Authored, and changed only through
    /// `apply`, so it reaches a client like every other board change.
    subscriptions: Vec<Subscription>,
    /// What those calendars said. Derived: never authored, never in the
    /// archive, and losing it costs a refresh rather than a calendar.
    overlay: Overlay,
    /// Next stable id to hand out. Seeded past the highest id present at load
    /// (`tasks::assign_missing_ids`).
    next_id: u64,
    data_dir: PathBuf,
    /// Moves on every change. What the phone watches, and what tells two
    /// pictures of the board apart.
    version: u64,
}

impl Board {
    /// Read the board from `data_dir`, quarantining an unreadable active set
    /// rather than refusing to start. The strings are what went wrong, for the
    /// error window.
    pub fn open(data_dir: PathBuf) -> (Self, Vec<String>) {
        let mut problems = Vec::new();

        let items = match tasks::read_at_startup(&data_dir) {
            Ok(items) => items,
            Err(error) if is_permission_denied(error.as_ref()) => {
                // Not corrupt — not ours to read. Quarantining would rename
                // the calendar away and start an empty board over it; the
                // file is left exactly where it is and the reason said.
                problems.push(
                    "read_at_startup.json exists but cannot be read (permission denied). Nothing was moved or changed: fix the file's owner or mode and start again."
                        .to_string(),
                );
                Vec::new()
            }
            Err(error) => {
                problems.push(tasks::quarantine_corrupt_file(&data_dir, "read_at_startup.json", error.as_ref()));
                Vec::new()
            }
        };

        // Tabs are turned into spaces on the way in: the notepad's face has no
        // tab glyph (§9.1).
        let notes = utilities::read_notepad_text(&data_dir)
            .map(|text| utilities::detab(&text))
            .unwrap_or_else(|_| "There was something wrong with taskdeck_data/notepad_text.json!".to_string());

        // The subscribed calendars. An unreadable list is reported and left
        // exactly where it is rather than quarantined: it holds addresses
        // that are themselves credentials, and renaming one aside is a good
        // way for someone to lose a link they cannot get back.
        let subscriptions = match subscriptions::read(&data_dir) {
            Ok(list) => list,
            Err(error) => {
                problems.push(format!(
                    "{} could not be read ({error}). Nothing was moved; the subscribed calendars are off until it is fixed.",
                    subscriptions::SUBSCRIPTIONS_FILE
                ));
                Vec::new()
            }
        };
        // And what they last said, so the wall is not blank for the ten
        // minutes before the first fetch comes back.
        let overlay = subscriptions::read_overlay(&data_dir).unwrap_or_default();

        let mut board = Self::from_parts(items, notes, data_dir);
        board.subscriptions = subscriptions;
        board.overlay = overlay;
        (board, problems)
    }

    /// The board's files that exist in `data_dir` but cannot be opened for
    /// reading — a folder copied in as another user, usually. A server refuses
    /// to start over these rather than serve an empty board in their place.
    pub fn unreadable_files(data_dir: &Path) -> Vec<String> {
        [
            "read_at_startup.json",
            "archived.jsonl",
            "notepad_text.json",
            "colorschemes.json",
            "userconfig.toml",
            subscriptions::SUBSCRIPTIONS_FILE,
        ]
            .into_iter()
            .filter(|name| {
                let path = data_dir.join(name);
                path.exists() && matches!(std::fs::File::open(&path), Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied)
            })
            .map(str::to_string)
            .collect()
    }

    /// A board over items already in hand — a replica handed down from a
    /// server, or a test. `data_dir` is where *this* board saves.
    pub fn from_parts(mut items: Vec<Active>, notes: String, data_dir: PathBuf) -> Self {
        let next_id = tasks::assign_missing_ids(&mut items);
        tasks::migrate_legacy_plans(&mut items);
        Self {
            items,
            archive: ArchiveLog::new(),
            notes,
            subscriptions: Vec::new(),
            overlay: Overlay::default(),
            next_id,
            data_dir,
            version: 0,
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The change counter. Moves on every successful `apply` and on
    /// `replace`.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Start handing out ids from `first` — a client replica keeps its own
    /// creations out of the server's number space until they are confirmed.
    pub fn number_from(&mut self, first: u64) {
        self.next_id = self.next_id.max(first);
    }

    /// Move the version without changing the data — for something a watcher
    /// of the board should redraw for that is not the board itself, such as
    /// the colour scheme it is painted in.
    pub fn touch(&mut self) {
        self.version = self.version.wrapping_add(1);
    }

    /// Start the version counter no lower than `at_least`. A server seeds it
    /// from the clock, so a restart never hands out a number a client has
    /// already seen and taken for the latest.
    pub fn seed_version(&mut self, at_least: u64) {
        self.version = self.version.max(at_least);
    }

    /// Make every item this board's own. A folder that was a client's replica
    /// can hold items created offline under temporary ids (`TEMPORARY_ID_FLOOR`
    /// and up) whose creation never reached the server; when that folder
    /// becomes a board in its own right — the desktop back in local mode, or
    /// the folder copied to a server — those ids must not stay, or every id
    /// the board hands out from then on is in the range clients reserve.
    /// Renumbers them from just past the highest real id, saves if anything
    /// changed, and answers how many were renumbered.
    pub fn adopt_temporaries(&mut self) -> Result<usize, BoardError> {
        let highest_real = self.items.iter().map(|item| item.id).filter(|id| *id < TEMPORARY_ID_FLOOR).max().unwrap_or(0);
        self.next_id = highest_real + 1;
        let mut renumbered = 0;
        for index in 0..self.items.len() {
            if self.items[index].id >= TEMPORARY_ID_FLOOR {
                self.items[index].id = self.next_item_id();
                renumbered += 1;
            }
        }
        if renumbered > 0 {
            self.save_items()?;
        }
        Ok(renumbered)
    }

    /// Write everything out — items, notes, and the archive as held — so a
    /// replica taken from a server is on this machine's disk too, for the
    /// next start without the server.
    pub fn save_all(&mut self) -> Result<(), String> {
        self.save_items_and_notes()?;
        if self.archive.is_loaded() {
            self.archive.persist(&self.data_dir).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// `save_all` without the archive — the two small files, for a replica
    /// refresh that did not touch the log.
    pub fn save_items_and_notes(&mut self) -> Result<(), String> {
        tasks::oversafe_activesave(&self.items, &self.data_dir).map_err(|e| e.to_string())?;
        subscriptions::save(&self.subscriptions, &self.data_dir).map_err(|e| e.to_string())?;
        utilities::save_notepad_text(self.notes.clone(), &self.data_dir).map_err(|e| e.to_string())
    }

    /// Cache what the calendars said. Derived data, so a failure is worth
    /// nothing more than the next refresh writing it again.
    pub fn save_overlay(&self) {
        let _ = subscriptions::save_overlay(&self.overlay, &self.data_dir);
    }

    /// A thing created here under a temporary id turned out to be `server`
    /// on the server: rename it, so the replica and the server agree until
    /// the next board arrives. True when there was such an item.
    pub fn renumber(&mut self, local: u64, server: u64) -> bool {
        match self.item_mut(local) {
            Some(item) => {
                item.id = server;
                true
            }
            None => false,
        }
    }

    /// Swap in another picture of the board — what a client does when the
    /// server's truth arrives. Keeps the id counter ahead of everything.
    /// The subscriptions and the overlay come in with the rest rather than
    /// through setters of their own: a client never fetches, so this is the
    /// only way either reaches it, and a caller that could forget one would
    /// draw yesterday's meetings beside today's blocks.
    pub fn replace(
        &mut self,
        items: Vec<Active>,
        archived: Option<Vec<Archived>>,
        notes: String,
        subscriptions: Vec<Subscription>,
        overlay: Overlay,
    ) {
        self.items = items;
        self.next_id = self.next_id.max(self.items.iter().map(|item| item.id + 1).max().unwrap_or(1));
        if let Some(rows) = archived {
            self.archive.replace_with(rows);
        }
        self.notes = notes;
        self.subscriptions = subscriptions;
        self.overlay = overlay;
        self.version = self.version.wrapping_add(1);
    }

    /* ─────────────────────── the subscribed calendars ────────────────────── */

    /// The calendars and their events, as a server handed them down. Used on
    /// a client's very first board, before any `SyncEvent::Board` has arrived.
    /// The version does not move: this is a board being built, not changed.
    pub fn adopt_from_server(&mut self, subscriptions: Vec<Subscription>, overlay: Overlay) {
        self.subscriptions = subscriptions;
        self.overlay = overlay;
    }

    pub fn subscriptions(&self) -> &[Subscription] {
        &self.subscriptions
    }

    pub fn overlay(&self) -> &Overlay {
        &self.overlay
    }

    /// Put a freshly fetched overlay in place. **True only when it actually
    /// differs**, and the version moves only then — a calendar server answering
    /// the same thing every ten minutes must not wake every parked phone and
    /// make every client refetch the whole board on a timer.
    pub fn adopt_overlay(&mut self, fresh: Overlay) -> bool {
        if self.overlay.digest() == fresh.digest() {
            // The events are the same. Keep the fresh status all the same, so
            // the desk can say when it last looked and what it heard, without
            // that being a change anybody else has to hear about.
            self.overlay.status = fresh.status;
            return false;
        }
        self.overlay = fresh;
        self.version = self.version.wrapping_add(1);
        true
    }

    pub fn item(&self, id: u64) -> Option<&Active> {
        self.items.iter().find(|item| item.id == id)
    }

    fn item_mut(&mut self, id: u64) -> Option<&mut Active> {
        self.items.iter_mut().find(|item| item.id == id)
    }

    fn next_item_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Make sure the archive is in memory.
    pub fn load_archive(&mut self) -> Result<(), String> {
        self.archive.load(&self.data_dir).map_err(|error| error.to_string())
    }

    /// The whole board, for the wire. Loads the archive if it has not been.
    pub fn state(&mut self) -> Result<BoardState, String> {
        // A log that cannot be read is an error, not an empty archive: a
        // client would take the emptiness for the truth and write it over its
        // own copy.
        self.load_archive()?;
        Ok(BoardState {
            version: self.version,
            items: self.items.clone(),
            archive: self.archive.entries().to_vec(),
            notes: self.notes.clone(),
            subscriptions: self.subscriptions.clone(),
            overlay: self.overlay.clone(),
        })
    }

    /// Seed for the task list's tie-break jitter: the **day** (§14.4).
    pub fn shuffle_seed(now: DateTime<Local>) -> u64 {
        now.date_naive().num_days_from_ce() as u64
    }

    /// Every task in the order the desktop's task list shows them — by score,
    /// most pressing first. Events and routines are not ranked (§18.1).
    pub fn ranked_ids(&self, now: DateTime<Local>) -> Vec<u64> {
        let seed = Self::shuffle_seed(now);
        let mut scored: Vec<(f32, u64)> = self
            .items
            .iter()
            .filter(|item| !item.is_event && !item.is_routine())
            .map(|item| (item.importance_score(now, seed), item.id))
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().map(|(_, id)| id).collect()
    }

    /// Where a new block goes on `day` when nobody said: after the last thing
    /// already on it, or the default morning hour on an empty day, clamped so
    /// a default-length block still fits.
    pub fn next_free_start(&self, day: NaiveDate) -> i32 {
        let day_end = planner::DAY_MINUTES - planner::DEFAULT_BLOCK_MINUTES as i32;
        self.items
            .iter()
            .flat_map(|item| planner::placements_for(item, day))
            .map(|placed| placed.placement.end())
            .max()
            .unwrap_or(DEFAULT_MORNING_HOUR * 60)
            .clamp(0, day_end)
    }

    /// The day's work blocks that reflow may move, and the spans it flows
    /// around: task sessions move; events and routines are the anchors (§20.2).
    fn reflow_inputs(&self, day: NaiveDate) -> ReflowInputs {
        let mut blocks = Vec::new();
        let mut anchors = Vec::new();
        for item in &self.items {
            for placed in planner::placements_for(item, day) {
                let planner::Placement::Block { start, minutes } = placed.placement else { continue };
                match placed.session {
                    Some(index) if !item.is_event && !item.is_routine() => {
                        blocks.push(planner::Flowable { key: (item.id, index), start, minutes });
                    }
                    _ => anchors.push((start, start + minutes as i32)),
                }
            }
        }
        // A subscribed calendar's hours are somebody else's claim on the day,
        // so reflow steps over them exactly as it steps over an event of our
        // own. An all-day band is not one of these: it would claim the whole
        // day and leave nowhere to put anything (§23).
        anchors.extend(self.overlay.anchors_on(day));
        (blocks, anchors)
    }

    /// Minutes of `day`'s booked work already behind the now-line (§16.8).
    /// Zero on any day but today, which has no now.
    pub fn behind_minutes(&self, day: NaiveDate, now: DateTime<Local>) -> i32 {
        let Some(now_minutes) = planner::now_marker(day, now) else { return 0 };
        let (blocks, _) = self.reflow_inputs(day);
        let placements: Vec<_> = blocks
            .iter()
            .map(|block| planner::Placement::Block { start: block.start, minutes: block.minutes })
            .collect();
        planner::behind_minutes(&placements, now_minutes)
    }

    /* ─────────────────────────── persistence ─────────────────────────── */

    fn save_items(&mut self) -> Result<(), BoardError> {
        self.version = self.version.wrapping_add(1);
        tasks::oversafe_activesave(&self.items, &self.data_dir)
            .map_err(|error| BoardError::failed(format!("Saving error:\n{error}")))
    }

    fn save_subscriptions(&mut self) -> Result<(), BoardError> {
        self.version = self.version.wrapping_add(1);
        subscriptions::save(&self.subscriptions, &self.data_dir)
            .map_err(|error| BoardError::failed(format!("Could not save the calendar list:\n{error}")))
    }

    fn save_notes(&mut self) -> Result<(), BoardError> {
        self.version = self.version.wrapping_add(1);
        utilities::save_notepad_text(self.notes.clone(), &self.data_dir)
            .map_err(|error| BoardError::failed(format!("Could not save notepad text:\n{error}")))
    }

    /* ─────────────────────────────── apply ─────────────────────────────── */

    /// Carry out one command, and save.
    ///
    /// This `match` is the complete list of what can happen to the data. Each
    /// arm validates, changes, and saves; a change that saved badly is still
    /// made — refusing to complete a task because the disk is full is the
    /// wrong trade — and the error says so. `now` is passed in rather than
    /// read, so the reflow and the tests have one clock.
    pub fn apply(&mut self, command: Command, now: DateTime<Local>) -> Result<Reply, BoardError> {
        let gone = || BoardError::gone("That item is no longer on the board.");
        let bad_time = || BoardError::bad_request("That time doesn't exist on this day (daylight saving).");

        match command {
            Command::Snapshot { .. } | Command::Feed | Command::Board => {
                Err(BoardError::bad_request("That is a query, not a command."))
            }

            Command::Add { name, deadline, importance, is_event, time_importance } => {
                let name = checked_name(&name)?;
                if name.is_empty() {
                    return Err(BoardError::bad_request("It needs a name."));
                }
                if is_event && deadline.is_none() {
                    return Err(BoardError::bad_request("An event needs a time."));
                }
                let deadline = sane_instant(deadline)?;
                // Undated and unranked is a shape the scorer calls malformed
                // and pins to the top of every list; what such a task means is
                // the quick-add's default horizon, so that is what it gets.
                let time_importance = if !is_event && deadline.is_none() && importance.is_none() && time_importance.is_none() {
                    Some(NEW_TASK_HORIZON)
                } else {
                    time_importance
                };
                // The same scale checks the footer's setters make: a level off
                // the scale would be saved, and then index a table it is not in.
                if importance.is_some_and(|level| level as usize >= IMPORTANCE_LABELS.len()) {
                    return Err(BoardError::bad_request("No such severity."));
                }
                if time_importance.is_some_and(|level| level as usize >= HORIZON_LABELS.len()) {
                    return Err(BoardError::bad_request("No such horizon."));
                }
                let id = self.push_new(name, deadline, importance, is_event, time_importance, now);
                self.save_items()?;
                Ok(Reply { id: Some(id), ..Default::default() })
            }

            Command::QuickAdd { name, deadline } => {
                let name = checked_name(&name)?;
                if name.is_empty() {
                    return Err(BoardError::bad_request("A task needs a name."));
                }
                let deadline = sane_instant(deadline)?;
                // The same shapes the desktop makes: a dated task carries a
                // middling severity, an undated one the middle horizon.
                let id = if deadline.is_some() {
                    self.push_new(name, deadline, Some(NEW_TASK_IMPORTANCE), false, None, now)
                } else {
                    self.push_new(name, None, None, false, Some(NEW_TASK_HORIZON), now)
                };
                self.save_items()?;
                Ok(Reply { id: Some(id), ..Default::default() })
            }

            Command::Rename { id, name } => {
                let name = checked_name(&name)?;
                if name.is_empty() {
                    return Err(BoardError::bad_request("A name cannot be empty."));
                }
                let item = self.item_mut(id).ok_or_else(gone)?;
                item.name = name;
                self.save_items()?;
                Ok(Reply::default())
            }

            Command::Complete { id, at } => {
                let item = self.item(id).ok_or_else(gone)?;
                if item.is_event || item.is_routine() {
                    return Err(BoardError::bad_request(
                        "Only a task can be finished — an event or a routine is deleted instead.",
                    ));
                }
                let at = archived_at(at, now)?;
                self.retire(id, Outcome::Finished, at)?;
                Ok(Reply::default())
            }

            Command::Delete { id, at } => {
                self.item(id).ok_or_else(gone)?;
                let at = archived_at(at, now)?;
                self.retire(id, Outcome::Dropped, at)?;
                Ok(Reply::default())
            }

            Command::Forget { id } => {
                self.item(id).ok_or_else(gone)?;
                self.items.retain(|item| item.id != id);
                self.save_items()?;
                Ok(Reply::default())
            }

            Command::MoveBlock { id, session, day, start, minutes } => {
                let day = sane_day(day)?;
                let item = self.item(id).ok_or_else(gone)?;
                let addressed = if item.is_event || item.is_routine() {
                    None
                } else {
                    match session {
                        Some(index) if index < item.sessions.len() => Some(index),
                        _ => return Err(BoardError::gone("That block is no longer there.")),
                    }
                };
                let (start, minutes) = planner::clamp_block(start as f32, minutes as f32);
                let when = planner::resolve_on_day(day, start).ok_or_else(bad_time)?;

                let item = self.item_mut(id).ok_or_else(gone)?;
                if let Some(rule) = item.recurrence.as_mut() {
                    // Moving or resizing a routine edits **the rule**, so it
                    // moves on every day it falls on: you do not reschedule
                    // Wednesday's sleep, you change what time you go to bed.
                    rule.start_minutes = start.clamp(0, planner::DAY_MINUTES - 1);
                    rule.minutes = minutes;
                } else if item.is_event {
                    item.deadline = Some(when);
                    item.duration_minutes = Some(minutes);
                } else if let Some(slot) = addressed.and_then(|index| item.sessions.get_mut(index)) {
                    *slot = Session { start: when, minutes };
                } else {
                    return Err(BoardError::gone("That block is no longer there."));
                }
                self.save_items()?;
                Ok(Reply::default())
            }

            Command::AddBlock { id, day, start, minutes } => {
                let day = sane_day(day)?;
                let item = self.item(id).ok_or_else(gone)?;
                if item.is_event || item.is_routine() {
                    return Err(BoardError::bad_request("Only a task can be given a block of time."));
                }
                let length = minutes.unwrap_or_else(|| planner::drop_length_for(item));
                let (start, length) = planner::clamp_block(start as f32, length as f32);
                let when = planner::resolve_on_day(day, start).ok_or_else(bad_time)?;

                let item = self.item_mut(id).ok_or_else(gone)?;
                item.sessions.push(Session { start: when, minutes: length.max(planner::MIN_BLOCK_MINUTES) });
                let index = item.sessions.len() - 1;
                self.save_items()?;
                Ok(Reply { session: Some(index), ..Default::default() })
            }

            Command::RemoveBlock { id, session } => {
                let item = self.item_mut(id).ok_or_else(gone)?;
                if item.is_event || item.is_routine() || session >= item.sessions.len() {
                    return Err(BoardError::gone("That block is no longer there."));
                }
                item.sessions.remove(session);
                self.save_items()?;
                Ok(Reply::default())
            }

            Command::Unplan { id } => {
                let item = self.item_mut(id).ok_or_else(gone)?;
                if item.is_event || item.is_routine() {
                    return Err(BoardError::bad_request("Only a task can be unplanned."));
                }
                // The estimate survives on purpose: giving up on the slots is
                // not forgetting how long the work takes.
                item.sessions.clear();
                self.save_items()?;
                Ok(Reply::default())
            }

            Command::SetEstimate { id, minutes } => {
                if !(planner::MIN_BLOCK_MINUTES..=planner::DAY_MINUTES as u32).contains(&minutes) {
                    return Err(BoardError::bad_request("A length is between fifteen minutes and a day."));
                }
                let item = self.item_mut(id).ok_or_else(gone)?;
                if let Some(rule) = item.recurrence.as_mut() {
                    // A routine's length is part of its rule, not an estimate of
                    // work — so it is set there and `duration_minutes` stays empty.
                    let (start, length) = planner::clamp_block(rule.start_minutes as f32, minutes as f32);
                    rule.start_minutes = start;
                    rule.minutes = length;
                } else {
                    item.duration_minutes = Some(minutes.max(planner::MIN_BLOCK_MINUTES));
                    if item.is_event {
                        // Legal-block clamping is the timeline's job everywhere
                        // else, so it is done here too: a length that would run
                        // past midnight pulls the start back, exactly as dragging
                        // the block's edge would.
                        if let Some(anchor) = item.deadline {
                            let start = anchor.hour() as i32 * 60 + anchor.minute() as i32;
                            let (start, length) = planner::clamp_block(start as f32, minutes as f32);
                            item.duration_minutes = Some(length);
                            if let Some(when) = planner::resolve_on_day(anchor.date_naive(), start) {
                                item.deadline = Some(when);
                            }
                        }
                    }
                }
                self.save_items()?;
                Ok(Reply::default())
            }

            Command::SetDeadline { id, deadline } => {
                let deadline = sane_instant(deadline)?;
                let item = self.item_mut(id).ok_or_else(gone)?;
                if item.is_event || item.is_routine() {
                    return Err(BoardError::bad_request("Only a task has a due date; an event's time is its time."));
                }
                item.deadline = deadline;
                // A dated task is scored by severity, and the user hasn't said
                // yet: seed the middle and leave the footer to adjust it.
                if deadline.is_some() && item.importance.is_none() {
                    item.importance = Some(NEW_TASK_IMPORTANCE);
                }
                self.save_items()?;
                Ok(Reply::default())
            }

            Command::SetSeverity { id, level } => {
                if level as usize >= IMPORTANCE_LABELS.len() {
                    return Err(BoardError::bad_request("No such severity."));
                }
                let item = self.item_mut(id).ok_or_else(gone)?;
                if item.is_event || item.is_routine() {
                    return Err(BoardError::bad_request("Only a task has a severity."));
                }
                item.importance = Some(level);
                self.save_items()?;
                Ok(Reply::default())
            }

            Command::SetHorizon { id, level } => {
                if level as usize >= HORIZON_LABELS.len() {
                    return Err(BoardError::bad_request("No such horizon."));
                }
                let item = self.item_mut(id).ok_or_else(gone)?;
                if item.is_event || item.is_routine() {
                    return Err(BoardError::bad_request("Only a task has a horizon."));
                }
                item.time_importance = Some(level);
                self.save_items()?;
                Ok(Reply::default())
            }

            Command::SetRepeat { id, days, day } => {
                let day = sane_day(day)?;
                let item = self.item_mut(id).ok_or_else(gone)?;
                let Some(rule) = item.recurrence.as_mut() else {
                    return Err(BoardError::bad_request("Only a routine repeats."));
                };
                rule.days = days & tasks::EVERY_DAY;
                if !rule.repeats() {
                    // A rule that stops repeating lands on the day being looked
                    // at, not on wherever it was first drawn.
                    rule.anchor = day;
                }
                self.save_items()?;
                Ok(Reply::default())
            }

            Command::Create { kind, name, day, start, minutes } => {
                let day = sane_day(day)?;
                let (start, minutes) = planner::clamp_block(start as f32, minutes as f32);
                let when = planner::resolve_on_day(day, start).ok_or_else(bad_time)?;
                let name = checked_name(&name)?;
                let name = if name.is_empty() {
                    match kind {
                        Kind::Task => "New task",
                        Kind::Event => "New event",
                        Kind::Routine => "New routine",
                    }
                    .to_string()
                } else {
                    name
                };
                let id = self.push_created(kind.to_create_kind(), name, day, when, start, minutes, now);
                self.save_items()?;
                Ok(Reply { id: Some(id), ..Default::default() })
            }

            Command::Reflow { day, from } => {
                let day = sane_day(day)?;
                // A client's own `from` is honoured as given, clamped to the
                // day, so the server's replay slides exactly as the client did.
                let from = from.map(|minutes| minutes.clamp(0, planner::DAY_MINUTES));
                let Some(now_minutes) = from.or_else(|| planner::now_marker(day, now)) else {
                    return Ok(Reply { moved: Some(0), ..Default::default() });
                };
                let (blocks, anchors) = self.reflow_inputs(day);
                let mut moved = 0;
                for ((id, index), start) in planner::reflow(&blocks, &anchors, now_minutes) {
                    let Some(when) = planner::resolve_on_day(day, start) else { continue };
                    let slot = self.item_mut(id).and_then(|item| item.sessions.get_mut(index));
                    if let Some(session) = slot
                        && session.start != when
                    {
                        session.start = when;
                        moved += 1;
                    }
                }
                if moved > 0 {
                    self.save_items()?;
                }
                Ok(Reply { moved: Some(moved), ..Default::default() })
            }

            Command::SetNotes { text } => {
                if text.chars().count() > NOTES_MAX_CHARS {
                    return Err(BoardError::bad_request("Those notes are too long for the notepad."));
                }
                self.notes = utilities::detab(&text);
                self.save_notes()?;
                Ok(Reply::default())
            }

            Command::Restore { id, archived_at } => {
                let key = ArchiveKey { id, archived_at };
                self.load_archive().map_err(BoardError::failed)?;
                let taken = self
                    .archive
                    .take(&self.data_dir, key)
                    .map_err(|error| BoardError::failed(format!("Could not update the archive:\n{error}")))?;
                let record = taken.ok_or_else(|| BoardError::gone("That is no longer in the archive."))?;

                let mut item = record.to_active();
                // The id it had may not be free: `next_id` is seeded past the
                // highest *live* id, a legacy row carries the `0` sentinel, and
                // a row that came from a client's cache may wear a temporary id.
                let unusable = item.id == 0
                    || item.id >= TEMPORARY_ID_FLOOR
                    || self.items.iter().any(|live| live.id == item.id);
                if unusable {
                    item.id = self.next_item_id();
                } else {
                    self.next_id = self.next_id.max(item.id + 1);
                }
                let restored = item.id;
                self.items.push(item);
                if let Err(error) = self.save_items() {
                    // The row has left the log; if the live set cannot be
                    // written it goes back, so the item is in one file or the
                    // other and never in neither. Should even that fail, the
                    // item stays live in memory — the next save that succeeds
                    // writes it — and the message says so.
                    match self.archive.record(&self.data_dir, record) {
                        Ok(()) => {
                            self.items.pop();
                            return Err(error);
                        }
                        Err(refile) => {
                            return Err(BoardError::failed(format!(
                                "{}\nThe archive row could not be put back either ({refile}), so the item is kept on the board in memory only until a save succeeds — free some space before quitting.",
                                error.message
                            )));
                        }
                    }
                }
                Ok(Reply { id: Some(restored), ..Default::default() })
            }

            Command::ForgetArchived { id, archived_at } => {
                let key = ArchiveKey { id, archived_at };
                self.load_archive().map_err(BoardError::failed)?;
                self.archive
                    .take(&self.data_dir, key)
                    .map_err(|error| BoardError::failed(format!("Could not update the archive:\n{error}")))?
                    .ok_or_else(|| BoardError::gone("That is no longer in the archive."))?;
                self.version = self.version.wrapping_add(1);
                Ok(Reply::default())
            }

            /* The subscribed calendars. Each of these edits the *list*; the
             * events themselves are fetched by whoever owns this board and
             * are never authored here. */
            Command::AddSubscription { name, url, color } => {
                if self.subscriptions.len() >= subscriptions::SUBSCRIPTIONS_MAX {
                    return Err(BoardError::bad_request(format!(
                        "That is already {} calendars, which is as many as this keeps.",
                        subscriptions::SUBSCRIPTIONS_MAX
                    )));
                }
                let url = subscriptions::checked_url(&url).map_err(BoardError::bad_request)?;
                if self.subscriptions.iter().any(|existing| existing.url == url) {
                    return Err(BoardError::bad_request("That calendar is already subscribed to."));
                }
                let name = checked_name(&name)?;
                let id = self.next_item_id();
                let name = if name.is_empty() { format!("Calendar {id}") } else { name };
                let color = color.unwrap_or(subscriptions::DEFAULT_COLORS
                    [self.subscriptions.len() % subscriptions::DEFAULT_COLORS.len()]);
                self.subscriptions.push(Subscription { id, name, url, color, enabled: true });
                self.save_subscriptions()?;
                Ok(Reply { id: Some(id), ..Reply::default() })
            }

            Command::RemoveSubscription { id } => {
                let before = self.subscriptions.len();
                self.subscriptions.retain(|subscription| subscription.id != id);
                if self.subscriptions.len() == before {
                    return Err(BoardError::gone("That calendar is no longer subscribed to."));
                }
                // Its events go now rather than at the next fetch, so the day
                // is right on the very next frame — and so a client, which
                // never fetches at all, is right ever.
                self.overlay.forget(id);
                self.save_subscriptions()?;
                Ok(Reply::default())
            }

            Command::RenameSubscription { id, name } => {
                let name = checked_name(&name)?;
                let subscription = self
                    .subscriptions
                    .iter_mut()
                    .find(|subscription| subscription.id == id)
                    .ok_or_else(|| BoardError::gone("That calendar is no longer subscribed to."))?;
                if !name.is_empty() {
                    subscription.name = name;
                }
                self.save_subscriptions()?;
                Ok(Reply::default())
            }

            Command::SetSubscriptionColor { id, color } => {
                let subscription = self
                    .subscriptions
                    .iter_mut()
                    .find(|subscription| subscription.id == id)
                    .ok_or_else(|| BoardError::gone("That calendar is no longer subscribed to."))?;
                subscription.color = color;
                self.save_subscriptions()?;
                Ok(Reply::default())
            }

            Command::SetSubscriptionEnabled { id, enabled } => {
                let subscription = self
                    .subscriptions
                    .iter_mut()
                    .find(|subscription| subscription.id == id)
                    .ok_or_else(|| BoardError::gone("That calendar is no longer subscribed to."))?;
                subscription.enabled = enabled;
                if !enabled {
                    self.overlay.forget(id);
                }
                self.save_subscriptions()?;
                Ok(Reply::default())
            }
        }
    }

    /// A fully-formed new item, as the New Task / New Event dialogs make one:
    /// nothing is placed on the planner yet.
    fn push_new(
        &mut self,
        name: String,
        deadline: Option<DateTime<Local>>,
        importance: Option<u8>,
        is_event: bool,
        time_importance: Option<u8>,
        now: DateTime<Local>,
    ) -> u64 {
        let id = self.next_item_id();
        self.items.push(Active {
            id,
            name,
            deadline,
            importance,
            time_importance,
            is_event,
            created: now,
            sessions: Vec::new(),
            planned_start: None,
            duration_minutes: None,
            recurrence: None,
        });
        id
    }

    /// The item a timeline create makes. `kind` decides which time field the
    /// gesture fills, which is the whole due-versus-planned distinction: a
    /// task gets its first session and no due date, an event *is* its time,
    /// a routine gets a rule that happens once, on this day, until told to
    /// repeat.
    #[allow(clippy::too_many_arguments)]
    fn push_created(
        &mut self,
        kind: CreateKind,
        name: String,
        day: NaiveDate,
        when: DateTime<Local>,
        start_minutes: i32,
        minutes: u32,
        now: DateTime<Local>,
    ) -> u64 {
        let is_event = kind == CreateKind::Event;
        let is_routine = kind == CreateKind::Routine;
        let recurrence = is_routine.then_some(tasks::Recurrence { days: 0, anchor: day, start_minutes, minutes });
        let id = self.next_item_id();
        self.items.push(Active {
            id,
            name,
            deadline: is_event.then_some(when),
            sessions: if is_event || is_routine { Vec::new() } else { vec![Session { start: when, minutes }] },
            planned_start: None,
            // The dragged-out length doubles as the first estimate. A routine's
            // length lives in its rule instead, so it has no estimate at all.
            duration_minutes: (!is_routine).then_some(minutes),
            // A task born on the timeline is undated, and undated tasks carry
            // a horizon, not a severity. A routine is never ranked.
            importance: None,
            time_importance: (!is_event && !is_routine).then_some(NEW_TASK_HORIZON),
            is_event,
            recurrence,
            created: now,
        });
        id
    }

    /// Take an item off the board and file what became of it.
    ///
    /// Filed **before** it leaves, and it only leaves if the filing worked:
    /// an item removed first with the archive unwritable is gone with no
    /// record anywhere, and an error is poor compensation for a task that no
    /// longer exists.
    fn retire(&mut self, id: u64, outcome: Outcome, now: DateTime<Local>) -> Result<(), BoardError> {
        let index = self
            .items
            .iter()
            .position(|item| item.id == id)
            .ok_or_else(|| BoardError::gone("That item is no longer on the board."))?;
        let record = Archived::retire(self.items[index].clone(), outcome, now);
        self.archive
            .record(&self.data_dir, record)
            .map_err(|error| BoardError::failed(format!("Could not write to the archive, so nothing was changed:\n{error}")))?;
        self.items.remove(index);
        self.save_items()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::fs;

    fn at(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, hour, minute, 0).unwrap()
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn fresh() -> (Board, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let board = Board::from_parts(Vec::new(), String::new(), dir.path().to_path_buf());
        (board, dir)
    }

    fn reopen(dir: &Path) -> Board {
        let (board, problems) = Board::open(dir.to_path_buf());
        assert!(problems.is_empty(), "{problems:?}");
        board
    }

    #[test]
    fn commands_parse_from_the_wire_with_either_time_shape() {
        let command: Command = serde_json::from_str(
            r#"{"op":"move_block","id":4,"session":1,"day":"2026-09-04","start":540,"minutes":90}"#,
        )
        .unwrap();
        assert_eq!(
            command,
            Command::MoveBlock { id: 4, session: Some(1), day: day(2026, 9, 4), start: 540, minutes: 90 }
        );
        let naive: Command = serde_json::from_str(r#"{"op":"set_deadline","id":4,"deadline":"2026-09-04T17:00"}"#).unwrap();
        assert_eq!(naive, Command::SetDeadline { id: 4, deadline: Some(at(2026, 9, 4, 17, 0)) });
        let cleared: Command = serde_json::from_str(r#"{"op":"set_deadline","id":4,"deadline":null}"#).unwrap();
        assert_eq!(cleared, Command::SetDeadline { id: 4, deadline: None });
        // What this program writes reads back identically — the outbox depends on it.
        let written = serde_json::to_string(&naive).unwrap();
        assert_eq!(serde_json::from_str::<Command>(&written).unwrap(), naive);
        assert!(serde_json::from_str::<Command>(r#"{"op":"set_deadline","id":4,"deadline":"friday"}"#).is_err());
        assert!(serde_json::from_str::<Command>(r#"{"op":"explode"}"#).is_err());
    }

    #[test]
    fn a_created_task_is_saved_and_read_back() {
        let (mut board, dir) = fresh();
        let now = at(2026, 9, 4, 8, 0);
        let reply = board
            .apply(Command::Create { kind: Kind::Task, name: "  report ".into(), day: day(2026, 9, 4), start: 547, minutes: 100 }, now)
            .unwrap();
        let id = reply.id.unwrap();
        let item = board.item(id).unwrap();
        assert_eq!(item.name, "report");
        // Snapped and clamped like a drag would be.
        assert_eq!(item.sessions[0].start, at(2026, 9, 4, 9, 0));
        assert_eq!(item.sessions[0].minutes, 105);
        assert_eq!(item.duration_minutes, Some(105));
        assert_eq!(item.time_importance, Some(NEW_TASK_HORIZON));
        assert!(item.deadline.is_none());

        let again = reopen(dir.path());
        assert_eq!(again.items.len(), 1);
        assert_eq!(again.item(id).unwrap().name, "report");
        // The id counter is past what is on disk.
        assert!(again.next_id > id);
    }

    #[test]
    fn a_create_with_an_absurd_length_is_clamped_not_a_crash() {
        // Anything a client can put in a `u32` and an `i32` may arrive here.
        // The board answers with the legal block nearest to it — the whole
        // day — the way it clamps a drag past midnight, rather than letting
        // `planner::snap` overflow on the way (see its own test).
        let (mut board, _dir) = fresh();
        let now = at(2026, 9, 5, 8, 0);
        let reply = board
            .apply(Command::Create { kind: Kind::Task, name: "all day".into(), day: day(2026, 9, 5), start: i32::MIN, minutes: u32::MAX }, now)
            .unwrap();
        let item = board.item(reply.id.unwrap()).unwrap();
        assert_eq!(item.sessions[0].start, at(2026, 9, 5, 0, 0));
        assert_eq!(item.sessions[0].minutes, crate::planner::DAY_MINUTES as u32);
    }

    #[test]
    fn two_items_under_one_id_are_told_apart_at_load() {
        // Two computers' files pasted together can carry one id twice. Left
        // alone, every command aimed at the second item would land on the
        // first; the loader gives the second a fresh id, as it does an item
        // with none.
        let (mut board, dir) = fresh();
        let now = at(2026, 9, 5, 8, 0);
        let first = board
            .apply(Command::Create { kind: Kind::Task, name: "first".into(), day: day(2026, 9, 5), start: 540, minutes: 30 }, now)
            .unwrap()
            .id
            .unwrap();
        let mut twin = board.item(first).unwrap().clone();
        twin.name = "second".into();
        let mut items = board.items.clone();
        items.push(twin);
        tasks::oversafe_activesave(&items, dir.path()).unwrap();

        let mut again = reopen(dir.path());
        let second = again.items.iter().find(|item| item.name == "second").map(|item| item.id).unwrap();
        assert_ne!(second, first, "the second holder was renumbered");
        again.apply(Command::Rename { id: second, name: "renamed".into() }, now).unwrap();
        assert_eq!(again.item(first).unwrap().name, "first");
        assert_eq!(again.item(second).unwrap().name, "renamed");
    }

    #[test]
    fn a_calendar_is_subscribed_to_renamed_recoloured_switched_off_and_removed() {
        let (mut board, dir) = fresh();
        let now = at(2026, 9, 5, 8, 0);
        let id = board
            .apply(
                Command::AddSubscription {
                    name: "  Work  ".into(),
                    url: " webcal://cal.example.com/w.ics ".into(),
                    color: None,
                },
                now,
            )
            .unwrap()
            .id
            .unwrap();
        let first = board.subscriptions().first().expect("one").clone();
        assert_eq!(first.name, "Work");
        assert_eq!(first.url, "https://cal.example.com/w.ics", "webcal is rewritten, not refused");
        assert!(first.enabled);

        // The same address twice is a mistake, not a second calendar.
        assert_eq!(
            board
                .apply(
                    Command::AddSubscription { name: "Again".into(), url: "https://cal.example.com/w.ics".into(), color: None },
                    now
                )
                .unwrap_err()
                .status,
            400
        );
        // And plain http is refused: the link is the password.
        assert_eq!(
            board
                .apply(Command::AddSubscription { name: "X".into(), url: "http://cal.example.com/x.ics".into(), color: None }, now)
                .unwrap_err()
                .status,
            400
        );

        board.apply(Command::RenameSubscription { id, name: "Office".into() }, now).unwrap();
        board.apply(Command::SetSubscriptionColor { id, color: [1, 2, 3, 255] }, now).unwrap();
        board.apply(Command::SetSubscriptionEnabled { id, enabled: false }, now).unwrap();
        let changed = board.subscriptions().first().expect("one").clone();
        assert_eq!((changed.name.as_str(), changed.color, changed.enabled), ("Office", [1, 2, 3, 255], false));

        // The list is board data, so it is on disk and comes back.
        assert_eq!(reopen(dir.path()).subscriptions(), board.subscriptions());

        board.apply(Command::RemoveSubscription { id }, now).unwrap();
        assert!(board.subscriptions().is_empty());
        assert_eq!(board.apply(Command::RemoveSubscription { id }, now).unwrap_err().status, 410);
    }

    #[test]
    fn a_subscribed_meeting_flows_the_day_around_itself_the_way_one_of_ours_does() {
        let (mut board, _dir) = fresh();
        let now = at(2026, 9, 5, 8, 0);
        let d = day(2026, 9, 5);
        let id = board
            .apply(
                Command::AddSubscription { name: "Work".into(), url: "https://cal.example.com/w.ics".into(), color: None },
                now,
            )
            .unwrap()
            .id
            .unwrap();

        // A meeting from noon to one, and an all-day band on the same day.
        let mut all_day = crate::subscriptions::OverlayEvent {
            subscription: id,
            day: d,
            start: 0,
            end: planner::DAY_MINUTES,
            all_day: true,
            free: false,
            summary: "Conference".into(),
        };
        let meeting = crate::subscriptions::OverlayEvent {
            start: 12 * 60,
            end: 13 * 60,
            all_day: false,
            summary: "Sprint review".into(),
            ..all_day.clone()
        };
        all_day.all_day = true;
        board.adopt_overlay(crate::subscriptions::Overlay::sealed(vec![all_day, meeting], Vec::new()));

        // Work booked at 11:30, and the day reflowed from 11:00.
        board
            .apply(Command::Create { kind: Kind::Task, name: "Report".into(), day: d, start: 11 * 60 + 30, minutes: 60 }, now)
            .unwrap();
        let moved = board.apply(Command::Reflow { day: d, from: Some(11 * 60) }, now).unwrap().moved;
        assert_eq!(moved, Some(1));

        let item = board.items.iter().find(|item| item.name == "Report").expect("the task");
        let session = item.sessions.first().expect("its block");
        assert_eq!(session.start.hour(), 13, "pushed past the subscribed meeting, not through it");

        // The all-day band claimed nothing: it is a fact about the day, not an
        // hour that is taken.
        assert_eq!(board.overlay().anchors_on(d), vec![(12 * 60, 13 * 60)]);
    }

    #[test]
    fn an_overlay_that_says_the_same_thing_again_does_not_move_the_version() {
        // A calendar server answering identically every ten minutes must not
        // wake every parked phone and make every client refetch the board.
        let (mut board, _dir) = fresh();
        let overlay = crate::subscriptions::Overlay::sealed(
            vec![crate::subscriptions::OverlayEvent {
                subscription: 1,
                day: day(2026, 9, 5),
                start: 600,
                end: 660,
                all_day: false,
                free: false,
                summary: "Standup".into(),
            }],
            Vec::new(),
        );
        assert!(board.adopt_overlay(overlay.clone()), "the first one is a change");
        let settled = board.version();
        assert!(!board.adopt_overlay(overlay), "the same again is not");
        assert_eq!(board.version(), settled);
    }

    #[test]
    fn creates_take_the_shape_their_kind_asks_for() {
        let (mut board, _dir) = fresh();
        let now = at(2026, 9, 4, 8, 0);
        let event = board.apply(Command::Create { kind: Kind::Event, name: "".into(), day: day(2026, 9, 5), start: 600, minutes: 45 }, now).unwrap().id.unwrap();
        let routine = board.apply(Command::Create { kind: Kind::Routine, name: "sleep".into(), day: day(2026, 9, 5), start: 1380, minutes: 60 }, now).unwrap().id.unwrap();
        let e = board.item(event).unwrap();
        assert_eq!(e.name, "New event");
        assert!(e.is_event);
        assert_eq!(e.deadline, Some(at(2026, 9, 5, 10, 0)));
        assert!(e.sessions.is_empty());
        let r = board.item(routine).unwrap();
        let rule = r.recurrence.unwrap();
        assert_eq!((rule.days, rule.anchor, rule.start_minutes, rule.minutes), (0, day(2026, 9, 5), 1380, 60));
        assert!(r.duration_minutes.is_none());
        // Add: an event without a time is refused; a task is fine.
        assert_eq!(
            board.apply(Command::Add { name: "x".into(), deadline: None, importance: None, is_event: true, time_importance: None }, now).unwrap_err().status,
            400
        );
        let task = board.apply(Command::Add { name: "x".into(), deadline: None, importance: None, is_event: false, time_importance: Some(2) }, now).unwrap().id.unwrap();
        assert_eq!(board.item(task).unwrap().time_importance, Some(2));
        // QuickAdd with a due date carries a middling severity.
        let dated = board.apply(Command::QuickAdd { name: "bill".into(), deadline: Some(at(2026, 9, 6, 12, 0)) }, now).unwrap().id.unwrap();
        assert_eq!(board.item(dated).unwrap().importance, Some(NEW_TASK_IMPORTANCE));
        assert_eq!(board.apply(Command::QuickAdd { name: "   ".into(), deadline: None }, now).unwrap_err().status, 400);
    }

    #[test]
    fn blocks_move_book_remove_and_unplan() {
        let (mut board, _dir) = fresh();
        let now = at(2026, 9, 4, 8, 0);
        let d = day(2026, 9, 4);
        let id = board.apply(Command::Create { kind: Kind::Task, name: "t".into(), day: d, start: 540, minutes: 60 }, now).unwrap().id.unwrap();
        board.apply(Command::SetEstimate { id, minutes: 120 }, now).unwrap();

        // Book the rest without saying how long: the remaining estimate.
        let reply = board.apply(Command::AddBlock { id, day: d, start: 840, minutes: None }, now).unwrap();
        assert_eq!(reply.session, Some(1));
        assert_eq!(board.item(id).unwrap().sessions[1].minutes, 60);
        assert_eq!(board.item(id).unwrap().remaining_minutes(), Some(0));

        board.apply(Command::MoveBlock { id, session: Some(1), day: d, start: 907, minutes: 100 }, now).unwrap();
        let s = board.item(id).unwrap().sessions[1];
        assert_eq!((s.start, s.minutes), (at(2026, 9, 4, 15, 0), 105));

        assert_eq!(board.apply(Command::MoveBlock { id, session: Some(7), day: d, start: 900, minutes: 60 }, now).unwrap_err().status, 410);
        board.apply(Command::RemoveBlock { id, session: 0 }, now).unwrap();
        assert_eq!(board.item(id).unwrap().sessions.len(), 1);
        board.apply(Command::Unplan { id }, now).unwrap();
        let item = board.item(id).unwrap();
        assert!(item.sessions.is_empty());
        assert_eq!(item.duration_minutes, Some(120), "the estimate survives unplanning");
        assert_eq!(board.apply(Command::AddBlock { id: 99, day: d, start: 600, minutes: None }, now).unwrap_err().status, 410);
    }

    #[test]
    fn events_and_routines_move_their_own_time() {
        let (mut board, _dir) = fresh();
        let now = at(2026, 9, 4, 8, 0);
        let d = day(2026, 9, 4);
        let event = board.apply(Command::Create { kind: Kind::Event, name: "dentist".into(), day: d, start: 630, minutes: 45 }, now).unwrap().id.unwrap();
        board.apply(Command::MoveBlock { id: event, session: None, day: d, start: 600, minutes: 60 }, now).unwrap();
        let e = board.item(event).unwrap();
        assert_eq!((e.deadline, e.duration_minutes), (Some(at(2026, 9, 4, 10, 0)), Some(60)));
        // An event's length past midnight pulls its start back.
        board.apply(Command::MoveBlock { id: event, session: None, day: d, start: 1410, minutes: 60 }, now).unwrap();
        assert_eq!(board.item(event).unwrap().deadline, Some(at(2026, 9, 4, 23, 0)));
        assert_eq!(board.apply(Command::AddBlock { id: event, day: d, start: 600, minutes: None }, now).unwrap_err().status, 400);
        assert_eq!(board.apply(Command::Unplan { id: event }, now).unwrap_err().status, 400);
        assert_eq!(board.apply(Command::SetDeadline { id: event, deadline: None }, now).unwrap_err().status, 400);

        let routine = board.apply(Command::Create { kind: Kind::Routine, name: "sleep".into(), day: d, start: 1380, minutes: 60 }, now).unwrap().id.unwrap();
        board.apply(Command::SetRepeat { id: routine, days: 0b0101_0101, day: d }, now).unwrap();
        assert_eq!(board.item(routine).unwrap().recurrence.unwrap().days, 0b0101_0101);
        board.apply(Command::MoveBlock { id: routine, session: None, day: d, start: 1350, minutes: 90 }, now).unwrap();
        let rule = board.item(routine).unwrap().recurrence.unwrap();
        assert_eq!((rule.start_minutes, rule.minutes), (1350, 90));
        // Back to once, on the day being looked at.
        board.apply(Command::SetRepeat { id: routine, days: 0, day: day(2026, 9, 9) }, now).unwrap();
        assert_eq!(board.item(routine).unwrap().recurrence.unwrap().anchor, day(2026, 9, 9));
        assert_eq!(board.apply(Command::SetRepeat { id: event, days: 1, day: d }, now).unwrap_err().status, 400);
        assert_eq!(board.apply(Command::SetEstimate { id: routine, minutes: 30 }, now).unwrap().moved, None);
        assert_eq!(board.item(routine).unwrap().recurrence.unwrap().minutes, 30);
    }

    #[test]
    fn the_ranking_knobs_follow_the_deadline() {
        let (mut board, _dir) = fresh();
        let now = at(2026, 9, 4, 8, 0);
        let id = board.apply(Command::QuickAdd { name: "t".into(), deadline: None }, now).unwrap().id.unwrap();
        board.apply(Command::SetHorizon { id, level: 3 }, now).unwrap();
        assert_eq!(board.item(id).unwrap().time_importance, Some(3));
        assert_eq!(board.apply(Command::SetHorizon { id, level: 4 }, now).unwrap_err().status, 400);
        // A first deadline seeds a middling severity...
        board.apply(Command::SetDeadline { id, deadline: Some(at(2026, 9, 6, 12, 0)) }, now).unwrap();
        assert_eq!(board.item(id).unwrap().importance, Some(NEW_TASK_IMPORTANCE));
        board.apply(Command::SetSeverity { id, level: 4 }, now).unwrap();
        assert_eq!(board.apply(Command::SetSeverity { id, level: 5 }, now).unwrap_err().status, 400);
        // ...and clearing it keeps the horizon that was there.
        board.apply(Command::SetDeadline { id, deadline: None }, now).unwrap();
        let item = board.item(id).unwrap();
        assert_eq!((item.deadline, item.time_importance, item.importance), (None, Some(3), Some(4)));
        board.apply(Command::Rename { id, name: "  named ".into() }, now).unwrap();
        assert_eq!(board.item(id).unwrap().name, "named");
        assert_eq!(board.apply(Command::Rename { id, name: " ".into() }, now).unwrap_err().status, 400);
    }

    #[test]
    fn retiring_files_first_and_restoring_brings_it_back_whole() {
        let (mut board, dir) = fresh();
        let now = at(2026, 9, 4, 8, 0);
        let d = day(2026, 9, 4);
        let id = board.apply(Command::Create { kind: Kind::Task, name: "t".into(), day: d, start: 540, minutes: 60 }, now).unwrap().id.unwrap();
        let event = board.apply(Command::Create { kind: Kind::Event, name: "e".into(), day: d, start: 600, minutes: 30 }, now).unwrap().id.unwrap();

        assert_eq!(board.apply(Command::Complete { id: event, at: None }, now).unwrap_err().status, 400);
        board.apply(Command::Complete { id, at: None }, at(2026, 9, 4, 11, 0)).unwrap();
        assert!(board.item(id).is_none());
        assert_eq!(board.apply(Command::Complete { id, at: None }, now).unwrap_err().status, 410);
        board.apply(Command::Delete { id: event, at: None }, at(2026, 9, 4, 12, 0)).unwrap();
        assert!(board.items.is_empty());

        // The log has both, newest first, and survives a reopen.
        let mut again = reopen(dir.path());
        again.load_archive().unwrap();
        let rows: Vec<_> = again.archive.entries().iter().map(|r| (r.name.clone(), r.outcome)).collect();
        assert_eq!(rows, vec![("e".to_string(), Outcome::Dropped), ("t".to_string(), Outcome::Finished)]);

        // Restore brings the session back, under the old id since it is free.
        let key = again.archive.entries()[1].key();
        let restored = again.apply(Command::Restore { id: key.id, archived_at: key.archived_at }, now).unwrap().id.unwrap();
        assert_eq!(restored, id);
        assert_eq!(again.item(id).unwrap().sessions.len(), 1);
        assert_eq!(again.archive.entries().len(), 1);
        assert_eq!(again.apply(Command::Restore { id: key.id, archived_at: key.archived_at }, now).unwrap_err().status, 410);

        // Forget is final.
        let key = again.archive.entries()[0].key();
        again.apply(Command::ForgetArchived { id: key.id, archived_at: key.archived_at }, now).unwrap();
        assert!(again.archive.entries().is_empty());
        assert_eq!(reopen(dir.path()).items.len(), 1);

        // Forget (live) drops without a record.
        again.apply(Command::Forget { id }, now).unwrap();
        assert!(again.items.is_empty());
        again.load_archive().unwrap();
        assert!(again.archive.entries().is_empty());
    }

    #[test]
    fn reflow_and_notes_and_queries() {
        let (mut board, dir) = fresh();
        let d = day(2026, 9, 4);
        let morning = at(2026, 9, 4, 8, 0);
        let a = board.apply(Command::Create { kind: Kind::Task, name: "a".into(), day: d, start: 540, minutes: 60 }, morning).unwrap().id.unwrap();
        board.apply(Command::Create { kind: Kind::Event, name: "fixed".into(), day: d, start: 720, minutes: 60 }, morning).unwrap();
        let noon = at(2026, 9, 4, 12, 5);
        assert_eq!(board.behind_minutes(d, noon), 60);
        let reply = board.apply(Command::Reflow { day: d, from: None }, noon).unwrap();
        assert_eq!(reply.moved, Some(1));
        // 12:05 rounds up to 12:15, inside the event, so the block lands after it.
        assert_eq!(board.item(a).unwrap().sessions[0].start, at(2026, 9, 4, 13, 0));
        assert_eq!(board.behind_minutes(d, noon), 0);
        // Not today: nothing to reflow.
        assert_eq!(board.apply(Command::Reflow { day: day(2026, 9, 5), from: None }, noon).unwrap().moved, Some(0));

        board.apply(Command::SetNotes { text: "milk\n\teggs".into() }, noon).unwrap();
        assert_eq!(board.notes, "milk\n    eggs");
        assert_eq!(reopen(dir.path()).notes, "milk\n    eggs");
        assert_eq!(board.apply(Command::SetNotes { text: "x".repeat(NOTES_MAX_CHARS + 1) }, noon).unwrap_err().status, 400);

        assert_eq!(board.apply(Command::Feed, noon).unwrap_err().status, 400);
        assert_eq!(board.ranked_ids(noon), vec![a]);
        assert_eq!(board.next_free_start(d), 14 * 60);
        assert_eq!(board.next_free_start(day(2026, 9, 5)), DEFAULT_MORNING_HOUR * 60);
        assert!(board.version() > 0);
    }

    #[test]
    fn dates_off_the_calendar_are_refused_before_anything_adds_to_them() {
        let (mut board, _dir) = fresh();
        let now = at(2026, 9, 4, 12, 0);
        let far = NaiveDate::from_ymd_opt(262142, 12, 31).unwrap();
        let early = NaiveDate::from_ymd_opt(1200, 1, 1).unwrap();
        let id = board.apply(Command::QuickAdd { name: "t".into(), deadline: None }, now).unwrap().id.unwrap();
        for command in [
            Command::Create { kind: Kind::Task, name: "x".into(), day: far, start: 600, minutes: 30 },
            Command::AddBlock { id, day: far, start: 600, minutes: None },
            Command::MoveBlock { id, session: Some(0), day: early, start: 600, minutes: 30 },
            Command::Reflow { day: far, from: Some(600) },
            Command::SetRepeat { id, days: 0, day: far },
            Command::SetDeadline { id, deadline: Some(Local.with_ymd_and_hms(262142, 12, 31, 10, 0, 0).unwrap()) },
            Command::Complete { id, at: Some(Local.with_ymd_and_hms(262142, 12, 31, 10, 0, 0).unwrap()) },
        ] {
            let error = board.apply(command, now).unwrap_err();
            assert_eq!(error.status, 400, "{}", error.message);
            assert!(error.message.contains("out of range"), "{}", error.message);
        }
        assert!(board.item(id).unwrap().sessions.is_empty(), "nothing was applied");
        // Off the wire as text, the same: not a time.
        assert!(parse_local_input("+262142-12-31T10:00").is_none());
        assert!(parse_local_input("2026-09-04T10:00").is_some());
    }

    #[test]
    fn an_add_with_nothing_to_rank_it_by_gets_the_quick_adds_horizon() {
        let (mut board, _dir) = fresh();
        let now = at(2026, 9, 4, 12, 0);
        let bare = Command::Add { name: "bare".into(), deadline: None, importance: None, is_event: false, time_importance: None };
        let id = board.apply(bare, now).unwrap().id.unwrap();
        assert_eq!(board.item(id).unwrap().time_importance, Some(NEW_TASK_HORIZON));
        // Given a level, it keeps it; an event is ranked by nothing.
        let ranked = Command::Add { name: "soon".into(), deadline: None, importance: None, is_event: false, time_importance: Some(2) };
        let id = board.apply(ranked, now).unwrap().id.unwrap();
        assert_eq!(board.item(id).unwrap().time_importance, Some(2));
        let event = Command::Add { name: "gig".into(), deadline: Some(now), importance: None, is_event: true, time_importance: None };
        let id = board.apply(event, now).unwrap().id.unwrap();
        assert_eq!(board.item(id).unwrap().time_importance, None);
    }

    #[test]
    fn forgetting_something_that_is_not_there_is_gone_not_fine() {
        let (mut board, _dir) = fresh();
        let now = at(2026, 9, 4, 12, 0);
        let before = board.version();
        assert_eq!(board.apply(Command::Forget { id: 404 }, now).unwrap_err().status, 410);
        assert_eq!(board.version(), before, "nothing changed, nothing saved");
    }

    #[test]
    fn a_reflow_from_a_client_slides_the_day_from_the_clients_now() {
        // Applied on a day that is not today here, with the `from` the
        // client saw: the slide is the client's, not this clock's.
        let (mut board, _dir) = fresh();
        let now = at(2026, 9, 10, 12, 0);
        let d = day(2026, 9, 4);
        let a = board.apply(Command::Create { kind: Kind::Task, name: "a".into(), day: d, start: 540, minutes: 60 }, now).unwrap().id.unwrap();
        let b = board.apply(Command::Create { kind: Kind::Task, name: "b".into(), day: d, start: 600, minutes: 30 }, now).unwrap().id.unwrap();
        let reply = board.apply(Command::Reflow { day: d, from: Some(620) }, now).unwrap();
        assert_eq!(reply.moved, Some(2));
        let start = |id: u64| board.item(id).unwrap().sessions[0].start.hour() as i32 * 60 + board.item(id).unwrap().sessions[0].start.minute() as i32;
        assert_eq!((start(a), start(b)), (630, 690));
        // Without `from`, a day that is not today is left alone.
        assert_eq!(board.apply(Command::Reflow { day: d, from: None }, now).unwrap().moved, Some(0));
    }

    #[test]
    fn a_command_is_stamped_with_the_clock_of_the_host_that_applied_it_first() {
        let now = at(2026, 9, 4, 12, 0);
        let d = now.date_naive();
        assert_eq!(Command::Complete { id: 3, at: None }.stamped(now), Command::Complete { id: 3, at: Some(now) });
        let earlier = at(2026, 9, 4, 8, 0);
        assert_eq!(Command::Delete { id: 3, at: Some(earlier) }.stamped(now), Command::Delete { id: 3, at: Some(earlier) });
        assert_eq!(Command::Reflow { day: d, from: None }.stamped(now), Command::Reflow { day: d, from: planner::now_marker(d, now) });
        assert_eq!(Command::Unplan { id: 1 }.stamped(now), Command::Unplan { id: 1 });
        // A restore is addressed by its id like a live item, so a remap
        // follows it into the archive.
        assert_eq!(Command::Restore { id: 5, archived_at: now }.item_id(), Some(5));
        let mut forget = Command::ForgetArchived { id: 5, archived_at: now };
        forget.set_item_id(9);
        assert_eq!(forget.item_id(), Some(9));

        // A client clock a day or more ahead does not pin the row to the top
        // of the ledger for years; a little ahead is kept as given.
        let (mut board, _dir) = fresh();
        let id = board.apply(Command::QuickAdd { name: "t".into(), deadline: None }, now).unwrap().id.unwrap();
        board.apply(Command::Complete { id, at: Some(at(2031, 1, 1, 0, 0)) }, now).unwrap();
        board.load_archive().unwrap();
        assert_eq!(board.archive.entries()[0].archived_at, now + chrono::Duration::days(1));
        let id = board.apply(Command::QuickAdd { name: "u".into(), deadline: None }, now).unwrap().id.unwrap();
        let ahead = now + chrono::Duration::minutes(90);
        board.apply(Command::Complete { id, at: Some(ahead) }, now).unwrap();
        // Filed where its time puts it: behind the row stamped a day ahead.
        let row = board.archive.entries().iter().find(|row| row.name == "u").unwrap();
        assert_eq!(row.archived_at, ahead);
        assert_eq!(board.archive.entries()[1].name, "u");
    }

    #[test]
    fn a_board_that_becomes_its_own_adopts_its_temporary_items() {
        // A replica cache with two items created offline under temporary ids
        // and one real one, opened as a board of its own.
        let (mut client, dir) = fresh();
        let now = at(2026, 9, 4, 12, 0);
        let real = client.apply(Command::QuickAdd { name: "real".into(), deadline: None }, now).unwrap().id.unwrap();
        client.number_from(TEMPORARY_ID_FLOOR);
        let temp_a = client.apply(Command::QuickAdd { name: "a".into(), deadline: None }, now).unwrap().id.unwrap();
        let temp_b = client.apply(Command::QuickAdd { name: "b".into(), deadline: None }, now).unwrap().id.unwrap();
        assert!(temp_a >= TEMPORARY_ID_FLOOR && temp_b > temp_a);

        let mut own = reopen(dir.path());
        assert_eq!(own.adopt_temporaries().unwrap(), 2);
        let ids: Vec<u64> = own.items.iter().map(|item| item.id).collect();
        assert!(ids.iter().all(|id| *id < TEMPORARY_ID_FLOOR), "{ids:?}");
        assert_eq!(ids[0], real);
        // Numbered on from the real ones, and the next creation follows.
        let next = own.apply(Command::QuickAdd { name: "c".into(), deadline: None }, now).unwrap().id.unwrap();
        assert_eq!(next, ids[2] + 1);
        assert_eq!(reopen(dir.path()).items.len(), 4, "saved");
        assert_eq!(own.adopt_temporaries().unwrap(), 0, "nothing left to adopt");
    }

    #[test]
    fn a_restored_row_wearing_a_temporary_id_is_renumbered() {
        // A client's replica retires something it created offline; the row
        // in its cache wears the temporary id. Restored on a board that hands
        // out real ids, it gets one.
        let (mut client, dir) = fresh();
        client.number_from(TEMPORARY_ID_FLOOR);
        let now = at(2026, 9, 4, 12, 0);
        let temp = client.apply(Command::QuickAdd { name: "offline".into(), deadline: None }, now).unwrap().id.unwrap();
        assert!(temp >= TEMPORARY_ID_FLOOR);
        client.apply(Command::Complete { id: temp, at: None }, now).unwrap();

        let mut server = reopen(dir.path());
        server.load_archive().unwrap();
        let key = server.archive.entries()[0].key();
        assert!(key.id >= TEMPORARY_ID_FLOOR);
        let back = server.apply(Command::Restore { id: key.id, archived_at: key.archived_at }, now).unwrap().id.unwrap();
        assert!(back < TEMPORARY_ID_FLOOR, "renumbered, got {back}");
        assert_eq!(server.item(back).unwrap().name, "offline");
    }

    #[cfg(unix)]
    #[test]
    fn the_board_state_is_an_error_rather_than_an_empty_archive_when_the_log_cannot_be_read() {
        use std::os::unix::fs::PermissionsExt;
        let (mut board, dir) = fresh();
        let now = at(2026, 9, 4, 12, 0);
        let id = board.apply(Command::QuickAdd { name: "done".into(), deadline: None }, now).unwrap().id.unwrap();
        board.apply(Command::Complete { id, at: None }, now).unwrap();
        let log = dir.path().join("archived.jsonl");
        fs::set_permissions(&log, fs::Permissions::from_mode(0o000)).unwrap();
        let mut again = reopen(dir.path());
        let result = again.state();
        fs::set_permissions(&log, fs::Permissions::from_mode(0o644)).unwrap();
        // Root reads anything; everyone else is refused, and told.
        if nix_is_root() {
            return;
        }
        assert!(result.is_err(), "an unreadable log must not be served as an empty one");
        assert_eq!(Board::unreadable_files(dir.path()), Vec::<String>::new(), "readable again");
    }

    #[cfg(unix)]
    fn nix_is_root() -> bool {
        std::fs::metadata("/").map(|m| {
            use std::os::unix::fs::MetadataExt;
            m.uid() == unsafe { libc_geteuid() }
        }).unwrap_or(false)
    }

    #[cfg(unix)]
    unsafe extern "C" {
        #[link_name = "geteuid"]
        fn libc_geteuid() -> u32;
    }

    #[test]
    fn a_name_is_trimmed_and_bounded_everywhere_a_name_comes_in() {
        let dir = tempfile::tempdir().unwrap();
        let (mut board, _) = Board::open(dir.path().to_path_buf());
        let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
        let d = now.date_naive();
        let longest = "n".repeat(NAME_MAX_CHARS);
        let too_long = format!("{longest}!");

        // At the limit, in; one over, refused — by every way in, with one message.
        let id = board.apply(Command::QuickAdd { name: format!("  {longest}  "), deadline: None }, now).unwrap().id.unwrap();
        assert_eq!(board.item(id).unwrap().name, longest);
        for command in [
            Command::QuickAdd { name: too_long.clone(), deadline: None },
            Command::Add { name: too_long.clone(), deadline: None, importance: None, is_event: false, time_importance: Some(1) },
            Command::Rename { id, name: too_long.clone() },
            Command::Create { kind: Kind::Task, name: too_long.clone(), day: d, start: 600, minutes: 30 },
        ] {
            let error = board.apply(command, now).unwrap_err();
            assert_eq!(error.status, 400);
            assert!(error.message.contains("200"), "{}", error.message);
        }
        assert_eq!(board.items.len(), 1, "nothing over the limit was let in");
        // Characters, not bytes: two hundred four-byte characters are fine.
        assert!(board.apply(Command::Rename { id, name: "𝄞".repeat(NAME_MAX_CHARS) }, now).is_ok());
    }

    #[test]
    fn a_level_off_the_scale_is_refused_at_add() {
        // Only the footer's setters checked the scale; a client could add an
        // item with severity 9 straight in, and every later draw would index
        // a table by it.
        let dir = tempfile::tempdir().unwrap();
        let (mut board, _) = Board::open(dir.path().to_path_buf());
        let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
        let over = Command::Add { name: "loud".into(), deadline: Some(now), importance: Some(9), is_event: false, time_importance: None };
        assert_eq!(board.apply(over, now).unwrap_err().status, 400);
        let far = Command::Add { name: "someday".into(), deadline: None, importance: None, is_event: false, time_importance: Some(4) };
        assert_eq!(board.apply(far, now).unwrap_err().status, 400);
        assert!(board.items.is_empty());
        let top = Command::Add { name: "fine".into(), deadline: Some(now), importance: Some(4), is_event: false, time_importance: None };
        assert!(board.apply(top, now).is_ok());
    }

    #[test]
    fn nothing_runs_past_midnight() {
        let (mut board, _dir) = fresh();
        let now = at(2026, 9, 4, 8, 0);
        let d = day(2026, 9, 4);
        // A create at the very end of the day is pulled back to fit, never cut.
        let id = board.apply(Command::Create { kind: Kind::Task, name: "late".into(), day: d, start: 1440, minutes: 30 }, now).unwrap().id.unwrap();
        let s = board.item(id).unwrap().sessions[0];
        assert_eq!((s.start, s.minutes), (at(2026, 9, 4, 23, 30), 30));
        // Moving a block so it would cross midnight pulls its start back.
        board.apply(Command::MoveBlock { id, session: Some(0), day: d, start: 1425, minutes: 60 }, now).unwrap();
        let s = board.item(id).unwrap().sessions[0];
        assert_eq!((s.start, s.minutes), (at(2026, 9, 4, 23, 0), 60));
        // So does lengthening an event's block.
        let event = board.apply(Command::Create { kind: Kind::Event, name: "e".into(), day: d, start: 1410, minutes: 15 }, now).unwrap().id.unwrap();
        board.apply(Command::SetEstimate { id: event, minutes: 120 }, now).unwrap();
        let e = board.item(event).unwrap();
        assert_eq!((e.deadline, e.duration_minutes), (Some(at(2026, 9, 4, 22, 0)), Some(120)));
        // A routine's rule is clamped the same way, and a length is never less than a quarter hour.
        let routine = board.apply(Command::Create { kind: Kind::Routine, name: "r".into(), day: d, start: 1430, minutes: 5 }, now).unwrap().id.unwrap();
        let rule = board.item(routine).unwrap().recurrence.unwrap();
        assert_eq!((rule.start_minutes, rule.minutes), (1425, 15));
        assert_eq!(board.apply(Command::SetEstimate { id, minutes: 0 }, now).unwrap_err().status, 400);
    }

    #[test]
    fn a_legacy_save_loads_migrated_and_a_corrupt_one_is_quarantined() {
        let dir = tempfile::tempdir().unwrap();
        // Pre-id, pre-sessions shape: a single planned slot and no `id`.
        fs::write(
            dir.path().join("read_at_startup.json"),
            r#"[{"importance":2,"time_importance":null,"name":"old","created":"2026-09-01T09:00:00+03:00",
                 "deadline":"2026-09-05T17:00:00+03:00","is_event":false,
                 "planned_start":"2026-09-03T10:00:00+03:00","duration_minutes":90}]"#,
        )
        .unwrap();
        let (board, problems) = Board::open(dir.path().to_path_buf());
        assert!(problems.is_empty(), "{problems:?}");
        let item = &board.items[0];
        assert!(item.id > 0, "an id is backfilled");
        assert_eq!(item.sessions.len(), 1, "the legacy slot became a session");
        assert_eq!(item.sessions[0].minutes, 90);
        assert!(item.planned_start.is_none());
        assert_eq!(item.duration_minutes, Some(90), "the slot's length doubles as the estimate");

        fs::write(dir.path().join("read_at_startup.json"), "{this is not json").unwrap();
        let (board, problems) = Board::open(dir.path().to_path_buf());
        assert!(board.items.is_empty());
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("read_at_startup.json"), "{}", problems[0]);
        let quarantined = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(quarantined, "the bad file is set aside, not deleted");
    }

    #[test]
    fn a_routine_comes_back_as_a_routine_and_the_log_loads_itself_for_a_restore() {
        let (mut board, dir) = fresh();
        let now = at(2026, 9, 4, 8, 0);
        let d = day(2026, 9, 4);
        let id = board.apply(Command::Create { kind: Kind::Routine, name: "sleep".into(), day: d, start: 23 * 60, minutes: 60 }, now).unwrap().id.unwrap();
        board.apply(Command::SetRepeat { id, days: tasks::EVERY_DAY, day: d }, now).unwrap();
        // A routine cannot be finished, only deleted — and deleting keeps a record.
        assert_eq!(board.apply(Command::Complete { id, at: None }, now).unwrap_err().status, 400);
        board.apply(Command::Delete { id, at: None }, now).unwrap();
        assert!(board.items.is_empty());
        // The record went to disk; this board's log was never read, so it
        // holds nothing until asked (§17.2).
        assert!(board.archive.entries().is_empty());
        board.load_archive().unwrap();
        let key = board.archive.entries()[0].key();

        // A fresh open has not read the log; Restore reads it itself.
        let mut again = reopen(dir.path());
        assert!(!again.archive.is_loaded());
        let back = again.apply(Command::Restore { id: key.id, archived_at: key.archived_at }, now).unwrap().id.unwrap();
        assert_eq!(back, id);
        let item = again.item(id).unwrap();
        assert!(item.is_routine(), "a restored routine is still a routine");
        let rule = item.recurrence.unwrap();
        assert_eq!((rule.days, rule.start_minutes, rule.minutes), (tasks::EVERY_DAY, 23 * 60, 60));
        assert!(!planner::placements_for(item, d + chrono::Duration::days(3)).is_empty(), "and it is back on its days");

        // Forgetting a row that is not there is 410, on an unloaded log too.
        let mut third = reopen(dir.path());
        assert_eq!(third.apply(Command::ForgetArchived { id: key.id, archived_at: key.archived_at }, now).unwrap_err().status, 410);
    }

    #[test]
    fn a_restored_item_whose_id_is_taken_gets_a_fresh_one() {
        let (mut board, _dir) = fresh();
        let now = at(2026, 9, 4, 8, 0);
        let id = board.apply(Command::QuickAdd { name: "first".into(), deadline: None }, now).unwrap().id.unwrap();
        board.apply(Command::Complete { id, at: None }, now).unwrap();
        // Something else now wears that id — a hand-edited save, say.
        let mut squatter = board.items.first().cloned().unwrap_or_else(|| {
            Active {
                id,
                importance: None,
                time_importance: Some(1),
                name: "squatter".into(),
                created: now,
                deadline: None,
                is_event: false,
                sessions: Vec::new(),
                planned_start: None,
                duration_minutes: None,
                recurrence: None,
            }
        });
        squatter.id = id;
        squatter.name = "squatter".into();
        board.items.push(squatter);

        let key = { board.load_archive().unwrap(); board.archive.entries()[0].key() };
        let restored = board.apply(Command::Restore { id: key.id, archived_at: key.archived_at }, now).unwrap().id.unwrap();
        assert_ne!(restored, id, "the old id was taken");
        assert_eq!(board.item(restored).unwrap().name, "first");
        assert_eq!(board.item(id).unwrap().name, "squatter");
        // And the counter is past both, so the next thing collides with neither.
        let next = board.apply(Command::QuickAdd { name: "next".into(), deadline: None }, now).unwrap().id.unwrap();
        assert!(next > restored && next > id);
    }

    #[test]
    fn replace_takes_the_servers_picture_and_keeps_ids_ahead() {
        let (mut board, _dir) = fresh();
        let now = at(2026, 9, 4, 8, 0);
        board.number_from(1 << 62);
        let local = board.apply(Command::QuickAdd { name: "offline".into(), deadline: None }, now).unwrap().id.unwrap();
        assert!(local >= 1 << 62);
        let before = board.version();
        let mut theirs = board.items.clone();
        theirs[0].id = 5;
        board.replace(theirs, None, "server notes".into(), Vec::new(), Overlay::default());
        assert_eq!(board.item(5).unwrap().name, "offline");
        assert_eq!(board.notes, "server notes");
        assert!(board.version() > before);
        // New local ids still come from the reserved range.
        let next = board.apply(Command::QuickAdd { name: "again".into(), deadline: None }, now).unwrap().id.unwrap();
        assert!(next >= 1 << 62);
        // A confirmed creation takes its real id.
        assert!(board.renumber(next, 9));
        assert!(board.item(9).is_some() && board.item(next).is_none());
        assert!(!board.renumber(next, 10));
        board.save_all().unwrap();
    }
}
