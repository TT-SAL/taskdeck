//! A strict JSON parser (README §5.3): RFC 8259 with duplicate keys,
//! floating-point numbers and trailing data refused. It reads the relay's
//! JSON answers (device.md §10.5, DEV-NET-009) and the request lines of
//! `taskdeck-devicectl` (§19.2, DEV-CTL-004). Unknown fields are refused by
//! each caller, which knows its table (`Obj`).

use std::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

/// Parses `text` strictly; `None` on any refusal. `depth` bounds nesting.
pub(crate) fn parse(text: &str, depth: u32) -> Option<Value> {
    let mut de = serde_json::Deserializer::from_str(text);
    let v = Strict { depth }.deserialize(&mut de).ok()?;
    de.end().ok()?;
    Some(v)
}

/// Parses bytes as UTF-8 strict JSON.
pub(crate) fn parse_bytes(bytes: &[u8], depth: u32) -> Option<Value> {
    parse(std::str::from_utf8(bytes).ok()?, depth)
}

struct Strict {
    depth: u32,
}

impl<'de> DeserializeSeed<'de> for Strict {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Strict {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("strict JSON")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(Number::from(v)))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(Number::from(v)))
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Value, E> {
        Err(E::custom("a float"))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_string()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Value, A::Error> {
        let depth = self.depth.checked_sub(1).ok_or_else(|| de::Error::custom("too deep"))?;
        let mut out = Vec::new();
        while let Some(v) = s.next_element_seed(Strict { depth })? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Value, A::Error> {
        let depth = self.depth.checked_sub(1).ok_or_else(|| de::Error::custom("too deep"))?;
        let mut out = Map::new();
        while let Some(k) = m.next_key::<String>()? {
            let v = m.next_value_seed(Strict { depth })?;
            if out.insert(k, v).is_some() {
                return Err(de::Error::custom("a duplicate key"));
            }
        }
        Ok(Value::Object(out))
    }
}

/// An object read field by field: every field must be taken, and `finish`
/// refuses one that was not (README §5.3: unknown fields are errors).
pub(crate) struct Obj {
    map: Map<String, Value>,
}

impl Obj {
    pub(crate) fn new(v: Value) -> Option<Obj> {
        match v {
            Value::Object(map) => Some(Obj { map }),
            _ => None,
        }
    }

    pub(crate) fn take(&mut self, k: &str) -> Option<Value> {
        self.map.remove(k)
    }

    /// A required integer from 0 to 2^53 − 1.
    pub(crate) fn u53(&mut self, k: &str) -> Option<u64> {
        let n = self.take(k)?.as_u64()?;
        (n < 1 << 53).then_some(n)
    }

    /// An optional integer from 0 to 2^53 − 1; `Err` when present and wrong.
    pub(crate) fn opt_u53(&mut self, k: &str) -> Result<Option<u64>, ()> {
        match self.take(k) {
            None => Ok(None),
            Some(v) => v.as_u64().filter(|n| *n < 1 << 53).map(Some).ok_or(()),
        }
    }

    pub(crate) fn i53(&mut self, k: &str) -> Option<i64> {
        let n = self.take(k)?.as_i64()?;
        (n.unsigned_abs() < 1 << 53).then_some(n)
    }

    pub(crate) fn string(&mut self, k: &str) -> Option<String> {
        match self.take(k)? {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn boolean(&mut self, k: &str) -> Option<bool> {
        self.take(k)?.as_bool()
    }

    /// Lowercase hex of exactly `N` bytes.
    pub(crate) fn hex<const N: usize>(&mut self, k: &str) -> Option<[u8; N]> {
        hex_exact::<N>(self.take(k)?.as_str()?)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// `Some(())` when every field was taken.
    pub(crate) fn finish(self) -> Option<()> {
        self.map.is_empty().then_some(())
    }
}

/// Lowercase hex of exactly `N` bytes (README §5.4).
pub(crate) fn hex_exact<const N: usize>(s: &str) -> Option<[u8; N]> {
    let b = s.as_bytes();
    if b.len() != 2 * N {
        return None;
    }
    let nib = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = (nib(b[2 * i])? << 4) | nib(b[2 * i + 1])?;
    }
    Some(out)
}

/// Lowercase hex of any length.
pub(crate) fn hex_vec(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if !b.len().is_multiple_of(2) {
        return None;
    }
    let nib = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    b.chunks(2).map(|p| Some((nib(p[0])? << 4) | nib(p[1])?)).collect()
}

/// Lowercase hex (README §5.4).
pub(crate) fn hex(bytes: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(D[usize::from(b >> 4)]));
        s.push(char::from(D[usize::from(b & 15)]));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_duplicates_floats_and_trailing_data() {
        assert!(parse(r#"{"a":1,"b":[true,"x",{"c":-2}]}"#, 8).is_some());
        assert!(parse(r#"{"a":1,"a":2}"#, 8).is_none());
        assert!(parse(r#"{"a":1.5}"#, 8).is_none());
        assert!(parse(r#"{"a":1e3}"#, 8).is_none());
        assert!(parse(r#"{"a":1} x"#, 8).is_none());
        assert!(parse(r#"[[[[1]]]]"#, 3).is_none());
        assert!(parse(r#"[[[[1]]]]"#, 5).is_some());
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(hex(&[0, 0xab, 0x10]), "00ab10");
        assert_eq!(hex_exact::<3>("00ab10"), Some([0, 0xab, 0x10]));
        assert_eq!(hex_exact::<3>("00AB10"), None);
        assert_eq!(hex_vec("0a0b"), Some(vec![10, 11]));
    }
}
