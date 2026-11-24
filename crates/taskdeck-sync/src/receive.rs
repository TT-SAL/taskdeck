//! Read answers and pushes (engine.md §12.1).

use crate::Head;

/// One entry of a page (engine.md §12.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageEntry {
    pub seq: u64,
    pub at: i64,
    pub frame: Vec<u8>,
}

/// A read answer or a push (engine.md §12.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub head: Head,
    pub relay_now: i64,
    pub floor: u64,
    pub purge_floor: u64,
    pub more: bool,
    pub truncated: bool,
    pub fenced: bool,
    pub entries: Vec<PageEntry>,
}
