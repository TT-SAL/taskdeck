//! `taskdeck-core`: the pure board. Data, log commands, the deterministic
//! versioned `apply`, planner, archive, scoring, the ICS parser and the
//! snapshot builders.
//!
//! Spec: `docs/sync/spec/core.md`. Owner: stage `core` (SYNC_PLAN.md §4,
//! §6.2). Created as a shell by stage `foundation`; the skeleton types come
//! from the spec's Public API (§20). Every body is `unimplemented!()` until
//! stage `core` fills it: `grep -rn 'unimplemented!()' crates/taskdeck-core`
//! lists what is left.
//!
//! No clock, zone, file, network, thread, environment or randomness here
//! (CORE-GEN-003): `clippy.toml` in this directory disallows them.
#![forbid(unsafe_code)]
// Skeleton (task 4): the bodies are `unimplemented!()`, so the parameters the
// spec names are not read yet. Stage `core` removes this line.
#![allow(unused_variables)]

/// An error of the catalogue (core.md §16, §20).
pub struct CoreError {
    pub code: &'static str,
    pub detail: Option<u32>,
}

/// An error of the compile step: code, HTTP status, message (core.md §13.1, §16, §20).
pub struct CompileError {
    pub code: &'static str,
    pub status: u16,
    pub message: String,
}

pub mod apply;
pub mod archive;
pub mod board;
pub mod canon;
pub mod command;
pub mod compile;
pub mod ics;
pub mod ids;
pub mod intent;
pub mod legacy;
pub mod overlay;
pub mod planner;
pub mod read;
pub mod reminders;
pub mod snapshot;
pub mod tasks;
pub mod time;
pub mod weather;

#[cfg(test)]
mod tests {
    use proptest as _;
    use sha2 as _;

    #[test]
    fn the_shell_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "taskdeck-core");
    }
}
