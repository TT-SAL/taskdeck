//! Tests of sessions, send and receive against the scripted sealer: the
//! evidence stop and its end, second and displaced, budgets and refused frames,
//! echoes, a tampered page, a restored copy, the epoch gate, a revoked device.

use super::script_tests::{ME, OTHER, Plan, SNode, ScriptM};
use crate::toy::ToyCommand;
use crate::{
    AdminOk, AdminRequest, AlarmKind, AppendResult, Body, CtlIntent, Effect, Event, Head, Kind, Notification,
    RelayRefusal, Sealer, SessionState, StatusWarning, StopAction, StopKind, UserAction, Via,
};

fn node() -> SNode {
    SNode::new(10, ScriptM { my_ctr: 3, other_ctr: 1, ..Default::default() }, vec![])
}

fn live() -> SNode {
    let mut n = node();
    let head = Head { seq: 10, chain: n.chain_b };
    n.live(head);
    n
}

fn set(n: &mut SNode, ticket: u64, key: u8) {
    n.feed(Event::Enqueue {
        ticket,
        command: ToyCommand::Set { key, value: 1 },
        at_ms: n.wall,
        origin_state: vec![0; 8],
        hold: false,
    });
}

fn other_frame(n: &mut SNode, ctr: u64, key: u8) -> Vec<u8> {
    let f = n.s.frame(OTHER, ctr);
    n.s.plan(&f, Plan::accept_cmd(OTHER, ctr, vec![1, key, 0, 0, 0, 0, 0, 0, 0, 1]));
    f
}

fn hellos(n: &SNode) -> usize {
    n.fx.iter().filter(|f| matches!(f, Effect::Hello)).count()
}

fn appends(n: &SNode) -> usize {
    n.fx.iter().filter(|f| matches!(f, Effect::Append { .. })).count()
}

// spec: ENG-SESS-010 ENG-EVID-003 ENG-EVID-005 ENG-RECV-001 ENG-UI-002
#[test]
fn an_evidence_stop_holds_sending_and_reading_and_ends_with_a_hello() {
    let mut n = node();
    n.feed(Event::Connected { push: true });
    // The relay's head is behind this device's log.
    let w = n.welcome(Head { seq: 8, chain: [8; 32] });
    n.feed(Event::Welcome(w));
    let st = n.e.status();
    assert_eq!(st.session, SessionState::Connecting, "a failed check leaves the session where it was");
    assert_eq!(st.stops.len(), 1);
    assert_eq!((st.stops[0].kind.clone(), st.stops[0].action), (StopKind::Evidence, StopAction::RestoreTail));
    assert_eq!(st.alarms[0].kind, AlarmKind::RelayBehind);
    // Editing goes on; nothing is sent, nothing from the relay is persisted.
    n.fx.clear();
    n.notes.clear();
    set(&mut n, 1, 6);
    assert!(n.notes.iter().any(|x| matches!(x, Notification::Enqueued { ticket: 1, .. })));
    assert_eq!(appends(&n), 0);
    let f = other_frame(&mut n, 2, 7);
    n.page(11, &[f]);
    assert!(!n.logged(11));
    assert!(!n.fx.iter().any(|x| matches!(x, Effect::Read { .. })));
    // An alarm behind a stop cannot be dismissed.
    let id = st.alarms[0].id;
    n.feed(Event::User(UserAction::DismissAlarm { id }));
    assert_eq!(n.e.status().alarms.len(), 1);
    // The relay holds the log head again: the stop ends, and the session says hello at once.
    n.fx.clear();
    let w = n.welcome(Head { seq: 10, chain: n.chain_b });
    n.feed(Event::Welcome(w));
    assert!(n.e.status().stops.is_empty());
    assert!(n.e.status().alarms.is_empty());
    assert_eq!(hellos(&n), 1, "Hello at once when the stop ends while Connecting");
    let w = n.welcome(Head { seq: 10, chain: n.chain_b });
    n.feed(Event::Welcome(w));
    assert_eq!(n.e.status().session, SessionState::Live);
    assert_eq!(appends(&n), 1, "the queued edit leaves once the session is Live");
}

// spec: ENG-SESS-003 ENG-SESS-004
#[test]
fn a_second_holder_of_the_identity_stops_the_engine_until_the_user_chooses() {
    let mut n = live();
    // The previous process of this device was still connected: ignored, once.
    n.e.prev_session = Some([4; 16]);
    n.feed(Event::Second { session: [4; 16] });
    assert!(n.e.status().stops.is_empty());
    n.fx.clear();
    n.feed(Event::Displaced { session: [5; 16] });
    let st = n.e.status();
    assert_eq!(st.stops[0].kind, StopKind::IdentityClone);
    assert_eq!(st.stops[0].action, StopAction::UseThisCopy);
    assert_eq!(st.session, SessionState::Offline);
    assert!(n.fx.iter().any(|f| matches!(f, Effect::Disconnect)));
    // No Connect until the user chooses this copy.
    n.fx.clear();
    n.mono += 8_000_000;
    n.wall += 8_000_000;
    n.feed(Event::Tick);
    assert!(!n.fx.iter().any(|f| matches!(f, Effect::Connect { .. })));
    n.feed(Event::User(UserAction::UseThisCopy));
    assert!(n.fx.iter().any(|f| matches!(f, Effect::Connect { .. })));
    // A second process that is not the previous one stops the engine as well.
    let mut n = live();
    n.feed(Event::Second { session: [6; 16] });
    assert_eq!(n.e.status().stops[0].kind, StopKind::IdentityClone);
}

// spec: ENG-RESULT-002 ENG-RESULT-003 ENG-SEND-003
#[test]
fn a_budget_pauses_appends_and_a_refused_own_frame_stops_without_changing_it() {
    let mut n = live();
    set(&mut n, 1, 6);
    let (req, frames) = n.appended().unwrap();
    n.feed(Event::AppendResults { req, results: vec![AppendResult::Refused { status: 429, code: "relay/budget".into() }] });
    let midnight = (n.wall / 86_400_000 + 1) * 86_400_000;
    assert_eq!(n.e.status().paused_until_ms, Some(midnight));
    n.mono += 3_600_000;
    n.wall += 3_600_000;
    n.feed(Event::Tick);
    assert_eq!(appends(&n), 0, "no Append until the next 00:00 UTC");
    n.mono += (midnight - n.wall) as u64 + 1;
    n.wall = midnight + 1;
    n.feed(Event::Tick);
    let (req, again) = n.appended().expect("the frame is resent after the pause");
    assert_eq!(again, frames, "identical bytes on every retry");
    // The relay refuses the frame for its signature: the engine stops and keeps the frame.
    n.feed(Event::AppendResults {
        req,
        results: vec![AppendResult::Refused { status: 400, code: "relay/bad_signature".into() }],
    });
    let st = n.e.status();
    assert!(st.stops.iter().any(|s| matches!(&s.kind, StopKind::LocalFault { code } if code == "engine/own_frame_refused")));
    assert_eq!(n.e.outbox[0].r.frame.as_ref(), Some(&frames[0]));
    n.feed(Event::Tick);
    assert_eq!(appends(&n), 0);
}

// spec: ENG-ECHO-001
#[test]
fn an_acknowledged_entry_must_come_back_at_its_seq_with_its_chain() {
    // The chain the relay acknowledged is not the chain of the frame it serves.
    let mut n = live();
    set(&mut n, 1, 6);
    let (req, frames) = n.appended().unwrap();
    n.feed(Event::AppendResults { req, results: vec![AppendResult::New { seq: 11, chain: [0xAB; 32] }] });
    n.page(11, &frames);
    let st = n.e.status();
    assert_eq!(st.alarms[0].kind, AlarmKind::Unechoed);
    assert_eq!(st.alarms[0].seq, Some(11));
    assert_eq!(st.stops[0].kind, StopKind::Evidence);
    // The relay serves another frame at the acknowledged seq.
    let mut n = live();
    set(&mut n, 1, 6);
    let (req, frames) = n.appended().unwrap();
    let chain = n.s.chain_step(&n.chain_b, 11, &frames[0]);
    n.feed(Event::AppendResults { req, results: vec![AppendResult::New { seq: 11, chain }] });
    let f = other_frame(&mut n, 2, 7);
    n.page(11, &[f]);
    assert_eq!(n.e.status().alarms[0].kind, AlarmKind::Unechoed);
    // An honest echo raises nothing, and the record leaves when its entry is applied.
    let mut n = live();
    set(&mut n, 1, 6);
    let (req, frames) = n.appended().unwrap();
    let chain = n.s.chain_step(&n.chain_b, 11, &frames[0]);
    n.feed(Event::AppendResults { req, results: vec![AppendResult::New { seq: 11, chain }] });
    n.page(11, &frames);
    assert!(n.e.status().alarms.is_empty());
    assert!(n.e.outbox.is_empty());
}

// spec: ENG-ECHO-002 ENG-SEND-005
#[test]
fn a_record_carried_by_an_admin_operation_goes_alone_and_its_late_answer_is_checked() {
    let mut n = live();
    set(&mut n, 1, 6);
    let (req, frames) = n.appended().unwrap();
    let via = Via::Fence { host: "new.test".into(), door: [2; 5], home_gen: 2 };
    let intent = CtlIntent::Fence { host: "new.test".into(), door: [2; 5], home_gen: 2 };
    n.e.enqueue_own(Kind::Ctl, Body::Ctl(intent), via, None).unwrap();
    let out = std::mem::take(&mut n.e.out);
    n.absorb(out);
    n.feed(Event::Tick);
    assert!(!n.fx.iter().any(|f| matches!(f, Effect::Admin { .. })), "not while an earlier record is unacknowledged");
    let chain = n.s.chain_step(&n.chain_b, 11, &frames[0]);
    n.feed(Event::AppendResults { req, results: vec![AppendResult::New { seq: 11, chain }] });
    let admin = n.fx.iter().find_map(|f| match f {
        Effect::Admin { req, op: AdminRequest::Fence { frame: Some(f), .. } } => Some((*req, f.clone())),
        _ => None,
    });
    let (areq, fence) = admin.expect("the Fence travels in its admin operation, alone");
    assert_eq!(appends(&n), 0);
    // The push of both entries arrives before the admin answer: the record is applied and dropped.
    n.page(11, &[frames[0].clone(), fence]);
    assert!(n.e.outbox.is_empty());
    assert!(n.e.status().alarms.is_empty());
    // The answer names a chain the log does not hold at that seq.
    let ok = AdminOk {
        seq: Some(12),
        chain: Some([0xCD; 32]),
        token: false,
        deleted: None,
        floor: None,
        more: None,
        purge_floor: None,
        done: None,
        head: None,
        door: None,
    };
    n.feed(Event::AdminResult { req: areq, result: Ok(ok) });
    assert_eq!(n.e.status().alarms[0].kind, AlarmKind::Unechoed);
}

// spec: ENG-RECV-003 ENG-CLASS-003
#[test]
fn nothing_from_the_first_bad_entry_of_a_page_on_is_persisted() {
    let mut n = live();
    let f1 = other_frame(&mut n, 2, 7);
    let f2 = n.s.frame(OTHER, 9);
    let mut p = Plan::accept_cmd(OTHER, 9, vec![]);
    p.class = crate::Class::Tamper;
    p.code = Some("seal/counter_gap");
    n.s.plan(&f2, p);
    let f3 = other_frame(&mut n, 3, 8);
    n.page(11, &[f1, f2, f3]);
    // The evidence stop raised by the dry run persists nothing of the page, the entry before the
    // cut included (§12.3 step 6, ENG-CLASS-003, ENG-EVID-003).
    assert!(!n.logged(11) && !n.logged(12) && !n.logged(13));
    let st = n.e.status();
    assert_eq!((st.head.seq, st.board_seq), (10, 10));
    assert_eq!(st.alarms[0].kind, AlarmKind::Counter);
    assert_eq!(st.alarms[0].code.as_deref(), Some("seal/counter_gap"));
    assert_eq!(st.stops[0].action, StopAction::ReHome);
}

// spec: ENG-RECON-002 ENG-RECON-003 ENG-SEALER-001 ENG-SEALER-002 ENG-SEALER-006 ENG-UI-004
#[test]
fn a_restored_copy_catches_up_before_it_sends_and_drops_what_was_already_sent() {
    let mut n = node();
    set(&mut n, 1, 6); // queued by this copy, never sealed here
    let xid = n.e.outbox[0].r.entry_id;
    // The relay holds two frames of this device that this folder never sealed (counters 4 and 5).
    let fa = n.s.frame(ME, 4);
    let mut pa = Plan::accept_cmd(ME, 4, vec![1, 6, 0, 0, 0, 0, 0, 0, 0, 1]);
    pa.entry_id = xid; // the same entry, sealed by the earlier copy
    n.s.plan(&fa, pa);
    let fb = n.s.frame(ME, 5);
    let mut pb = Plan::accept_cmd(ME, 5, vec![1, 9, 0, 0, 0, 0, 0, 0, 0, 1]);
    pb.entry_id = [0x55; 16];
    n.s.plan(&fb, pb);
    let c11 = n.s.chain_step(&n.chain_b, 11, &fa);
    let c12 = n.s.chain_step(&c11, 12, &fb);
    n.feed(Event::Connected { push: true });
    let mut w = n.welcome(Head { seq: 12, chain: c12 });
    w.last_ctr = Some(5);
    w.last_seq = Some(12);
    n.fx.clear();
    n.feed(Event::Welcome(w));
    assert_eq!(n.e.registry.restored, Some((4, 5)));
    assert_eq!(n.e.status().session, SessionState::Reconciling);
    assert_eq!(appends(&n), 0, "nothing is sealed before the log reaches last_seq");
    let read = n.fx.iter().find_map(|f| match f {
        Effect::Read { req, after: 10, .. } => Some(*req),
        _ => None,
    });
    assert!(read.is_some());
    n.page_for(read, 11, &[fa.clone(), fb.clone()]);
    // The engine supplied the frames of the restored range as its own, with the chain values it holds.
    let probes = n.s.0.borrow().probes.clone();
    assert_eq!(probes.len(), 2);
    assert_eq!(probes[0].own_sealed, Some(n.s.frame_hash(&fa)));
    assert_eq!(probes[1].own_sealed, Some(n.s.frame_hash(&fb)));
    assert!(probes.iter().all(|p| p.retained_from == 10 && p.chain_prev == Some(p.prev_chain) && p.chain_below.is_none()));
    assert_eq!((probes[0].prev_chain, probes[1].prev_chain), (n.chain_b, c11));
    // The record that the earlier copy had sent is dropped as already sent.
    assert!(n.e.outbox.is_empty());
    assert_eq!(appends(&n), 0);
    assert_eq!(n.e.registry.restored, None);
    assert_eq!(n.e.status().warnings, vec![StatusWarning::RestoredCopy { entries: 1 }]);
    assert_eq!(n.e.status().session, SessionState::Live);
    assert!(n.e.status().alarms.is_empty());
    // New frames are sealed above the relay's counter.
    set(&mut n, 2, 7);
    assert_eq!(n.e.outbox[0].r.ctr, Some(6));
    n.feed(Event::User(UserAction::AckRestoredCopy));
    assert!(n.e.status().warnings.is_empty());
}

// spec: ENG-EPOCH-001
#[test]
fn nothing_new_is_sealed_while_an_older_epoch_frame_is_unseen() {
    let mut n = live();
    set(&mut n, 1, 6);
    let (req, frames) = n.appended().unwrap();
    // A rotation by another device is classified before this device's frame is seen.
    let rot = n.s.frame(OTHER, 2);
    let mut p = Plan::accept_cmd(OTHER, 2, vec![]);
    p.kind = Some(Kind::Ctl);
    p.rotate = true;
    n.s.plan(&rot, p);
    n.page(11, &[rot]);
    assert_eq!(n.e.membership().epoch, 2);
    set(&mut n, 2, 7);
    assert!(n.e.outbox[1].r.frame.is_none(), "no new seal while the epoch-1 frame is unseen");
    // The old-epoch frame comes back in the log: the gate opens.
    let c11 = n.e.chain_at(11).unwrap();
    let chain = n.s.chain_step(&c11, 12, &frames[0]);
    n.feed(Event::AppendResults { req, results: vec![AppendResult::New { seq: 12, chain }] });
    n.page(12, &frames);
    assert_eq!(n.e.outbox.len(), 1);
    assert_eq!(n.e.outbox[0].r.epoch, Some(2), "sealed under the new epoch once the gate opened");
}

// spec: ENG-EPOCH-002 ENG-EPOCH-004
#[test]
fn a_device_the_relay_calls_revoked_keeps_everything_and_reconnects_once_an_hour() {
    let mut n = live();
    set(&mut n, 1, 6);
    let (req, frames) = n.appended().unwrap();
    n.feed(Event::AppendResults { req, results: vec![AppendResult::Refused { status: 403, code: "relay/revoked".into() }] });
    let st = n.e.status();
    assert_eq!(st.stops[0].kind, StopKind::Revoked { by_log: false });
    assert_eq!(st.stops[0].action, StopAction::RePair);
    let before = n.persists.len();
    n.feed(Event::RelayRefused {
        req: None,
        refusal: RelayRefusal { status: 403, code: "relay/revoked".into(), until: None, head: None, index: None },
    });
    n.feed(Event::Disconnected { unreachable: false, paused_until_ms: None });
    n.fx.clear();
    // Editing still works (the relay's word alone does not refuse an Enqueue); nothing is sent.
    set(&mut n, 2, 7);
    assert_eq!(n.e.outbox.len(), 2);
    assert_eq!(n.e.outbox[0].r.frame.as_ref(), Some(&frames[0]), "the sealed frame is kept");
    n.mono += 3_599_000;
    n.wall += 3_599_000;
    n.feed(Event::Tick);
    assert!(!n.fx.iter().any(|f| matches!(f, Effect::Connect { .. })), "at most one Connect per hour");
    n.mono += 2_000;
    n.wall += 2_000;
    n.feed(Event::Tick);
    assert!(n.fx.iter().any(|f| matches!(f, Effect::Connect { .. })));
    assert!(!n.fx.iter().any(|f| matches!(f, Effect::Append { .. } | Effect::Admin { .. } | Effect::Blob { .. })));
    // Nothing was deleted on the relay's word.
    for ops in &n.persists[before..] {
        for op in ops {
            assert!(!matches!(op, crate::PersistOp::TruncateLogBelow(..) | crate::PersistOp::DeleteBases(_) | crate::PersistOp::WriteIdentity(_)));
        }
    }
    // The relay answers again and the log holds this device as current: the claim is over.
    n.feed(Event::Connected { push: true });
    let w = n.welcome(Head { seq: 10, chain: n.chain_b });
    n.feed(Event::Welcome(w));
    assert!(n.e.status().stops.is_empty());
}
