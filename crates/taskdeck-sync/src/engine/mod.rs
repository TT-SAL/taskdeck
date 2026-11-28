//! The replica engine (engine.md §4, §7 to §30): one deterministic state
//! machine, `handle(input) -> effects`, generic over the board's state machine
//! and the sealer. This file holds the struct, the dispatch of events, the
//! nine steps of §4.5 and the small services every part uses (ids, the persist
//! queue, `Meta` writes, alarms, stops, halts, notifications).
//!
//! Rules that hold between the parts:
//! - Every `Persist` goes through `persist()`. `durable` is the id of the last
//!   `Persisted`; ids come from one counter, and the runtime confirms writes in
//!   order (ENG-PERSIST-001), so a write with id `p` is durable exactly when
//!   `p <= durable`. Whatever must wait for a write (§7.12) either remembers
//!   that write's id and is looked at again on every call, or is an `After`
//!   attached to it.
//! - `Meta` changes set `meta_dirty` and are written once at the end of the
//!   call (`flush_meta`), or earlier where an order matters.
//! - Notifications that announce something persisted are queued with
//!   `notify_durable` and leave only after the write that holds it.

use std::collections::{BTreeMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::{
    AdminOk, AlarmKind, AlarmRecord, BlobOk, Dev, Effect, Event, Evidence, HaltKind, HaltRecord, Hash32, HashRecord,
    Input, LocalReadResult, Marker, MembershipView, Meta, Notification, PersistOp, Registry, RelayRefusal, Sealer,
    StateMachine, StopKind, StopRecord,
};

mod board;
mod boot;
mod duty;
mod epoch;
mod evid;
mod movement;
mod ops;
mod outbox;
mod recv;
mod repair;
mod replay;
mod send;
mod sess;
mod start;
mod status;

#[cfg(test)]
mod core_tests;
#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod kit_tests;
#[cfg(test)]
mod poison_tests;
#[cfg(test)]
mod reconcile_tests;
#[cfg(test)]
mod relay_tests;
#[cfg(test)]
mod rkit_tests;
#[cfg(test)]
mod rules_tests;
#[cfg(test)]
mod script_tests;
#[cfg(test)]
mod sess_tests;
#[cfg(test)]
mod vectors_tests;

/// The limits of engine.md §34 that more than one file uses.
pub(crate) mod lim {
    pub const BATCH_FRAMES: usize = 16;
    pub const BATCH_BYTES: usize = 4_194_373;
    pub const PAGE_ENTRIES: usize = 256;
    pub const PAGE_BYTES: usize = 1_048_576;
    pub const FRAME_MIN: usize = 670;
    pub const FRAME_MAX: usize = 262_144;
    pub const STRIDE: u64 = 64;
    pub const SKIP_WINDOW: u64 = 16_384;
    pub const HOUR: i64 = 3_600_000;
    pub const DAY: i64 = 86_400_000;
    pub const WEEK: i64 = 604_800_000;
    pub const THIRTY_DAYS: i64 = 2_592_000_000;
    pub const HOLD_MS: u64 = 30_000;
    pub const POLL_MS: u64 = 30_000;
    pub const STOP_HELLO_MS: u64 = 900_000;
    pub const SLOW_CONNECT_MS: u64 = 3_600_000;
    pub const CAUGHT_UP_MS: u64 = 600_000;
    /// On a session with push: a `Hello` when the last one is this old.
    pub const PUSH_HELLO_MS: u64 = 600_000;
    pub const CKPT_ENTRIES: u32 = 200;
    pub const CKPT_MS: u64 = 600_000;
    pub const ALARMS: usize = 256;
    pub const BANNERS: usize = 256;
    pub const STOPS: usize = 16;
    pub const OPS: usize = 16;
}

/// Reserved (engine.md §37): the relay target comes only from `Loaded`.
#[derive(Clone, Debug, Default)]
pub struct Config {}

/// What runs when a `Persist` becomes durable (§7.12).
pub(crate) enum After {
    /// A notification that announces something that write holds (D1, ENG-REF-002, ENG-EVID-002).
    Notify(Notification),
    /// The page in flight is durable with its marker (D3): commit it.
    PageDurable,
    /// The checkpoint this truncation waits for is durable (D8, ENG-BOOT-006).
    Truncate { below: u64, floor_chain: Option<Hash32> },
    /// The final checkpoint, outbox and meta are durable (ENG-CKPT-002).
    ReadyToExit,
    /// The base of a purge is durable (D7): the older bases go.
    DeleteBases(Vec<u64>),
}

/// Why a `ReadLog`, `ReadBase`, `Admin` or `Blob` effect was emitted: its answer goes there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Want {
    /// `ReadBase` at the snapshot duty's seq (§19.4 step 1).
    SnapBase(u64),
    /// The snapshot duty's blob upload (§19.4 step 3).
    SnapUpload,
    /// The daily `Compact` (§20.2).
    Compact,
    /// A replay job's base, a chunk of its log, a frame a Skip in it needs (replay.rs).
    JobBase,
    JobLog,
    JobFrame(u64),
    /// Operations and pairing (ops.rs, movement.rs): the ticket names the operation.
    OpFrame(u64),
    OpAdmin(u64),
    OpBlob(u64),
    ShredBase(u64),
    MoveLog(u64),
    FenceFrame(u64),
    PurgeDuty,
    PairUpload,
    /// Restore tail (repair.rs): the frames from the log, the request, a repair call.
    RestoreChunk,
    RestoreCall,
    Repair,
    /// Rejoin and rebootstrap (boot.rs).
    RejoinList,
    RejoinFetch,
    RebootFetch,
}

impl Want {
    /// Is the answer a relay's (lost with the session), not the local store's?
    fn relay(&self) -> bool {
        !matches!(
            self,
            Want::SnapBase(_)
                | Want::JobBase
                | Want::JobLog
                | Want::JobFrame(_)
                | Want::OpFrame(_)
                | Want::ShredBase(_)
                | Want::MoveLog(_)
                | Want::FenceFrame(_)
                | Want::RestoreChunk
        )
    }
}

pub(crate) struct Pending {
    id: u64,
    /// The marker this write leaves behind, when it sets or clears one.
    marker: Option<Option<Marker>>,
    after: Vec<After>,
}

/// One notice or refusal record as held in memory (§7.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NoticeRow {
    pub kind: crate::NoticeKind,
    pub seq: u64,
    pub record: Vec<u8>,
    pub dismissed: bool,
}

/// The replica engine. Inert until `Start`.
pub struct Engine<SM: StateMachine, S: Sealer> {
    pub(crate) sm: SM,
    pub(crate) sealer: S,
    pub(crate) own: Dev,
    /// A `Start` succeeded: the fields below hold a replica.
    pub(crate) on: bool,
    /// A `PersistFailed` arrived: no `Persist`, `Append`, `Admin` or `Blob` until the next `Start`.
    pub(crate) dead: bool,
    pub(crate) wall: i64,
    pub(crate) mono: u64,
    pub(crate) next_id: u64,
    pub(crate) out: Vec<Effect>,
    pub(crate) notes: Vec<Notification>,
    pub(crate) durable_notes: Vec<Notification>,
    pub(crate) pq: VecDeque<Pending>,
    pub(crate) durable: u64,
    pub(crate) last_pid: u64,
    /// The latest write of a base or of log records: a local read is emitted only once it is
    /// durable, because the runtime's reads do not wait for its writes (ENG-EFFECT-001).
    pub(crate) read_gate: u64,
    /// Local reads waiting for `read_gate`.
    pub(crate) read_wait: Vec<Effect>,

    // ---- records (§7)
    pub(crate) meta: Meta,
    pub(crate) meta_dirty: bool,
    pub(crate) meta_pid: u64,
    pub(crate) registry: Registry,
    pub(crate) outbox: Vec<outbox::Rec<SM::Command>>,
    pub(crate) outbox_ops: usize,
    pub(crate) notices: Vec<NoticeRow>,
    pub(crate) notice_ops: usize,
    pub(crate) notices_through: u64,
    pub(crate) hashes: BTreeMap<u64, HashRecord>,
    pub(crate) bases: Vec<(u64, Hash32)>,

    // ---- the log (§7.2): chain and log clock of every seq from `chain_first` to `l`
    pub(crate) floor: u64,
    pub(crate) retained: u64,
    pub(crate) l: u64,
    pub(crate) chain_first: u64,
    pub(crate) chains: VecDeque<(Hash32, i64)>,

    // ---- the class head and the board head (§7.1)
    pub(crate) k: u64,
    pub(crate) mk: Option<S::Membership>,
    pub(crate) vk: MembershipView,
    pub(crate) b: u64,
    pub(crate) cb: Hash32,
    pub(crate) state: Option<SM::State>,
    pub(crate) mb: Option<S::Membership>,
    /// This device's `device_no` in `M_B`.
    pub(crate) no_b: Option<u16>,

    // ---- the pending view (§8)
    pub(crate) view: Option<SM::State>,
    pub(crate) view_version: u64,
    pub(crate) rebase_due: bool,

    // ---- the parts
    pub(crate) sess: sess::Sess,
    pub(crate) pipe: board::Pipe<SM, S>,
    pub(crate) jobs: replay::Jobs<SM, S>,
    pub(crate) duty: duty::Duty,
    pub(crate) evid: evid::Evid,
    pub(crate) boot: boot::Boot,
    pub(crate) opx: ops::Ops,
    pub(crate) send: send::Send,
    pub(crate) epochs: epoch::Epochs,
    /// The outstanding `ReadLog`, `ReadBase`, `Admin` and `Blob` effects, by id.
    pub(crate) wants: Vec<(u64, Want)>,

    /// The previous process's session id (§11.6), until one `Second` used it.
    pub(crate) prev_session: Option<[u8; 16]>,
    pub(crate) session_id: [u8; 16],
    pub(crate) protocol_hits: Vec<(&'static str, u8)>,
    pub(crate) tick_at: Option<u64>,
    pub(crate) status_dirty: bool,
    pub(crate) banners_dirty: bool,
    pub(crate) view_notified: u64,
    pub(crate) exiting: bool,
}

/// Runs a trait call that may execute code derived from replicated data
/// (ENG-GEN-012). `None` is a caught panic.
pub(crate) fn guard<T>(f: impl FnOnce() -> T) -> Option<T> {
    catch_unwind(AssertUnwindSafe(f)).ok()
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    /// An engine that does nothing until `Start` (engine.md §37).
    pub fn new(sm: SM, sealer: S, _config: Config) -> Self {
        let own = sealer.own_dev();
        Engine {
            sm,
            sealer,
            own,
            on: false,
            dead: false,
            wall: 0,
            mono: 0,
            next_id: 1,
            out: Vec::new(),
            notes: Vec::new(),
            durable_notes: Vec::new(),
            pq: VecDeque::new(),
            durable: 0,
            last_pid: 0,
            read_gate: 0,
            read_wait: Vec::new(),
            meta: Meta::default(),
            meta_dirty: false,
            meta_pid: 0,
            registry: Registry::default(),
            outbox: Vec::new(),
            outbox_ops: 0,
            notices: Vec::new(),
            notice_ops: 0,
            notices_through: 0,
            hashes: BTreeMap::new(),
            bases: Vec::new(),
            floor: 1,
            retained: 1,
            l: 0,
            chain_first: 1,
            chains: VecDeque::new(),
            k: 0,
            mk: None,
            vk: MembershipView::default(),
            b: 0,
            cb: [0; 32],
            state: None,
            mb: None,
            no_b: None,
            view: None,
            view_version: 1,
            rebase_due: false,
            sess: sess::Sess::default(),
            pipe: board::Pipe::default(),
            jobs: replay::Jobs::default(),
            duty: duty::Duty::default(),
            evid: evid::Evid::default(),
            boot: boot::Boot::default(),
            opx: ops::Ops::default(),
            send: send::Send::default(),
            epochs: epoch::Epochs::default(),
            wants: Vec::new(),
            prev_session: None,
            session_id: [0; 16],
            protocol_hits: Vec::new(),
            tick_at: None,
            status_dirty: false,
            banners_dirty: false,
            view_notified: 0,
            exiting: false,
        }
    }

    /// One input in, the effects out (engine.md §4.1, ENG-MODEL-001).
    pub fn handle(&mut self, input: Input<SM::Command>) -> Vec<Effect> {
        self.out = Vec::new();
        let Input { wall_ms, mono_ms, event } = input;
        if let Event::Start(loaded) = event {
            self.wall = wall_ms;
            self.mono = mono_ms;
            self.start(loaded);
            if self.on {
                self.steps();
            } else {
                self.finish_notes();
            }
            return std::mem::take(&mut self.out);
        }
        if !self.on {
            // Inert before a Start, and after one that could not load its records (§21.4).
            if matches!(event, Event::Shutdown) {
                self.out.push(Effect::Notify(Notification::ReadyToExit));
            }
            return std::mem::take(&mut self.out);
        }
        if mono_ms < self.mono {
            // ENG-MODEL-004.
            self.protocol("mono");
            self.finish_notes();
            return std::mem::take(&mut self.out);
        }
        self.wall = wall_ms;
        self.mono = mono_ms;
        match event {
            Event::Start(_) => {}
            Event::Shutdown => self.on_shutdown(),
            Event::Tick => {
                // A tick only makes the steps run; the deadline it answered is spent.
                self.tick_at = None;
            }
            Event::Persisted { id } => self.on_persisted(id),
            Event::PersistFailed { id } => self.on_persist_failed(id),
            Event::LocalRead { req, result } => self.on_local_read(req, result),
            Event::Enqueue { ticket, command, at_ms, origin_state, hold } => {
                self.on_enqueue(ticket, command, at_ms, origin_state, hold)
            }
            Event::ReleaseHold => self.end_hold(),
            Event::Op { ticket, op } => self.on_op(ticket, op),
            Event::PairRegister { ticket, device } => self.on_pair_register(ticket, device),
            Event::Resubmit { ticket, commands, from_device_no, origin_state } => {
                self.on_resubmit(ticket, commands, from_device_no, origin_state)
            }
            Event::User(action) => self.on_user(action),
            Event::Connected { push } => self.on_connected(push),
            Event::MissedRoots { first_epoch, wraps } => self.on_missed_roots(first_epoch, wraps),
            Event::Welcome(w) => self.on_welcome(w),
            Event::Page { req, page } => self.on_page(req, page),
            Event::AppendResults { req, results } => self.on_append_results(req, results),
            Event::AdminResult { req, result } => self.on_admin_result(req, result),
            Event::BlobResult { req, result } => self.on_blob_result(req, result),
            Event::RelayRefused { req, refusal } => self.on_relay_refused(req, refusal),
            Event::Second { session } => self.on_second(session),
            Event::Displaced { session } => self.on_displaced(session),
            Event::Disconnected { unreachable, paused_until_ms } => self.on_disconnected(unreachable, paused_until_ms),
        }
        self.steps();
        std::mem::take(&mut self.out)
    }

    /// The nine steps of §4.5, in order (ENG-MODEL-005).
    fn steps(&mut self) {
        self.step_session(); // 1
        self.step_receive(); // 2
        self.step_rebase(); // 3
        self.step_ops(); // 4
        self.step_duties(); // 5
        self.step_send(); // 6
        self.step_checkpoint(); // 7
        self.flush_meta();
        self.step_tick(); // 8
        self.finish_notes(); // 9
    }

    // ------------------------------------------------------------------ ids and effects

    /// The next id of the one counter (ENG-EFFECT-002).
    pub(crate) fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub(crate) fn fx(&mut self, e: Effect) {
        if matches!(e, Effect::ReadBase { .. } | Effect::ReadLog { .. }) && !self.is_durable(self.read_gate) {
            self.read_wait.push(e);
            return;
        }
        self.out.push(e);
    }

    /// Emits one `Persist` and returns its id; 0 when nothing was emitted
    /// (ENG-PERSIST-003: none after a failed write).
    pub(crate) fn persist(&mut self, ops: Vec<PersistOp>) -> u64 {
        if self.dead || ops.is_empty() {
            return 0;
        }
        let id = self.id();
        let mut marker = None;
        for op in &ops {
            match op {
                PersistOp::SetMarker(m) => marker = Some(Some(*m)),
                PersistOp::ClearMarker => marker = Some(None),
                PersistOp::Outbox(v) => self.outbox_ops += v.len(),
                PersistOp::RewriteOutbox(v) => self.outbox_ops = v.len(),
                PersistOp::Notices(v) => self.notice_ops += v.len(),
                PersistOp::RewriteNotices(v) => self.notice_ops = v.len(),
                PersistOp::WriteBase(_) | PersistOp::AppendLog(_) => self.read_gate = id,
                _ => {}
            }
        }
        self.pq.push_back(Pending { id, marker, after: Vec::new() });
        self.last_pid = id;
        self.out.push(Effect::Persist { id, ops });
        id
    }

    /// Is the write `pid` durable? (0 names no write.)
    pub(crate) fn is_durable(&self, pid: u64) -> bool {
        pid <= self.durable
    }

    /// Runs `a` when the write `pid` is durable: now, if it already is.
    pub(crate) fn after(&mut self, pid: u64, a: After) {
        if self.dead {
            return;
        }
        match self.pq.iter_mut().find(|p| p.id == pid) {
            Some(p) => p.after.push(a),
            None => self.run_after(a),
        }
    }

    fn run_after(&mut self, a: After) {
        match a {
            After::Notify(n) => self.notes.push(n),
            After::PageDurable => self.page_durable(),
            After::Truncate { below, floor_chain } => {
                self.persist(vec![PersistOp::TruncateLogBelow(below, floor_chain)]);
            }
            After::ReadyToExit => self.notes.push(Notification::ReadyToExit),
            After::DeleteBases(seqs) => {
                self.persist(vec![PersistOp::DeleteBases(seqs)]);
            }
        }
    }

    fn on_persisted(&mut self, id: u64) {
        match self.pq.front() {
            Some(p) if p.id == id => {}
            _ => {
                // An id never issued, already confirmed, or out of order (ENG-EFFECT-002).
                self.protocol("persisted");
                return;
            }
        }
        if let Some(p) = self.pq.pop_front() {
            self.durable = id;
            if !self.read_wait.is_empty() && self.is_durable(self.read_gate) {
                let reads = std::mem::take(&mut self.read_wait);
                self.out.extend(reads);
            }
            if let Some(m) = p.marker {
                self.pipe.marker = m;
            }
            for a in p.after {
                self.run_after(a);
            }
        }
    }

    /// ENG-PERSIST-003: stop with `engine/persist_failed` until the next `Start`.
    fn on_persist_failed(&mut self, id: u64) {
        if !self.pq.iter().any(|p| p.id == id) {
            self.protocol("persist_failed");
            return;
        }
        self.pq.clear();
        self.add_stop(StopKind::LocalFault { code: "engine/persist_failed".into() }, None);
        self.dead = true;
        self.meta_dirty = false;
        self.durable_notes.clear();
        self.drop_requests();
        self.status_dirty = true;
    }

    // ------------------------------------------------------------------ notifications

    /// A notification that changes nothing durable and waits for nothing.
    pub(crate) fn notify(&mut self, n: Notification) {
        self.notes.push(n);
    }

    /// A notification that leaves only after the next `Meta` write (alarms, banners, operations).
    pub(crate) fn notify_durable(&mut self, n: Notification) {
        self.durable_notes.push(n);
    }

    /// Writes `Meta` when it changed and returns the id of its latest write.
    pub(crate) fn flush_meta(&mut self) -> u64 {
        if self.meta_dirty && !self.dead {
            self.meta_dirty = false;
            let m = self.meta.clone();
            self.meta_pid = self.persist(vec![PersistOp::WriteMeta(m)]);
        }
        let pid = self.meta_pid;
        for n in std::mem::take(&mut self.durable_notes) {
            self.after(pid, After::Notify(n));
        }
        pid
    }

    pub(crate) fn touch_meta(&mut self) {
        self.meta_dirty = true;
        self.status_dirty = true;
    }

    /// Step 9 of §4.5: the `Notify` effects, after everything else.
    fn finish_notes(&mut self) {
        if self.view_notified != self.view_version && self.on {
            self.view_notified = self.view_version;
            self.notes.push(Notification::ViewChanged { version: self.view_version });
        }
        if self.banners_dirty {
            self.banners_dirty = false;
            self.notes.push(Notification::BannersChanged);
        }
        if self.status_dirty {
            self.status_dirty = false;
            self.notes.push(Notification::StatusChanged);
        }
        for n in std::mem::take(&mut self.notes) {
            self.out.push(Effect::Notify(n));
        }
    }

    // ------------------------------------------------------------------ alarms, stops, halts, faults

    /// Does an alarm of this kind stop the engine while unresolved (§21.1)?
    pub(crate) fn kind_stops(kind: AlarmKind) -> bool {
        matches!(
            kind,
            AlarmKind::RelayBehind
                | AlarmKind::Fork
                | AlarmKind::Counter
                | AlarmKind::Unechoed
                | AlarmKind::Withheld
                | AlarmKind::ForeignSigner
                | AlarmKind::IdentityClone
                | AlarmKind::StateMismatch
                | AlarmKind::SnapshotMismatch
                | AlarmKind::ResetMismatch
        )
    }

    /// Records an alarm with its evidence (§21.1, ENG-EVID-001, ENG-EVID-002) and
    /// returns its id. The same alarm is not recorded twice: an unresolved one of
    /// a stopping kind, or any one of an informational kind (a `Start` classifies
    /// its tail again).
    pub(crate) fn alarm(
        &mut self,
        kind: AlarmKind,
        seq: Option<u64>,
        author: Option<Dev>,
        code: Option<&str>,
        evidence: Evidence,
    ) -> u64 {
        let stopping = Self::kind_stops(kind);
        if let Some(a) = self.meta.alarms.iter().find(|a| {
            a.kind == kind
                && a.seq == seq
                && a.code.as_deref() == code
                && a.evidence == evidence
                && (!a.resolved || !stopping)
        }) {
            return a.id;
        }
        let id = self.meta.alarms.iter().map(|a| a.id).max().unwrap_or(0) + 1;
        if self.meta.alarms.len() >= lim::ALARMS {
            let at = self.meta.alarms.iter().position(|a| a.resolved).unwrap_or(0);
            self.meta.alarms.remove(at);
        }
        self.meta.alarms.push(AlarmRecord {
            id,
            kind,
            raised_ms: self.wall,
            seq,
            author,
            code: code.map(str::to_owned),
            evidence,
            resolved: false,
        });
        self.touch_meta();
        self.notify_durable(Notification::AlarmRaised { id });
        id
    }

    pub(crate) fn has_unresolved(&self, kind: AlarmKind) -> bool {
        self.meta.alarms.iter().any(|a| a.kind == kind && !a.resolved)
    }

    pub(crate) fn resolve_alarms(&mut self, f: impl Fn(&AlarmRecord) -> bool) {
        let mut changed = false;
        for a in self.meta.alarms.iter_mut().filter(|a| !a.resolved) {
            if f(a) {
                a.resolved = true;
                changed = true;
            }
        }
        if changed {
            self.touch_meta();
        }
    }

    pub(crate) fn has_stop(&self, f: impl Fn(&StopKind) -> bool) -> bool {
        self.meta.stops.iter().any(|s| f(&s.kind))
    }

    /// Adds a stop unless one of the same kind is in force (§21, §22, §24.4).
    pub(crate) fn add_stop(&mut self, kind: StopKind, alarm: Option<u64>) {
        let same = |a: &StopKind, b: &StopKind| match (a, b) {
            (StopKind::LocalFault { code: x }, StopKind::LocalFault { code: y }) => x == y,
            (StopKind::Evidence, StopKind::Evidence) => false,
            _ => std::mem::discriminant(a) == std::mem::discriminant(b),
        };
        if self.meta.stops.iter().any(|s| same(&s.kind, &kind) || (s.kind == kind && s.alarm == alarm)) {
            return;
        }
        if self.meta.stops.len() >= lim::STOPS {
            self.meta.stops.remove(0);
        }
        self.meta.stops.push(StopRecord { kind, since_ms: self.wall, alarm });
        self.touch_meta();
    }

    pub(crate) fn end_stops(&mut self, f: impl Fn(&StopRecord) -> bool) {
        let before = self.meta.stops.len();
        self.meta.stops.retain(|s| !f(s));
        if self.meta.stops.len() != before {
            self.touch_meta();
        }
    }

    /// A local fault (§21.4): stops sending and applying, shows its code, ends at the next `Start`.
    pub(crate) fn fault(&mut self, code: &'static str) {
        self.add_stop(StopKind::LocalFault { code: code.into() }, None);
    }

    /// `engine/runtime_protocol`: the event is ignored by the caller; the third
    /// violation of one kind in a `Start` is a persistent local fault (§21.4).
    pub(crate) fn protocol(&mut self, what: &'static str) {
        let n = match self.protocol_hits.iter_mut().find(|h| h.0 == what) {
            Some(h) => {
                h.1 = h.1.saturating_add(1);
                h.1
            }
            None => {
                self.protocol_hits.push((what, 1));
                1
            }
        };
        if n >= 3 {
            self.fault("engine/runtime_protocol");
        }
    }

    /// Is any stop in force that holds back sending (§10.1)? Every stop does.
    pub(crate) fn stopped(&self) -> bool {
        !self.meta.stops.is_empty() || self.dead
    }

    /// Every stop in force is a divergence stop: the beat still goes (§22.2).
    pub(crate) fn divergence_only(&self) -> bool {
        !self.dead && !self.meta.stops.is_empty() && self.meta.stops.iter().all(|s| s.kind == StopKind::Divergence)
    }

    /// Is a stop in force under which the engine reads and persists nothing from
    /// the relay (§21.2 item 2, §21.4)?
    pub(crate) fn no_apply(&self) -> bool {
        self.dead
            || self.has_stop(|k| {
                matches!(k, StopKind::Evidence | StopKind::LocalFault { .. } | StopKind::Stranded | StopKind::Closed)
            })
    }

    /// Raises a halt (§16.1) and writes it at once (ENG-HALT-002, D6).
    pub(crate) fn halt(&mut self, kind: HaltKind, seq: u64, author: Dev, code: &str) {
        if self.meta.halt.is_some() {
            return;
        }
        self.meta.halt = Some(HaltRecord { kind, seq, author, since_ms: self.wall, code: code.into() });
        self.touch_meta();
        self.flush_meta();
    }

    pub(crate) fn halted(&self) -> bool {
        self.meta.halt.is_some()
    }

    pub(crate) fn crash_halt_seq(&self) -> Option<u64> {
        match &self.meta.halt {
            Some(HaltRecord { kind: HaltKind::Crash { .. }, seq, .. }) => Some(*seq),
            _ => None,
        }
    }

    // ------------------------------------------------------------------ requests and their answers

    /// Registers why an effect is emitted and returns its id (ENG-EFFECT-002).
    pub(crate) fn want(&mut self, w: Want) -> u64 {
        let id = self.id();
        self.wants.push((id, w));
        id
    }

    pub(crate) fn take_want(&mut self, req: u64) -> Option<Want> {
        let i = self.wants.iter().position(|w| w.0 == req)?;
        Some(self.wants.remove(i).1)
    }

    /// The session ended: every relay request is unanswered (ENG-SEND-007).
    pub(crate) fn lose_wants(&mut self) {
        let lost: Vec<Want> = {
            let (relay, local): (Vec<_>, Vec<_>) = std::mem::take(&mut self.wants).into_iter().partition(|w| w.1.relay());
            self.wants = local;
            relay.into_iter().map(|w| w.1).collect()
        };
        for w in lost {
            self.want_lost(w);
        }
    }

    /// A relay request that will get no answer, or was refused as a whole: its
    /// owner may send it again. Returns the request's name for an alarm (§21.1).
    pub(crate) fn want_lost(&mut self, w: Want) -> &'static str {
        match w {
            Want::SnapUpload => {
                self.snapshot_uploaded(false);
                "blob"
            }
            Want::Compact => {
                self.compact_lost();
                "admin/compact"
            }
            Want::RestoreCall | Want::Repair => {
                self.repair_lost();
                "admin/restore_tail"
            }
            Want::RejoinList | Want::RejoinFetch => {
                self.rejoin_lost();
                "blob"
            }
            Want::RebootFetch => {
                self.reboot_lost();
                "blob"
            }
            other => self.op_want_lost(&other),
        }
    }

    fn on_local_read(&mut self, req: u64, result: LocalReadResult) {
        if let Some((r, t, false)) = self.pipe.need_req
            && r == req {
                self.skip_local_read(t, result);
                return;
            }
        let Some(w) = self.take_want(req) else { return };
        match w {
            Want::SnapBase(s) => {
                let base = match result {
                    LocalReadResult::Base(c) => Some(c),
                    _ => None,
                };
                self.snapshot_base(s, base);
            }
            Want::JobBase => {
                let base = match result {
                    LocalReadResult::Base(c) => Some(c),
                    _ => None,
                };
                self.job_base(base);
            }
            Want::JobLog => self.job_log(result),
            Want::JobFrame(t) => self.job_frame(t, result),
            Want::FenceFrame(seq) => {
                self.opx.mv.fence_req = false;
                self.fence_frame(seq, result);
            }
            Want::RestoreChunk => self.restore_chunk(result),
            other => self.op_want_local(other, result),
        }
    }

    fn on_admin_result(&mut self, req: u64, result: Result<AdminOk, RelayRefusal>) {
        if self.carried_result(req, &result) {
            return;
        }
        let Some(w) = self.take_want(req) else { return };
        match w {
            Want::Compact => {
                let refusal = result.as_ref().err().map(|r| (r.status, r.code.as_str()));
                self.compact_result(refusal);
            }
            Want::RestoreCall => self.restore_result(result),
            Want::Repair => self.repair_result(result),
            other => self.op_want_admin(other, result),
        }
    }

    fn on_blob_result(&mut self, req: u64, result: Result<BlobOk, RelayRefusal>) {
        let Some(w) = self.take_want(req) else { return };
        match w {
            Want::SnapUpload => self.snapshot_uploaded(matches!(result, Ok(BlobOk::Uploaded))),
            Want::RejoinList | Want::RejoinFetch => self.rejoin_blob(w, result),
            Want::RebootFetch => self.reboot_blob(result),
            other => self.op_want_blob(other, result),
        }
    }

    /// Step 2 of §4.5: the board, then the reads.
    fn step_receive(&mut self) {
        self.run_board();
        self.step_reads();
        if self.epochs.recheck && self.sess.relay_head.is_some_and(|h| h.seq <= self.l) && self.sess.read.is_none() {
            self.epochs.recheck = false;
            self.revoked_recheck();
        }
    }

    /// Step 8 of §4.5: the earliest armed deadline, as at most one `ScheduleTick`.
    fn step_tick(&mut self) {
        let mut at: Option<u64> = None;
        let mut arm = |t: Option<u64>| {
            if let Some(t) = t {
                at = Some(at.map_or(t, |a: u64| a.min(t)));
            }
        };
        let mono = self.mono;
        let wall = self.wall;
        // A wall-clock time that is still ahead, as a monotonic time.
        let ahead = |t: i64| -> Option<u64> { (t > wall).then(|| mono.saturating_add((t - wall) as u64)) };
        arm(self.hold_deadline());
        arm(self.checkpoint_deadline());
        match self.sess.state {
            crate::SessionState::Offline => {
                if !self.meta.relay.host.is_empty() && !self.exiting {
                    arm(self.sess.connect_mono.map(|t| t + lim::SLOW_CONNECT_MS).filter(|t| *t > mono));
                    arm(self.sess.seeding_until.filter(|t| *t > mono));
                    arm(self.sess.pause_all_until.and_then(ahead));
                }
            }
            crate::SessionState::Connecting => {
                if self.evidence_stop() {
                    arm(self.sess.hello_mono.map(|t| t + lim::STOP_HELLO_MS));
                }
            }
            crate::SessionState::Reconciling | crate::SessionState::Live => {
                if self.evidence_stop() {
                    arm(self.sess.hello_mono.map(|t| t + lim::STOP_HELLO_MS));
                } else if !self.sess.push || self.sess.wait_welcome {
                    arm(self.sess.hello_mono.map(|t| t + lim::POLL_MS));
                } else if self.sess.state == crate::SessionState::Live {
                    arm(self.sess.hello_mono.map(|t| t + lim::PUSH_HELLO_MS));
                }
                arm(self.sess.pause_append_until.and_then(ahead));
                arm(self.sess.pause_writes_until.and_then(ahead));
                if self.current_member() && !self.halted() && !self.stopped() {
                    // Duties: the next beat, the snapshot's rank wait, the upgrade's.
                    if !self.outbox.iter().any(|r| r.r.kind == crate::Kind::Beat) {
                        let step = if self.registry.pending_verdicts.is_empty() { lim::DAY } else { lim::HOUR };
                        arm(ahead(self.meta.last_beat_ms.saturating_add(step)));
                    }
                    if let Some((_, tau, _)) = self.meta.duties.snapshot {
                        let rank = self.duty.snap_rank.unwrap_or(0) as i64;
                        arm(ahead(tau.saturating_add(rank * lim::HOUR)));
                    }
                    if let Some((_, t)) = self.meta.duties.upgrade {
                        let rank = duty::rank_of(&self.vk, &self.own).unwrap_or(0) as i64;
                        arm(ahead(t.saturating_add(rank * lim::HOUR)));
                    }
                    if self.meta.duties.purge.is_some() {
                        arm(ahead(sess::next_utc_midnight(wall)));
                    }
                    if self.duty.fresh_hello.is_some_and(|t| mono.saturating_sub(t) < lim::POLL_MS) && !self.caught_up() {
                        arm(self.duty.fresh_hello.map(|t| t + lim::POLL_MS));
                    }
                }
            }
            crate::SessionState::Rejoining => {}
        }
        if let Some(t) = at {
            let t = t.max(mono + 1);
            if self.tick_at != Some(t) {
                self.tick_at = Some(t);
                self.out.push(Effect::ScheduleTick { at_mono_ms: t });
            }
        }
    }

    /// The notifications that wait for the next `Meta` write, taken to hang on another write.
    pub(crate) fn take_durable_notes(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.durable_notes)
    }

    // ------------------------------------------------------------------ the chain index (§7.2)

    /// `C_s` when the device holds it (ENG-SEALER-006): from `retained` to `L`.
    pub(crate) fn chain_at(&self, s: u64) -> Option<Hash32> {
        if s < self.retained || s < self.chain_first {
            return None;
        }
        self.chains.get((s - self.chain_first) as usize).map(|c| c.0)
    }

    /// `τ_s` for a seq of the log.
    pub(crate) fn tau_at(&self, s: u64) -> Option<i64> {
        if s < self.floor || s < self.chain_first {
            return None;
        }
        self.chains.get((s - self.chain_first) as usize).map(|c| c.1)
    }

    pub(crate) fn push_chain(&mut self, seq: u64, chain: Hash32, tau: i64) {
        if self.chains.is_empty() {
            self.chain_first = seq;
        }
        debug_assert_eq!(self.chain_first + self.chains.len() as u64, seq);
        self.chains.push_back((chain, tau));
    }

    /// Forgets the chain index below `seq`, keeping `C_{seq-1}` when it is held.
    pub(crate) fn trim_chains(&mut self, seq: u64) {
        while self.chain_first + 1 < seq && self.chains.len() > 1 {
            self.chains.pop_front();
            self.chain_first += 1;
        }
    }

    /// `(L, C_L)`.
    pub(crate) fn log_head(&self) -> crate::Head {
        crate::Head { seq: self.l, chain: self.chain_at(self.l).unwrap_or(self.cb) }
    }

    /// This device's record in `M_K`.
    pub(crate) fn me(&self) -> Option<&crate::DeviceView> {
        self.vk.devices.iter().find(|d| d.dev == self.own)
    }

    /// Refreshes the cached view of `M_K`.
    pub(crate) fn refresh_vk(&mut self) {
        if let Some(m) = &self.mk {
            self.vk = self.sealer.view(m);
        }
        self.status_dirty = true;
    }
}
