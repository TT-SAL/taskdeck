//! Known-answer suites (seal.md §24, last paragraph), run against the pinned
//! crates the way `taskdeck-seal` calls them. Data under `data/`, sources in
//! `README.md`. Each task adds its module here.

#[path = "../common/mod.rs"]
mod common;

mod s1_ed25519;
mod s1_hkdf;
mod s1_hpke;
mod s1_xchacha;
