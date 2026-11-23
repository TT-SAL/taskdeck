//! Randomness (seal.md §3, §26).

use crate::SealError;

pub(crate) mod private {
    /// Seals `Entropy` so no other crate implements it (SEAL-GEN-005).
    pub trait Sealed {}
}

/// The only way random bytes enter the crate (SEAL-GEN-004, SEAL-GEN-005; seal.md §26).
///
/// The trait is sealed: another crate cannot implement it, since its
/// supertrait is private to this crate.
///
/// ```compile_fail,E0277
/// struct Mine;
/// impl taskdeck_seal::Entropy for Mine {
///     fn fill(&mut self, out: &mut [u8]) -> Result<(), taskdeck_seal::SealError> {
///         out.fill(4);
///         Ok(())
///     }
/// }
/// ```
pub trait Entropy: private::Sealed {
    /// Fills `out`, or fails with `seal/entropy` (SEAL-GEN-004).
    fn fill(&mut self, out: &mut [u8]) -> Result<(), SealError>;
}

/// The operating system's generator through `getrandom` (SEAL-GEN-005; seal.md §26).
pub struct OsEntropy;

impl private::Sealed for OsEntropy {}

impl Entropy for OsEntropy {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), SealError> {
        getrandom::fill(out).map_err(|_| SealError::Entropy)
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
        std::hint::black_box(TEST_ENTROPY_MARKER);
        FixedEntropy { bytes, next: 0 }
    }
}

#[cfg(feature = "test-entropy")]
impl private::Sealed for FixedEntropy {}

#[cfg(feature = "test-entropy")]
impl Entropy for FixedEntropy {
    /// Hands out the next `out.len()` bytes; when fewer remain, fails with
    /// `seal/entropy` and hands out nothing (SEAL-GEN-004).
    fn fill(&mut self, out: &mut [u8]) -> Result<(), SealError> {
        let end = self.next.checked_add(out.len()).ok_or(SealError::Entropy)?;
        let src = self.bytes.get(self.next..end).ok_or(SealError::Entropy)?;
        out.copy_from_slice(src);
        self.next = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

/// The marker a `test-entropy` build contains and no other build does (SEAL-GEN-014; seal.md §26).
#[cfg(feature = "test-entropy")]
pub const TEST_ENTROPY_MARKER: &str = "taskdeck-test-entropy-enabled-3b9e5d";
