//! Sealed local files (seal.md §19, §26).
//!
//! A file is the 55-byte header of §19.1 followed by records `ct_len ‖ nonce
//! ‖ ciphertext`, each sealed under `K_file(kind)` (§6.6, SEAL-KEY-014) with
//! `AD(i) = ctx("file") ‖ header ‖ u64 i` (§19.3). `open_file` reads in the
//! order of §19.4 (SEAL-FILE-003).

use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use crate::prim::{hkdf32, take};
use crate::{BoardId, DevId, Entropy, SealError, ctx};

/// `"TDL1"` (seal.md §19.1).
const MAGIC: &[u8; 4] = b"TDL1";
/// The header (seal.md §19.1).
pub(crate) const HEADER_LEN: usize = 55;
/// `ct_len` and `nonce` before each ciphertext (seal.md §19.1).
const RECORD_PREFIX: usize = 4 + NONCE_LEN;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
/// The bounds of `ct_len` (seal.md §19.1, §23).
const CT_MIN: u32 = 16;
const CT_MAX: u32 = 67_108_880;

/// A sealed file's kind byte (seal.md §19.2, §26).
#[repr(u8)]
pub enum FileKind {
    Log = 1,
    Checkpoint = 2,
    Base = 3,
    Hashes = 4,
    Outbox = 5,
    Overlay = 6,
    Identity = 7,
    Notices = 8,
    Meta = 9,
    Marker = 10,
    History = 11,
    Device = 12,
    Picture = 13,
    Weather = 14,
    Draft = 15,
    Feed = 16,
    Bootstrap = 17,
    Unsent = 18,
    OwnPicture = 19,
}

/// The kinds of §19.2 in force: 1 to 19; 20 to 255 are reserved.
fn kind_is_known(k: u8) -> bool {
    (1..=19).contains(&k)
}

/// The storage key `L_d` (seal.md §6.6, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct StorageKey {
    key: [u8; 32],
}

impl StorageKey {
    /// Draws a new `L_d`: exactly 32 bytes from `rng` (seal.md §6.6).
    pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError> {
        let mut k = StorageKey { key: [0u8; 32] };
        rng.fill(&mut k.key)?;
        Ok(k)
    }

    /// `L_d` from its bytes (seal.md §6.6).
    pub fn from_bytes(b: [u8; 32]) -> Self {
        StorageKey { key: b }
    }

    /// `K_file(kind) = HKDF(salt = board_id, ikm = L_d, info = ctx("file-key")
    /// ‖ u8 kind ‖ dev, L = 32)` (seal.md §6.6, SEAL-KEY-014).
    pub(crate) fn file_key(&self, kind: u8, board: &BoardId, dev: &DevId) -> Zeroizing<[u8; 32]> {
        hkdf32(&board.0, &self.key, &[ctx::FILE_KEY, &[kind], &dev.0])
    }
}

/// The header of §19.1.
fn header(kind: u8, dev: &DevId, board: &BoardId, file_id: &[u8; 16]) -> Vec<u8> {
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(MAGIC);
    h.push(1);
    h.push(1);
    h.push(kind);
    h.extend_from_slice(&dev.0);
    h.extend_from_slice(&board.0);
    h.extend_from_slice(file_id);
    h
}

/// The fields of a header: `(kind, dev, board)`, when the bytes are exactly
/// a header of §19.1 with `magic`, `ver = 1`, `suite = 1` and a kind of
/// §19.2.
fn parse_header(h: &[u8]) -> Option<(u8, DevId, BoardId)> {
    if h.len() != HEADER_LEN || take::<4>(h, 0)? != MAGIC {
        return None;
    }
    let [ver, suite, kind] = *take::<3>(h, 4)?;
    if ver != 1 || suite != 1 || !kind_is_known(kind) {
        return None;
    }
    Some((kind, DevId(*take::<16>(h, 7)?), BoardId(*take::<16>(h, 23)?)))
}

/// `AD(record i) = ctx("file") ‖ header[55] ‖ u64 i` (seal.md §19.3).
fn record_ad(header: &[u8], i: u64) -> Vec<u8> {
    let mut ad = Vec::with_capacity(ctx::FILE.len() + HEADER_LEN + 8);
    ad.extend_from_slice(ctx::FILE);
    ad.extend_from_slice(header);
    ad.extend_from_slice(&i.to_be_bytes());
    ad
}

fn cipher(key: &[u8; 32]) -> Result<XChaCha20Poly1305, SealError> {
    XChaCha20Poly1305::new_from_slice(key).map_err(|_| SealError::FileCorrupt)
}

/// Seals the records of one file: its header and next record index (seal.md §19.1, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct FileSealer {
    file_key: [u8; 32],
    header: Vec<u8>,
    next: u64,
}

impl FileSealer {
    /// Starts a new file and returns its header bytes (seal.md §19.1,
    /// SEAL-FILE-002): draws exactly the 16-byte `file_id` from `rng`. The
    /// next record is record 0.
    pub fn create(
        key: &StorageKey,
        kind: FileKind,
        board: &BoardId,
        dev: &DevId,
        rng: &mut dyn Entropy,
    ) -> Result<(Self, Vec<u8>), SealError> {
        let kind = kind as u8;
        let mut file_id = [0u8; 16];
        rng.fill(&mut file_id)?;
        let h = header(kind, dev, board, &file_id);
        let fk = key.file_key(kind, board, dev);
        Ok((FileSealer { file_key: *fk, header: h.clone(), next: 0 }, h))
    }

    /// Continues a file whose first `records` records exist (seal.md §19.1):
    /// the next record is record `records`. `header` must be exactly a header
    /// of §19.1, else `seal/file_header`; the key is the one its kind, `dev`
    /// and `board_id` name (§6.6).
    pub fn resume(key: &StorageKey, header: &[u8], records: u64) -> Result<Self, SealError> {
        let (kind, dev, board) = parse_header(header).ok_or(SealError::FileHeader)?;
        let fk = key.file_key(kind, &board, &dev);
        Ok(FileSealer { file_key: *fk, header: header.to_vec(), next: records })
    }

    /// Seals the next record (seal.md §19.1, §19.3): draws exactly a fresh
    /// 24-byte nonce from `rng` (SEAL-GEN-012) and returns `ct_len ‖ nonce ‖
    /// ciphertext`. The index advances only when a record is returned.
    ///
    /// A plaintext over 67,108,864 bytes (a `ct_len` above its limit of §23)
    /// is `seal/payload_too_large`, before anything is drawn.
    // SPEC-QUESTION: §22 names no code for a sealed-file record over its
    // limit; this takes `seal/payload_too_large`, the sealing code of a
    // plaintext over its cap. Likewise a file whose record index would pass
    // 2^64 − 1 (no file reaches it).
    pub fn seal_record(&mut self, plaintext: &[u8], rng: &mut dyn Entropy) -> Result<Vec<u8>, SealError> {
        let ct_len = plaintext
            .len()
            .checked_add(TAG_LEN)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n <= CT_MAX)
            .ok_or(SealError::PayloadTooLarge)?;
        let after = self.next.checked_add(1).ok_or(SealError::PayloadTooLarge)?;
        let mut nonce = [0u8; NONCE_LEN];
        rng.fill(&mut nonce)?;
        let ad = record_ad(&self.header, self.next);
        let ct = cipher(&self.file_key)?
            .encrypt(&XNonce::from(nonce), Payload { msg: plaintext, aad: &ad })
            .map_err(|_| SealError::PayloadTooLarge)?;
        if u32::try_from(ct.len()).ok() != Some(ct_len) {
            return Err(SealError::PayloadTooLarge);
        }
        let mut out = Vec::with_capacity(RECORD_PREFIX + ct.len());
        out.extend_from_slice(&ct_len.to_be_bytes());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        self.next = after;
        Ok(out)
    }
}

/// An opened file's records and tail (seal.md §19.4, §26).
pub struct OpenedFile {
    pub records: Vec<zeroize::Zeroizing<Vec<u8>>>,
    pub tail: FileTail,
}

/// How a file ends (seal.md §19.4, §26). `valid_bytes` is the length of the
/// header and the records that opened: the offset of the tail.
pub enum FileTail {
    Clean,
    /// `seal/file_torn_tail`: an incomplete last record.
    Torn { valid_bytes: u64 },
    /// `seal/file_bad_tail`: a complete last record that does not open.
    Bad { valid_bytes: u64 },
}

/// Reads a sealed file (seal.md §19.4, SEAL-FILE-003, §26), in order:
///
/// 1. at least 55 bytes, and a header with `magic = "TDL1"`, `ver = 1`,
///    `suite = 1`, the expected `kind`, `dev` and `board_id`, else
///    `seal/file_header`;
/// 2. for each record from `i = 0`: fewer than 28 bytes left, a `ct_len`
///    outside 16 to 67,108,880, or fewer than `28 + ct_len` bytes left make
///    the rest a torn tail; the length is checked before any byte of the
///    ciphertext is read or allocated (SEAL-GEN-008); otherwise the record
///    is opened under `K_file(kind)` with `AD(i)` (SEAL-KEY-014);
/// 3. a record that does not open is `seal/file_corrupt` when bytes follow
///    it, a bad tail when it is the last.
///
/// Every record plaintext is returned in a zeroizing buffer (SEAL-FILE-004).
// SPEC-QUESTION: §19.4 says the reader "returns the records that opened,
// in order, with the outcome for the rest", and §26 returns
// `Result<OpenedFile, _>` whose `FileTail` has no corrupt variant: for
// `seal/file_corrupt` this returns the error alone, without the records
// before the corrupt one.
pub fn open_file(
    key: &StorageKey,
    kind: FileKind,
    board: &BoardId,
    dev: &DevId,
    bytes: &[u8],
) -> Result<OpenedFile, SealError> {
    let kind = kind as u8;
    let hdr = bytes.get(..HEADER_LEN).ok_or(SealError::FileHeader)?;
    match parse_header(hdr) {
        Some((k, d, b)) if k == kind && d == *dev && b == *board => {}
        _ => return Err(SealError::FileHeader),
    }
    let fk = key.file_key(kind, board, dev);
    let aead = cipher(&fk)?;
    let mut records = Vec::new();
    let mut at = HEADER_LEN;
    let mut i: u64 = 0;
    while at < bytes.len() {
        let Some((nonce, ct, end)) = record_at(bytes, at) else {
            return Ok(OpenedFile { records, tail: FileTail::Torn { valid_bytes: at as u64 } });
        };
        let ad = record_ad(hdr, i);
        match aead.decrypt(&XNonce::from(*nonce), Payload { msg: ct, aad: &ad }) {
            Ok(pt) => records.push(Zeroizing::new(pt)),
            Err(_) if end < bytes.len() => return Err(SealError::FileCorrupt),
            Err(_) => return Ok(OpenedFile { records, tail: FileTail::Bad { valid_bytes: at as u64 } }),
        }
        at = end;
        i = i.checked_add(1).ok_or(SealError::FileCorrupt)?;
    }
    Ok(OpenedFile { records, tail: FileTail::Clean })
}

/// The record at offset `at`: its nonce, its ciphertext and the offset after
/// it; `None` when the rest of the file is a torn tail (§19.4 step 2). The
/// `ct_len` is checked against its limit before it is used as a length, then
/// against the bytes left (SEAL-GEN-008).
fn record_at(bytes: &[u8], at: usize) -> Option<(&[u8; NONCE_LEN], &[u8], usize)> {
    let len = u32::from_be_bytes(*take::<4>(bytes, at)?);
    let nonce = take::<NONCE_LEN>(bytes, at.checked_add(4)?)?;
    if !(CT_MIN..=CT_MAX).contains(&len) {
        return None;
    }
    let start = at.checked_add(RECORD_PREFIX)?;
    let end = start.checked_add(usize::try_from(len).ok()?)?;
    Some((nonce, bytes.get(start..end)?, end))
}

#[cfg(test)]
mod tests;
