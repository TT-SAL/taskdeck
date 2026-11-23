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
   `docs/sync/status/seal/briefs/` (write the brief first where the table says "to write": the notes
   under the table say what it must hold). The workflow script every task uses is
   `docs/sync/status/seal/workflows/task.js`; "How this stage is run" says how it is launched.
4. Never run two sessions on this stage.

## How this stage is run

- One workflow per task, agents on `model: 'opus'` with effort `high`, set on every call. The task shape
  is SYNC_PLAN.md §7's, adapted (decision L2): implement, verify once, fix once, then the two security
  passes side by side with different lenses, then one fix pass for what both found. A second verify
  pass runs only after a high finding. Nothing runs a third time.
- A task's workflow is launched twice. First with `args.until = "verify"`: it returns after the
  implementer, the verifier and the verifier's fix. The lead reads the reports, rules on the spec
  questions, runs the gate, commits and pushes (so the work is safe and CI runs on three systems).
  Then the same script is resumed (`resumeFromRunId`, `until = "all"`, `base` = the commit before the
  task, `lead` = the lead's own rulings as findings): the finished agents replay from the cache, the
  two security passes run, and one fix pass takes their findings and the lead's rulings.
- Writers never touch the main checkout (foundation's D18). They work in the clone
  `target/work/seal` (git-ignored). When a workflow step has finished, the lead reads the diff there,
  runs the gate there, commits there, pulls that commit into the main checkout (`git pull --ff-only
  target/work/seal <branch>`; rebase the clone's commit first when the main checkout moved), pushes,
  and reads CI. Agents never commit and never run a git command that changes state.
- Agents read the line ranges of `seal.md` their brief names and grep the clause index
  (`target/work/seal/target/clause-index.txt`, written by the lead with
  `python3 docs/sync/status/foundation/tools/clause-index.py docs/sync/spec <out file>`); none reads a
  spec in full.
- The task gate every writer must pass, and the lead runs itself after each workflow step:
  `cargo test --locked -p taskdeck-seal && cargo clippy --locked -p taskdeck-seal --all-targets -- -D warnings && bash crates/taskdeck-seal/ci.sh`.
  The stage gate (`bash scripts/gate.sh seal`) can pass only after task S7.
- Usage: the owner allows this stage 3.5 million agent tokens per five-hour window (one other session
  shares the account's window). The lead counts each workflow twice, by its own report and from the
  agents' transcripts (`docs/sync/status/foundation/tools/tokens.py <journal.jsonl>`: input, cache
  writes and output), books the higher number ("Token ledger"), and starts nothing that would not fit.
- A push to the pull request cancels its running CI, so the lead pushes once per workflow step and
  keeps status edits for that push.

## Tasks

Sections and line ranges are those of `docs/sync/spec/seal.md` at the stage's start.

| # | Task | Modules (`crates/taskdeck-seal/src/`) | Spec | Vectors (`docs/sync/vectors/`) | State |
|---|---|---|---|---|---|
| S1 | Primitives, keys, wraps, chain and fingerprints, the sheet, the known-answer suites, the vectors target, `ci.sh` | `error`, `entropy`, `base32`, `encodings` (hosts, names), `ids`, `ctx`, `keys`, `chain`, `prim` | §3–§7, §15, §18 | `seal-kdf`, `seal-wrap`, `seal-chain`, `seal-sheet` | **done**, merged (TT-SAL/taskdeck#4; its high finding fixed by TT-SAL/taskdeck#6) |
| S2 | Frames, envelope, padding, payloads and control items (encode and strict parse, canonical JSON), login and admin signatures | `entries`, `signatures`, `encodings` (JSON) | §5.3, §8–§12, §16 | `seal-frame`, `seal-padding`, `seal-signatures` | **done**, merged (TT-SAL/taskdeck#9; low findings only) |
| S3 | Membership state machine and receipt checks: canonical bytes, every transition, classification, halts, Skip targets, races | `membership`, `receipt` | §13, §14 | `seal-membership`, `seal-receipt`, `seal-commit`, `seal-rotation` | **done**, merged (TT-SAL/taskdeck#10, with the spec change) |
| S4 | Sealed files; history records, the projection, backups and the backup signature | `file`, `backup` | §19, §21, §16.3 | `seal-file`, `seal-backup` | **done**, merged (TT-SAL/taskdeck#13, 2026-11-23 07:46 UTC; low findings only, four rulings carried into S5's brief) |
| S5 | Blobs and snapshots (opening a snapshot uses S4's projection); four rulings carried over from S4's security passes | `blob`, `backup`, `prim` | §20 | `seal-blob` | brief written; next to launch |
| S6 | Pairing: code, keys, slots, transcript and SAS, bundle, both machines; the relay-in-the-middle test | `pair` | §17 | `seal-pairing`, `seal-code` | brief written |
| S7 | Gate completion: tamper matrix and context confusion over everything, parser property tests, fuzz targets, the budget job, every clause cited by a test that proves it, the skeleton's `allow` removed, the low findings that are a few lines | tests, `fuzz/`, `examples/`, `ci.sh` | §3, §24 | all present | brief written (to revise with what S5 and S6 leave) |
| S8 | Hygiene pass, stage exit review, exit pull request | | | | planned |

The briefs of S4, S5 and S6 name line ranges of `seal.md` as it stood on 2026-11-23 02:30 UTC; after
a change to that file, check them against its headings before launching.

Notes behind the briefs (the lead's reading of the spec):

- **S4.** Read §6.6, §6.7, §11.3, §12.1, §13.2, §13.6 (the Author column, for the projection's reduced
  rules), §16.3, §19, §21, §22, §23, §24 and §26's file and backup parts; `engine.md` §18.3 for the
  state hash. The projection is a private function that S5's `open_snapshot` calls with the
  receiver's own board id. The reader's eleven steps and the projection's six each fail once in a test,
  in order, with every earlier step passing; forged (sealed by someone who knows the recovery public
  key, signed by a non-member), truncated and re-signed files are refused. Inflate under a hard cap
  while streaming; never allocate a claimed size. `write_backup` keeps nothing that opens the file
  (SEAL-BKP-007). Strict Ed25519 in the reader gets its SEAL-GEN-010 test here (a small-order key, a
  non-canonical `S`). Clauses: SEAL-FILE-001 to -004, SEAL-KEY-014, -015, SEAL-BKP-001 to -010,
  SEAL-SIG-006, SEAL-CTX-002 (the `backup` use), SEAL-CTX-003 (`file`; `wrap` against `backup-hpke`),
  SEAL-KEY-013 (roots in a backup against the commitments).
- **S5.** §20 in full, §10.2 (the padding function), §21.5 (history records), §26's blob part.
  `open_snapshot`'s signature has no expected state hash although §20.5 step 7 compares one: expect
  that as a spec question. Clauses: SEAL-BLOB-001 to -010, SEAL-CTX-003 (`blob`).
- **S6.** §17 in full, §5.2, §6.4, §20.5, §14 (N classifies the log from `h + 1`), §26's `pair` module.
  The relay-in-the-middle test: an attacker who knows the code and holds every slot substitutes keys;
  it gets through only when the digits the user types into N equal N's own, that is by guessing six
  digits. Clauses: SEAL-CODE-001, -002, SEAL-PAIR-001 to -017, SEAL-CTX-003 (`pair-bundle`),
  SEAL-KEY-015 (the bundle's token).
- **S7.** `scripts/gate.sh seal` green: `spec-coverage` at 164 of 164 with no hollow citation (the
  verifier of each task names them), `no-unimplemented`, `no-skipped-tests`; SEAL-GEN-001, -002, -003,
  -005, -011 for the whole crate; fuzz targets under `crates/taskdeck-seal/fuzz/` that compile on the
  pinned toolchain in `ci.sh` (Linux), with their own `[workspace]` and lockfile, covered by hand
  against the cooldown; the budget job (256 entries verified and opened within 100 ms, release build,
  median of five, baseline recorded here, 25% rule); the low findings listed below that are a few
  lines each (S1: the sheet lookup, SEAL-SHEET-003's citation; S2: the admin operations' signer and
  argument checks, the order of `sign_admin`'s checks, a beat ack of seq 0, `seal_frame` under epoch
  0, two hollow citations).

## Task S1: what happened

- Implementer: everything of the brief built, gate green. Verifier (who recomputed every published
  value independently in Python, with its own RFC 9180, and re-fetched every known-answer source):
  five low findings, none high or medium. Security passes: one high, one low. Fix pass: the lead's
  rulings applied; the high finding handed back because it needs `Cargo.lock`.
- **The high finding, fixed by TT-SAL/taskdeck#6 (merged 2026-11-23 01:35 UTC):** the SHA-256 and HMAC states that
  take in the seed, the epoch roots and the private keys are not wiped on drop, because `sha2` is
  built without its `zeroize` feature (SYNC_PLAN.md §3.4 "secrets zeroized"; SEAL-GEN-006 and
  SEAL-SHEET-003 in spirit). Fix: `sha2 = { workspace = true, features = ["zeroize"] }` in
  `crates/taskdeck-seal/Cargo.toml`; in `Cargo.lock` that adds the edge `zeroize` to `digest` 0.11.3
  and to `block-buffer` 0.12.1 and no package; the reproduction becomes a test in
  `src/prim/tests.rs` (`sha2::Sha256` and `sha2::Sha512` are `ZeroizeOnDrop`). `Cargo.lock` is a shared
  file (plan §3.2), so it went into a small pull request of its own; its CI ran every area, the
  desktop matrix and the supply-chain job included, green.
- CI on Windows failed on the first push: a checkout under `core.autocrlf` gave the embedded word
  list, the sources two tests scan and the published vectors CRLF line endings. Fixed with
  `.gitattributes` in the crate and in `docs/sync/vectors/` (`* -text`), reproduced and verified here
  with a clone under `core.autocrlf=true`.

## Task S2: what happened

- Implementer: everything of the brief built (canonical JSON in `src/canon.rs`, payloads and control
  items in `src/payloads.rs`, both private; the receive side as private functions, listed in the table
  at the top of `src/entries.rs`), gate green. Verifier (who recomputed the three vector families
  independently, with its own HKDF and HChaCha20): two low findings. Security passes: four low
  findings from the protocol lens, none from the parsing lens (it checked `padded_len` against a
  reference for every length to 300,000 and the builders' refusals one by one). No fix pass ran.
- What S3 inherits, by the implementer's design and the lead's ruling Q14: the step-15 parsers check
  what an item alone decides; the bounds relative to the entry's seq, the Genesis checks that need the
  board and "Genesis only at seq 1" are S3's, at step 16.

## Task S3: what happened

- Implementer (35 minutes, 0.89 million tokens, the largest of the stage): `membership.rs` with the
  canonical bytes and a strict parser that also checks the state's invariants, a private module
  `membership/transitions.rs` with one function per row of §13.6, `receipt.rs` with `classify` on S2's
  receive-side functions; 152 unit tests, a board simulator over the public API for the integration
  targets (`tests/common/board.rs`), four vector families (each case a full record of §14.1's inputs).
  Gate green in the lead's run. It raised fourteen spec questions (Q20 to Q33 below; the rulings are
  also at the end of its brief, for the passes that follow).
- Verify pass (second window): no high or medium finding. The verifier recomputed the four vector
  families independently, ran a history through every transition and the limit states to prove the
  parser accepts every state the transitions produce, and tested citations by mutation: two missing
  tests (the "keys still fresh" precondition of `confirm`; the item order of the Skip targets' second
  pass) and three citations that prove less than they claim.
- Security passes: no code defect of high or medium severity. The membership lens reproduced three
  weaknesses of the design itself (the code does what `seal.md` says): see "Open questions for the
  owner", O1. The parsing lens found that the membership parser accepted recovery records no log can
  produce (a `dev` that is not the `dev_R` of its key), and that one published vector was built on
  such a state. One code differed from a ruling's wording (a revocation of the last device beside a
  reinstatement that fails); the ruling was refined instead (Q25).
- Fix pass, one, with the lead's items: the 30-day condition before decryption (E3, below), the
  recovery records' invariants and the vector's fixture, the two missing tests, the citations.
- **A defect between two specs, found by this task (spec change E1, to raise):** a rotation that lost
  a race was sealed under the epoch the winner replaced, so `seal.md` §14.2's own step order refuses
  it at step 13 with `seal/stale_epoch`. `engine.md` §28.5 step 4 retries a raced revocation only when
  it is refused with `seal/ctl_epoch` or `seal/ctl_wraps` and calls any other refusal a failure: as
  written, a revocation that loses a rotation race would fail with the target already locked out at
  the relay. The change: §28.5 step 4 also names `seal/stale_epoch`; `seal.md` §13.8 and §22 say which
  code a lost race gets. It affects stage `engine` (not started) and nobody else.

- **E3, a second defect between two specs, older than this stage (found by the second check of the
  spec change):** step 9 of `seal.md` §14.2 accepted the previous epoch whenever it existed and tested
  the 30 days of §13.9 only at step 14, after step 10 had needed that epoch's root; `engine.md` §28.2
  has every device delete the roots below a purge's epoch after 30 days. A late frame under the purged
  epoch then stopped the receivers that had deleted the root (`seal/missing_root`, a local fault) and
  was refused by the others. Now the 30-day condition is part of step 9, before decryption
  (`seal/stale_epoch`, no root needed), and §28.2 deletes only when more than 30 days have passed.
  The code change is in S3's fix pass.
- **E4, left open for `engine`** (a row of `CHANGES.md`): `engine.md` §24.3 re-seals every later own
  record after a re-sealable refusal, a pending revocation among them, and §28.5 now composes a raced
  revocation again on the same codes; the two rules can meet on one record.

## Task S4: what happened

- Implementer (0.61 million tokens): `file.rs` (the storage key, the per-kind file keys, `FileSealer`,
  `open_file` with the tail rules of §19.4), `backup.rs` (history records, `write_backup`,
  `read_backup` with its eleven steps, `verify_backup_file`) and the private module
  `backup/projection.rs`, which runs the rows of §13.6 through S3's transitions and which S5's
  `open_snapshot` calls. Eleven spec questions (Q34 to Q44).
- Verify pass: no code defect. The verifier recomputed both vector families independently and ran an
  honest history through every transition. It then removed checks one at a time: four security checks
  could each be deleted without a test failing, among them the Genesis `board_id` comparison, the one
  check that stops a backup history forged by someone who is no member. The lead handed the four to
  the fix pass as test-only rulings; each now has a test that fails without the check (the fixer
  showed that by mutation), and the vector case "verify: header names another key" is now a file
  signed by the given key.
- Security passes: lows only; no fix pass was owed for them. The protocol lens recomputed the vectors
  with a hand-written HPKE and went through the binding of every field; it found three places where
  the reader accepts what no honest writer produces (rulings Q45 to Q47). Both lenses measured the
  projection: it copied the whole membership for every history record, so a hostile history at the
  limit (1,048,576 records, an 80 MB file, writable by any device that ever was a member) held
  `read_backup` for 516 seconds in a release build. S5's `open_snapshot` calls the same function, so
  the lead carried the fix into S5's brief with the two few-line rulings (Q45, Q46) and the
  compressor's state (Q48) instead of leaving them for S7.
- Nothing of the crate's code changed after the implementer; the lead's gate passed on the final tree
  (205 unit tests with `test-entropy`, 13 vector families).

## Decisions (with reasons)

- **L1. Tasks along the skeleton's modules**, in dependency order (table above). Membership and
  receipt are one task: `classify` is the only public way into the transitions. Files and backups
  (S4) come before blobs (S5) because opening a snapshot verifies its history with the backup
  reader's projection; seven implementation tasks instead of the six first planned keep each small
  enough to review.
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
  `SEAL_VECTORS=write` is set. The verifier of each task recomputes the values independently
  (Python's `hashlib`, `hmac` and `cryptography`, or `node:crypto`) from the spec text.
- **L9. The public API is seal.md §26 and nothing else, with or without a cargo feature.** The
  implementer of S1 had added `SymKey::expose_for_vectors` behind `test-entropy` to publish derived
  keys; removed. A value the API does not show is computed in the vectors target with the pinned
  crate and compared with the crate's private value by a unit test that reads the committed file.
- **L10. Usage is booked at the higher of two counts** (the workflow's report, the transcripts' sum).
  For S1 they were 1.02 and 1.24 million.
- **L11. The files of the crate and the vectors are `-text`** (`.gitattributes`), as foundation did
  for `fixtures/`: tests and the build compare their bytes.
- **L13. An implementer of more than about 1,500 lines costs about 0.9 million tokens**, twice the
  smaller tasks (S1 0.46, S2 0.51, S3 0.89): its context grows with every file it writes. The lead
  launches such a task with `until = "implement"`, watches the transcripts' count in the background,
  and books verify and security separately.
- **L14. An implemented task is pushed even before its verify pass** when the passes must wait for a
  usage window: the commit says "not yet verified", the pull request is a draft, and nothing merges
  before the passes.
- **L12. A hook step that needs `python3` runs on Linux only** (the vectors' format check in `ci.sh`):
  a Windows runner's bash may lack it, and the files are the same on every system.

## Rulings on spec questions

The implementers' questions and the lead's rulings. None changes a clause's text. The rulings of
tasks S1 to S3, the ownership of five caller clauses and the defect E1 were written into `seal.md`
and `engine.md` by the lead on 2026-11-23 02:30 UTC (sentences added or rewritten in place, so no
line moved; `scripts/spec-lint.sh` passes). They travel in the pull request of task S3. One agent that
did not write them then checked the spec diff against this table (0.27 million tokens) and found one
high, seven medium and five low points, all applied on 2026-11-23 05:56 UTC:

- E1 was incomplete: after two or more rotations the lost entry is refused with `seal/old_epoch`
  (step 9), which `engine.md` §28.5 step 4 now also names;
- `M_0`'s bytes keep `magic` and `version`; an empty `home_host` is valid before any Fence;
- SEAL-SIG-005 is owned by `engine` and `device` (the engine supplies the `register` arguments);
  SEAL-WRAP-003 and SEAL-SIG-002 gain `device` as an owner for the caller's half; `seal.md` §25 and
  `device.md` §24 and §6.4.7 tell `device` what it owns and that the identity bytes are 80 bytes;
- the catalogue rows of §22 name the new places that raise their codes; `Receipt.opened` is `Some`
  exactly when step 11 passed; `padded_len`'s overflow and `ctl_payload`'s codes are in §26.

Seal now owns 164 clauses: 163 of `seal.md` and ENG-HASH-001. Because the check found a high point, a
second check pass ran on the corrected change (0.26 million tokens): it went through the raced
revocation case by case and found E1 right and complete, no high point, five medium and six low:
E3 and E4 (under "Task S3"), SEAL-WRAP-003 also owned by `engine` (the engine halts on a wrap
delivered outside an entry), three more catalogue rows, the transport (not the sealer adapter)
carries the login's origin and the `register` arguments, `engine.md` §36 tells `engine` what it owns,
wording. All applied on 2026-11-23 06:13 UTC; nothing is checked a third time. Rulings of later tasks
go into a second spec change before the exit.

**E2, left open for `engine` and `device` (a row of `CHANGES.md`):** the check found the same kind of
defect as E1 for an own `Fence`: the relay fences itself in the same transaction that appends the
frame, so a `Fence` that loses a rotation race leaves the old relay fenced with no accepted `Fence`
(`engine.md` §28.3 and §28.7, `device.md` §15.8 step 5). The three places need one design by the
stages that build them; seal's lead recorded it and did not change them.

| # | Where | Question | Ruling |
|---|---|---|---|
| Q1 | §26 `base32::decode_typed` | no error code of its own in §22 | `seal/code_format` for every failure; callers map it (`from_sheet` to its step codes, `check_move_code` to `seal/move_code`). Spec note |
| Q2 | §26 `DeviceKeys::to_identity_bytes` | no spec gives the layout | version 1 is 80 bytes, the length is the version: `dev[16] ‖ Ed25519 private seed[32] ‖ X25519 private key[32]` (clamped, as RFC 9180 serialises it); another length or an unclamped key is `seal/file_corrupt`. Spec note; told to `device` |
| Q3 | §18.2 step 2 | which of length and character is checked first | length (`seal/sheet_length`), then characters (`seal/sheet_char`), as the step lists them |
| Q4 | §18.1 `to_sheet` | the separator of the returned text | the 14 groups joined by single spaces (69 characters); `from_sheet` reads it back. Spec note |
| Q5 | §15.4, §6.8 | does `move_code` return the grouped form | yes, `XXXX-XXXX`; `key_fingerprint` returns `XXXX-XXXX-XXXX-XXXX`. Spec note |
| Q6 | §7, SEAL-GEN-013 | codes of `wrap_root` and `unwrap_root` | `wrap_root` to a small-order key is `seal/dh_zero`; every failure of `unwrap_root` is `seal/commitment` (SEAL-WRAP-003) |
| Q7 | §26 `Recipient::Recovery` | its board differs from the `board` argument | refused (`seal/commitment`) |
| Q8 | §7 | the wrap's `recipient_id` is not covered by HPKE | `unwrap_root` requires it to equal the receiver's own id, so that every byte of a wrap is bound (the gate's tamper matrix) |
| Q9 | §24, §26 | `seal-kdf.json` publishes derived keys the API does not show | decision L9 |
| Q10 | README §5.5 | Wycheproof's "acceptable" Ed25519 cases | the 8 small-order public keys fail, the 21 mixed-order ones verify: that is `verify_strict`. The relay's verifier must give the same answers |

| Q11 | §10.2 `padded_len` | `U + m` overflows above 2^63 | saturates to `u64::MAX`, which is over the cap: refused, no panic |
| Q12 | §10.1, §11.2 | does `seal_frame` refuse an `at_ms` out of range and an empty `cmd` payload | yes, `seal/bad_plaintext`: every receiver would refuse the entry. Spec note (§22's "raised when") |
| Q13 | §26 `Signer::Recovery` | its board differs from the `board` argument | refused, `seal/unknown_author` |
| Q14 | §12.3, §12.5, §12.8, §12.9 against §13.6 | the tables bound a field by the entry's seq, and §13.6 states the same bounds with `seal/ctl_target`: which step and code | step 15 (`seal/bad_payload`) checks what the item alone decides, the lower bounds included; the bounds relative to the entry's seq are §13.6's, step 16, `seal/ctl_target`. Spec note |
| Q15 | §16.2, §22 | the code for an invalid admin argument | `admin_message` and `sign_admin` validate what the registry states; a host outside §5.4 is `seal/host_format`, anything else `seal/bad_payload`. Spec note |
| Q16 | §12.7 | the code of `ctl_payload` for an invalid item | `seal/host_format` for a Fence host, `seal/ctl_shape` for items that form no shape, `seal/bad_payload` otherwise |
| Q17 | §11.4, §11.5 | are `snap.epoch` 0 and `acks.entry` 0 allowed | `snap.epoch` is at least 1; `acks.entry` as the table says (a security pass notes seq 0 is never a snap entry: low, on S7's list) |
| Q18 | §16.1, §16.2 | the order of the nonce and the argument checks | the relay nonce's length first (`seal/challenge_length`), then the origin or the arguments; a refused `sign_login` draws nothing |
| Q19 | §26 | derives on the public data types | `Debug`, `Clone`, `PartialEq`, `Eq` on the non-secret ones; no name or signature changed |

| Q20 | §13.2, §13.3, §26 | what `M_0` is, and what the caller passes at seq 1 | the layout of §13.2 with every field zero and every list empty, the only state with epoch 0; at seq 1 `classify` works from the anchor and ignores the membership given. Spec note |
| Q21 | §13.8, §22 | the code of a rotation that lost a race | `seal/stale_epoch` (step 13), by the spec's own step order; `seal/ctl_epoch` only for a rotation sealed under the current epoch that names another epoch. Spec change E1 (`engine.md` §28.5) |
| Q22 | §12.1 | Genesis at another seq, or another shape at seq 1 | `seal/ctl_shape`; `seal/ctl_genesis` is for the item and anchor checks |
| Q23 | §14.3 | a rules version above 1 has no row | nothing is allowed under it: later frames are `seal/bad_version`; a `DeviceAdded` under it records no envelope |
| Q24 | §13.6 | which records "every recorded key" covers | device and recovery records, current or retired; not a pending proposal. Spec note |
| Q25 | §13.6 `DeviceRevoked` | when "at least one current device remains" is judged | over the whole entry, with its later revocations and reinstatements taken as applied; when none would be left the refusal is the first revocation's; a later item that then fails its own row refuses the entry with its own code (refined after the security pass) |
| Q26 | §13.5, §23 | `ctl_seqs` is full | `seal/ctl_cap`, after the items passed |
| Q27 | §14.8 | `chain_at` gives nothing at or above `retained_from` | the comparison is skipped, `based_on_unchecked` is set |
| Q28 | §14.2 | a root that does not match the recorded commitment | the local fault `seal/missing_root` |
| Q29 | §13.5 | the counter of a refused seq 1 | none: the state stays `M_0` |
| Q30 | §13.6, §14.6 | the author of a Skip target that is no well-formed frame | the 16 bytes at offset 20; none when the frame is shorter |
| Q31 | §14.6 | a missing target before a supplied frame with a wrong hash | `NeedFrames` first; the refusal follows when the frames are supplied |
| Q32 | §13.2 | does the parser check invariants beyond the layout | yes; every state the transitions produce must pass (the verifier checks the round trip). Spec note |
| Q33 | §26 `Receipt` | `code` and `opened` per class | with a halt, the halt's code; with `NeedFrames`, none; `opened` from step 11 on. Spec note |

| Q34 | §19.1 | a record over the limit | `seal_record` refuses with `seal/payload_too_large`, before it draws |
| Q35 | §19.4, §26 | a corrupt record and the records before it | `open_file` returns `seal/file_corrupt` alone |
| Q36 | §21.5 | `from_accepted` on what is no accepted control entry | `seal/frame_format`, `seal/bad_payload`, `seal/not_control` |
| Q37 | §21.3 | `write_backup` with inconsistent inputs | refused with the code of the reader's step that would reject the file |
| Q38 | §21.5, §21.6 | a segment's roots of epochs rotated inside it | a segment carries only roots whose commitment its membership records; the others are in the rotations' wraps to the recovery key. Spec note |
| Q39 | §21.6 step 11 | the segment's recomputed chain has nothing to be compared with | the crate recomputes it; the caller chains segments and nightly files. Spec note |
| Q40 | §21.6 step 11 | a `hash_version` the rules do not allow | `seal/backup_payload` |
| Q41 | §21.5 | history records beyond the file's range | after `seq`: step 9; at or after a segment's `first_seq`: step 11; a segment has `first_seq ≥ 2`. Spec note |
| Q42 | §21.6 projection | step 2 needs the Genesis key before steps 4 and 5 | the first record's items are parsed at step 2; step 6 binds the key to the board id |
| Q43 | §21.6 projection step 6 | how much of §13.6 the reduced rules run | the whole rows for `DeviceAdded`, `DeviceRevoked`, `DeviceReinstated`, `EpochRotated`; `RecoveryKeyChange` without the 72 hours; the others their author only |
| Q44 | §21.6 | where `verify_backup_file` compares the key | after steps 1 and 2, before the signature |

| Q45 | §21.6 step 10, SEAL-BKP-004 | step 10 compares `recipient` (a `dev_R`, which binds the recovery signing key only) with the projection's recovery identity: a history whose last recovery-key change pairs the sheet's signing key with another HPKE key passes, and a restore would wrap later roots to that key (S4 security, reproduced) | step 10 also requires both public keys of the projection's current recovery record to equal the typed seed's (`seal/backup_writer`). A strengthening. Spec change 2; code in S5's brief |
| Q46 | §21.6 projection, SEAL-MEM-004 | the projection accepted a first record whose `ctr` is not 1 | refused (`seal/backup_history`): the projection accepts no history a log cannot produce. Code in S5's brief |
| Q47 | §21.5, §21.6 | nothing in a file says at which seq its membership was taken, so a device revoked at seq `k` can still write a file that reads as a backup at any head with its history cut before `k` (S4 security, reproduced) | by design, and no check inside a file can change it: an authentic backup written before the revocation must stay readable. A backup proves that its writer was a current device at the end of its history and nothing later. Spec change 2 says so in §21.6; `recovery` shows the writer, the date and the last control entry, prefers the newest file, and compares with the relay's copy when one is reachable (the records' `frame_hash`, `head_chain`) |
| Q48 | SEAL-GEN-006, §21.3 | the DEFLATE compressor and decompressor (a pinned dependency's private state) hold the payload's bytes, the epoch roots among them, and are freed without being erased (S4 security, a note; `unsafe` is forbidden, so the crate cannot wipe them from outside) | as S1's high finding ruled for the hash states: a state that took in roots must not be left behind. The writer keeps the roots and the read token out of the compressor (they travel in stored blocks the crate writes itself, after a full flush); the reader resets the decompressor, which zeroes its window in the pinned version, before dropping it. What stays outside reach is said in the code and in the spec. Code in S5's brief |

Applied with the same change (they were candidates until task S3 confirmed the pattern):

- Clauses that state a duty of the crate's caller (the device's sealer adapter) and are owned by
  `seal` alone: the crate's functions are pure and cannot violate them, so no test of this crate can
  cite them honestly. So far: SEAL-FRAME-005 (a frame for one counter is sealed at most once),
  SEAL-ENV-003 and SEAL-ENV-004 (which epoch the sealer chooses), SEAL-SIG-005 (the `register`
  arguments name the frame that carries the matching `DeviceAdded`), SEAL-MEM-016 (a refused
  rotation's root is discarded). `device` is now their owner; it affects stage `device` only, which
  has not started.
- `seal.md` §25 said core provides "a function that encodes a JSON value canonically"; seal has its own
  (L4), and §25 now says so.

## Measurements

| What | Value | Where |
|---|---|---|
| Container | 4 CPUs, 15 GB RAM, 60 GB free, Rust 1.97.0 | session 1 |
| `cargo check --locked -p taskdeck-seal --all-targets`, cold | 25 s | `target/lead`, 2026-11-23 00:10 UTC |
| Task S1, first step (implementer, verifier) | 31 minutes; 0.46 and 0.28 million tokens | 2026-11-23 00:14 to 00:45 UTC |
| Task S1, second step (two security passes, one fix) | 10 minutes; 0.20, 0.14 and 0.15 million tokens | 2026-11-23 00:50 to 01:00 UTC |
| The crate's tests after S1 | 38 unit tests, 25 in the four gate targets it fills, 4 vector families; with `test-entropy` 44 unit tests | the lead's run, 01:01 UTC |
| Task S2, first step (implementer, verifier) | 28 minutes; 0.51 and 0.29 million tokens | 2026-11-23 01:03 to 01:32 UTC |
| Task S2, second step (two security passes, no fix) | 6 minutes; 0.17 and 0.18 million tokens | 2026-11-23 01:34 to 01:41 UTC |
| The crate's tests after S2 | 85 unit tests, 30 in the four gate targets, 7 vector families | the lead's run, 01:34 UTC |
| A pull-request CI run of the seal area | 6 to 9 minutes; with a lockfile change (every area) 19 minutes | TT-SAL/taskdeck#4, TT-SAL/taskdeck#6 |
| Task S3, the implementer | 35 minutes; 0.89 million tokens | 2026-11-23 01:42 to 02:17 UTC |
| The crate's tests after S3 | 152 unit tests, 34 in the four gate targets, 11 vector families (the two largest 412 KB and 213 KB) | the lead's run, 02:19 UTC |

## Token ledger

Agent tokens per workflow, by its own report and from the transcripts (input, cache writes and
output); the higher is booked. Limit: 3.5 million per window.

| Window (Helsinki time) | Workflow | Agents | Report | Transcripts | Booked |
|---|---|---|---|---|---|
| to 2026-11-23 06:50 | S1, to the verify pass | 2 | 0.55 M | 0.74 M | 0.74 M |
| | S1, security passes and fix | 3 | 0.47 M | 0.49 M | 0.49 M |
| | S2, to the verify pass | 2 | 0.60 M | 0.80 M | 0.80 M |
| | S2, security passes | 2 | 0.39 M | 0.35 M | 0.39 M |
| | S3, the implementer | 1 | 0.45 M | 0.89 M | 0.89 M |
| | **window total** | | | | **3.32 M** |
| from 2026-11-23 about 07:40 (the owner: the limit was hit and has reset; the lead takes that as the window's start) | first check of the spec change | 1 | 0.22 M | 0.27 M | 0.27 M |
| | S3, the verifier | 1 | 0.32 M | 0.39 M | 0.39 M |
| | second check of the spec change | 1 | 0.20 M | 0.26 M | 0.26 M |
| | S3, security passes | 2 | 0.45 M | 0.44 M | 0.45 M |
| | S3, the fix pass | 1 | 0.22 M | 0.25 M | 0.25 M |
| | S4, the implementer | 1 | 0.44 M | 0.61 M | 0.61 M |
| | S4, the verifier | 1 | 0.27 M | 0.33 M | 0.33 M |
| | S4, security passes and the fix pass | 3 | 0.55 M | 0.70 M | 0.70 M |
| | **window total** (nothing more is started in it: S5's implementer would not fit. The session's worker restarted at 05:41 UTC, when the owner wrote that the limit had reset, so this window ends at 10:41 UTC, 12:41 Helsinki, at the latest; S5 starts after that) | | | | **3.26 M** |

## Low findings left as they are

- **S1, security.** Decoding a typed sheet looks each character up with a scan that stops at the
  match (`base32::value_of`), so the time to decode depends on the seed's characters. Not a comparison
  in SEAL-GEN-007's sense, and the sheet is typed on the device that then holds the seed. A few lines
  (a lookup without a branch on the secret): on S7's list.
- **S1, verify.** The only test citing SEAL-SHEET-003 checks the types of `to_sheet` and `from_sheet`,
  not their buffers; that every buffer of §18 is zeroizing was confirmed by reading. On S7's list (a
  citation that proves the clause, or a note that it is proven by review).
- **S1, verify.** SEAL-GEN-012 and SEAL-KEY-007 are cited from S2 on (they are about frames).
- **S1, reviewers' notes, not findings.** Copies a move leaves on the stack (`StaticSecret::from(*bytes)`)
  cannot be observed or wiped without `unsafe`. `SymKey` does not carry its kind, so the type system
  does not stop a caller from using a snapshot key for entries; §26 fixes that API. `base32::encode`
  and `decode_typed` are public and return plain buffers; the crate's secret paths use zeroizing
  variants.

- **S2, verify and security.** `sign_admin` and `admin_message` sign operations the registry's own
  words exclude: `mint_read_token` for a `dev` that is not the signer, `reinstate` by a device or with
  `first_epoch` 1, `recovery_register` with a `dev_R` not derived from its `sign_pk`; `seed` allows
  65,535 entries where §23 allows 320 records. The relay refuses them all the same. On S7's list.
- **S2, security.** `sign_admin` reports `seal/unknown_author` before `seal/challenge_length` when both
  the recovery signer's board and the nonce are wrong (nothing is signed either way). A beat's ack may
  name seq 0. `seal_frame` seals under epoch 0, which every receiver refuses. On S7's list.
- **S2, verify.** Two citations claim more than their tests prove: SEAL-SIG-005 on a test of the
  `register` layout (a caller's clause, see the spec-change candidates), SEAL-PLAIN-001 on the test of
  the sealing-side `at_ms` refusal. On S7's list.

- **S4, security (both lenses).** The projection copied the membership once per history record
  (quadratic time; 516 s at the limit). Carried into S5's brief: a failing record ends the projection,
  so the items are applied in place.
- **S4, security, for S7's budget job.** `classify` returns a new membership for every entry, so each
  call copies the state, `ctl_seqs` included (8 bytes per control entry the board ever accepted, at
  most 8 MB at the limit of 1,048,576). An honest board has a few hundred control entries; a member
  who fills the log with control entries makes every later entry cost that copy on every device. S7
  measures it in the budget job (a board with 16 devices and 1,000 control entries) and records the
  number; a cheaper representation of `ctl_seqs` is a change to consider only if the budget needs it.
- **S4, reviewers' notes, not findings.** `EpochRoot::from_bytes` takes its 32 bytes by value (§26), so
  a caller's copy stays on the stack, as with every move of a secret. A segment's chain is recomputed
  and compared with nothing (Q39).

## Open questions for the owner

**O1. One compromised device defeats every "second device" rule and every time window of the
membership design** (found by task S3's security pass, 2026-11-23; each consequence reproduced by a
test against the crate; the code does what `seal.md` says, so this is the design's). Two facts:

- Any current device can add a device (`DeviceAdded`) with no other device taking part. A compromised
  device adds one of its own and then holds two members.
- Every time window is measured on `at_ms` values that the entries' authors set (ruling S6: so that
  all devices decide alike). Any member can date an entry forward, and the log clock follows.

What follows:

1. *The recovery key is replaced without the sheet, at once* (§13.11): the compromised device
   proposes, its own second device confirms with `at_ms` 72 hours ahead. V1-2's remedy, the 72-hour
   delay, gives the owner no real time. From then on the owner's sheet no longer overrules, and every
   later rotation wraps its root to the attacker's key.
2. *A rules version no build knows is activated* by the two-member override, or by the normal path
   after one forward-dated beat makes the honest devices look inactive (§13.10). Every honest device
   halts, and nothing sequenced afterwards is accepted by anyone, the recovery identity's entries
   included: the board cannot be repaired in band.
3. *The lost-sheet path is switched off for good* by one entry dated at the largest `at_ms`.

What still holds: every one of these entries raises a banner on every device; backups stay sealed to
the recovery key each desktop pinned (SEAL-BKP-006); every device keeps its local data; "Reset trust"
and a restore to a new board work. What is lost is the live board, and the claim of SYNC.md §7 that
one compromised device cannot take the recovery key.

Options, the owner's to choose (plan §3.5: the remedies remove a feature or weaken a claim):

- **A. The sheet is the second factor (the lead's recommendation).** The recovery key changes only
  with the current sheet (`direct`); `propose`, `confirm` and `cancel` go, and a lost sheet means
  "Reset trust" (a new board and a new sheet, devices pair again). A rules upgrade needs every
  current device to list the version (a device that is gone is revoked first), and the override is
  signed by the recovery identity instead of a second device. Reason: a device that can add devices
  can mint any quorum of devices, and no rule computed from the log can tell real time; the sheet is
  the one authority a compromised device cannot mint. Cost: two features of SYNC.md (§5.5 "the sheet
  is lost: new recovery key"; §4.5 the two-member override) change. Work: a small task in this crate
  (items, transitions, vectors) and a spec change that reaches `engine`, `device`, `desktop`,
  `recovery`.
- **B. Keep the design, state the limit.** SYNC.md §7 and the runbooks say that a compromised member
  can take the live board at once and that the remedy is "Reset trust". No code changes.
- **C. Keep both paths, add a detection.** Each device compares a `confirm` or an activation with its
  own clock and stops with an alarm when the 72 hours cannot have passed. It detects the forgery; it
  does not prevent it, and the stop is each device's own (not part of the replicated rules).

Nothing in tasks S4 to S7 depends on the answer; the crate implements `seal.md` as it stands until
the owner chooses.

## What the next stages need to know

To be completed at the exit. So far:

- L4: seal and core each have a canonical JSON encoder; `device` should compare them in a test.
- Q2: the identity bytes are 80 bytes, `dev ‖ Ed25519 seed ‖ X25519 private key`.
- Q10: `relay` must answer Wycheproof's 29 "acceptable" Ed25519 cases as `verify_strict` does (8 fail,
  21 verify); `crates/taskdeck-seal/tests/kat/data/ed25519_test.json` is the file.
- Q4, Q5: `to_sheet` returns the groups separated by spaces; `move_code` and `key_fingerprint` return
  the grouped text.
- A repository file that code compares byte for byte needs `-text` in a `.gitattributes`: Windows
  runners check text out with CRLF.
- For `engine`: spec change E1 (a raced rotation is refused with `seal/stale_epoch`).
- `core` changed `core.md` §4.5 (TT-SAL/taskdeck#7: a tree budget for the canonical parser, a lone
  surrogate escape is an error). Seal's own parser handles payloads of at most 257,979 bytes and
  refuses lone surrogates; task S7 checks it against §4.5 as it now stands.
- For `recovery` (Q47): `read_backup` proves that a file's writer was a current device at the last
  control entry of the file's history, and nothing later. A device revoked afterwards can still write
  files that read as older backups, with any board content. Show the writer, the written date and the
  seq of the last control entry; prefer the newest file; when a relay copy is reachable, compare the
  history records' `frame_hash` and the `head_chain` with it before adopting the state.
- For `recovery` (Q45): `read_backup` refuses with `seal/backup_writer` a file whose history ends with
  a recovery key that is not exactly the typed sheet's pair of public keys.
- For `recovery` and `device`: `read_backup` on a hostile file of the largest size allowed takes time
  linear in the history (one signature check per record, up to 1,048,576) and up to about 1.5 GB of
  memory (a 256 MiB file inflating to 512 MiB); run it off the UI thread.
