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
//! the same kind of sealer: class, entry id, purge rotations.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::relay::LogRow;
use super::store::Store;
use crate::fake::{FakeMembership, FakeSealer};
use crate::toy::{ToyCommand, ToyMachine};
use crate::{
    AdminOk, AlarmKind, Bootstrap, Class, ClassifyInput, Config, CtlItem, Dev, DeviceStatus, Effect, Engine, Event, Hash32, Head, Input, Kind,
    LocalReadResult, Page, PageEntry, RelayRefusal, RelayTarget, Sealer, StateMachine, Welcome,
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
    pub tau: i64,
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

/// Classifies `rows` (seqs 1..) with an observer of `template` holding `roots`.
pub(crate) fn ledger(rows: &[LogRow], template: Option<&FakeSealer>, roots: &[(u32, [u8; 32])]) -> Vec<Ledger> {
    let mut out = Vec::new();
    let Some(mut s) = template.and_then(|t| t.observer(0x0b5e_77e7, roots)) else { return out };
    let mut m: FakeMembership = s.membership_zero();
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
        let Ok(rc) = s.classify(&input) else {
            out.push(Ledger { seq: r.seq, accept: false, class: "error", entry_id: None, cmd: false, purge_before: None, new_epoch: None, skip_targets: vec![], tau });
            prev = r.chain;
            continue;
        };
        let mut purge_before = None;
        let mut new_epoch = None;
        let mut skip_targets = Vec::new();
        let mut entry_id = None;
        let mut cmd = false;
        if let Some(o) = &rc.opened {
            entry_id = Some(o.entry_id);
            cmd = o.kind == Kind::Cmd;
            if rc.class == Class::Accept {
                tau = tau.max(o.at_ms);
            }
            for it in &o.items {
                match it {
                    CtlItem::EpochRotated { epoch, purge_before: p } => {
                        new_epoch = Some(*epoch);
                        purge_before = *p;
                    }
                    CtlItem::Skip { seq, .. } => skip_targets.push(*seq),
                    _ => {}
                }
            }
        }
        out.push(Ledger {
            seq: r.seq,
            accept: rc.class == Class::Accept,
            class: class_name(&rc.class),
            entry_id,
            cmd,
            purge_before,
            new_epoch,
            skip_targets,
            tau,
        });
        m = rc.membership;
        prev = r.chain;
    }
    out
}

/// The oracle's confirmed state at each `B` it passed: `B → (C_B, board hash, state hash)`.
pub(crate) type OracleStates = BTreeMap<u64, (Hash32, Hash32, Hash32)>;

/// Replays `rows` from seq 1 with a fresh engine and no faults (§31.7).
pub(crate) fn oracle(rows: &[LogRow], template: Option<&FakeSealer>, roots: &[(u32, [u8; 32])], board: &[u8], relay: &RelayTarget) -> OracleStates {
    oracle_run(rows, template, roots, board, relay).0
}

/// The oracle's states, and the devices its final membership lists as revoked.
fn oracle_run(
    rows: &[LogRow],
    template: Option<&FakeSealer>,
    roots: &[(u32, [u8; 32])],
    board: &[u8],
    relay: &RelayTarget,
) -> (OracleStates, BTreeSet<Dev>) {
    let mut states = OracleStates::new();
    let Some(first) = rows.first() else { return (states, BTreeSet::new()) };
    let Some(sealer) = template.and_then(|t| t.observer(0x0a1c_1e00, roots)) else { return (states, BTreeSet::new()) };
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
                return (states, BTreeSet::new());
            }
            let fx = eng.handle(Input { wall_ms: wall, mono_ms: mono, event: ev });
            mono += 1;
            for e in fx {
                match e {
                    Effect::Persist { id, ops } => {
                        let hist = Some(eng.sealer().history());
                        store.apply(&ops, hist.as_ref());
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
                    Effect::Disconnect | Effect::ScheduleTick { .. } | Effect::Notify(_) => {}
                }
            }
            let (st, head) = eng.confirmed();
            if head != last {
                last = head;
                let bh = toy.board_hash(st);
                let sh = crate::state_hash(1, &bh, &eng.membership_bytes());
                states.entry(head.seq).or_insert((head.chain, bh, sh));
            }
        }
        if served >= rows.len() {
            break;
        }
        served += 1;
        let wall = rows[served - 1].at;
        q.push_back(Event::Page { req: None, page: page(served, wall, served as u64 - 1, 1) });
        q.push_back(Event::Tick);
    }
    let revoked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eng.membership().devices.iter().filter(|d| d.status == DeviceStatus::Revoked).map(|d| d.dev).collect()
    }))
    .unwrap_or_default();
    (states, revoked)
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
    pub acked: &'a [([u8; 16], usize, u64, bool)],
    pub enqueued: &'a [([u8; 16], usize, u64, u64)],
    pub cancelled: &'a BTreeSet<[u8; 16]>,
    pub injected: &'a [(u64, Hash32, u64)],
    pub deletions: &'a [(usize, super::world::Deletion, u64, u64)],
    pub t_f: u64,
}

/// One identity at the end of a run.
pub(crate) struct IdentFacts {
    pub dev: Dev,
    pub desc: String,
    pub retired: bool,
    /// Live, joined, current, unhalted, with no stop: invariant 1 applies.
    pub converges: bool,
    pub live: bool,
    pub halted_or_stopped_persisted: bool,
    pub revoked: bool,
    pub final_b: Option<(Head, Hash32, Hash32)>,
    pub caught_up: bool,
    pub joined_at: u64,
    pub samples: Vec<(u64, Hash32, Hash32, u64)>,
    /// The seqs `apply` ran for on the lineage of the final confirmed state,
    /// since it was last loaded from bytes (`counted`), oldest first.
    pub applied: Vec<u64>,
    pub alarms: Vec<(u64, AlarmKind, Option<u64>, u64)>,
    pub outbox: Vec<([u8; 16], u64)>,
    pub refusal_seqs: BTreeSet<u64>,
    pub expect_alarm: Option<(u64, String)>,
    pub last_line: u64,
}

/// The six invariants (§31.7). Returns (invariant, line, detail) per violation,
/// and a summary per invariant.
pub(crate) fn check(f: &Facts<'_>) -> Vec<(u8, u64, String)> {
    let mut v: Vec<(u8, u64, String)> = Vec::new();
    let reference = f.reference;
    let led = ledger(reference, f.template, &f.roots);
    let (states, revoked_in_ref) = oracle_run(reference, f.template, &f.roots, &f.board, &f.relay);
    let head = reference.last().map(|r| Head { seq: r.seq, chain: r.chain }).unwrap_or_default();
    let ref_chain: BTreeMap<u64, Hash32> = reference.iter().map(|r| (r.seq, r.chain)).collect();

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

    // 2. Acknowledged entries.
    let mut by_id: BTreeMap<[u8; 16], Vec<&Ledger>> = BTreeMap::new();
    for l in &led {
        if let Some(e) = l.entry_id {
            by_id.entry(e).or_default().push(l);
        }
    }
    let accepted_once = |e: &[u8; 16]| by_id.get(e).map_or(0, |ls| ls.iter().filter(|l| l.accept).count());
    // The entry ids on each other branch: a rollback's lost tail, a fork's
    // copy, the old relay after a move.
    let branch_ids: Vec<BTreeSet<[u8; 16]>> =
        f.branches.iter().map(|b| ledger(&b.rows, f.template, &f.roots).into_iter().filter_map(|l| l.entry_id).collect()).collect();
    let mut seen_ack = BTreeSet::new();
    for (eid, dev, line, _) in f.acked {
        if !seen_ack.insert(*eid) {
            continue;
        }
        // Strict reading (§31.7 has no exemption): whatever relay state
        // acknowledged it and whatever became of the device since.
        let n = accepted_once(eid);
        if n == 1 {
            continue;
        }
        let classes: Vec<&str> = by_id.get(eid).map(|ls| ls.iter().map(|l| l.class).collect()).unwrap_or_default();
        // Ruling 1 of the D2 brief: an entry sequenced only in a relay state
        // that a rollback or a fork removed from the reference branch is named
        // as such, with the fault's trace line.
        let lost = (n == 0)
            .then(|| f.branches.iter().zip(&branch_ids).find(|(b, ids)| ids.contains(eid) && (b.why == WHY_ROLLBACK || b.why == WHY_FORK)))
            .flatten()
            .map(|(b, _)| format!(": lost to a rolled-back relay state ({}, fault at trace line {})", b.why, b.line))
            .unwrap_or_default();
        v.push((2, *line, format!("dev {dev}: acknowledged entry {} is accepted {n} times in the reference log (classes there: {classes:?}){lost}", hexs(eid))));
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

    // 3. Queued edits. The three outcomes of §31.7, read strictly; and a
    // record dropped as a cancelled hold (ruling 5 of the D2 brief:
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
        let revoked = x.revoked || revoked_in_ref.contains(&x.dev);
        let held = n == 0 && in_outbox && (x.halted_or_stopped_persisted || revoked);
        let refused = n == 0
            && by_id.get(eid).is_some_and(|ls| ls.iter().any(|l| !l.accept && x.refusal_seqs.contains(&l.seq)));
        if !(held || refused) {
            v.push((3, *line, format!("dev {dev}: enqueued entry {} is accepted {n} times and not held or refused (in its outbox: {in_outbox})", hexs(eid))));
        }
    }

    // 4. Tampering: the confirmed state at every B against the oracle's,
    // skipping none (ruling 6 of the D2 brief): the oracle replays the
    // reference log with the same Skips, so its state at a B between a Skip's
    // target and the Skip is the one a device that confirmed that B had.
    // Named in the report, so that a failure there can be told apart.
    let skip_window = |b: u64| led.iter().find(|l| l.skip_targets.iter().any(|t| *t <= b && b < l.seq)).map(|l| l.seq);
    let mut branch_states: BTreeMap<usize, OracleStates> = BTreeMap::new();
    for (i, x) in f.idents.iter().enumerate() {
        for (b, c, bh, line) in &x.samples {
            if *b <= x.joined_at {
                continue;
            }
            let on_ref = ref_chain.get(b) == Some(c);
            let st = if on_ref {
                states.get(b)
            } else {
                let Some(k) = f.branches.iter().position(|br| br.rows.iter().any(|r| r.seq == *b && r.chain == *c)) else {
                    // On no branch at all: the device's chain at B was served by no relay.
                    v.push((4, *line, format!("dev {i}: confirmed state at {b} on a chain no relay served")));
                    continue;
                };
                let s = branch_states
                    .entry(k)
                    .or_insert_with(|| oracle(&f.branches[k].rows, f.template, &f.roots, &f.board, &f.relay));
                s.get(b)
            };
            if let Some((oc, obh, _)) = st
                && oc == c
                && obh != bh
            {
                let on = if on_ref {
                    "the reference log"
                } else {
                    f.branches.iter().find(|br| br.rows.iter().any(|r| r.seq == *b && r.chain == *c)).map_or("", |br| br.why)
                };
                let window = skip_window(*b).map(|s| format!(" (between a Skip's target and the Skip at {s})")).unwrap_or_default();
                v.push((4, *line, format!("dev {i}: confirmed state at {b} differs from the oracle's on {on}{window}")));
            }
        }
        if let Some((line, what)) = &x.expect_alarm {
            v.push((4, *line, format!("dev {i}: {what}, and no alarm was raised")));
        }
    }
    for (s, chain, line) in f.injected {
        // Only a frame the reference log still holds (a rollback may have removed it).
        if ref_chain.get(s) != Some(chain) {
            continue;
        }
        for (i, x) in f.idents.iter().enumerate() {
            // A device that confirmed a seq at or past the frame on the
            // reference branch has received it (one on another branch there
            // never saw it).
            let passed = x.samples.iter().any(|(b, c, _, _)| b >= s && ref_chain.get(b) == Some(c));
            if x.joined_at >= *s || !passed || x.retired {
                continue;
            }
            if !x.alarms.iter().any(|a| a.1 == AlarmKind::SkippedFrame && a.2 == Some(*s)) {
                v.push((4, *line, format!("dev {i}: the injected frame at {s} raised no SkippedFrame")));
            }
        }
    }

    // 5. No deletion by the relay.
    let purges: Vec<(u64, u64, u32, i64)> =
        led.iter().filter(|l| l.accept).filter_map(|l| Some((l.seq, l.purge_before?, l.new_epoch?, l.tau))).collect();
    let own_entry_at: BTreeMap<[u8; 16], u64> = led.iter().filter_map(|l| Some((l.entry_id?, l.seq))).rev().collect();
    for (dev, d, b, line) in f.deletions {
        use super::world::Deletion;
        let ok = match d {
            Deletion::Truncate(p, ctx) => *ctx || purges.iter().any(|(r, pb, _, _)| pb == p && r <= b) || (*p + 16_384 <= *b + 1),
            Deletion::Bases(seqs, ctx) => *ctx || purges.iter().any(|(r, _, _, _)| *r <= *b && seqs.iter().all(|s| s < r)),
            Deletion::Roots(es) => purges.iter().any(|(r, _, n, tau)| {
                *r <= *b && es.iter().all(|e| e < n) && led.iter().any(|l| l.seq <= *b && l.tau > tau + 2_592_000_000)
            }),
            // An identity change dropping the pending roots of own rotations:
            // each one's own entry is in the reference log at or below B, either
            // accepted (it reached the board head) or refused (a refused own
            // rotation, SEAL-MEM-016). Nothing else excuses the drop of one.
            Deletion::Pending(ids) => ids.iter().all(|e| own_entry_at.get(e).is_some_and(|s| s <= b)),
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
            v.push((6, x.last_line, format!("dev {i}: neither live and caught up nor halted or stopped with a persisted reason ({})", x.desc)));
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
