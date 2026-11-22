//! `taskdeck-sync`: the sans-IO replica engine, a deterministic function from
//! inputs to effects, generic over a state machine and a sealer it defines
//! itself (decision D6), and its simulator.
//!
//! Spec: `docs/sync/spec/engine.md`. Owner: stage `engine`, then `device`
//! (SYNC_PLAN.md §4). Created as a shell by stage `foundation`; the skeleton
//! types come from the spec's Public API (§37). Every body still to be
//! written panics as unimplemented; the toy state machine (§5.3) is working
//! code behind the feature `sim`.
//!
//! No clock, file, network, thread, environment or randomness here
//! (ENG-GEN-003): `clippy.toml` in this directory disallows them.
#![forbid(unsafe_code)]
// The skeleton's bodies do not read their parameters yet; the owning stage removes this.
#![allow(unused_variables)]

mod events;
mod model;
mod ops;
mod receive;
mod records;
mod session;
mod snapshots;
mod ui;

#[cfg(feature = "sim")]
pub mod fake;
#[cfg(feature = "sim")]
pub mod sim;
#[cfg(feature = "sim")]
pub mod toy;

// ---------- traits (§5, §6)
pub use model::{Applied, Coalesce, Ctx, SmError, SmRefusal, StateMachine};
pub use model::{
    BeatInfo, Body, Class, ClassifyInput, CtlIntent, CtlItem, Dev, DeviceStatus, DeviceView, Halt, Hash32, Head,
    Kind, MembershipView, Opened, PendingRecovery, PlainEntry, Receipt, RecoveryMode, RecoveryView, Sealed,
    SealedBlob, Sealer, SealerError, SnapInfo, SnapshotMaterial, UpgradeMode, Verdict,
};

// ---------- events and effects (§4)
pub use events::{AppendResult, Effect, Event, Input, LocalReadResult, NewDevice, RelayTarget};
pub use ops::{OpRecord, Operation};
pub use receive::{Page, PageEntry};
pub use session::{AdminOk, AdminRequest, RelayRefusal, SeedDevice, Welcome};
pub use snapshots::{BlobMeta, BlobOk, BlobRequest};
pub use ui::{
    AlarmKind, AlarmRecord, Banner, BannerDetail, BannerKind, Blocker, ClockStatus, DeviceRow, Evidence, ForkSource,
    HaltView, MovingView, NoticeEntry, Notification, OpStatus, PendingMark, RecoveryChangeView, RulesStatus,
    SessionState, SkipOffer, Status, StatusWarning, StopAction, StopView, UserAction, VersionStatus,
};

// ---------- records (§7)
pub use records::{
    Bootstrap, Checkpoint, DutyState, HaltKind, HaltRecord, HashRecord, LastEntry, Loaded, LogRecord, Marker, Meta,
    NoticeKind, NoticeOp, OutboxOp, OutboxRecord, PersistOp, Registry, Report, SnapRecord, StopKind, StopRecord, Via,
};

/// The state hash of a board hash and membership bytes (engine.md §18.3, ruling S7).
pub fn state_hash(hash_version: u32, board_hash: &Hash32, membership_bytes: &[u8]) -> Hash32 {
    unimplemented!()
}

/// The engine's configuration, reserved (engine.md §37).
pub struct Config {}

/// One board's replica (engine.md §4.1).
pub struct Engine<SM: StateMachine, S: Sealer> {
    _parts: core::marker::PhantomData<(SM, S)>,
}

impl<SM: StateMachine, S: Sealer> Engine<SM, S> {
    /// An engine, inert until `Start` (engine.md §37).
    pub fn new(sm: SM, sealer: S, config: Config) -> Self {
        unimplemented!()
    }

    /// Handles one input (engine.md §4.1, §4.5).
    pub fn handle(&mut self, input: Input<SM::Command>) -> Vec<Effect> {
        unimplemented!()
    }

    /// The pending view (engine.md §8.2).
    pub fn view(&self) -> &SM::State {
        unimplemented!()
    }

    /// The pending view's version (engine.md §8.2).
    pub fn view_version(&self) -> u64 {
        unimplemented!()
    }

    /// The confirmed state at `B` (engine.md §8.1).
    pub fn confirmed(&self) -> (&SM::State, Head) {
        unimplemented!()
    }

    /// The membership view of `M_K` (engine.md §6.2, §7.1).
    pub fn membership(&self) -> MembershipView {
        unimplemented!()
    }

    /// The bytes of `M_B`, for backups (engine.md §37).
    pub fn membership_bytes(&self) -> Vec<u8> {
        unimplemented!()
    }

    /// This device's number, once known (engine.md §37).
    pub fn device_no(&self) -> Option<u16> {
        unimplemented!()
    }

    /// The status (engine.md §30.1).
    pub fn status(&self) -> Status {
        unimplemented!()
    }

    /// The pending entries' marks (engine.md §8.2, §37).
    pub fn pending(&self) -> Vec<PendingMark> {
        unimplemented!()
    }

    /// The notice and refusal records (engine.md §13.4, §37).
    pub fn notices(&self) -> Vec<NoticeEntry> {
        unimplemented!()
    }

    /// The points a restore can target, as `(seq, τ)` (engine.md §28.1).
    pub fn restore_targets(&self) -> Vec<(u64, i64)> {
        unimplemented!()
    }

    /// A purge's preview, `(p, τ_{p−1})` (engine.md §28.2 step 1).
    pub fn purge_preview(&self) -> Result<(u64, i64), &'static str> {
        unimplemented!()
    }

    /// The sealer (engine.md §37).
    pub fn sealer(&self) -> &S {
        unimplemented!()
    }
}

// Declared for the stage that fills this crate: each dependency is named once
// so that it is resolved and compiled now, and a wrong feature set fails today
// rather than in wave 1.
const _: () = {
    let _ = <sha2::Sha256 as sha2::Digest>::new;
    let _ = zeroize::Zeroizing::<Vec<u8>>::new;
};

#[cfg(test)]
mod tests {
    use proptest as _;
    use serde as _;
    use serde_json as _;

    #[test]
    fn the_shell_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "taskdeck-sync");
    }
}
