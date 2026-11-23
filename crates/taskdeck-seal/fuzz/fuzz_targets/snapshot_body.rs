//! Fuzz target: the inflate and the snapshot body parser (§20.4), private. Row `snapshot_body` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `open_snapshot`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    // Bytes sealed here around a mutated compressed stream, the DEFLATE of a
    // mutated body, or the input as the stream: the inflate and the body
    // parser after the AEAD run.
    let b = blobs();
    let s = &b.sim;
    let mut i = Input::new(data);
    let which = i.byte();
    let slack = u64::from(i.byte() % 3);
    let rest = i.rest();
    let compressed = match which % 3 {
        0 => mutate(&b.compressed, rest),
        1 => backup::deflate(&mutate(&b.body, rest)),
        _ => rest.to_vec(),
    };
    let raw = blob::Raw::new(1, 2, s.board, s.seq(), b.writer, [3; 16], [4; 24]);
    let sealed = raw.seal(&blob::kind_key(&s.board, &s.roots[&2], 2, "snap"), &blob::pad(&compressed));
    let inflated = (b.body.len() as u64).saturating_sub(1) + slack;
    let r = BlobRef { sha256: backup::sha(&[&sealed]), ..blob::blob_ref(&sealed, inflated, 2) };
    let _ = open_snapshot(&split_parts(&sealed), &r, s.seq(), &b.hash, 1, &b.writer, &blob::snap_key(s), &s.board);
});
