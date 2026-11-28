//! Fuzz target: classify, frames (§14). Row `classify` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `classify`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let f = frames();
    let mut i = Input::new(data);
    let k = usize::from(i.byte());
    let shift = u64::from(i.byte() % 4);
    let rest = i.rest();
    let o = Opts { own: Some(1), seq: Some(f.sim.seq() + 1 + shift), ..Opts::default() };
    for frame in [rest.to_vec(), mutate(&f.frames[k % f.frames.len()], rest)] {
        let c = f.sim.inputs("", &frame, &o);
        if let Err(e) = replay(&c) {
            assert_eq!(e, SealError::MissingRoot);
        }
    }
});
