//! Operations against the model relay: pairing, revoking, purging, restoring to a
//! point, compaction, the move.

use super::flow_tests::{op_result, puts};
use super::kit_tests::Node;
use super::rkit_tests::{board, new_device, pair, pair_result};
use crate::{
    AdminRequest, BlobRequest, Body, CtlIntent, DeviceStatus, Effect, Event, Kind, Operation, PersistOp, RelayRefusal, Via,
};

// spec: ENG-OPS-001 ENG-OPS-008 ENG-BOOT-003
#[test]
fn a_pairing_uploads_the_snapshot_then_registers_the_device() {
    let (mut net, mut a) = board(101);
    a.set(1, 1, 10);
    net.settle(&mut [&mut a]);
    let head = a.e.status().head;
    let from = a.persists.len();
    let calls = net.calls.len();
    a.feed(Event::PairRegister { ticket: 7, device: new_device(102, "Laptop") });
    // Nothing is enqueued before the upload completed.
    assert!(puts(&a, from).iter().all(|r| r.kind != Kind::Ctl));
    net.settle(&mut [&mut a]);
    let (snapshot, added_seq) = pair_result(&a, 7).expect("an answer").expect("paired");
    assert_eq!(snapshot.seq, head.seq);
    assert_eq!(added_seq, head.seq + 1);
    let names: Vec<&str> = net.calls[calls..].iter().map(|c| c.1.as_str()).filter(|c| c.starts_with("blob") || c.starts_with("admin") || *c == "append").collect();
    assert_eq!(names, vec!["blob:upload:2", "admin:register"]);
    let ctl: Vec<_> = puts(&a, from).into_iter().filter(|r| r.kind == Kind::Ctl).collect();
    let Body::Ctl(CtlIntent::AddDevice { bootstrap_seq, .. }) = &ctl[0].body else { panic!("a DeviceAdded") };
    assert_eq!((*bootstrap_seq, ctl[0].based_on), (snapshot.seq, head));
    assert!(matches!(ctl[0].via, Via::Register { dev, .. } if dev == new_device(102, "Laptop").dev));
    // The new device starts from the bundle and converges.
    let mut b = pair(&mut net, &mut a, 103, "Phone", 8);
    b.set(1, 2, 20);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(a.board_bytes(), b.board_bytes());
    assert_eq!(b.e.device_no(), Some(3));
    assert_eq!(a.e.status().devices.len(), 3);
}

fn status_of(n: &Node, of: &Node) -> DeviceStatus {
    n.e.status().devices.iter().find(|d| d.dev == of.dev()).expect("a row").status
}

fn rotations(n: &Node, from: usize) -> Vec<[u8; 16]> {
    let mut ids = Vec::new();
    for r in puts(n, from) {
        if matches!(r.body, Body::Ctl(CtlIntent::Rotate { .. })) && !ids.contains(&r.entry_id) {
            ids.push(r.entry_id);
        }
    }
    ids
}

/// Takes the one admin request waiting at `n`.
fn take_admin(n: &mut Node) -> (u64, AdminRequest) {
    let i = n.fx.iter().position(|f| matches!(f, Effect::Admin { .. })).expect("an admin request");
    match n.fx.remove(i) {
        Some(Effect::Admin { req, op }) => (req, op),
        _ => unreachable!(),
    }
}

fn refusal(status: u16, code: &str) -> RelayRefusal {
    RelayRefusal { status, code: code.into(), until: None, head: None, index: None }
}

// spec: ENG-OPS-002 ENG-OPS-009
#[test]
fn a_revoke_asks_the_relay_first_then_rotates() {
    let (mut net, mut a) = board(111);
    let mut b = pair(&mut net, &mut a, 112, "Laptop", 1);
    let mut c = pair(&mut net, &mut a, 113, "Phone", 2);
    c.set(1, 1, 10);
    net.settle(&mut [&mut a, &mut b, &mut c]);

    // Any refusal but `relay/target` ends the operation with nothing enqueued.
    let from = a.persists.len();
    a.feed(Event::Op { ticket: 8, op: Operation::Revoke { dev: c.dev(), replace: false } });
    assert!(rotations(&a, from).is_empty());
    let (req, op) = take_admin(&mut a);
    assert_eq!(op, AdminRequest::Revoke { dev: c.dev() });
    a.feed(Event::AdminResult { req, result: Err(refusal(403, "relay/forbidden")) });
    assert_eq!(op_result(&a, 8), Some(Err("relay/forbidden")));
    assert!(rotations(&a, from).is_empty());
    assert!(a.e.status().ops.is_empty());

    // `409 relay/target` (already revoked at the relay) counts as done: the rotation is
    // enqueued in the same write as the operation's stage 2.
    let from = a.persists.len();
    let calls = net.calls.len();
    a.feed(Event::Op { ticket: 9, op: Operation::Revoke { dev: c.dev(), replace: false } });
    let (req, op) = take_admin(&mut a);
    let generation = net.gens[&(super::kit_tests::relay().host, a.dev())];
    let cred = crate::sim::relay::Cred::Token { dev: a.dev(), generation };
    let done = net.relay().admin(a.wall, &cred, &a.dev(), &op);
    assert!(done.result.is_ok());
    assert!(rotations(&a, from).is_empty());
    a.feed(Event::AdminResult { req, result: Err(refusal(409, "relay/target")) });
    let ids = rotations(&a, from);
    assert_eq!(ids.len(), 1);
    let write = a.persists[from..]
        .iter()
        .find(|w| w.iter().any(|op| matches!(op, PersistOp::Outbox(_))) && puts_in(w).iter().any(|r| r.entry_id == ids[0]))
        .expect("the write that enqueued the rotation");
    let stage = write.iter().find_map(|op| match op {
        PersistOp::WriteMeta(m) => m.ops.iter().find(|o| o.ticket == 9).map(|o| o.stage),
        _ => None,
    });
    assert_eq!(stage, Some(2));
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));
    assert!(net.calls[calls..].iter().any(|c| c.1 == "append"));
    assert_eq!((status_of(&a, &c), status_of(&b, &c)), (DeviceStatus::Revoked, DeviceStatus::Revoked));
    // The board goes on under the new epoch without the revoked device.
    a.set(2, 2, 20);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(a.board_bytes(), b.board_bytes());
    assert_eq!(b.e.confirmed().0.keys[&2].0, 20);
}

fn puts_in(write: &[PersistOp]) -> Vec<crate::OutboxRecord> {
    let mut out = Vec::new();
    for op in write {
        if let PersistOp::Outbox(ops) = op {
            for o in ops {
                if let crate::OutboxOp::Put(r) = o {
                    out.push(r.clone());
                }
            }
        }
    }
    out
}

// spec: ENG-OPS-002
#[test]
fn a_rotation_that_loses_a_race_is_composed_again() {
    let (mut net, mut a) = board(121);
    let mut b = pair(&mut net, &mut a, 122, "Laptop", 1);
    let mut c = pair(&mut net, &mut a, 123, "Phone", 2);
    let mut d = pair(&mut net, &mut a, 124, "Tablet", 3);
    net.settle(&mut [&mut a, &mut b, &mut c, &mut d]);
    let (fa, fb) = (a.persists.len(), b.persists.len());
    // A revokes C and B revokes D; each seals its rotation before it sees the other's.
    a.feed(Event::Op { ticket: 5, op: Operation::Revoke { dev: c.dev(), replace: false } });
    b.feed(Event::Op { ticket: 6, op: Operation::Revoke { dev: d.dev(), replace: false } });
    for n in [&mut a, &mut b] {
        let fx = n.fx.iter().position(|f| matches!(f, Effect::Admin { .. })).and_then(|i| n.fx.remove(i)).expect("the revoke");
        net.answer(n, fx);
        assert!(n.fx.iter().any(|f| matches!(f, Effect::Append { .. })));
    }
    // B's lands first; A's is refused in the log and A composes its rotation again.
    net.settle(&mut [&mut b, &mut a]);
    assert_eq!((op_result(&a, 5), op_result(&b, 6)), (Some(Ok(())), Some(Ok(()))));
    assert_eq!(rotations(&b, fb).len(), 1);
    assert_eq!(rotations(&a, fa).len(), 2);
    for n in [&a, &b] {
        assert_eq!((status_of(n, &c), status_of(n, &d)), (DeviceStatus::Revoked, DeviceStatus::Revoked));
    }
    a.set(1, 1, 1);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(a.board_bytes(), b.board_bytes());
}

// The codes of `relay.md` §19 reach the facade as they are (engine.md §33).
#[test]
fn every_code_of_the_relays_catalogue_passes_through() {
    let doc = include_str!("../../../../docs/sync/spec/relay.md");
    let from = doc.find("\n## 19. Error catalogue").expect("the catalogue");
    let to = from + doc[from..].find("\n## 20").expect("its end");
    let mut seen = 0;
    for piece in doc[from..to].split('`') {
        if piece.starts_with("relay/") && piece.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'/') {
            assert_eq!(super::send::intern(piece), piece);
            seen += 1;
        }
    }
    assert!(seen >= 66);
    assert_eq!(super::send::intern("relay/made_up"), "relay/refused");
}

/// Takes the one blob request waiting at `n`.
fn take_blob(n: &mut Node) -> (u64, BlobRequest) {
    let i = n.fx.iter().position(|f| matches!(f, Effect::Blob { .. })).expect("a blob request");
    match n.fx.remove(i) {
        Some(Effect::Blob { req, op }) => (req, op),
        _ => unreachable!(),
    }
}

fn controls(n: &Node, from: usize) -> usize {
    puts(n, from).iter().filter(|r| r.kind == Kind::Ctl).count()
}

// spec: ENG-OPS-004 ENG-OPS-006 ENG-OPS-008
#[test]
fn a_pairing_that_cannot_stand_is_dropped_or_undone() {
    let (mut net, mut a) = board(131);
    let mut b = pair(&mut net, &mut a, 132, "Laptop", 1);
    let c = pair(&mut net, &mut a, 133, "Phone", 2);
    let d = pair(&mut net, &mut a, 134, "Tablet", 3);
    net.settle(&mut [&mut a, &mut b]);
    let host = super::kit_tests::relay().host;

    // The upload is refused: nothing is enqueued, the pairing fails with the relay's code.
    let from = a.persists.len();
    a.feed(Event::PairRegister { ticket: 10, device: new_device(140, "One") });
    let (req, _) = take_blob(&mut a);
    a.feed(Event::BlobResult { req, result: Err(refusal(507, "relay/blob_quota")) });
    assert_eq!(pair_result(&a, 10), Some(Err("relay/blob_quota")));
    assert_eq!(controls(&a, from), 0);
    assert!(a.fx.iter().all(|f| !matches!(f, Effect::Admin { .. })));

    // The epoch moves while the upload is on its way: the DeviceAdded is never sealed.
    let from = a.persists.len();
    let calls = net.calls.len();
    a.feed(Event::PairRegister { ticket: 11, device: new_device(141, "Two") });
    let (req, op) = take_blob(&mut a);
    b.feed(Event::Op { ticket: 20, op: Operation::Revoke { dev: c.dev(), replace: false } });
    net.settle(&mut [&mut b, &mut a]);
    assert_eq!(op_result(&b, 20), Some(Ok(())));
    assert_eq!(status_of(&a, &c), DeviceStatus::Revoked);
    let cred = net.cred(&host, &a.dev());
    let result = net.relay().blob(a.wall, &cred, &op).result;
    assert!(result.is_ok());
    a.feed(Event::BlobResult { req, result });
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(pair_result(&a, 11), Some(Err("seal/pair_raced")));
    assert!(net.calls[calls..].iter().all(|c| c.1 != "admin:register"));
    // The record was dropped unsent: no frame was ever sealed for it and nothing waits.
    assert!(puts(&a, from).iter().filter(|r| r.kind == Kind::Ctl).all(|r| r.frame.is_none()));
    assert_eq!(a.e.status().queued, 0);

    // The epoch moves after the DeviceAdded was sealed and before the relay stored it: the
    // entry is refused in the log, and the device is revoked at the relay before the failure is reported.
    let three = new_device(142, "Three");
    a.feed(Event::PairRegister { ticket: 12, device: three.clone() });
    let (req, op) = take_blob(&mut a);
    let result = net.relay().blob(a.wall, &cred, &op).result;
    a.feed(Event::BlobResult { req, result });
    let (req, register) = take_admin(&mut a);
    assert!(matches!(register, AdminRequest::Register { dev, .. } if dev == three.dev));
    b.feed(Event::Op { ticket: 21, op: Operation::Revoke { dev: d.dev(), replace: false } });
    net.settle(&mut [&mut b]);
    assert_eq!(op_result(&b, 21), Some(Ok(())));
    let reply = net.relay().admin(a.wall, &cred, &a.dev(), &register);
    assert!(reply.result.is_ok());
    a.feed(Event::AdminResult { req, result: reply.result });
    // The relay stored it: the pairing is reported registered (§28.4 step 3) ...
    assert!(matches!(pair_result(&a, 12), Some(Ok(_))));
    // ... and then the entry is refused in A's log (step 4): revoke at the relay first, then the failure.
    let failed = |n: &Node| {
        n.notes.iter().find_map(|x| match x {
            crate::Notification::PairFailed { ticket: 12, code } => Some(*code),
            _ => None,
        })
    };
    let mut revoked_first = false;
    for _ in 0..200 {
        if a.fx.is_empty() {
            a.tick(30_000);
        }
        let Some(fx) = a.fx.pop_front() else { continue };
        if let Effect::Admin { op: AdminRequest::Revoke { dev }, .. } = &fx {
            assert_eq!(*dev, three.dev);
            assert_eq!(failed(&a), None);
            revoked_first = true;
        }
        net.answer(&mut a, fx);
        if failed(&a).is_some() {
            break;
        }
    }
    assert!(revoked_first);
    assert_eq!(failed(&a), Some("seal/stale_epoch"));
    assert!(a.e.status().devices.iter().all(|r| r.dev != three.dev || r.status != DeviceStatus::Current));
}

fn members(n: &Node) -> Vec<(u16, DeviceStatus, String)> {
    let mut rows: Vec<_> = n.e.status().devices.iter().map(|d| (d.device_no, d.status, d.name.clone())).collect();
    rows.sort_by_key(|r| r.0);
    rows
}

// spec: ENG-RESET-001 ENG-RESET-002 ENG-RESET-003
#[test]
fn a_restore_to_a_point_resets_the_board_and_nothing_else() {
    let (mut net, mut a) = board(151);
    let mut b = pair(&mut net, &mut a, 152, "Laptop", 1);
    let boot = a.e.status().devices.iter().find(|d| d.dev == b.dev()).expect("B's row").bootstrap_seq;
    for k in 1..=5u64 {
        a.set(k, k as u8, k as i64 * 10);
    }
    net.settle(&mut [&mut a, &mut b]);
    let t = a.e.status().board_seq;
    let keys_t = a.e.confirmed().0.keys.clone();
    b.set(6, 1, 999);
    a.enqueue(7, crate::toy::ToyCommand::Del { key: 2 });
    net.settle(&mut [&mut a, &mut b]);
    let head = a.e.status().board_seq;
    assert_ne!(a.e.confirmed().0.keys, keys_t);

    // Offered: below the board head and above every current device's bootstrap seq.
    let offered: Vec<u64> = a.e.restore_targets().iter().map(|x| x.0).collect();
    assert_eq!(offered, (boot + 1..head).collect::<Vec<_>>());
    assert!(offered.contains(&t));
    for bad in [boot, head, head + 5] {
        a.feed(Event::Op { ticket: 100 + bad, op: Operation::RestoreTo { seq: bad } });
        assert!(matches!(op_result(&a, 100 + bad), Some(Err(_))), "{bad}");
    }

    let before = (members(&a), a.e.membership().epoch);
    a.feed(Event::Op { ticket: 3, op: Operation::RestoreTo { seq: t } });
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 3), Some(Ok(())));
    for n in [&a, &b] {
        // The board is the one at `t`; the members and the epoch are what they were.
        assert_eq!(n.e.confirmed().0.keys.iter().map(|(k, v)| (*k, v.0)).collect::<Vec<_>>(), keys_t.iter().map(|(k, v)| (*k, v.0)).collect::<Vec<_>>());
        assert_eq!((members(n), n.e.membership().epoch), before);
    }
    // The other device shows the reset; its author does not.
    assert!(b.e.status().banners.iter().any(|x| x.kind == crate::BannerKind::Reset));
    assert_eq!(a.board_bytes(), b.board_bytes());

    // A ResetTo whose hash is not this device's hash at the target is not applied: the
    // device raises ResetMismatch and stops as diverged.
    a.set(8, 9, 90);
    net.settle(&mut [&mut a, &mut b]);
    let board = b.board_bytes();
    let wrong = CtlIntent::ResetTo { seq: t, state_hash: [9; 32], hash_version: 1 };
    a.e.enqueue_own(Kind::Ctl, Body::Ctl(wrong), Via::Append, None).expect("an entry id");
    let out = std::mem::take(&mut a.e.out);
    a.absorb(out);
    a.feed(Event::Tick);
    net.settle(&mut [&mut a, &mut b]);
    let st = b.e.status();
    assert_eq!(b.board_bytes(), board);
    assert!(st.alarms.iter().any(|x| x.kind == crate::AlarmKind::ResetMismatch));
    assert!(st.stops.iter().any(|s| matches!(s.kind, crate::StopKind::Divergence)));
}

fn position(n: &Node, from: usize, f: impl Fn(&PersistOp) -> bool) -> Option<usize> {
    n.persists[from..].iter().position(|w| w.iter().any(&f)).map(|i| i + from)
}

fn identity_writes(n: &Node, from: usize) -> usize {
    n.persists[from..].iter().flatten().filter(|op| matches!(op, PersistOp::WriteIdentity(_))).count()
}

// spec: ENG-PURGE-001 ENG-PURGE-002 ENG-PURGE-003 ENG-PURGE-004
#[test]
fn a_purge_leaves_no_current_device_behind() {
    let (mut net, mut a) = board(161);
    let mut b = pair(&mut net, &mut a, 162, "Laptop", 1);
    for k in 1..=6u64 {
        a.set(k, k as u8, k as i64);
    }
    net.settle(&mut [&mut a, &mut b]);
    // B's newest entry stands on this head; what A appends after it B has not reported on.
    let reported = b.e.status().head.seq;
    b.set(20, 20, 1);
    net.settle(&mut [&mut a, &mut b]);
    for k in 7..=9u64 {
        a.set(k, k as u8, k as i64);
    }
    net.settle(&mut [&mut a, &mut b]);
    assert!(a.e.status().board_seq > reported + 2);

    let (p, before_ms) = a.e.purge_preview().expect("something to purge");
    assert_eq!(p, reported + 1);
    assert_eq!(Some(before_ms), a.e.tau_at(p - 1));
    let (fa, fb) = (a.persists.len(), b.persists.len());
    let admins = net.admins.len();
    let epoch = a.e.membership().epoch;
    a.feed(Event::Op { ticket: 4, op: Operation::Purge });
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 4), Some(Ok(())));
    let n = a.e.membership().epoch;
    assert_eq!(n, epoch + 1);
    // The relay is asked to purge below `p` under the new epoch, after the rotation was accepted.
    assert!(net.admins[admins..].contains(&(a.dev(), AdminRequest::Purge { before_seq: p, epoch: n })));
    assert!(a.has_note(|x| matches!(x, crate::Notification::PurgeBackups { before_ms: t } if *t == before_ms)));
    assert!(a.has_note(|x| matches!(x, crate::Notification::ReuploadPicture { epoch: e } if *e == n)));
    // Every device: a base at the rotation's seq first, then the log below `p` and the older bases go.
    for (node, from) in [(&a, fa), (&b, fb)] {
        let base = position(node, from, |op| matches!(op, PersistOp::WriteBase(_))).expect("a base");
        let r = node.persists[base]
            .iter()
            .find_map(|op| match op {
                PersistOp::WriteBase(c) => Some(c.board_seq),
                _ => None,
            })
            .expect("its seq");
        let cut = position(node, from, |op| matches!(op, PersistOp::TruncateLogBelow(below, _) if *below == p)).expect("the truncation");
        let gone = position(node, from, |op| matches!(op, PersistOp::DeleteBases(_))).expect("the older bases deleted");
        assert!(base < cut && base < gone);
        assert_eq!(node.store.log_floor, p);
        assert!(node.store.bases.keys().all(|s| *s >= r));
        assert!(node.store.log.keys().all(|s| *s >= p));
        assert_eq!(node.e.membership().purge_seq, p);
    }
    // Nothing more to purge until another device reports a newer head.
    assert_eq!(a.e.purge_preview(), Err("engine/nothing_to_purge"));
    a.feed(Event::Op { ticket: 5, op: Operation::Purge });
    assert_eq!(op_result(&a, 5), Some(Err("engine/nothing_to_purge")));

    // The old roots go once the log clock is more than thirty days past the rotation, not before.
    let (ia, ib) = (a.persists.len(), b.persists.len());
    for _ in 0..29 {
        a.advance(86_400_000);
        b.advance(86_400_000);
        net.settle(&mut [&mut a, &mut b]);
    }
    assert_eq!((identity_writes(&a, ia), identity_writes(&b, ib)), (0, 0));
    for _ in 0..3 {
        a.advance(86_400_000);
        b.advance(86_400_000);
        net.settle(&mut [&mut a, &mut b]);
    }
    assert!(identity_writes(&a, ia) >= 1 && identity_writes(&b, ib) >= 1);
    a.set(30, 30, 30);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(a.board_bytes(), b.board_bytes());
}

// spec: ENG-SNAP-004 ENG-SNAP-005 ENG-SNAP-002
#[test]
fn a_snapshot_is_uploaded_once_announced_after_and_acknowledged() {
    let (mut net, mut a) = board(171);
    let mut b = pair(&mut net, &mut a, 172, "Laptop", 1);
    let (fa, fb) = (a.persists.len(), b.persists.len());
    let calls = net.calls.len();
    let snaps = |n: &Node, from: usize| {
        let mut ids = Vec::new();
        for r in puts(n, from) {
            if let Body::Snap(info) = &r.body
                && !ids.iter().any(|x: &([u8; 16], u64)| x.0 == r.entry_id)
            {
                ids.push((r.entry_id, info.seq));
            }
        }
        ids
    };
    // Like `settle`, but looking at every upload before it is answered: no announcement
    // exists while the upload of a snapshot is still on its way.
    let rounds = |net: &mut super::rkit_tests::Net, a: &mut Node, b: &mut Node, n: usize| {
        for _ in 0..n {
            for (node, from) in [(&mut *a, fa), (&mut *b, fb)] {
                while let Some(fx) = node.fx.pop_front() {
                    if matches!(&fx, Effect::Blob { op: BlobRequest::Upload { purpose: 1, .. }, .. }) {
                        assert!(snaps(node, from).is_empty());
                    }
                    net.answer(node, fx);
                }
            }
            a.tick(30_000);
            b.tick(30_000);
        }
    };
    // The log passes seq 64: the snapshot of 64 is due, and whoever's turn it is writes it.
    let mut k = 0u64;
    while net.log_len() < 70 {
        k += 1;
        a.set(k, (k % 50) as u8, k as i64);
        rounds(&mut net, &mut a, &mut b, 3);
    }
    for _ in 0..3 {
        a.advance(86_400_000);
        b.advance(86_400_000);
        rounds(&mut net, &mut a, &mut b, 6);
    }
    net.settle(&mut [&mut a, &mut b]);
    let uploads: Vec<_> = net.calls[calls..].iter().filter(|c| c.1 == "blob:upload:1").map(|c| c.0).collect();
    assert_eq!(uploads.len(), 1);
    let (writer, wf, other, of) = if uploads[0] == a.dev() { (&a, fa, &b, fb) } else { (&b, fb, &a, fa) };
    assert_eq!(snaps(writer, wf).iter().map(|x| x.1).collect::<Vec<_>>(), vec![64]);
    assert!(snaps(other, of).is_empty());
    // The other device recorded a hash at 64: one verdict, Ack, in its next beat.
    let acks: Vec<_> = puts(other, of)
        .into_iter()
        .filter_map(|r| match r.body {
            Body::Beat(info) if !info.acks.is_empty() => Some((r.entry_id, info.acks)),
            _ => None,
        })
        .collect();
    assert!(!acks.is_empty());
    assert!(acks.iter().all(|x| x.0 == acks[0].0), "one beat carries it");
    assert_eq!(acks[0].1.len(), 1);
    assert_eq!(acks[0].1[0].1, crate::Verdict::Ack);
    // The writer's own beats carry none.
    assert!(puts(writer, wf).iter().all(|r| !matches!(&r.body, Body::Beat(info) if !info.acks.is_empty())));
}

// spec: ENG-COMPACT-001 ENG-COMPACT-002 ENG-COMPACT-003 ENG-COMPACT-004
#[test]
fn compaction_keeps_what_the_board_needs() {
    const DAY: i64 = 86_400_000;
    let (mut net, mut a) = board(181);
    let mut b = pair(&mut net, &mut a, 182, "Laptop", 1);
    let added_seq = pair_result(&a, 1).expect("an answer").expect("paired").1;
    // A Skip early in the log: its entry and its target must outlive every compaction.
    b.set(1, 77, 7);
    net.settle(&mut [&mut a, &mut b]);
    let target = net.log_len();
    a.feed(Event::Op { ticket: 2, op: Operation::Skip { seq: target } });
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 2), Some(Ok(())));
    let skip_seq = net.log_len();
    let mut k = 0u64;
    let mut grow = |net: &mut super::rkit_tests::Net, a: &mut Node, b: &mut Node, n: u64| {
        let to = net.log_len() + n;
        while net.log_len() < to {
            for _ in 0..(to - net.log_len()).min(1_000) {
                k += 1;
                a.set(10 + k, (k % 60) as u8, k as i64);
            }
            net.settle(&mut [&mut *a, &mut *b]);
            // The relay's daily append budget pauses the sender until the next UTC day.
            if let Some(until) = a.e.status().paused_until_ms {
                let wait = (until - a.wall).max(0) as u64 + 60_000;
                a.advance(wait);
                b.advance(wait);
                net.settle(&mut [&mut *a, &mut *b]);
            }
        }
    };
    grow(&mut net, &mut a, &mut b, 100);
    // More than a year passes (beats, weekly snapshots), then the log grows past 16,384 more.
    let mut seen: Vec<(u64, i64)> = Vec::new();
    for _ in 0..370 {
        a.advance(DAY as u64);
        b.advance(DAY as u64);
        net.settle(&mut [&mut a, &mut b]);
        seen.push((net.log_len(), a.wall));
    }
    grow(&mut net, &mut a, &mut b, 16_500);
    for _ in 0..3 {
        a.advance(DAY as u64);
        b.advance(DAY as u64);
        net.settle(&mut [&mut a, &mut b]);
    }
    let compacts: Vec<_> = net
        .admins
        .iter()
        .zip(&net.admin_ctx)
        .filter_map(|((dev, op), ctx)| match op {
            AdminRequest::Compact { below_seq, snap_entry_seq, blob_id, keep } => {
                Some((*dev, *below_seq, *snap_entry_seq, *blob_id, keep.clone(), ctx.clone()))
            }
            _ => None,
        })
        .collect();
    assert!(!compacts.is_empty());
    let mut days = std::collections::BTreeSet::new();
    for (dev, below, snap_entry, blob, keep, (head_then, floor_then, now, answer)) in &compacts {
        let node = if *dev == a.dev() { &a } else { &b };
        assert_eq!(answer, &Ok(()), "the relay took it");
        // A usable snapshot by the other device, acknowledged by this one.
        let rec = node.e.registry.snapshots.iter().find(|r| r.entry_seq == *snap_entry).expect("the snapshot's record");
        assert_ne!(rec.author, *dev);
        assert_eq!(rec.own, Some(crate::Verdict::Ack));
        assert_eq!(rec.info.blob, *blob);
        // Not above the snapshot, not within the newest 16,384, not within 366 days.
        assert!(*below <= rec.info.seq);
        assert!(*below + 16_384 <= *head_then, "{below} {head_then}");
        assert!(seen.iter().any(|(h, w)| *h + 1 >= *below && *w + 366 * DAY <= *now), "{below}");
        // Every control entry and Skip target between the relay's floor and `below_seq` is kept
        // (what lies below the floor was kept by an earlier compaction and is not a candidate).
        for must in [added_seq, target, skip_seq] {
            assert!(must >= *below || must < *floor_then || keep.contains(&must), "{must} {keep:?}");
        }
        assert!(keep.windows(2).all(|w| w[0] < w[1]));
        assert!(days.insert((*dev, now.div_euclid(DAY))), "one Compact per UTC day");
    }
    // The board goes on, and what was kept is still served.
    a.set(5, 5, 5);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(a.board_bytes(), b.board_bytes());
    let floor = net.relay().board_row().map(|r| r.compact_floor).unwrap_or(0);
    assert!(floor > target, "something was deleted: floor {floor}");
    for must in [added_seq, target, skip_seq] {
        assert!(net.relay().log().contains_key(&must));
    }
}

fn notes_of<T>(n: &Node, f: impl Fn(&crate::Notification) -> Option<T>) -> Vec<T> {
    n.notes.iter().filter_map(f).collect()
}

fn type_code(n: &mut Node, text: &str) -> Result<(), &'static str> {
    let before = notes_of(n, |x| match x {
        crate::Notification::MoveCodeResult { result } => Some(*result),
        _ => None,
    })
    .len();
    n.feed(Event::User(crate::UserAction::MoveCode { text: text.to_owned() }));
    let after = notes_of(n, |x| match x {
        crate::Notification::MoveCodeResult { result } => Some(*result),
        _ => None,
    });
    // ENG-MOVE-008: exactly one answer, in the same call.
    assert_eq!(after.len(), before + 1);
    after[before]
}

// spec: ENG-MOVE-001 ENG-MOVE-002 ENG-MOVE-003 ENG-MOVE-006 ENG-MOVE-007 ENG-MOVE-008 ENG-MOVE-009
#[test]
fn a_move_fences_seeds_and_is_followed_only_with_the_code() {
    const NEW: &str = "relay2.test";
    let old = super::kit_tests::relay().host;
    let (mut net, mut a) = board(211);
    let mut b = pair(&mut net, &mut a, 212, "Laptop", 1);
    let mut k = 0u64;
    while net.log_len() < 70 {
        k += 1;
        a.set(k, (k % 50) as u8, k as i64);
        net.settle(&mut [&mut a, &mut b]);
    }
    for _ in 0..3 {
        a.advance(86_400_000);
        b.advance(86_400_000);
        net.settle(&mut [&mut a, &mut b]);
    }
    net.add_relay(NEW);
    let (calls, admins) = (net.calls.len(), net.admins.len());
    a.feed(Event::Op { ticket: 9, op: Operation::Move { host: NEW.to_owned() } });
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));

    // The old relay sequenced the Fence at `f`, its last seq; the seed carries the log through `f`.
    let f = net.relays[&old].head().expect("a head").seq;
    let mine: Vec<&str> = net.calls[calls..].iter().filter(|c| c.0 == a.dev()).map(|c| c.1.as_str()).collect();
    let at = |name: &str| mine.iter().position(|c| *c == name).unwrap_or_else(|| panic!("{name} in {mine:?}"));
    assert!(at("admin:door") < at("admin:fence") && at("admin:fence") < at("admin:seed") && at("admin:seed") < at("admin:seed_tail"));
    let seed = net.admins[admins..]
        .iter()
        .find_map(|(_, op)| match op {
            AdminRequest::Seed { host, home_gen, first_seq, last_seq, devices, blob_id, .. } => {
                Some((host.clone(), *home_gen, *first_seq, *last_seq, devices.clone(), *blob_id))
            }
            _ => None,
        })
        .expect("the seed");
    let (host, home_gen, first_seq, last_seq, devices, blob_id) = seed;
    assert_eq!((host.as_str(), home_gen, first_seq, last_seq), (NEW, 2, 1, f));
    // Every device and the recovery identity, each with its key and its counter at `first_seq − 1`.
    let mut seeded: Vec<_> = devices.iter().map(|d| (d.dev, d.last_ctr)).collect();
    seeded.sort();
    assert!(seeded.iter().any(|d| d.0 == a.dev()) && seeded.iter().any(|d| d.0 == b.dev()));
    assert_eq!(seeded.len(), 3, "two devices and the recovery identity");
    assert!(seeded.iter().all(|d| d.1 == 0));
    // The frames are the old relay's, unchanged.
    for seq in first_seq..=last_seq {
        assert_eq!(net.relays[NEW].log()[&seq].frame, net.relays[&old].log()[&seq].frame, "{seq}");
    }
    // Only a blob the mover sealed itself is uploaded there: the one the seed names.
    let blobs: Vec<_> = net.relays[NEW].blobs().values().map(|r| (r.id, r.author)).collect();
    assert_eq!(blobs, vec![(blob_id, a.dev())]);
    let reports = notes_of(&a, |x| match x {
        crate::Notification::MoveReport { carried_through, old_relay_answered } => Some((*carried_through, *old_relay_answered)),
        _ => None,
    });
    assert_eq!(reports, vec![(f, true)]);
    let code = notes_of(&a, |x| match x {
        crate::Notification::MoveCode { text } => Some(text.clone()),
        _ => None,
    })
    .pop()
    .expect("the move code");

    // The other device saw the Fence and stays where it is until its user types the code.
    assert_eq!(net.at[&b.dev()], old);
    let moving = b.e.status().moving.expect("a move awaits its code");
    assert_eq!((moving.host.as_str(), moving.home_gen, moving.fence_seq), (NEW, 2, f));
    assert!(b.e.status().banners.iter().any(|x| x.kind == crate::BannerKind::Moved));
    for _ in 0..5 {
        assert_eq!(type_code(&mut b, "0000-0000"), Err("seal/move_code"));
    }
    // Five wrong codes in a row lock the entry for an hour, the right code included.
    assert_eq!(type_code(&mut b, &code), Err("engine/move_locked"));
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(net.at[&b.dev()], old);
    b.advance(3_600_000);
    a.advance(3_600_000);
    assert_eq!(type_code(&mut b, &code), Ok(()));
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(net.at[&b.dev()], NEW);
    a.set(900, 1, 1);
    b.set(901, 2, 2);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(a.board_bytes(), b.board_bytes());
    assert!(net.relays[NEW].head().expect("a head").seq > f);
    assert_eq!(net.relays[&old].head().expect("a head").seq, f);
}
