//! Fuzz target: HistoryRecord::from_accepted (§21.5). Row `from_accepted` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `HistoryRecord::from_accepted`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let f = skip();
    let o = Opts { targets: vec![f.target], ..Opts::default() };
    let mut opened = f.sim.classify(&f.skip, &o).unwrap().opened.unwrap();
    let mut i = Input::new(data);
    let which = i.byte();
    let seq = i.u64();
    let rest = i.rest();
    match which % 4 {
        0 => opened.payload = rest.to_vec(),
        1 => opened.payload = mutate(&opened.payload, rest),
        2 => opened.kind = Kind::Beat,
        _ => {}
    }
    for frame in [rest.to_vec(), mutate(&f.skip, rest), f.skip.clone()] {
        if let Ok(h) = HistoryRecord::from_accepted(seq, &frame, &opened) {
            assert!(HistoryRecord::from_bytes(&h.to_bytes()).is_ok());
        }
    }
});
