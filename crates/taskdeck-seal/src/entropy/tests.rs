use super::*;

// OsEntropy fills from the operating system; two draws of 32 bytes differ.
// spec: SEAL-GEN-004
#[test]
fn os_entropy_fills() {
    let mut a = [0u8; 32];
    let mut b = [0u8; 32];
    OsEntropy.fill(&mut a).unwrap();
    OsEntropy.fill(&mut b).unwrap();
    assert_ne!(a, b);
    OsEntropy.fill(&mut []).unwrap();
}

// FixedEntropy replays its bytes in order and fails with `seal/entropy`
// once they run out, handing out nothing on that call.
// spec: SEAL-GEN-004
#[cfg(feature = "test-entropy")]
#[test]
fn fixed_entropy_replays_then_fails() {
    let mut e = FixedEntropy::new((0u8..10).collect());
    let mut a = [0u8; 4];
    e.fill(&mut a).unwrap();
    assert_eq!(a, [0, 1, 2, 3]);
    let mut b = [0xAAu8; 7];
    assert_eq!(e.fill(&mut b), Err(SealError::Entropy));
    assert_eq!(b, [0xAA; 7]);
    let mut c = [0u8; 6];
    e.fill(&mut c).unwrap();
    assert_eq!(c, [4, 5, 6, 7, 8, 9]);
    e.fill(&mut []).unwrap();
    assert_eq!(e.fill(&mut [0u8; 1]), Err(SealError::Entropy));
}

// spec: SEAL-GEN-014
#[cfg(feature = "test-entropy")]
#[test]
fn the_marker_is_the_spec_string() {
    assert_eq!(TEST_ENTROPY_MARKER, "taskdeck-test-entropy-enabled-3b9e5d");
}
