//! The context strings of seal.md §4.1 and the uses of task S1.

use crate::common::BOARD;
use hpke::{Deserializable as _, Serializable as _};
use taskdeck_seal::{
    DeviceKeys, EpochRoot, OsEntropy, Recipient, SealError, chain_zero, ctx, unwrap_root, wrap_root,
};

type Kem = hpke::kem::X25519HkdfSha256;
type Kdf = hpke::kdf::HkdfSha256;
type Aead = hpke::aead::ChaCha20Poly1305;

/// The registry of §4.1: each `<use>` once.
const NAMES: [&str; 40] = [
    "frame", "login", "admin", "backup", "ctl", "entry", "blob", "file", "pair-bundle", "wrap", "wrap-aad",
    "backup-hpke", "backup-aad", "key-entry", "key-snap", "key-blob", "file-key", "recovery-hpke", "recovery-sign",
    "pair-ticket", "pair-mac", "pair-bundle-key", "sas", "pair", "pair-tid", "pair-commit", "pair-transcript",
    "pair-code", "board", "recovery-dev", "epoch", "chain", "fingerprint", "key-fp", "move", "sheet-check", "state",
    "door", "token", "local-key",
];

// Each public const is its row's string, and no string is a prefix of
// another's use: each ends in 0x00 and holds no other 0x00.
// spec: SEAL-CTX-001
#[test]
fn the_registry_consts() {
    let consts: [&[u8]; 40] = [
        ctx::FRAME, ctx::LOGIN, ctx::ADMIN, ctx::BACKUP, ctx::CTL, ctx::ENTRY, ctx::BLOB, ctx::FILE,
        ctx::PAIR_BUNDLE, ctx::WRAP, ctx::WRAP_AAD, ctx::BACKUP_HPKE, ctx::BACKUP_AAD, ctx::KEY_ENTRY,
        ctx::KEY_SNAP, ctx::KEY_BLOB, ctx::FILE_KEY, ctx::RECOVERY_HPKE, ctx::RECOVERY_SIGN, ctx::PAIR_TICKET,
        ctx::PAIR_MAC, ctx::PAIR_BUNDLE_KEY, ctx::SAS, ctx::PAIR, ctx::PAIR_TID, ctx::PAIR_COMMIT,
        ctx::PAIR_TRANSCRIPT, ctx::PAIR_CODE, ctx::BOARD, ctx::RECOVERY_DEV, ctx::EPOCH, ctx::CHAIN,
        ctx::FINGERPRINT, ctx::KEY_FP, ctx::MOVE, ctx::SHEET_CHECK, ctx::STATE, ctx::DOOR, ctx::TOKEN,
        ctx::LOCAL_KEY,
    ];
    for (c, name) in consts.iter().zip(NAMES) {
        assert_eq!(*c, format!("taskdeck/v1/{name}\0").as_bytes());
        assert_eq!(c.iter().filter(|&&b| b == 0).count(), 1);
    }
    for (i, a) in consts.iter().enumerate() {
        for b in &consts[i + 1..] {
            assert!(!a.starts_with(b) && !b.starts_with(a));
        }
    }
    // The chain hashes begin with ctx("chain").
    let c0 = chain_zero(&BOARD);
    let want: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest([ctx::CHAIN, &BOARD.0].concat()).into();
    assert_eq!(c0.0, want);
}

/// The wrap's info and aad with `use_info` and `use_aad` in place of `wrap`
/// and `wrap-aad`.
fn info_aad(use_info: &[u8], use_aad: &[u8], e: u32, id: &[u8; 16], pk: &[u8; 32], c: &[u8; 32]) -> (Vec<u8>, Vec<u8>) {
    let info = [use_info, &BOARD.0, &e.to_be_bytes(), id, pk].concat();
    let aad = [use_aad, &[1, 1, 0x00, 0x20, 0x00, 0x01, 0x00, 0x03], c].concat();
    (info, aad)
}

// An HPKE ciphertext of the root made with the `backup-hpke` info (or the
// `backup-aad` aad) in the wrap's layout does not unwrap; a wrap does not
// open under those strings.
// spec: SEAL-CTX-001
// spec: SEAL-WRAP-002
#[test]
fn a_wrap_is_not_a_backup_payload() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let p = d.public();
    let root = EpochRoot::from_bytes([0x5C; 32]);
    let c = root.commitment(&BOARD, 2);
    let pk = <Kem as hpke::Kem>::PublicKey::from_bytes(&p.hpke_pk).unwrap();
    let ids = d.to_identity_bytes();
    let sk = <Kem as hpke::Kem>::PrivateKey::from_bytes(&ids[48..80]).unwrap();
    for (i, a) in [(ctx::BACKUP_HPKE, ctx::WRAP_AAD), (ctx::WRAP, ctx::BACKUP_AAD), (ctx::BACKUP_HPKE, ctx::BACKUP_AAD)] {
        let (info, aad) = info_aad(i, a, 2, &p.dev.0, &p.hpke_pk, &c);
        // A wrap-shaped ciphertext under the other strings.
        let mut rng = Os;
        let (enc, ct) =
            hpke::single_shot_seal_with_rng::<Aead, Kdf, Kem>(&hpke::OpModeS::Base, &pk, &info, &[0x5C; 32], &aad, &mut rng).unwrap();
        let mut w = [0u8; 96];
        w[..16].copy_from_slice(&p.dev.0);
        w[16..48].copy_from_slice(&enc.to_bytes());
        w[48..].copy_from_slice(&ct);
        assert_eq!(unwrap_root(&w, &BOARD, 2, &c, &Recipient::Device(&d)).err(), Some(SealError::Commitment));
        // A real wrap opened under the other strings.
        let w = wrap_root(&root, &BOARD, 2, &c, p.dev, &p.hpke_pk, &mut OsEntropy).unwrap();
        let enc = <Kem as hpke::Kem>::EncappedKey::from_bytes(&w[16..48]).unwrap();
        assert!(hpke::single_shot_open::<Aead, Kdf, Kem>(&hpke::OpModeR::Base, &sk, &enc, &info, &w[48..], &aad).is_err());
        let (info, aad) = info_aad(ctx::WRAP, ctx::WRAP_AAD, 2, &p.dev.0, &p.hpke_pk, &c);
        assert!(hpke::single_shot_open::<Aead, Kdf, Kem>(&hpke::OpModeR::Base, &sk, &enc, &info, &w[48..], &aad).is_ok());
    }
}

/// Random bytes for hpke in the test, from the operating system.
struct Os;
impl hpke::rand_core::TryRng for Os {
    type Error = core::convert::Infallible;
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        let mut b = [0u8; 4];
        self.try_fill_bytes(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut b = [0u8; 8];
        self.try_fill_bytes(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        getrandom::fill(dst).unwrap();
        Ok(())
    }
}
impl hpke::rand_core::TryCryptoRng for Os {}
