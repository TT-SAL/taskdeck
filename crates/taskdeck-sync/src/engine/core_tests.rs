//! Unit tests of the engine's first steps: start, records, enqueue, the pending
//! view, the naming hold, send, receive and apply, with the scripted driver of
//! kit_tests.rs.

use super::kit_tests::{Hub, creator, join};
use crate::toy::ToyCommand;
use crate::{Effect, Event, Notification, OutboxOp, PersistOp, Sealer, SessionState};

// spec: ENG-GEN-001 ENG-MODEL-001 ENG-BOOT-004 ENG-PEND-003 ENG-BEAT-003 ENG-EFFECT-002
#[test]
fn genesis_start_builds_the_board() {
    let (n, _f1) = creator(1);
    let st = n.e.status();
    assert_eq!(st.board_seq, 1);
    assert_eq!(st.head.seq, 1);
    assert_eq!(n.e.view_version(), 1);
    assert_eq!(n.e.device_no(), Some(1));
    // AppendLog[1], a checkpoint and a base at 1, in one write.
    let first = &n.persists[0];
    assert!(matches!(first[0], PersistOp::AppendLog(ref v) if v.len() == 1 && v[0].seq == 1));
    assert!(matches!(first[1], PersistOp::WriteCheckpoint(ref c) if c.board_seq == 1));
    assert!(matches!(first[2], PersistOp::WriteBase(ref c) if c.board_seq == 1));
    assert_eq!(n.store.meta.as_ref().map(|m| m.last_beat_ms), Some(0));
    assert!(n.fx.iter().any(|f| matches!(f, Effect::Connect { .. })));
    assert!(st.stops.is_empty() && st.halt.is_none());
}

// spec: ENG-BOOT-004
#[test]
fn a_genesis_whose_board_does_not_match_is_refused() {
    let (mut n, _) = creator(2);
    // The same frame with another board: the item's state hash no longer matches.
    let mut other = crate::toy::ToyMachine::new(false);
    let _ = &mut other;
    n.store = Default::default();
    if let Some(crate::Bootstrap::Genesis { board, .. }) = n.bootstrap.as_mut() {
        // rules 1, one key: a valid toy board that is not the genesis board
        *board = hex("544f593100000001000101000000000000000500000000000000020001000000000000");
    }
    n.start();
    let st = n.e.status();
    assert!(st.stops.iter().any(|s| matches!(&s.kind, crate::StopKind::LocalFault { code } if code == "engine/bad_genesis")));
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

// spec: ENG-ENQ-003 ENG-ENQ-004 ENG-ENQ-005 ENG-PEND-001 ENG-STATE-006 ENG-PERSIST-002
#[test]
fn an_enqueue_is_durable_before_it_is_announced() {
    let (mut n, _) = creator(3);
    n.auto = false;
    n.notes.clear();
    n.set(7, 5, 42);
    assert!(!n.has_note(|x| matches!(x, Notification::Enqueued { .. })), "announced before the write");
    assert_eq!(n.e.pending().len(), 1);
    assert_eq!(n.e.pending()[0].provisional_seq, 2);
    assert_eq!(n.e.view().keys.get(&5).map(|k| k.0), Some(42));
    let (state, _) = n.e.confirmed();
    assert!(state.keys.is_empty(), "the confirmed state is untouched");
    n.persist_all();
    assert!(n.has_note(
        |x| matches!(x, Notification::Enqueued { ticket: 7, entry_id: Some(_), provisional_seq: Some(2) })
    ));
    let put = n.store.outbox.iter().find_map(|o| match o {
        OutboxOp::Put(r) => Some(r.clone()),
        _ => None,
    });
    let r = put.expect("the record is in the outbox file");
    assert_eq!((r.order, r.based_on.seq, r.at_ms), (1, 1, n.wall));
    n.set(8, 6, 1);
    n.persist_all();
    assert_eq!(n.e.pending()[1].provisional_seq, 3);
}

// spec: ENG-ENQ-001 ENG-ENQ-002 ENG-ENQ-006 ENG-ENQ-007 ENG-PEND-002 ENG-PEND-004
#[test]
fn enqueue_refusals_queue_nothing() {
    let (mut n, _) = creator(4);
    n.notes.clear();
    n.enqueue(1, ToyCommand::Fail { code: 1 });
    n.enqueue(2, ToyCommand::Panic);
    n.enqueue(3, ToyCommand::Add { key: 9, delta: 1 });
    let at_ms = n.wall + 60_001;
    n.feed(Event::Enqueue {
        ticket: 4,
        command: ToyCommand::Set { key: 1, value: 1 },
        at_ms,
        origin_state: vec![],
        hold: false,
    });
    let codes: Vec<(u64, &str)> = n
        .notes
        .iter()
        .filter_map(|x| match x {
            Notification::EnqueueRefused { ticket, code } => Some((*ticket, *code)),
            _ => None,
        })
        .collect();
    assert_eq!(
        codes,
        vec![(1, "engine/toy_fail"), (2, "engine/trial_panic"), (3, "engine/toy_gone"), (4, "engine/runtime_protocol")]
    );
    assert!(n.e.pending().is_empty());
    assert!(!n.e.status().queue_large);
}

// spec: ENG-HOLD-001 ENG-HOLD-002 ENG-HOLD-003 ENG-HOLD-004 ENG-SEND-002
#[test]
fn the_naming_hold_merges_cancels_and_ends() {
    let (mut n, f1) = creator(5);
    let mut hub = Hub::with_genesis(&n, &f1);
    hub.settle(&mut [&mut n]);
    assert_eq!(n.e.status().session, SessionState::Live);
    let id = (1u64 << 40) + 1;
    let at_ms = n.wall;
    let create = |hold| Event::Enqueue {
        ticket: 1,
        command: ToyCommand::Create { id, name: 0 },
        at_ms,
        origin_state: 2u64.to_be_bytes().to_vec(),
        hold,
    };
    n.notes.clear();
    let base = hub.log.len();
    n.feed(create(true));
    hub.serve(&mut n);
    assert_eq!(hub.log.len(), base, "a held record is not sealed");
    // A rename of the held item merges into the create, keeping its entry id.
    n.enqueue(2, ToyCommand::Name { id, name: 77 });
    let ids: Vec<[u8; 16]> = n
        .notes
        .iter()
        .filter_map(|x| match x {
            Notification::Enqueued { entry_id: Some(e), .. } => Some(*e),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], ids[1]);
    assert_eq!(n.e.pending().len(), 1);
    assert_eq!(n.e.view().items.get(&id), Some(&77));
    // The hold ends at its 30,000 ms: the merged create leaves as one entry.
    n.tick(30_000);
    hub.settle(&mut [&mut n]);
    assert_eq!(hub.log.len(), base + 1);
    assert_eq!(n.e.confirmed().0.items.get(&id), Some(&77));
    // A create that is forgotten while held never leaves the device.
    let id2 = (1u64 << 40) + 2;
    n.notes.clear();
    let at_ms = n.wall;
    n.feed(Event::Enqueue {
        ticket: 3,
        command: ToyCommand::Create { id: id2, name: 0 },
        at_ms,
        origin_state: 3u64.to_be_bytes().to_vec(),
        hold: true,
    });
    n.enqueue(4, ToyCommand::Forget { id: id2 });
    assert!(n.has_note(|x| matches!(x, Notification::Enqueued { ticket: 4, entry_id: None, provisional_seq: None })));
    assert!(n.e.pending().is_empty());
    hub.settle(&mut [&mut n]);
    assert_eq!(hub.log.len(), base + 1);
    // Any other enqueue ends the hold.
    let at_ms = n.wall;
    n.feed(Event::Enqueue {
        ticket: 5,
        command: ToyCommand::Create { id: id2 + 1, name: 0 },
        at_ms,
        origin_state: 4u64.to_be_bytes().to_vec(),
        hold: true,
    });
    n.set(6, 1, 1);
    hub.settle(&mut [&mut n]);
    assert_eq!(hub.log.len(), base + 3);
}

// spec: ENG-SEND-001 ENG-SEND-003 ENG-SEND-004 ENG-SEND-006 ENG-SEND-008 ENG-RESULT-001 ENG-STATE-001
// spec: ENG-RECV-001 ENG-RECV-002 ENG-RECV-005 ENG-RECV-006 ENG-POISON-001 ENG-POISON-005 ENG-SESS-001 ENG-SESS-009
#[test]
fn own_entries_are_sent_acknowledged_echoed_and_applied() {
    let (mut n, f1) = creator(6);
    let mut hub = Hub::with_genesis(&n, &f1);
    hub.settle(&mut [&mut n]);
    for k in 0..20u8 {
        n.set(k as u64, k, k as i64);
    }
    // Twenty records leave in batches of at most sixteen frames, in order.
    let appends: Vec<usize> = n
        .fx
        .iter()
        .filter_map(|f| match f {
            Effect::Append { frames, .. } => Some(frames.len()),
            _ => None,
        })
        .collect();
    assert_eq!(appends, vec![1], "one append at a time; the first went with the first enqueue");
    hub.settle(&mut [&mut n]);
    // 20 commands and the first beat of a new replica.
    assert_eq!(hub.log.len(), 22);
    let (state, head) = n.e.confirmed();
    assert_eq!(head.seq, 22);
    assert_eq!(state.keys.len(), 20);
    assert!(n.e.pending().is_empty());
    assert_eq!(n.e.status().queued, 0);
    assert!(n.store.marker.is_none(), "the marker is cleared after the apply");
    assert_eq!(n.store.log.len(), 22);
    // Counters rise by one with the order of the records.
    let ctrs: Vec<u64> = hub.log.iter().skip(1).map(|(_, f)| n.e.sealer().frame_author(f).unwrap().1).collect();
    assert_eq!(ctrs, (1..=21).collect::<Vec<u64>>());
    // The outbox file, replayed, is empty: every record was dropped after its entry was applied
    // (and the file was rewritten once it held more than 2 x live + 64 ops, §7.13).
    let mut live: Vec<[u8; 16]> = Vec::new();
    for o in &n.store.outbox {
        match o {
            OutboxOp::Put(r) => {
                if !live.contains(&r.entry_id) {
                    live.push(r.entry_id)
                }
            }
            OutboxOp::Drop(id) => live.retain(|x| x != id),
        }
    }
    assert!(live.is_empty());
    assert!(n.persists.iter().any(|p| matches!(p[0], PersistOp::RewriteOutbox(_))));
    assert!(n.e.status().alarms.is_empty());
}

// spec: ENG-STATE-002 ENG-STATE-003 ENG-STATE-005 ENG-MODEL-002
#[test]
fn a_restart_rebuilds_the_same_replica() {
    let (mut n, f1) = creator(7);
    let mut hub = Hub::with_genesis(&n, &f1);
    hub.settle(&mut [&mut n]);
    for k in 0..5u8 {
        n.set(k as u64, k, 10 + k as i64);
    }
    hub.settle(&mut [&mut n]);
    n.set(99, 200, 1); // queued, then the process dies before it is sent
    let before = (n.board_bytes(), n.view_bytes(), n.e.status().head);
    let attempt = n.store.meta.as_ref().unwrap().attempt;
    n.fx.clear();
    n.restart(8);
    assert_eq!((n.board_bytes(), n.view_bytes(), n.e.status().head), before);
    assert!(n.e.view_version() <= 2, "the counter starts again at every Start");
    assert_eq!(n.e.pending().len(), 1);
    assert_eq!(n.store.meta.as_ref().unwrap().attempt, attempt + 1);
    hub.settle(&mut [&mut n]);
    assert_eq!(n.e.confirmed().0.keys.get(&200).map(|k| k.0), Some(1));
    assert!(n.e.status().alarms.is_empty());
}

// spec: ENG-BOOT-003 ENG-BOOT-006 ENG-HASH-003 ENG-SM-001 ENG-RECV-004 ENG-SEALER-001 ENG-SEALER-006 ENG-BANNER-001
#[test]
fn two_devices_converge() {
    let (mut a, f1) = creator(8);
    let mut hub = Hub::with_genesis(&a, &f1);
    let mut b = join(&mut hub, &mut a, 9, "Laptop");
    assert_eq!(b.e.device_no(), Some(2));
    // The join wrote the checkpoint and the base in one write, then the truncation.
    let i = b.persists.iter().position(|p| matches!(p[0], PersistOp::WriteCheckpoint(_))).unwrap();
    assert!(matches!(b.persists[i][1], PersistOp::WriteBase(_)));
    assert!(b.persists[i + 1..].iter().any(|p| matches!(p[0], PersistOp::TruncateLogBelow(_, Some(_)))));
    assert!(b.store.hashes.iter().all(|h| h.seq > b.store.log_floor - 1));
    // The new device shows a banner about its own addition; the author does not.
    assert_eq!(b.e.status().banners.len(), 1);
    assert!(a.e.status().banners.is_empty());
    for k in 0..10u8 {
        a.set(k as u64, k, k as i64);
        b.set(k as u64, 100 + k, k as i64);
    }
    a.set(50, 7, 700);
    b.set(51, 7, 701);
    hub.settle(&mut [&mut a, &mut b]);
    assert_eq!(a.board_bytes(), b.board_bytes());
    assert_eq!(a.e.status().fingerprint, b.e.status().fingerprint);
    assert_eq!(a.e.confirmed().0.keys.len(), 20);
    assert!(a.e.status().alarms.is_empty() && b.e.status().alarms.is_empty());
    assert_eq!(a.e.status().queued + b.e.status().queued, 0);
}
