# TaskDeck sync spec: core

*Board data, intents, log commands v1 (with the palette, home-place and picture settings), `Ctx` and `apply`, the origin's compile step, ids and stable block ids, conflict rules, rules versions, the canonical encoding v1 and the canonical board bytes, the zone-aware read queries, reminders and the legacy import. Owning stage: `core`. Conventions, notation and the glossary are in [`README.md`](README.md); this spec does not restate them.*

## 1. Scope

This spec fixes everything the `core` crate (`crates/taskdeck-core/`) computes: the replicated board state and its bytes, the only function that changes it, the conversion of a user action into replicated commands, and the questions the two user interfaces ask of the state. It realises:

- SYNC.md §4.4 (ids, `device_no` use, high-water marks, stable block ids, Restore's fallback), §4.5 (deterministic versioned `apply`, thin apply, `Ctx`, zones, Reflow and Unplan compiled, canonical encoding v1, display order), §4.6 (last writer wins, stamps and overwrite notices, `MoveBlocks` and `RemoveBlocks`, benign repeats, notes with a base), §4.1 (a replica loads `BoardData` as stored; normalisation only at import and genesis), §3 (the rows for notepad, weather and home place, palettes and the shared picture, reminders), §9 step 4 (the import check) and step 6 (the genesis `BoardData`);
- the verifier findings V2-1, V2-2, V2-3 (marks and minted ids), V2-4, V2-5, V2-16, V2-17 (the helper the naming hold needs), V2-19, V2-20 (the archive clamp from `Ctx.at`), the V2 determinism audit items (1) to (19), and V2 scenarios S1, S2, S7 and S15;
- R1 §1 (every determinism hazard), §2 (ids), §3 (the persistence split), §9 (board data against per-device data), §11 (session ids, archive keys, overlay forget as an effect, unreadable lines);
- SYNC_PLAN.md §2 (no feed: the `feed` and `board` queries are gone; no hub; reminders computed here), §3.3 ("Keep read-side names"), §3.4 (security rules), §6.1 item 3, §6.2 (the `core` stage and its gate), and what §6.9 (`desktop`) and §6.10 (`android`) need from the core;
- the rulings S2 (the genesis `BoardData`), S6 (no clock but the log's) and S7 (the canonical board bytes) of `docs/sync/status/foundation.md`.

Non-goals, each owned elsewhere:

- the confirmed and pending states, rebasing, the outbox, trial apply under `catch_unwind`, the naming hold, the persistent lists of notices and refusals, the clock-skew guard, halts: `engine.md`;
- frames, membership, the assignment of `device_no`, `RulesUpgrade` validity, the state-hash composition's inputs other than the board bytes: `seal.md` (and `engine.md` for the composition);
- files, the `Store` that writes today's files in local-only mode, the sealed checkpoint, overlay fetching, where a device's zone comes from, the `.lock` check: `device.md`;
- the phone page's routes, its parsing of replies, the changes to the forked page: `bridge.md`;
- display order, the notepad draft and merge view, the palette picker, the import-check command line: stage `desktop`.

## 2. Terms

| Term | Meaning |
|---|---|
| view | The `BoardData` the compile step reads: the origin's pending state (`engine.md`). |
| stamp | The pair `(seq, device_no)` of the entry that last changed a field group of an item or a block (§10.1). |
| minted id | An item, subscription or block id whose high part is the minting device's `device_no` (§7.1). |
| legacy id | An id below 2^40, from today's data (high part 0). |
| sid | A block id: the stable id of one booked block of a task (§7.3). Carried in the phone page's `session` field. |
| zone | A value implementing the `Zone` trait (§5.2): the caller's time zone. |
| local day, minutes | The calendar date and the minutes past midnight (0 to 1439) of an instant in a zone (§5.3). |
| benign repeat | A command whose intended end state already holds when it is applied; it succeeds and changes nothing (§10.6). |
| record schema | A table of fields defining a JSON record in the canonical encoding (§4.3). |
| scalar | A Unicode scalar value; lengths "in scalars" count them, as Rust's `str::chars().count()` does. |
| White_Space | The 25 code points U+0009 to U+000D, U+0020, U+0085, U+00A0, U+1680, U+2000 to U+200A, U+2028, U+2029, U+202F, U+205F and U+3000. |
| trim | Removing every leading and trailing White_Space code point, and nothing else. |
| detab | Replacing every U+0009 by four U+0020. |

## 3. Rules for every implementation

These hold for all code in `crates/taskdeck-core/` except its tests, benches and fuzz targets.

- **CORE-GEN-001** (core) The `core` crate MUST declare `#![forbid(unsafe_code)]` at its root.
- **CORE-GEN-002** (core) The `core` crate MUST compile for `wasm32-unknown-unknown` with its default features.
- **CORE-GEN-003** (core) The `core` crate MUST NOT read a clock, the file system, the network, environment variables, thread-local state or a random source, and MUST NOT spawn threads; every instant and zone it uses MUST be an argument of the call.
- **CORE-GEN-004** (core) The crate's clippy configuration MUST list `chrono::Local` and `chrono::offset::Local` under `disallowed-types`, and `chrono::Utc::now`, `chrono::Local::now`, `std::time::SystemTime::now` and `std::time::Instant::now` under `disallowed-methods`, and clippy MUST pass with these as errors.
- **CORE-GEN-005** (core) The `core` crate's non-development dependencies MUST be exactly `serde`, `serde_json` and `chrono` at the workspace's pinned versions.
- **CORE-GEN-006** (core) The functions `apply`, `upgrade_rules`, `reset`, `ids::rebind`, the canonical encoder and parser, `BoardData::check` and the id functions of §7 MUST NOT use `f32` or `f64` values, MUST NOT iterate a `HashMap` or `HashSet`, and MUST NOT depend on the order of any collection other than the orders this spec defines.
- **CORE-GEN-007** (core) Every public function that parses bytes or text (the canonical parser, the log-command parser, the board-bytes parser, the intent parser, the legacy import) MUST return an error, never panic, for any input; a property test over random and mutated inputs MUST prove it for each.
- **CORE-GEN-008** (core) `apply` MUST NOT panic for any `LogCommand` the parser of §9.1 returns and any `BoardData` that passes `BoardData::check` (§6.12); the fuzz target `command_apply` of §18.6 MUST prove it.
- **CORE-GEN-009** (core) Every parser MUST compare the input's length with the limit of §17 that applies to it before allocating anything for it, and MUST compute a base64 field's decoded length and check it against its limit before decoding it.
- **CORE-GEN-010** (core) An error, refusal, notice or report that the `core` crate returns MUST NOT contain an item or row name, the notes text, a subscription name or a subscription URL; the only exception is the intent parser's `core/intent_json` message (§16), which may quote the offending field text back to the user who typed it.

The core holds no secrets, so it has no zeroization or constant-time duties of its own. Subscription URLs are credentials (R1 §7); CORE-GEN-010 keeps them out of every diagnostic, and the sealed store (`device.md`) keeps them encrypted at rest.

## 4. Canonical encoding v1

The one JSON encoding every replicated or signed JSON value uses: log commands (§9), the board bytes (§6.13), the payloads of `seal.md` §11, §12 and §17.8, the sync metadata of `device.md`, notices and refusals (§10.7), and test vectors that carry such values. Other specs define their records with a schema table as in §4.3 and refer to this section for the bytes.

### 4.1 Values

A canonical value is one of: an object, an array, a string, an integer, `true` or `false`. JSON `null` and numbers with a fraction or an exponent do not occur.

### 4.2 Bytes

The encoding `enc(v)` of a value `v` is UTF-8 text without a byte-order mark, built as follows.

1. **No insignificant whitespace.** No byte U+0020, U+0009, U+000A or U+000D appears outside strings.
2. **Objects.** `{`, then the members separated by `,`, then `}`. A member is `enc(key)`, `:`, `enc(value)`. Members are in ascending order of their keys' UTF-8 bytes compared as unsigned byte strings, a proper prefix sorting first. No key appears twice.
3. **Arrays.** `[`, the elements separated by `,`, `]`, in the order the schema states.
4. **Strings.** `"`, the characters, `"`. A character is written as itself in UTF-8, except these, which are escaped:

   | Character | Written as |
   |---|---|
   | U+0022 `"` | `\"` |
   | U+005C `\` | `\\` |
   | U+0008 | `\b` |
   | U+0009 | `\t` |
   | U+000A | `\n` |
   | U+000C | `\f` |
   | U+000D | `\r` |
   | any other U+0000 to U+001F | `\u00` and two lowercase hexadecimal digits |

   No other character is escaped: `/`, U+007F and every character above it are written as themselves, and `\u` escapes of characters above U+001F never occur. (This is exactly what `serde_json`'s compact serializer writes.)
5. **Integers.** Decimal ASCII digits, with `-` before a negative value, no `+`, no leading zero except the value `0` itself, never `-0`. Every integer is within −(2^53 − 1) to 2^53 − 1; a schema may narrow the range.
6. **Literals.** `true` and `false`.

`enc` of a record (§4.3) is `enc` of the object its schema maps it to.

### 4.3 Record schemas and defaults

Every JSON record in this spec, and in each spec that uses this encoding, is defined by a table with the columns field, type, presence, constraints and meaning. Presence is one of:

- **required**: always present;
- **optional**: present with a valid value, or absent; absent means "none" and is the only way to say none;
- **default `d`**: absent exactly when the value equals `d`; an absent field means `d`. An empty array, an empty string or `false` is a default only where the table says so.

A record holds no field its table does not list. Arrays have the order their row states; where an array is a set, its row states the sort key, and duplicates under that key are not allowed unless the row says so.

### 4.4 Shared field types

| Type | JSON form | Rule |
|---|---|---|
| `int[a..b]` | integer | from `a` to `b` inclusive |
| `u53` | integer | 0 to 2^53 − 1 |
| `id` | integer | 0 to 2^53 − 1; minted and legacy ids as §7 states |
| `instant` | string, 30 bytes | `YYYY-MM-DDTHH:MM:SS.fffffffffZ`: year 1970 to 9999 (four digits), month 01 to 12, a day that exists in that month (Gregorian, proleptic), hour 00 to 23, minute 00 to 59, second 00 to 59 (never 60), a `.`, exactly nine fraction digits, uppercase `T` and `Z` |
| `date` | string, 10 bytes | `YYYY-MM-DD` with the year, month and day rules of `instant` |
| `text[a..b]` | string | from `a` to `b` scalars |
| `hexN` | string | exactly `2N` lowercase hexadecimal digits: `N` bytes |
| `base64` | string | RFC 4648 §4 with padding, decoding to the stated number of bytes; the encoding of those bytes is the only accepted text |
| `rgba` | array of 4 `int[0..255]` | red, green, blue, alpha |
| `stamp` | array of 2 integers | `[seq, device_no]`: `seq` `u53`, `device_no` `int[0..8191]` (§10.1) |
| `kind` | string | `"task"`, `"event"` or `"routine"` |

Ids, sids, seqs and every other integer are JSON numbers, never strings; every id is below 2^53, so JavaScript reads it exactly (README §5.3). Example: the instant 2026-09-04 09:00 UTC is `"2026-09-04T09:00:00.000000000Z"`; 2026-08-15T20:28:41.882008+03:00 is `"2026-08-15T17:28:41.882008000Z"`.

### 4.5 Parsing

A canonical parser for a record schema takes bytes and returns the record or an error, in this order:

1. the byte length is at most the limit for that record (§17), else `core/too_large`;
2. the bytes are valid UTF-8 and valid RFC 8259 JSON text with nesting depth at most 32, else `core/not_json`;
3. no object has a duplicate key, else `core/schema`;
4. the value matches the schema: no unknown field, every required field present, every field of its type and constraints, no `null`, no number with a fraction or exponent, else `core/schema`;
5. `enc` of the parsed record equals the input bytes exactly, else `core/not_canonical`.

Step 5 makes the encoding unique: whitespace, key order, escapes, a field written at its default, and the textual forms of instants and integers are all checked by it.

A string that holds a `\u` escape of a lone surrogate (`"\ud800"`) fails step 2 (`core/not_json`): it is RFC 8259 grammar but denotes no Unicode text. A record type names its own byte limit for step 1 (`Record::LIMIT`, §20).

**Tree budget.** A parser bounds what the parsed value costs before any schema is applied, with numbers that are the same on every platform. Against a budget of `max(32 × limit, 1,048,576)` it charges: for every string, member keys included, the number of bytes between its quotes as written, plus 16; for every array element, 32; for every object, 744 for each started group of five members (once for 1 to 5 members, twice for 6 to 10, nothing for an empty object). A text whose charges exceed the budget is `core/schema`, returned once the whole text has passed step 2. No value that a record schema of this spec admits within its limit comes near the budget: the densest record, a `Mark` of 34 bytes, is charged 810, under 24 times its length.

- **CORE-ENC-001** (core) The canonical encoder MUST produce exactly the bytes §4.2 defines for every value, with the members of every object in ascending unsigned-byte order of their keys.
- **CORE-ENC-002** (core) The canonical encoder MUST omit every field whose value equals the default its schema states and every optional field whose value is none, and MUST write every other field.
- **CORE-ENC-003** (core) The canonical encoder MUST escape exactly the characters of the table in §4.2 and MUST write every other character as itself.
- **CORE-ENC-004** (core) A canonical parser MUST perform the five steps of §4.5 in order and MUST return the code of the first that fails.
- **CORE-ENC-005** (core) A canonical parser MUST reject, with `core/schema`, an unknown field, a duplicate key, a missing required field, `null`, a number with a fraction or exponent, and a value outside its type's range.
- **CORE-ENC-006** (core) A canonical parser MUST reject, with `core/not_canonical`, input whose bytes differ from the canonical encoding of the value parsed from them.
- **CORE-ENC-007** (core) An instant MUST be encoded as the 30-byte form of §4.4 with exactly nine fraction digits, and a parser MUST reject any other text for an instant, including a second of 60, a lowercase `t` or `z`, an offset other than `Z`, and fewer or more fraction digits.
- **CORE-ENC-008** (core) The crate MUST expose the canonical encoder and parser as public functions over `serde_json::Value` (§20), so that `seal`, `engine` and `device` encode their own records with them.

## 5. Time, zones and planner arithmetic

### 5.1 Instants

An `Instant` is a UTC time with nanosecond resolution: a count of seconds since 1970-01-01T00:00:00Z (`i64`, leap seconds ignored) and a nanosecond part (`u32`, 0 to 999,999,999). Its range is 1970-01-01T00:00:00.000000000Z to 9999-12-31T23:59:59.999999999Z, both inclusive; nothing in the core constructs an instant outside it. Instants are ordered by seconds, then nanoseconds. An instant from a plaintext's `at_ms` (`seal.md` §10.1: 0 to 253,402,300,799,999, which is 9999-12-31T23:59:59.999Z, so the `at_ms` of every opened entry converts) is `seconds = at_ms div 1000`, `nanos = (at_ms mod 1000) × 1,000,000`. "Plus one day" adds 86,400 seconds; a sum after 9999-12-31T23:59:59.999999999Z is that instant (`Instant::MAX`).

### 5.2 Zones

The caller's zone reaches the core as a value implementing this trait, so the core needs no time-zone database and no new dependency:

```rust
pub trait Zone {
    /// Seconds east of UTC in force at `utc`; within −86,399 to 86,399.
    fn offset_at(&self, utc: Instant) -> i32;
    /// Every instant whose local wall time in this zone is `local`, ascending.
    fn from_local(&self, local: chrono::NaiveDateTime) -> LocalTime;
}
pub enum LocalTime { None, Single(Instant), Ambiguous(Instant, Instant) }   // earlier, later
```

The core provides three implementations:

| Type | Use | Behaviour |
|---|---|---|
| `FixedZone(i32)` | tests | a constant offset in seconds |
| `TableZone` | tests, vectors, a platform that hands over its rules | an `initial` offset and a list of transitions `(utc_ms, offset_s)` strictly ascending by `utc_ms`; the offset at `t` is that of the last transition with `utc_ms ≤ t`, else `initial`; `from_local(l)` returns, ascending, every instant `l − o` (with `o` the offset of a period) that falls inside the period whose offset is `o` |
| `ChronoZone<Z: chrono::TimeZone>` | devices | delegates to `chrono`: `offset_at` is `Z::offset_from_utc_datetime(..).fix().local_minus_utc()`, `from_local` maps `Z::from_local_datetime` (`LocalResult::None`, `Single`, `Ambiguous(a, b)`) |

A device passes `ChronoZone(chrono::Local)` or a `TableZone`; `device.md` chooses, at run time, and may pass it as `&dyn Zone`: every zone parameter of this crate is `&(impl Zone + ?Sized)` or a generic `Z: Zone + ?Sized` (§20). Inside the core the type `chrono::Local` never appears (CORE-GEN-004).

`TableZone::new` returns `core/schema` for transitions that are not strictly ascending by `utc_ms` and for an offset outside −86,399 to 86,399. When more than two instants share a local wall time (transitions less than a day apart), `TableZone::from_local` returns the first and the last as `Ambiguous`. `ChronoZone::from_local` returns the instants ascending whatever order `chrono` lists them in, and only those that `offset_at` maps back to `local`: chrono 0.4.45's `Local` lists the later instant of a fold first, and reports a fold at the first minute after one.

### 5.3 Local day and minutes

For an instant `t` and a zone `z`, the local date-time is `t + z.offset_at(t)` read as a naive date-time. Its date is the **local day** of `t`, and `hour × 60 + minute` (seconds and fractions dropped) its **minutes**. `t` **falls on** day `d` when its local day is `d`. These replace every `date_naive()`, `hour()` and `minute()` that today's code calls on a `DateTime<Local>`.

### 5.4 Resolving a local time

`resolve_on_day(d, m, z)` turns minutes past midnight on day `d` into an instant, exactly as today's `planner::resolve_on_day` (planner.rs:296-311) does with `z` in place of `Local`:

1. `m' = clamp(m, 0, 1439)`;
2. `local_at(d, x)` is the naive date-time `d` at hour `min(x div 60, 23)`, minute `x mod 60`, second 0; its instant is the earlier one for `Ambiguous`, the one for `Single`, none for `None`;
3. the result is `local_at(d, m')`, or, when that is none, `local_at(d, min(m' + 60, 1439))`, or none.

On a fold the core takes the earlier instant in every zone. Today's code takes `chrono`'s `.earliest()`, which for `Local` is the later one (§5.2); §13.5 says how the recorded transcripts, which hold the later one, are replayed.

### 5.5 Planner arithmetic in integers

Today's `snap` and `clamp_block` use `f32` on integer inputs (planner.rs:260-279). The core computes the same results in integers; all arithmetic below is on `i64`.

- `snap(m)`: if `m ≤ 0` then 0; else `min((m + 7) div 15, 96) × 15`. (For an integer `m`, `m / 15` never has a fractional part of exactly one half, so this equals today's round-half-away-from-zero, clamped to 0 to 96 steps.)
- `clamp_block(start, minutes) → (start', length)`: `length = max(snap(max(minutes, 15)), 15)`; `start' = clamp(snap(start), 0, 1440 − length)`.
- `snap_up(m) = clamp(ceil(m / 15) × 15, 0, 1440)`, with the ceiling taken by Euclidean division as planner.rs:653-656 does.

Examples (they are vectors, §18): `clamp_block(547, 100) = (540, 105)`; `clamp_block(i32::MIN, u32::MAX) = (0, 1440)`; `clamp_block(1440, 30) = (1410, 30)`; `clamp_block(1425, 60) = (1380, 60)`; `clamp_block(1430, 5) = (1425, 15)`; `snap(7) = 0`; `snap(8) = 15`; `snap_up(1) = 15`; `snap_up(1441) = 1440`.

`now_marker(d, now, z)` is the minutes of `now` when `now` falls on `d`, else none.

- **CORE-TIME-001** (core) An `Instant` MUST lie within the range of §5.1, and every function that produces one from outside data (the canonical parser, `at_ms`, the intent parser, the zone conversions) MUST return an error, never an out-of-range instant.
- **CORE-TIME-002** (core) Every read query and compile function that needs a local day or local minutes MUST compute them from an instant and the caller's `Zone` as §5.3 states, and MUST NOT take a local date-time from anywhere else.
- **CORE-TIME-003** (core) `resolve_on_day` MUST follow the three steps of §5.4, taking the earlier instant of an ambiguous local time and stepping once by 60 minutes over a gap.
- **CORE-TIME-004** (core) `snap`, `clamp_block` and `snap_up` MUST be computed in integer arithmetic exactly as §5.5 states, and MUST return the values listed there for the listed inputs.
- **CORE-TIME-005** (core) `TableZone` MUST compute offsets and local candidates exactly as the table of §5.2 states.

## 6. Board data

### 6.1 `BoardData`

`BoardData` is the complete replicated state of a board, everything `apply` reads or writes. It is one record of the canonical encoding (§4.3); its canonical bytes feed the state hash (§6.13). Field order in the table is alphabetical, as in the bytes.

| Field | Type | Presence | Constraints | Today's data | Meaning |
|---|---|---|---|---|---|
| `archive` | array of `ArchiveRow` (§6.6) | default `[]` | ascending by the archive order (§6.12); identical rows may repeat; at most 200,000 | yes: `archived.jsonl` | retired items, finished or dropped |
| `home` | `Home` (§6.9) | default `{"lat":0,"lon":0}` | | as a per-device setting: `coordinates` in `userconfig.toml`, `[f32; 2]` | the home place for weather, 0.01° |
| `items` | array of `Item` (§6.2) | default `[]` | ascending by `id`, ids distinct; at most 10,000 | yes: `read_at_startup.json` | live tasks, events and routines |
| `marks` | array of `Mark` (§6.10) | default `[]` | ascending by `device_no`, distinct | no (today one `next_id` counter in memory) | per-device high-water marks of minted numbers |
| `notes` | string | default `""` | at most 1,048,576 bytes; contains no U+0009 | yes: `notepad_text.json` | the notepad |
| `notes_rev` | `u53` | default `0` | | no | the notes revision; the base of `set_notes` (§10.5) |
| `palette` | array of 6 `rgba` | default six `[0,0,0,0]` | exactly 6 | as a per-device setting: `colorschemes.json` and `selected_colorscheme_id` | the board's six item colours: slots 0 to 4 the severity and horizon ramp, 5 events |
| `picture` | `PictureRef` (§6.9) | optional | | no (today `phone_background.img`, per device) | the shared picture, a sealed blob |
| `rules` | `int[1..2147483647]` | required | a version this build supports | no | the active rules version (§11) |
| `subscriptions` | array of `Subscription` (§6.8) | default `[]` | ascending by `id`, ids distinct, URLs distinct; at most 20 | yes: `subscriptions.json` | subscribed calendars |
| `unreadable` | array of `base64` | default `[]` | each 1 to 65,536 bytes; ascending by decoded bytes (unsigned, a proper prefix first); identical lines may repeat; at most 10,000 | yes: archive lines that do not parse, kept as bytes (archive.rs:641) | opaque archive lines, carried and never interpreted |

`BoardData` holds no version counter (it is the seq, `engine.md`), no overlay (per device, §14.6), no next-id counter (replaced by `marks`), no per-device settings (backgrounds, dials, window, scale, server settings, tokens) and no time zone (zones are per device, SYNC_PLAN.md §2).

### 6.2 `Item`

| Field | Type | Presence | Constraints | Today (`Active`, tasks.rs:169-237) | Meaning |
|---|---|---|---|---|---|
| `blocks` | array of `Block` (§6.3) | default `[]` | ascending by `sid`, distinct; at most 1,000 | `sessions` (index-addressed, no ids) | a task's booked blocks |
| `created` | `instant` | required | | `created` (local offset) | when the item was created: the creating entry's `Ctx.at` |
| `deadline` | `instant` | optional | | `deadline` | a task's due time; an event's start |
| `duration_minutes` | `int[0..1440]` | optional | | `duration_minutes` | a task's estimate; an event's length |
| `id` | `id` | required | §7.1; not of the form `k × 2^40` | `id` | the item id |
| `importance` | `int[0..4]` | optional | | `importance` | severity |
| `kind` | `kind` | required | `routine` exactly when `recurrence` is present | derived from `is_event` and `recurrence` | task, event or routine |
| `name` | string | required | at most 4,096 bytes | `name` | |
| `recurrence` | `Recurrence` (§6.4) | optional | present exactly when `kind` is `routine` | `recurrence` | a routine's rule |
| `stamps` | `ItemStamps` (§6.5) | required | | no | last-change stamps per field group |
| `time_importance` | `int[0..3]` | optional | | `time_importance` | horizon |

`planned_start` is gone: the import folds it into a block (§15.3). An item keeps every field it holds whatever its kind, as today: an event may carry `importance` (`Command::Add` stores it, board.rs:960-987), and a legacy routine may carry blocks or a deadline. Commands change only the fields their kind uses (§9).

### 6.3 `Block`

| Field | Type | Presence | Constraints | Today (`Session`) | Meaning |
|---|---|---|---|---|---|
| `minutes` | `int[0..1440]` | required | | `minutes` | its length |
| `sid` | `id` | required | §7.3 | none (addressed by index) | the stable block id |
| `stamp` | `stamp` | required | | no | the last change to this block |
| `start` | `instant` | required | | `start` (local offset) | its start |

### 6.4 `Recurrence`

| Field | Type | Presence | Constraints | Today | Meaning |
|---|---|---|---|---|---|
| `anchor` | `date` | required | | `anchor` (serde default 1970-01-01) | the one day a non-repeating rule falls on |
| `days` | `int[0..127]` | required | | `days` | bit 0 Monday to bit 6 Sunday; 0 is a one-off |
| `minutes` | `int[0..1440]` | required | | `minutes` | length |
| `start_minutes` | `int[0..1439]` | required | | `start_minutes` | start, minutes past local midnight, floating: each device reads it in its own zone |

### 6.5 `ItemStamps`

Five stamps (§10.1), each `stamp`, all required: `estimate` (the task estimate), `levels` (`importance`, `time_importance`), `name`, `repeat` (`recurrence.days`, `recurrence.anchor`), `time` (a task's `deadline`; an event's `deadline` and `duration_minutes`; a routine's `start_minutes` and `minutes`). Today's data has none.

### 6.6 `ArchiveRow`

| Field | Type | Presence | Constraints | Today (`Archived`, archive.rs:114-155) | Meaning |
|---|---|---|---|---|---|
| `archived_at` | `instant` | required | | `archived_at`, written `inactivated` | when it was retired; with `id`, the row's key |
| `blocks` | array of `RowBlock` | default `[]` | ascending by `sid`, distinct; at most 1,000 | `sessions` | the blocks it had |
| `created` | `instant` | required | | `created` | |
| `deadline` | `instant` | optional | | `deadline` | |
| `duration_minutes` | `int[0..1440]` | optional | | `duration_minutes` | |
| `id` | `id` | required | 0 for a pre-id legacy row; otherwise §7.1 | `id` | the item's id when retired |
| `importance` | `int[0..4]` | optional | | `importance` | |
| `kind` | `kind` | required | `routine` exactly when `recurrence` is present | derived | |
| `name` | string | required | at most 4,096 bytes | `name` | |
| `outcome` | string | default `"finished"` | `"finished"` or `"dropped"` | `outcome` (serde default `Finished`) | |
| `recurrence` | `Recurrence` | optional | present exactly when `kind` is `routine` | `recurrence` | |
| `time_importance` | `int[0..3]` | optional | | `time_importance` | |

`RowBlock` is `{"minutes": int[0..1440], "sid": id, "start": instant}`, all required: a `Block` without its stamp. A row has no stamps and never changes; it is only inserted and removed.

### 6.7 Unreadable lines

An unreadable line is the exact bytes of one line of a legacy `archived.jsonl` that was not UTF-8 or did not parse (§15.3), without its line terminator. The core never interprets one, and no command adds, changes or removes one. They are carried so that nothing is ever discarded (archive.rs:1209), and appear in the import report as a count.

### 6.8 `Subscription`

| Field | Type | Presence | Constraints | Today (subscriptions.rs:137-147) | Meaning |
|---|---|---|---|---|---|
| `color` | `rgba` | required | | `color` | the overlay colour, independent of the palette |
| `enabled` | boolean | default `true` | | `enabled` | |
| `id` | `id` | required | §7.1, from the item space | `id` | |
| `name` | string | required | at most 4,096 bytes | `name` | |
| `url` | string | required | 1 to 8,000 bytes | `url` | an `https://` address; a credential |

### 6.9 Settings

**Palette.** Six `rgba` slots, indexed as `calendar_item_color` indexes them (tasks.rs:594-613): 0 to 4 run from least to most important, 5 is events. All six `[0,0,0,0]` is the default and means what an all-transparent palette means today: the desktop draws its untinted default (`ColorScheme::default_scheme`, color.rs) and the phone page uses its own tints (phone.rs:2063-2064). Per-device colour schemes and their names stay on each desktop; `set_palette` publishes the colours of the scheme a desktop chose (stage `desktop`).

**`Home`.** `{"lat": int[-9000..9000], "lon": int[-18000..18000]}`, both required, in hundredths of a degree (latitude north positive, longitude east positive). `{0, 0}` is the default, as today's `coordinates` default `[0.0, 0.0]` is. Each device asks for weather at `lat / 100`, `lon / 100` (SYNC.md §3); the phone's own place never enters the board.

**`PictureRef`.** The shared picture, uploaded by a desktop as a blob of kind 2 (`seal.md` §20):

| Field | Type | Presence | Constraints | Meaning |
|---|---|---|---|---|
| `blob` | `hex16` | required | | the blob id |
| `epoch` | `int[1..4294967295]` | required | | the epoch whose `K_blob,e` sealed it |
| `parts` | `int[1..64]` | required | `ceil(size / 1,048,576)` | number of parts |
| `sha256` | `hex32` | required | | SHA-256 of the sealed bytes |
| `size` | `int[67..67108864]` | required | | length of the sealed bytes |

These are the fields `seal.md` §25 requires of a command that references a blob; a reader checks them as `seal.md` §20.5 states before decrypting.

### 6.10 `Mark`

| Field | Type | Presence | Constraints | Meaning |
|---|---|---|---|---|
| `block` | `int[0..1099511627775]` | required | | the highest block number `n` this `device_no` has had accepted |
| `device_no` | `int[0..8191]` | required | | 0 is the legacy space (§7.2) |
| `item` | `int[0..1099511627775]` | required | | the highest item or subscription number `n` accepted |

A mark with both numbers 0 is not stored.

### 6.11 The archive key

A row's key is `(id, archived_at)`. Commands name a row by its key (`restore`, `forget_archived`). Two rows may share a key; a command that names a key acts on the first matching row in the archive order (§6.12). (Today `take` removes the first match in memory order, archive.rs:754-769, which differs between devices for equal timestamps; the total order fixes that, V2 audit item 8.)

### 6.12 Orders and invariants

**Archive order.** Row `a` precedes row `b` when `a.archived_at < b.archived_at`; or they are equal and `a.id < b.id`; or both are equal and `enc(a)` precedes `enc(b)` as unsigned bytes (a proper prefix first). "Newest first" in read queries is the reverse of this order.

`BoardData::check` holds exactly when every one of these is true:

1. the record parses under its schema (§6.1 to §6.10), every array is in its stated order without forbidden duplicates, and no mark has both numbers 0 (§6.10);
2. every count is within its cap (§17);
3. every live item id, every subscription id, every non-zero archive-row id and every sid has a non-zero low part (`n = id mod 2^40 ≥ 1`) and a high part `id div 2^40` from 0 to 8191;
4. for every such id or sid with high part `h` and low part `n`, the mark of `h` has `item ≥ n` (items, rows, subscriptions) or `block ≥ n` (sids);
5. the sids of all live items' blocks are pairwise distinct;
6. `kind = routine` exactly when `recurrence` is present, in every item and row;
7. `notes` contains no U+0009;
8. `picture.parts = ceil(picture.size / 1,048,576)` when `picture` is present;
9. `rules` is a version this build supports (§11).

### 6.13 Canonical board bytes and the state hash

The **canonical board bytes** of a `BoardData` are `enc(BoardData)` (§4.2). They are what `seal.md` §20.4 puts in a snapshot body, what `seal.md` §21.5 puts in a nightly backup, and the input of `SHA-256(canonical board)` in the state hash of ruling S7, whose composition `engine.md` owns. Hash version 1 is: canonical encoding v1 of this section's `BoardData`, with `seal.md` §13.2's membership bytes. A later change to either is a new hash version (`seal.md` §14.3).

### 6.14 Loading

A replica loads `BoardData` only from canonical board bytes, exactly as stored: it runs no id assignment, no plan migration, no detab and no default substitution (V2-19). Those run only in the legacy import (§15).

- **CORE-DATA-001** (core) `BoardData` and every record it contains MUST be encoded and parsed with exactly the fields, types, presences and constraints of §6.1 to §6.10.
- **CORE-DATA-002** (core) The canonical board bytes MUST be `enc(BoardData)` with every array in the order §6.1 to §6.12 states, so that two equal states give equal bytes.
- **CORE-DATA-003** (core) The archive MUST be kept in the archive order of §6.12 at all times, and an inserted row MUST be placed at the position that order gives it.
- **CORE-DATA-004** (core) `BoardData::from_canonical_bytes` MUST accept input only if the canonical parser of §4.5 accepts it and `BoardData::check` holds for the result, and MUST return `core/invariant` naming the first failing rule of §6.12 otherwise.
- **CORE-DATA-005** (core) Loading `BoardData` from canonical bytes MUST NOT change any field: `to_canonical_bytes(from_canonical_bytes(b))` MUST equal `b` for every accepted `b`.
- **CORE-DATA-006** (core) `apply` MUST leave a `BoardData` that passes `check` whenever it is given one that passes `check`.
- **CORE-DATA-007** (core) The core MUST NOT store in `BoardData` any per-device value: overlay, zone, view counter, backgrounds, dials or tokens.

## 7. Ids

### 7.1 Item ids

An item id, a subscription id and a non-zero archive-row id each have the form `h × 2^40 + n`, with `h` from 0 to 8191 and `n` from 1 to 2^40 − 1, so every id is below 2^53 and exact in JavaScript (SYNC.md §4.4, README §5.6). `h ≥ 1` is the `device_no` of the device that minted it (`seal.md` §13.7). `h = 0` is the legacy space: ids from today's data, below 2^40, kept unchanged by the import except where §15.3 renumbers them. Ids with `h ≥ 1` in today's files, written there by local-only mode (`device.md` §9.2), "Export legacy" (`device.md` §18.7) or the recovery tool (`device.md` §15.8), are kept unchanged by the import as well (§15.3 step 9.4). Items and subscriptions share one numbering (today's shared `next_id`, board.rs:1332), so a number is minted once for either.

### 7.2 Marks

`marks` records, per `device_no`, the highest item number and the highest block number accepted from that device. A creating command carries the id it creates; `apply` accepts a newly minted id only if its high part is the author's `device_no` and its `n` is above that device's mark, and then raises the mark to `n` (SYNC.md §4.4, V2-3). So no id is ever issued twice, even after Forget, after a refused entry, or from a restored folder whose own counter went back. (Local-only mode stores no marks: it rebuilds them from today's files at each open, `device.md` §9.2.) The legacy space's mark (`device_no` 0) is set by the import to the highest legacy numbers and never moves again, because no author has `device_no` 0.

The origin mints with an `IdMinter`: the next item number is `max(minter.next_item, mark(view, device_no).item + 1)`, and after minting `minter.next_item` becomes that number plus one; blocks likewise with `next_block`. The device persists the minter with its outbox (`device.md`), so a number is never minted twice locally even when the entry that carried it is later refused.

`IdMinter::new(device_no)` (§20) is `{ device_no, next_item: 1, next_block: 1 }`: the minter a device starts
with when it creates a board or joins one, so its first numbers are `mark + 1` of the view.

### 7.3 Block ids

A block id (sid) has the same form, `h × 2^40 + n`, with its own marks (`block`). sids are minted by the origin for `add_block` and `create_task`, and by the import for legacy sessions (`h = 0`). A sid is unique among the live blocks of the whole board, not only within its item. It is carried in the phone page's existing `session` field and in `SessionDetail.index` (§14.3); the page only compares and echoes it (phone.html:4260, 4365, 4909-4931, 4939, 5066-5083), so its meaning changes from an index to an id without any change to the page's code. An archived row keeps its blocks' sids, and Restore brings them back unchanged.

### 7.4 Restore

A `restore` carries the row key and a `fallback` id minted by the origin. `apply` restores under the row's own id when it is not 0 and no live item holds it; otherwise under `fallback` (SYNC.md §4.4, R1 §2). The range check of §7.2 applies to `fallback` only, never to the row's own id, which was valid when it was minted (V2-3).

- **CORE-ID-001** (core) Every id and sid in `BoardData`, except the id 0 of a pre-id archive row, MUST have the form of §7.1 with a non-zero low part and a high part from 0 to 8191.
- **CORE-ID-002** (core) `apply` MUST refuse with `core/bad_id` a command that mints an item, subscription or block id whose high part differs from `Ctx.device_no` or whose low part is not above the author's mark for that kind.
- **CORE-ID-003** (core) `apply` MUST raise the author's mark to the low part of every id it accepts as newly minted, including a `restore`'s `fallback` whether or not the restore used it. A benign repeat (§10.6) accepts no id and raises no mark.
- **CORE-ID-004** (core) The compile step MUST mint each number as `max(minter.next, mark + 1)` of §7.2 and MUST advance the minter past it, so that one minter never yields the same number twice.
- **CORE-ID-007** (core) `IdMinter::new(device_no)` MUST return a minter with that `device_no`, `next_item = 1` and `next_block = 1`.
- **CORE-ID-005** (core) `apply` MUST restore a row under its own id when that id is not 0 and not held by a live item, and under the command's `fallback` otherwise, without applying the range check to the row's own id.

### 7.5 Rebinding

When a device pairs again as a new identity, the commands its old identity left unsent are resubmitted under the new `device_no` (`engine.md` §24.5). `rebind(commands, from_device_no, view, minter)` rewrites them; `minter` is the new identity's current `IdMinter` (its `device_no` is the new one) and `view` its pending state. Two maps start empty: the item map (item and subscription ids) and the block map (sids). For each command in order:

1. **Minting fields**, in this order: `create_task`: `id`, then each `blocks[].sid` in array order; `create_event` and `create_routine`: `id`; `add_block`: `sid`; `restore`: `fallback`; `add_subscription`: `id`. Each value whose high part is `from_device_no` is replaced by a new number, `minter.mint_item(view)` for `id` and `fallback`, `minter.mint_block(view)` for a sid, and the pair is added to the item map or the block map. A value with another high part is kept.
2. **Referring fields.** Every value equal to a key of the matching map is replaced by its value. Item-space references: the `id` of `rename`, `complete`, `delete`, `forget`, `move_block`, `add_block`, `set_estimate`, `set_deadline`, `set_severity`, `set_horizon`, `set_repeat`, `move_routine`, `set_event_span`, `restore`, `forget_archived`, `remove_subscription`, `rename_subscription`, `set_subscription_color` and `set_subscription_enabled`; `moves[].id` of `move_blocks`; `blocks[].id` of `remove_blocks`. Sid references: the `sid` of `move_block`; `moves[].sid` of `move_blocks`; `blocks[].sid` of `remove_blocks`.
3. The arrays that §9 orders by sid (`create_task.blocks`, `remove_blocks.blocks`) are sorted again ascending by sid. Every other value is kept, so each returned command parses (§9.1).

An id the old identity minted in an entry that was accepted is not in either map, so references to it are kept. `rebind` returns `core/bad_id` when the minter has no number left (§7.1); the minter is then unchanged.

- **CORE-ID-006** (core) `rebind` MUST replace, in command order, every value a minting field of §7.5 holds with high part `from_device_no` by a number from `minter` (`mint_item` for item-space values, `mint_block` for sids), MUST replace every referring field of §7.5 that equals a value replaced so far in the same space by its replacement, and MUST leave every other value unchanged.

## 8. `Ctx` and `apply`

### 8.1 `Ctx`

```rust
pub struct Ctx { pub seq: u64, pub device_no: u16, pub at: Instant, pub based_on: u64 }
```

| Field | Source (filled by the engine) | Constraint | Read by `apply` for |
|---|---|---|---|
| `seq` | the entry's seq; for an entry applied to the pending state, the provisional seq `engine.md` assigns | 2 to 2^53 − 1 | every stamp it writes (§10.1); a notice's `seq` |
| `device_no` | the author's `device_no` in the membership before the entry (`seal.md` §13.7) | 1 to 8191 | minted-id checks and marks (§7); stamps; whether a stamp is another device's (§10.2) |
| `at` | the plaintext's `at_ms` (§5.1): the origin's clock when it compiled the command | within §5.1's range; every opened entry's `at_ms` converts (`seal.md` §10.1) | `created` of a created item; the archive clamp of `complete` and `delete` |
| `based_on` | the plaintext's `based_on_seq`: the origin's confirmed head when it compiled (V2-5) | below `seq` | which stamps are "since the origin's view" (§10.2, §10.3) |

### 8.2 What `apply` reads

`apply(board, command, ctx)` reads exactly: every field of `board`, every field of `command`, and the four fields of `ctx`. It reads nothing else: no clock, zone, overlay, file, environment, random value, floating-point value, hash-map order or memory address (SYNC.md §4.5, CORE-GEN-003, CORE-GEN-006). Its result depends on collection order only through the orders of §6.12, which are part of the state. `apply` never consults the version of the build it runs in, except through `board.rules` (§11).

### 8.3 Result

```rust
pub struct Applied {
    pub benign: bool,              // a benign repeat (§10.6): nothing changed
    pub changes: ChangeSet,
    pub notices: Vec<Notice>,      // §10.7, in the order produced
    pub moved: u32,                // move_blocks: blocks moved
    pub skipped: u32,              // move_blocks: blocks skipped (§10.3)
    pub restored_as: Option<u64>,  // restore: the id the item came back under
}
pub struct ChangeSet {
    pub items: BTreeSet<u64>,                 // live items put or removed
    pub archive_inserted: Vec<ArchiveKey>,
    pub archive_removed: Vec<ArchiveKey>,
    pub notes: bool,
    pub subscriptions: BTreeSet<u64>,         // put or removed
    pub overlay_forget: BTreeSet<u64>,        // drop this subscription's overlay events (removed or disabled)
    pub settings: bool,                       // palette, home or picture
    pub marks: bool,
}
pub struct Refusal { pub code: &'static str, pub op: &'static str, pub target: Option<u64>,
                     pub notes: Option<NotesConflict> }
pub struct NotesConflict { pub base: u64, pub current: u64 }
```

The change set lets `device.md`'s local-only `Store` write today's files in today's order (archive first, then items, R1 §3), and tells a device to drop the overlay events of a removed or disabled subscription: today's `overlay.forget` inside `apply` (board.rs:1355, 1393) becomes this effect (V2 audit item 13). In synced mode `apply` runs inside the engine, which does not pass the change set on (`engine.md` §5.1 `Applied`); the device then drops the overlay of every subscription that is absent or disabled in the new pending view (`device.md` §13.1.3), which is the same set.

### 8.4 Evaluation

`apply` evaluates in this order and stops at the first failure:

1. `ctx` satisfies §8.1's constraints, else `core/bad_ctx`;
2. the command's `op` is in the op table of `board.rules` (§11.1), else `core/rules`;
3. the command's checks, in the order its section in §9 lists them;
4. the size check: the command's effects would leave canonical board bytes (§6.13) that are over 61,865,984 bytes (59 MiB) and longer than before the command, which is `core/cap`;
5. the command's effects, which cannot fail.

The caps of §9 do not bound the board's bytes on their own (200,000 archive rows of 300 bytes are 60 MB, and unreadable lines can be more), so step 4 does: no sequence of commands grows a board past 59 MiB. That keeps it loadable (§17: 60 MiB) with room for what `reset` adds (§11.3: at most 8,191 marks, under 0.6 MB). A command that does not lengthen the board is never refused by step 4, so a board can always be made smaller. An implementation may use any method that gives exactly this result; a bound that is cheap to compute and never below the true length lets it skip the exact count for all but the largest boards.

A refusal leaves `board` unchanged. An `Applied` with `benign` set has an empty change set and no notices.

- **CORE-CTX-001** (core) `apply` MUST refuse with `core/bad_ctx`, changing nothing, when `ctx.device_no` is outside 1 to 8191, `ctx.seq` is outside 2 to 2^53 − 1, or `ctx.based_on ≥ ctx.seq`.
- **CORE-CTX-002** (core) `apply`'s result and the `BoardData` it leaves MUST be a function of `board`, `command` and the four fields of `ctx` alone: two calls with equal inputs MUST return equal results and leave equal canonical board bytes, under any process time zone, clock and platform.
- **CORE-CTX-003** (core) `apply` MUST set `created` of every item it creates to `ctx.at`, and MUST NOT take an instant from anywhere but `ctx.at` and the command.
- **CORE-APPLY-001** (core) `apply` MUST evaluate the steps of §8.4 in order and MUST return the refusal code of the first check that fails.
- **CORE-APPLY-002** (core) A refused `apply` MUST leave the canonical board bytes unchanged.
- **CORE-APPLY-003** (core) A successful `apply` MUST report in its change set every live item, archive key, subscription and setting it changed, and nothing else; and MUST list in `overlay_forget` every subscription it removed or set to disabled.
- **CORE-APPLY-004** (core) A benign repeat MUST return `Applied` with `benign` set, an empty change set and no notices, and MUST leave the canonical board bytes unchanged.
- **CORE-APPLY-005** (core) `apply` MUST refuse with `core/cap`, changing nothing, a command whose effects would leave canonical board bytes that are over 61,865,984 bytes and longer than before the command, and MUST NOT refuse for size a command that does not lengthen them.

## 9. Log commands v1

### 9.1 Envelope and parsing

A log command is one record of the canonical encoding with a field `op` naming it. It is the whole `cmd` payload of one entry (`seal.md` §11.2): one entry carries exactly one command. Its byte length is 1 to 257,979 (`seal.md` §10.2 cap).

`parse_command(bytes)` performs, in order:

1. the length check of §4.5 step 1 (limit 257,979 bytes), else `core/too_large`;
2. §4.5 step 2, else `core/not_json`;
3. §4.5 step 3 (no duplicate key anywhere), else `core/schema`;
4. the value is an object with a string field `op`, and `op` names a command of §9.3, else `core/unknown_op`;
5. §4.5 step 4 against that command's schema, else `core/schema`;
6. §4.5 step 5, else `core/not_canonical`;
7. the command's cross-field constraints in its section ("Schema rule"), else `core/schema`.

A text over the tree budget of §4.5 is `core/schema` whatever its `op`: the budget is spent before step 4 reads one.

A command that parses is not yet valid against the board: that is `apply`'s business (§8.4). The engine maps a parse failure of a sequenced `cmd` entry to a refusal on every device (`seal.md` §14.4, `engine.md`); it is not an `apply` refusal and changes nothing.

### 9.2 Shared field types of commands

| Type | Rule |
|---|---|
| `name` | `text[1..200]`: 1 to 200 scalars. `apply` does not trim and checks nothing else about the characters (V2-1). |
| `len` | `int[15..1440]`: a length in minutes. |
| `day_start` | `int[0..1425]`: minutes past local midnight. |

Every other type is from §4.4.

### 9.3 The commands

| `op` | Fields | Target kinds | Mints | Stamps written | Notices | Benign repeat |
|---|---|---|---|---|---|---|
| `create_task` | `id`, `name`, `deadline`?, `importance`?, `time_importance`?, `duration_minutes`?, `blocks` | — | item, blocks | all | — | — |
| `create_event` | `id`, `name`, `start`, `minutes`?, `importance`?, `time_importance`? | — | item | all | — | — |
| `create_routine` | `id`, `name`, `days`, `anchor`, `start_minutes`, `minutes` | — | item | all | — | — |
| `rename` | `id`, `name` | any | | `name` | overwrite | — |
| `complete` | `id`, `at` | task | | | overwrite (`item`) | id archived, not live |
| `delete` | `id`, `at` | any | | | overwrite (`item`) | id archived, not live |
| `forget` | `id` | any | | | overwrite (`item`) | id not live |
| `move_block` | `id`, `sid`, `start`, `minutes` | task | | block | overwrite (`block`) | — |
| `move_blocks` | `moves` | task | | blocks | skipped | — |
| `add_block` | `id`, `sid`, `start`, `minutes` | task | block | block | — | — |
| `remove_blocks` | `blocks` | task | | | overwrite (`block`) | none present |
| `set_estimate` | `id`, `minutes` | task, event | | `estimate` (task), `time` (event) | overwrite | — |
| `set_deadline` | `id`, `deadline`?, `seed`? | task | | `time`, and `levels` when seeded | overwrite (`time`) | — |
| `set_severity` | `id`, `level` | task | | `levels` | overwrite | — |
| `set_horizon` | `id`, `level` | task | | `levels` | overwrite | — |
| `set_repeat` | `id`, `days`, `anchor` | routine | | `repeat` | overwrite | — |
| `move_routine` | `id`, `start_minutes`, `minutes` | routine | | `time` | overwrite | — |
| `set_event_span` | `id`, `start`, `minutes` | event | | `time` | overwrite | — |
| `set_notes` | `text`, `base` | — | | | — (refused when stale) | — |
| `restore` | `id`, `archived_at`, `fallback` | — | item (`fallback`) | all | — | row gone, item live |
| `forget_archived` | `id`, `archived_at` | — | | | — | row gone |
| `add_subscription` | `id`, `name`, `url`, `color` | — | item space | | — | — |
| `remove_subscription` | `id` | — | | | — | not subscribed |
| `rename_subscription` | `id`, `name` | — | | | — | — |
| `set_subscription_color` | `id`, `color` | — | | | — | — |
| `set_subscription_enabled` | `id`, `enabled` | — | | | — | — |
| `set_palette` | `colors` | — | | | — | — |
| `set_home` | `lat`, `lon` | — | | | — | — |
| `set_picture` | `picture`? | — | | | — | — |

In the sections below, "the item" is the live item whose `id` is the command's `id`; "gone" means there is no such item and the code is `core/gone`; "kind" means the item's kind is not among the target kinds and the code is `core/kind`; "mint check" is CORE-ID-002 for the named id (`core/bad_id`); "stamp" means setting the named stamp to `(ctx.seq, ctx.device_no)`. Every field without "optional" or a default is required.

### 9.4 `create_task`

| Field | Type | Presence | Constraints |
|---|---|---|---|
| `blocks` | array of `{minutes: len, sid: id, start: instant}` | default `[]` | 0 to 1,000; ascending by `sid`, distinct |
| `deadline` | `instant` | optional | |
| `duration_minutes` | `len` | optional | |
| `id` | `id` | required | |
| `importance` | `int[0..4]` | optional | |
| `name` | `name` | required | |
| `op` | `"create_task"` | required | |
| `time_importance` | `int[0..3]` | optional | |

Apply: (1) mint check of `id`; (2) mint check of every `sid`; (3) fewer than 10,000 live items, and the live blocks plus the new ones at most 50,000, else `core/cap`. Effects: insert the item with `kind` task, the given fields, `created = ctx.at`, each block with `stamp` = the entry's stamp, and every field stamp = the entry's stamp; raise the item mark to `id`'s low part and the block mark to the largest new sid's low part.

### 9.5 `create_event`

Fields: `op` `"create_event"`; `id` `id`; `name` `name`; `start` `instant`; `minutes` `len`, optional; `importance` `int[0..4]`, optional; `time_importance` `int[0..3]`, optional.

Apply: (1) mint check of `id`; (2) fewer than 10,000 live items, else `core/cap`. Effects: insert the item with `kind` event, `deadline = start`, `duration_minutes = minutes`, the levels given, no blocks, `created = ctx.at`, all stamps the entry's; raise the item mark.

### 9.6 `create_routine`

Fields: `op` `"create_routine"`; `id` `id`; `name` `name`; `days` `int[0..127]`; `anchor` `date`; `start_minutes` `day_start`; `minutes` `len`. Schema rule: `start_minutes + minutes ≤ 1440`.

Apply: (1) mint check of `id`; (2) the cap of §9.5. Effects: insert the item with `kind` routine, `recurrence = {days, anchor, start_minutes, minutes}`, no deadline, no duration, no levels, no blocks, `created = ctx.at`, all stamps the entry's; raise the item mark. No zone is consulted: the start floats (V2-4).

### 9.7 `rename`

Fields: `op`, `id` `id`, `name` `name`. Apply: gone. Effects: `name`; stamp `name`; overwrite notice on `name` (§10.2).

### 9.8 `complete` and `delete`

Fields: `op` (`"complete"` or `"delete"`), `id` `id`, `at` `instant`.

Apply:
1. If there is no live item with `id`: when some archive row has `id`, the command is a benign repeat; otherwise gone.
2. `complete` only: kind (task).
3. The archive holds fewer than 200,000 rows, else `core/cap`.

Effects: `archived_at = min(at, ctx.at + 1 day)`; build the row from the item exactly as today's `Archived::retire` does (archive.rs:186-201): every field copied, blocks as `RowBlock`s with their sids, `outcome` `finished` for `complete` and `dropped` for `delete`; insert it at its archive-order position; remove the item; overwrite notice on `item` (§10.2).

### 9.9 `forget`

Fields: `op`, `id` `id`. Apply: if no live item has `id`, a benign repeat. Effects: remove the item, writing no row; overwrite notice on `item`.

### 9.10 `move_block`

Fields: `op`, `id` `id`, `sid` `id`, `start` `instant`, `minutes` `len`.

Apply: (1) gone; (2) kind (task); (3) the item has a block `sid`, else `core/gone`. Effects: that block's `start` and `minutes` set; its stamp written; overwrite notice on `block` with `sid`.

### 9.11 `move_blocks`

| Field | Type | Presence | Constraints |
|---|---|---|---|
| `moves` | array of `{id: id, minutes: int[0..1440], sid: id, start: instant}` | required | 1 to 1,000, in the order the origin computed them; `sid`s distinct |
| `op` | `"move_blocks"` | required | |

Apply: (1) every move whose `id` names a live item names a task, else `core/kind`. Effects, for each move in order: if the item is not live, or has no block `sid`, skip it with reason `gone`; else if the block's stamp is another device's change since `ctx.based_on` (§10.3), skip it with reason `changed`; else set the block's `start` and `minutes` and write its stamp. `moved` and `skipped` count the two outcomes; each skip produces a `skipped` notice (§10.7). A `move_blocks` that moves nothing still succeeds.

### 9.12 `add_block`

Fields: `op`, `id` `id`, `sid` `id`, `start` `instant`, `minutes` `len`.

Apply: (1) gone; (2) kind (task); (3) mint check of `sid`; (4) the item has fewer than 1,000 blocks and the board fewer than 50,000 live blocks, else `core/cap`. Effects: insert the block with the entry's stamp; raise the block mark.

### 9.13 `remove_blocks`

| Field | Type | Presence | Constraints |
|---|---|---|---|
| `blocks` | array of `{id: id, sid: id}` | required | 1 to 1,000; ascending by `sid`, distinct |
| `op` | `"remove_blocks"` | required | |

Apply: (1) every pair whose `id` names a live item names a task, else `core/kind`. Effects: for each pair whose item is live and has block `sid`, remove that block, with an overwrite notice on `block` (§10.2); pairs whose block is not there are passed over silently (SYNC.md §4.6: only the blocks the origin saw are removed, and a block someone else added since survives). When no block was removed, the command is a benign repeat.

### 9.14 `set_estimate`

Fields: `op`, `id` `id`, `minutes` `len`. Apply: (1) gone; (2) kind (task or event). Effects: `duration_minutes = minutes`; stamp `estimate` for a task, `time` for an event; overwrite notice on that group.

### 9.15 `set_deadline`

Fields: `op`, `id` `id`, `deadline` `instant` optional (absent clears it), `seed` `int[0..4]` optional.

Apply: (1) gone; (2) kind (task). Effects: `deadline` set or cleared; stamp `time`; overwrite notice on `time`; then, when `seed` is present and the item's `importance` is none, `importance = seed` and stamp `levels` (no notice for the seed). The origin decides whether to seed (§13.4); `apply` only performs it, which keeps the policy out of `apply` while a concurrent `set_severity` is never overwritten by a seed.

### 9.16 `set_severity` and `set_horizon`

Fields: `op`, `id` `id`, `level` (`int[0..4]` for `set_severity`, `int[0..3]` for `set_horizon`). Apply: (1) gone; (2) kind (task). Effects: `importance` or `time_importance` set to `level`; stamp `levels`; overwrite notice on `levels`.

### 9.17 `set_repeat`

Fields: `op`, `id` `id`, `days` `int[0..127]`, `anchor` `date`. Apply: (1) gone; (2) kind (routine). Effects: `recurrence.days` and `recurrence.anchor` set; stamp `repeat`; overwrite notice on `repeat`.

### 9.18 `move_routine`

Fields: `op`, `id` `id`, `start_minutes` `day_start`, `minutes` `len`. Schema rule: `start_minutes + minutes ≤ 1440`. Apply: (1) gone; (2) kind (routine). Effects: `recurrence.start_minutes` and `recurrence.minutes` set; stamp `time`; overwrite notice on `time`.

### 9.19 `set_event_span`

Fields: `op`, `id` `id`, `start` `instant`, `minutes` `len`. Apply: (1) gone; (2) kind (event). Effects: `deadline = start`, `duration_minutes = minutes`; stamp `time`; overwrite notice on `time`.

### 9.20 `set_notes`

| Field | Type | Presence | Constraints |
|---|---|---|---|
| `base` | `u53` | default `0` | the `notes_rev` the origin edited from |
| `op` | `"set_notes"` | required | |
| `text` | string | default `""` | at most 20,000 scalars; contains no U+0009 |

Apply: (1) `base = board.notes_rev`, else `core/notes_stale`, with `NotesConflict{base, current: board.notes_rev}`; (2) `notes_rev` is below 2^53 − 1, else `core/cap`. Effects: `notes = text`; `notes_rev += 1`. In rules version 1 the cap counts scalars after the origin's detab (SYNC.md §4.6; today counts before, board.rs:1253-1256).

### 9.21 `restore`

Fields: `op`, `id` `id` (0 allowed), `archived_at` `instant`, `fallback` `id`.

Apply:
1. mint check of `fallback`;
2. find the first row in archive order whose key is `(id, archived_at)`; if there is none: when `id ≠ 0` and a live item has `id`, or a live item has `fallback`, a benign repeat; otherwise `core/gone`;
3. fewer than 10,000 live items, and the live blocks plus the row's at most 50,000, else `core/cap`;
4. no sid of the row's blocks is held by a live block, else `core/id_taken`.

Effects: the target id is the row's `id` when it is not 0 and no live item holds it, else `fallback`; remove the row; insert an item with that id and every field of the row as today's `Archived::to_active` does (archive.rs:210-226): `kind`, `name`, `created` (the row's, not `ctx.at`), `deadline`, levels, `duration_minutes`, `recurrence`, the blocks with their sids; every stamp and block stamp the entry's; raise the item mark to `fallback`'s low part; `restored_as` = the target id.

### 9.22 `forget_archived`

Fields: `op`, `id` `id` (0 allowed), `archived_at` `instant`. Apply: if no row has the key, a benign repeat. Effects: remove the first row in archive order with that key.

### 9.23 Subscription commands

| `op` | Fields | Apply, in order | Effects |
|---|---|---|---|
| `add_subscription` | `id` `id`; `name` `name`; `url` string of 9 to 2,000 scalars beginning with the 8 bytes `https://`; `color` `rgba` | mint check of `id` (item space); fewer than 20 subscriptions, else `core/cap`; no subscription has the same `url` byte for byte, else `core/duplicate_url` | insert `{id, name, url, color, enabled: true}`; raise the item mark |
| `remove_subscription` | `id` `id` | if none has `id`, a benign repeat | remove it; `overlay_forget` gets `id` |
| `rename_subscription` | `id` `id`; `name` `name` | none has `id`: `core/gone` | `name` set |
| `set_subscription_color` | `id` `id`; `color` `rgba` | none has `id`: `core/gone` | `color` set |
| `set_subscription_enabled` | `id` `id`; `enabled` boolean, required | none has `id`: `core/gone` | `enabled` set; when `false`, `overlay_forget` gets `id` |

### 9.24 Settings commands

| `op` | Fields | Effects |
|---|---|---|
| `set_palette` | `colors`: array of exactly 6 `rgba`, required | `palette = colors` |
| `set_home` | `lat` `int[-9000..9000]`, `lon` `int[-18000..18000]`, both required | `home = {lat, lon}` |
| `set_picture` | `picture` `PictureRef` (§6.9), optional | `picture` set, or cleared when absent |

Schema rule for `set_picture`: `parts = ceil(size / 1,048,576)`. None of them has a check in `apply` beyond §8.4 steps 1 and 2.

- **CORE-CMD-001** (core) `parse_command` MUST perform the seven steps of §9.1 in order and MUST return the code of the first that fails.
- **CORE-CMD-002** (core) `parse_command` MUST accept exactly the 29 commands of §9.3 with the fields, types, presences and constraints of §9.4 to §9.24, and MUST reject every other `op` with `core/unknown_op`.
- **CORE-CMD-003** (core) `parse_command` MUST reject with `core/schema` an instant in any form other than §4.4's, including a naive local time and an offset other than `Z` (V2-4).
- **CORE-CMD-004** (core) For each command, `apply` MUST perform exactly the checks its section lists, in that order, and on success exactly the effects it lists.
- **CORE-CMD-005** (core) `apply` of `complete` and `delete` MUST set `archived_at` to `min(at, ctx.at + 86,400 s)`, the sum taken as `Instant::MAX` when it would be later (§5.1).
- **CORE-CMD-006** (core) `apply` MUST NOT trim, detab, default, round, snap, clamp or resolve any value of a command; every such policy belongs to the compile step (§13), and `apply` MUST only check the ranges, lengths, existence and uniqueness its sections list (V2-1).
- **CORE-CMD-007** (core) `apply` of `move_routine`, `create_routine` and `set_repeat` MUST NOT consult any zone or resolve any local time (V2-4).
- **CORE-CMD-008** (core) `apply` of `set_notes` MUST refuse with `core/notes_stale` when `base` differs from `notes_rev`, and on success MUST increase `notes_rev` by exactly 1.
- **CORE-CMD-009** (core) `apply` of `set_deadline` MUST set `importance` to `seed` only when `seed` is present and `importance` is none at the time of apply.

## 10. Conflict rules

### 10.1 Ordering and stamps

The last writer wins per command, in seq order, as today (DOC §4.1), and identically on every device because every device applies the same entries in the same order with the same `apply` (SYNC.md §4.6).

A **stamp** `(seq, device_no)` records which entry last changed something. Each live item carries five (§6.5), each live block one (§6.3). A creating command, `restore` and `add_block` write the entry's stamp `(ctx.seq, ctx.device_no)` on everything they create; every other command writes it on exactly the groups its row of §9.3 names. The legacy import writes `(1, 0)` everywhere (§15.4); `device_no` 0 is never an author. An item's **latest stamp** is the one with the greatest `seq` among its five field stamps and its blocks' stamps; among stamps of equal `seq`, the first in the order `estimate`, `levels`, `name`, `repeat`, `time`, then the blocks in ascending sid (equal seqs from different devices occur only in a board that no sequence of commands produces).

A stamp `(s, d)` is **another device's change since the origin's view** for an entry with `ctx` when `s > ctx.based_on`, `d ≠ ctx.device_no` and `d ≠ 0`.

### 10.2 Overwrite notices

A command that changes a field group, a block or (for `complete`, `delete`, `forget`) a whole item whose stamp (for an item, its latest stamp) is another device's change since the origin's view still applies, and `apply` produces one **overwrite notice** for it (SYNC.md §4.6, V2-5; SYNC_PLAN.md §2 takes notices rather than refusals). The notice names the overwriting entry (`seq`, `author = ctx.device_no`), the overwritten change (`other`, `other_seq` from the stamp), the item, the sid for a block, and the group (`name`, `time`, `levels`, `estimate`, `repeat`, `block` or `item`). It concerns two devices, the author's and the `other` device's, and each of them records it (`engine.md` §13.4). The core defines the record (§10.7), not the words a device shows for it: those are the interface's, `bridge.md` A-04 on the phone and `device.md` §17.7 on a desktop, and they differ between the author's device and the `other` device's. A command produces at most one notice per group and block it touches.

### 10.3 `move_blocks` skips

A Reflow moves many blocks the origin saw at once. `move_blocks` does not overwrite: a block that is gone, or whose stamp is another device's change since the origin's view, is skipped, and the rest move (V2-5). Each skip produces a `skipped` notice for the author alone, naming the block, the reason (`gone` or `changed`) and, for `changed`, the device and seq of the change. A single drag of one block compiles to `move_block`, which overwrites with a notice (§10.2), because the user aimed at that block.

### 10.4 `remove_blocks`

Unplan compiles to `remove_blocks` with exactly the sids the origin saw (§13.4). A block added since by anyone is not in the list and survives; a listed block already removed is passed over; a listed block another device moved since the origin's view is removed, with an overwrite notice.

### 10.5 Notes and their base

The notes carry a revision `notes_rev`, raised by 1 by every accepted `set_notes`. The **base** of a `set_notes` is the `notes_rev` of the text the origin's user started editing from. A `set_notes` whose base is not the current revision is refused (`core/notes_stale`) on every device, the author's included, with `NotesConflict{base, current}`; nothing changes. The origin still holds its draft (the device keeps the unsaved draft, `device.md` §17.6; the outbox record is dropped when its refusal is recorded, `engine.md` §7.4), so it can show its draft beside the current notes and let the user merge (stage `desktop`, `bridge.md`); the merged text is a new `set_notes` with `base = current`. Two `set_notes` from one device in a row work: the second is compiled on the view in which the first already raised the revision. A counter is used rather than a hash of the text so that the core needs no hash function and a base never matches by coincidence (decided here, §21).

### 10.6 Benign repeats

When a command finds its intended end state already in place, it succeeds and changes nothing (V2-16):

| Command | End state already in place when |
|---|---|
| `complete`, `delete` | no live item has `id`, and some archive row has `id` |
| `forget` | no live item has `id` |
| `restore` | no row has the key, and a live item has `id` (when `id ≠ 0`) or `fallback` |
| `forget_archived` | no row has the key |
| `remove_blocks` | none of the listed blocks is present |
| `remove_subscription` | no subscription has `id` |

Every other "not there" is a refusal (`core/gone`). The compile step still reports today's errors for these when the origin's own view shows the target missing (§13.4), so a user who repeats an action on one device sees today's messages; the benign case arises only between devices.

### 10.7 Notice and refusal records

Notices and refusals live per device, outside `BoardData`, in the device's sealed notices file (`seal.md` §19.2 kind 8); `engine.md` owns the list (which entries it records, when it shows and clears them), `device.md` the file. Only entries applied to the confirmed state produce records: a notice produced while applying to the pending state is discarded (`engine.md`). A device keeps an `overwrite` notice when its own `device_no` is the notice's `author` or `other`, a `skipped` notice when it is the `author` (§10.3: the author alone), and a refusal when it is the refused entry's author. Both are canonical-encoding records of at most 4,096 bytes each (`device.md` §6.4.5). A refusal's `code` and `op` are read as any string, so that a record a newer build wrote still reads; `base` and `current` are present only when `op` is `set_notes`.

**Notice**

| Field | Type | Presence | Constraints | Meaning |
|---|---|---|---|---|
| `author` | `int[1..8191]` | required | | `ctx.device_no` of the applied entry |
| `group` | string | required | `name`, `time`, `levels`, `estimate`, `repeat`, `block`, `item` | what was overwritten or skipped |
| `item` | `id` | required | | the item |
| `kind` | string | required | `"overwrite"` or `"skipped"` | |
| `other` | `int[1..8191]` | optional | present for `overwrite`, and for `skipped` with reason `changed` | the device whose change was overwritten, or that changed the block |
| `other_seq` | `u53` | optional | present exactly when `other` is | the seq of that change |
| `reason` | string | optional | `skipped` only: `"changed"` or `"gone"` | |
| `seq` | `u53` | required | | `ctx.seq` of the applied entry |
| `sid` | `id` | optional | present exactly when `group` is `block` | |

**Refusal**

| Field | Type | Presence | Constraints | Meaning |
|---|---|---|---|---|
| `author` | `int[1..8191]` | required | | the refused entry's author |
| `base` | `u53` | optional | `set_notes` only | the command's base |
| `code` | string | required | a code of §16, a code of `seal.md` §22, or `engine/skipped` or `engine/reseal_limit` (`engine.md` §33) | why |
| `current` | `u53` | optional | `set_notes` only | `notes_rev` when it was applied |
| `op` | string | optional | absent when the payload did not parse | the command's `op` |
| `seq` | `u53` | required | | the refused entry's seq |
| `target` | `id` | optional | | the command's `id` (item or subscription), when it has one |

Neither record holds a name, notes text or URL (CORE-GEN-010); the UI looks those up by id in the current state.

- **CORE-CONF-001** (core) `apply` MUST write `(ctx.seq, ctx.device_no)` on exactly the stamps the row of §9.3 for the command names, and on every stamp of what it creates.
- **CORE-CONF-002** (core) `apply` MUST produce an overwrite notice exactly when a command changes a group, block or item whose stamp (latest stamp, for an item) is another device's change since the origin's view as §10.1 defines it, and no other overwrite notice.
- **CORE-CONF-003** (core) `apply` of `move_blocks` MUST skip every move whose block is gone or whose block stamp is another device's change since the origin's view, MUST move every other block, and MUST produce one `skipped` notice per skip.
- **CORE-CONF-004** (core) `apply` of `remove_blocks` MUST remove only listed blocks and MUST NOT fail because a listed block is absent.
- **CORE-CONF-005** (core) `apply` MUST treat exactly the cases of the table in §10.6 as benign repeats.
- **CORE-CONF-006** (core) Notice and refusal records MUST be encoded as the canonical records of §10.7.
- **CORE-CONF-007** (core) Two devices that apply the same entries to the same state MUST produce equal notices in equal order.

## 11. Rules versions

### 11.1 Rules version 1

A rules version fixes everything `apply` accepts and does: the set of ops, their fields and constraints, their checks and effects, the limits of §17 that `apply` enforces, and the canonical encoding version (SYNC.md §4.5, V2-1). **Rules version 1** is §6, §7, §9, §10 and §17 of this spec under canonical encoding v1. Its op table is the 29 ops of §9.3. It allows envelope (1, 1) and state-hash version 1 (`seal.md` §14.3).

`apply` is keyed by `board.rules`: it dispatches to the rules of that version, and a version's code is never changed once released; a fix is a new version, and the old one stays, pinned by its golden logs (§18). A later version may add ops, fields, checks or limits; every op of an earlier version keeps its meaning under the earlier version.

### 11.2 Changing the version

`BoardData.rules` mirrors the membership's `rules` (`seal.md` §13.1, §25). A version changes only through a sequenced `RulesUpgrade` that `seal.md` §13.10 accepts. When the engine accepts the control entry at seq `s` that activates version `v`, it calls `upgrade_rules(&mut board, v)` before applying any entry above `s` (`engine.md`):

1. `v > board.rules`, else `core/unknown_rules`;
2. `v` is in this build's `SUPPORTED_RULES`, else `core/unknown_rules` (the engine then halts, as `seal.md` §14.5 also reports);
3. `board.rules = v`, and the migration that version `v` defines is applied (version 1 defines none; a version that needs one defines it as a pure function of the board).

The compile step emits only ops in the op table of `view.rules`, so a writer never emits what the active rules forbid (SYNC.md §4.5); a feature that needs a newer version stays disabled in the UI until it is active, and the UI names the blocker from the membership (`engine.md`).

`SUPPORTED_RULES` is `[1]` in this build; the beat lists it (`seal.md` §11.4).

- **CORE-RULES-001** (core) `apply` MUST dispatch on `board.rules` and MUST refuse with `core/rules` a command whose `op` is not in that version's op table.
- **CORE-RULES-002** (core) `upgrade_rules` MUST perform the three steps of §11.2 in order and MUST leave the board unchanged when it returns an error.
- **CORE-RULES-003** (core) Once a rules version is released, replaying its golden logs (§18) MUST give the recorded results and canonical board bytes in every later build.
- **CORE-RULES-004** (core) The compile step MUST NOT emit a command whose `op` is not in the op table of `view.rules`.

### 11.3 Reset

A `ResetTo` (`seal.md` §13.6, `engine.md` §28.1) installs an earlier state of the board and leaves the membership unchanged. `reset(target, current)` computes the board it installs; `target` is the board at the reset's target seq and `current` the board just before the `ResetTo` entry. In order:

1. `target.rules ≤ current.rules`, else `core/unknown_rules`;
2. the result `b` is `target` with every field kept, except:
   - `marks`: one mark for each `device_no` that has a mark in `target` or in `current`, its `item` and its `block` each the larger of the two boards' values (an absent mark counts as 0), ascending by `device_no`;
   - `notes_rev = max(target.notes_rev, current.notes_rev) + 1`; when the larger of the two is already 2^53 − 1, `reset` returns `core/cap` instead;
3. when `b.rules < current.rules`: `upgrade_rules(&mut b, current.rules)` (§11.2); its error is returned.

The marks keep every number minted since the target from being minted again (§7.2); the raised revision makes every `set_notes` based on a revision of either board stale (§10.5); and `b.rules` mirrors the unchanged membership. `reset` produces no notices and, like `apply`, is a function of its arguments alone (CORE-CTX-002).

- **CORE-RULES-005** (core) `reset` MUST return `target` unchanged except that its marks are the elementwise maximum of both boards' marks, its `notes_rev` is one above the larger of the two, and its rules version is raised to `current.rules` by `upgrade_rules`, and MUST return `core/unknown_rules` when `target.rules` exceeds `current.rules`, and `core/cap` when the larger `notes_rev` is already 2^53 − 1.

## 12. Intents

### 12.1 What an intent is

An intent is a user action as a UI expresses it, before policy: it may carry naive local times, untrimmed names, a block's index-free `session`, and no ids for what it creates. Intents are never replicated or stored in the log; the compile step turns each into at most one log command (§13). The phone page sends intents as today's command JSON (through the bridge, `bridge.md`); the desktop builds them as Rust values or the same JSON.

### 12.2 Parsing intent JSON

`parse_intent(text)` reads today's wire shapes (`Command`, board.rs:111-280, with `wire::optional_local`, board.rs:392-419), so the forked page needs no change to what it sends:

1. the text is at most 84,096 bytes (today's `MAX_BODY_BYTES`, phone.rs:136), else `core/intent_json`;
2. it is a JSON object with a string field `op` naming an intent of §12.3, else `core/intent_json` ("unknown variant");
3. the fields are read as today's `serde` derive reads them: an unknown field is ignored (today's behaviour, R1 §11), a missing field takes the default the table gives, a missing field without one is an error, a wrong type is an error; integers into the Rust types the table names;
4. a **wire time** field (`deadline` of `add`, `quick_add`, `set_deadline`; `at` of `complete`, `delete`) that is absent, `null`, or a string that is empty after trim is none; otherwise, after trim, it is `WireTime::Exact` if `chrono::DateTime::parse_from_rfc3339` (chrono 0.4.45) accepts it and its seconds are not a leap second; else `WireTime::Naive` if `chrono::NaiveDateTime::parse_from_str` accepts it with `%Y-%m-%dT%H:%M` or `%Y-%m-%dT%H:%M:%S`; else an error with the message `` not a time: `<text>` ``;
5. `archived_at` of `restore` and `forget_archived` is RFC 3339 as in step 4, `Exact` only; a naive form is an error, as today;
6. a `date` field (`day`) is `YYYY-MM-DD` as `chrono::NaiveDate`'s `serde` reads it.

Every failure is `core/intent_json` with HTTP status 400 and the parser's message, as today's `BoardError::bad_request` carries it (board.rs:491-515).

### 12.3 The intents

"Phone" marks the 17 verbs the phone page sends (phone.html, the `send` calls listed in V2's confirmed strengths: "exactly 17 ops"); "desk" what the desktop sends (the `Command::` uses in ui.rs); "new" an intent this design adds.

| `op` | Fields (Rust type; default) | Sent by | Compiles to (§13.4) |
|---|---|---|---|
| `add` | `name` String; `deadline` wire time; `importance` u8 (none); `is_event` bool (false); `time_importance` u8 (none) | desk | `create_task` or `create_event` |
| `quick_add` | `name` String; `deadline` wire time | phone, desk | `create_task` |
| `create` | `kind` `board::Kind` (JSON `"task"`, `"event"` or `"routine"`); `name` String; `day` date; `start` i32; `minutes` u32 | phone, desk | `create_task`, `create_event` or `create_routine` |
| `rename` | `id` u64; `name` String | phone, desk | `rename` |
| `complete` | `id` u64; `at` wire time | phone, desk | `complete` |
| `delete` | `id` u64; `at` wire time | phone, desk | `delete` |
| `forget` | `id` u64 | desk | `forget` |
| `move_block` | `id` u64; `session` u64 (none): a sid; `day` date; `start` i32; `minutes` u32 | phone, desk | `move_block`, `set_event_span` or `move_routine` |
| `add_block` | `id` u64; `day` date; `start` i32; `minutes` u32 (none) | phone, desk | `add_block` |
| `remove_block` | `id` u64; `session` u64: a sid | phone, desk | `remove_blocks` |
| `unplan` | `id` u64 | phone, desk | `remove_blocks`, or nothing |
| `set_estimate` | `id` u64; `minutes` u32 | phone, desk | `set_estimate`, `move_routine` or `set_event_span` |
| `set_deadline` | `id` u64; `deadline` wire time | phone, desk | `set_deadline` |
| `set_severity` | `id` u64; `level` u8 | phone, desk | `set_severity` |
| `set_horizon` | `id` u64; `level` u8 | phone, desk | `set_horizon` |
| `set_repeat` | `id` u64; `days` u8; `day` date | phone, desk | `set_repeat` |
| `reflow` | `day` date; `from` i32 (none) | phone, desk | `move_blocks`, or nothing |
| `set_notes` | `text` String; `base` u64 (none; new) | phone, desk | `set_notes` |
| `restore` | `id` u64; `archived_at` RFC 3339 | phone, desk | `restore` |
| `forget_archived` | `id` u64; `archived_at` RFC 3339 | desk | `forget_archived` |
| `add_subscription` | `name` String; `url` String; `color` [u8; 4] (none) | desk | `add_subscription` |
| `remove_subscription` | `id` u64 | desk | `remove_subscription` |
| `rename_subscription` | `id` u64; `name` String | desk (also the automatic rename from a feed's title, subscriptions.rs:312-334) | `rename_subscription`, or nothing |
| `set_subscription_color` | `id` u64; `color` [u8; 4] | desk | `set_subscription_color` |
| `set_subscription_enabled` | `id` u64; `enabled` bool | desk | `set_subscription_enabled` |
| `set_palette` | `colors` [[u8; 4]; 6] | new (desk) | `set_palette` |
| `set_home` | `lat` f64; `lon` f64: degrees | new (desk) | `set_home` |
| `set_picture` | `picture` `PictureRef` (none) | new (desk, after `device` uploads the blob) | `set_picture` |
| `snapshot`, `feed`, `board` | today's fields | none (queries) | error `core/query` |

The snapshot query is answered by the read side (§14.3), not compiled; `feed` and `board` are gone with the `.ics` feed and the whole-board route (SYNC_PLAN.md §2).

- **CORE-INT-001** (core) `parse_intent` MUST accept exactly the shapes of §12.3 by the six steps of §12.2, ignoring unknown fields, and MUST return `core/intent_json` with status 400 for every other input.
- **CORE-INT-002** (core) `parse_intent` MUST NOT resolve a naive wire time; it MUST carry it to the compile step as `WireTime::Naive`, which resolves it in the origin's zone (§13.3).
- **CORE-INT-003** (core) `parse_intent` MUST read `session` as a sid (u64) and MUST NOT interpret it as an index.

## 13. The compile step

### 13.1 Inputs and output

```rust
pub struct CompileCtx<'a, Z: Zone + ?Sized> {
    pub view: &'a BoardData,     // the origin's pending state
    pub zone: &'a Z,             // the origin's zone
    pub now: Instant,            // the origin's own wall clock, in whole milliseconds, never corrected by a relay-derived offset (engine.md §27.3)
    pub device_no: u16,          // the origin's device_no
    pub overlay: &'a Overlay,    // the origin's overlay: its anchors for Reflow
}
pub struct Compiled { pub command: Option<LogCommand>, pub reply: Reply }
pub struct Reply { pub id: Option<u64>, pub session: Option<u64>, pub moved: Option<u32> }
pub fn compile<Z: Zone + ?Sized>(intent: &Intent, cx: &CompileCtx<Z>, minter: &mut IdMinter) -> Result<Compiled, CompileError>;
```

The engine puts `now` (as `at_ms`) and its confirmed head (as `based_on`) into the entry's plaintext, so that `apply` sees the same clock the compile step used (`engine.md`). `Reply` is today's reply (board.rs:524-535) with `session` now a sid; the bridge turns it into the page's `{"ok": true, ...}` (`bridge.md`). A `CompileError` carries a code of §16, today's HTTP status (400 or 410) and today's message text; it changes nothing, and the minter is advanced only when `compile` returns `Ok`.

### 13.2 What only the origin decides

Policy lives here and nowhere else (thin apply, SYNC.md §4.5, V2-1): default names, the default horizon and severity seeds, default subscription colours and names, ids, sids and the restore fallback, trimming, detab, snapping and clamping, zone conversion, the Reflow algorithm and its anchors, and which blocks an Unplan removes. None of it is replicated as behaviour; only its results are, inside the command.

### 13.3 Rules for every intent

1. **Trim** (G1): names, URLs and wire-time text are trimmed of White_Space (§2), an explicit list, so the result does not depend on the toolchain's Unicode tables (V2-1).
2. **Names** (G2): after trimming, a name over 200 scalars is `core/name_long`; an empty name is handled as the intent's row says.
3. **Wire times** (G3): `Exact` converts to an instant (an out-of-range one is `core/date_range`); `Naive` resolves as today's `parse_local_input` (board.rs:409-419) in the origin's zone: `resolve_on_day(date, hour × 60 + minute, zone)` (seconds dropped); no instant, or an instant whose local day's year is outside 1970 to 9999, is `core/intent_json` with `` not a time: `<text>` ``.
4. **Dates** (G4): a `day` whose year is outside 1970 to 9999 is `core/date_range` ("sane day").
5. **Instants** (G5): every instant the intent yields must lie within §5.1's range, else `core/date_range` ("sane instant"). The bound is checked on the UTC instant (V2-4), not on a local date as today (board.rs:446-460).
6. **The view** (G6): every check `apply` would make against the board (existence, kind, a block's presence, caps, URL uniqueness) is made against `view`, with today's message, so the user hears at once what today's server says; the command can still be refused later if another device's entry lands first.
7. **Ids** (G7): every id, sid and fallback is minted from the minter (§7.2) after every check has passed.
8. **Rules** (G8): CORE-RULES-004.
9. **Clock** (G9): `now` is the only clock: it defaults and caps `complete` and `delete` and gives Reflow's now-line; `created` comes from `Ctx.at`, which equals `now`.
10. **Detab** (G10): `set_notes` text is detabbed before its length is checked.

### 13.4 Per intent

Each row's steps run in order; the first failure is the result. "Missing" means no live item in `view` has `id` (`core/item_gone`, 410 "That item is no longer on the board."). `item` is that live item. `L(t)` is the local minutes of instant `t` and `D(t)` its local day, in the origin's zone. `clamp_block`, `resolve_on_day`, `snap` are §5.4 and §5.5, all in the origin's zone. "Cap" is the live-item cap of §9.5 (`core/cap_items`), checked on `view`; for a `create` of a task it includes the block caps of §9.4 (`core/cap_blocks`).

| Intent | Steps | Command | Reply |
|---|---|---|---|
| `add` | name (G2); empty: `core/name_empty` "It needs a name."; `is_event` and no `deadline`: `core/event_time`; `deadline` (G3, G5); if not `is_event` and no `deadline`, `importance` or `time_importance`: `time_importance = 1`; `importance ≥ 5`: `core/severity`; `time_importance ≥ 4`: `core/horizon`; cap of 10,000 live items: `core/cap_items`; mint `id` | `is_event`: `create_event{id, name, start: deadline, importance, time_importance}`; else `create_task{id, name, deadline, importance, time_importance}` | `id` |
| `quick_add` | name; empty: `core/name_empty` "A task needs a name."; `deadline` (G3, G5); cap; mint `id` | `create_task{id, name, deadline}` with `importance = 2` when dated, else `time_importance = 1` | `id` |
| `create` | `day` (G4); `(start, minutes) = clamp_block(start, minutes)`; task or event: `when = resolve_on_day(day, start)`, none: `core/bad_time`; `when` (G5); name (G2); empty: `"New task"`, `"New event"` or `"New routine"`; cap; mint `id` (and for a task one `sid`) | task: `create_task{id, name, time_importance: 1, duration_minutes: minutes, blocks: [{sid, start: when, minutes}]}`; event: `create_event{id, name, start: when, minutes}`; routine: `create_routine{id, name, days: 0, anchor: day, start_minutes: start, minutes}` | `id` |
| `rename` | name (G2); empty: `core/name_empty` "A name cannot be empty."; missing | `rename{id, name}` | — |
| `complete` | missing; event or routine: `core/finish_kind`; `at` (G3, G5); 200,000 archive rows in `view`: `core/cap_archive`; `at' = min(at, now + 1 day)`, or `now` when none | `complete{id, at: at'}` | — |
| `delete` | missing; `at` (G3, G5); 200,000 archive rows in `view`: `core/cap_archive`; `at'` as for `complete` | `delete{id, at: at'}` | — |
| `forget` | missing | `forget{id}` | — |
| `move_block` | `day` (G4); missing; task: `session` none or not a block of `item`: `core/block_gone`; `(start, minutes) = clamp_block(start, minutes)`; task or event: `when = resolve_on_day(day, start)`, none: `core/bad_time`, then G5 | task: `move_block{id, sid: session, start: when, minutes}`; event: `set_event_span{id, start: when, minutes}`; routine: `move_routine{id, start_minutes: clamp(start, 0, 1439), minutes}` | — |
| `add_block` | `day` (G4); missing; event or routine: `core/block_kind`; `length = minutes`, or when none `drop_length_for(item)` (planner.rs:553-558); `(start, length) = clamp_block(start, length)`; `when = resolve_on_day(day, start)`, none: `core/bad_time`; G5; caps of §9.12: `core/cap_blocks`; mint `sid` | `add_block{id, sid, start: when, minutes: max(length, 15)}` | `session = sid` |
| `remove_block` | missing; event or routine, or `session` not a block of `item`: `core/block_gone` | `remove_blocks{blocks: [{id, sid: session}]}` | — |
| `unplan` | missing; event or routine: `core/unplan_kind`; no blocks: no command | `remove_blocks{blocks}`: every block of `item` in `view`, ascending by sid | — |
| `set_estimate` | `minutes` outside 15 to 1440: `core/length_range`; missing; then by kind: routine: `(s, l) = clamp_block(item.start_minutes, minutes)`; event with a `deadline` `d`: `(s, l) = clamp_block(L(d), minutes)`, `when = resolve_on_day(D(d), s)`, and when none `when = d`; G5 | task, or event without a deadline: `set_estimate{id, minutes}`; routine: `move_routine{id, start_minutes: s, minutes: l}`; event with a deadline: `set_event_span{id, start: when, minutes: l}` | — |
| `set_deadline` | `deadline` (G3, G5); missing; event or routine: `core/deadline_kind` | `set_deadline{id, deadline, seed}`, `seed = 2` when `deadline` is present and `item.importance` is none in `view`, else absent | — |
| `set_severity` | `level ≥ 5`: `core/severity`; missing; event or routine: `core/severity_kind` | `set_severity{id, level}` | — |
| `set_horizon` | `level ≥ 4`: `core/horizon`; missing; event or routine: `core/horizon_kind` | `set_horizon{id, level}` | — |
| `set_repeat` | `day` (G4); missing; not a routine: `core/repeat_kind`; `days' = days AND 127`; `anchor = day` when `days' = 0`, else `item.recurrence.anchor` | `set_repeat{id, days: days', anchor}` | — |
| `reflow` | `day` (G4); `from` given: `f = clamp(from, 0, 1440)`; else `f = now_marker(day, now)`, and when none: no command, `moved = 0`; then the algorithm below | `move_blocks{moves}`, or no command when `moves` is empty | `moved` |
| `set_notes` | `t = detab(text)` (G10); `t` over 20,000 scalars: `core/notes_long` | `set_notes{text: t, base}`, `base` = the intent's `base`, or `view.notes_rev` when none | — |
| `restore` | `archived_at` (G5); no row of `view` has the key `(id, archived_at)`: `core/archive_gone`; cap of 10,000 live items: `core/cap_items`; mint `fallback` | `restore{id, archived_at, fallback}` | `id`: the row's `id` when it is not 0 and not live in `view`, else `fallback` |
| `forget_archived` | `archived_at` (G5); no row of `view` has the key: `core/archive_gone` | `forget_archived{id, archived_at}` | — |
| `add_subscription` | 20 subscriptions in `view`: `core/calendars_full`; `u = checked_url(url)` (below); a subscription in `view` with URL `u`: `core/url_duplicate`; name (G2); mint `id`; empty name: `name_from_url(u)` (subscriptions.rs:294-302) when it is some and at most 200 scalars, else `"Calendar <id>"` with `id` in decimal; `color` given, else `DEFAULT_COLORS[n mod 6]` with `n` the number of subscriptions in `view` | `add_subscription{id, name, url: u, color}` | `id` |
| `remove_subscription` | no subscription `id` in `view`: `core/calendar_gone` | `remove_subscription{id}` | — |
| `rename_subscription` | name (G2); no subscription `id`: `core/calendar_gone`; empty name: no command | `rename_subscription{id, name}` | — |
| `set_subscription_color` | no subscription `id`: `core/calendar_gone` | `set_subscription_color{id, color}` | — |
| `set_subscription_enabled` | no subscription `id`: `core/calendar_gone` | `set_subscription_enabled{id, enabled}` | — |
| `set_palette` | none | `set_palette{colors}` | — |
| `set_home` | `lat` and `lon` finite, `−90 ≤ lat ≤ 90`, `−180 ≤ lon ≤ 180`, else `core/place`; each rounded to hundredths: `round_half_away_from_zero(x × 100)` in `f64` | `set_home{lat, lon}` | — |
| `set_picture` | `picture` present: `parts = ceil(size / 1,048,576)` and the §6.9 ranges, else `core/picture` | `set_picture{picture}` | — |
| `snapshot`, `feed`, `board` | always `core/query` | — | — |

**`checked_url`** is today's (subscriptions.rs:156-180), with trim per G1: trim; empty: `core/url_empty`; over 2,000 scalars: `core/url_long`; a leading `webcal://` becomes `https://`; a leading `http://`: `core/url_http`; not beginning with `https://`: `core/url_scheme`; the host (the text after `https://` up to the first `/`, `?` or `#`) empty or without a `.`: `core/url_host`.

**Reflow**, after `f` is known (board.rs:888-908, 1226-1250, planner.rs:695-722, with the order changes of §13.5):

1. For each live item of `view` in ascending `id`, for each placement of the item on `day` (`placements_for`, §14.2): a block placement with a sid (a task's block) becomes a flowable `{key: (id, sid), start, minutes}`; every other block placement becomes an anchor `(start, start + minutes)`; markers are dropped. The flowables of one item come in ascending sid.
2. The anchors of `overlay.anchors_on(day)` (subscriptions.rs:460-466) are appended.
3. `planner::reflow(flowables, anchors, f)` gives a new start per flowable, in input order.
4. For each flowable in input order whose new start `m` differs from its current local minutes: `w = resolve_on_day(day, m)`; when `w` is some and differs from the block's `start`, append `{id, sid, start: w, minutes}` with the block's stored `minutes`.
5. `moved` is the number of moves.

- **CORE-COMP-001** (core) `compile` MUST perform the steps of each row of §13.4 in order, MUST return the code, status and message of the first that fails, and MUST otherwise return exactly the command and reply of that row.
- **CORE-COMP-002** (core) `compile` MUST trim exactly the White_Space code points of §2 and no others.
- **CORE-COMP-003** (core) `compile` MUST resolve every naive local time and every day-and-minutes pair in the zone of its `CompileCtx`, and MUST emit only UTC instants.
- **CORE-COMP-004** (core) `compile` MUST mint ids only after every check of the row has passed, and MUST NOT advance the minter when it returns an error.
- **CORE-COMP-005** (core) `compile` of `reflow` MUST compute its moves by the five steps of §13.4 from `view` and the given overlay, and MUST emit only blocks whose instant changes.
- **CORE-COMP-006** (core) `compile` of `unplan` MUST list exactly the sids of the item's blocks in `view`.
- **CORE-COMP-007** (core) `compile` MUST use `CompileCtx.now` as its only clock and MUST NOT read any other instant except those in the intent and the view.
- **CORE-COMP-008** (core) Every command `compile` returns MUST pass `parse_command` after canonical encoding, and MUST be accepted by `apply` on `view` with a `Ctx` whose `device_no` is the origin's, whose `at` is `now` and whose `based_on` is below its `seq`.

### 13.5 Today's commands against the new ones

The transcript test (SYNC_PLAN.md §6.2) replays `fixtures/`' recorded sequences: each recorded command is parsed as an intent (§12.2), compiled with the transcript's pinned clock as `now` (truncated to milliseconds) and its pinned `TZ` as a zone that reads local times as the recording did, and applied with `Ctx{seq: k + 1, device_no: 1, at: now, based_on: k}` at step `k`. The table classifies each recorded command against the state before it. The state after step `k` is compared with the recorded state only while steps 1 to `k` are all "same"; the comparison is the one of §18.4.

The replay's zone holds the rules of the transcript's `TZ` and, for a local time in a fold, returns the later instant alone: the recording took chrono's `.earliest()`, which is the later one (§5.4). A recorded command that resolves a time in a fold therefore stays "same". CORE-TIME-003 itself is proven by the `planner.json` vectors (§18.2).

| Today's `op` | New | Classification |
|---|---|---|
| `add`, `quick_add` | `create_task`, `create_event` | same |
| `create` | `create_task`, `create_event`, `create_routine` | same, except a routine whose `resolve_on_day(day, start)` is none in the pinned zone: changed (today 400, now created, V2-4) |
| `rename` | `rename` | same |
| `complete`, `delete` | `complete`, `delete` | same, except an instant whose local year and UTC year fall on different sides of 1970 or 9999: changed (V2-4) |
| `forget` | `forget` | same |
| `move_block` | `move_block`, `set_event_span`, `move_routine` | same, except a routine whose `resolve_on_day(day, start)` is none: changed (today 400) |
| `add_block` | `add_block` | same (the reply's `session` is a sid, not an index; replies are not compared) |
| `remove_block` | `remove_blocks` | same; the recorded `session` index `i` is translated to the sid of the `i`-th block of the item in ascending sid (§18.4) |
| `unplan` | `remove_blocks` | same |
| `set_estimate` | `set_estimate`, `move_routine`, `set_event_span` | same |
| `set_deadline` | `set_deadline` | same |
| `set_severity`, `set_horizon`, `set_repeat` | same names | same |
| `reflow` | `move_blocks` | same, except when two flowable blocks on the day have equal starts and today's `Vec` order differs from ascending `(id, sid)`: changed (ties) |
| `set_notes` | `set_notes` | same, except `chars(text) ≤ 20,000 < chars(detab(text))`: changed (cap after detab) |
| `restore`, `forget_archived` | same names | same, except when two rows share the key and today's memory order and the archive order pick different rows: changed |
| `add_subscription` | `add_subscription` | same (ids differ; translated as §18.4 says) |
| `remove_subscription`, `set_subscription_color`, `set_subscription_enabled` | same names | same |
| `rename_subscription` | `rename_subscription` or nothing | same (an empty name changed nothing but today's version, which is not compared) |
| `snapshot` | read query (§14.3) | gone: not a command; transcripts replay it as a snapshot comparison (§18.4) |
| `feed`, `board` | none | gone (SYNC_PLAN.md §2) |

A changed classification ends the comparison of that transcript at the step before it; the remaining steps are still replayed, so that `apply` runs on them, but not compared.

## 14. Read queries

### 14.1 Rules for every read query

Read queries answer what the desktop UI and the phone page ask today, under today's names (SYNC_PLAN.md §3.3), over a `&BoardData` and, where a local day or time matters, the caller's `Zone` as a parameter. They never change the board. They are presentation: they may use `f32` (ranking) and may differ between platforms in the last bit of a score, because nothing they return is stored or hashed (R1 §1, "Presentation only"). Each device draws in its own zone (SYNC.md §4.5, a deliberate change to DOC §21.3). Display order belongs to the UI; no read query reorders `BoardData` (SYNC.md §4.5).

### 14.2 The queries

Each is today's function (file:line in today's `src/`) moved into the core with these changes only: a `zone: &(impl Zone + ?Sized)` parameter wherever today's code reads a local date, hour or minute (§5.2); `Item` and `ArchiveRow` in place of `Active` and `Archived`; sids in place of session indices; and the order changes of §14.3.

| Today | In the core | Zone | Notes |
|---|---|---|---|
| `Board::ranked_ids(now)` (board.rs:860-870) | `ranked_ids(board, now, zone) -> Vec<u64>` | yes | tasks only; stable sort by `importance_score` descending over items in ascending id |
| `Board::shuffle_seed(now)` (board.rs:854-856) | `shuffle_seed(now, zone) -> u64` | yes | `num_days_from_ce` of the local day |
| `Active::importance_score(now, seed)` (tasks.rs:387-503) | `Item::importance_score(now, seed) -> f32` | no | unchanged, including its tables and jitter |
| `Board::behind_minutes(day, now)` (board.rs:912-920) | `behind_minutes(board, day, now, zone) -> i32` | yes | |
| `Board::next_free_start(day)` (board.rs:875-884) | `next_free_start(board, day, zone) -> i32` | yes | |
| `Board::reflow_inputs(day)` (board.rs:888-908) | `reflow_inputs(board, overlay, day, zone)` | yes | §13.4 steps 1 and 2 |
| `planner::placements_for`, `placements_of`, `appears_on` (planner.rs:153-225) | same names, `(…, day, zone)` | yes | `DayPlacement.session` is `Option<u64>`, a sid; blocks in ascending sid |
| `planner::lay_out`, `summarize`, `summary_text`, `format_minutes`, `format_duration`, `drop_length_for`, `duration_options`, `relative_due`, `reflow`, `behind_minutes` | same | no | `relative_due(deadline, now)` takes instants |
| `planner::backlog_group(deadline, day)` | `backlog_group(deadline, day, zone)` | yes | `Due` when `D(deadline) ≤ day` |
| `planner::now_marker(day, now)` | `time::now_marker(day, now, zone)` | yes | §5.5; in `time`, not `planner` (§20) |
| `planner::snap`, `clamp_block`, `resolve_on_day` (planner.rs:296-311 for the last) | `time::snap(m)`, `time::clamp_block(start, minutes)`, `time::resolve_on_day(day, minutes, zone)` | `resolve_on_day` only | §5.4, §5.5; in `time`, not `planner` (§20) |
| `Active::is_planned`, `planned_minutes`, `remaining_minutes`, `wants_planning`, `planner_anchor`, `calendar_item_color`, `is_routine` | `Item::` same | no | |
| `Recurrence::falls_on`, `repeats`, `includes`, `summary`, `summary_dated` | same | no | `toggle` stays in the UI |
| `Archived::verdict`, `timing`, `was_finished`, `was_judged`, `mark`, `lifetime`, `booked_minutes`, `sittings`, `color_id` | `ArchiveRow::` same; `verdict(zone)` | `verdict` only | |
| `archive::Filter`, `summarize`, `Summary::headline`, `group_by_month` | same; `group_by_month(rows, zone)` | `group_by_month` | rows newest first |
| `phone::snapshot(board, palette, ranked, from, days, now)` (phone.rs:218-272) | `snapshot(&SnapshotInput) -> Snapshot` | yes | §14.3 |
| `phone::build_day`, `build_tray`, `item_detail`, `done_rows`, `with_subscribed` | same names, with `zone` | yes | |
| `phone::answer_query` (phone.rs:193-214) | `answer_snapshot(&SnapshotInput) -> serde_json::Value` | yes | the `feed` and `board` arms are gone |
| `phone::calendar_feed` | removed | | SYNC_PLAN.md §2 |
| — | `archive_newest_first(board) -> impl Iterator<Item = &ArchiveRow>` | no | the reverse of the archive order |
| — | `archive_by_day(board, from, days, zone)` | yes | §14.4 |
| — | `reminders(board, from, until, defaults)` | no | §14.5 |
| `TaskApp::summarize_calendar` (ui.rs:2911-2955) | not moved: stays in `desktop`, over a copy | | today it drains and re-sorts `board.items`; ties on (start, id, sid) (SYNC.md §4.5, CORE-READ-006) |

Display order belongs to the caller (SYNC.md §4.5). No read query sorts, drains or reorders `BoardData.items` or any other list of `BoardData`: each orders what it returns in a list of its own, in the order this table states, and the board keeps its own order (items in ascending id, §6). The desktop's calendar summary, today's `summarize_calendar`, stays in the desktop and orders a copy: its entries ascend by (start, id, sid), where start is the instant the entry is drawn at (a block's start, or an event's own span start), id is the item's id, and sid is the block's sid, an event's own span (no block) sorting before every sid of the same item.

- **CORE-READ-005** (core) A read query MUST NOT sort, drain or reorder `BoardData.items` or any other list held by `BoardData`, and MUST order what it returns in a list of its own, as §14.2 states for it.
- **CORE-READ-006** (desktop) The desktop's calendar summary (today's `summarize_calendar`) MUST order a copy of the items, never `BoardData.items`, and MUST order its entries by ascending (start, id, sid) as §14.2 states.

### 14.3 The snapshot JSON

```rust
pub struct SnapshotInput<'a, Z: Zone + ?Sized> {
    pub board: &'a BoardData, pub overlay: &'a Overlay, pub zone: &'a Z, pub now: Instant,
    pub from: chrono::NaiveDate, pub days: u32,
    pub version: u64,            // the device's view counter (engine.md)
    pub ranked: Option<Vec<u64>>, pub look: Option<Look>, pub dials: Dials,
}
```

`snapshot` builds today's `Snapshot` (phone.rs:2055-2094) and every type inside it field for field, by today's algorithm (phone.rs:218-272 and 2335-2569: `days` clamped to 1 to 31; one `build_day` per day from `from`; `with_subscribed` from the overlay; `tray`, `items`, `ranked`, `done`, `notes`, `known`), serialised by `serde_json` with today's field names, attributes and field order. It differs from today's in exactly these ways:

1. `version` is `SnapshotInput.version`, not today's board version.
2. `palette` is `board.palette` (today: the desk's selected scheme, or six `[0,0,0,0]`).
3. `items` are in ascending `id` (today: the `Vec` order).
4. `ranked`, when the caller passes none, is `ranked_ids`, with ties in ascending `id`; `tray.due` and `tray.later` break ties the same way.
5. `EntrySnapshot.session` is the block's sid (today: its index), and the entries' sort key `(start, id, session)` compares sids.
6. `ItemDetail.sessions` are in ascending sid, and `SessionDetail.index` is the sid.
7. `DoneRow.archived_at` is the canonical instant text of §4.4 (today: RFC 3339 with the local offset and chrono's fraction digits). The page echoes it in `restore` (phone.html:5257), which §12.2 step 5 parses.
8. `done` and each day's `ghosts` take the archive newest first in the reverse archive order of §6.12; rows with equal `archived_at` come in that order rather than today's file order.
9. A new field `notes_base` (integer, always present, written right after `notes`) is `board.notes_rev`, so the forked page can send `set_notes` with its base (`bridge.md`).
10. `look` and `dials` are the caller's (today: process globals, phone.rs:1675-1715).
11. `known` and every `subscribed` list come from `SnapshotInput.overlay`, with today's rule: `known` is `overlay.covers` only when some subscription in `board` is enabled.
12. Every local date, weekday, `today`, `now_minutes`, `start`, `TrayCard.deadline`, `ItemDetail.deadline`, `ItemDetail.created`, `SessionDetail.day`, `DoneRow.when` and verdict time is computed in `SnapshotInput.zone`. With the zone equal to the process zone of a fixture, the values equal today's.
13. Ghosts and `done` never come out empty because of an archive read failure: the archive is in memory.
14. `kind` strings come from `Item.kind` (same values: `"task"`, `"event"`, `"routine"`).

`answer_snapshot` returns `serde_json::to_value(snapshot(input))`. The query string handling of `/api/state` (`from` defaulting to today, `days` defaulting to 1, phone.rs:964-981) is the bridge's (`bridge.md`), with "today" taken in the device's zone.

### 14.4 The archive bucketed by day

`archive_by_day(board, from, days, zone)` returns, for each of the `days` days from `from`, the ghost sources of that day: one pass over the archive newest first; for each row whose `kind` is not `event`, for each of its blocks in ascending sid, the pair `(row, block)` is appended to the bucket of `D(block.start)` when that day is in range. `build_day` takes each day's bucket, places each block as `Block{start: L(block.start), minutes: max(block.minutes, 15)}`, and sorts them stably by start, which equals today's per-day scan (phone.rs:2409-2421: only archived task blocks ghost; an archived event, having no deadline in today's ghost placeable, places nothing, and a row with blocks of any other kind ghosts its blocks). One pass serves a 31-day snapshot, which keeps the 40 ms budget with 10,000 rows (§17).

### 14.5 Reminders

`reminders(board, from, until, defaults) -> Vec<Reminder>` lists the reminders that fire in `[from, until)`. The `android` stage schedules them as exact alarms and recomputes them before notifying (SYNC_PLAN.md §6.10); the core only computes.

```rust
pub struct ReminderDefaults { pub event_lead_minutes: u32, pub block_lead_minutes: u32, pub due_lead_minutes: u32 }
impl Default for ReminderDefaults { /* 10, 0, 0 */ }
```

Sources, from live items only:

| `kind` | From | `at` | `minutes` |
|---|---|---|---|
| `event` | an event with a `deadline` | `deadline` | `duration_minutes`, when present |
| `block` | each block of a task | the block's `start` | the block's `minutes` |
| `due` | a task with a `deadline` | `deadline` | absent |

Routines give no reminders: a routine is a habit, not an appointment (decided here). `fire_at = at − lead`, with the lead of the kind in minutes (each 0 to 1,440); a source is included when `from ≤ fire_at < until`. The list is ascending by `fire_at`, then by `key` bytes.

**Reminder record** (canonical encoding, so that it crosses JNI as text):

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `at` | `instant` | required | the start or due time |
| `fire_at` | `instant` | required | when to notify |
| `item` | `id` | required | |
| `key` | string | required | `<kind>:<item>:<sid or 0>:<at>`, with ids in decimal and `at` in the form of §4.4: stable, so a recomputed reminder is recognised |
| `kind` | string | required | `"event"`, `"block"` or `"due"` |
| `minutes` | `int[0..1440]` | optional | |
| `name` | string | required | the item's name, for the notification on this device only |
| `sid` | `id` | optional | `block` only |

A reminder's Done compiles the `complete` intent, its Tomorrow the `move_block` intent one day later at the same local minutes (or `set_event_span` for an event), and Snooze is local to the phone; the lock-screen text is the `android` stage's.

### 14.6 ICS parser, overlay and weather types

`ics::parse(text, from, until, limits, zone)` is today's parser (ics.rs:182-214) with `zone` where it converts UTC and `TZID` times to `Local` (ics.rs:904, 1218, 740-749) and reads an undefined `TZID` as local wall time; everything else is unchanged, including `Limits::default()`. The overlay types (`OverlayEvent`, `FetchOutcome`, `FetchStatus`, `Covered`, `Overlay` with `sealed`, `covering`, `digest`, `anchors_on`, `events_on`, `forget`, `status_for`, `better_name`, `is_unnamed`; subscriptions.rs:184-484) and the weather report types (`Report`, `Hour`, `Day`, `Now`, `build_report`, `sky_symbol`, the cities; weather.rs) are copied unchanged, except that `FetchStatus.at` is `chrono::DateTime<chrono::FixedOffset>` (today `DateTime<Local>`, subscriptions.rs:337-339, a type CORE-GEN-004 forbids). Its `serde` form is RFC 3339 with the offset, which today's `DateTime<Local>` also writes and reads, so `subscribed_cache.json` keeps today's format in both directions. The caller fills it from its own clock and zone (`device.md` §13.1.2 step 8); none of the copied functions reads a clock (CORE-GEN-003). The overlay is per device and never enters `apply` (SYNC.md §3, R1 §7); a device keeps it in its own sealed file (`device.md`).

- **CORE-READ-001** (core) Every read query of §14.2 MUST take the caller's zone as a parameter wherever it needs a local day or local minutes, and MUST give the result today's function gives for the same board when the zone equals that function's process zone, except for the changes §14.2 and §14.3 list.
- **CORE-READ-002** (core) A read query MUST NOT change `BoardData`.
- **CORE-READ-003** (core) `ranked_ids` MUST sort tasks stably by `importance_score` descending over the items in ascending id.
- **CORE-SNAP-001** (core) `snapshot` MUST produce today's snapshot JSON field for field, except for exactly the fourteen changes of §14.3, and the snapshot fixtures (§18.5) MUST prove it.
- **CORE-SNAP-002** (core) `snapshot` MUST write `notes_base` equal to `notes_rev`, `EntrySnapshot.session` and `SessionDetail.index` as sids, and `DoneRow.archived_at` as the canonical instant text.
- **CORE-SNAP-003** (core) `archive_by_day` MUST produce, for each day, exactly the ghosts today's per-day scan produces, in the order §14.4 states, from one pass over the archive.
- **CORE-REM-001** (core) `reminders` MUST return exactly the sources of the table in §14.5 whose `fire_at` lies in `[from, until)`, in ascending `(fire_at, key)`, with `ReminderDefaults::default()` leads of 10, 0 and 0 minutes.
- **CORE-REM-002** (core) A reminder's `key` MUST be formed as §14.5 states, so that equal sources give equal keys across recomputations.
- **CORE-READ-004** (core) `ics::parse` MUST convert UTC and `TZID` times with the zone it is given, and MUST NOT read the process zone.

## 15. Legacy import

### 15.1 Purpose

The import turns one standalone desktop folder (SYNC_PLAN.md §2: "One standalone desktop folder, possibly written by the older `main` build") into the genesis `BoardData` that ruling S2's `Genesis` item hashes and that the genesis snapshot carries (`seal.md` §12.2, §20.4 with `S = 1`). It is the only place, with the genesis itself, where normalisation runs (V2-19). `TaskDeck --import-check <folder>` (stage `desktop`) runs it as a dry run and prints the report; "Create board from this folder" runs it and hands the result to the engine's genesis (`engine.md`). The core reads no files: the caller reads them and passes their bytes; the caller also refuses a folder whose `.lock` is held (SYNC.md §9 step 4, `device.md`).

### 15.2 Inputs

```rust
pub struct LegacyFolder {
    pub read_at_startup: Option<Vec<u8>>,   // read_at_startup.json; None when the file is missing
    pub archived: Option<Vec<u8>>,          // archived.jsonl
    pub notepad: Option<Vec<u8>>,           // notepad_text.json
    pub subscriptions: Option<Vec<u8>>,     // subscriptions.json (the main build has none)
    pub colorschemes: Option<Vec<u8>>,      // colorschemes.json
    pub config: LegacyConfig,               // from userconfig.toml, read by the caller with today's parser
}
pub struct LegacyConfig { pub coordinates: Option<[f64; 2]>, pub selected_colorscheme_id: Option<u32> }
```

The caller fills `LegacyConfig` with today's `initialization` parser (initialization.rs:243-362): `coordinates` when the key holds two finite numbers, `selected_colorscheme_id` when it parses, clamped to 0 to 200,000 as today. Each byte input is at most 268,435,456 bytes (256 MiB). Today's files and the `main` build's are read by the same parsers: the `main` build's `Active` (`id`, `importance`, `time_importance`, `name`, `created`, `deadline`, `is_event`) and `InActive` (no `time_importance`, `sessions`, `duration_minutes`, `recurrence` or `outcome`) are subsets of today's shapes that today's `serde` defaults complete (origin/main:src/tasks.rs).

### 15.3 Steps

In order; a step marked **fail** ends the import with no `BoardData` and a report that names the failure kind and, where stated, the ids involved.

1. **Sizes.** An input over 256 MiB: fail `too_large`.
2. **Items.** `read_at_startup` missing: no items. Otherwise parse it as a JSON array of today's `Active` (tasks.rs:169-237, lenient: unknown fields ignored, today's `serde` defaults); failure: fail `items_unreadable`.
3. **Archive.** `archived` missing: no rows. Otherwise split it at every byte `0x0A`; each piece, in file order, is a line. A line that is not UTF-8 is kept as an unreadable line; a line that is empty after trim (White_Space) is skipped; a line that does not parse as today's `Archived` (archive.rs:114-155, wire name `inactivated` for `archived_at`, today's defaults) is kept as an unreadable line (archive.rs:683-729). An unreadable line is kept byte for byte without its `0x0A`.
4. **Notes.** `notepad` missing: `""`. Otherwise it must parse as one JSON string; failure: fail `notes_unreadable` (today substitutes an error text, board.rs:595; the import must not). Then detab.
5. **Subscriptions.** Missing, or only White_Space: none. Otherwise a JSON array of today's `Subscription` (subscriptions.rs:137-147); failure: fail `subscriptions_unreadable`.
6. **Palette.** `colorschemes` missing: six `[0,0,0,0]`. Otherwise parse it as a JSON object from decimal `u32` keys to today's `ColorScheme` (color.rs:11-16); failure: warning `palette_unreadable` and six `[0,0,0,0]`. With the selected id `k` (`selected_colorscheme_id`, default 0): the scheme under key `k` gives `colors`; no such key gives six `[0,0,0,0]` (today's desktop then draws `default_scheme`, which is all zero, utilities.rs:94-103).
7. **Home.** `coordinates` none: `{0, 0}`. Otherwise `lat = round_half_away_from_zero(c[0] × 100)` and `lon = round_half_away_from_zero(c[1] × 100)` in `f64`; outside the ranges of §6.9: warning `home_out_of_range` and `{0, 0}`.

The first board created from a legacy folder takes its palette from the scheme that `selected_colorscheme_id` of that folder's `userconfig.toml` names in its `colorschemes.json`, and its home place from that file's `coordinates` (SYNC.md §9 step 6). The core does steps 6 and 7 from `LegacyConfig`; the caller fills `LegacyConfig` from the source folder's own `userconfig.toml` (§15.2, `device.md` §18.2).

8. **Plans.** `migrate_legacy_plans` exactly as today (tasks.rs:618-632): an item with a `planned_start`, not an event and without sessions, gains one session `{start: planned_start, minutes: max(duration_minutes or 30, 15)}`; `planned_start` is dropped from every item.
9. **Ids.** Let `L` be the largest id below 2^40 among items, rows and subscriptions (0 when there is none), and `next = L + 1`. Then, each list in file order:
   1. an item whose id is 0, or equal to an earlier item's id, or at least 2^62 (a temporary id, `adopt_temporaries`, board.rs:697-711) gets `next`, and `next` increases;
   2. a row whose id is at least 2^62 gets `next` likewise; a row with id 0 keeps it (a pre-id row);
   3. a subscription whose id is 0, or equal to an earlier subscription's id, or at least 2^62 gets `next` likewise (today's seeding bug can leave two subscriptions with one id, R1 §2);
   4. an item, row or subscription id from 2^40 to 2^62 − 1 whose high part (`id div 2^40`) is from 1 to 8191 and whose low part (`id mod 2^40`) is at least 1 is **device-minted** (§7.1) and is kept unchanged; any other id from 2^40 to 2^62 − 1 (a high part of 8192 or more, which is an id of at least 2^53, or a low part of 0), or any `next` that reaches 2^40: fail `id_range`, listing the ids.
   Every renumbering is reported as `(list, old id, new id)`. Ids held both by a live item and by an archive row are reported as `live_and_archived` (a warning; both are kept, and a later `restore` of such a row uses its fallback; V2-11).
10. **Kinds and sids.** Each item and row gets `kind`: `routine` when it has a `recurrence`, else `event` when `is_event`, else `task`. Sids are numbered from 1 by one counter: first the live items in ascending (new) id, each item's sessions in file order; then the rows in file line order, each row's sessions in order.
11. **Instants.** Every `DateTime<Local>` value becomes the instant it denotes, keeping every fraction digit (a nine-digit fraction holds them all); an instant outside §5.1's range: fail `instant_range`, listing the item or row ids.
12. **Fields.** Every value is checked against the schema of §6: names at most 4,096 bytes, levels, durations, recurrence fields, block minutes, URLs, at most 20 subscriptions with distinct URLs, notes at most 1,048,576 bytes, and the caps of §17; a violation: fail `field_range`, listing the ids, or `cap`.
13. **Notes cap.** Notes over 20,000 scalars after detab: warning `notes_over_cap`; they are kept, and the next `set_notes` must fit the cap.
14. **Assembly.** `rules = 1`; every stamp `(1, 0)`; the marks, ascending by `device_no`: for `device_no` 0, `{item: the largest low part among the item, row and subscription ids now present whose high part is 0, block: the last sid numbered}`; for each high part `h` from 1 to 8191 that some item, row or subscription id now present has, `{device_no: h, item: the largest low part among those ids with high part h, block: 0}`; no other marks, and a mark whose two numbers are 0 is not stored (§6.10); `notes_rev = 0`; no picture; the archive in archive order; the unreadable lines in their order (§6.1).
15. **Check.** `BoardData::check` must hold; otherwise fail `invariant` (an error in the import itself). The board's canonical bytes must be at most 61,865,984 (§8.4 step 4); otherwise fail `cap`.

The genesis of an empty board (no folder) is `BoardData::empty()`: `rules = 1` and every other field at its default; its canonical bytes are `{"rules":1}`.

### 15.4 The report

```rust
pub struct ImportReport {
    pub items_task: u32, pub items_event: u32, pub items_routine: u32, pub blocks: u32,
    pub archive_rows: u32, pub unreadable_lines: u32, pub subscriptions: u32,
    pub notes_scalars: u32, pub palette_set: bool, pub home_set: bool,
    pub renumbered: Vec<Renumbered>,          // { list: IdList, old: u64, new: u64 }
    pub live_and_archived: Vec<u64>,
    pub warnings: Vec<ImportWarning>,         // PaletteUnreadable, HomeOutOfRange, NotesOverCap
    pub failure: Option<ImportFailure>,       // { kind: FailureKind, ids: Vec<u64> }
}
pub fn import(folder: &LegacyFolder) -> (Option<BoardData>, ImportReport);
```

It holds counts, ids and kinds only, never a name, the notes, a URL or any other text from the board (SYNC_PLAN.md §6.2 item 8). The canonical hash that `--import-check` prints and that must repeat (SYNC.md §9 step 4) is SHA-256 of the genesis board's canonical bytes, computed by the caller.

- **CORE-IMP-001** (core) `import` MUST perform the steps of §15.3 in order and MUST stop at the first failing step with that step's failure kind.
- **CORE-IMP-002** (core) `import` MUST give equal canonical board bytes and equal reports for equal inputs.
- **CORE-IMP-003** (core, desktop) The import report MUST contain no name, notes text, URL or other text from the board, only the counts, ids and kinds of §15.4.
- **CORE-IMP-004** (core) `import` MUST keep every archive line that is not UTF-8 or does not parse as an unreadable line, byte for byte, and MUST NOT discard any line except one that is empty after trim.
- **CORE-IMP-005** (core) `import` MUST renumber ids exactly as §15.3 step 9 states, MUST report each renumbering, MUST keep every device-minted id of step 9.4 unchanged, and MUST fail with `id_range` exactly when step 9.4 names an id or `next` reaches 2^40.
- **CORE-IMP-006** (core) `import` MUST number sids as §15.3 step 10 states and MUST set the legacy marks as step 14 states, so that `BoardData::check` holds for the result.
- **CORE-IMP-007** (core) `import` MUST read the `main` build's files and today's files with the same parsers and MUST produce for each `main`-format fixture the genesis board recorded with it (§18.6).
- **CORE-IMP-008** (core) A function of the core other than `import` MUST NOT perform normalisation: id assignment, plan migration, detab of stored notes, or substitution of defaults for missing stored values.
- **CORE-IMP-009** (core) `import` MUST set the genesis palette from the scheme under `LegacyConfig.selected_colorscheme_id` (default 0) in `colorschemes`, and the genesis home from `LegacyConfig.coordinates`, exactly as §15.3 steps 6 and 7 state, with the defaults and the warnings `palette_unreadable` and `home_out_of_range` they name, and MUST set `palette_set` and `home_set` in the report accordingly.

## 16. Error catalogue

**Parse and data codes.** Returned by the canonical parser (§4.5), `parse_command` (§9.1) and `BoardData::from_canonical_bytes` (§6.12).

| Code | Meaning | Raised when | What the caller does |
|---|---|---|---|
| `core/too_large` | input over its byte limit | §4.5 step 1, §9.1 step 1 | engine: a `cmd` payload is refused on every device (`engine.md`); other callers reject the input |
| `core/not_json` | not UTF-8 JSON, or nested deeper than 32 | §4.5 step 2 | as `core/too_large` |
| `core/unknown_op` | `op` missing or not a command of §9.3 | §9.1 step 4 | as `core/too_large` |
| `core/schema` | unknown, duplicate or missing field, `null`, float, wrong type, value out of range, broken schema rule | §4.5 steps 3–4, §9.1 steps 3, 5 and 7 | as `core/too_large` |
| `core/not_canonical` | bytes are not the canonical encoding of their value | §4.5 step 5 | as `core/too_large` |
| `core/invariant` | board bytes parse but break a rule of §6.12 (the rule number is reported) | `from_canonical_bytes`; import step 15 | engine: a snapshot whose board fails is passed over with the alarm `BlobFailure` (`engine.md` §19.7, §19.9, §22.3); its own checkpoint or base is the local fault `engine/local_fault` (`engine.md` §7.10); import: report a failure |

**Apply refusals.** Returned by `apply` (§8, §9) as `Refusal.code`; the entry is refused on every device and reported to its author (§10.7).

| Code | Meaning | Raised when | What the caller does |
|---|---|---|---|
| `core/bad_ctx` | `Ctx` breaks §8.1 | §8.4 step 1 | a fault of the caller: the engine treats it as a bug, never as a user refusal (`engine.md`) |
| `core/rules` | `op` not in the active rules' op table | §8.4 step 2 | record the refusal; the UI says the author's build is ahead of the board |
| `core/gone` | the item, block or subscription is not there | §9.7–§9.23 | record the refusal; the UI says it was deleted elsewhere |
| `core/kind` | the item's kind does not take this command | §9 | record the refusal |
| `core/bad_id` | a minted id or sid is not the author's or not above its mark | CORE-ID-002 | record the refusal; the author's minter is behind its mark: the device advances it (`device.md`) |
| `core/id_taken` | a restored row's sid is held by a live block | §9.21 step 4 | record the refusal |
| `core/cap` | a cap of §17 would be exceeded | §9; §8.4 step 4 | record the refusal; the UI says the board is full |
| `core/duplicate_url` | that calendar URL is already subscribed | §9.23 | record the refusal |
| `core/notes_stale` | `set_notes` base is not the current revision | §9.20 | record the refusal with `base` and `current`; the origin offers its draft and the merge view |

**Rules.**

| Code | Meaning | Raised when | What the caller does |
|---|---|---|---|
| `core/unknown_rules` | `upgrade_rules` to a version that is not newer or not supported; `reset` with a target newer than the current board | §11.2, §11.3 | engine: halt with the rules halt (`seal.md` §14.5, `engine.md`); from `reset`, the local fault `engine/local_fault` (`engine.md` §28.1) |
| `core/cap` (from `reset`) | the notes revision cannot be raised past 2^53 − 1 | §11.3 | engine: the local fault `engine/local_fault`, as for `core/unknown_rules` from `reset` |

**Intent and compile errors.** Returned by `parse_intent` (§12) and `compile` (§13) as `CompileError{code, status, message}`; nothing is queued. The UI shows `message`; the bridge returns it as today's `{"error": message}` with `status` (`bridge.md`). Messages are today's (board.rs, subscriptions.rs) except where the status parentheses say new.

| Code | Meaning (status) | Raised when | What the caller does |
|---|---|---|---|
| `core/intent_json` | the parser's message, e.g. `` not a time: `<text>` `` (400) | §12.2; §13.3 G3 | show it |
| `core/query` | "That is a query, not a command." (400) | `snapshot`, `feed`, `board` | the bridge answers queries itself |
| `core/name_long` | "A name is at most 200 characters." (400) | G2 | show it |
| `core/name_empty` | `add`: "It needs a name."; `quick_add`: "A task needs a name."; `rename`: "A name cannot be empty." (400) | §13.4 | show it |
| `core/event_time` | "An event needs a time." (400) | `add` | show it |
| `core/date_range` | "That date is out of range (1970–9999)." (400) | G4, G5 | show it |
| `core/severity` | "No such severity." (400) | `add`, `set_severity` | show it |
| `core/horizon` | "No such horizon." (400) | `add`, `set_horizon` | show it |
| `core/item_gone` | "That item is no longer on the board." (410) | missing (§13.4) | show it; refresh |
| `core/block_gone` | "That block is no longer there." (410) | `move_block`, `remove_block` | show it; refresh |
| `core/bad_time` | "That time doesn't exist on this day (daylight saving)." (400) | `create`, `move_block`, `add_block` | show it |
| `core/finish_kind` | "Only a task can be finished — an event or a routine is deleted instead." (400) | `complete` | show it |
| `core/block_kind` | "Only a task can be given a block of time." (400) | `add_block` | show it |
| `core/unplan_kind` | "Only a task can be unplanned." (400) | `unplan` | show it |
| `core/length_range` | "A length is between fifteen minutes and a day." (400) | `set_estimate` | show it |
| `core/deadline_kind` | "Only a task has a due date; an event's time is its time." (400) | `set_deadline` | show it |
| `core/severity_kind` | "Only a task has a severity." (400) | `set_severity` | show it |
| `core/horizon_kind` | "Only a task has a horizon." (400) | `set_horizon` | show it |
| `core/repeat_kind` | "Only a routine repeats." (400) | `set_repeat` | show it |
| `core/notes_long` | "Those notes are too long for the notepad." (400) | `set_notes` | show it; keep the draft |
| `core/archive_gone` | "That is no longer in the archive." (410) | `restore`, `forget_archived` | show it; refresh |
| `core/calendars_full` | "That is already 20 calendars, which is as many as this keeps." (400) | `add_subscription` | show it |
| `core/url_empty` | "An address is needed." (400) | `checked_url` | show it |
| `core/url_long` | "That address is too long." (400) | `checked_url` | show it |
| `core/url_http` | "Use https — a calendar link is a password, and http sends it in clear." (400) | `checked_url` | show it |
| `core/url_scheme` | "An address should start with https://" (400) | `checked_url` | show it |
| `core/url_host` | "That address has no host in it." (400) | `checked_url` | show it |
| `core/url_duplicate` | "That calendar is already subscribed to." (400) | `add_subscription` | show it |
| `core/calendar_gone` | "That calendar is no longer subscribed to." (410) | subscription intents | show it; refresh |
| `core/cap_items` | "That is as many items as a board can hold." (400; new) | creating intents, `restore` | show it |
| `core/cap_blocks` | "That is as many blocks as a board can hold." (400; new) | `create` (task), `add_block` | show it |
| `core/cap_archive` | "The archive is full." (400; new) | `complete`, `delete` | show it |
| `core/place` | "That place is off the map." (400; new) | `set_home` | show it |
| `core/picture` | "That picture could not be shared." (400; new) | `set_picture` | a fault of the uploading code: log it |

**Import.**

| Code | Meaning | Raised when | What the caller does |
|---|---|---|---|
| `core/import_failed` | the folder cannot become a board; `ImportFailure.kind` is one of `too_large`, `items_unreadable`, `notes_unreadable`, `subscriptions_unreadable`, `id_range`, `instant_range`, `field_range`, `cap`, `invariant` | §15.3 | print the report; the user repairs the folder or keeps today's app |

## 17. Limits

| Name | Value | Unit | Enforced by |
|---|---|---|---|
| JSON nesting depth | 32 | levels | CORE-ENC-004 |
| log command | 1 to 257,979 | bytes | CORE-CMD-001 |
| canonical board bytes | 62,914,560 | bytes (60 MiB, so that a snapshot body, which holds the board, the membership and the control history, `seal.md` §20.4, stays under 64 MiB inflated) | CORE-DATA-004 |
| canonical board bytes after a command that lengthens them | 61,865,984 | bytes (59 MiB) | CORE-APPLY-005 |
| notice or refusal record | 4,096 | bytes | CORE-CONF-006 |
| parser tree budget | `max(32 × limit, 1,048,576)` | charged units (§4.5) | CORE-ENC-004 |
| intent JSON | 84,096 | bytes | CORE-INT-001 |
| legacy input file | 268,435,456 | bytes | CORE-IMP-001 |
| live items | 10,000 | items | CORE-CMD-004 |
| blocks per item, per archive row, per `create_task` | 1,000 | blocks | CORE-CMD-004, CORE-DATA-001 |
| live blocks | 50,000 | blocks | CORE-CMD-004 |
| archive rows | 200,000 | rows | CORE-CMD-004 |
| unreadable lines | 10,000 | lines | CORE-DATA-001 |
| unreadable line | 1 to 65,536 | bytes | CORE-DATA-001 |
| subscriptions | 20 | subscriptions | CORE-CMD-004 |
| moves per `move_blocks`, pairs per `remove_blocks` | 1 to 1,000 | entries | CORE-CMD-002 |
| name in a command | 1 to 200 | scalars | CORE-CMD-002 |
| name in data | 4,096 | bytes | CORE-DATA-001 |
| URL in a command | 9 to 2,000 | scalars | CORE-CMD-002 |
| URL in data | 1 to 8,000 | bytes | CORE-DATA-001 |
| notes in a command | 20,000 | scalars, after detab | CORE-CMD-002 |
| notes in data | 1,048,576 | bytes | CORE-DATA-001 |
| length `len` | 15 to 1,440 | minutes | CORE-CMD-002 |
| routine start `day_start` | 0 to 1,425 | minutes past local midnight | CORE-CMD-002 |
| severity, horizon | 0 to 4, 0 to 3 | levels | CORE-CMD-002 |
| weekday bits | 0 to 127 | | CORE-CMD-002 |
| archive clamp | 86,400 | seconds after `Ctx.at` | CORE-CMD-005 |
| `device_no` in `Ctx` | 1 to 8,191 | | CORE-CTX-001 |
| id low part | 1 to 2^40 − 1 | | CORE-ID-001 |
| id | below 2^53 | | CORE-ID-001 |
| instant range | 1970-01-01T00:00:00Z to 9999-12-31T23:59:59.999999999Z | | CORE-TIME-001 |
| home | −9,000 to 9,000 and −18,000 to 18,000 | hundredths of a degree | CORE-CMD-002 |
| picture | 67 to 67,108,864 bytes in 1 to 64 parts | | CORE-CMD-002 |
| reminder leads | 0 to 1,440 | minutes | CORE-REM-001 |
| snapshot days | 1 to 31 (clamped silently) | days | CORE-SNAP-001 |
| done rows | 40 | rows | CORE-SNAP-001 |
| budget: 31-day snapshot of the 10,000-row fixture | 40 | ms, median of five runs | CORE-PERF-001 |
| budget: 5,000 entries applied | 1,000 | ms | CORE-PERF-002 |
| budget: checkpoint (canonical board bytes of the 10,000-row fixture) parsed and checked | 150 | ms | CORE-PERF-003 |

- **CORE-PERF-001** (core) `snapshot` of 31 days from the 10,000-row fixture MUST complete within 40 ms as the median of five runs in the budget CI job.
- **CORE-PERF-002** (core) Applying 5,000 log commands of the budget workload to the 10,000-row fixture MUST complete within 1 s as the median of five runs.
- **CORE-PERF-003** (core) `BoardData::from_canonical_bytes` of the 10,000-row fixture's canonical bytes MUST complete within 150 ms as the median of five runs.

The 10,000-row fixture is the board that `import` makes of `fixtures/boards/archive-10k`. The snapshot of CORE-PERF-001 is the request of that board's recorded 31-day snapshot (§18.5). The budget workload of CORE-PERF-002 is 1,000 rounds of five commands on that board, authored by `device_no` 2, each with `based_on` one below its `seq`: a `create_task` with one block of 30 minutes, a `rename` of that task, a `move_block` of its block by 15 minutes, a `set_deadline`, and its `complete`. Only `apply` is timed; the commands are built beforehand.

## 18. Test vectors

### 18.1 Location and format

Stage `core` publishes these families as JSON files in the format of README §7 (a `"spec"` field naming the section, a `"cases"` array, byte strings in lowercase hex) under `crates/taskdeck-core/tests/vectors/`. Golden logs are under `crates/taskdeck-core/tests/vectors/golden/rules-<v>/`. Fixtures that `foundation` generates from today's code stay under `fixtures/` (SYNC_PLAN.md §4, §6.1 item 5); the core's tests read them in place.

Conventions inside core vectors:

- Canonical bytes are given as a JSON string holding the canonical text (its UTF-8 bytes are the bytes), field `"…_text"`, and where a hash is expected as `"…_sha256"`, lowercase hex of SHA-256 of those bytes (computed by the test with the dev-dependency `sha2`).
- A zone is `{"fixed": <offset_s>}` or `{"table": {"initial": <offset_s>, "transitions": [[<utc_ms>, <offset_s>], …]}}` (§5.2). Property tests also run under a named `TZ` with `ChronoZone(chrono::Local)`; vectors never depend on the host's zone.
- An instant input is the 30-byte text of §4.4; `at_ms` is an integer.

### 18.2 Families

| File | Spec | Inputs | Expected outputs |
|---|---|---|---|
| `canonical.json` | §4 | JSON values; byte strings | `enc` bytes; for invalid input (whitespace, key order, escapes such as `A` and `\/`, `-0`, leading zeros, floats, `null`, duplicate keys, depth 33, invalid UTF-8) the error code |
| `instants.json` | §4.4, §5.1 | instants as parts, `at_ms`, texts | the 30-byte text; parse results; rejections (second 60, `z`, `+00:00`, 8 and 10 fraction digits, year 1969 and 10000, 2026-02-29) |
| `planner.json` | §5 | `snap`, `clamp_block`, `snap_up` arguments; `resolve_on_day` with table zones holding a spring-forward and a fall-back transition | values, including every example of §5.5 |
| `commands.json` | §9 | command texts | parsed or the code of §9.1; per op one valid case at each boundary and one invalid case per field constraint and schema rule |
| `board-bytes.json` | §6 | board texts | accepted with `board_sha256`, or `core/invariant` with the rule number of §6.12; one case per rule |
| `golden/rules-1/*.json` | §8–§11 | a start board, entries | §18.3 |
| `reset.json` | §11.3 | target and current board texts | the result board text and its `board_sha256`, or the code; cases with a mark in only one board, with each board's `notes_rev` the larger, and with equal rules |
| `rebind.json` | §7.5 | command texts, `from_device_no`, view board text, minter | the rebound command texts and the minter after, or `core/bad_id`; cases for every minting and referring field, a reference to an accepted id, and a re-sorted `remove_blocks` |
| `intents.json` | §12 | the page's and the desktop's JSON shapes (every op of §12.3, every wire-time form) | the parsed intent, or `core/intent_json` |
| `compile.json` | §13 | intent text, view board text, zone, `now`, `device_no`, minter, overlay anchors | command text and reply, or code, status and message; the minter after |
| `reminders.json` | §14.5 | board text, `from`, `until`, defaults | reminder records |
| `import.json` | §15 | a folder under `fixtures/` (today's format and `main`'s), or the file bytes given in the case; the config values | genesis `board_sha256` and the report, or the failure; cases with device-minted ids (items, rows and subscriptions with ids `2^40 + 3` and `5 × 2^40 + 1`, a duplicate device-minted item id, a temporary id at or above 2^62 next to them) and the marks step 14 gives them; `id_range` for an id with high part 8192 and for the id `3 × 2^40` |

### 18.3 Golden logs

One file per topic (for example `create.json`, `blocks.json`, `archive.json`, `notes.json`, `subscriptions.json`, `settings.json`, `ids.json`, `conflicts.json`), each:

```json
{"spec": "core.md §9", "rules": 1, "cases": [
  {"name": "…", "start_text": "<canonical board bytes>",
   "entries": [
     {"ctx": {"at_ms": 0, "based_on": 1, "device_no": 1, "seq": 2},
      "command_text": "<canonical command bytes>",
      "expect": {"result": "applied" | "benign" | "refused" | "parse_error",
                 "code": "core/…", "notices": ["<notice record text>", …],
                 "moved": 0, "skipped": 0, "restored_as": 0,
                 "board_sha256": "<hex>"}}],
   "end_text": "<canonical board bytes>"}]}
```

`code` is present for `refused` and `parse_error`; `notices`, `moved`, `skipped` and `restored_as` when they apply; `board_sha256` always (unchanged for a refusal). Rules version 1's golden logs, which CORE-RULES-003 requires, cover: every check of every op of §9 with a refused case; every row of §10.6; every notice kind and group; skips for both reasons; and the boundary values 200 and 201 scalars, 14, 15, 1,440 and 1,441 minutes, levels 4 and 5 (severity) and 3 and 4 (horizon), notes of 20,000 and 20,001 scalars, ids at and above the mark, `device_no` 8191, the id 2^53 − 1, instants at both ends of the range, every cap at its value and one above, `based_on` equal to a stamp's seq and one below it, and a `complete` whose `ctx.at` is less than a day before the end of the instant range (the saturating clamp). Golden logs contain no zone, so they hold under any `TZ` (V2-4). Where one above a cap cannot be written as a command (1,001 blocks in a `create_task`, 1,001 moves or pairs), the case is a `parse_error`.

**Generated start boards.** A case that needs a start board too large to print (the caps of §17: 200,000 archive rows are about 22 MB of JSON) gives `start_gen` in place of `start_text`, and `end_sha256`, lowercase hex of SHA-256 of the end board's canonical bytes, in place of `end_text`. `start_gen` is `{"blocks": B, "items": I, "rows": R, "unreadable": U}`, each an integer from 0, an absent member being 0, and names this board, which the test builds:

- `rules` 1;
- `I` items, item `i` (1 ≤ `i` ≤ `I`) being `{"created":"1970-01-01T00:00:00.000000000Z","id":i,"kind":"task","name":"t","stamps":{"estimate":[1,0],"levels":[1,0],"name":[1,0],"repeat":[1,0],"time":[1,0]}}`;
- `B` blocks with sids 1 to `B`, block `s` in item `((s − 1) div 1000) + 1` (so `B ≤ 1000 × I`), each `{"minutes":15,"sid":s,"stamp":[1,0],"start":"1970-01-01T00:00:00.000000000Z"}`;
- `R` archive rows, row `j` (1 ≤ `j` ≤ `R`) being `{"archived_at":t,"created":"1970-01-01T00:00:00.000000000Z","id":I+j,"kind":"task","name":"t"}` with `t` the instant `j − 1` seconds after 1970-01-01T00:00:00Z;
- `U` unreadable lines, each 65,536 bytes of 0x00 (for the size check of §8.4 step 4);
- unless `I`, `B` and `R` are all 0, the one mark `{"block":B,"device_no":0,"item":I+R}`.

The board passes `check` (§6.12) whenever `I ≤ 10,000`, `B ≤ 1000 × I`, `B ≤ 50,000`, `R ≤ 200,000` and `U ≤ 10,000`.

### 18.4 Transcripts

Transcripts are `foundation`'s recorded sequences (SYNC_PLAN.md §6.1 item 5): a starting legacy folder, a pinned clock and `TZ`, and per step today's command JSON, today's reply and today's `BoardState` after it. The core replays them as §13.5 states, starting from `import` of the starting folder, with an **id map** `M` (initially the identity; renumberings of the import are added): before compiling a step, every id in the recorded command is translated through `M`, and a recorded `session` index `i` becomes the sid of the `i`-th block of that item in ascending sid; after compiling, when the recorded reply has an `id` `r` and the compiled reply an `id` `c`, `M[r] = c`. The recorded and replayed states are compared through this projection, with ids translated through `M`:

- items as a set keyed by id: `name`, `created` truncated to milliseconds, `deadline`, kind (`routine` when `recurrence` is present, else `event` when `is_event`, else `task`), `importance`, `time_importance`, `duration_minutes`, `recurrence`, and the sessions in index order against the blocks in ascending sid, each as `(start instant, minutes)`;
- the archive as a multiset of rows with the same fields plus `archived_at` and `outcome`, blocks as `(start, minutes)` in order;
- `notes`;
- subscriptions as a set keyed by id: `name`, `url`, `color`, `enabled`.

Not compared: today's `version`, the overlay, and everything new (stamps, marks, `notes_rev`, settings).

### 18.5 Snapshot fixtures

For each fixture board, `foundation` records the JSON today's `phone::snapshot` returns, with its inputs (`from`, `days`, pinned `now`, `TZ`, palette, `ranked`, look, dials and version). The core builds `SnapshotInput` from `import` of the same folder, the recorded overlay, look, dials, `ranked` and version, and the zone of the recorded `TZ`, and compares JSON values (object member order ignored) after transforming the recorded JSON by: sorting `items` by `id`; replacing each `session` and `SessionDetail.index` by the sid the import gave that item's session; rewriting each `DoneRow.archived_at` as the canonical text of the same instant; adding `notes_base: 0`; replacing `palette` by the imported palette. Changes 4 and 8 of §14.3 show only where a fixture has equal sort keys; the fixture boards have none (§19, `foundation`).

### 18.6 Fuzz and property tests

Fuzz targets under `crates/taskdeck-core/fuzz/` (SYNC_PLAN.md §6.2 item 9): `command_apply` (bytes, through `parse_command`, applied to a fixture board), `board_bytes`, `intent_compile`, `ics_parse`, `snapshot`. Property tests (proptest, a dev-dependency): **determinism** — random command sequences from compiled random intents; a second replica built from the canonical bytes at a random point plus the rest of the log; under two `TZ` values for the compile steps' zones and the process; equal canonical bytes and equal notices at the end; **rebase** — commands compiled on a pending view, applied over a confirmed state that other devices' commands moved on, never panic, and give the same results on two replicas.

### 18.7 A fixture folder the import refuses

`import` refuses two of `foundation`'s boards, as §15.3 requires: `dst-edges` with `instant_range` (two items and one archive row hold an instant before 1970-01-01T00:00:00Z, written as 1970-01-01 in a zone east of Greenwich) and `id-limits` with `id_range` (ids with a high part above 8191 or a low part of 0). The tests of §18.4 and §18.5 treat such a folder as follows.

- **`instant_range`.** The test removes from `read_at_startup.json` and `archived.jsonl` exactly the items and rows that hold an instant outside §5.1's range, checks that their ids are the ids the failure lists, and imports the repaired folder. A transcript is then replayed and compared as §18.4 states, with the removed records left out of every recorded state; a recorded command that names a removed item's id or a removed row's key is classified changed (§13.5). A snapshot is compared as §18.5 states after every object that carries a removed item's `id` is deleted from the recorded `items`, `tray.due`, `tray.later` and `days[].entries`, every such id from `ranked`, and every removed row from `done`. A snapshot in which a removed record places an entry or a ghost on one of its days is not compared: that day's `summary` counts it.
- **Any other failure.** The folder's transcript and snapshots are not replayed. The test asserts the failure kind and the ids the failure lists: for `id-limits`, `id_range` with exactly the folder's ids that §15.3 step 9.4 names.

The import's own vectors (§18.2, `import.json`) record both refusals.

## 19. Interfaces

**`seal.md`.**
- Provides to seal: the canonical encoding v1 and its encoder and parser over `serde_json::Value` (§4, CORE-ENC-008), which omit exactly the fields a schema gives a default (`seal.md` §5.3, §25); the canonical board bytes (§6.13) for snapshot bodies and backups; `BoardData.rules` mirroring `M.rules` (§11.2); the `PictureRef` fields that `seal.md` §25 requires of a blob-referencing command (§6.9).
- Requires from seal: `device_no` assigned at `DeviceAdded`, 1 to 8191, never reused (`seal.md` §13.7); a `cmd` payload returned unparsed (SEAL-KIND-002) and at most 257,979 bytes (`seal.md` §10.2); rules activation only through an accepted `RulesUpgrade` (`seal.md` §13.10); hash version 1 and envelope (1, 1) allowed by rules version 1 (`seal.md` §14.3).
- I assume `seal.md` §5.3's "byte strings are lowercase hex" is the seal profile and does not forbid this spec's `base64` type for `unreadable` lines, which no seal payload carries.

**`engine.md`.**
- Requires: the engine fills `Ctx` as §8.1 states, from the entry's seq, the author's `device_no` in `M_{s−1}`, `at_ms` and `based_on_seq`; for entries applied to the pending state, a provisional seq above the confirmed head, increasing in outbox order.
- Requires: the engine seals each compiled command with `at_ms` equal to `CompileCtx.now` in milliseconds and `based_on` equal to its confirmed head when it compiled (V2-5), so that `apply`'s `Ctx.at` is the clock `compile` used (CORE-COMP-007).
- Requires: the engine calls `parse_command` only on a payload that `seal` has authenticated and opened (SYNC_PLAN.md §3.4: nothing parsed before it is authenticated), and maps its failure to the `Refuse` outcome (`seal.md` §14.4).
- Requires: the engine calls `upgrade_rules` when the entry activating a version is accepted, before applying any later entry, and halts on `core/unknown_rules`.
- Requires: the engine records notices only from applies to the confirmed state, keeps a notice for its own `device_no` as `author` or `other`, and a refusal of its own entries, as the records of §10.7 in the sealed notices file, and records each seq's records once however often it applies the entry, a Skip replay or an adoption replacing those it changes (`engine.md` §13.4).
- Requires: the engine's naming hold (V2-17) coalesces a held `create_*` with a following `rename` into one create by `LogCommand::with_name`, and drops a held create followed by `forget` together with the forget.
- Requires: rebase re-applies the pending log commands, never re-compiles their intents, so a pending command's ids and instants are fixed at compile time.
- Requires: a resubmit after a re-pair (`engine.md` §24.5) rebinds the commands with `ids::rebind` (§7.5) from the new identity's current minter against its pending view, and applies them with the resubmitting device's `at_ms` and head, so their `created` instants, archive clamps and overwrite notices follow that clock and head.
- Requires: the state hash is composed as ruling S7 with this spec's canonical board bytes for hash version 1; the snapshot's `version` is the engine's view counter, which changes whenever the pending state does.
- Provides: §8 to §11 as pure functions, including `reset` (§11.3) behind `SM::reset` and `ids::rebind` (§7.5) behind `SM::rebind`; `RefusalRecord::to_canonical` (§10.7) behind `SM::refusal_bytes`; `compile` for the trial apply at enqueue.

**`device.md`.**
- Requires: the device persists the `IdMinter` with the outbox entry that consumed its numbers, in the same fsynced write (§7.2), and raises it to `mark + 1` after a catch-up that shows a higher mark.
- Requires: the device supplies the zone (`ChronoZone(chrono::Local)` or a `TableZone`, which it may pass as `&dyn Zone`, §5.2) and its overlay to compile and read calls, and keeps the overlay outside `BoardData`.
- Requires: the local-only `Store` writes today's files from the `ChangeSet`, archive changes before item changes (R1 §3), and drops overlay events for every id in `overlay_forget`; in synced mode the device drops the overlay of every subscription absent or disabled in the new pending view (`device.md` §13.1.3).
- Requires: checkpoints, bases and backups hold the canonical board bytes, loaded with `from_canonical_bytes` (§6.14).
- Requires: for the import, the device or desktop reads the legacy files, refuses a held `.lock`, and fills `LegacyConfig` with today's `userconfig.toml` parser (§15.2).
- Provides: `import` keeps every device-minted id and sets the mark of each high part it keeps (§15.3 steps 9.4 and 14), so the plain files that `device.md`'s local-only mode (§9.2), "Export legacy" (§18.7) and the recovery tool (§15.8) write read back with their ids unchanged.
- Relies on: `device.md` §6.4.5 carries each §10.7 record, at most 4,096 bytes, as the record of one `Add` op in the sealed file of kind 8 (`notices.tdk`, `seal.md` §19.2), up to 1,024 ops per sealed record.

**`bridge.md`.**
- Requires: the bridge parses the page's command JSON with `parse_intent`, compiles it, and answers with today's reply shape `{"ok": true, "version": …, "id"?, "session"?, "moved"?}`, `session` being the sid, or `{"error": message}` with the `CompileError`'s status.
- Requires: `/api/state` is answered by `answer_snapshot` with `from` defaulting to today in the device's zone and `days` to 1.
- Requires: the forked page sends `set_notes` with `base` from the snapshot's `notes_base`, and shows its notes conflict sheet when its entry is refused with `core/notes_stale`.
- Provides: nothing to the core.

**`relay.md`.** No interface: the relay never reads core data.

**Stage `desktop`.** Requires: the import check prints the report of §15.4 and SHA-256 of the genesis bytes; the palette and home place are published with `set_palette` and `set_home`; the notepad edits a draft and sends `set_notes` with its base; the UI keeps its own display order.

**Stage `android`.** Requires: reminders come from `reminders` (§14.5); Done and Tomorrow compile intents as §14.5 says.

**Stage `foundation`.** I assume the fixtures (SYNC_PLAN.md §6.1 item 5) record, per board, the inputs of §18.5, per transcript the data of §18.4, and avoid equal sort keys where today's order and the new total orders could differ (equal `archived_at`, equal reflow starts, equal ranking scores).

## 20. Public API

The `taskdeck-core` crate. Pure functions and plain data; no I/O, no clock. Signatures are Rust; the skeleton must compile with exactly these names. Types copied from today's `src/` keep their today's names and fields except as this spec changes them.

```rust
#![forbid(unsafe_code)]

// ---------- errors (§16)
pub struct CoreError { pub code: &'static str, pub detail: Option<u32> }   // detail: §6.12 rule number for core/invariant
pub struct CompileError { pub code: &'static str, pub status: u16, pub message: String }

// ---------- canonical encoding (§4)
pub mod canon {
    pub fn encode(value: &serde_json::Value) -> Result<Vec<u8>, super::CoreError>;      // core/schema for null or a float
    pub fn parse(bytes: &[u8], limit: usize) -> Result<serde_json::Value, super::CoreError>; // §4.5 steps 1–3 and 5
    pub fn is_canonical(bytes: &[u8], limit: usize) -> bool;
    pub trait Record: Sized { const LIMIT: usize;                                        // the record's byte limit (§17), checked before anything is parsed
                              fn to_value(&self) -> serde_json::Value;
                              fn from_value(v: &serde_json::Value) -> Result<Self, super::CoreError>;
                              fn to_canonical(&self) -> Vec<u8>;
                              fn from_canonical(bytes: &[u8]) -> Result<Self, super::CoreError>; }
}

// ---------- time (§5)
pub mod time {
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
    pub struct Instant { /* secs: i64, nanos: u32 */ }
    impl Instant {
        pub const MIN: Instant; pub const MAX: Instant;
        pub fn from_parts(secs: i64, nanos: u32) -> Option<Instant>;
        pub fn from_at_ms(at_ms: i64) -> Option<Instant>;
        pub fn to_canonical(&self) -> String;                         // 30 bytes
        pub fn parse_canonical(text: &str) -> Option<Instant>;
        pub fn from_chrono(t: chrono::DateTime<chrono::Utc>) -> Option<Instant>;
        pub fn to_chrono(&self) -> chrono::DateTime<chrono::Utc>;
        pub fn plus_seconds(&self, s: i64) -> Option<Instant>;
        pub fn as_unix_ms(&self) -> i64;                              // floor
    }
    pub enum LocalTime { None, Single(Instant), Ambiguous(Instant, Instant) }
    pub trait Zone { fn offset_at(&self, utc: Instant) -> i32; fn from_local(&self, local: chrono::NaiveDateTime) -> LocalTime; }
    pub struct FixedZone(pub i32);
    pub struct TableZone { pub initial: i32, pub transitions: Vec<(i64, i32)> }
    impl TableZone { pub fn new(initial: i32, transitions: Vec<(i64, i32)>) -> Result<Self, super::CoreError>; }
    pub struct ChronoZone<Z: chrono::TimeZone>(pub Z);
    impl Zone for FixedZone {} impl Zone for TableZone {} impl<Z: chrono::TimeZone> Zone for ChronoZone<Z> {}
    pub fn local_day(t: Instant, z: &(impl Zone + ?Sized)) -> chrono::NaiveDate;
    pub fn local_minutes(t: Instant, z: &(impl Zone + ?Sized)) -> i32;
    pub fn resolve_on_day(day: chrono::NaiveDate, minutes: i32, z: &(impl Zone + ?Sized)) -> Option<Instant>;
    pub fn now_marker(day: chrono::NaiveDate, now: Instant, z: &(impl Zone + ?Sized)) -> Option<i32>;
    pub fn snap(m: i64) -> i32;
    pub fn clamp_block(start: i64, minutes: i64) -> (i32, u32);
    pub fn snap_up(m: i32) -> i32;
}

// ---------- board data (§6)
pub mod board {
    pub enum Kind { Task, Event, Routine }
    pub enum Outcome { Finished, Dropped }
    pub struct Stamp { pub seq: u64, pub device_no: u16 }
    pub struct ItemStamps { pub estimate: Stamp, pub levels: Stamp, pub name: Stamp, pub repeat: Stamp, pub time: Stamp }
    pub struct Recurrence { pub days: u8, pub anchor: chrono::NaiveDate, pub start_minutes: i32, pub minutes: u32 }
    pub struct Block { pub sid: u64, pub start: Instant, pub minutes: u32, pub stamp: Stamp }
    pub struct RowBlock { pub sid: u64, pub start: Instant, pub minutes: u32 }
    pub struct Item { pub id: u64, pub kind: Kind, pub name: String, pub created: Instant, pub deadline: Option<Instant>,
                      pub importance: Option<u8>, pub time_importance: Option<u8>, pub duration_minutes: Option<u32>,
                      pub recurrence: Option<Recurrence>, pub blocks: Vec<Block>, pub stamps: ItemStamps }
    pub struct ArchiveRow { pub id: u64, pub kind: Kind, pub name: String, pub created: Instant, pub deadline: Option<Instant>,
                            pub importance: Option<u8>, pub time_importance: Option<u8>, pub duration_minutes: Option<u32>,
                            pub recurrence: Option<Recurrence>, pub blocks: Vec<RowBlock>, pub archived_at: Instant,
                            pub outcome: Outcome }
    pub struct ArchiveKey { pub id: u64, pub archived_at: Instant }
    pub struct Subscription { pub id: u64, pub name: String, pub url: String, pub color: [u8; 4], pub enabled: bool }
    pub struct Home { pub lat: i32, pub lon: i32 }
    pub struct PictureRef { pub blob: [u8; 16], pub epoch: u32, pub parts: u32, pub sha256: [u8; 32], pub size: u64 }
    pub struct Mark { pub device_no: u16, pub item: u64, pub block: u64 }
    pub struct BoardData {
        pub rules: u32, pub items: Vec<Item>, pub archive: Vec<ArchiveRow>, pub unreadable: Vec<Vec<u8>>,
        pub notes: String, pub notes_rev: u64, pub subscriptions: Vec<Subscription>,
        pub palette: [[u8; 4]; 6], pub home: Home, pub picture: Option<PictureRef>, pub marks: Vec<Mark>,
    }   // fields public for reading; mutation only through apply, upgrade_rules and import
    impl BoardData {
        pub fn empty() -> Self;                                          // §15.3: {"rules":1}
        pub fn to_canonical_bytes(&self) -> Vec<u8>;                     // §6.13
        pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, super::CoreError>;
        pub fn check(&self) -> Result<(), super::CoreError>;             // §6.12
        pub fn item(&self, id: u64) -> Option<&Item>;
        pub fn subscription(&self, id: u64) -> Option<&Subscription>;
        pub fn mark(&self, device_no: u16) -> Mark;                      // zero when absent
    }
}

// ---------- ids (§7)
pub mod ids {
    pub const LOW_BITS: u32 = 40;
    pub fn compose(device_no: u16, n: u64) -> Option<u64>;
    pub fn split(id: u64) -> (u16, u64);
    pub struct IdMinter { pub device_no: u16, pub next_item: u64, pub next_block: u64 }
    impl IdMinter { pub fn new(device_no: u16) -> Self;
                    pub fn mint_item(&mut self, view: &super::board::BoardData) -> Option<u64>;
                    pub fn mint_block(&mut self, view: &super::board::BoardData) -> Option<u64>; }
    pub fn rebind(commands: &[super::command::LogCommand], from_device_no: u16, view: &super::board::BoardData,
                  minter: &mut IdMinter) -> Result<Vec<super::command::LogCommand>, super::CoreError>;   // §7.5
}

// ---------- log commands (§9)
pub mod command {
    pub enum LogCommand { CreateTask(CreateTask), CreateEvent(CreateEvent), CreateRoutine(CreateRoutine),
        Rename { id: u64, name: String }, Complete { id: u64, at: Instant }, Delete { id: u64, at: Instant },
        Forget { id: u64 }, MoveBlock { id: u64, sid: u64, start: Instant, minutes: u32 },
        MoveBlocks { moves: Vec<BlockMove> }, AddBlock { id: u64, sid: u64, start: Instant, minutes: u32 },
        RemoveBlocks { blocks: Vec<(u64, u64)> }, SetEstimate { id: u64, minutes: u32 },
        SetDeadline { id: u64, deadline: Option<Instant>, seed: Option<u8> }, SetSeverity { id: u64, level: u8 },
        SetHorizon { id: u64, level: u8 }, SetRepeat { id: u64, days: u8, anchor: chrono::NaiveDate },
        MoveRoutine { id: u64, start_minutes: i32, minutes: u32 }, SetEventSpan { id: u64, start: Instant, minutes: u32 },
        SetNotes { text: String, base: u64 }, Restore { id: u64, archived_at: Instant, fallback: u64 },
        ForgetArchived { id: u64, archived_at: Instant },
        AddSubscription { id: u64, name: String, url: String, color: [u8; 4] }, RemoveSubscription { id: u64 },
        RenameSubscription { id: u64, name: String }, SetSubscriptionColor { id: u64, color: [u8; 4] },
        SetSubscriptionEnabled { id: u64, enabled: bool }, SetPalette { colors: [[u8; 4]; 6] },
        SetHome { lat: i32, lon: i32 }, SetPicture { picture: Option<super::board::PictureRef> } }
    pub struct CreateTask { pub id: u64, pub name: String, pub deadline: Option<Instant>, pub importance: Option<u8>,
                            pub time_importance: Option<u8>, pub duration_minutes: Option<u32>, pub blocks: Vec<NewBlock> }
    pub struct CreateEvent { pub id: u64, pub name: String, pub start: Instant, pub minutes: Option<u32>,
                             pub importance: Option<u8>, pub time_importance: Option<u8> }
    pub struct CreateRoutine { pub id: u64, pub name: String, pub days: u8, pub anchor: chrono::NaiveDate,
                               pub start_minutes: i32, pub minutes: u32 }
    pub struct NewBlock { pub sid: u64, pub start: Instant, pub minutes: u32 }
    pub struct BlockMove { pub id: u64, pub sid: u64, pub start: Instant, pub minutes: u32 }
    impl LogCommand {
        pub fn op(&self) -> &'static str;
        pub fn to_canonical(&self) -> Vec<u8>;
        pub fn with_name(self, name: String) -> Result<LogCommand, LogCommand>;   // Err(self) unless a create_* (V2-17)
        pub fn target(&self) -> Option<u64>;
    }
    pub fn parse_command(bytes: &[u8]) -> Result<LogCommand, super::CoreError>;   // §9.1; apply checks the rules
}

// ---------- apply and rules (§8, §10, §11)
pub mod apply {
    pub struct Ctx { pub seq: u64, pub device_no: u16, pub at: Instant, pub based_on: u64 }
    pub struct Applied { /* §8.3 */ }  pub struct ChangeSet { /* §8.3 */ }
    pub struct Refusal { /* §8.3 */ }  pub struct NotesConflict { pub base: u64, pub current: u64 }
    pub enum Group { Name, Time, Levels, Estimate, Repeat, Block, Item }
    pub enum Notice { Overwrite { seq: u64, author: u16, other: u16, other_seq: u64, item: u64, sid: Option<u64>, group: Group },
                      Skipped { seq: u64, author: u16, item: u64, sid: u64, reason: SkipReason, other: Option<(u16, u64)> } }
    pub enum SkipReason { Changed, Gone }
    pub struct RefusalRecord { pub seq: u64, pub author: u16, pub code: String, pub op: Option<String>,
                               pub target: Option<u64>, pub base: Option<u64>, pub current: Option<u64> }
    impl Notice { pub fn to_canonical(&self) -> Vec<u8>; pub fn from_canonical(b: &[u8]) -> Result<Self, super::CoreError>;
                  pub fn concerns(&self, device_no: u16) -> bool; }
    impl RefusalRecord { pub fn to_canonical(&self) -> Vec<u8>; pub fn from_canonical(b: &[u8]) -> Result<Self, super::CoreError>; }
    pub const SUPPORTED_RULES: &[u32] = &[1];
    pub const HASH_VERSIONS: &[u32] = &[1];
    pub fn apply(board: &mut BoardData, command: &LogCommand, ctx: &Ctx) -> Result<Applied, Refusal>;
    pub fn upgrade_rules(board: &mut BoardData, v: u32) -> Result<(), super::CoreError>;
    pub fn reset(target: &BoardData, current: &BoardData) -> Result<BoardData, super::CoreError>;   // §11.3
}

// ---------- intents and compile (§12, §13)
pub mod intent {
    pub enum WireTime { Exact(chrono::DateTime<chrono::FixedOffset>), Naive(chrono::NaiveDateTime, String) }
    pub enum Intent { /* one variant per row of §12.3, fields as its table; Create { kind: super::board::Kind, name: String, day: chrono::NaiveDate, start: i32, minutes: u32 } */ }
    pub fn parse_intent(text: &str) -> Result<Intent, super::CompileError>;
}
pub mod compile {
    pub struct CompileCtx<'a, Z: Zone + ?Sized> { /* §13.1 */ }
    pub struct Compiled { pub command: Option<LogCommand>, pub reply: Reply }
    pub struct Reply { pub id: Option<u64>, pub session: Option<u64>, pub moved: Option<u32> }
    pub fn compile<Z: Zone + ?Sized>(intent: &Intent, cx: &CompileCtx<Z>, minter: &mut IdMinter) -> Result<Compiled, CompileError>;
    pub fn checked_url(url: &str) -> Result<String, CompileError>;
    pub fn trim(text: &str) -> &str;                                       // White_Space only
    pub fn detab(text: &str) -> String;
}

// ---------- read side (§14), copied modules with the zone parameter; every zone parameter, here and in planner, archive, ics and the snapshot builders, is &(impl Zone + ?Sized) or Z: Zone + ?Sized (§5.2)
// Each module carries exactly the items below: those of §14.2's table and of today's src/, without UI-only items and file I/O.
// "today's fields" and "today's variants" are those of the type of that name in today's src/, with §14.2's changes.
pub mod planner {
    pub const DAY_MINUTES: i32 = 24 * 60;
    pub const SNAP_MINUTES: i32 = 15;
    pub const MIN_BLOCK_MINUTES: u32 = 15;
    pub const DEFAULT_BLOCK_MINUTES: u32 = 30;
    pub enum Placement { /* today's variants */ }
    impl Placement { pub fn start(&self) -> i32; pub fn end(&self) -> i32; }
    pub struct DayPlacement { /* today's fields; session: Option<u64>, a sid */ }
    pub struct Placeable<'a> { /* today's fields, borrowed from an &'a Item */ }
    impl<'a> Placeable<'a> { pub fn of(item: &'a Item) -> Self; }
    pub fn placements_for(item: &Item, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> Vec<DayPlacement>;
    pub fn placements_of(item: Placeable, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> Vec<DayPlacement>;
    pub fn appears_on(item: &Item, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> bool;
    pub struct Lane { /* today's fields */ }
    pub fn lay_out(placements: &[Placement]) -> Vec<Lane>;
    pub struct DaySummary { /* today's fields */ }
    pub fn summarize(placements: &[Placement]) -> DaySummary;
    pub fn summary_text(planned: DaySummary, routine: DaySummary, done: DaySummary) -> String;
    pub fn format_minutes(minutes: i32) -> String;
    pub fn format_duration(minutes: u32) -> String;
    pub fn drop_length_for(item: &Item) -> u32;
    pub fn duration_options(current: Option<u32>) -> Vec<u32>;
    pub enum BacklogGroup { /* today's variants */ }
    pub fn backlog_group(deadline: Option<Instant>, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> BacklogGroup;
    pub fn relative_due(deadline: Instant, now: Instant) -> String;
    pub struct Flowable<K> { /* today's fields */ }
    pub fn behind_minutes(blocks: &[Placement], now: i32) -> i32;
    pub fn reflow<K: Copy>(blocks: &[Flowable<K>], anchors: &[(i32, i32)], now: i32) -> Vec<(K, i32)>;
}   // now_marker, snap, clamp_block and resolve_on_day are in time, not here (§14.2)
pub mod tasks {
    pub const WEEKDAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    pub const EVERY_DAY: u8 = 0b0111_1111;
    pub const WEEKDAYS: u8 = 0b0001_1111;
    pub const IMPORTANCE_LABELS: [&str; 5];                       // today's labels
    pub const HORIZON_LABELS: [&str; 4] = ["Within a month", "Within a week", "Within days", "Whenever"];
    pub const HORIZON_DISPLAY_ORDER: [u8; 4] = [2, 1, 0, 3];
    pub const ROUTINE_COLOR_INDEX: usize = 0;
    pub const PALETTE_LEN: usize = 6;
    pub const HORIZON_WHENEVER: u8 = 3;
    pub const WEIGHT_BY_IMPORTANCE: [f32; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];
    pub const LEAD_DAYS_BY_IMPORTANCE: [f32; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];
    pub const WEIGHT_BY_HORIZON: [f32; 4] = [1.0, 2.0, 4.0, 0.5];
    pub const RIPEN_DAYS_BY_HORIZON: [f32; 4] = [30.0, 10.0, 3.0, 90.0];
    pub const PLANNED_LEAD_DAYS: f32 = 0.5;
    pub const OVERDUE_DOUBLING_DAYS: f32 = 1.0;
    pub const OVERDUE_PRESSURE_CAP: f32 = 4.0;
    pub const ASSUMED_IMPORTANCE: u8 = 2;
    pub const MALFORMED_SCORE: f32 = 1.0e6;
    pub const JITTER: f32 = 0.08;
    pub fn weekday_bit(day: chrono::NaiveDate) -> u8;
    pub fn calendar_item_color(is_event: bool, importance: Option<u8>, time_importance: Option<u8>) -> usize;
    impl Recurrence { pub fn falls_on(&self, day: chrono::NaiveDate) -> bool; pub fn repeats(&self) -> bool;
                      pub fn includes(&self, index: u32) -> bool; pub fn summary(&self) -> String;
                      pub fn summary_dated(&self) -> String; }
    impl Item { pub fn importance_score(&self, now: Instant, seed: u64) -> f32;
                pub fn tie_break_jitter(&self, seed: u64) -> f32;
                pub fn is_routine(&self) -> bool;
                pub fn calendar_item_color(&self) -> usize;
                pub fn planner_anchor(&self) -> Option<Instant>;
                pub fn is_planned(&self) -> bool;
                pub fn planned_minutes(&self) -> u32;
                pub fn remaining_minutes(&self) -> Option<u32>;
                pub fn wants_planning(&self) -> bool; }
}
pub mod archive {
    pub enum Timing { /* today's variants */ }
    impl ArchiveRow { pub fn verdict(&self, zone: &(impl Zone + ?Sized)) -> String;
                      pub fn timing(&self) -> Timing;
                      pub fn was_finished(&self) -> bool;
                      pub fn was_judged(&self) -> bool;
                      pub fn mark(&self) -> &'static str;
                      pub fn lifetime(&self) -> chrono::TimeDelta;
                      pub fn booked_minutes(&self) -> u32;
                      pub fn sittings(&self) -> usize;
                      pub fn color_id(&self) -> usize;
                      pub fn is_routine(&self) -> bool; }
    pub fn format_span(span: chrono::TimeDelta) -> String;
    pub fn format_total(minutes: u64) -> String;
    pub struct Filter { /* today's fields */ }
    pub enum OutcomeFilter { /* today's variants */ }
    pub enum KindFilter { /* today's variants */ }
    impl Filter { pub fn admits(&self, row: &ArchiveRow) -> bool; pub fn is_open(&self) -> bool; }
    pub struct Summary { /* today's fields */ }
    impl Summary { pub fn headline(&self) -> String; }
    pub fn summarize<'a>(rows: impl IntoIterator<Item = &'a ArchiveRow>) -> Summary;
    pub struct MonthRun { /* today's fields */ }
    pub fn group_by_month(rows: &[&ArchiveRow], zone: &(impl Zone + ?Sized)) -> Vec<MonthRun>;   // rows newest first
}
pub mod ics {
    pub struct Occurrence { /* today's fields */ }
    impl Occurrence { pub fn is_busy(&self) -> bool; }
    pub struct Parsed { /* today's fields */ }
    pub struct Limits { /* today's fields */ }
    impl Default for Limits {}
    pub fn parse(text: &str, from: chrono::NaiveDate, until: chrono::NaiveDate, limits: Limits,
                 zone: &(impl Zone + ?Sized)) -> Result<Parsed, String>;
}
pub mod overlay {
    pub struct Covered { /* today's fields */ }
    impl Covered { pub fn holds(&self, day: chrono::NaiveDate) -> bool; }
    pub struct OverlayEvent { /* today's fields */ }
    pub const LONG_EVENT_MINUTES: i32 = 12 * 60;
    impl OverlayEvent { pub fn minutes(&self) -> i32; pub fn is_background(&self) -> bool; pub fn is_busy(&self) -> bool; }
    pub enum FetchOutcome { /* today's variants */ }
    pub fn placeholder_name(id: u64) -> String;
    pub fn name_from_url(url: &str) -> Option<String>;
    pub fn better_name(subscription: &Subscription, overlay: &Overlay) -> Option<String>;
    pub fn is_unnamed(subscription: &Subscription) -> bool;
    pub struct FetchStatus { /* today's fields; at: chrono::DateTime<chrono::FixedOffset> (§14.6) */ }
    pub struct Overlay { /* today's fields */ }
    impl Overlay { pub fn covering(self, covers: Option<Covered>) -> Overlay;
                   pub fn sealed(events: Vec<OverlayEvent>, status: Vec<FetchStatus>) -> Overlay;
                   pub fn digest(&self) -> u64;
                   pub fn anchors_on(&self, day: chrono::NaiveDate) -> Vec<(i32, i32)>;
                   pub fn events_on(&self, day: chrono::NaiveDate) -> impl Iterator<Item = &OverlayEvent>;
                   pub fn forget(&mut self, subscription: u64);
                   pub fn status_for(&self, subscription: u64) -> Option<&FetchStatus>; }
}
pub mod weather {
    // The response types that today's code keeps private are public here, so that build_report can be called.
    pub struct WeatherResponse { /* today's fields */ }
    pub struct HourlyData { /* today's fields */ }
    pub struct CurrentData { /* today's fields */ }
    pub struct DailyData { /* today's fields */ }
    pub struct Hour { /* today's fields */ }
    pub struct Day { /* today's fields */ }
    pub struct Now { /* today's fields */ }
    pub struct Report { /* today's fields */ }
    pub fn build_report(json: &WeatherResponse, coordinates: [f32; 2]) -> Report;
    pub fn sky_symbol(code: i32, is_day: bool) -> &'static str;
    pub struct City { /* today's fields */ }
    pub fn nearest_city(latitude: f32, longitude: f32) -> Option<&'static City>;
    pub static CITIES: &[City];                                   // today's table
}
pub mod read {
    pub fn ranked_ids(board: &BoardData, now: Instant, zone: &(impl Zone + ?Sized)) -> Vec<u64>;
    pub fn shuffle_seed(now: Instant, zone: &(impl Zone + ?Sized)) -> u64;
    pub fn behind_minutes(board: &BoardData, day: chrono::NaiveDate, now: Instant, zone: &(impl Zone + ?Sized)) -> i32;
    pub fn next_free_start(board: &BoardData, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized)) -> i32;
    pub fn reflow_inputs(board: &BoardData, overlay: &Overlay, day: chrono::NaiveDate, zone: &(impl Zone + ?Sized))
        -> (Vec<planner::Flowable<(u64, u64)>>, Vec<(i32, i32)>);
    pub fn archive_newest_first(board: &BoardData) -> impl Iterator<Item = &ArchiveRow>;
    pub fn archive_by_day<'a>(board: &'a BoardData, from: chrono::NaiveDate, days: u32, zone: &(impl Zone + ?Sized))
        -> Vec<Vec<(&'a ArchiveRow, &'a RowBlock)>>;
}
pub mod snapshot {
    pub struct SnapshotInput<'a, Z: Zone + ?Sized> { /* §14.3 */ }
    pub struct Snapshot { /* today's fields, plus notes_base: u64 */ }
    pub struct Look { pub id: String, pub aspect: f32, pub top: String }
    pub struct Dials { pub blur: u32, pub light: u32 }
    pub struct DoneRow { /* today's fields */ }
    pub struct Labels { /* today's fields */ }
    impl Labels { pub fn current() -> Self; }
    pub struct DaySnapshot { /* today's fields */ }
    pub struct SubscribedSnapshot { /* today's fields */ }
    pub struct EntrySnapshot { /* today's fields */ }
    pub struct GhostSnapshot { /* today's fields */ }
    pub struct Tray { /* today's fields */ }
    pub struct TrayCard { /* today's fields */ }
    pub struct ItemDetail { /* today's fields */ }
    pub struct RepeatDetail { /* today's fields */ }
    pub struct SessionDetail { /* today's fields */ }
    pub fn snapshot<Z: Zone + ?Sized>(input: &SnapshotInput<Z>) -> Snapshot;
    pub fn answer_snapshot<Z: Zone + ?Sized>(input: &SnapshotInput<Z>) -> serde_json::Value;
    pub fn build_day(items: &[Item], ghosts: &[(&ArchiveRow, &RowBlock)], day: chrono::NaiveDate, now: Instant,
                     zone: &(impl Zone + ?Sized)) -> DaySnapshot;                       // §14.3
    pub fn build_tray(items: &[Item], day: chrono::NaiveDate, now: Instant, shuffle_seed: u64,
                      zone: &(impl Zone + ?Sized)) -> Tray;                             // §14.3 item 4
    pub fn item_detail(item: &Item, zone: &(impl Zone + ?Sized)) -> ItemDetail;        // §14.3 item 6
    pub fn done_rows<'a>(rows: impl IntoIterator<Item = &'a ArchiveRow>, zone: &(impl Zone + ?Sized)) -> Vec<DoneRow>;   // newest first, §14.3 items 7 and 8
    pub fn with_subscribed(days: Vec<DaySnapshot>, subscriptions: &[Subscription], overlay: &Overlay,
                           zone: &(impl Zone + ?Sized)) -> Vec<DaySnapshot>;             // §14.3 item 11
}
pub mod reminders {
    pub struct ReminderDefaults { pub event_lead_minutes: u32, pub block_lead_minutes: u32, pub due_lead_minutes: u32 }
    pub enum ReminderKind { Event, Block, Due }
    pub struct Reminder { pub key: String, pub kind: ReminderKind, pub item: u64, pub sid: Option<u64>, pub at: Instant,
                          pub fire_at: Instant, pub minutes: Option<u32>, pub name: String }
    impl Reminder { pub fn to_canonical(&self) -> Vec<u8>; }
    pub fn reminders(board: &BoardData, from: Instant, until: Instant, defaults: &ReminderDefaults) -> Vec<Reminder>;
}

// ---------- legacy import (§15)
pub mod legacy {
    pub struct LegacyFolder { /* §15.2 */ }  pub struct LegacyConfig { /* §15.2 */ }
    pub struct ImportReport { /* §15.4 */ }  pub struct Renumbered { pub list: IdList, pub old: u64, pub new: u64 }
    pub enum IdList { Items, Archive, Subscriptions }
    pub enum ImportWarning { PaletteUnreadable, HomeOutOfRange, NotesOverCap }
    pub enum FailureKind { TooLarge, ItemsUnreadable, NotesUnreadable, SubscriptionsUnreadable, IdRange, InstantRange,
                           FieldRange, Cap, Invariant }
    pub struct ImportFailure { pub kind: FailureKind, pub ids: Vec<u64> }
    pub fn import(folder: &LegacyFolder) -> (Option<BoardData>, ImportReport);
}
```

## 21. Open questions

Non-normative. Each states the default this spec takes.

1. **Stamps per field group**, not one per item (§6.5, §10.1). SYNC.md §4.6 records "the last (seq, device) of each item and block"; one stamp per item would turn a rename on one device and a new due date on another into a false "changed since you last synced". Decided here: five groups per item, one per block.
2. **A single drag compiles to `move_block`**, which overwrites with a notice, while Reflow's `move_blocks` skips what changed (§10.3). SYNC.md defines only `MoveBlocks`; decided here so that a deliberate move is never silently dropped.
3. **The notes base is a revision counter** (`notes_rev`), not a hash of the text (§10.5). SYNC.md §12 says "base hash"; a counter needs no hash function in the core, cannot match by coincidence, and works for two queued edits from one device. Decided here.
4. **Benign repeats** cover `forget`, `forget_archived`, `remove_blocks` and `remove_subscription` as well as V2-16's three (§10.6). Decided here (SYNC.md is silent on these).
5. **The compile step keeps today's errors** (410) when the origin's own view shows the target already gone, so repeating an action on one device reads as today; benign success arises only between devices. Decided here.
6. **Reminder defaults**: events 10 minutes before, blocks and due times on time, routines never (§14.5). Decided here (SYNC.md §11 open question 11); the `android` stage may expose the leads as device settings, which do not change the core.
7. **Caps in rules version 1** (10,000 live items, 50,000 live blocks, 1,000 blocks per item, 200,000 archive rows, 60 MiB board bytes; §17) are new; today has no caps beyond names, notes and subscriptions. Decided here so that a snapshot always fits `seal.md`'s 64 MiB; raising one is a new rules version.
8. **The zone is a trait** with `ChronoZone` and `TableZone` (§5.2), not `chrono-tz`, which SYNC.md §2.3 does not list. Decided here.
9. **Intents ignore unknown fields** (today's behaviour, so the forked page and older desktops keep working); log commands refuse them. Decided here.
10. **Instants always carry nine fraction digits** in JSON (§4.4). Decided here (SYNC.md: "UTC with nanoseconds").
11. **The home place is two integers in hundredths of a degree**, default `{0, 0}` as today's default. Decided here.
12. **The shared picture is a board setting** (`set_picture`) holding the blob reference `seal.md` §25 asks for. Decided here; only desktops upload it (SYNC.md §3).
13. **Items and subscriptions share one numbering and one mark** (as today's shared counter), with separate uniqueness. Decided here.
14. **The import fails** on an unreadable items, notes or subscriptions file instead of quarantining it as today's startup does, because a genesis must not silently drop data. Decided here.
15. **A legacy archive row that has both a recurrence and `is_event`** maps to `kind` routine and ghosts its blocks, where today it ghosts nothing; no build of today's code writes such a row. Accepted.
16. **A restored row whose sid is live** is refused (`core/id_taken`). It cannot arise from any sequence of commands this spec allows; the check is kept so that `apply` stays total. Decided here.
17. **Routines created or moved on a daylight-saving gap day** are accepted (today refused with 400), because a routine's time floats (V2-4). Decided here.
18. **`rename_subscription` with an empty name** compiles to nothing (today it saves and bumps the version without a change). Decided here.
19. **Snapshot ties**: the snapshot fixtures are assumed to have no equal sort keys (§19, `foundation`), so §14.3's order changes 4 and 8 need no transformation in the comparison. If a generated fixture has them, `foundation` changes the fixture, not this rule.
20. **`TableZone` for Android**: whether the phone passes `ChronoZone(chrono::Local)` or builds a `TableZone` from the platform's rules is `device.md`'s; the core supports both.
