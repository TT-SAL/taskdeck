# Stage `core`: rules for every agent

You work on `crates/taskdeck-core/`, the pure board of the TaskDeck sync design. The contract is
`docs/sync/spec/core.md`. Your step's brief names what to build or check. These rules hold for every step.

## Where you work

- **Writers** (implementers, fixers) work only in the working copy `/home/user/taskdeck/target/work/core`
  (a git clone; called W below). One writer runs at a time, so W is yours while you run.
- **Checkers** (verifiers, security passes) never edit W. Make a frozen copy of the commit you are given
  and work there: `git -C /home/user/taskdeck/target/work/core worktree add --detach /home/user/taskdeck/target/work/check-<your label> <commit>`.
  Scratch tests go into that copy; you report them as text.
- Never write under `/home/user/taskdeck` outside `target/`. Never push, never touch a remote, never run
  `git config --global`, never give `/dev/null` to a compiler as an output file.
- Change only `crates/taskdeck-core/**`. A change you need anywhere else (a spec, `scripts/`, `fixtures/`,
  a manifest outside the crate, `Cargo.lock`) is not yours: describe it under `for_lead`.
- Writers end with `git -C <W> add -A crates/taskdeck-core && git -C <W> commit -q -m "<step id>: <what>"`
  and report the hash. Do not amend or rewrite earlier commits.

## What the crate may not do (the gate checks all of it)

- No clock, zone, file, network, thread, environment or random source, in **any** target of the crate:
  `crates/taskdeck-core/clippy.toml` is applied to tests, examples and benches too
  (`cargo clippy --all-targets -- -D warnings`). So tests cannot call `std::fs::*`, `std::env::*`,
  `Instant::now` and the like, and no file may `allow` or `expect` a `disallowed_*` lint (a scan fails the gate).
- Tests read fixtures and vectors with `include_str!` / `include_bytes!` and one plain string literal,
  relative to the test file (`include_str!("../../../fixtures/boards/small/notepad_text.json")`): no `env!`,
  no `concat!`, no backslash. CI's reads check follows these.
- A value a test needs "from outside" (a process zone) is `taskdeck_core::time::ChronoZone(chrono::Local)`
  written as that value expression, with no `use chrono::Local` and no type naming `Local`.
- No `unsafe`, no new dependency: `serde`, `serde_json`, `chrono` (no `clock`) for the library; `proptest`,
  `sha2` and chrono's `clock` for tests only. Need another? Stop and say so under `for_lead`.
- No `#[ignore]`, no `todo!()`, no `unimplemented!()` in anything you write. Never weaken, skip or delete a
  test to get green; delete one only together with the code it tests.
- `apply`, the canonical encoder and parser, `check`, the id functions, `upgrade_rules`, `reset`, `rebind`:
  no `f32`/`f64`, no `HashMap`/`HashSet` iteration (CORE-GEN-006). Errors, refusals, notices and reports
  never carry a name, notes text or a URL (CORE-GEN-010).

## The spec

- Do not read `core.md` in full. Read the line ranges your brief names (`Read` with `offset`/`limit`), and
  find a clause with `grep -n 'CORE-XXX' /home/user/taskdeck/target/work/core-index.txt` (one line per clause).
- Names and signatures are those of core.md §20 and of the skeleton files. Change none. Add derives,
  private helpers and `pub(crate)` items freely.
- Where the spec is wrong, silent or can be read two ways on something you need: take the reading closest
  to its text, write `// SPEC-QUESTION: <one line>` at the place, and list it under `for_lead` with the
  section, both readings and the one you took. Do not invent behaviour quietly and do not edit the spec.

## Tests and vectors

- Every MUST clause your brief lists gets a test, and the test file says so: a comment `// spec: CORE-XXX-NNN`
  next to the test (or the id in the test's name). Only these files count: `crates/taskdeck-core/tests/**`
  and `crates/taskdeck-core/src/**/tests.rs`. So unit tests of module `x` live in `src/x/tests.rs`, declared
  by `#[cfg(test)] mod tests;` at the end of `src/x.rs` (no inline `mod tests { }`, no `#[path]`).
- Vector families are JSON files under `crates/taskdeck-core/tests/vectors/` in the format of
  `docs/sync/spec/README.md` §7 and core.md §18.1: `{"spec": "core.md §N", "cases": [{"name": …, …}]}`;
  canonical bytes as a string field `…_text`, hashes as `…_sha256` (lowercase hex, computed by the test with
  `sha2`). A test reads each file and checks every case. Expected values in a vector come from the spec's
  text worked by hand or checked by hand; never paste an implementation's output without reading it.
- Property tests use `proptest` with its default case count.

## Building

- From W (or your frozen copy): `cargo test --locked -p taskdeck-core`, always with `-p taskdeck-core`
  (the root package is a large desktop app; never build the workspace). The first build downloads crates.
- The step gate (must exit 0 before a writer commits):
  `cargo test --locked -p taskdeck-core && cargo clippy --locked -p taskdeck-core --all-targets -- -D warnings && bash crates/taskdeck-core/ci.sh && bash scripts/gate.sh --only purity-clippy-toml core`
- Today's code under `src/` (the root package) is reference only: read it, copy from it, never edit it,
  never depend on it. Tests compare with the data under `fixtures/`, never with `src/`.

## Spending tokens

- Batch: issue the reads and greps you need as parallel calls in one message. Do not re-read a file you just
  wrote. Pipe long command output through `tail -n 40`. Write a file in one call where you can.
- Do the task in the brief; leave other modules' `unimplemented!()` bodies alone.

## Severity (checkers)

High: a MUST of the spec or a rule above is broken, or today's app stops working. Medium: the work does
something other than its brief or the spec says, in ordinary use. Low: everything else, robustness outside
ordinary use included. Report only what you reproduced, with the failing test or command as text.
