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

/// The 25 `White_Space` code points of core.md §2, by an explicit list: not
/// `str::trim`, whose table follows the toolchain's Unicode version
/// (CORE-COMP-002).
const WHITE_SPACE: [char; 25] = [
    '\u{0009}', '\u{000A}', '\u{000B}', '\u{000C}', '\u{000D}', '\u{0020}', '\u{0085}', '\u{00A0}',
    '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}',
    '\u{2007}', '\u{2008}', '\u{2009}', '\u{200A}', '\u{2028}', '\u{2029}', '\u{202F}', '\u{205F}',
    '\u{3000}',
];

/// Trims Unicode `White_Space` only (core.md §2, §13.3): every leading and
/// trailing code point of `WHITE_SPACE`, and nothing else (CORE-COMP-002).
pub fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| WHITE_SPACE.contains(&c))
}

/// Replaces every U+0009 by four U+0020 (core.md §2, §13.3), as today's
/// `utilities::detab` does.
pub fn detab(text: &str) -> String {
    text.replace('\t', "    ")
}

#[cfg(test)]
mod tests;
