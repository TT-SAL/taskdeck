//! `seal-receipt.json` (seal.md §14): one case per step of §14.2, passing
//! and failing. Each case holds every input of §14.1 (`M_{s−1}` as its
//! canonical bytes, the frame, `s`, `C_{s−1}`, the roots for `E` and `E − 1`,
//! the chain values from `retained_from`, the own identity and its sealed
//! frames, the supported rules, the genesis anchor, the Skip target frames)
//! and every output (class, code, flags, `C_s`, `M_s`, the learned root, the
//! halt), or the local error. The file is replayed case by case at the end.
//!
//! Frames the public sealer refuses to make (an unknown envelope version, a
//! malformed plaintext) are built here with the pinned crates.

use crate::board::*;
use crate::check_or_write;
use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use ed25519_dalek::Signer as _;
use serde_json::{Value, json};
use taskdeck_seal::*;

/// A padded plaintext of §10.1, field by field.
pub fn padded(entry_id: [u8; 16], kind: Kind, at_ms: i64, based_on: Head, payload: &[u8]) -> Vec<u8> {
    let mut v = entry_id.to_vec();
    v.push(kind as u8);
    v.extend(at_ms.to_be_bytes());
    v.extend(based_on.seq.to_be_bytes());
    v.extend(based_on.chain.0);
    v.extend((payload.len() as u32).to_be_bytes());
    v.extend(payload);
    v.resize(padded_len(v.len() as u64) as usize, 0);
    v
}

/// A frame of §8 with any `ver`, `suite` and padded plaintext, signed by
/// device `i` of `s` (seal.md §8.1, §8.3, §9).
#[allow(clippy::too_many_arguments)]
pub fn raw(s: &mut Sim, i: usize, ctr: u64, ver: u8, suite: u8, epoch: u32, root: &[u8; 32], pt: &[u8]) -> Vec<u8> {
    let id = s.devs[i].to_identity_bytes();
    let sk = ed25519_dalek::SigningKey::from_bytes(id[16..48].try_into().unwrap());
    let len = (30 + pt.len() + 16) as u32;
    let mut f = b"TDF1".to_vec();
    f.extend(s.board.0);
    f.extend(&id[..16]);
    f.extend(ctr.to_be_bytes());
    f.extend(len.to_be_bytes());
    let info = [b"taskdeck/v1/key-entry\0".as_slice(), &epoch.to_be_bytes()].concat();
    let mut k = [0u8; 32];
    hkdf::Hkdf::<sha2::Sha256>::new(Some(&s.board.0), root).expand(&info, &mut k).unwrap();
    let ad = [b"taskdeck/v1/entry\0".as_slice(), &f, &[ver, suite], &epoch.to_be_bytes()].concat();
    let nonce: [u8; 24] = s.rng.arr();
    let ct = chacha20poly1305::XChaCha20Poly1305::new_from_slice(&k)
        .unwrap()
        .encrypt(&chacha20poly1305::XNonce::from(nonce), Payload { msg: pt, aad: &ad })
        .unwrap();
    f.extend([ver, suite]);
    f.extend(epoch.to_be_bytes());
    f.extend(nonce);
    f.extend(ct);
    let sig = sk.sign(&[b"taskdeck/v1/frame\0".as_slice(), &f].concat());
    f.extend(sig.to_bytes());
    f
}

/// Classifies `frame` at the next seq with `o`, records the case and checks
/// its class and code.
pub fn run(s: &Sim, cases: &mut Vec<Value>, name: &str, frame: &[u8], o: &Opts, class: &str, code: Option<&str>) {
    let inputs = s.inputs(name, frame, o);
    match replay(&inputs) {
        Ok(r) => cases.push(case(inputs, &r, class, code)),
        Err(e) => {
            assert_eq!((class, code), ("error", Some(e.code())), "{name}");
            let mut m = inputs.as_object().unwrap().clone();
            m.insert("error".into(), json!(e.code()));
            cases.push(Value::Object(m));
        }
    }
}

/// A seq-1 case: the inputs of `s.inputs` with the empty state and the anchor.
pub fn at_seq1(s: &Sim, cases: &mut Vec<Value>, name: &str, frame: &[u8], class: &str, code: Option<&str>) {
    let o = Opts { seq: Some(1), genesis: Some(s.anchor_parts()), ..Opts::default() };
    let mut inputs = s.inputs(name, frame, &o);
    inputs["membership"] = json!(crate::common::hex(&empty_membership()));
    inputs["roots"] = json!({});
    inputs["chains"] = json!({});
    let r = replay(&inputs).unwrap();
    cases.push(case(inputs, &r, class, code));
}

fn seq1_spec() -> Spec {
    Spec { ctr: Some(1), epoch: Some(1), ..Spec::default() }
}

#[test]
fn seal_receipt_vectors() {
    let mut s = Sim::new(51, 3);
    let mut cases = Vec::new();
    let d = Opts::default;

    // Seq 1.
    let (gi, go) = s.genesis_case.clone().unwrap();
    let mut g = gi.as_object().unwrap().clone();
    g["name"] = json!("step 17: Genesis accepted at seq 1");
    for (k, v) in go.as_object().unwrap() {
        g.insert(k.clone(), v.clone());
    }
    assert_eq!(g["class"], "Accept");
    cases.push(Value::Object(g));
    let c0 = Head { seq: 0, chain: chain_zero(&s.board) };
    let gitem = s.genesis_item();
    let f = s.ctl(Who::R, vec![CtlItem::Genesis(gitem.clone())], &Spec { epoch: Some(2), root: Some([1; 32]), based_on: Some(c0), ..seq1_spec() });
    at_seq1(&s, &mut cases, "step 9: epoch 2 at seq 1", &f, "Refuse", Some("seal/bad_epoch"));
    let f = s.ctl(Who::R, vec![CtlItem::Genesis(gitem.clone())], &Spec { based_on: Some(c0), ctr: Some(2), ..seq1_spec() });
    at_seq1(&s, &mut cases, "step 5: ctr 2 at seq 1", &f, "Tamper", Some("seal/counter_gap"));
    let f = s.ctl(Who::D(0), vec![CtlItem::Genesis(gitem.clone())], &Spec { based_on: Some(c0), ..seq1_spec() });
    at_seq1(&s, &mut cases, "step 3: Genesis signed by the device at seq 1", &f, "Skip", Some("seal/unknown_author"));
    let mut bad = gitem.clone();
    bad.commit = [4; 32];
    let f = s.ctl(Who::R, vec![CtlItem::Genesis(bad)], &Spec { based_on: Some(c0), ..seq1_spec() });
    at_seq1(&s, &mut cases, "step 16: Genesis commit not c_1 of the anchor's R_1", &f, "Refuse", Some("seal/ctl_genesis"));
    let fence = |g| CtlItem::Fence(Fence { host: "relay.example".into(), door: [1; 5], home_gen: g });
    let f = s.ctl(Who::R, vec![fence(2)], &Spec { based_on: Some(c0), ..seq1_spec() });
    at_seq1(&s, &mut cases, "step 16: a Fence at seq 1", &f, "Refuse", Some("seal/ctl_shape"));

    // Steps 1 to 6 at seq 4.
    let f = s.cmd(Who::D(1), &Spec::default());
    run(&s, &mut cases, "step 17: cmd accepted", &f, &d(), "Accept", None);
    run(&s, &mut cases, "step 1: 669 bytes", &f[..669], &d(), "Skip", Some("seal/frame_format"));
    let mut m = f.clone();
    m[44..48].copy_from_slice(&((f.len() - 111) as u32).to_be_bytes());
    run(&s, &mut cases, "step 1: len not total − 112", &m, &d(), "Skip", Some("seal/frame_format"));
    let mut other = Sim::new(52, 1);
    let of = other.cmd(Who::D(0), &Spec::default());
    run(&s, &mut cases, "step 2: a frame of another board", &of, &d(), "Skip", Some("seal/wrong_board"));
    let stranger = s.new_device();
    s.devs.push(stranger);
    let n = s.devs.len() - 1;
    let sf = s.cmd(Who::D(n), &Spec { ctr: Some(1), ..Spec::default() });
    run(&s, &mut cases, "step 3: an author the board does not know", &sf, &d(), "Skip", Some("seal/unknown_author"));
    let mut m = f.clone();
    m[100] ^= 1;
    run(&s, &mut cases, "step 4: a ciphertext byte flipped", &m, &d(), "Skip", Some("seal/bad_signature"));
    let gap = s.cmd(Who::D(1), &Spec { ctr: Some(3), ..Spec::default() });
    run(&s, &mut cases, "step 5: ctr skips a value", &gap, &d(), "Tamper", Some("seal/counter_gap"));
    let own = Opts { own: Some(1), ..Opts::default() };
    run(&s, &mut cases, "step 6: own frame, recorded", &f, &own, "Accept", None);
    let theirs = s.cmd(Who::D(1), &Spec { ctr: Some(1), ..Spec::default() });
    s.sealed.insert((s.dev(Who::D(1)), 1), frame_hash(&f));
    run(&s, &mut cases, "step 6: own dev, no record of this frame: own_dev_foreign", &theirs, &own, "Accept", None);
    assert_eq!(cases.last().unwrap()["own_dev_foreign"], true);
    s.accept(f);
    let replay_f = s.cmd(Who::D(1), &Spec { ctr: Some(1), ..Spec::default() });
    run(&s, &mut cases, "step 5: ctr replayed", &replay_f, &d(), "Tamper", Some("seal/counter_replay"));

    // Steps 8 to 16 at seq 5.
    let head = s.head();
    let root1 = s.roots[&1];
    let ctr = s.next_ctr(Who::D(0));
    let pt = padded(s.rng.arr(), Kind::Cmd, s.now, head, b"x");
    let f = raw(&mut s, 0, ctr, 2, 1, 1, &root1, &pt);
    run(&s, &mut cases, "step 8: envelope version 2", &f, &d(), "Refuse", Some("seal/bad_version"));
    let f = raw(&mut s, 0, ctr, 1, 1, 1, &root1, &pt);
    run(&s, &mut cases, "step 17: the same plaintext under (1, 1)", &f, &d(), "Accept", None);
    let f = s.cmd(Who::D(0), &Spec { epoch: Some(2), root: Some([5; 32]), ..Spec::default() });
    run(&s, &mut cases, "step 9: epoch above E", &f, &d(), "Refuse", Some("seal/bad_epoch"));
    let f = s.cmd(Who::D(0), &Spec { root: Some([5; 32]), ..Spec::default() });
    run(&s, &mut cases, "step 10: sealed under another key", &f, &d(), "Refuse", Some("seal/aead"));
    let mut bad = pt.clone();
    *bad.last_mut().unwrap() = 1;
    let f = raw(&mut s, 0, ctr, 1, 1, 1, &root1, &bad);
    run(&s, &mut cases, "step 11: a padding byte not zero", &f, &d(), "Refuse", Some("seal/bad_plaintext"));
    let f = s.cmd(Who::D(0), &Spec { based_on: Some(Head { seq: 5, chain: head.chain }), ..Spec::default() });
    run(&s, &mut cases, "step 12: based_on_seq = s", &f, &d(), "Tamper", Some("seal/based_on_future"));
    let f = s.cmd(Who::D(0), &Spec { based_on: Some(Head { seq: 2, chain: s.chains[1] }), ..Spec::default() });
    run(&s, &mut cases, "step 12: based_on_chain is not C_2", &f, &Opts { retained_from: Some(1), ..Opts::default() }, "Tamper", Some("seal/based_on_fork"));
    run(&s, &mut cases, "step 12: b below retained_from: unchecked", &f, &Opts { retained_from: Some(3), ..Opts::default() }, "Accept", None);
    assert_eq!(cases.last().unwrap()["based_on_unchecked"], true);
    let f = s.cmd(Who::R, &Spec::default());
    run(&s, &mut cases, "step 13: cmd by the recovery identity", &f, &d(), "Refuse", Some("seal/not_authorized"));
    let f = s.plain(Who::D(0), Kind::Beat, b"{}".to_vec(), &Spec::default());
    run(&s, &mut cases, "step 15: beat payload without its fields", &f, &d(), "Refuse", Some("seal/bad_payload"));
    let f = s.ctl_by(Who::D(0), Who::D(1), vec![fence(2)], &Spec::default());
    run(&s, &mut cases, "step 15: inner signature by another device", &f, &d(), "Refuse", Some("seal/bad_ctl_sig"));
    let f = s.ctl(Who::R, vec![CtlItem::Genesis(gitem)], &Spec::default());
    run(&s, &mut cases, "step 16: a Genesis at seq 5", &f, &d(), "Refuse", Some("seal/ctl_shape"));
    let t = 4;
    let skip = CtlItem::Skip(SkipItem { seq: t, frame_hash: frame_hash(&s.frames[t as usize - 1]) });
    let f = s.ctl(Who::D(0), vec![skip], &Spec::default());
    run(&s, &mut cases, "step 16: a Skip whose target frame is not supplied", &f, &d(), "NeedFrames", None);
    run(&s, &mut cases, "step 16: the same Skip with its target frame", &f, &Opts { targets: vec![4], ..Opts::default() }, "Accept", None);
    let f = s.beat(Who::D(2), vec![1], &Spec::default());
    run(&s, &mut cases, "step 17: beat accepted", &f, &d(), "Accept", None);
    let f = s.ctl(Who::D(0), vec![fence(2)], &Spec::default());
    run(&s, &mut cases, "step 17: Fence accepted", &f, &d(), "Accept", None);
    run(&s, &mut cases, "no root held for the epoch", &f, &Opts { hide_roots: vec![1], ..Opts::default() }, "error", Some("seal/missing_root"));

    // After a rotation that revokes device 2: steps 7, 9, 13, 14.
    let before = s.head();
    s.revoke(Who::D(0), 2);
    let f = s.cmd(Who::D(2), &Spec::default());
    run(&s, &mut cases, "step 7: a revoked device", &f, &d(), "Refuse", Some("seal/revoked_author"));
    let stale = |s: &mut Sim, b: Head| s.cmd(Who::D(1), &Spec { epoch: Some(1), based_on: Some(b), ..Spec::default() });
    let f = stale(&mut s, before);
    run(&s, &mut cases, "step 14: under E − 1, composed before the rotation", &f, &d(), "Accept", None);
    let h = s.head();
    let f = stale(&mut s, h);
    run(&s, &mut cases, "step 14: under E − 1, composed after the rotation", &f, &d(), "Refuse", Some("seal/stale_epoch"));
    let f = s.ctl(Who::D(1), vec![fence(2)], &Spec { epoch: Some(1), ..Spec::default() });
    run(&s, &mut cases, "step 13: a ctl under E − 1", &f, &d(), "Refuse", Some("seal/stale_epoch"));
    s.rotate(Who::D(0));
    let f = s.cmd(Who::D(1), &Spec { epoch: Some(1), based_on: Some(before), ..Spec::default() });
    run(&s, &mut cases, "step 9: an epoch below E − 1", &f, &d(), "Refuse", Some("seal/old_epoch"));

    let doc = json!({ "spec": "seal.md §14", "cases": cases });
    for c in doc["cases"].as_array().unwrap() {
        check_case(c);
    }
    check_or_write("seal-receipt.json", doc);
}
