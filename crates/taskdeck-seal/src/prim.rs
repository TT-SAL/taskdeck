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

#[cfg(test)]
mod tests;
