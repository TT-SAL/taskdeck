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
    Kind, MembershipView, Opened, PlainEntry, Receipt, RecoveryView, Sealed,
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
    HaltView, MovingView, NoticeEntry, Notification, OpStatus, PendingMark, RulesStatus,
    SessionState, SkipOffer, Status, StatusWarning, StopAction, StopView, UserAction, VersionStatus,
};

// ---------- records (§7)
pub use records::{
    Bootstrap, Checkpoint, DutyState, HaltKind, HaltRecord, HashRecord, LastEntry, Loaded, LogRecord, Marker, Meta,
    NoticeKind, NoticeOp, OutboxOp, OutboxRecord, PersistOp, Registry, Report, SnapRecord, StopKind, StopRecord, Via,
};

/// The state hash of a board hash and membership bytes (engine.md §18.3, ruling S7).
pub fn state_hash(hash_version: u32, board_hash: &Hash32, membership_bytes: &[u8]) -> Hash32 {
    use sha2::{Digest, Sha256};
    // The 86 bytes of §18.3 (ENG-HASH-001): ctx("state"), the version, the board hash, the membership hash.
    let mut input = [0u8; 86];
    input[..18].copy_from_slice(b"taskdeck/v1/state\0");
    input[18..22].copy_from_slice(&hash_version.to_be_bytes());
    input[22..54].copy_from_slice(board_hash);
    let mh: [u8; 32] = Sha256::digest(membership_bytes).into();
    input[54..].copy_from_slice(&mh);
    Sha256::digest(input).into()
}

mod engine;
pub use engine::{Config, Engine};

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
