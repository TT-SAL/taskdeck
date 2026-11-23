# Task B, step 1, `b1-command`: log commands v1 and their parser

Plan: SYNC_PLAN.md §6.2 item 3 ("Log commands deserialize strictly") and item 2. Task A (`a-base`: `canon`,
`time`, `board`, `ids`) is in the working copy; build on it and do not redesign it. If something in those
modules blocks you, make the smallest change that unblocks you and say so in your report.

## Fill `src/command.rs`

`LogCommand` and its structs, `op()`, `to_canonical()`, `with_name()`, `target()`, `parse_command()`.

| Read | core.md lines |
|---|---|
| §9.1–§9.2 envelope, parsing, shared types | 468–494 |
| §9.3 the 29 commands; §9.4–§9.24 fields and schema rules (ignore the "Apply"/"Effects" text: that is step B2) | 496–678 |
| the clauses CORE-CMD-001 to -003 | 680–682 |
| §4 canonical encoding, already implemented in `canon` | 58–139 |
| §6.9 `PictureRef` (for `set_picture`) | 296–312 |
| §16 parse codes; §17 limits | 1193–1204; 1271–1314 |
| §18.2, the `commands.json` row | 1335 |
| §20 `command` | 1528–1559 |

## Points the lead has settled

1. `parse_command` runs the seven steps of §9.1 in order and returns the first failing step's code. Length
   first (more than 257,979 bytes is `core/too_large`; zero bytes pass step 1 and fail step 2 with
   `core/not_json`), then JSON, then duplicate keys, then `op`, then the schema, then canonical bytes, then
   the cross-field "Schema rule"s.
2. A command is a canonical record: a field at its default is absent (`blocks: []` of `create_task`,
   `base: 0` and `text: ""` of `set_notes`), an optional field is absent when none, and a present default is
   `core/not_canonical` by step 6.
3. `to_canonical()` then `parse_command()` is the identity for every `LogCommand` whose values are in
   range; `parse_command` never panics on any input (CORE-GEN-007) and checks sizes before allocating
   (CORE-GEN-009): a `name` is counted in scalars, a `url` in scalars, `text` in scalars.
4. `with_name` replaces the name of `CreateTask`, `CreateEvent`, `CreateRoutine` and returns `Err(self)` for
   every other command (V2-17). `target()` is the command's `id` field where it has one (the item or
   subscription the command names; for creates the id created), else `None`.
5. `RemoveBlocks { blocks: Vec<(u64, u64)> }` holds `(id, sid)` pairs, in that order.

## Tests and vectors to write

- `tests/vectors/commands.json`: per op one valid case at each boundary of each field and one invalid case
  per field constraint and per schema rule, each with the expected code; plus the envelope cases of §9.1
  (oversized, not JSON, duplicate key, unknown `op`, missing `op`, unknown field, non-canonical bytes). Write
  the cases from the spec's tables, not from your parser's output.
- A test target `tests/commands.rs` that checks every case and holds the property tests: never panics on
  random and mutated bytes; `parse_command(to_canonical(c)) == c` for generated commands of all 29 ops.
- Clauses to cite: CORE-CMD-001, -002, -003, and CORE-GEN-007 and -009 for this parser.

## Done

The step gate of `common.md` exits 0 in W; no `unimplemented!()` is left in `src/command.rs`; you committed
in W. Report the commit, what you built, and every `for_lead` point.
