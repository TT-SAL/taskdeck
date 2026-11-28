//! The adapters of decision D6 (device.md §4.1, §25; engine.md §5.1, §6.1):
//! `CoreMachine` implements `taskdeck_sync::StateMachine` over
//! `taskdeck-core`, and `SealSealer` implements `taskdeck_sync::Sealer` over
//! `taskdeck-seal`. `flow_tests` runs the real engine on both, in process.
//! Where the real crates did not fit the traits is recorded in
//! `docs/sync/status/device/fit.md` (findings `f1-*`).

mod core_machine;
mod membership;
mod seal_sealer;

pub use core_machine::CoreMachine;
pub use membership::SealMembership;
pub use seal_sealer::{GenesisInput, SealSealer};

#[cfg(test)]
mod flow_tests;
