//! The determinism property test (core.md §18.6; SYNC_PLAN.md §6.2 item 9).
//!
//! The gate runs this target twice, under `TZ=Europe/Helsinki` and
//! `TZ=America/New_York`. Random intents are compiled by two or three devices
//! against their own views: the first in Berlin's zone, a second (of three)
//! in New York's, both explicit `TableZone`s, and the last in the process
//! zone, `ChronoZone(chrono::Local)`. The commands are applied in one order to
//! replica A; replica B is built from A's canonical bytes at a random point
//! and applies the rest of the log, each command passed as its canonical
//! bytes. Both end with equal bytes, and every entry B applies gives the
//! result A had: equal notices in equal order.
//!
//! The fixed case runs a log drawn from a fixed seed with explicit zones only
//! and pins the SHA-256 of the board it ends with, so the two `TZ` runs must
//! reach the same bytes: the process zone reaches neither `apply` nor a
//! compile with an explicit zone.

mod common;

use common::devices::{Device, Step, a_step, apply_checked, start_instant};
use common::{boards, zones};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::{RngAlgorithm, TestRng, TestRunner};
use sha2::{Digest, Sha256};
use taskdeck_core::apply::{Applied, Ctx, Refusal};
use taskdeck_core::board::BoardData;
use taskdeck_core::command::{LogCommand, parse_command};
use taskdeck_core::legacy::import;
use taskdeck_core::time::{ChronoZone, Instant, TableZone, Zone};

/// A start board: the imported `small` fixture, or the empty board.
fn start_board(small: bool) -> BoardData {
    if small {
        import(&boards::board("small")).0.expect("small imports")
    } else {
        BoardData::empty()
    }
}

/// The devices of a run: `count` of them, zones as the module says, or, with
/// `explicit_only`, Berlin, New York and a fixed +05:30 table.
fn devices(count: usize, explicit_only: bool, start: &BoardData) -> Vec<Device> {
    let mut list: Vec<Box<dyn Zone>> = vec![Box::new(zones::berlin())];
    if count == 3 {
        list.push(Box::new(zones::new_york()));
    }
    if explicit_only {
        list.push(Box::new(TableZone::new(19_800, Vec::new()).expect("a zone")));
    } else {
        list.push(Box::new(ChronoZone(chrono::Local)));
    }
    list.truncate(count);
    (1u16..).zip(list).map(|(no, zone)| Device::new(no, zone, start, 1)).collect()
}

/// What replica A did: the log, each entry's result, and the canonical bytes
/// after each prefix of the log (`bytes[k]`: after `k` entries).
struct Run {
    log: Vec<(LogCommand, Ctx)>,
    results: Vec<Result<Applied, Refusal>>,
    bytes: Vec<Vec<u8>>,
}

fn run(start: &BoardData, devices: &mut [Device], steps: &[Step]) -> Run {
    let mut a = start.clone();
    let mut seq = 1;
    let mut now: Instant = start_instant();
    let mut out = Run { log: Vec::new(), results: Vec::new(), bytes: vec![a.to_canonical_bytes()] };
    for step in steps {
        now = now.plus_seconds(i64::from(step.advance) * 60).expect("in range");
        let count = devices.len();
        let device = &mut devices[step.device % count];
        if step.sync {
            device.view = a.clone();
            device.head = seq;
        }
        let Some((command, ctx)) = device.compile_step(step, now, seq + 1) else { continue };
        seq += 1;
        let result = apply_checked(&mut a, &command, &ctx);
        out.log.push((command, ctx));
        out.results.push(result);
        out.bytes.push(a.to_canonical_bytes());
    }
    out
}

/// Replica B: from A's bytes after `k` entries, the rest of the log applied,
/// each command parsed from its canonical bytes. Checks every result against
/// A's and returns B's end bytes.
fn replay_from(run: &Run, k: usize) -> Result<Vec<u8>, TestCaseError> {
    let mut b = BoardData::from_canonical_bytes(&run.bytes[k]).expect("A's bytes load");
    // CORE-DATA-005: loading changes nothing.
    prop_assert_eq!(&b.to_canonical_bytes(), &run.bytes[k]);
    for (i, (command, ctx)) in run.log.iter().enumerate().skip(k) {
        let parsed = parse_command(&command.to_canonical()).expect("a compiled command parses");
        prop_assert_eq!(&parsed, command);
        let result = apply_checked(&mut b, &parsed, ctx);
        prop_assert_eq!(&result, &run.results[i], "entry {} ({})", i, command.op());
        prop_assert_eq!(&b.to_canonical_bytes(), &run.bytes[i + 1], "entry {} ({})", i, command.op());
    }
    Ok(b.to_canonical_bytes())
}

proptest! {
    // spec: CORE-CTX-002
    // spec: CORE-CONF-007
    // spec: CORE-DATA-005
    #[test]
    fn a_replica_from_bytes_at_any_point_reaches_the_same_board(
        small in any::<bool>(),
        count in 2usize..4,
        steps in proptest::collection::vec(a_step(), 1..60),
        cut in any::<prop::sample::Index>(),
    ) {
        let start = start_board(small);
        let mut devices = devices(count, false, &start);
        let run = run(&start, &mut devices, &steps);
        let k = cut.index(run.log.len() + 1);
        let end = replay_from(&run, k)?;
        prop_assert_eq!(&end, run.bytes.last().expect("the end"));
        // The same log from the start, on a third replica, as well.
        let again = replay_from(&run, 0)?;
        prop_assert_eq!(&again, &end);
    }
}

/// The steps of the fixed case: 60 drawn from proptest's ChaCha generator
/// with a fixed seed (no random source; the same steps on every run).
fn fixed_steps() -> Vec<Step> {
    let mut runner =
        TestRunner::new_with_rng(ProptestConfig::default(), TestRng::from_seed(RngAlgorithm::ChaCha, &[7; 32]));
    let strategy = proptest::collection::vec(a_step(), 60);
    strategy.new_tree(&mut runner).expect("steps").current()
}

/// The SHA-256 of the board the fixed log ends with. Recorded from the first
/// run of this test (under TZ=Europe/Helsinki) and checked unchanged under
/// TZ=America/New_York, TZ=UTC and TZ=Asia/Kolkata: it is not a value the
/// spec gives, but the pin that makes the two runs of the gate agree on the
/// bytes. It changes when the steps' strategy or the start board does.
const FIXED_END_SHA256: &str = "50fb7df8eca42b0b8ace1c4ee4a35adf2374fffcc2297f84b3faa4b5599daa5b";

// spec: CORE-CTX-002
// spec: CORE-CONF-007
#[test]
fn a_fixed_log_with_explicit_zones_ends_on_the_same_bytes_under_every_tz() {
    let steps = fixed_steps();
    let start = start_board(true);
    let mut devices = devices(3, true, &start);
    let run = run(&start, &mut devices, &steps);
    assert!(run.log.len() >= 30, "the fixed log has {} entries", run.log.len());
    assert!(run.results.iter().filter(|r| r.is_ok()).count() >= 15);
    let end = run.bytes.last().expect("the end");
    let hex: String = Sha256::digest(end).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, FIXED_END_SHA256);
    // A replica from bytes halfway agrees as well.
    let half = run.log.len() / 2;
    assert_eq!(&replay_from(&run, half).expect("agrees"), end);
}
