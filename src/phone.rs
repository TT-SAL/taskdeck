//! The phone view: TaskDeck serving its own calendar to a phone.
//!
//! The running desktop app hosts a small HTTP server on a background thread and
//! serves one embedded page (`phone.html`) that shows a day — timeline, tray,
//! due markers, routines — and edits it. The phone is a *thin client of the one
//! live process*: every change it makes arrives here as a [`Command`], is handed
//! to the UI thread over a channel, and is applied through the same setters a
//! gesture on the desktop planner uses, ending in `summarize_calendar` and a
//! save exactly as if the edit had been made at the desk.
//!
//! That shape is the whole design. `DOCUMENTATION.md` §4.1 is explicit that two
//! writers of `read_at_startup.json` clobber each other, so a sync daemon or a
//! second app editing the file is the documented data-loss case. Here there is
//! no second copy of the truth to reconcile — no identity mapping, no
//! tombstones, no conflict policy — because the problem those solve is never
//! created. The price is that the phone view lives only while the desktop app
//! runs, which for a wall calendar that is on all day is the usual state.
//!
//! The same server also publishes the calendar as an iCalendar feed
//! (`/calendar.ics`), so events, due dates, the day plan and routines can be
//! *subscribed to* from Google Calendar or a phone's own calendar app —
//! read-only, and useful precisely when the desktop is off.
//!
//! ## Threads
//!
//! Same shape as the weather thread: a plain thread, blocking I/O, an
//! `EventLoopProxy` to wake the UI. The worker never touches `TaskApp`. It
//! parses the request into a `Command`, sends it with a reply channel, pokes the
//! event loop, and waits for the answer. The UI thread drains the queue from
//! `App::user_event` — so a request is served even while the window is
//! minimized or asleep — and again at the top of each frame.
//!
//! Everything here that can be tested without a socket is pure and tested:
//! routing, authorisation, the JSON command shapes, the day snapshot, the ICS
//! feed.

use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::{IpAddr, UdpSocket},
    path::Path,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, TryRecvError, channel},
    },
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, NaiveDate, Timelike, Utc};
use flate2::{Compression, write::GzEncoder};
use serde::Serialize;
use tiny_http::{Header, Method, Request as HttpRequest, Response, Server};

/// What the server calls after queueing a request, so whoever serves the
/// queue wakes up. The desktop hands in a closure that pokes its event loop
/// (the UI thread may be asleep, §5.4); `taskdeck-server`, whose only thread
/// is already waiting on the queue, hands in nothing at all.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

use crate::{
    archive::Archived,
    board::{Board, Reply},
    planner,
    subscriptions,
    tasks::{self, Active, HORIZON_LABELS, IMPORTANCE_LABELS, WEEKDAY_NAMES},
};

// The command set is the board's (`board.rs`): the phone speaks it over HTTP,
// the desktop speaks it to a server, an offline desktop queues it. The names
// below are kept so the phone-facing code reads as it did.
pub use crate::board::{BoardError as PhoneError, Command, Kind, local_input, parse_local_input};

/// Port the server listens on unless the config says otherwise.
pub const DEFAULT_PORT: u16 = 7373;
/// Address the server binds when the setting says nothing: every interface,
/// because the phone is sometimes on the LAN and sometimes on the tailnet and
/// the token guards the door either way. One address serves that one only.
/// Where a fresh install listens: **this machine only**.
///
/// The phone reaches the server through `tailscale serve`, which connects to
/// `127.0.0.1` — so loopback is the whole of what the deployed path needs, and
/// every interface is a door nothing walks through except strangers. Measured
/// on a real server: two TCP connections that declare a body and never send it
/// hold both workers for as long as they stay open, and `tiny_http` 0.12 has no
/// socket read timeout to break that. Bound to `0.0.0.0` that is anyone on the
/// café or campus network the laptop joined; bound here it is nobody.
///
/// A LAN setup with no Tailscale sets `phone_bind_address` in the file. An
/// install that already names an address keeps it — this only decides what a
/// new one starts as, and the server says at startup which door is open.
pub const DEFAULT_BIND: &str = "127.0.0.1";
/// Lowest port the settings accept: the privileged range needs root and is
/// full of things that are not calendars.
pub const PORT_MIN: u16 = 1024;
/// Most days one `/api/state` call will describe.
///
/// The ceiling is still what it always was — a typo in the URL must not be able
/// to ask for ten years of days — but the number is no longer arbitrary. The
/// phone's agenda pages by calendar month, and 31 is the longest one, so a
/// request, a cache key and a slot on the month rail are all the same unit.
///
/// It stops there rather than higher because `build_day` rescans the whole
/// archive for every day it builds, and `ArchiveLog` holds every line ever
/// written: the per-day cost is flat only while the archive is small. Before
/// this number moves again, the archive wants bucketing by session date once
/// per snapshot, ahead of the map chain in `snapshot`.
///
/// The clamp that enforces it is deliberately silent — see `snapshot`.
pub const MAX_SNAPSHOT_DAYS: u32 = 31;

/// How long the worker waits for the UI thread to answer a command before
/// telling the phone to try again. Generous: a save on a slow disk plus a
/// calendar rebuild is well under a second, but the UI thread also has to be
/// woken first.
const REPLY_TIMEOUT: Duration = Duration::from_secs(8);
/// How long `/api/wait` holds a connection open before answering "nothing
/// changed". Under the proxies and idle timeouts a phone's connection may
/// pass through, and long enough that a quiet afternoon is a few requests.
const WAIT_TIMEOUT: Duration = Duration::from_secs(25);
/// Threads serving requests. Two, so a slow client cannot hold up the next
/// one. Long polls do not occupy a worker at all — see `Pulse` — which is what
/// keeps the number this small.
const WORKERS: usize = 2;
/// Largest request body accepted. A command is a few hundred bytes; the one
/// exception is `SetNotes`, whose text may run to `NOTES_MAX_CHARS` characters
/// of up to four bytes each — sized from that, with room for the envelope, so
/// the longest notes the board accepts are refused by the board's own message
/// and never by this cap.
const MAX_BODY_BYTES: usize = crate::board::NOTES_MAX_CHARS * 4 + 4 * 1024;
/// The header the page sends its token in.
const TOKEN_HEADER: &str = "X-TaskDeck-Token";
/// The header a client names a command with, the same on every retry of it,
/// so a repeat is answered from `Replies` rather than applied twice.
pub const REQUEST_HEADER: &str = "X-TaskDeck-Request";
/// How many recent replies are kept by key. A retry comes within seconds,
/// but a client replaying a long outbox in one burst can send several hundred
/// commands in those seconds, and a key from the start of the burst must
/// still be there when its retry comes. A thousand entries is a few hundred
/// kilobytes at most.
const REPLIES_KEPT: usize = 1024;
/// A declared body beyond this is never read — nor drained: `tiny_http`
/// drains an unread body on drop with one allocation of the declared size,
/// which for a hostile `Content-Length` is the whole process gone. Such a
/// request is leaked instead, which costs its one connection and nothing else.
const DRAIN_LIMIT: usize = 8 * 1024 * 1024;
/// Length of a generated token, in characters of the alphabet below.
const TOKEN_LENGTH: usize = 32;
/// Lower-case letters and digits: safe in a URL, a QR code, and a phone keyboard.
const TOKEN_ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";

const PAGE: &str = include_str!("phone.html");
const SERVICE_WORKER: &str = include_str!("phone_sw.js");
const ICON: &[u8] = include_bytes!("../icon.png");

/* ─────────────────────────────── Answering ─────────────────────────────── */

/// How many archive rows the snapshot carries for the phone's Done list.
/// Recent history is what a phone wants — "did I tick that off?" — not the
/// ledger; that stays at the desk.
pub const DONE_ROWS_MAX: usize = 40;

pub type PhoneReply = Result<serde_json::Value, PhoneError>;

/// A carried-out command, as the wire says it: `{ ok, version, id?, session?, moved? }`.
pub fn reply_json(reply: &Reply, version: u64) -> serde_json::Value {
    let mut value = serde_json::json!({ "ok": true, "version": version });
    if let Some(id) = reply.id {
        value["id"] = serde_json::json!(id);
    }
    if let Some(session) = reply.session {
        value["session"] = serde_json::json!(session);
    }
    if let Some(moved) = reply.moved {
        value["moved"] = serde_json::json!(moved);
    }
    value
}

/// Answer one of the two questions the phone asks — the snapshot or the
/// feed — from a board. Shared by every host of a board: the desktop app and
/// `taskdeck-server` both call this, so the phone sees the same answer
/// whichever is serving it.
///
/// `ranked` is the desktop's task list order when the host has one drawn;
/// otherwise it is computed here, the same way.
pub fn answer_query(
    board: &mut Board,
    palette: [[u8; 4]; 6],
    ranked: Option<Vec<u64>>,
    command: &Command,
    now: DateTime<Local>,
) -> PhoneReply {
    match command {
        Command::Snapshot { from, days } => {
            let snapshot = snapshot(board, palette, ranked, *from, *days, now);
            serde_json::to_value(snapshot).map_err(|error| PhoneError::failed(error.to_string()))
        }
        Command::Feed => Ok(serde_json::Value::String(calendar_feed(&board.items, now))),
        Command::Board => {
            let state = board
                .state()
                .map_err(|error| PhoneError::failed(format!("The archive could not be read, so the board was not served:\n{error}")))?;
            serde_json::to_value(state).map_err(|error| PhoneError::failed(error.to_string()))
        }
        _ => Err(PhoneError::bad_request("That is a command, not a query.")),
    }
}

/// Everything the page needs for `days` days from `from`, plus the tray for
/// `from`.
pub fn snapshot(
    board: &mut Board,
    palette: [[u8; 4]; 6],
    ranked: Option<Vec<u64>>,
    from: NaiveDate,
    days: u32,
    now: DateTime<Local>,
) -> Snapshot {
    // Ghosts need the archive — read once and kept (§17.2). A read failure
    // is not raised from here: the phone asks every few seconds, and an
    // error window re-opening at the desk on that cadence for a problem the
    // desk hears about the moment it opens the planner is nagging, not
    // reporting. The day is simply drawn without its ghosts.
    let _ = board.load_archive();
    // Clamped, and clamped quietly: asking for more than the server will build
    // is not a client error, it is a client meeting a server limit, and a 400
    // would break a page that sensibly asks for as much as it can use. The
    // caller is expected to count the days it got back rather than trust the
    // number it asked for, which is what the phone's pager does.
    let days = days.clamp(1, MAX_SNAPSHOT_DAYS);
    let day_snapshots = (0..days as i64)
        .map(|offset| from + ChronoDuration::days(offset))
        .map(|day| build_day(&board.items, board.archive.entries(), day, now))
        .collect();
    let day_snapshots = with_subscribed(day_snapshots, board.subscriptions(), board.overlay());

    Snapshot {
        version: board.version(),
        today: now.date_naive(),
        now_minutes: now.hour() as i32 * 60 + now.minute() as i32,
        palette,
        labels: Labels::current(),
        days: day_snapshots,
        tray: build_tray(&board.items, from, now, Board::shuffle_seed(now)),
        items: board.items.iter().map(item_detail).collect(),
        ranked: ranked.unwrap_or_else(|| board.ranked_ids(now)),
        done: done_rows(board.archive.entries()),
        notes: board.notes.clone(),
        // Only when there is something to be ignorant about.
        known: if board.subscriptions().iter().any(|s| s.enabled) { board.overlay().covers } else { None },
        look: backdrop().map(|made| Look {
            id: made.id.clone(),
            aspect: BACKGROUND_WIDE as f32 / BACKGROUND_TALL as f32,
            top: made.top.clone(),
        }),
        // Beside `look`, never inside it. The dials are settings and exist
        // whether or not a picture does; sent only with the picture, the sheet
        // would open on a hardcoded guess whenever the board had none — and the
        // first touch of either slider would write that guess over the file.
        dials: {
            let dials = look_dials();
            Dials { blur: dials.blur_percent, light: dials.light_percent }
        },
    }
}

/// One question from the phone, waiting on the UI thread's answer.
pub struct PhoneRequest {
    pub command: Command,
    pub reply: Sender<PhoneReply>,
}

/// Replies to recent commands, by the key the client sent with them
/// (`REQUEST_HEADER`). A client that lost the answer — a timeout, a reset, a
/// 503 from a board that answered late — sends the same command with the same
/// key and gets the first reply back instead of a second application. The
/// commands themselves carry no key, so this is where "once" lives; the list
/// is short because a retry comes within seconds, not days.
#[derive(Default)]
pub struct Replies {
    state: Mutex<RepliesState>,
}

#[derive(Default)]
struct RepliesState {
    order: VecDeque<String>,
    known: HashMap<String, Slot>,
}

/// What is known about one key.
enum Slot {
    /// A worker is waiting on the board for it right now.
    InFlight,
    /// The worker gave up waiting, but the board still has the request and
    /// will answer down this channel when it gets to it: the outcome is
    /// *unknown*, not "not applied", and a repeat must not apply it again.
    Late(Receiver<PhoneReply>),
    /// Answered.
    Done(serde_json::Value),
}

enum Claim {
    /// Already answered: here is what was said.
    Done(serde_json::Value),
    /// Ours to carry out.
    Ours,
    /// Handed to the board earlier and not answered yet: say so (`503`), and
    /// the client tries again with the same key later.
    Busy,
}

impl Replies {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RepliesState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether `key` has been answered. A key still in flight is waited for,
    /// up to the reply timeout, and is busy after that; a key the board still
    /// owes an answer for is settled the moment that answer is found; an
    /// empty key is nobody's and always ours.
    fn claim(&self, key: &str) -> Claim {
        if key.is_empty() {
            return Claim::Ours;
        }
        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            {
                let mut state = self.lock();
                match state.known.get(key) {
                    Some(Slot::Done(value)) => return Claim::Done(value.clone()),
                    Some(Slot::InFlight) => {}
                    Some(Slot::Late(late)) => match late.try_recv() {
                        Ok(Ok(value)) => {
                            state.known.insert(key.to_string(), Slot::Done(value.clone()));
                            return Claim::Done(value);
                        }
                        // The board refused it, or went away without a word:
                        // the repeat is a fresh attempt.
                        Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                            state.known.insert(key.to_string(), Slot::InFlight);
                            return Claim::Ours;
                        }
                        Err(TryRecvError::Empty) => return Claim::Busy,
                    },
                    None => {
                        state.known.insert(key.to_string(), Slot::InFlight);
                        state.order.push_back(key.to_string());
                        while state.order.len() > REPLIES_KEPT {
                            if let Some(old) = state.order.pop_front() {
                                state.known.remove(&old);
                            }
                        }
                        return Claim::Ours;
                    }
                }
            }
            if Instant::now() >= deadline {
                return Claim::Busy;
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn settle(&self, key: &str, value: serde_json::Value) {
        if key.is_empty() {
            return;
        }
        if let Some(slot) = self.lock().known.get_mut(key) {
            *slot = Slot::Done(value);
        }
    }

    /// The board said no: the next request with this key is a fresh one.
    fn release(&self, key: &str) {
        if key.is_empty() {
            return;
        }
        let mut state = self.lock();
        state.known.remove(key);
        state.order.retain(|k| k != key);
    }

    /// The board has the request but did not answer in time. Its answer, when
    /// it comes, arrives on `late`; until then the key is busy, not free.
    fn park_late(&self, key: &str, late: Receiver<PhoneReply>) {
        if key.is_empty() {
            return;
        }
        if let Some(slot) = self.lock().known.get_mut(key) {
            *slot = Slot::Late(late);
        }
    }
}

/// The client's name for a command, from its header; empty when it sent none.
fn request_key(headers: &[Header]) -> String {
    headers
        .iter()
        .find(|header| header.field.equiv(REQUEST_HEADER))
        .map(|header| header.value.as_str().trim().chars().take(128).collect())
        .unwrap_or_default()
}

/* ─────────────────────────────── The pulse ─────────────────────────────── */

/// Requests parked until there is something to tell them: what each one has
/// already seen, and when to answer it regardless. Generic over the request so
/// the decision can be tested without a socket.
struct Parked<T> {
    items: Vec<(T, u64, Instant)>,
}

impl<T> Parked<T> {
    fn new() -> Self {
        Self { items: Vec::new() }
    }

    fn push(&mut self, item: T, seen: u64, deadline: Instant) {
        self.items.push((item, seen, deadline));
    }

    /// Take everything that is due: the version has moved past what it saw,
    /// its deadline has passed, or `all` — the server is going away.
    fn take_due(&mut self, version: u64, now: Instant, all: bool) -> Vec<T> {
        let (due, kept): (Vec<_>, Vec<_>) = self
            .items
            .drain(..)
            .partition(|(_, seen, deadline)| all || *seen != version || now >= *deadline);
        self.items = kept;
        due.into_iter().map(|(item, _, _)| item).collect()
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.items.iter().map(|(_, _, deadline)| *deadline).min()
    }

    fn len(&self) -> usize {
        self.items.len()
    }
}

struct PulseState {
    version: u64,
    /// Counts `interrupt` calls, so a server going away can have its parked
    /// requests answered without the version pretending to have moved.
    interrupts: u64,
    parked: Parked<HttpRequest>,
    /// Set, under the lock, by the answering thread as it leaves; a park that
    /// finds it set is refused, so nothing waits on a thread that is gone.
    closed: bool,
}

/// The version of the world, and the requests waiting for it to move.
///
/// The UI thread publishes a new number after every save. A `/api/wait`
/// request is **parked** here rather than holding its worker: the worker
/// hands the request over and goes back to serving, and one thread per
/// server (`answer_parked`) answers every parked request the moment the
/// version moves past what it saw, or its deadline passes. That is what lets
/// a drag at the desk show on the phone within a second without the phone
/// asking every second, without the wait ever touching the UI thread — and
/// without an abandoned wait costing anything. The first version parked the
/// worker itself, and a phone reloading its page a few times (each reload
/// abandoning a wait the server could not see was abandoned) parked every
/// worker for the full timeout, and commands queued behind them.
#[derive(Default)]
pub struct Pulse {
    state: Mutex<PulseState>,
    changed: Condvar,
}

impl Default for PulseState {
    fn default() -> Self {
        Self { version: 0, interrupts: 0, parked: Parked::new(), closed: true }
    }
}

impl Pulse {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PulseState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn current(&self) -> u64 {
        self.lock().version
    }

    /// Announce `version`: everything parked with an older one is answered.
    pub fn publish(&self, version: u64) {
        self.lock().version = version;
        self.changed.notify_all();
    }

    /// Answer everything parked with the version unchanged. What a server
    /// being dropped does, so its listeners are told now rather than at their
    /// timeout — publishing a fake version instead would have the page reload
    /// for nothing, or worse, spin against a number the UI thread never issued.
    pub fn interrupt(&self) {
        {
            let mut state = self.lock();
            state.interrupts = state.interrupts.wrapping_add(1);
        }
        self.changed.notify_all();
    }

    /// Park a request until the version moves past `seen` or `deadline` —
    /// or hand it straight back when no thread is there to answer it.
    fn park(&self, request: HttpRequest, seen: u64, deadline: Instant) -> Option<HttpRequest> {
        let mut state = self.lock();
        if state.closed {
            return Some(request);
        }
        state.parked.push(request, seen, deadline);
        drop(state);
        self.changed.notify_all();
        None
    }

    /// How many requests are parked. For the tests and the curious.
    pub fn parked(&self) -> usize {
        self.lock().parked.len()
    }
}

/// The one thread per server that answers parked requests. Runs until the
/// server's `stopping` flag is set, answering everything on the way out.
fn answer_parked(pulse: &Pulse, stopping: &AtomicBool) {
    let mut state = pulse.lock();
    state.closed = false;
    let mut epoch = state.interrupts;
    loop {
        let now = Instant::now();
        let stop = stopping.load(Ordering::Relaxed);
        let interrupted = state.interrupts != epoch;
        epoch = state.interrupts;
        let version = state.version;

        let due = state.parked.take_due(version, now, stop || interrupted);
        if !due.is_empty() {
            // Respond with the lock released: a slow client must not hold
            // up the UI thread's next publish.
            drop(state);
            for request in due {
                let body = guarded(
                    Encoding::PLAIN.json(serde_json::json!({ "version": version })).with_header(no_store()),
                );
                let _ = request.respond(body);
            }
            state = pulse.lock();
            continue;
        }
        if stop {
            // Closed under the same lock a park takes: after this, a park is
            // refused rather than left for nobody.
            state.closed = true;
            break;
        }

        // Sleep until the earliest deadline, a publish, an interrupt, or a
        // new arrival — and at most a second, so a missed wake costs little.
        let until = state
            .parked
            .next_deadline()
            .map(|deadline| deadline.saturating_duration_since(now))
            .unwrap_or(Duration::from_secs(1))
            .min(Duration::from_secs(1));
        state = pulse
            .changed
            .wait_timeout(state, until)
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .0;
    }
}

/* ─────────────────────────────── The server ─────────────────────────────── */

/// The listening socket and the threads serving it. Dropping it stops them.
pub struct PhoneServer {
    server: Arc<Server>,
    stopping: Arc<AtomicBool>,
    pulse: Arc<Pulse>,
    pub port: u16,
}

impl PhoneServer {
    /// Bind `port` on `bind` — this machine alone by default (`DEFAULT_BIND`), or
    /// the one address given — and start serving.
    ///
    /// Loopback is the default because a default should not be a decision
    /// nobody made: the phone is sometimes on the LAN and sometimes on a
    /// Tailscale address, and which of those the reader wants is something only
    /// they know. Naming the address — `--bind 100.x.y.z`, or
    /// `phone_bind_address` — opens exactly that one and no other, which is the
    /// right posture for a box that also sits on a network it should not serve
    /// (`SERVER.md` §2). The token still guards the door in every case.
    /// `version` is the board's, as it stands right now.
    ///
    /// It is a parameter rather than something the caller is trusted to publish
    /// beforehand, because forgetting it is silent and expensive. `/api/wait`
    /// answers the instant its version differs from the one the phone sends,
    /// and the phone sends the version off its last snapshot — the board's,
    /// which a server seeds from the clock. A pulse still at zero therefore
    /// answers *every* wait immediately with a number no snapshot will ever
    /// carry, and the page refetches and asks again as fast as the link
    /// allows: thousands of requests a second, on a phone, in a pocket, with
    /// nothing wrong at either end except that they never agree.
    pub fn start(
        bind: &str,
        port: u16,
        token: String,
        tx: Sender<PhoneRequest>,
        wake: Wake,
        pulse: Arc<Pulse>,
        version: u64,
    ) -> Result<Self, String> {
        if token.is_empty() {
            return Err("The phone view has no token to guard it with.".to_string());
        }
        pulse.publish(version);
        let ip: IpAddr = bind
            .trim()
            .parse()
            .map_err(|_| format!("`{bind}` is not an address to listen on; use one like 0.0.0.0 or 100.64.0.1."))?;
        let server = Server::http((ip, port))
            .map_err(|error| format!("Could not listen on {ip}:{port}: {error}"))?;
        let server = Arc::new(server);
        let stopping = Arc::new(AtomicBool::new(false));

        {
            let pulse = Arc::clone(&pulse);
            let stop_flag = Arc::clone(&stopping);
            thread::Builder::new()
                .name("taskdeck-phone-pulse".to_string())
                .spawn(move || answer_parked(&pulse, &stop_flag))
                .map_err(|error| format!("Could not start the phone view's thread: {error}"))?;
        }

        let replies = Arc::new(Replies::new());
        for index in 0..WORKERS {
            let worker = Arc::clone(&server);
            let stop_flag = Arc::clone(&stopping);
            let token = token.clone();
            let tx = tx.clone();
            let wake = Arc::clone(&wake);
            let pulse = Arc::clone(&pulse);
            let replies = Arc::clone(&replies);
            thread::Builder::new()
                .name(format!("taskdeck-phone-{index}"))
                .spawn(move || {
                    loop {
                        match worker.recv() {
                            Ok(request) => handle(request, &token, &tx, &wake, &pulse, &replies),
                            // `recv` errs both when unblocked (we are stopping)
                            // and when accepting a connection failed. The second
                            // is not fatal to the listener, so it is logged and
                            // the loop goes round — after a pause, so a broken
                            // socket cannot spin a core.
                            Err(error) => {
                                if stop_flag.load(Ordering::Relaxed) {
                                    break;
                                }
                                eprintln!("Phone view: {error}");
                                thread::sleep(Duration::from_millis(200));
                            }
                        }
                    }
                })
                .map_err(|error| format!("Could not start the phone view's thread: {error}"))?;
        }

        Ok(Self { server, stopping, pulse, port })
    }
}

impl Drop for PhoneServer {
    fn drop(&mut self) {
        // Not joined: a worker may be mid-request, waiting on a reply that only
        // the thread dropping us can produce. Each notices the flag on its next
        // `recv`, and the socket closes when the last `Arc` goes. `unblock`
        // frees one blocked `recv` per call — queued, so a worker busy right
        // now still finds its unblock later — hence one per worker. The
        // interrupt has the pulse thread answer every parked listener and
        // leave.
        self.stopping.store(true, Ordering::Relaxed);
        self.pulse.interrupt();
        for _ in 0..WORKERS {
            self.server.unblock();
        }
    }
}

type Body = Response<std::io::Cursor<Vec<u8>>>;

/// Answer one HTTP request.
fn handle(
    mut request: HttpRequest,
    token: &str,
    tx: &Sender<PhoneRequest>,
    wake: &Wake,
    pulse: &Pulse,
    replies: &Replies,
) {
    if request.body_length().is_some_and(|length| length > DRAIN_LIMIT) {
        // See `DRAIN_LIMIT`: not answered, not drained, not dropped.
        std::mem::forget(request);
        return;
    }
    let url = request.url().to_string();
    let (path, query) = split_url(&url);
    let query = parse_query(query);
    let authorised = is_authorised(request.headers(), &query, token);
    let key = request_key(request.headers());
    let encoding = Encoding::of(request.headers());

    // Long poll: parked on the pulse and answered by its thread the moment the
    // world changes, or after `WAIT_TIMEOUT` with the version unchanged. This
    // worker hands the request over and is free at once.
    if *request.method() == Method::Get && path == "/api/wait" {
        if !authorised {
            let refused = json_error(401, "Not authorised. Open the phone view from the link in TaskDeck's settings.");
            let _ = request.respond(guarded(refused.with_header(no_store())));
            return;
        }
        let seen = query.lookup("version").and_then(|text| text.parse::<u64>().ok()).unwrap_or(0);
        if let Some(request) = pulse.park(request, seen, Instant::now() + WAIT_TIMEOUT) {
            // The pulse thread has left: nothing would answer a request
            // parked now. Answered here instead, with what there is.
            let now = Encoding::PLAIN.json(serde_json::json!({ "version": pulse.current() }));
            let _ = request.respond(guarded(now.with_header(no_store())));
        }
        return;
    }

    let response = match (request.method(), path) {
        // The page itself carries no data and is public: it has to be, so a
        // home-screen shortcut that opens `/` can pick its token back up from
        // storage before it asks for anything.
        (Method::Get | Method::Head, "/") | (Method::Get | Method::Head, "/index.html") => {
            static PACKED: OnceLock<Option<Vec<u8>>> = OnceLock::new();
            encoding.fixed(PAGE, &PACKED, "text/html; charset=utf-8")
        }
        (Method::Get | Method::Head, "/icon.png") => with_type(Response::from_data(ICON.to_vec()), "image/png"),
        (Method::Get | Method::Head, "/icon-192.png") => with_type(Response::from_data(icon_at(192)), "image/png"),
        (Method::Get | Method::Head, "/icon-512.png") => with_type(Response::from_data(icon_at(512)), "image/png"),
        // The offline shell (`phone_sw.js`). Public like the page; it holds
        // no data and a browser only honours it from a secure origin.
        (Method::Get | Method::Head, "/sw.js") => {
            static PACKED: OnceLock<Option<Vec<u8>>> = OnceLock::new();
            encoding.fixed(SERVICE_WORKER, &PACKED, "application/javascript; charset=utf-8")
        }
        (Method::Get | Method::Head, "/manifest.webmanifest") => {
            with_type(Response::from_string(manifest(query.lookup("token").map(String::as_str))), "application/manifest+json")
        }
        _ if !authorised => json_error(
            401,
            "Not authorised. Open the phone view from the link in TaskDeck's settings.",
        ),
        // `/bg-<hash>.jpg`. The hash is the content's, so the URL changes when
        // the picture does and never otherwise — which is what lets it be
        // cached for a year and never re-fetched. Everything else here is
        // `no-store`, and a background re-sent on every open would cost more
        // than the whole rest of the program.
        (Method::Get | Method::Head, path) if path.starts_with("/bg-") && path.ends_with(".jpg") => {
            match backdrop().filter(|made| path == format!("/bg-{}.jpg", made.id)) {
                // One URL, two codecs, chosen by what the client offers to
                // take. `Vary: Accept` so a cache never hands an AVIF to
                // something that cannot read one.
                Some(made) => {
                    let wants_avif = request
                        .headers()
                        .iter()
                        .any(|h| h.field.equiv("Accept") && h.value.as_str().contains("image/avif"));
                    let (body, kind) = match (&made.avif, wants_avif) {
                        (Some(avif), true) => (avif.clone(), "image/avif"),
                        _ => (made.jpeg.clone(), "image/jpeg"),
                    };
                    with_type(Response::from_data(body), kind).with_header(header("Vary", "Accept"))
                }
                None => json_error(404, "No such picture."),
            }
        }
        // The phone's own picture, already cropped by it to the shape this
        // server serves. An empty body means "use the desk's again".
        // Same gate as `/api/command` below, same reason, because this writes
        // too. It names a different type only because the phone sends the crop
        // as `application/octet-stream` — which a form and a simple
        // cross-origin `fetch` can no more set than they can `application/json`.
        (Method::Post, "/api/background") if !is_type(request.headers(), "application/octet-stream") => {
            json_error(415, "Send this as application/octet-stream.")
        }
        (Method::Post, "/api/background") => {
            let clearing = request.headers().iter().any(|h| h.field.equiv("X-TaskDeck-Clear"));
            match read_body_up_to(&mut request, MAX_UPLOAD_BYTES) {
            Ok(body) => match adopt_background(if clearing { &[] } else { &body }) {
                Ok(id) => encoding.json(serde_json::json!({ "ok": true, "id": id })),
                Err(why) => json_error(400, &why),
            },
            Err(error) => json_error(error.status, &error.message),
            }
        }
        // The two dials, turned from the phone. Kept in the config file so the
        // desk agrees, and the picture is re-made from whichever source it
        // came from — the phone's crop if there is one, the desk's if not.
        (Method::Post, "/api/look") if !is_json(request.headers()) => {
            json_error(415, "Send this as application/json.")
        }
        (Method::Post, "/api/look") => match read_body(&mut request) {
            Ok(body) => match serde_json::from_slice::<LookDialsWire>(&body) {
                Ok(wire) => {
                    set_look_dials(LookDials { blur_percent: wire.blur.min(100), light_percent: wire.light.min(100) });
                    let kept = remember_dials(wire.blur.min(100), wire.light.min(100));
                    let id = remake_backdrop();
                    encoding.json(serde_json::json!({ "ok": true, "id": id, "kept": kept }))
                }
                Err(_) => json_error(400, "That is not a pair of dials."),
            },
            Err(error) => json_error(error.status, &error.message),
        },
        (Method::Get, "/api/state") => match snapshot_command(&query) {
            Ok(command) => match ask(command, tx, wake) {
                Ok(value) => encoding.json(value),
                Err(error) => json_error(error.status, &error.message),
            },
            Err(error) => json_error(error.status, &error.message),
        },
        // The whole board, for a desktop that keeps a replica of it.
        (Method::Get, "/api/board") => match ask(Command::Board, tx, wake) {
            Ok(value) => encoding.json(value),
            Err(error) => json_error(error.status, &error.message),
        },
        // The content type is checked before the body is read, and this is the
        // only reason a browser cannot be made to write to the board from
        // somewhere else. A cross-origin `fetch` or form can send `text/plain`,
        // `multipart/form-data` or `application/x-www-form-urlencoded` with no
        // permission asked; asking for `application/json` puts the request in
        // the class that needs a CORS preflight, and this server answers no
        // preflight at all. Nothing else here stops it: there is no session
        // cookie to be `SameSite`, but the token rides in the query string, and
        // a token that has leaked once should not also be a write key for every
        // page the phone visits.
        (Method::Post, "/api/command") if !is_json(request.headers()) => {
            json_error(415, "Commands are sent as application/json.")
        }
        (Method::Post, "/api/command") => match read_body(&mut request) {
            Ok(body) => match serde_json::from_slice::<Command>(&body) {
                Ok(command) if command.is_query() => json_error(400, "That is a query, not a command."),
                Ok(command) => match replies.claim(&key) {
                    // Asked before, under this key, and answered: the same
                    // answer again, and the board hears nothing.
                    Claim::Done(value) => encoding.json(value),
                    // Asked before and still with the board: not again.
                    Claim::Busy => json_error(503, "TaskDeck is still working on that change; it is not lost."),
                    Claim::Ours => {
                        let (reply_tx, reply_rx) = channel();
                        if tx.send(PhoneRequest { command, reply: reply_tx }).is_err() {
                            replies.release(&key);
                            json_error(503, "TaskDeck is shutting down.")
                        } else {
                            wake();
                            match reply_rx.recv_timeout(REPLY_TIMEOUT) {
                                Ok(Ok(value)) => {
                                    replies.settle(&key, value.clone());
                                    encoding.json(value)
                                }
                                Ok(Err(error)) => {
                                    replies.release(&key);
                                    json_error(error.status, &error.message)
                                }
                                Err(_) => {
                                    // The board has it and will apply it; the
                                    // outcome is unknown, so the key stays
                                    // taken and the answer is collected later.
                                    replies.park_late(&key, reply_rx);
                                    json_error(503, "TaskDeck did not answer in time. Is it still running?")
                                }
                            }
                        }
                    }
                },
                Err(error) => json_error(400, &format!("Could not read the command: {error}")),
            },
            Err(error) => json_error(error.status, &error.message),
        },
        (Method::Get | Method::Head, "/calendar.ics") => match ask(Command::Feed, tx, wake) {
            Ok(serde_json::Value::String(feed)) => encoding.text(feed, "text/calendar; charset=utf-8"),
            Ok(_) => json_error(500, "The feed came back in the wrong shape."),
            Err(error) => json_error(error.status, &error.message),
        },
        _ => json_error(404, "No such page."),
    };

    let mut response = response.with_header(caching_for(path));
    for guard in guards() {
        response.add_header(guard);
    }
    let _ = request.respond(response);
}

/// Hand a command to the UI thread and wait for its answer.
fn ask(command: Command, tx: &Sender<PhoneRequest>, wake: &Wake) -> PhoneReply {
    let (reply_tx, reply_rx) = channel();
    if tx.send(PhoneRequest { command, reply: reply_tx }).is_err() {
        return Err(PhoneError { status: 503, message: "TaskDeck is shutting down.".to_string() });
    }
    // The serving thread may be asleep (§5.4); this is what wakes it.
    wake();
    match reply_rx.recv_timeout(REPLY_TIMEOUT) {
        Ok(reply) => reply,
        Err(_) => Err(PhoneError {
            status: 503,
            message: "TaskDeck did not answer in time. Is it still running?".to_string(),
        }),
    }
}

/// `GET /api/state?from=YYYY-MM-DD&days=N` → `Command::Snapshot`. Both
/// default: `from` to today, `days` to one.
fn snapshot_command(query: &[(String, String)]) -> Result<Command, PhoneError> {
    let from = match query.lookup("from") {
        Some(text) => NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .ok()
            // Fourteen days past the last day chrono has would overflow; a
            // year outside the calendar's range is refused like a typo.
            .filter(|day| (1970..=9999).contains(&day.year()))
            .ok_or_else(|| PhoneError::bad_request(format!("`from` should be a date like 2026-09-04, not `{text}`.")))?,
        None => Local::now().date_naive(),
    };
    let days = match query.lookup("days") {
        Some(text) => text
            .parse::<u32>()
            .map_err(|_| PhoneError::bad_request(format!("`days` should be a number, not `{text}`.")))?,
        None => 1,
    };
    Ok(Command::Snapshot { from, days })
}

fn read_body(request: &mut HttpRequest) -> Result<Vec<u8>, PhoneError> {
    read_body_up_to(request, MAX_BODY_BYTES)
}

fn read_body_up_to(request: &mut HttpRequest, cap: usize) -> Result<Vec<u8>, PhoneError> {
    if request.body_length().is_some_and(|length| length > cap) {
        return Err(PhoneError::bad_request("That request is too large."));
    }
    let mut body = Vec::new();
    request
        .as_reader()
        .take(cap as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|error| PhoneError::bad_request(format!("Could not read the request: {error}")))?;
    if body.len() > cap {
        return Err(PhoneError::bad_request("That request is too large."));
    }
    Ok(body)
}

/* ─────────────────────────── Authorisation ─────────────────────────── */

/// Whether a request carries the token — in the page's header, as a bearer
/// token, or in the query string (the only place a calendar app subscribing
/// to the feed can put it).
pub fn is_authorised(headers: &[Header], query: &[(String, String)], token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    // Every credential offered is tried: a proxy may add a bearer of its own
    // in front of the page's header, and that must not hide the right one.
    let from_headers = headers.iter().filter_map(|header| {
        if header.field.equiv(TOKEN_HEADER) {
            Some(header.value.as_str().trim().to_string())
        } else if header.field.equiv("Authorization") {
            header.value.as_str().trim().strip_prefix("Bearer ").map(|rest| rest.trim().to_string())
        } else {
            None
        }
    });
    let from_query = query.lookup("token").cloned();
    from_headers.chain(from_query).any(|candidate| same_token(&candidate, token))
}

/// Compare two tokens without leaking where they first differ. On a LAN this
/// is more habit than defence, and it costs nothing.
fn same_token(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A fresh token: 160 bits from the OS's randomness, spelled in a small
/// alphabet so it survives a QR code and a phone keyboard.
pub fn generate_token() -> String {
    random_bytes(TOKEN_LENGTH)
        .into_iter()
        .map(|byte| TOKEN_ALPHABET[byte as usize % TOKEN_ALPHABET.len()] as char)
        .collect()
}

/// `n` random bytes. `/dev/urandom` where there is one; elsewhere the
/// standard library's per-process hash seed is stirred with the clock and the
/// pid, which is not a CSPRNG but is unguessable from outside the machine —
/// which is the threat here.
fn random_bytes(n: usize) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::io::Read as _;
        if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
            let mut bytes = vec![0u8; n];
            if file.read_exact(&mut bytes).is_ok() {
                return bytes;
            }
        }
    }
    use std::hash::{BuildHasher, Hasher};
    let mut bytes = Vec::with_capacity(n);
    let mut counter: u64 = 0;
    while bytes.len() < n {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(counter);
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        hasher.write_u32(std::process::id());
        bytes.extend_from_slice(&hasher.finish().to_le_bytes());
        counter += 1;
    }
    bytes.truncate(n);
    bytes
}

/* ─────────────────────────────── HTTP bits ─────────────────────────────── */

/// Split a request URL into its path and its raw query string.
pub fn split_url(url: &str) -> (&str, &str) {
    match url.split_once('?') {
        Some((path, query)) => (path, query),
        None => (url, ""),
    }
}

/// `a=1&b=two` → `[("a","1"),("b","two")]`, percent-decoded.
pub fn parse_query(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

/// `lookup` rather than `get`: a slice already has an inherent `get`, which
/// would win the name and then refuse the `&str`.
trait QueryLookup {
    fn lookup(&self, key: &str) -> Option<&String>;
}

impl QueryLookup for [(String, String)] {
    fn lookup(&self, key: &str) -> Option<&String> {
        self.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                // Two bytes, never a `str` slice: `%` followed by a multibyte
                // character is not on a character boundary.
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|hex| u8::from_str_radix(hex, 16).ok());
                match hex {
                    Some(byte) => {
                        out.push(byte);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("static header text is ASCII")
}

/// Whether the body is offered as JSON. The parameters after `;` are the
/// sender's business — `application/json; charset=utf-8` is still JSON.
fn is_json(headers: &[Header]) -> bool {
    is_type(headers, "application/json")
}

/// Whether the body was offered as exactly `want`, parameters aside. The point
/// is never the parsing — it is that naming a type outside the three a form can
/// send (`text/plain`, `multipart/form-data`, `application/x-www-form-urlencoded`)
/// puts the request in the class a browser will not send cross-origin without a
/// preflight, and this server answers no preflight.
fn is_type(headers: &[Header], want: &str) -> bool {
    headers.iter().any(|header| {
        header.field.equiv("Content-Type")
            && header
                .value
                .as_str()
                .split(';')
                .next()
                .is_some_and(|kind| kind.trim().eq_ignore_ascii_case(want))
    })
}

fn no_store() -> Header {
    header("Cache-Control", "no-store")
}

/// Headers every answer carries, whatever it is.
///
/// `Referrer-Policy` is the load-bearing one: the token travels in the query
/// string — it has to, because that is the only place a calendar app
/// subscribing to the feed can put it — and without this any request the page
/// makes to somewhere else would carry the whole link, token and all, in the
/// `Referer` header.
///
/// The policy is what a page that is one self-contained file can afford:
/// nothing loads from anywhere, so everything is denied and only `'self'` and
/// inline are allowed back. `'unsafe-inline'` for script is not a compromise
/// here but a description — the script *is* the page. `frame-ancestors 'none'`
/// keeps it out of somebody else's iframe, and `form-action 'none'` means a
/// injected form has nowhere to post to.
///
/// `worker-src 'self'` is not decoration: without it the service worker falls
/// back to `script-src` and is refused, which costs the offline shell and the
/// instant open that the whole page is shaped around. Found by loading the page
/// rather than by reading the policy.
///
/// `blob:` is in `img-src` for one reason, worth naming so nobody widens it
/// further by accident: choosing a background on the phone has to show the
/// picture before it is sent, and an object URL is the only way to display a
/// file the user just picked without pushing a twelve-megapixel photograph
/// through a base64 string. It admits nothing from the network — a blob is
/// bytes this page already holds.
/// Put the guards on a response. Every answer leaves through here or through
/// the loop at the end of `serve`; the long poll needs its own call because it
/// answers from three places that never reach that loop — and being the request
/// the phone makes most often, it is the worst one to leave bare.
fn guarded<R: std::io::Read>(response: Response<R>) -> Response<R> {
    let mut response = response;
    for guard in guards() {
        response.add_header(guard);
    }
    response
}

fn guards() -> [Header; 3] {
    [
        header("Referrer-Policy", "no-referrer"),
        header("X-Content-Type-Options", "nosniff"),
        header(
            "Content-Security-Policy",
            "default-src 'none'; script-src 'self' 'unsafe-inline'; worker-src 'self'; \
             style-src 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; \
             manifest-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
        ),
    ]
}

/// How long a phone may keep this path before asking again.
///
/// Everything used to be `no-store`, which is right for a board that changes
/// and wrong for the app's own icon: 660 KB of the 750 KB a cold open moved
/// was one PNG, re-fetched every single time, on mobile data, on a phone the
/// user had pulled out to check a time. The icon is the one thing here that is
/// not data — it is part of the program — so it gets a week, and a rebuilt one
/// arrives within a week rather than never.
///
/// The page and the service worker stay uncached at this layer on purpose:
/// the service worker caches the shell itself, and it knows how to replace it
/// safely, which a `max-age` does not.
fn caching_for(path: &str) -> Header {
    match path {
        "/icon.png" | "/icon-192.png" | "/icon-512.png" => header("Cache-Control", "public, max-age=604800"),
        // Immutable, because the name contains the content's own hash.
        p if p.starts_with("/bg-") && p.ends_with(".jpg") => header("Cache-Control", "private, max-age=31536000, immutable"),
        _ => no_store(),
    }
}

fn with_type(response: Body, content_type: &str) -> Body {
    response.with_header(header("Content-Type", content_type))
}

/// Smallest body worth compressing, in bytes.
///
/// Below about one packet there is nothing to save and something to lose: gzip
/// adds a header and a checksum, so a short error sentence comes out *longer*
/// than it went in, and every hop still pays to decode it.
const GZIP_FROM_BYTES: usize = 1400;

/// Whether this client can decode gzip.
///
/// Everything here is text — a 127 KB page, a 25 KB snapshot, an iCalendar
/// feed — served to a phone that is often on mobile data, and it compresses to
/// somewhere near a third. The decision lives here rather than at the two dozen
/// places a response is built, because it is a property of the transport and
/// not of any particular answer.
#[derive(Clone, Copy)]
pub struct Encoding {
    gzip: bool,
}

impl Encoding {
    /// Nothing negotiated: for bodies built where no request is in hand.
    pub const PLAIN: Encoding = Encoding { gzip: false };

    fn of(headers: &[Header]) -> Encoding {
        Encoding { gzip: headers.iter().any(|h| h.field.equiv("Accept-Encoding") && accepts_gzip(h.value.as_str())) }
    }

    /// One text body, compressed when that is worth doing.
    fn text(self, body: String, content_type: &str) -> Body {
        if self.gzip
            && body.len() >= GZIP_FROM_BYTES
            && let Some(packed) = gzipped(body.as_bytes(), Compression::fast())
        {
            return with_type(Response::from_data(packed), content_type).with_header(header("Content-Encoding", "gzip"));
        }
        with_type(Response::from_string(body), content_type)
    }

    fn json(self, value: serde_json::Value) -> Body {
        self.text(value.to_string(), "application/json; charset=utf-8")
    }

    /// A body that never changes, compressed once and kept.
    ///
    /// The page and the service worker are the program, not the data. Packing
    /// 127 KB on every request would be work done again for an identical
    /// answer, so it is done once and at the best setting rather than the
    /// fastest — this is the one body where the extra effort is free.
    fn fixed(self, body: &'static str, packed: &'static OnceLock<Option<Vec<u8>>>, content_type: &str) -> Body {
        if self.gzip
            && let Some(bytes) = packed.get_or_init(|| gzipped(body.as_bytes(), Compression::best()))
        {
            return with_type(Response::from_data(bytes.clone()), content_type)
                .with_header(header("Content-Encoding", "gzip"));
        }
        with_type(Response::from_string(body), content_type)
    }
}

/// Whether an `Accept-Encoding` value offers gzip. `gzip;q=0` is a refusal,
/// which is rare and cheap to honour.
fn accepts_gzip(value: &str) -> bool {
    value.split(',').any(|part| {
        let mut bits = part.split(';').map(str::trim);
        let name = bits.next().unwrap_or_default();
        if !name.eq_ignore_ascii_case("gzip") && name != "*" {
            return false;
        }
        !bits.any(|q| q.replace(' ', "").eq_ignore_ascii_case("q=0") || q.replace(' ', "").starts_with("q=0.0"))
    })
}

/// Gzip, or `None` if it did not help — a body that grows is not compressed.
fn gzipped(bytes: &[u8], level: Compression) -> Option<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::with_capacity(bytes.len() / 2), level);
    encoder.write_all(bytes).ok()?;
    let packed = encoder.finish().ok()?;
    (packed.len() < bytes.len()).then_some(packed)
}

fn json_error(status: u16, message: &str) -> Body {
    // Never compressed: an error is one sentence, and see `GZIP_FROM_BYTES`.
    let body = serde_json::json!({ "error": message }).to_string();
    with_type(Response::from_string(body), "application/json; charset=utf-8").with_status_code(status)
}

/// The icon, scaled to what a home screen asks for and kept.
///
/// The source is 882x882 and 660 KB, which is most of what a first install
/// moves — and Android wants 192 and 512, so it was paying for a picture it
/// immediately threw most of away. Scaled once, on the first request for each
/// size, because a resize on the request path would otherwise be paid every
/// time by the one client least able to afford it.
fn icon_at(side: u32) -> Vec<u8> {
    static SMALL: OnceLock<Vec<u8>> = OnceLock::new();
    static LARGE: OnceLock<Vec<u8>> = OnceLock::new();
    let kept = if side <= 192 { &SMALL } else { &LARGE };
    kept.get_or_init(|| scaled_icon(side).unwrap_or_else(|| ICON.to_vec())).clone()
}

/// `None` if the source will not decode or the result will not encode — the
/// caller then serves the original, which is large but correct.
fn scaled_icon(side: u32) -> Option<Vec<u8>> {
    use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
    use image::{ImageEncoder, imageops::FilterType};
    let source = image::load_from_memory(ICON).ok()?;
    let small = source.resize(side, side, FilterType::Lanczos3).into_rgba8();
    let mut out = Vec::new();
    // Best rather than default: this runs once for the life of the process and
    // the result is sent to a phone, so the trade only ever goes one way.
    PngEncoder::new_with_quality(&mut out, CompressionType::Best, PngFilter::Adaptive)
        .write_image(&small, small.width(), small.height(), image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(out)
}

/* ─────────────────────────── The picture behind ─────────────────────────── */

/// The size the picture is sent at — small, because it is blurred.
///
/// Blurring is done here and never in the browser: a `filter: blur()` on a
/// full-screen layer is GPU work on every frame, while a blurred JPEG costs the
/// phone exactly nothing beyond the decode. And once the high frequencies are
/// gone there is nothing left for resolution to carry, so the picture can be a
/// quarter the size it was: the browser magnifies it about four times to fill
/// the screen, and magnifying something already soft is invisible.
///
/// Decode cost scales with **megapixels, not bytes** — roughly 45 MP/s on a
/// desk machine, eight to twenty times slower on an old phone in battery saver.
/// The desktop's own 3000x2000 original is 6 MP and one to nearly three seconds
/// per cold open. This is 1.12 MP.
const BACKGROUND_TALL: u32 = 1560;
/// Aspect the phone is cropped to: tall enough to cover a phone in portrait.
const BACKGROUND_WIDE: u32 = 720;
/// How the two dials in the phone's Look sheet become the numbers below.
///
/// Blur is a **fraction of the picture's own width**, never a pixel radius: a
/// radius that reads well on a 540-pixel crop is invisible on a 3000-pixel
/// desktop picture, so one dial has to mean the same *look* at any size.
#[derive(Debug, Clone, Copy)]
pub struct LookDials {
    pub blur_percent: u32,
    pub light_percent: u32,
}

impl LookDials {
    /// Blur radius in pixels for a picture this wide. The scale is chosen so
    /// the default 11 lands near two pixels on screen at the size served:
    /// enough to settle the grain and let the eye fall on the text, not enough
    /// to stop it being a photograph.
    ///
    /// Applied here and never as a CSS `filter`, which would be GPU work on
    /// every frame; and it pays for itself twice, because blur removes exactly
    /// the high frequencies a codec spends most of its bytes on.
    pub fn blur_for(&self, width: u32) -> f32 {
        width as f32 * self.blur_percent.min(100) as f32 * 0.0002
    }
    /// The brightest the picture is allowed to get, as relative luminance at
    /// the 99.9th percentile. The divisor puts the default 39 at 0.13, which is
    /// where the smallest thing that ever sits on bare picture — 14px in
    /// `--text #e8e6e1` — still has 4.5:1 against it. Turning the dial up past
    /// that is the reader's own call, not a bug.
    pub fn ceiling(&self) -> f32 {
        (self.light_percent.min(100) as f32 / 300.0).max(0.01)
    }
}

/// JPEG quality. Low, deliberately: this is a darkened backdrop, and the
/// difference between 55 and 80 is invisible under a tint and costs a third
/// more bytes on a link that is often mobile data.
const BACKGROUND_QUALITY: u8 = 55;
/// AVIF quality and encoder speed. Speed is the encoder's own scale where 10 is
/// fastest and worst; 4 is a compromise that keeps a one-off startup encode
/// under a second on a laptop while giving up almost nothing. Quality is not
/// the same scale as JPEG's — 70 here is visually well above JPEG 70.
const AVIF_QUALITY: u8 = 82;
const AVIF_SPEED: u8 = 4;
// Worth knowing before anyone tunes these: encoding one background at 540x1170
// takes about 0.45s in a release build and nearly nine seconds in a debug one.
// rav1e is a different program without optimisation, and the phone's Save tap
// is answered by the release binary.

fn to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
fn to_srgb(v: f32) -> u8 {
    let c = v.clamp(0.0, 1.0);
    let s = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

/// The picture, ready to send: cropped to a phone's shape, scaled, blurred,
/// darkened, and encoded once in both codecs.
pub struct Backdrop {
    /// Content hash, so the URL can be immutable and cached for a year — the
    /// only way a picture this size is not re-fetched on every single open.
    pub id: String,
    /// AVIF, when the encoder managed it. Measured on the real picture, AVIF is
    /// 38% smaller than JPEG on a sharp image and 55% smaller on a softened
    /// one — which is what buys back the resolution a small JPEG had to give
    /// up. Firefox for Android has read it since 93.
    pub avif: Option<Vec<u8>>,
    /// JPEG, for anything that does not offer to take AVIF. Not a nicety: the
    /// negotiation is on the request's own `Accept`, so a client that says
    /// nothing still gets a picture.
    pub jpeg: Vec<u8>,
    /// The average colour of its top strip, for the browser's theme colour.
    pub top: String,
}

/// Read, crop, scale, darken, encode. `None` for anything unreadable — the
/// phone then has no picture and every rule falls back to the flat ground.
///
/// `dials` are the two the phone owns: how soft the picture is and how light it
/// may get. They live in the config, so the answer survives a restart.
pub fn prepare_backdrop(path: &Path, dials: LookDials) -> Option<Backdrop> {
    let bytes = std::fs::read(path).ok()?;
    prepare_backdrop_bytes(&bytes, dials)
}

/// Most an uploaded picture may weigh. The phone crops before it sends, so what
/// arrives is a few hundred kilobytes; this is the wall, not the expectation,
/// and it stays under `DRAIN_LIMIT` so an oversized one is refused politely
/// rather than dropped.
pub const MAX_UPLOAD_BYTES: usize = 6 * 1024 * 1024;
/// Most pixels a decoder is allowed to allocate for an upload. A small file can
/// declare an enormous canvas, and this is bytes from outside the process.
const MAX_UPLOAD_PIXELS: u64 = 40 * 1_000_000;

/// The same, from bytes that came off the wire.
pub fn prepare_backdrop_bytes(bytes: &[u8], dials: LookDials) -> Option<Backdrop> {
    use image::imageops::FilterType;
    if bytes.is_empty() || bytes.len() > MAX_UPLOAD_BYTES {
        return None;
    }
    // Bounded before it is read, not after: a few hundred bytes of header can
    // ask a decoder for gigabytes, and this is a file somebody sent us.
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(20_000);
    limits.max_image_height = Some(20_000);
    limits.max_alloc = Some(MAX_UPLOAD_PIXELS * 4);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    reader.limits(limits);
    let source = reader.decode().ok()?;

    // Crop to the phone's shape from the centre, then scale. Cropping first
    // means the scale never has to magnify, and a landscape desktop picture
    // would otherwise be blown up 1.28x to cover a portrait screen and show a
    // third of itself.
    let (w, h) = (source.width(), source.height());
    let want = BACKGROUND_WIDE as f32 / BACKGROUND_TALL as f32;
    let (cw, ch) = if (w as f32 / h as f32) > want {
        (((h as f32) * want).round() as u32, h)
    } else {
        (w, ((w as f32) / want).round() as u32)
    };
    let cropped = source.crop_imm((w.saturating_sub(cw)) / 2, (h.saturating_sub(ch)) / 2, cw.max(1), ch.max(1));
    let small = cropped.resize_exact(BACKGROUND_WIDE, BACKGROUND_TALL, FilterType::Lanczos3);
    // Blurred before the tone-map, so the darkening solves against what will
    // actually be on screen: a blur moves the bright pixels around, and
    // measuring the peak before it would aim at a picture that no longer exists.
    // Guarded, because `image::imageops::blur` reads a sigma of exactly 0.0 as
    // "you did not mean that" and substitutes 0.8 (image-0.25.10,
    // imageops/sample.rs:1039). Handed the dial straight through, 0 would come
    // out blurrier than 1 through 5 — a control that reverses at the end of its
    // travel, which is worse than one that does nothing.
    let sigma = dials.blur_for(BACKGROUND_WIDE);
    let small = if sigma >= 0.1 {
        image::DynamicImage::ImageRgb8(image::imageops::blur(&small.into_rgb8(), sigma))
    } else {
        small
    };

    // Darken — and *solve* for how much rather than guessing it.
    //
    // The rule the whole thing hangs on: nothing on the picture may be bright
    // enough to swallow the smallest text that sits on it. Expressed as a
    // ceiling on the 99.9th percentile of relative luminance, because one blown
    // pixel is where a room name goes to die.
    //
    // Two steps, both in linear light. First a highlight roll-off, which leaves
    // shadows almost untouched (for small values it is the identity) and
    // crushes the top end, so the picture keeps its shape instead of turning to
    // mud. Then one scale factor. Luminance is a linear combination of linear
    // channels, so scaling them scales the luminance exactly — which means the
    // right factor is arithmetic, not a search.
    let source_pixels = small.into_rgb8();
    let shaped: Vec<[f32; 3]> = source_pixels
        .pixels()
        .map(|p| {
            let mut out = [0f32; 3];
            for channel in 0..3 {
                let lin = to_linear(p[channel]);
                out[channel] = lin / (1.0 + 4.0 * lin);
            }
            out
        })
        .collect();
    let mut luminances: Vec<f32> = shaped.iter().map(|c| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]).collect();
    luminances.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let brightest = luminances.get((luminances.len() as f32 * 0.999) as usize).copied().unwrap_or(1.0);
    // Only ever a darkening. The phone's type is 11 to 14px where a wall
    // calendar's is a heading, so a picture that is already dark enough is left
    // where it is rather than lifted to meet the ceiling.
    let ceiling = dials.ceiling();
    let mut scale = (ceiling / brightest.max(1e-6)).min(1.0);

    // JPEG ringing pushes highlights back up, so the answer is checked against
    // what actually comes out of the encoder and corrected. It converges in a
    // couple of passes; the cap is there so a pathological picture cannot spin.
    let mut bytes = Vec::new();
    let mut buf = source_pixels.clone();
    for _ in 0..4 {
        for (pixel, want) in buf.pixels_mut().zip(shaped.iter()) {
            for channel in 0..3 {
                pixel[channel] = to_srgb(want[channel] * scale);
            }
        }
        bytes.clear();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, BACKGROUND_QUALITY)
            .encode(buf.as_raw(), BACKGROUND_WIDE, BACKGROUND_TALL, image::ExtendedColorType::Rgb8)
            .ok()?;
        let back = image::load_from_memory(&bytes).ok()?.into_rgb8();
        let mut got: Vec<f32> = back
            .pixels()
            .map(|p| 0.2126 * to_linear(p[0]) + 0.7152 * to_linear(p[1]) + 0.0722 * to_linear(p[2]))
            .collect();
        got.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let peak = got.get((got.len() as f32 * 0.999) as usize).copied().unwrap_or(1.0);
        if peak <= ceiling {
            break;
        }
        scale *= (ceiling / peak) * 0.99;
    }

    // The average of the top eighth, for the browser's own theme colour, so the
    // status bar does not sit on a strip of a different shade.
    let mut top = [0u64; 3];
    let mut counted = 0u64;
    for (_, y, pixel) in buf.enumerate_pixels() {
        if y < BACKGROUND_TALL / 8 {
            for channel in 0..3 { top[channel] += pixel[channel] as u64; }
            counted += 1;
        }
    }
    let avg = |c: usize| top[c].checked_div(counted).unwrap_or(0) as u8;

    // The same settled pixels again in AVIF. Encoded once, at startup, so its
    // slowness costs a moment of boot and never a request; `None` if the
    // encoder refuses, and then everything falls through to the JPEG.
    let avif = {
        use image::ImageEncoder as _;
        let mut out = Vec::new();
        image::codecs::avif::AvifEncoder::new_with_speed_quality(&mut out, AVIF_SPEED, AVIF_QUALITY)
            .write_image(buf.as_raw(), BACKGROUND_WIDE, BACKGROUND_TALL, image::ExtendedColorType::Rgb8)
            .ok()
            .map(|()| out)
            .filter(|made| !made.is_empty())
    };

    // The name is hashed from the JPEG alone, on purpose: both codecs carry the
    // same picture, and a client that changes which one it accepts must not be
    // sent to a different URL for the same image.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in &bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(Backdrop {
        id: format!("{hash:016x}"),
        avif,
        jpeg: bytes,
        top: format!("#{:02x}{:02x}{:02x}", avg(0), avg(1), avg(2)),
    })
}

/// The prepared picture. Replaceable, because the phone can now send a new one.
///
/// Held here rather than passed through every request because preparing it
/// costs a decode, a blur, a tone-map and two encodes — work that must never
/// land on a request. An `Arc` so a request can take a reference and let go of
/// the lock before it starts writing bytes down a slow link.
static BACKDROP: Mutex<Option<Arc<Backdrop>>> = Mutex::new(None);
/// Where an uploaded picture is kept, what the desk's own picture is, and the
/// tint to prepare either with. Set once at startup by whoever owns the board,
/// because only they know the data dir.
static BACKDROP_HOME: OnceLock<BackdropHome> = OnceLock::new();

struct BackdropHome {
    data_dir: std::path::PathBuf,
    /// Where the dials are kept, so a change from the phone survives a restart.
    config_file: std::path::PathBuf,
    /// The desk's own picture, which is what "use the desk's" goes back to.
    /// `None` when the desk has none set, and then clearing leaves no picture.
    desk: Option<std::path::PathBuf>,
}

/// The two dials, turned from the phone and kept in the config file. These are
/// only what stands before the config is read; `set_backdrop_home` replaces
/// them at startup with what the file says.
static LOOK: Mutex<LookDials> = Mutex::new(LookDials {
    blur_percent: crate::initialization::BACKGROUND_BLUR_DEFAULT,
    light_percent: crate::initialization::BACKGROUND_LIGHT_DEFAULT,
});

pub fn look_dials() -> LookDials {
    *LOOK.lock().unwrap_or_else(|held| held.into_inner())
}
pub fn set_look_dials(dials: LookDials) {
    *LOOK.lock().unwrap_or_else(|held| held.into_inner()) = dials;
}
/// The file an uploaded crop is kept in, beside the board's own files. The
/// bytes are the phone's crop exactly as it sent them, not the prepared
/// picture, so a change to the pipeline is re-derived rather than baked in.
pub const BACKGROUND_FILE: &str = "phone_background.img";

pub fn set_backdrop(made: Option<Backdrop>) {
    *BACKDROP.lock().unwrap_or_else(|held| held.into_inner()) = made.map(Arc::new);
}
pub fn backdrop() -> Option<Arc<Backdrop>> {
    BACKDROP.lock().unwrap_or_else(|held| held.into_inner()).clone()
}
/// Tell the server where an uploaded picture lives, what to fall back to, and
/// how dark to make either.
pub fn set_backdrop_home(
    data_dir: std::path::PathBuf,
    config_file: std::path::PathBuf,
    desk: Option<std::path::PathBuf>,
    dials: LookDials,
) {
    set_look_dials(dials);
    let _ = BACKDROP_HOME.set(BackdropHome { data_dir, config_file, desk });
}

/// Re-make the picture with whatever the dials now say, from the crop the phone
/// sent if there is one and the desk's own if not.
pub fn remake_backdrop() -> Option<String> {
    let home = BACKDROP_HOME.get()?;
    let uploaded = home.data_dir.join(BACKGROUND_FILE);
    let source = if uploaded.exists() { Some(uploaded) } else { home.desk.clone() };
    let made = source.and_then(|picture| prepare_backdrop(&picture, look_dials()));
    // A remake that failed is not a reason to take the picture away. The source
    // can be unreadable for a moment — a file being replaced, an encoder
    // refusing — and answering that by clearing a backdrop the phone is already
    // showing turns a dial into a delete. Keep what is up and say nothing new.
    match made {
        Some(ready) => {
            let id = ready.id.clone();
            set_backdrop(Some(ready));
            Some(id)
        }
        None => backdrop().map(|kept| kept.id.clone()),
    }
}

/// Take a picture the phone sent: prepare it, write the crop down so a restart
/// still has it, and put it in front of the one that was there.
///
/// The crop is stored **as received**, not as prepared: the blur, the tone-map
/// and the two encodes are derived, and a change to any of them should be
/// re-derived on the next start rather than baked into a file forever.
///
/// An empty body clears it, and the desk's own picture comes back.
fn adopt_background(body: &[u8]) -> Result<Option<String>, String> {
    let Some(home) = BACKDROP_HOME.get() else {
        return Err("This copy has nowhere to keep a picture.".to_string());
    };
    if body.is_empty() {
        // Not "no picture": back to whatever the desk is showing, which is what
        // the button offering this says.
        let _ = std::fs::remove_file(home.data_dir.join(BACKGROUND_FILE));
        let back = home.desk.as_ref().and_then(|picture| prepare_backdrop(picture, look_dials()));
        let id = back.as_ref().map(|made| made.id.clone());
        set_backdrop(back);
        return Ok(id);
    }
    let made = prepare_backdrop_bytes(body, look_dials())
        .ok_or_else(|| "That file is not a picture this can read.".to_string())?;
    // Written the way every other file here is: a temp file, fsynced, renamed.
    // A half-written background would be read as a broken one at the next start.
    write_bytes_atomically(&home.data_dir, BACKGROUND_FILE, body)
        .map_err(|error| format!("Could not keep the picture: {error}"))?;
    let id = made.id.clone();
    set_backdrop(Some(made));
    Ok(Some(id))
}

#[derive(serde::Deserialize)]
struct LookDialsWire {
    blur: u32,
    light: u32,
}

/// Write the dials into the config file, so they survive a restart and the desk
/// reads the same numbers. `false` if it could not be written — the picture
/// still changes for this run, which is the honest half of the outcome.
fn remember_dials(blur: u32, light: u32) -> bool {
    let Some(home) = BACKDROP_HOME.get() else { return false };
    let file = home.config_file.clone();
    crate::initialization::write_config_value(&file, "background_blur_percent", blur as i64).is_ok()
        && crate::initialization::write_config_value(&file, "background_light_percent", light as i64).is_ok()
}

fn write_bytes_atomically(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(dir)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(bytes)?;
    temp.as_file_mut().sync_all()?;
    temp.persist(dir.join(name))?;
    crate::tasks::sync_directory(dir);
    Ok(())
}

/// The web-app manifest, so "Add to Home Screen" opens the view like an app.
/// The start URL keeps the token when the page asked with one, because iOS
/// gives a home-screen app storage of its own and would otherwise open it
/// with nothing to show its token.
fn manifest(token: Option<&str>) -> String {
    let start_url = match token {
        Some(token) if !token.is_empty() => format!("/?token={}", encode_token(token)),
        _ => "/".to_string(),
    };
    serde_json::json!({
        "name": "TaskDeck",
        "short_name": "TaskDeck",
        "start_url": start_url,
        "scope": "/",
        "display": "standalone",
        "background_color": "#0f1113",
        "theme_color": "#0f1113",
        "icons": [
            { "src": "/icon-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any" },
            { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any" }
        ]
    })
    .to_string()
}

/* ─────────────────────────── Where to point the phone ─────────────────────────── */

/// Addresses this machine is reachable on, best guess first.
///
/// Without enumerating interfaces (which needs platform code or a crate), the
/// trick is to ask the kernel which local address it *would* use to reach a
/// destination: connecting a UDP socket sends nothing but resolves the route.
/// A private-range destination yields the LAN interface; Tailscale's
/// `100.100.100.100` yields the tailnet address when one is up, and the LAN
/// address again when it is not — so duplicates are dropped.
/// Whether this is a Tailscale address — the `100.64.0.0/10` range Tailscale
/// hands out (RFC 6598, carrier-grade NAT space).
///
/// Worth telling apart from an ordinary LAN address because the two are not
/// equally useful: a tailnet address answers from the sofa, from a train and
/// from the office, while a LAN address answers only from this network. When
/// there is one of each, the tailnet one is the link to give somebody.
pub fn is_tailnet(address: &str) -> bool {
    match address.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            let [a, b, ..] = ip.octets();
            a == 100 && (64..128).contains(&b)
        }
        _ => false,
    }
}

/// Every address this machine can be reached on, **the most reachable first**.
///
/// The order is the whole point rather than a tidiness: whatever comes first
/// is what the QR code encodes and what a person scans. A phone away from the
/// house cannot reach `192.168.x.x`, so a QR carrying it does not fail — it
/// spins, forever, which is worse than failing.
pub fn local_addresses() -> Vec<String> {
    let mut out = raw_local_addresses();
    // A stable sort, so two addresses of the same kind keep the order the
    // probes found them in.
    out.sort_by_key(|address| !is_tailnet(address));
    out
}

fn raw_local_addresses() -> Vec<String> {
    let mut out = Vec::new();
    for probe in ["10.255.255.255:1", "100.100.100.100:1"] {
        let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else { continue };
        if socket.connect(probe).is_err() {
            continue;
        }
        let Ok(address) = socket.local_addr() else { continue };
        let ip = address.ip();
        if ip.is_loopback() || ip.is_unspecified() {
            continue;
        }
        let text = ip.to_string();
        if !out.contains(&text) {
            out.push(text);
        }
    }
    out
}

/// The addresses to show links for, given what the server binds: every
/// address this machine has when it listens everywhere, or the one address
/// it listens on — a link to any other would not answer.
pub fn addresses_for(bind: &str) -> Vec<String> {
    match bind.trim().parse::<IpAddr>() {
        Ok(ip) if !ip.is_unspecified() => vec![ip.to_string()],
        _ => local_addresses(),
    }
}

/// The link to open on the phone.
pub fn page_url(address: &str, port: u16, token: &str) -> String {
    format!("http://{}:{port}/?token={}", host_for_url(address), encode_token(token))
}

/// The page, at an address somebody else owns — `phone_public_url`.
///
/// Taken as given rather than rebuilt: whatever is in front of the server
/// decided the scheme, the host and the port, and guessing at any of them is
/// how a link that looks right stops working.
pub fn public_page_url(base: &str, token: &str) -> String {
    format!("{}/?token={}", base.trim_end_matches('/'), encode_token(token))
}

/// The same for the feed.
pub fn public_feed_url(base: &str, token: &str) -> String {
    format!("{}/calendar.ics?token={}", base.trim_end_matches('/'), encode_token(token))
}

/// The calendar-feed link to subscribe to.
pub fn feed_url(address: &str, port: u16, token: &str) -> String {
    format!("http://{}:{port}/calendar.ics?token={}", host_for_url(address), encode_token(token))
}

/// An address as a URL host: an IPv6 address goes in brackets.
pub fn host_for_url(address: &str) -> String {
    if address.contains(':') && !address.starts_with('[') {
        format!("[{address}]")
    } else {
        address.to_string()
    }
}

/// A token as a URL query value: RFC 3986 unreserved characters pass, the
/// rest is `%XX`. A generated token is all unreserved and comes out as it
/// went in; this is for a key someone typed into `userconfig.toml` by hand.
pub fn encode_token(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    for byte in token.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Open a link in the desktop's browser — Settings' "Open here", for a look at
/// the phone view without reaching for the phone. The platform's own opener,
/// so nothing new is linked in.
pub fn open_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("cmd");
        // `start` treats its first quoted argument as a window title.
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open a browser:\n{error}"))
}

/// The QR code for a link, as a square of dark/light modules: `(width, cells)`.
pub fn qr_modules(text: &str) -> Option<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    let width = code.width();
    let cells = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
    Some((width, cells))
}

/// Four light modules on every side. RFC-mandated, and the reason a QR with no
/// margin will not scan: the decoder isolates the finder patterns by the light
/// ground around them, and in a terminal that ground is whatever colour the
/// theme happens to be.
const QR_QUIET: usize = 4;
/// The extremes of the xterm-256 cube. Not the basic eight: a theme is free to
/// decide its own "black" is #073642, and Solarized does — these two are fixed.
const QR_INK: u8 = 16;
const QR_PAPER: u8 = 231;

/// The phone link as a QR code a terminal can print and a camera can read.
///
/// `None` when the text will not encode, which for a link means never; the
/// caller prints nothing extra rather than unwrapping.
///
/// Two things here are not obvious and are the whole reason this is not three
/// lines of the `qrcode` crate's own renderer:
///
/// **Polarity.** A QR is dark-on-light by specification, and the ZXing family
/// that Android's scanners come from declines to decode an inverted one rather
/// than spend half its time looking. The crate's `Dense1x2` default paints a
/// *dark* module as a printed glyph, which takes the terminal's foreground
/// colour — correct on a light terminal and inverted on a dark one, and a
/// server is usually read over ssh on a dark one. So the colours are written
/// out explicitly, and the code carries its own contrast whatever the theme.
///
/// **The glyph.** Only `▄` and a space, never `█` or `▀`: several terminal
/// fonts leave a hairline gap above a full or upper block, which stacks into
/// stripes through the code and breaks the scan. A lower half block with the
/// colours swapped draws the same pixels with no gap — the trick `qr2term`
/// documents.
pub fn qr_text(text: &str) -> Option<String> {
    let (width, cells) = qr_modules(text)?;
    let side = width + QR_QUIET * 2;
    // A light module outside the code, a real one inside it.
    let dark_at = |x: usize, y: usize| -> bool {
        let (Some(x), Some(y)) = (x.checked_sub(QR_QUIET), y.checked_sub(QR_QUIET)) else {
            return false;
        };
        if x >= width || y >= width {
            return false;
        }
        cells.get(y * width + x).copied().unwrap_or(false)
    };
    let paint = |dark: bool| if dark { QR_INK } else { QR_PAPER };

    let mut out = String::new();
    // Two module rows to a text row: a terminal cell is about twice as tall as
    // it is wide, so this is what makes the modules square.
    for pair in (0..side).step_by(2) {
        for x in 0..side {
            let top = dark_at(x, pair);
            // An odd number of rows leaves the last half light, which is quiet
            // zone either way.
            let bottom = dark_at(x, pair + 1);
            if top == bottom {
                out.push_str(&format!("\x1b[48;5;{}m ", paint(top)));
            } else {
                out.push_str(&format!("\x1b[38;5;{}m\x1b[48;5;{}m▄", paint(bottom), paint(top)));
            }
        }
        out.push_str("\x1b[0m\n");
    }
    Some(out)
}

/* ─────────────────────────────── Snapshots ─────────────────────────────── */

/// Everything the page needs to draw a day and its tray, in one answer.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    /// Bumped on every save; the page uses it to tell "nothing changed" from
    /// "redraw".
    pub version: u64,
    pub today: NaiveDate,
    /// Minutes from midnight, for the now-line.
    pub now_minutes: i32,
    /// The selected colour scheme, RGBA. All-transparent (COLORSCHEME ZERO)
    /// tells the page to use its own tints.
    pub palette: [[u8; 4]; 6],
    pub labels: Labels,
    pub days: Vec<DaySnapshot>,
    pub tray: Tray,
    /// Full details of every live item, so the sheet that opens on a tap has
    /// what it needs without another round trip.
    pub items: Vec<ItemDetail>,
    /// Every task, in the order the desktop's task list shows them — by
    /// score, most pressing first (§7). Ids into `items`.
    pub ranked: Vec<u64>,
    /// The most recent things to leave the board, newest first.
    pub done: Vec<DoneRow>,
    /// The desktop notepad, as it stands.
    pub notes: String,
    /// The span the subscribed calendars were actually read for, both ends
    /// inclusive. Absent when nothing is subscribed: a board with no calendars
    /// has no ignorance to declare.
    ///
    /// Without this a day past the window is byte-identical to a free day, and
    /// the phone would answer "nothing on this day" to a question it has not
    /// looked into. With it the page can say *nothing of yours* instead, which
    /// is the difference between a wrong answer and an honest one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub known: Option<subscriptions::Covered>,
    /// The picture behind the page, when the desk has one set. Absent means
    /// there is none, and every rule on the phone falls back to a flat ground.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub look: Option<Look>,
    pub dials: Dials,
}

/// What the phone needs to fetch and place the picture: where it is, how tall
/// it is relative to its width, and what colour its top strip averages, so the
/// browser's own status bar does not sit on a different shade.
#[derive(Debug, Clone, Serialize)]
pub struct Look {
    pub id: String,
    pub aspect: f32,
    pub top: String,
}

/// Where the two dials stand, so the sliders open on the truth rather than on a
/// default that may be nothing like what the file says. Always sent.
#[derive(Debug, Clone, Serialize)]
pub struct Dials {
    pub blur: u32,
    pub light: u32,
}

/// One recent archive row, with what the ledger says about it.
#[derive(Debug, Clone, Serialize)]
pub struct DoneRow {
    pub id: u64,
    /// With `id`, the archive's key for this row — what `Command::Restore`
    /// hands back. Serialised in full so it round-trips exactly.
    pub archived_at: DateTime<Local>,
    pub name: String,
    pub kind: &'static str,
    pub color: usize,
    pub finished: bool,
    /// "Fri 4 Sep 16:20".
    pub when: String,
    /// The ledger's verdict line: `finished 2d early · 2h estimated, 3h over 2 sittings · 12d on the board`.
    pub verdict: String,
}

pub fn done_rows(archived: &[Archived]) -> Vec<DoneRow> {
    archived
        .iter()
        .take(DONE_ROWS_MAX)
        .map(|row| DoneRow {
            id: row.id,
            archived_at: row.archived_at,
            name: row.name.clone(),
            kind: kind_name(row.is_event, row.is_routine()),
            color: row.color_id(),
            finished: row.was_finished(),
            when: row.archived_at.format("%a %-d %b %H:%M").to_string(),
            verdict: row.verdict(),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Labels {
    pub severity: [&'static str; 5],
    pub horizon: [&'static str; 4],
    /// Presentation order for `horizon`: soonest first, the parking lot last.
    pub horizon_order: [u8; 4],
    pub weekdays: [&'static str; 7],
}

impl Labels {
    pub fn current() -> Self {
        Self {
            severity: IMPORTANCE_LABELS,
            horizon: HORIZON_LABELS,
            horizon_order: tasks::HORIZON_DISPLAY_ORDER,
            weekdays: WEEKDAY_NAMES,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DaySnapshot {
    pub date: NaiveDate,
    pub weekday: String,
    /// "4 September 2026".
    pub label: String,
    pub is_today: bool,
    /// The masthead's day figure: `2h 30m planned · 3 blocks · 9h routine`.
    pub summary: String,
    /// Minutes of booked work already behind the now-line — what Reflow
    /// answers. Zero on any day but today.
    pub behind_minutes: i32,
    pub entries: Vec<EntrySnapshot>,
    pub ghosts: Vec<GhostSnapshot>,
    /// What a subscribed calendar says is on this day (§23). Drawn beside the
    /// day's own blocks and never tappable: nothing here can be edited from
    /// the phone, because nothing here belongs to this board.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subscribed: Vec<SubscribedSnapshot>,
}

/// One event off a subscribed calendar, as the page draws it.
#[derive(Debug, Clone, Serialize)]
pub struct SubscribedSnapshot {
    pub start: i32,
    pub end: i32,
    /// Which of `columns` side-by-side slots this one takes. Laid out here so
    /// the page draws and never decides, exactly as the day's own entries are
    /// — and so two meetings that overlap are both readable instead of one
    /// being printed on top of the other.
    pub column: usize,
    pub columns: usize,
    pub all_day: bool,
    /// A span that describes the day rather than taking an hour out of it: an
    /// all-day band, or something long enough to amount to one (§23). Drawn
    /// full width behind the rest and left out of the column packing, so a
    /// real lecture beside a twelve-hour course-period marker still gets the
    /// width it needs.
    pub background: bool,
    /// Where it is, on its own line. A course feed puts the room here in a
    /// dozen characters while the summary runs past a hundred (§23).
    #[serde(skip_serializing_if = "String::is_empty")]
    #[serde(rename = "where")]
    pub where_: String,
    /// A long press says this; nothing paints it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
    pub name: String,
    /// `#rrggbb`, the subscription's own colour rather than the scheme's: it
    /// says which calendar, not how urgent.
    pub color: String,
    /// Which calendar, for the label under a tap-free block.
    pub calendar: String,
}

/// One thing on a day's timeline — the page's copy of `PlannerEntry`, laid
/// out into its lane already so the page draws and never decides.
#[derive(Debug, Clone, Serialize)]
pub struct EntrySnapshot {
    pub id: u64,
    pub session: Option<usize>,
    pub name: String,
    pub kind: &'static str,
    pub color: usize,
    pub start: i32,
    pub minutes: u32,
    /// A point rather than a span: an event with no length, or a due time.
    pub marker: bool,
    /// The marker is a task's due time.
    pub due: bool,
    pub column: usize,
    pub columns: usize,
}

/// A block of a day that has already been accounted for (§17.4).
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

#[derive(Debug, Clone, Serialize)]
pub struct Tray {
    pub due: Vec<TrayCard>,
    pub later: Vec<TrayCard>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrayCard {
    pub id: u64,
    pub name: String,
    pub color: usize,
    pub deadline: Option<String>,
    /// "in 2h" / "3d" / "overdue".
    pub due_in: Option<String>,
    pub duration: Option<u32>,
    pub planned: u32,
    /// What a drop lands: the unplanned remainder, or the default.
    pub drop_minutes: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ItemDetail {
    pub id: u64,
    pub name: String,
    pub kind: &'static str,
    pub color: usize,
    /// Naive local `YYYY-MM-DDTHH:MM`, the shape a `datetime-local` input takes.
    pub deadline: Option<String>,
    pub duration: Option<u32>,
    pub planned: u32,
    pub remaining: Option<u32>,
    pub severity: Option<u8>,
    pub horizon: Option<u8>,
    pub repeat: Option<RepeatDetail>,
    pub sessions: Vec<SessionDetail>,
    pub created: NaiveDate,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepeatDetail {
    pub days: u8,
    pub summary: String,
    pub start: i32,
    pub minutes: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionDetail {
    pub index: usize,
    pub day: NaiveDate,
    pub start: i32,
    pub minutes: u32,
}

/// The wire name of what a thing is, from the two flags that decide it — for
/// live items and archive rows alike.
fn kind_name(is_event: bool, is_routine: bool) -> &'static str {
    if is_routine {
        "routine"
    } else if is_event {
        "event"
    } else {
        "task"
    }
}

fn kind_of(item: &Active) -> &'static str {
    kind_name(item.is_event, item.is_routine())
}

/// One day's timeline, laid out.
///
/// Live entries and archive ghosts go through `lay_out` together, exactly as
/// the desktop does (§17.4): an hour already spent behaves like an hour that
/// is booked, so a new block lands beside finished work rather than on top of
/// it.
/// Lay the subscribed calendars over the days that were just built.
///
/// Done here rather than inside `build_day` so that function keeps taking only
/// the board's own things — it is the one the tests drive, and a subscribed
/// calendar is not part of what it is testing.
fn with_subscribed(
    mut days: Vec<DaySnapshot>,
    subscriptions: &[crate::subscriptions::Subscription],
    overlay: &crate::subscriptions::Overlay,
) -> Vec<DaySnapshot> {
    for day in &mut days {
        let showing: Vec<(&crate::subscriptions::OverlayEvent, &crate::subscriptions::Subscription)> = overlay
            .events_on(day.date)
            .filter_map(|event| {
                // A calendar switched off is not drawn. The overlay is usually
                // cleared of it already; this is the belt to that brace, and
                // costs one lookup per event.
                let subscription = subscriptions.iter().find(|s| s.id == event.subscription)?;
                subscription.enabled.then_some((event, subscription))
            })
            .collect();

        // Laid out among themselves, the way the day's own entries are laid
        // out among theirs: two meetings at the same hour split the width
        // rather than one being drawn over the other. All-day bands take no
        // part — they are chips above the timeline, not blocks on it.
        // Only the ones that are really appointments take part in the column
        // packing; a background span is drawn behind them at full width.
        let placements: Vec<planner::Placement> = showing
            .iter()
            .map(|(event, _)| planner::Placement::Block {
                start: event.start,
                minutes: if event.is_background() { 0 } else { event.minutes().max(1) as u32 },
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

pub fn build_day(items: &[Active], archived: &[Archived], day: NaiveDate, now: DateTime<Local>) -> DaySnapshot {
    struct Live<'a> {
        item: &'a Active,
        placed: planner::DayPlacement,
    }
    let mut live: Vec<Live> = Vec::new();
    for item in items {
        for placed in planner::placements_for(item, day) {
            live.push(Live { item, placed });
        }
    }
    live.sort_by_key(|entry| (entry.placed.placement.start(), entry.item.id, entry.placed.session));

    struct Ghost<'a> {
        row: &'a Archived,
        placement: planner::Placement,
    }
    let mut ghosts: Vec<Ghost> = Vec::new();
    for row in archived {
        let placeable = planner::Placeable {
            is_event: row.is_event,
            deadline: None,
            duration_minutes: None,
            sessions: &row.sessions,
            recurrence: None,
        };
        for placed in planner::placements_of(placeable, day) {
            ghosts.push(Ghost { row, placement: placed.placement });
        }
    }
    ghosts.sort_by_key(|ghost| ghost.placement.start());

    let placements: Vec<planner::Placement> = live
        .iter()
        .map(|entry| entry.placed.placement)
        .chain(ghosts.iter().map(|ghost| ghost.placement))
        .collect();
    let lanes = planner::lay_out(&placements);
    let (live_lanes, ghost_lanes) = lanes.split_at(live.len());

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
                kind: kind_of(entry.item),
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

    // The masthead's figure, counted the way the desktop counts it (§18.2):
    // routines apart from planned work, and finished work apart from both.
    let (routine, planned): (Vec<_>, Vec<_>) = live.iter().partition(|entry| entry.item.is_routine());
    let planned_summary = planner::summarize(&planned.iter().map(|e| e.placed.placement).collect::<Vec<_>>());
    let routine_summary = planner::summarize(&routine.iter().map(|e| e.placed.placement).collect::<Vec<_>>());
    let done = planner::summarize(&ghosts.iter().map(|g| g.placement).collect::<Vec<_>>());
    let summary = planner::summary_text(planned_summary, routine_summary, done);

    // Only work blocks count as behind: an event that has passed happened,
    // and a routine that has passed was never owed.
    let behind_minutes = planner::now_marker(day, now)
        .map(|now_minutes| {
            let work: Vec<_> = planned
                .iter()
                .filter(|entry| !entry.item.is_event && entry.placed.session.is_some())
                .map(|entry| entry.placed.placement)
                .collect();
            planner::behind_minutes(&work, now_minutes)
        })
        .unwrap_or(0);

    DaySnapshot {
        date: day,
        weekday: day.format("%A").to_string(),
        label: day.format("%-d %B %Y").to_string(),
        is_today: day == now.date_naive(),
        summary,
        behind_minutes,
        entries,
        ghosts: ghost_snapshots,
        // Filled by `with_subscribed`: this function is the board's own things.
        subscribed: Vec::new(),
    }
}

/// The tray for `day`: everything still wanting a slot, most pressing first,
/// split into owed-by-this-day and the rest (§16.3.1).
pub fn build_tray(items: &[Active], day: NaiveDate, now: DateTime<Local>, shuffle_seed: u64) -> Tray {
    let mut wanting: Vec<(f32, &Active)> = items
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
            deadline: item.deadline.map(local_input),
            due_in: item.deadline.map(|deadline| planner::relative_due(deadline, now)),
            duration: item.duration_minutes,
            planned: item.planned_minutes(),
            drop_minutes: planner::drop_length_for(item),
        };
        match planner::backlog_group(item.deadline, day) {
            planner::BacklogGroup::Due => due.push(card),
            planner::BacklogGroup::Later => later.push(card),
        }
    }
    Tray { due, later }
}

pub fn item_detail(item: &Active) -> ItemDetail {
    ItemDetail {
        id: item.id,
        name: item.name.clone(),
        kind: kind_of(item),
        color: item.calendar_item_color(),
        deadline: item.deadline.map(local_input),
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
            .sessions
            .iter()
            .enumerate()
            .map(|(index, session)| SessionDetail {
                index,
                day: session.start.date_naive(),
                start: session.start.hour() as i32 * 60 + session.start.minute() as i32,
                minutes: session.minutes,
            })
            .collect(),
        created: item.created.date_naive(),
    }
}

/* ─────────────────────────────── The feed ─────────────────────────────── */

/// The live set as an iCalendar feed.
///
/// Four layers, each tagged with a `CATEGORIES` so a calendar app can tell
/// them apart: events as themselves; a task's deadline as a short `⚑` event
/// at the due time; each session as a `⏱` block; and routines as weekly
/// recurring events. Instants are written in UTC. A routine is written as a
/// *floating* local time with an `RRULE`, which is exactly what its rule is —
/// 23:00 means 23:00 on every day it lands on, clocks changing or not (§18.3).
///
/// Nothing is written back from here: the feed is read-only by construction,
/// which is what lets it be subscribed to from anywhere without a conflict
/// story.
pub fn calendar_feed(items: &[Active], now: DateTime<Local>) -> String {
    let stamp = now.with_timezone(&Utc).format("%Y%m%dT%H%M%SZ").to_string();
    let mut out = String::new();
    out.push_str("BEGIN:VCALENDAR\r\n");
    out.push_str("VERSION:2.0\r\n");
    out.push_str("PRODID:-//TaskDeck//Phone view//EN\r\n");
    out.push_str("CALSCALE:GREGORIAN\r\n");
    out.push_str("METHOD:PUBLISH\r\n");
    out.push_str("X-WR-CALNAME:TaskDeck\r\n");

    for item in items {
        if let Some(rule) = item.recurrence {
            let start = rule.start_minutes.clamp(0, planner::DAY_MINUTES - 1);
            let end = (start + rule.minutes.max(planner::MIN_BLOCK_MINUTES) as i32).min(planner::DAY_MINUTES);
            let first_day = if rule.repeats() {
                // The first day on or after today the rule falls on, so a
                // rule made months ago does not spell out months of history.
                (0..7)
                    .map(|offset| now.date_naive() + ChronoDuration::days(offset))
                    .find(|day| rule.falls_on(*day))
                    .unwrap_or(rule.anchor)
            } else {
                rule.anchor
            };
            let mut lines = vec![
                "BEGIN:VEVENT".to_string(),
                format!("UID:taskdeck-{}-routine@taskdeck", item.id),
                format!("DTSTAMP:{stamp}"),
                format!("DTSTART:{}", floating(first_day, start)),
                format!("DTEND:{}", floating(first_day, end)),
                format!("SUMMARY:{}", escape_text(&format!("↻ {}", item.name))),
                "CATEGORIES:TaskDeck routine".to_string(),
            ];
            if rule.repeats() {
                let by_day: Vec<&str> = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"]
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| rule.includes(*index as u32))
                    .map(|(_, code)| *code)
                    .collect();
                lines.push(format!("RRULE:FREQ=WEEKLY;BYDAY={}", by_day.join(",")));
            }
            lines.push("TRANSP:TRANSPARENT".to_string());
            lines.push("END:VEVENT".to_string());
            push_component(&mut out, &lines);
            continue;
        }

        if item.is_event {
            let Some(at) = item.deadline else { continue };
            let minutes = item.duration_minutes.unwrap_or(planner::SNAP_MINUTES as u32).max(1);
            let lines = [
                "BEGIN:VEVENT".to_string(),
                format!("UID:taskdeck-{}-event@taskdeck", item.id),
                format!("DTSTAMP:{stamp}"),
                format!("DTSTART:{}", utc(at)),
                format!("DTEND:{}", utc(at + ChronoDuration::minutes(minutes as i64))),
                format!("SUMMARY:{}", escape_text(&item.name)),
                "CATEGORIES:TaskDeck event".to_string(),
                "END:VEVENT".to_string(),
            ];
            push_component(&mut out, &lines);
            continue;
        }

        if let Some(due) = item.deadline {
            let lines = [
                "BEGIN:VEVENT".to_string(),
                format!("UID:taskdeck-{}-due@taskdeck", item.id),
                format!("DTSTAMP:{stamp}"),
                format!("DTSTART:{}", utc(due)),
                format!("DTEND:{}", utc(due + ChronoDuration::minutes(planner::SNAP_MINUTES as i64))),
                format!("SUMMARY:{}", escape_text(&format!("⚑ due: {}", item.name))),
                "CATEGORIES:TaskDeck due".to_string(),
                "TRANSP:TRANSPARENT".to_string(),
                "END:VEVENT".to_string(),
            ];
            push_component(&mut out, &lines);
        }

        for (index, session) in item.sessions.iter().enumerate() {
            let lines = [
                "BEGIN:VEVENT".to_string(),
                format!("UID:taskdeck-{}-session-{index}@taskdeck", item.id),
                format!("DTSTAMP:{stamp}"),
                format!("DTSTART:{}", utc(session.start)),
                format!(
                    "DTEND:{}",
                    utc(session.start + ChronoDuration::minutes(session.minutes.max(1) as i64))
                ),
                format!("SUMMARY:{}", escape_text(&format!("⏱ {}", item.name))),
                "CATEGORIES:TaskDeck plan".to_string(),
                "END:VEVENT".to_string(),
            ];
            push_component(&mut out, &lines);
        }
    }

    out.push_str("END:VCALENDAR\r\n");
    out
}

fn utc(at: DateTime<Local>) -> String {
    at.with_timezone(&Utc).format("%Y%m%dT%H%M%SZ").to_string()
}

/// A floating local date-time (no zone), for rules that mean "this wall-clock
/// time wherever the calendar is".
fn floating(day: NaiveDate, minutes: i32) -> String {
    let minutes = minutes.clamp(0, planner::DAY_MINUTES);
    // Midnight at the end of the day is the next day's 00:00 in this format.
    if minutes >= planner::DAY_MINUTES {
        return format!("{}T000000", (day + ChronoDuration::days(1)).format("%Y%m%d"));
    }
    format!("{}T{:02}{:02}00", day.format("%Y%m%d"), minutes / 60, minutes % 60)
}

/// RFC 5545 §3.3.11: backslash, semicolon and comma are escaped, and a line
/// break becomes a literal `\n`.
pub fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            other => out.push(other),
        }
    }
    out
}

/// Write the lines of one component, each folded to 75 octets (RFC 5545 §3.1)
/// and terminated with CRLF.
fn push_component(out: &mut String, lines: &[String]) {
    for line in lines {
        fold_line(out, line);
    }
}

fn fold_line(out: &mut String, line: &str) {
    const LIMIT: usize = 75;
    let mut width = 0;
    let mut first = true;
    for ch in line.chars() {
        let bytes = ch.len_utf8();
        // A continuation line starts with a space that counts against its own
        // 75 octets, so it holds one fewer.
        let room = if first { LIMIT } else { LIMIT - 1 };
        if width + bytes > room {
            out.push_str("\r\n ");
            width = 0;
            first = false;
        }
        out.push(ch);
        width += bytes;
    }
    out.push_str("\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder as _;
    use crate::tasks::{Recurrence, Session};
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, hour, minute, 0).unwrap()
    }

    fn task(id: u64, name: &str) -> Active {
        Active {
            id,
            importance: None,
            time_importance: Some(1),
            name: name.to_string(),
            created: at(2026, 9, 1, 9, 0),
            deadline: None,
            is_event: false,
            sessions: Vec::new(),
            planned_start: None,
            duration_minutes: None,
            recurrence: None,
        }
    }

    #[test]
    fn the_snapshot_declares_how_far_the_calendars_were_read_only_when_there_are_any() {
        use crate::subscriptions::{Covered, Overlay};
        // A scratch directory, never `"."`. A board writes its files beside
        // itself, so a test that says "here" makes the repository the board's
        // data directory — which is how a `subscriptions.json` came to be
        // committed at the root of this checkout. It held `example.invalid`
        // and nothing real, and the next one might not: these files hold
        // calendar URLs, and a calendar URL is a password.
        let home = tempfile::tempdir().expect("a scratch directory");
        let mut board = Board::from_parts(Vec::new(), String::new(), home.path().to_path_buf());
        let now = at(2026, 9, 5, 12, 0);
        let from = now.date_naive();

        // Nothing subscribed: no window, because there is nothing to be
        // ignorant about, and a day with nothing on it really is free.
        let bare = snapshot(&mut board, [[0; 4]; 6], Some(Vec::new()), from, 1, now);
        assert_eq!(bare.known, None);

        let covers = Covered { from: from - ChronoDuration::days(60), until: from + ChronoDuration::days(400) };
        board.adopt_overlay(Overlay::sealed(Vec::new(), Vec::new()).covering(Some(covers)));
        board
            .apply(Command::AddSubscription {
                name: "Timetable".into(),
                url: "https://example.invalid/a.ics".into(),
                color: None,
            }, now)
            .expect("the address is well formed");
        let with = snapshot(&mut board, [[0; 4]; 6], Some(Vec::new()), from, 1, now);
        assert_eq!(with.known, Some(covers));

        // Switched off is the same as absent: nothing was read, so nothing is
        // claimed to have been.
        let id = board.subscriptions().first().map(|s| s.id).expect("one subscription");
        board.apply(Command::SetSubscriptionEnabled { id, enabled: false }, now).expect("switches off");
        let off = snapshot(&mut board, [[0; 4]; 6], Some(Vec::new()), from, 1, now);
        assert_eq!(off.known, None);
    }

    #[test]
    fn a_request_for_more_days_than_the_server_builds_is_cut_down_quietly() {
        // The page is expected to count what came back rather than trust what
        // it asked for, so this must stay a 200 with fewer days — never a 400.
        // A scratch directory, never `"."`. A board writes its files beside
        // itself, so a test that says "here" makes the repository the board's
        // data directory — which is how a `subscriptions.json` came to be
        // committed at the root of this checkout. It held `example.invalid`
        // and nothing real, and the next one might not: these files hold
        // calendar URLs, and a calendar URL is a password.
        let home = tempfile::tempdir().expect("a scratch directory");
        let mut board = Board::from_parts(Vec::new(), String::new(), home.path().to_path_buf());
        let now = at(2026, 9, 5, 12, 0);
        let from = now.date_naive();
        for asked in [MAX_SNAPSHOT_DAYS + 1, 100, 400, u32::MAX] {
            let snap = snapshot(&mut board, [[0; 4]; 6], Some(Vec::new()), from, asked, now);
            assert_eq!(snap.days.len(), MAX_SNAPSHOT_DAYS as usize, "asked for {asked}");
        }
        let exact = snapshot(&mut board, [[0; 4]; 6], Some(Vec::new()), from, MAX_SNAPSHOT_DAYS, now);
        assert_eq!(exact.days.len(), MAX_SNAPSHOT_DAYS as usize);
        assert_eq!(exact.days.last().map(|d| d.date), Some(from + ChronoDuration::days(30)));
    }

    #[test]
    fn a_listener_waiting_on_the_version_the_board_is_at_is_not_answered_at_once() {
        // A pulse that starts at zero while the board's version starts at the
        // clock answers every single wait immediately, with a number no
        // snapshot carries — and the page refetches and asks again as fast as
        // the link allows. `PhoneServer::start` takes the board's version for
        // exactly this reason; here is the property that made it a parameter.
        let seeded = 1_788_639_208_669_u64;
        let pulse = Pulse::new();
        pulse.publish(seeded);
        assert_eq!(pulse.current(), seeded);

        // Fresh, it would have said zero — which is the bug, stated.
        assert_eq!(Pulse::new().current(), 0);
    }

    #[test]
    fn gzip_is_offered_only_when_the_client_says_it_can_take_it() {
        assert!(accepts_gzip("gzip"));
        assert!(accepts_gzip("gzip, deflate, br"));
        assert!(accepts_gzip("deflate, gzip;q=1.0, *;q=0.5"));
        assert!(accepts_gzip("*"), "a client that takes anything takes this");
        assert!(!accepts_gzip("deflate, br"));
        assert!(!accepts_gzip(""));
        // A refusal, spelled either way round.
        assert!(!accepts_gzip("gzip;q=0"));
        assert!(!accepts_gzip("gzip; q=0.0"));
    }

    #[test]
    fn a_body_too_small_to_be_worth_packing_is_sent_as_it_is() {
        // Gzip adds a header and a checksum, so a short sentence comes out
        // longer than it went in and every hop still pays to decode it.
        let short = "x".repeat(GZIP_FROM_BYTES - 1);
        assert!(gzipped(short.as_bytes(), Compression::fast()).is_some(), "it does compress; it is just not worth it");
        // Something already packed does not pack again, and is left alone.
        let noise: Vec<u8> = (0..4096u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        if let Some(packed) = gzipped(&noise, Compression::fast()) {
            assert!(packed.len() < noise.len(), "never returned when it would grow");
        }
    }

    #[test]
    fn the_page_and_a_snapshot_both_shrink_by_more_than_half() {
        // The two bodies a phone actually pays for on every open.
        let page = gzipped(PAGE.as_bytes(), Compression::best()).expect("the page compresses");
        assert!(page.len() * 2 < PAGE.len(), "{} -> {}", PAGE.len(), page.len());
        let snapshot = serde_json::json!({
            "days": (0..31).map(|d| serde_json::json!({
                "date": format!("2026-09-{:02}", (d % 28) + 1),
                "summary": "nothing on this day",
                "entries": [], "ghosts": [], "subscribed": [],
            })).collect::<Vec<_>>()
        })
        .to_string();
        let packed = gzipped(snapshot.as_bytes(), Compression::fast()).expect("json compresses");
        assert!(packed.len() * 2 < snapshot.len(), "{} -> {}", snapshot.len(), packed.len());
    }

    #[test]
    fn the_home_screen_is_offered_the_sizes_it_asks_for() {
        let text = manifest(None);
        assert!(text.contains("/icon-192.png") && text.contains("192x192"));
        assert!(text.contains("/icon-512.png") && text.contains("512x512"));
        // Scaled, and much smaller than the 882x882 original they come from.
        let small = icon_at(192);
        let large = icon_at(512);
        assert!(small.len() < ICON.len() / 4, "192 is {} of {}", small.len(), ICON.len());
        assert!(large.len() < ICON.len(), "512 is {} of {}", large.len(), ICON.len());
        // Real PNGs, at the sizes claimed.
        for (bytes, side) in [(&small, 192u32), (&large, 512)] {
            let decoded = image::load_from_memory(bytes).expect("a png");
            assert_eq!((decoded.width(), decoded.height()), (side, side));
        }
        // Asked for twice, the same bytes come back rather than a second scale.
        assert_eq!(icon_at(192).len(), small.len());
    }

    #[test]
    fn a_command_must_be_offered_as_json() {
        // The one thing stopping a web page the phone visits from writing to
        // the board with a leaked token: a cross-origin form or `fetch` can
        // send text/plain or a form encoding with nobody's permission, but
        // asking for JSON puts the request in the class that needs a preflight,
        // and this server answers none.
        assert!(is_json(&headers(&[("Content-Type", "application/json")])));
        assert!(is_json(&headers(&[("content-type", "application/json; charset=utf-8")])));
        assert!(is_json(&headers(&[("Content-Type", " APPLICATION/JSON ")])));
        for simple in ["text/plain", "text/plain;charset=UTF-8", "multipart/form-data", "application/x-www-form-urlencoded", "application/json-patch+json"] {
            assert!(!is_json(&headers(&[("Content-Type", simple)])), "{simple}");
        }
        assert!(!is_json(&headers(&[])), "a body offered as nothing is not JSON");
    }

    /// The three the long poll answers from, which is where they went missing:
    /// `guards()` holding the right headers says nothing about whether any
    /// response carries them, and this test used to check only the former while
    /// being named for the latter.
    #[test]
    fn the_long_polls_own_answers_carry_the_guards_too() {
        let wanted = ["referrer-policy", "x-content-type-options", "content-security-policy"];
        for (what, response) in [
            ("refusal", json_error(401, "no")),
            ("timeout", Encoding::PLAIN.json(serde_json::json!({ "version": 1u64 }))),
            ("wakeup", Encoding::PLAIN.json(serde_json::json!({ "version": 2u64 }))),
        ] {
            let got: Vec<String> = guarded(response.with_header(no_store()))
                .headers()
                .iter()
                .map(|h| h.field.as_str().as_str().to_ascii_lowercase())
                .collect();
            for name in wanted {
                assert!(got.contains(&name.to_string()), "the {what} answer is missing {name}: {got:?}");
            }
        }
    }

    #[test]
    fn every_answer_carries_the_guards() {
        // `Referrer-Policy` is the load-bearing one: the token is in the query
        // string, so without it any request out of the page would hand the
        // whole link to somewhere else in the `Referer` header.
        let names: Vec<String> = guards().iter().map(|h| h.field.as_str().as_str().to_ascii_lowercase()).collect();
        assert!(names.contains(&"referrer-policy".to_string()));
        assert!(names.contains(&"x-content-type-options".to_string()));
        assert!(names.contains(&"content-security-policy".to_string()));
        let policy = guards()[2].value.as_str().to_string();
        assert!(policy.contains("default-src 'none'"), "{policy}");
        assert!(policy.contains("frame-ancestors 'none'"), "{policy}");
        assert!(policy.contains("form-action 'none'"), "{policy}");
        // The service worker is refused without this — `worker-src` falls back
        // to `script-src`, and the offline shell goes with it.
        assert!(policy.contains("worker-src 'self'"), "{policy}");
    }

    #[test]
    fn a_fresh_install_listens_on_this_machine_only() {
        // The setting that decides who can reach any of this. Two connections
        // that declare a body and never send it hold both workers for as long
        // as they stay open, and `tiny_http` 0.12 has no read timeout to break
        // that — so the bind is the whole defence, and it starts closed.
        assert_eq!(DEFAULT_BIND, "127.0.0.1");
        assert_eq!(addresses_for(DEFAULT_BIND), vec!["127.0.0.1".to_string()]);
    }

    #[test]
    #[ignore = "a measurement, not an assertion: cargo test -- --ignored --nocapture codec_bake_off"]
    fn codec_bake_off() {
        use image::imageops::FilterType;
        let Ok(reader) = image::ImageReader::open("images/pexels-francesco-ungaro-1525041.jpg") else {
            return; // No picture in this checkout; nothing to measure.
        };
        let source = reader.decode().expect("decode");
        println!("\n  size        blur   jpeg-q70    avif-q70   avif saves");
        for (w, h) in [(360u32, 780u32), (462, 1000), (540, 1170), (640, 1386)] {
            for blur in [0.0f32, 0.8, 1.6] {
                let cropped = source.crop_imm(750, 0, 1500, 2000);
                let small = cropped.resize_exact(w, h, FilterType::Lanczos3).into_rgb8();
                let small = if blur > 0.0 { image::imageops::blur(&small, blur) } else { small };
                let mut jpg = Vec::new();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 70)
                    .encode(small.as_raw(), w, h, image::ExtendedColorType::Rgb8).expect("jpeg");
                let mut avif = Vec::new();
                image::codecs::avif::AvifEncoder::new_with_speed_quality(&mut avif, 4, 70)
                    .write_image(&small, w, h, image::ExtendedColorType::Rgb8).expect("avif");
                println!(
                    "  {w:>4}x{h:<6} {blur:>4.1}  {:>7.1} KB {:>9.1} KB {:>9.0}%",
                    jpg.len() as f32 / 1024.0,
                    avif.len() as f32 / 1024.0,
                    100.0 - (avif.len() as f32 / jpg.len() as f32 * 100.0),
                );
            }
        }
    }

    /// Relative luminance at the 99.9th percentile — the brightest the picture
    /// gets once one-in-a-thousand outliers are set aside.
    fn brightest(picture: &image::DynamicImage) -> f32 {
        let mut ys: Vec<f32> = picture.to_rgb8().pixels().map(|p| {
            0.2126 * to_linear(p[0]) + 0.7152 * to_linear(p[1]) + 0.0722 * to_linear(p[2])
        }).collect();
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ys[(ys.len() as f32 * 0.999) as usize]
    }

    /// Mean absolute difference between neighbouring pixels: high on a sharp
    /// picture, low on a blurred one. The cheapest honest measure of softness.
    fn detail(picture: &image::DynamicImage) -> f32 {
        let rgb = picture.to_rgb8();
        let (w, h) = rgb.dimensions();
        let raw = rgb.as_raw();
        let mut sum = 0.0f64;
        for y in 0..h as usize {
            let row = y * w as usize * 3;
            for i in 3..w as usize * 3 {
                sum += (raw[row + i] as f64 - raw[row + i - 3] as f64).abs();
            }
        }
        (sum / (h as f64 * (w - 1) as f64 * 3.0)) as f32
    }

    /// The dial-to-number mapping, which is the whole contract the phone sees.
    #[test]
    fn the_dials_map_to_numbers_that_mean_the_same_look_at_any_size() {
        let soft = LookDials { blur_percent: 40, light_percent: 39 };
        let sharp = LookDials { blur_percent: 0, light_percent: 39 };
        assert_eq!(sharp.blur_for(720), 0.0, "blur 0 is no blur, not a little blur");
        // Blur is a fraction of width, so the same dial is the same look on a
        // phone crop and on a desktop picture five times its size.
        assert!((soft.blur_for(3600) / soft.blur_for(720) - 5.0).abs() < 1e-4);

        // Brightness is a ceiling, so up means lighter, and the bottom of the
        // dial still leaves a picture rather than a black rectangle.
        let dim = LookDials { blur_percent: 11, light_percent: 15 };
        let bright = LookDials { blur_percent: 11, light_percent: 80 };
        assert!(bright.ceiling() > dim.ceiling() * 2.0);
        assert!(dim.ceiling() >= 0.01);
        // A dial that arrives out of range is clamped, never wrapped.
        assert_eq!(LookDials { blur_percent: 900, light_percent: 900 }.ceiling(),
                   LookDials { blur_percent: 100, light_percent: 100 }.ceiling());
    }

    /// The bottom of the blur dial, which is a trap rather than a bug in our own
    /// arithmetic: `image::imageops::blur` reads a sigma of exactly 0.0 as a
    /// mistake and substitutes 0.8 (image-0.25.10, imageops/sample.rs:1039), so
    /// a dial handed straight through comes out *blurrier* at 0 than at 5. The
    /// guard is in the pipeline; this is what would notice it being removed.
    #[test]
    fn the_blur_dial_does_not_reverse_at_the_bottom_of_its_travel() {
        let source = std::path::Path::new("images/pexels-francesco-ungaro-1525041.jpg");
        if !source.exists() {
            return; // No picture in this checkout; nothing to assert.
        }
        let at = |blur| {
            let made = prepare_backdrop(source, LookDials { blur_percent: blur, light_percent: 39 })
                .expect("the picture is readable");
            detail(&image::load_from_memory(&made.jpeg).expect("a jpeg comes out"))
        };
        // Sharpest at 0, and never sharper as the dial goes up.
        let ladder: Vec<f32> = [0, 3, 8].iter().map(|p| at(*p)).collect();
        assert!(
            ladder[0] >= ladder[1] && ladder[1] >= ladder[2],
            "detail must fall as the dial rises, got {ladder:?}"
        );
    }

    /// Blur is the one dial whose effect nothing else in the suite would catch:
    /// wire it to zero and every other assertion still passes. So it is checked
    /// where it actually lands, on the pixels that get sent.
    #[test]
    fn turning_the_blur_dial_up_softens_the_picture_that_is_sent() {
        let source = std::path::Path::new("images/pexels-francesco-ungaro-1525041.jpg");
        if !source.exists() {
            return; // No picture in this checkout; nothing to assert.
        }
        let at = |blur| {
            let made = prepare_backdrop(source, LookDials { blur_percent: blur, light_percent: 39 })
                .expect("the picture is readable");
            image::load_from_memory(&made.jpeg).expect("a jpeg comes out")
        };
        let sharp = detail(&at(0));
        let soft = detail(&at(45));
        assert!(soft < sharp * 0.6, "blur 45 left {soft} detail against {sharp} at blur 0");
    }

    #[test]
    fn a_backdrop_is_cropped_scaled_and_darkened_enough_to_put_text_on() {
        // Runs the real pipeline over the repository's own picture, which is
        // 3000x2000 and 927 KB — six megapixels, which on the target phone is
        // one to nearly three seconds of decode per cold open.
        let source = std::path::Path::new("images/pexels-francesco-ungaro-1525041.jpg");
        if !source.exists() {
            return; // No picture in this checkout; nothing to assert.
        }
        let dials = LookDials {
            blur_percent: crate::initialization::BACKGROUND_BLUR_DEFAULT,
            light_percent: crate::initialization::BACKGROUND_LIGHT_DEFAULT,
        };
        let made = prepare_backdrop(source, dials).expect("the picture is readable");
        let decoded = image::load_from_memory(&made.jpeg).expect("a jpeg comes out");
        assert_eq!((decoded.width(), decoded.height()), (BACKGROUND_WIDE, BACKGROUND_TALL));
        let sent = made.avif.as_ref().unwrap_or(&made.jpeg);
        assert!(sent.len() < 90_000, "{} bytes is too much to send", sent.len());
        assert_eq!(made.id.len(), 16, "the id is a content hash, so the URL can be immutable");

        // The point of the darkening: nothing bright enough to swallow text
        // should survive. Checked at the 99.9th percentile, because one blown
        // pixel is where a room name goes to die.
        let p999 = brightest(&decoded);
        let ceiling = dials.ceiling();
        assert!(
            p999 <= ceiling * 1.25,
            "brightest pixels are {p999}, over the {ceiling} the dial asked for"
        );
    }

    fn headers(pairs: &[(&str, &str)]) -> Vec<Header> {
        pairs.iter().map(|(k, v)| header(k, v)).collect()
    }

    #[test]
    fn urls_split_and_queries_decode() {
        assert_eq!(split_url("/api/state?from=2026-09-04&days=1"), ("/api/state", "from=2026-09-04&days=1"));
        assert_eq!(split_url("/"), ("/", ""));
        let query = parse_query("token=abc&name=hello+there%21&empty=");
        assert_eq!(query.lookup("token").map(String::as_str), Some("abc"));
        assert_eq!(query.lookup("name").map(String::as_str), Some("hello there!"));
        assert_eq!(query.lookup("empty").map(String::as_str), Some(""));
        assert_eq!(query.lookup("missing"), None);
    }

    #[test]
    fn percent_decoding_copes_with_junk() {
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("%C3%A4"), "ä");
        // `%` before a multibyte character is junk, not a panic.
        assert_eq!(percent_decode("%€"), "%€");
        assert_eq!(percent_decode("%aä"), "%aä");
    }

    #[test]
    fn authorisation_accepts_header_bearer_and_query_and_nothing_else() {
        let token = "abcdefghijk";
        assert!(is_authorised(&headers(&[("X-TaskDeck-Token", token)]), &[], token));
        assert!(is_authorised(&headers(&[("x-taskdeck-token", token)]), &[], token));
        assert!(is_authorised(&headers(&[("Authorization", "Bearer abcdefghijk")]), &[], token));
        // A stale bearer in front of the right header does not hide it.
        assert!(is_authorised(&headers(&[("Authorization", "Bearer stale"), ("X-TaskDeck-Token", token)]), &[], token));
        assert!(is_authorised(&[], &parse_query("token=abcdefghijk"), token));
        assert!(!is_authorised(&headers(&[("X-TaskDeck-Token", "abcdefghijX")]), &[], token));
        assert!(!is_authorised(&headers(&[("X-TaskDeck-Token", "abc")]), &[], token));
        assert!(!is_authorised(&[], &[], token));
        // An empty configured token opens no doors, not every door.
        assert!(!is_authorised(&[], &parse_query("token="), ""));
    }

    #[test]
    fn tokens_are_long_and_unalike() {
        let a = generate_token();
        let b = generate_token();
        assert_eq!(a.len(), TOKEN_LENGTH);
        assert_ne!(a, b);
        assert!(a.bytes().all(|byte| TOKEN_ALPHABET.contains(&byte)));
    }

    #[test]
    fn commands_parse_from_the_page_shapes() {
        let command: Command =
            serde_json::from_str(r#"{"op":"move_block","id":4,"session":1,"day":"2026-09-04","start":540,"minutes":90}"#)
                .unwrap();
        assert_eq!(
            command,
            Command::MoveBlock {
                id: 4,
                session: Some(1),
                day: NaiveDate::from_ymd_opt(2026, 9, 4).unwrap(),
                start: 540,
                minutes: 90
            }
        );
        let command: Command = serde_json::from_str(r#"{"op":"set_deadline","id":4,"deadline":null}"#).unwrap();
        assert_eq!(command, Command::SetDeadline { id: 4, deadline: None });
        let command: Command = serde_json::from_str(r#"{"op":"quick_add","name":"buy milk"}"#).unwrap();
        assert_eq!(command, Command::QuickAdd { name: "buy milk".to_string(), deadline: None });
        let command: Command =
            serde_json::from_str(r#"{"op":"create","kind":"routine","name":"sleep","day":"2026-09-04","start":1380,"minutes":60}"#)
                .unwrap();
        assert!(matches!(command, Command::Create { kind: Kind::Routine, .. }));
        assert!(serde_json::from_str::<Command>(r#"{"op":"explode"}"#).is_err());
    }

    #[test]
    fn snapshot_query_defaults_and_validates() {
        let command = snapshot_command(&parse_query("from=2026-09-04&days=3")).unwrap();
        assert_eq!(command, Command::Snapshot { from: NaiveDate::from_ymd_opt(2026, 9, 4).unwrap(), days: 3 });
        let command = snapshot_command(&[]).unwrap();
        assert!(matches!(command, Command::Snapshot { days: 1, .. }));
        assert_eq!(snapshot_command(&parse_query("from=yesterday")).unwrap_err().status, 400);
        assert_eq!(snapshot_command(&parse_query("days=lots")).unwrap_err().status, 400);
    }

    #[test]
    fn datetime_local_round_trips() {
        let when = at(2026, 9, 4, 17, 30);
        assert_eq!(local_input(when), "2026-09-04T17:30");
        assert_eq!(parse_local_input("2026-09-04T17:30"), Some(when));
        assert_eq!(parse_local_input("2026-09-04T17:30:00"), Some(when));
        assert_eq!(parse_local_input("Friday"), None);
    }

    #[test]
    fn a_day_lays_out_live_entries_and_ghosts_together() {
        let day = NaiveDate::from_ymd_opt(2026, 9, 4).unwrap();
        let mut planned = task(1, "report");
        planned.sessions.push(Session { start: at(2026, 9, 4, 9, 0), minutes: 60 });
        planned.deadline = Some(at(2026, 9, 4, 17, 0));
        let mut sleep = task(2, "sleep");
        sleep.recurrence = Some(Recurrence { days: tasks::EVERY_DAY, anchor: day, start_minutes: 23 * 60, minutes: 60 });

        let mut finished = task(3, "finished");
        finished.sessions.push(Session { start: at(2026, 9, 4, 9, 0), minutes: 60 });
        let archived = Archived::retire(finished, crate::archive::Outcome::Finished, at(2026, 9, 4, 10, 0));

        let snapshot = build_day(&[planned, sleep], &[archived], day, at(2026, 9, 4, 12, 0));
        assert!(snapshot.is_today);
        assert_eq!(snapshot.weekday, "Friday");
        assert_eq!(snapshot.label, "4 September 2026");
        // A block, a due marker, and a routine block.
        assert_eq!(snapshot.entries.len(), 3);
        let block = &snapshot.entries[0];
        assert_eq!((block.id, block.session, block.start, block.minutes, block.marker), (1, Some(0), 540, 60, false));
        // The ghost shares the nine o'clock hour, so both take a column.
        assert_eq!(block.columns, 2);
        assert_eq!(snapshot.ghosts.len(), 1);
        assert_eq!(snapshot.ghosts[0].columns, 2);
        assert!(snapshot.ghosts[0].finished);
        let due = snapshot.entries.iter().find(|e| e.due).unwrap();
        assert_eq!((due.start, due.marker), (17 * 60, true));
        let routine = snapshot.entries.iter().find(|e| e.kind == "routine").unwrap();
        assert_eq!(routine.start, 23 * 60);
        assert_eq!(snapshot.summary, "1h planned · 1 block · 1 due · 1h routine · 1h done");
        // The nine o'clock block is wholly behind a noon now-line.
        assert_eq!(snapshot.behind_minutes, 60);
    }

    #[test]
    fn behind_is_zero_on_any_day_but_today() {
        let day = NaiveDate::from_ymd_opt(2026, 9, 4).unwrap();
        let mut planned = task(1, "report");
        planned.sessions.push(Session { start: at(2026, 9, 4, 9, 0), minutes: 60 });
        let snapshot = build_day(&[planned], &[], day, at(2026, 9, 5, 12, 0));
        assert!(!snapshot.is_today);
        assert_eq!(snapshot.behind_minutes, 0);
        let command: Command = serde_json::from_str(r#"{"op":"reflow","day":"2026-09-04"}"#).unwrap();
        assert_eq!(command, Command::Reflow { day, from: None });
        let command: Command = serde_json::from_str(r#"{"op":"set_notes","text":"milk\neggs"}"#).unwrap();
        assert_eq!(command, Command::SetNotes { text: "milk\neggs".to_string() });
    }

    #[test]
    fn done_rows_round_trip_their_key_through_json() {
        let mut finished = task(3, "finished");
        finished.sessions.push(Session { start: at(2026, 9, 4, 9, 0), minutes: 60 });
        let archived = Archived::retire(finished, crate::archive::Outcome::Finished, at(2026, 9, 4, 10, 0));
        let rows = done_rows(std::slice::from_ref(&archived));
        assert_eq!(rows.len(), 1);
        assert!(rows[0].finished);
        assert_eq!(rows[0].kind, "task");
        assert_eq!(rows[0].when, "Fri 4 Sep 10:00");

        // The key the page hands back is the one the archive knows.
        let json = serde_json::to_value(&rows[0]).unwrap();
        let command: Command = serde_json::from_value(serde_json::json!({
            "op": "restore", "id": json["id"], "archived_at": json["archived_at"]
        }))
        .unwrap();
        match command {
            Command::Restore { id, archived_at } => {
                assert_eq!(crate::archive::ArchiveKey { id, archived_at }, archived.key());
            }
            other => panic!("parsed {other:?}"),
        }
    }

    #[test]
    fn the_tray_splits_by_the_shown_day() {
        let day = NaiveDate::from_ymd_opt(2026, 9, 4).unwrap();
        let now = at(2026, 9, 4, 12, 0);
        let mut owed = task(1, "owed");
        owed.deadline = Some(at(2026, 9, 3, 17, 0));
        owed.importance = Some(2);
        owed.duration_minutes = Some(120);
        owed.sessions.push(Session { start: at(2026, 9, 2, 9, 0), minutes: 60 });
        let later = task(2, "later");
        let mut event = task(3, "event");
        event.is_event = true;
        event.deadline = Some(now);

        let tray = build_tray(&[owed, later, event], day, now, 7);
        assert_eq!(tray.due.len(), 1);
        assert_eq!(tray.due[0].name, "owed");
        assert_eq!(tray.due[0].due_in.as_deref(), Some("overdue"));
        assert_eq!((tray.due[0].planned, tray.due[0].drop_minutes), (60, 60));
        assert_eq!(tray.later.len(), 1);
        assert_eq!(tray.later[0].name, "later");
        assert_eq!(tray.later[0].drop_minutes, planner::DEFAULT_BLOCK_MINUTES);
    }

    #[test]
    fn the_feed_writes_every_layer() {
        let day = NaiveDate::from_ymd_opt(2026, 9, 4).unwrap();
        let mut report = task(1, "report; final, really");
        report.deadline = Some(at(2026, 9, 5, 17, 0));
        report.sessions.push(Session { start: at(2026, 9, 4, 9, 0), minutes: 90 });
        let mut dentist = task(2, "dentist");
        dentist.is_event = true;
        dentist.deadline = Some(at(2026, 9, 6, 10, 0));
        dentist.duration_minutes = Some(45);
        let mut sleep = task(3, "sleep");
        sleep.recurrence = Some(Recurrence { days: 0b0001_1111, anchor: day, start_minutes: 23 * 60, minutes: 60 });
        let mut walk = task(4, "walk the dog");
        walk.recurrence = Some(Recurrence { days: 0, anchor: day, start_minutes: 14 * 60, minutes: 30 });

        let feed = calendar_feed(&[report, dentist, sleep, walk], at(2026, 9, 4, 8, 0));
        assert!(feed.starts_with("BEGIN:VCALENDAR\r\n"));
        assert!(feed.ends_with("END:VCALENDAR\r\n"));
        assert!(feed.contains("UID:taskdeck-1-due@taskdeck"));
        assert!(feed.contains("SUMMARY:⚑ due: report\\; final\\, really"));
        assert!(feed.contains("UID:taskdeck-1-session-0@taskdeck"));
        assert!(feed.contains("SUMMARY:⏱ report\\; final\\, really"));
        assert!(feed.contains("UID:taskdeck-2-event@taskdeck"));
        assert!(feed.contains("RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"));
        // Floating local time for the rule, in the same format the anchor day gives.
        assert!(feed.contains("DTSTART:20260904T230000\r\n"));
        assert!(feed.contains("DTEND:20260905T000000\r\n"));
        // The one-off routine has no rule and sits on its day.
        assert!(feed.contains("UID:taskdeck-4-routine@taskdeck"));
        assert!(feed.contains("DTSTART:20260904T140000\r\n"));
        assert_eq!(feed.matches("RRULE").count(), 1);
        // Every line, folded or not, is CRLF-terminated and within the limit.
        for line in feed.split("\r\n") {
            assert!(line.len() <= 75, "line too long: {line:?}");
            assert!(!line.contains('\n'));
        }
    }

    #[test]
    fn long_lines_fold_on_character_boundaries() {
        let mut out = String::new();
        let long = format!("SUMMARY:{}", "ä".repeat(60));
        fold_line(&mut out, &long);
        let unfolded = out.replace("\r\n ", "");
        assert_eq!(unfolded, format!("{long}\r\n"));
        for line in out.split("\r\n") {
            assert!(line.len() <= 75);
        }
    }

    #[test]
    fn a_repeated_request_key_gets_the_first_reply_back() {
        let replies = Replies::new();
        assert!(matches!(replies.claim("k1"), Claim::Ours));
        replies.settle("k1", serde_json::json!({ "ok": true, "id": 5 }));
        match replies.claim("k1") {
            Claim::Done(value) => assert_eq!(value["id"], 5),
            Claim::Ours | Claim::Busy => panic!("the reply was known"),
        }
        // A failed attempt releases the key, so a retry is carried out.
        assert!(matches!(replies.claim("k2"), Claim::Ours));
        replies.release("k2");
        assert!(matches!(replies.claim("k2"), Claim::Ours));
        // A timed-out attempt keeps the key busy until the board's late
        // answer arrives, and then that answer is the reply.
        assert!(matches!(replies.claim("k3"), Claim::Ours));
        let (late_tx, late_rx) = channel::<PhoneReply>();
        replies.park_late("k3", late_rx);
        assert!(matches!(replies.claim("k3"), Claim::Busy));
        late_tx.send(Ok(serde_json::json!({ "ok": true, "id": 7 }))).unwrap();
        match replies.claim("k3") {
            Claim::Done(value) => assert_eq!(value["id"], 7),
            _ => panic!("the late answer is the reply"),
        }
        // A late refusal frees the key for a fresh attempt.
        assert!(matches!(replies.claim("k4"), Claim::Ours));
        let (late_tx, late_rx) = channel::<PhoneReply>();
        replies.park_late("k4", late_rx);
        late_tx.send(Err(PhoneError::bad_request("no"))).unwrap();
        assert!(matches!(replies.claim("k4"), Claim::Ours));
        // No key, no memory.
        assert!(matches!(replies.claim(""), Claim::Ours));
        assert!(matches!(replies.claim(""), Claim::Ours));
        assert_eq!(request_key(&headers(&[("X-TaskDeck-Request", " abc ")])), "abc");
        assert_eq!(request_key(&[]), "");
    }

    #[test]
    fn links_bracket_ipv6_and_encode_the_token() {
        assert_eq!(page_url("fd7a::1", 7373, "k"), "http://[fd7a::1]:7373/?token=k");
        assert_eq!(feed_url("192.168.1.2", 7373, "a+b c&d"), "http://192.168.1.2:7373/calendar.ics?token=a%2Bb%20c%26d");
        assert!(manifest(Some("a+b")).contains(r#""start_url":"/?token=a%2Bb""#));
        let token = generate_token();
        assert_eq!(page_url("h", 1, &token), format!("http://h:1/?token={token}"));
        // A day off the calendar is refused before anything adds to it.
        let far = [("from".to_string(), "+262142-12-31".to_string())];
        assert_eq!(snapshot_command(&far).unwrap_err().status, 400);
        let fine = [("from".to_string(), "9999-12-31".to_string())];
        assert!(snapshot_command(&fine).is_ok());
    }

    #[test]
    fn links_follow_the_bind_and_a_bad_bind_is_refused_before_anything_listens() {
        assert_eq!(addresses_for("127.0.0.1"), vec!["127.0.0.1".to_string()]);
        assert_eq!(addresses_for(" 100.64.0.7 "), vec!["100.64.0.7".to_string()]);
        // Everywhere, or nonsense, means every address this machine has — and
        // the default is no longer that: a fresh install listens on loopback.
        assert_eq!(addresses_for("0.0.0.0"), local_addresses());
        assert_eq!(addresses_for(DEFAULT_BIND), vec!["127.0.0.1".to_string()]);
        assert_eq!(addresses_for("::"), local_addresses());
        assert_eq!(addresses_for("kitchen"), local_addresses());

        let (tx, _rx) = channel();
        let wake: Wake = Arc::new(|| {});
        let Err(error) = PhoneServer::start("kitchen", 0, "k".to_string(), tx, wake, Arc::new(Pulse::new()), 0) else { panic!("a bad bind must not listen") };
        assert!(error.contains("not an address"), "{error}");
    }

    #[test]
    fn the_body_cap_admits_the_longest_notes_the_board_does() {
        // Four-byte characters, the widest UTF-8 has, at the board's limit:
        // the wire form must fit under the cap, so the board's message — not
        // "too large" — is what a phone hears for notes one character over.
        let widest = "𝄞".repeat(crate::board::NOTES_MAX_CHARS);
        let body = serde_json::to_vec(&Command::SetNotes { text: widest }).unwrap();
        assert!(body.len() <= MAX_BODY_BYTES, "{} bytes over a cap of {}", body.len(), MAX_BODY_BYTES);
    }

    #[test]
    fn parked_requests_are_answered_when_due_and_only_then() {
        let now = Instant::now();
        let soon = now + Duration::from_millis(100);
        let later = now + Duration::from_secs(10);
        let mut parked: Parked<&str> = Parked::new();
        parked.push("stale", 1, later);
        parked.push("current-soon", 2, soon);
        parked.push("current-later", 2, later);
        assert_eq!(parked.len(), 3);
        assert_eq!(parked.next_deadline(), Some(soon));

        // Version 2 now: only the request that saw 1 is due.
        assert_eq!(parked.take_due(2, now, false), vec!["stale"]);
        assert_eq!(parked.len(), 2);
        // Its deadline passing makes the next one due.
        assert_eq!(parked.take_due(2, soon, false), vec!["current-soon"]);
        // A publish answers what is left; nothing is left afterwards.
        assert_eq!(parked.take_due(3, now, false), vec!["current-later"]);
        assert_eq!(parked.next_deadline(), None);
        // `all` empties it regardless.
        parked.push("going-away", 3, later);
        assert_eq!(parked.take_due(3, now, true), vec!["going-away"]);
        assert_eq!(parked.len(), 0);
    }

    #[test]
    fn the_pulse_publishes_and_interrupts() {
        let pulse = Arc::new(Pulse::new());
        assert_eq!(pulse.current(), 0);
        pulse.publish(3);
        assert_eq!(pulse.current(), 3);
        // An interrupt does not touch the version.
        pulse.interrupt();
        assert_eq!(pulse.current(), 3);
        assert_eq!(pulse.parked(), 0);
        // The answering thread leaves when told to, even with nothing parked.
        let stopping = Arc::new(AtomicBool::new(false));
        let (worker_pulse, worker_stop) = (Arc::clone(&pulse), Arc::clone(&stopping));
        let handle = thread::spawn(move || answer_parked(&worker_pulse, &worker_stop));
        thread::sleep(Duration::from_millis(30));
        stopping.store(true, Ordering::Relaxed);
        pulse.interrupt();
        handle.join().unwrap();
    }

    #[test]
    fn the_icon_is_the_one_thing_a_phone_may_keep() {
        // 660 KB of the 750 KB a cold open moved was this one file, fetched
        // again every time because everything was `no-store`. It is part of
        // the program rather than part of the board, so it keeps.
        assert_eq!(caching_for("/icon.png").value.as_str(), "public, max-age=604800");
        // Everything else is the board, or carries the token, and must not be
        // kept by anything between here and the phone.
        for path in ["/", "/index.html", "/sw.js", "/manifest.webmanifest", "/api/state", "/calendar.ics"] {
            assert_eq!(caching_for(path).value.as_str(), "no-store", "{path}");
        }
    }

    #[test]
    fn manifest_keeps_the_token_for_home_screen_shortcuts() {
        assert!(manifest(Some("abc")).contains(r#""start_url":"/?token=abc""#));
        assert!(manifest(None).contains(r#""start_url":"/""#));
    }

    /// Read the rendered ANSI back into a module grid.
    ///
    /// The point of the test below: a QR that is transposed, off by a row, or
    /// inverted still *looks* like a QR, and only a round trip catches it.
    fn modules_from_ansi(text: &str) -> Vec<Vec<bool>> {
        let mut grid: Vec<Vec<bool>> = Vec::new();
        let ink_bg = format!("48;5;{QR_INK}");
        let ink_fg = format!("38;5;{QR_INK}");
        for line in text.lines() {
            let (mut top, mut bottom) = (Vec::new(), Vec::new());
            // A cell may carry two escapes before its glyph, so the codes are
            // gathered until one actually arrives.
            let mut pending = String::new();
            let mut chars = line.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '\u{1b}' {
                    let mut code = String::new();
                    for next in chars.by_ref() {
                        if next == 'm' {
                            break;
                        }
                        code.push(next);
                    }
                    pending.push_str(&code);
                    continue;
                }
                match ch {
                    // A space paints only its background: both halves alike.
                    ' ' => {
                        let dark = pending.contains(&ink_bg);
                        top.push(dark);
                        bottom.push(dark);
                    }
                    // `▄`: foreground is the lower half, background the upper.
                    '\u{2584}' => {
                        top.push(pending.contains(&ink_bg));
                        bottom.push(pending.contains(&ink_fg));
                    }
                    _ => {}
                }
                pending.clear();
            }
            if !top.is_empty() {
                grid.push(top);
                grid.push(bottom);
            }
        }
        grid
    }

    #[test]
    fn a_printed_qr_is_the_same_code_the_screen_would_have_drawn() {
        // The longest thing this ever encodes: a tailnet host and a
        // thirty-two character token. Spelled out rather than taken from a
        // real machine, so the test carries nobody's network in it.
        let url = "http://a-laptop.tailnet-example.ts.net:7373/?token=abcdefghijkmnpqrstuvwxyz23456789";
        let (width, cells) = qr_modules(url).expect("a link encodes");
        let text = qr_text(url).expect("and renders");
        let grid = modules_from_ansi(&text);

        let side = width + QR_QUIET * 2;
        assert_eq!(text.lines().count(), side.div_ceil(2), "two module rows to a text row");
        assert!(grid.len() >= side, "every module row is accounted for");
        assert!(grid.iter().all(|row| row.len() == side), "and every row is the full width");

        // The quiet zone is light ink, not merely absent — the decoder finds
        // the corners by it, and a terminal's own background is whatever the
        // theme says.
        for x in 0..side {
            assert!(!grid[0][x] && !grid[QR_QUIET - 1][x], "the top margin is light");
        }
        for row in grid.iter().take(side) {
            assert!(!row[0] && !row[QR_QUIET - 1], "and so are the sides");
        }

        // And every module came back where it went in, the right way round.
        // Inverted, transposed or shifted by one, this is the assertion that
        // fails; by eye, all three still look like a QR code.
        for y in 0..width {
            for x in 0..width {
                assert_eq!(
                    grid[y + QR_QUIET][x + QR_QUIET],
                    cells[y * width + x],
                    "module ({x}, {y}) came back different"
                );
            }
        }
    }

    #[test]
    fn the_address_that_works_from_anywhere_is_the_one_offered_first() {
        // The QR encodes whatever comes first, and a phone away from the house
        // cannot reach 192.168.x.x — a code carrying it does not fail, it
        // spins forever, which is worse. Found by scanning one.
        assert!(is_tailnet("100.101.102.103"));
        assert!(is_tailnet("100.64.0.1") && is_tailnet("100.127.255.254"), "the ends of 100.64/10");
        assert!(!is_tailnet("100.63.255.255") && !is_tailnet("100.128.0.0"), "and just outside it");
        assert!(!is_tailnet("192.168.1.10"));
        assert!(!is_tailnet("10.0.0.4"));
        // 100.x that is not in the CGNAT block is somebody's public address.
        assert!(!is_tailnet("100.200.1.1"));
        assert!(!is_tailnet("not an address"));

        let mut addresses = vec!["192.168.1.10".to_string(), "100.101.102.103".to_string()];
        addresses.sort_by_key(|address| !is_tailnet(address));
        assert_eq!(addresses.first().map(String::as_str), Some("100.101.102.103"));
    }

    #[test]
    fn qr_codes_are_square() {
        let (width, cells) = qr_modules("http://192.168.1.10:7373/?token=abcdefghijkmnpqrstuvwxyz23456789").unwrap();
        assert_eq!(cells.len(), width * width);
        assert!(cells.iter().any(|dark| *dark));
    }
}
