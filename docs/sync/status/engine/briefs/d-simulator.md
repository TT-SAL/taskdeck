# Task D: the simulator

Working copy: `/home/user/wt/d` (branch `wt/d`). `CARGO_TARGET_DIR=/home/user/wt/target-d`.
Read `docs/sync/status/engine/briefs/README.md` first: its rules hold here. **Do not read
`src/engine/**`**: the simulator is the gate of stage `engine` (SYNC_PLAN.md §6.5), and what judges
the engine is written from the spec, not from the engine. Use the engine only through its public
API (`engine.md` §37) and the contracts of §4 to §30.

## What to read

`engine.md` §31 in full (it is short), §4 (the model: inputs, effects, durability, the order of
one call), §7.10 to §7.12 (`Loaded`, the persist operations, what must be durable first), §30
(status, notifications, user actions), §28 (operations) for the workload, and the sections an
invariant needs when you write it. `docs/sync/status/engine/api-fake-toy.md` (the fake sealer and
the toy: what to call) and `docs/sync/status/engine/api-model-relay.md` (the model relay, task C).
The public types: `src/events.rs`, `records.rs`, `session.rs`, `receive.rs`, `snapshots.rs`,
`ops.rs`, `ui.rs`, `model.rs`.

## What to build (`engine.md` §31.1, §31.4 to §31.9)

`taskdeck_sync::sim::run(seed, &SimConfig) -> SimReport`: `n` devices (2 to 4), each an
`Engine<ToyMachine, FakeSealer>` with a model runtime, against one or two model relays, on one
virtual clock, from one `u64` seed. Two runs with the same seed and configuration produce the
same trace byte for byte (ENG-SIM-003).

1. **The model runtime** (§31.4), per device: a persist queue with a latency drawn per write;
   durable storage holding exactly the writes whose `Persisted` was delivered, plus, after a
   crash, the write in progress if the fault generator so chooses (ENG-PERSIST-004); `Start` with
   `Loaded` read from durable storage after a restart delay (§7.10; `Loaded.chains` is
   `(seq, chain, tau)` in this crate, a spec change is on its way); ticks at `ScheduleTick`
   times; `Connect` after a full-jitter backoff from 1 to 60 s; a session id per process; every
   effect answered by the event §4.2 and §4.3 name. Two things the lead learned the hard way:
   writes are confirmed **strictly in the order they were emitted**, also the ones emitted while
   a `Persisted` is being handled (the engine ignores an out-of-order `Persisted`); and after a
   restart the fake is rebuilt with `FakeSealer::restore(identity, history, reseed, rules)` from
   the identity bytes of the last durable `WriteIdentity` (or the ones the runtime wrote before
   the first `Start`) and the control history the runtime keeps (`FakeSealer::history()`), with
   a new seed drawn from the device's generator.
2. **Boards and pairing.** The first device creates the board (`FakeSealer::create`, the model
   relay's create, `Start` with `Bootstrap::Genesis`). Every other device joins through the
   engine: `Event::PairRegister { ticket, device: NewDevice { dev, sign_pk, hpke_pk, name,
   transcript } }` on a device that is `Live`; the engine uploads a snapshot (purpose 2) and
   sends `Admin { Register }`; on `Notify(PairRegistered { head, snapshot, added_seq, .. })` the
   simulator plays the pairing machines: it opens the uploaded blob, builds the `FakeBundle`
   (`api-fake-toy.md`), and starts the new device with `Bootstrap::Join { material, added_seq,
   relay }`. A `PairFailed` means pair again later.
3. **The workload** (§31.5): toy commands (`Set`, `Add`, `Del`, `Create` with a hold followed by
   `Name` or `Forget`, `Fail`; `Panic` only in poison configurations) with `at_ms` the device's
   clock and the toy's origin state; sleep and wake schedules, one device with the phone pattern
   of §29.4; operations from `OpSet`: revoke and re-pair, Skip when a device is halted, a rules
   upgrade with `rules2`, purge, move to the second model relay, restore to a point, a
   recovery-key change sealed as the recovery identity. User actions the engine waits for
   (`ConfirmClock`, `MoveCode`, `ConfirmRejoin`, `UseThisCopy`, re-pair and resubmit after a
   revocation) are part of the workload: a simulated user answers them after a drawn delay.
4. **The faults** (§31.6, the table's ten rows with their parameter ranges), stopping at `T_f`;
   then 7,200,000 ms of virtual time with duties on and no workload.
5. **The trace** (§31.9, exactly that format) and `trace_sha256`.
6. **The six invariants with the oracle** (§31.7), checked at the end of every run; each
   violation reported with the seed, the invariant and the first trace line that shows it
   (ENG-SIM-002). Nobody loosens an invariant: where the text of §31.7 can be read two ways, take
   the stricter reading and say so in your report. The oracle is a fresh
   `Engine<ToyMachine, FakeSealer>` that replays the reference log from seq 1 with no faults; it
   needs a sealer that can open every epoch: if the fake lacks a way to build one, you may add
   crate-private helpers to `src/fake.rs` that change no existing behaviour, each listed in your
   report.
7. **`SimConfig`, `FaultSet`, `OpSet`, `SimStats`.** `src/sim.rs` has them with empty bodies
   where the spec leaves the fields to this stage. Give `FaultSet` one field per fault of §31.6
   (its parameter in the unit the table states), `OpSet` one switch or rate per operation of
   §31.5, `SimStats` counts per event and effect kind and whatever you need to show that a run
   exercised what it claims (entries appended, restarts, alarms by kind, operations done).
   You change them in `src/sim.rs`; list every field with its meaning and range in your report,
   because the lead turns that list into the spec change. Provide named configurations as
   functions: a quiet one (no faults, no operations), one for the gate (every fault at a
   moderate rate, every operation), and the single-fault and poison configurations you find
   useful.
8. **The test targets** the gate names (`scripts/gates/engine.gate`):
   - `tests/simulator.rs`: runs seeds `base..base + N` with the gate configuration and requires
     every invariant to hold; `N` is `SIM_SEEDS` (default 50 when unset), `base` is
     `TD_SIM_BASE` when set, else the first 16 hex digits of the commit id read as a `u64`
     (ENG-SIM-004); it prints `base` and every failing seed, and for each failing seed the JSON
     of its regression file (§31.8) so that it can be committed as it is. Reading the two
     variables and the commit id (from `.git`, by hand: no process is spawned) is the one place
     where a test of this crate touches the environment and files: put it in one function
     with `#[allow(clippy::disallowed_methods)]` and a comment that says why. The same file has
     a determinism test (one seed run twice gives the same `trace_sha256`, ENG-SIM-003) and a
     way to run one seed and print its trace up to the first violation:
     `TD_SIM_SEED=<seed> cargo test -p taskdeck-sync --test simulator one_seed -- --nocapture`
     (with `TD_SIM_CONFIG=<name>` to choose a named configuration).
   - `tests/regressions.rs`: runs every file under `tests/regress/` (the format of §31.8) and
     requires every invariant to hold (ENG-SIM-005). With no file yet it must still pass, and it
     must fail when a file does not parse.
   Cite the clauses in comments: ENG-SIM-002 to ENG-SIM-005, ENG-PERSIST-001, ENG-PERSIST-004,
   ENG-EFFECT-001 (the runtime's duties the simulator models), and each invariant's clauses
   where §31.7 leans on one.

## The engine is not finished

The lead is still writing parts of the engine. When a seed fails, find out whether the
simulator, the model relay, the fake or the engine is wrong, from the trace and the spec. Fix
what is yours. For the rest, do not work around it: report the seed, the configuration, the
first bad trace line and the clause you think is broken. A simulator that passes because it
avoids what the engine cannot do yet is worth nothing; a named configuration that leaves an
operation out, with the reason stated, is fine. Start with the quiet configuration, then add
faults one by one, then operations.

Measure and report: how long 1,000 seeds of the gate configuration take in this container
(release and debug), and the same for the quiet one; the gate must stay practical in CI on
three systems (aim for a few minutes), so choose `days` and the rates accordingly and say what
you chose.

## Files you may change

`crates/taskdeck-sync/src/sim.rs` (not the `splitmix` and `relay` lines), new files under
`crates/taskdeck-sync/src/sim/` (not `splitmix.rs`, not `relay.rs` or `relay/`),
`crates/taskdeck-sync/tests/simulator.rs`, `tests/regressions.rs`, `tests/regress/**`,
`docs/sync/status/engine/api-simulator.md` (at most 100 lines: how to run seeds, one seed with
its trace, the named configurations, how to add a regression file, how to read a trace), and
crate-private helpers in `src/fake.rs` as item 6 allows. If the model relay is wrong, report it
(seed, trace line, the `relay.md` clause); do not edit it.

## Done means

In `/home/user/wt/d`, with `CARGO_TARGET_DIR=/home/user/wt/target-d`:

- `cargo test --locked -p taskdeck-sync --message-format=short` passes;
- `SIM_SEEDS=1000 cargo test --locked -p taskdeck-sync --test simulator` passes with the quiet
  configuration at least; for the gate configuration, either it passes or every failing seed is
  in your report with its cause as far as you can tell;
- `cargo test --locked -p taskdeck-sync --test regressions` passes;
- clippy with `--all-targets -- -D warnings` passes; the wasm32 check with `--features sim` passes;
- no `unimplemented!()` or `todo!()` is left under `src/sim`;
- everything is committed on `wt/d`, `git status` is clean.

Report: the fields of `FaultSet`, `OpSet` and `SimStats`; the timings; what each invariant checks
and where its text allowed two readings; every failing seed with your diagnosis; what you could
not model and why; spec questions (clause ids).
