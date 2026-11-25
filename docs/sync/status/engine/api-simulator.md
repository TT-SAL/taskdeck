# The simulator: how to run it (task D)

Module `taskdeck_sync::sim` (feature `sim`; the test targets get it from the crate's dev-dependency on itself).
Spec: engine.md §31. Code: `src/sim.rs` (types), `src/sim/config.rs` (named configurations), `src/sim/world.rs` and
`src/sim/world/` (model runtime, network, workload, faults), `src/sim/store.rs` (durable storage), `src/sim/check.rs`
(ledger, oracle, the six invariants), `src/sim/trace.rs` (§31.9).

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
| `rules` | the rules build update, with machine 0 (the creator) on a rules-2 build |

Every run: machine 0 creates the board, the others pair through an engine device one minute apart; the last
machine of three or more is the phone. `T_f = days`; then 7,200,000 ms of settle (faults off, no workload, every
machine awake).

## Add a regression file (§31.8, ENG-SIM-005)

Copy the JSON the gate printed into `crates/taskdeck-sync/tests/regress/<seed>.json`, with the fix that makes it
pass. `cargo test -p taskdeck-sync --test regressions` runs every file and requires every invariant; a file that does
not parse (unknown field, wrong `spec`, invariant outside 1..6) fails it. `config` is `SimConfig::to_json`.

## Read a trace (§31.9)

One JSON object per line, `t` (virtual ms) and `k` first:

- `start` `dev attempt` (attempt = process number of that identity); `crash` `dev at` (`before|during|after`);
- `in` `dev ev [seq] [n]`: `seq` is the head of a `page`/`welcome`, the checkpoint seq of a `start`; `n` the entries,
  results, records, or the id of a `persisted`;
- `out` `dev fx [id] [n] [frames]`: `fx` is the effect, or the notification's name for a `Notify`; `id` the
  persist/request id or a tick's `at_mono_ms`; `frames` the SHA-256 of each frame it carries;
- `relay` `r op head chain`: every change of a relay's head (`r` 2 is a fork's copy);
- `fault` `kind dev|r p`: `loss`, `duplication`, `disconnection`, `clock_skew` (p: the offset), `relay_rollback`
  (from, to), `relay_fork` (seq, devices), `relay_fork_end`, `relay_withholding` (seq), `injected_frame` (seq,
  0 bad signature / 1 another board), `poison`, `recovery_key`, `rules_build`, `settle`;
- `alarm` `dev kind [seq]`; `check` `inv ok detail` (one per invariant at the end).

`dev` is an identity index: machines start as 0..n−1; a re-paired machine gets the next index.

## Runtime choices (§31.4) worth knowing when reading a trace

- Persist latency 1–20 ms, writes confirmed strictly in emission order; durable state is exactly the delivered
  writes, plus the write in progress at a `during` crash half the time. Restart delay 1–31 s.
- Messages: 5 ms plus the drawn delay; socket messages keep their order; HTTPS answers do not. A request with no
  answer after 20,000 ms (plus 1 s per 64 KiB) ends the session as unreachable (`device.md` §10.10).
- Every `Connect` logs in, opens a socket and says hello; backoff as DEV-BACKOFF-001.
- Ticks are delivered no sooner than 1,000 ms after the previous one (timer granularity); `SimStats.ticks_due_now`
  counts `ScheduleTick`s that asked for a tick within 1 ms.
- Sleep is `Shutdown` then exit; wake is a new process (`Start`).
- Withholding shows one device a stale view (head at the seq, nothing above it); a rollback restores the tables and
  ends every session; a fork serves a subset of devices from a copy until it ends.
