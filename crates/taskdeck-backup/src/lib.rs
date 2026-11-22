//! `taskdeck-backup`: the backup format, its writer and its reader.
//!
//! Spec: `docs/sync/spec/seal.md` §21 (the format) and
//! `docs/sync/spec/device.md` §15 (scheduling, destinations, re-reading).
//! Owner: stage `recovery`, then `device` (SYNC_PLAN.md §4, §6.8). Created as a
//! shell by stage `foundation`.
//!
//! No spec names a public item of this crate yet, so the skeletons task
//! (stage `foundation`, task 4) leaves it a shell: its public surface is
//! written by its owning stage.
#![forbid(unsafe_code)]

// Declared for the stage that fills this crate: each dependency is named once
// so that it is resolved and compiled now, and a wrong feature set fails today
// rather than in wave 1.
use flate2 as _;
use serde_json as _;
use taskdeck_core as _;
use taskdeck_seal as _;
const _: () = {
    #[allow(dead_code)] // never built: it only proves the derives compile
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Declared(u64);
};

#[cfg(test)]
mod tests {
    #[test]
    fn the_shell_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "taskdeck-backup");
    }
}
