# Task F, step 2, `f2-gate`: determinism, rebase, fuzz targets, budgets, and the whole gate

Plan: SYNC_PLAN.md §6.2 item 9 and the gate. After this step `bash scripts/gate.sh core` passes every item
a container can run. Everything else of the crate is in the working copy.

| Read | Where |
|---|---|
| the property tests and the fuzz targets | core.md §18.6 |
| the budgets, the 10,000-row fixture and the budget workload | core.md §17 (the three CORE-PERF clauses and the paragraph under them) |
| what the gate runs | `scripts/gates/core.gate`, `scripts/gates/README.md` (the `core` row) |
| how a stage adds CI checks | `scripts/ci/stage.sh` (the contract at its top), `crates/taskdeck-core/ci.sh` |

## Build

1. **`tests/determinism.rs`** (the gate runs it twice, under `TZ=Europe/Helsinki` and `TZ=America/New_York`).
   A property test: random intents compiled by two or three devices (`device_no` 1 to 3, a minter each) against
   their views, in an explicit zone each (two different `TableZone`s) and, for one device, in
   `ChronoZone(chrono::Local)`; the commands applied in one order to replica A; replica B built from A's
   canonical bytes at a random point plus the rest of the log; equal canonical bytes and equal notices at the
   end (CORE-CTX-002, CORE-CONF-007, CORE-DATA-005). And one fixed case: a log generated from a fixed seed
   with explicit zones only, whose end board's SHA-256 is a constant in the test, so both `TZ` runs must reach
   the same bytes (the process zone must not reach `apply` or a compile with an explicit zone).
2. **`tests/rebase.rs`**. A property test of pending edits over confirmed ones: a device compiles a few
   commands on its pending view; meanwhile other devices' commands are sequenced; the pending commands are
   applied again, as they are (not compiled again), after the others, with later seqs and their original
   `based_on`. No panic; two replicas doing the same reach the same bytes, results and notices; a refused
   command leaves the board unchanged.
3. **Fuzz targets** in `crates/taskdeck-core/fuzz/` (a package with its own `[workspace]` table and its own
   committed `Cargo.lock`, as `cargo fuzz init` lays it out): `command_apply` (bytes through `parse_command`,
   applied to a fixture board), `board_bytes`, `intent_compile`, `ics_parse`, `snapshot`. Nobody runs them
   here (that is stage `release`'s nightly job); they must compile on the pinned stable toolchain:
   `crates/taskdeck-core/ci.sh` runs `cargo check --locked` on the fuzz package, on Linux only (`CI_OS`).
   `libfuzzer-sys` is pre-approved; pin a version published at least seven days ago and say which, with its
   date, in your report. Fixture data comes in through `include_bytes!`.
4. **Budgets** (CORE-PERF-001 to -003; SYNC_PLAN.md §3.3: the median of five runs, in CI). No target of this
   crate may read a clock, so the clock is the shell's: three examples, `examples/budget_snapshot.rs`,
   `budget_apply.rs`, `budget_checkpoint.rs`, each reading one integer `n` from standard input, doing its
   setup once and its workload `n` times. `ci.sh`, on Linux only, builds them with `--release`, and for each
   takes five times the pair of wall times `T(n)` and `T(0)` (`date +%s%N`), computes `(T(n) − T(0)) / n`,
   takes the median, prints the three numbers on lines that start `budget:`, and fails when one is over its
   limit (40 ms, 1,000 ms for the 5,000 commands, 150 ms). Give `ci.sh` a place for a recorded baseline per
   budget (empty now: the lead fills it from the first CI run) and fail when a result is more than 25%
   worse than its baseline. The workloads are the ones core.md §17 defines.
5. **`tests/budgets.rs`**: runs each of the three workloads once for its result, not its time (the snapshot
   equals the recorded one; the 5,000 commands all apply and the archive grows by 1,000 rows; the canonical
   bytes load and round-trip), citing CORE-PERF-001 to -003. Share the workload code with the examples
   through `tests/common/` (a file both `mod`-include by path is not allowed: put the shared code where
   both can `include!` it or keep it in the library behind no feature; choose the simplest that CI's reads
   check accepts, and say what you chose).
6. **The whole gate.** Remove `#![allow(unused_variables)]` from `src/lib.rs`. Make
   `bash scripts/spec-coverage.sh core` pass: it lists each clause the stage owns that no test file cites;
   for each, cite it at the test that proves it, or write the missing test. Then `bash scripts/gate.sh core`
   must pass every `run` item.
7. Loose ends the earlier steps left for this one: the crate's doc comment at the top of `src/lib.rs` still
   describes the skeleton and names the macro the gate scans for (`bash scripts/gate-lib.sh no-unimplemented core`
   fails on it): rewrite it for the finished crate. Two `SPEC-QUESTION` comments are left in `src/ics.rs` (the
   order the step budget is spent in; an observance cut short is not noted): both are settled as implemented
   by core.md §14.6, bound 7: replace them by citations. Cite CORE-READ-007 at the timed tests of
   `tests/ics_bounds.rs` and CORE-APPLY-005 where the size check is tested, if spec coverage asks for them.
8. Bring `crates/taskdeck-core/README.md` up to date: what the crate is, its modules, how its tests read
   fixtures, how to run the gate, the budgets and where their numbers are printed. Short.

## Points the lead has settled

1. The scans of the gate read the fuzz package and the examples too: no `#[ignore]`, no `todo!()`, no
   `allow` of a `disallowed_*` lint anywhere under `crates/taskdeck-core/`.
2. Standard input is not on the crate's list of forbidden things; the environment and the clock are. Do not
   work around the list in any other way.
3. If a budget misses its limit in the container, do not tune the limit or the workload: report the three
   numbers and where the time goes (a profile by halves is enough). The lead decides.
4. Change nothing outside `crates/taskdeck-core/`. If the gate needs a change elsewhere (a gate file, the
   path rules of CI), describe it under `for_lead`.

## Done

`bash scripts/gate.sh core` exits 0 in W (every `run` item), the step gate of `common.md` exits 0, and you
committed in W. Report the commit, the three budget numbers measured in the container, the `libfuzzer-sys`
version and date, and every `for_lead` point.
