# W1b-spec: one spec change for everything workflow W1b found

As `w1a-spec.md` beside this file in every rule (verdicts **spec**, **none**, **crate**, **owner**;
the text where the rule lives; clause conventions; one `CHANGES.md` row; `spec-lint.sh`; one commit;
the answer's shape), with these differences. Task id `s2`.

- The findings are `fit.md` f3-1 to f3-13 (13 entries), written by the agent that built the runtime,
  pairing and `taskdeck-devicectl` (`crates/taskdeck-device/src/{runtime,create,pairing,ctl,strict_json}.rs`,
  `src/net/`), and the end-to-end tests (`tests/e2e/first_sync.rs`).
- Worktree: `git -C /home/user/taskdeck worktree add /home/user/wt/s2 -b spec/w1b
  claude/optimistic-lovelace-6z2f55`; branch `spec/w1b`; subject `spec: what the first sync end to
  end found (device W1b)`.
- Weigh especially:
  - **f3-1** (the manual clock of `taskdeck-devicectl` starts at 2026-01-05 while the standby runs
    on the real clock, so the engine's clock guard refuses composing until the test moves the
    clock): choose the rule for device.md §19.1 that removes the skew without a test having to
    move the clock first (for example: the manual clock starts at the host's clock when the process
    starts, or at a value `--clock manual:<ms>` gives; `set_ms` and `advance_ms` as now). Say in
    `proposed_fix` the small code change it needs in `src/ctl.rs` or the clock, which the next
    workflow makes.
  - **f3-9** (the six pairing-runtime points of the `CHANGES.md` row "Left open by stage `seal` for
    the pairing runtime"): write each decision into `device.md` §18.3 and §8.4 as the code does it,
    so that row is answered; name that row in yours.
  - **f3-8** (a write's reply is sent after the turn's publish): an addition to §4.3 step 4 and the
    text by DEV-THREAD-007; keep the clause's id, and check that nothing it promised is loosened.
  - **f3-3**, **f3-11**, **f3-12**: `Published::confirmed_state()` and `PairState`'s use and the
    `reason` member go into §25, §17.10, §18.3 and §19.2 as the code has them, unless the code is
    wrong against a MUST (then say so: a code fix, verdict **none** with the fix in
    `proposed_fix`).
  - **f3-6** lists shortcuts taken now (no socket, fixed retry, no budget, no `MissedRoots`, no blob
    resume, partial admin operations and devicectl): these are later work, not spec changes; verdict
    **none** unless the spec must say something now.
