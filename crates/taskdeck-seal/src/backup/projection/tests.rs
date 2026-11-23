//! Unit tests of the projection (seal.md §21.6 "Projection"): a real log's
//! history projects to its membership on what step 11 compares; each of the
//! six steps fails once with every earlier step passing; the reduced rules.

use super::*;
use crate::backup::tests::{Hist, dup};
use crate::payloads::items_bytes;
use crate::receipt::tests::board_tests::{Spec, Who, rand};
use crate::receipt::tests::scalar;
use crate::signatures::{sign_with, signer_identity};
use crate::{
    DeviceKeys, EntryId, Fence, FrameHash, OsEntropy, RecoveryKeyChange, RecoveryKeyMode, RecoverySeed, ResetTo,
    RulesUpgrade, RulesUpgradeMode, SkipItem,
};

fn project_ok(x: &Hist, recs: &[HistoryRecord]) -> Membership {
    project(recs, &x.b.board).unwrap()
}

#[track_caller]
fn rejected(x: &Hist, recs: &[HistoryRecord]) {
    assert_eq!(project(recs, &x.b.board).err(), Some(SealError::BackupHistory));
}

fn with(recs: Vec<HistoryRecord>, more: HistoryRecord) -> Vec<HistoryRecord> {
    let mut v = recs;
    v.push(more);
    v
}

fn new_keys_item(mode: RecoveryKeyMode) -> (CtlItem, crate::RecoveryKeys) {
    let nk = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
    let p = nk.public();
    (CtlItem::RecoveryKeyChange(RecoveryKeyChange { mode, sign_pk: Some(p.sign_pk), hpke_pk: Some(p.hpke_pk) }), nk)
}

/// A log with every kind of control entry, and the history it leaves.
fn busy_log() -> Hist {
    let mut x = Hist::new(3);
    x.cmd(Who::D(1));
    x.revoke(Who::D(0), 2);
    let (p, _) = new_keys_item(RecoveryKeyMode::Propose);
    x.ctl(Who::D(1), vec![p]);
    x.ctl(Who::R, vec![CtlItem::RecoveryKeyChange(RecoveryKeyChange { mode: RecoveryKeyMode::Cancel, sign_pk: None, hpke_pk: None })]);
    x.ctl(Who::D(0), vec![CtlItem::Fence(Fence { host: "relay.example.org".into(), door: [1; 5], home_gen: 2 })]);
    x.ctl(Who::D(0), vec![CtlItem::RulesUpgrade(RulesUpgrade { rules: 2, mode: RulesUpgradeMode::Override })]);
    let t = x.b.seq();
    x.ctl(Who::D(0), vec![CtlItem::ResetTo(ResetTo { seq: t, state_hash: [4; 32], hash_version: 1 })]);
    x.cmd(Who::D(1));
    let target = x.b.seq();
    let fh = crate::frame_hash(&x.b.frames[target as usize - 1]);
    x.ctl(Who::D(0), vec![CtlItem::Skip(SkipItem { seq: target, frame_hash: fh })]);
    x.change_recovery();
    x.rotate(Who::R);
    x.add_device();
    x.cmd(Who::D(3));
    x
}

// A real log's history projects to a membership with the same devices,
// keys, statuses, recovery records, epochs, commitments, pending proposal,
// purge_seq, next_device_no and ctl_seqs; each author's counter is that of
// its last control entry, the clock the largest control `at_ms`.
// spec: SEAL-BKP-008
#[test]
fn a_log_projects_to_its_membership() {
    let x = busy_log();
    let p = project_ok(&x, &x.recs());
    let m = &x.b.m;
    assert!(crate::backup::agrees(&p, m));
    assert_eq!(p.board_id, m.board_id);
    assert_eq!(p.salt, m.salt);
    assert_eq!(p.next_device_no, m.next_device_no);
    assert_eq!(p.purge_seq, m.purge_seq);
    assert_eq!(p.epoch_seq, m.epoch_seq);
    assert_eq!(p.recovery.len(), m.recovery.len());
    for (a, b) in p.recovery.iter().zip(&m.recovery) {
        assert_eq!((a.dev, a.sign_pk, a.hpke_pk, a.since_seq, a.until_seq), (b.dev, b.sign_pk, b.hpke_pk, b.since_seq, b.until_seq));
    }
    for (a, b) in p.devices.iter().zip(&m.devices) {
        assert_eq!((a.device_no, a.added_seq, a.added_by, a.status_seq, a.status_by), (b.device_no, b.added_seq, b.added_by, b.status_seq, b.status_by));
        let last = x.recs.iter().filter(|r| r.dev == a.dev).map(|r| r.ctr).max().unwrap_or(0);
        assert_eq!(a.last_ctr, last);
    }
    assert!(p.pending_recovery.is_none() && m.pending_recovery.is_none());
    assert_eq!(p.clock_ms, x.recs.iter().map(|r| r.at_ms).max().unwrap());
    // The reduced rules leave Fence, RulesUpgrade, ResetTo and Skip without
    // effect.
    assert_eq!((p.home_gen, p.rules), (1, 1));
    assert!(p.pending_override.is_none() && p.last_reset.is_none() && p.skips.is_empty());
    assert_eq!(m.home_gen, 2);
    // The same membership through the public parser: the projection is a
    // state the canonical bytes can hold.
    assert!(Membership::from_canonical_bytes(&p.to_canonical_bytes()).is_ok());
}

// Step 1: the first record is seq 1, and seqs ascend strictly.
// spec: SEAL-BKP-008
#[test]
fn step1_seqs() {
    let mut x = Hist::new(2);
    x.rotate(Who::D(1));
    let mut r = x.recs();
    r[0].seq = 2;
    rejected(&x, &r);
    let mut r = x.recs();
    r[2].seq = r[1].seq;
    rejected(&x, &r);
    let mut r = x.recs();
    r.swap(1, 2);
    rejected(&x, &r);
    rejected(&x, &[]);
    project_ok(&x, &x.recs());
}

// Step 2: the author is an authority of the projection so far: not a
// stranger, not a revoked device, not a retired recovery key; and the first
// record's dev is `dev_R` of its Genesis item.
// spec: SEAL-BKP-008
#[test]
fn step2_authority() {
    let mut x = Hist::new(3);
    let mut r = x.recs();
    r[0].dev = DevId(rand());
    rejected(&x, &r);
    // The Genesis record re-signed under another dev by the real recovery
    // key: its signature (step 4) holds, only step 2 rejects it, in the
    // projection and in `read_backup`.
    let mut r = x.recs();
    r[0].dev = DevId(rand());
    let inner = ctl_inner(&x.b.board, &r[0].dev, r[0].ctr, &r[0].entry_id, r[0].at_ms, &r[0].items);
    r[0].sig = sign_with(x.b.rec[0].signing_key(), &[&inner]);
    rejected(&x, &r);
    let file = x.seal_payload(&x.b.devs[0], &x.payload(&r));
    assert_eq!(x.read(&file).err(), Some(SealError::BackupHistory));
    assert!(x.read(&x.seal_payload(&x.b.devs[0], &x.payload(&x.recs()))).is_ok());
    // A stranger's properly signed record.
    x.b.devs.push(DeviceKeys::generate(&mut OsEntropy).unwrap());
    let s = x.b.seq() + 1;
    let item = CtlItem::Fence(Fence { host: "a.example".into(), door: [0; 5], home_gen: 2 });
    rejected(&x, &with(x.recs(), x.signed(Who::D(3), s, 1, std::slice::from_ref(&item))));
    x.b.devs.pop();
    // A revoked device's.
    x.revoke(Who::D(0), 2);
    let s = x.b.seq() + 1;
    rejected(&x, &with(x.recs(), x.signed(Who::D(2), s, 9, std::slice::from_ref(&item))));
    // A retired recovery key's.
    x.change_recovery();
    let s = x.b.seq() + 1;
    rejected(&x, &with(x.recs(), x.signed(Who::OldR(0), s, 9, std::slice::from_ref(&item))));
    // The current ones pass.
    project_ok(&x, &with(x.recs(), x.signed(Who::R, s, 9, std::slice::from_ref(&item))));
    project_ok(&x, &with(x.recs(), x.signed(Who::D(1), s, 9, &[item])));
}

// Step 3: the counter is above the author's previous record's; gaps pass.
// spec: SEAL-BKP-008
#[test]
fn step3_counter() {
    let mut x = Hist::new(2);
    x.cmd(Who::D(0));
    x.rotate(Who::D(0));
    let last = x.recs.last().unwrap().ctr;
    let s = x.b.seq() + 1;
    let item = CtlItem::Fence(Fence { host: "a.example".into(), door: [0; 5], home_gen: 2 });
    for ctr in [last, last - 1] {
        rejected(&x, &with(x.recs(), x.signed(Who::D(0), s, ctr, std::slice::from_ref(&item))));
    }
    project_ok(&x, &with(x.recs(), x.signed(Who::D(0), s, last + 5, &[item])));
}

// Step 4: the inner signature verifies strictly under the author's key over
// the inner message of §11.3: a flipped byte of any signed field, a
// signature by another member's key, a non-canonical `S` and a small-order
// `R` are rejected.
// spec: SEAL-BKP-008
// spec: SEAL-KIND-004
// spec: SEAL-GEN-010
#[test]
fn step4_inner_signature() {
    use sha2::Digest as _;
    let mut x = Hist::new(2);
    x.rotate(Who::D(1));
    let n = x.recs.len() - 1;
    let mut r = x.recs();
    r[n].sig[5] ^= 1;
    rejected(&x, &r);
    let mut r = x.recs();
    r[n].entry_id.0[0] ^= 1;
    rejected(&x, &r);
    let mut r = x.recs();
    r[n].at_ms += 1;
    rejected(&x, &r);
    let mut r = x.recs();
    r[n].ctr += 1;
    rejected(&x, &r);
    let mut r = x.recs();
    r[0].sig[0] ^= 1;
    rejected(&x, &r);
    // Signed by device 0 under device 1's dev.
    let last = x.recs.last().unwrap();
    let mut forged = dup(last);
    let (_, sk) = signer_identity(&x.b.signer(Who::D(0)), &x.b.board).unwrap();
    let inner = ctl_inner(&x.b.board, &forged.dev, forged.ctr, &forged.entry_id, forged.at_ms, &forged.items);
    forged.sig = sign_with(sk, &[&inner]);
    let mut r = x.recs();
    r[n] = forged;
    rejected(&x, &r);
    // Strict Ed25519: S + L, and R = identity with S = k·a.
    let mut r = x.recs();
    let s: [u8; 32] = r[n].sig[32..].try_into().unwrap();
    r[n].sig[32..].copy_from_slice(&scalar::add_bytes(&s, &scalar::L));
    rejected(&x, &r);
    let rec = &x.recs[n];
    let pk = x.b.dev_pub(1).sign_pk;
    let msg = ctl_inner(&x.b.board, &rec.dev, rec.ctr, &rec.entry_id, rec.at_ms, &rec.items);
    let mut a = x.b.devs[1].signing_key().to_scalar_bytes();
    a[0] &= 248;
    a[31] &= 127;
    a[31] |= 64;
    let mut big_r = [0u8; 32];
    big_r[0] = 1;
    let k = sha2::Sha512::new().chain_update(big_r).chain_update(pk).chain_update(&msg).finalize();
    let s = scalar::to_bytes(&scalar::mul(&scalar::reduce(&k), &scalar::reduce(&a)));
    let mut r = x.recs();
    r[n].sig[..32].copy_from_slice(&big_r);
    r[n].sig[32..].copy_from_slice(&s);
    rejected(&x, &r);
    project_ok(&x, &x.recs());
}

// Step 5: the items parse in their canonical encoding and form a shape;
// Genesis stands only in the first record.
// spec: SEAL-BKP-008
#[test]
fn step5_items() {
    let x = Hist::new(2);
    let s = x.b.seq() + 1;
    let ctr = x.b.next_ctr(Who::D(0));
    let item = CtlItem::Fence(Fence { host: "a.example".into(), door: [0; 5], home_gen: 2 });
    let (_, good) = items_bytes(std::slice::from_ref(&item)).unwrap();
    project_ok(&x, &with(x.recs(), x.signed_bytes(Who::D(0), s, ctr, good.clone())));
    // Not canonical: a space; not JSON; an empty array; an unknown item.
    let spaced = [&good[..1], b" ", &good[1..]].concat();
    for bytes in [spaced, b"{".to_vec(), b"[]".to_vec(), b"[{\"type\":\"Nope\"}]".to_vec()] {
        rejected(&x, &with(x.recs(), x.signed_bytes(Who::D(0), s, ctr, bytes)));
    }
    // No shape.
    rejected(&x, &with(x.recs(), x.signed(Who::D(0), s, ctr, &[item.clone(), item])));
    // Genesis again.
    let g = CtlItem::Genesis(x.b.genesis_item());
    rejected(&x, &with(x.recs(), x.signed(Who::R, s, 5, &[g])));
}

// Step 6, the reduced rules: a DeviceAdded with a recorded key, a rotation
// to an epoch other than the next or with wraps to the wrong recipients, a
// revocation of the author itself, a reinstatement by a device, ResetTo and
// RulesUpgrade by the recovery identity, a confirm by its proposer are
// rejected; Fence and Skip by the recovery identity pass, and a confirm
// before 72 hours passes (the time check is not repeated).
// spec: SEAL-BKP-008
#[test]
fn step6_reduced_rules() {
    let mut x = Hist::new(2);
    let s = x.b.seq() + 1;
    let c0 = x.b.next_ctr(Who::D(0));
    let cr = x.b.next_ctr(Who::R);
    let p1 = x.b.dev_pub(1);
    let dup_add = CtlItem::DeviceAdded(crate::DeviceAdded {
        dev: DevId(rand()),
        sign_pk: p1.sign_pk,
        hpke_pk: rand(),
        name: "X".into(),
        transcript: [0; 32],
        bootstrap_seq: 1,
    });
    rejected(&x, &with(x.recs(), x.signed(Who::D(0), s, c0, &[dup_add])));
    let mut rot = x.b.rotate_items(vec![]);
    if let Some(CtlItem::EpochRotated(r)) = rot.last_mut() {
        r.epoch += 1;
    }
    rejected(&x, &with(x.recs(), x.signed(Who::D(0), s, c0, &rot)));
    let mut rot = x.b.rotate_items(vec![]);
    if let Some(CtlItem::EpochRotated(r)) = rot.last_mut() {
        r.wraps.remove(0);
    }
    rejected(&x, &with(x.recs(), x.signed(Who::D(0), s, c0, &rot)));
    let me = x.b.dev(Who::D(0));
    let self_revoke = x.b.rotate_items(vec![CtlItem::DeviceRevoked(me)]);
    rejected(&x, &with(x.recs(), x.signed(Who::D(0), s, c0, &self_revoke)));
    let reset = CtlItem::ResetTo(ResetTo { seq: 1, state_hash: [0; 32], hash_version: 1 });
    rejected(&x, &with(x.recs(), x.signed(Who::R, s, cr, std::slice::from_ref(&reset))));
    project_ok(&x, &with(x.recs(), x.signed(Who::D(0), s, c0, &[reset])));
    let up = CtlItem::RulesUpgrade(RulesUpgrade { rules: 2, mode: RulesUpgradeMode::Normal });
    rejected(&x, &with(x.recs(), x.signed(Who::R, s, cr, std::slice::from_ref(&up))));
    project_ok(&x, &with(x.recs(), x.signed(Who::D(0), s, c0, &[up])));
    let fence = CtlItem::Fence(Fence { host: "a.example".into(), door: [0; 5], home_gen: 7 });
    project_ok(&x, &with(x.recs(), x.signed(Who::R, s, cr, &[fence])));
    let skip = CtlItem::Skip(SkipItem { seq: 1, frame_hash: FrameHash([0; 32]) });
    project_ok(&x, &with(x.recs(), x.signed(Who::R, s, cr, &[skip])));

    // A reinstatement by a device; by the recovery identity it passes.
    x.revoke(Who::D(0), 1);
    let s = x.b.seq() + 1;
    let back = x.b.rotate_items(vec![CtlItem::DeviceReinstated(x.b.dev(Who::D(1)))]);
    rejected(&x, &with(x.recs(), x.signed(Who::D(0), s, x.b.next_ctr(Who::D(0)), &back)));
    let p = project_ok(&x, &with(x.recs(), x.signed(Who::R, s, x.b.next_ctr(Who::R), &back)));
    assert!(p.device(&x.b.dev(Who::D(1))).unwrap().is_current());

    // Propose, then confirm at once: refused by `classify` (72 hours),
    // accepted by the projection; a confirm by the proposer is rejected.
    let mut y = Hist::new(2);
    let (prop, nk) = new_keys_item(RecoveryKeyMode::Propose);
    y.ctl(Who::D(0), vec![prop]);
    let p = nk.public();
    let confirm = CtlItem::RecoveryKeyChange(RecoveryKeyChange {
        mode: RecoveryKeyMode::Confirm,
        sign_pk: Some(p.sign_pk),
        hpke_pk: Some(p.hpke_pk),
    });
    let items = y.b.rotate_items(vec![confirm]);
    let f = y.b.ctl(Who::D(1), items.clone(), &Spec::default());
    assert_eq!(y.b.classify(&f).code, Some("seal/ctl_window"));
    let s = y.b.seq() + 1;
    let pm = project_ok(&y, &with(y.recs(), y.signed(Who::D(1), s, y.b.next_ctr(Who::D(1)), &items)));
    assert_eq!(pm.current_recovery().sign_pk, p.sign_pk);
    rejected(&y, &with(y.recs(), y.signed(Who::D(0), s, y.b.next_ctr(Who::D(0)), &items)));
}

/// A Genesis record for `board`, `dev_R` of `board`, signed by the board's
/// recovery key over `board`, with `g` as its item.
fn genesis_record(x: &Hist, board: &BoardId, g: crate::Genesis) -> HistoryRecord {
    let rk = &x.b.rec[0];
    let dev = rk.public().dev(board);
    let (_, items) = items_bytes(&[CtlItem::Genesis(g)]).unwrap();
    let entry_id = EntryId(rand());
    let inner = ctl_inner(board, &dev, 1, &entry_id, 7, &items);
    HistoryRecord { seq: 1, dev, ctr: 1, entry_id, at_ms: 7, frame_hash: FrameHash([0; 32]), items, sig: sign_with(rk.signing_key(), &[&inner]) }
}

// Step 6 for Genesis: the board id its recovery key and salt give is the
// trusted one (§6.2), its device is not `dev_R`, and its wraps name exactly
// the device and `dev_R`.
// spec: SEAL-BKP-008
// spec: SEAL-BKP-004
// spec: SEAL-KEY-017
#[test]
fn step6_genesis() {
    let x = Hist::new(1);
    let g = x.b.genesis_item();
    assert!(project(&[genesis_record(&x, &x.b.board, g.clone())], &x.b.board).is_ok());
    // Another board: the item is the board's own except that its wraps name
    // `dev_R` of `other`, so only the `board_id` check rejects it.
    let other = BoardId([0x5A; 16]);
    let mut for_other = g.clone();
    let root = &x.b.roots[&1];
    let rp = x.b.rec[0].public();
    let mut rc = [(g.device.dev, g.device.hpke_pk), (rp.dev(&other), rp.hpke_pk)];
    rc.sort();
    for_other.commit = root.commitment(&other, 1);
    for_other.wraps = rc
        .iter()
        .map(|(id, pk)| crate::wrap_root(root, &other, 1, &for_other.commit, *id, pk, &mut OsEntropy).unwrap())
        .collect();
    assert_eq!(project(&[genesis_record(&x, &other, for_other)], &other).err(), Some(SealError::BackupHistory));
    let mut bad_wraps = g.clone();
    bad_wraps.wraps[0][..16].copy_from_slice(&[0; 16]);
    bad_wraps.wraps.sort_by_key(|w| w[..16].to_vec());
    rejected(&x, &[genesis_record(&x, &x.b.board, bad_wraps)]);
    let mut same = g.clone();
    same.device.dev = x.b.dev(Who::R);
    rejected(&x, &[genesis_record(&x, &x.b.board, same)]);
}

// The projection copies nothing per record (task S4's security pass: a copy
// per record made its time grow with the square of the history): over a
// history of 300 records, `Membership::dup` runs not once.
// spec: SEAL-BKP-008
#[test]
fn the_projection_copies_nothing_per_record() {
    use crate::membership::DUPS;
    let x = Hist::new(2);
    let mut recs = x.recs();
    let item = CtlItem::Fence(Fence { host: "a.example".into(), door: [0; 5], home_gen: 2 });
    let (ctr0, seq0) = (x.b.next_ctr(Who::D(0)), x.b.seq());
    for i in 0..300u64 {
        recs.push(x.signed(Who::D(0), seq0 + 2 * (i + 1), ctr0 + i, std::slice::from_ref(&item)));
    }
    DUPS.with(|c| c.set(0));
    let p = project_ok(&x, &recs);
    assert_eq!(DUPS.with(|c| c.get()), 0);
    assert_eq!(p.ctl_seqs.len(), 302);
    // A failing record ends it, with nothing copied either.
    let mut bad = x.recs();
    bad.push(x.signed(Who::D(0), x.b.seq() + 1, 1, &[item]));
    DUPS.with(|c| c.set(0));
    rejected(&x, &bad);
    assert_eq!(DUPS.with(|c| c.get()), 0);
}

// Step 3 for the first record: its counter is 1 (SEAL-MEM-004, ruling Q46);
// the Genesis record re-signed by the real recovery key with `ctr` 7 fails.
// spec: SEAL-BKP-008
// spec: SEAL-MEM-004
#[test]
fn step3_the_first_counter_is_1() {
    let x = Hist::new(1);
    for ctr in [0u64, 2, 7] {
        let mut r = x.recs();
        r[0].ctr = ctr;
        let inner = ctl_inner(&x.b.board, &r[0].dev, ctr, &r[0].entry_id, r[0].at_ms, &r[0].items);
        r[0].sig = sign_with(x.b.rec[0].signing_key(), &[&inner]);
        rejected(&x, &r);
    }
    project_ok(&x, &x.recs());
}
