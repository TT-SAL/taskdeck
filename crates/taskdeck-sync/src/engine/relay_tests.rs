//! Operations against the model relay: pairing, revoking, purging, restoring to a
//! point, compaction, the move.

use super::flow_tests::{op_result, puts};
use super::kit_tests::Node;
use super::rkit_tests::{board, new_device, pair, pair_result};
use crate::{
    AdminRequest, BlobRequest, Body, CtlIntent, DeviceStatus, Effect, Event, Kind, Operation, PersistOp, RelayRefusal, Via,
};

// spec: ENG-OPS-001 ENG-OPS-008 ENG-BOOT-003 SEAL-SIG-005 SEAL-PAIR-010
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
    // The DeviceAdded names exactly what the new device sent, and the `register` arguments
    // name the same keys and carry the frame the relay stored at the added seq.
    let sent = new_device(102, "Laptop");
    let Body::Ctl(CtlIntent::AddDevice { dev, sign_pk, hpke_pk, name, transcript, .. }) = &ctl[0].body else { panic!("a DeviceAdded") };
    assert_eq!((dev, sign_pk, hpke_pk, name, transcript), (&sent.dev, &sent.sign_pk, &sent.hpke_pk, &sent.name, &sent.transcript));
    let (reg_frame, reg_dev, reg_pk) = net
        .admins
        .iter()
        .find_map(|(_, op)| match op {
            AdminRequest::Register { frame, dev, sign_pk } => Some((frame.clone(), *dev, *sign_pk)),
            _ => None,
        })
        .expect("the register request");
    assert_eq!((reg_dev, reg_pk), (sent.dev, sent.sign_pk));
    assert_eq!(net.relay().log()[&added_seq].frame, reg_frame);
    let sealed = puts(&a, from).into_iter().rev().find(|r| r.entry_id == ctl[0].entry_id).expect("the sealed record");
    assert_eq!(sealed.frame.as_ref(), Some(&reg_frame));
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

// spec: ENG-OPS-002 ENG-CTL-002
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
    // The pending root of A's refused rotation is discarded and the identity written before A
    // seals anything further: between the write that sealed the first rotation and the one
    // that seals the second, an identity is written that differs from the one before it.
    let ids = rotations(&a, fa);
    let sealed_at = |id: &[u8; 16]| {
        position(&a, fa, |op| matches!(op, PersistOp::Outbox(ops) if ops.iter().any(|o| matches!(o, crate::OutboxOp::Put(r) if r.entry_id == *id && r.frame.is_some())))).expect("sealed")
    };
    let (first, second) = (sealed_at(&ids[0]), sealed_at(&ids[1]));
    let identities: Vec<(usize, Vec<u8>)> = a.persists[..=second]
        .iter()
        .enumerate()
        .flat_map(|(i, w)| w.iter().filter_map(move |op| match op {
            PersistOp::WriteIdentity(bytes) => Some((i, bytes.to_vec())),
            _ => None,
        }))
        .collect();
    let with_pending = identities.iter().rev().find(|x| x.0 <= first + 1).expect("the identity with the pending root");
    let discarded = identities.iter().find(|x| x.0 > first + 1 && x.0 < second).expect("the identity after the refusal");
    assert_ne!(discarded.1, with_pending.1);
    let dropped = position(&a, fa, |op| matches!(op, PersistOp::Outbox(ops) if ops.iter().any(|o| matches!(o, crate::OutboxOp::Drop(id) if *id == ids[0])))).expect("the refused record dropped");
    assert!(dropped < second);
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
// spec: ENG-BOOT-001 ENG-BOOT-002 ENG-BOOT-007 ENG-BOOT-008 ENG-UI-003
#[test]
fn compaction_keeps_what_the_board_needs_and_a_device_left_behind_rejoins() {
    const DAY: i64 = 86_400_000;
    let (mut net, mut a) = board(181);
    let mut b = pair(&mut net, &mut a, 182, "Laptop", 1);
    let added_seq = pair_result(&a, 1).expect("an answer").expect("paired").1;
    // A third device pairs, queues one change and is then away for more than a year.
    let mut c = pair(&mut net, &mut a, 183, "Phone", 3);
    net.settle(&mut [&mut a, &mut b, &mut c]);
    c.feed(Event::Disconnected { unreachable: true, paused_until_ms: None });
    c.set(77, 99, 990);
    let waiting = puts(&c, 0).into_iter().rev().find(|r| r.kind == Kind::Cmd).expect("the queued record");
    let c_head = c.e.status().head.seq;
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

    // The device that was away comes back: the relay no longer holds the log after its head.
    assert!(c_head + 1 < floor);
    let dw = (a.wall - c.wall) as u64;
    c.advance(dw);
    let from = c.persists.len();
    c.feed(Event::Tick);
    net.settle(&mut [&mut c]);
    // It waits for its user: Rejoining, shown as one stop that is no record, and its log untouched.
    let st = c.e.status();
    assert_eq!(st.session, crate::SessionState::Rejoining);
    let stops: Vec<_> = st.stops.iter().map(|x| (x.kind.clone(), x.action)).collect();
    assert_eq!(stops, vec![(crate::StopKind::Rejoin, crate::StopAction::ConfirmRejoin)]);
    assert!(c.store.meta.as_ref().is_none_or(|m| m.stops.iter().all(|x| x.kind != crate::StopKind::Rejoin)));
    assert!(position(&c, from, |op| matches!(op, PersistOp::TruncateLogBelow(..))).is_none());
    assert!(position(&c, from, |op| matches!(op, PersistOp::WriteCheckpoint(x) if x.board_seq != c_head)).is_none());
    assert_eq!(c.e.status().head.seq, c_head);
    let old_board = c.board_bytes();
    c.feed(Event::User(crate::UserAction::ConfirmRejoin));
    net.settle(&mut [&mut a, &mut b, &mut c]);
    // Its state before the rejoin is kept as a base, written before anything is replaced.
    let kept = position(&c, from, |op| matches!(op, PersistOp::WriteBase(x) if x.board_seq == c_head && x.board == old_board)).expect("the base of the old state");
    let cut = position(&c, from, |op| matches!(op, PersistOp::TruncateLogBelow(..))).expect("the log replaced");
    assert!(kept < cut);
    // It ended on a usable snapshot at or above the relay's floor and caught up from there.
    let st = c.e.status();
    assert_eq!(st.session, crate::SessionState::Live);
    assert!(st.stops.is_empty());
    let continuity = |n: &Node| {
        n.e.status()
            .warnings
            .iter()
            .filter_map(|w| match w {
                crate::StatusWarning::ContinuityUnchecked { from } => Some(*from),
                _ => None,
            })
            .collect::<Vec<u64>>()
    };
    let from_seq = match continuity(&c).as_slice() {
        [from] => *from,
        other => panic!("one continuity warning, got {other:?}"),
    };
    assert!(from_seq + 1 >= floor);
    let rec = c.e.registry.snapshots.iter().find(|r| r.info.seq == from_seq).expect("the snapshot's record");
    assert!(super::duty::usable(rec, &c.dev()));
    assert_eq!(c.board_bytes(), a.board_bytes());
    // The record queued before the absence was sent as it was enqueued, and applied.
    let sent = puts(&c, from).into_iter().rev().find(|r| r.entry_id == waiting.entry_id).expect("sealed after the rejoin");
    assert_eq!((sent.at_ms, sent.based_on), (waiting.at_ms, waiting.based_on));
    assert!(sent.frame.is_some());
    assert_eq!(a.e.confirmed().0.keys[&99].0, 990);
    assert_eq!(c.e.status().queued, 0);
    // The warning stays across a restart until it is acknowledged, then it is gone for good.
    c.restart(184);
    assert_eq!(continuity(&c), vec![from_seq]);
    c.feed(Event::User(crate::UserAction::AckContinuity));
    assert!(continuity(&c).is_empty());
    c.restart(185);
    assert!(continuity(&c).is_empty());
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

// spec: ENG-MOVE-004 ENG-MOVE-005 ENG-MOVE-009
#[test]
fn a_move_from_a_relay_that_is_gone_sends_its_fence_as_an_append() {
    const NEW: &str = "relay2.test";
    let old = super::kit_tests::relay().host;
    let (mut net, mut a) = board(221);
    let mut b = pair(&mut net, &mut a, 222, "Laptop", 1);
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
    let last = net.log_len();
    // The old relay is gone; a copy of it without any Fence stands at a third address.
    let gone = net.relays.remove(&old).expect("the old relay");
    net.relays.insert("relay3.test".to_owned(), gone);
    for n in [&mut a, &mut b] {
        n.feed(Event::Disconnected { unreachable: true, paused_until_ms: None });
    }
    b.set(50, 50, 50);
    let calls = net.calls.len();
    a.feed(Event::Op { ticket: 9, op: Operation::Move { host: NEW.to_owned() } });
    net.settle(&mut [&mut a]);
    // A relay that does not answer is not gone until the user says so: the door is known, and
    // nothing is seeded.
    assert_eq!(op_result(&a, 9), None);
    assert!(net.calls[calls..].iter().all(|c| c.1 != "admin:seed"), "no seed without the Fence or the user's word");
    a.feed(Event::User(crate::UserAction::OldRelayGone));
    net.settle(&mut [&mut a]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));
    let mine: Vec<&str> = net.calls[calls..].iter().filter(|c| c.0 == a.dev()).map(|c| c.1.as_str()).collect();
    assert!(!mine.contains(&"admin:fence"), "{mine:?}");
    let at = |name: &str| mine.iter().position(|c| *c == name).unwrap_or_else(|| panic!("{name} in {mine:?}"));
    assert!(at("admin:door") < at("admin:seed") && at("admin:seed_tail") < at("append"));
    // The Fence is the first entry the new relay sequenced itself.
    let fence_seq = last + 1;
    assert_eq!(a.e.membership().home_seq, fence_seq);
    let reports = notes_of(&a, |x| match x {
        crate::Notification::MoveReport { carried_through, old_relay_answered } => Some((*carried_through, *old_relay_answered)),
        _ => None,
    });
    assert_eq!(reports, vec![(last, false)]);
    let code = notes_of(&a, |x| match x {
        crate::Notification::MoveCode { text } => Some(text.clone()),
        _ => None,
    })
    .pop()
    .expect("the move code");

    // The other device holds no Fence. It follows the address its user typed only when the log
    // there holds a Fence naming that address and the code fits; until then it sends nothing.
    let door = net.relays[NEW].door();
    let calls = net.calls.len();
    b.feed(Event::Op { ticket: 30, op: Operation::FollowAddress { host: "relay3.test".into(), door: net.relays["relay3.test"].door(), code: code.clone() } });
    net.settle(&mut [&mut b]);
    assert_eq!(op_result(&b, 30), Some(Err("engine/move_no_fence")));
    b.feed(Event::Op { ticket: 31, op: Operation::FollowAddress { host: NEW.into(), door, code: "0000-0000".into() } });
    net.settle(&mut [&mut b]);
    assert_eq!(op_result(&b, 31), Some(Err("seal/move_code")));
    assert_eq!(b.e.status().relay_host, old);
    assert_eq!(b.e.status().queued, 1);
    assert!(net.calls[calls..].iter().all(|c| c.0 != b.dev() || c.1 == "connect" || c.1 == "hello"), "nothing but reads");
    b.feed(Event::Op { ticket: 32, op: Operation::FollowAddress { host: NEW.into(), door, code } });
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&b, 32), Some(Ok(())));
    assert_eq!(b.e.status().relay_host, NEW);
    assert_eq!(b.e.status().queued, 0);
    assert_eq!(a.board_bytes(), b.board_bytes());
    assert_eq!(a.e.confirmed().0.keys[&50].0, 50);
}

// spec: ENG-OPS-010 ENG-SESS-007
#[test]
fn a_closed_board_is_fenced_to_nowhere_and_stops_who_follows() {
    const NOWHERE: &str = "closed.test";
    let (mut net, mut a) = board(231);
    let mut b = pair(&mut net, &mut a, 232, "Laptop", 1);
    net.settle(&mut [&mut a, &mut b]);
    let (calls, from) = (net.calls.len(), a.persists.len());
    let home_gen = a.e.membership().home_gen;
    a.feed(Event::Op { ticket: 4, op: Operation::CloseBoard { host: NOWHERE.into(), door: [9; 5] } });
    // Not done before the Fence is accepted in its log.
    assert_eq!(op_result(&a, 4), None);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 4), Some(Ok(())));
    let fences: Vec<_> = puts(&a, from).into_iter().filter(|r| matches!(r.body, Body::Ctl(CtlIntent::Fence { .. }))).collect();
    assert!(fences.iter().all(|r| r.entry_id == fences[0].entry_id), "exactly one Fence");
    assert_eq!(fences[0].body, Body::Ctl(CtlIntent::Fence { host: NOWHERE.into(), door: [9; 5], home_gen: home_gen + 1 }));
    assert!(matches!(fences[0].via, Via::Fence { .. }));
    // Nothing was sent to the relay it names: no door, no seed, no tail, no upload.
    let mine: Vec<&str> = net.calls[calls..].iter().filter(|c| c.0 == a.dev()).map(|c| c.1.as_str()).collect();
    assert!(mine.contains(&"admin:fence"));
    assert!(mine.iter().all(|c| !matches!(*c, "admin:door" | "admin:seed" | "admin:seed_tail") && !c.starts_with("blob")), "{mine:?}");
    assert_eq!(a.e.membership().home_gen, home_gen + 1);

    // A device that follows the Fence there and is refused as unauthorized is at a closed board:
    // it stops, connects no more, and is offered to re-pair.
    let code = notes_of(&a, |x| match x {
        crate::Notification::MoveCode { text } => Some(text.clone()),
        _ => None,
    })
    .pop()
    .expect("the move code");
    assert_eq!(type_code(&mut b, &code), Ok(()));
    let i = b.fx.iter().position(|f| matches!(f, Effect::Connect { relay } if relay.host == NOWHERE)).expect("a connect there");
    b.fx.drain(..=i);
    b.feed(Event::RelayRefused { req: None, refusal: refusal(401, "relay/unauthorized") });
    let st = b.e.status();
    let stop = st.stops.iter().find(|s| s.kind == crate::StopKind::Closed).expect("the stop Closed");
    assert_eq!(stop.action, crate::StopAction::RePair);
    b.fx.clear();
    for _ in 0..48 {
        b.tick(3_600_000);
    }
    assert!(b.fx.iter().all(|f| !matches!(f, Effect::Connect { .. })));
}

// spec: ENG-OPS-003
#[test]
fn an_operation_goes_on_from_its_stage_after_a_restart() {
    let (mut net, mut a) = board(241);
    let mut b = pair(&mut net, &mut a, 242, "Laptop", 1);
    let c = pair(&mut net, &mut a, 243, "Phone", 2);
    let d = pair(&mut net, &mut a, 244, "Tablet", 3);
    net.settle(&mut [&mut a, &mut b]);
    let has_admin = |n: &Node| n.fx.iter().any(|f| matches!(f, Effect::Admin { .. }));

    // The stage is durable before its relay call is made.
    a.auto = false;
    a.feed(Event::Op { ticket: 9, op: Operation::Revoke { dev: c.dev(), replace: false } });
    assert!(!has_admin(&a));
    a.auto = true;
    a.persist_all();
    assert!(has_admin(&a));
    // The process dies before the relay answered: after the Start the call is made again.
    let calls = net.calls.len();
    a.fx.clear();
    a.restart(245);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));
    assert_eq!(net.calls[calls..].iter().filter(|x| x.1 == "admin:revoke").count(), 1);
    assert_eq!(status_of(&b, &c), DeviceStatus::Revoked);

    // It dies at stage 2, the rotation enqueued and its frame on the way: the operation still ends.
    let from = a.persists.len();
    a.feed(Event::Op { ticket: 10, op: Operation::Revoke { dev: d.dev(), replace: false } });
    let i = a.fx.iter().position(|f| matches!(f, Effect::Admin { .. })).expect("the revoke");
    let fx = a.fx.remove(i).expect("it");
    net.answer(&mut a, fx);
    assert_eq!(rotations(&a, from).len(), 1);
    let calls = net.calls.len();
    a.fx.clear();
    a.restart(246);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 10), Some(Ok(())));
    assert!(net.calls[calls..].iter().all(|x| x.1 != "admin:revoke"), "stage 1 is not run again");
    assert_eq!(status_of(&b, &d), DeviceStatus::Revoked);
    assert_eq!(status_of(&a, &d), DeviceStatus::Revoked);
}

fn has_alarm(n: &Node, kind: crate::AlarmKind) -> bool {
    n.e.status().alarms.iter().any(|x| x.kind == kind)
}

fn diverged(n: &Node) -> bool {
    n.e.status().stops.iter().any(|s| s.kind == crate::StopKind::Divergence)
}

// spec: ENG-DIVERGE-001 ENG-DIVERGE-002 ENG-DIVERGE-003 ENG-DIVERGE-004
#[test]
fn the_minority_stops_and_adopts_with_a_typed_fingerprint() {
    const DAY: u64 = 86_400_000;
    let (mut net, mut a) = board(251);
    let mut b = pair(&mut net, &mut a, 252, "Laptop", 1);
    let mut c = pair(&mut net, &mut a, 253, "Phone", 2);
    let mut k = 0u64;
    let mut grow = |net: &mut super::rkit_tests::Net, a: &mut Node, b: &mut Node, to: u64| {
        while net.log_len() < to {
            k += 1;
            a.set(k, (k % 40) as u8, k as i64);
            net.settle(&mut [&mut *a, &mut *b]);
        }
    };
    grow(&mut net, &mut a, &mut b, 60);
    net.settle(&mut [&mut a, &mut b, &mut c]);
    let old_membership = a.e.membership_bytes();
    // C's replica goes wrong (a build that computes differently): one value is off. C is away
    // while the other two pass seq 64, write its snapshot and exchange their beats.
    if let Some(state) = c.e.state.as_mut() {
        state.keys.get_mut(&1).expect("key 1").0 += 1;
    }
    c.e.pipe.pre = c.e.state.clone();
    grow(&mut net, &mut a, &mut b, 70);
    for _ in 0..3 {
        a.advance(3_600_000);
        b.advance(3_600_000);
        net.settle(&mut [&mut a, &mut b]);
    }
    a.advance(DAY);
    b.advance(DAY);
    net.settle(&mut [&mut a, &mut b]);
    let writer = net.calls.iter().rev().find(|x| x.1 == "blob:upload:1").map(|x| x.0).expect("the snapshot of 64");
    assert!(!diverged(&a) && !diverged(&b));

    // The rule of §22.2 at A, which holds B's agreeing report at 64. One differing report:
    // two against one, A raises StateMismatch and does not stop.
    let own = a.e.hashes[&64].hash;
    assert!(a.e.registry.reports.iter().any(|r| r.seq == 64 && r.dev == b.dev() && r.hash == own));
    a.e.registry.reports.push(crate::Report { seq: 64, dev: c.dev(), hash: [9; 32], hash_version: 1, report_seq: a.e.b });
    a.e.check_divergence(64);
    assert!(has_alarm(&a, crate::AlarmKind::StateMismatch));
    assert!(!diverged(&a));

    // C comes back, applies seq 64 its own way and reads the others' reports: one against
    // two (at first one against one, a tie): it stops.
    net.settle(&mut [&mut a, &mut b, &mut c]);
    assert!(diverged(&c));
    assert!(!diverged(&a) && !diverged(&b));
    // Under the stop C seals nothing but its beat, which carries its report and no verdict, and
    // takes no other duty; it keeps reading.
    let uploads_by = |net: &super::rkit_tests::Net, n: &Node| net.calls.iter().filter(|x| x.0 == n.dev() && x.1.starts_with("blob:upload")).count();
    let uploads = uploads_by(&net, &c);
    let from = c.persists.len();
    c.set(900, 30, 900);
    grow(&mut net, &mut a, &mut b, 80);
    for _ in 0..3 {
        for n in [&mut a, &mut b, &mut c] {
            n.advance(DAY);
        }
        net.settle(&mut [&mut a, &mut b, &mut c]);
    }
    assert_eq!(uploads_by(&net, &c), uploads);
    let sealed: Vec<_> = puts(&c, from).into_iter().filter(|r| r.frame.is_some()).collect();
    assert!(!sealed.is_empty(), "C's beats go on under the stop");
    assert!(sealed.iter().all(|r| matches!(&r.body, Body::Beat(i) if i.acks.is_empty() && i.state.is_some())));
    assert_eq!(c.e.status().queued, 1);
    assert_eq!(c.e.status().head.seq, net.log_len());
    assert!(diverged(&c));

    // C adopts the snapshot of the device its user names only with that device's fingerprint typed right.
    assert_ne!(writer, c.dev());
    let fp = c.e.status().devices.iter().find(|d| d.dev == writer).expect("its row").key_fp.clone();
    let calls = net.calls.len();
    c.feed(Event::Op { ticket: 7, op: Operation::Adopt { dev: writer, typed_fp: "AAAA-BBBB-CCCC-DDDD".into() } });
    assert!(matches!(op_result(&c, 7), Some(Err(_))));
    assert_eq!(net.calls.len(), calls);
    assert!(c.fx.iter().all(|f| !matches!(f, Effect::Blob { .. })));
    // The other healthy device announces a snapshot whose membership bytes are not the board's
    // membership at its seq (a faulty build): an adoption from it is refused, and C stays stopped.
    {
        use crate::Sealer;
        let host = super::kit_tests::relay().host;
        let y: &mut Node = if writer == a.dev() { &mut b } else { &mut a };
        let mut material = super::rkit_tests::material_of(y);
        let sm = crate::toy::ToyMachine::new(false);
        // It claims seq 64 (the relay takes a snapshot only at an aligned seq).
        material.seq = 64;
        material.chain = y.e.chain_at(64).expect("the chain at 64");
        material.membership = old_membership.clone();
        material.state_hash = crate::state_hash(1, &crate::StateMachine::board_hash(&sm, y.e.confirmed().0), &material.membership);
        let mk = y.e.mk.clone().expect("a membership");
        let blob = y.e.sealer.seal_snapshot(&mk, &material).expect("sealed");
        let info = crate::SnapInfo {
            seq: material.seq,
            hash: material.state_hash,
            hash_version: 1,
            blob: blob.id,
            sha256: blob.sha256,
            size: blob.bytes.len() as u64,
            parts: blob.parts,
            inflated: blob.inflated,
            epoch: blob.epoch,
        };
        let cred = net.cred(&host, &y.dev());
        let at = y.wall;
        let up = net.relay().blob(at, &cred, &BlobRequest::Upload { purpose: 1, seq: material.seq, blob }).result;
        assert!(up.is_ok(), "{up:?}");
        y.e.enqueue_own(Kind::Snap, Body::Snap(info), Via::Append, None).expect("an entry id");
        let out = std::mem::take(&mut y.e.out);
        y.absorb(out);
        y.feed(Event::Tick);
        let (yd, yfp) = (y.dev(), c.e.status().devices.iter().find(|d| d.dev == y.dev()).expect("its row").key_fp.clone());
        net.settle(&mut [&mut a, &mut b, &mut c]);
        c.feed(Event::Op { ticket: 9, op: Operation::Adopt { dev: yd, typed_fp: yfp } });
        net.settle(&mut [&mut a, &mut b, &mut c]);
        assert!(matches!(op_result(&c, 9), Some(Err(_))), "{:?}", op_result(&c, 9));
        assert!(diverged(&c));
        assert_ne!(c.board_bytes(), a.board_bytes());
    }
    c.feed(Event::Op { ticket: 8, op: Operation::Adopt { dev: writer, typed_fp: fp } });
    net.settle(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(op_result(&c, 8), Some(Ok(())));
    assert!(!diverged(&c));
    net.settle(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(c.board_bytes(), a.board_bytes());
    assert_eq!(c.e.status().queued, 0);
    assert_eq!(a.e.confirmed().0.keys[&30].0, 900);
}

// spec: ENG-EVID-004 ENG-EVID-007
#[test]
fn a_relay_rolled_back_gets_its_tail_from_a_devices_log() {
    let (mut net, mut a) = board(261);
    let mut b = pair(&mut net, &mut a, 262, "Laptop", 1);
    for k in 1..=3u64 {
        a.set(k, k as u8, k as i64);
    }
    net.settle(&mut [&mut a, &mut b]);
    // The relay as it is now; what follows it will lose: a pairing and entries by all three.
    let earlier = net.relay().clone();
    let cut = net.log_len();
    let mut c = pair(&mut net, &mut a, 263, "Phone", 2);
    let added_seq = pair_result(&a, 2).expect("an answer").expect("paired").1;
    c.set(10, 10, 10);
    for k in 4..=8u64 {
        a.set(k, k as u8, k as i64);
    }
    b.set(20, 20, 20);
    net.settle(&mut [&mut a, &mut b, &mut c]);
    let full = net.relay().clone();
    let head = net.log_len();
    assert!(added_seq > cut && head > added_seq + 3);

    // A's log lacks one record of the tail: its read is answered Missing, and A sends nothing.
    net.relay().rollback_to(&earlier);
    let lost = a.store.log.remove(&(added_seq + 1)).expect("a record");
    let admins = net.admins.len();
    net.settle(&mut [&mut a]);
    assert!(has_alarm(&a, crate::AlarmKind::RelayBehind));
    // (What lies before the missing record is restored: the request up to the DeviceAdded and its repair.)
    for (dev, op) in &net.admins[admins..] {
        if let (true, AdminRequest::RestoreTail { first_seq, frames, .. }) = (*dev == a.dev(), op) {
            assert!(first_seq + frames.len() as u64 - 1 <= added_seq, "no request holds or passes the missing record");
        }
    }
    assert!(a.e.status().stops.iter().any(|s| matches!(s.kind, crate::StopKind::LocalFault { .. })));
    assert!(net.log_len() <= added_seq);
    a.store.log.insert(added_seq + 1, lost);
    // The relay is rolled back once more for the device that holds everything.
    net.relay().rollback_to(&earlier);

    // B holds the whole tail and restores it: from the relay's head on, the frames as its log
    // holds them, each request ending with the first control entry in it, and that entry's
    // repair (the added device's row) before the next request.
    let (admins, calls) = (net.admins.len(), net.calls.len());
    net.settle(&mut [&mut b]);
    assert_eq!(net.log_len(), head);
    for seq in cut + 1..=head {
        assert_eq!(net.relay().log()[&seq].frame, full.log()[&seq].frame, "{seq}");
    }
    let mine: Vec<(usize, &AdminRequest)> = net.admins[admins..].iter().enumerate().filter(|(_, x)| x.0 == b.dev()).map(|(i, x)| (i + admins, &x.1)).collect();
    let mut next = cut + 1;
    let mut registered_at = None;
    for (i, op) in &mine {
        match op {
            AdminRequest::RestoreTail { first_seq, chain_before, frames } => {
                assert_eq!(*first_seq, next);
                assert_eq!(net.admin_ctx[*i].0 + 1, next, "from the relay's head");
                assert_eq!(*chain_before, full.log()[&(next - 1)].chain);
                assert!(!frames.is_empty() && frames.len() <= 64);
                for (j, f) in frames.iter().enumerate() {
                    assert_eq!(*f, full.log()[&(next + j as u64)].frame);
                }
                let last = next + frames.len() as u64 - 1;
                // No request runs past the DeviceAdded; the next one waits for its repair.
                assert!(!(next <= added_seq && last > added_seq), "{next}..{last}");
                if next > added_seq {
                    assert!(registered_at.is_some(), "the row before the frames of its author");
                }
                next = last + 1;
            }
            AdminRequest::RegisterSeq { seq, dev, .. } => {
                assert_eq!((*seq, *dev), (added_seq, c.dev()));
                assert_eq!(next, added_seq + 1);
                registered_at = Some(*i);
            }
            other => panic!("an unexpected request {other:?}"),
        }
    }
    assert_eq!(next, head + 1);
    assert!(registered_at.is_some());
    assert!(net.calls[calls..].iter().any(|x| x.1 == "admin:restore_tail"));
    // The evidence is resolved; everyone goes on, the device whose row was lost included.
    a.restart(264);
    for n in [&mut a, &mut b, &mut c] {
        n.advance(3_700_000);
    }
    net.settle(&mut [&mut a, &mut b, &mut c]);
    for n in [&a, &b, &c] {
        assert!(n.e.status().stops.is_empty(), "{:?}", n.e.status().stops);
    }
    c.set(30, 30, 30);
    net.settle(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(a.board_bytes(), c.board_bytes());
    assert_eq!(b.e.confirmed().0.keys[&30].0, 30);
}

// spec: ENG-EVID-004
#[test]
fn a_repair_lost_on_the_way_is_sent_again_before_the_next_restore_request() {
    // `gate` 1022: a restore tail stored a `DeviceAdded`; the repair after it (the added device's
    // row) was lost with the connection; after the reconnect the next request carried a frame of
    // that device, and the relay refused it `relay/unknown_author` again and again.
    let (mut net, mut a) = board(281);
    let mut b = pair(&mut net, &mut a, 282, "Laptop", 1);
    a.set(1, 1, 1);
    net.settle(&mut [&mut a, &mut b]);
    let earlier = net.relay().clone();
    let mut c = pair(&mut net, &mut a, 283, "Phone", 2);
    let added_seq = pair_result(&a, 2).expect("an answer").expect("paired").1;
    c.set(10, 10, 10);
    b.set(20, 20, 20);
    net.settle(&mut [&mut a, &mut b, &mut c]);
    let full = net.relay().clone();
    let head = net.log_len();
    assert!(head > added_seq + 1);

    net.relay().rollback_to(&earlier);
    // B restores up to the `DeviceAdded`; the repair it sends next never reaches the relay.
    let mut lost = false;
    'run: for _ in 0..40 {
        while let Some(fx) = b.fx.pop_front() {
            if let Effect::Admin { op: AdminRequest::RegisterSeq { .. }, .. } = &fx {
                b.fx.clear();
                b.feed(Event::Disconnected { unreachable: false, paused_until_ms: None });
                lost = true;
                break 'run;
            }
            net.answer(&mut b, fx);
        }
        b.tick(30_000);
    }
    assert!(lost, "B restored the DeviceAdded and sent its repair");
    assert_eq!(net.log_len(), added_seq);
    let admins = net.admins.len();

    // After the reconnect, the entry's repair goes out again before any frame of its device.
    net.settle(&mut [&mut b]);
    assert_eq!(net.log_len(), head);
    for seq in added_seq..=head {
        assert_eq!(net.relay().log()[&seq].frame, full.log()[&seq].frame, "{seq}");
    }
    let mut registered = false;
    for (i, (dev, op)) in net.admins.iter().enumerate().skip(admins) {
        assert_ne!(net.admin_ctx[i].3, Err("relay/unknown_author".to_owned()), "{op:?}");
        match op {
            AdminRequest::RegisterSeq { seq, dev: added, .. } => {
                assert_eq!((*dev, *seq, *added), (b.dev(), added_seq, c.dev()));
                registered = true;
            }
            AdminRequest::RestoreTail { first_seq, .. } => {
                assert!(registered, "the row before the frames of its author: request from {first_seq}");
            }
            other => panic!("an unexpected request {other:?}"),
        }
    }
    for n in [&mut a, &mut b, &mut c] {
        n.advance(3_700_000);
    }
    net.settle(&mut [&mut a, &mut b, &mut c]);
    for n in [&a, &b, &c] {
        assert!(n.e.status().stops.is_empty(), "{:?}", n.e.status().stops);
    }
    c.set(30, 30, 30);
    net.settle(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(b.e.confirmed().0.keys[&30].0, 30);
}

// spec: ENG-OPS-005
#[test]
fn a_recovery_key_change_is_registered_by_the_engine_only_in_a_repair() {
    use crate::Sealer;
    use crate::sim::relay::Cred;
    let (mut net, mut a) = board(271);
    let mut b = pair(&mut net, &mut a, 272, "Laptop", 1);
    a.set(1, 1, 1);
    net.settle(&mut [&mut a, &mut b]);
    let earlier = net.relay().clone();
    let cut = net.log_len();

    // The recovery tool, holding the sheet, seals a K-shape entry and appends it through the door.
    let m_old = a.e.mk.clone().expect("a membership");
    let old = a.e.sealer().view(&m_old).recovery.last().cloned().expect("the recovery identity");
    let (_, spk, hpk) = crate::fake::FakeSealer::join_keys(279);
    let entry = |id: u8, n: &Node| crate::PlainEntry {
        entry_id: [id; 16],
        at_ms: n.wall,
        based_on: n.e.status().head,
        body: Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None }),
    };
    let e1 = entry(5, &a);
    let change = a.e.sealer.seal_recovery_key_change(&m_old, old.last_ctr + 1, &e1, (spk, hpk)).expect("sealed");
    let at = a.wall;
    assert!(net.relay().append(at, &Cred::Door, std::slice::from_ref(&change.frame)).result.is_ok());
    let k_seq = net.log_len();
    let admins = net.admins.len();
    net.settle(&mut [&mut a, &mut b]);
    for n in [&a, &b] {
        assert!(n.e.status().banners.iter().any(|x| x.kind == crate::BannerKind::RecoveryChanged));
        assert_ne!(n.e.sealer().view(n.e.mk.as_ref().expect("a membership")).recovery.last().map(|r| r.sign_pk), Some(old.sign_pk));
    }
    // A second change signed with the old sheet, which is no longer the recovery identity: refused.
    let (_, spk2, hpk2) = crate::fake::FakeSealer::join_keys(280);
    let e2 = entry(6, &a);
    let stale = a.e.sealer.seal_recovery_key_change(&m_old, old.last_ctr + 2, &e2, (spk2, hpk2)).expect("sealed");
    assert!(net.relay().append(at, &Cred::Door, std::slice::from_ref(&stale.frame)).result.is_ok());
    let refused_seq = net.log_len();
    a.set(2, 2, 2);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(a.e.sealer().view(a.e.mk.as_ref().expect("a membership")).recovery.last().map(|r| r.sign_pk), Some(spk));
    // Reading them, no engine registers anything: that is the sender's call.
    assert!(net.admins[admins..].iter().all(|(_, op)| !matches!(op, AdminRequest::RecoveryRegisterSeq { .. })));

    // The relay loses the tail; the device that restores it repairs the accepted change, by seq,
    // and sends nothing for the refused one.
    let head = net.log_len();
    net.relay().rollback_to(&earlier);
    let admins = net.admins.len();
    net.settle(&mut [&mut b]);
    assert_eq!(net.log_len(), head);
    let repairs: Vec<_> = net.admins[admins..]
        .iter()
        .filter_map(|(_, op)| match op {
            AdminRequest::RecoveryRegisterSeq { seq, sign_pk, frame_hash, .. } => Some((*seq, *sign_pk, *frame_hash)),
            _ => None,
        })
        .collect();
    assert_eq!(repairs, vec![(k_seq, spk, change.frame_hash)]);
    assert!(k_seq > cut && refused_seq > k_seq);
    net.settle(&mut [&mut a, &mut b]);
    assert!(a.e.status().stops.is_empty() && b.e.status().stops.is_empty());
}

// engine.md §11.9, the row "any other".
// Found by the simulator (seed 310 of `gate`): after a whole-request refusal the engine sent
// the same admin request again at once, 736,000 times. §11.9: not before the next welcome.
#[test]
fn a_refused_request_is_not_repeated_before_the_next_welcome() {
    let (mut net, mut a) = board(411);
    let mut b = pair(&mut net, &mut a, 412, "Laptop", 1);
    let c = pair(&mut net, &mut a, 413, "Phone", 2);
    net.settle(&mut [&mut a, &mut b]);
    a.feed(Event::Op { ticket: 9, op: Operation::Revoke { dev: c.dev(), replace: false } });
    let (req, _) = take_admin(&mut a);
    a.feed(Event::RelayRefused { req: Some(req), refusal: refusal(500, "relay/slow") });
    for _ in 0..5 {
        a.tick(1);
    }
    assert!(a.fx.iter().all(|f| !matches!(f, Effect::Admin { .. })), "nothing is sent again yet");
    assert!(has_alarm(&a, crate::AlarmKind::RelayProtocol));
    // The next poll's welcome lets it go on, and the operation ends.
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));
    assert_eq!(status_of(&b, &c), DeviceStatus::Revoked);
}

// spec: ENG-BOOT-005
#[test]
fn a_skip_it_cannot_replay_makes_a_device_wait_for_a_snapshot() {
    let (mut net, mut a) = board(421);
    let mut b = pair(&mut net, &mut a, 422, "Laptop", 1);
    b.set(1, 1, 5);
    net.settle(&mut [&mut a, &mut b]);
    let target = net.log_len();
    // C bootstraps after the target: it holds no state from before it.
    let mut c = pair(&mut net, &mut a, 423, "Phone", 2);
    assert_eq!(c.e.confirmed().0.keys[&1].0, 5);
    a.feed(Event::Op { ticket: 3, op: Operation::Skip { seq: target } });
    net.settle(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(op_result(&a, 3), Some(Ok(())));
    // A and B replayed. C cannot: it does not apply the Skip, stops its board before it and keeps reading.
    assert!(!a.e.confirmed().0.keys.contains_key(&1) && !b.e.confirmed().0.keys.contains_key(&1));
    let st = c.e.status();
    let skip_seq = st
        .stops
        .iter()
        .find_map(|s| match s.kind {
            crate::StopKind::Rebootstrap { after } => Some(after),
            _ => None,
        })
        .expect("the stop rebootstrap");
    assert!(skip_seq > target && skip_seq <= net.log_len());
    assert_eq!(st.board_seq, skip_seq - 1);
    assert_eq!(st.head.seq, net.log_len());
    assert_eq!(c.e.confirmed().0.keys[&1].0, 5);
    // The log passes an aligned seq; one of the others writes its snapshot and the other acknowledges it.
    let mut k = 0u64;
    while net.log_len() < 70 {
        k += 1;
        a.set(10 + k, (2 + k % 40) as u8, k as i64);
        net.settle(&mut [&mut a, &mut b, &mut c]);
    }
    assert_eq!(c.e.status().board_seq, skip_seq - 1);
    for _ in 0..3 {
        for n in [&mut a, &mut b, &mut c] {
            n.advance(86_400_000);
        }
        net.settle(&mut [&mut a, &mut b, &mut c]);
    }
    // C resumed from that snapshot, above the Skip, and applied what follows.
    let st = c.e.status();
    assert!(st.stops.is_empty(), "{:?}", st.stops);
    assert_eq!(st.board_seq, net.log_len());
    assert_eq!(c.board_bytes(), a.board_bytes());
    assert!(!c.e.confirmed().0.keys.contains_key(&1));
    c.set(900, 90, 9);
    net.settle(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(a.e.confirmed().0.keys[&90].0, 9);
}

// spec: ENG-MOVE-006 ENG-MOVE-009 ENG-MOVE-011
#[test]
fn a_move_whose_relay_is_away_for_a_moment_still_fences_it() {
    const NEW: &str = "relay2.test";
    let old = super::kit_tests::relay().host;
    let (mut net, mut a) = board(421);
    let mut b = pair(&mut net, &mut a, 422, "Laptop", 1);
    net.add_relay(NEW);
    // The session with the old relay is lost just as the move begins.
    a.feed(Event::Disconnected { unreachable: false, paused_until_ms: None });
    let calls = net.calls.len();
    a.feed(Event::Op { ticket: 9, op: Operation::Move { host: NEW.to_owned() } });
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));
    let mine: Vec<&str> = net.calls[calls..].iter().filter(|c| c.0 == a.dev()).map(|c| c.1.as_str()).collect();
    let at = |name: &str| mine.iter().position(|c| *c == name).unwrap_or_else(|| panic!("{name} in {mine:?}"));
    assert!(at("admin:fence") < at("admin:seed"), "{mine:?}");
    let f = net.relays[&old].head().expect("a head").seq;
    let reports = notes_of(&a, |x| match x {
        crate::Notification::MoveReport { carried_through, old_relay_answered } => Some((*carried_through, *old_relay_answered)),
        _ => None,
    });
    assert_eq!(reports, vec![(f, true)]);
}

// spec: ENG-SESS-006
#[test]
fn nothing_is_sent_to_a_fenced_relay() {
    const NEW: &str = "relay2.test";
    let old = super::kit_tests::relay().host;
    let (mut net, mut a) = board(431);
    let mut b = pair(&mut net, &mut a, 432, "Laptop", 1);
    let c = pair(&mut net, &mut a, 433, "Phone", 2);
    net.settle(&mut [&mut a, &mut b]);
    net.add_relay(NEW);
    a.feed(Event::Op { ticket: 9, op: Operation::Move { host: NEW.to_owned() } });
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));
    let code = notes_of(&a, |x| match x {
        crate::Notification::MoveCode { text } => Some(text.clone()),
        _ => None,
    })
    .pop()
    .expect("the move code");
    // The other device has not followed yet. What it would send to the fenced relay waits.
    assert_eq!(net.at[&b.dev()], old);
    let calls = net.calls.len();
    b.feed(Event::Op { ticket: 10, op: Operation::Revoke { dev: c.dev(), replace: false } });
    b.set(700, 7, 7);
    for _ in 0..4 {
        b.advance(700_000);
        net.settle(&mut [&mut a, &mut b]);
    }
    let sent: Vec<&str> = net.calls[calls..].iter().filter(|x| x.0 == b.dev()).map(|x| x.1.as_str()).collect();
    assert!(sent.iter().all(|x| !x.starts_with("admin") && *x != "append" && !x.starts_with("blob:upload")), "{sent:?}");
    assert_eq!(op_result(&b, 10), None);
    // Once it follows the move, the operation runs on the new relay.
    assert_eq!(type_code(&mut b, &code), Ok(()));
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(net.at[&b.dev()], NEW);
    assert_eq!(op_result(&b, 10), Some(Ok(())));
    assert_eq!(status_of(&a, &c), DeviceStatus::Revoked);
}

// spec: ENG-OPS-006 ENG-OPS-012
#[test]
fn a_pairing_cut_off_by_a_restart_is_reported_failed() {
    let (mut net, mut a) = board(441);
    net.settle(&mut [&mut a]);
    a.feed(Event::PairRegister { ticket: 5, device: new_device(442, "Laptop") });
    // The upload is answered; the record is enqueued durably, and the process dies before the
    // write that seals it.
    let (i, _) = a.fx.iter().enumerate().find(|(_, f)| matches!(f, Effect::Blob { .. })).expect("the upload");
    let fx = a.fx.remove(i).expect("it");
    a.auto = false;
    net.answer(&mut a, fx);
    assert!(a.persist_one());
    let admins = net.admins.len();
    a.fx.clear();
    a.auto = true;
    a.restart(443);
    net.settle(&mut [&mut a]);
    assert_eq!(pair_result(&a, 5), Some(Err("engine/pair_interrupted")));
    assert_eq!(net.admins.len(), admins, "nothing was registered");
    assert!(a.e.outbox.is_empty());
}

// spec: ENG-MOVE-003 ENG-MOVE-006 ENG-MOVE-010
#[test]
fn a_move_starts_its_seed_above_a_frame_the_relay_made_up() {
    const NEW: &str = "relay2.test";
    let old = super::kit_tests::relay().host;
    let (mut net, mut a) = board(451);
    let mut b = pair(&mut net, &mut a, 452, "Laptop", 1);
    let mut k = 0u64;
    let mut fill = |net: &mut super::rkit_tests::Net, a: &mut Node, b: &mut Node, upto: u64| {
        while net.log_len() < upto {
            k += 1;
            a.set(k, (k % 50) as u8, k as i64);
            net.settle(&mut [a, b]);
        }
        for _ in 0..3 {
            a.advance(86_400_000);
            b.advance(86_400_000);
            net.settle(&mut [a, b]);
        }
    };
    fill(&mut net, &mut a, &mut b, 70);
    // The relay stores a frame nobody signed: a stored frame with its last byte changed.
    let mut frame = net.relays[&old].log().values().last().expect("a frame").frame.clone();
    *frame.last_mut().expect("a byte") ^= 1;
    let now = a.wall;
    let (head, _) = net.relays.get_mut(&old).expect("the relay").inject_frame(now, frame).expect("stored");
    let x = head.seq;
    net.settle(&mut [&mut a, &mut b]);
    assert!(has_alarm(&a, crate::AlarmKind::SkippedFrame));
    net.add_relay(NEW);

    // No snapshot lies above that frame yet: the move ends before anything is fenced.
    let calls = net.calls.len();
    a.feed(Event::Op { ticket: 9, op: Operation::Move { host: NEW.to_owned() } });
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 9), Some(Err("engine/move_tail")));
    assert!(net.calls[calls..].iter().all(|c| c.1 != "admin:fence" && c.1 != "admin:seed"));
    a.set(800, 8, 8);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(a.board_bytes(), b.board_bytes(), "the board works on where it is");

    // Once a snapshot above it is usable, the seed starts right after the frame.
    fill(&mut net, &mut a, &mut b, 140);
    let admins = net.admins.len();
    a.feed(Event::Op { ticket: 10, op: Operation::Move { host: NEW.to_owned() } });
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 10), Some(Ok(())));
    let (first_seq, last_seq) = net.admins[admins..]
        .iter()
        .find_map(|(_, op)| match op {
            AdminRequest::Seed { first_seq, last_seq, .. } => Some((*first_seq, *last_seq)),
            _ => None,
        })
        .expect("the seed");
    let f = net.relays[&old].head().expect("a head").seq;
    assert_eq!((first_seq, last_seq), (x + 1, f));
    for seq in first_seq..=last_seq {
        assert_eq!(net.relays[NEW].log()[&seq].frame, net.relays[&old].log()[&seq].frame, "{seq}");
    }
}

/// Serves a node's effects one by one until the next one is the admin call `name`, which stays unanswered.
fn serve_until_admin(net: &mut super::rkit_tests::Net, n: &mut Node, name: &str) {
    for _ in 0..20_000 {
        match n.fx.front() {
            Some(Effect::Admin { op, .. }) if super::rkit_tests::admin_name(op) == name => return,
            Some(_) => {
                let fx = n.fx.pop_front().expect("an effect");
                net.answer(n, fx);
            }
            None => n.tick(30_000),
        }
    }
    panic!("no {name} call");
}

fn move_reports(n: &Node) -> Vec<(u64, bool)> {
    notes_of(n, |x| match x {
        crate::Notification::MoveReport { carried_through, old_relay_answered } => Some((*carried_through, *old_relay_answered)),
        _ => None,
    })
}

// spec: ENG-MOVE-003 ENG-OPS-003 ENG-MOVE-012
#[test]
fn a_seed_tail_nobody_answered_is_sent_again() {
    const NEW: &str = "relay2.test";
    let (mut net, mut a) = board(451);
    let mut b = pair(&mut net, &mut a, 452, "Laptop", 1);
    a.set(1, 1, 1);
    net.settle(&mut [&mut a, &mut b]);
    net.add_relay(NEW);
    a.feed(Event::Op { ticket: 9, op: Operation::Move { host: NEW.to_owned() } });
    serve_until_admin(&mut net, &mut a, "admin:seed_tail");
    // The request is lost on its way: the new relay stored nothing of the tail.
    let lost = a.fx.pop_front().expect("the tail request");
    assert!(matches!(lost, Effect::Admin { op: AdminRequest::SeedTail { .. }, .. }));
    a.feed(Event::Disconnected { unreachable: false, paused_until_ms: None });
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));
    // The board is open on the new relay, with the whole tail.
    let f = a.e.membership().home_seq;
    assert_eq!(net.relays[NEW].head().expect("a head").seq, f);
    assert_eq!(move_reports(&a), vec![(f, true)]);
}

// spec: ENG-MOVE-006 ENG-OPS-003
#[test]
fn a_move_restarted_behind_its_fence_goes_on_to_the_seed() {
    const NEW: &str = "relay2.test";
    let old = super::kit_tests::relay().host;
    let (mut net, mut a) = board(461);
    let mut b = pair(&mut net, &mut a, 462, "Laptop", 1);
    net.add_relay(NEW);
    a.feed(Event::Op { ticket: 9, op: Operation::Move { host: NEW.to_owned() } });
    // The old relay sequences the Fence; the process dies before it has read it back.
    serve_until_admin(&mut net, &mut a, "admin:fence");
    let fence = a.fx.pop_front().expect("the fence request");
    net.answer(&mut a, fence);
    a.restart(463);
    let calls = net.calls.len();
    let f = net.relays[&old].head().expect("a head").seq;
    // Whatever it asks the new relay for the door is answered only after it has read the Fence.
    let mut held = Vec::new();
    for _ in 0..200 {
        match a.fx.pop_front() {
            Some(fx @ Effect::Admin { op: AdminRequest::Door { .. }, .. }) => held.push(fx),
            Some(fx) => {
                net.answer(&mut a, fx);
            }
            None if a.e.status().board_seq >= f => break,
            None => a.tick(30_000),
        }
    }
    assert_eq!(a.e.status().board_seq, f);
    for fx in held {
        net.answer(&mut a, fx);
    }
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));
    let mine: Vec<&str> = net.calls[calls..].iter().filter(|c| c.0 == a.dev()).map(|c| c.1.as_str()).collect();
    assert!(mine.contains(&"admin:seed") && !mine.contains(&"admin:fence"), "{mine:?}");
    assert_eq!(move_reports(&a), vec![(f, true)]);
}

// spec: ENG-MOVE-003 ENG-OPS-003
#[test]
fn a_move_restarted_before_its_seed_seeds_with_the_fence_generation() {
    const NEW: &str = "relay2.test";
    let old = super::kit_tests::relay().host;
    let (mut net, mut a) = board(471);
    let mut b = pair(&mut net, &mut a, 472, "Laptop", 1);
    net.add_relay(NEW);
    a.feed(Event::Op { ticket: 9, op: Operation::Move { host: NEW.to_owned() } });
    // The Fence is accepted and read; the process dies with the seed not yet sent.
    serve_until_admin(&mut net, &mut a, "admin:seed");
    a.restart(473);
    let f = net.relays[&old].head().expect("a head").seq;
    serve_until_admin(&mut net, &mut a, "admin:seed");
    let Some(Effect::Admin { op: AdminRequest::Seed { home_gen, last_seq, .. }, .. }) = a.fx.front() else { panic!("a seed") };
    assert_eq!((*home_gen, *last_seq), (a.e.membership().home_gen, f));
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));
    assert_eq!(move_reports(&a), vec![(f, true)]);
}

// spec: ENG-MOVE-003 ENG-OPS-003
#[test]
fn a_move_restarted_after_its_whole_tail_does_not_fail_on_the_open_board() {
    const NEW: &str = "relay2.test";
    let old = super::kit_tests::relay().host;
    let (mut net, mut a) = board(481);
    let mut b = pair(&mut net, &mut a, 482, "Laptop", 1);
    net.add_relay(NEW);
    a.feed(Event::Op { ticket: 9, op: Operation::Move { host: NEW.to_owned() } });
    serve_until_admin(&mut net, &mut a, "admin:seed_tail");
    // The tail is stored and the board opens; the process dies before stage 5 is written.
    a.auto = false;
    let tail = a.fx.pop_front().expect("the tail request");
    net.answer(&mut a, tail);
    a.auto = true;
    a.restart(483);
    let calls = net.calls.len();
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));
    let mine: Vec<&str> = net.calls[calls..].iter().filter(|c| c.0 == a.dev()).map(|c| c.1.as_str()).collect();
    assert!(mine.contains(&"admin:seed"), "the seed is made again: {mine:?}");
    let f = net.relays[&old].head().expect("a head").seq;
    assert_eq!(move_reports(&a), vec![(f, true)]);
    assert_eq!(net.at[&a.dev()], NEW);
}

// spec: ENG-MOVE-009 ENG-OPS-003
#[test]
fn a_move_restarted_after_the_switch_reports_once_what_it_carried() {
    const NEW: &str = "relay2.test";
    let old = super::kit_tests::relay().host;
    let (mut net, mut a) = board(491);
    a.set(1, 1, 1);
    net.settle(&mut [&mut a]);
    net.add_relay(NEW);
    let last = net.log_len();
    net.relays.remove(&old).expect("the old relay");
    a.feed(Event::Disconnected { unreachable: true, paused_until_ms: None });
    a.feed(Event::Op { ticket: 9, op: Operation::Move { host: NEW.to_owned() } });
    a.feed(Event::User(crate::UserAction::OldRelayGone));
    // The target is switched; the process dies before the Fence is on the new relay.
    for _ in 0..20_000 {
        if matches!(a.fx.front(), Some(Effect::Connect { relay }) if relay.host == NEW) {
            break;
        }
        match a.fx.pop_front() {
            Some(fx) => {
                net.answer(&mut a, fx);
            }
            None => a.tick(30_000),
        }
    }
    assert!(move_reports(&a).is_empty());
    a.restart(492);
    let calls = net.calls.len();
    net.settle(&mut [&mut a]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));
    assert_eq!(move_reports(&a), vec![(last, false)]);
    assert_eq!(a.e.membership().home_seq, last + 1);
    // The snapshot of the move is sealed again and uploaded (§28.3 stage 6).
    let mine: Vec<&str> = net.calls[calls..].iter().filter(|c| c.0 == a.dev()).map(|c| c.1.as_str()).collect();
    assert!(mine.contains(&"blob:upload:1"), "{mine:?}");
    // A second restart reports nothing again.
    a.restart(493);
    net.settle(&mut [&mut a]);
    assert_eq!(move_reports(&a), vec![(last, false)]);
}

// spec: ENG-MOVE-003 ENG-OPS-003 ENG-MOVE-012
#[test]
fn a_move_whose_last_tail_answer_was_lost_goes_on_to_the_open_board() {
    const NEW: &str = "relay2.test";
    let old = super::kit_tests::relay().host;
    let (mut net, mut a) = board(495);
    let mut b = pair(&mut net, &mut a, 496, "Laptop", 1);
    net.add_relay(NEW);
    a.feed(Event::Op { ticket: 9, op: Operation::Move { host: NEW.to_owned() } });
    serve_until_admin(&mut net, &mut a, "admin:seed_tail");
    // The new relay stores the whole tail and opens the board; its answer never arrives.
    let Some(Effect::Admin { op, .. }) = a.fx.pop_front() else { panic!("the tail request") };
    let cred = net.cred(NEW, &a.dev());
    let now = a.wall;
    assert!(net.relays.get_mut(NEW).expect("the new relay").admin(now, &cred, &a.dev(), &op).result.is_ok());
    a.feed(Event::Disconnected { unreachable: false, paused_until_ms: None });
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));
    let f = net.relays[&old].head().expect("a head").seq;
    assert_eq!(move_reports(&a), vec![(f, true)]);
    assert_eq!(net.at[&a.dev()], NEW);
}

// spec: ENG-MOVE-009 ENG-OPS-003
#[test]
fn a_move_report_written_but_never_shown_is_made_again_after_a_start() {
    const NEW: &str = "relay2.test";
    let old = super::kit_tests::relay().host;
    let (mut net, mut a) = board(497);
    let mut b = pair(&mut net, &mut a, 498, "Laptop", 1);
    net.add_relay(NEW);
    a.feed(Event::Op { ticket: 9, op: Operation::Move { host: NEW.to_owned() } });
    a.auto = false;
    let reported = |ops: &[PersistOp]| {
        ops.iter().any(|o| matches!(o, PersistOp::WriteMeta(m) if m.ops.iter().any(|x| x.ticket == 9 && x.stage == 7)))
    };
    for _ in 0..20_000 {
        if a.pq.front().is_some_and(|w| reported(&w.1)) {
            break;
        }
        if a.persist_one() || a.read_one() {
            continue;
        }
        match a.fx.pop_front() {
            Some(fx) => {
                net.answer(&mut a, fx);
            }
            None => a.tick(30_000),
        }
    }
    // The write that records the report is on the disk; the process dies before it hears so.
    let (_, ops) = a.pq.pop_front().expect("the write of stage 7");
    assert!(reported(&ops));
    for op in &ops {
        a.store.apply(op);
    }
    assert!(move_reports(&a).is_empty());
    a.auto = true;
    a.restart(499);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&a, 9), Some(Ok(())));
    let f = net.relays[&old].head().expect("a head").seq;
    assert_eq!(move_reports(&a), vec![(f, true)]);
    assert!(a.has_note(|x| matches!(x, crate::Notification::MoveCode { .. })));
}

// spec: ENG-OPS-004 ENG-OPS-006 ENG-OPS-013
#[test]
fn a_pairing_found_after_a_start_still_ends_with_a_report() {
    let (mut net, mut a) = board(501);
    let mut b = pair(&mut net, &mut a, 502, "Laptop", 1);
    let d = pair(&mut net, &mut a, 503, "Tablet", 2);
    net.settle(&mut [&mut a, &mut b]);
    let host = super::kit_tests::relay().host;
    // A pairing whose DeviceAdded is sealed and on its way when the process dies.
    let begin = |net: &mut super::rkit_tests::Net, a: &mut Node, ticket: u64, device: crate::NewDevice, reseed: u64| {
        a.feed(Event::PairRegister { ticket, device });
        let (req, op) = take_blob(a);
        let cred = net.cred(&host, &a.dev());
        let result = net.relay().blob(a.wall, &cred, &op).result;
        a.feed(Event::BlobResult { req, result });
        let (_, register) = take_admin(a);
        assert!(matches!(register, AdminRequest::Register { .. }));
        a.restart(reseed);
    };
    let failed = |n: &Node, t: u64| {
        n.notes.iter().find_map(|x| match x {
            crate::Notification::PairFailed { ticket, code } if *ticket == t => Some(*code),
            _ => None,
        })
    };

    // The relay refuses the register that is sent again: dropped, and reported in the same call.
    begin(&mut net, &mut a, 10, new_device(510, "One"), 504);
    serve_until_admin(&mut net, &mut a, "admin:register");
    let (req, _) = take_admin(&mut a);
    a.feed(Event::AdminResult { req, result: Err(refusal(403, "relay/forbidden")) });
    assert_eq!(failed(&a, 10), Some("relay/forbidden"));
    assert!(a.e.outbox.is_empty());
    net.settle(&mut [&mut a, &mut b]);

    // The relay stores it and the entry is accepted: the device is a member, and the pairing
    // nobody waits for any more is reported interrupted.
    let two = new_device(511, "Two");
    begin(&mut net, &mut a, 11, two.clone(), 505);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(failed(&a, 11), Some("engine/pair_interrupted"));
    assert!(a.e.status().devices.iter().any(|r| r.dev == two.dev && r.status == DeviceStatus::Current));

    // The epoch moves before the relay stores it: the entry is refused in the log, the device
    // is revoked at the relay, then the failure is reported (§28.4 step 4).
    let three = new_device(512, "Three");
    begin(&mut net, &mut a, 12, three.clone(), 506);
    serve_until_admin(&mut net, &mut a, "admin:register");
    let (req, register) = take_admin(&mut a);
    b.feed(Event::Op { ticket: 21, op: Operation::Revoke { dev: d.dev(), replace: false } });
    net.settle(&mut [&mut b]);
    assert_eq!(op_result(&b, 21), Some(Ok(())));
    let cred = net.cred(&host, &a.dev());
    let reply = net.relay().admin(a.wall, &cred, &a.dev(), &register);
    assert!(reply.result.is_ok());
    a.feed(Event::AdminResult { req, result: reply.result });
    let calls = net.calls.len();
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(failed(&a, 12), Some("seal/stale_epoch"));
    assert!(net.calls[calls..].iter().any(|c| c.0 == a.dev() && c.1 == "admin:revoke"));
    assert!(a.e.status().devices.iter().all(|r| r.dev != three.dev || r.status != DeviceStatus::Current));
    assert!(a.e.outbox.is_empty());
}

// spec: ENG-CLASS-002 ENG-PERSIST-006
#[test]
fn the_alarm_of_a_made_up_frame_is_durable_before_a_checkpoint_above_it() {
    let old = super::kit_tests::relay().host;
    let (mut net, mut a) = board(521);
    let mut b = pair(&mut net, &mut a, 522, "Laptop", 1);
    net.settle(&mut [&mut a, &mut b]);
    // B is away. The relay stores a frame nobody signed, and the log goes on past an aligned seq.
    let mut frame = net.relays[&old].log().values().last().expect("a frame").frame.clone();
    *frame.last_mut().expect("a byte") ^= 1;
    let now = a.wall;
    let (head, _) = net.relays.get_mut(&old).expect("the relay").inject_frame(now, frame).expect("stored");
    let x = head.seq;
    let stride = super::lim::STRIDE;
    assert!(x < stride);
    let mut k = 0u64;
    while net.log_len() < stride + 3 {
        k += 1;
        a.set(k, (k % 50) as u8, k as i64);
        net.settle(&mut [&mut a]);
    }
    // B reads it all, its writes confirmed one by one. Nothing at or below a checkpoint is
    // classified again after a `Start`, so the frame's alarm must reach the disk before any
    // checkpoint above the frame can: it is the first write after the page is durable, ahead of
    // everything the board writes for the page's entries.
    b.auto = false;
    b.tick(700_000);
    let holds = |ops: &[PersistOp]| ops.iter().any(|o| matches!(o, PersistOp::AppendLog(v) if v.iter().any(|r| r.seq == x)));
    for _ in 0..20_000 {
        if b.pq.front().is_some_and(|w| holds(&w.1)) {
            break;
        }
        if b.persist_one() || b.read_one() {
            continue;
        }
        match b.fx.pop_front() {
            Some(fx) => {
                net.answer(&mut b, fx);
            }
            None => b.tick(30_000),
        }
    }
    assert!(b.persist_one(), "the page with the frame");
    let next = b.pq.front().expect("a write after the page");
    let alarmed = |m: &crate::Meta| m.alarms.iter().any(|al| al.kind == crate::AlarmKind::SkippedFrame && al.seq == Some(x));
    assert!(
        matches!(&next.1[..], [PersistOp::WriteMeta(m)] if alarmed(m)),
        "the first write after the page carries the alarm: {:?}",
        next.1.iter().map(|o| format!("{o:?}").chars().take(16).collect::<String>()).collect::<Vec<_>>()
    );
    let mut above = 0;
    for _ in 0..20_000 {
        if b.store.ckpt.as_ref().is_some_and(|c| c.board_seq >= x) {
            above += 1;
            let alarms = b.store.meta.as_ref().map(|m| m.alarms.clone()).unwrap_or_default();
            assert!(
                alarms.iter().any(|al| al.kind == crate::AlarmKind::SkippedFrame && al.seq == Some(x)),
                "a checkpoint at {:?} is durable and the alarm of the frame at {x} is not",
                b.store.ckpt.as_ref().map(|c| c.board_seq)
            );
        }
        if b.persist_one() || b.read_one() {
            continue;
        }
        match b.fx.pop_front() {
            Some(fx) => {
                net.answer(&mut b, fx);
            }
            None if above > 0 => break,
            None => b.tick(30_000),
        }
    }
    assert!(above > 0, "a checkpoint above the frame");
    b.auto = true;
    b.restart(523);
    net.settle(&mut [&mut a, &mut b]);
    assert!(b.e.meta.alarms.iter().any(|al| al.kind == crate::AlarmKind::SkippedFrame && al.seq == Some(x)));
    assert_eq!(b.board_bytes(), a.board_bytes());
}

// spec: ENG-RECON-007 ENG-HALT-006
#[test]
fn a_halted_device_fills_the_counter_gap_below_its_skip() {
    let (mut net, mut a) = board(531);
    let mut b = pair(&mut net, &mut a, 532, "Laptop", 1);
    net.settle(&mut [&mut a, &mut b]);
    // B seals an edit; its append is lost on the way, and nothing answers it yet.
    b.set(1, 1, 5);
    let i = b.fx.iter().position(|f| matches!(f, Effect::Append { .. })).expect("the append");
    b.fx.remove(i);
    // A's poison entry lands; B reads it and crash-halts with its sealed edit still unsent.
    super::flow_tests::poison(&mut a);
    net.settle(&mut [&mut a]);
    let p = net.log_len();
    net.settle(&mut [&mut b]);
    assert_eq!(b.e.status().halt.map(|h| h.seq), Some(p));
    // The session drops: the append is known lost, and B connects again, halted.
    b.feed(Event::Disconnected { unreachable: false, paused_until_ms: None });
    net.settle(&mut [&mut b]);
    assert_eq!(net.log_len(), p, "a halted device sends no edit by itself");
    // B skips the poison. The Skip's frame has the counter after the edit's, so the relay
    // answers `gap` until the edit is there: B sends the edit again, then the Skip.
    let calls = net.calls.len();
    b.feed(Event::Op { ticket: 7, op: Operation::Skip { seq: p } });
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(op_result(&b, 7), Some(Ok(())));
    let appends = net.calls[calls..].iter().filter(|c| c.0 == b.dev() && c.1 == "append").count();
    assert!(appends <= 4, "{appends} appends");
    for n in [&a, &b] {
        assert!(n.e.status().halt.is_none());
        assert_eq!(n.e.status().board_seq, net.log_len());
    }
    assert_eq!(a.board_bytes(), b.board_bytes());
    assert_eq!(a.e.confirmed().0.keys[&1].0, 5);
}

// spec: ENG-DIVERGE-001 ENG-DIVERGE-002 ENG-DIVERGE-005
#[test]
fn healthy_devices_that_heard_the_wrong_one_first_go_on_together() {
    let (mut net, mut a) = board(561);
    let mut b = pair(&mut net, &mut a, 562, "Laptop", 1);
    let mut c = pair(&mut net, &mut a, 563, "Phone", 2);
    let mut k = 0u64;
    while net.log_len() < 55 {
        k += 1;
        a.set(k, (k % 20) as u8, k as i64);
        net.settle(&mut [&mut a, &mut b, &mut c]);
    }
    // C's replica goes wrong (a build that computes differently): key 1 is off by one, and
    // nothing writes key 1 again.
    if let Some(state) = c.e.state.as_mut() {
        state.keys.get_mut(&1).expect("key 1").0 += 1;
    }
    c.e.pipe.pre = c.e.state.clone();
    // C goes on alone past seq 64 and writes the snapshot there, with its own hash.
    while net.log_len() < 70 {
        k += 1;
        c.set(k, 30, k as i64);
        net.settle(&mut [&mut c]);
    }
    for _ in 0..3 {
        c.advance(3_600_000);
        net.settle(&mut [&mut c]);
    }
    assert!(net.calls.iter().any(|x| x.0 == c.dev() && x.1 == "blob:upload:1"), "C wrote the snapshot of 64");
    // A reads C's snapshot entry before any agreeing report: one against one, a tie: A stops.
    let from_a = a.persists.len();
    a.advance(600_000);
    net.settle(&mut [&mut a]);
    assert!(diverged(&a), "a tie stops A");
    // On entering the stop A sent its own report at once, without verdicts. (Under the rule as it
    // was, A sent nothing, B then read C's entry alone and stopped too, and the two healthy
    // devices waited for each other for good.)
    let beats: Vec<_> = puts(&a, from_a).into_iter().filter(|r| r.kind == Kind::Beat && r.frame.is_some()).collect();
    assert!(beats.iter().any(|r| matches!(&r.body, Body::Beat(i) if i.acks.is_empty() && i.state.map(|s| s.0) == Some(64))));
    // B reads both: two against one, it does not stop; its beat, with its verdict on C's snapshot,
    // ends A's stop at 64, the seq where it began.
    b.advance(600_000);
    net.settle(&mut [&mut b]);
    assert!(!diverged(&b), "the majority goes on");
    a.advance(600_000);
    net.settle(&mut [&mut a]);
    assert!(!diverged(&a), "the stop ends at the seq where it began");
    net.settle(&mut [&mut a, &mut b, &mut c]);
    assert!(!diverged(&a) && !diverged(&b), "the healthy majority goes on");
    assert!(diverged(&c), "the minority stays stopped");
    a.set(900, 31, 900);
    net.settle(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(a.e.status().queued, 0);
    assert_eq!(a.board_bytes(), b.board_bytes());
}

// spec: ENG-SESS-007
#[test]
fn a_relay_without_a_row_for_the_device_closes_it_on_a_board_never_moved() {
    let host = super::kit_tests::relay().host;
    let (mut net, mut a) = board(571);
    net.settle(&mut [&mut a]);
    let before = net.relays[&host].clone();
    let mut b = pair(&mut net, &mut a, 572, "Laptop", 1);
    net.settle(&mut [&mut a, &mut b]);
    assert_eq!(b.e.membership().home_seq, 0, "the board never moved");
    // The relay goes back to a state that never had B (as a relay copy that paired B and was
    // thrown away): B's next sign-in is refused `401 relay/unauthorized`.
    net.relays.get_mut(&host).expect("the relay").rollback_to(&before);
    b.feed(Event::Disconnected { unreachable: false, paused_until_ms: None });
    net.settle(&mut [&mut b]);
    let stops = b.e.status().stops;
    assert!(stops.iter().any(|s| s.kind == crate::StopKind::Closed && s.action == crate::StopAction::RePair), "{stops:?}");
    // No `Connect` while the stop is in force.
    let dev = b.dev();
    let connects = |net: &super::rkit_tests::Net| net.calls.iter().filter(|x| x.0 == dev && x.1 == "connect").count();
    let n = connects(&net);
    for _ in 0..4 {
        b.advance(3_600_000);
        net.settle(&mut [&mut b]);
    }
    assert_eq!(connects(&net), n);
}
