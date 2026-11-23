//! Fuzz target: open_blob (§20.5). Row `open_blob` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `open_blob`, its
//! header and AEAD on mutated sealed bytes, and its padding check on padded
//! bodies sealed properly here (`reach_open_blob`).
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let _ = reach_open_blob(data);
});
