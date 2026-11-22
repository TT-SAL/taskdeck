//! The two traits the engine is generic over and the types they speak in
//! (engine.md §5.1, §6.1, §6.2). The traits have no bodies; the types are data.

// ---------- the state machine (§5.1)

/// The apply context of an entry (engine.md §5.1, ENG-SM-001).
pub struct Ctx {
    pub seq: u64,
    pub device_no: u16,
    pub at_ms: i64,
    pub based_on: u64,
}

/// A successful apply and the notices it produced (engine.md §5.1).
pub struct Applied<N> {
    pub benign: bool,
    pub notices: Vec<N>,
}

/// A state machine's refusal of a command (engine.md §5.1).
pub struct SmRefusal {
    pub code: &'static str,
    pub op: Option<&'static str>,
    pub target: Option<u64>,
    pub base: Option<u64>,
    pub current: Option<u64>,
}

/// A state machine's error outside apply (engine.md §5.1).
pub struct SmError {
    pub code: &'static str,
}

/// The outcome of coalescing a held create with the next command (engine.md §5.1 `coalesce`).
pub enum Coalesce<C> {
    Merge(C),
    CancelBoth,
    Keep,
}

/// The board's state machine as the engine sees it (engine.md §5.1, decision D6).
pub trait StateMachine {
    /// The board state (engine.md §5.1).
    type State: Clone;
    /// A parsed command (engine.md §5.1).
    type Command: Clone;
    /// A notice produced by apply (engine.md §5.1).
    type Notice: Clone;

    /// The rules versions this build applies (engine.md §5.1 contracts).
    fn supported_rules(&self) -> &[u32];
    /// The state-hash versions this build computes (engine.md §5.1 contracts).
    fn hash_versions(&self) -> &[u32];

    /// Parses canonical state bytes (engine.md §5.1 contracts).
    fn load(&self, bytes: &[u8]) -> Result<Self::State, SmError>;
    /// The canonical state bytes (engine.md §5.1 contracts).
    fn canonical_bytes(&self, state: &Self::State) -> Vec<u8>;
    /// SHA-256 of the canonical bytes (engine.md §5.1 contracts).
    fn board_hash(&self, state: &Self::State) -> [u8; 32];
    /// The state's active rules version (engine.md §5.1 contracts).
    fn rules(&self, state: &Self::State) -> u32;
    /// Activates a newer rules version (engine.md §5.1 contracts, ENG-SM-002).
    fn upgrade(&self, state: &mut Self::State, v: u32) -> Result<(), SmError>;

    /// The strict command parser (engine.md §5.1 contracts, ENG-SM-003).
    fn parse(&self, payload: &[u8]) -> Result<Self::Command, SmError>;
    /// The canonical payload of a command (engine.md §5.1 contracts).
    fn encode(&self, command: &Self::Command) -> Vec<u8>;
    /// The command's op name (engine.md §5.1).
    fn op_name(&self, command: &Self::Command) -> &'static str;
    /// The command's target (engine.md §5.1).
    fn target(&self, command: &Self::Command) -> Option<u64>;

    /// Applies a command to a state (engine.md §5.1 contracts).
    fn apply(
        &self,
        state: &mut Self::State,
        command: &Self::Command,
        ctx: &Ctx,
    ) -> Result<Applied<Self::Notice>, SmRefusal>;
    /// Applies a command to a copy (engine.md §5.1 contracts).
    fn trial_apply(
        &self,
        state: &Self::State,
        command: &Self::Command,
        ctx: &Ctx,
    ) -> Result<Applied<Self::Notice>, SmRefusal>;
    /// The state a `ResetTo` installs (engine.md §5.1 contracts, §28.1).
    fn reset(&self, target: &Self::State, current: &Self::State) -> Result<Self::State, SmError>;

    /// The id a command creates and may hold (engine.md §5.1 contracts, §9.3).
    fn hold(&self, command: &Self::Command) -> Option<u64>;
    /// Coalesces a held create with the next command (engine.md §5.1 contracts, §9.3).
    fn coalesce(&self, held: &Self::Command, next: &Self::Command) -> Coalesce<Self::Command>;
    /// Rewrites another identity's commands for this one (engine.md §5.1 contracts, §24.5).
    fn rebind(
        &self,
        commands: &[Self::Command],
        from_device_no: u16,
        to_device_no: u16,
        view: &Self::State,
        origin_state: &[u8],
    ) -> Result<(Vec<Self::Command>, Vec<u8>), SmError>;

    /// Whether a device keeps a notice (engine.md §5.1 contracts).
    fn notice_concerns(&self, notice: &Self::Notice, device_no: u16) -> bool;
    /// The canonical bytes of a notice record (engine.md §5.1 contracts).
    fn notice_bytes(&self, notice: &Self::Notice) -> Vec<u8>;
    /// The canonical bytes of a refusal record (engine.md §5.1 contracts).
    fn refusal_bytes(&self, seq: u64, author: u16, refusal: &SmRefusal) -> Vec<u8>;
}

// ---------- the sealer (§6.1)

/// A device id (engine.md §6.1).
pub type Dev = [u8; 16];
/// A 32-byte hash (engine.md §6.1).
pub type Hash32 = [u8; 32];

/// A log position and its chain value (engine.md §6.1).
pub struct Head {
    pub seq: u64,
    pub chain: Hash32,
}

/// An entry's kind (engine.md §6.1).
pub enum Kind {
    Cmd = 1,
    Ctl = 2,
    Beat = 3,
    Snap = 4,
}

/// The sealer's classification of a frame (engine.md §6.1, `seal.md` §14.4).
pub enum Class {
    Skip,
    Tamper,
    Refuse,
    Accept,
    NeedFrames(Vec<(u64, Hash32)>),
}

/// A halt the sealer reports (engine.md §6.1, §16).
pub enum Halt {
    Commitment,
    UnknownRules(u32),
}

/// A snapshot verdict (engine.md §6.1, §19.5).
pub enum Verdict {
    Ack,
    Mismatch,
}

/// The fields of a beat (engine.md §6.1, §23.2).
pub struct BeatInfo {
    pub rules: Vec<u32>,
    pub envelopes: Vec<(u8, u8)>,
    pub hash_versions: Vec<u32>,
    pub state: Option<(u64, Hash32, u32)>,
    pub acks: Vec<(u64, Verdict)>,
}

/// The fields of a `snap` entry (engine.md §6.1, §19.4).
pub struct SnapInfo {
    pub seq: u64,
    pub hash: Hash32,
    pub hash_version: u32,
    pub blob: [u8; 16],
    pub sha256: Hash32,
    pub size: u64,
    pub parts: u32,
    pub inflated: u64,
    pub epoch: u32,
}

/// The mode of a recovery-key change (engine.md §6.1).
pub enum RecoveryMode {
    Direct,
    Propose,
    Confirm,
    Cancel,
}

/// The mode of a rules upgrade (engine.md §6.1, §25).
pub enum UpgradeMode {
    Normal,
    Override,
}

/// An opened control item (engine.md §6.1, `seal.md` §11).
pub enum CtlItem {
    Genesis { hash_version: u32, state_hash: Hash32 },
    DeviceAdded { dev: Dev, name: String, key_fp: String, bootstrap_seq: u64 },
    DeviceRevoked { dev: Dev },
    DeviceReinstated { dev: Dev },
    EpochRotated { epoch: u32, purge_before: Option<u64> },
    RecoveryKeyChange { mode: RecoveryMode, key_fp: Option<String> },
    Fence { host: String, door: [u8; 5], home_gen: u32 },
    ResetTo { seq: u64, state_hash: Hash32, hash_version: u32 },
    Skip { seq: u64, frame_hash: Hash32 },
    RulesUpgrade { rules: u32, mode: UpgradeMode },
}

/// A control entry the engine asks the sealer to compose (engine.md §6.1).
pub enum CtlIntent {
    Rotate { revoke: Vec<Dev>, reinstate: Vec<Dev>, purge_before: Option<u64> },
    AddDevice {
        dev: Dev,
        sign_pk: Hash32,
        hpke_pk: Hash32,
        name: String,
        transcript: Hash32,
        bootstrap_seq: u64,
    },
    RecoveryChange { mode: RecoveryMode, sign_pk: Option<Hash32>, hpke_pk: Option<Hash32> },
    Fence { host: String, door: [u8; 5], home_gen: u32 },
    ResetTo { seq: u64, state_hash: Hash32, hash_version: u32 },
    Skip { targets: Vec<(u64, Hash32)> },
    RulesUpgrade { rules: u32, mode: UpgradeMode },
}

/// The body of an entry to seal (engine.md §6.1).
pub enum Body {
    Cmd(Vec<u8>),
    Ctl(CtlIntent),
    Beat(BeatInfo),
    Snap(SnapInfo),
}

/// An entry before sealing (engine.md §6.1).
pub struct PlainEntry {
    pub entry_id: [u8; 16],
    pub at_ms: i64,
    pub based_on: Head,
    pub body: Body,
}

/// The opened plaintext of an accepted frame (engine.md §6.1).
pub struct Opened {
    pub entry_id: [u8; 16],
    pub kind: Kind,
    pub at_ms: i64,
    pub based_on: Head,
    pub payload: Vec<u8>,
    pub items: Vec<CtlItem>,
    pub beat: Option<BeatInfo>,
    pub snap: Option<SnapInfo>,
}

/// The outcome of classifying one frame (engine.md §6.1, `seal.md` §14.4).
pub struct Receipt<M> {
    pub class: Class,
    pub code: Option<&'static str>,
    pub chain: Hash32,
    pub author: Dev,
    pub ctr: u64,
    pub frame_hash: Hash32,
    pub own_dev_foreign: bool,
    pub based_on_unchecked: bool,
    pub opened: Option<Opened>,
    pub membership: M,
    pub keys_changed: bool,
    pub halt: Option<Halt>,
}

/// The inputs of one classification (engine.md §6.1, `seal.md` §14.1).
pub struct ClassifyInput<'a, M> {
    pub seq: u64,
    pub frame: &'a [u8],
    pub prev_chain: Hash32,
    pub membership: &'a M,
    pub chain_at: &'a dyn Fn(u64) -> Option<Hash32>,
    pub retained_from: u64,
    pub own_sealed: &'a dyn Fn(u64) -> Option<Hash32>,
    pub skip_targets: &'a dyn Fn(u64) -> Option<&'a [u8]>,
}

/// A sealed frame (engine.md §6.1 `seal`).
pub struct Sealed {
    pub frame: Vec<u8>,
    pub frame_hash: Hash32,
    pub epoch: u32,
    pub keys_changed: bool,
}

/// What a snapshot carries (engine.md §6.1, §19).
pub struct SnapshotMaterial {
    pub seq: u64,
    pub chain: Hash32,
    pub hash_version: u32,
    pub state_hash: Hash32,
    pub board: Vec<u8>,
    pub membership: Vec<u8>,
}

/// A sealed snapshot blob (engine.md §6.1 `seal_snapshot`).
pub struct SealedBlob {
    pub id: [u8; 16],
    pub bytes: Vec<u8>,
    pub sha256: Hash32,
    pub parts: u32,
    pub inflated: u64,
    pub epoch: u32,
}

/// A sealer error (engine.md §6.1).
pub struct SealerError {
    pub code: &'static str,
}

/// The frame and membership layer as the engine sees it (engine.md §6.1, decision D6).
pub trait Sealer {
    /// A membership state (engine.md §6.1).
    type Membership: Clone;

    /// This device's id (engine.md §6.1 contracts).
    fn own_dev(&self) -> Dev;
    /// The read-only membership view (engine.md §6.1 contracts, §6.2).
    fn view(&self, m: &Self::Membership) -> MembershipView;
    /// The canonical membership bytes (engine.md §6.1 contracts).
    fn membership_bytes(&self, m: &Self::Membership) -> Vec<u8>;
    /// The strict membership parser (engine.md §6.1 contracts).
    fn membership_from_bytes(&self, bytes: &[u8]) -> Result<Self::Membership, SealerError>;
    /// The envelopes this build opens (engine.md §6.1 contracts).
    fn supported_envelopes(&self) -> Vec<(u8, u8)>;

    /// The chain's starting value (engine.md §6.1 contracts).
    fn chain_zero(&self) -> Hash32;
    /// One chain step (engine.md §6.1 contracts).
    fn chain_step(&self, prev: &Hash32, seq: u64, frame: &[u8]) -> Hash32;
    /// A frame's hash (engine.md §6.1 contracts).
    fn frame_hash(&self, frame: &[u8]) -> Hash32;
    /// A frame header's author and counter (engine.md §6.1 contracts).
    fn frame_author(&self, frame: &[u8]) -> Result<(Dev, u64), SealerError>;

    /// Classifies one frame (engine.md §6.1 contracts, `seal.md` §14).
    fn classify(
        &mut self,
        input: &ClassifyInput<'_, Self::Membership>,
    ) -> Result<Receipt<Self::Membership>, SealerError>;

    /// A fresh entry id (engine.md §6.1 contracts).
    fn new_entry_id(&mut self) -> Result<[u8; 16], SealerError>;
    /// Seals an entry (engine.md §6.1 contracts).
    fn seal(&mut self, m: &Self::Membership, ctr: u64, entry: &PlainEntry) -> Result<Sealed, SealerError>;
    /// Opens a frame this device signed (engine.md §6.1 contracts).
    fn open_own(&self, m: &Self::Membership, frame: &[u8]) -> Result<[u8; 16], SealerError>;
    /// Forgets a refused rotation's pending root (engine.md §6.1 contracts).
    fn discard_pending(&mut self, entry_id: &[u8; 16]) -> bool;
    /// Deletes the epoch roots below an epoch (engine.md §6.1 contracts, §28.2).
    fn forget_roots_below(&mut self, epoch: u32) -> bool;
    /// Keeps the missed wraps of a login answer (engine.md §6.1 contracts, §24.4).
    fn take_missed_wraps(
        &mut self,
        m: &Self::Membership,
        first_epoch: u32,
        wraps: &[[u8; 96]],
    ) -> Result<bool, SealerError>;

    /// Seals a snapshot (engine.md §6.1 contracts, `seal.md` §20).
    fn seal_snapshot(&mut self, m: &Self::Membership, material: &SnapshotMaterial)
    -> Result<SealedBlob, SealerError>;
    /// Opens a snapshot (engine.md §6.1 contracts, `seal.md` §20.5).
    fn open_snapshot(
        &self,
        parts: &[&[u8]],
        expected: &SnapInfo,
        writer: &Dev,
        exact: bool,
    ) -> Result<(SnapshotMaterial, u32, u64), SealerError>;

    /// The sync fingerprint text (engine.md §6.1 contracts, `seal.md` §15.3).
    fn sync_fingerprint(&self, head: &Head) -> String;
    /// The move code text (engine.md §6.1 contracts, `seal.md` §15.4).
    fn move_code(&self, fence_seq: u64, fence_hash: &Hash32) -> String;
    /// Checks a typed move code (engine.md §6.1 contracts, `seal.md` §15.4).
    fn check_move_code(&self, fence_seq: u64, fence_hash: &Hash32, typed: &str) -> Result<(), SealerError>;
    /// The sealed identity file's bytes (engine.md §6.1 contracts, §7.11).
    fn export_identity(&self) -> zeroize::Zeroizing<Vec<u8>>;
}

// ---------- the membership view (§6.2)

/// A device's status (engine.md §6.2).
pub enum DeviceStatus {
    Current,
    Revoked,
}

/// One device of the membership view (engine.md §6.2).
pub struct DeviceView {
    pub dev: Dev,
    pub sign_pk: Hash32,
    pub device_no: u16,
    pub status: DeviceStatus,
    pub name: String,
    pub key_fp: String,
    pub added_seq: u64,
    pub added_by: Dev,
    pub bootstrap_seq: u64,
    pub status_seq: u64,
    pub status_by: Dev,
    pub last_ctr: u64,
    pub beat_ms: i64,
    pub rules: Vec<u32>,
    pub envelopes: Vec<(u8, u8)>,
    pub hash_versions: Vec<u32>,
}

/// One recovery identity of the membership view (engine.md §6.2).
pub struct RecoveryView {
    pub dev_r: Dev,
    pub sign_pk: Hash32,
    pub hpke_pk: Hash32,
    pub since_seq: u64,
    pub until_seq: u64,
    pub last_ctr: u64,
}

/// A pending recovery-key proposal (engine.md §6.2).
pub struct PendingRecovery {
    pub proposer: Dev,
    pub sign_pk: Hash32,
    pub hpke_pk: Hash32,
    pub key_fp: String,
    pub seq: u64,
    pub start_ms: i64,
}

/// The read-only membership view (engine.md §6.2, `seal.md` §13.1).
pub struct MembershipView {
    pub board_id: [u8; 16],
    pub rules: u32,
    pub epoch: u32,
    pub epoch_seq: u64,
    pub epoch_clock_ms: i64,
    pub clock_ms: i64,
    pub purge_seq: u64,
    pub home_gen: u32,
    pub home_seq: u64,
    pub home_host: String,
    pub home_door: [u8; 5],
    pub devices: Vec<DeviceView>,
    pub recovery_dev: Dev,
    pub recovery_fp: String,
    pub recovery: Vec<RecoveryView>,
    pub pending_recovery: Option<PendingRecovery>,
    pub pending_override: Option<(u32, Dev, u64)>,
    pub last_reset: Option<(u64, u64)>,
    pub skips: Vec<(u64, Hash32)>,
    pub ctl_seqs: Vec<u64>,
}
