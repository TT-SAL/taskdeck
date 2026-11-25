//! The oracle and the six invariants (engine.md §31.7, ENG-SIM-002).
//!
//! The **reference log** is the board's log as its home relay stored it (the
//! simulator keeps every row ever stored there, so compaction and purge at the
//! relay do not hide early seqs; after a move, the new relay's rows from the
//! seed on). The **oracle** is a fresh `Engine<ToyMachine, FakeSealer>` with
//! the genesis anchor that replays the reference log from seq 1 with no
//! faults: its sealer is the creator's anchor with every epoch root any device
//! held (`FakeSealer::observer`), its runtime confirms every write at once and
//! serves the log one entry per push, so that its confirmed state is seen at
//! every seq it passes. The **ledger** is the reference log classified once by
//! the same kind of sealer: class, entry id, purge rotations, and what the
//! board apply of each entry needs.
//!
//! Task D6 (§31.7 as the owner amended it on 2026-11-25): an entry lost with a
//! **removed relay state** (a fork's copy; a rolled-back seq that the
//! reference log holds otherwise after the settle, or not at all because every
//! restore tail that would have brought it back was refused for a frame the
//! relay made up; not one a restore tail could have brought back, sent, or
//! owed by a device that held it and never sent) is an outcome when every
//! acknowledgement of it came from such a state and a device that held its
//! frame raised an alarm with evidence (`never lost silently`); a final
//! refusal its device reported is an outcome of invariant 2; and between a
//! Skip's target `t` and the Skip, a device that never applied `t` is
//! compared with the log replayed without `t` (`replay_without`: the toy on
//! the ledger, no engine).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use sha2::Digest;

use super::relay::LogRow;
use super::store::Store;
use super::world::Sel;
use crate::fake::{FakeMembership, FakeSealer};
use crate::toy::{ToyCommand, ToyMachine, ToyState};
use crate::{
    AdminOk, AlarmKind, Bootstrap, Class, ClassifyInput, Config, Ctx, CtlItem, Dev, DeviceStatus, Effect, Engine, Event, Hash32, Head, Input,
    Kind, LocalReadResult, MembershipView, Notification, Page, PageEntry, RelayRefusal, RelayTarget, Sealer, StateMachine, UserAction,
    Welcome,
};

/// `BranchLog::why` of a rollback's lost tail and of a fork's copy.
pub(crate) const WHY_ROLLBACK: &str = "the relay before a rollback";
pub(crate) const WHY_FORK: &str = "the forked copy of the relay";

/// Another branch of the log a device may have followed (a rollback's lost
/// tail, a fork's copy, the old relay after a move).
#[derive(Clone, Debug)]
pub(crate) struct BranchLog {
    pub rows: Vec<LogRow>,
    pub why: &'static str,
    /// The trace line of the fault (or move) that made it a branch.
    pub line: u64,
    /// The first seq the fault took from the reference log: a rollback's
    /// first dropped seq (the rows from here on are the ones it dropped), the
    /// first seq after a fork's point.
    pub removed_from: u64,
}

/// One classified entry of a log.
#[derive(Clone, Debug)]
pub(crate) struct Ledger {
    pub seq: u64,
    pub accept: bool,
    pub class: &'static str,
    pub entry_id: Option<[u8; 16]>,
    /// A `Cmd` entry: the one kind the engine hands to `apply`.
    pub cmd: bool,
    pub purge_before: Option<u64>,
    pub new_epoch: Option<u32>,
    pub skip_targets: Vec<u64>,
    /// The devices a `DeviceAdded` item of the entry adds (accepted entries
    /// only: a refused one carries no items).
    pub added: Vec<Dev>,
    pub tau: i64,
    /// What the board apply of an accepted `cmd` entry needs (§13.1): its
    /// payload, and `Ctx { seq, d_author, at_ms, based_on }` with `d_author`
    /// the author's `device_no` in `M_{seq−1}`.
    pub payload: Vec<u8>,
    pub d_author: u16,
    pub at_ms: i64,
    pub based_on: u64,
    /// The active rules version in `M_seq` (one that a `RulesUpgrade`
    /// activates is `SM::upgrade`d before the next entry, §14.1).
    pub rules: u32,
    /// The target seq of a `ResetTo` item (§28.1).
    pub reset_to: Option<u64>,
    /// The host a `Fence` item of the entry names (§28.3).
    pub fence_host: Option<String>,
}

/// A frame a relay made up and showed (the `injected frame` fault of §31.6).
#[derive(Clone, Debug)]
pub(crate) struct Injected {
    pub seq: u64,
    /// The relay's chain at `seq` with it.
    pub chain: Hash32,
    /// The trace line of the fault.
    pub line: u64,
    /// Its hash.
    pub frame: Hash32,
    /// The host of the relay that showed it.
    pub host: String,
}

fn class_name(c: &Class) -> &'static str {
    match c {
        Class::Skip => "skip",
        Class::Tamper => "tamper",
        Class::Refuse => "refuse",
        Class::Accept => "accept",
        Class::NeedFrames(_) => "need_frames",
    }
}

/// Classifies `rows` (seqs 1..) with an observer of `template` holding the
/// roots `pick_roots` chooses for them among `roots`.
pub(crate) fn ledger(rows: &[LogRow], template: Option<&FakeSealer>, roots: &[(u32, [u8; 32])]) -> Vec<Ledger> {
    ledger_with(rows, template, &pick_roots(rows, template, roots))
}

/// One root per epoch for the log `rows`, out of every `(epoch, root)` any
/// device held. Two branches of one board can each rotate to the same epoch
/// with a different root (a fork's copy and the reference log, a rollback's
/// lost tail), and an observer holds one root per epoch: for each epoch with
/// several, the choice under which the classification of `rows` reaches
/// furthest before a frame it cannot open (`seal/missing_root`); the first
/// such choice in `(epoch, root)` order on a tie.
pub(crate) fn pick_roots(rows: &[LogRow], template: Option<&FakeSealer>, roots: &[(u32, [u8; 32])]) -> Vec<(u32, [u8; 32])> {
    let mut by_epoch: BTreeMap<u32, Vec<[u8; 32]>> = BTreeMap::new();
    for (e, r) in roots {
        let v = by_epoch.entry(*e).or_default();
        if !v.contains(r) {
            v.push(*r);
        }
    }
    let combos = by_epoch.values().map(Vec::len).product::<usize>();
    let choice = |k: usize| -> Vec<(u32, [u8; 32])> {
        let mut k = k;
        by_epoch
            .iter()
            .map(|(e, rs)| {
                let r = rs[k % rs.len()];
                k /= rs.len();
                (*e, r)
            })
            .collect()
    };
    if combos <= 1 {
        return choice(0);
    }
    let reach = |c: &[(u32, [u8; 32])]| ledger_with(rows, template, c).iter().position(|l| l.class == "error").unwrap_or(rows.len());
    let mut best = (0usize, choice(0));
    for k in 0..combos.min(64) {
        let c = choice(k);
        let n = reach(&c);
        if k == 0 || n > best.0 {
            best = (n, c);
        }
        if n == rows.len() {
            break;
        }
    }
    best.1
}

/// Classifies `rows` (seqs 1..) with an observer of `template` holding `roots`.
fn ledger_with(rows: &[LogRow], template: Option<&FakeSealer>, roots: &[(u32, [u8; 32])]) -> Vec<Ledger> {
    let mut out = Vec::new();
    let Some(mut s) = template.and_then(|t| t.observer(0x0b5e_77e7, roots)) else { return out };
    let mut m: FakeMembership = s.membership_zero();
    // The membership after the last accepted entry: device numbers and the
    // rules version change only with accepted entries.
    let mut view: MembershipView = s.view(&m);
    let mut prev = s.chain_zero();
    let chains: BTreeMap<u64, Hash32> = rows.iter().map(|r| (r.seq, r.chain)).collect();
    let frames: BTreeMap<u64, &[u8]> = rows.iter().map(|r| (r.seq, r.frame.as_slice())).collect();
    let mut tau = 0i64;
    for r in rows {
        let chain_at = |q: u64| chains.get(&q).copied();
        let own = |_: u64| None;
        let skip = |q: u64| frames.get(&q).copied();
        let input = ClassifyInput {
            seq: r.seq,
            frame: &r.frame,
            prev_chain: prev,
            membership: &m,
            chain_at: &chain_at,
            retained_from: 0,
            own_sealed: &own,
            skip_targets: &skip,
        };
        let mut l = Ledger {
            seq: r.seq,
            accept: false,
            class: "error",
            entry_id: None,
            cmd: false,
            purge_before: None,
            new_epoch: None,
            skip_targets: vec![],
            added: vec![],
            tau,
            payload: Vec::new(),
            d_author: 0,
            at_ms: 0,
            based_on: 0,
            rules: view.rules,
            reset_to: None,
            fence_host: None,
        };
        let Ok(rc) = s.classify(&input) else {
            out.push(l);
            prev = r.chain;
            continue;
        };
        l.accept = rc.class == Class::Accept;
        l.class = class_name(&rc.class);
        if let Some(o) = &rc.opened {
            l.entry_id = Some(o.entry_id);
            l.cmd = o.kind == Kind::Cmd;
            l.at_ms = o.at_ms;
            l.based_on = o.based_on.seq;
            if l.accept {
                tau = tau.max(o.at_ms);
                if l.cmd {
                    l.payload = o.payload.clone();
                    l.d_author = view.devices.iter().find(|d| d.dev == rc.author).map_or(0, |d| d.device_no);
                }
            }
            for it in &o.items {
                match it {
                    CtlItem::EpochRotated { epoch, purge_before: p } => {
                        l.new_epoch = Some(*epoch);
                        l.purge_before = *p;
                    }
                    CtlItem::Skip { seq, .. } => l.skip_targets.push(*seq),
                    CtlItem::DeviceAdded { dev, .. } => l.added.push(*dev),
                    CtlItem::ResetTo { seq, .. } => l.reset_to = Some(*seq),
                    CtlItem::Fence { host, .. } => l.fence_host = Some(host.clone()),
                    _ => {}
                }
            }
        }
        l.tau = tau;
        m = rc.membership;
        if l.accept {
            view = s.view(&m);
            l.rules = view.rules;
        }
        out.push(l);
        prev = r.chain;
    }
    out
}

/// The ledger entry at `q`.
fn ledger_at(led: &[Ledger], q: u64) -> Option<&Ledger> {
    let i = usize::try_from(q).ok()?.checked_sub(1)?;
    led.get(i).filter(|l| l.seq == q).or_else(|| led.iter().find(|l| l.seq == q))
}

/// The oracle's confirmed state at each `B` it passed: `B → (C_B, board hash, state hash)`.
pub(crate) type OracleStates = BTreeMap<u64, (Hash32, Hash32, Hash32)>;

/// The rules the oracle's sealer supports: the template's (`World::setup`).
pub(crate) const ORACLE_RULES: [u32; 2] = [1, 2];

/// The oracle's run on one log (§31.7).
pub(crate) struct OracleRun {
    /// Its first confirmed state at each `B`.
    pub states: OracleStates,
    /// The devices its final membership lists as revoked.
    pub revoked: BTreeSet<Dev>,
    /// Its first toy state at each `B`: where invariant 4's replays without a
    /// Skip's target start (`replay_without`).
    pub boards: BTreeMap<u64, ToyState>,
    /// The seqs of the crash halts it had (an entry whose `apply` panics, §15, §16.2).
    pub halts: BTreeSet<u64>,
    /// The seq of each accepted `Fence` whose move code its user typed, in
    /// order, with the result the engine gave (`MoveCodeResult`, ENG-MOVE-008).
    pub moves: Vec<(u64, Option<Result<(), &'static str>>)>,
}

/// Keeps the first value seen at `b` (§31.7: the oracle's state at each `B` is
/// the first one it had there; a later visit to `B`, as a Skip replay that
/// showed its intermediate states would make, does not overwrite it).
pub(crate) fn keep_first<V>(map: &mut BTreeMap<u64, V>, b: u64, v: impl FnOnce() -> V) {
    map.entry(b).or_insert_with(v);
}

/// The oracle on `rows` (§31.7): a fresh engine replaying them from seq 1
/// with no faults, under the roots `pick_roots` chooses among `roots`. Its
/// state at each `B` is the first one it had there (`keep_first`).
#[cfg(test)]
pub(crate) fn oracle(
    rows: &[LogRow],
    template: Option<&FakeSealer>,
    roots: &[(u32, [u8; 32])],
    board: &[u8],
    relay: &RelayTarget,
) -> (OracleStates, BTreeSet<Dev>) {
    let r = oracle_full(rows, template, roots, board, relay);
    (r.states, r.revoked)
}

/// `oracle`, with the toy states and crash halts of the run.
pub(crate) fn oracle_full(rows: &[LogRow], template: Option<&FakeSealer>, roots: &[(u32, [u8; 32])], board: &[u8], relay: &RelayTarget) -> OracleRun {
    oracle_run(rows, template, &pick_roots(rows, template, roots), board, relay)
}

/// The move code the oracle's user types (§31.7, task D7): when the engine
/// has accepted a `Fence` that awaits one (its status shows `move_pending`,
/// `Status::moving`) and the user has not typed one for that `Fence` yet,
/// `Sealer::move_code(f, H(Fence frame))` of the `Fence` at `f` in `rows`, as
/// a device's user types it (§28.3 "On every other device" item 2). The
/// relay the oracle reads is fenced at `f` until then (§12.2, ENG-RECV-007).
fn pending_move(eng: &Engine<ToyMachine, FakeSealer>, rows: &[LogRow], typed: &[(u64, Option<Result<(), &'static str>>)]) -> Option<(u64, String)> {
    let f = eng.status().moving?.fence_seq;
    if typed.iter().any(|(s, _)| *s == f) {
        return None;
    }
    let row = rows.binary_search_by_key(&f, |r| r.seq).ok().map(|i| &rows[i])?;
    let fh: Hash32 = sha2::Sha256::digest(&row.frame).into();
    Some((f, eng.sealer().move_code(f, &fh)))
}

/// Replays `rows` from seq 1 with a fresh engine and no faults (§31.7). Its
/// user types the move code of each accepted `Fence` that awaits one
/// (`pending_move`), and the `Connect` that follows is answered as the first
/// one: the session goes on with the same rows, so that the oracle follows
/// every move in the log (task D7).
fn oracle_run(rows: &[LogRow], template: Option<&FakeSealer>, roots: &[(u32, [u8; 32])], board: &[u8], relay: &RelayTarget) -> OracleRun {
    let mut run =
        OracleRun { states: OracleStates::new(), revoked: BTreeSet::new(), boards: BTreeMap::new(), halts: BTreeSet::new(), moves: Vec::new() };
    let Some(first) = rows.first() else { return run };
    let Some(sealer) = template.and_then(|t| t.observer(0x0a1c_1e00, roots)) else { return run };
    let identity = sealer.export_identity().to_vec();
    let mut store = Store::new(identity, Vec::new(), Bootstrap::Genesis { board: board.to_vec(), frame_1: first.frame.clone(), relay: relay.clone() });
    let toy = ToyMachine::new(true);
    let mut eng: Engine<ToyMachine, FakeSealer> = Engine::new(ToyMachine::new(true), sealer, Config {});
    let mut q: VecDeque<Event<ToyCommand>> = VecDeque::new();
    q.push_back(Event::Start(store.loaded("sim-oracle", [0x0a; 16])));
    let mut served = 1usize;
    let mut mono = 1u64;
    let mut last = Head::default();
    let welcome = |served: usize, wall: i64| Welcome {
        head: Head { seq: rows[served - 1].seq, chain: rows[served - 1].chain },
        last_ctr: None,
        last_seq: None,
        relay_now: wall,
        floor: 0,
        purge_floor: 0,
        fenced: false,
        fence: None,
        appends_left: None,
        push: true,
    };
    let page = |served: usize, wall: i64, after: u64, limit: u16| {
        let from = after as usize;
        let to = (from + usize::from(limit.max(1))).min(served);
        let entries: Vec<PageEntry> =
            rows.get(from..to).unwrap_or(&[]).iter().map(|r| PageEntry { seq: r.seq, at: r.at, frame: r.frame.clone() }).collect();
        Page {
            head: Head { seq: rows[served - 1].seq, chain: rows[served - 1].chain },
            relay_now: wall,
            floor: 0,
            purge_floor: 0,
            more: to < served,
            truncated: false,
            fenced: false,
            entries,
        }
    };
    let refused = || RelayRefusal { status: 403, code: "relay/forbidden".to_owned(), until: None, head: None, index: None };
    let mut guard = 0u64;
    loop {
        let wall = rows[served - 1].at.max(first.at);
        while let Some(ev) = q.pop_front() {
            guard += 1;
            if guard > 50_000_000 {
                run.revoked.clear();
                return run;
            }
            let fx = eng.handle(Input { wall_ms: wall, mono_ms: mono, event: ev });
            mono += 1;
            for e in fx {
                match e {
                    Effect::Persist { id, ops } => {
                        let hist = Some(eng.sealer().history());
                        store.apply(&ops, hist.as_ref());
                        if let Some(h) = store.meta.as_ref().and_then(|m| m.halt.as_ref())
                            && matches!(h.kind, crate::HaltKind::Crash { .. })
                        {
                            run.halts.insert(h.seq);
                        }
                        q.push_back(Event::Persisted { id });
                    }
                    Effect::ReadLog { req, from, to } => {
                        let recs: Vec<_> = (from..=to).filter_map(|s| store.log.get(&s).cloned()).collect();
                        let result = if from <= to && recs.len() as u64 == to - from + 1 {
                            LocalReadResult::Records(recs)
                        } else {
                            LocalReadResult::Missing { code: "device/log_missing" }
                        };
                        q.push_back(Event::LocalRead { req, result });
                    }
                    Effect::ReadBase { req, seq } => {
                        let result = match store.bases.get(&seq) {
                            Some(c) => LocalReadResult::Base(c.clone()),
                            None => LocalReadResult::Missing { code: "device/base_missing" },
                        };
                        q.push_back(Event::LocalRead { req, result });
                    }
                    Effect::Connect { .. } => {
                        q.push_back(Event::Connected { push: true });
                        q.push_back(Event::Welcome(welcome(served, wall)));
                    }
                    Effect::Hello => q.push_back(Event::Welcome(welcome(served, wall))),
                    Effect::Read { req, after, limit } => q.push_back(Event::Page { req: Some(req), page: page(served, wall, after, limit) }),
                    Effect::Append { req, .. } => q.push_back(Event::RelayRefused { req: Some(req), refusal: refused() }),
                    Effect::Admin { req, .. } => q.push_back(Event::AdminResult { req, result: Err::<AdminOk, _>(refused()) }),
                    Effect::Blob { req, .. } => q.push_back(Event::BlobResult { req, result: Err(refused()) }),
                    Effect::Notify(Notification::MoveCodeResult { result }) => {
                        if let Some(m) = run.moves.last_mut() {
                            m.1 = Some(result);
                        }
                    }
                    Effect::Disconnect | Effect::ScheduleTick { .. } | Effect::Notify(_) => {}
                }
            }
            let (st, head) = eng.confirmed();
            if head != last {
                last = head;
                let bh = toy.board_hash(st);
                let sh = crate::state_hash(1, &bh, &eng.membership_bytes());
                keep_first(&mut run.states, head.seq, || (head.chain, bh, sh));
                keep_first(&mut run.boards, head.seq, || st.clone());
            }
        }
        // §28.3 "On every other device" item 2: the user types the move code
        // of an accepted `Fence` that awaits one, before the next entry.
        if let Some((f, text)) = pending_move(&eng, rows, &run.moves) {
            run.moves.push((f, None));
            q.push_back(Event::User(UserAction::MoveCode { text }));
            continue;
        }
        if served >= rows.len() {
            break;
        }
        served += 1;
        let wall = rows[served - 1].at;
        q.push_back(Event::Page { req: None, page: page(served, wall, served as u64 - 1, 1) });
        q.push_back(Event::Tick);
    }
    run.revoked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eng.membership().devices.iter().filter(|d| d.status == DeviceStatus::Revoked).map(|d| d.dev).collect()
    }))
    .unwrap_or_default();
    run
}

/// The board hash at each `B` of `a + 1..=end` of the log `led` replayed from
/// the oracle's first toy state at `a` (`boards`, the oracle's first toy
/// state at each `B`) with the seqs of `out` left out: §31.7 invariant 4's
/// "the reference log replayed with `t` left out", computed from the
/// oracle's classes without running the engine. The toy applies each
/// accepted `cmd` entry in order under `Ctx { seq, d_author, at_ms,
/// based_on }` (§13.1: a parse failure or an `apply` refusal changes
/// nothing), and is upgraded with the entry whose membership activates a
/// newer rules version, before the next entry (§14.1, ENG-SM-002; the board
/// at that seq has it already). An accepted `ResetTo` of target `r` at `q`
/// is applied as a device that never applied the left-out seqs applies it:
/// in an ordinary board apply (§16.2, §17.3 step 1), not a replay, so §28.1
/// "Every device" step 1 compares its state hash at `r` with the entry's
/// (ENG-RESET-002), the hash of the state the oracle held there when it
/// applied the entry: its first state there, as far as `without` finds the
/// replay agreeing with the oracle. The device's state at `r` is the
/// replay's own for a target above `a`, else the oracle's first, which no
/// left-out seq precedes. With the oracle's board
/// there (the same `M_r`, so the same state hash, §18.3) the comparison
/// passes and the board becomes `SM::reset(state_r, state_{q−1})` (step 3),
/// the state a replay through `q` gives without the comparison (§28.1, last
/// paragraph): both agree (task D8, gate 5175). With another board there (a
/// left-out seq changed it) they do not: step 1 raises `ResetMismatch` and
/// leaves the board, a replay resets it, as the oracle's Skip replay at `s`
/// does (ENG-SKIP-002), and §31.7 does not say which one invariant 4 expects
/// (a spec question, task D8's fix pass). The replay then stops before `q`:
/// no expected value, so every state there is a violation (the stricter
/// reading, ruling 6). It also stops before what it does not model: a
/// `Panic` (the apply panics: a crash halt, §15), an accepted Skip (§17.3), a
/// `ResetTo` whose target the oracle never confirmed.
pub(crate) fn replay_without(led: &[Ledger], boards: &BTreeMap<u64, ToyState>, a: u64, end: u64, out: &BTreeSet<u64>) -> BTreeMap<u64, Hash32> {
    let toy = ToyMachine::new(true);
    let mut v = BTreeMap::new();
    let Some(start) = boards.get(&a) else { return v };
    let mut st = start.clone();
    // The replay's own states from `a` on: the targets of a `ResetTo` above `a`.
    let mut own: BTreeMap<u64, ToyState> = BTreeMap::from([(a, st.clone())]);
    for q in a + 1..=end {
        let Some(l) = ledger_at(led, q) else { break };
        if l.accept && !l.skip_targets.is_empty() {
            break;
        }
        if l.accept
            && let Some(r) = l.reset_to
        {
            // The state whose hash the entry names, and the device's own at `r`.
            let Some(named) = boards.get(&r) else { break };
            let mine = if r > a { own.get(&r) } else { Some(named) };
            let Some(target) = mine.filter(|m| toy.board_hash(m) == toy.board_hash(named)) else { break };
            match toy.reset(target, &st) {
                Ok(x) => st = x,
                Err(_) => break,
            }
        }
        if l.accept && l.cmd && !out.contains(&q) {
            match toy.parse(&l.payload) {
                Ok(ToyCommand::Panic) => break,
                Ok(c) => {
                    let ctx = Ctx { seq: q, device_no: l.d_author, at_ms: l.at_ms, based_on: l.based_on };
                    let _ = toy.apply(&mut st, &c, &ctx);
                }
                Err(_) => {}
            }
        }
        if l.rules > st.rules && toy.upgrade(&mut st, l.rules).is_err() {
            break;
        }
        v.insert(q, toy.board_hash(&st));
        own.insert(q, st.clone());
    }
    v
}

/// One log a device may have followed (the reference log, a branch), with
/// what invariant 4 compares on it.
struct LogCtx {
    why: &'static str,
    chain: BTreeMap<u64, Hash32>,
    led: Vec<Ledger>,
    run: OracleRun,
    /// `(t, s)`: each Skip's target and the seq of the first accepted Skip of it.
    windows: Vec<(u64, u64)>,
}

fn windows_of(led: &[Ledger]) -> Vec<(u64, u64)> {
    let mut w: BTreeMap<u64, u64> = BTreeMap::new();
    for l in led.iter().filter(|l| l.accept) {
        for t in &l.skip_targets {
            w.entry(*t).or_insert(l.seq);
        }
    }
    w.into_iter().collect()
}

/// The board hash at each `B` of the windows of the targets `out` on `ctx`,
/// replayed without them (`replay_without`, from the oracle's first state
/// below the lowest target). The replay is checked against the oracle where
/// both ran alike: run with every target the oracle applied (it crash-halts
/// on none but a poison entry, §16.2), it must give the oracle's first state
/// at each `B` where the oracle confirmed one. It covers no `B` from the
/// first disagreement on, and none when nothing was compared: a replay that
/// models the log otherwise than the board apply is no expected value.
fn without(ctx: &LogCtx, out: &BTreeSet<u64>) -> BTreeMap<u64, Hash32> {
    let Some(&t0) = out.first() else { return BTreeMap::new() };
    let a = t0 - 1;
    let end = ctx.windows.iter().filter(|(t, _)| out.contains(t)).map(|(_, s)| s - 1).max().unwrap_or(a);
    let alike: BTreeSet<u64> = out.intersection(&ctx.run.halts).copied().collect();
    let mut upto = end;
    let mut compared = false;
    for (b, h) in replay_without(&ctx.led, &ctx.run.boards, a, end, &alike) {
        let Some(o) = ctx.run.states.get(&b) else { continue };
        if o.1 != h {
            upto = b - 1;
            break;
        }
        compared = true;
    }
    if !compared {
        return BTreeMap::new();
    }
    replay_without(&ctx.led, &ctx.run.boards, a, upto, out)
}

/// Whether the device's state sampled at trace line `line` is that of a
/// device that never applied the Skip's target `t` on the log `chain` (§31.7
/// invariant 4): the last time before it that its board crossed `t` (from
/// below `t` to `t` or above, on that log), its class head `K` was at or past
/// the Skip at `s` on that log, so that the Skip was in its `M_K` when `t`
/// reached its board and `t` was treated as skipped (§12.4 step 1, §17.3 step
/// 1; a crash halt on `t` holds the board below `t` until the Skip clears it,
/// §16.2). Read per state, as the amended text's "it crash-halted on t"
/// requires: a device whose process applied `t` and died with it under its
/// marker (§15) crash-halts on `t`, and its states after the Skip never
/// applied it, while the states of the process that did were held to the
/// oracle. With no crossing of `t` before it (the board started above `t`,
/// as after a join) it is not.
fn never_applied(x: &IdentFacts, chain: &BTreeMap<u64, Hash32>, t: u64, s: u64, line: u64) -> bool {
    x.crossings
        .iter()
        .rev()
        .find(|c| c.line <= line && c.from < t && t <= c.to && chain.get(&c.to) == Some(&c.to_chain))
        .is_some_and(|c| c.k >= s && chain.get(&c.k) == Some(&c.k_chain))
}

/// An acknowledgement a relay gave a device (§31.7 invariant 2: `new` or
/// `dup`, or a `register` or `fence` answer with a seq), whether its answer
/// reached the device or not.
#[derive(Clone, Debug)]
pub(crate) struct AckFact {
    pub eid: [u8; 16],
    pub ident: usize,
    /// The relay state that gave it: relay `r`'s own, or the fork's copy.
    pub sel: Sel,
    pub seq: u64,
    pub frame: Hash32,
    /// The trace line when the relay gave it.
    pub line: u64,
}

/// How the relay made up a frame (the `injected frame` fault of §31.6: "a
/// frame with a bad signature or another board's id"), and so the code a
/// restore tail that carries it is refused with (`relay.md` §12.9 runs the
/// checks of §9.3 on each frame: P7 `relay/bad_signature`, P2
/// `relay/wrong_board`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MadeUp {
    BadSignature,
    OtherBoard,
}

impl MadeUp {
    /// The code a restore tail is refused with at this frame.
    pub(crate) fn code(self) -> &'static str {
        match self {
            MadeUp::BadSignature => "relay/bad_signature",
            MadeUp::OtherBoard => "relay/wrong_board",
        }
    }
}

/// How a relay answered a `restore_tail` (`relay.md` §12.9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    /// It stored the frames.
    Stored,
    /// It refused the request: the code, and the index of the frame the
    /// refusal names (none for a refusal of the whole request).
    Refused(String, Option<u32>),
    /// The request never reached it (lost on the way).
    Unanswered,
    /// It was never sent: its device owed it (§21.3, ENG-EVID-004). In
    /// session with the relay while the relay's head was below its durable
    /// log on its chain, the device sent none and had none on its way for the
    /// response timeout (`World::owe_from`).
    Owed,
}

/// A `restore_tail` a device sent (`relay.md` §12.9, engine.md §21.3), or one
/// it owed and did not send (`Answer::Owed`): what it carried, the relay's
/// first answer (a duplicated copy of the request is no other restore tail),
/// and what the relay and the sender held then.
#[derive(Clone, Debug)]
pub(crate) struct RestoreTry {
    pub sel: Sel,
    /// The trace line when the relay answered it, when it was sent if it
    /// never reached the relay, or when it fell due if it was owed.
    pub line: u64,
    pub first_seq: u64,
    /// The frames it carried (none known for one owed).
    pub frames: Vec<Hash32>,
    pub answer: Answer,
    /// The relay's head when the request reached it, when it was sent if it
    /// never did, or when it fell due.
    pub head: Option<Head>,
    /// The sender's durable log then: the first seq of the unbroken run of
    /// rows that ends at its head, and that head `(L, C_L)`.
    pub holds: Option<(u64, Head)>,
}

impl RestoreTry {
    /// Whether it carried `frame` at `seq`.
    pub(crate) fn carries(&self, seq: u64, frame: &Hash32) -> bool {
        self.frame_at(seq) == Some(frame)
    }

    /// The frame it carried at `seq`.
    fn frame_at(&self, seq: u64) -> Option<&Hash32> {
        seq.checked_sub(self.first_seq).and_then(|i| usize::try_from(i).ok()).and_then(|i| self.frames.get(i))
    }

    /// The seq of the frame it was refused for, when the relay made that
    /// frame up and refused it with the code it refuses such a frame with
    /// (`MadeUp::code`): "refused for a frame the relay made up" (§31.7).
    /// Another code at that frame (`relay/frame_mismatch`,
    /// `relay/unknown_author`, …) refused it for something else.
    pub(crate) fn made_up_at(&self, made_up: &BTreeMap<Hash32, MadeUp>) -> Option<u64> {
        let Answer::Refused(code, Some(i)) = &self.answer else { return None };
        let fh = self.frames.get(*i as usize)?;
        (made_up.get(fh)?.code() == code).then(|| self.first_seq + u64::from(*i))
    }

    /// That a restore tail could have brought back `seq`, as this one shows
    /// (the text of a violation).
    fn could_text(&self, seq: u64) -> String {
        let at = self.head.map_or_else(String::new, |h| format!(" at {} on that tail", h.seq));
        let what = match &self.answer {
            Answer::Stored => "the relay stored one that carried it".to_owned(),
            Answer::Refused(code, None) => format!("the relay{at} refused one as a whole, {code}"),
            Answer::Refused(code, Some(i)) => format!("the relay{at} refused one at its frame {i}, {code}, not for a frame it made up"),
            Answer::Unanswered => format!("one sent while the relay was{at} never reached it"),
            Answer::Owed => format!("its device owed one while the relay was{at}, and sent none in time"),
        };
        format!("a restore tail could have brought back {seq} (trace line {}: {what})", self.line)
    }
}

/// The chain value a log (rows by seq) holds at `seq`.
fn chain_in(rows: &[LogRow], seq: u64) -> Option<Hash32> {
    rows.binary_search_by_key(&seq, |r| r.seq).ok().map(|i| rows[i].chain)
}

/// Whether restore tail `x`, sent after the rollback whose lost tail is `b`,
/// could have brought back `b`'s `frame` at `q` (§31.7: "a dropped seq ...
/// that a restore tail could have brought back, is not removed"). Read by the
/// relay's state when the request met it, not by the request's own answer:
///
/// - stored, it brought back what it carried;
/// - else, unless the relay refused it for a frame it made up: when the
///   relay held a prefix of `b` below `q` (its chain at its head is `b`'s),
///   `b` holds no frame the relay made up above that head up to `q`, and the
///   sender held `b`'s rows from that head to `q`. A restore tail from that
///   head is accepted then (`relay.md` §12.9), and the device sends the rest
///   from its log, each request after the repairs the one before it needs
///   (§21.3 steps 1 and 2); so a request refused there as a whole (a wrong
///   `chain_before` or `first_seq`), at a frame the relay did not make up (a
///   missing repair: `relay/unknown_author`), or that never reached the relay
///   leaves every seq up to `q` to that restore tail, not to another device's
///   entry. Not a request refused `relay/not_next` with the relay's head at
///   or past its first seq: another device restored those seqs first, the
///   device resumes from that head (§21.3 step 3), and its next request is
///   judged on its own (or, sent none, the one it owed from that answer);
/// - one owed and never sent (`Answer::Owed`) as one that never reached the
///   relay, by the relay's state when it fell due: a device that holds more
///   than the relay, on the relay's chain, restores it (§21.3,
///   ENG-EVID-004); one that does not leaves the seq to the restore tail it
///   owed, not to another device's entry.
fn could_restore(x: &RestoreTry, b: &BranchLog, q: u64, frame: &Hash32, made_up: &BTreeMap<Hash32, MadeUp>, logs: &[&[LogRow]]) -> bool {
    match &x.answer {
        Answer::Stored => x.carries(q, frame),
        _ if x.made_up_at(made_up).is_some() => false,
        answer => {
            let Some(h) = x.head else { return false };
            let stale = matches!(answer, Answer::Refused(code, None) if code == "relay/not_next") && h.seq >= x.first_seq;
            let b_at = |s: u64| chain_in(&b.rows, s);
            // The sender held `b`'s rows `h + 1 ..= q`: the request carried
            // them, or its log's unbroken run did, on a log whose chain at
            // `q` is `b`'s (a chain value names the log up to its seq).
            let held = (x.first_seq <= h.seq + 1 && x.carries(q, frame))
                || x.holds.is_some_and(|(lo, top)| {
                    lo <= h.seq + 1 && top.seq >= q && logs.iter().any(|l| chain_in(l, top.seq) == Some(top.chain) && chain_in(l, q) == b_at(q))
                });
            h.seq < q
                && !stale
                && b_at(h.seq) == Some(h.chain)
                && !b.rows.iter().any(|r| r.seq > h.seq && r.seq <= q && made_up.contains_key(&r.hash))
                && held
        }
    }
}

/// The alarms whose evidence is a relay state that contradicts the device's
/// log or acknowledgements: the kinds of §21.1 whose response is the evidence
/// stop of §21.2, the evidence SYNC.md §4.8 lists (an unechoed own entry, a
/// relay head behind the stored one, a chain mismatch, a counter gap, entries
/// that never arrived).
pub(crate) fn is_evidence(k: AlarmKind) -> bool {
    matches!(k, AlarmKind::RelayBehind | AlarmKind::Fork | AlarmKind::Counter | AlarmKind::Unechoed | AlarmKind::Withheld)
}

/// An alarm of `is_evidence`'s kinds, once durable (`Meta`, ENG-EVID-002),
/// with the frames the device's durable log and outbox held then.
#[derive(Clone, Debug)]
pub(crate) struct EvidenceAlarm {
    pub line: u64,
    pub kind: AlarmKind,
    pub held: BTreeSet<Hash32>,
}

/// One move of a device's confirmed head within a process (§31.7 invariant
/// 4): from `from` to `to` (chain `to_chain`), with its class head `K` then,
/// its durable log head (`K` is the last persisted seq, §12.3 step 9), chain
/// `k_chain`, at trace line `line` (that of the sample taken with it).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Crossing {
    pub from: u64,
    pub to: u64,
    pub to_chain: Hash32,
    pub k: u64,
    pub k_chain: Hash32,
    pub line: u64,
}

/// What the end of a run hands the checks.
pub(crate) struct Facts<'a> {
    pub reference: &'a [LogRow],
    pub branches: &'a [BranchLog],
    pub template: Option<&'a FakeSealer>,
    pub roots: Vec<(u32, [u8; 32])>,
    pub board: Vec<u8>,
    pub relay: RelayTarget,
    pub idents: Vec<IdentFacts>,
    /// The acknowledgements delivered to a device: (entry id, ident, line, from the home relay).
    pub acked: &'a [([u8; 16], usize, u64, bool)],
    /// Every acknowledgement a relay gave, delivered or not.
    pub acks: &'a [AckFact],
    pub enqueued: &'a [([u8; 16], usize, u64, u64)],
    /// Pairings' `DeviceAdded` entry ids that `register` acknowledged, with
    /// the device each one adds.
    pub registered: &'a BTreeMap<[u8; 16], Dev>,
    /// The operation each entry's outbox record belongs to (`OutboxRecord.op`): (ident, ticket).
    pub entry_ops: &'a BTreeMap<[u8; 16], (usize, u64)>,
    pub cancelled: &'a BTreeSet<[u8; 16]>,
    pub injected: &'a [Injected],
    /// The frames the relay made up (`injected_frame`, §31.6), by hash.
    pub made_up: &'a BTreeMap<Hash32, MadeUp>,
    /// Every restore tail a device sent.
    pub restore_tails: &'a [RestoreTry],
    /// Every restore tail a device owed and did not send (`Answer::Owed`).
    pub owed: &'a [RestoreTry],
    pub deletions: &'a [(usize, super::world::Deletion, u64, u64)],
    pub t_f: u64,
}

/// One identity at the end of a run.
#[derive(Clone)]
pub(crate) struct IdentFacts {
    pub dev: Dev,
    pub desc: String,
    pub retired: bool,
    /// The trace line at which the user retired it, re-pairing its machine as
    /// a new device after `ReplaceReady` (§24.5, §28.5 step 3; task D7).
    pub retired_at: Option<u64>,
    /// The highest trace line at which a relay state of the reference log (a
    /// relay, not a fork's copy) produced a welcome, page or answer that
    /// reached it; 0 for none.
    pub met_reference: u64,
    /// The highest seq its log reached (an `AppendLog` or a checkpoint).
    pub log_reach: u64,
    /// Live, joined, current, unhalted, with no stop: invariant 1 applies.
    pub converges: bool,
    pub live: bool,
    pub halted_or_stopped_persisted: bool,
    pub revoked: bool,
    pub final_b: Option<(Head, Hash32, Hash32)>,
    pub caught_up: bool,
    pub joined_at: u64,
    pub samples: Vec<(u64, Hash32, Hash32, u64)>,
    /// How its confirmed head moved, with its class head each time (invariant 4).
    pub crossings: Vec<Crossing>,
    /// The seqs of the crash halts its storage held (§16.2).
    pub crash_halts: BTreeSet<u64>,
    /// The seqs `apply` ran for on the lineage of the final confirmed state,
    /// since it was last loaded from bytes (`counted`), oldest first.
    pub applied: Vec<u64>,
    pub alarms: Vec<(u64, AlarmKind, Option<u64>, u64)>,
    /// The frames a relay made up that its engine took into its log (an
    /// `AppendLog` that carried one): (seq, frame hash, trace line).
    pub took: Vec<(u64, Hash32, u64)>,
    /// Its durable alarms of `is_evidence`'s kinds.
    pub evidence: Vec<EvidenceAlarm>,
    pub outbox: Vec<([u8; 16], u64)>,
    pub refusal_seqs: BTreeSet<u64>,
    /// Its `PairFailed` notifications: (the device the pairing adds, trace line).
    pub pair_failed: Vec<(Dev, u64)>,
    /// Its `OpDone` notifications: (ticket, trace line).
    pub op_done: Vec<(u64, u64)>,
    pub expect_alarm: Option<(u64, String)>,
    pub last_line: u64,
}

/// How the relay state that gave an acknowledgement fared (§31.7).
enum Fate<'b> {
    /// A removed relay state: the branch that holds it.
    Removed(&'b BranchLog),
    /// A rolled-back state whose seq is not removed: the reference log holds
    /// that entry there again; or a restore tail could have brought it back
    /// (the one that shows it); or the reference log holds none there and no
    /// restore tail that would have brought it back was refused for a frame
    /// the relay made up.
    Repaired(&'b BranchLog, Option<&'b RestoreTry>),
    /// A state no rollback or fork took away.
    Kept,
}

/// The six invariants (§31.7). Returns (invariant, line, detail) per violation,
/// and a summary per invariant.
pub(crate) fn check(f: &Facts<'_>) -> Vec<(u8, u64, String)> {
    let mut v: Vec<(u8, u64, String)> = Vec::new();
    let reference = f.reference;
    let led = ledger(reference, f.template, &f.roots);
    let run = oracle_full(reference, f.template, &f.roots, &f.board, &f.relay);
    let states = &run.states;
    let head = reference.last().map(|r| Head { seq: r.seq, chain: r.chain }).unwrap_or_default();
    let ref_chain: BTreeMap<u64, Hash32> = reference.iter().map(|r| (r.seq, r.chain)).collect();
    let ref_frame: BTreeMap<u64, Hash32> = reference.iter().map(|r| (r.seq, r.hash)).collect();

    // 1. Convergence.
    for (i, x) in f.idents.iter().enumerate() {
        if !x.converges {
            continue;
        }
        let Some((b, bh, sh)) = x.final_b else { continue };
        if b != head {
            v.push((1, x.last_line, format!("dev {i}: B = {} but the reference head is {}", b.seq, head.seq)));
            continue;
        }
        match states.get(&b.seq) {
            Some((c, obh, osh)) if *c == b.chain && *obh == bh && *osh == sh => {}
            Some(_) => v.push((1, x.last_line, format!("dev {i}: state at {} differs from the oracle's", b.seq))),
            None => v.push((1, x.last_line, format!("dev {i}: the oracle never reached B = {}", b.seq))),
        }
    }

    // The rollback or fork branch on which a device applied an entry: the
    // branch holds it with class `Accept` at `q`, and the device confirmed a
    // `B ≥ q` on that branch's chain (a diagnostic of invariant 3's texts).
    let lost_ledgers: Vec<(&BranchLog, Vec<Ledger>)> = f
        .branches
        .iter()
        .filter(|b| b.why == WHY_ROLLBACK || b.why == WHY_FORK)
        .map(|b| (b, ledger(&b.rows, f.template, &f.roots)))
        .collect();
    let applied_on_lost_branch = |eid: &[u8; 16], x: &IdentFacts| -> Option<(&'static str, u64)> {
        lost_ledgers.iter().find_map(|(br, l)| {
            let q = l.iter().find(|l| l.accept && l.entry_id.as_ref() == Some(eid))?.seq;
            let chain: BTreeMap<u64, Hash32> = br.rows.iter().map(|r| (r.seq, r.chain)).collect();
            x.samples.iter().any(|(b, c, _, _)| *b >= q && chain.get(b) == Some(c)).then_some((br.why, br.line))
        })
    };
    // Task D3: a device whose pairing only a rolled-back or forked relay
    // state holds (its `DeviceAdded` is accepted on such a branch and not in
    // the reference log): the relay forgot its registration with that state,
    // and it is refused `relay/unauthorized` from then on.
    let pairing_lost = |x: &IdentFacts| -> String {
        if x.joined_at == 0 || led.iter().any(|l| l.accept && l.added.contains(&x.dev)) {
            return String::new();
        }
        lost_ledgers
            .iter()
            .find(|(_, l)| l.iter().any(|l| l.accept && l.added.contains(&x.dev)))
            .map(|(br, _)| format!(": its pairing is only on a rolled-back relay state ({}, fault at trace line {})", br.why, br.line))
            .unwrap_or_default()
    };

    // Removed relay states (§31.7, task D6). A dropped seq of a rollback is
    // removed when the reference log, after the settle, holds a different
    // entry there, or holds none because every restore tail that would have
    // brought it back was refused for a frame the relay made up (`relay.md`
    // §12.9); one that the reference log holds again, or that a restore tail
    // could have brought back (`could_restore`: judged by the relay's state
    // when a request met it, so that a request refused as a whole, one stuck
    // behind a refused one, and one lost on the way count as well as one
    // stored; and one a device owed and never sent, by the relay's state when
    // it fell due: a seq is not left to another device's entry because the
    // device that held it sent nothing), is not. A restore tail restores seqs
    // in order from the relay head, each request after the one before it
    // succeeded (§21.3 step 1), so one refused at a made-up frame at `p`
    // brings back no seq from `p` on, whether or not a request carried it: a
    // seq the reference log holds nothing at is removed when the relay refused
    // a restore tail for a made-up frame at or below it (or one that carried
    // it, for a made-up frame above it). A seq a request that carried it was
    // refused for a made-up frame over could be brought back by no other
    // request either (every request from a head below it carries both).
    // Task D7 (§31.7 as amended): a dropped seq the reference log holds none
    // at, above its final head, is also removed when a lower seq that the
    // same rollback dropped holds a different entry in the reference log (no
    // restore tail can bring back a frame whose chain no longer leads to
    // it), and no restore tail could have brought it back before that
    // (the general rule: such a seq is not removed). Nothing wider: one with
    // no such lower seq is held to the rules above.
    let mut logs: Vec<&[LogRow]> = vec![reference];
    logs.extend(f.branches.iter().map(|b| b.rows.as_slice()));
    let overwritten_below = |b: &'_ BranchLog, q: u64| -> bool {
        q > head.seq && b.rows.iter().any(|r| r.seq >= b.removed_from && r.seq < q && ref_frame.get(&r.seq).is_some_and(|h| *h != r.hash))
    };
    let seq_removed = |b: &'_ BranchLog, a: &AckFact| -> (bool, Option<&RestoreTry>) {
        let after: Vec<&RestoreTry> = f.restore_tails.iter().chain(f.owed).filter(|x| x.sel == Sel::Main(0) && x.line > b.line).collect();
        // A refusal for a frame the relay made up on `b`'s tail, at its seq.
        let made_up_on_b = |x: &RestoreTry| x.made_up_at(f.made_up).filter(|p| b.rows.iter().any(|r| r.seq == *p && Some(&r.hash) == x.frame_at(*p)));
        let blocked = after.iter().any(|x| x.carries(a.seq, &a.frame) && made_up_on_b(x).is_some());
        let could = after
            .iter()
            .copied()
            .find(|x| (x.answer == Answer::Stored || !blocked) && could_restore(x, b, a.seq, &a.frame, f.made_up, &logs));
        let removed = match ref_frame.get(&a.seq) {
            Some(h) => *h != a.frame && could.is_none(),
            None => {
                could.is_none()
                    && (after.iter().any(|x| made_up_on_b(x).is_some_and(|p| p <= a.seq || x.carries(a.seq, &a.frame))) || overwritten_below(b, a.seq))
            }
        };
        (removed, could)
    };
    // An acknowledgement comes from a removed relay state when the relay gave
    // it at a seq of that state: on a fork's copy (every branch the oracle
    // does not name), or before a rollback (relay 0, the home before any
    // move) whose dropped seqs include it and whose lost tail held that frame
    // there, at a removed seq.
    let fate = |a: &AckFact| {
        match a.sel {
            Sel::Fork => f.branches.iter().rev().find(|b| b.why == WHY_FORK && b.line <= a.line).map_or(Fate::Kept, Fate::Removed),
            Sel::Main(r) => {
                let mut repaired = None;
                for b in f.branches.iter().filter(|b| b.why == WHY_ROLLBACK && r == 0 && b.line > a.line && b.removed_from <= a.seq) {
                    let held = b.rows.binary_search_by_key(&a.seq, |x| x.seq).is_ok_and(|i| b.rows[i].hash == a.frame);
                    if !held {
                        continue;
                    }
                    match seq_removed(b, a) {
                        (true, _) => return Fate::Removed(b),
                        (false, could) => {
                            repaired.get_or_insert((b, could));
                        }
                    }
                }
                repaired.map_or(Fate::Kept, |(b, could)| Fate::Repaired(b, could))
            }
        }
    };
    // A device that held the frame a removed state acknowledged raised an
    // alarm of §21.1 with evidence (`is_evidence`) after that fault.
    let alarmed = |a: &AckFact, b: &BranchLog| {
        f.idents.iter().any(|x| x.evidence.iter().any(|e| is_evidence(e.kind) && e.line > b.line && e.held.contains(&a.frame)))
    };
    let acks_of = |eid: &[u8; 16]| {
        let e = *eid;
        f.acks.iter().filter(move |a| a.eid == e)
    };
    // "Never lost silently" (§31.7 invariant 2, third outcome): every
    // acknowledgement of it came from a removed relay state, and a device that
    // held its frame raised an alarm with evidence. Strict reading: at least
    // one acknowledgement (an entry no relay acknowledged was never lost by
    // one). Otherwise, why not.
    let quiet = |eid: &[u8; 16]| -> Result<(), String> {
        let mut removed = Vec::new();
        for a in acks_of(eid) {
            match fate(a) {
                Fate::Removed(b) => removed.push((a, b)),
                Fate::Repaired(b, could) => {
                    let why = match (ref_frame.get(&a.seq), could) {
                        (Some(h), _) if *h == a.frame => format!("the reference log holds it at {} again", a.seq),
                        (Some(_), Some(x)) => format!("another entry took {}, but {}", a.seq, x.could_text(a.seq)),
                        (None, Some(x)) => format!("the reference log holds nothing at {}, and {}", a.seq, x.could_text(a.seq)),
                        (_, None) => format!(
                            "the reference log holds nothing at {}, nor another entry at a lower seq the rollback dropped, and no restore tail that would have brought it back was refused for a frame the relay made up",
                            a.seq
                        ),
                    };
                    return Err(format!("lost to a rolled-back relay state that is not removed ({}, fault at trace line {}): {why}", b.why, b.line));
                }
                Fate::Kept => return Err(format!("acknowledged at {} (trace line {}) by a relay state no rollback or fork removed", a.seq, a.line)),
            }
        }
        let Some((_, b0)) = removed.first() else { return Err("no relay acknowledged it".to_owned()) };
        if removed.iter().any(|(a, b)| alarmed(a, b)) {
            Ok(())
        } else {
            Err(format!(
                "lost to a removed relay state ({}, fault at trace line {}), and no device that held its frame raised an alarm of §21.1 with evidence after it",
                b0.why, b0.line
            ))
        }
    };
    // "Its device reported the refusal" (§31.7 invariant 2, second outcome):
    // a device the relay acknowledged it to holds a refusal record at its seq,
    // or emitted, after the relay sequenced it, the `PairFailed` of the
    // pairing whose `DeviceAdded` it is (§28.4 step 4) or the `OpDone` of the
    // operation its outbox record belonged to (§17.1).
    // The devices an entry is acknowledged to: its device (invariant 2).
    let devs_of = |eid: &[u8; 16]| -> BTreeSet<usize> {
        acks_of(eid).map(|a| a.ident).chain(f.acked.iter().filter(|x| x.0 == *eid).map(|x| x.1)).collect()
    };
    let reported = |eid: &[u8; 16], q: u64, delivered: u64| -> bool {
        let given = acks_of(eid).filter(|a| a.seq == q).map(|a| a.line).min().unwrap_or(delivered);
        devs_of(eid).iter().filter_map(|d| f.idents.get(*d).map(|x| (*d, x))).any(|(d, x)| {
            x.refusal_seqs.contains(&q)
                || f.registered.get(eid).is_some_and(|pd| x.pair_failed.iter().any(|(p, l)| p == pd && *l > given))
                || f.entry_ops.get(eid).is_some_and(|(od, t)| *od == d && x.op_done.iter().any(|(tt, l)| tt == t && *l > given))
        })
    };

    // 2. Acknowledged entries. One of the four outcomes of §31.7, read
    // strictly: accepted exactly once; held once, finally refused, and
    // reported by its device; never lost silently; its device retired while
    // cut off.
    let mut by_id: BTreeMap<[u8; 16], Vec<&Ledger>> = BTreeMap::new();
    for l in &led {
        if let Some(e) = l.entry_id {
            by_id.entry(e).or_default().push(l);
        }
    }
    let accepted_once = |e: &[u8; 16]| by_id.get(e).map_or(0, |ls| ls.iter().filter(|l| l.accept).count());
    // "Its device was retired while cut off" (§31.7 invariant 2, last
    // outcome; the owner's decision of 2026-11-25, task D7): every device of
    // `devs` (the entry's device) was retired, its machine re-paired as a new
    // device after a `ReplaceReady` that named it (§24.5, §28.5 step 3), and
    // before that it had not met what became of the entry: every
    // acknowledgement of it came from a removed relay state (at least one, as
    // for "never lost silently") and the device met no relay state of the
    // reference log after the fault that removed it (the earliest such
    // fault: no welcome, page or answer that a relay, not a fork's copy, gave
    // from that fault on, `IdentFacts::met_reference`); or the reference log
    // holds it once, finally refused, at a seq the device's log never reached
    // (`IdentFacts::log_reach`). Nothing wider: a device not retired so, one
    // that met the reference relay after the fault, one whose log reached the
    // refusal's seq is held to the other outcomes. `Err` says why not, for a
    // retired device (empty for one that was not retired at all).
    let retired_cut_off = |eid: &[u8; 16], devs: &BTreeSet<usize>| -> Result<(), String> {
        let xs: Vec<(usize, &IdentFacts)> = devs.iter().filter_map(|d| f.idents.get(*d).map(|x| (*d, x))).collect();
        if xs.is_empty() || xs.len() != devs.len() || xs.iter().any(|(_, x)| !x.retired) {
            return Err(String::new());
        }
        if let Some((d, _)) = xs.iter().find(|(_, x)| x.retired_at.is_none()) {
            return Err(format!("; its device (dev {d}) was retired, but not after a ReplaceReady that named it"));
        }
        let rows: &[&Ledger] = by_id.get(eid).map_or(&[], Vec::as_slice);
        match rows {
            [l] if l.class == "refuse" => match xs.iter().find(|(_, x)| x.log_reach >= l.seq) {
                None => Ok(()),
                Some((d, x)) => Err(format!("; its device (dev {d}) was retired, but its log reached {} (the refusal is at {})", x.log_reach, l.seq)),
            },
            [] => {
                let mut first: Option<u64> = None;
                for a in acks_of(eid) {
                    let Fate::Removed(b) = fate(a) else {
                        return Err(format!("; its device was retired, but the relay acknowledged it at {} (trace line {}) from a relay state no fault removed", a.seq, a.line));
                    };
                    first = Some(first.map_or(b.line, |l| l.min(b.line)));
                }
                let Some(first) = first else { return Err("; its device was retired, but no relay acknowledged it".to_owned()) };
                match xs.iter().find(|(_, x)| x.met_reference >= first) {
                    None => Ok(()),
                    Some((d, x)) => Err(format!(
                        "; its device (dev {d}) was retired, but met a relay state of the reference log (trace line {}) after the fault at trace line {first}",
                        x.met_reference
                    )),
                }
            }
            _ => Err(String::new()),
        }
    };
    let mut seen_ack = BTreeSet::new();
    for (eid, dev, line, _) in f.acked {
        if !seen_ack.insert(*eid) {
            continue;
        }
        let rows: &[&Ledger] = by_id.get(eid).map_or(&[], Vec::as_slice);
        let n = rows.iter().filter(|l| l.accept).count();
        if n == 1 {
            continue;
        }
        let classes: Vec<&str> = rows.iter().map(|l| l.class).collect();
        let base = format!("dev {dev}: acknowledged entry {} is accepted {n} times in the reference log (classes there: {classes:?})", hexs(eid));
        if n == 0 && rows.len() == 1 && rows[0].class == "refuse" {
            if reported(eid, rows[0].seq, *line) {
                continue;
            }
            let Err(retired) = retired_cut_off(eid, &devs_of(eid)) else { continue };
            let what = if f.registered.contains_key(eid) {
                "a pairing's DeviceAdded refused after register (§28.4 step 4), "
            } else if f.entry_ops.contains_key(eid) {
                "an operation's entry, "
            } else {
                ""
            };
            v.push((2, *line, format!(
                "{base}: {what}finally refused at {}, and its device reported no refusal record, PairFailed of its pairing or end of its operation for it{retired}",
                rows[0].seq
            )));
            continue;
        }
        if n == 0 && rows.is_empty() {
            match quiet(eid) {
                Ok(()) => {}
                Err(why) => {
                    let Err(retired) = retired_cut_off(eid, &devs_of(eid)) else { continue };
                    v.push((2, *line, format!("{base}: {why}{retired}")));
                }
            }
            continue;
        }
        v.push((2, *line, base));
    }

    // 2, second half: every device whose B is past that seq has applied it
    // exactly once. Read on the log the device followed (the reference log,
    // or the branch whose chain it holds at B), from the first seq its final
    // state's lineage applied (what came before is in the bytes it loaded,
    // which invariants 1 and 4 compare). A Cmd entry is applied once; one an
    // accepted Skip at or below B targets is applied by no device that
    // replayed past the Skip (ENG-SKIP-002), so it is expected 0 times there.
    let mut branch_ledgers: BTreeMap<usize, Vec<Ledger>> = BTreeMap::new();
    let acked_ids: BTreeSet<[u8; 16]> = f.acked.iter().map(|a| a.0).collect();
    for (i, x) in f.idents.iter().enumerate() {
        let Some((b, _, _)) = x.final_b else { continue };
        let Some(&floor) = x.applied.first() else { continue };
        let on: &[Ledger] = if ref_chain.get(&b.seq) == Some(&b.chain) {
            &led
        } else {
            let Some(k) = f.branches.iter().position(|br| br.rows.iter().any(|r| r.seq == b.seq && r.chain == b.chain)) else { continue };
            branch_ledgers.entry(k).or_insert_with(|| ledger(&f.branches[k].rows, f.template, &f.roots))
        };
        let mut times: BTreeMap<u64, usize> = BTreeMap::new();
        for s in &x.applied {
            *times.entry(*s).or_default() += 1;
        }
        let skipped: BTreeSet<u64> =
            on.iter().filter(|l| l.accept && l.seq <= b.seq).flat_map(|l| l.skip_targets.iter().copied()).collect();
        for l in on {
            let Some(eid) = l.entry_id else { continue };
            if !l.accept || !l.cmd || l.seq < floor || l.seq > b.seq || l.seq <= x.joined_at || !acked_ids.contains(&eid) {
                continue;
            }
            let n = times.get(&l.seq).copied().unwrap_or(0);
            let want = usize::from(!skipped.contains(&l.seq));
            if n != want {
                v.push((2, x.last_line, format!(
                    "dev {i}: B = {} is past seq {} of acknowledged entry {}, which it applied {n} times (expected {want})",
                    b.seq, l.seq, hexs(&eid)
                )));
            }
        }
    }

    // 3. Queued edits. The outcomes of §31.7, read strictly; and a record
    // dropped as a cancelled hold (ruling 5 of the D2 brief:
    // `Coalesce::CancelBoth`, matched to its `Forget` when it was dropped).
    for (eid, dev, _, line) in f.enqueued {
        if f.cancelled.contains(eid) {
            continue;
        }
        let n = accepted_once(eid);
        if n == 1 {
            continue;
        }
        let x = &f.idents[*dev];
        let in_outbox = x.outbox.iter().any(|(o, _)| o == eid);
        let revoked = x.revoked || run.revoked.contains(&x.dev);
        let held = n == 0 && in_outbox && (x.halted_or_stopped_persisted || revoked);
        let refused = n == 0
            && by_id.get(eid).is_some_and(|ls| ls.iter().any(|l| !l.accept && x.refusal_seqs.contains(&l.seq)));
        if held || refused {
            continue;
        }
        // Never lost silently, as invariant 2 states it (task D6).
        let quiet = if n == 0 { quiet(eid) } else { Err(String::new()) };
        let Err(why) = quiet else { continue };
        // Its device retired while cut off, as invariant 2 states it (task D7).
        let retired = if n == 0 { retired_cut_off(eid, &BTreeSet::from([*dev])) } else { Err(String::new()) };
        let Err(retired) = retired else { continue };
        let mut why = if why.is_empty() { String::new() } else { format!(": {why}") };
        why.push_str(&retired);
        // Task D3: a record its device applied on a relay state that a
        // rollback or a fork later removed from the reference branch is
        // named as such, with the fault's trace line.
        match if n == 0 && !in_outbox { applied_on_lost_branch(eid, x) } else { None } {
            Some((b, l)) => why.push_str(&format!("; applied on a rolled-back relay state ({b}, fault at trace line {l})")),
            None => why.push_str(&pairing_lost(x)),
        }
        v.push((3, *line, format!("dev {dev}: enqueued entry {} is accepted {n} times and not held or refused (in its outbox: {in_outbox}){why}", hexs(eid))));
    }

    // 4. Tampering (§31.7): the confirmed state at every B past the device's
    // join against the oracle's first state at that B on the log it followed
    // (the reference log, or the branch whose chain it holds at B), skipping
    // none (ruling 6 of the D2 brief), with one exception (task D6): between
    // a Skip's target t and the Skip at s (t ≤ B < s), a device that never
    // applied t (`never_applied`) is compared with that log replayed with t
    // left out (`without`: the toy on the oracle's classes, no engine;
    // ENG-SKIP-002, ENG-HALT-004). A B in a window that no expected value
    // covers is a violation.
    let ref_ctx = LogCtx { why: "the reference log", chain: ref_chain.clone(), windows: windows_of(&led), led: led.clone(), run };
    let mut branch_ctx: BTreeMap<usize, LogCtx> = BTreeMap::new();
    let mut without_cache: BTreeMap<(usize, Vec<u64>), BTreeMap<u64, Hash32>> = BTreeMap::new();
    for (i, x) in f.idents.iter().enumerate() {
        for (b, c, bh, line) in &x.samples {
            if *b <= x.joined_at {
                continue;
            }
            let k = if ref_chain.get(b) == Some(c) {
                0
            } else {
                match f.branches.iter().position(|br| br.rows.iter().any(|r| r.seq == *b && r.chain == *c)) {
                    Some(p) => p + 1,
                    None => {
                        // On no branch at all: the device's chain at B was served by no relay.
                        v.push((4, *line, format!("dev {i}: confirmed state at {b} on a chain no relay served")));
                        continue;
                    }
                }
            };
            let ctx: &LogCtx = if k == 0 {
                &ref_ctx
            } else {
                branch_ctx.entry(k).or_insert_with(|| {
                    let br = &f.branches[k - 1];
                    let led = ledger(&br.rows, f.template, &f.roots);
                    LogCtx {
                        why: br.why,
                        chain: br.rows.iter().map(|r| (r.seq, r.chain)).collect(),
                        windows: windows_of(&led),
                        run: oracle_full(&br.rows, f.template, &f.roots, &f.board, &f.relay),
                        led,
                    }
                })
            };
            let win: Vec<(u64, u64)> = ctx.windows.iter().filter(|(t, s)| t <= b && b < s).copied().collect();
            let out: BTreeSet<u64> = win.iter().filter(|(t, s)| never_applied(x, &ctx.chain, *t, *s, *line)).map(|w| w.0).collect();
            let on = ctx.why;
            if out.is_empty() {
                // The oracle's state at B, when it is on the same chain.
                let same = ctx.run.states.get(b).filter(|(oc, _, _)| oc == c);
                if let Some((_, obh, _)) = same
                    && obh != bh
                {
                    let window = win.first().map(|(_, s)| format!(" (between a Skip's target and the Skip at {s})")).unwrap_or_default();
                    let halted = win.iter().find(|(t, _)| x.crash_halts.contains(t)).map(|(t, _)| format!("; it crash-halted on the target {t}")).unwrap_or_default();
                    v.push((4, *line, format!("dev {i}: confirmed state at {b} differs from the oracle's on {on}{window}{halted}")));
                }
                // Ruling 6: nothing between a Skip's target and the Skip goes
                // uncompared; a B there the oracle never confirmed is reported.
                if same.is_none()
                    && let Some((_, s)) = win.first()
                {
                    v.push((4, *line, format!("dev {i}: confirmed state at {b} between a Skip's target and the Skip at {s}, where the oracle confirmed nothing")));
                }
            } else {
                let ts: Vec<u64> = out.iter().copied().collect();
                let s = win.iter().filter(|(t, _)| out.contains(t)).map(|w| w.1).min().unwrap_or(0);
                let exp = without_cache.entry((k, ts.clone())).or_insert_with(|| without(ctx, &out)).get(b).copied();
                match exp {
                    Some(e) if e == *bh => {}
                    Some(_) => v.push((4, *line, format!(
                        "dev {i}: confirmed state at {b} differs from {on} replayed without {ts:?}, which it never applied (between a Skip's target and the Skip at {s})"
                    ))),
                    None => v.push((4, *line, format!(
                        "dev {i}: confirmed state at {b} between a Skip's target and the Skip at {s}, where no replay of {on} without {ts:?}, which it never applied, reaches"
                    ))),
                }
            }
        }
        if let Some((line, what)) = &x.expect_alarm {
            v.push((4, *line, format!("dev {i}: {what}, and no alarm was raised")));
        }
    }
    // Injected frames (§31.7 invariant 4 as amended, task D7): every one
    // raised `SkippedFrame`, except one above a `Fence` that the relay
    // showing it sequenced (an accepted `Fence`, below the frame in that
    // relay's log, that moves the board to another relay: the relay it
    // fenced, §12.2), which no device takes (§12.3 step 5, ENG-RECV-007). Such
    // a frame is checked instead to be in no device's log: a device that took
    // it broke ENG-RECV-007. Its log is the one that holds it at its seq with
    // its chain (the reference log; the old relay's after a move; a
    // rollback's lost tail).
    let mut inj_ledgers: BTreeMap<usize, Vec<Ledger>> = BTreeMap::new();
    for inj in f.injected {
        let (s, line) = (inj.seq, inj.line);
        let fenced_at = {
            let on: Option<&[Ledger]> = if ref_chain.get(&s) == Some(&inj.chain) {
                Some(led.as_slice())
            } else {
                f.branches
                    .iter()
                    .position(|b| chain_in(&b.rows, s) == Some(inj.chain))
                    .map(|k| inj_ledgers.entry(k).or_insert_with(|| ledger(&f.branches[k].rows, f.template, &f.roots)).as_slice())
            };
            on.and_then(|l| l.iter().find(|l| l.accept && l.seq < s && l.fence_host.as_ref().is_some_and(|h| *h != inj.host)).map(|l| l.seq))
        };
        if let Some(fs) = fenced_at {
            for (i, x) in f.idents.iter().enumerate() {
                if let Some((_, _, at)) = x.took.iter().find(|t| t.1 == inj.frame) {
                    v.push((4, *at, format!(
                        "dev {i}: took the injected frame at {s} into its log, above the Fence at {fs} that fenced the relay showing it (ENG-RECV-007)"
                    )));
                }
            }
            continue;
        }
        // Only a frame the reference log still holds (a rollback may have removed it).
        if ref_chain.get(&s) != Some(&inj.chain) {
            continue;
        }
        for (i, x) in f.idents.iter().enumerate() {
            // A device that confirmed a seq at or past the frame on the
            // reference branch has received it (one on another branch there
            // never saw it).
            let passed = x.samples.iter().any(|(b, c, _, _)| *b >= s && ref_chain.get(b) == Some(c));
            if x.joined_at >= s || !passed || x.retired {
                continue;
            }
            if !x.alarms.iter().any(|a| a.1 == AlarmKind::SkippedFrame && a.2 == Some(s)) {
                v.push((4, line, format!("dev {i}: the injected frame at {s} raised no SkippedFrame")));
            }
        }
    }

    // 5. No deletion by the relay. Each deletion is judged on the log the
    // device followed at the seq it is judged at: the reference log, or the
    // branch whose chain it holds there (`Deletion::chain`). A fork's copy
    // can sequence a rotation, or a purge, that the reference log never
    // holds, and a device on it applies that entry as any accepted one
    // (task D5 for `Pending` and `Snap`; task D8, gate 17163374614648704986
    // and 5685, for the purge's `TruncateLogBelow`, `DeleteBases` and
    // `forget_roots_below`, §28.2 "On every device"). With no chain, or one
    // no log holds there, the reference log.
    let mut del_ledgers: BTreeMap<usize, Vec<Ledger>> = BTreeMap::new();
    for (dev, d, b, line) in f.deletions {
        use super::world::Deletion;
        let k = d
            .chain()
            .filter(|c| ref_chain.get(b) != Some(c))
            .and_then(|c| f.branches.iter().position(|br| br.rows.iter().any(|r| r.seq == *b && r.chain == c)));
        let on: &[Ledger] = match k {
            None => &led,
            Some(k) => del_ledgers.entry(k).or_insert_with(|| ledger(&f.branches[k].rows, f.template, &f.roots)),
        };
        // The accepted purge rotations of that log: (seq, purge_before, new epoch, τ there).
        let purges = || on.iter().filter(|l| l.accept).filter_map(|l| Some((l.seq, l.purge_before?, l.new_epoch?, l.tau)));
        let ok = match d {
            Deletion::Truncate(p, ctx, _) => *ctx || purges().any(|(r, pb, _, _)| pb == *p && r <= *b) || (*p + 16_384 <= *b + 1),
            Deletion::Bases(seqs, ctx, _) => *ctx || purges().any(|(r, _, _, _)| r <= *b && seqs.iter().all(|s| *s < r)),
            Deletion::Roots(es, _) => purges().any(|(r, _, n, tau)| {
                r <= *b && es.iter().all(|e| *e < n) && on.iter().any(|l| l.seq <= *b && l.tau > tau + 2_592_000_000)
            }),
            // An identity change dropping the pending roots of own rotations:
            // each one's own entry is at or below B in the log the device
            // followed there, either accepted (it reached the board head) or
            // refused (a refused own rotation, SEAL-MEM-016). Nothing else
            // excuses the drop of one.
            Deletion::Pending(ids, _) => {
                let at: BTreeMap<[u8; 16], u64> = on.iter().filter_map(|l| Some((l.entry_id?, l.seq))).rev().collect();
                ids.iter().all(|e| at.get(e).is_some_and(|s| s <= b))
            }
            // Task D5 (§19.4 step 5, ENG-SNAP-004): a `snap` record never
            // sent, dropped at or after an accepted rotation that ended the
            // epoch its blob was sealed under, in the log the device followed
            // there, where no row carries its entry. Nothing wider: no
            // rotation, a refused one, one above B, or one to an epoch no
            // later than the blob's explains nothing.
            Deletion::Snap(eid, epoch, _) => {
                // Never sent: no row of that log carries its entry.
                !on.iter().any(|l| l.entry_id == Some(*eid))
                    && on.iter().any(|l| l.accept && l.seq <= *b && l.new_epoch.is_some_and(|n| n > *epoch))
            }
        };
        if !ok {
            v.push((5, *line, format!("dev {dev}: {d:?} with B = {b} is not explained by §31.7 invariant 5")));
        }
    }

    // 6. Settling (the parts the world does not check while it runs).
    for (i, x) in f.idents.iter().enumerate() {
        if x.retired {
            continue;
        }
        if !(x.live && x.caught_up) && !x.halted_or_stopped_persisted {
            v.push((6, x.last_line, format!("dev {i}: neither live and caught up nor halted or stopped with a persisted reason ({}){}", x.desc, pairing_lost(x))));
        }
        if x.live && !x.halted_or_stopped_persisted {
            for (e, put) in &x.outbox {
                if *put < f.t_f + 600_000 {
                    v.push((6, x.last_line, format!("dev {i}: outbox record {} from {} ms is still queued", hexs(e), put)));
                    break;
                }
            }
        }
    }
    v
}

pub(crate) fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::script::{Case, HandLog};

    // §31.7 invariant 4 (task D6): the replay without a Skip's target is an
    // expected value only as far as the same replay, run as the oracle ran
    // (here with the target applied), gives the oracle's first states: from
    // the first `B` where they disagree it covers nothing, and with nothing
    // to compare (no oracle state from the target to the Skip) it covers no
    // `B` at all.
    #[test]
    fn a_replay_that_disagrees_with_the_oracle_is_no_expected_value() {
        let mut log = HandLog::new(41);
        for k in 0..3u8 {
            log.set(k, 10 + i64::from(k));
        }
        let (t, _) = log.set(1, 99);
        log.set(2, 5);
        log.set(4, 6);
        let (s, _) = log.skip(t);
        let c = Case::of(&log);
        let led = ledger(&c.reference, Some(&c.template), &c.roots);
        let mut ctx = LogCtx {
            why: "the reference log",
            chain: c.reference.iter().map(|r| (r.seq, r.chain)).collect(),
            windows: windows_of(&led),
            run: oracle_full(&c.reference, Some(&c.template), &c.roots, &c.board, &c.relay),
            led,
        };
        assert_eq!(ctx.windows, vec![(t, s)]);
        let out = BTreeSet::from([t]);
        let keys = |ctx: &LogCtx| without(ctx, &out).keys().copied().filter(|b| *b < s).collect::<Vec<u64>>();
        assert_eq!(keys(&ctx), (t..s).collect::<Vec<u64>>(), "every B of the window");
        ctx.run.states.get_mut(&(t + 1)).expect("the oracle's state at t + 1").1 = [0xee; 32];
        assert_eq!(keys(&ctx), vec![t], "nothing from the first disagreement on");
        for b in t..s {
            ctx.run.states.remove(&b);
        }
        assert_eq!(keys(&ctx), Vec::<u64>::new(), "nothing compared");
    }
}
