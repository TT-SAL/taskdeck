//! Login and admin signatures (seal.md §16.1, §16.2; task S7): flipping any
//! byte of the signed message, of the signature, or of an admin operation's
//! `args` (whose hash the relay recomputes) makes the strict verification a
//! relay runs fail. The crate has no verifier of its own for these: the
//! check here is the relay's, `verify_strict` over the bytes of §16.

use crate::common::BOARD;
use taskdeck_seal::*;

fn sha(b: &[u8]) -> [u8; 32] {
    <sha2::Sha256 as sha2::Digest>::digest(b).into()
}

fn verifies(pk: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> bool {
    let Ok(vk) = ed25519_dalek::VerifyingKey::from_bytes(pk) else { return false };
    vk.verify_strict(msg, &ed25519_dalek::Signature::from_bytes(sig)).is_ok()
}

/// Every byte of `msg` and of `sig`, flipped three ways, fails.
fn flip_all(pk: &[u8; 32], msg: &[u8], sig: &[u8; 64]) {
    assert!(verifies(pk, msg, sig));
    for mask in [0x01u8, 0x80, 0xFF] {
        for i in 0..msg.len() {
            let mut m = msg.to_vec();
            m[i] ^= mask;
            assert!(!verifies(pk, &m, sig), "message byte {i} with {mask:#04x}");
        }
        for i in 0..64 {
            let mut s = *sig;
            s[i] ^= mask;
            assert!(!verifies(pk, msg, &s), "signature byte {i} with {mask:#04x}");
        }
    }
}

// A login signature the crate made: every byte of the message of §16.1
// (the context, origin, board, dev, gen and both nonces) and of the
// signature is bound.
// spec: SEAL-SIG-001
#[test]
fn flipping_any_byte_of_a_login() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let p = d.public();
    let origin = "https://relay.example.org";
    let nonce = [0x4E; 32];
    let (cn, sig) = sign_login(&d, origin, &BOARD, 3, &nonce, &mut OsEntropy).unwrap();
    let msg = login_message(origin, &BOARD, &p.dev, 3, &nonce, &cn).unwrap();
    flip_all(&p.sign_pk, &msg, &sig);
}

// Admin signatures the crate made, by a device and by the recovery
// identity: every byte of the message of §16.2 and of the signature is
// bound, and so is every byte of the `args`: with one flipped, the message
// the relay rebuilds (with `args_hash = H(args)`) does not verify.
// spec: SEAL-SIG-003
// spec: SEAL-SIG-004
#[test]
fn flipping_any_byte_of_an_admin_operation() {
    let d = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let rk = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
    let nonce = [0x4E; 32];
    let other = DeviceKeys::generate(&mut OsEntropy).unwrap().public();
    let cases: Vec<(Signer, DevId, [u8; 32], AdminOp)> = vec![
        (Signer::Device(&d), d.public().dev, d.public().sign_pk, AdminOp::Revoke(other.dev)),
        (
            Signer::Device(&d),
            d.public().dev,
            d.public().sign_pk,
            AdminOp::Register { dev: other.dev, sign_pk: other.sign_pk, frame_hash: FrameHash([7; 32]) },
        ),
        (Signer::Recovery(&rk, BOARD), rk.public().dev(&BOARD), rk.public().sign_pk, AdminOp::Revoke(other.dev)),
    ];
    for (signer, dev, pk, op) in cases {
        let sig = sign_admin(&signer, &op, &BOARD, &nonce).unwrap();
        let msg = admin_message(&op, &BOARD, &dev, &nonce).unwrap();
        flip_all(&pk, &msg, &sig);
        // The args: the hash sits 32 bytes before the end of the message.
        let args = op.args();
        assert_eq!(&msg[msg.len() - 64..msg.len() - 32], &sha(&args));
        for i in 0..args.len() {
            let mut a = args.clone();
            a[i] ^= 0x01;
            let mut m = msg.clone();
            let n = m.len();
            m[n - 64..n - 32].copy_from_slice(&sha(&a));
            assert!(!verifies(&pk, &m, &sig), "{} args byte {i}", op.name());
        }
    }
}
