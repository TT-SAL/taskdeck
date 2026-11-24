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
| 0 | Skeleton brought to §37 as it is now; derives; `sim` for tests; module layout | done |
| 1 | start, records, enqueue, the pending view, the naming hold | written and tested (`core_tests.rs`, `engine-pending.json`) |
| 2 | sessions, send, append results, reconcile, echoes | written; tested: `engine-reconcile.json` (welcomes, conflicts, gaps), `sess_tests.rs` (displaced, budget, echoes, tampered page, restored copy) |
| 3 | receive: pages, dry run, persist, commit, apply; classes, refusals, notices | written; `engine-receipts.json` passes against `ScriptedSealer` (`script_tests.rs`) |
| 4 | poison pills, halts, Skip (operation, frames, replay) | written; `engine-poison.json` passes (`poison_tests.rs`); read-ahead, Skip and Skip replay flows untested |
| 5 | checkpoints, bases, hashes, retention | written; `engine-hash.json` passes; cadence, bases, retention untested |
| 6 | beats, snapshot duty, verdicts, compaction, the clock, upgrade proposal, purge duty | written; the pure decisions pass `engine-duties.json` and `engine-clock.json`; the flows untested |
| 7 | evidence stop, divergence stop, restore tail (repair.rs), adoption (ops.rs), rejoin and rebootstrap (boot.rs) | written; evidence stop tested; the rest untested |
| 8 | epoch gate, re-sealing, revoked, missed roots, export and resubmit | written; epoch gate and a revoked device tested (`sess_tests.rs`); resubmit untested |
| 9 | operations, pairing (ops.rs), the move, the move code, follow address (movement.rs) | written, untested (needs the model relay, task C) |
| 10 | status, pending, notices, restore targets, purge preview | written; partly tested |
| 11 | rules for every implementation (§4, §36 source rules, limits, clocks, persist failure, no deletion) | tested (`rules_tests.rs`) |

**Step in progress:** TT-SAL/taskdeck#28 to green and merged; then the spec-change pull request
(the list is under "Spec changes to make"); then task C merged, task D launched, the engine under
the simulator. Meanwhile: flow tests for the clauses not yet cited.

Every section of the spec has code. Left to test (37 clauses, `bash scripts/spec-coverage.sh engine`):
- with `Node`/`Hub` (`kit_tests.rs`; `flow_tests.rs` has 9 flows): ENG-CTL-002 (needs a refused own
  rotation), ENG-RULES-001/002 (needs a kit device with `rules2`), ENG-SNAP-004/005 and ENG-UI-003 (need
  blobs: the model relay), ENG-SKIP-003, ENG-DIVERGE-001..004 and ENG-SEALER-003 (scripted sealer),
  ENG-SESS-007.
- with the model relay (`rkit_tests.rs` is the lead's driver over it, poll sessions; `relay_tests.rs` has
  pairing, revoke, restore to a point, purge, the snapshot duty, compaction): the move (ENG-MOVE-*),
  ENG-OPS-010 (close board), restore tail (ENG-EVID-004/007), rejoin (ENG-BOOT-*), ENG-OPS-003/005.
- RELAY-* and ENG-SIM-* citations come with tasks C and D.

The tests' own relay (`Hub` in `kit_tests.rs`) answers only Connect, Hello, Read and Append.

Numbers at the start (2026-11-24, day-planner `99b510e`): `spec-coverage.sh engine` 233 clauses
owned, 2 cited; `unimplemented!()` in `crates/taskdeck-sync/src`: 16.
Now: `unimplemented!()` 1 (`sim.rs`: `run`, task D's); `spec-coverage.sh engine`: **196 of 233
cited, 37 uncovered, 0 bad**. 122 lib tests, 16 relay conformance tests and 4 integration tests pass; all 197 cases of task
B's seven families pass.

## Map of the code (`crates/taskdeck-sync/`)

Types (the spec's §37, no logic; all `Clone, Debug, PartialEq, Eq`, small ones `Copy`, a few
`Default`): `src/model.rs` (the two traits and the sealer's types, §5, §6; `Sealer::membership_zero`
is new, spec change pending), `events.rs`, `records.rs` (`Loaded.chains` is `(seq, chain, tau)`,
spec change pending), `session.rs`, `receive.rs`, `snapshots.rs`, `ops.rs`, `ui.rs`. `PersistOp`
and `Effect`: hand-written `Debug` (identity bytes never print), `PartialEq` only in test builds.
`src/lib.rs`: module list, re-exports, `state_hash` (§18.3).

The engine, `src/engine/` (the lead's; agents do not read it). One struct `Engine<SM, S>` in
`mod.rs`; every other file is an `impl` block on it plus one small state struct held in a field.

| File | Field | Holds | Spec |
|---|---|---|---|
| `mod.rs` | — | the struct; `handle` (dispatch); `steps()` = the nine steps of §4.5; ids; `persist()` (every Persist goes through it), `durable`, `after(pid, After)`; `flush_meta`/`touch_meta`; `notify`/`notify_durable`; `alarm`, `add_stop`/`end_stops`, `fault`, `protocol`, `halt`; `stopped()`, `no_apply()`; the chain index (`chain_at`, `tau_at`, `push_chain`, `trim_chains`); `Want` (why a ReadLog/ReadBase/Admin/Blob was emitted) with `want`/`take_want`/`want_lost`; `on_local_read`/`on_admin_result`/`on_blob_result`; `step_receive`; `step_tick`; `lim` (the limits of §34) | §4, §7.12 |
| `start.rs` | — | `start(Loaded)`, `reset_process`, `load_checkpoint`, `genesis`, `join`, `install_snapshot` (also for a rejoin) | §7.10, §19.8, §16.1 clearing |
| `outbox.rs` | `outbox: Vec<Rec>` | `Rec` (record + parsed command, `pid`, `held_at`, `doomed`, `prov`); `rebase`, `trial`, `on_enqueue`, `enqueue_cmd`, hold (`last_is_held`, `end_hold`, `merge_into_held`); `compose`/`enqueue_own` for ctl, beat, snap; `export_unsent`, `on_resubmit` | §7.4, §8, §9, §24.5 |
| `send.rs` | `send` | `may_send`, `step_send` (the loop of §10.2), `next_ctr`, `seal_record`, `send_carried` (Register/Fence), `on_append_results`, `acknowledge`, `echo_entry`, `on_conflict`, `on_gap`, `on_frame_refused`, `restored_clear`, `carried_result`, `forget_request`, `intern` (relay codes as `&'static str`) | §10, §11.4, §11.5, §11.7, §24.2 |
| `sess.rs` | `sess` | the session table (`step_session`), `step_reads` (reads, run end, Live), `on_welcome` (checks, reconcile), `recon_progress`, `on_page` (head checks, then `take_page`), `on_disconnected`, `drop_requests`, second/displaced, `on_relay_refused` | §11, §12.2 |
| `recv.rs` | `pipe.flight`, `pipe.stash` | `Entry` (a classified entry), `take_page` (limits, contiguity), `run_stash`, `dry_run` (classification; the only caller of `Sealer::classify` besides genesis), `finish_page` (the page's Persist), `fetch_skip_frames`, `page_durable` (the commit), `commit_entry` (alarms of the class, `seen`, halts, membership effects, banners) | §12, §13.1, §14, §17.2 |
| `board.rs` | `pipe` | `Pipe` (queue, batches, marker, `pre`/`redo`, records to write); `plan_tail`, `run_board`, `apply_next`, `on_apply_panic`, `crash_halt`, `settle_own` (the own record: drop, refusal record, re-seal), `reseal_from`, `apply_ctl`, `apply_beat`, `apply_snap`, `aligned` (hash, base), `checkpoint`, `write_base`, `write_checkpoint`, `step_checkpoint`, `on_shutdown`, `retention`, `truncate_log` | §12.4, §13, §15, §16, §18, §7.13, §24.3 |
| `duty.rs` | `duty` | pure: `duty_order`, `rank_of`, `usable`, `beat_due`, `snapshot_due`, `clock_state`; `eligible`, `step_duties`, beats, the snapshot duty (`snapshot_due_at`, `snapshot_step`, `snapshot_base`, `snapshot_uploaded`), compaction | §19.2 to §19.5, §20, §23, §27, §29 |
| `evid.rs` | `evid` | `raise_evidence`, `resolve_evidence`, `check_divergence`, `enter_divergence` | §21.1, §21.2, §22.1, §22.2 |
| `epoch.rs` | `epochs` | `set_revoked`, `clear_revoked`, `relay_answered`, `revoked_recheck`, `on_missed_roots` | §24.4 |
| `status.rs` | — | the public read accessors, `status()`, `pending()`, `notices()`, `on_user` | §30 |
| `replay.rs` | `jobs` | `Job` (a replay from a base: `ReadBase`, `ReadLog` chunks, classify and apply again into its own state; `want_states` stashes states for `ResetTo` targets; `swap` for an adoption; `from_rec` collects records and hashes), `Purpose` (Skip, Reset, RestoreHash, AdoptBoard, Seed), `start_job`, `job_base`, `job_log`, `job_step`, `job_done`; `begin_skip_replay`/`skip_replayed`, `begin_reset`/`reset_replayed`, `replace_records_from` | §17.3, §28.1, §13.4 |
| `ops.rs` | `opx` | `OpMem` (per operation: the stage's write, busy), `on_op` (checks, then per operation), `drive_ops`, `op_begin`/`op_stage`/`op_done`, Skip (`skip_allowed`, `skip_compose`), revoke (`revoke_compose`), purge (`purge_preview`, `local_purge`, `shred_step`, `purge_step`), restore (`restore_ok`, `restore_targets`, `restore_hash_ready`), adoption (`adopt_fetch`, `adopt_blob`, `adopt_board_ready`), `upgrade_step`, `own_entry_done` (an own record settled: moves its operation on), pairing (`on_pair_register`, `pair_uploaded`, `before_carried_seal`, `carried_done`, `pair_entry_done`), `host_ok`, `crockford` | §17.1, §22.3, §25, §28 |
| `movement.rs` | `opx.mv` | the move in stages 1 to 6 (`move_drive`, `move_admin`, `move_seed`, `seed_replayed`, `move_tail`, `move_log`, `move_blob`, `move_report`), `fence_accepted`, `on_move_code`, `switch_relay`, follow address (`follow_begin`, `follow_welcome`, `follow_page`: a dry-run scan that persists nothing) | §28.3 |
| `repair.rs` | `evid.tail` | restore tail: `repair_on_welcome`, `repair_step`, `restore_chunk`, `repairs_for`, `restore_result`, `repair_result` | §21.3 |
| `boot.rs` | `boot` | hooks only so far | §19.7, §19.9 |
| `vectors_tests.rs` | — | loads task B's families: hash, clock, duties, pending | §35 |
| `kit_tests.rs` | — | the lead's driver: `Store` (applies PersistOps, builds `Loaded`), `Node` (engine + store + clocks; `feed`, `persist_one`, `restart`, `enqueue`), `Hub` (a small relay: `append`, `welcome`, `page`, `serve`, `settle`), `creator(seed)`, `join(hub, a, seed, name)` | — |
| `core_tests.rs` | — | tests of steps 1 to 3 | — |

Rules between the parts (see also the header of each file):
- `L >= K >= B`. `pipe.queue` holds the classified entries `B+1..=K`. `mk`/`vk` are `M_K` and its
  cached view; `mb` is `M_B`; `no_b` this device's `device_no` in `M_B`.
- A record's `seen` is set at the commit (recv.rs), it is dropped at the board apply (board.rs
  `settle_own`); `ack` comes from append results (send.rs).
- Nothing emits `Effect::Persist` except `Engine::persist`. A dependency of §7.12 is either a
  remembered write id compared with `durable` on every call, or an `After`.
- A stop is a `StopRecord` in `meta.stops`; every stop holds back sending (`stopped()`); only
  Evidence, LocalFault, Stranded and Closed hold back reading (`no_apply()`); the board is held
  by a crash or rules halt at its seq, a local fault, or `Rebootstrap` (`board_held()`).

Agents' files: `src/fake.rs`, `src/fake/**`, `src/toy.rs`, `src/sim/splitmix.rs` (task A);
`src/sim/relay*` (C); the other files under `src/sim/` (D); `tests/vectors/*.json`;
`tests/simulator.rs`, `tests/regressions.rs`, `tests/regress/*.json` (D).
`docs/sync/status/engine/api-fake-toy.md` is what to call on the fake and the toy.

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
| TT-SAL/taskdeck#27 | the dev-dependency on itself with `sim` (L1) | merged (squash `cb8ac72`); two macOS runner cancellations re-run |
| TT-SAL/taskdeck#28 | the engine as written so far, the fake and the toy, the first vectors (stage in progress, not the exit) | draft, open. First CI run failed on Windows only: a test of task A reads `Cargo.toml` and `src/lib.rs` with `include_str!` and the checkout had CRLF; fixed with `crates/taskdeck-sync/.gitattributes` (`* -text`, as the seal crate does) |

Order (L4, L5): #28 merges; the branch restarts from day-planner; the spec change is the branch's
only commit in a pull request of its own; then work goes on.

## Spec changes to make (one pull request, after #28)

1. `Loaded.chains` carries τ: `Vec<(u64, Hash32, i64)>` (the code is ahead of §37).
2. `Sealer::membership_zero(&self) -> Self::Membership` (the code is ahead of §37).
3. `classify` may be called again for a seq and answers the same.
4. A `Hello` on a push session when a duty waits only for a fresh head (§29.1's 600,000 ms).
5. Fields of `FaultSet`, `OpSet`, `SimStats` (proposed by task D's writer).
6. Code `engine/restore_target` for a refused `RestoreTo` (§33 has none).
7. Wording: `registry.restored` ("persisted in the next WriteMeta" but Registry lives in the
   checkpoint); `meta.prev_session`; §6.1 history against the rows of `export_identity`;
   SEAL-FRAME-005 against re-sealing a freed counter; ENG-REF-001 for an own beat, snap or ctl.
8. The 14 questions of `tests/vectors/README.md` and those of `api-fake-toy.md`.
Still to decide: E2 (may need a change under `relay/`, a pull request of its own), T21, Q79.

## Workflows (agents)

All run the script `engine-task` (write, check once, fix once; agents on `opus`), saved by the
harness under the session directory; its args are `{id, brief, dir, target, paths, gate}`.

| Run | Task | Brief | State | Result | Tokens |
|---|---|---|---|---|---|
| wf_75ec09fc-a39 | A fake, toy, vectors splitmix/toy/fake | `engine/briefs/a-fake-toy.md` | done, merged | the fake follows seal.md as it is now (14 differences fixed), the toy unchanged, 3 vector files, `api-fake-toy.md`; check clean | 756,180 |
| wf_3b93dd8a-626 | B seven vector families | `engine/briefs/b-vectors.md` | done, merged | 197 cases, generators under `engine/tools/vectors-b/`, 14 spec questions in `tests/vectors/README.md`; 1 high and 2 medium fixed, 1 low (a clock case's name) | 777,276 |
| wf_4e7bb2da-24b | A2 fake follow-up | `engine/briefs/a2-fake-followup.md` | done, merged | `seal` no longer returns a stored frame for a reused counter unless entry and epoch are the same; `membership_zero`, `restore` | 341,974 |
| wf_6790fa34-60e | C model relay, first attempt | `engine/briefs/c-model-relay.md` | lost: the owner's usage limit cut the writer off | nothing kept beyond commit 678fdc2 on `wt/c` | 265,340 |
| wf_e736e661-ee0 | C model relay, second attempt | `engine/briefs/c-model-relay.md` | done, merged (the lead ran its gate: 16 conformance tests pass) | `src/sim/relay.rs` + `relay/{seq,admin,blobs,socket,util}.rs`, `tests/relay_conformance.rs` (11 families of `relay/test/vectors/`, about 575 steps; 7 families left out with reasons in the file header), `engine/api-model-relay.md`; check: no findings; 8 questions on relay.md; asks the fake for an accessor of the genesis `rsign_pk` and `salt` | 532,591 |
| wf_70e5dcc6-438 | D simulator | `engine/briefs/d-simulator.md` | running since 2026-11-24 20:50 UTC in `/home/user/wt/d` (branch `wt/d`, target `/home/user/wt/target-d`); task id wddgelgx0 | | |

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
