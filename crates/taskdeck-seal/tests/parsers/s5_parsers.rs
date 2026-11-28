//! The openers of task S5 over random and mutated parts: `open_snapshot`,
//! `open_snapshot_declared` and `open_blob`, on mutated sealed bytes (with
//! the hash recomputed, so the later steps see them), on random parts, and
//! on bytes sealed here around mutated compressed streams and mutated
//! snapshot bodies, so that the inflate and the body parser see them too.
//! Each returns, never panics.

use std::sync::OnceLock;

use proptest::prelude::*;
use taskdeck_seal::*;

use crate::backup::{deflate, sha};
use crate::blob::*;
use crate::board::*;

struct Fixture {
    sim: Sim,
    writer: DevId,
    snapshot: Vec<u8>,
    blob: Vec<u8>,
    body: Vec<u8>,
    compressed: Vec<u8>,
    /// The body's state hash, which `open_snapshot` expects (version 1).
    hash: [u8; 32],
}

fn fixture() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        let mut s = Sim::new(52, 2);
        s.rotate(Who::D(1));
        let writer = s.devs[0].public().dev;
        let b = snap_body(&s);
        let snapshot = seal_snapshot(&b, &snap_key(&s), &s.board, 2, &writer, &mut OsEntropy).unwrap().bytes;
        let blob = seal_blob(&[7u8; 900], &blob_key(&s), &s.board, 2, &writer, &mut OsEntropy).unwrap().bytes;
        let body = body_bytes(&b);
        let compressed = deflate(&body);
        Fixture { sim: s, writer, snapshot, blob, body, compressed, hash: b.state_hash }
    })
}

fn mutate(mut b: Vec<u8>, at: usize, op: u8, x: u8) -> Vec<u8> {
    if b.is_empty() {
        return vec![x];
    }
    let i = at % b.len();
    match op % 5 {
        0 => b[i] = x,
        1 => b.insert(i, x),
        2 => {
            b.remove(i);
        }
        3 => b.truncate(i),
        _ => {
            let end = (i + 4).min(b.len());
            for y in &mut b[i..end] {
                *y = 0xFF;
            }
        }
    }
    b
}

/// Runs the three openers on `bytes` (split into parts) with a reference
/// whose hash matches, and with the given part split.
fn open_all(fx: &Fixture, bytes: &[u8], inflated: u64, split: usize) {
    let s = &fx.sim;
    let root = EpochRoot::from_bytes(s.roots[&2]);
    let roots = |e: u32| if e == 2 { Some(&root) } else { None };
    let r = BlobRef { sha256: sha(&[bytes]), ..blob_ref(bytes, inflated, 2) };
    let at = split.min(bytes.len());
    for parts in [split_parts(bytes), vec![&bytes[..at], &bytes[at..]]] {
        let _ = open_snapshot(&parts, &r, s.seq(), &fx.hash, 1, &fx.writer, &snap_key(s), &s.board);
        let _ = open_snapshot_declared(&parts, &r.sha256, r.size, s.seq(), &fx.writer, &roots, &s.board);
        let _ = open_blob(&parts, &r, &fx.writer, &blob_key(s), &s.board);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    // Random parts and mutated sealed snapshots and blobs, the hash
    // recomputed: the openers return; a mutated snapshot never opens.
    // spec: SEAL-GEN-011
    // spec: SEAL-GEN-008
    #[test]
    fn openers_never_panic_on_sealed_bytes(
        random in proptest::collection::vec(any::<u8>(), 0..800),
        which in any::<bool>(), at in any::<usize>(), op in any::<u8>(), x in any::<u8>(), split in any::<usize>(),
        inflated in any::<u64>(),
    ) {
        let fx = fixture();
        let sealed = if which { &fx.snapshot } else { &fx.blob };
        let m = mutate(sealed.clone(), at, op, x);
        let with_header = [&fx.snapshot[..51], &random[..]].concat();
        for input in [&random, &m, &with_header] {
            open_all(fx, input, inflated % (1 << 27), split);
        }
        if m != fx.snapshot {
            let s = &fx.sim;
            let r = BlobRef { sha256: sha(&[&m]), ..blob_ref(&m, fx.body.len() as u64, 2) };
            prop_assert!(open_snapshot(&split_parts(&m), &r, s.seq(), &fx.hash, 1, &fx.writer, &snap_key(s), &s.board).is_err());
        }
    }

    // Bytes sealed here around mutated compressed streams and around the
    // deflate of mutated snapshot bodies: the inflate and every parser after
    // the AEAD return.
    // spec: SEAL-GEN-011
    // spec: SEAL-GEN-008
    #[test]
    fn openers_never_panic_on_bodies(
        at in any::<usize>(), op in any::<u8>(), x in any::<u8>(), at2 in any::<usize>(), op2 in any::<u8>(), y in any::<u8>(),
        stream in any::<bool>(), slack in 0u64..3,
    ) {
        let fx = fixture();
        let s = &fx.sim;
        let compressed = if stream {
            mutate(fx.compressed.clone(), at, op, x)
        } else {
            deflate(&mutate(mutate(fx.body.clone(), at, op, x), at2, op2, y))
        };
        let raw = Raw::new(1, 2, s.board, s.seq(), fx.writer, [3; 16], [4; 24]);
        let sealed = raw.seal(&kind_key(&s.board, &s.roots[&2], 2, "snap"), &pad(&compressed));
        open_all(fx, &sealed, (fx.body.len() as u64).saturating_sub(1) + slack, 1 << 20);
    }
}
