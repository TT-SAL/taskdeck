use super::*;
use sha2::{Digest, Sha256};

/// Whether a type implements a trait, decided at compile time (the
/// inherent method wins only when the bound holds).
macro_rules! implements {
    ($t:ty: $($tr:tt)+) => {{
        struct Check<T: ?Sized>(core::marker::PhantomData<T>);
        #[allow(dead_code)]
        trait No { fn yes(&self) -> bool { false } }
        impl<T: ?Sized> No for Check<T> {}
        #[allow(dead_code)]
        impl<T: ?Sized + $($tr)+> Check<T> { fn yes(&self) -> bool { true } }
        Check::<$t>(core::marker::PhantomData).yes()
    }};
}

fn sha(parts: &[&[u8]]) -> [u8; 32] {
    let mut s = Sha256::new();
    for p in parts {
        s.update(p);
    }
    s.finalize().into()
}

fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    hkdf::Hkdf::<Sha256>::new(Some(salt), ikm).expand(info, &mut out).unwrap();
    out
}

fn hpke_pk(ikm: &[u8]) -> [u8; 32] {
    use hpke::{Kem as _, Serializable as _};
    let (_, pk) = hpke::kem::X25519HkdfSha256::derive_keypair(ikm);
    pk.to_bytes().into()
}

fn seed(bytes: [u8; 32]) -> RecoverySeed {
    RecoverySeed { seed: bytes }
}

fn device(dev: [u8; 16], sk: [u8; 32], ikm: [u8; 32]) -> DeviceKeys {
    DeviceKeys {
        dev: DevId(dev),
        sign: ed25519_dalek::SigningKey::from_bytes(&sk),
        hpke: hpke_derive(&ikm).0,
    }
}

const BOARD: BoardId = BoardId([0x42; 16]);

// The recovery keys are the HKDF outputs of §6.1 under their own contexts:
// the Ed25519 key whose private key is sk_r, and DeriveKeyPair(ikm_h).
// spec: SEAL-KEY-002
// spec: SEAL-KEY-003
// spec: SEAL-CTX-001
#[test]
fn recovery_keys_follow_section_6_1() {
    let s: [u8; 32] = core::array::from_fn(|i| i as u8);
    let keys = seed(s).keys();
    let sk_r = hkdf(&[], &s, b"taskdeck/v1/recovery-sign\0");
    let ikm_h = hkdf(&[], &s, b"taskdeck/v1/recovery-hpke\0");
    let public = keys.public();
    assert_eq!(public.sign_pk, ed25519_dalek::SigningKey::from_bytes(&sk_r).verifying_key().to_bytes());
    assert_eq!(public.hpke_pk, hpke_pk(&ikm_h));
    assert_ne!(sk_r, ikm_h);
}

// spec: SEAL-KEY-005
// spec: SEAL-KEY-006
// spec: SEAL-CTX-001
#[test]
fn board_id_and_recovery_dev() {
    let pk = [7u8; 32];
    let salt = [9u8; 16];
    let b = board_id(&pk, &salt);
    assert_eq!(b.0[..], sha(&[b"taskdeck/v1/board\0", &pk, &salt])[..16]);
    let public = RecoveryPublic { sign_pk: pk, hpke_pk: [1u8; 32] };
    let d = public.dev(&b);
    assert_eq!(d.0[..], sha(&[b"taskdeck/v1/recovery-dev\0", &b.0, &pk])[..16]);
    // A new recovery key on the same board has another dev_R.
    assert_ne!(RecoveryPublic { sign_pk: [8u8; 32], hpke_pk: [1u8; 32] }.dev(&b), d);
}

// spec: SEAL-KEY-011
// spec: SEAL-KEY-012
// spec: SEAL-CTX-001
#[test]
fn commitment_and_epoch_keys() {
    let r = [0x5Au8; 32];
    let root = EpochRoot::from_bytes(r);
    for e in [1u32, 2, 0x0102_0304, u32::MAX] {
        let eb = e.to_be_bytes();
        assert_eq!(root.commitment(&BOARD, e), sha(&[b"taskdeck/v1/epoch\0", &BOARD.0, &eb, &r]));
        let k = |kind| *derive_epoch_key(&root, &BOARD, e, kind).as_bytes();
        let entry = k(KeyKind::Entry);
        let snap = k(KeyKind::Snap);
        let blob = k(KeyKind::Blob);
        assert_eq!(entry, hkdf(&BOARD.0, &r, &[b"taskdeck/v1/key-entry\0".as_slice(), &eb].concat()));
        assert_eq!(snap, hkdf(&BOARD.0, &r, &[b"taskdeck/v1/key-snap\0".as_slice(), &eb].concat()));
        assert_eq!(blob, hkdf(&BOARD.0, &r, &[b"taskdeck/v1/key-blob\0".as_slice(), &eb].concat()));
        assert_ne!(entry, snap);
        assert_ne!(entry, blob);
        assert_ne!(snap, blob);
    }
    assert_eq!(*root.to_bytes(), r);
}

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "{s}");
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

// Every K of the committed docs/sync/vectors/seal-kdf.json, which the vectors
// target computes with the hkdf crate, is what derive_epoch_key gives, so
// the crate and the published file cannot drift apart.
// spec: SEAL-KEY-012
#[test]
fn published_epoch_keys_match_derive_epoch_key() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/sync/vectors/seal-kdf.json");
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut seen = 0;
    for c in doc["cases"].as_array().unwrap() {
        let Some(k_entry) = c.get("k_entry") else { continue };
        let field = |k: &str| c[k].as_str().unwrap_or_else(|| panic!("{k} in {c}"));
        let board = BoardId(unhex(field("board_id")).try_into().unwrap());
        let root = EpochRoot::from_bytes(unhex(field("root")).try_into().unwrap());
        let e = u32::try_from(c["epoch"].as_u64().unwrap()).unwrap();
        for (kind, want) in [
            (KeyKind::Entry, k_entry.as_str().unwrap()),
            (KeyKind::Snap, field("k_snap")),
            (KeyKind::Blob, field("k_blob")),
        ] {
            assert_eq!(derive_epoch_key(&root, &board, e, kind).as_bytes().to_vec(), unhex(want), "{kind:?} {c}");
        }
        assert_eq!(root.commitment(&board, e).to_vec(), unhex(field("commitment")), "{c}");
        seen += 1;
    }
    assert!(seen >= 4, "the file has {seen} epoch cases");
}

/// The sheet of a seed, built from §18.1's text alone.
fn sheet_by_hand(s: &[u8; 32]) -> String {
    const A: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut bits: Vec<u8> = s.iter().flat_map(|b| (0..8).rev().map(move |k| (b >> k) & 1)).collect();
    bits.extend([0, 0, 0, 0]);
    let d = sha(&[b"taskdeck/v1/sheet-check\0", s]);
    bits.extend(d.iter().flat_map(|b| (0..8).rev().map(move |k| (b >> k) & 1)).take(20));
    let chars: Vec<char> = bits
        .chunks(5)
        .map(|c| char::from(A[c.iter().fold(0usize, |a, &b| (a << 1) | usize::from(b))]))
        .collect();
    chars.chunks(4).map(|g| g.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

// spec: SEAL-SHEET-001
#[test]
fn sheet_text_is_seed_then_check_in_fourteen_groups() {
    for s in [[0u8; 32], [0xFF; 32], core::array::from_fn(|i| (i * 37 + 11) as u8)] {
        let text = seed(s).to_sheet();
        assert_eq!(*text, sheet_by_hand(&s));
        let groups: Vec<&str> = text.split(' ').collect();
        assert_eq!(groups.len(), 14);
        assert!(groups.iter().all(|g| g.len() == 4));
        assert_eq!(groups[..13].concat(), base32::encode(&s));
        assert_eq!(RecoverySeed::from_sheet(&text).unwrap().seed, s);
    }
}

// The four steps of §18.2 in order; the first failing step names the error.
// spec: SEAL-SHEET-002
// spec: SEAL-ENC-002
#[test]
fn reading_a_sheet_follows_the_four_steps() {
    let s: [u8; 32] = core::array::from_fn(|i| (i * 7) as u8);
    let good: String = seed(s).to_sheet().chars().filter(|c| *c != ' ').collect();
    // Step 1: spaces, hyphens, tabs, case, O, I and L.
    let typed: String = good
        .chars()
        .enumerate()
        .map(|(i, c)| match c {
            '0' if i % 2 == 0 => 'o',
            '1' if i % 2 == 0 => 'L',
            '1' => 'i',
            c => c.to_ascii_lowercase(),
        })
        .collect();
    let typed = format!(" {}-\t{} ", &typed[..10], &typed[10..]);
    assert_eq!(RecoverySeed::from_sheet(&typed).unwrap().seed, s);
    // Step 2: length, then characters.
    let err = |t: &str| RecoverySeed::from_sheet(t).err();
    assert_eq!(err(&good[..55]), Some(SealError::SheetLength));
    assert_eq!(err(&format!("{good}0")), Some(SealError::SheetLength));
    assert_eq!(err(""), Some(SealError::SheetLength));
    assert_eq!(err(&format!("U{}", &good[1..])), Some(SealError::SheetChar));
    assert_eq!(err(&format!("U{good}")), Some(SealError::SheetLength));
    // Step 3: the 4 padding bits after the seed (the low 4 bits of char 52).
    let mut padded: Vec<u8> = good.clone().into_bytes();
    let v = base32::ALPHABET.iter().position(|&a| a == padded[51]).unwrap();
    padded[51] = base32::ALPHABET[v | 1];
    let padded = String::from_utf8(padded).unwrap();
    // Only a padding bit is set: the 56th character, the last of the check, is good.
    assert_eq!(err(&padded), Some(SealError::SheetPadding));
    // A padding error wins over a check error: padding bits and the last
    // check character both wrong (step 3 comes before step 4).
    let mut both: Vec<u8> = padded.into_bytes();
    both[55] = if both[55] == b'0' { b'1' } else { b'0' };
    assert_eq!(err(&String::from_utf8(both).unwrap()), Some(SealError::SheetPadding));
    // Step 4: the check.
    let mut bad: Vec<u8> = good.clone().into_bytes();
    bad[55] = if bad[55] == b'0' { b'1' } else { b'0' };
    assert_eq!(err(&String::from_utf8(bad).unwrap()), Some(SealError::SheetCheck));
    let mut bad_seed: Vec<u8> = good.into_bytes();
    bad_seed[0] = if bad_seed[0] == b'0' { b'1' } else { b'0' };
    assert_eq!(err(&String::from_utf8(bad_seed).unwrap()), Some(SealError::SheetCheck));
}

// Secret types erase themselves on drop and have no Debug, Display, Clone
// or Serialize; the seed leaves only as the sheet, in a zeroizing string;
// private bytes leave only in zeroizing buffers.
// spec: SEAL-GEN-006
// spec: SEAL-KEY-004
// spec: SEAL-KEY-009
// spec: SEAL-SHEET-003
#[test]
fn secret_types_zeroize_and_do_not_show() {
    use zeroize::ZeroizeOnDrop;
    fn zod<T: ZeroizeOnDrop>() {}
    zod::<RecoverySeed>();
    zod::<RecoveryKeys>();
    zod::<DeviceKeys>();
    zod::<EpochRoot>();
    zod::<SymKey>();
    assert!(implements!(RecoverySeed: ZeroizeOnDrop));
    macro_rules! hidden {
        ($($t:ty),*) => {$(
            assert!(!implements!($t: core::fmt::Debug), stringify!($t));
            assert!(!implements!($t: core::fmt::Display), stringify!($t));
            assert!(!implements!($t: Clone), stringify!($t));
            assert!(!implements!($t: serde::Serialize), stringify!($t));
        )*};
    }
    hidden!(RecoverySeed, RecoveryKeys, DeviceKeys, EpochRoot, SymKey);
    assert!(implements!(RecoveryPublic: core::fmt::Debug));
    let s = seed([3u8; 32]);
    let _: Zeroizing<String> = s.to_sheet();
    let d = device([1; 16], [2; 32], [3; 32]);
    let _: Zeroizing<Vec<u8>> = d.to_identity_bytes();
    let _: Zeroizing<[u8; 32]> = EpochRoot::from_bytes([4; 32]).to_bytes();
}

// The layout of `to_identity_bytes`, its round trip, and the rejection of
// every other length and of an unclamped X25519 key.
// spec: SEAL-KEY-009
#[test]
fn identity_bytes_round_trip() {
    let d = device([1; 16], [2; 32], [3; 32]);
    let bytes = d.to_identity_bytes();
    assert_eq!(bytes.len(), 80);
    assert_eq!(bytes[..16], [1; 16]);
    assert_eq!(bytes[16..48], [2; 32]);
    assert_eq!(bytes[48..], *d.hpke.as_bytes());
    let back = DeviceKeys::from_identity_bytes(&bytes).unwrap();
    assert_eq!(back.public(), d.public());
    assert_eq!(*back.to_identity_bytes(), *bytes);
    for len in (0..200).filter(|&l| l != 80) {
        let v: Vec<u8> = bytes.iter().copied().cycle().take(len).collect();
        assert_eq!(DeviceKeys::from_identity_bytes(&v).err(), Some(SealError::FileCorrupt), "{len}");
    }
    for (i, mask) in [(48, 0x01u8), (48, 0x04), (79, 0x80), (79, 0x40)] {
        let mut v = bytes.to_vec();
        v[i] ^= mask;
        assert_eq!(DeviceKeys::from_identity_bytes(&v).err(), Some(SealError::FileCorrupt));
    }
}

/// hpke's ciphertext opened by hand with §7's info and aad.
fn open_by_hand(wrap: &[u8; 96], sk: &[u8; 32], info: &[u8], aad: &[u8]) -> Option<Vec<u8>> {
    use hpke::Deserializable as _;
    let sk = <Kem as hpke::Kem>::PrivateKey::from_bytes(sk).ok()?;
    let enc = <Kem as hpke::Kem>::EncappedKey::from_bytes(&wrap[16..48]).ok()?;
    hpke::single_shot_open::<HpkeAead, Kdf, Kem>(&hpke::OpModeR::Base, &sk, &enc, info, &wrap[48..], aad).ok()
}

fn info_by_hand(e: u32, id: &[u8; 16], pk: &[u8; 32]) -> Vec<u8> {
    [b"taskdeck/v1/wrap\0".as_slice(), &BOARD.0, &e.to_be_bytes(), id, pk].concat()
}

fn aad_by_hand(c: &[u8; 32]) -> Vec<u8> {
    [b"taskdeck/v1/wrap-aad\0".as_slice(), &[1, 1, 0x00, 0x20, 0x00, 0x01, 0x00, 0x03], c].concat()
}

// A wrap is the recipient id, enc and the HPKE ciphertext of the root under
// §7's info and aad; it opens for its recipient, device or recovery key.
// spec: SEAL-WRAP-001
// spec: SEAL-WRAP-002
// spec: SEAL-CTX-001
#[test]
fn wrap_layout_and_open() {
    let d = device([0x11; 16], [0x22; 32], [0x33; 32]);
    let root = EpochRoot::from_bytes([0x44; 32]);
    let e = 3;
    let c = root.commitment(&BOARD, e);
    let p = d.public();
    let w = wrap_root(&root, &BOARD, e, &c, p.dev, &p.hpke_pk, &mut crate::OsEntropy).unwrap();
    assert_eq!(w[..16], p.dev.0);
    let pt = open_by_hand(&w, d.hpke.as_bytes(), &info_by_hand(e, &p.dev.0, &p.hpke_pk), &aad_by_hand(&c)).unwrap();
    assert_eq!(pt, [0x44; 32]);
    let got = unwrap_root(&w, &BOARD, e, &c, &Recipient::Device(&d)).unwrap();
    assert_eq!(*got.to_bytes(), [0x44; 32]);

    let rk = seed([5; 32]).keys();
    let rp = rk.public();
    let dev_r = rp.dev(&BOARD);
    let w = wrap_root(&root, &BOARD, e, &c, dev_r, &rp.hpke_pk, &mut crate::OsEntropy).unwrap();
    let pt = open_by_hand(&w, rk.hpke.as_bytes(), &info_by_hand(e, &dev_r.0, &rp.hpke_pk), &aad_by_hand(&c)).unwrap();
    assert_eq!(pt, [0x44; 32]);
    assert!(unwrap_root(&w, &BOARD, e, &c, &Recipient::Recovery(&rk, BOARD)).is_ok());
    // The recipient's board must be the board of the call.
    assert_eq!(
        unwrap_root(&w, &BOARD, e, &c, &Recipient::Recovery(&rk, BoardId([0x43; 16]))).err(),
        Some(SealError::Commitment)
    );
}

// A root that opens but does not match the commitment is refused.
// spec: SEAL-KEY-013
#[test]
fn an_opened_root_must_match_its_commitment() {
    let d = device([0x11; 16], [0x22; 32], [0x33; 32]);
    let p = d.public();
    let good = EpochRoot::from_bytes([0x44; 32]);
    let other = EpochRoot::from_bytes([0x45; 32]);
    let c = good.commitment(&BOARD, 2);
    // A dishonest author wraps another root under the honest commitment.
    let w = wrap_root(&other, &BOARD, 2, &c, p.dev, &p.hpke_pk, &mut crate::OsEntropy).unwrap();
    assert!(open_by_hand(&w, d.hpke.as_bytes(), &info_by_hand(2, &p.dev.0, &p.hpke_pk), &aad_by_hand(&c)).is_some());
    assert_eq!(unwrap_root(&w, &BOARD, 2, &c, &Recipient::Device(&d)).err(), Some(SealError::Commitment));
}

// An all-zero X25519 output fails the wrap (a small-order recipient key)
// and the unwrap (a small-order enc).
// spec: SEAL-GEN-013
#[test]
fn an_all_zero_dh_output_fails() {
    let root = EpochRoot::from_bytes([1; 32]);
    let c = root.commitment(&BOARD, 1);
    let mut one = [0u8; 32];
    one[0] = 1;
    for pk in [[0u8; 32], one] {
        assert_eq!(
            wrap_root(&root, &BOARD, 1, &c, DevId([0; 16]), &pk, &mut crate::OsEntropy).err(),
            Some(SealError::DhZero)
        );
    }
    let d = device([0x11; 16], [0x22; 32], [0x33; 32]);
    let p = d.public();
    let mut w = wrap_root(&root, &BOARD, 1, &c, p.dev, &p.hpke_pk, &mut crate::OsEntropy).unwrap();
    for enc in [[0u8; 32], one] {
        w[16..48].copy_from_slice(&enc);
        assert_eq!(unwrap_root(&w, &BOARD, 1, &c, &Recipient::Device(&d)).err(), Some(SealError::Commitment));
    }
}

// The comparisons of secret-derived values in this module are `ct_eq`.
// spec: SEAL-GEN-007
#[test]
fn secret_comparisons_are_constant_time() {
    let src = include_str!("../keys.rs");
    // A method ends at the first line `    }`, a free function at `}`.
    let body = |name: &str, end: &str| {
        let start = src.find(&format!("pub fn {name}(")).unwrap();
        let rest = &src[start..];
        &rest[..rest.find(end).unwrap()]
    };
    for (f, end) in [("from_sheet", "\n    }\n"), ("unwrap_root", "\n}\n")] {
        let b = body(f, end);
        assert!(b.contains(".ct_eq("), "{f}");
        if f == "unwrap_root" {
            assert_eq!(b.matches(".ct_eq(").count(), 2, "the recipient id and the commitment");
        }
        assert!(!b.contains(" == want") && !b.contains("== commit") && !b.contains("== own_id"), "{f}");
    }
}

#[cfg(feature = "test-entropy")]
mod fixed {
    use super::*;
    use crate::FixedEntropy;

    // spec: SEAL-KEY-001
    // spec: SEAL-GEN-004
    #[test]
    fn a_seed_is_exactly_32_drawn_bytes() {
        let bytes: Vec<u8> = (100..132).collect();
        let s = RecoverySeed::generate(&mut FixedEntropy::new(bytes.clone())).unwrap();
        assert_eq!(s.seed.to_vec(), bytes);
        assert_eq!(
            RecoverySeed::generate(&mut FixedEntropy::new(bytes[..31].to_vec())).err(),
            Some(SealError::Entropy)
        );
    }

    // `dev`, then the signing key, then the X25519 ikm, from fresh draws.
    // spec: SEAL-KEY-008
    // spec: SEAL-GEN-004
    #[test]
    fn device_keys_draw_dev_sign_then_ikm() {
        let bytes: Vec<u8> = (0..80).collect();
        let d = DeviceKeys::generate(&mut FixedEntropy::new(bytes.clone())).unwrap();
        let p = d.public();
        assert_eq!(p.dev.0[..], bytes[..16]);
        let sk: [u8; 32] = bytes[16..48].try_into().unwrap();
        assert_eq!(p.sign_pk, ed25519_dalek::SigningKey::from_bytes(&sk).verifying_key().to_bytes());
        assert_eq!(p.hpke_pk, hpke_pk(&bytes[48..80]));
        for n in [0, 15, 16, 47, 48, 79] {
            assert_eq!(
                DeviceKeys::generate(&mut FixedEntropy::new(bytes[..n].to_vec())).err(),
                Some(SealError::Entropy),
                "{n}"
            );
        }
    }

    // spec: SEAL-KEY-010
    // spec: SEAL-GEN-004
    #[test]
    fn a_root_is_exactly_32_drawn_bytes() {
        let r = EpochRoot::generate(&mut FixedEntropy::new(vec![9; 32])).unwrap();
        assert_eq!(*r.to_bytes(), [9; 32]);
        assert_eq!(EpochRoot::generate(&mut FixedEntropy::new(vec![9; 31])).err(), Some(SealError::Entropy));
    }

    // wrap_root draws 32 bytes ikm, enc is DeriveKeyPair(ikm)'s public key,
    // and nothing else is drawn.
    // spec: SEAL-WRAP-004
    // spec: SEAL-GEN-004
    #[test]
    fn wrap_draws_exactly_32_bytes() {
        let d = device([0x11; 16], [0x22; 32], [0x33; 32]);
        let p = d.public();
        let root = EpochRoot::from_bytes([0x44; 32]);
        let c = root.commitment(&BOARD, 1);
        let bytes: Vec<u8> = (0..64).collect();
        let mut rng = FixedEntropy::new(bytes.clone());
        let w = wrap_root(&root, &BOARD, 1, &c, p.dev, &p.hpke_pk, &mut rng).unwrap();
        assert_eq!(w[16..48], hpke_pk(&bytes[..32]));
        let mut rest = [0u8; 32];
        rng.fill(&mut rest).unwrap();
        assert_eq!(rest[..], bytes[32..]);
        let again = wrap_root(&root, &BOARD, 1, &c, p.dev, &p.hpke_pk, &mut FixedEntropy::new(bytes.clone())).unwrap();
        assert_eq!(w, again);
        assert_eq!(
            wrap_root(&root, &BOARD, 1, &c, p.dev, &p.hpke_pk, &mut FixedEntropy::new(bytes[..31].to_vec())).err(),
            Some(SealError::Entropy)
        );
    }
}
