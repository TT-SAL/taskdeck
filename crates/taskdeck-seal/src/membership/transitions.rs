//! The transitions of seal.md §13.6 on a working copy `W` (SEAL-MEM-010 to
//! SEAL-MEM-027): one function per item, each returning the refusal code of
//! its row, and the two passes of an `S`-shape entry (§14.6).
//!
//! The caller (`receipt::classify`, step 16) has already checked the shape
//! (§12.1) and that the author is a current authority of `M_{s−1}`; it
//! passes the author's class as decided from `M_{s−1}` (§13.4), and keeps
//! `M_{s−1}` untouched: when an item fails, `W` is discarded.

use subtle::ConstantTimeEq;

use super::{
    COMMIT_MAX, CURRENT_MAX, DEVICE_NO_MAX, DEVICES_MAX, DeviceRecord, DeviceStatus, Membership, RECOVERY_MAX,
    RecoveryRecord, SKIPS_MAX,
};
use crate::entries::{
    CtlItem, DeviceAdded, EpochRotated, RecoveryKeyChange, ResetTo, RulesUpgrade, RulesUpgradeMode, SkipItem,
};
use crate::payloads::{Shape, wrap_recipient};
use crate::prim::take;
use crate::{BoardId, DevId, FrameHash, RecoveryPublic, SealError, frame_hash, frame_header};

/// The active-member window (seal.md §13.10, §23).
pub(crate) const ACTIVE_WINDOW_MS: i64 = 2_592_000_000;
/// The Skip window (seal.md §13.6, §23, ruling S15).
pub(crate) const SKIP_WINDOW: u64 = 16_384;

/// The author of a control entry, its class decided from `M_{s−1}` (§13.4):
/// "D" (a current device) or "R" (the current recovery identity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Auth {
    Device(DevId),
    Recovery(DevId),
}

impl Auth {
    pub(crate) fn dev(&self) -> DevId {
        match self {
            Auth::Device(d) | Auth::Recovery(d) => *d,
        }
    }
}

/// What §13.6 needs of the entry besides its items.
pub(crate) struct EntryCx<'a> {
    /// The entry's seq `s`.
    pub s: u64,
    /// The entry's own `at_ms`.
    pub at_ms: i64,
    /// `τ'`, the log clock after the entry (§13.5).
    pub tau: i64,
    pub author: Auth,
    pub board: BoardId,
    /// The frame at a Skip target, when the caller holds it (§14.6).
    pub skip_targets: &'a dyn Fn(u64) -> Option<&'a [u8]>,
}

/// The outcome of the items of one control entry.
pub(crate) enum CtlOutcome {
    /// Every item passed; `W` holds their effects. `activated` is the rules
    /// version a `RulesUpgrade` activated, for the rules halt (§14.5).
    Applied { activated: Option<u32> },
    /// The first item, in the order of §13.6 (§14.6 for `S`), that failed.
    Refused(SealError),
    /// The Skip targets not supplied (§14.6).
    NeedFrames(Vec<(u64, FrameHash)>),
}

/// The rules versions this build knows, and what each allows (§14.3).
pub(crate) fn envelope_allowed(rules: u32, ver: u8, suite: u8) -> bool {
    rules == 1 && (ver, suite) == (1, 1)
}

/// The `(ver, suite)` pairs a rules version allows (§14.3); none for a
/// version this build does not know.
// SPEC-QUESTION: §14.3 defines rules version 1 only; for a later version
// (activated by a RulesUpgrade this build may not support) this build
// knows no table, so it allows nothing: every frame under it is refused
// with `seal/bad_version`, and a DeviceAdded under it records no envelope.
pub(crate) fn envelopes_for(rules: u32) -> Vec<(u8, u8)> {
    if rules == 1 { vec![(1, 1)] } else { Vec::new() }
}

/// The state-hash versions a rules version allows (§14.3).
pub(crate) fn hash_version_allowed(rules: u32, hv: u32) -> bool {
    rules == 1 && hv == 1
}

/// Applies the items of a control entry of shape `shape` to `w` (§13.6).
pub(crate) fn apply(w: &mut Membership, shape: Shape, items: &[CtlItem], cx: &EntryCx<'_>) -> CtlOutcome {
    if shape == Shape::S {
        return skips(w, items, cx);
    }
    let mut activated = None;
    for (i, item) in items.iter().enumerate() {
        let rest = items.get(i + 1..).unwrap_or(&[]);
        let r = match item {
            // The shape check of step 16 lets a Genesis through only at seq 1,
            // which `classify` handles with the anchor (§13.3).
            CtlItem::Genesis(_) => Err(SealError::CtlShape),
            CtlItem::DeviceAdded(a) => device_added(w, a, cx),
            CtlItem::DeviceRevoked(t) => revoke(w, t, rest, cx),
            CtlItem::DeviceReinstated(t) => reinstate(w, t, cx),
            CtlItem::EpochRotated(r) => rotate(w, r, cx),
            CtlItem::RecoveryKeyChange(k) => recovery_key_change(w, k, cx),
            CtlItem::Fence(f) => {
                if Some(f.home_gen) != w.home_gen.checked_add(1) {
                    Err(SealError::CtlHomeGen)
                } else {
                    w.home_gen = f.home_gen;
                    w.home_seq = cx.s;
                    w.home_host = f.host.clone();
                    w.home_door = f.door;
                    Ok(())
                }
            }
            CtlItem::ResetTo(t) => reset_to(w, t, cx),
            // Skip items stand only in `S` (§12.1).
            CtlItem::Skip(_) => Err(SealError::CtlShape),
            CtlItem::RulesUpgrade(u) => rules_upgrade(w, u, cx).map(|v| {
                if v.is_some() {
                    activated = v;
                }
            }),
        };
        if let Err(e) = r {
            return CtlOutcome::Refused(e);
        }
    }
    CtlOutcome::Applied { activated }
}

fn recorded_dev(w: &Membership, dev: &DevId) -> bool {
    w.devices.iter().any(|d| d.dev == *dev) || w.recovery.iter().any(|r| r.dev == *dev)
}

fn recorded_sign_pk(w: &Membership, pk: &[u8; 32]) -> bool {
    w.devices.iter().any(|d| d.sign_pk == *pk) || w.recovery.iter().any(|r| r.sign_pk == *pk)
}

fn recorded_hpke_pk(w: &Membership, pk: &[u8; 32]) -> bool {
    w.devices.iter().any(|d| d.hpke_pk == *pk) || w.recovery.iter().any(|r| r.hpke_pk == *pk)
}

// "Every recorded" key or id is every device record and every recovery
// record, current or retired (§13.6, below the table).

/// `DeviceAdded` (§13.6, §13.7; SEAL-MEM-013, SEAL-MEM-014).
pub(crate) fn device_added(w: &mut Membership, a: &DeviceAdded, cx: &EntryCx<'_>) -> Result<(), SealError> {
    let Auth::Device(author) = cx.author else { return Err(SealError::CtlAuthor) };
    if recorded_dev(w, &a.dev) || recorded_sign_pk(w, &a.sign_pk) || recorded_hpke_pk(w, &a.hpke_pk) {
        return Err(SealError::CtlDuplicateKey);
    }
    if w.current_count() >= CURRENT_MAX || w.devices.len() >= DEVICES_MAX || w.next_device_no > DEVICE_NO_MAX {
        return Err(SealError::CtlDeviceCap);
    }
    if a.bootstrap_seq < w.purge_seq || a.bootstrap_seq >= cx.s {
        return Err(SealError::CtlTarget);
    }
    let rec = DeviceRecord {
        dev: a.dev,
        sign_pk: a.sign_pk,
        hpke_pk: a.hpke_pk,
        device_no: w.next_device_no,
        status: DeviceStatus::Current,
        added_seq: cx.s,
        added_by: author,
        bootstrap_seq: a.bootstrap_seq,
        status_seq: cx.s,
        status_by: author,
        last_ctr: 0,
        beat_ms: cx.at_ms,
        rules: vec![w.rules],
        envelopes: envelopes_for(w.rules),
        hash_versions: vec![1],
        name: a.name.clone(),
    };
    let at = w.devices.partition_point(|d| d.dev < a.dev);
    w.devices.insert(at, rec);
    w.next_device_no = w.next_device_no.checked_add(1).ok_or(SealError::CtlDeviceCap)?;
    Ok(())
}

/// `DeviceRevoked` (§13.6).
// SPEC-QUESTION: "after the whole entry at least one current device
// remains" is evaluated when the revocation is reached, from `W` and the
// remaining revocations and reinstatements of the entry, each counted with
// its effect (a later item that fails refuses the entry with its own code
// anyway); the failure is this item's, the first in order.
pub(crate) fn revoke(w: &mut Membership, target: &DevId, rest: &[CtlItem], cx: &EntryCx<'_>) -> Result<(), SealError> {
    if cx.author == Auth::Device(*target) {
        return Err(SealError::CtlAuthor);
    }
    if !w.device(target).is_some_and(DeviceRecord::is_current) {
        return Err(SealError::CtlTarget);
    }
    let remaining = w
        .devices
        .iter()
        .filter(|d| {
            if d.dev == *target || rest.iter().any(|i| matches!(i, CtlItem::DeviceRevoked(t) if *t == d.dev)) {
                return false;
            }
            d.is_current() || rest.iter().any(|i| matches!(i, CtlItem::DeviceReinstated(t) if *t == d.dev))
        })
        .count();
    if remaining == 0 {
        return Err(SealError::CtlLastDevice);
    }
    let by = cx.author.dev();
    if let Some(d) = w.device_mut(target) {
        d.status = DeviceStatus::Revoked;
        d.status_seq = cx.s;
        d.status_by = by;
    }
    if w.pending_override.is_some_and(|(_, voter, _)| voter == *target) {
        w.pending_override = None;
    }
    Ok(())
}

/// `DeviceReinstated` (§13.6, §13.12).
pub(crate) fn reinstate(w: &mut Membership, target: &DevId, cx: &EntryCx<'_>) -> Result<(), SealError> {
    let Auth::Recovery(by) = cx.author else { return Err(SealError::CtlAuthor) };
    let revoked_by_device = match w.device(target) {
        Some(d) => !d.is_current() && w.devices.iter().any(|x| x.dev == d.status_by),
        None => false,
    };
    if !revoked_by_device {
        return Err(SealError::CtlTarget);
    }
    if w.current_count() >= CURRENT_MAX {
        return Err(SealError::CtlDeviceCap);
    }
    if let Some(d) = w.device_mut(target) {
        d.status = DeviceStatus::Current;
        d.status_seq = cx.s;
        d.status_by = by;
        d.beat_ms = cx.at_ms;
    }
    Ok(())
}

/// The recipients a rotation must wrap to in `W`: its current devices and
/// its current recovery identity, ascending (§13.6 `EpochRotated`).
pub(crate) fn rotation_recipients(w: &Membership) -> Vec<[u8; 16]> {
    let mut ids: Vec<[u8; 16]> = w.devices.iter().filter(|d| d.is_current()).map(|d| d.dev.0).collect();
    if let Some(r) = w.recovery.iter().find(|r| r.until_seq == 0) {
        ids.push(r.dev.0);
    }
    ids.sort_unstable();
    ids
}

/// `EpochRotated` (§13.6, §13.8; SEAL-MEM-015, SEAL-MEM-017).
pub(crate) fn rotate(w: &mut Membership, r: &EpochRotated, cx: &EntryCx<'_>) -> Result<(), SealError> {
    if Some(r.epoch) != w.epoch.checked_add(1) {
        return Err(SealError::CtlEpoch);
    }
    let got: Vec<[u8; 16]> = r.wraps.iter().map(wrap_recipient).collect();
    if got != rotation_recipients(w) {
        return Err(SealError::CtlWraps);
    }
    if r.purge_before.is_some() && matches!(cx.author, Auth::Recovery(_)) {
        return Err(SealError::CtlAuthor);
    }
    if let Some(p) = r.purge_before
        && !(w.purge_seq < p && p <= cx.s)
    {
        return Err(SealError::CtlTarget);
    }
    if w.commitments.len() >= COMMIT_MAX {
        return Err(SealError::CtlCap);
    }
    w.epoch = r.epoch;
    w.epoch_seq = cx.s;
    w.epoch_clock_ms = cx.tau;
    w.commitments.push((r.epoch, cx.s, r.commit));
    if let Some(p) = r.purge_before {
        w.purge_seq = p;
    }
    Ok(())
}

/// The new keys of a recovery-key change are fresh: as for `DeviceAdded`,
/// and the new `dev_R` (§6.3) differs from every recorded `dev` and `dev_R`.
fn fresh_recovery(w: &Membership, sign_pk: &[u8; 32], hpke_pk: &[u8; 32], board: &BoardId) -> bool {
    let dev_r = RecoveryPublic { sign_pk: *sign_pk, hpke_pk: *hpke_pk }.dev(board);
    !(recorded_sign_pk(w, sign_pk) || recorded_hpke_pk(w, hpke_pk) || recorded_dev(w, &dev_r))
}

/// Installs a new recovery key at seq `s` (§13.6 `RecoveryKeyChange`): the current
/// record retires, the new one starts with counter 0 (SEAL-KEY-007).
fn install_recovery(w: &mut Membership, sign_pk: &[u8; 32], hpke_pk: &[u8; 32], cx: &EntryCx<'_>) -> Result<(), SealError> {
    if w.recovery.len() >= RECOVERY_MAX {
        return Err(SealError::CtlCap);
    }
    for r in w.recovery.iter_mut().filter(|r| r.until_seq == 0) {
        r.until_seq = cx.s;
    }
    let dev = RecoveryPublic { sign_pk: *sign_pk, hpke_pk: *hpke_pk }.dev(&cx.board);
    let rec = RecoveryRecord { dev, sign_pk: *sign_pk, hpke_pk: *hpke_pk, since_seq: cx.s, until_seq: 0, last_ctr: 0 };
    let at = w.recovery.partition_point(|r| r.since_seq <= cx.s);
    w.recovery.insert(at, rec);
    Ok(())
}

/// `RecoveryKeyChange` (§13.6, §13.11): author R, fresh keys, then the new
/// key is installed. The backup reader's projection (§21.6 step 6) runs this
/// same row.
pub(crate) fn recovery_key_change(w: &mut Membership, k: &RecoveryKeyChange, cx: &EntryCx<'_>) -> Result<(), SealError> {
    if !matches!(cx.author, Auth::Recovery(_)) {
        return Err(SealError::CtlAuthor);
    }
    if !fresh_recovery(w, &k.sign_pk, &k.hpke_pk, &cx.board) {
        return Err(SealError::CtlDuplicateKey);
    }
    install_recovery(w, &k.sign_pk, &k.hpke_pk, cx)
}

/// `ResetTo` (§13.6, §13.13; SEAL-MEM-027): only `last_reset` changes.
fn reset_to(w: &mut Membership, t: &ResetTo, cx: &EntryCx<'_>) -> Result<(), SealError> {
    if !matches!(cx.author, Auth::Device(_)) {
        return Err(SealError::CtlAuthor);
    }
    let ok = t.seq < cx.s
        && t.seq >= w.purge_seq
        && w.devices.iter().filter(|d| d.is_current()).all(|d| t.seq > d.bootstrap_seq)
        && hash_version_allowed(w.rules, t.hash_version);
    if !ok {
        return Err(SealError::CtlTarget);
    }
    w.last_reset = Some((cx.s, t.seq, t.hash_version, t.state_hash));
    Ok(())
}

/// The active members of a `RulesUpgrade` by `a` (§13.10, SEAL-MEM-019).
pub(crate) fn active_members<'w>(w: &'w Membership, a: &DevId) -> Vec<&'w DeviceRecord> {
    let reference = w.devices.iter().filter(|d| d.is_current() && d.dev != *a).map(|d| d.beat_ms).max();
    w.devices
        .iter()
        .filter(|d| d.is_current())
        .filter(|d| match reference {
            None => d.dev == *a,
            Some(r) => d.dev == *a || d.beat_ms >= r.saturating_sub(ACTIVE_WINDOW_MS),
        })
        .collect()
}

/// `RulesUpgrade` in both modes (§13.6, §13.10; SEAL-MEM-019 to
/// SEAL-MEM-022). Returns the version it activates, if any.
fn rules_upgrade(w: &mut Membership, u: &RulesUpgrade, cx: &EntryCx<'_>) -> Result<Option<u32>, SealError> {
    let Auth::Device(a) = cx.author else { return Err(SealError::CtlAuthor) };
    let v = u.rules;
    if v <= w.rules {
        return Err(SealError::CtlTarget);
    }
    match u.mode {
        RulesUpgradeMode::Normal => {
            if !active_members(w, &a).iter().all(|d| d.rules.contains(&v)) {
                return Err(SealError::CtlSupport);
            }
            w.rules = v;
            w.pending_override = None;
            Ok(Some(v))
        }
        RulesUpgradeMode::Override => match w.pending_override {
            Some((pv, voter, _)) if pv == v && voter == a => Err(SealError::CtlDuplicate),
            Some((pv, voter, _)) if pv == v && w.device(&voter).is_some_and(DeviceRecord::is_current) => {
                w.rules = v;
                w.pending_override = None;
                Ok(Some(v))
            }
            _ => {
                w.pending_override = Some((v, a, cx.s));
                Ok(None)
            }
        },
    }
}

/// The `dev` of a Skip target frame, read from its header (§8.1).
// SPEC-QUESTION: §13.6 compares "the target frame's dev" with the author;
// a frame at `t` may itself be malformed (classified `Skip` at step 1).
// Its bytes are authenticated by `frame_hash` in the signed item, so its
// `dev` is read from offset 20 when the frame is long enough; a frame too
// short to hold one has no `dev`, which differs from every author.
fn target_dev(frame: &[u8]) -> Option<DevId> {
    match frame_header(frame) {
        Ok(h) => Some(h.dev),
        Err(_) => take::<16>(frame, 20).map(|d| DevId(*d)),
    }
}

/// An `S`-shape entry in the two passes of §14.6 (SEAL-RCPT-009,
/// SEAL-MEM-027).
fn skips(w: &mut Membership, items: &[CtlItem], cx: &EntryCx<'_>) -> CtlOutcome {
    let targets: Vec<&SkipItem> = items.iter().filter_map(|i| if let CtlItem::Skip(s) = i { Some(s) } else { None }).collect();
    if targets.len() != items.len() {
        return CtlOutcome::Refused(SealError::CtlShape);
    }
    // Pass 1: what needs no frame: the floor, `ctl_seqs`, `skips`, and the
    // limit of the list (§23).
    let floor = w.purge_seq.max(cx.s.saturating_sub(SKIP_WINDOW));
    for (i, t) in targets.iter().enumerate() {
        let ok = t.seq < cx.s
            && t.seq >= floor
            && w.ctl_seqs.binary_search(&t.seq).is_err()
            && w.skips.binary_search_by(|x| x.0.cmp(&t.seq)).is_err();
        if !ok {
            return CtlOutcome::Refused(SealError::CtlTarget);
        }
        if w.skips.len().saturating_add(i) >= SKIPS_MAX {
            return CtlOutcome::Refused(SealError::CtlCap);
        }
    }
    // Pass 2, in item order: the frame's presence, its hash, its author. The
    // first missing frame ends the pass with every missing target listed.
    // SPEC-QUESTION: §14.6 lists presence, hash and author "for each item in
    // order" and also returns "every missing (t, frame_hash)": when a missing
    // frame comes before a supplied frame with a wrong hash, this returns
    // `NeedFrames` (the first failure in that order), and the refusal follows
    // when the caller classifies again with the frames; the verdict is the
    // same on every device.
    for (i, t) in targets.iter().enumerate() {
        let Some(frame) = (cx.skip_targets)(t.seq) else {
            let missing = targets
                .iter()
                .skip(i)
                .filter(|x| (cx.skip_targets)(x.seq).is_none())
                .map(|x| (x.seq, x.frame_hash))
                .collect();
            return CtlOutcome::NeedFrames(missing);
        };
        if !bool::from(frame_hash(frame).0.ct_eq(&t.frame_hash.0)) {
            return CtlOutcome::Refused(SealError::CtlTarget);
        }
        if let Auth::Device(a) = cx.author
            && target_dev(frame) == Some(a)
        {
            return CtlOutcome::Refused(SealError::CtlAuthor);
        }
    }
    for t in targets {
        let at = w.skips.partition_point(|x| x.0 < t.seq);
        w.skips.insert(at, (t.seq, t.frame_hash));
    }
    CtlOutcome::Applied { activated: None }
}
