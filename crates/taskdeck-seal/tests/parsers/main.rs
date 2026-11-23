//! Property tests: no parser panics on random or mutated input (SEAL-GEN-011).
//! Each task adds its module here.

#[path = "../common/mod.rs"]
mod common;
#[path = "../common/board.rs"]
mod board;
#[path = "../common/backup.rs"]
mod backup;

mod s1_parsers;
mod s2_frames;
mod s3_membership;
mod s4_parsers;
