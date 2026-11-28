//! `SealSealer`: `taskdeck_sync::Sealer` over `taskdeck-seal` (engine.md
//! §6.1, decision D6; device.md §4.1, §6.4.7, §24, §25).
//!
//! The sealer owns every secret of this identity: the device keys, the epoch
//! roots it holds, the roots it drew for its own rotations not yet accepted
//! (pending), and the genesis anchor on the creating device. Each is held in
//! a type of `taskdeck-seal` that zeroizes on drop, and leaves the sealer only
//! through `export_identity` (device.md §6.4.7), in `Zeroizing` bytes.
//!
//! The caller's duties of seal.md §25 carried here:
//! - SEAL-FRAME-005: per counter the last sealing is kept, and asked again
//!   for the same entry under the same epoch `seal` returns those bytes.
//! - SEAL-ENV-003, SEAL-ENV-004: `seal` seals under `m`'s current epoch (the
//!   engine passes `M_K`, engine.md §36); a rotation to `E + 1` is sealed
//!   under `E`.
//! - SEAL-MEM-016: a refused rotation's root is dropped by `discard_pending`
//!   and is never a sealing root (only `roots` are).
//! - SEAL-KEY-012: `K_entry` seals and opens entries, `K_snap` snapshots.
//! - SEAL-KEY-013: a root enters `roots` only from `classify` (which checks
//!   it against `c_e`), from `unwrap_root` against the commitment the
//!   membership records, or as `R_1` of this board's own anchor; `seal` and
//!   `seal_snapshot` check the root against `m`'s commitment again.
//! - SEAL-WRAP-003: a missed wrap that does not open or does not match is
//!   `seal/commitment` from `take_missed_wraps`, and `Halt::Commitment` from
//!   `classify`.
//!
//! Every random byte comes from `taskdeck_seal::OsEntropy` (DEV-GEN-003).
//! No method panics on any input; `classify` computes everything before it
//! changes what the sealer holds, so a panic inside `taskdeck-seal` leaves
//! the sealer as it was (the engine's `guard`, `engine.md` §6.1).

use std::cell::RefCell;
use std::collections::BTreeMap;

use subtle::ConstantTimeEq;
use taskdeck_seal as seal;
use taskdeck_seal::{
    BlobId, BlobRef, BoardId, Chain, DevId, DeviceKeys, EntryId, EpochRoot, FrameHash, GenesisAnchor, HistoryRecord,
    KeyKind, Membership, OsEntropy, OwnIdentity, Plain, ReceiptInput, Recipient, RecoveryKeys, Signer, SnapshotBody,
};
use taskdeck_sync::{
    BeatInfo, Body, Class, ClassifyInput, CtlIntent, CtlItem, Dev, Halt, Hash32, Head, Kind, MembershipView, Opened,
    PlainEntry, Receipt, Sealed, SealedBlob, Sealer, SealerError, SnapInfo, SnapshotMaterial, UpgradeMode, Verdict,
};
use zeroize::Zeroizing;

use super::membership::{DevRec, Fields, RecRec, SealMembership};

/// The identity file's magic (device.md §6.4.7).
const IDENTITY_MAGIC: &[u8; 4] = b"TDI1";
/// `DeviceKeys::to_identity_bytes` is 80 bytes (seal.md §26).
const KEYS_LEN: usize = 80;
/// At most 16 pending roots (device.md §6.4.7, `n_pending` 0 to 16).
const PENDING_MAX: usize = 16;
/// The code when a 17th rotation would be pending (decision f1-9).
const PENDING_FULL: &str = "device/pending_roots";
/// The code of identity bytes that are not device.md §6.4.7.
const IDENTITY_FORMAT: &str = "device/record_format";

fn se(e: seal::SealError) -> SealerError {
    SealerError { code: e.code() }
}

fn code(c: &'static str) -> SealerError {
    SealerError { code: c }
}

/// A copy of a root (`EpochRoot` is not `Clone`, seal.md §26).
fn copy_root(r: &EpochRoot) -> EpochRoot {
    EpochRoot::from_bytes(*r.to_bytes())
}

fn same_root(a: &EpochRoot, b: &EpochRoot) -> bool {
    bool::from(a.to_bytes().ct_eq(&b.to_bytes()[..]))
}

/// A control intent's items, and the `(epoch, root)` a rotation drew.
type Composed = (Vec<seal::CtlItem>, Option<(u32, EpochRoot)>);

/// This device's last sealing under one counter (SEAL-FRAME-005).
struct OwnSealed {
    entry: PlainEntry,
    epoch: u32,
    frame: Vec<u8>,
    frame_hash: Hash32,
}

/// What a creating device hands the sealer for seq 1 (device.md §18.1 steps
/// 3 and 4): the keys it drew and the genesis board's hash.
pub struct GenesisInput<'a> {
    /// The recovery identity, which signs seq 1 with `ctr = 1`.
    pub recovery: &'a RecoveryKeys,
    pub salt: [u8; 16],
    /// This device's keys; the sealer keeps them.
    pub keys: DeviceKeys,
    /// `R_1`; the sealer keeps it.
    pub root1: EpochRoot,
    pub device_name: &'a str,
    /// The genesis entry's `at_ms`.
    pub at_ms: i64,
    /// `SHA-256` of the genesis board's canonical bytes (`SM::board_hash`).
    pub board_hash: Hash32,
}

/// The sealer over `taskdeck-seal` (device.md §4.1, decision D6).
pub struct SealSealer {
    keys: DeviceKeys,
    dev: DevId,
    board: BoardId,
    /// Kept for the identity's life once present (decision f1-8).
    anchor: Option<GenesisAnchor>,
    roots: BTreeMap<u32, EpochRoot>,
    /// Own rotations not yet accepted: entry id → (epoch, root).
    pending: BTreeMap<[u8; 16], (u32, EpochRoot)>,
    /// The wraps of a `login` answer (ruling S24), by epoch; memory only.
    missed: BTreeMap<u32, [u8; 96]>,
    /// One history record per accepted control entry (device.md §6.4.8,
    /// §15.7), by seq, as `HistoryRecord::to_bytes`. Behind a `RefCell`
    /// because `open_snapshot` takes `&self` (engine.md §6.1).
    history: RefCell<BTreeMap<u64, Vec<u8>>>,
    sealed: BTreeMap<u64, OwnSealed>,
    supported_rules: Vec<u32>,
    /// `M_0` (seal.md §13.3), parsed once.
    zero: SealMembership,
}

/// A cursor over identity bytes; every read is bounds-checked.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], SealerError> {
        if self.0.len() < n {
            return Err(code(IDENTITY_FORMAT));
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn arr<const N: usize>(&mut self) -> Result<[u8; N], SealerError> {
        self.take(N)?.try_into().map_err(|_| code(IDENTITY_FORMAT))
    }
    fn u8(&mut self) -> Result<u8, SealerError> {
        Ok(self.arr::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, SealerError> {
        Ok(u16::from_be_bytes(self.arr()?))
    }
    fn u32(&mut self) -> Result<u32, SealerError> {
        Ok(u32::from_be_bytes(self.arr()?))
    }
    /// A root, through a zeroizing buffer.
    fn root(&mut self) -> Result<EpochRoot, SealerError> {
        let b = Zeroizing::new(self.arr::<32>()?);
        Ok(EpochRoot::from_bytes(*b))
    }
}

impl SealSealer {
    fn blank(keys: DeviceKeys, board: BoardId) -> Result<SealSealer, SealerError> {
        let dev = keys.public().dev;
        // `M_0`: the layout with every field zero; seal's parser accepts
        // exactly those bytes for it (seal.md §13.3).
        let zero = Fields::default().encode().ok_or(code("seal/membership_format"))?;
        let zero = SealMembership::new(Membership::from_canonical_bytes(&zero).map_err(se)?);
        Ok(SealSealer {
            keys,
            dev,
            board,
            anchor: None,
            roots: BTreeMap::new(),
            pending: BTreeMap::new(),
            missed: BTreeMap::new(),
            history: RefCell::new(BTreeMap::new()),
            sealed: BTreeMap::new(),
            supported_rules: taskdeck_core::apply::SUPPORTED_RULES.to_vec(),
            zero,
        })
    }

    /// The creating device's sealer and the genesis frame, seq 1
    /// (device.md §18.1 step 4; seal.md §12.2, §13.3): the `Genesis` item
    /// naming this device, epoch 1 with `R_1`'s commitment and its wraps to
    /// this device and `dev_R` (ascending), rules 1, hash version 1, the
    /// state hash of the genesis board and `M_1`, no snapshot (finding
    /// f1-5); signed by the recovery identity with `ctr = 1`, `based_on =
    /// (0, C_0)`, sealed under `K_entry,1`. Draws the entry id, the two
    /// wraps, then the nonce.
    pub fn create(g: GenesisInput<'_>) -> Result<(SealSealer, Vec<u8>), SealerError> {
        let rp = g.recovery.public();
        let board = seal::board_id(&rp.sign_pk, &g.salt);
        let dev_r = rp.dev(&board);
        let me = g.keys.public();
        let commit = g.root1.commitment(&board, 1);
        let mut rcpt = [(me.dev, me.hpke_pk), (dev_r, rp.hpke_pk)];
        rcpt.sort_by_key(|r| r.0);
        let entry_id = seal::new_entry_id(&mut OsEntropy).map_err(se)?;
        let mut wraps = Vec::with_capacity(2);
        for (id, pk) in rcpt {
            wraps.push(seal::wrap_root(&g.root1, &board, 1, &commit, id, &pk, &mut OsEntropy).map_err(se)?);
        }
        // `M_1` (seal.md §13.3), which the state hash covers, before the
        // frame exists: seal offers no way to compute it (finding f1-4), so
        // it is laid out here and seal's own parser judges it.
        let m1 = Fields {
            board_id: board.0,
            salt: g.salt,
            rules: 1,
            epoch: 1,
            epoch_seq: 1,
            epoch_clock_ms: g.at_ms,
            clock_ms: g.at_ms,
            home_gen: 1,
            next_device_no: 2,
            commitments: vec![(1, 1, commit)],
            recovery: vec![RecRec {
                dev: dev_r.0,
                sign_pk: rp.sign_pk,
                hpke_pk: rp.hpke_pk,
                since_seq: 1,
                until_seq: 0,
                last_ctr: 1,
            }],
            devices: vec![DevRec {
                dev: me.dev.0,
                sign_pk: me.sign_pk,
                hpke_pk: me.hpke_pk,
                device_no: 1,
                status: 1,
                added_seq: 1,
                added_by: dev_r.0,
                bootstrap_seq: 0,
                status_seq: 1,
                status_by: dev_r.0,
                last_ctr: 0,
                beat_ms: g.at_ms,
                rules: vec![1],
                envelopes: vec![(1, 1)],
                hash_versions: vec![1],
                name: g.device_name.to_owned(),
            }],
            ctl_seqs: vec![1],
            ..Fields::default()
        };
        let m1 = m1.encode().ok_or(code("seal/bad_payload"))?;
        Membership::from_canonical_bytes(&m1).map_err(se)?;
        let state_hash = taskdeck_sync::state_hash(1, &g.board_hash, &m1);
        let item = seal::CtlItem::Genesis(seal::Genesis {
            salt: g.salt,
            recovery: rp,
            device: seal::GenesisDevice {
                dev: me.dev,
                sign_pk: me.sign_pk,
                hpke_pk: me.hpke_pk,
                name: g.device_name.to_owned(),
            },
            epoch: 1,
            commit,
            wraps,
            rules: 1,
            hash_version: 1,
            state_hash,
            snapshot: None,
        });
        let signer = Signer::Recovery(g.recovery, board);
        let payload = seal::ctl_payload(&[item], &signer, &board, 1, &entry_id, g.at_ms).map_err(se)?;
        let plain = Plain {
            entry_id,
            kind: seal::Kind::Ctl,
            at_ms: g.at_ms,
            based_on: seal::Head { seq: 0, chain: seal::chain_zero(&board) },
            payload,
        };
        let key = seal::derive_epoch_key(&g.root1, &board, 1, KeyKind::Entry);
        let frame = seal::seal_frame(&signer, &board, 1, 1, &key, &plain, &mut OsEntropy).map_err(se)?;
        let mut s = SealSealer::blank(g.keys, board)?;
        s.roots.insert(1, copy_root(&g.root1));
        s.anchor = Some(GenesisAnchor { rsign_pk: rp.sign_pk, salt: g.salt, root1: g.root1 });
        Ok((s, frame))
    }

    /// The sealer of a later `open` (device.md §6.4.7): the board id (the
    /// identity bytes do not carry it; the store does, finding f1-7), the
    /// identity bytes `export_identity` gave, and the history records of
    /// `history.tdk` (kind 11, `HistoryRecord::to_bytes`, ascending seq).
    /// `device/record_format` for identity bytes that are not exactly the
    /// layout, roots not strictly ascending, pending roots not strictly
    /// ascending by entry id, an anchor of another board, or a history
    /// record that does not parse or repeats a seq.
    pub fn open(board_id: [u8; 16], identity: &[u8], history: &[Vec<u8>]) -> Result<SealSealer, SealerError> {
        let bad = || code(IDENTITY_FORMAT);
        let mut r = Cursor(identity);
        if r.take(4)? != IDENTITY_MAGIC || usize::from(r.u16()?) != KEYS_LEN {
            return Err(bad());
        }
        let keys = DeviceKeys::from_identity_bytes(r.take(KEYS_LEN)?).map_err(|_| bad())?;
        let board = BoardId(board_id);
        let mut s = SealSealer::blank(keys, board)?;
        for _ in 0..r.u16()? {
            let e = r.u32()?;
            let root = r.root()?;
            if s.roots.last_key_value().is_some_and(|(last, _)| *last >= e) {
                return Err(bad());
            }
            s.roots.insert(e, root);
        }
        let n = r.u8()?;
        if usize::from(n) > PENDING_MAX {
            return Err(bad());
        }
        for _ in 0..n {
            let id = r.arr::<16>()?;
            let e = r.u32()?;
            let root = r.root()?;
            if s.pending.last_key_value().is_some_and(|(last, _)| *last >= id) {
                return Err(bad());
            }
            s.pending.insert(id, (e, root));
        }
        match r.u8()? {
            0 => {}
            1 => {
                let rsign_pk = r.arr::<32>()?;
                let salt = r.arr::<16>()?;
                let root1 = r.root()?;
                if seal::board_id(&rsign_pk, &salt) != board {
                    return Err(bad());
                }
                s.anchor = Some(GenesisAnchor { rsign_pk, salt, root1 });
            }
            _ => return Err(bad()),
        }
        if !r.0.is_empty() {
            return Err(bad());
        }
        {
            let mut held = s.history.borrow_mut();
            for rec in history {
                let h = HistoryRecord::from_bytes(rec).map_err(|_| code(IDENTITY_FORMAT))?;
                if held.insert(h.seq(), rec.clone()).is_some() {
                    return Err(code(IDENTITY_FORMAT));
                }
            }
        }
        Ok(s)
    }

    /// The board id.
    pub fn board_id(&self) -> [u8; 16] {
        self.board.0
    }

    /// The history records this sealer holds, ascending by seq, each as
    /// `HistoryRecord::to_bytes` (device.md §6.4.8): every accepted control
    /// entry it classified, those it was opened with, and those of the
    /// snapshots it opened (§15.7). The runtime writes those above the last
    /// seq of `history.tdk`.
    pub fn history(&self) -> Vec<(u64, Vec<u8>)> {
        self.history.borrow().iter().map(|(s, b)| (*s, b.clone())).collect()
    }

    /// A root of `epoch` this sealer holds that matches the commitment `m`
    /// records (SEAL-KEY-013); `seal/missing_root` otherwise.
    fn checked_root(&self, m: &SealMembership, epoch: u32) -> Result<&EpochRoot, SealerError> {
        let root = self.roots.get(&epoch).ok_or(code("seal/missing_root"))?;
        let c = m.commitment(epoch).ok_or(code("seal/missing_root"))?;
        if bool::from(root.commitment(&self.board, epoch).ct_eq(&c)) {
            Ok(root)
        } else {
            Err(code("seal/missing_root"))
        }
    }

    /// The items of a control intent against `m` (engine.md §6.1 `seal`),
    /// and the root a rotation draws. For a rotation the root is drawn
    /// first, then one wrap per recipient of the working copy `W`
    /// (ENG-SEALER-007, ENG-SEALER-008).
    fn compose(&self, m: &SealMembership, intent: &CtlIntent) -> Result<Composed, SealerError> {
        let out = match intent {
            CtlIntent::Rotate { revoke, reinstate, purge_before } => {
                let f = m.fields().ok_or(code("seal/membership_format"))?;
                let n = f.epoch.checked_add(1).ok_or(code("seal/ctl_epoch"))?;
                let mut recipients: BTreeMap<[u8; 16], [u8; 32]> =
                    f.devices.iter().filter(|d| d.status == 1).map(|d| (d.dev, d.hpke_pk)).collect();
                let mut items = Vec::with_capacity(revoke.len() + reinstate.len() + 1);
                for d in revoke {
                    items.push(seal::CtlItem::DeviceRevoked(DevId(*d)));
                    recipients.remove(d);
                }
                for d in reinstate {
                    items.push(seal::CtlItem::DeviceReinstated(DevId(*d)));
                    if let Some(rec) = f.devices.iter().find(|x| x.dev == *d) {
                        recipients.insert(rec.dev, rec.hpke_pk);
                    }
                }
                if let Some(r) = f.recovery.iter().find(|r| r.until_seq == 0) {
                    recipients.insert(r.dev, r.hpke_pk);
                }
                let root = EpochRoot::generate(&mut OsEntropy).map_err(se)?;
                let commit = root.commitment(&self.board, n);
                let mut wraps = Vec::with_capacity(recipients.len());
                for (id, pk) in &recipients {
                    wraps.push(seal::wrap_root(&root, &self.board, n, &commit, DevId(*id), pk, &mut OsEntropy).map_err(se)?);
                }
                items.push(seal::CtlItem::EpochRotated(seal::EpochRotated {
                    epoch: n,
                    commit,
                    wraps,
                    purge_before: *purge_before,
                }));
                (items, Some((n, root)))
            }
            CtlIntent::AddDevice { dev, sign_pk, hpke_pk, name, transcript, bootstrap_seq } => (
                vec![seal::CtlItem::DeviceAdded(seal::DeviceAdded {
                    dev: DevId(*dev),
                    sign_pk: *sign_pk,
                    hpke_pk: *hpke_pk,
                    name: name.clone(),
                    transcript: *transcript,
                    bootstrap_seq: *bootstrap_seq,
                })],
                None,
            ),
            CtlIntent::Fence { host, door, home_gen } => {
                (vec![seal::CtlItem::Fence(seal::Fence { host: host.clone(), door: *door, home_gen: *home_gen })], None)
            }
            CtlIntent::ResetTo { seq, state_hash, hash_version } => (
                vec![seal::CtlItem::ResetTo(seal::ResetTo {
                    seq: *seq,
                    state_hash: *state_hash,
                    hash_version: *hash_version,
                })],
                None,
            ),
            CtlIntent::Skip { targets } => (
                targets
                    .iter()
                    .map(|(seq, fh)| seal::CtlItem::Skip(seal::SkipItem { seq: *seq, frame_hash: FrameHash(*fh) }))
                    .collect(),
                None,
            ),
            CtlIntent::RulesUpgrade { rules, mode } => (
                vec![seal::CtlItem::RulesUpgrade(seal::RulesUpgrade {
                    rules: *rules,
                    mode: match mode {
                        UpgradeMode::Normal => seal::RulesUpgradeMode::Normal,
                        UpgradeMode::Override => seal::RulesUpgradeMode::Override,
                    },
                })],
                None,
            ),
        };
        Ok(out)
    }

    /// Whether a rotation of `m`'s board wraps to this device.
    fn wraps_to_me(&self, r: &seal::EpochRotated) -> bool {
        r.wraps.iter().any(|w| w.get(..16) == Some(&self.dev.0[..]))
    }
}

fn kind_of(k: seal::Kind) -> Kind {
    match k {
        seal::Kind::Cmd => Kind::Cmd,
        seal::Kind::Ctl => Kind::Ctl,
        seal::Kind::Beat => Kind::Beat,
        seal::Kind::Snap => Kind::Snap,
    }
}

fn head_of(h: &seal::Head) -> Head {
    Head { seq: h.seq, chain: h.chain.0 }
}

/// The engine's item of an opened control item (engine.md §6.1).
fn item_of(i: &seal::CtlItem) -> CtlItem {
    match i {
        seal::CtlItem::Genesis(g) => CtlItem::Genesis { hash_version: g.hash_version, state_hash: g.state_hash },
        seal::CtlItem::DeviceAdded(d) => CtlItem::DeviceAdded {
            dev: d.dev.0,
            name: d.name.clone(),
            key_fp: seal::key_fingerprint(&d.sign_pk, &d.hpke_pk),
            bootstrap_seq: d.bootstrap_seq,
        },
        seal::CtlItem::DeviceRevoked(d) => CtlItem::DeviceRevoked { dev: d.0 },
        seal::CtlItem::DeviceReinstated(d) => CtlItem::DeviceReinstated { dev: d.0 },
        seal::CtlItem::EpochRotated(r) => CtlItem::EpochRotated { epoch: r.epoch, purge_before: r.purge_before },
        seal::CtlItem::RecoveryKeyChange(k) => {
            CtlItem::RecoveryKeyChange { key_fp: seal::key_fingerprint(&k.sign_pk, &k.hpke_pk) }
        }
        seal::CtlItem::Fence(f) => CtlItem::Fence { host: f.host.clone(), door: f.door, home_gen: f.home_gen },
        seal::CtlItem::ResetTo(r) => {
            CtlItem::ResetTo { seq: r.seq, state_hash: r.state_hash, hash_version: r.hash_version }
        }
        seal::CtlItem::Skip(s) => CtlItem::Skip { seq: s.seq, frame_hash: s.frame_hash.0 },
        seal::CtlItem::RulesUpgrade(u) => CtlItem::RulesUpgrade {
            rules: u.rules,
            mode: match u.mode {
                seal::RulesUpgradeMode::Normal => UpgradeMode::Normal,
                seal::RulesUpgradeMode::Override => UpgradeMode::Override,
            },
        },
    }
}

fn beat_of(b: &seal::Beat) -> BeatInfo {
    BeatInfo {
        rules: b.rules.clone(),
        envelopes: b.envelopes.clone(),
        hash_versions: b.hash_versions.clone(),
        state: b.state.as_ref().map(|s| (s.seq, s.hash, s.hash_version)),
        acks: b
            .acks
            .iter()
            .map(|a| {
                (a.entry, match a.verdict {
                    seal::AckVerdict::Ack => Verdict::Ack,
                    seal::AckVerdict::Mismatch => Verdict::Mismatch,
                })
            })
            .collect(),
    }
}

fn beat_to_seal(b: &BeatInfo) -> seal::Beat {
    seal::Beat {
        rules: b.rules.clone(),
        envelopes: b.envelopes.clone(),
        hash_versions: b.hash_versions.clone(),
        state: b.state.map(|(seq, hash, hash_version)| seal::BeatState { seq, hash, hash_version }),
        acks: b
            .acks
            .iter()
            .map(|(entry, v)| seal::BeatAck {
                entry: *entry,
                verdict: match v {
                    Verdict::Ack => seal::AckVerdict::Ack,
                    Verdict::Mismatch => seal::AckVerdict::Mismatch,
                },
            })
            .collect(),
    }
}

fn snap_of(s: &seal::SnapAnnounce) -> SnapInfo {
    SnapInfo {
        seq: s.seq,
        hash: s.hash,
        hash_version: s.hash_version,
        blob: s.blob.0,
        sha256: s.sha256,
        size: s.size,
        parts: s.parts,
        inflated: s.inflated,
        epoch: s.epoch,
    }
}

fn snap_to_seal(s: &SnapInfo) -> seal::SnapAnnounce {
    seal::SnapAnnounce {
        seq: s.seq,
        hash: s.hash,
        hash_version: s.hash_version,
        blob: BlobId(s.blob),
        sha256: s.sha256,
        size: s.size,
        parts: s.parts,
        inflated: s.inflated,
        epoch: s.epoch,
    }
}

fn opened_of(o: &seal::Opened) -> Opened {
    Opened {
        entry_id: o.entry_id.0,
        kind: kind_of(o.kind),
        at_ms: o.at_ms,
        based_on: head_of(&o.based_on),
        payload: o.payload.clone(),
        items: o.items.as_ref().map(|v| v.iter().map(item_of).collect()).unwrap_or_default(),
        beat: o.beat.as_ref().map(beat_of),
        snap: o.snap.as_ref().map(snap_of),
    }
}

/// The length of a snapshot body before compression (seal.md §20.4):
/// `seal::SealedBlob` does not report it (finding f1-10).
fn inflated_len(body: &SnapshotBody) -> u64 {
    let hist: u64 = body.history.iter().map(|r| r.to_bytes().len() as u64).sum();
    (4 + 8 + 32 + 4 + 32 + 4 + 4 + 4) as u64 + body.board.len() as u64 + body.membership.len() as u64 + hist
}

impl Sealer for SealSealer {
    type Membership = SealMembership;

    fn membership_zero(&self) -> SealMembership {
        self.zero.clone()
    }

    fn own_dev(&self) -> Dev {
        self.dev.0
    }

    fn view(&self, m: &SealMembership) -> MembershipView {
        m.view()
    }

    fn membership_bytes(&self, m: &SealMembership) -> Vec<u8> {
        m.bytes().to_vec()
    }

    fn membership_from_bytes(&self, bytes: &[u8]) -> Result<SealMembership, SealerError> {
        Membership::from_canonical_bytes(bytes).map(SealMembership::new).map_err(se)
    }

    fn supported_envelopes(&self) -> Vec<(u8, u8)> {
        vec![(1, 1)]
    }

    fn chain_zero(&self) -> Hash32 {
        seal::chain_zero(&self.board).0
    }

    fn chain_step(&self, prev: &Hash32, seq: u64, frame: &[u8]) -> Hash32 {
        seal::chain_step(&Chain(*prev), seq, frame).0
    }

    fn frame_hash(&self, frame: &[u8]) -> Hash32 {
        seal::frame_hash(frame).0
    }

    fn frame_author(&self, frame: &[u8]) -> Result<(Dev, u64), SealerError> {
        seal::frame_header(frame).map(|h| (h.dev.0, h.ctr)).map_err(se)
    }

    fn classify(&mut self, input: &ClassifyInput<'_, SealMembership>) -> Result<Receipt<SealMembership>, SealerError> {
        let header = seal::frame_header(input.frame).ok();
        let frame_hash = seal::frame_hash(input.frame).0;
        let r = {
            let roots = |e: u32| self.roots.get(&e);
            let chain_at = |b: u64| (input.chain_at)(b).map(Chain);
            let sealed = |c: u64| (input.own_sealed)(c).map(FrameHash);
            let skip = |s: u64| -> Option<&[u8]> { (input.skip_targets)(s) };
            let ri = ReceiptInput {
                seq: input.seq,
                frame: input.frame,
                prev_chain: Chain(input.prev_chain),
                membership: input.membership.seal(),
                roots: &roots,
                chain_at: &chain_at,
                retained_from: input.retained_from,
                own: Some(OwnIdentity { keys: &self.keys, sealed: &sealed }),
                supported_rules: &self.supported_rules,
                genesis: if input.seq == 1 { self.anchor.as_ref() } else { None },
                skip_targets: &skip,
            };
            seal::classify(&ri).map_err(se)?
        };
        let accepted = matches!(r.class, seal::Class::Accept);
        let mut halt = r.halt.as_ref().map(|h| match h {
            seal::Halt::Commitment => Halt::Commitment,
            seal::Halt::UnknownRules(v) => Halt::UnknownRules(*v),
        });
        let mut code_out = r.code;
        let membership = SealMembership::new(r.membership);
        let own_entry = header.is_some_and(|h| h.dev == self.dev);

        // Everything that changes what the sealer holds is computed first.
        let mut learn: Vec<(u32, EpochRoot)> = Vec::new();
        let mut drop_pending: Option<[u8; 16]> = None;
        let mut drop_missed: Vec<u32> = Vec::new();
        let mut record: Option<(u64, Vec<u8>)> = None;
        if accepted {
            if let Some((e, root)) = r.learned_root {
                if self.roots.get(&e).is_none_or(|held| !same_root(held, &root)) {
                    learn.push((e, root));
                }
                drop_missed.push(e);
            }
            if let Some(o) = &r.opened
                && o.kind == seal::Kind::Ctl
            {
                // ENG-SEALER-009: an own rotation's pending root goes now.
                if own_entry && self.pending.contains_key(&o.entry_id.0) {
                    drop_pending = Some(o.entry_id.0);
                }
                // A missed wrap (ruling S24): an accepted rotation that does
                // not wrap to this device has its missed wrap opened against
                // the rotation's commitment (engine.md §6.1 `classify`).
                for item in o.items.iter().flatten() {
                    let seal::CtlItem::EpochRotated(rot) = item else { continue };
                    if self.wraps_to_me(rot) || self.roots.contains_key(&rot.epoch) {
                        continue;
                    }
                    let Some(w) = self.missed.get(&rot.epoch) else { continue };
                    match seal::unwrap_root(w, &self.board, rot.epoch, &rot.commit, &Recipient::Device(&self.keys)) {
                        Ok(root) => {
                            learn.push((rot.epoch, root));
                            drop_missed.push(rot.epoch);
                        }
                        Err(_) => {
                            halt = Some(Halt::Commitment);
                            code_out = Some(seal::SealError::Commitment.code());
                        }
                    }
                }
                let rec = HistoryRecord::from_accepted(input.seq, input.frame, o).map_err(se)?;
                record = Some((input.seq, rec.to_bytes()));
            }
        }
        let opened = r.opened.as_ref().map(opened_of);
        let class = match r.class {
            seal::Class::Skip => Class::Skip,
            seal::Class::Tamper => Class::Tamper,
            seal::Class::Refuse => Class::Refuse,
            seal::Class::Accept => Class::Accept,
            seal::Class::NeedFrames(v) => Class::NeedFrames(v.into_iter().map(|(s, h)| (s, h.0)).collect()),
        };

        // Then applied, with nothing that can fail.
        let keys_changed = !learn.is_empty() || drop_pending.is_some();
        for (e, root) in learn {
            self.roots.insert(e, root);
        }
        if let Some(id) = drop_pending {
            self.pending.remove(&id);
        }
        for e in drop_missed {
            self.missed.remove(&e);
        }
        if let Some((s, bytes)) = record {
            self.history.borrow_mut().insert(s, bytes);
        }
        Ok(Receipt {
            class,
            code: code_out,
            chain: r.chain.0,
            author: header.map_or([0; 16], |h| h.dev.0),
            ctr: header.map_or(0, |h| h.ctr),
            frame_hash,
            own_dev_foreign: r.own_dev_foreign,
            based_on_unchecked: r.based_on_unchecked,
            opened,
            membership,
            keys_changed,
            halt,
        })
    }

    fn new_entry_id(&mut self) -> Result<[u8; 16], SealerError> {
        seal::new_entry_id(&mut OsEntropy).map(|e| e.0).map_err(se)
    }

    fn seal(&mut self, m: &SealMembership, ctr: u64, entry: &PlainEntry) -> Result<Sealed, SealerError> {
        let epoch = m.seal().epoch();
        if let Some(x) = self.sealed.get(&ctr).filter(|x| x.entry == *entry && x.epoch == epoch) {
            return Ok(Sealed { frame: x.frame.clone(), frame_hash: x.frame_hash, epoch, keys_changed: false });
        }
        if ctr == 0 {
            return Err(code("seal/frame_format"));
        }
        if matches!(entry.body, Body::Ctl(CtlIntent::Rotate { .. })) && self.pending.len() >= PENDING_MAX {
            return Err(code(PENDING_FULL));
        }
        let root = copy_root(self.checked_root(m, epoch)?);
        let (kind, payload, drawn) = match &entry.body {
            Body::Cmd(p) => (seal::Kind::Cmd, p.clone(), None),
            Body::Beat(b) => (seal::Kind::Beat, seal::beat_payload(&beat_to_seal(b)).map_err(se)?, None),
            Body::Snap(i) => (seal::Kind::Snap, seal::snap_payload(&snap_to_seal(i)).map_err(se)?, None),
            Body::Ctl(intent) => {
                let (items, drawn) = self.compose(m, intent)?;
                let signer = Signer::Device(&self.keys);
                let id = EntryId(entry.entry_id);
                let p = seal::ctl_payload(&items, &signer, &self.board, ctr, &id, entry.at_ms).map_err(se)?;
                (seal::Kind::Ctl, p, drawn)
            }
        };
        let plain = Plain {
            entry_id: EntryId(entry.entry_id),
            kind,
            at_ms: entry.at_ms,
            based_on: seal::Head { seq: entry.based_on.seq, chain: Chain(entry.based_on.chain) },
            payload,
        };
        let key = seal::derive_epoch_key(&root, &self.board, epoch, KeyKind::Entry);
        let frame =
            seal::seal_frame(&Signer::Device(&self.keys), &self.board, ctr, epoch, &key, &plain, &mut OsEntropy)
                .map_err(se)?;
        let frame_hash = seal::frame_hash(&frame).0;
        let keys_changed = drawn.is_some();
        if let Some(p) = drawn {
            self.pending.insert(entry.entry_id, p);
        }
        self.sealed.insert(ctr, OwnSealed { entry: entry.clone(), epoch, frame: frame.clone(), frame_hash });
        Ok(Sealed { frame, frame_hash, epoch, keys_changed })
    }

    fn open_own(&self, m: &SealMembership, frame: &[u8]) -> Result<[u8; 16], SealerError> {
        let h = seal::frame_header(frame).map_err(se)?;
        if h.dev != self.dev || h.board != self.board {
            return Err(code("seal/unknown_author"));
        }
        let fh = seal::frame_hash(frame).0;
        if let Some(x) = self.sealed.values().find(|x| x.frame_hash == fh) {
            return Ok(x.entry.entry_id);
        }
        // seal.md §26 has no opener for one frame outside `classify`
        // (finding f1-3): `classify` runs on a probe state, `m` with this
        // device current at `ctr − 1` and the frame's epoch as the current
        // one, so that steps 1 to 11 (signature under this device's key,
        // AEAD under a root it holds, unpadding) run, and the opened
        // `entry_id` is read whatever the later steps say.
        let epoch = frame.get(50..54).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes).unwrap_or(0);
        let mut f = m.fields().cloned().ok_or(code("seal/membership_format"))?;
        if epoch == 0 || epoch > f.epoch {
            return Err(code("seal/bad_epoch"));
        }
        let d = f.devices.iter_mut().find(|d| d.dev == self.dev.0).ok_or(code("seal/unknown_author"))?;
        d.status = 1;
        d.last_ctr = h.ctr.saturating_sub(1);
        f.commitments.retain(|c| c.0 <= epoch);
        f.epoch_seq = f.commitments.last().map_or(f.epoch_seq, |c| c.1);
        f.epoch = epoch;
        let probe = f.encode().ok_or(code("seal/membership_format"))?;
        let probe = Membership::from_canonical_bytes(&probe).map_err(se)?;
        let roots = |e: u32| self.roots.get(&e);
        let none_chain = |_: u64| None;
        let none_frame = |_: u64| -> Option<&[u8]> { None };
        let ri = ReceiptInput {
            seq: u64::MAX,
            frame,
            prev_chain: Chain([0; 32]),
            membership: &probe,
            roots: &roots,
            chain_at: &none_chain,
            retained_from: u64::MAX,
            own: None,
            supported_rules: &self.supported_rules,
            genesis: None,
            skip_targets: &none_frame,
        };
        let r = seal::classify(&ri).map_err(se)?;
        match r.opened {
            Some(o) => Ok(o.entry_id.0),
            None => Err(code(r.code.unwrap_or("seal/aead"))),
        }
    }

    fn discard_pending(&mut self, entry_id: &[u8; 16]) -> bool {
        self.pending.remove(entry_id).is_some()
    }

    fn forget_roots_below(&mut self, epoch: u32) -> bool {
        let kept = self.roots.split_off(&epoch);
        let mut changed = !self.roots.is_empty();
        self.roots = kept;
        // The anchor carries `R_1`; a purge past epoch 1 deletes it too
        // (decision f1-8).
        if epoch > 1 && self.anchor.take().is_some() {
            changed = true;
        }
        changed
    }

    fn take_missed_wraps(&mut self, m: &SealMembership, first_epoch: u32, wraps: &[[u8; 96]]) -> Result<bool, SealerError> {
        let mut missed = BTreeMap::new();
        let mut e = first_epoch;
        for w in wraps {
            missed.insert(e, *w);
            e = e.checked_add(1).ok_or(code("seal/bad_payload"))?;
        }
        // Oldest first, each once `m` records its commitment (device.md
        // §10.3, SEAL-WRAP-002, SEAL-KEY-013); all or nothing.
        let mut learned = Vec::new();
        for (e, w) in &missed {
            if self.roots.contains_key(e) {
                continue;
            }
            let Some(c) = m.commitment(*e) else { continue };
            let root = seal::unwrap_root(w, &self.board, *e, &c, &Recipient::Device(&self.keys))
                .map_err(|_| code(seal::SealError::Commitment.code()))?;
            learned.push((*e, root));
        }
        self.missed = missed;
        let any = !learned.is_empty();
        for (e, root) in learned {
            self.missed.remove(&e);
            self.roots.insert(e, root);
        }
        Ok(any)
    }

    fn seal_snapshot(&mut self, m: &SealMembership, material: &SnapshotMaterial) -> Result<SealedBlob, SealerError> {
        let at_s = Fields::parse(&material.membership).ok_or(code("seal/membership_format"))?;
        Membership::from_canonical_bytes(&material.membership).map_err(se)?;
        let history = {
            let held = self.history.borrow();
            let mut out = Vec::with_capacity(at_s.ctl_seqs.len());
            for s in &at_s.ctl_seqs {
                let b = held.get(s).ok_or(code("seal/blob_open"))?;
                out.push(HistoryRecord::from_bytes(b).map_err(se)?);
            }
            out
        };
        let epoch = m.seal().epoch();
        let root = self.checked_root(m, epoch)?;
        let key = seal::derive_epoch_key(root, &self.board, epoch, KeyKind::Snap);
        let body = SnapshotBody {
            seq: material.seq,
            chain: Chain(material.chain),
            hash_version: material.hash_version,
            state_hash: material.state_hash,
            board: material.board.clone(),
            membership: material.membership.clone(),
            history,
        };
        let inflated = inflated_len(&body);
        let b = seal::seal_snapshot(&body, &key, &self.board, epoch, &self.dev, &mut OsEntropy).map_err(se)?;
        Ok(SealedBlob { id: b.id.0, bytes: b.bytes, sha256: b.sha256, parts: b.parts, inflated, epoch })
    }

    fn open_snapshot(
        &self,
        parts: &[&[u8]],
        expected: &SnapInfo,
        writer: &Dev,
        exact: bool,
    ) -> Result<(SnapshotMaterial, u32, u64), SealerError> {
        let writer = DevId(*writer);
        let (body, epoch, inflated) = if exact {
            // Every root held was checked against its commitment when it
            // was learned (SEAL-KEY-013).
            let root = self.roots.get(&expected.epoch).ok_or(code("seal/missing_root"))?;
            let key = seal::derive_epoch_key(root, &self.board, expected.epoch, KeyKind::Snap);
            let bref = BlobRef {
                id: BlobId(expected.blob),
                sha256: expected.sha256,
                size: expected.size,
                parts: expected.parts,
                inflated: expected.inflated,
                epoch: expected.epoch,
            };
            let body = seal::open_snapshot(
                parts,
                &bref,
                expected.seq,
                &expected.hash,
                expected.hash_version,
                &writer,
                &key,
                &self.board,
            )
            .map_err(se)?;
            (body, expected.epoch, expected.inflated)
        } else {
            if u64::from(expected.parts) != expected.size.div_ceil(1_048_576) {
                return Err(code("seal/blob_size"));
            }
            let roots = |e: u32| self.roots.get(&e);
            seal::open_snapshot_declared(parts, &expected.sha256, expected.size, expected.seq, &writer, &roots, &self.board)
                .map_err(se)?
        };
        // The body's history, verified by seal (§20.5 step 7), kept for the
        // runtime (device.md §15.7).
        {
            let mut held = self.history.borrow_mut();
            for rec in &body.history {
                held.entry(rec.seq()).or_insert_with(|| rec.to_bytes());
            }
        }
        let material = SnapshotMaterial {
            seq: body.seq,
            chain: body.chain.0,
            hash_version: body.hash_version,
            state_hash: body.state_hash,
            board: body.board,
            membership: body.membership,
        };
        Ok((material, epoch, inflated))
    }

    fn sync_fingerprint(&self, head: &Head) -> String {
        seal::sync_fingerprint(&self.board, &seal::Head { seq: head.seq, chain: Chain(head.chain) }).to_string()
    }

    fn move_code(&self, fence_seq: u64, fence_hash: &Hash32) -> String {
        seal::move_code(&self.board, fence_seq, &FrameHash(*fence_hash))
    }

    fn check_move_code(&self, fence_seq: u64, fence_hash: &Hash32, typed: &str) -> Result<(), SealerError> {
        seal::check_move_code(&self.board, fence_seq, &FrameHash(*fence_hash), typed).map_err(se)
    }

    fn export_identity(&self) -> Zeroizing<Vec<u8>> {
        let keys = self.keys.to_identity_bytes();
        let mut o = Zeroizing::new(Vec::with_capacity(
            10 + keys.len() + 36 * self.roots.len() + 52 * self.pending.len() + 80,
        ));
        o.extend_from_slice(IDENTITY_MAGIC);
        // 80 bytes (seal.md §26), so the vb16 length and the counts fit.
        o.extend_from_slice(&(keys.len() as u16).to_be_bytes());
        o.extend_from_slice(&keys);
        o.extend_from_slice(&(self.roots.len().min(usize::from(u16::MAX)) as u16).to_be_bytes());
        for (e, root) in self.roots.iter().take(usize::from(u16::MAX)) {
            o.extend_from_slice(&e.to_be_bytes());
            o.extend_from_slice(&root.to_bytes()[..]);
        }
        o.push(self.pending.len().min(PENDING_MAX) as u8);
        for (id, (e, root)) in self.pending.iter().take(PENDING_MAX) {
            o.extend_from_slice(id);
            o.extend_from_slice(&e.to_be_bytes());
            o.extend_from_slice(&root.to_bytes()[..]);
        }
        match &self.anchor {
            None => o.push(0),
            Some(a) => {
                o.push(1);
                o.extend_from_slice(&a.rsign_pk);
                o.extend_from_slice(&a.salt);
                o.extend_from_slice(&a.root1.to_bytes()[..]);
            }
        }
        o
    }
}

#[cfg(test)]
mod tests;
