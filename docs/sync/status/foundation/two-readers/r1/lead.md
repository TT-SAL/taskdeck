# Two-readers round 1: result and the lead's rulings

Result: the two readers of each group agreed on every byte layout, counter, seq and chain value that both
gave. The differences were all about behaviour the specs left open (the order of durable writes, defaults,
who waits for what). Counting differences, the ones that needed a spec change: 40 (edit 10, join 17, trust 13).
Changes applied: seal 3, core 1, relay 4, engine 23, device 7, bridge 2, the review round's follow-ups
included. After them the specs hold 930 clauses and the lead's format check finds no problem.

Rulings on the comparers' "For the lead" points, all as the comparers recommended:

- **T1. `ClearMarker`** is emitted last in the post-apply `Persist` that carries the notices and the outbox
  drops (TR1-EDIT-4).
- **T2. bridge.md §6.3 states the values today's page sends** for `complete` and `quick_add` (TR1-EDIT-7),
  so the spec stands without the old code.
- **T3. `StopKind::Rejoin`** joins engine.md's public API, with `ConfirmRejoin` as its action (TR1-JOIN-10).
- **T4. Acknowledging the continuity warning** gets an action, a `Meta` field, a facade call and a bridge call
  (TR1-JOIN-11). The same gap for `RestoredCopy` and `BasedOnUnchecked` goes to the next pass.
- **T5. A join may emit `TruncateLogBelow`** (TR1-JOIN-6).
- **T6. Scenario 6's world could not arise** (compaction keeps the newest 16,384 seqs). The brief's scenario 6
  is restated for round 2 with a head of 20,000.
- **T7. A `revoke` answered `409 relay/target` counts as done**; any other refusal ends the operation
  (TR1-TRUST-1).
- **T8. Blobs lost to a relay rollback are not uploaded again**; the existing fall-backs apply (TR1-TRUST-8).
- **T9. ENG-SEALER-007 to -009 stay in engine.md with owner `device`** (TR1-TRUST-2 and -3).
- **T10. No second alarm of a kind while one is unresolved** (TR1-TRUST-5).

Fixers' questions, settled: clause ids need not stand in number order within a section (README §4.2); a new
clause sits after the clause its change names, whatever the section number the change wrote.

For the next pass (added to `resolutions/r1-followups.md`): a test-vector family in seal.md §24 for the bytes
drawn for each wrap's ephemeral key; the acknowledgement gap of `RestoredCopy` and `BasedOnUnchecked`.
