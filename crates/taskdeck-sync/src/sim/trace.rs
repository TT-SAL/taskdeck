//! The simulator's trace (engine.md §31.9): UTF-8 JSON Lines, one object per
//! line, keys in the order the table lists, integers only, byte strings as
//! lowercase hex. `trace_sha256` is SHA-256 of the whole file (ENG-SIM-003).

use sha2::{Digest, Sha256};

/// One value of a trace object.
pub(crate) enum V<'a> {
    I(i128),
    S(&'a str),
    Hex(&'a [u8]),
    Hexes(&'a [[u8; 32]]),
    Ints(&'a [i64]),
}

/// The trace being written: its hash always, its bytes when kept.
pub(crate) struct Trace {
    hasher: Sha256,
    keep: bool,
    bytes: Vec<u8>,
    lines: u64,
    line: String,
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn push_hex(s: &mut String, b: &[u8]) {
    s.push('"');
    for x in b {
        s.push(HEX[(x >> 4) as usize] as char);
        s.push(HEX[(x & 15) as usize] as char);
    }
    s.push('"');
}

fn push_str(s: &mut String, v: &str) {
    s.push('"');
    for c in v.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                s.push_str("\\u00");
                s.push(HEX[(c as usize) >> 4] as char);
                s.push(HEX[(c as usize) & 15] as char);
            }
            c => s.push(c),
        }
    }
    s.push('"');
}

impl Trace {
    pub(crate) fn new(keep: bool) -> Self {
        Trace { hasher: Sha256::new(), keep, bytes: Vec::new(), lines: 0, line: String::with_capacity(256) }
    }

    /// Writes one line `{"t": t, "k": k, <fields>}` and returns its number (1-based).
    pub(crate) fn line(&mut self, t: u64, k: &str, fields: &[(&str, V<'_>)]) -> u64 {
        use core::fmt::Write;
        let s = &mut self.line;
        s.clear();
        let _ = write!(s, "{{\"t\":{t},\"k\":");
        push_str(s, k);
        for (key, v) in fields {
            s.push(',');
            push_str(s, key);
            s.push(':');
            match v {
                V::I(i) => {
                    let _ = write!(s, "{i}");
                }
                V::S(x) => push_str(s, x),
                V::Hex(b) => push_hex(s, b),
                V::Hexes(hs) => {
                    s.push('[');
                    for (i, h) in hs.iter().enumerate() {
                        if i > 0 {
                            s.push(',');
                        }
                        push_hex(s, h);
                    }
                    s.push(']');
                }
                V::Ints(xs) => {
                    s.push('[');
                    for (i, x) in xs.iter().enumerate() {
                        if i > 0 {
                            s.push(',');
                        }
                        let _ = write!(s, "{x}");
                    }
                    s.push(']');
                }
            }
        }
        s.push_str("}\n");
        self.hasher.update(s.as_bytes());
        if self.keep {
            self.bytes.extend_from_slice(s.as_bytes());
        }
        self.lines += 1;
        self.lines
    }

    /// The number of lines written.
    pub(crate) fn lines(&self) -> u64 {
        self.lines
    }

    /// The SHA-256 of the whole trace and its bytes (empty unless kept).
    pub(crate) fn finish(self) -> ([u8; 32], Vec<u8>) {
        (self.hasher.finalize().into(), self.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // engine.md §31.9: keys in order, integers, lowercase hex, one object per line.
    #[test]
    fn a_line_has_the_keys_in_order() {
        let mut t = Trace::new(true);
        let n = t.line(5, "alarm", &[("dev", V::I(1)), ("kind", V::S("fork")), ("seq", V::I(7))]);
        t.line(6, "out", &[("frames", V::Hexes(&[[0xab; 32]])), ("p", V::Ints(&[-1, 2])), ("h", V::Hex(&[0, 255]))]);
        assert_eq!(n, 1);
        let (_, bytes) = t.finish();
        let text = String::from_utf8(bytes).unwrap();
        let mut lines = text.lines();
        assert_eq!(lines.next().unwrap(), r#"{"t":5,"k":"alarm","dev":1,"kind":"fork","seq":7}"#);
        let second = lines.next().unwrap();
        assert!(second.starts_with(r#"{"t":6,"k":"out","frames":["abab"#));
        assert!(second.ends_with(r#""p":[-1,2],"h":"00ff"}"#));
    }
}
