//! Fuzz target: check_move_code (§15.4). Row `check_move_code` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `check_move_code`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let mut i = Input::new(data);
    let seq = i.u64();
    let fh = FrameHash([0x17; 32]);
    let code = move_code(&common::BOARD, seq, &fh);
    let _ = check_move_code(&common::BOARD, seq, &fh, &text(i.rest()));
    let m = text(&mutate(code.as_bytes(), i.rest()));
    let _ = check_move_code(&common::BOARD, seq, &fh, &m);
});
