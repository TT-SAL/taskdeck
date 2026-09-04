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
    io::Read,
    net::{IpAddr, UdpSocket},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, TryRecvError, channel},
    },
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, NaiveDate, Timelike, Utc};
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
pub const DEFAULT_BIND: &str = "0.0.0.0";
/// Lowest port the settings accept: the privileged range needs root and is
/// full of things that are not calendars.
pub const PORT_MIN: u16 = 1024;
/// Most days one `/api/state` call will describe. The page asks for one; the
/// ceiling is so a typo in the URL cannot ask for ten years of them.
pub const MAX_SNAPSHOT_DAYS: u32 = 14;

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
/// How many recent replies are kept by key. A retry comes within seconds.
const REPLIES_KEPT: usize = 256;
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
    let days = days.clamp(1, MAX_SNAPSHOT_DAYS);
    let day_snapshots = (0..days as i64)
        .map(|offset| from + ChronoDuration::days(offset))
        .map(|day| build_day(&board.items, board.archive.entries(), day, now))
        .collect();

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
                let body = json_ok(serde_json::json!({ "version": version })).with_header(no_store());
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
    /// Bind `port` on `bind` — every interface by default (`DEFAULT_BIND`), or
    /// the one address given — and start serving.
    ///
    /// Every interface rather than the LAN one alone, because "the phone" is
    /// sometimes on the LAN and sometimes on a Tailscale address, and the token
    /// is what guards the door in both cases. Binding one address is the
    /// narrower posture for a box that also sits on a network it should not
    /// serve (`SERVER.md` §2).
    pub fn start(
        bind: &str,
        port: u16,
        token: String,
        tx: Sender<PhoneRequest>,
        wake: Wake,
        pulse: Arc<Pulse>,
    ) -> Result<Self, String> {
        if token.is_empty() {
            return Err("The phone view has no token to guard it with.".to_string());
        }
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

    // Long poll: parked on the pulse and answered by its thread the moment the
    // world changes, or after `WAIT_TIMEOUT` with the version unchanged. This
    // worker hands the request over and is free at once.
    if *request.method() == Method::Get && path == "/api/wait" {
        if !authorised {
            let refused = json_error(401, "Not authorised. Open the phone view from the link in TaskDeck's settings.");
            let _ = request.respond(refused.with_header(no_store()));
            return;
        }
        let seen = query.lookup("version").and_then(|text| text.parse::<u64>().ok()).unwrap_or(0);
        if let Some(request) = pulse.park(request, seen, Instant::now() + WAIT_TIMEOUT) {
            // The pulse thread has left: nothing would answer a request
            // parked now. Answered here instead, with what there is.
            let now = json_ok(serde_json::json!({ "version": pulse.current() }));
            let _ = request.respond(now.with_header(no_store()));
        }
        return;
    }

    let response = match (request.method(), path) {
        // The page itself carries no data and is public: it has to be, so a
        // home-screen shortcut that opens `/` can pick its token back up from
        // storage before it asks for anything.
        (Method::Get | Method::Head, "/") | (Method::Get | Method::Head, "/index.html") => {
            with_type(Response::from_string(PAGE), "text/html; charset=utf-8")
        }
        (Method::Get | Method::Head, "/icon.png") => with_type(Response::from_data(ICON.to_vec()), "image/png"),
        // The offline shell (`phone_sw.js`). Public like the page; it holds
        // no data and a browser only honours it from a secure origin.
        (Method::Get, "/sw.js") => {
            with_type(Response::from_string(SERVICE_WORKER), "application/javascript; charset=utf-8")
        }
        (Method::Get, "/manifest.webmanifest") => {
            with_type(Response::from_string(manifest(query.lookup("token").map(String::as_str))), "application/manifest+json")
        }
        _ if !authorised => json_error(
            401,
            "Not authorised. Open the phone view from the link in TaskDeck's settings.",
        ),
        (Method::Get, "/api/state") => match snapshot_command(&query) {
            Ok(command) => match ask(command, tx, wake) {
                Ok(value) => json_ok(value),
                Err(error) => json_error(error.status, &error.message),
            },
            Err(error) => json_error(error.status, &error.message),
        },
        // The whole board, for a desktop that keeps a replica of it.
        (Method::Get, "/api/board") => match ask(Command::Board, tx, wake) {
            Ok(value) => json_ok(value),
            Err(error) => json_error(error.status, &error.message),
        },
        (Method::Post, "/api/command") => match read_body(&mut request) {
            Ok(body) => match serde_json::from_slice::<Command>(&body) {
                Ok(command) if command.is_query() => json_error(400, "That is a query, not a command."),
                Ok(command) => match replies.claim(&key) {
                    // Asked before, under this key, and answered: the same
                    // answer again, and the board hears nothing.
                    Claim::Done(value) => json_ok(value),
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
                                    json_ok(value)
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
            Ok(serde_json::Value::String(feed)) => {
                with_type(Response::from_string(feed), "text/calendar; charset=utf-8")
            }
            Ok(_) => json_error(500, "The feed came back in the wrong shape."),
            Err(error) => json_error(error.status, &error.message),
        },
        _ => json_error(404, "No such page."),
    };

    let _ = request.respond(response.with_header(no_store()));
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
    if request.body_length().is_some_and(|length| length > MAX_BODY_BYTES) {
        return Err(PhoneError::bad_request("That request is too large."));
    }
    let mut body = Vec::new();
    request
        .as_reader()
        .take(MAX_BODY_BYTES as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|error| PhoneError::bad_request(format!("Could not read the request: {error}")))?;
    if body.len() > MAX_BODY_BYTES {
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

fn no_store() -> Header {
    header("Cache-Control", "no-store")
}

fn with_type(response: Body, content_type: &str) -> Body {
    response.with_header(header("Content-Type", content_type))
}

fn json_ok(value: serde_json::Value) -> Body {
    with_type(Response::from_string(value.to_string()), "application/json; charset=utf-8")
}

fn json_error(status: u16, message: &str) -> Body {
    let body = serde_json::json!({ "error": message }).to_string();
    with_type(Response::from_string(body), "application/json; charset=utf-8").with_status_code(status)
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
        "icons": [{ "src": "/icon.png", "sizes": "882x882", "type": "image/png" }]
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
pub fn local_addresses() -> Vec<String> {
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
        // Everywhere, or nonsense, means every address this machine has.
        assert_eq!(addresses_for(DEFAULT_BIND), local_addresses());
        assert_eq!(addresses_for("::"), local_addresses());
        assert_eq!(addresses_for("kitchen"), local_addresses());

        let (tx, _rx) = channel();
        let wake: Wake = Arc::new(|| {});
        let Err(error) = PhoneServer::start("kitchen", 0, "k".to_string(), tx, wake, Arc::new(Pulse::new())) else { panic!("a bad bind must not listen") };
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
    fn manifest_keeps_the_token_for_home_screen_shortcuts() {
        assert!(manifest(Some("abc")).contains(r#""start_url":"/?token=abc""#));
        assert!(manifest(None).contains(r#""start_url":"/""#));
    }

    #[test]
    fn qr_codes_are_square() {
        let (width, cells) = qr_modules("http://192.168.1.10:7373/?token=abcdefghijkmnpqrstuvwxyz23456789").unwrap();
        assert_eq!(cells.len(), width * width);
        assert!(cells.iter().any(|dark| *dark));
    }
}
