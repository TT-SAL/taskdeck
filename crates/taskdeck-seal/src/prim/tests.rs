use super::*;

// The hash states that take in the seed, the roots and the private keys are
// wiped on drop: sha2's `zeroize` feature is on (the crate's Cargo.toml), and
// the HMAC and HKDF states are built from these cores.
// spec: SEAL-GEN-006
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

/// `inflate_raw(data, cap)`, with the number of resets `Inflater::drop`
/// made meanwhile on this thread.
fn inflate_counting_resets(data: &[u8], cap: usize) -> (Option<Zeroizing<Vec<u8>>>, u64) {
    let before = INFLATER_RESETS.with(|c| c.get());
    let r = inflate_raw(data, cap);
    (r, INFLATER_RESETS.with(|c| c.get()) - before)
}

// The inflater gives the same answers on every path after the reset added
// for SEAL-GEN-006: a good stream, a bad one, one over the cap, one with a
// trailing byte, one cut short; and on each of them it resets its
// decompressor exactly once before dropping it (the reader's half of
// ruling Q48: in the pinned miniz_oxide the reset zeroes the 32 KiB window
// that holds the last bytes inflated).
// spec: SEAL-GEN-006
#[test]
fn inflate_raw_paths() {
    let d = deflate_raw(&text(100_000)).unwrap();
    let (ok, n) = inflate_counting_resets(&d, 100_000);
    assert_eq!(ok.unwrap().len(), 100_000);
    assert_eq!(n, 1, "the success path");
    for (name, data, cap) in [
        ("output over the cap", d.to_vec(), 99_999),
        ("input after the stream's end", [&d[..], &[0]].concat(), 100_000),
        ("a truncated stream", d[..d.len() - 1].to_vec(), 100_000),
        ("a wrong stream", vec![0xFF; 10], 100),
        ("an empty stream", Vec::new(), 100),
    ] {
        let (r, n) = inflate_counting_resets(&data, cap);
        assert!(r.is_none(), "{name}");
        assert_eq!(n, 1, "{name}");
    }
}

/// Two bytes in three that do not compress, one `a`.
fn mixed(n: usize) -> Vec<u8> {
    let mut out = noise(n, 3);
    for b in out.iter_mut().step_by(3) {
        *b = b'a';
    }
    out
}

// A sync flush is written whole (§21.3: the head's output ends on a byte
// boundary after its last block, so the stored blocks that follow are read
// as blocks): at these head sizes the LZ code buffer fills while the last
// bytes are still in miniz_oxide's lookahead, and the first sync-flush call
// returns having taken all the input, with room left in the output and no
// sync marker written (security-1, S5 finding 1). The head's own output
// then ends with the sync marker and inflates to the whole head.
// spec: SEAL-BKP-001
#[test]
fn a_sync_flush_is_complete_when_the_code_buffer_fills() {
    for n in [122_631usize, 122_788, 184_400] {
        let head = mixed(n);
        let mut out = Zeroizing::new(Vec::new());
        compress_piece(&mut out, &head, false).unwrap();
        assert!(out.ends_with(&[0, 0, 0xFF, 0xFF]), "head of {n} bytes: no sync marker at the end");
        let mut d = flate2::Decompress::new(false);
        let mut got = vec![0u8; n + 1];
        d.decompress(&out, &mut got, flate2::FlushDecompress::Sync).unwrap();
        assert_eq!(d.total_in(), out.len() as u64);
        assert_eq!(d.total_out(), n as u64, "head of {n} bytes: the flushed output holds only part of it");
        assert_eq!(&got[..n], &head[..]);
        one_stream(&head, &noise(40, 5), &text(11));
    }
}

// The proof's comparison (`inflates_to`, task S7b): a stream inflates to
// exactly the bytes it was made from, fed whole or cut into two pieces at
// any point; it does not for one byte more or less expected, a byte of the
// expected output changed, input after the stream's end, a stream cut
// short, or bytes that are not DEFLATE. The decompressor is reset on every
// path.
// spec: SEAL-BKP-001
#[test]
fn inflates_to_compares_while_inflating() {
    let data = [text(70_000), noise(70_000, 9)].concat();
    let d = deflate_raw(&data).unwrap();
    assert!(inflates_to(&[&d], &data).is_some());
    for cut in (0..=d.len()).step_by(97).chain([d.len()]) {
        assert!(inflates_to(&[&d[..cut], &d[cut..]], &data).is_some(), "cut at {cut}");
    }
    assert!(inflates_to(&[&d, &[]], &data).is_some());
    let mut other = data.clone();
    other[100_000] ^= 1;
    let before = INFLATER_RESETS.with(|c| c.get());
    let more = [&data[..], &[0u8][..]].concat();
    type Case<'a> = (&'a str, Vec<&'a [u8]>, &'a [u8]);
    let bad: [Case<'_>; 7] = [
        ("one byte more expected", vec![&d[..]], &more),
        ("one byte less expected", vec![&d[..]], &data[..data.len() - 1]),
        ("a byte of the output changed", vec![&d[..]], &other),
        ("input after the end", vec![&d[..], &[0u8][..]], &data),
        ("a stream cut short", vec![&d[..d.len() - 1]], &data),
        ("not DEFLATE", vec![&[0xFFu8; 10][..]], &data),
        ("nothing", vec![], &data),
    ];
    for (name, pieces, want) in &bad {
        assert!(inflates_to(pieces, want).is_none(), "{name}");
    }
    assert_eq!(INFLATER_RESETS.with(|c| c.get()) - before, bad.len() as u64);
    // The head's proof: a sync-flushed head and an empty final stored block.
    let head = mixed(150_000);
    let mut out = Zeroizing::new(Vec::new());
    compress_piece(&mut out, &head, false).unwrap();
    assert!(inflates_to(&[&out, &EMPTY_FINAL_STORED], &head).is_some());
    assert!(inflates_to(&[&out[..out.len() - 1], &EMPTY_FINAL_STORED], &head).is_none());
    assert!(inflates_to(&[&out], &head).is_none(), "no final block: the stream does not end");
}

// `deflate_split` refuses a stream that fails its proof: a byte of the
// compressed head or of the compressed tail changed after the compressor
// wrote it gives `None`; a byte of a stored block's data (the secret, which
// the proof does not read) changed does not.
// spec: SEAL-BKP-001
#[test]
fn deflate_split_proves_its_stream() {
    let (head, secret, tail) = (mixed(122_631), noise(100, 1), text(500));
    let faults: [(SplitFault, bool); 5] = [
        (|s, _, _| s[0] ^= 0x04, false),
        (|s, h, _| s[h - 1] ^= 0x01, false),
        (|s, _, t| s[t] ^= 0x04, false),
        (
            |s, _, _| {
                let n = s.len();
                s[n - 1] ^= 0x01;
            },
            false,
        ),
        (|s, h, _| s[h + 5] ^= 0x01, true),
    ];
    for (i, (f, passes)) in faults.into_iter().enumerate() {
        SPLIT_FAULT.with(|c| c.set(Some(f)));
        let r = deflate_split(&head, &secret, &tail);
        assert_eq!(r.is_some(), passes, "fault {i}");
    }
    assert!(SPLIT_FAULT.with(|c| c.get()).is_none());
    assert!(deflate_split(&head, &secret, &tail).is_some());
}

/// Bytes of the kind `k`: 0 noise, 1 text, 2 two in three noise.
fn run_of(k: u8, n: usize, seed: u64) -> Vec<u8> {
    match k {
        0 => noise(n, seed),
        1 => text(n),
        _ => {
            let mut v = noise(n, seed);
            for b in v.iter_mut().step_by(3) {
                *b = b'a';
            }
            v
        }
    }
}

fn runs(rs: &[(u8, usize, u64)]) -> Vec<u8> {
    rs.iter().flat_map(|&(k, n, seed)| run_of(k, n, seed)).collect()
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(48))]

    // `deflate_split` read back by `inflate_raw`: heads of up to 480 KB made
    // of runs of compressible and incompressible bytes in random
    // proportions, secrets of up to 140,000 bytes, tails of runs too. Every
    // stream passes its own proof and inflates to `head ‖ secret ‖ tail`.
    // spec: SEAL-BKP-001
    #[test]
    fn deflate_split_reads_back(
        head in proptest::collection::vec((0u8..3, 1usize..60_000, proptest::prelude::any::<u64>()), 0..8),
        secret_len in 0usize..140_000,
        tail in proptest::collection::vec((0u8..3, 1usize..20_000, proptest::prelude::any::<u64>()), 0..3),
    ) {
        let (head, tail) = (runs(&head), runs(&tail));
        let secret = noise(secret_len, 77);
        let c = deflate_split(&head, &secret, &tail);
        proptest::prop_assert!(c.is_some());
        let c = c.unwrap();
        let want = [&head[..], &secret[..], &tail[..]].concat();
        let got = inflate_raw(&c, want.len());
        proptest::prop_assert!(got.as_deref().is_some_and(|g| g == &want[..]));
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

    // `inflate_raw` on random bytes and on real streams mutated (a byte
    // changed, the stream cut or extended), with any cap: it returns, never
    // panics, and what it returns is never longer than the cap.
    // spec: SEAL-GEN-011
    // spec: SEAL-BLOB-003
    #[test]
    fn inflate_never_panics(
        random in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..600),
        kind in 0u8..3, n in 0usize..20_000, seed in proptest::prelude::any::<u64>(),
        at in proptest::prelude::any::<usize>(), x in proptest::prelude::any::<u8>(), op in 0u8..4,
        cap in 0usize..40_000,
    ) {
        let data = run_of(kind, n, seed);
        let mut c = deflate_raw(&data).unwrap().to_vec();
        let i = at % (c.len() + 1);
        match op {
            0 => if let Some(b) = c.get_mut(i) { *b ^= x | 1 },
            1 => c.truncate(i),
            2 => c.extend(std::iter::repeat_n(x, i % 64 + 1)),
            _ => c.insert(i, x),
        }
        for input in [&random, &c] {
            if let Some(out) = inflate_raw(input, cap) {
                proptest::prop_assert!(out.len() <= cap);
            }
        }
        let whole = deflate_raw(&data).unwrap();
        let back = inflate_raw(&whole, data.len());
        proptest::prop_assert!(back.as_deref().is_some_and(|v| v[..] == data[..]));
    }
}
