//! Hash chain, head and fingerprints (seal.md §15, §26).

use subtle::ConstantTimeEq;

use crate::prim::h;
use crate::{BoardId, Chain, FrameHash, Head, SealError, base32, ctx};

/// The BIP-39 English word list, embedded at build time (seal.md §15.3,
/// SEAL-FP-002): a byte-for-byte copy of `docs/sync/spec/wordlist.txt`.
pub(crate) const WORDLIST: &str = include_str!("wordlist.txt");

/// `C_0 = H(ctx("chain") ‖ board_id)` (seal.md §15.1, SEAL-CHAIN-001, §26).
pub fn chain_zero(board: &BoardId) -> Chain {
    Chain(h(&[ctx::CHAIN, &board.0]))
}

/// `C_s = H(ctx("chain") ‖ C_{s−1} ‖ u64 s ‖ H(frame))` (seal.md §15.1,
/// SEAL-CHAIN-001, SEAL-CHAIN-002, §26): any frame bytes, whatever their
/// receipt class.
pub fn chain_step(prev: &Chain, seq: u64, frame: &[u8]) -> Chain {
    let frame_hash = h(&[frame]);
    Chain(h(&[ctx::CHAIN, &prev.0, &seq.to_be_bytes(), &frame_hash]))
}

/// The sync fingerprint: a seq and four words (seal.md §15.3, §26).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncFingerprint {
    pub seq: u64,
    pub words: [&'static str; 4],
}

/// The text of §15.3: decimal `s`, one space, the four words joined by
/// single spaces.
impl core::fmt::Display for SyncFingerprint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.seq)?;
        for w in self.words {
            write!(f, " {w}")?;
        }
        Ok(())
    }
}

/// Line `i + 1` of the word list (0-based index `i`, below 2048).
fn word(i: usize) -> &'static str {
    // The list has 2048 lines (SEAL-FP-002, tested), and `i` has 11 bits.
    WORDLIST.lines().nth(i).unwrap_or("")
}

/// The sync fingerprint of a head (seal.md §15.3, SEAL-FP-001, §26).
pub fn sync_fingerprint(board: &BoardId, head: &Head) -> SyncFingerprint {
    let x = h(&[ctx::FINGERPRINT, &board.0, &head.seq.to_be_bytes(), &head.chain.0]);
    let mut first = [0u8; 8];
    first.copy_from_slice(x.get(..8).unwrap_or(&[0u8; 8]));
    let v = u64::from_be_bytes(first);
    let index = |k: u32| ((v >> (64 - 11 * (k + 1))) & 0x7FF) as usize;
    SyncFingerprint { seq: head.seq, words: [word(index(0)), word(index(1)), word(index(2)), word(index(3))] }
}

/// `H(ctx("move") ‖ board_id ‖ u64 fence_seq ‖ fence_frame_hash)[0..5]` (seal.md §15.4).
fn move_bytes(board: &BoardId, fence_seq: u64, fence_hash: &FrameHash) -> [u8; 5] {
    let d = h(&[ctx::MOVE, &board.0, &fence_seq.to_be_bytes(), &fence_hash.0]);
    let mut out = [0u8; 5];
    out.copy_from_slice(d.get(..5).unwrap_or(&[0u8; 5]));
    out
}

/// The move code of a Fence (seal.md §15.4, SEAL-FP-003, §26), in its
/// display form `XXXX-XXXX`.
// SPEC-QUESTION: §15.4 defines the text as the 8 characters "shown as
// XXXX-XXXX" and §26 returns one `String`; this returns the shown form,
// with the hyphen (SEAL-ENC-001: grouping is applied after encoding).
pub fn move_code(board: &BoardId, fence_seq: u64, fence_hash: &FrameHash) -> String {
    let chars = base32::encode(&move_bytes(board, fence_seq, fence_hash));
    group(&chars, 4, '-')
}

/// Checks a typed move code, or `seal/move_code` (seal.md §15.4,
/// SEAL-FP-004, §26): the text must decode (§5.2) to exactly the 5 bytes of
/// this Fence, compared in constant time.
pub fn check_move_code(
    board: &BoardId,
    fence_seq: u64,
    fence_hash: &FrameHash,
    typed: &str,
) -> Result<(), SealError> {
    let decoded = base32::decode_typed_zeroizing(typed, 5).ok_or(SealError::MoveCode)?;
    let want = move_bytes(board, fence_seq, fence_hash);
    if bool::from(decoded.as_slice().ct_eq(&want[..])) {
        Ok(())
    } else {
        Err(SealError::MoveCode)
    }
}

/// A key fingerprint for banners (seal.md §6.8, SEAL-KEY-016, §26): the
/// 16 characters of `H(ctx("key-fp") ‖ sign_pk ‖ hpke_pk)[0..10]` as four
/// groups of four joined by `-`.
pub fn key_fingerprint(sign_pk: &[u8; 32], hpke_pk: &[u8; 32]) -> String {
    let d = h(&[ctx::KEY_FP, sign_pk, hpke_pk]);
    let chars = base32::encode(d.get(..10).unwrap_or(&[0u8; 10]));
    group(&chars, 4, '-')
}

/// Joins groups of `n` characters with `sep`.
fn group(chars: &str, n: usize, sep: char) -> String {
    let mut out = String::with_capacity(chars.len() + chars.len() / n);
    for (i, c) in chars.chars().enumerate() {
        if i > 0 && i % n == 0 {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

/// The state hash of ENG-HASH-001 (engine.md §18.3; seal.md §25, §26):
/// SHA-256 of the 86 bytes `ctx("state") ‖ u32 hash_version ‖ board_hash ‖
/// SHA-256(membership_bytes)`.
pub fn state_hash(hash_version: u32, board_hash: &[u8; 32], membership_bytes: &[u8]) -> [u8; 32] {
    let membership_hash = h(&[membership_bytes]);
    h(&[ctx::STATE, &hash_version.to_be_bytes(), board_hash, &membership_hash])
}

#[cfg(test)]
mod tests;
