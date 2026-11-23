//! Canonical encoding v1 (`core.md` §4) for the JSON payloads of seal.md
//! §5.3, private to this crate (lead decision L4: seal has no dependency on
//! core). The encoder writes `core.md` §4.2 exactly, sorting object keys
//! itself; the parser follows `core.md` §4.5 and seal.md §5.3, and the
//! schema helpers below report each rejection of SEAL-ENC-004 by name.

use serde_json::{Map, Number, Value};

/// The largest integer of the encoding, `2^53 − 1` (`core.md` §4.2 rule 5).
pub(crate) const INT_MAX: u64 = (1 << 53) - 1;
/// The deepest nesting a parser accepts (`core.md` §4.5 step 2).
pub(crate) const DEPTH_MAX: usize = 32;

/// Why a JSON payload was refused. Every variant is `seal/bad_payload` at
/// §14.2 step 15; the names let the unit tests show each rejection of
/// SEAL-ENC-004 apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JsonError {
    /// Not valid UTF-8 or not RFC 8259 JSON text.
    NotJson,
    /// Nesting deeper than 32.
    TooDeep,
    /// An object with a key twice.
    DuplicateKey,
    /// A `null`.
    Null,
    /// A number with a fraction or an exponent.
    Float,
    /// An integer outside its stated range (or outside ±(2^53 − 1)).
    IntRange,
    /// The bytes are not `enc` of the value parsed from them (SEAL-ENC-003).
    NotCanonical,
    /// A field the table does not list.
    UnknownField,
    /// A required field is absent.
    MissingField,
    /// A field of the wrong JSON type.
    WrongType,
    /// A byte string that is not lowercase hex of the stated width.
    BadHex,
    /// A string over its byte limit or outside its alphabet (§5.5).
    StringLimit,
    /// A host outside §5.4 (SEAL-ENC-006).
    Host,
    /// An array with fewer or more elements than its stated count.
    ArrayCount,
    /// An array not in its stated order, or with a duplicate under its key.
    ArrayOrder,
    /// A control item whose `type` names no item of §12.
    UnknownItem,
    /// Any other constraint of a table (a multiple of 64, `parts`, a bound
    /// that depends on the entry).
    Constraint,
}

// ---------------------------------------------------------------- encoder

/// `enc(v)` of `core.md` §4.2. Fails on a value the encoding has no bytes
/// for: `null`, a number that is not an integer, an integer outside
/// ±(2^53 − 1).
pub(crate) fn encode(v: &Value) -> Result<Vec<u8>, JsonError> {
    let mut out = Vec::new();
    write_value(v, &mut out)?;
    Ok(out)
}

fn write_value(v: &Value, out: &mut Vec<u8>) -> Result<(), JsonError> {
    match v {
        Value::Null => Err(JsonError::Null),
        Value::Bool(true) => {
            out.extend_from_slice(b"true");
            Ok(())
        }
        Value::Bool(false) => {
            out.extend_from_slice(b"false");
            Ok(())
        }
        Value::Number(n) => write_int(n, out),
        Value::String(s) => {
            write_string(s, out);
            Ok(())
        }
        Value::Array(a) => {
            out.push(b'[');
            for (i, e) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_value(e, out)?;
            }
            out.push(b']');
            Ok(())
        }
        Value::Object(m) => {
            // Sorted here by the keys' UTF-8 bytes as unsigned byte strings,
            // whatever order the map keeps (`core.md` §4.2 rule 2).
            let mut members: Vec<(&String, &Value)> = m.iter().collect();
            members.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            out.push(b'{');
            for (i, (k, e)) in members.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_string(k, out);
                out.push(b':');
                write_value(e, out)?;
            }
            out.push(b'}');
            Ok(())
        }
    }
}

fn write_int(n: &Number, out: &mut Vec<u8>) -> Result<(), JsonError> {
    if let Some(u) = n.as_u64() {
        if u > INT_MAX {
            return Err(JsonError::IntRange);
        }
        out.extend_from_slice(u.to_string().as_bytes());
        Ok(())
    } else if let Some(i) = n.as_i64() {
        if i.unsigned_abs() > INT_MAX {
            return Err(JsonError::IntRange);
        }
        out.extend_from_slice(i.to_string().as_bytes());
        Ok(())
    } else {
        Err(JsonError::Float)
    }
}

/// A string with exactly the escapes of `core.md` §4.2 rule 4.
fn write_string(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    let mut buf = [0u8; 4];
    for c in s.chars() {
        match c {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{08}' => out.extend_from_slice(b"\\b"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\u{0C}' => out.extend_from_slice(b"\\f"),
            '\r' => out.extend_from_slice(b"\\r"),
            c if (c as u32) < 0x20 => {
                let b = c as u32;
                out.extend_from_slice(b"\\u00");
                out.push(hex_digit(b >> 4));
                out.push(hex_digit(b & 15));
            }
            c => out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes()),
        }
    }
    out.push(b'"');
}

// ----------------------------------------------------------------- parser

/// The strict parser of `core.md` §4.5 steps 2 to 5 with seal.md §5.3's
/// profile: valid UTF-8 RFC 8259 text, depth at most 32, no duplicate key,
/// no `null`, no fraction or exponent, integers within ±(2^53 − 1), and the
/// bytes exactly `enc` of the parsed value (SEAL-ENC-003). The schema checks
/// (step 4) are the callers'; each caller also re-encodes its record, which
/// catches a field written at its default. The byte limit (step 1) is the
/// caller's: a payload is bounded by the frame (§10.1).
pub(crate) fn parse(bytes: &[u8]) -> Result<Value, JsonError> {
    let text = core::str::from_utf8(bytes).map_err(|_| JsonError::NotJson)?;
    let mut p = Parser { s: text.as_bytes(), i: 0 };
    p.ws();
    let v = p.value(1)?;
    p.ws();
    if p.i != p.s.len() {
        return Err(JsonError::NotJson);
    }
    if encode(&v)? != bytes {
        return Err(JsonError::NotCanonical);
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.i += 1;
        Some(c)
    }

    fn eat(&mut self, c: u8) -> Result<(), JsonError> {
        if self.bump() == Some(c) { Ok(()) } else { Err(JsonError::NotJson) }
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn literal(&mut self, word: &[u8]) -> Result<(), JsonError> {
        let end = self.i.checked_add(word.len()).ok_or(JsonError::NotJson)?;
        if self.s.get(self.i..end) == Some(word) {
            self.i = end;
            Ok(())
        } else {
            Err(JsonError::NotJson)
        }
    }

    /// One value at nesting `depth` (an object or array at depth 1 is the
    /// outermost).
    fn value(&mut self, depth: usize) -> Result<Value, JsonError> {
        match self.peek() {
            Some(b'{') => {
                if depth > DEPTH_MAX {
                    return Err(JsonError::TooDeep);
                }
                self.object(depth)
            }
            Some(b'[') => {
                if depth > DEPTH_MAX {
                    return Err(JsonError::TooDeep);
                }
                self.array(depth)
            }
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b't') => self.literal(b"true").map(|_| Value::Bool(true)),
            Some(b'f') => self.literal(b"false").map(|_| Value::Bool(false)),
            Some(b'n') => {
                self.literal(b"null")?;
                Err(JsonError::Null)
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(JsonError::NotJson),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, JsonError> {
        self.eat(b'{')?;
        let mut m = Map::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Value::Object(m));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(JsonError::NotJson);
            }
            let k = self.string()?;
            self.ws();
            self.eat(b':')?;
            self.ws();
            let v = self.value(depth + 1)?;
            if m.contains_key(&k) {
                return Err(JsonError::DuplicateKey);
            }
            m.insert(k, v);
            self.ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b'}') => return Ok(Value::Object(m)),
                _ => return Err(JsonError::NotJson),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, JsonError> {
        self.eat(b'[')?;
        let mut a = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Value::Array(a));
        }
        loop {
            self.ws();
            a.push(self.value(depth + 1)?);
            self.ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b']') => return Ok(Value::Array(a)),
                _ => return Err(JsonError::NotJson),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let c = self.bump().ok_or(JsonError::NotJson)?;
            let d = char::from(c).to_digit(16).ok_or(JsonError::NotJson)?;
            v = (v << 4) | d;
        }
        Ok(v)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let start = self.i;
            // Copy a run of plain bytes at once; the input is valid UTF-8 and
            // the run stops only at ASCII bytes, so it is a str boundary.
            while let Some(c) = self.peek() {
                if c == b'"' || c == b'\\' || c < 0x20 {
                    break;
                }
                self.i += 1;
            }
            let run = self.s.get(start..self.i).ok_or(JsonError::NotJson)?;
            out.push_str(core::str::from_utf8(run).map_err(|_| JsonError::NotJson)?);
            match self.bump() {
                Some(b'"') => return Ok(out),
                Some(b'\\') => {
                    let c = match self.bump() {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{08}',
                        Some(b'f') => '\u{0C}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) {
                                self.eat(b'\\')?;
                                self.eat(b'u')?;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(JsonError::NotJson);
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            // A lone surrogate is no character.
                            char::from_u32(code).ok_or(JsonError::NotJson)?
                        }
                        _ => return Err(JsonError::NotJson),
                    };
                    out.push(c);
                }
                // An unescaped control character, or the end of input.
                _ => return Err(JsonError::NotJson),
            }
        }
    }

    fn number(&mut self) -> Result<Value, JsonError> {
        let neg = self.peek() == Some(b'-');
        if neg {
            self.i += 1;
        }
        let digits_start = self.i;
        match self.bump() {
            Some(b'0') => {}
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.i += 1;
                }
            }
            _ => return Err(JsonError::NotJson),
        }
        let digits_end = self.i;
        let mut float = false;
        if self.peek() == Some(b'.') {
            self.i += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(JsonError::NotJson);
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.i += 1;
            }
            float = true;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(JsonError::NotJson);
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.i += 1;
            }
            float = true;
        }
        if float {
            return Err(JsonError::Float);
        }
        let digits = self.s.get(digits_start..digits_end).ok_or(JsonError::NotJson)?;
        // 2^53 − 1 has 16 digits; more is out of range whatever they are.
        if digits.len() > 16 {
            return Err(JsonError::IntRange);
        }
        let mut u = 0u64;
        for d in digits {
            u = u * 10 + u64::from(d - b'0');
        }
        if u > INT_MAX {
            return Err(JsonError::IntRange);
        }
        if neg {
            // `-0` parses as 0 and fails the canonical comparison.
            let i = i64::try_from(u).map_err(|_| JsonError::IntRange)?;
            Ok(Value::Number(Number::from(-i)))
        } else {
            Ok(Value::Number(Number::from(u)))
        }
    }
}

// ----------------------------------------------------------------- schema

/// The fields of one object, read at most once each; `finish` refuses a
/// field nobody read (an unknown field, SEAL-ENC-004).
pub(crate) struct Fields<'a> {
    map: &'a Map<String, Value>,
    read: usize,
}

impl<'a> Fields<'a> {
    pub(crate) fn of(v: &'a Value) -> Result<Self, JsonError> {
        match v {
            Value::Object(map) => Ok(Fields { map, read: 0 }),
            _ => Err(JsonError::WrongType),
        }
    }

    /// An optional field (or one with a default).
    pub(crate) fn opt(&mut self, k: &str) -> Option<&'a Value> {
        let v = self.map.get(k)?;
        self.read += 1;
        Some(v)
    }

    /// A required field.
    pub(crate) fn req(&mut self, k: &str) -> Result<&'a Value, JsonError> {
        self.opt(k).ok_or(JsonError::MissingField)
    }

    /// Refuses any field not read.
    pub(crate) fn finish(self) -> Result<(), JsonError> {
        if self.read == self.map.len() { Ok(()) } else { Err(JsonError::UnknownField) }
    }
}

/// An integer from `min` to `max`.
pub(crate) fn int(v: &Value, min: u64, max: u64) -> Result<u64, JsonError> {
    match v {
        Value::Number(n) => match n.as_u64() {
            Some(u) if (min..=max).contains(&u) && u <= INT_MAX => Ok(u),
            Some(_) => Err(JsonError::IntRange),
            None if n.is_i64() => Err(JsonError::IntRange),
            None => Err(JsonError::Float),
        },
        _ => Err(JsonError::WrongType),
    }
}

/// An integer from `min` to `max` that fits a `u32`.
pub(crate) fn int32(v: &Value, min: u32, max: u32) -> Result<u32, JsonError> {
    let u = int(v, u64::from(min), u64::from(max))?;
    u32::try_from(u).map_err(|_| JsonError::IntRange)
}

/// A string.
pub(crate) fn string(v: &Value) -> Result<&str, JsonError> {
    match v {
        Value::String(s) => Ok(s),
        _ => Err(JsonError::WrongType),
    }
}

/// An array of `min` to `max` elements.
pub(crate) fn array(v: &Value, min: usize, max: usize) -> Result<&[Value], JsonError> {
    match v {
        Value::Array(a) if (min..=max).contains(&a.len()) => Ok(a),
        Value::Array(_) => Err(JsonError::ArrayCount),
        _ => Err(JsonError::WrongType),
    }
}

/// Exactly `2N` lowercase hex digits (seal.md §5.3).
pub(crate) fn hex<const N: usize>(v: &Value) -> Result<[u8; N], JsonError> {
    let s = string(v)?.as_bytes();
    if s.len() != 2 * N {
        return Err(JsonError::BadHex);
    }
    let digit = |c: u8| match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        _ => Err(JsonError::BadHex),
    };
    let mut out = [0u8; N];
    for (o, pair) in out.iter_mut().zip(s.chunks_exact(2)) {
        let (hi, lo) = match pair {
            [hi, lo] => (*hi, *lo),
            _ => return Err(JsonError::BadHex),
        };
        *o = (digit(hi)? << 4) | digit(lo)?;
    }
    Ok(out)
}

/// Lowercase hex of `bytes`, as a JSON string.
pub(crate) fn hex_value(bytes: &[u8]) -> Value {
    let mut s = String::with_capacity(2 * bytes.len());
    for b in bytes {
        s.push(char::from(hex_digit(u32::from(b >> 4))));
        s.push(char::from(hex_digit(u32::from(b & 15))));
    }
    Value::String(s)
}

/// The lowercase hex digit of the low four bits of `n`.
fn hex_digit(n: u32) -> u8 {
    char::from_digit(n & 15, 16).map_or(b'0', |c| c as u8)
}

/// An integer value of the encoding, or `IntRange` above 2^53 − 1.
pub(crate) fn int_value(u: u64) -> Result<Value, JsonError> {
    if u > INT_MAX { Err(JsonError::IntRange) } else { Ok(Value::Number(Number::from(u))) }
}

#[cfg(test)]
mod tests;
