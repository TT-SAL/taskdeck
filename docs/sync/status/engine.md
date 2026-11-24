# Stage `engine`: status

**State: in progress.** Not final. Lead: the session of 2026-11-24 on branch
`claude/cool-franklin-c6e6um`. The journal (`engine/journal.md`) holds every decision and finding
in the order it happened; this file is the current truth and is rewritten, not appended to.

## Resuming: read this first

1. Read this file, then the last 40 lines of `engine/journal.md`. Do not read `engine.md` (the
   spec) in full again: use the clause index
   (`python3 docs/sync/status/foundation/tools/clause-index.py docs/sync/spec /tmp/td-clauses.txt`,
   output outside the repository) and the sections a step names.
2. `git status`, `git log --oneline -5`, `git worktree list`. A restart loses running workflows:
   look at "Workflows" below and at each working copy (`/home/user/wt/<task>`, branch `wt/<task>`)
   before starting one again. A working copy with commits is work to integrate, not to redo.
3. The step in progress is under "Steps". The to-do list is `bash scripts/spec-coverage.sh engine`
   (clauses not yet cited) and `grep -rn 'unimplemented!()' crates/taskdeck-sync/src | wc -l`.

## How this stage is run (the owner's instructions, which override plan §3.1 and §7)

- **The lead writes the engine itself** (`Engine::handle`, everything `engine.md` §4 and §7 to §30
  decide, its unit tests and a small scripted driver). No agent edits it.
- **Agents (model `opus`, never Fable, at most two at a time) write what the engine is tested
  against, without reading the engine's implementation**: A the fake and the toy and their vectors;
  B the expected values of the vector families hash, pending, receipts, poison, duties, clock,
  reconcile (the lead writes the tests that load them; where engine and values differ, the spec
  decides); C the model relay and ENG-SIM-001; D the simulator (the lead makes the spec change for
  `FaultSet`, `OpSet`, `SimStats` from D's proposal); E, once the engine runs under the simulator,
  `engine-scenarios.json` and `engine-traces.json`. Each task is a workflow of the task shape:
  write, check once with reproductions by an agent that did not write it, fix high and medium once.
- Each agent works in a working copy of its own (`git worktree`, `/home/user/wt/<task>`, branch
  `wt/<task>`, never pushed) with its own `CARGO_TARGET_DIR`, and changes only its task's files.
  The lead integrates a task when it is done. The lead owns `Cargo.toml`, `src/lib.rs`,
  `src/sim.rs` (the simulator's module list and its spec-fixed public types) and every file two
  tasks share.
- **A failing seed:** find out why first. Engine wrong: the lead fixes it and the seed becomes a
  regression file (§31.8). Simulator, model relay or fake wrong: an agent fixes it (give it the
  seed, the first bad trace line, the clause). Spec wrong or unclear: a spec change in a pull
  request of its own. The lead never edits the simulator, the model relay or the fake to make a
  seed pass; nobody loosens an invariant.
- Small steps, each ending with its tests passing, committed and pushed. The engine goes under the
  simulator as soon as the first steps can run there.
- When `bash scripts/gate.sh engine` passes in the lead's own run: one round of checks by agents
  that did not write what they check (spec ×2, works, gate power, hostile parties ×2, hygiene
  last); findings count only with a reproduction; high and medium are fixed with a test that failed
  before; low are listed here. Then the exit as plan §3.5 says.
- Usage: Fable is the scarce part. Read large files once; never `src/fake.rs` or the simulator's
  sources in full; short command output.

## Steps (the engine, by the lead)

| # | Step | State |
|---|---|---|
| 0 | Skeleton brought to §37 as it is now; derives; `sim` for tests; module layout | done locally (see "Pull requests") |
| 1 | start, records, enqueue, the pending view, the naming hold | next |
| 2 | sessions, send, append results, reconcile, echoes | |
| 3 | receive: pages, dry run, persist, commit, apply; classes, refusals, notices | |
| 4 | poison pills, halts, Skip | |
| 5 | checkpoints, bases, hashes, retention | |
| 6 | beats, duties, snapshots, verdicts, compaction, the clock | |
| 7 | evidence, stops, repairs (restore tail), divergence and adoption | |
| 8 | epochs, re-sealing, revoked, missed roots, re-pair and resubmit | |
| 9 | operations: restore, purge, move, follow, revoke, pairing, close board | |
| 10 | status, pending, notices, restore targets, purge preview | |

Numbers at the start (2026-11-24, day-planner `99b510e`): `spec-coverage.sh engine` 233 clauses
owned, 2 cited, 1 citation of a withdrawn clause (SEAL-MEM-025 in `src/fake/tests.rs`);
`unimplemented!()` in `crates/taskdeck-sync/src`: 16 (15 in `lib.rs`, 1 in `sim.rs`).

## Map of the code (`crates/taskdeck-sync/`)

Types (the spec's §37, no logic; all `Clone, Debug, PartialEq, Eq`, small ones `Copy`):
`src/model.rs` (the two traits and the sealer's types, §5, §6), `events.rs` (`Input`, `Event`,
`Effect`, `AppendResult`, `LocalReadResult`, `NewDevice`, `RelayTarget`), `records.rs` (§7: every
persisted record, `PersistOp`, `Loaded`, `Bootstrap`), `session.rs` (`Welcome`, `AdminRequest`,
`AdminOk`, `RelayRefusal`, `SeedDevice`), `receive.rs` (`Page`, `PageEntry`), `snapshots.rs`
(blobs), `ops.rs` (`Operation`, `OpRecord`), `ui.rs` (§30: `Status` and its parts, `Notification`,
`UserAction`, banners, alarms, `Evidence`). `PersistOp` and `Effect` have a hand-written `Debug`
that prints only the length of `WriteIdentity`, and `PartialEq` only under `cfg(any(test, feature =
"sim"))` (ENG-GEN-010, ENG-GEN-011).

Planned layout of the engine (filled in as it is written; `src/engine/` is the lead's, agents do
not read it): `src/engine/mod.rs` (the struct, `handle`, the nine steps of §4.5), and one file per
step of the table above. Unit tests in files named `*_tests.rs` (what `spec-coverage.sh` counts
under `src/`), the scripted driver in `src/engine/driver_tests.rs`.

Agents' files: `src/fake.rs`, `src/fake/**`, `src/toy.rs`, `src/sim/splitmix.rs` (task A);
`src/sim/relay*` (C); the other files under `src/sim/` (D); `tests/vectors/*.json`;
`tests/simulator.rs`, `tests/regressions.rs`, `tests/regress/*.json` (D).

## Commands

- One test: `cargo test --locked -p taskdeck-sync --message-format=short <name>`
- All: `cargo test --locked -p taskdeck-sync --message-format=short 2>&1 | tail -5`
- Lints as CI runs them: `cargo clippy --locked -p taskdeck-sync --all-targets --message-format=short -- -D warnings`
- CI's steps for this stage: `bash scripts/ci/stage.sh engine` (then `crates/taskdeck-sync/ci.sh` when it exists)
- wasm: `cargo check --locked -p taskdeck-sync --target wasm32-unknown-unknown`, and again with `--features sim`
- The gate: `bash scripts/gate.sh engine`
- Does the skeleton match §37: `python3 docs/sync/status/engine/tools/apidiff.py` (prints nothing but the simulator's and the toy's names when it does)

## Decisions (with reasons) and deviations

- **L1. Tests get `sim` through a dev-dependency of the crate on itself**
  (`taskdeck-sync = { path = ".", features = ["sim"] }`). The gate's commands name no feature and
  must not pass with zero tests; `required-features` would make them fail to start. Costs one line
  in `Cargo.lock`, so it went first in a pull request of its own (plan §3.2).
- **L2. Derives on every public type** (`Clone, Debug, PartialEq, Eq`; `Copy` on the small ones).
  §37 fixes names and fields, not derives; the engine, its tests and `device` need them. The
  identity export never prints and is never compared outside test builds (see the map).
- **L3. `src/sim.rs` is the lead's** (module list, the public types §37 fixes); the simulator's
  code lives in files under `src/sim/`. `SplitMix64` moved to `src/sim/splitmix.rs`.
- **L4. One branch, pull requests in sequence.** The session may push only to its own branch, so a
  pull request "of its own" (shared files, a spec change) is the branch's only commits at that
  moment: push, wait for CI, squash-merge, restart the branch from `day-planner`, rebase the rest.
  While such a pull request waits for CI, later work stays local.

## Pull requests

| # | What | State |
|---|---|---|
| TT-SAL/taskdeck#27 | the dev-dependency on itself with `sim` (L1) | open, CI running |

## Workflows (agents)

| Id | Task | Brief | State | Result | Tokens |
|---|---|---|---|---|---|
| | A fake, toy, vectors splitmix/toy/fake | `engine/briefs/a-fake-toy.md` | about to start | | |

## Open threads

None yet.

## Spec questions met

- E2 (left open by `seal`): an own `Fence` that loses a rotation race leaves the old relay fenced
  with no accepted `Fence` (§28.3, §28.7). To decide when step 9 is built; may need a change under
  `relay/`.
- E4 (left open by `seal`): §24.3 re-seals a pending revocation's rotation, and §28.5 composes it
  again; the second record is refused `seal/ctl_target`. To decide in step 8.
- Left open by `foundation` (T21): whether `MoveReport` carries the seed's first seq; whether the
  shared picture is uploaded again after a move.
- Left open by `seal` (Q79): nothing binds the log between the bundle's head and `added_seq`; the
  remedy (the bundle carries the chain value at `added_seq`) is `engine`'s choice.

## What the next stages need to know

(collected as the stage goes)

- Test builds of `taskdeck-sync` always have the feature `sim` (L1); other builds never do.
- The public types derive `Clone, Debug, PartialEq, Eq` (L2); `Effect` and `PersistOp` are
  comparable only in test builds or with `sim`.
- The skeleton had kept the withdrawn recovery-key path (`RecoveryMode`, `PendingRecovery`,
  `Operation::ProposeRecoveryKey` and so on); it is gone, as §37 states.
