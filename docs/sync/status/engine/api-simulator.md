# The simulator: how to run it (tasks D to D8)

Module `taskdeck_sync::sim` (feature `sim`; the test targets get it from the crate's dev-dependency on itself).
Spec: engine.md §31, §31.7 as the owner amended it on 2026-11-25 (tasks D6, D7). Code: `src/sim.rs` (types),
`src/sim/config.rs` (named configurations), `src/sim/world.rs` and `src/sim/world/` (model runtime, network,
workload, faults), `src/sim/store.rs` (durable storage), `src/sim/check.rs` (ledger, oracle, the six invariants),
`src/sim/trace.rs` (§31.9), `src/sim/script.rs` (hand-built logs for the simulator's own tests).

## Run seeds

```sh
export CARGO_TARGET_DIR=...           # as usual
SIM_SEEDS=1000 cargo test -p taskdeck-sync --test simulator seeds_hold   # the gate (engine.gate)
TD_SIM_CONFIG=quiet TD_SIM_BASE=1 SIM_SEEDS=200 cargo test -p taskdeck-sync --test simulator seeds_hold -- --nocapture
```

`base` is `TD_SIM_BASE`, else the first 16 hex digits of the commit id (read from `.git`), ENG-SIM-004. The test
prints `base`, each failing seed with its first five violations (`TD_SIM_ALL=1`: every violation of every failing
seed), and the JSON of its regression file. Use `--release` for many seeds (450 `gate` seeds take about 35 s).

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
| `rules` | the rules build update, machine 0 on a rules-2 build; a machine that pairs onto a board at rules 2 gets the rules-2 build first (ruling 9) |

Every run: machine 0 creates the board, the others pair through an engine device one minute apart; the last
machine of three or more is the phone. `T_f = days`; then 7,200,000 ms of settle (faults off, no workload, every
machine awake).

## Add a regression file (§31.8, ENG-SIM-005)

Copy the JSON the gate printed into `crates/taskdeck-sync/tests/regress/<seed>.json`, with the fix that makes it
pass: `invariant`, `trace_sha256` and `first_bad_line` are of the failing run, never of the fixed one (102, 111 and
116 record runs with the engine before 2139a96; 1 and 200 have no known failing run). With a shared
`CARGO_TARGET_DIR`, a second copy of the crate reuses the first copy's binary unless one of its sources is newer:
check the `Compiling` line. Add no file for a seed that still fails. `--test regressions` runs every file and
requires every invariant; a file that does not parse fails it. `config` is `SimConfig::to_json`.

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
- Every `Connect` logs in, opens a socket and says hello (DEV-BACKOFF-001); nothing the relay sends on that socket
  reaches the engine before the login's answer has. Ticks come at the time asked (now, when it has passed).
- `relay/token_stale` never reaches the engine: the runtime logs in again (§11.9); a refused login reaches it as
  `RelayRefused { req: None }`, then `Disconnected`. A disconnect discards every outstanding request.
- The simulated user answers prompts again after a restart; on `ReplaceReady` (§28.5 step 3) it exports the revoked
  machine's unsent edits, retires that identity and pairs the machine again. Sleep is `Shutdown` then exit.
- Withholding shows one device a stale view; a rollback restores the tables and ends every session; a fork serves
  a subset of devices from a copy until it ends.

## The invariants as checked (§31.7 as amended, strict readings)

What a run records for them (task D6): every acknowledgement a relay gave, delivered or not (`World::acks`: the
relay state, home relay or the fork's copy, its seq, frame and trace line); every `restore_tail` a device sent
(`World::restore_tails`, recorded when sent): its frames, the relay's first answer (stored, or refused with the code
and the index of the frame a refusal names; none when the request was lost on the way; a duplicated copy is no
other request), the relay's head when the request met it (when it was sent, if it never did) and the sender's
durable log then (`Store::run_head`: its head and the first seq of the unbroken run of rows that ends there);
every restore tail a device owed and did not send (`World::owed`, `Answer::Owed`): after a call, a process in a
welcomed session with relay 0 (the home a rollback rolls back, never moved) whose head is below the device's durable
log head on its chain, not halted (§16.4), with no restore tail of its on its way, owes one from that head (§21.3:
"as any device does that holds more than the relay", ENG-EVID-004) until it sends one, its session or process ends,
or the run ends; owed when that took `RESTORE_DUE` (20,000 ms, the response timeout) or longer, recorded with the
relay's head and the device's durable log when it fell due (a correct engine sends a `ReadLog` and then the request
within milliseconds: no seed of `gate`, `rollback` or the named configurations changed its verdict); the
frames the relay made up, each with how (`World::made_up`: a bad signature, another board's frame); each alarm of
kind `RelayBehind`, `Fork`, `Counter`, `Unechoed` or `Withheld` once its `WriteMeta` is durable, with every frame its
device's durable log and live outbox held then (`IdentFacts::evidence`: the kinds whose response in §21.1 is the
evidence stop, SYNC.md §4.8's evidence); every `PairFailed` and `OpDone`; the operation each outbox record belongs
to (`OutboxRecord.op`); each move of a device's board within a process with its class head `K`, the durable log head
(`IdentFacts::crossings`). Task D7: the relay state that produced each message a device's engine got (a relay, or the
fork's copy) and the trace line then, so `IdentFacts::met_reference` is the last line at which a relay produced a
welcome, page or answer that reached it (the runtime's own `Disconnected` is none); a retirement after a
`ReplaceReady` that named the identity (`retired_at`, the user's export and re-pair of §24.5; a re-pair for another
reason has none); how far its log reached (`log_reach`: its `AppendLog`s and checkpoints); each injected frame with
its hash and the host of the relay that showed it (`check::Injected`); each made-up frame an `AppendLog` of an
identity carried (`IdentFacts::took`).

**The oracle** (task D7) types the move code of each accepted `Fence` that awaits one (`Status::moving`,
`Sealer::move_code(f, H(frame))`, §28.3 "On every other device" item 2) and answers the `Connect` that follows as its
first one, so it follows every move to the reference log's final head (a relay fenced at a `Fence`, ENG-RECV-007,
would hold it there). `OracleRun::moves` lists the `Fence`s it followed, with the engine's `MoveCodeResult`.

- **Removed relay state.** An acknowledgement comes from one when the fork's copy gave it, or when relay 0 gave it
  before a rollback whose lost tail held that frame at that seq, after the settle the reference log holds another
  frame there, or none and the relay refused a restore tail for a frame it made up at or below that seq or one that
  carried it, and no restore tail could have brought it back. One could have (`check::could_restore`, read by the
  relay's state when a request met it, not by the request's answer) when it stored that frame there, or when, not
  refused for a frame the relay made up, it met the relay holding a prefix of the lost tail below the seq (or was
  sent then and lost), with no made-up frame between, from a device that held the lost tail from that head to the
  seq: refused as a whole (a wrong `chain_before` or `first_seq`), at a frame the relay did not make up (a missing
  repair), or never answered, it leaves every later seq that device held to the restore tail (§21.3 steps 1 and 2),
  not to another device's entry. Not a request refused `relay/not_next` with the relay's head at or past its first
  seq (another device restored those seqs first, §21.3 step 3: the device resumes from that head, and its next
  request is judged on its own), nor one that met another device's entry at the head (the race); a seq a request that
  carried it was refused over for a made-up frame comes back with no request. A restore tail owed and never sent is
  judged as one that never reached the relay, by the relay's state when it fell due: a seq is not left to another
  device's entry because the device that held it sent nothing, stopped after a `relay/not_next` it should have
  resumed from, or after a stored request it should have continued.
  "Refused for a frame the relay made up" is a refusal at that frame with the code the relay gives what it made up
  (`check::MadeUp`): `relay/wrong_board` for another board's frame (the code brief D6 names), `relay/bad_signature`
  for a bad signature (§31.6 makes up both; gate 107, 1047, 1062 and 2282 have it); any other code there refused the
  request for something else. Anything else is a plain rollback, repaired, not removed: with every restore tail
  refused at its first frame, refused `relay/chain_mismatch`, sent with a wrong `chain_before`, or lost on the way,
  `rollback` fails 12 of 12 (task D6); with none ever sent, 299 of seeds 100..399 (355 loses nothing: its second
  rollback returns the relay to its state before the first).
  Task D7: a dropped seq the reference log holds none at, above its final head, is also removed when a lower seq the
  same rollback dropped holds another frame in the reference log; read strictly, not when a restore tail could have
  brought it back (the general rule). With no such lower seq it is held to the rules above.
- **Never lost silently** (2 and 3): at least one relay acknowledgement, every one from a removed state, and an
  evidence alarm raised after that fault (the rollback's or the fork's `fault` line) by a device that held an
  acknowledged frame of it then.
- 2: an acknowledged `entry_id` is accepted exactly once; or held once, class `Refuse`, and reported, after the relay
  sequenced it, by a device it was acknowledged to: a refusal record at its seq, the `PairFailed` of the pairing
  whose `DeviceAdded` it is (`World::registered`), or the `OpDone` of its record's operation; or never lost silently;
  or (task D7) its device, every device it was acknowledged to, was retired after `ReplaceReady` while cut off: every
  acknowledgement came from a removed state (at least one) and no relay produced a message that reached it at or after
  the earliest such fault's trace line, or the reference log holds it once, refused, above the device's `log_reach`.
  Otherwise the text names the outcome that failed (`lost to a removed relay state (…), and no device that held its
  frame raised an alarm …`, `lost to a rolled-back relay state that is not removed (…)`, `acknowledged at … by a
  relay state no rollback or fork removed`, `… finally refused at …, and its device reported no …`). Second half as
  before: every device whose final `B` is past the seq applied it exactly once (`CountedToy`, `src/sim/counted.rs`),
  read on the log it followed; an entry an accepted Skip at or below `B` targets is expected 0 times.
- 3: accepted once; or still in the outbox of a device halted or stopped with a persisted reason, or revoked; or
  refused with a refusal record; or dropped as a cancelled hold; or never lost silently; or its device (the one that
  enqueued it) retired while cut off, as 2 reads it.
- 4: the confirmed state at every sampled `B` past the device's join against the oracle's first state at `B` on the
  log it followed (`check::oracle`: a fresh engine, no faults; `keep_first`), except between a Skip's target `t` and
  the Skip `s`: a state whose device last crossed `t` (before that state, on that log) with `K ≥ s` there is that of
  a device that never applied `t` (§12.4 step 1, §17.3 step 1; a crash halt on `t` holds the board below `t` until
  the Skip clears it) and is compared with `check::replay_without`: the toy applying the ledger's accepted `cmd`
  entries from the oracle's first state at `t − 1` with `t` left out, upgraded where the membership activates a rules
  version, a `ResetTo` of `r` at `q` applied as such a device applies it, in a board apply with §28.1 step 1's
  comparison: `SM::reset(state_r, state_{q−1})` when its state at `r` (the replay's own, the oracle's below `t`) has
  the board of the oracle's first state there, whose hash the entry names; with another board there the replay stops
  before `q` (the open reading below), as it stops at a `Panic` or a Skip; it counts only as far as the same replay
  run as the oracle ran agrees with the oracle. A `B` in `t..s` that no expected value covers is a violation (ruling
  6). The toy stamps each key with the seq that set it: the log without
  `t` keeps the seqs (a twin whose `t` is a `Fail`), it is not a log one entry shorter. Alarms of §21.1 for
  contradicting welcomes and pages, `SkippedFrame` for injected frames, and the duplicated answer of a read (task D5)
  as before. Task D7: an injected frame above an accepted `Fence` that
  names another host than the relay that showed it (that `Fence` fenced it), in the log that holds the frame (the
  reference log, or a branch such as the old relay after a move), owes no `SkippedFrame`; instead no identity's
  `AppendLog` may have carried it (`took`, ENG-RECV-007).
- 5: deletions explained as listed in §31.7 and tasks D2 to D5 (a `Register` drop needs its `PairFailed` in the same or
  the next call, not asked when a crash cut either call, task D6); a deletion counts once its write is applied. Each
  is read on the log the device followed at its `B` (task D8: every `Deletion` carries the device's `C_B`, so a purge
  that a fork's copy sequenced explains the truncation, base deletion and root drop of a device on that copy).
- 6: also no `Append` other than of beats, `snap` entries and the rules-upgrade proposal in the last 1,800,000 ms.
- Roots: every `(epoch, root)` any device held; each log is classified with the choice per epoch that opens it
  furthest (`check::pick_roots`).

## The simulator's own tests (task D4)

A test of the simulator holds whatever the engine does at a seed: no test names a seq, a device number or a trace
line that a run happened to produce. In this order of preference:

- **A quiet `World`** (no fault, no operation): `world.rs` runs quiet seed 3, changes one fact (an archive row, a
  sample, an acknowledgement, a deletion, `tamper_facts`), and asserts which invariants the change breaks; it also
  checks that the run records the facts above. `a_quiet_run_holds_every_invariant` is the one unit test that asks
  the engine something.
- **A hand-built log** (`src/sim/script.rs`): `HandLog` creates a board with the creator's fake ("Desk"), seals
  Desk's commands, `DeviceAdded` (also one entry sealed twice), beats, rules upgrades and rotations, seals Skips as
  the recovery identity, has the model relay make up a frame (`inject`), moves the board (`move_to`: the relay sequences
  Desk's `Fence`, a new relay is seeded, the old one is handed back), and sequences them; two `HandLog`s of one
  seed built with the same first steps are a fork or a rollback's lost tail (the second draws one entry id it never
  seals). `Case` holds the facts `check::check` is handed. No engine runs except the §31.7 oracle; where a test leans
  on what it confirms, it asserts that as a premise.
- **A rollback scripted on a quiet `World`** (`world.rs`, `rolled_world`): quiet seed 3 at its end, some identities
  stopped, the relay's state marked, Desk's edits appended, the relay rolled back to the mark by the fault's own
  code (the quiet fault rates are zero, so only these faults happen); then sessions, starts and edits by hand.
  `World::drop_restore_tails` (tests only) drops the restore tails of the identities and first seqs it names before
  they are recorded or sent: an engine that sends none, or never resumes. Premises are asserted.
- A bounded seed search only for a situation nothing else can build; none is needed.

Each test fails when its rule is removed (tasks D6 to D8 removed or widened each rule in turn and ran the tests). D4's
open item: no oracle run can tell `keep_first` from an overwrite, because the oracle's confirmed head never returns to
a seq it passed (a Skip replay is not shown through `confirmed`); `the_oracle_keeps_its_first_state_at_each_b` tests
the rule itself. A `HandLog` also seals Desk's purge rotations (`purge`) and `ResetTo`s (`reset_to`, with the
oracle's state hash at the target, which the oracle checks, §28.1) (task D8).

## Open reading (task D6, for the owner)

The owner decided task D6's other two (a device retired while cut off; a dropped seq above the final head): task D7.

- A restore tail owed (D6's second fix pass): the check gives a device in session `RESTORE_DUE` (the response
  timeout, 20,000 ms) to send it once the relay shows a head below its log on its chain. One sent within that time
  is judged by what it met (another device's entry first: the race, removed); none sent by then is owed, judged by
  the relay's state when it fell due, also when a request it did send (a repair of §21.3 step 2) was lost on the way,
  as a lost restore tail is. A debt that a session's or process's end cuts before it falls due is none. Every device
  that holds more than the relay on its chain owes one, whatever other evidence it holds (§21.3: "as any device does
  that holds more than the relay"), except a halted one (§16.4). This replaces the reading that took a device that
  sent no next request after a stored one, or none after `relay/not_next`, for the race.

## Open reading (task D8, for the owner)

- A `ResetTo` at `q` of a target `r` inside a Skip's window (`t ≤ r < q < s`), where `t` changed the board at `r`. A
  device that never applied `t` applies the `ResetTo` in a board apply (§16.2, §17.3 step 1): §28.1 step 1 compares
  its hash at `r` (without `t`) with the entry's (with `t`), raises `ResetMismatch`, leaves its board and stops
  (§22.2). A replay through `q` resets without comparing (§28.1, last paragraph), as the oracle's Skip replay at `s`
  does (ENG-SKIP-002). Read one way, invariant 4 expects the reset board at `q`, which only an engine that skips
  ENG-RESET-002's comparison holds; read the other, the unchanged board at `q`, and that device then differs from
  the oracle at `s`. The check takes neither: no expected value from `q` (a violation, the stricter reading). Where
  `t` left the board at `r` unchanged, both readings give the reset board, and the check expects it.
