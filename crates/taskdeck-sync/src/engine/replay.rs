//! Replays from a base (§17.3, §28.1, §22.3, §28.3): jobs that read a base and
//! the log (`ReadBase`, `ReadLog`), classify and apply again into a state of
//! their own, and hand the result to the board or to an operation.
//!
//! STEP 4/9: the job machinery is written with Skip, ResetTo and the operations.

use std::marker::PhantomData;

use super::Engine;
use crate::{Dev, Hash32, Sealer, StateMachine};

pub(crate) struct Jobs<SM: StateMachine, S: Sealer> {
    _p: PhantomData<(SM, S)>,
}

impl<SM: StateMachine, S: Sealer> Default for Jobs<SM, S> {
    fn default() -> Self {
        Jobs { _p: PhantomData }
    }
}

impl<SM: StateMachine, S: Sealer> Jobs<SM, S> {
    /// A job is replacing the confirmed state: the board waits.
    pub(crate) fn holds_board(&self) -> bool {
        false
    }
    pub(crate) fn holds_log(&self) -> bool {
        false
    }
    pub(crate) fn take_done_reset(&mut self, _r: u64) -> bool {
        false
    }
    pub(crate) fn take_done_skip(&mut self, _s: u64, _t: u64) -> bool {
        false
    }
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    pub(crate) fn begin_reset(&mut self, _r: u64, _t: u64, _hash: Hash32, _author: Dev) -> bool {
        self.fault("engine/local_fault");
        false
    }
    pub(crate) fn begin_skip_replay(&mut self, _s: u64, _t: u64) -> bool {
        self.fault("engine/local_fault");
        false
    }
}
