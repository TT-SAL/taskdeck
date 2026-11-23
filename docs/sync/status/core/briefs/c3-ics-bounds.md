# Task C, step 3, `c3-ics-bounds`: the calendar parser's cost is bounded

The security pass on the ported ICS parser found seven things, all rated low on the stage's scale (robustness
on hostile input). The lead has ruled that they are fixed all the same: the parser now runs on the phone
against text from an address anyone may control, and `docs/sync/spec/device.md` DEV-ICS-004 gives a fetch 20
seconds, which a device cannot keep if a 120 KB feed parses for 109 s or a 1 MB feed takes 1.16 GB. The bounds
below go into core.md §14.6 with spec change 5, right after this work; until then this brief is the text.

The findings, with the reproduction of each as text, are in
`/home/user/taskdeck/docs/sync/status/core/findings/security-c-ports.json`; the one low finding of the check
(a test of today's that was not ported) is in `findings/verify-c-ports.json` beside it.

## Do

First add each reproduction as a test and see it fail (a slow one: see it be slow, then give it a size that
shows the bound without taking minutes). Then make `src/ics.rs` and `src/overlay.rs` keep these bounds:

1. A `BY…` list of a recurrence rule is read without duplicates and holds at most 64 values; a rule with a
   longer list is a rule the parser cannot follow, treated as today's parser treats one.
2. A `VTIMEZONE` holds at most 512 observances and yields at most 4,096 transition points inside the window it
   is asked for; a zone over either bound is treated as an undefined zone is today. Transitions are found by
   search, not by a scan per lookup.
3. The `EXDATE` values, the `RDATE` values and the `RECURRENCE-ID` overrides of one event are each at most
   `max_occurrences_per_event`; what is beyond is dropped and noted once. Matching an occurrence against the
   exclusions is a lookup, not a scan. The per-event cap holds before the overrides are expanded into memory.
4. Arithmetic on durations, ends and spans is checked: an occurrence whose end cannot be computed is dropped
   and noted; no build panics and none returns an end before its start. `OverlayEvent::minutes` saturates.
5. `Parsed.problems` holds one line per kind of problem with a count, at most 64 lines, and no line (and no
   `Err` of `parse`) quotes text of the file: not a `TZID`, not a `CALSCALE` value (CORE-GEN-010).
6. Occurrences that tie on (day, start, end, name) are ordered by their remaining fields, so one calendar
   listed in two orders parses equal.

And port the missing test the check named (`two_meetings_at_the_same_hour_are_given_a_column_each`).

## Points the lead has settled

1. Nothing else about the parser changes: `Limits::default()` keeps today's numbers, and every ported test
   keeps its expected values. The bounds only bite on calendars no calendar program writes.
2. The new numbers (64, 512, 4,096) are private constants next to `Limits`, each with the reason in a comment.
3. Bounded time is proven by tests that must finish: put the hostile inputs through `ics::parse` at the full
   8 MiB body size where the reproduction scales (build the text in the test), using proptest's own `timeout`
   for the cases that were slow (the crate's tests cannot read a clock). Say in your report how long each
   takes now in a debug build.
4. No function here reads a clock, and nothing is allocated in proportion to a number the file states rather
   than to the file's size or the limits.

Clauses to cite: CORE-READ-004 stays where it is; cite CORE-GEN-010 at the problems test and CORE-GEN-003 where
it applies. Spec questions go under `for_lead` as always.

## Done

The step gate of `common.md` exits 0 in W; you committed in W. Report the commit, per finding what the test
showed before and after, the timings, and every `for_lead` point.
