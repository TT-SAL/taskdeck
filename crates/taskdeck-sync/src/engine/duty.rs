//! Duties with no always-on device (§29): who is eligible, the duty order,
//! beats (§23), the snapshot duty (§19.2 to §19.5), compaction (§20), and the
//! clock guard (§27).
//!
//! Rules: every duty is looked at again on each call (`step_duties`) and taken
//! only while `eligible()` holds. The decisions are pure functions of the
//! membership view, the registry, `Meta` and the clocks (`rank_of`,
//! `usable`, `beat_due`, `snapshot_due`, `clock_state`), so the vectors of
//! `engine-duties.json` and `engine-clock.json` test them directly.

use sha2::{Digest, Sha256};

use super::{Engine, Want, lim};
use crate::{
    AdminRequest, AlarmKind, BeatInfo, BlobRequest, Body, Checkpoint, Dev, DeviceStatus, DeviceView, Effect, Evidence,
    Kind, MembershipView, Sealer, SessionState, SnapInfo, SnapRecord, SnapshotMaterial, StateMachine, Verdict, Via,
    state_hash,
};

#[derive(Default)]
pub(crate) struct Duty {
    /// A checkpoint is owed before the next entry is applied (§18.1 case 4).
    pub ckpt_wanted: bool,
    /// The latest measurement: `(skew_ms, mono_ms)` (§27.1).
    pub skew: Option<(i64, u64)>,
    /// This device's rank in `M_S` when the snapshot duty was recorded.
    pub snap_rank: Option<u32>,
    /// The snapshot being written: the base read, then the upload.
    pub snap_busy: bool,
    pub snap_info: Option<SnapInfo>,
    /// Snapshots owed because of a mismatch (§19.2 condition 2).
    pub owed_mismatch: u32,
    pub compact_busy: bool,
    /// The latest `Hello` sent only to refresh the head for a duty.
    pub fresh_hello: Option<u64>,
}

/// The duty order of §29.2: the current devices by `beat_ms` descending, then `dev` ascending.
pub(crate) fn duty_order(devices: &[DeviceView]) -> Vec<Dev> {
    let mut v: Vec<&DeviceView> = devices.iter().filter(|d| d.status == DeviceStatus::Current).collect();
    v.sort_by(|a, b| b.beat_ms.cmp(&a.beat_ms).then(a.dev.cmp(&b.dev)));
    v.into_iter().map(|d| d.dev).collect()
}

/// A device's 0-based rank in the duty order (ENG-DUTY-001).
pub(crate) fn rank_of(view: &MembershipView, dev: &Dev) -> Option<u32> {
    duty_order(&view.devices).iter().position(|d| d == dev).map(|p| p as u32)
}

/// §19.3: at least one `Ack` from a device other than the writer, and no `Mismatch` from any.
pub(crate) fn usable(r: &SnapRecord, own: &Dev) -> bool {
    let own_v = r.own.map(|v| (*own, v));
    let all = r.verdicts.iter().map(|v| (v.0, v.1)).chain(own_v);
    let mut ack = false;
    for (dev, v) in all {
        match v {
            Verdict::Mismatch => return false,
            Verdict::Ack if dev != r.author => ack = true,
            Verdict::Ack => {}
        }
    }
    ack
}

/// §23.1: which condition makes a beat due, if any (1, 2 or 3).
pub(crate) fn beat_due(wall_ms: i64, last_beat_ms: i64, lists_differ: bool, pending_verdicts: bool) -> Option<u8> {
    let since = wall_ms.saturating_sub(last_beat_ms);
    if since >= lim::DAY {
        Some(1)
    } else if lists_differ {
        Some(2)
    } else if pending_verdicts && since >= lim::HOUR {
        Some(3)
    } else {
        None
    }
}

/// §19.2: is a snapshot due at the aligned `S` (ENG-SNAP-001, ENG-SNAP-006)?
pub(crate) fn snapshot_due(
    s: u64,
    tau_s: i64,
    snapshots: &[SnapRecord],
    own: &Dev,
    own_snap_in_outbox: bool,
    owed_mismatch: bool,
) -> bool {
    if own_snap_in_outbox {
        return false;
    }
    if let Some(newest_own) = snapshots.iter().filter(|r| &r.author == own).max_by_key(|r| r.entry_seq)
        && tau_s.saturating_sub(newest_own.entry_clock) < lim::DAY {
            return false;
        }
    if owed_mismatch {
        return true;
    }
    let fresh_usable = snapshots.iter().any(|r| {
        usable(r, own) && tau_s.saturating_sub(r.entry_clock) < lim::WEEK && s.saturating_sub(r.info.seq) < 1000
    });
    let waiting = snapshots
        .iter()
        .any(|r| r.verdicts.is_empty() && r.own.is_none() && tau_s.saturating_sub(r.entry_clock) < 2 * lim::DAY);
    !fresh_usable && !waiting
}

/// §27: `(skew, warning, blocked)` from the latest measurement, a confirmation and the clocks.
pub(crate) fn clock_state(
    measurement: Option<(i64, u64)>,
    confirmation: Option<(i64, i64)>,
    wall_ms: i64,
    mono_ms: u64,
) -> (Option<i64>, bool, bool) {
    let skew = measurement.filter(|m| mono_ms.saturating_sub(m.1) <= 86_400_000).map(|m| m.0);
    let Some(s) = skew else { return (None, false, false) };
    let warning = s.unsigned_abs() > 300_000;
    let confirmed = confirmation.is_some_and(|(c_wall, c_skew)| {
        wall_ms.saturating_sub(c_wall) <= lim::WEEK && (s as i128 - c_skew as i128).abs() <= 300_000
    });
    (Some(s), warning, s.unsigned_abs() > 3_600_000 && !confirmed)
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    // ------------------------------------------------------------------ the clock (§27)

    /// §27.1: every welcome, hello answer and page carries `relay_now`.
    pub(crate) fn measure(&mut self, relay_now: i64) {
        self.duty.skew = Some((self.wall.saturating_sub(relay_now), self.mono));
        self.status_dirty = true;
    }

    pub(crate) fn clock(&self) -> (Option<i64>, bool, bool) {
        clock_state(self.duty.skew, self.meta.clock, self.wall, self.mono)
    }

    /// ENG-CLOCK-002: composing is blocked.
    pub(crate) fn clock_blocked(&self) -> bool {
        self.clock().2
    }

    /// `UserAction::ConfirmClock`.
    pub(crate) fn confirm_clock(&mut self) {
        if let (Some(s), _, _) = self.clock() {
            self.meta.clock = Some((self.wall, s));
            self.touch_meta();
        }
    }

    // ------------------------------------------------------------------ eligible (§29.1)

    /// Current, joined and caught up (§29.1 item 1).
    pub(crate) fn caught_up(&self) -> bool {
        self.sess.state == SessionState::Live
            && self.sess.relay_head.is_some_and(|h| h.seq == self.l)
            && self.sess.caught_mono.is_some_and(|t| self.mono.saturating_sub(t) <= lim::CAUGHT_UP_MS)
    }

    pub(crate) fn current_member(&self) -> bool {
        self.no_b.is_some() && self.me().is_some_and(|d| d.status == DeviceStatus::Current)
    }

    /// ENG-DUTY-002.
    pub(crate) fn eligible(&self) -> bool {
        self.current_member()
            && self.caught_up()
            && !self.halted()
            && !self.stopped()
            && !self.clock_blocked()
            && !self.pipe.tail
            && !self.exiting
    }

    /// Everything but the freshness of the head holds: a `Hello` would make the device eligible.
    fn eligible_but_stale(&self) -> bool {
        self.current_member()
            && self.sess.state == SessionState::Live
            && !self.caught_up()
            && self.sess.read.is_none()
            && self.pipe.flight.is_none()
            && !self.halted()
            && !self.stopped()
            && !self.clock_blocked()
            && !self.pipe.tail
            && !self.exiting
    }

    /// Step 5 of §4.5.
    pub(crate) fn step_duties(&mut self) {
        if self.dead || !self.on {
            return;
        }
        if self.sess.wait_welcome {
            // §11.9: after a whole-request refusal nothing is repeated until the next welcome.
            return;
        }
        self.shred_step();
        self.fence_lookup();
        self.repair_step();
        self.reboot_step();
        if !self.eligible() {
            if self.eligible_but_stale() && self.duty_pending() {
                // A duty waits only for a head newer than 600,000 ms: ask for one.
                if self.sess.hello_mono.is_none_or(|t| self.mono.saturating_sub(t) >= lim::POLL_MS) {
                    self.duty.fresh_hello = Some(self.mono);
                    self.say_hello();
                }
            }
            return;
        }
        self.beat_step();
        self.snapshot_step();
        self.compact_step();
        self.upgrade_step();
        self.purge_step();
    }

    /// Would some duty act if the device were eligible?
    fn duty_pending(&self) -> bool {
        self.beat_reason().is_some()
            || self.meta.duties.snapshot.is_some()
            || self.meta.duties.purge.is_some()
            || self.meta.duties.upgrade.is_some()
            // A compaction is a duty only on a day on which there is something to compact.
            || (self.compact_day_open() && self.compact_bound().is_some())
    }

    // ------------------------------------------------------------------ beats (§23)

    fn beat_reason(&self) -> Option<u8> {
        if self.outbox.iter().any(|r| r.r.kind == Kind::Beat) {
            return None;
        }
        let me = self.me()?;
        let lists_differ = me.rules != self.sm.supported_rules()
            || me.envelopes != self.sealer.supported_envelopes()
            || me.hash_versions != self.sm.hash_versions();
        beat_due(self.wall, self.meta.last_beat_ms, lists_differ, !self.registry.pending_verdicts.is_empty())
    }

    /// ENG-BEAT-001, ENG-BEAT-002.
    fn beat_step(&mut self) {
        if self.beat_reason().is_none() {
            return;
        }
        let aligned = self.b - self.b % lim::STRIDE;
        let state = if aligned >= lim::STRIDE {
            self.hashes.get(&aligned).map(|h| (h.seq, h.hash, h.hash_version))
        } else {
            None
        };
        let mut acks = self.registry.pending_verdicts.clone();
        acks.sort_by_key(|a| a.0);
        acks.dedup_by_key(|a| a.0);
        acks.truncate(64);
        let info = BeatInfo {
            rules: self.sm.supported_rules().to_vec(),
            envelopes: self.sealer.supported_envelopes(),
            hash_versions: self.sm.hash_versions().to_vec(),
            state,
            acks,
        };
        if self.enqueue_own(Kind::Beat, Body::Beat(info), Via::Append, None).is_some() {
            self.meta.last_beat_ms = self.wall;
            self.touch_meta();
        }
    }

    // ------------------------------------------------------------------ the snapshot duty (§19.2, §19.4)

    /// At the board apply of the aligned `S`: is a snapshot due? Records the duty.
    pub(crate) fn snapshot_due_at(&mut self, s: u64, tau: i64) -> bool {
        let in_outbox = self.outbox.iter().any(|r| r.r.kind == Kind::Snap);
        let due = snapshot_due(s, tau, &self.registry.snapshots, &self.own, in_outbox, self.duty.owed_mismatch > 0);
        if due {
            self.meta.duties.snapshot = Some((s, tau, 0));
            self.duty.snap_rank = self.mb.as_ref().and_then(|m| rank_of(&self.sealer.view(m), &self.own));
            self.duty.snap_busy = false;
            self.touch_meta();
        }
        due
    }

    /// An accepted `snap` entry clears the duty for its seq and below (§19.2).
    pub(crate) fn snap_accepted(&mut self, _entry_seq: u64, author: Dev, info: &SnapInfo) {
        if let Some((s, _, _)) = self.meta.duties.snapshot
            && info.seq >= s {
                self.meta.duties.snapshot = None;
                self.touch_meta();
            }
        if author == self.own && self.duty.owed_mismatch > 0 {
            self.duty.owed_mismatch -= 1;
        }
        self.rejoin_snap_seen(author, info);
    }

    /// ENG-SNAP-002: writes the due snapshot when eligible and the rank wait has passed.
    fn snapshot_step(&mut self) {
        let Some((s, tau, failures)) = self.meta.duties.snapshot else { return };
        if self.duty.snap_busy {
            return;
        }
        if self.outbox.iter().any(|r| r.r.kind == Kind::Snap) {
            // The announcement of the uploaded snapshot is on its way (§19.4 step 4): the duty
            // waits for its fate instead of uploading again.
            return;
        }
        if failures >= 3 {
            self.meta.duties.snapshot = None;
            self.touch_meta();
            return;
        }
        let rank = self.duty.snap_rank.or_else(|| rank_of(&self.vk, &self.own)).unwrap_or(0);
        if self.wall < tau.saturating_add(rank as i64 * lim::HOUR) {
            return;
        }
        if !self.bases.iter().any(|b| b.0 == s) || !self.hashes.contains_key(&s) {
            // The base it would be written from is gone: the duty cannot be met.
            self.meta.duties.snapshot = None;
            self.touch_meta();
            return;
        }
        self.duty.snap_busy = true;
        let req = self.want(Want::SnapBase(s));
        self.fx(Effect::ReadBase { req, seq: s });
    }

    /// §19.4 steps 1 to 3: the base at `S` arrived.
    pub(crate) fn snapshot_base(&mut self, s: u64, base: Option<Checkpoint>) {
        let Some(c) = base.filter(|c| c.board_seq == s) else {
            self.duty.snap_busy = false;
            self.fault("engine/local_fault");
            return;
        };
        let Some(m) = self.mk.clone() else { return };
        let board_hash: [u8; 32] = Sha256::digest(&c.board).into();
        let hash = state_hash(1, &board_hash, &c.membership);
        if self.hashes.get(&s).map(|h| h.hash) != Some(hash) {
            self.duty.snap_busy = false;
            self.fault("engine/local_fault");
            return;
        }
        let material = SnapshotMaterial {
            seq: s,
            chain: c.board_chain,
            hash_version: 1,
            state_hash: hash,
            board: c.board,
            membership: c.membership,
        };
        match self.sealer.seal_snapshot(&m, &material) {
            Err(_) => {
                // `seal/snapshot_large`: nothing is uploaded or announced; the duty stays.
                self.duty.snap_busy = false;
                self.bump_snapshot_failure();
            }
            Ok(blob) => {
                self.duty.snap_info = Some(SnapInfo {
                    seq: s,
                    hash,
                    hash_version: 1,
                    blob: blob.id,
                    sha256: blob.sha256,
                    size: blob.bytes.len() as u64,
                    parts: blob.parts,
                    inflated: blob.inflated,
                    epoch: blob.epoch,
                });
                let req = self.want(Want::SnapUpload);
                self.fx(Effect::Blob { req, op: BlobRequest::Upload { purpose: 1, seq: s, blob } });
            }
        }
    }

    pub(crate) fn bump_snapshot_failure(&mut self) {
        if let Some((s, tau, f)) = self.meta.duties.snapshot {
            self.meta.duties.snapshot = if f + 1 >= 3 { None } else { Some((s, tau, f + 1)) };
            self.touch_meta();
        }
    }

    /// §19.4 step 4: the upload finished (ENG-SNAP-004).
    pub(crate) fn snapshot_uploaded(&mut self, ok: bool) {
        self.duty.snap_busy = false;
        let info = self.duty.snap_info.take();
        match (ok, info) {
            (true, Some(info)) => {
                let still_due = self.meta.duties.snapshot.is_some_and(|d| d.0 == info.seq);
                if still_due && self.current_member() && !self.stopped() && !self.halted() {
                    self.enqueue_own(Kind::Snap, Body::Snap(info), Via::Append, None);
                }
            }
            _ => self.bump_snapshot_failure(),
        }
    }

    // ------------------------------------------------------------------ compaction (§20)

    fn utc_day(&self) -> i64 {
        self.wall.div_euclid(lim::DAY)
    }

    /// May this device compact today, by its rank (§20.1)?
    fn compact_day_open(&self) -> bool {
        if self.duty.compact_busy || self.meta.duties.last_compact_day >= self.utc_day() {
            return false;
        }
        let rank = rank_of(&self.vk, &self.own).unwrap_or(0) as i64;
        self.wall >= self.utc_day() * lim::DAY + rank * lim::HOUR
    }

    /// §20.2 (ENG-COMPACT-001 to ENG-COMPACT-004).
    /// Steps 1 and 2 of §20.2: the snapshot relied on (its seq is the first bound, then its
    /// entry's seq and its blob) and the bound `b`. None: no compaction today.
    fn compact_bound(&self) -> Option<(u64, [u8; 16], u64)> {
        // Step 1: the newest usable snapshot by another writer that this device acknowledged.
        let rec = self
            .registry
            .snapshots
            .iter()
            .filter(|r| r.author != self.own && r.own == Some(Verdict::Ack) && usable(r, &self.own))
            .max_by_key(|r| r.entry_seq)?;
        let (s, x, blob_id) = (rec.info.seq, rec.entry_seq, rec.info.blob);
        // Step 2: the bound.
        if self.l <= lim::SKIP_WINDOW {
            return None;
        }
        let cutoff = self.wall.saturating_sub(31_622_400_000);
        let y = self.last_seq_at_or_before(cutoff)?;
        let b = s.min(self.l - lim::SKIP_WINDOW).min(y + 1);
        (b > self.sess.floor).then_some((x, blob_id, b))
    }

    fn compact_step(&mut self) {
        if !self.compact_day_open() {
            return;
        }
        let Some((x, blob_id, mut b)) = self.compact_bound() else { return };
        let floor = self.sess.floor;
        // Step 3: the keep list.
        let mut keep: Vec<u64> = self
            .vk
            .ctl_seqs
            .iter()
            .copied()
            .chain(self.vk.skips.iter().map(|t| t.0))
            .filter(|q| *q >= floor && *q < b)
            .collect();
        keep.sort_unstable();
        keep.dedup();
        if keep.len() > 131_072 {
            b = keep[131_072];
            keep.truncate(131_072);
        }
        self.duty.compact_busy = true;
        let req = self.want(Want::Compact);
        self.fx(Effect::Admin {
            req,
            op: AdminRequest::Compact { below_seq: b, snap_entry_seq: x, blob_id, keep },
        });
    }

    /// The largest seq of the log whose log clock is at or before `cutoff`.
    fn last_seq_at_or_before(&self, cutoff: i64) -> Option<u64> {
        let (mut lo, mut hi) = (self.floor, self.l + 1);
        if self.tau_at(lo).is_none_or(|t| t > cutoff) {
            return None;
        }
        while lo + 1 < hi {
            let mid = lo + (hi - lo) / 2;
            if self.tau_at(mid).is_some_and(|t| t <= cutoff) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(lo)
    }

    /// The answer of a `Compact` (§20.2): the day is recorded whatever it says.
    pub(crate) fn compact_result(&mut self, refusal: Option<(u16, &str)>) {
        self.duty.compact_busy = false;
        self.meta.duties.last_compact_day = self.utc_day();
        self.touch_meta();
        if let Some((status, code)) = refusal {
            let quiet = matches!(
                (status, code),
                (409, "relay/too_recent") | (409, "relay/no_snapshot") | (409, "relay/target") | (429, "relay/delete_budget")
            );
            if !quiet {
                self.alarm(
                    AlarmKind::RelayProtocol,
                    None,
                    None,
                    Some(code),
                    Evidence::Request { request: "admin/compact".to_owned() },
                );
            }
        }
    }

    /// A `Compact` whose answer never came: the next call may send it again today.
    pub(crate) fn compact_lost(&mut self) {
        self.duty.compact_busy = false;
    }
}
