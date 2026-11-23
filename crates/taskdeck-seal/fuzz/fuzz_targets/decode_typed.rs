//! Fuzz target: base32::decode_typed (§5.2). Row `decode_typed` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `base32::decode_typed`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let mut i = Input::new(data);
    let n = usize::from(i.byte());
    let t = text(i.rest());
    if let Ok(b) = base32::decode_typed(&t, n) {
        assert_eq!(b.len(), n);
    }
});
