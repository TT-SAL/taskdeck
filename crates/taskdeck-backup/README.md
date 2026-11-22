# taskdeck-backup

The backup format, its writer and its reader.

- Owner: stage `recovery`, then `device` (SYNC_PLAN.md §4). Created as a shell by stage `foundation`.
- Spec: `docs/sync/spec/seal.md` §21 (the format) and `docs/sync/spec/device.md` §15 (scheduling, destinations, re-reading).
- CI: `bash scripts/ci/stage.sh recovery` runs `cargo test --locked -p taskdeck-backup` and
  `cargo clippy --locked -p taskdeck-backup --all-targets -- -D warnings` from the
  repository root, then `bash crates/taskdeck-backup/ci.sh` if that file exists, with
  `CI_STAGE`, `CI_OS` and `CI_WIDE` set (the contract is at the top of
  `scripts/ci/stage.sh`). The stage's own checks go in that `ci.sh`, not in
  `ci.yml`.
