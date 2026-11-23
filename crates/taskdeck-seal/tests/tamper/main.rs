//! Flipping any byte of any frame, wrap, bundle or file is rejected. Each
//! task adds its module here.

#[path = "../common/mod.rs"]
mod common;
#[path = "../common/board.rs"]
mod board;
#[path = "../common/backup.rs"]
mod backup;

mod s1_wraps;
mod s3_receipt;
mod s4_backups;
mod s4_files;
