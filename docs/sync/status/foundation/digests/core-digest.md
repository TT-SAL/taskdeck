# Core digest: today's board, as the code has it

Fact sheet for the drafter of `docs/sync/spec/core.md`. Every fact carries `file:line` (paths
relative to `src/` unless they start with another folder). No design, no opinions. Where
DOCUMENTATION.md says something different, the difference is noted as **DOC DIFF**.

Clock and zone notation used throughout:
- **[now]** = reads the `now: DateTime<Local>` passed in (never `Local::now()` inside `Board::apply`).
- **[zone]** = depends on the process-local time zone (`chrono::Local`): `resolve_on_day`,
  `date_naive()`, `hour()/minute()` on a `DateTime<Local>`, or serde of `DateTime<Local>`.
- **[disk]** = reads or writes the data folder.
- **[overlay]** = reads the subscribed-calendar overlay held on the board.
- **[global]** = reads a process-wide static.

---

## 1. Data model

### 1.1 `Session` — tasks.rs:13-17
```rust
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct Session { pub start: DateTime<Local>, pub minutes: u32 }
```
No serde attributes. One booked block of work on a task. `start` is an instant (serialised RFC 3339
with the local offset, chrono's default); `minutes` is its length.

### 1.2 `Recurrence` — tasks.rs:72-89
```rust
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct Recurrence {
    pub days: u8,                         // bit 0 = Monday … bit 6 = Sunday; 0 = one-off
    #[serde(default = "epoch")]
    pub anchor: NaiveDate,                // the one day a non-repeating rule falls on
    pub start_minutes: i32,               // minutes from midnight
    pub minutes: u32,                     // length
}
```
- `epoch()` = 1970-01-01 (tasks.rs:93-95); default for rules written before one-offs existed.
- `weekday_bit(day) = 1 << day.weekday().num_days_from_monday()` (tasks.rs:98-100).
- `falls_on(day)`: non-repeating → `day == anchor`; else `days & weekday_bit(day) != 0` (tasks.rs:104-109).
- `repeats()` = `days != 0` (tasks.rs:112-114); `includes(i)` = bit i set (tasks.rs:117-119).
- `toggle(index, on_day)`: flips bit; when clearing leaves zero bits, `anchor = on_day` (tasks.rs:129-139).
- `summary()`: `0`→"just this day", `EVERY_DAY`→"every day", `WEEKDAYS`(0b0001_1111)→"weekdays",
  `0b0110_0000`→"weekends", else weekday names joined by `" · "` (tasks.rs:142-156).
- `summary_dated()`: repeating → `summary()`; else `"just {anchor:%-d %b %Y}"` (tasks.rs:160-166).
- Constants: `WEEKDAY_NAMES = ["Mon","Tue","Wed","Thu","Fri","Sat","Sun"]` (tasks.rs:22),
  `EVERY_DAY = 0b0111_1111` (tasks.rs:25), private `WEEKDAYS = 0b0001_1111` (tasks.rs:43).
- **DOC DIFF**: DOCUMENTATION.md:401 lists the rule as `{ days, start_minutes, minutes }` and omits
  `anchor`.

### 1.3 `Active` — tasks.rs:169-237 (the live item; task, event or routine)
`#[derive(Debug, Serialize, Deserialize, Clone)]`, no container attributes. Fields in declaration
(= serialisation) order:

| field | type | serde | meaning |
|---|---|---|---|
| `id` | `u64` | `#[serde(default)]` | stable identity; `0` = unassigned sentinel (tasks.rs:171-176) |
| `importance` | `Option<u8>` | — | severity 0..=4 for dated tasks (index into `IMPORTANCE_LABELS`) |
| `time_importance` | `Option<u8>` | — | horizon 0..=3 for undated tasks (index into `HORIZON_LABELS`) |
| `name` | `String` | — | cosmetic, may repeat |
| `created` | `DateTime<Local>` | — | creation instant |
| `deadline` | `Option<DateTime<Local>>` | — | task: when due; event: when it happens |
| `is_event` | `bool` | — | event flag |
| `sessions` | `Vec<Session>` | `#[serde(default)]` | task's booked blocks (tasks.rs:196-197) |
| `planned_start` | `Option<DateTime<Local>>` | `#[serde(default)]` | **legacy** single slot (tasks.rs:202-203) |
| `duration_minutes` | `Option<u32>` | `#[serde(default)]` | event: block length; task: total estimate (tasks.rs:216-217) |
| `recurrence` | `Option<Recurrence>` | `#[serde(default)]` | `Some` ⇒ routine (tasks.rs:235-236) |

`importance`, `time_importance`, `name`, `created`, `deadline`, `is_event` carry no
`#[serde(default)]`. serde's derive reads a missing `Option<T>` field as `None`, so the fields a row
cannot leave out are `name`, `created` and `is_event` (not verified by running; see §10). No field
is `skip_serializing_if`, so `planned_start: null` and `recurrence: null` are always written.

Kinds (derived from flags):
- routine ⇔ `recurrence.is_some()` (`is_routine`, tasks.rs:506-508); checked before `is_event`
  everywhere it matters (placements, colour, scoring, kind name).
- event ⇔ `is_event && !routine`; task otherwise.

Derived methods (all pure):
- `planner_anchor()`: routine→None; event→`deadline`; task→`min(sessions.start)` or `deadline` (tasks.rs:521-531).
- `is_planned()`: routine→true; event→`deadline.is_some() && duration_minutes.is_some()`; task→`!sessions.is_empty()` (tasks.rs:536-546).
- `planned_minutes()` = Σ `sessions[i].minutes` (tasks.rs:549-551).
- `remaining_minutes()` = `duration_minutes.map(|e| e.saturating_sub(planned_minutes()))` (tasks.rs:555-558).
- `wants_planning()`: false for events and routines; else `sessions.is_empty() || remaining > 0` (tasks.rs:563-571).
- `calendar_item_color()`: routine→`ROUTINE_COLOR_INDEX` (0); else `calendar_item_color(is_event, importance, time_importance)` (tasks.rs:510-515).

Free function `calendar_item_color(is_event, importance, time_importance) -> usize` (tasks.rs:594-613):
event→5; else `importance` as index; else horizon (`>= HORIZON_WHENEVER` (3) → 0, else horizon);
else 0; result `.min(PALETTE_LEN - 1)` where `PALETTE_LEN = 6` (tasks.rs:576).

Valid shapes per DOCUMENTATION.md:388-393 (event: importance None, time_importance None,
deadline Some). **DOC DIFF**: `Command::Add` accepts and stores `importance`/`time_importance` on an
event (board.rs:960-987, no event check on either level); the colour ignores them.

Example record (written from the serde shape; `read_at_startup.json` is a pretty-printed array,
`serde_json::to_string_pretty`, tasks.rs:732):
```json
[
  {
    "id": 17,
    "importance": null,
    "time_importance": 1,
    "name": "report",
    "created": "2026-09-04T08:00:00+03:00",
    "deadline": null,
    "is_event": false,
    "sessions": [ { "start": "2026-09-04T09:00:00+03:00", "minutes": 105 } ],
    "planned_start": null,
    "duration_minutes": 105,
    "recurrence": null
  },
  {
    "id": 18, "importance": null, "time_importance": null, "name": "sleep",
    "created": "2026-09-04T08:00:00+03:00", "deadline": null, "is_event": false,
    "sessions": [], "planned_start": null, "duration_minutes": null,
    "recurrence": { "days": 127, "anchor": "2026-09-04", "start_minutes": 1380, "minutes": 480 }
  }
]
```
(`DateTime<Local>` values written by `Local::now()` carry fractional seconds, e.g.
`"2026-08-15T20:28:41.882008+03:00"`, archive.rs:894.)

### 1.4 `Outcome` — archive.rs:79-89
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome { #[default] Finished, Dropped }
```
Wire values `"finished"` / `"dropped"`. `glyph()`: ✓ / ✗ (archive.rs:93-98). `verb()`: "finished" /
"dropped" (archive.rs:101-106).

### 1.5 `Archived` — archive.rs:114-155 (one archive row)
`#[derive(Debug, Clone, Serialize, Deserialize)]`. Fields in serialisation order:

| field | type | serde |
|---|---|---|
| `id` | `u64` | `#[serde(default)]` (0 = pre-id row) |
| `name` | `String` | — |
| `created` | `DateTime<Local>` | — |
| `deadline` | `Option<DateTime<Local>>` | — |
| `is_event` | `bool` | — |
| `importance` | `Option<u8>` | — |
| `time_importance` | `Option<u8>` | `#[serde(default)]` |
| `sessions` | `Vec<Session>` | `#[serde(default)]` |
| `duration_minutes` | `Option<u32>` | `#[serde(default)]` |
| `recurrence` | `Option<Recurrence>` | `#[serde(default)]` |
| `archived_at` | `DateTime<Local>` | `#[serde(rename = "inactivated")]` |
| `outcome` | `Outcome` | `#[serde(default)]` → `Finished` |

There is no `planned_start` on a row. `Archived::retire(item, outcome, at)` copies every other
`Active` field (archive.rs:186-201); `to_active()` is its inverse with `planned_start: None`
(archive.rs:210-226). Key: `ArchiveKey { id: u64, archived_at: DateTime<Local> }` (archive.rs:163-167),
`key()` (archive.rs:228-230). `ArchiveKey` is not serialisable; on the wire it is the pair
`id` + `archived_at` in `Restore`/`ForgetArchived`.

Example line (`archived.jsonl`, one compact JSON object per line, `serde_json::to_string`,
archive.rs:807):
```json
{"id":17,"name":"report","created":"2026-09-04T08:00:00+03:00","deadline":null,"is_event":false,"importance":null,"time_importance":1,"sessions":[{"start":"2026-09-04T09:00:00+03:00","minutes":105}],"duration_minutes":105,"recurrence":null,"inactivated":"2026-09-04T11:00:00+03:00","outcome":"finished"}
```
Legacy line that must still load (archive.rs:894):
```json
{"id":97,"importance":2,"name":"old row","created":"2026-08-15T20:28:41.882008+03:00","deadline":"2026-08-29T15:45:00+03:00","is_event":false,"inactivated":"2026-08-15T21:35:10.023652+03:00"}
```
→ `outcome = Finished`, `sessions = []`, `time_importance = None` (archive.rs:896-903). A legacy
event row (archive.rs:953) loads with `outcome = Finished` but `was_finished()` is false.

### 1.6 Notes
`String` on the board (board.rs:550). File `notepad_text.json` holds one pretty-printed JSON string
(`serde_json::to_string_pretty(&payload)`, utilities.rs:113); missing file is seeded with `""`
(utilities.rs:153-159). Tabs are replaced by `NOTEPAD_TAB = "    "` (four spaces) on load
(board.rs:593-595) and on `SetNotes` (board.rs:1256), via `utilities::detab` (utilities.rs:146-148).
Unreadable file → notes become the literal
`"There was something wrong with taskdeck_data/notepad_text.json!"` (board.rs:595), no problem
string is reported, file not moved.

Example file content: `"milk\n    eggs"`.

### 1.7 `Subscription` — subscriptions.rs:137-147
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription { pub id: u64, pub name: String, pub url: String, pub color: [u8; 4], pub enabled: bool }
```
No serde attributes (all fields required). `color` is RGBA, independent of the colour scheme.
File `subscriptions.json` (`SUBSCRIPTIONS_FILE`, subscriptions.rs:53), pretty-printed JSON array
(subscriptions.rs:505-507); missing or whitespace-only file = empty list (subscriptions.rs:490-500).
Example:
```json
[ { "id": 3, "name": "Work", "url": "https://cal.example.com/w.ics", "color": [90, 140, 200, 255], "enabled": true } ]
```
Subscription ids come from the **same** `next_id` counter as items (board.rs:1332).

### 1.8 Overlay (derived, not authored) — subscriptions.rs:184-484
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayEvent {           // subscriptions.rs:190-210
    pub subscription: u64, pub day: NaiveDate, pub start: i32, pub end: i32,
    #[serde(default)] pub all_day: bool, #[serde(default)] pub free: bool,
    pub summary: String,
    #[serde(default)] pub location: String, #[serde(default)] pub description: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FetchOutcome {             // subscriptions.rs:261-277
    Ok { events: usize, problems: Vec<String>, #[serde(default)] title: Option<String> },
    Failed { why: String },
}
pub struct FetchStatus { pub subscription: u64, pub at: DateTime<Local>, pub outcome: FetchOutcome } // :336-341
pub struct Covered { pub from: NaiveDate, pub until: NaiveDate }   // :116-120, both inclusive, Copy
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overlay {                // subscriptions.rs:347-368
    #[serde(default)] pub events: Vec<OverlayEvent>,
    #[serde(default)] pub status: Vec<FetchStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub covers: Option<Covered>,
}
```
Cache file `subscribed_cache.json` (`OVERLAY_FILE`, subscriptions.rs:56), compact JSON
(subscriptions.rs:523-525). Read: any failure → `None` → `Overlay::default()` (board.rs:613,
subscriptions.rs:511-518); re-sealed on read with `covers` re-attached. Details in §7.

### 1.9 Board in memory — board.rs:540-564
```rust
pub struct Board {
    pub items: Vec<Active>, pub archive: ArchiveLog, pub notes: String,
    subscriptions: Vec<Subscription>, overlay: Overlay,
    next_id: u64, data_dir: PathBuf, version: u64,
}
```
- `version`: starts 0 (`from_parts`, board.rs:655); moves `wrapping_add(1)` in `save_items`
  (board.rs:925), `save_subscriptions` (board.rs:931), `save_notes` (board.rs:937), `ForgetArchived`
  (board.rs:1313), `replace` (board.rs:773), `adopt_overlay` when events differ (board.rs:814), and
  `touch()` (board.rs:678-680, called only from ui.rs:3343). `seed_version(at_least)` = max
  (board.rs:685-687); hosts seed it with `Utc::now().timestamp_millis()` (main.rs:305-307,
  main.rs:91, server_main.rs:262). The version is bumped **before** the write, so a failed save
  still moves it.
- **DOC DIFF**: DOCUMENTATION.md:2437 says the version is bumped "by the board on every save it
  makes (`Board::touch`, the one funnel)". The code bumps in the three `save_*` helpers and the
  places listed above; `touch` is not the funnel.
- `next_id`: seeded by `assign_missing_ids` (board.rs:645); `number_from(first)` = max (board.rs:671-673);
  `next_item_id()` returns and increments with `saturating_add` (board.rs:826-830).
- `BoardState` (board.rs:473-486), the wire form of the whole board:
  `{ version: u64, items: Vec<Active>, archive: Vec<Archived>, notes: String,
  #[serde(default)] subscriptions: Vec<Subscription>, #[serde(default)] overlay: Overlay }`.
  **DOC DIFF**: DOCUMENTATION.md:2345 describes `/api/board` as "items, archive, notes, version";
  it also carries `subscriptions` and `overlay`.

### 1.10 Files in the data folder
`taskdeck_data/` = `DATA_DIR_NAME` (paths.rs:24), `userconfig.toml` inside it (paths.rs:178).
`Board::unreadable_files` checks: `read_at_startup.json`, `archived.jsonl`, `notepad_text.json`,
`colorschemes.json`, `userconfig.toml`, `subscriptions.json` (board.rs:624-640).

| file | writer | format | atomic |
|---|---|---|---|
| `read_at_startup.json` | `tasks::oversafe_activesave` (tasks.rs:725-751) | pretty JSON array of `Active` | temp file + fsync + persist + dir fsync |
| `archived.jsonl` | `ArchiveLog::record`/`take`/`persist` (archive.rs:733-817) | JSONL of `Archived` | append+fsync; whole-file rewrite atomic |
| `notepad_text.json` | `utilities::save_notepad_text` (utilities.rs:106-132) | pretty JSON string | atomic |
| `subscriptions.json` | `subscriptions::save` (subscriptions.rs:505-535) | pretty JSON array | atomic |
| `subscribed_cache.json` | `subscriptions::save_overlay` | compact JSON `Overlay` | atomic; failure ignored (board.rs:734-736) |
| `colorschemes.json` | `color::save_colorschemes` (color.rs:241-269) | pretty JSON object `{ "<u32>": ColorScheme }` | atomic |
| `userconfig.toml` | `initialization::write_normalized_config` (initialization.rs:405-433) | TOML | atomic (initialization.rs:368) |
| `outbox.json` | `sync::Outbox` (client only; DOCUMENTATION.md:446) | JSON array of `Queued { command, local_id, key }` | atomic |

`read_at_startup.json` missing → created as `[]` (tasks.rs:666-671). `colorschemes.json` missing →
created as `{}` (color.rs:274-279).

### 1.11 Open, quarantine, and load-time migrations
`Board::open(data_dir) -> (Board, Vec<String>)` (board.rs:570-619) [disk]:
1. `tasks::read_at_startup` (tasks.rs:663-679). On `PermissionDenied` (anywhere in the error chain
   via `downcast_ref::<std::io::Error>`, board.rs:439-443): problem string, empty items, file left.
   Any other error: `tasks::quarantine_corrupt_file(dir, "read_at_startup.json", err)` renames it to
   `read_at_startup.json.corrupt-<%Y%m%d-%H%M%S>` using `Local::now()` (tasks.rs:705-723) [zone][clock],
   items empty.
2. Notes (above). 3. `subscriptions::read`; error → problem string, empty list, file not moved
   (board.rs:601-610). 4. Overlay cache, errors swallowed.
5. `Board::from_parts(items, notes, dir)` (board.rs:644-657): `assign_missing_ids` then
   `migrate_legacy_plans`; archive **not** loaded (lazy, `ArchiveLog::new()`).

`assign_missing_ids(&mut [Active]) -> u64` (tasks.rs:688-699): `next = max(id) + 1` (saturating;
1 when empty); walking in order, an item with `id == 0` or an id already taken by an earlier item
gets `next` (then `next += 1`); returns the next free id. Tests: tasks.rs:1434-1481.

`migrate_legacy_plans(&mut [Active])` (tasks.rs:618-632): for each item, `planned_start.take()`;
if it was `Some` and the item is not an event and has no sessions, push
`Session { start, minutes: duration_minutes.unwrap_or(DEFAULT_BLOCK_MINUTES (30)).max(MIN_BLOCK_MINUTES (15)) }`.
`duration_minutes` is left as is (it stays the estimate). Events lose `planned_start` silently.
Idempotent. Recurrence is not checked (a routine with a stray `planned_start` would gain a session).
Legacy file example that must load (board.rs:2168-2173): pre-id, pre-sessions:
`[{"importance":2,"time_importance":null,"name":"old","created":"2026-09-01T09:00:00+03:00","deadline":"2026-09-05T17:00:00+03:00","is_event":false,"planned_start":"2026-09-03T10:00:00+03:00","duration_minutes":90}]`
→ id backfilled, one 90-minute session, `planned_start` None, estimate 90 (board.rs:2175-2182).

`Board::adopt_temporaries()` (board.rs:697-711): `next_id = (max id < TEMPORARY_ID_FLOOR, or 0) + 1`;
every item with `id >= TEMPORARY_ID_FLOOR` (`1 << 62`, board.rs:58) renumbered in list order;
saves (and bumps version) only if any renumbered; returns the count. Called at startup by
main.rs:94 and server_main.rs:265. Note it **lowers** `next_id` to just past the highest real
*live* id, ignoring ids held by archive rows.

`Board::replace(items, archived: Option<Vec<Archived>>, notes, subscriptions, overlay)`
(board.rs:757-774): takes the server's picture; `next_id = max(next_id, max(item.id + 1), 1)`;
archive replaced only if `Some`; version +1. `renumber(local, server)` renames one item
(board.rs:741-749). `adopt_from_server(subs, overlay)` sets both without a version move
(board.rs:781-784). `save_all` writes items, subscriptions, notes and (if loaded) the whole archive
(board.rs:716-722).

---

## 2. Commands

### 2.1 Envelope
`#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] #[serde(tag = "op", rename_all = "snake_case")] pub enum Command`
(board.rs:111-280). Internally tagged; wire `op` is the snake_case variant name. Unknown `op` is a
parse error (board.rs:1534). `Kind` = `#[serde(rename_all = "lowercase")]` `task|event|routine`
(board.rs:78-84).

Instant fields marked *wire-instant* use `#[serde(default, deserialize_with = "wire::optional_local")]`
(board.rs:392-405): absent, `null`, or blank/whitespace string → `None`; otherwise
`parse_local_input` (board.rs:409-419): RFC 3339 → `with_timezone(&Local)`; else naive
`%Y-%m-%dT%H:%M` or `%Y-%m-%dT%H:%M:%S` → `planner::resolve_on_day(date, h*60+m)` [zone] (seconds
dropped), then `None` if the date's year is outside 1970..=9999; failure → serde error
`` not a time: `{text}` ``. Serialisation of these fields is chrono's default RFC 3339 (round-trips,
board.rs:1531-1532). `NaiveDate` fields are `YYYY-MM-DD`. `Restore`/`ForgetArchived.archived_at` is a
plain `DateTime<Local>` (RFC 3339 only; naive form not accepted).

### 2.2 Variants

| op | fields (type, serde) | kind | `Reply` | errors |
|---|---|---|---|---|
| `snapshot` | `from: NaiveDate`, `days: u32` `#[serde(default = "one")]` (=1, board.rs:282-284) | query | — (`apply` → 400) | 400 |
| `feed` | — | query | — | 400 |
| `board` | — | query | — | 400 |
| `add` | `name: String`; `deadline` wire-instant; `importance: Option<u8>` `default`; `is_event: bool` `default`; `time_importance: Option<u8>` `default` | mutation | `{id}` | 400, 500 |
| `quick_add` | `name: String`; `deadline` wire-instant | mutation | `{id}` | 400, 500 |
| `rename` | `id: u64`, `name: String` | mutation | `{}` | 400, 410, 500 |
| `complete` | `id: u64`; `at` wire-instant | mutation | `{}` | 400, 410, 500 |
| `delete` | `id: u64`; `at` wire-instant | mutation | `{}` | 400, 410, 500 |
| `forget` | `id: u64` | mutation | `{}` | 410, 500 |
| `move_block` | `id: u64`; `session: Option<usize>` `default`; `day: NaiveDate`; `start: i32`; `minutes: u32` | mutation | `{}` | 400, 410, 500 |
| `add_block` | `id: u64`; `day: NaiveDate`; `start: i32`; `minutes: Option<u32>` `default` | mutation | `{session}` | 400, 410, 500 |
| `remove_block` | `id: u64`, `session: usize` | mutation | `{}` | 410, 500 |
| `unplan` | `id: u64` | mutation | `{}` | 400, 410, 500 |
| `set_estimate` | `id: u64`, `minutes: u32` | mutation | `{}` | 400, 410, 500 |
| `set_deadline` | `id: u64`; `deadline` wire-instant (`null` clears) | mutation | `{}` | 400, 410, 500 |
| `set_severity` | `id: u64`, `level: u8` | mutation | `{}` | 400, 410, 500 |
| `set_horizon` | `id: u64`, `level: u8` | mutation | `{}` | 400, 410, 500 |
| `set_repeat` | `id: u64`, `days: u8`, `day: NaiveDate` | mutation | `{}` | 400, 410, 500 |
| `create` | `kind: Kind`, `name: String`, `day: NaiveDate`, `start: i32`, `minutes: u32` | mutation | `{id}` | 400, 500 |
| `reflow` | `day: NaiveDate`; `from: Option<i32>` `default` | mutation | `{moved}` | 400, 500 |
| `set_notes` | `text: String` | mutation | `{}` | 400, 500 |
| `restore` | `id: u64`, `archived_at: DateTime<Local>` | mutation | `{id}` | 410, 500 |
| `forget_archived` | `id: u64`, `archived_at: DateTime<Local>` | mutation | `{}` | 410, 500 |
| `add_subscription` | `name: String`, `url: String`, `color: Option<[u8;4]>` `default` | mutation | `{id}` | 400, 500 |
| `remove_subscription` | `id: u64` | mutation | `{}` | 410, 500 |
| `rename_subscription` | `id: u64`, `name: String` | mutation | `{}` | 400, 410, 500 |
| `set_subscription_color` | `id: u64`, `color: [u8;4]` | mutation | `{}` | 410, 500 |
| `set_subscription_enabled` | `id: u64`, `enabled: bool` | mutation | `{}` | 410, 500 |

Helpers on `Command`:
- `is_query()` = Snapshot | Feed | Board (board.rs:289-291).
- `touches_archive()` = Complete | Delete | Restore | ForgetArchived (board.rs:295-303).
- `touches_calendars()` = the five subscription ops (board.rs:307-316).
- `item_id()` / `set_item_id()` cover Rename, Complete, Delete, Forget, MoveBlock, AddBlock,
  RemoveBlock, Unplan, SetEstimate, SetDeadline, SetSeverity, SetHorizon, SetRepeat, Restore,
  ForgetArchived — **not** the subscription ops (board.rs:322-363).
- `stamped(now)`: `Complete{at:None}`→`at: Some(now)`; `Delete{at:None}`→same;
  `Reflow{from:None}`→`from: planner::now_marker(day, now)` (None if `day` is not `now`'s date);
  others unchanged (board.rs:371-378) [now][zone].
- `creates_item()` = Add | QuickAdd | Create | Restore (board.rs:382-387). (`AddSubscription` also
  returns an id but is not listed.)

### 2.3 Replies and errors
- `Reply { #[serde(default, skip_serializing_if = "Option::is_none")] id: Option<u64>, session: Option<usize>, moved: Option<usize> }`
  (same attributes on all three; board.rs:524-535).
- HTTP form: `phone::reply_json` → `{"ok": true, "version": <u64>, "id"?, "session"?, "moved"?}`
  (phone.rs:172-184). Error body `{"error": "<message>"}` with the status (phone.rs:1372-1376).
- `BoardError { status: u16, message: String }` (Serialize/Deserialize; board.rs:491-495):
  `bad_request` 400, `gone` 410, `failed` 500; `is_rejection()` = 400..500 (board.rs:497-515).
  `phone::PhoneError` is a re-export of it (phone.rs:78).

### 2.4 Limits and constants (everything that bounds a command)

| constant | value | where |
|---|---|---|
| `NAME_MAX_CHARS` | 200 (chars, after trim) | board.rs:53 |
| `NOTES_MAX_CHARS` | 20_000 (chars, counted **before** detab) | board.rs:48, 1253 |
| `NEW_TASK_IMPORTANCE` | 2 | board.rs:40 |
| `NEW_TASK_HORIZON` | 1 | board.rs:44 |
| `TEMPORARY_ID_FLOOR` | `1 << 62` | board.rs:58 |
| `FIRST_YEAR`..=`LAST_YEAR` | 1970..=9999 (private) | board.rs:64-65 |
| `DEFAULT_MORNING_HOUR` | 7 (private) | board.rs:68 |
| archived_at future cap | `now + 1 day` | board.rs:466-469 |
| `IMPORTANCE_LABELS` | `["Not important","Mildly important","Important","Highly important","Lethally important"]` (len 5) | tasks.rs:29-30 |
| `HORIZON_LABELS` | `["Within a month","Within a week","Within days","Whenever"]` (len 4) | tasks.rs:37 |
| `HORIZON_DISPLAY_ORDER` | `[2, 1, 0, 3]` | tasks.rs:40 |
| `HORIZON_WHENEVER` | 3 | tasks.rs:314 |
| `ROUTINE_COLOR_INDEX` | 0 | tasks.rs:50 |
| `PALETTE_LEN` | 6 | tasks.rs:576 |
| `DAY_MINUTES` | 1440 | planner.rs:23 |
| `SNAP_MINUTES` | 15 | planner.rs:28 |
| `MIN_BLOCK_MINUTES` | 15 | planner.rs:32 |
| `DEFAULT_BLOCK_MINUTES` | 30 | planner.rs:36 |
| `DURATION_CHOICES` | `[15,30,45,60,90,120,150,180,240,300,360,420,480,600]` (private, UI picker) | planner.rs:563-565 |
| `SUBSCRIPTIONS_MAX` | 20 | subscriptions.rs:72 |
| subscription url max | 2000 chars | subscriptions.rs:161 |
| `DEFAULT_COLORS` | six RGBA, see §8 | subscriptions.rs:60-67 |
| `SUMMARY_MAX_CHARS` / `LOCATION_MAX_CHARS` / `DESCRIPTION_MAX_CHARS` / `WHY_MAX_CHARS` | 200 / 120 / 300 / 200 | subscriptions.rs:75-88 |
| `EVENTS_MAX` | 5_000 (whole overlay) | subscriptions.rs:79 |
| `LONG_EVENT_MINUTES` | 720 | subscriptions.rs:233 |
| `WINDOW_BACK_DAYS` / `WINDOW_FORWARD_DAYS` | 60 / 400 | subscriptions.rs:102-103 |
| `MAX_SNAPSHOT_DAYS` | 31 | phone.rs:116 |
| `DONE_ROWS_MAX` | 40 | phone.rs:167 |
| `MAX_BODY_BYTES` | `NOTES_MAX_CHARS * 4 + 4 * 1024` | phone.rs:136 |
| `ON_THE_NOSE_MINUTES` | 30 (private) | archive.rs:68 |
| scoring tables | see §4.3 | tasks.rs:268-349 |
| ICS `Limits::default()` | see §7.1 | ics.rs:126-137 |

### 2.5 `Board::apply(command, now) -> Result<Reply, BoardError>` arm by arm (board.rs:951-1399)
Shared: `gone()` = 410 "That item is no longer on the board." (board.rs:952); `bad_time()` = 400
"That time doesn't exist on this day (daylight saving)." (board.rs:953). `checked_name(name)`
trims and refuses `> NAME_MAX_CHARS` chars with 400 "A name is at most 200 characters."
(board.rs:429-435). `sane_day(day)` refuses a year outside 1970..=9999 with 400
"That date is out of range (1970–9999)." (board.rs:446-452); `sane_instant` applies it to
`at.date_naive()` [zone] (board.rs:455-460). Every successful mutating arm ends in one of the
`save_*` helpers [disk], which bump `version` first and map a write error to 500 (board.rs:924-940);
the in-memory change is **kept** when the save fails (board.rs:946-949), except where noted.

- **Snapshot | Feed | Board** → 400 "That is a query, not a command." (board.rs:956-958).
- **Add** (board.rs:960-988): `checked_name`; empty → 400 "It needs a name."; `is_event && deadline.is_none()`
  → 400 "An event needs a time."; `sane_instant(deadline)`; if `!is_event && deadline None &&
  importance None && time_importance None` → `time_importance = Some(NEW_TASK_HORIZON)`;
  `importance >= 5` → 400 "No such severity."; `time_importance >= 4` → 400 "No such horizon.";
  `push_new` (board.rs:1403-1427): new id, `created: now` [now], sessions empty,
  `planned_start None`, `duration_minutes None`, `recurrence None`. Order note: the name is checked
  before the event-time rule, which is before the date range, which is before the level checks.
- **QuickAdd** (board.rs:990-1005): `checked_name`; empty → 400 "A task needs a name.";
  `sane_instant`; dated → `push_new(importance Some(2), time_importance None)`; undated →
  `push_new(importance None, time_importance Some(1))`. Deadline time is not snapped.
- **Rename** (board.rs:1007-1016): `checked_name`; empty → 400 "A name cannot be empty."; missing → 410.
  Works on any kind.
- **Complete** (board.rs:1018-1028): missing → 410; event or routine → 400 "Only a task can be
  finished — an event or a routine is deleted instead."; `at = archived_at(at, now)` =
  `sane_instant(at)?` then `min(at, now + 1 day)`, or `now` if absent [now] (board.rs:466-469);
  `retire(id, Finished, at)`.
- **Delete** (board.rs:1030-1035): missing → 410; any kind; `retire(id, Dropped, at)`.
- **retire** (board.rs:1475-1487): `Archived::retire(clone, outcome, at)`; `archive.record` (append
  one line [disk]) **first** — failure → 500 "Could not write to the archive, so nothing was
  changed:\n…" and the item stays; then remove the item and `save_items`.
- **Forget** (board.rs:1037-1042): missing → 410 (no version move, board.rs:1958-1964); removes the
  item with no archive row; saves.
- **MoveBlock** (board.rs:1044-1075): `sane_day(day)`; missing → 410; for a task, `session` must be
  `Some(i)` with `i < sessions.len()` else 410 "That block is no longer there." (events and routines
  ignore `session`); `(start, minutes) = planner::clamp_block(start as f32, minutes as f32)`;
  `when = resolve_on_day(day, start)` [zone] or 400 `bad_time` (also for routines).
  Routine: `rule.start_minutes = start.clamp(0, 1439)`, `rule.minutes = minutes` (day ignored, rule
  edited for all days). Event: `deadline = Some(when)`, `duration_minutes = Some(minutes)`. Task:
  `sessions[i] = Session { start: when, minutes }`.
- **AddBlock** (board.rs:1077-1092): `sane_day`; missing → 410; event/routine → 400 "Only a task can
  be given a block of time."; `length = minutes.unwrap_or(planner::drop_length_for(item))`;
  `clamp_block`; `resolve_on_day` or 400; push `Session { when, length.max(15) }`;
  reply `session = Some(new index)` (always the last index).
- **RemoveBlock** (board.rs:1094-1102): missing → 410; event/routine or `session >= len` → 410
  "That block is no longer there."; `sessions.remove(session)` (later indices shift down).
- **Unplan** (board.rs:1104-1114): missing → 410; event/routine → 400 "Only a task can be
  unplanned."; `sessions.clear()`; estimate kept.
- **SetEstimate** (board.rs:1116-1146): `minutes` outside `15..=1440` → 400 "A length is between
  fifteen minutes and a day." (checked before the item lookup); missing → 410. Routine:
  `(start, length) = clamp_block(rule.start_minutes, minutes)` → both written into the rule (start
  may be pulled back, length snapped). Task: `duration_minutes = Some(minutes.max(15))` — **not
  snapped** (e.g. 100 stays 100). Event: same, then if it has a deadline:
  `start = deadline.hour()*60 + deadline.minute()` [zone], `clamp_block(start, minutes)`,
  `duration_minutes = Some(length)` (snapped), `deadline = resolve_on_day(deadline.date_naive(), start)`
  if that resolves (else unchanged) [zone].
- **SetDeadline** (board.rs:1148-1162): `sane_instant`; missing → 410; event/routine → 400 "Only a
  task has a due date; an event's time is its time."; `deadline = deadline`; if `Some` and
  `importance.is_none()` → `importance = Some(NEW_TASK_IMPORTANCE)`. `time_importance` untouched;
  clearing keeps `importance` (board.rs:1836-1838).
- **SetSeverity** (board.rs:1164-1175): `level >= 5` → 400 "No such severity." (before lookup);
  missing → 410; event/routine → 400 "Only a task has a severity."; `importance = Some(level)`.
- **SetHorizon** (board.rs:1177-1188): `level >= 4` → 400 "No such horizon."; missing → 410;
  event/routine → 400 "Only a task has a horizon."; `time_importance = Some(level)`.
- **SetRepeat** (board.rs:1190-1204): `sane_day(day)`; missing → 410; not a routine → 400 "Only a
  routine repeats."; `rule.days = days & EVERY_DAY`; if now non-repeating, `rule.anchor = day`
  (a repeating rule keeps its old anchor).
- **Create** (board.rs:1206-1224): `sane_day`; `clamp_block(start, minutes)`; `resolve_on_day` or
  400; `checked_name`; empty → "New task" / "New event" / "New routine"; `push_created`
  (board.rs:1435-1467):
  - task: `sessions = [Session { when, minutes }]`, `deadline None`, `duration_minutes Some(minutes)`,
    `importance None`, `time_importance Some(1)`, `recurrence None`;
  - event: `deadline Some(when)`, `sessions []`, `duration_minutes Some(minutes)`, both levels None;
  - routine: `recurrence Some({ days: 0, anchor: day, start_minutes: start, minutes })`,
    `duration_minutes None`, `deadline None`, levels None;
  - all: new id, `created: now` [now].
- **Reflow** (board.rs:1226-1250): `sane_day`; `from` given → `clamp(0, 1440)`; else
  `planner::now_marker(day, now)` [now][zone]; neither → `Ok(moved: Some(0))` without saving.
  `(blocks, anchors) = reflow_inputs(day)` [overlay][zone]; for each `((id, index), start)` from
  `planner::reflow`, `resolve_on_day(day, start)` (skip on None) and assign if different; saves
  (and bumps version) **only if `moved > 0`**; reply `moved: Some(n)`.
- **SetNotes** (board.rs:1252-1259): `text.chars().count() > 20_000` → 400 "Those notes are too long
  for the notepad."; `notes = detab(text)`; `save_notes`.
- **Restore** (board.rs:1261-1304): `load_archive()` [disk] (500 on failure); `archive.take(dir, key)`
  rewrites the file [disk] (500 "Could not update the archive:\n…"); not found → 410 "That is no
  longer in the archive."; `item = record.to_active()`; id unusable if `0`, `>= TEMPORARY_ID_FLOOR`,
  or held by a live item → `next_item_id()`; else keep it and `next_id = max(next_id, id + 1)`;
  push; on `save_items` failure re-`record` the row and pop the item (or, if re-recording fails,
  keep it in memory and return 500 with an extra sentence); reply `id = Some(restored id)`.
- **ForgetArchived** (board.rs:1306-1315): `load_archive` [disk]; `take` [disk]; not found → 410;
  `version += 1`; no item save.
- **AddSubscription** (board.rs:1320-1344): `len >= 20` → 400 "That is already 20 calendars, which
  is as many as this keeps."; `checked_url(url)` (subscriptions.rs:156-180: trim; empty → "An
  address is needed."; > 2000 chars → "That address is too long."; `webcal://` → `https://`;
  `http://` → "Use https — a calendar link is a password, and http sends it in clear."; not
  `https://` → "An address should start with https://"; host (up to `/ ? #`) empty or without a
  dot → "That address has no host in it.") → 400; duplicate url (exact string after rewrite) → 400
  "That calendar is already subscribed to."; `checked_name`; `id = next_item_id()` (shared counter);
  empty name → `name_from_url(url)` (host, userinfo stripped, port stripped, `www.` stripped;
  subscriptions.rs:294-302) else `placeholder_name(id)` = `"Calendar {id}"`; `color = color or
  DEFAULT_COLORS[len % 6]`; push `{ enabled: true }`; `save_subscriptions`; reply `id`.
- **RemoveSubscription** (board.rs:1346-1358): not present → 410 "That calendar is no longer
  subscribed to."; `overlay.forget(id)` (events and status of it dropped); save.
- **RenameSubscription** (board.rs:1360-1372): `checked_name`; missing → 410; **empty name leaves the
  name unchanged but still saves and bumps the version** and returns Ok.
- **SetSubscriptionColor** (board.rs:1374-1383): missing → 410; sets color (any alpha).
- **SetSubscriptionEnabled** (board.rs:1385-1397): missing → 410; sets flag; `false` →
  `overlay.forget(id)`.

Id assignment summary: `next_item_id()` for Add, QuickAdd, Create, AddSubscription, Restore (when
unusable), `adopt_temporaries`. Ids are never reused by the counter; `replace` and Restore only push
it up.

### 2.6 Reads of clock, zone, overlay, disk inside `apply`
- `now` [now]: `created` (push_new/push_created), Complete/Delete default and cap, Reflow default.
- Zone [zone]: `wire::optional_local` naive parsing (at deserialisation, before `apply`);
  `resolve_on_day` in MoveBlock, AddBlock, Create, Reflow, SetEstimate (event); `deadline.hour()`,
  `.minute()`, `.date_naive()` in SetEstimate; `sane_instant` uses `date_naive()`; `now_marker`
  compares `now.date_naive()`; `DateTime<Local>` deserialisation of `archived_at` (converted into the
  local offset).
- Overlay [overlay]: Reflow (`reflow_inputs` → `overlay.anchors_on(day)`, board.rs:906); also
  `behind_minutes` (does **not** use the overlay: only the blocks half of `reflow_inputs`).
- Disk [disk]: every save; Complete/Delete append to the archive; Restore/ForgetArchived load and
  rewrite the archive. `apply` does not read any other file.

---

## 3. What the clients send

### 3.1 The phone page's 17 ops (src/phone.html)
All go through `send()` → `POST /api/command` with header `X-TaskDeck-Request: <key>`
(phone.html:1500), body = the JSON below, parsed as `Command` (phone.rs:890-892); a query op there
→ 400. Each maps 1:1 to the `Command` variant of the same `op`.

| op | sent at | body as sent |
|---|---|---|
| `move_block` | phone.html:4365, 4939, 5069, 5080, 5159 | `{id, session, day, start, minutes}`; 5159 (event "when" input) omits `session` |
| `set_notes` | 4727 | `{text}` |
| `complete` | 4881 | `{id, at}`; `at = new Date().toISOString()` with ms stripped, e.g. `"2026-09-04T09:00:00Z"` (phone.html:4825) |
| `restore` | 4885, 4899, 5257 | `{id, archived_at}`; Undo reuses the `at` it sent; the Done list sends `row.archived_at` from the snapshot |
| `delete` | 4895 | `{id, at}` (same stamp) |
| `add_block` | 4908, 5137 | `{id, day, start, minutes}` (minutes given) |
| `remove_block` | 4923, 5083 | `{id, session}` |
| `rename` | 5006 | `{id, name}` |
| `set_deadline` | 5096 | `{id, deadline}`; `datetime-local` string or `null` |
| `set_severity` | 5116 | `{id, level}` |
| `set_horizon` | 5121 | `{id, level}` |
| `set_estimate` | 5125, 5163 | `{id, minutes}` |
| `unplan` | 5144 | `{id}` |
| `set_repeat` | 5178, 5181, 5182 | `{id, days, day}`; toggle `base ^ (1<<i)`, "All" = 127, "Once" = 0 |
| `quick_add` | 5303 | `{name, deadline}`; deadline `null` or `datetime-local` (default `${day}T17:00`) |
| `create` | 5361 | `{kind, name (trimmed), day, start, minutes}` |
| `reflow` | 5457 | `{day}` — **no `from`**, so the board's own clock decides |

Queries the page makes by GET: `/api/state?from=…&days=…` → `Command::Snapshot`
(phone.html:1694, 2529; phone.rs:865-871, 964-981), `/api/wait?version=N` (long poll, not a Command,
phone.rs:732-746), `/api/weather` (not a Command, phone.rs:857-864). Non-command POSTs:
`/api/look`, `/api/background` (phone.rs:821-851). `snapshot_command`: `from` absent →
`Local::now().date_naive()` [clock][zone] (phone.rs:972); `from` must parse `%Y-%m-%d` with year in
1970..=9999 else 400; `days` absent → 1, non-u32 → 400.

### 3.2 What the desktop sends that the page does not (ui.rs)
`Command::` uses in ui.rs: Add (ui.rs:2862), Forget (2879), RenameSubscription (2764, 3649),
MoveBlock (3228), Reflow (3244, with `from = planner::now_marker(day, Local::now())`, ui.rs:3243),
AddBlock (3256 with `Some(minutes)`, 4850 with `None`), RemoveBlock (3267), SetDeadline (3284),
Unplan (3292), Create (3309, always `name: ""`), SetSubscriptionEnabled (3625),
SetSubscriptionColor (3633), RemoveSubscription (3652), AddSubscription (3672, `color: None`),
QuickAdd (3881, `deadline: None`), Rename (3921), SetRepeat (4747), SetSeverity (4750),
SetHorizon (4753), SetEstimate (4835), Complete/Delete (6040-6041, `at: Some(Local::now())`),
Restore (6111), ForgetArchived (6123), SetNotes (6878).
Not sent by the page: **`add`, `forget`, `forget_archived`, `add_subscription`,
`remove_subscription`, `rename_subscription`, `set_subscription_color`, `set_subscription_enabled`**.
Neither UI sends `feed`/`board` through `/api/command`; they are GET `/calendar.ics` and
`/api/board`. The server also applies `RenameSubscription` itself after a fetch, from
`subscriptions::better_name` (server_main.rs:482-491; desktop ui.rs:2761).

---

## 4. Read side

### 4.1 Process globals and parameters the read side takes today
- `phone::answer_query(board: &mut Board, palette: [[u8;4];6], ranked: Option<Vec<u64>>, command: &Command, now: DateTime<Local>) -> PhoneReply`
  (phone.rs:193-214): Snapshot → `serde_json::to_value(snapshot(..))`; Feed →
  `Value::String(calendar_feed(&board.items, now))`; Board → `board.state()` (500 "The archive could
  not be read, so the board was not served:\n…" on failure); other → 400 "That is a command, not a query."
- Callers: desktop `execute_phone_command` passes `palette = self.phone_palette()` (selected scheme's
  colours or `[[0;4];6]`, ui.rs:3482-3487), `ranked = Some(self.list_tasks ids)` and `Local::now()`
  (ui.rs:3493-3500); server passes the palette read once at startup from `colorschemes.json` +
  `selected_colorscheme_id` (server_main.rs:276-279), `ranked = None`, `now = Local::now()`
  (server_main.rs:450-451).
- Statics read by `snapshot`: `backdrop()` (`static BACKDROP`, phone.rs:1675, 1712) and
  `look_dials()` (`static LOOK`, phone.rs:1693-1700) [global]. `Labels::current()` is constants.
- Desktop `list_tasks` order: `summarize_calendar` sorts non-events by `importance_score(self.date,
  shuffle_seed)` (ui.rs:2932-2939) with `shuffle_seed = self.date.date_naive().num_days_from_ce()`
  (ui.rs:2907-2909), then `refilter_tasks` drops events and routines (ui.rs:1879-1889). `self.date`
  is the UI's clock field, not `Local::now()` at query time.

### 4.2 `Board::ranked_ids(now) -> Vec<u64>` (board.rs:860-870)
Filter `!is_event && !is_routine()`; score `importance_score(now, Board::shuffle_seed(now))`; sort
descending by `partial_cmp`, `unwrap_or(Equal)` (stable sort, ties keep `items` order); ids.
`Board::shuffle_seed(now) = now.date_naive().num_days_from_ce() as u64` (board.rs:854-856) [zone].

### 4.3 `Active::importance_score(time_now, shuffle_seed) -> f32` (tasks.rs:387-389)
`= base_score(now) * tie_break_jitter(seed)`. All arithmetic is `f32`.

Tables (tasks.rs:268-349):
- `WEIGHT_BY_IMPORTANCE = [1.0, 2.0, 4.0, 8.0, 16.0]`
- `LEAD_DAYS_BY_IMPORTANCE = [0.5, 1.0, 2.0, 4.0, 8.0]`
- `WEIGHT_BY_HORIZON = [1.0, 2.0, 4.0, 0.5]`
- `RIPEN_DAYS_BY_HORIZON = [30.0, 10.0, 3.0, 90.0]`
- `PLANNED_LEAD_DAYS = 0.5`, `OVERDUE_DOUBLING_DAYS = 1.0`, `OVERDUE_PRESSURE_CAP = 4.0`,
  `ASSUMED_IMPORTANCE = 2`, `MALFORMED_SCORE = 1.0e6`, `JITTER = 0.08`.
- `weight_for(table, level) = table[min(level, len-1)]` (tasks.rs:376-378).
- `duration_in_days(d) = d.num_minutes() as f32 / 1440.0` (tasks.rs:638-640) — whole minutes.
- `deadline_pressure(days_left, lead)`: `lead = max(lead, 0.01)`; `days_left >= 0` →
  `exp2(-days_left / lead)`; else `exp2(-days_left / 1.0).min(4.0)` (tasks.rs:356-367).
- `ripeness(age, ripen) = 1 - exp2(-max(age,0) / max(ripen,0.01))` (tasks.rs:371-374).

`base_score` (tasks.rs:393-463):
1. routine → 0.0.
2. `planned_pressure` = max over sessions of `deadline_pressure(days(session.start - now), 0.5).min(1.0)`, or None.
3. deadline Some: `imp = importance.unwrap_or(2)`; `pressure = max(deadline_pressure(days(deadline - now), lead[imp]), planned or 0)`; return `weight[imp] * pressure`. (Applies to events too: an event with a deadline is scored this way with importance None→2; `ranked_ids` filters events out, `build_tray` never sees them.)
4. no deadline: weight = `importance` → WEIGHT_BY_IMPORTANCE; else `time_importance` → WEIGHT_BY_HORIZON; else if sessions non-empty → WEIGHT_BY_IMPORTANCE[2]; else return `MALFORMED_SCORE`.
5. planned_pressure Some → `weight * planned`.
6. else `weight * ripeness(days(now - created), RIPEN[horizon] or RIPEN[1] (10.0) when only importance)`.

`tie_break_jitter(seed)` (tasks.rs:487-503), splitmix64-style:
```
mixed = id.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ seed.wrapping_mul(0xBF58_476D_1CE4_E5B9)
mixed ^= mixed >> 30; mixed = mixed.wrapping_mul(0xBF58_476D_1CE4_E5B9)
mixed ^= mixed >> 27; mixed = mixed.wrapping_mul(0x94D0_49BB_1331_11EB)
mixed ^= mixed >> 31
unit = (mixed >> 40) as f32 / (1u64 << 24) as f32      // [0,1)
factor = 1.0 + unit * 0.08
```

### 4.4 `Board::behind_minutes(day, now) -> i32` (board.rs:912-920)
`now_marker(day, now)` None → 0. Else the task-session blocks from `reflow_inputs(day)` (not the
anchors) → `planner::behind_minutes(blocks, now_minutes)`. `phone::build_day` computes the same
figure independently from its live entries (non-event, non-routine, `session.is_some()`,
phone.rs:2482-2491).

`Board::next_free_start(day)` (board.rs:875-884): max `placement.end()` over every item's
placements on `day` (markers count `at + 15`), or `7*60`; clamped to `0..=1440-30`. Not used by
`apply`.

### 4.5 `phone::snapshot(board, palette, ranked, from, days, now) -> Snapshot` (phone.rs:218-272)
1. `let _ = board.load_archive();` errors ignored (ghosts and done just come out empty) [disk].
2. `days = days.clamp(1, 31)`, silently.
3. For `offset in 0..days`: `build_day(&items, archive.entries(), from + offset, now)`.
4. `with_subscribed(days, subscriptions, overlay)` [overlay].
5. Fields: `version = board.version()`; `today = now.date_naive()` [zone]; `now_minutes =
   now.hour()*60 + now.minute()` [zone]; `palette`; `labels = Labels::current()`; `days`;
   `tray = build_tray(items, from, now, Board::shuffle_seed(now))`; `items = items.map(item_detail)`;
   `ranked = ranked.unwrap_or_else(|| board.ranked_ids(now))`; `done = done_rows(archive.entries())`;
   `notes`; `known = covers` only if some subscription is `enabled`, else None; `look` from
   `backdrop()` [global] (`aspect = 720.0/1560.0` from `BACKGROUND_WIDE`/`BACKGROUND_TALL`,
   phone.rs:1423-1425); `dials = Dials { blur: look_dials().blur_percent, light: .light_percent }` [global].
- **DOC DIFF**: DOCUMENTATION.md:2462 says `known` is "absent entirely when nothing is subscribed";
  the code makes it absent unless at least one subscription is *enabled* (phone.rs:257), and also
  whenever `overlay.covers` is None.

### 4.6 JSON shapes (all `#[derive(Debug, Clone, Serialize)]`, field names as written, no `rename_all`)

`Snapshot` (phone.rs:2055-2094):
| field | type | serde |
|---|---|---|
| `version` | u64 | |
| `today` | NaiveDate | |
| `now_minutes` | i32 | |
| `palette` | `[[u8;4];6]` | |
| `labels` | `Labels` | |
| `days` | `Vec<DaySnapshot>` | |
| `tray` | `Tray` | |
| `items` | `Vec<ItemDetail>` | |
| `ranked` | `Vec<u64>` | |
| `done` | `Vec<DoneRow>` | |
| `notes` | String | |
| `known` | `Option<Covered>` → `{from, until}` | `skip_serializing_if = "Option::is_none"` |
| `look` | `Option<Look>` | `skip_serializing_if = "Option::is_none"` |
| `dials` | `Dials` | |

- `Look { id: String, aspect: f32, top: String }` (phone.rs:2099-2104).
- `Dials { blur: u32, light: u32 }` (phone.rs:2108-2112).
- `Labels { severity: [&str;5], horizon: [&str;4], horizon_order: [u8;4], weekdays: [&str;7] }`
  = IMPORTANCE_LABELS, HORIZON_LABELS, HORIZON_DISPLAY_ORDER, WEEKDAY_NAMES (phone.rs:2148-2166).
- `DaySnapshot` (phone.rs:2168-2187): `date: NaiveDate`, `weekday: String` (`%A`), `label: String`
  (`%-d %B %Y`, e.g. "4 September 2026"), `is_today: bool` (`day == now.date_naive()`),
  `summary: String`, `behind_minutes: i32`, `entries: Vec<EntrySnapshot>`,
  `ghosts: Vec<GhostSnapshot>`, `subscribed: Vec<SubscribedSnapshot>`
  `#[serde(default, skip_serializing_if = "Vec::is_empty")]`.
- `EntrySnapshot` (phone.rs:2225-2240): `id: u64`, `session: Option<usize>` (null when absent),
  `name`, `kind: &'static str` (`"task"|"event"|"routine"`, phone.rs:2310-2322), `color: usize`,
  `start: i32`, `minutes: u32` (0 for markers), `marker: bool`, `due: bool`, `column: usize`,
  `columns: usize`.
- `GhostSnapshot` (phone.rs:2243-2252): `name`, `finished: bool` (`was_finished()`),
  `color: usize` (`color_id()`), `start`, `minutes`, `column`, `columns`.
- `SubscribedSnapshot` (phone.rs:2190-2221): `start: i32`, `end: i32`, `column: usize`,
  `columns: usize`, `all_day: bool`, `background: bool`, `where_: String`
  `#[serde(skip_serializing_if = "String::is_empty")] #[serde(rename = "where")]`,
  `note: String` `#[serde(skip_serializing_if = "String::is_empty")]`, `name: String`,
  `color: String` (`#rrggbb` from the subscription RGB, alpha dropped), `calendar: String`.
- `Tray { due: Vec<TrayCard>, later: Vec<TrayCard> }` (phone.rs:2254-2258).
- `TrayCard` (phone.rs:2260-2272): `id`, `name`, `color: usize`, `deadline: Option<String>`
  (`local_input` = `%Y-%m-%dT%H:%M` [zone], board.rs:422-424), `due_in: Option<String>`
  (`relative_due`), `duration: Option<u32>`, `planned: u32`, `drop_minutes: u32`.
- `ItemDetail` (phone.rs:2274-2290): `id`, `name`, `kind`, `color`, `deadline: Option<String>`
  (`local_input`), `duration: Option<u32>`, `planned: u32`, `remaining: Option<u32>`,
  `severity: Option<u8>` (= importance), `horizon: Option<u8>` (= time_importance),
  `repeat: Option<RepeatDetail>`, `sessions: Vec<SessionDetail>`, `created: NaiveDate`
  (`created.date_naive()` [zone]).
- `RepeatDetail { days: u8, summary: String (rule.summary()), start: i32, minutes: u32 }` (phone.rs:2292-2298).
- `SessionDetail { index: usize, day: NaiveDate, start: i32, minutes: u32 }` (phone.rs:2300-2306);
  `day`/`start` from `session.start` in local time [zone] (phone.rs:2556-2566).
- `DoneRow` (phone.rs:2115-2129): `id`, `archived_at: DateTime<Local>` (full precision RFC 3339),
  `name`, `kind`, `color`, `finished: bool`, `when: String` (`%a %-d %b %H:%M`, e.g.
  "Fri 4 Sep 16:20"), `verdict: String`. `done_rows` takes the first 40 entries (newest first)
  (phone.rs:2131-2146).

Abbreviated example:
```json
{"version":1767000000000,"today":"2026-09-04","now_minutes":725,"palette":[[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0]],
 "labels":{"severity":["Not important","Mildly important","Important","Highly important","Lethally important"],"horizon":["Within a month","Within a week","Within days","Whenever"],"horizon_order":[2,1,0,3],"weekdays":["Mon","Tue","Wed","Thu","Fri","Sat","Sun"]},
 "days":[{"date":"2026-09-04","weekday":"Friday","label":"4 September 2026","is_today":true,"summary":"1h planned · 1 block","behind_minutes":60,
   "entries":[{"id":17,"session":0,"name":"report","kind":"task","color":1,"start":540,"minutes":60,"marker":false,"due":false,"column":0,"columns":1}],
   "ghosts":[]}],
 "tray":{"due":[],"later":[]},
 "items":[{"id":17,"name":"report","kind":"task","color":1,"deadline":null,"duration":60,"planned":60,"remaining":0,"severity":null,"horizon":1,"repeat":null,"sessions":[{"index":0,"day":"2026-09-04","start":540,"minutes":60}],"created":"2026-09-04"}],
 "ranked":[17],"done":[],"notes":"","dials":{"blur":11,"light":39}}
```

### 4.7 `build_day(items, archived, day, now) -> DaySnapshot` (phone.rs:2391-2505)
- Live: every `placements_for(item, day)`; sorted by `(placement.start(), item.id, placed.session)`
  (`Option<usize>`: None < Some) (phone.rs:2402).
- Ghosts: for every archive row, `placements_of(Placeable { is_event: row.is_event, deadline: None,
  duration_minutes: None, sessions: &row.sessions, recurrence: None }, day)`; sorted by `start()`
  (stable; archive order newest-first for ties) (phone.rs:2409-2421). Consequence: only archived
  **task sessions** ghost; an archived event (is_event, no deadline) and an archived routine (no
  sessions) produce none; due markers never ghost.
- `lay_out` over live placements then ghost placements in one list; split back (phone.rs:2423-2429).
  Ghost markers are filtered out after layout (cannot occur in practice).
- Summary: live partitioned by `is_routine()`; `summary_text(summarize(non-routine),
  summarize(routine), summarize(ghosts))` (phone.rs:2474-2478).
- `behind_minutes`: only when `now_marker(day, now)` is Some; non-event entries with `session.is_some()`
  among the non-routine partition (phone.rs:2482-2491).
- `subscribed` left empty here.

### 4.8 `with_subscribed(days, subscriptions, overlay)` (phone.rs:2335-2389)
Per day: `overlay.events_on(date)` (overlay order = sealed sort), dropped unless its subscription
exists and is `enabled`; placements `Block { start, minutes: 0 if is_background() else max(end-start,1) }`;
`lay_out`; background events forced to `column 0, columns 1`; others `lane.columns.max(1)`.

### 4.9 `build_tray(items, day, now, shuffle_seed) -> Tray` (phone.rs:2509-2536)
Filter `wants_planning()`; sort by `importance_score` descending (stable, `unwrap_or(Equal)`);
`backlog_group(deadline, day)`: `Due` if `deadline.date_naive() <= day` [zone], else `Later`
(planner.rs:605-610). `due_in = relative_due(deadline, now)`: negative minutes → "overdue"; ≥1 day →
"{days}d"; ≥1 hour → "{hours}h"; else "{m}m" (planner.rs:614-629). `drop_minutes =
drop_length_for(item)`.

### 4.10 `item_detail(item)` (phone.rs:2538-2569): see `ItemDetail` above.

### 4.11 `calendar_feed(items, now) -> String` (phone.rs:2585-2685)
CRLF lines folded at 75 octets (phone.rs:2727-2745); header `VERSION:2.0`,
`PRODID:-//TaskDeck//Phone view//EN`, `CALSCALE:GREGORIAN`, `METHOD:PUBLISH`, `X-WR-CALNAME:TaskDeck`.
`DTSTAMP` = `now` in UTC. Per item: routine → one VEVENT, UID `taskdeck-{id}-routine@taskdeck`,
floating `DTSTART/DTEND` on the first day in `now.date_naive() + 0..7` the rule falls on (or the
anchor), `SUMMARY:↻ {name}`, `RRULE:FREQ=WEEKLY;BYDAY=…` when repeating, `TRANSP:TRANSPARENT`;
event with deadline → UID `taskdeck-{id}-event@taskdeck`, UTC, length `duration.unwrap_or(15).max(1)`;
task deadline → UID `taskdeck-{id}-due@taskdeck`, 15 minutes, `SUMMARY:⚑ due: {name}`, TRANSPARENT;
each session → UID `taskdeck-{id}-session-{index}@taskdeck`, `SUMMARY:⏱ {name}`. Text escaped per
RFC 5545 (phone.rs:2704-2717). [now][zone]

---

## 5. Planner (pure; planner.rs)

### 5.1 Placement
- `Placement::Block { start: i32, minutes: u32 }` / `Placement::Marker { at: i32, due: bool }`
  (planner.rs:64-72); `start()`, `end()` (marker end = `at + 15`) (planner.rs:74-91).
- `DayPlacement { session: Option<usize>, placement }` (planner.rs:104-108).
- `Placeable { is_event, deadline, duration_minutes, sessions: &[Session], recurrence }` and
  `Placeable::of(&Active)` (planner.rs:131-150).
- `placements_of(p, day)` (planner.rs:153-209):
  1. recurrence Some: if `falls_on(day)` → one `Block { start: start_minutes.clamp(0, 1439),
     minutes: minutes.max(15) }`, `session None`; return (nothing else considered).
  2. event: if `deadline` falls on `day` → `Block { at, duration.max(15) }` when duration is Some,
     else `Marker { at, due: false }`; return.
  3. task: one `Block { start, minutes.max(15) }` per session starting on `day`, `session Some(i)`,
     in session order; then `Marker { at: due, due: true }` if the deadline falls on `day`.
  "Falls on day" = `at.date_naive() == day` and minutes = local `hour*60 + minute` [zone]
  (`minutes_into_day`, planner.rs:94-96). A session that runs past midnight is placed only on its
  start day and not clipped.
- `appears_on(item, day)` mirrors it without building (planner.rs:216-225).

### 5.2 Snap, clamp, resolve
- `snap(m: f32) -> i32 = ((m / 15).round().clamp(0.0, 96.0)) as i32 * 15` (planner.rs:260-268).
  `f32::round` rounds half away from zero; NaN → 0.
- `clamp_block(start: f32, minutes: f32) -> (i32, u32)` (planner.rs:273-279):
  `length = snap(max(minutes, 15.0)).max(15)`; `start = snap(start).clamp(0, 1440 - length)`.
  The board converts its integers with `as f32` (board.rs:1055, 1084, 1124, 1136, 1208).
  Examples pinned by tests: (547, 100) → (540, 105) (board.rs:1542-1550); (i32::MIN, u32::MAX) →
  (0, 1440) (board.rs:1570-1574); (1440, 30) → (1410, 30); (1425, 60) → (1380, 60); routine
  (1430, 5) → (1425, 15) (board.rs:2145-2160).
- `block_from_drag(a, b)` orders the ends then `clamp_block(from, to - from)` (planner.rs:282-289).
- `resolve_on_day(day, minutes) -> Option<DateTime<Local>>` (planner.rs:308-311):
  `minutes.clamp(0, 1439)`; `local_at(day, m)` = `day.and_hms_opt(min(m/60, 23), m%60, 0)` →
  `Local.from_local_datetime(..).earliest()` (planner.rs:296-303) — an ambiguous (fall-back) time
  takes the **earlier** instant; a gap (spring-forward) retries at `min(m + 60, 1439)`. [zone]
- `now_marker(day, now) = minutes_into_day(now, day)` (planner.rs:541-543).
- `format_minutes` → `HH:MM` (clamped 0..=1440) (planner.rs:314-317); `format_duration` → `45m` /
  `2h` / `1h 30m` (planner.rs:320-326).

### 5.3 Budget and estimate arithmetic
- `drop_length_for(item) = remaining_minutes().filter(>0).unwrap_or(30).max(15)` (planner.rs:553-558).
  Used by `AddBlock` with `minutes: None` and by `TrayCard.drop_minutes`.
- `remaining = estimate.saturating_sub(Σ sessions.minutes)`; sessions shorter than 15 still count
  their stored minutes in `planned_minutes`, while placement draws them at `max(15)`.
- `duration_options(current)` folds `current.max(15)` into `DURATION_CHOICES` (planner.rs:573-583).

### 5.4 Layout and summary
- `lay_out(&[Placement]) -> Vec<Lane{column, columns}>` (planner.rs:402-450): stable sort of indices
  by `(start(), -end())`; a placement whose start is `>= cluster_end` closes the cluster; column =
  first column whose end `<= start`, else a new one; every member of a cluster gets
  `columns = number of columns used in it`. Output parallel to input.
- `summarize(&[Placement]) -> DaySummary { planned_minutes: i32 (union of block spans), blocks: usize,
  due: usize (due markers) }` (planner.rs:453-505).
- `summary_text(planned, routine, done)` (planner.rs:516-537): all of planned.blocks, planned.due,
  done.blocks, routine.blocks zero → "nothing on this day"; parts joined by `" · "`:
  "{planned} planned" if `planned.blocks > 0 || (done.blocks == 0 && routine.blocks == 0)`;
  "{n} block(s)" if blocks > 0; "{n} due" if due > 0; "{routine} routine"; "{done} done".

### 5.5 Reflow
`reflow_inputs(day)` (board.rs:888-908): for every item, every `Placement::Block` on `day`:
`session Some(i)` and the item is neither event nor routine → `Flowable { key: (id, i), start,
minutes }`; otherwise → anchor `(start, start + minutes)`. Markers dropped. Then
`overlay.anchors_on(day)` appended (events with `day == day && is_busy()`, subscriptions.rs:460-466;
`is_busy` = `!is_background() && !free && end > start`, subscriptions.rs:256-258). The
subscription's `enabled` flag is **not** checked here (disabled ones are normally already forgotten
from the overlay).

`planner::reflow(blocks, anchors, now) -> Vec<(K, i32)>` (planner.rs:695-722), integers only:
1. `order` = indices stably sorted by `start` (input order breaks ties).
2. anchors: drop empty (`end <= start`), sort ascending (tuple order).
3. `cursor = snap_up(now)` where `snap_up(m) = ceil-to-15 then clamp(0, 1440)` (planner.rs:653-656).
4. per block in order: `length = minutes.max(15)`; `start = max(cursor, block.start)`; one pass over
   the sorted anchors: if `start < a_end && a_start < start + length` → `start = snap_up(a_end)`;
   `start = min(start, 1440 - length).max(0)`; record; `cursor = start + length`.
5. Answer in **input** order (`placed[index]`), every block included.
Nothing moves earlier; a block ahead of the cursor stays unless the cursor passes it; end-of-day
clamp may leave overlaps. The board applies only starts that differ (board.rs:1236-1245).

`planner::behind_minutes(blocks, now)` (planner.rs:667-679): each block clipped to `[start,
min(now, end))`, empty dropped, then `summarize(..).planned_minutes` (union).

### 5.6 Where floats are used
`f32`: scoring (`tasks.rs` §4.3), `snap`/`clamp_block` inputs (`i32/u32 as f32`; exact below 2^24),
`TimelineGeometry` (UI only, planner.rs:229-257), `preview` (UI drags, planner.rs:372-383),
`Look.aspect`. Everything in reflow, lay_out, summarize, behind is integer.

---

## 6. Archive (archive.rs)

### 6.1 `ArchiveLog` — archive.rs:633-800
State: `entries: Vec<Archived>` (newest first), `unreadable: Vec<Vec<u8>>`, `loaded: bool`.
- `load(dir)` (archive.rs:683-729) [disk], once: `File::open`; `NotFound` → empty, other open error →
  Err; split on `b'\n'`; a line that is not UTF-8 → kept as bytes in `unreadable`; blank (trimmed)
  lines skipped; JSON parse failure → kept as bytes in `unreadable`; then
  `entries.sort_by(|a, b| b.archived_at.cmp(&a.archived_at))` (stable: equal timestamps keep
  **file order**).
- `record(dir, row)` (archive.rs:733-745): `append` (serialise first, open with create+append,
  write, `sync_all`; archive.rs:802-817); if loaded, insert at the first index whose
  `archived_at <= row.archived_at` (so a new row goes **before** existing rows with an equal
  timestamp); if not loaded, memory untouched.
- `take(dir, key)` (archive.rs:754-769): `load`; find first `row.key() == key`; remove; `rewrite`;
  on rewrite failure reinsert at the same index and Err. Not found → `Ok(None)`.
- `rewrite(dir)` (archive.rs:774-799): body = every unreadable line verbatim + `\n` **first**, then
  entries in reverse (oldest first) each `serde_json::to_string` + `\n`; temp file, fsync, persist,
  dir fsync.
- `replace_with(rows)` (archive.rs:662-667): `sort_by_key(Reverse(archived_at))` (stable), clears
  `unreadable`, `loaded = true`. `persist` = `rewrite` (archive.rs:671-673).
- Ordering facts among equal `archived_at`: memory after `record` puts the newer-recorded row first;
  `rewrite` writes memory reversed, and a later `load` keeps file order — so after a rewrite and
  reload, two equal-timestamp rows come back in the **opposite** order from memory before the
  rewrite. `take` removes the **first** row matching the key; two rows with an identical key
  (same id and instant) are indistinguishable.
- `Board::state()` loads the archive and fails rather than serving an empty one (board.rs:838-851;
  test board.rs:2062-2079).

### 6.2 Verdicts and summaries (pure)
- `lifetime() = max(archived_at - created, 0)` (archive.rs:248-250); `booked_minutes()`,
  `sittings()` (archive.rs:253-260).
- `timing()` (archive.rs:264-277): event or routine → `Undated`; no deadline → `Undated`;
  `archived_at <= deadline` → `Met(deadline - archived_at)`; else `Missed(archived_at - deadline)`.
- `was_finished() = !is_event && !is_routine() && outcome == Finished` (archive.rs:287-289);
  `mark()` ✓/✗ by it (archive.rs:293-299); `was_judged() = was_finished() && deadline.is_some()`
  (archive.rs:304-306).
- `verdict()` (archive.rs:310-342), parts joined by `"  ·  "` (two spaces each side):
  routine → `"routine dropped"` (repeating) or `"time dropped"`, then
  `"{summary_dated} at {HH:MM} for {duration}"`; event → `"event removed"` and
  `"was set for {%-d %b %H:%M}"` if deadline; task → `outcome_phrase` + `time_phrase`; always last
  `"{format_span(lifetime)} on the board"`.
  - `outcome_phrase` (archive.rs:345-357): Undated → verb; Met/Missed with `num_minutes() <= 30` →
    `"{verb} right on the deadline"`; Met → `"{verb} {span} early"`; Missed → `"{verb} {span} late"`.
  - `time_phrase` (archive.rs:365-383): sittings 0/1/n → none / `"{d} in one sitting"` /
    `"{d} over {n} sittings"`; with estimate: `"{e} estimated, {spent}"` or
    `"{e} estimated, never booked"`; without: spent, or `"no time set aside"`.
- `format_span` (archive.rs:392-409): `< 1 min` "a moment"; `< 60` "{m}m"; `< 24h` "{h}h";
  `< 90 d` "{d}d"; else "{days/30}mo". `format_total` (archive.rs:414-425).
- `Filter`/`OutcomeFilter`/`KindFilter` (archive.rs:428-485), `Summary` + `summarize` (median
  lifetime = `sorted[len/2]`) + `headline()` (archive.rs:491-589), `group_by_month` runs
  (archive.rs:593-624). These feed the desktop ledger; the snapshot uses only `verdict`,
  `was_finished`, `color_id`.
- `color_id()`: routine → 0, else `calendar_item_color` (archive.rs:234-239).

### 6.3 Restore, forget, ghosts
Restore and ForgetArchived: §2.5. Restore of a routine row gives a routine back (board.rs:2197-2227).
Ghosts: §4.7 (only archived task sessions on the day; laid out together with live entries; desktop
caches them as `planner_ghosts`, DOCUMENTATION.md:1820-1823).

---

## 7. ICS, overlay, weather types

### 7.1 ICS parser (ics.rs)
- Entry point: `pub fn parse(text: &str, from: NaiveDate, until: NaiveDate, limits: Limits) -> Result<Parsed, String>`
  (ics.rs:182-214). Both ends inclusive. `Err` only for a structurally broken file.
- `Limits` (ics.rs:106-137), defaults: `max_depth 8`, `max_lines 200_000`, `max_events 20_000`,
  `max_occurrences_per_event 1_000`, `max_occurrences 50_000`, `max_rule_steps 100_000`.
- Output `Occurrence { day: NaiveDate, start: i32, end: i32, all_day: bool, free: bool, name: String,
  #[serde(default)] location: String, #[serde(default)] description: String }` (ics.rs:44-70),
  derives Serialize/Deserialize; `is_busy()` (ics.rs:77-79). `Parsed { occurrences, name:
  Option<String> (X-WR-CALNAME), problems: Vec<String> }` (ics.rs:83-97), occurrences sorted by
  `(day, start, end, name)` (ics.rs:209-211).
- RRULE support: FREQ DAILY/WEEKLY/MONTHLY/YEARLY, INTERVAL ≥ 1, COUNT, UNTIL, BYDAY (with
  ordinal), BYMONTHDAY, BYMONTH, BYSETPOS; not BYYEARDAY, BYWEEKNO, BYHOUR and below, EXRULE,
  SECONDLY/MINUTELY/HOURLY (ics.rs:763-785).
- Zone reads [zone]: UTC times converted `with_timezone(&Local)` (ics.rs:1218, 904); `TZID` times
  resolved via the file's own VTIMEZONE (`Zones::resolve`, ics.rs:740-749) then to Local; an
  undefined TZID is read as local wall time with a problem line (ics.rs:1223-1228); floating times
  are taken as written. Imports `board::NAME_MAX_CHARS` for name bounding (ics.rs:32).
- Fetch side (not pure): `fetch_one` caps at `MAX_ICS_BYTES = 8 MiB` and `FETCH_TIMEOUT = 20 s`
  (subscriptions.rs:93-95, 545-576); `fetch_all` window = `Local::now().date_naive()` − 60 … + 400
  days [clock][zone] (subscriptions.rs:594-600); a failed fetch carries that subscription's previous
  events forward; `covers` updated only if something was read (subscriptions.rs:637). Refresh every
  600 s (`REFRESH_EVERY`, subscriptions.rs:97).

### 7.2 Overlay operations (subscriptions.rs:381-484)
- `Overlay::sealed(events, status)` (subscriptions.rs:392-422): drop `end < start`; clamp
  `start` to 0..=1440 and `end` to start..=1440; truncate summary/location/description to
  200/120/300 chars; take first `EVENTS_MAX` (5000) **before** sorting; sort by
  `(day, start, end, subscription, summary)`; truncate `Failed.why` to 200; `covers: None`.
  `.covering(covers)` sets the span (subscriptions.rs:387-390).
- `digest()` (subscriptions.rs:431-450): FNV-1a 64 (`FNV_OFFSET 0xcbf2_9ce4_8422_2325`,
  `FNV_PRIME 0x0000_0100_0000_01b3`) over each event's `subscription` (u64 LE), `day` as
  `%Y%m%d` text, `start` (i32 LE), `end` (i32 LE), `[all_day as u8, free as u8]`, summary bytes,
  location bytes, description bytes. `status` and `covers` excluded.
- `Board::adopt_overlay(fresh) -> bool` (board.rs:798-816): equal digest → copy `status` and
  `covers` only, return false, no version move; else replace, version +1, true.
- `OverlayEvent::minutes()`, `is_background()` (`all_day || minutes >= 720`), `is_busy()`
  (subscriptions.rs:235-259). `anchors_on`, `events_on`, `forget`, `status_for` (subscriptions.rs:460-483).
- `better_name` / `is_unnamed` (subscriptions.rs:312-334): only while the name equals
  `"Calendar {id}"` or `name_from_url(url)`; prefers a non-blank `FetchOutcome::Ok.title`.

### 7.3 Weather report types
The snapshot carries **no** weather type: weather is served separately at `GET /api/weather` from
the static `REPORT` (weather.rs:244-255) [global], never through the board. Types (all
`#[derive(Clone, Debug, Serialize)]`): `Report { place: String, latitude: f32, longitude: f32,
timezone: String, utc_offset_seconds: i64, fetched_at: u64, now: Option<Now>, hours: Vec<Hour>,
days: Vec<Day> }` (weather.rs:219-235); `Hour { t, temp, feels, code, sym, day, pop, mm, wind, gust,
dir, uv, humidity }` (weather.rs:148-178); `Day { date, code, sym, high, low, feels_high, feels_low,
mm, pop, wind, gust, uv, sunrise, sunset }` (weather.rs:180-202); `Now { temp, feels, code, sym, day,
wind, gust, dir, humidity, mm }` (weather.rs:204-217). Times are the place's local
`YYYY-MM-DDTHH:MM` strings. Fetched from Open-Meteo with `latitude = coordinates[0]`,
`longitude = coordinates[1]`, `FORECAST_DAYS = 7` (weather.rs:291-298, 430).

---

## 8. Settings the design makes board settings

### 8.1 Palette (today: `colorschemes.json` + `userconfig.toml`)
- `ColorScheme { name: String, colors: [[u8; 4]; 6], is_user_configurable: bool }` (color.rs:11-16),
  Serialize/Deserialize, no attributes. Six RGBA slots: 0–4 severity/horizon ramp (least → most
  important), 5 events (color.rs:50-51).
- File `colorschemes.json`: `HashMap<u32, ColorScheme>` pretty JSON (keys as strings)
  (color.rs:241-287). Example: `{"0":{"name":"COLORSCHEME ZERO","colors":[[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0]],"is_user_configurable":false}}`.
- Built-ins (`builtin_schemes`, color.rs:61-154) at ids 0..=6: `COLORSCHEME ZERO` (all `[0,0,0,0]`,
  color.rs:28-39), `EMBER`, `TIDE`, `MOSS`, `DUSK`, `DISCO`, `MILD DISCO`; ramp alphas
  `RAMP_ALPHA = [74, 88, 104, 120, 136]`, event alpha `EVENT_ALPHA = 104` (color.rs:24-25). EMBER
  e.g. `[[84,100,118,74],[124,124,96,88],[190,150,56,104],[212,96,36,120],[206,44,56,136],[72,140,196,104]]`.
  `install_builtins` reinstalls them at every startup and moves a user scheme off a reserved id,
  following `selected_id` (color.rs:189-239).
- Selection: `selected_colorscheme_id: u32` in `userconfig.toml`, default 0, clamped 0..=200000
  (initialization.rs:313-316). Missing id → desktop draws `default_scheme()` (utilities.rs:94-103);
  phone/server get `[[0;4];6]` (ui.rs:3482-3487, server_main.rs:276-279). All-transparent tells the
  page to use its own tints (phone.rs:2063-2064).
- The server reads the palette **once at startup** (server_main.rs:276-279); the desktop reads it per
  query. A scheme change on the desktop calls `board.touch()` (ui.rs:3343).

### 8.2 Home place (today: `userconfig.toml` `coordinates`)
- `coordinates: [f32; 2]` (initialization.rs:441), read from TOML key `coordinates` as a
  two-element array, both finite, else default `[0.0, 0.0]` (initialization.rs:280-296); written
  back with `float_pair_array` (initialization.rs:414). Order is `[latitude, longitude]`
  (weather.rs:291-298). The weather service takes it at start and on `set_coordinates`
  (weather.rs:269-272, 432).
- Other `Config` keys (initialization.rs:438-495, defaults in `config_from`, initialization.rs:243-362):
  `start_in_fullscreen` (false), `background` (""), `enable_fps_counter` (false),
  `window_size_startup` ([1280,720], each ≥ 200), `calendar_weeks_to_show` (100, 6..=520),
  `selected_monitor_name` (""), `three_day_weather` (false), `background_image_tint_percent`
  (30, 1..=100), `background_blur_percent` (11, ≤100), `background_light_percent` (39, ≤100),
  `ui_scale_percent` (0 = auto, else 40..=100), `phone_server_enabled` (false),
  `phone_server_port` (7373, ≥ 1024), `phone_token` (""), `phone_bind_address` ("127.0.0.1"),
  `phone_public_url` (""), `frame_cap_fps` (0 = uncapped, else 15..=360), `server_url` (""),
  `server_token` ("").

### 8.3 Subscription colours
`DEFAULT_COLORS` (subscriptions.rs:60-67): `[90,140,200,255]`, `[200,120,90,255]`,
`[120,175,110,255]`, `[175,120,190,255]`, `[200,175,90,255]`, `[110,175,185,255]`, assigned
`len % 6` at subscribe time.

---

## 9. Tests worth porting

### 9.1 board.rs (mod tests, board.rs:1490-2288)
- `commands_parse_from_the_wire_with_either_time_shape` (1517): `move_block` JSON shape; naive and `null` deadlines; round-trip; bad time and unknown op rejected.
- `a_created_task_is_saved_and_read_back` (1538): Create trims, snaps (547,100)→(09:00,105), estimate = length, horizon 1, reopen keeps id and counter.
- `a_create_with_an_absurd_length_is_clamped_not_a_crash` (1562): `i32::MIN`/`u32::MAX` → whole day.
- `two_items_under_one_id_are_told_apart_at_load` (1578): duplicate id renumbered at load, commands then hit the right item.
- `a_calendar_is_subscribed_to_renamed_recoloured_switched_off_and_removed` (1605): trim, webcal→https, duplicate and http refused (400), rename/colour/disable persist, remove twice → 410.
- `a_subscribed_meeting_flows_the_day_around_itself_the_way_one_of_ours_does` (1660): overlay meeting anchors reflow; all-day band is not an anchor.
- `an_overlay_that_says_the_same_thing_again_does_not_move_the_version` (1712): equal digest → no version move.
- `creates_take_the_shape_their_kind_asks_for` (1737): event/routine create shapes, default name, Add event without time 400, QuickAdd dated gets severity 2, blank name 400.
- `blocks_move_book_remove_and_unplan` (1765): AddBlock without minutes takes the remainder; MoveBlock snap; bad session 410; RemoveBlock; Unplan keeps estimate; unknown id 410.
- `events_and_routines_move_their_own_time` (1793): event MoveBlock sets deadline/duration, end-of-day pull-back; task-only ops on events 400; routine SetRepeat/MoveBlock edit the rule; Once lands on the viewed day.
- `the_ranking_knobs_follow_the_deadline` (1823): SetHorizon range, first deadline seeds severity 2, SetSeverity range, clearing deadline keeps both knobs, Rename trims/refuses blank.
- `retiring_files_first_and_restoring_brings_it_back_whole` (1845): Complete event 400; Complete/Delete archive newest first; Restore under old id; second Restore 410; ForgetArchived; Forget leaves no row.
- `reflow_and_notes_and_queries` (1887): behind 60; reflow at 12:05 lands after the event at 13:00; other day moved 0; notes detab and length cap; Feed via apply 400; ranked_ids; next_free_start.
- `dates_off_the_calendar_are_refused_before_anything_adds_to_them` (1916): year out of range → 400 "out of range" for every dated op; nothing applied.
- `an_add_with_nothing_to_rank_it_by_gets_the_quick_adds_horizon` (1942): bare Add → horizon 1; given level kept; event stays unranked.
- `forgetting_something_that_is_not_there_is_gone_not_fine` (1958): Forget missing → 410, version unchanged.
- `a_reflow_from_a_client_slides_the_day_from_the_clients_now` (1967): `from` honoured on a non-today day; without it 0 moved.
- `a_command_is_stamped_with_the_clock_of_the_host_that_applied_it_first` (1984): `stamped`, `item_id`/`set_item_id` on archive ops; `at` capped at now+1 day; slightly-ahead `at` kept and ordered.
- `a_board_that_becomes_its_own_adopts_its_temporary_items` (2016): temporary ids renumbered past real ones, saved, idempotent.
- `a_restored_row_wearing_a_temporary_id_is_renumbered` (2040).
- `the_board_state_is_an_error_rather_than_an_empty_archive_when_the_log_cannot_be_read` (2062, unix).
- `a_name_is_trimmed_and_bounded_everywhere_a_name_comes_in` (2096): 200 chars in, 201 out on QuickAdd/Add/Rename/Create; chars not bytes.
- `a_level_off_the_scale_is_refused_at_add` (2123).
- `nothing_runs_past_midnight` (2140): create/move/event-length/routine clamp; SetEstimate 0 → 400.
- `a_legacy_save_loads_migrated_and_a_corrupt_one_is_quarantined` (2165).
- `a_routine_comes_back_as_a_routine_and_the_log_loads_itself_for_a_restore` (2197): Complete routine 400; Delete; unloaded log; Restore loads it; ForgetArchived on unloaded log 410.
- `a_restored_item_whose_id_is_taken_gets_a_fresh_one` (2230).
- `replace_takes_the_servers_picture_and_keeps_ids_ahead` (2266): replace, version moves, temp range kept, `renumber`.

### 9.2 planner.rs (mod tests, planner.rs:724-1318)
- `behind_counts_only_the_part_of_a_block_that_has_passed` (743): clipping, markers ignored, overlap counted once.
- `summary_text_says_only_what_there_is` (755): exact strings for all combinations.
- `reflow_slides_work_past_now_in_order_and_around_anchors` (772): now rounded up; ahead block stays; pushed past anchor.
- `reflow_moves_nothing_when_nothing_is_behind` (790).
- `reflow_keeps_input_order_in_its_answer_and_steps_over_stacked_anchors` (798): answer in input order; anchor ending off-grid snaps up.
- `reflow_clamps_at_the_end_of_the_day_rather_than_dropping_anything` (809).
- `reflow_ignores_empty_anchors_and_respects_the_minimum_length` (818): also `snap_up` values.
- `a_routine_places_a_block_on_every_day_its_rule_names` (876); `a_routine_ignores_a_deadline_and_sessions_it_should_not_have` (902); `a_routine_block_is_never_shorter_than_a_block_can_be` (918).
- `geometry_round_trips_minutes_through_pixels` (927) — UI geometry.
- `snap_rounds_to_the_quarter_hour_and_stays_in_the_day` (939); `snap_survives_a_length_no_clock_could_hold` (951).
- `clamp_block_enforces_a_minimum_and_keeps_blocks_inside_the_day` (968); `block_from_drag_works_in_both_directions` (985).
- `placements_distinguish_planned_work_from_a_due_date` (993); `a_task_split_across_a_day_shows_every_session_and_its_deadline` (1022); `events_are_blocks_once_they_have_a_length` (1052); `appears_on_agrees_with_placements_for` (1067); `unplanned_deadlineless_task_has_no_placement` (1094).
- Drag previews (UI): `create_drag_previews_the_pulled_out_block` (1099), `move_drag_keeps_the_grab_point_under_the_pointer` (1111), `move_drag_cannot_push_a_block_out_of_the_day` (1124), `resize_drag_holds_the_start_and_respects_the_minimum` (1135), `backlog_drop_uses_the_supplied_length_at_the_pointer` (1147).
- `lay_out_gives_disjoint_blocks_the_full_width` (1159); `lay_out_splits_overlapping_blocks_into_columns` (1170); `lay_out_uses_a_third_column_only_for_a_real_three_way_overlap` (1189); `lay_out_reuses_a_column_once_it_is_free` (1205).
- `summarize_counts_overlapping_time_once` (1217); `summarize_adds_disjoint_spans` (1231).
- `formatting_is_human_readable` (1240); `resolve_on_day_returns_the_requested_wall_clock_time` (1250).
- `a_plain_drag_plans` (1259): `CreateKind::default()` is Task.
- `duration_options_keep_a_hand_dragged_length_selectable` (1265); `drop_length_is_the_unplanned_remainder` (1278); `backlog_group_puts_everything_still_owed_at_the_top` (1296); `relative_due_reads_as_a_countdown` (1311).

### 9.3 archive.rs (mod tests, archive.rs:819-1283)
- `retiring_and_restoring_lose_nothing` (849); `a_restored_undated_task_is_not_scored_as_corrupt` (878).
- `legacy_rows_load_and_mean_what_they_meant` (891): legacy line, outcome defaults Finished.
- `the_timestamp_keeps_its_name_on_disk` (907): wire name `inactivated`, never `archived_at`.
- `timing_reads_the_deadline_against_the_finish` (917): finishing exactly at the deadline is Met.
- `an_event_is_never_late` (933); `a_legacy_event_row_does_not_claim_to_have_been_finished` (948).
- `a_dropped_routine_comes_back_as_a_routine` (972): verdict "routine dropped" / "every day at 23:00 for 8h".
- `the_verdict_holds_the_estimate_against_the_booking` (1009): "finished 2d early", "2h estimated, 3h over 2 sittings".
- `a_deadline_met_to_the_minute_is_neither_early_nor_late` (1027): 10 minutes early → "right on the deadline".
- `an_unbooked_task_says_so` (1036); `spans_are_one_coarse_unit` (1048); `totals_drop_the_minutes_once_they_stop_mattering` (1059).
- `the_summary_counts_only_promises_that_were_actually_made` (1067); `an_empty_summary_says_nothing_rather_than_zeroes` (1099).
- `filters_narrow_independently` (1104); `months_become_runs_in_the_order_given` (1128); `grouping_nothing_produces_nothing` (1145).
- `the_log_round_trips_through_a_file_newest_first` (1150); `recording_into_an_unloaded_log_does_not_double_the_row` (1168); `taking_a_row_out_rewrites_the_file_without_it` (1182).
- `unreadable_lines_are_counted_and_never_discarded` (1209); `a_line_that_is_not_even_text_is_set_aside_not_fatal` (1236); `a_log_out_of_order_on_disk_still_reads_newest_first` (1265).

### 9.4 tasks.rs (mod tests, tasks.rs:780-1511)
- Routines/rules: `a_routine_is_neither_ranked_nor_waiting_to_be_planned` (824), `a_rule_fires_on_the_days_it_names` (845), `no_days_is_a_one_off_not_a_broken_rule` (860), `clearing_the_last_day_lands_on_the_day_you_are_looking_at` (875), `a_rule_says_what_it_amounts_to` (895), `a_routine_saved_before_one_offs_existed_still_loads` (908: `{"days":127,"start_minutes":1380,"minutes":480}`).
- Colour: `calendar_item_color_mapping` (919), `a_palette_index_is_always_inside_the_palette` (935).
- Scoring: `score_rises_as_a_deadline_approaches` (981), `pressure_is_exactly_one_at_the_deadline` (1000), `importance_separates_tasks_sharing_a_deadline` (1014), `importance_stops_out_arguing_urgency_eventually` (1023), `a_distant_deadline_still_ranks_by_importance` (1038), `overdue_pressure_levels_off_at_the_cap` (1045), `a_late_trivial_task_does_not_bury_important_upcoming_work` (1059), `undated_tasks_ripen_towards_their_weight_and_stop` (1072), `a_ripe_undated_task_never_outranks_comparable_overdue_work` (1089), `scores_stay_finite_at_absurd_distances` (1107), `a_planned_task_rises_as_its_slot_approaches` (1121), `a_slipped_plan_does_not_escalate_like_a_missed_deadline` (1138), `planning_a_dated_task_lifts_it_on_the_day` (1152), `a_planned_task_with_no_importance_is_still_scored` (1180), `a_dated_task_missing_its_importance_is_scored_as_middling` (1191), `the_most_pressing_session_drives_planned_pressure` (1204), `whenever_surfaces_eventually_but_out_argues_nothing` (1221), `an_item_with_nothing_to_go_on_is_surfaced_at_the_top` (1299), `an_event_closer_in_time_scores_higher` (1308).
- Jitter: `jitter_is_small_bounded_and_actually_varies_per_task` (1319), `jitter_holds_all_day_and_turns_over_at_midnight` (1341), `a_days_worth_of_edits_cannot_reorder_the_list` (1358), `jitter_cannot_reorder_tasks_that_genuinely_differ` (1381).
- Budget/migration: `sessions_report_planned_and_remaining_time` (1235), `legacy_single_slots_migrate_into_sessions` (1261).
- Files/ids: `quarantine_moves_corrupt_file_aside` (1393), `quarantine_reports_when_no_file_present` (1423), `assign_missing_ids_backfills_and_preserves` (1434), `assign_missing_ids_gives_a_repeated_id_to_its_first_holder_only` (1458), `assign_missing_ids_starts_at_one_when_empty` (1478), `bucket_by_deadline_day_groups_and_preserves_order` (1484).

### 9.5 phone.rs snapshot-side tests (also relevant)
`the_snapshot_declares_how_far_the_calendars_were_read_only_when_there_are_any` (2775),
`a_request_for_more_days_than_the_server_builds_is_cut_down_quietly` (2814),
`commands_parse_from_the_page_shapes` (3184), `snapshot_query_defaults_and_validates` (3210),
`datetime_local_round_trips` (3220), `a_day_lays_out_live_entries_and_ghosts_together` (3229),
`behind_is_zero_on_any_day_but_today` (3264), `done_rows_round_trip_their_key_through_json` (3278),
`the_tray_splits_by_the_shown_day` (3303), `the_feed_writes_every_layer` (3327),
`the_body_cap_admits_the_longest_notes_the_board_does` (3445).

---

## 10. Could not determine / not verified
- I did not read the bodies of the planner tests from planner.rs:876 onward, the phone.rs tests, or
  sync.rs; their one-liners above are from names and nearby code, not from reading each assertion.
- I did not read ics.rs past the entry point, limits, RRULE struct and zone conversion
  (ics.rs:223-1418 unread in detail): exact unfolding, all-day handling, EXDATE/RECURRENCE-ID
  override rules and `spread` across midnight are not digested.
- The exact chrono serialisation of `DateTime<Local>` in this build (fractional-second digits) was
  not checked by running anything; examples follow chrono's documented default and the test
  fixtures at archive.rs:894.
- Whether serde_json accepts a missing `importance`/`time_importance`/`deadline` on `Active`
  without `#[serde(default)]`: serde's derive treats a missing `Option<T>` field as `None`, so they
  should load; not verified by running.
- How the desktop's own planner computes its masthead/behind figures (ui.rs) was not compared line by
  line with `build_day`; only `planner_behind_minutes` → `Board::behind_minutes` (ui.rs:3233-3235)
  was seen.
- The phone page's outbox/retry and offline handling (phone.html) and the desktop client sync path
  (sync.rs, main.rs) were not digested beyond the facts cited.
- DOCUMENTATION.md §16–§18 were skimmed only at §16.3.2, §16.8 and §17.4; other subsections were not
  compared against the code.
