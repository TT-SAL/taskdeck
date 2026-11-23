//! Fuzz target: open_snapshot (§20.5). Row `open_snapshot` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `open_snapshot`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let b = blobs();
    let s = &b.sim;
    let mut i = Input::new(data);
    let split = usize::from(i.byte());
    let inflated = i.u64() % (1 << 27);
    for bytes in [i.rest().to_vec(), mutate(&b.snapshot, i.rest())] {
        let r = BlobRef { sha256: backup::sha(&[&bytes]), ..blob::blob_ref(&bytes, inflated, 2) };
        let at = split.min(bytes.len());
        for parts in [split_parts(&bytes), vec![&bytes[..at], &bytes[at..]]] {
            let _ = open_snapshot(&parts, &r, s.seq(), &b.hash, 1, &b.writer, &blob::snap_key(s), &s.board);
        }
    }
});
