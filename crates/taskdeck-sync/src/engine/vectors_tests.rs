//! The tests that load the vector families whose expected values task B derived
//! from the spec (engine.md §35): hash, clock, duties, pending. Where the engine
//! and a vector differ, the spec decides. A field a case leaves out is not
//! checked (tests/vectors/README.md).

use serde_json::Value;

use super::duty::{beat_due, clock_state, duty_order, rank_of, snapshot_due, usable};
use super::outbox::Rec;
use super::{Config, Engine, lim};
use crate::fake::FakeSealer;
use crate::toy::ToyMachine;
use crate::{
    Body, DeviceStatus, DeviceView, Head, Kind, MembershipView, OutboxRecord, SnapInfo, SnapRecord, StateMachine,
    Verdict, Via, state_hash,
};

pub(crate) fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex")).collect()
}

fn arr<const N: usize>(s: &str) -> [u8; N] {
    let v = hex(s);
    let mut a = [0u8; N];
    a.copy_from_slice(&v);
    a
}

pub(crate) fn cases(text: &str) -> Vec<Value> {
    let v: Value = serde_json::from_str(text).expect("the vector file parses");
    v["cases"].as_array().expect("cases").clone()
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or_else(|| panic!("{k} in {v}"))
}

fn i(v: &Value, k: &str) -> i64 {
    v[k].as_i64().unwrap_or_else(|| panic!("{k} in {v}"))
}

fn u(v: &Value, k: &str) -> u64 {
    v[k].as_u64().unwrap_or_else(|| panic!("{k} in {v}"))
}

// spec: ENG-HASH-001
#[test]
fn engine_hash_vectors() {
    let all = cases(include_str!("../../tests/vectors/engine-hash.json"));
    assert!(all.len() >= 6);
    for c in &all {
        let board_hash: [u8; 32] = arr(s(c, "board_hash"));
        let membership = hex(s(c, "membership"));
        let got = state_hash(u(c, "hash_version") as u32, &board_hash, &membership);
        assert_eq!(got.to_vec(), hex(s(c, "state_hash")), "{}", s(c, "name"));
        assert_eq!(hex(s(c, "input")).len(), 86);
        if let Some(board) = c.get("board").and_then(Value::as_str) {
            use sha2::{Digest, Sha256};
            let h: [u8; 32] = Sha256::digest(hex(board)).into();
            assert_eq!(h, board_hash, "{}", s(c, "name"));
        }
    }
}

// spec: ENG-CLOCK-001 ENG-CLOCK-002 ENG-CLOCK-003
#[test]
fn engine_clock_vectors() {
    let all = cases(include_str!("../../tests/vectors/engine-clock.json"));
    assert!(all.len() >= 20);
    for c in &all {
        // The latest measurement counts (§27.1).
        let m = c["measurements"].as_array().expect("measurements").last().map(|m| {
            (i(m, "wall_ms") - i(m, "relay_now"), u(m, "mono_ms"))
        });
        let conf = c["confirmation"].as_array().map(|a| (a[0].as_i64().unwrap(), a[1].as_i64().unwrap())).or_else(|| {
            c["confirmation"].as_object().map(|o| {
                let get = |ks: [&str; 2]| ks.iter().find_map(|k| o.get(*k)).and_then(Value::as_i64).expect("confirmation field");
                (get(["wall_ms", "c_wall"]), get(["skew_ms", "c_skew"]))
            })
        });
        let now = &c["now"];
        let (skew, warning, blocked) = clock_state(m, conf, i(now, "wall_ms"), u(now, "mono_ms"));
        let name = s(c, "name");
        if c.get("skew_ms").is_some() {
            assert_eq!(skew, c["skew_ms"].as_i64(), "{name}: skew");
        }
        if let Some(w) = c.get("warning").and_then(Value::as_bool) {
            assert_eq!(warning, w, "{name}: warning");
        }
        if let Some(b) = c.get("blocked").and_then(Value::as_bool) {
            assert_eq!(blocked, b, "{name}: blocked");
        }
    }
}

fn device(dev: [u8; 16], status: DeviceStatus, beat_ms: i64) -> DeviceView {
    DeviceView {
        dev,
        sign_pk: [0; 32],
        device_no: 1,
        status,
        name: String::new(),
        key_fp: String::new(),
        added_seq: 1,
        added_by: dev,
        bootstrap_seq: 0,
        status_seq: 1,
        status_by: dev,
        last_ctr: 0,
        beat_ms,
        rules: vec![1],
        envelopes: vec![(1, 1)],
        hash_versions: vec![1],
    }
}

// spec: ENG-DUTY-001 ENG-DUTY-002 ENG-BEAT-001 ENG-SNAP-001 ENG-SNAP-002 ENG-SNAP-003 ENG-SNAP-006
#[test]
fn engine_duties_vectors() {
    let all = cases(include_str!("../../tests/vectors/engine-duties.json"));
    let mut seen = [0usize; 3];
    for c in &all {
        let name = s(c, "name");
        match s(c, "kind") {
            "rank" => {
                seen[0] += 1;
                let devices: Vec<DeviceView> = c["devices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|d| {
                        let status =
                            if s(d, "status") == "current" { DeviceStatus::Current } else { DeviceStatus::Revoked };
                        device(arr(s(d, "dev")), status, i(d, "beat_ms"))
                    })
                    .collect();
                let order: Vec<Vec<u8>> = duty_order(&devices).into_iter().map(|d| d.to_vec()).collect();
                let want: Vec<Vec<u8>> = c["order"].as_array().unwrap().iter().map(|x| hex(x.as_str().unwrap())).collect();
                assert_eq!(order, want, "{name}");
                let view = MembershipView { devices, ..Default::default() };
                for r in c["ranks"].as_array().unwrap() {
                    assert_eq!(rank_of(&view, &arr(s(r, "dev"))), Some(u(r, "rank") as u32), "{name}");
                }
            }
            "beat" => {
                seen[1] += 1;
                let differ = c["build"] != c["record"];
                let reason = beat_due(i(c, "wall_ms"), i(c, "last_beat_ms"), differ, u(c, "pending_verdicts") > 0);
                let eligible = c["eligible"].as_bool().unwrap();
                let unsent = c["unsent_beat"].as_bool().unwrap();
                let due = eligible && !unsent && reason.is_some();
                assert_eq!(due, c["due"].as_bool().unwrap(), "{name}");
                if let Some(conds) = c.get("conditions").and_then(Value::as_array) {
                    assert_eq!(reason.map(u64::from), conds.first().and_then(Value::as_u64), "{name}: the first condition");
                }
            }
            "snapshot" => {
                seen[2] += 1;
                let own: [u8; 16] = arr(s(c, "self"));
                let snaps: Vec<SnapRecord> = c["snaps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| {
                        let verdict = |v: &Value| match v.as_str() {
                            Some("Ack") => Some(Verdict::Ack),
                            Some("Mismatch") => Some(Verdict::Mismatch),
                            _ => None,
                        };
                        SnapRecord {
                            entry_seq: u(r, "entry_seq"),
                            author: arr(s(r, "author")),
                            info: SnapInfo {
                                seq: u(r, "info_seq"),
                                hash: [0; 32],
                                hash_version: 1,
                                blob: [0; 16],
                                sha256: [0; 32],
                                size: 0,
                                parts: 1,
                                inflated: 0,
                                epoch: 1,
                            },
                            entry_clock: i(r, "entry_clock"),
                            verdicts: r["verdicts"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|v| (arr(s(v, "dev")), verdict(&v["verdict"]).unwrap(), u(v, "beat_seq")))
                                .collect(),
                            own: verdict(&r["own"]),
                        }
                    })
                    .collect();
                let due = snapshot_due(
                    u(c, "S"),
                    i(c, "tau_S"),
                    &snaps,
                    &own,
                    c["own_snap_in_outbox"].as_bool().unwrap(),
                    c["owed_mismatch"].as_bool().unwrap(),
                );
                assert_eq!(due, c["due"].as_bool().unwrap(), "{name}: due");
                for r in &snaps {
                    // §19.3 is what "usable" means in every case that names it.
                    let _ = usable(r, &own);
                }
                if let Some(w) = c.get("write").filter(|w| w.is_object()) {
                    let earliest = i(c, "tau_S") + i(w, "rank") * lim::HOUR;
                    if w.get("earliest_wall_ms").is_some() {
                        assert_eq!(earliest, i(w, "earliest_wall_ms"), "{name}: the rank wait");
                    }
                    let writes = due
                        && w["eligible"].as_bool().unwrap()
                        && i(w, "wall_ms") >= earliest
                        && !w["snap_accepted_at_or_above_S"].as_bool().unwrap();
                    assert_eq!(writes, w["writes_now"].as_bool().unwrap(), "{name}: writes now");
                }
            }
            other => panic!("unknown kind {other}"),
        }
    }
    assert!(seen.iter().all(|n| *n >= 5), "{seen:?}");
}

// spec: ENG-PEND-001 ENG-PEND-002 ENG-PEND-004 ENG-PEND-005 ENG-SM-004
#[test]
fn engine_pending_vectors() {
    let all = cases(include_str!("../../tests/vectors/engine-pending.json"));
    assert!(all.len() >= 15);
    for c in &all {
        let name = s(c, "name");
        let rules2 = c["rules2"].as_bool().unwrap_or(false);
        let sm = ToyMachine::new(rules2);
        let (sealer, _, _) = FakeSealer::create(1, "Desk");
        let mut e = Engine::new(ToyMachine::new(rules2), sealer, Config {});
        e.state = Some(sm.load(&hex(s(c, "confirmed"))).expect("the confirmed state loads"));
        e.b = u(c, "board_seq");
        e.no_b = c["device_no"].as_u64().map(|n| n as u16);
        for r in c["records"].as_array().unwrap() {
            let kind = match s(r, "kind") {
                "cmd" => Kind::Cmd,
                "ctl" => Kind::Ctl,
                "beat" => Kind::Beat,
                _ => Kind::Snap,
            };
            let payload = hex(r["payload"].as_str().unwrap_or(""));
            let cmd = if kind == Kind::Cmd { sm.parse(&payload).ok() } else { None };
            let order = u(r, "order");
            let mut id = [0u8; 16];
            id[..8].copy_from_slice(&order.to_be_bytes());
            let rec = OutboxRecord {
                entry_id: id,
                order,
                at_ms: i(r, "at_ms"),
                based_on: Head { seq: u(r, "based_on_seq"), chain: [0; 32] },
                kind,
                body: Body::Cmd(payload),
                origin_state: Vec::new(),
                via: Via::Append,
                hold: false,
                ctr: None,
                frame: None,
                frame_hash: None,
                epoch: None,
                ack: None,
                seen: r["seen"].as_u64(),
                op: None,
                reseals: 0,
            };
            e.outbox.push(Rec::new(rec, cmd));
        }
        e.rebase();
        let view = sm.canonical_bytes(e.view.as_ref().expect("a view"));
        assert_eq!(view, hex(s(c, "view")), "{name}: the view");
        let Some(unapplied) = c.get("unapplied").and_then(Value::as_array) else { continue };
        let got: Vec<(u64, u64, Option<&'static str>)> =
            e.outbox.iter().filter_map(|r| r.prov.map(|p| (r.r.order, p, r.doomed))).collect();
        let want: Vec<(u64, u64, Option<String>)> = unapplied
            .iter()
            .map(|x| (u(x, "order"), u(x, "provisional_seq"), x["doomed"].as_str().map(str::to_owned)))
            .collect();
        assert_eq!(got.len(), want.len(), "{name}: the records of U");
        for (g, w) in got.iter().zip(&want) {
            assert_eq!((g.0, g.1, g.2.map(str::to_owned)), (w.0, w.1, w.2.clone()), "{name}");
        }
    }
}
