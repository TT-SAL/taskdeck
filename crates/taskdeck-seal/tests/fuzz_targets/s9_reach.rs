//! Task S9: the fuzz targets `open_blob`, `open_snapshot`,
//! `open_snapshot_declared`, `pair_new` and `pair_bundle` reach the parsers
//! their rows of the parser table (`tests/parsers/main.rs`) name. Each
//! target's body is a function of the fuzz crate's library that returns how
//! far its input got; these tests give it inputs built here and assert that.
//!
//! Where two steps give the same code (`seal/blob_open` for the AEAD, the
//! padding and step 7), they are told apart by construction: the failing
//! input differs from one that opens only in the plaintext the harness
//! sealed under the right key and AD, and the sealed bytes are checked to
//! open under the AEAD with the pinned crates (`Reach::aead_opens`).

use taskdeck_seal::*;

use crate::{
    BLOB_BODY, BUNDLE_PT_BOUND, Reach, blobs, fields, reach_open_blob, reach_open_snapshot, reach_open_snapshot_declared,
    reach_pair_bundle, reach_pair_new,
};

/// One edit of `mutate`: at `pos`, operation `op` (0 set, 1 flip, 2 insert,
/// 3 remove, 4 cut), value `x`.
fn edit(pos: usize, op: u8, x: u8) -> Vec<u8> {
    let p = u16::try_from(pos).unwrap().to_be_bytes();
    vec![p[0], p[1], op, x]
}

fn input(head: &[u8], edits: &[Vec<u8>]) -> Vec<u8> {
    [head.to_vec(), edits.concat()].concat()
}

/// Failed with `code` after the AEAD opened.
fn after_aead(r: &Reach, code: SealError) {
    assert_eq!(r.result, Err(code));
    assert!(r.aead_opens, "the sealed bytes open under the AEAD");
}

fn opens(r: &Reach) {
    assert_eq!(r.result, Ok(()));
    assert!(r.aead_opens);
}

// ------------------------------------------------------------ open_blob

// `open_blob`: a padded body sealed properly opens; the same body with one
// byte of its zero padding set fails at the padding (step 5), after the
// AEAD has opened. Mutated sealed bytes fail at the AEAD, which does not
// open them.
// spec: SEAL-GEN-011
#[test]
fn open_blob_reaches_the_padding() {
    // Mode 1, no edit: the fixture's padded body, sealed here.
    let ok = reach_open_blob(&[1]);
    opens(&ok);
    let padded = ok.shaped.clone().unwrap();
    assert_eq!(padded.len(), usize::try_from(padded_len(4 + BLOB_BODY.len() as u64)).unwrap());
    let last = padded.len() - 1;
    let bad = reach_open_blob(&input(&[1], &[edit(last, 0, 1)]));
    after_aead(&bad, SealError::BlobOpen);
    // By construction only the plaintext changed: the last padding byte.
    let shaped = bad.shaped.unwrap();
    assert_eq!(shaped[..last], padded[..last]);
    assert_eq!((padded[last], shaped[last]), (0, 1));
    // The fixture's sealed bytes as they are open; one ciphertext byte
    // changed (its hash recomputed) fails at the AEAD itself.
    opens(&reach_open_blob(&[0]));
    let r = reach_open_blob(&input(&[0, 0], &[edit(100, 1, 1)]));
    assert_eq!((r.result, r.aead_opens), (Err(SealError::BlobOpen), false));
}

// ------------------------------------------------------------ the snapshots

/// The positions of the commitment of epoch 2 in the fixture's membership
/// bytes, and those bytes.
fn commitment_in_membership() -> (usize, Vec<u8>) {
    let b = blobs();
    let s = &b.sim;
    let f = fields(&b.body).unwrap();
    let c = EpochRoot::from_bytes(s.roots[&2]).commitment(&s.board, 2);
    let at = f.membership.windows(32).position(|w| w == c).expect("the membership records the commitment of epoch 2");
    (at, f.membership.to_vec())
}

/// The four inputs of the snapshot tests, for either mode, and what each
/// opener must answer, with the inflate's failure given by `inflate`.
fn snapshot_cases(open: fn(&[u8]) -> Reach, inflate: &[u8]) {
    // Opens: the fixture's body, compressed and sealed here (mode 2, the
    // hash its own), and laid out again from its fields (mode 3).
    let ok = open(&[2, 0]);
    opens(&ok);
    let honest = ok.shaped.clone().unwrap();
    assert_eq!(honest, blobs().body);
    opens(&open(&[3, 0]));

    // The inflate (step 6): `seal/inflate`, an error of its own.
    after_aead(&open(inflate), SealError::Inflate);

    // The body parser (step 7): the honest body with its magic changed,
    // nothing else. It inflates to its expected length and its `S`, version
    // and state hash are the expected ones, so the parse of §20.4 is the
    // only check it fails.
    let r = open(&input(&[2, 0], &[edit(0, 0, b'X')]));
    after_aead(&r, SealError::BlobOpen);
    let shaped = r.shaped.unwrap();
    assert_eq!((&shaped[1..], shaped[0]), (&honest[1..], b'X'));

    // The membership parser: the membership cut to 10 bytes, the body laid
    // out again around it with its lengths and its own state hash. The body
    // parses; its membership bytes do not.
    let r = open(&input(&[3, 1], &[edit(10, 4, 0)]));
    after_aead(&r, SealError::BlobOpen);
    let shaped = r.shaped.unwrap();
    let f = fields(&shaped).unwrap();
    assert_eq!(f.membership.len(), 10);
    assert!(Membership::from_canonical_bytes(f.membership).is_err());

    // The agreement check: one byte of the commitment of epoch 2 flipped.
    // The membership parses, and the history (unchanged) records another
    // commitment for that epoch (§21.6 step 11).
    let (at, membership) = commitment_in_membership();
    let r = open(&input(&[3, 1], &[edit(at, 1, 1)]));
    after_aead(&r, SealError::BlobOpen);
    let shaped = r.shaped.unwrap();
    let f = fields(&shaped).unwrap();
    assert_ne!(f.membership, &membership[..]);
    assert!(Membership::from_canonical_bytes(f.membership).is_ok());
}

// `open_snapshot`: one input opens; one fails with `seal/inflate`; one is
// refused by the body parser; one by the membership parser and one by the
// agreement check, each after the AEAD has opened.
// spec: SEAL-GEN-011
#[test]
fn open_snapshot_reaches_its_parsers() {
    // Mode 1: the fixture's compressed body, `inflated` its length − 1.
    snapshot_cases(reach_open_snapshot, &[1, 0]);
    opens(&reach_open_snapshot(&[1, 1]));
}

// `open_snapshot_declared`: the same four in the declared mode, where no
// hash is compared; the inflate fails on a stream cut in half.
// spec: SEAL-GEN-011
#[test]
fn open_snapshot_declared_reaches_its_parsers() {
    let half = blobs().compressed.len() / 2;
    snapshot_cases(reach_open_snapshot_declared, &input(&[1, 0], &[edit(half, 4, 0)]));
    opens(&reach_open_snapshot_declared(&[1, 0]));
}

// ------------------------------------------------------------ pairing

/// N's effects for `data`, flattened, and its abort code.
fn n_run(data: &[u8]) -> (Vec<Vec<&'static str>>, Option<SealError>) {
    let r = reach_pair_new(data);
    (r.effects, r.abort)
}

// `pair_new`: N goes as far as the input lets it: `AskSas` after the code,
// slots A and C; `FetchSnapshot` after slot D; `Joined` after the snapshot,
// the log and the frame a `Skip` names (`FetchFrames`).
// spec: SEAL-GEN-011
#[test]
fn pair_new_reaches_the_join() {
    let (fx, abort) = n_run(&[3]);
    assert_eq!((fx.last().unwrap().as_slice(), abort), (&["AskSas"][..], None));
    let (fx, abort) = n_run(&[5]);
    assert_eq!((fx.last().unwrap().as_slice(), abort), (&["FetchSnapshot"][..], None));
    let (fx, abort) = n_run(&[8]);
    assert_eq!(abort, None);
    assert_eq!(
        fx,
        [
            vec!["FetchSlot"],
            vec!["PostSlot", "FetchSlot"],
            vec!["AskSas"],
            vec!["ShowAccepted", "FetchSlot"],
            vec!["FetchSnapshot"],
            vec!["ReadLog"],
            vec!["FetchFrames"],
            vec!["Joined"],
        ]
    );
    // The same through `n_event`'s honest choice (`k % 8` 0 to 3).
    let (fx, _) = n_run(&[0, 0, 1, 2, 3, 0, 1, 2, 3]);
    assert_eq!(fx.last().unwrap(), &vec!["Joined"]);
}

// `pair_new`: a mutated slot C, slot D and `Entries`, each refused with its
// own code at the step that reads it: slot C MACed again with `r_E` changed
// fails the commitment (`seal/commit_mismatch`), and without the new MAC the
// MAC (`seal/slot_mac`); slot D MACed again with its ciphertext changed
// fails the AEAD (`seal/bundle_open`); a bundle JSON broken and sealed again
// under the bundle key fails the parse (`seal/bundle_invalid`); one entry of
// the log changed fails step 6 (`seal/bundle_invalid`, after `ReadLog`).
// spec: SEAL-GEN-011
#[test]
fn pair_new_refuses_mutated_events_at_their_steps() {
    // Slot C: label, role, `body_len`, then `epk_E` (32) and `r_E` (32).
    let r_e = 4 + 32 + 1;
    let (fx, abort) = n_run(&input(&[2, 4 + 8, 1], &[edit(r_e, 1, 1)]));
    assert_eq!((fx.len(), abort), (3, Some(SealError::CommitMismatch)));
    let (fx, abort) = n_run(&input(&[2, 4, 1], &[edit(r_e, 1, 1)]));
    assert_eq!((fx.len(), abort), (3, Some(SealError::SlotMac)));
    // Slot D: the nonce (24), then the ciphertext.
    let (fx, abort) = n_run(&input(&[4, 4 + 8, 1], &[edit(4 + 24 + 10, 1, 1)]));
    assert_eq!((fx.len(), abort), (5, Some(SealError::BundleOpen)));
    let (fx, abort) = n_run(&input(&[4, 4 + 16, 1], &[edit(0, 0, b'[')]));
    assert_eq!((fx.len(), abort), (5, Some(SealError::BundleInvalid)));
    // The bundle as it is, sealed again with another nonce: accepted.
    let (fx, abort) = n_run(&[4, 4 + 16, 0]);
    assert_eq!((fx.last().unwrap().as_slice(), abort), (&["FetchSnapshot"][..], None));
    // `Entries`: the first entry with one byte of its ciphertext changed.
    let (fx, abort) = n_run(&input(&[6, 4, 1], &[edit(300, 1, 1)]));
    assert_eq!(fx[5], ["ReadLog"]);
    assert_eq!((fx.len(), abort), (7, Some(SealError::BundleInvalid)));
}

// `pair_bundle`: plaintexts of 16,000 and of 16,001 bytes pass the harness,
// the second refused by slot D's framing with `seal/slot_mac` (§17.4: a body
// of at most 16,040 bytes); one far over the u16 `body_len` is bounded, not
// wrapped; a broken bundle is refused with `seal/bundle_invalid` (§17.9 step
// 3) and the honest one accepted; and a run is a function of its input.
// spec: SEAL-GEN-011
#[test]
fn pair_bundle_expects_what_the_spec_says_at_each_size() {
    let r = reach_pair_bundle(&[[1].as_slice(), &[b'a'; 16_000]].concat());
    assert_eq!((r.slot_d.len(), r.abort), (36 + 16_040, Some(SealError::BundleInvalid)));
    let r = reach_pair_bundle(&[[1].as_slice(), &[b'a'; 16_001]].concat());
    assert_eq!((r.slot_d.len(), r.abort), (36 + 16_041, Some(SealError::SlotMac)));
    let r = reach_pair_bundle(&[[1].as_slice(), &vec![b'a'; 70_000]].concat());
    assert_eq!((r.slot_d.len(), r.abort), (36 + BUNDLE_PT_BOUND + 40, Some(SealError::SlotMac)));
    assert_eq!(36 + BUNDLE_PT_BOUND + 40, 36 + 65_535);

    let r = reach_pair_bundle(&[0]);
    assert_eq!((r.effects, r.abort), (vec!["FetchSnapshot"], None));
    let r = reach_pair_bundle(&input(&[0], &[edit(0, 0, b'[')]));
    assert_eq!(r.abort, Some(SealError::BundleInvalid));
    let r = reach_pair_bundle(&input(&[0], &[edit(30, 1, 0x20)]));
    assert_eq!(r.abort, Some(SealError::BundleInvalid));

    for data in [vec![0], input(&[0], &[edit(7, 2, b'x')]), vec![1, 2, 3]] {
        assert_eq!(reach_pair_bundle(&data).slot_d, reach_pair_bundle(&data).slot_d);
    }
}
