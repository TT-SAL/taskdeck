# Stage `core`: status

**State: final** (wave 1), since 2026-11-23 17:00 UTC; started 2026-11-23 00:00 UTC. Branch
`claude/brave-newton-xmg71m`; the exit pull request is TT-SAL/taskdeck#18. Every item of SYNC_PLAN.md §6.2 is
built, checked and green: `bash scripts/gate.sh core` passes every `run` item in the lead's run, and CI passes
on Windows, macOS and Linux with the budgets measured.
This file is the stage's memory (SYNC_PLAN.md §3.5). A fresh session resumes from it alone.

## Resuming: read this first

1. Read SYNC_PLAN.md in full, then this file. `docs/sync/spec/core.md` is the contract; the lead of the
   first session read it in full, with R1 and V2-1 to V2-5, V2-16, V2-17, V2-19.
2. Work on the branch your session names, from `day-planner`, and merge `day-planner` in before each workflow.
3. Continue at "Next steps". Every brief is in `docs/sync/status/core/briefs/`; the workflow that runs briefs
   is `docs/sync/status/core/workflows/pipeline.js`, with the arguments in `workflows/w*-args.json`; what the
   checkers found is kept in `docs/sync/status/core/findings/`.
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
  (one other session shares the window). The owner's windows run from 05:40, 10:40 and 15:40 UTC (17:40
  Helsinki time), not from the lead's first turn. The lead adds up what each workflow reports and starts nothing
  that would not fit; the ledger is below. The lead integrates once per finished workflow.

## Tasks

The plan's nine items (§6.2) are built as six task units; each unit is one pass through the task shape.

| Unit | Steps (writers) | Plan items | State | Tokens |
|---|---|---|---|---|
| A `a-base` | canonical encoding, time and zones, board data and `check`, id minting | 2, 4, 6 | **done**: built, checked (one high finding, fixed, second check clean), security pass (one high finding, fixed); lead's gate 2026-11-23 01:39 UTC: 77 tests, clippy, `ci.sh`, the purity item, CI's reads check | 1.16 M |
| B `b-apply` | B1 log commands and `parse_command`; B2 `apply`, conflict rules, rules versions, `reset`, `rebind`, golden logs; B3 the lead's rulings applied (the size check of CORE-APPLY-005, the tree budget in fixed numbers, `Record::LIMIT`) | 2, 3, 4, 5, 6 | **done**: built, checked (all 29 commands against §9 line by line, the conflict boundaries, over 170 golden cases worked by hand: nothing high or medium, one low), security pass (nothing high or medium, two lows); lead's gate 2026-11-23 02:14 UTC. Step B4 then applied spec change 4 (`reset` at the last revision, the 1,001-block golden case, a board that fails `check` no longer encodes to empty bytes) | 1.65 M |
| C `c-ports` | C1 planner, scoring, archive; C2 ICS parser, overlay types, weather report types, `compile::{trim, detab}`; C3 the calendar parser's cost bounded in six ways; C4 one step budget for all the rules of a file | 1 | **done**: built, checked (nothing high or medium), security pass (seven lows, fixed by ruling D11 in C3 with their reproductions as tests: the 8 MiB hostile feeds that timed out at 60 s now parse in 1.5 to 4.2 s in a debug build; C4: 20,000 events that each walk a full rule went from 37.7 s to 0.41 s in a release build); lead's gate 2026-11-23 12:10 UTC (332 tests) | 1.59 M |
| D `d-compile` | D1 intents and the compile step; D2 the lead's rulings applied (`briefs/d2-rulings.md`) | 3 | **done**: built, checked, security-passed (one medium and two high findings fixed with their reproductions as tests), rulings applied; lead's gate 11:59 UTC | 1.37 M |
| E `e-read` | E1 legacy import; E2 read queries, snapshot builders, reminders, the snapshot-fixtures test | 7, 8 | **done**: built and checked. The check found nothing (the fifteen import steps against the code, two folders worked by hand, the fourteen snapshot differences, mutations of the builders caught by the fixture test, the reminder vectors by hand). 43 of the 48 recorded snapshots match field for field; the other five are those core.md §18.7 excludes. Lead's gate 11:59 UTC with the `snapshot-fixtures` item | 0.93 M |
| F `f-gate` | F1 the transcript test; F3 three points from it (`briefs/f3-rulings.md`); F2 determinism and rebase property tests, fuzz targets, the budget job, spec coverage, the crate's README | 9 and the gate | **done**: built, checked (nothing high or medium; two lows). The checker broke `apply` and `compile` in four ways and the transcript test caught each (31 to 232 differences); a zone read, a naive time resolved in the process zone and marks kept in insertion order each failed the determinism tests. Lead's run 2026-11-23 16:27 UTC: `bash scripts/gate.sh core` passes every `run` item; the Linux budget run passes | 0.99 M |
| G exit | hygiene pass, exit review, exit pull request | §7 "once per stage" | **done**: the hygiene pass found three lows (two probe tests under scratch names, a real service's host name in a test copied from today's app, three `allow`s without a reason), all fixed by the lead; the exit review found one medium (CORE-CMD-006 was cited but no test failed if `apply` trimmed a name), fixed with its reproduction as a test, and one low, listed | 0.55 M |

## Next steps

None for this stage: it has exited. For the owner: wave 2 (`device`, `recovery`) starts once `seal`, `relay`
and `engine` have exited too (SYNC_PLAN.md §1.2). A later stage that needs a change under
`crates/taskdeck-core/` makes it through a small pull request of its own and notes it in both status files
(§3.2); a spec question goes through a spec-change pull request (§3.3).

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

- **D6. Counting tokens.** A workflow's own report and the sum over its agents' transcripts (input, cache
  writes and output) differ, in both directions (workflow 1: 1.51 M reported, 1.80 M by transcripts; workflow
  4: 0.13 M reported, 0.08 M by transcripts). The lead counts each workflow at the higher of its two numbers,
  as foundation's D25 did.
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

- **D11. The calendar parser's cost is bounded, although the findings are low on the plan's scale.** The
  security pass on the ported ICS parser found feeds of 120 KB to 1 MB that parse for minutes or take over a
  gigabyte; all of it is today's parser, unchanged. On the plan's scale that is low (robustness on hostile
  input), and low findings are listed, not fixed. The lead has them fixed all the same, once, in step C3:
  the parser now runs on the phone against text from any address a calendar link names, and
  `device.md` DEV-ICS-004 gives a fetch 20 seconds, which no device could keep. core.md §14.6 said "today's
  parser, unchanged"; spec change 5 states the bounds.
- **D12. The fuzz package's lockfile.** `crates/taskdeck-core/fuzz` carries its own `[workspace]` table and
  `Cargo.lock`, as `cargo fuzz init` lays it out, so `scripts/supply-chain.sh` (cargo-deny, cargo-audit, the
  cooldown) does not read that lockfile. It is covered this way: every one of its 28 entries has a version the
  root `Cargo.lock` already holds, `libfuzzer-sys` 0.4.13 included (published 2026-06-04, checked against the
  crates.io index; pre-approved by SYNC_PLAN.md §3.4 for tooling). The lead checked it at the exit. The
  fuzzers never run in a pull request: `ci.sh` only compiles them, on Linux; stage `release` runs them in its
  nightly job and should extend the supply-chain gate to this lockfile when it adds `cargo-fuzz`.
- **D13. The container gate does not time the budgets.** `scripts/gates/core.gate` names the budgets a CI item;
  `bash scripts/gate.sh core` runs `ci.sh` without `CI_OS`, which skips the budget runs and the fuzz check.
  CI's core job sets `CI_OS`, so both run there on Linux. The lead runs `CI_OS=linux bash
  crates/taskdeck-core/ci.sh` by hand at each integration that touches them.
- **D14. Spec change 6 takes back part of change 5.** Change 5 made a naive wire time outside the instant range
  `core/date_range`, for uniformity; the transcript test showed that today answers "not a time", and nothing
  required the change. The rule the transcript test enforces is the better guide: a behaviour today's app has
  changes only for a reason the spec states.
- **D15. Budget baselines: the slower of two runs, compared on GitHub's runners only.** The two CI runs of the
  same commit gave 111.7 and 133.3 ms for the apply budget (19% apart), so one run as the baseline would put the
  next ordinary run near the 25% line of SYNC_PLAN.md §3.3; the slower of the two is the baseline. The
  container is not a GitHub runner (its checkpoint load is about 80 ms against CI's 51), so `ci.sh` checks the
  baselines only where `GITHUB_ACTIONS=true` and the limits everywhere.
- **D10. Fewer turns of the lead.** The first window ended at the account's limit although this stage's agents
  stayed under their 3.5 M: the lead took about a hundred turns, most of them woken by a single GitHub event
  (a check finished, a pull request merged), and every turn reads the whole conversation again. From the
  second session window on the lead does not subscribe to a pull request's events and sets no safety-net
  timers: it reads the `gate` check once, when it integrates, and waits for workflows only.

## Deviations from the plan

- **The budgets run in the core job's own hook, not in a job of their own** (SYNC_PLAN.md §3.3: "Measure in a
  dedicated CI job"). `scripts/gates/core.gate`, written by `foundation`, names the core stage's `ci.sh` hook
  as that job; the hook runs on the same `ubuntu-latest` runner after the tests and clippy, so the numbers are
  a shared runner's. The exit review raised it; the tightly clustered runs (within 3% inside a run) say the
  numbers are usable.
- **Low findings were fixed where they were cheap or mattered** (the calendar parser, D11; the hygiene pass's
  three), although SYNC_PLAN.md §7 lists low findings rather than fixing them.

## Spec changes raised by this stage

| # | What | State |
|---|---|---|
| 1 | core.md §18.3: a golden case may name a generated start board (`start_gen`) for the caps of §17, whose boards are too large to print (200,000 archive rows is about 22 MB of JSON) | merged, TT-SAL/taskdeck#5 |
| 2 | core.md §18.4, §18.5: what the transcript and snapshot tests do with a fixture folder that `import` refuses. `dst-edges` holds three records before 1970-01-01T00:00:00Z (`instant_range`) and `id-limits` holds ids outside every range (`id_range`); the spec said only "starting from `import` of the starting folder" | merged, TT-SAL/taskdeck#5 |
| 3 | The lead's rulings on what tasks A and B found: the size check of `apply` (new clause CORE-APPLY-005), the parser's tree budget, `Record::LIMIT`, the fold (D7), and nine smaller points made exact. `docs/sync/spec/CHANGES.md` lists them; `briefs/b3-rulings.md` is the same list as a task | merged, TT-SAL/taskdeck#7 |
| 4 | From task B's check: `reset` at the last notes revision is `core/cap`; a command text over the tree budget is `core/schema` whatever its `op` (§9.1); CORE-ID-003 does not cover a benign `restore`; the 1,001-block `create_task` of §18.3 is a parse error in the golden logs | merged, TT-SAL/taskdeck#11 |
| 5 | From tasks C, D and E: the calendar parser's seven bounds (new clause CORE-READ-007); a local time outside the instant range is `core/date_range`, not a daylight-saving gap (CORE-TIME-003 reworded); `compile` makes every check `apply` makes (`core/cap_board`, a stale `set_notes` base, the two `restore` checks, the rules check, a minter that is not the device's); an intent is a JSON object; the import's open points as implemented; `unreadable` and `legacy-other/empty` in §18.7; `build_report` leaves `fetched_at` to the caller. `docs/sync/spec/CHANGES.md` has the row | merged, TT-SAL/taskdeck#15 |
| 6 | From the transcript test (F1): a naive wire time outside the instant range keeps today's answer ("not a time"), where change 5 had made it `core/date_range` without need; §18.4 says how the replay keeps its id map (per id list), that a row's id is not compared, and what a block index with no block becomes | merged, TT-SAL/taskdeck#17 |

## Token ledger

Agent tokens (input, cache writes and output), each workflow at the higher of its own report and the sum over
its agents' transcripts (D6). The windows are the owner's (see "How this stage is run").

| Window (UTC) | Workflow | Agents | Tokens | Window total |
|---|---|---|---|---|
| to 2026-11-23 02:53 | 1: task A whole, B1 and B2 implemented | 8 | 1,514,311 reported; 1,800,254 by transcripts | 1.80 M |
| | 2: B3, check and security pass of B, C1 implemented | 4 | 1,037,538 reported; 1,159,154 by transcripts | 2.96 M |
| | 3: C2 implemented | 1 | 245,001 reported; 306,345 by transcripts | 3.27 M |
| | 4: B4 (four points of spec change 4) | 1 | 129,140 reported; 84,825 by transcripts | 3.39 M; the account's limit was reached at 02:53 (the lead's own turns, D10) and the session stood still until 09:45 |
| 05:40 to 10:40 | 5, its part before 10:40: check of C; E1; security pass of C; D | 4 | about 1.64 M by transcripts | 1.64 M |
| 10:40 to 15:40 | 5, the rest: check and security pass of D; E2 | 5 | about 0.66 M (workflow 5 in all: 1,879,829 reported; 2,298,333 by transcripts) | 0.66 M |
| | 6: the check of E; D2 (rulings); C3 (the calendar parser's bounds) | 3 | 742,325 reported; 877,782 by transcripts | 1.54 M |
| | 7: C4 (a step budget for a file's rules) | 1 | 125,831 reported; 86,852 by transcripts | 1.63 M |
| | 8: F1 written; F2 cut by the account's limit at about 13:20 | 2 | 395,808 reported; 337,147 by transcripts | 1.97 M; the other session and the lead's turns used the rest |
| 15:40 to 20:40 | 10: F3, F2, the check of F | 3 | 593,008 reported; 660,709 by transcripts | 0.66 M |
| | 9: hygiene pass and exit review, one fix | 3 | 475,648 reported; 550,167 by transcripts | 1.21 M |

The stage in all: 35 agent runs, about 8.4 M tokens by the rule above (8.2 M by transcripts alone): builders
5.0 M (17 implementer runs, one of them cut by the limit, and 5 fixers), checkers 3.2 M (7 checks, 4 security
passes, the hygiene pass and the exit review). Foundation spent about 34 M, most of it checking (its D24).

## What the checks found

| Task | Pass | Finding | Outcome |
|---|---|---|---|
| A | verify | high: `ChronoZone::from_local` took chrono's first candidate as the earlier one; under `TZ=Europe/Berlin` chrono lists the later first, so `resolve_on_day` returned 01:30Z for 02:30 on 2026-10-25 instead of 00:30Z | fixed (candidates checked by a round trip, returned ascending); the second check found nothing |
| A | security | high: `canon::parse` built a `serde_json` tree with no bound; a 60 MiB text of small objects took 7.6 GB and the process aborted | fixed with a budget; made platform-independent by ruling (D9) |
| B | verify | nothing high or medium; one low (below) and four points for the lead (spec change 4) | no fix pass needed |
| B | security | nothing high or medium; two lows, both about the cost of the size check on very large boards | no fix pass needed |
| A, B | CI (windows-latest) | the dependency test looked for the manifest's table header between bare newlines; a Windows checkout has CRLF | fixed by the lead (the test reads by lines), proven against a CRLF copy |
| C | verify | nothing high or medium: every ported function compared with today's source line by line, no expected value of a ported test changed; one low (a test of today's not ported) | no fix pass; the test comes with C3 |
| C | security (ICS parser) | seven lows, all inherited from today's parser: a 120 KB feed parses for 109 s (unbounded `BY…` lists), a 1.1 MB feed for 178 s (a scan over an unbounded zone table), a 1 MB feed peaks at 1.16 GB (overrides bypass the per-event cap), a duration near `i64::MAX` overflows, unbounded problem lines that quote the file, an unstable order of ties, an overflow in `OverlayEvent::minutes` | **fixed by ruling (D11)**, step C3, although low on the scale |
| C | CI (windows-latest) | a test searched the ICS source for a signature across a line break (CRLF again) | fixed by the lead; the rule is now in `briefs/common.md` |
| D | verify | medium: `compile` returned a command that `apply` refuses for size on a board at the 59 MiB cap (CORE-COMP-008) | fixed: `compile` runs `apply`'s size check on the view |
| E | verify | nothing, high, medium or low | no fix pass needed |
| F | verify | nothing high or medium; two lows (below) | no fix pass needed |
| exit | hygiene | three lows: `tests/oom_probe.rs` and `tests/probe_verify.rs`, real regression tests under scratch names; `sisu.helsinki.fi`, a real service's host, in a test of `overlay` copied from today's app; three `allow`s without a stated reason | all fixed by the lead: renamed `canon_tree_budget.rs` and `compile_size_check.rs`, the host made up, the reasons written |
| exit | review | medium: CORE-CMD-006 was cited, but no test failed if `apply` trimmed a `rename` name; one low (below) | fixed: `tests/apply_verbatim.rs` applies six commands with names that carry leading and trailing White_Space and fails under each of six such mutations |
| D | security (intent parser) | high: `parse_intent` accepted a JSON array as an intent (`["forget",5]`), because a serde derive also reads the sequence form; high: `compile` emitted a `set_notes` with a base of 2^53 or more, which encoded to empty bytes | both fixed; `LogCommand::to_canonical` loses its silent empty result in step D2 |

## Low findings left as they are

- **The size check counts the whole board on boards over about 15 MB** (task B, security). The cheap bound
  (six bytes per string byte) passes the cap from about 107,000 archive rows; from then on a command that can
  lengthen the board pays an exact count (2.5 ms at 200,000 rows), so 5,000 entries take 18 to 32 s instead
  of 0.3 s. Results are right; the cost is on boards decades away. A cached length in `BoardData` would
  remove it and is a change to §20.
- **At the cap, every lengthening command runs on a copy of the board** (task B, security): about 85 ms per
  entry on a 59 MiB board. Such a board is full; a member that fills a board is answered by revoking it.
- **A calendar past the parser's limits still depends on listing order** (task C, C4's report): over
  `max_occurrences` the first events win, `Overlay::sealed` truncates to 5,000 before it sorts, and two
  overrides with one `RECURRENCE-ID` resolve by file order, as today. Bound 6 of §14.6 covers ties only.
- **The per-event cap can be passed by up to 399 occurrences** (task C, C4's report): as today, the check runs
  after each start's span is cut into days. Kept, because a strict cut would change today's output for long
  multi-day series.
- **The rebase property test never reaches a `move_blocks` skip for a block another device changed** (task F,
  verify). CORE-CONF-003's "changed since" path is proven by the golden logs, not by the property tests.
- **The apply budget times a clone of the 10,000-row board per run besides the 5,000 applies** (task F, verify);
  core.md §17 says only `apply` is timed. The number is the worse for it (97.6 ms against 1,000 ms), never
  the better, so the budget holds a fortiori.

- **The test of CORE-READ-003 has no tied scores** (exit review), so reversing the tie order of `ranked_ids`
  passes it; the snapshot fixtures have no ties either (core.md §18.5 assumes so). Scores carry a jitter by
  id, so a tie needs two malformed items; the order is still pinned by the code's stable sort over ascending ids.

Fixed since they were listed: `reset` at the last notes revision (step B4); the overlay test of today's that
was not ported (step C3).

## Measurements

| What | Value | Where |
|---|---|---|
| An agent of this stage (input, cache writes, output) | implementers 0.19 to 0.45 M; a check 0.17 to 0.26 M; a security pass 0.14 M; a fix 0.09 to 0.13 M | workflow 1 |
| Workflow 1 | 8 agents, 61 minutes | 2026-11-23 00:10 to 01:16 UTC |
| `BoardData::from_canonical_bytes`, a made-up board of 4.4 MB with 10,000 archive rows, release build | 71 ms median in the container (budget: 150 ms in CI, CORE-PERF-003) | task A's verifier; not the budget job |
| The `golden` test target, debug build | 25 s (249 cases; the generated boards for the caps dominate) | lead's gate run |
| A spec-only pull request in CI | 6 minutes | TT-SAL/taskdeck#5, #7 |
| A pull request that touches the core crate, in CI | 8 minutes; the core job runs on Windows, macOS and Linux | TT-SAL/taskdeck#8 |
| The size check of `apply` (CORE-APPLY-005), release build, a board of 10,000 items (2.4 MB) | about 12 µs per command; 5,000 renames in 69 ms; an exact count of a 200,000-row board 2.5 ms | task B3's implementer; not the budget job |
| Workflow 2 | 4 agents, 42 minutes | 2026-11-23 01:28 to 02:10 UTC |
| A port: C1 (2,300 lines of today's code, 76 tests) and C2 (2,000 lines, 76 tests) | 0.28 M and 0.31 M tokens, 22 and 13 minutes | workflows 2 and 3 |
| The first usage window | 14 agent runs, 3.39 M tokens: tasks A and B whole, C implemented | 2026-11-23 00:10 to 02:35 UTC |
| The crate at the exit review | 340 tests (unit and integration), all passing on Windows, macOS and Linux; 100 of the 100 clauses the stage owns cited by a test (`bash scripts/spec-coverage.sh core`) | lead's run, CI |
| **Budget baselines on `ubuntu-latest`** (CORE-PERF-001 to -003), median of five | 31-day snapshot of the 10,000-row board **0.941 ms** (limit 40); 5,000 commands applied **133.279 ms** (limit 1,000; the two runs gave 111.7 and 133.3, and the slower is the baseline; includes a board clone per run); checkpoint loaded **51.083 ms** (limit 150; 48.0 and 51.1) | the push run and the pull-request run of TT-SAL/taskdeck#18, 2026-11-23 16:34 UTC; recorded in `crates/taskdeck-core/ci.sh`, compared on GitHub's runners only |
| Budgets in the container, `CI_OS=linux bash crates/taskdeck-core/ci.sh`, median of five | snapshot 0.98 to 1.1 ms; apply 97.6 to 117.6 ms; checkpoint 76 to 83 ms | lead's runs 2026-11-23 16:27 and 16:52 UTC |
| The snapshot fixtures | 43 of the 48 recorded snapshots equal field for field after core.md §18.5's transformation; the other five are those §18.7 excludes | `--test snapshot_fixtures` |
| The transcripts, steps compared and not compared | Berlin: `every-command` 51/0, `refusals` 73/1, `dst-edges` 13/4, `seeded-01` to `-04` 30/0 each, `-05` and `-06` 20/0 each, `id-limits` 0/6 (refused, §18.7). New York: the same but `refusals` 19/55 and `dst-edges` 14/3 (a deadline in New York's 9999 that is 10000 in UTC ends the comparison, §13.5) | `--test transcripts` |
| The calendar parser on hostile feeds | 8 MiB of a repeated `BYDAY`: killed at 30 s before, 1.5 s after (debug); 20,000 daily rules from year 1: 37.7 s before, 0.41 s after (release) | steps C3 and C4 |
| Workflow 10 | 3 agents, 35 minutes | 2026-11-23 15:48 to 16:23 UTC |

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
- A test that reads one of the crate's own text files must go by lines: a Windows checkout has CRLF line
  endings (`fixtures/` is exempt through its `.gitattributes`; JSON vectors do not mind).
- The working copy and the branch share no history for the crate's files (the lead copies files, it does not
  merge the agents' commits), so the working copy is never merged with the branch: after an integration it is
  reset to the branch, and a writer's commit that is not yet integrated is cherry-picked on top.
- The library builds without chrono's `alloc` feature, so `format()` does not exist there; the ported date
  strings are written out by hand.
- `libfuzzer-sys` compiles libFuzzer's C++ sources in its build script even under `cargo check`, so the fuzz
  check needs a C++ compiler (the container and `ubuntu-latest` have one).
- GitHub can cancel a queued job before it starts (no steps; `Stage device · macos-latest` of run 45217123916),
  and `gate` then fails on `cancelled`. It is not a failure of the pull request: the next push runs it again.
- The session's worker was restarted twice; the second restart moved the lead from Fable 5.2 to Opus 5.5
  (2026-11-23 15:42 UTC). Agents ran on `model: 'opus'` throughout.

## Open questions for the owner

None.

## To tell the owner at exit

- `apply` refuses a command that would lengthen a board past 59 MiB (D8). At 350 bytes a row, that is some
  170,000 archive rows; nothing near it for decades of ordinary use.
- A time typed inside the repeated hour of a fall-back day now means the first of the two (D7); today's
  desktop on Linux takes the second.
- The calendar parser's findings were low on the plan's scale and were fixed all the same (D11): a feed of
  about a megabyte took minutes or over a gigabyte, against the 20 seconds a device has for a fetch.
- Loading a board through the generic JSON tree takes about fifteen times its size in memory for a moment
  (a 4 MB board: about 60 MB). A direct reader would cut that; the budget job shows the load at about 83 ms
  against 150, so it is not built (D9).
- Four fixture folders are refused by the import as the spec defines it: `dst-edges` (instants before 1970 in
  UTC), `id-limits` (ids outside every range), `unreadable` (an archived name of 50,071 bytes) and
  `legacy-other/empty` (a notepad file that is not a string). That is the import check doing its job;
  `desktop`'s gate item "the import check passes on every legacy fixture" has to read "runs and reports" for
  these four.
- The fuzz package keeps its own lockfile, outside the supply-chain gate, equal to the root lockfile at the
  exit (D12).
- A test of today's app names a real service's host (`sisu.helsinki.fi`, in `src/subscriptions.rs` around line
  840). The core's copy of that test now uses a made-up host; today's copy is stage `desktop`'s to change or
  delete with the old code.

## What the next stages need to know

**For every stage that links `taskdeck-core`** (`device`, and through it `desktop`, `android`, `recovery`):

- **The API is core.md §20.** Two notes it carries: `canon::Record` has a required `const LIMIT`, so a record
  type of another crate names its byte limit; `BoardData::to_canonical_bytes` and `LogCommand::to_canonical`
  panic for a board that fails `check` or a command holding an integer above 2^53 − 1, so encode only what
  passed `check`, `parse_command` or `compile`. No public signature differs from §20.
- **Zones and instants.** Pass `ChronoZone(chrono::Local)` or a `TableZone`; every zone parameter takes
  `&dyn Zone`. A fold resolves to the earlier instant; a local time outside the instant range resolves to none
  (core.md §5.4). `Instant::plus_seconds` saturates at `Instant::MAX` past the end of the range (§5.1) and
  returns none only below its start.
- **`compile`.** `CompileCtx.device_no` must equal the minter's `device_no` (`core/bad_id` otherwise). A
  `set_notes` always carries `view.notes_rev` and refuses a stale intent `base` with `core/notes_stale`, so the
  page sends `base` from the snapshot's `notes_base`. A full board is `core/cap_board`. A reflow keeps its
  first 1,000 moves. Codes, statuses and messages are core.md §16, character for character.
- **`apply` and the engine.** The size check of CORE-APPLY-005 refuses with `core/cap`; `reset` returns
  `core/cap` at the last notes revision; a benign repeat is `Applied { benign: true, .. }` with nothing
  changed; `Notice::concerns` is the author and the other device for an overwrite, the author alone for a skip.
  `SUPPORTED_RULES` is `[1]`. The engine's state-machine adapter (`device`'s task 2) wraps `apply`, `reset`,
  `ids::rebind`, `RefusalRecord::to_canonical` and `compile`.
- **The import.** The four refused folders are listed under "To tell the owner". A report holds counts, ids
  and kinds only; a failed one holds its `failure` and nothing else. `blocks` counts live blocks.
- **The read side.** `snapshot` gives today's JSON with the fourteen changes of core.md §14.3 (`notes_base`,
  sids for session indexes, canonical `archived_at`, the board's palette). `archive_by_day` reads at most 366
  days; a reminder lead above 1,440 minutes is read as 1,440.
- **Calendars and weather.** `ics::parse` keeps the seven bounds of core.md §14.6, and its `problems` quote
  nothing from the file. `weather::build_report` leaves `Report.fetched_at` at 0: the caller sets it. An
  overlay read from a cache goes through `Overlay::sealed` before use.
- **Tests and CI.** A test of the crate reads fixtures with `include_str!` / `include_bytes!`, never at run
  time; a test that reads a text file must not depend on CRLF. CI's core job prints three `budget:` lines on
  Linux and fails at a limit or 25% over a baseline (`crates/taskdeck-core/ci.sh`).

**For particular stages:**

- `desktop`: CORE-READ-006 (the calendar summary orders a copy by (start, id, sid)) and the import report of
  CORE-IMP-003 are partly yours; the notepad sends `set_notes` with the base of the text it edited; the palette
  and the home place go out as the `set_palette` and `set_home` intents.
- `android`: reminders come from `reminders::reminders`; Done compiles `complete`, Tomorrow compiles `move_block`
  one day later (or `set_event_span` for an event).
- `release`: the fuzz targets are `crates/taskdeck-core/fuzz` (`command_apply`, `board_bytes`,
  `intent_compile`, `ics_parse`, `snapshot`); run them nightly, and bring the fuzz lockfile under the
  supply-chain gate (D12).
