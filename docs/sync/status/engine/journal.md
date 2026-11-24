# Stage `engine`: journal

Append-only. One dated line per decision, finding, trap and result, written when it happens.

- 2026-11-24 Start. Read SYNC_PLAN.md, engine.md in full (once), V2, SYNC.md §4.3/§4.7/§4.8/§5.4, and the hand-offs of foundation, seal (E2, E4), core, relay.
- 2026-11-24 Finding: the skeleton (`src/model.rs`, `ops.rs`, `ui.rs`, `lib.rs`) still had the recovery-key propose/confirm/cancel path that the spec change of 2026-11-23 withdrew; engine.md §37 no longer has it. `docs/sync/status/engine/tools/apidiff.py` compares the skeleton's structs and enums with the spec's Rust blocks: the only differences were that path. Removed by the lead; `src/fake.rs` no longer compiles with `sim` until task A follows (5 errors, all the withdrawn path).
- 2026-11-24 Finding: the skeleton's types had no derives at all. Added `Clone, Debug, PartialEq, Eq` (decision L2); `PersistOp` and `Effect` get a redacting `Debug` and test-only equality because of `WriteIdentity`.
- 2026-11-24 Decision L1: the test targets get `sim` by a dev-dependency of the crate on itself. Trap: it adds one line to `Cargo.lock`, a shared file, so it is TT-SAL/taskdeck#27, alone. Checked locally on that commit alone: `bash scripts/ci/stage.sh engine` passes, 54 tests (the fake's and the toy's now run in CI).
- 2026-11-24 Trap: this session may push only to its own branch, so a "pull request of its own" must be the branch's only commits while it waits for CI (decision L4). Work done meanwhile stays local: keep it small.
- 2026-11-24 Decision L3: `src/sim.rs` is the lead's shared file; `SplitMix64` moved to `src/sim/splitmix.rs` (task A's), the model relay and the simulator get files under `src/sim/`.
- 2026-11-24 Trap (tokens): `cat crates/taskdeck-sync/clippy.toml` prints 120 lines of disallowed paths. Never print it again.
