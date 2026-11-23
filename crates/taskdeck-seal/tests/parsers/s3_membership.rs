//! `Membership::from_canonical_bytes` and `classify` over random and
//! mutated inputs: each returns, never panics; what the parser accepts is
//! canonical; `classify` returns no error but `seal/missing_root`.

use std::sync::OnceLock;

use crate::board::*;
use proptest::prelude::*;
use taskdeck_seal::*;

struct Fixture {
    sim: Sim,
    membership: Vec<u8>,
    frames: Vec<Vec<u8>>,
}

fn fixture() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        let mut s = Sim::new(41, 3);
        s.rotate(Who::D(0));
        let mut frames = vec![s.cmd(Who::D(1), &Spec::default()), s.beat(Who::D(2), vec![1], &Spec::default())];
        let items = s.rotate_items(vec![CtlItem::DeviceRevoked(s.dev(Who::D(2)))]);
        frames.push(s.ctl(Who::D(0), items, &Spec::default()));
        let k = s.new_recovery().public();
        let p = CtlItem::RecoveryKeyChange(RecoveryKeyChange { mode: RecoveryKeyMode::Propose, sign_pk: Some(k.sign_pk), hpke_pk: Some(k.hpke_pk) });
        frames.push(s.ctl(Who::D(1), vec![p], &Spec::default()));
        let membership = s.m.to_canonical_bytes();
        Fixture { sim: s, membership, frames }
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
            // A count or length field set to an extreme value.
            let end = (i + 4).min(b.len());
            for y in &mut b[i..end] {
                *y = 0xFF;
            }
        }
    }
    b
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    // The canonical-bytes parser on random bytes and on mutated real
    // states: an error or a state whose bytes are the input.
    // spec: SEAL-GEN-011
    // spec: SEAL-GEN-008
    // spec: SEAL-MEM-002
    #[test]
    fn membership_parser_never_panics(
        random in proptest::collection::vec(any::<u8>(), 0..1500),
        at in any::<usize>(), op in any::<u8>(), x in any::<u8>(), at2 in any::<usize>(), y in any::<u8>(),
    ) {
        let fx = fixture();
        for input in [random, mutate(mutate(fx.membership.clone(), at, op, x), at2, op.wrapping_add(1), y)] {
            if let Ok(m) = Membership::from_canonical_bytes(&input) {
                prop_assert_eq!(m.to_canonical_bytes(), input);
            }
        }
    }

    // `classify` on random frames, mutated frames, and with a mutated
    // state the parser accepted: it returns, and its only error is the
    // local `seal/missing_root`.
    // spec: SEAL-GEN-011
    // spec: SEAL-GEN-008
    #[test]
    fn classify_never_panics(
        random in proptest::collection::vec(any::<u8>(), 0..1200),
        which in any::<usize>(), at in any::<usize>(), op in any::<u8>(), x in any::<u8>(),
        mat in any::<usize>(), mop in any::<u8>(), mx in any::<u8>(), seq_shift in 0u64..4,
    ) {
        let fx = fixture();
        let f = fx.frames[which % fx.frames.len()].clone();
        let mutated = mutate(f.clone(), at, op, x);
        let o = Opts { own: Some(1), seq: Some(fx.sim.seq() + 1 + seq_shift), ..Opts::default() };
        for frame in [&random, &mutated, &f] {
            let mut c = fx.sim.inputs("", frame, &o);
            if let Err(e) = replay(&c) {
                prop_assert_eq!(e, SealError::MissingRoot);
            }
            let m = mutate(fx.membership.clone(), mat, mop, mx);
            if Membership::from_canonical_bytes(&m).is_ok() {
                c["membership"] = serde_json::json!(crate::common::hex(&m));
                if let Err(e) = replay(&c) {
                    prop_assert_eq!(e, SealError::MissingRoot);
                }
            }
        }
    }
}
