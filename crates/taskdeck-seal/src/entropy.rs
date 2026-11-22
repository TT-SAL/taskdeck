//! Randomness (seal.md §3, §26).

use crate::SealError;

pub(crate) mod private {
    /// Seals `Entropy` so no other crate implements it (SEAL-GEN-005).
    pub trait Sealed {}
}

/// The only way random bytes enter the crate (SEAL-GEN-004, SEAL-GEN-005; seal.md §26).
pub trait Entropy: private::Sealed {
    /// Fills `out`, or fails with `seal/entropy` (SEAL-GEN-004).
    fn fill(&mut self, out: &mut [u8]) -> Result<(), SealError>;
}

/// The operating system's generator through `getrandom` (SEAL-GEN-005; seal.md §26).
pub struct OsEntropy;

impl private::Sealed for OsEntropy {}

impl Entropy for OsEntropy {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), SealError> {
        unimplemented!()
    }
}

/// Replays given bytes in draw order, for the vectors (seal.md §24, §26; SEAL-GEN-005).
#[cfg(feature = "test-entropy")]
pub struct FixedEntropy {
    bytes: Vec<u8>,
    next: usize,
}

#[cfg(feature = "test-entropy")]
impl FixedEntropy {
    /// Replays `bytes`; passes `TEST_ENTROPY_MARKER` to `std::hint::black_box` (SEAL-GEN-014).
    pub fn new(bytes: Vec<u8>) -> Self {
        unimplemented!()
    }
}

#[cfg(feature = "test-entropy")]
impl private::Sealed for FixedEntropy {}

#[cfg(feature = "test-entropy")]
impl Entropy for FixedEntropy {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), SealError> {
        unimplemented!()
    }
}

/// The marker a `test-entropy` build contains and no other build does (SEAL-GEN-014; seal.md §26).
#[cfg(feature = "test-entropy")]
pub const TEST_ENTROPY_MARKER: &str = "taskdeck-test-entropy-enabled-3b9e5d";
