//! Rejoin after a long absence (§19.7) and the stop of §19.9. STEP 6/7.

use super::Engine;
use crate::{Dev, Sealer, SessionState, SnapInfo, StateMachine};

#[derive(Default)]
pub(crate) struct Boot {
    pub confirmed: bool,
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    /// §19.7 step 1: the relay no longer holds the log this device needs.
    pub(crate) fn enter_rejoin(&mut self) {
        if self.sess.state != SessionState::Rejoining {
            self.sess.state = SessionState::Rejoining;
            self.sess.rejoin_since = Some(self.wall);
            self.status_dirty = true;
        }
    }
    pub(crate) fn confirm_rejoin(&mut self) {
        self.boot.confirmed = true;
    }
    pub(crate) fn rejoin_snap_seen(&mut self, _author: Dev, _info: &SnapInfo) {}
}
