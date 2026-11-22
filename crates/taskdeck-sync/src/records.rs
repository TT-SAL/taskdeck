//! The records the engine persists and reads back at `Start` (engine.md §7, §37).

use crate::{
    AlarmRecord, Banner, Body, Dev, Hash32, Head, Kind, OpRecord, RelayTarget, SnapInfo, SnapshotMaterial, Verdict,
};

/// One log record (engine.md §7.2).
pub struct LogRecord {
    pub seq: u64,
    pub chain: Hash32,
    pub tau: i64,
    pub frame: Vec<u8>,
}

/// The checkpoint, and a base (engine.md §7.3, §7.9).
pub struct Checkpoint {
    pub board_seq: u64,
    pub board_chain: Hash32,
    pub board: Vec<u8>,
    pub membership: Vec<u8>,
    pub log_floor: u64,
    pub registry: Registry,
}

/// Engine data the log alone does not give (engine.md §7.3).
pub struct Registry {
    pub snapshots: Vec<SnapRecord>,
    pub reports: Vec<Report>,
    pub pending_verdicts: Vec<(u64, Verdict)>,
    pub last_entry: Vec<LastEntry>,
    pub restored: Option<(u64, u64)>,
}

/// An accepted `snap` entry and its verdicts (engine.md §7.3).
pub struct SnapRecord {
    pub entry_seq: u64,
    pub author: Dev,
    pub info: SnapInfo,
    pub entry_clock: i64,
    pub verdicts: Vec<(Dev, Verdict, u64)>,
    pub own: Option<Verdict>,
}

/// A state-hash report (engine.md §7.3, §22.1).
pub struct Report {
    pub seq: u64,
    pub dev: Dev,
    pub hash: Hash32,
    pub hash_version: u32,
    pub report_seq: u64,
}

/// An author's newest accepted entry (engine.md §7.3).
pub struct LastEntry {
    pub dev: Dev,
    pub at_ms: i64,
    pub seq: u64,
    pub based_on_seq: u64,
}

/// How an outbox record reaches the relay (engine.md §7.4, §11.7).
pub enum Via {
    Append,
    Register { dev: Dev, sign_pk: Hash32 },
    Fence { host: String, door: [u8; 5], home_gen: u32 },
}

/// One outbox record (engine.md §7.4).
pub struct OutboxRecord {
    pub entry_id: [u8; 16],
    pub order: u64,
    pub at_ms: i64,
    pub based_on: Head,
    pub kind: Kind,
    pub body: Body,
    pub origin_state: Vec<u8>,
    pub via: Via,
    pub hold: bool,
    pub ctr: Option<u64>,
    pub frame: Option<Vec<u8>>,
    pub frame_hash: Option<Hash32>,
    pub epoch: Option<u32>,
    pub ack: Option<(u64, Hash32)>,
    pub seen: Option<u64>,
    pub op: Option<u64>,
    pub reseals: u32,
}

/// One outbox file operation (engine.md §7.4).
// The spec fixes the variants; `Put` carries the whole record by value.
#[allow(clippy::large_enum_variant)]
pub enum OutboxOp {
    Put(OutboxRecord),
    Drop([u8; 16]),
}

/// The state hash at an aligned seq (engine.md §7.5).
pub struct HashRecord {
    pub seq: u64,
    pub hash_version: u32,
    pub hash: Hash32,
}

/// The apply marker (engine.md §7.6, §15).
pub struct Marker {
    pub first: u64,
    pub last: u64,
    pub attempt: u64,
}

/// The kind of a notice record (engine.md §7.7, §37).
pub enum NoticeKind {
    Notice,
    Refusal,
}

/// One notices file operation (engine.md §7.7).
pub enum NoticeOp {
    Add { kind: NoticeKind, seq: u64, record: Vec<u8> },
    Dismiss { seq: u64, index: u32 },
}

/// The halt in force (engine.md §7.8 `halt`, §16).
pub struct HaltRecord {
    pub kind: HaltKind,
    pub seq: u64,
    pub author: Dev,
    pub since_ms: i64,
    pub code: String,
}

/// The three halts (engine.md §16.1, §37).
pub enum HaltKind {
    Crash { frame_hash: Hash32 },
    Rules { version: u32 },
    Commitment,
}

/// A stop in force (engine.md §7.8 `stops`, §21, §22).
pub struct StopRecord {
    pub kind: StopKind,
    pub since_ms: i64,
    pub alarm: Option<u64>,
}

/// The kinds of stop (engine.md §21, §22, §37).
pub enum StopKind {
    Evidence,
    Divergence,
    ForeignSigner,
    IdentityClone,
    Revoked { by_log: bool },
    LocalFault { code: String },
    Stranded,
    Rebootstrap { after: u64 },
    Closed,
    Rejoin,
}

/// The duties' persisted state (engine.md §7.8 `duties`, §29.3).
pub struct DutyState {
    pub snapshot: Option<(u64, i64, u8)>,
    pub last_compact_day: i64,
    pub purge: Option<(u64, u32)>,
    pub upgrade: Option<(u32, i64)>,
}

/// The engine's meta record (engine.md §7.8).
pub struct Meta {
    pub attempt: u64,
    pub last_failed: Option<(u64, u64)>,
    pub build_id: String,
    pub halt: Option<HaltRecord>,
    pub stops: Vec<StopRecord>,
    pub alarms: Vec<AlarmRecord>,
    pub banners: Vec<Banner>,
    pub relay: RelayTarget,
    pub prev_session: Option<[u8; 16]>,
    pub conflicts: u32,
    pub clock: Option<(i64, i64)>,
    pub last_beat_ms: i64,
    pub duties: DutyState,
    pub ops: Vec<OpRecord>,
    pub move_tries: (u32, i64),
    pub continuity_unchecked: Option<u64>,
    pub restored_copy: Option<u32>,
    pub based_on_unchecked: u32,
}

/// One persist operation (engine.md §7.11).
// The spec fixes the variants; `WriteMeta` carries the whole `Meta` by value.
#[allow(clippy::large_enum_variant)]
pub enum PersistOp {
    AppendLog(Vec<LogRecord>),
    SetMarker(Marker),
    ClearMarker,
    WriteCheckpoint(Checkpoint),
    WriteBase(Checkpoint),
    DeleteBases(Vec<u64>),
    TruncateLogBelow(u64, Option<Hash32>),
    AppendHashes(Vec<HashRecord>),
    Outbox(Vec<OutboxOp>),
    RewriteOutbox(Vec<OutboxRecord>),
    Notices(Vec<NoticeOp>),
    RewriteNotices(Vec<NoticeOp>),
    WriteMeta(Meta),
    WriteIdentity(zeroize::Zeroizing<Vec<u8>>),
}

/// How a new replica starts (engine.md §7.10 `bootstrap`, §19.8).
pub enum Bootstrap {
    Genesis { board: Vec<u8>, frame_1: Vec<u8>, relay: RelayTarget },
    Join { material: SnapshotMaterial, added_seq: u64, relay: RelayTarget },
}

/// What `Start` carries (engine.md §7.10).
pub struct Loaded {
    pub checkpoint: Option<Checkpoint>,
    pub bases: Vec<(u64, Hash32)>,
    pub chains: Vec<(u64, Hash32)>,
    pub log_floor: u64,
    pub floor_chain: Option<Hash32>,
    pub tail: Vec<LogRecord>,
    pub hashes: Vec<HashRecord>,
    pub outbox: Vec<OutboxOp>,
    pub notices: Vec<NoticeOp>,
    pub meta: Option<Meta>,
    pub marker: Option<Marker>,
    pub build_id: String,
    pub session: [u8; 16],
    pub bootstrap: Option<Bootstrap>,
}
