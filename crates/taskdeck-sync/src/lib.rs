//! `taskdeck-sync`: the sans-IO replica engine, a deterministic function from
//! inputs to effects, generic over a state machine and a sealer it defines
//! itself (decision D6), and its simulator.
//!
//! Spec: `docs/sync/spec/engine.md`. Owner: stage `engine`, then `device`
//! (SYNC_PLAN.md §4). Created as a shell by stage `foundation`; the skeleton
//! types come from the spec's Public API (§37).
//!
//! No clock, file, network, thread, environment or randomness here
//! (ENG-GEN-003): `clippy.toml` in this directory disallows them.
#![forbid(unsafe_code)]

// Declared for the stage that fills this crate: each dependency is named once
// so that it is resolved and compiled now, and a wrong feature set fails today
// rather than in wave 1.
const _: () = {
    let _ = <sha2::Sha256 as sha2::Digest>::new;
    let _ = zeroize::Zeroizing::<Vec<u8>>::new;
};

#[cfg(test)]
mod tests {
    use proptest as _;
    use serde as _;
    use serde_json as _;

    #[test]
    fn the_shell_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "taskdeck-sync");
    }
}
