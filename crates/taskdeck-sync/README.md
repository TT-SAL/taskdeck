# taskdeck-sync

The sans-IO replica engine, generic over the state-machine and sealer traits it defines itself (decision D6), and its simulator behind the feature `sim` (off by default, never in a release build). No clock, file, network, thread or environment access: `clippy.toml` here disallows them.

- Owner: stage `engine`, then `device` (SYNC_PLAN.md §4). Created as a shell by stage `foundation`.
- Spec: `docs/sync/spec/engine.md`.
- CI: `bash scripts/ci/stage.sh engine` runs `cargo test --locked -p taskdeck-sync` and
  `cargo clippy --locked -p taskdeck-sync --all-targets -- -D warnings` from the
  repository root, then `bash crates/taskdeck-sync/ci.sh` if that file exists, with
  `CI_STAGE`, `CI_OS` and `CI_WIDE` set (the contract is at the top of
  `scripts/ci/stage.sh`). The stage's own checks go in that `ci.sh`, not in
  `ci.yml`.
