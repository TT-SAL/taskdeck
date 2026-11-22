//! Shared encodings (seal.md §5, §26).

use crate::SealError;

/// `"https://" ‖ host`, or `seal/host_format` (seal.md §5.4, SEAL-ENC-006, §26).
pub fn relay_origin(host: &str) -> Result<String, SealError> {
    unimplemented!()
}
