//! The relay's layouts and JSON answers as the device reads and writes them
//! (relay.md §4, §5.4, §9.2, §9.6, §10, §12.1; device.md §10.5): every body
//! parsed strictly before it becomes an event (DEV-NET-009), every request
//! body laid out byte for byte.

use serde_json::Value;
use taskdeck_sync::{AppendResult, Head, Page, PageEntry, RelayRefusal, Welcome};

use crate::strict_json::{self, Obj, hex_exact};

/// A frame's length bounds (relay.md §9.2).
pub(crate) const FRAME_MIN: usize = 670;
pub(crate) const FRAME_MAX: usize = 262_144;
/// At most 16 frames per batch (relay.md §9.2).
pub(crate) const BATCH_MAX: usize = 16;
/// The largest JSON answer the device reads (the largest table, `blob-list`
/// with 64 objects, is under 32 KiB).
pub(crate) const JSON_MAX: usize = 65_536;
/// A page is at most 71 + 256 × 20 bytes of headers plus 1 MiB of frames
/// plus one frame of 256 KiB (relay.md §10.5: the first row is always taken).
pub(crate) const PAGE_MAX: usize = 71 + 256 * 20 + 1_048_576 + FRAME_MAX;
/// Results of 16 frames, each at most 13 + 262,144 bytes (relay.md §9.6).
pub(crate) const RESULTS_MAX: usize = 5 + BATCH_MAX * (13 + FRAME_MAX);
/// A blob part (relay.md §10.7).
pub(crate) const PART_MAX: usize = 1_048_576;
/// A pairing slot: at most 36 + 16,040 bytes (relay.md §10.8).
pub(crate) const SLOT_MAX: usize = 16_076;

const MAX_INT: u64 = (1 << 53) - 1;

/// `"TDA1" ‖ u8 n ‖ n × (u32 len ‖ frame)` (relay.md §9.2).
pub(crate) fn batch(frames: &[Vec<u8>]) -> Option<Vec<u8>> {
    if frames.is_empty() || frames.len() > BATCH_MAX {
        return None;
    }
    let mut out = Vec::with_capacity(5 + frames.iter().map(|f| 4 + f.len()).sum::<usize>());
    out.extend_from_slice(b"TDA1");
    out.push(frames.len() as u8);
    for f in frames {
        if !(FRAME_MIN..=FRAME_MAX).contains(&f.len()) {
            return None;
        }
        out.extend_from_slice(&(f.len() as u32).to_be_bytes());
        out.extend_from_slice(f);
    }
    Some(out)
}

/// A bounds-checked reader.
pub(crate) struct Rd<'a>(pub(crate) &'a [u8]);

impl<'a> Rd<'a> {
    pub(crate) fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    pub(crate) fn arr<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }
    pub(crate) fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    pub(crate) fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.arr()?))
    }
    pub(crate) fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.arr()?))
    }
    pub(crate) fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.arr()?))
    }
    pub(crate) fn i64(&mut self) -> Option<i64> {
        Some(i64::from_be_bytes(self.arr()?))
    }
    pub(crate) fn done(&self) -> bool {
        self.0.is_empty()
    }
}

/// The results body of relay.md §9.6, for a batch of `n` frames.
pub(crate) fn parse_results(body: &[u8], n: usize) -> Option<Vec<AppendResult>> {
    let mut r = Rd(body);
    if r.take(4)? != b"TDR1" || usize::from(r.u8()?) != n {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let res = match r.u8()? {
            0 => AppendResult::New { seq: seq(r.u64()?)?, chain: r.arr()? },
            1 => AppendResult::Dup { seq: seq(r.u64()?)?, chain: r.arr()? },
            2 => {
                let s = r.u64()?;
                let flen = r.u32()? as usize;
                if s == 0 && flen != 0 || s != 0 && !(FRAME_MIN..=FRAME_MAX).contains(&flen) || s > MAX_INT {
                    return None;
                }
                AppendResult::Conflict { seq: s, frame: r.take(flen)?.to_vec() }
            }
            3 => {
                let last_ctr = r.u64()?;
                let last_seq = r.u64()?;
                if last_ctr > MAX_INT || last_seq > MAX_INT {
                    return None;
                }
                AppendResult::Gap { last_ctr, last_seq }
            }
            4 => {
                let status = r.u16()?;
                let len = usize::from(r.u8()?);
                let code = std::str::from_utf8(r.take(len)?).ok()?;
                if !is_code(code) {
                    return None;
                }
                AppendResult::Refused { status, code: code.to_string() }
            }
            5 => AppendResult::Skipped,
            _ => return None,
        };
        out.push(res);
    }
    r.done().then_some(out)
}

fn seq(s: u64) -> Option<u64> {
    (1..=MAX_INT).contains(&s).then_some(s)
}

/// An error code of the catalogues: `<spec>/<name>`, ASCII lowercase,
/// digits and `_` (relay.md §19).
pub(crate) fn is_code(c: &str) -> bool {
    let Some((a, b)) = c.split_once('/') else { return false };
    let ok = |s: &str| !s.is_empty() && s.bytes().all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || x == b'_');
    c.len() <= 64 && ok(a) && ok(b)
}

/// The page of relay.md §10.5.
pub(crate) fn parse_page(body: &[u8]) -> Option<Page> {
    let mut r = Rd(body);
    if r.take(4)? != b"TDP1" {
        return None;
    }
    let head = Head { seq: lim(r.u64()?)?, chain: r.arr()? };
    let relay_now = r.i64()?;
    let floor = lim(r.u64()?)?;
    let purge_floor = lim(r.u64()?)?;
    let flags = r.u8()?;
    if flags & !0b111 != 0 {
        return None;
    }
    let n = r.u16()?;
    if n > 256 {
        return None;
    }
    let mut entries = Vec::with_capacity(usize::from(n));
    for _ in 0..n {
        let s = seq(r.u64()?)?;
        let at = r.i64()?;
        let flen = r.u32()? as usize;
        if !(FRAME_MIN..=FRAME_MAX).contains(&flen) {
            return None;
        }
        if entries.last().is_some_and(|e: &PageEntry| e.seq >= s) {
            return None;
        }
        entries.push(PageEntry { seq: s, at, frame: r.take(flen)?.to_vec() });
    }
    if !r.done() {
        return None;
    }
    Some(Page {
        head,
        relay_now,
        floor,
        purge_floor,
        more: flags & 1 != 0,
        truncated: flags & 2 != 0,
        fenced: flags & 4 != 0,
        entries,
    })
}

fn lim(n: u64) -> Option<u64> {
    (n <= MAX_INT).then_some(n)
}

/// A JSON answer: strict (README §5.3), an object.
pub(crate) fn json_obj(body: &[u8]) -> Option<Obj> {
    if body.len() > JSON_MAX {
        return None;
    }
    Obj::new(strict_json::parse_bytes(body, 8)?)
}

fn head_of(v: Value) -> Option<Head> {
    let mut o = Obj::new(v)?;
    let h = Head { seq: o.u53("seq")?, chain: o.hex::<32>("chain")? };
    o.finish()?;
    Some(h)
}

/// The `hello` answer (relay.md §10.4) as a `Welcome` (device.md §10.5).
pub(crate) fn parse_hello(body: &[u8]) -> Option<Welcome> {
    let mut o = json_obj(body)?;
    let head = head_of(o.take("head")?)?;
    let last_ctr = o.opt_u53("last_ctr").ok()?;
    let last_seq = o.opt_u53("last_seq").ok()?;
    let appends_left = match o.opt_u53("appends_left").ok()? {
        None => None,
        Some(n) if n <= 3_000 => Some(n as u32),
        Some(_) => return None,
    };
    let relay_now = o.i53("relay_now")?;
    let floor = o.u53("floor")?;
    let purge_floor = o.u53("purge_floor")?;
    let fenced = match o.string("state")?.as_str() {
        "open" => false,
        "fenced" => true,
        _ => return None,
    };
    let fence = match o.take("fence") {
        None => None,
        Some(v) => {
            let mut f = Obj::new(v)?;
            let host = f.string("host")?;
            let door = f.hex::<5>("door")?;
            let home_gen = u32::try_from(f.u53("home_gen")?).ok()?;
            f.finish()?;
            Some((host, door, home_gen))
        }
    };
    if fence.is_some() != fenced {
        return None;
    }
    let _door: [u8; 5] = o.hex("door")?;
    let push = o.boolean("push")?;
    o.finish()?;
    Some(Welcome { head, last_ctr, last_seq, relay_now, floor, purge_floor, fenced, fence, appends_left, push })
}

/// An error body (relay.md §4.2) as a `RelayRefusal`.
pub(crate) fn parse_refusal(status: u16, body: &[u8]) -> Option<RelayRefusal> {
    let mut o = json_obj(body)?;
    let code = o.string("error")?;
    if !is_code(&code) {
        return None;
    }
    let until = match o.take("until") {
        None => None,
        Some(v) => Some(v.as_i64().filter(|n| *n >= 0 && n.unsigned_abs() <= MAX_INT)?),
    };
    let index = match o.opt_u53("index").ok()? {
        None => None,
        Some(n) if n <= 255 => Some(n as u32),
        Some(_) => return None,
    };
    let head = match o.take("head") {
        None => None,
        Some(v) => Some(head_of(v)?),
    };
    // Parsed and not passed on (device.md §10.6).
    o.opt_u53("read_epoch").ok()?;
    let last_ctr = o.opt_u53("last_ctr").ok()?;
    let last_seq = o.opt_u53("last_seq").ok()?;
    if last_ctr.is_some() != last_seq.is_some() {
        return None;
    }
    o.finish()?;
    Some(RelayRefusal { status, code, until, head, index })
}

/// A relay token's text form (relay.md §5.1): `TD1.` and 92 base64url
/// characters. The device never parses it further (SEAL-KEY-015).
pub(crate) fn is_token(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() == 96
        && b.starts_with(b"TD1.")
        && b[4..].iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_')
}

/// The admin envelope of relay.md §12.1.
pub(crate) fn admin_envelope(signer: &[u8; 16], nonce: &[u8; 32], sig: &[u8; 64], args: &[u8], extra: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(120 + args.len() + extra.len());
    o.extend_from_slice(signer);
    o.extend_from_slice(nonce);
    o.extend_from_slice(sig);
    o.extend_from_slice(&(args.len() as u32).to_be_bytes());
    o.extend_from_slice(args);
    o.extend_from_slice(&(extra.len() as u32).to_be_bytes());
    o.extend_from_slice(extra);
    o
}

// ---------------------------------------------------------------- the relay address (relay.md §5.4)

/// `B` of relay.md §5.4: Crockford base32 of `board_id ‖ door`.
fn address_b(board: &[u8; 16], door: &[u8; 5]) -> String {
    let mut b = [0u8; 21];
    b[..16].copy_from_slice(board);
    b[16..].copy_from_slice(door);
    taskdeck_seal::base32::encode(&b)
}

/// The canonical text form `host "/" B` (RELAY-ADDR-001).
pub(crate) fn address_text(host: &str, board: &[u8; 16], door: &[u8; 5]) -> String {
    format!("{host}/{}", address_b(board, door))
}

/// The display form: `B` in groups of 4 joined by `-`, the last of 2.
pub(crate) fn address_display(host: &str, board: &[u8; 16], door: &[u8; 5]) -> String {
    let b = address_b(board, door);
    let groups: Vec<&str> = b.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap_or("")).collect();
    format!("{host}/{}", groups.join("-"))
}

/// A host after relay.md §5.4 step 4 (letters to lowercase, a trailing
/// `:443` removed) that passes `seal.md` §5.4; else `relay/address`.
pub(crate) fn normalise_host(host: &str) -> Result<String, &'static str> {
    let mut h = host.to_ascii_lowercase();
    if let Some(s) = h.strip_suffix(":443") {
        h = s.to_string();
    }
    taskdeck_seal::relay_origin(&h).map_err(|_| ADDRESS)?;
    Ok(h)
}

pub(crate) const ADDRESS: &str = "relay/address";

/// Parses a typed or pasted relay address by the six steps of relay.md §5.4
/// (RELAY-ADDR-002): `(host, board_id, door)`.
pub(crate) fn parse_address(text: &str) -> Result<(String, [u8; 16], [u8; 5]), &'static str> {
    // 1.
    let t = text.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'));
    // 2.
    let t = match t.get(..8) {
        Some(p) if p.eq_ignore_ascii_case("https://") => &t[8..],
        _ if t.contains("://") => return Err(ADDRESS),
        _ => t,
    };
    // 3.
    let (host, b) = t.split_once('/').ok_or(ADDRESS)?;
    if b.contains('/') {
        return Err(ADDRESS);
    }
    // 4.
    let host = normalise_host(host)?;
    // 5, 6.
    let bytes = taskdeck_seal::base32::decode_typed(b, 21).map_err(|_| ADDRESS)?;
    let board: [u8; 16] = bytes[..16].try_into().map_err(|_| ADDRESS)?;
    let door: [u8; 5] = bytes[16..21].try_into().map_err(|_| ADDRESS)?;
    Ok((host, board, door))
}

/// Whether a host is a loopback name that `test-trust` reaches over `http`
/// (device.md §11.1 item 1). `[::1]` is not a host of `seal.md` §5.4.
pub(crate) fn is_loopback(host: &str) -> bool {
    let name = host.split_once(':').map_or(host, |(n, _)| n);
    name == "127.0.0.1" || name == "localhost"
}

/// The JSON answer's hex field of exactly `N` bytes.
pub(crate) fn hex_field<const N: usize>(o: &mut Obj, k: &str) -> Option<[u8; N]> {
    hex_exact::<N>(o.take(k)?.as_str()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_round_trips_and_parses_typed_forms() {
        let board = [7u8; 16];
        let door = [1, 2, 3, 4, 5];
        let text = address_text("relay.example:8443", &board, &door);
        assert_eq!(parse_address(&text), Ok(("relay.example:8443".into(), board, door)));
        let shown = address_display("relay.example", &board, &door);
        assert_eq!(parse_address(&format!(" HTTPS://Relay.Example:443/{}\n", &shown[14..])), Ok(("relay.example".into(), board, door)));
        assert_eq!(parse_address(&format!("http://{text}")), Err(ADDRESS));
        assert_eq!(parse_address("relay.example"), Err(ADDRESS));
        assert_eq!(parse_address(&format!("{text}/x")), Err(ADDRESS));
    }

    #[test]
    fn a_page_and_results_parse_strictly() {
        let mut p = b"TDP1".to_vec();
        p.extend_from_slice(&3u64.to_be_bytes());
        p.extend_from_slice(&[9; 32]);
        p.extend_from_slice(&5i64.to_be_bytes());
        p.extend_from_slice(&1u64.to_be_bytes());
        p.extend_from_slice(&1u64.to_be_bytes());
        p.push(1);
        p.extend_from_slice(&1u16.to_be_bytes());
        p.extend_from_slice(&3u64.to_be_bytes());
        p.extend_from_slice(&4i64.to_be_bytes());
        p.extend_from_slice(&(670u32).to_be_bytes());
        p.extend_from_slice(&[0; 670]);
        let page = parse_page(&p).expect("parses");
        assert!(page.more && !page.truncated && page.entries[0].seq == 3);
        let mut longer = p.clone();
        longer.push(0);
        assert!(parse_page(&longer).is_none());
        let mut r = b"TDR1".to_vec();
        r.push(2);
        r.push(0);
        r.extend_from_slice(&4u64.to_be_bytes());
        r.extend_from_slice(&[1; 32]);
        r.push(4);
        r.extend_from_slice(&429u16.to_be_bytes());
        r.push(12);
        r.extend_from_slice(b"relay/budget");
        let res = parse_results(&r, 2).expect("parses");
        assert_eq!(res[1], AppendResult::Refused { status: 429, code: "relay/budget".into() });
        assert!(parse_results(&r, 3).is_none());
    }

    #[test]
    fn a_refusal_and_a_hello_parse_strictly() {
        let r = parse_refusal(409, br#"{"error":"relay/not_next","head":{"seq":2,"chain":"0000000000000000000000000000000000000000000000000000000000000000"}}"#);
        assert_eq!(r.map(|r| (r.code, r.head.map(|h| h.seq))), Some(("relay/not_next".into(), Some(2))));
        assert!(parse_refusal(400, br#"{"error":"relay/x","extra":1}"#).is_none());
        let h = br#"{"head":{"seq":1,"chain":"0000000000000000000000000000000000000000000000000000000000000000"},"last_ctr":0,"last_seq":0,"appends_left":3000,"relay_now":5,"floor":1,"purge_floor":1,"state":"open","door":"0102030405","push":false}"#;
        let w = parse_hello(h).expect("parses");
        assert_eq!((w.head.seq, w.last_ctr, w.fenced, w.push), (1, Some(0), false, false));
        assert!(parse_hello(&h[..h.len() - 1]).is_none());
    }
}
