# Task C: the model relay and its conformance test

Working copy: `/home/user/wt/c` (branch `wt/c`). `CARGO_TARGET_DIR=/home/user/wt/target-c`.
Read `docs/sync/status/engine/briefs/README.md` first: its rules hold here. Do not read
`src/engine/**`.

## Why

The simulator (`engine.md` §31) runs engines against a **model relay**: `relay.md` implemented
over in-memory tables, with no I/O and no clock of its own. It is half of what judges the engine,
so it is written from `relay.md`, not from the engine, and it is tested against the relay's own
published conformance cases (ENG-SIM-001).

## What to build

1. **`taskdeck_sync::sim::relay`** (`crates/taskdeck-sync/src/sim/relay.rs`, more files under
   `src/sim/relay/` if you want them; the lead has declared `pub mod relay;` in `src/sim.rs`).
   `engine.md` §31.3 lists exactly what it models: `relay.md` §7 (the tables it needs: board,
   devices, log, blobs, parts), §8.1 (board states), §9 (the batch: P1 to P7 with the fake
   signature of `engine.md` §6.3, T1 to T5 with the budget), §10.4 `hello`, §10.5 `read`
   (selection, the page fields, `truncated`), §10.7 and §13 (blobs and their retention), §11.3
   and §11.4 (hello, welcome, `second`, `displaced`, push to every ready socket), §12.3 to §12.10
   and §12.12 (`register`, `revoke`, `reinstate`, `fence`, `seed` and `seed-tail`, `compact`,
   `restore_tail`, `purge`, `recovery_register`, with their own checks), §14.1 and §14.2 (the
   append and delete budgets). It does **not** model the gate, the door, token MACs, nonces or
   admin signatures: an admin request passes `relay.md` §12.1 steps A4 and A6 by construction,
   and A5 is checked against its rows. Its clock is a `now_ms` the caller passes with every
   call (the simulator's virtual clock plus a per-relay offset).
   - It speaks the engine's types where they exist, so the simulator can hand effects straight
     to it: `Welcome`, `Page`, `PageEntry`, `AppendResult`, `AdminRequest`, `AdminOk`,
     `RelayRefusal`, `BlobRequest`, `BlobOk`, `BlobMeta`, `SeedDevice`, `Head`, `Dev`
     (`src/session.rs`, `receive.rs`, `snapshots.rs`, `events.rs`, `model.rs`). Where the model
     needs something those types lack (who the caller is, the caller's session id, the login
     answer's `wraps` of `relay.md` §10.3), it is an argument or a return value of your API.
   - Frames are the fake's: the frame layout of `seal.md` §8.1 with the fake signature of
     `engine.md` §6.3. Verify the outer signature yourself from §6.3's formula (`sha2` is a
     dependency of the crate); do not call into `src/fake.rs` for it, and do not edit it.
   - Sockets: the model keeps which devices have a ready socket and returns, from each call,
     the messages it would push to other sockets (`push` pages, `second`, `displaced`, a fenced
     `state`), addressed by device and session, for the simulator to deliver.
   - What the simulator's faults need (§31.6): the whole relay is `Clone` (a rollback restores an
     earlier clone; a fork serves some devices from a clone); a way to store a frame as it is
     (an injected frame with a bad signature or another board's id); a way to list its log and
     blobs for the invariants (§31.7).
   - Deterministic: no `HashMap` iteration that can change an answer, no clock, no randomness.
2. **The conformance test** `crates/taskdeck-sync/tests/relay_conformance.rs`: runs the relay's
   published cases (`relay/test/vectors/*.json`; `relay/test/vectors/README.md` describes the
   format) against the model. The cases are symbolic: the relay's own harness
   (`relay/test/lib/harness.mjs`) expands them into real frames; your test expands them with the
   fake sealer (`taskdeck_sync::fake`, see `docs/sync/status/engine/api-fake-toy.md`) or with
   frames you build from §6.3. Embed the vector files with `include_str!` (the crate's clippy
   configuration forbids file access); parse them with `serde_json` (a dev-dependency).
   State exactly, in the test file's header and in your report, **which families and which
   steps apply** to a model that has no gate, tokens, nonces or signatures, and why each
   left-out case is left out. A case that applies must pass; do not skip one because the model
   is wrong, fix the model. Cite `spec: ENG-SIM-001`, and the `relay.md` clauses whose owners
   include `engine` (RELAY-SEQ-001 to RELAY-SEQ-004, RELAY-SEQ-010, RELAY-READ-001,
   RELAY-READ-002, RELAY-SOCK-008; the clause index shows owners) in the tests that prove them.
3. **`docs/sync/status/engine/api-model-relay.md`**, at most 120 lines: every public item with
   its signature and one line; how to create a board from a genesis frame, register a device,
   open a socket, append, read, run each admin operation, upload and fetch a blob; what each
   call returns for pushes; how to clone for a rollback or a fork; how to inject a raw frame.
   The simulator's author programs against this file.

Where `relay.md` leaves the model in doubt, `relay/relay-core.js` and its suite
(`relay/test/`) show what the relay does: follow them, and list each such place in your report
with the clause that should say it. Do not change anything under `relay/`.

## Files you may change

`crates/taskdeck-sync/src/sim/relay.rs`, `crates/taskdeck-sync/src/sim/relay/**`,
`crates/taskdeck-sync/tests/relay_conformance.rs`, `docs/sync/status/engine/api-model-relay.md`.
Nothing else: not `src/sim.rs`, not `src/fake.rs`, not `Cargo.toml`, nothing under `relay/`. If
you need a helper from the fake that is not there, say so in your report.

## Done means

In `/home/user/wt/c`, with `CARGO_TARGET_DIR=/home/user/wt/target-c`:

- `cargo test --locked -p taskdeck-sync --message-format=short` passes (the conformance test
  included);
- `cargo clippy --locked -p taskdeck-sync --all-targets --message-format=short -- -D warnings` passes;
- `cargo check --locked -p taskdeck-sync --features sim --target wasm32-unknown-unknown` passes;
- everything is committed on `wt/c`, `git status` is clean.

Report: the families and steps that apply and those left out (with reasons); every place where
you followed `relay-core.js` because `relay.md` was not enough; spec questions (clause ids).
