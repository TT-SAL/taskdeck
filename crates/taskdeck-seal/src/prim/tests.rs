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

/// Deterministic bytes that do not compress (SHA-256 of a counter).
fn noise(n: usize, seed: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(n + 32);
    let mut i = seed << 32;
    while out.len() < n {
        out.extend_from_slice(&h(&[&i.to_be_bytes()]));
        i += 1;
    }
    out.truncate(n);
    out
}

/// Compressible bytes.
fn text(n: usize) -> Vec<u8> {
    b"taskdeck history record ".iter().copied().cycle().take(n).collect()
}

#[track_caller]
fn one_stream(head: &[u8], secret: &[u8], tail: &[u8]) -> Vec<u8> {
    let c = deflate_split(head, secret, tail).unwrap();
    let want = [head, secret, tail].concat();
    assert_eq!(inflate_raw(&c, want.len()).unwrap().as_slice(), &want[..]);
    let mut got = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::DeflateDecoder::new(&c[..]), &mut got).unwrap();
    assert_eq!(got, want);
    // The secret stands in stored blocks, as it is.
    assert!(secret.len() < 4 || c.windows(secret.len().min(65_535)).any(|w| w == &secret[..secret.len().min(65_535)]));
    c.to_vec()
}

/// The length of the sync-flushed compression of `head` alone.
fn flushed_len(head: &[u8]) -> usize {
    let mut out = Zeroizing::new(Vec::new());
    compress_piece(&mut out, head, false).unwrap();
    out.len()
}

// `deflate_split` writes one raw DEFLATE stream that inflates to
// `head ‖ secret ‖ tail` (§21.3): for heads, secrets and tails of many sizes,
// compressible and not; with an empty secret and an empty head; with a head
// whose flushed output ends exactly at the end of the 64 KiB working buffer
// (once and twice over); and with the largest secret a file can carry
// (65,535 roots and a 512-byte token, 2,359,780 bytes in 37 stored blocks).
// spec: SEAL-BKP-001
// spec: SEAL-BKP-002
#[test]
fn deflate_split_is_one_stream() {
    let tail = text(11);
    for n in [0usize, 1, 13, 100, 4096, 65_535, 65_536, 65_537, 200_000] {
        for secret_len in [0usize, 4, 36 + 4, 65_535, 65_536, 131_071] {
            one_stream(&text(n), &noise(secret_len, 1), &tail);
            one_stream(&noise(n, 2), &noise(secret_len, 3), &noise(n / 3, 4));
        }
    }
    // A flush that ends exactly at the end of the working buffer: a head of
    // noise one byte longer than one whose flushed output is 65,535 (then
    // 131,071) bytes fills the buffer exactly with its sync marker, so the
    // loop calls once more and that call writes one more empty, non-final
    // block ending in another sync marker.
    for target in [CHUNK, 2 * CHUNK] {
        let n = (target - 60..target)
            .find(|&n| flushed_len(&noise(n - 1, 1)) == target - 1)
            .expect("a head whose flush fills the buffer exactly");
        let head = noise(n, 1);
        let len = flushed_len(&head);
        assert!(len > target + 1);
        let c = one_stream(&head, &noise(40, 5), &tail);
        assert_eq!(&c[target - 4..target], &[0, 0, 0xFF, 0xFF]);
        assert_eq!(&c[len - 4..len], &[0, 0, 0xFF, 0xFF]);
    }
    // The largest secret.
    let big = noise(2 + 65_535 * 36 + 2 + 512, 6);
    let c = one_stream(&text(1000), &big, &tail);
    assert!(c.len() > big.len() + 36 * 5);
}

// The inflater gives the same answers on every path after the reset added
// for SEAL-GEN-006: a good stream, a bad one, one over the cap, one with a
// trailing byte, one cut short.
#[test]
fn inflate_raw_paths() {
    let d = deflate_raw(&text(100_000)).unwrap();
    assert_eq!(inflate_raw(&d, 100_000).unwrap().len(), 100_000);
    assert!(inflate_raw(&d, 99_999).is_none());
    assert!(inflate_raw(&[&d[..], &[0]].concat(), 100_000).is_none());
    assert!(inflate_raw(&d[..d.len() - 1], 100_000).is_none());
    assert!(inflate_raw(&[0xFF; 10], 100).is_none());
    assert!(inflate_raw(&[], 100).is_none());
}


