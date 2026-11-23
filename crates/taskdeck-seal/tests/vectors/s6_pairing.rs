//! `seal-pairing.json` (seal.md §17): one pairing from fixed inputs (E's
//! board and membership, the relay origin, both machines' entropy in draw
//! order, what N is given with the code, and what E's engine returns with
//! `Registered`), with the code, `ticket_key`, `tid`, `mac_key`, slots A to
//! D, `T`, the six digits, the `DeviceAdded` E asks for, the bundle key, the
//! bundle and its ciphertext; then one invalid case per check, each
//! replayed from the valid case's inputs with one slot or entry replaced,
//! and the code it aborts with.

use serde_json::{Value, json};
use taskdeck_seal::pair::*;
use taskdeck_seal::*;

use crate::board::*;
use crate::common::hex;
use crate::pairing::*;
use crate::{check_or_write, count};

const BASE_NAME: &str = "a pairing on a board of two devices";

fn e_entropy() -> Vec<u8> {
    [count(0x10, 74), count(0xA0, 24)].concat()
}

fn n_entropy() -> Vec<u8> {
    count(0x40, 112)
}

/// The valid case's run, from its fixed inputs.
fn run(seed: u64, devices: usize) -> Run {
    Run::with(Sim::new(seed, devices), 0, source(e_entropy()), source(n_entropy()))
}

fn base() -> Run {
    run(6100, 2)
}

fn snap_json(s: &SnapAnnounce) -> Value {
    json!({ "seq": s.seq, "hash": hex(&s.hash), "hash_version": s.hash_version, "blob": hex(&s.blob.0),
            "sha256": hex(&s.sha256), "size": s.size, "parts": s.parts, "inflated": s.inflated, "epoch": s.epoch })
}

fn inputs(r: &Run, membership: &[u8], head: Head) -> Value {
    json!({
        "board_id": hex(&r.sim.board.0),
        "membership": hex(membership),
        "head": { "seq": head.seq, "chain": hex(&head.chain.0) },
        "relay_origin": ORIGIN,
        "own_dev": hex(&r.dev_e().0),
        "now": PT0,
        "entropy": { "existing": hex(&e_entropy()), "new": hex(&n_entropy()) },
        "code_entered": { "host": HOST, "door": hex(&DOOR), "name": NAME, "now": PT0 + 1 },
    })
}

fn merge(a: Value, b: Value) -> Value {
    let mut m = a.as_object().unwrap().clone();
    for (k, v) in b.as_object().unwrap() {
        m.insert(k.clone(), v.clone());
    }
    Value::Object(m)
}

/// An invalid case replayed from the valid case: `what` names the input
/// replaced, `value` its value.
fn invalid(name: &str, what: &str, value: Value, code: SealError) -> Value {
    json!({ "name": name, "base": BASE_NAME, what: value, "error": code.code() })
}

fn e_on_b(r: &mut Run, b: Vec<u8>) -> SealError {
    e_abort(&r.e_on(ExistingEvent::SlotB { bytes: b })).unwrap()
}

// spec: SEAL-PAIR-001
// spec: SEAL-PAIR-002
// spec: SEAL-PAIR-006
// spec: SEAL-PAIR-009
// spec: SEAL-PAIR-016
#[test]
fn seal_pairing() {
    let mut cases = Vec::new();

    // ------------------------------------------------- the valid case
    let mut r = base();
    let membership = Sim::new(6100, 2).m.to_canonical_bytes();
    let head = Sim::new(6100, 2).head();
    assert_eq!(membership, r.sim.m.to_canonical_bytes());
    r.step_b().step_c().step_sas().step_confirmed().step_d();
    assert_eq!(names(&r.send_d()), ["FetchSnapshot"]);
    let n_e = n_entropy();
    let esk_n: [u8; 32] = n_e[80..112].try_into().unwrap();
    let epk_e: [u8; 32] = body(&r.slot_c)[..32].try_into().unwrap();
    let dh = x(esk_n, epk_e);
    let t = transcript(&r.sim.board, &r.k.tid, &r.dev_e(), &r.slot_a, &r.slot_b, &r.slot_c);
    assert_eq!(r.digits, sas(&r.k.s, &dh, &t));
    assert_eq!(r.k.slot(A, ROLE_E, body(&r.slot_a)), r.slot_a);
    let key = bundle_key(&r.k.s, &dh, &t);
    let ad = bundle_ad(&r.sim.board, &r.k.tid, &t);
    let d = body(&r.slot_d).to_vec();
    let bundle = aead_open(&key, &ad, &d).unwrap();
    let snap = r.snap.as_ref().unwrap();
    let dev = r.device.clone().unwrap();
    let outputs = json!({
        "registered": {
            "h": snap.h, "chain": hex(&snap.chain.0), "snapshot": snap_json(&snap.ann), "added_seq": r.added_seq,
            "token": TOKEN, "roots": snap.roots.iter().map(|(e, k)| json!({ "epoch": e, "root": hex(k) })).collect::<Vec<_>>(),
            "host": HOST, "door": hex(&DOOR),
        },
        "code": r.code,
        "ticket_key": hex(&r.k.ticket_key),
        "tid": hex(&r.k.tid),
        "mac_key": hex(&r.k.mac_key),
        "slot_a": hex(&r.slot_a),
        "slot_b": hex(&r.slot_b),
        "slot_c": hex(&r.slot_c),
        "dh": hex(&dh),
        "transcript": hex(&t),
        "digits": r.digits,
        "device_added": { "dev": hex(&dev.dev.0), "sign_pk": hex(&dev.sign_pk), "hpke_pk": hex(&dev.hpke_pk),
                          "name": dev.name, "transcript": hex(&dev.transcript), "bootstrap_seq": dev.bootstrap_seq },
        "bundle_key": hex(&key),
        "bundle": String::from_utf8(bundle.clone()).unwrap(),
        "nonce": hex(&d[..24]),
        "ciphertext": hex(&d[24..]),
        "slot_d": hex(&r.slot_d),
    });
    cases.push(merge(merge(json!({ "name": BASE_NAME }), inputs(&r, &membership, head)), outputs));
    let (slot_a, slot_b, slot_c, digits) = (r.slot_a.clone(), r.slot_b.clone(), r.slot_c.clone(), r.digits.clone());

    // ------------------------------------------- E's checks on slot B
    let k = r.k.clone();
    let edit_b = |f: &dyn Fn(&mut Vec<u8>)| {
        let mut bb = body(&slot_b).to_vec();
        f(&mut bb);
        k.slot(B, ROLE_N, &bb)
    };
    let mut flipped = slot_b.clone();
    let n = flipped.len();
    flipped[n - 1] ^= 1;
    let e_dev = r.dev_e().0;
    let b_cases: Vec<(&str, Vec<u8>, SealError)> = vec![
        ("E: slot B with a flipped MAC byte", flipped, SealError::SlotMac),
        ("E: slot B under label C", k.slot(C, ROLE_N, body(&slot_b)), SealError::SlotMac),
        ("E: slot B from role E", k.slot(B, ROLE_E, body(&slot_b)), SealError::SlotMac),
        ("E: slot B with no rules version", edit_b(&|b| {
            b[112] = 0;
            b.drain(113..117);
        }), SealError::SlotFormat),
        ("E: slot B naming E's own dev", edit_b(&|b| b[..16].copy_from_slice(&e_dev)), SealError::PairKeysConflict),
        ("E: slot B without E's rules version", edit_b(&|b| b[113..117].copy_from_slice(&2u32.to_be_bytes())), SealError::PairRules),
        ("E: slot B with an all-zero epk_N", edit_b(&|b| b[80..112].fill(0)), SealError::DhZero),
    ];
    for (name, b, code) in b_cases {
        let mut q = base();
        q.step_b();
        assert_eq!(q.slot_b, slot_b);
        assert_eq!(e_on_b(&mut q, b.clone()), code, "{name}");
        cases.push(invalid(name, "slot_b", json!(hex(&b)), code));
    }
    // A board of sixteen current devices.
    let mut q = run(6101, 16);
    let m16 = Sim::new(6101, 16).m.to_canonical_bytes();
    let h16 = Sim::new(6101, 16).head();
    q.step_b();
    let b16 = q.slot_b.clone();
    assert_eq!(e_on_b(&mut q, b16.clone()), SealError::CtlDeviceCap);
    let mut c = inputs(&q, &m16, h16);
    c["name"] = json!("E: slot B on a board of sixteen current devices");
    c["slot_b"] = json!(hex(&b16));
    c["error"] = json!(SealError::CtlDeviceCap.code());
    cases.push(c);

    // ------------------------------------------------- N's checks
    let n_slot = |slots: &[Vec<u8>], name: &str, what: &str, code: SealError| -> Value {
        let mut q = base();
        let _ = q.n_on(q.code_event());
        let mut fx = Vec::new();
        for s in slots {
            fx = q.n_on(NewEvent::Slot { bytes: s.clone() });
        }
        assert_eq!(n_abort(&fx), Some(code), "{name}");
        let v = if slots.len() == 1 { json!(hex(&slots[0])) } else { json!(slots.iter().map(|s| hex(s)).collect::<Vec<_>>()) };
        invalid(name, what, v, code)
    };
    let mut fa = slot_a.clone();
    fa[10] ^= 1;
    cases.push(n_slot(&[fa], "N: slot A with a flipped body byte", "slot_a", SealError::SlotMac));
    cases.push(n_slot(&[k.slot(A, ROLE_E, &body(&slot_a)[..47])], "N: slot A of 47 bytes", "slot_a", SealError::SlotFormat));
    let mut cb = body(&slot_c).to_vec();
    cb[40] ^= 1;
    cases.push(n_slot(&[slot_a.clone(), k.slot(C, ROLE_E, &cb)], "N: slot C not opening the commitment", "slots_a_c", SealError::CommitMismatch));
    let mut one = [0u8; 32];
    one[0] = 1;
    let r0 = [0x5Au8; 32];
    let a0 = k.slot(A, ROLE_E, &[&r.dev_e().0[..], &commit(&one, &r0)].concat());
    let c0 = k.slot(C, ROLE_E, &[one, r0].concat());
    cases.push(n_slot(&[a0, c0], "N: slot C with a point of order 1 (all-zero dh)", "slots_a_c", SealError::DhZero));
    // Three wrong entries.
    let mut q = base();
    q.step_b().step_c().step_sas();
    let wrong = format!("{:06}", (digits.parse::<u32>().unwrap() + 1) % 1_000_000);
    for i in 0..3 {
        let fx = q.n_on(NewEvent::SasEntered { digits: wrong.clone() });
        assert_eq!(n_abort(&fx).is_some(), i == 2);
    }
    cases.push(invalid("N: three wrong entries of the digits", "typed", json!([wrong, wrong, wrong]), SealError::SasMismatch));
    // Slot D: the bundle's ciphertext, and a bundle naming another door.
    let d_case = |name: &str, d: Vec<u8>, code: SealError| -> Value {
        let mut q = base();
        q.step_b().step_c().step_sas().step_confirmed().step_d();
        assert_eq!(q.slot_d, r.slot_d);
        let fx = q.n_on(NewEvent::Slot { bytes: d.clone() });
        assert_eq!(n_abort(&fx), Some(code), "{name}");
        invalid(name, "slot_d", json!(hex(&d)), code)
    };
    let mut fd = d.clone();
    fd[30] ^= 1;
    cases.push(d_case("N: slot D with a flipped ciphertext byte, MAC made again", k.slot(D, ROLE_E, &fd), SealError::BundleOpen));
    let other = String::from_utf8(bundle).unwrap().replace(&format!("\"door\":\"{}\"", hex(&DOOR)), "\"door\":\"0000000000\"");
    let sealed = aead_seal(&key, [0x33; 24], &ad, other.as_bytes());
    cases.push(d_case("N: slot D whose bundle names another door", k.slot(D, ROLE_E, &sealed), SealError::BundleInvalid));
    // Time.
    let mut q = base();
    let fx = q.e_on(ExistingEvent::Tick { now: PT0 + 600_000 });
    assert_eq!(e_abort(&fx), Some(SealError::PairTimeout));
    cases.push(invalid("E: a tick 600,000 ms after the start", "tick", json!(PT0 + 600_000), SealError::PairTimeout));

    check_or_write("seal-pairing.json", json!({ "spec": "seal.md §17", "cases": cases }));
}

fn names(fx: &[NewEffect]) -> Vec<&'static str> {
    fx.iter().map(n_name).collect()
}
