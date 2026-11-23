//! Pairing: the code and the two sans-IO machines (seal.md §17, §26).
//!
//! The code (§17.2) is a ticket and a MAC key, never a key to the board.
//! Every slot (§17.4) carries an HMAC over its label, its sender's role, the
//! ticket id, its length and its body; it is verified in constant time
//! before any field of the body is read (SEAL-PAIR-003). E commits to its
//! ephemeral key in slot A, N reveals its keys in slot B, and E opens the
//! commitment in slot C only after slot B verified (SEAL-PAIR-005); the
//! transcript `T` covers slots A to C verbatim (§17.7). N compares the six
//! digits the user copied from E (SEAL-PAIR-007); E seals nothing for N
//! before the user confirmed on E (SEAL-PAIR-008).
//!
//! Every secret of an attempt (`s`, `ticket_key`, `mac_key`, the ephemeral
//! secret keys, `r_E`, `dh`, the SAS) lives in one boxed `Secrets` that
//! erases itself on drop; the bundle key is derived when it is used, in a
//! zeroizing buffer. An abort drops the box, N's long-term keys and the
//! bundle, and leaves the machine `Aborted` for good (SEAL-PAIR-014).
//!
//! What a machine does with an event its state does not expect (lead
//! ruling: the last row of each table, an abort): a slot is
//! `seal/slot_mac` (it is not the slot the state awaits); on N, a
//! `Snapshot`, `Entries` or `Frames` is `seal/bundle_invalid`; on E, a
//! `RegisterFailed{code}` aborts with its own `code`; any other event
//! (`UserConfirmed`, `Registered`, `CodeEntered`, `SasEntered`) is
//! `seal/pair_cancelled`. `Tick` before the limit, and any event in `Done`
//! or `Aborted`, does nothing.
// SPEC-QUESTION: §17.11 names no code for an event a state does not expect;
// the codes above are this crate's reading (lead ruling: abort).

use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize as _, Zeroizing};

use crate::canon::INT_MAX;
use crate::encodings::{device_name_is_valid, host_is_valid};
use crate::membership::{CURRENT_MAX, DEVICE_NO_MAX, DEVICES_MAX, Reader};
use crate::payloads::{BLOB_SIZE_MAX, BLOB_SIZE_MIN, PART_SIZE, VERSION_MAX};
use crate::prim::{h, hkdf8, hkdf32, hmac32, push_zeroizing, take};
use crate::{
    BlobId, BlobRef, BoardId, Chain, Class, CtlItem, DevId, DeviceAdded, DeviceKeys, EpochRoot, FrameHash, Head,
    KeyKind, Membership, OwnIdentity, ReceiptInput, SealError, SnapAnnounce, SnapshotBody, base32, classify, ctx,
    derive_epoch_key, frame_hash, frame_header, open_snapshot, relay_origin, state_hash,
};

/// Slot labels (seal.md §17.4).
pub(crate) const LABEL_A: u8 = 0x41;
pub(crate) const LABEL_B: u8 = 0x42;
pub(crate) const LABEL_C: u8 = 0x43;
pub(crate) const LABEL_D: u8 = 0x44;
/// Senders' roles (seal.md §17.4).
pub(crate) const ROLE_E: u8 = 0x45;
pub(crate) const ROLE_N: u8 = 0x4E;
/// `label ‖ role ‖ u16 body_len` and the 32-byte MAC (seal.md §17.4).
const SLOT_HEAD: usize = 4;
const MAC_LEN: usize = 32;
/// Body widths (seal.md §17.4, §23).
const A_BODY: usize = 48;
const B_BODY_MIN: usize = 119;
const B_BODY_MAX: usize = 242;
const C_BODY: usize = 64;
const D_BODY_MIN: usize = 40;
const D_BODY_MAX: usize = 16_040;
/// The largest body of any slot (slot D, §23).
const SLOT_BODY_MAX: usize = D_BODY_MAX;
/// The bundle JSON (seal.md §17.8, §23).
const BUNDLE_MAX: usize = 16_000;
/// Rules versions in slot B (seal.md §17.4).
const RULES_MAX: usize = 16;
/// The pairing lifetime in milliseconds (seal.md §17.11, §23).
const LIFETIME_MS: u64 = 600_000;
/// SAS entries on N (seal.md §17.11, §23).
const SAS_TRIES: u8 = 3;
/// Characters of a typed code after normalising (seal.md §17.2).
const CODE_CHARS: usize = 18;
/// Characters of the secret in the code (seal.md §17.2).
const SECRET_CHARS: usize = 16;
/// Bits of the secret and of the check (seal.md §17.2, §23).
const SECRET_BITS: usize = 80;
const CHECK_BITS: usize = 10;
/// The longest relay token (seal.md §6.7, §23, SEAL-KEY-015).
const TOKEN_MAX: usize = 512;
/// The XChaCha20-Poly1305 nonce (seal.md §17.8).
const NONCE_LEN: usize = 24;

/// The rules versions this build implements (seal.md §14.3 defines version
/// 1 only, `membership::transitions::envelopes_for`): N's list in slot B and
/// the `supported_rules` of N's classifications (§17.9 step 6).
pub(crate) const SUPPORTED_RULES: &[u32] = &[1];

/// Draws a pairing secret and returns it with its code text (seal.md §17.2, §26).
///
/// Draws exactly the 10 bytes of `s` (SEAL-CODE-001); the text is the
/// display form of §17.2, `XXXX-XXXX-XXXX-XXXX-CC`: the 16 characters of
/// `s` and the 2 of the first 10 bits of `H(ctx("pair-code") ‖ s)`, in
/// groups of 4 joined by hyphens (SEAL-ENC-001: grouping after encoding).
// SPEC-QUESTION: §26 returns the code as a `String` and `ShowCode{code}`
// carries one, so the code text (which encodes `s`) cannot be a zeroizing
// type as SEAL-GEN-006 asks of the pairing secret; `PairSecret` and the
// machines' state erase `s`, the strings handed out are the caller's.
// SPEC-QUESTION: §17.2 gives both "code" (18 characters) and "display"; the
// returned text is the display form, which `parse_code` reads back.
pub fn new_code(rng: &mut dyn super::Entropy) -> Result<(PairSecret, String), super::SealError> {
    let mut p = PairSecret { s: [0u8; 10] };
    rng.fill(&mut p.s)?;
    let code = p.display();
    Ok((p, code))
}

/// Parses a typed pairing code (seal.md §17.2, §26).
///
/// Normalises as §5.2 lists and requires exactly 18 alphabet characters
/// (`seal/code_format`); the first 16 are the 80 bits of `s`, the last 2 the
/// 10 check bits, compared in constant time with the first 10 bits of
/// `H(ctx("pair-code") ‖ s)` (`seal/code_check`). Nothing is derived from
/// `s` before the check passed (SEAL-CODE-002).
pub fn parse_code(text: &str) -> Result<PairSecret, super::SealError> {
    let values = base32::normalise(text, CODE_CHARS).map_err(|_| SealError::CodeFormat)?;
    let sv = values.get(..SECRET_CHARS).ok_or(SealError::CodeFormat)?;
    let cv = values.get(SECRET_CHARS..).ok_or(SealError::CodeFormat)?;
    let s = base32::decode_bits(sv, SECRET_BITS).ok_or(SealError::CodeFormat)?;
    let typed = base32::decode_bits(cv, CHECK_BITS).ok_or(SealError::CodeFormat)?;
    let mut p = PairSecret { s: [0u8; 10] };
    if s.len() != p.s.len() {
        return Err(SealError::CodeFormat);
    }
    p.s.copy_from_slice(&s);
    let want = p.check();
    if !bool::from(typed.as_slice().ct_eq(&want[..])) {
        return Err(SealError::CodeCheck);
    }
    Ok(p)
}

/// The 80-bit pairing secret `s` (seal.md §17.2, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct PairSecret {
    s: [u8; 10],
}

impl PairSecret {
    /// The first 10 bits of `H(ctx("pair-code") ‖ s)`, as 2 bytes whose low
    /// 6 bits are zero.
    fn check(&self) -> Zeroizing<[u8; 2]> {
        let d = Zeroizing::new(h(&[ctx::PAIR_CODE, &self.s]));
        let mut out = Zeroizing::new([0u8; 2]);
        if let (Some(a), Some(b)) = (d.first(), d.get(1)) {
            out[0] = *a;
            out[1] = *b & 0xC0;
        }
        out
    }

    /// `XXXX-XXXX-XXXX-XXXX-CC` (seal.md §17.2).
    fn display(&self) -> String {
        let chars = base32::encode_zeroizing(&self.s);
        let check = self.check();
        let check = base32::encode_bits(&check[..], CHECK_BITS);
        let mut out = String::with_capacity(CODE_CHARS + 4);
        for (i, c) in chars.chars().chain(check.chars()).enumerate() {
            if i > 0 && i % 4 == 0 {
                out.push('-');
            }
            out.push(c);
        }
        out
    }
}

/// The secrets of one attempt (seal.md §17.11): erased on drop, so an abort
/// erases them by dropping the box. `ticket_key` is N's (E needs only
/// `tid`), `r` is E's, `sas` is N's (the six ASCII digits it compares).
#[derive(zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
struct Secrets {
    s: [u8; 10],
    ticket_key: [u8; 32],
    mac_key: [u8; 32],
    esk: [u8; 32],
    r: [u8; 32],
    dh: [u8; 32],
    sas: [u8; 6],
}

impl Secrets {
    /// A box of zeros, filled in place (no copy of a secret is left by a move).
    fn boxed() -> Box<Secrets> {
        Box::new(Secrets {
            s: [0; 10],
            ticket_key: [0; 32],
            mac_key: [0; 32],
            esk: [0; 32],
            r: [0; 32],
            dh: [0; 32],
            sas: [0; 6],
        })
    }

    /// `s` and the keys of §17.3 from it for `board` (SEAL-PAIR-001,
    /// SEAL-PAIR-015); returns `tid`.
    fn derive(&mut self, secret: &PairSecret, board: &BoardId) -> [u8; 16] {
        self.s = secret.s;
        let tk = hkdf32(&board.0, &self.s, &[ctx::PAIR_TICKET]);
        let mk = hkdf32(&board.0, &self.s, &[ctx::PAIR_MAC]);
        self.ticket_key = *tk;
        self.mac_key = *mk;
        tid_of(&tk)
    }

    /// `bundle_key = HKDF(salt = s, ikm = dh, info = ctx("pair-bundle-key") ‖ T, L = 32)` (§17.8).
    fn bundle_key(&self, t: &[u8; 32]) -> Zeroizing<[u8; 32]> {
        hkdf32(&self.s, &self.dh, &[ctx::PAIR_BUNDLE_KEY, t])
    }

    /// The six SAS digits of §17.7 as ASCII.
    fn sas_digits(&self, t: &[u8; 32]) -> Zeroizing<[u8; 6]> {
        let okm = hkdf8(&self.s, &self.dh, &[ctx::SAS, t]);
        let mut v = u64::from_be_bytes(*okm) % 1_000_000;
        let mut out = Zeroizing::new([b'0'; 6]);
        for d in out.iter_mut().rev() {
            // `v % 10` is below 10, so the cast cannot truncate.
            *d = b'0' + (v % 10) as u8;
            v /= 10;
        }
        out
    }
}

/// `tid = H(ctx("pair-tid") ‖ ticket_key)[0..16]` (seal.md §17.3, SEAL-PAIR-015).
fn tid_of(ticket_key: &[u8; 32]) -> [u8; 16] {
    let d = h(&[ctx::PAIR_TID, ticket_key]);
    let mut out = [0u8; 16];
    out.copy_from_slice(d.get(..16).unwrap_or(&[0u8; 16]));
    out
}

/// `epk = X25519(esk, 9)` with the raw RFC 7748 function (seal.md §17.7,
/// SEAL-PAIR-017): no `DeriveKeyPair`.
fn ephemeral_public(esk: &[u8; 32]) -> [u8; 32] {
    x25519_dalek::x25519(*esk, x25519_dalek::X25519_BASEPOINT_BYTES)
}

/// `X25519(esk, epk)`, `seal/dh_zero` when the output is all zero (SEAL-GEN-013).
fn diffie_hellman(esk: &[u8; 32], epk: &[u8; 32], out: &mut [u8; 32]) -> Result<(), SealError> {
    *out = x25519_dalek::x25519(*esk, *epk);
    if bool::from(out.ct_eq(&[0u8; 32])) {
        return Err(SealError::DhZero);
    }
    Ok(())
}

/// `commit = H(ctx("pair-commit") ‖ epk_E ‖ r_E)` (seal.md §17.5).
fn commitment(epk: &[u8; 32], r: &[u8; 32]) -> [u8; 32] {
    h(&[ctx::PAIR_COMMIT, epk, r])
}

/// The MAC of a slot (seal.md §17.4):
/// `HMAC(mac_key, ctx("pair") ‖ label ‖ role ‖ tid ‖ u16 body_len ‖ body)`.
fn slot_mac(mac_key: &[u8; 32], label: u8, role: u8, tid: &[u8; 16], len: u16, body: &[u8]) -> [u8; 32] {
    hmac32(mac_key, &[ctx::PAIR, &[label, role], tid, &len.to_be_bytes(), body])
}

/// A slot's bytes (seal.md §17.4, SEAL-PAIR-002); `None` for a body over
/// the largest slot body.
fn make_slot(label: u8, role: u8, tid: &[u8; 16], mac_key: &[u8; 32], body: &[u8]) -> Option<Vec<u8>> {
    if body.len() > SLOT_BODY_MAX {
        return None;
    }
    let len = u16::try_from(body.len()).ok()?;
    let mac = slot_mac(mac_key, label, role, tid, len, body);
    let mut out = Vec::with_capacity(SLOT_HEAD + body.len() + MAC_LEN);
    out.extend_from_slice(&[label, role]);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(&mac);
    Some(out)
}

/// Verifies a slot of `label` from `role` and returns its body (seal.md
/// §17.4, SEAL-PAIR-003), reading only the fixed header the check needs:
/// `body_len` is checked against the largest slot body of §23 and against
/// the bytes present before the body is read; then the MAC (in constant
/// time), the label and the role. Any failure is `seal/slot_mac`: a slot
/// whose length does not frame a MAC is a slot whose MAC fails. The body's
/// own layout is the caller's next check (`seal/slot_format`).
// SPEC-QUESTION: §17.6 orders "the MAC" before "the layout and limits";
// SEAL-GEN-008 asks that `body_len` be checked against its maximum before
// the body is read. The framing checks (the slot maximum of 16,040 and the
// bytes present) come first, under `seal/slot_mac`; the per-label widths
// come after the MAC, under `seal/slot_format`.
fn open_slot<'a>(bytes: &'a [u8], label: u8, role: u8, tid: &[u8; 16], mac_key: &[u8; 32]) -> Result<&'a [u8], SealError> {
    let bad = SealError::SlotMac;
    let [l, r] = *take::<2>(bytes, 0).ok_or(bad)?;
    let len = u16::from_be_bytes(*take::<2>(bytes, 2).ok_or(bad)?);
    let n = usize::from(len);
    if n > SLOT_BODY_MAX || bytes.len() != SLOT_HEAD + n + MAC_LEN {
        return Err(bad);
    }
    let body = bytes.get(SLOT_HEAD..SLOT_HEAD + n).ok_or(bad)?;
    let mac = take::<MAC_LEN>(bytes, SLOT_HEAD + n).ok_or(bad)?;
    let want = slot_mac(mac_key, label, role, tid, len, body);
    let ok = want.ct_eq(mac) & l.ct_eq(&label) & r.ct_eq(&role);
    if !bool::from(ok) {
        return Err(bad);
    }
    Ok(body)
}

/// Slot B's fields (seal.md §17.4).
struct SlotB {
    dev: DevId,
    sign_pk: [u8; 32],
    hpke_pk: [u8; 32],
    epk: [u8; 32],
    rules: Vec<u32>,
    name: String,
}

/// Slot B's layout and limits (seal.md §17.4, §17.6 check 2): 119 to 242
/// bytes; 1 to 16 rules versions, strictly ascending, each 1 to 2^31 − 1;
/// a name of §5.5; no byte left over.
// SPEC-QUESTION: §17.4 says only "the rules versions N's build supports";
// this takes the range of every rules version elsewhere (1 to 2^31 − 1,
// §11.4, §12.10).
fn parse_b(body: &[u8]) -> Option<SlotB> {
    if !(B_BODY_MIN..=B_BODY_MAX).contains(&body.len()) {
        return None;
    }
    let mut r = Reader::new(body);
    let dev = DevId(r.arr()?);
    let sign_pk = r.arr()?;
    let hpke_pk = r.arr()?;
    let epk = r.arr()?;
    let n = usize::from(r.u8()?);
    if n == 0 {
        return None;
    }
    let n = r.count(n, RULES_MAX, 4)?;
    let mut rules = Vec::with_capacity(n);
    for _ in 0..n {
        let v = r.u32()?;
        if !(1..=VERSION_MAX).contains(&v) || rules.last().is_some_and(|p| *p >= v) {
            return None;
        }
        rules.push(v);
    }
    let name_len = usize::from(r.u8()?);
    let name = r.bytes(name_len)?;
    if !device_name_is_valid(name) || r.remaining() != 0 {
        return None;
    }
    let name = core::str::from_utf8(name).ok()?.to_owned();
    Some(SlotB { dev, sign_pk, hpke_pk, epk, rules, name })
}

/// `T` (seal.md §17.7): `H(ctx("pair-transcript") ‖ vb16(relay origin) ‖
/// board_id ‖ tid ‖ dev_E ‖ slot A ‖ slot B ‖ slot C)`, the slots verbatim.
fn transcript(origin: &str, board: &BoardId, tid: &[u8; 16], dev_e: &DevId, slots: [&[u8]; 3]) -> Option<[u8; 32]> {
    let n = u16::try_from(origin.len()).ok()?;
    let [a, b, c] = slots;
    Some(h(&[ctx::PAIR_TRANSCRIPT, &n.to_be_bytes(), origin.as_bytes(), &board.0, tid, &dev_e.0, a, b, c]))
}

/// `AD = ctx("pair-bundle") ‖ board_id ‖ tid ‖ T` (seal.md §17.8).
fn bundle_ad(board: &BoardId, tid: &[u8; 16], t: &[u8; 32]) -> Vec<u8> {
    let mut ad = Vec::with_capacity(ctx::PAIR_BUNDLE.len() + 16 + 16 + 32);
    ad.extend_from_slice(ctx::PAIR_BUNDLE);
    ad.extend_from_slice(&board.0);
    ad.extend_from_slice(tid);
    ad.extend_from_slice(t);
    ad
}

/// A relay origin of §5.4: `"https://"` and a host (seal.md §5.4).
fn origin_is_valid(origin: &str) -> bool {
    origin.strip_prefix("https://").is_some_and(host_is_valid)
}

// ------------------------------------------------------------ the bundle

/// The constraints of §17.8's table on a bundle (SEAL-PAIR-009), and the
/// bundle's own consistency: `snapshot.seq` is `head.seq` (the bundle's
/// snapshot carries no `seq`; it is the snapshot at `h`).
// SPEC-QUESTION: §17.8's `snapshot` object has no `seq` while §26's
// `Bundle.snapshot` is a `SnapAnnounce`, which has one; the JSON carries no
// `seq`, the parser sets it to `head.seq`, and E refuses a `Registered`
// whose `snapshot.seq` is not `h`. §11.5's `seq` constraints (a multiple of
// 64) do not apply to `h`.
fn bundle_is_valid(b: &Bundle) -> bool {
    let s = &b.snapshot;
    let h = b.head.seq;
    let roots_ok = match b.roots.as_slice() {
        [(a, _)] => s.epoch == 1 && *a == 1,
        [(a, _), (c, _)] => s.epoch >= 2 && *c == s.epoch && Some(*a) == s.epoch.checked_sub(1),
        _ => false,
    };
    host_is_valid(&b.host)
        && h <= INT_MAX
        && b.added_seq > h
        && b.added_seq <= INT_MAX
        && token_is_valid(&b.token)
        && s.seq == h
        && s.epoch >= 1
        && (1..=VERSION_MAX).contains(&s.hash_version)
        && (BLOB_SIZE_MIN..=BLOB_SIZE_MAX).contains(&s.size)
        && u64::from(s.parts) == s.size.div_ceil(PART_SIZE)
        && (1..=BLOB_SIZE_MAX).contains(&s.inflated)
        && roots_ok
}

/// A relay token: opaque ASCII, 1 to 512 bytes (SEAL-KEY-015).
fn token_is_valid(t: &str) -> bool {
    !t.is_empty() && t.len() <= TOKEN_MAX && t.is_ascii()
}

/// Appends bytes to a zeroizing buffer.
fn put(o: &mut Zeroizing<Vec<u8>>, b: &[u8]) {
    push_zeroizing(o, b);
}

fn put_int(o: &mut Zeroizing<Vec<u8>>, mut v: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    loop {
        i -= 1;
        // `v % 10` is below 10, so the cast cannot truncate.
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    put(o, buf.get(i..).unwrap_or_default());
}

fn put_hex(o: &mut Zeroizing<Vec<u8>>, bytes: &[u8]) {
    const D: &[u8; 16] = b"0123456789abcdef";
    put(o, b"\"");
    for b in bytes {
        let pair = [D[usize::from(b >> 4)], D[usize::from(b & 15)]];
        put(o, &pair);
    }
    put(o, b"\"");
}

/// A JSON string with exactly the escapes of `core.md` §4.2 rule 4 (the
/// encoder of `canon.rs`), for the ASCII strings of the bundle.
fn put_str(o: &mut Zeroizing<Vec<u8>>, s: &str) {
    const D: &[u8; 16] = b"0123456789abcdef";
    put(o, b"\"");
    for &c in s.as_bytes() {
        match c {
            b'"' => put(o, b"\\\""),
            b'\\' => put(o, b"\\\\"),
            0x08 => put(o, b"\\b"),
            b'\t' => put(o, b"\\t"),
            b'\n' => put(o, b"\\n"),
            0x0C => put(o, b"\\f"),
            b'\r' => put(o, b"\\r"),
            c if c < 0x20 => put(o, &[b'\\', b'u', b'0', b'0', D[usize::from(c >> 4)], D[usize::from(c & 15)]]),
            c => put(o, &[c]),
        }
    }
    put(o, b"\"");
}

/// The bundle's canonical JSON (seal.md §5.3, §17.8), keys in byte order,
/// written into a zeroizing buffer (it holds the roots, SEAL-GEN-006).
fn encode_bundle(b: &Bundle) -> Zeroizing<Vec<u8>> {
    let mut o = Zeroizing::new(Vec::with_capacity(BUNDLE_MAX + 1024));
    let s = &b.snapshot;
    put(&mut o, b"{\"added_seq\":");
    put_int(&mut o, b.added_seq);
    put(&mut o, b",\"board_id\":");
    put_hex(&mut o, &b.board_id.0);
    put(&mut o, b",\"door\":");
    put_hex(&mut o, &b.door);
    put(&mut o, b",\"head\":{\"chain\":");
    put_hex(&mut o, &b.head.chain.0);
    put(&mut o, b",\"seq\":");
    put_int(&mut o, b.head.seq);
    put(&mut o, b"},\"host\":");
    put_str(&mut o, &b.host);
    put(&mut o, b",\"roots\":[");
    for (i, (e, r)) in b.roots.iter().enumerate() {
        if i > 0 {
            put(&mut o, b",");
        }
        put(&mut o, b"{\"epoch\":");
        put_int(&mut o, u64::from(*e));
        put(&mut o, b",\"root\":");
        put_hex(&mut o, &r.to_bytes()[..]);
        put(&mut o, b"}");
    }
    put(&mut o, b"],\"snapshot\":{\"blob\":");
    put_hex(&mut o, &s.blob.0);
    put(&mut o, b",\"epoch\":");
    put_int(&mut o, u64::from(s.epoch));
    put(&mut o, b",\"hash\":");
    put_hex(&mut o, &s.hash);
    put(&mut o, b",\"hash_version\":");
    put_int(&mut o, u64::from(s.hash_version));
    put(&mut o, b",\"inflated\":");
    put_int(&mut o, s.inflated);
    put(&mut o, b",\"parts\":");
    put_int(&mut o, u64::from(s.parts));
    put(&mut o, b",\"sha256\":");
    put_hex(&mut o, &s.sha256);
    put(&mut o, b",\"size\":");
    put_int(&mut o, s.size);
    put(&mut o, b"},\"token\":");
    put_str(&mut o, &b.token);
    put(&mut o, b"}");
    o
}

/// A cursor over the bundle's bytes that accepts only canonical JSON of the
/// bundle's schema.
struct Cur<'a> {
    b: &'a [u8],
    i: usize,
}

impl Cur<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.i = self.i.checked_add(1)?;
        Some(c)
    }

    fn lit(&mut self, s: &[u8]) -> Option<()> {
        let end = self.i.checked_add(s.len())?;
        (self.b.get(self.i..end)? == s).then_some(())?;
        self.i = end;
        Some(())
    }

    /// A canonical non-negative integer: `0`, or a digit 1 to 9 and at most
    /// 15 more digits, at most 2^53 − 1 (`core.md` §4.2 rule 5).
    fn int(&mut self) -> Option<u64> {
        let first = self.bump()?;
        if first == b'0' {
            return (!matches!(self.peek(), Some(b'0'..=b'9'))).then_some(0);
        }
        if !first.is_ascii_digit() {
            return None;
        }
        let mut v = u64::from(first - b'0');
        let mut digits = 1;
        while let Some(c @ b'0'..=b'9') = self.peek() {
            digits += 1;
            if digits > 16 {
                return None;
            }
            self.i = self.i.checked_add(1)?;
            v = v.checked_mul(10)?.checked_add(u64::from(c - b'0'))?;
        }
        (v <= INT_MAX).then_some(v)
    }

    fn int32(&mut self) -> Option<u32> {
        u32::try_from(self.int()?).ok()
    }

    /// `"` and exactly `2 × out.len()` lowercase hex digits and `"`.
    fn hex(&mut self, out: &mut [u8]) -> Option<()> {
        self.lit(b"\"")?;
        for o in out.iter_mut() {
            let hi = hex_digit(self.bump()?)?;
            let lo = hex_digit(self.bump()?)?;
            *o = (hi << 4) | lo;
        }
        self.lit(b"\"")
    }

    /// A string of ASCII characters with exactly the canonical escapes, at
    /// most `max` characters; anything else is refused.
    fn string(&mut self, max: usize) -> Option<String> {
        self.lit(b"\"")?;
        let mut out = String::with_capacity(max);
        loop {
            let c = match self.bump()? {
                b'"' => return Some(out),
                b'\\' => match self.bump()? {
                    b'"' => b'"',
                    b'\\' => b'\\',
                    b'b' => 0x08,
                    b't' => b'\t',
                    b'n' => b'\n',
                    b'f' => 0x0C,
                    b'r' => b'\r',
                    b'u' => {
                        self.lit(b"00")?;
                        let v = (hex_digit(self.bump()?)? << 4) | hex_digit(self.bump()?)?;
                        // Only the control characters without a short escape.
                        if v >= 0x20 || matches!(v, 0x08 | 0x09 | 0x0A | 0x0C | 0x0D) {
                            return None;
                        }
                        v
                    }
                    _ => return None,
                },
                c if !(0x20..0x80).contains(&c) => return None,
                c => c,
            };
            if out.len() >= max {
                return None;
            }
            out.push(char::from(c));
        }
    }
}

/// A lowercase hex digit.
fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// Parses the bundle JSON strictly (seal.md §5.3, §17.9 step 3): the bytes
/// must be the canonical encoding (`core.md` §4.2) of an object with
/// exactly the fields of §17.8's table, keys in byte order, no whitespace,
/// and every constraint of the table (`bundle_is_valid`). Accepting exactly
/// the canonical encodings of valid bundles is the same as parsing under
/// §5.3, re-encoding and comparing (SEAL-ENC-003), and it keeps the roots
/// out of any non-zeroizing value.
fn parse_bundle(bytes: &[u8]) -> Option<Bundle> {
    if bytes.len() > BUNDLE_MAX {
        return None;
    }
    let mut c = Cur { b: bytes, i: 0 };
    c.lit(b"{\"added_seq\":")?;
    let added_seq = c.int()?;
    c.lit(b",\"board_id\":")?;
    let mut board = [0u8; 16];
    c.hex(&mut board)?;
    c.lit(b",\"door\":")?;
    let mut door = [0u8; 5];
    c.hex(&mut door)?;
    c.lit(b",\"head\":{\"chain\":")?;
    let mut chain = [0u8; 32];
    c.hex(&mut chain)?;
    c.lit(b",\"seq\":")?;
    let seq = c.int()?;
    c.lit(b"},\"host\":")?;
    let host = c.string(crate::encodings::HOST_MAX)?;
    c.lit(b",\"roots\":[")?;
    let mut roots: Vec<(u32, EpochRoot)> = Vec::with_capacity(2);
    if c.peek() == Some(b']') {
        c.lit(b"]")?;
    } else {
        loop {
            if roots.len() >= 2 {
                return None;
            }
            c.lit(b"{\"epoch\":")?;
            let e = c.int32()?;
            c.lit(b",\"root\":")?;
            let mut root = Zeroizing::new([0u8; 32]);
            c.hex(&mut root[..])?;
            c.lit(b"}")?;
            roots.push((e, EpochRoot::from_bytes(*root)));
            match c.bump()? {
                b',' => continue,
                b']' => break,
                _ => return None,
            }
        }
    }
    c.lit(b",\"snapshot\":{\"blob\":")?;
    let mut blob = [0u8; 16];
    c.hex(&mut blob)?;
    c.lit(b",\"epoch\":")?;
    let epoch = c.int32()?;
    c.lit(b",\"hash\":")?;
    let mut hash = [0u8; 32];
    c.hex(&mut hash)?;
    c.lit(b",\"hash_version\":")?;
    let hash_version = c.int32()?;
    c.lit(b",\"inflated\":")?;
    let inflated = c.int()?;
    c.lit(b",\"parts\":")?;
    let parts = c.int32()?;
    c.lit(b",\"sha256\":")?;
    let mut sha256 = [0u8; 32];
    c.hex(&mut sha256)?;
    c.lit(b",\"size\":")?;
    let size = c.int()?;
    c.lit(b"},\"token\":")?;
    let token = c.string(TOKEN_MAX)?;
    c.lit(b"}")?;
    if c.i != bytes.len() {
        return None;
    }
    let b = Bundle {
        board_id: BoardId(board),
        host,
        door,
        roots,
        head: Head { seq, chain: Chain(chain) },
        snapshot: SnapAnnounce { seq, hash, hash_version, blob: BlobId(blob), sha256, size, parts, inflated, epoch },
        added_seq,
        token,
    };
    bundle_is_valid(&b).then_some(b)
}

// ------------------------------------------------------------- machines

/// The states of E's machine (seal.md §17.10).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ExistingState {
    Idle,
    Posted,
    Showing,
    Registering,
    Done,
    Aborted,
}

/// The states of N's machine (seal.md §17.10).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NewState {
    Idle,
    AwaitA,
    AwaitC,
    AwaitSas,
    AwaitD,
    AwaitSnapshot,
    Joining,
    Done,
    Aborted,
}

/// E's machine (seal.md §17.10, §26).
pub struct Existing {
    state: ExistingState,
    start_ms: u64,
    tid: [u8; 16],
    /// Erased on every abort and at `Done` (SEAL-PAIR-014).
    sec: Option<Box<Secrets>>,
    /// E's membership state at `Start`, for the checks of §17.6.
    membership: Option<Membership>,
    own: DevId,
    origin: String,
    /// `h` of `Start`: the `bootstrap_seq` E proposes before the engine
    /// takes the snapshot (see `on`, `UserConfirmed`).
    head_seq: u64,
    epk: [u8; 32],
    slot_a: Vec<u8>,
    n: Option<SlotB>,
    transcript: [u8; 32],
}

/// N's machine (seal.md §17.10, §26).
pub struct New {
    state: NewState,
    start_ms: u64,
    board: BoardId,
    host: String,
    door: [u8; 5],
    name: String,
    origin: String,
    tid: [u8; 16],
    /// Erased on every abort and at `Done` (SEAL-PAIR-014).
    sec: Option<Box<Secrets>>,
    /// The long-term keys of this attempt, erased on abort (§17.10).
    keys: Option<DeviceKeys>,
    dev_e: DevId,
    commit: [u8; 32],
    slot_a: Vec<u8>,
    slot_b: Vec<u8>,
    transcript: [u8; 32],
    sas_tries: u8,
    bundle: Option<Bundle>,
    snapshot: Option<SnapshotBody>,
    join: Option<Join>,
}

/// An event for E's machine (seal.md §17.10, §26).
#[allow(clippy::large_enum_variant)] // `Registered` is the spec's (seal.md §17.10)
pub enum ExistingEvent {
    SlotB {
        bytes: Vec<u8>,
    },
    UserConfirmed,
    Registered {
        h: u64,
        chain: Chain,
        snapshot: SnapAnnounce,
        added_seq: u64,
        token: String,
        roots: Vec<(u32, EpochRoot)>,
        host: String,
        door: [u8; 5],
    },
    RegisterFailed {
        code: super::SealError,
    },
    Cancel,
    Tick {
        now: u64,
    },
}

/// An effect of E's machine (seal.md §17.10, §26).
pub enum ExistingEffect {
    RegisterTicket { tid: [u8; 16], slot_a: Vec<u8> },
    ShowCode { code: String },
    PostSlot { slot: Vec<u8> },
    ShowSas { digits: String },
    NeedSnapshotAndRegister { device: DeviceAdded },
    Done { dev: DevId },
    DeleteTicket { tid: [u8; 16] },
    Abort { code: super::SealError },
}

/// An event for N's machine (seal.md §17.10, §26).
pub enum NewEvent {
    CodeEntered {
        code: String,
        host: String,
        board_id: BoardId,
        door: [u8; 5],
        name: String,
        now: u64,
    },
    Slot {
        bytes: Vec<u8>,
    },
    SasEntered {
        digits: String,
    },
    Snapshot {
        parts: Vec<Vec<u8>>,
    },
    Entries {
        frames: Vec<(u64, Vec<u8>)>,
    },
    Frames {
        frames: Vec<(u64, Vec<u8>)>,
    },
    Cancel,
    Tick {
        now: u64,
    },
}

/// An effect of N's machine (seal.md §17.10, §26).
#[allow(clippy::large_enum_variant)] // `Joined` is the spec's (seal.md §17.10)
pub enum NewEffect {
    FetchSlot {
        tid: [u8; 16],
        ticket_key: zeroize::Zeroizing<[u8; 32]>,
        label: u8,
    },
    AskAgain {
        code: super::SealError,
    },
    PostSlot {
        slot: Vec<u8>,
    },
    AskSas,
    ShowAccepted,
    FetchSnapshot {
        token: String,
        blob: BlobId,
        parts: u32,
    },
    ReadLog {
        token: String,
        from: u64,
    },
    FetchFrames {
        token: String,
        seqs: Vec<u64>,
    },
    Joined {
        device_no: u16,
        keys: DeviceKeys,
        bundle: Bundle,
        snapshot: SnapshotBody,
    },
    Abort {
        code: super::SealError,
    },
}

/// The pairing bundle of slot D (seal.md §17.8).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct Bundle {
    #[zeroize(skip)]
    pub board_id: BoardId,
    pub host: String,
    pub door: [u8; 5],
    /// Each root erases itself on drop.
    #[zeroize(skip)]
    pub roots: Vec<(u32, EpochRoot)>,
    #[zeroize(skip)]
    pub head: Head,
    #[zeroize(skip)]
    pub snapshot: SnapAnnounce,
    pub added_seq: u64,
    pub token: String,
}

impl Existing {
    /// Handles `Start`: draws `s`, `esk_E`, `r_E` and posts slot A (seal.md §17.10).
    ///
    /// The guard: `own` is a current device of `membership`, and
    /// `relay_origin` is `"https://"` and a host of §5.4. A failed guard, or
    /// entropy that fails, returns a machine in `Aborted` with only
    /// `Abort{code}` (no ticket exists yet): `seal/ctl_author` when `own` is
    /// not a current device, `seal/host_format` for the origin,
    /// `seal/entropy`. Whether E is halted or holds a token is the caller's
    /// to check (the crate sees neither).
    // SPEC-QUESTION: §17.10's guard "E is current and unhalted" names no code;
    // a device that is not current could not author the `DeviceAdded`
    // (§13.6), so this takes that item's code, `seal/ctl_author`.
    pub fn start(
        membership: &Membership,
        head: Head,
        relay_origin: &str,
        own: &DevId,
        now: u64,
        rng: &mut dyn super::Entropy,
    ) -> (Self, Vec<ExistingEffect>) {
        let mut e = Existing {
            state: ExistingState::Idle,
            start_ms: now,
            tid: [0; 16],
            sec: None,
            membership: None,
            own: *own,
            origin: String::new(),
            head_seq: head.seq,
            epk: [0; 32],
            slot_a: Vec::new(),
            n: None,
            transcript: [0; 32],
        };
        let fx = match e.begin(membership, relay_origin, rng) {
            Ok(fx) => fx,
            Err(code) => {
                e.state = ExistingState::Aborted;
                vec![ExistingEffect::Abort { code }]
            }
        };
        (e, fx)
    }

    fn begin(
        &mut self,
        membership: &Membership,
        relay_origin: &str,
        rng: &mut dyn super::Entropy,
    ) -> Result<Vec<ExistingEffect>, SealError> {
        if !membership.device(&self.own).is_some_and(|d| d.is_current()) {
            return Err(SealError::CtlAuthor);
        }
        if !origin_is_valid(relay_origin) {
            return Err(SealError::HostFormat);
        }
        let board = membership.board();
        // Draws, in the order of §17.10: `s`, `esk_E`, `r_E` (SEAL-PAIR-016).
        let (secret, code) = new_code(rng)?;
        // The code text is erased if this aborts before it is handed out
        // (§17.11); once handed out (`ShowCode`) it is the caller's.
        let mut code = Zeroizing::new(code);
        let mut sec = Secrets::boxed();
        rng.fill(&mut sec.esk)?;
        rng.fill(&mut sec.r)?;
        let tid = sec.derive(&secret, &board);
        drop(secret);
        let epk = ephemeral_public(&sec.esk);
        let commit = commitment(&epk, &sec.r);
        let mut body = Vec::with_capacity(A_BODY);
        body.extend_from_slice(&self.own.0);
        body.extend_from_slice(&commit);
        let slot_a = make_slot(LABEL_A, ROLE_E, &tid, &sec.mac_key, &body).ok_or(SealError::SlotFormat)?;
        self.tid = tid;
        self.epk = epk;
        self.slot_a = slot_a.clone();
        self.sec = Some(sec);
        self.membership = Some(membership.dup());
        self.origin = relay_origin.to_owned();
        self.state = ExistingState::Posted;
        Ok(vec![ExistingEffect::RegisterTicket { tid, slot_a }, ExistingEffect::ShowCode { code: core::mem::take(&mut *code) }])
    }

    /// Handles one event (seal.md §17.10).
    pub fn on(&mut self, ev: ExistingEvent, rng: &mut dyn super::Entropy) -> Vec<ExistingEffect> {
        use ExistingState as S;
        match (self.state, ev) {
            (S::Idle | S::Done | S::Aborted, _) => Vec::new(),
            (_, ExistingEvent::Cancel) => self.fail(SealError::PairCancelled),
            (_, ExistingEvent::Tick { now }) => {
                if now.saturating_sub(self.start_ms) >= LIFETIME_MS {
                    self.fail(SealError::PairTimeout)
                } else {
                    Vec::new()
                }
            }
            (S::Posted, ExistingEvent::SlotB { bytes }) => match self.slot_b(&bytes) {
                Ok(fx) => fx,
                Err(code) => self.fail(code),
            },
            (S::Showing, ExistingEvent::UserConfirmed) => self.confirmed(),
            (S::Registering, ExistingEvent::Registered { h, chain, snapshot, added_seq, token, roots, host, door }) => {
                let b = Bundle {
                    board_id: self.membership.as_ref().map(Membership::board).unwrap_or(BoardId([0; 16])),
                    host,
                    door,
                    roots,
                    head: Head { seq: h, chain },
                    snapshot,
                    added_seq,
                    token,
                };
                match self.registered(&b, rng) {
                    Ok(fx) => fx,
                    Err(code) => self.fail(code),
                }
            }
            (S::Registering, ExistingEvent::RegisterFailed { code }) => {
                self.erase();
                vec![ExistingEffect::Abort { code }]
            }
            (_, ExistingEvent::RegisterFailed { code }) => self.fail(code),
            (_, ExistingEvent::SlotB { .. }) => self.fail(SealError::SlotMac),
            (_, ExistingEvent::UserConfirmed | ExistingEvent::Registered { .. }) => {
                self.fail(SealError::PairCancelled)
            }
        }
    }

    /// Erases every secret and leaves the machine `Aborted` (SEAL-PAIR-014).
    fn erase(&mut self) {
        self.sec = None;
        self.state = ExistingState::Aborted;
    }

    /// The last row of E's table: `DeleteTicket{tid}`, `Abort{code}`.
    fn fail(&mut self, code: SealError) -> Vec<ExistingEffect> {
        self.erase();
        vec![ExistingEffect::DeleteTicket { tid: self.tid }, ExistingEffect::Abort { code }]
    }

    /// `Posted` + `SlotB`: the checks of §17.6 in order, then slot C, `T`
    /// and the SAS. Slot C is built only here, after slot B verified
    /// (SEAL-PAIR-005).
    fn slot_b(&mut self, bytes: &[u8]) -> Result<Vec<ExistingEffect>, SealError> {
        let sec = self.sec.as_mut().ok_or(SealError::SlotMac)?;
        let m = self.membership.as_ref().ok_or(SealError::SlotMac)?;
        // 1. The MAC, label B, role N.
        let body = open_slot(bytes, LABEL_B, ROLE_N, &self.tid, &sec.mac_key)?;
        // 2. The layout and limits.
        let b = parse_b(body).ok_or(SealError::SlotFormat)?;
        // 3. N's keys and id are new to E's membership.
        let conflict = m.devices.iter().any(|d| d.dev == b.dev || d.sign_pk == b.sign_pk || d.hpke_pk == b.hpke_pk)
            || m.recovery.iter().any(|r| r.dev == b.dev || r.sign_pk == b.sign_pk || r.hpke_pk == b.hpke_pk);
        if conflict {
            return Err(SealError::PairKeysConflict);
        }
        // 4. N supports E's active rules version.
        if !b.rules.contains(&m.rules) {
            return Err(SealError::PairRules);
        }
        // 5. Room for one more device (§23).
        // SPEC-QUESTION: §17.6 lists 16 current devices and `next_device_no ≤
        // 8191`; §13.6 also caps device records at 256 (`seal/ctl_device_cap`,
        // "device limits of §23"), which is checked here too, since the
        // `DeviceAdded` would be refused for it.
        if m.current_count() >= CURRENT_MAX || m.next_device_no > DEVICE_NO_MAX || m.devices.len() >= DEVICES_MAX {
            return Err(SealError::CtlDeviceCap);
        }
        diffie_hellman(&sec.esk, &b.epk, &mut sec.dh)?;
        let mut body_c = [0u8; C_BODY];
        body_c[..32].copy_from_slice(&self.epk);
        body_c[32..].copy_from_slice(&sec.r);
        let slot_c = make_slot(LABEL_C, ROLE_E, &self.tid, &sec.mac_key, &body_c).ok_or(SealError::SlotFormat)?;
        let t = transcript(&self.origin, &m.board(), &self.tid, &self.own, [&self.slot_a, bytes, &slot_c])
            .ok_or(SealError::HostFormat)?;
        let digits = sec.sas_digits(&t);
        let shown: String = digits.iter().map(|d| char::from(*d)).collect();
        self.transcript = t;
        self.n = Some(b);
        self.state = ExistingState::Showing;
        Ok(vec![ExistingEffect::PostSlot { slot: slot_c }, ExistingEffect::ShowSas { digits: shown }])
    }

    /// `Showing` + `UserConfirmed`: the `DeviceAdded` item for the engine
    /// (SEAL-PAIR-010), naming exactly slot B's keys and name and `T`.
    // SPEC-QUESTION: `bootstrap_seq` must be `h`, the head at which the
    // engine takes the snapshot (§17.1 step 9, SEAL-PAIR-010), which is not
    // known when this effect is emitted; the item carries the `h` of
    // `Start`, and the engine sets it to the snapshot's `h` before sealing
    // (a duty of the caller). N checks `bootstrap_seq = h` (§17.9 step 7).
    fn confirmed(&mut self) -> Vec<ExistingEffect> {
        let Some(b) = self.n.as_ref() else { return self.fail(SealError::PairCancelled) };
        let device = DeviceAdded {
            dev: b.dev,
            sign_pk: b.sign_pk,
            hpke_pk: b.hpke_pk,
            name: b.name.clone(),
            transcript: self.transcript,
            bootstrap_seq: self.head_seq,
        };
        self.state = ExistingState::Registering;
        vec![ExistingEffect::NeedSnapshotAndRegister { device }]
    }

    /// `Registering` + `Registered`: seals the bundle (§17.8) and posts it
    /// as slot D. A bundle that breaks §17.8's table is `seal/bundle_invalid`
    /// (nothing is posted); the nonce is the only draw (SEAL-GEN-012).
    // SPEC-QUESTION: §17.10 gives `Registered` no guard; values the bundle's
    // table excludes (an `added_seq` not above `h`, a token or host out of
    // range, roots other than the current and previous epoch, snapshot
    // fields outside §11.5) would make N abort, so E aborts first, with the
    // code N would give, `seal/bundle_invalid`.
    fn registered(&mut self, b: &Bundle, rng: &mut dyn super::Entropy) -> Result<Vec<ExistingEffect>, SealError> {
        let sec = self.sec.as_ref().ok_or(SealError::BundleInvalid)?;
        let dev_n = self.n.as_ref().map(|n| n.dev).ok_or(SealError::BundleInvalid)?;
        if !bundle_is_valid(b) {
            return Err(SealError::BundleInvalid);
        }
        let json = encode_bundle(b);
        if json.len() > BUNDLE_MAX {
            return Err(SealError::BundleInvalid);
        }
        let mut nonce = [0u8; NONCE_LEN];
        rng.fill(&mut nonce)?;
        let key = sec.bundle_key(&self.transcript);
        let ad = bundle_ad(&b.board_id, &self.tid, &self.transcript);
        let ct = XChaCha20Poly1305::new_from_slice(&key[..])
            .map_err(|_| SealError::BundleInvalid)?
            .encrypt(&XNonce::from(nonce), Payload { msg: &json, aad: &ad })
            .map_err(|_| SealError::BundleInvalid)?;
        let mut body = Vec::with_capacity(NONCE_LEN + ct.len());
        body.extend_from_slice(&nonce);
        body.extend_from_slice(&ct);
        let slot_d = make_slot(LABEL_D, ROLE_E, &self.tid, &sec.mac_key, &body).ok_or(SealError::BundleInvalid)?;
        self.sec = None;
        self.state = ExistingState::Done;
        Ok(vec![ExistingEffect::PostSlot { slot: slot_d }, ExistingEffect::Done { dev: dev_n }])
    }
}

/// Erases the typed code or digits an event carries before it is dropped
/// (§17.11): `NewEvent` hands them to the crate by value.
fn erase_typed(ev: NewEvent) {
    match ev {
        NewEvent::CodeEntered { mut code, .. } => code.zeroize(),
        NewEvent::SasEntered { mut digits } => digits.zeroize(),
        _ => {}
    }
}

/// N's progress through §17.9 step 6.
struct Join {
    /// `M_{t−1}`, from the snapshot's `M_h`.
    m: Membership,
    /// `C_{t−1}`, from `C_h = head.chain`.
    chain: Chain,
    /// `C_{h+1}` to `C_{t−1}`.
    chains: Vec<Chain>,
    /// The entries from `h + 1`, as `Entries` gave them; `None` before.
    entries: Option<Vec<(u64, Vec<u8>)>>,
    /// The next entry to classify.
    next: usize,
    /// Roots learned from own wraps after `added_seq` (checked against
    /// their commitments by `classify`).
    learned: Vec<(u32, EpochRoot)>,
    /// The Skip targets supplied for the entry at `next`.
    supplied: Vec<(u64, Vec<u8>)>,
    /// Whether the entry at `next` was already classified with its targets.
    resolved: bool,
    /// The frames asked of the relay, with the hashes their Skip names.
    asked: Option<Vec<(u64, FrameHash)>>,
    /// Step 7's view of the entry at `added_seq`: accepted as required,
    /// refused, or anything else; and step 8's `device_no`.
    added: Option<Added>,
}

enum Added {
    Ok(u16),
    Refused,
    Mismatch,
}

// The spec's API (seal.md §26) names the type `New` and its constructor `New::new()`, with no `Default`.
#[allow(clippy::new_without_default, clippy::self_named_constructors)]
impl New {
    /// N's machine in `Idle`; it draws nothing until `Slot{A}` (seal.md §17.10).
    pub fn new() -> Self {
        New {
            state: NewState::Idle,
            start_ms: 0,
            board: BoardId([0; 16]),
            host: String::new(),
            door: [0; 5],
            name: String::new(),
            origin: String::new(),
            tid: [0; 16],
            sec: None,
            keys: None,
            dev_e: DevId([0; 16]),
            commit: [0; 32],
            slot_a: Vec::new(),
            slot_b: Vec::new(),
            transcript: [0; 32],
            sas_tries: 0,
            bundle: None,
            snapshot: None,
            join: None,
        }
    }

    /// Handles one event (seal.md §17.10).
    ///
    /// The typed code and digits come by value: from here the crate holds
    /// the only copy, and erases it on every path (§17.11).
    pub fn on(&mut self, ev: NewEvent, rng: &mut dyn super::Entropy) -> Vec<NewEffect> {
        use NewState as S;
        let r = match (self.state, ev) {
            (S::Done | S::Aborted, ev) => {
                erase_typed(ev);
                return Vec::new();
            }
            (_, NewEvent::Cancel) => Err(SealError::PairCancelled),
            (S::Idle, NewEvent::Tick { .. }) => Ok(Vec::new()),
            (_, NewEvent::Tick { now }) => {
                if now.saturating_sub(self.start_ms) >= LIFETIME_MS {
                    Err(SealError::PairTimeout)
                } else {
                    Ok(Vec::new())
                }
            }
            (S::Idle, NewEvent::CodeEntered { code, host, board_id, door, name, now }) => {
                let code = Zeroizing::new(code);
                Ok(self.code_entered(&code, host, board_id, door, name, now))
            }
            (S::AwaitA, NewEvent::Slot { bytes }) => self.slot_a(bytes, rng),
            (S::AwaitC, NewEvent::Slot { bytes }) => self.slot_c(&bytes),
            (S::AwaitSas, NewEvent::SasEntered { digits }) => {
                let digits = Zeroizing::new(digits);
                self.sas_entered(&digits)
            }
            (S::AwaitD, NewEvent::Slot { bytes }) => self.slot_d(&bytes),
            (S::AwaitSnapshot, NewEvent::Snapshot { parts }) => self.snapshot(&parts),
            (S::Joining, NewEvent::Entries { frames }) => self.entries(frames),
            (S::Joining, NewEvent::Frames { frames }) => self.frames(frames),
            (_, NewEvent::Slot { .. }) => Err(SealError::SlotMac),
            (_, NewEvent::Snapshot { .. } | NewEvent::Entries { .. } | NewEvent::Frames { .. }) => {
                Err(SealError::BundleInvalid)
            }
            (_, ev @ (NewEvent::CodeEntered { .. } | NewEvent::SasEntered { .. })) => {
                erase_typed(ev);
                Err(SealError::PairCancelled)
            }
        };
        match r {
            Ok(fx) => fx,
            Err(code) => self.fail(code),
        }
    }

    /// Erases `s`, every derived key, the long-term keys of this attempt,
    /// the bundle and the join state, and leaves the machine `Aborted`
    /// (SEAL-PAIR-014): `Abort{code}`.
    fn fail(&mut self, code: SealError) -> Vec<NewEffect> {
        self.sec = None;
        self.keys = None;
        self.bundle = None;
        self.snapshot = None;
        self.join = None;
        self.state = NewState::Aborted;
        vec![NewEffect::Abort { code }]
    }

    fn ticket_key(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.sec.as_ref().map(|s| s.ticket_key).unwrap_or([0; 32]))
    }

    /// `Idle` + `CodeEntered`: the code's check first (SEAL-CODE-002), then
    /// the relay address and the name; on any failure `AskAgain{code}` and
    /// nothing is derived or fetched.
    // SPEC-QUESTION: §17.10's guard names the code's check only; a host
    // outside §5.4 is asked again with `seal/host_format` (§22: "ask the user
    // again"), and a name outside §5.5, which slot B could not carry, with
    // `seal/slot_format` (no code of §22 names a device name).
    fn code_entered(
        &mut self,
        code: &str,
        host: String,
        board: BoardId,
        door: [u8; 5],
        name: String,
        now: u64,
    ) -> Vec<NewEffect> {
        let secret = match parse_code(code) {
            Ok(s) => s,
            Err(code) => return vec![NewEffect::AskAgain { code }],
        };
        let origin = match relay_origin(&host) {
            Ok(o) => o,
            Err(code) => return vec![NewEffect::AskAgain { code }],
        };
        if !device_name_is_valid(name.as_bytes()) {
            return vec![NewEffect::AskAgain { code: SealError::SlotFormat }];
        }
        let mut sec = Secrets::boxed();
        let tid = sec.derive(&secret, &board);
        drop(secret);
        self.sec = Some(sec);
        self.tid = tid;
        self.board = board;
        self.host = host;
        self.door = door;
        self.name = name;
        self.origin = origin;
        self.start_ms = now;
        self.state = NewState::AwaitA;
        vec![NewEffect::FetchSlot { tid, ticket_key: self.ticket_key(), label: LABEL_A }]
    }

    /// `AwaitA` + `Slot{A}`: the MAC, then the layout; then the draws of
    /// §17.10 in order (SEAL-PAIR-016) and slot B.
    fn slot_a(&mut self, bytes: Vec<u8>, rng: &mut dyn super::Entropy) -> Result<Vec<NewEffect>, SealError> {
        let sec = self.sec.as_mut().ok_or(SealError::SlotMac)?;
        let body = open_slot(&bytes, LABEL_A, ROLE_E, &self.tid, &sec.mac_key)?;
        if body.len() != A_BODY {
            return Err(SealError::SlotFormat);
        }
        let dev_e = DevId(*take::<16>(body, 0).ok_or(SealError::SlotFormat)?);
        let commit = *take::<32>(body, 16).ok_or(SealError::SlotFormat)?;
        // `dev_N`, the signing key, the X25519 `ikm` (§6.4), then `esk_N`.
        let keys = DeviceKeys::generate(rng)?;
        rng.fill(&mut sec.esk)?;
        let epk = ephemeral_public(&sec.esk);
        let p = keys.public();
        let name = self.name.as_bytes();
        let name_len = u8::try_from(name.len()).map_err(|_| SealError::SlotFormat)?;
        let mut body_b = Vec::with_capacity(B_BODY_MAX);
        body_b.extend_from_slice(&p.dev.0);
        body_b.extend_from_slice(&p.sign_pk);
        body_b.extend_from_slice(&p.hpke_pk);
        body_b.extend_from_slice(&epk);
        body_b.push(u8::try_from(SUPPORTED_RULES.len()).map_err(|_| SealError::SlotFormat)?);
        for v in SUPPORTED_RULES {
            body_b.extend_from_slice(&v.to_be_bytes());
        }
        body_b.push(name_len);
        body_b.extend_from_slice(name);
        let slot_b = make_slot(LABEL_B, ROLE_N, &self.tid, &sec.mac_key, &body_b).ok_or(SealError::SlotFormat)?;
        self.keys = Some(keys);
        self.dev_e = dev_e;
        self.commit = commit;
        self.slot_a = bytes;
        self.slot_b = slot_b.clone();
        self.state = NewState::AwaitC;
        Ok(vec![
            NewEffect::PostSlot { slot: slot_b },
            NewEffect::FetchSlot { tid: self.tid, ticket_key: self.ticket_key(), label: LABEL_C },
        ])
    }

    /// `AwaitC` + `Slot{C}`: the MAC, the layout, the commitment
    /// (SEAL-PAIR-004), `dh`, `T` and the SAS.
    fn slot_c(&mut self, bytes: &[u8]) -> Result<Vec<NewEffect>, SealError> {
        let sec = self.sec.as_mut().ok_or(SealError::SlotMac)?;
        let body = open_slot(bytes, LABEL_C, ROLE_E, &self.tid, &sec.mac_key)?;
        if body.len() != C_BODY {
            return Err(SealError::SlotFormat);
        }
        let epk_e = take::<32>(body, 0).ok_or(SealError::SlotFormat)?;
        let r_e = take::<32>(body, 32).ok_or(SealError::SlotFormat)?;
        if !bool::from(commitment(epk_e, r_e).ct_eq(&self.commit)) {
            return Err(SealError::CommitMismatch);
        }
        diffie_hellman(&sec.esk, epk_e, &mut sec.dh)?;
        let t = transcript(&self.origin, &self.board, &self.tid, &self.dev_e, [&self.slot_a, &self.slot_b, bytes])
            .ok_or(SealError::HostFormat)?;
        sec.sas = *sec.sas_digits(&t);
        self.transcript = t;
        self.state = NewState::AwaitSas;
        Ok(vec![NewEffect::AskSas])
    }

    /// `AwaitSas` + `SasEntered`: a constant-time comparison; the third
    /// mismatch aborts (SEAL-PAIR-007).
    // SPEC-QUESTION: §17.7 does not say whether typed digits are normalised;
    // they are compared as typed (six ASCII digits; a space or another
    // length is a mismatch).
    fn sas_entered(&mut self, digits: &str) -> Result<Vec<NewEffect>, SealError> {
        let sec = self.sec.as_ref().ok_or(SealError::SasMismatch)?;
        if bool::from(digits.as_bytes().ct_eq(&sec.sas[..])) {
            self.state = NewState::AwaitD;
            return Ok(vec![
                NewEffect::ShowAccepted,
                NewEffect::FetchSlot { tid: self.tid, ticket_key: self.ticket_key(), label: LABEL_D },
            ]);
        }
        self.sas_tries = self.sas_tries.saturating_add(1);
        if self.sas_tries >= SAS_TRIES {
            return Err(SealError::SasMismatch);
        }
        Ok(vec![NewEffect::AskSas])
    }

    /// `AwaitD` + `Slot{D}`: §17.9 steps 1 to 3.
    // SPEC-QUESTION: step 1 names `seal/slot_mac` only; slot D's width (40 to
    // 16,040 bytes) is checked after the MAC with `seal/slot_format`, as
    // §17.11 states for every slot.
    fn slot_d(&mut self, bytes: &[u8]) -> Result<Vec<NewEffect>, SealError> {
        let sec = self.sec.as_ref().ok_or(SealError::SlotMac)?;
        // 1.
        let body = open_slot(bytes, LABEL_D, ROLE_E, &self.tid, &sec.mac_key)?;
        if !(D_BODY_MIN..=D_BODY_MAX).contains(&body.len()) {
            return Err(SealError::SlotFormat);
        }
        // 2.
        let nonce = *take::<NONCE_LEN>(body, 0).ok_or(SealError::SlotFormat)?;
        let ct = body.get(NONCE_LEN..).ok_or(SealError::SlotFormat)?;
        let key = sec.bundle_key(&self.transcript);
        let ad = bundle_ad(&self.board, &self.tid, &self.transcript);
        let pt = Zeroizing::new(
            XChaCha20Poly1305::new_from_slice(&key[..])
                .map_err(|_| SealError::BundleOpen)?
                .decrypt(&XNonce::from(nonce), Payload { msg: ct, aad: &ad })
                .map_err(|_| SealError::BundleOpen)?,
        );
        // 3.
        let b = parse_bundle(&pt).ok_or(SealError::BundleInvalid)?;
        if b.board_id != self.board || b.host != self.host || b.door != self.door {
            return Err(SealError::BundleInvalid);
        }
        let fx = NewEffect::FetchSnapshot { token: b.token.clone(), blob: b.snapshot.blob, parts: b.snapshot.parts };
        self.bundle = Some(b);
        self.state = NewState::AwaitSnapshot;
        Ok(vec![fx])
    }

    /// `AwaitSnapshot` + `Snapshot`: §17.9 steps 4 and 5.
    // SPEC-QUESTION: SEAL-KEY-013 asks that a root learned from a bundle be
    // checked against the commitment of the device's membership before a
    // key is derived from it; N holds no membership until the snapshot has
    // opened under `K_snap` from the bundle's root (step 4). This follows the
    // order of §17.9: `K_snap` opens the snapshot (an AEAD failure for a
    // wrong root), then step 5 checks every root against `M_h`'s
    // commitments before any other key is derived from one.
    fn snapshot(&mut self, parts: &[Vec<u8>]) -> Result<Vec<NewEffect>, SealError> {
        let bad = SealError::BundleInvalid;
        let b = self.bundle.as_ref().ok_or(bad)?;
        let s = &b.snapshot;
        let h_seq = b.head.seq;
        // 4.
        let root = b.roots.iter().find(|(e, _)| *e == s.epoch).map(|(_, r)| r).ok_or(bad)?;
        let key = derive_epoch_key(root, &self.board, s.epoch, KeyKind::Snap);
        let expected =
            BlobRef { id: s.blob, sha256: s.sha256, size: s.size, parts: s.parts, inflated: s.inflated, epoch: s.epoch };
        let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        let body = open_snapshot(&refs, &expected, h_seq, &s.hash, s.hash_version, &self.dev_e, &key, &self.board)
            .map_err(|_| bad)?;
        // 5.
        if body.chain != b.head.chain {
            return Err(bad);
        }
        let m = Membership::from_canonical_bytes(&body.membership).map_err(|_| bad)?;
        let top = b.roots.last().map(|(e, _)| *e).ok_or(bad)?;
        if m.board() != self.board || !m.device(&self.dev_e).is_some_and(|d| d.is_current()) || m.epoch() != top {
            return Err(bad);
        }
        for (e, r) in &b.roots {
            let recorded = m.commitment(*e).ok_or(bad)?;
            if !bool::from(r.commitment(&self.board, *e).ct_eq(recorded)) {
                return Err(bad);
            }
        }
        // The state hash recomputed from the body: `open_snapshot` compared
        // the body's `state_hash` and `hash_version` with the bundle's and
        // no longer recomputes it (ruling Q57), so the crate does it here.
        // SPEC-QUESTION: step 5 recomputes the state hash "(`engine.md`)",
        // which defines it for the versions the membership's rules allow
        // (§14.3); `open_snapshot` used to refuse any other version (ruling
        // Q50) and no longer does. A version the rules do not allow is
        // refused here, as before (the refusing reading).
        if !crate::membership::transitions::hash_version_allowed(m.rules, body.hash_version) {
            return Err(bad);
        }
        let recomputed = state_hash(body.hash_version, &h(&[&body.board]), &body.membership);
        let hash_ok = recomputed.ct_eq(&s.hash) & body.state_hash.ct_eq(&s.hash);
        if body.hash_version != s.hash_version || !bool::from(hash_ok) {
            return Err(bad);
        }
        let from = h_seq.checked_add(1).ok_or(bad)?;
        let fx = NewEffect::ReadLog { token: b.token.clone(), from };
        self.join = Some(Join {
            m,
            chain: b.head.chain,
            chains: Vec::new(),
            entries: None,
            next: 0,
            learned: Vec::new(),
            supplied: Vec::new(),
            resolved: false,
            asked: None,
            added: None,
        });
        self.snapshot = Some(body);
        self.state = NewState::Joining;
        Ok(vec![fx])
    }

    /// `Joining` + `Entries`: §17.9 step 6 from `h + 1`.
    // SPEC-QUESTION: §17.10 has one `Entries` event carry the log from
    // `h + 1` to the relay's head; this takes it as the whole log (paging is
    // the caller's), refuses a second `Entries` with `seal/bundle_invalid`,
    // and requires the seqs to be `h + 1`, `h + 2`, … with no gap.
    fn entries(&mut self, frames: Vec<(u64, Vec<u8>)>) -> Result<Vec<NewEffect>, SealError> {
        let j = self.join.as_mut().ok_or(SealError::BundleInvalid)?;
        if j.entries.is_some() {
            return Err(SealError::BundleInvalid);
        }
        j.entries = Some(frames);
        self.run()
    }

    /// `Joining` + `Frames`: each frame has the seq asked for and the
    /// `frame_hash` its Skip item names; anything else aborts.
    fn frames(&mut self, frames: Vec<(u64, Vec<u8>)>) -> Result<Vec<NewEffect>, SealError> {
        let bad = SealError::BundleInvalid;
        let j = self.join.as_mut().ok_or(bad)?;
        let asked = j.asked.take().ok_or(bad)?;
        if frames.len() != asked.len() {
            return Err(bad);
        }
        for (t, fh) in &asked {
            let mut it = frames.iter().filter(|(s, _)| s == t);
            let (Some((_, f)), None) = (it.next(), it.next()) else { return Err(bad) };
            if !bool::from(frame_hash(f).0.ct_eq(&fh.0)) {
                return Err(bad);
            }
        }
        j.supplied.extend(frames);
        j.resolved = true;
        self.run()
    }

    /// Continues step 6, then steps 7 and 8.
    fn run(&mut self) -> Result<Vec<NewEffect>, SealError> {
        let mut j = self.join.take().ok_or(SealError::BundleInvalid)?;
        let r = self.advance(&mut j);
        let token = self.bundle.as_ref().map(|b| b.token.clone()).unwrap_or_default();
        match r? {
            Some(seqs) => {
                self.join = Some(j);
                Ok(vec![NewEffect::FetchFrames { token, seqs }])
            }
            None => self.finish(&j),
        }
    }

    /// Classifies the entries from `next` in seq order (§17.9 step 6);
    /// `Some(seqs)` when frames at or below `h` must be fetched first.
    fn advance(&self, j: &mut Join) -> Result<Option<Vec<u64>>, SealError> {
        let bad = SealError::BundleInvalid;
        let b = self.bundle.as_ref().ok_or(bad)?;
        let keys = self.keys.as_ref().ok_or(bad)?;
        let me = keys.public();
        let h_seq = b.head.seq;
        let entries = j.entries.take().ok_or(bad)?;
        let out = (|| {
            while let Some((t, frame)) = entries.get(j.next) {
                let t = *t;
                let want = u64::try_from(j.next).ok().and_then(|i| h_seq.checked_add(i)).and_then(|s| s.checked_add(1));
                if want != Some(t) {
                    return Err(bad);
                }
                let roots = |e: u32| {
                    b.roots.iter().chain(j.learned.iter()).find(|(x, _)| *x == e).map(|(_, r)| r)
                };
                let chain_at = |s: u64| {
                    if s == h_seq {
                        return Some(b.head.chain);
                    }
                    let i = usize::try_from(s.checked_sub(h_seq)?.checked_sub(1)?).ok()?;
                    j.chains.get(i).copied()
                };
                let skip = |s: u64| j.supplied.iter().find(|(x, _)| *x == s).map(|(_, f)| f.as_slice());
                let sealed = |_: u64| None;
                let input = ReceiptInput {
                    seq: t,
                    frame,
                    prev_chain: j.chain,
                    membership: &j.m,
                    roots: &roots,
                    chain_at: &chain_at,
                    retained_from: h_seq,
                    own: Some(OwnIdentity { keys, sealed: &sealed }),
                    supported_rules: SUPPORTED_RULES,
                    genesis: None,
                    skip_targets: &skip,
                };
                // `seal/missing_root`, the only error: a rotation after `h`
                // of which N is not a recipient.
                let r = classify(&input).map_err(|e| match e {
                    SealError::MissingRoot => SealError::PairRaced,
                    _ => bad,
                })?;
                if let Class::NeedFrames(list) = &r.class {
                    if j.resolved {
                        return Err(bad);
                    }
                    let mut ask = Vec::new();
                    for (s, fh) in list {
                        if *s > h_seq {
                            // An entry already read (a Skip names only seqs
                            // below its own).
                            let f = entries.iter().take(j.next).find(|(x, _)| x == s).ok_or(bad)?;
                            j.supplied.push((*s, f.1.clone()));
                        } else {
                            ask.push((*s, *fh));
                        }
                    }
                    j.resolved = true;
                    if ask.is_empty() {
                        continue;
                    }
                    let seqs = ask.iter().map(|(s, _)| *s).collect();
                    j.asked = Some(ask);
                    return Ok(Some(seqs));
                }
                // SPEC-QUESTION: §17.9 step 6 names `Tamper` only. An entry that
                // passes the signature check under N's own new key (the
                // own-dev flag, §14.7: N has sealed nothing) and an `Accept`
                // with a halt (§14.5) are not named; this aborts on both: the
                // flag and the commitment halt with `seal/bundle_invalid`, the
                // rules halt (a rules version this build lacks) with
                // `seal/pair_rules` (§22: "the user updates N").
                if matches!(r.class, Class::Tamper) || r.own_dev_foreign {
                    return Err(bad);
                }
                match r.halt {
                    Some(crate::Halt::UnknownRules(_)) => return Err(SealError::PairRules),
                    Some(crate::Halt::Commitment) => return Err(bad),
                    None => {}
                }
                if t == b.added_seq {
                    j.added = Some(Self::check_added(&r, frame, b, &self.dev_e, &me, &self.name, &self.transcript));
                }
                if let Some(l) = r.learned_root {
                    j.learned.push(l);
                }
                j.chains.push(r.chain);
                j.chain = r.chain;
                j.m = r.membership;
                j.next = j.next.checked_add(1).ok_or(bad)?;
                j.supplied.clear();
                j.resolved = false;
            }
            Ok(None)
        })();
        j.entries = Some(entries);
        out
    }

    /// §17.9 step 7 for the entry at `added_seq`, and step 8's `device_no`.
    // SPEC-QUESTION: step 7 lists N's `dev`, `sign_pk`, `hpke_pk`, name and
    // `T`; SEAL-PAIR-010 also fixes `bootstrap_seq = h`, which is checked
    // here too (the refusing reading).
    fn check_added(
        r: &crate::Receipt,
        frame: &[u8],
        b: &Bundle,
        dev_e: &DevId,
        me: &crate::DevicePublic,
        name: &str,
        t: &[u8; 32],
    ) -> Added {
        match r.class {
            Class::Accept => {}
            Class::Refuse => return Added::Refused,
            _ => return Added::Mismatch,
        }
        let author = frame_header(frame).map(|hd| hd.dev).ok();
        let Some(o) = r.opened.as_ref() else { return Added::Mismatch };
        let item_ok = match o.items.as_deref() {
            Some([CtlItem::DeviceAdded(a)]) => {
                a.dev == me.dev
                    && a.sign_pk == me.sign_pk
                    && a.hpke_pk == me.hpke_pk
                    && a.name == name
                    && bool::from(a.transcript.ct_eq(t))
                    && a.bootstrap_seq == b.head.seq
            }
            _ => false,
        };
        if author != Some(*dev_e) || o.based_on.seq != b.head.seq || !item_ok {
            return Added::Mismatch;
        }
        match r.membership.device_no(&me.dev) {
            Some(n) => Added::Ok(n),
            None => Added::Mismatch,
        }
    }

    /// Steps 7 and 8 after step 6 passed for every entry: `Joined`, the only
    /// effect that hands anything out (SEAL-PAIR-011, SEAL-PAIR-012).
    // SPEC-QUESTION: a log that ends before `added_seq` holds no entry there;
    // step 7's "otherwise" gives `seal/device_added_mismatch`.
    fn finish(&mut self, j: &Join) -> Result<Vec<NewEffect>, SealError> {
        let device_no = match j.added {
            Some(Added::Ok(n)) => n,
            Some(Added::Refused) => return Err(SealError::PairRaced),
            Some(Added::Mismatch) | None => return Err(SealError::DeviceAddedMismatch),
        };
        let (Some(keys), Some(bundle), Some(snapshot)) = (self.keys.take(), self.bundle.take(), self.snapshot.take())
        else {
            return Err(SealError::BundleInvalid);
        };
        self.sec = None;
        self.state = NewState::Done;
        Ok(vec![NewEffect::Joined { device_no, keys, bundle, snapshot }])
    }
}

#[cfg(test)]
mod tests;
