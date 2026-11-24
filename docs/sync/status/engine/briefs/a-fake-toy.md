# Task A: the fake sealer, the toy state machine and their vectors

Working copy: `/home/user/wt/a` (branch `wt/a`). `CARGO_TARGET_DIR=/home/user/wt/target-a`.
Read `docs/sync/status/engine/briefs/README.md` first: its rules hold here.

## Why

`foundation` left `FakeSealer` (`src/fake.rs`, tests in `src/fake/tests.rs`) and `ToyMachine`
(`src/toy.rs`) as working code. Since then the specs changed, above all `seal.md` (the owner's
decision on the recovery key, `docs/sync/spec/CHANGES.md` 2026-11-23: no `propose`, `confirm` or
`cancel` of a recovery key, no 72-hour window; the recovery key changes only by a `K`-shape entry
signed by the current recovery identity). The lead has already brought the public types to
`engine.md` §37 as it is now (`src/model.rs`: `RecoveryMode`, `PendingRecovery`,
`MembershipView.pending_recovery` and `CtlIntent::RecoveryChange` are gone;
`CtlItem::RecoveryKeyChange { key_fp: String }`), so the crate does not compile with the feature
`sim` until the fake follows. Every type also gained `#[derive(Clone, Debug, PartialEq, Eq)]`.

Everything else in stage `engine` (the engine's tests, the model relay, the simulator) is built on
the fake and the toy, so this task comes first.

## What to do

1. **Remove the withdrawn recovery-key path from the fake** and from its tests (one test cites the
   withdrawn clause SEAL-MEM-025; `scripts/spec-coverage.sh engine` must report no citation of a
   withdrawn clause afterwards).
2. **Bring `FakeSealer` to the specs as they are now.** It is `seal.md` §5, §8 to §15 and §20 with
   exactly the substitutions of `engine.md` §6.3, and nothing else changed (ENG-FAKE-001 to
   ENG-FAKE-003), behind the `Sealer` trait of `engine.md` §6.1 with every contract of its table.
   Do not trust the code as `foundation` left it: go through `git log --oneline -- docs/sync/spec/seal.md
   docs/sync/spec/engine.md` since the commit that added `src/fake.rs`, read each spec change's row
   in `CHANGES.md`, and check the fake against every clause those changes touched, and against
   `seal.md` §14.2 step by step. Known points: the 30-day condition of `seal.md` §13.9 is checked
   at step 9 before decryption (`seal/stale_epoch`); a rotation that lost a race is refused with
   `seal/stale_epoch` or `seal/old_epoch`; `open_snapshot` with `exact = true` recomputes the state
   hash from the body's board and membership (`seal.md` §20.5 step 7); the control history in
   snapshot bodies and `FakeBundle` (`engine.md` §6.3 "Control history", ruling S27);
   `take_missed_wraps` and what `classify` does with a missed wrap and with an own rotation
   (`engine.md` §6.1 table, ENG-SEALER-009); the recovery identity's `K`-shape and `R`-shape
   entries (`seal.md` §12) must classify, because the simulator's workload seals them as the
   recovery identity (`engine.md` §31.5): give the fake a `pub(crate)` way to seal an entry as the
   recovery identity of a board it created, and one to build the `reinstate` wraps (`seal.md`
   §16.2) for a revoked device.
3. **Check `ToyMachine` against `engine.md` §5.3 and §5.1 as they are now**, line by line, and fix
   what differs (ENG-TOY-001, ENG-TOY-002).
4. **Publish the vectors** (`engine.md` §35, format: `docs/sync/spec/README.md` §7) under
   `crates/taskdeck-sync/tests/vectors/`: `engine-splitmix.json`, `engine-toy.json`,
   `engine-fake.json`, with the contents the table of §35 lists (for the fake: one failing case per
   step of `seal.md` §14.2). The expected values must not come only from the code under test:
   where a value can be derived independently (SHA-256 over a layout written out by hand, the
   SplitMix64 reference outputs, the toy's byte layouts), derive it with a small script of your
   own (Python is fine, kept outside the repository) and compare. Say in your report which values
   are independent and which are the code's own output recorded.
5. **Tests that load the vectors**: `crates/taskdeck-sync/tests/vectors_toy_fake.rs` (an
   integration test; it sees `taskdeck_sync::{toy, fake, sim::SplitMix64}` because the crate's
   test builds have the feature `sim`). Cite the clauses in comments as `spec: ENG-TOY-001` and so
   on: ENG-TOY-001, ENG-TOY-002, ENG-FAKE-001, ENG-FAKE-002, ENG-FAKE-003, and the `seal.md`
   clauses whose owner list includes `engine` and that the fake proves, if any (the clause index
   shows owners in brackets).
6. **Write `docs/sync/status/engine/api-fake-toy.md`**: at most 120 lines. Every `pub` and
   `pub(crate)` item of `fake.rs` and `toy.rs` with its signature and one line on what it does and
   in which order it draws random bytes, how a test builds a board (`create`), a second device
   (`join_keys`, `bundle`, `join`), a recovery-signed entry and reinstate wraps, how to make a
   frame that fails each step of `seal.md` §14.2, and the toy's origin state. The engine's author
   and the simulator's author will program against this file without reading `fake.rs`.

## Files you may change

`crates/taskdeck-sync/src/fake.rs`, `src/fake/**`, `src/toy.rs` (and a `src/toy/tests.rs` if you
split one out), `src/sim/splitmix.rs`, `tests/vectors/engine-{splitmix,toy,fake}.json`,
`tests/vectors_toy_fake.rs`, `docs/sync/status/engine/api-fake-toy.md`. Nothing else: not
`src/lib.rs`, not `src/model.rs`, not `src/sim.rs`, not `Cargo.toml`. If the public types are
wrong against §37, or you need a change outside your files, say so in your report; do not make it.

## Done means

In `/home/user/wt/a`, with `CARGO_TARGET_DIR=/home/user/wt/target-a`:

- `cargo test --locked -p taskdeck-sync --message-format=short` passes;
- `cargo clippy --locked -p taskdeck-sync --all-targets --message-format=short -- -D warnings` passes;
- `cargo check --locked -p taskdeck-sync --features sim --target wasm32-unknown-unknown` passes;
- `bash scripts/spec-coverage.sh engine` no longer reports a citation of a withdrawn clause (it
  still fails for the clauses the engine owns that no test cites yet: that is expected);
- everything is committed on `wt/a`, `git status` is clean.

Report: what differed between the fake or the toy and the specs (clause by clause), what you
changed, anything in the specs that is unclear or contradicts itself (clause ids), and the token
count you used if you know it.
