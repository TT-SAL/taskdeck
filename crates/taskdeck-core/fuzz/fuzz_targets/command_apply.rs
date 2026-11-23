//! Bytes through `parse_command`, applied to the fixture board (core.md
//! §18.6): neither panics (CORE-GEN-007), a refusal changes nothing
//! (CORE-APPLY-002), and the board passes `check` afterwards. Two control
//! bytes pick the context.
#![no_main]

use libfuzzer_sys::fuzz_target;
use taskdeck_core::apply::{Ctx, apply};
use taskdeck_core::command::parse_command;
use taskdeck_core_fuzz::{Control, fixture_board, now_plus};

fuzz_target!(|data: &[u8]| {
    let mut control = Control(data);
    let device_no = u16::from(control.byte() % 4);
    let minutes = i64::from(control.byte());
    let Ok(command) = parse_command(control.rest()) else { return };
    let mut board = fixture_board();
    let before = board.to_canonical_bytes();
    let ctx = Ctx { seq: 2, device_no, at: now_plus(minutes), based_on: 1 };
    match apply(&mut board, &command, &ctx) {
        Ok(_) => assert!(board.check().is_ok(), "apply left a board that fails check"),
        Err(_) => assert_eq!(board.to_canonical_bytes(), before, "a refusal changed the board"),
    }
});
