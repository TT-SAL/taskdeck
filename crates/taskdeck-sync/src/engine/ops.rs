//! Operations (§28), pairing (§28.4), the move and its code (§28.3), purge
//! (§28.2), rules upgrades (§25). STEP 9: only what the first steps need.

use super::{Engine, lim};
use crate::{
    Body, CtlIntent, DeviceStatus, DeviceView, Hash32, NewDevice, Notification, Operation, OutboxRecord, Page,
    Sealer, StateMachine,
};

#[derive(Default)]
pub(crate) struct Ops {}

/// The `Operation` variant in snake_case (§30.1 `OpStatus.kind`).
pub(crate) fn op_kind(op: &Operation) -> &'static str {
    match op {
        Operation::RestoreTo { .. } => "restore_to",
        Operation::Purge => "purge",
        Operation::Move { .. } => "move",
        Operation::FollowAddress { .. } => "follow_address",
        Operation::Revoke { .. } => "revoke",
        Operation::Skip { .. } => "skip",
        Operation::RulesOverride { .. } => "rules_override",
        Operation::CloseBoard { .. } => "close_board",
        Operation::Adopt { .. } => "adopt",
    }
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    pub(crate) fn on_op(&mut self, ticket: u64, _op: Operation) {
        self.notify(Notification::OpDone { ticket, result: Err("engine/not_eligible") });
    }
    pub(crate) fn on_pair_register(&mut self, ticket: u64, _device: NewDevice) {
        let code = if self.halted() { "engine/halted" } else { "engine/not_eligible" };
        self.notify(Notification::PairFailed { ticket, code });
    }
    pub(crate) fn step_ops(&mut self) {}
    /// An own record's entry reached the board head (`code`: refused), or could not be sealed (`seq` 0).
    pub(crate) fn own_entry_done(&mut self, _r: &OutboxRecord, _seq: u64, _code: Option<&'static str>) {}
    pub(crate) fn before_carried_seal(&mut self, _i: usize) -> bool {
        true
    }
    pub(crate) fn carried_done(&mut self, _id: &[u8; 16], _result: Result<(u64, Hash32), &'static str>) {}
    pub(crate) fn carried_conflict(&mut self, _id: &[u8; 16]) {}
    pub(crate) fn fence_accepted(&mut self, _seq: u64, _frame_hash: Hash32, _own: bool) {
        self.status_dirty = true;
    }
    pub(crate) fn follow_holds_reads(&self) -> bool {
        false
    }
    pub(crate) fn follow_page(&mut self, _page: &Page) -> bool {
        false
    }
    pub(crate) fn on_move_code(&mut self, _text: String) {
        self.notify(Notification::MoveCodeResult { result: Err("engine/move_no_fence") });
    }
    pub(crate) fn upgrade_step(&mut self) {}
    pub(crate) fn purge_step(&mut self) {}
    pub(crate) fn shred_step(&mut self) {}
    pub(crate) fn local_purge(&mut self, _r: u64, _p: u64, _epoch: u32, _e: &super::recv::Entry<S::Membership>) {}

    pub(crate) fn restore_hash_ready(&mut self, _ticket: u64, _t: u64, _out: super::replay::Replayed<SM, S>) {}
    pub(crate) fn adopt_membership_ready(&mut self, _ticket: u64, _s: u64, _out: super::replay::Replayed<SM, S>) {}
    pub(crate) fn adopt_board_ready(&mut self, _ticket: u64, _s: u64, _out: super::replay::Replayed<SM, S>) {}
    pub(crate) fn seed_replayed(&mut self, _ticket: u64, _out: super::replay::Replayed<SM, S>) {}

    /// §17.1: may this device compose a Skip of `t` (ENG-SKIP-004)?
    pub(crate) fn skip_allowed(&self, t: u64) -> Result<(), &'static str> {
        const NO: &str = "engine/skip_target";
        if t < self.floor || t > self.l {
            return Err(NO);
        }
        if self.vk.ctl_seqs.contains(&t) || self.vk.skips.iter().any(|s| s.0 == t) {
            return Err(NO);
        }
        if self.outbox.iter().any(|r| r.r.seen == Some(t)) {
            return Err(NO); // this device's own entry
        }
        let floor = self.vk.purge_seq.max((self.l + 1).saturating_sub(lim::SKIP_WINDOW));
        if t < floor {
            return Err(NO);
        }
        Ok(())
    }

    /// The active members of `seal.md` §13.10, with this device as the author.
    pub(crate) fn active_members(&self) -> Vec<&DeviceView> {
        let now = self.vk.clock_ms;
        self.vk
            .devices
            .iter()
            .filter(|d| d.status == DeviceStatus::Current)
            .filter(|d| d.dev == self.own || now.saturating_sub(d.beat_ms) <= lim::THIRTY_DAYS)
            .collect()
    }

    /// Does this outbox record carry a Skip?
    pub(crate) fn is_skip(body: &Body) -> bool {
        matches!(body, Body::Ctl(CtlIntent::Skip { .. }))
    }
}
