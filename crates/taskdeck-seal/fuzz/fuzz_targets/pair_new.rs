//! Fuzz target: N's machine (§17.10), events and slots A to D, the bundle,
//! the snapshot and the join. Row `pair_new` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `pair::New::on`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    // A fresh N with the draws of a whole honest run, then the run's events
    // for the phase N is in, mutated (slots MACed again under the code, the
    // bundle sealed again under its key), out of place, ticks, or any event
    // of real size, as the input chooses (`reach_pair_new`).
    let _ = reach_pair_new(data);
});
