//! RFC 9180 Appendix A.2.1: DHKEM(X25519, HKDF-SHA256), HKDF-SHA256,
//! ChaCha20-Poly1305, base mode, through the hpke calls `wrap_root` and
//! `unwrap_root` make: `DeriveKeyPair`, the sender's setup with the
//! ephemeral key drawn as 32 bytes `ikmE`, the key schedule (every
//! encryption and export), and the receiver's open.

use crate::common::{b, read_json};
use hpke::{Deserializable as _, Kem as _, Serializable as _};

type Kem = hpke::kem::X25519HkdfSha256;
type Kdf = hpke::kdf::HkdfSha256;
type Aead = hpke::aead::ChaCha20Poly1305;

/// Hands hpke `ikmE` as its random bytes, once, as `wrap_root` does.
struct Once(Vec<u8>, usize);
impl hpke::rand_core::TryRng for Once {
    type Error = core::convert::Infallible;
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        panic!("hpke asked for a u32")
    }
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        panic!("hpke asked for a u64")
    }
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        assert_eq!(self.1, 0, "hpke drew twice");
        assert_eq!(dst.len(), self.0.len(), "hpke drew another length");
        dst.copy_from_slice(&self.0);
        self.1 += 1;
        Ok(())
    }
}
impl hpke::rand_core::TryCryptoRng for Once {}

fn case() -> serde_json::Value {
    let doc = read_json("tests/kat/data/rfc9180.json");
    let c = doc["cases"][0].clone();
    assert_eq!((c["mode"].as_u64(), c["kem_id"].as_u64(), c["kdf_id"].as_u64(), c["aead_id"].as_u64()), (Some(0), Some(32), Some(1), Some(3)));
    c
}

// DeriveKeyPair(ikm) gives the RFC's key pairs (the recovery key of §6.1,
// the device key of §6.4 and the ephemeral key of §7 are made this way).
// hpke serialises an X25519 private key clamped (RFC 9180 §7.1.2), while
// the vectors print the unclamped scalar, so the clamped form is compared,
// and the vector's own bytes must give the same public key.
// spec: SEAL-KEY-002
// spec: SEAL-WRAP-004
#[test]
fn derive_key_pair() {
    let c = case();
    for (ikm, sk, pk) in [("ikmE", "skEm", "pkEm"), ("ikmR", "skRm", "pkRm")] {
        let (s, p) = Kem::derive_keypair(&b(&c, ikm));
        let mut clamped = b(&c, sk);
        clamped[0] &= 248;
        clamped[31] &= 127;
        clamped[31] |= 64;
        assert_eq!(s.to_bytes().to_vec(), clamped);
        assert_eq!(p.to_bytes().to_vec(), b(&c, pk));
        let loaded = <Kem as hpke::Kem>::PrivateKey::from_bytes(&b(&c, sk)).unwrap();
        assert_eq!(Kem::sk_to_pk(&loaded).to_bytes().to_vec(), b(&c, pk));
    }
}

// The sender, with the ephemeral key from 32 bytes ikmE: enc, every
// encryption of the context in order, every export; the single-shot seal
// that wrap_root calls gives the first encryption.
// spec: SEAL-WRAP-001
// spec: SEAL-WRAP-004
#[test]
fn sender_key_schedule_and_encryptions() {
    let c = case();
    let pk_r = <Kem as hpke::Kem>::PublicKey::from_bytes(&b(&c, "pkRm")).unwrap();
    let info = b(&c, "info");
    let mut rng = Once(b(&c, "ikmE"), 0);
    let (enc, mut ctx) = hpke::setup_sender_with_rng::<Aead, Kdf, Kem>(&hpke::OpModeS::Base, &pk_r, &info, &mut rng).unwrap();
    assert_eq!(enc.to_bytes().to_vec(), b(&c, "enc"));
    let encs = c["encryptions"].as_array().unwrap();
    assert_eq!(encs.len(), 257);
    for (i, e) in encs.iter().enumerate() {
        let ct = ctx.seal(&b(e, "pt"), &b(e, "aad")).unwrap();
        assert_eq!(ct, b(e, "ct"), "encryption {i}");
    }
    for x in c["exports"].as_array().unwrap() {
        let mut out = vec![0u8; x["L"].as_u64().unwrap() as usize];
        ctx.export(&b(x, "exporter_context"), &mut out).unwrap();
        assert_eq!(out, b(x, "exported_value"));
    }
    let first = &encs[0];
    let (enc1, ct1) = hpke::single_shot_seal_with_rng::<Aead, Kdf, Kem>(
        &hpke::OpModeS::Base,
        &pk_r,
        &info,
        &b(first, "pt"),
        &b(first, "aad"),
        &mut Once(b(&c, "ikmE"), 0),
    )
    .unwrap();
    assert_eq!(enc1.to_bytes().to_vec(), b(&c, "enc"));
    assert_eq!(ct1, b(first, "ct"));
}

// The receiver opens every encryption in order, the single-shot open that
// unwrap_root calls opens the first, and a flipped byte does not open.
// spec: SEAL-WRAP-002
#[test]
fn receiver_opens() {
    let c = case();
    let sk_r = <Kem as hpke::Kem>::PrivateKey::from_bytes(&b(&c, "skRm")).unwrap();
    let enc = <Kem as hpke::Kem>::EncappedKey::from_bytes(&b(&c, "enc")).unwrap();
    let info = b(&c, "info");
    let mut ctx = hpke::setup_receiver::<Aead, Kdf, Kem>(&hpke::OpModeR::Base, &sk_r, &enc, &info).unwrap();
    let encs = c["encryptions"].as_array().unwrap();
    for (i, e) in encs.iter().enumerate() {
        assert_eq!(ctx.open(&b(e, "ct"), &b(e, "aad")).unwrap(), b(e, "pt"), "encryption {i}");
    }
    let first = &encs[0];
    let pt = hpke::single_shot_open::<Aead, Kdf, Kem>(&hpke::OpModeR::Base, &sk_r, &enc, &info, &b(first, "ct"), &b(first, "aad")).unwrap();
    assert_eq!(pt, b(first, "pt"));
    let mut bad = b(first, "ct");
    bad[0] ^= 1;
    assert!(hpke::single_shot_open::<Aead, Kdf, Kem>(&hpke::OpModeR::Base, &sk_r, &enc, &info, &bad, &b(first, "aad")).is_err());
}
