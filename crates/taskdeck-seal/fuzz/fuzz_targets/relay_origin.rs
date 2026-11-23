//! Fuzz target: relay_origin (§5.4). Row `relay_origin` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `relay_origin`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let _ = relay_origin(&text(data));
    let _ = relay_origin(&text(&mutate(b"relay.example.org:8443", data)));
});
