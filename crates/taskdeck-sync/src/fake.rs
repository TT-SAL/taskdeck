//! The fake sealer of the simulator (engine.md §6.3), compiled only with the
//! feature `sim` (ENG-TOY-002).
//!
//! It is `seal.md` §5, §8 to §15 and §20 with the substitutions of engine.md
//! §6.3 and nothing else changed: the frame, body, plaintext and padding
//! layouts, the checks of §14.2 in their order, the shapes and transitions of
//! §12 and §13, the canonical membership bytes of §13.2 and their parser's
//! state checks, the chain of §15.1, the snapshot blob of §20 and the
//! projection of §21.6 that checks a snapshot's history hold as written.
//! Every random byte comes from one seeded SplitMix64 generator in call order
//! (ENG-FAKE-002). Every integer is big-endian, every map is a `BTreeMap` and
//! no `usize` is encoded, so the bytes do not depend on the platform.
//! `docs/sync/status/engine/api-fake-toy.md` describes what the engine and the
//! simulator call; `tests/vectors/engine-fake.json` holds its bytes.
//!
//! Where §6.3 and `seal.md` leave a choice, this module takes these:
//! - A wrap draws its 32 bytes (`seal.md` §7, SEAL-WRAP-004) and discards
//!   them, so the generator's position matches the real sealer's draw order
//!   (§6.1 `seal`); the genesis helper does the same for its two wraps, after
//!   the entry id and before the frame's nonce, and so does `reinstate_wraps`.
//! - "H(items bytes)" of the inner control signature, and a history record's
//!   `items`, are the bytes `u8 n_items ‖ items` of the fake `ctl` payload.
//! - Rules versions above 1 allow what rules version 1 allows (`seal.md` §14.3
//!   lists only version 1): `(1, 1)` and state-hash version 1 (ENG-FAKE-003).
//! - The genesis helper composes seq 1 at `at_ms = 0`; `create_at` takes
//!   another time. A fake made by `create` or `join` supports rules `[1]`
//!   unless the simulator sets another list (`set_supported_rules`); both
//!   setters are crate-private (engine.md §6.3).
//! - `M_0` (`seal.md` §13.3) is `FakeMembership::default()`.
//! - `seal` refuses `ctr = 0` with `seal/frame_format`, the code
//!   SEAL-FRAME-004 names, and an unencodable item (a device name outside
//!   §5.5) with `seal/bad_payload`.
//! - `join` keeps the bundle's roots whose commitment the snapshot's
//!   membership records (SEAL-KEY-013).
//! - The fake signature needs only the signer's public key, so any fake of a
//!   board can seal as its recovery identity (`seal_as_recovery`,
//!   `seal_recovery_key_change`), as the recovery tool does for the
//!   simulator's workload (engine.md §31.5); such frames keep no pending root.
//! - The simulator reaches the fake also through the crate-private
//!   `create_at`, `join_keys`, `bundle`, `set_supported_rules`,
//!   `seal_as_recovery`, `seal_recovery_key_change` and `reinstate_wraps`, so
//!   the public surface stays engine.md §37's.

use std::cell::RefCell;
use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::sim::SplitMix64;
use crate::toy::{ToyMachine, ToyState};
use crate::{
    BeatInfo, Body, Class, ClassifyInput, CtlIntent, CtlItem, Dev, DeviceStatus, DeviceView, Halt, Hash32, Head, Kind,
    MembershipView, Opened, PlainEntry, Receipt, RecoveryView, Sealed, SealedBlob,
    Sealer, SealerError, SnapInfo, SnapshotMaterial, StateMachine, UpgradeMode, Verdict,
};

/// The error and receipt codes this module returns (`seal.md` §22).
mod code {
    pub const FRAME_FORMAT: &str = "seal/frame_format";
    pub const WRONG_BOARD: &str = "seal/wrong_board";
    pub const UNKNOWN_AUTHOR: &str = "seal/unknown_author";
    pub const BAD_SIGNATURE: &str = "seal/bad_signature";
    pub const COUNTER_REPLAY: &str = "seal/counter_replay";
    pub const COUNTER_GAP: &str = "seal/counter_gap";
    pub const REVOKED_AUTHOR: &str = "seal/revoked_author";
    pub const BAD_VERSION: &str = "seal/bad_version";
    pub const BAD_EPOCH: &str = "seal/bad_epoch";
    pub const OLD_EPOCH: &str = "seal/old_epoch";
    pub const MISSING_ROOT: &str = "seal/missing_root";
    pub const AEAD: &str = "seal/aead";
    pub const BAD_PLAINTEXT: &str = "seal/bad_plaintext";
    pub const BASED_ON_FUTURE: &str = "seal/based_on_future";
    pub const BASED_ON_FORK: &str = "seal/based_on_fork";
    pub const NOT_AUTHORIZED: &str = "seal/not_authorized";
    pub const STALE_EPOCH: &str = "seal/stale_epoch";
    pub const BAD_PAYLOAD: &str = "seal/bad_payload";
    pub const BAD_CTL_SIG: &str = "seal/bad_ctl_sig";
    pub const CTL_SHAPE: &str = "seal/ctl_shape";
    pub const CTL_GENESIS: &str = "seal/ctl_genesis";
    pub const CTL_AUTHOR: &str = "seal/ctl_author";
    pub const CTL_TARGET: &str = "seal/ctl_target";
    pub const CTL_DUPLICATE_KEY: &str = "seal/ctl_duplicate_key";
    pub const CTL_DUPLICATE: &str = "seal/ctl_duplicate";
    pub const CTL_DEVICE_CAP: &str = "seal/ctl_device_cap";
    pub const CTL_CAP: &str = "seal/ctl_cap";
    pub const CTL_EPOCH: &str = "seal/ctl_epoch";
    pub const CTL_WRAPS: &str = "seal/ctl_wraps";
    pub const CTL_LAST_DEVICE: &str = "seal/ctl_last_device";
    pub const CTL_HOME_GEN: &str = "seal/ctl_home_gen";
    pub const CTL_SUPPORT: &str = "seal/ctl_support";
    pub const COMMITMENT: &str = "seal/commitment";
    pub const UNKNOWN_RULES: &str = "seal/unknown_rules";
    pub const PAYLOAD_TOO_LARGE: &str = "seal/payload_too_large";
    pub const MEMBERSHIP_FORMAT: &str = "seal/membership_format";
    pub const HOST_FORMAT: &str = "seal/host_format";
    pub const MOVE_CODE: &str = "seal/move_code";
    pub const BLOB_SIZE: &str = "seal/blob_size";
    pub const BLOB_HASH: &str = "seal/blob_hash";
    pub const BLOB_HEADER: &str = "seal/blob_header";
    pub const BLOB_OPEN: &str = "seal/blob_open";
    pub const SNAPSHOT_LARGE: &str = "seal/snapshot_large";
    pub const INFLATE: &str = "seal/inflate";
}

// ---------- limits (`seal.md` §23)

const FRAME_MAGIC: &[u8; 4] = b"TDF1";
const MEMBERSHIP_MAGIC: &[u8; 4] = b"TDM1";
const BLOB_MAGIC: &[u8; 4] = b"TDB1";
const SNAPSHOT_MAGIC: &[u8; 4] = b"TDSN";
const IDENTITY_MAGIC: &[u8; 4] = b"FKID";
const FRAME_MIN: usize = 670;
const FRAME_MAX: usize = 262_144;
const HEADER_LEN: usize = 48;
const BODY_PREFIX: usize = 30;
const TAG_LEN: usize = 16;
const PLAIN_PREFIX: u64 = 69;
const PADDED_MAX: u64 = 258_048;
const AT_MS_MAX: i64 = 253_402_300_799_999;
const I31_MAX: u32 = 0x7FFF_FFFF;
const BLOB_MAX: u64 = 67_108_864;
const BLOB_HEADER_LEN: usize = 51;
const PART: u64 = 1_048_576;
const HISTORY_ITEMS_MAX: usize = 262_144;
const WINDOW_30_DAYS: i64 = 2_592_000_000;
const SKIP_WINDOW: u64 = 16_384;
const MAX_CURRENT: usize = 16;
const MAX_RECORDS: usize = 256;
const MAX_DEVICE_NO: u16 = 8_191;
const MAX_RECOVERY: usize = 64;
const MAX_COMMITMENTS: usize = 65_535;
const MAX_SKIPS: usize = 65_535;
const MAX_CTL: usize = 1_048_576;
const MAX_ITEMS: usize = 16;
const MAX_LIST: usize = 16;
const MAX_ACKS: usize = 64;

// ---------- primitives (engine.md §6.3 table)

/// SHA-256 over the concatenation of `parts`.
fn h(parts: &[&[u8]]) -> Hash32 {
    let mut d = Sha256::new();
    for p in parts {
        d.update(p);
    }
    d.finalize().into()
}

/// A context string of `seal.md` §4.1: `taskdeck/v1/<use>` and one `0x00`.
fn ctx(name: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(13 + name.len());
    v.extend_from_slice(b"taskdeck/v1/");
    v.extend_from_slice(name.as_bytes());
    v.push(0);
    v
}

fn first16(x: &Hash32) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(&x[..16]);
    out
}

/// Equality without an early exit (`seal.md` §15.4, §20.5 step 2).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `N` bytes from the generator: consecutive big-endian outputs, the last truncated (§6.3).
fn draw<const N: usize>(rng: &mut SplitMix64) -> [u8; N] {
    let mut out = [0u8; N];
    for chunk in out.chunks_mut(8) {
        let v = rng.next().to_be_bytes();
        chunk.copy_from_slice(&v[..chunk.len()]);
    }
    out
}

/// A fake key pair's public parts: `sign_pk = H("FAKE-PK" ‖ sk)`, `hpke_pk = H("FAKE-HPKE" ‖ sk)`.
fn public_keys(sk: &[u8; 32]) -> (Hash32, Hash32) {
    (h(&[b"FAKE-PK", sk]), h(&[b"FAKE-HPKE", sk]))
}

/// The fake signature over `m` by `sign_pk` (64 bytes).
fn sign(sign_pk: &Hash32, m: &[u8]) -> [u8; 64] {
    let a = h(&[b"FAKE-SIG", sign_pk, m]);
    let b = h(&[b"FAKE-SIG2", sign_pk, m]);
    let mut sig = [0u8; 64];
    sig[..32].copy_from_slice(&a);
    sig[32..].copy_from_slice(&b);
    sig
}

fn verify(sign_pk: &Hash32, m: &[u8], sig: &[u8]) -> bool {
    ct_eq(&sign(sign_pk, m), sig)
}

fn board_id_of(rsign_pk: &Hash32, salt: &[u8; 16]) -> [u8; 16] {
    first16(&h(&[b"FAKE-BOARD", rsign_pk, salt]))
}

fn dev_r_of(board_id: &[u8; 16], rsign_pk: &Hash32) -> Dev {
    first16(&h(&[b"FAKE-REC", board_id, rsign_pk]))
}

fn entry_key(board_id: &[u8; 16], e: u32, root: &[u8; 32]) -> Hash32 {
    h(&[b"FAKE-KEY", board_id, &e.to_be_bytes(), root])
}

fn snap_key(board_id: &[u8; 16], e: u32, root: &[u8; 32]) -> Hash32 {
    h(&[b"FAKE-SNAPKEY", board_id, &e.to_be_bytes(), root])
}

fn commitment(board_id: &[u8; 16], e: u32, root: &[u8; 32]) -> Hash32 {
    h(&[b"FAKE-EPOCH", board_id, &e.to_be_bytes(), root])
}

/// The fake wrap of `root` (epoch `e`) to recipient `id` with `hpke_pk` (96 bytes).
fn wrap(id: &Dev, hpke_pk: &Hash32, e: u32, root: &[u8; 32]) -> [u8; 96] {
    let mask = h(&[b"FAKE-WRAP", hpke_pk, &e.to_be_bytes()]);
    let tag = h(&[b"FAKE-WRAPTAG", root, &e.to_be_bytes()]);
    let mut w = [0u8; 96];
    w[..16].copy_from_slice(id);
    for ((out, r), k) in w[48..80].iter_mut().zip(root).zip(&mask) {
        *out = r ^ k;
    }
    w[80..].copy_from_slice(&tag[..16]);
    w
}

/// Unwraps with the receiver's own id and `hpke_pk`, checking the tag and
/// `c_e`; a wrap to another recipient id does not open (`seal.md` §7).
fn unwrap(w: &[u8; 96], own: &Dev, hpke_pk: &Hash32, e: u32, commit: &Hash32, board_id: &[u8; 16]) -> Option<[u8; 32]> {
    if w[..16] != own[..] {
        return None;
    }
    let mask = h(&[b"FAKE-WRAP", hpke_pk, &e.to_be_bytes()]);
    let mut root = [0u8; 32];
    for ((out, c), k) in root.iter_mut().zip(&w[48..80]).zip(&mask) {
        *out = c ^ k;
    }
    let tag = h(&[b"FAKE-WRAPTAG", &root, &e.to_be_bytes()]);
    if !ct_eq(&tag[..16], &w[80..]) || !ct_eq(&commitment(board_id, e, &root), commit) {
        return None;
    }
    Some(root)
}

/// The fake AEAD: `p ‖ H("FAKE-AEAD" ‖ k ‖ a ‖ p)[0..16]`.
fn aead_seal(k: &Hash32, ad: &[u8], p: &[u8]) -> Vec<u8> {
    let tag = h(&[b"FAKE-AEAD", k, ad, p]);
    let mut out = Vec::with_capacity(p.len() + TAG_LEN);
    out.extend_from_slice(p);
    out.extend_from_slice(&tag[..TAG_LEN]);
    out
}

fn aead_open<'a>(k: &Hash32, ad: &[u8], c: &'a [u8]) -> Option<&'a [u8]> {
    let split = c.len().checked_sub(TAG_LEN)?;
    let (p, t) = c.split_at(split);
    let tag = h(&[b"FAKE-AEAD", k, ad, p]);
    ct_eq(&tag[..TAG_LEN], t).then_some(p)
}

/// `C_0` (`seal.md` §15.1).
fn chain_zero_of(board_id: &[u8; 16]) -> Hash32 {
    h(&[&ctx("chain"), board_id])
}

/// `C_s` (`seal.md` §15.1).
fn chain_next(prev: &Hash32, s: u64, frame_hash: &Hash32) -> Hash32 {
    h(&[&ctx("chain"), prev, &s.to_be_bytes(), frame_hash])
}

/// `P(U)` (`seal.md` §10.2).
fn pad_len(u: u64) -> u64 {
    if u <= 8_192 {
        return u.div_ceil(512).saturating_mul(512);
    }
    let e = u64::from(63 - u.leading_zeros());
    let s = u64::from(63 - e.leading_zeros()) + 1;
    let m = (1u64 << (e - s)) - 1;
    u.saturating_add(m) & !m
}

/// Crockford base32 (`seal.md` §5.2).
fn crockford(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut out = String::new();
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &b in bytes {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[((acc >> bits) & 31) as usize]));
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(char::from(ALPHABET[((acc << (5 - bits)) & 31) as usize]));
    }
    out
}

/// The key fingerprint text (`seal.md` §6.8): four groups of four.
fn key_fp(sign_pk: &Hash32, hpke_pk: &Hash32) -> String {
    let x = h(&[&ctx("key-fp"), sign_pk, hpke_pk]);
    let s = crockford(&x[..10]);
    format!("{}-{}-{}-{}", &s[0..4], &s[4..8], &s[8..12], &s[12..16])
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    out
}

/// A host of `seal.md` §5.4.
fn host_ok(host: &str) -> bool {
    if host.is_empty() || host.len() > 255 {
        return false;
    }
    let (name, port) = match host.split_once(':') {
        Some((n, p)) => (n, Some(p)),
        None => (host, None),
    };
    if name.is_empty() || name.len() > 253 {
        return false;
    }
    let label_ok = |l: &str| {
        let b = l.as_bytes();
        !b.is_empty()
            && b.len() <= 63
            && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
            && b[0] != b'-'
            && b[b.len() - 1] != b'-'
    };
    if !name.split('.').all(label_ok) {
        return false;
    }
    match port {
        None => true,
        Some(p) => {
            let b = p.as_bytes();
            !b.is_empty()
                && b.len() <= 5
                && b.iter().all(u8::is_ascii_digit)
                && b[0] != b'0'
                && p.parse::<u32>().is_ok_and(|v| v <= 65_535 && v != 443)
        }
    }
}

/// A device name of `seal.md` §5.5.
fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.chars().any(|c| {
            let v = u32::from(c);
            v <= 0x1F || (0x7F..=0x9F).contains(&v)
        })
}

fn ascending<T: Ord>(v: &[T]) -> bool {
    v.windows(2).all(|w| w[0] < w[1])
}

/// What a rules version allows (`seal.md` §14.3; versions above 1 as version 1).
fn envelopes_for(_rules: u32) -> Vec<(u8, u8)> {
    vec![(1, 1)]
}

fn hash_versions_for(_rules: u32) -> Vec<u32> {
    vec![1]
}

/// The state hash of engine.md §18.3, version 1.
fn state_hash_of(board_hash: &Hash32, membership_bytes: &[u8]) -> Hash32 {
    state_hash_v(1, board_hash, membership_bytes)
}

/// The 86-byte composition of engine.md §18.3 with `hash_version` in its version field.
fn state_hash_v(hash_version: u32, board_hash: &Hash32, membership_bytes: &[u8]) -> Hash32 {
    h(&[&ctx("state"), &hash_version.to_be_bytes(), board_hash, &h(&[membership_bytes])])
}

// ---------- byte reader and writer

/// A strict big-endian reader.
struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { rest: bytes }
    }

    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, tail) = self.rest.split_first_chunk::<N>()?;
        self.rest = tail;
        Some(*head)
    }

    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.rest.len() < n {
            return None;
        }
        let (head, tail) = self.rest.split_at(n);
        self.rest = tail;
        Some(head)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|b| b[0])
    }

    fn u16(&mut self) -> Option<u16> {
        self.take().map(u16::from_be_bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_be_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.take().map(u64::from_be_bytes)
    }

    fn i64(&mut self) -> Option<i64> {
        self.take().map(i64::from_be_bytes)
    }

    fn vb8(&mut self) -> Option<&'a [u8]> {
        let n = self.u8()?;
        self.bytes(usize::from(n))
    }

    fn vb32(&mut self) -> Option<&'a [u8]> {
        let n = usize::try_from(self.u32()?).ok()?;
        self.bytes(n)
    }

    fn text8(&mut self) -> Option<String> {
        std::str::from_utf8(self.vb8()?).ok().map(str::to_owned)
    }

    fn is_empty(&self) -> bool {
        self.rest.is_empty()
    }
}

fn put_vb8(out: &mut Vec<u8>, b: &[u8]) -> Option<()> {
    out.push(u8::try_from(b.len()).ok()?);
    out.extend_from_slice(b);
    Some(())
}

fn put_vb32(out: &mut Vec<u8>, b: &[u8]) -> Option<()> {
    out.extend_from_slice(&u32::try_from(b.len()).ok()?.to_be_bytes());
    out.extend_from_slice(b);
    Some(())
}

fn put_count8(out: &mut Vec<u8>, n: usize) -> Option<()> {
    out.push(u8::try_from(n).ok()?);
    Some(())
}

// ---------- control items (engine.md §6.3 "Fake payload encodings")

/// A Genesis item's fields (engine.md §6.3, type 1).
#[derive(Clone)]
struct GenesisItem {
    salt: [u8; 16],
    rsign_pk: Hash32,
    rhpke_pk: Hash32,
    dev: Dev,
    sign_pk: Hash32,
    hpke_pk: Hash32,
    name: String,
    commit: Hash32,
    wraps: [[u8; 96]; 2],
    rules: u32,
    hash_version: u32,
    state_hash: Hash32,
}

/// One control item with every field of its fake encoding.
#[derive(Clone)]
enum Item {
    Genesis(Box<GenesisItem>),
    DeviceAdded { dev: Dev, sign_pk: Hash32, hpke_pk: Hash32, name: String, transcript: Hash32, bootstrap_seq: u64 },
    Revoked(Dev),
    Reinstated(Dev),
    /// `purge_before = 0` means absent.
    Rotated { epoch: u32, commit: Hash32, wraps: Vec<[u8; 96]>, purge_before: u64 },
    /// The new recovery key: `sign_pk`, `hpke_pk`.
    Recovery { sign_pk: Hash32, hpke_pk: Hash32 },
    Fence { host: String, door: [u8; 5], home_gen: u32 },
    ResetTo { seq: u64, state_hash: Hash32, hash_version: u32 },
    Skip { seq: u64, frame_hash: Hash32 },
    /// `mode`: 1 normal, 2 override.
    Upgrade { rules: u32, mode: u8 },
}

fn encode_item(item: &Item, out: &mut Vec<u8>) -> Option<()> {
    match item {
        Item::Genesis(g) => {
            out.push(1);
            out.extend_from_slice(&g.salt);
            out.extend_from_slice(&g.rsign_pk);
            out.extend_from_slice(&g.rhpke_pk);
            out.extend_from_slice(&g.dev);
            out.extend_from_slice(&g.sign_pk);
            out.extend_from_slice(&g.hpke_pk);
            put_vb8(out, g.name.as_bytes())?;
            out.extend_from_slice(&g.commit);
            out.extend_from_slice(&g.wraps[0]);
            out.extend_from_slice(&g.wraps[1]);
            out.extend_from_slice(&g.rules.to_be_bytes());
            out.extend_from_slice(&g.hash_version.to_be_bytes());
            out.extend_from_slice(&g.state_hash);
        }
        Item::DeviceAdded { dev, sign_pk, hpke_pk, name, transcript, bootstrap_seq } => {
            out.push(2);
            out.extend_from_slice(dev);
            out.extend_from_slice(sign_pk);
            out.extend_from_slice(hpke_pk);
            put_vb8(out, name.as_bytes())?;
            out.extend_from_slice(transcript);
            out.extend_from_slice(&bootstrap_seq.to_be_bytes());
        }
        Item::Revoked(dev) => {
            out.push(3);
            out.extend_from_slice(dev);
        }
        Item::Reinstated(dev) => {
            out.push(4);
            out.extend_from_slice(dev);
        }
        Item::Rotated { epoch, commit, wraps, purge_before } => {
            out.push(5);
            out.extend_from_slice(&epoch.to_be_bytes());
            out.extend_from_slice(commit);
            put_count8(out, wraps.len())?;
            for w in wraps {
                out.extend_from_slice(w);
            }
            out.extend_from_slice(&purge_before.to_be_bytes());
        }
        Item::Recovery { sign_pk, hpke_pk } => {
            out.push(6);
            out.extend_from_slice(sign_pk);
            out.extend_from_slice(hpke_pk);
        }
        Item::Fence { host, door, home_gen } => {
            out.push(7);
            put_vb8(out, host.as_bytes())?;
            out.extend_from_slice(door);
            out.extend_from_slice(&home_gen.to_be_bytes());
        }
        Item::ResetTo { seq, state_hash, hash_version } => {
            out.push(8);
            out.extend_from_slice(&seq.to_be_bytes());
            out.extend_from_slice(state_hash);
            out.extend_from_slice(&hash_version.to_be_bytes());
        }
        Item::Skip { seq, frame_hash } => {
            out.push(9);
            out.extend_from_slice(&seq.to_be_bytes());
            out.extend_from_slice(frame_hash);
        }
        Item::Upgrade { rules, mode } => {
            out.push(10);
            out.extend_from_slice(&rules.to_be_bytes());
            out.push(*mode);
        }
    }
    Some(())
}

/// `u8 n_items ‖ items`: the "items bytes" of the inner signature and of a history record.
fn encode_items(items: &[Item]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    put_count8(&mut out, items.len())?;
    for item in items {
        encode_item(item, &mut out)?;
    }
    Some(out)
}

/// Parses one item and checks the constraints of its `seal.md` §12 table that
/// need no position (those that do are §13.6's, step 16).
fn parse_item(r: &mut Reader<'_>) -> Option<Item> {
    let item = match r.u8()? {
        1 => {
            let salt = r.take()?;
            let rsign_pk = r.take()?;
            let rhpke_pk = r.take()?;
            let dev = r.take()?;
            let sign_pk = r.take()?;
            let hpke_pk = r.take()?;
            let name = r.text8()?;
            let commit = r.take()?;
            let wraps = [r.take()?, r.take()?];
            let rules = r.u32()?;
            let hash_version = r.u32()?;
            let state_hash = r.take()?;
            if !name_ok(&name) || rules != 1 || hash_version != 1 {
                return None;
            }
            Item::Genesis(Box::new(GenesisItem {
                salt,
                rsign_pk,
                rhpke_pk,
                dev,
                sign_pk,
                hpke_pk,
                name,
                commit,
                wraps,
                rules,
                hash_version,
                state_hash,
            }))
        }
        2 => {
            let dev = r.take()?;
            let sign_pk = r.take()?;
            let hpke_pk = r.take()?;
            let name = r.text8()?;
            let transcript = r.take()?;
            let bootstrap_seq = r.u64()?;
            if !name_ok(&name) || bootstrap_seq == 0 {
                return None;
            }
            Item::DeviceAdded { dev, sign_pk, hpke_pk, name, transcript, bootstrap_seq }
        }
        3 => Item::Revoked(r.take()?),
        4 => Item::Reinstated(r.take()?),
        5 => {
            let epoch = r.u32()?;
            let commit = r.take()?;
            let n = usize::from(r.u8()?);
            let mut wraps = Vec::with_capacity(n);
            for _ in 0..n {
                wraps.push(r.take::<96>()?);
            }
            let purge_before = r.u64()?;
            let ids: Vec<[u8; 16]> = wraps.iter().map(first16_of).collect();
            if epoch < 2 || !(2..=17).contains(&n) || !ascending(&ids) {
                return None;
            }
            Item::Rotated { epoch, commit, wraps, purge_before }
        }
        6 => Item::Recovery { sign_pk: r.take()?, hpke_pk: r.take()? },
        7 => {
            let host = r.text8()?;
            let door = r.take()?;
            let home_gen = r.u32()?;
            if !host_ok(&host) || home_gen < 2 {
                return None;
            }
            Item::Fence { host, door, home_gen }
        }
        8 => {
            let seq = r.u64()?;
            let state_hash = r.take()?;
            let hash_version = r.u32()?;
            if seq == 0 || !(1..=I31_MAX).contains(&hash_version) {
                return None;
            }
            Item::ResetTo { seq, state_hash, hash_version }
        }
        9 => {
            let seq = r.u64()?;
            let frame_hash = r.take()?;
            if seq == 0 {
                return None;
            }
            Item::Skip { seq, frame_hash }
        }
        10 => {
            let rules = r.u32()?;
            let mode = r.u8()?;
            if !(2..=I31_MAX).contains(&rules) || !(1..=2).contains(&mode) {
                return None;
            }
            Item::Upgrade { rules, mode }
        }
        _ => return None,
    };
    Some(item)
}

fn first16_of(w: &[u8; 96]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(&w[..16]);
    out
}

/// Parses `u8 n_items ‖ items` exactly.
fn parse_items(bytes: &[u8]) -> Option<Vec<Item>> {
    let mut r = Reader::new(bytes);
    let n = usize::from(r.u8()?);
    if !(1..=MAX_ITEMS).contains(&n) {
        return None;
    }
    let mut items = Vec::with_capacity(n);
    for _ in 0..n {
        items.push(parse_item(&mut r)?);
    }
    r.is_empty().then_some(items)
}

/// The shapes of `seal.md` §12.1.
fn shape_ok(items: &[Item], s: u64) -> bool {
    use Item as I;
    if s == 1 {
        return matches!(items, [I::Genesis(_)]);
    }
    match items {
        [I::Genesis(_)] => false,
        [I::DeviceAdded { .. }] | [I::Fence { .. }] | [I::ResetTo { .. }] | [I::Upgrade { .. }] => true,
        [I::Recovery { .. }, I::Rotated { .. }] => true,
        _ => {
            if items.iter().all(|i| matches!(i, I::Skip { .. })) {
                let mut seqs: Vec<u64> =
                    items.iter().filter_map(|i| if let I::Skip { seq, .. } = i { Some(*seq) } else { None }).collect();
                seqs.sort_unstable();
                return !seqs.is_empty() && seqs.len() <= MAX_ITEMS && ascending(&seqs);
            }
            let Some((I::Rotated { .. }, rest)) = items.split_last() else {
                return false;
            };
            let mut targets = Vec::with_capacity(rest.len());
            for i in rest {
                match i {
                    I::Revoked(d) | I::Reinstated(d) => targets.push(*d),
                    _ => return false,
                }
            }
            targets.sort_unstable();
            targets.len() <= 15 && ascending(&targets)
        }
    }
}

/// The inner control signature's message (engine.md §6.3).
fn ctl_inner(board_id: &[u8; 16], dev: &Dev, ctr: u64, entry_id: &[u8; 16], at_ms: i64, items_bytes: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(8 + 16 + 16 + 8 + 16 + 8 + 32);
    m.extend_from_slice(b"FAKE-CTL");
    m.extend_from_slice(board_id);
    m.extend_from_slice(dev);
    m.extend_from_slice(&ctr.to_be_bytes());
    m.extend_from_slice(entry_id);
    m.extend_from_slice(&at_ms.to_be_bytes());
    m.extend_from_slice(&h(&[items_bytes]));
    m
}

/// A parsed fake `ctl` payload: the items, their bytes and the inner signature.
struct CtlPayload<'a> {
    items: Vec<Item>,
    items_bytes: &'a [u8],
    sig: &'a [u8],
}

fn parse_ctl(payload: &[u8]) -> Option<CtlPayload<'_>> {
    let split = payload.len().checked_sub(64)?;
    let (items_bytes, sig) = payload.split_at(split);
    Some(CtlPayload { items: parse_items(items_bytes)?, items_bytes, sig })
}

// ---------- beat and snap payloads

fn verdict_byte(v: &Verdict) -> u8 {
    match v {
        Verdict::Ack => 1,
        Verdict::Mismatch => 2,
    }
}

fn encode_beat(b: &BeatInfo) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    put_count8(&mut out, b.rules.len())?;
    for r in &b.rules {
        out.extend_from_slice(&r.to_be_bytes());
    }
    put_count8(&mut out, b.envelopes.len())?;
    for (ver, suite) in &b.envelopes {
        out.push(*ver);
        out.push(*suite);
    }
    put_count8(&mut out, b.hash_versions.len())?;
    for v in &b.hash_versions {
        out.extend_from_slice(&v.to_be_bytes());
    }
    match &b.state {
        None => out.push(0),
        Some((seq, hash, hv)) => {
            out.push(1);
            out.extend_from_slice(&seq.to_be_bytes());
            out.extend_from_slice(hash);
            out.extend_from_slice(&hv.to_be_bytes());
        }
    }
    put_count8(&mut out, b.acks.len())?;
    for (entry, v) in &b.acks {
        out.extend_from_slice(&entry.to_be_bytes());
        out.push(verdict_byte(v));
    }
    Some(out)
}

fn version_list(r: &mut Reader<'_>) -> Option<Vec<u32>> {
    let n = usize::from(r.u8()?);
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(r.u32()?);
    }
    let ok = (1..=MAX_LIST).contains(&n) && ascending(&v) && v.iter().all(|x| (1..=I31_MAX).contains(x));
    ok.then_some(v)
}

/// The beat payload, checked against `seal.md` §11.4 (`b` = `based_on_seq`, `s` the beat's seq).
fn parse_beat(payload: &[u8], b: u64, s: u64) -> Option<BeatInfo> {
    let mut r = Reader::new(payload);
    let rules = version_list(&mut r)?;
    let n = usize::from(r.u8()?);
    let mut envelopes = Vec::with_capacity(n);
    for _ in 0..n {
        envelopes.push((r.u8()?, r.u8()?));
    }
    if !(1..=MAX_LIST).contains(&n) || !ascending(&envelopes) || envelopes.iter().any(|(v, s)| *v == 0 || *s == 0) {
        return None;
    }
    let hash_versions = version_list(&mut r)?;
    let state = match r.u8()? {
        0 => None,
        1 => {
            let seq = r.u64()?;
            let hash = r.take()?;
            let hv = r.u32()?;
            if seq < 64 || !seq.is_multiple_of(64) || seq > b || !(1..=I31_MAX).contains(&hv) {
                return None;
            }
            Some((seq, hash, hv))
        }
        _ => return None,
    };
    let n = usize::from(r.u8()?);
    if n > MAX_ACKS {
        return None;
    }
    let mut acks = Vec::with_capacity(n);
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        let entry = r.u64()?;
        let verdict = match r.u8()? {
            1 => Verdict::Ack,
            2 => Verdict::Mismatch,
            _ => return None,
        };
        if entry == 0 || entry >= s {
            return None;
        }
        entries.push(entry);
        acks.push((entry, verdict));
    }
    if !ascending(&entries) || !r.is_empty() {
        return None;
    }
    Some(BeatInfo { rules, envelopes, hash_versions, state, acks })
}

fn encode_snap(i: &SnapInfo) -> Vec<u8> {
    let mut out = Vec::with_capacity(116);
    out.extend_from_slice(&i.seq.to_be_bytes());
    out.extend_from_slice(&i.hash);
    out.extend_from_slice(&i.hash_version.to_be_bytes());
    out.extend_from_slice(&i.blob);
    out.extend_from_slice(&i.sha256);
    out.extend_from_slice(&i.size.to_be_bytes());
    out.extend_from_slice(&i.parts.to_be_bytes());
    out.extend_from_slice(&i.inflated.to_be_bytes());
    out.extend_from_slice(&i.epoch.to_be_bytes());
    out
}

/// The snap payload, checked against `seal.md` §11.5.
fn parse_snap(payload: &[u8], b: u64, frame_epoch: u32) -> Option<SnapInfo> {
    let mut r = Reader::new(payload);
    let i = SnapInfo {
        seq: r.u64()?,
        hash: r.take()?,
        hash_version: r.u32()?,
        blob: r.take()?,
        sha256: r.take()?,
        size: r.u64()?,
        parts: r.u32()?,
        inflated: r.u64()?,
        epoch: r.u32()?,
    };
    let ok = r.is_empty()
        && i.seq >= 64
        && i.seq.is_multiple_of(64)
        && i.seq <= b
        && (1..=I31_MAX).contains(&i.hash_version)
        && (67..=BLOB_MAX).contains(&i.size)
        && u64::from(i.parts) == i.size.div_ceil(PART)
        && (1..=BLOB_MAX).contains(&i.inflated)
        && i.epoch == frame_epoch;
    ok.then_some(i)
}

// ---------- frame, body and plaintext (`seal.md` §8 to §10)

/// A frame after §14.2 step 1.
struct FrameView<'a> {
    board_id: [u8; 16],
    dev: Dev,
    ctr: u64,
    header: &'a [u8],
    body: &'a [u8],
    signed_prefix: &'a [u8],
    sig: &'a [u8],
}

fn parse_frame(frame: &[u8]) -> Option<FrameView<'_>> {
    if frame.len() < FRAME_MIN || frame.len() > FRAME_MAX || &frame[..4] != FRAME_MAGIC {
        return None;
    }
    let mut r = Reader::new(&frame[4..HEADER_LEN]);
    let board_id = r.take()?;
    let dev = r.take()?;
    let ctr = r.u64()?;
    let len = usize::try_from(r.u32()?).ok()?;
    if len != frame.len() - 112 {
        return None;
    }
    let end = HEADER_LEN + len;
    Some(FrameView {
        board_id,
        dev,
        ctr,
        header: &frame[..HEADER_LEN],
        body: &frame[HEADER_LEN..end],
        signed_prefix: &frame[..end],
        sig: &frame[end..],
    })
}

/// `ctx("frame") ‖ frame[0 .. 48 + len]` (`seal.md` §8.3).
fn frame_signed(prefix: &[u8]) -> Vec<u8> {
    let mut m = ctx("frame");
    m.extend_from_slice(prefix);
    m
}

/// `ctx("entry") ‖ header ‖ ver ‖ suite ‖ epoch` (`seal.md` §9.2).
fn entry_ad(header: &[u8], ver: u8, suite: u8, epoch: u32) -> Vec<u8> {
    let mut ad = ctx("entry");
    ad.extend_from_slice(header);
    ad.push(ver);
    ad.push(suite);
    ad.extend_from_slice(&epoch.to_be_bytes());
    ad
}

/// The author identity and envelope a frame is built under.
struct Envelope<'a> {
    board_id: &'a [u8; 16],
    dev: &'a Dev,
    sign_pk: &'a Hash32,
    ctr: u64,
    ver: u8,
    suite: u8,
    epoch: u32,
    key: Hash32,
}

/// Lays out and signs a frame (`seal.md` §8.1, §9.1, §8.3).
fn build_frame(env: &Envelope<'_>, nonce: &[u8; 24], padded: &[u8]) -> Vec<u8> {
    let len = BODY_PREFIX + padded.len() + TAG_LEN;
    let mut f = Vec::with_capacity(112 + len);
    f.extend_from_slice(FRAME_MAGIC);
    f.extend_from_slice(env.board_id);
    f.extend_from_slice(env.dev);
    f.extend_from_slice(&env.ctr.to_be_bytes());
    // `len` is at most 258,094 here (padded is at most 258,048).
    f.extend_from_slice(&u32::try_from(len).unwrap_or(u32::MAX).to_be_bytes());
    let ad = entry_ad(&f, env.ver, env.suite, env.epoch);
    f.push(env.ver);
    f.push(env.suite);
    f.extend_from_slice(&env.epoch.to_be_bytes());
    f.extend_from_slice(nonce);
    f.extend_from_slice(&aead_seal(&env.key, &ad, padded));
    let sig = sign(env.sign_pk, &frame_signed(&f));
    f.extend_from_slice(&sig);
    f
}

/// The padded plaintext of `seal.md` §10.1, or `None` above the cap (SEAL-PAD-002).
fn build_plain(entry_id: &[u8; 16], kind: u8, at_ms: i64, based_on: &Head, payload: &[u8]) -> Option<Vec<u8>> {
    let u = PLAIN_PREFIX + u64::try_from(payload.len()).ok()?;
    let p = pad_len(u);
    if p > PADDED_MAX {
        return None;
    }
    let mut out = Vec::with_capacity(usize::try_from(p).ok()?);
    out.extend_from_slice(entry_id);
    out.push(kind);
    out.extend_from_slice(&at_ms.to_be_bytes());
    out.extend_from_slice(&based_on.seq.to_be_bytes());
    out.extend_from_slice(&based_on.chain);
    out.extend_from_slice(&u32::try_from(payload.len()).ok()?.to_be_bytes());
    out.extend_from_slice(payload);
    out.resize(usize::try_from(p).ok()?, 0);
    Some(out)
}

/// An opened plaintext after the unpadding checks of `seal.md` §10.3.
struct Plain<'a> {
    entry_id: [u8; 16],
    kind: u8,
    at_ms: i64,
    based_on_seq: u64,
    based_on_chain: Hash32,
    payload: &'a [u8],
}

fn parse_plain(p: &[u8]) -> Option<Plain<'_>> {
    let l = u64::try_from(p.len()).ok()?;
    if l < PLAIN_PREFIX {
        return None;
    }
    let mut r = Reader::new(p);
    let entry_id = r.take()?;
    let kind = r.u8()?;
    let at_ms = r.i64()?;
    let based_on_seq = r.u64()?;
    let based_on_chain = r.take()?;
    let payload_len = r.u32()?;
    let u = PLAIN_PREFIX + u64::from(payload_len);
    if u > l || pad_len(u) != l {
        return None;
    }
    let u = usize::try_from(u).ok()?;
    if p[u..].iter().any(|b| *b != 0) || !(1..=4).contains(&kind) || !(0..=AT_MS_MAX).contains(&at_ms) {
        return None;
    }
    Some(Plain { entry_id, kind, at_ms, based_on_seq, based_on_chain, payload: &p[PLAIN_PREFIX as usize..u] })
}

fn kind_of(k: u8) -> Kind {
    match k {
        1 => Kind::Cmd,
        2 => Kind::Ctl,
        3 => Kind::Beat,
        _ => Kind::Snap,
    }
}

// ---------- membership (`seal.md` §13)

/// A device record (`seal.md` §13.1).
#[derive(Clone, Debug, PartialEq, Eq)]
struct DevRec {
    dev: Dev,
    sign_pk: Hash32,
    hpke_pk: Hash32,
    device_no: u16,
    current: bool,
    added_seq: u64,
    added_by: Dev,
    bootstrap_seq: u64,
    status_seq: u64,
    status_by: Dev,
    last_ctr: u64,
    beat_ms: i64,
    rules: Vec<u32>,
    envelopes: Vec<(u8, u8)>,
    hash_versions: Vec<u32>,
    name: String,
}

/// A recovery record (`seal.md` §13.1).
#[derive(Clone, Debug, PartialEq, Eq)]
struct RecRec {
    dev_r: Dev,
    sign_pk: Hash32,
    hpke_pk: Hash32,
    since_seq: u64,
    until_seq: u64,
    last_ctr: u64,
}

/// The latest accepted ResetTo (`seal.md` §13.1).
#[derive(Clone, Debug, PartialEq, Eq)]
struct LastReset {
    at_seq: u64,
    target: u64,
    hash_version: u32,
    state_hash: Hash32,
}

/// The fake's membership state (engine.md §6.3, `seal.md` §13.1). The default
/// value is `M_0`, the empty state before seq 1 (`seal.md` §13.3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FakeMembership {
    board_id: [u8; 16],
    salt: [u8; 16],
    rules: u32,
    epoch: u32,
    epoch_seq: u64,
    epoch_clock_ms: i64,
    clock_ms: i64,
    home_gen: u32,
    home_seq: u64,
    home_host: String,
    home_door: [u8; 5],
    purge_seq: u64,
    next_device_no: u16,
    /// `(e, seq, c_e)`, ascending by `e`.
    commitments: Vec<(u32, u64, Hash32)>,
    /// Ascending by `since_seq`.
    recovery: Vec<RecRec>,
    /// Ascending by `dev`.
    devices: Vec<DevRec>,
    /// `(rules, voter, seq)`.
    pending_override: Option<(u32, Dev, u64)>,
    last_reset: Option<LastReset>,
    /// `(t, frame_hash)`, ascending by `t`.
    skips: Vec<(u64, Hash32)>,
    ctl_seqs: Vec<u64>,
}

impl FakeMembership {
    fn device(&self, dev: &Dev) -> Option<&DevRec> {
        self.devices.binary_search_by(|d| d.dev.cmp(dev)).ok().map(|i| &self.devices[i])
    }

    fn device_mut(&mut self, dev: &Dev) -> Option<&mut DevRec> {
        match self.devices.binary_search_by(|d| d.dev.cmp(dev)) {
            Ok(i) => Some(&mut self.devices[i]),
            Err(_) => None,
        }
    }

    fn is_current_device(&self, dev: &Dev) -> bool {
        self.device(dev).is_some_and(|d| d.current)
    }

    fn current_recovery(&self) -> Option<&RecRec> {
        self.recovery.iter().find(|r| r.until_seq == 0)
    }

    fn current_count(&self) -> usize {
        self.devices.iter().filter(|d| d.current).count()
    }

    fn commitment_of(&self, e: u32) -> Option<&Hash32> {
        self.commitments.iter().find(|(x, _, _)| *x == e).map(|(_, _, c)| c)
    }

    fn set_last_ctr(&mut self, dev: &Dev, ctr: u64) {
        if let Some(d) = self.device_mut(dev) {
            d.last_ctr = ctr;
        } else if let Some(r) = self.recovery.iter_mut().find(|r| r.dev_r == *dev) {
            r.last_ctr = ctr;
        }
    }

    /// Whether a key or id is already recorded (`seal/ctl_duplicate_key`).
    fn recorded(&self, dev: &Dev, sign_pk: &Hash32, hpke_pk: &Hash32) -> bool {
        self.devices.iter().any(|d| d.dev == *dev || d.sign_pk == *sign_pk || d.hpke_pk == *hpke_pk)
            || self.recovery.iter().any(|r| r.dev_r == *dev || r.sign_pk == *sign_pk || r.hpke_pk == *hpke_pk)
    }

    /// The canonical membership bytes (`seal.md` §13.2).
    fn to_bytes(&self) -> Vec<u8> {
        let mut o = Vec::new();
        o.extend_from_slice(MEMBERSHIP_MAGIC);
        o.extend_from_slice(&1u32.to_be_bytes());
        o.extend_from_slice(&self.board_id);
        o.extend_from_slice(&self.salt);
        o.extend_from_slice(&self.rules.to_be_bytes());
        o.extend_from_slice(&self.epoch.to_be_bytes());
        o.extend_from_slice(&self.epoch_seq.to_be_bytes());
        o.extend_from_slice(&self.epoch_clock_ms.to_be_bytes());
        o.extend_from_slice(&self.clock_ms.to_be_bytes());
        o.extend_from_slice(&self.home_gen.to_be_bytes());
        o.extend_from_slice(&self.home_seq.to_be_bytes());
        // Every list here is bounded by §23 far below its count's width; a
        // host is at most 255 bytes and a name 64 (checked where they enter).
        let _ = put_vb8(&mut o, self.home_host.as_bytes());
        o.extend_from_slice(&self.home_door);
        o.extend_from_slice(&self.purge_seq.to_be_bytes());
        o.extend_from_slice(&self.next_device_no.to_be_bytes());
        o.extend_from_slice(&(self.commitments.len() as u32).to_be_bytes());
        for (e, seq, c) in &self.commitments {
            o.extend_from_slice(&e.to_be_bytes());
            o.extend_from_slice(&seq.to_be_bytes());
            o.extend_from_slice(c);
        }
        o.extend_from_slice(&(self.recovery.len() as u16).to_be_bytes());
        for r in &self.recovery {
            o.extend_from_slice(&r.dev_r);
            o.extend_from_slice(&r.sign_pk);
            o.extend_from_slice(&r.hpke_pk);
            o.extend_from_slice(&r.since_seq.to_be_bytes());
            o.extend_from_slice(&r.until_seq.to_be_bytes());
            o.extend_from_slice(&r.last_ctr.to_be_bytes());
        }
        o.extend_from_slice(&(self.devices.len() as u16).to_be_bytes());
        for d in &self.devices {
            o.extend_from_slice(&d.dev);
            o.extend_from_slice(&d.sign_pk);
            o.extend_from_slice(&d.hpke_pk);
            o.extend_from_slice(&d.device_no.to_be_bytes());
            o.push(if d.current { 1 } else { 2 });
            o.extend_from_slice(&d.added_seq.to_be_bytes());
            o.extend_from_slice(&d.added_by);
            o.extend_from_slice(&d.bootstrap_seq.to_be_bytes());
            o.extend_from_slice(&d.status_seq.to_be_bytes());
            o.extend_from_slice(&d.status_by);
            o.extend_from_slice(&d.last_ctr.to_be_bytes());
            o.extend_from_slice(&d.beat_ms.to_be_bytes());
            o.push(d.rules.len() as u8);
            for r in &d.rules {
                o.extend_from_slice(&r.to_be_bytes());
            }
            o.push(d.envelopes.len() as u8);
            for (v, s) in &d.envelopes {
                o.push(*v);
                o.push(*s);
            }
            o.push(d.hash_versions.len() as u8);
            for v in &d.hash_versions {
                o.extend_from_slice(&v.to_be_bytes());
            }
            let _ = put_vb8(&mut o, d.name.as_bytes());
        }
        match &self.pending_override {
            None => o.push(0),
            Some((rules, voter, seq)) => {
                o.push(1);
                o.extend_from_slice(&rules.to_be_bytes());
                o.extend_from_slice(voter);
                o.extend_from_slice(&seq.to_be_bytes());
            }
        }
        match &self.last_reset {
            None => o.push(0),
            Some(l) => {
                o.push(1);
                o.extend_from_slice(&l.at_seq.to_be_bytes());
                o.extend_from_slice(&l.target.to_be_bytes());
                o.extend_from_slice(&l.hash_version.to_be_bytes());
                o.extend_from_slice(&l.state_hash);
            }
        }
        o.extend_from_slice(&(self.skips.len() as u32).to_be_bytes());
        for (t, fh) in &self.skips {
            o.extend_from_slice(&t.to_be_bytes());
            o.extend_from_slice(fh);
        }
        o.extend_from_slice(&(self.ctl_seqs.len() as u32).to_be_bytes());
        for s in &self.ctl_seqs {
            o.extend_from_slice(&s.to_be_bytes());
        }
        o
    }

    /// The strict parser of the canonical bytes (SEAL-MEM-002).
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader::new(bytes);
        if &r.take::<4>()? != MEMBERSHIP_MAGIC || r.u32()? != 1 {
            return None;
        }
        let mut m = FakeMembership {
            board_id: r.take()?,
            salt: r.take()?,
            rules: r.u32()?,
            epoch: r.u32()?,
            epoch_seq: r.u64()?,
            epoch_clock_ms: r.i64()?,
            clock_ms: r.i64()?,
            home_gen: r.u32()?,
            home_seq: r.u64()?,
            home_host: r.text8()?,
            home_door: r.take()?,
            purge_seq: r.u64()?,
            next_device_no: r.u16()?,
            ..FakeMembership::default()
        };
        if !m.home_host.is_empty() && !host_ok(&m.home_host) {
            return None;
        }
        let n = usize::try_from(r.u32()?).ok()?;
        if n > MAX_COMMITMENTS {
            return None;
        }
        for _ in 0..n {
            m.commitments.push((r.u32()?, r.u64()?, r.take()?));
        }
        let n = usize::from(r.u16()?);
        if n > MAX_RECOVERY {
            return None;
        }
        for _ in 0..n {
            m.recovery.push(RecRec {
                dev_r: r.take()?,
                sign_pk: r.take()?,
                hpke_pk: r.take()?,
                since_seq: r.u64()?,
                until_seq: r.u64()?,
                last_ctr: r.u64()?,
            });
        }
        let n = usize::from(r.u16()?);
        if n > MAX_RECORDS {
            return None;
        }
        for _ in 0..n {
            let dev = r.take()?;
            let sign_pk = r.take()?;
            let hpke_pk = r.take()?;
            let device_no = r.u16()?;
            let current = match r.u8()? {
                1 => true,
                2 => false,
                _ => return None,
            };
            let mut d = DevRec {
                dev,
                sign_pk,
                hpke_pk,
                device_no,
                current,
                added_seq: r.u64()?,
                added_by: r.take()?,
                bootstrap_seq: r.u64()?,
                status_seq: r.u64()?,
                status_by: r.take()?,
                last_ctr: r.u64()?,
                beat_ms: r.i64()?,
                rules: Vec::new(),
                envelopes: Vec::new(),
                hash_versions: Vec::new(),
                name: String::new(),
            };
            for _ in 0..r.u8()? {
                d.rules.push(r.u32()?);
            }
            for _ in 0..r.u8()? {
                d.envelopes.push((r.u8()?, r.u8()?));
            }
            for _ in 0..r.u8()? {
                d.hash_versions.push(r.u32()?);
            }
            d.name = r.text8()?;
            let lists_ok = d.rules.len() <= MAX_LIST
                && d.envelopes.len() <= MAX_LIST
                && d.hash_versions.len() <= MAX_LIST
                && ascending(&d.rules)
                && ascending(&d.envelopes)
                && ascending(&d.hash_versions);
            if !(1..=MAX_DEVICE_NO).contains(&device_no) || !lists_ok || !name_ok(&d.name) {
                return None;
            }
            m.devices.push(d);
        }
        m.pending_override = match r.u8()? {
            0 => None,
            1 => Some((r.u32()?, r.take()?, r.u64()?)),
            _ => return None,
        };
        m.last_reset = match r.u8()? {
            0 => None,
            1 => Some(LastReset {
                at_seq: r.u64()?,
                target: r.u64()?,
                hash_version: r.u32()?,
                state_hash: r.take()?,
            }),
            _ => return None,
        };
        let n = usize::try_from(r.u32()?).ok()?;
        if n > MAX_SKIPS {
            return None;
        }
        for _ in 0..n {
            m.skips.push((r.u64()?, r.take()?));
        }
        let n = usize::try_from(r.u32()?).ok()?;
        if n > MAX_CTL || n > r.rest.len() / 8 {
            return None;
        }
        for _ in 0..n {
            m.ctl_seqs.push(r.u64()?);
        }
        let skip_seqs: Vec<u64> = m.skips.iter().map(|s| s.0).collect();
        let ok = r.is_empty() && ascending(&skip_seqs) && ascending(&m.ctl_seqs) && m.reachable();
        ok.then_some(m)
    }

    /// The state checks of `seal.md` §13.2 that a parser makes beyond the
    /// layout: bytes whose state no log can produce are refused, `M_0` of
    /// §13.3 apart (the only state whose `epoch` is 0, every field zero).
    fn reachable(&self) -> bool {
        if self.epoch == 0 {
            return *self == FakeMembership::default();
        }
        let commit_epochs: Vec<u32> = self.commitments.iter().map(|c| c.0).collect();
        let epochs_ok = commit_epochs.len() as u64 == u64::from(self.epoch)
            && commit_epochs.iter().zip(1..=self.epoch).all(|(e, want)| *e == want);
        let since: Vec<u64> = self.recovery.iter().map(|x| x.since_seq).collect();
        let one_current = self.recovery.iter().filter(|x| x.until_seq == 0).count() == 1;
        let chained = self.recovery.windows(2).all(|w| w[0].until_seq == w[1].since_seq);
        let devs_r = self.recovery.iter().all(|x| x.dev_r == dev_r_of(&self.board_id, &x.sign_pk));
        let board_ok = self.recovery.first().is_some_and(|x| board_id_of(&x.sign_pk, &self.salt) == self.board_id);
        let mut ids: Vec<Dev> = self.devices.iter().map(|d| d.dev).chain(self.recovery.iter().map(|x| x.dev_r)).collect();
        let devs: Vec<Dev> = self.devices.iter().map(|d| d.dev).collect();
        ids.sort_unstable();
        let mut nos: Vec<u16> = self.devices.iter().map(|d| d.device_no).collect();
        nos.sort_unstable();
        let nos_ok = ascending(&nos) && nos.last().is_none_or(|n| *n < self.next_device_no);
        epochs_ok
            && ascending(&since)
            && one_current
            && chained
            && devs_r
            && board_ok
            && ascending(&devs)
            && ascending(&ids)
            && nos_ok
    }

    /// The read-only view (engine.md §6.2).
    fn to_view(&self) -> MembershipView {
        let current = self.current_recovery();
        MembershipView {
            board_id: self.board_id,
            rules: self.rules,
            epoch: self.epoch,
            epoch_seq: self.epoch_seq,
            epoch_clock_ms: self.epoch_clock_ms,
            clock_ms: self.clock_ms,
            purge_seq: self.purge_seq,
            home_gen: self.home_gen,
            home_seq: self.home_seq,
            home_host: self.home_host.clone(),
            home_door: self.home_door,
            devices: self
                .devices
                .iter()
                .map(|d| DeviceView {
                    dev: d.dev,
                    sign_pk: d.sign_pk,
                    device_no: d.device_no,
                    status: if d.current { DeviceStatus::Current } else { DeviceStatus::Revoked },
                    name: d.name.clone(),
                    key_fp: key_fp(&d.sign_pk, &d.hpke_pk),
                    added_seq: d.added_seq,
                    added_by: d.added_by,
                    bootstrap_seq: d.bootstrap_seq,
                    status_seq: d.status_seq,
                    status_by: d.status_by,
                    last_ctr: d.last_ctr,
                    beat_ms: d.beat_ms,
                    rules: d.rules.clone(),
                    envelopes: d.envelopes.clone(),
                    hash_versions: d.hash_versions.clone(),
                })
                .collect(),
            recovery_dev: current.map(|r| r.dev_r).unwrap_or_default(),
            recovery_fp: current.map(|r| key_fp(&r.sign_pk, &r.hpke_pk)).unwrap_or_default(),
            recovery: self
                .recovery
                .iter()
                .map(|r| RecoveryView {
                    dev_r: r.dev_r,
                    sign_pk: r.sign_pk,
                    hpke_pk: r.hpke_pk,
                    since_seq: r.since_seq,
                    until_seq: r.until_seq,
                    last_ctr: r.last_ctr,
                })
                .collect(),
            pending_override: self.pending_override,
            last_reset: self.last_reset.as_ref().map(|l| (l.at_seq, l.target)),
            skips: self.skips.clone(),
            ctl_seqs: self.ctl_seqs.clone(),
        }
    }
}

/// `M_1` as `seal.md` §13.3 lists it.
fn genesis_state(g: &GenesisItem, board_id: [u8; 16], dev_r: Dev, at_ms: i64) -> FakeMembership {
    FakeMembership {
        board_id,
        salt: g.salt,
        rules: 1,
        epoch: 1,
        epoch_seq: 1,
        epoch_clock_ms: at_ms,
        clock_ms: at_ms,
        home_gen: 1,
        home_seq: 0,
        home_host: String::new(),
        home_door: [0; 5],
        purge_seq: 0,
        next_device_no: 2,
        commitments: vec![(1, 1, g.commit)],
        recovery: vec![RecRec {
            dev_r,
            sign_pk: g.rsign_pk,
            hpke_pk: g.rhpke_pk,
            since_seq: 1,
            until_seq: 0,
            last_ctr: 1,
        }],
        devices: vec![DevRec {
            dev: g.dev,
            sign_pk: g.sign_pk,
            hpke_pk: g.hpke_pk,
            device_no: 1,
            current: true,
            added_seq: 1,
            added_by: dev_r,
            bootstrap_seq: 0,
            status_seq: 1,
            status_by: dev_r,
            last_ctr: 0,
            beat_ms: at_ms,
            rules: vec![1],
            envelopes: vec![(1, 1)],
            hash_versions: vec![1],
            name: g.name.clone(),
        }],
        pending_override: None,
        last_reset: None,
        skips: Vec::new(),
        ctl_seqs: vec![1],
    }
}

// ---------- history records (`seal.md` §21.5) and the projection (§21.6)

/// One history record (`seal.md` §21.5).
struct HistRec<'a> {
    seq: u64,
    dev: Dev,
    ctr: u64,
    entry_id: [u8; 16],
    at_ms: i64,
    frame_hash: Hash32,
    items: &'a [u8],
    sig: &'a [u8],
}

impl HistRec<'_> {
    fn to_bytes(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(156 + self.items.len());
        o.extend_from_slice(&self.seq.to_be_bytes());
        o.extend_from_slice(&self.dev);
        o.extend_from_slice(&self.ctr.to_be_bytes());
        o.extend_from_slice(&self.entry_id);
        o.extend_from_slice(&self.at_ms.to_be_bytes());
        o.extend_from_slice(&self.frame_hash);
        let _ = put_vb32(&mut o, self.items);
        o.extend_from_slice(self.sig);
        o
    }
}

fn read_hist<'a>(r: &mut Reader<'a>) -> Option<HistRec<'a>> {
    let rec = HistRec {
        seq: r.u64()?,
        dev: r.take()?,
        ctr: r.u64()?,
        entry_id: r.take()?,
        at_ms: r.i64()?,
        frame_hash: r.take()?,
        items: r.vb32()?,
        sig: r.bytes(64)?,
    };
    (rec.items.len() <= HISTORY_ITEMS_MAX).then_some(rec)
}

/// The projection of `seal.md` §21.6, built from history records alone: a
/// membership state to which `DeviceAdded`, `DeviceRevoked`,
/// `DeviceReinstated`, `EpochRotated` and `RecoveryKeyChange` apply their
/// rows of §13.6 whole, and `Fence`, `ResetTo`, `Skip` and `RulesUpgrade` only
/// their Author column (projection step 6).
#[derive(Default)]
struct Projection {
    w: FakeMembership,
    /// The seq of the last record (projection step 1).
    last_seq: u64,
}

impl Projection {
    /// One record, by the projection steps 1 to 6 of `seal.md` §21.6.
    fn step(&mut self, board_id: &[u8; 16], rec: &HistRec<'_>) -> Option<()> {
        // Step 1.
        if (self.last_seq == 0 && rec.seq != 1) || (self.last_seq != 0 && rec.seq <= self.last_seq) {
            return None;
        }
        let items = parse_items(rec.items)?;
        // Step 2: the author is an authority so far (the first record: `dev_R` of its Genesis key).
        let (who, sign_pk, last_ctr) = if rec.seq == 1 {
            let Some(Item::Genesis(g)) = items.first() else { return None };
            if rec.dev != dev_r_of(board_id, &g.rsign_pk) {
                return None;
            }
            (Who::Recovery, g.rsign_pk, 0)
        } else if let Some(d) = self.w.device(&rec.dev).filter(|d| d.current) {
            (Who::Device, d.sign_pk, d.last_ctr)
        } else {
            let r = self.w.current_recovery().filter(|r| r.dev_r == rec.dev && r.since_seq < rec.seq)?;
            (Who::Recovery, r.sign_pk, r.last_ctr)
        };
        // Step 3: above the author's previous counter; the first record's is 1.
        if rec.ctr <= last_ctr || (rec.seq == 1 && rec.ctr != 1) {
            return None;
        }
        // Step 4.
        if !verify(&sign_pk, &ctl_inner(board_id, &rec.dev, rec.ctr, &rec.entry_id, rec.at_ms, rec.items), rec.sig) {
            return None;
        }
        // Step 5.
        if !shape_ok(&items, rec.seq) {
            return None;
        }
        // Step 6.
        if let [Item::Genesis(g)] = items.as_slice() {
            let mut ids = [g.dev, rec.dev];
            ids.sort_unstable();
            let ok = board_id_of(&g.rsign_pk, &g.salt) == *board_id
                && g.dev != rec.dev
                && first16_of(&g.wraps[0]) == ids[0]
                && first16_of(&g.wraps[1]) == ids[1];
            if !ok {
                return None;
            }
            self.w = genesis_state(g, *board_id, rec.dev, rec.at_ms);
        } else {
            let e = EntryCtx { s: rec.seq, author: rec.dev, who, at_ms: rec.at_ms, tau: rec.at_ms };
            apply_items(&mut self.w, &items, &e, Rows::History).ok()?;
            if self.w.ctl_seqs.len() >= MAX_CTL {
                return None;
            }
            self.w.ctl_seqs.push(rec.seq);
        }
        self.w.set_last_ctr(&rec.dev, rec.ctr);
        self.last_seq = rec.seq;
        Some(())
    }

    /// The agreement of `seal.md` §21.6 step 11 with a membership state: the
    /// same board id, devices with the same keys and statuses, current
    /// recovery key, epochs and commitments, and `ctl_seqs`.
    fn agrees(&self, m: &FakeMembership) -> bool {
        let p = &self.w;
        let devices_agree = p.devices.len() == m.devices.len()
            && p.devices.iter().zip(&m.devices).all(|(a, b)| {
                a.dev == b.dev && a.sign_pk == b.sign_pk && a.hpke_pk == b.hpke_pk && a.current == b.current
            });
        let key = |x: &FakeMembership| x.current_recovery().map(|r| (r.dev_r, r.sign_pk, r.hpke_pk));
        p.board_id == m.board_id
            && devices_agree
            && key(p) == key(m)
            && p.commitments == m.commitments
            && p.ctl_seqs == m.ctl_seqs
    }
}

// ---------- the sealer

/// The genesis anchor (`seal.md` §13.3).
struct Anchor {
    rsign_pk: Hash32,
    salt: [u8; 16],
    root: [u8; 32],
}

/// The author class of a frame at its position (`seal.md` §13.4).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Who {
    Device,
    Recovery,
    NonAuthority,
}

/// Why a control entry did not pass step 16.
enum Fail {
    Refuse(&'static str),
    Need(Vec<(u64, Hash32)>),
}

/// A rotation's epoch and its freshly drawn root, kept as pending under the entry id.
type DrawnRoot = (u32, [u8; 32]);

/// What `seal_as` composes: an entry's body, or the items of a K-shape entry.
enum Content<'a> {
    Body(&'a Body),
    KeyChange(Hash32, Hash32),
}

/// A snapshot body's material and its history records by seq.
type OpenedBody = (SnapshotMaterial, Vec<(u64, Vec<u8>)>);

/// The fake sealer (engine.md §6.3).
pub struct FakeSealer {
    rng: SplitMix64,
    dev: Dev,
    sk: [u8; 32],
    sign_pk: Hash32,
    hpke_pk: Hash32,
    board_id: [u8; 16],
    anchor: Option<Anchor>,
    /// The epoch roots it holds.
    roots: BTreeMap<u32, [u8; 32]>,
    /// The roots of its own unsent or unclassified rotations, by entry id: `(epoch, root)`.
    pending: BTreeMap<[u8; 16], (u32, [u8; 32])>,
    /// The wraps a login answer carried (ruling S24), by epoch; never persisted.
    missed: BTreeMap<u32, [u8; 96]>,
    /// One history record per accepted control entry, by seq (§6.3 "Control history").
    history: RefCell<BTreeMap<u64, Vec<u8>>>,
    /// The frames it sealed, by counter (SEAL-FRAME-005): `(frame, hash, epoch)`.
    sealed: BTreeMap<u64, (Vec<u8>, Hash32, u32)>,
    /// The rules versions this build supports (`seal.md` §14.5).
    supported_rules: Vec<u32>,
}

/// What a pairing hands the new device (engine.md §6.3, §37).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FakeBundle {
    /// Every epoch root the existing device holds, `(e, R_e)`.
    pub roots: Vec<(u32, [u8; 32])>,
    /// The bundle's snapshot (`seal.md` §17.8).
    pub material: SnapshotMaterial,
    /// The history records of the snapshot's control entries (`seal.md` §21.5).
    pub history: Vec<Vec<u8>>,
    /// The seq of the entry that added the new device.
    pub added_seq: u64,
}

/// The fake's device keys from a seed: the key (32 bytes), then `dev` (§6.3).
fn device_keys(rng: &mut SplitMix64) -> ([u8; 32], Dev) {
    let sk = draw::<32>(rng);
    let dev = draw::<16>(rng);
    (sk, dev)
}

impl FakeSealer {
    fn blank(rng: SplitMix64, sk: [u8; 32], dev: Dev, board_id: [u8; 16]) -> Self {
        let (sign_pk, hpke_pk) = public_keys(&sk);
        FakeSealer {
            rng,
            dev,
            sk,
            sign_pk,
            hpke_pk,
            board_id,
            anchor: None,
            roots: BTreeMap::new(),
            pending: BTreeMap::new(),
            missed: BTreeMap::new(),
            history: RefCell::new(BTreeMap::new()),
            sealed: BTreeMap::new(),
            supported_rules: vec![1],
        }
    }

    /// The genesis helper: the fake, frame 1 and the board bytes (engine.md §6.3).
    pub fn create(seed: u64, name: &str) -> (Self, Vec<u8>, Vec<u8>) {
        Self::create_at(seed, name, 0)
    }

    /// The genesis helper with the genesis entry's `at_ms`.
    // Used by the simulator stage `engine` writes, and by the tests.
    #[allow(dead_code)]
    pub(crate) fn create_at(seed: u64, name: &str, at_ms: i64) -> (Self, Vec<u8>, Vec<u8>) {
        let mut rng = SplitMix64 { state: seed };
        let salt = draw::<16>(&mut rng);
        let rsk = draw::<32>(&mut rng);
        let (rsign_pk, rhpke_pk) = public_keys(&rsk);
        let (sk, dev) = device_keys(&mut rng);
        let (sign_pk, hpke_pk) = public_keys(&sk);
        let root = draw::<32>(&mut rng);
        let entry_id = draw::<16>(&mut rng);
        let board_id = board_id_of(&rsign_pk, &salt);
        let dev_r = dev_r_of(&board_id, &rsign_pk);
        let commit = commitment(&board_id, 1, &root);
        let mut recipients = [(dev, hpke_pk), (dev_r, rhpke_pk)];
        recipients.sort_by_key(|r| r.0);
        let wraps = recipients.map(|(id, pk)| {
            let _ephemeral = draw::<32>(&mut rng);
            wrap(&id, &pk, 1, &root)
        });

        let toy = ToyMachine::new(false);
        let empty = ToyState { rules: 1, keys: BTreeMap::new(), items: BTreeMap::new(), marks: BTreeMap::new() };
        let board = toy.canonical_bytes(&empty);
        let mut g = GenesisItem {
            salt,
            rsign_pk,
            rhpke_pk,
            dev,
            sign_pk,
            hpke_pk,
            name: name.to_owned(),
            commit,
            wraps,
            rules: 1,
            hash_version: 1,
            state_hash: [0; 32],
        };
        let m1 = genesis_state(&g, board_id, dev_r, at_ms);
        g.state_hash = state_hash_of(&toy.board_hash(&empty), &m1.to_bytes());

        let mut fake = Self::blank(rng, sk, dev, board_id);
        fake.anchor = Some(Anchor { rsign_pk, salt, root });
        fake.roots.insert(1, root);
        let based_on = Head { seq: 0, chain: chain_zero_of(&board_id) };
        let items = vec![Item::Genesis(Box::new(g))];
        // A name over 255 bytes cannot be encoded: frame 1 is then empty and
        // every receiver skips it (`seal/frame_format`).
        let frame = fake.ctl_frame(&dev_r, &rsign_pk, 1, 1, &root, &entry_id, at_ms, &based_on, &items).unwrap_or_default();
        (fake, frame, board)
    }

    /// The pairing helper (engine.md §6.3), standing in for `seal.md` §17:
    /// the new device's keys from `seed`, and the roots the bundle hands over
    /// that match the snapshot membership's commitments (SEAL-KEY-013).
    pub fn join(seed: u64, name: &str, bundle: FakeBundle) -> Self {
        let mut rng = SplitMix64 { state: seed };
        let (sk, dev) = device_keys(&mut rng);
        let m = FakeMembership::from_bytes(&bundle.material.membership).unwrap_or_default();
        let mut fake = Self::blank(rng, sk, dev, m.board_id);
        for (e, root) in bundle.roots {
            if m.commitment_of(e).is_some_and(|c| *c == commitment(&m.board_id, e, &root)) {
                fake.roots.insert(e, root);
            }
        }
        let mut history = fake.history.borrow_mut();
        for bytes in bundle.history {
            let mut r = Reader::new(&bytes);
            if let Some(rec) = read_hist(&mut r).filter(|_| r.is_empty()) {
                history.insert(rec.seq, bytes.clone());
            }
        }
        drop(history);
        fake
    }

    /// The public keys a fake made by `join(seed, ..)` will have: `(dev, sign_pk, hpke_pk)`.
    // Used by the simulator stage `engine` writes, and by the tests.
    #[allow(dead_code)]
    pub(crate) fn join_keys(seed: u64) -> (Dev, Hash32, Hash32) {
        let (sk, dev) = device_keys(&mut SplitMix64 { state: seed });
        let (sign_pk, hpke_pk) = public_keys(&sk);
        (dev, sign_pk, hpke_pk)
    }

    /// The bundle an existing device hands a new one (engine.md §6.3).
    // Used by the simulator stage `engine` writes, and by the tests.
    #[allow(dead_code)]
    pub(crate) fn bundle(&self, material: SnapshotMaterial, added_seq: u64) -> FakeBundle {
        let history = self.history.borrow().range(..=material.seq).map(|(_, b)| b.clone()).collect();
        FakeBundle { roots: self.roots.iter().map(|(e, r)| (*e, *r)).collect(), material, history, added_seq }
    }

    /// Sets the rules versions this build supports (the simulator's `rules2`, §31.1).
    // Used by the simulator stage `engine` writes, and by the tests.
    #[allow(dead_code)]
    pub(crate) fn set_supported_rules(&mut self, rules: &[u32]) {
        self.supported_rules = rules.to_vec();
    }

    /// Composes, seals and signs a `ctl` frame by any author (the genesis helper, the tests).
    #[allow(clippy::too_many_arguments)]
    fn ctl_frame(
        &mut self,
        author: &Dev,
        author_pk: &Hash32,
        ctr: u64,
        epoch: u32,
        root: &[u8; 32],
        entry_id: &[u8; 16],
        at_ms: i64,
        based_on: &Head,
        items: &[Item],
    ) -> Result<Vec<u8>, SealerError> {
        let payload = self.ctl_payload(author, author_pk, ctr, entry_id, at_ms, items)?;
        self.entry_frame(author, author_pk, ctr, epoch, root, entry_id, 2, at_ms, based_on, &payload)
    }

    fn ctl_payload(
        &self,
        author: &Dev,
        author_pk: &Hash32,
        ctr: u64,
        entry_id: &[u8; 16],
        at_ms: i64,
        items: &[Item],
    ) -> Result<Vec<u8>, SealerError> {
        let mut payload = encode_items(items).ok_or(SealerError { code: code::BAD_PAYLOAD })?;
        let sig = sign(author_pk, &ctl_inner(&self.board_id, author, ctr, entry_id, at_ms, &payload));
        payload.extend_from_slice(&sig);
        Ok(payload)
    }

    /// Pads, draws the nonce, seals and signs one entry.
    #[allow(clippy::too_many_arguments)]
    fn entry_frame(
        &mut self,
        author: &Dev,
        author_pk: &Hash32,
        ctr: u64,
        epoch: u32,
        root: &[u8; 32],
        entry_id: &[u8; 16],
        kind: u8,
        at_ms: i64,
        based_on: &Head,
        payload: &[u8],
    ) -> Result<Vec<u8>, SealerError> {
        let padded =
            build_plain(entry_id, kind, at_ms, based_on, payload).ok_or(SealerError { code: code::PAYLOAD_TOO_LARGE })?;
        let nonce = draw::<24>(&mut self.rng);
        let env = Envelope {
            board_id: &self.board_id,
            dev: author,
            sign_pk: author_pk,
            ctr,
            ver: 1,
            suite: 1,
            epoch,
            key: entry_key(&self.board_id, epoch, root),
        };
        Ok(build_frame(&env, &nonce, &padded))
    }

    /// Draws a root and wraps it to `recipients` (§6.1 `seal`'s draw order).
    fn rotation(
        &mut self,
        m: &FakeMembership,
        recipients: &BTreeMap<Dev, Hash32>,
        purge_before: u64,
    ) -> Result<(Item, (u32, [u8; 32])), SealerError> {
        let n = m.epoch.checked_add(1).ok_or(SealerError { code: code::CTL_EPOCH })?;
        let root = draw::<32>(&mut self.rng);
        let commit = commitment(&self.board_id, n, &root);
        let mut wraps = Vec::with_capacity(recipients.len());
        for (id, pk) in recipients {
            let _ephemeral = draw::<32>(&mut self.rng);
            wraps.push(wrap(id, pk, n, &root));
        }
        Ok((Item::Rotated { epoch: n, commit, wraps, purge_before }, (n, root)))
    }

    /// The items of a control intent, composed against `m` (§6.1 `seal`).
    fn compose(&mut self, m: &FakeMembership, intent: &CtlIntent) -> Result<(Vec<Item>, Option<DrawnRoot>), SealerError> {
        let bad = SealerError { code: code::BAD_PAYLOAD };
        let current_devices = || -> BTreeMap<Dev, Hash32> {
            m.devices.iter().filter(|d| d.current).map(|d| (d.dev, d.hpke_pk)).collect()
        };
        let out = match intent {
            CtlIntent::Rotate { revoke, reinstate, purge_before } => {
                let mut items = Vec::with_capacity(revoke.len() + reinstate.len() + 1);
                let mut recipients = current_devices();
                for d in revoke {
                    items.push(Item::Revoked(*d));
                    recipients.remove(d);
                }
                for d in reinstate {
                    items.push(Item::Reinstated(*d));
                    if let Some(rec) = m.device(d) {
                        recipients.insert(*d, rec.hpke_pk);
                    }
                }
                if let Some(r) = m.current_recovery() {
                    recipients.insert(r.dev_r, r.hpke_pk);
                }
                let (item, root) = self.rotation(m, &recipients, purge_before.unwrap_or(0))?;
                items.push(item);
                (items, Some(root))
            }
            CtlIntent::AddDevice { dev, sign_pk, hpke_pk, name, transcript, bootstrap_seq } => {
                if !name_ok(name) {
                    return Err(bad);
                }
                let item = Item::DeviceAdded {
                    dev: *dev,
                    sign_pk: *sign_pk,
                    hpke_pk: *hpke_pk,
                    name: name.clone(),
                    transcript: *transcript,
                    bootstrap_seq: *bootstrap_seq,
                };
                (vec![item], None)
            }
            CtlIntent::Fence { host, door, home_gen } => {
                if !host_ok(host) {
                    return Err(SealerError { code: code::HOST_FORMAT });
                }
                (vec![Item::Fence { host: host.clone(), door: *door, home_gen: *home_gen }], None)
            }
            CtlIntent::ResetTo { seq, state_hash, hash_version } => {
                (vec![Item::ResetTo { seq: *seq, state_hash: *state_hash, hash_version: *hash_version }], None)
            }
            CtlIntent::Skip { targets } => {
                (targets.iter().map(|(seq, frame_hash)| Item::Skip { seq: *seq, frame_hash: *frame_hash }).collect(), None)
            }
            CtlIntent::RulesUpgrade { rules, mode } => {
                let mode = if matches!(mode, UpgradeMode::Normal) { 1 } else { 2 };
                (vec![Item::Upgrade { rules: *rules, mode }], None)
            }
        };
        Ok(out)
    }

    /// The items of a K-shape entry (`seal.md` §12.1): the new recovery key,
    /// then a rotation wrapping to the current devices and the new key.
    fn compose_key_change(
        &mut self,
        m: &FakeMembership,
        sign_pk: Hash32,
        hpke_pk: Hash32,
    ) -> Result<(Vec<Item>, Option<DrawnRoot>), SealerError> {
        let mut recipients: BTreeMap<Dev, Hash32> =
            m.devices.iter().filter(|d| d.current).map(|d| (d.dev, d.hpke_pk)).collect();
        recipients.insert(dev_r_of(&self.board_id, &sign_pk), hpke_pk);
        let (item, root) = self.rotation(m, &recipients, 0)?;
        Ok((vec![Item::Recovery { sign_pk, hpke_pk }, item], Some(root)))
    }

    /// `seal` for any author: this device's own entries (the trait), or the
    /// recovery identity's (`seal_as_recovery`). Only own entries keep their
    /// frame (SEAL-FRAME-005) and the pending root of their rotation.
    fn seal_as(
        &mut self,
        author: &Dev,
        author_pk: &Hash32,
        m: &FakeMembership,
        ctr: u64,
        entry: &PlainEntry,
        content: Content<'_>,
    ) -> Result<Sealed, SealerError> {
        if ctr == 0 {
            return Err(SealerError { code: code::FRAME_FORMAT });
        }
        let own = *author == self.dev;
        if let Some((frame, frame_hash, epoch)) = self.sealed.get(&ctr).filter(|_| own) {
            return Ok(Sealed { frame: frame.clone(), frame_hash: *frame_hash, epoch: *epoch, keys_changed: false });
        }
        let based_on_ok = entry.based_on.seq != 0 || entry.based_on.chain == chain_zero_of(&self.board_id);
        if !based_on_ok || !(0..=AT_MS_MAX).contains(&entry.at_ms) {
            return Err(SealerError { code: code::BAD_PLAINTEXT });
        }
        let epoch = m.epoch;
        let root = *self.roots.get(&epoch).ok_or(SealerError { code: code::MISSING_ROOT })?;
        let (kind, payload, drawn) = match content {
            Content::Body(Body::Cmd(p)) => (1, p.clone(), None),
            Content::Body(Body::Beat(b)) => (3, encode_beat(b).ok_or(SealerError { code: code::BAD_PAYLOAD })?, None),
            Content::Body(Body::Snap(i)) => (4, encode_snap(i), None),
            Content::Body(Body::Ctl(intent)) => {
                let (items, drawn) = self.compose(m, intent)?;
                (2, self.ctl_payload(author, author_pk, ctr, &entry.entry_id, entry.at_ms, &items)?, drawn)
            }
            Content::KeyChange(sign_pk, hpke_pk) => {
                let (items, drawn) = self.compose_key_change(m, sign_pk, hpke_pk)?;
                (2, self.ctl_payload(author, author_pk, ctr, &entry.entry_id, entry.at_ms, &items)?, drawn)
            }
        };
        let frame =
            self.entry_frame(author, author_pk, ctr, epoch, &root, &entry.entry_id, kind, entry.at_ms, &entry.based_on, &payload)?;
        let frame_hash = h(&[&frame]);
        let mut keys_changed = false;
        if own {
            if let Some(pending) = drawn {
                self.pending.insert(entry.entry_id, pending);
                keys_changed = true;
            }
            self.sealed.insert(ctr, (frame.clone(), frame_hash, epoch));
        }
        Ok(Sealed { frame, frame_hash, epoch, keys_changed })
    }

    /// Seals `entry` as the current recovery identity of `m` with counter
    /// `ctr` (its `last_ctr + 1`), as the recovery tool does (engine.md §31.5):
    /// a `Body::Ctl` is composed as `seal` composes it (shape R from `Rotate`,
    /// F, S). The fake's signature needs only the public key `m` records, so
    /// any fake of the board can stand in for the sheet. Draws as `seal`; keeps
    /// neither the frame nor a pending root, so `keys_changed` is false.
    // Used by the simulator stage `engine` writes, and by the tests.
    #[allow(dead_code)]
    pub(crate) fn seal_as_recovery(&mut self, m: &FakeMembership, ctr: u64, entry: &PlainEntry) -> Result<Sealed, SealerError> {
        let r = m.current_recovery().ok_or(SealerError { code: code::BAD_PAYLOAD })?;
        let (dev_r, rsign_pk) = (r.dev_r, r.sign_pk);
        self.seal_as(&dev_r, &rsign_pk, m, ctr, entry, Content::Body(&entry.body))
    }

    /// Seals a K-shape entry (`seal.md` §12.1, §13.11) as the current recovery
    /// identity of `m`: `RecoveryKeyChange` to `new_key` (`sign_pk`,
    /// `hpke_pk`), then `EpochRotated` wrapping to the current devices and the
    /// new key. `entry.body` is not read. Draws the root, the wraps, the nonce.
    // Used by the simulator stage `engine` writes, and by the tests.
    #[allow(dead_code)]
    pub(crate) fn seal_recovery_key_change(
        &mut self,
        m: &FakeMembership,
        ctr: u64,
        entry: &PlainEntry,
        new_key: (Hash32, Hash32),
    ) -> Result<Sealed, SealerError> {
        let r = m.current_recovery().ok_or(SealerError { code: code::BAD_PAYLOAD })?;
        let (dev_r, rsign_pk) = (r.dev_r, r.sign_pk);
        self.seal_as(&dev_r, &rsign_pk, m, ctr, entry, Content::KeyChange(new_key.0, new_key.1))
    }

    /// The wraps of the `reinstate` admin operation (`seal.md` §16.2, ruling
    /// S24) for the revoked device `dev` of `m`: `(first_epoch, wraps)`, the
    /// roots of `first_epoch` (the epoch the revoking entry's rotation set up)
    /// to `m.epoch` (the epoch the reinstating entry is sealed under), in
    /// order, each wrapped to the target's `hpke_pk`; one 32-byte draw per
    /// wrap. `seal/ctl_target` when `dev` is no device that a device revoked
    /// (§13.6 `DeviceReinstated`) by an entry at or above `m.purge_seq`; `seal/missing_root` when this
    /// fake lacks one of the roots; `seal/bad_payload` for more than 256 wraps.
    // Used by the simulator stage `engine` writes, and by the tests.
    #[allow(dead_code)]
    pub(crate) fn reinstate_wraps(&mut self, m: &FakeMembership, dev: &Dev) -> Result<(u32, Vec<[u8; 96]>), SealerError> {
        let target = SealerError { code: code::CTL_TARGET };
        let by_device = |d: &&DevRec| m.device(&d.status_by).is_some();
        let d = m.device(dev).filter(|d| !d.current && d.status_seq >= m.purge_seq).filter(by_device).ok_or(target.clone())?;
        let first = m.commitments.iter().find(|(_, seq, _)| *seq == d.status_seq).map(|(e, _, _)| *e).ok_or(target)?;
        let n = m.epoch.checked_sub(first).map_or(0, |x| x + 1);
        if n > 256 {
            return Err(SealerError { code: code::BAD_PAYLOAD });
        }
        let (id, pk) = (d.dev, d.hpke_pk);
        let mut wraps = Vec::with_capacity(n as usize);
        for e in first..=m.epoch {
            let root = *self.roots.get(&e).ok_or(SealerError { code: code::MISSING_ROOT })?;
            let _ephemeral = draw::<32>(&mut self.rng);
            wraps.push(wrap(&id, &pk, e, &root));
        }
        Ok((first, wraps))
    }

    /// The author of the frame at `s` and its class (`seal.md` §13.4, §14.2 step 3).
    fn author_at(&self, m: &FakeMembership, s: u64, dev: &Dev) -> Option<(Who, Hash32, u64)> {
        if s == 1 {
            let a = self.anchor.as_ref()?;
            return (*dev == dev_r_of(&self.board_id, &a.rsign_pk)).then_some((Who::Recovery, a.rsign_pk, 0));
        }
        if let Some(d) = m.device(dev) {
            let who = if d.current { Who::Device } else { Who::NonAuthority };
            return Some((who, d.sign_pk, d.last_ctr));
        }
        let r = m.recovery.iter().find(|r| r.dev_r == *dev)?;
        let who = if r.until_seq == 0 && r.since_seq < s { Who::Recovery } else { Who::NonAuthority };
        Some((who, r.sign_pk, r.last_ctr))
    }

    fn classify_inner(&mut self, input: &ClassifyInput<'_, FakeMembership>) -> Result<Receipt<FakeMembership>, SealerError> {
        let s = input.seq;
        // Seq 1 is classified from the anchor alone, from `M_0`, whatever the caller passes (§13.3).
        let m_zero = FakeMembership::default();
        let m0 = if s == 1 { &m_zero } else { input.membership };
        let frame_hash = h(&[input.frame]);
        let mut r = Receipt {
            class: Class::Skip,
            code: None,
            chain: chain_next(&input.prev_chain, s, &frame_hash),
            author: [0; 16],
            ctr: 0,
            frame_hash,
            own_dev_foreign: false,
            based_on_unchecked: false,
            opened: None,
            membership: m0.clone(),
            keys_changed: false,
            halt: None,
        };
        let skip = |mut r: Receipt<FakeMembership>, c| {
            r.code = Some(c);
            Ok(r)
        };
        // Step 1.
        let Some(f) = parse_frame(input.frame) else { return skip(r, code::FRAME_FORMAT) };
        r.author = f.dev;
        r.ctr = f.ctr;
        // Steps 2 to 4.
        if f.board_id != self.board_id {
            return skip(r, code::WRONG_BOARD);
        }
        let Some((who, sign_pk, last_ctr)) = self.author_at(m0, s, &f.dev) else {
            return skip(r, code::UNKNOWN_AUTHOR);
        };
        if !verify(&sign_pk, &frame_signed(f.signed_prefix), f.sig) {
            return skip(r, code::BAD_SIGNATURE);
        }
        // Step 5.
        if last_ctr.checked_add(1) != Some(f.ctr) {
            r.class = Class::Tamper;
            r.code = Some(if f.ctr <= last_ctr { code::COUNTER_REPLAY } else { code::COUNTER_GAP });
            return Ok(r);
        }
        // Step 6.
        if f.dev == self.dev && (input.own_sealed)(f.ctr) != Some(frame_hash) {
            r.own_dev_foreign = true;
        }
        let refuse = |mut r: Receipt<FakeMembership>, c| {
            r.membership.set_last_ctr(&f.dev, f.ctr);
            r.class = Class::Refuse;
            r.code = Some(c);
            Ok(r)
        };
        // Step 7.
        if who == Who::NonAuthority {
            return refuse(r, code::REVOKED_AUTHOR);
        }
        // Step 8.
        let (ver, suite) = (f.body[0], f.body[1]);
        let rules = if s == 1 { 1 } else { m0.rules };
        if !envelopes_for(rules).contains(&(ver, suite)) {
            return refuse(r, code::BAD_VERSION);
        }
        // Step 9, with condition 4 of §13.9 (30 days of log clock) before decryption.
        let epoch = u32::from_be_bytes([f.body[2], f.body[3], f.body[4], f.body[5]]);
        let current = if s == 1 { 1 } else { m0.epoch };
        let stale = s != 1 && current >= 2 && epoch == current - 1;
        if epoch != current && !stale {
            return refuse(r, if epoch > current || s == 1 { code::BAD_EPOCH } else { code::OLD_EPOCH });
        }
        if stale && m0.clock_ms.saturating_sub(m0.epoch_clock_ms) > WINDOW_30_DAYS {
            return refuse(r, code::STALE_EPOCH);
        }
        // A root this device lacks, or one that does not match the commitment
        // its membership records (SEAL-KEY-013), is its own store's fault.
        let root = if s == 1 {
            self.anchor.as_ref().map(|a| a.root)
        } else {
            self.roots.get(&epoch).copied().filter(|x| m0.commitment_of(epoch) == Some(&commitment(&self.board_id, epoch, x)))
        };
        let root = root.ok_or(SealerError { code: code::MISSING_ROOT })?;
        // Step 10.
        let ad = entry_ad(f.header, ver, suite, epoch);
        let Some(padded) = aead_open(&entry_key(&self.board_id, epoch, &root), &ad, &f.body[BODY_PREFIX..]) else {
            return refuse(r, code::AEAD);
        };
        // Step 11.
        let Some(p) = parse_plain(padded) else { return refuse(r, code::BAD_PLAINTEXT) };
        let based_on = Head { seq: p.based_on_seq, chain: p.based_on_chain };
        r.opened = Some(Opened {
            entry_id: p.entry_id,
            kind: kind_of(p.kind),
            at_ms: p.at_ms,
            based_on: Head { seq: p.based_on_seq, chain: p.based_on_chain },
            payload: p.payload.to_vec(),
            items: Vec::new(),
            beat: None,
            snap: None,
        });
        // Step 12.
        let b = based_on.seq;
        let fork = if b >= s {
            r.class = Class::Tamper;
            r.code = Some(code::BASED_ON_FUTURE);
            return Ok(r);
        } else if b == 0 {
            based_on.chain != chain_zero_of(&self.board_id)
        } else if b < input.retained_from {
            r.based_on_unchecked = true;
            false
        } else {
            match (input.chain_at)(b) {
                Some(c) => c != based_on.chain,
                None => {
                    r.based_on_unchecked = true;
                    false
                }
            }
        };
        if fork {
            r.class = Class::Tamper;
            r.code = Some(code::BASED_ON_FORK);
            return Ok(r);
        }
        // Step 13.
        if who == Who::Recovery && p.kind != 2 {
            return refuse(r, code::NOT_AUTHORIZED);
        }
        if stale && (p.kind == 2 || p.kind == 4) {
            return refuse(r, code::STALE_EPOCH);
        }
        // Step 14: conditions 1 to 3 of §13.9 (step 13 refused the other kinds).
        if stale && !(who == Who::Device && b < m0.epoch_seq) {
            return refuse(r, code::STALE_EPOCH);
        }
        // Step 15.
        let tau = m0.clock_ms.max(p.at_ms);
        let mut w = m0.clone();
        let mut beat = None;
        let mut snap = None;
        let mut ctl = None;
        match p.kind {
            // A `cmd` payload is opaque here (SEAL-KIND-002): the engine's parser judges it.
            1 => {}
            2 => {
                let Some(c) = parse_ctl(p.payload) else { return refuse(r, code::BAD_PAYLOAD) };
                let inner = ctl_inner(&self.board_id, &f.dev, f.ctr, &p.entry_id, p.at_ms, c.items_bytes);
                if !verify(&sign_pk, &inner, c.sig) {
                    return refuse(r, code::BAD_CTL_SIG);
                }
                ctl = Some(c);
            }
            3 => match parse_beat(p.payload, b, s) {
                Some(i) => beat = Some(i),
                None => return refuse(r, code::BAD_PAYLOAD),
            },
            _ => match parse_snap(p.payload, b, epoch) {
                Some(i) => snap = Some(i),
                None => return refuse(r, code::BAD_PAYLOAD),
            },
        }
        // Step 16.
        if let Some(c) = &ctl {
            if !shape_ok(&c.items, s) {
                return refuse(r, code::CTL_SHAPE);
            }
            let e = EntryCtx { s, author: f.dev, who, at_ms: p.at_ms, tau };
            let outcome =
                if s == 1 { self.genesis(&mut w, &c.items, &f, p.at_ms) } else { apply_items(&mut w, &c.items, &e, Rows::Log(input)) };
            match outcome {
                Ok(()) => {}
                Err(Fail::Refuse(c)) => return refuse(r, c),
                Err(Fail::Need(frames)) => {
                    r.class = Class::NeedFrames(frames);
                    return Ok(r);
                }
            }
            if w.ctl_seqs.last() != Some(&s) {
                if w.ctl_seqs.len() >= MAX_CTL {
                    return refuse(r, code::CTL_CAP);
                }
                w.ctl_seqs.push(s);
            }
        }
        // Step 17: accept, with the bookkeeping of §13.5 and §13.10.
        w.set_last_ctr(&f.dev, f.ctr);
        w.clock_ms = tau;
        if let (Some(i), Some(d)) = (&beat, w.device_mut(&f.dev)) {
            d.beat_ms = d.beat_ms.max(p.at_ms);
            d.rules = i.rules.clone();
            d.envelopes = i.envelopes.clone();
            d.hash_versions = i.hash_versions.clone();
        }
        let items_view = ctl.as_ref().map(|c| c.items.iter().map(opened_item).collect()).unwrap_or_default();
        if let Some(c) = &ctl {
            self.record_history(s, &f, &p, &frame_hash, c);
            let own_entry = (f.dev == self.dev).then_some(&p.entry_id);
            self.accept_keys(&mut r.keys_changed, &mut r.halt, &w, &c.items, own_entry);
            if w.rules != m0.rules && s != 1 && !self.supported_rules.contains(&w.rules) && r.halt.is_none() {
                r.halt = Some(Halt::UnknownRules(w.rules));
            }
        }
        if let Some(o) = r.opened.as_mut() {
            o.items = items_view;
            o.beat = beat;
            o.snap = snap;
        }
        r.code = match &r.halt {
            Some(Halt::Commitment) => Some(code::COMMITMENT),
            Some(Halt::UnknownRules(_)) => Some(code::UNKNOWN_RULES),
            None => None,
        };
        r.class = Class::Accept;
        r.membership = w;
        Ok(r)
    }

    /// The Genesis checks of `seal.md` §13.3 (SEAL-MEM-004, SEAL-MEM-005, SEAL-KEY-017).
    fn genesis(&self, w: &mut FakeMembership, items: &[Item], f: &FrameView<'_>, at_ms: i64) -> Result<(), Fail> {
        let fail = Fail::Refuse(code::CTL_GENESIS);
        let (Some(a), [Item::Genesis(g)]) = (self.anchor.as_ref(), items) else { return Err(fail) };
        let board_id = board_id_of(&g.rsign_pk, &g.salt);
        let dev_r = dev_r_of(&board_id, &g.rsign_pk);
        let mut ids = [g.dev, dev_r];
        ids.sort_unstable();
        let ok = g.salt == a.salt
            && g.rsign_pk == a.rsign_pk
            && board_id == f.board_id
            && g.commit == commitment(&board_id, 1, &a.root)
            && g.dev != dev_r
            && first16_of(&g.wraps[0]) == ids[0]
            && first16_of(&g.wraps[1]) == ids[1];
        if !ok {
            return Err(fail);
        }
        *w = genesis_state(g, board_id, dev_r, at_ms);
        Ok(())
    }

    /// Appends the history record of an accepted control entry (§6.3 "Control history").
    fn record_history(&self, s: u64, f: &FrameView<'_>, p: &Plain<'_>, frame_hash: &Hash32, c: &CtlPayload<'_>) {
        let rec = HistRec {
            seq: s,
            dev: f.dev,
            ctr: f.ctr,
            entry_id: p.entry_id,
            at_ms: p.at_ms,
            frame_hash: *frame_hash,
            items: c.items_bytes,
            sig: c.sig,
        };
        self.history.borrow_mut().insert(s, rec.to_bytes());
    }

    /// The roots an accepted rotation gives this device, and the commitment halt (`seal.md` §14.5).
    fn accept_keys(
        &mut self,
        keys_changed: &mut bool,
        halt: &mut Option<Halt>,
        w: &FakeMembership,
        items: &[Item],
        own_entry: Option<&[u8; 16]>,
    ) {
        for item in items {
            let Item::Rotated { epoch, commit, wraps, .. } = item else { continue };
            let own = wraps.iter().find(|x| first16_of(x) == self.dev).copied();
            let missed = self.missed.get(epoch).filter(|_| !self.roots.contains_key(epoch)).copied();
            if let Some(x) = own.or(missed) {
                match unwrap(&x, &self.dev, &self.hpke_pk, *epoch, commit, &w.board_id) {
                    Some(root) => {
                        if self.roots.insert(*epoch, root) != Some(root) {
                            *keys_changed = true;
                        }
                        self.missed.remove(epoch);
                    }
                    None => *halt = Some(Halt::Commitment),
                }
            }
            // An own rotation: its pending root goes in the same call (ENG-SEALER-009).
            if own_entry.is_some_and(|id| self.pending.remove(id).is_some()) {
                *keys_changed = true;
            }
        }
    }
}

/// The entry-wide inputs of the item rules (`seal.md` §13.6).
struct EntryCtx {
    s: u64,
    author: Dev,
    who: Who,
    at_ms: i64,
    tau: i64,
}

/// The current devices after the Revoke and Reinstate items of `rest` (`seal/ctl_last_device`).
fn current_after(w: &FakeMembership, rest: &[Item]) -> usize {
    let mut current: BTreeMap<Dev, bool> = w.devices.iter().map(|d| (d.dev, d.current)).collect();
    for item in rest {
        match item {
            Item::Revoked(d) => {
                current.insert(*d, false);
            }
            Item::Reinstated(d) => {
                current.insert(*d, true);
            }
            _ => {}
        }
    }
    current.values().filter(|c| **c).count()
}

/// Installs a new recovery key (`seal.md` §13.6 `RecoveryKeyChange`).
fn install_recovery(w: &mut FakeMembership, s: u64, sign_pk: Hash32, hpke_pk: Hash32) -> Result<(), Fail> {
    let dev_r = dev_r_of(&w.board_id, &sign_pk);
    if w.recorded(&dev_r, &sign_pk, &hpke_pk) {
        return Err(Fail::Refuse(code::CTL_DUPLICATE_KEY));
    }
    if w.recovery.len() >= MAX_RECOVERY {
        return Err(Fail::Refuse(code::CTL_CAP));
    }
    for r in &mut w.recovery {
        if r.until_seq == 0 {
            r.until_seq = s;
        }
    }
    w.recovery.push(RecRec { dev_r, sign_pk, hpke_pk, since_seq: s, until_seq: 0, last_ctr: 0 });
    Ok(())
}

/// The Skip items of an `S`-shape entry, in the two passes of `seal.md` §14.6.
fn apply_skips(w: &mut FakeMembership, items: &[Item], e: &EntryCtx, input: &ClassifyInput<'_, FakeMembership>) -> Result<(), Fail> {
    let targets: Vec<(u64, Hash32)> =
        items.iter().filter_map(|i| if let Item::Skip { seq, frame_hash } = i { Some((*seq, *frame_hash)) } else { None }).collect();
    let floor = w.purge_seq.max(e.s.saturating_sub(SKIP_WINDOW));
    for (t, _) in &targets {
        let in_window = *t < e.s && *t >= floor;
        if !in_window || w.ctl_seqs.binary_search(t).is_ok() || w.skips.iter().any(|(x, _)| x == t) {
            return Err(Fail::Refuse(code::CTL_TARGET));
        }
    }
    for (t, fh) in &targets {
        let Some(frame) = (input.skip_targets)(*t) else {
            let missing = targets.iter().filter(|(x, _)| (input.skip_targets)(*x).is_none()).copied().collect();
            return Err(Fail::Need(missing));
        };
        if h(&[frame]) != *fh {
            return Err(Fail::Refuse(code::CTL_TARGET));
        }
        // The author is the 16 bytes at offset 20, which `frame_hash`
        // authenticates; a frame shorter than that has none (§14.6).
        if e.who == Who::Device && frame.get(20..36) == Some(e.author.as_slice()) {
            return Err(Fail::Refuse(code::CTL_AUTHOR));
        }
    }
    if w.skips.len() + targets.len() > MAX_SKIPS {
        return Err(Fail::Refuse(code::CTL_CAP));
    }
    w.skips.extend(targets);
    w.skips.sort_by_key(|(t, _)| *t);
    Ok(())
}

/// Which rows of `seal.md` §13.6 the items run.
enum Rows<'a, 'b> {
    /// Classification: every row whole, with the caller's Skip targets.
    Log(&'a ClassifyInput<'b, FakeMembership>),
    /// The projection of §21.6 step 6: `Fence`, `ResetTo`, `Skip` and
    /// `RulesUpgrade` check only their Author column and change nothing.
    History,
}

/// Applies the items of an accepted-so-far control entry to `W` (`seal.md` §13.6).
fn apply_items(w: &mut FakeMembership, items: &[Item], e: &EntryCtx, rows: Rows<'_, '_>) -> Result<(), Fail> {
    let input = match rows {
        Rows::Log(input) => Some(input),
        Rows::History => None,
    };
    if matches!(items.first(), Some(Item::Skip { .. })) {
        // An S-shape entry: by a current device or the current recovery identity, both authorities here.
        return match input {
            Some(input) => apply_skips(w, items, e, input),
            None => Ok(()),
        };
    }
    let refuse = |c| Err(Fail::Refuse(c));
    let device = e.who == Who::Device;
    let recovery = e.who == Who::Recovery;
    for (k, item) in items.iter().enumerate() {
        match item {
            Item::Genesis(_) | Item::Skip { .. } => return refuse(code::CTL_SHAPE),
            Item::DeviceAdded { dev, sign_pk, hpke_pk, name, bootstrap_seq, .. } => {
                if !device {
                    return refuse(code::CTL_AUTHOR);
                }
                if w.recorded(dev, sign_pk, hpke_pk) {
                    return refuse(code::CTL_DUPLICATE_KEY);
                }
                if w.current_count() >= MAX_CURRENT || w.devices.len() >= MAX_RECORDS || w.next_device_no > MAX_DEVICE_NO {
                    return refuse(code::CTL_DEVICE_CAP);
                }
                if *bootstrap_seq < w.purge_seq || *bootstrap_seq >= e.s {
                    return refuse(code::CTL_TARGET);
                }
                let rec = DevRec {
                    dev: *dev,
                    sign_pk: *sign_pk,
                    hpke_pk: *hpke_pk,
                    device_no: w.next_device_no,
                    current: true,
                    added_seq: e.s,
                    added_by: e.author,
                    bootstrap_seq: *bootstrap_seq,
                    status_seq: e.s,
                    status_by: e.author,
                    last_ctr: 0,
                    beat_ms: e.at_ms,
                    rules: vec![w.rules],
                    envelopes: envelopes_for(w.rules),
                    hash_versions: vec![1],
                    name: name.clone(),
                };
                let at = w.devices.binary_search_by(|d| d.dev.cmp(dev)).unwrap_or_else(|i| i);
                w.devices.insert(at, rec);
                w.next_device_no += 1;
            }
            Item::Revoked(t) => {
                if !(recovery || (device && *t != e.author)) {
                    return refuse(code::CTL_AUTHOR);
                }
                if !w.is_current_device(t) {
                    return refuse(code::CTL_TARGET);
                }
                if current_after(w, &items[k..]) == 0 {
                    return refuse(code::CTL_LAST_DEVICE);
                }
                if let Some(d) = w.device_mut(t) {
                    d.current = false;
                    d.status_seq = e.s;
                    d.status_by = e.author;
                }
                if w.pending_override.is_some_and(|(_, voter, _)| voter == *t) {
                    w.pending_override = None;
                }
            }
            Item::Reinstated(t) => {
                if !recovery {
                    return refuse(code::CTL_AUTHOR);
                }
                let by_device = w.device(t).is_some_and(|d| !d.current && w.device(&d.status_by).is_some());
                if !by_device {
                    return refuse(code::CTL_TARGET);
                }
                if w.current_count() >= MAX_CURRENT {
                    return refuse(code::CTL_DEVICE_CAP);
                }
                if let Some(d) = w.device_mut(t) {
                    d.current = true;
                    d.status_seq = e.s;
                    d.status_by = e.author;
                    d.beat_ms = e.at_ms;
                }
            }
            Item::Rotated { epoch, commit, wraps, purge_before } => {
                if w.epoch.checked_add(1) != Some(*epoch) {
                    return refuse(code::CTL_EPOCH);
                }
                let mut recipients: Vec<Dev> = w.devices.iter().filter(|d| d.current).map(|d| d.dev).collect();
                if let Some(r) = w.current_recovery() {
                    recipients.push(r.dev_r);
                }
                recipients.sort_unstable();
                let named: Vec<Dev> = wraps.iter().map(first16_of).collect();
                if named != recipients {
                    return refuse(code::CTL_WRAPS);
                }
                if *purge_before != 0 {
                    if !device {
                        return refuse(code::CTL_AUTHOR);
                    }
                    if !(w.purge_seq < *purge_before && *purge_before <= e.s) {
                        return refuse(code::CTL_TARGET);
                    }
                }
                if w.commitments.len() >= MAX_COMMITMENTS {
                    return refuse(code::CTL_CAP);
                }
                w.epoch = *epoch;
                w.epoch_seq = e.s;
                w.epoch_clock_ms = e.tau;
                w.commitments.push((*epoch, e.s, *commit));
                if *purge_before != 0 {
                    w.purge_seq = *purge_before;
                }
            }
            Item::Recovery { sign_pk, hpke_pk } => {
                if !recovery {
                    return refuse(code::CTL_AUTHOR);
                }
                install_recovery(w, e.s, *sign_pk, *hpke_pk)?;
            }
            // The projection of §21.6 checks only the Author column (D or R: always met here).
            Item::Fence { .. } if input.is_none() => {}
            Item::Fence { host, door, home_gen } => {
                if w.home_gen.checked_add(1) != Some(*home_gen) {
                    return refuse(code::CTL_HOME_GEN);
                }
                w.home_gen = *home_gen;
                w.home_seq = e.s;
                w.home_host = host.clone();
                w.home_door = *door;
            }
            Item::ResetTo { seq, state_hash, hash_version } => {
                if !device {
                    return refuse(code::CTL_AUTHOR);
                }
                if input.is_none() {
                    continue;
                }
                let above_bootstraps = w.devices.iter().filter(|d| d.current).all(|d| *seq > d.bootstrap_seq);
                if *seq >= e.s || *seq < w.purge_seq || !above_bootstraps || !hash_versions_for(w.rules).contains(hash_version) {
                    return refuse(code::CTL_TARGET);
                }
                w.last_reset =
                    Some(LastReset { at_seq: e.s, target: *seq, hash_version: *hash_version, state_hash: *state_hash });
            }
            Item::Upgrade { rules, mode } => {
                if !device {
                    return refuse(code::CTL_AUTHOR);
                }
                if input.is_none() {
                    continue;
                }
                if *rules <= w.rules {
                    return refuse(code::CTL_TARGET);
                }
                if *mode == 1 {
                    let reference = w.devices.iter().filter(|d| d.current && d.dev != e.author).map(|d| d.beat_ms).max();
                    let active = |d: &DevRec| {
                        d.current
                            && (d.dev == e.author
                                || reference.is_some_and(|r| d.beat_ms >= r.saturating_sub(WINDOW_30_DAYS)))
                    };
                    if !w.devices.iter().filter(|d| active(d)).all(|d| d.rules.contains(rules)) {
                        return refuse(code::CTL_SUPPORT);
                    }
                    w.rules = *rules;
                    w.pending_override = None;
                } else {
                    match w.pending_override {
                        Some((v, voter, _)) if v == *rules && voter == e.author => return refuse(code::CTL_DUPLICATE),
                        Some((v, voter, _)) if v == *rules && w.is_current_device(&voter) => {
                            w.rules = *rules;
                            w.pending_override = None;
                        }
                        _ => w.pending_override = Some((*rules, e.author, e.s)),
                    }
                }
            }
        }
    }
    Ok(())
}

/// The engine's view of an opened item (engine.md §6.1 `CtlItem`).
fn opened_item(item: &Item) -> CtlItem {
    match item {
        Item::Genesis(g) => CtlItem::Genesis { hash_version: g.hash_version, state_hash: g.state_hash },
        Item::DeviceAdded { dev, sign_pk, hpke_pk, name, bootstrap_seq, .. } => CtlItem::DeviceAdded {
            dev: *dev,
            name: name.clone(),
            key_fp: key_fp(sign_pk, hpke_pk),
            bootstrap_seq: *bootstrap_seq,
        },
        Item::Revoked(dev) => CtlItem::DeviceRevoked { dev: *dev },
        Item::Reinstated(dev) => CtlItem::DeviceReinstated { dev: *dev },
        Item::Rotated { epoch, purge_before, .. } => {
            CtlItem::EpochRotated { epoch: *epoch, purge_before: (*purge_before != 0).then_some(*purge_before) }
        }
        Item::Recovery { sign_pk, hpke_pk } => CtlItem::RecoveryKeyChange { key_fp: key_fp(sign_pk, hpke_pk) },
        Item::Fence { host, door, home_gen } => CtlItem::Fence { host: host.clone(), door: *door, home_gen: *home_gen },
        Item::ResetTo { seq, state_hash, hash_version } => {
            CtlItem::ResetTo { seq: *seq, state_hash: *state_hash, hash_version: *hash_version }
        }
        Item::Skip { seq, frame_hash } => CtlItem::Skip { seq: *seq, frame_hash: *frame_hash },
        Item::Upgrade { rules, mode } => CtlItem::RulesUpgrade {
            rules: *rules,
            mode: if *mode == 1 { UpgradeMode::Normal } else { UpgradeMode::Override },
        },
    }
}

/// The snapshot body of `seal.md` §20.4 (uncompressed: §6.3).
fn snapshot_body(material: &SnapshotMaterial, history: &[Vec<u8>]) -> Option<Vec<u8>> {
    let mut o = Vec::new();
    o.extend_from_slice(SNAPSHOT_MAGIC);
    o.extend_from_slice(&material.seq.to_be_bytes());
    o.extend_from_slice(&material.chain);
    o.extend_from_slice(&material.hash_version.to_be_bytes());
    o.extend_from_slice(&material.state_hash);
    put_vb32(&mut o, &material.board)?;
    put_vb32(&mut o, &material.membership)?;
    o.extend_from_slice(&u32::try_from(history.len()).ok()?.to_be_bytes());
    for rec in history {
        o.extend_from_slice(rec);
    }
    Some(o)
}

/// `ctx("blob") ‖ board_id ‖ sealed[0..27] ‖ u64 S ‖ writer_dev` (`seal.md` §20.2).
fn blob_ad(board_id: &[u8; 16], header27: &[u8], s: u64, writer: &Dev) -> Vec<u8> {
    let mut ad = ctx("blob");
    ad.extend_from_slice(board_id);
    ad.extend_from_slice(header27);
    ad.extend_from_slice(&s.to_be_bytes());
    ad.extend_from_slice(writer);
    ad
}

impl Sealer for FakeSealer {
    type Membership = FakeMembership;

    fn own_dev(&self) -> Dev {
        self.dev
    }

    fn view(&self, m: &FakeMembership) -> MembershipView {
        m.to_view()
    }

    fn membership_bytes(&self, m: &FakeMembership) -> Vec<u8> {
        m.to_bytes()
    }

    fn membership_from_bytes(&self, bytes: &[u8]) -> Result<FakeMembership, SealerError> {
        FakeMembership::from_bytes(bytes).ok_or(SealerError { code: code::MEMBERSHIP_FORMAT })
    }

    fn supported_envelopes(&self) -> Vec<(u8, u8)> {
        vec![(1, 1)]
    }

    fn chain_zero(&self) -> Hash32 {
        chain_zero_of(&self.board_id)
    }

    fn chain_step(&self, prev: &Hash32, seq: u64, frame: &[u8]) -> Hash32 {
        chain_next(prev, seq, &h(&[frame]))
    }

    fn frame_hash(&self, frame: &[u8]) -> Hash32 {
        h(&[frame])
    }

    fn frame_author(&self, frame: &[u8]) -> Result<(Dev, u64), SealerError> {
        parse_frame(frame).map(|f| (f.dev, f.ctr)).ok_or(SealerError { code: code::FRAME_FORMAT })
    }

    fn classify(
        &mut self,
        input: &ClassifyInput<'_, FakeMembership>,
    ) -> Result<Receipt<FakeMembership>, SealerError> {
        self.classify_inner(input)
    }

    fn new_entry_id(&mut self) -> Result<[u8; 16], SealerError> {
        Ok(draw::<16>(&mut self.rng))
    }

    fn seal(&mut self, m: &FakeMembership, ctr: u64, entry: &PlainEntry) -> Result<Sealed, SealerError> {
        let (dev, sign_pk) = (self.dev, self.sign_pk);
        self.seal_as(&dev, &sign_pk, m, ctr, entry, Content::Body(&entry.body))
    }

    fn open_own(&self, m: &FakeMembership, frame: &[u8]) -> Result<[u8; 16], SealerError> {
        let err = |c| Err(SealerError { code: c });
        let Some(f) = parse_frame(frame) else { return err(code::FRAME_FORMAT) };
        if f.dev != self.dev || f.board_id != self.board_id {
            return err(code::UNKNOWN_AUTHOR);
        }
        if !verify(&self.sign_pk, &frame_signed(f.signed_prefix), f.sig) {
            return err(code::BAD_SIGNATURE);
        }
        let (ver, suite) = (f.body[0], f.body[1]);
        let epoch = u32::from_be_bytes([f.body[2], f.body[3], f.body[4], f.body[5]]);
        let Some(root) = self.roots.get(&epoch) else { return err(code::MISSING_ROOT) };
        let ad = entry_ad(f.header, ver, suite, epoch);
        let Some(padded) = aead_open(&entry_key(&self.board_id, epoch, root), &ad, &f.body[BODY_PREFIX..]) else {
            return err(code::AEAD);
        };
        parse_plain(padded).map(|p| p.entry_id).ok_or(SealerError { code: code::BAD_PLAINTEXT })
    }

    fn discard_pending(&mut self, entry_id: &[u8; 16]) -> bool {
        self.pending.remove(entry_id).is_some()
    }

    fn forget_roots_below(&mut self, epoch: u32) -> bool {
        let kept = self.roots.split_off(&epoch);
        let changed = !self.roots.is_empty();
        self.roots = kept;
        changed
    }

    fn take_missed_wraps(
        &mut self,
        m: &FakeMembership,
        first_epoch: u32,
        wraps: &[[u8; 96]],
    ) -> Result<bool, SealerError> {
        self.missed.clear();
        let mut e = first_epoch;
        for x in wraps {
            self.missed.insert(e, *x);
            e = e.saturating_add(1);
        }
        let mut learned = false;
        let open: Vec<(u32, [u8; 96])> = self.missed.iter().map(|(e, x)| (*e, *x)).collect();
        for (e, x) in open {
            if self.roots.contains_key(&e) {
                continue;
            }
            let Some(commit) = m.commitment_of(e) else { continue };
            let root =
                unwrap(&x, &self.dev, &self.hpke_pk, e, commit, &m.board_id).ok_or(SealerError { code: code::COMMITMENT })?;
            self.roots.insert(e, root);
            self.missed.remove(&e);
            learned = true;
        }
        Ok(learned)
    }

    fn seal_snapshot(&mut self, m: &FakeMembership, material: &SnapshotMaterial) -> Result<SealedBlob, SealerError> {
        let large = SealerError { code: code::SNAPSHOT_LARGE };
        let blob_open = SealerError { code: code::BLOB_OPEN };
        // Refused before anything is drawn (`seal.md` §20.4): membership bytes
        // that do not parse, another board's, a history record above `S`, and
        // a history that is not one record per seq of `ctl_seqs`.
        let at_s = FakeMembership::from_bytes(&material.membership).ok_or(SealerError { code: code::MEMBERSHIP_FORMAT })?;
        if at_s.board_id != self.board_id || at_s.ctl_seqs.last().is_some_and(|s| *s > material.seq) {
            return Err(blob_open);
        }
        let history: Vec<Vec<u8>> = {
            let held = self.history.borrow();
            let mut out = Vec::with_capacity(at_s.ctl_seqs.len());
            for s in &at_s.ctl_seqs {
                out.push(held.get(s).cloned().ok_or(blob_open.clone())?);
            }
            out
        };
        let hist_len: u64 = history.iter().map(|r| r.len() as u64).sum();
        let body_len = 4 + 8 + 32 + 4 + 32 + 4 + material.board.len() as u64 + 4 + material.membership.len() as u64 + 4 + hist_len;
        if body_len > BLOB_MAX {
            return Err(large);
        }
        let padded_len = pad_len(4 + body_len);
        if BLOB_HEADER_LEN as u64 + padded_len + TAG_LEN as u64 > BLOB_MAX {
            return Err(large);
        }
        let root = *self.roots.get(&m.epoch).ok_or(SealerError { code: code::MISSING_ROOT })?;
        let body = snapshot_body(material, &history).ok_or(large)?;
        let blob_id = draw::<16>(&mut self.rng);
        let nonce = draw::<24>(&mut self.rng);
        let mut bytes = Vec::with_capacity(BLOB_HEADER_LEN + padded_len as usize + TAG_LEN);
        bytes.extend_from_slice(BLOB_MAGIC);
        bytes.extend_from_slice(&[1, 1, 1]);
        bytes.extend_from_slice(&m.epoch.to_be_bytes());
        bytes.extend_from_slice(&blob_id);
        bytes.extend_from_slice(&nonce);
        let mut padded = Vec::with_capacity(padded_len as usize);
        padded.extend_from_slice(&(body.len() as u32).to_be_bytes());
        padded.extend_from_slice(&body);
        padded.resize(padded_len as usize, 0);
        let ad = blob_ad(&self.board_id, &bytes[..27], material.seq, &self.dev);
        let sealed = aead_seal(&snap_key(&self.board_id, m.epoch, &root), &ad, &padded);
        bytes.extend_from_slice(&sealed);
        let size = bytes.len() as u64;
        Ok(SealedBlob {
            id: blob_id,
            sha256: h(&[&bytes]),
            bytes,
            parts: size.div_ceil(PART) as u32,
            inflated: body_len,
            epoch: m.epoch,
        })
    }

    fn open_snapshot(
        &self,
        parts: &[&[u8]],
        expected: &SnapInfo,
        writer: &Dev,
        exact: bool,
    ) -> Result<(SnapshotMaterial, u32, u64), SealerError> {
        let err = |c| SealerError { code: c };
        // Step 1.
        let size = expected.size;
        let n = size.div_ceil(PART);
        let sizes_ok = (BLOB_HEADER_LEN as u64 + TAG_LEN as u64..=BLOB_MAX).contains(&size)
            && parts.len() as u64 == n
            && u64::from(expected.parts) == n
            && parts.iter().enumerate().all(|(i, p)| {
                let len = p.len() as u64;
                if (i as u64) + 1 < n { len == PART } else { (1..=PART).contains(&len) }
            })
            && parts.iter().map(|p| p.len() as u64).sum::<u64>() == size;
        if !sizes_ok {
            return Err(err(code::BLOB_SIZE));
        }
        // Step 2.
        let bytes: Vec<u8> = parts.concat();
        if !ct_eq(&h(&[&bytes]), &expected.sha256) {
            return Err(err(code::BLOB_HASH));
        }
        // Step 3.
        if bytes.len() < BLOB_HEADER_LEN + TAG_LEN {
            return Err(err(code::BLOB_HEADER));
        }
        let epoch = u32::from_be_bytes([bytes[7], bytes[8], bytes[9], bytes[10]]);
        // The header's `blob_id` is not compared: the signed `sha256` fixes every byte.
        let header_ok = &bytes[..4] == BLOB_MAGIC && bytes[4..7] == [1, 1, 1] && (!exact || epoch == expected.epoch);
        if !header_ok {
            return Err(err(code::BLOB_HEADER));
        }
        // Steps 4 and 5.
        let root = self.roots.get(&epoch).ok_or(err(code::MISSING_ROOT))?;
        let ad = blob_ad(&self.board_id, &bytes[..27], expected.seq, writer);
        let padded = aead_open(&snap_key(&self.board_id, epoch, root), &ad, &bytes[BLOB_HEADER_LEN..])
            .ok_or(err(code::BLOB_OPEN))?;
        let mut r = Reader::new(padded);
        let body_len = u64::from(r.u32().ok_or(err(code::BLOB_OPEN))?);
        let u = 4 + body_len;
        if u > padded.len() as u64 || pad_len(u) != padded.len() as u64 || padded[u as usize..].iter().any(|b| *b != 0) {
            return Err(err(code::BLOB_OPEN));
        }
        let body = &padded[4..u as usize];
        // Step 6: no compression (§6.3); the inflated length is the body's.
        let cap = if exact { expected.inflated.min(BLOB_MAX) } else { BLOB_MAX };
        if body_len > cap || (exact && body_len != expected.inflated) {
            return Err(err(code::INFLATE));
        }
        // Step 7.
        let open = || -> Option<OpenedBody> {
            let mut r = Reader::new(body);
            if &r.take::<4>()? != SNAPSHOT_MAGIC {
                return None;
            }
            let material = SnapshotMaterial {
                seq: r.u64()?,
                chain: r.take()?,
                hash_version: r.u32()?,
                state_hash: r.take()?,
                board: r.vb32()?.to_vec(),
                membership: r.vb32()?.to_vec(),
            };
            let n = usize::try_from(r.u32()?).ok()?;
            if n > MAX_CTL {
                return None;
            }
            let mut projection = Projection::default();
            let mut records = Vec::new();
            for _ in 0..n {
                let rec = read_hist(&mut r)?;
                if rec.seq > material.seq {
                    return None;
                }
                projection.step(&self.board_id, &rec)?;
                records.push((rec.seq, rec.to_bytes()));
            }
            let m = FakeMembership::from_bytes(&material.membership)?;
            // With `exact`, the signed values and the state hash recomputed from
            // the body's own board and membership (engine.md §18.3).
            let recomputed = state_hash_v(material.hash_version, &h(&[&material.board]), &material.membership);
            let signed_ok = material.state_hash == expected.hash
                && material.hash_version == expected.hash_version
                && recomputed == expected.hash;
            let ok = r.is_empty()
                && material.seq == expected.seq
                && (!exact || signed_ok)
                && m.board_id == self.board_id
                && projection.agrees(&m);
            ok.then_some((material, records))
        };
        let (material, records) = open().ok_or(err(code::BLOB_OPEN))?;
        let mut held = self.history.borrow_mut();
        for (seq, bytes) in records {
            held.entry(seq).or_insert(bytes);
        }
        Ok((material, epoch, body_len))
    }

    fn sync_fingerprint(&self, head: &Head) -> String {
        let x = h(&[&ctx("fingerprint"), &self.board_id, &head.seq.to_be_bytes(), &head.chain]);
        format!("{} {}", head.seq, hex(&x[..6]))
    }

    fn move_code(&self, fence_seq: u64, fence_hash: &Hash32) -> String {
        let mc = h(&[&ctx("move"), &self.board_id, &fence_seq.to_be_bytes(), fence_hash]);
        hex(&mc[..5])
    }

    fn check_move_code(&self, fence_seq: u64, fence_hash: &Hash32, typed: &str) -> Result<(), SealerError> {
        let typed: String = typed.chars().filter(|c| *c != ' ' && *c != '-').collect::<String>().to_lowercase();
        let expected = self.move_code(fence_seq, fence_hash);
        if ct_eq(typed.as_bytes(), expected.as_bytes()) { Ok(()) } else { Err(SealerError { code: code::MOVE_CODE }) }
    }

    fn export_identity(&self) -> zeroize::Zeroizing<Vec<u8>> {
        let mut o = Vec::new();
        o.extend_from_slice(IDENTITY_MAGIC);
        o.extend_from_slice(&1u32.to_be_bytes());
        o.extend_from_slice(&self.dev);
        o.extend_from_slice(&self.sk);
        o.extend_from_slice(&self.sign_pk);
        o.extend_from_slice(&self.hpke_pk);
        o.extend_from_slice(&self.board_id);
        match &self.anchor {
            None => o.push(0),
            Some(a) => {
                o.push(1);
                o.extend_from_slice(&a.rsign_pk);
                o.extend_from_slice(&a.salt);
                o.extend_from_slice(&a.root);
            }
        }
        o.extend_from_slice(&(self.roots.len() as u32).to_be_bytes());
        for (e, root) in &self.roots {
            o.extend_from_slice(&e.to_be_bytes());
            o.extend_from_slice(root);
        }
        o.extend_from_slice(&(self.pending.len() as u32).to_be_bytes());
        for (id, (e, root)) in &self.pending {
            o.extend_from_slice(id);
            o.extend_from_slice(&e.to_be_bytes());
            o.extend_from_slice(root);
        }
        zeroize::Zeroizing::new(o)
    }
}

#[cfg(test)]
mod tests;
