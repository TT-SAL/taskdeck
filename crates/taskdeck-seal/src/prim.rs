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

/// Raw DEFLATE (RFC 1951) of `data`, as flate2 writes it (seal.md §20.4,
/// §21.3), into a zeroizing buffer; `None` if the compressor fails.
pub(crate) fn deflate_raw(data: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    use flate2::{Compress, Compression, FlushCompress, Status};
    let mut c = Compress::new(Compression::default(), false);
    let mut out = Zeroizing::new(Vec::with_capacity(data.len() / 2 + 64));
    let mut buf = Zeroizing::new(vec![0u8; CHUNK]);
    loop {
        let in_before = c.total_in();
        let out_before = c.total_out();
        let input = data.get(usize::try_from(in_before).ok()?..)?;
        let st = c.compress(input, &mut buf[..], FlushCompress::Finish).ok()?;
        let produced = usize::try_from(c.total_out().checked_sub(out_before)?).ok()?;
        push_zeroizing(&mut out, buf.get(..produced)?);
        match st {
            Status::StreamEnd => break,
            Status::Ok | Status::BufError if produced == 0 && c.total_in() == in_before => return None,
            Status::Ok | Status::BufError => {}
        }
    }
    Some(out)
}

/// Inflates raw DEFLATE (RFC 1951) while streaming, under a cap of `cap`
/// bytes of output (seal.md §20.5, §21.6 step 7): `None` when the stream is
/// wrong or truncated, when it would produce more than `cap` bytes, or when
/// any input follows the end of the stream. Nothing is allocated for a
/// claimed size (DEFLATE claims none): the output grows only with the bytes
/// actually inflated, in a zeroizing buffer.
pub(crate) fn inflate_raw(data: &[u8], cap: usize) -> Option<Zeroizing<Vec<u8>>> {
    use flate2::{Decompress, FlushDecompress, Status};
    let mut d = Decompress::new(false);
    let mut out = Zeroizing::new(Vec::new());
    let mut buf = Zeroizing::new(vec![0u8; CHUNK]);
    loop {
        let in_before = d.total_in();
        let out_before = d.total_out();
        let input = data.get(usize::try_from(in_before).ok()?..)?;
        let st = d.decompress(input, &mut buf[..], FlushDecompress::None).ok()?;
        let produced = usize::try_from(d.total_out().checked_sub(out_before)?).ok()?;
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
