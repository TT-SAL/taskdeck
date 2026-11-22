# scripts/ci

The scripts behind `.github/workflows/ci.yml`. The workflow only wires GitHub's
events to these scripts; every decision is here, where it runs and is tested in
a container (`bash scripts/tests/run.sh`, the `test-ci-*.sh` files).

| Script | Job | What it does |
|---|---|---|
| `changes.sh` | `changes` | Decides from the event and the changed paths which areas need CI; writes `root`, `supply`, `foundation`, `core`, `seal`, `engine`, `device`, `recovery`, `relay`, `android` (`true`/`false`), `wide` and `oses` |
| `toolchain.sh` | every job that runs cargo | Installs exactly the toolchain `rust-toolchain.toml` pins (profile, components, targets) by name, never `rustup default` or `rustup update`; fails unless `rustc --version` reports the pinned channel |
| `stage.sh <stage>` | `core`, `seal`, `engine`, `device`, `recovery`, `relay`, `android` | Tests and lints the stage's crates and runs the stage's own `ci.sh` hooks (below) |
| `supply-chain.sh` | `supply-chain` | Installs the pinned cargo-deny and cargo-audit (`scripts/dev-setup.sh supply-chain`), then runs `scripts/supply-chain.sh` once: with `--base` set to the pull request's base or the push's `before` when that commit is in the clone, without it (the cooldown's default base) otherwise |
| `reads.sh` | `changes` | Fails unless a change to whatever the code reads from elsewhere in the tree marks at least the reader's areas (below) |
| `gate.sh` | `gate` | Prints every job's result; fails if `changes` did not succeed or any job failed or was cancelled; skipped passes |

## When jobs run

Pushes run only Linux jobs. Pull requests, and pushes whose head commit message
contains `[ci full]` (in any case: `[CI full]` too, as the old workflow's `contains()` matched it), are *wide*: the desktop matrix and the emulator run, and
the stage jobs `core`, `seal`, `engine`, `device` and `recovery` run on Ubuntu,
Windows and macOS (`oses`). Within that, a job runs only when its area is true.

The changed paths are the pull request's changes against its base (the merge
commit against its first parent), or a push's `before..after`. When that cannot
be worked out (no previous commit, a commit not in the clone, a failed diff, a
checkout that is not a merge commit), or `[ci full]` is asked for, every area
is true.

| Area | Paths |
|---|---|
| every area | `Cargo.lock`, `Cargo.toml` (root), `rust-toolchain.toml`, `.github/workflows/**`, `scripts/ci/**`, `.cargo/**` except `.cargo/audit.toml` (a `.cargo/config.toml` changes every build), `scripts/dev-setup.sh` (it installs the tools of the supply-chain job and of every stage's `ci.sh` hook), a file directly under `crates/` (such as `crates/clippy.toml`: clippy and rustfmt read their config from every parent of a crate), any path no rule here knows, and any path that holds a control character such as a newline (its name could otherwise inject a workflow command into the log) |
| `root` | `src/**`, `build.rs`, `resources.rc`, `icon.ico`, `icon.png`, `noback.png`, `1920px-Blue_Marble_2002.png`, `fonts/**`, `images/**`, `weather_svgs_2/**`; `scripts/check-release-panic.sh` and `crates/panic-probe/**` (the desktop job runs that check on every OS) |
| `supply` | `Cargo.lock`, any `Cargo.toml`, `deny.toml`, `.cargo/audit.toml`, `scripts/supply-chain.sh`, `scripts/cooldown*` |
| `foundation` | `docs/sync/spec/**`, `fixtures/**`, `scripts/**`, `crates/**`, `build.rs` (checked by `scripts/check-build-date.sh`); and whenever `root` is true (the fixture generator, `fixtures/generator`, links the root package, and `scripts/check-fixtures.sh` compares the committed fixtures with what it writes) |
| `core` | `crates/taskdeck-core/**`, `fixtures/**` except `fixtures/generator/**` (the generator's code is `foundation`'s alone) |
| `seal` | `crates/taskdeck-seal/**`, `docs/sync/vectors/**` |
| `engine` | `crates/taskdeck-sync/**` |
| `relay` | `relay/**`, `docs/sync/vectors/**` (the relay's suite takes seal's published vectors) |
| `device` | `crates/taskdeck-device/**`, `tests/e2e/**`; and whenever `core`, `seal`, `engine`, `relay` or `recovery` is true (it links the backup crate, and its end-to-end tests run against the relay) |
| `recovery` | `crates/taskdeck-backup/**`, `crates/taskdeck-recover/**`, `crates/taskdeck-device/**`, `crates/taskdeck-sync/**` (the tool's online steps link the device runtime, which links the engine); and whenever `core` or `seal` is true |
| `android` | `android/**`, `crates/taskdeck-android/**`, `scripts/check-android-targets.sh` (the job's hook runs it); and whenever `device` is true |
| none | `docs/sync/status/**`, `docs/sync/research/**`, `docs/sync/runbooks/**`, `deploy/**` (today's server's install script and unit file), `*.md`, `*.png` and `LICENSE` at the root, `.gitignore` (unless a rule above matched first: `icon.png` is `root`). The area rules ignore case, because a case variant (`ICON.png`) is the same file on Windows and macOS; the no-CI list does not, so `Deploy/x` means every area |

The `root` list is what the root package compiles or embeds (`include_bytes!`,
`include_str!`, `egui::include_image!`, `resources.rc`), plus `images/`, which
the app reads at run time and an ignored measurement test opens. A new file the
root package embeds must be added to `changes.sh` and to this table.

### What the code reads

`reads.sh`, a step of the `changes` job, so it runs on every change, fails the
run when the code reads a file whose change would not run that code's jobs. A
read is a path dependency in any dependencies table of a `Cargo.toml` (also
under `[target.<cfg>]`, and `[patch]`), written with `path` or inherited with
`workspace = true` from the root's `[workspace.dependencies]` (every tracked
file of the crate must mark every area the reader's `src/lib.rs` marks; a dependency on
the top of the tree is one on the root package, whose files are the tracked files
under `src/`, `build.rs` and the root `Cargo.toml`), a target's
source (`path` of `[lib]`, `[[bin]]`, `[[test]]`, `[[example]]`, `[[bench]]`;
`build` of `[package]`; manifests are parsed with python3's `tomllib`, and one
that is not valid TOML, or a path that is not a string, fails), a path in `include!`, `include_bytes!`, `include_str!`,
`include_image!` (any delimiter) or `#[path = ...]` (also inside
`#[cfg_attr(...)]`) in a `.rs` file or in a file a `#[path]` or `include!` pulls
in (`concat!` counts as its literals joined, or, when something else such as a
macro's `$stem` follows them, as those literals; an argument that is not a
string literal, a literal holding a backslash, an escape or a Windows separator,
a `#[path]` in a file with an inline `mod x { ... }`, and a path outside the
tree cannot be checked and fail), and a symlink's
target (one hop). Paths are taken as written, `..` resolved without following
symlinks: a read through a tracked symlink is checked against the link, and the
link against its target. A read must name a file git tracks (for `concat!` with
a `$stem`, a directory, and every tracked file under it is checked), so a read
through a symlinked directory, or of a file that is not there, fails; so does a
read of a path no rule knows (a shared file passes). In particular, **when the
root package gains a path dependency on a workspace crate, that crate's area
must mark `root`** (for the device runtime: a `device` → `root` rule in
`changes.sh`, which core, seal, the engine and recovery then reach through
`device`), or every pull request touching only that crate would skip the
`linux` and `desktop` jobs. Fix a failure in `changes.sh` and this table, never
by moving the read out of sight.

Try it on any list of paths:

```sh
printf 'src/ui.rs\nREADME.md\n' | CI_EVENT=push bash scripts/ci/changes.sh --paths-from -
git diff --name-only origin/day-planner HEAD | CI_EVENT=pull_request bash scripts/ci/changes.sh --paths-from -
```

## The stage hook contract

A later stage adds its own checks without editing `ci.yml`. `scripts/ci/stage.sh
<stage>` (the full contract is at the top of that file):

1. maps the stage to its directories (SYNC_PLAN.md §4): `core` →
   `crates/taskdeck-core`; `seal` → `crates/taskdeck-seal`; `engine` →
   `crates/taskdeck-sync`; `device` → `crates/taskdeck-device`, `tests/e2e`;
   `recovery` → `crates/taskdeck-backup`, `crates/taskdeck-recover`; `relay` →
   `relay`; `android` → `android`, `crates/taskdeck-android`;
2. reports and skips a directory that does not exist yet;
3. for each directory with a `Cargo.toml`, runs `cargo test --locked -p <package>`
   and `cargo clippy --locked -p <package> --all-targets -- -D warnings`;
4. then, for each directory holding a `ci.sh`, runs `bash <dir>/ci.sh` from the
   repository root with `CI_STAGE` (the stage), `CI_OS` (`linux`, `windows` or
   `macos`) and `CI_WIDE` (`true` on wide runs) set. That hook is the stage's: a
   wasm32 build, a budget, `node --test`, Gradle. It installs what it needs
   (`scripts/dev-setup.sh <stage>`) and exits non-zero to fail the job;
5. runs every step even after one fails, prints a summary and exits 1 if any
   failed.

A hook runs on every system of the job's matrix: it checks `CI_OS` itself when
something only works on one of them. The toolchain from `rust-toolchain.toml` is
already installed when it runs.

## Rules for ci.yml

`scripts/tests/test-ci-workflow.sh` checks these on every run:

- `permissions: contents: read`; no secrets; no caches of any kind (a release
  must never read a cache a test job wrote, V1-4);
- only `actions/checkout` and the emulator runner, each pinned by full commit
  SHA with the version in a comment;
- `timeout-minutes` on every job, `fail-fast: false` on every matrix, and the
  `concurrency` group that cancels superseded pull-request runs, never push
  runs (a push run tests only what changed since `before`, so the changes of a
  cancelled push would reach no run; a pull-request run tests the whole pull
  request). Each push run has a group of its own (its `run_id` in the group
  name): GitHub cancels a *pending* run when a newer one joins its group, even
  with `cancel-in-progress` false;
- every `run:` that calls bash sets `shell: bash` (Windows runners); no
  `${{ }}` inside shell text: event data reaches scripts through `env:`;
- the toolchain step before anything that runs cargo;
- `gate` has `if: always()`, needs every other job, one per line, and is named
  `gate` only on `pull_request` events (`gate (push)` otherwise), so a branch
  rule that requires `gate` is satisfied only by the wide pull-request run.

A new job goes into the gate's `needs`; the test fails until it does.
