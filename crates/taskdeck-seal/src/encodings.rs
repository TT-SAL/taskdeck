//! Shared encodings (seal.md §5, §26).

use crate::SealError;

/// The longest host, `:port` included (seal.md §5.4, §23).
pub(crate) const HOST_MAX: usize = 255;
/// The longest host name before any `:` (seal.md §5.4).
const HOST_NAME_MAX: usize = 253;
/// The longest label of a host name (seal.md §5.4).
const LABEL_MAX: usize = 63;
/// The longest device name in bytes (seal.md §5.5, §23).
pub(crate) const DEVICE_NAME_MAX: usize = 64;

/// `"https://" ‖ host`, or `seal/host_format` (seal.md §5.4, SEAL-ENC-006, §26).
pub fn relay_origin(host: &str) -> Result<String, SealError> {
    if !host_is_valid(host) {
        return Err(SealError::HostFormat);
    }
    let mut out = String::with_capacity(8 + host.len());
    out.push_str("https://");
    out.push_str(host);
    Ok(out)
}

/// Whether `host` is a host of §5.4 (SEAL-ENC-006): the regular expression
/// `^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*(:[1-9][0-9]{0,4})?$`,
/// at most 253 bytes before any `:`, a port of at most 65535 that is not
/// 443, and at most 255 bytes in all. The callers of tasks S2 to S5 map a
/// `false` to the code of their own step.
pub(crate) fn host_is_valid(host: &str) -> bool {
    if host.is_empty() || host.len() > HOST_MAX {
        return false;
    }
    let (name, port) = match host.split_once(':') {
        Some((n, p)) => (n, Some(p)),
        None => (host, None),
    };
    if name.is_empty() || name.len() > HOST_NAME_MAX {
        return false;
    }
    if !name.split('.').all(label_is_valid) {
        return false;
    }
    match port {
        None => true,
        Some(p) => port_is_valid(p),
    }
}

fn label_is_valid(label: &str) -> bool {
    let b = label.as_bytes();
    let (Some(&first), Some(&last)) = (b.first(), b.last()) else {
        return false;
    };
    b.len() <= LABEL_MAX
        && first != b'-'
        && last != b'-'
        && b.iter()
            .all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn port_is_valid(p: &str) -> bool {
    let b = p.as_bytes();
    if b.is_empty() || b.len() > 5 || b.first() == Some(&b'0') || !b.iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    match p.parse::<u32>() {
        Ok(n) => (1..=65535).contains(&n) && n != 443,
        Err(_) => false,
    }
}

/// Whether `bytes` is a device name of §5.5 (SEAL-ENC-007): 1 to 64 bytes
/// of UTF-8 with no character in `U+0000`–`U+001F` or `U+007F`–`U+009F`.
/// The callers of tasks S2 to S5 map a `false` to the code of their own step.
pub(crate) fn device_name_is_valid(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > DEVICE_NAME_MAX {
        return false;
    }
    match core::str::from_utf8(bytes) {
        Ok(s) => !s
            .chars()
            .any(|c| matches!(c, '\u{0000}'..='\u{001F}' | '\u{007F}'..='\u{009F}')),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests;
