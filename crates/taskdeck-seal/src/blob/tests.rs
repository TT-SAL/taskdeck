//! Unit tests of snapshots and blobs (seal.md §20): sealing (layout, draws,
//! refusals), each step of §20.5 failing once in order with every earlier
//! step passing, for a snapshot and for a blob of kind 2, the declared mode's
//! differences, a decompression bomb, the history checks, and parts.

use super::*;
use crate::backup::tests::{Hist, dup};
use crate::receipt::tests::board::{Who, rand};
use crate::receipt::tests::board::copy;
use crate::{OsEntropy, chain_zero, state_hash};

const BOARD_BYTES: &[u8] = b"{\"cards\":[1,2,3]}";

/// A log with a rotation and a cmd: epoch 2, a history of three records.
fn hist() -> Hist {
    let mut x = Hist::new(2);
    x.rotate(Who::D(1));
    x.cmd(Who::D(0));
    x
}

fn body_of(x: &Hist) -> SnapshotBody {
    let membership = x.b.m.to_canonical_bytes();
    SnapshotBody {
        seq: x.b.seq(),
        chain: x.b.head().chain,
        hash_version: 1,
        state_hash: state_hash(1, &h(&[BOARD_BYTES]), &membership),
        board: BOARD_BYTES.to_vec(),
        membership,
        history: x.recs(),
    }
}

#[cfg(feature = "test-entropy")]
fn copy_body(b: &SnapshotBody) -> SnapshotBody {
    SnapshotBody {
        seq: b.seq,
        chain: b.chain,
        hash_version: b.hash_version,
        state_hash: b.state_hash,
        board: b.board.clone(),
        membership: b.membership.clone(),
        history: b.history.iter().map(dup).collect(),
    }
}

fn key(x: &Hist, kind: KeyKind) -> SymKey {
    let e = x.b.m.epoch;
    derive_epoch_key(&x.b.roots[&e], &x.b.board, e, kind)
}

fn writer(x: &Hist) -> DevId {
    x.b.dev(Who::D(1))
}

fn blob_ref(sb: &SealedBlob, inflated: u64, epoch: u32) -> BlobRef {
    BlobRef { id: sb.id, sha256: sb.sha256, size: sb.bytes.len() as u64, parts: sb.parts, inflated, epoch }
}

/// A sealed snapshot of `x` by device 1, with its reference.
fn sealed(x: &Hist) -> (SealedBlob, BlobRef) {
    let body = body_of(x);
    let n = encode_body(&body).unwrap().len() as u64;
    let sb = seal_snapshot(&body, &key(x, KeyKind::Snap), &x.b.board, x.b.m.epoch, &writer(x), &mut OsEntropy).unwrap();
    let r = blob_ref(&sb, n, x.b.m.epoch);
    (sb, r)
}

/// The state hash the signed entry of `x`'s snapshot names (version 1).
fn want(x: &Hist) -> [u8; 32] {
    body_of(x).state_hash
}

/// Opens with the expected values of `x`'s signed entry.
fn open(x: &Hist, bytes: &[u8], r: &BlobRef) -> Result<SnapshotBody, SealError> {
    open_snapshot(&split_parts(bytes), r, x.b.seq(), &want(x), 1, &writer(x), &key(x, KeyKind::Snap), &x.b.board)
}

/// Sealed bytes built here from a padded body: any header, any `S`, under
/// any key, so that a later step can fail with every earlier one passing.
#[allow(clippy::too_many_arguments)]
fn raw(kind: u8, epoch: u32, s: u64, w: &DevId, board: &BoardId, k: &SymKey, padded: &[u8], magic: &[u8; 4], ver: [u8; 2]) -> Vec<u8> {
    let mut header = [0u8; AD_HEADER];
    header[..4].copy_from_slice(magic);
    header[4..7].copy_from_slice(&[ver[0], ver[1], kind]);
    header[7..11].copy_from_slice(&epoch.to_be_bytes());
    header[11..].copy_from_slice(&rand::<16>());
    let nonce = rand::<24>();
    let ad = blob_ad(board, &header, s, w);
    let ct = cipher(k).unwrap().encrypt(&XNonce::from(nonce), Payload { msg: padded, aad: &ad }).unwrap();
    [&header[..], &nonce[..], &ct[..]].concat()
}

/// `u32 len ‖ body ‖ zeros` to `P(4 + len)` (§20.3).
fn pad(body: &[u8]) -> Vec<u8> {
    let p = padded_len(4 + body.len() as u64) as usize;
    let mut v = (body.len() as u32).to_be_bytes().to_vec();
    v.extend_from_slice(body);
    v.resize(p, 0);
    v
}

/// A snapshot of `x` whose compressed body is `compressed`, sealed with
/// the right header, `S` and key; the reference names `inflated`.
fn sealed_compressed(x: &Hist, compressed: &[u8], inflated: u64) -> (Vec<u8>, BlobRef) {
    let e = x.b.m.epoch;
    let bytes = raw(1, e, x.b.seq(), &writer(x), &x.b.board, &key(x, KeyKind::Snap), &pad(compressed), MAGIC, [1, 1]);
    let r = BlobRef { id: BlobId([0; 16]), sha256: h(&[&bytes]), size: bytes.len() as u64, parts: bytes.len().div_ceil(PART_LEN) as u32, inflated, epoch: e };
    (bytes, r)
}

/// A snapshot of `x` whose inflated body is `body` (any bytes).
fn sealed_body_bytes(x: &Hist, body: &[u8]) -> (Vec<u8>, BlobRef) {
    sealed_compressed(x, &deflate_raw(body).unwrap(), body.len() as u64)
}

#[track_caller]
fn fails<T>(r: Result<T, SealError>, e: SealError) {
    match r {
        Ok(_) => panic!("opened, want {e:?}"),
        Err(got) => assert_eq!(got, e),
    }
}

// ------------------------------------------------------------- sealing

// A snapshot is one AEAD message laid out as §20.2 (magic, ver, suite,
// kind 1, epoch, id, nonce, ciphertext; 67 + P bytes) under the AD of
// §20.2, of the padded body of §20.3, whose body is the raw DEFLATE of the
// layout of §20.4 with one history record per `ctl_seqs` seq; it opens back
// to the same fields.
// spec: SEAL-BLOB-001
// spec: SEAL-BLOB-004
// spec: SEAL-BLOB-006
// spec: SEAL-CTX-003
#[test]
fn a_snapshot_is_laid_out_as_20_2_to_20_4() {
    let x = hist();
    let body = body_of(&x);
    let (sb, r) = sealed(&x);
    let b = &sb.bytes;
    assert_eq!(&b[0..4], b"TDB1");
    assert_eq!(&b[4..7], &[1, 1, 1]);
    assert_eq!(&b[7..11], &2u32.to_be_bytes());
    assert_eq!(&b[11..27], &sb.id.0);
    assert_eq!(sb.sha256, h(&[b]));
    assert_eq!(sb.parts, 1);
    // The AEAD of §20.2 with the AD rebuilt here.
    let ad = [ctx::BLOB, &x.b.board.0, &b[..27], &x.b.seq().to_be_bytes(), &writer(&x).0].concat();
    let padded = cipher(&key(&x, KeyKind::Snap))
        .unwrap()
        .decrypt(&XNonce::from(<[u8; 24]>::try_from(&b[27..51]).unwrap()), Payload { msg: &b[51..], aad: &ad })
        .unwrap();
    assert_eq!(b.len(), 67 + padded.len());
    let n = u32::from_be_bytes(padded[..4].try_into().unwrap()) as usize;
    assert_eq!(padded.len() as u64, padded_len(4 + n as u64));
    assert!(padded[4 + n..].iter().all(|&z| z == 0));
    let mut inflated = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::DeflateDecoder::new(&padded[4..4 + n]), &mut inflated).unwrap();
    // §20.4, field by field.
    let mut want = b"TDSN".to_vec();
    want.extend_from_slice(&body.seq.to_be_bytes());
    want.extend_from_slice(&body.chain.0);
    want.extend_from_slice(&1u32.to_be_bytes());
    want.extend_from_slice(&body.state_hash);
    want.extend_from_slice(&(BOARD_BYTES.len() as u32).to_be_bytes());
    want.extend_from_slice(BOARD_BYTES);
    want.extend_from_slice(&(body.membership.len() as u32).to_be_bytes());
    want.extend_from_slice(&body.membership);
    want.extend_from_slice(&3u32.to_be_bytes());
    for rec in &body.history {
        want.extend_from_slice(&rec.to_bytes());
    }
    assert_eq!(inflated, want);
    assert_eq!(r.inflated, want.len() as u64);
    // The same AEAD message does not open under the AD of an entry (§9.2).
    let entry_ad = [ctx::ENTRY, &b[..48], &[1, 1], &2u32.to_be_bytes()].concat();
    assert!(
        cipher(&key(&x, KeyKind::Snap))
            .unwrap()
            .decrypt(&XNonce::from(<[u8; 24]>::try_from(&b[27..51]).unwrap()), Payload { msg: &b[51..], aad: &entry_ad })
            .is_err()
    );
    // It opens back.
    let got = open(&x, b, &r).unwrap();
    assert_eq!((got.seq, got.chain, got.hash_version, got.state_hash), (body.seq, body.chain, 1, body.state_hash));
    assert_eq!((got.board, got.membership), (body.board, body.membership));
    assert_eq!(got.history.iter().map(|r| r.to_bytes()).collect::<Vec<_>>(), body.history.iter().map(|r| r.to_bytes()).collect::<Vec<_>>());
}

// A blob of kind 2: the layout of §20.2 with kind 2, `S = 0` in the AD,
// the padded body of §20.3; bodies of many sizes seal to `67 + P(4 + n)`
// bytes, split into parts of 1,048,576 bytes and open back.
// spec: SEAL-BLOB-001
// spec: SEAL-CTX-003
#[test]
fn a_blob_is_laid_out_as_20_2_and_20_3() {
    let x = hist();
    let k = key(&x, KeyKind::Blob);
    let w = writer(&x);
    for n in [0usize, 1, 508, 509, 8188, 8189, 20_000, 1_048_576 - 67, 2_500_000] {
        let body: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
        let sb = seal_blob(&body, &k, &x.b.board, 2, &w, &mut OsEntropy).unwrap();
        assert_eq!(sb.bytes.len() as u64, 67 + padded_len(4 + n as u64), "{n}");
        assert_eq!(sb.bytes[6], 2);
        assert_eq!(sb.parts as usize, sb.bytes.len().div_ceil(1_048_576));
        let parts = split_parts(&sb.bytes);
        assert_eq!(parts.len(), sb.parts as usize);
        assert!(parts.iter().rev().skip(1).all(|p| p.len() == 1_048_576));
        assert_eq!(parts.concat(), sb.bytes);
        let r = blob_ref(&sb, 0, 2);
        assert_eq!(open_blob(&parts, &r, &w, &k, &x.b.board).unwrap(), body, "{n}");
        // `S = 0` is in the AD.
        let ad = [ctx::BLOB, &x.b.board.0, &sb.bytes[..27], &0u64.to_be_bytes(), &w.0].concat();
        let b = &sb.bytes;
        assert!(
            cipher(&k)
                .unwrap()
                .decrypt(&XNonce::from(<[u8; 24]>::try_from(&b[27..51]).unwrap()), Payload { msg: &b[51..], aad: &ad })
                .is_ok()
        );
    }
    assert!(split_parts(&[]).is_empty());
}

// Each sealing call draws a fresh blob id (and nonce): two seals of the same
// body differ in both.
// spec: SEAL-BLOB-005
// spec: SEAL-GEN-012
#[test]
fn every_seal_draws_a_fresh_id() {
    let x = hist();
    let (a, _) = sealed(&x);
    let (b, _) = sealed(&x);
    assert_ne!(a.id, b.id);
    assert_ne!(a.bytes[27..51], b.bytes[27..51]);
    let k = key(&x, KeyKind::Blob);
    let c = seal_blob(b"x", &k, &x.b.board, 2, &writer(&x), &mut OsEntropy).unwrap();
    let d = seal_blob(b"x", &k, &x.b.board, 2, &writer(&x), &mut OsEntropy).unwrap();
    assert_ne!(c.id, d.id);
}

// The id is the first 16 bytes drawn and the nonce the next 24, nothing
// else: with exactly 40 bytes the header carries them; 39 bytes is
// `seal/entropy`; a refused body draws nothing.
// spec: SEAL-BLOB-010
// spec: SEAL-BLOB-005
// spec: SEAL-GEN-012
// spec: SEAL-GEN-004
#[cfg(feature = "test-entropy")]
#[test]
fn draws_are_the_id_then_the_nonce() {
    use crate::FixedEntropy;
    let x = hist();
    let drawn: Vec<u8> = (0..40).collect();
    let body = body_of(&x);
    let sb = seal_snapshot(&body, &key(&x, KeyKind::Snap), &x.b.board, 2, &writer(&x), &mut FixedEntropy::new(drawn.clone())).unwrap();
    assert_eq!((&sb.bytes[11..27], &sb.bytes[27..51]), (&drawn[..16], &drawn[16..]));
    assert_eq!(sb.id.0[..], drawn[..16]);
    let pb = seal_blob(b"jpeg", &key(&x, KeyKind::Blob), &x.b.board, 2, &writer(&x), &mut FixedEntropy::new(drawn.clone())).unwrap();
    assert_eq!((&pb.bytes[11..27], &pb.bytes[27..51]), (&drawn[..16], &drawn[16..]));
    let short = drawn[..39].to_vec();
    fails(seal_snapshot(&body, &key(&x, KeyKind::Snap), &x.b.board, 2, &writer(&x), &mut FixedEntropy::new(short.clone())), SealError::Entropy);
    fails(seal_blob(b"jpeg", &key(&x, KeyKind::Blob), &x.b.board, 2, &writer(&x), &mut FixedEntropy::new(short)), SealError::Entropy);
    // Refused before any draw: an empty source does not fail first.
    let mut bad = copy_body(&body);
    bad.history.pop();
    fails(seal_snapshot(&bad, &key(&x, KeyKind::Snap), &x.b.board, 2, &writer(&x), &mut FixedEntropy::new(vec![])), SealError::BlobOpen);
}

// `seal_snapshot` refuses a body its reader would reject: membership bytes
// that do not parse; a history that is not exactly `ctl_seqs` from seq 1
// (one missing, swapped, one extra, none); a body over 67,108,864 bytes
// before compression; a sealed blob over 67,108,864 bytes. None of them
// draws a byte or produces sealed bytes.
// spec: SEAL-BLOB-006
// spec: SEAL-BLOB-009
#[test]
fn seal_snapshot_refusals() {
    let x = hist();
    let k = key(&x, KeyKind::Snap);
    let w = writer(&x);
    let s = |b: &SnapshotBody| seal_snapshot(b, &k, &x.b.board, 2, &w, &mut OsEntropy).map(|_| ());
    assert!(s(&body_of(&x)).is_ok());
    let mut b = body_of(&x);
    b.membership.push(0);
    fails(s(&b), SealError::MembershipFormat);
    let mut b = body_of(&x);
    b.history.remove(1);
    fails(s(&b), SealError::BlobOpen);
    let mut b = body_of(&x);
    b.history.swap(1, 2);
    fails(s(&b), SealError::BlobOpen);
    let mut b = body_of(&x);
    let extra = dup(&b.history[2]);
    b.history.push(extra);
    fails(s(&b), SealError::BlobOpen);
    let mut b = body_of(&x);
    b.history.clear();
    fails(s(&b), SealError::BlobOpen);
    // Too large before compression: by one byte, and at the limit.
    let fixed = encode_body(&body_of(&x)).unwrap().len() - BOARD_BYTES.len();
    let mut b = body_of(&x);
    b.board = vec![0; INFLATED_MAX - fixed + 1];
    fails(s(&b), SealError::SnapshotLarge);
    b.board.pop();
    assert_eq!(encode_body(&b).unwrap().len(), INFLATED_MAX);
    // The sealed blob over the maximum: a compressed body of 67,108,864
    // bytes needs `67 + P(4 + n)` > 67,108,864.
    let big = vec![0u8; INFLATED_MAX];
    fails(seal(KIND_SNAPSHOT, &big, &k, &x.b.board, 2, 9, &w, &mut OsEntropy, SealError::SnapshotLarge), SealError::SnapshotLarge);
    // Near the top, `P` rounds to multiples of 2^20: the largest body that
    // fits is 63 MiB − 4 bytes, sealed to 66,060,355 bytes.
    let fits = 66_060_284u64;
    assert_eq!(padded_len(4 + fits) + 67, 66_060_355);
    assert_eq!(padded_len(5 + fits) + 67, 67_108_931);
    fails(seal(KIND_SNAPSHOT, &vec![0u8; fits as usize + 1], &k, &x.b.board, 2, 9, &w, &mut OsEntropy, SealError::SnapshotLarge), SealError::SnapshotLarge);
    // A kind-2 blob over the maximum.
    fails(seal_blob(&vec![0u8; fits as usize + 1], &key(&x, KeyKind::Blob), &x.b.board, 2, &w, &mut OsEntropy), SealError::PayloadTooLarge);
}

/// An `Entropy` that refuses every draw: a call that draws fails with
/// `seal/entropy`, so a refusal with another code drew nothing first.
struct NoDraw;

impl crate::entropy::private::Sealed for NoDraw {}

impl Entropy for NoDraw {
    fn fill(&mut self, _out: &mut [u8]) -> Result<(), SealError> {
        Err(SealError::Entropy)
    }
}

// `seal_snapshot` refuses, with `seal/blob_open` and before it draws, a body
// with a history record above its `S` and a membership of another board than
// the one it seals for (ruling Q56, §20.4): bodies its reader would reject.
// A body that passes these checks reaches the draws.
// spec: SEAL-BLOB-006
#[test]
fn seal_snapshot_refuses_what_its_reader_would() {
    let x = hist();
    let k = key(&x, KeyKind::Snap);
    let w = writer(&x);
    let s = |b: &SnapshotBody, board: &BoardId| seal_snapshot(b, &k, board, 2, &w, &mut NoDraw).map(|_| ());
    fails(s(&body_of(&x), &x.b.board), SealError::Entropy);
    // A history record above S: the body claims an earlier S.
    let mut b = body_of(&x);
    b.seq = b.history.last().unwrap().seq - 1;
    fails(s(&b, &x.b.board), SealError::BlobOpen);
    b.seq += 1;
    fails(s(&b, &x.b.board), SealError::Entropy);
    // A membership (and its history) of another board.
    let y = hist();
    assert_ne!(y.b.board, x.b.board);
    fails(s(&body_of(&y), &x.b.board), SealError::BlobOpen);
    fails(s(&body_of(&y), &y.b.board), SealError::Entropy);
}

// --------------------------------------------------- opening, in order

// A snapshot fails each step of §20.5 once, in order, with every earlier
// step passing: 1 the parts, 2 the hash (nothing decrypted: the key is not
// needed), 3 the header, 4 the AEAD, 5 the padding, 6 the inflate, 7 the
// body.
// spec: SEAL-BLOB-001
// spec: SEAL-BLOB-002
// spec: SEAL-GEN-009
// spec: SEAL-GEN-008
#[test]
fn a_snapshot_fails_each_step_in_order() {
    let x = hist();
    let (sb, r) = sealed(&x);
    let b = &sb.bytes;
    let k = key(&x, KeyKind::Snap);
    let w = writer(&x);
    let s = x.b.seq();
    let hash = want(&x);
    let go = |parts: &[&[u8]], r: &BlobRef| open_snapshot(parts, r, s, &hash, 1, &w, &k, &x.b.board);
    assert!(go(&[b], &r).is_ok());
    // 1.
    for (n, size) in [(0u32, r.size), (2, r.size), (1, r.size + 1), (1, r.size - 1), (1, 66), (1, 67_108_865)] {
        fails(go(&[b], &BlobRef { parts: n, size, ..blob_ref(&sb, r.inflated, 2) }), SealError::BlobSize);
    }
    fails(go(&[&b[..10], &b[10..]], &BlobRef { parts: 2, ..blob_ref(&sb, r.inflated, 2) }), SealError::BlobSize);
    fails(go(&[b, &[]], &BlobRef { parts: 2, ..blob_ref(&sb, r.inflated, 2) }), SealError::BlobSize);
    // 2. A flipped byte anywhere; nothing is decrypted, so even a wrong key
    // gives the hash code.
    for i in [0usize, 6, 11, 30, 51, b.len() - 1] {
        let mut f = b.clone();
        f[i] ^= 1;
        fails(go(&[&f], &r), SealError::BlobHash);
    }
    let other = derive_epoch_key(&x.b.roots[&1], &x.b.board, 1, KeyKind::Snap);
    let mut f = b.clone();
    f[60] ^= 1;
    fails(open_snapshot(&[&f], &r, s, &hash, 1, &w, &other, &x.b.board), SealError::BlobHash);
    // 3. Each header field, the hash recomputed; the expected epoch.
    let rehash = |f: &[u8]| BlobRef { sha256: h(&[f]), ..blob_ref(&sb, r.inflated, 2) };
    for (i, v) in [(0usize, b'X'), (3, b'2'), (4, 2), (5, 2), (6, 2), (6, 0), (10, 3)] {
        let mut f = b.clone();
        f[i] = v;
        fails(go(&[&f], &rehash(&f)), SealError::BlobHeader);
    }
    fails(go(&[b], &BlobRef { epoch: 1, ..blob_ref(&sb, r.inflated, 2) }), SealError::BlobHeader);
    // 4. The AEAD: a byte of the id, the nonce or the ciphertext (hash
    // recomputed); another `S`, writer, board, key.
    for i in [11usize, 26, 27, 50, 51, b.len() - 1] {
        let mut f = b.clone();
        f[i] ^= 1;
        fails(go(&[&f], &rehash(&f)), SealError::BlobOpen);
    }
    fails(open_snapshot(&[b], &r, s + 1, &hash, 1, &w, &k, &x.b.board), SealError::BlobOpen);
    fails(open_snapshot(&[b], &r, s, &hash, 1, &x.b.dev(Who::D(0)), &k, &x.b.board), SealError::BlobOpen);
    fails(open_snapshot(&[b], &r, s, &hash, 1, &w, &k, &BoardId([1; 16])), SealError::BlobOpen);
    fails(open_snapshot(&[b], &r, s, &hash, 1, &w, &other, &x.b.board), SealError::BlobOpen);
    // 5. The padding: `U` over `L`, `L` not `P(U)`, a padding byte not zero.
    let good = pad(&deflate_raw(&encode_body(&body_of(&x)).unwrap()).unwrap());
    let mut over = good.clone();
    over[..4].copy_from_slice(&(good.len() as u32 - 3).to_be_bytes());
    let mut longer = good.clone();
    longer.extend([0u8; 512]);
    let mut dirty = good.clone();
    *dirty.last_mut().unwrap() = 1;
    let whole = |f: &[u8]| BlobRef { size: f.len() as u64, parts: 1, ..rehash(f) };
    for p in [over, longer, dirty, vec![0, 0, 0], Vec::new()] {
        let f = raw(1, 2, s, &w, &x.b.board, &k, &p, MAGIC, [1, 1]);
        fails(go(&[&f], &whole(&f)), SealError::BlobOpen);
    }
    let f = raw(1, 2, s, &w, &x.b.board, &k, &good, MAGIC, [1, 1]);
    assert!(go(&[&f], &whole(&f)).is_ok());
    // 6. The inflate: one byte more or less than `inflated`; input after the
    // stream; a stream cut short; not DEFLATE.
    fails(go(&[b], &BlobRef { inflated: r.inflated + 1, ..blob_ref(&sb, r.inflated, 2) }), SealError::Inflate);
    fails(go(&[b], &BlobRef { inflated: r.inflated - 1, ..blob_ref(&sb, r.inflated, 2) }), SealError::Inflate);
    fails(go(&[b], &BlobRef { inflated: 0, ..blob_ref(&sb, r.inflated, 2) }), SealError::Inflate);
    let c = deflate_raw(&encode_body(&body_of(&x)).unwrap()).unwrap();
    for bad in [[&c[..], &[0]].concat(), c[..c.len() - 1].to_vec(), vec![0xFF; 40]] {
        let (f, rf) = sealed_compressed(&x, &bad, r.inflated);
        fails(go(&[&f], &rf), SealError::Inflate);
    }
    // 7. The body (see the next tests for the history).
    let e = encode_body(&body_of(&x)).unwrap();
    let (f, rf) = sealed_body_bytes(&x, &e);
    assert!(go(&[&f], &rf).is_ok());
    let mut magic = e.clone();
    magic[0] = b'X';
    let trailing = [&e[..], &[0]].concat();
    let mut seq = e.clone();
    seq[4..12].copy_from_slice(&(s + 1).to_be_bytes());
    let mut hash = e.clone();
    hash[48] ^= 1;
    let mut hv = e.clone();
    hv[44..48].copy_from_slice(&2u32.to_be_bytes());
    let mut nh = e.clone();
    let at = e.len() - body_of(&x).history.iter().map(|r| r.to_bytes().len()).sum::<usize>() - 4;
    nh[at..at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    for bad in [magic, trailing, seq, hash, hv, nh, e[..e.len() - 1].to_vec(), Vec::new()] {
        let (f, rf) = sealed_body_bytes(&x, &bad);
        fails(go(&[&f], &rf), SealError::BlobOpen);
    }
    // Membership bytes that do not parse, with the state hash over them
    // expected.
    let mut b2 = body_of(&x);
    b2.membership.push(0);
    b2.state_hash = state_hash(1, &h(&[&b2.board]), &b2.membership);
    let (f, rf) = sealed_body_bytes(&x, &encode_body(&b2).unwrap());
    fails(open_snapshot(&[&f], &rf, s, &b2.state_hash, 1, &w, &k, &x.b.board), SealError::BlobOpen);
}

// Step 7 compares the body's `hash_version` and `state_hash` with the
// expected values (the signed entry's or the bundle's), as it does `S`: an
// expected state hash or version other than the body's, and a body whose
// state hash or version is not the expected one, are `seal/blob_open`. It
// also recomputes the state hash from the body's own `board` and
// `membership` under the expected version (ruling Q57 as completed): a body
// whose board does not give its signed state hash is refused, even when the
// body carries the expected hash, and so is a body that carries a hash of
// its own board that is not the expected one. It does not check the version
// against the membership's rules: a version-2 body whose hash is computed
// under version 2 opens when version 2 is expected. The declared mode
// compares none of these.
// spec: SEAL-BLOB-007
#[test]
fn step_7_compares_the_expected_values() {
    let x = hist();
    let (sb, r) = sealed(&x);
    let body = body_of(&x);
    let (w, k, s) = (writer(&x), key(&x, KeyKind::Snap), x.b.seq());
    let go = |f: &[u8], r: &BlobRef, hash: &[u8; 32], hv: u32| open_snapshot(&[f], r, s, hash, hv, &w, &k, &x.b.board);
    assert!(go(&sb.bytes, &r, &body.state_hash, 1).is_ok());
    // The expected values differ from the body's.
    for i in [0usize, 17, 31] {
        let mut other = body.state_hash;
        other[i] ^= 1;
        fails(go(&sb.bytes, &r, &other, 1), SealError::BlobOpen);
    }
    for hv in [0u32, 2, u32::MAX] {
        fails(go(&sb.bytes, &r, &body.state_hash, hv), SealError::BlobOpen);
    }
    // The body's values differ from the expected ones.
    let mut b = body_of(&x);
    b.state_hash[5] ^= 1;
    let (f, rf) = sealed_body_bytes(&x, &encode_body(&b).unwrap());
    fails(go(&f, &rf, &body.state_hash, 1), SealError::BlobOpen);
    // Expected as it stands in the body, it is not the hash of the body's
    // board and membership.
    fails(go(&f, &rf, &b.state_hash, 1), SealError::BlobOpen);
    let mut b = body_of(&x);
    b.hash_version = 2;
    let (f, rf) = sealed_body_bytes(&x, &encode_body(&b).unwrap());
    fails(go(&f, &rf, &body.state_hash, 1), SealError::BlobOpen);
    // A body whose board does not give its signed state hash, although it
    // carries that hash, is refused whoever the caller is.
    let mut b = body_of(&x);
    b.board = b"{\"cards\":[]}".to_vec();
    assert_ne!(state_hash(1, &h(&[&b.board]), &b.membership), body.state_hash);
    let (f, rf) = sealed_body_bytes(&x, &encode_body(&b).unwrap());
    fails(go(&f, &rf, &body.state_hash, 1), SealError::BlobOpen);
    // The same board with its own hash carried and expected opens.
    b.state_hash = state_hash(1, &h(&[&b.board]), &b.membership);
    let (f, rf) = sealed_body_bytes(&x, &encode_body(&b).unwrap());
    let got = go(&f, &rf, &b.state_hash, 1).unwrap();
    assert_eq!((got.board, got.state_hash), (b.board.clone(), b.state_hash));
    // A hash computed under version 1 expected as version 2 (the body
    // carrying version 2): the recomputation is under the expected version.
    let mut b = body_of(&x);
    b.hash_version = 2;
    let (f, rf) = sealed_body_bytes(&x, &encode_body(&b).unwrap());
    fails(go(&f, &rf, &body.state_hash, 2), SealError::BlobOpen);
    // A version the membership's rules do not allow, its hash computed
    // under that version, opens when that version is expected.
    let mut b = body_of(&x);
    b.hash_version = 2;
    b.state_hash = state_hash(2, &h(&[&b.board]), &b.membership);
    let (f, rf) = sealed_body_bytes(&x, &encode_body(&b).unwrap());
    assert_eq!(go(&f, &rf, &b.state_hash, 2).unwrap().hash_version, 2);
    // The declared mode compares neither: a body with another state hash
    // and another version opens.
    let mut b = body_of(&x);
    b.hash_version = 7;
    b.state_hash = [0x66; 32];
    let (f, rf) = sealed_body_bytes(&x, &encode_body(&b).unwrap());
    let root2 = copy(&x.b.roots[&2]);
    let roots = |e: u32| if e == 2 { Some(&root2) } else { None };
    let (got, _, _) = declared(&x, &[&f], &rf.sha256, rf.size, &roots).unwrap();
    assert_eq!((got.hash_version, got.state_hash), (7, [0x66; 32]));
}

// The history: one that fails a step of the projection (a signature, the
// order, the first record's counter), one that disagrees with the
// membership (an older membership; a membership with another device key),
// and a record after `S` are `seal/blob_open`, in both modes; the
// projection is checked against the receiver's own board id.
// spec: SEAL-BLOB-007
// spec: SEAL-BKP-008
#[test]
fn a_history_that_fails_or_disagrees() {
    let mut x = Hist::new(2);
    let older = x.b.m.to_canonical_bytes();
    let older_recs = x.recs();
    x.rotate(Who::D(1));
    x.cmd(Who::D(0));
    let s = x.b.seq();
    let w = writer(&x);
    let k = key(&x, KeyKind::Snap);
    let root2 = copy(&x.b.roots[&2]);
    let roots = |e: u32| if e == 2 { Some(&root2) } else { None };
    type Both = (Result<SnapshotBody, SealError>, Result<(SnapshotBody, u32, u64), SealError>);
    // Each body is opened with its own state hash and version expected, so
    // that only the history can fail.
    let both = |b: &SnapshotBody| -> Both {
        let (f, rf) = sealed_body_bytes(&x, &encode_body(b).unwrap());
        (
            open_snapshot(&[&f], &rf, s, &b.state_hash, b.hash_version, &w, &k, &x.b.board),
            open_snapshot_declared(&[&f], &rf.sha256, rf.size, s, &w, &roots, &x.b.board),
        )
    };
    let check = |b: &SnapshotBody| {
        let (a, d) = both(b);
        fails(a, SealError::BlobOpen);
        fails(d, SealError::BlobOpen);
    };
    let (a, d) = both(&body_of(&x));
    assert!(a.is_ok() && d.is_ok());
    let mut b = body_of(&x);
    b.history[1].sig[0] ^= 1;
    check(&b);
    let mut b = body_of(&x);
    b.history.swap(1, 2);
    check(&b);
    let mut b = body_of(&x);
    b.history[0].ctr = 7;
    check(&b);
    // Older membership: its ctl_seqs and commitments differ.
    let mut b = body_of(&x);
    b.membership = older.clone();
    b.state_hash = state_hash(1, &h(&[&b.board]), &b.membership);
    check(&b);
    // The older history with the current membership.
    let mut b = body_of(&x);
    b.history = older_recs;
    check(&b);
    // A membership with another device key.
    let mut m = x.b.m.dup();
    m.devices[1].hpke_pk[0] ^= 1;
    let mut b = body_of(&x);
    b.membership = m.to_canonical_bytes();
    b.state_hash = state_hash(1, &h(&[&b.board]), &b.membership);
    check(&b);
    // A record after S (the snapshot claims an earlier S).
    let mut b = body_of(&x);
    b.seq = b.history.last().unwrap().seq - 1;
    let bytes = raw(1, 2, b.seq, &w, &x.b.board, &k, &pad(&deflate_raw(&encode_body(&b).unwrap()).unwrap()), MAGIC, [1, 1]);
    let rf = BlobRef { id: BlobId([0; 16]), sha256: h(&[&bytes]), size: bytes.len() as u64, parts: 1, inflated: encode_body(&b).unwrap().len() as u64, epoch: 2 };
    fails(open_snapshot(&[&bytes], &rf, b.seq, &b.state_hash, 1, &w, &k, &x.b.board), SealError::BlobOpen);
    // Another board id for the receiver: the Genesis record's board_id check
    // (and the AD) reject it; sealed for that board, only the projection does.
    let other = BoardId([0x42; 16]);
    let kb = derive_epoch_key(&x.b.roots[&2], &other, 2, KeyKind::Snap);
    let e = encode_body(&body_of(&x)).unwrap();
    let bytes = raw(1, 2, s, &w, &other, &kb, &pad(&deflate_raw(&e).unwrap()), MAGIC, [1, 1]);
    let rf = BlobRef { id: BlobId([0; 16]), sha256: h(&[&bytes]), size: bytes.len() as u64, parts: 1, inflated: e.len() as u64, epoch: 2 };
    fails(open_snapshot(&[&bytes], &rf, s, &want(&x), 1, &w, &kb, &other), SealError::BlobOpen);
}

// A decompression bomb: a small sealed snapshot whose stream would inflate
// to 8 MiB is stopped at the signed `inflated` cap: the decompressor
// produces no more than `inflated` and one working buffer (64 KiB) before
// it stops, far less than a cap of 67,108,864 bytes would let through; in
// the declared mode a stream past 67,108,864 bytes is stopped at that cap.
// spec: SEAL-BLOB-003
// spec: SEAL-BLOB-008
#[test]
fn a_decompression_bomb_stops_at_the_cap() {
    use crate::prim::INFLATED;
    let x = hist();
    let e = encode_body(&body_of(&x)).unwrap();
    let bomb = [&e[..], &vec![0u8; 8 << 20][..]].concat();
    let c = deflate_raw(&bomb).unwrap();
    assert!(c.len() < 64 * 1024);
    let (f, rf) = sealed_compressed(&x, &c, e.len() as u64);
    let before = INFLATED.with(|n| n.get());
    fails(open(&x, &f, &rf), SealError::Inflate);
    let produced = INFLATED.with(|n| n.get()) - before;
    assert!(produced <= e.len() as u64 + 64 * 1024, "inflated {produced} bytes for a cap of {}", e.len());
    // Over the declared mode's cap: 64 sync-flushed pieces of 1 MiB of
    // zeros and one byte, then a final empty block: 67,108,865 bytes.
    let zeros = vec![0u8; 1 << 20];
    let mut piece = zeroize::Zeroizing::new(Vec::new());
    crate::prim::compress_piece(&mut piece, &zeros, false).unwrap();
    let mut one = zeroize::Zeroizing::new(Vec::new());
    crate::prim::compress_piece(&mut one, &[0], true).unwrap();
    let mut c = piece.repeat(64);
    c.extend_from_slice(&one);
    let (f, rf) = sealed_compressed(&x, &c, 0);
    let root2 = copy(&x.b.roots[&2]);
    let roots = |ep: u32| if ep == 2 { Some(&root2) } else { None };
    fails(open_snapshot_declared(&[&f], &rf.sha256, rf.size, x.b.seq(), &writer(&x), &roots, &x.b.board), SealError::Inflate);
    // At the cap the inflate passes and the body fails to parse.
    let mut end = zeroize::Zeroizing::new(Vec::new());
    crate::prim::compress_piece(&mut end, &[], true).unwrap();
    let mut c = piece.repeat(64);
    c.extend_from_slice(&end);
    let (f, rf) = sealed_compressed(&x, &c, 0);
    fails(open_snapshot_declared(&[&f], &rf.sha256, rf.size, x.b.seq(), &writer(&x), &roots, &x.b.board), SealError::BlobOpen);
}

type Roots<'a> = dyn Fn(u32) -> Option<&'a EpochRoot> + 'a;

fn declared<'a>(x: &Hist, parts: &[&[u8]], sha: &[u8; 32], size: u64, roots: &Roots<'a>) -> Result<(SnapshotBody, u32, u64), SealError> {
    open_snapshot_declared(parts, sha, size, x.b.seq(), &writer(x), roots, &x.b.board)
}

// The declared mode: it returns the body with the header's epoch and the
// inflated length; step 1 takes the declared size and derives the parts;
// step 2 the declared hash; step 3 requires kind 1 and reads the epoch;
// step 4 derives the key only from the caller's root for that epoch
// (`seal/missing_root` without one; a root that is not that epoch's does not
// open), and opens the AEAD before any field of the body is read; step 7
// checks `S` and not the state hash.
// spec: SEAL-BLOB-008
// spec: SEAL-KEY-013
// spec: SEAL-GEN-009
#[test]
fn the_declared_mode() {
    let x = hist();
    let (sb, r) = sealed(&x);
    let b = &sb.bytes;
    let w = writer(&x);
    let r1 = copy(&x.b.roots[&1]);
    let r2 = copy(&x.b.roots[&2]);
    let both = |e: u32| match e {
        1 => Some(&r1),
        2 => Some(&r2),
        _ => None,
    };
    let go = |parts: &[&[u8]], sha: &[u8; 32], size: u64, roots: &Roots<'_>| declared(&x, parts, sha, size, roots);
    let (body, epoch, inflated) = go(&[b], &sb.sha256, r.size, &both).unwrap();
    assert_eq!((epoch, inflated), (2, r.inflated));
    assert_eq!(body.membership, x.b.m.to_canonical_bytes());
    // 1, 2.
    fails(go(&[b], &sb.sha256, r.size + 1, &both), SealError::BlobSize);
    fails(go(&[b], &sb.sha256, 67_108_865, &both), SealError::BlobSize);
    fails(go(&[b], &sb.sha256, u64::MAX, &both), SealError::BlobSize);
    fails(go(&[b], &[0; 32], r.size, &both), SealError::BlobHash);
    // 3. A blob of kind 2 is not a snapshot.
    let pb = seal_blob(b"jpeg", &key(&x, KeyKind::Blob), &x.b.board, 2, &w, &mut OsEntropy).unwrap();
    fails(go(&[&pb.bytes], &pb.sha256, pb.bytes.len() as u64, &both), SealError::BlobHeader);
    // 4. No root for the header's epoch; a root given for that epoch that
    // is another epoch's; only the right one opens.
    let none = |_: u32| -> Option<&EpochRoot> { None };
    fails(go(&[b], &sb.sha256, r.size, &none), SealError::MissingRoot);
    let only1 = |e: u32| if e == 1 { Some(&r1) } else { None };
    fails(go(&[b], &sb.sha256, r.size, &only1), SealError::MissingRoot);
    let swapped = |e: u32| if e == 2 { Some(&r1) } else { None };
    fails(go(&[b], &sb.sha256, r.size, &swapped), SealError::BlobOpen);
    let only2 = |e: u32| if e == 2 { Some(&r2) } else { None };
    assert!(go(&[b], &sb.sha256, r.size, &only2).is_ok());
    // A body that is garbage behind a valid AEAD fails at the inflate; the
    // same with a tampered ciphertext fails at the AEAD first.
    let (f, _) = sealed_compressed(&x, &[0xFF; 30], 0);
    fails(go(&[&f], &h(&[&f]), f.len() as u64, &both), SealError::Inflate);
    let mut g = f.clone();
    g[60] ^= 1;
    fails(go(&[&g], &h(&[&g]), g.len() as u64, &both), SealError::BlobOpen);
    // 7. The state hash is not compared in the declared mode; `S` is.
    let mut bb = body_of(&x);
    bb.state_hash = [0x55; 32];
    let (f, rf) = sealed_body_bytes(&x, &encode_body(&bb).unwrap());
    fails(open(&x, &f, &rf), SealError::BlobOpen);
    let (got, _, _) = go(&[&f], &rf.sha256, rf.size, &both).unwrap();
    assert_eq!(got.state_hash, [0x55; 32]);
    let mut bb = body_of(&x);
    bb.seq += 64;
    let (f, rf) = sealed_body_bytes(&x, &encode_body(&bb).unwrap());
    fails(go(&[&f], &rf.sha256, rf.size, &both), SealError::BlobOpen);
}

// A blob of kind 2 fails each of steps 1 to 5 once, in order, with every
// earlier step passing; `expected.inflated` is not read; a snapshot is not
// a blob of kind 2.
// spec: SEAL-BLOB-001
// spec: SEAL-BLOB-002
// spec: SEAL-GEN-008
#[test]
fn a_blob_fails_each_step_in_order() {
    let x = hist();
    let k = key(&x, KeyKind::Blob);
    let w = writer(&x);
    let body: Vec<u8> = (0..1_500_000u32).map(|i| (i % 253) as u8).collect();
    let sb = seal_blob(&body, &k, &x.b.board, 2, &w, &mut OsEntropy).unwrap();
    let b = &sb.bytes;
    let parts = split_parts(b);
    assert_eq!(parts.len(), 2);
    let r = blob_ref(&sb, 0, 2);
    let go = |parts: &[&[u8]], r: &BlobRef| open_blob(parts, r, &w, &k, &x.b.board);
    assert_eq!(go(&parts, &r).unwrap(), body);
    assert_eq!(go(&parts, &BlobRef { inflated: u64::MAX, ..blob_ref(&sb, 0, 2) }).unwrap(), body);
    // 1.
    fails(go(&parts[..1], &r), SealError::BlobSize);
    fails(go(&[b], &r), SealError::BlobSize);
    fails(go(&[b], &BlobRef { parts: 1, ..blob_ref(&sb, 0, 2) }), SealError::BlobSize);
    fails(go(&[&b[..1_048_575], &b[1_048_575..]], &r), SealError::BlobSize);
    fails(go(&[&b[..1_048_577], &b[1_048_577..]], &r), SealError::BlobSize);
    fails(go(&[parts[0], parts[1], &[]], &BlobRef { parts: 3, ..blob_ref(&sb, 0, 2) }), SealError::BlobSize);
    fails(go(&parts, &BlobRef { size: r.size - 1, ..blob_ref(&sb, 0, 2) }), SealError::BlobSize);
    // 2. Parts in the wrong order of the right sizes would need equal
    // sizes; a flipped byte in each part.
    for i in [5usize, 1_048_576, b.len() - 1] {
        let mut f = b.clone();
        f[i] ^= 1;
        fails(go(&split_parts(&f), &r), SealError::BlobHash);
    }
    // 3.
    let rehash = |f: &[u8]| BlobRef { sha256: h(&[f]), ..blob_ref(&sb, 0, 2) };
    for (i, v) in [(0usize, b'T' ^ 1), (4, 0), (5, 9), (6, 1), (8, 9)] {
        let mut f = b.clone();
        f[i] = v;
        fails(go(&split_parts(&f), &rehash(&f)), SealError::BlobHeader);
    }
    // 4.
    for i in [11usize, 27, 51, b.len() - 1] {
        let mut f = b.clone();
        f[i] ^= 1;
        fails(go(&split_parts(&f), &rehash(&f)), SealError::BlobOpen);
    }
    fails(open_blob(&parts, &r, &x.b.dev(Who::D(0)), &k, &x.b.board), SealError::BlobOpen);
    fails(open_blob(&parts, &r, &w, &key(&x, KeyKind::Snap), &x.b.board), SealError::BlobOpen);
    fails(open_blob(&parts, &r, &w, &k, &BoardId([1; 16])), SealError::BlobOpen);
    // 5.
    let good = pad(b"jpeg");
    let mut over = good.clone();
    over[..4].copy_from_slice(&600u32.to_be_bytes());
    let mut dirty = good.clone();
    dirty[100] = 9;
    for p in [over, [&good[..], &[0u8; 512][..]].concat(), dirty] {
        let f = raw(2, 2, 0, &w, &x.b.board, &k, &p, MAGIC, [1, 1]);
        fails(go(&[&f], &BlobRef { parts: 1, size: f.len() as u64, ..rehash(&f) }), SealError::BlobOpen);
    }
    let f = raw(2, 2, 0, &w, &x.b.board, &k, &good, MAGIC, [1, 1]);
    assert_eq!(go(&[&f], &BlobRef { parts: 1, size: f.len() as u64, ..rehash(&f) }).unwrap(), b"jpeg");
    // A snapshot is not a blob of kind 2, nor a blob a snapshot.
    let (snap, sr) = sealed(&x);
    fails(open_blob(&[&snap.bytes], &sr, &w, &key(&x, KeyKind::Snap), &x.b.board), SealError::BlobHeader);
    let one = seal_blob(b"jpeg", &k, &x.b.board, 2, &w, &mut OsEntropy).unwrap();
    fails(open_snapshot(&[&one.bytes], &blob_ref(&one, 4, 2), x.b.seq(), &want(&x), 1, &w, &k, &x.b.board), SealError::BlobHeader);
}

// `K_snap,e` and `K_blob,e` differ, and a snapshot does not open under the
// blob key or the entry key of its epoch, nor a blob under the snapshot key.
// spec: SEAL-KEY-012
#[test]
fn a_snapshot_key_is_not_a_blob_key() {
    let x = hist();
    let (sb, r) = sealed(&x);
    let snap = key(&x, KeyKind::Snap);
    let blob = key(&x, KeyKind::Blob);
    let entry = key(&x, KeyKind::Entry);
    assert_ne!(snap.as_bytes(), blob.as_bytes());
    assert_ne!(snap.as_bytes(), entry.as_bytes());
    let w = writer(&x);
    assert!(open_snapshot(&[&sb.bytes], &r, x.b.seq(), &want(&x), 1, &w, &snap, &x.b.board).is_ok());
    for k in [&blob, &entry] {
        fails(open_snapshot(&[&sb.bytes], &r, x.b.seq(), &want(&x), 1, &w, k, &x.b.board), SealError::BlobOpen);
    }
    let pb = seal_blob(b"jpeg", &blob, &x.b.board, 2, &w, &mut OsEntropy).unwrap();
    let pr = blob_ref(&pb, 0, 2);
    assert!(open_blob(&[&pb.bytes], &pr, &w, &blob, &x.b.board).is_ok());
    for k in [&snap, &entry] {
        fails(open_blob(&[&pb.bytes], &pr, &w, k, &x.b.board), SealError::BlobOpen);
    }
}

// A genesis snapshot: `S = 1`, the Genesis record alone, sealed by the
// first device under `K_snap,1`.
// spec: SEAL-BLOB-006
#[test]
fn a_genesis_snapshot() {
    let x = Hist::new(1);
    assert_eq!(x.b.seq(), 1);
    let body = body_of(&x);
    assert_eq!(body.history.len(), 1);
    let k = key(&x, KeyKind::Snap);
    let w = x.b.dev(Who::D(0));
    let sb = seal_snapshot(&body, &k, &x.b.board, 1, &w, &mut OsEntropy).unwrap();
    let r = blob_ref(&sb, encode_body(&body).unwrap().len() as u64, 1);
    let got = open_snapshot(&[&sb.bytes], &r, 1, &body.state_hash, 1, &w, &k, &x.b.board).unwrap();
    assert_eq!(got.seq, 1);
    assert_ne!(got.chain, chain_zero(&x.b.board));
}
