//! Integrity evidence and the stop (§21), determinism mismatch and the
//! divergence stop (§22.1, §22.2).
//!
//! Rules: evidence is an alarm plus a `StopKind::Evidence` stop that names it;
//! the stop ends only when its alarm is resolved by a repair (§21.3) or by
//! following a `Fence` (§28.3). The divergence stop compares this device's
//! hash record at an aligned seq with the latest reports of the current
//! devices (`registry.reports`).

use std::collections::BTreeSet;

use super::Engine;
use crate::{AlarmKind, Dev, DeviceStatus, Evidence, Hash32, Sealer, StateMachine, StopKind};

#[derive(Default)]
pub(crate) struct Evid {
    /// Aligned seqs whose mismatch already made a snapshot due (§19.2 condition 2).
    pub owed_for: BTreeSet<u64>,
    /// The aligned seq at which the divergence stop was entered.
    pub diverged_at: Option<u64>,
    /// The divergence stop was just entered: a beat with this device's report goes at once (§22.2).
    pub beat_now: bool,
    /// The restore tail in progress (§21.3), see repair.rs.
    pub tail: Option<super::repair::Tail>,
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    /// Raises an alarm with evidence and the evidence stop (§21.2, ENG-EVID-003).
    pub(crate) fn raise_evidence(
        &mut self,
        kind: AlarmKind,
        seq: Option<u64>,
        author: Option<Dev>,
        code: Option<&str>,
        evidence: Evidence,
    ) {
        let id = self.alarm(kind, seq, author, code, evidence);
        self.add_stop(StopKind::Evidence, Some(id));
    }

    /// Resolves the evidence alarms `f` selects and ends their stops (ENG-EVID-005).
    pub(crate) fn resolve_evidence(&mut self, f: impl Fn(AlarmKind) -> bool) {
        let ids: Vec<u64> = self.meta.alarms.iter().filter(|a| !a.resolved && f(a.kind)).map(|a| a.id).collect();
        if ids.is_empty() {
            return;
        }
        self.resolve_alarms(|a| ids.contains(&a.id));
        self.end_stops(|s| s.kind == StopKind::Evidence && s.alarm.is_some_and(|a| ids.contains(&a)));
        if !self.evidence_stop() {
            // ENG-SESS-010: the stop ended; the session looks again at once.
            self.sess.need_hello = true;
            self.sess.wait_welcome = false;
        }
    }

    /// §22.2 for the aligned seq `s`: called when this device records its hash
    /// there and when a report for `s` arrives (ENG-DIVERGE-001).
    pub(crate) fn check_divergence(&mut self, s: u64) {
        let Some(own) = self.hashes.get(&s).filter(|h| h.hash_version == 1).map(|h| h.hash) else { return };
        let current: Vec<Dev> =
            self.vk.devices.iter().filter(|d| d.status == DeviceStatus::Current).map(|d| d.dev).collect();
        let mut groups: Vec<(Hash32, Vec<Dev>)> = vec![(own, vec![self.own])];
        for r in self.registry.reports.iter().filter(|r| r.seq == s && r.hash_version == 1) {
            if r.dev == self.own || !current.contains(&r.dev) {
                continue;
            }
            match groups.iter_mut().find(|g| g.0 == r.hash) {
                Some(g) => g.1.push(r.dev),
                None => groups.push((r.hash, vec![r.dev])),
            }
        }
        let g = groups[0].1.len();
        let m = groups.iter().skip(1).map(|x| x.1.len()).max().unwrap_or(0);
        // §22.2: the stop ends when, at a later aligned seq, this device's group is strictly
        // larger than every other group and has another member.
        if g > m && g >= 2 && self.evid.diverged_at.is_some_and(|d| s >= d) {
            // ENG-DIVERGE-005: at the seq where the stop began, or a later one.
            self.evid.diverged_at = None;
            self.evid.beat_now = false;
            self.end_stops(|x| x.kind == StopKind::Divergence);
            if m == 0 {
                self.resolve_alarms(|a| a.kind == AlarmKind::StateMismatch);
            }
        }
        if m == 0 {
            return;
        }
        groups.sort();
        for g in &mut groups {
            g.1.sort();
        }
        let id = self.alarm(AlarmKind::StateMismatch, Some(s), None, None, Evidence::Mismatch { groups });
        if g >= m && self.evid.owed_for.insert(s) {
            // The majority, or a tie, owes one snapshot (§19.2 condition 2).
            self.duty.owed_mismatch += 1;
        }
        if g <= m {
            self.enter_divergence(s, id);
        }
    }

    /// The divergence stop (§22.2, ENG-DIVERGE-002).
    pub(crate) fn enter_divergence(&mut self, s: u64, alarm: u64) {
        if self.evid.diverged_at.is_none() {
            self.evid.diverged_at = Some(s);
            // ENG-DIVERGE-002: its report is the evidence the other devices need to count it.
            self.evid.beat_now = true;
        }
        self.add_stop(StopKind::Divergence, Some(alarm));
    }

}
