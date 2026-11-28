# Stage `device`: status

**State: in progress, milestone 1 of 2 ("device: first sync").** Not final. Lead: the session on
branch `claude/optimistic-lovelace-6z2f55`. Started 2026-11-28 while stage `engine` is paused before
its exit (its code is merged, its session paused; `docs/sync/status/engine.md`). This file is the
stage's memory: rewritten as things change, not appended to.

## How this stage is run (the owner's decisions of 2026-11-27)

These override SYNC_PLAN.md where they differ; everything else of plan §3 holds.

1. **Start although `engine` has not exited.** Its code is merged and its session is paused. The
   stage may change `crates/taskdeck-sync/` through small pull requests of their own, noted here
   and in `docs/sync/status/engine.md` ("Changes by the device session"). Where the plan allows, the
   adapters that fit the real core and seal to the engine's traits live in `crates/taskdeck-device/`
   (`device.md` §4.1, decision D6), so that most pull requests leave `crates/taskdeck-sync/` alone.
   A pull request that changes `crates/taskdeck-sync/` runs the simulator's 50 default seeds, chosen
   from the merge commit's id (CI does this: `cargo test -p taskdeck-sync`). If such a pull request
   fails only on one of the simulator's seeds, that is `engine`'s open item: it is listed in
   `engine.md` as open; the simulator, the model relay, the fake and the invariants are not changed
   for it; nothing is pushed only to get new seeds; and the owner is told.
2. **One milestone for now, in two parts.**
   - First **"device: first sync"**: two `taskdeck-devicectl` processes with the real core and the
     real seal in the engine. One creates a board, the other pairs with it, an edit made on each
     reaches the other through `relay/standby.js` on loopback, and each still holds the board after
     it is stopped and started again. It is a test under `tests/e2e/` that CI runs.
   - Then **"device: facade"**: what `desktop` needs of the facade, with local-only mode and the read
     queries.
   This is plan §6.7 tasks 1 and 2, and only as much of tasks 3, 4, 5 and 9 as they need.
3. **The spec's file and wire formats from the first line**, even where their tests come later.
   Shortcuts for now: what sits behind the feature `test-trust`. **Not now:** the crash-kill suite,
   backoff and budgets, platform key stores, fetchers, blobs beyond the snapshot that pairing needs,
   operations, the hostile-proxy suite, V2's scenarios, backups.
4. **The end-to-end test is the check** in this milestone: no separate verify or security agents
   (this overrides plan §7) and no chase for clause coverage. The lead still runs every gate and
   reads CI itself, and plan §3.2 and §3.4 hold in full.
5. **Where the real parts do not fit, or a spec is wrong:** finding that is what this milestone is
   for. Each case is listed here ("Where the parts did not fit"). Fixes go through spec-change pull
   requests as plan §3.3 says, one per workflow for everything it found.
6. **Merge each pull request as soon as it is green**, marked "(stage in progress, not the exit)".
7. **Budget: 5 million agent tokens**, counted as the other stages' ledgers do (below). Integrate
   once per finished workflow and keep the lead's own turns few. At 3 million, tell the owner where
   the stage is; at 5 million, stop.
8. **Then stop and report:** what works, every place the parts did not fit, what it cost, and what
   the lead would do next. The rest of the stage does not start.

**The owner, 2026-11-28:** agents are generally never on the `max` effort level, only the most
important security or otherwise critical ones. The lead sets `effort` on every agent: `high` for
the builders, `medium` for the harness and spec drafting; none of this milestone's agents is `max`.

The lead may open pull requests from its branch into `day-planner` and merge them itself once CI is
green. It asks the owner only about what plan §3.5 lists; otherwise it decides, records the decision
here, and keeps going.

## The plan of work

All agents run on model `opus`, at most two at a time (plan §3.1), one writer at a time in a crate.
Briefs: `docs/sync/status/device/briefs/` (rules common to all in its `README.md`). Every agent's
findings go to `docs/sync/status/device/fit.md` as they are met.

| Workflow | Agents | What | Pull requests |
|---|---|---|---|
| W1a | H ∥ (F1 → F2) | H: the end-to-end harness and a standby smoke test (`tests/e2e/`). F1: the adapters `StateMachine` over core and `Sealer` over seal, with an in-process engine test. F2: the sealed store, the key stores of `test-trust`, `Loaded` at open | the work; one spec change for what it found |
| W1b | F3 → F4 | F3: transport, runtime, the facade's subset, creating a board, `taskdeck-devicectl`, one device end to end through the standby. F4: pairing (both roles, the snapshot blob it needs), the first-sync test in CI | "device: first sync"; one spec change |
| W2 | to plan after W1b | the facade for `desktop`: local-only mode, the read queries | "device: facade"; one spec change |

## Decisions (with reasons)

- **L1. One branch, pull requests in sequence** (as `engine` L4). The session pushes only to its own
  branch, so a pull request "of its own" (a spec change, a change to `crates/taskdeck-sync/`, to
  another exited stage's crate, or to a shared file) is the branch's only commits at that moment.
  Agents commit and push their work to the branch as they go (a container can be reclaimed). A
  change outside `device`'s paths is made in a commit of its own, touching only that path, with a
  subject that starts with its area (`engine:`, `core:`, `seal:`, `relay:`); at integration the lead
  puts such commits into a pull request of their own first (restarting the branch from
  `day-planner`, the other commits kept and replayed after it merges).
- **L2. The end-to-end harness is a test target of `taskdeck-device`**: `[[test]] name = "e2e"`,
  `path = "../../tests/e2e/main.rs"`, `required-features = ["test-trust"]`, so the code lives under
  `tests/e2e/` (device.md §4.1, §20.1) and the test finds the binary through
  `CARGO_BIN_EXE_taskdeck-devicectl`, with no new workspace member and no change to `Cargo.lock`.
  `tests/e2e/ci.sh` runs `cargo test --locked -p taskdeck-device --features test-trust --test e2e`.
  The gate file's row `e2e` (`scripts/gates/device.gate`) names the command without
  `--features test-trust`; that row is put right when the stage's gate is first run, in a pull
  request of its own.
- **L3. The end-to-end test runs on the Linux job only for now** (CI_OS `linux`); on Windows and
  macOS `tests/e2e/ci.sh` says so and exits 0. Node 22 with `node:sqlite` is found as
  `relay/ci.sh` finds it. The other operating systems come with the crash suite and the key stores.
- **L4. The standby is reached over plain `http` on `127.0.0.1`** (device.md §11.1 item 1, behind
  `test-trust`), not over TLS with a test certificate; `TrustRoots::Extra` comes with the Worker
  runs.
- **L5. Tokens are booked as `seal` books them**: per workflow, the workflow's own report and the
  sum over its agents' transcripts of input, cache writes and output
  (`docs/sync/status/foundation/tools/tokens.py <journal.jsonl>`, its `in+cw+out`); the higher is
  booked.

## Where the parts did not fit

The full list, with evidence, is `docs/sync/status/device/fit.md` (written by the agents as they
meet each case, then judged by the lead here). None yet.

## Token ledger

| Workflow | Agents | Report | Transcripts | Booked | Running total |
|---|---|---|---|---|---|
| | | | | | 0 |

## Workflows

| Run | Workflow | State | Result |
|---|---|---|---|

## Pull requests

| Pull request | What | On day-planner |
|---|---|---|

## Gate

`bash scripts/gate.sh device` cannot pass during this milestone (the hostile-relay, crash and
fetcher suites, coverage and the skeleton's `unimplemented!()` are later work). What the lead runs
for each pull request: the CI jobs it triggers, read by the lead; `cargo test` and `cargo clippy` of
`taskdeck-device` with and without `test-trust`; `bash tests/e2e/ci.sh`; and `cargo test -p
taskdeck-sync` (with its 50 default seeds) when the engine's crate changes.

## Open questions for the owner

None yet.

## Commands

- Device tests: `cargo test --locked -p taskdeck-device` and again with `--features test-trust`
- Lints: `cargo clippy --locked -p taskdeck-device --all-targets -- -D warnings`, again with
  `--features test-trust`
- End to end: `bash tests/e2e/ci.sh` (needs Node 22)
- CI's steps for this stage: `bash scripts/ci/stage.sh device`
- The clause index: `python3 docs/sync/status/foundation/tools/clause-index.py docs/sync/spec <file
  outside the repository>`
- Tokens of a finished workflow: `python3 docs/sync/status/foundation/tools/tokens.py
  <transcript dir>/journal.jsonl`
