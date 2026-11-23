# Task B, step 3, `b3-rulings`: the lead's rulings on what tasks A and B found

Tasks A (`canon`, `time`, `board`, `ids`) and B (`command`, `apply`, `rebind`, golden logs) are in the working
copy, and the working copy now holds the spec with the lead's rulings merged in (`docs/sync/spec/core.md`;
`docs/sync/spec/CHANGES.md`, last row, lists them). Bring the code in line. Each `SPEC-QUESTION` comment that a
ruling below settles is replaced by a plain comment citing the section.

Find the spec text by heading (`grep -n '^## \|^### ' docs/sync/spec/core.md`); line numbers moved.

## Rulings to apply

1. **`Record::LIMIT` is required** (core.md §20, §4.5). Remove the default from `const LIMIT: usize` in
   `canon::Record`. `BoardData` and the records that only occur inside a board: 62,914,560. `Notice` and
   `RefusalRecord`: 4,096 (§10.7, §17), and their `from_canonical` checks it first.
2. **The tree budget uses fixed numbers** (§4.5 "Tree budget"). Today's charge uses `size_of` and `Vec`
   capacity, which differ between a 32-bit phone and a 64-bit desktop. Charge exactly: every string, keys
   included, the bytes between its quotes as written, plus 16; every array element 32; every object 744 for
   each started group of five members. Budget `max(32 × limit, 1,048,576)`; over it is `core/schema` once the
   whole text has passed step 2. Add vectors to `canonical.json` at the boundary (a text whose charges equal
   the budget parses; one more unit is `core/schema`; over budget and then not JSON is `core/not_json`), with
   the charges worked by hand from the rule.
3. **The size check of `apply`** (§8.4 step 4, CORE-APPLY-005; §17). After a command's own checks and before
   its effects: if its effects would leave canonical board bytes over 61,865,984 and longer than before the
   command, refuse with `core/cap`, the board unchanged. A command that does not lengthen the bytes is never
   refused for size. The result must be exactly that, on every device; how you get it is yours. A way that
   keeps `apply` fast: compute a bound `U` that is never below the true length, from lengths alone (no
   encoding: per record a constant plus six times its strings' byte lengths plus a constant per block; a pass
   over the lists is cheap), add a bound of what the command can add, and only when that sum is over the cap
   apply to a copy, encode it and compare exactly. Do not add a field to `BoardData`. Prove the bound with a
   property test (`U ≥ to_canonical_bytes().len()` on generated boards, and the bound of a command's growth on
   generated commands) and keep the 5,000-entry budget in mind (CORE-PERF-002: 1 s on a 10,000-row board).
4. **Golden cases for the size check**, in `tests/vectors/golden/rules-1/caps.json`: `start_gen` now takes
   `"unreadable": U` (U lines of 65,536 zero bytes each; §18.3). Cases: a command that lands exactly on
   61,865,984 bytes is applied; one byte more is `core/cap`; on a board already over the cap (build one with
   `start_gen`) a command that shortens it or keeps its length is applied and one that lengthens it is refused.
5. **`set_notes` at the last revision** (§9.20): `notes_rev` at 2^53 − 1 is `core/cap`: as implemented;
   cite the section and add the golden case if it is missing.
6. **Settled as implemented; replace the question comments by citations**: a mark with both numbers 0 is
   `core/invariant` rule 1 (§6.12); `TableZone::new` errors are `core/schema` and three or more candidates give
   the first and the last (§5.2); a lone surrogate escape is `core/not_json` (§4.5); `ChronoZone::from_local`
   is ascending and round-trip checked (§5.2), and `resolve_on_day` takes the earlier instant (§5.4,
   CORE-TIME-003); `target()` of `restore` and `forget_archived` is their `id` (§10.7 `target`); a `skipped`
   notice concerns its author alone (§10.7); the latest stamp's tie order (§10.1); `RefusalRecord` reads any
   `code` and `op` string, and `base` / `current` only with `op` `set_notes` (§10.7).
7. `src/lib.rs` still has foundation's inline `mod tests` with `env!`: move its one test to `tests/purity.rs`
   or delete it with a replacement there, and keep `#![allow(unused_variables)]` (other modules are skeletons).

## Tests

Every ruling that changes behaviour gets its test or vector (rulings 1 to 5). Cite CORE-APPLY-005 where you
test the size check, CORE-ENC-004 at the budget vectors. Existing vectors and golden files change only where a
ruling changes the expected result; say which did.

## Done

The step gate of `common.md` exits 0 in W, and so does `cargo test --locked -p taskdeck-core --test golden`;
you committed in W. Report the commit, what changed per ruling, the measured cost of the size check per
`apply` on a 10,000-row board if you measured it, and every `for_lead` point.
