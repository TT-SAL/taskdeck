# Briefs of stage `device`

One file per agent task. The rules below hold for every brief; each brief adds only what differs.
The stage's status file is `docs/sync/status/device.md` ("How this stage is run" overrides the plan
where they differ).

## Reading

- **Never read a spec in full.** The clause index (every clause on one line, under its section
  heading) is built already at `$TD_SCRATCH/clauses.txt`; grep it, then read only the sections your
  brief names (`sed -n` on the line ranges from `grep -n '^#' docs/sync/spec/<file>.md`). Rebuild it
  with `python3 docs/sync/status/foundation/tools/clause-index.py docs/sync/spec
  $TD_SCRATCH/clauses.txt` if it is missing; never write it inside the repository.
- `$TD_SCRATCH` is `/tmp/claude-0/-home-user-taskdeck/cf010276-5fe8-544b-94ad-f4bb073be52f/scratchpad`.
  Keep your scratch files under `$TD_SCRATCH/<your task id>/`, never in the repository.
- Read large files once, and in parts. Prefer `grep -n` and `sed -n` over whole files.

## Where to work

- The chain of the device crate (tasks F1, F2, F3, F4) works in `/home/user/taskdeck` on branch
  `claude/optimistic-lovelace-6z2f55`. One writer at a time in a crate (plan §3.1): a task that runs
  beside yours works in a git worktree of its own, never in a crate you are writing.
- Every cargo command sets `CARGO_TARGET_DIR=/home/user/td-target/<your task id>` (two agents
  compile at once).
- Change only the paths your brief lists. Never edit: `crates/taskdeck-core/`,
  `crates/taskdeck-seal/`, `relay/`, `docs/sync/spec/`, the root package (`src/`, `build.rs`),
  `Cargo.lock`, the root `Cargo.toml`, `.github/`, `scripts/`, `fixtures/`, other stages' status
  files.
- **`crates/taskdeck-sync/`** (the engine; its stage is paused): change it only when an engine fault
  blocks your task and cannot be worked around in the device crate. Then: a commit of its own,
  touching only `crates/taskdeck-sync/`, subject starting `engine:`, with a test in that crate that
  fails before the fix and passes after; `cargo test --locked -p taskdeck-sync` (which runs the
  simulator's 50 default seeds) and `cargo clippy --locked -p taskdeck-sync --all-targets -- -D
  warnings` must pass. **Never change the simulator (`src/sim/**`), the model relay, the fake
  (`src/fake*`), the toy or an invariant**, and never push only to get new seeds. A seed that fails
  only after your change: stop, record it in `fit.md` with the seed and the first bad trace line,
  and report it; do not chase it.
- A fault in `core`, `seal` or the relay: do not edit those paths. Work around it in the device crate
  if that is honest, record it in `fit.md` with a reproduction (a command, or a test kept in your
  scratch directory), and report it. If it blocks you entirely, report `done: false` with the
  blocker.

## Writing

- Rust as the crates around it: no `unsafe`; secrets in `zeroize::Zeroizing` and zeroized on drop;
  constant-time comparison of secrets (`subtle`); every size checked before allocation; nothing
  parsed before it is authenticated (plan §3.4). Never print or log a token, key, seed, pairing code
  or create secret (DEV-CTL-002).
- **The spec's file and wire formats from the first line**: every byte the store writes and the
  transport sends is as `device.md`, `seal.md` and `relay.md` state, even where their tests come
  later. Shortcuts are allowed only behind the feature `test-trust` (device.md §11.1).
- **No new dependency.** Only what `crates/taskdeck-device/Cargo.toml` already declares; dev-only
  use of what it declares under `[dev-dependencies]`. `Cargo.lock` must not change: every cargo
  command takes `--locked`. If a crate feature you need would change `Cargo.lock`, stop and report
  it as a blocker.
- Tests next to the code they test, deterministic. Cite the clause id a test checks in its name or
  doc comment where there is one; do not write tests only to cite clauses. Never `#[ignore]`, skip
  or loosen a test; delete one only with the code it tests. No `unimplemented!()` or `todo!()` in
  code you write; the skeleton's `unimplemented!()` stays only in facade methods no task has built
  yet.
- Unit tests do not sleep or touch the network. The end-to-end harness may wait, always with a
  timeout.

## Findings: `docs/sync/status/device/fit.md`

Finding where the real parts do not fit, or where a spec is wrong, unclear or silent, is what this
milestone is for. Record every case as you meet it, appending to `fit.md` (create it with a one-line
header if it is missing):

```
## <task id>-<n>. <one-line title>
- Kind: misfit | spec-wrong | spec-unclear | spec-silent | decision | engine-fault | core-fault | seal-fault | relay-fault
- Where: <spec file §section / clause id; trait method; crate function, file:line>
- Says / does: <what the spec or trait says; what the real crate does>
- Done here: <the workaround or decision taken, and where in the code>
- Proposed fix: <spec change (the text, briefly) | engine change | core/seal/relay change | none>
- Evidence: <test name, or the command and its output in one line>
```

A **decision** is a choice the spec leaves to `device` (for example the pairing runtime's open points
of `docs/sync/spec/CHANGES.md`): state the choice and the reason. Keep entries short; the lead turns
them into spec changes.

## Ending

- Green means, from the repository root (or your worktree):
  `cargo test --locked -p taskdeck-device`, `cargo test --locked -p taskdeck-device --features
  test-trust`, `cargo clippy --locked -p taskdeck-device --all-targets -- -D warnings` and the same
  clippy with `--features test-trust` all pass, plus whatever your brief adds.
- Commit each green step, and at least at the end. Subject `device: <what> (stage in progress)`; end
  the message with these two lines:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_01QD8DmVJD7VT4kKpAr3537u`.
  The chain pushes each commit: `git push -u origin claude/optimistic-lovelace-6z2f55` (retry up to
  four times on a network error, waiting 2, 4, 8, 16 s). A task in a worktree commits on its own
  branch and never pushes. Never push to another branch, never force-push, never open a pull
  request.
- `git status` clean of stray files before the last commit.
- Your final answer is the structured output your task asks for. `done` is true only if green.
  `next` is what the next task must know that is not in the code or `fit.md`.
