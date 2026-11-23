//! Ids (core.md §7, §20).

use std::collections::BTreeMap;

use crate::CoreError;
use crate::board::BoardData;
use crate::command::LogCommand;

/// The bits of an id below the device number (core.md §7.1).
pub const LOW_BITS: u32 = 40;

/// An id from a device number and a counter (core.md §7.1).
///
/// `device_no` from 0 (the legacy space) to 8191 and `n` from 1 to
/// 2^40 − 1, so the id is below 2^53 (CORE-ID-001); none otherwise.
pub fn compose(device_no: u16, n: u64) -> Option<u64> {
    if device_no <= MAX_DEVICE_NO && (1..=MAX_LOW).contains(&n) {
        Some((u64::from(device_no) << LOW_BITS) | n)
    } else {
        None
    }
}

/// An id's device number and counter (core.md §7.1).
///
/// The high part saturates at `u16::MAX` for an id at or above 2^56, which is
/// no id of §7.1 (its high part would exceed 8191 in any case).
pub fn split(id: u64) -> (u16, u64) {
    (u16::try_from(id >> LOW_BITS).unwrap_or(u16::MAX), id & MAX_LOW)
}

/// The largest low part: 2^40 − 1 (core.md §7.1).
const MAX_LOW: u64 = (1 << LOW_BITS) - 1;
/// The largest `device_no` (core.md §7.1).
const MAX_DEVICE_NO: u16 = 8191;

/// A device's next item and block ids (core.md §7.1 to §7.3).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct IdMinter {
    pub device_no: u16,
    pub next_item: u64,
    pub next_block: u64,
}

impl IdMinter {
    /// A minter for a device (core.md §7.1).
    pub fn new(device_no: u16) -> Self {
        IdMinter { device_no, next_item: 1, next_block: 1 }
    }

    /// Mints an item id against a view (core.md §7.1, §7.2).
    ///
    /// The number is `max(next_item, mark(view, device_no).item + 1)` and
    /// `next_item` becomes that number plus one (CORE-ID-004). None, with the
    /// minter unchanged, when the number space of the device is used up or
    /// `device_no` is not one an author can have (1 to 8191).
    pub fn mint_item(&mut self, view: &BoardData) -> Option<u64> {
        let mark = view.mark(self.device_no).item;
        let (id, next) = mint(self.device_no, self.next_item, mark)?;
        self.next_item = next;
        Some(id)
    }

    /// Mints a block id against a view (core.md §7.3).
    ///
    /// As `mint_item`, with `next_block` and the block mark (core.md §7.3).
    pub fn mint_block(&mut self, view: &BoardData) -> Option<u64> {
        let mark = view.mark(self.device_no).block;
        let (id, next) = mint(self.device_no, self.next_block, mark)?;
        self.next_block = next;
        Some(id)
    }
}

/// The id `max(next, mark + 1)` of `device_no` and the minter's next number.
fn mint(device_no: u16, next: u64, mark: u64) -> Option<(u64, u64)> {
    // The legacy space (0) has no author (core.md §7.2).
    if device_no == 0 {
        return None;
    }
    let n = next.max(mark.saturating_add(1));
    let id = compose(device_no, n)?;
    Some((id, n + 1))
}

/// Rebinds another device's commands to fresh ids (core.md §7.5).
pub fn rebind(
    commands: &[LogCommand],
    from_device_no: u16,
    view: &BoardData,
    minter: &mut IdMinter,
) -> Result<Vec<LogCommand>, CoreError> {
    // The minter changes only when every command is rebound (§7.5).
    let mut m = *minter;
    let mut maps = Maps::default();
    let mut out = Vec::with_capacity(commands.len());
    for c in commands {
        let mut c = c.clone();
        // Step 1: the minting fields, in their order.
        mint_fields(&mut c, from_device_no, view, &mut m, &mut maps)?;
        // Step 2: the referring fields.
        refer_fields(&mut c, &maps);
        // Step 3: the arrays §9 orders by sid, sorted again.
        match &mut c {
            LogCommand::CreateTask(t) => t.blocks.sort_by_key(|b| b.sid),
            LogCommand::RemoveBlocks { blocks } => blocks.sort_by_key(|p| p.1),
            _ => {}
        }
        out.push(c);
    }
    *minter = m;
    Ok(out)
}

/// The item map (item and subscription ids) and the block map (sids) of
/// core.md §7.5.
#[derive(Default)]
struct Maps {
    items: BTreeMap<u64, u64>,
    blocks: BTreeMap<u64, u64>,
}

fn bad_id() -> CoreError {
    CoreError::code("core/bad_id")
}

/// Replaces one minting field holding a value of `from` by a new number.
fn remint(
    v: &mut u64,
    from: u16,
    map: &mut BTreeMap<u64, u64>,
    mint: impl FnOnce() -> Option<u64>,
) -> Result<(), CoreError> {
    if split(*v).0 == from {
        let new = mint().ok_or_else(bad_id)?;
        map.insert(*v, new);
        *v = new;
    }
    Ok(())
}

fn mint_fields(c: &mut LogCommand, from: u16, view: &BoardData, m: &mut IdMinter, maps: &mut Maps) -> Result<(), CoreError> {
    match c {
        LogCommand::CreateTask(t) => {
            remint(&mut t.id, from, &mut maps.items, || m.mint_item(view))?;
            for b in &mut t.blocks {
                remint(&mut b.sid, from, &mut maps.blocks, || m.mint_block(view))?;
            }
        }
        LogCommand::CreateEvent(e) => remint(&mut e.id, from, &mut maps.items, || m.mint_item(view))?,
        LogCommand::CreateRoutine(r) => remint(&mut r.id, from, &mut maps.items, || m.mint_item(view))?,
        LogCommand::AddBlock { sid, .. } => remint(sid, from, &mut maps.blocks, || m.mint_block(view))?,
        LogCommand::Restore { fallback, .. } => remint(fallback, from, &mut maps.items, || m.mint_item(view))?,
        LogCommand::AddSubscription { id, .. } => remint(id, from, &mut maps.items, || m.mint_item(view))?,
        _ => {}
    }
    Ok(())
}

fn refer(v: &mut u64, map: &BTreeMap<u64, u64>) {
    if let Some(&new) = map.get(v) {
        *v = new;
    }
}

fn refer_fields(c: &mut LogCommand, maps: &Maps) {
    match c {
        LogCommand::MoveBlock { id, sid, .. } => {
            refer(id, &maps.items);
            refer(sid, &maps.blocks);
        }
        LogCommand::MoveBlocks { moves } => {
            for mv in moves {
                refer(&mut mv.id, &maps.items);
                refer(&mut mv.sid, &maps.blocks);
            }
        }
        LogCommand::RemoveBlocks { blocks } => {
            for (id, sid) in blocks {
                refer(id, &maps.items);
                refer(sid, &maps.blocks);
            }
        }
        LogCommand::Rename { id, .. }
        | LogCommand::Complete { id, .. }
        | LogCommand::Delete { id, .. }
        | LogCommand::Forget { id }
        | LogCommand::AddBlock { id, .. }
        | LogCommand::SetEstimate { id, .. }
        | LogCommand::SetDeadline { id, .. }
        | LogCommand::SetSeverity { id, .. }
        | LogCommand::SetHorizon { id, .. }
        | LogCommand::SetRepeat { id, .. }
        | LogCommand::MoveRoutine { id, .. }
        | LogCommand::SetEventSpan { id, .. }
        | LogCommand::Restore { id, .. }
        | LogCommand::ForgetArchived { id, .. }
        | LogCommand::RemoveSubscription { id }
        | LogCommand::RenameSubscription { id, .. }
        | LogCommand::SetSubscriptionColor { id, .. }
        | LogCommand::SetSubscriptionEnabled { id, .. } => refer(id, &maps.items),
        LogCommand::CreateTask(_)
        | LogCommand::CreateEvent(_)
        | LogCommand::CreateRoutine(_)
        | LogCommand::SetNotes { .. }
        | LogCommand::AddSubscription { .. }
        | LogCommand::SetPalette { .. }
        | LogCommand::SetHome { .. }
        | LogCommand::SetPicture { .. } => {}
    }
}

#[cfg(test)]
mod tests;
