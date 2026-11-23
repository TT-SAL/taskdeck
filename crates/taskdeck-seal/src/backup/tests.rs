//! Unit tests of backups (seal.md §21, §16.3): history records, the writer,
//! the eleven steps of §21.6 each failing once in order with every earlier
//! step passing, `verify_backup_file`, and strict signatures. The projection's
//! own steps are in `projection/tests.rs`, which uses `Hist` from here.

use zeroize::Zeroizing;

use super::*;
use crate::payloads::ctl_inner;
use crate::prim::deflate_raw;
use crate::receipt::tests::board::{Board, Spec, Who, copy, rand};
use crate::receipt::tests::scalar;
use crate::signatures::signer_identity;
use crate::{Class, CtlItem, OsEntropy, RecoverySeed};

/// A board whose accepted control entries are kept as history records.
pub struct Hist {
    pub b: Board,
    pub recs: Vec<HistoryRecord>,
}

pub fn dup(r: &HistoryRecord) -> HistoryRecord {
    HistoryRecord::from_bytes(&r.to_bytes()).unwrap()
}

impl Hist {
    /// Genesis, then `n − 1` devices added by device 0.
    pub fn new(n: usize) -> Hist {
        let b = Board::new(1);
        let f = b.frames[0].clone();
        let r = b.classify_genesis(&f, &b.anchor());
        let rec = HistoryRecord::from_accepted(1, &f, r.opened.as_ref().unwrap()).unwrap();
        let mut h = Hist { b, recs: vec![rec] };
        for _ in 1..n {
            h.add_device();
        }
        h
    }

    /// Classifies and records a frame; an accepted `ctl` becomes a record.
    pub fn push(&mut self, f: Vec<u8>) -> crate::Receipt {
        let r = self.b.push(f);
        if matches!(r.class, Class::Accept)
            && let Some(o) = &r.opened
            && o.kind == Kind::Ctl
        {
            let rec = HistoryRecord::from_accepted(self.b.seq(), self.b.frames.last().unwrap(), o).unwrap();
            self.recs.push(rec);
        }
        r
    }

    pub fn accept(&mut self, f: Vec<u8>) {
        let r = self.push(f);
        assert!(matches!(r.class, Class::Accept) && r.code.is_none(), "not accepted: {:?}", r.code);
    }

    pub fn ctl(&mut self, who: Who, items: Vec<CtlItem>) {
        let f = self.b.ctl(who, items, &Spec::default());
        self.accept(f);
    }

    pub fn cmd(&mut self, who: Who) {
        let f = self.b.cmd(who, &Spec::default());
        self.accept(f);
    }

    pub fn add_device(&mut self) -> usize {
        let k = DeviceKeys::generate(&mut OsEntropy).unwrap();
        let item = self.b.added_item(&k);
        self.b.devs.push(k);
        self.ctl(Who::D(0), vec![item]);
        self.b.devs.len() - 1
    }

    pub fn revoke(&mut self, who: Who, t: usize) {
        let items = self.b.rotate_items(vec![CtlItem::DeviceRevoked(self.b.dev(Who::D(t)))]);
        self.ctl(who, items);
    }

    pub fn rotate(&mut self, who: Who) {
        let items = self.b.rotate_items(vec![]);
        self.ctl(who, items);
    }

    /// A direct recovery-key change by the current recovery identity, with
    /// its rotation; the new keys become `Who::R`.
    pub fn change_recovery(&mut self) {
        let nk = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
        let p = nk.public();
        let k = CtlItem::RecoveryKeyChange(crate::RecoveryKeyChange {
            mode: crate::RecoveryKeyMode::Direct,
            sign_pk: Some(p.sign_pk),
            hpke_pk: Some(p.hpke_pk),
        });
        let items = self.b.rotate_items(vec![k]);
        self.ctl(Who::R, items);
        self.b.rec.push(nk);
    }

    pub fn recs(&self) -> Vec<HistoryRecord> {
        self.recs.iter().map(dup).collect()
    }

    pub fn roots(&self) -> Vec<(u32, EpochRoot)> {
        self.b.roots.iter().map(|(e, r)| (*e, copy(r))).collect()
    }

    pub fn rec_keys(&self) -> &RecoveryKeys {
        self.b.rec.last().unwrap()
    }

    /// A record by `who` at `seq` with `ctr` over `items` (no shape check),
    /// its inner signature by `who`'s key (§11.3).
    pub fn signed(&self, who: Who, seq: u64, ctr: u64, items: &[CtlItem]) -> HistoryRecord {
        let (_, bytes) = crate::payloads::items_bytes(items).unwrap();
        self.signed_bytes(who, seq, ctr, bytes)
    }

    pub fn signed_bytes(&self, who: Who, seq: u64, ctr: u64, items: Vec<u8>) -> HistoryRecord {
        let dev = self.b.dev(who);
        let entry_id = EntryId(rand());
        let at_ms = self.b.now;
        let inner = ctl_inner(&self.b.board, &dev, ctr, &entry_id, at_ms, &items);
        let (_, sk) = signer_identity(&self.b.signer(who), &self.b.board).unwrap();
        HistoryRecord { seq, dev, ctr, entry_id, at_ms, frame_hash: FrameHash(rand()), items, sig: sign_with(sk, &[&inner]) }
    }

    pub fn nightly(&self) -> BackupContent {
        let membership = self.b.m.to_canonical_bytes();
        let board = b"{\"board\":1}".to_vec();
        let state_hash = state_hash(1, &h(&[&board]), &membership);
        BackupContent::Nightly { membership, board, head: self.b.head(), hash_version: 1, state_hash }
    }

    pub fn input<'a>(&self, history: &'a [HistoryRecord], content: BackupContent) -> BackupInput<'a> {
        BackupInput {
            board: self.b.board,
            seq: self.b.seq(),
            written_ms: 1_767_225_600_123,
            history,
            content,
            roots: self.roots(),
            read_token: Some("tok-1".into()),
            meta: b"{\"m\":1}".to_vec(),
            pinned_recovery: self.rec_keys().public(),
        }
    }

    /// A nightly backup written by device `w`.
    pub fn write(&self, w: usize) -> Vec<u8> {
        let recs = self.recs();
        write_backup(&self.b.devs[w], self.input(&recs, self.nightly()), &mut OsEntropy).unwrap()
    }

    pub fn read(&self, file: &[u8]) -> Result<VerifiedBackup, SealError> {
        read_backup(file, &self.b.board, self.rec_keys())
    }

    /// The fields of a header for this board, sealed to the current
    /// recovery key.
    pub fn fields(&self, writer: &DeviceKeys, file_type: u8, seq: u64) -> HeaderFields {
        let p = writer.public();
        HeaderFields {
            file_type,
            board: self.b.board,
            writer_dev: p.dev,
            writer_sign_pk: p.sign_pk,
            recipient: self.rec_keys().public().dev(&self.b.board),
            seq,
            written_ms: 5,
        }
    }

    /// A file of raw-DEFLATE bytes `compressed`, sealed and signed by `writer`.
    pub fn seal_compressed(&self, writer: &DeviceKeys, file_type: u8, seq: u64, compressed: &[u8]) -> Vec<u8> {
        let f = self.fields(writer, file_type, seq);
        seal_file(writer, &f, &self.rec_keys().public().hpke_pk, compressed, &mut OsEntropy).unwrap()
    }

    /// A nightly file of payload bytes `payload`, sealed and signed by `writer`.
    pub fn seal_payload(&self, writer: &DeviceKeys, payload: &[u8]) -> Vec<u8> {
        self.seal_compressed(writer, NIGHTLY, self.b.seq(), &deflate_raw(payload).unwrap())
    }

    /// The nightly payload of §21.5 for `recs` and this board's state.
    pub fn payload(&self, recs: &[HistoryRecord]) -> Zeroizing<Vec<u8>> {
        encode_payload(recs, &self.nightly(), &self.roots(), b"tok-1", b"{}").unwrap()
    }
}

/// Re-signs a file as `k` (§16.3), whatever its body.
pub fn resign(file: &mut [u8], k: &DeviceKeys) {
    let n = file.len() - 64;
    let sig = sign_with(k.signing_key(), &[ctx::BACKUP, &h(&[&file[..n]])]);
    file[n..].copy_from_slice(&sig);
}

#[track_caller]
fn fails(r: Result<VerifiedBackup, SealError>, e: SealError) {
    match r {
        Ok(_) => panic!("read, want {e:?}"),
        Err(got) => assert_eq!(got, e),
    }
}

// ------------------------------------------------------------- records

// `from_accepted` takes seq, dev, ctr, entry_id, at_ms, H(frame), the
// canonical items and the inner signature (§21.5); `to_bytes` is the layout
// of §21.5 and `from_bytes` reads it back.
// spec: SEAL-BKP-010
// spec: SEAL-BKP-002
#[test]
fn from_accepted_fields_and_layout() {
    let mut x = Hist::new(1);
    let k = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let item = x.b.added_item(&k);
    let f = x.b.ctl(Who::D(0), vec![item.clone()], &Spec::default());
    let r = x.b.push(f.clone());
    let o = r.opened.as_ref().unwrap();
    let rec = HistoryRecord::from_accepted(2, &f, o).unwrap();
    let hdr = crate::frame_header(&f).unwrap();
    let (_, items_bytes) = crate::payloads::items_bytes(&[item]).unwrap();
    let u = parse_ctl_unsigned(&o.payload).unwrap();
    assert_eq!(rec.seq(), 2);
    let b = rec.to_bytes();
    assert_eq!(b.len(), 156 + items_bytes.len());
    assert_eq!(&b[0..8], &2u64.to_be_bytes());
    assert_eq!(&b[8..24], &hdr.dev.0);
    assert_eq!(&b[24..32], &hdr.ctr.to_be_bytes());
    assert_eq!(&b[32..48], &o.entry_id.0);
    assert_eq!(&b[48..56], &o.at_ms.to_be_bytes());
    assert_eq!(&b[56..88], &h(&[&f]));
    assert_eq!(&b[88..92], &(items_bytes.len() as u32).to_be_bytes());
    assert_eq!(&b[92..92 + items_bytes.len()], &items_bytes[..]);
    assert_eq!(&b[92 + items_bytes.len()..], &u.sig);
    // The signature is the inner one of §11.3 over these fields.
    let inner = ctl_inner(&x.b.board, &hdr.dev, hdr.ctr, &o.entry_id, o.at_ms, &items_bytes);
    assert!(crate::signatures::verify_strict(&x.b.dev_pub(0).sign_pk, &inner, &u.sig).is_ok());
    assert_eq!(HistoryRecord::from_bytes(&b).unwrap().to_bytes(), b);
}

// An opened entry of another kind is `seal/not_control`.
// spec: SEAL-BKP-010
#[test]
fn from_accepted_refuses_other_kinds() {
    let mut x = Hist::new(1);
    let f = x.b.cmd(Who::D(0), &Spec::default());
    let r = x.b.push(f.clone());
    assert!(matches!(r.class, Class::Accept));
    let o = r.opened.as_ref().unwrap();
    assert_eq!(HistoryRecord::from_accepted(2, &f, o).err(), Some(SealError::NotControl));
    let f = x.b.beat(Who::D(0), vec![1], &Spec::default());
    let r = x.b.push(f.clone());
    assert_eq!(HistoryRecord::from_accepted(3, &f, r.opened.as_ref().unwrap()).err(), Some(SealError::NotControl));
}

// `from_bytes` reads exactly one record: a short one, a trailing byte, an
// items length over 262,144 or over the bytes left are `seal/backup_history`.
// spec: SEAL-BKP-002
// spec: SEAL-GEN-008
#[test]
fn from_bytes_exactly_one_record() {
    let x = Hist::new(1);
    let b = x.recs[0].to_bytes();
    for bad in [&b[..b.len() - 1], &b[..155], &[][..], &[b.clone(), vec![0]].concat()[..]] {
        assert_eq!(HistoryRecord::from_bytes(bad).err(), Some(SealError::BackupHistory));
    }
    let mut big = b.clone();
    big[88..92].copy_from_slice(&262_145u32.to_be_bytes());
    assert_eq!(HistoryRecord::from_bytes(&big).err(), Some(SealError::BackupHistory));
    let mut huge = b.clone();
    huge[88..92].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(HistoryRecord::from_bytes(&huge).err(), Some(SealError::BackupHistory));
    // At the limit: 262,144 bytes of items parse as a record.
    let mut at = b[..88].to_vec();
    at.extend_from_slice(&262_144u32.to_be_bytes());
    at.extend(std::iter::repeat_n(b'x', 262_144));
    at.extend_from_slice(&[0u8; 64]);
    assert!(HistoryRecord::from_bytes(&at).is_ok());
}

// ------------------------------------------------------------- writing

// A nightly file is laid out as §21.2: magic, ver, suite, file_type,
// board, writer, its key, recipient (`dev_R` of the pinned key), seq,
// written_ms, enc, ct_len, ct, sig; the signature is the writer's over
// `ctx("backup") ‖ H(file body)`; the ciphertext opens with HPKE base mode
// under the recovery key with the info and aad of §21.3, to raw DEFLATE of
// the payload of §21.5.
// spec: SEAL-BKP-001
// spec: SEAL-SIG-006
// spec: SEAL-BKP-002
#[test]
fn the_file_layout() {
    use hpke::Deserializable as _;
    let mut x = Hist::new(2);
    x.rotate(Who::D(1));
    let file = x.write(0);
    let n = file.len();
    let w = x.b.dev_pub(0);
    let rp = x.rec_keys().public();
    assert_eq!(&file[0..4], b"TDBK");
    assert_eq!(&file[4..7], &[1, 1, 1]);
    assert_eq!(&file[7..23], &x.b.board.0);
    assert_eq!(&file[23..39], &w.dev.0);
    assert_eq!(&file[39..71], &w.sign_pk);
    assert_eq!(&file[71..87], &rp.dev(&x.b.board).0);
    assert_eq!(&file[87..95], &x.b.seq().to_be_bytes());
    assert_eq!(&file[95..103], &1_767_225_600_123i64.to_be_bytes());
    let ct_len = u64::from_be_bytes(file[135..143].try_into().unwrap()) as usize;
    assert_eq!(n, 207 + ct_len);
    // §16.3.
    let signed = [ctx::BACKUP, &h(&[&file[..143 + ct_len]])].concat();
    let sig = ed25519_dalek::Signature::from_bytes(file[n - 64..].try_into().unwrap());
    ed25519_dalek::VerifyingKey::from_bytes(&w.sign_pk).unwrap().verify_strict(&signed, &sig).unwrap();
    // §21.3.
    let info = [ctx::BACKUP_HPKE, &x.b.board.0, &x.b.seq().to_be_bytes(), &w.dev.0, &rp.dev(&x.b.board).0, &rp.hpke_pk].concat();
    let aad = [ctx::BACKUP_AAD, &file[..143]].concat();
    let sk = <Kem as hpke::Kem>::PrivateKey::from_bytes(x.rec_keys().hpke_secret().as_bytes()).unwrap();
    let enc = <Kem as hpke::Kem>::EncappedKey::from_bytes(&file[103..135]).unwrap();
    let pt = hpke::single_shot_open::<HpkeAead, Kdf, Kem>(&hpke::OpModeR::Base, &sk, &enc, &info, &file[143..n - 64], &aad).unwrap();
    let mut payload = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::DeflateDecoder::new(&pt[..]), &mut payload).unwrap();
    // §21.5, nightly.
    let recs = x.recs();
    let mut want = b"TDBP".to_vec();
    want.extend_from_slice(&1u32.to_be_bytes());
    want.push(1);
    want.extend_from_slice(&(recs.len() as u32).to_be_bytes());
    for r in &recs {
        want.extend_from_slice(&r.to_bytes());
    }
    let m = x.b.m.to_canonical_bytes();
    want.extend_from_slice(&(m.len() as u32).to_be_bytes());
    want.extend_from_slice(&m);
    let board = b"{\"board\":1}";
    want.extend_from_slice(&(board.len() as u32).to_be_bytes());
    want.extend_from_slice(board);
    want.extend_from_slice(&x.b.head().chain.0);
    want.extend_from_slice(&1u32.to_be_bytes());
    want.extend_from_slice(&state_hash(1, &h(&[board]), &m));
    want.extend_from_slice(&2u16.to_be_bytes());
    for (e, r) in &x.b.roots {
        want.extend_from_slice(&e.to_be_bytes());
        want.extend_from_slice(&r.to_bytes()[..]);
    }
    want.extend_from_slice(&5u16.to_be_bytes());
    want.extend_from_slice(b"tok-1");
    want.extend_from_slice(&7u32.to_be_bytes());
    want.extend_from_slice(b"{\"m\":1}");
    assert_eq!(payload, want);
}

// A nightly file and a segment read back with every field; the history of
// the file is exactly the membership's `ctl_seqs`, in seq order.
// spec: SEAL-BKP-001
// spec: SEAL-BKP-002
// spec: SEAL-BKP-003
#[test]
fn write_then_read() {
    let mut x = Hist::new(2);
    x.cmd(Who::D(1));
    x.revoke(Who::D(0), 1);
    x.cmd(Who::D(0));
    let file = x.write(0);
    let v = x.read(&file).unwrap();
    assert_eq!(v.writer, x.b.dev_pub(0));
    assert_eq!((v.seq, v.file_type), (x.b.seq(), 1));
    // `written_ms` as the header carries it (§26).
    assert_eq!(v.written_ms, 1_767_225_600_123);
    assert_eq!(&file[95..103], &v.written_ms.to_be_bytes());
    assert_eq!(v.read_token.as_deref(), Some("tok-1"));
    assert_eq!(v.meta, b"{\"m\":1}");
    assert_eq!(v.roots.iter().map(|(e, r)| (*e, *r.to_bytes())).collect::<Vec<_>>(), x.roots().iter().map(|(e, r)| (*e, *r.to_bytes())).collect::<Vec<_>>());
    match &v.content {
        BackupContent::Nightly { membership, head, hash_version, .. } => {
            assert_eq!(membership, &x.b.m.to_canonical_bytes());
            assert_eq!((*head, *hash_version), (x.b.head(), 1));
        }
        BackupContent::Segment { .. } => panic!("not a segment"),
    }
    assert_eq!(v.projection.ctl_seqs, x.b.m.ctl_seqs);
    assert!(agrees(&v.projection, &x.b.m));

    // A segment: M_{first_seq − 1}, the history to first_seq − 1, the frames
    // first_seq to seq.
    let before = x.b.m.to_canonical_bytes();
    let recs = x.recs();
    let first_seq = x.b.seq() + 1;
    let chain_before = x.b.head().chain;
    let roots = x.roots();
    x.cmd(Who::D(0));
    x.cmd(Who::D(0));
    let frames: Vec<Vec<u8>> = x.b.frames[first_seq as usize - 1..].to_vec();
    let content = BackupContent::Segment { membership: before, first_seq, chain_before, frames: frames.clone() };
    let mut input = x.input(&recs, content);
    input.roots = roots;
    input.read_token = None;
    let file = write_backup(&x.b.devs[0], input, &mut OsEntropy).unwrap();
    assert_eq!(file[6], 2);
    let v = x.read(&file).unwrap();
    assert_eq!((v.seq, v.file_type, v.read_token.is_none()), (x.b.seq(), 2, true));
    match v.content {
        BackupContent::Segment { first_seq: f, chain_before: c, frames: fr, .. } => {
            assert_eq!((f, c, fr), (first_seq, chain_before, frames));
        }
        BackupContent::Nightly { .. } => panic!("not nightly"),
    }
}

// The writer refuses when the membership's current recovery key is not the
// pinned one (V1-2), and draws nothing then.
// spec: SEAL-BKP-006
#[test]
fn unpinned_key_is_refused() {
    let x = Hist::new(1);
    let recs = x.recs();
    let other = RecoverySeed::generate(&mut OsEntropy).unwrap().keys().public();
    for pinned in [other, RecoveryPublic { sign_pk: x.rec_keys().public().sign_pk, hpke_pk: other.hpke_pk }] {
        let mut input = x.input(&recs, x.nightly());
        input.pinned_recovery = pinned;
        assert_eq!(write_backup(&x.b.devs[0], input, &mut OsEntropy).err(), Some(SealError::BackupUnpinned));
    }
}

// The writer refuses inputs its reader would reject: a history other than
// `ctl_seqs`, a writer that is not a current member, a root that does not
// match its commitment or appears twice, a bad token, `meta` over its limit,
// a head other than `seq`, a segment whose frames are not first_seq to seq.
// A root of an epoch with no commitment is left out, not refused.
// spec: SEAL-BKP-002
// spec: SEAL-KEY-015
// spec: SEAL-KEY-013
#[test]
fn writer_refuses_inconsistent_inputs() {
    let mut x = Hist::new(2);
    x.revoke(Who::D(0), 1);
    let recs = x.recs();
    let w = |i: BackupInput, k: usize| write_backup(&x.b.devs[k], i, &mut OsEntropy).err();
    assert_eq!(w(x.input(&recs[..recs.len() - 1], x.nightly()), 0), Some(SealError::BackupHistory));
    let swapped = [dup(&recs[1]), dup(&recs[0]), dup(&recs[2])];
    assert_eq!(w(x.input(&swapped, x.nightly()), 0), Some(SealError::BackupHistory));
    assert_eq!(w(x.input(&recs, x.nightly()), 1), Some(SealError::BackupWriter));
    let stranger = DeviceKeys::generate(&mut OsEntropy).unwrap();
    assert_eq!(write_backup(&stranger, x.input(&recs, x.nightly()), &mut OsEntropy).err(), Some(SealError::BackupWriter));
    let bad = Some(SealError::BackupPayload);
    let mut i = x.input(&recs, x.nightly());
    i.roots.push((1, EpochRoot::from_bytes([9; 32])));
    assert_eq!(w(i, 0), bad);
    let mut i = x.input(&recs, x.nightly());
    i.roots[0].1 = EpochRoot::from_bytes([9; 32]);
    assert_eq!(w(i, 0), bad);
    let mut i = x.input(&recs, x.nightly());
    i.roots.push((3, EpochRoot::from_bytes([9; 32])));
    assert_eq!(w(i, 0), None);
    for t in [String::new(), "x".repeat(513), "é".into()] {
        let mut i = x.input(&recs, x.nightly());
        i.read_token = Some(t);
        assert_eq!(w(i, 0), bad);
    }
    let mut i = x.input(&recs, x.nightly());
    i.read_token = Some("x".repeat(512));
    assert!(w(i, 0).is_none());
    let mut i = x.input(&recs, x.nightly());
    i.meta = vec![b' '; 1_048_577];
    assert_eq!(w(i, 0), bad);
    let mut i = x.input(&recs, x.nightly());
    i.seq += 1;
    assert_eq!(w(i, 0), bad);
    let seg = |frames: usize, first_seq: u64| BackupContent::Segment {
        membership: x.b.m.to_canonical_bytes(),
        first_seq,
        chain_before: x.b.head().chain,
        frames: vec![vec![1u8; 700]; frames],
    };
    let s = x.b.seq();
    let mut i = x.input(&recs, seg(2, s + 1));
    i.seq = s + 2;
    assert!(w(i, 0).is_none());
    for (n, first, last) in [(1, s + 1, s + 2), (3, s + 1, s + 2), (1, 0, 0), (1, s, s)] {
        let mut i = x.input(&recs, seg(n, first));
        i.seq = last;
        assert_eq!(w(i, 0), bad, "{n} {first} {last}");
    }
    let mut i = x.input(&recs, x.nightly());
    i.board = BoardId([1; 16]);
    assert!(w(i, 0).is_some());
    // Membership bytes that do not parse.
    let mut i = x.input(&recs, x.nightly());
    if let BackupContent::Nightly { membership, .. } = &mut i.content {
        membership.push(0);
    }
    assert_eq!(w(i, 0), Some(SealError::MembershipFormat));
}

// The writer draws exactly the 32 bytes of the HPKE ephemeral `ikm` and
// keeps nothing: the same inputs and entropy give the same file; the file
// holds neither a root nor the plaintext payload; a failing source is
// `seal/entropy`.
// spec: SEAL-BKP-007
// spec: SEAL-GEN-004
#[cfg(feature = "test-entropy")]
#[test]
fn writer_draws_32_bytes_and_keeps_nothing() {
    use crate::FixedEntropy;
    let x = Hist::new(1);
    let recs = x.recs();
    let a = write_backup(&x.b.devs[0], x.input(&recs, x.nightly()), &mut FixedEntropy::new(vec![7; 32])).unwrap();
    let b = write_backup(&x.b.devs[0], x.input(&recs, x.nightly()), &mut FixedEntropy::new(vec![7; 32])).unwrap();
    assert_eq!(a, b);
    let r1 = *x.b.roots[&1].to_bytes();
    assert!(!a.windows(32).any(|w| w == r1));
    assert!(!a.windows(4).any(|w| w == b"TDBP"));
    let e = write_backup(&x.b.devs[0], x.input(&recs, x.nightly()), &mut FixedEntropy::new(vec![7; 31])).err();
    assert_eq!(e, Some(SealError::Entropy));
    // The ephemeral key is DeriveKeyPair(ikm).
    let (_, pk) = crate::prim::hpke_derive(&[7; 32]);
    assert_eq!(&a[103..135], &pk);
}

// ------------------------------------------------------ the steps of §21.6

// Step 1: 222 bytes and 256 MiB + 1 are `seal/backup_size`; 223 passes it.
// spec: SEAL-BKP-003
#[test]
fn step1_size() {
    let x = Hist::new(1);
    let k = x.rec_keys();
    fails(read_backup(&[0u8; 222], &x.b.board, k), SealError::BackupSize);
    fails(read_backup(&vec![0u8; 268_435_457], &x.b.board, k), SealError::BackupSize);
    fails(read_backup(&[0u8; 223], &x.b.board, k), SealError::BackupHeader);
    fails(read_backup(&[], &x.b.board, k), SealError::BackupSize);
}

// Step 2: magic, ver, suite, file_type, and ct_len = size − 207.
// spec: SEAL-BKP-003
#[test]
fn step2_header() {
    let x = Hist::new(1);
    let file = x.write(0);
    for (i, v) in [(0usize, b'X'), (4, 2), (5, 2), (6, 0), (6, 3), (142, file[142] ^ 1)] {
        let mut f = file.clone();
        f[i] = v;
        fails(x.read(&f), SealError::BackupHeader);
    }
    let mut longer = file.clone();
    longer.push(0);
    fails(x.read(&longer), SealError::BackupHeader);
    fails(x.read(&file[..file.len() - 1]), SealError::BackupHeader);
    assert!(x.read(&file).is_ok());
}

// Step 3: the board id is the sheet's, never the file's (V1-13).
// spec: SEAL-BKP-003
// spec: SEAL-BKP-004
#[test]
fn step3_board() {
    let x = Hist::new(1);
    let file = x.write(0);
    fails(read_backup(&file, &BoardId([3; 16]), x.rec_keys()), SealError::BackupBoard);
    let mut f = file.clone();
    f[7] ^= 1;
    fails(x.read(&f), SealError::BackupBoard);
}

// Step 4: `recipient` is `dev_R` of the typed seed: another sheet, or a
// file sealed to another recovery key, is `seal/backup_recipient`.
// spec: SEAL-BKP-003
// spec: SEAL-BKP-004
#[test]
fn step4_recipient() {
    let x = Hist::new(1);
    let file = x.write(0);
    let other = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
    fails(read_backup(&file, &x.b.board, &other), SealError::BackupRecipient);
    // Sealed to another recovery key by a member.
    let mut f = x.fields(&x.b.devs[0], NIGHTLY, x.b.seq());
    f.recipient = other.public().dev(&x.b.board);
    let c = deflate_raw(&x.payload(&x.recs())).unwrap();
    let to_other = seal_file(&x.b.devs[0], &f, &other.public().hpke_pk, &c, &mut OsEntropy).unwrap();
    fails(x.read(&to_other), SealError::BackupRecipient);
    assert!(read_backup(&to_other, &x.b.board, &other).is_err());
}

// Step 5: the writer signature, strictly: a flipped byte of the body or of
// `sig`, a signature by another key, an `S` that is not canonical and an
// `R` of small order (which the non-strict equation accepts) all fail.
// spec: SEAL-BKP-003
// spec: SEAL-SIG-006
// spec: SEAL-GEN-010
#[test]
fn step5_signature() {
    use ed25519_dalek::Verifier as _;
    use sha2::Digest as _;
    let x = Hist::new(2);
    let file = x.write(0);
    let n = file.len();
    for i in [95usize, 103, 120, 143, n - 65, n - 64, n - 33, n - 1] {
        let mut f = file.clone();
        f[i] ^= 1;
        fails(x.read(&f), SealError::BackupSignature);
    }
    let mut f = file.clone();
    resign(&mut f, &x.b.devs[1]);
    fails(x.read(&f), SealError::BackupSignature);
    for f in strict_failures(&file, &x.b.devs[0]) {
        fails(x.read(&f), SealError::BackupSignature);
        assert_eq!(verify_backup_file(&f, &x.b.dev_pub(0).sign_pk).err(), Some(SealError::BackupSignature));
    }
    // The small-order R signature passes the non-strict equation.
    let pk = x.b.dev_pub(0).sign_pk;
    let small = &strict_failures(&file, &x.b.devs[0])[1];
    let msg = [ctx::BACKUP, &sha2::Sha256::digest(&file[..n - 64])[..]].concat();
    let sig = ed25519_dalek::Signature::from_bytes(small[n - 64..].try_into().unwrap());
    assert!(ed25519_dalek::VerifyingKey::from_bytes(&pk).unwrap().verify(&msg, &sig).is_ok());
    assert!(x.read(&file).is_ok());
}

/// The file with its signature replaced by (1) the valid `S` plus the group
/// order, (2) `R` = the identity and `S = k·a`, both over the same body.
pub fn strict_failures(file: &[u8], k: &DeviceKeys) -> Vec<Vec<u8>> {
    use sha2::Digest as _;
    let n = file.len();
    let pk = k.public().sign_pk;
    let msg = [ctx::BACKUP, &sha2::Sha256::digest(&file[..n - 64])[..]].concat();
    let mut high_s = file.to_vec();
    let s: [u8; 32] = file[n - 32..].try_into().unwrap();
    high_s[n - 32..].copy_from_slice(&scalar::add_bytes(&s, &scalar::L));
    let mut a = k.signing_key().to_scalar_bytes();
    a[0] &= 248;
    a[31] &= 127;
    a[31] |= 64;
    let mut r = [0u8; 32];
    r[0] = 1;
    let kk = sha2::Sha512::new().chain_update(r).chain_update(pk).chain_update(&msg).finalize();
    let s = scalar::to_bytes(&scalar::mul(&scalar::reduce(&kk), &scalar::reduce(&a)));
    let mut small_r = file.to_vec();
    small_r[n - 64..n - 32].copy_from_slice(&r);
    small_r[n - 32..].copy_from_slice(&s);
    vec![high_s, small_r]
}

// Step 6: a file re-signed by its writer after a change to `ct`, or to a
// header field the info or aad binds (seq, written_ms, enc), does not open.
// spec: SEAL-BKP-003
// spec: SEAL-BKP-001
#[test]
fn step6_open() {
    let x = Hist::new(1);
    let file = x.write(0);
    let n = file.len();
    for i in [87usize, 94, 95, 102, 103, 134, 143, n / 2, n - 65] {
        let mut f = file.clone();
        f[i] ^= 1;
        resign(&mut f, &x.b.devs[0]);
        fails(x.read(&f), SealError::BackupOpen);
    }
}

// Step 7: a stream that is not raw DEFLATE, one cut short, and one followed
// by any byte fail with `seal/inflate`; the cap is 512 MiB, and more output
// than the cap fails while streaming.
// spec: SEAL-BKP-003
#[test]
fn step7_inflate() {
    let x = Hist::new(1);
    let w = &x.b.devs[0];
    let good = deflate_raw(&x.payload(&x.recs())).unwrap();
    assert!(x.read(&x.seal_compressed(w, NIGHTLY, x.b.seq(), &good)).is_ok());
    let trailing = [&good[..], &[0u8]].concat();
    let cut = &good[..good.len() - 1];
    for c in [&trailing[..], cut, &[0xFF; 40][..], &[][..]] {
        fails(x.read(&x.seal_compressed(w, NIGHTLY, x.b.seq(), c)), SealError::Inflate);
    }
    assert_eq!(INFLATE_CAP, 536_870_912);
    let zeros = deflate_raw(&[0u8; 100_000]).unwrap();
    assert_eq!(inflate_raw(&zeros, 100_000).map(|v| v.len()), Some(100_000));
    assert!(inflate_raw(&zeros, 99_999).is_none());
    assert!(inflate_raw(&[&zeros[..], &[0]].concat(), 100_000).is_none());
}

// Step 8: magic, version, file_type, n_hist (1 to 1,048,576, against the
// bytes left) and the records alone, each length checked before reading.
// spec: SEAL-BKP-003
// spec: SEAL-GEN-008
#[test]
fn step8_payload_head() {
    let x = Hist::new(1);
    let w = &x.b.devs[0];
    let p = x.payload(&x.recs());
    let mut cases: Vec<Vec<u8>> = Vec::new();
    for (i, v) in [(0usize, b'X'), (7, 2), (8, 2), (8, 1 ^ 3)] {
        let mut q = p.to_vec();
        q[i] = v;
        cases.push(q);
    }
    for n in [0u32, 100_000, 1_048_577, u32::MAX] {
        let mut q = p.to_vec();
        q[9..13].copy_from_slice(&n.to_be_bytes());
        cases.push(q);
    }
    let mut q = p.to_vec();
    q[13 + 88..13 + 92].copy_from_slice(&u32::MAX.to_be_bytes());
    cases.push(q);
    cases.push(p[..20].to_vec());
    cases.push(Vec::new());
    for q in cases {
        fails(x.read(&x.seal_payload(w, &q)), SealError::BackupPayload);
    }
}

// Step 9: a history that fails the projection is `seal/backup_history`,
// and so is a record after the file's seq.
// spec: SEAL-BKP-003
// spec: SEAL-BKP-008
#[test]
fn step9_history() {
    let mut x = Hist::new(2);
    x.rotate(Who::D(1));
    let w = &x.b.devs[0];
    let mut recs = x.recs();
    recs[1].sig[0] ^= 1;
    fails(x.read(&x.seal_payload(w, &x.payload(&recs))), SealError::BackupHistory);
    let mut recs = x.recs();
    recs.swap(1, 2);
    fails(x.read(&x.seal_payload(w, &x.payload(&recs))), SealError::BackupHistory);
    // A record after the header's seq (the history itself is sound).
    let recs = x.recs();
    let f = x.seal_compressed(w, NIGHTLY, recs[2].seq - 1, &deflate_raw(&x.payload(&recs)).unwrap());
    fails(x.read(&f), SealError::BackupHistory);
}

// Step 9, a history forged by someone who is no member: an attacker with
// its own recovery keys X and device A writes, for the victim's board id, a
// Genesis with its own salt and X (epoch 1, wraps to A and to dev_R(X)),
// signed by X as recovery signer for the victim's board, then a direct
// recovery-key change by X to the victim's recovery key with a rotation
// wrapped to A and the victim's dev_R; the rest of the payload agrees with
// that history, it is sealed to the victim's recovery key and signed by A.
// Only the Genesis `board_id` check (§6.2: the victim's board id is not
// H(X, salt)) rejects it.
// spec: SEAL-BKP-008
// spec: SEAL-BKP-004
// spec: SEAL-KEY-017
#[test]
fn step9_history_forged_by_a_stranger() {
    use crate::membership::transitions::{self, Auth, EntryCx};
    use crate::payloads::items_bytes;
    use crate::{EpochRotated, Signer, Genesis, GenesisDevice, RecoveryKeyChange, RecoveryKeyMode, wrap_root};

    let victim = Hist::new(2);
    let bv = victim.b.board;
    let rv = victim.rec_keys().public();
    let xk = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
    let xp = xk.public();
    let dev_x = xp.dev(&bv);
    let a = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let ap = a.public();
    let at_ms = 1_767_225_600_000;
    let wraps = |root: &EpochRoot, epoch: u32, rc: &mut Vec<(DevId, [u8; 32])>| {
        let commit = root.commitment(&bv, epoch);
        rc.sort();
        let w: Vec<_> =
            rc.iter().map(|(id, pk)| wrap_root(root, &bv, epoch, &commit, *id, pk, &mut OsEntropy).unwrap()).collect();
        (commit, w)
    };
    let r1 = EpochRoot::from_bytes(rand());
    let (c1, w1) = wraps(&r1, 1, &mut vec![(ap.dev, ap.hpke_pk), (dev_x, xp.hpke_pk)]);
    let g = Genesis {
        salt: rand(),
        recovery: xp,
        device: GenesisDevice { dev: ap.dev, sign_pk: ap.sign_pk, hpke_pk: ap.hpke_pk, name: "A".into() },
        epoch: 1,
        commit: c1,
        wraps: w1,
        rules: 1,
        hash_version: 1,
        state_hash: [0x33; 32],
        snapshot: None,
    };
    assert_ne!(crate::board_id(&xp.sign_pk, &g.salt), bv);
    let r2 = EpochRoot::from_bytes(rand());
    let (c2, w2) = wraps(&r2, 2, &mut vec![(ap.dev, ap.hpke_pk), (rv.dev(&bv), rv.hpke_pk)]);
    let change = RecoveryKeyChange { mode: RecoveryKeyMode::Direct, sign_pk: Some(rv.sign_pk), hpke_pk: Some(rv.hpke_pk) };
    let rot = EpochRotated { epoch: 2, commit: c2, wraps: w2, purge_before: None };
    let (_, sk) = signer_identity(&Signer::Recovery(&xk, bv), &bv).unwrap();
    let record = |seq: u64, items: &[CtlItem]| {
        let (_, items) = items_bytes(items).unwrap();
        let entry_id = EntryId(rand());
        let inner = ctl_inner(&bv, &dev_x, seq, &entry_id, at_ms, &items);
        HistoryRecord { seq, dev: dev_x, ctr: seq, entry_id, at_ms, frame_hash: FrameHash(rand()), items, sig: sign_with(sk, &[&inner]) }
    };
    let recs = vec![
        record(1, &[CtlItem::Genesis(g.clone())]),
        record(2, &[CtlItem::RecoveryKeyChange(change.clone()), CtlItem::EpochRotated(rot.clone())]),
    ];
    // The membership that history leads to, by the rows of §13.3 and §13.6.
    let mut m = Membership::genesis(bv, &g, dev_x, at_ms);
    m.set_last_ctr(&dev_x, 1);
    let none = |_: u64| -> Option<&[u8]> { None };
    let cx = EntryCx { s: 2, at_ms, tau: at_ms, author: Auth::Recovery(dev_x), board: bv, skip_targets: &none };
    transitions::recovery_key_change(&mut m, &change, &cx, false).unwrap();
    transitions::rotate(&mut m, &rot, &cx).unwrap();
    m.set_last_ctr(&dev_x, 2);
    m.ctl_seqs.push(2);
    // The membership bytes carry the victim's salt and recovery records, so
    // that they parse (§13.2 binds the first recovery key and the salt to
    // `board_id`) and still agree with the projection on what step 11
    // compares: the check that stops the file is the projection's.
    m.salt = victim.b.salt;
    m.recovery = victim.b.m.recovery.iter().map(|r| r.dup()).collect();
    let membership = m.to_canonical_bytes();
    assert!(Membership::from_canonical_bytes(&membership).is_ok());
    let board = b"{}".to_vec();
    let state_hash = state_hash(1, &h(&[&board]), &membership);
    let content = BackupContent::Nightly { membership, board, head: Head { seq: 2, chain: Chain(rand()) }, hash_version: 1, state_hash };
    let p = encode_payload(&recs, &content, &[(1, r1), (2, r2)], b"", b"{}").unwrap();
    let forged = victim.seal_compressed(&a, NIGHTLY, 2, &deflate_raw(&p).unwrap());
    fails(victim.read(&forged), SealError::BackupHistory);
}

// Step 10: a forged file (sealed to the recovery public key, signed by a
// key that is no member) and a file by a revoked writer are
// `seal/backup_writer`; so is a member's file sealed to a recovery key the
// projection has retired.
// spec: SEAL-BKP-003
// spec: SEAL-BKP-004
#[test]
fn step10_writer() {
    let mut x = Hist::new(2);
    let stranger = DeviceKeys::generate(&mut OsEntropy).unwrap();
    let forged = x.seal_payload(&stranger, &x.payload(&x.recs()));
    fails(x.read(&forged), SealError::BackupWriter);
    // A member's key under another member's `dev`.
    let mut f = x.fields(&x.b.devs[0], NIGHTLY, x.b.seq());
    f.writer_sign_pk = x.b.dev_pub(1).sign_pk;
    let c = deflate_raw(&x.payload(&x.recs())).unwrap();
    let mixed = seal_file(&x.b.devs[1], &f, &x.rec_keys().public().hpke_pk, &c, &mut OsEntropy).unwrap();
    fails(x.read(&mixed), SealError::BackupWriter);
    // Revoked.
    x.revoke(Who::D(0), 1);
    let by_revoked = x.seal_payload(&x.b.devs[1], &x.payload(&x.recs()));
    fails(x.read(&by_revoked), SealError::BackupWriter);
    // Sealed to the old recovery key after a change, read with the old sheet.
    let mut y = Hist::new(1);
    let old_public = y.rec_keys().public();
    y.change_recovery();
    let mut fy = y.fields(&y.b.devs[0], NIGHTLY, y.b.seq());
    fy.recipient = old_public.dev(&y.b.board);
    let c = deflate_raw(&y.payload(&y.recs())).unwrap();
    let to_old = seal_file(&y.b.devs[0], &fy, &old_public.hpke_pk, &c, &mut OsEntropy).unwrap();
    fails(read_backup(&to_old, &y.b.board, &y.b.rec[0]), SealError::BackupWriter);
    assert!(y.read(&y.write(0)).is_ok());
}

// Nothing of the payload after the history is parsed before step 10: a
// file whose rest is garbage fails at step 9 or 10 when they fail, and only
// at step 11 when they pass.
// spec: SEAL-BKP-005
// spec: SEAL-GEN-009
#[test]
fn nothing_after_the_history_before_step_10() {
    let x = Hist::new(1);
    let recs = x.recs();
    let p = x.payload(&recs);
    let head = 13 + recs.iter().map(|r| r.to_bytes().len()).sum::<usize>();
    let garbage = [&p[..head], &[0xFF; 50][..]].concat();
    let stranger = DeviceKeys::generate(&mut OsEntropy).unwrap();
    fails(x.read(&x.seal_payload(&stranger, &garbage)), SealError::BackupWriter);
    let mut bad_hist = garbage.clone();
    bad_hist[13 + 100] ^= 1;
    fails(x.read(&x.seal_payload(&x.b.devs[0], &bad_hist)), SealError::BackupHistory);
    fails(x.read(&x.seal_payload(&x.b.devs[0], &garbage)), SealError::BackupPayload);
}

/// A payload built field by field, for step 11.
struct Parts {
    membership: Vec<u8>,
    board: Vec<u8>,
    chain: [u8; 32],
    hash_version: u32,
    state_hash: [u8; 32],
    roots: Vec<(u32, [u8; 32])>,
    n_roots: Option<u16>,
    token: Vec<u8>,
    meta: Vec<u8>,
    meta_len: Option<u32>,
    tail: Vec<u8>,
}

impl Parts {
    fn of(x: &Hist) -> Parts {
        let membership = x.b.m.to_canonical_bytes();
        let board = b"{}".to_vec();
        let state_hash = state_hash(1, &h(&[&board]), &membership);
        Parts {
            membership,
            board,
            chain: x.b.head().chain.0,
            hash_version: 1,
            state_hash,
            roots: x.b.roots.iter().map(|(e, r)| (*e, *r.to_bytes())).collect(),
            n_roots: None,
            token: b"t".to_vec(),
            meta: b"{}".to_vec(),
            meta_len: None,
            tail: Vec::new(),
        }
    }

    fn bytes(&self, recs: &[HistoryRecord]) -> Vec<u8> {
        let mut p = b"TDBP".to_vec();
        p.extend_from_slice(&1u32.to_be_bytes());
        p.push(1);
        p.extend_from_slice(&(recs.len() as u32).to_be_bytes());
        for r in recs {
            p.extend_from_slice(&r.to_bytes());
        }
        p.extend_from_slice(&(self.membership.len() as u32).to_be_bytes());
        p.extend_from_slice(&self.membership);
        p.extend_from_slice(&(self.board.len() as u32).to_be_bytes());
        p.extend_from_slice(&self.board);
        p.extend_from_slice(&self.chain);
        p.extend_from_slice(&self.hash_version.to_be_bytes());
        p.extend_from_slice(&self.state_hash);
        p.extend_from_slice(&self.n_roots.unwrap_or(self.roots.len() as u16).to_be_bytes());
        for (e, r) in &self.roots {
            p.extend_from_slice(&e.to_be_bytes());
            p.extend_from_slice(r);
        }
        p.extend_from_slice(&(self.token.len() as u16).to_be_bytes());
        p.extend_from_slice(&self.token);
        p.extend_from_slice(&self.meta_len.unwrap_or(self.meta.len() as u32).to_be_bytes());
        p.extend_from_slice(&self.meta);
        p.extend_from_slice(&self.tail);
        p
    }
}

// Step 11: the rest of the payload, strictly. The membership must parse
// and agree with the projection; every root must match its commitment; the
// state hash of a nightly file is recomputed; the layout has no trailing
// byte, ordered roots, a token of at most 512 ASCII bytes and `meta` of at
// most 1,048,576 bytes. Each is `seal/backup_payload`.
// spec: SEAL-BKP-003
// spec: SEAL-KEY-013
// spec: SEAL-KEY-015
// spec: SEAL-GEN-008
#[test]
fn step11_rest() {
    let mut x = Hist::new(2);
    let older = x.b.m.to_canonical_bytes();
    x.rotate(Who::D(0));
    let w = &x.b.devs[0];
    let recs = x.recs();
    let read = |p: &Parts| x.read(&x.seal_payload(w, &p.bytes(&recs)));
    assert!(read(&Parts::of(&x)).is_ok());
    let mut cases: Vec<Parts> = Vec::new();
    // The membership: not canonical; an older state (other commitments and
    // ctl_seqs); one with another device status.
    let mut p = Parts::of(&x);
    p.membership.push(0);
    cases.push(p);
    let mut p = Parts::of(&x);
    p.membership = older;
    p.state_hash = state_hash(1, &h(&[&p.board]), &p.membership);
    cases.push(p);
    {
        let mut m = x.b.m.dup();
        m.devices[0].status = crate::membership::DeviceStatus::Revoked;
        let mut p = Parts::of(&x);
        p.membership = m.to_canonical_bytes();
        p.state_hash = state_hash(1, &h(&[&p.board]), &p.membership);
        cases.push(p);
        let mut m = x.b.m.dup();
        m.devices[1].hpke_pk[0] ^= 1;
        let mut p = Parts::of(&x);
        p.membership = m.to_canonical_bytes();
        p.state_hash = state_hash(1, &h(&[&p.board]), &p.membership);
        cases.push(p);
        // Each differing from the projection in one thing alone: one
        // device's sign_pk; the current recovery key; ctl_seqs.
        let mut m = x.b.m.dup();
        m.devices[1].sign_pk = DeviceKeys::generate(&mut OsEntropy).unwrap().public().sign_pk;
        let mut p = Parts::of(&x);
        p.membership = m.to_canonical_bytes();
        p.state_hash = state_hash(1, &h(&[&p.board]), &p.membership);
        cases.push(p);
        let mut m = x.b.m.dup();
        let fresh = RecoverySeed::generate(&mut OsEntropy).unwrap().keys().public();
        let cur = m.recovery.iter_mut().find(|r| r.until_seq == 0).unwrap();
        cur.hpke_pk = fresh.hpke_pk;
        let mut p = Parts::of(&x);
        p.membership = m.to_canonical_bytes();
        p.state_hash = state_hash(1, &h(&[&p.board]), &p.membership);
        cases.push(p);
        let mut m = x.b.m.dup();
        let last = *m.ctl_seqs.last().unwrap();
        m.ctl_seqs.push(last + 1);
        let mut p = Parts::of(&x);
        p.membership = m.to_canonical_bytes();
        p.state_hash = state_hash(1, &h(&[&p.board]), &p.membership);
        cases.push(p);
    }
    // The roots: one that does not match; one of an epoch with no
    // commitment; out of order; a count beyond the bytes.
    let mut p = Parts::of(&x);
    p.roots[1].1[0] ^= 1;
    cases.push(p);
    let mut p = Parts::of(&x);
    p.roots.push((3, [1; 32]));
    cases.push(p);
    let mut p = Parts::of(&x);
    p.roots.swap(0, 1);
    cases.push(p);
    let mut p = Parts::of(&x);
    p.n_roots = Some(u16::MAX);
    cases.push(p);
    // The state hash, its version.
    let mut p = Parts::of(&x);
    p.state_hash[0] ^= 1;
    cases.push(p);
    let mut p = Parts::of(&x);
    p.hash_version = 2;
    p.state_hash = state_hash(2, &h(&[&p.board]), &p.membership);
    cases.push(p);
    let mut p = Parts::of(&x);
    p.board.push(b' ');
    cases.push(p);
    // The token, meta and trailing bytes.
    let mut p = Parts::of(&x);
    p.token = vec![b'a'; 513];
    cases.push(p);
    let mut p = Parts::of(&x);
    p.token = vec![0xC3, 0xA9];
    cases.push(p);
    let mut p = Parts::of(&x);
    p.meta = vec![b' '; 1_048_577];
    cases.push(p);
    let mut p = Parts::of(&x);
    p.meta_len = Some(u32::MAX);
    cases.push(p);
    let mut p = Parts::of(&x);
    p.tail = vec![0];
    cases.push(p);
    for (i, p) in cases.iter().enumerate() {
        match read(p) {
            Err(e) => assert_eq!(e, SealError::BackupPayload, "case {i}"),
            Ok(_) => panic!("case {i} read"),
        }
    }
    // At the limits: a 512-byte token, 1,048,576 bytes of meta, no token.
    let mut p = Parts::of(&x);
    p.token = vec![b'a'; 512];
    p.meta = vec![b' '; 1_048_576];
    assert!(read(&p).is_ok());
    let mut p = Parts::of(&x);
    p.token = Vec::new();
    assert!(read(&p).unwrap().read_token.is_none());
}

// Step 11 for a segment: `n_frames` is `seq − first_seq + 1`, each frame
// within 262,144 bytes, and the history ends before `first_seq`.
// spec: SEAL-BKP-003
// spec: SEAL-BKP-002
#[test]
fn step11_segment() {
    let mut x = Hist::new(1);
    let recs = x.recs();
    let before = x.b.m.to_canonical_bytes();
    let first_seq = x.b.seq() + 1;
    let chain_before = x.b.head().chain;
    x.cmd(Who::D(0));
    x.cmd(Who::D(0));
    let frames: Vec<Vec<u8>> = x.b.frames[first_seq as usize - 1..].to_vec();
    let seg = |frames: Vec<Vec<u8>>, first_seq: u64| BackupContent::Segment {
        membership: before.clone(),
        first_seq,
        chain_before,
        frames,
    };
    let build = |c: &BackupContent, recs: &[HistoryRecord], last: u64| {
        let p = encode_payload(recs, c, &x.roots(), b"", b"{}").unwrap();
        x.seal_compressed(&x.b.devs[0], SEGMENT, last, &deflate_raw(&p).unwrap())
    };
    let last = x.b.seq();
    assert!(x.read(&build(&seg(frames.clone(), first_seq), &recs, last)).is_ok());
    let bad = [
        build(&seg(frames[..1].to_vec(), first_seq), &recs, last),
        build(&seg(frames.clone(), 0), &recs, last),
        build(&seg(frames.clone(), first_seq), &recs, first_seq - 1),
        build(&seg(vec![frames[0].clone(), vec![0; 262_145]], first_seq), &recs, last),
        build(&seg(frames[1..].to_vec(), first_seq + 1), &recs, last + 1),
    ];
    for (i, f) in bad.iter().enumerate() {
        match x.read(f) {
            Err(e) => assert_eq!(e, SealError::BackupPayload, "case {i}"),
            Ok(_) => panic!("case {i} read"),
        }
    }
    // The history reaches first_seq (one frame at seq 1, the history's own).
    let f = build(&seg(frames[1..].to_vec(), recs[0].seq), &recs, recs[0].seq);
    fails(x.read(&f), SealError::BackupPayload);
}

// ------------------------------------------------ verify_backup_file

// Steps 1, 2 and 5 only, under the given key: a good file gives its header;
// a re-signed file, a header naming another key and a truncated file fail;
// a file whose ciphertext is garbage but signed passes (nothing of `ct` is
// opened or parsed).
// spec: SEAL-BKP-009
#[test]
fn verify_backup_file_steps() {
    let x = Hist::new(2);
    let file = x.write(0);
    let pk = x.b.dev_pub(0).sign_pk;
    let hdr = verify_backup_file(&file, &pk).unwrap();
    assert_eq!(hdr.file_type, 1);
    assert_eq!(hdr.board, x.b.board);
    assert_eq!((hdr.writer_dev, hdr.writer_sign_pk), (x.b.dev_pub(0).dev, pk));
    assert_eq!(hdr.recipient, x.rec_keys().public().dev(&x.b.board));
    assert_eq!((hdr.seq, hdr.written_ms, hdr.ct_len), (x.b.seq(), 1_767_225_600_123, (file.len() - 207) as u64));
    let mut re = file.clone();
    resign(&mut re, &x.b.devs[1]);
    assert_eq!(verify_backup_file(&re, &pk).err(), Some(SealError::BackupSignature));
    // The header names key B, the file is signed by the given key A: only
    // the comparison of the header's key rejects it.
    let mut fb = x.fields(&x.b.devs[0], NIGHTLY, x.b.seq());
    fb.writer_sign_pk = x.b.dev_pub(1).sign_pk;
    let names_b = seal_file(&x.b.devs[0], &fb, &x.rec_keys().public().hpke_pk, &[0xFF; 64], &mut OsEntropy).unwrap();
    assert!(check_signature(&names_b, &pk).is_ok());
    assert_eq!(verify_backup_file(&names_b, &pk).err(), Some(SealError::BackupSignature));
    // Written and signed by another key B, checked under A.
    let by_other = x.write(1);
    assert_eq!(verify_backup_file(&by_other, &pk).err(), Some(SealError::BackupSignature));
    assert!(verify_backup_file(&by_other, &x.b.dev_pub(1).sign_pk).is_ok());
    assert_eq!(verify_backup_file(&file[..file.len() - 1], &pk).err(), Some(SealError::BackupHeader));
    assert_eq!(verify_backup_file(&file[..222], &pk).err(), Some(SealError::BackupSize));
    // The steps run in order: size, header, then the key and signature.
    let mut hdr_bad = file.clone();
    hdr_bad[4] = 9;
    assert_eq!(verify_backup_file(&hdr_bad, &[0; 32]).err(), Some(SealError::BackupHeader));
    let garbage = x.seal_compressed(&x.b.devs[0], NIGHTLY, 9, &[0xFF; 64]);
    assert!(verify_backup_file(&garbage, &pk).is_ok());
    assert!(x.read(&garbage).is_err());
}

// --------------------------------------------- task S5: carried rulings

// Step 10 compares the keys, not only `dev_R` (ruling Q45): two devices
// propose and confirm a recovery key made of a new sheet's signing key and
// a third party's HPKE key; a current device seals a file to the new
// sheet's real HPKE key. Every step up to 9 passes and the projection's
// current `dev_R` is the recipient, but its HPKE key is not the sheet's:
// reading with the new sheet's seed is `seal/backup_writer`.
// spec: SEAL-BKP-004
#[test]
fn step10_compares_the_recovery_keys() {
    use crate::{RecoveryKeyChange, RecoveryKeyMode};
    let mut x = Hist::new(2);
    let sheet = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
    let third = RecoverySeed::generate(&mut OsEntropy).unwrap().keys();
    let mixed = RecoveryPublic { sign_pk: sheet.public().sign_pk, hpke_pk: third.public().hpke_pk };
    let change = |mode| CtlItem::RecoveryKeyChange(RecoveryKeyChange { mode, sign_pk: Some(mixed.sign_pk), hpke_pk: Some(mixed.hpke_pk) });
    x.ctl(Who::D(0), vec![change(RecoveryKeyMode::Propose)]);
    x.b.now += 259_200_000;
    let items = x.b.rotate_items(vec![change(RecoveryKeyMode::Confirm)]);
    x.ctl(Who::D(1), items);
    let cur = x.b.m.current_recovery();
    assert_eq!((cur.sign_pk, cur.hpke_pk), (mixed.sign_pk, mixed.hpke_pk));
    // Sealed by device 0 to the sheet's own keys.
    let sp = sheet.public();
    let mut f = x.fields(&x.b.devs[0], NIGHTLY, x.b.seq());
    f.recipient = sp.dev(&x.b.board);
    assert_eq!(f.recipient, cur.dev);
    let c = deflate_raw(&x.payload(&x.recs())).unwrap();
    let file = seal_file(&x.b.devs[0], &f, &sp.hpke_pk, &c, &mut OsEntropy).unwrap();
    fails(read_backup(&file, &x.b.board, &sheet), SealError::BackupWriter);
    // The writer refuses to seal to a key the membership does not hold.
    let recs = x.recs();
    let mut i = x.input(&recs, x.nightly());
    i.pinned_recovery = sp;
    assert_eq!(write_backup(&x.b.devs[0], i, &mut OsEntropy).err(), Some(SealError::BackupUnpinned));
    // A file sealed to the membership's own pair is read by no sheet of
    // these: the third party's seed gives another `dev_R` (step 4).
    let mut i = x.input(&recs, x.nightly());
    i.pinned_recovery = mixed;
    let to_mixed = write_backup(&x.b.devs[0], i, &mut OsEntropy).unwrap();
    fails(read_backup(&to_mixed, &x.b.board, &third), SealError::BackupRecipient);
    fails(read_backup(&to_mixed, &x.b.board, &sheet), SealError::BackupOpen);
}

// The first record's counter must be 1 (SEAL-MEM-004, ruling Q46): the
// Genesis record re-signed by the real recovery key with `ctr` 7 is
// `seal/backup_history`.
// spec: SEAL-MEM-004
// spec: SEAL-BKP-008
#[test]
fn the_first_record_counter_is_1() {
    let x = Hist::new(2);
    let mut r = x.recs();
    r[0].ctr = 7;
    let inner = ctl_inner(&x.b.board, &r[0].dev, 7, &r[0].entry_id, r[0].at_ms, &r[0].items);
    r[0].sig = sign_with(x.b.rec[0].signing_key(), &[&inner]);
    fails(x.read(&x.seal_payload(&x.b.devs[0], &x.payload(&r))), SealError::BackupHistory);
    assert!(x.read(&x.seal_payload(&x.b.devs[0], &x.payload(&x.recs()))).is_ok());
}

// `write_backup` leaves out the roots its file's membership has no
// commitment for (ruling Q38): a segment written by a device that holds the
// roots of epochs rotated inside the segment and after it, and a nightly
// file with a root drawn for a rotation not yet accepted, read back with
// only the roots that have commitments. A root that does not match its
// commitment, and two roots for one epoch (even an epoch with no
// commitment), stay `seal/backup_payload`.
// spec: SEAL-BKP-002
// spec: SEAL-KEY-013
#[test]
fn roots_without_a_commitment_are_left_out() {
    let epochs = |v: &VerifiedBackup| v.roots.iter().map(|(e, _)| *e).collect::<Vec<_>>();
    let mut x = Hist::new(2);
    x.rotate(Who::D(0));
    let recs = x.recs();
    let before = x.b.m.to_canonical_bytes();
    let first_seq = x.b.seq() + 1;
    let chain_before = x.b.head().chain;
    x.cmd(Who::D(0));
    x.rotate(Who::D(1));
    x.cmd(Who::D(1));
    x.rotate(Who::D(0));
    assert_eq!(x.b.m.epoch, 4);
    let frames: Vec<Vec<u8>> = x.b.frames[first_seq as usize - 1..].to_vec();
    let content = BackupContent::Segment { membership: before, first_seq, chain_before, frames };
    let input = x.input(&recs, content);
    assert_eq!(input.roots.len(), 4);
    let file = write_backup(&x.b.devs[0], input, &mut OsEntropy).unwrap();
    let v = x.read(&file).unwrap();
    assert_eq!(epochs(&v), [1, 2]);
    // Nightly, with a root for epoch 5 that no accepted rotation committed.
    let recs = x.recs();
    let mut i = x.input(&recs, x.nightly());
    i.roots.push((5, EpochRoot::from_bytes(rand())));
    let v = x.read(&write_backup(&x.b.devs[0], i, &mut OsEntropy).unwrap()).unwrap();
    assert_eq!(epochs(&v), [1, 2, 3, 4]);
    // No roots at all.
    let mut i = x.input(&recs, x.nightly());
    i.roots.clear();
    let v = x.read(&write_backup(&x.b.devs[0], i, &mut OsEntropy).unwrap()).unwrap();
    assert!(v.roots.is_empty());
    // The two refusals.
    let mut i = x.input(&recs, x.nightly());
    i.roots[3].1 = EpochRoot::from_bytes(rand());
    assert_eq!(write_backup(&x.b.devs[0], i, &mut OsEntropy).err(), Some(SealError::BackupPayload));
    let mut i = x.input(&recs, x.nightly());
    i.roots.push((2, EpochRoot::from_bytes(*x.b.roots[&2].to_bytes())));
    assert_eq!(write_backup(&x.b.devs[0], i, &mut OsEntropy).err(), Some(SealError::BackupPayload));
    let mut i = x.input(&recs, x.nightly());
    i.roots.push((9, EpochRoot::from_bytes(rand())));
    i.roots.push((9, EpochRoot::from_bytes(rand())));
    assert_eq!(write_backup(&x.b.devs[0], i, &mut OsEntropy).err(), Some(SealError::BackupPayload));
}

// The epoch roots and the read token never pass through a DEFLATE
// compressor (ruling Q48, §21.3): of every byte the writer hands to a
// compressor, none is a root or the token, while the history and `meta` are
// among them; the file reads back with the roots and the token.
// spec: SEAL-GEN-006
#[test]
fn the_roots_never_reach_the_compressor() {
    use crate::prim::COMPRESSOR_INPUT;
    let mut x = Hist::new(2);
    x.rotate(Who::D(1));
    let recs = x.recs();
    let mut i = x.input(&recs, x.nightly());
    i.read_token = Some("rt1.secret-read-token-0123456789".into());
    i.meta = b"{\"meta\":\"visible\"}".to_vec();
    COMPRESSOR_INPUT.with(|c| c.borrow_mut().clear());
    let file = write_backup(&x.b.devs[0], i, &mut OsEntropy).unwrap();
    let seen = COMPRESSOR_INPUT.with(|c| std::mem::take(&mut *c.borrow_mut()));
    for r in x.b.roots.values() {
        let root = *r.to_bytes();
        assert!(!seen.windows(32).any(|w| w == root));
    }
    assert!(!seen.windows(10).any(|w| w == b"rt1.secret"));
    assert!(seen.windows(4).any(|w| w == b"TDBP"));
    assert!(seen.windows(recs[1].to_bytes().len()).any(|w| w == recs[1].to_bytes()));
    assert!(seen.windows(7).any(|w| w == b"visible"));
    let v = x.read(&file).unwrap();
    assert_eq!(v.roots.len(), 2);
    assert_eq!(v.read_token.as_deref(), Some("rt1.secret-read-token-0123456789"));
    assert_eq!(v.meta, b"{\"meta\":\"visible\"}");
}

// The compressed payload of a file is one raw DEFLATE stream of the whole
// payload, which any inflater reads: here flate2's reader and the crate's,
// for files with no roots, with two, and with no token.
// spec: SEAL-BKP-001
// spec: SEAL-BKP-002
#[test]
fn the_compressed_payload_is_one_stream() {
    use hpke::Deserializable as _;
    let mut x = Hist::new(2);
    x.rotate(Who::D(1));
    let recs = x.recs();
    for (roots, token) in [(true, true), (false, true), (true, false), (false, false)] {
        let mut i = x.input(&recs, x.nightly());
        if !roots {
            i.roots.clear();
        }
        if !token {
            i.read_token = None;
        }
        let want = encode_payload(&recs, &x.nightly(), &if roots { x.roots() } else { Vec::new() }, if token { b"tok-1" } else { b"" }, b"{\"m\":1}").unwrap();
        let file = write_backup(&x.b.devs[0], i, &mut OsEntropy).unwrap();
        let n = file.len();
        let rp = x.rec_keys().public();
        let info = hpke_info(&x.b.board, x.b.seq(), &x.b.dev_pub(0).dev, &rp.dev(&x.b.board), &rp.hpke_pk);
        let sk = <Kem as hpke::Kem>::PrivateKey::from_bytes(x.rec_keys().hpke_secret().as_bytes()).unwrap();
        let enc = <Kem as hpke::Kem>::EncappedKey::from_bytes(&file[103..135]).unwrap();
        let c = hpke::single_shot_open::<HpkeAead, Kdf, Kem>(&hpke::OpModeR::Base, &sk, &enc, &info, &file[143..n - 64], &hpke_aad(&file[..143])).unwrap();
        let mut got = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::DeflateDecoder::new(&c[..]), &mut got).unwrap();
        assert_eq!(got, want.to_vec());
        assert_eq!(inflate_raw(&c, INFLATE_CAP).unwrap().to_vec(), want.to_vec());
        assert!(x.read(&file).is_ok());
    }
}
