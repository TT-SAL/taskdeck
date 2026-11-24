//! A revoked device and its reinstatement (§24.4).
//!
//! Rules: `StopKind::Revoked { by_log }` is the revoked state. `by_log = false`
//! is the relay's claim alone: it deletes nothing and clears itself when the
//! relay answers again or the log says otherwise.

use super::Engine;
use crate::{DeviceStatus, HaltKind, PersistOp, Sealer, StateMachine, StopKind};

#[derive(Default)]
pub(crate) struct Epochs {
    /// The classification reached the relay's head since the missed roots arrived.
    pub recheck: bool,
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    /// §24.4 cases 1 and 2 (ENG-EPOCH-002).
    pub(crate) fn set_revoked(&mut self, by_log: bool) {
        if self.has_stop(|k| matches!(k, StopKind::Revoked { by_log: true })) {
            return;
        }
        if by_log {
            self.end_stops(|s| matches!(s.kind, StopKind::Revoked { .. }));
        }
        self.add_stop(StopKind::Revoked { by_log }, None);
    }

    pub(crate) fn clear_revoked(&mut self) {
        self.end_stops(|s| matches!(s.kind, StopKind::Revoked { .. }));
        self.sess.paused_root = false;
    }

    /// The relay answered a welcome: a revocation only the relay claimed is over
    /// when the log holds this device as current.
    pub(crate) fn relay_answered(&mut self) {
        if self.has_stop(|k| matches!(k, StopKind::Revoked { by_log: false }))
            && self.me().is_some_and(|d| d.status == DeviceStatus::Current)
        {
            self.clear_revoked();
        }
    }

    /// The classification reached the relay's head: a device its membership
    /// still records as revoked is revoked again, by the log (§24.4).
    pub(crate) fn revoked_recheck(&mut self) {
        if self.me().is_some_and(|d| d.status == DeviceStatus::Revoked) && !self.revoked() {
            self.set_revoked(true);
        }
    }

    /// `MissedRoots` (ruling S24, ENG-EPOCH-005).
    pub(crate) fn on_missed_roots(&mut self, first_epoch: u32, wraps: Vec<[u8; 96]>) {
        if wraps.is_empty() || wraps.len() > 256 || first_epoch == 0 {
            self.protocol("missed_roots");
            return;
        }
        if !self.revoked() {
            return; // the relay returns the wraps until the next revoke
        }
        let Some(m) = self.mk.as_ref() else { return };
        match self.sealer.take_missed_wraps(m, first_epoch, &wraps) {
            Ok(true) => {
                // D4: the identity first; then classification goes on from the paused seq.
                let id = self.sealer.export_identity();
                self.send.identity_pid = self.persist(vec![PersistOp::WriteIdentity(id)]);
                self.clear_revoked();
                self.epochs.recheck = true;
            }
            Ok(false) => {}
            Err(e) if e.code == "seal/commitment" => {
                let author = self.vk.recovery_dev;
                let k = self.k;
                self.halt(HaltKind::Commitment, k, author, "seal/commitment");
            }
            Err(_) => self.fault("engine/local_fault"),
        }
    }
}
