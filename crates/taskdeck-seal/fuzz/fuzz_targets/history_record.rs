//! Fuzz target: HistoryRecord::from_bytes (§21.5). Row `history_record` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `HistoryRecord::from_bytes`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let b = backups();
    let mut i = Input::new(data);
    let k = usize::from(i.byte());
    let rest = i.rest();
    for input in [rest.to_vec(), mutate(&b.records[k % b.records.len()], rest)] {
        if let Ok(r) = HistoryRecord::from_bytes(&input) {
            assert_eq!(r.to_bytes(), input);
        }
    }
});
