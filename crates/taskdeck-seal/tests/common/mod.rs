//! Helpers shared by the integration targets (`#[path = "../common/mod.rs"] mod common;`).
#![allow(dead_code)]

/// Lowercase hex of bytes.
pub fn hex(bytes: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(2 * bytes.len());
    for b in bytes {
        s.push(char::from(D[usize::from(b >> 4)]));
        s.push(char::from(D[usize::from(b & 15)]));
    }
    s
}

/// Bytes of lowercase or uppercase hex; panics on anything else (test data only).
pub fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length: {s}");
    let v = |c: u8| match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => panic!("not hex: {s}"),
    };
    s.as_bytes().chunks(2).map(|p| (v(p[0]) << 4) | v(p[1])).collect()
}

/// A fixed-width array from hex.
pub fn unhex_n<const N: usize>(s: &str) -> [u8; N] {
    unhex(s).try_into().unwrap_or_else(|v: Vec<u8>| panic!("want {N} bytes, got {}", v.len()))
}

/// The crate's directory.
pub fn crate_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Reads a JSON file under the crate.
pub fn read_json(rel: &str) -> serde_json::Value {
    let p = crate_dir().join(rel);
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// A string field of a JSON object.
pub fn s<'a>(v: &'a serde_json::Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or_else(|| panic!("no string {k} in {v}"))
}

/// Hex bytes of a JSON field.
pub fn b(v: &serde_json::Value, k: &str) -> Vec<u8> {
    unhex(s(v, k))
}

/// A board id for tests.
pub const BOARD: taskdeck_seal::BoardId = taskdeck_seal::BoardId([0xB0; 16]);
