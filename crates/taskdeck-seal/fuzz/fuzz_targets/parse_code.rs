//! Fuzz target: pair::parse_code (§17.2). Row `parse_code` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `pair::parse_code`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let _ = pair::parse_code(&text(data));
    let _ = pair::parse_code(&text(&mutate(b"0123-4567-89AB-CDEF-GH", data)));
});
