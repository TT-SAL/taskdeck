//! The parsers of task S1 over random and mutated inputs.

use crate::common::BOARD;
use proptest::prelude::*;
use taskdeck_seal::{
    DeviceKeys, EpochRoot, FrameHash, OsEntropy, Recipient, RecoverySeed, SealError, base32, check_move_code,
    move_code, relay_origin, unwrap_root, wrap_root,
};

/// Text made mostly of characters that matter to the parsers.
fn typed() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            4 => proptest::sample::select("0123456789ABCDEFGHJKMNPQRSTVWXYZabcxyzOoIiLlUu -\t\n.:_".chars().collect::<Vec<_>>()),
            1 => any::<char>(),
        ],
        0..90,
    )
    .prop_map(|v| v.into_iter().collect())
}

/// A string with one character replaced, inserted or removed.
fn mutate(s: &str, at: usize, op: u8, c: char) -> String {
    let mut v: Vec<char> = s.chars().collect();
    let i = if v.is_empty() { 0 } else { at % v.len() };
    match op % 3 {
        0 if !v.is_empty() => v[i] = c,
        1 => v.insert(i.min(v.len()), c),
        _ if !v.is_empty() => {
            v.remove(i);
        }
        _ => {}
    }
    v.into_iter().collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    // spec: SEAL-GEN-011
    // spec: SEAL-ENC-002
    #[test]
    fn decode_typed_never_panics(text in typed(), n in 0usize..64, huge in any::<usize>()) {
        if let Ok(v) = base32::decode_typed(&text, n) {
            prop_assert_eq!(v.len(), n);
            prop_assert_eq!(base32::decode_typed(&base32::encode(&v), n).unwrap(), v);
        }
        let _ = base32::decode_typed(&text, huge);
    }

    // spec: SEAL-GEN-011
    // spec: SEAL-ENC-002
    #[test]
    fn decode_typed_of_mutated_encodings(bytes in proptest::collection::vec(any::<u8>(), 0..40), at in any::<usize>(), op in any::<u8>(), c in any::<char>()) {
        let text = mutate(&base32::encode(&bytes), at, op, c);
        if let Ok(v) = base32::decode_typed(&text, bytes.len()) {
            prop_assert_eq!(v.len(), bytes.len());
        }
    }

    // spec: SEAL-GEN-011
    // spec: SEAL-SHEET-002
    #[test]
    fn from_sheet_never_panics(text in typed()) {
        if let Err(e) = RecoverySeed::from_sheet(&text) {
            prop_assert!(matches!(e, SealError::SheetLength | SealError::SheetChar | SealError::SheetPadding | SealError::SheetCheck));
        }
    }

    // spec: SEAL-GEN-011
    // spec: SEAL-SHEET-002
    #[test]
    fn from_sheet_of_mutated_sheets(at in any::<usize>(), op in any::<u8>(), c in any::<char>()) {
        let seed = RecoverySeed::generate(&mut OsEntropy).unwrap();
        let sheet = seed.to_sheet();
        let text = mutate(&sheet, at, op, c);
        match RecoverySeed::from_sheet(&text) {
            // An accepted mutation is a removed separator, a case change or
            // a look-alike: the same seed.
            Ok(back) => prop_assert_eq!(back.keys().public(), seed.keys().public()),
            Err(e) => prop_assert!(matches!(e, SealError::SheetLength | SealError::SheetChar | SealError::SheetPadding | SealError::SheetCheck)),
        }
    }

    // spec: SEAL-GEN-011
    #[test]
    fn from_identity_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
        match DeviceKeys::from_identity_bytes(&bytes) {
            Ok(k) => prop_assert_eq!(k.to_identity_bytes().to_vec(), bytes),
            Err(e) => prop_assert_eq!(e, SealError::FileCorrupt),
        }
    }

    // spec: SEAL-GEN-011
    #[test]
    fn from_identity_bytes_of_mutated_identities(i in 0usize..80, mask in 1u8..=255, cut in 0usize..81) {
        let ids = DeviceKeys::generate(&mut OsEntropy).unwrap().to_identity_bytes();
        let mut v = ids.to_vec();
        v[i] ^= mask;
        if let Ok(k) = DeviceKeys::from_identity_bytes(&v) {
            prop_assert_eq!(k.to_identity_bytes().to_vec(), v);
        }
        prop_assert_eq!(DeviceKeys::from_identity_bytes(&ids[..cut.min(79)]).err(), Some(SealError::FileCorrupt));
    }

    // spec: SEAL-GEN-011
    // spec: SEAL-ENC-006
    #[test]
    fn relay_origin_never_panics(text in typed()) {
        match relay_origin(&text) {
            Ok(o) => {
                prop_assert_eq!(o, format!("https://{text}"));
                prop_assert!(text.len() <= 255 && text.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-.:".contains(&b)));
            }
            Err(e) => prop_assert_eq!(e, SealError::HostFormat),
        }
    }

    // spec: SEAL-GEN-011
    // spec: SEAL-ENC-006
    #[test]
    fn relay_origin_of_mutated_hosts(at in any::<usize>(), op in any::<u8>(), c in any::<char>()) {
        let text = mutate("relay-1.example.com:8443", at, op, c);
        if let Ok(o) = relay_origin(&text) {
            prop_assert_eq!(o, format!("https://{text}"));
            prop_assert!(!text.ends_with(":443") && !text.contains(".."));
        }
    }

    // spec: SEAL-GEN-011
    // spec: SEAL-FP-004
    #[test]
    fn check_move_code_never_panics(text in typed(), seq in any::<u64>(), at in any::<usize>(), op in any::<u8>(), c in any::<char>()) {
        let fh = FrameHash([0x31; 32]);
        if let Err(e) = check_move_code(&BOARD, seq, &fh, &text) {
            prop_assert_eq!(e, SealError::MoveCode);
        }
        let code = move_code(&BOARD, seq, &fh);
        prop_assert_eq!(check_move_code(&BOARD, seq, &fh, &code), Ok(()));
        let _ = check_move_code(&BOARD, seq, &fh, &mutate(&code, at, op, c));
    }

    // spec: SEAL-GEN-011
    // spec: SEAL-WRAP-002
    #[test]
    fn unwrap_root_never_panics(bytes in proptest::collection::vec(any::<u8>(), 96), epoch in any::<u32>(), commit in any::<[u8; 32]>()) {
        let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
        let w: [u8; 96] = bytes.try_into().unwrap();
        prop_assert_eq!(unwrap_root(&w, &BOARD, epoch, &commit, &Recipient::Device(&d)).err(), Some(SealError::Commitment));
    }

    // spec: SEAL-GEN-011
    // spec: SEAL-WRAP-002
    #[test]
    fn unwrap_root_of_mutated_wraps(i in 0usize..96, mask in 1u8..=255, j in 0usize..96, mask2 in any::<u8>()) {
        let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
        let p = d.public();
        let root = EpochRoot::generate(&mut OsEntropy).unwrap();
        let c = root.commitment(&BOARD, 5);
        let mut w = wrap_root(&root, &BOARD, 5, &c, p.dev, &p.hpke_pk, &mut OsEntropy).unwrap();
        w[i] ^= mask;
        w[j] ^= mask2;
        // The net change is zero only when both flips cancel out.
        if i != j || mask != mask2 {
            prop_assert_eq!(unwrap_root(&w, &BOARD, 5, &c, &Recipient::Device(&d)).err(), Some(SealError::Commitment));
        }
    }
}
