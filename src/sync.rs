//! The desktop as a client of `taskdeck-server`.
//!
//! When `server_url` is set, the board lives on the server and the desktop
//! keeps a **replica**: it loads the board from the server at startup, applies
//! every edit locally at once (so the UI never waits on the network) and sends
//! the same command to the server, and takes the server's picture whenever the
//! server says it changed. The replica is saved to this machine's
//! `taskdeck_data/` like any board, which is what lets the desktop start, and
//! be used, with the server unreachable.
//!
//! ## Offline
//!
//! Commands that cannot be sent wait in an **outbox** — a small file of
//! commands in order — and are replayed in order when the server is back. Two
//! things make that safe rather than a merge:
//!
//! - Edits are commands, not files. "Move block 2 of task 17 to 14:30"
//!   replays cleanly against whatever the server has; nobody's whole board
//!   overwrites anybody's.
//! - Something created offline has a **temporary id** from a range the server
//!   never issues (`Board::number_from`). When its creation is replayed the
//!   server answers with the real id, every later queued command that named
//!   the temporary one is re-pointed, and the UI is told so its selection
//!   follows.
//!
//! Conflicts are resolved per command, last writer wins, at replay time. A
//! command the server refuses — the item was finished from the phone
//! meanwhile, say — is dropped and **reported**, never retried forever and
//! never silently lost.
//!
//! ## Threads
//!
//! Two, both plain and blocking, in the house style (§5.6):
//!
//! - The **sender** owns the outbox. It takes commands from the UI, appends
//!   them, and flushes the front of the queue to the server; a network failure
//!   marks the desktop offline and the flush is retried on a short timer.
//! - The **listener** long-polls `/api/wait` and, when the version moves,
//!   fetches the board and hands it to the UI — but only while nothing is
//!   waiting to be sent, and only if it is at least as new as the last reply
//!   the sender got, so a fetch racing an edit can never show the board
//!   without it.
//!
//! Both report through one shared [`Status`] the menu bar reads each frame,
//! and through [`SyncEvent`]s the UI drains with the phone's queue.

use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, Sender, channel},
    },
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::{
    board::{BoardState, Command},
    phone::Wake,
};

/// File the outbox lives in, inside `taskdeck_data/`.
pub const OUTBOX_FILE: &str = "outbox.json";

/// First id a client hands out to things it creates. Far above anything a
/// server will ever reach, so a temporary id can never be mistaken for a real
/// one, and remapping is the only way one becomes the other.
pub const CLIENT_ID_FLOOR: u64 = crate::board::TEMPORARY_ID_FLOOR;

/// How long the sender waits for a reply before calling the server
/// unreachable. Short: the UI has already applied the edit, and a slow
/// server is not worth more than this per command.
const SEND_TIMEOUT: Duration = Duration::from_secs(8);
/// How often the sender retries the outbox while offline. Three seconds in
/// the program; a fraction of that under `cargo test`, where the same pacing
/// is exercised without the suite waiting on it.
const RETRY_EVERY: Duration = if cfg!(test) { Duration::from_millis(300) } else { Duration::from_secs(3) };
/// Longer than the server's own 25 s wait, plus slack for the connection.
const WAIT_TIMEOUT: Duration = Duration::from_secs(40);
/// How long the startup fetch waits before falling back to the local cache.
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(4);
/// Backoff for the listener after a failure: up to this.
const LISTEN_BACKOFF_MAX: Duration = Duration::from_secs(30);

/* ─────────────────────────────── The remote ─────────────────────────────── */

/// Why a call to the server did not go through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteError {
    /// Nothing answered, or not in time. Try again later.
    Unreachable(String),
    /// The server answered and said no. Do not try again.
    Rejected { status: u16, message: String },
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteError::Unreachable(why) => write!(f, "server unreachable: {why}"),
            RemoteError::Rejected { status, message } => write!(f, "{message} ({status})"),
        }
    }
}

/// What the server says about a carried-out command.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WireReply {
    pub version: u64,
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub session: Option<usize>,
    #[serde(default)]
    pub moved: Option<usize>,
}

/// The server, as three calls.
#[derive(Clone)]
pub struct Remote {
    base: String,
    token: String,
    client: reqwest::blocking::Client,
}

impl Remote {
    /// `base` is `http://host:port`, with or without a trailing slash.
    pub fn new(base: &str, token: &str, timeout: Duration) -> Result<Self, String> {
        let base = base.trim().trim_end_matches('/').to_string();
        if !base.starts_with("http://") && !base.starts_with("https://") {
            return Err(format!("The server address should start with http:// or https://, not `{base}`."));
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { base, token: token.trim().to_string(), client })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn get(&self, path: &str) -> Result<serde_json::Value, RemoteError> {
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .header("X-TaskDeck-Token", &self.token)
            .send()
            .map_err(|error| RemoteError::Unreachable(error.to_string()))?;
        Self::read(response)
    }

    fn read(response: reqwest::blocking::Response) -> Result<serde_json::Value, RemoteError> {
        let status = response.status().as_u16();
        let text = response
            .text()
            .map_err(|error| RemoteError::Unreachable(format!("unreadable answer: {error}")))?;
        let body: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| RemoteError::Unreachable(format!("unreadable answer: {error}")))?;
        if (200..300).contains(&status) {
            Ok(body)
        } else {
            let message = body
                .get("error")
                .and_then(|m| m.as_str())
                .unwrap_or("the server refused")
                .to_string();
            Err(RemoteError::Rejected { status, message })
        }
    }

    /// The whole board.
    pub fn board(&self) -> Result<BoardState, RemoteError> {
        let value = self.get("/api/board")?;
        serde_json::from_value(value).map_err(|error| RemoteError::Unreachable(format!("unreadable board: {error}")))
    }

    /// Carry out one command on the server. `key`, when not empty, names this
    /// command the same on every retry, so a server that already applied it
    /// — the reply was lost, or came after the timeout — answers with its
    /// first reply instead of applying it again (`phone::Replies`).
    pub fn send(&self, command: &Command, key: &str) -> Result<WireReply, RemoteError> {
        let body = serde_json::to_vec(command)
            .map_err(|error| RemoteError::Unreachable(format!("unwritable command: {error}")))?;
        let mut request = self
            .client
            .post(format!("{}/api/command", self.base))
            .header("X-TaskDeck-Token", &self.token)
            .header("Content-Type", "application/json");
        if !key.is_empty() {
            request = request.header(crate::phone::REQUEST_HEADER, key);
        }
        let response = request
            .body(body)
            .send()
            .map_err(|error| RemoteError::Unreachable(error.to_string()))?;
        let value = Self::read(response)?;
        serde_json::from_value(value).map_err(|error| RemoteError::Unreachable(format!("unreadable reply: {error}")))
    }

    /// Block until the server's version moves past `seen`, or its timeout.
    pub fn wait(&self, seen: u64) -> Result<u64, RemoteError> {
        let value = self.get(&format!("/api/wait?version={seen}"))?;
        value
            .get("version")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| RemoteError::Unreachable("unreadable wait".to_string()))
    }
}

/* ─────────────────────────── Whose cache is this? ─────────────────────────── */

/// The file that says a `taskdeck_data/` folder is a replica of a server's
/// board, and which server. Its content is the server URL.
pub const CLIENT_MARKER: &str = ".client-of";

/// Whether `data_dir` already holds a replica of `server_url`'s board.
pub fn is_replica_of(data_dir: &Path, server_url: &str) -> bool {
    fs::read_to_string(data_dir.join(CLIENT_MARKER))
        .map(|text| text.trim() == server_url.trim().trim_end_matches('/'))
        .unwrap_or(false)
}

/// Write the marker: from now on this folder is `server_url`'s replica.
pub fn mark_replica_of(data_dir: &Path, server_url: &str) -> Result<(), String> {
    fs::write(data_dir.join(CLIENT_MARKER), server_url.trim().trim_end_matches('/')).map_err(|e| e.to_string())
}

/// Before a server's board is written over a folder that was a board of its
/// own, move that board aside — dated, never deleted — and say what moved.
///
/// This is the moment a calendar gets lost otherwise: `server_url` set on a
/// desktop whose own `taskdeck_data/` was never copied to the server. The
/// files come back to life by copying them to the server (see `SERVER.md`)
/// or by clearing `server_url` and renaming them back. Returns a message
/// for the error window when anything was set aside.
pub fn set_aside_local_board(data_dir: &Path) -> Result<Option<String>, String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (name, keep_if) in [
        ("read_at_startup.json", "[]"),
        ("archived.jsonl", ""),
        ("notepad_text.json", "\"\""),
        // Edits queued for whichever server this folder answered to before
        // must not be replayed against the one it answers to now.
        (OUTBOX_FILE, "[]"),
    ] {
        let path = data_dir.join(name);
        let Ok(text) = fs::read_to_string(&path) else { continue };
        // An empty board, log or notepad is nothing to keep.
        if text.trim().is_empty() || text.trim() == keep_if {
            continue;
        }
        let aside = data_dir.join(format!("{name}.local-{stamp}"));
        if let Err(error) = fs::rename(&path, &aside) {
            // Half a set-aside would leave the folder looking like an empty
            // board: what did move is moved back, so the caller finds the
            // board exactly as it was, and is told what could not move.
            let mut message = format!("could not set aside {name}: {error}");
            for (from, to) in moved.iter().rev() {
                if let Err(undo) = fs::rename(to, from) {
                    message.push_str(&format!(
                        "\n{} had already been set aside as {} and could not be moved back ({undo}) — rename it back by hand.",
                        from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                    ));
                }
            }
            return Err(message);
        }
        moved.push((path, aside));
    }
    if moved.is_empty() {
        return Ok(None);
    }
    let moved: Vec<String> = moved
        .into_iter()
        .map(|(_, aside)| aside.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
        .collect();
    Ok(Some(format!(
        "The board that lived on this computer was set aside, because the board now lives on the server:\n{}\nIf it should have gone to the server: copy those files into the server's taskdeck_data/ and rename each back to the name before `.local-`. The server opens only the plain names, so copying them as they are restores nothing. Do it while the server's board is still empty; if it already holds a board, the two have to be merged by hand rather than one written over the other. To use it here instead, clear server_url in Settings → Server and rename them back. Nothing was deleted.",
        moved.join("\n")
    )))
}

/// This folder is its own board again: `server_url` is empty, so the marker
/// that made it a replica goes, and an outbox of edits meant for that server
/// is set aside — never replayed later against whatever server comes next,
/// and never lost. Returns a note when there was anything to say.
pub fn forget_replica(data_dir: &Path) -> Result<Option<String>, String> {
    let marker = data_dir.join(CLIENT_MARKER);
    let Ok(server) = fs::read_to_string(&marker) else { return Ok(None) };
    fs::remove_file(&marker).map_err(|e| format!("could not remove {CLIENT_MARKER}: {e}"))?;
    let outbox = data_dir.join(OUTBOX_FILE);
    let mut note = format!("This copy runs on its own board again; it was a replica of {}.", server.trim());
    // What the outbox holds decides its fate: nothing — removed; edits, or
    // anything that cannot be read as edits — set aside, never removed.
    let waiting = match fs::read_to_string(&outbox) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Some(note)),
        Err(_) => None,
        Ok(text) => serde_json::from_str::<Vec<Queued>>(&text).ok().map(|queue| queue.len()),
    };
    match waiting {
        Some(0) => {
            let _ = fs::remove_file(&outbox);
        }
        _ => {
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let aside = data_dir.join(format!("{OUTBOX_FILE}.local-{stamp}"));
            fs::rename(&outbox, &aside).map_err(|e| format!("could not set aside {OUTBOX_FILE}: {e}"))?;
            let aside_name = aside.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match waiting {
                Some(waiting) => note.push_str(&format!(
                    "\n{waiting} change{} meant for that server had not been sent; they were set aside as {aside_name} and are applied nowhere.",
                    if waiting == 1 { "" } else { "s" }
                )),
                None => note.push_str(&format!(
                    "\nIts outbox could not be read as a list of changes and was set aside as {aside_name}; nothing was removed."
                )),
            }
        }
    }
    Ok(Some(note))
}

/* ─────────────────────────────── The outbox ─────────────────────────────── */

/// One command waiting to be sent, and — if it created something — the
/// temporary id the local board gave that thing, so the real one can be
/// mapped onto it when the server answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Queued {
    pub command: Command,
    #[serde(default)]
    pub local_id: Option<u64>,
    /// Names this command the same on every retry (`Remote::send`). Empty in
    /// an outbox written before keys existed; such a command is sent without.
    #[serde(default)]
    pub key: String,
}

/// Commands waiting for the server, in order, on disk.
pub struct Outbox {
    path: PathBuf,
    queue: VecDeque<Queued>,
}

impl Outbox {
    /// Read the outbox from `data_dir`, or start empty. An unreadable file is
    /// set aside rather than deleted, and the reason returned.
    pub fn open(data_dir: &Path) -> (Self, Option<String>) {
        let path = data_dir.join(OUTBOX_FILE);
        let mut problem = None;
        let queue = match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Vec<Queued>>(&text) {
                Ok(items) => items.into(),
                Err(error) => {
                    // Stamped, as every other set-aside is: a second corrupt
                    // outbox in a later run must not overwrite the first.
                    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                    let aside = data_dir.join(format!("{OUTBOX_FILE}.corrupt-{stamp}"));
                    let _ = fs::rename(&path, &aside);
                    problem = Some(format!(
                        "The outbox of unsent changes could not be read and was set aside as {}:\n{error}",
                        aside.display()
                    ));
                    VecDeque::new()
                }
            },
            Err(_) => VecDeque::new(),
        };
        (Self { path, queue }, problem)
    }

    /// An outbox that is never written — for tests and for a desktop that is
    /// not a client.
    pub fn in_memory() -> Self {
        Self { path: PathBuf::new(), queue: VecDeque::new() }
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn front(&self) -> Option<&Queued> {
        self.queue.front()
    }

    /// The highest temporary id anything still queued names — as the id it
    /// created, or the id it addresses. A board that hands out temporary ids
    /// starts past this, so an id from the last run is never given out again
    /// while a remap for it can still arrive (§22.4).
    pub fn highest_temporary_id(&self) -> Option<u64> {
        self.queue
            .iter()
            .flat_map(|queued| [queued.local_id, queued.command.item_id()])
            .flatten()
            .filter(|id| *id >= CLIENT_ID_FLOOR)
            .max()
    }

    pub fn push(&mut self, queued: Queued) -> Result<(), String> {
        self.queue.push_back(queued);
        self.persist()
    }

    pub fn pop_front(&mut self) -> Result<Option<Queued>, String> {
        let front = self.queue.pop_front();
        self.persist()?;
        Ok(front)
    }

    /// A creation was acknowledged: everything still queued that named the
    /// temporary id now names the real one.
    pub fn remap(&mut self, local: u64, server: u64) -> Result<(), String> {
        for queued in &mut self.queue {
            if queued.command.item_id() == Some(local) {
                queued.command.set_item_id(server);
            }
        }
        self.persist()
    }

    /// Atomic, like every other file this program writes: a crash between
    /// two edits must not leave half an outbox.
    fn persist(&self) -> Result<(), String> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        let dir = self.path.parent().ok_or("outbox has no directory")?;
        let body = serde_json::to_string_pretty(&self.queue.iter().collect::<Vec<_>>()).map_err(|e| e.to_string())?;
        let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
        temp.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
        temp.as_file_mut().sync_all().map_err(|e| e.to_string())?;
        temp.persist(&self.path).map_err(|e| e.to_string())?;
        // The rename is atomic, but its directory entry can still be in the
        // page cache when the power goes (§4.1) — and a power cut is exactly
        // when the outbox has to be there at the next start.
        crate::tasks::sync_directory(dir);
        Ok(())
    }
}

/* ─────────────────────────────── The engine ─────────────────────────────── */

/// What the menu bar shows: are we in touch, and how much is waiting.
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub online: bool,
    pub pending: usize,
    /// The last thing that went wrong, for the hover text.
    pub last_error: Option<String>,
    /// The outbox could not be written, and has not been since. Shown
    /// whether or not the server is in touch: it is about this disk.
    pub storage_error: Option<String>,
    /// The server's version after the sender's last successful send. The
    /// listener never delivers a board older than this.
    pub acked_version: u64,
}

/// What the engine tells the UI.
#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// The server's board. Replace the replica with it.
    Board(BoardState),
    /// Something created here under a temporary id is now `server` on the
    /// server — and in the next board delivered.
    Remapped { local: u64, server: u64 },
    /// A queued command was refused and dropped.
    Rejected { what: String, message: String },
    /// The desktop is back in touch / has lost touch.
    Online(bool),
}

/// The UI's end of the engine.
pub struct SyncHandle {
    tx: Mutex<Option<Sender<Queued>>>,
    pub events: Receiver<SyncEvent>,
    status: Arc<Mutex<Status>>,
    remote: Remote,
    quit: Arc<AtomicBool>,
    sender: Mutex<Option<thread::JoinHandle<()>>>,
}

impl SyncHandle {
    /// Queue a command the local board has already applied. `local_id` is
    /// the temporary id it created, if it created something. Counted as
    /// pending from this moment — not from when the sender files it — so the
    /// listener never delivers a board from the gap in between.
    pub fn queue(&self, command: Command, local_id: Option<u64>) {
        let key = crate::phone::generate_token();
        if let Ok(mut s) = self.status.lock() {
            s.pending += 1;
        }
        if let Ok(tx) = self.tx.lock()
            && let Some(tx) = tx.as_ref()
        {
            let _ = tx.send(Queued { command, local_id, key });
        }
    }

    /// Stop sending and file whatever is still in hand, then return. What
    /// the desktop calls on its way out — a quit, a restart — so an edit made
    /// in the last second is in `outbox.json` before the process is gone.
    /// Waits for at most one send in flight.
    pub fn shutdown(&self) {
        self.quit.store(true, Ordering::Relaxed);
        if let Ok(mut tx) = self.tx.lock() {
            tx.take();
        }
        if let Some(thread) = self.sender.lock().ok().and_then(|mut slot| slot.take()) {
            let _ = thread.join();
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn server(&self) -> &str {
        self.remote.base()
    }
}

/// A short name for a command in a "could not be applied" message.
pub fn describe(command: &Command) -> String {
    match command {
        Command::Add { name, .. } | Command::QuickAdd { name, .. } => format!("add \"{name}\""),
        Command::Create { name, .. } => format!("create \"{name}\""),
        Command::Rename { name, .. } => format!("rename to \"{name}\""),
        Command::Complete { id, .. } => format!("complete #{id}"),
        Command::Delete { id, .. } => format!("delete #{id}"),
        Command::Forget { id } => format!("discard #{id}"),
        Command::MoveBlock { id, .. } => format!("move a block of #{id}"),
        Command::AddBlock { id, .. } => format!("book a block for #{id}"),
        Command::RemoveBlock { id, .. } => format!("remove a block of #{id}"),
        Command::Unplan { id } => format!("unplan #{id}"),
        Command::SetEstimate { id, .. } => format!("change the length of #{id}"),
        Command::SetDeadline { id, .. } => format!("change the deadline of #{id}"),
        Command::SetSeverity { id, .. } => format!("change the severity of #{id}"),
        Command::SetHorizon { id, .. } => format!("change the horizon of #{id}"),
        Command::SetRepeat { id, .. } => format!("change the weekdays of #{id}"),
        Command::Reflow { day, .. } => format!("reflow {day}"),
        Command::SetNotes { .. } => "save the notes".to_string(),
        Command::Restore { id, .. } => format!("put back #{id}"),
        Command::ForgetArchived { id, .. } => format!("forget archived #{id}"),
        Command::AddSubscription { name, .. } => format!("subscribe to {name}"),
        Command::RemoveSubscription { id } => format!("stop subscribing to #{id}"),
        Command::RenameSubscription { id, .. } => format!("rename calendar #{id}"),
        Command::SetSubscriptionColor { id, .. } => format!("recolour calendar #{id}"),
        Command::SetSubscriptionEnabled { id, enabled } => {
            format!("{} calendar #{id}", if *enabled { "switch on" } else { "switch off" })
        }
        Command::Snapshot { .. } | Command::Feed | Command::Board => "a query".to_string(),
    }
}

/// Start the engine: the sender and the listener. `seen` is the version of
/// the board the UI starts with (0 when it started from the local cache, so
/// the first successful wait fetches the server's board at once).
pub fn start(remote: Remote, outbox: Outbox, wake: Wake, seen: u64, online: bool) -> SyncHandle {
    let (tx, rx) = channel::<Queued>();
    let (event_tx, event_rx) = channel::<SyncEvent>();
    let status = Arc::new(Mutex::new(Status {
        online,
        pending: outbox.len(),
        last_error: None,
        storage_error: None,
        acked_version: seen,
    }));
    let quit = Arc::new(AtomicBool::new(false));

    let sender_thread = {
        let remote = remote.clone();
        let status = Arc::clone(&status);
        let events = event_tx.clone();
        let wake = Arc::clone(&wake);
        let quit = Arc::clone(&quit);
        thread::Builder::new()
            .name("taskdeck-sync-send".to_string())
            .spawn(move || sender(remote, outbox, rx, status, events, wake, quit))
            .expect("could not start the sync sender")
    };
    {
        let remote = remote.clone();
        let status = Arc::clone(&status);
        thread::Builder::new()
            .name("taskdeck-sync-listen".to_string())
            .spawn(move || listener(remote, status, event_tx, wake, seen))
            .expect("could not start the sync listener");
    }

    SyncHandle {
        tx: Mutex::new(Some(tx)),
        events: event_rx,
        status,
        remote,
        quit,
        sender: Mutex::new(Some(sender_thread)),
    }
}

fn set_online(status: &Mutex<Status>, events: &Sender<SyncEvent>, wake: &Wake, online: bool, error: Option<String>) {
    let changed = {
        let mut s = status.lock().unwrap_or_else(|p| p.into_inner());
        let changed = s.online != online;
        s.online = online;
        if let Some(error) = error {
            s.last_error = Some(error);
        }
        changed
    };
    if changed {
        let _ = events.send(SyncEvent::Online(online));
        wake();
    }
}


/// The sender: owns the outbox, flushes it in order.
fn sender(
    remote: Remote,
    mut outbox: Outbox,
    rx: Receiver<Queued>,
    status: Arc<Mutex<Status>>,
    events: Sender<SyncEvent>,
    wake: Wake,
    quit: Arc<AtomicBool>,
) {
    let remote = match Remote::new(remote.base(), &remote.token, SEND_TIMEOUT) {
        Ok(remote) => remote,
        Err(_) => remote,
    };
    // Every temporary id the server has answered for, for the rest of the
    // run: a command the UI queued between the server's answer and its own
    // next frame still names the temporary one, and is re-pointed on arrival.
    let mut known: HashMap<u64, u64> = HashMap::new();
    loop {
        // Take everything the UI has queued since the last pass, then try to
        // send. Waiting with a timeout is what makes a retry happen while
        // offline even when nobody is editing.
        match rx.recv_timeout(RETRY_EVERY) {
            Ok(queued) => {
                file(&mut outbox, queued, &known, &status);
                while let Ok(more) = rx.try_recv() {
                    file(&mut outbox, more, &known, &status);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        // Quitting: what is filed is safe on disk; nothing more is sent.
        if quit.load(Ordering::Relaxed) {
            break;
        }

        while let Some(queued) = outbox.front().cloned() {
            if quit.load(Ordering::Relaxed) {
                break;
            }
            match remote.send(&queued.command, &queued.key) {
                Ok(reply) => {
                    if let Ok(mut s) = status.lock() {
                        // Not `max`: a server that restarted answers with a
                        // smaller number, and the listener must follow it.
                        s.acked_version = reply.version;
                    }
                    if let (Some(local), Some(server)) = (queued.local_id, reply.id)
                        && local != server
                    {
                        known.insert(local, server);
                        note_storage(&status, outbox.remap(local, server));
                        let _ = events.send(SyncEvent::Remapped { local, server });
                        wake();
                    }
                    note_storage(&status, outbox.pop_front().map(|_| ()));
                    dec_pending(&status);
                    set_online(&status, &events, &wake, true, None);
                }
                Err(RemoteError::Rejected { status: code @ (401 | 403), message }) => {
                    // Not a verdict on the edit but on the key: nothing is
                    // dropped, the desktop is out of touch until the setting
                    // is fixed, and the menu bar says why.
                    set_online(&status, &events, &wake, false, Some(format!("{message} ({code}) — check the server key in Settings")));
                    break;
                }
                Err(RemoteError::Rejected { status: code @ 500..=599, message }) => {
                    // The server's own trouble — shutting down, its board not
                    // answering in time, its disk refusing a save — is not a
                    // verdict on the edit either. Kept, and tried again as an
                    // unreachable server would be. (The phone page treats
                    // these the same way.)
                    set_online(&status, &events, &wake, false, Some(format!("{message} ({code})")));
                    break;
                }
                Err(RemoteError::Rejected { status: code, message }) => {
                    // The server said no: dropped and reported, never retried.
                    note_storage(&status, outbox.pop_front().map(|_| ()));
                    dec_pending(&status);
                    let _ = events.send(SyncEvent::Rejected {
                        what: describe(&queued.command),
                        message: format!("{message} ({code})"),
                    });
                    wake();
                    set_online(&status, &events, &wake, true, None);
                }
                Err(RemoteError::Unreachable(why)) => {
                    set_online(&status, &events, &wake, false, Some(why));
                    break;
                }
            }
        }
    }
    // Whatever arrived while the last sends ran is filed before the thread
    // goes: a quit must lose nothing the UI handed over.
    while let Ok(more) = rx.try_recv() {
        file(&mut outbox, more, &known, &status);
    }
}

/// File one command in the outbox — re-pointed if it names a temporary id the
/// server has already answered for — and remember whether the disk took it.
fn file(outbox: &mut Outbox, mut queued: Queued, known: &HashMap<u64, u64>, status: &Mutex<Status>) {
    if let Some(server) = queued.command.item_id().and_then(|id| known.get(&id)) {
        queued.command.set_item_id(*server);
    }
    note_storage(status, outbox.push(queued));
}

/// The outbox's last word on the disk. An error stays in `storage_error`
/// until a write succeeds, however many network errors come and go meanwhile:
/// an outbox that is not being written is an outbox that is empty at the next
/// start, and that has to be said while there is still time to do something.
fn note_storage(status: &Mutex<Status>, result: Result<(), String>) {
    if let Ok(mut s) = status.lock() {
        s.storage_error = result.err().map(|error| format!("The outbox could not be saved: {error}"));
    }
}

fn dec_pending(status: &Mutex<Status>) {
    if let Ok(mut s) = status.lock() {
        s.pending = s.pending.saturating_sub(1);
    }
}

/// The listener: waits for the server to change, then fetches its board.
fn listener(remote: Remote, status: Arc<Mutex<Status>>, events: Sender<SyncEvent>, wake: Wake, mut seen: u64) {
    let remote = match Remote::new(remote.base(), &remote.token, WAIT_TIMEOUT) {
        Ok(remote) => remote,
        Err(_) => remote,
    };
    let mut backoff = Duration::from_secs(2);
    loop {
        // Nothing is delivered while edits are waiting to go out: the board
        // the server would hand back does not have them yet.
        let pending = status.lock().map(|s| s.pending).unwrap_or(0);
        if pending > 0 {
            thread::sleep(Duration::from_millis(400));
            continue;
        }

        match remote.wait(seen) {
            Ok(version) => {
                set_online(&status, &events, &wake, true, None);
                if version < seen {
                    // The server's count went backwards: it restarted (a
                    // server seeds its count from the clock, so this takes a
                    // clock set back) or was replaced. Everything it says from
                    // here on is newer than anything seen; start over.
                    seen = 0;
                    if let Ok(mut s) = status.lock() {
                        s.acked_version = 0;
                    }
                }
                if version == seen {
                    backoff = Duration::from_secs(2);
                    continue;
                }
                match remote.board() {
                    Ok(state) => {
                        backoff = Duration::from_secs(2);
                        let (pending, acked) = status.lock().map(|s| (s.pending, s.acked_version)).unwrap_or((0, 0));
                        // A fetch that raced an edit — older than what the
                        // sender has since been told — is thrown away; the
                        // next wait returns at once with the newer version.
                        if pending == 0 && state.version >= acked {
                            seen = state.version;
                            let _ = events.send(SyncEvent::Board(state));
                            wake();
                        }
                    }
                    // The wait answered but the board did not — or it came in
                    // a shape this build cannot read, which is what two
                    // computers on different versions look like. A pause, and
                    // a longer one each time: without it the next wait returns
                    // at once, the version having still moved, and the two
                    // requests loop at network speed.
                    Err(RemoteError::Unreachable(why)) | Err(RemoteError::Rejected { message: why, .. }) => {
                        set_online(&status, &events, &wake, false, Some(why));
                        thread::sleep(backoff);
                        backoff = (backoff * 2).min(LISTEN_BACKOFF_MAX);
                    }
                }
            }
            Err(RemoteError::Unreachable(why)) => {
                set_online(&status, &events, &wake, false, Some(why));
                thread::sleep(backoff);
                backoff = (backoff * 2).min(LISTEN_BACKOFF_MAX);
            }
            Err(RemoteError::Rejected { status: code @ 500..=599, message }) => {
                // The server's own trouble — shutting down, not answering in
                // time, a disk refusing it: back off as for a server that is
                // not there.
                set_online(&status, &events, &wake, false, Some(format!("{message} ({code})")));
                thread::sleep(backoff);
                backoff = (backoff * 2).min(LISTEN_BACKOFF_MAX);
            }
            Err(RemoteError::Rejected { status: code, message }) => {
                // A refused wait is a wrong key, not a flaky network: say so
                // — in the same words the sender uses, since either thread may
                // be the one that noticed — and do not hammer the server.
                let detail = if matches!(code, 401 | 403) {
                    format!("{message} ({code}) — check the server key in Settings")
                } else {
                    format!("{message} ({code})")
                };
                set_online(&status, &events, &wake, false, Some(detail));
                thread::sleep(LISTEN_BACKOFF_MAX);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 4).unwrap()
    }

    #[test]
    fn the_outbox_keeps_order_remaps_ids_and_survives_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let (mut outbox, problem) = Outbox::open(dir.path());
        assert!(problem.is_none());
        assert!(outbox.is_empty());

        let temp = CLIENT_ID_FLOOR + 3;
        outbox.push(Queued { command: Command::QuickAdd { name: "offline".into(), deadline: None }, local_id: Some(temp), key: String::new() }).unwrap();
        outbox.push(Queued { command: Command::Rename { id: temp, name: "renamed".into() }, local_id: None, key: String::new() }).unwrap();
        outbox.push(Queued { command: Command::AddBlock { id: temp, day: day(), start: 600, minutes: None }, local_id: None, key: String::new() }).unwrap();
        outbox.push(Queued { command: Command::Rename { id: 7, name: "someone else".into() }, local_id: None, key: String::new() }).unwrap();
        assert_eq!(outbox.len(), 4);

        // The server acknowledged the creation as id 42: the queued commands
        // that named the temporary id follow it; the unrelated one does not.
        outbox.pop_front().unwrap();
        outbox.remap(temp, 42).unwrap();
        let ids: Vec<Option<u64>> = outbox.queue.iter().map(|q| q.command.item_id()).collect();
        assert_eq!(ids, vec![Some(42), Some(42), Some(7)]);

        // What is on disk is what is in memory.
        let (again, problem) = Outbox::open(dir.path());
        assert!(problem.is_none());
        assert_eq!(again.len(), 3);
        assert_eq!(again.front().unwrap().command, Command::Rename { id: 42, name: "renamed".into() });

        // The highest temporary id anything queued names, for the board's
        // counter to start past.
        let (mut queued, _) = Outbox::open(dir.path());
        assert_eq!(queued.highest_temporary_id(), None, "nothing temporary left after the remap");
        queued.push(Queued { command: Command::Unplan { id: temp + 5 }, local_id: None, key: String::new() }).unwrap();
        queued.push(Queued { command: Command::QuickAdd { name: "n".into(), deadline: None }, local_id: Some(temp + 2), key: String::new() }).unwrap();
        assert_eq!(queued.highest_temporary_id(), Some(temp + 5));

        // An outbox written before keys existed loads, keyless.
        fs::write(dir.path().join(OUTBOX_FILE), r#"[{"command":{"op":"unplan","id":9}}]"#).unwrap();
        let (old, problem) = Outbox::open(dir.path());
        assert!(problem.is_none());
        assert_eq!(old.front().map(|q| q.key.as_str()), Some(""));

        // A corrupt file is set aside, not silently emptied.
        fs::write(dir.path().join(OUTBOX_FILE), "{not json").unwrap();
        let (empty, problem) = Outbox::open(dir.path());
        assert!(empty.is_empty());
        assert!(problem.unwrap().contains("set aside"));
        let set_aside = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().starts_with(&format!("{OUTBOX_FILE}.corrupt-")));
        assert!(set_aside, "the corrupt outbox was not set aside under a stamped name");
    }

    #[test]
    fn a_local_board_is_set_aside_once_and_the_marker_remembers_the_server() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("read_at_startup.json"), r#"[{"id":1,"name":"mine"}]"#).unwrap();
        fs::write(dir.path().join("archived.jsonl"), "").unwrap();
        fs::write(dir.path().join("notepad_text.json"), "\"remember\"").unwrap();
        assert!(!is_replica_of(dir.path(), "http://srv:7373"));

        let note = set_aside_local_board(dir.path()).unwrap().expect("something to set aside");
        assert!(note.contains("read_at_startup.json.local-"));
        assert!(note.contains("notepad_text.json.local-"));
        assert!(!note.contains("archived.jsonl"), "an empty log is nothing to keep");
        assert!(!dir.path().join("read_at_startup.json").exists());
        let kept: Vec<_> = fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(kept.iter().any(|n| n.starts_with("read_at_startup.json.local-")));

        mark_replica_of(dir.path(), "http://srv:7373/").unwrap();
        assert!(is_replica_of(dir.path(), "http://srv:7373"));
        assert!(!is_replica_of(dir.path(), "http://other:7373"));
        // Nothing left to move: a second call is quiet.
        assert_eq!(set_aside_local_board(dir.path()).unwrap(), None);
    }

    #[test]
    fn a_remote_refuses_an_address_without_a_scheme_and_trims_a_slash() {
        assert!(Remote::new("100.64.0.1:7373", "k", Duration::from_secs(1)).is_err());
        let remote = Remote::new(" http://100.64.0.1:7373/ ", "k", Duration::from_secs(1)).unwrap();
        assert_eq!(remote.base(), "http://100.64.0.1:7373");
    }

    #[test]
    fn descriptions_name_the_thing() {
        assert_eq!(describe(&Command::Complete { id: 3, at: None }), "complete #3");
        assert_eq!(describe(&Command::QuickAdd { name: "milk".into(), deadline: None }), "add \"milk\"");
        assert_eq!(describe(&Command::SetNotes { text: "x".into() }), "save the notes");
    }

    /// A stand-in server: records every command it is sent, answers creates
    /// with id 100, and answers everything else as the real one would —
    /// or, in the other moods, refuses the key (401) or is busy (503) for
    /// all of it.
    struct MockServer {
        port: u16,
        received: Arc<Mutex<Vec<Command>>>,
        /// The `X-TaskDeck-Request` key of every command request, in order.
        keys: Arc<Mutex<Vec<String>>>,
        /// How many times `/api/board` was asked for.
        boards: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Mood {
        Normal,
        RefuseKey,
        Busy,
        Broken,
        /// The board is slow: the first two requests under a key are answered
        /// `503 still working`, the third as normal.
        BusyTwice,
        /// The board comes back in a shape this build cannot read.
        Gibberish,
    }

    fn mock_server(mood: Mood) -> MockServer {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let received = Arc::new(Mutex::new(Vec::<Command>::new()));
        let seen = Arc::clone(&received);
        let keys = Arc::new(Mutex::new(Vec::<String>::new()));
        let keys_seen = Arc::clone(&keys);
        let boards = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let boards_seen = Arc::clone(&boards);
        thread::spawn(move || {
            let version = Arc::new(Mutex::new(1u64));
            let mut attempts: HashMap<String, u32> = HashMap::new();
            for mut request in server.incoming_requests() {
                let url = request.url().to_string();
                let key = request
                    .headers()
                    .iter()
                    .find(|header| header.field.equiv(crate::phone::REQUEST_HEADER))
                    .map(|header| header.value.as_str().to_string())
                    .unwrap_or_default();
                if url.starts_with("/api/command") {
                    keys_seen.lock().unwrap().push(key.clone());
                }
                let json = |body: String, status: u16| {
                    tiny_http::Response::from_string(body)
                        .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
                        .with_status_code(status)
                };
                match mood {
                    Mood::RefuseKey => {
                        let _ = request.respond(json(r#"{"error":"Not authorised"}"#.to_string(), 401));
                        continue;
                    }
                    Mood::Busy => {
                        let _ = request.respond(json(r#"{"error":"TaskDeck is shutting down."}"#.to_string(), 503));
                        continue;
                    }
                    Mood::Broken => {
                        let _ = request.respond(json(r#"{"error":"Saving error: disk full"}"#.to_string(), 500));
                        continue;
                    }
                    Mood::BusyTwice if url.starts_with("/api/command") => {
                        let count = attempts.entry(key.clone()).or_insert(0);
                        *count += 1;
                        if *count <= 2 {
                            let _ = request.respond(json(r#"{"error":"TaskDeck is still working on that change; it is not lost."}"#.to_string(), 503));
                            continue;
                        }
                    }
                    Mood::Gibberish if url.starts_with("/api/board") => {
                        boards_seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let _ = request.respond(json(r#"{"nonsense":true}"#.to_string(), 200));
                        continue;
                    }
                    Mood::Normal | Mood::BusyTwice | Mood::Gibberish => {}
                }
                if url.starts_with("/api/command") {
                    let mut body = String::new();
                    std::io::Read::read_to_string(request.as_reader(), &mut body).unwrap();
                    let command: Command = serde_json::from_str(&body).unwrap();
                    let mut v = version.lock().unwrap();
                    *v += 1;
                    let reply = if command.creates_item() {
                        format!(r#"{{"ok":true,"version":{},"id":100}}"#, *v)
                    } else {
                        format!(r#"{{"ok":true,"version":{}}}"#, *v)
                    };
                    seen.lock().unwrap().push(command);
                    let _ = request.respond(json(reply, 200));
                } else if url.starts_with("/api/wait") {
                    // Never hurry: a real server holds this for 25 s.
                    thread::sleep(Duration::from_millis(300));
                    let v = *version.lock().unwrap();
                    let _ = request.respond(json(format!(r#"{{"version":{v}}}"#), 200));
                } else if url.starts_with("/api/board") {
                    boards_seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let v = *version.lock().unwrap();
                    let _ = request.respond(json(
                        format!(r#"{{"version":{v},"items":[],"archive":[],"notes":"from the mock"}}"#),
                        200,
                    ));
                } else {
                    let _ = request.respond(json(r#"{"error":"No such page."}"#.to_string(), 404));
                }
            }
        });
        MockServer { port, received, keys, boards }
    }

    fn wait_until(what: &str, condition: impl FnMut() -> bool) {
        wait_for(what, Duration::from_secs(5), condition);
    }

    fn wait_for(what: &str, limit: Duration, mut condition: impl FnMut() -> bool) {
        let started = std::time::Instant::now();
        while !condition() {
            assert!(started.elapsed() < limit, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn the_sender_flushes_in_order_and_remaps_a_created_id() {
        let mock = mock_server(Mood::Normal);
        let remote = Remote::new(&format!("http://127.0.0.1:{}", mock.port), "k", Duration::from_secs(2)).unwrap();
        let wake: Wake = Arc::new(|| {});
        let handle = start(remote, Outbox::in_memory(), wake, 1, true);

        let temp = CLIENT_ID_FLOOR + 1;
        handle.queue(Command::QuickAdd { name: "offline".into(), deadline: None }, Some(temp));
        handle.queue(Command::Rename { id: temp, name: "renamed".into() }, None);
        handle.queue(Command::Rename { id: 7, name: "other".into() }, None);

        wait_until("three commands to arrive", || mock.received.lock().unwrap().len() == 3);
        let received = mock.received.lock().unwrap().clone();
        // In order, and the rename that named the temporary id now names 100.
        assert!(matches!(received[0], Command::QuickAdd { .. }));
        assert_eq!(received[1], Command::Rename { id: 100, name: "renamed".into() });
        assert_eq!(received[2], Command::Rename { id: 7, name: "other".into() });

        // The UI was told about the id, and about the board the listener fetched.
        let mut remapped = None;
        let mut boards = 0;
        wait_until("the remap and a board", || {
            while let Ok(event) = handle.events.try_recv() {
                match event {
                    SyncEvent::Remapped { local, server } => remapped = Some((local, server)),
                    SyncEvent::Board(state) => {
                        assert_eq!(state.notes, "from the mock");
                        boards += 1;
                    }
                    _ => {}
                }
            }
            remapped.is_some() && boards > 0
        });
        assert_eq!(remapped, Some((temp, 100)));
        let status = handle.status();
        assert!(status.online);
        assert_eq!(status.pending, 0);
        assert!(status.acked_version >= 4);

        // A command queued after the answer but still naming the temporary
        // id — the UI has not drained `Remapped` yet — is re-pointed on arrival.
        handle.queue(Command::Rename { id: temp, name: "late".into() }, None);
        wait_until("the late rename", || mock.received.lock().unwrap().len() == 4);
        assert_eq!(mock.received.lock().unwrap()[3], Command::Rename { id: 100, name: "late".into() });
    }

    #[test]
    fn a_failing_server_keeps_the_edits_as_well() {
        // A 500 is the server's disk or code, not a verdict on the edit.
        let mock = mock_server(Mood::Broken);
        let remote = Remote::new(&format!("http://127.0.0.1:{}", mock.port), "k", Duration::from_secs(2)).unwrap();
        let wake: Wake = Arc::new(|| {});
        let handle = start(remote, Outbox::in_memory(), wake, 0, true);
        handle.queue(Command::Unplan { id: 7 }, None);
        wait_until("the sender to notice", || {
            let status = handle.status();
            !status.online && status.pending == 1
        });
        assert!(handle.status().last_error.as_deref().unwrap_or("").contains("(500)"));
        let mut refused = false;
        while let Ok(event) = handle.events.try_recv() {
            if matches!(event, SyncEvent::Rejected { .. }) {
                refused = true;
            }
        }
        assert!(!refused, "nothing may be dropped over a 500");
    }

    #[test]
    fn a_board_this_build_cannot_read_is_retried_with_a_pause_not_a_spin() {
        // The wait says the version moved; the board comes back in a shape
        // this build cannot read — two computers on different versions. The
        // next wait returns at once, because the version has still moved, so
        // without a pause between the two the listener hammers the server at
        // network speed and floods the window with offline events.
        let mock = mock_server(Mood::Gibberish);
        let remote = Remote::new(&format!("http://127.0.0.1:{}", mock.port), "k", Duration::from_secs(2)).unwrap();
        let wake: Wake = Arc::new(|| {});
        let handle = start(remote, Outbox::in_memory(), wake, 0, true);
        wait_until("the listener to fetch the board once", || {
            mock.boards.load(std::sync::atomic::Ordering::Relaxed) >= 1
        });
        thread::sleep(Duration::from_millis(1500));
        let fetches = mock.boards.load(std::sync::atomic::Ordering::Relaxed);
        assert!(fetches <= 2, "{fetches} board fetches in 1.5 s is a spin, not a retry");
        assert!(!handle.status().online);
        assert!(handle.status().last_error.as_deref().unwrap_or("").contains("unreadable board"));
    }

    #[test]
    fn a_busy_answer_is_retried_under_the_same_key_and_applied_once() {
        // The server's board is slow: the first attempts hear "still working"
        // (503). The sender keeps the command, retries on its timer — not in
        // a spin — and sends the same key every time, so when the board is
        // done the reply cache answers and nothing is applied twice.
        let mock = mock_server(Mood::BusyTwice);
        let remote = Remote::new(&format!("http://127.0.0.1:{}", mock.port), "k", Duration::from_secs(2)).unwrap();
        let wake: Wake = Arc::new(|| {});
        let handle = start(remote, Outbox::in_memory(), wake, 0, true);
        let started = std::time::Instant::now();
        handle.queue(Command::Unplan { id: 7 }, None);

        wait_for("the command to be applied", Duration::from_secs(15), || mock.received.lock().unwrap().len() == 1);
        // Two refusals, each a full RETRY_EVERY apart, before the answer:
        // paced by the timer, not hammered.
        assert!(started.elapsed() >= RETRY_EVERY * 2, "retried too fast: {:?}", started.elapsed());
        let keys = mock.keys.lock().unwrap().clone();
        assert_eq!(keys.len(), 3, "{keys:?}");
        assert!(!keys[0].is_empty() && keys.iter().all(|key| key == &keys[0]), "one key throughout: {keys:?}");
        wait_until("the outbox to drain", || {
            let status = handle.status();
            status.pending == 0 && status.online
        });
        assert_eq!(mock.received.lock().unwrap().len(), 1, "applied once");
    }

    #[test]
    fn shutdown_files_what_was_queued_and_returns() {
        // A server that never answers: the send would block for its timeout,
        // but a shutdown files the queue and comes back without sending.
        let dir = tempfile::tempdir().unwrap();
        let (outbox, _) = Outbox::open(dir.path());
        let remote = Remote::new("http://127.0.0.1:9", "k", Duration::from_secs(1)).unwrap();
        let wake: Wake = Arc::new(|| {});
        let handle = start(remote, outbox, wake, 0, false);
        handle.queue(Command::Unplan { id: 1 }, None);
        handle.queue(Command::Unplan { id: 2 }, None);
        let started = std::time::Instant::now();
        handle.shutdown();
        assert!(started.elapsed() < Duration::from_secs(3));
        let (again, _) = Outbox::open(dir.path());
        assert_eq!(again.len(), 2, "both edits are on disk");
        assert!(!again.front().unwrap().key.is_empty(), "each carries its key");
    }

    #[test]
    fn a_folder_that_stops_being_a_replica_keeps_its_unsent_edits_aside() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(forget_replica(dir.path()).unwrap(), None, "nothing to forget");
        mark_replica_of(dir.path(), "http://srv:7373").unwrap();
        let (mut outbox, _) = Outbox::open(dir.path());
        outbox.push(Queued { command: Command::Unplan { id: 3 }, local_id: None, key: "k".into() }).unwrap();
        let note = forget_replica(dir.path()).unwrap().expect("a note");
        assert!(note.contains("1 change") && note.contains("outbox.json.local-"), "{note}");
        // An outbox that cannot be read as changes is set aside too, never removed.
        mark_replica_of(dir.path(), "http://srv:7373").unwrap();
        fs::write(dir.path().join(OUTBOX_FILE), "{not an outbox").unwrap();
        let note = forget_replica(dir.path()).unwrap().expect("a note");
        assert!(note.contains("could not be read") && note.contains("outbox.json.local-"), "{note}");
        assert!(!dir.path().join(OUTBOX_FILE).exists());
        assert!(!is_replica_of(dir.path(), "http://srv:7373"));
        assert!(!dir.path().join(OUTBOX_FILE).exists());
        let kept: Vec<_> = fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(kept.iter().any(|n| n.starts_with("outbox.json.local-")), "{kept:?}");
        // Next time the folder meets a server, the set-aside of a first
        // contact happens again, outbox included.
        fs::write(dir.path().join("read_at_startup.json"), r#"[{"id":1,"name":"mine"}]"#).unwrap();
        let (mut outbox, _) = Outbox::open(dir.path());
        outbox.push(Queued { command: Command::Unplan { id: 4 }, local_id: None, key: "k".into() }).unwrap();
        let note = set_aside_local_board(dir.path()).unwrap().expect("something to set aside");
        assert!(note.contains("outbox.json.local-"), "{note}");
        assert!(!dir.path().join(OUTBOX_FILE).exists());
    }

    #[test]
    fn a_refused_key_keeps_the_edits_and_says_so() {
        let mock = mock_server(Mood::RefuseKey);
        let remote = Remote::new(&format!("http://127.0.0.1:{}", mock.port), "wrong", Duration::from_secs(2)).unwrap();
        let wake: Wake = Arc::new(|| {});
        let handle = start(remote, Outbox::in_memory(), wake, 0, true);
        handle.queue(Command::Rename { id: 7, name: "kept".into() }, None);

        wait_until("the sender to notice", || {
            let status = handle.status();
            !status.online && status.pending == 1
        });
        let status = handle.status();
        assert!(status.last_error.as_deref().unwrap_or("").contains("check the server key"));
        // Nothing was dropped and nothing was reported as refused.
        let mut refused = false;
        while let Ok(event) = handle.events.try_recv() {
            if matches!(event, SyncEvent::Rejected { .. }) {
                refused = true;
            }
        }
        assert!(!refused);
    }

    #[test]
    fn a_busy_server_keeps_the_edits_too() {
        // 503 is the server's own "not now" — mid-shutdown, or its board not
        // answering in time. Not a verdict on the edit: kept, offline, said.
        let mock = mock_server(Mood::Busy);
        let remote = Remote::new(&format!("http://127.0.0.1:{}", mock.port), "k", Duration::from_secs(2)).unwrap();
        let wake: Wake = Arc::new(|| {});
        let handle = start(remote, Outbox::in_memory(), wake, 0, true);
        handle.queue(Command::Complete { id: 7, at: None }, None);

        wait_until("the sender to notice", || {
            let status = handle.status();
            !status.online && status.pending == 1
        });
        let status = handle.status();
        let error = status.last_error.as_deref().unwrap_or("");
        assert!(error.contains("(503)") && error.contains("shutting down"), "got {error:?}");
        assert!(!error.contains("check the server key"), "a busy server is not a wrong key");
        let mut refused = false;
        while let Ok(event) = handle.events.try_recv() {
            if matches!(event, SyncEvent::Rejected { .. }) {
                refused = true;
            }
        }
        assert!(!refused, "nothing may be dropped over a 503");
    }

    #[test]
    fn an_unreachable_server_is_unreachable_quickly() {
        // Nothing listens on this port; the error is the network's, not a refusal.
        let remote = Remote::new("http://127.0.0.1:9", "k", Duration::from_millis(800)).unwrap();
        match remote.board() {
            Err(RemoteError::Unreachable(_)) => {}
            other => panic!("expected unreachable, got {other:?}"),
        }
    }
}
