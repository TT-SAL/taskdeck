//! Frames the earlier modules did not flip (task S7): a `snap` entry, and
//! the inner signature of a control payload, whose bytes no relay can reach
//! but whose author can: every byte of the payload flipped and then sealed
//! properly by the frame's author.

use crate::board::*;
use crate::s3_receipt::{check_state, flip_all};
use taskdeck_seal::*;

// Every byte of a `snap` entry, flipped two ways: never `Accept`, nothing
// of the plaintext returned, the state as §13.5 allows for the class.
// spec: SEAL-RCPT-002
// spec: SEAL-KIND-006
#[test]
fn flipped_snap_frames_are_never_accepted() {
    let mut s = Sim::new(71, 2);
    for _ in 0..64 {
        let f = s.cmd(Who::D(0), &Spec::default());
        s.accept(f);
    }
    let snap = SnapAnnounce {
        seq: 64,
        hash: [0x55; 32],
        hash_version: 1,
        blob: BlobId([0x10; 16]),
        sha256: [0x66; 32],
        size: 1_500_000,
        parts: 2,
        inflated: 4_000_000,
        epoch: s.epoch(),
    };
    let f = s.plain(Who::D(1), Kind::Snap, snap_payload(&snap).unwrap(), &Spec::default());
    assert!(flip_all(&s, &f, &Opts::default()) > 1000);
}

// A control payload whose every byte is flipped in turn (its items, the
// `sig` field's hex, the JSON around them), each sealed properly by the
// frame's author with the same `ctr`, `entry_id` and `at_ms`: the inner
// signature or the payload's parser rejects it, never `Accept`, and the
// state changes at most by the author's counter. (The plaintext is the
// author's own here, sealed by its key, so `opened` may be set: §26 sets it
// whenever step 11 passed.) The unflipped payload,
// sealed the same way, is accepted (the control).
// spec: SEAL-KIND-003
// spec: SEAL-KIND-004
#[test]
fn a_flipped_inner_signature_or_payload_is_refused() {
    for (seed, who, items) in [
        (72, Who::R, vec![CtlItem::Fence(Fence { host: "relay.example".into(), door: [1; 5], home_gen: 2 })]),
        (73, Who::D(0), vec![CtlItem::DeviceRevoked(DevId([0; 16]))]),
    ] {
        let mut s = Sim::new(seed, 3);
        let items = match &items[0] {
            CtlItem::DeviceRevoked(_) => {
                let revoked = CtlItem::DeviceRevoked(s.dev(Who::D(2)));
                s.rotate_items(vec![revoked])
            }
            _ => items,
        };
        let ctr = s.next_ctr(who);
        let epoch = s.epoch();
        let root = s.roots[&epoch];
        let at_ms = s.now;
        let based_on = s.head();
        let entry_id = EntryId([0x3C; 16]);
        let payload = ctl_payload(&items, &s.signer(who), &s.board, ctr, &entry_id, at_ms).unwrap();
        let seal = |s: &mut Sim, payload: Vec<u8>| {
            let plain = Plain { entry_id, kind: Kind::Ctl, at_ms, based_on, payload };
            s.seal(who, &plain, ctr, epoch, root)
        };
        let good = seal(&mut s, payload.clone());
        let prev = s.m.to_canonical_bytes();
        assert!(matches!(s.classify(&good, &Opts::default()).unwrap().class, Class::Accept));
        for i in 0..payload.len() {
            for mask in [0x01u8, 0x20] {
                let mut p = payload.clone();
                p[i] ^= mask;
                let f = seal(&mut s, p);
                let r = s.classify(&f, &Opts::default()).unwrap();
                check_state(&prev, &r);
                assert!(r.learned_root.is_none());
                assert!(matches!(r.class, Class::Refuse), "byte {i} with {mask:#04x}: {:?}", r.code);
            }
        }
    }
}
