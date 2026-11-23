//! Fuzz target: DeviceKeys::from_identity_bytes (§6.4). Row `from_identity_bytes` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `DeviceKeys::from_identity_bytes`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let _ = DeviceKeys::from_identity_bytes(data);
    let w = wrap();
    let id = w.device.to_identity_bytes();
    if let Ok(k) = DeviceKeys::from_identity_bytes(&mutate(&id, data)) {
        let _ = k.public();
    }
});
