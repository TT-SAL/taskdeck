# fixtures/

Data that later stages test against instead of today's code (SYNC_PLAN.md §6.1 item 5): made-up
boards written by today's `Board` code, the same boards in the older build's file formats, the JSON
`phone::snapshot` returns for them, and transcripts of wire commands with the state after each.
**Everything here is invented**: names, notes, calendars and the home place are synthetic; subscription
addresses are under `example.invalid` and `example.com`; no line comes from a real board.

Everything except this file and `generator/` is generated. Do not edit it by hand.

```sh
bash scripts/regen-fixtures.sh      # regenerate everything
bash scripts/check-fixtures.sh      # regenerate into a temporary directory; fail on any difference (CI)
cargo test --locked -p taskdeck-fixtures
```

## Layout

| Path | What | Bytes (`du -sb`) |
|---|---|---|
| `boards/<board>/` | a data directory, exactly the files today's build keeps for that board | 3,978,443 |
| `legacy-main/<board>/` | the same board in the older build's formats, with a `userconfig.toml` | 1,923,769 |
| `legacy-main/MIGRATION.json` | what today's code made of each legacy directory (counts) | 4,613 |
| `snapshots/europe-berlin/<board>/<from>_<days>d_at-<now>.json` | `phone::snapshot`, one file per request | 1,159,985 |
| `snapshots/america-new-york/<board>/…` | the same for `small` and `dst-edges`, under New York | 456,861 |
| `transcripts/europe-berlin/<name>.jsonl` | ten transcripts and `COVERAGE.json` | 3,196,369 |
| `transcripts/america-new-york/<name>.jsonl` | the same ten, under New York | 3,209,906 |
| `MANIFEST.sha256` | `sha256sum` format, one line per generated file | 15,331 |
| **all generated data** | | **13,945,277** |

Board sizes: `archive-10k` 3,760,036; `unreadable` 127,783 (two deliberately over-long lines);
`id-limits` 49,509; `small` 20,581; `dst-edges` 16,074; `empty` 4,460. No compression.

## The pinned clock and zones

- **Zone.** Boards are written with `TZ=Europe/Berlin` (gap 2026-03-29 02:00→03:00, fold
  2026-10-25 03:00→02:00). Snapshots and transcripts are written again with `TZ=America/New_York`
  (gap 2026-03-08 07:00Z, fold 2026-11-01 06:00Z). The generator refuses to run unless `TZ` is
  exactly the zone of the set it writes, and checks that `chrono::Local` really has that zone's
  offsets (a machine without the tz database would give UTC). The rules come from the system's tz
  database; both zones' rules have been unchanged since 2007.
- **Clock.** Every `now` is an explicit UTC instant in the generator (`clock::utc`); today's API
  takes `now` everywhere the generator calls it (`Board::apply`, `phone::snapshot`). The values are
  in the files: each transcript step's `now`, each snapshot's file name.
- **Randomness.** SplitMix64 written in the crate, seeded per use (`archive-10k`: 10000; seeded
  transcript *n*: *n*).

Where today's code reads ambient state on the paths the generator calls, and what is done about it:

| Ambient read | Where | What the generator does |
|---|---|---|
| local zone (`chrono::Local`, `TZ`) | every `DateTime<Local>`: parsing naive wire times, `resolve_on_day`, serialising offsets, "today" in snapshots and reflow | pinned and verified |
| system clock (`Local::now`, `Utc::now`, `SystemTime`) | `quarantine_corrupt_file`, `taskdeck-server`'s `seed_version`, the feed fetcher, `generate_token`, weather, `sync.rs` set-aside | none of these is called; no board has a corrupt items file |
| version counter | `taskdeck-server` seeds it from the clock | **normalised**: every board is opened fresh, so `version` starts at 0 |
| `HashMap` order | `colorschemes.json` (`color::save_colorschemes` writes a `HashMap`) | **normalised**: written in key order, same pretty-printed shape |
| process globals | `phone::backdrop()`, `phone::look_dials()` | pinned: no backdrop (`look` absent), dials 11 and 39 (the defaults) |
| files written by reading | `Board::open` seeds a missing items file (`[]`) and notepad (`""`); `read_colorschemes` seeds `{}` | boards are always read from a temporary copy |
| temporary file names | every atomic save | not part of any output |
| floating point | `importance_score` (f32 `exp2`) orders `ranked` and the tray | not normalised: same on every IEEE platform in practice, but a one-ULP difference in a platform's `exp2` could reorder near-ties (R1 §1 "presentation only") |

## The boards

Every board directory holds `read_at_startup.json`, `notepad_text.json`, `colorschemes.json` and
`userconfig.toml`; `archived.jsonl` once something has been archived; `subscriptions.json` and
`subscribed_cache.json` once a calendar has been subscribed to. Runtime-only files (`.lock`,
`outbox.json`, `.client-of`, `phone_background.img`, quarantined copies) are not part of any board.
`userconfig.toml` is what `get_check_and_set_config` writes on a first start, then the home place
(`coordinates = [-41.25, -132.5]`, open ocean) and the palette (`selected_colorscheme_id = 100`, a
fixture scheme in `colorschemes.json` beside the built-ins).

| Board | What it holds | For |
|---|---|---|
| `empty` | a first start: `[]`, `""`, built-in palettes, default settings | `core` genesis; seeded transcripts start here |
| `small` | 22 live items: tasks at every severity and horizon, undated, quick-added, renamed; tasks with several blocks, an estimate, a block moved, one unplanned, one with a block removed; events (wire and timeline, moved, resized); routines (daily, weekdays, one-off); 6 archive rows (met, missed, undated, dropped task, removed event, dropped routine; on the way one row was restored and one forgotten for good, so the log has been rewritten whole); notes with a tab and non-ASCII text; 4 subscriptions (one webcal rewritten, one unnamed and renamed, one disabled) and an overlay with timed, all-day, free, twelve-hour and disabled-calendar events, OK and failed fetch status and a covered window | `core` transcripts and snapshots, `android-shell` bridge, `desktop` import check |
| `archive-10k` | 10,000 archive rows over four years (tasks finished and dropped, events, routines, sessions, estimates) below 150 live items (110 tasks, 35 events, 5 routines) | the `core` budgets (31-day snapshot, entries applied, checkpoint load), `recovery`, `android` |
| `dst-edges` | items, blocks, due dates and archive times in and around Berlin's gap and fold (through day+minutes, naive wire times, and both offsets of the fold), at New York's transition instants, across midnight, at year ends (one due in New York's 2026 and Berlin's 2027), and on 1970-01-01 local (1969 in UTC) and 9999-12-31; one archive row in 9999 written by `ArchiveLog::record` (the wire clamps a retirement to a day past `now`) | zone handling in `core`; import-check's "every instant survives conversion to UTC" (SYNC.md §9 step 4) |
| `id-limits` | item ids 1, 2^40±1, 2^53±1, 2^62−1…2^62+2, 2^63−1, 2^63, u64::MAX−1, u64::MAX; archive ids near the same limits; names at 200 characters (ASCII, astral, with a combining mark); notes at 20,000 characters; 24 sessions on one task; 1440-minute lengths; a routine at 23:59 for 1440 minutes; the maximum 20 subscriptions, **ids 1–10 twice** (ten added, the board reopened, ten more: the counter is seeded from items only, R1 §2 item 4), one name at 200 characters and one address at 2,000 | import-check's id folding and limits; device ids below 2^53 |
| `unreadable` | an archive whose lines include invalid JSON, a JSON array and string, a row with fields missing, an unknown outcome, an empty and a blank line, invalid UTF-8, a byte-order mark, a CRLF row (readable), a readable row with a 50,000-character name, a 70,000-character unreadable line, and a last line with no final newline, among good rows; and an item that is **both live and archived** (V2-11) | import-check's "unreadable lines carried or reported" and "id both live and archived" |

The `unreadable` archive is assembled byte by byte (it stands for a file damaged outside the
program); every other file is written by today's own writers.

## The older build's formats (`legacy-main/`)

`origin/main` was not available in the checkout the generator was built in (no `main` branch, a
shallow clone), so its code could not be built or read. The shapes are what today's code and
documentation record of the older build, each with its evidence:

| Legacy shape | Evidence |
|---|---|
| archive rows `{"id","importance","name","created","deadline","is_event","inactivated"}` in that order; no outcome, sessions, estimate or horizon | `legacy_rows_load_and_mean_what_they_meant` in `src/archive.rs`; DOCUMENTATION.md §6 "Superseded `InActive`" |
| only finished rows (and events): the old delete wrote nothing | DOCUMENTATION.md §6; `a_legacy_event_row_does_not_claim_to_have_been_finished` |
| every fifth item and row without an `id` (written before ids existed) | `#[serde(default)]` on `id` in `tasks.rs` and `archive.rs`; the pre-id test in `src/board.rs` |
| items without `sessions`; a task's first block as `planned_start` + `duration_minutes` | `a_legacy_save_loads_migrated_and_a_corrupt_one_is_quarantined` in `src/board.rs`; `migrate_legacy_plans` |
| no routines; the horizon on the old three-level scale (3 written as 0) | DOCUMENTATION.md §7 (horizon "load-compatible with the old 3-level urgency scale"), §18 |
| a notepad with tabs; `legacy-main/empty` seeded with `{}` | DOCUMENTATION.md §9.1; `utilities::read_notepad_text` ("not the `{}` this used to seed it with") |
| `userconfig.toml` with numbers as strings (`calendar_weeks_to_show = "100"`, `selected_colorscheme_id = "100"`) and only the keys that predate the phone, the server and the newer dials | the `write_normalized_config` test in `src/initialization.rs`; DOCUMENTATION.md §11 |
| no `subscriptions.json` or `subscribed_cache.json` | subscribed calendars are §23, newer than the archive redesign |

`legacy-main/MIGRATION.json` records, per directory, what the down-conversion left out
(`written`) and what today's `Board::open` (plus `load_archive`, `read_config_only` and the
palette lookup) made of it (`opened`): items and ids backfilled, sessions made from
`planned_start`, rows with the `0` id, unreadable lines, whether the notepad fell back to the
placeholder, tabs left, whether the palette and home place were found. The generator fails if
today's code reports a problem opening any of them or ends with an id that is 0 or repeated.

## Snapshot files

`snapshots/<zone>/<board>/<from>_<days>d_at-<now>.json`: `phone::snapshot(board, palette, None,
from, days, now)` serialised with `serde_json::to_string_pretty` plus a newline, where `palette` is
the board's selected scheme (the lookup `taskdeck-server` makes), `ranked` is computed by today's
code, and `now` is the UTC instant in the name (`YYYYMMDDTHHMMZ`). The board is opened fresh for
each board's set (version 0). Requests:

- `small`, `now` 2026-10-20 08:15Z: days 2026-10-20, 10-23, 10-25 (Berlin's fold), 11-01 (New
  York's fold); 7 days from 10-19; 31 days from 10-05; `days` 0 and 40 (clamped to 1 and 31).
- `archive-10k`, same `now`: 1, 7 and 31 days.
- `dst-edges`: Berlin's gap day at 03:30 local, the fold day and the day before inside the second
  02:30, New York's gap day and its fold day at both 01:30s, two days at the year end, 1970-01-01,
  9999-12-31, 7 days from 10-22, 31 days from 03-01.
- `id-limits`, `unreadable`: 1 and 7 days; `empty`: 1 day.

## Transcript files

JSON Lines. The first line is the start, then one line per command:

```text
{"kind":"start","transcript":<name>,"zone":<TZ>,"board":<board under boards/>,"seed":<n or null>,"now":<RFC 3339>,"state":<BoardState>}
{"kind":"step","step":<1..>,"now":<RFC 3339>,"wire":<request body, exactly as sent>,"op":<its "op" or null>,"status":<HTTP status>,"body":<response body>,"state":<BoardState after>}
```

A step is answered as `taskdeck-server` answers `POST /api/command`: a body that does not parse as a
command is `400 {"error":"Could not read the command: …"}`, a query (`snapshot`, `feed`, `board`) is
`400 {"error":"That is a query, not a command."}`, anything else is `Board::apply(command, now)`:
`200` with `phone::reply_json` (`{"id"?, "moved"?, "ok":true, "session"?, "version"}`) or the
refusal's status with `{"error": message}`. The request-key replay cache and the HTTP framing are
not modelled. `state` is `BoardState` (`Command::Board`'s answer: `version`, `items`, `archive`
newest first, `notes`, `subscriptions`, `overlay`) in today's serialisation, compact. A replayer
needs only the start state, the `wire` text and the `now` of each step.

| Transcript | Start | What |
|---|---|---|
| `every-command` | `small` | all 25 commands carried out (most in several forms), the three queries refused |
| `refusals` | `small` | every refusal of a well-formed request, and unparseable bodies; ends with notes accepted at 20,000 characters that become longer once tabs expand (V2 audit item 18) |
| `dst-edges` | `dst-edges` | naive times in both zones' gaps and folds, reflow inside the fold, instants whose local year differs between the zones |
| `id-limits` | `id-limits` | the calendar cap; `quick_add` on a board holding u64::MAX gets u64::MAX again; ids above 2^53 and 2^62 |
| `seeded-01`…`04` | `empty` | 30 seeded commands each, some aimed at ids that are not there |
| `seeded-05`, `06` | `small` | 20 seeded commands each |

`COVERAGE.json` counts the ops carried out and refused and each refusal message. Under Berlin the
generator fails unless every command was carried out and every refusal in
`transcripts::REFUSALS` was seen. Two refusals cannot be reached and are not listed: "That time
doesn't exist on this day (daylight saving)." (no gap in either zone is longer than the hour
`resolve_on_day` steps over) and the 500s of a failed save.

## Quirks of today's output worth knowing

- Times carry the writing zone's offset (`+01:00`/`+02:00` in the boards, `-05:00`/`-04:00` in the
  New York transcripts' new data); the same instant has different bytes (R1 §1 item 2).
- The same `wire` and `now` give different results per zone: a naive time resolves locally, "today"
  is local (a reflow without `from` moves blocks only on the local today), and the 1970–9999 check
  is on the local date (`dst-edges` steps 14–15 are refused in one zone and accepted in the other).
- Archive retirements are clamped to `now` + 1 day (`small`'s rows were made at a later `now` for that).
- The id counter saturates: on `id-limits` a new item gets u64::MAX, which is already taken.
- Subscription ids repeat after a reopen (`id-limits`), and `with_subscribed` matches the first.
- `colorschemes.json` key order is not stable in today's own writes (normalised here).
- The legacy `{}` notepad opens as today's placeholder text, not as an empty notepad.

## The generator

`generator/` is the workspace member `taskdeck-fixtures`. It depends on the root package (without
`desk`) and on crates the root package already uses; nothing else. `taskdeck-fixtures --zone
berlin|new-york --out <dir>` writes one zone's set; the Berlin set must be written first (the New
York run reads its boards, and writes the manifest).
