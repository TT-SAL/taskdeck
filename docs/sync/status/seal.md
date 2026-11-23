# Stage `seal`: status

**State: in progress** (wave 1). Lead session 1 started 2026-11-23 00:01 UTC on branch
`claude/stoic-mayer-diyvrp`. This file is the stage's memory (SYNC_PLAN.md §3.5): a fresh session
resumes from it alone.

## Resuming: read this first

1. Read SYNC_PLAN.md in full, then this file to the end, then `docs/sync/status/seal/briefs/common.md`.
   `docs/sync/spec/seal.md` is the contract; R4 and V1 (`docs/sync/research/`) are the reasons behind it.
2. Work on the branch your session names, from `day-planner`. Everything this stage finished is merged
   there task by task (decision L6); what a session's running workflow had not finished starts again.
3. Continue at the first task in "Tasks" that is not `done`, with its brief in
   `docs/sync/status/seal/briefs/`. The workflow script every task uses is
   `docs/sync/status/seal/workflows/task.js`.
4. Never run two sessions on this stage.

## How this stage is run

- One workflow per task, agents on `model: 'opus'` with effort `high`, set on every call. The task shape
  is SYNC_PLAN.md §7's, adapted (decision L2): implement, verify once, fix once, then the two security
  passes side by side with different lenses, then one fix pass for what both found. A second verify
  pass runs only after a high finding. Nothing runs a third time.
- Writers never touch the main checkout (foundation's D18). They work in the clone
  `target/work/seal` (git-ignored). When a workflow has finished, the lead reads the diff there, runs
  the gate there, commits there, pulls that commit into the main checkout (`git pull --ff-only
  target/work/seal <branch>`), pushes, and reads CI. Agents never commit and never run a git command
  that changes state.
- Agents read the line ranges of `seal.md` their brief names and grep the clause index
  (`target/work/seal/target/clause-index.txt`, written by the lead with
  `python3 docs/sync/status/foundation/tools/clause-index.py docs/sync/spec <out file>`); none reads a
  spec in full.
- The task gate every writer must pass, and the lead runs itself after each workflow:
  `cargo test --locked -p taskdeck-seal && cargo clippy --locked -p taskdeck-seal --all-targets -- -D warnings && bash crates/taskdeck-seal/ci.sh`.
  The stage gate (`bash scripts/gate.sh seal`) can pass only after task S6.
- Usage: the owner allows this stage 3.5 million agent tokens per five-hour window (one other session
  shares the account's window). The lead adds up what each workflow reports ("Token ledger") and
  starts nothing that would not fit. A workflow is launched in steps (`args.until`) when its cost is
  uncertain, and resumed from its cache.

## Tasks

Sections and line ranges are those of `docs/sync/spec/seal.md` at the stage's start.

| # | Task | Modules (`crates/taskdeck-seal/src/`) | Spec | Vectors (`docs/sync/vectors/`) | State |
|---|---|---|---|---|---|
| S1 | Primitives, keys, wraps, chain and fingerprints, the sheet, the known-answer suites, the vectors target, `ci.sh` | `error`, `entropy`, `base32`, `encodings` (hosts, names), `ids`, `ctx`, `keys`, `chain` | §3–§7, §15, §18 | `seal-kdf`, `seal-wrap`, `seal-chain`, `seal-sheet` | in progress |
| S2 | Frames, envelope, padding, payloads and control items (encode and strict parse, canonical JSON), login, admin and backup signatures | `entries`, `signatures`, `encodings` (JSON) | §5.3, §8–§12, §16 | `seal-frame`, `seal-padding`, `seal-signatures` | planned |
| S3 | Membership state machine and receipt checks: canonical bytes, every transition, classification, halts, Skip targets, races | `membership`, `receipt` | §13, §14 | `seal-membership`, `seal-receipt`, `seal-commit`, `seal-rotation` | planned |
| S4 | Sealed files, blobs and snapshots, history records, the projection, backups | `file`, `blob`, `backup` | §19–§21 | `seal-file`, `seal-blob`, `seal-backup` | planned |
| S5 | Pairing: code, keys, slots, transcript and SAS, bundle, both machines; the relay-in-the-middle test | `pair` | §17 | `seal-pairing`, `seal-code` | planned |
| S6 | Gate completion: tamper matrix and context confusion over everything, parser property tests, fuzz targets, the budget job, every clause cited, the skeleton's `allow` removed | tests, `fuzz/`, `ci.sh` | §3, §24 | all present | planned |
| S7 | Hygiene pass, stage exit review, exit pull request | | | | planned |

## Decisions (with reasons)

- **L1. Six implementation tasks along the skeleton's modules**, in dependency order (table above).
  Membership and receipt are one task: `classify` is the only public way into the transitions.
- **L2. The two security passes run side by side, then one fix pass.** SYNC_PLAN.md §7 allows adapting
  the shape while its properties hold: the writer does not judge its own work, a finding needs a
  reproduction that is checked before anyone acts on it, a failed agent never counts as a clean
  result. The passes are two agents with different lenses (§6.3: protocol misuse and membership or
  rollback logic; parsing, resource exhaustion and pairing). It saves one fix agent per task and uses
  the container's second agent slot.
- **L3. Writers work in a clone; the lead integrates** (foundation's D18): nothing reaches the public
  repository before the lead has read it and run the gate.
- **L4. `taskdeck-seal` carries its own canonical JSON encoder** (private, over `serde_json::Value`:
  sorted keys, compact, exactly `core.md` §4.2). The workspace foundation built gives seal no
  dependency on core ("seal does not depend on core"), and core is being written at the same time.
  `device`, which links both, should compare the two encoders in a test. Noted for the next stages.
- **L5. Deterministic entropy stays behind the cargo feature `test-entropy`.** The five test targets
  the gate names pass without it (they use `OsEntropy`); tests that need `FixedEntropy`, the vectors
  target among them, run in `crates/taskdeck-seal/ci.sh` with the feature. A dev-dependency of the
  crate on itself with the feature was rejected: every test build would then carry the marker of
  SEAL-GEN-014.
- **L6. One pull request per finished task, merged when CI is green** (foundation's D2), so
  `day-planner` always holds the stage's work and a new session starts from it. The exit pull request
  is the one whose status says `final`.
- **L7. The word list is copied into the crate** (`crates/taskdeck-seal/src/wordlist.txt`), pinned by
  the SHA-256 test of SEAL-FP-002 and compared with `docs/sync/spec/wordlist.txt` in a test. An
  `include_str!` of the spec's file would be a read outside the crate's CI area and would need a
  change to `scripts/ci/changes.sh`.
- **L8. Vectors are written by code.** The test target `vectors` builds every family from fixed inputs
  through the public API, compares it with the committed file, and writes the file when
  `SEAL_VECTORS=write` is set. The verifier of each task recomputes a sample independently (Python's
  `hashlib` and `hmac`, or `node:crypto`) from the spec text.

## Spec questions and changes

None yet. Kept for the first spec-change pull request, if the tasks confirm them:

- Clauses that state a duty of the caller of the crate (the device's sealer adapter) and are owned by
  `seal` alone, such as SEAL-FRAME-005 (a frame for one counter is sealed at most once): the crate's
  functions are pure and cannot violate them. Candidate change: add `device` as an owner.
- `seal.md` §25 says core provides "a function that encodes a JSON value canonically"; seal has its own
  (L4). Candidate change: one sentence there.

## Measurements

| What | Value | Where |
|---|---|---|
| Container | 4 CPUs, 15 GB RAM, 60 GB free, Rust 1.97.0 | session 1 |
| `cargo check --locked -p taskdeck-seal --all-targets`, cold | 25 s | `target/lead`, 2026-11-23 00:10 UTC |

## Token ledger

Agent tokens as each workflow reports them (input, cache writes and output). Limit: 3.5 million per
window.

| Window (Helsinki time) | Workflow | Agents | Tokens |
|---|---|---|---|
| to 2026-11-23 06:50 | (none yet) | | |

## Low findings left as they are

None yet.

## Open questions for the owner

None.

## What the next stages need to know

To be written at the exit. So far: L4 (two canonical encoders, to be compared by `device`).
