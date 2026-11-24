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
