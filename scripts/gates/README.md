# scripts/gates

One file per stage of SYNC_PLAN.md §6: what `bash scripts/gate.sh <stage>`
runs and prints. A gate is data, so a stage lead reads exactly what is expected
and a change to a gate is a visible diff. A stage that finds its gate wrong
changes it in a pull request of its own and says why (SYNC_PLAN.md §3.3). Never
weaken a gate to pass it.

```sh
bash scripts/gate.sh --list core        # the items, nothing run
bash scripts/gate.sh core               # run every `run` item, print every item
bash scripts/gate.sh --only golden core # one item
```

## Format

One item per line; blank lines and lines starting with `#` are comments.

```text
run | <id> | <what the plan asks, in a few words, with its § reference> | <command run from the repository root>
ci  | <id> | <what the plan asks, with its § reference> | <the CI job that checks it>
```

- `<id>`: lowercase letters, digits and hyphens, unique in the file.
- The description quotes the plan where it can and always carries a `§`
  reference. Where the plan's wording left a choice, the item takes the
  stricter reading and says so ("stricter reading").
- The command runs with `bash -o pipefail -c`, stdin from `/dev/null`. It
  passes on exit 0. It may hold `|` and `&&`.
- `gate.sh` runs every `run` item, also after one failed, then lists every item
  as PASS, FAIL or CI-ONLY. It exits 0 only if every `run` item passed. An
  unknown stage, a missing or empty file, or a malformed line is an error
  (exit 2).

Every stage's gate ends with these items, in this order, and `gate.sh` refuses
a gate file without them (§3.3):

| Id | Command | What it proves |
|---|---|---|
| `spec-coverage` | `bash scripts/spec-coverage.sh <stage>` | every live clause the stage owns is cited by one of its test files, and no test cites an id that is no clause or is withdrawn |
| `no-unimplemented` | `bash scripts/gate-lib.sh no-unimplemented <stage>` | no `unimplemented!(`, `todo!(` or `unreachable!("TODO` in the stage's owned paths, tests included. Not in `foundation`'s gate: skeleton bodies may be `unimplemented!()` there "and nowhere later" (§6.1 item 4) |
| `no-skipped-tests` | `bash scripts/gate-lib.sh no-skipped-tests <stage>` | no `#[ignore`, `it.skip`, `test.skip`, `describe.skip`, `xit(`, `@Ignore`, `@Disabled` or `assumeTrue(false` in the stage's owned paths |

The two scans read the stage's owned paths of SYNC_PLAN.md §4 (`bash
scripts/gate-lib.sh owned-paths <stage>` prints them; a path two stages share
is in both lists) and only source files: `.rs`, `.js`, `.mjs`, `.cjs`, `.ts`,
`.html`, `.kt`, `.kts`, `.java`. They skip `target`, `node_modules`, `.git`,
`.gradle` and `build` directories. `src/phone.rs` carries an `#[ignore]` today
(the `codec_bake_off` measurement): it belongs to code `desktop` deletes, so
`desktop`'s and `rc`'s gates fail until it is gone, as the plan intends.

## Conventions the later stages follow

The gates name these targets and scripts before they exist; a stage creates
them under these names (or changes its gate, in its own pull request, §3.3).
Every Rust test runs with `--locked`.

| Stage | What the gate runs |
|---|---|
| `core` | integration test targets `determinism` (run twice, `TZ=Europe/Helsinki` and `TZ=America/New_York`), `rebase`, `transcripts`, `snapshot_fixtures`, `golden` in `taskdeck-core`; purity is `cargo check -p taskdeck-core --target wasm32-unknown-unknown`, `cargo clippy -p taskdeck-core --all-targets -- -D warnings`, and `crates/taskdeck-core/clippy.toml` keeping every entry of `core-clippy-required.txt` (the clock, `chrono::Local`, file, network, thread and environment entries `foundation` put there), with none of those lints allowed off in the crate; budgets are a CI item |
| `seal` | targets `kat`, `tamper`, `contexts`, `pairing_mitm`, `parsers` in `taskdeck-seal`; the line `#![forbid(unsafe_code)]` in `crates/taskdeck-seal/src/lib.rs`; `cargo check -p taskdeck-seal` for the three Android targets; vectors in `docs/sync/vectors/*.json`, well formed per `docs/sync/spec/README.md` §7 (`gate-lib.sh vectors`); the budget is a CI item |
| `engine` | targets `simulator` (with `SIM_SEEDS=1000`) and `regressions` in `taskdeck-sync`; `cargo check -p taskdeck-sync --target wasm32-unknown-unknown` |
| `relay` | `bash relay/test/run.sh core`, `… standby`, `… worker` (the relay stage writes that runner; each run includes the cases of relay.md §21: rows per operation and the 2% day, refusals before the object, replays without a write); `relay/package.json` without `dependencies`, `optionalDependencies`, `peerDependencies` or bundled ones (`gate-lib.sh npm-no-deps`) |
| `android-shell`, `android` | `bash android/ci.sh unit` (Gradle unit tests), `bash android/ci.sh link` (the `armeabi-v7a` link) and `bash android/ci.sh apk-size` (builds the universal APK and fails at 25 MB or more); everything that needs an emulator is a CI item |
| `device` | targets `e2e`, `hostile_relay`, `crash`, `fetchers` of the `taskdeck-device` package, wherever the files are (`crates/taskdeck-device/tests/` or `tests/e2e/` through a `[[test]]` path); the key stores on Windows and macOS, the Worker run and the catch-up budget are CI items |
| `recovery` | targets `restore`, `forged`, `drill`, each in `taskdeck-backup` or `taskdeck-recover` (`cargo test -p taskdeck-backup -p taskdeck-recover --test <name>`) |
| `desktop` | `cargo test --locked --lib` of the root package; targets `headless_flows` and `import_check` of `task_deck`; `gate-lib.sh removed-modules`: the files `src/phone.rs`, `src/phone.html`, `src/phone_sw.js`, `src/sync.rs`, `src/server_main.rs` and `fixtures/generator` are gone, and no tracked file outside `docs/` and Markdown names `taskdeck-server`, `phone.html`, `phone_sw.js`, `tiny_http`, `qrcode` or the fixture generator, and no root-package file names `phone.rs`, `sync.rs`, `mod phone` or `mod sync`; the three-OS UI tests are a CI item |
| `release` | `bash scripts/tests/run.sh` (its verifier scripts' tests) and `bash scripts/release/check-runbooks.sh` (runs each runbook's container steps literally); the dry run and the nightly jobs are CI items |
| `rc` | every suite a container runs (`cargo test --locked --workspace`, the relay's three targets, the Android unit tests and link, the spec lint, the scripts' tests); the whole-system CI run, the attack rounds, the requirements check and the candidate's hashes are CI items or records |
| `foundation` | the workspace sanity, `cargo check --workspace --exclude task_deck --all-targets`, the wasm32 check of core and sync, `scripts/check-android-targets.sh` (plain `cargo check` for the crates without C code, `cargo ndk` for the rest, FAIL without an NDK), the spec lint with a filled requirement map, the fixtures check, the scripts' tests, the release-panic and build-date checks, the supply chain, today's library tests and server build |

Install what a stage's gate needs with `bash scripts/dev-setup.sh <stage>`.

## Other files

- `core-clippy-required.txt`: the entries `crates/taskdeck-core/clippy.toml`
  must keep (`gate-lib.sh clippy-config`).
