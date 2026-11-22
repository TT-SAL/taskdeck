# android/

The Android app: the Gradle project, the Kotlin shell, the hardened WebView, the forked phone page (`android/page/`) and the vendored platform-verifier component. The JNI library it loads is `crates/taskdeck-android`.

Nothing lives here yet: the directory is created by stage `foundation`.

- Owner: stage `android-shell`, then `android` (SYNC_PLAN.md §4).
- Spec: `docs/sync/spec/bridge.md`.
- CI: `bash scripts/ci/stage.sh android` runs `bash android/ci.sh` from the
  repository root if that file exists, with `CI_STAGE`, `CI_OS` and `CI_WIDE`
  set (the contract is at the top of `scripts/ci/stage.sh`). The stage's own
  checks go in that `ci.sh`, not in `ci.yml`.
