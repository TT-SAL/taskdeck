//! What the fuzz targets of `taskdeck-seal` share (task S7): the helpers of
//! the integration tests (`tests/common/`), included unchanged; the fixtures
//! the targets mutate, built once from fixed bytes (`FixedEntropy`); and the
//! reading of an input into the edits and events a target applies, so that
//! every target is a function of its input alone.
#![allow(dead_code, clippy::all)]

#[path = "../../tests/common/mod.rs"]
pub mod common;
#[path = "../../tests/common/board.rs"]
pub mod board;
#[path = "../../tests/common/backup.rs"]
pub mod backup;
#[path = "../../tests/common/blob.rs"]
pub mod blob;
#[path = "../../tests/common/pairing.rs"]
pub mod pairing;

use std::sync::OnceLock;

pub use board::{Opts, Sim, Spec, Who, replay};
use taskdeck_seal::pair::{ExistingEvent, NewEvent};
use taskdeck_seal::*;

/// Reads small values from the front of an input.
pub struct Input<'a> {
    data: &'a [u8],
}

impl<'a> Input<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Input { data }
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    /// The next byte, 0 past the end.
    pub fn byte(&mut self) -> u8 {
        match self.data.split_first() {
            Some((b, rest)) => {
                self.data = rest;
                *b
            }
            None => 0,
        }
    }
    /// The next 8 bytes as a big-endian u64, zeros past the end.
    pub fn u64(&mut self) -> u64 {
        (0..8).fold(0u64, |acc, _| (acc << 8) | u64::from(self.byte()))
    }
    /// The next `n` bytes, or fewer at the end.
    pub fn take(&mut self, n: usize) -> &'a [u8] {
        let (a, b) = self.data.split_at(n.min(self.data.len()));
        self.data = b;
        a
    }
    /// What is left, without consuming it.
    pub fn rest(&self) -> &'a [u8] {
        self.data
    }
}

/// `base` with up to 16 edits read from `edits`, 4 bytes each: a position
/// (u16, taken modulo the length), an operation and a value. The operations
/// set, flip, insert, remove, cut there, or raise four bytes to `0xFF` (a
/// length or count field at its extreme).
pub fn mutate(base: &[u8], edits: &[u8]) -> Vec<u8> {
    let mut b = base.to_vec();
    for e in edits.chunks(4).take(16) {
        let get = |i: usize| e.get(i).copied().unwrap_or(0);
        let x = get(3);
        if b.is_empty() {
            b.push(x);
            continue;
        }
        let i = usize::from(u16::from_be_bytes([get(0), get(1)])) % b.len();
        match get(2) % 6 {
            0 => b[i] = x,
            1 => b[i] ^= x | 1,
            2 => b.insert(i, x),
            3 => {
                b.remove(i);
            }
            4 => b.truncate(i),
            _ => {
                let end = (i + 4).min(b.len());
                b[i..end].iter_mut().for_each(|y| *y = 0xFF);
            }
        }
    }
    b
}

/// The input as text (invalid UTF-8 replaced).
pub fn text(data: &[u8]) -> String {
    String::from_utf8_lossy(data).into_owned()
}

/// Entropy that hands out `n` fixed bytes.
pub fn fixed(n: usize) -> FixedEntropy {
    FixedEntropy::new((0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(7)).collect())
}

/// A board with frames of each kind accepted or ready, its state, and a
/// payload of each kind (`cmd`, `ctl`, `beat`, `snap`) to mutate.
pub struct Frames {
    pub sim: Sim,
    pub frames: Vec<Vec<u8>>,
    pub membership: Vec<u8>,
    pub payloads: [Vec<u8>; 4],
}

pub fn frames() -> &'static Frames {
    static F: OnceLock<Frames> = OnceLock::new();
    F.get_or_init(|| {
        let mut s = Sim::new(41, 3);
        s.rotate(Who::D(0));
        let mut frames = vec![s.cmd(Who::D(1), &Spec::default()), s.beat(Who::D(2), vec![1], &Spec::default())];
        let items = s.rotate_items(vec![CtlItem::DeviceRevoked(s.dev(Who::D(2)))]);
        frames.push(s.ctl(Who::D(0), items, &Spec::default()));
        let k = s.new_recovery().public();
        let p = CtlItem::RecoveryKeyChange(RecoveryKeyChange {
            mode: RecoveryKeyMode::Propose,
            sign_pk: Some(k.sign_pk),
            hpke_pk: Some(k.hpke_pk),
        });
        frames.push(s.ctl(Who::D(1), vec![p], &Spec::default()));
        let membership = s.m.to_canonical_bytes();
        let skip = [CtlItem::Skip(SkipItem { seq: 1, frame_hash: FrameHash([2; 32]) })];
        let ctl = ctl_payload(&skip, &s.signer(Who::D(0)), &s.board, 9, &EntryId([5; 16]), s.now).unwrap();
        let beat = Beat { rules: vec![1], envelopes: vec![(1, 1)], hash_versions: vec![1], state: None, acks: vec![] };
        let snap = SnapAnnounce {
            seq: 0,
            hash: [0x55; 32],
            hash_version: 1,
            blob: BlobId([0x10; 16]),
            sha256: [0x66; 32],
            size: 1_500_000,
            parts: 2,
            inflated: 4_000_000,
            epoch: s.epoch(),
        };
        let payloads = [b"{\"op\":\"x\"}".to_vec(), ctl, beat_payload(&beat).unwrap(), snap_payload(&snap).unwrap()];
        Frames { sim: s, frames, membership, payloads }
    })
}

/// A Skip entry by device 0 of the `cmd` at `target`.
pub struct Skip {
    pub sim: Sim,
    pub skip: Vec<u8>,
    pub target: u64,
    pub target_frame: Vec<u8>,
}

pub fn skip() -> &'static Skip {
    static F: OnceLock<Skip> = OnceLock::new();
    F.get_or_init(|| {
        let mut s = Sim::new(47, 3);
        let cmd = s.cmd(Who::D(2), &Spec::default());
        s.accept(cmd.clone());
        let target = s.seq();
        let item = CtlItem::Skip(SkipItem { seq: target, frame_hash: frame_hash(&cmd) });
        let skip = s.ctl(Who::D(0), vec![item], &Spec::default());
        Skip { sim: s, skip, target, target_frame: cmd }
    })
}

/// A device, and a wrap of a root to it for epoch `epoch` of `common::BOARD`.
pub struct Wrap {
    pub device: DeviceKeys,
    pub epoch: u32,
    pub commit: [u8; 32],
    pub wrap: [u8; 96],
}

pub fn wrap() -> &'static Wrap {
    static F: OnceLock<Wrap> = OnceLock::new();
    F.get_or_init(|| {
        let device = DeviceKeys::generate(&mut fixed(64)).unwrap();
        let p = device.public();
        let root = EpochRoot::from_bytes([9; 32]);
        let epoch = 5;
        let commit = root.commitment(&common::BOARD, epoch);
        let wrap = wrap_root(&root, &common::BOARD, epoch, &commit, p.dev, &p.hpke_pk, &mut fixed(64)).unwrap();
        Wrap { device, epoch, commit, wrap }
    })
}

/// The sheet of a fixed seed.
pub fn sheet() -> &'static str {
    static F: OnceLock<String> = OnceLock::new();
    F.get_or_init(|| RecoverySeed::generate(&mut fixed(32)).unwrap().to_sheet().as_str().to_owned())
}

/// A sealed file of kind `Checkpoint` with three records.
pub struct Files {
    pub key: StorageKey,
    pub board: BoardId,
    pub dev: DevId,
    pub sealed: Vec<u8>,
}

pub fn files() -> &'static Files {
    static F: OnceLock<Files> = OnceLock::new();
    F.get_or_init(|| {
        let key = StorageKey::from_bytes([0x33; 32]);
        let (board, dev) = (BoardId([1; 16]), DevId([2; 16]));
        let mut rng = fixed(1024);
        let (mut fs, mut sealed) = FileSealer::create(&key, FileKind::Checkpoint, &board, &dev, &mut rng).unwrap();
        for p in [&b"one"[..], &[0u8; 300][..], b""] {
            sealed.extend(fs.seal_record(p, &mut rng).unwrap());
        }
        Files { key, board, dev, sealed }
    })
}

/// A nightly backup and a segment of one board, their payloads before
/// sealing, and the board's history records.
pub struct Backups {
    pub sim: Sim,
    pub backup: Vec<u8>,
    pub segment: Vec<u8>,
    pub payload: Vec<u8>,
    pub segment_payload: Vec<u8>,
    pub records: Vec<Vec<u8>>,
}

pub fn backups() -> &'static Backups {
    static F: OnceLock<Backups> = OnceLock::new();
    F.get_or_init(|| {
        use backup::*;
        let mut s = Sim::new(51, 2);
        s.rotate(Who::D(1));
        let bytes = s.rng.bytes(32);
        let backup = write(&s, 0, bytes);
        let payload = Payload::nightly(&s).bytes();
        let history = copy_history(&s.history);
        let membership = s.m.to_canonical_bytes();
        let first_seq = s.seq() + 1;
        let chain_before = s.head().chain;
        let rs = roots(&s);
        let mut sp = Payload::nightly(&s);
        let f = s.cmd(Who::D(0), &Spec::default());
        s.accept(f);
        let frames = s.frames[first_seq as usize - 1..].to_vec();
        sp.file_type = 2;
        sp.first_seq = first_seq;
        sp.chain_before = chain_before.0;
        sp.frames = frames.clone();
        let mut i = input(&s, &history, BackupContent::Segment { membership, first_seq, chain_before, frames });
        i.roots = rs;
        let bytes = s.rng.bytes(32);
        let segment = write_backup(&s.devs[0], i, &mut *board::source(bytes)).unwrap();
        let records = s.history.iter().map(|r| r.to_bytes()).collect();
        Backups { sim: s, backup, segment, payload, segment_payload: sp.bytes(), records }
    })
}

/// A sealed snapshot and a sealed blob of kind 2 at epoch 2, the snapshot's
/// body, its DEFLATE and its state hash.
pub struct Blobs {
    pub sim: Sim,
    pub writer: DevId,
    pub snapshot: Vec<u8>,
    pub blob: Vec<u8>,
    pub body: Vec<u8>,
    pub compressed: Vec<u8>,
    pub hash: [u8; 32],
}

pub fn blobs() -> &'static Blobs {
    static F: OnceLock<Blobs> = OnceLock::new();
    F.get_or_init(|| {
        use blob::*;
        let mut s = Sim::new(52, 2);
        s.rotate(Who::D(1));
        let writer = s.devs[0].public().dev;
        let b = snap_body(&s);
        let snapshot = seal_snapshot(&b, &snap_key(&s), &s.board, 2, &writer, &mut fixed(64)).unwrap().bytes;
        let blob = seal_blob(&[7u8; 900], &blob_key(&s), &s.board, 2, &writer, &mut fixed(64)).unwrap().bytes;
        let body = body_bytes(&b);
        let compressed = backup::deflate(&body);
        Blobs { sim: s, writer, snapshot, blob, body, compressed, hash: b.state_hash }
    })
}

/// One event for N read from the input; a slot is one of the run's, mutated
/// and re-MACed under the run's code when the input asks.
pub fn n_event(i: &mut Input<'_>, r: &pairing::Run) -> NewEvent {
    let k = i.byte();
    let n = usize::from(i.byte());
    let x = i.u64();
    let bytes = i.take(n).to_vec();
    match k % 9 {
        0 => NewEvent::CodeEntered {
            code: if x % 2 == 0 { r.code.clone() } else { text(&bytes) },
            host: if x % 3 == 0 { text(&bytes) } else { pairing::HOST.into() },
            board_id: r.sim.board,
            door: pairing::DOOR,
            name: pairing::NAME.into(),
            now: x,
        },
        1 => NewEvent::Slot { bytes },
        2 => {
            let base = [&r.slot_a, &r.slot_c, &r.slot_d][usize::from(k / 9 % 3)];
            let m = mutate(base, &bytes);
            let m = if x % 2 == 1 && m.len() >= 36 { r.k.slot(m[0], m[1], &m[4..m.len() - 32]) } else { m };
            NewEvent::Slot { bytes: m }
        }
        3 => NewEvent::SasEntered { digits: text(&bytes) },
        4 => NewEvent::Snapshot { parts: bytes.chunks(97).map(|c| c.to_vec()).collect() },
        5 => NewEvent::Entries { frames: vec![(x, bytes)] },
        6 => NewEvent::Frames { frames: vec![(x, bytes)] },
        7 => NewEvent::Cancel,
        _ => NewEvent::Tick { now: x },
    }
}

/// One event for E read from the input; slot B is N's, mutated and
/// re-MACed when the input asks.
pub fn e_event(i: &mut Input<'_>, r: &pairing::Run) -> ExistingEvent {
    let k = i.byte();
    let n = usize::from(i.byte());
    let x = i.u64();
    let bytes = i.take(n).to_vec();
    match k % 7 {
        0 => ExistingEvent::SlotB { bytes },
        1 => {
            let m = mutate(&r.slot_b, &bytes);
            let m = if x % 2 == 1 && m.len() >= 36 { r.k.slot(m[0], m[1], &m[4..m.len() - 32]) } else { m };
            ExistingEvent::SlotB { bytes: m }
        }
        2 => ExistingEvent::UserConfirmed,
        3 => ExistingEvent::Registered {
            h: x % 1000,
            chain: Chain([1; 32]),
            snapshot: SnapAnnounce {
                seq: x % 1000,
                hash: [2; 32],
                hash_version: (x % 3) as u32,
                blob: BlobId([3; 16]),
                sha256: [4; 32],
                size: x % 5_000_000,
                parts: (x % 7) as u32,
                inflated: x % 100,
                epoch: (x % 4) as u32,
            },
            added_seq: x % 1003,
            token: text(&bytes),
            roots: (0..(k % 4)).map(|e| (u32::from(e), EpochRoot::from_bytes([e; 32]))).collect(),
            host: if x % 2 == 0 { pairing::HOST.into() } else { text(&bytes) },
            door: [0; 5],
        },
        4 => ExistingEvent::RegisterFailed { code: SealError::PairRaced },
        5 => ExistingEvent::Cancel,
        _ => ExistingEvent::Tick { now: x },
    }
}
