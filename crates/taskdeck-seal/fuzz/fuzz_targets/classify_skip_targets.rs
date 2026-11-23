//! Fuzz target: classify, Skip targets (§12.9, §14.1). Row `classify_skip_targets` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `classify`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let f = skip();
    let o = Opts { targets: vec![f.target], ..Opts::default() };
    let mut c = f.sim.inputs("", &f.skip, &o);
    for t in [data.to_vec(), mutate(&f.target_frame, data)] {
        c["skip_targets"] = serde_json::json!({ f.target.to_string(): common::hex(&t) });
        match replay(&c) {
            Ok(r) => assert!(!matches!(r.class, Class::Accept) || t == f.target_frame),
            Err(e) => assert_eq!(e, SealError::MissingRoot),
        }
    }
});
