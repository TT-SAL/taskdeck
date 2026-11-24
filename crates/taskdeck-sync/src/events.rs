//! What the runtime hands the engine and what the engine hands back
//! (engine.md §4.2, §4.3, §37).

use crate::{
    AdminOk, AdminRequest, BlobOk, BlobRequest, Checkpoint, Dev, Hash32, LogRecord, Loaded, Notification,
    Operation, Page, PersistOp, RelayRefusal, UserAction, Welcome,
};

/// One input with the runtime's clocks (engine.md §4.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input<C> {
    pub wall_ms: i64,
    pub mono_ms: u64,
    pub event: Event<C>,
}

/// An event the runtime sends (engine.md §4.2).
// The spec fixes the variants; `Start` carries the whole `Loaded` by value.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event<C> {
    Start(Loaded),
    Shutdown,
    Tick,
    Persisted { id: u64 },
    PersistFailed { id: u64 },
    LocalRead { req: u64, result: LocalReadResult },
    Enqueue { ticket: u64, command: C, at_ms: i64, origin_state: Vec<u8>, hold: bool },
    ReleaseHold,
    Op { ticket: u64, op: Operation },
    PairRegister { ticket: u64, device: NewDevice },
    Resubmit { ticket: u64, commands: Vec<C>, from_device_no: u16, origin_state: Vec<u8> },
    User(UserAction),
    Connected { push: bool },
    MissedRoots { first_epoch: u32, wraps: Vec<[u8; 96]> },
    Welcome(Welcome),
    Page { req: Option<u64>, page: Page },
    AppendResults { req: u64, results: Vec<AppendResult> },
    AdminResult { req: u64, result: Result<AdminOk, RelayRefusal> },
    BlobResult { req: u64, result: Result<BlobOk, RelayRefusal> },
    RelayRefused { req: Option<u64>, refusal: RelayRefusal },
    Second { session: [u8; 16] },
    Displaced { session: [u8; 16] },
    Disconnected { unreachable: bool, paused_until_ms: Option<i64> },
}

/// The answer to a `ReadLog` or `ReadBase` (engine.md §4.3, §37).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalReadResult {
    Records(Vec<LogRecord>),
    Base(Checkpoint),
    Missing { code: &'static str },
}

/// The new device of a pairing (engine.md §4.2 `PairRegister`, §28.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewDevice {
    pub dev: Dev,
    pub sign_pk: Hash32,
    pub hpke_pk: Hash32,
    pub name: String,
    pub transcript: Hash32,
}

/// The relay's answer for one appended frame (engine.md §10.4, §37).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppendResult {
    New { seq: u64, chain: Hash32 },
    Dup { seq: u64, chain: Hash32 },
    Conflict { seq: u64, frame: Vec<u8> },
    Gap { last_ctr: u64, last_seq: u64 },
    Refused { status: u16, code: String },
    Skipped,
}

/// An effect the runtime performs (engine.md §4.3).
#[derive(Clone, Debug)]
#[cfg_attr(any(test, feature = "sim"), derive(PartialEq, Eq))]
pub enum Effect {
    Persist { id: u64, ops: Vec<PersistOp> },
    ReadLog { req: u64, from: u64, to: u64 },
    ReadBase { req: u64, seq: u64 },
    Connect { relay: RelayTarget },
    Disconnect,
    Hello,
    Read { req: u64, after: u64, limit: u16 },
    Append { req: u64, frames: Vec<Vec<u8>> },
    Admin { req: u64, op: AdminRequest },
    Blob { req: u64, op: BlobRequest },
    ScheduleTick { at_mono_ms: u64 },
    Notify(Notification),
}

/// Where a device syncs (engine.md §7.8 `relay`, §37).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayTarget {
    pub host: String,
    pub door: [u8; 5],
    pub home_gen: u32,
}
