//! The projection of seal.md §21.6: the reduced membership a reader derives
//! from history records alone (SEAL-BKP-008), with no epoch root and no
//! entry other than the accepted control entries.

use subtle::ConstantTimeEq;

use super::HistoryRecord;
use crate::entries::Genesis;
use crate::membership::transitions::{self, Auth, EntryCx};
use crate::membership::{CTL_SEQS_MAX, Membership};
use crate::payloads::{Shape, ctl_inner, ctl_shape, parse_items_bytes, wrap_recipient};
use crate::signatures::verify_strict;
use crate::{BoardId, CtlItem, DevId, SealError, board_id};

/// The projection of `history` for the board `board` (seal.md §21.6
/// "Projection", SEAL-BKP-008): starting empty, for each record in order,
///
/// 1. its `seq` is above the previous record's, and the first is seq 1;
/// 2. its `dev` is an authority of the projection so far (a current device,
///    or the current recovery identity in force before the record's seq),
///    and for the first record the `dev_R` of its Genesis item's recovery
///    key;
/// 3. its `ctr` is above the `ctr` of the previous record by the same `dev`
///    (gaps are normal), and the first record's `ctr` is 1 (SEAL-MEM-004,
///    ruling Q46: the projection accepts no history a log cannot produce);
/// 4. its `sig` verifies strictly under that author's `sign_pk` over the
///    inner message of §11.3 with `board`, the record's `dev`, `ctr`,
///    `entry_id`, `at_ms` and `H(items)` (SEAL-KIND-004, SEAL-GEN-010);
/// 5. its items parse (§12, the canonical encoding of §5.3) and form a
///    shape (§12.1), `G` exactly at the first record;
/// 6. the items change the projection by the reduced rules (below).
///
/// Every failure is `seal/backup_history`, and the first failing record
/// ends the projection. So each record's items are applied to the
/// projection itself, with no copy to undo them (a copy per record would
/// make the time grow with the square of the history): nothing of a failed
/// projection is returned. The cost of each record is bounded by the limits
/// of §23 on what it reads (the devices, the recovery records, its items),
/// never by the number of records before it; `ctl_seqs` and the
/// commitments only grow at their end.
///
/// `board` is the board id the reader trusts: the sheet's for a backup
/// (§21.6 step 9), and, for task S5's `open_snapshot` (§20.5), the
/// receiver's own board id. The Genesis record must give exactly that
/// `board_id` from its recovery key and salt (§6.2).
///
/// **The reduced rules.** Genesis (first record only) sets the board, checks
/// `board_id` (§6.2), that the first device is not `dev_R` and that its two
/// wraps name exactly the device and `dev_R`, and adds the device and the
/// recovery key, as §13.3. `DeviceAdded`, `DeviceRevoked`,
/// `DeviceReinstated` and `EpochRotated` run their rows of §13.6
/// (`membership::transitions`) unchanged: every precondition they check
/// (fresh keys and ids, the device caps, `bootstrap_seq` against
/// `purge_seq` and the record's seq, the authors, the targets' statuses, the
/// last current device, the next epoch number, the wraps against the
/// current devices and recovery identity, `purge_before`) is decided by
/// control entries alone. `RecoveryKeyChange` runs its row without the
/// 72-hour check. `Fence` and `Skip` (author D or R: every authority) and
/// `ResetTo` and `RulesUpgrade` (author D) check only their Author column
/// and change nothing.
///
/// **What the returned `Membership` holds.** Exactly as the log would: the
/// board id and salt, the devices with their keys, numbers, statuses and
/// `added`/`status` fields, `next_device_no`, the recovery records, the
/// pending recovery-key proposal, the epochs and commitments, `purge_seq`,
/// and `ctl_seqs` (the records' seqs). What a history cannot give is filled
/// from it alone: each author's `last_ctr` is the `ctr` of its last record
/// (0 for an author with none), the log clock `clock_ms` is the largest
/// record `at_ms` (and `epoch_clock_ms` that clock at the rotation), a
/// device's `beat_ms` is the `at_ms` of the record that added or reinstated
/// it and its beat lists those §13.6 gives a new device; `rules` stays 1,
/// the home stays the Genesis one (`home_gen` 1), and there is no pending
/// override, reset or skip. Step 11 of §21.6 compares only the devices'
/// keys and statuses, the current recovery key, the epochs and commitments
/// and `ctl_seqs`.
// SPEC-QUESTION: §21.6 step 2 asks for the Genesis item's recovery key for
// the first record, before step 4 has verified that record's signature and
// step 5 parsed its items. For the first record only, this parses the items
// at step 2 to find the key (an unparsable first record fails there); the
// key is then bound to the trusted `board` at step 6 (§6.2).
// SPEC-QUESTION: §21.6 step 6 lists for DeviceAdded only "adds a current
// device with fresh keys", for revocations "the authors of §13.6", and for
// EpochRotated "requires the next epoch number and records commit". The
// other preconditions of their rows are as decidable from the history as
// those (they read only control-entry state), and a log that accepted the
// entry passed them; this checks them too (the refusing reading).
pub(crate) fn project(history: &[HistoryRecord], board: &BoardId) -> Result<Membership, SealError> {
    let bad = SealError::BackupHistory;
    let (first, rest) = history.split_first().ok_or(bad)?;
    let mut m = genesis(first, board).ok_or(bad)?;
    let mut prev_seq = first.seq;
    for rec in rest {
        // 1.
        if rec.seq <= prev_seq {
            return Err(bad);
        }
        // 2.
        let (auth, sign_pk, last_ctr) = authority(&m, &rec.dev, rec.seq).ok_or(bad)?;
        // 3.
        if rec.ctr <= last_ctr {
            return Err(bad);
        }
        // 4.
        let inner = ctl_inner(board, &rec.dev, rec.ctr, &rec.entry_id, rec.at_ms, &rec.items);
        verify_strict(&sign_pk, &inner, &rec.sig).map_err(|_| bad)?;
        // 5.
        let items = parse_items_bytes(&rec.items).map_err(|_| bad)?;
        let shape = ctl_shape(&items).map_err(|_| bad)?;
        if shape == Shape::G {
            return Err(bad);
        }
        // 6.
        let tau = m.clock_ms.max(rec.at_ms);
        let none = |_: u64| -> Option<&[u8]> { None };
        let cx = EntryCx { s: rec.seq, at_ms: rec.at_ms, tau, author: auth, board: *board, skip_targets: &none };
        for (i, item) in items.iter().enumerate() {
            let later = items.get(i + 1..).unwrap_or(&[]);
            reduced(&mut m, item, later, &cx).map_err(|_| bad)?;
        }
        if m.ctl_seqs.len() >= CTL_SEQS_MAX {
            return Err(bad);
        }
        m.set_last_ctr(&rec.dev, rec.ctr);
        m.clock_ms = tau;
        m.ctl_seqs.push(rec.seq);
        prev_seq = rec.seq;
    }
    Ok(m)
}

/// Steps 1 to 6 for the first record: seq 1, `dev = dev_R` of the Genesis
/// recovery key, `ctr` 1, the signature, the shape `G`, and Genesis (§13.3).
fn genesis(rec: &HistoryRecord, board: &BoardId) -> Option<Membership> {
    // 1.
    if rec.seq != 1 {
        return None;
    }
    // 2. (the items are read here for the key; see `project`).
    let items = parse_items_bytes(&rec.items).ok()?;
    let [CtlItem::Genesis(g)] = items.as_slice() else { return None };
    let dev_r = g.recovery.dev(board);
    if rec.dev != dev_r {
        return None;
    }
    // 3. No earlier record; the counter of the entry at seq 1 is 1
    // (SEAL-MEM-004).
    if rec.ctr != 1 {
        return None;
    }
    // 4.
    let inner = ctl_inner(board, &rec.dev, rec.ctr, &rec.entry_id, rec.at_ms, &rec.items);
    verify_strict(&g.recovery.sign_pk, &inner, &rec.sig).ok()?;
    // 5.
    if ctl_shape(&items).ok()? != Shape::G {
        return None;
    }
    // 6.
    genesis_ok(g, board, &dev_r).then_some(())?;
    let mut m = Membership::genesis(*board, g, dev_r, rec.at_ms);
    m.set_last_ctr(&dev_r, rec.ctr);
    Some(m)
}

/// The Genesis checks a history can repeat (§13.3, SEAL-MEM-005 without the
/// commitment, which needs `R_1`): `board_id` from the recovery key and the
/// salt (§6.2), a first device other than `dev_R`, and wraps naming exactly
/// the two.
fn genesis_ok(g: &Genesis, board: &BoardId, dev_r: &DevId) -> bool {
    let mut want = [g.device.dev.0, dev_r.0];
    want.sort_unstable();
    let got: Vec<[u8; 16]> = g.wraps.iter().map(wrap_recipient).collect();
    bool::from(board_id(&g.recovery.sign_pk, &g.salt).0.ct_eq(&board.0)) && g.device.dev != *dev_r && got == want
}

/// Step 2 for a later record: the author's class, `sign_pk` and last
/// counter, when it is a current device or the current recovery identity in
/// force before `s` (§13.4).
fn authority(m: &Membership, dev: &DevId, s: u64) -> Option<(Auth, [u8; 32], u64)> {
    if let Some(d) = m.device(dev) {
        return d.is_current().then_some((Auth::Device(d.dev), d.sign_pk, d.last_ctr));
    }
    let r = m.recovery_record(dev)?;
    (r.until_seq == 0 && r.since_seq < s).then_some((Auth::Recovery(r.dev), r.sign_pk, r.last_ctr))
}

/// Step 6 for one item (see `project` for the reduced rules).
fn reduced(w: &mut Membership, item: &CtlItem, later: &[CtlItem], cx: &EntryCx<'_>) -> Result<(), SealError> {
    match item {
        CtlItem::Genesis(_) => Err(SealError::CtlShape),
        CtlItem::DeviceAdded(a) => transitions::device_added(w, a, cx),
        CtlItem::DeviceRevoked(t) => transitions::revoke(w, t, later, cx),
        CtlItem::DeviceReinstated(t) => transitions::reinstate(w, t, cx),
        CtlItem::EpochRotated(r) => transitions::rotate(w, r, cx),
        CtlItem::RecoveryKeyChange(k) => transitions::recovery_key_change(w, k, cx, false),
        // Author D or R: every authority of step 2.
        CtlItem::Fence(_) | CtlItem::Skip(_) => Ok(()),
        // Author D.
        CtlItem::ResetTo(_) | CtlItem::RulesUpgrade(_) => match cx.author {
            Auth::Device(_) => Ok(()),
            Auth::Recovery(_) => Err(SealError::CtlAuthor),
        },
    }
}

#[cfg(test)]
mod tests;
