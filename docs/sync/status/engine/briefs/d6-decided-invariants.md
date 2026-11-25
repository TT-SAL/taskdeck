# Task D6: the simulator's checks carry the owner's decisions

Base: the commit that holds this brief. Read `d-simulator.md`, `d2-simulator-followup.md`,
`d3-gate-seeds.md`, `d4-sim-tests.md` and `d5-check-faults.md` in this directory: their rules
still hold (an invariant is read strictly as worded below; you do not read
`crates/taskdeck-sync/src/engine/**`; you change only the files this task lists; the simulator's
own tests build the situations they check by hand, in `src/sim/script.rs` or beside the check,
and none names what a seed produced).

## What the owner decided (2026-11-25)

The gate as written could not pass although the engine does what the rest of the spec asks of
it. The owner changed what three invariants promise. The new text of `engine.md` §31.7 follows;
it is the contract for this task. It goes into the spec in a pull request of its own while you
work, so your checkout still shows the old text: use this one.

### 31.7 The six invariants

Let the **reference log** be the final model relay's log after the settle, or, in a run with a fork, the log of the branch the oracle names. The **oracle** is a fresh `Engine<ToyMachine, FakeSealer>` with the genesis anchor that replays the reference log from seq 1 with no faults.

A **removed relay state** is a part of a relay's history that a rollback or fork fault took away from the reference log: in a run with a fork, every branch the oracle does not name; after a rollback, each dropped seq at which the reference log, after the settle, holds a different entry, or holds none because every restore tail that would have brought it back (§21.3) was refused for a frame the relay made up (`relay.md` §12.9). A dropped seq that the reference log holds again, or that a restore tail could have brought back, is not removed: a plain rollback is repaired. An acknowledgement **comes from** a removed relay state when the relay gave it at a seq of that state, on that branch or before that rollback. For these two faults the promise is that an entry is **never lost silently**: a relay that forks, or that rolls back under entries another device then overwrites, can lose entries, and SYNC.md §4.8 answers it with an alarm with evidence and a re-home that lists what it does not carry over (§28.3), not with the entries.

1. **Convergence.** At the end, every device that is `Live`, joined, current and unhalted, with no stop, has the same `B` and the same state hash as the oracle at that seq.
2. **Acknowledged entries.** For every `entry_id` that a relay acknowledged (`new` or `dup`) to a device, at the end one of these holds:
   - the reference log holds it exactly once with class `Accept` (applied, or refused by `apply`), after any number of re-sealable refusals, and every device whose `B` is past that seq has applied it exactly once;
   - the reference log holds it once, finally refused, and its device reported the refusal: with a refusal record, with the `PairFailed` of its pairing, or with the end of its operation; as for a pairing's `DeviceAdded` that a rotation made stale (§28.4 step 4), and for a Skip whose target another device skipped first (§17.1);
   - it was never lost silently: every acknowledgement of it came from a removed relay state, and a device that holds its frame raised an alarm of §21.1 with evidence.
3. **Queued edits.** For every `Notify(Enqueued)` with an `entry_id`: at the end, either the reference log holds that `entry_id` exactly once with class `Accept`, or the record is still in the outbox of a device that is halted, stopped or revoked, or it was finally refused with a refusal record on its device, or it was never lost silently as invariant 2 states.
4. **Tampering.** No device's confirmed state at any moment differs from the oracle's at the same `B` (on the branch it followed), except, from a Skip's target `t` up to the Skip at `s` (`t ≤ B < s`), the state of a device that never applied `t` (it crash-halted on `t`, or the Skip was committed to its `M_K` before `t` reached its board, §17.3 step 1): that state is compared with the reference log replayed with `t` left out, which the simulator computes from the oracle's classes without running the engine under test (ENG-SKIP-002, ENG-HALT-004); and every device that received a welcome or page contradicting its own log, or a page from a rolled-back or forked relay state that it would otherwise have applied, raised an alarm of §21.1 with evidence before applying any entry of it; every injected frame raised `SkippedFrame`.
5. **No deletion by the relay.** Every `TruncateLogBelow`, `DeleteBases`, outbox `Drop`, `forget_roots_below` and identity change a device emits is explained by one of: the retention rules of §7.13; an accepted purge rotation (§28.2); a `ConfirmRejoin` (§19.7); a join (§19.8); an entry that reached the board head; a cancelled hold; an accepted epoch rotation or refused own rotation.
6. **Settling.** At `T_f + 7,200,000`, every device is either `Live`, caught up, with no record older than `T_f + 600,000` in its outbox, or halted or stopped with a persisted reason; no request has been outstanding longer than 120,000 ms; and in the last 1,800,000 ms no device emitted an `Append` other than of beats and duty entries.

## What to do

1. **Removed relay state, and "never lost silently" (invariants 2 and 3).** The check already
   names these losses ("lost to a rolled-back relay state …", "applied on a rolled-back relay
   state …"). Make them pass exactly when the new text says, and no wider:
   - a fork: every branch the oracle does not name is removed;
   - a rollback: a dropped seq is removed only where the reference log, after the settle, holds a
     different entry, or holds none because every restore tail that would have brought it back
     was refused for a frame the relay made up (`relay.md` §12.9, the code `relay/wrong_board`).
     A dropped seq that the reference log holds again, or that a restore tail could have brought
     back, is **not** removed: a loss there still breaks invariant 2 (a plain rollback is repaired;
     the `rollback` configuration passes today and must keep failing if restore tail breaks);
   - every acknowledgement of the entry came from a removed state, **and** a device that holds the
     entry's frame raised an alarm of §21.1 with evidence (take the kinds from `engine.md` §21.1).
     A loss without such an alarm still fails.
2. **A final refusal (invariant 2).** The reference log holds the entry once, finally refused, and
   its device reported it: a refusal record, the `PairFailed` of its pairing, or the end of its
   operation (`OpDone`). The two cases the gate shows: a pairing's `DeviceAdded` made stale by a
   rotation (§28.4 step 4) and the second of two Skips of one target (§17.1; since this base the
   engine ends that Skip operation with `Ok(())`, a decision of the owner). Any final refusal
   without such a report still fails.
3. **The Skip window (invariant 4).** For a device that never applied the target `t` (it
   crash-halted on `t`, or the Skip was committed to its `M_K` before `t` reached its board),
   compare its confirmed states at `t ≤ B < s` with the reference log replayed with `t` left out,
   computed from the oracle's classes **without running the engine under test** (for example the
   toy state machine applied to the accepted `cmd` entries in order, `t` left out, and the hash
   the check compares computed from that). Task D3 tried a second oracle run with injected
   crashes; its checker rejected that because the expected value then came from the engine under
   faults. Ruling 6 still holds: a `B` in the window that no expected value covers is a
   violation. Every other device and every other `B` is compared with the oracle as before.
4. **Invariants 1 and 6 are unchanged.** Since this base the engine stops, with `Closed` and
   re-pair, a device whose sign-in the relay refuses `401 relay/unauthorized` on a board never
   moved (a device paired through a relay state that was later lost). Such a device is then
   "stopped with a persisted reason". If a seed still shows a device that joined on a removed
   branch and neither settles nor stops, report it as an engine fault; do not add an exception.
5. **Two leftovers.** D5's low: the crash-cut exemption for a pairing's `Drop` covers the drop's
   own call but not the next call, where its `PairFailed` may be due; fix it with a test. D4's
   open item: no test fails when a Skip replay overwrites the oracle's first state at a `B`; add
   one if it can be built by hand, else say why not.
6. Each change with a test that fails when its rule is removed (say how you tried each), among
   them: a plain rollback whose entries no restore tail brought back is **not** excepted; a loss
   to a removed state with no alarm is **not** excepted; a final refusal with no report is **not**
   accepted; a device that applied `t` is still held to the oracle in the window.
7. Then run, reading every violation (`TD_SIM_ALL=1`): `gate` at `TD_SIM_BASE=100 SIM_SEEDS=50`,
   `TD_SIM_BASE=1000 SIM_SEEDS=100`, `TD_SIM_BASE=2000 SIM_SEEDS=300`; every named configuration
   with 12 seeds at base 100, `fork` included. The lead's count on this base before the change
   (450 `gate` seeds): 268 fail, all of them in the four classes above. List every seed that
   still fails under exactly one cause, as seed, configuration, first bad trace line, clause and
   the sequence in five lines. Do not fix the engine and do not widen a check to make a seed pass.

## Done when

The unit tests, `regressions` (all ten files: `1099.json` failed only with losses to a forked
relay state and is expected to pass now; if it does not, report why and leave the file as it
is), `relay_conformance`, clippy with `-D warnings` and the wasm check pass, and
`docs/sync/status/engine/api-simulator.md` describes the new invariants as the check reads them.
Use your own `CARGO_TARGET_DIR`, and run `cargo clean -p taskdeck-sync` there after a checker's
copy was built in it.
