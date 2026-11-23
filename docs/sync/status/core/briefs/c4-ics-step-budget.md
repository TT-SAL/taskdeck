# Task C, step 4, `c4-ics-step-budget`: one step budget for all the rules of a file

Step C3 bounded the calendar parser's cost in six ways and measured one product it was not asked to bound:
every event and every time-zone observance with a rule may walk the full `max_rule_steps` (100,000), so
20,000 events of `DTSTART:00010101T090000` with `RRULE:FREQ=DAILY` (1.4 MB, no occurrence in the window) take
about 31 s in a release build, against the 20 s a device has for a fetch (`docs/sync/spec/device.md`
DEV-ICS-004). This is the lead's ruling on it; it goes into core.md §14.6 as a seventh bound with spec change 5.

## The bound

All the recurrence rules that one call of `ics::parse` follows, those of events and those of `VTIMEZONE`
observances together, take at most `200 × max_rule_steps` steps (20,000,000 with today's limits). A rule that
the budget cuts short is treated exactly as a rule that reached `max_rule_steps` on its own is treated today:
what it found so far stands, and the problem is noted once per file ("one line per kind, with a count").

- The factor 200 is a private constant next to the others (`MAX_BY_VALUES` and so on), with its reason: a
  calendar of 5,000 daily events that began ten years ago needs 18 million steps; a hostile file can spend
  no more than the budget, about a third of a second here in a release build.
- The budget is one counter for the whole call, spent in file order. It is a number of steps, never a time:
  the parser reads no clock.
- Nothing else changes. `Limits::default()` keeps today's numbers and every existing test keeps its
  expected values.

## Tests

- The reproduction first: the 20,000-event file above, built in the test, under proptest's `timeout` as the
  other timed cases in `tests/ics_bounds.rs` are; see it exceed the timeout before the change and finish after.
- 512 `DAILY` observances from year 1 in one `VTIMEZONE`, the same way.
- A calendar inside the budget is unchanged: 2,000 daily events that began ten years before the window give
  the same occurrences with and without the counter (compare with a second parse under a larger
  `max_rule_steps`, or count them by hand).
- The file-order rule: with a budget that runs out in the middle, the events before the cut have all their
  occurrences and the note appears once.

Cite CORE-GEN-003 at the timed tests, as C3 did. (A clause for the bounds, CORE-READ-007, comes with spec
change 5; the last task of the stage adds its citation.)

## Done

The step gate of `common.md` exits 0 in W; you committed in W. Report the commit, the timings before and
after in a debug and a release build, and every `for_lead` point.
