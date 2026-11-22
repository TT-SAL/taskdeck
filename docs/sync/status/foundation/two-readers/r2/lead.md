# Two-readers round 2: result and the lead's rulings

Method (decision D22): two readers, each writing all six scenarios from the specs alone; one comparer per
scenario group; one fixer per spec. Reader A read nearly all of the specs in full and then had little room
left to write (its files are the shorter ones); reader B read the sections its scenarios needed. Round 3
goes back to reading by section.

Result: the two readers agreed on every byte layout, width, context string, counter, seq and chain value
that both gave. Counting differences, the ones that needed a spec change: 12 (edit 4, join 5, trust 3),
against 40 in round 1. A further 20 differences were one reader missing clear text, and 20 were wording.
Changes applied: seal 2, core 1, relay 1, engine 6, device 2, bridge 1; nothing left unapplied. The specs
hold 938 clauses and the lead's format check finds no problem.

Rulings on the comparers' "For the lead" points:

- **T11. The specs fix the words of the lines that report a sync outcome**, the same on the phone and on a
  desktop apart from "This phone's" and "This device's": the notice and refusal lines of `bridge.md` A-04
  (TR2-EDIT-4, DEV-FACADE-010) and the rejoin prompt (TR2-JOIN-4, DEV-FACADE-011, BRIDGE-SCREEN-002). Two
  readers who invent the words differ, a test cannot assert an invented line, and a user with both apps
  should read the same sentence. Stage `desktop` owns the two desktop clauses and may change the words
  through `spec/CHANGES.md`.
- **T12. Records queued before a rejoin are sent unchanged afterwards**, however old, with no prompt and no
  review (TR2-JOIN-5, ENG-BOOT-008). An overwrite notice or a refusal reports what they did, as for any
  record. A review of old records would be a new feature.
- **T13. Rulings T2 and T8 stand.** Reader B's `complete` body without `at` goes against the text T2 put
  into `bridge.md` §6.3 (edit difference 13); that a snapshot whose blob a rollback lost stays usable, so
  that no new snapshot falls due, follows from T8 (trust difference 5). No change.

Fixers' questions, settled: ENG-SEALER-008 stays owned by `device` (T9); DEV-FACADE-010 and -011 stand after
the two new paragraphs of `device.md` §17.7; the sections the new `core.md` §10.2 paragraph cites exist
(`engine.md` §13.4, `device.md` §17.7, `bridge.md` A-04, `core.md` §10.7).

For the next pass (added to `resolutions/r1-followups.md`): `relay.md` §8.2's row for `login-nonce` and
`login` must list `state = 1` (item 10); the README's glossary example for "notice" (item 11).

World facts the comparers found missing are added to the brief for round 3: each device's next item id,
that the relay has never compacted, that no beat falls due, that nobody has appended yet on the day of
`T0`; for scenario 5 the author rows of the relay's older copy; for scenario 6 that the queued edit was
never sealed, that its task still exists and that Mac and Phone were members before seq 100. What the specs
decide (for example that a new device's first entry is its beat) stays out of the brief.
