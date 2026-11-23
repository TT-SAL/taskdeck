//! Canonical encoding v1 (core.md §4, §20).
//!
//! The encoder writes exactly the bytes of §4.2. The parser is hand-written:
//! `serde_json` keeps the last of two duplicate keys and nests to 128 levels,
//! so it cannot perform steps 2 and 3 of §4.5 itself. It builds a
//! `serde_json::Value` directly, counts container depth (a scalar is depth 0,
//! `[]` depth 1; at most 32) and records, without stopping, the step-3 and
//! step-4 findings it can see on the way (a duplicate key, `null`, a number
//! with a fraction or exponent, an integer outside ±(2^53 − 1)), so that a
//! later syntax error still reports `core/not_json` first (CORE-ENC-004).

use serde_json::{Map, Number, Value};

use crate::CoreError;
use crate::time::Instant;

/// The largest magnitude of a canonical integer: 2^53 − 1 (core.md §4.2 rule 5).
pub(crate) const MAX_INT: i64 = (1 << 53) - 1;

/// The JSON nesting depth limit (core.md §17).
pub(crate) const MAX_DEPTH: usize = 32;

/// The limit of the canonical board bytes, 60 MiB (core.md §17).
pub(crate) const BOARD_LIMIT: usize = 62_914_560;

pub(crate) const TOO_LARGE: &str = "core/too_large";
pub(crate) const NOT_JSON: &str = "core/not_json";
pub(crate) const SCHEMA: &str = "core/schema";
pub(crate) const NOT_CANONICAL: &str = "core/not_canonical";

pub(crate) fn schema() -> CoreError {
    CoreError::code(SCHEMA)
}

/// Canonical bytes of a value; `core/schema` for null or a float (core.md §4.2).
pub fn encode(value: &serde_json::Value) -> Result<Vec<u8>, CoreError> {
    let mut out = Vec::new();
    write_value(value, &mut out)?;
    Ok(out)
}

/// Parsing, steps 1 to 3 and 5 (core.md §4.5).
pub fn parse(bytes: &[u8], limit: usize) -> Result<serde_json::Value, CoreError> {
    let value = parse_value(bytes, limit)?;
    let again = encode(&value)?;
    if again != bytes {
        return Err(CoreError::code(NOT_CANONICAL));
    }
    Ok(value)
}

/// Whether `bytes` are canonical and within `limit` (core.md §4.2, §4.5).
pub fn is_canonical(bytes: &[u8], limit: usize) -> bool {
    parse(bytes, limit).is_ok()
}

/// A record with a schema and defaults (core.md §4.3).
pub trait Record: Sized {
    /// The byte limit of §17 that applies to this record's canonical bytes,
    /// checked before anything is parsed (core.md §4.5 step 1, §20). Every
    /// record names its own; there is no default.
    const LIMIT: usize;

    /// The record as a value (core.md §4.3).
    fn to_value(&self) -> serde_json::Value;
    /// The record from a value (core.md §4.3, §4.5).
    fn from_value(v: &serde_json::Value) -> Result<Self, CoreError>;
    /// The record's canonical bytes (core.md §4.2).
    fn to_canonical(&self) -> Vec<u8> {
        // A record's value holds no null and no float, so `encode` cannot fail.
        encode(&self.to_value()).unwrap_or_default()
    }
    /// The record from canonical bytes (core.md §4.5).
    fn from_canonical(bytes: &[u8]) -> Result<Self, CoreError> {
        record_from_canonical(bytes, Self::LIMIT)
    }
}

/// The five steps of §4.5 for a record: 1 to 3 here, 4 in `from_value`, 5 by
/// encoding the parsed record once and comparing (core.md §4.5).
pub(crate) fn record_from_canonical<R: Record>(bytes: &[u8], limit: usize) -> Result<R, CoreError> {
    let value = parse_value(bytes, limit)?;
    let record = R::from_value(&value)?;
    drop(value);
    if record.to_canonical() != bytes {
        return Err(CoreError::code(NOT_CANONICAL));
    }
    Ok(record)
}

// ---------------------------------------------------------------- encoder

fn write_value(v: &Value, out: &mut Vec<u8>) -> Result<(), CoreError> {
    match v {
        Value::Null => Err(schema()),
        Value::Bool(true) => {
            out.extend_from_slice(b"true");
            Ok(())
        }
        Value::Bool(false) => {
            out.extend_from_slice(b"false");
            Ok(())
        }
        Value::Number(n) => {
            let i = number_int(n).ok_or_else(schema)?;
            write_int(i, out);
            Ok(())
        }
        Value::String(s) => {
            write_str(s, out);
            Ok(())
        }
        Value::Array(a) => {
            out.push(b'[');
            for (k, e) in a.iter().enumerate() {
                if k > 0 {
                    out.push(b',');
                }
                write_value(e, out)?;
            }
            out.push(b']');
            Ok(())
        }
        Value::Object(m) => {
            // Members in ascending unsigned-byte order of their keys
            // (§4.2 rule 2), whatever order the map keeps them in.
            let mut members: Vec<(&String, &Value)> = m.iter().collect();
            if !members.is_sorted_by(|a, b| a.0.as_bytes() <= b.0.as_bytes()) {
                members.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            }
            out.push(b'{');
            for (k, (key, value)) in members.into_iter().enumerate() {
                if k > 0 {
                    out.push(b',');
                }
                write_str(key, out);
                out.push(b':');
                write_value(value, out)?;
            }
            out.push(b'}');
            Ok(())
        }
    }
}

/// The integer of a JSON number within ±(2^53 − 1), or none.
fn number_int(n: &Number) -> Option<i64> {
    let i = n.as_i64()?;
    if (-MAX_INT..=MAX_INT).contains(&i) { Some(i) } else { None }
}

fn write_int(i: i64, out: &mut Vec<u8>) {
    if i < 0 {
        out.push(b'-');
    }
    let mut m = i.unsigned_abs();
    let mut buf = [0u8; 20];
    let mut at = buf.len();
    loop {
        at -= 1;
        // `m % 10` is below 10, so the cast keeps it.
        buf[at] = b'0' + (m % 10) as u8;
        m /= 10;
        if m == 0 {
            break;
        }
    }
    out.extend_from_slice(&buf[at..]);
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// A string with exactly the escapes of the table in §4.2 rule 4 (CORE-ENC-003).
fn write_str(s: &str, out: &mut Vec<u8>) {
    let b = s.as_bytes();
    out.push(b'"');
    let mut run = 0;
    for (i, &c) in b.iter().enumerate() {
        let esc: &[u8] = match c {
            b'"' => b"\\\"",
            b'\\' => b"\\\\",
            0x08 => b"\\b",
            0x09 => b"\\t",
            0x0a => b"\\n",
            0x0c => b"\\f",
            0x0d => b"\\r",
            0x00..=0x1f => &[],
            _ => continue,
        };
        out.extend_from_slice(&b[run..i]);
        if esc.is_empty() {
            out.extend_from_slice(b"\\u00");
            out.push(HEX[usize::from(c >> 4)]);
            out.push(HEX[usize::from(c & 15)]);
        } else {
            out.extend_from_slice(esc);
        }
        run = i + 1;
    }
    out.extend_from_slice(&b[run..]);
    out.push(b'"');
}

// ---------------------------------------------------------------- parser

/// §4.5 steps 1 to 3, plus the value-level findings of step 4 that a
/// `serde_json::Value` cannot carry (`null` is kept out too, so a record's
/// `from_value` never sees one): `core/too_large`, `core/not_json`,
/// `core/schema`.
pub(crate) fn parse_value(bytes: &[u8], limit: usize) -> Result<Value, CoreError> {
    let (v, bad) = parse_value_steps(bytes, limit)?;
    if bad {
        return Err(schema());
    }
    Ok(v)
}

/// §4.5 steps 1 to 3 only: `core/too_large`, `core/not_json`, and
/// `core/schema` for a duplicate key. The step-4 value findings (`null`, a
/// number with a fraction or exponent, an integer outside ±(2^53 − 1)) do not
/// fail here: each such value is `Value::Null` in the result and the flag is
/// true. `parse_command` needs this split, since its step 4 (`op`) comes
/// between §4.5 steps 3 and 4 (core.md §9.1).
pub(crate) fn parse_value_steps(bytes: &[u8], limit: usize) -> Result<(Value, bool), CoreError> {
    // Step 1, before anything is allocated for the input (CORE-GEN-009).
    if bytes.len() > limit {
        return Err(CoreError::code(TOO_LARGE));
    }
    // Step 2. A byte-order mark is not JSON whitespace, so the grammar
    // rejects it below; checked here as well to make the rule visible.
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(CoreError::code(NOT_JSON));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| CoreError::code(NOT_JSON))?;
    let mut p = Parser {
        s: text,
        b: bytes,
        i: 0,
        dup: false,
        bad: false,
        left: tree_budget(limit),
        over: false,
    };
    p.ws();
    let v = p.value(0).map_err(|()| CoreError::code(NOT_JSON))?;
    p.ws();
    if p.i != p.b.len() {
        return Err(CoreError::code(NOT_JSON));
    }
    // Step 3, only once the text is JSON. A tree over its budget is
    // `core/schema` too, returned only now that the whole text has passed
    // step 2 (core.md §4.5 "Tree budget").
    if p.dup || p.over {
        return Err(schema());
    }
    Ok((v, p.bad))
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
    /// A duplicate key was seen (step 3).
    dup: bool,
    /// A `null`, float or out-of-range integer was seen (step 4).
    bad: bool,
    /// Units of the tree budget still left (core.md §4.5 "Tree budget").
    left: usize,
    /// The budget ran out: from then on the parser only checks the grammar
    /// (step 2) and allocates nothing; the result is `core/schema`.
    over: bool,
}

/// The tree budget of core.md §4.5: `max(32 × limit, 1,048,576)` units.
///
/// Without a bound, a `serde_json` tree costs far more than its text (a
/// `BTreeMap`'s first member allocates a whole node, so `{"":0}` costs about
/// 125 times its length and a 60 MiB board could take 7 GiB before step 4
/// ran; CORE-GEN-007). The charges are fixed numbers, the same on every
/// platform, so that a text is refused on a 32-bit phone exactly when it is
/// on a 64-bit desktop (core.md §4.5 "Tree budget").
pub(crate) fn tree_budget(limit: usize) -> usize {
    limit.saturating_mul(TREE_FACTOR).max(TREE_FLOOR)
}

/// The multiple of the input's limit in the tree budget (core.md §4.5).
pub(crate) const TREE_FACTOR: usize = 32;

/// The least tree budget, for small limits: 1,048,576 units (core.md §4.5).
pub(crate) const TREE_FLOOR: usize = 1 << 20;

/// The charge of a string beyond the bytes between its quotes (core.md §4.5).
const CHARGE_STRING: usize = 16;
/// The charge of an array element (core.md §4.5).
const CHARGE_ELEMENT: usize = 32;
/// The charge of an object per started group of five members (core.md §4.5).
const CHARGE_GROUP: usize = 744;

type P<T> = Result<T, ()>;

impl Parser<'_> {
    /// Charges `units` against the tree budget; false once it is spent.
    fn charge(&mut self, units: usize) -> bool {
        if !self.over {
            match self.left.checked_sub(units) {
                Some(l) => self.left = l,
                None => self.over = true,
            }
        }
        !self.over
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.i += 1;
        }
    }

    fn lit(&mut self, word: &[u8]) -> P<()> {
        if self.b.get(self.i..self.i + word.len()) == Some(word) {
            self.i += word.len();
            Ok(())
        } else {
            Err(())
        }
    }

    fn value(&mut self, depth: usize) -> P<Value> {
        match self.peek().ok_or(())? {
            b'{' => self.object(depth + 1),
            b'[' => self.array(depth + 1),
            b'"' => Ok(Value::String(self.string()?)),
            b't' => self.lit(b"true").map(|()| Value::Bool(true)),
            b'f' => self.lit(b"false").map(|()| Value::Bool(false)),
            b'n' => {
                self.lit(b"null")?;
                self.bad = true;
                Ok(Value::Null)
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(()),
        }
    }

    fn object(&mut self, depth: usize) -> P<Value> {
        if depth > MAX_DEPTH {
            return Err(());
        }
        self.i += 1; // `{`
        let mut map = Map::new();
        // Members as written, duplicates included (core.md §4.5).
        let mut members = 0usize;
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(());
            }
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(());
            }
            self.i += 1;
            self.ws();
            let v = self.value(depth)?;
            // 744 for each started group of five members: at the 1st, 6th,
            // 11th member and so on (core.md §4.5).
            let starts_group = members.is_multiple_of(5);
            members += 1;
            if self.over || (starts_group && !self.charge(CHARGE_GROUP)) {
                // Over budget: the tree is dropped for `core/schema`; keep
                // reading only to report `core/not_json` first if it applies.
                map = Map::new();
            } else {
                if map.contains_key(&key) {
                    self.dup = true;
                }
                map.insert(key, v);
            }
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::Object(map));
                }
                _ => return Err(()),
            }
        }
    }

    fn array(&mut self, depth: usize) -> P<Value> {
        if depth > MAX_DEPTH {
            return Err(());
        }
        self.i += 1; // `[`
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.ws();
            let v = self.value(depth)?;
            // 32 for every element (core.md §4.5).
            if self.charge(CHARGE_ELEMENT) {
                items.push(v);
            } else {
                items = Vec::new();
            }
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(()),
            }
        }
    }

    fn hex4(&mut self) -> P<u32> {
        let mut x = 0u32;
        for _ in 0..4 {
            let d = match self.peek().ok_or(())? {
                c @ b'0'..=b'9' => c - b'0',
                c @ b'a'..=b'f' => c - b'a' + 10,
                c @ b'A'..=b'F' => c - b'A' + 10,
                _ => return Err(()),
            };
            x = x * 16 + u32::from(d);
            self.i += 1;
        }
        Ok(x)
    }

    /// A string at `"`. The input is valid UTF-8 and every split point is an
    /// ASCII byte, so each slice taken is on a character boundary.
    fn string(&mut self) -> P<String> {
        self.i += 1; // `"`
        let start = self.i;
        // Fast path: no escape.
        loop {
            match self.peek().ok_or(())? {
                b'"' => {
                    let end = self.i;
                    self.i += 1;
                    // The bytes between the quotes as written, plus 16.
                    if !self.charge(end - start + CHARGE_STRING) {
                        return Ok(String::new());
                    }
                    return Ok(self.s.get(start..end).ok_or(())?.to_owned());
                }
                b'\\' => break,
                0x00..=0x1f => return Err(()),
                _ => self.i += 1,
            }
        }
        // The bytes between the quotes as written, escapes included, plus
        // 16, charged once (core.md §4.5).
        let keep = self.charge(self.string_end()? - start + CHARGE_STRING);
        let mut out = String::new();
        if keep {
            out.reserve_exact(self.string_end()? - start);
            out.push_str(self.s.get(start..self.i).ok_or(())?);
        }
        loop {
            let run = self.i;
            loop {
                match self.peek().ok_or(())? {
                    b'"' | b'\\' => break,
                    0x00..=0x1f => return Err(()),
                    _ => self.i += 1,
                }
            }
            if keep {
                out.push_str(self.s.get(run..self.i).ok_or(())?);
            }
            if self.peek() == Some(b'"') {
                self.i += 1;
                return Ok(out);
            }
            self.i += 1; // `\`
            let c = self.peek().ok_or(())?;
            self.i += 1;
            let ch = match c {
                b'"' => '"',
                b'\\' => '\\',
                b'/' => '/',
                b'b' => '\u{8}',
                b'f' => '\u{c}',
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                b'u' => {
                    let hi = self.hex4()?;
                    let code = if (0xd800..0xdc00).contains(&hi) {
                        // A high surrogate must be followed by `\u` and a low one.
                        self.lit(b"\\u")?;
                        let lo = self.hex4()?;
                        if !(0xdc00..0xe000).contains(&lo) {
                            return Err(());
                        }
                        0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00)
                    } else if (0xdc00..0xe000).contains(&hi) {
                        // A lone surrogate escape denotes no Unicode text:
                        // step 2, `core/not_json` (core.md §4.5).
                        return Err(());
                    } else {
                        hi
                    };
                    char::from_u32(code).ok_or(())?
                }
                _ => return Err(()),
            };
            if keep {
                out.push(ch);
            }
        }
    }

    /// The index of the `"` that closes the string being read, found
    /// without consuming anything; the grammar is checked by `string`.
    fn string_end(&self) -> P<usize> {
        let mut j = self.i;
        loop {
            match self.b.get(j).ok_or(())? {
                b'"' => return Ok(j),
                b'\\' => j += 2,
                _ => j += 1,
            }
        }
    }

    /// A number (RFC 8259 §6). A fraction, an exponent or an integer outside
    /// ±(2^53 − 1) is valid JSON but no canonical value: `core/schema`.
    fn number(&mut self) -> P<Value> {
        let neg = self.peek() == Some(b'-');
        if neg {
            self.i += 1;
        }
        let digits = self.i;
        match self.peek().ok_or(())? {
            b'0' => self.i += 1,
            b'1'..=b'9' => {
                while let Some(b'0'..=b'9') = self.peek() {
                    self.i += 1;
                }
            }
            _ => return Err(()),
        }
        let int_end = self.i;
        let mut integer = true;
        if self.peek() == Some(b'.') {
            self.i += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(());
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.i += 1;
            }
            integer = false;
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.i += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.i += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(());
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.i += 1;
            }
            integer = false;
        }
        if !integer || int_end - digits > 16 {
            self.bad = true;
            return Ok(Value::Null);
        }
        let mut m: i64 = 0;
        for &d in &self.b[digits..int_end] {
            m = m * 10 + i64::from(d - b'0');
        }
        if m > MAX_INT {
            self.bad = true;
            return Ok(Value::Null);
        }
        Ok(Value::from(if neg { -m } else { m }))
    }
}

// ---------------------------------------------------------------- schema helpers
//
// Step 4 of §4.5 for the record schemas of this crate (and of `command`): a
// reader over one object that finds unknown fields, and one reader per
// shared field type of §4.4. Every failure is `core/schema`.

/// The fields of one JSON object, read once each; `done` fails on any field
/// not read (an unknown field, CORE-ENC-005).
pub(crate) struct Obj<'a> {
    map: &'a Map<String, Value>,
    used: usize,
}

impl<'a> Obj<'a> {
    pub(crate) fn new(v: &'a Value) -> Result<Obj<'a>, CoreError> {
        match v {
            Value::Object(map) => Ok(Obj { map, used: 0 }),
            _ => Err(schema()),
        }
    }

    pub(crate) fn req(&mut self, key: &str) -> Result<&'a Value, CoreError> {
        self.opt(key).ok_or_else(schema)
    }

    pub(crate) fn opt(&mut self, key: &str) -> Option<&'a Value> {
        let v = self.map.get(key)?;
        self.used += 1;
        Some(v)
    }

    pub(crate) fn done(self) -> Result<(), CoreError> {
        if self.used == self.map.len() { Ok(()) } else { Err(schema()) }
    }
}

/// An integer within `lo..=hi` (`int[a..b]`, `u53`, `id`).
pub(crate) fn int(v: &Value, lo: i64, hi: i64) -> Result<i64, CoreError> {
    match v {
        Value::Number(n) => match number_int(n) {
            Some(i) if lo <= i && i <= hi => Ok(i),
            _ => Err(schema()),
        },
        _ => Err(schema()),
    }
}

/// A `u53` or an `id` (core.md §4.4).
pub(crate) fn u53(v: &Value) -> Result<u64, CoreError> {
    int(v, 0, MAX_INT).map(|i| i.unsigned_abs())
}

pub(crate) fn boolean(v: &Value) -> Result<bool, CoreError> {
    v.as_bool().ok_or_else(schema)
}

pub(crate) fn string(v: &Value) -> Result<&str, CoreError> {
    v.as_str().ok_or_else(schema)
}

/// A string of `lo` to `hi` bytes.
pub(crate) fn string_bytes(v: &Value, lo: usize, hi: usize) -> Result<&str, CoreError> {
    let s = string(v)?;
    if (lo..=hi).contains(&s.len()) { Ok(s) } else { Err(schema()) }
}

pub(crate) fn array(v: &Value) -> Result<&Vec<Value>, CoreError> {
    v.as_array().ok_or_else(schema)
}

/// An `instant` (core.md §4.4, CORE-ENC-007).
pub(crate) fn instant(v: &Value) -> Result<Instant, CoreError> {
    Instant::parse_canonical(string(v)?).ok_or_else(schema)
}

/// A `date` (core.md §4.4).
pub(crate) fn date(v: &Value) -> Result<chrono::NaiveDate, CoreError> {
    let s = string(v)?.as_bytes();
    if s.len() != 10 {
        return Err(schema());
    }
    crate::time::parse_ymd(s).ok_or_else(schema)
}

/// The text of a `date` (core.md §4.4).
pub(crate) fn date_text(d: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    let mut out = String::with_capacity(10);
    crate::time::push_digits(&mut out, i64::from(d.year()), 4);
    out.push('-');
    crate::time::push_digits(&mut out, i64::from(d.month()), 2);
    out.push('-');
    crate::time::push_digits(&mut out, i64::from(d.day()), 2);
    out
}

/// An `rgba` (core.md §4.4).
pub(crate) fn rgba(v: &Value) -> Result<[u8; 4], CoreError> {
    let a = array(v)?;
    if a.len() != 4 {
        return Err(schema());
    }
    let mut out = [0u8; 4];
    for (o, e) in out.iter_mut().zip(a) {
        *o = u8::try_from(int(e, 0, 255)?).map_err(|_| schema())?;
    }
    Ok(out)
}

pub(crate) fn rgba_value(c: [u8; 4]) -> Value {
    Value::Array(c.iter().map(|&x| Value::from(x)).collect())
}

/// A `hexN` (core.md §4.4): exactly `2N` lowercase hexadecimal digits.
pub(crate) fn hex<const N: usize>(v: &Value) -> Result<[u8; N], CoreError> {
    let s = string(v)?.as_bytes();
    if s.len() != 2 * N {
        return Err(schema());
    }
    let digit = |c: u8| match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        _ => Err(schema()),
    };
    let mut out = [0u8; N];
    for (o, pair) in out.iter_mut().zip(s.chunks_exact(2)) {
        *o = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Ok(out)
}

pub(crate) fn hex_text(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 15)]));
    }
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_digit(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some(u32::from(c - b'A')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// The RFC 4648 §4 text, with padding, of `bytes`.
pub(crate) fn base64_text(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map_or(0, |&b| u32::from(b));
        let b2 = chunk.get(2).map_or(0, |&b| u32::from(b));
        let n = (b0 << 16) | (b1 << 8) | b2;
        let sym = |shift: u32| char::from(B64[((n >> shift) & 63) as usize]);
        out.push(sym(18));
        out.push(sym(12));
        out.push(if chunk.len() > 1 { sym(6) } else { '=' });
        out.push(if chunk.len() > 2 { sym(0) } else { '=' });
    }
    out
}

/// A `base64` field decoding to `lo` to `hi` bytes. The decoded length is
/// computed from the text and checked before anything is decoded
/// (CORE-GEN-009); only the one canonical text of the bytes is accepted
/// (padding present, unused bits zero).
pub(crate) fn base64(v: &Value, lo: usize, hi: usize) -> Result<Vec<u8>, CoreError> {
    let s = string(v)?.as_bytes();
    if s.is_empty() || s.len() % 4 != 0 {
        return Err(schema());
    }
    let pad = s.iter().rev().take_while(|&&c| c == b'=').count();
    if pad > 2 {
        return Err(schema());
    }
    let len = s.len() / 4 * 3 - pad;
    if !(lo..=hi).contains(&len) {
        return Err(schema());
    }
    let mut out = Vec::with_capacity(len);
    let body = &s[..s.len() - pad];
    for quad in body.chunks(4) {
        let mut n = 0u32;
        for &c in quad {
            n = (n << 6) | b64_digit(c).ok_or_else(schema)?;
        }
        match quad.len() {
            4 => out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]),
            3 => {
                // 18 bits: two bytes and two unused bits, which must be zero.
                if n & 3 != 0 {
                    return Err(schema());
                }
                out.extend_from_slice(&[(n >> 10) as u8, (n >> 2) as u8]);
            }
            2 => {
                // 12 bits: one byte and four unused bits, which must be zero.
                if n & 15 != 0 {
                    return Err(schema());
                }
                out.push((n >> 4) as u8);
            }
            _ => return Err(schema()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
