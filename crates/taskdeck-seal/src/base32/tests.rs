use super::*;
use crate::SealError;
use proptest::prelude::*;

/// A bit-by-bit reference encoder written from §5.2's text.
fn reference(bytes: &[u8], nbits: usize) -> String {
    let bit = |j: usize| -> u8 {
        if j < nbits { bytes.get(j / 8).map_or(0, |b| (b >> (7 - j % 8)) & 1) } else { 0 }
    };
    (0..nbits.div_ceil(5))
        .map(|g| {
            let v = (0..5).fold(0usize, |acc, k| (acc << 1) | usize::from(bit(5 * g + k)));
            char::from(b"0123456789ABCDEFGHJKMNPQRSTVWXYZ"[v])
        })
        .collect()
}

// spec: SEAL-ENC-001
#[test]
fn encode_known_values() {
    assert_eq!(encode(b""), "");
    assert_eq!(encode(&[0x00]), "00");
    assert_eq!(encode(&[0xFF]), "ZW");
    assert_eq!(encode(&[0xFF; 5]), "ZZZZZZZZ");
    // 0x08 0x86 0x42 0x98 0xE8 = 00001 00010 00011 00100 00101 00110 00111 01000
    assert_eq!(encode(&[0x08, 0x86, 0x42, 0x98, 0xE8]), "12345678");
    assert_eq!(encode(&[0u8; 32]).len(), 52);
    assert_eq!(encode(&[0u8; 10]).len(), 16);
}

proptest! {
    // spec: SEAL-ENC-001
    #[test]
    fn encode_is_uppercase_alphabet_without_separators(bytes in proptest::collection::vec(any::<u8>(), 0..80)) {
        let s = encode(&bytes);
        prop_assert_eq!(s.len(), (8 * bytes.len()).div_ceil(5));
        prop_assert!(s.bytes().all(|c| ALPHABET.contains(&c)));
        prop_assert_eq!(&s, &reference(&bytes, 8 * bytes.len()));
        prop_assert_eq!(&*encode_zeroizing(&bytes), &s);
    }

    // spec: SEAL-ENC-002
    #[test]
    fn decode_reverses_encode(bytes in proptest::collection::vec(any::<u8>(), 0..80)) {
        let s = encode(&bytes);
        prop_assert_eq!(decode_typed(&s, bytes.len()).unwrap(), bytes.clone());
        prop_assert_eq!(decode_typed(&s.to_lowercase(), bytes.len()).unwrap(), bytes);
    }

    #[test]
    fn encode_bits_matches_the_reference(bytes in proptest::collection::vec(any::<u8>(), 0..8), nbits in 0usize..70) {
        prop_assert_eq!(&*encode_bits(&bytes, nbits), &reference(&bytes, nbits));
    }

    #[test]
    fn decode_bits_reverses_encode_bits(bytes in proptest::collection::vec(any::<u8>(), 1..8), nbits in 1usize..64) {
        let nbits = nbits.min(8 * bytes.len());
        let text = encode_bits(&bytes, nbits);
        let values = normalise(&text, nbits.div_ceil(5)).unwrap();
        let got = decode_bits(&values, nbits).unwrap();
        let mut want = bytes[..nbits.div_ceil(8)].to_vec();
        if nbits % 8 != 0 {
            let last = want.len() - 1;
            want[last] &= 0xFFu8 << (8 - nbits % 8);
        }
        prop_assert_eq!(got.to_vec(), want);
    }
}

// Normalising removes spaces, hyphens and tabs, folds case, and maps O to
// 0 and I and L to 1; U and anything else outside the alphabet is refused.
// spec: SEAL-ENC-002
#[test]
fn normalise_in_order() {
    let ok = |t: &str, n: usize| normalise(t, n).map(|v| v.to_vec());
    assert_eq!(ok("0o-Oi\tIl L", 7), Ok(vec![0, 0, 0, 1, 1, 1, 1]));
    assert_eq!(ok("abcdefgh", 8), ok("ABCDEFGH", 8));
    assert_eq!(ok("U0000000", 8), Err(NormError::Char));
    assert_eq!(ok("u0000000", 8), Err(NormError::Char));
    assert_eq!(ok("é0000000", 8), Err(NormError::Char));
    assert_eq!(ok("0000\n0000", 8), Err(NormError::Length));
    assert_eq!(ok("0000\n000", 8), Err(NormError::Char));
    assert_eq!(ok("0000_000", 8), Err(NormError::Char));
    assert_eq!(ok("0000000", 8), Err(NormError::Length));
    assert_eq!(ok("000000000", 8), Err(NormError::Length));
    // Only U+0020, U+002D and U+0009 are removed.
    assert_eq!(ok("0000\u{00A0}0000", 8), Err(NormError::Length));
    // No other look-alike is mapped.
    assert_eq!(ok("S0000000", 8).unwrap()[0], 25);
    assert_eq!(ok("ı0000000", 8), Err(NormError::Char));
}

// spec: SEAL-ENC-002
#[test]
fn decode_typed_rejects_wrong_padding_and_length() {
    // One byte is 2 characters, 8 bits used, 2 padding bits: "ZW" is 0xFF,
    // "ZX" sets the last padding bit, "ZY" the first.
    assert_eq!(decode_typed("ZW", 1), Ok(vec![0xFF]));
    assert_eq!(decode_typed("ZX", 1), Err(SealError::CodeFormat));
    assert_eq!(decode_typed("ZY", 1), Err(SealError::CodeFormat));
    assert_eq!(decode_typed("Z", 1), Err(SealError::CodeFormat));
    assert_eq!(decode_typed("ZW0", 1), Err(SealError::CodeFormat));
    assert_eq!(decode_typed("ZU", 1), Err(SealError::CodeFormat));
    assert_eq!(decode_typed("", 0), Ok(vec![]));
    assert_eq!(decode_typed("0", 0), Err(SealError::CodeFormat));
    assert_eq!(decode_typed("00", usize::MAX), Err(SealError::CodeFormat));
    // Five bytes use every bit of 8 characters: no padding to check.
    assert_eq!(decode_typed("zzzz-zzzz", 5), Ok(vec![0xFF; 5]));
}

#[test]
fn decode_bits_refuses_values_above_31() {
    assert!(decode_bits(&[32, 0], 8).is_none());
    assert!(decode_bits(&[0], 8).is_none());
}
