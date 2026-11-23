//! Fuzz target: canonical JSON, the payload and item parsers, unpadding (§10.3, §11), private. Row `payloads` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `classify`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    // A frame sealed properly by a member around the input as its payload
    // (or a real payload mutated), of the kind the first byte names: every
    // parser after the AEAD runs.
    let f = frames();
    let s = &f.sim;
    let mut i = Input::new(data);
    let k = i.byte();
    let rest = i.rest();
    let kind = [Kind::Cmd, Kind::Ctl, Kind::Beat, Kind::Snap][usize::from(k % 4)];
    let payload = if k & 4 == 0 { rest.to_vec() } else { mutate(&f.payloads[usize::from(k % 4)], rest) };
    let e = s.epoch();
    let key = derive_epoch_key(&EpochRoot::from_bytes(s.roots[&e]), &s.board, e, KeyKind::Entry);
    let plain = Plain { entry_id: EntryId([5; 16]), kind, at_ms: s.now, based_on: s.head(), payload };
    let Ok(frame) = seal_frame(&s.signer(Who::D(0)), &s.board, s.next_ctr(Who::D(0)), e, &key, &plain, &mut fixed(24)) else {
        return;
    };
    if let Err(e) = replay(&s.inputs("", &frame, &Opts::default())) {
        assert_eq!(e, SealError::MissingRoot);
    }
});
