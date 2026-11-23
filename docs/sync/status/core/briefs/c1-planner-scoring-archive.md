# Task C, step 1, `c1-ports`: planner, scoring, archive (a port)

Plan: SYNC_PLAN.md §6.2 item 1 and §7 "Porting: copy the code, then test it against the recorded
transcripts, before changing behaviour". You move today's pure read-side logic into the core, unchanged in
what it computes. `canon`, `time`, `board`, `ids`, `command`, `apply` are in the working copy; build on them.

## Fill (from → to)

| Today, under `/home/user/taskdeck/src/` | Core file | Items (exactly those of core.md §20) |
|---|---|---|
| `planner.rs` lines 1–723 | `src/planner.rs` | constants; `Placement`; `DayPlacement`; `Placeable`; `placements_for`, `placements_of`, `appears_on`; `Lane`, `lay_out`; `DaySummary`, `summarize`, `summary_text`; `format_minutes`, `format_duration`; `drop_length_for`, `duration_options`; `BacklogGroup`, `backlog_group`; `relative_due`; `Flowable`, `behind_minutes`, `reflow` |
| `tasks.rs` lines 1–779 | `src/tasks.rs` | the constants and tables; `weekday_bit`; `calendar_item_color`; `Recurrence::{falls_on, repeats, includes, summary, summary_dated}`; `Item::{importance_score, tie_break_jitter, is_routine, calendar_item_color, planner_anchor, is_planned, planned_minutes, remaining_minutes, wants_planning}` |
| `archive.rs` lines 1–818 | `src/archive.rs` | `Timing`; `ArchiveRow::{verdict, timing, was_finished, was_judged, mark, lifetime, booked_minutes, sittings, color_id, is_routine}`; `format_span`, `format_total`; `Filter`, `OutcomeFilter`, `KindFilter`; `Summary`, `summarize`; `MonthRun`, `group_by_month` |

Not ported: UI geometry and drags (`TimelineGeometry`, `Drag`, `preview`, `block_from_drag`, `CreateKind`),
`Recurrence::toggle`, every file read or write, the archive log and its loading. `snap`, `clamp_block`,
`resolve_on_day` and `now_marker` are already in `time` (task A): call them.

| Read | core.md lines |
|---|---|
| §14.1–§14.2 rules for read queries and the table of what moves and how | 991–1030 |
| §5.2–§5.3 zones, local day and minutes | 147–173 |
| §6.2–§6.6 `Item`, `Block`, `Recurrence`, `ArchiveRow` against today's `Active`, `Session`, `Archived` | 223–280 |
| §20 `planner`, `tasks`, `archive` | 1620–1707 |

## The only changes to make

1. `Item` and `ArchiveRow` (core) in place of `Active` and `Archived`; `Block` / `RowBlock` in place of
   `Session`. `kind` is a field now: where today's code tests `is_event` or `recurrence.is_some()`, test `kind`
   (`Routine` when a recurrence is present, else `Event`, else `Task`: §15.3 step 10 gives the same mapping).
2. Time: `Instant` in place of `DateTime<Local>`. Where today's code takes a local date, hour or minute from a
   `DateTime<Local>` (`date_naive()`, `hour()`, `minute()`, `with_timezone(&Local)`, `Local.from_local_datetime`),
   take a `zone: &(impl Zone + ?Sized)` parameter and use `time::local_day`, `time::local_minutes`,
   `time::resolve_on_day` (CORE-TIME-002, CORE-READ-001). Differences of two instants need no zone.
   The signatures in §20 say which functions take a zone; a function without one must not need one.
3. Blocks are addressed by sid: `DayPlacement.session` is `Option<u64>` holding the block's `sid`; wherever
   today's code walks `sessions` in index order, walk `blocks` in ascending sid (they are stored so).
4. Nothing here changes a `BoardData` or reorders one of its lists (CORE-READ-002, CORE-READ-005).
5. Presentation code may keep `f32` exactly as today (`importance_score` and its tables, the jitter): the
   snapshot fixtures compare `ranked` with today's output, so port the arithmetic operation by operation,
   in the same order and with the same types. Do not "tidy" it.
6. Keep today's doc comments where they explain the why; drop references to code that is not ported.

Everything else is copied: same names, same constants, same strings, same arithmetic. If a function cannot
be ported without a decision the spec does not make, list it under `for_lead` and port the closest reading.

## Tests

- Port today's unit tests of the ported functions (`planner.rs` from line 724, `tasks.rs` from 780,
  `archive.rs` from 819) into `src/planner/tests.rs`, `src/tasks/tests.rs`, `src/archive/tests.rs`: same
  inputs, same expected values. Where a test built its clock or instants with `Local`, give it an explicit
  zone (`FixedZone`, or a `TableZone` with the transitions the test needs) and say in a comment which zone
  the old test ran in. A test of something not ported is not ported. Do not change an expected value: a
  ported test that fails is a port error or a `for_lead` point.
- Add tests that a zone other than UTC changes what it should: one function per module with a `TableZone`
  that has a spring-forward and a fall-back transition.
- Clauses to cite: CORE-READ-001, CORE-READ-002, CORE-READ-005, CORE-TIME-002.

## Done

The step gate of `common.md` exits 0 in W; no `unimplemented!()` is left in the three modules; you committed
in W. Report the commit, what you ported, what you left out and why, and every `for_lead` point.
