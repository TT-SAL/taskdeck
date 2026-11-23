//! Backups over the public API (task S4) for the `tamper`, `contexts`,
//! `parsers` and `vectors` targets: a nightly or segment input from a `Sim`,
//! and a test-side sealer, built with the pinned crates from §21.2, §21.3 and
//! §16.3, for files the public writer refuses to make (a forged writer, a
//! payload with a defect after the HPKE layer). Include after `board.rs`:
//! `#[path = "../common/backup.rs"] mod backup;`.
#![allow(dead_code)]

use ed25519_dalek::Signer as _;
use hpke::{Deserializable as _, Serializable as _};
use taskdeck_seal::*;

use crate::board::*;

pub type Kem = hpke::kem::X25519HkdfSha256;
pub type Kdf = hpke::kdf::HkdfSha256;
pub type Aead = hpke::aead::ChaCha20Poly1305;

pub const BOARD_BYTES: &[u8] = b"{\"cards\":[]}";
pub const WRITTEN_MS: i64 = 1_767_225_600_123;

pub fn sha(parts: &[&[u8]]) -> [u8; 32] {
    <sha2::Sha256 as sha2::Digest>::digest(parts.concat()).into()
}

/// Copies of history records.
pub fn copy_history(h: &[HistoryRecord]) -> Vec<HistoryRecord> {
    h.iter().map(|r| HistoryRecord::from_bytes(&r.to_bytes()).unwrap()).collect()
}

pub fn roots(s: &Sim) -> Vec<(u32, EpochRoot)> {
    s.roots.iter().map(|(e, r)| (*e, EpochRoot::from_bytes(*r))).collect()
}

/// The nightly content of a `Sim` at its head, with a fixed board.
pub fn nightly(s: &Sim) -> BackupContent {
    let membership = s.m.to_canonical_bytes();
    let state_hash = state_hash(1, &sha(&[BOARD_BYTES]), &membership);
    BackupContent::Nightly { membership, board: BOARD_BYTES.to_vec(), head: s.head(), hash_version: 1, state_hash }
}

pub fn input<'a>(s: &Sim, history: &'a [HistoryRecord], content: BackupContent) -> BackupInput<'a> {
    BackupInput {
        board: s.board,
        seq: s.seq(),
        written_ms: WRITTEN_MS,
        history,
        content,
        roots: roots(s),
        read_token: Some("rt1.backup-token".into()),
        meta: b"{\"v\":1}".to_vec(),
        pinned_recovery: s.rec.last().unwrap().public(),
    }
}

/// A nightly backup by device `w`, with 32 bytes of entropy.
pub fn write(s: &Sim, w: usize, entropy: Vec<u8>) -> Vec<u8> {
    let h = copy_history(&s.history);
    write_backup(&s.devs[w], input(s, &h, nightly(s)), &mut *source(entropy)).unwrap()
}

pub fn signing_key(k: &DeviceKeys) -> ed25519_dalek::SigningKey {
    let id = k.to_identity_bytes();
    ed25519_dalek::SigningKey::from_bytes(id[16..48].try_into().unwrap())
}

/// Re-signs a file as `k` (§16.3), whatever its body.
pub fn resign(file: &mut [u8], k: &DeviceKeys) {
    let n = file.len() - 64;
    let sig = signing_key(k).sign(&[b"taskdeck/v1/backup\0".as_slice(), &sha(&[&file[..n]])].concat());
    file[n..].copy_from_slice(&sig.to_bytes());
}

/// Raw DEFLATE of `b` (RFC 1951).
pub fn deflate(b: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(b).unwrap();
    e.finish().unwrap()
}

pub fn inflate(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::DeflateDecoder::new(b), &mut out).unwrap();
    out
}

/// Hands hpke the given bytes, for a deterministic ephemeral key.
pub struct Given(pub Vec<u8>);
impl hpke::rand_core::TryRng for Given {
    type Error = core::convert::Infallible;
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        unreachable!("hpke draws bytes")
    }
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        unreachable!("hpke draws bytes")
    }
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        let rest = self.0.split_off(dst.len());
        dst.copy_from_slice(&self.0);
        self.0 = rest;
        Ok(())
    }
}
impl hpke::rand_core::TryCryptoRng for Given {}

/// What goes in the header of a test-sealed file.
pub struct Header {
    pub file_type: u8,
    pub board: BoardId,
    pub writer: DevicePublic,
    pub recipient: RecoveryPublic,
    pub seq: u64,
    pub written_ms: i64,
    /// The HPKE info's use, normally `backup-hpke`.
    pub info_ctx: &'static [u8],
}

impl Header {
    pub fn of(s: &Sim, writer: &DeviceKeys, file_type: u8, seq: u64) -> Header {
        Header {
            file_type,
            board: s.board,
            writer: writer.public(),
            recipient: s.rec.last().unwrap().public(),
            seq,
            written_ms: WRITTEN_MS,
            info_ctx: b"taskdeck/v1/backup-hpke\0",
        }
    }
}

/// A file of §21.2 built here: `compressed` sealed with HPKE base mode to
/// `h.recipient` (ephemeral `ikm` given), signed by `signer` (§16.3).
pub fn seal_compressed(h: &Header, signer: &DeviceKeys, compressed: &[u8], ikm: [u8; 32]) -> Vec<u8> {
    let dev_r = h.recipient.dev(&h.board);
    let info = [h.info_ctx, &h.board.0, &h.seq.to_be_bytes(), &h.writer.dev.0, &dev_r.0, &h.recipient.hpke_pk].concat();
    let pk = <Kem as hpke::Kem>::PublicKey::from_bytes(&h.recipient.hpke_pk).unwrap();
    let (enc, mut cx) =
        hpke::setup_sender_with_rng::<Aead, Kdf, Kem>(&hpke::OpModeS::Base, &pk, &info, &mut Given(ikm.to_vec())).unwrap();
    let mut f = b"TDBK".to_vec();
    f.extend([1, 1, h.file_type]);
    f.extend(h.board.0);
    f.extend(h.writer.dev.0);
    f.extend(h.writer.sign_pk);
    f.extend(dev_r.0);
    f.extend(h.seq.to_be_bytes());
    f.extend(h.written_ms.to_be_bytes());
    f.extend(enc.to_bytes());
    f.extend(((compressed.len() + 16) as u64).to_be_bytes());
    let aad = [b"taskdeck/v1/backup-aad\0".as_slice(), &f].concat();
    let ct = cx.seal(compressed, &aad).unwrap();
    f.extend(ct);
    f.extend([0u8; 64]);
    resign(&mut f, signer);
    f
}

/// Opens a file's ciphertext with the recovery keys of a seed (the
/// reader's side of §21.3, here): the compressed payload.
pub fn open_ct(file: &[u8], board: &BoardId, seed: &[u8; 32]) -> Option<Vec<u8>> {
    open_ct_with(file, board, seed, b"taskdeck/v1/backup-hpke\0")
}

/// `open_ct` with another use in the info's place.
pub fn open_ct_with(file: &[u8], board: &BoardId, seed: &[u8; 32], info_ctx: &[u8]) -> Option<Vec<u8>> {
    let seed_keys = seed_keys(seed);
    let dev_r = seed_keys.1.dev(board);
    let n = file.len();
    let info = [info_ctx, &board.0, &file[87..95], &file[23..39], &dev_r.0, &seed_keys.1.hpke_pk].concat();
    let aad = [b"taskdeck/v1/backup-aad\0".as_slice(), &file[..143]].concat();
    let enc = <Kem as hpke::Kem>::EncappedKey::from_bytes(&file[103..135]).ok()?;
    hpke::single_shot_open::<Aead, Kdf, Kem>(&hpke::OpModeR::Base, &seed_keys.0, &enc, &info, &file[143..n - 64], &aad).ok()
}

/// The recovery HPKE key pair of a seed (§6.1), computed here.
pub fn seed_keys(seed: &[u8; 32]) -> (<Kem as hpke::Kem>::PrivateKey, RecoveryPublic) {
    let mut ikm = [0u8; 32];
    hkdf::Hkdf::<sha2::Sha256>::new(Some(&[]), seed).expand(b"taskdeck/v1/recovery-hpke\0", &mut ikm).unwrap();
    let (sk, _) = <Kem as hpke::Kem>::derive_keypair(&ikm);
    (sk, recovery_from_seed(seed).public())
}

/// A payload of §21.5 written field by field.
#[derive(Clone)]
pub struct Payload {
    pub file_type: u8,
    pub history: Vec<Vec<u8>>,
    pub membership: Vec<u8>,
    pub board: Vec<u8>,
    pub head_chain: [u8; 32],
    pub hash_version: u32,
    pub state_hash: [u8; 32],
    pub first_seq: u64,
    pub chain_before: [u8; 32],
    pub frames: Vec<Vec<u8>>,
    pub roots: Vec<(u32, [u8; 32])>,
    pub token: Vec<u8>,
    pub meta: Vec<u8>,
    pub magic: [u8; 4],
}

impl Payload {
    /// The nightly payload of a `Sim` at its head.
    pub fn nightly(s: &Sim) -> Payload {
        let membership = s.m.to_canonical_bytes();
        Payload {
            file_type: 1,
            history: s.history.iter().map(|r| r.to_bytes()).collect(),
            state_hash: state_hash(1, &sha(&[BOARD_BYTES]), &membership),
            membership,
            board: BOARD_BYTES.to_vec(),
            head_chain: s.head().chain.0,
            hash_version: 1,
            first_seq: 0,
            chain_before: [0; 32],
            frames: Vec::new(),
            roots: s.roots.iter().map(|(e, r)| (*e, *r)).collect(),
            token: b"rt1.backup-token".to_vec(),
            meta: b"{\"v\":1}".to_vec(),
            magic: *b"TDBP",
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        let vb32 = |p: &mut Vec<u8>, x: &[u8]| {
            p.extend((x.len() as u32).to_be_bytes());
            p.extend(x);
        };
        let mut p = self.magic.to_vec();
        p.extend(1u32.to_be_bytes());
        p.push(self.file_type);
        p.extend((self.history.len() as u32).to_be_bytes());
        for r in &self.history {
            p.extend(r);
        }
        vb32(&mut p, &self.membership);
        if self.file_type == 1 {
            vb32(&mut p, &self.board);
            p.extend(self.head_chain);
            p.extend(self.hash_version.to_be_bytes());
            p.extend(self.state_hash);
        } else {
            p.extend(self.first_seq.to_be_bytes());
            p.extend(self.chain_before);
            p.extend((self.frames.len() as u32).to_be_bytes());
            for f in &self.frames {
                vb32(&mut p, f);
            }
        }
        p.extend((self.roots.len() as u16).to_be_bytes());
        for (e, r) in &self.roots {
            p.extend(e.to_be_bytes());
            p.extend(r);
        }
        p.extend((self.token.len() as u16).to_be_bytes());
        p.extend(&self.token);
        vb32(&mut p, &self.meta);
        p
    }
}
