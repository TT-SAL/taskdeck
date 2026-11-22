# taskdeck-android

The JNI library `libtaskdeck.so` (the `cdylib` named `taskdeck`): the bridge envelope, the backend behind it, the Keystore key store and the JNI entry points. It is built with exactly one of the features `backend-echo` (stage `android-shell`; links no `taskdeck-device`) and `backend-device` (stage `android`).

- Owner: stage `android-shell`, then `android` (SYNC_PLAN.md §4). Created as a shell by stage `foundation`.
- Spec: `docs/sync/spec/bridge.md` (§11, §21).
- CI: `bash scripts/ci/stage.sh android` runs `cargo test --locked -p taskdeck-android` and
  `cargo clippy --locked -p taskdeck-android --all-targets -- -D warnings` from the
  repository root, then `bash crates/taskdeck-android/ci.sh` if that file exists, with
  `CI_STAGE`, `CI_OS` and `CI_WIDE` set (the contract is at the top of
  `scripts/ci/stage.sh`). The stage's own checks go in that `ci.sh`, not in
  `ci.yml`.
