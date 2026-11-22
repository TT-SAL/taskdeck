# Brief: task 6, tooling (stage foundation)

SYNC_PLAN.md §6.1 item 6: "`scripts/gate.sh`, `scripts/spec-coverage.sh`, `scripts/dev-setup.sh <stage>`
(installs what a stage needs in a fresh container: Android SDK and NDK, `cargo-ndk`, `cargo-deny`,
`wrangler`)". §6: "Gate means two things: `scripts/gate.sh <stage>` exits 0 for everything a container can
run, and the stage's CI jobs are green for the rest. The script prints the items only CI can check."
§3.3: "Every MUST clause has an id and a test. `scripts/spec-coverage.sh <stage>` fails if a clause owned
by the stage appears in no test." "Never weaken a gate to pass it. No skipped, ignored or loosened tests,
no `unimplemented!()` left in owned paths." §3.4: "Install tools from their official sources at pinned
versions, with checksums where published."

Paths you may create or change: `scripts/**` (extend `scripts/dev-setup.sh`; do not weaken existing
scripts or tests). Nothing else. Read `docs/sync/spec/README.md` §4 (clause format and how tests cite
clauses), `scripts/ci/README.md` and the existing scripts first, and keep their style.

## 1. `scripts/spec-lint.sh`
Checks the six spec files and the README, fails with one line per problem:
- every clause line matches the README's regular expression; ids are unique across all specs; each id's
  prefix matches its file; every owner is an allowed stage name or the single word `withdrawn`;
- no line shaped like a clause sits inside a code block or a table;
- every uppercase MUST or MUST NOT outside code blocks stands inside a clause (a clause line or its
  indented continuation); report offenders by file and line;
- each spec has the closing sections the README names, in order, with the exact titles;
- every error code in an "Error catalogue" table has the form `<spec>/<name>`; no code appears twice;
- every clause id mentioned anywhere in the specs or the README's requirement map exists (dangling
  references are errors), and withdrawn ids are not referenced as live;
- the requirement map (between the two marker comments in `docs/sync/spec/README.md`) is a well-formed
  table in which every row maps a SYNC.md section reference to at least one of: an existing clause id, a
  stage name of SYNC_PLAN.md, or a "plan §2" row reference; an empty map is an error unless
  `--allow-empty-map` is given (the map is filled by another task).
Options: `--summary` prints clause counts per spec and per owner stage.

## 2. `scripts/spec-coverage.sh <stage>`
For every live clause whose owner list contains `<stage>`, require a citation in at least one test file of
that stage: the marker `spec: <ID>` or the id inside a test name (README §4.3), matched as a whole id (an
id that is a prefix of another must not match it). Test files per stage (this script is the source of
truth; keep the table at the top of the script and print it with `--paths`):
- core: `crates/taskdeck-core/tests/**`, `crates/taskdeck-core/fuzz/fuzz_targets/**`, and under
  `crates/taskdeck-core/src/**` only files named `tests.rs` or ending in `_tests.rs`;
- seal: the same pattern under `crates/taskdeck-seal/`; engine: under `crates/taskdeck-sync/`;
- device: under `crates/taskdeck-device/`, plus `tests/e2e/**`;
- recovery: under `crates/taskdeck-backup/` and `crates/taskdeck-recover/`;
- relay: `relay/test/**`;
- android-shell and android: `android/**/src/test/**`, `android/**/src/androidTest/**`,
  `android/page/test/**`, and the Rust pattern under `crates/taskdeck-android/`;
- desktop: `src/**` and `tests/**` except `tests/e2e/**`;
- release: `scripts/release/**`, `scripts/tests/**`; rc: every path above.
Output: the uncovered clause ids, one per line with file and line of the clause; exit non-zero if any.
`--list` prints every clause of the stage with the test files that cite it. A citation of an id that
does not exist, or of a withdrawn clause, is an error too (a test that cites nothing real proves nothing).
During foundation no stage has tests yet, so the script fails for every stage that owns clauses: that is
correct. Do not add a way to make it pass without tests.

## 3. `scripts/gate.sh <stage>`
One gate per stage of SYNC_PLAN.md §6 (foundation, core, seal, relay, engine, android-shell, device,
recovery, desktop, android, release, rc). Keep each gate as data in `scripts/gates/<stage>.gate`, one item
per line, so a stage lead can read exactly what is expected and a change to a gate is a visible diff:
`run | <id> | <what the plan asks, in a few words, with its § reference> | <command run from the repository root>`
`ci  | <id> | <what the plan asks> | <the CI job that checks it>`
`gate.sh` prints every item with PASS, FAIL or CI-ONLY, runs all `run` items even after a failure, and
exits 0 only if every `run` item passed. `--list` prints the items without running; `--only <id>` runs one.
An unknown stage, an empty gate file or a malformed line is an error.

Items every stage's gate has (after its own items), from §3.3:
- `spec-coverage`: `bash scripts/spec-coverage.sh <stage>`;
- `no-unimplemented`: no `unimplemented!(`, `todo!(` or `unreachable!("TODO` in the stage's owned paths
  (SYNC_PLAN.md §4), tests included. Not in foundation's gate: skeleton bodies may be `unimplemented!()`
  in foundation "and nowhere later";
- `no-skipped-tests`: no `#[ignore`, `it.skip`, `test.skip`, `describe.skip`, `xit(`, `@Ignore`,
  `@Disabled`, `assumeTrue(false` in the stage's owned paths. Today's `src/phone.rs` carries one
  `#[ignore]` (the `codec_bake_off` measurement); it belongs to code the `desktop` stage deletes, so do
  not special-case it: `desktop`'s gate fails until that code is gone, as the plan intends.
Implement the two scans in `scripts/gate-lib.sh` (or similar) with tests.

Stage items: translate each gate list of SYNC_PLAN.md §6.1–§6.12 faithfully, one item per bullet or
clause of the plan's text, quoting the plan in the description. Use these conventions for commands, and
document them in `scripts/gates/README.md` (the later stages must name their test targets accordingly;
a stage that finds its gate wrong changes it in a pull request of its own and says why, §3.3):
- Rust integration test targets are named for the gate item, run with `--locked`:
  core: `determinism` (run twice: `TZ=Europe/Helsinki` and `TZ=America/New_York`), `rebase`,
  `transcripts`, `snapshot_fixtures`, `golden`; purity is `cargo check -p taskdeck-core --target
  wasm32-unknown-unknown` plus `cargo clippy -p taskdeck-core --all-targets -- -D warnings` plus a check
  that `crates/taskdeck-core/clippy.toml` still disallows the clock, `chrono::Local`, file, network and
  thread entries foundation put there; budgets are CI items ("a dedicated CI job, median of five").
  seal: `kat`, `tamper`, `contexts`, `pairing_mitm`, `parsers`; `#![forbid(unsafe_code)]` present in
  `crates/taskdeck-seal/src/lib.rs`; `cargo check -p taskdeck-seal` for the three Android targets;
  vectors present under `docs/sync/vectors/` and well-formed per README §7; the budget is a CI item.
  engine: `simulator` with `SIM_SEEDS=1000`, `regressions`, and `cargo check -p taskdeck-sync --target
  wasm32-unknown-unknown`.
  device: `e2e`, `hostile_relay`, `crash`, `fetchers` in `crates/taskdeck-device` or `tests/e2e`; the
  key-store tests on Windows and macOS and the Worker run are CI items; the catch-up budget is a CI item.
  recovery: `restore`, `forged`, `drill` across `taskdeck-backup` and `taskdeck-recover`.
  desktop: `cargo test --locked --lib` for the root package, `headless_flows`, `import_check`, and a scan
  proving no reference remains to the removed modules (the `taskdeck-server` binary, `phone.rs`,
  `phone.html`, `phone_sw.js`, `sync.rs`, `tiny_http`, `qrcode`, the fixture generator); the three-OS UI
  tests are a CI item.
- relay: `bash relay/test/run.sh core`, `… standby`, `… worker` (the relay stage writes that runner), and
  a check that `relay/package.json` has no `dependencies`.
- android-shell and android: Gradle unit tests and the `armeabi-v7a` link through `bash android/ci.sh
  unit` and `bash android/ci.sh link`; everything that needs an emulator is a CI item.
- release and rc: mostly CI items; `run` items for what a container can do (the plan says which).
- foundation: `cargo metadata --locked` sanity; `cargo check --locked --workspace --exclude task_deck
  --all-targets`; the wasm32 check of core and sync; the Android-target checks (`cargo check` for the
  three targets for the crates without C dependencies; `cargo ndk` for the rest when an NDK is installed,
  otherwise FAIL with the dev-setup command to run); `bash scripts/spec-lint.sh`;
  `bash scripts/check-fixtures.sh`; `bash scripts/tests/run.sh`; `bash scripts/check-release-panic.sh`;
  `bash scripts/check-build-date.sh`; `bash scripts/supply-chain.sh`; today's
  `cargo test --locked --no-default-features --lib` and the server build; CI items: "every CI job green,
  including the desktop matrix", "skeletons compile on Windows and macOS".
Where the plan's wording leaves a choice, take the stricter reading and say so in the item's description.

## 4. `scripts/dev-setup.sh <stage>`
Extend the existing script (it installs cargo-deny and cargo-audit for foundation). Add, each pinned and
verified, idempotent, with `--check`:
- the Rust toolchain of `rust-toolchain.toml` with its components and targets (through rustup);
- for android-shell and android: the Android command-line tools, the SDK platform and build-tools the
  design names (SYNC.md §8: compileSdk 37, build-tools to match), NDK r28 (16 KB pages), installed under
  a directory the script prints and exports (`ANDROID_HOME`, `ANDROID_NDK_HOME`), with the SHA-256 of
  every archive written in the script (Google publishes checksums for the command-line tools and the NDK;
  `sdkmanager` verifies what it downloads itself: say which is which); `cargo-ndk` 4.1.2 by
  `cargo install --locked --version` (or a verified binary);
- for relay, device, rc: `wrangler` 4.166.0 (the version SYNC_PLAN.md §5 checked), installed into a
  directory of its own with an exact version and a lockfile, never globally and never on anything but a
  cloud container or CI (the script refuses unless it sees it runs in one or `--i-am-in-a-container` is
  given); say how its integrity is established (npm's lockfile integrity hashes);
- a table at the top: stage → tools. `all` installs everything.
Actually run the Android and wrangler installs in this container once, to prove the script works, and
report sizes and times. If a download host is unreachable, report it; do not invent a mirror.

## 5. Tests
`scripts/tests/test-*.sh` for spec-lint and spec-coverage (small made-up spec trees in a temporary
directory: a duplicate id, a bad owner, a clause in a code block, a MUST outside a clause, a dangling
reference, a prefix-collision id, a citation of a withdrawn clause, a covered and an uncovered clause),
for the gate runner (a made-up gate file with a passing, a failing and a CI item; malformed lines), and
for the two scans. All run by `bash scripts/tests/run.sh`.

Report: every gate item per stage (the table), every place you had to interpret the plan, what you ran.

## State after the implementation (2026-11-22)

Built: `spec-lint.sh`, `spec-coverage.sh`, `gate.sh` with one gate file per stage, the gate scans,
`check-android-targets.sh`, and `dev-setup.sh` for every stage (the Android SDK and NDK, cargo-ndk and
wrangler were installed once in the container: 3.0 GB). The foundation gate passed 13 of its 14 run items
in the implementer's run; spec-lint fails on two real defects of core.md and on the empty requirement map.

## Lead rulings for task tooling

1. **wrangler stays at 4.166.0**, the version SYNC_PLAN.md names, although it and its pinned dependencies
   were younger than seven days when pinned. It runs only in cloud containers and CI, from a lockfile with
   integrity hashes and without install scripts. Stage `relay` may move the pin.
2. **`dev-setup.sh` accepts the Android SDK licences** for its own SDK directory and says so when it does;
   the lead tells the owner.
3. **The map rules of spec-lint stand** (rows in SYNC.md's order, a row for every section of §3 to §9 that
   has no subsections, a `plan §2:` topic equal to the first cell of a row of that table). The requirement
   map is written to meet them.
4. **Two rows of their own in the stage table of `dev-setup.sh`**, so that CI installs only what a job
   needs: `supply-chain` (cargo-deny, cargo-audit), used by `scripts/ci/supply-chain.sh`; and
   `android-targets` (the Rust toolchain and cargo-ndk, and the NDK only when neither `ANDROID_NDK_HOME`
   nor `ANDROID_NDK_LATEST_HOME` names a usable one), used by `crates/taskdeck-android/ci.sh`, the hook of
   CI's android job, before it runs `scripts/check-android-targets.sh`. GitHub's Ubuntu runners carry an
   NDK; a container does not.
5. **spec-lint joins the foundation CI job last**, once the requirement map is filled and core.md's two
   defects are fixed (the lead makes that change with the exit).

## Already raised and handled (do not re-report)

(none yet)
