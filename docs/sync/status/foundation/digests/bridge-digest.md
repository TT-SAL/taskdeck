# bridge-digest.md — the phone page and the server that answers it (fact sheet)

Facts only, each with `file:line` (repository `/home/user/taskdeck`). `phone.html` = `src/phone.html`,
`phone.rs` = `src/phone.rs`. The page is one IIFE in one `<script>` (phone.html:948-5702).

---

## 1. Every network request the page makes

### 1.1 `api(path, options)` (phone.html:1240-1274)
- Headers: `X-TaskDeck-Token: state.token` always, merged with `options.headers` (1241). If `options.body` and no
  `Content-Type`: `typeof body === 'string'` -> `application/json`, else `application/octet-stream` (1244-1246).
- `fetch(path, {...options, headers, cache: 'no-store', signal})` (1251). Same-origin relative paths only.
- Timeouts via `AbortController`: `REQUEST_TIMEOUT_MS = 15000`; paths starting `/api/wait` get
  `WAIT_TIMEOUT_MS = 45000` (1238-1239, 1247-1249). Body read under the same timer (1254).
- Response: body text parsed as JSON if non-empty (parse errors ignored -> `null`) (1255-1256).
  Non-2xx -> `throw new ApiError(status, payload.error || "TaskDeck answered ${status}.")` (1257-1260).
  Abort -> `ApiError(0, "TaskDeck did not answer in time. Is it running, and is this phone on a working network?")`;
  any other fetch failure -> `ApiError(0, "Can't reach TaskDeck. Is it running on the desktop, and is this phone on the same network?")`
  (1262-1268). `class ApiError extends Error { status }` (1272-1274).
- `unreachable(error) = error.status === 0 || error.status >= 500` (1488).

### 1.2 `api()` call sites

| # | Where | Method / path / query | Body / extra headers | Response fields read | On failure |
|---|---|---|---|---|---|
| 1 | `post()` phone.html:1500, used by `send` (1776) and `replay` (1560) | `POST /api/command` | `JSON.stringify(command)` (so `Content-Type: application/json`); header `X-TaskDeck-Request: <key>` | `send` returns the reply object to callers, which read `id` (create, 5362), `session` (add_block, 4909, 4920), `moved` (reflow, 5458); others test truthiness | see §6 (`send`, `replay`) |
| 2 | `load()` phone.html:1694 | `GET /api/state?from=${range.from}&days=${range.days}` (`spanFor`, 1635-1651; days <= 31 or falls back to `{asked, span}`) | none | whole `Snapshot` (§3.4): `days[]`, `version`, `items`, `known`, `look`, `dials`, `tray`, `palette`, `done`, `notes`, `ranked`, `labels` | 401 -> `showGate()`; else fall back to `recall(day)`, set `state.staleAt`, banner (1723-1756) |
| 3 | `fetchPage()` phone.html:2529 (agenda paging, `extendAfter` 2507-2513 / `extendBefore` 2515-2522) | `GET /api/state?from=${from}&days=${days}` (rest of a month / start of a month) | none | `absorb(snap)` (days, known) | 401 -> gate; else `state.agenda.stalled = true` (2537-2543) |
| 4 | `loadWeather()` phone.html:2829 | `GET /api/weather` (only when `state.weather.here` is null) | none | `report.hours` (Array check), then the whole report (`latitude`, `longitude`, `place`, `utc_offset_seconds`, `timezone`, `now`, `hours`, `days`, `fetched_at`) | 401 -> gate; else if `wx.desk` known -> Open-Meteo direct for the desk's place, else `wx.error = message` (2834-2851) |
| 5 | Look sheet dial `send` phone.html:4660 | `POST /api/look` | `JSON.stringify({ blur, light })` (0-100 each; seeded from `snap.dials` or 11/39, 4649-4650) | nothing; then `load({quiet:true})` | `toast(error.message)` (4662) |
| 6 | "Use the desk's picture" phone.html:4684 | `POST /api/background` | headers `X-TaskDeck-Clear: 1`; body `' '` — a **string**, so `api()` sets `Content-Type: application/json` | nothing | `hint.textContent = error.message` (4688). NOTE: the server answers `/api/background` with **415** unless `Content-Type` is `application/octet-stream` (phone.rs:821-823), and the clear header is only read after that check (phone.rs:825); as written this request is refused with 415 `"Send this as application/octet-stream."` |
| 7 | `sendBackground()` phone.html:4715 | `POST /api/background` | `await blob.arrayBuffer()` (ArrayBuffer -> `application/octet-stream`); JPEG from a 1080x2340 canvas at quality 0.92 (4540, 4695-4714) | nothing | thrown to the Save button: `hint.textContent = error.message` (4632-4635) |
| 8 | `watch()` phone.html:5553 | `GET /api/wait?version=${state.snap.version}` | none | `answer.version` | 401 -> gate and `watch` returns; else `failures += 1`, sleep `min(30000, 2000 * failures)` (5568-5572) |

### 1.3 Requests outside `api()`
- **Background photo, CSS**: `paintField()` sets `--photo: url("/bg-${look.id}.jpg?token=${encodeURIComponent(token)}")`
  on `<html>` and class `has-photo` (phone.html:2355-2376); fetched by the browser (token in the query, no header).
- **Background photo, explicit**: `keepPhoto(path)` `fetch(`${path}?token=...`)` into Cache Storage `taskdeck-photo-v1`
  when not already there, deleting other entries (2382-2408).
- **Open-Meteo direct**: `fetchForecast(lat, lon, target)` `fetch(`${OPEN_METEO}&latitude=${latitude}&longitude=${longitude}`)`
  with a 15 s abort, no custom headers (3122-3138). `OPEN_METEO` (3031-3035) =
  `https://api.open-meteo.com/v1/forecast?current=temperature_2m,apparent_temperature,weather_code,is_day,wind_speed_10m,wind_gusts_10m,wind_direction_10m,relative_humidity_2m,precipitation&hourly=temperature_2m,apparent_temperature,weather_code,is_day,precipitation_probability,precipitation,wind_speed_10m,wind_gusts_10m,wind_direction_10m,uv_index,relative_humidity_2m&daily=weather_code,temperature_2m_max,temperature_2m_min,apparent_temperature_max,apparent_temperature_min,precipitation_sum,precipitation_probability_max,wind_speed_10m_max,wind_gusts_10m_max,uv_index_max,sunrise,sunset&wind_speed_unit=ms&timezone=auto&forecast_days=7`.
  Called for the phone's own place (`wx.here`, lat/lon rounded to 3 decimals at 3862-3863) or, when `/api/weather`
  fails, for the desk's remembered place (`wx.desk` from the last desk report, full precision) (2825-2841).
  Errors: `"Open-Meteo could not be reached from this phone (...)"`, `"Open-Meteo answered ${status}"`.
  Builds the same report shape as `weather::build_report` (`buildReport`, 3037-3120) plus `for` and `via: 'phone'`.
- **Sky SVGs**: `<img src="/weather/${sym}.svg">` (3730, 3832); `warmSymbols()` fetches every stem in `WX_SKY` once
  (plain `fetch`, no token), then sets `localStorage['taskdeck-sky-warm'] = '1'` (3944-3952); called on boot in the
  weather view (5678).
- **Manifest**: `installManifest()` appends `<link rel="manifest" href="/manifest.webmanifest?token=${encodeURIComponent(token)}">`
  (1223-1230), on boot when a token exists (5649).
- **Icons**: `<link rel="apple-touch-icon" href="/icon-192.png">`, `<link rel="icon" href="/icon-192.png" ...>`
  (phone.html:16-17).
- **Service worker**: `navigator.serviceWorker.register('/sw.js').catch(() => {})` (5698-5700), unconditional of token.
- **Photo picker preview**: `img.src = URL.createObjectURL(chosen)` (blob:) (4581-4591); `revokeObjectURL` is never
  called.
- **Navigation**: gate `location.href = `/?token=${encodeURIComponent(token)}`` (5467-5471).
- Not used anywhere in the page: `XMLHttpRequest`, `navigator.sendBeacon`, `WebSocket`, `EventSource`, `import(`,
  `window.open`, `<form>`, `target=`, `document.cookie`, `sessionStorage`, `indexedDB` (grep).

---

## 2. Every op the page posts to `/api/command`

Wire format: `board::Command`, `#[serde(tag = "op", rename_all = "snake_case")]` (board.rs:111-280). Instants accept
RFC 3339 or naive local `YYYY-MM-DDTHH:MM[:SS]` (`wire::optional_local`, `parse_local_input`, board.rs:392-419).
Replies: `{ "ok": true, "version": N, "id"?: u64, "session"?: usize, "moved"?: usize }` (`reply_json`, phone.rs:172-184).

The 17 verbs the page sends (grep `op: '` in phone.html):

| op | JSON sent | Built at | Reply used | Undo |
|---|---|---|---|---|
| `move_block` | `{op, id, session, day, start, minutes}` — drag (`session: entry.session`, `day: state.snap.days[0].date`, 4365); sheet time/length edits (5069, 5080); postpone `day: +1` (4939-4942); event "When" sends **no `session`** (5159) | 4365, 4939, 5069, 5080, 5159 | none | postpone: `unqueue(key)` or send the same move back to the original day (4944-4948) |
| `set_notes` | `{op, text}` | 4727 | truthiness -> toast "Notes saved" | none |
| `complete` | `{op, id, at}` with `at = stamp()` = `new Date().toISOString()` with millis stripped, e.g. `2026-11-21T10:15:30Z` (4825, 4877-4881) | 4881 | truthiness | `unqueue(key)` if still local, else `{op:'restore', id, archived_at: at}` (4883-4887) |
| `restore` | `{op, id, archived_at}` — Undo (4885, 4899) or Done-tab row (`row.archived_at` from `snap.done`, 5257) | 4885, 4899, 5257 | truthiness -> toast | — |
| `delete` | `{op, id, at}` (`stamp()`) | 4895 | truthiness | as `complete` (4897-4901) |
| `add_block` | `{op, id, day, start, minutes}` — Book (`nextFreeSlot`, 4906-4908); sheet "book remaining" (5137) | 4908, 5137 | `result.session` -> `state.selected.session` (4909-4912) | `unqueue(key)` or `{op:'remove_block', id, session}` where session = `result.session` or `sessionAt(id, day, start, minutes)` from the snapshot (4915-4924) |
| `remove_block` | `{op, id, session}` | 4923, 5083 | none | none |
| `rename` | `{op, id, name}` (trimmed, non-empty, changed) | 5006 | none | none |
| `set_deadline` | `{op, id, deadline}` — `"YYYY-MM-DDTHH:MM"` from chips (`dueOptions`, 4806-4820) or `datetime-local`, or `null` to clear | 5096 | none | none |
| `set_severity` | `{op, id, level: Number}` | 5116 | none | none |
| `set_horizon` | `{op, id, level: Number}` | 5121 | none | none |
| `set_estimate` | `{op, id, minutes}` | 5125, 5163 | none | none |
| `unplan` | `{op, id}` | 5144 | none | none |
| `set_repeat` | `{op, id, days, day}` — toggles bit `1 << i`; `All` = 127; `Once` = 0 | 5178, 5181, 5182 | none | none |
| `quick_add` | `{op, name, deadline}` (`deadline` = `datetime-local` value or `null`) | 5303 | none | none |
| `create` | `{op, kind: 'task'|'event'|'routine', name, day, start, minutes}` | 5361 | `result.id` -> `openItem(id, kind === 'task' ? 0 : null)` | none |
| `reflow` | `{op, day: state.day}` (no `from`) | 5457 | `result.moved` -> toast | none |

- Not sent by the page: `add`, `forget`, `forget_archived`, `snapshot`/`feed`/`board` (queries; refused with 400
  `"That is a query, not a command."` on `/api/command`, phone.rs:892), and all subscription ops.
- Every command carries a request key: `requestKey()` = `crypto.randomUUID()` or two `Math.random().toString(36)`
  slices (1496-1499); `send(command, { key = requestKey() })` (1761); queued entries keep the key so a replay reuses it
  (1533, 1560).
- `session` values: from the snapshot (`EntrySnapshot.session: Option<usize>`, phone.rs:2228; `SessionDetail.index`,
  phone.rs:2302) and from `add_block` replies; `null`/absent addresses an event's or routine's own block
  (board.rs:166-176). Indexes are positions in `Active.sessions`, so they are only valid against the snapshot they were
  read from.
- The desktop host stamps a command before applying (`command.stamped(now)`: fills `at` for complete/delete and `from`
  for reflow when absent, board.rs:371-378; ui.rs:2699-2700). The server main loop applies without `stamped`
  (server_main.rs:455). `archived_at(at, now)` caps a client `at` at `now + 1 day` (board.rs:466-469).

---

## 3. Every route the server answers (`phone.rs` `handle`, phone.rs:709-943)

Server: `tiny_http` 0.12 (`Server::http((ip, port))`, phone.rs:638), `WORKERS = 2` threads (130), one pulse thread.

### 3.1 Before routing
- `Content-Length > DRAIN_LIMIT (8 MiB)` -> request is `mem::forget`-ed: no response, connection leaked (152, 717-721).
- `split_url` on `?`, `parse_query` with `+`->space and `%XX` decoding (1083-1137). Duplicate keys: first wins (`lookup`).
- Auth (`is_authorised`, 1008-1025): token non-empty and any of: header `X-TaskDeck-Token` (trimmed), header
  `Authorization: Bearer <t>`, query `token` — compared with `same_token` (constant-time, 1029-1034). All candidates tried.
- `request_key`: header `X-TaskDeck-Request`, trimmed, first 128 chars (407-413).
- `Encoding::of`: gzip if any `Accept-Encoding` offers `gzip` or `*` without `q=0` (1284-1286, 1353-1362).

### 3.2 Route table

| Method | Path | Auth | Request | Response | Status | Cache-Control | Ref |
|---|---|---|---|---|---|---|---|
| GET | `/api/wait` | yes (401 JSON + no-store + guards) | `?version=<u64>` (missing/bad -> 0) | `{"version": N}` (never gzip) | 200 | `no-store` | 732-746, 541-587 |
| GET, HEAD | `/`, `/index.html` | no | — | `PAGE` (`include_str!("phone.html")`), gzip `best` cached once | 200 | `no-store` | 752-755, 158 |
| GET, HEAD | `/icon.png` | no | — | `ICON` (`include_bytes!("../icon.png")`), `image/png` | 200 | `public, max-age=604800` | 756, 160, 1236 |
| GET, HEAD | `/icon-192.png`, `/icon-512.png` | no | — | scaled PNG (Lanczos3, `CompressionType::Best`), cached in `OnceLock`; falls back to original | 200 | `public, max-age=604800` | 757-758, 1385-1406 |
| GET, HEAD | `/sw.js` | no | — | `SERVICE_WORKER` (`include_str!("phone_sw.js")`), `application/javascript; charset=utf-8` | 200 | `no-store` | 761-764 |
| GET, HEAD | `/weather/<stem>.svg` | no | stem looked up in a fixed table (`weather::symbol_svg`) | `image/svg+xml` | 200 / 404 `"No such symbol."` | `public, max-age=31536000, immutable` | 775-783, 1242 |
| GET, HEAD | `/manifest.webmanifest` | no | `?token=` optional | JSON (see 3.5), `application/manifest+json` | 200 | `no-store` | 784-786, 1810-1829 |
| any | anything below, unauthorised | — | — | `{"error":"Not authorised. Open the phone view from the link in TaskDeck's settings."}` | 401 | `no-store` | 787-790 |
| GET, HEAD | `/bg-<id>.jpg` | yes | `Accept` containing `image/avif` -> AVIF | AVIF or JPEG, `Vary: Accept` | 200 / 404 `"No such picture."` | `private, max-age=31536000, immutable` | 796-814, 1238 |
| POST | `/api/background` | yes | `Content-Type: application/octet-stream` required (else 415 `"Send this as application/octet-stream."`); header `X-TaskDeck-Clear` (any value) = clear; body <= `MAX_UPLOAD_BYTES` 6 MiB | `{"ok":true,"id":<string|null>}` | 200 / 400 (`"That file is not a picture this can read."`, `"Could not keep the picture: ..."`, `"This copy has nowhere to keep a picture."`, `"That request is too large."`) | `no-store` | 821-833, 1756-1778 |
| POST | `/api/look` | yes | `Content-Type: application/json` (else 415 `"Send this as application/json."`); body `{"blur": u32, "light": u32}` (both required; clamped to 100) | `{"ok":true,"id":<string|null>,"kept":bool}` | 200 / 400 `"That is not a pair of dials."` | `no-store` | 837-851 |
| GET | `/api/weather` | yes | — | `weather::Report` JSON (bytes cached per report, gzip best) | 200 / 503 `"No forecast yet. TaskDeck fetches one when it starts and every ten minutes after."` | `no-store` | 857-864, 1330-1348 |
| GET | `/api/state` | yes | `?from=YYYY-MM-DD` (year 1970..=9999; default today on the host clock), `&days=<u32>` (default 1; clamped silently to 1..=31) | `Snapshot` JSON | 200 / 400 (``"`from` should be a date like 2026-09-04, not `...`."``, ``"`days` should be a number, not `...`."``) / 503 | `no-store` | 865-871, 962-981, 218-272 |
| GET | `/api/board` | yes | — | `BoardState` JSON | 200 / 500 (archive unreadable) / 503 | `no-store` | 873-876, 206-211; board.rs:473-486 |
| POST | `/api/command` | yes | `Content-Type: application/json` (else 415 `"Commands are sent as application/json."`); body <= `MAX_BODY_BYTES` (= `NOTES_MAX_CHARS * 4 + 4096`); optional `X-TaskDeck-Request` | `reply_json` | 200 / 400 (`"Could not read the command: {serde error}"`, query op, board 400s) / 410 / 500 / 503 | `no-store` | 887-929, 136 |
| GET, HEAD | `/calendar.ics` | yes (query `token` is how calendar apps send it) | — | iCalendar text, `text/calendar; charset=utf-8`, gzip fast when >= 1400 bytes | 200 / 500 / 503 | `no-store` | 930-934, 2585 ff. |
| any | other | — | — | `{"error":"No such page."}` | 404 | `no-store` | 935 |

- POST to `/api/wait` or OPTIONS anywhere: no special handling; falls to 401 (unauthorised) or 404. No CORS headers,
  no preflight answers (comment phone.rs:877-886).
- HEAD on `/api/*` is not routed (only GET), so it is 404 when authorised.
- `ask(command)` (946-960): sends `PhoneRequest { command, reply }` to the board thread, `wake()`, waits
  `REPLY_TIMEOUT = 8 s` (122); channel closed -> 503 `"TaskDeck is shutting down."`; timeout -> 503
  `"TaskDeck did not answer in time. Is it still running?"`.
- Body read: `read_body_up_to(cap)`: declared length > cap or more than cap bytes read -> 400 `"That request is too large."`;
  read error -> 400 `"Could not read the request: ..."` (983-1001).
- Errors are `json_error(status, message)` = `{"error": message}`, `application/json; charset=utf-8`, never gzipped
  (1372-1376). Board statuses: `bad_request` 400, `gone` 410, `failed` 500 (board.rs:497-508).

### 3.3 Response headers on every response
`guards()` (phone.rs:1208-1220), added to every response in `handle` (938-942) and to the three `/api/wait` answer
paths via `guarded()` (1200-1206), verbatim:
```
Referrer-Policy: no-referrer
X-Content-Type-Options: nosniff
Content-Security-Policy: default-src 'none'; script-src 'self' 'unsafe-inline'; worker-src 'self'; style-src 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self' https://api.open-meteo.com; manifest-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'
```
(The Rust string uses `\` line continuations, so the policy is one line with single spaces.) Plus
`Cache-Control` from `caching_for(path)` (1234-1245) on every `handle` response; `/api/wait` answers carry
`Cache-Control: no-store` explicitly. `Content-Encoding: gzip` when compressed (`Encoding::text` >= 1400 bytes with
`Compression::fast()`; `fixed` and `report` with `Compression::best()`; only if smaller, 1256-1370).

### 3.4 Shapes
- `Snapshot` (phone.rs:2055-2094): `version: u64`, `today: NaiveDate`, `now_minutes: i32`, `palette: [[u8;4];6]`,
  `labels: Labels { severity: [&str;5], horizon: [&str;4], horizon_order: [u8;4], weekdays: [&str;7] }` (2148-2166),
  `days: Vec<DaySnapshot>`, `tray: Tray { due, later: Vec<TrayCard> }`, `items: Vec<ItemDetail>`, `ranked: Vec<u64>`,
  `done: Vec<DoneRow>` (<= `DONE_ROWS_MAX` 40), `notes: String`, `known?: Covered {from, until}` (only if any enabled
  subscription), `look?: { id, aspect, top }`, `dials: { blur, light }`.
- `DaySnapshot { date, weekday, label, is_today, summary, behind_minutes, entries: Vec<EntrySnapshot>, ghosts: Vec<GhostSnapshot>, subscribed?: Vec<SubscribedSnapshot> }`
  (2168-2187). `EntrySnapshot { id, session: Option<usize>, name, kind, color, start, minutes, marker, due, column, columns }`
  (2225-2240). `GhostSnapshot { name, finished, color, start, minutes, column, columns }` (2243-2252).
  `SubscribedSnapshot { start, end, column, columns, all_day, background, "where"?, note?, name, color: "#rrggbb", calendar }`
  (2190-2221).
- `TrayCard { id, name, color, deadline?, due_in?, duration?, planned, drop_minutes }` (2260-2272).
  `ItemDetail { id, name, kind, color, deadline: Option<"YYYY-MM-DDTHH:MM">, duration?, planned, remaining?, severity?, horizon?, repeat?: { days, summary, start, minutes }, sessions: [{ index, day, start, minutes }], created }`
  (2274-2306). `kind` is `"task" | "event" | "routine"` (2310-2318).
- `DoneRow { id, archived_at: DateTime<Local> (RFC 3339 with offset), name, kind, color, finished, when, verdict }` (2115-2146).
- `BoardState { version, items: Vec<Active>, archive: Vec<Archived>, notes, subscriptions, overlay }` (board.rs:473-486).
  Note: `subscriptions[].url` (the calendar credential) and `overlay.status[].outcome.why` travel in this response.

### 3.5 Manifest (phone.rs:1810-1829)
`{"name":"TaskDeck","short_name":"TaskDeck","start_url":"/?token=<encoded>" or "/","scope":"/","display":"standalone","background_color":"#0f1113","theme_color":"#0f1113","icons":[{"src":"/icon-192.png","sizes":"192x192","type":"image/png","purpose":"any"},{"src":"/icon-512.png","sizes":"512x512","type":"image/png","purpose":"any"}]}`
— served without auth; echoes whatever `token` query it was given into `start_url`.

### 3.6 Request-key replay cache (`Replies`, phone.rs:280-404)
- Per `PhoneServer` (created in `start`, 652) — in memory only, lost on restart. `REPLIES_KEPT = 1024`, FIFO eviction
  (147, 357-363).
- `claim(key)`: empty key -> `Ours`. `Done(v)` -> replay `v` (board not asked). `InFlight` -> poll every 25 ms up to
  `REPLY_TIMEOUT` 8 s, then `Busy`. `Late(rx)` -> `try_recv`: `Ok(Ok(v))` -> Done; `Ok(Err)`/disconnected -> Ours; empty ->
  Busy. New key -> InFlight, `Ours` (332-373).
- In `/api/command` (893-925): `Busy` -> 503 `"TaskDeck is still working on that change; it is not lost."`; `Ours` ->
  send to board; `Ok` -> `settle` (Done); board `Err` -> `release` (forget key) and its status; reply timeout ->
  `park_late` and 503 `"TaskDeck did not answer in time. Is it still running?"`; channel closed -> `release`, 503
  `"TaskDeck is shutting down."`.
- Errors are not cached (a refused key can be retried as new).

### 3.7 `/api/wait` semantics (phone.rs:415-587, 729-746)
- Parked on `Pulse` with `seen` and deadline `now + WAIT_TIMEOUT (25 s)`; the worker returns at once.
- `answer_parked` answers a parked request when `seen != version` (any difference, not only greater), the deadline
  passes, the server stops, or `interrupt()` — always with the current version `{"version": version}` (435-442, 552-562).
  Sleeps until the earliest deadline, at most 1 s (575-585).
- `PhoneServer::start` publishes the board's version first (633). If the pulse thread has gone (`closed`), the request is
  answered immediately with `pulse.current()` (739-744).
- Publishers: desktop `phone_changed` after every apply and `phone_repaint` (ui.rs:3336-3345); server main loop after
  each applied command, overlay change and auto-rename (server_main.rs:460, 480, 490).

---

## 4. Everything the page stores

`localStorage` only (every access in try/catch). No `sessionStorage`, no cookies, no IndexedDB.

| Key | Holds | Read | Written / removed |
|---|---|---|---|
| `taskdeck-token` (`TOKEN_KEY`) | the token string | `readToken` (1220) | from `?token=` on every load (1215-1218) |
| `taskdeck-quiet-offline` (`QUIET_KEY`) | `'yes'` = "Don't warn" chosen | 1044 | `setQuietOffline` set/remove (1045-1051), from banner action (1744) |
| `taskdeck-notice` (`NOTICE_KEY`) | text of unacknowledged refusals | 1043 | `rememberNotice` (1052-1058) |
| `taskdeck-mode` | `'day'`, `'agenda'`, `'weather'` (only day/weather honoured on read) | 1062-1065 | `setMode` (2025) |
| `taskdeck-snap-<YYYY-MM-DD>` (`SNAP_KEY`) | `{ at: ms, snap: <one-day Snapshot> }`; newest 14 kept (`SNAPS_KEPT`); never overwritten by a lower `version` | `recall` (1313-1318); `snapKeysOldestLast` walks all keys (1283-1293) | `remember` (1294-1312), evicts oldest, then all others on quota error |
| `taskdeck-month-<YYYY-MM>` (`MONTH_KEY`) | `{ at: ms, days: [DaySnapshot...] }` for months -2..+4 from today | `recallMonth` (1384-1391), `seedAgenda` | `keepMonth` (1349-1378); `pruneMonths` at boot removes out-of-range (1394-1409) |
| `taskdeck-outbox` (`OUTBOX_KEY`) | `[{ command, label, at: ms, key }]` | `outbox()` (1472-1474) | `keepOutbox` (1475-1481), removed when empty; `queueEdit` (1531-1542), `replay`, `unqueue` (4828-4835), Discard (1588-1592) |
| `taskdeck-weather` (`WEATHER_KEY`) | `{ at: ms, report }` (report with `for`, `via`) | `recallWeather` (2738-2745) | `keepWeather` (2746-2748) |
| `taskdeck-weather-place` (`PLACE_KEY`) | `{ latitude, longitude, at }` the phone's own place (3 decimals) | `recallPlace` (2793-2797) | `keepPlace` (2803-2808) — never sent to the desk |
| `taskdeck-weather-desk` (`DESK_KEY`) | `{ latitude, longitude, place }` from the desk's last report | `recallPlace` (2798-2801) | `keepDesk` (2809-2812) via `adoptWeather` (2759-2762) |
| `taskdeck-sky-warm` (`SKY_WARM_KEY`) | `'1'` once all sky SVGs were fetched | 3946 | 3950 |

- Cache Storage: page writes `taskdeck-photo-v1` (`PHOTO_CACHE`, 2382; `keepPhoto`). Service worker uses
  `taskdeck-shell-v2`, `taskdeck-photo-v1`, `taskdeck-sky-v1` (phone_sw.js:12-22).
- The day and month caches hold item names, notes, done rows (`snap` includes `items`, `notes`, `done`).

---

## 5. The service worker (`src/phone_sw.js`, 161 lines, 7,803 bytes)
- Caches: `CACHE = 'taskdeck-shell-v2'` with `SHELL = ['/']`; `PHOTO = 'taskdeck-photo-v1'`; `SKY = 'taskdeck-sky-v1'`;
  `KEEP = [CACHE, PHOTO, SKY]` (12-28).
- `install`: `cache.addAll(['/'])`, `skipWaiting()` (30-34). `activate`: delete caches not in `KEEP`, `clients.claim()` (36-42).
- `fetch` (44-113): only GET, same origin. Ignored (network): paths starting `/api/`, `/calendar.ics`,
  `/manifest.webmanifest`, `/sw.js` (51).
  - `/bg-*`: `photo()` — cache-first keyed by **path without query**, on network success deletes all other entries,
    stores; offline miss -> `Response.error()` (60-63, 131-161).
  - `/weather/*.svg`: `sky()` — cache-first by path, kept forever (70-73, 115-129).
  - Shell: `'/'`, `'/index.html'` or any `request.mode === 'navigate'` maps to `'/'` (75-76); serve the cached copy at
    once, refresh in the background; replace only on `response.ok` and `content-type` containing `text/html` (98-112).
    Other paths (icons) are not intercepted.
- Page dependencies on it: registration (phone.html:5698-5700); `PHOTO_CACHE` name shared (2378-2382); offline cold
  open relies on the cached shell plus localStorage; sky pictures offline rely on `taskdeck-sky-v1` (3938-3952);
  only works in a secure context (phone_sw.js:8-11; page comment 5696-5697).

---

## 6. Page behaviour that depends on the transport
- **Token gate**: no token -> `showGate()` (5645-5647); any 401 from `load`, `fetchPage`, `loadWeather`, `watch`,
  `replay` -> gate (1724, 2538, 2835, 5569, 1564); `send` on 401 keeps the edit and shows the gate (1783-1786; the later
  `else if (error.status === 401)` at 1793 is unreachable). Gate hides mast/timeline/agenda/weather/bar, says how many
  edits are kept (5400-5419); "Open" navigates to `/?token=...` (5467-5471).
- **`send(command, {refreshSheet, key})`** (1761-1800): if the outbox has entries, `flush()` first; still waiting ->
  queue this one behind (1774). Otherwise `post`; success -> `load({quiet:true})`, re-render sheet, return reply.
  Errors: unreachable (0 or >= 500, includes 503 Busy) -> `queueEdit` with the same key; 401 -> keep + gate;
  410 -> toast message, clear selection, close sheet, reload; other (400, 415...) -> toast message; returns `null`.
- **Outbox replay** (`flush`/`replay`, 1544-1586): in order; stops at the first unreachable; 401 -> gate and stop;
  other refusals dropped and collected into a notice `"A change made here could not be applied: <label> — <message>"`
  (or `"N changes made here could not be applied: ..."`, first 3 then "and N more"), persisted via `rememberNotice`.
  Head removed only if storage still has the same head (`at` and `key`). Triggered by `load()` before each fetch
  (1688) and by `send`.
- **Offline banners and "as of"**: on a failed load, `state.staleAt = keptSince()` (oldest month on screen in agenda,
  the day's `at` in day view, 1415-1429); masthead shows `" · as of HH:MM"` (`staleNote`, 1937-1941); banner
  `"Showing kept days — the oldest here is from HH:MM — <error>"` or `"Showing this day as of HH:MM — <error>"` plus
  `" · N changes waiting to be sent"`, with Retry, Discard (if waiting) and "Don't warn" (if nothing waiting)
  (1723-1756). Quiet mode hides the banner only when nothing is waiting (1742). Pending banner:
  `"N change(s) waiting to be sent: <labels>"` with Retry/Discard (1593-1600). Weather: `" · as of"` after 45 min
  (`WX_OLD_MS`, 3359-3365).
- **Waiting marks**: item sheet shows `"N change(s) to this is/are waiting to be sent — what is shown here is from before it/them."`
  using `waitingFor(id)` = outbox entries whose `command.id === id` (1484, 5036-5039). No per-row marks in the
  timeline/agenda (grep).
- **Refusals**: toasts for direct refusals; persisted notice banner with OK for replay refusals (1603-1610).
  Undo toasts last 6 s, others 3.2 s (5390-5399).
- **Long-poll loop** `watch()` (5540-5574): only while `visibilityState === 'visible'` (else sleep 1 s); no snapshot ->
  sleep 5 s then `load({quiet:true})`; `/api/wait?version=<snap.version>`; if the answer differs from the current
  snapshot version -> `load({quiet:true})`, and if the version did not move, sleep 5 s. Errors: backoff `min(30 s, 2 s × failures)`.
- **Timers** (`startPolling`, 5576-5618): every 60 s `load({quiet:true})` (with midnight roll-over in the agenda);
  every 60 s in weather view `tickWeather()` + `loadWeather()` (fresh for `WX_FRESH_MS` 5 min, 2735); every 60 s
  now-line/glance redraw. `visibilitychange` -> visible: re-render and `load({quiet:true})` (5619-5627); `online` event ->
  clear stall, reload weather, `load` (5629-5633).
- **Load ordering**: `loadSeq` drops overtaken answers (1661, 1700); cached day painted first, then outbox flush, then
  fetch (1675-1694). Agenda requests are chained (`state.agenda.chain`, 1001).

---

## 7. DOM sinks and navigation
- **The single `innerHTML` sink**: `el()` attribute `html` -> `node.innerHTML = v` (phone.html:1073). Callers:
  - 3604: weather readout `bits.join(' · ')` — `<b>${temp(...)}</b>`, `xml(skyWord(code))`, numeric strings, `dirWord(...)`.
  - 3755: the chart SVG string from `wxChartSvg(win, width)` (built with `parts.push(...)` template strings, text escaped
    with `xml(...)`, 3439-3537).
  - 3759-3761: three static legend spans (`<i style="background:var(--w-...)"></i>...`).
  Data in these comes from the weather report (server's or Open-Meteo's) and plan-band block labels (escaped with `xml`,
  3522).
- Other DOM writes use `textContent`, `createTextNode`, `setAttribute` (`el`, 1068-1082) and `replaceChildren`.
- **URL assignments / navigation**: `location.href = /?token=...` (5470); `link.href` for the manifest (1228);
  `img.src = blob:` (4591); `<img src="/weather/...svg">` (3730, 3832); CSS `--photo: url("/bg-...jpg?token=...")`
  (2369). No `window.open`, no `target=`, no `<a href>` built in JS (grep), no `<form>`.
- **File input**: `<input type="file" accept="image/*" style="display:none">`, opened by "Choose picture…" (4576, 4639);
  read via `URL.createObjectURL`; cropped to canvas 1080x2340 and sent as JPEG 0.92 (4695-4716).
- **Geolocation**: `navigator.geolocation.getCurrentPosition(..., { enableHighAccuracy: false, timeout: 15000, maximumAge: 300000 })`,
  only offered when `window.isSecureContext` (3852-3880, 3911).
- **Other APIs**: `navigator.vibrate(10)` on drag (4340); `confirm('Drop the changes waiting to be sent?')` (1589);
  `due.showPicker()` (5104); `crypto.randomUUID` (1497). No clipboard use.
- `<meta name="theme-color">` content rewritten from `look.top` (`themeColor`, 2410-2414).

---

## 8. Inline script and style facts (for a strict CSP)
- **One** inline `<script>` element (phone.html:948-5702), no `src` scripts, no modules. Service worker is a separate
  file (`/sw.js`).
- **One** `<style>` element (phone.html:19-855).
- Inline `style="..."` attributes: in markup `<noscript><p style="...">` (885); created by script through `el({style})`
  (e.g. 1986, 2131, 2426, 2437, 3350, 3608, 3816, 3820, 4216, 4240, 4251, 4264, 4576, 5109, 5132, 5181, 5182, 5250, 5309),
  inside `innerHTML` strings (SVG `style="..."` 3439-3537, legend `<i style>` 3759-3761), and CSSOM `.style.*` /
  `setProperty` (2363-2370, 4111, 4355, 4424, 4458-4482, 4565-4586).
- Inline event-handler attributes in markup: none (grep for ` on[a-z]+="`). Handlers are `addEventListener` or
  property assignment (`img.onload`, `img.onerror`, 4582-4590); `el()` maps `on*` keys to `addEventListener` (1074).
- `eval`-like: none (`eval(`, `new Function`, string `setTimeout` — none found).
- `data:` URLs: none in the page (CSP allows `img-src data:`). `blob:` URL: one (`createObjectURL`, 4581).
- External origin contacted: `https://api.open-meteo.com` only (3031).
- Current CSP already relies on `script-src 'unsafe-inline'` and `style-src 'unsafe-inline'` (phone.rs:1214-1215).

---

## 9. Sizes (measured on the working tree)
- `src/phone.html`: 5,704 lines, 288,367 bytes.
- CSS (lines 20-854 inside `<style>`): 835 lines, 49,467 bytes.
- JS (lines 949-5701 inside `<script>`): 4,753 lines, 233,627 bytes.
- Gzip of the whole page: 88,921 bytes at `gzip -9`, 106,878 at `gzip -1` (command-line gzip; the server uses
  `flate2` `Compression::best()` for the page, phone.rs:1309-1317, so the served gzipped size should be close to the
  `-9` figure but was not measured from the binary).
- `src/phone_sw.js`: 161 lines, 7,803 bytes; 3,176 bytes at `gzip -9`.
- Code comments citing "127 KB ... compresses to 37" (Cargo.toml flate2 comment; phone.rs:1260, 1306) are out of
  date relative to these measurements.

---

## What I could not determine
- The exact served (flate2) gzipped byte count of the page; only command-line gzip was measured.
- The full list of image formats `with_guessed_format()` will decode in this build (depends on `image` 0.25.10 default
  features; Cargo.toml adds only `avif`, which is an encoder feature).
- Whether "Use the desk's picture" works on any browser: by reading, `api()` labels the `' '` body
  `application/json` and the server returns 415 before reading `X-TaskDeck-Clear`; not exercised at runtime.
- Exact text of reqwest error strings in `FetchOutcome::Failed.why` (whether they always include the feed URL) — not
  run; reqwest's `Display` normally includes `for url (...)`.
- Contents of `build_day`, `build_tray`, `with_subscribed` layout logic beyond their output shapes (not needed for the
  wire, not read in full).
- Every caller of `render*` helpers in phone.html was not traced; DOM facts are from grep and the cited ranges.
