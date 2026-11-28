//! The test that loads `engine-reconcile.json` (§10.3, §11.2, §11.3, §11.4):
//! welcomes, conflicts and gaps against the scripted sealer.

use std::collections::VecDeque;

use serde_json::Value;

use super::outbox::Rec;
use super::script_tests::{ME, OTHER, Plan, SNode, ScriptM};
use super::vectors_tests::{cases, hex};
use crate::toy::ToyCommand;
use crate::{
    AlarmKind, AppendResult, Body, Effect, Event, Evidence, ForkSource, Head, Kind, OutboxRecord, SessionState,
    StopKind, Via, Welcome,
};

fn h32(v: &Value) -> [u8; 32] {
    let b = hex(v.as_str().unwrap());
    let mut a = [0u8; 32];
    a.copy_from_slice(&b);
    a
}

/// An outbox record as the vectors describe one: a counter, a frame or none, an acknowledgement or none.
fn record(n: &mut SNode, order: u64, ctr: Option<u64>, frame: bool, ack: Option<(u64, [u8; 32])>) -> [u8; 16] {
    let mut id = [0xA0u8; 16];
    id[15] = order as u8;
    let f = frame.then(|| {
        let f = n.s.frame(ME, ctr.unwrap_or(0));
        let mut p = Plan::accept_cmd(ME, ctr.unwrap_or(0), vec![1, 6, 0, 0, 0, 0, 0, 0, 0, 1]);
        p.entry_id = id;
        n.s.plan(&f, p);
        f
    });
    let r = OutboxRecord {
        entry_id: id,
        order,
        at_ms: n.wall,
        based_on: Head { seq: 1, chain: [0; 32] },
        kind: Kind::Cmd,
        body: Body::Cmd(vec![1, 6, 0, 0, 0, 0, 0, 0, 0, 1]),
        origin_state: vec![],
        via: Via::Append,
        hold: false,
        ctr: if frame { ctr } else { None },
        frame_hash: f.as_ref().map(|f| crate::Sealer::frame_hash(&n.s, f)),
        frame: f,
        epoch: frame.then_some(1),
        ack,
        seen: None,
        op: None,
        reseals: 0,
    };
    n.e.outbox.push(Rec::new(r, Some(ToyCommand::Set { key: 6, value: 1 })));
    id
}

// spec: ENG-SESS-002 ENG-SESS-008 ENG-RECON-001 ENG-RECON-004 ENG-RECON-008 ENG-RECON-009 ENG-SEND-006
#[test]
fn engine_reconcile_welcome_vectors() {
    let all = cases(include_str!("../../tests/vectors/engine-reconcile.json"));
    let mut count = 0;
    for c in all.iter().filter(|c| c["kind"] == "welcome") {
        count += 1;
        let name = c["name"].as_str().unwrap();
        let l = c["L"].as_u64().unwrap();
        let a = if c["has_record"].as_bool().unwrap_or(true) { c["a"].as_u64().unwrap() } else { 0 };
        let w = &c["welcome"];
        let head = Head { seq: w["head"]["seq"].as_u64().unwrap(), chain: h32(&w["head"]["chain"]) };
        let (r, q) = (w["last_ctr"].as_u64(), w["last_seq"].as_u64());
        let mut n = SNode::new(l, ScriptM { my_ctr: a, other_ctr: 1, ..Default::default() }, vec![]);
        // The chain values this device holds, from the lowest seq the case names to L.
        let chains = c["local_chains"].as_object().unwrap();
        let lo = chains.keys().map(|k| k.parse::<u64>().unwrap()).min().unwrap_or(l).min(l);
        let mut idx = VecDeque::new();
        for s in lo..=l {
            let v = chains.get(&s.to_string()).map(h32).unwrap_or([s as u8; 32]);
            idx.push_back((v, 0));
        }
        n.e.chain_first = lo;
        n.e.retained = lo;
        n.e.cb = idx.back().unwrap().0;
        n.e.chains = idx;
        let mut unsealed = Vec::new();
        for (i, rec) in c["records"].as_array().unwrap().iter().enumerate() {
            let ack = rec["ack"].as_array().map(|x| (x[0].as_u64().unwrap(), h32(&x[1])));
            let framed = rec["frame"].as_bool().unwrap();
            let id = record(&mut n, i as u64 + 1, rec["ctr"].as_u64(), framed, ack);
            if !framed {
                unsealed.push(id);
            }
        }
        for kind in c["unresolved_alarms"].as_array().unwrap() {
            let (k, ev) = match kind.as_str().unwrap() {
                "RelayBehind" => (
                    AlarmKind::RelayBehind,
                    Evidence::RelayBehind {
                        relay_head: Head { seq: 1, chain: [0; 32] },
                        relay_last_ctr: None,
                        local_head: Head { seq: 2, chain: [0; 32] },
                        ack: None,
                    },
                ),
                _ => (
                    AlarmKind::Fork,
                    Evidence::Fork { local_chain: [1; 32], relay_chain: [2; 32], source: ForkSource::Welcome },
                ),
            };
            n.e.raise_evidence(k, None, None, None, ev);
        }
        let before: Vec<u64> = n.e.meta.alarms.iter().map(|x| x.id).collect();
        let hh = n.e.sealed_high().max(a);
        assert_eq!(hh, c["h"].as_u64().unwrap(), "{name}: h");
        n.feed(Event::Connected { push: true });
        n.feed(Event::Welcome(Welcome {
            head,
            last_ctr: r,
            last_seq: q,
            relay_now: n.wall,
            floor: 1,
            purge_floor: 0,
            fenced: false,
            fence: None,
            appends_left: None,
            push: true,
        }));
        // The alarms this welcome raised: at most one per kind, none of an unresolved kind.
        let raised: Vec<_> = n.e.meta.alarms.iter().filter(|x| !before.contains(&x.id)).cloned().collect();
        let mut kinds: Vec<String> = raised.iter().map(|x| format!("{:?}", x.kind)).collect();
        kinds.sort();
        let mut want: Vec<String> =
            c["alarms"].as_array().unwrap().iter().map(|x| x["kind"].as_str().unwrap().to_owned()).collect();
        want.sort();
        assert_eq!(kinds, want, "{name}: alarms");
        for x in c["alarms"].as_array().unwrap() {
            let al = raised.iter().find(|y| format!("{:?}", y.kind) == x["kind"].as_str().unwrap()).unwrap();
            match &al.evidence {
                Evidence::RelayBehind { relay_head, relay_last_ctr, local_head, ack } => {
                    assert_eq!(*relay_head, head, "{name}");
                    assert_eq!(*relay_last_ctr, r, "{name}");
                    assert_eq!(local_head.seq, l, "{name}");
                    if let Some(want) = x.get("ack") {
                        assert_eq!(ack.map(|k| k.0), want.as_array().map(|k| k[0].as_u64().unwrap()), "{name}: ack");
                    }
                }
                Evidence::Fork { local_chain, relay_chain, source } => {
                    assert_eq!(al.seq, Some(head.seq), "{name}");
                    assert_eq!(*relay_chain, head.chain, "{name}");
                    assert_eq!(Some(*local_chain), chains.get(&head.seq.to_string()).map(h32), "{name}");
                    assert_eq!(*source, ForkSource::Welcome, "{name}");
                }
                other => panic!("{name}: {other:?}"),
            }
        }
        let full = c["reconcile"] == "full";
        assert_eq!(n.e.sess.relay_head.is_some(), full, "{name}: reconcile {}", c["reconcile"]);
        let restored = c["restored"].as_array().map(|x| (x[0].as_u64().unwrap(), x[1].as_u64().unwrap()));
        assert_eq!(n.e.registry.restored, restored, "{name}: restored");
        if full && q.unwrap_or(0) > l && c["seal_before_q"] == false {
            assert!(!n.e.may_send(), "{name}: nothing is sealed before the log reaches q");
        }
        // Once the log has reached q.
        if let (true, Some(a_after)) = (full, c.get("a_after").and_then(Value::as_u64)) {
            let q = q.unwrap_or(0).max(l);
            while n.e.l < q {
                let s = n.e.l + 1;
                n.e.push_chain(s, [s as u8; 32], 0);
                n.e.l = s;
                n.e.k = s;
            }
            n.e.mk = Some(ScriptM { my_ctr: a_after, other_ctr: 1, ..Default::default() });
            n.e.refresh_vk();
            n.e.recon_progress();
            let withheld = n.e.meta.alarms.iter().any(|x| x.kind == AlarmKind::Withheld);
            assert_eq!(withheld, c["withheld"].as_bool().unwrap_or(false), "{name}: withheld");
            if let (false, Some(next)) = (withheld, c.get("next_ctr").and_then(Value::as_u64)) {
                // A Live engine has already sealed its unsealed records: the first counter it
                // used after the reconcile is the one the case expects.
                let used = n.e.outbox.iter().filter(|r| unsealed.contains(&r.r.entry_id)).filter_map(|r| r.r.ctr).min();
                assert_eq!(used.unwrap_or_else(|| n.e.next_ctr()), next, "{name}: next_ctr");
            }
        }
    }
    assert!(count >= 12);
}

// spec: ENG-RECON-005 ENG-RECON-006 ENG-RECON-003 ENG-SESS-003 ENG-SEALER-005
#[test]
fn engine_reconcile_conflict_vectors() {
    let all = cases(include_str!("../../tests/vectors/engine-reconcile.json"));
    let mut count = 0;
    for c in all.iter().filter(|c| c["kind"] == "conflict") {
        count += 1;
        let name = c["name"].as_str().unwrap();
        let x = &c["expected"];
        let mut n = SNode::new(10, ScriptM { my_ctr: 3, other_ctr: 1, ..Default::default() }, vec![]);
        let head = Head { seq: 10, chain: n.chain_b };
        n.live(head);
        for (t, key) in [(1u64, 6u8), (2, 7)] {
            n.feed(Event::Enqueue {
                ticket: t,
                command: ToyCommand::Set { key, value: 1 },
                at_ms: n.wall,
                origin_state: vec![0; 8],
                hold: false,
            });
        }
        // The first enqueue was sent alone; its answer is the conflict.
        let (req, frames) = n.appended().expect("the first record was sent");
        assert_eq!(frames.len(), 1);
        let ids: Vec<[u8; 16]> = n.e.outbox.iter().map(|r| r.r.entry_id).collect();
        let (xid, yid) = (ids[0], ids[1]);
        let old_frame = frames[0].clone();
        n.e.meta.conflicts = c["conflicts_before"].as_u64().unwrap() as u32;
        let seq = c["seq"].as_u64().unwrap();
        let stored = n.s.frame(ME, 4);
        let mut plan = Plan::accept_cmd(ME, 4, vec![]);
        match c["open_own"].as_str().unwrap_or("none") {
            "x" => plan.entry_id = xid,
            "other" | "y" => plan.entry_id = yid,
            "none" => plan.entry_id = [0x77; 16],
            _ => plan.author = OTHER, // `open_own` fails
        }
        n.s.plan(&stored, plan);
        let frame = if seq == 0 { Vec::new() } else { stored };
        n.fx.clear();
        n.feed(Event::AppendResults { req, results: vec![AppendResult::Conflict { seq, frame }] });
        let rec = |n: &SNode, id: &[u8; 16]| n.e.rec_index(id).map(|i| n.e.outbox[i].r.clone()).unwrap();
        match x["acknowledged"].as_str() {
            Some("x") => assert_eq!(rec(&n, &xid).ack.map(|a| a.0), Some(seq), "{name}: x acknowledged"),
            Some(_) => assert_eq!(rec(&n, &yid).ack.map(|a| a.0), Some(seq), "{name}: y acknowledged"),
            None => assert!(rec(&n, &xid).ack.is_none() && rec(&n, &yid).ack.is_none(), "{name}: none acknowledged"),
        }
        if let Some(cleared) = x.get("cleared") {
            let xr = rec(&n, &xid);
            match cleared.as_str() {
                Some(_) => assert!(xr.frame.as_ref() != Some(&old_frame), "{name}: x cleared for re-sealing"),
                None => assert_eq!(xr.frame.as_ref(), Some(&old_frame), "{name}: x keeps its frame"),
            }
        }
        let alarm = n.e.meta.alarms.iter().find(|a| matches!(a.kind, AlarmKind::RestoredCopy | AlarmKind::RelayProtocol));
        assert_eq!(alarm.map(|a| format!("{:?}", a.kind)), x["alarm"].as_str().map(str::to_owned), "{name}: alarm");
        assert_eq!(u64::from(n.e.meta.conflicts), x["conflicts_after"].as_u64().unwrap(), "{name}: conflicts");
        let clone = n.e.meta.stops.iter().any(|s| s.kind == StopKind::IdentityClone);
        assert_eq!(clone, x["identity_clone"].as_bool().unwrap(), "{name}: identity_clone");
        if x["rehello"] == true && !clone {
            let hello = n.fx.iter().any(|f| matches!(f, Effect::Hello));
            assert!(hello || n.e.sess.need_hello, "{name}: a new reconcile before anything is sealed");
            assert!(!n.e.may_send(), "{name}: nothing is sealed before the new welcome");
        }
        if clone {
            assert_eq!(n.e.sess.state, SessionState::Offline, "{name}");
        }
    }
    assert!(count >= 8);
}

// spec: ENG-RECON-007 ENG-SEND-003 ENG-SEND-007
#[test]
fn engine_reconcile_gap_vectors() {
    let all = cases(include_str!("../../tests/vectors/engine-reconcile.json"));
    let mut count = 0;
    for c in all.iter().filter(|c| c["kind"] == "gap") {
        count += 1;
        let name = c["name"].as_str().unwrap();
        let recs = c["records"].as_array().unwrap();
        let low = recs.iter().filter_map(|r| r["ctr"].as_u64()).min().unwrap_or(1);
        let r = c["last_ctr"].as_u64().unwrap();
        let mut n = SNode::new(10, ScriptM { my_ctr: (low - 1).min(r), other_ctr: 1, ..Default::default() }, vec![]);
        let head = Head { seq: 10, chain: n.chain_b };
        n.live(head);
        let mut unacked = Vec::new();
        for (i, rec) in recs.iter().enumerate() {
            let ctr = rec["ctr"].as_u64().unwrap();
            let ack = rec["ack"].as_bool().unwrap().then_some((100 + ctr, [ctr as u8; 32]));
            let id = record(&mut n, i as u64 + 1, Some(ctr), true, ack);
            if ack.is_none() {
                unacked.push((id, ctr));
            }
        }
        // An append of the unacknowledged frames is outstanding; its answer is the gap.
        let ids: Vec<[u8; 16]> = unacked.iter().map(|u| u.0).collect();
        n.e.send.append = Some((999, ids));
        n.fx.clear();
        n.feed(Event::AppendResults {
            req: 999,
            results: vec![AppendResult::Gap { last_ctr: r, last_seq: c["last_seq"].as_u64().unwrap() }],
        });
        let x = &c["expected"];
        let behind = n.e.meta.alarms.iter().any(|a| a.kind == AlarmKind::RelayBehind);
        assert_eq!(behind, x["relay_behind"].as_bool().unwrap(), "{name}: relay_behind");
        if let Some(from) = x.get("resend_from_ctr").and_then(Value::as_u64) {
            // The next loop resends, identical, from the record with that counter.
            n.feed(Event::Tick);
            let sent = n.appended().map(|(_, frames)| frames).unwrap_or_default();
            let first = sent.first().and_then(|f| crate::Sealer::frame_author(&n.s, f).ok()).map(|a| a.1);
            let expect = unacked.iter().map(|u| u.1).find(|ctr| *ctr >= from);
            assert_eq!(first, expect, "{name}: resend from the counter after last_ctr");
        }
    }
    assert!(count >= 4);
}
