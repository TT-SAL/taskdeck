# Briefs of stage `engine`

One file per agent task. Rules that hold for every brief (the lead states them once here; each
brief repeats only what differs):

- **Who writes what.** The lead (this stage's session) writes the engine: `src/lib.rs`,
  `src/engine/**` and the unit tests beside it. Agents write what the engine is tested against and
  **do not read or edit `src/engine/**`**, nor the engine's part of `src/lib.rs`. They may read the
  public types (`src/model.rs`, `events.rs`, `records.rs`, `session.rs`, `receive.rs`,
  `snapshots.rs`, `ops.rs`, `ui.rs`) and work from `docs/sync/spec/engine.md`, `relay.md`,
  `seal.md`. Reason: the simulator is the stage's gate, and the author of the engine must not be
  the author of what judges it.
- **Never read a spec in full.** Build the clause index once, outside the repository:
  `python3 docs/sync/status/foundation/tools/clause-index.py docs/sync/spec /tmp/td-clauses.txt`
  (the second argument is the file it writes; never a path inside the repository), then read the
  sections the brief names.
- **Where to work.** In the working copy the brief names, never in `/home/user/taskdeck`. Every
  cargo command sets `CARGO_TARGET_DIR` to the directory the brief names. Change only the files the
  brief lists. Commit in the working copy on its branch; never push; never touch another branch.
  `git status` must be clean of stray files before the last commit (no tool caches, no scratch
  files, no index file).
- **No clock, no delay, no I/O in tests.** No `std::time`, no sleeping, no environment, no
  network, no thread. Everything is deterministic from seeds (ENG-GEN-003 holds for the crate's
  tests and the simulator too). The crate's `clippy.toml` enforces part of it:
  `cargo clippy --locked -p taskdeck-sync --all-targets -- -D warnings` must pass.
- **Never weaken a test.** No `#[ignore]`, no skipped case, no `unimplemented!()` or `todo!()` left
  in the files of the task.
- **Nothing real.** No secret, relay address, token, account name or text from a real board; names
  are made up ("Desk", "Laptop", "Phone").
- **Keep output short.** `cargo ... --message-format=short`; long output into a file under the
  target directory, and read its end.
