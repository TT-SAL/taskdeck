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

pub(crate) use seal_sealer::snap_to_seal;

/// `M_0` (seal.md §13.3): the membership layout with every field zero.
pub(crate) fn membership_zero() -> Option<Vec<u8>> {
    membership::Fields::default().encode()
}

/// The home generation a membership records (seal.md §13.1).
pub(crate) fn home_gen_of(bytes: &[u8]) -> Option<u32> {
    membership::Fields::parse(bytes).map(|f| f.home_gen)
}
