# Task F, step 3, `f3-rulings`: three points from the transcript test

Small step: do exactly this. The spec in the working copy holds the rulings (core.md §13.3 G3, §18.4;
`docs/sync/spec/CHANGES.md`, last row).

1. **A naive wire time outside the instant range keeps today's answer** (§13.3 G3): `core/intent_json`, status
   400, message `` not a time: `<text>` `` (the text as typed, after trimming). Step D2 had made it
   `core/date_range`; the recorded transcripts say otherwise (`refusals`, step 10, both zones). Only the naive
   wire times of G3 change back: a row that resolves a day and minutes outside the range stays
   `core/date_range`. Flip the `compile.json` cases back that D2 flipped for naive wire times, and fix their names.
2. **The transcript test has nothing left waiting**: with point 1, `refusals` step 10 compares in both zones.
   Remove the `AWAITING_THE_LEAD` list and its machinery from `tests/transcripts.rs`, and update the asserted
   counts (they can only grow).
3. **Every classification rule of the test is exercised**: `the_rules_fire_on_their_cases` has no hand-made
   case for the reflow-ties rule and the key-ties rule of §13.5. Add one each: a case where the rule must fire
   and its nearest neighbour where it must not.

The lead has also settled, as the test already does: the id map is kept per id list (§18.4); a row's id is not
compared; a recorded block index with no block becomes sid 0. Replace the `SPEC-QUESTION` comments about these
in the test by citations of §18.4.

Done: the step gate of `common.md` exits 0 in W, and so does `cargo test --locked -p taskdeck-core --test transcripts`;
you committed in W. Report the commit, the new counts, and what changed per point.
