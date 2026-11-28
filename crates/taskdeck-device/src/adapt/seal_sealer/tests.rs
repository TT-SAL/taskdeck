//! Unit tests of `SealSealer`: the contracts of engine.md §6.1 the engine
//! relies on that the in-process flow does not reach, the identity export of
//! device.md §6.4.7, and the caller's duties of seal.md §25.

use std::collections::BTreeMap;

use taskdeck_seal as seal;
use taskdeck_seal::{DeviceKeys, Entropy, EpochRoot, GenesisAnchor, Membership, OsEntropy, RecoveryKeys, RecoverySeed};
use taskdeck_sync::{
    Body, Class, ClassifyInput, CtlIntent, CtlItem, Hash32, Head, Kind, PlainEntry, Receipt, Sealer, SnapInfo,
    SnapshotMaterial, StateMachine,
};

use super::super::membership::{Fields, SealMembership};
use super::super::{CoreMachine, GenesisInput, SealSealer};

const T0: i64 = 1_700_000_000_000;

/// A board just created: the sealer, the recovery keys, `R_1`'s bytes, the
/// log (frames and `C_0 ..= C_L`), and `M_L`.
struct Board {
    s: SealSealer,
    recovery: RecoveryKeys,
    root1: [u8; 32],
    salt: [u8; 16],
    frames: Vec<Vec<u8>>,
    chains: Vec<Hash32>,
    m: SealMembership,
    own: BTreeMap<u64, Hash32>,
}

fn new_board() -> Board {
    let recovery = RecoverySeed::generate(&mut OsEntropy).expect("seed").keys();
    let mut salt = [0u8; 16];
    OsEntropy.fill(&mut salt).expect("entropy");
    let root = EpochRoot::generate(&mut OsEntropy).expect("root");
    let root1 = *root.to_bytes();
    let sm = CoreMachine::new();
    let (s, f1) = SealSealer::create(GenesisInput {
        recovery: &recovery,
        salt,
        keys: DeviceKeys::generate(&mut OsEntropy).expect("keys"),
        root1: root,
        device_name: "Desk",
        at_ms: T0,
        board_hash: sm.board_hash(&taskdeck_core::board::BoardData::empty()),
    })
    .expect("genesis");
    let zero = s.membership_zero();
    let c0 = s.chain_zero();
    let mut b = Board { s, recovery, root1, salt, frames: Vec::new(), chains: vec![c0], m: zero, own: BTreeMap::new() };
    let r = b.classify_next(&f1);
    assert_eq!(r.class, Class::Accept, "{:?}", r.code);
    b.commit(f1, r);
    b
}

impl Board {
    fn head(&self) -> Head {
        Head { seq: self.frames.len() as u64, chain: *self.chains.last().expect("C_0") }
    }

    /// Classifies `frame` at the next seq.
    fn classify_next(&mut self, frame: &[u8]) -> Receipt<SealMembership> {
        let seq = self.frames.len() as u64 + 1;
        classify_at(&mut self.s, seq, frame, &self.chains, &self.m, &self.own)
    }

    fn commit(&mut self, frame: Vec<u8>, r: Receipt<SealMembership>) {
        self.chains.push(r.chain);
        self.frames.push(frame);
        self.m = r.membership;
    }

    /// Seals an entry of this device at the head with the next counter.
    fn seal(&mut self, body: Body) -> (PlainEntry, taskdeck_sync::Sealed) {
        let entry = PlainEntry { entry_id: self.s.new_entry_id().expect("id"), at_ms: T0 + 1, based_on: self.head(), body };
        let ctr = self.ctr() + 1;
        let sealed = self.s.seal(&self.m, ctr, &entry).expect("seals");
        self.own.insert(ctr, sealed.frame_hash);
        (entry, sealed)
    }

    fn ctr(&self) -> u64 {
        let me = self.s.own_dev();
        self.s.view(&self.m).devices.iter().find(|d| d.dev == me).map_or(0, |d| d.last_ctr)
    }

    fn anchor(&self) -> GenesisAnchor {
        GenesisAnchor {
            rsign_pk: self.recovery.public().sign_pk,
            salt: self.salt,
            root1: EpochRoot::from_bytes(self.root1),
        }
    }
}

fn classify_at(
    s: &mut SealSealer,
    seq: u64,
    frame: &[u8],
    chains: &[Hash32],
    m: &SealMembership,
    own: &BTreeMap<u64, Hash32>,
) -> Receipt<SealMembership> {
    let chain_at = |b: u64| chains.get(b as usize).copied().filter(|_| b >= 1);
    let own_sealed = |c: u64| own.get(&c).copied();
    let skip = |_: u64| -> Option<&[u8]> { None };
    let input = ClassifyInput {
        seq,
        frame,
        prev_chain: chains[seq as usize - 1],
        membership: m,
        chain_at: &chain_at,
        retained_from: 1,
        own_sealed: &own_sealed,
        skip_targets: &skip,
    };
    s.classify(&input).expect("classifies")
}

/// What `taskdeck-seal` itself, with no own identity, reads of a frame.
fn seal_read(b: &Board, seq: u64, frame: &[u8], roots: &BTreeMap<u32, EpochRoot>) -> seal::Receipt {
    let m = Membership::from_canonical_bytes(b.m.bytes()).expect("parses");
    let rs = |e: u32| roots.get(&e);
    let chains = b.chains.clone();
    let chain_at = move |x: u64| chains.get(x as usize).copied().map(seal::Chain);
    let none = |_: u64| -> Option<&[u8]> { None };
    let input = seal::ReceiptInput {
        seq,
        frame,
        prev_chain: seal::Chain(b.chains[seq as usize - 1]),
        membership: &m,
        roots: &rs,
        chain_at: &chain_at,
        retained_from: 1,
        own: None,
        supported_rules: &[1],
        genesis: None,
        skip_targets: &none,
    };
    seal::classify(&input).expect("classifies")
}

fn same(a: &Receipt<SealMembership>, b: &Receipt<SealMembership>) -> bool {
    a.class == b.class
        && a.code == b.code
        && a.chain == b.chain
        && a.author == b.author
        && a.ctr == b.ctr
        && a.frame_hash == b.frame_hash
        && a.own_dev_foreign == b.own_dev_foreign
        && a.based_on_unchecked == b.based_on_unchecked
        && a.opened == b.opened
        && a.membership.bytes() == b.membership.bytes()
        && a.halt == b.halt
}

/// The roots an identity export holds, read from device.md §6.4.7's layout.
type Exported = (Vec<(u32, [u8; 32])>, Vec<([u8; 16], u32, [u8; 32])>, bool);

fn exported_roots(id: &[u8]) -> Exported {
    let mut p = 4 + 2 + 80;
    let n = u16::from_be_bytes([id[p], id[p + 1]]) as usize;
    p += 2;
    let mut roots = Vec::new();
    for _ in 0..n {
        let e = u32::from_be_bytes(id[p..p + 4].try_into().expect("4"));
        roots.push((e, id[p + 4..p + 36].try_into().expect("32")));
        p += 36;
    }
    let k = id[p] as usize;
    p += 1;
    let mut pending = Vec::new();
    for _ in 0..k {
        let entry: [u8; 16] = id[p..p + 16].try_into().expect("16");
        let e = u32::from_be_bytes(id[p + 16..p + 20].try_into().expect("4"));
        pending.push((entry, e, id[p + 20..p + 52].try_into().expect("32")));
        p += 52;
    }
    let anchor = id[p] == 1;
    (roots, pending, anchor)
}

// ---------------------------------------------------------------- membership

/// engine.md §6.1 `membership_zero`: `M_0` (seal.md §13.3), whose bytes
/// seal's parser accepts, epoch 0, nothing in it.
#[test]
fn membership_zero_is_the_empty_state() {
    let b = new_board();
    let z = b.s.membership_zero();
    assert_eq!(z.seal().epoch(), 0);
    assert_eq!(b.s.membership_from_bytes(&b.s.membership_bytes(&z)).expect("parses").bytes(), z.bytes());
    let v = b.s.view(&z);
    assert!(v.devices.is_empty() && v.recovery.is_empty() && v.recovery_fp.is_empty());
}

/// engine.md §6.2: every field of the view is the field of seal.md §13.1;
/// the parser here reads the bytes seal wrote and writes them back exactly.
#[test]
fn the_view_agrees_with_seal_and_the_bytes_round_trip() {
    let b = new_board();
    let m = b.m.seal();
    let f = Fields::parse(b.m.bytes()).expect("parses");
    assert_eq!(f.encode().expect("encodes"), b.m.bytes());
    let v = b.s.view(&b.m);
    assert_eq!(v.board_id, m.board().0);
    assert_eq!((v.epoch, v.rules), (m.epoch(), m.rules()));
    assert_eq!(v.devices.len(), m.devices().len());
    for (dv, d) in v.devices.iter().zip(m.devices()) {
        assert_eq!((dv.dev, dv.device_no, dv.last_ctr, &dv.name), (d.dev.0, d.device_no, d.last_ctr, &d.name));
        assert_eq!(dv.key_fp, seal::key_fingerprint(&d.sign_pk, &d.hpke_pk));
    }
    let r = m.current_recovery();
    assert_eq!(v.recovery_dev, r.dev.0);
    assert_eq!(v.recovery_fp, b.recovery.public().fingerprint());
    assert_eq!((v.epoch_seq, v.home_gen, v.ctl_seqs.clone()), (1, 1, vec![1]));
    assert_eq!((v.epoch_clock_ms, v.clock_ms), (T0, T0));
}

/// Finding f1-4: the `M_1` the creating device lays out for the Genesis
/// state hash is exactly the state `classify` produces from seq 1
/// (seal.md §13.3), and the item's hash is ENG-HASH-001 of it.
#[test]
fn the_genesis_state_hash_covers_the_m1_classify_produces() {
    let b = new_board();
    let r = seal_read_genesis(&b);
    let Some(seal::CtlItem::Genesis(g)) = r.opened.and_then(|o| o.items).and_then(|v| v.into_iter().next()) else {
        panic!("a Genesis item")
    };
    let board_hash = CoreMachine::new().board_hash(&taskdeck_core::board::BoardData::empty());
    assert_eq!(g.state_hash, taskdeck_sync::state_hash(1, &board_hash, b.m.bytes()));
    assert_eq!(r.membership.to_canonical_bytes(), b.m.bytes());
}

fn seal_read_genesis(b: &Board) -> seal::Receipt {
    let zero = Membership::from_canonical_bytes(&Fields::default().encode().expect("M_0")).expect("M_0");
    let anchor = b.anchor();
    let none_root = |_: u32| None;
    let none_chain = |_: u64| None;
    let none = |_: u64| -> Option<&[u8]> { None };
    let input = seal::ReceiptInput {
        seq: 1,
        frame: &b.frames[0],
        prev_chain: seal::Chain(b.chains[0]),
        membership: &zero,
        roots: &none_root,
        chain_at: &none_chain,
        retained_from: 1,
        own: None,
        supported_rules: &[1],
        genesis: Some(&anchor),
        skip_targets: &none,
    };
    seal::classify(&input).expect("classifies")
}

// ---------------------------------------------------------------- classify

/// engine.md §6.1 `classify`: for the same inputs the same receipt, and a
/// repeat changes nothing the sealer holds.
#[test]
fn classify_is_repeatable() {
    let mut b = new_board();
    let (_, sealed) = b.seal(Body::Cmd(b"{\"op\":\"forget\",\"id\":1}".to_vec()));
    let first = b.classify_next(&sealed.frame);
    let id1 = b.s.export_identity().to_vec();
    let again = b.classify_next(&sealed.frame);
    assert!(same(&first, &again));
    assert_eq!(b.s.export_identity().to_vec(), id1);
    assert!(!again.keys_changed);
    assert_eq!(first.class, Class::Accept);
    assert_eq!(first.author, b.s.own_dev());
    assert_eq!(first.ctr, 1);
    assert_eq!(first.frame_hash, sealed.frame_hash);
    assert!(!first.own_dev_foreign);
    let o = first.opened.expect("opened");
    assert_eq!((o.kind, o.payload.as_slice()), (Kind::Cmd, b"{\"op\":\"forget\",\"id\":1}".as_slice()));
}

/// engine.md §6.1 `classify`, seal.md §14.7: a frame under this device's
/// own `dev` that `own_sealed` does not return is flagged.
#[test]
fn an_own_frame_the_engine_did_not_seal_is_flagged() {
    let mut b = new_board();
    let (_, sealed) = b.seal(Body::Cmd(b"{\"op\":\"forget\",\"id\":1}".to_vec()));
    b.own.clear();
    let r = b.classify_next(&sealed.frame);
    assert!(r.own_dev_foreign);
}

/// engine.md §6.1 `classify` of a frame that fails seal.md §14.2 step 1.
#[test]
fn a_frame_that_is_no_frame_is_skipped() {
    let mut b = new_board();
    let r = b.classify_next(&[0u8; 700]);
    assert_eq!((r.class, r.code, r.author, r.ctr), (Class::Skip, Some("seal/frame_format"), [0; 16], 0));
    assert_eq!(r.membership.bytes(), b.m.bytes());
    assert!(b.s.frame_author(&[0u8; 10]).is_err());
}

// ---------------------------------------------------------------- seal

/// SEAL-FRAME-005, ENG-SEALER-005: asked again for the same entry under the
/// same counter and epoch, `seal` returns the stored bytes and draws nothing.
#[test]
fn sealing_the_same_entry_again_returns_the_same_bytes() {
    let mut b = new_board();
    let (entry, first) = b.seal(Body::Cmd(b"{\"op\":\"forget\",\"id\":1}".to_vec()));
    let again = b.s.seal(&b.m, 1, &entry).expect("seals");
    assert_eq!(again.frame, first.frame);
    assert!(!again.keys_changed);
    let other = PlainEntry { entry_id: b.s.new_entry_id().expect("id"), ..entry };
    assert_ne!(b.s.seal(&b.m, 1, &other).expect("seals").frame, first.frame);
    assert_eq!(b.s.frame_author(&first.frame).expect("a header"), (b.s.own_dev(), 1));
    assert_eq!(b.s.seal(&b.m, 0, &other).expect_err("ctr 0").code, "seal/frame_format");
}

/// ENG-SEALER-007, ENG-SEALER-008, ENG-SEALER-009, SEAL-ENV-004: a rotation
/// to `E + 1` is sealed under `E`, wraps to the current devices and the
/// recovery identity once each, ascending; its root is pending until its
/// acceptance, which keeps the root the own wrap gives and removes the
/// pending one in the same call.
#[test]
fn an_own_rotation_is_pending_until_accepted() {
    let mut b = new_board();
    let rotate = Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None });
    let (entry, sealed) = b.seal(rotate);
    assert!(sealed.keys_changed);
    assert_eq!(sealed.epoch, 1);
    let (roots, pending, anchor) = exported_roots(&b.s.export_identity());
    assert_eq!(roots.iter().map(|r| r.0).collect::<Vec<_>>(), vec![1]);
    assert_eq!(pending.len(), 1);
    assert_eq!((pending[0].0, pending[0].1), (entry.entry_id, 2));
    assert!(anchor);

    // What seal reads: the wraps' recipients.
    let roots1 = BTreeMap::from([(1, EpochRoot::from_bytes(b.root1))]);
    let r = seal_read(&b, 2, &sealed.frame, &roots1);
    let Some(seal::CtlItem::EpochRotated(rot)) = r.opened.and_then(|o| o.items).and_then(|v| v.into_iter().next()) else {
        panic!("a rotation")
    };
    let mut want = vec![b.s.own_dev(), b.s.view(&b.m).recovery_dev];
    want.sort_unstable();
    let got: Vec<[u8; 16]> = rot.wraps.iter().map(|w| w[..16].try_into().expect("16")).collect();
    assert_eq!(got, want);
    assert_eq!(rot.epoch, 2);

    let r = b.classify_next(&sealed.frame);
    assert_eq!(r.class, Class::Accept, "{:?}", r.code);
    assert!(r.keys_changed);
    assert_eq!(r.opened.as_ref().expect("opened").items, vec![CtlItem::EpochRotated { epoch: 2, purge_before: None }]);
    b.commit(sealed.frame.clone(), r);
    let (roots, pending, _) = exported_roots(&b.s.export_identity());
    assert_eq!(roots.iter().map(|r| r.0).collect::<Vec<_>>(), vec![1, 2]);
    assert!(pending.is_empty());
    // The next entry is sealed under epoch 2 (SEAL-ENV-003).
    let (_, next) = b.seal(Body::Cmd(b"{\"op\":\"forget\",\"id\":1}".to_vec()));
    assert_eq!(next.epoch, 2);
    let r = b.classify_next(&next.frame);
    assert_eq!(r.class, Class::Accept, "{:?}", r.code);
    // The control history holds seq 1 and the rotation (device.md §6.4.8).
    assert_eq!(b.s.history().iter().map(|h| h.0).collect::<Vec<_>>(), vec![1, 2]);
}

/// SEAL-MEM-016, engine.md §6.1 `discard_pending`.
#[test]
fn a_refused_rotations_root_is_discarded() {
    let mut b = new_board();
    let rotate = Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None });
    let (entry, _) = b.seal(rotate);
    assert!(b.s.discard_pending(&entry.entry_id));
    assert!(!b.s.discard_pending(&entry.entry_id));
    let (_, pending, _) = exported_roots(&b.s.export_identity());
    assert!(pending.is_empty());
}

/// engine.md §6.1 `forget_roots_below` (§28.2): the roots below and the
/// anchor, which carries `R_1` (decision f1-8).
#[test]
fn forgetting_roots_drops_the_anchor_with_r1() {
    let mut b = new_board();
    assert!(!b.s.forget_roots_below(1));
    assert!(b.s.forget_roots_below(2));
    let (roots, _, anchor) = exported_roots(&b.s.export_identity());
    assert!(roots.is_empty() && !anchor);
    assert!(!b.s.forget_roots_below(2));
}

// ---------------------------------------------------------------- open_own

/// engine.md §6.1 `open_own`: a frame this device signed, also in a later
/// process that keeps no sealing of it (finding f1-3), gives its entry id;
/// another author's frame is an error.
#[test]
fn open_own_opens_an_own_frame_in_a_new_process() {
    let mut b = new_board();
    let (entry, sealed) = b.seal(Body::Cmd(b"{\"op\":\"forget\",\"id\":1}".to_vec()));
    assert_eq!(b.s.open_own(&b.m, &sealed.frame).expect("opens"), entry.entry_id);
    let later = SealSealer::open(b.s.board_id(), &b.s.export_identity(), &[]).expect("opens");
    assert_eq!(later.open_own(&b.m, &sealed.frame).expect("opens"), entry.entry_id);
    assert!(later.open_own(&b.m, &b.frames[0]).is_err(), "seq 1 is the recovery identity's");
    let mut forged = sealed.frame.clone();
    let last = forged.len() - 1;
    forged[last] ^= 1;
    assert!(later.open_own(&b.m, &forged).is_err());
}

// ---------------------------------------------------------------- the identity

/// device.md §6.4.7: the layout and its length, and `open` of an export
/// gives the same export.
#[test]
fn the_identity_export_round_trips() {
    let mut b = new_board();
    let rotate = Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None });
    b.seal(rotate);
    let id = b.s.export_identity();
    assert_eq!(&id[..4], b"TDI1");
    assert_eq!(u16::from_be_bytes([id[4], id[5]]), 80);
    assert_eq!(id.len(), 10 + 80 + 36 + 52 + 80);
    let history: Vec<Vec<u8>> = b.s.history().into_iter().map(|h| h.1).collect();
    let again = SealSealer::open(b.s.board_id(), &id, &history).expect("opens");
    assert_eq!(again.export_identity().to_vec(), id.to_vec());
    assert_eq!(again.own_dev(), b.s.own_dev());
    assert_eq!(again.history(), b.s.history());
}

/// device.md §6.4.7: anything else is `device/record_format`.
#[test]
fn identity_bytes_that_are_not_the_layout_are_refused() {
    let b = new_board();
    let id = b.s.export_identity().to_vec();
    let board = b.s.board_id();
    let refuse = |bytes: &[u8], board: [u8; 16]| {
        SealSealer::open(board, bytes, &[]).err().map(|e| e.code)
    };
    let mut magic = id.clone();
    magic[0] = b'X';
    assert_eq!(refuse(&magic, board), Some("device/record_format"));
    let mut trailing = id.clone();
    trailing.push(0);
    assert_eq!(refuse(&trailing, board), Some("device/record_format"));
    assert_eq!(refuse(&id[..id.len() - 1], board), Some("device/record_format"));
    assert_eq!(refuse(&id, [9; 16]), Some("device/record_format"), "an anchor of another board");
    let rec = b.s.history().remove(0).1;
    assert!(SealSealer::open(board, &id, &[rec.clone(), rec]).is_err(), "a history seq twice");
}

// ---------------------------------------------------------------- missed wraps

/// engine.md §6.1 `take_missed_wraps` (ruling S24, SEAL-WRAP-003): a wrap of
/// an epoch whose commitment `m` records opens at once; one that does not
/// match is `seal/commitment` and nothing is learned.
#[test]
fn missed_wraps_open_against_the_recorded_commitment() {
    let mut b = new_board();
    let before = b.s.export_identity().to_vec();
    let rotate = Body::Ctl(CtlIntent::Rotate { revoke: vec![], reinstate: vec![], purge_before: None });
    let (_, sealed) = b.seal(rotate);
    let (_, pending, _) = exported_roots(&b.s.export_identity());
    let root2 = EpochRoot::from_bytes(pending[0].2);
    let r = b.classify_next(&sealed.frame);
    b.commit(sealed.frame, r);
    let board = seal::BoardId(b.s.board_id());
    let commit = root2.commitment(&board, 2);
    assert_eq!(b.m.commitment(2), Some(commit));

    // The same identity before it held root 2, as a reinstated device would be.
    let mut late = SealSealer::open(b.s.board_id(), &before, &[]).expect("opens");
    let keys = DeviceKeys::from_identity_bytes(&before[6..86]).expect("keys");
    let me = keys.public();
    let wrong = EpochRoot::generate(&mut OsEntropy).expect("root");
    let bad = seal::wrap_root(&wrong, &board, 2, &commit, me.dev, &me.hpke_pk, &mut OsEntropy).expect("wraps");
    assert_eq!(late.take_missed_wraps(&b.m, 2, &[bad]).expect_err("no match").code, "seal/commitment");
    let good = seal::wrap_root(&root2, &board, 2, &commit, me.dev, &me.hpke_pk, &mut OsEntropy).expect("wraps");
    assert!(late.take_missed_wraps(&b.m, 2, &[good]).expect("opens"));
    let (roots, _, _) = exported_roots(&late.export_identity());
    assert_eq!(roots.iter().map(|r| r.0).collect::<Vec<_>>(), vec![1, 2]);
    assert!(!late.take_missed_wraps(&b.m, 2, &[good]).expect("held already"));
}

// ---------------------------------------------------------------- snapshots

/// engine.md §6.1 `seal_snapshot`, `open_snapshot` (seal.md §20): the
/// history up to `S` goes into the body, the inflated length the adapter
/// reports is the body's (finding f1-10), and both opening modes give the
/// material back.
#[test]
fn a_snapshot_seals_and_opens_in_both_modes() {
    let b = new_board();
    let sm = CoreMachine::new();
    let board = sm.canonical_bytes(&taskdeck_core::board::BoardData::empty());
    let membership = b.m.bytes().to_vec();
    let material = SnapshotMaterial {
        seq: 1,
        chain: b.chains[1],
        hash_version: 1,
        state_hash: taskdeck_sync::state_hash(1, &sm.board_hash(&taskdeck_core::board::BoardData::empty()), &membership),
        board,
        membership,
    };
    let mut s = SealSealer::open(b.s.board_id(), &b.s.export_identity(), &b.s.history().into_iter().map(|h| h.1).collect::<Vec<_>>())
        .expect("opens");
    let blob = s.seal_snapshot(&b.m, &material).expect("seals");
    assert_eq!(blob.epoch, 1);
    let info = SnapInfo {
        seq: 1,
        hash: material.state_hash,
        hash_version: 1,
        blob: blob.id,
        sha256: blob.sha256,
        size: blob.bytes.len() as u64,
        parts: blob.parts,
        inflated: blob.inflated,
        epoch: 1,
    };
    let parts = seal::split_parts(&blob.bytes);
    let writer = s.own_dev();
    let (m, epoch, inflated) = s.open_snapshot(&parts, &info, &writer, true).expect("opens exactly");
    assert_eq!((m, epoch, inflated), (material.clone(), 1, blob.inflated));
    let (m, epoch, inflated) = s.open_snapshot(&parts, &info, &writer, false).expect("opens declared");
    assert_eq!((m, epoch, inflated), (material.clone(), 1, blob.inflated));
    let wrong = SnapInfo { inflated: blob.inflated + 1, ..info };
    assert!(s.open_snapshot(&parts, &wrong, &writer, true).is_err());
    // Without its history the sealer cannot fill the body.
    let mut bare = SealSealer::open(b.s.board_id(), &b.s.export_identity(), &[]).expect("opens");
    assert_eq!(bare.seal_snapshot(&b.m, &material).expect_err("no history").code, "seal/blob_open");
}

// ---------------------------------------------------------------- texts

/// engine.md §6.1 `sync_fingerprint`, `move_code`, `check_move_code` (seal.md §15.3, §15.4).
#[test]
fn fingerprint_and_move_code_are_seals() {
    let b = new_board();
    let head = b.head();
    let board = seal::BoardId(b.s.board_id());
    let want = seal::sync_fingerprint(&board, &seal::Head { seq: head.seq, chain: seal::Chain(head.chain) });
    assert_eq!(b.s.sync_fingerprint(&head), want.to_string());
    let code = b.s.move_code(1, &[7; 32]);
    assert_eq!(code.len(), 9);
    assert!(b.s.check_move_code(1, &[7; 32], &code).is_ok());
    assert_eq!(b.s.check_move_code(2, &[7; 32], &code).expect_err("another fence").code, "seal/move_code");
    assert_eq!(b.s.supported_envelopes(), vec![(1, 1)]);
    assert_eq!(b.s.chain_zero(), seal::chain_zero(&board).0);
}
