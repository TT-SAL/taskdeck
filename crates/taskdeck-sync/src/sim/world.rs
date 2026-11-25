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

use super::check::{self, AckFact, Answer, BranchLog, Crossing, EvidenceAlarm, Facts, MadeUp, RestoreTry};
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
/// How long a device stays in session with a relay that holds a prefix of its
/// durable log, with no restore tail sent or on its way, before the restore
/// tail §21.3 has it send counts as owed (`World::owed`, task D6): the
/// response timeout, the time a request it sent then would have had to be
/// answered. An engine that sends one at once (ENG-EVID-004, ENG-EFFECT-001:
/// a `ReadLog`, then the request) takes milliseconds.
const RESTORE_DUE: u64 = TIMEOUT;
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
    /// `from`: the relay state that produced the message, and the trace line then.
    Down { id: usize, proc: u32, conn: u64, msg: Down, from: (Sel, u64) },
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
    /// The `Read`s this process emitted whose answer has not reached its
    /// engine yet: a page with another request id is one the engine ignores
    /// (ENG-EFFECT-002), so it is no relay head the device saw (task D5).
    reads_open: BTreeSet<u64>,
    /// The confirmed `B` this process last showed (at its `Start`, the board
    /// seq it loaded): where its next `Crossing` starts (task D6).
    b_from: u64,
    /// Since when this process owes the home relay a restore tail (§21.3,
    /// `World::owe_from`), and that restore tail as it would have met the
    /// relay then (task D6).
    owe: Option<(u64, RestoreTry)>,
}

/// One `Persist` of one process: (identity, process number, persist id).
pub(super) type WriteKey = (usize, u32, u64);

/// What a deletion of invariant 5 tells the checks, held until the write
/// that carries it is applied to durable storage (task D5): a write that a
/// crash or an exit cut off before it was applied deletes nothing.
pub(super) enum Staged {
    Deletion((usize, Deletion, u64, u64)),
    Violation(Violation),
    Cancelled([u8; 16]),
    /// An alarm of `check::is_evidence`'s kinds: (identity, kind, its trace
    /// line). It counts once durable, with the frames held then (task D6).
    Alarm(usize, AlarmKind, u64),
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
    /// How its confirmed head moved within each process, with the class head
    /// then (§31.7 invariant 4: whether it applied a Skip's target, task D6).
    pub(crate) crossings: Vec<Crossing>,
    /// The seqs of the crash halts this identity's storage held (§16.2).
    pub(crate) crash_halts: BTreeSet<u64>,
    last_b: (u64, Hash32),
    /// Alarms raised: (id, kind, seq, time).
    pub(crate) alarms: Vec<(u64, AlarmKind, Option<u64>, u64)>,
    alarm_lines: Vec<u64>,
    /// Its durable alarms with evidence of a relay state, and the frames it
    /// held then (§31.7 "never lost silently", task D6).
    pub(crate) evidence: Vec<EvidenceAlarm>,
    /// A confirmed rejoin in progress (§19.7): the checkpoints written since
    /// `ConfirmRejoin` (seq → chain), and whether its base deletion was used.
    /// It explains exactly the rejoin's own `TruncateLogBelow(S + 1, Some(C_S))`
    /// and one `DeleteBases`, then ends.
    rejoin: Option<(BTreeMap<u64, Hash32>, bool)>,
    /// Calls delivered to this identity's engine (the pairing ruling counts them).
    calls: u64,
    /// Register records dropped and not yet explained: (dev, call, line, the
    /// write that drops it).
    pair_drops: Vec<(Dev, u64, u64, Option<WriteKey>)>,
    /// Calls whose effects a crash cut off before the runtime performed them
    /// all (task D5: "the same or the next call" is not asked of them).
    cut_calls: BTreeSet<u64>,
    /// The hashes of the frames this identity's `Append`s carried (any process).
    sent: BTreeSet<Hash32>,
    /// `PairFailed` notifications: (dev, call).
    pair_failed: Vec<(Dev, u64)>,
    /// Every `PairFailed`: (the device its pairing adds, trace line); and
    /// every `OpDone`: (ticket, trace line). A final refusal's report (§31.7
    /// invariant 2, task D6).
    pub(crate) pair_failed_log: Vec<(Dev, u64)>,
    pub(crate) op_done: Vec<(u64, u64)>,
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
    /// A device's `ReplaceReady` named this identity (§28.5 step 3) before
    /// its machine was re-paired (task D7).
    replace_ready: bool,
    /// The trace line at which the simulated user retired this identity,
    /// re-pairing its machine as a new device after `ReplaceReady` (§24.5);
    /// none when it was not retired, or retired otherwise (task D7).
    pub(crate) retired_at: Option<u64>,
    /// The highest trace line at which a relay state of the reference log (a
    /// relay, not a fork's copy) produced a message that then reached this
    /// identity's engine (a welcome, page or answer: any event but the
    /// runtime's own `Disconnected`); 0 for none (task D7).
    pub(crate) met_reference: u64,
    /// The highest seq its log reached: of an `AppendLog` or a checkpoint it
    /// emitted (task D7).
    pub(crate) log_reach: u64,
    /// The frames a relay made up that its engine took into its log: (seq,
    /// frame hash, trace line of the `Persist`) (task D7).
    pub(crate) took: Vec<(u64, Hash32, u64)>,
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
    /// Entry ids a relay acknowledged, as the answer reached the device:
    /// (entry id, ident, line, from the reference branch).
    pub(crate) acked: Vec<([u8; 16], usize, u64, bool)>,
    /// Every acknowledgement a relay gave, delivered or not, with the relay
    /// state, seq and frame (§31.7: where it came from, task D6).
    pub(crate) acks: Vec<AckFact>,
    /// Every `restore_tail` a device sent, with the relay's first answer or
    /// none (`relay.md` §12.9, task D6).
    pub(crate) restore_tails: Vec<RestoreTry>,
    /// The restore tail each request (ident, process, request id) is.
    restore_sent: BTreeMap<(usize, u32, u64), usize>,
    /// Every restore tail a device owed and did not send (`owe_from`,
    /// `RESTORE_DUE`): `Answer::Owed`, as it would have met the relay when it
    /// fell due (task D6).
    pub(crate) owed: Vec<RestoreTry>,
    /// The frames the relay made up (`injected_frame`), by hash, with how.
    pub(crate) made_up: BTreeMap<Hash32, MadeUp>,
    /// The operation each outbox record belongs to: entry id → (ident, ticket).
    pub(crate) entry_ops: BTreeMap<[u8; 16], (usize, u64)>,
    /// `Enqueued` with an entry id: (entry id, ident, time, line).
    pub(crate) enqueued: Vec<([u8; 16], usize, u64, u64)>,
    pub(crate) cancelled: BTreeSet<[u8; 16]>,
    /// The entry ids of pairings' `DeviceAdded` frames that a relay's
    /// `register` acknowledged with a seq (§28.4 step 3), with the device each adds.
    pub(crate) registered: BTreeMap<[u8; 16], Dev>,
    /// A test's change to the end-of-run facts of each identity, before the checks.
    #[cfg(test)]
    pub(crate) tamper_facts: Option<TamperFacts>,
    /// A test's stand-in for an engine that sends no restore tail: a
    /// `restore_tail` of the identity and first seq this names is dropped
    /// before it is recorded or reaches a relay.
    #[cfg(test)]
    pub(crate) drop_restore_tails: Option<Box<dyn Fn(usize, u64) -> bool>>,
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
    pub(crate) injected_seqs: Vec<check::Injected>,
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
    /// The trace line of the current fork's `relay_fork` fault, and the
    /// relay head when it started.
    fork_line: u64,
    fork_at: u64,
    /// The identity each completed pairing ticket started (`PairRegistered`).
    paired: BTreeMap<u64, usize>,
    /// The `Persist` whose operations are being noted (`note_persist`).
    cur_write: Option<WriteKey>,
    /// Writes emitted and not yet applied to durable storage.
    unapplied: BTreeSet<WriteKey>,
    /// What those writes tell invariant 5 once applied.
    staged: Vec<(WriteKey, Staged)>,
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
    /// An unsent `snap` record dropped (§19.4 step 5, ENG-SNAP-004): its
    /// entry id, the epoch its blob was sealed under, and the chain the
    /// device holds at `B` (which log it followed there). Explained only by
    /// an accepted rotation to a later epoch at or below `B` (task D5).
    Snap([u8; 16], u32, Option<Hash32>),
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
            acks: Vec::new(),
            restore_tails: Vec::new(),
            restore_sent: BTreeMap::new(),
            owed: Vec::new(),
            made_up: BTreeMap::new(),
            entry_ops: BTreeMap::new(),
            enqueued: Vec::new(),
            cancelled: BTreeSet::new(),
            #[cfg(test)]
            tamper_facts: None,
            #[cfg(test)]
            drop_restore_tails: None,
            registered: BTreeMap::new(),
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
            fork_at: 0,
            paired: BTreeMap::new(),
            cur_write: None,
            unapplied: BTreeSet::new(),
            staged: Vec::new(),
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
            crossings: Vec::new(),
            crash_halts: BTreeSet::new(),
            last_b: (0, [0; 32]),
            alarms: Vec::new(),
            alarm_lines: Vec::new(),
            evidence: Vec::new(),
            rejoin: None,
            calls: 0,
            pair_drops: Vec::new(),
            cut_calls: BTreeSet::new(),
            sent: BTreeSet::new(),
            pair_failed: Vec::new(),
            pair_failed_log: Vec::new(),
            op_done: Vec::new(),
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
            replace_ready: false,
            retired_at: None,
            met_reference: 0,
            log_reach: 0,
            took: Vec::new(),
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
                let (chain, frame) = (w.archive[9].chain, w.archive[9].hash);
                w.injected_seqs.push(check::Injected { seq: 10, chain, line: 5, frame, host: HOSTS[0].to_owned() });
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

    /// Quiet seed 3 run until virtual time `until` (to its end for `u64::MAX`), not finished.
    fn quiet_world(cfg: &SimConfig, until: u64) -> World<'_> {
        let mut w = World::new(3, cfg, false);
        w.setup();
        while let Some(s) = w.q.pop() {
            w.t = s.t;
            if matches!(s.ev, Ev::End) {
                break;
            }
            w.step(s.ev);
            if w.t >= until {
                break;
            }
        }
        w
    }

    /// The last accepted entry of `w`'s reference log: (entry id, seq, frame hash).
    fn last_entry(w: &World<'_>) -> ([u8; 16], u64, Hash32) {
        let roots: Vec<(u32, [u8; 32])> = w.roots.iter().copied().collect();
        let led = check::ledger(&w.archive, w.template.as_ref(), &roots);
        let l = led.iter().rev().find(|l| l.accept && l.entry_id.is_some()).expect("an entry");
        (l.entry_id.expect("an entry id"), l.seq, w.archive[l.seq as usize - 1].hash)
    }

    /// The violations of invariant 2 that name `eid`, at the end of `w`.
    fn naming(w: World<'_>, eid: &[u8; 16]) -> Vec<String> {
        let (r, _) = w.finish_run();
        r.violations.into_iter().filter(|v| v.invariant == 2 && v.detail.contains(&check::hexs(eid))).map(|v| v.detail).collect()
    }

    // §31.7 (task D6): a plain rollback is repaired, not excused. Quiet seed
    // 3 with its last accepted entry dropped by a rollback (the reference log
    // truncated below it, the relay before the rollback as the branch) and
    // acknowledged by the relay before the fault: no restore tail brought it
    // back and no other entry took its seq, so its seq is not removed and its
    // loss breaks invariant 2 although a device that held its frame raised an
    // alarm with evidence after the fault.
    #[test]
    fn an_entry_lost_to_a_plain_rollback_is_not_excused() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let mut w = quiet_world(&cfg, u64::MAX);
        let (eid, seq, frame) = last_entry(&w);
        w.branches.push(BranchLog { rows: w.archive.clone(), why: check::WHY_ROLLBACK, line: 77, removed_from: seq });
        w.archive.truncate(seq as usize - 1);
        w.acks.retain(|a| a.eid != eid);
        w.acks.push(AckFact { eid, ident: 0, sel: Sel::Main(0), seq, frame, line: 50 });
        w.acked.push((eid, 0, 51, true));
        w.idents[0].evidence.push(EvidenceAlarm { line: 80, kind: AlarmKind::RelayBehind, held: BTreeSet::from([frame]) });
        let v = naming(w, &eid);
        assert!(
            v.iter().any(|d| d.contains("lost to a rolled-back relay state that is not removed") && d.contains("line 77")),
            "{v:?}"
        );
    }

    /// Steps `w`'s events for `ms` of virtual time.
    fn run_for(w: &mut World<'_>, ms: u64) {
        let until = w.t + ms;
        while let Some(s) = w.q.pop() {
            if s.t > until {
                w.q.push(s);
                break;
            }
            w.t = s.t;
            w.step(s.ev);
        }
        w.t = until;
    }

    /// Steps `w`'s events, at most `ms` of virtual time, until `done` holds after one: whether it did.
    fn run_until(w: &mut World<'_>, ms: u64, done: impl Fn(&World<'_>) -> bool) -> bool {
        let until = w.t + ms;
        while let Some(s) = w.q.pop() {
            if s.t > until {
                w.q.push(s);
                return false;
            }
            w.t = s.t;
            w.step(s.ev);
            if done(w) {
                return true;
            }
        }
        false
    }

    /// Identity `id` enqueues `Set { key, value }` (a user's edit): its entry id.
    fn edit(w: &mut World<'_>, id: usize, key: u8, value: i64) -> [u8; 16] {
        let n = w.enqueued.len();
        let at_ms = w.wall(w.idents[id].machine);
        let ticket = (1 << 40) + n as u64;
        w.deliver(id, Event::Enqueue { ticket, command: ToyCommand::Set { key, value }, at_ms, origin_state: vec![0; 8], hold: false });
        assert!(run_until(w, 5_000, |w| w.enqueued.len() > n), "the premise: dev {id} queued the edit");
        assert_eq!(w.enqueued[n].1, id, "the premise: dev {id}'s edit");
        w.enqueued[n].0
    }

    /// Quiet seed 3 at its end, its three identities running and holding the
    /// relay's head, then a relay rollback by hand (§31.6): `down` stopped
    /// first, the relay's state marked, Desk (identity 0) appends `n` edits,
    /// the relay rolls back to the mark. Returns the world, the mark's head and
    /// the edits' entry ids and frames, which Desk holds at the seqs after it.
    fn rolled_world<'a>(cfg: &'a SimConfig, down: &[usize], n: u8) -> (World<'a>, Head, Vec<([u8; 16], Hash32)>) {
        let mut w = quiet_world(cfg, u64::MAX);
        let head = w.relays[0].head().expect("a board");
        assert_eq!(w.idents.len(), 3, "the premise: three identities");
        for (i, x) in w.idents.iter().enumerate() {
            assert!(x.proc.is_some() && x.store.head() == Some((head.seq, head.chain)), "the premise: dev {i} runs and holds the relay's head");
        }
        for id in down {
            w.stop_proc(*id);
        }
        // The quiet configuration's fault rates are all zero: only the faults
        // this test injects happen.
        w.faults_on = true;
        w.fault(FaultEv::RollbackMark(9));
        let mut edits = Vec::new();
        for k in 0..n {
            let eid = edit(&mut w, 0, k, 70 + i64::from(k));
            run_for(&mut w, 5_000);
            let q = head.seq + 1 + u64::from(k);
            let frame = w.archive.get(q as usize - 1).map(|r| r.hash).expect("the relay sequenced the edit");
            assert!(w.acks.iter().any(|a| a.eid == eid && a.seq == q && a.frame == frame), "the premise: the relay acknowledged the edit at {q}");
            assert!(w.acked.iter().any(|a| a.0 == eid && a.1 == 0), "the premise: the acknowledgement reached Desk");
            assert_eq!(w.idents[0].store.log_hash.get(&q), Some(&frame), "the premise: Desk holds {q}");
            edits.push((eid, frame));
        }
        w.fault(FaultEv::Rollback(9));
        assert_eq!(w.relays[0].head(), Some(head), "the premise: the relay rolled back to the mark");
        (w, head, edits)
    }

    // The reproduction of D6's second fix pass (§31.7: "a dropped seq ...
    // that a restore tail could have brought back, is not removed: a plain
    // rollback is repaired"; §21.3, ENG-EVID-004). A rollback drops Desk's
    // edit, which Laptop holds too; both raise `RelayBehind` and stay
    // connected while the relay holds the lost tail's prefix, and every
    // restore tail they emit is dropped before it is recorded or sent (an
    // engine that never sends one); then the phone, down through all of it,
    // starts and its own edit takes the dropped seq. That loss is no removed
    // relay state: a restore tail could have brought the seq back, and its
    // device's alarm does not excuse it.
    #[test]
    fn a_plain_rollback_no_restore_tail_was_sent_for_is_not_excused() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let (mut w, head, edits) = rolled_world(&cfg, &[2], 1);
        let (eid, frame) = edits[0];
        let q = head.seq + 1;
        assert_eq!(w.idents[1].store.log_hash.get(&q), Some(&frame), "the premise: Laptop holds {q}");
        w.drop_restore_tails = Some(Box::new(|id, _| id < 2));
        let fault = w.branches.last().expect("the rollback's branch").line;
        run_for(&mut w, 120_000);
        assert!(
            w.idents[..2].iter().any(|x| x.evidence.iter().any(|e| e.kind == AlarmKind::RelayBehind && e.line > fault && e.held.contains(&frame))),
            "the premise: a device that held {q} raised RelayBehind after the rollback"
        );
        assert_eq!(w.relays[0].head(), Some(head), "the premise: no restore tail reached the relay");
        w.start_proc(2);
        run_for(&mut w, 70_000);
        edit(&mut w, 2, 9, 9);
        run_for(&mut w, 10_000);
        assert!(w.archive.get(q as usize - 1).is_some_and(|r| r.hash != frame), "the premise: the phone's edit took {q}");
        let v = naming(w, &eid);
        let owed = format!("another entry took {q}, but a restore tail could have brought back {q} (trace line ");
        let at = format!(": its device owed one while the relay was at {} on that tail, and sent none in time)", head.seq);
        assert!(
            v.iter().any(|d| d.contains("lost to a rolled-back relay state that is not removed") && d.contains(&owed) && d.ends_with(&at)),
            "{v:?}"
        );
    }

    // The same reproduction for a restore tail refused `relay/not_next` with
    // the relay's head at its first seq (another device restored that seq
    // first): §21.3 step 3 has the device resume from the head + 1, and it
    // never does (each later restore tail of Desk's is dropped). Desk alone
    // holds both dropped edits; Laptop's restore of the first reaches the
    // relay while Desk's request is on its way. The phone's edit then takes
    // the second seq: no removed relay state either.
    #[test]
    fn a_seq_no_restore_tail_resumed_for_after_relay_not_next_is_not_excused() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let (mut w, head, edits) = rolled_world(&cfg, &[1, 2], 2);
        let q = head.seq + 1;
        w.drop_restore_tails = Some(Box::new(move |id, first| id == 0 && first > q));
        let n = w.restore_tails.len();
        assert!(run_until(&mut w, 60_000, |w| w.restore_tails.len() > n), "the premise: Desk sends a restore tail");
        assert_eq!(w.restore_tails[n].first_seq, q, "the premise: from {q}");
        let lost = w.branches.last().expect("the rollback's branch").rows[q as usize - 1].frame.clone();
        let call = Call::Admin { req: 1 << 52, op: AdminRequest::RestoreTail { first_seq: q, chain_before: head.chain, frames: vec![lost] } };
        w.at_relay(1, 0, 0, Sel::Main(0), call);
        assert_eq!(w.relays[0].head().map(|h| h.seq), Some(q), "the premise: Laptop's restore tail brought back {q}");
        run_for(&mut w, 60_000);
        assert_eq!(
            (w.restore_tails[n].answer.clone(), w.restore_tails[n].head.map(|h| h.seq)),
            (Answer::Refused("relay/not_next".to_owned(), None), Some(q)),
            "the premise: Desk's restore tail was refused relay/not_next with the relay's head at its first seq"
        );
        w.start_proc(2);
        run_for(&mut w, 70_000);
        edit(&mut w, 2, 9, 9);
        run_for(&mut w, 10_000);
        let (eid, frame) = edits[1];
        assert!(w.archive.get(q as usize).is_some_and(|r| r.hash != frame), "the premise: the phone's edit took {}", q + 1);
        let v = naming(w, &eid);
        let owed = format!("another entry took {0}, but a restore tail could have brought back {0} (trace line ", q + 1);
        let at = format!(": its device owed one while the relay was at {q} on that tail, and sent none in time)");
        assert!(
            v.iter().any(|d| d.contains("lost to a rolled-back relay state that is not removed") && d.contains(&owed) && d.ends_with(&at)),
            "{v:?}"
        );
    }

    /// Whether identity `id`'s process owes a restore tail now (`owe_from`).
    fn owes(w: &World<'_>, id: usize) -> bool {
        w.idents[id].proc.as_ref().is_some_and(|p| p.owe.is_some())
    }

    /// Identity `id`'s session ends (the `disconnection` fault's effect); it
    /// connects again with no backoff.
    fn disconnect(w: &mut World<'_>, id: usize) {
        w.close_conn(id, true);
        w.idents[id].failures = 0;
        w.deliver(id, Event::Disconnected { unreachable: true, paused_until_ms: None });
    }

    /// Whether identity `id` is in session (`Some(true)`: welcomed) or connecting (`Some(false)`).
    fn session(w: &World<'_>, id: usize) -> Option<bool> {
        w.idents[id].proc.as_ref().and_then(|p| p.conn.as_ref()).map(|c| c.welcomed_t.is_some())
    }

    // Task D6 (§21.3, ENG-EVID-004): what a restore tail owed is. After a
    // rollback below Desk's log (Desk alone runs), each restore tail Desk
    // emits dropped: a session that showed Desk the relay's head below its
    // log, on its chain, opens a debt with the relay's head and Desk's
    // durable log then; a debt whose session ends before `RESTORE_DUE` is
    // none, one that runs to it is owed (here when the dropped request's
    // timeout ends the session); none opens while Desk is halted (§16.4: a
    // halted device takes no restore tail), once the board moved, before the
    // session is welcomed, or in a session with another relay. Desk's
    // restore tail, no longer dropped, pays its debt at once and brings the
    // seq back.
    #[test]
    fn a_restore_tail_is_owed_when_none_is_sent_in_time() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let (mut w, head, edits) = rolled_world(&cfg, &[1, 2], 1);
        let q = head.seq + 1;
        let frame = edits[0].1;
        w.drop_restore_tails = Some(Box::new(|id, _| id == 0));
        assert!(run_until(&mut w, 60_000, |w| owes(w, 0)), "Desk owes a restore tail once in session");
        let (t0, x) = w.idents[0].proc.as_ref().and_then(|p| p.owe.clone()).expect("a debt");
        let holds = w.idents[0].store.run_head();
        assert_eq!(holds.map(|(lo, top)| (lo <= q, top.seq)), Some((true, q)), "the premise: Desk's log runs through {q}");
        assert_eq!((x.sel, x.first_seq, x.frames.len(), x.answer.clone(), x.head, x.holds), (Sel::Main(0), q, 0, Answer::Owed, Some(head), holds));
        assert!(x.line > w.branches.last().expect("the rollback").line);
        // The session ends before the restore tail falls due: nothing owed.
        run_for(&mut w, 1_000);
        assert!(owes(&w, 0) && w.t < t0 + RESTORE_DUE);
        disconnect(&mut w, 0);
        assert!(!owes(&w, 0) && w.owed.is_empty(), "a debt whose session ended early is none");
        // The next session runs until the dropped request's timeout ends it.
        assert!(run_until(&mut w, 120_000, |w| !w.owed.is_empty()), "a debt that ran to RESTORE_DUE is owed");
        assert_eq!((w.owed[0].first_seq, w.owed[0].answer.clone(), w.owed[0].head), (q, Answer::Owed, Some(head)));
        // Halted, Desk owes none (§16.4); unhalted, it owes again.
        let halt = crate::HaltRecord { kind: crate::HaltKind::Commitment, seq: q, author: w.idents[0].dev, since_ms: 0, code: "test".to_owned() };
        let meta = w.idents[0].store.meta.clone().expect("a meta");
        w.idents[0].store.meta = Some(crate::Meta { halt: Some(halt), ..meta.clone() });
        disconnect(&mut w, 0);
        assert!(run_until(&mut w, 60_000, |w| session(w, 0) == Some(true)), "Desk is in session again");
        run_for(&mut w, 1_000);
        assert!(!owes(&w, 0), "a halted device owes no restore tail");
        w.idents[0].store.meta = Some(meta);
        assert!(run_until(&mut w, 60_000, |w| owes(w, 0)), "unhalted, it owes one again");
        // Once the board moved, the old relay is owed nothing.
        disconnect(&mut w, 0);
        w.moved = true;
        assert!(run_until(&mut w, 60_000, |w| session(w, 0) == Some(true)), "Desk is in session again");
        run_for(&mut w, 1_000);
        assert!(!owes(&w, 0), "no restore tail is owed after a move");
        w.moved = false;
        // Not before the session is welcomed (the device has not seen the
        // relay's head), nor in a session with another relay.
        disconnect(&mut w, 0);
        assert!(run_until(&mut w, 60_000, |w| session(w, 0) == Some(false)), "Desk connects again");
        w.owe_from(0);
        assert!(!owes(&w, 0), "no debt before the session is welcomed");
        assert!(run_until(&mut w, 60_000, |w| session(w, 0) == Some(true)) && owes(&w, 0), "welcomed, Desk owes one");
        let p = w.idents[0].proc.as_mut().expect("running");
        p.owe = None;
        p.conn.as_mut().expect("a session").sel = Sel::Fork;
        w.owe_from(0);
        assert!(!owes(&w, 0), "no debt to a relay that is not the home relay");
        w.idents[0].proc.as_mut().and_then(|p| p.conn.as_mut()).expect("a session").sel = Sel::Main(0);
        w.owe_from(0);
        assert!(owes(&w, 0));
        // Sent, the restore tail pays at once and brings the seq back.
        w.drop_restore_tails = None;
        let n = w.owed.len();
        assert!(run_until(&mut w, 60_000, |w| w.relays[0].head().is_some_and(|h| h.seq == q)), "Desk restores {q}");
        assert_eq!(w.archive.get(q as usize - 1).map(|r| r.hash), Some(frame));
        assert!(!owes(&w, 0) && w.owed.len() == n, "a restore tail sent at once pays its debt");
    }

    // Task D6: a plain rollback an engine repairs owes nothing. Desk's edit
    // dropped by a rollback while Laptop and the phone are down: Desk sends
    // its restore tail at once, no restore tail is owed, the reference log
    // holds the edit again, and nothing is lost.
    #[test]
    fn a_plain_rollback_the_engine_repairs_owes_no_restore_tail() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let (mut w, head, edits) = rolled_world(&cfg, &[1, 2], 1);
        let (eid, frame) = edits[0];
        run_for(&mut w, 120_000);
        assert_eq!(w.archive.get(head.seq as usize).map(|r| r.hash), Some(frame), "Desk's restore tail brought the edit back");
        assert!(w.restore_tails.iter().any(|x| x.answer == Answer::Stored), "Desk sent one and the relay stored it");
        assert!(w.owed.is_empty(), "{:?}", w.owed);
        assert_eq!(naming(w, &eid), Vec::<String>::new());
    }

    // Task D6: a debt also ends with its process and with the run, owed when
    // it ran `RESTORE_DUE` by then. Desk's edit dropped by a rollback and
    // Desk's first debt (`carry` puts it on a process, opened `ago` ms
    // before); the phone's edit takes the seq while Desk is down; a debt
    // still open at the run's end is owed, and the loss is no removed state.
    #[test]
    fn a_debt_ends_with_its_process_and_with_the_run() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let (mut w, head, edits) = rolled_world(&cfg, &[1, 2], 1);
        let (eid, frame) = edits[0];
        let q = head.seq + 1;
        w.drop_restore_tails = Some(Box::new(|id, _| id == 0));
        assert!(run_until(&mut w, 60_000, |w| owes(w, 0)), "Desk owes a restore tail");
        let debt = w.idents[0].proc.as_ref().and_then(|p| p.owe.clone()).expect("a debt").1;
        let carry = |w: &mut World<'_>, ago: u64| {
            let t = w.t;
            w.idents[0].proc.as_mut().expect("running").owe = Some((t - ago, debt.clone()));
        };
        carry(&mut w, RESTORE_DUE - 1);
        w.stop_proc(0);
        assert!(w.owed.is_empty(), "a process that ended before its restore tail fell due owed none");
        w.start_proc(0);
        carry(&mut w, RESTORE_DUE);
        w.stop_proc(0);
        assert_eq!(w.owed.len(), 1, "a process that ended once its restore tail fell due owed one");
        w.owed.clear();
        w.start_proc(2);
        run_for(&mut w, 70_000);
        edit(&mut w, 2, 9, 9);
        run_for(&mut w, 10_000);
        assert!(w.archive.get(q as usize - 1).is_some_and(|r| r.hash != frame), "the premise: the phone's edit took {q}");
        w.start_proc(0);
        carry(&mut w, RESTORE_DUE);
        let v = naming(w, &eid);
        let at = format!(": its device owed one while the relay was at {} on that tail, and sent none in time)", head.seq);
        assert!(v.iter().any(|d| d.contains("lost to a rolled-back relay state that is not removed") && d.ends_with(&at)), "{v:?}");
    }

    // Task D6 (§21.3: the race): a relay whose head is another device's
    // entry is no prefix of the device's log, and is owed no restore tail.
    // Desk's two edits dropped by a rollback; Desk is down while the phone's
    // edit takes the first dropped seq; Desk starts again and meets the
    // relay's head below its own on another chain.
    #[test]
    fn a_relay_on_another_chain_is_owed_no_restore_tail() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let (mut w, head, edits) = rolled_world(&cfg, &[1, 2], 2);
        let q = head.seq + 1;
        w.stop_proc(0);
        w.start_proc(2);
        run_for(&mut w, 70_000);
        edit(&mut w, 2, 9, 9);
        run_for(&mut w, 10_000);
        let top = w.relays[0].head().expect("a board");
        assert!(top.seq == q && w.archive[q as usize - 1].hash != edits[0].1, "the premise: the phone's edit took {q}");
        w.idents[0].failures = 0;
        w.start_proc(0);
        assert!(run_until(&mut w, 60_000, |w| session(w, 0) == Some(true)), "the premise: Desk meets the relay");
        assert!(!run_until(&mut w, 60_000, |w| owes(w, 0)), "no debt to a relay on another chain");
        assert!(w.idents[0].store.head().is_some_and(|(l, _)| l > top.seq), "the premise: Desk's log is above the relay's head");
        assert!(w.owed.is_empty(), "{:?}", w.owed);
    }

    // §31.7 (task D6), end to end through the run's facts: quiet seed 3 with
    // its last accepted entry lost to a fork's copy (the reference log
    // truncated below it, the copy as the branch, the acknowledgement the
    // copy's) breaks invariant 2, and is excused once a device that held its
    // frame raised an alarm with evidence after the fork began.
    #[test]
    fn a_loss_to_a_forks_copy_is_excused_by_an_alarm_of_a_device_that_held_it() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let lost = |alarm: bool| {
            let mut w = quiet_world(&cfg, u64::MAX);
            let (eid, seq, frame) = last_entry(&w);
            w.branches.push(BranchLog { rows: w.archive.clone(), why: check::WHY_FORK, line: 40, removed_from: seq });
            w.archive.truncate(seq as usize - 1);
            w.acks.retain(|a| a.eid != eid);
            w.acks.push(AckFact { eid, ident: 1, sel: Sel::Fork, seq, frame, line: 50 });
            if alarm {
                w.idents[2].evidence.push(EvidenceAlarm { line: 80, kind: AlarmKind::Fork, held: BTreeSet::from([frame]) });
            }
            naming(w, &eid)
        };
        let v = lost(false);
        assert!(v.iter().any(|d| d.contains("lost to a removed relay state (the forked copy of the relay, fault at trace line 40)")), "{v:?}");
        assert_eq!(lost(true), Vec::<String>::new());
    }

    // Task D6: every acknowledgement that reached a device is one a relay
    // gave (`World::acks`), before it was delivered, with the relay state
    // that gave it and a seq where the reference log holds its frame (a quiet
    // run has no rollback, fork or move).
    #[test]
    fn a_relay_acknowledgement_records_its_relay_state_seq_and_frame() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let w = quiet_world(&cfg, u64::MAX);
        assert!(!w.acked.is_empty(), "the quiet run acknowledges entries");
        for (eid, id, line, _) in &w.acked {
            let a = w.acks.iter().find(|a| a.eid == *eid && a.ident == *id).unwrap_or_else(|| panic!("no relay acknowledgement of {}", check::hexs(eid)));
            assert_eq!(a.sel, Sel::Main(0));
            assert!(a.line < *line, "given at {} before delivered at {line}", a.line);
            assert_eq!(w.archive[a.seq as usize - 1].hash, a.frame, "seq {}", a.seq);
        }
    }

    // Task D6: an alarm with evidence of a relay state counts once its
    // `WriteMeta` is durable, with the frames its device held then (its
    // log's and its live outbox's, here with a record its write puts there);
    // an alarm of another kind counts as none.
    #[test]
    fn an_evidence_alarm_counts_once_durable_with_the_frames_held() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let mut w = quiet_world(&cfg, u64::MAX);
        let mut m = w.idents[1].store.meta.clone().expect("a meta");
        let id = w.idents[1].max_alarm;
        let record = |id: u64, kind: AlarmKind, evidence: crate::Evidence| crate::AlarmRecord {
            id,
            kind,
            raised_ms: 0,
            seq: None,
            author: None,
            code: None,
            evidence,
            resolved: false,
        };
        let behind = crate::Evidence::RelayBehind { relay_head: Head::default(), relay_last_ctr: None, local_head: Head::default(), ack: None };
        m.alarms.push(record(id + 1, AlarmKind::RelayBehind, behind));
        m.alarms.push(record(id + 2, AlarmKind::SkippedFrame, crate::Evidence::Frame { frame_hash: None }));
        let before = w.idents[1].evidence.len();
        let mut framed = unsent(&w, 0x92);
        framed.frame = Some(vec![0x92; 700]);
        let ops = [PersistOp::Outbox(vec![OutboxOp::Put(framed)]), PersistOp::WriteMeta(m)];
        let k = unapplied_write(&mut w, &ops);
        assert_eq!(w.idents[1].evidence.len(), before, "not durable yet");
        w.idents[1].store.apply(&ops, None);
        w.wrote(k);
        let st = &w.idents[1].store;
        let held: BTreeSet<Hash32> =
            st.log.values().map(|r| h(&r.frame)).chain(st.live_outbox().iter().filter_map(|r| r.frame.as_ref().map(|f| h(f)))).collect();
        assert!(st.log.len() > 1 && st.live_outbox().iter().any(|r| r.frame.is_some()), "the device holds frames in its log and its outbox");
        let new: Vec<(AlarmKind, &BTreeSet<Hash32>)> = w.idents[1].evidence[before..].iter().map(|e| (e.kind, &e.held)).collect();
        assert_eq!(new, vec![(AlarmKind::RelayBehind, &held)]);
    }

    // Task D6 (§31.7 invariant 4): each move of a device's board within a
    // process is recorded with its class head then, the durable log head,
    // which is never below the board (the board applies only what was
    // classified and persisted, §12.3 step 9, §12.4), but at the creator's
    // first move: its board is at the genesis entry from the bootstrap
    // (§19.8) before the log write is durable.
    #[test]
    fn a_crossing_records_each_move_of_the_board_with_its_class_head() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let w = quiet_world(&cfg, u64::MAX);
        for (i, x) in w.idents.iter().enumerate() {
            assert!(!x.crossings.is_empty(), "dev {i}'s board moved");
            for c in &x.crossings {
                assert!(c.from != c.to && (c.k >= c.to || c.from == 0 && c.to == 1), "dev {i}: {c:?}");
            }
            let (b, ch, _, _) = x.samples.last().expect("a sample");
            assert!(x.crossings.iter().any(|c| c.to == *b && c.to_chain == *ch), "dev {i}: its last sample at {b} ends a crossing");
        }
    }

    // Task D6: a restore tail a relay answered is recorded with what it
    // carried, how the relay answered (`relay.md` §12.9): refused before its
    // frames (`relay/not_next`: no index), or at a frame (another board's
    // frame: `relay/wrong_board` at its index), and the relay's head then.
    #[test]
    fn a_restore_tail_records_how_the_relay_answered() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let mut w = quiet_world(&cfg, u64::MAX);
        let head = w.relays[0].head().expect("a board");
        let p = w.idents[0].proc.as_ref().expect("running");
        let (proc, conn) = (p.no, p.conn_no);
        let mine = w.archive[1].frame.clone();
        let other = FakeSealer::create_at(77, "Other", WALL0).1;
        let tail = |first_seq: u64, frames: Vec<Vec<u8>>| Call::Admin {
            req: 1 << 50,
            op: AdminRequest::RestoreTail { first_seq, chain_before: head.chain, frames },
        };
        let n = w.restore_tails.len();
        w.at_relay(0, proc, conn, Sel::Main(0), tail(head.seq, vec![mine.clone()]));
        w.at_relay(0, proc, conn, Sel::Main(0), tail(head.seq + 1, vec![other.clone()]));
        type Answered = (Sel, u64, Vec<Hash32>, Answer, Option<Head>);
        let got: Vec<Answered> = w.restore_tails[n..].iter().map(|x| (x.sel, x.first_seq, x.frames.clone(), x.answer.clone(), x.head)).collect();
        assert_eq!(
            got,
            vec![
                (Sel::Main(0), head.seq, vec![h(&mine)], Answer::Refused("relay/not_next".to_owned(), None), Some(head)),
                (Sel::Main(0), head.seq + 1, vec![h(&other)], Answer::Refused("relay/wrong_board".to_owned(), Some(0)), Some(head)),
            ]
        );
    }

    // Task D6 (§31.7: "a restore tail could have brought back"): a restore
    // tail is recorded when it is sent, with the relay's head and the
    // sender's durable log then (`Store::run_head`), unanswered (a request
    // lost on the way stays so); the relay's first answer, with its head
    // then, completes it, and a second copy of the request (`duplication`)
    // is no other restore tail.
    #[test]
    fn a_restore_tail_is_recorded_when_sent_and_completed_by_its_first_answer() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let mut w = quiet_world(&cfg, u64::MAX);
        let head = w.relays[0].head().expect("a board");
        let p = w.idents[0].proc.as_ref().expect("running");
        let (proc, conn) = (p.no, p.conn_no);
        let holds = w.idents[0].store.run_head();
        let (lo, top) = holds.expect("a durable log");
        assert!(lo < top.seq && w.idents[0].store.chain_at(top.seq) == Some(top.chain), "a run of rows {lo}..={}", top.seq);
        let req = 1 << 51;
        let call = Call::Admin { req, op: AdminRequest::RestoreTail { first_seq: head.seq + 1, chain_before: head.chain, frames: vec![w.archive[1].frame.clone()] } };
        let n = w.restore_tails.len();
        let sent = w.trace.lines();
        w.send_up(0, call.clone(), req, 0);
        let got = |w: &World<'_>| w.restore_tails[n..].iter().map(|x| (x.line, x.answer.clone(), x.head, x.holds)).collect::<Vec<_>>();
        assert_eq!(got(&w), vec![(sent, Answer::Unanswered, Some(head), holds)]);
        w.at_relay(0, proc, conn, Sel::Main(0), call.clone());
        let answered = w.trace.lines();
        let first = w.restore_tails[n].answer.clone();
        assert!(matches!(&first, Answer::Refused(_, Some(0))), "refused at its frame (an old counter): {first:?}");
        assert_eq!(got(&w), vec![(answered, first.clone(), Some(head), holds)]);
        // The relay's head moves on; a second copy of the request meets it there.
        let now = w.relay_now(0);
        w.relays[0].inject_frame(now, FakeSealer::create_at(78, "Other", WALL0).1).expect("a board");
        assert_ne!(w.relays[0].head(), Some(head));
        w.at_relay(0, proc, conn, Sel::Main(0), call);
        assert_eq!(got(&w), vec![(answered, first, Some(head), holds)], "a second copy");
    }

    // Task D6: a frame the relay makes up (`injected_frame`, §31.6) is
    // recorded as made up: its hash is the frame the reference log holds at
    // the seq the fault names.
    #[test]
    fn an_injected_frame_is_recorded_as_made_up() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let mut w = quiet_world(&cfg, 3_600_000);
        assert!(w.made_up.is_empty());
        w.fault(FaultEv::Inject(1));
        let x = w.injected_seqs.last().expect("an injected frame").clone();
        assert_eq!(w.made_up, BTreeMap::from([(w.archive[x.seq as usize - 1].hash, MadeUp::OtherBoard)]));
        // Task D7: with its hash, its chain there and the host of the relay that showed it.
        assert_eq!((x.frame, x.chain, x.host.as_str()), (w.archive[x.seq as usize - 1].hash, w.archive[x.seq as usize - 1].chain, HOSTS[0]));
        w.fault(FaultEv::Inject(0));
        let seq = w.injected_seqs.last().expect("an injected frame").seq;
        assert_eq!(w.made_up.get(&w.archive[seq as usize - 1].hash), Some(&MadeUp::BadSignature));
        // Task D7: an `AppendLog` that carries a made-up frame is recorded as
        // taken into that identity's log, with its trace line; one that
        // carries a device's frame is not.
        let row = |q: u64| {
            let r = &w.archive[q as usize - 1];
            crate::LogRecord { seq: r.seq, chain: r.chain, tau: 0, frame: r.frame.clone() }
        };
        let (made, honest) = (row(x.seq), row(x.seq - 1));
        let before = w.idents[1].took.clone();
        w.note_persist(1, &[PersistOp::AppendLog(vec![honest, made])], 97);
        let mut want = before;
        want.push((x.seq, x.frame, 97));
        assert_eq!(w.idents[1].took, want);
    }

    // Task D6: what reports a final refusal, as a run records it: every
    // `PairFailed` with the device its pairing adds, every `OpDone` with its
    // ticket, at their trace lines; and the operation an outbox record
    // belongs to (`OutboxRecord.op`).
    #[test]
    fn reports_and_operations_are_recorded() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let mut w = quiet_world(&cfg, u64::MAX);
        w.pair_devs.insert(1 << 40, [0x44; 16]);
        w.on_notify(1, Notification::PairFailed { ticket: 1 << 40, code: "engine/pair_interrupted" }, 91);
        w.on_notify(1, Notification::OpDone { ticket: 33, result: Ok(()) }, 92);
        assert_eq!(w.idents[1].pair_failed_log.last(), Some(&([0x44; 16], 91)));
        assert_eq!(w.idents[1].op_done.last(), Some(&(33, 92)));
        let mut r = unsent(&w, 0x91);
        r.op = Some(33);
        w.note_persist(1, &[PersistOp::Outbox(vec![OutboxOp::Put(r)])], 9);
        assert_eq!(w.entry_ops.get(&[0x91; 16]), Some(&(1, 33)));
    }

    // Task D7: what invariant 2's last outcome ("its device was retired while
    // cut off") reads, as a run records it. An event a relay produced reaches
    // an engine: its identity met a relay state of the reference log at the
    // trace line the relay produced it (`met_reference`); not for a fork's
    // copy, nor for the runtime's own `Disconnected`. How far its log reached
    // (`log_reach`): the highest seq of an `AppendLog` or checkpoint it
    // emitted. A retirement after a `ReplaceReady` that named the identity
    // (`retired_at`, §24.5, §28.5 step 3), and none for a re-pair otherwise.
    #[test]
    fn what_a_retirement_while_cut_off_reads_is_recorded() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let mut w = quiet_world(&cfg, u64::MAX);
        for (i, x) in w.idents.iter().enumerate() {
            assert!(x.met_reference > 0 && x.met_reference <= w.trace.lines(), "dev {i} met relay 0 ({})", x.met_reference);
            assert!(x.log_reach > 0 && x.log_reach >= x.store.head().map_or(0, |h| h.0), "dev {i}'s log reached its head ({})", x.log_reach);
            assert_eq!(x.retired_at, None, "dev {i}");
        }
        let p = w.idents[1].proc.as_ref().expect("running");
        let (proc, conn) = (p.no, p.conn_no);
        let (seq, chain) = w.idents[1].store.head().expect("a log");
        let page = Page { head: Head { seq, chain }, relay_now: 0, floor: 0, purge_floor: 0, more: false, truncated: false, fenced: false, entries: vec![] };
        let before = w.idents[1].met_reference;
        let late = w.trace.lines() + 1_000;
        w.down(1, proc, conn, Down::Sock(SockMsg::Push(page.clone())), (Sel::Fork, late));
        assert_eq!(w.idents[1].met_reference, before, "a fork's copy is no relay state of the reference log");
        w.down(1, proc, conn, Down::Sock(SockMsg::Push(page)), (Sel::Main(0), late));
        assert_eq!(w.idents[1].met_reference, late, "relay 0's push");
        let p = w.idents[1].proc.as_ref().expect("running");
        let (proc, conn) = (p.no, p.conn_no);
        w.down(1, proc, conn, Down::Answer(1 << 50, Event::Disconnected { unreachable: true, paused_until_ms: None }), (Sel::Main(0), late + 1));
        assert_eq!(w.idents[1].met_reference, late, "the runtime's Disconnected");
        let mut r = w.idents[1].store.log.values().next_back().cloned().expect("a row");
        r.seq = 1 << 30;
        w.note_persist(1, &[PersistOp::AppendLog(vec![r])], 9);
        assert_eq!(w.idents[1].log_reach, 1 << 30, "an AppendLog");
        let mut c = w.idents[1].store.checkpoint.clone().expect("a checkpoint");
        c.board_seq = 1 << 31;
        w.note_persist(1, &[PersistOp::WriteCheckpoint(c)], 9);
        assert_eq!(w.idents[1].log_reach, 1 << 31, "a checkpoint");
        w.workload_on = true;
        let replaced = w.idents[2].dev;
        w.on_notify(0, Notification::ReplaceReady { dev: replaced }, 93);
        w.begin_repair(2);
        assert!(w.idents[2].retired);
        assert_eq!(w.idents[2].retired_at, Some(w.trace.lines()), "retired after ReplaceReady");
        w.begin_repair(1);
        assert!(w.idents[1].retired);
        assert_eq!(w.idents[1].retired_at, None, "re-paired with no ReplaceReady");
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
        w.down(0, proc, conn, Down::Sock(SockMsg::Push(page.clone())), (Sel::Main(0), w.trace.lines()));
        assert_eq!(pages(&w), before + 1, "a push on a session whose login was answered");
        if let Some(c) = w.idents[0].proc.as_mut().and_then(|p| p.conn.as_mut()) {
            c.welcomed_t = None;
        }
        w.down(0, proc, conn, Down::Sock(SockMsg::Push(page)), (Sel::Main(0), w.trace.lines()));
        assert_eq!(pages(&w), before + 1, "a push before the login's answer");
    }

    /// Notes `ops` as identity 1's write `k`, not yet applied (task D5).
    fn unapplied_write(w: &mut World<'_>, ops: &[PersistOp]) -> WriteKey {
        let k = (1, w.proc_no(1).unwrap_or(0), 1 << 40);
        w.unapplied.insert(k);
        w.cur_write = Some(k);
        w.note_persist(1, ops, 9);
        w.cur_write = None;
        k
    }

    // Invariant 5, task D5: a deletion counts only when its write was
    // applied. The same unexplained truncation, unsent drop and pairing's
    // drop break invariant 5 once their write is applied, and nothing when a
    // crash cut the write off before it was (the process ended with it in
    // the queue) or when the run ends with it still there.
    #[test]
    fn a_deletion_counts_only_when_its_write_is_applied() {
        let truncate = |w: &mut World<'_>| unapplied_write(w, &[PersistOp::TruncateLogBelow(40, None)]);
        let unsent_drop = |w: &mut World<'_>| {
            let r = unsent(w, 0x71);
            w.idents[1].live.insert(r.entry_id, r);
            unapplied_write(w, &[PersistOp::Outbox(vec![OutboxOp::Drop([0x71; 16])])])
        };
        let pair_drop = |w: &mut World<'_>| {
            let mut r = unsent(w, 0x72);
            r.via = crate::Via::Register { dev: [0x45; 16], sign_pk: [0; 32] };
            w.idents[1].live.insert(r.entry_id, r);
            unapplied_write(w, &[PersistOp::Outbox(vec![OutboxOp::Drop([0x72; 16])])])
        };
        type Make<'f> = &'f dyn Fn(&mut World<'_>) -> WriteKey;
        let cases: [(&str, Make<'_>); 3] = [("truncate", &truncate), ("unsent drop", &unsent_drop), ("pairing's drop", &pair_drop)];
        for (name, make) in cases {
            assert_eq!(
                tampered(|w| {
                    let k = make(w);
                    w.wrote(k);
                }),
                vec![5],
                "{name}, applied"
            );
            assert_eq!(
                tampered(|w| {
                    let k = make(w);
                    w.discard_unapplied(k.0, k.1);
                }),
                Vec::<u8>::new(),
                "{name}, cut off by a crash"
            );
            assert_eq!(
                tampered(|w| {
                    make(w);
                }),
                Vec::<u8>::new(),
                "{name}, still queued at the end"
            );
        }
    }

    // Invariant 5, task D5: "in the same or the next call" is not asked of
    // a call whose effects a crash cut off before the runtime performed
    // them (its `PairFailed` may be among them), nor of one whose next call
    // was cut so (task D6); a drop with neither call cut is still held to it.
    #[test]
    fn a_pairing_drop_in_a_call_a_crash_cut_is_not_judged() {
        let drop_reg = |w: &mut World<'_>| {
            let mut r = unsent(w, 0x73);
            r.via = crate::Via::Register { dev: [0x46; 16], sign_pk: [0; 32] };
            drop_record(w, r);
        };
        assert_eq!(tampered(drop_reg), vec![5]);
        assert_eq!(
            tampered(|w| {
                drop_reg(w);
                let call = w.idents[1].calls;
                w.idents[1].cut_calls.insert(call);
            }),
            Vec::<u8>::new()
        );
        // Task D6 (D5's low finding): nor of a drop whose next call a crash
        // cut, where its `PairFailed` may have been due.
        assert_eq!(
            tampered(|w| {
                drop_reg(w);
                let call = w.idents[1].calls;
                w.idents[1].cut_calls.insert(call + 1);
            }),
            Vec::<u8>::new()
        );
        assert_eq!(
            tampered(|w| {
                drop_reg(w);
                let call = w.idents[1].calls;
                w.idents[1].cut_calls.insert(call + 2);
            }),
            vec![5]
        );
    }

    // Invariant 4, task D5 (ENG-EFFECT-002): a page that answers a read the
    // engine already received an answer to (a duplicated answer delivered
    // after the first copy), or one it never issued, is no relay head the
    // device saw: it owes no alarm of §21.1. A push and the first answer to
    // a read still count.
    #[test]
    fn a_duplicated_answer_to_a_read_is_no_relay_head() {
        let cfg = SimConfig::named("quiet").expect("quiet");
        let mut w = World::new(3, &cfg, false);
        w.setup();
        let ready = |w: &World<'_>| {
            w.idents[0].proc.as_ref().and_then(|p| p.conn.as_ref()).is_some_and(|c| c.welcomed_t.is_some())
                && w.idents[0].store.head().is_some_and(|(l, _)| l >= 2)
        };
        while let Some(s) = w.q.pop() {
            w.t = s.t;
            if matches!(s.ev, Ev::End) {
                break;
            }
            w.step(s.ev);
            if w.t > 600_000 && ready(&w) {
                break;
            }
        }
        assert!(ready(&w), "device 0 is welcomed with a log of two entries");
        let (l, chain) = w.idents[0].store.head().expect("a log");
        let below = w.idents[0].store.chain_at(l - 1).expect("the chain below the head");
        let page = |seq: u64, chain: Hash32| Page { head: Head { seq, chain }, relay_now: 0, floor: 0, purge_floor: 0, more: false, truncated: false, fenced: false, entries: vec![] };
        let stale = |req: Option<u64>| Event::Page { req, page: page(l - 1, below) };
        // An id far above any the engine issued in this run.
        let r = 1u64 << 50;
        assert!(w.contradiction(0, &stale(None)).is_some(), "a push below the log head");
        assert!(w.contradiction(0, &stale(Some(r))).is_none(), "an answer to a read never issued");
        w.idents[0].proc.as_mut().expect("running").reads_open.insert(r);
        assert!(w.contradiction(0, &stale(Some(r))).is_some(), "the first answer to a read");
        // The first copy (at the log head) reaches the engine; the second, below it, answers nothing.
        w.deliver(0, Event::Page { req: Some(r), page: page(l, chain) });
        assert!(w.contradiction(0, &stale(Some(r))).is_none(), "a second answer to the same read");
        assert!(w.contradiction(0, &stale(None)).is_some(), "a push below the log head, still");
    }

    // Invariant 5, task D5 (§19.4 step 5): an outbox drop of a `snap` record
    // no `Append` carried is handed to the check as `Deletion::Snap` (with
    // its blob's epoch), judged against the log's rotations at the end
    // (`script.rs`); the drop of one an `Append` carried stays a violation.
    #[test]
    fn an_unsent_snap_drop_is_judged_by_the_rotations() {
        let snap = |w: &World<'_>, eid: u8| {
            let mut r = unsent(w, eid);
            r.kind = Kind::Snap;
            r.frame = Some(vec![eid; 40]);
            r.body = Body::Snap(crate::SnapInfo {
                seq: 1,
                hash: [0; 32],
                hash_version: 1,
                blob: [eid; 16],
                sha256: [0; 32],
                size: 1,
                parts: 1,
                inflated: 1,
                epoch: 7,
            });
            r
        };
        assert_eq!(
            tampered(|w| {
                let r = snap(w, 0x81);
                let before = w.violations.len();
                drop_record(w, r);
                assert_eq!(w.violations.len(), before, "no violation at the drop");
                assert!(matches!(w.deletions.last(), Some((1, Deletion::Snap(e, 7, _), 0, 9)) if *e == [0x81; 16]), "{:?}", w.deletions.last());
            }),
            vec![5],
            "no rotation in a quiet run explains it"
        );
        let sent = |w: &mut World<'_>| {
            let r = snap(w, 0x82);
            w.idents[1].sent.insert(h(r.frame.as_ref().expect("a frame")));
            let before = (w.violations.len(), w.deletions.len());
            drop_record(w, r);
            assert_eq!((w.violations.len(), w.deletions.len()), (before.0 + 1, before.1), "a sent snap's drop is a violation");
        };
        assert_eq!(tampered(sent), vec![5]);
    }
}
