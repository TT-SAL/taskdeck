//! The signature contexts of task S2 (`frame`, `login`, `admin`, `ctl`):
//! a signature the crate made for one use does not verify as another. Task
//! S4 adds `backup`.

use crate::common::BOARD;
use taskdeck_seal::{
    AdminOp, CtlItem, DevId, DeviceKeys, EntryId, EpochRoot, Head, KeyKind, Kind, OsEntropy, Plain, RecoverySeed, Signer,
    SkipItem, FrameHash, admin_message, chain_zero, ctl_payload, ctx, derive_epoch_key, login_message, seal_frame,
    sign_admin, sign_login,
};

fn sha(b: &[u8]) -> [u8; 32] {
    <sha2::Sha256 as sha2::Digest>::digest(b).into()
}

pub fn verifies(pk: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> bool {
    ed25519_dalek::VerifyingKey::from_bytes(pk)
        .unwrap()
        .verify_strict(msg, &ed25519_dalek::Signature::from_bytes(sig))
        .is_ok()
}

/// One signature the crate made, with the bytes it signed.
pub struct Made {
    pub use_: &'static [u8],
    pub signed: Vec<u8>,
    pub sig: [u8; 64],
}

pub fn made(signer: &Signer, dev: DevId, keys: Option<&DeviceKeys>) -> Vec<Made> {
    let nonce = [0x4E; 32];
    let mut out = Vec::new();
    // frame
    let key = derive_epoch_key(&EpochRoot::from_bytes([1; 32]), &BOARD, 1, KeyKind::Entry);
    let items = [CtlItem::Skip(SkipItem { seq: 1, frame_hash: FrameHash([2; 32]) })];
    let entry_id = EntryId([3; 16]);
    let payload = ctl_payload(&items, signer, &BOARD, 7, &entry_id, 99).unwrap();
    let plain = Plain { entry_id, kind: Kind::Ctl, at_ms: 99, based_on: Head { seq: 0, chain: chain_zero(&BOARD) }, payload: payload.clone() };
    let f = seal_frame(signer, &BOARD, 7, 1, &key, &plain, &mut OsEntropy).unwrap();
    let n = f.len();
    out.push(Made { use_: ctx::FRAME, signed: [ctx::FRAME, &f[..n - 64]].concat(), sig: f[n - 64..].try_into().unwrap() });
    // ctl: {"items":<items>,"sig":"<hex>"}
    let items_bytes = &payload[9..payload.len() - 138];
    let v: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    let sig = crate::common::unhex_n::<64>(v["sig"].as_str().unwrap());
    let inner = [ctx::CTL, &BOARD.0, &dev.0, &7u64.to_be_bytes(), &entry_id.0, &99i64.to_be_bytes(), &sha(items_bytes)].concat();
    out.push(Made { use_: ctx::CTL, signed: inner, sig });
    // admin
    let op = AdminOp::Revoke(DevId([9; 16]));
    let sig = sign_admin(signer, &op, &BOARD, &nonce).unwrap();
    out.push(Made { use_: ctx::ADMIN, signed: admin_message(&op, &BOARD, &dev, &nonce).unwrap(), sig });
    // login (devices only: the recovery identity never logs in, §16.1)
    if let Some(k) = keys {
        let origin = "https://relay.example.org";
        let (cn, sig) = sign_login(k, origin, &BOARD, 1, &nonce, &mut OsEntropy).unwrap();
        out.push(Made { use_: ctx::LOGIN, signed: login_message(origin, &BOARD, &dev, 1, &nonce, &cn).unwrap(), sig });
    }
    out
}

// For each pair of the uses frame, login, admin and ctl: the signature the
// crate made for one verifies over its own bytes and fails over the same
// bytes under the other use's context; and the signed bytes of one use
// never begin with another use's context.
// spec: SEAL-CTX-002
// spec: SEAL-CTX-001
#[test]
fn a_signature_for_one_use_fails_as_another() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let rk = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
    let uses = [ctx::FRAME, ctx::LOGIN, ctx::ADMIN, ctx::CTL];
    for (all, pk) in [
        (made(&Signer::Device(&d), d.public().dev, Some(&d)), d.public().sign_pk),
        (made(&Signer::Recovery(&rk, BOARD), rk.public().dev(&BOARD), None), rk.public().sign_pk),
    ] {
        for m in &all {
            assert!(m.signed.starts_with(m.use_));
            assert!(verifies(&pk, &m.signed, &m.sig));
            let rest = &m.signed[m.use_.len()..];
            for other in uses.iter().filter(|u| **u != m.use_) {
                assert!(!m.signed.starts_with(other));
                assert!(!verifies(&pk, &[*other, rest].concat(), &m.sig));
                // Nor over another use's bytes the crate made.
                for o in all.iter().filter(|o| o.use_ == *other) {
                    assert!(!verifies(&pk, &o.signed, &m.sig));
                }
            }
        }
    }
}
