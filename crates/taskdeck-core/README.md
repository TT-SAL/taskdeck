# taskdeck-core

The pure board: data, log commands, the deterministic versioned `apply`, planner, archive, scoring, the ICS parser, the snapshot builders and the legacy import. No clock, zone, file, network, thread or environment access: `clippy.toml` here disallows them, and the crate builds for `wasm32-unknown-unknown`.

- Owner: stage `core` (SYNC_PLAN.md §4). Created as a shell by stage `foundation`.
- Spec: `docs/sync/spec/core.md`.
- CI: `bash scripts/ci/stage.sh core` runs `cargo test --locked -p taskdeck-core` and
  `cargo clippy --locked -p taskdeck-core --all-targets -- -D warnings` from the
  repository root, then `bash crates/taskdeck-core/ci.sh` if that file exists, with
  `CI_STAGE`, `CI_OS` and `CI_WIDE` set (the contract is at the top of
  `scripts/ci/stage.sh`). The stage's own checks go in that `ci.sh`, not in
  `ci.yml`.
