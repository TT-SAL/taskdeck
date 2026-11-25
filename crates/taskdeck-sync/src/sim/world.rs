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
use super::counted::CountedToy;
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

pub(crate) type Eng = Engine<CountedToy, FakeSealer>;

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
/// §31.7 invariant 6: the end of the run in which only beats and duty entries are appended.
const SETTLE_QUIET: u64 = 1_800_000;
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
}

/// A test's change to the end-of-run facts (`World::tamper_facts`).
#[cfg(test)]
pub(crate) type TamperFacts = Box<dyn Fn(&mut [check::IdentFacts])>;

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
    /// The seqs of the crash halts this identity's storage held (§16.2): the
    /// one kind of device that may confirm a `B` in `t..s` without `t`.
    pub(crate) crash_halts: BTreeSet<u64>,
    last_b: (u64, Hash32),
    /// Alarms raised: (id, kind, seq, time).
    pub(crate) alarms: Vec<(u64, AlarmKind, Option<u64>, u64)>,
    alarm_lines: Vec<u64>,
    /// A confirmed rejoin in progress (§19.7): the checkpoints written since
    /// `ConfirmRejoin` (seq → chain), and whether its base deletion was used.
    /// It explains exactly the rejoin's own `TruncateLogBelow(S + 1, Some(C_S))`
    /// and one `DeleteBases`, then ends.
    rejoin: Option<(BTreeMap<u64, Hash32>, bool)>,
    /// Calls delivered to this identity's engine (the pairing ruling counts them).
    calls: u64,
    /// Register records dropped and not yet explained: (dev, call, line).
    pair_drops: Vec<(Dev, u64, u64)>,
    /// `PairFailed` notifications: (dev, call).
    pair_failed: Vec<(Dev, u64)>,
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
    /// The entry ids of pairings' `DeviceAdded` frames that a relay's
    /// `register` acknowledged with a seq (§28.4 step 3).
    pub(crate) registered: BTreeSet<[u8; 16]>,
    /// A test's change to the end-of-run facts of each identity, before the checks.
    #[cfg(test)]
    pub(crate) tamper_facts: Option<TamperFacts>,
    /// The first time an entry id was put in an outbox.
    pub(crate) first_put: BTreeMap<[u8; 16], u64>,
    /// Deletions to explain at the end (invariant 5): (ident, what, B then, line).
    pub(crate) deletions: Vec<(usize, Deletion, u64, u64)>,
    pub(crate) violations: Vec<Violation>,
    /// The observer template: the creator's genesis fake with the anchor.
    template: Option<FakeSealer>,
    /// Every `(epoch, root)` any device held (two branches may rotate to one
    /// epoch with two roots: `check::pick_roots`).
    pub(crate) roots: BTreeSet<(u32, [u8; 32])>,
    /// Injected frames: (seq, chain there, line).
    pub(crate) injected_seqs: Vec<(u64, Hash32, u64)>,
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
    /// The `Enqueue` of a `Forget` being delivered: (identity, ticket, item id).
    cur_forget: Option<(usize, u64, u64)>,
    /// The device each pairing ticket registers (kept after the ticket ends).
    pair_devs: BTreeMap<u64, Dev>,
    /// The trace line of the current fork's `relay_fork` fault.
    fork_line: u64,
    /// The identity each completed pairing ticket started (`PairRegistered`).
    paired: BTreeMap<u64, usize>,
}

/// A deletion a device emitted, to be explained (§31.7 invariant 5).
#[derive(Clone, Debug)]
pub(crate) enum Deletion {
    Truncate(u64, bool),
    Bases(Vec<u64>, bool),
    Roots(Vec<u32>),
    /// Pending roots dropped by an identity write, with the chain the device
    /// holds at the highest seq that write makes durable (which branch it
    /// followed there: a fork's copy can sequence a rotation the reference
    /// log never holds).
    Pending(Vec<[u8; 16]>, Option<Hash32>),
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
            #[cfg(test)]
            tamper_facts: None,
            registered: BTreeSet::new(),
            first_put: BTreeMap::new(),
            deletions: Vec::new(),
            violations: Vec::new(),
            template: None,
            roots: BTreeSet::new(),
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
            cur_forget: None,
            pair_devs: BTreeMap::new(),
            fork_line: 0,
            paired: BTreeMap::new(),
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
        // §31.9: `r` is 0 or 1; a fork's copy is relay 0's, its ops are named `fork_…`.
        let (r, op) = match sel {
            Sel::Main(r) => (r as i128, op.to_owned()),
            Sel::Fork => (0, format!("fork_{op}")),
        };
        let head = self.relay(sel).head().unwrap_or_default();
        self.trace.line(
            self.t,
            "relay",
            &[("r", V::I(r)), ("op", V::S(&op)), ("head", V::I(i128::from(head.seq))), ("chain", V::Hex(&head.chain))],
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
            // No board, so no device settles (§31.7 invariant 6); reported as such so the regression file parses (§31.8).
            self.violations.push(Violation { invariant: 6, line: 0, detail: format!("relay create refused: {}", e.code) });
        }
        let door = relay.door();
        self.relays.push(relay);
        let mut template = fake.observer(seeds[0] ^ 0x5eed_0b5e_77e7, &[]);
        if let Some(t) = &mut template {
            t.set_supported_rules(&check::ORACLE_RULES);
        }
        self.template = template;
        let target = RelayTarget { host: HOSTS[0].to_owned(), door, home_gen: 1 };
        let identity = fake.export_identity().to_vec();
        let history = fake.history();
        for (e, r) in fake.roots() {
            self.roots.insert((e, r));
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
            crash_halts: BTreeSet::new(),
            last_b: (0, [0; 32]),
            alarms: Vec::new(),
            alarm_lines: Vec::new(),
            rejoin: None,
            calls: 0,
            pair_drops: Vec::new(),
            pair_failed: Vec::new(),
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

    /// The violations of quiet seed 3 run to its end, with `f` applied before the checks.
    fn quiet_violations(f: impl FnOnce(&mut World<'_>)) -> Vec<(u8, String)> {
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
        r.violations.into_iter().map(|v| (v.invariant, v.detail)).collect()
    }

    /// The invariants `f` breaks: those of the violations the tampered run
    /// has and the same run untampered has not (task D4: a tampering test
    /// holds whatever the engine does at the seed).
    fn tampered(f: impl FnOnce(&mut World<'_>)) -> Vec<u8> {
        let mut new = quiet_violations(f);
        for b in quiet_violations(|_| {}) {
            if let Some(i) = new.iter().position(|x| *x == b) {
                new.remove(i);
            }
        }
        let mut invs: Vec<u8> = new.iter().map(|v| v.0).collect();
        invs.sort_unstable();
        invs.dedup();
        invs
    }

    // The one test here that asks the engine something: a run with no fault
    // and no operation holds every invariant (the tampering tests below do
    // not lean on it: they compare with the untampered run).
    #[test]
    fn a_quiet_run_holds_every_invariant() {
        assert_eq!(quiet_violations(|_| {}), Vec::new());
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
        assert_eq!(
            tampered(|w| {
                let c = w.archive[9].chain;
                w.injected_seqs.push((10, c, 5));
            }),
            vec![4]
        );
    }

    // Invariant 5: a truncation no rule explains.
    #[test]
    fn an_unexplained_truncation_breaks_no_deletion() {
        assert_eq!(tampered(|w| w.deletions.push((0, Deletion::Truncate(40, false), 100, 9))), vec![5]);
        assert_eq!(tampered(|w| w.deletions.push((0, Deletion::Roots(vec![1]), 100, 9))), vec![5]);
    }

    // Invariant 5: the drop of a pending root no device ever sealed is
    // unexplained (`sim::script` checks the drop of a rotation's own root
    // against accepted rotations).
    #[test]
    fn a_pending_root_drop_needs_its_own_rotation() {
        let bogus = |w: &mut World<'_>| {
            let b = w.archive.last().map_or(0, |r| r.seq);
            w.deletions.push((0, Deletion::Pending(vec![[0xEE; 16]], None), b, 9));
        };
        assert_eq!(tampered(bogus), vec![5]);
    }

    /// An outbox record of identity `id` the quiet run put, to build test records from.
    fn some_record(w: &World<'_>, id: usize) -> OutboxRecord {
        w.idents[id]
            .store
            .outbox
            .iter()
            .find_map(|o| if let OutboxOp::Put(r) = o { Some(r.clone()) } else { None })
            .expect("a record")
    }

    // Invariant 2 has no exemption (§31.7; the D2 brief): an acknowledged entry
    // still in the outbox of a retired, halted or revoked device breaks it.
    #[test]
    fn a_lost_acknowledgement_is_never_excused() {
        assert_eq!(
            tampered(|w| {
                let mut r = some_record(w, 1);
                r.entry_id = [9; 16];
                w.idents[1].store.outbox.push(OutboxOp::Put(r));
                w.idents[1].retired = true;
                w.acked.push(([9; 16], 1, 1, true));
            }),
            vec![2]
        );
    }

    // Ruling 1 of the D2 brief: an acknowledged entry that only a rolled-back
    // relay state held is reported as such, with the fault's line.
    #[test]
    fn an_entry_lost_to_a_rollback_is_named() {
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
        let roots: Vec<(u32, [u8; 32])> = w.roots.iter().copied().collect();
        let led = check::ledger(&w.archive, w.template.as_ref(), &roots);
        let eid = led.iter().rev().find(|l| l.accept).and_then(|l| l.entry_id).expect("an entry");
        let seq = led.iter().rev().find(|l| l.entry_id == Some(eid)).map(|l| l.seq).expect("its seq");
        w.branches.push(BranchLog { rows: w.archive.clone(), why: check::WHY_ROLLBACK, line: 77 });
        w.archive.truncate(seq as usize - 1);
        w.acked.push((eid, 0, 1, true));
        let (r, _) = w.finish_run();
        assert!(
            r.violations.iter().any(|v| v.invariant == 2 && v.detail.contains("lost to a rolled-back relay state") && v.detail.contains("line 77")),
            "{:?}",
            r.violations.iter().map(|v| &v.detail).collect::<Vec<_>>()
        );
    }

    // Invariant 3 (ruling 5): only the record dropped as a cancelled hold is accepted.
    #[test]
    fn an_edit_marked_cancelled_without_its_drop_is_not_excused() {
        assert_eq!(tampered(|w| w.enqueued.push(([7; 16], 0, 1, 1))), vec![3]);
        assert_eq!(
            tampered(|w| {
                w.enqueued.push(([7; 16], 0, 1, 1));
                w.cancelled.insert([7; 16]);
            }),
            Vec::<u8>::new()
        );
    }

    // Invariant 5: a confirmed rejoin explains its own truncation (§19.7 step 4:
    // `TruncateLogBelow(S + 1, Some(C_S))` after a checkpoint at `S`) and one
    // base deletion, once; nothing after it.
    #[test]
    fn a_rejoin_explains_only_its_own_deletions() {
        let rejoin = |w: &mut World<'_>, twice: bool| {
            let mut c = w.idents[1].store.checkpoint.clone().expect("a checkpoint");
            c.board_seq = 39;
            let chain = c.board_chain;
            w.idents[1].rejoin = Some((BTreeMap::new(), false));
            w.note_persist(1, &[PersistOp::WriteCheckpoint(c), PersistOp::DeleteBases(vec![1])], 9);
            w.note_persist(1, &[PersistOp::TruncateLogBelow(40, Some(chain))], 9);
            if twice {
                w.note_persist(1, &[PersistOp::TruncateLogBelow(40, Some(chain)), PersistOp::DeleteBases(vec![2])], 9);
            }
        };
        assert_eq!(tampered(|w| rejoin(w, false)), Vec::<u8>::new());
        assert_eq!(tampered(|w| rejoin(w, true)), vec![5]);
        // Without the confirmation, the same truncation is unexplained.
        assert_eq!(
            tampered(|w| {
                let chain = w.idents[1].store.checkpoint.as_ref().expect("a checkpoint").board_chain;
                w.note_persist(1, &[PersistOp::TruncateLogBelow(40, Some(chain))], 9);
            }),
            vec![5]
        );
    }

    /// Puts `r` in identity 1's live outbox and drops it, as the engine would.
    fn drop_record(w: &mut World<'_>, r: OutboxRecord) {
        let eid = r.entry_id;
        w.idents[1].live.insert(eid, r);
        w.explain_drop(1, &eid, 0, 9);
    }

    fn unsent(w: &World<'_>, eid: u8) -> OutboxRecord {
        let mut r = some_record(w, 1);
        r.entry_id = [eid; 16];
        (r.frame, r.frame_hash, r.ack, r.seen) = (None, None, None, None);
        r
    }

    // Invariant 5 (ruling 2): a pairing's `DeviceAdded` dropped needs a
    // `PairFailed` for that device in the same or the next call.
    #[test]
    fn a_register_drop_needs_its_pair_failed() {
        let reg = |w: &World<'_>| {
            let mut r = unsent(w, 0x51);
            r.via = crate::Via::Register { dev: [0x44; 16], sign_pk: [0; 32] };
            r
        };
        assert_eq!(
            tampered(|w| {
                let r = reg(w);
                drop_record(w, r);
            }),
            vec![5]
        );
        assert_eq!(
            tampered(|w| {
                let r = reg(w);
                drop_record(w, r);
                let call = w.idents[1].calls + 1;
                w.idents[1].pair_failed.push(([0x44; 16], call));
            }),
            Vec::<u8>::new()
        );
    }

    // Invariant 5: a held record's drop is a cancelled hold only during the
    // `Enqueue` of the `Forget` of its item (`Coalesce::CancelBoth`, §9.3).
    #[test]
    fn a_held_drop_needs_its_forget() {
        let held = |w: &World<'_>| {
            let mut r = unsent(w, 0x61);
            r.hold = true;
            r.body = Body::Cmd(ToyMachine::new(true).encode(&ToyCommand::Create { id: 5, name: 1 }));
            r
        };
        assert_eq!(
            tampered(|w| {
                let r = held(w);
                drop_record(w, r);
            }),
            vec![5]
        );
        assert_eq!(
            tampered(|w| {
                let r = held(w);
                w.cur_forget = Some((1, 99, 5));
                drop_record(w, r);
                w.cur_forget = None;
                assert!(w.cancelled.contains(&[0x61; 16]));
            }),
            Vec::<u8>::new()
        );
    }

    // Invariant 6: in the last 1,800,000 ms only beats and duty entries are appended.
    #[test]
    fn a_late_append_of_an_edit_breaks_settling() {
        let late = |w: &mut World<'_>, duty: bool| {
            let f = w
                .archive
                .iter()
                .find(|r| w.frames.get(&h(&r.frame)).is_some_and(|x| x.2 == duty))
                .map(|r| r.frame.clone())
                .expect("a frame");
            w.t = w.t_end - 1_000;
            w.settle_append(1, &[f], 9);
        };
        assert_eq!(tampered(|w| late(w, false)), vec![6]);
        assert_eq!(tampered(|w| late(w, true)), Vec::<u8>::new());
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

    // Invariant 2, second half: every device whose B is past an acknowledged
    // entry's seq has applied it exactly once (`counted` keeps what each
    // device's confirmed state applied).
    #[test]
    fn an_entry_applied_twice_or_never_breaks_acknowledged_entries() {
        fn twice(x: &mut [check::IdentFacts]) {
            for f in x {
                if let Some(&s) = f.applied.last() {
                    f.applied.push(s);
                }
            }
        }
        fn never(x: &mut [check::IdentFacts]) {
            for f in x {
                if f.applied.len() > 1 {
                    f.applied.pop();
                }
            }
        }
        assert_eq!(tampered(|w| w.tamper_facts = Some(Box::new(twice))), vec![2]);
        assert_eq!(tampered(|w| w.tamper_facts = Some(Box::new(never))), vec![2]);
    }

    // §31.4: the runtime opens its socket from the login's answer. A push
    // the relay sends on the socket of a login whose answer has not arrived
    // (lost, or still on its way) reaches no engine. Before task D3 it did
    // (gate seed 1076: device 2's login answer was lost after a rollback, the
    // pushes of a restore tail reached its engine while it was connecting,
    // and invariant 4 asked for a `RelayBehind` alarm for a page it ignored).
    #[test]
    fn a_push_before_the_login_answer_reaches_no_engine() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let mut w = World::new(3, &cfg, false);
        w.setup();
        let welcomed = |w: &World<'_>| w.idents[0].proc.as_ref().and_then(|p| p.conn.as_ref()).is_some_and(|c| c.welcomed_t.is_some());
        while let Some(s) = w.q.pop() {
            w.t = s.t;
            if matches!(s.ev, Ev::End) {
                break;
            }
            w.step(s.ev);
            if w.t > 600_000 && welcomed(&w) {
                break;
            }
        }
        assert!(welcomed(&w));
        let (seq, chain) = w.idents[0].store.head().expect("a log");
        let page = Page { head: Head { seq, chain }, relay_now: 0, floor: 0, purge_floor: 0, more: false, truncated: false, fenced: false, entries: vec![] };
        let pages = |w: &World<'_>| w.stats.events.get("page").copied().unwrap_or(0);
        let p = w.idents[0].proc.as_ref().expect("running");
        let (proc, conn) = (p.no, p.conn_no);
        let before = pages(&w);
        w.down(0, proc, conn, Down::Sock(SockMsg::Push(page.clone())));
        assert_eq!(pages(&w), before + 1, "a push on a session whose login was answered");
        if let Some(c) = w.idents[0].proc.as_mut().and_then(|p| p.conn.as_mut()) {
            c.welcomed_t = None;
        }
        w.down(0, proc, conn, Down::Sock(SockMsg::Push(page)));
        assert_eq!(pages(&w), before + 1, "a push before the login's answer");
    }
}
