//! Receipt checks (seal.md §14, §26).

use crate::{
    Beat, Chain, CtlItem, DeviceKeys, EntryId, EpochRoot, FrameHash, GenesisAnchor, Head, Kind,
    Membership, SealError, SnapAnnounce,
};

/// The inputs of one classification (seal.md §14.1, §26).
pub struct ReceiptInput<'a> {
    pub seq: u64,
    pub frame: &'a [u8],
    pub prev_chain: Chain,
    /// `M_{s−1}`.
    pub membership: &'a Membership,
    pub roots: &'a dyn Fn(u32) -> Option<&'a EpochRoot>,
    /// `C_b` for `b ≥ retained_from`.
    pub chain_at: &'a dyn Fn(u64) -> Option<Chain>,
    pub retained_from: u64,
    /// `None` for read-only readers.
    pub own: Option<OwnIdentity<'a>>,
    pub supported_rules: &'a [u32],
    /// `Some` only for seq 1.
    pub genesis: Option<&'a GenesisAnchor>,
    pub skip_targets: &'a dyn Fn(u64) -> Option<&'a [u8]>,
}

/// This device's keys and its own frames' hashes (seal.md §14.1, §14.7, §26).
pub struct OwnIdentity<'a> {
    pub keys: &'a DeviceKeys,
    pub sealed: &'a dyn Fn(u64) -> Option<FrameHash>,
}

/// A receipt class (seal.md §14.4, §26).
pub enum Class {
    Skip,
    Tamper,
    Refuse,
    Accept,
    NeedFrames(Vec<(u64, FrameHash)>),
}

/// A halt (seal.md §14.5, §26).
pub enum Halt {
    Commitment,
    UnknownRules(u32),
}

/// The result of a classification (seal.md §14, §26).
pub struct Receipt {
    pub class: Class,
    pub code: Option<&'static str>,
    /// `C_s`.
    pub chain: Chain,
    pub own_dev_foreign: bool,
    pub based_on_unchecked: bool,
    pub opened: Option<Opened>,
    /// `M_s` (equal to `M_{s−1}` when unchanged).
    pub membership: Membership,
    pub learned_root: Option<(u32, EpochRoot)>,
    pub halt: Option<Halt>,
}

/// An opened entry (seal.md §14.2, §26).
pub struct Opened {
    pub entry_id: EntryId,
    pub kind: Kind,
    pub at_ms: i64,
    pub based_on: Head,
    pub payload: Vec<u8>,
    pub items: Option<Vec<CtlItem>>,
    pub beat: Option<Beat>,
    pub snap: Option<SnapAnnounce>,
}

/// Classifies one sequenced frame (seal.md §14.2, §26).
pub fn classify(input: &ReceiptInput) -> Result<Receipt, SealError> {
    unimplemented!()
}
