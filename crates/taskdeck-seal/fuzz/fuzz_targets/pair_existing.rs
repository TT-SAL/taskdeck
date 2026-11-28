//! Fuzz target: E's machine (§17.10), events and slot B. Row `pair_existing` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `pair::Existing::on`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let mut i = Input::new(data);
    let mut r = pairing::Run::new(Sim::new(722, 1), 0);
    if i.byte() % 2 == 1 {
        r.step_b();
    }
    for _ in 0..8 {
        if i.is_empty() {
            break;
        }
        let ev = e_event(&mut i, &r);
        let _ = r.e.on(ev, &mut fixed(256));
    }
});
