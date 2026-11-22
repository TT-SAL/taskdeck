//! Frames, envelope, plaintext, kinds, payloads and control items (seal.md §8 to §12, §26).

use crate::{
    BlobId, BoardId, DevId, Entropy, EntryId, FrameHash, Head, RecoveryPublic, SealError, Signer,
    SymKey,
};

/// The kind byte of a plaintext (seal.md §11.1, §26).
pub enum Kind {
    Cmd = 1,
    Ctl = 2,
    Beat = 3,
    Snap = 4,
}

/// The plaintext fields of an entry (seal.md §10.1, §26).
pub struct Plain {
    pub entry_id: EntryId,
    pub kind: Kind,
    pub at_ms: i64,
    pub based_on: Head,
    pub payload: Vec<u8>,
}

/// The padding function `P(U)` (seal.md §10.2, §26).
pub fn padded_len(unpadded: u64) -> u64 {
    unimplemented!()
}

/// Draws a new entry id (seal.md §10.1, §26).
pub fn new_entry_id(rng: &mut dyn Entropy) -> Result<EntryId, SealError> {
    unimplemented!()
}

/// Seals and signs one frame (seal.md §8, §9, §10, §26).
pub fn seal_frame(
    signer: &Signer,
    board: &BoardId,
    ctr: u64,
    epoch: u32,
    key: &SymKey,
    plain: &Plain,
    rng: &mut dyn Entropy,
) -> Result<Vec<u8>, SealError> {
    unimplemented!()
}

/// `H(frame)` (seal.md §8.2, §26).
pub fn frame_hash(frame: &[u8]) -> FrameHash {
    unimplemented!()
}

/// A frame's fixed header fields (seal.md §8.1, §26).
pub struct FrameHeader {
    pub board: BoardId,
    pub dev: DevId,
    pub ctr: u64,
    pub len: u32,
}

/// Parses a frame's header: §14.2 step 1 only (seal.md §26).
pub fn frame_header(frame: &[u8]) -> Result<FrameHeader, SealError> {
    unimplemented!()
}

/// One control item (seal.md §12, §26).
#[allow(clippy::large_enum_variant)] // the spec's variants are unboxed (seal.md §26)
pub enum CtlItem {
    Genesis(Genesis),
    DeviceAdded(DeviceAdded),
    DeviceRevoked(DevId),
    DeviceReinstated(DevId),
    EpochRotated(EpochRotated),
    RecoveryKeyChange(RecoveryKeyChange),
    Fence(Fence),
    ResetTo(ResetTo),
    Skip(SkipItem),
    RulesUpgrade(RulesUpgrade),
}

/// The `Genesis` item (seal.md §12.2).
pub struct Genesis {
    pub salt: [u8; 16],
    pub recovery: RecoveryPublic,
    pub device: GenesisDevice,
    pub epoch: u32,
    pub commit: [u8; 32],
    pub wraps: Vec<[u8; 96]>,
    pub rules: u32,
    pub hash_version: u32,
    pub state_hash: [u8; 32],
    pub snapshot: Option<GenesisSnapshot>,
}

/// The `device` object of `Genesis` (seal.md §12.2).
pub struct GenesisDevice {
    pub dev: DevId,
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
    pub name: String,
}

/// The optional `snapshot` object of `Genesis` (seal.md §12.2).
pub struct GenesisSnapshot {
    pub blob: BlobId,
    pub sha256: [u8; 32],
    pub size: u64,
    pub parts: u32,
    pub inflated: u64,
}

/// The `DeviceAdded` item (seal.md §12.3).
pub struct DeviceAdded {
    pub dev: DevId,
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
    pub name: String,
    pub transcript: [u8; 32],
    pub bootstrap_seq: u64,
}

/// The `EpochRotated` item (seal.md §12.5).
pub struct EpochRotated {
    pub epoch: u32,
    pub commit: [u8; 32],
    pub wraps: Vec<[u8; 96]>,
    pub purge_before: Option<u64>,
}

/// The `RecoveryKeyChange` item (seal.md §12.6).
pub struct RecoveryKeyChange {
    pub mode: RecoveryKeyMode,
    pub sign_pk: Option<[u8; 32]>,
    pub hpke_pk: Option<[u8; 32]>,
}

/// The `mode` of `RecoveryKeyChange` (seal.md §12.6, §13.11).
pub enum RecoveryKeyMode {
    Direct,
    Propose,
    Confirm,
    Cancel,
}

/// The `Fence` item (seal.md §12.7).
pub struct Fence {
    pub host: String,
    pub door: [u8; 5],
    pub home_gen: u32,
}

/// The `ResetTo` item (seal.md §12.8).
pub struct ResetTo {
    pub seq: u64,
    pub state_hash: [u8; 32],
    pub hash_version: u32,
}

/// The `Skip` item (seal.md §12.9).
pub struct SkipItem {
    pub seq: u64,
    pub frame_hash: FrameHash,
}

/// The `RulesUpgrade` item (seal.md §12.10).
pub struct RulesUpgrade {
    pub rules: u32,
    pub mode: RulesUpgradeMode,
}

/// The `mode` of `RulesUpgrade` (seal.md §12.10, §13.10).
pub enum RulesUpgradeMode {
    Normal,
    Override,
}

/// The beat payload (seal.md §11.4, §26).
pub struct Beat {
    pub rules: Vec<u32>,
    pub envelopes: Vec<(u8, u8)>,
    pub hash_versions: Vec<u32>,
    pub state: Option<BeatState>,
    pub acks: Vec<BeatAck>,
}

/// The `state` object of a beat (seal.md §11.4).
pub struct BeatState {
    pub seq: u64,
    pub hash: [u8; 32],
    pub hash_version: u32,
}

/// One `acks` object of a beat (seal.md §11.4).
pub struct BeatAck {
    pub entry: u64,
    pub verdict: AckVerdict,
}

/// The `verdict` of a beat ack (seal.md §11.4).
pub enum AckVerdict {
    Ack,
    Mismatch,
}

/// The snapshot announcement payload (seal.md §11.5, §26).
pub struct SnapAnnounce {
    pub seq: u64,
    pub hash: [u8; 32],
    pub hash_version: u32,
    pub blob: BlobId,
    pub sha256: [u8; 32],
    pub size: u64,
    pub parts: u32,
    pub inflated: u64,
    pub epoch: u32,
}

/// The control payload with its inner signature (seal.md §11.3, §26).
pub fn ctl_payload(
    items: &[CtlItem],
    signer: &Signer,
    board: &BoardId,
    ctr: u64,
    entry_id: &EntryId,
    at_ms: i64,
) -> Result<Vec<u8>, SealError> {
    unimplemented!()
}

/// The beat payload's JSON (seal.md §11.4, §26).
pub fn beat_payload(beat: &Beat) -> Result<Vec<u8>, SealError> {
    unimplemented!()
}

/// The snapshot announcement's JSON (seal.md §11.5, §26).
pub fn snap_payload(snap: &SnapAnnounce) -> Result<Vec<u8>, SealError> {
    unimplemented!()
}
