//! Snapshots and blobs (seal.md §20): flipping any byte of the sealed bytes
//! fails at the hash; with the hash recomputed to match, at the header or at
//! the AEAD.

use crate::blob::*;
use crate::board::*;
use taskdeck_seal::*;

/// Every byte of `sealed`, flipped two ways: `open` fails with
/// `seal/blob_hash` under the original reference, and with
/// `seal/blob_header` (bytes 0 to 10: magic, ver, suite, kind, epoch) or
/// `seal/blob_open` (the id, the nonce, the ciphertext) under a reference
/// whose hash is recomputed.
type Open<'a> = dyn Fn(&[u8], &BlobRef) -> Result<(), SealError> + 'a;

fn flip_all(sealed: &[u8], r: &BlobRef, open: &Open<'_>) {
    assert!(open(sealed, r).is_ok());
    for i in 0..sealed.len() {
        for mask in [0x01u8, 0x80] {
            let mut f = sealed.to_vec();
            f[i] ^= mask;
            assert_eq!(open(&f, r), Err(SealError::BlobHash), "byte {i} with {mask:#04x}");
            let rehashed = BlobRef { sha256: crate::backup::sha(&[&f]), ..blob_ref(sealed, r.inflated, r.epoch) };
            let want = if i < 11 { SealError::BlobHeader } else { SealError::BlobOpen };
            assert_eq!(open(&f, &rehashed), Err(want), "byte {i} with {mask:#04x}, hash recomputed");
        }
    }
}

// Every byte of a sealed snapshot and of a sealed blob of kind 2, flipped
// two ways, is rejected: at the hash, before any decryption; and with the
// hash recomputed, at the header or the AEAD. The declared mode rejects
// each flip at the hash as well.
// spec: SEAL-BLOB-002
// spec: SEAL-BLOB-001
// spec: SEAL-BLOB-008
#[test]
fn flipping_any_byte_of_a_blob() {
    let mut s = Sim::new(75, 2);
    s.rotate(Who::D(1));
    let w = s.devs[1].public().dev;
    let body = snap_body(&s);
    let inflated = body_bytes(&body).len() as u64;
    let k = snap_key(&s);
    let sb = seal_snapshot(&body, &k, &s.board, 2, &w, &mut OsEntropy).unwrap();
    let r = blob_ref(&sb.bytes, inflated, 2);
    let seq = s.seq();
    flip_all(&sb.bytes, &r, &|b, r| open_snapshot(&split_parts(b), r, seq, &w, &k, &s.board).map(|_| ()));
    let root = EpochRoot::from_bytes(s.roots[&2]);
    let roots = |e: u32| if e == 2 { Some(&root) } else { None };
    for i in 0..sb.bytes.len() {
        let mut f = sb.bytes.clone();
        f[i] ^= 0x40;
        let e = open_snapshot_declared(&[&f], &sb.sha256, r.size, seq, &w, &roots, &s.board).err();
        assert_eq!(e, Some(SealError::BlobHash), "byte {i}");
    }
    let kb = blob_key(&s);
    let jpeg: Vec<u8> = (0..3000u32).map(|i| (i * 7) as u8).collect();
    let pb = seal_blob(&jpeg, &kb, &s.board, 2, &w, &mut OsEntropy).unwrap();
    let pr = blob_ref(&pb.bytes, 0, 2);
    flip_all(&pb.bytes, &pr, &|b, r| open_blob(&split_parts(b), r, &w, &kb, &s.board).map(|_| ()));
}
