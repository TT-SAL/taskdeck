//! Fuzz target: open_snapshot_declared (§20.5). Row `open_snapshot_declared` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `open_snapshot_declared`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let b = blobs();
    let s = &b.sim;
    let root = EpochRoot::from_bytes(s.roots[&2]);
    let roots = |e: u32| if e == 2 { Some(&root) } else { None };
    let mut i = Input::new(data);
    let size = i.u64() % (1 << 27);
    for bytes in [i.rest().to_vec(), mutate(&b.snapshot, i.rest())] {
        let sha = backup::sha(&[&bytes]);
        let _ = open_snapshot_declared(&split_parts(&bytes), &sha, size.max(bytes.len() as u64), s.seq(), &b.writer, &roots, &s.board);
    }
});
