//! The runtime (device.md §4.2 to §4.4, §8): the thread `taskdeck-sync`, its
//! loop and its three queues, the engine's effects performed, ticks, the
//! published view and the change counters.
//!
//! The sync thread owns the engine and the store (DEV-THREAD-001) and does
//! no network I/O (DEV-THREAD-002): relay requests go to the thread
//! `taskdeck-https` (`net`), whose answers come back on the sync thread's
//! one channel as event-queue inputs.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use taskdeck_core::board::BoardData;
use taskdeck_core::command::LogCommand;
use taskdeck_core::compile::{CompileCtx, compile};
use taskdeck_core::ids::IdMinter;
use taskdeck_core::intent::Intent;
use taskdeck_core::overlay::Overlay;
use taskdeck_core::time::{Instant, Zone};
use taskdeck_sync::{Config, Effect, Engine, Event, Input, Notification, RelayTarget, StateMachine, Status};
use zeroize::Zeroizing;

use crate::net::{self, Back, Job, NetConfig};
use crate::pairing::{PairNet, Pairing};
use crate::store::{self, IdentityStore, Opened, RelayToken};
use crate::{
    Changes, CoreMachine, DeviceError, FileLayer, KeyStore, Mode, Published, SealSealer, StoredIdentity,
    Submitted,
};

pub(crate) type Eng = Engine<CoreMachine, SealSealer>;

/// The zone the device compiles with (device.md §16, §17.2).
pub(crate) enum DeviceZone {
    Table(taskdeck_core::time::TableZone),
    Local(taskdeck_core::time::ChronoZone<chrono::Local>),
}

impl DeviceZone {
    pub(crate) fn zone(&self) -> &dyn Zone {
        match self {
            DeviceZone::Table(z) => z,
            DeviceZone::Local(z) => z,
        }
    }
}

/// The device clock (device.md §8.1, §11.1 item 5): `wall_ms` the wall
/// clock, `mono_ms` the monotonic time since `open`; with `test-trust`, the
/// injected clock of `OpenOptions`.
#[derive(Clone)]
pub(crate) struct Clock {
    #[cfg(feature = "test-trust")]
    test: Option<Arc<dyn crate::TestClock>>,
    start: std::time::Instant,
}

impl Clock {
    #[cfg(feature = "test-trust")]
    pub(crate) fn new(test: Option<Arc<dyn crate::TestClock>>) -> Clock {
        Clock { test, start: std::time::Instant::now() }
    }

    #[cfg(not(feature = "test-trust"))]
    pub(crate) fn new() -> Clock {
        Clock { start: std::time::Instant::now() }
    }

    pub(crate) fn wall_ms(&self) -> i64 {
        #[cfg(feature = "test-trust")]
        if let Some(t) = &self.test {
            return t.wall_ms();
        }
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }

    pub(crate) fn mono_ms(&self) -> u64 {
        #[cfg(feature = "test-trust")]
        if let Some(t) = &self.test {
            return t.mono_ms();
        }
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Whether the clock is injected: it moves without waking the thread,
    /// so the loop looks at it every few milliseconds.
    fn injected(&self) -> bool {
        #[cfg(feature = "test-trust")]
        if self.test.is_some() {
            return true;
        }
        false
    }
}

/// What the facade and the sync thread share (device.md §4.4, §17.3).
pub(crate) struct Shared {
    pub(crate) published: Mutex<Arc<Published>>,
    pub(crate) changes: Mutex<Changes>,
    pub(crate) changed: Condvar,
}

impl Shared {
    pub(crate) fn view(&self) -> Arc<Published> {
        match self.published.lock() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }

    pub(crate) fn changes(&self) -> Changes {
        match self.changes.lock() {
            Ok(g) => *g,
            Err(p) => *p.into_inner(),
        }
    }
}

/// The lazily started thread `taskdeck-https` (device.md §4.2).
pub(crate) struct Net {
    inner: Mutex<NetInner>,
}

struct NetInner {
    tx: Option<Sender<Job>>,
    join: Option<std::thread::JoinHandle<()>>,
    to_sync: Option<Sender<Msg>>,
    clock: Clock,
}

impl Net {
    pub(crate) fn new(to_sync: Sender<Msg>, clock: Clock) -> Net {
        Net { inner: Mutex::new(NetInner { tx: None, join: None, to_sync: Some(to_sync), clock }) }
    }

    /// Queues a job, starting the thread at the first one.
    pub(crate) fn submit(&self, job: Job) -> bool {
        let Ok(mut g) = self.inner.lock() else { return false };
        if g.tx.is_none() {
            let Some(to_sync) = g.to_sync.clone() else { return false };
            let clock = g.clock.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let sink: net::Sink = Box::new(move |b| to_sync.send(Msg::Net(b)).is_ok());
            let wall: net::WallClock = Box::new(move || clock.wall_ms());
            let join = std::thread::Builder::new().name("taskdeck-https".into()).spawn(move || net::run(rx, sink, wall));
            match join {
                Ok(j) => {
                    g.tx = Some(tx);
                    g.join = Some(j);
                }
                Err(_) => return false,
            }
        }
        g.tx.as_ref().is_some_and(|tx| tx.send(job).is_ok())
    }

    /// Closes the queue and joins the thread (DEV-THREAD-003).
    pub(crate) fn stop(&self) {
        let join = match self.inner.lock() {
            Ok(mut g) => {
                g.tx = None;
                g.to_sync = None;
                g.join.take()
            }
            Err(_) => None,
        };
        if let Some(j) = join {
            let _ = j.join();
        }
    }
}

/// A facade request (device.md §4.5): a one-shot reply channel each.
pub(crate) enum Call {
    Submit { intent: Box<Intent>, hold: bool, reply: Sender<Result<Submitted, DeviceError>> },
    Shutdown { reply: Sender<Result<(), DeviceError>> },
    /// §18.1 step 7: the inactive identity directory.
    CreatePersist { new: Box<crate::create::Persist>, reply: Sender<Result<(), DeviceError>> },
    /// §18.1 steps 9 and 10: the token, the activation, the reopen.
    CreateActivate { token: RelayToken, reply: Sender<Result<(), DeviceError>> },
    Pair(crate::pairing::PairCall),
}

/// One input of the sync thread's channel.
pub(crate) enum Msg {
    Call(Call),
    Net(Back),
    Pair(PairNet),
}

/// An answer the engine owes a waiting `submit`.
struct Waiting {
    reply: Sender<Result<Submitted, DeviceError>>,
    compiled: taskdeck_core::compile::Reply,
}

/// Synced mode: the engine and its store.
pub(crate) struct Synced {
    pub(crate) engine: Eng,
    pub(crate) store: IdentityStore,
    pub(crate) minter: Option<IdMinter>,
    pub(crate) board: [u8; 16],
    pub(crate) dev: [u8; 16],
    /// The relay target of the latest `Connect`, else of `Loaded`.
    pub(crate) relay: Option<RelayTarget>,
    waiting: BTreeMap<u64, Waiting>,
}

/// What the loop changed since the last publish.
#[derive(Default)]
struct Dirty {
    view: bool,
    status: bool,
    notices: bool,
    banners: bool,
    pairing: bool,
}

/// How the sync thread starts.
pub(crate) struct Start {
    pub(crate) shared: Arc<Shared>,
    pub(crate) rx: Receiver<Msg>,
    pub(crate) net: Arc<Net>,
    pub(crate) clock: Clock,
    pub(crate) files: Arc<dyn FileLayer>,
    pub(crate) data_dir: PathBuf,
    pub(crate) key_store: Arc<dyn KeyStore>,
    pub(crate) build_id: String,
    pub(crate) zone: DeviceZone,
    pub(crate) default_name: String,
    pub(crate) opened: Option<Box<Opened>>,
    pub(crate) mode: Mode,
    pub(crate) identities: Vec<StoredIdentity>,
    pub(crate) lock: store::Lock,
    pub(crate) cloud: bool,
}

pub(crate) struct Runtime {
    pub(crate) shared: Arc<Shared>,
    rx: Receiver<Msg>,
    pub(crate) net: Arc<Net>,
    pub(crate) clock: Clock,
    pub(crate) files: Arc<dyn FileLayer>,
    pub(crate) data_dir: PathBuf,
    pub(crate) key_store: Arc<dyn KeyStore>,
    build_id: String,
    zone: DeviceZone,
    pub(crate) default_name: String,
    local: VecDeque<Event<LogCommand>>,
    calls: VecDeque<Call>,
    events: VecDeque<Msg>,
    pub(crate) synced: Option<Box<Synced>>,
    pub(crate) pending_create: Option<IdentityStore>,
    pub(crate) pair: Pairing,
    pub(crate) mode: Mode,
    pub(crate) identities: Vec<StoredIdentity>,
    conn: u64,
    tick_at: Option<u64>,
    exiting: Option<(Sender<Result<(), DeviceError>>, std::time::Instant)>,
    exit: bool,
    dirty: Dirty,
    last_vv: u64,
    last_status: Option<Status>,
    next_ticket: u64,
    /// The relay address shown (§17.8), once known.
    pub(crate) relay_address: Option<String>,
    /// The data folder's lock, held for the runtime's life (§5.2).
    pub(crate) lock: store::Lock,
    pub(crate) cloud: bool,
}

/// How long `shutdown` waits for the engine's `ReadyToExit` (§17.2: the
/// call returns within 10,000 ms).
const EXIT_WAIT: Duration = Duration::from_millis(8_000);

pub(crate) fn err(code: &'static str) -> DeviceError {
    DeviceError { code, status: None, message: None }
}

impl Runtime {
    /// Builds the runtime and, in synced mode, the engine; then hands
    /// `Start` to it (§8.1).
    pub(crate) fn new(s: Start) -> Result<Runtime, DeviceError> {
        let mut rt = Runtime {
            shared: s.shared,
            rx: s.rx,
            net: s.net,
            clock: s.clock,
            files: s.files,
            data_dir: s.data_dir,
            key_store: s.key_store,
            build_id: s.build_id,
            zone: s.zone,
            default_name: s.default_name,
            local: VecDeque::new(),
            calls: VecDeque::new(),
            events: VecDeque::new(),
            synced: None,
            pending_create: None,
            pair: Pairing::Idle,
            mode: s.mode,
            identities: s.identities,
            conn: 0,
            tick_at: None,
            exiting: None,
            exit: false,
            dirty: Dirty { view: true, status: true, ..Dirty::default() },
            last_vv: 0,
            last_status: None,
            next_ticket: 1,
            relay_address: None,
            lock: s.lock,
            cloud: s.cloud,
        };
        if let Some(o) = s.opened {
            rt.start_synced(*o)?;
        }
        rt.publish();
        Ok(rt)
    }

    pub(crate) fn ticket(&mut self) -> u64 {
        let t = self.next_ticket;
        self.next_ticket += 1;
        t
    }

    /// Synced mode from an opened identity directory (device.md §5.4 step 6,
    /// §18.1 step 10, §18.3): the sealer from `identity.tdk` and
    /// `history.tdk`, a new engine, the transport configured, `Start`.
    pub(crate) fn start_synced(&mut self, opened: Opened) -> Result<(), DeviceError> {
        let sealer = opened.sealer()?;
        let keys = sealer.device_keys().ok_or_else(|| err("device/store_corrupt"))?;
        let board = sealer.board_id();
        let dev = keys.public().dev.0;
        let relay = match (&opened.loaded.meta, &opened.loaded.bootstrap) {
            (Some(m), _) => Some(m.relay.clone()),
            (None, Some(taskdeck_sync::Bootstrap::Genesis { relay, .. } | taskdeck_sync::Bootstrap::Join { relay, .. })) => {
                Some(relay.clone())
            }
            (None, None) => None,
        };
        let tokens = opened.store.settings().tokens.clone();
        let minter = opened.store.minter();
        let Opened { store, loaded, .. } = opened;
        let engine = Engine::new(CoreMachine::new(), sealer, Config {});
        self.relay_address =
            relay.as_ref().map(|r| crate::net::wire::address_text(&r.host, &board, &r.door));
        self.synced = Some(Box::new(Synced { engine, store, minter, board, dev, relay, waiting: BTreeMap::new() }));
        self.mode = Mode::Synced;
        self.conn += 1;
        self.tick_at = None;
        self.net.submit(Job::Configure(Some(Box::new(NetConfig { board, dev, keys, tokens }))));
        self.identities = store::identities(&*self.files, &self.data_dir);
        self.dirty.view = true;
        self.dirty.status = true;
        self.feed(Event::Start(loaded));
        Ok(())
    }

    /// The loop of §4.3 until `shutdown` ends it.
    pub(crate) fn run(mut self) {
        while !self.exit {
            self.turn();
        }
    }

    fn drain(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            self.queue(m);
        }
    }

    fn queue(&mut self, m: Msg) {
        match m {
            Msg::Call(c) => self.calls.push_back(c),
            other => self.events.push_back(other),
        }
    }

    /// One loop turn (§4.3): one input by the priority local, call, event
    /// (DEV-THREAD-004, DEV-THREAD-005); its effects; one publish.
    fn turn(&mut self) {
        self.drain();
        if let Some(ev) = self.local.pop_front() {
            self.feed(ev);
        } else if let Some(c) = self.calls.pop_front() {
            self.call(c);
        } else if let Some(m) = self.events.pop_front() {
            self.event(m);
        } else if self.tick_due() {
            self.tick_at = None;
            self.feed(Event::Tick);
        } else if self.pairing_due() {
            self.pairing_tick();
        } else {
            self.wait();
            return;
        }
        self.check_exit();
        self.pair_progress();
        self.publish();
    }

    fn tick_due(&self) -> bool {
        self.synced.is_some() && self.tick_at.is_some_and(|t| self.clock.mono_ms() >= t)
    }

    /// Waits on the channel until the next deadline: the tick, a pairing
    /// timer, or the end of a shutdown. An injected clock is looked at every
    /// 10 ms (DEV-RUN-002 asks for at most 50 ms).
    fn wait(&mut self) {
        let now = self.clock.mono_ms();
        let mut until: Option<u64> = self.tick_at;
        if let Some(p) = self.pair.next_due() {
            until = Some(until.map_or(p, |t| t.min(p)));
        }
        let mut d = until.map_or(Duration::from_millis(1_000), |t| Duration::from_millis(t.saturating_sub(now).min(1_000)));
        if self.clock.injected() || self.exiting.is_some() {
            d = d.min(Duration::from_millis(10));
        }
        match self.rx.recv_timeout(d) {
            Ok(m) => self.queue(m),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                // Every facade handle is gone: stop as `shutdown` would.
                if self.exiting.is_none() {
                    self.begin_exit(None);
                }
            }
        }
        self.check_exit();
    }

    // ------------------------------------------------------------ the engine

    pub(crate) fn feed(&mut self, event: Event<LogCommand>) {
        let Some(s) = self.synced.as_mut() else { return };
        let input = Input { wall_ms: self.clock.wall_ms(), mono_ms: self.clock.mono_ms(), event };
        let fx = s.engine.handle(input);
        self.perform(fx);
    }

    /// The effects of one `handle` call (§8.2), in emission order.
    fn perform(&mut self, effects: Vec<Effect>) {
        for fx in effects {
            let Some(s) = self.synced.as_mut() else { return };
            match fx {
                Effect::Persist { id, ops } => {
                    let history = s.engine.sealer().history();
                    match s.store.persist(&ops, &history) {
                        Ok(()) => self.local.push_back(Event::Persisted { id }),
                        Err(_) => self.local.push_back(Event::PersistFailed { id }),
                    }
                }
                Effect::ReadLog { req, from, to } => {
                    let result = s.store.read_log(from, to);
                    self.local.push_back(Event::LocalRead { req, result });
                }
                Effect::ReadBase { req, seq } => {
                    let (result, _quarantined) = s.store.read_base(seq, self.clock.wall_ms());
                    self.local.push_back(Event::LocalRead { req, result });
                }
                Effect::Connect { relay } => {
                    self.conn += 1;
                    s.relay = Some(relay.clone());
                    let conn = self.conn;
                    self.net.submit(Job::Effect { conn, effect: Effect::Connect { relay } });
                }
                Effect::Disconnect => {
                    // Answers of requests already sent are discarded (§10.5).
                    self.conn += 1;
                }
                e @ (Effect::Hello | Effect::Read { .. } | Effect::Append { .. } | Effect::Admin { .. } | Effect::Blob { .. }) => {
                    let conn = self.conn;
                    self.net.submit(Job::Effect { conn, effect: e });
                }
                Effect::ScheduleTick { at_mono_ms } => self.tick_at = Some(at_mono_ms),
                Effect::Notify(n) => self.notify(n),
            }
        }
    }

    /// §8.4.
    fn notify(&mut self, n: Notification) {
        match n {
            Notification::Enqueued { ticket, entry_id, provisional_seq } => {
                let Some(s) = self.synced.as_mut() else { return };
                if let Some(w) = s.waiting.remove(&ticket) {
                    let view_version = s.engine.view_version();
                    let _ = w.reply.send(Ok(Submitted { reply: w.compiled, entry_id, provisional_seq, view_version }));
                }
            }
            Notification::EnqueueRefused { ticket, code } => {
                if let Some(w) = self.synced.as_mut().and_then(|s| s.waiting.remove(&ticket)) {
                    let _ = w.reply.send(Err(err(code)));
                }
            }
            Notification::ViewChanged { .. } => self.dirty.view = true,
            Notification::StatusChanged | Notification::AlarmRaised { .. } | Notification::ReplaceReady { .. } => {
                self.dirty.status = true
            }
            Notification::NoticesAdded { .. } => self.dirty.notices = true,
            Notification::BannersChanged => self.dirty.banners = true,
            Notification::PairRegistered { ticket, head, snapshot, added_seq } => {
                self.pair_registered(ticket, head, snapshot, added_seq);
            }
            Notification::PairFailed { ticket, code } => self.pair_failed(ticket, code),
            Notification::ReadyToExit
                if self.exiting.is_some() => {
                    self.exit_now(Ok(()));
                }
            _ => {}
        }
    }

    // ------------------------------------------------------------ inputs

    fn event(&mut self, m: Msg) {
        match m {
            Msg::Net(Back::Event { conn, event }) => {
                if conn == self.conn {
                    self.feed(*event);
                }
            }
            Msg::Net(Back::StoreToken { token, ack }) => {
                let ok = match self.synced.as_mut() {
                    Some(s) => {
                        let mut settings = s.store.settings().clone();
                        settings.put_token(token);
                        s.store.write_settings(settings).is_ok()
                    }
                    None => false,
                };
                let _ = ack.send(ok);
            }
            Msg::Net(Back::RegisterToken { dev, token }) => self.pair.register_token(dev, token),
            Msg::Pair(p) => self.pair_net(p),
            Msg::Call(c) => self.call(c),
        }
    }

    fn call(&mut self, c: Call) {
        if self.exiting.is_some() {
            refuse(c, err("device/closed"));
            return;
        }
        match c {
            Call::Submit { intent, hold, reply } => {
                let r = self.submit(&intent, hold, reply);
                if let Err((e, reply)) = r {
                    let _ = reply.send(Err(e));
                }
            }
            Call::Shutdown { reply } => self.begin_exit(Some(reply)),
            Call::CreatePersist { new, reply } => {
                let r = crate::create::persist(self, *new);
                let _ = reply.send(r);
            }
            Call::CreateActivate { token, reply } => {
                let r = crate::create::activate(self, token);
                let _ = reply.send(r);
            }
            Call::Pair(p) => self.pair_call(p),
        }
    }

    /// `submit` in synced mode (§17.4, DEV-FACADE-001): compile against the
    /// engine's pending view with the device zone, clock and minter, and
    /// `Enqueue` in the same turn; the reply waits for `Enqueued`.
    #[allow(clippy::type_complexity)]
    fn submit(
        &mut self,
        intent: &Intent,
        hold: bool,
        reply: Sender<Result<Submitted, DeviceError>>,
    ) -> Result<(), (DeviceError, Sender<Result<Submitted, DeviceError>>)> {
        let wall = self.clock.wall_ms();
        let ticket = self.ticket();
        let Some(s) = self.synced.as_mut() else { return Err((err("device/wrong_mode"), reply)) };
        let Some(device_no) = s.engine.device_no() else { return Err((err("engine/not_joined"), reply)) };
        let Some(now) = Instant::from_at_ms(wall) else { return Err((err("device/record_range"), reply)) };
        let mut minter = match s.minter {
            Some(m) if m.device_no == device_no => m,
            _ => IdMinter::new(device_no),
        };
        let overlay = Overlay::default();
        let compiled = {
            let cx = CompileCtx { view: s.engine.view(), zone: self.zone.zone(), now, device_no, overlay: &overlay };
            compile(intent, &cx, &mut minter)
        };
        let compiled = match compiled {
            Ok(c) => c,
            Err(e) => {
                let de = DeviceError { code: e.code, status: Some(e.status), message: Some(e.message) };
                return Err((de, reply));
            }
        };
        let Some(command) = compiled.command else {
            let view_version = s.engine.view_version();
            let _ = reply.send(Ok(Submitted { reply: compiled.reply, entry_id: None, provisional_seq: None, view_version }));
            return Ok(());
        };
        s.minter = Some(minter);
        let origin_state = CoreMachine::origin_state(&minter);
        s.waiting.insert(ticket, Waiting { reply, compiled: compiled.reply });
        self.feed(Event::Enqueue { ticket, command, at_ms: wall, origin_state, hold });
        Ok(())
    }

    // ------------------------------------------------------------ shutdown (§17.2)

    fn begin_exit(&mut self, reply: Option<Sender<Result<(), DeviceError>>>) {
        let (tx, _rx) = std::sync::mpsc::channel();
        let reply = reply.unwrap_or(tx);
        self.exiting = Some((reply, std::time::Instant::now()));
        self.pair.cancel_all();
        if self.synced.is_some() {
            self.feed(Event::Shutdown);
        } else {
            self.exit_now(Ok(()));
        }
    }

    fn check_exit(&mut self) {
        if let Some((_, since)) = &self.exiting
            && since.elapsed() >= EXIT_WAIT && self.local.is_empty() {
                self.exit_now(Err(err("device/shutdown_timeout")));
            }
    }

    fn exit_now(&mut self, r: Result<(), DeviceError>) {
        // The local queue holds the `Persisted` of the final writes: they are
        // durable already; nothing more is handed to the engine.
        if let Some((reply, _)) = self.exiting.take() {
            self.exit = true;
            self.publish();
            let _ = reply.send(r);
        }
    }

    // ------------------------------------------------------------ the published view (§4.4)

    pub(crate) fn mark_pairing(&mut self) {
        self.dirty.pairing = true;
    }

    pub(crate) fn mark_mode(&mut self) {
        self.dirty.view = true;
        self.dirty.status = true;
    }

    /// Publishes a new view when the turn changed what it shows, and raises
    /// the counters (DEV-THREAD-007, DEV-NOTIFY-001).
    pub(crate) fn publish(&mut self) {
        let status = self.synced.as_ref().map(|s| s.engine.status());
        if let Some(s) = self.synced.as_ref()
            && s.engine.view_version() != self.last_vv {
                self.dirty.view = true;
            }
        if status != self.last_status {
            self.dirty.status = true;
        }
        let d = std::mem::take(&mut self.dirty);
        if !(d.view || d.status || d.notices || d.banners || d.pairing) {
            return;
        }
        let sm = CoreMachine::new();
        let (board, version, confirmed) = match self.synced.as_ref() {
            Some(s) => {
                let (c, _) = s.engine.confirmed();
                let bytes = sm.canonical_bytes(c);
                let hash = taskdeck_sync::state_hash(1, &sm.board_hash(c), &s.engine.membership_bytes());
                (Arc::new(s.engine.view().clone()), s.engine.view_version(), Some((bytes, hash)))
            }
            None => (Arc::new(BoardData::empty()), 0, None),
        };
        self.last_vv = version;
        self.last_status.clone_from(&status);
        let device_name = match (&self.synced, &self.pending_create) {
            (Some(s), _) => s.store.settings().device_name.clone(),
            _ => self.default_name.clone(),
        };
        let p = Published {
            board,
            version,
            status,
            mode: self.mode,
            confirmed,
            pairing: self.pair.state(),
            relay_address: self.relay_address.clone(),
            device_name,
            identities: self.identities.clone(),
        };
        match self.shared.published.lock() {
            Ok(mut g) => *g = Arc::new(p),
            Err(poisoned) => *poisoned.into_inner() = Arc::new(p),
        }
        let mut c = match self.shared.changes.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        c.any += 1;
        if d.view {
            c.view += 1;
        }
        if d.status {
            c.status += 1;
        }
        if d.notices {
            c.notices += 1;
        }
        if d.banners {
            c.banners += 1;
        }
        if d.pairing {
            c.pairing += 1;
        }
        drop(c);
        self.shared.changed.notify_all();
    }

    pub(crate) fn wall_ms(&self) -> i64 {
        self.clock.wall_ms()
    }

    pub(crate) fn mono_ms(&self) -> u64 {
        self.clock.mono_ms()
    }

    pub(crate) fn sync_sender(&self) -> Option<Sender<Msg>> {
        self.net.inner.lock().ok().and_then(|g| g.to_sync.clone())
    }

    pub(crate) fn build_id(&self) -> &str {
        &self.build_id
    }
}

/// Answers a call refused before it ran.
fn refuse(c: Call, e: DeviceError) {
    match c {
        Call::Submit { reply, .. } => {
            let _ = reply.send(Err(e));
        }
        Call::Shutdown { reply } | Call::CreatePersist { reply, .. } | Call::CreateActivate { reply, .. } => {
            let _ = reply.send(Err(e));
        }
        Call::Pair(p) => p.refuse(e),
    }
}

/// The waiting caller's side of a call (§4.5): sends it, blocks for the reply.
pub(crate) fn ask<T>(tx: &Mutex<Option<Sender<Msg>>>, make: impl FnOnce(Sender<T>) -> Call) -> Result<T, DeviceError> {
    let (reply, wait) = std::sync::mpsc::channel();
    let sent = match tx.lock() {
        Ok(g) => g.as_ref().is_some_and(|t| t.send(Msg::Call(make(reply))).is_ok()),
        Err(_) => false,
    };
    if !sent {
        return Err(err("device/closed"));
    }
    wait.recv().map_err(|_| err("device/closed"))
}

/// Zeroizing helper for tokens handed over as strings.
pub(crate) fn token_string(s: &str) -> Zeroizing<String> {
    Zeroizing::new(s.to_string())
}

/// `pairing_due` and `pairing_tick` live in `pairing`.
impl Runtime {
    fn pairing_due(&self) -> bool {
        self.pair.next_due().is_some_and(|t| self.clock.mono_ms() >= t)
    }
}
