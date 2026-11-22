//! `taskdeck-recover`: the offline recovery tool. Reads a backup with the
//! recovery sheet, prints and re-types the sheet, and runs the recovery
//! signer's operations.
//!
//! Spec: `docs/sync/spec/seal.md` §12, §13.12, §18 and §21, and
//! `docs/sync/spec/device.md` §15.8. Owner: stage `recovery`, then `device`
//! (SYNC_PLAN.md §4, §6.8). Created as a shell by stage `foundation`.
#![forbid(unsafe_code)]

// Declared for the stage that fills this crate: each dependency is named once
// so that it is resolved and compiled now.
use taskdeck_backup as _;
use taskdeck_core as _;
use taskdeck_seal as _;

fn main() {
    eprintln!("taskdeck-recover: not built yet (stage recovery)");
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_shell_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "taskdeck-recover");
    }
}
