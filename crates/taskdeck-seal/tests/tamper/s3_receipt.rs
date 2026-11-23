//! Frames through `classify` (seal.md §14): flipping any byte of a frame,
//! a Genesis frame, a control entry with a rotation or a beat never yields
//! `Accept`, and never changes the membership state beyond what §13.5 allows
//! for the class returned.

use crate::board::*;
use taskdeck_seal::*;

/// The positions where two byte strings differ.
fn diff(a: &[u8], b: &[u8]) -> Vec<usize> {
    assert_eq!(a.len(), b.len());
    (0..a.len()).filter(|&i| a[i] != b[i]).collect()
}

/// Checks the class of a tampered frame and what it did to the state:
/// never `Accept`, nothing of the plaintext returned, no root learned.
pub(crate) fn check(prev: &[u8], r: &Receipt) {
    assert!(!matches!(r.class, Class::Accept), "a flipped byte was accepted");
    assert!(r.opened.is_none(), "a flipped frame returned its plaintext");
    assert!(r.learned_root.is_none(), "a flipped frame taught a root");
    check_state(prev, r);
}

/// What a class other than `Accept` may do to the state (§13.5).
pub(crate) fn check_state(prev: &[u8], r: &Receipt) {
    assert!(!matches!(r.class, Class::Accept), "accepted");
    let after = r.membership.to_canonical_bytes();
    match r.class {
        // §13.5: Refuse sets the author's counter and nothing else: the
        // bytes differ in one 8-byte field at most.
        Class::Refuse => {
            let d = diff(prev, &after);
            assert!(d.is_empty() || d[d.len() - 1] - d[0] < 8, "a refusal changed more than a counter");
        }
        _ => assert_eq!(prev, after.as_slice(), "the state changed"),
    }
}

pub(crate) fn flip_all(s: &Sim, f: &[u8], o: &Opts) -> usize {
    let prev = s.m.to_canonical_bytes();
    let ok = s.classify(f, o).unwrap();
    assert!(matches!(ok.class, Class::Accept));
    let mut n = 0;
    for i in 0..f.len() {
        for mask in [0x01u8, 0x80] {
            let mut m = f.to_vec();
            m[i] ^= mask;
            let r = s.classify(&m, o).unwrap();
            check(&prev, &r);
            n += 1;
        }
    }
    n
}

// Every byte of a `cmd`, a `beat`, a control entry that revokes and
// rotates, and a `Fence`, flipped two ways: never `Accept`; the state as
// §13.5 allows for the class.
// spec: SEAL-RCPT-002
// spec: SEAL-MEM-008
#[test]
fn flipped_frames_are_never_accepted() {
    let mut s = Sim::new(31, 3);
    let o = Opts::default();
    let f = s.cmd(Who::D(1), &Spec::default());
    assert!(flip_all(&s, &f, &o) > 1000);
    let f = s.beat(Who::D(2), vec![1], &Spec::default());
    flip_all(&s, &f, &o);
    let items = s.rotate_items(vec![CtlItem::DeviceRevoked(s.dev(Who::D(2)))]);
    let f = s.ctl(Who::D(0), items, &Spec::default());
    flip_all(&s, &f, &Opts { own: Some(1), ..Opts::default() });
    let fence = CtlItem::Fence(Fence { host: "relay.example".into(), door: [1; 5], home_gen: 2 });
    let f = s.ctl(Who::R, vec![fence], &Spec::default());
    flip_all(&s, &f, &o);
}

// Every byte of the Genesis frame, flipped, at seq 1 with its anchor: never
// `Accept`, and the state stays the empty one.
// spec: SEAL-RCPT-002
// spec: SEAL-MEM-004
#[test]
fn flipped_genesis_is_never_accepted() {
    let s = Sim::new(32, 1);
    let (inputs, _) = s.genesis_case.clone().unwrap();
    let f = crate::common::unhex(inputs["frame"].as_str().unwrap());
    let empty = empty_membership();
    for i in 0..f.len() {
        let mut m = f.clone();
        m[i] ^= 0x01;
        let mut c = inputs.clone();
        c["frame"] = serde_json::json!(crate::common::hex(&m));
        let r = replay(&c).unwrap();
        check(&empty, &r);
    }
}
