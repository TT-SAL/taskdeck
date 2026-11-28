//! Fuzz target: RecoverySeed::from_sheet (§18.2). Row `from_sheet` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `RecoverySeed::from_sheet`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let _ = RecoverySeed::from_sheet(&text(data));
    let sheet = sheet();
    let _ = RecoverySeed::from_sheet(&text(&mutate(sheet.as_bytes(), data)));
});
