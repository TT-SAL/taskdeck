//! What the user interfaces are told and what they send back
//! (engine.md §14.2, §21.1, §30, §37).

use crate::{Dev, DeviceStatus, HaltKind, Hash32, Head, NoticeKind, SnapInfo, StopKind};

/// The engine's status (engine.md §30.1).
pub struct Status {
    pub session: SessionState,
    pub relay_host: String,
    pub push: bool,
    pub last_synced_ms: Option<i64>,
    pub unreachable_since_ms: Option<i64>,
    pub paused_until_ms: Option<i64>,
    pub queued: u32,
    pub oldest_queued_ms: Option<i64>,
    pub doomed: u32,
    pub queue_large: bool,
    pub head: Head,
    pub board_seq: u64,
    pub fingerprint: String,
    pub clock: ClockStatus,
    pub halt: Option<HaltView>,
    pub stops: Vec<StopView>,
    pub alarms: Vec<AlarmRecord>,
    pub banners: Vec<Banner>,
    pub devices: Vec<DeviceRow>,
    pub rules: RulesStatus,
    pub recovery_change: Option<RecoveryChangeView>,
    pub moving: Option<MovingView>,
    pub ops: Vec<OpStatus>,
    pub notices_unread: u32,
    pub refusals_unread: u32,
    pub warnings: Vec<StatusWarning>,
}

/// The session state shown (engine.md §30.1).
pub enum SessionState {
    Offline,
    Connecting,
    Reconciling,
    Live,
    Rejoining,
}

/// The clock guard shown (engine.md §30.1, §27.2).
pub struct ClockStatus {
    pub skew_ms: Option<i64>,
    pub warning: bool,
    pub blocked: bool,
    pub confirmed_until_ms: Option<i64>,
}

/// A Skip the user may start (engine.md §30.1, §17).
pub struct SkipOffer {
    pub seq: u64,
    pub here: bool,
}

/// The halt shown (engine.md §30.1, §16).
pub struct HaltView {
    pub kind: HaltKind,
    pub seq: u64,
    pub author: Dev,
    pub author_name: String,
    pub author_fp: String,
    pub since_ms: i64,
    pub code: String,
    pub skip_offer: Option<SkipOffer>,
}

/// The action a stop offers (engine.md §30.1).
pub enum StopAction {
    RestoreTail,
    ReHome,
    Adopt,
    RePair,
    ReplaceDevice,
    UseThisCopy,
    Restart,
    ConfirmRejoin,
    Wait,
}

/// A stop shown (engine.md §30.1).
pub struct StopView {
    pub kind: StopKind,
    pub since_ms: i64,
    pub alarm: Option<u64>,
    pub action: StopAction,
}

/// A device row shown (engine.md §30.1).
pub struct DeviceRow {
    pub dev: Dev,
    pub name: String,
    pub key_fp: String,
    pub device_no: u16,
    pub status: DeviceStatus,
    pub is_self: bool,
    pub last_beat_ms: i64,
    pub last_entry_ms: Option<i64>,
    pub rules: Vec<u32>,
    pub bootstrap_seq: u64,
}

/// A device that blocks a rules version (engine.md §30.1, §25.2).
pub struct Blocker {
    pub dev: Dev,
    pub name: String,
    pub key_fp: String,
    pub last_beat_ms: i64,
    pub rules: Vec<u32>,
}

/// One rules version's readiness (engine.md §30.1, §25.2).
pub struct VersionStatus {
    pub version: u32,
    pub blockers: Vec<Blocker>,
    pub override_vote: Option<(u32, Dev, u64)>,
}

/// The rules shown (engine.md §30.1, §25.2).
pub struct RulesStatus {
    pub active: u32,
    pub versions: Vec<VersionStatus>,
}

/// A recovery-key change shown (engine.md §30.1, §26.1).
pub struct RecoveryChangeView {
    pub proposer: Dev,
    pub proposer_name: String,
    pub key_fp: String,
    pub start_ms: i64,
    pub confirm_after_ms: i64,
    pub can_confirm: bool,
}

/// A move in progress shown (engine.md §30.1, §28.3).
pub struct MovingView {
    pub host: String,
    pub home_gen: u32,
    pub fence_seq: u64,
    pub tries_left: u32,
    pub locked_until_ms: Option<i64>,
}

/// An operation in progress shown (engine.md §30.1).
pub struct OpStatus {
    pub ticket: u64,
    pub kind: String,
    pub stage: u8,
    pub started_ms: i64,
}

/// A warning shown until acknowledged (engine.md §30.1).
pub enum StatusWarning {
    RestoredCopy { entries: u32 },
    ContinuityUnchecked { from: u64 },
    BasedOnUnchecked { count: u32 },
}

/// A pending entry's mark (engine.md §37, §8.2).
pub struct PendingMark {
    pub entry_id: [u8; 16],
    pub provisional_seq: u64,
    pub op: &'static str,
    pub target: Option<u64>,
    pub doomed: Option<&'static str>,
    pub acknowledged: bool,
}

/// A notice or refusal record shown (engine.md §37, §13.4).
pub struct NoticeEntry {
    pub kind: NoticeKind,
    pub seq: u64,
    pub index: u32,
    pub record: Vec<u8>,
    pub dismissed: bool,
}

/// A notification to the facade (engine.md §30.3).
pub enum Notification {
    Enqueued { ticket: u64, entry_id: Option<[u8; 16]>, provisional_seq: Option<u64> },
    EnqueueRefused { ticket: u64, code: &'static str },
    ViewChanged { version: u64 },
    StatusChanged,
    NoticesAdded { notices: u32, refusals: u32 },
    BannersChanged,
    AlarmRaised { id: u64 },
    OpProgress { ticket: u64, stage: u8 },
    OpDone { ticket: u64, result: Result<(), &'static str> },
    PairRegistered { ticket: u64, head: Head, snapshot: SnapInfo, added_seq: u64 },
    PairFailed { ticket: u64, code: &'static str },
    MoveCode { text: String },
    MoveReport { carried_through: u64, old_relay_answered: bool },
    MoveCodeResult { result: Result<(), &'static str> },
    Unsent { commands: Vec<Vec<u8>>, from_device_no: u16, origin_state: Vec<u8> },
    ResubmitReport { ticket: u64, queued: u32, refused: Vec<(u32, &'static str)> },
    ReplaceReady { dev: Dev },
    PurgeBackups { before_ms: i64 },
    ReuploadPicture { epoch: u32 },
    RecoveryKeyChanged { key_fp: String },
    ReadyToExit,
}

/// A user's acknowledgement, confirmation or choice (engine.md §30.4).
pub enum UserAction {
    AckBanner { id: (u64, u8) },
    DismissNotice { seq: u64, index: u32 },
    DismissAlarm { id: u64 },
    ConfirmClock,
    MoveCode { text: String },
    ConfirmRejoin,
    UseThisCopy,
    ExportUnsent,
    AckContinuity,
    AckRestoredCopy,
    AckBasedOnUnchecked,
}

/// A banner about an accepted control item (engine.md §14.2).
pub struct Banner {
    pub id: (u64, u8),
    pub kind: BannerKind,
    pub seq: u64,
    pub at_ms: i64,
    pub author: Dev,
    pub author_name: String,
    pub author_fp: String,
    pub subject: Option<Dev>,
    pub subject_name: Option<String>,
    pub subject_fp: Option<String>,
    pub detail: BannerDetail,
    pub acked: bool,
}

/// The kinds of banner (engine.md §14.2).
pub enum BannerKind {
    DeviceAdded,
    DeviceRevoked,
    DeviceReinstated,
    KeysRotated,
    HistoryPurged,
    RecoveryProposed,
    RecoveryConfirmed,
    RecoveryChanged,
    RecoveryCancelled,
    Moved,
    Reset,
    Skipped,
    RulesUpgraded,
    RulesVote,
}

/// A banner's detail (engine.md §14.2).
pub enum BannerDetail {
    None,
    Purge { before_seq: u64 },
    Recovery { key_fp: String, confirm_after_ms: Option<i64> },
    Moved { host: String, home_gen: u32 },
    Reset { target: u64 },
    Skipped { target: u64, target_author: Dev },
    Rules { version: u32 },
}

/// An alarm raised with evidence (engine.md §21.1).
pub struct AlarmRecord {
    pub id: u64,
    pub kind: AlarmKind,
    pub raised_ms: i64,
    pub seq: Option<u64>,
    pub author: Option<Dev>,
    pub code: Option<String>,
    pub evidence: Evidence,
    pub resolved: bool,
}

/// The kinds of alarm (engine.md §21.1, the `AlarmKind` table).
pub enum AlarmKind {
    RelayBehind,
    Fork,
    Counter,
    Unechoed,
    Withheld,
    ForeignSigner,
    IdentityClone,
    StateMismatch,
    SnapshotMismatch,
    ResetMismatch,
    SkippedFrame,
    RefusedFrame,
    RelayProtocol,
    BlobFailure,
    RestoredCopy,
}

/// Where a fork was seen (engine.md §21.1).
pub enum ForkSource {
    Welcome,
    PageHead,
    RepeatedEntry,
    BasedOn,
}

/// An alarm's evidence (engine.md §21.1).
pub enum Evidence {
    RelayBehind { relay_head: Head, relay_last_ctr: Option<u64>, local_head: Head, ack: Option<(u64, Hash32)> },
    Fork { local_chain: Hash32, relay_chain: Hash32, source: ForkSource },
    Counter { ctr: u64, expected: u64 },
    Unechoed { ack_frame_hash: Hash32, ack_chain: Hash32, log_frame_hash: Hash32, log_chain: Hash32 },
    Withheld {
        relay_head: u64,
        claimed_ctr: Option<u64>,
        cursor: u64,
        missing_from: u64,
        missing_to: u64,
        counters: bool,
    },
    ForeignSigner { ctr: u64 },
    IdentityClone { session: Option<[u8; 16]>, ctrs: Vec<u64> },
    Mismatch { groups: Vec<(Hash32, Vec<Dev>)> },
    Frame { frame_hash: Option<Hash32> },
    Request { request: String },
    Blob { blob: [u8; 16] },
    RestoredCopy { entries: u32 },
}
