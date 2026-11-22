//! The fake sealer of the simulator (engine.md §6.3), compiled only with the
//! feature `sim` (ENG-TOY-002).
//!
//! Not yet (left for the next part of task 4, skeletons part B):
//! - `FakeSealer`, `FakeMembership` and `FakeBundle`: their fields (engine.md
//!   §6.3; `seal.md` §13.1 for the membership).
//! - `FakeSealer::create` and `FakeSealer::join` (engine.md §6.3, `seal.md`
//!   §12.2, §13.3, §17).
//! - Every method of `impl Sealer for FakeSealer` (engine.md §6.1, §6.3 over
//!   `seal.md` §5, §8 to §15 and §20).

use crate::{
    ClassifyInput, Dev, Hash32, Head, MembershipView, PlainEntry, Receipt, Sealed, SealedBlob, Sealer, SealerError,
    SnapInfo, SnapshotMaterial,
};

/// The fake sealer (engine.md §6.3).
pub struct FakeSealer {}

/// The fake's membership state (engine.md §6.3, `seal.md` §13.1).
#[derive(Clone)]
pub struct FakeMembership {}

/// What a pairing hands the new device (engine.md §6.3, §37).
pub struct FakeBundle {}

impl FakeSealer {
    /// The genesis helper: the fake, frame 1 and the board bytes (engine.md §6.3).
    pub fn create(seed: u64, name: &str) -> (Self, Vec<u8>, Vec<u8>) {
        unimplemented!()
    }

    /// The pairing helper (engine.md §6.3).
    pub fn join(seed: u64, name: &str, bundle: FakeBundle) -> Self {
        unimplemented!()
    }
}

impl Sealer for FakeSealer {
    type Membership = FakeMembership;

    fn own_dev(&self) -> Dev {
        unimplemented!()
    }

    fn view(&self, m: &FakeMembership) -> MembershipView {
        unimplemented!()
    }

    fn membership_bytes(&self, m: &FakeMembership) -> Vec<u8> {
        unimplemented!()
    }

    fn membership_from_bytes(&self, bytes: &[u8]) -> Result<FakeMembership, SealerError> {
        unimplemented!()
    }

    fn supported_envelopes(&self) -> Vec<(u8, u8)> {
        unimplemented!()
    }

    fn chain_zero(&self) -> Hash32 {
        unimplemented!()
    }

    fn chain_step(&self, prev: &Hash32, seq: u64, frame: &[u8]) -> Hash32 {
        unimplemented!()
    }

    fn frame_hash(&self, frame: &[u8]) -> Hash32 {
        unimplemented!()
    }

    fn frame_author(&self, frame: &[u8]) -> Result<(Dev, u64), SealerError> {
        unimplemented!()
    }

    fn classify(
        &mut self,
        input: &ClassifyInput<'_, FakeMembership>,
    ) -> Result<Receipt<FakeMembership>, SealerError> {
        unimplemented!()
    }

    fn new_entry_id(&mut self) -> Result<[u8; 16], SealerError> {
        unimplemented!()
    }

    fn seal(&mut self, m: &FakeMembership, ctr: u64, entry: &PlainEntry) -> Result<Sealed, SealerError> {
        unimplemented!()
    }

    fn open_own(&self, m: &FakeMembership, frame: &[u8]) -> Result<[u8; 16], SealerError> {
        unimplemented!()
    }

    fn discard_pending(&mut self, entry_id: &[u8; 16]) -> bool {
        unimplemented!()
    }

    fn forget_roots_below(&mut self, epoch: u32) -> bool {
        unimplemented!()
    }

    fn take_missed_wraps(
        &mut self,
        m: &FakeMembership,
        first_epoch: u32,
        wraps: &[[u8; 96]],
    ) -> Result<bool, SealerError> {
        unimplemented!()
    }

    fn seal_snapshot(&mut self, m: &FakeMembership, material: &SnapshotMaterial) -> Result<SealedBlob, SealerError> {
        unimplemented!()
    }

    fn open_snapshot(
        &self,
        parts: &[&[u8]],
        expected: &SnapInfo,
        writer: &Dev,
        exact: bool,
    ) -> Result<(SnapshotMaterial, u32, u64), SealerError> {
        unimplemented!()
    }

    fn sync_fingerprint(&self, head: &Head) -> String {
        unimplemented!()
    }

    fn move_code(&self, fence_seq: u64, fence_hash: &Hash32) -> String {
        unimplemented!()
    }

    fn check_move_code(&self, fence_seq: u64, fence_hash: &Hash32, typed: &str) -> Result<(), SealerError> {
        unimplemented!()
    }

    fn export_identity(&self) -> zeroize::Zeroizing<Vec<u8>> {
        unimplemented!()
    }
}
