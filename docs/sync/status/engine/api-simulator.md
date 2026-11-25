# The simulator: how to run it (tasks D, D2, D3 and D4)

Module `taskdeck_sync::sim` (feature `sim`; the test targets get it from the crate's dev-dependency on itself).
Spec: engine.md §31. Code: `src/sim.rs` (types), `src/sim/config.rs` (named configurations), `src/sim/world.rs` and
`src/sim/world/` (model runtime, network, workload, faults), `src/sim/store.rs` (durable storage), `src/sim/check.rs`
(ledger, oracle, the six invariants), `src/sim/trace.rs` (§31.9), `src/sim/script.rs` (hand-built logs for the
simulator's own tests).

## Run seeds

```sh
export CARGO_TARGET_DIR=...           # as usual
SIM_SEEDS=1000 cargo test -p taskdeck-sync --test simulator seeds_hold   # the gate (engine.gate)
TD_SIM_CONFIG=quiet TD_SIM_BASE=1 SIM_SEEDS=200 cargo test -p taskdeck-sync --test simulator seeds_hold -- --nocapture
```

`base` is `TD_SIM_BASE`, else the first 16 hex digits of the commit id (read from `.git`), ENG-SIM-004. The test
prints `base`, each failing seed with its first five violations, and the JSON of its regression file. Use `--release`
for many seeds (timings in task D's report).

## One seed and its trace

```sh
TD_SIM_SEED=4242 TD_SIM_CONFIG=gate cargo test -p taskdeck-sync --test simulator one_seed -- --nocapture --exact
```

prints the trace up to the first violation, then `trace_sha256`, the `SimStats` and every violation. `TD_SIM_FULL=1`
prints the whole trace. Without `TD_SIM_SEED`, `one_seed` runs seed 1 of `quiet` and requires every invariant.
From code: `sim::run(seed, &SimConfig::named("gate").unwrap())`, or `sim::run_traced` for the trace's bytes.

## Named configurations (`SimConfig::named`, `sim::NAMES`)

| Name | What |
|---|---|
| `quiet` | 3 machines, 1 day, no fault, no operation: the workload, sleep and wake, the phone pattern, pairing |
| `gate` | 4 machines, every fault at a moderate rate, every operation, `rules2` on machines 1 and 3 |
| `loss`, `dup`, `delay`, `disconnect`, `crash`, `skew`, `rollback`, `fork`, `withhold`, `inject` | one fault at the top of its range |
| `revoke`, `purge`, `restore`, `move`, `recovery` | one operation |
| `skip`, `poison` | one poison entry, the Skip offer answered |
| `rules` | the rules build update, with machine 0 (the creator) on a rules-2 build; a machine that pairs onto a board at rules 2 gets the rules-2 build first (ruling 9) |

Every run: machine 0 creates the board, the others pair through an engine device one minute apart; the last
machine of three or more is the phone. `T_f = days`; then 7,200,000 ms of settle (faults off, no workload, every
machine awake).

## Add a regression file (§31.8, ENG-SIM-005)

Copy the JSON the gate printed into `crates/taskdeck-sync/tests/regress/<seed>.json`, with the fix that makes it
pass: `invariant`, `trace_sha256` and `first_bad_line` are of the failing run, never of the fixed one. When the fix is
already in, rebuild the failing run (this simulator over the engine before the fix) and copy what it prints; 102, 111
and 116 record their runs with the engine before 2139a96. Seeds 1 (`quiet`) and 200 (`move`) pass over that engine too,
with this simulator and with task D's (3f657f7), so no failing run is known for them: their three fields are not of
one. With a shared `CARGO_TARGET_DIR`, a second copy of the crate
reuses the first copy's binary unless one of the first copy's sources is newer: check the `Compiling` line. Add no file for a seed that still fails. `cargo test -p taskdeck-sync --test regressions` runs every file and requires every invariant; a file that does
not parse (unknown field, wrong `spec`, invariant outside 1..6) fails it. `config` is `SimConfig::to_json`.

## Read a trace (§31.9)

One JSON object per line, `t` (virtual ms) and `k` first:

- `start` `dev attempt` (attempt = process number of that identity); `crash` `dev at` (`before|during|after`);
- `in` `dev ev [seq] [n]`: `seq` is the head of a `page`/`welcome`, the checkpoint seq of a `start`; `n` the entries,
  results, records, or the id of a `persisted`;
- `out` `dev fx [id] [n] [frames]`: `fx` is the effect, or the notification's name for a `Notify`; `id` the
  persist/request id or a tick's `at_mono_ms`; `frames` the SHA-256 of each frame it carries;
- `relay` `r op head chain`: every change of a relay's head (`r` 0 or 1; a fork's copy is relay 0 with `op`
  `fork_…`; the workload's own appends are `op` `poison` and `recovery_append`);
- `fault` `kind dev|r p`: `loss`, `duplication`, `disconnection`, `clock_skew` (p: the offset), `relay_rollback`
  (from, to), `relay_fork` (seq, devices), `relay_fork_end`, `relay_withholding` (seq), `relay_withholding_end`,
  `injected_frame` (seq, 0 bad signature / 1 another board); faults only (operations count in `SimStats`);
- `alarm` `dev kind [seq]`; `check` `inv ok detail` (one per invariant at the end).

`dev` is an identity index: machines start as 0..n−1; a re-paired machine gets the next index.

## Runtime choices (§31.4) worth knowing when reading a trace

- Persist latency 1–20 ms, writes confirmed strictly in emission order; durable state is exactly the delivered
  writes, plus the write in progress at a `during` crash half the time. Restart delay 1–31 s.
- Messages: 5 ms plus the drawn delay; socket messages keep their order; HTTPS answers do not. A request with no
  answer after 20,000 ms (plus 1 s per 64 KiB) ends the session as unreachable (`device.md` §10.10).
- Every `Connect` logs in, opens a socket and says hello; backoff as DEV-BACKOFF-001. Nothing the relay sends on that
  socket reaches the engine before the login's answer has (D3: a lost answer left pushes flowing to a connecting engine).
- Ticks are delivered at the time asked (now, when it has passed); `SimStats.ticks_due_now` counts `ScheduleTick`s
  that asked for a tick within 1 ms.
- `relay/token_stale` never reaches the engine: the runtime logs in again and sends the request with the new token
  (§11.9, `SimStats.token_renewals`); a refused login reaches it as `RelayRefused { req: None }`, then `Disconnected`.
- An answer to a request still outstanding is delivered even when the request was sent while connecting; a
  disconnect discards every outstanding request.
- The simulated user answers prompts again after a restart and after a refused move code.
- Sleep is `Shutdown` then exit; wake is a new process (`Start`).
- Withholding shows one device a stale view (head at the seq, nothing above it); a rollback restores the tables and
  ends every session; a fork serves a subset of devices from a copy until it ends.

## The invariants as checked (§31.7, strict readings)

- 2: every acknowledged `entry_id` once with class `Accept` in the reference log, whatever became of its device; one
  that only a rolled-back or forked relay state held reads `lost to a rolled-back relay state (…, fault at trace line N)`;
  a refused entry that a relay's `register` acknowledged with a seq (a pairing's `DeviceAdded`; a refused frame
  carries no items, so the check tells it by the `register`, `World::registered`) reads
  `a pairing's DeviceAdded refused after register (§28.4 step 4)`.
  And every device whose final `B` is past that seq has applied it exactly once: the devices run `CountedToy`
  (`src/sim/counted.rs`), the toy with a trail of the seqs `apply` ran for on each state value's lineage since its
  last load from bytes (invisible to the engine: not in the bytes, the hash or any decision). It is read on the log
  the device followed (the reference log, or the branch with its chain at `B`), for `Cmd` entries from the first seq
  the trail holds; an entry an accepted Skip at or below `B` targets is expected 0 times (ENG-SKIP-002). A violation
  reads `B = … is past seq … of acknowledged entry …, which it applied n times (expected m)`.
- 4: the confirmed state at every sampled `B` past the device's join, against the oracle's on the branch it followed,
  a `B` between a Skip's target `t` and the Skip `s` included (ruling 6; such a violation names the Skip). The oracle
  is the one of §31.7 only (`check::oracle`): a fresh engine replaying the log from seq 1 with no faults, its first
  state at each `B` (a Skip replay does not overwrite it). A `B` in `t..s` it never confirmed is reported too.
  Task D3 had added a second history, the oracle replayed with its process killed while `t` was under a marker
  until it crash-halted on `t`, open to a device that crash-halted on `t`; the D3 fix pass removed it: those states
  come from the engine under test run under faults, not from an independent expected value, and accepting them
  loosened invariant 4 (test `invariant_4_at_a_skip_window_is_judged_by_the_no_fault_oracle_only`, `script.rs`). A violation of a
  device whose storage held a crash halt on `t` (`IdentFacts::crash_halts`) ends with
  `; it crash-halted on the target t: ENG-HALT-004 against §31.7 invariant 4, a spec question (api-simulator.md)`.
- 3: accepted once; or still in the outbox of a device halted or stopped with a persisted reason, or revoked (in
  its status or in the oracle's membership); or refused with a refusal record; or dropped as a cancelled hold. A
  record its device applied on a branch a rollback or fork removed reads `applied on a rolled-back relay state (…)`.
- 5: a `Drop` reached the board head, or is the held `Create` of the `Forget` being enqueued, or a `Register`
  record with a `PairFailed` for its device in the same or the next call; a `ConfirmRejoin` explains its own
  `TruncateLogBelow(S + 1, Some(C_S))` and one `DeleteBases`, then nothing; an identity write that drops a pending
  root of an own rotation needs that rotation's own entry, in the log the device followed there (the chain it holds
  at that seq), at or below the highest seq the same write makes durable (accepted: it reached the head; refused: a refused own rotation). Another device's rotation
  excuses no pending root.
- 6: also no `Append` other than of beats, `snap` entries and the rules-upgrade proposal in the last 1,800,000 ms.
  3 and 6 name a device whose `DeviceAdded` only a rolled-back or forked state holds: `its pairing is only on …`.
- Roots: every `(epoch, root)` any device held; two branches may rotate to one epoch with two roots, and each log is
  classified with the choice per epoch that opens it furthest (`check::pick_roots`; D3, gate seeds 1019 and 1058).

## The simulator's own tests (task D4)

A test of the simulator holds whatever the engine does at a seed: no test names a seq, a device number or a trace
line that a run happened to produce (every engine fix moves seeds). They are built in this order of preference:

- **A quiet `World`** (no fault, no operation): `world.rs` runs quiet seed 3 to its end, changes one fact (an archive
  row, a sample, an acknowledgement, a deletion, `tamper_facts`), and asserts which invariants the change breaks,
  counting only the violations the untampered run does not have. `a_quiet_run_holds_every_invariant` is the one unit
  test that asks the engine something (that run holds every invariant).
- **A hand-built log** (`src/sim/script.rs`): `HandLog` creates a board with the creator's fake ("Desk"), seals
  Desk's commands, `DeviceAdded` and rotations, seals Skips as the board's recovery identity (`seal.md` §13.6: a
  device may not skip its own frame), and has the model relay sequence them; two `HandLog`s of one seed built with the
  same first steps are a fork (both Desks draw entry ids from the seed, so a test that needs the fork's entries to
  carry ids of their own has the fork's Desk draw one id it never seals). `Case` holds the facts `check::check` is handed (reference log, branches, identities,
  acknowledgements, deletions), `settled` and `honest` build identities whose samples are the oracle's. No engine runs
  except the §31.7 oracle; where a test leans on what the oracle confirms, it asserts that as a premise with its own
  message (for example: the oracle halts on a second poison entry inside a Skip's window, ENG-POISON-003).
- A bounded seed search (at most 40 runs, the first seed that shows a pattern, a failure naming the pattern) only for
  a situation nothing else can build; none is needed today.

Each test of `script.rs` fails when its rule is removed from `check.rs` (task D4 tried each by hand; the fix pass added
`a_pending_root_drop_on_a_fork_is_judged_by_the_forks_log` for invariant 5's `Deletion::Pending` with a chain, which
the rewrite of the epoch-rotation test had left without one). The files under
`tests/regress/` are exempt: they record failing runs. `tests/simulator.rs` is the gate itself (seeds of a range,
determinism, one seed's trace).

## Spec question (task D3): a device that crash-halted on a Skip's target, against invariant 4

Gate seed 1014 (1098 likewise: device 0, target 200, Skip at 201): device 2 crash-halts on the healthy entry 192 (§15.2, two hard aborts while 192 is under
its marker), every other device applies it, and device 2's Skip of 192 is accepted at 197. After the Skip clears the
halt device 2 applies 192 as skipped and the entries after it (§17.3 step 1, ENG-HALT-004), so its confirmed state
at 192, 195 and 196 lacks 192 (ENG-SKIP-002). The §31.7 oracle applied 192 when it reached it and has it at each of
those `B`. The check reports three invariant-4 violations; they stay failing until the owner decides.

- Reading A (invariant 4 as written): "no device's confirmed state at any moment differs from the oracle's at the
  same `B`" with the oracle a fault-free replay. Device 2's states differ: either the engine is wrong (ENG-HALT-004
  should not let a device confirm a `B` in `t..s` without `t` when the oracle has `t` there, for example by
  confirming nothing below `s` after the Skip) or the invariant is violated by design.
- Reading B (ENG-HALT-004 and ENG-SKIP-002 as written): a device that crash-halted on `t` legitimately confirms the
  `B`s of `t..s` without `t`; invariant 4 then needs an expected value for those `B`s that is derived independently
  (for instance the log replayed with `t` excluded), stated in §31.7. The check does not choose one.
