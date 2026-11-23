//! The parsers of task S4 over random and mutated inputs: `open_file`,
//! `read_backup` (on mutated files, and on files sealed here around mutated
//! payloads, so that the parsers after the HPKE layer see them too),
//! `verify_backup_file` and `HistoryRecord::from_bytes`. Each returns, never
//! panics.

use std::sync::OnceLock;

use proptest::prelude::*;
use taskdeck_seal::*;

use crate::backup::*;
use crate::board::*;

struct Fixture {
    sim: Sim,
    key: StorageKey,
    dev: DevId,
    sealed: Vec<u8>,
    backup: Vec<u8>,
    segment: Vec<u8>,
    payload: Vec<u8>,
    segment_payload: Vec<u8>,
    records: Vec<Vec<u8>>,
}

fn fixture() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        let mut s = Sim::new(51, 2);
        s.rotate(Who::D(1));
        let key = StorageKey::from_bytes([0x33; 32]);
        let dev = s.devs[0].public().dev;
        let (mut fs, mut sealed) = FileSealer::create(&key, FileKind::Checkpoint, &s.board, &dev, &mut OsEntropy).unwrap();
        for p in [&b"one"[..], &[0u8; 300][..], b""] {
            sealed.extend(fs.seal_record(p, &mut OsEntropy).unwrap());
        }
        let bytes = s.rng.bytes(32);
        let backup = write(&s, 0, bytes);
        let payload = Payload::nightly(&s).bytes();
        let history = copy_history(&s.history);
        let membership = s.m.to_canonical_bytes();
        let first_seq = s.seq() + 1;
        let chain_before = s.head().chain;
        let rs = roots(&s);
        let mut sp = Payload::nightly(&s);
        let f = s.cmd(Who::D(0), &Spec::default());
        s.accept(f);
        let frames = s.frames[first_seq as usize - 1..].to_vec();
        sp.file_type = 2;
        sp.first_seq = first_seq;
        sp.chain_before = chain_before.0;
        sp.frames = frames.clone();
        let mut i = input(&s, &history, BackupContent::Segment { membership, first_seq, chain_before, frames });
        i.roots = rs;
        let bytes = s.rng.bytes(32);
        let segment = write_backup(&s.devs[0], i, &mut *source(bytes)).unwrap();
        let records = s.history.iter().map(|r| r.to_bytes()).collect();
        Fixture { sim: s, key, dev, sealed, backup, segment, payload, segment_payload: sp.bytes(), records }
    })
}

fn mutate(mut b: Vec<u8>, at: usize, op: u8, x: u8) -> Vec<u8> {
    if b.is_empty() {
        return vec![x];
    }
    let i = at % b.len();
    match op % 5 {
        0 => b[i] = x,
        1 => b.insert(i, x),
        2 => {
            b.remove(i);
        }
        3 => b.truncate(i),
        _ => {
            let end = (i + 4).min(b.len());
            for y in &mut b[i..end] {
                *y = 0xFF;
            }
        }
    }
    b
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    // `open_file` on random bytes and mutated real files: it returns, and
    // never more records than the file can hold.
    // spec: SEAL-GEN-011
    // spec: SEAL-GEN-008
    // spec: SEAL-FILE-003
    #[test]
    fn open_file_never_panics(
        random in proptest::collection::vec(any::<u8>(), 0..800),
        at in any::<usize>(), op in any::<u8>(), x in any::<u8>(), at2 in any::<usize>(), y in any::<u8>(),
    ) {
        let fx = fixture();
        let with_header = [&fx.sealed[..55], &random[..]].concat();
        for input in [random.clone(), with_header, mutate(mutate(fx.sealed.clone(), at, op, x), at2, op.wrapping_add(2), y)] {
            if let Ok(o) = open_file(&fx.key, FileKind::Checkpoint, &fx.sim.board, &fx.dev, &input) {
                prop_assert!(o.records.len() <= input.len().saturating_sub(55) / 44);
            }
        }
    }

    // `read_backup` and `verify_backup_file` on random bytes and mutated
    // files: they return; a mutated file is never read.
    // spec: SEAL-GEN-011
    // spec: SEAL-GEN-008
    #[test]
    fn backup_readers_never_panic(
        random in proptest::collection::vec(any::<u8>(), 0..1000),
        which in any::<bool>(), at in any::<usize>(), op in any::<u8>(), x in any::<u8>(),
    ) {
        let fx = fixture();
        let s = &fx.sim;
        let rk = s.rec.last().unwrap();
        let pk = s.devs[0].public().sign_pk;
        let file = if which { &fx.backup } else { &fx.segment };
        let m = mutate(file.clone(), at, op, x);
        let mut header_kept = file[..143].to_vec();
        header_kept.extend(&random);
        for input in [&random, &m, &header_kept] {
            let r = read_backup(input, &s.board, rk);
            prop_assert!(r.is_err() || input == file);
            let _ = verify_backup_file(input, &pk);
        }
    }

    // `read_backup` on files sealed and signed by a member around mutated
    // payloads (nightly and segment), raw DEFLATE of them and mutated
    // compressed streams: every parser after the HPKE layer returns.
    // spec: SEAL-GEN-011
    // spec: SEAL-GEN-008
    #[test]
    fn backup_payload_parsers_never_panic(
        which in any::<bool>(), at in any::<usize>(), op in any::<u8>(), x in any::<u8>(),
        at2 in any::<usize>(), op2 in any::<u8>(), y in any::<u8>(), squash in any::<bool>(),
    ) {
        let fx = fixture();
        let s = &fx.sim;
        let (p, ft) = if which { (&fx.payload, 1) } else { (&fx.segment_payload, 2) };
        let seq = if which { s.seq() - 1 } else { s.seq() };
        let m = mutate(mutate(p.clone(), at, op, x), at2, op2, y);
        let mut compressed = deflate(&m);
        if squash {
            compressed = mutate(compressed, at2, op, y);
        }
        let h = Header::of(s, &s.devs[0], ft, seq);
        let f = seal_compressed(&h, &s.devs[0], &compressed, [9; 32]);
        let _ = read_backup(&f, &s.board, s.rec.last().unwrap());
    }

    // `HistoryRecord::from_bytes` on random bytes and mutated records: an
    // error, or a record whose bytes are the input.
    // spec: SEAL-GEN-011
    // spec: SEAL-GEN-008
    // spec: SEAL-BKP-002
    #[test]
    fn history_record_parser_never_panics(
        random in proptest::collection::vec(any::<u8>(), 0..600),
        which in any::<usize>(), at in any::<usize>(), op in any::<u8>(), x in any::<u8>(),
    ) {
        let fx = fixture();
        let r = &fx.records[which % fx.records.len()];
        for input in [random, mutate(r.clone(), at, op, x)] {
            if let Ok(rec) = HistoryRecord::from_bytes(&input) {
                prop_assert_eq!(rec.to_bytes(), input);
            }
        }
    }
}
