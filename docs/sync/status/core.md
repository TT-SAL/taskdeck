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
| B `b-apply` | B1 log commands and `parse_command`; B2 `apply`, conflict rules, rules versions, `reset`, `rebind`, golden logs; B3 the lead's rulings applied (the size check of CORE-APPLY-005, the tree budget in fixed numbers, `Record::LIMIT`) | 2, 3, 4, 5, 6 | **done**: built, checked (all 29 commands against §9 line by line, the conflict boundaries, over 170 golden cases worked by hand: nothing high or medium, one low), security pass (nothing high or medium, two lows); lead's gate 2026-11-23 02:14 UTC. Step B4 then applied spec change 4 (`reset` at the last revision, the 1,001-block golden case, a board that fails `check` no longer encodes to empty bytes) | 1.65 M |
| C `c-ports` | C1 planner, scoring, archive; C2 ICS parser, overlay types, weather report types, `compile::{trim, detab}`; C3 the calendar parser's cost bounded (lead's ruling on the security pass) | 1 | C1 and C2 **built and checked**: nothing high or medium (one low); the security pass on the ICS parser: seven lows. C3 running (workflow 6) | 1.15 M so far |
| D `d-compile` | D1 intents and the compile step; D2 the lead's rulings applied | 3 | D1 **built, checked, security-passed**: one medium and two high findings, all fixed with their reproductions as tests; lead's gate 2026-11-23 11:09 UTC (293 tests). D2 running (workflow 6) | 1.11 M so far |
| E `e-read` | E1 legacy import; E2 read queries, snapshot builders, reminders, the snapshot-fixtures test | 7, 8 | E1 and E2 built and in the tree: 43 of the 48 recorded snapshots match field for field, the other five are the ones core.md §18.7 says not to compare; lead's gate 11:09 UTC with the `snapshot-fixtures` item. Their check is running (workflow 6) | 0.63 M so far |
| F `f-gate` | F1 transcripts test; F2 determinism and rebase property tests, fuzz targets, the budget job, spec coverage | 9 and the gate | not started | |
| G exit | hygiene pass, exit review, exit pull request | §7 "once per stage" | not started | |

## Next steps

1. **The window from 10:00 UTC:** workflow 5, `workflows/w5-args.json` with `workflows/pipeline.js`:
   the check of task C (both ports) and its security pass (the ICS parser), while the writers go on with D
   (`briefs/d-compile.md`), E1 (`e1-import.md`) and E2 (`e2-read-snapshot.md`); D gets its check and its
   security pass (the intent parser) in the same workflow. About 2.7 to 3.2 M tokens. Before launching: bring
   the working copy to the branch (`git -C target/work/core fetch origin && git -C target/work/core reset --hard origin/<branch>`).
2. Spec change 5 once the check of C has reported, in its own pull request, then the code.
3. **The window after it:** the check of E; F1 (`f1-transcripts.md`), F2 (`f2-gate.md`) and their check; the budget
   baselines from the first CI run of the budget job into `crates/taskdeck-core/ci.sh` and this file; then the
   hygiene pass, the exit review, and the exit pull request with this file's state set to `final`.

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
- **D10. Fewer turns of the lead.** The first window ended at the account's limit although this stage's agents
  stayed under their 3.5 M: the lead took about a hundred turns, most of them woken by a single GitHub event
  (a check finished, a pull request merged), and every turn reads the whole conversation again. From the
  second session window on the lead does not subscribe to a pull request's events and sets no safety-net
  timers: it reads the `gate` check once, when it integrates, and waits for workflows only.

## Spec changes raised by this stage

| # | What | State |
|---|---|---|
| 1 | core.md §18.3: a golden case may name a generated start board (`start_gen`) for the caps of §17, whose boards are too large to print (200,000 archive rows is about 22 MB of JSON) | merged, TT-SAL/taskdeck#5 |
| 2 | core.md §18.4, §18.5: what the transcript and snapshot tests do with a fixture folder that `import` refuses. `dst-edges` holds three records before 1970-01-01T00:00:00Z (`instant_range`) and `id-limits` holds ids outside every range (`id_range`); the spec said only "starting from `import` of the starting folder" | merged, TT-SAL/taskdeck#5 |
| 4 | From task B's check: `reset` at the last notes revision is `core/cap`; a command text over the tree budget is `core/schema` whatever its `op` (§9.1); CORE-ID-003 does not cover a benign `restore`; the 1,001-block `create_task` of §18.3 is a parse error in the golden logs | merged, TT-SAL/taskdeck#11 |
| 5 | From tasks C, D and E (`briefs/d2-rulings.md` is the list, `briefs/c3-ics-bounds.md` the calendar parser's bounds): the size check in `compile` (`core/cap_board`), `set_notes` with a stale base refused at compile, a local time outside the instant range is not a gap, the compile codes the table lacked, the import's open points, `unreadable` and `legacy-other/empty` in §18.7, the ICS bounds. Earlier wording of this row: from the second port: `weather::build_report` leaves `Report.fetched_at` at 0 and the caller sets it (today's function reads the system clock there); an ICS time in UTC outside the instant range takes the zone's offset at the nearer end of the range. With whatever the check of the ports adds | to come, after the check of task C |
| 3 | The lead's rulings on what tasks A and B found: the size check of `apply` (new clause CORE-APPLY-005), the parser's tree budget, `Record::LIMIT`, the fold (D7), and nine smaller points made exact. `docs/sync/spec/CHANGES.md` lists them; `briefs/b3-rulings.md` is the same list as a task | merged, TT-SAL/taskdeck#7 |

## Token ledger

Agent tokens as the workflows report them (input, cache writes and output).

| Window (UTC) | Workflow | Agents | Tokens | Window total |
|---|---|---|---|---|
| 2026-11-22 23:57 to 2026-11-23 04:57 | 1: task A whole, B1 and B2 implemented | 8 | 1,514,311 reported; 1,800,254 by transcripts | 1.80 M |
| | 2: B3, check and security pass of B, C1 implemented | 4 | 1,037,538 reported; 1,159,154 by transcripts | 2.96 M |
| | 3: C2 implemented | 1 | 245,001 reported; 306,345 by transcripts | 3.27 M |
| | 4: B4 (four points of spec change 4) | 1 | 129,140 reported; 84,825 by transcripts | **3.39 M: the window is spent**; agents rest until about 04:57 UTC |
| 2026-11-23 05:00 to 10:00 | none: the account's limit was hit at about 02:53 UTC, in the first window, and this session stood still until 09:45 UTC | 0 | 0 | 0 |
| 2026-11-23 10:00 to 15:00 | 5: check and security pass of C; E1; D with its check and security pass; E2 (`workflows/w5-args.json`) | 9 | 1,879,829 reported; 2,298,333 by transcripts | 2.30 M |
| | 6: the check of E; D2 (rulings); C3 (the calendar parser's bounds) | running | | |

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
| D | security (intent parser) | high: `parse_intent` accepted a JSON array as an intent (`["forget",5]`), because a serde derive also reads the sequence form; high: `compile` emitted a `set_notes` with a base of 2^53 or more, which encoded to empty bytes | both fixed; `LogCommand::to_canonical` loses its silent empty result in step D2 |

## Low findings left as they are

- **`reset` at the last notes revision** (task B, verify). With `notes_rev` at 2^53 − 1 in either board,
  `reset` returns a board with `notes_rev` 2^53, which fails `check`. Nine quadrillion notepad saves away;
  settled by spec change 4 (`core/cap`) and fixed with the next writer step, since it costs a line.
- **The size check counts the whole board on boards over about 15 MB** (task B, security). The cheap bound
  (six bytes per string byte) passes the cap from about 107,000 archive rows; from then on a command that can
  lengthen the board pays an exact count (2.5 ms at 200,000 rows), so 5,000 entries take 18 to 32 s instead
  of 0.3 s. Results are right; the cost is on boards decades away. A cached length in `BoardData` would
  remove it and is a change to §20.
- **At the cap, every lengthening command runs on a copy of the board** (task B, security): about 85 ms per
  entry on a 59 MiB board. Such a board is full; a member that fills a board is answered by revoking it.

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
