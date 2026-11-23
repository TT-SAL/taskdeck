//! The rebase property test (core.md §18.6; SYNC_PLAN.md §6.2 item 9):
//! pending edits replayed over confirmed ones.
//!
//! Device 1 compiles a few commands on its pending view (the confirmed state
//! at `head`, with its own commands applied to it under provisional seqs).
//! Meanwhile devices 2 and 3 compile commands on their views, and those are
//! sequenced first. Then device 1's pending commands are applied again, as
//! they are (not compiled again), after the others, with later seqs and their
//! original `based_on`. Nothing panics; two replicas doing the same, one of
//! them loaded from the other's canonical bytes, reach the same bytes, results
//! and notices; a refused command leaves the board unchanged.

mod common;

use common::devices::{Device, Step, a_step, apply_checked, start_instant};
use common::{boards, zones};
use proptest::prelude::*;
use taskdeck_core::apply::{Applied, Ctx, Refusal};
use taskdeck_core::board::BoardData;
use taskdeck_core::command::LogCommand;
use taskdeck_core::legacy::import;
use taskdeck_core::time::{ChronoZone, Instant};

/// The confirmed state the run starts from: the `small` fixture or the empty
/// board, moved on by `warmup` steps of devices 2 and 3, all sequenced.
fn confirmed(small: bool, others: &mut [Device], warmup: &[Step], now: &mut Instant) -> (BoardData, u64) {
    let mut board = if small { import(&boards::board("small")).0.expect("small imports") } else { BoardData::empty() };
    for d in others.iter_mut() {
        d.view = board.clone();
    }
    let mut seq = 1;
    for step in warmup {
        *now = now.plus_seconds(i64::from(step.advance) * 60).expect("in range");
        let device = &mut others[step.device % 2];
        device.view = board.clone();
        device.head = seq;
        if let Some((command, ctx)) = device.compile_step(step, *now, seq + 1) {
            seq += 1;
            let _ = apply_checked(&mut board, &command, &ctx);
        }
    }
    (board, seq)
}

proptest! {
    // spec: CORE-CTX-002
    // spec: CORE-CONF-007
    // spec: CORE-APPLY-002
    #[test]
    fn pending_commands_replayed_after_others_agree_on_two_replicas(
        small in any::<bool>(),
        warmup in proptest::collection::vec(a_step(), 0..12),
        pending in proptest::collection::vec(a_step(), 1..12),
        meanwhile in proptest::collection::vec(a_step(), 0..20),
        local_other in any::<bool>(),
    ) {
        let mut now = start_instant();
        let third: Box<dyn taskdeck_core::time::Zone> =
            if local_other { Box::new(ChronoZone(chrono::Local)) } else { Box::new(zones::new_york()) };
        let empty = BoardData::empty();
        let mut others = vec![Device::new(2, Box::new(zones::new_york()), &empty, 1), Device::new(3, third, &empty, 1)];
        let (state, head) = confirmed(small, &mut others, &warmup, &mut now);

        // Device 1 compiles on its pending view: the confirmed state at `head`.
        let mut origin = Device::new(1, Box::new(zones::berlin()), &state, head);
        let mut queued: Vec<(LogCommand, Ctx)> = Vec::new();
        for step in &pending {
            now = now.plus_seconds(i64::from(step.advance) * 60).expect("in range");
            let provisional = head + 1 + queued.len() as u64;
            if let Some(entry) = origin.compile_step(step, now, provisional) {
                prop_assert_eq!(entry.1.based_on, head);
                queued.push(entry);
            }
        }

        // Meanwhile the others' commands are sequenced after `head`.
        let mut log: Vec<(LogCommand, Ctx)> = Vec::new();
        let mut confirmed_now = state.clone();
        let mut seq = head;
        for step in &meanwhile {
            now = now.plus_seconds(i64::from(step.advance) * 60).expect("in range");
            let device = &mut others[step.device % 2];
            if step.sync {
                device.view = confirmed_now.clone();
                device.head = seq;
            }
            if let Some((command, ctx)) = device.compile_step(step, now, seq + 1) {
                seq += 1;
                let _ = apply_checked(&mut confirmed_now, &command, &ctx);
                log.push((command, ctx));
            }
        }
        // Then device 1's pending commands, as they are, with later seqs and
        // their original `based_on`.
        for (command, ctx) in queued {
            seq += 1;
            log.push((command, Ctx { seq, ..ctx }));
        }

        // Two replicas apply the same log: one from the confirmed state, one
        // loaded from its canonical bytes.
        let mut one = state.clone();
        let mut two = BoardData::from_canonical_bytes(&state.to_canonical_bytes()).expect("the state loads");
        let mut results: Vec<Result<Applied, Refusal>> = Vec::new();
        for (command, ctx) in &log {
            prop_assert!(ctx.based_on < ctx.seq);
            let a = apply_checked(&mut one, command, ctx);
            let b = apply_checked(&mut two, command, ctx);
            prop_assert_eq!(&a, &b, "{} at seq {}", command.op(), ctx.seq);
            prop_assert!(one.check().is_ok());
            results.push(a);
        }
        prop_assert_eq!(one.to_canonical_bytes(), two.to_canonical_bytes());
        // The same log again from the start gives the same results.
        let mut three = state.clone();
        for ((command, ctx), expected) in log.iter().zip(&results) {
            prop_assert_eq!(&apply_checked(&mut three, command, ctx), expected);
        }
        prop_assert_eq!(three.to_canonical_bytes(), one.to_canonical_bytes());
    }
}
