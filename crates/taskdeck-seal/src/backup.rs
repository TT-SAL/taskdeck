//! Backups and segments (seal.md §21, §26).
//!
//! A file is the 143-byte header of §21.2, the HPKE ciphertext (base mode to
//! the recovery key, §21.3) of the raw-DEFLATE payload of §21.5, and the
//! writer's signature of §16.3. `read_backup` runs the eleven steps of §21.6
//! in order and rejects the file at the first that fails (SEAL-BKP-003);
//! the history is verified by the projection (`projection::project`) before
//! anything of the payload after it is parsed (SEAL-BKP-005).

use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::keys::hpke_sender;
use crate::membership::Reader;
use crate::payloads::parse_ctl_unsigned;
use crate::prim::{HpkeAead, Kdf, Kem, deflate_raw, h, inflate_raw, push_zeroizing, take};
use crate::signatures::{sign_with, verify_strict_parts};
use crate::{
    BoardId, Chain, DevId, DeviceKeys, DevicePublic, Entropy, EntryId, EpochRoot, FrameHash, Head, Kind,
    Membership, Opened, RecoveryKeys, RecoveryPublic, SealError, chain_step, ctx, frame_header, state_hash,
};

pub(crate) mod projection;

/// `"TDBK"` (seal.md §21.2).
const MAGIC: &[u8; 4] = b"TDBK";
/// `"TDBP"` (seal.md §21.5).
const PAYLOAD_MAGIC: &[u8; 4] = b"TDBP";
/// The header, bytes 0 to 142 (seal.md §21.2).
pub(crate) const HEADER_LEN: usize = 143;
const SIG_LEN: usize = 64;
const TAG_LEN: usize = 16;
/// Header and signature: a file is `207 + ct_len` bytes (seal.md §21.2).
const OVERHEAD: usize = HEADER_LEN + SIG_LEN;
/// The smallest file: an empty plaintext's tag (seal.md §21.6 step 1).
const FILE_MIN: usize = OVERHEAD + TAG_LEN;
/// The largest file, 256 MiB (seal.md §21.2, §23).
const FILE_MAX: usize = 268_435_456;
/// The inflate cap, 512 MiB (seal.md §21.6 step 7, §23).
const INFLATE_CAP: usize = 536_870_912;
/// History records per backup (seal.md §21.5, §23).
pub(crate) const HIST_MAX: usize = 1_048_576;
/// `items` per history record (seal.md §21.5, §23).
pub(crate) const ITEMS_MAX: usize = 262_144;
/// A history record without its items (seal.md §21.5).
pub(crate) const RECORD_FIXED: usize = 156;
/// Frames per segment (seal.md §21.5, §23).
const FRAMES_MAX: usize = 1_048_576;
/// The largest frame (seal.md §8.1, §23).
const FRAME_MAX: usize = 262_144;
/// Backup `meta` (seal.md §21.5, §23).
const META_MAX: usize = 1_048_576;
/// A relay token (seal.md §6.7, §23).
const TOKEN_MAX: usize = 512;
/// `file_type` (seal.md §21.2).
const NIGHTLY: u8 = 1;
const SEGMENT: u8 = 2;

/// One history record of an accepted control entry (seal.md §21.5, §26).
pub struct HistoryRecord {
    pub(crate) seq: u64,
    pub(crate) dev: DevId,
    pub(crate) ctr: u64,
    pub(crate) entry_id: EntryId,
    pub(crate) at_ms: i64,
    pub(crate) frame_hash: FrameHash,
    pub(crate) items: Vec<u8>,
    pub(crate) sig: [u8; 64],
}

impl HistoryRecord {
    /// Builds the record of an accepted `ctl` entry (SEAL-BKP-010): `seq` the
    /// given seq, `dev` and `ctr` from the frame header, `entry_id` and
    /// `at_ms` from the opened plaintext, `frame_hash = H(frame)`, `items`
    /// the canonical encoding of the payload's `items` and `sig` its inner
    /// signature (§11.3).
    ///
    /// An opened entry of another kind is `seal/not_control`; a frame whose
    /// header does not parse is `seal/frame_format`, and a payload that is
    /// not a control payload of §11.3 `seal/bad_payload`.
    // SPEC-QUESTION: §21.5 and SEAL-BKP-010 name only `seal/not_control`; an
    // accepted `ctl` entry always has a header that parses and a payload of
    // §11.3, so the two other codes stand for a caller that passes something
    // `classify` never accepted, with the codes the receiver would give it.
    pub fn from_accepted(seq: u64, frame: &[u8], opened: &Opened) -> Result<Self, SealError> {
        if opened.kind != Kind::Ctl {
            return Err(SealError::NotControl);
        }
        let hdr = frame_header(frame)?;
        let u = parse_ctl_unsigned(&opened.payload).map_err(|_| SealError::BadPayload)?;
        if u.items_bytes.len() > ITEMS_MAX {
            return Err(SealError::BadPayload);
        }
        Ok(HistoryRecord {
            seq,
            dev: hdr.dev,
            ctr: hdr.ctr,
            entry_id: opened.entry_id,
            at_ms: opened.at_ms,
            frame_hash: FrameHash(h(&[frame])),
            items: u.items_bytes,
            sig: u.sig,
        })
    }

    /// The record's bytes (seal.md §21.5): `seq ‖ dev ‖ ctr ‖ entry_id ‖
    /// at_ms ‖ frame_hash ‖ vb32(items) ‖ sig`, `156 + n` bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(RECORD_FIXED + self.items.len());
        self.write(&mut o);
        o
    }

    fn write(&self, o: &mut Vec<u8>) {
        o.extend_from_slice(&self.seq.to_be_bytes());
        o.extend_from_slice(&self.dev.0);
        o.extend_from_slice(&self.ctr.to_be_bytes());
        o.extend_from_slice(&self.entry_id.0);
        o.extend_from_slice(&self.at_ms.to_be_bytes());
        o.extend_from_slice(&self.frame_hash.0);
        // Every constructor bounds `items` by 262,144 bytes.
        o.extend_from_slice(&u32::try_from(self.items.len()).unwrap_or(u32::MAX).to_be_bytes());
        o.extend_from_slice(&self.items);
        o.extend_from_slice(&self.sig);
    }

    /// Reads exactly one record, or `seal/backup_history` (seal.md §21.5).
    /// The items' length is checked against 262,144 and the bytes left
    /// before they are read (SEAL-GEN-008); no byte may follow. The items
    /// are not interpreted here: their signature is checked first, by the
    /// projection (SEAL-GEN-009).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SealError> {
        let mut r = Reader::new(bytes);
        let rec = HistoryRecord::read(&mut r).ok_or(SealError::BackupHistory)?;
        if r.remaining() != 0 {
            return Err(SealError::BackupHistory);
        }
        Ok(rec)
    }

    /// One record at the reader's position.
    pub(crate) fn read(r: &mut Reader<'_>) -> Option<HistoryRecord> {
        let seq = r.u64()?;
        let dev = DevId(r.arr()?);
        let ctr = r.u64()?;
        let entry_id = EntryId(r.arr()?);
        let at_ms = r.i64()?;
        let frame_hash = FrameHash(r.arr()?);
        let n = usize::try_from(r.u32()?).ok()?;
        // The items, then the 64-byte signature, must fit.
        let n = r.count(n, ITEMS_MAX, 1)?;
        if n.checked_add(SIG_LEN)? > r.remaining() {
            return None;
        }
        let items = r.bytes(n)?.to_vec();
        let sig = r.arr()?;
        Some(HistoryRecord { seq, dev, ctr, entry_id, at_ms, frame_hash, items, sig })
    }

    /// The entry's seq (seal.md §21.5).
    pub fn seq(&self) -> u64 {
        self.seq
    }
}

/// A nightly backup's or a segment's content (seal.md §21.5, §26).
pub enum BackupContent {
    Nightly {
        membership: Vec<u8>,
        board: Vec<u8>,
        head: Head,
        hash_version: u32,
        state_hash: [u8; 32],
    },
    Segment {
        membership: Vec<u8>,
        first_seq: u64,
        chain_before: Chain,
        frames: Vec<Vec<u8>>,
    },
}

impl BackupContent {
    fn file_type(&self) -> u8 {
        match self {
            BackupContent::Nightly { .. } => NIGHTLY,
            BackupContent::Segment { .. } => SEGMENT,
        }
    }

    fn membership(&self) -> &[u8] {
        match self {
            BackupContent::Nightly { membership, .. } | BackupContent::Segment { membership, .. } => membership,
        }
    }
}

/// What a writer puts in a backup file (seal.md §21, §26).
pub struct BackupInput<'a> {
    pub board: BoardId,
    pub seq: u64,
    pub written_ms: i64,
    pub history: &'a [HistoryRecord],
    pub content: BackupContent,
    pub roots: Vec<(u32, EpochRoot)>,
    pub read_token: Option<String>,
    pub meta: Vec<u8>,
    pub pinned_recovery: RecoveryPublic,
}

/// The fields of the header of §21.2 before `enc`.
struct HeaderFields {
    file_type: u8,
    board: BoardId,
    writer_dev: DevId,
    writer_sign_pk: [u8; 32],
    recipient: DevId,
    seq: u64,
    written_ms: i64,
}

/// `info = ctx("backup-hpke") ‖ board_id ‖ u64 seq ‖ writer_dev ‖ recipient
/// ‖ rhpke_pk` (seal.md §21.3).
fn hpke_info(board: &BoardId, seq: u64, writer: &DevId, recipient: &DevId, rhpke_pk: &[u8; 32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(ctx::BACKUP_HPKE.len() + 16 + 8 + 16 + 16 + 32);
    v.extend_from_slice(ctx::BACKUP_HPKE);
    v.extend_from_slice(&board.0);
    v.extend_from_slice(&seq.to_be_bytes());
    v.extend_from_slice(&writer.0);
    v.extend_from_slice(&recipient.0);
    v.extend_from_slice(rhpke_pk);
    v
}

/// `aad = ctx("backup-aad") ‖ file[0..143]` (seal.md §21.3).
fn hpke_aad(header: &[u8]) -> Vec<u8> {
    [ctx::BACKUP_AAD, header].concat()
}

/// The signed bytes of §16.3: `ctx("backup") ‖ H(file body)`.
fn signed_parts(body: &[u8]) -> [u8; 32] {
    h(&[body])
}

/// Seals `compressed` (the raw DEFLATE of a payload) into a file and signs
/// it (seal.md §21.2, §21.3, §16.3; SEAL-BKP-001). Draws exactly the 32
/// bytes of the HPKE ephemeral key's `ikm`. Keeps nothing: the HPKE context
/// is dropped here (SEAL-BKP-007).
fn seal_file(
    writer: &DeviceKeys,
    f: &HeaderFields,
    rhpke_pk: &[u8; 32],
    compressed: &[u8],
    rng: &mut dyn Entropy,
) -> Result<Vec<u8>, SealError> {
    let ct_len = compressed.len().checked_add(TAG_LEN).ok_or(SealError::BackupPayload)?;
    let size = ct_len.checked_add(OVERHEAD).ok_or(SealError::BackupPayload)?;
    if size > FILE_MAX {
        return Err(SealError::BackupPayload);
    }
    let info = hpke_info(&f.board, f.seq, &f.writer_dev, &f.recipient, rhpke_pk);
    let (enc, mut hpke_cx) = hpke_sender(rhpke_pk, &info, rng)?;
    let mut file = Vec::with_capacity(size);
    file.extend_from_slice(MAGIC);
    file.push(1);
    file.push(1);
    file.push(f.file_type);
    file.extend_from_slice(&f.board.0);
    file.extend_from_slice(&f.writer_dev.0);
    file.extend_from_slice(&f.writer_sign_pk);
    file.extend_from_slice(&f.recipient.0);
    file.extend_from_slice(&f.seq.to_be_bytes());
    file.extend_from_slice(&f.written_ms.to_be_bytes());
    file.extend_from_slice(&enc);
    file.extend_from_slice(&(ct_len as u64).to_be_bytes());
    let aad = hpke_aad(&file);
    let ct = hpke_cx.seal(compressed, &aad).map_err(|_| SealError::BackupPayload)?;
    drop(hpke_cx);
    if ct.len() != ct_len {
        return Err(SealError::BackupPayload);
    }
    file.extend_from_slice(&ct);
    let sig = sign_with(writer.signing_key(), &[ctx::BACKUP, &signed_parts(&file)]);
    file.extend_from_slice(&sig);
    Ok(file)
}

fn put_vb32(o: &mut Zeroizing<Vec<u8>>, x: &[u8]) -> Result<(), SealError> {
    let n = u32::try_from(x.len()).map_err(|_| SealError::BackupPayload)?;
    push_zeroizing(o, &n.to_be_bytes());
    push_zeroizing(o, x);
    Ok(())
}

/// The payload of §21.5, in a zeroizing buffer (it holds the roots).
fn encode_payload(
    history: &[HistoryRecord],
    content: &BackupContent,
    roots: &[(u32, EpochRoot)],
    token: &[u8],
    meta: &[u8],
) -> Result<Zeroizing<Vec<u8>>, SealError> {
    let mut p = Zeroizing::new(Vec::new());
    push_zeroizing(&mut p, PAYLOAD_MAGIC);
    push_zeroizing(&mut p, &1u32.to_be_bytes());
    push_zeroizing(&mut p, &[content.file_type()]);
    let n = u32::try_from(history.len()).map_err(|_| SealError::BackupPayload)?;
    push_zeroizing(&mut p, &n.to_be_bytes());
    for r in history {
        push_zeroizing(&mut p, &r.to_bytes());
    }
    put_vb32(&mut p, content.membership())?;
    match content {
        BackupContent::Nightly { board, head, hash_version, state_hash, .. } => {
            put_vb32(&mut p, board)?;
            push_zeroizing(&mut p, &head.chain.0);
            push_zeroizing(&mut p, &hash_version.to_be_bytes());
            push_zeroizing(&mut p, state_hash);
        }
        BackupContent::Segment { first_seq, chain_before, frames, .. } => {
            push_zeroizing(&mut p, &first_seq.to_be_bytes());
            push_zeroizing(&mut p, &chain_before.0);
            let n = u32::try_from(frames.len()).map_err(|_| SealError::BackupPayload)?;
            push_zeroizing(&mut p, &n.to_be_bytes());
            for f in frames {
                put_vb32(&mut p, f)?;
            }
        }
    }
    let n = u16::try_from(roots.len()).map_err(|_| SealError::BackupPayload)?;
    push_zeroizing(&mut p, &n.to_be_bytes());
    for (e, r) in roots {
        push_zeroizing(&mut p, &e.to_be_bytes());
        push_zeroizing(&mut p, &r.to_bytes()[..]);
    }
    let n = u16::try_from(token.len()).map_err(|_| SealError::BackupPayload)?;
    push_zeroizing(&mut p, &n.to_be_bytes());
    push_zeroizing(&mut p, token);
    put_vb32(&mut p, meta)?;
    Ok(p)
}

/// A relay token of §6.7: 1 to 512 ASCII bytes (SEAL-KEY-015).
fn token_is_valid(t: &[u8]) -> bool {
    (1..=TOKEN_MAX).contains(&t.len()) && t.is_ascii()
}

/// Writes a backup or segment file (seal.md §21.2 to §21.5, §26).
///
/// Refuses, before anything is drawn:
/// - membership bytes that do not parse (`seal/membership_format`);
/// - a membership whose current recovery key is not `pinned_recovery`
///   (`seal/backup_unpinned`, SEAL-BKP-006, V1-2); the file is sealed to the
///   pinned key, whose `dev_R` is its `recipient`;
/// - a writer that is not a current device of the membership with its own
///   signing key (`seal/backup_writer`);
/// - a history whose seqs are not exactly the membership's `ctl_seqs`
///   (`seal/backup_history`, SEAL-BKP-002);
/// - anything else the reader of §21.6 would reject in the layout of §21.5
///   or §23 (`seal/backup_payload`): a membership of another board, a
///   nightly head other than `seq`, a segment whose frames are not the
///   seqs `first_seq` to `seq` or whose history reaches `first_seq`, a frame
///   over 262,144 bytes, roots with an epoch twice or one that does not
///   match the commitment the membership records (SEAL-KEY-013), a token
///   that is not 1 to 512 ASCII bytes, `meta` over 1,048,576 bytes, a
///   payload over 512 MiB or a file over 256 MiB.
///
/// Then: raw DEFLATE of the payload, HPKE base mode to the recovery key with
/// the `info` and `aad` of §21.3 (the ephemeral key from 32 drawn bytes,
/// `DeriveKeyPair`, as `wrap_root` makes it), and the signature of §16.3.
/// It keeps and returns nothing that opens the file: the payload and its
/// compression are wiped, the HPKE context dropped (SEAL-BKP-007).
// SPEC-QUESTION: §21.3 states the writer's refusal for an unpinned key and
// §22 no code for the other inconsistent inputs; this refuses them with the
// code of the step of §21.6 that would reject the file, so that the writer
// never writes a file its reader rejects for its layout.
// SPEC-QUESTION: §21.5 has a segment carry "every root the writer holds"
// next to `M_{first_seq − 1}`, and §21.6 step 11 checks "every root" against
// its commitment; a root of an epoch rotated inside the segment has no
// commitment in that membership. This takes the refusing reading: the
// writer may put only roots whose commitment the file's membership records.
pub fn write_backup(writer: &DeviceKeys, input: BackupInput, rng: &mut dyn Entropy) -> Result<Vec<u8>, SealError> {
    let BackupInput { board, seq, written_ms, history, content, mut roots, read_token, meta, pinned_recovery } = input;
    let bad = SealError::BackupPayload;
    let m = Membership::from_canonical_bytes(content.membership())?;
    let cur = m.current_recovery();
    if m.epoch() == 0 || cur.sign_pk != pinned_recovery.sign_pk || cur.hpke_pk != pinned_recovery.hpke_pk {
        return Err(SealError::BackupUnpinned);
    }
    if m.board() != board {
        return Err(bad);
    }
    let me = writer.public();
    if !m.device(&me.dev).is_some_and(|d| d.is_current() && d.sign_pk == me.sign_pk) {
        return Err(SealError::BackupWriter);
    }
    if history.len() > HIST_MAX || !history.iter().map(|r| r.seq).eq(m.ctl_seqs.iter().copied()) {
        return Err(SealError::BackupHistory);
    }
    match &content {
        BackupContent::Nightly { head, .. } => {
            if head.seq != seq {
                return Err(bad);
            }
        }
        BackupContent::Segment { first_seq, frames, .. } => {
            let n = seq.checked_sub(*first_seq).and_then(|d| d.checked_add(1));
            if *first_seq == 0 || n != Some(frames.len() as u64) || frames.len() > FRAMES_MAX {
                return Err(bad);
            }
            if history.last().is_some_and(|r| r.seq >= *first_seq) || frames.iter().any(|f| f.len() > FRAME_MAX) {
                return Err(bad);
            }
        }
    }
    roots.sort_by_key(|(e, _)| *e);
    if roots.windows(2).any(|w| matches!(w, [a, b] if a.0 == b.0)) {
        return Err(bad);
    }
    for (e, r) in &roots {
        let ok = m.commitment(*e).is_some_and(|c| bool::from(r.commitment(&board, *e).ct_eq(c)));
        if !ok {
            return Err(bad);
        }
    }
    let token: &[u8] = match &read_token {
        None => &[],
        Some(t) if token_is_valid(t.as_bytes()) => t.as_bytes(),
        Some(_) => return Err(bad),
    };
    if meta.len() > META_MAX {
        return Err(bad);
    }
    let payload = encode_payload(history, &content, &roots, token, &meta)?;
    if payload.len() > INFLATE_CAP {
        return Err(bad);
    }
    let compressed = deflate_raw(&payload).ok_or(bad)?;
    drop(payload);
    let f = HeaderFields {
        file_type: content.file_type(),
        board,
        writer_dev: me.dev,
        writer_sign_pk: me.sign_pk,
        recipient: pinned_recovery.dev(&board),
        seq,
        written_ms,
    };
    seal_file(writer, &f, &pinned_recovery.hpke_pk, &compressed, rng)
}

/// A backup file that passed every step of §21.6 (seal.md §26).
///
/// `projection` is the membership the history alone gives (§21.6
/// "Projection"; see `projection::project` for what it holds besides the
/// devices, keys, statuses, recovery records, epochs, commitments and
/// `ctl_seqs` that step 11 compares).
pub struct VerifiedBackup {
    pub writer: DevicePublic,
    pub seq: u64,
    pub file_type: u8,
    pub content: BackupContent,
    pub roots: Vec<(u32, EpochRoot)>,
    pub read_token: Option<String>,
    pub meta: Vec<u8>,
    pub projection: Membership,
}

/// A backup file's header (seal.md §21.2, §26).
pub struct BackupHeader {
    pub file_type: u8,
    pub board: BoardId,
    pub writer_dev: DevId,
    pub writer_sign_pk: [u8; 32],
    pub recipient: DevId,
    pub seq: u64,
    pub written_ms: i64,
    pub ct_len: u64,
}

/// §21.6 steps 1 and 2: the size, then the fixed header; nothing else is
/// read. Returns the header fields and `enc`.
fn check_header(bytes: &[u8]) -> Result<(BackupHeader, [u8; 32]), SealError> {
    // 1.
    if !(FILE_MIN..=FILE_MAX).contains(&bytes.len()) {
        return Err(SealError::BackupSize);
    }
    // 2.
    let bad = SealError::BackupHeader;
    if take::<4>(bytes, 0) != Some(MAGIC) {
        return Err(bad);
    }
    let [ver, suite, file_type] = *take::<3>(bytes, 4).ok_or(bad)?;
    let ct_len = u64::from_be_bytes(*take::<8>(bytes, 135).ok_or(bad)?);
    let want = (bytes.len() - OVERHEAD) as u64;
    if ver != 1 || suite != 1 || !matches!(file_type, NIGHTLY | SEGMENT) || ct_len != want {
        return Err(bad);
    }
    let hdr = BackupHeader {
        file_type,
        board: BoardId(*take::<16>(bytes, 7).ok_or(bad)?),
        writer_dev: DevId(*take::<16>(bytes, 23).ok_or(bad)?),
        writer_sign_pk: *take::<32>(bytes, 39).ok_or(bad)?,
        recipient: DevId(*take::<16>(bytes, 71).ok_or(bad)?),
        seq: u64::from_be_bytes(*take::<8>(bytes, 87).ok_or(bad)?),
        written_ms: i64::from_be_bytes(*take::<8>(bytes, 95).ok_or(bad)?),
        ct_len,
    };
    Ok((hdr, *take::<32>(bytes, 103).ok_or(bad)?))
}

/// §21.6 step 5: the signature verifies strictly under `writer_sign_pk`
/// over `ctx("backup") ‖ H(file[0 .. 143 + ct_len])` (SEAL-SIG-006,
/// SEAL-GEN-010). `bytes` passed steps 1 and 2.
fn check_signature(bytes: &[u8], writer_sign_pk: &[u8; 32]) -> Result<(), SealError> {
    let bad = SealError::BackupSignature;
    let end = bytes.len().checked_sub(SIG_LEN).ok_or(bad)?;
    let body = bytes.get(..end).ok_or(bad)?;
    let sig = take::<64>(bytes, end).ok_or(bad)?;
    verify_strict_parts(writer_sign_pk, &[ctx::BACKUP, &signed_parts(body)], sig).map_err(|_| bad)
}

/// Checks a file's header and writer signature: §21.6 steps 1, 2 and 5
/// only, in that order, under the writer's own key (SEAL-BKP-009; seal.md
/// §26). A header whose `writer_sign_pk` is not `writer_sign_pk` is
/// `seal/backup_signature`, checked before the signature. Opens, inflates
/// and parses no byte of `ct`.
pub fn verify_backup_file(bytes: &[u8], writer_sign_pk: &[u8; 32]) -> Result<BackupHeader, SealError> {
    let (hdr, _) = check_header(bytes)?;
    if hdr.writer_sign_pk != *writer_sign_pk {
        return Err(SealError::BackupSignature);
    }
    check_signature(bytes, writer_sign_pk)?;
    Ok(hdr)
}

/// §21.6 step 8: `magic`, `version`, `file_type` and the history records
/// alone, each length checked before it is read. Returns the records and the
/// reader, positioned after them.
fn parse_history<'a>(p: &'a [u8], file_type: u8) -> Option<(Vec<HistoryRecord>, Reader<'a>)> {
    let mut r = Reader::new(p);
    if &r.arr::<4>()? != PAYLOAD_MAGIC || r.u32()? != 1 || r.u8()? != file_type {
        return None;
    }
    let n = usize::try_from(r.u32()?).ok()?;
    if n == 0 {
        return None;
    }
    let n = r.count(n, HIST_MAX, RECORD_FIXED)?;
    let mut history = Vec::with_capacity(n);
    for _ in 0..n {
        history.push(HistoryRecord::read(&mut r)?);
    }
    Some((history, r))
}

/// `vb32` bytes of at most `max`.
fn vb32<'a>(r: &mut Reader<'a>, max: usize) -> Option<&'a [u8]> {
    let n = usize::try_from(r.u32()?).ok()?;
    let n = r.count(n, max, 1)?;
    r.bytes(n)
}

/// What step 11 parses after the history.
struct Rest {
    content: BackupContent,
    roots: Vec<(u32, EpochRoot)>,
    read_token: Option<String>,
    meta: Vec<u8>,
}

/// §21.6 step 11, the layout: the fields of §21.5 after the history, every
/// length checked before it is read, every list in its order, nothing after.
fn parse_rest(r: &mut Reader<'_>, hdr: &BackupHeader) -> Option<Rest> {
    let membership = vb32(r, usize::MAX)?.to_vec();
    let content = if hdr.file_type == NIGHTLY {
        let board = vb32(r, usize::MAX)?.to_vec();
        let chain = Chain(r.arr()?);
        let hash_version = r.u32()?;
        let state_hash = r.arr()?;
        BackupContent::Nightly { membership, board, head: Head { seq: hdr.seq, chain }, hash_version, state_hash }
    } else {
        let first_seq = r.u64()?;
        let chain_before = Chain(r.arr()?);
        let n = usize::try_from(r.u32()?).ok()?;
        let want = hdr.seq.checked_sub(first_seq)?.checked_add(1)?;
        if first_seq == 0 || u64::try_from(n).ok()? != want {
            return None;
        }
        let n = r.count(n, FRAMES_MAX, 4)?;
        let mut frames = Vec::with_capacity(n);
        for _ in 0..n {
            frames.push(vb32(r, FRAME_MAX)?.to_vec());
        }
        BackupContent::Segment { membership, first_seq, chain_before, frames }
    };
    let n = usize::from(r.u16()?);
    let n = r.count(n, usize::from(u16::MAX), 36)?;
    let mut roots: Vec<(u32, EpochRoot)> = Vec::with_capacity(n);
    for _ in 0..n {
        let e = r.u32()?;
        let root = EpochRoot::from_bytes(r.arr()?);
        if roots.last().is_some_and(|(p, _)| *p >= e) {
            return None;
        }
        roots.push((e, root));
    }
    let n = usize::from(r.u16()?);
    let token = r.bytes(r.count(n, TOKEN_MAX, 1)?)?;
    let read_token = match token {
        [] => None,
        t if token_is_valid(t) => Some(core::str::from_utf8(t).ok()?.to_owned()),
        _ => return None,
    };
    let meta = vb32(r, META_MAX)?.to_vec();
    if r.remaining() != 0 {
        return None;
    }
    Some(Rest { content, roots, read_token, meta })
}

/// §21.6 step 11, the agreement of the membership bytes with the projection:
/// the same devices with the same keys and statuses, the same current
/// recovery key, the same epochs and commitments, the same `ctl_seqs`; and
/// nothing else of either state.
fn agrees(m: &Membership, p: &Membership) -> bool {
    let devices = m.devices.len() == p.devices.len()
        && m.devices.iter().zip(&p.devices).all(|(a, b)| {
            a.dev == b.dev && a.sign_pk == b.sign_pk && a.hpke_pk == b.hpke_pk && a.is_current() == b.is_current()
        });
    let (ra, rb) = (m.current_recovery(), p.current_recovery());
    let recovery = ra.dev == rb.dev && ra.sign_pk == rb.sign_pk && ra.hpke_pk == rb.hpke_pk;
    devices && recovery && m.epoch == p.epoch && m.commitments == p.commitments && m.ctl_seqs == p.ctl_seqs
}

/// Reads a backup file with a typed sheet's keys (seal.md §21.6, §26): the
/// eleven steps in order, the file rejected at the first that fails
/// (SEAL-BKP-003). The board id and the recovery key are the sheet's, never
/// the file's (SEAL-BKP-004).
///
/// 1. `223 ≤ size ≤ 268,435,456` (`seal/backup_size`);
/// 2. the fixed header (`seal/backup_header`);
/// 3. `board_id` is the sheet's (`seal/backup_board`);
/// 4. `recipient` is the seed's `dev_R` (`seal/backup_recipient`);
/// 5. the writer signature, strict (`seal/backup_signature`);
/// 6. HPKE `OpenBase` with the seed's key and the `info` and `aad` rebuilt
///    from the header and the seed's public key (`seal/backup_open`);
/// 7. inflate while streaming under 512 MiB, failing on more output or on
///    input after the stream's end (`seal/inflate`);
/// 8. `magic`, `version`, `file_type` and the history records alone
///    (`seal/backup_payload`);
/// 9. the projection (`seal/backup_history`), and no record after the
///    file's `seq`;
/// 10. the writer is a current device of the projection with
///     `writer_sign_pk`, and `recipient` its current recovery identity
///     (`seal/backup_writer`);
/// 11. the rest, strictly (`seal/backup_payload`): the layout; the
///     membership parses (§13.2) and agrees with the projection; every root
///     matches its commitment (SEAL-KEY-013); a nightly file's state hash
///     recomputed from `board` and `membership` (`engine.md` §18.3); a
///     segment's chain recomputed from `chain_before` (§15.1).
///
/// Nothing of the payload after the history is parsed before step 10
/// (SEAL-BKP-005). The plaintexts are held in zeroizing buffers.
// SPEC-QUESTION: step 11 recomputes a segment's chain from `chain_before`
// but §21.5 carries no value to compare the result with (the head of a
// segment is not in the file, and its history ends at `first_seq − 1`):
// the chain is recomputed over every frame, and the only check it allows is
// that each frame is at most 262,144 bytes (its `vb32` limit here).
// SPEC-QUESTION: step 11 recomputes the state hash for the file's
// `hash_version`; `engine.md` §18.3 defines version 1 only. A version that
// the membership's rules do not allow (§14.3) is rejected.
// SPEC-QUESTION: §21.5 says the history runs "from seq 1 to the file's
// seq (or first_seq − 1 for a segment)"; a record after the header's `seq`
// fails step 9 (it is part of the history's check), one at or after a
// segment's `first_seq` fails step 11 (`first_seq` is parsed there).
pub fn read_backup(bytes: &[u8], sheet_board: &BoardId, recovery: &RecoveryKeys) -> Result<VerifiedBackup, SealError> {
    use hpke::Deserializable as _;
    // 1, 2.
    let (hdr, enc) = check_header(bytes)?;
    // 3.
    if hdr.board != *sheet_board {
        return Err(SealError::BackupBoard);
    }
    // 4.
    let rp = recovery.public();
    let dev_r = rp.dev(sheet_board);
    if hdr.recipient != dev_r {
        return Err(SealError::BackupRecipient);
    }
    // 5.
    check_signature(bytes, &hdr.writer_sign_pk)?;
    // 6.
    let open = SealError::BackupOpen;
    let header = bytes.get(..HEADER_LEN).ok_or(open)?;
    let ct = bytes.get(HEADER_LEN..bytes.len() - SIG_LEN).ok_or(open)?;
    let info = hpke_info(sheet_board, hdr.seq, &hdr.writer_dev, &dev_r, &rp.hpke_pk);
    let aad = hpke_aad(header);
    let enc = <Kem as hpke::Kem>::EncappedKey::from_bytes(&enc).map_err(|_| open)?;
    let sk = <Kem as hpke::Kem>::PrivateKey::from_bytes(recovery.hpke_secret().as_bytes()).map_err(|_| open)?;
    let mut rx = hpke::setup_receiver::<HpkeAead, Kdf, Kem>(&hpke::OpModeR::Base, &sk, &enc, &info).map_err(|_| open)?;
    let compressed = Zeroizing::new(rx.open(ct, &aad).map_err(|_| open)?);
    drop(rx);
    // 7.
    let payload = inflate_raw(&compressed, INFLATE_CAP).ok_or(SealError::Inflate)?;
    drop(compressed);
    // 8.
    let (history, mut r) = parse_history(&payload, hdr.file_type).ok_or(SealError::BackupPayload)?;
    // 9.
    let projection = projection::project(&history, sheet_board)?;
    if history.iter().any(|rec| rec.seq > hdr.seq) {
        return Err(SealError::BackupHistory);
    }
    // 10.
    let writer = projection.device(&hdr.writer_dev).filter(|d| d.is_current() && d.sign_pk == hdr.writer_sign_pk);
    let Some(writer) = writer else { return Err(SealError::BackupWriter) };
    let writer = DevicePublic { dev: writer.dev, sign_pk: writer.sign_pk, hpke_pk: writer.hpke_pk };
    if projection.current_recovery().dev != hdr.recipient {
        return Err(SealError::BackupWriter);
    }
    // 11.
    let bad = SealError::BackupPayload;
    let rest = parse_rest(&mut r, &hdr).ok_or(bad)?;
    let m = Membership::from_canonical_bytes(rest.content.membership()).map_err(|_| bad)?;
    if !agrees(&m, &projection) {
        return Err(bad);
    }
    for (e, root) in &rest.roots {
        let ok = m.commitment(*e).is_some_and(|c| bool::from(root.commitment(sheet_board, *e).ct_eq(c)));
        if !ok {
            return Err(bad);
        }
    }
    match &rest.content {
        BackupContent::Nightly { membership, board, hash_version, state_hash: want, .. } => {
            if !crate::membership::transitions::hash_version_allowed(m.rules, *hash_version) {
                return Err(bad);
            }
            let got = state_hash(*hash_version, &h(&[board]), membership);
            if !bool::from(got.ct_eq(want)) {
                return Err(bad);
            }
        }
        BackupContent::Segment { first_seq, chain_before, frames, .. } => {
            if history.last().is_some_and(|rec| rec.seq >= *first_seq) {
                return Err(bad);
            }
            let mut c = *chain_before;
            let mut s = *first_seq;
            for f in frames {
                c = chain_step(&c, s, f);
                s = s.checked_add(1).ok_or(bad)?;
            }
        }
    }
    Ok(VerifiedBackup {
        writer,
        seq: hdr.seq,
        file_type: hdr.file_type,
        content: rest.content,
        roots: rest.roots,
        read_token: rest.read_token,
        meta: rest.meta,
        projection,
    })
}

#[cfg(test)]
mod tests;
