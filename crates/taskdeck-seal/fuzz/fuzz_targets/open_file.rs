//! Fuzz target: open_file (§19.4). Row `open_file` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `open_file`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let f = files();
    for input in [data.to_vec(), mutate(&f.sealed, data)] {
        if let Ok(o) = open_file(&f.key, FileKind::Checkpoint, &f.board, &f.dev, &input) {
            assert!(o.records.len() <= 3);
        }
    }
});
