//! The compile step (core.md §13, §20).

use crate::CompileError;
use crate::board::BoardData;
use crate::command::LogCommand;
use crate::ids::IdMinter;
use crate::intent::Intent;
use crate::overlay::Overlay;
use crate::time::{Instant, Zone};

/// The origin's inputs to the compile step (core.md §13.1).
pub struct CompileCtx<'a, Z: Zone + ?Sized> {
    pub view: &'a BoardData,
    pub zone: &'a Z,
    pub now: Instant,
    pub device_no: u16,
    pub overlay: &'a Overlay,
}

/// The compiled command, if any, and the reply (core.md §13.1).
pub struct Compiled {
    pub command: Option<LogCommand>,
    pub reply: Reply,
}

/// Today's reply, `session` a sid (core.md §13.1).
pub struct Reply {
    pub id: Option<u64>,
    pub session: Option<u64>,
    pub moved: Option<u32>,
}

/// Compiles an intent into a log command (core.md §13.3, §13.4).
pub fn compile<Z: Zone + ?Sized>(
    intent: &Intent,
    cx: &CompileCtx<Z>,
    minter: &mut IdMinter,
) -> Result<Compiled, CompileError> {
    unimplemented!()
}

/// Today's subscription URL check (core.md §13.4).
pub fn checked_url(url: &str) -> Result<String, CompileError> {
    unimplemented!()
}

/// Trims Unicode `White_Space` only (core.md §13.3).
pub fn trim(text: &str) -> &str {
    unimplemented!()
}

/// Replaces tabs in text (core.md §13.3).
pub fn detab(text: &str) -> String {
    unimplemented!()
}
