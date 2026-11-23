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
| A `a-base` | canonical encoding, time and zones, board data and `check`, id minting | 2, 4, 6 | **done**: built, checked (one high finding, fixed, second check clean), security pass (one high finding, fixed); lead's gate 2026-11-23 01:39 UTC: 77 tests, clippy, `ci.sh`, the purity item, CI's reads check | 1.16 M |
| B `b-apply` | B1 log commands and `parse_command`; B2 `apply`, conflict rules, rules versions, `reset`, `rebind`, golden logs (249 cases); B3 the lead's rulings applied (the size check, the tree budget in fixed numbers, `Record::LIMIT`) | 2, 3, 4, 5, 6 | B1 and B2 implemented and in the tree, **not yet checked**; B3, the check and the security pass are running (workflow 2) | 0.64 M so far |
| C `c-ports` | C1 planner, scoring, archive; C2 ICS parser, overlay types, weather report types | 1 | C1 running (workflow 2) | |
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

- **D6. Counting tokens.** A workflow's own report (1.51 M for workflow 1) was lower than the sum over its
  agents' transcripts (1.80 M: input, cache writes and output). The lead budgets against the higher number.
- **D7. A local time in a fold: the spec's rule stands.** Task A's verifier found that chrono 0.4.45's `Local`
  lists the later instant of a fold first, so today's app resolves 02:30 on a fall-back day to the later
  instant, while CORE-TIME-003 says the earlier. Most platforms take the earlier, and today's result depends
  on chrono's order, so the clause is kept and the difference is written into the spec (§5.2, §5.4). The
  recorded transcripts hold the later instant in nine steps of `dst-edges`; they are replayed with a zone
  that reads a fold as the recording did (§13.5), which keeps every one of those steps comparable.
- **D8. A size check in `apply`** (CORE-APPLY-005, core.md §8.4 step 4). Ruling S18 gave rules version 1 caps
  "so a snapshot always fits its 64 MiB limit", but the caps did not bound the bytes: 200,000 archive rows of
  300 bytes are 60 MB, and a board over 60 MiB cannot be loaded, checkpointed or snapshotted again. `apply`
  refuses a command that would lengthen a board past 59 MiB; shortening always works; 1 MiB is left for what
  `reset` adds. It costs a pass over the lists per command, no new field in `BoardData`.
- **D9. A tree budget in the canonical parser, in fixed numbers** (core.md §4.5). Task A's security pass showed
  a 60 MiB text costing 7 GiB before any schema ran. The fixer's budget used `size_of`, which differs between
  a 32-bit phone and a 64-bit desktop, so the same bytes could parse on one and not on the other; the spec
  now fixes the charges. A parser that reads a board straight into its records, without the generic tree,
  would use far less memory; it is left until the budget job shows whether the 150 ms load budget needs it.

## Spec changes raised by this stage

| # | What | State |
|---|---|---|
| 1 | core.md §18.3: a golden case may name a generated start board (`start_gen`) for the caps of §17, whose boards are too large to print (200,000 archive rows is about 22 MB of JSON) | merged, TT-SAL/taskdeck#5 |
| 2 | core.md §18.4, §18.5: what the transcript and snapshot tests do with a fixture folder that `import` refuses. `dst-edges` holds three records before 1970-01-01T00:00:00Z (`instant_range`) and `id-limits` holds ids outside every range (`id_range`); the spec said only "starting from `import` of the starting folder" | merged, TT-SAL/taskdeck#5 |
| 3 | The lead's rulings on what tasks A and B found: the size check of `apply` (new clause CORE-APPLY-005), the parser's tree budget, `Record::LIMIT`, the fold (D7), and nine smaller points made exact. `docs/sync/spec/CHANGES.md` lists them; `briefs/b3-rulings.md` is the same list as a task | merged, TT-SAL/taskdeck#7 |

## Token ledger

Agent tokens as the workflows report them (input, cache writes and output).

| Window (UTC) | Workflow | Agents | Tokens | Window total |
|---|---|---|---|---|
| 2026-11-22 23:57 to 2026-11-23 04:57 | 1: task A whole, B1 and B2 implemented | 8 | 1,514,311 reported; 1,800,254 by transcripts | 1.80 M |
| | 2: B3, check and security pass of B, C1 implemented | running | | |

## What the checks found

| Task | Pass | Finding | Outcome |
|---|---|---|---|
| A | verify | high: `ChronoZone::from_local` took chrono's first candidate as the earlier one; under `TZ=Europe/Berlin` chrono lists the later first, so `resolve_on_day` returned 01:30Z for 02:30 on 2026-10-25 instead of 00:30Z | fixed (candidates checked by a round trip, returned ascending); the second check found nothing |
| A | security | high: `canon::parse` built a `serde_json` tree with no bound; a 60 MiB text of small objects took 7.6 GB and the process aborted | fixed with a budget; made platform-independent by ruling (D9) |

## Low findings left as they are

None so far.

## Measurements

| What | Value | Where |
|---|---|---|
| An agent of this stage (input, cache writes, output) | implementers 0.19 to 0.45 M; a check 0.17 to 0.26 M; a security pass 0.14 M; a fix 0.09 to 0.13 M | workflow 1 |
| Workflow 1 | 8 agents, 61 minutes | 2026-11-23 00:10 to 01:16 UTC |
| `BoardData::from_canonical_bytes`, a made-up board of 4.4 MB with 10,000 archive rows, release build | 71 ms median in the container (budget: 150 ms in CI, CORE-PERF-003) | task A's verifier; not the budget job |
| The `golden` test target, debug build | 25 s (249 cases; the generated boards for the caps dominate) | lead's gate run |
| A spec-only pull request in CI | 6 minutes | TT-SAL/taskdeck#5, #7 |

## Facts found along the way

- chrono 0.4.45: `Local.from_local_datetime` on a fold returns `Ambiguous(later, earlier)` under
  `TZ=Europe/Berlin`, and reports a fold at the first minute after one (03:00 on 2026-10-25).
- A push to an open pull request cancels its running CI, and the cancelled run's `gate` then reports a
  failure for the old head. It is not a failure of the pull request: check the head it names.
- The `check_suite` event for a green run does not always arrive. Ask for the pull request's check runs
  (the first four are enough: `gate` is listed first) rather than wait.
- A cloud session's stop hook asks for every commit to be pushed when the lead's turn ends, so nothing an
  agent wrote may be committed in the main checkout before the lead's gate has run on it.
- `rustfmt` is not part of the pinned toolchain, and CI runs no format check.

## Open questions for the owner

None.

## To tell the owner at exit

- `apply` refuses a command that would lengthen a board past 59 MiB (D8). At 350 bytes a row, that is some
  170,000 archive rows; nothing near it for decades of ordinary use.
- A time typed inside the repeated hour of a fall-back day now means the first of the two (D7); today's
  desktop on Linux takes the second.
- Loading a board through the generic JSON tree takes about fifteen times its size in memory for a moment
  (a 4 MB board: about 60 MB). A direct reader would cut that; it is not built unless the budget job asks (D9).

- Two fixture boards are refused by the import as the spec defines it (`dst-edges`: instants before 1970 in
  UTC; `id-limits`: ids outside every range). That is the import check doing its job; `desktop`'s gate item
  "the import check passes on every legacy fixture" has to read "runs and reports" for those two.
