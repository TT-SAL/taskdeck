//! Fuzz target: the bundle JSON parser of slot D (§17.8), private. Row `pair_bundle` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `pair::New::on`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    // A bundle plaintext from the input, sealed properly by a test playing E
    // with fixed draws; the harness asserts what the spec says at each size
    // (`reach_pair_bundle`).
    let _ = reach_pair_bundle(data);
});
