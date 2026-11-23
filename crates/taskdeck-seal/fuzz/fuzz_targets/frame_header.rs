//! Fuzz target: frame_header (§8.1). Row `frame_header` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `frame_header`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let f = frames();
    let _ = frame_header(data);
    let mut i = Input::new(data);
    let k = usize::from(i.byte());
    let m = mutate(&f.frames[k % f.frames.len()], i.rest());
    if let Ok(h) = frame_header(&m) {
        assert!(m.len() >= 670);
        let _ = h;
    }
});
