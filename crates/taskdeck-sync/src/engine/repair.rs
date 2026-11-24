//! Repairs (§21.3): restore tail. STEP 7.

use super::Engine;
use crate::{Head, Sealer, StateMachine};

pub(crate) struct Tail {
    pub relay_head: Head,
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    /// A welcome arrived under evidence: the repair looks at the relay's head.
    pub(crate) fn repair_on_welcome(&mut self, head: Head) {
        self.evid.tail = Some(Tail { relay_head: head });
    }
}
