//! The fuzz targets of `crates/taskdeck-seal/fuzz/` (task S7, brief item 4),
//! run as ordinary tests over fixed inputs. `ci.sh` only compiles the fuzz
//! crate and stage `release` runs the fuzzers; this target runs every
//! target's body and the fixtures it builds, so that a target that cannot
//! fuzz at all (a fixture that fails on every input) fails here.
//!
//! The fuzz crate's library is included unchanged, with its shared helpers
//! (`common`, `board`, ...) re-exported at the root as they stand in the fuzz
//! crate, so that their `crate::` paths resolve to the same modules. Each
//! target file is included unchanged: `libfuzzer_sys` and
//! `taskdeck_seal_fuzz` name this crate here, and its `fuzz_target!` turns
//! the target's body into a function `run`.
#![cfg(feature = "test-entropy")]

extern crate self as libfuzzer_sys;
extern crate self as taskdeck_seal_fuzz;

#[path = "../../fuzz/src/lib.rs"]
mod fuzz;
pub use fuzz::*;

#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/backup_payload.rs"]
mod t_backup_payload;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/check_move_code.rs"]
mod t_check_move_code;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/classify.rs"]
mod t_classify;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/classify_skip_targets.rs"]
mod t_classify_skip_targets;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/decode_typed.rs"]
mod t_decode_typed;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/file_resume.rs"]
mod t_file_resume;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/frame_header.rs"]
mod t_frame_header;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/from_accepted.rs"]
mod t_from_accepted;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/from_identity_bytes.rs"]
mod t_from_identity_bytes;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/from_sheet.rs"]
mod t_from_sheet;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/history_record.rs"]
mod t_history_record;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/membership.rs"]
mod t_membership;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/open_blob.rs"]
mod t_open_blob;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/open_file.rs"]
mod t_open_file;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/open_snapshot.rs"]
mod t_open_snapshot;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/open_snapshot_declared.rs"]
mod t_open_snapshot_declared;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/pair_bundle.rs"]
mod t_pair_bundle;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/pair_existing.rs"]
mod t_pair_existing;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/pair_new.rs"]
mod t_pair_new;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/parse_code.rs"]
mod t_parse_code;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/payloads.rs"]
mod t_payloads;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/read_backup.rs"]
mod t_read_backup;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/relay_origin.rs"]
mod t_relay_origin;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/snapshot_body.rs"]
mod t_snapshot_body;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/unwrap_root.rs"]
mod t_unwrap_root;
#[allow(unused_attributes, clippy::all)]
#[path = "../../fuzz/fuzz_targets/verify_backup_file.rs"]
mod t_verify_backup_file;

mod s7_fuzz;

// The body of a target as a function `run(data)`. Defined after the targets,
// so that each reaches it through its own `use libfuzzer_sys::fuzz_target`.
macro_rules! fuzz_target {
    (|$data:ident: &[u8]| $body:block) => {
        pub fn run($data: &[u8]) $body
    };
}
pub(crate) use fuzz_target;
