//! What the fuzz targets of `taskdeck-seal` share (task S7): the helpers of
//! the integration tests (`tests/common/`), included unchanged; the fixtures
//! the targets mutate, built once from fixed bytes (`FixedEntropy`); and the
//! reading of an input into the edits and events a target applies, so that
//! every target is a function of its input alone; and (task S9) the work of
//! the targets `open_blob`, `open_snapshot`, `open_snapshot_declared`,
//! `pair_new` and `pair_bundle` as functions that return how far an input
//! got, which the target drops and the test target `fuzz_targets` asserts.
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
use taskdeck_seal::pair::{ExistingEvent, New, NewEffect, NewEvent};
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
        // §11.5: `seq` is a positive multiple of 64 (the payload builder
        // refuses anything else).
        let snap = SnapAnnounce {
            seq: 64,
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
        // `generate` draws 80 bytes: the id (16), the signing key (32) and
        // the HPKE key material (32).
        let device = DeviceKeys::generate(&mut fixed(80)).unwrap();
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

// ------------------------------------------------------------ the openers

/// How far an opener got (task S9): its result; whether the sealed bytes it
/// was given open under the AEAD with the key of epoch 2 and the AD of §20.2
/// for the `S` and writer of the call (checked here with the pinned crates,
/// apart from the crate), so that a `seal/blob_open` after the AEAD can be
/// told from the AEAD's own; and what the harness shaped and sealed: the
/// padded body of a blob, or a snapshot body before compression, or the
/// padded stream of a snapshot where the input chose it.
pub struct Reach {
    pub result: Result<(), SealError>,
    pub aead_opens: bool,
    pub shaped: Option<Vec<u8>>,
}

/// Whether `sealed` opens under the AEAD with `key` and the AD of §20.2 for
/// its own header, `s` and `writer`.
pub fn aead_opens(sealed: &[u8], key: &[u8; 32], board: &BoardId, s: u64, writer: &DevId) -> bool {
    let Some(header) = sealed.get(..27) else { return false };
    let ad = [b"taskdeck/v1/blob\0".as_slice(), &board.0, header, &s.to_be_bytes(), &writer.0].concat();
    sealed.len() >= 51 && blob::open_raw(sealed, key, &ad).is_some()
}

/// The parts of `bytes`: as `split_parts` cuts them, or (when `at` is given)
/// in two at `at`.
fn cut(bytes: &[u8], at: Option<usize>) -> Vec<&[u8]> {
    match at {
        Some(at) => {
            let at = at.min(bytes.len());
            vec![&bytes[..at], &bytes[at..]]
        }
        None => split_parts(bytes),
    }
}

/// The work of the fuzz target `open_blob` (§20.5 steps 1 to 5). The first
/// byte chooses the mode:
/// - `% 3 == 0`: sealed bytes, the fixture's blob mutated by the rest (or,
///   with bit 2, the rest itself), with their own hash, so that steps 1 and 2
///   pass and the header and the AEAD (steps 3 and 4) judge them; the next
///   byte is a split point, used with bit 3 (two parts);
/// - `% 3 == 1`: the fixture's padded body (§20.3) mutated by the rest,
///   sealed properly here: the padding (step 5) judges it;
/// - `% 3 == 2`: the rest as the padded body, sealed properly here.
pub fn reach_open_blob(data: &[u8]) -> Reach {
    let b = blobs();
    let s = &b.sim;
    let key = blob::kind_key(&s.board, &s.roots[&2], 2, "blob");
    let mut i = Input::new(data);
    let mode = i.byte();
    let (bytes, at, shaped) = match mode % 3 {
        0 => {
            let at = usize::from(i.byte());
            let bytes = if mode & 4 == 4 { i.rest().to_vec() } else { mutate(&b.blob, i.rest()) };
            (bytes, (mode & 8 == 8).then_some(at), None)
        }
        m => {
            let padded = if m == 1 { mutate(&blob::pad(&BLOB_BODY), i.rest()) } else { i.rest().to_vec() };
            let raw = blob::Raw::new(2, 2, s.board, 0, b.writer, [3; 16], [4; 24]);
            (raw.seal(&key, &padded), None, Some(padded))
        }
    };
    let r = BlobRef { sha256: backup::sha(&[&bytes]), ..blob::blob_ref(&bytes, 0, 2) };
    let result = open_blob(&cut(&bytes, at), &r, &b.writer, &blob::blob_key(s), &s.board).map(|_| ());
    Reach { result, aead_opens: aead_opens(&bytes, &key, &s.board, 0, &b.writer), shaped }
}

/// The body of the fixture's blob of kind 2.
pub const BLOB_BODY: [u8; 900] = [7u8; 900];

/// The fields of a snapshot body (§20.4) as far as its lengths frame them;
/// `history` is everything after the membership (the count and records).
pub struct Fields<'a> {
    pub seq: u64,
    pub hash_version: u32,
    pub board: &'a [u8],
    pub membership: &'a [u8],
    pub history: &'a [u8],
}

/// The fields of `b`, `None` when its lengths do not frame a board and a
/// membership. The magic is not read.
pub fn fields(b: &[u8]) -> Option<Fields<'_>> {
    let u32_at = |at: usize| -> Option<usize> {
        usize::try_from(u32::from_be_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?)).ok()
    };
    let seq = u64::from_be_bytes(b.get(4..12)?.try_into().ok()?);
    let hash_version = u32::from_be_bytes(b.get(44..48)?.try_into().ok()?);
    let board_end = 84usize.checked_add(u32_at(80)?)?;
    let board = b.get(84..board_end)?;
    let m_start = board_end.checked_add(4)?;
    let m_end = m_start.checked_add(u32_at(board_end)?)?;
    let membership = b.get(m_start..m_end)?;
    let history = b.get(m_end..)?;
    Some(Fields { seq, hash_version, board, membership, history })
}

/// A snapshot body of §20.4 from its parts.
pub fn assemble(seq: u64, chain: &[u8], hash_version: u32, hash: &[u8; 32], board: &[u8], membership: &[u8], history: &[u8]) -> Vec<u8> {
    let mut o = b"TDSN".to_vec();
    o.extend(seq.to_be_bytes());
    o.extend(chain);
    o.extend(hash_version.to_be_bytes());
    o.extend(hash);
    o.extend((board.len() as u32).to_be_bytes());
    o.extend(board);
    o.extend((membership.len() as u32).to_be_bytes());
    o.extend(membership);
    o.extend(history);
    o
}

/// Sealed snapshot bytes shaped from an input, and what a reader is to
/// expect of them.
pub struct SnapCase {
    pub sealed: Vec<u8>,
    /// Where to cut the parts (`None`: as `split_parts` cuts them).
    pub at: Option<usize>,
    pub s: u64,
    pub inflated: u64,
    /// The size a declared reader is given.
    pub size: u64,
    pub hash: [u8; 32],
    pub hash_version: u32,
    /// The body before compression, where the harness shaped one.
    pub body: Option<Vec<u8>>,
    /// The padded stream (§20.3) sealed as it is, where the input chose it.
    pub padded: Option<Vec<u8>>,
}

/// The key of the fixture's snapshots (epoch 2), computed here.
pub fn snap_key_bytes() -> [u8; 32] {
    let s = &blobs().sim;
    blob::kind_key(&s.board, &s.roots[&2], 2, "snap")
}

/// Sealed bytes of kind 1 at epoch 2 around `compressed`, for `S = seq`.
fn seal_snap(seq: u64, compressed: &[u8]) -> Vec<u8> {
    seal_snap_padded(seq, &blob::pad(compressed))
}

/// Sealed bytes of kind 1 at epoch 2 with `padded` as the plaintext, as it
/// is (no padding added), for `S = seq`.
fn seal_snap_padded(seq: u64, padded: &[u8]) -> Vec<u8> {
    let b = blobs();
    let raw = blob::Raw::new(1, 2, b.sim.board, seq, b.writer, [3; 16], [4; 24]);
    raw.seal(&snap_key_bytes(), padded)
}

/// The padded stream of §20.3 around the fixture's compressed body.
pub fn padded_snapshot_stream() -> Vec<u8> {
    blob::pad(&blobs().compressed)
}

/// The snapshot an input shapes, for `open_snapshot` and
/// `open_snapshot_declared`. The first byte chooses the mode:
/// - `% 4 == 0`: sealed bytes, the fixture's snapshot mutated by the rest
///   (with bit 2, the rest itself), with their own hash: steps 1 to 4; the
///   next byte is a split point (used with bit 3), the next 8 the expected
///   `inflated` (or the declared size);
/// - `% 4 == 1`: the fixture's compressed body mutated by the rest, padded
///   and sealed properly here: the inflate (step 6) and, when it inflates,
///   the body; the next byte gives `inflated` as the body's length − 1, + 0,
///   or + 1. With bit 2, the fixture's padded stream (§20.3) mutated by the
///   rest, and with bit 3 (not bit 2) the rest itself, is sealed properly
///   here as it is, with no padding added: the padding (step 5) judges it;
/// - `% 4 == 2`: the fixture's body mutated by the rest, compressed and
///   sealed here, with `inflated` its length; when its lengths still frame a
///   board and a membership (and the next byte is even), its `state_hash` is
///   rewritten to the hash of its own board and membership under its own
///   `hash_version`, and `S`, `hash_version` and `state_hash` are expected
///   as it states them, so that the parsers behind the comparison run;
/// - `% 4 == 3`: one field of the fixture's body (by the next byte: the
///   board, the membership, or the history with its count) mutated by the
///   rest, the body laid out again around it with its lengths and its state
///   hash: the membership parser, the projection and the agreement check.
pub fn snap_case(i: &mut Input<'_>) -> SnapCase {
    let b = blobs();
    let s = &b.sim;
    let mode = i.byte();
    let honest = |sealed: Vec<u8>, inflated: u64, body: Option<Vec<u8>>| SnapCase {
        size: sealed.len() as u64,
        sealed,
        at: None,
        s: s.seq(),
        inflated,
        hash: b.hash,
        hash_version: 1,
        body,
        padded: None,
    };
    match mode % 4 {
        0 => {
            let at = usize::from(i.byte());
            let inflated = i.u64() % (1 << 27);
            let sealed = if mode & 4 == 4 { i.rest().to_vec() } else { mutate(&b.snapshot, i.rest()) };
            let size = inflated.max(sealed.len() as u64);
            SnapCase { at: (mode & 8 == 8).then_some(at), size, ..honest(sealed, inflated, None) }
        }
        1 => {
            let slack = u64::from(i.byte() % 3);
            let inflated = (b.body.len() as u64).saturating_sub(1) + slack;
            let padded = match mode & 12 {
                0 => return honest(seal_snap(s.seq(), &mutate(&b.compressed, i.rest())), inflated, None),
                4 | 12 => mutate(&padded_snapshot_stream(), i.rest()),
                _ => i.rest().to_vec(),
            };
            let sealed = seal_snap_padded(s.seq(), &padded);
            SnapCase { padded: Some(padded), ..honest(sealed, inflated, None) }
        }
        2 => {
            let fix = i.byte() % 2 == 0;
            let mut body = mutate(&b.body, i.rest());
            let own = fields(&body).filter(|_| fix).map(|f| {
                (f.seq, f.hash_version, state_hash(f.hash_version, &backup::sha(&[f.board]), f.membership))
            });
            let sealed;
            match own {
                Some((seq, hv, hash)) => {
                    body[48..80].copy_from_slice(&hash);
                    sealed = seal_snap(seq, &backup::deflate(&body));
                    let c = honest(sealed, body.len() as u64, None);
                    SnapCase { s: seq, hash, hash_version: hv, body: Some(body), ..c }
                }
                None => {
                    sealed = seal_snap(s.seq(), &backup::deflate(&body));
                    honest(sealed, body.len() as u64, Some(body))
                }
            }
        }
        _ => {
            let f = fields(&b.body).expect("the fixture's body frames");
            let (mut board, mut membership, mut history) = (f.board.to_vec(), f.membership.to_vec(), f.history.to_vec());
            match i.byte() % 3 {
                0 => board = mutate(&board, i.rest()),
                1 => membership = mutate(&membership, i.rest()),
                _ => history = mutate(&history, i.rest()),
            }
            let hash = state_hash(1, &backup::sha(&[&board]), &membership);
            let body = assemble(s.seq(), &b.body[12..44], 1, &hash, &board, &membership, &history);
            let sealed = seal_snap(s.seq(), &backup::deflate(&body));
            SnapCase { hash, ..honest(sealed, body.len() as u64, Some(body)) }
        }
    }
}

/// The work of the fuzz target `open_snapshot` (§20.5, every step): the
/// snapshot `snap_case` shapes, opened against what it says to expect.
pub fn reach_open_snapshot(data: &[u8]) -> Reach {
    let b = blobs();
    let s = &b.sim;
    let c = snap_case(&mut Input::new(data));
    let r = BlobRef { sha256: backup::sha(&[&c.sealed]), ..blob::blob_ref(&c.sealed, c.inflated, 2) };
    let result = open_snapshot(&cut(&c.sealed, c.at), &r, c.s, &c.hash, c.hash_version, &b.writer, &blob::snap_key(s), &s.board);
    Reach {
        result: result.map(|_| ()),
        aead_opens: aead_opens(&c.sealed, &snap_key_bytes(), &s.board, c.s, &b.writer),
        shaped: c.body.or(c.padded),
    }
}

/// The work of the fuzz target `open_snapshot_declared` (§20.5, the declared
/// mode): the snapshot `snap_case` shapes, sealed under the key of the
/// declared root, opened with its own hash and size and the `S` it states;
/// no hash is compared, so a mutated membership and history reach their
/// parsers directly.
pub fn reach_open_snapshot_declared(data: &[u8]) -> Reach {
    let b = blobs();
    let s = &b.sim;
    let root = EpochRoot::from_bytes(s.roots[&2]);
    let roots = |e: u32| if e == 2 { Some(&root) } else { None };
    let c = snap_case(&mut Input::new(data));
    let sha = backup::sha(&[&c.sealed]);
    let result = open_snapshot_declared(&cut(&c.sealed, c.at), &sha, c.size, c.s, &b.writer, &roots, &s.board);
    Reach {
        result: result.map(|_| ()),
        aead_opens: aead_opens(&c.sealed, &snap_key_bytes(), &s.board, c.s, &b.writer),
        shaped: c.body.or(c.padded),
    }
}

// ------------------------------------------------------------ pairing

/// A whole honest pairing (§17.10) on a board of two devices, both machines
/// driven through the public API with fixed entropy to E's `Done` and N's
/// `Joined`, with a `Skip` after `h` of the entry at `h`, so that N asks for
/// that frame (`FetchFrames`). What N was given at each step, N's draws (a
/// fresh N given them posts the same slot B, so the run's slots C and D
/// verify for it), and the bundle with its key and AD.
pub struct Pairing {
    pub code: String,
    pub board: BoardId,
    pub k: pairing::Code,
    pub n_draws: Vec<u8>,
    pub slot_a: Vec<u8>,
    pub slot_c: Vec<u8>,
    pub slot_d: Vec<u8>,
    pub digits: String,
    pub snapshot: Vec<u8>,
    pub log: Vec<(u64, Vec<u8>)>,
    pub frames: Vec<(u64, Vec<u8>)>,
    pub bundle: Vec<u8>,
    pub bundle_key: [u8; 32],
    pub bundle_ad: Vec<u8>,
}

pub fn pairing_run() -> &'static Pairing {
    static F: OnceLock<Pairing> = OnceLock::new();
    F.get_or_init(|| {
        use pairing::*;
        let names = |fx: &[NewEffect]| fx.iter().map(n_name).collect::<Vec<_>>();
        let mut sim = Sim::new(721, 2);
        let f = sim.cmd(Who::D(1), &Spec::default());
        sim.accept(f);
        // E: `s`, `esk_E`, `r_E`, the nonce of slot D; N: §17.10 "Draws".
        let e_draws = sim.rng.bytes(74 + 24);
        let n_draws = sim.rng.bytes(112);
        let mut r = Run::with(sim, 0, board::source(e_draws), board::source(n_draws.clone()));
        r.step_b().step_c().step_sas().step_confirmed().take_snapshot();
        let t = r.snap.as_ref().unwrap().h;
        let fh = frame_hash(&r.sim.frames[t as usize - 1]);
        let f = r.sim.ctl(Who::D(0), vec![CtlItem::Skip(SkipItem { seq: t, frame_hash: fh })], &Spec::default());
        let rc = r.sim.classify(&f, &Opts { targets: vec![t], ..Opts::default() }).unwrap();
        assert!(matches!(rc.class, Class::Accept), "{:?}", rc.code);
        r.sim.commit(f, rc);
        let rc = r.append_added(|_| {}, &Spec::default());
        assert!(matches!(rc.class, Class::Accept), "{:?}", rc.code);
        let ev = r.registered();
        r.post_d(ev);
        r.step_joining();
        let log = r.log();
        let fx = r.send_log();
        assert_eq!(names(&fx), ["FetchFrames"]);
        let frames = vec![(t, r.sim.frames[t as usize - 1].clone())];
        let fx = r.n_on(NewEvent::Frames { frames: frames.clone() });
        assert_eq!(names(&fx), ["Joined"], "{:?}", n_abort(&fx));
        // A fresh N with N's draws posts the run's slot B.
        let mut n = New::new();
        let mut rng = FixedEntropy::new(n_draws.clone());
        n.on(r.code_event(), &mut rng);
        let fx = n.on(NewEvent::Slot { bytes: r.slot_a.clone() }, &mut rng);
        assert!(matches!(&fx[0], NewEffect::PostSlot { slot } if *slot == r.slot_b));
        // The bundle, as N sees it: `esk_N` is N's fourth draw.
        let esk: [u8; 32] = n_draws[80..112].try_into().unwrap();
        let epk_e: [u8; 32] = body(&r.slot_c)[..32].try_into().unwrap();
        let dh = x(esk, epk_e);
        let tr = transcript(&r.sim.board, &r.k.tid, &r.dev_e(), &r.slot_a, &r.slot_b, &r.slot_c);
        let bundle_key = pairing::bundle_key(&r.k.s, &dh, &tr);
        let bundle_ad = pairing::bundle_ad(&r.sim.board, &r.k.tid, &tr);
        let bundle = aead_open(&bundle_key, &bundle_ad, body(&r.slot_d)).expect("slot D opens under the bundle key");
        Pairing {
            code: r.code.clone(),
            board: r.sim.board,
            k: r.k.clone(),
            n_draws,
            slot_a: r.slot_a.clone(),
            slot_c: r.slot_c.clone(),
            slot_d: r.slot_d.clone(),
            digits: r.digits.clone(),
            snapshot: r.snap.as_ref().unwrap().bytes.clone(),
            log,
            frames,
            bundle,
            bundle_key,
            bundle_ad,
        }
    })
}

/// Where N stands in the honest run, as its effects show it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Code,
    A,
    C,
    Sas,
    D,
    Snapshot,
    Log,
    Frames,
    Over,
}

pub const PHASES: [Phase; 9] =
    [Phase::Code, Phase::A, Phase::C, Phase::Sas, Phase::D, Phase::Snapshot, Phase::Log, Phase::Frames, Phase::Over];

impl Phase {
    /// The phase after N emitted `fx`.
    pub fn after(self, fx: &[NewEffect]) -> Phase {
        fx.iter().fold(self, |p, f| match f {
            NewEffect::FetchSlot { label, .. } if *label == pairing::A => Phase::A,
            NewEffect::FetchSlot { label, .. } if *label == pairing::C => Phase::C,
            NewEffect::FetchSlot { .. } => Phase::D,
            NewEffect::AskAgain { .. } => Phase::Code,
            NewEffect::AskSas => Phase::Sas,
            NewEffect::FetchSnapshot { .. } => Phase::Snapshot,
            NewEffect::ReadLog { .. } => Phase::Log,
            NewEffect::FetchFrames { .. } => Phase::Frames,
            NewEffect::Joined { .. } | NewEffect::Abort { .. } => Phase::Over,
            NewEffect::PostSlot { .. } | NewEffect::ShowAccepted => p,
        })
    }
}

/// The honest event of the run for `phase` (a tick once it is over).
pub fn honest(p: &Pairing, phase: Phase) -> NewEvent {
    match phase {
        Phase::Code => NewEvent::CodeEntered {
            code: p.code.clone(),
            host: pairing::HOST.into(),
            board_id: p.board,
            door: pairing::DOOR,
            name: pairing::NAME.into(),
            now: pairing::PT0 + 1,
        },
        Phase::A => NewEvent::Slot { bytes: p.slot_a.clone() },
        Phase::C => NewEvent::Slot { bytes: p.slot_c.clone() },
        Phase::Sas => NewEvent::SasEntered { digits: p.digits.clone() },
        Phase::D => NewEvent::Slot { bytes: p.slot_d.clone() },
        Phase::Snapshot => NewEvent::Snapshot { parts: split_parts(&p.snapshot).into_iter().map(<[u8]>::to_vec).collect() },
        Phase::Log => NewEvent::Entries { frames: p.log.clone() },
        Phase::Frames => NewEvent::Frames { frames: p.frames.clone() },
        Phase::Over => NewEvent::Tick { now: pairing::PT0 + 2 },
    }
}

/// `slot` with its MAC made again under the run's code over its own label,
/// role and body, when it frames a MAC and its body can be laid out.
fn remac(p: &Pairing, slot: Vec<u8>) -> Vec<u8> {
    if slot.len() < 36 {
        return slot;
    }
    p.k.try_slot(slot[0], slot[1], &slot[4..slot.len() - 32]).unwrap_or(slot)
}

/// The honest event for `phase` mutated by `edits`. `flags` bit 0: a slot's
/// MAC is made again under the run's code; bit 1: slot D carries the bundle
/// JSON mutated and sealed again under the bundle key (then MACed), an
/// entry or frame gets the next seq; bits 2 and up pick the entry or frame.
fn mutated(p: &Pairing, phase: Phase, flags: u8, edits: &[u8]) -> NewEvent {
    let slot = |s: &[u8]| {
        let m = mutate(s, edits);
        if flags & 1 == 1 { remac(p, m) } else { m }
    };
    let one = |list: &[(u64, Vec<u8>)]| {
        let mut list = list.to_vec();
        let j = usize::from(flags >> 2) % list.len().max(1);
        if let Some(e) = list.get_mut(j) {
            if flags & 2 == 2 {
                e.0 = e.0.wrapping_add(1);
            }
            e.1 = mutate(&e.1, edits);
        }
        list
    };
    match phase {
        Phase::Code => NewEvent::CodeEntered {
            code: text(&mutate(p.code.as_bytes(), edits)),
            host: if flags & 2 == 2 { text(&mutate(pairing::HOST.as_bytes(), edits)) } else { pairing::HOST.into() },
            board_id: p.board,
            door: pairing::DOOR,
            name: pairing::NAME.into(),
            now: pairing::PT0 + 1,
        },
        Phase::A => NewEvent::Slot { bytes: slot(&p.slot_a) },
        Phase::C => NewEvent::Slot { bytes: slot(&p.slot_c) },
        Phase::Sas => NewEvent::SasEntered { digits: text(&mutate(p.digits.as_bytes(), edits)) },
        Phase::D if flags & 2 == 2 => {
            let sealed = pairing::aead_seal(&p.bundle_key, [9; 24], &p.bundle_ad, &mutate(&p.bundle, edits));
            NewEvent::Slot { bytes: p.k.try_slot(pairing::D, pairing::ROLE_E, &sealed).unwrap_or(sealed) }
        }
        Phase::D => NewEvent::Slot { bytes: slot(&p.slot_d) },
        Phase::Snapshot => {
            NewEvent::Snapshot { parts: split_parts(&mutate(&p.snapshot, edits)).into_iter().map(<[u8]>::to_vec).collect() }
        }
        Phase::Log => NewEvent::Entries { frames: one(&p.log) },
        Phase::Frames => NewEvent::Frames { frames: one(&p.frames) },
        Phase::Over => NewEvent::Tick { now: pairing::PT0 + 2 },
    }
}

/// Any event, its bytes of a length read from the input (two bytes, so up
/// to 65,535: frames and slots of real size).
fn raw_event(i: &mut Input<'_>) -> NewEvent {
    let k = i.byte();
    let n = usize::from(u16::from_be_bytes([i.byte(), i.byte()]));
    let x = i.u64();
    let bytes = i.take(n).to_vec();
    match k % 8 {
        0 => NewEvent::CodeEntered {
            code: text(&bytes),
            host: if x % 2 == 0 { pairing::HOST.into() } else { text(&bytes) },
            board_id: BoardId([k; 16]),
            door: pairing::DOOR,
            name: if x % 3 == 0 { pairing::NAME.into() } else { text(&bytes) },
            now: x,
        },
        1 => NewEvent::Slot { bytes },
        2 => NewEvent::SasEntered { digits: text(&bytes) },
        3 => NewEvent::Snapshot { parts: bytes.chunks(usize::try_from(x % 4096).unwrap_or(0) + 1).map(<[u8]>::to_vec).collect() },
        4 => NewEvent::Entries { frames: vec![(x, bytes)] },
        5 => NewEvent::Frames { frames: vec![(x, bytes)] },
        6 => NewEvent::Cancel,
        _ => NewEvent::Tick { now: x },
    }
}

/// One event for N in `phase`, read from the input: the byte `k` chooses
/// the honest event (`k % 8` 0 to 3); the honest event mutated (4: a count
/// of edits, then the edits; `k / 8` the flags of `mutated`); the honest
/// event of another phase, out of place (5: `k / 8` picks the phase); a tick
/// (6: the next 8 bytes); or any event (7: `raw_event`).
pub fn n_event(i: &mut Input<'_>, p: &Pairing, phase: Phase) -> NewEvent {
    let k = i.byte();
    match k % 8 {
        0..=3 => honest(p, phase),
        4 => {
            let n = usize::from(i.byte() % 17) * 4;
            let edits = i.take(n);
            mutated(p, phase, k / 8, edits)
        }
        5 => honest(p, PHASES[usize::from(k / 8) % PHASES.len()]),
        6 => NewEvent::Tick { now: i.u64() },
        _ => raw_event(i),
    }
}

/// What N emitted for each event, and its abort code if it aborted.
pub struct NReach {
    pub effects: Vec<Vec<&'static str>>,
    pub abort: Option<SealError>,
}

/// The work of the fuzz target `pair_new` (§17.10, N's machine): a fresh N
/// with the run's draws; the first byte (`% 9`) is how many honest events
/// come first, so that an input starts anywhere in the run; then up to 24
/// events of `n_event`, each for the phase N's effects show.
pub fn reach_pair_new(data: &[u8]) -> NReach {
    let p = pairing_run();
    let mut i = Input::new(data);
    let mut n = New::new();
    let mut rng = FixedEntropy::new(p.n_draws.clone());
    let mut phase = Phase::Code;
    let mut out = NReach { effects: Vec::new(), abort: None };
    let first = i.byte() % 9;
    for step in 0..32u8 {
        let ev = if step < first {
            honest(p, phase)
        } else if i.is_empty() || step >= first + 24 {
            break;
        } else {
            n_event(&mut i, p, phase)
        };
        let fx = n.on(ev, &mut rng);
        phase = phase.after(&fx);
        out.abort = out.abort.or(pairing::n_abort(&fx));
        out.effects.push(fx.iter().map(pairing::n_name).collect());
    }
    out
}

/// What N did with a slot D (`pair_bundle`).
pub struct BundleReach {
    pub abort: Option<SealError>,
    pub effects: Vec<&'static str>,
    pub slot_d: Vec<u8>,
}

/// The largest bundle plaintext whose slot D can be laid out: its body,
/// `nonce ‖ ciphertext ‖ tag`, is at most 65,535 bytes (a u16 `body_len`).
pub const BUNDLE_PT_BOUND: usize = 65_535 - 24 - 16;

/// The work of the fuzz target `pair_bundle` (§17.8, §17.9 steps 1 to 3):
/// a bundle plaintext (with the first byte odd, the rest; even, the honest
/// JSON mutated by the rest), bounded by `BUNDLE_PT_BOUND`, sealed properly
/// by a test playing E (`WithFakeE`, its draws fixed) into N's slot D. The
/// harness expects what the spec says at each size: a slot D body over
/// 16,040 bytes (a plaintext over 16,000) is refused by its framing with
/// `seal/slot_mac` (§17.4); any other is authentic, and is never refused
/// with `seal/slot_mac` or `seal/bundle_open`.
pub fn reach_pair_bundle(data: &[u8]) -> BundleReach {
    let board = BoardId([0xB0; 16]);
    let s = SnapAnnounce { seq: 70, hash: [1; 32], hash_version: 1, blob: BlobId([2; 16]), sha256: [3; 32], size: 900, parts: 1, inflated: 900, epoch: 2 };
    let json = pairing::bundle_json(&board, pairing::HOST, &pairing::DOOR, &[(1, [7; 32]), (2, [8; 32])], Head { seq: 70, chain: Chain([5; 32]) }, &s, 71, pairing::TOKEN);
    let mut i = Input::new(data);
    let raw = i.byte() % 2 == 1;
    let mut pt = if raw { i.rest().to_vec() } else { mutate(&json, i.rest()) };
    pt.truncate(BUNDLE_PT_BOUND);
    let w = pairing::WithFakeE::new(board, DevId([0xE0; 16])).accept();
    let slot_d = w.slot_d(&pt);
    let mut n = w.n;
    let fx = n.on(pair::NewEvent::Slot { bytes: slot_d.clone() }, &mut fixed(0));
    let abort = pairing::n_abort(&fx);
    if slot_d.len() - 36 > 16_040 {
        assert_eq!(abort, Some(SealError::SlotMac), "a slot D body over 16,040 bytes");
    } else {
        assert!(abort != Some(SealError::SlotMac) && abort != Some(SealError::BundleOpen), "{abort:?}");
    }
    BundleReach { abort, effects: fx.iter().map(pairing::n_name).collect(), slot_d }
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
