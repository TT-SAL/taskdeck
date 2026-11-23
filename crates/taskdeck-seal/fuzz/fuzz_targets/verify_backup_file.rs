//! Fuzz target: verify_backup_file (§21.6 steps 1, 2, 5). Row `verify_backup_file` of the parser table
//! (`tests/parsers/main.rs`); the function under test is `verify_backup_file`.
#![no_main]

use libfuzzer_sys::fuzz_target;
#[allow(unused_imports)]
use taskdeck_seal::*;
#[allow(unused_imports)]
use taskdeck_seal_fuzz::*;

fuzz_target!(|data: &[u8]| {
    let b = backups();
    let pk = b.sim.devs[0].public().sign_pk;
    let m = mutate(&b.backup, data);
    assert!(verify_backup_file(&m, &pk).is_err() || m == b.backup);
    let _ = verify_backup_file(data, &pk);
});
