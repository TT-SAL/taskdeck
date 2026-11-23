//! Known-answer suites (seal.md §24, last paragraph), run against the pinned
//! crates the way `taskdeck-seal` calls them. Data under `data/`, sources in
//! `README.md`. Each task adds its module here.
//!
//! These tests call only the pinned crates, never `taskdeck_seal`, so they
//! cite no clause: they prove no clause about this crate's derivations,
//! wraps or verification (the unit tests beside the modules do). The suites
//! are required by seal.md §24 and by the stage gate, not by a clause.

#[path = "../common/mod.rs"]
mod common;

mod s1_ed25519;
mod s1_hkdf;
mod s1_hpke;
mod s1_xchacha;
