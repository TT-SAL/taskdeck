//! Fuzz target: read_backup (§21.6). Row `read_backup` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `read_backup`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let b = backups();
    let rk = b.sim.rec.last().unwrap();
    let mut i = Input::new(data);
    let file = if i.byte() % 2 == 0 { &b.backup } else { &b.segment };
    let m = mutate(file, i.rest());
    let r = read_backup(&m, &b.sim.board, rk);
    assert!(r.is_err() || &m == file);
    let _ = read_backup(i.rest(), &b.sim.board, rk);
});
