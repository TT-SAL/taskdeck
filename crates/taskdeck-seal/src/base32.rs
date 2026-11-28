//! Crockford base32 (seal.md §5.2, §26).
//!
//! Besides the public `encode` and `decode_typed`, the crate's other modules
//! use the private helpers below: encoding into a zeroizing string, encoding
//! and decoding bit strings that are not whole bytes (the 20-bit check of
//! §18.1, the 10-bit check of §17.2), and the normalisation of typed text.

use zeroize::Zeroizing;

/// The alphabet, value 0 to 31 (seal.md §5.2).
pub(crate) const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Marks a normalised character that is not in the alphabet.
const INVALID: u8 = 0xFF;

/// Encodes bytes in the alphabet of §5.2 (seal.md §26).
///
/// Only uppercase alphabet characters, no separators (SEAL-ENC-001).
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(chars_for_bytes(bytes.len()));
    encode_into(bytes, usize::MAX, &mut out);
    out
}

/// Decodes typed text of `bytes` bytes, with §5.2's normalisation (seal.md §26).
///
/// Every failure (a length other than `ceil(8 × bytes / 5)` characters after
/// normalising, a character outside the alphabet, padding bits that are not
/// zero) is `seal/code_format`.
// SPEC-QUESTION: §22 has no code of its own for `decode_typed`; this takes
// `seal/code_format` (the code of a typed code of the wrong shape, §17.2).
// The callers with codes of their own (the sheet, the move code, the pairing
// code) map the failure to theirs.
pub fn decode_typed(text: &str, bytes: usize) -> Result<Vec<u8>, super::SealError> {
    let out = decode_typed_zeroizing(text, bytes).ok_or(super::SealError::CodeFormat)?;
    Ok(out.to_vec())
}

/// `decode_typed` into a zeroizing buffer, `None` on any failure.
pub(crate) fn decode_typed_zeroizing(text: &str, bytes: usize) -> Option<Zeroizing<Vec<u8>>> {
    let nbits = bytes.checked_mul(8)?;
    let values = normalise(text, nbits.div_ceil(5)).ok()?;
    decode_bits(&values, nbits)
}

/// `ceil(8n / 5)` without overflow.
fn chars_for_bytes(n: usize) -> usize {
    // ceil(8n/5) = n + ceil(3n/5); computed as n/5*8 + ceil(8(n%5)/5).
    (n / 5).saturating_mul(8).saturating_add((n % 5 * 8).div_ceil(5))
}

/// Appends the encoding of `bytes` to `out`, at most `max_chars` characters.
fn encode_into(bytes: &[u8], max_chars: usize, out: &mut String) {
    let mut acc: u32 = 0;
    let mut nacc: u32 = 0;
    let mut written = 0usize;
    for &b in bytes {
        acc = (acc << 8) | u32::from(b);
        nacc += 8;
        while nacc >= 5 {
            if written == max_chars {
                return;
            }
            nacc -= 5;
            push_value(out, (acc >> nacc) & 31);
            written += 1;
        }
        acc &= (1 << nacc) - 1;
    }
    if nacc > 0 && written < max_chars {
        push_value(out, (acc << (5 - nacc)) & 31);
    }
}

fn push_value(out: &mut String, v: u32) {
    let c = ALPHABET.get(v as usize).copied().unwrap_or(b'0');
    out.push(char::from(c));
}

/// Encodes `bytes` into a zeroizing string whose capacity is reserved first,
/// so that no copy is left behind by a reallocation (SEAL-SHEET-003).
pub(crate) fn encode_zeroizing(bytes: &[u8]) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(chars_for_bytes(bytes.len())));
    encode_into(bytes, usize::MAX, &mut out);
    out
}

/// Encodes the first `nbits` bits of `bytes` (most significant bit of byte 0
/// first), with zero bits appended on the right to a whole character:
/// `ceil(nbits / 5)` characters. Bits past the end of `bytes` count as zero.
pub(crate) fn encode_bits(bytes: &[u8], nbits: usize) -> Zeroizing<String> {
    let nbytes = nbits.div_ceil(8).min(bytes.len());
    let mut masked = Zeroizing::new(bytes.get(..nbytes).unwrap_or_default().to_vec());
    let keep = nbits % 8;
    if keep != 0
        && nbytes == nbits.div_ceil(8)
        && let Some(last) = masked.last_mut()
    {
        *last &= 0xFFu8 << (8 - keep);
    }
    let nchars = nbits.div_ceil(5);
    let mut out = Zeroizing::new(String::with_capacity(nchars));
    encode_into(&masked, nchars, &mut out);
    while out.len() < nchars {
        out.push('0');
    }
    out
}

/// Why typed text failed normalisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NormError {
    /// Not exactly the expected number of characters after normalising.
    Length,
    /// A character outside the alphabet after normalising.
    Char,
}

/// Normalises typed text as §5.2 lists, in order: (1) remove spaces,
/// hyphens and tabs; (2) fold `a`–`z` to `A`–`Z`; (3) map `O` to `0`, `I`
/// and `L` to `1`. Then requires exactly `expected` characters (checked
/// before anything is allocated), each in the alphabet; returns their values.
pub(crate) fn normalise(text: &str, expected: usize) -> Result<Zeroizing<Vec<u8>>, NormError> {
    let kept = || text.chars().filter(|c| !matches!(c, ' ' | '-' | '\t'));
    if kept().count() != expected {
        return Err(NormError::Length);
    }
    let mut out = Zeroizing::new(Vec::with_capacity(expected));
    for c in kept() {
        out.push(value_of(c));
    }
    if out.contains(&INVALID) {
        return Err(NormError::Char);
    }
    Ok(out)
}

/// The value of one character after steps (2) and (3) of §5.2.
fn value_of(c: char) -> u8 {
    let c = c.to_ascii_uppercase();
    let c = match c {
        'O' => '0',
        'I' | 'L' => '1',
        other => other,
    };
    if !c.is_ascii() {
        return INVALID;
    }
    ALPHABET
        .iter()
        .position(|&a| char::from(a) == c)
        .and_then(|p| u8::try_from(p).ok())
        .unwrap_or(INVALID)
}

/// Decodes alphabet values (from `normalise`) into the `nbits` bits they
/// carry, as `ceil(nbits / 8)` bytes whose unused low bits are zero. `None`
/// when the count is not `ceil(nbits / 5)` or a padding bit is not zero.
pub(crate) fn decode_bits(values: &[u8], nbits: usize) -> Option<Zeroizing<Vec<u8>>> {
    if values.len() != nbits.div_ceil(5) {
        return None;
    }
    let mut out = Zeroizing::new(vec![0u8; nbits.div_ceil(8)]);
    let mut pos = 0usize;
    for &v in values {
        if v > 31 {
            return None;
        }
        for b in 0..5u32 {
            let bit = (v >> (4 - b)) & 1;
            if pos < nbits {
                let byte = out.get_mut(pos / 8)?;
                *byte |= bit << (7 - (pos % 8));
            } else if bit != 0 {
                return None;
            }
            pos += 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests;
