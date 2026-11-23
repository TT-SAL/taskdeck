//! Property tests: no parser panics on random or mutated input (SEAL-GEN-011).
//! Each task adds its module here.
//!
//! The table: every function that takes bytes or text from outside, its
//! property test over random inputs and over valid inputs mutated (bytes
//! changed, inserted, removed, cut, length or count fields raised to their
//! extreme), and its fuzz target (`crates/taskdeck-seal/fuzz/fuzz_targets/`).
//! Private parsers are tested beside their module (`src/<module>/tests.rs`)
//! and fuzzed through the public call that reaches them, sealed properly
//! around the mutated bytes so that they pass the AEAD. The targets of the
//! openers and of pairing are functions of the fuzz crate's library
//! (`reach_*`) that return how far an input got; `tests/fuzz_targets`
//! (`s9_reach`) proves that each reaches the parsers its rows name.
//!
//! | Parser                                    | Property test                                                          | Fuzz target              |
//! |-------------------------------------------|------------------------------------------------------------------------|--------------------------|
//! | `frame_header`                            | `s2_frames::frame_header_never_panics`                                 | `frame_header`           |
//! | `classify`, the frame                     | `s3_membership::classify_never_panics`                                 | `classify`               |
//! | `classify`, the Skip targets              | `s7_parsers::classify_never_panics_on_skip_targets`                    | `classify_skip_targets`  |
//! | `Membership::from_canonical_bytes`        | `s3_membership::membership_parser_never_panics`                        | `membership`; in a snapshot body, `open_snapshot`, `open_snapshot_declared` |
//! | `unwrap_root`                             | `s1_parsers::unwrap_root_never_panics`, `unwrap_root_of_mutated_wraps` | `unwrap_root`            |
//! | `RecoverySeed::from_sheet`                | `s1_parsers::from_sheet_never_panics`, `from_sheet_of_mutated_sheets`  | `from_sheet`             |
//! | `base32::decode_typed`                    | `s1_parsers::decode_typed_never_panics`, `..._of_mutated_encodings`    | `decode_typed`           |
//! | `check_move_code`                         | `s1_parsers::check_move_code_never_panics`                             | `check_move_code`        |
//! | `DeviceKeys::from_identity_bytes`         | `s1_parsers::from_identity_bytes_never_panics`, `..._of_mutated_identities` | `from_identity_bytes` |
//! | `relay_origin`                            | `s1_parsers::relay_origin_never_panics`, `..._of_mutated_hosts`        | `relay_origin`           |
//! | `open_file`                               | `s4_parsers::open_file_never_panics`                                   | `open_file`              |
//! | `FileSealer::resume` (a file header)      | `s7_parsers::file_resume_never_panics`                                 | `file_resume`            |
//! | `HistoryRecord::from_bytes`               | `s4_parsers::history_record_parser_never_panics`                       | `history_record`         |
//! | `HistoryRecord::from_accepted`            | `s7_parsers::from_accepted_never_panics`                               | `from_accepted`          |
//! | `read_backup`                             | `s4_parsers::backup_readers_never_panic`                               | `read_backup`            |
//! | `verify_backup_file`                      | `s4_parsers::backup_readers_never_panic`                               | `verify_backup_file`     |
//! | `open_blob`, its header, AEAD and padding | `s5_parsers::openers_never_panic_on_sealed_bytes` (header, AEAD); `s9_parsers::open_blob_never_panics_on_padded_bodies` (padding) | `open_blob` (mutated sealed bytes; padded bodies sealed in the target) |
//! | `open_snapshot`, every step of §20.5      | `s5_parsers::openers_never_panic_on_sealed_bytes`, `..._on_bodies`     | `open_snapshot` (mutated sealed bytes; streams, bodies and body fields sealed in the target, the expected hash the body's own) |
//! | `open_snapshot_declared`, every step      | `s5_parsers::openers_never_panic_on_sealed_bytes`, `..._on_bodies`     | `open_snapshot_declared` (as `open_snapshot`, sealed under the declared root's key) |
//! | `pair::parse_code`                        | `s6_pairing::parse_code_never_panics`                                  | `parse_code`             |
//! | N's machine (`New::on`), any events       | `s6_pairing::the_machines_never_panic_on_random_events`, `mutated_slots_never_panic`, `a_mutated_join_never_panics` | `pair_new` (a whole honest run's events, mutated, out of place, ticks, any event of real size) |
//! | E's machine (`Existing::on`), any events  | `s6_pairing::the_machines_never_panic_on_random_events`, `mutated_slots_never_panic` | `pair_existing` |
//! | private: canonical JSON (`canon::parse`)  | `src/canon/tests.rs::parse_never_panics`                               | `payloads`               |
//! | private: the payload and item parsers     | `src/payloads/tests.rs::payload_parsers_never_panic`                   | `payloads`               |
//! | private: envelope header, unpadding       | `src/entries/tests.rs::receive_side_never_panics`                      | `payloads`, `classify`   |
//! | private: inflate (`prim::inflate_raw`)    | `src/prim/tests.rs::inflate_never_panics`; `s5_parsers::openers_never_panic_on_bodies`; `s4_parsers::backup_payload_parsers_never_panic` | `snapshot_body`, `backup_payload`, `open_snapshot`, `open_snapshot_declared` |
//! | private: the snapshot body parser, the projection and the agreement check | `s5_parsers::openers_never_panic_on_bodies`  | `snapshot_body`, `open_snapshot`, `open_snapshot_declared` |
//! | private: the backup payload parsers       | `s4_parsers::backup_payload_parsers_never_panic`                       | `backup_payload`         |
//! | private: the bundle JSON of slot D        | `s6_pairing::mutated_bundles_never_panic`                              | `pair_bundle`, `pair_new` (the JSON sealed again under the bundle key) |
//! | private: the slot bodies A to D           | `s6_pairing::mutated_slots_never_panic` (re-MACed)                     | `pair_new`, `pair_existing` |
//!
//! Builders that take caller values but parse nothing from outside
//! (`seal_frame`, the payload builders, `login_message`, `admin_message`,
//! `padded_len`) are covered by `s2_frames` as well.

#[path = "../common/mod.rs"]
mod common;
#[path = "../common/board.rs"]
mod board;
#[path = "../common/backup.rs"]
mod backup;
#[path = "../common/blob.rs"]
mod blob;
#[path = "../common/pairing.rs"]
mod pairing;

mod s1_parsers;
mod s2_frames;
mod s3_membership;
mod s4_parsers;
mod s5_parsers;
mod s6_pairing;
mod s7_parsers;
mod s9_parsers;
