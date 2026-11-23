//! Fuzz target: unwrap_root (§7). Row `unwrap_root` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `unwrap_root`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let w = wrap();
    let mut i = Input::new(data);
    let epoch = i.byte();
    let m = mutate(&w.wrap, i.rest());
    let mut b = [0u8; 96];
    for (d, s) in b.iter_mut().zip(m.iter()) {
        *d = *s;
    }
    let r = unwrap_root(&b, &common::BOARD, u32::from(epoch % 2) + w.epoch, &w.commit, &Recipient::Device(&w.device));
    assert!(r.is_err() || (b == w.wrap && epoch % 2 == 0));
});
