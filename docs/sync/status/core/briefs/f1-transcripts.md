# Task F, step 1, `f1-transcripts`: the transcript test

Plan: SYNC_PLAN.md §6.2 gate, "transcript test: replaying `fixtures/`' recorded sequences gives the recorded
states, for the commands whose meaning did not change". Everything it needs is in the working copy: the
import, intents and `compile`, `apply`, the ported read side, and `tests/common/` (fixture folders, zones).

## Build

The test target **`tests/transcripts.rs`** (the stage gate runs
`cargo test --locked -p taskdeck-core --test transcripts`), with a table of the 20 transcript files in
`tests/common/transcripts.rs` (`include_str!` each; 10 names × `europe-berlin`, `america-new-york`).

| Read | Where |
|---|---|
| what a transcript file holds; the order of `state`; the quirks of today's output | `fixtures/README.md`, "Transcript files" and "Quirks" |
| how each recorded command is classified against the new ones, and the replay's zone | core.md §13.5 |
| the replay: id map, the compared projection | core.md §18.4 |
| the two folders the import refuses | core.md §18.7 |
| §12.2 (intent parsing), §13.4 (the compile table), §16 (codes, statuses, messages) | by heading |

## How the replay goes (core.md §13.5, §18.4, in short)

1. Start from `import` of the transcript's board folder (`boards.rs`); `dst-edges` from the repaired folder
   and `id-limits` not at all (§18.7: assert the refusal).
2. Per step: translate the recorded command's ids through the id map `M` and a `session` index into the sid
   of that item's i-th block; `parse_intent`; `compile` with `now` (the step's clock, truncated to
   milliseconds), `device_no` 1, the replay zone, and the overlay of the recorded state before the step;
   `apply` with `Ctx { seq: k + 1, device_no: 1, at: now, based_on: k }`. One `IdMinter` per transcript.
3. The replay zone is the transcript's zone from `zones.rs` wrapped so that a local time in a fold gives the
   later instant alone (§13.5: the recording did).
4. Compare, while every step so far is classified "same": the outcome (a recorded 200 against a compiled
   command or a "nothing to do" reply; a recorded 4xx against the `CompileError`'s status and message,
   character for character; for an unparseable body the status and the prefix `Could not read the command: `),
   `M[recorded reply id] = compiled reply id`, the reply's `moved`, and the state through the projection of §18.4.
5. A step is "changed" only by a row of the table in §13.5 or by §18.7, decided by a rule written in the
   test (a function per exception that looks at the step and the state before it), never by "it did not
   match". After a changed step the rest is replayed without comparing.

## Points the lead has settled

1. A mismatch that no rule of §13.5 or §18.7 explains is a defect: in the test, or in the core. Find which.
   A defect in the core (the import, `compile`, `apply`, a ported function) you fix in the core, with the
   failing transcript step as the regression test and a line in your report. A difference that looks intended
   by the spec but is not in §13.5 goes under `for_lead` with the transcript, the step, the recorded and the
   replayed value; leave that step failing in your report rather than classify it yourself. Do not add a rule
   of your own and do not loosen the projection.
2. Count, per transcript, the steps compared and the steps not compared, and assert both numbers as
   constants in the test, so a step can never drop out silently. Put the 20 pairs in your report.
3. Today's quirks that the fixtures README lists and the spec changed on purpose (duplicate ids after a
   reopen, the saturating counter, ties in the archive order, the notes cap before detab, the 1970–9999 check
   on the local date) are each a row of §13.5 or covered by §18.7. Check each against its row before you
   rely on it.
4. The recorded `state` lists the archive newest first in today's order and items in today's `Vec` order;
   the projection compares items as a set and the archive as a multiset (§18.4), so order does not matter.
5. Keep the test fast: the transcripts are 6 MB; parse each file once.

Clauses to cite: CORE-COMP-001, CORE-COMP-003, CORE-COMP-005, CORE-COMP-006, CORE-INT-001, CORE-IMP-005,
CORE-CMD-004, CORE-READ-001.

## Done

The step gate of `common.md` exits 0 in W, and so does `cargo test --locked -p taskdeck-core --test transcripts`;
you committed in W. Report the commit, the 20 pairs of counts, every core defect you fixed, and every
`for_lead` point.
