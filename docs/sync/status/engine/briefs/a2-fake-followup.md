# Task A2: four things the engine and the simulator need from the fake

Working copy: `/home/user/wt/a2` (branch `wt/a2`). `CARGO_TARGET_DIR=/home/user/wt/target-a`.
Read `docs/sync/status/engine/briefs/README.md` first: its rules hold here. Task A
(`a-fake-toy.md`) is done and merged; this is its follow-up. Do not read `src/engine/**`.

In this working copy the crate does not compile with the feature `sim`: the lead added a method
to the `Sealer` trait (`src/model.rs`) that the fake does not implement yet. That is item 1.

1. **`Sealer::membership_zero(&self) -> Self::Membership`** (new in `src/model.rs`; a spec change
   to `engine.md` §6.1 follows): `M_0`, the membership before seq 1 (`seal.md` §13.3). The engine
   hands it to `classify` for the genesis entry. Implement it for `FakeSealer`.
2. **`seal` must seal the entry it is given.** `api-fake-toy.md` says "`seal` returns the stored
   frame for a counter it already sealed (no draw)". The engine never asks for the same entry
   twice under one counter (ENG-SEALER-005: it keeps the frame in its record), but it does seal a
   *different* entry, or the same entry under a newer epoch, with a counter the fake has seen:
   after a frame was cleared for re-sealing before it reached the relay (`engine.md` §11.3 step 3,
   §11.4, §24.3), `next_ctr` (§10.3) can be that counter again. Returning the stored frame then
   hands the engine a frame of another entry or another epoch. Remove the stored-frame answer, or
   limit it to the case where the entry (id, kind, `at_ms`, `based_on`, body) and the membership's
   epoch are the same as when it was stored. Say in `api-fake-toy.md` what `own_sealed`-related
   state the fake keeps, if any.
3. **`classify` may be called again for a seq it classified before**, with the same inputs: after
   a restart the engine classifies the log above its checkpoint again, and replays (a Skip, a
   `ResetTo`) classify old entries again. It must then return the same receipt (apart from
   `keys_changed`), must not add a second history record for a control entry, must not fail
   because a pending root is already gone or a root is already known, and must keep working when
   the calls go back to a lower seq and forward again. Add tests.
4. **Rebuilding a fake after a restart.** The simulator's model runtime (`engine.md` §31.4) keeps
   only what the engine persisted: the bytes of `export_identity()` from the last durable
   `WriteIdentity`. Give the fake, crate-private:
   - `history(&self) -> Vec<Vec<u8>>`: its control history records (the runtime persists them,
     as `device.md` §15.7 does for the real sealer), and
   - `restore(identity: &[u8], history: Vec<Vec<u8>>, reseed: u64, supported_rules: &[u32]) ->
     Result<FakeSealer, SealerError>`: the fake that a new process would have: keys, roots,
     pending roots and anchor from the identity bytes, the given history, and a generator
     seeded with `reseed` (a restart must not replay the old generator: it would draw the same
     entry ids again). Missed wraps are not restored (`engine.md` §6.1: never persisted).
   A fake restored from `export_identity()` and `history()` of a live one must classify, seal
   (with other random values), open and seal snapshots exactly as the live one would. Add tests,
   and one case to `engine-fake.json` if the identity layout is not yet covered by a vector.

Update `docs/sync/status/engine/api-fake-toy.md` for all four (keep it under 130 lines).

Files you may change: `crates/taskdeck-sync/src/fake.rs`, `src/fake/**`,
`tests/vectors/engine-fake.json`, `tests/vectors_toy_fake.rs`,
`docs/sync/status/engine/api-fake-toy.md`. Nothing else. `src/engine/**` does not compile into
your tests' concerns: if a test build fails inside `src/engine/`, report it and stop; do not
edit it.

Done means, in `/home/user/wt/a2`: `cargo test --locked -p taskdeck-sync --message-format=short`
passes; clippy with `--all-targets -- -D warnings` passes; the wasm32 check with `--features sim`
passes; everything committed on `wt/a2`; `git status` clean.

Report: what you changed, and anything in the specs that is unclear (clause ids).
