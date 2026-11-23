# Stage exit, `g-hygiene`: the hygiene pass

SYNC_PLAN.md §7, "Once per stage": one hygiene pass over everything stage `core` put into the repository.
You did no work in this stage. You change nothing; you report.

The stage's work is the difference between commit `545c5da` (the branch point) and the commit you are given,
under `crates/taskdeck-core/`, `docs/sync/status/core.md`, `docs/sync/status/core/` and `docs/sync/spec/`.
(`git diff --stat 545c5da <commit> -- <paths>`; other paths changed through merges of other stages' work and
are not yours to judge.)

Look for, in this order:

1. **Secrets and addresses** (SYNC_PLAN.md §3.2: the repository is public): a key, token, password, relay
   address, account name, e-mail address or host name of a real service, in code, vectors, tests or papers.
2. **Anything from a real board.** Every name, note and calendar address in vectors, golden logs and tests
   must be made up (`example.invalid`, `example.com`, invented names). The data under `fixtures/` is
   foundation's and is synthetic by its own README; text copied from it is fine.
3. **Stray files**: probe or scratch tests a checker left behind (names such as `probe`, `scratch`, `zz_`),
   files that test nothing, editor or build leftovers, `*.proptest-regressions` nobody meant to keep, files
   over a megabyte that are not vectors a test reads, a vector file no test reads.
4. **Dependencies** (SYNC_PLAN.md §3.4): the crate's `Cargo.toml` (`serde`, `serde_json`, `chrono` and
   nothing else outside the dev-dependencies; `proptest`, `sha2`, chrono's `clock` for tests); the fuzz
   package's manifest and lockfile if there is one (`libfuzzer-sys` is approved; name every crate its lockfile
   holds that is not in the root `Cargo.lock`, with its version, so the lead can judge them); no change to the
   root `Cargo.toml` or `Cargo.lock` by this stage.
5. **Paths**: a change of this stage outside the paths above (a script, a fixture, a file under `src/`).
6. **Leftovers in the code**: a `SPEC-QUESTION` comment, a `TODO`, a `dbg!`, a `println!` in the library, an
   `allow` attribute without a stated reason, a commented-out test.

Report each finding with its severity (common.md), and as its reproduction the file and line and the command
that shows it. Say under `checked` what you looked at and what you did not.
