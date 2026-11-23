# Task D, step 2, `d2-rulings`: the lead's rulings on what tasks C, D and E found

Tasks A to E are in the working copy. Their implementers and checkers raised questions the spec did not
answer; these are the answers. They go into the spec through a pull request of their own right after this
work (spec change 5), so the spec in your copy does not hold them yet: this brief is the text. Bring the code
in line, and where a `SPEC-QUESTION` comment is settled below, replace it by a plain comment that says
"lead ruling, spec change 5" and the section.

## Rulings that change code

1. **The board-size check in `compile`** (core.md §13.3 G6, §8.4 step 4). `compile` already runs `apply`'s size
   check on the view. Its error is code `core/cap_board`, status 400, message `The board is full.` (today the
   fix returns `core/cap`). It comes after the row's own checks and before ids are minted.
2. **`set_notes`** (§13.4 row, in this order after the length check): an intent `base` that is given and is
   not `view.notes_rev` is `core/notes_stale`, status 400, message `The notes changed on another device.`;
   `view.notes_rev` at 2^53 − 1 is `core/cap_board`. The command is always `set_notes{text, base: view.notes_rev}`.
   So every `set_notes` that `compile` returns is accepted by `apply` on the view: remove the `apply_code`
   exception from `compile.json` and from the test that reads it, and the separate refusal of a base above
   2^53 − 1 folds into this rule.
3. **The minter is the device's** (§13.3 G7). `minter.device_no` other than `cx.device_no` is code
   `core/bad_id`, status 400, message `The id counter belongs to another device.`, checked first, for every
   intent; the minter is unchanged.
4. **A local time outside the instant range is not a gap** (§5.4, §13.3 G3 and G5). Today's reading turns
   00:00 on 1970-01-01 in a zone east of Greenwich into 01:00 (the 60-minute step over a "gap"), and answers a
   time after the end of 9999 with the daylight-saving message.
   - `time::resolve_on_day`: a local time is **outside the range** when its naive date-time, taken as seconds
     since 1970-01-01T00:00:00, minus `zone.offset_at(Instant::MIN)` is below 0, or minus
     `zone.offset_at(Instant::MAX)` is above 253,402,300,799. For such a time the result is none, and step 3
     does not try 60 minutes later. Give `time` a `pub(crate)` function that answers that question, for `compile`.
   - `compile`: wherever a row says "none: `core/bad_time`", and for a naive wire time (G3), a local time
     outside the range is `core/date_range` (400, `That date is out of range (1970–9999).`); `core/bad_time` and
     `not a time` stay for a real gap.
   - Add the cases to `planner.json` (both ends of the range, a zone east and a zone west of Greenwich) and to
     `compile.json`.
5. **`LogCommand::to_canonical` never returns empty bytes.** Like `BoardData::to_canonical_bytes` since step
   B4: for a command holding an integer the encoding cannot carry, panic with a message that says so (no
   text of the command in it), and say it in the doc comment. `compile` never builds such a command; its
   property test for CORE-COMP-008 covers that.
6. **`read::archive_by_day`**: a `days` above 366 is read as 366 (§14.4), so no caller can ask for billions
   of buckets.

## Rulings that keep the code as it is (replace the question comments by citations)

- `compile`: `restore` also checks the block cap (`core/cap_blocks`) and a sid already live (`core/id_taken`, 400,
  `That item's blocks are already on the board.`); an op outside the view's rules is `core/rules`, 400,
  `This board's rules do not allow that yet.`; a minter with no number left is `core/cap_items` or
  `core/cap_blocks`; a reflow keeps its first 1,000 moves; the three queries are answered `core/query` by
  `parse_intent`; a bad `archived_at` has step 4's message; an intent is a JSON object, never an array.
- Import: `id_range` for `next` only when a renumbering would assign 2^40 or more; the ids it lists are the
  out-of-range ids, ascending and distinct, or the ids left without a number; more than 20 subscriptions is
  `field_range` with the ids past the twentieth; a failed import's report holds its `failure` and nothing else;
  `blocks` counts live blocks; a leap second is `instant_range`.
- Read side: a reminder lead above 1,440 is read as 1,440; `weather::build_report` leaves `fetched_at` at 0
  for the caller to set; an ICS time in UTC outside the instant range takes the zone's offset at the nearer
  end of the range.
- Test fixtures (core.md §18.7): `fixtures/boards/unreadable` is refused with `field_range` and is not
  replayed, as the tests already do; `fixtures/legacy-other/empty` is refused with `notes_unreadable`.

## Tests

Every ruling that changes code gets its test or vector; say which existing vectors changed and why. Clauses
to cite: CORE-COMP-001, CORE-COMP-008, CORE-TIME-003, CORE-ID-004, CORE-READ-002.

## Done

The step gate of `common.md` exits 0 in W, and so do `cargo test --locked -p taskdeck-core --test golden` and
`--test snapshot_fixtures`; no `SPEC-QUESTION` that a ruling above settles is left; you committed in W. Report
the commit, what changed per ruling, and every `for_lead` point.
