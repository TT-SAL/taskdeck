# Stage `core`: status

**State: in progress** (wave 1), started 2026-11-23 00:00 UTC. Branch `claude/brave-newton-xmg71m`.
This file is the stage's memory (SYNC_PLAN.md §3.5). A fresh session resumes from it alone.

## Resuming: read this first

1. Read SYNC_PLAN.md in full, then this file. `docs/sync/spec/core.md` is the contract; the lead of the
   first session read it in full, with R1 and V2-1 to V2-5, V2-16, V2-17, V2-19.
2. Work on the branch your session names, from `day-planner`, and merge `day-planner` in before each workflow.
3. Continue at "Tasks": the first row that is not `done`. Its brief is in `docs/sync/status/core/briefs/`;
   the workflow that runs briefs is `docs/sync/status/core/workflows/pipeline.js`.
4. Never run two sessions on this stage. A workflow cannot be resumed from another session: what is
   committed and pushed is what exists.

## How this stage is run

- **Task shape** (SYNC_PLAN.md §7): one agent writes, one agent that did not write checks once on a frozen
  copy, one pass fixes what was high or medium, each finding first reproduced as a test. A second check only
  after a high finding. A security pass (one agent) after the fix where the task parses untrusted input: the
  canonical and board-bytes parsers, `parse_command`, the ICS parser, the intent parser.
- **Agents.** Every `agent()` call sets `model: 'opus'`; implementers, verifiers and security passes at
  effort `high`, fixers at `medium`. At most two agents run at once: one writer and one checker.
- **Working copy.** Writers work in a clone, `target/work/core` (git-ignored), one at a time, and commit
  there after their step gate passes. A checker works on a frozen worktree of the commit it checks
  (`target/work/check-<label>`), so the next writer can run beside it. The lead takes only
  `crates/taskdeck-core/**` from the clone into the main checkout, runs the gate there, commits and pushes;
  nothing an agent wrote reaches the repository before the lead's gate run. (An agent's commit in the clone
  is a snapshot for the checkers, never pushed.)
- **Briefs.** `briefs/common.md` holds the rules every agent follows; one brief per step names the files,
  the spec line ranges, the lead's rulings, and the tests and vectors to write. Agents never read a spec in
  full. An agent that meets a gap in the spec takes the reading closest to the text, marks it
  `SPEC-QUESTION` and reports it; the lead rules, and a spec change goes through its own pull request.
- **Budget.** The owner's limit for this session is 3.5 million agent tokens per five-hour usage window
  (one other session shares the window). The lead adds up what each workflow reports and starts nothing
  that would not fit; the ledger is below. The lead integrates once per finished workflow.

## Tasks

The plan's nine items (§6.2) are built as six task units; each unit is one pass through the task shape.

| Unit | Steps (writers) | Plan items | State | Tokens |
|---|---|---|---|---|
| A `a-base` | canonical encoding, time and zones, board data and `check`, id minting | 2, 4, 6 | running | |
| B `b-apply` | B1 log commands and `parse_command`; B2 `apply`, conflict rules, rules versions, `reset`, `rebind`, golden logs | 2, 3, 4, 5, 6 | running (implementation) | |
| C `c-ports` | C1 planner, scoring, archive; C2 ICS parser, overlay types, weather report types | 1 | not started | |
| D `d-compile` | intents and the compile step | 3 | not started | |
| E `e-read` | E1 legacy import; E2 read queries, snapshot builders, reminders, the snapshot-fixtures test | 7, 8 | not started | |
| F `f-gate` | F1 transcripts test; F2 determinism and rebase property tests, fuzz targets, the budget job, spec coverage | 9 and the gate | not started | |
| G exit | hygiene pass, exit review, exit pull request | §7 "once per stage" | not started | |

## Decisions (with reasons)

- **D1. Six task units, not nine tasks.** The plan's items overlap in code (`apply` is items 2, 4, 5 and 6
  at once), and every unit costs one check and one fix whatever its size. A unit is cut where one checker
  can still hold the whole of it: by module, in dependency order. Where a unit has two implementers they
  run one after the other and one checker checks both; the shape's properties hold (the writer does not
  judge its own work; a finding needs a reproduction; a failed agent is never a clean result).
- **D2. Agents commit in the working copy.** A checker needs a frozen tree while the next writer works, and
  a workflow script cannot run git itself. The commits are snapshots inside `target/work/`; the lead's
  commits on the branch are the only ones pushed.
- **D3. Tests embed fixtures and vectors with `include_str!` / `include_bytes!`.** The crate's `clippy.toml`
  forbids file and environment access in every target, and the gate rejects an `allow` of those lints, so a
  test cannot open `fixtures/` at run time. CI's reads check follows the embeds, and `fixtures/**` already
  marks the `core` area.
- **D4. Unit tests live in `src/<module>/tests.rs`.** `scripts/spec-coverage.sh core` counts clause ids only
  in `tests/**` and `src/**/tests.rs`, and CI's reads check cannot follow a `#[path]` beside an inline module.
- **D5. Which tasks get a security pass.** Those that add a parser of input another party controls: the
  canonical and board-bytes parsers (A), `parse_command` (B), the ICS parser (C), the intent parser (D). The
  legacy import (E) reads the owner's own folder once, in a dry run first; it gets the never-panic property
  test CORE-GEN-007 asks for and no separate pass.

## Spec changes raised by this stage

| # | What | State |
|---|---|---|
| 1 | core.md §18.3: a golden case may name a generated start board (`start_gen`) for the caps of §17, whose boards are too large to print (200,000 archive rows is about 22 MB of JSON) | in the first pull request |
| 2 | core.md §18.4, §18.5: what the transcript and snapshot tests do with a fixture folder that `import` refuses. `dst-edges` holds three records before 1970-01-01T00:00:00Z (`instant_range`) and `id-limits` holds ids outside every range (`id_range`); the spec said only "starting from `import` of the starting folder" | in the first pull request |

## Token ledger

Agent tokens as the workflows report them (input, cache writes and output).

| Window (UTC) | Workflow | Agents | Tokens | Window total |
|---|---|---|---|---|
| 2026-11-23 00:00–05:00 | | | | |

## Low findings left as they are

None yet.

## Measurements

None yet.

## Open questions for the owner

None.

## To tell the owner at exit

- Two fixture boards are refused by the import as the spec defines it (`dst-edges`: instants before 1970 in
  UTC; `id-limits`: ids outside every range). That is the import check doing its job; `desktop`'s gate item
  "the import check passes on every legacy fixture" has to read "runs and reports" for those two.
