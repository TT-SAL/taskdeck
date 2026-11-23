//! The pairing parsers and machines of task S6 over random and mutated
//! input: `parse_code` on any text; both machines over random event
//! sequences; mutated slots A to D (as they come, and re-MACed by a relay
//! that knows the code, so that the layout checks, the commitment, the AEAD
//! and the bundle's JSON parser see them); a mutated bundle JSON sealed
//! properly; and mutated log entries and frames in the join. Each returns,
//! never panics.

use proptest::prelude::*;
use taskdeck_seal::pair::*;
use taskdeck_seal::*;

use crate::board::*;
use crate::pairing::*;

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

/// A raw event: a kind, bytes, a number, text.
type Raw = (u8, Vec<u8>, u64, String);

fn raw() -> impl Strategy<Value = Raw> {
    (any::<u8>(), prop::collection::vec(any::<u8>(), 0..400), any::<u64>(), any::<String>())
}

fn n_event(r: Raw) -> NewEvent {
    let (k, b, x, s) = r;
    let arr = |n: usize| -> Vec<u8> { b.iter().copied().chain(std::iter::repeat(0)).take(n).collect() };
    match k % 8 {
        0 => NewEvent::CodeEntered {
            code: s.clone(),
            host: if x % 2 == 0 { HOST.into() } else { s },
            board_id: BoardId(arr(16).try_into().unwrap()),
            door: arr(5).try_into().unwrap(),
            name: NAME.into(),
            now: x,
        },
        1 => NewEvent::Slot { bytes: b },
        2 => NewEvent::SasEntered { digits: s },
        3 => NewEvent::Snapshot { parts: b.chunks(97).map(|c| c.to_vec()).collect() },
        4 => NewEvent::Entries { frames: vec![(x, b)] },
        5 => NewEvent::Frames { frames: vec![(x, b)] },
        6 => NewEvent::Cancel,
        _ => NewEvent::Tick { now: x },
    }
}

fn e_event(r: Raw) -> ExistingEvent {
    let (k, b, x, s) = r;
    match k % 6 {
        0 => ExistingEvent::SlotB { bytes: b },
        1 => ExistingEvent::UserConfirmed,
        2 => ExistingEvent::Registered {
            h: x % 1000,
            chain: Chain([1; 32]),
            snapshot: SnapAnnounce {
                seq: x % 1000,
                hash: [2; 32],
                hash_version: (x % 3) as u32,
                blob: BlobId([3; 16]),
                sha256: [4; 32],
                size: x % 5_000_000,
                parts: (x % 7) as u32,
                inflated: x % 100,
                epoch: (x % 4) as u32,
            },
            added_seq: x % 1003,
            token: s.clone(),
            roots: (0..(k % 4)).map(|e| (u32::from(e), EpochRoot::from_bytes([e; 32]))).collect(),
            host: if x % 2 == 0 { HOST.into() } else { s },
            door: [0; 5],
        },
        3 => ExistingEvent::RegisterFailed { code: SealError::PairRaced },
        4 => ExistingEvent::Cancel,
        _ => ExistingEvent::Tick { now: x },
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    // spec: SEAL-GEN-011
    // spec: SEAL-CODE-002
    #[test]
    fn parse_code_never_panics(s in any::<String>(), t in "[0-9A-Za-z \\-\t]{0,24}") {
        let _ = parse_code(&s);
        let _ = parse_code(&t);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    // Random events into a fresh N, and into N and E in the middle of a
    // pairing (after the code and slot A, resp. after start).
    // spec: SEAL-GEN-011
    #[test]
    fn the_machines_never_panic_on_random_events(evs in prop::collection::vec(raw(), 1..8), evs_e in prop::collection::vec(raw(), 1..8)) {
        let mut n = New::new();
        for r in evs.clone() {
            let _ = n.on(n_event(r), &mut OsEntropy);
        }
        let mut run = Run::new(Sim::new(720, 1), 0);
        let _ = run.n_on(run.code_event());
        let _ = run.n_on(NewEvent::Slot { bytes: run.slot_a.clone() });
        for r in evs {
            let _ = run.n_on(n_event(r));
        }
        for r in evs_e {
            let _ = run.e_on(e_event(r));
        }
    }

    // Mutated slots, as posted and re-MACed, into the machine that reads them.
    // spec: SEAL-GEN-011
    // spec: SEAL-PAIR-003
    #[test]
    fn mutated_slots_never_panic(at in any::<usize>(), op in any::<u8>(), x in any::<u8>(), remac in any::<bool>()) {
        let mut r = Run::new(Sim::new(721, 1), 0);
        let k = r.k.clone();
        let fix = |b: Vec<u8>| if remac && b.len() >= 36 { k.slot(b[0], b[1], &b[4..b.len() - 32]) } else { b };
        // A into N.
        let _ = r.n_on(r.code_event());
        let a = fix(mutate(r.slot_a.clone(), at, op, x));
        let _ = r.n_on(NewEvent::Slot { bytes: a });
        // B into E.
        let mut r = Run::new(Sim::new(721, 1), 0);
        r.step_b();
        let b = fix(mutate(r.slot_b.clone(), at, op, x));
        let _ = r.e_on(ExistingEvent::SlotB { bytes: b });
        // C into N.
        let mut r = Run::new(Sim::new(721, 1), 0);
        r.step_b().step_c();
        let c = fix(mutate(r.slot_c.clone(), at, op, x));
        let _ = r.n_on(NewEvent::Slot { bytes: c });
        // D into N.
        let mut r = Run::new(Sim::new(721, 1), 0);
        r.through_d();
        let k = r.k.clone();
        let d = mutate(r.slot_d.clone(), at, op, x);
        let d = if remac && d.len() >= 36 { k.slot(d[0], d[1], &d[4..d.len() - 32]) } else { d };
        let _ = r.send_d();
        let mut q = Run::new(Sim::new(721, 1), 0);
        q.through_d();
        let _ = q.n_on(NewEvent::Slot { bytes: d });
    }

    // A mutated bundle JSON, sealed properly by a test playing E, into N's
    // bundle parser.
    // spec: SEAL-GEN-011
    // spec: SEAL-PAIR-009
    #[test]
    fn mutated_bundles_never_panic(at in any::<usize>(), op in any::<u8>(), x in any::<u8>()) {
        let board = BoardId([0xB0; 16]);
        let s = SnapAnnounce { seq: 70, hash: [1; 32], hash_version: 1, blob: BlobId([2; 16]), sha256: [3; 32], size: 900, parts: 1, inflated: 900, epoch: 2 };
        let json = bundle_json(&board, HOST, &DOOR, &[(1, [7; 32]), (2, [8; 32])], Head { seq: 70, chain: Chain([5; 32]) }, &s, 71, TOKEN);
        let w = WithFakeE::new(board, DevId([0xE0; 16])).accept();
        let d = w.slot_d(&mutate(json, at, op, x));
        let mut n = w.n;
        let fx = n.on(NewEvent::Slot { bytes: d }, &mut OsEntropy);
        prop_assert!(n_abort(&fx) != Some(SealError::SlotMac) && n_abort(&fx) != Some(SealError::BundleOpen));
        let _ = n.on(NewEvent::Snapshot { parts: vec![vec![x; 70]] }, &mut OsEntropy);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // Mutated snapshot parts, log entries and frames in the join.
    // spec: SEAL-GEN-011
    #[test]
    fn a_mutated_join_never_panics(at in any::<usize>(), op in any::<u8>(), x in any::<u8>(), which in 0u8..4) {
        let mut r = Run::new(Sim::new(722, 2), 0);
        r.step_b().step_c().step_sas().step_confirmed().take_snapshot();
        let f = r.sim.cmd(Who::D(1), &Spec::default());
        r.sim.accept(f);
        r.append_added(|_| {}, &Spec::default());
        let ev = r.registered();
        r.post_d(ev);
        let _ = r.send_d();
        if which == 0 {
            let bytes = mutate(r.snap.as_ref().unwrap().bytes.clone(), at, op, x);
            let _ = r.send_snapshot(Some(bytes));
            return Ok(());
        }
        let _ = r.send_snapshot(None);
        let mut log = r.log();
        let i = at % log.len();
        match which {
            1 => log[i].1 = mutate(log[i].1.clone(), at / 7, op, x),
            2 => log[i].0 = log[i].0.wrapping_add(u64::from(x)),
            _ => log.truncate(i),
        }
        let _ = r.n_on(NewEvent::Entries { frames: log.clone() });
        let _ = r.n_on(NewEvent::Frames { frames: log });
    }
}
