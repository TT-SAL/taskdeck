# taskdeck-device

The device runtime: the sealed store, the transport (ureq and tungstenite over rustls with the ring provider and the platform verifier), the key stores, the fetchers, blobs, backup scheduling and the facade. The binary `taskdeck-devicectl` exists only with the feature `test-trust` (off by default, never in a release build).

- Owner: stage `device` (SYNC_PLAN.md §4). Created as a shell by stage `foundation`.
- Spec: `docs/sync/spec/device.md`.
- CI: `bash scripts/ci/stage.sh device` runs `cargo test --locked -p taskdeck-device` and
  `cargo clippy --locked -p taskdeck-device --all-targets -- -D warnings` from the
  repository root, then `bash crates/taskdeck-device/ci.sh` if that file exists, with
  `CI_STAGE`, `CI_OS` and `CI_WIDE` set (the contract is at the top of
  `scripts/ci/stage.sh`). The stage's own checks go in that `ci.sh`, not in
  `ci.yml`.
