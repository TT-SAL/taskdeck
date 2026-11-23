//! RFC 5869 Appendix A, test cases 1 to 3 (HKDF-SHA256), through the `hkdf`
//! calls the crate makes (extract with a salt, expand with the info in parts).

use crate::common::{b, read_json, s};
use sha2::Sha256;

// HKDF as seal.md §2 defines it, which the recovery keys (§6.1) and the
// per-epoch keys (§6.5) are derived with.
// spec: SEAL-KEY-002
// spec: SEAL-KEY-003
// spec: SEAL-KEY-012
#[test]
fn rfc5869_cases_1_to_3() {
    let doc = read_json("tests/kat/data/rfc5869.json");
    let cases = doc["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 3);
    for c in cases {
        let (prk, hk) = hkdf::Hkdf::<Sha256>::extract(Some(&b(c, "salt")), &b(c, "ikm"));
        assert_eq!(prk.to_vec(), b(c, "prk"), "{}", s(c, "name"));
        let info = b(c, "info");
        let mut okm = vec![0u8; c["l"].as_u64().unwrap() as usize];
        let (a, z) = info.split_at(info.len() / 2);
        hk.expand_multi_info(&[a, z], &mut okm).unwrap();
        assert_eq!(okm, b(c, "okm"), "{}", s(c, "name"));
    }
}
