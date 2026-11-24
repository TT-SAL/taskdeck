//! Hashes, the fake frame signature, frame headers and error codes of the
//! model relay.

use sha2::{Digest, Sha256};

use crate::{Dev, Hash32, RelayRefusal};

/// One UTC day of the relay clock (`relay.md` §2).
pub(crate) const DAY: i64 = 86_400_000;
/// One MiB: the part size and the page's frame budget (`relay.md` §10.5, §10.7).
pub(crate) const MIB: usize = 1_048_576;
/// The largest integer the relay accepts in a request (RELAY-GEN-008).
pub(crate) const MAX_SAFE: u64 = (1 << 53) - 1;
/// The highest token generation (`relay.md` §5.2).
pub(crate) const MAX_GEN: u32 = u32::MAX;
/// New frames per role-1 author per UTC day (`relay.md` §14.1).
pub(crate) const APPEND_BUDGET: u32 = 3_000;
/// Log rows compaction and purge may delete per UTC day (`relay.md` §14.2).
pub(crate) const DELETE_BUDGET: u64 = 5_000;

/// SHA-256 of the concatenation of `parts`.
pub(crate) fn h(parts: &[&[u8]]) -> Hash32 {
    let mut d = Sha256::new();
    for p in parts {
        d.update(p);
    }
    d.finalize().into()
}

/// A context string of `seal.md` §4.1: `taskdeck/v1/<use>` and one `0x00`.
pub(crate) fn ctx(name: &str) -> Vec<u8> {
    let mut v = b"taskdeck/v1/".to_vec();
    v.extend_from_slice(name.as_bytes());
    v.push(0);
    v
}

pub(crate) fn first16(x: &Hash32) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(&x[..16]);
    out
}

/// `C_0` (`seal.md` §15.1).
pub(crate) fn chain_zero(board: &[u8; 16]) -> Hash32 {
    h(&[&ctx("chain"), board])
}

/// `C_s` from `C_{s-1}` (`seal.md` §15.1, SEAL-CHAIN-001).
pub(crate) fn chain_step(prev: &Hash32, seq: u64, frame_hash: &Hash32) -> Hash32 {
    h(&[&ctx("chain"), prev, &seq.to_be_bytes(), frame_hash])
}

/// The fake signature over `m` by `sign_pk` (engine.md §6.3), written from the
/// formula: `H("FAKE-SIG" ‖ pk ‖ m) ‖ H("FAKE-SIG2" ‖ pk ‖ m)`.
pub fn fake_sign(sign_pk: &Hash32, m: &[u8]) -> [u8; 64] {
    let a = h(&[b"FAKE-SIG", sign_pk, m]);
    let b = h(&[b"FAKE-SIG2", sign_pk, m]);
    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&a);
    sig[32..].copy_from_slice(&b);
    sig
}

/// Verification of a fake signature: recompute and compare without an early exit.
pub(crate) fn fake_verify(sign_pk: &Hash32, m: &[u8], sig: &[u8]) -> bool {
    let want = fake_sign(sign_pk, m);
    sig.len() == 64 && want.iter().zip(sig).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A fake public signing key from 32 secret bytes (engine.md §6.3).
pub fn fake_sign_pk(sk: &[u8; 32]) -> Hash32 {
    h(&[b"FAKE-PK", sk])
}

/// The fake `board_id` (engine.md §6.3): `H("FAKE-BOARD" ‖ rsign_pk ‖ salt)[0..16]`.
pub fn fake_board_id(rsign_pk: &Hash32, salt: &[u8; 16]) -> [u8; 16] {
    first16(&h(&[b"FAKE-BOARD", rsign_pk, salt]))
}

/// The fake `dev_R` (engine.md §6.3): `H("FAKE-REC" ‖ board_id ‖ rsign_pk)[0..16]`.
pub fn fake_dev_r(board: &[u8; 16], rsign_pk: &Hash32) -> Dev {
    first16(&h(&[b"FAKE-REC", board, rsign_pk]))
}

/// The bytes a frame's signature covers (`seal.md` §8.3).
pub(crate) fn frame_signed(frame: &[u8], len: usize) -> Vec<u8> {
    let mut m = ctx("frame");
    m.extend_from_slice(&frame[..48 + len]);
    m
}

/// Signs `frame[0 .. 48 + len]` with the fake signature and returns the whole
/// frame (`seal.md` §8.1): `header_and_body ‖ sig`.
pub fn fake_signed_frame(sign_pk: &Hash32, header_and_body: &[u8]) -> Vec<u8> {
    let mut m = ctx("frame");
    m.extend_from_slice(header_and_body);
    let mut out = header_and_body.to_vec();
    out.extend_from_slice(&fake_sign(sign_pk, &m));
    out
}

/// What P1 to P3 of `relay.md` §9.3 read from a frame.
pub(crate) struct FrameHead {
    pub dev: Dev,
    pub ctr: u64,
    pub len: usize,
}

/// P1 to P3 of `relay.md` §9.3, in order; the code of the first that fails.
pub(crate) fn frame_head(frame: &[u8], board: &[u8; 16]) -> Result<FrameHead, &'static str> {
    let n = frame.len();
    if !(670..=262_144).contains(&n) || &frame[..4] != b"TDF1" {
        return Err("relay/bad_frame");
    }
    let len = u32::from_be_bytes(frame[44..48].try_into().unwrap_or([0; 4])) as usize;
    if len + 112 != n {
        return Err("relay/bad_frame");
    }
    if &frame[4..20] != board {
        return Err("relay/wrong_board");
    }
    let ctr = u64::from_be_bytes(frame[36..44].try_into().unwrap_or([0; 8]));
    if ctr == 0 || ctr > MAX_SAFE {
        return Err("relay/bad_frame");
    }
    let mut dev = [0u8; 16];
    dev.copy_from_slice(&frame[20..36]);
    Ok(FrameHead { dev, ctr, len })
}

/// The HTTP status of a relay error code (`relay.md` §19).
pub fn status_of(code: &str) -> u16 {
    match code {
        "relay/unauthorized" | "relay/token_stale" | "relay/no_nonce" | "relay/bad_login_sig" | "relay/bad_admin_sig" => 401,
        "relay/forbidden" | "relay/fenced" | "relay/revoked" | "relay/not_signer" | "relay/not_author" => 403,
        "relay/no_board" | "relay/no_blob" | "relay/not_found" => 404,
        "relay/seeding" | "relay/exists" | "relay/gen_exhausted" | "relay/bad_gen" | "relay/conflict" | "relay/gap"
        | "relay/duplicate_key" | "relay/device_cap" | "relay/target" | "relay/last_device" | "relay/home_gen"
        | "relay/not_next" | "relay/chain_mismatch" | "relay/no_snapshot" | "relay/too_recent" | "relay/stale_epoch"
        | "relay/blob_exists" | "relay/blob_quota" | "relay/blob_incomplete" | "relay/part_conflict" => 409,
        "relay/budget" | "relay/delete_budget" => 429,
        "relay/storage_refused" => 507,
        _ => 400,
    }
}

/// A refusal, with the `last_ctr` and `last_seq` of a register or fence
/// frame's gap or conflict (§4.2), boxed to keep results small.
pub(crate) type Fail = Box<(RelayRefusal, Option<(u64, u64)>)>;

/// A refusal with the status its code has.
pub(crate) fn refuse(code: &str) -> RelayRefusal {
    RelayRefusal { status: status_of(code), code: code.to_string(), until: None, head: None, index: None }
}

/// The host rule of `seal.md` §5.4, as `relay-core.js` `validHost` checks it.
pub(crate) fn valid_host(host: &str) -> bool {
    let b = host.as_bytes();
    if b.is_empty() || b.len() > 255 {
        return false;
    }
    let (name, port) = match host.find(':') {
        Some(i) => (&host[..i], Some(&host[i + 1..])),
        None => (host, None),
    };
    if name.len() > 253 {
        return false;
    }
    let label_ok = |l: &str| {
        let lb = l.as_bytes();
        let alnum = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
        !lb.is_empty()
            && lb.len() <= 63
            && alnum(lb[0])
            && alnum(lb[lb.len() - 1])
            && lb.iter().all(|&c| alnum(c) || c == b'-')
    };
    if !name.split('.').all(label_ok) {
        return false;
    }
    if let Some(p) = port {
        let pb = p.as_bytes();
        if pb.is_empty() || pb.len() > 5 || pb[0] == b'0' || !pb.iter().all(u8::is_ascii_digit) {
            return false;
        }
        let v: u32 = p.parse().unwrap_or(0);
        if v > 65_535 || v == 443 {
            return false;
        }
    }
    true
}
