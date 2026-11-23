//! `classify` over the bytes a caller supplies for the targets of a Skip
//! item (seal.md §12.9, §14.1 `skip_targets`; task S7): random bytes, and
//! the real target frame mutated (bytes changed, inserted, removed, cut,
//! length fields raised). It returns, never panics; its only error is
//! `seal/missing_root`; and it accepts the Skip only with the real target.

use std::sync::OnceLock;

use crate::board::*;
use crate::common::hex;
use proptest::prelude::*;
use taskdeck_seal::*;

struct Fixture {
    sim: Sim,
    /// The Skip entry, by device 0, of the `cmd` at `target`.
    skip: Vec<u8>,
    target: u64,
    target_frame: Vec<u8>,
}

fn fixture() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        let mut s = Sim::new(47, 3);
        let cmd = s.cmd(Who::D(2), &Spec::default());
        s.accept(cmd.clone());
        let target = s.seq();
        let item = CtlItem::Skip(SkipItem { seq: target, frame_hash: frame_hash(&cmd) });
        let skip = s.ctl(Who::D(0), vec![item], &Spec::default());
        Fixture { sim: s, skip, target, target_frame: cmd }
    })
}

fn mutate(mut b: Vec<u8>, at: usize, op: u8, x: u8) -> Vec<u8> {
    if b.is_empty() {
        return vec![x];
    }
    let i = at % b.len();
    match op % 5 {
        0 => b[i] ^= x | 1,
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

    // spec: SEAL-GEN-011
    // spec: SEAL-GEN-008
    #[test]
    fn classify_never_panics_on_skip_targets(
        random in proptest::collection::vec(any::<u8>(), 0..1200),
        at in any::<usize>(), op in any::<u8>(), x in any::<u8>(),
    ) {
        let fx = fixture();
        let o = Opts { targets: vec![fx.target], ..Opts::default() };
        let mut c = fx.sim.inputs("", &fx.skip, &o);
        // The control: with the real target the Skip is accepted.
        let r = replay(&c).unwrap();
        prop_assert!(matches!(r.class, Class::Accept));
        let mutated = mutate(fx.target_frame.clone(), at, op, x);
        for t in [&random, &mutated] {
            c["skip_targets"] = serde_json::json!({ fx.target.to_string(): hex(t) });
            match replay(&c) {
                Ok(r) => prop_assert!(!matches!(r.class, Class::Accept) || t == &fx.target_frame),
                Err(e) => prop_assert_eq!(e, SealError::MissingRoot),
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    // `FileSealer::resume` reads a file header (§19.1) handed back by the
    // caller: on random bytes and on a real header mutated, it returns,
    // never panics, and a sealer it resumes seals a record.
    // spec: SEAL-GEN-011
    #[test]
    fn file_resume_never_panics(
        random in proptest::collection::vec(any::<u8>(), 0..120),
        at in any::<usize>(), op in any::<u8>(), x in any::<u8>(), records in any::<u64>(),
    ) {
        let key = StorageKey::from_bytes([0x33; 32]);
        let (_, header) = FileSealer::create(&key, FileKind::Outbox, &BoardId([1; 16]), &DevId([2; 16]), &mut OsEntropy).unwrap();
        let mutated = mutate(header.clone(), at, op, x);
        for h in [&random, &mutated, &header] {
            if let Ok(mut s) = FileSealer::resume(&key, h, records % 4) {
                let rec = s.seal_record(b"x", &mut OsEntropy);
                prop_assert!(rec.is_ok());
            }
        }
        prop_assert!(FileSealer::resume(&key, &header, 0).is_ok());
    }

    // `HistoryRecord::from_accepted` on random and mutated frame bytes, with
    // the opened entry of a real control entry, its payload random or
    // mutated, and with another kind: it returns, never panics; what it
    // returns reads back through `from_bytes`.
    // spec: SEAL-GEN-011
    // spec: SEAL-BKP-010
    #[test]
    fn from_accepted_never_panics(
        random in proptest::collection::vec(any::<u8>(), 0..800),
        at in any::<usize>(), op in any::<u8>(), x in any::<u8>(), which in 0u8..4, seq in any::<u64>(),
    ) {
        let fx = fixture();
        let o = Opts { targets: vec![fx.target], ..Opts::default() };
        let r = fx.sim.classify(&fx.skip, &o).unwrap();
        let mut opened = r.opened.unwrap();
        let frame = mutate(fx.skip.clone(), at, op, x);
        match which {
            0 => opened.payload = random.clone(),
            1 => opened.payload = mutate(opened.payload.clone(), at, op.wrapping_add(1), x),
            2 => opened.kind = Kind::Beat,
            _ => {}
        }
        for f in [&random, &frame, &fx.skip] {
            if let Ok(h) = HistoryRecord::from_accepted(seq, f, &opened) {
                let bytes = h.to_bytes();
                prop_assert!(HistoryRecord::from_bytes(&bytes).is_ok());
            }
        }
    }
}
