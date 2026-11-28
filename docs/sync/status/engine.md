# Stage `engine`: status

**State: paused before its exit** (the owner's decision of 2026-11-27, below). Not final. Lead:
the session on branch `claude/cool-franklin-c6e6um`. The journal (`engine/journal.md`) holds every
decision and finding in the order it happened; this file is the current truth, rewritten, not
appended to. It has two readers: the device session that works while the stage is paused, and
the lead when the stage goes on.

## The owner's decision of 2026-11-27

The stage pauses before its exit.
A device session first wires the real core, seal and relay into the engine and runs one board end
to end (those parts have only met fakes so far); whatever it finds comes back here before the
exit review, so the review runs once. Before the pause the lead does, in order: E2 with the next
spec change (one pull request: §7.12, §17.3 with `WriteBase`, §28.3) and the engine fix; the
gate of plan §6.5 only (the 1,000 seeds the CI step runs; no further bases or sample sets, that
is the nightly job's, §6.11; a failing seed met anyway is listed here as open); the engine's pull
request "(stage in progress, not the exit)" merged with CI green on the three systems; this file
rewritten for the device session and for the lead's later self; then stop (no task E, no exit
review). While the stage is paused, the device session may change `crates/taskdeck-sync/`
through small pull requests of its own, noted in both status files. On 2026-11-28 E2 proved
larger than proposed (its followers need a simulator task), and the owner chose to leave it
open: the spec change carries §7.12 and §17.3 with `WriteBase` only (TT-SAL/taskdeck#38).

## For the device session

The engine has met only stand-ins so far: the fake sealer, the toy state machine and the model
relay. The device session wires the real `core`, `seal` and `relay` in and runs one board end to
end; what it finds comes back to this stage before the exit review.

**Where things are** (`crates/taskdeck-sync/`, a library with no I/O):
- The engine: `Engine::<SM, S>::new(sm, sealer, config)` and `Engine::handle(Input) -> Vec<Effect>`
  (`src/engine/mod.rs`); the read accessors `status()`, `confirmed()`, `pending()`, `notices()`
  (`src/engine/status.rs`). The public API is `engine.md` §37; what one `handle` call does is §4.
- The two traits the real crates implement: `StateMachine` (`src/model.rs:47`, for `core`;
  `engine.md` §5) and `Sealer` (`src/model.rs:328`, for `seal`; §6). The other public types are in
  `src/{events,records,session,receive,snapshots,ops,ui}.rs`; `src/lib.rs` re-exports them and
  holds `state_hash` (§18.3).
- The stand-ins and how to call them: the fake sealer (`src/fake.rs`, `src/fake/`) and the toy
  (`src/toy.rs`): `docs/sync/status/engine/api-fake-toy.md`; the model relay (`src/sim/relay*`,
  checked against `relay/test/vectors/` by `tests/relay_conformance.rs`): `api-model-relay.md`;
  the simulator (`src/sim/`, `tests/simulator.rs`, `tests/regressions.rs`): `api-simulator.md`.
- The runtime's side of every effect (the store's action for each `PersistOp`, `ReadBase`,
  `ReadLog`, sessions, ticks) is `device.md`; the order of writes is `engine.md` §7.12.

**What I would warn the device session about:**
- Persistence: each write is one `Effect::Persist`; apply its ops in order and answer `Persisted`
  in the order of the writes. A local read must see every write emitted before it (§7.12 D11).
  `WriteBase` creates `base-<seq>.tdk` or replaces it (TT-SAL/taskdeck#38): a Skip replay writes
  bases again.
- The engine reads no clock: `wall_ms` and `mono_ms` arrive with the inputs, and it asks for ticks
  with `ScheduleTick`. The simulator delivers each tick at the time asked, the runtime renews a
  stale token itself, and a poison entry is produced as `api-simulator.md` says: these are the
  simulator's rulings, in no spec text yet.
- Every call into `SM` and `Sealer` runs under `catch_unwind` (`guard`, `src/engine/mod.rs`): a
  panic there becomes a halt or the local fault `engine/local_fault`. The release profile unwinds
  (`scripts/check-release-panic.sh` proves it); the real crates must leave nothing broken behind a
  panic. `classify` must be repeatable: the same input gives the same receipt (§6).
- The fake follows `seal.md` as it was on 2026-11-25 (the differences found then are fixed and
  listed in `api-fake-toy.md`); the toy is not `core` (its commands, notices and refusals are its
  own). Where a real crate reads the spec otherwise than its stand-in, bring the difference here.
- Test builds of `taskdeck-sync` always have the feature `sim` (L1); `Effect` and `PersistOp`
  compare only in test builds or with `sim` (L2). The skeleton's withdrawn recovery-key path is
  gone, as §37 states.
- Known open, E2 (below): a move whose own `Fence` loses a race with a key change leaves the old
  relay fenced with no accepted `Fence`, and the board has nowhere to write. An end-to-end run that
  changes keys during a move can meet it.
- While the stage is paused the device session may change `crates/taskdeck-sync/` through small
  pull requests of its own, noted in its status file and under "Changes by the device session"
  below. The stage's rule holds for everyone: the simulator, the model relay and the fake are not
  changed to make a seed pass, and no invariant is loosened.

## For the lead, when the stage goes on

**Where things stand (2026-11-28):** everything the stage built is on day-planner: the spec
change TT-SAL/taskdeck#38 and the engine's pull request "(stage in progress, not the exit)" that
carries this file, merged with CI green on the three systems (its checks hold the gate's run: the
1,000 seeds from the id of its merge commit, ENG-SIM-004). At that pull request: every unit test,
`regressions` (26 files), `relay_conformance` (16), clippy and the wasm check pass;
`spec-coverage engine` 244 of 244; `unimplemented!()` 0. Open: E2, and the lows below.

**Left to do, in order:**
1. What the device session brought back (its status file, and "Changes by the device session"
   below): engine faults fixed by the lead, each with a test that failed before; spec changes in
   pull requests of their own (plan §3.3).
2. E2, if the owner wants it before the exit (see "Open, and lows").
3. Task E (agents): `engine-scenarios.json` and `engine-traces.json`.
4. `bash scripts/gate.sh engine` in the lead's own run.
5. The one round of checks: spec ×2 (sections 4–17; 18–30 with plan §6.5, one also reading this
   file for `device` and §36), works, gate power (faults injected into a scratch copy of the
   engine; report those 1,000 seeds do not catch), hostile parties ×2, hygiene last; agents on
   `opus`, at most two at a time, none checking what it wrote; findings only with a reproduction;
   high and medium fixed with a test that failed before; low listed here. One round only.
6. The exit pull request as plan §3.5 says (what was built, what was measured, what the next
   stages need); this file `final`; tell the owner which sessions to start next.

**On return:** read this file, then the end of `engine/journal.md`. `git fetch`; restart the stage
branch from day-planner (it holds everything). For the spec use the clause index
(`python3 docs/sync/status/foundation/tools/clause-index.py docs/sync/spec /tmp/td-clauses.txt`,
output outside the repository), never `engine.md` in full. The to-do list: `bash
scripts/spec-coverage.sh engine` and `grep -rn 'unimplemented!()' crates/taskdeck-sync/src | wc -l`.
A restart loses running workflows: look at "Workflows" and at each working copy before starting
one again.

## Changes by the device session

None yet. The device session notes here each pull request of its own that changes
`crates/taskdeck-sync/` while the stage is paused.

## Open, and lows

- **E2 (`gate` 17163374614648704878 at base 17163374614648704531; first named by `seal`).**
  Device 2's move sealed its `Fence` before it had read entry 67 (device 1's key change, a new
  epoch); the old relay's `fence` stored it at 68 and fenced itself; every device classified 68
  `seal/stale_epoch`, the mover ended the move with that code (`move_entry_done` in
  `movement.rs`), and every device then met `relay/fenced` with no accepted `Fence`
  (`relay_protocol` alarms). The engine alone cannot close the race (the relay's `fence` has no
  head condition). A fix needs both halves: the mover goes on as for a relay that is gone (seed
  with `last_seq = L`, its `Fence` appended to the new relay in stage 5), and the followers follow
  by address (`FollowAddress`); the simulator drives neither `FollowAddress` nor `OldRelayGone`,
  and its users type a move code only while their log holds an accepted `Fence`
  (`src/sim/world/load.rs`), so that half is a simulator task. A test can build the race: B's key
  change sequenced between A's sealed `Fence` and its `admin:fence` (`serve_until_admin`). The
  owner decided on 2026-11-28 to leave it open.
- **Three seeds of the engine pull request's first CI set (base 3996381313176633817, the merge
  commit 3775ffa), not yet diagnosed; the owner is asked how to go on:**
  `gate` 3996381313176633862 (fork: device 1 raises `relay_behind` at a welcome showing 250 and stays
  `Reconciling` to the end, no stop, no read; invariants 3 and 6); 3996381313176634032 (rollback and
  withholding: the check says a restore tail could have brought back seq 61 and its device sent none
  in time; invariants 2 and 3 on devices 1 to 3); 3996381313176634077 (rollback, fork, two made-up
  frames: device 1's entries acknowledged at 288 to 292 are not in the reference log; invariant 2).
  Each is either an engine fault in the restore tail under a fork or rollback, or the check's
  (a simulator task).
- Lows and questions for the round of checks (no seed fails for them today; details in the
  journal): a move ends every alarm (`mv_switch`); `RelayBehind` can end at a relay head above `L`
  without the chain at `L` (`gate` 5463, its first part); a `FollowAddress` whose candidate relay
  refuses the login waits without a time limit; a `ResetTo` inside a Skip window (§28.1 against
  §31.7, task D8); task D9's lows (coverage read from `log_reach` on any branch; a crash cut while
  sealing counted) and questions (a beat refused late in a run; a snapshot duty counted alone); the
  trial apply is `apply` on a copy, not `SM::trial_apply`; a `Disconnected` also drops requests in
  flight to another relay (sent again; harmless).

## Pull requests and spec changes

All from this branch, one at a time (L4); every spec change a pull request of its own (§3.3).

| Pull request | What | On day-planner |
|---|---|---|
| TT-SAL/taskdeck#27 | the crate's tests get the feature `sim` (L1) | cb8ac72 |
| TT-SAL/taskdeck#28 | the engine as first written, the fake, the toy, the model relay, the first vectors | d21e05e |
| TT-SAL/taskdeck#29 | spec: tau in `Loaded.chains`, `Sealer::membership_zero`, a Hello for a fresh head, `engine/restore_target` | e38d44e |
| TT-SAL/taskdeck#30 | the engine's work after #29 | closed for a parking cycle; carried by the engine's pull request of 2026-11-28 |
| TT-SAL/taskdeck#31 | spec: the owner's decisions on divergence, sign-in and Skip | 5b999eb |
| TT-SAL/taskdeck#32 | spec, gate: removed relay state, a final refusal, the Skip window (§31.7) | fc18f4c |
| TT-SAL/taskdeck#33 | spec: a fenced relay is read up to its `Fence` (ENG-RECV-007) and other rules from the gate | 561d084 |
| TT-SAL/taskdeck#34 | spec, gate: Q7, Q8, the oracle follows every move | d96b360 |
| TT-SAL/taskdeck#35 | spec: Q10, the evidence cut, ENG-CLASS-003, five clarifications | eda5051 |
| TT-SAL/taskdeck#36 | spec, gate: Q9, a duty entry refused with a re-sealable code | fa138b2 |
| TT-SAL/taskdeck#38 | spec: §7.12 D13, a Skip replay writes again the bases from its target on, `WriteBase` replaces | e5b4c98 |
| the engine's pull request of 2026-11-28 | the engine's work since #28, with this file | merged with it |

Not yet in any spec text: the simulator's rulings above; the 14 questions of
`tests/vectors/README.md` and those of `api-fake-toy.md`, `api-model-relay.md`,
`api-simulator.md`.

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

## Map of the code (`crates/taskdeck-sync/`)

Types (the spec's §37, no logic; all `Clone, Debug, PartialEq, Eq`, small ones `Copy`, a few
`Default`): `src/model.rs` (the two traits and the sealer's types, §5, §6; `Sealer::membership_zero`, TT-SAL/taskdeck#29), `events.rs`, `records.rs` (`Loaded.chains` is `(seq, chain, tau)`), `session.rs`, `receive.rs`, `snapshots.rs`, `ops.rs`, `ui.rs`. `PersistOp`
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
| `boot.rs` | `boot` | rejoin and rebootstrap | §19.7, §19.9 |
| `vectors_tests.rs` | — | loads task B's families: hash, clock, duties, pending | §35 |
| `kit_tests.rs` | — | the lead's driver: `Store` (applies PersistOps, builds `Loaded`), `Node` (engine + store + clocks; `feed`, `persist_one`, `restart`, `enqueue`), `Hub` (a small relay: `append`, `welcome`, `page`, `serve`, `settle`), `creator(seed)`, `join(hub, a, seed, name)` | — |
| `core_tests.rs` | — | tests of steps 1 to 3 | — |
| other `*_tests.rs` | — | `flow_tests.rs` (flows on the hub), `rkit_tests.rs` (the lead's driver over the model relay) and `relay_tests.rs` (flows on it), `sess_tests.rs`, `reconcile_tests.rs`, `poison_tests.rs`, `script_tests.rs`, `rules_tests.rs`, `vectors_tests.rs` | — |

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
- The gate's simulator step as CI runs it: `SIM_SEEDS=1000 cargo test --locked -p taskdeck-sync --test simulator` (the seeds start at the first 16 hex digits of the commit id, ENG-SIM-004; `TD_SIM_BASE=<n>` sets the start)
- One seed, every violation: `TD_SIM_SEED=<n> TD_SIM_CONFIG=gate TD_SIM_ALL=1 cargo test -q --release -p taskdeck-sync --test simulator one_seed -- --exact --nocapture` (`TD_SIM_FULL=1` for the trace; strip the poison entries' backtraces first, the journal has the filter)

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
| wf_70e5dcc6-438 | D simulator | `engine/briefs/d-simulator.md` | written and checked; the fix pass was cut off by the usage limit; applied to the branch as a patch | `src/sim.rs`, `src/sim/{config,store,trace,check,world}.rs`, `world/{rt,load,end}.rs`, `tests/simulator.rs`, `tests/regressions.rs`, `engine/api-simulator.md`; three helpers in the fake; quiet: 1,000 seeds pass (40 s release, 228 s debug); gate: 154 of 200 seeds failed against the engine of 7a764c7; 7 engine defects reported with seeds; 10 spec questions; the checker found 4 or more high defects in the invariants | 893,600 |
| wf_fbed9968-73a | D2 simulator follow-up (the check's findings, the lead's rulings, what still fails) | `engine/briefs/d2-simulator-followup.md` | done, applied to the branch as a patch (the lead ran its gate) | invariants read strictly, `sim/counted.rs`, regression files 1, 102, 111, 116, 200, a report of what still fails by cause (the journal, 2026-11-25) | 977,242 |
| wf_0eb7fcfb-f49 | D3 what still fails under `gate`, seed by seed (diagnosis; simulator faults fixed, engine faults reported) | `engine/briefs/d3-gate-seeds.md` | done, applied to the branch as a patch (the lead ran its gate) | `src/sim/{check,world}.rs`, `src/sim/world/{rt,load,end}.rs`, `api-simulator.md`; gate base 1000: 61 of 100 fail, base 100: 27 of 50 | 827,210 |
| wf_1623f41a-09b | D4 simulator tests, first attempt | `engine/briefs/d4-sim-tests.md` | lost: the container restarted nine minutes into the writer (54 turns); its 16 uncommitted lines were discarded | nothing kept | not reported (about 55,000 output tokens) |
| wf_a03961da-e96 | D4 (second attempt) simulator tests that do not depend on what the engine does at a seed; the gate classified again | `engine/briefs/d4-sim-tests.md` | done, applied to the branch as a patch from `wt/d4` 32a1637 (the lead ran its gate) | `src/sim/script.rs` (hand-built logs for the check), `src/sim/world.rs`, `api-simulator.md`; reported: gate 116 (a pairing refused after a restart dropped without `PairFailed`), 1021 (the crash-halt question again) | 471,311 |
| wf_fc162934-033 | D5 three check faults; every violation printed | `engine/briefs/d5-check-faults.md` | done, applied to the branch as a patch from `wt/d5` a868350 (the lead ran its gate) | `src/sim/{check,script,world}.rs`, `src/sim/world/rt.rs`, `tests/simulator.rs` (`TD_SIM_ALL`), `api-simulator.md`; one low: the crash-cut exemption for a pairing drop covers the drop's own call, not the next | 327,558 |
| wf_791ac2d4-7d3 | D6 the simulator's checks carry the owner's decisions (§31.7 invariants 2, 3, 4) | `engine/briefs/d6-decided-invariants.md` | done (write, two checks, two fix passes); applied to the branch as a patch from `wt/d6` b20dadf (the lead runs its gate) | `src/sim/{check,script,store,world}.rs`, `src/sim/world/{end,load,rt}.rs`, `api-simulator.md`: removed relay state from recorded acknowledgements, restore tails (sent, answered, owed) and made-up frames; never lost silently (an evidence alarm, durable, by a device holding the frame); a final refusal reported; the Skip window compared with a replay without t. Reported: `gate` 14 of 450 fail (fork plus re-pair ×10, 2272, engine 1022, 1073, 2154). Lows: `api-simulator.md` 204 lines (limit 100, 180 before); `relay/bad_signature` counts as a made-up frame's code (8 seeds) | 2,562,299 (5 agents, 819 tool uses, 5.3 h) |
| wf_1db629f6-355 | D7 a device retired while cut off; the dropped seq above the final head; invariant 4's injected frame above a Fence; the oracle follows every move (ENG-RECV-007) | `engine/briefs/d7-retired-oracle.md` | done (write, one check: no high or medium); applied to the branch as a patch from `wt/d7` 22872ab (the lead ran its gate) | `src/sim/{check,script,world}.rs`, `src/sim/world/{end,load,rt}.rs`, `api-simulator.md`: the oracle types each move code; retired while cut off (`retired_at` after a `ReplaceReady` naming the identity, `met_reference`, `log_reach`); `overwritten_below`; made-up frames above a Fence taken by no device. Reported: `gate` 0 of 450 fail, every configuration passes, all 13 regression files pass. One low (below) | 816,176 (2 agents, 309 tool uses, 61 min) |
| wf_394a4866-a90 | D8 what fails at 1,000 seeds: the checks' share (12 seeds) | `engine/briefs/d8-wide-gate.md` | done (write, two checks: one high fixed); applied to the branch as a patch from `wt/d8` fa40f7b (the lead ran its gate) | `src/sim/{check,script,world}.rs`, `src/sim/world/rt.rs`, `api-simulator.md`: deletions judged on the log a device followed at B (4986, 5685); the replay without a Skip's target applies a ResetTo with the comparison a board apply makes (5175). Reported engine faults 5197, 5271, 5463; spec questions A (4662, 4884, 5243: paired through a rolled-back state, closed, no alarm), E (5507, 5159, 5712: entries persisted before an evidence cut) and a ResetTo whose target lies in a Skip's window (§28.1 against §31.7, no seed) | 1,504,222 (4 agents, 517 tool uses, 105 min) |
| wf_39597d35-ce1 | D9 a duty entry refused with a re-sealable code is reported by its duty going on (Q9) | `engine/briefs/d9-duty-entries.md` | done (write, one check: one medium fixed with a test); applied to the branch as a patch from `wt/d9` 2cae4ce (the lead ran its gate) | `src/sim/{check,script,world}.rs`, `src/sim/world/{end,load,rt}.rs`, `api-simulator.md`: invariant 2 takes a `snap` entry or beat refused `seal/stale_epoch`/`seal/old_epoch` whose own device's duty went on (a later own entry of that kind, the duty ended after three failures, or another device's snapshot at or above S reached its log); the simulated user re-pairs a machine once per retirement (gate 17163374614648705494). Reported: 1,000 seeds at 17163374614648704531 and at 5000 fail 3: 17163374614648704878 (spec question E2: a move's Fence refused after the relay fenced itself), 5024 and 5942 (engine faults). Lows: coverage read from `log_reach` on any branch; a crash-cut sealing write counted as a sealing | 1,078,991 (3 agents, 391 tool uses, 74 min) |

## The owner's decisions, and the questions as they were put

### The owner's decisions (2026-11-25: "We will go with your recommendations and what you believe is optimal")

Decided as the lead proposed; each is to be written into the spec change and carried out.
**Done in the engine (2026-11-25, each with a test that failed before):** 1 (the beat under the
divergence stop, the stop ending at its own seq: `healthy_devices_that_heard_the_wrong_one_first_go_on_together`,
and the existing divergence test now asserts that only beats are sealed under the stop), the
sign-in rule of 2 (`a_relay_without_a_row_for_the_device_closes_it_on_a_board_never_moved`;
also not while following an address), the extra of 3
(`a_skip_whose_target_another_device_skipped_first_is_done`). Their tests cite ENG-DIVERGE-005
and ENG-SKIP-006, clauses the spec change adds: until it merges `spec-coverage` reports these two
as bad citations. The simulator's part of 2, 3 and 4 is task D6. After the sign-in rule no `gate`
seed at base 1000 has a device that retries its sign-in for good; the classes are as before.

1. **Divergence (§22.2):** under the divergence stop a device still sends its beat (the state
   report only, no verdicts), and one at once when it enters the stop; the stop also ends at the
   seq where it began once the device's group is strictly larger and has another member. Engine
   (lead) and spec.
2. **Rollback and fork (§31.7, plan §6.5 item 2):** for these two faults the promise is "never
   lost silently": invariants 2 and 3 except an entry whose only acknowledgements came from a
   relay state that a rollback or fork removed from the reference branch, provided a device
   holding it raised §21.1 evidence; invariants 1 and 6 also except a device that joined on a
   removed branch; a restore tail refused for a relay-made frame is covered. **The sign-in rule:**
   the §11.9 row for `401 relay/unauthorized` (stop `Closed`, `RePair`, ENG-SESS-007) also holds
   on a board never moved (no `Fence` in `M_K`, `meta.relay.home_gen = M_K.home_gen`). The plan's
   gate line gets the same note. Spec, simulator (agent), engine (lead).
3. **Final refusals (§31.7 invariant 2):** invariant 2 allows what invariant 3 allows (finally
   refused with a refusal record); the cases are §28.4 step 4 and the second of two Skips of one
   target. The lead's optional extra, taken as optimal: a Skip whose entry is refused because its
   target is already skipped ends with `Ok(())`. Spec, simulator (agent), engine (lead).
4. **The Skip window (§31.7 invariant 4):** a device that never applied the target `t` is compared,
   from `t` to the Skip, with the reference log replayed without `t`, computed by the simulator
   without the engine under test. Spec, simulator (agent).
5. **ENG-MOVE-009:** "exactly once, and once more after each `Start` that finds the report written
   and the move not ended" (already in the patch and the engine).
6. **`1099.json`** stays as it is; it is expected to pass once the simulator carries decision 2.

The questions as they were put, for the record:

#### (asked on 2026-11-25)

1. **Divergence (§22.2, ENG-DIVERGE-001/002)**: as written the stop can halt the healthy majority
   (details in the next paragraph). Proposal: a stopped device still sends its beat, and the stop
   also ends at the seq where it began once its group is strictly larger. Does not block the gate.
2. **What the gate promises when the relay itself misbehaves (rollback, fork).** §31.7 invariant 2
   says every acknowledged entry is in the final log, invariant 3 that every enqueued edit is
   accepted, held by a stopped device or refused. A relay that rolls back, or serves a forked copy,
   acknowledges entries that the reference log never holds; the devices that hold them raise
   evidence and stop, but the entries are gone (and a record a device already applied on the lost
   branch is no longer in its outbox). After every engine fix, 26 of 50 `gate` seeds at base 100
   and 57 of 100 at base 1000 fail only for this. Three cases ride on the same answer:
   - a device paired through the lost branch: the real relay never heard of it and answers
     `401 relay/unauthorized` at each hourly login, for good. §11.9's row for that answer names
     only a board that was moved (the relay of the latest `Fence`). Proposal: the row also holds
     for a board never moved, so the device stops with `Closed` and offers re-pair;
   - a restore tail that holds a frame the relay made up: `relay.md` §12.9 refuses it
     (`relay/wrong_board`), so the rolled-back entries above it cannot be restored;
   - SYNC_PLAN.md §6.5 words the gate as "tampering alarms without a state change", which the
     lead reads as: for these faults the promise is the alarm.
   Proposal: invariants 2 and 3 except an entry whose only acknowledgements came from a relay
   state that a rollback or fork removed from the reference branch, when a device holding it
   raised §21.1 evidence; invariants 1 and 6 already except stopped devices, and would also except
   a device that joined on a removed branch.
3. **Invariant 2 against a final refusal the spec itself defines.** Invariant 2 demands `Accept`
   for every acknowledged entry; invariant 3 already allows "finally refused with a refusal
   record". Two cases the spec makes final: a pairing's `DeviceAdded` that `register` acknowledged
   and a racing rotation made stale (§28.4 step 4: revoke, `PairFailed`, pair again; `gate` 1018),
   and the second of two Skips of one target composed by two devices at once (the sealer refuses
   it with `seal/ctl_target`; before the engine's fixes moved the seeds: `gate` 108, 1017, 1029).
   Proposal: invariant 2 allows what invariant 3 allows.
4. **Invariant 4 between a Skip's target and the Skip.** A device that crash-halted on the target
   `t` (or first reached `t` with the Skip already in its log) never applies `t`; the states it
   confirms from `t` up to the Skip are the log without `t`, which is what ENG-SKIP-002 and
   ENG-HALT-004 ask of it. The oracle of §31.7 replays with no faults, applies `t` and replays
   again at the Skip, so its states in that window have `t` applied, and invariant 4 ("no
   confirmed state differs from the oracle's at the same `B`") fails by design (`gate` 1098, 1014
   before the fixes). §17.3 already says reports from that window "describe the state before the
   Skip". Proposal: invariant 4 compares a device that never applied `t` with the log replayed
   without `t` in that window (an expected value computed without the engine under test).
5. **ENG-MOVE-009 "exactly one `MoveReport`".** See "Spec changes to make": after a crash between
   the durable write and the notification, the engine emits the code and the report again at the
   next `Start`, or the code is lost and nobody can follow the move. Proposal: "exactly one per
   `Start`". The engine does this now; say so if it should not.
6. **Regression file `1099.json`.** It records a run the engine fixed (a `SeedTail` nobody
   answered), passed when added, and fails since a later engine fix moved the seed into a fork
   loss (question 2). Every regression file made from a `gate` seed can turn this way until
   question 2 is decided. The lead leaves it failing. Say if it should be handled otherwise.
7. For information, no decision needed now: a board whose log holds a frame the relay made up can
   move only once a snapshot lies above that frame (the engine now makes one due at the next
   aligned seq), and a frame injected in the seconds between the move's check and its Fence still
   strands the board read-only on the old relay. Curing either fully needs `relay.md` §12.7/§12.9
   to accept such a frame in a tail, which weakens a relay MUST.


**For the owner (a MUST would change, so the lead does not decide it): the divergence stop can
halt the healthy majority.** §22.2 and ENG-DIVERGE-001 make a device stop when, among the reports
it knows at an aligned seq, its own group is not larger than another. A device that has seen one
differing report and no agreeing one yet is in a tie of one against one and stops; under the stop
it sends nothing (ENG-DIVERGE-002), so its own report never reaches the others, and the stop ends
only at a later aligned seq. Reproduced on 2026-11-24 with three devices of which one computes a
different state: the diverged device had beaten last, so it wrote the snapshot of seq 64; the two
healthy devices each read its report first and both stopped; nothing was sent any more except by
the diverged device. The engine does what the spec says. The lead's proposal, not applied: under
the divergence stop a device still sends its beat (the report is the evidence everyone needs) and
sends one at once when it enters the stop; and the stop also ends when, at the seq where it began,
the device's group has become strictly larger with another member. With two devices that
disagree nothing changes (both stay stopped). The test
`the_minority_stops_and_adopts_with_a_typed_fingerprint` arranges the order so that the rule as
written gives the intended result, and says so in a comment.

**For the owner, second question (an invariant would change, so the lead does not decide it):
invariant 2 of §31.7 against a relay rollback.** Invariant 2 says every entry a relay
acknowledged is in the reference log at the end. Seed 107 of `rollback`: the relay rolled back
two seqs that only one device had read; that device slept; another device, which had never seen
those seqs, appended at them. The first device detects it (RelayBehind, then the evidence stop),
as §21 asks, but its two acknowledged entries are in no relay's log, and no rule of the spec can
bring them back without losing the other device's. SYNC_PLAN.md §6.5 words the gate as "no lost
or twice-applied entry" beside "tampering alarms without a state change", which reads as: a
rollback is tampering, and the promise for it is the alarm, not the entry. The lead's proposal,
not applied: invariant 2 excepts an entry whose only acknowledgement came from a relay state
that a rollback or fork fault removed from the reference branch, provided a device that holds
its frame raised evidence of §21.1 (so the loss is never silent). Until the owner decides, the
simulator reports such seeds as failing, with the text "lost to a rolled-back relay state", and
the gate of 1,000 seeds cannot pass with the rollback and fork faults on.

Low, for the round of checks to weigh: the engine's trial apply is `apply` on a copy, not
`SM::trial_apply` (§9.1 says "apply the command to a copy"; ENG-GEN-012 lists `trial_apply`).
A session's `Disconnected` also drops requests in flight to another relay (the door, seed and
tail of a move), which are sent again; harmless.

## Spec questions met

- From task D7 (each read strictly, none changes a seed today): (a) a re-pair after the device's
  own `Revoked` stop with no `ReplaceReady` is not "retired" (§31.7 invariant 2's last bullet says
  "after `ReplaceReady`"); (b) a rejoin to a snapshot above a refused entry's seq counts as the
  log having reached that seq; (c) a seq above the final head that a restore tail could have
  brought back is not removed even above an overwritten lower seq. Low (D7's check): invariant 4's
  exception for a made-up frame above a `Fence` also takes a `Fence` the showing relay never
  sequenced (a board moved twice: a frame the final relay makes up is excused by the first
  `Fence`); unreachable in the gate (every move goes to the same second host).

- E2 (left open by `seal`): an own `Fence` that loses a rotation race leaves the old relay fenced
  with no accepted `Fence` (§28.3, §28.7). Met by the gate on 2026-11-25; left open by the owner
  on 2026-11-28 ("Open, and lows" above).
- E4 (left open by `seal`): §24.3 re-seals a pending revocation's rotation, and §28.5 composes it
  again; the second record is refused `seal/ctl_target`. To decide in step 8.
- Left open by `foundation` (T21): whether `MoveReport` carries the seed's first seq; whether the
  shared picture is uploaded again after a move.
- Left open by `seal` (Q79): nothing binds the log between the bundle's head and `added_seq`; the
  remedy (the bundle carries the chain value at `added_seq`) is `engine`'s choice.
- **Decided by the owner (2026-11-25, after D8): Q9 "Duty going on counts" (an own `snap` entry
  or beat refused with a re-sealable code is reported by its duty going on; 5049, 5346, 5434), Q10
  "Alarm, then close" (a device closed because the relay holds no row for it first raises
  `relay_behind` when it holds an own entry a relay acknowledged; 4662, 4884, 5243).** Spec text:
  local branches `spec-change-4` 0e12d17 (behaviour: Q10; an evidence cut persists nothing of its
  page, ENG-CLASS-003; §21.3 step 5 echo; §28.3 stage 3 chain held; §28.4 step 3 acknowledgement
  durable; §28.8 `OpDone` in the ending call) and `spec-gate-3` b958be6 (Q9), each its own pull
  request; then the engine changes (the evidence cut, kept as a patch, and Q10) and task D9 (Q9's
  check).
- **Decided by the owner (2026-11-25): Q7 "Accept as known limit", Q8 "Count it as removed".**
  Spec text: local branches `spec-change-3` 183e016 (behaviour: ENG-RECV-007 a fenced relay is
  read up to its Fence only; §21.3 step 1 repairs again; §28.4 step 4 `PairFailed` with the
  revoke) and `spec-gate-2` 6099cd3 (gate: §31.7 retired device, the dropped seq above the final
  head, invariant 4's injected frame above a Fence; SYNC.md §11 risk 7; SYNC_PLAN.md §6.5 item 2)
  in `/home/user/wt/spec`, each to merge as its own pull request (parking cycle), then the engine
  change for ENG-RECV-007 and task D7 (the simulator's checks for both).
- Asked of the owner (2026-11-25), after D6 and the fixes of 1022, 1073, 2154: (Q7) a device
  the user retires (revoked with replace, re-paired) while it is cut off: on a fork's copy its
  entries only the copy acknowledged are lost with no alarm (`gate` 128, 1019, 2041, 2111, 2124,
  2155, 2240, 2245, 2277, 2295); shown a stale view, it never sees and never reports its own
  entry's refusal (`gate` 104). Options: accept as a known limit (recommended), hold the export
  until the device caught up, or export unconfirmed edits. (Q8) a rollback-dropped seq above the
  reference log's final head whose lower dropped seqs other entries took (`gate` 2272): count it
  as removed (recommended). Every named configuration passes 30 of 30 at base 3000 on a24bcb6.
- To change by the lead in the next spec pull request (no owner question): a page from a relay
  that an accepted `Fence` moved the board away from is not taken above that `Fence`, and the
  relay's head counts as the `Fence`'s seq (§12.3, §28.3; found with `gate` 2154's test).
