//! The whole-crate rules of seal.md §3 that no function call can show:
//! checked on the crate's sources and manifest (task S7), and the fuzz
//! crate's lockfile against the root's (SYNC_PLAN.md §3.4). Each module
//! reads the files under the crate; no script under `scripts/` is involved
//! (lead ruling of S7).

#[path = "../common/mod.rs"]
mod common;

mod s7_lockfile;
mod s7_sources;
