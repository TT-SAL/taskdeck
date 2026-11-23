//! The rows of seal.md §13.6 through `classify`: each row accepted with its
//! effects, each precondition failing once with its code, the author column,
//! the limits of §23, the two passes of §14.6, the recovery identity's
//! powers (§13.12), and the all-or-nothing rule.

use super::board::*;
use super::{only_counter, unchanged};
use crate::membership::{DeviceRecord, DeviceStatus, RecoveryRecord};
use crate::*;

fn fence(g: u32) -> CtlItem {
    CtlItem::Fence(Fence { host: "relay.example".into(), door: [1; 5], home_gen: g })
}

fn try_ctl(b: &mut Board, who: Who, items: Vec<CtlItem>) -> Receipt {
    let f = b.ctl(who, items, &Spec::default());
    b.classify(&f)
}

fn rkc(mode: RecoveryKeyMode, k: Option<RecoveryPublic>) -> CtlItem {
    CtlItem::RecoveryKeyChange(RecoveryKeyChange { mode, sign_pk: k.map(|k| k.sign_pk), hpke_pk: k.map(|k| k.hpke_pk) })
}

fn new_recovery() -> RecoveryKeys {
    RecoverySeed::generate(&mut OsEntropy).unwrap().keys()
}

fn fake_device(i: usize, status: DeviceStatus) -> DeviceRecord {
    let mut dev = [0xF0; 16];
    dev[14..].copy_from_slice(&u16::try_from(i).unwrap().to_be_bytes());
    let mut pk = [0xA0; 32];
    pk[30..].copy_from_slice(&u16::try_from(i).unwrap().to_be_bytes());
    let mut hk = [0xB0; 32];
    hk[30..].copy_from_slice(&u16::try_from(i).unwrap().to_be_bytes());
    DeviceRecord {
        dev: DevId(dev),
        sign_pk: pk,
        hpke_pk: hk,
        device_no: 100 + u16::try_from(i).unwrap(),
        status,
        added_seq: 1,
        added_by: DevId([0; 16]),
        bootstrap_seq: 1,
        status_seq: 1,
        status_by: DevId([0; 16]),
        last_ctr: 0,
        beat_ms: 0,
        rules: vec![1],
        envelopes: vec![(1, 1)],
        hash_versions: vec![1],
        name: "x".into(),
    }
}

fn sort_devices(m: &mut Membership) {
    m.devices.sort_by_key(|d| d.dev);
}

// ---------------------------------------------------------- DeviceAdded

// `DeviceAdded` by a current device: the record of §13.6 with `device_no =
// next_device_no`, which then grows by one.
// spec: SEAL-MEM-011
// spec: SEAL-MEM-013
#[test]
fn device_added_effects() {
    let mut b = Board::new(1);
    let k = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let item = b.added_item(&k);
    let s = b.seq() + 1;
    let at = b.now;
    let r = try_ctl(&mut b, Who::D(0), vec![item]);
    expect(&r, "Accept", None);
    let p = k.public();
    let d = r.membership.device(&p.dev).unwrap();
    assert_eq!((d.sign_pk, d.hpke_pk, d.device_no, d.added_seq, d.status_seq), (p.sign_pk, p.hpke_pk, 2, s, s));
    assert!(matches!(d.status, DeviceStatus::Current));
    assert_eq!((d.added_by, d.status_by, d.bootstrap_seq, d.last_ctr, d.beat_ms), (b.dev(Who::D(0)), b.dev(Who::D(0)), 1, 0, at));
    assert_eq!((d.rules.clone(), d.envelopes.clone(), d.hash_versions.clone(), d.name.as_str()), (vec![1], vec![(1, 1)], vec![1], "Phone"));
    assert_eq!(r.membership.next_device_no, 3);
    assert_eq!(r.membership.device_no(&p.dev), Some(2));
    // Every other field: as before, with the bookkeeping.
    let mut want = b.m.dup();
    want.devices.push(d.dup());
    sort_devices(&mut want);
    want.next_device_no = 3;
    want.set_last_ctr(&b.dev(Who::D(0)), 1);
    want.ctl_seqs.push(s);
    want.clock_ms = at;
    assert_eq!(r.membership.to_canonical_bytes(), want.to_canonical_bytes());
}

// A new `dev` already a device's or a `dev_R`, a `sign_pk` or `hpke_pk`
// already recorded: `seal/ctl_duplicate_key`.
// spec: SEAL-MEM-011
#[test]
fn device_added_duplicate_keys() {
    let mut b = Board::new(2);
    let fresh = DeviceKeys::generate(&mut OsEntropy).unwrap().public();
    let base = DeviceAdded { dev: fresh.dev, sign_pk: fresh.sign_pk, hpke_pk: fresh.hpke_pk, name: "N".into(), transcript: [1; 32], bootstrap_seq: 1 };
    let old = b.dev_pub(1);
    let rp = b.rec[0].public();
    for a in [
        DeviceAdded { dev: old.dev, ..base.clone() },
        DeviceAdded { dev: b.dev(Who::R), ..base.clone() },
        DeviceAdded { sign_pk: old.sign_pk, ..base.clone() },
        DeviceAdded { hpke_pk: old.hpke_pk, ..base.clone() },
        DeviceAdded { sign_pk: rp.sign_pk, ..base.clone() },
        DeviceAdded { hpke_pk: rp.hpke_pk, ..base.clone() },
    ] {
        let r = try_ctl(&mut b, Who::D(0), vec![CtlItem::DeviceAdded(a)]);
        expect(&r, "Refuse", Some("seal/ctl_duplicate_key"));
        only_counter(&b.m, &r, &b.dev(Who::D(0)), b.next_ctr(Who::D(0)));
    }
    expect(&try_ctl(&mut b, Who::D(0), vec![CtlItem::DeviceAdded(base)]), "Accept", None);
}

// 16 current devices, 256 device records, or `next_device_no` above 8191:
// `seal/ctl_device_cap`; `device_no` 8191 is still assigned.
// spec: SEAL-MEM-014
// spec: SEAL-MEM-011
#[test]
fn device_added_caps() {
    let mut b = Board::new(16);
    let k = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let item = b.added_item(&k);
    expect(&try_ctl(&mut b, Who::D(0), vec![item.clone()]), "Refuse", Some("seal/ctl_device_cap"));

    let mut b = Board::new(1);
    for i in 0..255 {
        b.m.devices.push(fake_device(i, DeviceStatus::Revoked));
    }
    sort_devices(&mut b.m);
    let item = b.added_item(&k);
    expect(&try_ctl(&mut b, Who::D(0), vec![item.clone()]), "Refuse", Some("seal/ctl_device_cap"));
    b.m.devices.retain(|d| d.dev.0[0] != 0xF0 || d.dev.0[15] != 0);
    expect(&try_ctl(&mut b, Who::D(0), vec![item.clone()]), "Accept", None);

    let mut b = Board::new(1);
    b.m.next_device_no = 8192;
    let item = b.added_item(&k);
    expect(&try_ctl(&mut b, Who::D(0), vec![item.clone()]), "Refuse", Some("seal/ctl_device_cap"));
    b.m.next_device_no = 8191;
    let r = try_ctl(&mut b, Who::D(0), vec![item]);
    expect(&r, "Accept", None);
    assert_eq!(r.membership.device_no(&k.public().dev), Some(8191));
    assert_eq!(r.membership.next_device_no, 8192);
}

// `bootstrap_seq` at or after the entry's seq, or below `purge_seq`:
// `seal/ctl_target`.
// spec: SEAL-MEM-011
#[test]
fn device_added_bootstrap() {
    let mut b = Board::new(1);
    for _ in 0..4 {
        let f = b.cmd(Who::D(0), &Spec::default());
        b.accept(f);
    }
    let k = DeviceKeys::generate(&mut OsEntropy).unwrap().public();
    let s = b.seq() + 1;
    let add = |bs| CtlItem::DeviceAdded(DeviceAdded { dev: k.dev, sign_pk: k.sign_pk, hpke_pk: k.hpke_pk, name: "N".into(), transcript: [1; 32], bootstrap_seq: bs });
    expect(&try_ctl(&mut b, Who::D(0), vec![add(s)]), "Refuse", Some("seal/ctl_target"));
    expect(&try_ctl(&mut b, Who::D(0), vec![add(s + 1)]), "Refuse", Some("seal/ctl_target"));
    b.m.purge_seq = 3;
    expect(&try_ctl(&mut b, Who::D(0), vec![add(2)]), "Refuse", Some("seal/ctl_target"));
    expect(&try_ctl(&mut b, Who::D(0), vec![add(3)]), "Accept", None);
    expect(&try_ctl(&mut b, Who::D(0), vec![add(s - 1)]), "Accept", None);
}

// `device_no` is assigned in log order and never reused after a revocation.
// spec: SEAL-MEM-013
#[test]
fn device_no_never_reused() {
    let mut b = Board::new(2);
    b.revoke(Who::D(0), 1);
    let i = b.add_device();
    assert_eq!(b.m.device_no(&b.dev(Who::D(1))), Some(2));
    assert_eq!(b.m.device_no(&b.dev(Who::D(i))), Some(3));
    assert_eq!(b.m.next_device_no, 4);
}

// -------------------------------------------------------- DeviceRevoked

// `DeviceRevoked` by a device or the recovery identity: status, seq and
// author recorded; a proposal and an override vote by the target cleared.
// spec: SEAL-MEM-011
// spec: SEAL-MEM-025
#[test]
fn revoke_effects() {
    for by in [Who::D(0), Who::R] {
        let mut b = Board::new(3);
        let k = new_recovery().public();
        let f = b.ctl(Who::D(2), vec![rkc(RecoveryKeyMode::Propose, Some(k))], &Spec::default());
        b.accept(f);
        let f = b.ctl(Who::D(2), vec![CtlItem::RulesUpgrade(RulesUpgrade { rules: 2, mode: RulesUpgradeMode::Override })], &Spec::default());
        b.accept(f);
        assert!(b.m.pending_recovery().is_some() && b.m.pending_override.is_some());
        let target = b.dev(Who::D(2));
        let s = b.seq() + 1;
        let items = b.rotate_items(vec![CtlItem::DeviceRevoked(target)]);
        let r = try_ctl(&mut b, by, items);
        expect(&r, "Accept", None);
        let d = r.membership.device(&target).unwrap();
        assert!(matches!(d.status, DeviceStatus::Revoked));
        assert_eq!((d.status_seq, d.status_by), (s, b.dev(by)));
        assert!(r.membership.pending_recovery().is_none());
        assert!(r.membership.pending_override.is_none());
    }
}

// Revoking a proposal's or vote's non-author leaves them.
// spec: SEAL-MEM-025
#[test]
fn revoke_other_keeps_proposal() {
    let mut b = Board::new(3);
    let k = new_recovery().public();
    let f = b.ctl(Who::D(2), vec![rkc(RecoveryKeyMode::Propose, Some(k))], &Spec::default());
    b.accept(f);
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(b.dev(Who::D(1)))]);
    let r = try_ctl(&mut b, Who::D(0), items);
    expect(&r, "Accept", None);
    assert!(r.membership.pending_recovery().is_some());
}

// A device revoking itself: `seal/ctl_author`; a target that is not a
// current device: `seal/ctl_target`; no current device left after the whole
// entry: `seal/ctl_last_device`.
// spec: SEAL-MEM-011
#[test]
fn revoke_refusals() {
    let mut b = Board::new(2);
    let me = b.dev(Who::D(0));
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(me)]);
    expect(&try_ctl(&mut b, Who::D(0), items), "Refuse", Some("seal/ctl_author"));
    let stranger = DevId([9; 16]);
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(stranger)]);
    expect(&try_ctl(&mut b, Who::D(0), items), "Refuse", Some("seal/ctl_target"));
    b.revoke(Who::D(0), 1);
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(b.dev(Who::D(1)))]);
    expect(&try_ctl(&mut b, Who::D(0), items), "Refuse", Some("seal/ctl_target"));
    // The recovery identity cannot revoke the last current device (the
    // rotation names the recipients before the revocation, two wraps).
    let rc = b.recipients_after(&[]);
    let items = vec![CtlItem::DeviceRevoked(me), CtlItem::EpochRotated(b.rotation_to(b.m.epoch + 1, &rc, None))];
    expect(&try_ctl(&mut b, Who::R, items), "Refuse", Some("seal/ctl_last_device"));
    // Revoking the last one while reinstating another in the same entry
    // leaves one current device.
    let d1 = b.dev(Who::D(1));
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(me), CtlItem::DeviceReinstated(d1)]);
    expect(&try_ctl(&mut b, Who::R, items), "Accept", None);
}

// Two revocations that together leave no current device: the first of
// them fails, with `seal/ctl_last_device`.
// spec: SEAL-MEM-011
// spec: SEAL-MEM-012
#[test]
fn revoke_all_in_one_entry() {
    let mut b = Board::new(2);
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(b.dev(Who::D(0))), CtlItem::DeviceRevoked(b.dev(Who::D(1)))]);
    // The rotation would wrap to the recovery key alone (one wrap): the
    // parser refuses fewer than two wraps, so the entry is built with the
    // wraps of the current state.
    let mut items = items;
    let rc = b.recipients_after(&[]);
    let r = b.rotation_to(2, &rc, None);
    *items.last_mut().unwrap() = CtlItem::EpochRotated(r);
    expect(&try_ctl(&mut b, Who::R, items), "Refuse", Some("seal/ctl_last_device"));
}

// ----------------------------------------------------- DeviceReinstated

// The recovery identity reinstates a device a device revoked: current again,
// `beat_ms = at_ms`, `last_ctr` unchanged. Refused: by a device
// (`seal/ctl_author`), a device the recovery identity revoked or a current
// one (`seal/ctl_target`), 16 current devices (`seal/ctl_device_cap`).
// spec: SEAL-MEM-011
// spec: SEAL-MEM-026
#[test]
fn reinstate_row() {
    let mut b = Board::new(3);
    let f = b.cmd(Who::D(2), &Spec::default());
    b.accept(f);
    b.revoke(Who::D(0), 2);
    let target = b.dev(Who::D(2));
    let items = b.rotate_items(vec![CtlItem::DeviceReinstated(target)]);
    expect(&try_ctl(&mut b, Who::D(1), items), "Refuse", Some("seal/ctl_author"));
    let items = b.rotate_items(vec![CtlItem::DeviceReinstated(b.dev(Who::D(1)))]);
    expect(&try_ctl(&mut b, Who::R, items), "Refuse", Some("seal/ctl_target"));
    let s = b.seq() + 1;
    let at = b.now;
    let items = b.rotate_items(vec![CtlItem::DeviceReinstated(target)]);
    let f = b.ctl(Who::R, items, &Spec::default());
    let r = b.accept(f);
    let d = r.membership.device(&target).unwrap();
    assert!(matches!(d.status, DeviceStatus::Current));
    assert_eq!((d.status_seq, d.status_by, d.beat_ms, d.last_ctr), (s, b.dev(Who::OldR(0)), at, 1));
    // Revoked by the recovery identity: not reinstatable.
    b.revoke(Who::R, 2);
    let items = b.rotate_items(vec![CtlItem::DeviceReinstated(target)]);
    expect(&try_ctl(&mut b, Who::R, items), "Refuse", Some("seal/ctl_target"));

    let mut b = Board::new(16);
    b.revoke(Who::D(0), 15);
    let back = b.add_device();
    assert!(back > 15);
    // Wrapping to 17 devices and the recovery key would exceed the 17 wraps
    // a rotation holds; the rotation names the 16 current devices.
    let rc = b.recipients_after(&[]);
    let items = vec![CtlItem::DeviceReinstated(b.dev(Who::D(15))), CtlItem::EpochRotated(b.rotation_to(b.m.epoch + 1, &rc, None))];
    expect(&try_ctl(&mut b, Who::R, items), "Refuse", Some("seal/ctl_device_cap"));
}

// --------------------------------------------------------- EpochRotated

// An accepted rotation: `epoch = n`, `epoch_seq = s`, `epoch_clock_ms =
// τ'`, `(n, s, commit)` appended; the next entries are under `n`.
// spec: SEAL-MEM-011
// spec: SEAL-MEM-017
#[test]
fn rotate_effects() {
    let mut b = Board::new(2);
    let f = b.cmd(Who::D(1), &Spec { at_ms: Some(T0 + 900_000), ..Spec::default() });
    b.accept(f);
    let s = b.seq() + 1;
    let items = b.rotate_items(vec![]);
    let CtlItem::EpochRotated(rot) = items[0].clone() else { panic!() };
    // An at_ms below the clock: τ' is the clock.
    let f = b.ctl(Who::D(0), items, &Spec { at_ms: Some(T0), ..Spec::default() });
    let r = b.accept(f);
    let m = &r.membership;
    assert_eq!((m.epoch, m.epoch_seq, m.epoch_clock_ms), (2, s, T0 + 900_000));
    assert_eq!(m.commitments.last(), Some(&(2, s, rot.commit)));
    let f = b.cmd(Who::D(1), &Spec::default());
    b.accept(f);
}

// An epoch other than `W.epoch + 1`: `seal/ctl_epoch`; wraps that do not
// name exactly the current devices and the current recovery identity:
// `seal/ctl_wraps`.
// spec: SEAL-MEM-015
// spec: SEAL-MEM-011
#[test]
fn rotate_refusals() {
    let mut b = Board::new(3);
    let rc = b.recipients_after(&[]);
    for e in [3, 7] {
        let r = b.rotation_to(e, &rc, None);
        expect(&try_ctl(&mut b, Who::D(0), vec![CtlItem::EpochRotated(r)]), "Refuse", Some("seal/ctl_epoch"));
    }
    // One recipient missing, a stranger added, the recovery key missing.
    let stranger = (DevId([0x99; 16]), DeviceKeys::generate(&mut OsEntropy).unwrap().public().hpke_pk);
    let without_one: Vec<_> = rc.iter().filter(|x| x.0 != b.dev(Who::D(2))).copied().collect();
    let with_stranger: Vec<_> = rc.iter().copied().chain([stranger]).collect();
    let without_r: Vec<_> = rc.iter().filter(|x| x.0 != b.dev(Who::R)).copied().collect();
    for set in [without_one, with_stranger, without_r] {
        let r = b.rotation_to(2, &set, None);
        expect(&try_ctl(&mut b, Who::D(0), vec![CtlItem::EpochRotated(r)]), "Refuse", Some("seal/ctl_wraps"));
    }
    // A revoked device keeps no wrap: wrapping to it is refused.
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(b.dev(Who::D(2)))]);
    let r = b.rotation_to(2, &rc, None);
    let bad = vec![CtlItem::DeviceRevoked(b.dev(Who::D(2))), CtlItem::EpochRotated(r)];
    expect(&try_ctl(&mut b, Who::D(0), bad), "Refuse", Some("seal/ctl_wraps"));
    expect(&try_ctl(&mut b, Who::D(0), items), "Accept", None);
}

// `purge_before` by a device within `W.purge_seq < p ≤ s` sets `purge_seq`;
// outside, `seal/ctl_target`; by the recovery identity, `seal/ctl_author`.
// spec: SEAL-MEM-011
// spec: SEAL-MEM-026
#[test]
fn rotate_purge() {
    let mut b = Board::new(2);
    for _ in 0..3 {
        let f = b.cmd(Who::D(0), &Spec::default());
        b.accept(f);
    }
    let s = b.seq() + 1;
    let rc = b.recipients_after(&[]);
    let r = b.rotation_to(2, &rc, Some(s + 1));
    expect(&try_ctl(&mut b, Who::D(0), vec![CtlItem::EpochRotated(r)]), "Refuse", Some("seal/ctl_target"));
    let r = b.rotation_to(2, &rc, Some(3));
    expect(&try_ctl(&mut b, Who::R, vec![CtlItem::EpochRotated(r)]), "Refuse", Some("seal/ctl_author"));
    let r = b.rotation_to(2, &rc, Some(s));
    let f = b.ctl(Who::D(0), vec![CtlItem::EpochRotated(r)], &Spec::default());
    let r = b.accept(f);
    assert_eq!(r.membership.purge_seq, s);
    // Not above the current purge_seq: refused.
    let rc = b.recipients_after(&[]);
    let r = b.rotation_to(3, &rc, Some(s));
    expect(&try_ctl(&mut b, Who::D(0), vec![CtlItem::EpochRotated(r)]), "Refuse", Some("seal/ctl_target"));
}

// The commitments list at its limit (65,535 epochs): `seal/ctl_cap`.
// spec: SEAL-MEM-011
#[test]
fn rotate_cap() {
    let mut b = Board::new(1);
    let root = EpochRoot::from_bytes([0x31; 32]);
    let e = 65_535u32;
    b.m.commitments = (1..e).map(|i| (i, 1, [0u8; 32])).collect();
    b.m.commitments.push((e, 1, root.commitment(&b.board, e)));
    b.m.epoch = e;
    b.roots.insert(e, root);
    let rc = b.recipients_after(&[]);
    let r = b.rotation_to(e + 1, &rc, None);
    expect(&try_ctl(&mut b, Who::D(0), vec![CtlItem::EpochRotated(r)]), "Refuse", Some("seal/ctl_cap"));
    b.m.commitments.remove(0);
    let r = b.rotation_to(e + 1, &rc, None);
    expect(&try_ctl(&mut b, Who::D(0), vec![CtlItem::EpochRotated(r)]), "Accept", None);
}

// ---------------------------------------------------- RecoveryKeyChange

// `direct` by the recovery identity (a K-shape entry, whose rotation the
// old identity authors: the class is decided from `M_{s−1}`): the old record
// retires at `s`, the new one starts at `s` with counter 0; the new identity
// authors from `s + 1` with counter 1, the old one is refused.
// spec: SEAL-MEM-011
// spec: SEAL-KEY-007
// spec: SEAL-MEM-007
#[test]
fn recovery_direct() {
    let mut b = Board::new(2);
    let nk = new_recovery();
    let np = nk.public();
    let s = b.seq() + 1;
    let items = b.rotate_items(vec![rkc(RecoveryKeyMode::Direct, Some(np))]);
    let f = b.ctl(Who::R, items, &Spec::default());
    let r = b.accept(f);
    b.rec.push(nk);
    let m = &r.membership;
    assert_eq!(m.recovery.len(), 2);
    let old = &m.recovery[0];
    assert_eq!((old.since_seq, old.until_seq, old.last_ctr), (1, s, 2));
    let cur = m.current_recovery();
    assert_eq!((cur.dev, cur.sign_pk, cur.hpke_pk, cur.since_seq, cur.until_seq, cur.last_ctr), (np.dev(&b.board), np.sign_pk, np.hpke_pk, s, 0, 0));
    let f = b.ctl(Who::R, vec![fence(2)], &Spec { ctr: Some(2), ..Spec::default() });
    expect(&b.classify(&f), "Tamper", Some("seal/counter_gap"));
    let f = b.ctl(Who::OldR(0), vec![fence(2)], &Spec::default());
    expect(&b.classify(&f), "Refuse", Some("seal/revoked_author"));
    let f = b.ctl(Who::R, vec![fence(2)], &Spec::default());
    let r = b.accept(f);
    assert_eq!(r.membership.current_recovery().last_ctr, 1);
}

// `direct` by a device: `seal/ctl_author`; with keys already recorded:
// `seal/ctl_duplicate_key`; 64 recovery records: `seal/ctl_cap`.
// spec: SEAL-MEM-011
// spec: SEAL-MEM-026
#[test]
fn recovery_direct_refusals() {
    let mut b = Board::new(2);
    let np = new_recovery().public();
    let items = b.rotate_items(vec![rkc(RecoveryKeyMode::Direct, Some(np))]);
    expect(&try_ctl(&mut b, Who::D(0), items), "Refuse", Some("seal/ctl_author"));
    let d = b.dev_pub(1);
    for k in [
        RecoveryPublic { sign_pk: d.sign_pk, hpke_pk: np.hpke_pk },
        RecoveryPublic { sign_pk: np.sign_pk, hpke_pk: d.hpke_pk },
        b.rec[0].public(),
    ] {
        let items = b.rotate_items(vec![rkc(RecoveryKeyMode::Direct, Some(k))]);
        expect(&try_ctl(&mut b, Who::R, items), "Refuse", Some("seal/ctl_duplicate_key"));
    }
    let cur = b.m.recovery.remove(0);
    for i in 0..63u8 {
        b.m.recovery.push(RecoveryRecord { dev: DevId([i; 16]), sign_pk: [i; 32], hpke_pk: [i; 32], since_seq: 0, until_seq: 1, last_ctr: 0 });
    }
    b.m.recovery.push(cur);
    let items = b.rotate_items(vec![rkc(RecoveryKeyMode::Direct, Some(np))]);
    expect(&try_ctl(&mut b, Who::R, items), "Refuse", Some("seal/ctl_cap"));
    b.m.recovery.remove(0);
    let items = b.rotate_items(vec![rkc(RecoveryKeyMode::Direct, Some(np))]);
    expect(&try_ctl(&mut b, Who::R, items), "Accept", None);
}

// `propose`, then `confirm` by another device 72 hours of `start_ms` later
// (the log clock after the proposal, not the proposer's `at_ms`), with a
// rotation to the new key; every refusal of the three rows once.
// spec: SEAL-MEM-011
// spec: SEAL-MEM-023
// spec: SEAL-MEM-024
// spec: SEAL-MEM-026
#[test]
fn recovery_propose_confirm() {
    let mut b = Board::new(3);
    let nk = new_recovery();
    let np = nk.public();
    // Confirm and cancel without a proposal.
    let items = b.rotate_items(vec![rkc(RecoveryKeyMode::Confirm, Some(np))]);
    expect(&try_ctl(&mut b, Who::D(1), items), "Refuse", Some("seal/ctl_no_pending"));
    expect(&try_ctl(&mut b, Who::D(1), vec![rkc(RecoveryKeyMode::Cancel, None)]), "Refuse", Some("seal/ctl_no_pending"));
    // Propose by the recovery identity, with recorded keys.
    expect(&try_ctl(&mut b, Who::R, vec![rkc(RecoveryKeyMode::Propose, Some(np))]), "Refuse", Some("seal/ctl_author"));
    let dup = RecoveryPublic { sign_pk: b.dev_pub(2).sign_pk, hpke_pk: np.hpke_pk };
    expect(&try_ctl(&mut b, Who::D(0), vec![rkc(RecoveryKeyMode::Propose, Some(dup))]), "Refuse", Some("seal/ctl_duplicate_key"));
    // A clock ahead of the proposer's at_ms: start_ms is the clock.
    let f = b.cmd(Who::D(2), &Spec { at_ms: Some(T0 + 5_000_000), ..Spec::default() });
    b.accept(f);
    let s = b.seq() + 1;
    let f = b.ctl(Who::D(0), vec![rkc(RecoveryKeyMode::Propose, Some(np))], &Spec { at_ms: Some(T0), ..Spec::default() });
    let r = b.accept(f);
    let p = r.membership.pending_recovery().unwrap();
    assert_eq!((p.proposer, p.sign_pk, p.hpke_pk, p.seq, p.start_ms), (b.dev(Who::D(0)), np.sign_pk, np.hpke_pk, s, T0 + 5_000_000));
    let start = T0 + 5_000_000;
    // A second proposal.
    let other = new_recovery().public();
    expect(&try_ctl(&mut b, Who::D(1), vec![rkc(RecoveryKeyMode::Propose, Some(other))]), "Refuse", Some("seal/ctl_pending"));
    let at = |t| Spec { at_ms: Some(t), ..Spec::default() };
    let confirm = |b: &mut Board, who, k, t| {
        let items = b.rotate_items(vec![rkc(RecoveryKeyMode::Confirm, Some(k))]);
        let f = b.ctl(who, items, &at(t));
        b.classify(&f)
    };
    let open = start + 259_200_000;
    expect(&confirm(&mut b, Who::D(0), np, open), "Refuse", Some("seal/ctl_author"));
    expect(&confirm(&mut b, Who::R, np, open), "Refuse", Some("seal/ctl_author"));
    expect(&confirm(&mut b, Who::D(1), other, open), "Refuse", Some("seal/ctl_target"));
    expect(&confirm(&mut b, Who::D(1), np, open - 1), "Refuse", Some("seal/ctl_window"));
    let s = b.seq() + 1;
    let r = confirm(&mut b, Who::D(1), np, open);
    expect(&r, "Accept", None);
    assert!(r.membership.pending_recovery().is_none());
    let cur = r.membership.current_recovery();
    assert_eq!((cur.sign_pk, cur.since_seq, cur.last_ctr), (np.sign_pk, s, 0));
    assert_eq!(r.membership.recovery[0].until_seq, s);
}

// `cancel` by a device or by the recovery identity clears the proposal.
// spec: SEAL-MEM-011
#[test]
fn recovery_cancel() {
    for by in [Who::D(1), Who::R, Who::D(0)] {
        let mut b = Board::new(2);
        let np = new_recovery().public();
        let f = b.ctl(Who::D(0), vec![rkc(RecoveryKeyMode::Propose, Some(np))], &Spec::default());
        b.accept(f);
        let r = try_ctl(&mut b, by, vec![rkc(RecoveryKeyMode::Cancel, None)]);
        expect(&r, "Accept", None);
        assert!(r.membership.pending_recovery().is_none());
    }
}

// ---------------------------------------------------------------- Fence

// `Fence` with the next home generation: `home_gen`, `home_seq`, host and
// door recorded; any other generation: `seal/ctl_home_gen`.
// spec: SEAL-MEM-011
#[test]
fn fence_row() {
    let mut b = Board::new(1);
    for g in [3, 4] {
        expect(&try_ctl(&mut b, Who::D(0), vec![fence(g)]), "Refuse", Some("seal/ctl_home_gen"));
    }
    let s = b.seq() + 1;
    for who in [Who::D(0), Who::R] {
        let r = try_ctl(&mut b, who, vec![fence(2)]);
        expect(&r, "Accept", None);
        let m = &r.membership;
        assert_eq!((m.home_gen, m.home_seq, m.home_host.as_str(), m.home_door), (2, s, "relay.example", [1; 5]));
    }
}

// -------------------------------------------------------------- ResetTo

// `ResetTo` changes only `last_reset`; a target at or after `s`, below
// `purge_seq`, at or below a current device's `bootstrap_seq`, or a hash
// version rules 1 does not allow: `seal/ctl_target`; by the recovery
// identity: `seal/ctl_author`.
// spec: SEAL-MEM-011
// spec: SEAL-MEM-027
// spec: SEAL-MEM-026
#[test]
fn reset_to_row() {
    let mut b = Board::new(2);
    for _ in 0..5 {
        let f = b.cmd(Who::D(0), &Spec::default());
        b.accept(f);
    }
    let s = b.seq() + 1;
    let reset = |t, hv| CtlItem::ResetTo(ResetTo { seq: t, state_hash: [6; 32], hash_version: hv });
    // Device 1 was added at seq 2 with bootstrap_seq 1.
    for (t, hv) in [(s, 1), (s + 3, 1), (1, 1), (4, 2)] {
        expect(&try_ctl(&mut b, Who::D(0), vec![reset(t, hv)]), "Refuse", Some("seal/ctl_target"));
    }
    b.m.purge_seq = 5;
    expect(&try_ctl(&mut b, Who::D(0), vec![reset(4, 1)]), "Refuse", Some("seal/ctl_target"));
    expect(&try_ctl(&mut b, Who::R, vec![reset(5, 1)]), "Refuse", Some("seal/ctl_author"));
    let before = b.m.dup();
    let r = try_ctl(&mut b, Who::D(0), vec![reset(5, 1)]);
    expect(&r, "Accept", None);
    let mut want = before.dup();
    want.last_reset = Some((s, 5, 1, [6; 32]));
    want.set_last_ctr(&b.dev(Who::D(0)), b.next_ctr(Who::D(0)));
    want.clock_ms = b.now;
    want.ctl_seqs.push(s);
    assert_eq!(r.membership.to_canonical_bytes(), want.to_canonical_bytes());
}

// ----------------------------------------------------------------- Skip

fn skip(b: &Board, t: u64) -> CtlItem {
    CtlItem::Skip(SkipItem { seq: t, frame_hash: frame_hash(&b.frames[t as usize - 1]) })
}

// `Skip` appends its targets to `skips` and changes nothing else; a missing
// target frame gives `NeedFrames` with every missing target and no change;
// a supplied frame with another hash, or the device's own frame, refuses.
// spec: SEAL-MEM-011
// spec: SEAL-MEM-027
// spec: SEAL-RCPT-009
#[test]
fn skip_row() {
    let mut b = Board::new(2);
    for i in 0..6 {
        let f = b.cmd(Who::D(i % 2), &Spec::default());
        b.accept(f);
    }
    // seqs 3..8 are cmds: 3, 5, 7 by device 0; 4, 6, 8 by device 1.
    let s = b.seq() + 1;
    let f = b.ctl(Who::D(1), vec![skip(&b, 3), skip(&b, 5), skip(&b, 7)], &Spec::default());
    let hide = Opts { hide_frames: vec![5, 7], ..Opts::default() };
    let r = b.classify_with(&f, &hide).unwrap();
    let Class::NeedFrames(ref need) = r.class else { panic!("{}", class_name(&r)) };
    assert_eq!(need, &vec![(5, frame_hash(&b.frames[4])), (7, frame_hash(&b.frames[6]))]);
    assert_eq!(r.code, None);
    unchanged(&b.m, &r);
    let before = b.m.dup();
    let r = b.classify(&f);
    expect(&r, "Accept", None);
    let mut want = before.dup();
    want.skips = vec![(3, frame_hash(&b.frames[2])), (5, frame_hash(&b.frames[4])), (7, frame_hash(&b.frames[6]))];
    want.set_last_ctr(&b.dev(Who::D(1)), b.next_ctr(Who::D(1)));
    want.clock_ms = b.now;
    want.ctl_seqs.push(s);
    assert_eq!(r.membership.to_canonical_bytes(), want.to_canonical_bytes());
    // A wrong hash; the author's own frame; the recovery identity may skip any.
    let wrong = CtlItem::Skip(SkipItem { seq: 3, frame_hash: FrameHash([1; 32]) });
    expect(&try_ctl(&mut b, Who::D(1), vec![wrong]), "Refuse", Some("seal/ctl_target"));
    let items = vec![skip(&b, 3), skip(&b, 4)];
    expect(&try_ctl(&mut b, Who::D(1), items), "Refuse", Some("seal/ctl_author"));
    let items = vec![skip(&b, 3), skip(&b, 4)];
    expect(&try_ctl(&mut b, Who::R, items), "Accept", None);
}

// The preconditions that need no frame refuse before any frame is asked
// for: a target at or after `s`, below `purge_seq`, below `s − 16,384`, in
// `ctl_seqs` or already in `skips` is `seal/ctl_target` with the frames
// missing as with them.
// spec: SEAL-RCPT-009
// spec: SEAL-MEM-011
#[test]
fn skip_targets_without_frames() {
    let mut b = Board::new(2);
    for _ in 0..8 {
        let f = b.cmd(Who::D(0), &Spec::default());
        b.accept(f);
    }
    let f = b.ctl(Who::D(1), vec![skip(&b, 3)], &Spec::default());
    b.accept(f);
    let all_hidden: Vec<u64> = (1..=b.seq()).collect();
    let hidden = Opts { hide_frames: all_hidden.clone(), ..Opts::default() };
    let s = b.seq() + 1;
    let ahead = CtlItem::Skip(SkipItem { seq: s, frame_hash: FrameHash([1; 32]) });
    for items in [vec![skip(&b, 4), ahead], vec![skip(&b, 2)], vec![skip(&b, 4), skip(&b, 3)]] {
        let f = b.ctl(Who::D(1), items, &Spec::default());
        expect(&b.classify_with(&f, &hidden).unwrap(), "Refuse", Some("seal/ctl_target"));
        expect(&b.classify(&f), "Refuse", Some("seal/ctl_target"));
    }
    b.m.purge_seq = 5;
    let f = b.ctl(Who::D(1), vec![skip(&b, 4)], &Spec::default());
    expect(&b.classify_with(&f, &hidden).unwrap(), "Refuse", Some("seal/ctl_target"));
    let f = b.ctl(Who::D(1), vec![skip(&b, 5)], &Spec::default());
    let r = b.classify_with(&f, &hidden).unwrap();
    assert!(matches!(r.class, Class::NeedFrames(_)));
    b.m.purge_seq = 0;
    // The Skip window: at seq 16,390 the floor is 6.
    let far = |seq| Opts { seq: Some(seq), ..Opts::default() };
    let f = b.ctl(Who::D(1), vec![skip(&b, 5)], &Spec::default());
    expect(&b.classify_with(&f, &far(16_390)).unwrap(), "Refuse", Some("seal/ctl_target"));
    let f = b.ctl(Who::D(1), vec![skip(&b, 6)], &Spec::default());
    expect(&b.classify_with(&f, &far(16_390)).unwrap(), "Accept", None);
    let f = b.ctl(Who::D(1), vec![skip(&b, 5)], &Spec::default());
    expect(&b.classify_with(&f, &far(16_389)).unwrap(), "Accept", None);
}

// `skips` at its limit (65,535): `seal/ctl_cap`.
// spec: SEAL-MEM-011
#[test]
fn skip_cap() {
    let mut b = Board::new(2);
    let f = b.cmd(Who::D(0), &Spec::default());
    b.accept(f);
    b.m.skips = (0..65_535u64).map(|i| (1_000_000 + i, FrameHash([0; 32]))).collect();
    let items = vec![skip(&b, 3)];
    expect(&try_ctl(&mut b, Who::D(1), items), "Refuse", Some("seal/ctl_cap"));
    b.m.skips.pop();
    let items = vec![skip(&b, 3)];
    expect(&try_ctl(&mut b, Who::D(1), items), "Accept", None);
}

// --------------------------------------------------------- RulesUpgrade

fn beat_at(b: &mut Board, who: Who, rules: Vec<u32>, at: i64) {
    let f = b.beat(who, rules, &Spec { at_ms: Some(at), ..Spec::default() });
    b.accept(f);
}

// Active members (§13.10): the reference is the newest beat among the
// current devices other than the author; a device whose beat is older than
// 30 days before it is not active, and its rules do not block `normal`. An
// active one without `v` refuses with `seal/ctl_support`; the author's own
// clock cannot make the others inactive.
// spec: SEAL-MEM-019
// spec: SEAL-MEM-020
// spec: SEAL-MEM-011
#[test]
fn rules_normal() {
    let day = 86_400_000;
    let mut b = Board::new(3);
    let up = || CtlItem::RulesUpgrade(RulesUpgrade { rules: 2, mode: RulesUpgradeMode::Normal });
    beat_at(&mut b, Who::D(0), vec![1, 2], T0 + 40 * day);
    beat_at(&mut b, Who::D(1), vec![1, 2], T0 + 40 * day);
    // Device 2's beat_ms is T0 (its DeviceAdded), 40 days before the
    // reference: not active.
    let active = crate::membership::transitions::active_members(&b.m, &b.dev(Who::D(0)));
    assert_eq!(active.len(), 2);
    let r = try_ctl(&mut b, Who::D(0), vec![up()]);
    expect(&r, "Accept", Some("seal/unknown_rules"));
    assert_eq!(r.membership.rules, 2);
    // Within 30 days, it blocks.
    beat_at(&mut b, Who::D(2), vec![1], T0 + 11 * day);
    expect(&try_ctl(&mut b, Who::D(0), vec![up()]), "Refuse", Some("seal/ctl_support"));
    // The author's own far beat is not the reference.
    beat_at(&mut b, Who::D(0), vec![1, 2], T0 + 400 * day);
    beat_at(&mut b, Who::D(1), vec![1], T0 + 12 * day);
    let r = try_ctl(&mut b, Who::D(0), vec![up()]);
    expect(&r, "Refuse", Some("seal/ctl_support"));
    // The author itself lacking v.
    beat_at(&mut b, Who::D(1), vec![1, 2], T0 + 400 * day);
    beat_at(&mut b, Who::D(2), vec![1, 2], T0 + 400 * day);
    beat_at(&mut b, Who::D(0), vec![1], T0 + 400 * day);
    expect(&try_ctl(&mut b, Who::D(0), vec![up()]), "Refuse", Some("seal/ctl_support"));
    let r = try_ctl(&mut b, Who::D(1), vec![up()]);
    expect(&r, "Refuse", Some("seal/ctl_support"));
    // v not above the rules: refused; by the recovery identity: refused.
    let same = CtlItem::RulesUpgrade(RulesUpgrade { rules: 2, mode: RulesUpgradeMode::Normal });
    expect(&try_ctl(&mut b, Who::R, vec![same]), "Refuse", Some("seal/ctl_author"));
}

// `v ≤ W.rules` is `seal/ctl_target` in both modes. Under a rules version
// above 1 this build allows no envelope (step 8 refuses every frame), so
// the row is tested on the transition itself.
// spec: SEAL-MEM-011
#[test]
fn rules_not_above() {
    use crate::membership::transitions::{Auth, CtlOutcome, EntryCx, apply};
    use crate::payloads::Shape;
    let b = Board::new(1);
    let skip = |_: u64| None;
    let cx = EntryCx { s: 5, at_ms: T0, tau: T0, author: Auth::Device(b.dev(Who::D(0))), board: b.board, skip_targets: &skip };
    for mode in [RulesUpgradeMode::Normal, RulesUpgradeMode::Override] {
        for (rules, v) in [(3, 2), (3, 3)] {
            let mut w = b.m.dup();
            w.rules = rules;
            let r = apply(&mut w, Shape::U, &[CtlItem::RulesUpgrade(RulesUpgrade { rules: v, mode })], &cx);
            assert!(matches!(r, CtlOutcome::Refused(SealError::CtlTarget)));
        }
        let mut w = b.m.dup();
        w.devices[0].rules = vec![1, 4];
        w.rules = 3;
        let r = apply(&mut w, Shape::U, &[CtlItem::RulesUpgrade(RulesUpgrade { rules: 4, mode })], &cx);
        assert!(matches!(r, CtlOutcome::Applied { .. }));
    }
}

// `normal` clears a pending override vote and activates `v`; the beat
// updates its author's lists and `beat_ms = max(beat_ms, at_ms)`.
// spec: SEAL-MEM-011
// spec: SEAL-MEM-019
#[test]
fn rules_normal_clears_vote_and_beat_fields() {
    let mut b = Board::new(1);
    let f = b.beat(Who::D(0), vec![1, 2, 3], &Spec::default());
    let r = b.accept(f);
    let d = &r.membership.devices()[0];
    assert_eq!((d.rules.clone(), d.beat_ms), (vec![1, 2, 3], b.now - 1000));
    let f = b.beat(Who::D(0), vec![1, 2, 3], &Spec { at_ms: Some(T0 - 5), ..Spec::default() });
    let r = b.accept(f);
    assert_eq!(r.membership.devices()[0].beat_ms, b.now - 2000);
    let f = b.ctl(Who::D(0), vec![CtlItem::RulesUpgrade(RulesUpgrade { rules: 3, mode: RulesUpgradeMode::Override })], &Spec::default());
    b.accept(f);
    assert!(b.m.pending_override.is_some());
    let r = try_ctl(&mut b, Who::D(0), vec![CtlItem::RulesUpgrade(RulesUpgrade { rules: 2, mode: RulesUpgradeMode::Normal })]);
    expect(&r, "Accept", Some("seal/unknown_rules"));
    assert!(r.membership.pending_override.is_none());
}

// `override`: a first vote is recorded; a second vote by the same voter for
// the same `v` is `seal/ctl_duplicate`; a vote by a different current device
// activates `v`; a vote whose voter was revoked no longer counts.
// spec: SEAL-MEM-021
// spec: SEAL-MEM-011
#[test]
fn rules_override() {
    let mut b = Board::new(3);
    let vote = |v| CtlItem::RulesUpgrade(RulesUpgrade { rules: v, mode: RulesUpgradeMode::Override });
    let s = b.seq() + 1;
    let f = b.ctl(Who::D(0), vec![vote(2)], &Spec::default());
    let r = b.accept(f);
    assert_eq!(r.membership.pending_override, Some((2, b.dev(Who::D(0)), s)));
    assert_eq!(r.membership.rules, 1);
    expect(&try_ctl(&mut b, Who::D(0), vec![vote(2)]), "Refuse", Some("seal/ctl_duplicate"));
    // Another v replaces the vote.
    let r = try_ctl(&mut b, Who::D(1), vec![vote(3)]);
    expect(&r, "Accept", None);
    assert_eq!(r.membership.pending_override.map(|p| p.0), Some(3));
    let r = try_ctl(&mut b, Who::D(1), vec![vote(2)]);
    expect(&r, "Accept", Some("seal/unknown_rules"));
    assert_eq!((r.membership.rules, r.membership.pending_override), (2, None));
    // A revoked voter's vote is cleared (§13.10): device 2 votes alone after.
    b.revoke(Who::D(1), 0);
    assert!(b.m.pending_override.is_none());
    let f = b.ctl(Who::D(1), vec![vote(2)], &Spec::default());
    b.accept(f);
    b.m.devices.iter_mut().find(|d| d.dev == b.devs[1].public().dev).unwrap().status = DeviceStatus::Revoked;
    let r = try_ctl(&mut b, Who::D(2), vec![vote(2)]);
    expect(&r, "Accept", None);
    assert_eq!(r.membership.pending_override.map(|p| p.1), Some(b.dev(Who::D(2))));
}

// ------------------------------------------------------- whole entries

// All or nothing: an `R`-shape entry whose second revocation fails is
// refused with that item's code, and the first revocation does not apply;
// with two failing items, the first one's code.
// spec: SEAL-MEM-010
// spec: SEAL-MEM-012
#[test]
fn all_or_nothing() {
    let mut b = Board::new(3);
    let d1 = b.dev(Who::D(1));
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(d1), CtlItem::DeviceRevoked(DevId([7; 16]))]);
    let r = try_ctl(&mut b, Who::D(0), items);
    expect(&r, "Refuse", Some("seal/ctl_target"));
    only_counter(&b.m, &r, &b.dev(Who::D(0)), b.next_ctr(Who::D(0)));
    let me = b.dev(Who::D(0));
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(me), CtlItem::DeviceRevoked(DevId([7; 16]))]);
    expect(&try_ctl(&mut b, Who::D(0), items), "Refuse", Some("seal/ctl_author"));
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(DevId([7; 16])), CtlItem::DeviceRevoked(me)]);
    expect(&try_ctl(&mut b, Who::D(0), items), "Refuse", Some("seal/ctl_target"));
    // A K-shape whose rotation fails after a good confirm: nothing applies.
    let np = new_recovery().public();
    let rc = b.recipients_after(&[]);
    let r = b.rotation_to(2, &rc, None);
    let r = try_ctl(&mut b, Who::R, vec![rkc(RecoveryKeyMode::Direct, Some(np)), CtlItem::EpochRotated(r)]);
    expect(&r, "Refuse", Some("seal/ctl_wraps"));
    assert_eq!(r.membership.recovery.len(), 1);
}

// `ctl_seqs` at its limit (1,048,576): a control entry is `seal/ctl_cap`.
// spec: SEAL-MEM-011
#[test]
fn ctl_seqs_cap() {
    let mut b = Board::new(1);
    b.m.ctl_seqs = (0..1_048_576u64).collect();
    expect(&try_ctl(&mut b, Who::D(0), vec![fence(2)]), "Refuse", Some("seal/ctl_cap"));
    b.m.ctl_seqs.pop();
    expect(&try_ctl(&mut b, Who::D(0), vec![fence(2)]), "Accept", None);
}

// The recovery identity's powers (§13.12): allowed `DeviceRevoked`,
// `DeviceReinstated`, `EpochRotated`, `RecoveryKeyChange` `direct` and
// `cancel`, `Fence`, `Skip`; refused with `seal/ctl_author`: `DeviceAdded`,
// `propose`, `confirm`, `ResetTo`, `RulesUpgrade`, `purge_before`.
// spec: SEAL-MEM-026
#[test]
fn recovery_powers() {
    let mut b = Board::new(3);
    let f = b.cmd(Who::D(0), &Spec::default());
    b.accept(f);
    let k = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let np = new_recovery().public();
    let added = b.added_item(&k);
    let refused = vec![
        vec![added],
        vec![rkc(RecoveryKeyMode::Propose, Some(np))],
        vec![CtlItem::ResetTo(ResetTo { seq: 2, state_hash: [0; 32], hash_version: 1 })],
        vec![CtlItem::RulesUpgrade(RulesUpgrade { rules: 2, mode: RulesUpgradeMode::Normal })],
        vec![CtlItem::RulesUpgrade(RulesUpgrade { rules: 2, mode: RulesUpgradeMode::Override })],
    ];
    for items in refused {
        expect(&try_ctl(&mut b, Who::R, items), "Refuse", Some("seal/ctl_author"));
    }
    let f = b.ctl(Who::D(0), vec![rkc(RecoveryKeyMode::Propose, Some(np))], &Spec::default());
    b.accept(f);
    let items = b.rotate_items(vec![rkc(RecoveryKeyMode::Confirm, Some(np))]);
    expect(&try_ctl(&mut b, Who::R, items), "Refuse", Some("seal/ctl_author"));
    // Allowed.
    expect(&try_ctl(&mut b, Who::R, vec![rkc(RecoveryKeyMode::Cancel, None)]), "Accept", None);
    expect(&try_ctl(&mut b, Who::R, vec![fence(2)]), "Accept", None);
    let items = vec![skip(&b, 4)];
    expect(&try_ctl(&mut b, Who::R, items), "Accept", None);
    let items = b.rotate_items(vec![]);
    expect(&try_ctl(&mut b, Who::R, items), "Accept", None);
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(b.dev(Who::D(2)))]);
    expect(&try_ctl(&mut b, Who::R, items), "Accept", None);
    let items = b.rotate_items(vec![rkc(RecoveryKeyMode::Direct, Some(new_recovery().public()))]);
    expect(&try_ctl(&mut b, Who::R, items), "Accept", None);
}
