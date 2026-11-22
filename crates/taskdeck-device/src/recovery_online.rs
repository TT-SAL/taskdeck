//! The recovery tool's online steps (device.md §15.8, §25).

/// The transport to one relay with the door credential (device.md §15.8).
pub struct RecoveryLink {}

/// The relay's answer to a Fence (device.md §15.8 step 5).
pub enum FenceAnswer {
    Fenced { seq: u64, chain: [u8; 32] },
    Counter { last_ctr: u64, last_seq: u64 },
}

/// The relay's answer to an append (device.md §15.8 steps 9 and 11).
pub enum AppendAnswer {
    Stored { seq: u64, chain: [u8; 32] },
    Gap { last_ctr: u64, last_seq: u64 },
    Conflict { seq: u64 },
}

impl RecoveryLink {
    /// Opens the link (device.md §15.8).
    pub fn new(
        address: &str,
        recovery: &taskdeck_seal::RecoveryKeys,
        trust: super::TrustRoots,
    ) -> Result<Self, super::DeviceError> {
        unimplemented!()
    }

    /// Gets a read token (device.md §15.8 step 1).
    pub fn read_token(&mut self, backup_token: Option<&str>) -> Result<(), super::DeviceError> {
        unimplemented!()
    }

    /// Reads the entries after `after` (device.md §15.8).
    pub fn read_after(&mut self, after: u64) -> Result<taskdeck_sync::Page, super::DeviceError> {
        unimplemented!()
    }

    /// Sends the Fence (device.md §15.8 step 5).
    pub fn fence(
        &mut self,
        frame: Vec<u8>,
        host: &str,
        door: [u8; 5],
        home_gen: u32,
    ) -> Result<FenceAnswer, super::DeviceError> {
        unimplemented!()
    }

    /// Reinstates a device with the door (device.md §15.8 step 8).
    pub fn reinstate(&mut self, dev: [u8; 16], first_epoch: u32, wraps: Vec<[u8; 96]>) -> Result<(), super::DeviceError> {
        unimplemented!()
    }

    /// Appends one frame of `dev_R` with the door (device.md §15.8 step 9).
    pub fn append(&mut self, frame: Vec<u8>) -> Result<AppendAnswer, super::DeviceError> {
        unimplemented!()
    }

    /// Revokes a device with the door (device.md §15.8 step 11).
    pub fn revoke(&mut self, dev: [u8; 16]) -> Result<(), super::DeviceError> {
        unimplemented!()
    }
}
