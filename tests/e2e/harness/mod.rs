//! The harness of the end-to-end suite (device.md §19, §20.1): the standby
//! relay on loopback (`standby.rs`), a `taskdeck-devicectl` process driven by
//! the protocol of §19.2 (`ctl.rs`), and a minimal HTTP client (`http.rs`).
//!
//! Scenario tests are written with `Standby` and `Ctl` alone. Helpers are
//! written for the scenarios of later tasks, so some are unused here.
#![allow(dead_code, unused_imports)]

mod ctl;
mod http;
mod node;
mod standby;

pub use ctl::{Board, Ctl, CtlError, CtlOptions, PairStart, Sheet, Source};
pub use http::{HttpReply, http_get, http_request};
pub use node::find_node;
pub use standby::Standby;

/// The device under test: `taskdeck-devicectl`, built with `test-trust` for
/// this test target (`required-features`), which Cargo then places beside it.
pub const DEVICECTL: &str = env!("CARGO_BIN_EXE_taskdeck-devicectl");

/// Lowercase hex, the form of every byte string of the protocol (§19.2).
pub fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// `n` random bytes from the operating system, as lowercase hex.
pub fn random_hex(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).expect("the operating system's randomness");
    hex_encode(&bytes)
}
