# taskdeck-recover

The offline recovery tool, a binary: reads a backup with the recovery sheet, prints and re-types the sheet, and runs the recovery signer's operations.

- Owner: stage `recovery`, then `device` (SYNC_PLAN.md §4). Created as a shell by stage `foundation`.
- Spec: `docs/sync/spec/seal.md` §12, §13.12, §18 and §21; `docs/sync/spec/device.md` §15.8.
- CI: `bash scripts/ci/stage.sh recovery` runs `cargo test --locked -p taskdeck-recover` and
  `cargo clippy --locked -p taskdeck-recover --all-targets -- -D warnings` from the
  repository root, then `bash crates/taskdeck-recover/ci.sh` if that file exists, with
  `CI_STAGE`, `CI_OS` and `CI_WIDE` set (the contract is at the top of
  `scripts/ci/stage.sh`). The stage's own checks go in that `ci.sh`, not in
  `ci.yml`.
