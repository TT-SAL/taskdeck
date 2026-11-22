//! Ids (core.md §7, §20).

use crate::CoreError;
use crate::board::BoardData;
use crate::command::LogCommand;

/// The bits of an id below the device number (core.md §7.1).
pub const LOW_BITS: u32 = 40;

/// An id from a device number and a counter (core.md §7.1).
pub fn compose(device_no: u16, n: u64) -> Option<u64> {
    unimplemented!()
}

/// An id's device number and counter (core.md §7.1).
pub fn split(id: u64) -> (u16, u64) {
    unimplemented!()
}

/// A device's next item and block ids (core.md §7.1 to §7.3).
pub struct IdMinter {
    pub device_no: u16,
    pub next_item: u64,
    pub next_block: u64,
}

impl IdMinter {
    /// A minter for a device (core.md §7.1).
    pub fn new(device_no: u16) -> Self {
        unimplemented!()
    }

    /// Mints an item id against a view (core.md §7.1, §7.2).
    pub fn mint_item(&mut self, view: &BoardData) -> Option<u64> {
        unimplemented!()
    }

    /// Mints a block id against a view (core.md §7.3).
    pub fn mint_block(&mut self, view: &BoardData) -> Option<u64> {
        unimplemented!()
    }
}

/// Rebinds another device's commands to fresh ids (core.md §7.5).
pub fn rebind(
    commands: &[LogCommand],
    from_device_no: u16,
    view: &BoardData,
    minter: &mut IdMinter,
) -> Result<Vec<LogCommand>, CoreError> {
    unimplemented!()
}
