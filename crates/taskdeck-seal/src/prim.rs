//! Thin wrappers over the pinned crates (SEAL-GEN-003): SHA-256 of a
//! concatenation, HKDF-SHA256 to 32 bytes, and the HPKE suite of §7.

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// The HPKE KEM of seal.md §7: DHKEM(X25519, HKDF-SHA256).
pub(crate) type Kem = hpke::kem::X25519HkdfSha256;
/// The HPKE KDF of seal.md §7: HKDF-SHA256.
pub(crate) type Kdf = hpke::kdf::HkdfSha256;
/// The HPKE AEAD of seal.md §7: ChaCha20-Poly1305.
pub(crate) type HpkeAead = hpke::aead::ChaCha20Poly1305;

/// `H(parts[0] ‖ parts[1] ‖ …)` (seal.md §2).
pub(crate) fn h(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for p in parts {
        hasher.update(p);
    }
    hasher.finalize().into()
}

/// `HKDF(salt, ikm, info_parts[0] ‖ info_parts[1] ‖ …, 32)` (seal.md §2).
pub(crate) fn hkdf32(salt: &[u8], ikm: &[u8], info_parts: &[&[u8]]) -> Zeroizing<[u8; 32]> {
    let mut okm = Zeroizing::new([0u8; 32]);
    let hk = hkdf::Hkdf::<Sha256>::new(Some(salt), ikm);
    // 32 bytes is far below HKDF-SHA256's limit of 255 × 32, so expansion
    // cannot fail; the length is a constant, never an input.
    hk.expand_multi_info(info_parts, &mut okm[..])
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    okm
}

/// `HKDF(salt, ikm, info_parts[0] ‖ info_parts[1] ‖ …, 8)` (seal.md §2; the
/// SAS of §17.7).
pub(crate) fn hkdf8(salt: &[u8], ikm: &[u8], info_parts: &[&[u8]]) -> Zeroizing<[u8; 8]> {
    let mut okm = Zeroizing::new([0u8; 8]);
    let hk = hkdf::Hkdf::<Sha256>::new(Some(salt), ikm);
    // 8 bytes is far below HKDF-SHA256's limit; the length is a constant.
    hk.expand_multi_info(info_parts, &mut okm[..])
        .expect("8 bytes is a valid HKDF-SHA256 output length");
    okm
}

/// `HMAC(key, parts[0] ‖ parts[1] ‖ …)` with HMAC-SHA256 (seal.md §2), from
/// the `hmac` crate that `hkdf` re-exports (SEAL-GEN-003).
pub(crate) fn hmac32(key: &[u8; 32], parts: &[&[u8]]) -> [u8; 32] {
    use hkdf::hmac::{Hmac, KeyInit as _, Mac as _};
    // HMAC takes a key of any length; a 32-byte key never fails.
    let mut m = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts a 32-byte key");
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

/// HPKE `DeriveKeyPair(ikm)` (RFC 9180 §7.1.3) for DHKEM(X25519, HKDF-SHA256):
/// the private key as an X25519 static secret, and the 32-byte public key.
pub(crate) fn hpke_derive(ikm: &[u8]) -> (x25519_dalek::StaticSecret, [u8; 32]) {
    use hpke::{Kem as _, Serializable as _};
    let (sk, pk) = Kem::derive_keypair(ikm);
    let mut skb = Zeroizing::new([0u8; 32]);
    sk.write_exact(&mut skb[..]);
    let mut pkb = [0u8; 32];
    pk.write_exact(&mut pkb);
    (x25519_dalek::StaticSecret::from(*skb), pkb)
}

/// The X25519 public key of a static secret, as HPKE serialises it.
pub(crate) fn x25519_public(sk: &x25519_dalek::StaticSecret) -> [u8; 32] {
    x25519_dalek::PublicKey::from(sk).to_bytes()
}

/// Splits a fixed-width array at constant offsets without indexing.
pub(crate) fn take<const N: usize>(bytes: &[u8], at: usize) -> Option<&[u8; N]> {
    let end = at.checked_add(N)?;
    bytes.get(at..end)?.try_into().ok()
}

/// Appends `data` to a zeroizing buffer without leaving a copy behind: when
/// the capacity does not suffice, the bytes move to a new buffer and the old
/// one is wiped on drop (a plain `Vec` would reallocate and free unwiped
/// memory).
pub(crate) fn push_zeroizing(out: &mut Zeroizing<Vec<u8>>, data: &[u8]) {
    let need = out.len().saturating_add(data.len());
    if need > out.capacity() {
        let cap = need.max(out.capacity().saturating_mul(2)).max(4096);
        let mut bigger = Zeroizing::new(Vec::with_capacity(cap));
        bigger.extend_from_slice(out);
        *out = bigger;
    }
    out.extend_from_slice(data);
}

/// The working buffer of `deflate_raw` and `inflate_raw`.
const CHUNK: usize = 64 * 1024;

/// The most bytes one stored block holds (RFC 1951 §3.2.4).
const STORED_MAX: usize = 65_535;

#[cfg(test)]
thread_local! {
    /// Test builds only: every byte handed to a DEFLATE compressor on this
    /// thread, so that a test can show which bytes never reach one
    /// (SEAL-GEN-006, `deflate_split`).
    pub(crate) static COMPRESSOR_INPUT: core::cell::RefCell<Vec<u8>> = const { core::cell::RefCell::new(Vec::new()) };
}

/// What one more sync-flush call with no input writes when the compressor
/// holds nothing back (miniz_oxide 0.8.9, `flush_block` with an empty LZ
/// buffer): a non-final static block holding only its end-of-block code
/// (bits 0, 01, 0000000), then the empty stored block of the sync flush
/// (bits 000, padding, `00 00 FF FF`).
const EMPTY_SYNC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0xFF, 0xFF];

/// The most probe calls `compress_piece` makes after all of a sync piece's
/// input is in. Each call that does not return `EMPTY_SYNC` writes out what
/// the compressor still held (at most its 258-byte lookahead and one LZ
/// buffer, `flush_remaining` included), so a few suffice.
const PROBES_MAX: usize = 16;

#[cfg(test)]
thread_local! {
    /// Test builds only: the bytes `inflate_raw` had its decompressor
    /// produce on this thread, those over the cap included (SEAL-BLOB-003).
    pub(crate) static INFLATED: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
    /// Test builds only: the resets `Inflater::drop` made on this thread
    /// (SEAL-GEN-006, ruling Q48).
    pub(crate) static INFLATER_RESETS: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
}

/// Runs a fresh compressor over `data` and appends its output to `out`.
/// With `finish` the stream ends (its last block carries the final bit);
/// without, a sync flush ends the output on a byte boundary after the last
/// block, with no final bit.
///
/// The flush loop (sync): a call that takes in all of `data` and leaves room
/// in the working buffer has not necessarily written the whole flush. In
/// the pinned miniz_oxide, when the LZ code buffer fills, `compress_normal`
/// writes a block in mid-stream and returns with bytes still in its
/// lookahead; `compress_inner` then skips the sync `flush_block`
/// (`lookahead_size != 0`), and `deflate` still returns `Ok` because the
/// input is empty. So once all input is in and a call has left room in the
/// buffer (nothing is held back in `flush_remaining`), the loop calls again
/// with no input and the same flush, and stops at the first such call whose
/// whole output is `EMPTY_SYNC`. That proves the output before it was
/// complete: every byte of `data` written, no bits pending (pending bits
/// would shift that output), every block ended on a byte boundary. Any
/// lookahead, LZ code or held-back byte would make the call's output longer
/// (a block with one literal or match and the sync marker takes at least 7
/// bytes). That last call's output, an empty block that the stream does not
/// need, is not appended, so the output is the same as before the probe
/// whenever the first flush was already complete. A call that fills the
/// buffer exactly is followed by another with the same flush, as zlib
/// documents.
pub(crate) fn compress_piece(out: &mut Zeroizing<Vec<u8>>, data: &[u8], finish: bool) -> Option<()> {
    use flate2::{Compress, Compression, FlushCompress, Status};
    #[cfg(test)]
    COMPRESSOR_INPUT.with(|c| c.borrow_mut().extend_from_slice(data));
    let flush = if finish { FlushCompress::Finish } else { FlushCompress::Sync };
    let mut c = Compress::new(Compression::default(), false);
    let mut buf = Zeroizing::new(vec![0u8; CHUNK]);
    // Sync only: the previous call took in all of `data` and left room in
    // the buffer, so this call is a probe.
    let mut probe = false;
    let mut probes = 0usize;
    loop {
        let in_before = c.total_in();
        let out_before = c.total_out();
        let input = data.get(usize::try_from(in_before).ok()?..)?;
        let st = c.compress(input, &mut buf[..], flush).ok()?;
        let produced = usize::try_from(c.total_out().checked_sub(out_before)?).ok()?;
        let written = buf.get(..produced)?;
        if probe {
            if written == EMPTY_SYNC {
                return Some(());
            }
            probes = probes.checked_add(1)?;
            if probes >= PROBES_MAX {
                return None;
            }
        }
        push_zeroizing(out, written);
        let all_in = usize::try_from(c.total_in()).ok()? == data.len();
        match st {
            Status::StreamEnd => return finish.then_some(()),
            _ if !finish && all_in && produced < buf.len() => probe = true,
            Status::Ok | Status::BufError if produced == 0 && c.total_in() == in_before => return None,
            Status::Ok | Status::BufError => probe = false,
        }
    }
}

/// Raw DEFLATE (RFC 1951) of `data`, as flate2 writes it (seal.md §20.4,
/// §21.3), into a zeroizing buffer; `None` if the compressor fails.
///
/// Only for bytes that hold no secret: the compressor's private buffers
/// keep what it was given and are freed unwiped (see `deflate_split`).
pub(crate) fn deflate_raw(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    let mut out = Zeroizing::new(Vec::with_capacity(data.len() / 2 + 64));
    compress_piece(&mut out, data, true)?;
    Some(out)
}

/// An empty final stored block (RFC 1951 §3.2.4): `BFINAL = 1`, `BTYPE =
/// 00`, padding to the byte, `LEN = 0`, `NLEN = 0xFFFF`. After the head's
/// sync-flushed output it ends the stream there, for the proof of
/// `deflate_split`.
const EMPTY_FINAL_STORED: [u8; 5] = [0x01, 0x00, 0x00, 0xFF, 0xFF];

/// Test builds only: a change to a stream of `deflate_split`, given the
/// stream, the end of the compressed head and the start of the compressed
/// tail.
#[cfg(test)]
pub(crate) type SplitFault = fn(&mut Vec<u8>, usize, usize);

#[cfg(test)]
thread_local! {
    /// Test builds only: a change `deflate_split` makes to its stream
    /// before the proof, given the stream, the end of the compressed head
    /// and the start of the compressed tail (a stand-in for a compressor
    /// that writes a stream that does not read back).
    pub(crate) static SPLIT_FAULT: core::cell::Cell<Option<SplitFault>> =
        const { core::cell::Cell::new(None) };
    /// Test builds only: every byte handed to the proof's decompressor on
    /// this thread (`inflates_to`).
    pub(crate) static PROOF_INPUT: core::cell::RefCell<Vec<u8>> = const { core::cell::RefCell::new(Vec::new()) };
}

/// One raw DEFLATE stream (RFC 1951) of `head ‖ secret ‖ tail` in which
/// `secret` never passes through a compressor (seal.md §21.3, SEAL-GEN-006):
/// `head` through a compressor ended with a sync flush (byte-aligned, no
/// final bit); then `secret` in stored blocks written here, each a byte
/// `0x00` (not final, type 00), `LEN` and `NLEN` as little-endian u16 and at
/// most 65,535 bytes as they are (§3.2.4); then `tail` through a fresh
/// compressor, finished, whose last block carries the final bit. A reader
/// inflates it as any other stream. The output is in a zeroizing buffer.
///
/// The stream is proved before it is returned (task S7b: a defect of the
/// compressor's flush showed only on some data, and a backup that does not
/// read back is found only at a restore): the compressed head followed by an
/// empty final stored block inflates to exactly `head`, and the compressed
/// tail inflates to exactly `tail`, each compared while inflating
/// (`inflates_to`). The stored blocks are written here byte by byte and do
/// not pass through the decompressor, so neither does `secret`.
///
/// What stays outside the crate's reach: the two compressors' private
/// buffers (the window, hash chains and LZ buffer of miniz_oxide's
/// `CompressorOxide`) keep `head` and `tail` and are freed unwiped; that is
/// why the secret bytes never enter one. `None` if a compressor fails or the
/// proof does.
pub(crate) fn deflate_split(head: &[u8], secret: &[u8], tail: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    let blocks = secret.len().div_ceil(STORED_MAX).max(1);
    let cap = (head.len() / 2).saturating_add(secret.len()).saturating_add(5 * blocks).saturating_add(tail.len() / 2 + 64);
    let mut out = Zeroizing::new(Vec::with_capacity(cap));
    compress_piece(&mut out, head, false)?;
    let head_end = out.len();
    for chunk in secret.chunks(STORED_MAX) {
        let len = u16::try_from(chunk.len()).ok()?;
        push_zeroizing(&mut out, &[0x00]);
        push_zeroizing(&mut out, &len.to_le_bytes());
        push_zeroizing(&mut out, &(!len).to_le_bytes());
        push_zeroizing(&mut out, chunk);
    }
    let tail_start = out.len();
    compress_piece(&mut out, tail, true)?;
    #[cfg(test)]
    if let Some(fault) = SPLIT_FAULT.with(core::cell::Cell::take) {
        fault(&mut out, head_end, tail_start);
    }
    inflates_to(&[out.get(..head_end)?, &EMPTY_FINAL_STORED], head)?;
    inflates_to(&[out.get(tail_start..)?], tail)?;
    Some(out)
}

/// Whether the raw DEFLATE stream that is the concatenation of `pieces`
/// inflates to exactly `want`, its end coming exactly at the end of the
/// last piece: `Some(())`, or `None` at the first difference, for output
/// beyond `want` or short of it, a wrong or truncated stream, or input left
/// after its end. The output is compared chunk by chunk as it is inflated,
/// in one 64 KiB working buffer: no copy of `want` is made, and the pieces
/// are fed in turn rather than joined. The decompressor is reset before it
/// is dropped (`Inflater`).
fn inflates_to(pieces: &[&[u8]], want: &[u8]) -> Option<()> {
    use flate2::{Decompress, FlushDecompress, Status};
    #[cfg(test)]
    PROOF_INPUT.with(|c| pieces.iter().for_each(|p| c.borrow_mut().extend_from_slice(p)));
    let mut inflater = Inflater(Decompress::new(false));
    let d = &mut inflater.0;
    let mut buf = Zeroizing::new(vec![0u8; CHUNK]);
    let mut piece = 0usize;
    let mut at = 0usize;
    let mut done = 0usize;
    loop {
        // Skip the pieces fed whole.
        while pieces.get(piece).is_some_and(|p| at >= p.len()) {
            piece = piece.checked_add(1)?;
            at = 0;
        }
        let input = match pieces.get(piece) {
            Some(p) => p.get(at..)?,
            None => &[],
        };
        let in_before = d.total_in();
        let out_before = d.total_out();
        let st = d.decompress(input, &mut buf[..], FlushDecompress::None).ok()?;
        let consumed = usize::try_from(d.total_in().checked_sub(in_before)?).ok()?;
        let produced = usize::try_from(d.total_out().checked_sub(out_before)?).ok()?;
        let end = done.checked_add(produced)?;
        if buf.get(..produced)? != want.get(done..end)? {
            return None;
        }
        done = end;
        at = at.checked_add(consumed)?;
        match st {
            Status::StreamEnd => break,
            // No progress: the input ended before the stream did.
            Status::Ok | Status::BufError if produced == 0 && consumed == 0 => return None,
            Status::Ok | Status::BufError => {}
        }
    }
    while pieces.get(piece).is_some_and(|p| at >= p.len()) {
        piece = piece.checked_add(1)?;
        at = 0;
    }
    (done == want.len() && piece == pieces.len()).then_some(())
}

/// A decompressor that is reset when it is dropped, on every path. In the
/// pinned miniz_oxide (0.8.9, `inflate/stream.rs`) flate2's `reset` is
/// `InflateState::reset(FullReset)`, which zeroes the 32 KiB window holding
/// the last bytes inflated; `black_box` keeps the compiler from dropping the
/// write. What stays outside the crate's reach: the decompressor's bit
/// buffer, a few bytes of input, which the reset does not clear.
struct Inflater(flate2::Decompress);

impl Drop for Inflater {
    fn drop(&mut self) {
        self.0.reset(false);
        core::hint::black_box(&mut self.0);
        // Test builds only: count the reset by its effect (flate2's reset
        // sets both totals back to 0, as miniz_oxide's zeroes the window).
        #[cfg(test)]
        if self.0.total_in() == 0 && self.0.total_out() == 0 {
            INFLATER_RESETS.with(|c| c.set(c.get().saturating_add(1)));
        }
    }
}

/// Inflates raw DEFLATE (RFC 1951) while streaming, under a cap of `cap`
/// bytes of output (seal.md §20.5, §21.6 step 7): `None` when the stream is
/// wrong or truncated, when it would produce more than `cap` bytes, or when
/// any input follows the end of the stream. Nothing is allocated for a
/// claimed size (DEFLATE claims none): the output grows only with the bytes
/// actually inflated, in a zeroizing buffer. The decompressor is reset
/// before it is dropped, failures included (`Inflater`).
pub(crate) fn inflate_raw(data: &[u8], cap: usize) -> Option<Zeroizing<Vec<u8>>> {
    use flate2::{Decompress, FlushDecompress, Status};
    let mut inflater = Inflater(Decompress::new(false));
    let d = &mut inflater.0;
    let mut out = Zeroizing::new(Vec::new());
    let mut buf = Zeroizing::new(vec![0u8; CHUNK]);
    loop {
        let in_before = d.total_in();
        let out_before = d.total_out();
        let input = data.get(usize::try_from(in_before).ok()?..)?;
        let st = d.decompress(input, &mut buf[..], FlushDecompress::None).ok()?;
        let produced = usize::try_from(d.total_out().checked_sub(out_before)?).ok()?;
        #[cfg(test)]
        INFLATED.with(|c| c.set(c.get().saturating_add(produced as u64)));
        if out.len().checked_add(produced)? > cap {
            return None;
        }
        push_zeroizing(&mut out, buf.get(..produced)?);
        match st {
            Status::StreamEnd => break,
            // No progress: the input ended before the stream did.
            Status::Ok | Status::BufError if produced == 0 && d.total_in() == in_before => return None,
            Status::Ok | Status::BufError => {}
        }
    }
    (usize::try_from(d.total_in()).ok()? == data.len()).then_some(out)
}

#[cfg(test)]
mod tests;
