//! Pairing: the code and the two sans-IO machines (seal.md §17, §26).

use crate::{
    BlobId, BoardId, Chain, DevId, DeviceAdded, DeviceKeys, EpochRoot, Head, Membership,
    SnapAnnounce, SnapshotBody,
};

/// Draws a pairing secret and returns it with its code text (seal.md §17.2, §26).
pub fn new_code(rng: &mut dyn super::Entropy) -> Result<(PairSecret, String), super::SealError> {
    unimplemented!()
}

/// Parses a typed pairing code (seal.md §17.2, §26).
pub fn parse_code(text: &str) -> Result<PairSecret, super::SealError> {
    unimplemented!()
}

/// The 80-bit pairing secret `s` (seal.md §17.2, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct PairSecret {
    s: [u8; 10],
}

/// The states of E's machine (seal.md §17.10).
enum ExistingState {
    Idle,
    Posted,
    Showing,
    Registering,
    Done,
    Aborted,
}

/// The states of N's machine (seal.md §17.10).
enum NewState {
    Idle,
    AwaitA,
    AwaitC,
    AwaitSas,
    AwaitD,
    AwaitSnapshot,
    Joining,
    Done,
    Aborted,
}

/// E's machine (seal.md §17.10, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct Existing {
    #[zeroize(skip)]
    state: ExistingState,
    #[zeroize(skip)]
    start_ms: u64,
    #[zeroize(skip)]
    board: BoardId,
    tid: [u8; 16],
    s: [u8; 10],
    esk: [u8; 32],
    r: [u8; 32],
    mac_key: [u8; 32],
    dh: [u8; 32],
    transcript: [u8; 32],
}

/// N's machine (seal.md §17.10, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct New {
    #[zeroize(skip)]
    state: NewState,
    #[zeroize(skip)]
    start_ms: u64,
    #[zeroize(skip)]
    board: BoardId,
    tid: [u8; 16],
    s: [u8; 10],
    ticket_key: [u8; 32],
    mac_key: [u8; 32],
    esk: [u8; 32],
    dh: [u8; 32],
    transcript: [u8; 32],
    sas: [u8; 8],
    sas_tries: u8,
    keys: Option<DeviceKeys>,
}

/// An event for E's machine (seal.md §17.10, §26).
#[allow(clippy::large_enum_variant)] // `Registered` is the spec's (seal.md §17.10)
pub enum ExistingEvent {
    SlotB {
        bytes: Vec<u8>,
    },
    UserConfirmed,
    Registered {
        h: u64,
        chain: Chain,
        snapshot: SnapAnnounce,
        added_seq: u64,
        token: String,
        roots: Vec<(u32, EpochRoot)>,
        host: String,
        door: [u8; 5],
    },
    RegisterFailed {
        code: super::SealError,
    },
    Cancel,
    Tick {
        now: u64,
    },
}

/// An effect of E's machine (seal.md §17.10, §26).
pub enum ExistingEffect {
    RegisterTicket { tid: [u8; 16], slot_a: Vec<u8> },
    ShowCode { code: String },
    PostSlot { slot: Vec<u8> },
    ShowSas { digits: String },
    NeedSnapshotAndRegister { device: DeviceAdded },
    Done { dev: DevId },
    DeleteTicket { tid: [u8; 16] },
    Abort { code: super::SealError },
}

/// An event for N's machine (seal.md §17.10, §26).
pub enum NewEvent {
    CodeEntered {
        code: String,
        host: String,
        board_id: BoardId,
        door: [u8; 5],
        name: String,
        now: u64,
    },
    Slot {
        bytes: Vec<u8>,
    },
    SasEntered {
        digits: String,
    },
    Snapshot {
        parts: Vec<Vec<u8>>,
    },
    Entries {
        frames: Vec<(u64, Vec<u8>)>,
    },
    Frames {
        frames: Vec<(u64, Vec<u8>)>,
    },
    Cancel,
    Tick {
        now: u64,
    },
}

/// An effect of N's machine (seal.md §17.10, §26).
#[allow(clippy::large_enum_variant)] // `Joined` is the spec's (seal.md §17.10)
pub enum NewEffect {
    FetchSlot {
        tid: [u8; 16],
        ticket_key: zeroize::Zeroizing<[u8; 32]>,
        label: u8,
    },
    AskAgain {
        code: super::SealError,
    },
    PostSlot {
        slot: Vec<u8>,
    },
    AskSas,
    ShowAccepted,
    FetchSnapshot {
        token: String,
        blob: BlobId,
        parts: u32,
    },
    ReadLog {
        token: String,
        from: u64,
    },
    FetchFrames {
        token: String,
        seqs: Vec<u64>,
    },
    Joined {
        device_no: u16,
        keys: DeviceKeys,
        bundle: Bundle,
        snapshot: SnapshotBody,
    },
    Abort {
        code: super::SealError,
    },
}

/// The pairing bundle of slot D (seal.md §17.8).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct Bundle {
    #[zeroize(skip)]
    pub board_id: BoardId,
    pub host: String,
    pub door: [u8; 5],
    /// Each root erases itself on drop.
    #[zeroize(skip)]
    pub roots: Vec<(u32, EpochRoot)>,
    #[zeroize(skip)]
    pub head: Head,
    #[zeroize(skip)]
    pub snapshot: SnapAnnounce,
    pub added_seq: u64,
    pub token: String,
}

impl Existing {
    /// Handles `Start`: draws `s`, `esk_E`, `r_E` and posts slot A (seal.md §17.10).
    pub fn start(
        membership: &Membership,
        head: Head,
        relay_origin: &str,
        own: &DevId,
        now: u64,
        rng: &mut dyn super::Entropy,
    ) -> (Self, Vec<ExistingEffect>) {
        unimplemented!()
    }

    /// Handles one event (seal.md §17.10).
    pub fn on(&mut self, ev: ExistingEvent, rng: &mut dyn super::Entropy) -> Vec<ExistingEffect> {
        unimplemented!()
    }
}

// The spec's API (seal.md §26) names the type `New` and its constructor `New::new()`, with no `Default`.
#[allow(clippy::new_without_default, clippy::self_named_constructors)]
impl New {
    /// N's machine in `Idle`; it draws nothing until `Slot{A}` (seal.md §17.10).
    pub fn new() -> Self {
        unimplemented!()
    }

    /// Handles one event (seal.md §17.10).
    pub fn on(&mut self, ev: NewEvent, rng: &mut dyn super::Entropy) -> Vec<NewEffect> {
        unimplemented!()
    }
}
