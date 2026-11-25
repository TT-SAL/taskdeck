# Task D2: the simulator, follow-up

Working copy: `/home/user/wt/d2` (branch `wt/d2`). `CARGO_TARGET_DIR=/home/user/wt/target-d`.
Read `docs/sync/status/engine/briefs/README.md` and `d-simulator.md` first: their rules hold here
(**do not read `src/engine/**`**; nobody loosens an invariant). Task D wrote the simulator; its
check found defects and its fix pass never ran. This task is that fix pass, against the engine as
it is now (the lead fixed what task D reported: the move, the join's first Meta, reads before
their write is durable, the expired hold's timer, the repeated admin request).

## 1. Fix what the check found

The check's findings are the `findings` array of the second `result` line of
`/root/.claude/projects/-home-user-taskdeck/dcc7fc8e-52e3-5679-a732-027c5697d5fb/subagents/workflows/wf_70e5dcc6-438/journal.jsonl`
(read that line with a few lines of python; each finding has a reproduction). Fix every high and
medium one. The ones the lead has read:

- Invariant 6: "in the last 1,800,000 ms no device emitted an `Append` other than of beats and
  duty entries" is never checked. Check it.
- Invariant 2 excuses a lost acknowledged entry when its device is retired, halted, stopped or
  revoked. §31.7 has no such exemption: remove it.
- Invariant 5: one `ConfirmRejoin` excuses every later deletion on that identity; and the drop
  of any unsent Register record or held record is excused without looking. An excuse must match
  the one deletion it explains (the rejoin's own truncation and base deletion; the cancelled
  hold's record; and see the ruling on pairing below).

## 2. Rulings on task D's questions (the lead's; the spec change follows)

1. **Rollback or fork followed by another device's append** (question 1): do not excuse anything
   in the code. Classify instead: when an acknowledged entry is missing from the reference log
   and its frame was sequenced only in a relay state that a `relay_rollback` or `relay_fork`
   fault later removed from the reference branch, report the violation with the text
   `lost to a rolled-back relay state` and the fault's trace line, so that these seeds can be
   told apart. Whether §31.7 exempts them is the owner's decision; until then they fail.
2. **A pairing's `DeviceAdded` dropped** after `seal/pair_raced` or after the relay refused
   `register` (question 2): explained (§28.4 step 3; the record can only travel in `register`).
   Invariant 5 accepts the drop of a record whose `via` is `Register` when the same device
   emitted `Notify(PairFailed)` for it in the same or the next call.
3. **An empty page while the relay head is above `L`** (question 3): report the seed to the lead
   if the engine still re-reads without a pause; do not model around it.
4. **Poison** (question 4): as you built it.
5. **A held create cancelled by `Forget`** (question 5): invariant 3 accepts an `Enqueued` entry
   whose record was dropped as a cancelled hold (`CancelBoth`).
6. **Invariant 4 at a Skip** (question 6): compare at every `B` the oracle has; the oracle
   replays the reference log with the same Skips, so its state at the Skip's seq is the state
   after the Skip replay. Skip nothing.
7. **Ticks** (question 7): the runtime may deliver a `Tick` at or after the time asked.
8. **`bootstrap.tdk`** (question 8): settled in the engine (the first `Meta` is in the first write).
9. **A rules-1 build joining a board that is at rules 2** (question 9, defect 6): the pairing
   machine must not start such a join (`seal.md` §17.9 checks the bundle; the engine answers
   `engine/local_fault` for pairing material it cannot load, §33). In the workload, a machine
   whose build lacks the board's active rules is given the newer build before it pairs. Every
   seed of `rules` failed for this; make the workload right, then report what still fails.
10. **`Welcome` without a door, `RelayRefusal` without `last_ctr`** (question 10): leave as is.

§11.9 says the runtime handles `relay/token_stale` itself (it logs in again) and that a refused
login reaches the engine as `RelayRefused { req: None }` followed by `Disconnected`. Check that
the model runtime does both.

## 3. Then diagnose what still fails

Run every named configuration (12 seeds each is enough to start) and the gate. With the lead's
engine of today, at base 100: `rules` fails every seed (ruling 9), `rollback` 1 of 12 (ruling 1),
`gate` 9 of 12 (invariants 3, 2 and 6: "enqueued entry is accepted 0 times", "acknowledged entry
... classes there: [refuse]", "an admin request outstanding since 86400000 ms"); every other
configuration passes 12 of 12. For each failing seed that is not the simulator's own fault,
report: seed, configuration, the first bad trace line, what the trace shows around it (ten lines
at most), and the clause you think is broken. Group seeds with one cause. Where the cause is in
your code, fix it. Do not narrow the `gate` configuration.

Add no regression file for a seed that still fails. For each of these seeds, which the lead's
fixes are meant to have cured, add its regression file if it passes now (the JSON the test
prints, §31.8): seed 200 of `move`, 111 and 116 of `crash`, 102 of `rollback`, 1 of `quiet`,
310 of `gate`.

## Files you may change

As task D: `src/sim.rs` (not the `splitmix` and `relay` lines), `src/sim/**` except
`splitmix.rs`, `relay.rs`, `relay/`; `tests/simulator.rs`, `tests/regressions.rs`,
`tests/regress/**`; `docs/sync/status/engine/api-simulator.md`; in `src/fake.rs` only the three
helpers task D added.

## Done means

In `/home/user/wt/d2` with `CARGO_TARGET_DIR=/home/user/wt/target-d`: `cargo test --locked -p
taskdeck-sync --lib`, `--test regressions` and `--test relay_conformance` pass; clippy with
`--all-targets -- -D warnings` passes; the wasm32 check with `--features sim` passes; everything
is committed on `wt/d2`. The `simulator` target may still fail: then your report lists every
failing seed of base 100 (12 per configuration, 50 of the gate) with its cause or its group.

Report: what you fixed, by finding; the failing seeds grouped by cause; the final field lists of
`FaultSet`, `OpSet`, `SimStats` if they changed; timings (1,000 quiet seeds, 50 gate seeds,
debug and release).
