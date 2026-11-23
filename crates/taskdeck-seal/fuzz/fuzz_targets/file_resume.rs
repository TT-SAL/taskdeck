//! Fuzz target: FileSealer::resume (§19.1). Row `file_resume` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `FileSealer::resume`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let f = files();
    let mut i = Input::new(data);
    let records = u64::from(i.byte());
    for h in [i.rest().to_vec(), mutate(&f.sealed[..55], i.rest())] {
        if let Ok(mut s) = FileSealer::resume(&f.key, &h, records) {
            assert!(s.seal_record(b"x", &mut fixed(64)).is_ok());
        }
    }
});
