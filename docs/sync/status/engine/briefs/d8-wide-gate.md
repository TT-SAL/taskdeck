# Task D8: what fails at 1,000 seeds (the checks' share)

Base: the commit that holds this brief. Read `d-simulator.md` and `d2-simulator-followup.md` to
`d7-retired-oracle.md` in this directory: their rules still hold (an invariant is read strictly as
`engine.md` §31.7 words it; you do not read `crates/taskdeck-sync/src/engine/**`; you change only
the files this task lists; the simulator's own tests build the situations they check by hand, and
none names what a seed produced; no test is skipped, ignored or loosened). `engine.md` §31.7 in
your checkout is the current text (TT-SAL/taskdeck#34 merged it), and ENG-RECV-007 (§12.2, §12.3
step 5) is in the engine of this base.

## What the lead found

After task D7, `gate` passes 450 of 450 seeds at bases 100, 1000 and 2000, and every named
configuration passes 30 of 30 at base 3000. Two runs of 1,000 `gate` seeds do not (release,
`TD_SIM_ALL=1`): the base CI uses for this commit's parent (17163374614648704531) has 6 failing
seeds, base 5000 has 11. Their first violations:

| Seed | First violation (trace line) |
|---|---|
| 17163374614648704662 | inv. 2 (1265): lost to a removed relay state (the relay before a rollback), no device that held its frame raised an alarm |
| 17163374614648704884 | inv. 2 (1287): the same |
| 5243 | inv. 2 (3060): the same |
| 5197 | inv. 2 (2994): the same, the forked copy of the relay |
| 5271 | inv. 2 (3707): the same, the forked copy |
| 17163374614648704986 | inv. 5 (8365): `Truncate(114, false)` with B = 157 not explained |
| 5685 | inv. 5 (8770): `Truncate(114, false)` with B = 175 not explained |
| 17163374614648705463 | inv. 4 (9011): confirmed state at 167 on a chain no relay served |
| 17163374614648705507 | inv. 4 (13374): confirmed state at 243 on a chain no relay served |
| 5159 | inv. 4 (8835): confirmed state at 163 on a chain no relay served |
| 5712 | inv. 4 (13448): confirmed state at 264 on a chain no relay served |
| 5175 | inv. 4 (4273): confirmed state at 84 between a Skip's target and the Skip at 85, where no replay of the reference log without [83] reaches |

The lead investigates the other five itself (an operation's or pairing's final refusal never
reported: 17163374614648705049, 5346, 5434; devices left `Reconciling` with 96 edits never
acknowledged: 5459; a `Cmd` append in the last 1,800,000 ms: 5797): leave those alone.

## What to do

1. For each of the twelve seeds above (`TD_SIM_SEED=<seed> TD_SIM_CONFIG=gate`, `one_seed`, with
   `TD_SIM_FULL=1` and `TD_SIM_ALL=1`), find why it fails, with the trace lines that show it, and
   put it in one of three classes:
   - **a check fault**: the run does what §31.7 allows, and the simulator's check (or what the
     run records for it) says otherwise. Fix it, with a hand-built test that fails before the fix
     and passes after it, and say which seeds it settles.
   - **an engine fault**: the engine did something the spec forbids, or did not do what it
     requires (name the clause, the device, what it did and the trace lines). Do not fix it and do
     not change the check to pass it: report it. The lead fixes the engine.
   - **a spec question**: §31.7 or the rest of the spec is silent or contradicts itself for what
     the run did. Report it with the run and the readings; do not decide it.
   Several seeds may share one cause: say so, and give each cause once with every seed it covers.
2. Run the gate as D7 did after your changes: `gate` at bases 100 (50 seeds), 1000 (100) and 2000
   (300), and at 17163374614648704531 and 5000 with 1,000 seeds each; every named configuration
   with 12 seeds at base 100; `--test regressions` (every file must pass; you change none). Report
   every seed that still fails with its class.

## Files you may change

`crates/taskdeck-sync/src/sim.rs` (not the splitmix and relay lines), files under
`crates/taskdeck-sync/src/sim/` except `splitmix.rs`, `relay.rs` and `relay/`,
`crates/taskdeck-sync/tests/simulator.rs`, `crates/taskdeck-sync/tests/regressions.rs`, and
`docs/sync/status/engine/api-simulator.md` (say what changed, cut what no longer holds). Not
`crates/taskdeck-sync/tests/regress/**`; do not read `crates/taskdeck-sync/src/engine/**`.

## Done

The task's gate (unit tests, regressions, relay conformance, clippy with `-D warnings`, the wasm32
check with `--features sim`) passes in your worktree with your own `CARGO_TARGET_DIR` (clean the
crate there before the last run); your work is committed on your branch; your report gives each
cause once, with its class, its seeds, its trace lines, the test that pins a fixed check fault,
and the gate runs.
