//! Crockford base32 (seal.md §5.2, §26).

/// Encodes bytes in the alphabet of §5.2 (seal.md §26).
pub fn encode(bytes: &[u8]) -> String {
    unimplemented!()
}

/// Decodes typed text of `bytes` bytes, with §5.2's normalisation (seal.md §26).
pub fn decode_typed(text: &str, bytes: usize) -> Result<Vec<u8>, super::SealError> {
    unimplemented!()
}
