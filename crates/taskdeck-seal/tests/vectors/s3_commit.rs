//! `seal-commit.json` (seal.md §6.5, §13.8, §14.5): rotations with good and
//! bad wraps classified by a recipient (`own`), a device the rotation
//! revokes, and a reader without an own identity: `Accept` with the learned
//! root, or `Accept` with the commitment halt. Inputs and outputs as in
//! `seal-receipt.json`; the file is replayed case by case at the end.

use crate::board::*;
use crate::check_or_write;
use crate::s3_receipt::run;
use serde_json::json;
use taskdeck_seal::*;

#[test]
fn seal_commit_vectors() {
    let mut s = Sim::new(71, 3);
    let mut cases = Vec::new();
    let own = |i| Opts { own: Some(i), ..Opts::default() };

    // A good rotation.
    let items = s.rotate_items(vec![]);
    let f = s.ctl(Who::D(0), items, &Spec::default());
    let (_, root) = s.staged.unwrap();
    run(&s, &mut cases, "good wraps: a recipient learns the root", &f, &own(1), "Accept", None);
    assert_eq!(cases.last().unwrap()["learned_root"]["root"], json!(crate::common::hex(&root)));
    run(&s, &mut cases, "good wraps: the author's own device learns the root", &f, &own(0), "Accept", None);
    run(&s, &mut cases, "good wraps: a reader without an own identity", &f, &Opts::default(), "Accept", None);

    // Device 2's wrap holds another root under the right commitment, then
    // the right root under another commitment.
    let rc = s.recipients_after(&[]);
    let target = s.dev(Who::D(2));
    let pk = s.devs[2].public().hpke_pk;
    for (name, other_root, other_commit) in [
        ("a wrap holding another root: the commitment halt", true, false),
        ("a wrap made under another commitment: the commitment halt", false, true),
    ] {
        let mut r = s.rotation_to(2, &rc, None);
        let (e, good) = s.staged.unwrap();
        let root = EpochRoot::from_bytes(if other_root { [0x44; 32] } else { good });
        let commit = if other_commit { EpochRoot::from_bytes([0x45; 32]).commitment(&s.board, e) } else { r.commit };
        let w = wrap_root(&root, &s.board, e, &commit, target, &pk, &mut *source(s.rng.bytes(32))).unwrap();
        for x in r.wraps.iter_mut() {
            if x[..16] == target.0 {
                *x = w;
            }
        }
        let f = s.ctl(Who::D(0), vec![CtlItem::EpochRotated(r)], &Spec::default());
        run(&s, &mut cases, name, &f, &own(2), "Accept", Some("seal/commitment"));
        assert_eq!(cases.last().unwrap()["halt"], "Commitment");
        run(&s, &mut cases, &format!("{name}, seen by a device whose wrap is good"), &f, &own(1), "Accept", None);
    }

    // The rotation revokes device 2: it is no recipient, learns nothing.
    let items = s.rotate_items(vec![CtlItem::DeviceRevoked(target)]);
    let f = s.ctl(Who::D(0), items, &Spec::default());
    run(&s, &mut cases, "a device the rotation revokes: no wrap, no halt", &f, &own(2), "Accept", None);
    assert_eq!(cases.last().unwrap()["learned_root"], serde_json::Value::Null);

    let doc = json!({ "spec": "seal.md §14.5", "cases": cases });
    for c in doc["cases"].as_array().unwrap() {
        check_case(c);
    }
    check_or_write("seal-commit.json", doc);
}
