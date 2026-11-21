# Brief: task 5, fixtures as data (stage foundation)

SYNC_PLAN.md §6.1 item 5: "A generator built on today's `Board` code, run with a pinned clock and `TZ`,
writes under `fixtures/`:
- made-up boards: small, 10,000 archive rows, daylight-saving edges, ids near every limit, unreadable
  archive lines;
- the same in the older `main` build's file formats (build that branch's code, or write them from
  DOCUMENTATION.md's tables and the legacy-row tests in `archive.rs`), with a `userconfig.toml`;
- for each board, the JSON today's `phone::snapshot` returns;
- transcripts: seeded command sequences with the state today's `Board` reaches after each command.
Later stages test against this data, never against the old code, so `desktop` can delete that code freely."

Who uses it: `core` (§6.2: "transcript test: replaying `fixtures/`' recorded sequences gives the recorded
states, for the commands whose meaning did not change"; "snapshot JSON equal to the fixtures'"; budgets
"with the 10,000-row fixture: 31-day snapshot within 40 ms, 5,000 entries applied within 1 s, checkpoint
loaded within 150 ms"; legacy import "old files in (including the palette and home place from
`userconfig.toml`)"); `android-shell` (§6.6: "the bridge answers from the fixtures"); `desktop` (§6.9: "the
import check passes on every legacy fixture and its report contains no board text"); `recovery` and
`android` (a 10,000-row archive). Read those sections, SYNC.md §9 step 4 (what import-check must detect) and
docs/sync/research/R1 §1, §2, §11 and V2's determinism audit before designing the boards: the fixtures
must contain the cases those documents say today's code gets wrong or the new code must handle.

Paths you may create or change: `fixtures/**` (data, `fixtures/README.md`, and the generator crate under
`fixtures/generator/`), the root `Cargo.toml` (only to add `fixtures/generator` to `[workspace] members`)
and `Cargo.lock` (only the new member's entry: the generator must add NO new third-party dependency; it
may depend on the root package by path with `default-features = false` and on crates the root package
already depends on), `scripts/regen-fixtures.sh`, `scripts/check-fixtures.sh`, `scripts/tests/**` (add
only). Do not edit `src/`: use the root library's public API (`task_deck::board`, `task_deck::phone`,
…). If something you need is not public, find another way through the public API (the same way the HTTP
server and the tests reach it); if there is none, stop and report exactly what is missing.

## Requirements
1. **Made-up data only.** Invented names and texts, obviously synthetic (no real people, places of
   residence, URLs that resolve to real calendars, or anything copied from a real board). Subscription URLs
   use the reserved domain `example.invalid` or `example.com` paths. Coordinates: a made-up home place.
2. **Deterministic.** A pinned clock (pass explicit `now` values everywhere today's API takes one; find
   every place today's code reads the system clock, zone or randomness on the paths you call, and either
   avoid it, pin it, or normalise its output, and document which) and a pinned `TZ` (the generator refuses
   to run unless `TZ` is the value it expects for the set it writes). Its own randomness is a small seeded
   generator written in the crate (for example splitmix64); no `rand`. Running it twice, on any machine,
   gives byte-identical files. `scripts/regen-fixtures.sh` regenerates everything; `scripts/check-fixtures.sh`
   regenerates into a temporary directory and fails on any difference from the committed files (this is a
   CI check).
3. **Boards** (each a directory holding exactly the files today's build keeps in its data directory, as
   today's code writes them, so a later stage can point an importer at the directory):
   - `small`: a few dozen items covering every kind of item and every field: tasks, events, routines, with
     and without due dates, estimates, blocks (several per item), severities and horizons; archive rows of
     every verdict; notes; subscriptions.
   - `archive-10k`: about 10,000 archive rows and a realistic live set; the budget fixture.
   - `dst-edges`: items, blocks, due dates and archive times on and around daylight-saving transitions
     (in the pinned zone, and instants that are transitions in `America/New_York`), around midnight, at year
     ends, and at the earliest and latest years today's code accepts.
   - `id-limits`: ids at and next to every boundary the design names (legacy ids near 2^40, the
     JavaScript-safe limit 2^53, the temporary range from 2^62 that `adopt_temporaries` folds), the largest
     counters, names and notes at their maximum lengths, the maximum number of subscriptions, and the
     duplicate-subscription-id case R1 §2 reproduced.
   - `unreadable`: an archive with lines today's code cannot read (invalid JSON, wrong shapes, invalid
     UTF-8, an empty line, an over-long line) kept exactly as bytes, among good lines; and a board with an
     id that is both live and archived (V2-11).
   - anything else the documents above call for (say why).
4. **The older `main` build's formats.** `origin/main` (already fetched in this checkout: do not run `git fetch`; `git show origin/main:<path>` and
   `git archive origin/main | tar -x -C <a scratch directory under /home/user/taskdeck/target/legacy-main>` are allowed)
   is the older build. Find out exactly which files it keeps and in what shapes, preferably by building
   its library in that scratch directory (the workspace excludes `target/`, so a tree unpacked there builds
   on its own; its own `Cargo.lock`, `--locked`, its own target directory) and driving its save functions, otherwise from its source, DOCUMENTATION.md's tables
   and the legacy-row tests in `src/archive.rs`. Write each board again in those formats, with a
   `userconfig.toml` that carries a palette and a home place (and the other keys that build writes), under
   `fixtures/legacy-main/`. Then prove today's code opens each legacy directory, and record what it
   migrates (counts only).
5. **Snapshot JSON.** For each board, the JSON `phone::snapshot` returns today, for a documented set of
   requests (single days including transition days, a 7-day and a 31-day window, the tray; fixed `now`
   values), with the process-global inputs (backdrop, look dials, palette, weather) pinned to documented
   values. One file per request, named by its parameters. Written under the pinned `TZ`, and a second
   time under `TZ=America/New_York` for a subset, so `core` can test zone handling.
6. **Transcripts.** Seeded sequences of today's wire commands (the JSON the phone page and the desktop
   send), covering every command and every refusal, starting from stated boards. For each step: the
   command, the `now` passed, the reply or error exactly as today's code returns it, and the board's full
   state after it (items, archive, notes, subscriptions, in today's serialisation, or a stated lossless
   form of it). Use JSON Lines. Generate each transcript under the pinned zone and under
   `America/New_York`. Keep them replayable by a program that has no access to today's code.
7. **Sizes.** The whole of `fixtures/` should stay under about 15 MB; say what each part weighs. Prefer
   more, smaller transcripts to one huge one. No compression: the data must stay diffable.
8. **`fixtures/README.md`**: the layout; the pinned clock and zones; how to regenerate and check; for each
   board what it is for and which stage uses it; the exact shape of the transcript and snapshot files;
   every normalisation applied; a statement that the data is invented. Plus `fixtures/MANIFEST.sha256`
   (one line per file) written by the generator.
9. **Tests** in the generator crate for its own seeded generator and file writers, and a
   `scripts/tests/test-*.sh` that runs `scripts/check-fixtures.sh`.

The build of the root library takes about 6 minutes cold. Use the default target directory for the generator (you are the only implementer compiling), and a separate target directory under `target/legacy-main/` for the older build.
Report: what you built, the size table, every place today's code reads ambient state and what you did
about it, how the legacy formats were established (built or derived, with evidence), and anything the
later stages should know about quirks in today's output.
