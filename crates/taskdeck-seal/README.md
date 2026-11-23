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
