//! The contexts of task S5 (SEAL-CTX-003, V1-12): a blob does not open as an
//! entry, a file record or the other blob kind, nor those as a blob; a
//! snapshot sealed for one `S`, writer, board or epoch does not open for
//! another.

use taskdeck_seal::*;

use crate::backup::sha;
use crate::blob::*;
use crate::board::*;

fn fixture() -> (Sim, SnapshotBody) {
    let mut s = Sim::new(95, 2);
    s.rotate(Who::D(0));
    let b = snap_body(&s);
    (s, b)
}

// A snapshot's AEAD message does not open under the AD of an entry or of a
// file record built from its own bytes with the same key, nor under the AD
// of a blob of kind 2; a frame's body and a file record do not open under
// the `blob` AD with their keys; and each container rejects the other's
// bytes.
// spec: SEAL-CTX-003
#[test]
fn a_blob_is_not_an_entry_nor_a_file_record() {
    let (mut s, body) = fixture();
    let w = s.devs[0].public().dev;
    let k = snap_key(&s);
    let sb = seal_snapshot(&body, &k, &s.board, 2, &w, &mut OsEntropy).unwrap();
    let b = &sb.bytes;
    let ks = kind_key(&s.board, &s.roots[&2], 2, "snap");
    let blob_ad = [b"taskdeck/v1/blob\0".as_slice(), &s.board.0, &b[..27], &s.seq().to_be_bytes(), &w.0].concat();
    assert!(open_raw(b, &ks, &blob_ad).is_some());
    // Under the AD of an entry (§9.2) and of a file record (§19.3), from the
    // same header bytes.
    let entry_ad = [ctx::ENTRY, &b[..48], &[1, 1], &2u32.to_be_bytes()].concat();
    let file_ad = [ctx::FILE, &b[..55], &0u64.to_be_bytes()].concat();
    for ad in [&entry_ad, &file_ad] {
        assert!(open_raw(b, &ks, ad).is_none());
    }
    // The same bytes with `entry` or `file` in the AD's use do not open.
    for use_ in [ctx::ENTRY, ctx::FILE, ctx::PAIR_BUNDLE] {
        let ad = [use_, &s.board.0, &b[..27], &s.seq().to_be_bytes(), &w.0].concat();
        assert!(open_raw(b, &ks, &ad).is_none());
    }
    // A blob sealed here with the `entry` use in the AD's place is refused.
    let mut raw = Raw::new(2, 2, s.board, 0, w, [1; 16], [2; 24]);
    raw.ad_ctx = b"taskdeck/v1/entry\0";
    let kb = kind_key(&s.board, &s.roots[&2], 2, "blob");
    let f = raw.seal(&kb, &pad(b"jpeg"));
    let bk = blob_key(&s);
    assert_eq!(open_blob(&[&f], &blob_ref(&f, 0, 2), &w, &bk, &s.board).err(), Some(SealError::BlobOpen));
    raw.ad_ctx = b"taskdeck/v1/blob\0";
    let f = raw.seal(&kb, &pad(b"jpeg"));
    assert_eq!(open_blob(&[&f], &blob_ref(&f, 0, 2), &w, &bk, &s.board).unwrap(), b"jpeg");
    // A frame is not a blob, and a blob is not a frame.
    let frame = s.cmd(Who::D(0), &Spec::default());
    let ke = kind_key(&s.board, &s.roots[&2], 2, "entry");
    let n = frame.len();
    let frame_entry_ad = [ctx::ENTRY, &frame[..48], &frame[48..54]].concat();
    let nonce: [u8; 24] = frame[54..78].try_into().unwrap();
    let frame_body = [&frame[..27], &nonce[..], &frame[78..n - 64]].concat();
    assert!(open_raw(&frame_body, &ke, &frame_entry_ad).is_some());
    let as_blob = [b"taskdeck/v1/blob\0".as_slice(), &s.board.0, &frame[..27], &0u64.to_be_bytes(), &w.0].concat();
    assert!(open_raw(&frame_body, &ke, &as_blob).is_none());
    assert_eq!(open_blob(&split_parts(&frame), &blob_ref(&frame, 0, 2), &w, &bk, &s.board).err(), Some(SealError::BlobHeader));
    let r = s.classify(b, &Opts::default()).unwrap();
    assert_eq!((class_name(&r.class), r.code), ("Skip", Some("seal/frame_format")));
    // A file record is not a blob.
    let key = StorageKey::from_bytes([0x6C; 32]);
    let (mut fs, header) = FileSealer::create(&key, FileKind::Log, &s.board, &w, &mut OsEntropy).unwrap();
    let rec = fs.seal_record(b"a record of a log", &mut OsEntropy).unwrap();
    let file = [&header[..], &rec[..]].concat();
    assert_eq!(open_blob(&[&file], &blob_ref(&file, 0, 2), &w, &bk, &s.board).err(), Some(SealError::BlobHeader));
    let o = open_file(&key, FileKind::Log, &s.board, &w, &b[..]);
    assert!(o.is_err() || o.unwrap().records.is_empty());
}

// The two blob kinds: a snapshot is refused as a blob of kind 2 and the
// reverse (at the header); with the kind byte rewritten and the hash
// recomputed, the AEAD fails, the kind being in the AD, even under the key
// it was sealed with.
// spec: SEAL-CTX-003
// spec: SEAL-KEY-012
#[test]
fn a_snapshot_is_not_a_blob_of_kind_2() {
    let (s, body) = fixture();
    let w = s.devs[0].public().dev;
    let k = snap_key(&s);
    let sb = seal_snapshot(&body, &k, &s.board, 2, &w, &mut OsEntropy).unwrap();
    let inflated = body_bytes(&body).len() as u64;
    assert_eq!(open_blob(&[&sb.bytes], &blob_ref(&sb.bytes, 0, 2), &w, &k, &s.board).err(), Some(SealError::BlobHeader));
    let mut f = sb.bytes.clone();
    f[6] = 2;
    assert_eq!(open_blob(&[&f], &blob_ref(&f, 0, 2), &w, &k, &s.board).err(), Some(SealError::BlobOpen));
    let kb = blob_key(&s);
    let pb = seal_blob(b"jpeg", &kb, &s.board, 2, &w, &mut OsEntropy).unwrap();
    assert_eq!(open_snapshot(&[&pb.bytes], &blob_ref(&pb.bytes, 4, 2), 0, &w, &kb, &s.board).err(), Some(SealError::BlobHeader));
    let mut g = pb.bytes.clone();
    g[6] = 1;
    assert_eq!(open_snapshot(&[&g], &blob_ref(&g, 4, 2), 0, &w, &kb, &s.board).err(), Some(SealError::BlobOpen));
    assert!(open_snapshot(&[&sb.bytes], &blob_ref(&sb.bytes, inflated, 2), s.seq(), &w, &k, &s.board).is_ok());
}

// A snapshot sealed for one `S`, writer, board or epoch does not open for
// another (V1-12): each of the four, alone, makes the AEAD fail (for the
// epoch, the header rewritten and the hash recomputed, under the same key);
// the declared mode likewise for `S`, writer and board.
// spec: SEAL-CTX-003
// spec: SEAL-BLOB-001
#[test]
fn a_snapshot_is_bound_to_s_writer_board_and_epoch() {
    let (s, body) = fixture();
    let w = s.devs[0].public().dev;
    let other_w = s.devs[1].public().dev;
    let k = snap_key(&s);
    let sb = seal_snapshot(&body, &k, &s.board, 2, &w, &mut OsEntropy).unwrap();
    let r = blob_ref(&sb.bytes, body_bytes(&body).len() as u64, 2);
    let seq = s.seq();
    let parts = split_parts(&sb.bytes);
    assert!(open_snapshot(&parts, &r, seq, &w, &k, &s.board).is_ok());
    let open = SealError::BlobOpen;
    assert_eq!(open_snapshot(&parts, &r, seq + 64, &w, &k, &s.board).err(), Some(open));
    assert_eq!(open_snapshot(&parts, &r, seq, &other_w, &k, &s.board).err(), Some(open));
    assert_eq!(open_snapshot(&parts, &r, seq, &w, &k, &BoardId([0x0B; 16])).err(), Some(open));
    let mut f = sb.bytes.clone();
    f[7..11].copy_from_slice(&1u32.to_be_bytes());
    let r1 = BlobRef { sha256: sha(&[&f]), epoch: 1, ..blob_ref(&f, r.inflated, 1) };
    assert_eq!(open_snapshot(&[&f], &r1, seq, &w, &k, &s.board).err(), Some(open));
    // Declared mode: the root of the header's epoch, the same bindings.
    let root = EpochRoot::from_bytes(s.roots[&2]);
    let roots = |e: u32| if e == 2 { Some(&root) } else { None };
    let d = |s_: u64, w_: &DevId, b_: &BoardId| open_snapshot_declared(&parts, &sb.sha256, r.size, s_, w_, &roots, b_).err();
    assert_eq!(d(seq, &w, &s.board), None);
    assert_eq!(d(seq + 64, &w, &s.board), Some(open));
    assert_eq!(d(seq, &other_w, &s.board), Some(open));
    assert_eq!(d(seq, &w, &BoardId([0x0B; 16])), Some(open));
}
