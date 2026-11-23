//! Bytes through `BoardData::from_canonical_bytes` (core.md §6.14, §18.6):
//! never a panic (CORE-GEN-007), and every accepted input encodes back to
//! itself (CORE-DATA-005).
#![no_main]

use libfuzzer_sys::fuzz_target;
use taskdeck_core::board::BoardData;

fuzz_target!(|data: &[u8]| {
    if let Ok(board) = BoardData::from_canonical_bytes(data) {
        assert_eq!(board.to_canonical_bytes(), data, "loading changed the bytes");
    }
});
