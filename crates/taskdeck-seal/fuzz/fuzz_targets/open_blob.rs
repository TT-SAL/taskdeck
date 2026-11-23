//! Fuzz target: open_blob (§20.5). Row `open_blob` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `open_blob`.
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
    for bytes in [i.rest().to_vec(), mutate(&b.blob, i.rest())] {
        let r = BlobRef { sha256: backup::sha(&[&bytes]), ..blob::blob_ref(&bytes, 0, 2) };
        let at = split.min(bytes.len());
        for parts in [split_parts(&bytes), vec![&bytes[..at], &bytes[at..]]] {
            let _ = open_blob(&parts, &r, &b.writer, &blob::blob_key(s), &s.board);
        }
    }
});
