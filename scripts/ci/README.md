# scripts/ci

The scripts behind `.github/workflows/ci.yml`. The workflow only wires GitHub's
events to these scripts; every decision is here, where it runs and is tested in
a container (`bash scripts/tests/run.sh`, the `test-ci-*.sh` files).

| Script | Job | What it does |
|---|---|---|
| `changes.sh` | `changes` | Decides from the event and the changed paths which areas need CI; writes `root`, `supply`, `foundation`, `core`, `seal`, `engine`, `device`, `recovery`, `relay`, `android` (`true`/`false`), `wide` and `oses` |
| `toolchain.sh` | every job that runs cargo | Installs exactly the toolchain `rust-toolchain.toml` pins (profile, components, targets) by name, never `rustup default` or `rustup update`; fails unless `rustc --version` reports the pinned channel |
| `stage.sh <stage>` | `core`, `seal`, `engine`, `device`, `recovery`, `relay`, `android` | Tests and lints the stage's crates and runs the stage's own `ci.sh` hooks (below) |
| `supply-chain.sh` | `supply-chain` | Runs `scripts/supply-chain.sh` once: with `--base` set to the pull request's base or the push's `before` when that commit is in the clone, without it (the cooldown's default base) otherwise |
| `gate.sh` | `gate` | Prints every job's result; fails if `changes` did not succeed or any job failed or was cancelled; skipped passes |

## When jobs run

Pushes run only Linux jobs. Pull requests, and pushes whose head commit message
contains `[ci full]`, are *wide*: the desktop matrix and the emulator run, and
the stage jobs `core`, `seal`, `engine`, `device` and `recovery` run on Ubuntu,
Windows and macOS (`oses`). Within that, a job runs only when its area is true.

The changed paths are the pull request's changes against its base (the merge
commit against its first parent), or a push's `before..after`. When that cannot
be worked out (no previous commit, a commit not in the clone, a failed diff, a
checkout that is not a merge commit), or `[ci full]` is asked for, every area
is true.

| Area | Paths |
|---|---|
| every area | `Cargo.lock`, `Cargo.toml` (root), `rust-toolchain.toml`, `.github/workflows/**`, `scripts/ci/**`, `.cargo/**` except `.cargo/audit.toml` (a `.cargo/config.toml` changes every build), and any path no rule here knows |
| `root` | `src/**`, `build.rs`, `resources.rc`, `icon.ico`, `icon.png`, `noback.png`, `1920px-Blue_Marble_2002.png`, `fonts/**`, `images/**`, `weather_svgs_2/**`; `scripts/check-release-panic.sh` and `crates/panic-probe/**` (the desktop job runs that check on every OS) |
| `supply` | `Cargo.lock`, any `Cargo.toml`, `deny.toml`, `.cargo/audit.toml`, `scripts/dev-setup.sh`, `scripts/supply-chain.sh`, `scripts/cooldown*` |
| `foundation` | `docs/sync/spec/**`, `fixtures/**`, `scripts/**`, `crates/**`, `build.rs` (checked by `scripts/check-build-date.sh`) |
| `core` | `crates/taskdeck-core/**`, `fixtures/**` |
| `seal` | `crates/taskdeck-seal/**`, `docs/sync/vectors/**` |
| `engine` | `crates/taskdeck-sync/**` |
| `relay` | `relay/**` |
| `device` | `crates/taskdeck-device/**`, `tests/e2e/**`; and whenever `core`, `seal` or `engine` is true |
| `recovery` | `crates/taskdeck-backup/**`, `crates/taskdeck-recover/**`; and whenever `core` or `seal` is true |
| `android` | `android/**`, `crates/taskdeck-android/**`; and whenever `device` is true |
| none | `docs/sync/status/**`, `docs/sync/research/**`, `docs/sync/runbooks/**`, `deploy/**` (today's server's install script and unit file), `*.md`, `*.png` and `LICENSE` at the root, `.gitignore` (unless a rule above matched first: `icon.png` is `root`) |

The `root` list is what the root package compiles or embeds (`include_bytes!`,
`include_str!`, `egui::include_image!`, `resources.rc`), plus `images/`, which
the app reads at run time and an ignored measurement test opens. A new file the
root package embeds must be added to `changes.sh` and to this table.

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
  `concurrency` group that cancels superseded runs;
- every `run:` that calls bash sets `shell: bash` (Windows runners); no
  `${{ }}` inside shell text: event data reaches scripts through `env:`;
- the toolchain step before anything that runs cargo;
- `gate` has `if: always()`, needs every other job, one per line, and is named
  `gate` only on `pull_request` events (`gate (push)` otherwise), so a branch
  rule that requires `gate` is satisfied only by the wide pull-request run.

A new job goes into the gate's `needs`; the test fails until it does.
