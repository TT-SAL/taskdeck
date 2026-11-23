//! Keys and epoch-root wraps (seal.md §6, §7, §26).

use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::prim::{HpkeAead, Kdf, Kem, h, hkdf32, hpke_derive, take, x25519_public};
use crate::{BoardId, DevId, Entropy, SealError, base32, ctx};

/// Characters of the seed on the sheet (seal.md §18.1).
const SHEET_SEED_CHARS: usize = 52;
/// Characters of the check on the sheet (seal.md §18.1).
const SHEET_CHECK_CHARS: usize = 4;
/// Bits of the sheet check (seal.md §18.1, §23).
const SHEET_CHECK_BITS: usize = 20;
/// Characters of a typed sheet after normalising (seal.md §18.2, §23).
const SHEET_CHARS: usize = SHEET_SEED_CHARS + SHEET_CHECK_CHARS;

/// The 32-byte recovery seed (seal.md §6.1, SEAL-KEY-001, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct RecoverySeed {
    seed: [u8; 32],
}

impl RecoverySeed {
    /// Draws a new seed: exactly 32 bytes from `rng` (seal.md §6.1, SEAL-KEY-001).
    pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError> {
        let mut s = RecoverySeed { seed: [0u8; 32] };
        rng.fill(&mut s.seed)?;
        Ok(s)
    }

    /// The sheet text (seal.md §18.1, SEAL-KEY-004): the 52 characters of the
    /// seed and the 4 of its check, in 14 groups of 4 separated by one space.
    // SPEC-QUESTION: §18.1 says "in 14 groups of 4" and §18.1's print layout
    // separates groups by a space; the separator of the returned text is not
    // stated. This returns the groups joined by single spaces (69 characters),
    // which `from_sheet` reads back since normalising removes spaces.
    pub fn to_sheet(&self) -> Zeroizing<String> {
        let chars = base32::encode_zeroizing(&self.seed);
        let digest = Zeroizing::new(h(&[ctx::SHEET_CHECK, &self.seed]));
        let check = base32::encode_bits(&digest[..], SHEET_CHECK_BITS);
        let mut out = Zeroizing::new(String::with_capacity(SHEET_CHARS + SHEET_CHARS / 4 - 1));
        for (i, c) in chars.chars().chain(check.chars()).enumerate() {
            if i > 0 && i % 4 == 0 {
                out.push(' ');
            }
            out.push(c);
        }
        out
    }

    /// Reads a typed sheet (seal.md §18.2), in the order of its four steps,
    /// stopping at the first that fails (SEAL-SHEET-002).
    // SPEC-QUESTION: step 2 names two codes, `seal/sheet_length` and
    // `seal/sheet_char`, without an order between them. This checks the
    // length first (a 57-character text with a bad character is
    // `seal/sheet_length`): it needs no allocation, and §22 defines
    // `seal/sheet_length` as "not 56 characters after normalising" whatever
    // the characters are.
    pub fn from_sheet(text: &str) -> Result<Self, SealError> {
        // Steps 1 and 2: normalise, then exactly 56 alphabet characters.
        let values = base32::normalise(text, SHEET_CHARS).map_err(|e| match e {
            base32::NormError::Length => SealError::SheetLength,
            base32::NormError::Char => SealError::SheetChar,
        })?;
        let seed_values = values.get(..SHEET_SEED_CHARS).ok_or(SealError::SheetLength)?;
        let check_values = values.get(SHEET_SEED_CHARS..).ok_or(SealError::SheetLength)?;
        // Step 3: 260 bits, the last 4 zero.
        let seed = base32::decode_bits(seed_values, 256).ok_or(SealError::SheetPadding)?;
        let mut out = RecoverySeed { seed: [0u8; 32] };
        if seed.len() != out.seed.len() {
            return Err(SealError::SheetPadding);
        }
        out.seed.copy_from_slice(&seed);
        // Step 4: the 20-bit check.
        let typed = base32::decode_bits(check_values, SHEET_CHECK_BITS).ok_or(SealError::SheetCheck)?;
        let mut want = Zeroizing::new(h(&[ctx::SHEET_CHECK, &out.seed]));
        if let Some(b) = want.get_mut(2) {
            *b &= 0xF0;
        }
        let want = want.get(..3).ok_or(SealError::SheetCheck)?;
        if !bool::from(typed.as_slice().ct_eq(want)) {
            return Err(SealError::SheetCheck);
        }
        Ok(out)
    }

    /// The recovery keys derived from the seed (seal.md §6.1, SEAL-KEY-002,
    /// SEAL-KEY-003).
    pub fn keys(&self) -> RecoveryKeys {
        let sk_r = hkdf32(&[], &self.seed, &[ctx::RECOVERY_SIGN]);
        let ikm_h = hkdf32(&[], &self.seed, &[ctx::RECOVERY_HPKE]);
        let (hpke, _) = hpke_derive(&ikm_h[..]);
        RecoveryKeys { sign: ed25519_dalek::SigningKey::from_bytes(&sk_r), hpke }
    }
}

/// The recovery signing key and HPKE secret (seal.md §6.1, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct RecoveryKeys {
    sign: ed25519_dalek::SigningKey,
    hpke: x25519_dalek::StaticSecret,
}

impl RecoveryKeys {
    /// The recovery public key (seal.md §6.1).
    pub fn public(&self) -> RecoveryPublic {
        RecoveryPublic {
            sign_pk: self.sign.verifying_key().to_bytes(),
            hpke_pk: x25519_public(&self.hpke),
        }
    }

    /// The signing key, for the signatures of task S2.
    pub(crate) fn signing_key(&self) -> &ed25519_dalek::SigningKey {
        &self.sign
    }
}

/// The recovery public key `(rsign_pk, rhpke_pk)` (seal.md §6.1, §26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecoveryPublic {
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
}

impl RecoveryPublic {
    /// The recovery identity's `dev_R` on a board (seal.md §6.3, SEAL-KEY-006).
    pub fn dev(&self, board: &BoardId) -> DevId {
        DevId(first16(&h(&[ctx::RECOVERY_DEV, &board.0, &self.sign_pk])))
    }

    /// The key fingerprint for banners (seal.md §6.8, SEAL-KEY-016).
    pub fn fingerprint(&self) -> String {
        crate::chain::key_fingerprint(&self.sign_pk, &self.hpke_pk)
    }
}

/// `board_id` from the recovery signing key and the salt (seal.md §6.2, §26).
pub fn board_id(rsign_pk: &[u8; 32], salt: &[u8; 16]) -> BoardId {
    BoardId(first16(&h(&[ctx::BOARD, rsign_pk, salt])))
}

fn first16(d: &[u8; 32]) -> [u8; 16] {
    let mut out = [0u8; 16];
    out.copy_from_slice(d.get(..16).unwrap_or(&[0u8; 16]));
    out
}

/// Length of `DeviceKeys::to_identity_bytes`, version 1.
const IDENTITY_LEN: usize = 16 + 32 + 32;

/// A device's `dev`, signing key and X25519 key (seal.md §6.4, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct DeviceKeys {
    #[zeroize(skip)]
    dev: DevId,
    sign: ed25519_dalek::SigningKey,
    hpke: x25519_dalek::StaticSecret,
}

impl DeviceKeys {
    /// Draws new device keys (seal.md §6.4, SEAL-KEY-008), in the order of
    /// §17.10 "Draws": `dev` (16 bytes), the Ed25519 private key (32), the
    /// X25519 `ikm` (32), whose key pair is HPKE `DeriveKeyPair(ikm)`.
    pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError> {
        let mut dev = [0u8; 16];
        rng.fill(&mut dev)?;
        let mut sk = Zeroizing::new([0u8; 32]);
        rng.fill(&mut sk[..])?;
        let mut ikm = Zeroizing::new([0u8; 32]);
        rng.fill(&mut ikm[..])?;
        let (hpke, _) = hpke_derive(&ikm[..]);
        Ok(DeviceKeys { dev: DevId(dev), sign: ed25519_dalek::SigningKey::from_bytes(&sk), hpke })
    }

    /// The device's public keys (seal.md §6.4).
    pub fn public(&self) -> DevicePublic {
        DevicePublic {
            dev: self.dev,
            sign_pk: self.sign.verifying_key().to_bytes(),
            hpke_pk: x25519_public(&self.hpke),
        }
    }

    /// The signing key, for the signatures of task S2.
    pub(crate) fn signing_key(&self) -> &ed25519_dalek::SigningKey {
        &self.sign
    }

    /// The identity file contents (seal.md §19.2 kind 7; `device.md` §6.4.7
    /// carries them as `vb16`), in a zeroizing buffer (SEAL-KEY-009).
    ///
    /// Layout, version 1, 80 bytes; the version is the length, so a later
    /// layout has another length:
    ///
    /// | Offset | Field | Width | Value |
    /// |---|---|---|---|
    /// | 0 | `dev` | 16 | the device's `dev` |
    /// | 16 | signing key | 32 | the Ed25519 private key (RFC 8032 §5.1.5 seed) |
    /// | 48 | X25519 key | 32 | the HPKE private key as RFC 9180 serialises it (clamped) |
    // SPEC-QUESTION: no spec defines this layout (device.md §6.4.7 only
    // embeds it as vb16); the one above is this crate's, per the lead's
    // ruling, and is listed in the task report.
    pub fn to_identity_bytes(&self) -> Zeroizing<Vec<u8>> {
        let mut out = Zeroizing::new(Vec::with_capacity(IDENTITY_LEN));
        out.extend_from_slice(&self.dev.0);
        out.extend_from_slice(self.sign.as_bytes());
        out.extend_from_slice(self.hpke.as_bytes());
        out
    }

    /// Reads identity file contents (seal.md §19.2 kind 7): exactly the 80
    /// bytes of `to_identity_bytes`, with an X25519 key in the clamped form
    /// that function writes; anything else is `seal/file_corrupt`.
    // SPEC-QUESTION: §22 has no code for malformed identity contents; this
    // takes `seal/file_corrupt`, the code of a sealed file that is not what
    // its writer wrote (§19.4).
    pub fn from_identity_bytes(bytes: &[u8]) -> Result<Self, SealError> {
        if bytes.len() != IDENTITY_LEN {
            return Err(SealError::FileCorrupt);
        }
        let dev = take::<16>(bytes, 0).ok_or(SealError::FileCorrupt)?;
        let sk = take::<32>(bytes, 16).ok_or(SealError::FileCorrupt)?;
        let xk = take::<32>(bytes, 48).ok_or(SealError::FileCorrupt)?;
        let clamped = xk[0] & 0x07 == 0 && xk[31] & 0xC0 == 0x40;
        if !clamped {
            return Err(SealError::FileCorrupt);
        }
        Ok(DeviceKeys {
            dev: DevId(*dev),
            sign: ed25519_dalek::SigningKey::from_bytes(sk),
            hpke: x25519_dalek::StaticSecret::from(*xk),
        })
    }
}

/// A device's public keys (seal.md §6.4, §26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DevicePublic {
    pub dev: DevId,
    pub sign_pk: [u8; 32],
    pub hpke_pk: [u8; 32],
}

impl DevicePublic {
    /// The key fingerprint for banners (seal.md §6.8, SEAL-KEY-016).
    pub fn fingerprint(&self) -> String {
        crate::chain::key_fingerprint(&self.sign_pk, &self.hpke_pk)
    }
}

/// Who signs: a device, or the recovery identity on a board (seal.md §8.3, §16, §26).
pub enum Signer<'a> {
    Device(&'a DeviceKeys),
    Recovery(&'a RecoveryKeys, BoardId),
}

/// An epoch root `R_e` (seal.md §6.5, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct EpochRoot {
    root: [u8; 32],
}

impl EpochRoot {
    /// Draws a new root: exactly 32 bytes from `rng` (seal.md §6.5, SEAL-KEY-010).
    pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError> {
        let mut r = EpochRoot { root: [0u8; 32] };
        rng.fill(&mut r.root)?;
        Ok(r)
    }

    /// A root from its bytes (seal.md §26).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        EpochRoot { root: bytes }
    }

    /// The root's bytes, for §19.2 kind 7 only (SEAL-KEY-009).
    pub fn to_bytes(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.root)
    }

    /// The commitment `c_e = H(ctx("epoch") ‖ board_id ‖ u32 e ‖ R_e)`
    /// (seal.md §6.5, SEAL-KEY-011).
    pub fn commitment(&self, board: &BoardId, epoch: u32) -> [u8; 32] {
        h(&[ctx::EPOCH, &board.0, &epoch.to_be_bytes(), &self.root])
    }
}

/// Which per-kind key of an epoch (seal.md §6.5, §26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyKind {
    Entry,
    Snap,
    Blob,
}

/// A derived symmetric key (seal.md §6.5, §26).
#[derive(zeroize::ZeroizeOnDrop)]
pub struct SymKey {
    key: [u8; 32],
}

impl SymKey {
    /// The key bytes, for the AEAD calls of tasks S2 and S4.
    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.key
    }

    /// The key bytes, for the published vectors only (`seal-kdf.json`).
    // SPEC-QUESTION: §26 gives `SymKey` no way to show its bytes, yet §24's
    // `seal-kdf.json` publishes `K_entry,e`, `K_snap,e` and `K_blob,e`, and the
    // vectors are built through the public API. This accessor exists only
    // with the feature `test-entropy`, which release builds never enable.
    #[cfg(feature = "test-entropy")]
    #[doc(hidden)]
    pub fn expose_for_vectors(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.key)
    }
}

/// `K_entry,e`, `K_snap,e` or `K_blob,e` (seal.md §6.5, SEAL-KEY-012, §26):
/// `HKDF(salt = board_id, ikm = R_e, info = ctx(kind) ‖ u32 e, L = 32)`.
pub fn derive_epoch_key(root: &EpochRoot, board: &BoardId, epoch: u32, kind: KeyKind) -> SymKey {
    let info = match kind {
        KeyKind::Entry => ctx::KEY_ENTRY,
        KeyKind::Snap => ctx::KEY_SNAP,
        KeyKind::Blob => ctx::KEY_BLOB,
    };
    let okm = hkdf32(&board.0, &root.root, &[info, &epoch.to_be_bytes()]);
    SymKey { key: *okm }
}

/// The HPKE `info` of a wrap (seal.md §7):
/// `ctx("wrap") ‖ board_id ‖ u32 e ‖ recipient_id ‖ recipient_pk`.
fn wrap_info(board: &BoardId, epoch: u32, id: &DevId, pk: &[u8; 32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(ctx::WRAP.len() + 16 + 4 + 16 + 32);
    v.extend_from_slice(ctx::WRAP);
    v.extend_from_slice(&board.0);
    v.extend_from_slice(&epoch.to_be_bytes());
    v.extend_from_slice(&id.0);
    v.extend_from_slice(pk);
    v
}

/// The HPKE `aad` of a wrap (seal.md §7): `ctx("wrap-aad") ‖ u8 ver=1 ‖
/// u8 suite=1 ‖ u16 0x0020 ‖ u16 0x0001 ‖ u16 0x0003 ‖ c_e`.
fn wrap_aad(commit: &[u8; 32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(ctx::WRAP_AAD.len() + 8 + 32);
    v.extend_from_slice(ctx::WRAP_AAD);
    v.extend_from_slice(&[1, 1]);
    v.extend_from_slice(&0x0020u16.to_be_bytes());
    v.extend_from_slice(&0x0001u16.to_be_bytes());
    v.extend_from_slice(&0x0003u16.to_be_bytes());
    v.extend_from_slice(commit);
    v
}

/// Hands hpke the 32 bytes `ikm` that `wrap_root` drew, once, so that the
/// ephemeral key pair is `DeriveKeyPair(ikm)` (RFC 9180 §7.1.3; hpke's
/// `GenerateKeyPair` fills exactly `Nsk` = 32 bytes and derives from them).
/// Any other request is recorded and makes `wrap_root` fail (SEAL-WRAP-004).
struct IkmOnce<'a> {
    ikm: &'a [u8; 32],
    handed: bool,
    misused: bool,
}

impl hpke::rand_core::TryRng for IkmOnce<'_> {
    type Error = core::convert::Infallible;
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        self.misused = true;
        Ok(0)
    }
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        self.misused = true;
        Ok(0)
    }
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        if self.handed || dst.len() != self.ikm.len() {
            self.misused = true;
            dst.fill(0);
        } else {
            dst.copy_from_slice(self.ikm);
            self.handed = true;
        }
        Ok(())
    }
}

impl hpke::rand_core::TryCryptoRng for IkmOnce<'_> {}

/// Wraps a root to one recipient (seal.md §7, SEAL-WRAP-001, SEAL-WRAP-004, §26).
///
/// Draws exactly 32 bytes `ikm` from `rng` and nothing else; the ephemeral
/// key pair is `DeriveKeyPair(ikm)`. Fails with `seal/entropy` when `rng`
/// fails, and with `seal/dh_zero` when the X25519 output is all zero (a
/// small-order `recipient_pk`, SEAL-GEN-013).
pub fn wrap_root(
    root: &EpochRoot,
    board: &BoardId,
    epoch: u32,
    commit: &[u8; 32],
    recipient: DevId,
    recipient_pk: &[u8; 32],
    rng: &mut dyn Entropy,
) -> Result<[u8; 96], SealError> {
    use hpke::{Deserializable as _, Serializable as _};
    let mut ikm = Zeroizing::new([0u8; 32]);
    rng.fill(&mut ikm[..])?;
    let pk = <Kem as hpke::Kem>::PublicKey::from_bytes(recipient_pk).map_err(|_| SealError::DhZero)?;
    let info = wrap_info(board, epoch, &recipient, recipient_pk);
    let aad = wrap_aad(commit);
    let mut once = IkmOnce { ikm: &ikm, handed: false, misused: false };
    // hpke fails the encapsulation if and only if the X25519 output is all
    // zero; the single AEAD encryption of a fresh context cannot fail.
    // SPEC-QUESTION: §22 lists `seal/dh_zero` under pairing (§17.7) only;
    // SEAL-GEN-013 makes an all-zero output a failure of every operation, and
    // this reports it with the same code.
    let (enc, ct) = hpke::single_shot_seal_with_rng::<HpkeAead, Kdf, Kem>(
        &hpke::OpModeS::Base,
        &pk,
        &info,
        &root.root,
        &aad,
        &mut once,
    )
    .map_err(|_| SealError::DhZero)?;
    if once.misused || !once.handed {
        return Err(SealError::Entropy);
    }
    let mut out = [0u8; 96];
    let (id_out, rest) = out.split_at_mut(16);
    let (enc_out, ct_out) = rest.split_at_mut(32);
    id_out.copy_from_slice(&recipient.0);
    enc.write_exact(enc_out);
    if ct.len() != ct_out.len() {
        return Err(SealError::DhZero);
    }
    ct_out.copy_from_slice(&ct);
    Ok(out)
}

/// Opens a wrap addressed to this identity (seal.md §7, SEAL-WRAP-002,
/// SEAL-KEY-013, §26).
///
/// The HPKE `info` and `aad` are rebuilt from `board`, `epoch`, `commit` and
/// the receiver's own id and public key, never from the wrap; the wrap's
/// `recipient_id` must be the receiver's own id; and the opened root must
/// match `commit`. Every failure is `seal/commitment` (SEAL-WRAP-003).
pub fn unwrap_root(
    wrap: &[u8; 96],
    board: &BoardId,
    epoch: u32,
    commit: &[u8; 32],
    own: &Recipient,
) -> Result<EpochRoot, SealError> {
    use hpke::Deserializable as _;
    let fail = SealError::Commitment;
    let (own_id, secret) = match own {
        Recipient::Device(k) => (k.dev, &k.hpke),
        Recipient::Recovery(k, b) => {
            // SPEC-QUESTION: `Recipient::Recovery` names a board and so does
            // `board`; §26 does not say which wins when they differ. This
            // refuses: such a call names two boards at once.
            if b != board {
                return Err(fail);
            }
            (k.public().dev(b), &k.hpke)
        }
    };
    let own_pk = x25519_public(secret);
    let id = take::<16>(wrap, 0).ok_or(fail)?;
    let enc = take::<32>(wrap, 16).ok_or(fail)?;
    let ct = wrap.get(48..).ok_or(fail)?;
    if !bool::from(id.ct_eq(&own_id.0)) {
        return Err(fail);
    }
    let enc = <Kem as hpke::Kem>::EncappedKey::from_bytes(enc).map_err(|_| fail)?;
    let sk = <Kem as hpke::Kem>::PrivateKey::from_bytes(secret.as_bytes()).map_err(|_| fail)?;
    let info = wrap_info(board, epoch, &own_id, &own_pk);
    let aad = wrap_aad(commit);
    let pt = Zeroizing::new(
        hpke::single_shot_open::<HpkeAead, Kdf, Kem>(&hpke::OpModeR::Base, &sk, &enc, &info, ct, &aad)
            .map_err(|_| fail)?,
    );
    let mut root = EpochRoot { root: [0u8; 32] };
    if pt.len() != root.root.len() {
        return Err(fail);
    }
    root.root.copy_from_slice(&pt);
    if !bool::from(root.commitment(board, epoch).ct_eq(commit)) {
        return Err(fail);
    }
    Ok(root)
}

/// Whose wrap is opened: a device, or the recovery identity on a board (seal.md §7, §26).
pub enum Recipient<'a> {
    Device(&'a DeviceKeys),
    Recovery(&'a RecoveryKeys, BoardId),
}

#[cfg(test)]
mod tests;
