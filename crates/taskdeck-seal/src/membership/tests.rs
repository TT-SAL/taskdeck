//! Unit tests of the membership state: the canonical bytes of §13.2 against
//! an encoder written here from the table, the strict parser's rejections,
//! the empty state, and that no secret enters the bytes.

use super::*;
use crate::receipt::tests::board::{Board, Spec, Who};
use crate::{CtlItem, Fence, OsEntropy, RecoveryKeyChange, RecoveryKeyMode, RecoverySeed, ResetTo, RulesUpgrade, RulesUpgradeMode, SkipItem, frame_hash};

/// The bytes of §13.2, written field by field from the table, independent
/// of `to_canonical_bytes`.
fn table_bytes(m: &Membership) -> Vec<u8> {
    let mut o = b"TDM1".to_vec();
    o.extend(1u32.to_be_bytes());
    o.extend(m.board_id.0);
    o.extend(m.salt);
    o.extend(m.rules.to_be_bytes());
    o.extend(m.epoch.to_be_bytes());
    o.extend(m.epoch_seq.to_be_bytes());
    o.extend(m.epoch_clock_ms.to_be_bytes());
    o.extend(m.clock_ms.to_be_bytes());
    o.extend(m.home_gen.to_be_bytes());
    o.extend(m.home_seq.to_be_bytes());
    assert_eq!(o.len(), 84, "the fixed part before the lists");
    o.push(m.home_host.len() as u8);
    o.extend(m.home_host.as_bytes());
    o.extend(m.home_door);
    o.extend(m.purge_seq.to_be_bytes());
    o.extend(m.next_device_no.to_be_bytes());
    o.extend((m.commitments.len() as u32).to_be_bytes());
    for (e, s, c) in &m.commitments {
        o.extend(e.to_be_bytes());
        o.extend(s.to_be_bytes());
        o.extend(c);
    }
    o.extend((m.recovery.len() as u16).to_be_bytes());
    for r in &m.recovery {
        o.extend(r.dev.0);
        o.extend(r.sign_pk);
        o.extend(r.hpke_pk);
        o.extend(r.since_seq.to_be_bytes());
        o.extend(r.until_seq.to_be_bytes());
        o.extend(r.last_ctr.to_be_bytes());
    }
    o.extend((m.devices.len() as u16).to_be_bytes());
    for d in &m.devices {
        let start = o.len();
        o.extend(d.dev.0);
        o.extend(d.sign_pk);
        o.extend(d.hpke_pk);
        o.extend(d.device_no.to_be_bytes());
        o.push(if d.is_current() { 1 } else { 2 });
        o.extend(d.added_seq.to_be_bytes());
        o.extend(d.added_by.0);
        o.extend(d.bootstrap_seq.to_be_bytes());
        o.extend(d.status_seq.to_be_bytes());
        o.extend(d.status_by.0);
        o.extend(d.last_ctr.to_be_bytes());
        o.extend(d.beat_ms.to_be_bytes());
        assert_eq!(o.len() - start, 155, "the fixed part of a device record");
        o.push(d.rules.len() as u8);
        for v in &d.rules {
            o.extend(v.to_be_bytes());
        }
        o.push(d.envelopes.len() as u8);
        for (a, b) in &d.envelopes {
            o.extend([*a, *b]);
        }
        o.push(d.hash_versions.len() as u8);
        for v in &d.hash_versions {
            o.extend(v.to_be_bytes());
        }
        o.push(d.name.len() as u8);
        o.extend(d.name.as_bytes());
    }
    match &m.pending_recovery {
        None => o.push(0),
        Some(p) => {
            o.push(1);
            o.extend(p.proposer.0);
            o.extend(p.sign_pk);
            o.extend(p.hpke_pk);
            o.extend(p.seq.to_be_bytes());
            o.extend(p.start_ms.to_be_bytes());
        }
    }
    match &m.pending_override {
        None => o.push(0),
        Some((r, v, s)) => {
            o.push(1);
            o.extend(r.to_be_bytes());
            o.extend(v.0);
            o.extend(s.to_be_bytes());
        }
    }
    match &m.last_reset {
        None => o.push(0),
        Some((a, t, h, sh)) => {
            o.push(1);
            o.extend(a.to_be_bytes());
            o.extend(t.to_be_bytes());
            o.extend(h.to_be_bytes());
            o.extend(sh);
        }
    }
    o.extend((m.skips.len() as u32).to_be_bytes());
    for (t, h) in &m.skips {
        o.extend(t.to_be_bytes());
        o.extend(h.0);
    }
    o.extend((m.ctl_seqs.len() as u32).to_be_bytes());
    for s in &m.ctl_seqs {
        o.extend(s.to_be_bytes());
    }
    o
}

/// A board whose state has every optional part: a Fence, two rotations, a
/// revoked device, a proposal, an override vote, a reset and a skip.
fn rich() -> Board {
    let mut b = Board::new(4);
    let f = b.cmd(Who::D(1), &Spec::default());
    b.accept(f);
    let f = b.ctl(Who::D(0), vec![CtlItem::Fence(Fence { host: "relay.example:8443".into(), door: [1, 2, 3, 4, 5], home_gen: 2 })], &Spec::default());
    b.accept(f);
    b.revoke(Who::D(0), 3);
    b.rotate(Who::R);
    let k = RecoverySeed::generate(&mut OsEntropy).unwrap().keys().public();
    let f = b.ctl(
        Who::D(1),
        vec![CtlItem::RecoveryKeyChange(RecoveryKeyChange { mode: RecoveryKeyMode::Propose, sign_pk: Some(k.sign_pk), hpke_pk: Some(k.hpke_pk) })],
        &Spec::default(),
    );
    b.accept(f);
    let f = b.ctl(Who::D(2), vec![CtlItem::RulesUpgrade(RulesUpgrade { rules: 5, mode: RulesUpgradeMode::Override })], &Spec::default());
    b.accept(f);
    let f = b.ctl(Who::D(0), vec![CtlItem::ResetTo(ResetTo { seq: 6, state_hash: [7; 32], hash_version: 1 })], &Spec::default());
    b.accept(f);
    let t = 5;
    let f = b.ctl(Who::D(0), vec![CtlItem::Skip(SkipItem { seq: t, frame_hash: frame_hash(&b.frames[t as usize - 1]) })], &Spec::default());
    b.accept(f);
    let f = b.beat(Who::D(2), vec![1, 5], &Spec::default());
    b.accept(f);
    b
}

// The bytes are exactly §13.2's table, every list in its order; parsing
// them gives the same state; equal states give equal bytes.
// spec: SEAL-MEM-001
#[test]
fn canonical_layout() {
    let b = rich();
    let m = &b.m;
    assert!(m.pending_recovery.is_some() && m.pending_override.is_some() && m.last_reset.is_some());
    assert!(!m.skips.is_empty() && !m.home_host.is_empty() && m.recovery.len() == 1 && m.epoch == 3);
    let bytes = m.to_canonical_bytes();
    assert_eq!(bytes, table_bytes(m));
    let again = Membership::from_canonical_bytes(&bytes).unwrap();
    assert_eq!(again.to_canonical_bytes(), bytes);
    assert_eq!(m.dup().to_canonical_bytes(), bytes);
    // Lists in their orders.
    assert!(m.devices.windows(2).all(|w| w[0].dev < w[1].dev));
    // The genesis state too, and the empty state.
    let g = Board::new(1);
    assert_eq!(g.m.to_canonical_bytes(), table_bytes(&g.m));
    let e = Membership::empty().to_canonical_bytes();
    assert_eq!(e, table_bytes(&Membership::empty()));
    assert!(Membership::from_canonical_bytes(&e).is_ok());
}

// The accessors of §26.
#[test]
fn accessors() {
    let b = rich();
    let m = Membership::from_canonical_bytes(&b.m.to_canonical_bytes()).unwrap();
    assert_eq!((m.board(), m.epoch(), m.rules()), (b.board, 3, 1));
    assert_eq!(m.devices().len(), 4);
    assert_eq!(m.current_recovery().dev, b.dev(Who::R));
    assert_eq!(m.pending_recovery().unwrap().proposer, b.dev(Who::D(1)));
    for i in 0..4 {
        assert_eq!(m.device_no(&b.dev(Who::D(i))), Some(i as u16 + 1));
    }
    assert_eq!(m.device_no(&b.dev(Who::R)), None);
    assert_eq!(Membership::empty().current_recovery().until_seq, 0);
}

fn rejects(bytes: &[u8]) {
    assert_eq!(Membership::from_canonical_bytes(bytes).err(), Some(SealError::MembershipFormat));
}

fn reencoded(m: &Membership, change: impl FnOnce(&mut Membership)) -> Vec<u8> {
    let mut x = m.dup();
    change(&mut x);
    x.to_canonical_bytes()
}

// Wrong magic or version, trailing bytes, any truncation, a flag other than
// 0 or 1: `seal/membership_format`.
// spec: SEAL-MEM-002
// spec: SEAL-GEN-008
#[test]
fn rejects_layout() {
    let b = rich();
    let bytes = b.m.to_canonical_bytes();
    let mut m = bytes.clone();
    m[3] = b'2';
    rejects(&m);
    let mut m = bytes.clone();
    m[7] = 2;
    rejects(&m);
    rejects(&[bytes.as_slice(), &[0]].concat());
    for n in 0..bytes.len() {
        rejects(&bytes[..n]);
    }
    // The three flags sit before `n_skip` (one skip) and `n_ctl`.
    let tail = 4 + 40 * b.m.skips.len() + 4 + 8 * b.m.ctl_seqs.len();
    let reset_flag = bytes.len() - tail - 53;
    assert_eq!(bytes[reset_flag], 1);
    let mut m = bytes.clone();
    m[reset_flag] = 2;
    rejects(&m);
    let g = Board::new(1).m.to_canonical_bytes();
    let gtail = 4 + 4 + 8;
    for i in 1..=3 {
        let mut m = g.clone();
        m[g.len() - gtail - i] = 2;
        rejects(&m);
        let mut m = g.clone();
        m[g.len() - gtail - i] = 0;
        assert!(Membership::from_canonical_bytes(&m).is_ok());
    }
}

// Lists out of order or with duplicates, fields out of range, counts over
// their limits of §23 or beyond the bytes remaining: `seal/membership_format`.
// spec: SEAL-MEM-002
// spec: SEAL-GEN-008
#[test]
fn rejects_content() {
    let b = rich();
    let m = &b.m;
    rejects(&reencoded(m, |x| x.devices.swap(0, 1)));
    rejects(&reencoded(m, |x| {
        let d = x.devices[0].dup();
        x.devices.insert(1, d);
    }));
    rejects(&reencoded(m, |x| x.commitments.swap(0, 1)));
    rejects(&reencoded(m, |x| x.commitments.pop().map(|_| ()).unwrap_or(())));
    rejects(&reencoded(m, |x| x.ctl_seqs.swap(0, 1)));
    rejects(&reencoded(m, |x| {
        let c = x.ctl_seqs[0];
        x.ctl_seqs.insert(0, c);
    }));
    rejects(&reencoded(m, |x| {
        let s = x.skips[0];
        x.skips.push(s);
    }));
    rejects(&reencoded(m, |x| x.devices[0].device_no = 0));
    rejects(&reencoded(m, |x| x.devices[0].device_no = 8192));
    rejects(&reencoded(m, |x| x.devices[1].device_no = x.devices[0].device_no));
    rejects(&reencoded(m, |x| x.devices[0].device_no = x.next_device_no));
    rejects(&reencoded(m, |x| x.next_device_no = 8193));
    rejects(&reencoded(m, |x| x.devices[0].rules = vec![2, 1]));
    rejects(&reencoded(m, |x| x.devices[0].envelopes = vec![(1, 1), (1, 1)]));
    rejects(&reencoded(m, |x| x.devices[0].hash_versions = (1..=17).collect()));
    rejects(&reencoded(m, |x| x.devices[0].name = String::new()));
    rejects(&reencoded(m, |x| x.devices[0].name = "a\u{7}b".into()));
    rejects(&reencoded(m, |x| x.home_host = "Not A Host".into()));
    // The recovery list: two current keys, none, out of order, a key that
    // does not give the board id.
    rejects(&reencoded(m, |x| {
        let mut r = x.recovery[0].dup();
        r.since_seq = 99;
        r.dev = DevId([0x66; 16]);
        x.recovery.push(r);
    }));
    rejects(&reencoded(m, |x| x.recovery[0].until_seq = 5));
    rejects(&reencoded(m, |x| x.recovery[0].sign_pk = [1; 32]));
    rejects(&reencoded(m, |x| x.salt = [1; 16]));
    // A device id equal to a dev_R.
    rejects(&reencoded(m, |x| {
        x.devices[0].dev = x.recovery[0].dev;
        x.devices.sort_by_key(|d| d.dev);
    }));
    // A status byte of 3: patched into the first device record.
    let bytes = m.to_canonical_bytes();
    let host = m.home_host.len();
    let first_dev = 84 + 1 + host + 15 + 4 + 44 * m.commitments.len() + 2 + 104 * m.recovery.len() + 2;
    assert_eq!(bytes[first_dev..first_dev + 16], m.devices[0].dev.0);
    let mut x = bytes.clone();
    x[first_dev + 82] = 3;
    rejects(&x);
    // Counts: over the limit, or beyond the bytes remaining.
    let n_dev = first_dev - 2;
    let mut x = bytes.clone();
    x[n_dev..n_dev + 2].copy_from_slice(&257u16.to_be_bytes());
    rejects(&x);
    let mut x = bytes.clone();
    x[n_dev..n_dev + 2].copy_from_slice(&200u16.to_be_bytes());
    rejects(&x);
    let n_commit = 84 + 1 + host + 15;
    for n in [u32::MAX, 65_536, 65_535] {
        let mut x = bytes.clone();
        x[n_commit..n_commit + 4].copy_from_slice(&n.to_be_bytes());
        rejects(&x);
    }
    let n_ctl = bytes.len() - 4 - 8 * m.ctl_seqs.len();
    for n in [u32::MAX, 1_048_577, 1_048_576] {
        let mut x = bytes.clone();
        x[n_ctl..n_ctl + 4].copy_from_slice(&n.to_be_bytes());
        rejects(&x);
    }
}

// The empty state: only its exact bytes are accepted with epoch 0.
// spec: SEAL-MEM-002
#[test]
fn empty_state() {
    let e = Membership::empty().to_canonical_bytes();
    let m = Membership::from_canonical_bytes(&e).unwrap();
    assert_eq!((m.epoch(), m.devices().len()), (0, 0));
    rejects(&reencoded(&Membership::empty(), |x| x.rules = 1));
    rejects(&reencoded(&Membership::empty(), |x| x.ctl_seqs = vec![1]));
    rejects(&reencoded(&Board::new(1).m, |x| x.epoch = 0));
}

// No epoch root, private key or other secret of the board's devices and
// recovery identities appears in the canonical bytes.
// spec: SEAL-MEM-003
#[test]
fn no_secrets() {
    let b = rich();
    let bytes = b.m.to_canonical_bytes();
    let contains = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    for r in b.roots.values() {
        assert!(!contains(&r.to_bytes()[..]));
    }
    for d in &b.devs {
        let id = d.to_identity_bytes();
        assert!(!contains(&id[16..48]) && !contains(&id[48..80]));
        assert!(contains(&id[..16]));
    }
    let sk = b.rec[0].signing_key().to_bytes();
    assert!(!contains(&sk));
}

// A recovery record whose `dev` is not the `dev_R` of its `sign_pk` (§6.3),
// or whose `until_seq` is not the `since_seq` of the record after it (a gap
// or an overlap), or a last record that is retired: no log produces it, and
// the parser rejects it. Records that chain, from real recovery-key changes,
// pass.
// spec: SEAL-MEM-002
// spec: SEAL-KEY-006
#[test]
fn rejects_recovery_records() {
    let mut b = Board::new(2);
    for _ in 0..2 {
        let k = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
        let p = k.public();
        let items = b.rotate_items(vec![CtlItem::RecoveryKeyChange(RecoveryKeyChange { mode: RecoveryKeyMode::Direct, sign_pk: Some(p.sign_pk), hpke_pk: Some(p.hpke_pk) })]);
        let f = b.ctl(Who::R, items, &Spec::default());
        b.accept(f);
        b.rec.push(k);
    }
    let m = &b.m;
    assert_eq!(m.recovery.len(), 3);
    assert!(Membership::from_canonical_bytes(&m.to_canonical_bytes()).is_ok());
    // A foreign dev: on a retired record, on the current one.
    rejects(&reencoded(m, |x| x.recovery[1].dev = DevId([0x66; 16])));
    rejects(&reencoded(m, |x| x.recovery[2].dev = DevId([0x66; 16])));
    // Another board's dev_R for the same key.
    rejects(&reencoded(m, |x| x.recovery[1].dev = crate::RecoveryPublic { sign_pk: x.recovery[1].sign_pk, hpke_pk: [0; 32] }.dev(&crate::BoardId([1; 16]))));
    // A gap and an overlap between records 0 and 1.
    let since1 = m.recovery[1].since_seq;
    rejects(&reencoded(m, |x| x.recovery[0].until_seq = since1 - 1));
    rejects(&reencoded(m, |x| x.recovery[0].until_seq = since1 + 1));
    rejects(&reencoded(m, |x| x.recovery[1].since_seq += 1));
    // The last record retired and an earlier one current.
    rejects(&reencoded(m, |x| {
        x.recovery[1].until_seq = 0;
        x.recovery[2].until_seq = 99;
    }));
}
