# tests/e2e/

The end-to-end suite: devices driven through `taskdeck-devicectl` (feature `test-trust`) against the standby relay on loopback.

Nothing lives here yet: the directory is created by stage `foundation`.

- Owner: stage `device` (SYNC_PLAN.md §4).
- Spec: `docs/sync/spec/device.md` §20.
- CI: `bash scripts/ci/stage.sh device` runs `bash tests/e2e/ci.sh` from the
  repository root if that file exists, with `CI_STAGE`, `CI_OS` and `CI_WIDE`
  set (the contract is at the top of `scripts/ci/stage.sh`). The stage's own
  checks go in that `ci.sh`, not in `ci.yml`.
