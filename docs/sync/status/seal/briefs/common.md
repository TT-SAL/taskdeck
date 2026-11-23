# Stage `seal`: rules for every agent

You work for the lead of stage `seal` (SYNC_PLAN.md §6.3) on the crate `taskdeck-seal`, the
cryptographic core of TaskDeck sync. Your task brief says what to do; this file holds what is the same
for every task. `W` is the working copy, `/home/user/taskdeck/target/work/seal`.

## Where you work

- Work only in `W`. Start every shell command with `cd /home/user/taskdeck/target/work/seal &&`, and
  give Read, Edit and Write absolute paths under `W`. Never touch `/home/user/taskdeck` outside `W`.
- Never run a git command that changes state (add, commit, stash, checkout, reset, restore, clean,
  pull). `git status` and `git diff` are fine. The lead commits.
- You are root: never give `/dev/null` as an output file to a compiler or linker.
- You may change `crates/taskdeck-seal/**` and `docs/sync/vectors/**`, nothing else: not `Cargo.lock`,
  not another `Cargo.toml`, not `scripts/`, not `docs/sync/spec/`. If the task seems to need a change
  elsewhere (a dependency, a wrong spec, a wrong gate), do not make it: report it.
- Cargo: always `--locked`, and your own target directory, `CARGO_TARGET_DIR=target/<your label>`
  (relative to `W`). No new dependency: use what `crates/taskdeck-seal/Cargo.toml` declares.
  Integration tests may use the crate's normal dependencies directly.

## Reading

- The contract is `docs/sync/spec/seal.md`. Never read a spec in full. Read the line ranges your brief
  names, as one batch of parallel Read calls in your first message. To find a clause, grep
  `W/target/clause-index.txt` (every clause of the six specs, one line each; grep it, never read it).
- Conventions (`docs/sync/spec/README.md` §5): every fixed-width integer is big-endian; a context
  string is `taskdeck/v1/<use>` followed by one `0x00` byte; hex is lowercase; Ed25519 is verified
  strictly.
- Keep your message count low: batch look-ups, write a file in one call, run the gate when a piece is
  complete rather than after every edit.

## Engineering rules (SYNC_PLAN.md §3.3 and §3.4, seal.md §3)

- The public API is seal.md §26 and is already in the skeleton: a name or signature changes only with
  the spec. Private items are yours to design.
- Compose the named primitives exactly as the spec says, by calling the pinned crates. No hand-written
  primitive. No `unsafe` (`#![forbid(unsafe_code)]` stays).
- Randomness enters only through `Entropy`. Secrets (the seed, private keys, roots, derived keys,
  pairing secrets, plaintexts that hold them) live in `Zeroizing` or `ZeroizeOnDrop` types that have no
  `Debug`, `Display`, `Clone` or `Serialize`.
- Compare in constant time (`subtle::ConstantTimeEq`) every MAC, commitment, SAS, code, ticket proof
  and any other value derived from a secret.
- Check every length against its limit (seal.md §23) and against the bytes remaining before anything
  is allocated or read. Parse nothing before it is authenticated, except the fixed header fields the
  check itself needs. A parser returns an error and never panics: no indexing, `unwrap` or `expect` on
  values that come from input, no arithmetic that can overflow.
- Ed25519 verification is `verify_strict`. An all-zero X25519 output is a failure.
- The crate reads no clock, file, network, environment variable or thread-local state. (Tests may read
  files under the crate and under `docs/sync/vectors/`.)
- Leave no `unimplemented!()` or `todo!()` in what your task covers, and write no `#[ignore]`. Never
  weaken, skip or delete a test to pass; a test goes only together with the code it tests.
- If the spec is silent, contradicts itself or cannot be implemented as written, do not improvise
  quietly. Take the reading the surrounding sections support best (when in doubt, the one that
  refuses), mark the place with a comment `// SPEC-QUESTION: ...`, and list it in your report with the
  section and both readings. The lead rules, and changes the spec where needed.
- A clause that states a duty of the crate's caller (the device's sealer adapter) which no function of
  this crate can violate gets no hollow test: list it in your report.

## Tests

- Every test cites the clauses it proves: `// spec: SEAL-XXX-NNN` in a comment on or directly above
  the test, one marker per id. A citation claims the test fails when the clause is violated.
- Unit tests of private items go in `src/<module>/tests.rs`, declared in `src/<module>.rs` as
  `#[cfg(test)] mod tests;`.
- Integration tests: one directory per target, `tests/<target>/main.rs`, with one module file per task
  beside it (`tests/<target>/<task>_<topic>.rs`, for example `tests/tamper/s1_wraps.rs`). The gate
  names five targets, and they must pass WITHOUT cargo features:
  - `kat`: the known-answer suites;
  - `tamper`: flipping any byte of any frame, wrap, bundle or file is rejected;
  - `contexts`: nothing made for one use verifies or opens for another;
  - `pairing_mitm`: task S5's;
  - `parsers`: property tests (proptest), parsers never panic.
  Shared helpers live in `tests/common/mod.rs`, included with `#[path = "../common/mod.rs"] mod common;`.
- `FixedEntropy` exists only with the cargo feature `test-entropy`. A test that needs it carries
  `#[cfg(feature = "test-entropy")]`; `crates/taskdeck-seal/ci.sh` runs the suite with the feature. A
  test in the five gate targets that needs no fixed entropy uses `OsEntropy`.
- Published vectors (`docs/sync/vectors/<family>.json`; seal.md §24, README §7) are written by code,
  never by hand. The target `vectors` (`tests/vectors/main.rs`, feature `test-entropy`, one module per
  family) builds each family from fixed inputs through the public API, compares it byte for byte with
  the committed file, and writes the file instead when the environment variable `SEAL_VECTORS=write`
  is set. Each case has a `"name"`, every input (entropy as `"entropy"`, the bytes in draw order) and
  every expected output, so that another implementation can replay it; an invalid case carries the
  expected error code or receipt class. Byte strings are lowercase hex.
  `bash scripts/gate-lib.sh vectors docs/sync/vectors` checks the format.
- The task gate, which must exit 0 before you report (with your `CARGO_TARGET_DIR`, from `W`):
  `cargo test --locked -p taskdeck-seal && cargo clippy --locked -p taskdeck-seal --all-targets -- -D warnings && bash crates/taskdeck-seal/ci.sh`

## Reviewers (verify and security passes)

- You did not write the task. Never modify a tracked file.
- Reproduce every finding. Write a scratch integration test as an untracked file
  `crates/taskdeck-seal/tests/zz_<your label>_<n>.rs`, run exactly that target
  (`cargo test --locked -p taskdeck-seal --features test-entropy --test zz_<your label>_<n>`), and put
  the test's source and its failing output into the finding's `repro`. Delete your scratch files
  before you finish. A finding whose reproduction you did not see fail does not count; say so instead
  of reporting it. For a private item, give the unit test as text and the reasoning.
- Recompute published vector values independently where you can (Python's `hashlib` and `hmac`, or
  `node:crypto`), from the spec text and not from the crate's code.
- Severity. High: a rule of SYNC_PLAN.md §3.4 or a MUST of the spec is broken. Medium: the work does
  something other than its brief or the spec says, in ordinary use. Low: everything else, robustness
  outside ordinary use included.

## Reports

End with the structured report your prompt asks for. Say plainly what is not done, what you are
unsure of and what you did not run. The lead checks every claim that something passes.
