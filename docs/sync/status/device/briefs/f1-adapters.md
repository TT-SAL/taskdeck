# F1: the adapters, and the engine on the real core and seal in process

Workflow W1a, chain step 1. Common rules: `README.md` beside this file. Task id `f1`.

## Goal

`CoreMachine` implements `taskdeck_sync::StateMachine` over `taskdeck-core`, and `SealSealer`
implements `taskdeck_sync::Sealer` over `taskdeck-seal` (device.md §4.1 decision D6, §25 "adapters").
Then prove in process that the real engine runs on them: one device, from genesis to an applied edit
and back after a restart. Until now the engine has met only stand-ins (the toy, the fake sealer); what
does not fit is this task's main finding, and goes to `fit.md` as you meet it.

## Paths

`crates/taskdeck-device/src/adapt/` (new: for example `mod.rs`, `core_machine.rs`,
`seal_sealer.rs`, and the in-process test module), the `mod` and `pub use` lines in
`crates/taskdeck-device/src/lib.rs` (§25 names `CoreMachine` and `SealSealer` at the crate root), and
`docs/sync/status/device/fit.md`. Engine faults only as the README says.

## Read

- The two traits as code: `crates/taskdeck-sync/src/model.rs` (`StateMachine` from line 47,
  `Sealer` from line 328) and the types they name. They are the contract; read them whole.
- `docs/sync/status/engine.md`, section "For the device session" (what the engine's lead warns
  about: `catch_unwind` around every call, `classify` repeatable, the release profile unwinds).
- `engine.md` §5.1 (the trait and its contracts), §6.1 and §6.2 (the sealer trait, the membership
  view), §7.10 (`Loaded`), §19.8 (bootstrap at creation and at pairing), §36 (interfaces), the
  sections those send you to for a method you are implementing.
- `core.md` §20 (public API); then as needed §8 (`apply`), §9.1 (parsing commands), §11 (rules
  versions, reset), §7.5 (rebinding), §10.7 (notice and refusal records), §13.1 (compile).
  `docs/sync/status/core.md`, "What the next stages need to know".
- `seal.md` §26 (public API), §25 (the caller's duties, which are yours here: SEAL-FRAME-005,
  SEAL-ENV-003, SEAL-ENV-004, SEAL-MEM-016, and the caller's halves of SEAL-WRAP-003, SEAL-SIG-002,
  SEAL-KEY-012, SEAL-KEY-013); then as needed §13 (membership), §14 (receipt checks), §15 (chain,
  head, fingerprints, move code), §20 (snapshots), §6.4 and §6.5 (device keys, epoch roots).
  `docs/sync/status/seal.md`, "What the next stages need to know".
- `device.md` §6.4.7 (the identity export, file kind 7) and §24 (the `seal.md` and `core.md`
  bullets).
- How the stand-ins answer, for comparison only: `docs/sync/status/engine/api-fake-toy.md`. How
  the engine's lead drove an engine in its own tests: `crates/taskdeck-sync/src/engine/kit_tests.rs`
  (`Store`, `Node`, `Hub`, `creator`); copy what you need into the device crate's test module, do not
  depend on it.

## Build

1. `CoreMachine`: every method of `StateMachine`, wrapping `taskdeck-core` (`apply`, `reset`,
   `ids::rebind`, `RefusalRecord::to_canonical`, the canonical encodings, the hash, the rules
   versions). Check each contract of `engine.md` §5.1 against what core does; where they differ,
   that is a finding.
2. `SealSealer`: every method of `Sealer` over `taskdeck-seal`: the membership and its view and
   bytes, `classify` (seal's receipt checks), `seal`, `open_own`, `new_entry_id` (entropy from
   `taskdeck_seal::OsEntropy`; never the feature `test-entropy`), `discard_pending`,
   `forget_roots_below`, `take_missed_wraps`, the snapshot calls, the fingerprint and the move code,
   `export_identity`. How a `SealSealer` is made: from the keys and genesis a creating device holds
   (what `CreateFlow` will hand it, device.md §18.1), and from the identity bytes of §6.4.7 at a
   later `open`. Keep the seed and private keys in `Zeroizing`.
3. A test module that runs `taskdeck_sync::Engine::<CoreMachine, SealSealer>` with an in-memory
   record store (applying each `PersistOp` and answering `Persisted` in order) and a small test
   relay that sequences frames and answers `Hello`, `Read` and `Append` as `relay.md` §9 and §10.4
   to §10.6 say (chain values with the real chain function). It must show, citing the clauses it
   checks:
   - genesis: the genesis built with `taskdeck-seal` as a creating device builds it (seal.md §12.2,
     §13.3), `Loaded.bootstrap = Genesis { .. }`, the engine accepting seq 1 (ENG-BOOT-004);
   - an edit: an intent compiled by `taskdeck_core::compile` against the engine's view, enqueued,
     sealed, appended, read back, applied; the confirmed state's canonical bytes equal those of
     `taskdeck_core::apply` on the genesis board; `taskdeck_sync::state_hash` of the result;
   - a restart: `Loaded` rebuilt from the persisted records, `Start`, the same state and hash, and a
     second edit after it;
   - each adapter method's contract that the engine relies on and that this flow does not reach,
     in a unit test of its own (for example `classify` repeatable, `load` of `canonical_bytes` is
     the identity, a refused command's refusal record).
4. Every finding in `fit.md` as the README says, including the methods whose meaning was not clear
   from the trait and the spec together.

## Done

The README's green commands, and `cargo test --locked -p taskdeck-device adapt` showing the
in-process flow passing. Commit and push. In `next`, say how F2 (the store) should construct a
`SealSealer` from stored bytes and what `Loaded` fields your test store fills, so F2 can run your
flow over the real store.
