//! Canonical encoding v1 (core.md §4, §20).

use crate::CoreError;

/// Canonical bytes of a value; `core/schema` for null or a float (core.md §4.2).
pub fn encode(value: &serde_json::Value) -> Result<Vec<u8>, CoreError> {
    unimplemented!()
}

/// Parsing, steps 1 to 3 and 5 (core.md §4.5).
pub fn parse(bytes: &[u8], limit: usize) -> Result<serde_json::Value, CoreError> {
    unimplemented!()
}

/// Whether `bytes` are canonical and within `limit` (core.md §4.2, §4.5).
pub fn is_canonical(bytes: &[u8], limit: usize) -> bool {
    unimplemented!()
}

/// A record with a schema and defaults (core.md §4.3).
pub trait Record: Sized {
    /// The record as a value (core.md §4.3).
    fn to_value(&self) -> serde_json::Value;
    /// The record from a value (core.md §4.3, §4.5).
    fn from_value(v: &serde_json::Value) -> Result<Self, CoreError>;
    /// The record's canonical bytes (core.md §4.2).
    fn to_canonical(&self) -> Vec<u8>;
    /// The record from canonical bytes (core.md §4.5).
    fn from_canonical(bytes: &[u8]) -> Result<Self, CoreError>;
}
