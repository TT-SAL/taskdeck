//! The published vectors (seal.md §24, README §7): each family is built from
//! fixed inputs through the public API and compared byte for byte with
//! `docs/sync/vectors/<family>`; with `SEAL_VECTORS=write` the file is
//! written instead. One module per family; each task adds its own.
#![cfg(feature = "test-entropy")]

#[path = "../common/mod.rs"]
mod common;

mod s1_chain;
mod s1_kdf;
mod s1_sheet;
mod s1_wrap;

use serde_json::Value;

/// Compares `doc` with the committed family `file`, or writes it.
pub fn check_or_write(file: &str, doc: Value) {
    let dir = common::crate_dir().join("../../docs/sync/vectors");
    let path = dir.join(file);
    let mut text = serde_json::to_string_pretty(&doc).unwrap();
    text.push('\n');
    if std::env::var("SEAL_VECTORS").as_deref() == Ok("write") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, text).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (run with SEAL_VECTORS=write to create it)", path.display()));
    assert!(committed == text, "{} differs from what the code builds (SEAL_VECTORS=write rewrites it)", path.display());
}

/// `n` bytes counting up from `start`, for readable fixed inputs.
pub fn count(start: u8, n: usize) -> Vec<u8> {
    (0..n).map(|i| start.wrapping_add(i as u8)).collect()
}
