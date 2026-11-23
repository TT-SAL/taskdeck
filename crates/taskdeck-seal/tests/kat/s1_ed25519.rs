//! RFC 8032 §7.1 and Wycheproof `ed25519_test.json`, verified with
//! `verify_strict` as the crate verifies every signature (README §5.5).

use crate::common::{b, read_json, s};
use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};

fn strict(pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let Ok(pk) = <[u8; 32]>::try_from(pk) else { return false };
    let Ok(vk) = VerifyingKey::from_bytes(&pk) else { return false };
    let Ok(sig) = Signature::from_slice(sig) else { return false };
    vk.verify_strict(msg, &sig).is_ok()
}

// RFC 8032 §7.1: the public key and signature of each secret key, and
// strict verification of each signature (and refusal of a flipped one).
// spec: SEAL-GEN-010
#[test]
fn rfc8032_section_7_1() {
    let doc = read_json("tests/kat/data/rfc8032.json");
    let cases = doc["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 5);
    for c in cases {
        let sk: [u8; 32] = b(c, "secret_key").try_into().unwrap();
        let key = SigningKey::from_bytes(&sk);
        let msg = b(c, "message");
        assert_eq!(key.verifying_key().to_bytes().to_vec(), b(c, "public_key"), "{}", s(c, "name"));
        let sig = key.sign(&msg);
        assert_eq!(sig.to_bytes().to_vec(), b(c, "signature"), "{}", s(c, "name"));
        assert!(strict(&b(c, "public_key"), &msg, &b(c, "signature")));
        let mut bad = b(c, "signature");
        bad[63] ^= 0x01;
        assert!(!strict(&b(c, "public_key"), &msg, &bad));
    }
}

/// The result strict verification must give for a Wycheproof case.
///
/// `valid` and `invalid` are Wycheproof's own. The `acceptable` cases are
/// decided by README §5.5's strict rules ("canonical S, no small-order
/// public key or R", the `verify_strict` behaviour): those flagged
/// `SmallOrderPublicKey` must fail; those flagged `MixedOrderPublicKey` (a
/// prime-order point plus a torsion point, which is not of small order)
/// carry a signature that is valid under the cofactorless equation and must
/// verify. Any other `acceptable` case fails this test until its expectation
/// is written here.
fn expected(t: &serde_json::Value) -> bool {
    let flags: Vec<&str> = t["flags"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    match s(t, "result") {
        "valid" => true,
        "invalid" => false,
        "acceptable" if flags == ["SmallOrderPublicKey"] => false,
        "acceptable" if flags == ["MixedOrderPublicKey"] => true,
        other => panic!("tcId {}: no expectation for {other} {flags:?}", t["tcId"]),
    }
}

// Every case of Wycheproof ed25519_test.json gives its expected result under
// strict verification: non-canonical S, small-order keys and small-order R fail.
// spec: SEAL-GEN-010
#[test]
fn wycheproof_ed25519() {
    let doc = read_json("tests/kat/data/ed25519_test.json");
    let mut n = 0;
    for g in doc["testGroups"].as_array().unwrap() {
        let pk = b(&g["publicKey"], "pk");
        for t in g["tests"].as_array().unwrap() {
            let got = strict(&pk, &b(t, "msg"), &b(t, "sig"));
            assert_eq!(got, expected(t), "tcId {} {}", t["tcId"], s(t, "comment"));
            n += 1;
        }
    }
    assert_eq!(n, doc["numberOfTests"].as_u64().unwrap());
}
