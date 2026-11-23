# taskdeck-seal

Frames, contexts, envelope, keys, membership, chain, pairing, HPKE wraps, sealed files and the backup format, as pure functions and sans-IO machines. The feature `test-entropy` (off by default, never in a release build) is for the test vectors.

- Owner: stage `seal` (SYNC_PLAN.md §4). Created as a shell by stage `foundation`.
- Spec: `docs/sync/spec/seal.md`.
- CI: `bash scripts/ci/stage.sh seal` runs `cargo test --locked -p taskdeck-seal` and
  `cargo clippy --locked -p taskdeck-seal --all-targets -- -D warnings` from the
  repository root, then `bash crates/taskdeck-seal/ci.sh` if that file exists, with
  `CI_STAGE`, `CI_OS` and `CI_WIDE` set (the contract is at the top of
  `scripts/ci/stage.sh`). The stage's own checks go in that `ci.sh`, not in
  `ci.yml`.
- Tests: unit tests beside each module (`src/<module>/tests.rs`); integration
  targets under `tests/` (`kat`, `tamper`, `contexts`, `pairing_mitm`, `parsers`,
  the stage gate's five; `vectors` and `fuzz_targets` with `test-entropy`; `memory`;
  `crate_rules`).
  The tamper, context and parser matrices are the comment tables at the top of
  `tests/tamper/main.rs`, `tests/contexts/main.rs` and `tests/parsers/main.rs`.
- Fuzz targets: `fuzz/`, a crate of its own (own `[workspace]` and lockfile), one
  target per row of the parser table; `ci.sh` compiles them on Linux, the test
  target `fuzz_targets` runs each body and its fixture over fixed inputs and
  proves, with inputs it builds, that the targets of the openers and of
  pairing (`reach_*` in `fuzz/src/lib.rs`) reach the parsers their rows name;
  stage `release` runs the fuzzers. `crate_rules` keeps `fuzz/Cargo.lock` within the
  root `Cargo.lock`.
- Budget: `cargo run --release --locked -p taskdeck-seal --example budget` (run by
  `ci.sh` on Linux); `budget-baseline.txt` holds the baseline in microseconds,
  as the CI job measured it on GitHub's Linux runner. The 100 ms limit is judged
  everywhere, the 25% rule only there (`--judge-baseline`, which `ci.sh` passes
  on GitHub's runners).
