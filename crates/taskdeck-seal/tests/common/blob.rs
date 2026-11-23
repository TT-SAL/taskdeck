//! Snapshots and blobs over the public API (task S5) for the `tamper`,
//! `contexts`, `parsers` and `vectors` targets: a snapshot body from a
//! `Sim`, and a test-side sealer built with the pinned crates from §20.2 and
//! §20.3, for sealed bytes the public sealer refuses to make (any header,
//! any padded body, any `S`). Include after `board.rs` and `backup.rs`:
//! `#[path = "../common/blob.rs"] mod blob;`.
#![allow(dead_code)]

use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use taskdeck_seal::*;

use crate::backup::{BOARD_BYTES, copy_history, sha};
use crate::board::*;

pub const PART: usize = 1_048_576;

/// The snapshot body of a `Sim` at its head, with a fixed board.
pub fn snap_body(s: &Sim) -> SnapshotBody {
    let membership = s.m.to_canonical_bytes();
    SnapshotBody {
        seq: s.seq(),
        chain: s.head().chain,
        hash_version: 1,
        state_hash: state_hash(1, &sha(&[BOARD_BYTES]), &membership),
        board: BOARD_BYTES.to_vec(),
        membership,
        history: copy_history(&s.history),
    }
}

/// The bytes of §20.4, written here field by field.
pub fn body_bytes(b: &SnapshotBody) -> Vec<u8> {
    let mut o = b"TDSN".to_vec();
    o.extend(b.seq.to_be_bytes());
    o.extend(b.chain.0);
    o.extend(b.hash_version.to_be_bytes());
    o.extend(b.state_hash);
    o.extend((b.board.len() as u32).to_be_bytes());
    o.extend(&b.board);
    o.extend((b.membership.len() as u32).to_be_bytes());
    o.extend(&b.membership);
    o.extend((b.history.len() as u32).to_be_bytes());
    for r in &b.history {
        o.extend(r.to_bytes());
    }
    o
}

/// `K_snap,e` (`snap`) or `K_blob,e` (`blob`) computed here (§6.5).
pub fn kind_key(board: &BoardId, root: &[u8; 32], epoch: u32, kind: &str) -> [u8; 32] {
    let info = [format!("taskdeck/v1/key-{kind}\0").as_bytes(), &epoch.to_be_bytes()].concat();
    let mut k = [0u8; 32];
    hkdf::Hkdf::<sha2::Sha256>::new(Some(&board.0), root).expand(&info, &mut k).unwrap();
    k
}

/// `u32 len ‖ body ‖ zeros` to `P(4 + len)` (§20.3).
pub fn pad(body: &[u8]) -> Vec<u8> {
    let p = padded_len(4 + body.len() as u64) as usize;
    let mut v = (body.len() as u32).to_be_bytes().to_vec();
    v.extend_from_slice(body);
    v.resize(p, 0);
    v
}

/// What goes in the header and the AD of test-sealed bytes.
#[derive(Clone)]
pub struct Raw {
    pub magic: [u8; 4],
    pub ver: u8,
    pub suite: u8,
    pub kind: u8,
    pub epoch: u32,
    pub id: [u8; 16],
    pub nonce: [u8; 24],
    pub board: BoardId,
    pub s: u64,
    pub writer: DevId,
    /// The AD's use, normally `blob`.
    pub ad_ctx: &'static [u8],
}

impl Raw {
    pub fn new(kind: u8, epoch: u32, board: BoardId, s: u64, writer: DevId, id: [u8; 16], nonce: [u8; 24]) -> Raw {
        Raw { magic: *b"TDB1", ver: 1, suite: 1, kind, epoch, id, nonce, board, s, writer, ad_ctx: b"taskdeck/v1/blob\0" }
    }

    pub fn header(&self) -> Vec<u8> {
        let mut h = self.magic.to_vec();
        h.extend([self.ver, self.suite, self.kind]);
        h.extend(self.epoch.to_be_bytes());
        h.extend(self.id);
        h
    }

    /// `ctx(use) ‖ board_id ‖ sealed[0..27] ‖ u64 S ‖ writer_dev` (§20.2).
    pub fn ad(&self) -> Vec<u8> {
        [self.ad_ctx, &self.board.0, &self.header(), &self.s.to_be_bytes(), &self.writer.0].concat()
    }

    /// The sealed bytes of `padded` under `key`.
    pub fn seal(&self, key: &[u8; 32], padded: &[u8]) -> Vec<u8> {
        let ct = XChaCha20Poly1305::new_from_slice(key)
            .unwrap()
            .encrypt(&XNonce::from(self.nonce), Payload { msg: padded, aad: &self.ad() })
            .unwrap();
        [&self.header()[..], &self.nonce[..], &ct[..]].concat()
    }
}

/// Opens sealed bytes with the pinned crates, `None` when the AEAD fails.
pub fn open_raw(sealed: &[u8], key: &[u8; 32], ad: &[u8]) -> Option<Vec<u8>> {
    let nonce: [u8; 24] = sealed.get(27..51)?.try_into().ok()?;
    XChaCha20Poly1305::new_from_slice(key).unwrap().decrypt(&XNonce::from(nonce), Payload { msg: &sealed[51..], aad: ad }).ok()
}

/// A reference to `bytes` (its hash, size and part count).
pub fn blob_ref(bytes: &[u8], inflated: u64, epoch: u32) -> BlobRef {
    BlobRef {
        id: BlobId(bytes.get(11..27).map(|b| b.try_into().unwrap()).unwrap_or([0; 16])),
        sha256: sha(&[bytes]),
        size: bytes.len() as u64,
        parts: bytes.len().div_ceil(PART) as u32,
        inflated,
        epoch,
    }
}

/// The snapshot key of a `Sim`'s current epoch.
pub fn snap_key(s: &Sim) -> SymKey {
    let e = s.m.epoch();
    derive_epoch_key(&EpochRoot::from_bytes(s.roots[&e]), &s.board, e, KeyKind::Snap)
}

pub fn blob_key(s: &Sim) -> SymKey {
    let e = s.m.epoch();
    derive_epoch_key(&EpochRoot::from_bytes(s.roots[&e]), &s.board, e, KeyKind::Blob)
}
