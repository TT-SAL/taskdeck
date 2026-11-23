//! The pairs the earlier modules left (task S7): each file kind against
//! every other (the `file` AD carries the kind, and `K_file(kind)` is its own
//! key); the slot MAC of pairing against the other keys derived from the
//! same secret and the other HMAC contexts of §4.1, and against the other
//! labels and roles; the three epoch keys of §6.5 against each other.

use taskdeck_seal::pair::*;
use taskdeck_seal::*;

use crate::blob::*;
use crate::board::*;
use crate::pairing::*;

/// The twelve file kinds of §19.2, by index 0 to 11 (`FileKind` is not `Copy`).
fn kind(i: usize) -> FileKind {
    let all = [
    FileKind::Log,
    FileKind::Checkpoint,
    FileKind::Base,
    FileKind::Hashes,
    FileKind::Outbox,
    FileKind::Overlay,
    FileKind::Identity,
    FileKind::Notices,
    FileKind::Meta,
    FileKind::Marker,
    FileKind::History,
    FileKind::Device,
    ];
    all.into_iter().nth(i).unwrap()
}

/// A file of `kind` with two records, under `key`, for `board` and `dev`.
fn file(key: &StorageKey, kind: FileKind, board: &BoardId, dev: &DevId) -> Vec<u8> {
    let (mut s, mut f) = FileSealer::create(key, kind, board, dev, &mut OsEntropy).unwrap();
    f.extend(s.seal_record(b"first record", &mut OsEntropy).unwrap());
    f.extend(s.seal_record(b"second record", &mut OsEntropy).unwrap());
    f
}

// For every ordered pair of the twelve file kinds, under one storage key,
// board and device: a file of one kind does not open as the other kind,
// and its records spliced behind a header of the other kind open none
// (the subkey and the AD both differ). Each file opens as its own kind
// (the control).
// spec: SEAL-CTX-003
// spec: SEAL-FILE-001
#[test]
fn a_file_of_one_kind_is_no_other_kind() {
    let key = StorageKey::generate(&mut OsEntropy).unwrap();
    let board = BoardId([0xB7; 16]);
    let dev = DevId([0xD7; 16]);
    let files: Vec<Vec<u8>> = (0..12).map(|i| file(&key, kind(i), &board, &dev)).collect();
    for i in 0..12 {
        let own = open_file(&key, kind(i), &board, &dev, &files[i]).unwrap();
        assert!(matches!(own.tail, FileTail::Clean) && own.records.len() == 2, "{i}");
        for j in 0..12 {
            if i == j {
                continue;
            }
            if let Ok(o) = open_file(&key, kind(j), &board, &dev, &files[i]) {
                assert!(o.records.is_empty(), "kind {} opened as {}", i + 1, j + 1);
            }
            let spliced = [&files[j][..55], &files[i][55..]].concat();
            let o = open_file(&key, kind(j), &board, &dev, &spliced);
            assert!(o.map(|o| o.records.is_empty()).unwrap_or(true), "kind {} records behind a header of {}", i + 1, j + 1);
        }
    }
}

/// N, having taken E's code, handed `slot`: the names of its effects and
/// its abort code.
fn n_takes(r: &Run, slot: Vec<u8>) -> (Vec<&'static str>, Option<SealError>) {
    let mut n = New::new();
    let fx = n.on(r.code_event(), &mut OsEntropy);
    assert_eq!(fx.iter().map(n_name).collect::<Vec<_>>(), ["FetchSlot"]);
    let fx = n.on(NewEvent::Slot { bytes: slot }, &mut OsEntropy);
    (fx.iter().map(n_name).collect(), n_abort(&fx))
}

// Slot A with its body unchanged and its MAC made as §17.4 says but under
// another key derived from the same code (`ticket_key`), or under
// `mac_key` with another HMAC context of §4.1 (`door`, `token`) or a hash
// context of pairing in the place of `pair`: N aborts with `seal/slot_mac`.
// Made as §17.4 says, it is taken (the control). E's slot A handed to N
// again where it waits for slot C is not taken as C (ignored as a repeat,
// task S7b); slot A's bytes under label C fail the MAC; N's own slot B
// handed back to it, and E's own slot A handed back to E: the label or role
// is another slot's, and the machine aborts.
// spec: SEAL-PAIR-002
// spec: SEAL-PAIR-003
// spec: SEAL-CTX-001
#[test]
fn a_slot_mac_is_bound_to_its_key_context_label_and_role() {
    let r = Run::new(Sim::new(97, 2), 0);
    let k = &r.k;
    let b = body(&r.slot_a).to_vec();
    let len = (b.len() as u16).to_be_bytes();
    let with = |key: &[u8], context: &[u8]| {
        let m = mac(key, &[context, &[A, ROLE_E], &k.tid, &len, &b].concat());
        [&[A, ROLE_E][..], &len, &b, &m].concat()
    };
    let good = with(&k.mac_key, &cx("pair"));
    assert_eq!(good, r.slot_a);
    assert_eq!(n_takes(&r, good).0, ["PostSlot", "FetchSlot"]);
    for (key, context) in [
        (&k.ticket_key, cx("pair")),
        (&k.mac_key, cx("door")),
        (&k.mac_key, cx("token")),
        (&k.mac_key, cx("pair-commit")),
        (&k.mac_key, cx("pair-transcript")),
        (&k.mac_key, Vec::new()),
    ] {
        let (names, abort) = n_takes(&r, with(key, &context));
        assert_eq!((names, abort), (vec!["Abort"], Some(SealError::SlotMac)), "{:?}", String::from_utf8_lossy(&context));
    }

    // Labels and roles: slot A where N waits for slot C, N's slot B where it
    // waits for slot C.
    let mut r2 = Run::new(Sim::new(98, 2), 0);
    r2.step_b();
    let a = r2.slot_a.clone();
    let fx = r2.n_on(NewEvent::Slot { bytes: a });
    // A slot A delivered again is ignored (task S7b), never taken as C.
    let names: Vec<_> = fx.iter().map(n_name).collect();
    assert!(names.is_empty() || n_abort(&fx).is_some(), "slot A taken as slot C: {names:?}");
    let mut r2 = Run::new(Sim::new(98, 2), 0);
    r2.step_b().step_c();
    let fx = r2.n_on(NewEvent::Slot { bytes: [&[C, ROLE_E][..], &r2.slot_a[2..]].concat() });
    assert_eq!(n_abort(&fx), Some(SealError::SlotMac), "slot A relabelled C");
    let mut r3 = Run::new(Sim::new(99, 2), 0);
    r3.step_b();
    let b3 = r3.slot_b.clone();
    let fx = r3.n_on(NewEvent::Slot { bytes: b3 });
    assert!(n_abort(&fx).is_some(), "N's own slot B taken as slot C");
    // And E: its own slot A where it waits for slot B.
    let mut r4 = Run::new(Sim::new(100, 2), 0);
    let a4 = r4.slot_a.clone();
    let fx = r4.e_on(ExistingEvent::SlotB { bytes: a4 });
    assert!(e_abort(&fx).is_some(), "E's own slot A taken as slot B");
}

// The three epoch keys of §6.5, derived from one root for one epoch, are
// three uses: a frame sealed under `K_snap` or `K_blob` is refused at the
// AEAD (`classify` derives `K_entry`); a blob sealed under `K_entry` or
// `K_snap` does not open under `K_blob`; a snapshot sealed under `K_blob` or
// `K_entry` does not open under `K_snap`. The right key opens each (the
// control).
// spec: SEAL-KEY-012
// spec: SEAL-CTX-001
#[test]
fn the_epoch_keys_are_three_uses() {
    let mut s = Sim::new(96, 2);
    s.rotate(Who::D(1));
    let e = s.epoch();
    let root = EpochRoot::from_bytes(s.roots[&e]);
    let key = |kind| derive_epoch_key(&root, &s.board, e, kind);
    let w = s.devs[1].public().dev;

    // Frames.
    for (kind, want) in [(KeyKind::Entry, None), (KeyKind::Snap, Some("seal/aead")), (KeyKind::Blob, Some("seal/aead"))] {
        let plain = Plain {
            entry_id: EntryId([0x21; 16]),
            kind: Kind::Cmd,
            at_ms: s.now,
            based_on: s.head(),
            payload: b"{\"op\":\"x\"}".to_vec(),
        };
        let ctr = s.next_ctr(Who::D(0));
        let f = seal_frame(&s.signer(Who::D(0)), &s.board, ctr, e, &key(kind), &plain, &mut OsEntropy).unwrap();
        let r = s.classify(&f, &Opts::default()).unwrap();
        assert_eq!(r.code, want);
    }

    // Blobs of kind 2.
    let jpeg: Vec<u8> = (0..2000u32).map(|i| (i * 13) as u8).collect();
    for (kind, opens) in [(KeyKind::Blob, true), (KeyKind::Entry, false), (KeyKind::Snap, false)] {
        let sb = seal_blob(&jpeg, &key(kind), &s.board, e, &w, &mut OsEntropy).unwrap();
        let r = blob_ref(&sb.bytes, 0, e);
        let o = open_blob(&split_parts(&sb.bytes), &r, &w, &key(KeyKind::Blob), &s.board);
        assert_eq!(o.is_ok(), opens, "{kind:?}");
        if !opens {
            assert_eq!(o.err(), Some(SealError::BlobOpen));
        }
    }

    // Snapshots.
    let body = snap_body(&s);
    let inflated = body_bytes(&body).len() as u64;
    let seq = s.seq();
    for (kind, opens) in [(KeyKind::Snap, true), (KeyKind::Entry, false), (KeyKind::Blob, false)] {
        let sb = seal_snapshot(&body, &key(kind), &s.board, e, &w, &mut OsEntropy).unwrap();
        let r = blob_ref(&sb.bytes, inflated, e);
        let o = open_snapshot(&split_parts(&sb.bytes), &r, seq, &body.state_hash, 1, &w, &key(KeyKind::Snap), &s.board);
        assert_eq!(o.is_ok(), opens, "{kind:?}");
        if !opens {
            assert_eq!(o.err(), Some(SealError::BlobOpen));
        }
    }
}
