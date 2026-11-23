//! Fuzz target: the backup payload parsers after HPKE, and inflate (§21.4), private. Row `backup_payload` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `read_backup`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    // A member seals and signs a file around a mutated payload, its DEFLATE,
    // or a mutated compressed stream: every parser after the HPKE layer runs.
    let b = backups();
    let s = &b.sim;
    let mut i = Input::new(data);
    let which = i.byte();
    let (p, ft, seq) = if which % 2 == 0 { (&b.payload, 1, s.seq() - 1) } else { (&b.segment_payload, 2, s.seq()) };
    let rest = i.rest();
    let compressed = match which / 2 % 3 {
        0 => backup::deflate(&mutate(p, rest)),
        1 => mutate(&backup::deflate(p), rest),
        _ => rest.to_vec(),
    };
    let h = backup::Header::of(s, &s.devs[0], ft, seq);
    let f = backup::seal_compressed(&h, &s.devs[0], &compressed, [9; 32]);
    let _ = read_backup(&f, &s.board, s.rec.last().unwrap());
});
