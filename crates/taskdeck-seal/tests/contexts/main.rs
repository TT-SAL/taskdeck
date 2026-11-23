//! Nothing made for one use verifies or opens for another. Each task adds
//! its module here.
//!
//! The matrix: for each primitive, the uses of seal.md §4.1 that share it,
//! and the module that proves each pair (an object made for one use does
//! not verify or open as the other, with the same key wherever the API lets
//! a caller reuse one). `s1_contexts::the_registry_consts` checks every
//! string of the registry, and that none is a prefix of another.
//!
//! | Primitive        | Pair of uses                                              | Module                                        |
//! |------------------|-----------------------------------------------------------|-----------------------------------------------|
//! | signature        | `frame`, `login`, `admin`, `ctl`: each of the 6 pairs     | `s2_signatures` (device and recovery signer)  |
//! | signature        | `backup` against `frame`, `login`, `admin`, `ctl`         | `s4_contexts::a_backup_signature_is_no_other` |
//! | AEAD AD          | `entry` and `file`                                        | `s4_contexts::a_file_record_is_not_an_entry`  |
//! | AEAD AD          | `blob` and `entry`, `blob` and `file`                     | `s5_contexts` (same key)                      |
//! | AEAD AD          | blob kind 1 (snapshot) and kind 2                         | `s5_contexts::a_snapshot_is_not_a_blob_of_kind_2` |
//! | AEAD AD          | a snapshot for one `S`, writer, board, epoch and another  | `s5_contexts`                                 |
//! | AEAD AD          | `pair-bundle` and `entry`, `blob`, `file`, both ways      | `s6_contexts` (the bundle key)                |
//! | AEAD AD          | each file kind and each other (12 × 11), one `L_d`        | `s7_contexts::a_file_of_one_kind_is_no_other_kind` |
//! | HPKE info        | `wrap` opened as `backup-hpke`                            | `s1_contexts::a_wrap_is_not_a_backup_payload` |
//! | HPKE info        | `backup-hpke` opened as `wrap`                            | `s4_contexts::a_backup_is_not_a_wrap`         |
//! | HKDF info        | `key-entry`, `key-snap`, `key-blob` (frame, blob, snapshot) | `s7_contexts::the_epoch_keys_are_three_uses` |
//! | HKDF info        | `pair-mac` and `pair-ticket` (slot MAC under either key)  | `s7_contexts::a_slot_mac_is_bound_to_its_key_context_label_and_role` |
//! | HMAC input       | `pair` and `door`, `token`, the pairing hash contexts      | `s7_contexts` (same key)                      |
//! | HMAC input       | slot labels A to D and roles E and N                      | `s7_contexts`, and `tamper/s6_slots`          |
//!
//! Not paired here, with the reason: `local-key` (its use is `device.md`'s),
//! `door` and `token` beyond the slot MAC (their layouts are `relay.md`'s;
//! the crate computes neither), and the hash contexts, which no opener or
//! verifier of the crate takes from a caller (each hash is recomputed from
//! its own context; their strings are checked by `the_registry_consts` and
//! the published vectors).

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

mod s1_contexts;
mod s2_signatures;
mod s4_contexts;
mod s5_contexts;
mod s6_contexts;
mod s7_contexts;
