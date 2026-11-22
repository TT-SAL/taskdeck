# Brief: baseline task 1c, the CI restructure (stage foundation)

SYNC_PLAN.md §6.1 item 1, last bullet: "Filter CI jobs by path at job level, and add an aggregate job named
`gate` that always runs, fails if any job failed or was cancelled, and treats skipped jobs as passed."
§3.2: "Pushes to a pull request run the full CI matrix; other pushes run only the Linux job unless the
commit message contains `[ci full]`." §1.4: the owner may protect `day-planner` so it only accepts pull
requests whose `gate` check passed. SYNC.md §8 "CI": ephemeral runners, no secrets, `contents: read`,
actions pinned by commit SHA. V1-4: no caches (a release must never be able to read a cache a test job wrote).

Paths you may create or change: `.github/workflows/ci.yml`, `scripts/ci/**`, `scripts/tests/**` (add tests;
do not weaken existing ones). Nothing else. Read the current `.github/workflows/ci.yml` first and keep its
header comment's intent, its pinned action SHAs and its emulator job's content.

## The design (decided by the lead; implement it exactly, and report anything that cannot work)

### Jobs
1. `changes` (ubuntu, always, a few seconds): checks out with full history and runs
   `scripts/ci/changes.sh`, which decides from the event and the changed paths which areas need CI and
   writes job outputs: one `true`/`false` per area, `wide` (`true` on `pull_request` events and on pushes
   whose head commit message contains `[ci full]`), and `oses` (a JSON array: `["ubuntu-latest"]`, or
   `["ubuntu-latest","windows-latest","macos-latest"]` when wide).
2. `linux`: today's "Linux · headless tests" of the root package; runs when area `root` is true.
3. `desktop`: today's three-OS matrix of the root package; runs when `root` and `wide` are true. Add one
   step to it on every OS: `bash scripts/check-release-panic.sh`.
4. `emulator`: today's emulator matrix (API 26 and 36), unchanged inside; runs when `android` and `wide`.
5. `supply-chain` (ubuntu): `bash scripts/dev-setup.sh foundation`, then `bash scripts/supply-chain.sh`
   with the cooldown's base set to the pull request's base commit, or the push's `before` commit; runs when
   area `supply` is true. (Those scripts are written by another task in this same workflow run; read them
   when they exist, and if they do not exist yet, write the job against their documented interface:
   `scripts/dev-setup.sh <stage>`, `scripts/supply-chain.sh` reading the base from `--base <rev>` passed
   through to `scripts/cooldown.sh`, and say so in your report.)
6. `foundation` (ubuntu): runs when area `foundation` is true. Steps: `bash scripts/tests/run.sh` (the
   scripts' own tests), `bash scripts/check-release-panic.sh`, `bash scripts/check-build-date.sh`. Later
   tasks of this stage add spec lint, the fixture check and the Android and wasm32 skeleton checks to this
   job; leave a clearly marked place for them.
7. One job per later stage's code, so those stages rarely need to edit this shared file:
   `core`, `seal`, `engine`, `device`, `recovery` (matrix over `oses`), `relay` and `android` (ubuntu only).
   Each runs when its area is true and does: checkout, toolchain, `bash scripts/ci/stage.sh <stage>`.
   `scripts/ci/stage.sh <stage>` maps the stage to its paths (SYNC_PLAN.md §4):
   core → `crates/taskdeck-core`; seal → `crates/taskdeck-seal`; engine → `crates/taskdeck-sync`;
   device → `crates/taskdeck-device` and `tests/e2e`; recovery → `crates/taskdeck-backup` and
   `crates/taskdeck-recover`; relay → `relay`; android → `android` and `crates/taskdeck-android`.
   For each Rust crate directory that exists: `cargo test --locked -p <package>` and
   `cargo clippy --locked -p <package> --all-targets -- -D warnings`. Then, for each of the stage's
   directories that contains an executable-by-bash file `ci.sh`, run `bash <dir>/ci.sh` with the
   environment variables `CI_STAGE`, `CI_OS` (`linux`, `windows`, `macos`) and `CI_WIDE` set: that hook is
   owned by the stage and is where it adds its own checks (wasm32 builds, budgets, `node --test`, Gradle)
   without touching `ci.yml`. A stage directory that does not exist yet is reported and skipped, not an
   error (the crates arrive in a later task of this stage). Document this contract at the top of
   `scripts/ci/stage.sh` and in a short `scripts/ci/README.md`.
8. `gate`: `if: always()`, `needs:` every other job (one per line). It fails if any needed job's result is
   `failure` or `cancelled`, or if `changes` is anything but `success`; `skipped` counts as passed. Its
   display name is exactly `gate` on `pull_request` events and `gate (push)` on `push` events, so that a
   branch-protection rule requiring `gate` can only be satisfied by the pull-request run (the wide one),
   never by the narrow push run on the same commit. Print a table of every job and its result.

### Areas and paths (`scripts/ci/changes.sh`)
- Changed paths: on `pull_request`, the pull request's changes against its base (the default checkout is
  the merge commit: `git diff --name-only HEAD^1 HEAD`; require that HEAD has two parents and fall back to
  "everything" if not). On `push`, `git diff --name-only <before> <after>`; if `before` is all zeros, does
  not exist in the clone, or the diff fails for any reason, fall back to "everything". A head commit
  message containing `[ci full]` means "everything" and wide.
- Never interpolate event data into shell text: pass the event name, SHAs and the commit message to the
  script through `env:` and read them as variables.
- "Everything" sets every area true.
- Shared files set every area true: `Cargo.lock`, the root `Cargo.toml`, `rust-toolchain.toml`,
  `.github/workflows/**`, `scripts/ci/**`.
- `root`: `src/**`, `build.rs`, `resources.rc`, `icon.ico`, `icon.png`, `noback.png`, `fonts/**`,
  `images/**`, `weather_svgs_2/**` (check which of these the root package really embeds with
  `include_bytes!`/`include_str!`/`include_image!` and keep the list truthful).
- `supply`: `Cargo.lock`, any `Cargo.toml`, `deny.toml`, `.cargo/**`, `scripts/dev-setup.sh`,
  `scripts/supply-chain.sh`, `scripts/cooldown*`.
- `foundation`: `docs/sync/spec/**`, `fixtures/**`, `scripts/**`, `crates/**`.
- `core`: `crates/taskdeck-core/**`, `fixtures/**`. `seal`: `crates/taskdeck-seal/**`,
  `docs/sync/vectors/**`. `engine`: `crates/taskdeck-sync/**`. `relay`: `relay/**`.
  `device`: `crates/taskdeck-device/**`, `tests/e2e/**`, and whenever core, seal or engine is true.
  `recovery`: `crates/taskdeck-backup/**`, `crates/taskdeck-recover/**`, and whenever core or seal is true.
  `android`: `android/**`, `crates/taskdeck-android/**`, and whenever device is true.
- Paths that need no CI: `docs/sync/status/**`, `docs/sync/research/**`, `docs/sync/runbooks/**`, `*.md`
  and `LICENSE` at the repository root, `*.png` at the root, `.gitignore`.
- Any changed path that matches none of the rules above sets every area true (fail safe).
- The script must be testable without GitHub: it takes the list of changed paths from a file or standard
  input in a test mode and prints the outputs. Add tests to `scripts/tests/` covering every rule above,
  the fall-backs, `[ci full]`, a docs-only change (nothing but `changes` and `gate` run), and one path per
  area. `bash scripts/tests/run.sh` runs them (if that runner does not exist yet, create it so that it
  runs every `scripts/tests/test-*.sh` and fails if any fails; another task adds its own test files next
  to yours, so do not hard-code file names).

### Toolchain step (`scripts/ci/toolchain.sh`, used by every job that runs cargo)
`rust-toolchain.toml` pins the toolchain. The step must make rustup install exactly that toolchain with
its components and targets on Linux, Windows and macOS runners, whatever rustup version they carry
(rustup 1.28 and later changed when an override is installed automatically; do not rely on it), then print
`rustc --version` and `cargo --version` and FAIL unless `rustc --version` reports the channel written in
`rust-toolchain.toml`. Do not run `rustup default` or `rustup update stable`. Every `run:` that calls a
bash script sets `shell: bash` so it works on Windows runners.

### Rules
- Keep: `permissions: contents: read`; no secrets; no caches of any kind; actions pinned by full commit
  SHA with the version in a comment; `timeout-minutes` on every job; the `concurrency` group that cancels
  superseded runs; `fail-fast: false` on matrices.
- Use no new third-party action. `actions/checkout` and the emulator runner already in the file are the
  only ones allowed.
- Everything that can be a script is a script under `scripts/ci/`, so it can be run and tested in a
  container; the YAML only wires events to scripts.
- You cannot run GitHub Actions here. Validate what you can: parse the YAML (python3 with PyYAML if
  present, or a careful review otherwise), run every script under `bash -n` and for real where possible,
  run `scripts/ci/changes.sh` in its test mode against the real diff of this branch, and simulate
  `scripts/ci/stage.sh` for each stage in this checkout. The lead will read the real CI run.
- Report precisely what you could not verify locally.

## State after the implementation (2026-11-22, second session)

Implemented and committed: `.github/workflows/ci.yml`, `scripts/ci/` (`changes.sh`, `toolchain.sh`, `stage.sh`,
`gate.sh`, `supply-chain.sh`, `README.md`) and six test files `scripts/tests/test-ci-*.sh`.
`bash scripts/tests/run.sh` passes. Owned paths for the attack loop: those files.

# Lead rulings for task baseline-1c (2026-11-22)

1. **`scripts/supply-chain.sh` gains `--base <rev>`, passed through to `scripts/cooldown.sh`** (a change to the
   supply-chain task's script, made with that task's rulings). `scripts/ci/supply-chain.sh` stays as the place
   that works out the base from the event, and then makes one call instead of running the cooldown twice.
2. **`deploy/**` needs no CI.** It holds the install script and the unit file of today's server; no job reads it.
3. **`.cargo/audit.toml` belongs to the area `supply`; any other path under `.cargo/` is a shared file** (every
   area): a `.cargo/config.toml` would change every build.
4. **A run cancelled by the concurrency group makes `gate` fail for that run.** That is the plan's wording
   ("fails if any job failed or was cancelled") and it stays; the superseding run is the one that counts.
5. **Real runs are the evidence.** What GitHub actually did with this workflow is written down by the lead in
   the session scratchpad (`ci-evidence/1c.md`) and, in short, in the status file. A finding that contradicts
   an observed run needs its own observation.

## Already raised and handled (do not re-report)

- [high] `scripts/check-build-date.sh` failed under the workflow's `CARGO_TERM_COLOR: always`, so the Foundation job and `gate` failed on GitHub (the lead's finding from the first real run; fixed: the script switches cargo's colour off for its own calls, and `scripts/tests/test-check-build-date.sh` runs it with colour on)
- [medium] A change to `build.rs` did not run the foundation job, which holds the one check that guards it (fixed: `build.rs` marks `root` and `foundation`)
- [medium] Changes to `scripts/check-release-panic.sh` or `crates/panic-probe/**` skipped the desktop matrix, where that check runs on Windows and macOS (fixed: they also mark `root`)
- [medium] The area dependencies left out relay to device, vectors to relay and recovery to device (ruled by the lead: ruling 6)

6. **More area dependencies (lead ruling, 2026-11-22).** `relay` true makes `device` true (the device's
   end-to-end tests run against the standby relay), and so `android`; `docs/sync/vectors/**` also marks `relay`
   (the relay's suite takes seal's published vectors); `recovery` true makes `device` true (the device runtime
   links the backup crate). Each only makes more jobs run. The lead confirms the two additions of round 1:
   `build.rs` marks `root` and `foundation`; `scripts/check-release-panic.sh` and `crates/panic-probe/**` also
   mark `root`.
- [medium] A changed file name holding a line break could inject a workflow command through the path filter's log and make jobs skip (fixed: everything the script logs has control characters replaced, and a path holding one means "everything")
- [medium] No area marked `root` for a crate the desktop app will link, and [low] a file under a no-CI path that the build reads (an include, a symlink) could change with every job skipped (fixed by `scripts/ci/reads.sh`, run by the `changes` job: a path dependency, an include or a symlink target must mark every area its reader marks)

7. **The area rules are a checked contract (lead ruling, 2026-11-22).** `scripts/ci/reads.sh` stays: when a
   later stage adds a path dependency, an include or a symlink that the rules do not cover, the run fails until
   that stage adds the rule to `scripts/ci/changes.sh`, in a small pull request of its own (SYNC_PLAN.md §3.2).
   No `device` to `root` rule is added now: the check will ask for it when the root package links the device
   crate.

8. **Observed (lead, 2026-11-22 06:06 UTC).** The pull-request run on `fee727d` was green in every job: `gate`
   passed on the pull-request run and `gate (push)` on the push run; the Foundation job passed with the colour
   fix; the Supply chain job passed with the lockfile rulings; the desktop job built and tested the moved
   crates on Windows, macOS and Linux.
- [medium] `scripts/ci/reads.sh` missed a crate linked through `[workspace.dependencies]` and a target's `path` (fixed: inherited dependencies and target and build-script paths are checked like written paths)
- [medium] A push run cancelled by a newer push left its own changes untested, because a push run only looks at what changed since `before` (fixed: only pull-request runs cancel their predecessors; push runs all run)

9. **Push runs are not cancelled (lead ruling, 2026-11-22; amends ruling 4).** A pull-request run tests the
   whole pull request, so a newer one may cancel an older one, and the cancelled run's `gate` fails for that
   run. A push run tests only its own `before` to `after`, so it is never cancelled.

10. **What `scripts/ci/reads.sh` reads is a closed list (lead ruling, 2026-11-22).** It checks: a path
    dependency in a manifest's dependency tables, written inline or as a table, or inherited from
    `[workspace.dependencies]`; a target's `path` and a package's `build`; the literal path in `include!`,
    `include_bytes!`, `include_str!`, `include_image!` and `#[path]`, and the first literal of `concat!`; a
    tracked symlink's target. Round 4 found one more form, and a parser written in awk can always be met with
    one more: from here on a finding against this check counts only if one of the listed forms, written the
    way cargo or rustc accepts it in this repository's files, gets past it. Reads that code makes at run time
    (opening a file by name, a build script reading the tree) and forms not on the list are left to review.
- [medium] Push runs still lost changes: GitHub cancels a pending run of a concurrency group when a newer one is queued, whatever `cancel-in-progress` says (fixed: each push run has a group of its own)
- [medium, medium, low] `scripts/ci/reads.sh` missed a single-quoted path, an include written with `[]` or `{}` or a `#[path]` inside `cfg_attr`, and `workspace = true` after a multi-line array (fixed, each with a test)

11. **`scripts/ci/reads.sh` reads manifests with a real TOML parser (lead ruling, 2026-11-22).** Three rounds
    in a row found one more spelling that the script's own awk reader of `Cargo.toml` did not understand.
    That reader goes: the manifest part uses python3's `tomllib` (as `scripts/ci/gate.sh` and
    `scripts/cooldown.py` already do; the `changes` job runs on Ubuntu, which has python3 3.11 or later), so
    every spelling TOML allows is read the same way, and a manifest that does not parse fails the check. The
    Rust-source part stays a search over the whitespace-squeezed text for the listed macros and attributes
    with any of the three delimiters; ruling 10's closed list stands for it. After this change a finding of
    the form "another TOML spelling" cannot arise, and "another Rust spelling of a listed form" counts only
    for `include!`, `include_bytes!`, `include_str!`, `include_image!`, `concat!` and `path =` in an attribute.

12. **The Rust-source part of `scripts/ci/reads.sh` fails closed, and that ends the list of spellings (lead
    ruling, 2026-11-22 08:25 UTC).** The second loop's rounds 1, 2 and 4 each found one more way to write a
    read that the search did not follow (an escape in a literal, a second literal of `concat!`, a module in
    a file not named `*.rs`, a `#[path]` inside an inline module). The check guards against honest mistakes:
    a file this repository's code embeds or compiles, kept where the path rules say no CI is needed. It is
    not a defence against a contributor who hides a read on purpose, and no search of sources can be one
    (a build script, a procedural macro or a test may read any file); such a change runs CI and is reviewed
    when it is made. So:
    - a read (`include!`, `include_bytes!`, `include_str!`, `include_image!`, `#[path]`) that the check
      cannot work out as written fails the check and is named ("cannot be checked: write a plain literal");
    - a read whose path does not name a tracked file of the repository fails the check; a path that leaves
      the tree, or that no rule knows, never passes by "marking every area";
    - a `#[path]` or `include!` that names a file the check does not scan fails the check, or that file is
      scanned too; a `#[path]` inside an inline module fails the check;
    - one form cannot be resolved to a single file and still passes, because today's `src/weather.rs` uses
      it and `src/` is not this stage's to change: `concat!` whose leading string literals are followed by
      something that is not a literal (a macro variable). Its joined leading literals must name a tracked
      directory (or a prefix inside one), and every tracked file under that directory must mark at least
      the reader's areas; otherwise the check fails;
    - the fixers build this in the simplest way and delete special cases it makes unnecessary.

    From here on a finding against `reads.sh` counts only in two cases: a read written the ordinary way
    (one string literal, or `concat!` of string literals, in a tracked `*.rs` file) names a tracked file
    that marks fewer areas than its reader, and the check passes; or the check passes a read it could not
    resolve to a tracked file. Any other spelling is either refused by the rule above or left to review.

13. **The hand-written readers of TOML in `scripts/ci/` read the plain spelling and fail on any other (lead
    ruling, 2026-11-22 09:00 UTC).** `stage.sh` reads a package's name; `toolchain.sh` reads the channel, the
    profile, the components and the targets. Both run on Windows and macOS runners, where no TOML parser can
    be relied on, so they stay small bash readers. After the second loop each of them either reads a value
    correctly or fails with a message that names the file and the key. From here on a finding against
    these readers counts only if a script goes on, without failing, with a value it read wrongly or did not
    read. A spelling that makes the script fail loudly is not a finding.

Second loop (2026-11-22, six rounds, every finding fixed in the round that raised it; do not re-report):

- [second loop, round 1, medium] scripts/ci/reads.sh takes an include!/#[path] string literal verbatim, so an escape sequence or a Windows backslash path gets past the check (fixed, with a test)
- [second loop, round 2, medium] A change to scripts/dev-setup.sh never runs the stage jobs whose ci.sh hooks stage.sh tells to call it (fixed, with a test)
- [second loop, round 2, low] A case variant of an embedded root image is classified 'no CI' but replaces the embedded file on Windows and macOS checkouts (fixed, with a test)
- [second loop, round 2, low] reads.sh passes without checking anything when its tree is not a git work tree (fixed, with a test)
- [second loop, round 3, medium] A clippy.toml directly under crates/ changes every stage crate's `cargo clippy -D warnings` result, but marks only the foundation area, so no stage job runs (fixed, with a test)
- [second loop, round 3, low] stage.sh rejects a valid single-quoted package name (`name = 'x'`) as 'has no [package] name' (fixed, with a test)
- [second loop, round 3, low] `[ci full]` is now matched case-sensitively; today's workflow matched it in any case (fixed, with a test)
- [second loop, round 4, medium] reads.sh checks only the first literal of concat!, so concat!("", "../docs/sync/status/x.txt") or concat!("../", "docs/sync/status/x.txt") gets past it (fixed, with a test)
- [second loop, round 4, low] reads.sh misses listed reads that rustc compiles from a non-.rs module file, and resolves #[path] inside an inline module from the wrong directory (fixed, with a test)
- [second loop, round 5, medium] reads.sh passes an include that goes through a tracked symlink under a no-CI path, so the embedded content can change with only changes and gate running (fixed, with a test)
- [second loop, round 5, medium] reads.sh passes a read that does not name a tracked file, and a read of a path no rule knows passes by 'marking every area', contrary to ruling 12 (fixed, with a test)
- [second loop, round 6, low] scripts/ci/toolchain.sh reports success when `rustup toolchain install` and `rustup component add` both failed, because set -e is off inside the `( ... ) || die` subshell (fixed, with a test)
- [second loop, round 6, low] scripts/ci/toolchain.sh silently drops single-quoted (TOML literal string) components and targets from rust-toolchain.toml and still passes (fixed, with a test)

14. **A path dependency on the root package, and what follows for the areas (lead ruling, 2026-11-22
    10:35 UTC).** `fixtures/generator` depends on the root package by path, because the fixtures are what
    today's code writes. Three rules:
    - `fixtures/generator/**` marks `foundation` only. The rest of `fixtures/**` marks what it marked
      before (its data is read by later stages' tests; the generator's code is not).
    - Whatever marks the area `root` also marks `foundation`: a change to today's code changes what the
      generator writes, and `scripts/check-fixtures.sh` (run by `scripts/tests/run.sh` in the foundation
      job) compares the committed fixtures with it.
    - In `scripts/ci/reads.sh`, a path dependency whose directory is the top of the tree is a dependency on
      the root package, and its files are the tracked files under `src/` plus `build.rs` and the root
      `Cargo.toml`, not every tracked file of the repository.

15. **The last CI wiring (lead ruling, 2026-11-22; changes the design text above where they differ).** The
    supply-chain job has no step of its own for tools: `scripts/ci/supply-chain.sh` installs the row
    `supply-chain`. The foundation job runs `scripts/spec-lint.sh`. The android job runs on the three systems
    on wide runs like the other stage jobs, so that `taskdeck-android` compiles on Windows and macOS; its
    hooks decide by `CI_OS` what they do. `SYNC.md` and `SYNC_PLAN.md` mark the foundation area, because
    spec-lint reads both; `scripts/wrangler/**` is a shared path like `scripts/dev-setup.sh`.

Third loop (2026-11-22, three rounds, converged): round 1 confirmed the lead's three findings above and found
the two path-rule gaps of ruling 15 (all fixed); round 2 found nothing; round 3 found one low (stage.sh reads
a `name = "..."` line inside a multi-line string of `[package]` as the package name), left as it is.
