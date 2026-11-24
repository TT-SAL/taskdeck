//! Flows over the tests' hub with the fake sealer and the toy: a crash halt with
//! read-ahead and a Skip, a Skip of an applied entry with its replay, checkpoints,
//! bases and shutdown.

use super::kit_tests::{Hub, Node, creator, join};
use crate::toy::{ToyCommand, ToyMachine};
use crate::{
    Body, Event, Kind, NewDevice, Notification, Operation, PersistOp, StateMachine, UserAction, Via,
};

/// A poison entry: the device seals the toy's `Panic` without the trial apply of
/// §9.1 (a build with a fault would; an honest engine refuses it at `Enqueue`).
fn poison(n: &mut Node) {
    let payload = ToyMachine::new(false).encode(&ToyCommand::Panic);
    n.e.enqueue_own(Kind::Cmd, Body::Cmd(payload), Via::Append, None).expect("an entry id");
    let out = std::mem::take(&mut n.e.out);
    n.absorb(out);
    n.feed(Event::Tick);
}

fn op_result(n: &Node, t: u64) -> Option<Result<(), &'static str>> {
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

// spec: ENG-SKIP-002 ENG-REF-004 ENG-REF-001 ENG-REF-003
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
