# Stage `foundation`: status

**State: in progress** (wave 0). Lead session started 2026-11-21 on branch `claude/focused-volta-tbqufb`.
This file is the stage's memory (SYNC_PLAN.md §3.5). A fresh session resumes from it alone:
read SYNC_PLAN.md, then this file, then continue at the first task below that is not `done`.

## How this stage is run

- Workflows of Opus agents (`model: 'opus'`), at most two agents at a time, one workflow at a time.
- Agents write; the lead runs every gate and reads CI itself. Agents never commit or push.
- Every implementation task goes through the task loop of SYNC_PLAN.md §7 (implement, attack through
  three lenses, fix, until two dry rounds). Spec work uses drafters, pairwise interface reviews and
  the two-readers test (§6.1).
- Pull requests go from `claude/focused-volta-tbqufb` into `day-planner`, merged with a merge commit
  so the branch stays valid for the next one. The exit pull request is the one that sets this file
  to `final`.

## Tasks

| # | Task (SYNC_PLAN.md §6.1) | State | Evidence |
|---|---|---|---|
| 0 | Status file, draft pull request | in progress | this file |
| 1a | Toolchain pin, lockfile refresh, `panic = "unwind"` with its release test, `build.rs` date | todo | |
| 1b | `deny.toml`, cargo-deny, cargo-audit, seven-day cooldown | todo | |
| 1c | CI: path filters at job level, aggregate `gate` job | todo | |
| 2 | Workspace: member crates with every planned dependency pinned; `relay/`, `android/` READMEs | todo | |
| 3a | Spec conventions (`docs/sync/spec/README.md`) | todo | |
| 3b | Spec drafts: core, seal, relay, engine, device, bridge | todo | |
| 3c | Pairwise interface reviews (seal–relay, seal–engine, core–engine, engine–relay, device–bridge, device–engine) | todo | |
| 3d | Two-readers test, six scenarios, until two rounds agree (at most four) | todo | |
| 3e | Requirement map in `spec/README.md` (SYNC.md §3–§9 to clause, stage or plan §2 row) | todo | |
| 4 | Skeleton crates: public types and traits, toy state machine, fake sealer | todo | |
| 5 | Fixtures as data: boards, legacy `main` formats, snapshot JSON, transcripts | todo | |
| 6 | Tooling: `scripts/gate.sh`, `scripts/spec-coverage.sh`, `scripts/dev-setup.sh` | todo | |
| 7 | Stage exit review, exit pull request, merge | todo | |

## Decisions (with reasons)

- **D1. Agent model.** `model: 'opus'` (Opus 5.5), as the owner's start prompt says.
- **D2. Pull requests.** Several, merged in order with merge commits (baseline first), instead of one
  long-lived one: the CI restructure is best proven by a real pull-request run early, and
  `day-planner` gains the `gate` check the owner may protect it with (§1.4).
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

## Deviations from the plan

None so far.

## Measurements

| What | Value | Where |
|---|---|---|
| Container | 4 CPUs, 15 GB RAM, 60 GB free, Rust 1.97.0, Node 22.23.5, Java 21, Gradle | this session |

## Facts found along the way

- Versions named in SYNC.md §2.3 and §5.1 all exist on crates.io. Youngest on 2026-11-21:
  `zeroize` 1.9.2 (2026-11-17, inside the cooldown until 2026-11-24). `Cargo.lock` has 1.9.0 today.
- `origin/main` (the older build, commit `03554d6`) is fetchable: `git fetch origin main`.
- `Cargo.lock` today: rustls 0.23.43, rustls-webpki 0.103.14, webpki-roots 1.0.9, tokio 1.52.3.

## Open questions for the owner

None so far.

## What the next stages need to know

To be written at exit.
