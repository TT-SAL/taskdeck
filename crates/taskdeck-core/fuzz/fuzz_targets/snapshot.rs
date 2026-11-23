//! `snapshot` of a board read from the input's bytes, or of the fixture board
//! when they are not canonical board bytes (core.md §14.3, §18.6): never a
//! panic (CORE-GEN-007), and the board unchanged (CORE-READ-002). Four control
//! bytes pick the first day, the number of days (clamped to 1 to 31 by
//! `snapshot` itself), `now` and the zone's offset.
#![no_main]

use chrono::NaiveDate;
use libfuzzer_sys::fuzz_target;
use taskdeck_core::board::BoardData;
use taskdeck_core::overlay::Overlay;
use taskdeck_core::snapshot::{Dials, SnapshotInput, snapshot};
use taskdeck_core::time::FixedZone;
use taskdeck_core_fuzz::{Control, fixture_board, now_plus};

fuzz_target!(|data: &[u8]| {
    let mut control = Control(data);
    let first = NaiveDate::from_ymd_opt(2026, 9, 1).expect("a date");
    let from = first + chrono::Days::new(u64::from(control.byte()));
    let days = u32::from(control.byte() % 64);
    let now = now_plus(i64::from(control.u16()) - 30_000);
    let zone = FixedZone((i32::from(control.byte()) - 128) * 900);
    let board = BoardData::from_canonical_bytes(control.rest()).unwrap_or_else(|_| fixture_board());
    let before = board.to_canonical_bytes();
    let overlay = Overlay::default();
    let input = SnapshotInput {
        board: &board,
        overlay: &overlay,
        zone: &zone,
        now,
        from,
        days,
        version: 0,
        ranked: None,
        look: None,
        dials: Dials { blur: 11, light: 39 },
    };
    let _ = snapshot(&input);
    assert_eq!(board.to_canonical_bytes(), before, "snapshot changed the board");
});
