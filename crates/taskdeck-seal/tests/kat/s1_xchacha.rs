//! draft-irtf-cfrg-xchacha-03 Appendix A and Wycheproof
//! `xchacha20_poly1305_test.json` (XChaCha20-Poly1305, README §5.5), through
//! the `chacha20poly1305` calls the crate's AEAD makes (tasks S2 and S4).
//!
//! These suites are required by seal.md §24 itself; no clause of §3 to §21
//! names the AEAD's known answers, so these tests cite none.

use crate::common::{b, read_json, s};
use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

fn seal(key: &[u8], nonce: &[u8], msg: &[u8], aad: &[u8]) -> Option<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(key).ok()?;
    let nonce = XNonce::try_from(nonce).ok()?;
    cipher.encrypt(&nonce, Payload { msg, aad }).ok()
}

fn open(key: &[u8], nonce: &[u8], ct: &[u8], aad: &[u8]) -> Option<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(key).ok()?;
    let nonce = XNonce::try_from(nonce).ok()?;
    cipher.decrypt(&nonce, Payload { msg: ct, aad }).ok()
}

#[test]
fn draft_appendix_a_3_1_aead() {
    let doc = read_json("tests/kat/data/xchacha.json");
    let c = &doc["aead"][0];
    let ct = seal(&b(c, "key"), &b(c, "iv"), &b(c, "plaintext"), &b(c, "aad")).unwrap();
    assert_eq!(ct, [b(c, "ciphertext"), b(c, "tag")].concat(), "{}", s(c, "name"));
    assert_eq!(open(&b(c, "key"), &b(c, "iv"), &ct, &b(c, "aad")).unwrap(), b(c, "plaintext"));
}

// The AEAD encrypts its message with the keystream from block counter 1
// (RFC 8439 §2.8), so its ciphertext without the tag is A.3.2.2's.
#[test]
fn draft_appendix_a_3_2_2_keystream_at_counter_1() {
    let doc = read_json("tests/kat/data/xchacha.json");
    let c = &doc["stream_counter_1"][0];
    let pt = b(c, "plaintext");
    let ct = seal(&b(c, "key"), &b(c, "iv"), &pt, b"").unwrap();
    assert_eq!(ct[..pt.len()], b(c, "ciphertext")[..], "{}", s(c, "name"));
    let ks: Vec<u8> = pt.iter().zip(&ct).map(|(p, c)| p ^ c).collect();
    assert_eq!(ks, b(c, "keystream"));
}

// Every case of Wycheproof xchacha20_poly1305_test.json gives its expected
// result: a valid case seals to its ciphertext and tag and opens; an invalid
// one does not open (a nonce of another size cannot even be given).
#[test]
fn wycheproof_xchacha20_poly1305() {
    let doc = read_json("tests/kat/data/xchacha20_poly1305_test.json");
    let mut n = 0;
    for g in doc["testGroups"].as_array().unwrap() {
        for t in g["tests"].as_array().unwrap() {
            let (key, iv, aad, msg) = (b(t, "key"), b(t, "iv"), b(t, "aad"), b(t, "msg"));
            let ct = [b(t, "ct"), b(t, "tag")].concat();
            match s(t, "result") {
                "valid" => {
                    assert_eq!(seal(&key, &iv, &msg, &aad), Some(ct.clone()), "tcId {}", t["tcId"]);
                    assert_eq!(open(&key, &iv, &ct, &aad), Some(msg), "tcId {}", t["tcId"]);
                }
                "invalid" => assert_eq!(open(&key, &iv, &ct, &aad), None, "tcId {}", t["tcId"]),
                other => panic!("tcId {}: no expectation for {other}", t["tcId"]),
            }
            n += 1;
        }
    }
    assert_eq!(n, doc["numberOfTests"].as_u64().unwrap());
}
