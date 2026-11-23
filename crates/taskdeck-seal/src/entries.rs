//! Frames, envelope, plaintext, kinds, payloads and control items (seal.md §8 to §12, §26).
//!
//! The sending side is public (§26). The receiving side is private, one
//! function per step of §14.2 that this module owns, each returning the
//! code §14.2 gives its step; task S3's `classify` calls them in order:
//!
//! | Step | Function | Code on failure |
//! |---|---|---|
//! | 1 | `frame_header` | `seal/frame_format` |
//! | 4 | `verify_frame_signature` | `seal/bad_signature` |
//! | 8, 9 | `envelope_header` (reads `ver`, `suite`, `epoch`) | none: the checks are S3's |
//! | 10 | `open_envelope` | `seal/aead` |
//! | 11 | `unpad` | `seal/bad_plaintext` |
//! | 15 | `payloads::parse_ctl`, `parse_beat`, `parse_snap` | `seal/bad_payload`, `seal/bad_ctl_sig` |
//! | 16 | `payloads::ctl_shape` (shape only) | `seal/ctl_shape` |

use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::prim::{h, take};
use crate::signatures::{sign_with, signer_identity};
use crate::{
    BlobId, BoardId, Chain, DevId, Entropy, EntryId, FrameHash, Head, RecoveryPublic, SealError, Signer,
    SymKey, chain_zero, ctx, payloads,
};

/// `"TDF1"` (seal.md §8.1).
pub(crate) const MAGIC: &[u8; 4] = b"TDF1";
/// The frame header: magic, `board_id`, `dev`, `ctr`, `len` (seal.md §8.1).
pub(crate) const HEADER_LEN: usize = 48;
/// The signature (seal.md §8.1).
pub(crate) const SIG_LEN: usize = 64;
/// Header and signature: a frame is `112 + len` bytes (seal.md §8.1).
pub(crate) const FRAME_OVERHEAD: usize = HEADER_LEN + SIG_LEN;
/// The largest frame (seal.md §8.1, §23).
pub(crate) const FRAME_MAX: usize = 262_144;
/// The smallest frame (seal.md §8.1, §23).
pub(crate) const FRAME_MIN: usize = 670;
/// `ver`, `suite`, `epoch`, `nonce` (seal.md §9.1).
pub(crate) const BODY_PREFIX: usize = 30;
/// The XChaCha20-Poly1305 nonce (seal.md §9.1).
pub(crate) const NONCE_LEN: usize = 24;
/// The AEAD tag (seal.md §9.1).
pub(crate) const TAG_LEN: usize = 16;
/// The fixed plaintext prefix (seal.md §10.1).
pub(crate) const PLAIN_PREFIX: u64 = 69;
/// The largest padded plaintext (seal.md §10.2, §23).
pub(crate) const PADDED_MAX: u64 = 258_048;
/// The largest payload (seal.md §10.2, §23).
pub(crate) const PAYLOAD_MAX: u64 = PADDED_MAX - PLAIN_PREFIX;
/// The largest `at_ms`: 9999-12-31T23:59:59.999Z (seal.md §10.1, §23).
pub(crate) const AT_MS_MAX: i64 = 253_402_300_799_999;
/// Envelope version and suite under rules version 1 (seal.md §9.1).
pub(crate) const VER: u8 = 1;
pub(crate) const SUITE: u8 = 1;

/// The kind byte of a plaintext (seal.md §11.1, §26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Cmd = 1,
    Ctl = 2,
    Beat = 3,
    Snap = 4,
}

impl Kind {
    fn from_byte(b: u8) -> Option<Kind> {
        match b {
            1 => Some(Kind::Cmd),
            2 => Some(Kind::Ctl),
            3 => Some(Kind::Beat),
            4 => Some(Kind::Snap),
            _ => None,
        }
    }
}

/// The plaintext fields of an entry (seal.md §10.1, §26).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plain {
    pub entry_id: EntryId,
    pub kind: Kind,
    pub at_ms: i64,
    pub based_on: Head,
    pub payload: Vec<u8>,
}

/// The padding function `P(U)` (seal.md §10.2, §26).
///
/// 512-byte buckets up to 8192, Padmé above. `P(0) = 0`. Where `U + m`
/// would exceed `u64` (only for `U` above 2^63, far beyond any plaintext),
/// the result is `u64::MAX`, which is above the cap and so refused.
// SPEC-QUESTION: §10.2 computes on unsigned 64-bit integers without
// saying what `(U + m)` is when it overflows; this saturates (any such U is
// over the cap either way).
pub fn padded_len(unpadded: u64) -> u64 {
    let u = unpadded;
    if u <= 8192 {
        return u.div_ceil(512) * 512;
    }
    let e = u64::from(63 - u.leading_zeros()); // floor(log2 U), U > 0
    let s = u64::from(63 - e.leading_zeros()) + 1; // floor(log2 E) + 1, E ≥ 13
    let z = e - s;
    let m = (1u64 << z) - 1;
    match u.checked_add(m) {
        Some(x) => x & !m,
        None => u64::MAX,
    }
}

/// Draws a new entry id: exactly 16 bytes (seal.md §10.1, SEAL-PLAIN-002, §26).
pub fn new_entry_id(rng: &mut dyn Entropy) -> Result<EntryId, SealError> {
    let mut id = [0u8; 16];
    rng.fill(&mut id)?;
    Ok(EntryId(id))
}

/// The padded plaintext of §10.1, or the sealing error: a payload over the
/// cap is `seal/payload_too_large` (SEAL-PAD-002); a `based_on` of seq 0
/// whose chain is not `C_0`, or an `at_ms` outside §10.1's range, is
/// `seal/bad_plaintext` (SEAL-PLAIN-003).
// SPEC-QUESTION: §22 names `seal/bad_plaintext` at sealing only for the
// `based_on` rule; an `at_ms` outside 0 to 253,402,300,799,999 would make
// every receiver refuse the entry at step 11, so the sealer refuses it too,
// with the same code. Likewise an empty `cmd` payload (§11.2 says 1 to
// 257,979 bytes) is refused with `seal/bad_plaintext`.
pub(crate) fn pad_plaintext(board: &BoardId, plain: &Plain) -> Result<Vec<u8>, SealError> {
    let n = plain.payload.len() as u64;
    if n > PAYLOAD_MAX {
        return Err(SealError::PayloadTooLarge);
    }
    if plain.based_on.seq == 0 && plain.based_on.chain != chain_zero(board) {
        return Err(SealError::BadPlaintext);
    }
    if !(0..=AT_MS_MAX).contains(&plain.at_ms) {
        return Err(SealError::BadPlaintext);
    }
    if plain.kind == Kind::Cmd && plain.payload.is_empty() {
        return Err(SealError::BadPlaintext);
    }
    let u = PLAIN_PREFIX + n;
    let p = padded_len(u);
    let p = usize::try_from(p).map_err(|_| SealError::PayloadTooLarge)?;
    let len32 = u32::try_from(n).map_err(|_| SealError::PayloadTooLarge)?;
    let mut out = Vec::with_capacity(p);
    out.extend_from_slice(&plain.entry_id.0);
    out.push(plain.kind as u8);
    out.extend_from_slice(&plain.at_ms.to_be_bytes());
    out.extend_from_slice(&plain.based_on.seq.to_be_bytes());
    out.extend_from_slice(&plain.based_on.chain.0);
    out.extend_from_slice(&len32.to_be_bytes());
    out.extend_from_slice(&plain.payload);
    out.resize(p, 0);
    Ok(out)
}

/// The AEAD associated data of §9.2: `ctx("entry") ‖ frame[0..48] ‖ ver ‖
/// suite ‖ u32 epoch` (SEAL-ENV-002).
pub(crate) fn entry_ad(header: &[u8; HEADER_LEN], ver: u8, suite: u8, epoch: u32) -> Vec<u8> {
    let mut ad = Vec::with_capacity(ctx::ENTRY.len() + HEADER_LEN + 6);
    ad.extend_from_slice(ctx::ENTRY);
    ad.extend_from_slice(header);
    ad.push(ver);
    ad.push(suite);
    ad.extend_from_slice(&epoch.to_be_bytes());
    ad
}

fn cipher(key: &SymKey) -> Result<XChaCha20Poly1305, SealError> {
    XChaCha20Poly1305::new_from_slice(key.as_bytes()).map_err(|_| SealError::Aead)
}

/// Seals and signs one frame (seal.md §8, §9, §10, §26).
///
/// Refuses, before drawing anything: `ctr = 0` with `seal/frame_format`
/// (SEAL-FRAME-004); the plaintext errors of `pad_plaintext`; a
/// `Signer::Recovery` whose board is not `board` with `seal/unknown_author`.
/// Then draws exactly the 24-byte nonce (SEAL-GEN-012) and nothing else:
/// the entry id is the caller's, in `plain` (SEAL-PLAIN-002). The author's
/// `dev` is the device's, or `dev_R` for `Signer::Recovery` (SEAL-KEY-007);
/// the counter is the caller's argument.
pub fn seal_frame(
    signer: &Signer,
    board: &BoardId,
    ctr: u64,
    epoch: u32,
    key: &SymKey,
    plain: &Plain,
    rng: &mut dyn Entropy,
) -> Result<Vec<u8>, SealError> {
    if ctr == 0 {
        return Err(SealError::FrameFormat);
    }
    let (dev, sk) = signer_identity(signer, board)?;
    let padded = pad_plaintext(board, plain)?;
    let body_len = BODY_PREFIX + padded.len() + TAG_LEN;
    let len = u32::try_from(body_len).map_err(|_| SealError::PayloadTooLarge)?;
    let mut nonce = [0u8; NONCE_LEN];
    rng.fill(&mut nonce)?;

    let mut header = [0u8; HEADER_LEN];
    {
        let (m, rest) = header.split_at_mut(4);
        m.copy_from_slice(MAGIC);
        let (b, rest) = rest.split_at_mut(16);
        b.copy_from_slice(&board.0);
        let (d, rest) = rest.split_at_mut(16);
        d.copy_from_slice(&dev.0);
        let (c, l) = rest.split_at_mut(8);
        c.copy_from_slice(&ctr.to_be_bytes());
        l.copy_from_slice(&len.to_be_bytes());
    }
    let ad = entry_ad(&header, VER, SUITE, epoch);
    let ct = cipher(key)?
        .encrypt(&XNonce::from(nonce), Payload { msg: &padded, aad: &ad })
        .map_err(|_| SealError::Aead)?;
    if ct.len() != padded.len() + TAG_LEN {
        return Err(SealError::Aead);
    }

    let mut frame = Vec::with_capacity(FRAME_OVERHEAD + body_len);
    frame.extend_from_slice(&header);
    frame.push(VER);
    frame.push(SUITE);
    frame.extend_from_slice(&epoch.to_be_bytes());
    frame.extend_from_slice(&nonce);
    frame.extend_from_slice(&ct);
    let sig = sign_with(sk, &[ctx::FRAME, &frame]);
    frame.extend_from_slice(&sig);
    Ok(frame)
}

/// `H(frame)` over all `112 + len` bytes (seal.md §8.2, §26).
pub fn frame_hash(frame: &[u8]) -> FrameHash {
    FrameHash(h(&[frame]))
}

/// A frame's fixed header fields (seal.md §8.1, §26).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub board: BoardId,
    pub dev: DevId,
    pub ctr: u64,
    pub len: u32,
}

/// Parses a frame's header: §14.2 step 1 only (seal.md §26, SEAL-FRAME-003).
///
/// The total length is 670 to 262,144 bytes, the magic `"TDF1"` and `len`
/// the total minus 112, checked in that order before any other field is
/// read; any failure is `seal/frame_format`. Nothing else is checked
/// (`ctr = 0` included: that is the counter check of step 5).
pub fn frame_header(frame: &[u8]) -> Result<FrameHeader, SealError> {
    let bad = SealError::FrameFormat;
    if !(FRAME_MIN..=FRAME_MAX).contains(&frame.len()) {
        return Err(bad);
    }
    if take::<4>(frame, 0) != Some(MAGIC) {
        return Err(bad);
    }
    let len = u32::from_be_bytes(*take::<4>(frame, 44).ok_or(bad)?);
    if usize::try_from(len).ok() != frame.len().checked_sub(FRAME_OVERHEAD) {
        return Err(bad);
    }
    Ok(FrameHeader {
        board: BoardId(*take::<16>(frame, 4).ok_or(bad)?),
        dev: DevId(*take::<16>(frame, 20).ok_or(bad)?),
        ctr: u64::from_be_bytes(*take::<8>(frame, 36).ok_or(bad)?),
        len,
    })
}

/// The bytes the frame signature covers, `frame[0 .. 48 + len]`, and the
/// signature, for a frame that passed `frame_header`.
fn signed_part(frame: &[u8]) -> Option<(&[u8], &[u8; 64])> {
    let end = frame.len().checked_sub(SIG_LEN)?;
    let sig = take::<64>(frame, end)?;
    Some((frame.get(..end)?, sig))
}

/// §14.2 step 4: the frame's signature verifies strictly (SEAL-GEN-010)
/// under `sign_pk` over `ctx("frame") ‖ frame[0 .. 48 + len]`
/// (SEAL-FRAME-002), else `seal/bad_signature`.
///
/// Re-checks the layout of step 1 first (a frame that fails it is
/// `seal/frame_format`). The caller chooses `sign_pk` from the header's
/// `dev` and `M_{s−1}` (step 3).
pub(crate) fn verify_frame_signature(frame: &[u8], sign_pk: &[u8; 32]) -> Result<(), SealError> {
    frame_header(frame)?;
    let (signed, sig) = signed_part(frame).ok_or(SealError::FrameFormat)?;
    crate::signatures::verify_strict_parts(sign_pk, &[ctx::FRAME, signed], sig).map_err(|_| SealError::BadSignature)
}

/// The fixed fields of a body (seal.md §9.1) that steps 8 and 9 read
/// before decryption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EnvelopeHeader {
    pub ver: u8,
    pub suite: u8,
    pub epoch: u32,
}

/// Reads `ver`, `suite` and `epoch` of a frame that passed `frame_header`
/// (else `seal/frame_format`). Call it only after step 4 (SEAL-GEN-009):
/// these are fields the signature protects. Whether (`ver`, `suite`) is
/// allowed (step 8, `seal/bad_version`) and the epoch accepted (step 9,
/// `seal/bad_epoch`, `seal/old_epoch`) is the caller's decision.
pub(crate) fn envelope_header(frame: &[u8]) -> Result<EnvelopeHeader, SealError> {
    frame_header(frame)?;
    let bad = SealError::FrameFormat;
    let b = take::<6>(frame, HEADER_LEN).ok_or(bad)?;
    let [ver, suite, e0, e1, e2, e3] = *b;
    Ok(EnvelopeHeader { ver, suite, epoch: u32::from_be_bytes([e0, e1, e2, e3]) })
}

/// §14.2 step 10: opens the body under `key` (`K_entry,epoch` for the
/// frame's epoch) with the AD of §9.2 rebuilt from the frame's own header,
/// `ver`, `suite` and `epoch` (SEAL-ENV-002), else `seal/aead`. Returns the
/// padded plaintext, for `unpad`.
///
/// Re-checks the layout of step 1 (`seal/frame_format`); the signature
/// must have been verified first (SEAL-GEN-009).
pub(crate) fn open_envelope(frame: &[u8], key: &SymKey) -> Result<Vec<u8>, SealError> {
    let hdr = frame_header(frame)?;
    let env = envelope_header(frame)?;
    let bad = SealError::Aead;
    let header = take::<HEADER_LEN>(frame, 0).ok_or(SealError::FrameFormat)?;
    let nonce = take::<NONCE_LEN>(frame, HEADER_LEN + 6).ok_or(bad)?;
    let body_end = HEADER_LEN.checked_add(usize::try_from(hdr.len).map_err(|_| bad)?).ok_or(bad)?;
    let ct = frame.get(HEADER_LEN + BODY_PREFIX..body_end).ok_or(bad)?;
    let ad = entry_ad(header, env.ver, env.suite, env.epoch);
    cipher(key)?.decrypt(&XNonce::from(*nonce), Payload { msg: ct, aad: &ad }).map_err(|_| bad)
}

/// Which of the five checks of §10.3 failed, numbered as there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnpadCheck {
    /// 1: `L ≥ 69`.
    Length = 1,
    /// 2: `U = 69 + payload_len ≤ L`.
    PayloadLen = 2,
    /// 3: `P(U) = L`.
    Padded = 3,
    /// 4: every byte from `U` to `L` is zero.
    Zeros = 4,
    /// 5: `kind` is 1 to 4 and `at_ms` 0 to 253,402,300,799,999.
    KindTime = 5,
}

/// The five checks of §10.3 in order (SEAL-PAD-003), naming the one that
/// failed. `unpad` is the step-11 form.
pub(crate) fn unpad_checked(pt: &[u8]) -> Result<Plain, UnpadCheck> {
    let l = pt.len() as u64;
    // 1.
    if l < PLAIN_PREFIX {
        return Err(UnpadCheck::Length);
    }
    // 2. Computed in u64: 69 + a u32 cannot overflow.
    let payload_len = u32::from_be_bytes(*take::<4>(pt, 65).ok_or(UnpadCheck::Length)?);
    let u = PLAIN_PREFIX + u64::from(payload_len);
    if u > l {
        return Err(UnpadCheck::PayloadLen);
    }
    // 3.
    if padded_len(u) != l {
        return Err(UnpadCheck::Padded);
    }
    // 4. `u ≤ l = pt.len()`, so both conversions and the slices hold.
    let u_us = usize::try_from(u).map_err(|_| UnpadCheck::PayloadLen)?;
    if pt.get(u_us..).ok_or(UnpadCheck::PayloadLen)?.iter().any(|&b| b != 0) {
        return Err(UnpadCheck::Zeros);
    }
    // 5.
    let kind = Kind::from_byte(*pt.get(16).ok_or(UnpadCheck::Length)?).ok_or(UnpadCheck::KindTime)?;
    let at_ms = i64::from_be_bytes(*take::<8>(pt, 17).ok_or(UnpadCheck::Length)?);
    if !(0..=AT_MS_MAX).contains(&at_ms) {
        return Err(UnpadCheck::KindTime);
    }
    Ok(Plain {
        entry_id: EntryId(*take::<16>(pt, 0).ok_or(UnpadCheck::Length)?),
        kind,
        at_ms,
        based_on: Head {
            seq: u64::from_be_bytes(*take::<8>(pt, 25).ok_or(UnpadCheck::Length)?),
            chain: Chain(*take::<32>(pt, 33).ok_or(UnpadCheck::Length)?),
        },
        payload: pt.get(69..u_us).ok_or(UnpadCheck::PayloadLen)?.to_vec(),
    })
}

/// §14.2 step 11: the unpadding checks of §10.3 in order (SEAL-PAD-003),
/// else `seal/bad_plaintext`. The `based_on` checks (step 12) are the
/// caller's. A `cmd` payload is returned unread (SEAL-KIND-002).
pub(crate) fn unpad(pt: &[u8]) -> Result<Plain, SealError> {
    unpad_checked(pt).map_err(|_| SealError::BadPlaintext)
}

/// One control item (seal.md §12, §26).
#[allow(clippy::large_enum_variant)] // the spec's variants are unboxed (seal.md §26)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CtlItem {
    Genesis(Genesis),
    DeviceAdded(DeviceAdded),
    DeviceRevoked(DevId),
    DeviceReinstated(DevId),
    EpochRotated(EpochRotated),
    RecoveryKeyChange(RecoveryKeyChange),
    Fence(Fence),
    ResetTo(ResetTo),
    Skip(SkipItem),
    RulesUpgrade(RulesUpgrade),
}

/// The `Genesis` item (seal.md §12.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Genesis {
    pub salt: [u8; 16],
    pub recovery: RecoveryPublic,
    pub device: GenesisDevice,
    pub epoch: u32,
    pub commit: [u8; 32],
    pub wraps: Vec<[u8; 96]>,
    pub rules: u32,
    pub hash_version: u32,
    pub state_hash: [u8; 32],
    pub snapshot: Option<GenesisSnapshot>,
}

/// The `device` object of `Genesis` (seal.md §12.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenesisDevice {
    pub dev: DevId,
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
    pub name: String,
}

/// The optional `snapshot` object of `Genesis` (seal.md §12.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenesisSnapshot {
    pub blob: BlobId,
    pub sha256: [u8; 32],
    pub size: u64,
    pub parts: u32,
    pub inflated: u64,
}

/// The `DeviceAdded` item (seal.md §12.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAdded {
    pub dev: DevId,
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
    pub name: String,
    pub transcript: [u8; 32],
    pub bootstrap_seq: u64,
}

/// The `EpochRotated` item (seal.md §12.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpochRotated {
    pub epoch: u32,
    pub commit: [u8; 32],
    pub wraps: Vec<[u8; 96]>,
    pub purge_before: Option<u64>,
}

/// The `RecoveryKeyChange` item (seal.md §12.6): the new recovery key,
/// installed by the current recovery identity (§13.11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryKeyChange {
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
}

/// The `Fence` item (seal.md §12.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    pub host: String,
    pub door: [u8; 5],
    pub home_gen: u32,
}

/// The `ResetTo` item (seal.md §12.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetTo {
    pub seq: u64,
    pub state_hash: [u8; 32],
    pub hash_version: u32,
}

/// The `Skip` item (seal.md §12.9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkipItem {
    pub seq: u64,
    pub frame_hash: FrameHash,
}

/// The `RulesUpgrade` item (seal.md §12.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesUpgrade {
    pub rules: u32,
    pub mode: RulesUpgradeMode,
}

/// The `mode` of `RulesUpgrade` (seal.md §12.10, §13.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RulesUpgradeMode {
    Normal,
    Override,
}

/// The beat payload (seal.md §11.4, §26).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beat {
    pub rules: Vec<u32>,
    pub envelopes: Vec<(u8, u8)>,
    pub hash_versions: Vec<u32>,
    pub state: Option<BeatState>,
    pub acks: Vec<BeatAck>,
}

/// The `state` object of a beat (seal.md §11.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeatState {
    pub seq: u64,
    pub hash: [u8; 32],
    pub hash_version: u32,
}

/// One `acks` object of a beat (seal.md §11.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeatAck {
    pub entry: u64,
    pub verdict: AckVerdict,
}

/// The `verdict` of a beat ack (seal.md §11.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AckVerdict {
    Ack,
    Mismatch,
}

/// The snapshot announcement payload (seal.md §11.5, §26).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapAnnounce {
    pub seq: u64,
    pub hash: [u8; 32],
    pub hash_version: u32,
    pub blob: BlobId,
    pub sha256: [u8; 32],
    pub size: u64,
    pub parts: u32,
    pub inflated: u64,
    pub epoch: u32,
}

/// The control payload with its inner signature (seal.md §11.3, §26).
///
/// Refuses: 0 or more than 16 items, or an item its parser of §12 rejects,
/// with `seal/bad_payload` (a Fence host outside §5.4 with
/// `seal/host_format`, SEAL-ENC-006); items that form no shape of §12.1
/// with `seal/ctl_shape`; a `Signer::Recovery` of another board with
/// `seal/unknown_author`. The bounds an item states relative to the
/// entry's seq cannot be checked here (the seq is not known when sealing).
pub fn ctl_payload(
    items: &[CtlItem],
    signer: &Signer,
    board: &BoardId,
    ctr: u64,
    entry_id: &EntryId,
    at_ms: i64,
) -> Result<Vec<u8>, SealError> {
    let (dev, sk) = signer_identity(signer, board)?;
    let (items_value, items_bytes) = payloads::items_bytes(items)?;
    payloads::ctl_shape(items)?;
    let inner = payloads::ctl_inner(board, &dev, ctr, entry_id, at_ms, &items_bytes);
    let sig = sign_with(sk, &[&inner]);
    crate::canon::encode(&payloads::ctl_value(items_value, &sig)).map_err(|_| SealError::BadPayload)
}

/// The beat payload's JSON (seal.md §11.4, §26), or `seal/bad_payload` for
/// a beat its parser rejects. The bounds that need the entry (`state.seq ≤
/// based_on_seq`, `acks.entry` below the beat's seq) are the receiver's.
pub fn beat_payload(beat: &Beat) -> Result<Vec<u8>, SealError> {
    payloads::encode_beat(beat)
}

/// The snapshot announcement's JSON (seal.md §11.5, §26), or
/// `seal/bad_payload` for one its parser rejects. The bounds that need the
/// entry (`seq ≤ based_on_seq`, `epoch` equal to the frame's) are the
/// receiver's.
pub fn snap_payload(snap: &SnapAnnounce) -> Result<Vec<u8>, SealError> {
    payloads::encode_snap(snap)
}

#[cfg(test)]
mod tests;
