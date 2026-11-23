//! What the fuzz targets share: the fixture board they start from, and a
//! reader of the control bytes at the front of an input.
#![forbid(unsafe_code)]

use taskdeck_core::board::BoardData;
use taskdeck_core::legacy::{LegacyConfig, LegacyFolder, import};
use taskdeck_core::time::Instant;

/// `fixtures/boards/small`, imported (core.md §15): live tasks, events and
/// routines with blocks, archive rows, subscriptions and notes. Its config is
/// what its `userconfig.toml` holds (as `tests/common/boards.rs` writes it).
pub fn fixture_board() -> BoardData {
    let folder = LegacyFolder {
        read_at_startup: Some(include_bytes!("../../../../fixtures/boards/small/read_at_startup.json").to_vec()),
        archived: Some(include_bytes!("../../../../fixtures/boards/small/archived.jsonl").to_vec()),
        notepad: Some(include_bytes!("../../../../fixtures/boards/small/notepad_text.json").to_vec()),
        subscriptions: Some(include_bytes!("../../../../fixtures/boards/small/subscriptions.json").to_vec()),
        colorschemes: Some(include_bytes!("../../../../fixtures/boards/small/colorschemes.json").to_vec()),
        config: LegacyConfig { coordinates: Some([-41.25, -132.5]), selected_colorscheme_id: Some(100) },
    };
    import(&folder).0.expect("the small fixture imports")
}

/// The fixtures' `now`, 2026-10-20T08:15Z, moved by `minutes`.
pub fn now_plus(minutes: i64) -> Instant {
    let now = Instant::parse_canonical("2026-10-20T08:15:00.000000000Z").expect("an instant");
    now.plus_seconds(minutes.saturating_mul(60)).unwrap_or(now)
}

/// Reads control bytes off the front of an input, zero when it runs out.
pub struct Control<'a>(pub &'a [u8]);

impl<'a> Control<'a> {
    pub fn byte(&mut self) -> u8 {
        match self.0.split_first() {
            Some((b, rest)) => {
                self.0 = rest;
                *b
            }
            None => 0,
        }
    }

    pub fn u16(&mut self) -> u16 {
        u16::from_le_bytes([self.byte(), self.byte()])
    }

    /// What is left of the input.
    pub fn rest(self) -> &'a [u8] {
        self.0
    }
}
