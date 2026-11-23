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

/// An error of the catalogue (core.md §16, §20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreError {
    pub code: &'static str,
    pub detail: Option<u32>,
}

/// An error of the compile step: code, HTTP status, message (core.md §13.1, §16, §20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub code: &'static str,
    pub status: u16,
    pub message: String,
}

impl CoreError {
    /// An error with a code and no detail.
    pub(crate) const fn code(code: &'static str) -> CoreError {
        CoreError { code, detail: None }
    }

    /// `core/invariant` naming the first failing rule of core.md §6.12.
    pub(crate) const fn invariant(rule: u32) -> CoreError {
        CoreError { code: "core/invariant", detail: Some(rule) }
    }
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
