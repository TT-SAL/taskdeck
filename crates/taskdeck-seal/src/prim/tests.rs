use super::*;

// The hash states that take in the seed, the roots and the private keys are
// wiped on drop: sha2's `zeroize` feature is on (the crate's Cargo.toml), and
// the HMAC and HKDF states are built from these cores.
// spec: SEAL-GEN-006
// spec: SEAL-SHEET-003
#[test]
fn hash_states_zeroize_on_drop() {
    fn zeroize_on_drop<T: zeroize::ZeroizeOnDrop>() {}
    zeroize_on_drop::<sha2::Sha256>();
    zeroize_on_drop::<sha2::Sha512>();
}

#[test]
fn take_never_reads_past_the_end() {
    let b = [1u8, 2, 3, 4];
    assert_eq!(take::<2>(&b, 2), Some(&[3, 4]));
    assert_eq!(take::<2>(&b, 3), None);
    assert_eq!(take::<1>(&b, usize::MAX), None);
    assert_eq!(take::<0>(&b, 4), Some(&[]));
}

// HKDF with an empty salt and with the info given in parts is RFC 5869's
// HKDF over the concatenated info (seal.md §2).
// spec: SEAL-KEY-012
#[test]
fn hkdf_parts_are_concatenated() {
    let a = hkdf32(b"salt", b"ikm", &[b"ab", b"cd"]);
    let b = hkdf32(b"salt", b"ikm", &[b"abcd"]);
    assert_eq!(*a, *b);
    let mut c = [0u8; 32];
    hkdf::Hkdf::<sha2::Sha256>::new(None, b"ikm").expand(b"x", &mut c).unwrap();
    assert_eq!(*hkdf32(&[], b"ikm", &[b"x"]), c);
}
