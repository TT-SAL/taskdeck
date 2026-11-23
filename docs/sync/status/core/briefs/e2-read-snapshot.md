# Task E, step 2, `e2-read`: read queries, snapshot builders, reminders

Plan: SYNC_PLAN.md §6.2 item 7 and the gate item "snapshot JSON equal to the fixtures'". The UI and the phone
page keep asking today's questions under today's names; the device's zone comes in as a parameter. Tasks A to
D and the import (step E1) are in the working copy.

## Fill

- `src/read.rs`: `ranked_ids`, `shuffle_seed`, `behind_minutes`, `next_free_start`, `archive_newest_first`,
  `archive_by_day` (`reflow_inputs` is there already).
- `src/snapshot.rs`: `SnapshotInput`, `Snapshot` and every type inside it, `Look`, `Dials`, `Labels::current`,
  `snapshot`, `answer_snapshot`, `build_day`, `build_tray`, `item_detail`, `done_rows`, `with_subscribed`.
- `src/reminders.rs`: `ReminderDefaults`, `ReminderKind`, `Reminder` (`to_canonical`), `reminders`.

| Read | core.md lines |
|---|---|
| §14.1–§14.2 the read queries and what changed against today | 991–1030 |
| §14.3 the snapshot JSON: the fourteen differences from today's | 1032–1060 |
| §14.4 the archive bucketed by day; §14.5 reminders | 1062–1098 |
| the clauses of §14 | 1104–1112 |
| §18.5 snapshot fixtures; §18.7 folders the import refuses | 1385–1387; 1393–1400 |
| §20 `read`, `snapshot`, `reminders` | 1754–1800 |

Today's code, under `/home/user/taskdeck/src/`: `board.rs` 854–920 (`shuffle_seed`, `ranked_ids`,
`next_free_start`, `behind_minutes`); `phone.rs` 193–272 (`answer_query`, `snapshot`), 1675–1715 (the look and
dials globals that become inputs), 2055–2100 (`Snapshot` and its types, with their serde attributes and
field order), 2335–2570 (`build_day`, `build_tray`, `item_detail`, `done_rows`, `with_subscribed`).
The recorded snapshots are described in `/home/user/taskdeck/fixtures/README.md` ("Snapshot files").

## Points the lead has settled

1. This is a port: today's algorithm, field for field, with exactly the fourteen changes of §14.3. Field
   names, serde attributes and field order of every snapshot type are today's (`notes_base` right after
   `notes`). Do not improve the output.
2. Every local date, weekday and minute comes from `SnapshotInput.zone` (CORE-READ-001). No query changes or
   reorders `BoardData` (CORE-READ-002, CORE-READ-005).
3. `archive_by_day` is one pass over the archive newest first (§14.4); `snapshot` uses it for the ghosts of
   all its days. The 31-day snapshot of the 10,000-row board has a budget of 40 ms as a median in CI
   (CORE-PERF-001): no per-day scan of the archive, no clone of the board.
4. The snapshot-fixtures test builds each `SnapshotInput` from `import` of the fixture folder
   (`tests/common/boards.rs`), with `overlay` read from the folder's `subscribed_cache.json` where it has one
   (else empty), `look` none, dials 11 and 39, `version` 0, `ranked` none, and the zone of the snapshot's
   directory. The recorded JSON is transformed as §18.5 says before the comparison, and `dst-edges` and
   `id-limits` follow §18.7.
5. Zones for the fixtures: tests cannot set `TZ`, so add `tests/common/zones.rs` with `berlin()` and
   `new_york()` as `TableZone`s generated from today's rules (EU: last Sunday of March and of October at
   01:00 UTC, +1 h / +2 h; US: second Sunday of March and first Sunday of November at 02:00 local, −5 h / −4 h)
   for the years 2000 to 2100, standard time outside. The fixture data lies in 2022 to 2027 plus winter days
   of 1970 and 9999, where both zones are on standard time.
6. A fixture comparison that fails is a port error, or a difference §14.3 does not list: report the second
   kind under `for_lead` with the file, the JSON path and both values. Do not add a transformation of your own.
7. Reminders are §14.5 as written; a `Reminder` is a canonical record.

## Tests and vectors to write

- The test target **`tests/snapshot_fixtures.rs`** (the stage gate runs
  `cargo test --locked -p taskdeck-core --test snapshot_fixtures`): all 48 files under `fixtures/snapshots/`,
  embedded with `include_str!` through a table in `tests/common/snapshots.rs` (file name → zone, board, `from`,
  `days`, `now`; the name carries the last three). Count the compared and the not-compared files and assert
  both counts, so that a file can never drop out silently.
- Port today's unit tests of the ported builders (`phone.rs` from line 2747: only those of `snapshot`,
  `build_day`, `build_tray`, `item_detail`, `done_rows`, `with_subscribed`) into `src/snapshot/tests.rs`, and
  the tests of the four board queries from `board.rs` (from line 1490) into `src/read/tests.rs`, with explicit zones.
- `tests/vectors/reminders.json` (§18.2) and its test: each source kind, the leads, both ends of
  `[from, until)`, the order by `(fire_at, key)`, the key's form.
- Clauses to cite: CORE-READ-001 to -005, CORE-SNAP-001 to -003, CORE-REM-001, CORE-REM-002.

## Done

The step gate of `common.md` exits 0 in W, and so does `cargo test --locked -p taskdeck-core --test snapshot_fixtures`;
no `unimplemented!()` is left in `read.rs`, `snapshot.rs`, `reminders.rs`; you committed in W. Report the
commit, the two counts, and every `for_lead` point.
