//! Legacy import (core.md §15, §20).

use crate::board::BoardData;

/// Today's files, read by the caller (core.md §15.2).
pub struct LegacyFolder {
    pub read_at_startup: Option<Vec<u8>>,
    pub archived: Option<Vec<u8>>,
    pub notepad: Option<Vec<u8>>,
    pub subscriptions: Option<Vec<u8>>,
    pub colorschemes: Option<Vec<u8>>,
    pub config: LegacyConfig,
}

/// What the caller read from `userconfig.toml` (core.md §15.2).
pub struct LegacyConfig {
    pub coordinates: Option<[f64; 2]>,
    pub selected_colorscheme_id: Option<u32>,
}

/// The import report: counts, ids and kinds only (core.md §15.4).
pub struct ImportReport {
    pub items_task: u32,
    pub items_event: u32,
    pub items_routine: u32,
    pub blocks: u32,
    pub archive_rows: u32,
    pub unreadable_lines: u32,
    pub subscriptions: u32,
    pub notes_scalars: u32,
    pub palette_set: bool,
    pub home_set: bool,
    pub renumbered: Vec<Renumbered>,
    pub live_and_archived: Vec<u64>,
    pub warnings: Vec<ImportWarning>,
    pub failure: Option<ImportFailure>,
}

/// One renumbered id (core.md §15.3 step 9, §15.4).
pub struct Renumbered {
    pub list: IdList,
    pub old: u64,
    pub new: u64,
}

/// The list a renumbered id was in (core.md §15.4).
pub enum IdList {
    Items,
    Archive,
    Subscriptions,
}

/// A warning of the import (core.md §15.4).
pub enum ImportWarning {
    PaletteUnreadable,
    HomeOutOfRange,
    NotesOverCap,
}

/// The step an import failed at (core.md §15.3, §15.4).
pub enum FailureKind {
    TooLarge,
    ItemsUnreadable,
    NotesUnreadable,
    SubscriptionsUnreadable,
    IdRange,
    InstantRange,
    FieldRange,
    Cap,
    Invariant,
}

/// A failed import and the ids it names (core.md §15.4).
pub struct ImportFailure {
    pub kind: FailureKind,
    pub ids: Vec<u64>,
}

/// The genesis board and the report (core.md §15.3).
pub fn import(folder: &LegacyFolder) -> (Option<BoardData>, ImportReport) {
    unimplemented!()
}
