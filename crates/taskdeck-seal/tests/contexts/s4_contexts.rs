//! The contexts of task S4: the `backup` signature against the other
//! signature uses (SEAL-CTX-002), the `file` AD against the `entry` AD, and
//! the `backup-hpke` info against the `wrap` info (SEAL-CTX-003).

use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use taskdeck_seal::*;

use crate::backup::*;
use crate::board::*;
use crate::s2_signatures::{Made, made, verifies};

fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 32];
    hkdf::Hkdf::<sha2::Sha256>::new(Some(salt), ikm).expand(info, &mut k).unwrap();
    k
}

fn opens(key: &[u8; 32], nonce: &[u8], ct: &[u8], ad: &[u8]) -> bool {
    let n: [u8; 24] = nonce.try_into().unwrap();
    XChaCha20Poly1305::new_from_slice(key).unwrap().decrypt(&XNonce::from(n), Payload { msg: ct, aad: ad }).is_ok()
}

// The backup signature joins the pairs of SEAL-CTX-002: for each pair of
// frame, login, admin, ctl and backup, the signature the crate made for one
// verifies over its own bytes and fails over the same bytes under the other
// use's context and over the other use's bytes.
// spec: SEAL-CTX-002
// spec: SEAL-SIG-006
#[test]
fn a_backup_signature_is_no_other() {
    let mut s = Sim::new(91, 1);
    let d = &s.devs[0];
    let bytes = s.rng.bytes(32);
    let file = write(&s, 0, bytes);
    let n = file.len();
    let mut all = made(&Signer::Device(d), d.public().dev, Some(d));
    all.push(Made {
        use_: ctx::BACKUP,
        signed: [ctx::BACKUP, &sha(&[&file[..n - 64]])].concat(),
        sig: file[n - 64..].try_into().unwrap(),
    });
    let pk = d.public().sign_pk;
    let uses = [ctx::FRAME, ctx::LOGIN, ctx::ADMIN, ctx::CTL, ctx::BACKUP];
    for m in &all {
        assert!(m.signed.starts_with(m.use_));
        assert!(verifies(&pk, &m.signed, &m.sig));
        let rest = &m.signed[m.use_.len()..];
        for other in uses.iter().filter(|u| **u != m.use_) {
            assert!(!m.signed.starts_with(other));
            assert!(!verifies(&pk, &[*other, rest].concat(), &m.sig));
            for o in all.iter().filter(|o| o.use_ == *other) {
                assert!(!verifies(&pk, &o.signed, &m.sig));
            }
        }
        // As the file's signature, only the backup signature passes.
        let mut f = file.clone();
        f[n - 64..].copy_from_slice(&m.sig);
        assert_eq!(verify_backup_file(&f, &pk).is_ok(), m.use_ == ctx::BACKUP);
    }
}

// A file record does not open under the AD of an entry, nor a frame's body
// under the AD of a file record, with the same key; spliced into the other
// container, each is rejected (a bad tail; `seal/aead`).
// spec: SEAL-CTX-003
#[test]
fn a_file_record_is_not_an_entry() {
    let mut s = Sim::new(92, 1);
    let l_d = [0x6C; 32];
    let dev = s.devs[0].public().dev;
    let key = StorageKey::from_bytes(l_d);
    let (mut fs, header) = FileSealer::create(&key, FileKind::Log, &s.board, &dev, &mut OsEntropy).unwrap();
    let rec = fs.seal_record(b"a log record", &mut OsEntropy).unwrap();
    let k_file = hkdf(&s.board.0, &l_d, &[ctx::FILE_KEY, &[1u8][..], &dev.0].concat());
    let file_ad = [ctx::FILE, &header[..], &0u64.to_be_bytes()].concat();
    assert!(opens(&k_file, &rec[4..28], &rec[28..], &file_ad));
    // A frame by the same device.
    let f = s.cmd(Who::D(0), &Spec::default());
    let len = f.len();
    let entry_ad = [ctx::ENTRY, &f[..48], &f[48..50], &f[50..54]].concat();
    let k_entry = hkdf(&s.board.0, &s.roots[&1], &[ctx::KEY_ENTRY, &1u32.to_be_bytes()].concat());
    assert!(opens(&k_entry, &f[54..78], &f[78..len - 64], &entry_ad));
    // Each under the other use's AD, same key.
    let as_entry = [ctx::ENTRY, &header[..48], &[1, 1], &1u32.to_be_bytes()].concat();
    assert!(!opens(&k_file, &rec[4..28], &rec[28..], &as_entry));
    let as_file = [ctx::FILE, &f[..55], &0u64.to_be_bytes()].concat();
    assert!(!opens(&k_entry, &f[54..78], &f[78..len - 64], &as_file));

    // Spliced: a record sealed under K_file with an entry AD.
    let nonce = [3u8; 24];
    let ct = XChaCha20Poly1305::new_from_slice(&k_file)
        .unwrap()
        .encrypt(&XNonce::from(nonce), Payload { msg: b"a log record", aad: &as_entry })
        .unwrap();
    let mut spliced = header.clone();
    spliced.extend((ct.len() as u32).to_be_bytes());
    spliced.extend(nonce);
    spliced.extend(&ct);
    let o = open_file(&key, FileKind::Log, &s.board, &dev, &spliced).unwrap();
    assert!(o.records.is_empty() && matches!(o.tail, FileTail::Bad { .. }));
    let good = [&header[..], &rec[..]].concat();
    assert_eq!(open_file(&key, FileKind::Log, &s.board, &dev, &good).unwrap().records.len(), 1);
    // A frame whose body is sealed under K_entry with a file AD.
    let body_ct = XChaCha20Poly1305::new_from_slice(&k_entry)
        .unwrap()
        .decrypt(&XNonce::from(<[u8; 24]>::try_from(&f[54..78]).unwrap()), Payload { msg: &f[78..len - 64], aad: &entry_ad })
        .unwrap();
    let file_like_ad = [ctx::FILE, &f[..48], &[0u8; 7], &0u64.to_be_bytes()].concat();
    let re = XChaCha20Poly1305::new_from_slice(&k_entry)
        .unwrap()
        .encrypt(&XNonce::from(<[u8; 24]>::try_from(&f[54..78]).unwrap()), Payload { msg: &body_ct, aad: &file_like_ad })
        .unwrap();
    let mut g = f[..78].to_vec();
    g.extend(&re);
    let sk = signing_key(&s.devs[0]);
    let sig = ed25519_dalek::Signer::sign(&sk, &[ctx::FRAME, &g[..]].concat());
    g.extend(sig.to_bytes());
    let r = s.classify(&g, &Opts::default()).unwrap();
    assert_eq!((class_name(&r.class), r.code), ("Refuse", Some("seal/aead")));
    let r = s.classify(&f, &Opts::default()).unwrap();
    assert_eq!(class_name(&r.class), "Accept");
}

// A backup's HPKE ciphertext does not open with the `wrap` info in the
// `backup-hpke` info's place, and a file sealed with the `wrap` info is
// rejected by `read_backup` at step 6. (The wrap side, a wrap under the
// `backup-hpke` info, is `s1_contexts::a_wrap_is_not_a_backup_payload`.)
// spec: SEAL-CTX-003
#[test]
fn a_backup_is_not_a_wrap() {
    let mut s = Sim::new(93, 1);
    let bytes = s.rng.bytes(32);
    let file = write(&s, 0, bytes);
    let seed = s.seeds[0];
    assert!(open_ct(&file, &s.board, &seed).is_some());
    assert!(open_ct_with(&file, &s.board, &seed, ctx::WRAP).is_none());
    let compressed = deflate(&crate::backup::Payload::nightly(&s).bytes());
    let mut h = Header::of(&s, &s.devs[0], 1, s.seq());
    let good = seal_compressed(&h, &s.devs[0], &compressed, [5; 32]);
    assert!(read_backup(&good, &s.board, s.rec.last().unwrap()).is_ok());
    h.info_ctx = b"taskdeck/v1/wrap\0";
    let as_wrap = seal_compressed(&h, &s.devs[0], &compressed, [5; 32]);
    assert_eq!(read_backup(&as_wrap, &s.board, s.rec.last().unwrap()).err(), Some(SealError::BackupOpen));
}
