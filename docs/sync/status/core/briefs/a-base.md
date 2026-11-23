# Task A, `a-base`: canonical encoding, time, board data, id minting

Plan: SYNC_PLAN.md §6.2 items 2 (BoardData, UTC types, canonical serialisation), 4 (ids, marks) and 6
(canonical encoding v1). Everything later in the stage builds on these four modules.

## Fill these files (bodies are `unimplemented!()` today)

| File | What | core.md lines |
|---|---|---|
| `src/canon.rs` | `encode`, `parse`, `is_canonical`, the `Record` trait's use | §4: 58–139 |
| `src/time.rs` | `Instant`, `LocalTime`, `Zone`, `FixedZone`, `TableZone`, `ChronoZone`, `local_day`, `local_minutes`, `resolve_on_day`, `now_marker`, `snap`, `clamp_block`, `snap_up` | §5: 141–199 |
| `src/board.rs` | every record of §6 with its `Record` impl; `BoardData::{empty, to_canonical_bytes, from_canonical_bytes, check, item, subscription, mark}`; the archive order of §6.12 as a `pub(crate)` comparison that later tasks reuse | §6: 201–358 |
| `src/ids.rs` | `compose`, `split`, `IdMinter::{new, mint_item, mint_block}`. **Not** `rebind` (task B2) | §7.1–§7.4: 360–389 |
| `src/lib.rs` | `CoreError`, `CompileError`: derive what callers need (`Debug`, `Clone`, `PartialEq`, `Eq`) | §16: 1193–1204 |

Also read: terms §2 (23–39), rules §3 (41–56), limits §17 (1271–1314), vectors §18.1–§18.2 (1316–1343),
the API of these modules in §20 (1425–1526). Today's arithmetic to match is `/home/user/taskdeck/src/planner.rs`
lines 255–311 (`snap`, `clamp_block`, `resolve_on_day`) and 645–660 (`snap_up`).

## Points the lead has settled

1. `canon::parse` does §4.5 steps 1, 2, 3 and 5 on a `serde_json::Value`. `serde_json::Value` drops
   duplicate keys and nests to 128, so it cannot be the parser as it is: detect a duplicate key anywhere
   (`core/schema`) and a nesting depth above 32 (`core/not_json`) yourself. `null`, a number with a fraction
   or exponent, and an integer outside ±(2^53 − 1) are `core/schema` (CORE-ENC-005). A byte-order mark is
   `core/not_json`. Check the length against `limit` before anything else is touched (CORE-GEN-009).
2. Nesting depth counts containers: a scalar is depth 0, `[]` is depth 1; 32 nested containers parse, 33 do not.
3. In `BoardData::from_canonical_bytes`, a field of the wrong type, an unknown or missing field or a value
   outside its field's range is `core/schema`; an array out of its order, a forbidden duplicate, a count over
   a cap or a cross-record rule is `core/invariant` with the number of the first failing rule of §6.12 in
   `CoreError.detail` (rule 1 for order and duplicates, 2 for caps, and so on).
4. `unreadable` lines are base64 in JSON (RFC 4648 §4 with padding, hand-written: no dependency). Compute the
   decoded length and check it against 65,536 before decoding (CORE-GEN-009); only the one canonical base64
   text of a byte string is accepted.
5. `from_canonical_bytes` of the 10,000-row fixture (about 4 MB) has a budget of 150 ms as a median in CI
   (CORE-PERF-003), and the 60 MiB limit applies. Do not build intermediate copies you do not need; the
   canonical check re-encodes once.
6. Keep every `BoardData` list in its spec order at all times (items by id, archive in archive order, …).
   Give later tasks what they need to keep it so: `pub(crate)` helpers for "insert an archive row at its
   place", "find the first row with a key", "the mark of a device, raised to n".
7. `ChronoZone<Z>` is generic; nothing in the library names `chrono::Local` (the library is built without
   chrono's `clock`, where `Local` does not exist; `bash crates/taskdeck-core/ci.sh` proves it).
8. Leave `#![allow(unused_variables)]` in `lib.rs`: other modules are still skeletons.

## Tests and vectors to write

- `tests/vectors/canonical.json`, `instants.json`, `planner.json`, `board-bytes.json` with the contents core.md
  §18.2 lists for each (every example of §5.5; one `board-bytes` case per rule of §6.12; the invalid inputs
  the table names), and a test target that reads them all (`tests/vectors_base.rs`).
- Property tests (`tests/base_props.rs`): `canon::parse` and `BoardData::from_canonical_bytes` return an
  error and never panic on random and mutated bytes (CORE-GEN-007); `to_canonical_bytes(from_canonical_bytes(b)) == b`
  for generated boards (CORE-DATA-005); `encode` then `parse` is the identity on generated values; a minter
  never yields a number twice (CORE-ID-004).
- `tests/purity.rs` for the clauses a build proves: CORE-GEN-001 (`include_str!` of `src/lib.rs` holds
  `#![forbid(unsafe_code)]`), CORE-GEN-002 and CORE-GEN-003 (`ci.sh` still builds the library alone for
  wasm32), CORE-GEN-004 (`clippy.toml` lists the six entries the clause names), CORE-GEN-005 (the crate's
  `Cargo.toml` has exactly `serde`, `serde_json`, `chrono` under `[dependencies]`). Read those files with
  `include_str!` and check their text.
- Clauses to cite in tests: CORE-GEN-001 to -007 and -009 (as far as these modules go), CORE-ENC-001 to -008,
  CORE-TIME-001 to -005, CORE-DATA-001, -002, -004, -005, -007, CORE-ID-001, -004, -007.

## Done

The step gate of `common.md` exits 0 in W; no `unimplemented!()` is left in the four modules except
`ids::rebind`; you committed in W. Report the commit, what you built, and every `for_lead` point.
