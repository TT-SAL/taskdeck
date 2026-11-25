# Task D4: simulator tests that do not depend on what the engine does at a seed

Base: the commit that holds this brief. Read `d-simulator.md`, `d2-simulator-followup.md` and
`d3-gate-seeds.md` in this directory for the rules that still hold (the invariants are read
strictly and nobody loosens one; an engine fault is reported, not worked around; you do not read
`crates/taskdeck-sync/src/engine/**`; you change only the files this task lists).

## What is wrong

Four unit tests in `crates/taskdeck-sync/src/sim/world.rs` run a `gate` seed and assert what
happened in it:

- `a_device_that_applied_the_skip_target_is_held_to_the_oracle` (seed 1014: "device 2 crash-halted
  on 192, device 0 did not");
- `invariant_4_at_a_skip_window_is_judged_by_the_no_fault_oracle_only` (seed 1014: "the Skip of
  192");
- `a_refused_entry_register_acknowledged_is_named_a_pairing` (seed 1017: "the entry at 218 is
  refused");
- `a_pairing_lost_with_a_fork_is_named_and_a_raced_skip_is_not_a_pairing`.

The engine was fixed after they were written (the move after a restart and after a lost answer,
reports from before a Skip, a second Skip of one target), so those seeds now run differently and
all four fail. Every later engine fix will move seeds again. A test of the simulator must hold
whatever the engine does at a seed.

## What to do

1. Rewrite the four tests so that each builds the situation it checks. In order of preference:
   a `World` driven by scripted events with no random faults and no random operations; facts
   handed to the check through the hooks that exist (`tamper_facts` and the like); a ledger built
   by hand. Only if a situation can come out of nothing but a run: search a bounded range of seeds
   (at most 40 runs) for the pattern, take the first seed that shows it, and fail with a message
   that names the pattern when none does; say in the test's comment why it could not be scripted.
   No test may name a seq, a device number or a trace line that a seed happened to produce.
2. The same for every other test under `src/sim/` and `tests/simulator.rs` that pins a seed's run.
   Find them (run the unit tests, and read what each asserts). The files under `tests/regress/`
   are exempt: they record failing runs and only have to pass.
3. One finding of D3's check is still open: the rule "a `B` between a Skip's target and the Skip
   where the oracle confirmed nothing is a violation" has no test. Add one that fails when the
   rule is removed.
4. Then run `gate` with `TD_SIM_BASE=100 SIM_SEEDS=50` and with `TD_SIM_BASE=1000 SIM_SEEDS=100`,
   and every named configuration with 12 seeds at base 100. List every failing seed under exactly
   one cause. The lead's run on this commit, to compare with: base 100: 26 fail, each only with
   "lost to a rolled-back relay state"; base 1000: 59 fail, 57 only with that text, 1018 (the
   §28.4 step 4 text), 1098 (a crash halt on a Skip's target against invariant 4). Anything else
   is reported as: seed, configuration, first bad trace line, clause, the sequence in five lines.
   Do not fix the engine and do not adapt a check to it.

## Done when

The unit tests, `regressions`, `relay_conformance`, clippy with `-D warnings` and the wasm check
pass; each rewritten test fails when the rule it checks is removed from `check.rs` (say how you
tried it); `docs/sync/status/engine/api-simulator.md` says how the simulator's own tests are
built. Use your own `CARGO_TARGET_DIR`, and run `cargo clean -p taskdeck-sync` there after a
checker's copy was built in it.
