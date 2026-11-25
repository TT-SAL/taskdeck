//! Flows over the tests' hub with the fake sealer and the toy: a crash halt with
//! read-ahead and a Skip, a Skip of an applied entry with its replay, checkpoints,
//! bases and shutdown.

use super::kit_tests::{Hub, Node, creator, join, relay};
use crate::toy::{ToyCommand, ToyMachine};
use crate::{
    Body, DeviceStatus, Effect, Event, Kind, NewDevice, Notification, Operation, OutboxOp, OutboxRecord, PersistOp,
    Sealer, SessionState, StateMachine, UserAction, Via,
};

/// A poison entry: the device seals the toy's `Panic` without the trial apply of
/// §9.1 (a build with a fault would; an honest engine refuses it at `Enqueue`).
pub(crate) fn poison(n: &mut Node) {
    let payload = ToyMachine::new(false).encode(&ToyCommand::Panic);
    n.e.enqueue_own(Kind::Cmd, Body::Cmd(payload), Via::Append, None).expect("an entry id");
    let out = std::mem::take(&mut n.e.out);
    n.absorb(out);
    n.feed(Event::Tick);
}

pub(crate) fn op_result(n: &Node, t: u64) -> Option<Result<(), &'static str>> {
    n.notes.iter().find_map(|x| match x {
        Notification::OpDone { ticket, result } if *ticket == t => Some(*result),
        _ => None,
    })
}

// spec: ENG-HALT-003 ENG-HALT-004 ENG-HALT-006 ENG-HALT-007 ENG-CTL-001 ENG-ENQ-001
#[test]
fn a_crash_halt_reads_ahead_and_a_skip_clears_it() {
    let (mut a, f1) = creator(31);
    let mut hub = Hub::with_genesis(&a, &f1);
    let mut b = join(&mut hub, &mut a, 32, "Laptop");
    let mut c = join(&mut hub, &mut a, 33, "Phone");
    a.set(1, 1, 10);
    hub.settle(&mut [&mut a, &mut b, &mut c]);
    let before = a.board_bytes();
    let base = hub.log.len() as u64;

    // An honest enqueue of the poison is refused by the trial apply.
    c.enqueue(70, ToyCommand::Panic);
    assert!(c.has_note(|x| matches!(x, Notification::EnqueueRefused { ticket: 70, code: "engine/trial_panic" })));

    // C's poison lands at `p`; B's entry, sealed before B read the poison, lands behind it.
    poison(&mut c);
    b.set(2, 2, 20);
    hub.settle(&mut [&mut c, &mut b, &mut a]);
    let p = base + 1;
    assert_eq!(hub.log.len() as u64, p + 1);
    for n in [&a, &b, &c] {
        let st = n.e.status();
        let h = st.halt.expect("a crash halt");
        assert_eq!((h.seq, h.author), (p, c.dev()));
        // Read ahead: the log head moved on, the board did not.
        assert_eq!(st.head.seq, p + 1);
        assert_eq!(st.board_seq, p - 1);
        assert_eq!(n.board_bytes(), before);
    }

    // A halted device seals nothing and takes no duty: an enqueue waits, a day passes, no beat.
    a.set(3, 3, 30);
    a.advance(90_000_000);
    b.advance(90_000_000);
    hub.settle(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(hub.log.len() as u64, p + 1);
    assert_eq!(a.e.status().queued, 1);
    let device = NewDevice { dev: [9; 16], sign_pk: [1; 32], hpke_pk: [2; 32], name: "Tablet".into(), transcript: [3; 32] };
    a.feed(Event::PairRegister { ticket: 71, device });
    assert!(a.has_note(|x| matches!(x, Notification::PairFailed { ticket: 71, code: "engine/halted" })));

    // The author cannot skip its own entry; another device can.
    c.feed(Event::Op { ticket: 72, op: Operation::Skip { seq: p } });
    assert_eq!(op_result(&c, 72), Some(Err("engine/skip_target")));
    a.feed(Event::Op { ticket: 73, op: Operation::Skip { seq: p } });
    hub.settle(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(op_result(&a, 73), Some(Ok(())));
    // The Skip is a control entry committed while the board was halted; its effect
    // cleared the halt everywhere, and the entries behind the poison were applied in order.
    for n in [&a, &b, &c] {
        let st = n.e.status();
        assert!(st.halt.is_none());
        assert_eq!(st.board_seq, hub.log.len() as u64);
        assert_eq!(st.queued, 0);
        assert_eq!(n.board_bytes(), a.board_bytes());
    }
    let keys = &a.e.confirmed().0.keys;
    assert_eq!((keys[&1].0, keys[&2].0, keys[&3].0), (10, 20, 30));
}

// spec: ENG-SKIP-002 ENG-REF-004 ENG-REF-001 ENG-REF-003 ENG-CKPT-001
#[test]
fn a_skip_of_an_applied_entry_replays_without_it() {
    let (mut a, f1) = creator(41);
    let mut hub = Hub::with_genesis(&a, &f1);
    let mut b = join(&mut hub, &mut a, 42, "Laptop");
    let mut c = join(&mut hub, &mut a, 43, "Phone");
    hub.settle(&mut [&mut a, &mut b, &mut c]);
    // The target: B sets key 1. C sees nothing from here on until the end.
    b.set(1, 1, 5);
    hub.settle(&mut [&mut a, &mut b]);
    let t = hub.log.len() as u64;
    // Two sets of key 9 made without knowing of each other: a notice at the later one.
    a.set(2, 9, 100);
    b.set(3, 9, 200);
    hub.settle(&mut [&mut a, &mut b]);
    let notice = a.e.notices().into_iter().find(|n| n.seq > t).expect("a notice");
    a.feed(Event::User(UserAction::DismissNotice { seq: notice.seq, index: notice.index }));
    // A's own Add of key 1 stands on the target.
    a.enqueue(4, ToyCommand::Add { key: 1, delta: 1 });
    hub.settle(&mut [&mut a, &mut b]);
    let add_seq = hub.log.len() as u64;
    assert_eq!(a.e.confirmed().0.keys[&1].0, 6);
    assert!(a.e.notices().iter().all(|n| n.seq != add_seq));
    let writes = a.persists.len();

    a.feed(Event::Op { ticket: 5, op: Operation::Skip { seq: t } });
    hub.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 5), Some(Ok(())));
    // Without the target, key 1 never existed and the Add is refused.
    for n in [&a, &b] {
        assert!(!n.e.confirmed().0.keys.contains_key(&1));
        assert_eq!(n.e.confirmed().0.keys[&9].0, a.e.confirmed().0.keys[&9].0);
    }
    assert_eq!(a.board_bytes(), b.board_bytes());
    // One RewriteNotices replaced the records at and above the target: the notice is
    // there again with equal bytes and still dismissed; the Add now has a refusal record.
    let rewrites: Vec<_> = a.persists[writes..].iter().flatten().filter(|op| matches!(op, PersistOp::RewriteNotices(..))).collect();
    assert_eq!(rewrites.len(), 1);
    // §18.1 case 4: a checkpoint after the replay.
    assert!(!checkpoints(&a, writes).is_empty());
    let after = a.e.notices();
    let again = after.iter().find(|n| n.seq == notice.seq && n.index == notice.index).expect("the notice again");
    assert_eq!(again.record, notice.record);
    assert!(again.dismissed);
    let refusal = after.iter().find(|n| n.seq == add_seq).expect("a refusal record for the Add");
    assert!(!refusal.dismissed);
    // The same on B, where the notice was never dismissed.
    assert!(b.e.notices().iter().any(|n| n.seq == notice.seq && !n.dismissed));

    // A device that never applied the target ends in the same state.
    hub.settle(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(c.board_bytes(), a.board_bytes());
    assert_eq!(c.e.status().fingerprint, a.e.status().fingerprint);
    // C wrote neither entry: no record concerns it (ENG-REF-001, ENG-REF-003). B has a
    // refusal record for its skipped entry and none for A's Add.
    assert!(c.e.notices().is_empty());
    assert!(b.e.notices().iter().any(|n| n.seq == t) && b.e.notices().iter().all(|n| n.seq != add_seq));
}

fn checkpoints(n: &Node, from: usize) -> Vec<u64> {
    n.persists[from..]
        .iter()
        .flatten()
        .filter_map(|op| match op {
            PersistOp::WriteCheckpoint(c) => Some(c.board_seq),
            _ => None,
        })
        .collect()
}

fn ready(n: &Node) -> bool {
    n.has_note(|x| matches!(x, Notification::ReadyToExit))
}

// spec: ENG-CKPT-001 ENG-CKPT-002
#[test]
fn checkpoints_follow_the_cadence_and_shutdown_waits_for_them() {
    let (mut a, f1) = creator(51);
    let mut hub = Hub::with_genesis(&a, &f1);
    hub.settle(&mut [&mut a]);
    a.tick(600_000);
    hub.settle(&mut [&mut a]);
    let last = *checkpoints(&a, 0).last().expect("a checkpoint after the bootstrap");
    assert_eq!(last, a.e.status().board_seq);

    // Case 1: 200 entries applied since the last checkpoint, long before 600,000 ms pass.
    // The frames go out at once; the next poll, thirty seconds later, reads them back.
    let from = a.persists.len();
    let round = |a: &mut Node, hub: &mut Hub, keys: std::ops::Range<u64>| {
        for k in keys {
            a.set(100 + k, (k % 200) as u8, k as i64);
        }
        hub.serve(a);
        a.tick(30_000);
        hub.serve(a);
    };
    round(&mut a, &mut hub, 0..150);
    assert_eq!(a.e.status().board_seq, last + 150);
    assert!(checkpoints(&a, from).is_empty());
    round(&mut a, &mut hub, 150..210);
    assert_eq!(checkpoints(&a, from), vec![last + 210]);

    // Case 2: 600,000 ms since the last checkpoint and B has changed; not when B has not.
    let from = a.persists.len();
    round(&mut a, &mut hub, 210..211);
    a.tick(569_000);
    assert!(checkpoints(&a, from).is_empty());
    a.tick(1_000);
    assert_eq!(checkpoints(&a, from), vec![last + 211]);
    a.tick(600_000);
    a.tick(600_000);
    assert_eq!(checkpoints(&a, from).len(), 1);

    // Case 3: Shutdown. ReadyToExit comes only when the checkpoint, the outbox and the meta are durable.
    hub.settle(&mut [&mut a]);
    a.set(900, 1, 1);
    hub.settle(&mut [&mut a]);
    let b = a.e.status().board_seq;
    a.auto = false;
    a.set(901, 2, 2);
    let from = a.persists.len();
    a.feed(Event::Shutdown);
    assert!(checkpoints(&a, from).contains(&b));
    assert!(a.pq.len() >= 2);
    while !a.pq.is_empty() {
        assert!(!ready(&a));
        a.persist_one();
    }
    assert!(ready(&a));
    // What a restart loads is what was running.
    a.auto = true;
    let board = a.board_bytes();
    a.restart(52);
    assert_eq!(a.board_bytes(), board);
    assert_eq!(a.e.status().queued, 1);
}

// spec: ENG-HASH-002 ENG-CKPT-003 ENG-BEAT-002
#[test]
fn hashes_at_every_aligned_seq_and_bases_with_the_state_at_theirs() {
    let (mut a, f1) = creator(61);
    let mut hub = Hub::with_genesis(&a, &f1);
    hub.settle(&mut [&mut a]);
    let sm = ToyMachine::new(false);
    // What the board and the membership were when B was exactly an aligned seq.
    let mut at: std::collections::BTreeMap<u64, (Vec<u8>, Vec<u8>, crate::Hash32)> = Default::default();
    let mut k = 0u64;
    let mut grow = |a: &mut Node, hub: &mut Hub, to: u64, at: &mut std::collections::BTreeMap<u64, (Vec<u8>, Vec<u8>, crate::Hash32)>| {
        while (hub.log.len() as u64) < to {
            k += 1;
            a.set(k, (k % 50) as u8, k as i64);
            hub.settle(&mut [&mut *a]);
            let seq = hub.log.len() as u64;
            assert_eq!(a.e.status().board_seq, seq);
            if seq.is_multiple_of(64) {
                let (state, _) = a.e.confirmed();
                at.insert(seq, (sm.canonical_bytes(state), a.e.membership_bytes(), sm.board_hash(state)));
            }
        }
    };
    grow(&mut a, &mut hub, 150, &mut at);
    // Eight days of log clock pass, well away from an aligned seq.
    a.wall += 8 * 86_400_000;
    a.advance(8 * 86_400_000);
    hub.settle(&mut [&mut a]);
    grow(&mut a, &mut hub, 200, &mut at);
    assert!(at.contains_key(&64) && at.contains_key(&128) && at.contains_key(&192));

    let hashes: Vec<_> = a
        .persists
        .iter()
        .flatten()
        .filter_map(|op| match op {
            PersistOp::AppendHashes(v) => Some(v.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(hashes.iter().map(|h| h.seq).collect::<Vec<_>>(), vec![64, 128, 192]);
    for h in &hashes {
        let (_, membership, board_hash) = &at[&h.seq];
        assert_eq!(h.hash_version, 1);
        assert_eq!(h.hash, crate::state_hash(1, board_hash, membership));
    }

    let bases: Vec<_> = a
        .persists
        .iter()
        .flatten()
        .filter_map(|op| match op {
            PersistOp::WriteBase(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    // Every base is at an aligned seq (or the genesis bootstrap's) with the state exactly there.
    for c in &bases {
        if c.board_seq > 1 {
            let (board, membership, _) = &at[&c.board_seq];
            assert_eq!((&c.board, &c.membership), (board, membership));
        }
    }
    let seqs: Vec<u64> = bases.iter().map(|c| c.board_seq).collect();
    // Condition 1 (no base yet) gave one at the first aligned seq unless the bootstrap wrote
    // one; condition 2 (seven days of log clock) gave the one at 192.
    assert!(seqs.contains(&64) || seqs.contains(&1), "{seqs:?}");
    assert!(seqs.contains(&192), "{seqs:?}");

    // §23.2: what each beat carried. The first one had no hash record to report; the one
    // after the eight days reports the record at the largest aligned seq at or below its head.
    let beats = puts(&a, 0).into_iter().filter(|r| r.kind == Kind::Beat).collect::<Vec<_>>();
    assert!(beats.len() >= 2);
    for r in &beats {
        let Body::Beat(info) = &r.body else { panic!("a beat's body") };
        assert_eq!(info.rules, sm.supported_rules());
        assert_eq!(info.envelopes, a.e.sealer().supported_envelopes());
        assert_eq!(info.hash_versions, sm.hash_versions());
        assert!(info.acks.is_empty());
        let aligned = r.based_on.seq / 64 * 64;
        match info.state {
            None => assert!(r.based_on.seq < 64),
            Some((seq, hash, version)) => {
                assert_eq!((seq, version), (aligned, 1));
                assert_eq!(hash, hashes.iter().find(|h| h.seq == seq).expect("the record").hash);
            }
        }
    }
    assert!(beats[0].based_on.seq < 64);
    assert!(beats.iter().any(|r| r.based_on.seq >= 128));
}

/// Every record put into the outbox from write `from` on, in order.
pub(crate) fn puts(n: &Node, from: usize) -> Vec<OutboxRecord> {
    let mut out = Vec::new();
    for op in n.persists[from..].iter().flatten() {
        if let PersistOp::Outbox(ops) = op {
            for o in ops {
                if let OutboxOp::Put(r) = o {
                    out.push(r.clone());
                }
            }
        }
    }
    out
}

// spec: ENG-UI-001 ENG-BANNER-002
#[test]
fn the_status_follows_the_state() {
    let (mut a, f1) = creator(71);
    let mut hub = Hub::with_genesis(&a, &f1);
    let mut b = join(&mut hub, &mut a, 72, "Laptop");
    b.set(1, 5, 50);
    hub.settle(&mut [&mut a, &mut b]);
    let st = a.e.status();
    assert_eq!(st.session, SessionState::Live);
    assert_eq!((st.relay_host.as_str(), st.push), (relay().host.as_str(), false));
    assert!(st.last_synced_ms.is_some_and(|t| t <= a.wall && t > a.wall - 120_000));
    assert_eq!((st.unreachable_since_ms, st.paused_until_ms), (None, None));
    assert_eq!((st.queued, st.oldest_queued_ms, st.doomed, st.queue_large), (0, None, 0, false));
    assert_eq!(st.head, hub.head(a.e.sealer()));
    assert_eq!(st.board_seq, hub.log.len() as u64);
    assert_eq!(st.fingerprint, a.e.sealer().sync_fingerprint(&st.head));
    assert_eq!(st.fingerprint, b.e.status().fingerprint);
    assert!(st.halt.is_none() && st.stops.is_empty() && st.alarms.is_empty() && st.moving.is_none() && st.ops.is_empty());
    assert_eq!((st.notices_unread, st.refusals_unread), (0, 0));
    assert!(st.warnings.is_empty());
    assert_eq!(st.rules.active, 1);
    let mut rows: Vec<_> = st.devices.iter().map(|d| (d.device_no, d.name.as_str(), d.status, d.is_self)).collect();
    rows.sort_by_key(|r| r.0);
    assert_eq!(rows, vec![(1, "Desk", DeviceStatus::Current, true), (2, "Laptop", DeviceStatus::Current, false)]);
    let row = st.devices.iter().find(|d| d.device_no == 2).expect("B's row");
    assert_eq!(row.dev, b.dev());
    assert!(row.bootstrap_seq > 0 && row.last_entry_ms.is_some());
    assert_eq!(row.rules, vec![1]);

    // A banner stays until it is acknowledged on this device, across a restart.
    let banner = b.e.status().banners.first().cloned().expect("the banner about the device's own addition");
    b.restart(73);
    assert_eq!(b.e.status().banners, vec![banner.clone()]);
    b.feed(Event::User(UserAction::AckBanner { id: banner.id }));
    assert!(b.e.status().banners.is_empty());
    b.restart(74);
    assert!(b.e.status().banners.is_empty());
    assert!(a.e.status().banners.is_empty());
    hub.settle(&mut [&mut a, &mut b]);

    // Unreachable: the time of the first such disconnect since the last Live.
    a.feed(Event::Disconnected { unreachable: true, paused_until_ms: None });
    let since = a.wall;
    a.advance(5_000);
    a.feed(Event::Disconnected { unreachable: true, paused_until_ms: None });
    let st = a.e.status();
    assert_ne!(st.session, SessionState::Live);
    assert_eq!(st.unreachable_since_ms, Some(since));
    // Queued while away: an Add that stands on B's key 5.
    a.enqueue(2, ToyCommand::Add { key: 5, delta: 1 });
    let st = a.e.status();
    assert_eq!((st.queued, st.oldest_queued_ms, st.doomed), (1, Some(a.wall), 0));
    assert_eq!(a.e.pending().len(), 1);
    // Meanwhile B deletes key 5. A comes back and reads it while its own frame is still on its way.
    b.enqueue(3, ToyCommand::Del { key: 5 });
    hub.settle(&mut [&mut b]);
    a.tick(60_000);
    let mut held = Vec::new();
    for _ in 0..200 {
        let Some(fx) = a.fx.pop_front() else { break };
        match fx {
            Effect::Connect { .. } => a.feed(Event::Connected { push: false }),
            Effect::Hello => {
                let w = hub.welcome(&a);
                a.feed(Event::Welcome(w));
            }
            Effect::Read { req, after, limit } => {
                let page = hub.page(&a, after, limit);
                a.feed(Event::Page { req: Some(req), page });
            }
            Effect::Append { req, frames } => held.push((req, frames)),
            _ => {}
        }
    }
    let st = a.e.status();
    assert_eq!(st.session, SessionState::Live);
    assert_eq!(st.unreachable_since_ms, None);
    assert_eq!((st.queued, st.doomed), (1, 1));
    assert_eq!(a.e.pending()[0].doomed, Some("engine/toy_gone"));
    assert_eq!(st.board_seq, hub.log.len() as u64);
    // The frame lands and is refused at the board head: a refusal record, nothing queued.
    assert_eq!(held.len(), 1);
    let (req, frames) = held.remove(0);
    let results = hub.append(a.e.sealer(), frames);
    a.feed(Event::AppendResults { req, results });
    hub.settle(&mut [&mut a, &mut b]);
    let st = a.e.status();
    assert_eq!((st.queued, st.doomed, st.refusals_unread, st.notices_unread), (0, 0, 1, 0));
    assert_eq!(b.e.status().refusals_unread, 0);
}

// spec: ENG-OPS-007
#[test]
fn a_host_that_is_not_one_is_refused_before_anything_happens() {
    let (mut a, f1) = creator(81);
    let mut hub = Hub::with_genesis(&a, &f1);
    hub.settle(&mut [&mut a]);
    let writes = a.persists.len();
    let bad = || "relay example".to_owned();
    a.feed(Event::Op { ticket: 1, op: Operation::Move { host: bad() } });
    a.feed(Event::Op { ticket: 2, op: Operation::FollowAddress { host: bad(), door: [1; 5], code: "X".into() } });
    a.feed(Event::Op { ticket: 3, op: Operation::CloseBoard { host: bad(), door: [1; 5] } });
    for t in 1..=3 {
        assert_eq!(op_result(&a, t), Some(Err("engine/bad_host")));
    }
    assert!(a.fx.is_empty());
    assert_eq!(a.persists.len(), writes);
    assert!(a.e.status().ops.is_empty());
}

// spec: ENG-OPS-011
#[test]
fn no_rotation_on_a_timer() {
    let (mut a, f1) = creator(91);
    let mut hub = Hub::with_genesis(&a, &f1);
    let mut b = join(&mut hub, &mut a, 92, "Laptop");
    b.set(1, 1, 1);
    hub.settle(&mut [&mut a, &mut b]);
    let (wa, wb) = (a.persists.len(), b.persists.len());
    // Four hundred days of beats and duties: no control entry is composed by anyone.
    for _ in 0..400 {
        a.advance(86_400_000);
        b.advance(86_400_000);
        hub.settle(&mut [&mut a, &mut b]);
    }
    let composed: Vec<Kind> = puts(&a, wa).into_iter().chain(puts(&b, wb)).map(|r| r.kind).collect();
    assert!(composed.contains(&Kind::Beat));
    assert!(composed.iter().all(|k| *k != Kind::Ctl));

}

// spec: ENG-SKIP-004
#[test]
fn a_skip_target_outside_the_window_is_refused() {
    let (mut a, f1) = creator(93);
    let mut hub = Hub::with_genesis(&a, &f1);
    let mut b = join(&mut hub, &mut a, 94, "Laptop");
    b.set(1, 1, 1);
    hub.settle(&mut [&mut a, &mut b]);
    let old = hub.log.len() as u64;
    // 16,384 entries later the first target is outside the window by one.
    let mut k = 0u64;
    while (hub.log.len() as u64) < old + 16_383 {
        let n = (old + 16_383 - hub.log.len() as u64).min(1_000);
        for _ in 0..n {
            k += 1;
            a.set(10 + k, (2 + k % 100) as u8, k as i64);
        }
        hub.settle(&mut [&mut a, &mut b]);
    }
    b.set(2, 200, 2);
    hub.settle(&mut [&mut a, &mut b]);
    let l = a.e.status().head.seq;
    let recent = l;
    assert!(old < l + 1 - 16_384 && old + 1 >= l + 1 - 16_384, "old {old} l {l}");
    a.feed(Event::Op { ticket: 5, op: Operation::Skip { seq: old } });
    assert_eq!(op_result(&a, 5), Some(Err("engine/skip_target")));
    a.feed(Event::Op { ticket: 6, op: Operation::Skip { seq: recent } });
    hub.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 6), Some(Ok(())));
    assert!(a.e.confirmed().0.keys.contains_key(&1) && !a.e.confirmed().0.keys.contains_key(&200));
}

// spec: ENG-EPOCH-003
#[test]
fn imported_commands_are_rebound_and_resubmitted_in_order() {
    let (mut a, f1) = creator(95);
    let mut hub = Hub::with_genesis(&a, &f1);
    let mut b = join(&mut hub, &mut a, 96, "Laptop");
    a.set(1, 7, 70);
    hub.settle(&mut [&mut a, &mut b]);
    let old = |n: u64| (5u64 << 40) | n;
    let commands = vec![
        ToyCommand::Set { key: 1, value: 11 },
        ToyCommand::Create { id: old(3), name: 30 },
        ToyCommand::Fail { code: 1 },
        ToyCommand::Name { id: old(3), name: 31 },
        ToyCommand::Add { key: 99, delta: 1 },
        ToyCommand::Add { key: 7, delta: 1 },
    ];
    b.auto = false;
    let from = b.persists.len();
    b.advance(1_000);
    let head = b.e.status().head;
    b.feed(Event::Resubmit { ticket: 40, commands, from_device_no: 5, origin_state: 1u64.to_be_bytes().to_vec() });
    b.auto = true;
    b.persist_all();
    let report = b.notes.iter().find_map(|x| match x {
        Notification::ResubmitReport { ticket: 40, queued, refused } => Some((*queued, refused.clone())),
        _ => None,
    });
    assert_eq!(report, Some((4, vec![(2, "engine/toy_fail"), (4, "engine/toy_gone")])));
    // Each record as it was first put (it is put again when it is sealed).
    let mut recs: Vec<OutboxRecord> = Vec::new();
    for r in puts(&b, from) {
        if r.kind == Kind::Cmd && recs.iter().all(|x| x.entry_id != r.entry_id) {
            recs.push(r);
        }
    }
    let sm = ToyMachine::new(false);
    let cmds: Vec<ToyCommand> = recs
        .iter()
        .map(|r| match &r.body {
            Body::Cmd(p) => sm.parse(p).expect("a command"),
            _ => panic!("a command record"),
        })
        .collect();
    // The id minted by device 5 is minted anew by device 2, and the later reference follows it.
    let new_id = (2u64 << 40) | 1;
    assert_eq!(
        cmds,
        vec![
            ToyCommand::Set { key: 1, value: 11 },
            ToyCommand::Create { id: new_id, name: 30 },
            ToyCommand::Name { id: new_id, name: 31 },
            ToyCommand::Add { key: 7, delta: 1 },
        ]
    );
    assert!(recs.windows(2).all(|w| w[0].order < w[1].order));
    assert!(recs.iter().all(|r| r.at_ms == b.wall && r.based_on == head));
    hub.settle(&mut [&mut a, &mut b]);
    assert_eq!(a.e.confirmed().0.keys[&7].0, 71);
    assert_eq!(a.e.confirmed().0.items[&new_id], 31);
}

// spec: ENG-DUTY-003 ENG-DUTY-002
#[test]
fn a_duty_that_waits_only_for_a_fresh_head_asks_for_one() {
    let hellos = |n: &Node| n.fx.iter().filter(|f| matches!(f, Effect::Hello)).count();
    let appends = |n: &Node| n.fx.iter().filter(|f| matches!(f, Effect::Append { .. })).count();
    let (mut a, f1) = creator(201);
    let mut hub = Hub::with_genesis(&a, &f1);
    // A push session: the relay says what arrives, the engine does not poll.
    hub.push = true;
    hub.serve(&mut a);
    for _ in 0..4 {
        a.tick(30_000);
        hub.serve(&mut a);
    }
    // The first beat went out; its echo arrives as a push.
    let page = hub.page(&a, a.e.status().head.seq, 256);
    a.feed(Event::Page { req: None, page });
    assert_eq!(a.e.status().board_seq, hub.log.len() as u64);
    a.fx.clear();
    // Twenty minutes of quiet. No duty is due, so nothing asks every 30,000 ms; the engine still
    // asks for the head once per 600,000 ms: a push that never arrived cannot leave it behind.
    for _ in 0..40 {
        a.tick(30_000);
    }
    assert_eq!((hellos(&a), appends(&a)), (2, 0));
    a.fx.clear();
    // A day later the beat is due. The device is eligible but for the age of its head: it asks.
    a.advance(86_400_000);
    a.feed(Event::Tick);
    assert_eq!((hellos(&a), appends(&a)), (1, 0));
    // Not again within 30,000 ms; once more after them.
    a.tick(10_000);
    a.tick(19_000);
    assert_eq!(hellos(&a), 1);
    a.tick(1_000);
    assert_eq!((hellos(&a), appends(&a)), (2, 0));
    // The welcome gives it a head to be caught up with: the beat is taken.
    let w = hub.welcome(&a);
    a.feed(Event::Welcome(w));
    assert_eq!(appends(&a), 1);
}

// spec: RELAY-STORE-006
#[test]
fn the_relays_times_decide_nothing() {
    // The same run twice: once with the relay's times as they are, once with every `at` and
    // every `relay_now` ten years off.
    let run = |skew: i64| {
        let (mut a, f1) = creator(301);
        let mut hub = Hub::with_genesis(&a, &f1);
        for round in 0..40u64 {
            if round % 4 == 0 {
                a.set(round, (round % 7) as u8, round as i64);
            }
            if round == 20 {
                a.advance(86_400_000);
            }
            for _ in 0..200 {
                let Some(fx) = a.fx.pop_front() else { break };
                match fx {
                    Effect::Connect { .. } => a.feed(Event::Connected { push: false }),
                    Effect::Hello => {
                        let mut w = hub.welcome(&a);
                        w.relay_now += skew;
                        a.feed(Event::Welcome(w));
                        // The guard of §27.2 is the one use of `relay_now`: an hour or more off blocks
                        // composing until the user confirms the clock.
                        if a.e.status().clock.blocked {
                            a.feed(Event::User(UserAction::ConfirmClock));
                        }
                    }
                    Effect::Read { req, after, limit } => {
                        let mut page = hub.page(&a, after, limit);
                        for e in &mut page.entries {
                            e.at += skew;
                        }
                        a.feed(Event::Page { req: Some(req), page });
                    }
                    Effect::Append { req, frames } => {
                        let results = hub.append(a.e.sealer(), frames);
                        a.feed(Event::AppendResults { req, results });
                    }
                    _ => {}
                }
            }
            a.tick(30_000);
        }
        let taus: Vec<(u64, i64)> = a.store.log.iter().map(|(s, r)| (*s, r.tau)).collect();
        let stamps: Vec<(Kind, i64, u64)> = puts(&a, 0).iter().map(|r| (r.kind, r.at_ms, r.based_on.seq)).collect();
        let clock = a.e.status().clock;
        let values: Vec<(u8, i64)> = a.e.confirmed().0.keys.iter().map(|(k, v)| (*k, v.0)).collect();
        (values, taus, stamps, hub.log.len(), clock)
    };
    let honest = run(0);
    let off = run(315_360_000_000);
    assert!(honest.3 > 10);
    // The same entries and the same board (the block of §27.2 moved one beat before the
    // confirmation, nothing else), and no stamp anywhere near the relay's time: every log
    // clock value and every `at_ms` is this device's own clock.
    assert_eq!((&honest.0, honest.3), (&off.0, off.3));
    let own_clock = |t: i64| (0..=1_700_000_000_000 + 2 * 86_400_000).contains(&t);
    assert!(off.1.iter().all(|x| own_clock(x.1)) && off.2.iter().all(|x| own_clock(x.1)));
    assert_eq!(honest.2.iter().filter(|x| x.0 == Kind::Cmd).collect::<Vec<_>>(), off.2.iter().filter(|x| x.0 == Kind::Cmd).collect::<Vec<_>>());
    // Only the clock guard of SYNC.md §4.5 looks at `relay_now`: the warning stays after the confirmation.
    assert!(!honest.4.warning);
    assert!(off.4.warning);
    assert_eq!(off.4.skew_ms.map(i64::abs), Some(315_360_000_000));
    assert!(!off.4.blocked);
}

fn upgrades(n: &Node, from: usize) -> Vec<(u32, crate::UpgradeMode)> {
    let mut seen: Vec<[u8; 16]> = Vec::new();
    let mut out = Vec::new();
    for r in puts(n, from) {
        if let Body::Ctl(crate::CtlIntent::RulesUpgrade { rules, mode }) = &r.body
            && !seen.contains(&r.entry_id)
        {
            seen.push(r.entry_id);
            out.push((*rules, *mode));
        }
    }
    out
}

// spec: ENG-RULES-001 ENG-RULES-002
#[test]
fn the_status_names_who_blocks_a_newer_rules_version() {
    let (mut a, f1) = creator(311);
    let mut hub = Hub::with_genesis(&a, &f1);
    let mut b = join(&mut hub, &mut a, 313, "Laptop");
    hub.settle(&mut [&mut a, &mut b]);
    // A gets a newer build; its next beat says so.
    a.restart_with_rules2(312);
    hub.settle(&mut [&mut a, &mut b]);
    // A's build supports rules 2 and B's does not: at A, B blocks version 2. B's build knows no newer version.
    let rules = a.e.status().rules;
    assert_eq!((rules.active, rules.versions.len()), (1, 1));
    assert_eq!(rules.versions[0].version, 2);
    assert_eq!(rules.versions[0].blockers.iter().map(|x| x.dev).collect::<Vec<_>>(), vec![b.dev()]);
    assert_eq!(rules.versions[0].blockers[0].rules, vec![1]);
    assert_eq!(rules.versions[0].override_vote, None);
    assert!(b.e.status().rules.versions.is_empty());
    // A month passes: while B blocks, nobody composes an upgrade, least of all an override.
    let (fa, fb) = (a.persists.len(), b.persists.len());
    for _ in 0..30 {
        a.advance(86_400_000);
        b.advance(86_400_000);
        hub.settle(&mut [&mut a, &mut b]);
    }
    assert!(upgrades(&a, fa).is_empty() && upgrades(&b, fb).is_empty());
    // Only the user's operation composes one.
    a.feed(Event::Op { ticket: 5, op: Operation::RulesOverride { version: 2 } });
    hub.settle(&mut [&mut a, &mut b]);
    assert_eq!(upgrades(&a, fa), vec![(2, crate::UpgradeMode::Override)]);
    assert!(upgrades(&b, fb).is_empty());
    assert_eq!(op_result(&a, 5), Some(Ok(())));
    let rules = a.e.status().rules;
    assert_eq!(rules.active, 1);
    assert!(matches!(rules.versions[0].override_vote, Some((2, dev, _)) if dev == a.dev()));
    assert_eq!(rules.versions[0].blockers.len(), 1);
    // B gets the newer build too: nobody blocks, and the board moves to rules 2.
    b.restart_with_rules2(314);
    for _ in 0..3 {
        hub.settle(&mut [&mut a, &mut b]);
        a.advance(86_400_000);
        b.advance(86_400_000);
    }
    hub.settle(&mut [&mut a, &mut b]);
    for n in [&a, &b] {
        let rules = n.e.status().rules;
        assert_eq!(rules.active, 2);
        assert!(rules.versions.is_empty());
    }
    let overrides = upgrades(&a, fa).into_iter().chain(upgrades(&b, fb)).filter(|u| u.1 == crate::UpgradeMode::Override).count();
    assert_eq!(overrides, 1);
    assert_eq!(a.board_bytes(), b.board_bytes());
}

// spec: ENG-HOLD-004 ENG-EFFECT-002
// Found by the simulator (seed 1 of `quiet`): a hold that had timed out while its record
// could not be sent kept asking for a tick one millisecond ahead, forever.
#[test]
fn a_hold_that_timed_out_arms_no_timer() {
    let (mut a, f1) = creator(401);
    let mut hub = Hub::with_genesis(&a, &f1);
    hub.settle(&mut [&mut a]);
    a.feed(Event::Disconnected { unreachable: true, paused_until_ms: None });
    a.enqueue(1, ToyCommand::Create { id: (1u64 << 40) | 1, name: 5 });
    assert_eq!(a.e.status().queued, 1);
    a.tick(31_000);
    a.fx.clear();
    for _ in 0..5 {
        a.tick(1);
    }
    let mono = a.mono;
    let busy = a.fx.iter().filter(|f| matches!(f, Effect::ScheduleTick { at_mono_ms } if *at_mono_ms <= mono + 1)).count();
    assert_eq!(busy, 0);
}

// spec: ENG-SKIP-003
#[test]
fn a_skip_replay_replaces_the_hashes_and_forgets_the_older_reports() {
    let (mut a, f1) = creator(431);
    let mut hub = Hub::with_genesis(&a, &f1);
    let mut b = join(&mut hub, &mut a, 432, "Laptop");
    b.set(1, 1, 5);
    hub.settle(&mut [&mut a, &mut b]);
    let target = hub.log.len() as u64;
    let mut k = 0u64;
    while (hub.log.len() as u64) < 70 {
        k += 1;
        a.set(10 + k, (2 + k % 40) as u8, k as i64);
        hub.settle(&mut [&mut a, &mut b]);
    }
    // Both report their hash at 64, which holds the target's effect.
    a.advance(86_400_000);
    b.advance(86_400_000);
    hub.settle(&mut [&mut a, &mut b]);
    let before = a.e.hashes[&64].hash;
    assert_eq!(before, b.e.hashes[&64].hash);
    assert!(a.e.registry.reports.iter().any(|r| r.seq == 64 && r.dev == b.dev() && r.hash == before));
    let from = a.persists.len();
    a.feed(Event::Op { ticket: 3, op: Operation::Skip { seq: target } });
    hub.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 3), Some(Ok(())));
    // The record at 64 is written again with the state that never held the target ...
    let after = a.e.hashes[&64].hash;
    assert_ne!(after, before);
    assert_eq!(after, b.e.hashes[&64].hash);
    let rewritten = a.persists[from..].iter().flatten().any(|op| matches!(op, PersistOp::AppendHashes(v) if v.iter().any(|h| h.seq == 64 && h.hash == after)));
    assert!(rewritten);
    // ... and B's report from before the Skip is no longer compared with it: no alarm, no stop.
    assert!(a.e.registry.reports.iter().all(|r| !(r.seq == 64 && r.hash == before)));
    for n in [&a, &b] {
        let st = n.e.status();
        assert!(st.alarms.is_empty() && st.stops.is_empty(), "{:?} {:?}", st.alarms.len(), st.stops);
    }
    // The next beats report the new hash, and still nothing is raised.
    a.advance(86_400_000);
    b.advance(86_400_000);
    hub.settle(&mut [&mut a, &mut b]);
    assert!(a.e.registry.reports.iter().any(|r| r.seq == 64 && r.dev == b.dev() && r.hash == after));
    assert!(a.e.status().alarms.is_empty() && b.e.status().alarms.is_empty());
}

// spec: ENG-SEALER-003
#[test]
fn a_skip_target_frame_is_supplied_only_when_its_hash_fits() {
    let (mut a, f1) = creator(441);
    let mut hub = Hub::with_genesis(&a, &f1);
    let mut b = join(&mut hub, &mut a, 442, "Laptop");
    a.set(1, 1, 5);
    hub.settle(&mut [&mut a, &mut b]);
    let target = hub.log.len() as u64;
    a.set(2, 2, 6);
    hub.settle(&mut [&mut a, &mut b]);
    // A's stored copy of the target is not the frame the log had (a damaged file).
    let other = a.store.log[&(target + 1)].frame.clone();
    a.store.log.get_mut(&target).expect("the record").frame = other;
    b.feed(Event::Op { ticket: 3, op: Operation::Skip { seq: target } });
    hub.settle(&mut [&mut b]);
    assert_eq!(op_result(&b, 3), Some(Ok(())));
    // A reads the Skip. Its own copy of the target does not hash to what the Skip names, so it
    // is not handed to the sealer: the Skip is not accepted on a frame that was not checked,
    // nothing is applied, and the engine stops with the fault of §17.2.
    let skip_seq = hub.log.len() as u64;
    for _ in 0..4 {
        a.tick(30_000);
        hub.serve(&mut a);
    }
    let st = a.e.status();
    assert!(st.stops.iter().any(|x| x.kind == crate::StopKind::LocalFault { code: "engine/skip_unreachable".into() }), "{:?}", st.stops);
    assert!(st.board_seq < skip_seq);
    assert_eq!(a.e.confirmed().0.keys[&1].0, 5);
    // B, whose copy is the frame the log had, accepted it.
    assert!(!b.e.confirmed().0.keys.contains_key(&1));
}

// spec: ENG-SKIP-003
#[test]
fn reports_from_before_a_skip_are_not_compared_by_a_device_that_never_applied_the_target() {
    let (mut a, f1) = creator(951);
    let mut hub = Hub::with_genesis(&a, &f1);
    let mut b = join(&mut hub, &mut a, 952, "Laptop");
    let mut c = join(&mut hub, &mut a, 953, "Phone");
    hub.settle(&mut [&mut a, &mut b, &mut c]);
    // C is away from here on. The target lies just below an aligned seq.
    let stride = super::lim::STRIDE;
    let mut k = 0u64;
    while (hub.log.len() as u64) < stride - 3 {
        k += 1;
        a.set(k, (k % 40) as u8, k as i64);
        hub.settle(&mut [&mut a, &mut b]);
    }
    b.set(900, 41, 5);
    hub.settle(&mut [&mut a, &mut b]);
    let t = hub.log.len() as u64;
    while (hub.log.len() as u64) < stride + 2 {
        k += 1;
        a.set(k, (k % 40) as u8, k as i64);
        hub.settle(&mut [&mut a, &mut b]);
    }
    assert!(t < stride);
    // A day passes: A and B report their state at the aligned seq, with the target applied.
    a.advance(90_000_000);
    b.advance(90_000_000);
    hub.settle(&mut [&mut a, &mut b]);
    assert!(a.e.registry.reports.iter().any(|r| r.seq == stride && r.dev == b.dev()));
    a.feed(Event::Op { ticket: 5, op: Operation::Skip { seq: t } });
    hub.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 5), Some(Ok(())));
    // C reads the target, the reports and the Skip together: it never applies the target.
    hub.settle(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(c.e.status().board_seq, hub.log.len() as u64);
    assert_eq!(c.board_bytes(), a.board_bytes());
    let kinds: Vec<_> = c.e.meta.alarms.iter().map(|x| x.kind).collect();
    assert!(kinds.is_empty(), "no alarm for a state from before the Skip: {kinds:?}");
    assert!(c.e.status().stops.is_empty());
}

// spec: ENG-SKIP-004
#[test]
fn a_second_skip_of_a_target_whose_skip_is_in_the_outbox_is_refused() {
    let (mut a, f1) = creator(961);
    let mut hub = Hub::with_genesis(&a, &f1);
    let mut b = join(&mut hub, &mut a, 962, "Laptop");
    b.set(1, 1, 5);
    hub.settle(&mut [&mut a, &mut b]);
    let t = hub.log.len() as u64;
    a.feed(Event::Op { ticket: 5, op: Operation::Skip { seq: t } });
    // The first Skip is composed and not yet in the log.
    a.feed(Event::Op { ticket: 6, op: Operation::Skip { seq: t } });
    assert_eq!(op_result(&a, 6), Some(Err("engine/skip_target")));
    hub.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 5), Some(Ok(())));
    assert_eq!(a.e.membership().skips.len(), 1);
}

// spec: ENG-SKIP-006
#[test]
fn a_skip_whose_target_another_device_skipped_first_is_done() {
    let (mut a, f1) = creator(981);
    let mut hub = Hub::with_genesis(&a, &f1);
    let mut b = join(&mut hub, &mut a, 982, "Laptop");
    let mut c = join(&mut hub, &mut a, 983, "Phone");
    c.set(1, 1, 5);
    hub.settle(&mut [&mut a, &mut b, &mut c]);
    let t = hub.log.len() as u64;
    // A and B each skip C's entry before either has seen the other's Skip.
    a.feed(Event::Op { ticket: 5, op: Operation::Skip { seq: t } });
    b.feed(Event::Op { ticket: 6, op: Operation::Skip { seq: t } });
    hub.settle(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(hub.log.len() as u64, t + 2, "both Skips are in the log");
    // The second is refused, and its operation is done all the same: the target is skipped.
    assert_eq!(op_result(&a, 5), Some(Ok(())));
    assert_eq!(op_result(&b, 6), Some(Ok(())));
    for n in [&a, &b, &c] {
        assert!(n.e.membership().skips.iter().any(|s| s.0 == t));
        assert_eq!(n.board_bytes(), a.board_bytes());
    }
}
