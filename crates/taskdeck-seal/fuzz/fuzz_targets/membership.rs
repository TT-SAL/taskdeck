//! Fuzz target: Membership::from_canonical_bytes (§13.2). Row `membership` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `Membership::from_canonical_bytes`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let f = frames();
    for input in [data.to_vec(), mutate(&f.membership, data)] {
        if let Ok(m) = Membership::from_canonical_bytes(&input) {
            assert_eq!(m.to_canonical_bytes(), input);
        }
    }
});
