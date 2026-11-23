//! `seal-membership.json` (seal.md §11 to §14): one case per row of §13.6
//! accepted, one per refusal code of §13.6, and the races of V2-8; inputs
//! and outputs as in `seal-receipt.json`. The file is replayed case by case
//! at the end.

use crate::board::*;
use crate::check_or_write;
use crate::s3_receipt::run;
use serde_json::{Value, json};
use taskdeck_seal::*;

fn fence(g: u32) -> CtlItem {
    CtlItem::Fence(Fence { host: "relay.example".into(), door: [1, 2, 3, 4, 5], home_gen: g })
}

fn rkc(mode: RecoveryKeyMode, k: Option<RecoveryPublic>) -> CtlItem {
    CtlItem::RecoveryKeyChange(RecoveryKeyChange { mode, sign_pk: k.map(|k| k.sign_pk), hpke_pk: k.map(|k| k.hpke_pk) })
}

fn rules(v: u32, mode: RulesUpgradeMode) -> CtlItem {
    CtlItem::RulesUpgrade(RulesUpgrade { rules: v, mode })
}

/// Classifies a control entry by `who` at the next seq, records the case,
/// and commits the frame when `commit` is set.
#[allow(clippy::too_many_arguments)]
fn ctl(s: &mut Sim, cases: &mut Vec<Value>, name: &str, who: Who, items: Vec<CtlItem>, sp: &Spec, class: &str, code: Option<&str>, commit: bool) {
    let f = s.ctl(who, items, sp);
    run(s, cases, name, &f, &Opts::default(), class, code);
    if commit {
        s.push(f);
    }
}

#[test]
fn seal_membership_vectors() {
    let mut cases = Vec::new();
    let d = Spec::default;
    let acc = "Accept";
    let rf = "Refuse";

    // Genesis.
    let s = Sim::new(61, 1);
    let (gi, go) = s.genesis_case.clone().unwrap();
    let mut g = gi.as_object().unwrap().clone();
    g["name"] = json!("Genesis accepted: the state of §13.3");
    for (k, v) in go.as_object().unwrap() {
        g.insert(k.clone(), v.clone());
    }
    cases.push(Value::Object(g));
    let mut s2 = Sim::new(62, 1);
    let mut gitem = s2.genesis_item();
    gitem.salt = [9; 16];
    let c0 = Head { seq: 0, chain: chain_zero(&s2.board) };
    let f = s2.ctl(Who::R, vec![CtlItem::Genesis(gitem)], &Spec { ctr: Some(1), epoch: Some(1), based_on: Some(c0), ..d() });
    crate::s3_receipt::at_seq1(&s2, &mut cases, "Genesis: salt not the anchor's", &f, rf, Some("seal/ctl_genesis"));

    // DeviceAdded, Fence, ResetTo, RecoveryKeyChange, RulesUpgrade.
    let mut s = Sim::new(63, 3);
    let k = s.new_device();
    let item = s.added_item(&k);
    ctl(&mut s, &mut cases, "DeviceAdded accepted", Who::D(0), vec![item.clone()], &d(), acc, None, false);
    ctl(&mut s, &mut cases, "DeviceAdded by the recovery identity", Who::R, vec![item], &d(), rf, Some("seal/ctl_author"), false);
    let p = k.public();
    let dup = s.devs[1].public().sign_pk;
    let added = |sign_pk, bootstrap_seq| CtlItem::DeviceAdded(DeviceAdded { dev: p.dev, sign_pk, hpke_pk: p.hpke_pk, name: "Phone".into(), transcript: [7; 32], bootstrap_seq });
    ctl(&mut s, &mut cases, "DeviceAdded with a recorded sign_pk", Who::D(0), vec![added(dup, 1)], &d(), rf, Some("seal/ctl_duplicate_key"), false);
    let next = s.seq() + 1;
    ctl(&mut s, &mut cases, "DeviceAdded with bootstrap_seq = s", Who::D(0), vec![added(p.sign_pk, next)], &d(), rf, Some("seal/ctl_target"), false);
    ctl(&mut s, &mut cases, "Fence with a home generation not the next", Who::D(1), vec![fence(3)], &d(), rf, Some("seal/ctl_home_gen"), false);
    ctl(&mut s, &mut cases, "Fence accepted", Who::D(1), vec![fence(2)], &d(), acc, None, true);
    let reset = |t, hv| CtlItem::ResetTo(ResetTo { seq: t, state_hash: [6; 32], hash_version: hv });
    ctl(&mut s, &mut cases, "ResetTo with hash_version 2", Who::D(0), vec![reset(3, 2)], &d(), rf, Some("seal/ctl_target"), false);
    ctl(&mut s, &mut cases, "ResetTo accepted", Who::D(0), vec![reset(3, 1)], &d(), acc, None, true);

    let nk = s.new_recovery();
    let np = nk.public();
    ctl(&mut s, &mut cases, "RecoveryKeyChange cancel without a proposal", Who::D(2), vec![rkc(RecoveryKeyMode::Cancel, None)], &d(), rf, Some("seal/ctl_no_pending"), false);
    ctl(&mut s, &mut cases, "RecoveryKeyChange propose accepted", Who::D(0), vec![rkc(RecoveryKeyMode::Propose, Some(np))], &d(), acc, None, true);
    let start = s.now - 1000;
    let other = s.new_recovery().public();
    ctl(&mut s, &mut cases, "RecoveryKeyChange propose with one pending", Who::D(1), vec![rkc(RecoveryKeyMode::Propose, Some(other))], &d(), rf, Some("seal/ctl_pending"), false);
    let open = start + 259_200_000;
    let at = |t| Spec { at_ms: Some(t), ..Spec::default() };
    let items = s.rotate_items(vec![rkc(RecoveryKeyMode::Confirm, Some(np))]);
    ctl(&mut s, &mut cases, "RecoveryKeyChange confirm by the proposer", Who::D(0), items.clone(), &at(open), rf, Some("seal/ctl_author"), false);
    ctl(&mut s, &mut cases, "RecoveryKeyChange confirm 1 ms before 72 hours", Who::D(1), items.clone(), &at(open - 1), rf, Some("seal/ctl_window"), false);
    let wrong = s.rotate_items(vec![rkc(RecoveryKeyMode::Confirm, Some(other))]);
    ctl(&mut s, &mut cases, "RecoveryKeyChange confirm with other keys", Who::D(1), wrong, &at(open), rf, Some("seal/ctl_target"), false);
    ctl(&mut s, &mut cases, "RecoveryKeyChange cancel accepted", Who::R, vec![rkc(RecoveryKeyMode::Cancel, None)], &d(), acc, None, false);
    let items = s.rotate_items(vec![rkc(RecoveryKeyMode::Confirm, Some(np))]);
    ctl(&mut s, &mut cases, "RecoveryKeyChange confirm at 72 hours, with the rotation to the new key", Who::D(1), items, &at(open), acc, None, true);
    s.rec.push(nk);

    let v2 = || rules(2, RulesUpgradeMode::Override);
    ctl(&mut s, &mut cases, "RulesUpgrade override: the first vote", Who::D(0), vec![v2()], &d(), acc, None, true);
    ctl(&mut s, &mut cases, "RulesUpgrade override: the same voter again", Who::D(0), vec![v2()], &d(), rf, Some("seal/ctl_duplicate"), false);
    ctl(&mut s, &mut cases, "RulesUpgrade override: a second voter activates (rules halt)", Who::D(1), vec![v2()], &d(), acc, Some("seal/unknown_rules"), false);
    let f = s.beat(Who::D(0), vec![1, 2], &d());
    s.push(f);
    ctl(&mut s, &mut cases, "RulesUpgrade normal: an active member lacks v", Who::D(0), vec![rules(2, RulesUpgradeMode::Normal)], &d(), rf, Some("seal/ctl_support"), false);
    for i in [1, 2] {
        let f = s.beat(Who::D(i), vec![1, 2], &d());
        s.push(f);
    }
    ctl(&mut s, &mut cases, "RulesUpgrade normal accepted (rules halt)", Who::D(0), vec![rules(2, RulesUpgradeMode::Normal)], &d(), acc, Some("seal/unknown_rules"), false);

    // Skip.
    let t = 4;
    let skip = |s: &Sim, t: u64| CtlItem::Skip(SkipItem { seq: t, frame_hash: frame_hash(&s.frames[t as usize - 1]) });
    let cmd = s.cmd(Who::D(2), &d());
    s.push(cmd);
    let t2 = s.seq();
    let f = s.ctl(Who::D(0), vec![skip(&s, t2)], &d());
    run(&s, &mut cases, "Skip: the target frame not supplied", &f, &Opts::default(), "NeedFrames", None);
    run(&s, &mut cases, "Skip accepted", &f, &Opts { targets: vec![t2], ..Opts::default() }, acc, None);
    let f = s.ctl(Who::D(2), vec![skip(&s, t2)], &d());
    run(&s, &mut cases, "Skip of the author's own frame", &f, &Opts { targets: vec![t2], ..Opts::default() }, rf, Some("seal/ctl_author"));
    let f = s.ctl(Who::D(0), vec![CtlItem::Skip(SkipItem { seq: t2, frame_hash: FrameHash([1; 32]) })], &d());
    run(&s, &mut cases, "Skip whose frame_hash is not the target's", &f, &Opts { targets: vec![t2], ..Opts::default() }, rf, Some("seal/ctl_target"));
    let f = s.ctl(Who::D(0), vec![skip(&s, t)], &d());
    run(&s, &mut cases, "Skip of a control entry", &f, &Opts::default(), rf, Some("seal/ctl_target"));

    // Revoke, reinstate, rotate.
    let me = s.dev(Who::D(0));
    let items = s.rotate_items(vec![CtlItem::DeviceRevoked(me)]);
    ctl(&mut s, &mut cases, "DeviceRevoked: a device revoking itself", Who::D(0), items, &d(), rf, Some("seal/ctl_author"), false);
    let items = s.rotate_items(vec![CtlItem::DeviceRevoked(DevId([9; 16]))]);
    ctl(&mut s, &mut cases, "DeviceRevoked: not a current device", Who::D(0), items, &d(), rf, Some("seal/ctl_target"), false);
    let rc = s.recipients_after(&[]);
    let r3 = s.rotation_to(s.m.epoch() + 2, &rc, None);
    ctl(&mut s, &mut cases, "EpochRotated to E + 2", Who::D(0), vec![CtlItem::EpochRotated(r3)], &d(), rf, Some("seal/ctl_epoch"), false);
    let fewer: Vec<_> = rc.iter().filter(|x| x.0 != s.dev(Who::D(2))).copied().collect();
    let r = s.rotation_to(s.m.epoch() + 1, &fewer, None);
    ctl(&mut s, &mut cases, "EpochRotated without a current device's wrap", Who::D(0), vec![CtlItem::EpochRotated(r)], &d(), rf, Some("seal/ctl_wraps"), false);
    let r = s.rotation_to(s.m.epoch() + 1, &rc, Some(3));
    ctl(&mut s, &mut cases, "EpochRotated with purge_before by the recovery identity", Who::R, vec![CtlItem::EpochRotated(r)], &d(), rf, Some("seal/ctl_author"), false);
    let next = s.seq() + 2;
    let r = s.rotation_to(s.m.epoch() + 1, &rc, Some(next));
    ctl(&mut s, &mut cases, "EpochRotated with purge_before after s", Who::D(0), vec![CtlItem::EpochRotated(r)], &d(), rf, Some("seal/ctl_target"), false);
    let r = s.rotation_to(s.m.epoch() + 1, &rc, Some(3));
    ctl(&mut s, &mut cases, "EpochRotated with purge_before accepted", Who::D(0), vec![CtlItem::EpochRotated(r)], &d(), acc, None, true);
    let d2 = s.dev(Who::D(2));
    let items = s.rotate_items(vec![CtlItem::DeviceRevoked(d2)]);
    ctl(&mut s, &mut cases, "DeviceRevoked with the rotation accepted", Who::D(1), items, &d(), acc, None, true);
    let items = s.rotate_items(vec![CtlItem::DeviceReinstated(d2)]);
    ctl(&mut s, &mut cases, "DeviceReinstated by a device", Who::D(0), items.clone(), &d(), rf, Some("seal/ctl_author"), false);
    ctl(&mut s, &mut cases, "DeviceReinstated by the recovery identity accepted", Who::R, items, &d(), acc, None, true);
    let rp = s.rec[0].public();
    let items = s.rotate_items(vec![rkc(RecoveryKeyMode::Direct, Some(rp))]);
    ctl(&mut s, &mut cases, "RecoveryKeyChange direct with a recorded key", Who::R, items, &d(), rf, Some("seal/ctl_duplicate_key"), false);
    let nk = s.new_recovery();
    let items = s.rotate_items(vec![rkc(RecoveryKeyMode::Direct, Some(nk.public()))]);
    ctl(&mut s, &mut cases, "RecoveryKeyChange direct accepted", Who::R, items, &d(), acc, None, false);

    // The limits: 16 current devices; 64 recovery records (a state whose
    // recovery list was filled, given as its canonical bytes); the last device.
    let mut s = Sim::new(64, 16);
    let k = s.new_device();
    let item = s.added_item(&k);
    ctl(&mut s, &mut cases, "DeviceAdded with 16 current devices", Who::D(0), vec![item], &d(), rf, Some("seal/ctl_device_cap"), false);
    let mut s = Sim::new(65, 1);
    let nk = s.new_recovery();
    let items = s.rotate_items(vec![rkc(RecoveryKeyMode::Direct, Some(nk.public()))]);
    let f = s.ctl(Who::R, items, &d());
    let mut inputs = s.inputs("RecoveryKeyChange direct with 64 recovery records", &f, &Opts::default());
    inputs["membership"] = json!(crate::common::hex(&with_recovery_records(&s.m, 63)));
    let r = replay(&inputs).unwrap();
    cases.push(case(inputs, &r, rf, Some("seal/ctl_cap")));
    let rc = s.recipients_after(&[]);
    let me = s.dev(Who::D(0));
    let r = s.rotation_to(2, &rc, None);
    ctl(&mut s, &mut cases, "DeviceRevoked of the last current device", Who::R, vec![CtlItem::DeviceRevoked(me), CtlItem::EpochRotated(r)], &d(), rf, Some("seal/ctl_last_device"), false);

    // The races of V2-8.
    let mut s = Sim::new(66, 3);
    let rc = s.recipients_after(&[]);
    let first = s.rotation_to(2, &rc, None);
    let staged = s.staged;
    let second = s.rotation_to(2, &rc, None);
    let f1 = s.ctl(Who::D(0), vec![CtlItem::EpochRotated(first)], &d());
    let f2 = s.ctl(Who::D(1), vec![CtlItem::EpochRotated(second)], &d());
    let k = s.new_device();
    let item = s.added_item(&k);
    let late_add = s.ctl(Who::D(2), vec![item], &d());
    let before = s.head();
    let stale_cmd = s.cmd(Who::D(2), &Spec { based_on: Some(before), ..d() });
    run(&s, &mut cases, "race: the first rotation for epoch 2", &f1, &Opts::default(), acc, None);
    s.staged = staged;
    s.push(f1);
    for own in [None, Some(1)] {
        let name = if own.is_some() { "race: the second rotation, on its author's device" } else { "race: the second rotation for epoch 2" };
        run(&s, &mut cases, name, &f2, &Opts { own, ..Opts::default() }, rf, Some("seal/stale_epoch"));
    }
    run(&s, &mut cases, "race: a DeviceAdded sealed before the rotation", &late_add, &Opts::default(), rf, Some("seal/stale_epoch"));
    run(&s, &mut cases, "race: a stale-epoch cmd within 30 days", &stale_cmd, &Opts::default(), acc, None);
    let clock = s.now;
    let f = s.cmd(Who::D(0), &Spec { at_ms: Some(clock + 2_592_000_001), ..d() });
    s.push(f);
    run(&s, &mut cases, "race: the same cmd after 30 days of log clock", &stale_cmd, &Opts::default(), rf, Some("seal/stale_epoch"));

    let doc = json!({ "spec": "seal.md §13", "cases": cases });
    for c in doc["cases"].as_array().unwrap() {
        check_case(c);
    }
    check_or_write("seal-membership.json", doc);
}

/// `m`'s canonical bytes with `n` retired recovery records after the
/// genesis record (`since_seq` 2 to `n + 1`, each retired at once).
fn with_recovery_records(m: &Membership, n: u16) -> Vec<u8> {
    let b = m.to_canonical_bytes();
    let host = b[84] as usize;
    let n_commit = 84 + 1 + host + 15;
    let commits = u32::from_be_bytes(b[n_commit..n_commit + 4].try_into().unwrap()) as usize;
    let n_rec = n_commit + 4 + 44 * commits;
    let count = u16::from_be_bytes([b[n_rec], b[n_rec + 1]]);
    let after_first = n_rec + 2 + 104;
    let mut out = b[..n_rec].to_vec();
    out.extend((count + n).to_be_bytes());
    out.extend(&b[n_rec + 2..after_first]);
    for i in 0..n {
        let mut rec = vec![0xC0; 16];
        rec[15] = i as u8;
        rec.extend([0xC1; 32]);
        rec[47] = i as u8;
        rec.extend([0xC2; 32]);
        rec[79] = i as u8;
        rec.extend((u64::from(i) + 2).to_be_bytes());
        rec.extend((u64::from(i) + 3).to_be_bytes());
        rec.extend(0u64.to_be_bytes());
        out.extend(rec);
    }
    out.extend(&b[after_first..]);
    assert!(Membership::from_canonical_bytes(&out).is_ok());
    out
}
