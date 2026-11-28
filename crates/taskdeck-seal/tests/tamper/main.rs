//! Flipping any byte of any frame, wrap, bundle or file is rejected. Each
//! task adds its module here.
//!
//! "Rejected": the opener or verifier returns its error (or, for a frame,
//! a class other than `Accept`), the state does not change beyond what the
//! class allows (§13.5: a refusal sets the author's counter), and nothing of
//! the plaintext is returned.
//!
//! The matrix: every sealed, signed or authenticated object of seal.md, and
//! the module that flips every byte of it.
//!
//! | Object (seal.md §)                                   | Opener or verifier                    | Module                          |
//! |------------------------------------------------------|---------------------------------------|---------------------------------|
//! | frame `cmd` (§8, §9)                                 | `classify`                            | `s3_receipt`                    |
//! | frame `beat` (§11.4)                                 | `classify`                            | `s3_receipt`                    |
//! | frame `ctl`: revocation and rotation, by a device    | `classify`                            | `s3_receipt`                    |
//! | frame `ctl`: `Fence`, by the recovery identity       | `classify`                            | `s3_receipt`                    |
//! | frame `ctl`: `Genesis` at seq 1 (§13.3)              | `classify` with the anchor            | `s3_receipt`                    |
//! | frame `snap` (§11.5)                                 | `classify`                            | `s7_frames`                     |
//! | inner `ctl` signature and payload (§11.3), resealed  | `classify`                            | `s7_frames`                     |
//! | wrap to a device (§7)                                | `unwrap_root`                         | `s1_wraps`                      |
//! | wrap to the recovery identity (§7)                   | `unwrap_root`                         | `s1_wraps`                      |
//! | login signature and message (§16.1)                  | `verify_strict` (the relay's check)   | `s7_signatures`                 |
//! | admin signature, message and `args` (§16.2)          | `verify_strict` (the relay's check)   | `s7_signatures`                 |
//! | pairing slot A (§17.4, §17.5)                        | N's machine                           | `s6_slots`                      |
//! | pairing slot B                                       | E's machine                           | `s6_slots`                      |
//! | pairing slot C                                       | N's machine                           | `s6_slots`                      |
//! | pairing slot D and the bundle in it (§17.8)          | N's machine (also re-MACed)           | `s6_slots`                      |
//! | sealed file: header and records (§19)                | `open_file`                           | `s4_files`                      |
//! | snapshot, blob kind 1 (§20)                          | `open_snapshot`, `open_snapshot_declared` | `s5_blobs`                  |
//! | blob kind 2 (§20)                                    | `open_blob`                           | `s5_blobs`                      |
//! | nightly backup (§21)                                 | `read_backup`, `verify_backup_file`   | `s4_backups`                    |
//! | segment (§21)                                        | `read_backup`, `verify_backup_file`   | `s4_backups`                    |
//! | typed recovery sheet and its check (§18)             | `RecoverySeed::from_sheet`            | `s7_typed`                      |
//! | typed pairing code and its check (§17.2)             | `parse_code`, N's machine             | `s7_typed`                      |
//! | typed move code (§15.4)                              | `check_move_code`                     | `s7_typed`                      |
//!
//! Not flipped here: the state hash, the sync fingerprint and the key
//! fingerprints, which nothing opens or verifies inside the crate.

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

mod s1_wraps;
mod s3_receipt;
mod s4_backups;
mod s4_files;
mod s5_blobs;
mod s6_slots;
mod s7_frames;
mod s7_signatures;
mod s7_typed;
