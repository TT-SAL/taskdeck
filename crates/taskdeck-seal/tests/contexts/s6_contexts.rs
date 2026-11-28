//! The contexts of task S6 (SEAL-CTX-003, the `pair-bundle` use): the
//! bundle's AEAD message does not open under the AD of an entry, a blob or
//! a file record built from its own bytes and key, nor under its own AD with
//! another use; a message made with the bundle key under another use's AD,
//! or a frame, blob or file record, does not open on N as a bundle; and the
//! slot D body is no frame, blob or file to their openers.

use taskdeck_seal::pair::*;
use taskdeck_seal::*;

use crate::blob::*;
use crate::board::*;
use crate::pairing::*;

fn a_bundle() -> Vec<u8> {
    let s = SnapAnnounce {
        seq: 70,
        hash: [1; 32],
        hash_version: 1,
        blob: BlobId([2; 16]),
        sha256: [3; 32],
        size: 900,
        parts: 1,
        inflated: 900,
        epoch: 1,
    };
    bundle_json(&BoardId([0xB0; 16]), HOST, &DOOR, &[(1, [7; 32])], Head { seq: 70, chain: Chain([5; 32]) }, &s, 71, TOKEN)
}

// spec: SEAL-CTX-003
#[test]
fn the_bundle_is_not_an_entry_a_blob_nor_a_file_record() {
    let board = BoardId([0xB0; 16]);
    let pt = a_bundle();
    let w = WithFakeE::new(board, DevId([0xE0; 16])).accept();
    let key = bundle_key(&w.fe.k.s, &w.dh, &w.t);
    let ad = bundle_ad(&board, &w.fe.k.tid, &w.t);
    let d = w.slot_d(&pt);
    let b = body(&d).to_vec();
    assert_eq!(aead_open(&key, &ad, &b).unwrap(), pt);
    // The same bytes under the AD of an entry (§9.2), a blob (§20.2) and a
    // file record (§19.3), and under the bundle's AD with another use.
    let entry_ad = [ctx::ENTRY, &b[..48], &[1, 1], &1u32.to_be_bytes()].concat();
    let blob_ad = [ctx::BLOB, &board.0, &b[..27], &70u64.to_be_bytes(), &[0xE0; 16]].concat();
    let file_ad = [ctx::FILE, &b[..55], &0u64.to_be_bytes()].concat();
    for other in [entry_ad, blob_ad, file_ad] {
        assert!(aead_open(&key, &other, &b).is_none());
    }
    for use_ in [ctx::ENTRY, ctx::BLOB, ctx::FILE] {
        let other = [use_, &board.0, &w.fe.k.tid, &w.t].concat();
        assert!(aead_open(&key, &other, &b).is_none());
    }
    let mut n = w.n;
    assert_eq!(n_name(&n.on(NewEvent::Slot { bytes: d }, &mut OsEntropy)[0]), "FetchSnapshot");

    // Made with the bundle key under another use's AD: N does not open it.
    for use_ in [ctx::ENTRY, ctx::BLOB, ctx::FILE, ctx::PAIR] {
        let w = WithFakeE::new(board, DevId([0xE0; 16])).accept();
        let key = bundle_key(&w.fe.k.s, &w.dh, &w.t);
        let other = [use_, &board.0, &w.fe.k.tid, &w.t].concat();
        let d = w.fe.k.slot(D, ROLE_E, &aead_seal(&key, [4; 24], &other, &pt));
        let mut n = w.n;
        let fx = n.on(NewEvent::Slot { bytes: d }, &mut OsEntropy);
        assert_eq!(n_abort(&fx), Some(SealError::BundleOpen));
    }
}

// A frame, a sealed blob and a sealed file, carried as slot D's body by a
// relay that knows the code, do not open as a bundle; and slot D's body is
// refused by the frame, blob and file openers.
// spec: SEAL-CTX-003
#[test]
fn frames_blobs_and_files_are_not_bundles() {
    let mut s = Sim::new(710, 1);
    let w = s.devs[0].public().dev;
    let frame = s.cmd(Who::D(0), &Spec::default());
    let blob = seal_blob(&[9u8; 300], &blob_key(&s), &s.board, 1, &w, &mut OsEntropy).unwrap().bytes;
    let key = StorageKey::from_bytes([0x6C; 32]);
    let (mut fs, header) = FileSealer::create(&key, FileKind::Log, &s.board, &w, &mut OsEntropy).unwrap();
    let rec = fs.seal_record(&a_bundle(), &mut OsEntropy).unwrap();
    let file = [&header[..], &rec[..]].concat();
    for carried in [&frame[..16_000.min(frame.len())], &blob[..], &file[..], &rec[4..]] {
        let wf = WithFakeE::new(s.board, w).accept();
        let d = wf.fe.k.slot(D, ROLE_E, carried);
        let mut n = wf.n;
        let fx = n.on(NewEvent::Slot { bytes: d }, &mut OsEntropy);
        assert_eq!(n_abort(&fx), Some(SealError::BundleOpen));
    }
    // Slot D's body to the other openers.
    let wf = WithFakeE::new(s.board, w).accept();
    let b = body(&wf.slot_d(&a_bundle())).to_vec();
    let r = s.classify(&b, &Opts::default()).unwrap();
    assert_eq!((class_name(&r.class), r.code), ("Skip", Some("seal/frame_format")));
    let bk = blob_key(&s);
    assert!(open_blob(&[&b], &blob_ref(&b, 0, 1), &w, &bk, &s.board).is_err());
    let o = open_file(&key, FileKind::Log, &s.board, &w, &b);
    assert!(o.is_err() || o.unwrap().records.is_empty());
}
