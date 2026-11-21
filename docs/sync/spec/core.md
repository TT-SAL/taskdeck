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

- the confirmed and pending states, rebasing, the outbox, trial apply under `catch_unwind`, the naming hold, the persistent lists of notices and refusals, the skew-corrected clock, halts: `engine.md`;
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
- **CORE-GEN-006** (core) The functions `apply`, `upgrade_rules`, the canonical encoder and parser, `BoardData::check` and the id functions of §7 MUST NOT use `f32` or `f64` values, MUST NOT iterate a `HashMap` or `HashSet`, and MUST NOT depend on the order of any collection other than the orders this spec defines.
- **CORE-GEN-007** (core) Every public function that parses bytes or text (the canonical parser, the log-command parser, the board-bytes parser, the intent parser, the legacy import) MUST return an error, never panic, for any input; a property test over random and mutated inputs MUST prove it for each.
- **CORE-GEN-008** (core) `apply` MUST NOT panic for any `LogCommand` the parser of §9.1 returns and any `BoardData` that passes `BoardData::check` (§6.12); the fuzz target `apply` of §18 MUST prove it.
- **CORE-GEN-009** (core) Every parser MUST compare the input's length with the limit of §17 that applies to it before allocating anything for it, and MUST compute a base64 field's decoded length and check it against its limit before decoding it.
- **CORE-GEN-010** (core) No error, refusal, notice or report that the `core` crate returns MUST contain an item or row name, the notes text, a subscription name or a subscription URL; the only exception is the intent parser's `core/intent_json` message (§16), which may quote the offending field text back to the user who typed it.

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

Example: the instant 2026-09-04 09:00 UTC is `"2026-09-04T09:00:00.000000000Z"`; 2026-08-15T20:28:41.882008+03:00 is `"2026-08-15T17:28:41.882008000Z"`.

### 4.5 Parsing

A canonical parser for a record schema takes bytes and returns the record or an error, in this order:

1. the byte length is at most the limit for that record (§17), else `core/too_large`;
2. the bytes are valid UTF-8 and valid RFC 8259 JSON text with nesting depth at most 32, else `core/not_json`;
3. no object has a duplicate key, else `core/schema`;
4. the value matches the schema: no unknown field, every required field present, every field of its type and constraints, no `null`, no number with a fraction or exponent, else `core/schema`;
5. `enc` of the parsed record equals the input bytes exactly, else `core/not_canonical`.

Step 5 makes the encoding unique: whitespace, key order, escapes, a field written at its default, and the textual forms of instants and integers are all checked by it.

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

An `Instant` is a UTC time with nanosecond resolution: a count of seconds since 1970-01-01T00:00:00Z (`i64`, leap seconds ignored) and a nanosecond part (`u32`, 0 to 999,999,999). Its range is 1970-01-01T00:00:00.000000000Z to 9999-12-31T23:59:59.999999999Z, both inclusive; nothing in the core constructs an instant outside it. Instants are ordered by seconds, then nanoseconds. An instant from a plaintext's `at_ms` (`seal.md` §10.1, 0 to 2^53 − 1) is `seconds = at_ms div 1000`, `nanos = (at_ms mod 1000) × 1,000,000`. "Plus one day" adds 86,400 seconds.

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

A device passes `ChronoZone(chrono::Local)` or a `TableZone`; `device.md` chooses. Inside the core the type `chrono::Local` never appears (CORE-GEN-004).

### 5.3 Local day and minutes

For an instant `t` and a zone `z`, the local date-time is `t + z.offset_at(t)` read as a naive date-time. Its date is the **local day** of `t`, and `hour × 60 + minute` (seconds and fractions dropped) its **minutes**. `t` **falls on** day `d` when its local day is `d`. These replace every `date_naive()`, `hour()` and `minute()` that today's code calls on a `DateTime<Local>`.

### 5.4 Resolving a local time

`resolve_on_day(d, m, z)` turns minutes past midnight on day `d` into an instant, exactly as today's `planner::resolve_on_day` (planner.rs:296-311) does with `z` in place of `Local`:

1. `m' = clamp(m, 0, 1439)`;
2. `local_at(d, x)` is the naive date-time `d` at hour `min(x div 60, 23)`, minute `x mod 60`, second 0; its instant is the earlier one for `Ambiguous`, the one for `Single`, none for `None`;
3. the result is `local_at(d, m')`, or, when that is none, `local_at(d, min(m' + 60, 1439))`, or none.

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

`BoardData` holds no version counter (it is the seq, `engine.md`), no overlay (per device, §14.7), no next-id counter (replaced by `marks`), no per-device settings (backgrounds, dials, window, scale, server settings, tokens) and no time zone (zones are per device, SYNC_PLAN.md §2).

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

`planned_start` is gone: the import folds it into a block (§15.3). An item keeps every field it holds whatever its kind, as today: an event may carry `importance` (`Command::Add` stores it, digest §1.3), and a legacy routine may carry blocks or a deadline. Commands change only the fields their kind uses (§9).

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

1. the record parses under its schema (§6.1 to §6.10) and every array is in its stated order without forbidden duplicates;
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

An item id, a subscription id and a non-zero archive-row id each have the form `h × 2^40 + n`, with `h` from 0 to 8191 and `n` from 1 to 2^40 − 1, so every id is below 2^53 and exact in JavaScript (SYNC.md §4.4, README §5.6). `h ≥ 1` is the `device_no` of the device that minted it (`seal.md` §13.7). `h = 0` is the legacy space: ids from today's data, below 2^40, kept unchanged by the import except where §15.3 renumbers them. Items and subscriptions share one numbering (today's shared `next_id`, board.rs:1332), so a number is minted once for either.

### 7.2 Marks

`marks` records, per `device_no`, the highest item number and the highest block number accepted from that device. A creating command carries the id it creates; `apply` accepts a newly minted id only if its high part is the author's `device_no` and its `n` is above that device's mark, and then raises the mark to `n` (SYNC.md §4.4, V2-3). So no id is ever issued twice, even after Forget, after a refused entry, or from a restored folder whose own counter went back. The legacy space's mark (`device_no` 0) is set by the import to the highest legacy numbers and never moves again, because no author has `device_no` 0.

The origin mints with an `IdMinter`: the next item number is `max(minter.next_item, mark(view, device_no).item + 1)`, and after minting `minter.next_item` becomes that number plus one; blocks likewise with `next_block`. The device persists the minter with its outbox (`device.md`), so a number is never minted twice locally even when the entry that carried it is later refused.

### 7.3 Block ids

A block id (sid) has the same form, `h × 2^40 + n`, with its own marks (`block`). sids are minted by the origin for `add_block` and `create_task`, and by the import for legacy sessions (`h = 0`). A sid is unique among the live blocks of the whole board, not only within its item. It is carried in the phone page's existing `session` field and in `SessionDetail.index` (§14.3); the page only compares and echoes it (phone.html:4260, 4365, 4909-4931, 4939, 5066-5083), so its meaning changes from an index to an id without any change to the page's code. An archived row keeps its blocks' sids, and Restore brings them back unchanged.

### 7.4 Restore

A `restore` carries the row key and a `fallback` id minted by the origin. `apply` restores under the row's own id when it is not 0 and no live item holds it; otherwise under `fallback` (SYNC.md §4.4, R1 §2). The range check of §7.2 applies to `fallback` only, never to the row's own id, which was valid when it was minted (V2-3).

- **CORE-ID-001** (core) Every id and sid in `BoardData` MUST have the form of §7.1 with a non-zero low part and a high part from 0 to 8191.
- **CORE-ID-002** (core) `apply` MUST refuse with `core/bad_id` a command that mints an item, subscription or block id whose high part differs from `Ctx.device_no` or whose low part is not above the author's mark for that kind.
- **CORE-ID-003** (core) `apply` MUST raise the author's mark to the low part of every id it accepts as newly minted, including a `restore`'s `fallback` whether or not the restore used it.
- **CORE-ID-004** (core) The compile step MUST mint each number as `max(minter.next, mark + 1)` of §7.2 and MUST advance the minter past it, so that one minter never yields the same number twice.
- **CORE-ID-005** (core) `apply` MUST restore a row under its own id when that id is not 0 and not held by a live item, and under the command's `fallback` otherwise, without applying the range check to the row's own id.

## 8. `Ctx` and `apply`

### 8.1 `Ctx`

```rust
pub struct Ctx { pub seq: u64, pub device_no: u16, pub at: Instant, pub based_on: u64 }
```

| Field | Source (filled by the engine) | Constraint | Read by `apply` for |
|---|---|---|---|
| `seq` | the entry's seq; for an entry applied to the pending state, the provisional seq `engine.md` assigns | 2 to 2^53 − 1 | every stamp it writes (§10.1); a notice's `seq` |
| `device_no` | the author's `device_no` in the membership before the entry (`seal.md` §13.7) | 1 to 8191 | minted-id checks and marks (§7); stamps; whether a stamp is another device's (§10.2) |
| `at` | the plaintext's `at_ms` (§5.1): the origin's clock when it compiled the command | | `created` of a created item; the archive clamp of `complete` and `delete` |
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

The change set lets `device.md`'s local-only `Store` write today's files in today's order (archive first, then items, R1 §3), and tells a device to drop the overlay events of a removed or disabled subscription: today's `overlay.forget` inside `apply` (board.rs:1355, 1393) becomes this effect (V2 audit item 13).

### 8.4 Evaluation

`apply` evaluates in this order and stops at the first failure:

1. `ctx` satisfies §8.1's constraints, else `core/bad_ctx`;
2. the command's `op` is in the op table of `board.rules` (§11.1), else `core/rules`;
3. the command's checks, in the order its section in §9 lists them;
4. the command's effects, which cannot fail.

A refusal leaves `board` unchanged. An `Applied` with `benign` set has an empty change set and no notices.

- **CORE-CTX-001** (core) `apply` MUST refuse with `core/bad_ctx`, changing nothing, when `ctx.device_no` is outside 1 to 8191, `ctx.seq` is outside 2 to 2^53 − 1, or `ctx.based_on ≥ ctx.seq`.
- **CORE-CTX-002** (core) `apply`'s result and the `BoardData` it leaves MUST be a function of `board`, `command` and the four fields of `ctx` alone: two calls with equal inputs MUST return equal results and leave equal canonical board bytes, under any process time zone, clock and platform.
- **CORE-CTX-003** (core) `apply` MUST set `created` of every item it creates to `ctx.at`, and MUST NOT take an instant from anywhere but `ctx.at` and the command.
- **CORE-APPLY-001** (core) `apply` MUST evaluate the steps of §8.4 in order and MUST return the refusal code of the first check that fails.
- **CORE-APPLY-002** (core) A refused `apply` MUST leave the canonical board bytes unchanged.
- **CORE-APPLY-003** (core) A successful `apply` MUST report in its change set every live item, archive key, subscription and setting it changed, and nothing else; and MUST list in `overlay_forget` every subscription it removed or set to disabled.
- **CORE-APPLY-004** (core) A benign repeat MUST return `Applied` with `benign` set, an empty change set and no notices, and MUST leave the canonical board bytes unchanged.

## 9. Log commands v1

### 9.1 Envelope and parsing

A log command is one record of the canonical encoding with a field `op` naming it. It is the whole `cmd` payload of one entry (`seal.md` §11.2): one entry carries exactly one command. Its byte length is 1 to 257,979 (`seal.md` §10.2 cap).

`parse_command(bytes, rules)` performs, in order:

1. the length check of §4.5 step 1 (limit 257,979 bytes), else `core/too_large`;
2. §4.5 step 2, else `core/not_json`;
3. the value is an object with a string field `op`, and `op` names a command of §9.3, else `core/unknown_op`;
4. §4.5 steps 3 and 4 against that command's schema, else `core/schema`;
5. §4.5 step 5, else `core/not_canonical`;
6. the command's cross-field constraints in its section ("Schema rules"), else `core/schema`.

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

Apply: (1) `base = board.notes_rev`, else `core/notes_stale`, with `NotesConflict{base, current: board.notes_rev}`. Effects: `notes = text`; `notes_rev += 1`. In rules version 1 the cap counts scalars after the origin's detab (SYNC.md §4.6; today counts before, board.rs:1253-1256).

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

None of them has a check of its own.

- **CORE-CMD-001** (core) `parse_command` MUST perform the six steps of §9.1 in order and MUST return the code of the first that fails.
- **CORE-CMD-002** (core) `parse_command` MUST accept exactly the 29 commands of §9.3 with the fields, types, presences and constraints of §9.4 to §9.24, and MUST reject every other `op` with `core/unknown_op`.
- **CORE-CMD-003** (core) `parse_command` MUST reject with `core/schema` an instant in any form other than §4.4's, including a naive local time and an offset other than `Z` (V2-4).
- **CORE-CMD-004** (core) For each command, `apply` MUST perform exactly the checks its section lists, in that order, and on success exactly the effects it lists.
- **CORE-CMD-005** (core) `apply` of `complete` and `delete` MUST set `archived_at` to `min(at, ctx.at + 86,400 s)`.
- **CORE-CMD-006** (core) `apply` MUST NOT trim, detab, default, round, snap, clamp or resolve any value of a command; every such policy belongs to the compile step (§13), and `apply` MUST only check the ranges, lengths, existence and uniqueness its sections list (V2-1).
- **CORE-CMD-007** (core) `apply` of `move_routine`, `create_routine` and `set_repeat` MUST NOT consult any zone or resolve any local time (V2-4).
- **CORE-CMD-008** (core) `apply` of `set_notes` MUST refuse with `core/notes_stale` when `base` differs from `notes_rev`, and on success MUST increase `notes_rev` by exactly 1.
- **CORE-CMD-009** (core) `apply` of `set_deadline` MUST set `importance` to `seed` only when `seed` is present and `importance` is none at the time of apply.

## 10. Conflict rules

### 10.1 Ordering and stamps

The last writer wins per command, in seq order, as today (DOC §4.1), and identically on every device because every device applies the same entries in the same order with the same `apply` (SYNC.md §4.6).

A **stamp** `(seq, device_no)` records which entry last changed something. Each live item carries five (§6.5), each live block one (§6.3). A creating command, `restore` and `add_block` write the entry's stamp `(ctx.seq, ctx.device_no)` on everything they create; every other command writes it on exactly the groups its row of §9.3 names. The legacy import writes `(1, 0)` everywhere (§15.4); `device_no` 0 is never an author. An item's **latest stamp** is the one with the greatest `seq` among its five field stamps and its blocks' stamps.

A stamp `(s, d)` is **another device's change since the origin's view** for an entry with `ctx` when `s > ctx.based_on`, `d ≠ ctx.device_no` and `d ≠ 0`.

### 10.2 Overwrite notices

A command that changes a field group, a block or (for `complete`, `delete`, `forget`) a whole item whose stamp (for an item, its latest stamp) is another device's change since the origin's view still applies, and `apply` produces one **overwrite notice** for it (SYNC.md §4.6, V2-5; SYNC_PLAN.md §2 takes notices rather than refusals). The notice names the overwriting entry (`seq`, `author = ctx.device_no`), the overwritten change (`other`, `other_seq` from the stamp), the item, the sid for a block, and the group (`name`, `time`, `levels`, `estimate`, `repeat`, `block` or `item`). It is shown on two devices: the author's and the `other` device's ("changed on Desk since you last synced"). A command produces at most one notice per group and block it touches.

### 10.3 `move_blocks` skips

A Reflow moves many blocks the origin saw at once. `move_blocks` does not overwrite: a block that is gone, or whose stamp is another device's change since the origin's view, is skipped, and the rest move (V2-5). Each skip produces a `skipped` notice for the author alone, naming the block, the reason (`gone` or `changed`) and, for `changed`, the device and seq of the change. A single drag of one block compiles to `move_block`, which overwrites with a notice (§10.2), because the user aimed at that block.

### 10.4 `remove_blocks`

Unplan compiles to `remove_blocks` with exactly the sids the origin saw (§13.4). A block added since by anyone is not in the list and survives; a listed block already removed is passed over; a listed block another device moved since the origin's view is removed, with an overwrite notice.

### 10.5 Notes and their base

The notes carry a revision `notes_rev`, raised by 1 by every accepted `set_notes`. The **base** of a `set_notes` is the `notes_rev` of the text the origin's user started editing from. A `set_notes` whose base is not the current revision is refused (`core/notes_stale`) on every device, the author's included, with `NotesConflict{base, current}`; nothing changes. The origin still holds its draft (the refused command's `text` is in its outbox entry), so it can show its draft beside the current notes and let the user merge (stage `desktop`, `bridge.md`); the merged text is a new `set_notes` with `base = current`. Two `set_notes` from one device in a row work: the second is compiled on the view in which the first already raised the revision. A counter is used rather than a hash of the text so that the core needs no hash function and a base never matches by coincidence (decided here, §21).

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

Notices and refusals live per device, outside `BoardData`, in the device's sealed notices file (`seal.md` §19.2 kind 8); `engine.md` owns the list (which entries it records, when it shows and clears them), `device.md` the file. Only entries applied to the confirmed state produce records: a notice produced while applying to the pending state is discarded (`engine.md`). A device keeps a notice when its own `device_no` is the notice's `author` or `other`, and a refusal when it is the refused entry's author. Both are canonical-encoding records:

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
| `code` | string | required | a code of §16 or of `seal.md` §22 | why |
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

