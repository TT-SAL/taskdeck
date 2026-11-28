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

**The owner, 2026-11-28 (two messages):** agents are generally never on the `max` effort level,
only the most important security or otherwise critical ones; and agents whose instructions are
thorough and whose work needs little judgment may run on `sonnet`, at `high` effort (Sonnet is not
usable on the lowest levels). The lead sets model and effort on every agent: the builders (adapters,
store, runtime, pairing, facade) and spec drafting on `opus` at `high`; the end-to-end harness, a
mechanical task with a full brief, on `sonnet` at `high`; none of this milestone's agents on `max`.

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
meet each case, then judged by the lead here). W1a found 31 (h-1 to h-3, f1-1 to f1-15, f2-1 to
f2-13); their judgement and the spec change for them come with workflow W1a-spec. The ones that
weigh most: f1-5 (a genesis snapshot of a non-empty board is circular as the spec stands: its body
needs the frame that names its hash; only an empty board can be created today), f1-3 (seal has no
opener for one own frame outside `classify`), f1-1 and f1-2 (seal's `Membership` and `Receipt` give
the engine's adapter less than the traits need, so the adapter parses the §13.2 bytes itself), f2-12
(`KeyLost` cannot show the old device's name), h-1 (the standby's origin must name a port that port 0
only learns after the start).

## Token ledger

| Workflow | Agents | Report | Transcripts | Booked | Running total |
|---|---|---|---|---|---|
| W1a, first launch (wf_90de5f43-084), stopped at once by the lead to move H to `sonnet` (the owner's message) | 2 | — | 0.17 M | 0.17 M | 0.17 M |
| W1a (wf_94e3dd4d-937): H 0.18 M (sonnet), F1 0.46 M, F2 0.44 M | 3 | 0.89 M | 1.09 M | 1.09 M | 1.26 M |

Each agent costs about 83,000 tokens of cache writes before it does anything (its system prompt and
tool definitions), so the stage uses few, larger agents.

## Workflows

| Run | Workflow | State | Result |
|---|---|---|---|
| wf_90de5f43-084 | W1a, first launch | stopped by the lead at once | nothing kept (H moved to `sonnet`) |
| wf_94e3dd4d-937 | W1a: H ∥ (F1 → F2), briefs `h-harness.md`, `f1-adapters.md`, `f2-store.md` | done, integrated (43 min, 201 tool uses) | H: the harness (`tests/e2e/harness/`: `Standby`, `Ctl` with typed helpers, Node 22 lookup), the standby smoke test and seven tests of the harness against a Node stand-in device, `tests/e2e/ci.sh`. F1: `CoreMachine`, `SealSealer`, `SealMembership` (`src/adapt/`), the engine in process on them from genesis through an edit, a restart and a second edit, 33 tests. F2: the sealed store (`src/store/`: file layer, data folder and lock, the §6.4 encodings, record JSON, `IdentityStore` with `persist`, `read_log`, `read_base`, `open` with torn and bad tails), `FileKeyStore` and `MemoryKeyStore`, 17 tests and F1's flow from disk. 31 findings (`fit.md` h-1 to h-3, f1-1 to f1-15, f2-1 to f2-13) |

**The lead's gate run of W1a** (2026-11-28): `scripts/ci/stage.sh device` (51 unit tests; the e2e
target's 8; clippy) and the `test-trust` build (53 tests, clippy) pass; `Cargo.lock` and every path
outside `device`'s are unchanged; no ignored test, no print outside tests; `unimplemented!()` 135 of
the skeleton's 161 left, all in facade methods no task has built. **One defect, fixed by the lead:**
DEV-GEN-001 allows `unsafe` only in the module `platform::windows`; F2 had put the Windows
`MoveFileExW` call in `store/os_files.rs`'s module `platform_windows`. Moved, unchanged, to
`src/platform.rs` (`crate::platform::windows`); its `// SAFETY:` comment was there. The Windows CI
job is its compile check (no Windows target in the container).

## Pull requests

| Pull request | What | On day-planner |
|---|---|---|
| TT-SAL/taskdeck#41 | W1a: adapters, store, harness | (waiting for CI) |

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
