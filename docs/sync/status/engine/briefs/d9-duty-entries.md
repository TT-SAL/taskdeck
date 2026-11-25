# Task D9: a duty entry refused with a re-sealable code is reported by its duty going on

Base: the commit that holds this brief. Read `d-simulator.md` and `d2-simulator-followup.md` to
`d8-wide-gate.md` in this directory: their rules still hold (an invariant is read strictly as
worded below; you do not read `crates/taskdeck-sync/src/engine/**`; you change only the files
this task lists; the simulator's own tests build the situations they check by hand, and none
names what a seed produced; no test is skipped, ignored or loosened).

## What the owner decided (2026-11-25, "Duty going on counts")

`engine.md` §31.7 invariant 2's second outcome now reads (the new text goes into the spec in a
pull request of its own while you work; use this one):

> - the reference log holds it once, finally refused, and its device reported the refusal: with a
>   refusal record, with the `PairFailed` of its pairing, or with the end of its operation; as for
>   a pairing's `DeviceAdded` that a rotation made stale (§28.4 step 4), and for a Skip whose
>   target another device skipped first (§17.1); or it is an entry of the engine's own duties (a
>   `snap` entry or a beat), refused with a re-sealable code, for which no refusal record is
>   written (ENG-REF-001) and no entry is sealed again (§24.3), and its duty went on: the snapshot
>   duty counted the failure (§19.4), or the device beat again (§23), the owner's decision of
>   2026-11-25;

The case: an own `snap` entry sealed before a rotation that the relay sequenced first is refused
`seal/stale_epoch` (or `seal/old_epoch`); the engine writes no refusal record for those codes and
re-seals only `cmd` entries, and the snapshot duty counts the failure and goes on. `gate` seeds
17163374614648705049, 5346 and 5434 fail today only for this reason (an own `Snap` entry refused
`seal/stale_epoch` at 68, 89 and 136).

## What to do

1. Make invariant 2 (and invariant 3 where it defers to invariant 2) accept exactly that outcome:
   an entry of kind `Snap` or `Beat`, finally refused in the reference log with
   `seal/stale_epoch` or `seal/old_epoch`, whose device "went on" with that duty after the
   refusal. Read "went on" strictly from what the run records: for a `snap` entry, the same
   device later wrote a `snap` entry of its own (an acknowledged one, whatever became of it), or
   its snapshot duty ended (three failures, §19.4), or another device's snapshot covered the duty
   (if you can tell that only from what the engine writes, say how); for a beat, the same device
   later wrote a beat. If you cannot tell from the run's records, report it rather than widening.
   Nothing wider: a `cmd` entry, a control entry of an operation or a pairing, or a duty entry
   refused with any other code is held to the other outcomes as today.
2. Tests: hand-built, each failing when the rule is removed or widened (mutations, as D6 to D8
   did): a refused `snap` entry with its duty going on passes; the same without the duty going on
   fails; a `cmd` entry refused with `seal/stale_epoch` that was never re-sealed still fails; a
   duty entry refused with another code still needs its refusal record.
3. Run the gate as D8 did (release, `TD_SIM_ALL=1`): `gate` at bases 100 (50), 1000 (100), 2000
   (300), 17163374614648704531 (1,000) and 5000 (1,000); every named configuration at base 100
   with 12 seeds; `--test regressions` (every file must pass; you change none). Report every seed
   that still fails with its class (engine fault, check fault, spec question) and its trace
   lines, as D8 did.

## Files you may change

`crates/taskdeck-sync/src/sim.rs` (not the splitmix and relay lines), files under
`crates/taskdeck-sync/src/sim/` except `splitmix.rs`, `relay.rs` and `relay/`,
`crates/taskdeck-sync/tests/simulator.rs`, `crates/taskdeck-sync/tests/regressions.rs`, and
`docs/sync/status/engine/api-simulator.md` (say what changed). Not
`crates/taskdeck-sync/tests/regress/**`; do not read `crates/taskdeck-sync/src/engine/**`.

## Done

The task's gate (unit tests, regressions, relay conformance, clippy with `-D warnings`, the wasm32
check with `--features sim`) passes in your worktree with your own `CARGO_TARGET_DIR` (clean the
crate there before the last run); your work is committed on your branch; your report lists what
you built, the mutations, the gate runs and every failing seed with its class.
