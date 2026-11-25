# Task D3: what still fails under `gate`, seed by seed

Working copy: `/home/user/wt/d3` (branch `wt/d3`). `CARGO_TARGET_DIR=/home/user/wt/target-d`.
Read `docs/sync/status/engine/briefs/README.md`, `d-simulator.md` and `d2-simulator-followup.md`
first: their rules hold here (**do not read `src/engine/**`**; nobody loosens an invariant; no
excuse without the rule that names it). Tasks D and D2 wrote the simulator. Since D2 the lead
changed the engine (the journal, 2026-11-25): nothing is sent to a fenced relay; a pairing cut off
by a restart answers `PairFailed` (`engine/pair_interrupted`); a move waits for its relay unless
the user says `UserAction::OldRelayGone`; a `Hello` per 600,000 ms on a push session; a move's
seed starts above a frame classified `Skip` or the move ends with `engine/move_tail` before the
fence; a skipped frame above every snapshot makes one snapshot due.

Every configuration without rollback and fork now passes (17 configurations, 60 seeds each at
base 1000). What follows is `gate`, `TD_SIM_BASE=1000`, 100 seeds: 63 fail, 45 of them only with
`lost to a rolled-back relay state` (the owner decides those; leave them failing).

## 1. Find out why each of these fails (first), with the trace

For each seed: the event sequence that leads to the first violation (trace lines quoted, at most
ten per seed), the clause of `docs/sync/spec/engine.md` (or `relay.md`) it turns on, and who is
wrong: the engine, the simulator (workload, model runtime, oracle, check), the model relay, or
the spec. Use the clause index, not the whole spec.

- **1014, 1098**: invariant 4, "confirmed state at B differs from the oracle's on the reference
  log (between a Skip's target and the Skip at S)". The lead's guess, to confirm or refute from
  the trace: the Skip's target is a healthy entry on which one device crash-halted (two crashes
  at its apply), every other device applied it, and the oracle's state at a `B` in `t..S` is the
  one after its own Skip replay, not the one it had when it first reached that `B`. A device's
  confirmed state at `B` is compared with the oracle's state **when the oracle's `B` was that
  value on the same history**: before the Skip was applied for a device that had not seen it. If
  that is the cause it is the oracle's sampling: fix it (keep the first state the oracle had at
  each `B`; a Skip replay does not overwrite it), with a test that fails before. Ruling 6 stands:
  nothing in `t..S` is left uncompared.
- **1099**: invariant 3, five entries "in its outbox: true", no rollback text.
- **1058**: one invariant-2 violation with classes `[]` and **no** `lost to a rolled-back relay
  state` text, beside four that have it. Is that entry lost to the same fault and not attributed
  (then the attribution is incomplete: fix it), or lost otherwise (then say exactly how)?
- **1017, 1029**: invariant 2, an acknowledged entry with classes `["refuse"]`. Is it a
  pairing's `DeviceAdded` that `register` acknowledged and a racing rotation made stale (§28.4
  step 4; the owner decides that case, leave it failing but make the violation text say
  `a pairing's DeviceAdded refused after register (§28.4 step 4)` so that these seeds can be told
  apart), or something else?
- Of the seeds whose violations include invariant 3 "in its outbox: false" (1004, 1013, 1016,
  1028, 1031, 1035, 1039, 1049 and three more): are all of those records ones their device
  applied on a relay state that a rollback or fork later removed? If so make the text say so
  (`applied on a rolled-back relay state`, with the fault's trace line), still failing. Name any
  seed where it is not so.
- **1019**: invariant 1, "the oracle never reached B = N": a device on a forked copy? Say what
  state the device is in at the end (stopped with evidence, or live).

## 2. What to change

Only the simulator's own faults, each with a test that failed before (cite the clause), and the
violation texts named above. An engine fault is reported, not worked around: seed, configuration,
first bad trace line, clause, the event sequence. A spec question is reported with both readings.
Do not change `src/engine/**`, `src/fake.rs`, the model relay, `Cargo.toml`, `src/lib.rs`.

## 3. Done when

The done gate passes; `git status` is clean in the working copy; `api-simulator.md` says what
changed; and the report lists, for `gate` at base 1000 (100 seeds) and base 100 (50 seeds) after
your changes, each failing seed under exactly one of: lost to a rollback or fork only; §28.4
step 4 only; an engine fault (with the sequence); a spec question; unknown.
