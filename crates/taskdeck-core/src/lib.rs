//! `taskdeck-core`: the pure board. Data, log commands, the deterministic
//! versioned `apply`, planner, archive, scoring, the ICS parser and the
//! snapshot builders.
//!
//! Spec: `docs/sync/spec/core.md`. Owner: stage `core` (SYNC_PLAN.md §4,
//! §6.2). Created as a shell by stage `foundation`; the skeleton types come
//! from the spec's Public API (§20).
//!
//! No clock, zone, file, network, thread, environment or randomness here
//! (CORE-GEN-003): `clippy.toml` in this directory disallows them.
#![forbid(unsafe_code)]

// Declared for the stage that fills this crate: each dependency is named once
// so that it is resolved and compiled now, and a wrong feature set fails today
// rather than in wave 1.
use chrono as _;
use serde_json as _;
const _: () = {
    // `derive` is on.
    #[allow(dead_code)] // never built: it only proves the derives compile
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Declared(chrono::NaiveDate);
};

#[cfg(test)]
mod tests {
    use proptest as _;
    use sha2 as _;

    #[test]
    fn the_shell_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "taskdeck-core");
    }
}
