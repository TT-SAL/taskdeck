# Task B, step 4, `b4-rulings`: four small points from task B's check

Task B was checked with nothing high or medium. These follow the spec change the lead made from that check
(`docs/sync/spec/CHANGES.md`, last row). Small step: do exactly this and nothing else.

1. **`reset` at the last notes revision** (core.md §11.3, CORE-RULES-005). When the larger of the two boards'
   `notes_rev` is already 2^53 − 1, `reset` returns `core/cap` and changes nothing (today it returns a board
   that fails `check`). Add the case to `tests/vectors/reset.json`.
2. **`BoardData::to_canonical_bytes` never returns empty bytes.** Today an encoding failure is swallowed
   (`unwrap_or_default`). A board that passes `check` always encodes; for one that does not (built by hand,
   with an integer above 2^53 − 1), panic with a message that says so, and say it in the doc comment. Check
   that no caller inside the crate encodes a board before it is known to pass `check` (after the fix of
   point 1, `reset` does not).
3. **A golden `parse_error` case** (§18.3): a `create_task` with 1,001 blocks, in
   `tests/vectors/golden/rules-1/caps.json`, result `parse_error`, code `core/schema`, board unchanged.
4. **CORE-ID-003 and the benign `restore`**: a benign repeat raises no mark. That is what the code does; add
   the assertion to an existing test of the benign `restore` (the mark of the author is unchanged), citing
   CORE-ID-003 and CORE-APPLY-004.

Done: the step gate of `common.md` exits 0 in W, and so does `cargo test --locked -p taskdeck-core --test golden`;
you committed in W. Report the commit and what changed per point.
