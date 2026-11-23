//! Fuzz target: N's machine (§17.10), events and slots A to D. Row `pair_new` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `pair::New::on`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    // N in the middle of a real pairing, then events read from the input,
    // slots among them mutated from E's and re-MACed when the input asks.
    let mut i = Input::new(data);
    let mut r = pairing::Run::new(Sim::new(721, 1), 0);
    let stage = i.byte() % 4;
    let _ = r.n_on(r.code_event());
    if stage >= 1 {
        let _ = r.n_on(pair::NewEvent::Slot { bytes: r.slot_a.clone() });
    }
    for _ in 0..8 {
        if i.is_empty() {
            break;
        }
        let ev = n_event(&mut i, &r);
        let _ = r.n.on(ev, &mut fixed(256));
    }
});
