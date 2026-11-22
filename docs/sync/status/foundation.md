# Stage `foundation`: status

**State: in progress** (wave 0). The second lead session started on 2026-11-21 at 21:20 UTC on
branch `claude/upbeat-hamilton-klp7n9`. The first ran from 12:39 to about 19:45 UTC on branch
`claude/focused-volta-tbqufb` and stopped at a clean point because its context was nearly full; its
pull request, TT-SAL/taskdeck#1, is merged into `day-planner` and was not the exit pull request.
This file is the stage's memory (SYNC_PLAN.md §3.5).

## Resuming: read this first

1. Read SYNC_PLAN.md in full (it is short), then this file to the end. SYNC.md and the papers in
   `docs/sync/research/` are reference for judging what agents ask; a resuming lead need not reread
   them all before starting.
2. Work on the branch your session names, from `day-planner`. Open a draft pull request early.
3. Continue at "Next steps". The working papers a lead needs are in `docs/sync/status/foundation/`
   (see "Working papers"): a brief for every remaining task, the code digests, a ready workflow for
   step 1, and a small journal helper.
4. Never run two sessions on this stage. The first session has stopped and cancelled its scheduled
   resume.
5. A workflow cannot be resumed from another session. What a session's workflows finished is in this
   file and in the commits; what they had not finished starts again from "Next steps".

## How this stage is run

- Workflows of Opus agents (`model: 'opus'`; agents report Opus 5.5), at most two agents at a time.
  Since the second session: two workflows side by side, each running strictly one agent at a time
  (D15). The **spec track** only reads and writes under `docs/sync/spec/`; the **code track** is the
  only one that compiles. When one track waits for the lead or has no work, the other may run two
  agents wide.
- Agents write; the lead runs every gate and reads CI itself. Agents never commit or push and never
  run a git command that changes state.
- Since the second session, writers never touch the main checkout (D18). Each track has a working
  copy, a clone under `target/work/` (`c` for code, `s` for specs), which git ignores. Implementers,
  fixers and attackers work on the clone; reviewers read the main checkout. When a writer has
  finished, the lead copies its files into the main checkout, runs the gate there, commits and
  pushes, then moves the clone's HEAD to the new commit
  (`git -C target/work/c fetch -q origin && git -C target/work/c reset -q --mixed origin/<branch>`).
  The main checkout is clean whenever the lead's turn ends, and nothing reaches the public
  repository before the lead has read it.
- Every implementation task goes through the task loop of SYNC_PLAN.md §7 (implement, attack through
  three lenses, fix, until two dry rounds). Spec work uses drafters, pairwise interface reviews and
  the two-readers test (§6.1).
- Each task has a brief the agents read (scope, owned paths, gate, lead rulings, findings already
  handled). The lead adds a ruling to the brief the moment a loop starts to circle (D12, D14).
- A running loop reads a lead inbox each round (`<scratch>/lead-findings/<task>.md`, one `## ` section
  per finding), so the lead can feed in a finding or a ruling without stopping the workflow.
- The session's stop hook wants a clean tree whenever the lead's turn ends. With the working copies
  that holds by construction. A background `tools/waitwriter.py <journal> <seen>` wakes the lead
  when the next implementer or fixer has finished.
- Pull-request CI runs the full matrix on every push, about 13 minutes; results arrive as
  notifications. Every pushed head of the first session was green.
- Usage (D13): one five-hour window of the account allows about 60 agent runs at these settings.
  Size a workflow to about 20 runs, stop launching at about four-fifths of a window, and ask the
  owner for the meter when unsure. A workflow cut by the limit loses only its unfinished agents.

## Working papers (`docs/sync/status/foundation/`)

| Path | What |
|---|---|
| `briefs/drafter-common.md`, `briefs/{seal,core,relay,engine,device,bridge}.md` | the rules and briefs the six specs were drafted under; spec fixers reread them |
| `briefs/reviews.md` | task descriptions for the pairwise interface reviewer, the resolver, the per-spec fixer, and the requirement map |
| `briefs/task-1a.md` | baseline task 1a: scope, gate, the lead's seven rulings, every finding already handled |
| `briefs/supply-chain.md`, `briefs/ci.md`, `briefs/workspace.md`, `briefs/fixtures.md`, `briefs/tooling.md` | the lead's designs for tasks 1b, 1c, 2, 5 and 6, ready to hand to an implementer |
| `briefs/core-digest.md`, `briefs/device-bridge-digest.md` | what the code digests were asked to contain |
| `digests/core-digest.md`, `digests/device-digest.md`, `digests/bridge-digest.md` | facts about today's code with file and line, written for the drafters; useful to `core`, `device` and `android-shell` too |
| `workflows/s1-interface-reviews.js` | spec track, round 1: nine pairwise reviews, then one resolver per pair, one after another, writing `<scratch>/resolutions/r1-<a>-<b>.md`. It stops there: the lead rules on each "For the lead" list before fixers edit a spec (D16). Launch with `args: { scratch: <session scratchpad>, width: 1 }` |
| `workflows/c1-supply-chain-1a-loop-ci.js` | code track, on the working copy `target/work/c`: finishing task 1b, task 1a's attack loop until two dry rounds, task 1c's implementation. Launch with `args: { scratch: <session scratchpad> }` after creating `lead-findings/` in the scratchpad, cloning the working copy and warming its `target/debug` and `target/attack-0` to `-2` (copies of the main `target/debug` after `cargo test --locked --no-default-features --lib --no-run`) |
| `tools/waitwriter.py`, `tools/speccheck.py` | the lead's waiter for finished writers, and a quick format check of the six specs (clause lines, unique ids, owners, MUST outside clauses, closing sections, dangling references). Neither is the stage's tooling; `scripts/spec-lint.sh` comes with task 6 |
| `tools/wf.py` | prints which agents of a running workflow have finished and their reports (`WF_JOURNAL=<transcript dir>/journal.jsonl`) |
| `tools/tokens.py`, `tools/sync-clone.sh` | adds up the tokens of a workflow's agents from their transcripts (`tokens.py <journal.jsonl>`), per agent and in total; brings a track's working copy to the pushed branch after the lead has integrated |

These are working papers of the stage, not contracts. The exit pull request may delete them.

## Tasks

| # | Task (SYNC_PLAN.md §6.1) | State | Evidence |
|---|---|---|---|
| 0 | Status file, first pull request | done | this file; TT-SAL/taskdeck#1, merged |
| 1a | Toolchain pin, lockfile refresh, `panic = "unwind"` with its release test, `build.rs` date | **done.** The fourth loop (second session) ran six rounds: rounds 1 and 3 dry, rounds 2 and 4 one low finding each about which invalid `SOURCE_DATE_EPOCH` spellings the date check tries (fixed; rulings 8 and 9 in the brief close that list), rounds 5 and 6 dry on all three lenses: converged. The lead ran the gate after each fix: 265 passed, 1 ignored | commits `6ca1240`, `938e4b9`; `briefs/task-1a.md` |
| 1b | `deny.toml`, cargo-deny, cargo-audit, seven-day cooldown | **done.** The lead's lockfile rulings (D19) are applied and proven in CI on Windows, macOS and Linux. The first loop of six rounds found a real gap in every round (index failures failing open, twice; notices; a lockfile source in another spelling; replaced crates.io sources; a configuration `include`), all fixed and each verified by the lead (script tests, the gate, the cooldown over all 519 packages). Ruling 8 in the brief then made the gate's five properties a closed list, and the second loop's rounds 1 and 2 were both dry on all three lenses (real tools on scratch projects, the lockfile compared entry by entry with the base, fresh tool and cargo homes, dead network): converged | `briefs/supply-chain.md`; commits to `e4c81fe` |
| 1c | CI: path filters at job level, aggregate `gate` job | implemented, committed, and proven on GitHub: the pull-request run on `fee727d` was green in every job on all three systems, with `gate` named `gate` only on the pull-request run. Two loops so far, eleven attack rounds, and each found something real: the colour defect from the first real run, path-filter gaps, a file-name injection, cancelled push runs losing their changes, and, round after round, one more way to write a file read that the reads check did not follow. All fixed, each with a test; the lead ran the script tests after each loop. The second loop (six rounds) ended with the reads check failing closed on anything it cannot resolve to a tracked file (ruling 12) and with two low findings in `toolchain.sh` (ruling 13 closes that class). The second loop's fixes are proven on GitHub: the push run on `bef6b87` ran every area on all three systems and was green. Not converged: a third loop runs with the next code workflow | `briefs/ci.md`; `scripts/ci/README.md` |
| 2 | Workspace: member crates with every planned dependency pinned; `relay/`, `android/` READMEs | implemented: seven shell crates (core, seal, sync, device, backup, recover, android) with their dependencies in a root `[workspace.dependencies]` table, exact pins for the versions SYNC.md names, TLS on the ring provider only, one RustCrypto generation, 56 new third-party lockfile entries (the youngest published 2026-11-12) and nothing existing moved; READMEs for `relay/`, `android/`, `tests/e2e/`. Where a spec clause and the brief's table disagreed the implementer followed the clause (confirmed, ruling 1 of the brief). The lead ran every proof in the main checkout (20 steps, all passed, 2026-11-22 09:19 UTC: tests, clippy with each feature, wasm32, the Android target for the crates without C code, the stage script for every stage, supply chain, script tests, the reads check, the server build, today's 265 tests), and the pull-request run on `08dfc65` was green on Windows, macOS and Linux, so the device crate's Windows-only and macOS-only declarations compile there. Three lead findings wait for the attack loop (a default backend for the Android library, S32; `taskdeck-recover` depends on `taskdeck-device`; `rand_core` for seal) | `briefs/workspace.md` |
| 3a | Spec conventions (`docs/sync/spec/README.md`) | done, read in full by the lead | commit `460aa26` |
| 3b | Spec drafts: core, seal, relay, engine, device, bridge | seal drafted (2,038 lines, 155 clauses; the lead read §3–§10, §14, §17); core drafted (1,622 lines, 93 clauses; the lead read the report and §14.3); relay drafted and finished (1,846 lines, 166 clauses; typical day recomputed at 199 Worker requests, 215 object requests, 1,524 rows written, 4,217 rows read, all under 2%); engine drafted (2,390 lines, 182 clauses, with the outcome of each of V2's fifteen scenarios); device drafted (2,085 lines, 146 clauses); bridge drafted (1,655 lines, 110 clauses). All six are first drafts: 11,635 lines, 852 clauses | `docs/sync/spec/*.md` |
| 3c | Pairwise interface reviews: the plan's six pairs (seal–relay, seal–engine, core–engine, engine–relay, device–bridge, device–engine) plus core–device, seal–device, relay–device | **done.** Nine reviews brought 152 findings (26 high, 75 medium, 51 low); nine resolvers turned them into 232 exact changes; the lead ruled on the open points (L1 to L23); one more resolver wrote the rulings as exact text; fixers applied everything spec by spec and a second agent checked each spec against its diff (seal 102 of 102 items exact, core 15 of 15, relay 78 of 79 and the last fixed, engine 179 of 179, device 225 of 226 with the one a placement the lead confirmed, bridge 34 of 34; nothing unrequested). The six specs now hold 899 clauses (7 withdrawn) in 12,237 lines, and the lead's format check finds no problem. Five small follow-ups for the next spec pass are in `resolutions/r1-followups.md` | commits `220f07a` to this one; `resolutions/` |
| 3d | Two-readers test, six scenarios, until two rounds agree (at most four) | **rounds 1 and 2 done.** One fixed world and six scenarios (`briefs/two-readers.md`); readers work from the specs alone. In both rounds they agreed on every byte layout, counter, seq and chain value both gave. Differences that needed a spec change, all about behaviour the specs left open: 40 in round 1 (edit 10, join 17, trust 13), 12 in round 2 (edit 4, join 5, trust 3). All changes are applied (938 clauses, format check clean). The lead's rulings: `two-readers/r1/lead.md` (T1 to T10), `two-readers/r2/lead.md` (T11 to T13). Round 2 used two readers for all six scenarios (D22). Rounds 3 and 4 follow; what remains after round 4 goes to `spec/CHANGES.md` | `two-readers/r1/`, `two-readers/r2/` |
| 3e | Requirement map in `spec/README.md` (SYNC.md §3–§9 to clause, stage or plan §2 row) | todo | |
| 4 | Skeleton crates: public types and traits, toy state machine, fake sealer | todo; brief ready (`briefs/skeletons.md`). Comes after the two-readers rounds have settled the specs' "Public API" sections, with a last check of the surface after round 4 | |
| 5 | Fixtures as data: boards, legacy `main` formats, snapshot JSON, transcripts | todo; brief ready (`briefs/fixtures.md`) | |
| 6 | Tooling: `scripts/gate.sh`, `scripts/spec-coverage.sh`, `scripts/dev-setup.sh` | todo; brief ready (`briefs/tooling.md`) | |
| 7 | Stage exit review, exit pull request, merge | todo | |

## Next steps

In this order; a resuming session starts at the first open one. Steps 1 and 6 to 8 are the spec
track, steps 2 to 5 the code track (D15); the two run side by side.

Where the second session stands (2026-11-22 09:45 UTC): tasks 0, 1a, 1b, 3a, 3b and 3c are done.
Task 1c is implemented and green in CI after two attack loops that did not converge; a third loop
is queued. Task 2 is implemented, proven by the lead and green in CI on all three systems; its
attack loop is queued, with three lead findings. Two-readers rounds 1 and 2 are done and applied
(938 clauses); rounds 3 and 4 follow. Nothing is running: the third usage window (07:20 to 12:20
UTC) stands at about 6.1 million agent tokens, so the next launches wait for the reset. Ready to
launch then: `workflows/s4-two-readers-later-round.js` with round 3 and the follow-ups 6 to 13 of
`resolutions/r1-followups.md`; `workflows/c4-workspace-loop-fixtures-tooling.js` with the steps
`1c-loop`, `workspace-loop` and `fixtures` (then `fixtures-loop`, `tooling`, `tooling-loop` by
resume). After those: task 4 (`briefs/skeletons.md`) once round 4 has settled the specs' "Public
API" sections, the requirement map (3e), and the stage exit.

1. **Interface reviews of the six spec drafts.** Nine pairs, each reviewer also checking that the
   rulings S1 to S31 are reflected; then one resolver per spec, in dependency order, writes the exact
   changes (D16); the lead rules on every "For the lead" item; then one fixer per spec applies them.
   Repeat with a second, lighter round until a round brings nothing of substance.
2. **Task 1a's attack loop** again from round 1 until two dry rounds. Scope and the closed mutation
   list are in `briefs/task-1a.md` (D11, D12, D14).
3. **Task 1b** (`briefs/supply-chain.md`). When the implementer reports which advisories remain in
   the lockfile, the lead decides for each between a targeted `cargo update -p` and a reviewed
   ignore, then runs its attack loop. SYNC_PLAN.md §6.1 allows `cargo update -p` only for rustls,
   rustls-webpki, webpki-roots and tokio, so any other crate needs a written reason here.
4. **Task 1c** (`briefs/ci.md`). Push it early in a pull request: only a real CI run shows whether
   the path filter, the per-stage jobs and the `gate` job behave. Then its attack loop.
5. **Task 2** (`briefs/workspace.md`), **task 5** (`briefs/fixtures.md`), **task 6**
   (`briefs/tooling.md`), each with its attack loop. Task 2 needs task 1b's scripts; task 6 comes
   after the crates and fixtures exist.
6. **Two-readers test** on the reviewed specs: two independent readers per scenario (first edit,
   offline edit replayed, pairing, revocation with rotation, relay rollback, snapshot bootstrap), a
   third agent lists the differences, spec fixers resolve them; stop after two rounds without a
   difference or after four, recording what remains in `spec/CHANGES.md`. The lead must first write
   each scenario's fixed starting state so both readers begin alike.
7. **Requirement map** in `spec/README.md` (`briefs/reviews.md`, part D), then `scripts/spec-lint.sh`
   over it.
8. **Task 4**: skeleton crates from the specs' Public API sections, with the toy state machine and the
   fake sealer in `taskdeck-sync`.
9. **Stage exit**: an agent that did no work here lists what is missing; then remove the draft notice
   from `spec/README.md`, set this file to `final`, write "What the next stages need to know", open
   the exit pull request with what was built and measured, merge on green, and tell the owner to
   start wave 1 (`core`, `seal`, `relay`, `engine`, `android-shell`).

## Decisions (with reasons)

- **D1. Agent model.** `model: 'opus'` (Opus 5.5), as the owner's start prompt says.
- **D2. Pull requests.** Several, merged in order with merge commits, instead of one long-lived
  one: `day-planner` then always holds the stage's work, a new session starts from it, and the CI
  restructure can be proven by a real pull-request run before the exit.
- **D3. Cooldown without a new tool.** The crates.io sparse index publishes a `pubtime` for every
  version (checked 2026-11-21), so the seven-day cooldown is a script of our own over `Cargo.lock`.
  `cargo-cooldown` is not on §3.4's pre-approved list and is not needed.
- **D4. Toolchain pin: Rust 1.97.0.** It is the toolchain the cloud containers ship, so stages
  start without a download; today's code is tested on it here.
- **D5. Fixed-width integers in every wire and file layout are big-endian.** SYNC.md gives layouts
  without a byte order; HPKE (RFC 9180) and the relay's `DataView` default agree on network order.
- **D6. Generic traits live in the engine crate.** `taskdeck-sync` defines the state-machine and
  sealer traits plus the toy implementations; `taskdeck-device` adapts the real core and seal to
  them (§6.7 task 2). The engine then depends on neither and compiles for wasm32 on its own.
- **D7. tokio moves forward, to 1.53.3.** RUSTSEC-2026-0386 is patched in 1.51.6 (an older line) and
  from 1.53.3. A refresh does not downgrade, so the lowest patched release above today's 1.52.3 is
  taken. Cargo also re-pointed the `windows-sys` edge of six crates whose own versions did not move;
  those entries were put back, so the lockfile diff is the four crates and nothing else.
- **D8. `build.rs` names its inputs.** Printing `rerun-if-env-changed=SOURCE_DATE_EPOCH` switches
  off cargo's default (rerun when any package file changes). Instead of scanning the directory to
  imitate it, the script lists `build.rs`, `resources.rc`, `icon.ico`, `Cargo.toml`, `Cargo.lock`
  and `src`. `BUILD_DATE` is refreshed when the app's own inputs change, no longer when an unrelated
  file does. Short enough to review before signing a tag (SYNC.md §8), and later directories such
  as `fixtures/` and `android/` do not trigger rebuilds of the app.
- **D9. The release-panic test is a probe binary.** `cargo test --release` always unwinds, so it
  proves nothing about the profile. `crates/panic-probe` is built with `cargo build --release` and
  must catch a panic; `scripts/check-release-panic.sh` runs it. Proven to fail under `abort`.
- **D10. The fingerprint word list is BIP-39 English** (2,048 words), kept as
  `docs/sync/spec/wordlist.txt`. The lead fetched the official file and compared SHA-256
  `2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda`.
- **D11. Today's binaries get a panic hook that aborts, in release builds.** Attack round 5 showed
  what `panic = "unwind"` costs today's app: a panic on a background thread no longer ends the
  process, so the server's systemd unit (`Restart=always`) no longer recovers it. The plan requires
  the unwinding profile and also "every merge keeps today's app working", so `main.rs` and
  `server_main.rs` install a hook that prints as before and then aborts, only in builds without
  debug assertions (debug builds unwound before too). `desktop` removes or narrows it when the new
  runtime's `catch_unwind` guards need unwinding inside the app. See "Deviations".
- **D12. Findings against the two check scripts count only if CI or the documented local use would
  be misled.** Rounds 2 to 5 kept finding exotic local setups (a target directory shared between
  checkouts, hostile environment variables); the scripts grew with each. The property they guard is
  what matters.
- **D13. Usage-aware workflows.** The account's five-hour limit cut one long workflow in the
  middle (15:15 UTC, 2026-11-21): 18 agents had finished and were committed, 109 calls failed. Since
  then: workflows of about 15 to 20 agent runs, routine agents (attackers, fixers, implementers) at
  effort `high`, spec drafters at the session's effort, an attack loop that stops at once if an
  agent fails (so a failed agent never counts as a clean result), and no more than about 80% of a
  window's budget planned.
- **D14. The checks of task 1a guard a closed list of mutations.** After the hook landed, eleven attack
  rounds found nothing wrong with the product changes, and one small gap after another in how many
  hypothetical edits the two check scripts and the hook's test would notice; three fixes of the hook's
  test in a row each opened a new gap. The lead fixed the list: the profile's panic setting; the probe
  panicking; `build.rs` using local time, accepting one of eight named kinds of invalid
  `SOURCE_DATE_EPOCH`, or dropping or narrowing a listed rerun input; a `main()` not starting with the
  guarded hook call or naming the hook twice; the hook not printing as the default hook does, or not
  aborting. The hook's test reads the two main files only and runs in the debug profile. Anything else
  is left to review.
- **D15. Two tracks, one agent each.** A workflow's own limit is two agents, and one workflow holding
  both the spec work and the code work could not be steered: its second queue had to wait for the
  lead's decision on task 1b's advisories while the first ran for hours. Two workflows, each
  strictly sequential, keep the plan's "two agents at a time" and let the lead restart either track
  alone. Only the code track compiles, so no two agents ever build at once.
- **D16. One resolver per pair, and the lead rules before a spec is edited.** The first review
  alone (seal with relay) brought 16 findings, so nine bring about 140: too many for one resolver,
  and grouping by spec would give seal's resolver a third of them. Each pair's findings go to one
  resolver; the nine run one after another, each reading what the earlier ones settled and building
  on it. The workflow stops after the resolvers; the lead reads every "For the lead" list, writes
  its rulings beside them (`r1-lead.md`), and only then starts one fixer per spec.
- **D17. The start prompt of the second session names the agents' model as "opus 5.5".** The
  workflows keep `model: 'opus'`, which is that model (D1): the session's first agent reported
  `claude-opus-5-5`.
- **D18. Writers work on clones, the lead integrates.** The first implementer of the session wrote
  into the main checkout, so the stop hook met untracked, unfinished files as soon as the lead
  paused. Committing them would have pushed work nobody had read to a public repository; hiding
  them from the hook would have defeated it. So the code track and the spec track each write in a
  clone under `target/work/`, and only what the lead has read and verified enters the main
  checkout. It also keeps the two tracks, and the lead's own gate runs, out of each other's way.
- **D19. The lockfile advisories outside the four refreshed crates** (the written reasons that step 3
  of "Next steps" asked for; SYNC_PLAN.md §6.1 names four crates and forbids a blanket update, and
  also requires cargo-deny and cargo-audit, which cannot pass on today's lockfile). A crate with a
  fixed release moves by one targeted `cargo update -p … --precise` to the lowest fixed release at
  least seven days old; a crate with none gets a reviewed ignore; egui, wgpu and winit do not move.
  - Moves: `crossbeam-epoch` to 0.9.20 (RUSTSEC-2026-0204, an invalid pointer dereference, in both
    binaries); `rustix` 1.1.4 to 1.1.6 (RUSTSEC-2026-0486, unsound); `webbrowser` to 1.2.2
    (RUSTSEC-2026-0257, argument injection, desktop binary); `clipboard-win` to 5.4.2 (the locked
    version is yanked); `wayland-scanner` to 0.31.11 with `quick-xml` 0.41.0 (RUSTSEC-2026-0194 and
    -0195; a build-time generator on the Linux desktop, so it cannot change the look on Windows or
    macOS; a reviewed ignore is the fall-back if the Linux desktop job fails); `quinn-proto` to
    0.11.19, `quinn-udp` to 0.5.16 and `chacha20` to 0.10.2 (eight advisories and a yanked version in
    entries that no target compiles, so the moves change no built code).
  - Ignores, none with a fixed release: `paste` (RUSTSEC-2024-0436, a build-time macro under the AVIF
    encoder, which `desktop` removes); `proc-macro-error` and `structopt` (RUSTSEC-2024-0370,
    RUSTSEC-2022-0104: the command-line helper `kmeans_colors` enables by default and the app never
    calls; turning that feature off is a change to today's app, left to `desktop`); `ttf-parser`
    (RUSTSEC-2026-0192, Wayland title-bar fonts on the Linux desktop).
  - Also: duplicate versions outside the cryptographic list are allowed without a warning (41 of
    them, from the pinned window toolkit, were 2,650 of the gate's 3,500 output lines); cargo-audit
    fails on unmaintained, unsound and yanked, not on notices, so the two tools agree.
- **D20. Cheaper workflows (the owner's instruction, 2026-11-22 00:00 UTC).** The second session's first
  two workflows used 5.9 and 3.0 million tokens and brought the account's five-hour window to 92% in
  under three hours. The owner asked for no launch near the limit (a workflow cut by it wastes its
  unfinished agents) and for more economical designs. From here on: every agent gets an explicit
  effort (medium for agents that apply, check or attack by a closed list, high for resolvers,
  implementers and attackers of new code; nothing inherits the session's effort); reading lists are
  short (the rules an agent needs are in its prompt, and it reads only the sections it touches,
  never a whole plan, brief or spec unless reviewing that text is the task); an attack round is one
  fresh agent that covers the task's three lenses, not three agents and an inbox reader (SYNC_PLAN.md
  §7 allows adapting the loop while its properties hold: the writer does not judge its own work, a
  finding needs a reproduction, a failed agent is never a clean result, convergence is reported);
  lead findings go into the task's brief, which the next attacker reads; a fix-up is not followed by
  a second automatic check. The lead pushes once per task and reads CI when it needs it. Before a
  launch the lead considers where the usage window stands; the owner sees the meter, the lead does
  not.
- **D22. A usage budget per window (the owner again, 2026-11-22 07:20 UTC: the limit was hit).** The
  second window ended with both workflows cut: 2.7, 2.9 and 2.6 million tokens of agents plus the
  lead's own turns filled it. A five-hour window holds about 8 to 9 million agent tokens. From here
  on the lead plans at most about 6 million per window, adds up each workflow's reported tokens,
  and launches nothing that would not fit. A reader in the two-readers test costs about 440,000
  tokens because it reads nearly all of the specs, whatever its scenarios: rounds 2 to 4 therefore
  use two readers for all six scenarios, as SYNC_PLAN.md §6.1 words it, instead of six. The lead
  integrates once per finished workflow, not after every fix, to spend fewer turns of its own.
- **D23. Counting usage, and fewer messages per agent (2026-11-22 08:15 UTC).** The number a workflow
  reports is input plus cache writes plus output; `tools/tokens.py` adds the same up from the agents'
  transcripts while a workflow runs, so the lead knows where a window stands before it launches
  (this window at 08:15: 3.0 million). The transcripts also show what the reported number hides:
  each message an agent sends re-reads its whole context, and round 1's readers each re-read about
  10 million tokens over some 40 messages. So from round 2 a reader issues the reads of all the
  specs as parallel calls in its first message and writes each file in one call, and every agent is
  told to batch its look-ups. A workflow whose cost is uncertain is launched in steps (`until`): it
  returns after a phase, the lead looks at the window, and a resume replays the finished agents
  from the cache. Round 2's readers come first (about 1.3 million); the comparers and fixers (about
  1.2 million) follow in the same window only if it then stands under about 5.5 million.
- **D21. The spec resolutions are kept in the repository** (`docs/sync/status/foundation/resolutions/`):
  nine files with 232 exact changes and the lead's rulings on them are eighteen agent runs of work
  and existed only in the session's scratch space.

## Rulings for the specs

SYNC.md leaves these open and more than one spec touches each, so the lead fixed them before the
drafters started. They are inputs to the drafts; the pairwise reviews and the two-readers test
may still overturn one, and then the change is recorded here and in `spec/CHANGES.md`.

- **S1. Byte order.** Big-endian everywhere (D5).
- **S2. Genesis.** The log's first entry (seq 1) is a control entry holding a `Genesis` item,
  authored and signed by the recovery identity (SYNC.md §9 step 6, V1-5, V1-13). It carries the
  salt and the recovery public keys (so `board_id` certifies itself), the first device, epoch 1
  with its wraps and commitment, rules version 1, and the hash of the genesis state plus, for an
  imported board, the id of the genesis snapshot blob. `seal.md` owns the layout, `engine.md` the
  bootstrap from it, `core.md` the genesis `BoardData`, `relay.md` the create route that stores it.
- **S3. The recovery identity as an author.** It has a `dev` id of its own, derived from its
  signing key, and its own counter; the relay lists it with role `recovery`. It holds no token.
  The relay accepts a frame it authored from any connection that passed the Worker gate, because
  the frame's signature is what authenticates it.
- **S4. The Worker gate and the door.** SYNC_PLAN.md §6.4 requires unauthenticated requests to be
  refused before the Durable Object runs, yet a new device in pairing, a device logging in on a
  re-homed relay and the recovery tool hold no token. So every request must carry one of: a token
  whose HMAC verifies (any generation; the object then checks state and generation); the create
  secret, for `create` only; or the board's **door**, for the pre-token routes only (pairing as
  the new device, login, recovery-signed reads and admin operations). The door is
  `HMAC-SHA256(TOKEN_KEY, "taskdeck/v1/door\0" ‖ board_id)[0..5]`: stateless for the Worker,
  fixed for the life of a deployment, and not a secret that protects data. The **relay address**
  a person types or pastes is the host plus Crockford base32 of `board_id ‖ door` (21 bytes, 34
  characters). It is what the pairing sheet shows beside the code, what a Fence names, and what the
  recovery sheet prints as "relay host and board id". A leaked door lets a stranger reach the
  object's pre-token routes and nothing else: the same exposure as a revoked device's stale token.
- **S5. A control entry holds an ordered list of control items**, applied all or nothing (revoke
  and rotate travel in one entry, SYNC.md §5.4).
- **S6. Time windows are decided from the log.** Every validity rule that involves time (72 hours
  for a recovery-key change, 30 days for a stale-epoch entry, 30 days for an active member) is
  computed from `at_ms` fields of sequenced entries, never from the relay's clock or the applying
  device's, so all devices decide alike. Local clocks decide only when a device offers or takes
  an action.
- **S7. State hash.** `SHA-256("taskdeck/v1/state\0" ‖ u32 hash_version ‖ SHA-256(canonical board)
  ‖ SHA-256(canonical membership))`. `core.md` defines the canonical board bytes, `seal.md` the
  canonical membership bytes (device set, epochs, per-device counters), `engine.md` the
  composition and when it is taken.
- **S8. No always-on device.** Any current, unhalted device, the phone included, can take every
  duty (snapshots, acknowledgements, compaction, beats, restore tail, upgrade proposals). Backups
  are written by whichever desktop is awake. A week with only the phone awake loses nothing and
  blocks nothing except backups.
- **S9. Push or poll is a capability the relay states.** The Worker pushes over a socket; the
  standby answers plain HTTPS and clients poll `read`. Routes and bodies are otherwise the same.
- **S10. Each spec ends with a Public API section**, and the skeleton crates are written from
  those. `engine.md` owns the state-machine and sealer traits (D6).

Rulings on what the seal draft raised (2026-11-21):

- **S11. The pairing bundle does not repeat the device set and the recovery public keys.** They
  travel in the fresh snapshot's canonical membership bytes, which the bundle authenticates by the
  snapshot's hash. Slot D stays under 16 KB.
- **S12. Every control payload carries an inner signature over its items.** A backup reader can then
  verify the membership history back to the board id from signatures alone, after a purge has
  deleted old epoch roots. It costs 64 bytes per control entry and one context string (`ctl`).
- **S13. A purge is signalled by `purge_before` on `EpochRotated`.** `engine.md` and `relay.md` adopt it.
- **S14. Revoking the last current device is refused, from the recovery identity too.** A board with
  no device cannot be read by anyone but the recovery key; a total compromise is answered by
  restoring to a new board and fencing the old one.
- **S15 (see the settled form below). Skip must classify the same on every device.** Whether a device still holds the target
  frame must not change the verdict. The specs must define a floor computed from the log below which
  a Skip target is refused everywhere, keep every frame above that floor fetchable from the relay
  (compaction respects it), and have a device that lacks the frame fetch it. Open until the
  seal–engine review settles the exact rule.

Rulings on what the core draft raised (2026-11-21):

- **S16. Stamps are per field group.** Five per item (name, time, levels, estimate, repeat) and one
  per block, instead of SYNC.md's one per item, so unrelated concurrent edits raise no false
  "changed since you last synced" notice.
- **S17. The notes base is a revision counter** (`notes_rev`), not a hash. It refuses a stale edit
  just the same, works for two queued edits from one device, and keeps hashing out of the core.
- **S18. Rules version 1 has caps SYNC.md does not name:** 10,000 live items, 50,000 live blocks,
  1,000 blocks per item, 200,000 archive rows, 60 MiB of board bytes, so a snapshot always fits its
  64 MiB limit. The import check reports a board that exceeds one. Told to the owner at exit.
- **S19. Reminder lead times are parameters of `reminders()`.** The defaults the spec names (events
  10 minutes before, blocks and due times on time, routines off) are what `android` starts with and
  may expose as a device setting. SYNC.md's open question 11 stays with the owner, without blocking.
- **S20. The legacy import stops on an unreadable primary file** (items, notes or subscriptions)
  instead of importing a partial board; unreadable archive lines are carried as opaque bytes.
- The snapshot JSON keeps today's names and shape. `core.md` §14.3 lists every difference: the
  inputs that were process globals, deterministic tie-breaks, block ids in place of indexes,
  canonical UTC text, and one new field, `notes_base`.

Rulings on what the relay draft raised (2026-11-21):

- **S21. The recovery tool may hold a read-only token it minted itself.** A recovery-signed
  `mint_read_token` gives the recovery tool a backup-read token for topping up a restore when the
  token inside the backup has been revoked by a rotation. "No relay token" (SYNC.md §5.5, ruling S3)
  means the sheet carries none and the recovery identity never appends by token.
- **S22. The create secret gates the create family**: `create`, and seeding a moved board on a new
  relay. A new relay has no door for the board until the seed has run.
- **S23. The relay counts appends, the client counts requests.** The 3,000-appends budget lives in
  the author's row and costs no extra write; the 5,000-requests budget is kept by the client, since
  counting requests at the relay would write a row per request. A member that floods the relay is
  a compromised device and is answered by revoking it.
- For the seal–relay review to settle: narrow the relay's share of the Genesis checks to what it can
  verify without keys; cap a host at 255 bytes so it fits its one-byte length; record that an
  identical repeat of a pairing slot post is answered again rather than refused.

Rulings on what the engine draft raised (2026-11-21):

- **S15, settled. The Skip floor is `F(s) = max(purge_seq, s − 16,384)`.** A Skip at seq `s` may
  target only an entry at or above that floor, on every device alike, and compaction keeps the newest
  16,384 seqs, so the target frame is always fetchable. `seal.md` §13.6 takes the precondition.
- **S24, reworded after the reviews (lead ruling L1). The recovery-signed `reinstate` operation
  carries the roots the reinstated device missed.** They cannot travel inside the reinstating entry:
  that entry is sealed under an epoch the revoked device does not hold. The recovery identity holds
  a wrap of every epoch root, so it wraps to the device every root from the epoch its revocation set
  up to the epoch the reinstating entry is sealed under (at most 256); the relay keeps them for that
  device and returns them at its next login; the device checks each against the commitments in the
  log, oldest first. It can then read and apply what was sealed while it was out, and need not pair
  again (SYNC.md §5.5: the recovery key overrules a hostile revocation).
- **S25. Snapshots keep SYNC.md §4.7's two triggers:** the newest acknowledged snapshot is a week old
  by the log clock, or 1,000 entries behind the head. To keep an import from producing a burst, a
  device writes at most one snapshot per 24 hours of log clock. (The draft had dropped the entry
  trigger.)
- A determinism mismatch is a "divergence stop" for the minority and for both sides of a tie: stop
  sending, keep applying, show both fingerprints. It is not a fourth halt (SYNC.md §4.2 names three).
- "Purge history before now" reaches as far as the most lagging current device's last reported head,
  so no device is stranded; `desktop` shows the seq it will reach before the owner confirms.
- A device away more than 365 days rejoins only after the user confirms, because rejoining replaces
  its local log.
- `taskdeck-sync` needs `sha2` and `zeroize` at run time (state hash, identity export); the
  workspace task declares them.

Rulings on what the device draft raised (2026-11-21):

- **S26. `unsafe` is allowed in platform FFI, and only there.** SYNC_PLAN.md §3.4 says "no `unsafe`"
  in its cryptography rule. DPAPI and a write-through rename on Windows exist only as `unsafe`
  functions of the pre-approved `windows` crate, and the JNI entry points need it too. So: none in
  core, seal, sync, backup or recover (`#![forbid(unsafe_code)]`); in `taskdeck-device` one module
  per operating system, in `taskdeck-android` the JNI entry points, each block with its safety
  argument written beside it. Told to the owner at exit.
- **S27. Any desktop can write backups, however it joined.** A paired device lacks the control
  entries below its bootstrap seq, so by the draft it could not write a verifiable backup, and a
  later-paired Mac would not be the second writer SYNC.md §6.6 expects. The snapshot a new device
  bootstraps from therefore carries the control history back to Genesis (it is small). The
  seal–engine and device reviews apply it.
- **S28. A backup's file name carries a writer tag** (`TaskDeck-<date>-<seq>-<tag>.tdbk`), so two
  desktops writing into one synced folder never overwrite each other. A small change to SYNC.md §6.6.
- The recovery tool learns the current read epoch from the relay's answer instead of guessing it;
  the relay–device review applies it. Local-only mode mints ids with device number 1, and the import
  accepts them; the core–device review applies it. A desktop on a polling relay asks every 30
  seconds, about 2,880 requests a day, inside its budget.
- Three review pairs are added to the plan's six, because the device draft asks for changes in all
  three: core–device, seal–device and relay–device.

Rulings on what the bridge draft raised (2026-11-21):

- **S29. The phone keeps its own picture.** SYNC.md §3 gives the phone "its own Look". The
  device–bridge review adds the two facade calls and the per-device file kind it needs.
- **S30. Every call on the phone that needs device authentication needs API 30 or later.**
  BIOMETRIC_STRONG without a PIN fallback is available through the framework only from API 30, and
  `androidx.biometric` is not a pre-approved dependency. Below API 30 the phone shows and the desktop
  acts, which is what SYNC_PLAN.md §2 already says for adding, revoking and replacing devices.
- **S31. The app declares `ACCESS_NETWORK_STATE`** besides the permissions of SYNC.md §2.1: a job with
  a network constraint cannot be scheduled without it. It is a normal permission with no prompt.
  Calendar access is gone with the export. Told to the owner at exit.
- The page's caches go in `android`, as SYNC_PLAN.md §6.10 says, so the first frame waits for one
  bridge round trip; the cold-start budget checks that. The minimum WebView version (120) and bundled
  roots on API 26 to 33 are the spec's defaults for `android-shell` to confirm (§6.6 task 7).
- **S32. `taskdeck-android` has a default backend** (2026-11-22, found while writing the skeleton brief).
  bridge.md §11 makes a build with neither `backend-echo` nor `backend-device` a `compile_error!`. Then
  no workspace-wide cargo command works without feature arguments, and the CI contract of
  `scripts/ci/stage.sh` (`cargo test -p <package>`) fails for that crate. Ruling: the crate declares
  `default = ["backend-device"]`; both or neither stays a `compile_error!`; the echo flavour builds
  with `--no-default-features --features backend-echo`. A build that forgets its flags gets the real
  backend, never the echo one. bridge.md is changed to match in the next spec pass (follow-up 9).

## Deviations from the plan

- **`foundation` edits three files under `src/`**, which SYNC_PLAN.md §4 gives to `desktop`:
  `src/main.rs`, `src/server_main.rs` and a small new module for the panic hook (D11). Without it
  the plan's own `panic = "unwind"` would break "every merge keeps today's app working" for the
  server. Nothing else in `src/` is touched. `desktop` should note it in its status file when it
  starts.

## Measurements

| What | Value | Where |
|---|---|---|
| Container | 4 CPUs, 15 GB RAM, 60 GB free, Rust 1.97.0, Node 22.23.5, Java 21, Gradle | this session |
| Today's headless tests in the container | 264 passed, 1 ignored (`codec_bake_off`, already ignored); 5 min 53 s cold, about 3 min of it tests | `cargo test --locked --no-default-features --lib` |
| Release build of `taskdeck-server` with `panic = "unwind"` and fat LTO | builds in 4 min 3 s; 9,240,728 bytes; `--version` prints the build date | container, commit `c7b3d07` |
| Full CI matrix with the pin, the refresh and the unwinding profile | green: Linux headless, desktop on Windows, macOS and Ubuntu, emulators at API 26 and 36 | pull-request run on `c7b3d07` |
| Today's tests after the panic hook | 265 passed, 1 ignored, on Linux, Windows and macOS | pull-request runs from `a813998` on |
| A full pull-request CI run | about 13 minutes | every run of the first session |
| The six spec drafts | 11,635 lines, 852 clauses (seal 155, core 93, relay 166, engine 182, device 146, bridge 110) | `docs/sync/spec/` |
| Agent runs in the first session | about 110 finished, of them 13 attack rounds on task 1a | workflow journals |
| Task 1a's gate in the second session's container, run by the lead | both check scripts pass, the server builds, 265 tests passed and 1 ignored in 157 s; a cold build of the test binary took 7 min 34 s | `target/lead`, 2026-11-21 21:36 UTC |
| A pairwise interface review | about 11 minutes; the first returned 16 findings (3 high, 6 medium, 7 low) and 17 points found sound | spec track, seal with relay |
| Spec workflow 1 (nine reviews, nine resolvers) | 18 agents, 5.9 million tokens, 2 h 19 min; a resolver took about 10 minutes and wrote 30 to 60 KB | 2026-11-21 21:28 to 2026-11-22 00:00 UTC |
| Code workflow 1 (task 1b, task 1a's loop of six rounds, task 1c) | 28 agents, 3.0 million tokens, 2 h 41 min | 2026-11-21 21:38 to 2026-11-22 00:20 UTC |
| Spec workflow 2 (rulings as text, ten fixer runs, six checks, two fix-ups) | 19 agents, 2.7 million tokens, 54 minutes: about 140,000 tokens an agent against 330,000 in workflow 1, with short reading lists and medium effort (D20) | 2026-11-22 04:23 to 05:17 UTC |
| Code workflow 2 (rulings for 1b and 1c, six attack rounds on 1b, five on 1c) | 23 agents finished, 2 cut by the usage limit; 2.9 million tokens; about 125,000 tokens an agent | 2026-11-22 04:23 to 06:27 UTC |
| Two-readers round 1, first part (follow-ups resolver, five readers) | 6 agents, 2.6 million tokens: about 440,000 a reader | 2026-11-22 05:21 to about 06:45 UTC |
| Two-readers round 1, second part (one reader, three comparers, six fixers) | 10 agents, 1.7 million tokens; the whole round: 16 agents, 4.4 million tokens | 2026-11-22 07:22 to 08:05 UTC |
| Two-readers round 2 (two readers for all six scenarios, three comparers, six fixers) | 11 agents, 2.0 million tokens (readers 1.06 million, the rest 0.90 million), 38 minutes of agent time; round 1 took 4.4 million | 2026-11-22 08:15 to 08:55 UTC |
| Code workflow 3 (ruling 11, two attack rounds on 1b, six on 1c with six fixes, the workspace implementation) | 16 agents, 2.4 million tokens, 1 h 44 min; the workspace implementer alone 270,000 | 2026-11-22 07:22 to 09:06 UTC |
| The third usage window (07:20 to 12:20 UTC) | about 6.4 million agent tokens: two-readers round 1's second part 1.7, round 2 2.0, code workflow 3 2.4, and the lead's own turns; no cut | D22, D23 |
| A pull-request CI run with the seven workspace crates | 19.5 minutes (13 before): the stage jobs build the device crate's TLS stack on three systems without a cache | run 124 on `08dfc65` |
| The first real run of the new CI (pull request, every area) | 13 minutes; all stage jobs and the toolchain script work on Windows, macOS and Linux runners; `gate` is named `gate` only on the pull-request run and failed because two jobs failed; one real defect found: `scripts/check-build-date.sh` cannot see `build.rs` rerun when the workflow's `CARGO_TERM_COLOR: always` colours cargo's output (a lead finding for task 1c's loop) | run 44976738183 on `3d6ff82`, 2026-11-22 02:35 UTC |

## Facts found along the way

- Versions named in SYNC.md §2.3 and §5.1 all exist on crates.io. Youngest on 2026-11-21:
  `zeroize` 1.9.2 (2026-11-17, inside the cooldown until 2026-11-24). `Cargo.lock` has 1.9.0 today.
- `origin/main` (the older build, commit `03554d6`) is fetchable: `git fetch origin main`.
- `Cargo.lock` before the refresh: rustls 0.23.43, rustls-webpki 0.103.14, webpki-roots 1.0.9,
  tokio 1.52.3. After it: 0.23.46, 0.103.17, 1.0.12, 1.53.3.
- Under `panic = "abort"` a panic on any background thread killed the app at once. With `unwind`
  only that thread dies (weather, calendars, the phone workers, the sync sender or listener) and
  the process carries on without it; a panic on the main thread exits with 101 after running
  destructors. `src/` has no `catch_unwind`, no panic hook and no `lock().unwrap()` outside tests.
  Debug builds always behaved this way. D11 restores the old behaviour for release builds.
- CI accepts `rust-toolchain.toml` as it is: every job passed on the pinned toolchain with the old
  toolchain step (`rustup default stable`) still in place. Task 1c replaces that step.
- The root manifest is a workspace root with `exclude = ["target"]`. A Cargo package anywhere else
  in the checkout must be a workspace member, be listed in `exclude`, or carry its own `[workspace]`
  table (as `cargo fuzz init` writes). A tree unpacked under `target/` builds on its own.

- Agents left to harden a check script will keep finding one more hypothetical edit it misses, and
  each fix grows the script. Name the mutations a check must catch before the loop starts.
- A spec drafter cut off late leaves a nearly complete file; a finishing pass that runs the closing
  checks is far cheaper than a redraft (this is how `relay.md` was completed).
- A commit message that quotes the marker `[ci full]`, even inside a sentence, makes that push's run
  wide. It happened on `bef6b87` (harmless: it gave the proof on three systems). Write "the full-CI
  marker" in a message that only talks about it.
- `cargo build --workspace` on Windows would write `taskdeck.pdb` (the Android library's name) and
  `TaskDeck.pdb` (the root binary) into one directory. The default build is the root package alone,
  and CI never builds both in one job.


## Open questions for the owner

None. Nothing so far needed the owner under SYNC_PLAN.md §3.5.

## To tell the owner at exit

Decisions the lead took that the owner may want to overrule:

- `foundation` touched three files under `src/` for the panic hook (D11, "Deviations").
- `unsafe` is allowed in platform FFI only (S26).
- Rules version 1 has size caps SYNC.md does not name (S18).
- The Android app declares `ACCESS_NETWORK_STATE` (S31); device-authenticated actions on the phone
  need API 30 (S30).
- Reminder defaults are a starting point; SYNC.md's open question 11 is still the owner's (S19).
- A backup's file name carries a writer tag (S28).
- `zeroize` stays at the lockfile's 1.9.0 until 1.9.2 (named by SYNC.md §5.1, published 2026-11-17)
  is seven days old, on 2026-11-24; then one `cargo update -p zeroize --precise 1.9.2`.
- Eight more crates moved in the lockfile than the four the plan names, each for an advisory or a
  yanked version, and `deny.toml` carries four more reviewed ignores than SYNC.md's one (D19).
- cargo-deny is pinned at 0.20.2: the two newer releases were inside the seven-day rule on
  2026-11-21. cargo-audit's release publishes no checksum, so its pinned hash is the download's.
- From the spec reviews (lead rulings L1 to L19 in `resolutions/r1-lead.md`): a seeded relay may
  hold more than SYNC.md §6.7's 30 days of tail, so that a Skip target stays fetchable (L2);
  local-only mode refuses a folder over a cap of rules version 1 and names the ids (L15, next to
  S18); a host never carries `:443` (L17); links slower than 64 KiB/s cannot upload a 1 MiB part
  within the relay's deadline, which the rehearsal should measure (L19).
- An attack round is now one agent covering three lenses instead of three agents (D20).

## What the next stages need to know

To be written at exit.
