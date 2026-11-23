//! Unit tests of sealed files (seal.md §19, §6.6): `K_file`, the header,
//! the records and their AD, and the reading steps of §19.4.

use super::*;
use crate::OsEntropy;

const BOARD: BoardId = BoardId([0xB0; 16]);
const DEV: DevId = DevId([0xD0; 16]);

fn key() -> StorageKey {
    StorageKey::from_bytes([0x11; 32])
}

/// A file of kind `kind` with the given record plaintexts.
fn file(kind: FileKind, plaintexts: &[&[u8]]) -> Vec<u8> {
    let (mut s, mut out) = FileSealer::create(&key(), kind, &BOARD, &DEV, &mut OsEntropy).unwrap();
    for p in plaintexts {
        out.extend(s.seal_record(p, &mut OsEntropy).unwrap());
    }
    out
}

fn open(kind: FileKind, bytes: &[u8]) -> Result<OpenedFile, SealError> {
    open_file(&key(), kind, &BOARD, &DEV, bytes)
}

fn plain(o: &OpenedFile) -> Vec<Vec<u8>> {
    o.records.iter().map(|r| r.to_vec()).collect()
}

/// The offsets of the records of a file of these plaintext lengths.
fn offsets(lens: &[usize]) -> Vec<usize> {
    let mut at = HEADER_LEN;
    let mut v = Vec::new();
    for l in lens {
        v.push(at);
        at += 44 + l;
    }
    v.push(at);
    v
}

// `K_file(kind)` is HKDF(salt = board_id, ikm = L_d, info = ctx("file-key")
// ‖ u8 kind ‖ dev, 32): a different key per kind, device and board, and
// never `L_d` itself; a record of kind k opens only under `K_file(k)`.
// spec: SEAL-KEY-014
#[test]
fn file_key_per_kind() {
    let k = key();
    let mut want = [0u8; 32];
    let info = [ctx::FILE_KEY, &[7u8][..], &DEV.0].concat();
    hkdf::Hkdf::<sha2::Sha256>::new(Some(&BOARD.0), &[0x11; 32]).expand(&info, &mut want).unwrap();
    assert_eq!(*k.file_key(7, &BOARD, &DEV), want);
    let mut seen = vec![*k.file_key(7, &BOARD, &DEV)];
    for (kind, b, d) in [(8u8, BOARD, DEV), (7, BoardId([1; 16]), DEV), (7, BOARD, DevId([1; 16]))] {
        let f = *k.file_key(kind, &b, &d);
        assert!(!seen.contains(&f));
        seen.push(f);
    }
    assert!(!seen.contains(&[0x11; 32]));
    // The ciphertext of record 0 opens under K_file(7) with AD(0).
    let f = file(FileKind::Identity, &[b"secret"]);
    let nonce: [u8; 24] = f[59..83].try_into().unwrap();
    let ad = [ctx::FILE, &f[..55], &0u64.to_be_bytes()].concat();
    let c = XChaCha20Poly1305::new_from_slice(&want).unwrap();
    assert_eq!(c.decrypt(&XNonce::from(nonce), Payload { msg: &f[83..], aad: &ad }).unwrap(), b"secret");
    let other = *k.file_key(8, &BOARD, &DEV);
    let c = XChaCha20Poly1305::new_from_slice(&other).unwrap();
    assert!(c.decrypt(&XNonce::from(nonce), Payload { msg: &f[83..], aad: &ad }).is_err());
    // A storage key drawn from entropy is 32 drawn bytes.
    let a = StorageKey::generate(&mut OsEntropy).unwrap();
    assert_ne!(a.key, [0; 32]);
}

// The header is magic, ver 1, suite 1, kind, dev, board_id, file_id (55
// bytes); a record is ct_len ‖ nonce ‖ ciphertext with its tag (44 + n
// bytes), records numbered from 0 under AD(i) = ctx("file") ‖ header ‖ u64 i.
// spec: SEAL-FILE-001
// spec: SEAL-FILE-002
#[test]
fn layout_and_ad() {
    let f = file(FileKind::Log, &[b"one", b"second"]);
    assert_eq!(&f[..4], b"TDL1");
    assert_eq!(&f[4..7], &[1, 1, 1]);
    assert_eq!(&f[7..23], &DEV.0);
    assert_eq!(&f[23..39], &BOARD.0);
    let o = offsets(&[3, 6]);
    assert_eq!(f.len(), o[2]);
    assert_eq!(&f[o[0]..o[0] + 4], &19u32.to_be_bytes());
    assert_eq!(&f[o[1]..o[1] + 4], &22u32.to_be_bytes());
    let k = key().file_key(1, &BOARD, &DEV);
    let c = XChaCha20Poly1305::new_from_slice(&*k).unwrap();
    for (i, (at, want)) in [(o[0], &b"one"[..]), (o[1], &b"second"[..])].into_iter().enumerate() {
        let nonce: [u8; 24] = f[at + 4..at + 28].try_into().unwrap();
        let end = if i == 0 { o[1] } else { o[2] };
        let ad = [ctx::FILE, &f[..55], &(i as u64).to_be_bytes()].concat();
        assert_eq!(c.decrypt(&XNonce::from(nonce), Payload { msg: &f[at + 28..end], aad: &ad }).unwrap(), want);
    }
    // Two files of the same kind get different file ids.
    let g = file(FileKind::Log, &[]);
    assert_ne!(&f[39..55], &g[39..55]);
    assert_eq!(g.len(), 55);
    let r = open(FileKind::Log, &g).unwrap();
    assert!(r.records.is_empty() && matches!(r.tail, FileTail::Clean));
}

// `create` draws exactly the 16-byte file id, `seal_record` exactly a
// 24-byte nonce per record, fresh each time; a failing source is
// `seal/entropy` and consumes no record index.
// spec: SEAL-FILE-002
// spec: SEAL-GEN-012
// spec: SEAL-GEN-004
#[cfg(feature = "test-entropy")]
#[test]
fn draws() {
    use crate::FixedEntropy;
    let (mut s, h) = FileSealer::create(&key(), FileKind::Outbox, &BOARD, &DEV, &mut FixedEntropy::new(vec![9; 16])).unwrap();
    assert_eq!(&h[39..55], &[9; 16]);
    assert!(FileSealer::create(&key(), FileKind::Outbox, &BOARD, &DEV, &mut FixedEntropy::new(vec![9; 15])).is_err());
    assert_eq!(s.seal_record(b"x", &mut FixedEntropy::new(vec![1; 23])).err(), Some(SealError::Entropy));
    let r0 = s.seal_record(b"x", &mut FixedEntropy::new(vec![1; 24])).unwrap();
    assert_eq!(&r0[4..28], &[1; 24]);
    let r1 = s.seal_record(b"x", &mut OsEntropy).unwrap();
    assert_ne!(&r0[4..28], &r1[4..28]);
    let f = [h, r0, r1].concat();
    assert_eq!(open(FileKind::Outbox, &f).unwrap().records.len(), 2);
}

// `resume` continues the numbering after `records` records; a header that
// is not one of §19.1 is `seal/file_header`.
// spec: SEAL-FILE-002
#[test]
fn resume_numbers_on() {
    let mut f = file(FileKind::Notices, &[b"a", b"b"]);
    let mut s = FileSealer::resume(&key(), &f[..55], 2).unwrap();
    f.extend(s.seal_record(b"c", &mut OsEntropy).unwrap());
    let r = open(FileKind::Notices, &f).unwrap();
    assert_eq!(plain(&r), vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
    // Resumed at the wrong index, the new record does not open.
    let mut g = file(FileKind::Notices, &[b"a"]);
    let mut s = FileSealer::resume(&key(), &g[..55], 3).unwrap();
    g.extend(s.seal_record(b"c", &mut OsEntropy).unwrap());
    assert!(matches!(open(FileKind::Notices, &g).unwrap().tail, FileTail::Bad { .. }));
    for bad in [&f[..54], &f[..56], &[&b"TDL2"[..], &f[4..55]].concat()[..]] {
        assert_eq!(FileSealer::resume(&key(), bad, 0).err(), Some(SealError::FileHeader));
    }
    let mut h = f[..55].to_vec();
    h[6] = 20;
    assert_eq!(FileSealer::resume(&key(), &h, 0).err(), Some(SealError::FileHeader));
}

// Step 1: at least 55 bytes and the header's magic, ver, suite, kind, dev
// and board_id; anything else is `seal/file_header`.
// spec: SEAL-FILE-003
#[test]
fn step1_header() {
    let f = file(FileKind::Meta, &[b"m"]);
    assert_eq!(open(FileKind::Meta, &f[..54]).err(), Some(SealError::FileHeader));
    for (i, v) in [(0usize, b'X'), (4, 2), (5, 2), (6, 8), (7, 0), (22, 0), (23, 0), (38, 0)] {
        let mut g = f.clone();
        g[i] = v;
        assert_eq!(open(FileKind::Meta, &g).err(), Some(SealError::FileHeader), "byte {i}");
    }
    assert_eq!(open(FileKind::Marker, &f).err(), Some(SealError::FileHeader));
    assert_eq!(open_file(&key(), FileKind::Meta, &BoardId([1; 16]), &DEV, &f).err(), Some(SealError::FileHeader));
    assert_eq!(open_file(&key(), FileKind::Meta, &BOARD, &DevId([1; 16]), &f).err(), Some(SealError::FileHeader));
    assert_eq!(plain(&open(FileKind::Meta, &f).unwrap()), vec![b"m".to_vec()]);
}

// Step 2: a tail of fewer than 28 bytes, a `ct_len` outside 16 to
// 67,108,880, or fewer bytes than it claims is a torn tail, with the good
// records before it; a claimed length is never allocated.
// spec: SEAL-FILE-003
// spec: SEAL-GEN-008
#[test]
fn step2_torn_tail() {
    let f = file(FileKind::Hashes, &[b"aaaa", b"bbbbbb"]);
    let o = offsets(&[4, 6]);
    for cut in [o[1] + 1, o[1] + 27, o[1] + 28, o[2] - 1] {
        let r = open(FileKind::Hashes, &f[..cut]).unwrap();
        assert_eq!(plain(&r), vec![b"aaaa".to_vec()]);
        assert!(matches!(r.tail, FileTail::Torn { valid_bytes } if valid_bytes == o[1] as u64), "cut {cut}");
    }
    for len in [0u32, 15, 67_108_881, u32::MAX, 23] {
        let mut g = f.clone();
        g[o[1]..o[1] + 4].copy_from_slice(&len.to_be_bytes());
        let r = open(FileKind::Hashes, &g).unwrap();
        assert!(matches!(r.tail, FileTail::Torn { valid_bytes } if valid_bytes == o[1] as u64), "len {len}");
        assert_eq!(r.records.len(), 1);
    }
    // A torn first record.
    let r = open(FileKind::Hashes, &f[..60]).unwrap();
    assert!(r.records.is_empty() && matches!(r.tail, FileTail::Torn { valid_bytes: 55 }));
}

// Step 3: a record that does not open is `seal/file_corrupt` when any byte
// follows it, and a bad tail when it is the last bytes of the file.
// spec: SEAL-FILE-003
#[test]
fn step3_bad_tail_and_corrupt() {
    let f = file(FileKind::Overlay, &[b"aaaa", b"bbbbbb"]);
    let o = offsets(&[4, 6]);
    for i in [o[1] + 4, o[1] + 30, o[2] - 1] {
        let mut g = f.clone();
        g[i] ^= 1;
        let r = open(FileKind::Overlay, &g).unwrap();
        assert_eq!(plain(&r), vec![b"aaaa".to_vec()]);
        assert!(matches!(r.tail, FileTail::Bad { valid_bytes } if valid_bytes == o[1] as u64));
    }
    for i in [o[0] + 4, o[0] + 30, o[1] - 1] {
        let mut g = f.clone();
        g[i] ^= 1;
        assert_eq!(open(FileKind::Overlay, &g).err(), Some(SealError::FileCorrupt));
    }
    // A good record followed by a torn one, after a corrupt one, is corrupt.
    let mut g = f[..o[2] - 3].to_vec();
    g[o[0] + 40] ^= 1;
    assert_eq!(open(FileKind::Overlay, &g).err(), Some(SealError::FileCorrupt));
}

// Records swapped, a record moved from another file of the same kind (another
// file id), of another kind, device or board, never open where they are.
// spec: SEAL-FILE-001
// spec: SEAL-KEY-014
#[test]
fn records_cannot_move() {
    let f = file(FileKind::Log, &[b"aaaa", b"bbbb"]);
    let o = offsets(&[4, 4]);
    let swapped = [&f[..o[0]], &f[o[1]..o[2]], &f[o[0]..o[1]]].concat();
    assert_eq!(open(FileKind::Log, &swapped).err(), Some(SealError::FileCorrupt));
    let other_file = file(FileKind::Log, &[b"zzzz"]);
    let moved = [&f[..o[1]], &other_file[55..]].concat();
    assert!(matches!(open(FileKind::Log, &moved).unwrap().tail, FileTail::Bad { .. }));
    // Another kind, device or board: the same key, the record under this header.
    for (kind, b, d) in [(FileKind::Outbox, BOARD, DEV), (FileKind::Log, BoardId([2; 16]), DEV), (FileKind::Log, BOARD, DevId([2; 16]))] {
        let (mut s, _) = FileSealer::create(&key(), kind, &b, &d, &mut OsEntropy).unwrap();
        let rec = s.seal_record(b"aaaa", &mut OsEntropy).unwrap();
        let spliced = [&f[..55], &rec[..]].concat();
        assert!(matches!(open(FileKind::Log, &spliced).unwrap().tail, FileTail::Bad { .. }));
    }
    // The same header with another storage key.
    let g = open_file(&StorageKey::from_bytes([0x12; 32]), FileKind::Log, &BOARD, &DEV, &f);
    assert_eq!(g.err(), Some(SealError::FileCorrupt));
}

// Record plaintexts come back in zeroizing buffers, the identity file's
// among them; a record over 67,108,864 bytes of plaintext is refused before
// anything is drawn.
// spec: SEAL-FILE-004
// spec: SEAL-GEN-006
#[test]
fn zeroizing_records_and_the_limit() {
    fn zeroizing(_: &Vec<zeroize::Zeroizing<Vec<u8>>>) {}
    fn zeroize_on_drop<T: zeroize::ZeroizeOnDrop>() {}
    zeroize_on_drop::<StorageKey>();
    zeroize_on_drop::<FileSealer>();
    let k = crate::DeviceKeys::generate(&mut OsEntropy).unwrap();
    let f = file(FileKind::Identity, &[&k.to_identity_bytes()]);
    let r = open(FileKind::Identity, &f).unwrap();
    zeroizing(&r.records);
    assert_eq!(r.records[0].as_slice(), k.to_identity_bytes().as_slice());
    let (mut s, _) = FileSealer::create(&key(), FileKind::Draft, &BOARD, &DEV, &mut OsEntropy).unwrap();
    let big = vec![0u8; 67_108_865];
    assert_eq!(s.seal_record(&big, &mut OsEntropy).err(), Some(SealError::PayloadTooLarge));
    let r = s.seal_record(&big[..67_108_864], &mut OsEntropy).unwrap();
    assert_eq!(&r[..4], &67_108_880u32.to_be_bytes());
}
