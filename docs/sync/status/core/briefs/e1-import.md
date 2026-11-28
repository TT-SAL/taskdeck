# Task E, step 1, `e1-import`: the legacy import

Plan: SYNC_PLAN.md §6.2 item 8: "old files in (including the palette and home place from `userconfig.toml`),
a report and a genesis `BoardData` out. The report contains counts, ids and error kinds only, never text
from the board." Tasks A to D are in the working copy.

## Fill `src/legacy.rs`

`LegacyFolder`, `LegacyConfig`, `ImportReport`, `Renumbered`, `IdList`, `ImportWarning`, `FailureKind`,
`ImportFailure`, `import`.

| Read | core.md lines |
|---|---|
| §15 the import: inputs, the fifteen steps, the report | 1114–1191 |
| §6 the records the genesis board is made of; §6.12 `check` | 201–342 |
| §7.1–§7.2 ids and marks | 362–373 |
| §16 import codes; §17 limits | 1265–1269; 1271–1314 |
| §18.2 row `import.json`; §18.7 the two folders the import refuses | 1343; 1393–1400 |
| §20 `legacy` | 1802–1812 |

Today's shapes and steps, under `/home/user/taskdeck/src/`: `tasks.rs` 160–240 (`Active`, `Session`,
`Recurrence` with their serde attributes), 618–632 (`migrate_legacy_plans`), 688–699 (`assign_missing_ids`);
`archive.rs` 105–160 (`Archived`), 683–729 (how lines are read); `subscriptions.rs` 137–147; `color.rs` 1–20
(`ColorScheme`); `board.rs` 697–711 (`adopt_temporaries`). The fixtures are described in
`/home/user/taskdeck/fixtures/README.md` (read "The boards" and "The older builds' formats").

## Points the lead has settled

1. Parse the legacy files with private `serde` structs that mirror today's (same field names, same defaults,
   unknown fields ignored, `inactivated` as the wire name of `archived_at`). A time is read as
   `chrono::DateTime<chrono::FixedOffset>` (RFC 3339 with any offset: what today's `DateTime<Local>` reads)
   and becomes the `Instant` it denotes, every fraction digit kept.
2. The fifteen steps run in the order written; the first failing step ends the import with its failure
   kind (CORE-IMP-001). Sizes are checked before anything is parsed.
3. Two fixture boards must be refused (§18.7): `fixtures/boards/dst-edges` with `instant_range` (ids 21, 22
   and one archive row) and `fixtures/boards/id-limits` with `id_range`. That is the import working.
4. The report holds counts, ids and kinds, never text (CORE-IMP-003): test it by importing `small` and
   searching the report's `Debug` text for every name, the notes and every URL of that folder.
5. `import` is a pure function of its bytes (CORE-IMP-002); no other function of the core normalises
   (CORE-IMP-008): `from_canonical_bytes` already refuses instead of repairing.
6. Create `tests/common/boards.rs`: a module that later test targets share (`mod common;` with
   `#![allow(dead_code)]` inside it), giving each fixture folder as a `LegacyFolder` by `include_bytes!` of its
   files, with the `LegacyConfig` its `userconfig.toml` holds written out as values (for `fixtures/boards/*`:
   `coordinates = [-41.25, -132.5]`, `selected_colorscheme_id = 100`; read each `legacy-main/` and
   `legacy-other/` folder's own file). Keep it small: one function per folder group, a table inside.

## Tests and vectors to write

- `tests/vectors/import.json` (§18.2): for each folder under `fixtures/boards`, `fixtures/legacy-main` and
  `fixtures/legacy-other`, the genesis `board_sha256` and the report, or the failure; and the cases given as
  bytes in the vector that the row names (device-minted ids, a duplicate device-minted item id, a temporary id
  next to them, the marks step 14 gives; `id_range` for a high part of 8192 and for `3 × 2^40`).
- A test target `tests/import.rs`: every case; the counts against `fixtures/legacy-main/MIGRATION.json` and
  `fixtures/legacy-other/MIGRATION.json` where both count the same thing (items, rows with id 0, unreadable
  lines, sessions made from `planned_start`, whether the palette and the home place were found); every
  accepted genesis board passes `check` and round-trips through its canonical bytes; the import of the same
  bytes twice gives equal bytes and reports.
- Property test: `import` returns and never panics on mutated copies of the `small` and `unreadable`
  folders' files (CORE-GEN-007).
- Clauses to cite: CORE-IMP-001 to -009, CORE-GEN-007, CORE-GEN-010.

## Done

The step gate of `common.md` exits 0 in W; no `unimplemented!()` is left in `src/legacy.rs`; you committed
in W. Report the commit, what you built, and every `for_lead` point (a fixture the spec's steps cannot
import as you expected is one).
