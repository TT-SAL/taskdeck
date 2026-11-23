//! Unit tests of `classify` (seal.md §13, §14): one test per step of §14.2,
//! passing and failing; Genesis (§13.3); the halts (§14.5); the flags
//! (§14.7, §14.8); the races of V2-8; determinism. The rows of §13.6 are in
//! `items_tests.rs`.

pub(crate) mod board_tests;
mod items_tests;

use board_tests::*;

use crate::membership::DeviceStatus;
use crate::*;

fn unchanged(before: &Membership, r: &Receipt) {
    assert_eq!(r.membership.to_canonical_bytes(), before.to_canonical_bytes());
}

/// `M_{s−1}` with only the author's counter set to `ctr` (§13.5 `Refuse`).
fn only_counter(before: &Membership, r: &Receipt, dev: &DevId, ctr: u64) {
    let mut want = before.dup();
    want.set_last_ctr(dev, ctr);
    assert_eq!(r.membership.to_canonical_bytes(), want.to_canonical_bytes());
}

// ------------------------------------------------------------ step 1 to 4

// Step 1: a frame of 669 bytes, a wrong magic and a wrong `len` are `Skip`
// with `seal/frame_format`, and the state is unchanged; `C_s` is computed
// over the frame whatever its class.
// spec: SEAL-RCPT-001
// spec: SEAL-RCPT-002
#[test]
fn step1_layout() {
    let mut b = Board::new(1);
    let good = b.cmd(Who::D(0), &Spec::default());
    let mut magic = good.clone();
    magic[3] = b'2';
    let mut len = good.clone();
    len[47] ^= 1;
    for f in [good[..669].to_vec(), magic, len, vec![]] {
        let r = b.classify(&f);
        expect(&r, "Skip", Some("seal/frame_format"));
        unchanged(&b.m, &r);
        assert_eq!(r.chain, chain_step(b.chains.last().unwrap(), b.seq() + 1, &f));
        assert!(r.opened.is_none() && r.learned_root.is_none() && r.halt.is_none());
    }
    expect(&b.classify(&good), "Accept", None);
}

// Step 2: a frame of another board is `Skip` with `seal/wrong_board`.
// spec: SEAL-RCPT-002
#[test]
fn step2_board() {
    let b = Board::new(1);
    let mut other = Board::new(1);
    let f = other.cmd(Who::D(0), &Spec::default());
    let r = b.classify(&f);
    expect(&r, "Skip", Some("seal/wrong_board"));
    unchanged(&b.m, &r);
}

// Step 3: an author this board does not know is `Skip` with
// `seal/unknown_author`; a device is not an author at the seq of the entry
// that adds it, only from the next one (SEAL-MEM-007).
// spec: SEAL-RCPT-002
// spec: SEAL-MEM-007
#[test]
fn step3_author() {
    let mut b = Board::new(1);
    let k = DeviceKeys::generate(&mut OsEntropy).unwrap();
    b.devs.push(k);
    let stranger = b.cmd(Who::D(1), &Spec { ctr: Some(1), ..Spec::default() });
    let r = b.classify(&stranger);
    expect(&r, "Skip", Some("seal/unknown_author"));
    unchanged(&b.m, &r);
    // Added at seq a: its frame classified at seq a (against M_{a−1}) is
    // unknown; at a + 1 it is accepted.
    let item = b.added_item(&b.devs[1]);
    let add = b.ctl(Who::D(0), vec![item], &Spec::default());
    let early = b.cmd(Who::D(1), &Spec { ctr: Some(1), ..Spec::default() });
    expect(&b.classify(&early), "Skip", Some("seal/unknown_author"));
    b.accept(add);
    let late = b.cmd(Who::D(1), &Spec { ctr: Some(1), ..Spec::default() });
    expect(&b.classify(&late), "Accept", None);
}

// Step 4: a flipped bit anywhere under the signature is `Skip` with
// `seal/bad_signature`, and the counter is not consumed (SEAL-MEM-009).
// spec: SEAL-RCPT-002
// spec: SEAL-MEM-009
#[test]
fn step4_signature() {
    let mut b = Board::new(1);
    let f = b.cmd(Who::D(0), &Spec::default());
    for i in [20, 43, 60, 100, f.len() - 70, f.len() - 1] {
        let mut m = f.clone();
        m[i] ^= 1;
        let r = b.classify(&m);
        if (20..36).contains(&i) {
            // A changed dev names no author.
            expect(&r, "Skip", Some("seal/unknown_author"));
        } else {
            expect(&r, "Skip", Some("seal/bad_signature"));
        }
        unchanged(&b.m, &r);
    }
    b.accept(f);
}

/// `x mod L` and `(x · y) mod L` over 256-bit little-endian integers, for
/// building an Ed25519 signature with a chosen `R` (test only).
pub(crate) mod scalar {
    pub const L: [u64; 4] = [0x5812631a5cf5d3ed, 0x14def9dea2f79cd6, 0, 0x1000000000000000];

    fn ge(a: &[u64; 4], b: &[u64; 4]) -> bool {
        for i in (0..4).rev() {
            if a[i] != b[i] {
                return a[i] > b[i];
            }
        }
        true
    }
    fn sub(a: &mut [u64; 4], b: &[u64; 4]) {
        let mut borrow = 0u64;
        for i in 0..4 {
            let (d1, o1) = a[i].overflowing_sub(b[i]);
            let (d2, o2) = d1.overflowing_sub(borrow);
            a[i] = d2;
            borrow = u64::from(o1 || o2);
        }
    }
    fn add_mod(a: &mut [u64; 4], b: &[u64; 4]) {
        let mut carry = 0u64;
        for i in 0..4 {
            let (s1, o1) = a[i].overflowing_add(b[i]);
            let (s2, o2) = s1.overflowing_add(carry);
            a[i] = s2;
            carry = u64::from(o1 || o2);
        }
        if ge(a, &L) {
            sub(a, &L);
        }
    }
    /// `bytes` (little-endian, any length) mod L.
    pub fn reduce(bytes: &[u8]) -> [u64; 4] {
        let mut r = [0u64; 4];
        for byte in bytes.iter().rev() {
            for bit in (0..8).rev() {
                let r2 = r;
                add_mod(&mut r, &r2);
                if (byte >> bit) & 1 == 1 {
                    add_mod(&mut r, &[1, 0, 0, 0]);
                }
            }
        }
        r
    }
    pub fn mul(x: &[u64; 4], y: &[u64; 4]) -> [u64; 4] {
        let mut r = [0u64; 4];
        for i in (0..256).rev() {
            let r2 = r;
            add_mod(&mut r, &r2);
            if (y[i / 64] >> (i % 64)) & 1 == 1 {
                add_mod(&mut r, x);
            }
        }
        r
    }
    pub fn to_bytes(x: &[u64; 4]) -> [u8; 32] {
        let mut out = [0u8; 32];
        for i in 0..4 {
            out[8 * i..8 * i + 8].copy_from_slice(&x[i].to_le_bytes());
        }
        out
    }
    pub fn add_bytes(a: &[u8; 32], b: &[u64; 4]) -> [u8; 32] {
        let mut x = [0u64; 4];
        for i in 0..4 {
            x[i] = u64::from_le_bytes(a[8 * i..8 * i + 8].try_into().unwrap());
        }
        let mut carry = 0u64;
        for i in 0..4 {
            let (s1, o1) = x[i].overflowing_add(b[i]);
            let (s2, o2) = s1.overflowing_add(carry);
            x[i] = s2;
            carry = u64::from(o1 || o2);
        }
        to_bytes(&x)
    }
}

// Strict Ed25519 through `classify` (README §5.5): the valid `S` plus the
// group order (a non-canonical `S`), and a signature whose `R` is the
// identity (a small-order point) with `S = k·a`, which the non-strict
// equation accepts, are both `Skip` with `seal/bad_signature`.
// spec: SEAL-GEN-010
#[test]
fn strict_signatures() {
    use ed25519_dalek::Verifier as _;
    use sha2::Digest as _;
    let mut b = Board::new(1);
    let f = b.cmd(Who::D(0), &Spec::default());
    let n = f.len();
    let pk = b.dev_pub(0).sign_pk;
    let vk = ed25519_dalek::VerifyingKey::from_bytes(&pk).unwrap();
    let msg = [ctx::FRAME, &f[..n - 64]].concat();

    let mut high_s = f.clone();
    let s: [u8; 32] = f[n - 32..].try_into().unwrap();
    high_s[n - 32..].copy_from_slice(&scalar::add_bytes(&s, &scalar::L));
    expect(&b.classify(&high_s), "Skip", Some("seal/bad_signature"));

    // R = identity (order 1); S = H(R ‖ A ‖ M) · a mod L.
    let mut a = b.devs[0].signing_key().to_scalar_bytes();
    a[0] &= 248;
    a[31] &= 127;
    a[31] |= 64;
    let mut r = [0u8; 32];
    r[0] = 1;
    let k = sha2::Sha512::new().chain_update(r).chain_update(pk).chain_update(&msg).finalize();
    let s = scalar::to_bytes(&scalar::mul(&scalar::reduce(&k), &scalar::reduce(&a)));
    let sig: [u8; 64] = [r, s].concat().try_into().unwrap();
    let sig = ed25519_dalek::Signature::from_bytes(&sig);
    assert!(vk.verify(&msg, &sig).is_ok(), "the non-strict equation holds");
    assert!(vk.verify_strict(&msg, &sig).is_err());
    let mut small_r = f.clone();
    small_r[n - 64..].copy_from_slice(&sig.to_bytes());
    expect(&b.classify(&small_r), "Skip", Some("seal/bad_signature"));
    expect(&b.classify(&f), "Accept", None);
}

// ---------------------------------------------------------------- step 5

// Step 5: a replayed counter and a skipped one are `Tamper`, and leave the
// state unchanged, counter included.
// spec: SEAL-RCPT-003
// spec: SEAL-MEM-009
// spec: SEAL-MEM-008
#[test]
fn step5_counter() {
    let mut b = Board::new(1);
    let f1 = b.cmd(Who::D(0), &Spec::default());
    b.accept(f1.clone());
    let replay = b.cmd(Who::D(0), &Spec { ctr: Some(1), ..Spec::default() });
    let r = b.classify(&replay);
    expect(&r, "Tamper", Some("seal/counter_replay"));
    unchanged(&b.m, &r);
    let zero = b.cmd(Who::D(0), &Spec { ctr: Some(1), ..Spec::default() });
    expect(&b.classify(&zero), "Tamper", Some("seal/counter_replay"));
    let gap = b.cmd(Who::D(0), &Spec { ctr: Some(3), ..Spec::default() });
    let r = b.classify(&gap);
    expect(&r, "Tamper", Some("seal/counter_gap"));
    unchanged(&b.m, &r);
    let next = b.cmd(Who::D(0), &Spec { ctr: Some(2), ..Spec::default() });
    expect(&b.classify(&next), "Accept", None);
}

// ---------------------------------------------------------------- step 6

// Step 6: a frame under this device's own dev whose `(ctr, frame_hash)`
// the device has no record of sets `own_dev_foreign` and is classified by
// the remaining steps unchanged; with the record it does not.
// spec: SEAL-RCPT-010
#[test]
fn step6_own_dev() {
    let mut b = Board::new(2);
    let f = b.cmd(Who::D(0), &Spec::default());
    let mine = Opts { own: Some(0), ..Opts::default() };
    let r = b.classify_with(&f, &mine).unwrap();
    expect(&r, "Accept", None);
    assert!(!r.own_dev_foreign);
    // The same position sealed by another holder of the key.
    let ctr = b.next_ctr(Who::D(0));
    let theirs = b.cmd(Who::D(0), &Spec { ctr: Some(ctr), ..Spec::default() });
    b.sealed.insert((b.dev(Who::D(0)), ctr), frame_hash(&f));
    let r = b.classify_with(&theirs, &mine).unwrap();
    expect(&r, "Accept", None);
    assert!(r.own_dev_foreign);
    let plain = b.classify(&theirs);
    assert_eq!(r.membership.to_canonical_bytes(), plain.membership.to_canonical_bytes());
    // Another device's frame never sets it; a frame refused later keeps it.
    let other = b.cmd(Who::D(1), &Spec::default());
    assert!(!b.classify_with(&other, &mine).unwrap().own_dev_foreign);
    let late = b.cmd(Who::D(0), &Spec { ctr: Some(ctr), epoch: Some(1), root: Some([9; 32]), ..Spec::default() });
    b.sealed.insert((b.dev(Who::D(0)), ctr), frame_hash(&f));
    let r = b.classify_with(&late, &mine).unwrap();
    expect(&r, "Refuse", Some("seal/aead"));
    assert!(r.own_dev_foreign);
}

// ---------------------------------------------------------------- step 7

// Step 7: a revoked device's frame and a retired recovery key's frame are
// `Refuse` with `seal/revoked_author`, and consume the counter.
// spec: SEAL-RCPT-004
// spec: SEAL-MEM-009
#[test]
fn step7_revoked() {
    let mut b = Board::new(2);
    b.revoke(Who::D(0), 1);
    let f = b.cmd(Who::D(1), &Spec::default());
    let dev = b.dev(Who::D(1));
    let ctr = b.next_ctr(Who::D(1));
    let r = b.classify(&f);
    expect(&r, "Refuse", Some("seal/revoked_author"));
    only_counter(&b.m, &r, &dev, ctr);
    assert!(r.opened.is_none());
}

// ---------------------------------------------------------------- step 8

// Step 8: an envelope version or suite rules version 1 does not allow is
// `Refuse` with `seal/bad_version`, never a halt (V1-1).
// spec: SEAL-RCPT-004
// spec: SEAL-RCPT-005
#[test]
fn step8_version() {
    let b = Board::new(1);
    let pt = b.padded_cmd();
    let root = copy(&b.roots[&1]);
    let ctr = b.next_ctr(Who::D(0));
    for (ver, suite) in [(2, 1), (1, 2), (0, 0)] {
        let f = b.raw(Who::D(0), ctr, ver, suite, 1, &root, &pt);
        let r = b.classify(&f);
        expect(&r, "Refuse", Some("seal/bad_version"));
        assert!(r.halt.is_none());
        only_counter(&b.m, &r, &b.dev(Who::D(0)), ctr);
    }
    expect(&b.classify(&b.raw(Who::D(0), ctr, 1, 1, 1, &root, &pt)), "Accept", None);
}

// ---------------------------------------------------------------- step 9

// Step 9: an epoch above E is `seal/bad_epoch`, one below E − 1
// `seal/old_epoch`, both refused without decryption (a ciphertext no key
// opens gets the epoch's code, not `seal/aead`); E and E − 1 pass.
// spec: SEAL-ENV-005
// spec: SEAL-RCPT-004
#[test]
fn step9_epoch() {
    let mut b = Board::new(2);
    b.rotate(Who::D(0));
    b.rotate(Who::D(0));
    assert_eq!(b.m.epoch, 3);
    let junk = [0x5A; 32];
    let above = b.cmd(Who::D(0), &Spec { epoch: Some(4), root: Some(junk), ..Spec::default() });
    expect(&b.classify(&above), "Refuse", Some("seal/bad_epoch"));
    let old = b.cmd(Who::D(1), &Spec { epoch: Some(1), root: Some(junk), ..Spec::default() });
    expect(&b.classify(&old), "Refuse", Some("seal/old_epoch"));
    let zero = b.cmd(Who::D(0), &Spec { ctr: Some(b.next_ctr(Who::D(0)) + 1), epoch: Some(0), root: Some(junk), ..Spec::default() });
    // A gap is found first (step 5); with the right counter epoch 0 is old.
    expect(&b.classify(&zero), "Tamper", Some("seal/counter_gap"));
    let zero = b.cmd(Who::D(0), &Spec { epoch: Some(0), root: Some(junk), ..Spec::default() });
    expect(&b.classify(&zero), "Refuse", Some("seal/old_epoch"));
    let cur = b.cmd(Who::D(0), &Spec::default());
    expect(&b.classify(&cur), "Accept", None);
}

// No root held for an epoch step 9 accepted, or a held root that does not
// match the commitment the state records, is the local error
// `seal/missing_root`, never a class.
// spec: SEAL-KEY-013
#[test]
fn missing_root() {
    let mut b = Board::new(1);
    b.rotate(Who::D(0));
    let f = b.cmd(Who::D(0), &Spec::default());
    let hide = Opts { hide_roots: vec![2], ..Opts::default() };
    assert_eq!(b.classify_with(&f, &hide).err(), Some(SealError::MissingRoot));
    let wrong = Opts { wrong_root: Some(2), ..Opts::default() };
    assert_eq!(b.classify_with(&f, &wrong).err(), Some(SealError::MissingRoot));
    expect(&b.classify(&f), "Accept", None);
}

// --------------------------------------------------------- steps 10, 11

// Step 10: a frame sealed under another key for the right epoch is
// `Refuse` with `seal/aead`, its counter consumed, never a halt.
// spec: SEAL-RCPT-004
// spec: SEAL-RCPT-005
#[test]
fn step10_aead() {
    let mut b = Board::new(1);
    let f = b.cmd(Who::D(0), &Spec { root: Some([0x77; 32]), ..Spec::default() });
    let r = b.classify(&f);
    expect(&r, "Refuse", Some("seal/aead"));
    assert!(r.halt.is_none() && r.opened.is_none());
    only_counter(&b.m, &r, &b.dev(Who::D(0)), 1);
}

// Step 11: every malformed plaintext of seal-padding.json that fills a frame
// of at least 670 bytes, sealed into a validly signed frame, is `Refuse` with
// `seal/bad_plaintext`; the others make frames below 670 bytes (step 1).
// spec: SEAL-PAD-003
// spec: SEAL-RCPT-005
#[test]
fn step11_published_malformed_plaintexts() {
    let b = Board::new(1);
    let doc: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{}/../../docs/sync/vectors/seal-padding.json", env!("CARGO_MANIFEST_DIR"))).unwrap(),
    )
    .unwrap();
    let root = copy(&b.roots[&1]);
    let ctr = b.next_ctr(Who::D(0));
    let (mut refused, mut short) = (0, 0);
    for c in doc["cases"].as_array().unwrap() {
        let (Some(pt), Some(_)) = (c.get("plaintext"), c.get("check")) else { continue };
        let pt: Vec<u8> = (0..pt.as_str().unwrap().len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&pt.as_str().unwrap()[i..i + 2], 16).unwrap())
            .collect();
        let f = b.raw(Who::D(0), ctr, 1, 1, 1, &root, &pt);
        let r = b.classify(&f);
        if f.len() < 670 {
            expect(&r, "Skip", Some("seal/frame_format"));
            short += 1;
        } else {
            expect(&r, "Refuse", Some("seal/bad_plaintext"));
            assert_eq!(c["error"], "seal/bad_plaintext");
            only_counter(&b.m, &r, &b.dev(Who::D(0)), ctr);
            refused += 1;
        }
    }
    assert!(refused >= 8 && short >= 2, "{refused} refused, {short} short");
}

// ---------------------------------------------------------------- step 12

// Step 12: `based_on_seq ≥ s` is `Tamper` with `seal/based_on_future`; a
// chain that differs from `C_0` or from the receiver's `C_b` is `Tamper`
// with `seal/based_on_fork`; a `b` below `retained_from` sets
// `based_on_unchecked` and is accepted. Tamper leaves the state unchanged.
// spec: SEAL-RCPT-011
// spec: SEAL-RCPT-012
// spec: SEAL-RCPT-013
// spec: SEAL-RCPT-003
#[test]
fn step12_based_on() {
    let mut b = Board::new(1);
    for _ in 0..3 {
        let f = b.cmd(Who::D(0), &Spec::default());
        b.accept(f);
    }
    let s = b.seq() + 1;
    let at = |seq: u64, chain: Chain| Spec { based_on: Some(Head { seq, chain }), ..Spec::default() };
    let f = b.cmd(Who::D(0), &at(s, b.chains[3]));
    let r = b.classify(&f);
    expect(&r, "Tamper", Some("seal/based_on_future"));
    unchanged(&b.m, &r);
    assert_eq!(r.opened.as_ref().unwrap().based_on.seq, s);
    expect(&{ let f = b.cmd(Who::D(0), &at(s + 7, b.chains[3])); b.classify(&f) }, "Tamper", Some("seal/based_on_future"));
    // The sealer refuses a seq-0 head with another chain (SEAL-PLAIN-003):
    // the frame is built from its parts.
    let pt = padded(Kind::Cmd, b.now, Head { seq: 0, chain: Chain([1; 32]) }, b"x");
    let f = b.raw(Who::D(0), b.next_ctr(Who::D(0)), 1, 1, 1, &copy(&b.roots[&1]), &pt);
    expect(&b.classify(&f), "Tamper", Some("seal/based_on_fork"));
    expect(&{ let f = b.cmd(Who::D(0), &at(2, b.chains[1])); b.classify(&f) }, "Tamper", Some("seal/based_on_fork"));
    let ok = b.cmd(Who::D(0), &at(2, b.chains[2]));
    let r = b.classify(&ok);
    expect(&r, "Accept", None);
    assert!(!r.based_on_unchecked);
    // Below the retained log: unchecked, a warning, whatever the chain.
    let retained = Opts { retained_from: Some(3), ..Opts::default() };
    let r = { let f = b.cmd(Who::D(0), &at(2, Chain([1; 32]))); b.classify_with(&f, &retained) }.unwrap();
    expect(&r, "Accept", None);
    assert!(r.based_on_unchecked);
    let r = { let f = b.cmd(Who::D(0), &at(3, Chain([1; 32]))); b.classify_with(&f, &retained) }.unwrap();
    expect(&r, "Tamper", Some("seal/based_on_fork"));
    // The flag travels with a later refusal.
    let late = b.plain(Who::D(0), Kind::Beat, b"{}".to_vec(), &at(2, Chain([1; 32])));
    let r = b.classify_with(&late, &retained).unwrap();
    expect(&r, "Refuse", Some("seal/bad_payload"));
    assert!(r.based_on_unchecked);
}

// ------------------------------------------------------- steps 13 and 14

// Step 13: the recovery identity authors only control entries (`cmd`,
// `beat` and `snap` by it are `seal/not_authorized`); a `ctl` or `snap`
// under E − 1 is `seal/stale_epoch`.
// spec: SEAL-KIND-001
// spec: SEAL-MEM-026
// spec: SEAL-RCPT-004
#[test]
fn step13_kind() {
    let mut b = Board::new(2);
    let r = { let f = b.cmd(Who::R, &Spec::default()); b.classify(&f) };
    expect(&r, "Refuse", Some("seal/not_authorized"));
    only_counter(&b.m, &r, &b.dev(Who::R), b.next_ctr(Who::R));
    expect(&{ let f = b.beat(Who::R, vec![1], &Spec::default()); b.classify(&f) }, "Refuse", Some("seal/not_authorized"));
    let snap = SnapAnnounce {
        seq: 64,
        hash: [1; 32],
        hash_version: 1,
        blob: BlobId([2; 16]),
        sha256: [3; 32],
        size: 100,
        parts: 1,
        inflated: 100,
        epoch: 1,
    };
    let snap_by_r =
        b.plain(Who::R, Kind::Snap, snap_payload(&snap).unwrap(), &Spec { based_on: Some(Head { seq: 64, chain: Chain([0; 32]) }), ..Spec::default() });
    // based_on 64 ≥ s fails step 12 first; with a past head it reaches step 13.
    expect(&b.classify(&snap_by_r), "Tamper", Some("seal/based_on_future"));
    let snap0 = SnapAnnounce { seq: 64, ..snap.clone() };
    for _ in 0..64 {
        let f = b.cmd(Who::D(1), &Spec::default());
        b.accept(f);
    }
    let head64 = Some(Head { seq: 64, chain: b.chains[64] });
    let f = b.plain(Who::R, Kind::Snap, snap_payload(&snap0).unwrap(), &Spec { based_on: head64, ..Spec::default() });
    expect(&b.classify(&f), "Refuse", Some("seal/not_authorized"));
    let f = b.plain(Who::D(0), Kind::Snap, snap_payload(&snap0).unwrap(), &Spec { based_on: head64, ..Spec::default() });
    expect(&b.classify(&f), "Accept", None);
    b.rotate(Who::D(1));
    // Under E − 1: snap and ctl refused, before their payloads are read.
    let f = b.plain(Who::D(0), Kind::Snap, snap_payload(&snap0).unwrap(), &Spec { epoch: Some(1), based_on: head64, ..Spec::default() });
    expect(&b.classify(&f), "Refuse", Some("seal/stale_epoch"));
    let fence = CtlItem::Fence(Fence { host: "relay.example".into(), door: [1; 5], home_gen: 2 });
    let f = b.ctl(Who::D(0), vec![fence.clone()], &Spec { epoch: Some(1), ..Spec::default() });
    expect(&b.classify(&f), "Refuse", Some("seal/stale_epoch"));
    let f = b.ctl(Who::R, vec![fence], &Spec { epoch: Some(1), ..Spec::default() });
    expect(&b.classify(&f), "Refuse", Some("seal/stale_epoch"));
}

// Step 14 (§13.9): under E − 1 a `cmd` or `beat` by a current device is
// accepted only when it was composed before the rotation (`based_on_seq <
// epoch_seq`) and the log clock has moved at most 30 days since it; each
// failing condition is `seal/stale_epoch`. A revoked device's stale entry
// is refused at step 7.
// spec: SEAL-MEM-018
// spec: SEAL-MEM-017
#[test]
fn step14_stale_epoch() {
    let mut b = Board::new(3);
    let before = b.head();
    b.rotate(Who::D(0));
    let rot_seq = b.seq();
    assert_eq!(b.m.epoch_seq, rot_seq);
    let stale = |b: &mut Board, who, based_on: Head| b.cmd(who, &Spec { epoch: Some(1), based_on: Some(based_on), ..Spec::default() });
    let f = stale(&mut b, Who::D(1), before);
    expect(&b.classify(&f), "Accept", None);
    let f = b.beat(Who::D(1), vec![1], &Spec { epoch: Some(1), based_on: Some(before), ..Spec::default() });
    expect(&b.classify(&f), "Accept", None);
    // Composed after the rotation (b = epoch_seq).
    let h = b.head();
    let f = stale(&mut b, Who::D(1), h);
    expect(&b.classify(&f), "Refuse", Some("seal/stale_epoch"));
    // Current epoch: accepted (SEAL-MEM-017).
    let f = b.cmd(Who::D(1), &Spec::default());
    expect(&b.classify(&f), "Accept", None);
    // 30 days of log clock since the rotation, exactly: still accepted.
    let rot_clock = b.m.epoch_clock_ms;
    let window = 2_592_000_000;
    let f = b.cmd(Who::D(2), &Spec { at_ms: Some(rot_clock + window), ..Spec::default() });
    b.accept(f);
    let f = stale(&mut b, Who::D(1), before);
    expect(&b.classify(&f), "Accept", None);
    // One millisecond more: refused.
    let f = b.cmd(Who::D(2), &Spec { at_ms: Some(rot_clock + window + 1), ..Spec::default() });
    b.accept(f);
    let f = stale(&mut b, Who::D(1), before);
    let r = b.classify(&f);
    expect(&r, "Refuse", Some("seal/stale_epoch"));
    only_counter(&b.m, &r, &b.dev(Who::D(1)), b.next_ctr(Who::D(1)));
}

// Step 9 with condition 4 of §13.9 (spec change E3): a frame under E − 1
// more than 30 days of log clock after the rotation is `Refuse` with
// `seal/stale_epoch` before the root of E − 1 is asked for and before
// decryption: a receiver that has deleted that root gives the same class,
// code, flags and state as one that holds it, and a ciphertext no key opens
// gets `seal/stale_epoch`, not `seal/aead`. At exactly 30 days the root is
// still needed, and a cmd that meets conditions 1 to 3 is accepted.
// spec: SEAL-RCPT-001
// spec: SEAL-MEM-018
// spec: SEAL-ENV-005
#[test]
fn step9_stale_window() {
    let mut b = Board::new(3);
    let before = b.head();
    b.rotate(Who::D(0));
    let rot_clock = b.m.epoch_clock_ms;
    let window = 2_592_000_000;
    let stale = |b: &mut Board, root: Option<[u8; 32]>| {
        b.cmd(Who::D(1), &Spec { epoch: Some(1), based_on: Some(before), root, ..Spec::default() })
    };
    let hide = Opts { hide_roots: vec![1], ..Opts::default() };
    // Exactly 30 days: the root of E − 1 is needed, and the cmd accepted.
    let f = b.cmd(Who::D(2), &Spec { at_ms: Some(rot_clock + window), ..Spec::default() });
    b.accept(f);
    let f = stale(&mut b, None);
    assert_eq!(b.classify_with(&f, &hide).err(), Some(SealError::MissingRoot));
    expect(&b.classify(&f), "Accept", None);
    // One millisecond more: refused at step 9, with or without the root.
    let f = b.cmd(Who::D(2), &Spec { at_ms: Some(rot_clock + window + 1), ..Spec::default() });
    b.accept(f);
    let f = stale(&mut b, None);
    let held = b.classify(&f);
    let gone = b.classify_with(&f, &hide).unwrap();
    for r in [&held, &gone] {
        expect(r, "Refuse", Some("seal/stale_epoch"));
        assert!(r.opened.is_none());
        only_counter(&b.m, r, &b.dev(Who::D(1)), b.next_ctr(Who::D(1)));
    }
    assert_eq!((held.own_dev_foreign, held.based_on_unchecked), (gone.own_dev_foreign, gone.based_on_unchecked));
    assert_eq!(held.membership.to_canonical_bytes(), gone.membership.to_canonical_bytes());
    // A ciphertext no key opens: still the epoch's code.
    let junk = stale(&mut b, Some([0x5A; 32]));
    expect(&b.classify(&junk), "Refuse", Some("seal/stale_epoch"));
    expect(&b.classify_with(&junk, &hide).unwrap(), "Refuse", Some("seal/stale_epoch"));
}

// ---------------------------------------------------------------- step 15

// Step 15: a beat whose payload violates §11.4 is `seal/bad_payload`; a
// control payload whose inner signature is not the author's is
// `seal/bad_ctl_sig`; both `Refuse`, never a halt.
// spec: SEAL-RCPT-004
// spec: SEAL-RCPT-005
#[test]
fn step15_payload() {
    let mut b = Board::new(2);
    let f = b.plain(Who::D(0), Kind::Beat, b"{\"rules\":[1]}".to_vec(), &Spec::default());
    let r = b.classify(&f);
    expect(&r, "Refuse", Some("seal/bad_payload"));
    assert!(r.halt.is_none() && r.opened.is_some());
    let f = b.plain(Who::D(0), Kind::Ctl, b"not json".to_vec(), &Spec::default());
    expect(&b.classify(&f), "Refuse", Some("seal/bad_payload"));
    let fence = CtlItem::Fence(Fence { host: "relay.example".into(), door: [1; 5], home_gen: 2 });
    let f = b.ctl_raw(Who::D(0), Who::D(1), vec![fence.clone()], &Spec::default());
    expect(&b.classify(&f), "Refuse", Some("seal/bad_ctl_sig"));
    let f = b.ctl_raw(Who::D(0), Who::D(0), vec![fence], &Spec::default());
    expect(&b.classify(&f), "Accept", None);
}

// ---------------------------------------------------------- step 16, 17

// Step 16: items that form no shape are `seal/ctl_shape`; a Genesis at a
// seq other than 1 is refused (SEAL-CTL-002).
// spec: SEAL-CTL-002
// spec: SEAL-RCPT-004
#[test]
fn step16_shape() {
    let mut b = Board::new(1);
    let fence = |g| CtlItem::Fence(Fence { host: "relay.example".into(), door: [1; 5], home_gen: g });
    let f = b.ctl_raw(Who::D(0), Who::D(0), vec![fence(2), fence(3)], &Spec::default());
    let r = b.classify(&f);
    expect(&r, "Refuse", Some("seal/ctl_shape"));
    assert_eq!(r.opened.as_ref().unwrap().items.as_ref().unwrap().len(), 2);
    let g = b.genesis_item();
    let f = b.ctl(Who::R, vec![CtlItem::Genesis(g)], &Spec::default());
    expect(&b.classify(&f), "Refuse", Some("seal/ctl_shape"));
}

// Step 17: an accepted entry changes the counter, the clock (`τ' = max(τ,
// at_ms)`, never backwards) and, for `ctl`, `ctl_seqs`, as §13.5 states.
// spec: SEAL-MEM-008
#[test]
fn step17_accept_bookkeeping() {
    let mut b = Board::new(1);
    let f = b.cmd(Who::D(0), &Spec { at_ms: Some(T0 + 50_000), ..Spec::default() });
    let before = b.m.dup();
    let r = b.accept(f);
    let mut want = before.dup();
    want.set_last_ctr(&b.dev(Who::D(0)), 1);
    want.clock_ms = T0 + 50_000;
    assert_eq!(r.membership.to_canonical_bytes(), want.to_canonical_bytes());
    let o = r.opened.unwrap();
    assert_eq!((o.kind, o.at_ms, o.payload.as_slice()), (Kind::Cmd, T0 + 50_000, b"{\"op\":\"x\"}".as_slice()));
    // An earlier at_ms leaves the clock.
    let f = b.cmd(Who::D(0), &Spec { at_ms: Some(T0), ..Spec::default() });
    let r = b.accept(f);
    assert_eq!(r.membership.clock_ms, T0 + 50_000);
    // A control entry appends its seq.
    let s = b.seq() + 1;
    let fence = CtlItem::Fence(Fence { host: "relay.example".into(), door: [1; 5], home_gen: 2 });
    let f = b.ctl(Who::D(0), vec![fence], &Spec::default());
    let r = b.accept(f);
    assert_eq!(r.membership.ctl_seqs, vec![1, s]);
}

// ------------------------------------------------------------- genesis

// Seq 1 with its anchor: accepted, and the state is exactly §13.3's list.
// spec: SEAL-MEM-006
#[test]
fn genesis_state() {
    let b = Board::new(1);
    let m = &b.m;
    let g = b.genesis_item();
    let dev_r = b.dev(Who::R);
    let d = b.dev_pub(0);
    let want = Membership::genesis(b.board, &g, dev_r, T0);
    assert_eq!(m.to_canonical_bytes(), want.to_canonical_bytes());
    // The list of §13.3, field by field.
    assert_eq!((m.board(), m.salt, m.rules(), m.epoch(), m.epoch_seq), (b.board, b.salt, 1, 1, 1));
    assert_eq!((m.epoch_clock_ms, m.clock_ms, m.home_gen, m.home_seq, m.purge_seq, m.next_device_no), (T0, T0, 1, 0, 0, 2));
    assert_eq!((m.home_host.as_str(), m.home_door), ("", [0; 5]));
    assert_eq!(m.commitments, vec![(1, 1, b.roots[&1].commitment(&b.board, 1))]);
    let r = m.current_recovery();
    assert_eq!((r.dev, r.sign_pk, r.since_seq, r.until_seq, r.last_ctr), (dev_r, b.rec[0].public().sign_pk, 1, 0, 1));
    assert_eq!(m.recovery.len(), 1);
    let x = &m.devices()[0];
    assert_eq!((x.dev, x.sign_pk, x.hpke_pk, x.device_no), (d.dev, d.sign_pk, d.hpke_pk, 1));
    assert!(matches!(x.status, DeviceStatus::Current));
    assert_eq!((x.added_seq, x.added_by, x.bootstrap_seq, x.status_seq, x.status_by, x.last_ctr, x.beat_ms), (1, dev_r, 0, 1, dev_r, 0, T0));
    assert_eq!((x.rules.clone(), x.envelopes.clone(), x.hash_versions.clone(), x.name.as_str()), (vec![1], vec![(1, 1)], vec![1], "Laptop"));
    assert_eq!(m.devices().len(), 1);
    assert_eq!(m.ctl_seqs, vec![1]);
    assert!(m.pending_recovery().is_none() && m.pending_override.is_none() && m.last_reset.is_none() && m.skips.is_empty());
    assert_eq!(m.device_no(&d.dev), Some(1));
}

/// A fresh genesis frame for `b` with `g` as its item, and overrides.
fn genesis_frame(b: &mut Board, items: Vec<CtlItem>, sp: &Spec) -> Vec<u8> {
    let sp = Spec { based_on: sp.based_on.or(Some(Head { seq: 0, chain: chain_zero(&b.board) })), ctr: sp.ctr.or(Some(1)), ..sp.clone() };
    b.ctl_raw(Who::R, Who::R, items, &sp)
}

// Seq 1: every condition of SEAL-MEM-004, SEAL-MEM-005 and SEAL-KEY-017
// fails once, with the class and code of the step that checks it.
// spec: SEAL-MEM-004
// spec: SEAL-MEM-005
// spec: SEAL-KEY-017
// spec: SEAL-CTL-002
#[test]
fn genesis_checks() {
    let mut b = Board::new(1);
    let anchor = b.anchor();
    let g = b.genesis_item();
    let at = b.now;
    let ok = genesis_frame(&mut b, vec![CtlItem::Genesis(g.clone())], &Spec::default());
    let r = b.classify_genesis(&ok, &anchor);
    expect(&r, "Accept", None);
    // The membership passed in (the state after a later seq) is ignored:
    // seq 1 starts from M_0.
    let want = Membership::genesis(b.board, &g, b.dev(Who::R), at);
    assert_eq!(r.membership.to_canonical_bytes(), want.to_canonical_bytes());

    // Signed by the first device instead of dev_R: unknown author.
    let by_dev = b.ctl_raw(Who::D(0), Who::D(0), vec![CtlItem::Genesis(g.clone())], &Spec { ctr: Some(1), based_on: Some(Head { seq: 0, chain: chain_zero(&b.board) }), ..Spec::default() });
    expect(&b.classify_genesis(&by_dev, &anchor), "Skip", Some("seal/unknown_author"));
    // Another anchor key: another dev_R (unknown); another salt: another board.
    let other_rk = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
    let a2 = GenesisAnchor { rsign_pk: other_rk.public().sign_pk, salt: b.salt, root1: copy(&b.roots[&1]) };
    expect(&b.classify_genesis(&ok, &a2), "Skip", Some("seal/wrong_board"));
    let a3 = GenesisAnchor { rsign_pk: anchor.rsign_pk, salt: [0; 16], root1: copy(&b.roots[&1]) };
    expect(&b.classify_genesis(&ok, &a3), "Skip", Some("seal/wrong_board"));
    // ctr 2, epoch 2, based_on not (0, C_0), another R_1, kind cmd.
    let f = genesis_frame(&mut b, vec![CtlItem::Genesis(g.clone())], &Spec { ctr: Some(2), ..Spec::default() });
    expect(&b.classify_genesis(&f, &anchor), "Tamper", Some("seal/counter_gap"));
    let f = genesis_frame(&mut b, vec![CtlItem::Genesis(g.clone())], &Spec { epoch: Some(2), root: Some([1; 32]), ..Spec::default() });
    expect(&b.classify_genesis(&f, &anchor), "Refuse", Some("seal/bad_epoch"));
    // A seq-0 head with another chain, which the sealer refuses to make.
    let eid = EntryId(rand());
    let payload = ctl_payload(&[CtlItem::Genesis(g.clone())], &b.signer(Who::R), &b.board, 1, &eid, b.now).unwrap();
    let mut pt = padded(Kind::Ctl, b.now, Head { seq: 0, chain: Chain([0; 32]) }, &payload);
    pt[..16].copy_from_slice(&eid.0);
    let f = b.raw(Who::R, 1, 1, 1, 1, &copy(&b.roots[&1]), &pt);
    expect(&b.classify_genesis(&f, &anchor), "Tamper", Some("seal/based_on_fork"));
    let f = genesis_frame(&mut b, vec![CtlItem::Genesis(g.clone())], &Spec { based_on: Some(Head { seq: 1, chain: Chain([0; 32]) }), ..Spec::default() });
    expect(&b.classify_genesis(&f, &anchor), "Tamper", Some("seal/based_on_future"));
    let a4 = GenesisAnchor { rsign_pk: anchor.rsign_pk, salt: b.salt, root1: EpochRoot::from_bytes([3; 32]) };
    expect(&b.classify_genesis(&ok, &a4), "Refuse", Some("seal/aead"));
    let f = b.plain(Who::R, Kind::Cmd, b"x".to_vec(), &Spec { ctr: Some(1), based_on: Some(Head { seq: 0, chain: chain_zero(&b.board) }), ..Spec::default() });
    expect(&b.classify_genesis(&f, &anchor), "Refuse", Some("seal/not_authorized"));
    // Another shape at seq 1.
    let fence = CtlItem::Fence(Fence { host: "relay.example".into(), door: [1; 5], home_gen: 2 });
    let f = genesis_frame(&mut b, vec![fence], &Spec::default());
    expect(&b.classify_genesis(&f, &anchor), "Refuse", Some("seal/ctl_shape"));

    // The item: salt, recovery key, commit, wraps, device id.
    let bad = |b: &mut Board, change: &dyn Fn(&mut Genesis)| {
        let mut g2 = g.clone();
        change(&mut g2);
        let f = genesis_frame(b, vec![CtlItem::Genesis(g2)], &Spec::default());
        b.classify_genesis(&f, &anchor)
    };
    let r = bad(&mut b, &|g| g.salt = [9; 16]);
    expect(&r, "Refuse", Some("seal/ctl_genesis"));
    assert_eq!(r.membership.to_canonical_bytes(), Membership::empty().to_canonical_bytes());
    let other_pk = other_rk.public();
    expect(&bad(&mut b, &|g| g.recovery = other_pk), "Refuse", Some("seal/ctl_genesis"));
    expect(&bad(&mut b, &|g| g.commit = [4; 32]), "Refuse", Some("seal/ctl_genesis"));
    let stranger = DeviceKeys::generate(&mut OsEntropy).unwrap().public();
    let root1 = copy(&b.roots[&1]);
    let board = b.board;
    let wrap_to_stranger = wrap_root(&root1, &board, 1, &g.commit, stranger.dev, &stranger.hpke_pk, &mut OsEntropy).unwrap();
    let dev_r = b.dev(Who::R);
    expect(
        &bad(&mut b, &|g| {
            let keep = *g.wraps.iter().find(|w| w[..16] == dev_r.0).unwrap();
            let mut w = vec![keep, wrap_to_stranger];
            w.sort_by_key(|x| <[u8; 16]>::try_from(&x[..16]).unwrap());
            g.wraps = w;
        }),
        "Refuse",
        Some("seal/ctl_genesis"),
    );
    expect(&bad(&mut b, &|g| g.device.dev = dev_r), "Refuse", Some("seal/ctl_genesis"));
}

// The caller's contract broken: seq 0, and seq 1 without an anchor.
#[test]
fn caller_faults() {
    let mut b = Board::new(1);
    let f = b.cmd(Who::D(0), &Spec::default());
    let r = b.classify_with(&f, &Opts { seq: Some(0), ..Opts::default() }).unwrap();
    expect(&r, "Skip", Some("seal/unknown_author"));
    let r = b.classify_with(&f, &Opts { seq: Some(1), ..Opts::default() }).unwrap();
    expect(&r, "Skip", Some("seal/wrong_board"));
    assert_eq!(r.membership.to_canonical_bytes(), Membership::empty().to_canonical_bytes());
}

// ---------------------------------------------------------------- halts

/// A rotation by device `by` whose wrap to device `bad_for` holds another
/// root (or, with `same_root`, is made under another commitment).
fn bad_wrap_rotation(b: &mut Board, by: usize, bad_for: usize, same_root: bool) -> Vec<u8> {
    let rc = b.recipients_after(&[]);
    let mut r = b.rotation_to(b.m.epoch + 1, &rc, None);
    let target = b.dev(Who::D(bad_for));
    let pk = b.dev_pub(bad_for).hpke_pk;
    let other = EpochRoot::from_bytes([0x44; 32]);
    let (root, commit) = if same_root {
        let (e, bytes) = b.staged.unwrap();
        (EpochRoot::from_bytes(bytes), EpochRoot::from_bytes([0x45; 32]).commitment(&b.board, e))
    } else {
        (other, r.commit)
    };
    let w = wrap_root(&root, &b.board, r.epoch, &commit, target, &pk, &mut OsEntropy).unwrap();
    for x in r.wraps.iter_mut() {
        if x[..16] == target.0 {
            *x = w;
        }
    }
    b.ctl(Who::D(by), vec![CtlItem::EpochRotated(r)], &Spec::default())
}

// The commitment halt: a recipient whose own wrap holds a root that does
// not match `commit`, or does not open, gets `Accept` with the halt and no
// root; the other recipients learn the root and continue; a reader without
// an own identity accepts without a halt.
// spec: SEAL-RCPT-007
// spec: SEAL-WRAP-003
// spec: SEAL-KEY-013
// spec: SEAL-RCPT-005
#[test]
fn commitment_halt() {
    for same_root in [false, true] {
        let mut b = Board::new(3);
        let f = bad_wrap_rotation(&mut b, 0, 2, same_root);
        let (e, root) = b.staged.unwrap();
        let victim = b.classify_with(&f, &Opts { own: Some(2), ..Opts::default() }).unwrap();
        expect(&victim, "Accept", Some("seal/commitment"));
        assert!(matches!(victim.halt, Some(Halt::Commitment)));
        assert!(victim.learned_root.is_none());
        assert_eq!(victim.membership.epoch(), e);
        let fine = b.classify_with(&f, &Opts { own: Some(1), ..Opts::default() }).unwrap();
        expect(&fine, "Accept", None);
        let (le, lr) = fine.learned_root.as_ref().unwrap();
        assert_eq!((*le, *lr.to_bytes()), (e, root));
        assert_eq!(fine.membership.to_canonical_bytes(), victim.membership.to_canonical_bytes());
        let reader = b.classify(&f);
        expect(&reader, "Accept", None);
        assert!(reader.learned_root.is_none());
    }
}

// A device revoked by the rotation is not a recipient: no halt, no root.
// spec: SEAL-RCPT-007
#[test]
fn revoked_device_learns_nothing() {
    let mut b = Board::new(3);
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(b.dev(Who::D(2)))]);
    let f = b.ctl(Who::D(0), items, &Spec::default());
    let r = b.classify_with(&f, &Opts { own: Some(2), ..Opts::default() }).unwrap();
    expect(&r, "Accept", None);
    assert!(r.learned_root.is_none() && r.halt.is_none());
}

// The rules halt comes with `Accept` when the activated version is not in
// `supported_rules`, and not when it is.
// spec: SEAL-RCPT-008
// spec: SEAL-MEM-022
#[test]
fn rules_halt() {
    let mut b = Board::new(1);
    let f = b.beat(Who::D(0), vec![1, 2], &Spec::default());
    b.accept(f);
    let up = CtlItem::RulesUpgrade(RulesUpgrade { rules: 2, mode: RulesUpgradeMode::Normal });
    let f = b.ctl(Who::D(0), vec![up], &Spec::default());
    let r = b.classify(&f);
    expect(&r, "Accept", Some("seal/unknown_rules"));
    assert!(matches!(r.halt, Some(Halt::UnknownRules(2))));
    assert_eq!(r.membership.rules(), 2);
    let r = b.classify_with(&f, &Opts { supported: Some(vec![1, 2]), ..Opts::default() }).unwrap();
    expect(&r, "Accept", None);
    b.commit(f, &r);
    // Entries above the activating seq are checked under rules 2, whose
    // table this build does not have: (1, 1) is no longer allowed.
    let f = b.cmd(Who::D(0), &Spec::default());
    expect(&b.classify(&f), "Refuse", Some("seal/bad_version"));
}

// ---------------------------------------------------------------- races

// V2-8 (a): two rotations for one epoch; the first in log order wins, the
// second (sealed under the old epoch) is refused on every device, its author's
// device included, which learns no root and keeps the epoch (§13.8).
// spec: SEAL-MEM-016
// spec: SEAL-RCPT-004
#[test]
fn race_two_rotations() {
    let mut b = Board::new(3);
    let rc = b.recipients_after(&[]);
    let first = b.rotation_to(2, &rc, None);
    let staged_first = b.staged.unwrap();
    let second = b.rotation_to(2, &rc, None);
    let f1 = b.ctl(Who::D(0), vec![CtlItem::EpochRotated(first)], &Spec::default());
    let f2 = b.ctl(Who::D(1), vec![CtlItem::EpochRotated(second)], &Spec::default());
    b.staged = Some(staged_first);
    b.accept(f1);
    for own in [None, Some(0), Some(1), Some(2)] {
        let r = b.classify_with(&f2, &Opts { own, ..Opts::default() }).unwrap();
        // Sealed under the epoch the first rotation replaced: a control
        // entry under E − 1.
        expect(&r, "Refuse", Some("seal/stale_epoch"));
        assert!(r.learned_root.is_none() && r.halt.is_none());
        assert_eq!(r.membership.epoch(), 2);
        only_counter(&b.m, &r, &b.dev(Who::D(1)), b.next_ctr(Who::D(1)));
    }
}

// V2-8 (b): a DeviceAdded sealed before a rotation and sequenced after it
// is a control entry under E − 1: refused with `seal/stale_epoch`.
// spec: SEAL-MEM-018
// spec: SEAL-KIND-001
#[test]
fn race_device_added_after_rotation() {
    let mut b = Board::new(2);
    let k = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let item = b.added_item(&k);
    let add = b.ctl(Who::D(0), vec![item], &Spec::default());
    let rot = {
        let items = b.rotate_items(vec![]);
        b.ctl(Who::D(1), items, &Spec::default())
    };
    b.accept(rot);
    let r = b.classify(&add);
    expect(&r, "Refuse", Some("seal/stale_epoch"));
    assert_eq!(r.membership.devices().len(), 2);
}

// V2-8 (d): a stale-epoch `cmd` inside the 30 days is accepted; outside,
// refused (also `step14_stale_epoch`).
// spec: SEAL-MEM-018
#[test]
fn race_stale_cmd() {
    let mut b = Board::new(2);
    let before = b.head();
    let inside = b.cmd(Who::D(1), &Spec { based_on: Some(before), ..Spec::default() });
    b.rotate(Who::D(0));
    expect(&b.classify(&inside), "Accept", None);
    let rot_clock = b.m.epoch_clock_ms;
    let f = b.cmd(Who::D(0), &Spec { at_ms: Some(rot_clock + 2_592_000_001), ..Spec::default() });
    b.accept(f);
    expect(&b.classify(&inside), "Refuse", Some("seal/stale_epoch"));
}

// ---------------------------------------------------------- determinism

// Two receivers with equal inputs (two separately built input sets over
// the same bytes) return equal classes, codes, flags and states, for every
// frame of a log with rotations, refusals and tampering.
// spec: SEAL-RCPT-006
#[test]
fn determinism() {
    let mut b = Board::new(3);
    let mut frames = Vec::new();
    frames.push(b.cmd(Who::D(0), &Spec::default()));
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(b.dev(Who::D(2)))]);
    frames.push(b.ctl(Who::D(1), items, &Spec::default()));
    frames.push(b.cmd(Who::D(2), &Spec::default()));
    frames.push(b.beat(Who::D(0), vec![1], &Spec::default()));
    let mut replay = frames[0].clone();
    replay[100] ^= 1;
    frames.push(replay);
    for f in frames {
        let bytes = b.m.to_canonical_bytes();
        let m2 = Membership::from_canonical_bytes(&bytes).unwrap();
        let r1 = b.classify(&f);
        let mut b2_roots = std::collections::BTreeMap::new();
        for (e, r) in &b.roots {
            b2_roots.insert(*e, copy(r));
        }
        let roots = |e: u32| b2_roots.get(&e);
        let chains = b.chains.clone();
        let chain_at = |x: u64| chains.get(x as usize).copied();
        let frames2 = b.frames.clone();
        let skip = |t: u64| frames2.get(t as usize - 1).map(|v| v.as_slice());
        let input = ReceiptInput {
            seq: b.seq() + 1,
            frame: &f,
            prev_chain: *b.chains.last().unwrap(),
            membership: &m2,
            roots: &roots,
            chain_at: &chain_at,
            retained_from: 1,
            own: None,
            supported_rules: &[1],
            genesis: None,
            skip_targets: &skip,
        };
        let r2 = classify(&input).unwrap();
        assert_eq!((class_name(&r1), r1.code, r1.own_dev_foreign, r1.based_on_unchecked), (class_name(&r2), r2.code, r2.own_dev_foreign, r2.based_on_unchecked));
        assert_eq!(r1.membership.to_canonical_bytes(), r2.membership.to_canonical_bytes());
        assert_eq!(r1.chain, r2.chain);
        b.commit(f, &r1);
    }
}

// `classify` never changes its input state, whatever the class.
#[test]
fn input_untouched() {
    let mut b = Board::new(2);
    let before = b.m.to_canonical_bytes();
    let items = b.rotate_items(vec![CtlItem::DeviceRevoked(b.dev(Who::D(1)))]);
    let f = b.ctl(Who::D(0), items, &Spec::default());
    let r = b.classify(&f);
    expect(&r, "Accept", None);
    assert_eq!(b.m.to_canonical_bytes(), before);
    assert_ne!(r.membership.to_canonical_bytes(), before);
}

// Nothing protected is interpreted before its protection verified: with a
// broken signature, a frame under an unknown version, under an epoch above
// E, or whose Skip targets are missing is `Skip` with `seal/bad_signature`
// (not `seal/bad_version`, `seal/bad_epoch` or `NeedFrames`); a plaintext
// whose `based_on` is in the future but whose AEAD fails is `seal/aead`
// (not `seal/based_on_future`).
// spec: SEAL-GEN-009
#[test]
fn nothing_read_before_verification() {
    let mut b = Board::new(2);
    let f = b.cmd(Who::D(1), &Spec::default());
    b.accept(f);
    let broken = |mut f: Vec<u8>| {
        let n = f.len();
        f[n - 1] ^= 1;
        f
    };
    let root = copy(&b.roots[&1]);
    let ctr = b.next_ctr(Who::D(0));
    let v2 = b.raw(Who::D(0), ctr, 2, 1, 1, &root, &b.padded_cmd());
    expect(&b.classify(&v2), "Refuse", Some("seal/bad_version"));
    expect(&b.classify(&broken(v2)), "Skip", Some("seal/bad_signature"));
    let above = b.cmd(Who::D(0), &Spec { epoch: Some(2), root: Some([3; 32]), ..Spec::default() });
    expect(&b.classify(&above), "Refuse", Some("seal/bad_epoch"));
    expect(&b.classify(&broken(above)), "Skip", Some("seal/bad_signature"));
    let item = CtlItem::Skip(SkipItem { seq: 3, frame_hash: frame_hash(&b.frames[2]) });
    let skip = b.ctl(Who::D(0), vec![item], &Spec::default());
    let hidden = Opts { hide_frames: vec![3], ..Opts::default() };
    assert!(matches!(b.classify_with(&skip, &hidden).unwrap().class, Class::NeedFrames(_)));
    expect(&b.classify_with(&broken(skip), &hidden).unwrap(), "Skip", Some("seal/bad_signature"));
    let future = Head { seq: 99, chain: Chain([0; 32]) };
    let f = b.cmd(Who::D(0), &Spec { based_on: Some(future), ..Spec::default() });
    expect(&b.classify(&f), "Tamper", Some("seal/based_on_future"));
    let f = b.cmd(Who::D(0), &Spec { based_on: Some(future), root: Some([3; 32]), ..Spec::default() });
    let r = b.classify(&f);
    expect(&r, "Refuse", Some("seal/aead"));
    assert!(r.opened.is_none());
}
