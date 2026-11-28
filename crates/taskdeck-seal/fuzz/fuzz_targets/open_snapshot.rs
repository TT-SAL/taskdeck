//! Fuzz target: open_snapshot (§20.5). Row `open_snapshot` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `open_snapshot`:
//! mutated sealed bytes (steps 1 to 4), and padded streams, compressed
//! streams, bodies and body fields shaped from the input and sealed properly
//! here (a padded stream as it is, with no padding added), with the
//! expected state hash the body's own, so that the padding, the inflate, the
//! body parser, the membership parser, the projection and the agreement
//! check run (`reach_open_snapshot`).
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let _ = reach_open_snapshot(data);
});
