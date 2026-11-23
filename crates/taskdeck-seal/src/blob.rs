//! Snapshots and blobs (seal.md §20, §26).
//!
//! A sealed blob is the 51-byte header of §20.2 and one XChaCha20-Poly1305
//! message of the padded body of §20.3 under `K_snap,e` (kind 1) or
//! `K_blob,e` (kind 2), with the AD of §20.2. A snapshot's body is the raw
//! DEFLATE of the layout of §20.4. The openers run the steps of §20.5 in
//! order and stop at the first that fails; nothing is decrypted before the
//! SHA-256 of the parts is checked (SEAL-BLOB-002), and no field of the body
//! is parsed before the AEAD has opened (SEAL-GEN-009).

use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq;

use crate::backup::{RECORD_FIXED, agrees, projection};
use crate::membership::Reader;
use crate::prim::{deflate_raw, h, inflate_raw, take};
use crate::{
    BlobId, BoardId, Chain, DevId, Entropy, EpochRoot, HistoryRecord, KeyKind, Membership, SealError, SymKey, ctx,
    derive_epoch_key, padded_len,
};

/// `"TDB1"` (seal.md §20.2).
const MAGIC: &[u8; 4] = b"TDB1";
/// `"TDSN"` (seal.md §20.4).
const BODY_MAGIC: &[u8; 4] = b"TDSN";
/// `ver` and `suite` (seal.md §20.2).
const VER: u8 = 1;
const SUITE: u8 = 1;
/// `blob_kind` (seal.md §20.2).
const KIND_SNAPSHOT: u8 = 1;
const KIND_BLOB: u8 = 2;
/// The bytes of the header the AD binds, `sealed[0..27]` (seal.md §20.2).
const AD_HEADER: usize = 27;
/// The header, magic to nonce (seal.md §20.2).
const HEADER_LEN: usize = 51;
const ID_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
/// The header and the tag: a sealed blob is `67 + padded length` bytes.
const OVERHEAD: usize = HEADER_LEN + TAG_LEN;
/// The largest sealed blob (seal.md §20.2, §23).
const SEALED_MAX: usize = 67_108_864;
/// A part (seal.md §20.5, §23).
const PART_LEN: usize = 1_048_576;
/// The largest snapshot body before compression (seal.md §20.4, §23).
const INFLATED_MAX: usize = 67_108_864;
/// History records per snapshot (seal.md §23).
const HIST_MAX: usize = 1_048_576;
/// The fixed part of a snapshot body: magic, `S`, `chain`, `hash_version`,
/// `state_hash`, and the three length fields (seal.md §20.4).
const BODY_FIXED: usize = 4 + 8 + 32 + 4 + 32 + 4 + 4 + 4;

/// A blob kind (seal.md §20.2, §26).
pub enum BlobKind {
    Snapshot = 1,
    Picture = 2,
}

/// A sealed blob: id, bytes, hash and part count (seal.md §20.2, §26).
pub struct SealedBlob {
    pub id: BlobId,
    pub bytes: Vec<u8>,
    pub sha256: [u8; 32],
    pub parts: u32,
}

/// What a reader expects of a blob before decrypting (seal.md §20.5, §26).
pub struct BlobRef {
    pub id: BlobId,
    pub sha256: [u8; 32],
    pub size: u64,
    pub parts: u32,
    pub inflated: u64,
    pub epoch: u32,
}

/// A snapshot body (seal.md §20.4, §26).
pub struct SnapshotBody {
    pub seq: u64,
    pub chain: Chain,
    pub hash_version: u32,
    pub state_hash: [u8; 32],
    pub board: Vec<u8>,
    pub membership: Vec<u8>,
    pub history: Vec<HistoryRecord>,
}

/// `AD = ctx("blob") ‖ board_id ‖ sealed[0..27] ‖ u64 S ‖ writer_dev`
/// (seal.md §20.2, SEAL-BLOB-001).
fn blob_ad(board: &BoardId, header: &[u8; AD_HEADER], s: u64, writer: &DevId) -> Vec<u8> {
    let mut ad = Vec::with_capacity(ctx::BLOB.len() + 16 + AD_HEADER + 8 + 16);
    ad.extend_from_slice(ctx::BLOB);
    ad.extend_from_slice(&board.0);
    ad.extend_from_slice(header);
    ad.extend_from_slice(&s.to_be_bytes());
    ad.extend_from_slice(&writer.0);
    ad
}

fn cipher(key: &SymKey) -> Option<XChaCha20Poly1305> {
    XChaCha20Poly1305::new_from_slice(key.as_bytes()).ok()
}

/// Seals `body` as blob kind `kind` (seal.md §20.1 to §20.3): checks the
/// size first (`too_large`, nothing drawn), then draws the `blob_id` (16
/// bytes) and the nonce (24 bytes), in that order and nothing else
/// (SEAL-BLOB-010, SEAL-BLOB-005, SEAL-GEN-012).
#[allow(clippy::too_many_arguments)]
fn seal(
    kind: u8,
    body: &[u8],
    key: &SymKey,
    board: &BoardId,
    epoch: u32,
    s: u64,
    writer: &DevId,
    rng: &mut dyn Entropy,
    too_large: SealError,
) -> Result<SealedBlob, SealError> {
    let body_len = u32::try_from(body.len()).map_err(|_| too_large)?;
    let p = padded_len(4 + u64::from(body_len));
    let p = usize::try_from(p).map_err(|_| too_large)?;
    let size = p.checked_add(OVERHEAD).filter(|n| *n <= SEALED_MAX).ok_or(too_large)?;
    let mut id = [0u8; ID_LEN];
    rng.fill(&mut id)?;
    let mut nonce = [0u8; NONCE_LEN];
    rng.fill(&mut nonce)?;
    let mut header = [0u8; AD_HEADER];
    {
        let (m, rest) = header.split_at_mut(4);
        m.copy_from_slice(MAGIC);
        let (v, rest) = rest.split_at_mut(3);
        v.copy_from_slice(&[VER, SUITE, kind]);
        let (e, i) = rest.split_at_mut(4);
        e.copy_from_slice(&epoch.to_be_bytes());
        i.copy_from_slice(&id);
    }
    let mut padded = Vec::with_capacity(p);
    padded.extend_from_slice(&body_len.to_be_bytes());
    padded.extend_from_slice(body);
    padded.resize(p, 0);
    let ad = blob_ad(board, &header, s, writer);
    let ct = cipher(key)
        .ok_or(too_large)?
        .encrypt(&XNonce::from(nonce), Payload { msg: &padded, aad: &ad })
        .map_err(|_| too_large)?;
    let mut bytes = Vec::with_capacity(size);
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&nonce);
    bytes.extend_from_slice(&ct);
    if bytes.len() != size {
        return Err(too_large);
    }
    let parts = u32::try_from(size.div_ceil(PART_LEN)).map_err(|_| too_large)?;
    Ok(SealedBlob { id: BlobId(id), sha256: h(&[&bytes]), bytes, parts })
}

/// The canonical bytes of a snapshot body (seal.md §20.4), or
/// `seal/snapshot_large` when they would exceed 67,108,864 bytes; the length
/// is computed before anything is written.
fn encode_body(body: &SnapshotBody) -> Result<Vec<u8>, SealError> {
    let big = SealError::SnapshotLarge;
    let mut len = BODY_FIXED.checked_add(body.board.len()).ok_or(big)?.checked_add(body.membership.len()).ok_or(big)?;
    for r in &body.history {
        len = len.checked_add(RECORD_FIXED).and_then(|n| n.checked_add(r.items.len())).ok_or(big)?;
    }
    if len > INFLATED_MAX {
        return Err(big);
    }
    let mut o = Vec::with_capacity(len);
    o.extend_from_slice(BODY_MAGIC);
    o.extend_from_slice(&body.seq.to_be_bytes());
    o.extend_from_slice(&body.chain.0);
    o.extend_from_slice(&body.hash_version.to_be_bytes());
    o.extend_from_slice(&body.state_hash);
    for x in [&body.board, &body.membership] {
        o.extend_from_slice(&u32::try_from(x.len()).map_err(|_| big)?.to_be_bytes());
        o.extend_from_slice(x);
    }
    o.extend_from_slice(&u32::try_from(body.history.len()).map_err(|_| big)?.to_be_bytes());
    for r in &body.history {
        r.write(&mut o);
    }
    Ok(o)
}

/// Seals a snapshot, blob kind 1 (seal.md §20.2 to §20.4, §26).
///
/// Refuses, before anything is drawn (ruling Q37: a body its reader would
/// reject): `membership` bytes that do not parse (`seal/membership_format`);
/// a `history` whose seqs are not exactly the membership's `ctl_seqs`, in
/// order, starting at seq 1 (`seal/blob_open`, SEAL-BLOB-006); a body over
/// 67,108,864 bytes before compression, or a sealed blob that would exceed
/// 67,108,864 bytes (`seal/snapshot_large`, no sealed bytes, SEAL-BLOB-009).
/// It does not run the projection (that is the reader's, §20.5 step 7).
///
/// Then: the body of §20.4 compressed with raw DEFLATE (a snapshot body
/// holds no key), padded as §20.3 and sealed under `key` (`K_snap,e`) with
/// the AD of §20.2 for `S = body.seq` and `writer` (SEAL-BLOB-001,
/// SEAL-BLOB-004); `blob_id` then nonce are the only draws (SEAL-BLOB-010).
pub fn seal_snapshot(
    body: &SnapshotBody,
    key: &SymKey,
    board: &BoardId,
    epoch: u32,
    writer: &DevId,
    rng: &mut dyn Entropy,
) -> Result<SealedBlob, SealError> {
    let m = Membership::from_canonical_bytes(&body.membership)?;
    let starts_at_1 = body.history.first().is_some_and(|r| r.seq == 1);
    if !starts_at_1 || !body.history.iter().map(|r| r.seq).eq(m.ctl_seqs.iter().copied()) {
        return Err(SealError::BlobOpen);
    }
    let bytes = encode_body(body)?;
    let compressed = deflate_raw(&bytes).ok_or(SealError::SnapshotLarge)?;
    seal(KIND_SNAPSHOT, &compressed, key, board, epoch, body.seq, writer, rng, SealError::SnapshotLarge)
}

/// The key of step 4: the caller's, or the declared mode's from its roots.
enum KeySource<'k, 'a> {
    Given(&'k SymKey),
    Roots(&'k dyn Fn(u32) -> Option<&'a EpochRoot>),
}

/// What steps 1 to 5 check.
struct Check<'k, 'a> {
    sha256: &'k [u8; 32],
    size: u64,
    parts: u32,
    kind: u8,
    /// The expected epoch; `None` in the declared mode (read from the header).
    epoch: Option<u32>,
    key: KeySource<'k, 'a>,
    s: u64,
    writer: &'k DevId,
    board: &'k BoardId,
}

/// Steps 1 to 5 of §20.5, in order, each with its code. Returns the body of
/// §20.3 (without `body_len` and padding) and the header's epoch.
fn open_sealed(parts: &[&[u8]], c: &Check<'_, '_>) -> Result<(Vec<u8>, u32), SealError> {
    // 1. Exactly `parts` parts, each 1,048,576 bytes but the last (1 to
    // 1,048,576), `size` bytes in all, and `size` within §23.
    let size = usize::try_from(c.size).map_err(|_| SealError::BlobSize)?;
    if !(OVERHEAD..=SEALED_MAX).contains(&size) || usize::try_from(c.parts).ok() != Some(parts.len()) {
        return Err(SealError::BlobSize);
    }
    let mut total = 0usize;
    for (i, p) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        let ok = if last { (1..=PART_LEN).contains(&p.len()) } else { p.len() == PART_LEN };
        if !ok {
            return Err(SealError::BlobSize);
        }
        total = total.checked_add(p.len()).ok_or(SealError::BlobSize)?;
    }
    if total != size {
        return Err(SealError::BlobSize);
    }
    // 2. The SHA-256 of the concatenation, in constant time, before anything
    // is decrypted (SEAL-BLOB-002).
    let mut hasher = Sha256::new();
    for p in parts {
        hasher.update(p);
    }
    let got: [u8; 32] = hasher.finalize().into();
    if !bool::from(got.ct_eq(c.sha256)) {
        return Err(SealError::BlobHash);
    }
    let mut sealed = Vec::with_capacity(size);
    for p in parts {
        sealed.extend_from_slice(p);
    }
    // 3. The header: magic, ver, suite, kind, and the epoch.
    let bad = SealError::BlobHeader;
    if take::<4>(&sealed, 0) != Some(MAGIC) {
        return Err(bad);
    }
    let [ver, suite, kind] = *take::<3>(&sealed, 4).ok_or(bad)?;
    let epoch = u32::from_be_bytes(*take::<4>(&sealed, 7).ok_or(bad)?);
    if ver != VER || suite != SUITE || kind != c.kind || c.epoch.is_some_and(|e| e != epoch) {
        return Err(bad);
    }
    // 4. The AEAD with the AD of §20.2, under the kind's key for that epoch.
    let derived;
    let key = match c.key {
        KeySource::Given(k) => k,
        KeySource::Roots(roots) => {
            let root = roots(epoch).ok_or(SealError::MissingRoot)?;
            derived = derive_epoch_key(root, c.board, epoch, KeyKind::Snap);
            &derived
        }
    };
    let open = SealError::BlobOpen;
    let header = take::<AD_HEADER>(&sealed, 0).ok_or(open)?;
    let nonce = take::<NONCE_LEN>(&sealed, AD_HEADER).ok_or(open)?;
    let ct = sealed.get(HEADER_LEN..).ok_or(open)?;
    let ad = blob_ad(c.board, header, c.s, c.writer);
    let padded =
        cipher(key).ok_or(open)?.decrypt(&XNonce::from(*nonce), Payload { msg: ct, aad: &ad }).map_err(|_| open)?;
    // 5. The padding, as §10.3 with `U = 4 + body_len`.
    Ok((unpad(&padded).ok_or(open)?.to_vec(), epoch))
}

/// The checks of §10.3 for a padded body of §20.3: `L ≥ 4`, `U = 4 +
/// body_len ≤ L`, `P(U) = L`, zeros from `U` to `L`. Returns the body.
fn unpad(padded: &[u8]) -> Option<&[u8]> {
    let l = padded.len();
    let body_len = usize::try_from(u32::from_be_bytes(*take::<4>(padded, 0)?)).ok()?;
    let u = body_len.checked_add(4)?;
    if u > l || usize::try_from(padded_len(u64::try_from(u).ok()?)).ok()? != l {
        return None;
    }
    if padded.get(u..)?.iter().any(|&b| b != 0) {
        return None;
    }
    padded.get(4..u)
}

/// `vb32` bytes, the length checked against the bytes left before reading.
fn vb32(r: &mut Reader<'_>) -> Option<Vec<u8>> {
    let n = usize::try_from(r.u32()?).ok()?;
    let n = r.count(n, INFLATED_MAX, 1)?;
    Some(r.bytes(n)?.to_vec())
}

/// The layout of §20.4, every length checked against its limit and the
/// bytes left before it is read, and no trailing byte.
fn parse_body(b: &[u8]) -> Option<SnapshotBody> {
    let mut r = Reader::new(b);
    if &r.arr::<4>()? != BODY_MAGIC {
        return None;
    }
    let seq = r.u64()?;
    let chain = Chain(r.arr()?);
    let hash_version = r.u32()?;
    let state_hash = r.arr()?;
    let board = vb32(&mut r)?;
    let membership = vb32(&mut r)?;
    let n = usize::try_from(r.u32()?).ok()?;
    let n = r.count(n, HIST_MAX, RECORD_FIXED)?;
    let mut history = Vec::with_capacity(n);
    for _ in 0..n {
        history.push(HistoryRecord::read(&mut r)?);
    }
    if r.remaining() != 0 {
        return None;
    }
    Some(SnapshotBody { seq, chain, hash_version, state_hash, board, membership, history })
}

/// The values step 7 compares with the body's (seal.md §20.5): `S` always;
/// `hash_version` and `state_hash` only in the normal mode, where they come
/// from the signed entry or the bundle (`None` in the declared mode).
struct Expect<'a> {
    s: u64,
    hash: Option<(&'a [u8; 32], u32)>,
}

/// Step 7 of §20.5 (`seal/blob_open`): the body parses with no trailing
/// bytes; its `S`, and with `expect.hash` its `hash_version` and
/// `state_hash`, equal the expected values; the membership parses (§13.2);
/// no history record is after `S`; the history verifies by the projection
/// of §21.6 for the receiver's own `board`; and it agrees with the
/// membership as §21.6 step 11 states (SEAL-BLOB-007). The state hash is not
/// recomputed from the body's `board` and `membership`: that is the
/// caller's, from the board it loads (`engine.md`; ruling Q57).
// SPEC-QUESTION: §20.4 says the history holds a record for each seq of
// `M_S.ctl_seqs`, which the agreement with `membership` checks; nothing
// states that those seqs are at or below `S`. As a backup's step 9 does for
// its header's seq, a record after `S` is rejected (the refusing reading).
fn verify_body(b: &[u8], expect: &Expect<'_>, board: &BoardId) -> Result<SnapshotBody, SealError> {
    let bad = SealError::BlobOpen;
    let body = parse_body(b).ok_or(bad)?;
    if body.seq != expect.s {
        return Err(bad);
    }
    if let Some((state_hash, hash_version)) = expect.hash {
        let same = body.state_hash.ct_eq(state_hash) & body.hash_version.ct_eq(&hash_version);
        if !bool::from(same) {
            return Err(bad);
        }
    }
    let m = Membership::from_canonical_bytes(&body.membership).map_err(|_| bad)?;
    if body.history.last().is_some_and(|r| r.seq > expect.s) {
        return Err(bad);
    }
    let p = projection::project(&body.history, board).map_err(|_| bad)?;
    if !agrees(&m, &p) {
        return Err(bad);
    }
    Ok(body)
}

/// Opens a snapshot against its reference (seal.md §20.5, §26): the seven
/// steps in order, each with its code.
///
/// 1. `expected.parts` parts of 1,048,576 bytes but the last, `size` in all
///    (`seal/blob_size`);
/// 2. their SHA-256 is `expected.sha256`, in constant time, before any
///    decryption (`seal/blob_hash`, SEAL-BLOB-002);
/// 3. `"TDB1"`, version 1, suite 1, kind 1 and `expected.epoch`
///    (`seal/blob_header`);
/// 4. the AEAD under `key` (`K_snap,e`) with the AD of §20.2 for `s` and
///    `writer` (`seal/blob_open`);
/// 5. the padding (`seal/blob_open`);
/// 6. inflate while streaming under a cap of `expected.inflated` (at most
///    67,108,864): more output, input after the stream's end, or less output
///    than `expected.inflated` is `seal/inflate` (SEAL-BLOB-003);
/// 7. the body (`verify_body`, `seal/blob_open`): its `S`, `hash_version`
///    and `state_hash` are `s`, `hash_version` and `state_hash` (the signed
///    entry's or the bundle's), its history verifies and agrees with its
///    membership.
///
/// The caller recomputes the state hash from the board it loads and
/// compares it with the signed one (`engine.md`); this does not.
#[allow(clippy::too_many_arguments)]
pub fn open_snapshot(
    parts: &[&[u8]],
    expected: &BlobRef,
    s: u64,
    state_hash: &[u8; 32],
    hash_version: u32,
    writer: &DevId,
    key: &SymKey,
    board: &BoardId,
) -> Result<SnapshotBody, SealError> {
    let check = Check {
        sha256: &expected.sha256,
        size: expected.size,
        parts: expected.parts,
        kind: KIND_SNAPSHOT,
        epoch: Some(expected.epoch),
        key: KeySource::Given(key),
        s,
        writer,
        board,
    };
    let (compressed, _) = open_sealed(parts, &check)?;
    // 6.
    let cap = usize::try_from(expected.inflated).unwrap_or(usize::MAX).min(INFLATED_MAX);
    let inflated = inflate_raw(&compressed, cap).ok_or(SealError::Inflate)?;
    if u64::try_from(inflated.len()).ok() != Some(expected.inflated) {
        return Err(SealError::Inflate);
    }
    // 7.
    verify_body(&inflated, &Expect { s, hash: Some((state_hash, hash_version)) }, board)
}

/// Opens a snapshot in declared mode: `(body, epoch, inflated)` (seal.md
/// §20.5 "Opening a declared snapshot", §26; SEAL-BLOB-008).
///
/// The steps of `open_snapshot` with these differences: step 1 takes the
/// declared `size` and `parts = ceil(size / 1,048,576)`; step 2 the declared
/// `sha256`; step 3 checks kind 1 and reads the epoch from the header; step
/// 4 derives `K_snap` only from the root `roots` gives for that epoch
/// (`seal/missing_root` when it gives none) and opens the AEAD before any
/// field of the body is parsed; step 6 inflates under 67,108,864 bytes with
/// no expected length; step 7 checks `S` against `s` and compares neither
/// `hash_version` nor `state_hash`. Nothing declared is trusted: the caller
/// checks every value against the signed `snap` entry once it has
/// classified it (`engine.md` §19.7 step 5).
// `<'a>`: the spec's `Option<&EpochRoot>` needs a named lifetime to compile.
pub fn open_snapshot_declared<'a>(
    parts: &[&[u8]],
    sha256: &[u8; 32],
    size: u64,
    s: u64,
    writer: &DevId,
    roots: &dyn Fn(u32) -> Option<&'a EpochRoot>,
    board: &BoardId,
) -> Result<(SnapshotBody, u32, u64), SealError> {
    let n = usize::try_from(size).ok().filter(|n| *n <= SEALED_MAX).ok_or(SealError::BlobSize)?;
    let n = u32::try_from(n.div_ceil(PART_LEN)).map_err(|_| SealError::BlobSize)?;
    let check = Check {
        sha256,
        size,
        parts: n,
        kind: KIND_SNAPSHOT,
        epoch: None,
        key: KeySource::Roots(roots),
        s,
        writer,
        board,
    };
    let (compressed, epoch) = open_sealed(parts, &check)?;
    // 6.
    let inflated = inflate_raw(&compressed, INFLATED_MAX).ok_or(SealError::Inflate)?;
    let len = u64::try_from(inflated.len()).map_err(|_| SealError::Inflate)?;
    // 7.
    let body = verify_body(&inflated, &Expect { s, hash: None }, board)?;
    Ok((body, epoch, len))
}

/// Seals a blob of kind 2 (seal.md §20.2, §20.3, §26): `body` padded as
/// §20.3 and sealed under `key` (`K_blob,e`) with the AD of §20.2 for
/// `S = 0` and `writer`; `blob_id` then nonce are the only draws
/// (SEAL-BLOB-010). A body whose sealed blob would exceed 67,108,864 bytes
/// is refused before anything is drawn.
// SPEC-QUESTION: §22 names no code for a kind-2 blob over the sealed-blob
// maximum (`seal/snapshot_large` is a snapshot's); this takes
// `seal/payload_too_large`, the code of a plaintext over its limit.
pub fn seal_blob(
    body: &[u8],
    key: &SymKey,
    board: &BoardId,
    epoch: u32,
    writer: &DevId,
    rng: &mut dyn Entropy,
) -> Result<SealedBlob, SealError> {
    seal(KIND_BLOB, body, key, board, epoch, 0, writer, rng, SealError::PayloadTooLarge)
}

/// Opens a blob of kind 2; `expected.inflated` is not read (seal.md §20.5,
/// §26): steps 1 to 5 of `open_snapshot` with kind 2, `S = 0` and `key`
/// (`K_blob,e`). Returns the body (the JPEG bytes, `device.md`).
pub fn open_blob(
    parts: &[&[u8]],
    expected: &BlobRef,
    writer: &DevId,
    key: &SymKey,
    board: &BoardId,
) -> Result<Vec<u8>, SealError> {
    let check = Check {
        sha256: &expected.sha256,
        size: expected.size,
        parts: expected.parts,
        kind: KIND_BLOB,
        epoch: Some(expected.epoch),
        key: KeySource::Given(key),
        s: 0,
        writer,
        board,
    };
    open_sealed(parts, &check).map(|(body, _)| body)
}

/// Splits sealed bytes into parts of 1,048,576 bytes, the last 1 to
/// 1,048,576 (seal.md §20.5, §26); no part for empty input.
pub fn split_parts(sealed: &[u8]) -> Vec<&[u8]> {
    sealed.chunks(PART_LEN).collect()
}

#[cfg(test)]
mod tests;
