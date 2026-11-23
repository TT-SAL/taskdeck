# Stage exit, `g-exit-review`: what is missing?

SYNC_PLAN.md §7, "Stage exit": one agent that did no work in the stage reads the status file, the diff and
the spec's clause index, and lists what is missing. You are that agent. You change nothing; you report. What
you find of high or medium severity is the stage's next work; the rest is listed.

Read, in this order:

1. `SYNC_PLAN.md` §6.2 (the stage: nine items to build, the gate), §3 (the rules), §7 (the shapes). Short.
2. `docs/sync/status/core.md` in full: it is the stage's memory.
3. The clause index of the spec: `python3 docs/sync/status/foundation/tools/clause-index.py docs/sync/spec /tmp/clauses.txt`
   writes one line per clause; the `core` clauses are under `# core.md`. Do not read `core.md` in full; read
   a section when a clause sends you there.
4. The stage's diff: `git diff --stat 545c5da <the commit you are given> -- crates/taskdeck-core docs/sync`.

Then answer, with evidence:

- **The nine items of §6.2**: is each one in the tree? Name the file that holds it.
- **The gate of §6.2**, item by item: run `bash scripts/gate.sh core` in your copy; every `run` item must pass.
  For each item, does the test do what the plan's sentence says (the determinism test really builds a second
  replica from a snapshot at a random point and runs under two `TZ` values; the transcript test really
  replays the recorded sequences and compares states; the budgets are measured as a median of five in CI)?
- **Clauses**: `bash scripts/spec-coverage.sh core` must pass. Then take every tenth `core` clause of the index
  and check that the test citing it would fail if the clause were broken (read the test; where in doubt,
  break the code in your copy and run it).
- **Rules of §3**: owned paths only; no skipped or ignored test; no `unimplemented!()`; nothing weakened; no
  new dependency; the crate compiles for `wasm32-unknown-unknown`; the status file records the tokens of each
  task, the decisions with reasons, deviations, measurements, the low findings, and what the next stages need
  to know.
- **The status file itself**: could a fresh session resume from it alone? Is anything in it stale or wrong
  against the tree?
- **What the next stages need** (`device`, `desktop`, `android`, `recovery` use this crate): is the public API
  the one of core.md §20? Name anything a user of the crate would trip over that the status file does not say.

Report each thing that is missing or wrong as a finding with its severity (common.md) and its evidence as
the reproduction (a command and its output, or a file and line). Say under `checked` what you covered and
what you did not.
