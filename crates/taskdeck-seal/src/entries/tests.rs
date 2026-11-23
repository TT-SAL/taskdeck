use super::*;
use crate::{DeviceKeys, EpochRoot, KeyKind, OsEntropy, RecoverySeed, derive_epoch_key};
use proptest::prelude::*;

const BOARD: BoardId = BoardId([0xB0; 16]);

fn root() -> EpochRoot {
    EpochRoot::from_bytes([0x11; 32])
}

fn key(e: u32) -> SymKey {
    derive_epoch_key(&root(), &BOARD, e, KeyKind::Entry)
}

fn plain(kind: Kind, payload: Vec<u8>) -> Plain {
    Plain {
        entry_id: EntryId([0xE0; 16]),
        kind,
        at_ms: 1_700_000_000_000,
        based_on: Head { seq: 0, chain: chain_zero(&BOARD) },
        payload,
    }
}

fn sha(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    for p in parts {
        sha2::Digest::update(&mut hasher, p);
    }
    sha2::Digest::finalize(hasher).into()
}

// ---------------------------------------------------------------- padding

// The 512-byte buckets up to 8192 and Padmé above, at the boundaries §10.2
// lists, and P against the formula written out independently.
// spec: SEAL-PAD-001
#[test]
fn padding_at_the_boundaries() {
    for (u, p) in [
        (0, 0),
        (1, 512),
        (69, 512),
        (512, 512),
        (513, 1024),
        (600, 1024),
        (8191, 8192),
        (8192, 8192),
        (8193, 8704),
        (8704, 8704),
        (8705, 9216),
        (16383, 16384),
        (16384, 16384),
        (16385, 17408),
        (20000, 20480),
        (258_048, 258_048),
        (258_049, 262_144),
    ] {
        assert_eq!(padded_len(u), p, "P({u})");
    }
    assert_eq!(padded_len(u64::MAX), u64::MAX);
    assert_eq!(padded_len(1 << 63), 1 << 63);
    // Padmé written out: the low z bits cleared upwards, z = E − S.
    for u in (8193u64..300_000).step_by(37).chain([1 << 20, (1 << 32) + 1, 1 << 62]) {
        let e = 63 - u.leading_zeros() as u64;
        let s = (63 - e.leading_zeros() as u64) + 1;
        let step = 1u64 << (e - s);
        let want = u.div_ceil(step) * step;
        assert_eq!(padded_len(u), want, "P({u})");
    }
    // For U from 8193 to 16383 Padmé also rounds to multiples of 512.
    for u in 8193..16384u64 {
        assert_eq!(padded_len(u), u.div_ceil(512) * 512, "P({u})");
    }
}

// ------------------------------------------------------------------ frame

// The frame, body and plaintext are laid out as §8.1, §9.1 and §10.1, the
// AEAD under K_entry,epoch with the AD of §9.2 opens it, the padding is
// zeros to P(U), and the signature covers ctx("frame") ‖ frame[0..48+len];
// every byte here is checked against a layout written out by hand.
// spec: SEAL-FRAME-001
// spec: SEAL-FRAME-002
// spec: SEAL-ENV-001
// spec: SEAL-ENV-002
// spec: SEAL-PLAIN-001
// spec: SEAL-PAD-001
// spec: SEAL-PLAIN-003
// spec: SEAL-CTX-001
#[test]
fn a_sealed_frame_byte_by_byte() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let based_on = Head { seq: 77, chain: Chain([0xCC; 32]) };
    let p = Plain { based_on, ..plain(Kind::Beat, b"{\"x\":1}".to_vec()) };
    let f = seal_frame(&Signer::Device(&d), &BOARD, 42, 7, &key(7), &p, &mut OsEntropy).unwrap();
    let len = f.len() - 112;
    assert_eq!(&f[0..4], b"TDF1");
    assert_eq!(&f[4..20], &BOARD.0);
    assert_eq!(&f[20..36], &d.public().dev.0);
    assert_eq!(&f[36..44], &42u64.to_be_bytes());
    assert_eq!(&f[44..48], &(len as u32).to_be_bytes());
    assert_eq!(f[48], 1);
    assert_eq!(f[49], 1);
    assert_eq!(&f[50..54], &7u32.to_be_bytes());
    let nonce = &f[54..78];
    let ct = &f[78..48 + len];
    assert_eq!(ct.len(), 512 + 16);
    // AD = ctx("entry") ‖ frame[0..48] ‖ ver ‖ suite ‖ epoch.
    let ad = [b"taskdeck/v1/entry\0".as_slice(), &f[0..48], &[1, 1], &7u32.to_be_bytes()].concat();
    use chacha20poly1305::aead::{Aead, KeyInit};
    let c = chacha20poly1305::XChaCha20Poly1305::new_from_slice(key(7).as_bytes()).unwrap();
    let pt = c.decrypt(nonce.try_into().unwrap(), Payload { msg: ct, aad: &ad }).unwrap();
    assert_eq!(pt.len(), 512);
    assert_eq!(&pt[0..16], &[0xE0; 16]);
    assert_eq!(pt[16], 3);
    assert_eq!(&pt[17..25], &1_700_000_000_000i64.to_be_bytes());
    assert_eq!(&pt[25..33], &77u64.to_be_bytes());
    assert_eq!(&pt[33..65], &[0xCC; 32]);
    assert_eq!(&pt[65..69], &7u32.to_be_bytes());
    assert_eq!(&pt[69..76], b"{\"x\":1}");
    assert!(pt[76..].iter().all(|&b| b == 0));
    // The signature.
    let signed = [b"taskdeck/v1/frame\0".as_slice(), &f[..48 + len]].concat();
    let vk = ed25519_dalek::VerifyingKey::from_bytes(&d.public().sign_pk).unwrap();
    let sig = ed25519_dalek::Signature::from_bytes(f[48 + len..].try_into().unwrap());
    vk.verify_strict(&signed, &sig).unwrap();
    // The other key of the epoch does not open it (SEAL-KEY-012's use).
    let snap = derive_epoch_key(&root(), &BOARD, 7, KeyKind::Snap);
    assert_eq!(open_envelope(&f, &snap).err(), Some(SealError::Aead));
    // And the receive side reads it back.
    assert_eq!(frame_header(&f).unwrap(), FrameHeader { board: BOARD, dev: d.public().dev, ctr: 42, len: len as u32 });
    verify_frame_signature(&f, &d.public().sign_pk).unwrap();
    assert_eq!(envelope_header(&f).unwrap(), EnvelopeHeader { ver: 1, suite: 1, epoch: 7 });
    assert_eq!(unpad(&open_envelope(&f, &key(7)).unwrap()).unwrap(), p);
    assert_eq!(frame_hash(&f).0, sha(&[&f]));
}

// A `cmd` payload comes back from the receive side unchanged, whatever its
// bytes (seal does not parse it).
// spec: SEAL-KIND-002
#[test]
fn a_cmd_payload_is_returned_unread() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    for payload in [vec![0xFF, 0x00, b'{'], vec![0u8; 443], b"not json at all".to_vec()] {
        let p = plain(Kind::Cmd, payload.clone());
        let f = seal_frame(&Signer::Device(&d), &BOARD, 1, 2, &key(2), &p, &mut OsEntropy).unwrap();
        let opened = unpad(&open_envelope(&f, &key(2)).unwrap()).unwrap();
        assert_eq!(opened.kind, Kind::Cmd);
        assert_eq!(opened.payload, payload);
    }
}

// The sizes: a 443-byte payload fills one bucket exactly, 444 needs two;
// 257,979 is the largest payload and its frame is 258,206 bytes; 257,980 is
// `seal/payload_too_large`.
// spec: SEAL-PAD-001
// spec: SEAL-PAD-002
#[test]
fn frame_sizes_and_the_cap() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let seal = |n: usize| seal_frame(&Signer::Device(&d), &BOARD, 1, 1, &key(1), &plain(Kind::Cmd, vec![7; n]), &mut OsEntropy);
    assert_eq!(seal(1).unwrap().len(), 112 + 30 + 512 + 16);
    assert_eq!(seal(1).unwrap().len(), 670);
    assert_eq!(seal(443).unwrap().len(), 670);
    assert_eq!(seal(444).unwrap().len(), 112 + 30 + 1024 + 16);
    assert_eq!(seal(8123).unwrap().len(), 112 + 30 + 8192 + 16);
    assert_eq!(seal(8124).unwrap().len(), 112 + 30 + 8704 + 16);
    assert_eq!(seal(257_979).unwrap().len(), 258_206);
    assert_eq!(seal(257_980).err(), Some(SealError::PayloadTooLarge));
    assert_eq!(seal(300_000).err(), Some(SealError::PayloadTooLarge));
}

// `ctr = 0` is `seal/frame_format`, refused before anything is drawn.
// spec: SEAL-FRAME-004
#[test]
fn ctr_zero_is_refused() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let r = seal_frame(&Signer::Device(&d), &BOARD, 0, 1, &key(1), &plain(Kind::Cmd, vec![1]), &mut OsEntropy);
    assert_eq!(r.err(), Some(SealError::FrameFormat));
}

// `based_on` is copied unchanged; at seq 0 only `C_0` is accepted.
// spec: SEAL-PLAIN-003
#[test]
fn based_on_is_the_callers() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let seal = |b: Head| {
        let p = Plain { based_on: b, ..plain(Kind::Cmd, vec![1]) };
        seal_frame(&Signer::Device(&d), &BOARD, 3, 1, &key(1), &p, &mut OsEntropy)
    };
    assert_eq!(seal(Head { seq: 0, chain: Chain([0; 32]) }).err(), Some(SealError::BadPlaintext));
    assert_eq!(seal(Head { seq: 0, chain: chain_zero(&BoardId([0xB1; 16])) }).err(), Some(SealError::BadPlaintext));
    for b in [Head { seq: 0, chain: chain_zero(&BOARD) }, Head { seq: 9, chain: Chain([0x99; 32]) }, Head { seq: u64::MAX, chain: Chain([0; 32]) }] {
        let f = seal(b).unwrap();
        assert_eq!(unpad(&open_envelope(&f, &key(1)).unwrap()).unwrap().based_on, b);
    }
}

// The `at_ms` row of §10.1's table: the value the sealer lays out at
// offsets 17 to 24 is the caller's, as a big-endian i64, at both ends of the
// range, and a value outside 0 to 253,402,300,799,999 is never laid out
// (the sealer refuses it with `seal/bad_plaintext`).
// spec: SEAL-PLAIN-001
#[test]
fn at_ms_range() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let seal = |p: &Plain| seal_frame(&Signer::Device(&d), &BOARD, 3, 1, &key(1), p, &mut OsEntropy);
    for at in [-1, AT_MS_MAX + 1, i64::MIN, i64::MAX] {
        assert_eq!(seal(&Plain { at_ms: at, ..plain(Kind::Cmd, vec![1]) }).err(), Some(SealError::BadPlaintext), "{at}");
    }
    for at in [0, 1, AT_MS_MAX] {
        let f = seal(&Plain { at_ms: at, ..plain(Kind::Cmd, vec![1]) }).unwrap();
        let pt = open_envelope(&f, &key(1)).unwrap();
        assert_eq!(pt[17..25], at.to_be_bytes(), "{at}");
        assert_eq!(unpad(&pt).unwrap().at_ms, at);
    }
}

// What every receiver would refuse at step 11 is not sealed: an empty `cmd`
// (a beat may be empty).
#[test]
fn empty_cmd_is_not_sealed() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let seal = |p: &Plain| seal_frame(&Signer::Device(&d), &BOARD, 3, 1, &key(1), p, &mut OsEntropy);
    assert_eq!(seal(&plain(Kind::Cmd, vec![])).err(), Some(SealError::BadPlaintext));
    assert!(seal(&plain(Kind::Beat, vec![])).is_ok());
}

// A frame signed by `Signer::Recovery` carries `dev_R` and the counter the
// caller gives; a recovery signer of another board is refused.
// spec: SEAL-KEY-007
#[test]
fn a_recovery_frame_carries_dev_r() {
    let seed = RecoverySeed::generate(&mut OsEntropy).unwrap();
    let rk = seed.keys();
    let dev_r = rk.public().dev(&BOARD);
    for ctr in [1, 2, 1000] {
        let f = seal_frame(&Signer::Recovery(&rk, BOARD), &BOARD, ctr, 1, &key(1), &plain(Kind::Ctl, vec![1]), &mut OsEntropy).unwrap();
        let h = frame_header(&f).unwrap();
        assert_eq!(h.dev, dev_r);
        assert_eq!(h.ctr, ctr);
        verify_frame_signature(&f, &rk.public().sign_pk).unwrap();
    }
    let r = seal_frame(&Signer::Recovery(&rk, BoardId([1; 16])), &BOARD, 1, 1, &key(1), &plain(Kind::Ctl, vec![1]), &mut OsEntropy);
    assert_eq!(r.err(), Some(SealError::UnknownAuthor));
}

// Each sealing draws a fresh nonce: two sealings of the same entry differ
// in nonce and ciphertext and keep the entry id.
// spec: SEAL-GEN-012
// spec: SEAL-PLAIN-002
#[test]
fn every_sealing_draws_a_fresh_nonce() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let p = plain(Kind::Cmd, vec![5; 10]);
    let a = seal_frame(&Signer::Device(&d), &BOARD, 1, 1, &key(1), &p, &mut OsEntropy).unwrap();
    let b = seal_frame(&Signer::Device(&d), &BOARD, 1, 1, &key(1), &p, &mut OsEntropy).unwrap();
    assert_ne!(a[54..78], b[54..78]);
    assert_ne!(a[78..], b[78..]);
    for f in [&a, &b] {
        assert_eq!(unpad(&open_envelope(f, &key(1)).unwrap()).unwrap().entry_id, p.entry_id);
    }
    assert_ne!(new_entry_id(&mut OsEntropy).unwrap(), new_entry_id(&mut OsEntropy).unwrap());
}

// `seal_frame` draws exactly the 24 bytes of the nonce and puts them in
// the body; `new_entry_id` draws exactly 16; a short source is
// `seal/entropy`, and nothing is drawn by a refused call.
// spec: SEAL-GEN-012
// spec: SEAL-PLAIN-002
#[cfg(feature = "test-entropy")]
#[test]
fn the_draws() {
    use crate::FixedEntropy;
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let p = plain(Kind::Cmd, vec![5; 10]);
    let bytes: Vec<u8> = (0..48).collect();
    let mut rng = FixedEntropy::new(bytes.clone());
    let a = seal_frame(&Signer::Device(&d), &BOARD, 1, 1, &key(1), &p, &mut rng).unwrap();
    // A refused call draws nothing.
    assert_eq!(seal_frame(&Signer::Device(&d), &BOARD, 0, 1, &key(1), &p, &mut rng).err(), Some(SealError::FrameFormat));
    let big = plain(Kind::Cmd, vec![0; 257_980]);
    assert_eq!(seal_frame(&Signer::Device(&d), &BOARD, 1, 1, &key(1), &big, &mut rng).err(), Some(SealError::PayloadTooLarge));
    let b = seal_frame(&Signer::Device(&d), &BOARD, 2, 1, &key(1), &p, &mut rng).unwrap();
    assert_eq!(a[54..78], bytes[..24]);
    assert_eq!(b[54..78], bytes[24..]);
    assert_eq!(seal_frame(&Signer::Device(&d), &BOARD, 3, 1, &key(1), &p, &mut rng).err(), Some(SealError::Entropy));
    let mut short = FixedEntropy::new(vec![0; 23]);
    assert_eq!(seal_frame(&Signer::Device(&d), &BOARD, 1, 1, &key(1), &p, &mut short).err(), Some(SealError::Entropy));
    let mut rng = FixedEntropy::new((0..32).collect());
    assert_eq!(new_entry_id(&mut rng).unwrap().0, core::array::from_fn::<u8, 16, _>(|i| i as u8));
    assert_eq!(new_entry_id(&mut rng).unwrap().0, core::array::from_fn::<u8, 16, _>(|i| 16 + i as u8));
    assert_eq!(new_entry_id(&mut rng).err(), Some(SealError::Entropy));
}

// ---------------------------------------------------------- receive side

fn sealed(d: &DeviceKeys) -> Vec<u8> {
    seal_frame(&Signer::Device(d), &BOARD, 5, 3, &key(3), &plain(Kind::Cmd, b"hello".to_vec()), &mut OsEntropy).unwrap()
}

// Step 1: the total length 670 to 262,144, the magic and `len` = total −
// 112, each refused with `seal/frame_format`; a length field that does not
// match the bytes remaining is never trusted.
// spec: SEAL-FRAME-003
// spec: SEAL-GEN-008
#[test]
fn the_layout_check() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let f = sealed(&d);
    assert!(frame_header(&f).is_ok());
    let bad = Some(SealError::FrameFormat);
    assert_eq!(frame_header(&f[..669]).err(), bad);
    assert_eq!(frame_header(&[]).err(), bad);
    assert_eq!(frame_header(&f[..47]).err(), bad);
    let mut m = f.clone();
    m[0] = b't';
    assert_eq!(frame_header(&m).err(), bad);
    for delta in [1i64, -1, 1 << 20] {
        let mut m = f.clone();
        let len = u32::from_be_bytes(m[44..48].try_into().unwrap()) as i64 + delta;
        m[44..48].copy_from_slice(&(len as u32).to_be_bytes());
        assert_eq!(frame_header(&m).err(), bad, "{delta}");
    }
    // One byte appended or removed, with len unchanged.
    let mut m = f.clone();
    m.push(0);
    assert_eq!(frame_header(&m).err(), bad);
    // The bounds themselves, with a consistent len: 670 and 262,144 pass
    // step 1, 669 and 262,145 do not.
    let frame_of = |total: usize| {
        let mut v = vec![0u8; total];
        v[..4].copy_from_slice(b"TDF1");
        v[44..48].copy_from_slice(&((total - 112) as u32).to_be_bytes());
        v
    };
    assert!(frame_header(&frame_of(670)).is_ok());
    assert!(frame_header(&frame_of(262_144)).is_ok());
    assert_eq!(frame_header(&frame_of(669)).err(), bad);
    assert_eq!(frame_header(&frame_of(262_145)).err(), bad);
    // ctr = 0 is not a layout error (step 5 checks counters).
    assert_eq!(frame_header(&frame_of(670)).unwrap().ctr, 0);
}

// Step 4 under the author's key only; another key, a key that does not
// decode, or a changed frame is `seal/bad_signature`.
// spec: SEAL-FRAME-002
// spec: SEAL-GEN-010
#[test]
fn the_signature_check() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let other = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let f = sealed(&d);
    assert_eq!(verify_frame_signature(&f, &d.public().sign_pk), Ok(()));
    assert_eq!(verify_frame_signature(&f, &other.public().sign_pk), Err(SealError::BadSignature));
    assert_eq!(verify_frame_signature(&f, &[0; 32]), Err(SealError::BadSignature));
    // A small-order public key (the identity point) with the identity R and
    // S = 0 verifies under non-strict rules for any message; strictly, not.
    let mut forged = f.clone();
    let n = forged.len();
    let mut identity = [0u8; 32];
    identity[0] = 1;
    forged[n - 64..n - 32].copy_from_slice(&identity);
    forged[n - 32..].fill(0);
    assert_eq!(verify_frame_signature(&forged, &identity), Err(SealError::BadSignature));
    // S not reduced (S + ℓ) is refused.
    let mut high = f.clone();
    let l: [u8; 32] = [
        0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
    ];
    let mut carry = 0u16;
    for i in 0..32 {
        let s = u16::from(high[n - 32 + i]) + u16::from(l[i]) + carry;
        high[n - 32 + i] = s as u8;
        carry = s >> 8;
    }
    assert_eq!(verify_frame_signature(&high, &d.public().sign_pk), Err(SealError::BadSignature));
    assert_eq!(verify_frame_signature(&f[..f.len() - 1], &d.public().sign_pk), Err(SealError::FrameFormat));
}

// Flipping any byte of a sealed frame makes the layout check or the
// signature check fail (the tamper target gets frames with `classify`,
// task S3).
// spec: SEAL-FRAME-002
// spec: SEAL-FRAME-003
#[test]
fn every_flipped_byte_is_caught() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let f = sealed(&d);
    let pk = d.public().sign_pk;
    for i in 0..f.len() {
        for x in [0x01u8, 0x80, 0xFF] {
            let mut m = f.clone();
            m[i] ^= x;
            let caught = frame_header(&m).is_err() || verify_frame_signature(&m, &pk).is_err();
            assert!(caught, "byte {i} ^ {x:#x}");
        }
    }
}

// The AEAD is checked before any plaintext byte is returned: a changed
// header field (the AD), body field or ciphertext byte is `seal/aead`,
// whatever the signature, and the plaintext is never handed out.
// spec: SEAL-ENV-002
// spec: SEAL-GEN-009
#[test]
fn every_flipped_byte_fails_the_aead() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let f = sealed(&d);
    let k = key(3);
    assert!(open_envelope(&f, &k).is_ok());
    let len = f.len() - 112;
    for i in 0..48 + len {
        let mut m = f.clone();
        m[i] ^= 0x01;
        let r = open_envelope(&m, &k);
        // Flipping magic or len fails the layout first; everything else the AEAD.
        let want = if i < 4 || (44..48).contains(&i) { SealError::FrameFormat } else { SealError::Aead };
        assert_eq!(r.err(), Some(want), "byte {i}");
    }
    // The signature is not part of the AD: the envelope still opens.
    let mut m = f.clone();
    m[48 + len] ^= 1;
    assert!(open_envelope(&m, &k).is_ok());
    // Under another epoch's key, or the AD of another use, it does not open.
    assert_eq!(open_envelope(&f, &key(4)).err(), Some(SealError::Aead));
    use chacha20poly1305::aead::{Aead, KeyInit};
    let c = chacha20poly1305::XChaCha20Poly1305::new_from_slice(k.as_bytes()).unwrap();
    for other in [crate::ctx::BLOB, crate::ctx::FILE, crate::ctx::PAIR_BUNDLE] {
        let ad = [other, &f[0..48], &f[48..54]].concat();
        assert!(c.decrypt(f[54..78].try_into().unwrap(), Payload { msg: &f[78..48 + len], aad: &ad }).is_err());
    }
}

// ---------------------------------------------------------------- unpad

fn good_plaintext(payload_len: usize) -> Vec<u8> {
    let p = plain(Kind::Snap, vec![0x5A; payload_len]);
    pad_plaintext(&BOARD, &p).unwrap()
}

fn set_len(pt: &mut [u8], n: u32) {
    pt[65..69].copy_from_slice(&n.to_be_bytes());
}

// The five checks of §10.3, each failing alone, and the order between
// them when two fail at once; `unpad` maps every one to `seal/bad_plaintext`.
// spec: SEAL-PAD-003
// spec: SEAL-GEN-008
#[test]
fn the_five_unpadding_checks() {
    use UnpadCheck::*;
    let ok = good_plaintext(10);
    assert_eq!(unpad_checked(&ok).unwrap().payload, vec![0x5A; 10]);
    // 1.
    assert_eq!(unpad_checked(&ok[..68]).err(), Some(Length));
    assert_eq!(unpad_checked(&[]).err(), Some(Length));
    // 2: U > L, including a length that would overflow 32 bits.
    let mut m = ok.clone();
    set_len(&mut m, 512 - 69 + 1);
    assert_eq!(unpad_checked(&m).err(), Some(PayloadLen));
    set_len(&mut m, u32::MAX);
    assert_eq!(unpad_checked(&m).err(), Some(PayloadLen));
    let mut short = ok[..69].to_vec();
    set_len(&mut short, 1);
    assert_eq!(unpad_checked(&short).err(), Some(PayloadLen));
    // 3: P(U) ≠ L.
    let mut long = good_plaintext(10);
    long.resize(1024, 0);
    assert_eq!(unpad_checked(&long).err(), Some(Padded));
    assert_eq!(unpad_checked(&ok[..511]).err(), Some(Padded));
    let mut exact = ok[..69].to_vec();
    set_len(&mut exact, 0);
    assert_eq!(unpad_checked(&exact).err(), Some(Padded));
    // 4: a non-zero padding byte, at U and at the last byte.
    for i in [79, 511] {
        let mut m = ok.clone();
        m[i] = 1;
        assert_eq!(unpad_checked(&m).err(), Some(Zeros), "{i}");
    }
    // 5: kind and at_ms.
    for k in [0u8, 5, 255] {
        let mut m = ok.clone();
        m[16] = k;
        assert_eq!(unpad_checked(&m).err(), Some(KindTime), "kind {k}");
    }
    for at in [-1i64, AT_MS_MAX + 1, i64::MIN, i64::MAX] {
        let mut m = ok.clone();
        m[17..25].copy_from_slice(&at.to_be_bytes());
        assert_eq!(unpad_checked(&m).err(), Some(KindTime), "at_ms {at}");
    }
    for (k, at) in [(1u8, 0i64), (4, AT_MS_MAX)] {
        let mut m = ok.clone();
        m[16] = k;
        m[17..25].copy_from_slice(&at.to_be_bytes());
        assert!(unpad_checked(&m).is_ok());
    }
    // The order: 2 before 5, 3 before 4, 4 before 5.
    let mut m = ok.clone();
    m[16] = 0;
    set_len(&mut m, 600);
    assert_eq!(unpad_checked(&m).err(), Some(PayloadLen));
    let mut m = long.clone();
    m[1000] = 1;
    assert_eq!(unpad_checked(&m).err(), Some(Padded));
    let mut m = ok.clone();
    m[500] = 9;
    m[16] = 9;
    assert_eq!(unpad_checked(&m).err(), Some(Zeros));
    assert_eq!(unpad(&m).err(), Some(SealError::BadPlaintext));
}

// ---------------------------------------------- the committed vectors

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "{s}");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn vectors(file: &str) -> serde_json::Value {
    let path = format!("{}/../../docs/sync/vectors/{file}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap()
}

// Every case of the committed seal-padding.json, which the vectors target
// writes: P(U) as `padded_len` gives it, and each malformed plaintext
// failing the check the file names (the unpadding error is private).
// spec: SEAL-PAD-001
// spec: SEAL-PAD-003
#[test]
fn published_padding_cases() {
    let doc = vectors("seal-padding.json");
    let (mut sizes, mut plaintexts) = (0, 0);
    for c in doc["cases"].as_array().unwrap() {
        if let Some(u) = c.get("unpadded") {
            assert_eq!(padded_len(u.as_u64().unwrap()), c["padded"].as_u64().unwrap(), "{c}");
            sizes += 1;
        }
        if let Some(pt) = c.get("plaintext") {
            let pt = unhex(pt.as_str().unwrap());
            match c.get("check") {
                Some(check) => {
                    assert_eq!(unpad_checked(&pt).err().map(|e| e as u64), check.as_u64(), "{c}");
                    assert_eq!(c["error"], "seal/bad_plaintext");
                    assert_eq!(unpad(&pt).err().map(|e| e.code()), Some("seal/bad_plaintext"));
                }
                None => {
                    let p = unpad_checked(&pt).unwrap();
                    assert_eq!(p.at_ms, c["at_ms"].as_i64().unwrap(), "{c}");
                    assert_eq!(p.kind as u64, c["kind"].as_u64().unwrap(), "{c}");
                    assert_eq!(p.payload, unhex(c["payload"].as_str().unwrap()), "{c}");
                }
            }
            plaintexts += 1;
        }
    }
    assert!(sizes >= 7 && plaintexts >= 8, "{sizes} sizes, {plaintexts} plaintexts");
}

// Every frame of the committed seal-frame.json verifies under its author's
// key, opens under K_entry from its root to the published padded plaintext
// and unpads to the published fields.
// The rejected frames fail the step the file names with its code.
// spec: SEAL-FRAME-002
// spec: SEAL-FRAME-003
// spec: SEAL-ENV-002
// spec: SEAL-PLAIN-001
#[test]
fn published_frames_open() {
    let doc = vectors("seal-frame.json");
    let mut seen = 0;
    let mut rejected = 0;
    for c in doc["cases"].as_array().unwrap() {
        if let Some(bytes) = c.get("frame_bytes") {
            let f = unhex(bytes.as_str().unwrap());
            let r = match c["step"].as_u64() {
                Some(1) => frame_header(&f).map(|_| ()),
                Some(4) => verify_frame_signature(&f, &unhex(c["sign_pk"].as_str().unwrap()).try_into().unwrap()),
                s => panic!("step {s:?} in {c}"),
            };
            assert_eq!(r.err().map(|e| e.code()), c["error"].as_str(), "{c}");
            rejected += 1;
        }
        let Some(frame) = c.get("frame") else { continue };
        let f = unhex(frame.as_str().unwrap());
        let field = |k: &str| unhex(c[k].as_str().unwrap_or_else(|| panic!("{k} in {c}")));
        let pk: [u8; 32] = field("sign_pk").try_into().unwrap();
        verify_frame_signature(&f, &pk).unwrap();
        let board = BoardId(field("board_id").try_into().unwrap());
        let e = c["epoch"].as_u64().unwrap() as u32;
        let k = derive_epoch_key(&EpochRoot::from_bytes(field("root").try_into().unwrap()), &board, e, KeyKind::Entry);
        assert_eq!(k.as_bytes().to_vec(), field("k_entry"));
        let pt = open_envelope(&f, &k).unwrap();
        assert_eq!(pt, field("padded_plaintext"), "{}", c["name"]);
        let p = unpad(&pt).unwrap();
        assert_eq!(p.payload, field("payload"));
        assert_eq!(p.entry_id.0.to_vec(), field("entry_id"));
        assert_eq!(frame_hash(&f).0.to_vec(), field("frame_hash"));
        seen += 1;
    }
    assert!(seen >= 4 && rejected >= 4, "{seen} frames, {rejected} rejected");
}

// ---------------------------------------------------------- properties

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    // The private receive-side parsers return, never panic, on random and
    // mutated frames and plaintexts.
    // spec: SEAL-GEN-011
    // spec: SEAL-GEN-008
    #[test]
    fn receive_side_never_panics(
        random in proptest::collection::vec(any::<u8>(), 0..800),
        at in any::<usize>(), x in any::<u8>(), cut in any::<usize>(), len_field in any::<u32>(),
    ) {
        let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
        let f = sealed(&d);
        let pk = d.public().sign_pk;
        let mut m = f.clone();
        let i = at % m.len();
        m[i] ^= x | 1;
        let mut lf = f.clone();
        lf[44..48].copy_from_slice(&len_field.to_be_bytes());
        let cutf = f[..cut % (f.len() + 1)].to_vec();
        let k = key(3);
        for input in [&random, &m, &lf, &cutf] {
            let _ = frame_header(input);
            let _ = verify_frame_signature(input, &pk);
            let _ = envelope_header(input);
            let _ = open_envelope(input, &k);
            let _ = unpad(input);
        }
        let mut pt = good_plaintext(at % 600);
        let j = at % pt.len();
        pt[j] ^= x;
        let _ = unpad_checked(&pt);
        pt[65..69].copy_from_slice(&len_field.to_be_bytes());
        let _ = unpad_checked(&pt);
        let _ = unpad_checked(&random);
    }
}
