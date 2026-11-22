//! `taskdeck-seal`: frames, contexts, envelope, keys, membership, chain,
//! pairing, HPKE wraps, sealed files and backups, as pure functions and
//! sans-IO machines.
//!
//! Spec: `docs/sync/spec/seal.md`. Owner: stage `seal` (SYNC_PLAN.md §4).
//! Created as a shell by stage `foundation`; the skeleton types come from the
//! spec's Public API (§26).
#![forbid(unsafe_code)]

// Declared for the stage that fills this crate: each dependency is named once
// so that it is resolved and compiled now, and a wrong feature set fails today
// rather than in wave 1.
use chacha20poly1305 as _;
use ed25519_dalek as _;
use flate2 as _;
use getrandom as _;
use hkdf as _;
use hpke as _;
use serde_json as _;
use sha2 as _;
use subtle as _;
use x25519_dalek as _;
const _: () = {
    // The features seal.md needs, named so that turning one off fails here.
    let _ = <x25519_dalek::StaticSecret as From<[u8; 32]>>::from;
    // `alloc`: XChaCha20-Poly1305 into a `Vec`.
    type Encrypt = fn(
        &chacha20poly1305::XChaCha20Poly1305,
        &chacha20poly1305::XNonce,
        &'static [u8],
    ) -> Result<Vec<u8>, chacha20poly1305::Error>;
    let _: Encrypt = <chacha20poly1305::XChaCha20Poly1305 as chacha20poly1305::aead::Aead>::encrypt;
    let _ = ed25519_dalek::VerifyingKey::verify_strict;
    let _ = <hpke::kem::X25519HkdfSha256 as hpke::Kem>::derive_keypair;
    let _ = hpke::single_shot_open::<hpke::aead::ChaCha20Poly1305, hpke::kdf::HkdfSha256, hpke::kem::X25519HkdfSha256>;
    #[allow(dead_code)] // never built: it only proves the derives compile
    #[derive(serde::Serialize, serde::Deserialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
    struct Declared([u8; 32]);
};

#[cfg(test)]
mod tests {
    use proptest as _;

    #[test]
    fn the_shell_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "taskdeck-seal");
    }
}
