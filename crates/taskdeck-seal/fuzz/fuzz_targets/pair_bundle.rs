//! Fuzz target: the bundle JSON parser of slot D (§17.8), private. Row `pair_bundle` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `pair::New::on`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    // A mutated bundle JSON, sealed properly by a test playing E.
    let board = BoardId([0xB0; 16]);
    let s = SnapAnnounce { seq: 70, hash: [1; 32], hash_version: 1, blob: BlobId([2; 16]), sha256: [3; 32], size: 900, parts: 1, inflated: 900, epoch: 2 };
    let json = pairing::bundle_json(&board, pairing::HOST, &pairing::DOOR, &[(1, [7; 32]), (2, [8; 32])], Head { seq: 70, chain: Chain([5; 32]) }, &s, 71, pairing::TOKEN);
    let mut i = Input::new(data);
    let raw = i.byte() % 2 == 1;
    let pt = if raw { i.rest().to_vec() } else { mutate(&json, i.rest()) };
    let w = pairing::WithFakeE::new(board, DevId([0xE0; 16])).accept();
    let d = w.slot_d(&pt);
    let mut n = w.n;
    let fx = n.on(pair::NewEvent::Slot { bytes: d }, &mut fixed(256));
    assert!(pairing::n_abort(&fx) != Some(SealError::SlotMac));
});
