//! Fuzz target: open_snapshot_declared (§20.5). Row `open_snapshot_declared` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `open_snapshot_declared`:
//! as `open_snapshot`, with the snapshot sealed under the declared root's key
//! and no hash compared, so that a mutated membership and history reach
//! their parsers directly (`reach_open_snapshot_declared`).
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let _ = reach_open_snapshot_declared(data);
});
