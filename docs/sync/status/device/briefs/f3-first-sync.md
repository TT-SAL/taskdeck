# F3: the runtime, creating a board, pairing, and the first sync end to end

Workflow W1b, after W1a (H, F1, F2). Common rules: `README.md` beside this file. Task id `f3`
(a continuation agent, if one runs, is `f3b` and starts from the last pushed commit and the
previous agent's `next`).

## Goal: the milestone "device: first sync"

Two `taskdeck-devicectl` processes, the real core and the real seal in the engine: one creates a
board, the other pairs with it, an edit made on each reaches the other through `relay/standby.js`
on loopback, and each still holds the board after it is stopped and started again. It is a test
under `tests/e2e/` that CI runs (`tests/e2e/ci.sh`, Linux).

Work in two parts, each ending green, committed and pushed:

- **Part A, one device end to end** (plan §6.7 item 2: "one command end to end through the standby
  relay before anything else"): the test `one_device_round_trip`: devicectl creates a board on the
  standby, submits an intent, the entry is appended, read back and applied (its status shows the
  entry confirmed), and after `shutdown` and a new start on the same folders the device holds the
  same board with the same state hash and takes another edit.
- **Part B, the first sync**: the test `first_sync`: device A creates, device B joins with the
  address and code of A's `pair_start`, A sees the six digits, B types them (`join_sas`), A
  confirms; B ends up holding the board (the same state hash as A's); an edit on A reaches B and an
  edit on B reaches A (both state hashes equal again); both are shut down and started again and
  still hold the board with that hash.

## Paths

`crates/taskdeck-device/` (all of it: the runtime, transport, facade, flows, the binary
`src/bin/taskdeck-devicectl.rs`), `tests/e2e/`, `docs/sync/status/device/fit.md`. Engine faults
only as the README says (a commit of its own, `engine:`).

## Start

1. What H, F1 and F2 left for you (their `next`) is `docs/sync/status/device/w1a-next.md`: read it
   first. H's harness is merged already
   (`tests/e2e/harness/`, its README says how a scenario is written).
2. Read the code F1 and F2 wrote (`src/adapt/`, `src/store*`, `src/keystore/`) before you build on
   it; the skeleton in `src/lib.rs` is device.md §25 as code. Read the status file's "Where the
   parts did not fit" table (W1a's judged findings) and `fit.md`'s entries it names when they touch
   what you build: they are settled, and the spec text for them is merged before you start.
3. Only an empty board can be created today (fit f1-5, open): `create` with `source: "empty"`.

## Read (sections; never a whole spec)

- `device.md`: §4.2 to §4.5 (threads, the loop, published views, calls), §8 (the runtime: inputs,
  effects, what the store adds, notifications), §10.1 to §10.6, §10.9 and §10.10 (transport: stack,
  headers, tokens and login, session id, effects and their requests, errors, polling, timeouts),
  §11.1 (what `test-trust` allows: `http` on loopback, the manual clock), §14 only as far as the
  snapshot blob of a pairing needs, §16 as far as tokens per relay host need, §17.2 to §17.4,
  §17.8 and §17.10 (opening and shutdown, change notification, intents, status, pairing and
  creating), §18.1 and §18.3 (creating a board, pairing), §19 (devicectl), §20.1, §21, §22.
- `engine.md`: §4 (inputs, effects, one call in order), §10 and §11 (send, sessions, reconcile,
  admin requests), §12.1 and §12.2 (pages, when the engine reads), §19.6 and §19.8 (blob requests,
  bootstrap), §28.4 (adding a device), §30.1 (`Status`), §36 (interfaces), §37 for the types.
- `seal.md`: §16.1 (login), §17 (pairing: the sequence, code, slots, SAS, bundle, N's checks, the
  two state machines of §17.10, limits and aborts of §17.11), §20 (snapshots and blobs), §25 and
  §26 for the calls.
- `relay.md`: §5 (credentials, the relay address, header forms), §9 and §10 (sequencing; the routes
  you call: `create`, login, `hello`, `read`, `append`, snapshots and blobs, pairing tickets and
  slots, `caps` and `door`), §12.1 and §12.3 (the admin envelope, `register`), §14.5 (what a client
  sees and how it tells the cases apart), §15 (the standby: polling, no socket).
- `docs/sync/spec/CHANGES.md`, the row "Left open by stage `seal` for the pairing runtime" (six
  points), and `docs/sync/status/seal.md` "What the next stages need to know", "For `device`"
  (pairing runtime). **Decide each of the six points** as the runtime needs it, implement the
  decision, and record it in `fit.md` as a `decision` with its reason.

## Build

- **Transport** (§10): HTTPS requests over `ureq` with the crate's rustls configuration, one at a
  time on the thread `taskdeck-https`, and `http` to `127.0.0.1` only with `test-trust` (§11.1
  item 1; DEV-TRUST-004 holds without it). The headers of §10.2, the login and tokens of §10.3 (the
  token kept per relay host, never written outside the store, never logged), a session id per
  process, every request the engine's effects call for (§10.5), the answers classified as
  `relay.md` §14.5 says and handed to the engine as its inputs. The standby has no socket: poll
  `hello` and `read` while the device is in the foreground (§10.9; devicectl starts in the
  foreground). **Not now:** the socket thread, backoff with jitter (a fixed short retry delay is
  enough, noted as a shortcut in `fit.md`), budgets, platform trust beyond what `ureq` needs to
  build.
- **Runtime** (§4.2 to §4.4, §8): the sync thread and its loop, the three queues, the engine's
  effects performed (persists synchronously through F2's store, each answered `Persisted` in
  order), ticks from `ScheduleTick` against the device's clock (the test clock of `OpenOptions` with
  `test-trust`), published views, change counters, notifications.
- **Facade** (§17, §25), the part this milestone needs: `open` (synced mode for a folder that holds
  a board; a folder with none opens empty, ready to create or join), `shutdown` (joins every thread
  it started, DEV-THREAD-003), `changes`, `wait_for_change`, `submit_json` and `submit` (the
  compile step against the pending view, DEV-FACADE-001), `view()` with `board()`, `status()`,
  `begin_create` with `CreateFlow::sheet`, `confirm_sheet` and `finish`, `pair_start`,
  `pair_confirm`, `pair_cancel`, `join`, `join_sas`, and what `status()` must show of a pairing.
  Every other facade method stays as the skeleton left it.
- **Pairing** (§18.3 with the decisions above): both roles of seal's machines, fed monotonic time
  and ticks by the runtime (SEAL-PAIR-013), `UserConfirmed` only on `pair_confirm`
  (SEAL-PAIR-008), the relay's pairing tickets and slots, the engine's side (`DeviceAdded`,
  `register`), the snapshot blob the new device needs, and the new device started with
  `Loaded.bootstrap = Join { .. }`. The digits typed are passed as six ASCII digits.
- **devicectl** (§19): `run` with the options of §19.1 that the two tests use, the protocol of
  §19.2 exactly as its paragraph "Answers" lays it out (DEV-CTL-003; strict JSON lines; DEV-CTL-004
  for an invalid line, an unknown `cmd` and invalid arguments; `changed` and `exit` events), and the
  commands:
  `status`, `board`, `submit`, `create`, `sheet`, `confirm_sheet`, `finish_create`, `pair_start`,
  `pair_status`, `pair_confirm`, `pair_cancel`, `join`, `join_sas`, `wait`, `clock`, `shutdown`,
  `kill`. Any other command of the table is answered with its id and `device/ctl_request` for now
  (noted in `fit.md` as a shortcut). DEV-CTL-002: no token, key or create secret in its output; a seed or code
  only in the answers of `sheet` and `pair_start`.
- **The two tests** under `tests/e2e/` with H's harness. With the manual clock, whatever waits on a
  tick (polling, the pairing machines' limits) moves only when the test advances the clock; write
  the tests so (advance in steps, wait on the change counters), not with `--clock system`.

- **Small follow-ups from W1a** while you are in the code: `IdentityStore::open` fails with
  `device/write_failed` when a write during open fails (fit f2-6, `device.md` §7.4); cite
  DEV-FILE-012 in the store test that checks a gap above the checkpoint if one exists, else add that
  test; cite DEV-CTL-003 and DEV-CTL-004 in devicectl's tests.

## Done

The README's green commands, `bash tests/e2e/ci.sh` (both tests pass; run it three times in a row
to see that it is stable), and `cargo test --locked -p taskdeck-sync` if you changed the engine.
Commit and push each part. Every place where the real parts did not fit, where a spec was wrong,
unclear or silent, and every decision, is in `fit.md`. In `next`, say what the facade still lacks
that `desktop` will need first, and every shortcut taken (§11.1 or otherwise).
