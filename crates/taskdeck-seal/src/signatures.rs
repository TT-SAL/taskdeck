//! Login, admin and backup signatures (seal.md §16, §26).

use ed25519_dalek::Signer as _;

use crate::encodings::host_is_valid;
use crate::prim::h;
use crate::{BlobId, BoardId, Chain, DevId, DeviceKeys, Entropy, FrameHash, SealError, Signer, ctx};

/// The relay's login and admin nonce (seal.md §16.1, §16.2, §23).
pub(crate) const RELAY_NONCE_LEN: usize = 32;
/// The longest relay origin (seal.md §5.4, §23).
pub(crate) const ORIGIN_MAX: usize = 263;
/// Wraps per reinstatement (seal.md §16.2, §23).
pub(crate) const REINSTATE_WRAPS_MAX: usize = 256;

/// The author's `dev` and signing key: the device's own, or `dev_R` and the
/// recovery signing key on the board the `Signer` names (seal.md §6.3,
/// SEAL-KEY-007). A `Signer::Recovery` whose board is not `board` is
/// `seal/unknown_author`: no key of `board` would sign.
// SPEC-QUESTION: `Signer::Recovery` names a board and every signing
// function of §26 takes one too; §26 does not say which wins when they
// differ. As `unwrap_root` does for `Recipient::Recovery`, this refuses;
// §22 names no code for it, and `seal/unknown_author` ("no key of this
// board signed it") is the nearest.
pub(crate) fn signer_identity<'a>(
    signer: &Signer<'a>,
    board: &BoardId,
) -> Result<(DevId, &'a ed25519_dalek::SigningKey), SealError> {
    match signer {
        Signer::Device(k) => Ok((k.public().dev, k.signing_key())),
        Signer::Recovery(k, b) => {
            if b != board {
                return Err(SealError::UnknownAuthor);
            }
            Ok((k.public().dev(b), k.signing_key()))
        }
    }
}

/// Ed25519 signature over the concatenation of `parts`.
pub(crate) fn sign_with(sk: &ed25519_dalek::SigningKey, parts: &[&[u8]]) -> [u8; 64] {
    let msg = parts.concat();
    sk.sign(&msg).to_bytes()
}

/// Strict Ed25519 verification (SEAL-GEN-010) of `sig` over `msg` under
/// `pk`: a public key that does not decode fails too.
pub(crate) fn verify_strict(pk: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> Result<(), ()> {
    let vk = ed25519_dalek::VerifyingKey::from_bytes(pk).map_err(|_| ())?;
    vk.verify_strict(msg, &ed25519_dalek::Signature::from_bytes(sig)).map_err(|_| ())
}

/// `verify_strict` over the concatenation of `parts`.
pub(crate) fn verify_strict_parts(pk: &[u8; 32], parts: &[&[u8]], sig: &[u8; 64]) -> Result<(), ()> {
    verify_strict(pk, &parts.concat(), sig)
}

/// A relay origin of §5.4: `https://`, a host of §5.4, at most 263 bytes.
fn origin_is_valid(origin: &str) -> bool {
    origin.len() <= ORIGIN_MAX && origin.strip_prefix("https://").is_some_and(host_is_valid)
}

/// The login message bytes (seal.md §16.1, SEAL-SIG-001, §26):
/// `ctx("login") ‖ vb16(origin) ‖ board_id ‖ dev ‖ u32 gen ‖ relay_nonce ‖
/// client_nonce`, `120 + n` bytes.
///
/// A `relay_nonce` that is not exactly 32 bytes is `seal/challenge_length`
/// (SEAL-SIG-002), checked first; an `origin` that is not a relay origin of
/// §5.4 is `seal/host_format`.
pub fn login_message(
    origin: &str,
    board: &BoardId,
    dev: &DevId,
    generation: u32, // the wire field `gen` of §16.1; `gen` is reserved in edition 2024
    relay_nonce: &[u8],
    client_nonce: &[u8; 32],
) -> Result<Vec<u8>, SealError> {
    if relay_nonce.len() != RELAY_NONCE_LEN {
        return Err(SealError::ChallengeLength);
    }
    if !origin_is_valid(origin) {
        return Err(SealError::HostFormat);
    }
    let n = u16::try_from(origin.len()).map_err(|_| SealError::HostFormat)?;
    let mut m = Vec::with_capacity(120 + origin.len());
    m.extend_from_slice(ctx::LOGIN);
    m.extend_from_slice(&n.to_be_bytes());
    m.extend_from_slice(origin.as_bytes());
    m.extend_from_slice(&board.0);
    m.extend_from_slice(&dev.0);
    m.extend_from_slice(&generation.to_be_bytes());
    m.extend_from_slice(relay_nonce);
    m.extend_from_slice(client_nonce);
    Ok(m)
}

/// Signs a login: `(client_nonce, sig)` (seal.md §16.1, SEAL-SIG-001,
/// SEAL-SIG-002, §26).
///
/// Validates as `login_message` before drawing anything, then draws exactly
/// the 32-byte `client_nonce` and signs with the device's key; `dev` is the
/// device's own.
pub fn sign_login(
    keys: &DeviceKeys,
    origin: &str,
    board: &BoardId,
    generation: u32, // the wire field `gen` of §16.1; `gen` is reserved in edition 2024
    relay_nonce: &[u8],
    rng: &mut dyn Entropy,
) -> Result<([u8; 32], [u8; 64]), SealError> {
    let dev = keys.public().dev;
    login_message(origin, board, &dev, generation, relay_nonce, &[0u8; 32])?;
    let mut client_nonce = [0u8; 32];
    rng.fill(&mut client_nonce)?;
    let msg = login_message(origin, board, &dev, generation, relay_nonce, &client_nonce)?;
    Ok((client_nonce, sign_with(keys.signing_key(), &[&msg])))
}

/// An admin operation of the registry (seal.md §16.2, §26).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminOp {
    Register {
        dev: DevId,
        sign_pk: [u8; 32],
        frame_hash: FrameHash,
    },
    Revoke(DevId),
    Reinstate {
        dev: DevId,
        first_epoch: u32,
        wraps: Vec<[u8; 96]>,
    },
    RecoveryRegister {
        dev: DevId,
        sign_pk: [u8; 32],
        frame_hash: FrameHash,
    },
    Fence {
        host: String,
        door: [u8; 5],
        home_gen: u32,
    },
    Seed(SeedArgs),
    Compact(CompactArgs),
    RestoreTail {
        first_seq: u64,
        frames: Vec<FrameHash>,
    },
    Purge {
        before_seq: u64,
        epoch: u32,
    },
    MintReadToken {
        dev: DevId,
        epoch: u32,
    },
    RevokeReadToken(DevId),
}

/// A count as the `u16` or `u32` the registry gives it, saturated: `args`
/// cannot fail, and `admin_message` refuses a count that does not fit
/// before it lays out the bytes.
fn count16(n: usize) -> [u8; 2] {
    u16::try_from(n).unwrap_or(u16::MAX).to_be_bytes()
}

fn count32(n: usize) -> [u8; 4] {
    u32::try_from(n).unwrap_or(u32::MAX).to_be_bytes()
}

impl AdminOp {
    /// The registry's operation name (seal.md §16.2, SEAL-SIG-004).
    pub fn name(&self) -> &'static str {
        match self {
            AdminOp::Register { .. } => "register",
            AdminOp::Revoke(_) => "revoke",
            AdminOp::Reinstate { .. } => "reinstate",
            AdminOp::RecoveryRegister { .. } => "recovery_register",
            AdminOp::Fence { .. } => "fence",
            AdminOp::Seed(_) => "seed",
            AdminOp::Compact(_) => "compact",
            AdminOp::RestoreTail { .. } => "restore_tail",
            AdminOp::Purge { .. } => "purge",
            AdminOp::MintReadToken { .. } => "mint_read_token",
            AdminOp::RevokeReadToken(_) => "revoke_read_token",
        }
    }

    /// The `args` bytes as the registry lays them out (seal.md §16.2,
    /// SEAL-SIG-004). For an operation `admin_message` refuses (see
    /// `validate`), the bytes are not meaningful: a count or length that
    /// does not fit its field is written saturated.
    pub fn args(&self) -> Vec<u8> {
        let mut a = Vec::new();
        match self {
            AdminOp::Register { dev, sign_pk, frame_hash } | AdminOp::RecoveryRegister { dev, sign_pk, frame_hash } => {
                a.extend_from_slice(&dev.0);
                a.extend_from_slice(sign_pk);
                a.extend_from_slice(&frame_hash.0);
            }
            AdminOp::Revoke(dev) | AdminOp::RevokeReadToken(dev) => a.extend_from_slice(&dev.0),
            AdminOp::Reinstate { dev, first_epoch, wraps } => {
                a.extend_from_slice(&dev.0);
                a.extend_from_slice(&first_epoch.to_be_bytes());
                a.extend_from_slice(&count16(wraps.len()));
                for w in wraps {
                    a.extend_from_slice(w);
                }
            }
            AdminOp::Fence { host, door, home_gen } => {
                a.push(u8::try_from(host.len()).unwrap_or(u8::MAX));
                a.extend_from_slice(host.as_bytes());
                a.extend_from_slice(door);
                a.extend_from_slice(&home_gen.to_be_bytes());
            }
            AdminOp::Seed(s) => {
                a.extend_from_slice(&s.home_gen.to_be_bytes());
                a.extend_from_slice(&s.first_seq.to_be_bytes());
                a.extend_from_slice(&s.chain_before.0);
                a.extend_from_slice(&s.last_seq.to_be_bytes());
                a.extend_from_slice(&s.chain_last.0);
                a.extend_from_slice(&s.blob_id.0);
                a.extend_from_slice(&count16(s.entries.len()));
                for e in &s.entries {
                    a.extend_from_slice(&e.dev.0);
                    a.extend_from_slice(&e.sign_pk);
                    a.push(e.role);
                    a.push(e.status);
                    a.extend_from_slice(&e.last_ctr.to_be_bytes());
                }
            }
            AdminOp::Compact(c) => {
                a.extend_from_slice(&c.below_seq.to_be_bytes());
                a.extend_from_slice(&c.snap_entry_seq.to_be_bytes());
                a.extend_from_slice(&c.blob_id.0);
                a.extend_from_slice(&count32(c.keep_seqs.len()));
                for k in &c.keep_seqs {
                    a.extend_from_slice(&k.to_be_bytes());
                }
            }
            AdminOp::RestoreTail { first_seq, frames } => {
                a.extend_from_slice(&first_seq.to_be_bytes());
                a.extend_from_slice(&count32(frames.len()));
                for f in frames {
                    a.extend_from_slice(&f.0);
                }
            }
            AdminOp::Purge { before_seq, epoch } => {
                a.extend_from_slice(&before_seq.to_be_bytes());
                a.extend_from_slice(&epoch.to_be_bytes());
            }
            AdminOp::MintReadToken { dev, epoch } => {
                a.extend_from_slice(&dev.0);
                a.extend_from_slice(&epoch.to_be_bytes());
            }
        }
        a
    }

    /// The registry's constraints on the arguments (seal.md §16.2, §23),
    /// checked before anything is signed. §22 names a code only for a host
    /// (`seal/host_format`); every other refusal here is `seal/bad_payload`.
    ///
    /// - `reinstate`: 1 to 256 wraps, each to `dev` (its recipient id),
    ///   `first_epoch ≥ 1` and `first_epoch + n − 1` within `u32`;
    /// - `fence`: a host of §5.4 (`seal/host_format`), `home_gen ≥ 2` (§12.7);
    /// - `seed`: at most 65,535 entries, `role` and `status` each 1 or 2,
    ///   `first_seq ≥ 1` and `last_seq ≥ first_seq − 1`;
    /// - `compact`: `keep_seq` strictly ascending and each below `below_seq`,
    ///   their count within `u32`;
    /// - `restore_tail`: `first_seq ≥ 1`, the count within `u32`;
    /// - `purge`: `before_seq ≥ 1` and `epoch ≥ 2` (a rotation's new epoch);
    /// - `mint_read_token`: `epoch ≥ 1`.
    // SPEC-QUESTION: §16.2 states these through the meaning of each field,
    // not as checks, and §22 has no code for an admin operation's arguments;
    // this refuses with `seal/bad_payload` (the arguments are the payload
    // of the admin call), and lists the choice in the task report.
    fn validate(&self) -> Result<(), SealError> {
        let bad = Err(SealError::BadPayload);
        match self {
            AdminOp::Reinstate { dev, first_epoch, wraps } => {
                let n = u32::try_from(wraps.len()).unwrap_or(u32::MAX);
                if wraps.is_empty() || wraps.len() > REINSTATE_WRAPS_MAX || *first_epoch == 0 {
                    return bad;
                }
                if first_epoch.checked_add(n - 1).is_none() {
                    return bad;
                }
                if wraps.iter().any(|w| crate::payloads::wrap_recipient(w) != dev.0) {
                    return bad;
                }
            }
            AdminOp::Fence { host, home_gen, .. } => {
                if !host_is_valid(host) {
                    return Err(SealError::HostFormat);
                }
                if *home_gen < 2 {
                    return bad;
                }
            }
            AdminOp::Seed(s) => {
                if u16::try_from(s.entries.len()).is_err() || s.first_seq == 0 {
                    return bad;
                }
                if s.last_seq < s.first_seq - 1 {
                    return bad;
                }
                if s.entries.iter().any(|e| !matches!(e.role, 1 | 2) || !matches!(e.status, 1 | 2)) {
                    return bad;
                }
            }
            AdminOp::Compact(c) => {
                if u32::try_from(c.keep_seqs.len()).is_err() {
                    return bad;
                }
                if c.keep_seqs.windows(2).any(|w| matches!(w, [a, b] if a >= b)) {
                    return bad;
                }
                if c.keep_seqs.iter().any(|&k| k >= c.below_seq) {
                    return bad;
                }
            }
            AdminOp::RestoreTail { first_seq, frames } => {
                if *first_seq == 0 || u32::try_from(frames.len()).is_err() {
                    return bad;
                }
            }
            AdminOp::Purge { before_seq, epoch } => {
                if *before_seq == 0 || *epoch < 2 {
                    return bad;
                }
            }
            AdminOp::MintReadToken { epoch, .. } => {
                if *epoch == 0 {
                    return bad;
                }
            }
            AdminOp::Register { .. }
            | AdminOp::RecoveryRegister { .. }
            | AdminOp::Revoke(_)
            | AdminOp::RevokeReadToken(_) => {}
        }
        Ok(())
    }
}

/// The `args` of `seed` (seal.md §16.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedArgs {
    pub home_gen: u32,
    pub first_seq: u64,
    pub chain_before: Chain,
    pub last_seq: u64,
    pub chain_last: Chain,
    pub blob_id: BlobId,
    pub entries: Vec<SeedEntry>,
}

/// One `dev ‖ sign_pk ‖ role ‖ status ‖ last_ctr` entry of `seed` (seal.md §16.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedEntry {
    pub dev: DevId,
    pub sign_pk: [u8; 32],
    pub role: u8,
    pub status: u8,
    pub last_ctr: u64,
}

/// The `args` of `compact` (seal.md §16.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactArgs {
    pub below_seq: u64,
    pub snap_entry_seq: u64,
    pub blob_id: BlobId,
    pub keep_seqs: Vec<u64>,
}

/// The admin message bytes (seal.md §16.2, SEAL-SIG-003, §26):
/// `ctx("admin") ‖ vb8(op) ‖ board_id ‖ signer ‖ H(args) ‖ relay_nonce`,
/// `115 + n` bytes.
///
/// A `relay_nonce` that is not exactly 32 bytes is `seal/challenge_length`
/// (SEAL-SIG-002), checked first; then the operation's arguments (see
/// `AdminOp::validate`).
pub fn admin_message(op: &AdminOp, board: &BoardId, signer: &DevId, relay_nonce: &[u8]) -> Result<Vec<u8>, SealError> {
    if relay_nonce.len() != RELAY_NONCE_LEN {
        return Err(SealError::ChallengeLength);
    }
    op.validate()?;
    let name = op.name().as_bytes();
    let n = u8::try_from(name.len()).map_err(|_| SealError::BadPayload)?;
    let mut m = Vec::with_capacity(115 + name.len());
    m.extend_from_slice(ctx::ADMIN);
    m.push(n);
    m.extend_from_slice(name);
    m.extend_from_slice(&board.0);
    m.extend_from_slice(&signer.0);
    m.extend_from_slice(&h(&[&op.args()]));
    m.extend_from_slice(relay_nonce);
    Ok(m)
}

/// Signs an admin operation (seal.md §16.2, SEAL-SIG-003, §26): the
/// signer's `dev` (or `dev_R`) in the message, its key signing. A
/// `Signer::Recovery` of another board is `seal/unknown_author` (see
/// `signer_identity`).
pub fn sign_admin(signer: &Signer, op: &AdminOp, board: &BoardId, relay_nonce: &[u8]) -> Result<[u8; 64], SealError> {
    let (dev, sk) = signer_identity(signer, board)?;
    let msg = admin_message(op, board, &dev, relay_nonce)?;
    Ok(sign_with(sk, &[&msg]))
}

#[cfg(test)]
mod tests;
