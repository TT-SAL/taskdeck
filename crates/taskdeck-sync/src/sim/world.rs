//! The simulated world (engine.md §31.1, §31.4 to §31.6): the devices with
//! their model runtimes, the model relays, the network, the workload and the
//! faults, on one virtual clock, from one seed.
//!
//! The model runtime does what `device.md` asks of a runtime (§31.4):
//! - ENG-PERSIST-001: one serial persist queue per process; a write starts only
//!   after the one before it is durable, and `Persisted` events go out in the
//!   order of the effects, also for writes emitted while a `Persisted` is being
//!   handled;
//! - ENG-PERSIST-004: durable storage holds exactly the writes whose
//!   `Persisted` was delivered, plus, after a crash during a write, that write
//!   when the fault generator keeps it (complete and valid);
//! - ENG-EFFECT-001: every effect other than `Persist` is performed in the
//!   order emitted, at once, without waiting for any `Persist`;
//! - `Start` with `Loaded` read from durable storage after a restart delay
//!   (§7.10), the fake rebuilt by `FakeSealer::restore` with a fresh seed;
//!   ticks at `ScheduleTick` times; `Connect` after the backoff of `device.md`
//!   §10.7 (DEV-BACKOFF-001); one session id per process.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};

use sha2::{Digest, Sha256};

use super::check::{self, BranchLog, Facts};
use super::relay::{Cred, LogRow, ModelRelay, Out, SockMsg, SocketId};
use super::store::Store;
use super::trace::{Trace, V};
use super::{SimConfig, SimReport, SimStats, SplitMix64, Violation};

mod end;
mod load;
mod rt;
use crate::fake::FakeSealer;
use crate::toy::{ToyCommand, ToyMachine};
use crate::{
    AdminRequest, AlarmKind, AppendResult, BlobRequest, Body, Bootstrap, Config, CtlIntent, Dev, DeviceStatus, Effect,
    Engine, Event, Hash32, Head, Input, Kind, LocalReadResult, NewDevice, Notification, Operation, OutboxOp,
    OutboxRecord, Page, PersistOp, PlainEntry, RelayRefusal, RelayTarget, Sealer, SessionState, StateMachine,
    StopAction, StopKind, UserAction,
};

pub(crate) type Eng = Engine<ToyMachine, FakeSealer>;

/// The wall clock at virtual time 0 (a date in 2023; the beat interval needs a clock far from 0).
pub(crate) const WALL0: i64 = 1_700_000_000_000;
const HOUR: u64 = 3_600_000;
const SETTLE: u64 = 7_200_000;
const HOSTS: [&str; 2] = ["relay-a.example", "relay-b.example"];
/// The response timeout of `device.md` §10.10 (headers after the request).
const TIMEOUT: u64 = 20_000;
/// The base one-way latency of a message, before the drawn delay.
const LATENCY: u64 = 5;
const KEY_HELLO: u64 = 1 << 63;
/// The runtime's timer granularity: ticks at least this far apart.
const TICK_MIN: u64 = 1_000;
const KEY_CONNECT: u64 = 1 << 62;

// ---------- scheduled events

/// Which relay object serves a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Sel {
    Main(usize),
    Fork,
}

/// A request a device sends to a relay.
#[derive(Clone, Debug)]
enum Call {
    Login,
    Hello,
    Read { req: u64, after: u64, limit: u16 },
    Append { req: u64, frames: Vec<Vec<u8>> },
    Admin { req: u64, op: AdminRequest },
    Blob { req: u64, op: BlobRequest },
}

#[derive(Clone, Copy, Debug)]
enum CrashAt {
    Before,
    During,
    After,
}

/// A queued persist: its id, its ops, the sealer's history when it was emitted.
type Write = (u64, Vec<PersistOp>, Option<Vec<Vec<u8>>>);

/// Entry ids an answer acknowledges, by (identity, process, request key): (entry id, reference branch).
type PendingAcks = BTreeMap<(usize, u32, u64), Vec<([u8; 16], bool)>>;

/// What travels down to a device.
// It carries the engine's events by value, as the engine takes them.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
enum Down {
    /// Events in order (a connect's `MissedRoots`, `Connected`, `Welcome`).
    Events(Vec<Event<ToyCommand>>),
    /// An answer to an outstanding request (its key).
    Answer(u64, Event<ToyCommand>),
    /// A socket message.
    Sock(SockMsg),
}

// Events carry the engine's events by value, as the engine takes them.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
enum Ev {
    ToRelay { id: usize, proc: u32, conn: u64, sel: Sel, call: Call },
    Down { id: usize, proc: u32, conn: u64, msg: Down },
    PersistDone { id: usize, proc: u32, crash: Option<CrashAt> },
    Local { id: usize, proc: u32, ev: LocalAsk },
    Tick { id: usize, proc: u32, tgen: u64 },
    Connect { id: usize, proc: u32, tgen: u64 },
    Timeout { id: usize, proc: u32, conn: u64, key: u64 },
    Start { id: usize },
    Exit { id: usize, proc: u32 },
    Work { machine: usize },
    Sleep { machine: usize },
    Wake { machine: usize },
    User { id: usize, proc: u32, ev: Event<ToyCommand> },
    Poll { machine: usize },
    Pair { machine: usize, replaces: Option<usize> },
    PairCheck { ticket: u64 },
    PairRetry { pp: Box<PendingPair> },
    Repair { id: usize },
    Fault(FaultEv),
    OpAt(OpKind),
    Settle,
    End,
}

#[derive(Clone, Copy, Debug)]
enum LocalAsk {
    Log { req: u64, from: u64, to: u64 },
    Base { req: u64, seq: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaultEv {
    Disconnect,
    SkewJump,
    RollbackMark(u32),
    Rollback(u32),
    ForkStart,
    ForkEnd,
    WithholdStart,
    WithholdEnd,
    Inject(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpKind {
    Revoke,
    RulesBuild,
    Purge,
    Move,
    Restore,
    Recovery,
    Poison,
    PoisonSend,
}

/// The machine number of the faulty build (§31.5 poison).
const FAULTY: usize = usize::MAX - 1;

/// A member run without an engine: the faulty build (§15).
struct Faulty {
    dev: Dev,
    fake: FakeSealer,
    relay: usize,
    generation: u32,
    #[allow(dead_code)]
    added_seq: u64,
    ctr: u64,
}

struct Sched {
    t: u64,
    n: u64,
    ev: Ev,
}
impl PartialEq for Sched {
    fn eq(&self, o: &Self) -> bool {
        (self.t, self.n) == (o.t, o.n)
    }
}
impl Eq for Sched {}
impl PartialOrd for Sched {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Sched {
    fn cmp(&self, o: &Self) -> Ordering {
        (o.t, o.n).cmp(&(self.t, self.n))
    }
}

// ---------- devices

/// A session to a relay.
struct Conn {
    no: u64,
    sel: Sel,
    socket: Option<SocketId>,
    welcomed_t: Option<u64>,
    /// Socket messages are delivered in order (TCP): the time of the last one.
    sock_t: u64,
}

/// One process of one identity.
struct Proc {
    no: u32,
    engine: Eng,
    start_t: u64,
    session: [u8; 16],
    queue: VecDeque<Write>,
    writing: bool,
    tick_gen: u64,
    connect_gen: u64,
    conn: Option<Conn>,
    conn_no: u64,
    connecting: bool,
    /// The relay of the last `Connect`.
    target: Option<Sel>,
    /// Outstanding requests: key → (time sent, name).
    outstanding: BTreeMap<u64, (u64, &'static str)>,
    exiting: bool,
    hello_no: u64,
    last_tick: u64,
}

/// One device identity (a machine is paired again as a new identity).
pub(crate) struct Ident {
    machine: usize,
    name: String,
    rules2: bool,
    build: u32,
    pub(crate) dev: Dev,
    rng: SplitMix64,
    pub(crate) store: Store,
    proc: Option<Proc>,
    /// The fake for the very first `Start` (genesis or join).
    first: Option<FakeSealer>,
    nproc: u32,
    tokens: BTreeMap<usize, u32>,
    failures: u32,
    skip_backoff: bool,
    pub(crate) retired: bool,
    next_n: u64,
    pub(crate) joined_at: u64,
    /// Samples of the confirmed state: (B, C_B, board hash, line).
    pub(crate) samples: Vec<(u64, Hash32, Hash32, u64)>,
    last_b: (u64, Hash32),
    /// Alarms raised: (id, kind, seq, time).
    pub(crate) alarms: Vec<(u64, AlarmKind, Option<u64>, u64)>,
    alarm_lines: Vec<u64>,
    rejoin_confirmed: bool,
    /// The engine loaded no replica at a `Start` (it refused it).
    unloaded: bool,
    asked: BTreeSet<String>,
    resubmit: Option<(Vec<Vec<u8>>, u16)>,
    genesis: Option<(Vec<u8>, RelayTarget)>,
    max_alarm: u64,
    /// A contradicting welcome or page delivered with no alarm since: (line, what, L then).
    expect_alarm: Option<(u64, String, u64)>,
    /// The live outbox as emitted (entry id → record).
    live: BTreeMap<[u8; 16], OutboxRecord>,
    /// The held create: (ticket, item id, entry id once known).
    held: Option<(u64, u64, Option<[u8; 16]>)>,
    forget_ticket: Option<u64>,
    /// Roots and pending entry ids in the last identity written.
    last_roots: BTreeSet<u32>,
    last_pending: BTreeSet<[u8; 16]>,
    unsent: Option<(Vec<Vec<u8>>, u16)>,
    repair_started: bool,
    pub(crate) last_status: Option<crate::Status>,
}

struct Machine {
    ident: usize,
    awake: bool,
    phone: bool,
    /// A re-pair is waiting for this machine.
    repairing: bool,
}

#[derive(Clone, Debug)]
struct PendingPair {
    machine: usize,
    seed: u64,
    name: String,
    via: usize,
    replaces: Option<usize>,
}

pub(crate) struct World<'a> {
    cfg: &'a SimConfig,
    t: u64,
    n: u64,
    q: BinaryHeap<Sched>,
    net: SplitMix64,
    fault: SplitMix64,
    work: SplitMix64,
    pub(crate) trace: Trace,
    pub(crate) stats: SimStats,
    relays: Vec<ModelRelay>,
    relay_offset: Vec<i64>,
    fork: Option<(ModelRelay, BTreeSet<usize>)>,
    withheld: Option<(usize, u64)>,
    rollback_marks: BTreeMap<u32, ModelRelay>,
    /// Which relay is the reference (the board's current home).
    home: usize,
    pub(crate) idents: Vec<Ident>,
    machines: Vec<Machine>,
    skew: Vec<i64>,
    faults_on: bool,
    workload_on: bool,
    t_f: u64,
    t_end: u64,
    tickets: u64,
    pairs: BTreeMap<u64, PendingPair>,
    reg_tokens: BTreeMap<Dev, (usize, u32)>,
    /// The reference log (every row ever stored at the home relay, by seq), and other branches.
    pub(crate) archive: Vec<LogRow>,
    pub(crate) branches: Vec<BranchLog>,
    /// Frame hash → (entry id, kind, duty entry).
    frames: BTreeMap<Hash32, ([u8; 16], Kind, bool)>,
    /// Entry ids a relay acknowledged: (entry id, ident, line, from the reference branch).
    pub(crate) acked: Vec<([u8; 16], usize, u64, bool)>,
    /// `Enqueued` with an entry id: (entry id, ident, time, line).
    pub(crate) enqueued: Vec<([u8; 16], usize, u64, u64)>,
    pub(crate) cancelled: BTreeSet<[u8; 16]>,
    /// The first time an entry id was put in an outbox.
    pub(crate) first_put: BTreeMap<[u8; 16], u64>,
    /// Deletions to explain at the end (invariant 5): (ident, what, B then, line).
    pub(crate) deletions: Vec<(usize, Deletion, u64, u64)>,
    pub(crate) violations: Vec<Violation>,
    /// The observer template: the creator's genesis fake with the anchor.
    template: Option<FakeSealer>,
    pub(crate) roots: BTreeMap<u32, [u8; 32]>,
    pub(crate) injected_seqs: Vec<(u64, u64)>,
    move_code: Option<String>,
    moved: bool,
    seed: u64,
    pending_seeds: Vec<u64>,
    pairs_seed: BTreeMap<usize, (u64, String)>,
    op_tickets: BTreeMap<u64, &'static str>,
    /// A seed in progress on another relay: (relay, first_seq, last_seq).
    seeding: Option<(usize, u64, u64)>,
    faulty: Vec<Faulty>,
    /// The time of the last rollback or fork end: contradictions after it expect an alarm.
    contra_since: u64,
    pending_acks: PendingAcks,
}

/// A deletion a device emitted, to be explained (§31.7 invariant 5).
#[derive(Clone, Debug)]
pub(crate) enum Deletion {
    Truncate(u64, bool),
    Bases(Vec<u64>, bool),
    Roots(Vec<u32>),
    Pending(Vec<[u8; 16]>),
}

fn h(b: &[u8]) -> Hash32 {
    Sha256::digest(b).into()
}

/// An exponential-like draw with mean `mean` from integers only: a geometric
/// number of halvings plus a uniform fraction (no floating point, so a seed
/// gives the same run on every system).
fn exp_draw(r: &mut SplitMix64, mean: u64) -> u64 {
    let g = u64::from(r.next().leading_zeros().min(40));
    let x = g * 1024 + r.below(1024);
    (mean.saturating_mul(x) * 2 / 3072).max(1)
}

fn ev_name(e: &Event<ToyCommand>) -> &'static str {
    match e {
        Event::Start(_) => "start",
        Event::Shutdown => "shutdown",
        Event::Tick => "tick",
        Event::Persisted { .. } => "persisted",
        Event::PersistFailed { .. } => "persist_failed",
        Event::LocalRead { .. } => "local_read",
        Event::Enqueue { .. } => "enqueue",
        Event::ReleaseHold => "release_hold",
        Event::Op { .. } => "op",
        Event::PairRegister { .. } => "pair_register",
        Event::Resubmit { .. } => "resubmit",
        Event::User(_) => "user",
        Event::Connected { .. } => "connected",
        Event::MissedRoots { .. } => "missed_roots",
        Event::Welcome(_) => "welcome",
        Event::Page { .. } => "page",
        Event::AppendResults { .. } => "append_results",
        Event::AdminResult { .. } => "admin_result",
        Event::BlobResult { .. } => "blob_result",
        Event::RelayRefused { .. } => "relay_refused",
        Event::Second { .. } => "second",
        Event::Displaced { .. } => "displaced",
        Event::Disconnected { .. } => "disconnected",
    }
}

fn fx_name(e: &Effect) -> &'static str {
    match e {
        Effect::Persist { .. } => "persist",
        Effect::ReadLog { .. } => "read_log",
        Effect::ReadBase { .. } => "read_base",
        Effect::Connect { .. } => "connect",
        Effect::Disconnect => "disconnect",
        Effect::Hello => "hello",
        Effect::Read { .. } => "read",
        Effect::Append { .. } => "append",
        Effect::Admin { .. } => "admin",
        Effect::Blob { .. } => "blob",
        Effect::ScheduleTick { .. } => "schedule_tick",
        Effect::Notify(_) => "notify",
    }
}

fn note_name(n: &Notification) -> &'static str {
    match n {
        Notification::Enqueued { .. } => "enqueued",
        Notification::EnqueueRefused { .. } => "enqueue_refused",
        Notification::ViewChanged { .. } => "view_changed",
        Notification::StatusChanged => "status_changed",
        Notification::NoticesAdded { .. } => "notices_added",
        Notification::BannersChanged => "banners_changed",
        Notification::AlarmRaised { .. } => "alarm_raised",
        Notification::OpProgress { .. } => "op_progress",
        Notification::OpDone { .. } => "op_done",
        Notification::PairRegistered { .. } => "pair_registered",
        Notification::PairFailed { .. } => "pair_failed",
        Notification::MoveCode { .. } => "move_code",
        Notification::MoveReport { .. } => "move_report",
        Notification::MoveCodeResult { .. } => "move_code_result",
        Notification::Unsent { .. } => "unsent",
        Notification::ResubmitReport { .. } => "resubmit_report",
        Notification::ReplaceReady { .. } => "replace_ready",
        Notification::PurgeBackups { .. } => "purge_backups",
        Notification::ReuploadPicture { .. } => "reupload_picture",
        Notification::RecoveryKeyChanged { .. } => "recovery_key_changed",
        Notification::ReadyToExit => "ready_to_exit",
    }
}

pub(crate) fn alarm_name(k: AlarmKind) -> &'static str {
    match k {
        AlarmKind::RelayBehind => "relay_behind",
        AlarmKind::Fork => "fork",
        AlarmKind::Counter => "counter",
        AlarmKind::Unechoed => "unechoed",
        AlarmKind::Withheld => "withheld",
        AlarmKind::ForeignSigner => "foreign_signer",
        AlarmKind::IdentityClone => "identity_clone",
        AlarmKind::StateMismatch => "state_mismatch",
        AlarmKind::SnapshotMismatch => "snapshot_mismatch",
        AlarmKind::ResetMismatch => "reset_mismatch",
        AlarmKind::SkippedFrame => "skipped_frame",
        AlarmKind::RefusedFrame => "refused_frame",
        AlarmKind::RelayProtocol => "relay_protocol",
        AlarmKind::BlobFailure => "blob_failure",
        AlarmKind::RestoredCopy => "restored_copy",
    }
}

fn op_name(op: &Operation) -> &'static str {
    match op {
        Operation::RestoreTo { .. } => "restore_to",
        Operation::Purge => "purge",
        Operation::Move { .. } => "move",
        Operation::FollowAddress { .. } => "follow_address",
        Operation::Revoke { .. } => "revoke",
        Operation::Skip { .. } => "skip",
        Operation::RulesOverride { .. } => "rules_override",
        Operation::CloseBoard { .. } => "close_board",
        Operation::Adopt { .. } => "adopt",
    }
}

fn refusal(status: u16, code: &str) -> RelayRefusal {
    RelayRefusal { status, code: code.to_owned(), until: None, head: None, index: None }
}

/// Roots and pending entry ids of an identity export (api-fake-toy.md layout).
fn identity_keys(b: &[u8]) -> Option<(BTreeSet<u32>, BTreeSet<[u8; 16]>)> {
    let mut at = 4 + 4 + 16 + 32 + 32 + 32 + 16;
    let flag = *b.get(at)?;
    at += 1;
    if flag == 1 {
        at += 32 + 16 + 32;
    }
    let u32_at = |i: usize| -> Option<u32> { Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?)) };
    let n = u32_at(at)? as usize;
    at += 4;
    let mut roots = BTreeSet::new();
    for _ in 0..n {
        roots.insert(u32_at(at)?);
        at += 36;
    }
    let n = u32_at(at)? as usize;
    at += 4;
    let mut pending = BTreeSet::new();
    for _ in 0..n {
        pending.insert(b.get(at..at + 16)?.try_into().ok()?);
        at += 52;
    }
    Some((roots, pending))
}

pub(crate) fn run(seed: u64, cfg: &SimConfig, keep: bool) -> (SimReport, Vec<u8>) {
    let mut w = World::new(seed, cfg, keep);
    w.setup();
    while let Some(s) = w.q.pop() {
        w.t = s.t;
        if matches!(s.ev, Ev::End) {
            break;
        }
        w.step(s.ev);
    }
    w.finish_run()
}

impl<'a> World<'a> {
    fn new(seed: u64, cfg: &'a SimConfig, keep: bool) -> Self {
        let n = usize::from(cfg.devices.clamp(2, 4));
        let mut master = SplitMix64 { state: seed };
        // §31.2: one seed per device in index order, then network, faults, workload.
        let dev_seeds: Vec<u64> = (0..n).map(|_| master.next()).collect();
        let net = SplitMix64 { state: master.next() };
        let fault = SplitMix64 { state: master.next() };
        let work = SplitMix64 { state: master.next() };
        let t_f = u64::from(cfg.days.max(1)) * 24 * HOUR;
        let mut w = World {
            cfg,
            t: 0,
            n: 0,
            q: BinaryHeap::new(),
            net,
            fault,
            work,
            trace: Trace::new(keep),
            stats: SimStats::default(),
            relays: Vec::new(),
            relay_offset: Vec::new(),
            fork: None,
            withheld: None,
            rollback_marks: BTreeMap::new(),
            home: 0,
            idents: Vec::new(),
            machines: Vec::new(),
            skew: vec![0; n],
            faults_on: true,
            workload_on: true,
            t_f,
            t_end: t_f + SETTLE,
            tickets: 0,
            pairs: BTreeMap::new(),
            reg_tokens: BTreeMap::new(),
            archive: Vec::new(),
            branches: Vec::new(),
            frames: BTreeMap::new(),
            acked: Vec::new(),
            enqueued: Vec::new(),
            cancelled: BTreeSet::new(),
            first_put: BTreeMap::new(),
            deletions: Vec::new(),
            violations: Vec::new(),
            template: None,
            roots: BTreeMap::new(),
            injected_seqs: Vec::new(),
            move_code: None,
            moved: false,
            seed,
            pending_seeds: Vec::new(),
            pairs_seed: BTreeMap::new(),
            op_tickets: BTreeMap::new(),
            seeding: None,
            faulty: Vec::new(),
            contra_since: 0,
            pending_acks: BTreeMap::new(),
        };
        for i in 0..n {
            w.machines.push(Machine { ident: usize::MAX, awake: true, phone: i + 1 == n && n > 2, repairing: false });
        }
        w.pending_seeds = dev_seeds;
        w
    }

    fn at(&mut self, t: u64, ev: Ev) {
        self.n += 1;
        self.q.push(Sched { t, n: self.n, ev });
    }

    fn wall(&self, machine: usize) -> i64 {
        WALL0 + self.t as i64 + self.skew[machine]
    }

    fn relay_now(&self, r: usize) -> i64 {
        WALL0 + self.t as i64 + self.relay_offset.get(r).copied().unwrap_or(0)
    }

    fn sel_now(&self, sel: Sel) -> i64 {
        match sel {
            Sel::Main(r) => self.relay_now(r),
            Sel::Fork => self.relay_now(0),
        }
    }

    fn relay(&mut self, sel: Sel) -> &mut ModelRelay {
        match sel {
            Sel::Main(r) => &mut self.relays[r],
            Sel::Fork => match &mut self.fork {
                Some((r, _)) => r,
                None => &mut self.relays[0],
            },
        }
    }

    fn sel_of_host(&self, id: usize, host: &str) -> Option<Sel> {
        let r = HOSTS.iter().position(|h| *h == host)?;
        if r >= self.relays.len() {
            return None;
        }
        if r == 0 && self.fork.as_ref().is_some_and(|(_, s)| s.contains(&id)) {
            return Some(Sel::Fork);
        }
        Some(Sel::Main(r))
    }

    fn fault_line(&mut self, kind: &'static str, who: (&str, i128), p: &[i64]) -> u64 {
        *self.stats.faults.entry(kind).or_default() += 1;
        self.trace.line(self.t, "fault", &[("kind", V::S(kind)), (who.0, V::I(who.1)), ("p", V::Ints(p))])
    }

    fn relay_line(&mut self, sel: Sel, op: &str) {
        let r = match sel {
            Sel::Main(r) => r as i128,
            Sel::Fork => 2,
        };
        let head = self.relay(sel).head().unwrap_or_default();
        self.trace.line(
            self.t,
            "relay",
            &[("r", V::I(r)), ("op", V::S(op)), ("head", V::I(i128::from(head.seq))), ("chain", V::Hex(&head.chain))],
        );
        if sel == Sel::Main(self.home) {
            self.sync_archive();
        }
    }

    /// Appends the home relay's new rows to the reference log.
    fn sync_archive(&mut self) {
        let next = self.archive.len() as u64 + 1;
        let rows: Vec<LogRow> = self.relays[self.home].log().range(next..).map(|(_, r)| r.clone()).collect();
        for r in rows {
            if r.seq == self.archive.len() as u64 + 1 {
                self.archive.push(r);
            }
        }
    }

    // ---------- setup

    fn setup(&mut self) {
        let n = self.machines.len();
        let seeds = std::mem::take(&mut self.pending_seeds);
        for _ in 0..2 {
            let off = self.net.below(2_001) as i64;
            self.relay_offset.push(off);
        }
        // Machine 0 creates the board (§19.8): the fake's genesis, the relay's create.
        let rules2 = self.rules2(0);
        let (mut fake, f1, board) = FakeSealer::create_at(seeds[0], "Desk", self.wall(0));
        if rules2 {
            fake.set_supported_rules(&[1, 2]);
        }
        let (rsign_pk, salt) = fake.genesis_keys().unwrap_or_default();
        let board_id = crate::sim::relay::fake_board_id(&rsign_pk, &salt);
        let mut relay = ModelRelay::new(board_id, [1; 32]);
        let dev = fake.own_dev();
        let sign_pk = self.sign_pk_of(&fake);
        let now = self.relay_now(0);
        let reply = relay.create(now, &rsign_pk, &salt, &dev, &sign_pk, &f1);
        let tgen = reply.token_gen.unwrap_or(1);
        if let Err(e) = &reply.result {
            self.violations.push(Violation { invariant: 0, line: 0, detail: format!("relay create refused: {}", e.code) });
        }
        let door = relay.door();
        self.relays.push(relay);
        let mut template = fake.observer(seeds[0] ^ 0x5eed_0b5e_77e7, &[]);
        if let Some(t) = &mut template {
            t.set_supported_rules(&[1, 2]);
        }
        self.template = template;
        let target = RelayTarget { host: HOSTS[0].to_owned(), door, home_gen: 1 };
        let identity = fake.export_identity().to_vec();
        let history = fake.history();
        for (e, r) in fake.roots() {
            self.roots.insert(e, r);
        }
        let store = Store::new(identity, history, Bootstrap::Genesis { board: board.clone(), frame_1: f1, relay: target.clone() });
        let id = self.new_ident(0, "Desk".to_owned(), seeds[0], fake, store, 0);
        self.idents[id].genesis = Some((board, target));
        self.idents[id].tokens.insert(0, tgen);
        self.relay_line(Sel::Main(0), "create");
        self.at(0, Ev::Start { id });
        // The other machines pair through machine 0 (§28.4), one minute apart.
        for (m, s) in seeds.iter().enumerate().skip(1) {
            let name = ["Desk", "Laptop", "Tablet", "Phone"][if self.machines[m].phone { 3 } else { m.min(2) }];
            self.pairs_seed.insert(m, (*s, name.to_owned()));
            self.at(60_000 * m as u64, Ev::Pair { machine: m, replaces: None });
        }
        for m in 0..n {
            let first = 120_000 + exp_draw(&mut self.work, 600_000);
            self.at(first, Ev::Work { machine: m });
            self.at(300_000 + 1_000 * m as u64, Ev::Poll { machine: m });
        }
        self.schedule_sleep();
        self.schedule_faults();
        self.schedule_ops();
        self.at(self.t_f, Ev::Settle);
        self.at(self.t_end, Ev::End);
    }

    fn rules2(&self, machine: usize) -> bool {
        self.cfg.rules2.get(machine).copied().unwrap_or(false)
    }

    fn sign_pk_of(&self, fake: &FakeSealer) -> Hash32 {
        let id = fake.export_identity();
        let mut pk = [0u8; 32];
        if id.len() >= 88 {
            pk.copy_from_slice(&id[56..88]);
        }
        pk
    }

    fn new_ident(&mut self, machine: usize, name: String, seed: u64, fake: FakeSealer, store: Store, joined_at: u64) -> usize {
        let id = self.idents.len();
        let dev = fake.own_dev();
        let (roots, pending) = identity_keys(&store.identity).unwrap_or_default();
        self.idents.push(Ident {
            machine,
            name,
            rules2: self.rules2(machine),
            build: 1,
            dev,
            rng: SplitMix64 { state: seed ^ 0xd1ce_5eed_0000_0001 },
            store,
            proc: None,
            first: Some(fake),
            nproc: 0,
            tokens: BTreeMap::new(),
            failures: 0,
            skip_backoff: false,
            retired: false,
            next_n: 1,
            joined_at,
            samples: Vec::new(),
            last_b: (0, [0; 32]),
            alarms: Vec::new(),
            alarm_lines: Vec::new(),
            rejoin_confirmed: false,
            unloaded: false,
            asked: BTreeSet::new(),
            resubmit: None,
            genesis: None,
            max_alarm: 0,
            expect_alarm: None,
            live: BTreeMap::new(),
            held: None,
            forget_ticket: None,
            last_roots: roots,
            last_pending: pending,
            unsent: None,
            repair_started: false,
            last_status: None,
        });
        self.machines[machine].ident = id;
        id
    }

    fn schedule_sleep(&mut self) {
        let n = self.machines.len();
        // Desktops sleep and wake on drawn schedules; the phone is awake most of
        // the time, and once every desktop sleeps for a long stretch while the
        // phone stays awake (§29.4, "a week with only the phone awake", scaled).
        let phone_only = if n > 2 {
            let start = 4 * HOUR + self.work.below(self.t_f.saturating_sub(10 * HOUR).max(1));
            Some((start, start + 4 * HOUR + self.work.below(2 * HOUR)))
        } else {
            None
        };
        for m in 0..n {
            let mut t = HOUR + self.work.below(4 * HOUR);
            while t < self.t_f {
                let (awake_mean, sleep_mean) = if self.machines[m].phone { (8 * HOUR, HOUR / 2) } else { (5 * HOUR, 2 * HOUR) };
                let sleep_len = exp_draw(&mut self.work, sleep_mean).min(6 * HOUR);
                if m == 0 && t < 2 * HOUR {
                    t += HOUR;
                    continue;
                }
                self.at(t, Ev::Sleep { machine: m });
                let wake = (t + sleep_len).min(self.t_f);
                self.at(wake, Ev::Wake { machine: m });
                t = wake + exp_draw(&mut self.work, awake_mean).max(HOUR / 2);
            }
            if let Some((a, b)) = phone_only
                && !self.machines[m].phone {
                    self.at(a, Ev::Sleep { machine: m });
                    self.at(b.min(self.t_f), Ev::Wake { machine: m });
                }
        }
    }

    fn schedule_faults(&mut self) {
        let f = self.cfg.faults.clone();
        let span = self.t_f.max(1);
        if f.disconnects_per_hour > 0 {
            let t = exp_draw(&mut self.fault, HOUR / u64::from(f.disconnects_per_hour));
            self.at(t, Ev::Fault(FaultEv::Disconnect));
        }
        if f.skew_ms != 0 {
            let n = self.machines.len();
            for m in 0..n {
                let s = self.draw_skew();
                self.skew[m] = s;
            }
            let t = exp_draw(&mut self.fault, 24 * HOUR);
            self.at(t, Ev::Fault(FaultEv::SkewJump));
        }
        for i in 0..f.rollbacks.min(2) {
            let a = 2 * HOUR + self.fault.below(span.saturating_sub(5 * HOUR).max(1));
            let b = a + 600_000 + self.fault.below(2 * HOUR);
            self.at(a, Ev::Fault(FaultEv::RollbackMark(i)));
            self.at(b.min(self.t_f), Ev::Fault(FaultEv::Rollback(i)));
        }
        if f.fork {
            let a = 3 * HOUR + self.fault.below(span.saturating_sub(6 * HOUR).max(1));
            self.at(a, Ev::Fault(FaultEv::ForkStart));
            let b = a + HOUR + self.fault.below(2 * HOUR);
            self.at(b.min(self.t_f), Ev::Fault(FaultEv::ForkEnd));
        }
        if f.withhold {
            let a = 2 * HOUR + self.fault.below(span.saturating_sub(5 * HOUR).max(1));
            self.at(a, Ev::Fault(FaultEv::WithholdStart));
            let b = a + HOUR + self.fault.below(2 * HOUR);
            self.at(b.min(self.t_f), Ev::Fault(FaultEv::WithholdEnd));
        }
        for i in 0..f.injected.min(3) {
            let a = HOUR + self.fault.below(span.saturating_sub(2 * HOUR).max(1));
            self.at(a, Ev::Fault(FaultEv::Inject(i)));
        }
    }

    fn draw_skew(&mut self) -> i64 {
        let s = self.cfg.faults.skew_ms.unsigned_abs().min(7_200_000);
        if s == 0 {
            return 0;
        }
        self.fault.below(2 * s + 1) as i64 - s as i64
    }

    fn schedule_ops(&mut self) {
        let o = self.cfg.ops.clone();
        let span = self.t_f.saturating_sub(4 * HOUR).max(1);
        let later = |w: &mut Self, k: OpKind, count: u32| {
            for _ in 0..count {
                let t = 2 * HOUR + w.work.below(span);
                w.at(t, Ev::OpAt(k));
            }
        };
        later(self, OpKind::Revoke, o.revoke);
        later(self, OpKind::Purge, o.purge);
        later(self, OpKind::Restore, o.restore);
        later(self, OpKind::Recovery, o.recovery_key);
        later(self, OpKind::Poison, o.poison);
        later(self, OpKind::Move, u32::from(o.move_relay));
        later(self, OpKind::RulesBuild, u32::from(o.rules_upgrade));
    }
}

#[cfg(test)]
mod tests {
    //! The checks find what they are meant to find (ENG-SIM-002): a quiet run
    //! holds every invariant, and the same run with one fact tampered breaks
    //! exactly the invariant that fact belongs to.
    use super::*;

    fn tampered(f: fn(&mut World<'_>)) -> Vec<u8> {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let mut w = World::new(3, &cfg, false);
        w.setup();
        while let Some(s) = w.q.pop() {
            w.t = s.t;
            if matches!(s.ev, Ev::End) {
                break;
            }
            w.step(s.ev);
        }
        f(&mut w);
        let (r, _) = w.finish_run();
        let mut invs: Vec<u8> = r.violations.iter().map(|v| v.invariant).collect();
        invs.dedup();
        invs
    }

    #[test]
    fn a_quiet_run_holds_every_invariant() {
        assert_eq!(tampered(|_| {}), Vec::<u8>::new());
    }

    // Invariant 1: a device whose state is not the oracle's at its B.
    #[test]
    fn a_lost_reference_entry_breaks_convergence() {
        assert!(tampered(|w| {
            w.archive.pop();
        })
        .contains(&1));
    }

    // Invariant 3: an enqueued entry that never reached the log.
    #[test]
    fn a_vanished_edit_breaks_queued_edits() {
        assert_eq!(tampered(|w| w.enqueued.push(([7; 16], 0, 1, 1))), vec![3]);
    }

    // Invariant 2: an acknowledged entry that is not in the reference log.
    #[test]
    fn a_lost_acknowledgement_breaks_acknowledged_entries() {
        assert_eq!(tampered(|w| w.acked.push(([9; 16], 1, 1, true))), vec![2]);
    }

    // Invariant 4: a confirmed state that differs from the oracle's, and a
    // contradiction with no alarm.
    #[test]
    fn a_wrong_state_or_a_silent_contradiction_breaks_tampering() {
        assert_eq!(
            tampered(|w| {
                let j = w.idents[1].joined_at + 1;
                let s = w.idents[1].samples.iter_mut().rev().find(|s| s.0 > j).expect("a sample");
                s.2 = [0xee; 32];
            }),
            vec![4]
        );
        assert_eq!(tampered(|w| w.idents[0].expect_alarm = Some((5, "a test".to_owned(), 3))), vec![4]);
        assert_eq!(tampered(|w| w.injected_seqs.push((10, 5))), vec![4]);
    }

    // Invariant 5: a truncation no rule explains.
    #[test]
    fn an_unexplained_truncation_breaks_no_deletion() {
        assert_eq!(tampered(|w| w.deletions.push((0, Deletion::Truncate(40, false), 100, 9))), vec![5]);
        assert_eq!(tampered(|w| w.deletions.push((0, Deletion::Roots(vec![1]), 100, 9))), vec![5]);
    }

    // Invariant 6: a request outstanding too long, a device not caught up.
    #[test]
    fn an_old_request_or_a_lagging_device_breaks_settling() {
        assert_eq!(
            tampered(|w| {
                if let Some(p) = w.idents[2].proc.as_mut() {
                    p.outstanding.insert(77, (0, "read"));
                }
            }),
            vec![6]
        );
        assert!(tampered(|w| {
            w.idents[2].proc = None;
        })
        .contains(&6));
    }
}
