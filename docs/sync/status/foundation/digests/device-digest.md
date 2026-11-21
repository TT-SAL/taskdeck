# device-digest.md — today's runtime around the board (fact sheet)

Facts only, each with `file:line` (repository `/home/user/taskdeck`). Where docs and code differ the
code is stated and the difference noted.

---

## 1. Folders and locks

### 1.1 `AppDirs` (`src/paths.rs`)
- Constants: `DATA_DIR_NAME = "taskdeck_data"` (paths.rs:24), `LOCK_FILE_NAME = ".lock"` (paths.rs:26, private),
  `IMAGES_DIR_NAME = "images"` (paths.rs:28).
- `pub struct AppDirs { pub root: PathBuf, pub data: PathBuf, pub images: PathBuf }`, `#[derive(Debug, Clone)]`
  (paths.rs:123-131). `data = root.join("taskdeck_data")`, `images = root.join("images")`.
- `AppDirs::resolve()` (paths.rs:154-163): `resolve_root()`, then `fs::create_dir_all` on `data` and `images`,
  errors ignored (`let _ =`).
- `AppDirs::locate()` (paths.rs:171-174): same root, creates nothing. Used only by `taskdeck-server --print-link`
  (server_main.rs:169).
- `config_file()` -> `data.join("userconfig.toml")` (paths.rs:177-179).
- `image_path(name)` keeps only `Path::file_name()` of `name`, joins onto `images`; `None` when no file name
  (`""`, `".."`) (paths.rs:189-192). Tests paths.rs:330-349.
- `background_options()` -> every entry name in `images/` (no filtering by extension); unreadable dir -> empty
  `Vec` (paths.rs:197-205).
- `resolve_root()` order (paths.rs:208-232):
  1. env `TASKDECK_HOME` if set and non-empty (paths.rs:209-211).
  2. `cargo_project_root(exe_dir)`: exe dir's parent is named `target` and its parent has a `Cargo.toml` file
     (paths.rs:239-249).
  3. `exe_dir` if `exe_dir/taskdeck_data` or `exe_dir/images` is a directory (paths.rs:222-224).
  4. `exe_dir` if not inside a macOS bundle (`.../Contents/MacOS`, paths.rs:254-257) and writable — probed with
     `NamedTempFile::new_in(dir).is_ok()` (paths.rs:262-264) (paths.rs:226-228).
  5. `user_data_dir()` (paths.rs:231).
- `user_data_dir()` per OS:
  - macOS: `home_dir().join("Library/Application Support/TaskDeck")` (paths.rs:267-270).
  - Windows: `%APPDATA%` (non-empty) else `home_dir()`, then `.join("TaskDeck")` (paths.rs:273-280).
  - other unix: `$XDG_DATA_HOME` if absolute, else `home_dir()/.local/share`, then `.join("taskdeck")`
    (lower-case) (paths.rs:284-291).
- `home_dir()`: `USERPROFILE` on Windows, `HOME` elsewhere; empty/unset -> `PathBuf::from(".")` (paths.rs:293-305).
- DOCUMENTATION.md:131-150 describes the same order; it matches the code.

### 1.2 The lock (`claim_data_dir`, paths.rs:96-120)
- `pub struct DataClaim { _locked: Option<fs::File>, contended: bool }` (paths.rs:67-75).
- `claim_data_dir(data)`: `create_dir_all(data)` (ignored), opens `data/.lock` with
  `create(true).truncate(false).write(true)` (paths.rs:101-105).
  - Open fails -> `DataClaim { _locked: None, contended: false }` (no warning) (paths.rs:110).
  - `file.try_lock()` (std `File::try_lock`, exclusive):
    `Ok` -> `{ Some(file), false }`; `Err(TryLockError::WouldBlock)` -> `{ None, true }`;
    `Err(TryLockError::Error(_))` (fs does not lock) -> `{ None, false }` (paths.rs:113-119).
  - The lock file is never written; its content is empty.
- `is_contended()` (paths.rs:81-83); `warning()` text verbatim (paths.rs:86-93):
  `"Another TaskDeck is already using this taskdeck_data folder.\n\nBoth copies keep their own picture of your tasks and write all of it when anything changes, so whichever one saves last will replace what the other did. Close one of them."`
- What a contended lock does today:
  - Desktop (`main.rs`): claim held in `_data_claim` for the whole of `run()` (main.rs:35); if contended the
    warning is pushed to `startup_errors` (main.rs:43-45) and the app **continues**; shown in the startup error
    window (`startup_error`, main.rs:277-281).
  - Server (`server_main.rs`): claim taken after `--print-link` handling (server_main.rs:222); if contended it prints
    the warning and `"taskdeck-server: another TaskDeck has this data directory open; refusing to start."` to stderr
    and `exit(1)` (server_main.rs:223-230). `--print-link` takes no lock (server_main.rs:171-174).
- DOCUMENTATION.md §4.1 (DOCUMENTATION.md:174-216) states the same: desktop warned, server refuses.

### 1.3 First-contact set-aside and the replica marker (`src/sync.rs`)
- `pub const CLIENT_MARKER: &str = ".client-of";` content = server URL (sync.rs:219-221).
- `is_replica_of(data_dir, server_url)`: reads `.client-of`, compares `text.trim()` with
  `server_url.trim().trim_end_matches('/')`; unreadable -> `false` (sync.rs:224-228).
- `mark_replica_of(data_dir, server_url)`: **plain `fs::write`** (not atomic, no fsync) of
  `server_url.trim().trim_end_matches('/')` (sync.rs:231-233).
- `set_aside_local_board(data_dir) -> Result<Option<String>, String>` (sync.rs:243-290):
  - stamp `chrono::Local::now().format("%Y%m%d-%H%M%S")` (sync.rs:244).
  - Files in order, each with the "empty" content that is *not* set aside (sync.rs:246-253):
    `("read_at_startup.json", "[]")`, `("archived.jsonl", "")`, `("notepad_text.json", "\"\"")`,
    `(OUTBOX_FILE /* "outbox.json" */, "[]")`.
  - A missing/unreadable file is skipped; `text.trim()` empty or equal to `keep_if` is skipped (sync.rs:255-259).
  - Renamed to `<name>.local-<stamp>` (sync.rs:260-261). On a rename error every earlier rename is undone in
    reverse; error message `"could not set aside {name}: {error}"` plus per-file notes if an undo fails
    (sync.rs:261-276).
  - Not set aside: `subscriptions.json`, `subscribed_cache.json`, `colorschemes.json`, `userconfig.toml`, images.
  - Returns `Ok(None)` if nothing moved, else a long user message listing the new names (sync.rs:279-289).
- Use at startup (main.rs:112-170): when `server_url` is non-empty and `Remote::new` succeeds,
  `replica_already = is_replica_of(...)`; on a successful `remote.board()`: if not already a replica,
  `set_aside_local_board`; on `Err` -> `safe_to_replace = false`, run on local board, server not used
  (main.rs:126-134); on success `mark_replica_of` (main.rs:136-140). Then the replica is built from the server
  state and `board.save_all()` (main.rs:148-161).
  - `remote.board()` fails and `replica_already` -> local board + outbox + `sync::start(.., seen 0, online false)`
    (main.rs:172-186).
  - `remote.board()` fails and not a replica -> runs on its own board, no sync (main.rs:188-202).
- `forget_replica(data_dir) -> Result<Option<String>, String>` (sync.rs:296-330); called when `server_url` is empty
  (main.rs:86-90):
  - No readable `.client-of` -> `Ok(None)` (sync.rs:298).
  - Removes `.client-of`; error `"could not remove .client-of: {e}"` (sync.rs:299).
  - Note `"This copy runs on its own board again; it was a replica of {server}."` (sync.rs:301).
  - `outbox.json`: NotFound -> return note; parses as `Vec<Queued>` with length 0 -> removed; otherwise (edits or
    unreadable) renamed to `outbox.json.local-<stamp>` and the note says how many changes / that it could not be
    read (sync.rs:304-328).
  - Then `Board::open`, `seed_version(clock_version())`, `adopt_temporaries()` (main.rs:91-98).
- `Board::adopt_temporaries` renumbers ids `>= TEMPORARY_ID_FLOOR` from just past the highest real id and saves
  items if any changed (board.rs:697-711). `TEMPORARY_ID_FLOOR: u64 = 1 << 62` (board.rs:58);
  `sync::CLIENT_ID_FLOOR = TEMPORARY_ID_FLOOR` (sync.rs:74).

---

## 2. Atomic file helpers

All use `tempfile::NamedTempFile::new_in(<same dir>)`, write, `as_file_mut().sync_all()`, `persist(<final>)`
(an atomic rename), then — in all but one — `crate::tasks::sync_directory(dir)`.

| Function | File | Steps | Ref |
|---|---|---|---|
| `tasks::oversafe_activesave(&Vec<Active>, data_dir)` | `read_at_startup.json` | `create_dir_all`; `serde_json::to_string_pretty`; temp; `BufWriter` write_all + flush; `sync_all`; `persist`; `sync_directory` | tasks.rs:725-751 |
| `tasks::sync_directory(dir)` | — | unix: `File::open(dir)` then `sync_all()`, errors ignored; non-unix: no-op | tasks.rs:765-772 |
| `color::save_colorschemes(&HashMap<u32, ColorScheme>, data_dir)` | `colorschemes.json` | same as above, pretty JSON | color.rs:241-267 |
| `utilities::save_notepad_text(String, data_dir)` | `notepad_text.json` | same, pretty JSON of the string | utilities.rs:106-130 |
| `subscriptions::write_atomically(dir, name, text)` | `subscriptions.json` (`save`, pretty) and `subscribed_cache.json` (`save_overlay`, compact) | `create_dir_all`; temp `write_all`; `sync_all`; `persist`; `sync_directory` | subscriptions.rs:505-534 |
| `initialization::write_atomically(path, text)` | `userconfig.toml` | dir = parent or `"."`; temp `write_all`; `sync_all`; `persist(path).map_err(|e| e.error)`; `sync_directory` (no `create_dir_all`) | initialization.rs:368-378 |
| `phone::write_bytes_atomically(dir, name, bytes)` | `phone_background.img` | `create_dir_all`; temp; `sync_all`; `persist`; `sync_directory` | phone.rs:1797-1804 |
| `sync::Outbox::persist` | `outbox.json` | skipped if in-memory (empty path); pretty JSON of the queue; temp `write_all`; `sync_all`; `persist`; `sync_directory` | sync.rs:436-451 |
| `archive::ArchiveLog::rewrite(dir)` (via `take`, `persist`) | `archived.jsonl` | `create_dir_all`; body = unreadable lines first (verbatim, each + `\n`), then entries in `rev()` order (oldest first) one JSON per line; temp `BufWriter`; `sync_all`; `persist`; `sync_directory` | archive.rs:774-799 |
| `archive::append(dir, row)` (via `ArchiveLog::record`) | `archived.jsonl` | `create_dir_all`; serialize line first + `\n`; `OpenOptions::create(true).append(true)`; `write_all`; `file.sync_all()`; **no directory fsync** | archive.rs:802-817, 733-745 |
| `sync::mark_replica_of` | `.client-of` | plain `fs::write`, not atomic | sync.rs:231-233 |
| `tasks::read_at_startup` | seeds `read_at_startup.json` with `[]` via plain `fs::write` when missing | | tasks.rs:663-679 |

- Quarantine: `tasks::quarantine_corrupt_file(data_dir, file_name, cause)` renames to
  `<file_name>.corrupt-<%Y%m%d-%H%M%S>` (tasks.rs:705-723). Used for `read_at_startup.json` (board.rs:585-587) and
  `colorschemes.json` (main.rs:220-230). `subscriptions.json` unreadable is reported and *not* moved
  (board.rs:597-610); permission-denied `read_at_startup.json` is reported and not moved (board.rs:575-584).
- `Board` save wrappers bump `version` (wrapping) then save: `save_items` (board.rs:924-928),
  `save_subscriptions` (board.rs:930-934), `save_notes` (board.rs:936-940). `save_all` = items, subscriptions,
  notes, then archive rewrite if loaded (board.rs:716-730). `save_overlay` ignores errors (board.rs:734-736).

### Write order of `retire`, `Restore`, `ForgetArchived`
- `retire(id, outcome, now)` (board.rs:1475-1487), used by `Complete` (board.rs:1026) and `Delete`
  (board.rs:1033): 1) `Archived::retire(...)`; 2) `archive.record(...)` — **append to `archived.jsonl` first**;
  on failure `"Could not write to the archive, so nothing was changed:\n{error}"` and nothing removed;
  3) `items.remove(index)`; 4) `save_items()` (rewrites `read_at_startup.json`).
- `Command::Restore { id, archived_at }` (board.rs:1261-1304): 1) `load_archive`; 2) `archive.take(...)` —
  **rewrites `archived.jsonl` whole first** (archive.rs:754-769; on rewrite failure the in-memory row is put
  back); missing -> `BoardError::gone("That is no longer in the archive.")` (410); 3) id reuse unless `0`,
  `>= TEMPORARY_ID_FLOOR`, or taken (then `next_item_id()`); 4) `items.push`; 5) `save_items()`; if that fails,
  `archive.record` re-appends the row and pops the item; if the re-append also fails the item stays in memory
  and a combined message is returned (board.rs:1284-1302). Reply `{ id: Some(restored) }`.
- `Command::ForgetArchived { id, archived_at }` (board.rs:1306-1315): `load_archive`; `archive.take` (whole
  rewrite); missing -> 410 gone; `version` bumped; no other file written.

---

## 3. The outbox today (`src/sync.rs`)

### Constants (sync.rs:69-89)
- `OUTBOX_FILE = "outbox.json"` (69); `CLIENT_ID_FLOOR = board::TEMPORARY_ID_FLOOR` (= `1 << 62`) (74).
- `SEND_TIMEOUT = 8 s` (79); `RETRY_EVERY = 3 s` (300 ms under test) (83); `WAIT_TIMEOUT = 40 s` (85);
  `STARTUP_TIMEOUT = 4 s` (pub, 87); `LISTEN_BACKOFF_MAX = 30 s` (89). Listener initial backoff `2 s`
  (sync.rs:762), doubled up to 30 s (811, 818, 826). Pending-wait sleep `400 ms` (768).

### `Queued` and `Outbox`
- `pub struct Queued { pub command: Command, #[serde(default)] pub local_id: Option<u64>, #[serde(default)] pub key: String }`,
  derives `Serialize, Deserialize` (sync.rs:337-346). On disk: JSON array, pretty-printed:
  `[{"command": <Command JSON>, "local_id": <u64|null>, "key": "<token>"}]`.
- `Outbox { path: PathBuf, queue: VecDeque<Queued> }` (sync.rs:349-352).
- `Outbox::open(data_dir)` (sync.rs:357-379): read `outbox.json`; parse error -> rename to
  `outbox.json.corrupt-<stamp>` and message `"The outbox of unsent changes could not be read and was set aside as {path}:\n{error}"`;
  missing/unreadable -> empty.
- `in_memory()` -> empty path, never written (383-385).
- `push` (append + persist), `pop_front` (pop + persist), `remap(local, server)` (every queued command whose
  `item_id() == Some(local)` gets `set_item_id(server)`, persist) (412-432).
- `highest_temporary_id()` — max of `local_id` and `command.item_id()` over the queue, filtered `>= CLIENT_ID_FLOOR`
  (403-410); at startup `board.number_from(highest+1 or CLIENT_ID_FLOOR)` (main.rs:167, 184).
- Persistence: see §2 (sync.rs:436-451).

### The request key
- `SyncHandle::queue(command, local_id)` generates `key = crate::phone::generate_token()` per command
  (sync.rs:500-510), increments `status.pending` immediately (503), sends on the channel.
- `Remote::send(command, key)` (sync.rs:188-205): `POST {base}/api/command`, headers `X-TaskDeck-Token: <token>`,
  `Content-Type: application/json`, and if key non-empty `crate::phone::REQUEST_HEADER: <key>`; body
  `serde_json::to_vec(command)`. Reply parsed as `WireReply { version: u64, id: Option<u64>, session: Option<usize>, moved: Option<usize> }`
  (sync.rs:112-121). Key empty in pre-key outboxes -> sent without header (sync.rs:342-345).

### `Remote` (sync.rs:124-215)
- `Remote::new(base, token, timeout)`: trims, strips trailing `/`; must start `http://` or `https://`, else
  ``"The server address should start with http:// or https://, not `{base}`."``; `reqwest::blocking::Client::builder().timeout(timeout)`
  (no redirect/https-only settings) (sync.rs:133-143).
- `get(path)`: `GET {base}{path}` with `X-TaskDeck-Token` (149-157).
- `read(response)` (159-176): body must parse as JSON (else `Unreachable("unreadable answer: ...")`); 2xx -> body;
  other -> `Rejected { status, message: body["error"] or "the server refused" }`.
- `board()` -> `GET /api/board` -> `BoardState` (179-182); `wait(seen)` -> `GET /api/wait?version={seen}` -> `version`
  (208-214).
- `RemoteError::{Unreachable(String), Rejected { status: u16, message: String }}`; Display
  `"server unreachable: {why}"` / `"{message} ({status})"` (sync.rs:94-109).

### Sender error mapping (`sender`, sync.rs:631-729)
- Thread `"taskdeck-sync-send"` (590). Rebuilds `Remote` with `SEND_TIMEOUT` (640-643).
- Loop: `rx.recv_timeout(RETRY_EVERY)`; files everything received (re-pointed via `known` map of answered temp
  ids, `file()` sync.rs:733-738); breaks if channel disconnected or `quit` set (652-665). Then flushes the front:
  - `Ok(reply)`: `acked_version = reply.version` (not max, 676); if `local_id` and `reply.id` differ -> `known.insert`,
    `outbox.remap`, `SyncEvent::Remapped { local, server }`, wake (678-685); `pop_front`; `pending -= 1`; online.
  - `Rejected 401 | 403`: **kept**; offline; error `"{message} ({code}) — check the server key in Settings"`; stop
    this pass (690-696).
  - `Rejected 500..=599`: **kept**; offline; `"{message} ({code})"`; stop pass (697-705).
  - `Rejected` any other status (e.g. 400, 404, 409, 410, 413, 429...): **dropped** (`pop_front`), pending -1,
    `SyncEvent::Rejected { what: describe(command), message: "{message} ({code})" }`, wake, online (706-716).
  - `Unreachable(why)`: **kept**; offline; stop pass (717-720).
- On exit drains the channel into the outbox (726-728).
- `describe(command)` strings (sync.rs:536-566).
- `note_storage` sets `status.storage_error = "The outbox could not be saved: {error}"` until a write succeeds
  (744-748).

### `Status`, `SyncEvent`, `SyncHandle`
- `Status { online, pending, last_error: Option<String>, storage_error: Option<String>, acked_version: u64 }`
  (sync.rs:457-469).
- `SyncEvent::{Board(BoardState), Remapped { local, server }, Rejected { what, message }, Online(bool)}` (472-483).
- `SyncHandle::shutdown()`: sets `quit`, drops the sender channel, joins the sender thread (waits for at most one
  send in flight) (516-524).
- `start(remote, outbox, wake, seen, online)` spawns both threads (571-611).

### Listener (`listener`, sync.rs:757-842)
- Thread `"taskdeck-sync-listen"` (598). Rebuilds `Remote` with `WAIT_TIMEOUT` 40 s (758-761).
- While `pending > 0`: sleep 400 ms, no request (766-770).
- `wait(seen)` Ok: online; `version < seen` -> `seen = 0`, `acked_version = 0` (775-784); `version == seen` -> loop;
  else `board()`; delivered only if `pending == 0 && state.version >= acked` -> `seen = state.version`,
  `SyncEvent::Board`, wake (789-801); board error -> offline + backoff (808-812).
- `wait` Unreachable -> offline + backoff; `Rejected 500..=599` -> offline + backoff; other `Rejected` -> offline, message
  (with the Settings hint for 401/403), sleep `LISTEN_BACKOFF_MAX` (815-839).
- UI side: `drain_sync_events` (ui.rs:2770-2844) skips `Board` while `pending > 0` (ui.rs:2781-2783); otherwise `board.replace(...)`, then `save_all()` if the archive differs (length or first key) else `save_items_and_notes()`, keeps local notes while a notes save is pending (ui.rs:2798-2806); `Remapped` renumbers the item and follows UI selections (ui.rs:2811-2836); `Rejected` -> error window `"A change made here could not be applied on the server and was dropped — {what}:\n{message}"` (ui.rs:2837-2841); `Online` ignored (ui.rs:2842).

---

## 4. ICS fetching (`src/subscriptions.rs`)

### Constants (subscriptions.rs:53-103)
- `SUBSCRIPTIONS_FILE = "subscriptions.json"` (53); `OVERLAY_FILE = "subscribed_cache.json"` (56).
- `DEFAULT_COLORS: [[u8;4];6]` = `[90,140,200,255]`, `[200,120,90,255]`, `[120,175,110,255]`, `[175,120,190,255]`,
  `[200,175,90,255]`, `[110,175,185,255]` (60-67).
- `SUBSCRIPTIONS_MAX = 20` (72); `SUMMARY_MAX_CHARS = 200` (75); `EVENTS_MAX = 5_000` (79);
  `LOCATION_MAX_CHARS = 120` (82); `DESCRIPTION_MAX_CHARS = 300` (85); `WHY_MAX_CHARS = 200` (88).
- `MAX_ICS_BYTES = 8 * 1024 * 1024` (93); `FETCH_TIMEOUT = 20 s` (95); `REFRESH_EVERY = 600 s` (200 ms under test) (97).
- `WINDOW_BACK_DAYS = 60`, `WINDOW_FORWARD_DAYS = 400` (102-103).
- `ics::Limits::default()`: `max_depth 8, max_lines 200_000, max_events 20_000, max_occurrences_per_event 1_000,
  max_occurrences 50_000, max_rule_steps 100_000` (ics.rs:126-137). `ics::parse(text, from, until, limits)` (ics.rs:182).

### HTTP client (`run`, subscriptions.rs:730-751)
- `reqwest::blocking::Client::builder().timeout(FETCH_TIMEOUT).redirect(reqwest::redirect::Policy::limited(3)).https_only(true).user_agent(concat!("TaskDeck/", env!("CARGO_PKG_VERSION")))`.
- Build failure -> `eprintln!("taskdeck: no calendar client ({error}); subscriptions are off this run")`, thread returns.
- Request (`fetch_one`, subscriptions.rs:545-576): `GET url` with `Accept: text/calendar, text/plain;q=0.9, */*;q=0.5`.
  Non-2xx -> `"the calendar answered {status}"`. `Content-Length > MAX_ICS_BYTES` -> `"that calendar is too large to read"`.
  Body read with `.take(MAX_ICS_BYTES + 1)`; longer -> same message. `String::from_utf8_lossy`, then `ics::parse`.
  Send/read errors -> `shorten(&error.to_string())` (first 200 chars) — **this is reqwest's error Display, which
  normally includes the request URL (the credential)**; `shorten` only truncates (578-580). These `why` strings are
  stored in `FetchOutcome::Failed { why }` in the overlay status, which is persisted in `subscribed_cache.json` and
  sent to clients in `BoardState.overlay`.

### `checked_url` (subscriptions.rs:156-180)
- Trim; empty -> `"An address is needed."`; > 2000 chars -> `"That address is too long."`; `webcal://` rewritten to
  `https://`; `http://` -> `"Use https — a calendar link is a password, and http sends it in clear."`; not `https://`
  -> `"An address should start with https://"`; host (up to `/ ? #`) empty or without `.` ->
  `"That address has no host in it."`. Called in `Command::AddSubscription` (board.rs:1327). Duplicate url ->
  `"That calendar is already subscribed to."` (board.rs:1328-1330).

### Cadence and window
- `fetch_all` (593-639): window `from = today - 60 d`, `until = today + 400 d` (Local date). Only `enabled`
  subscriptions. Per subscription `at = Local::now()`. Ok -> events mapped to `OverlayEvent`
  (`summary = without_repeated_location(name, location)`), `FetchOutcome::Ok { events: count, problems, title: parsed.name }`.
  Err -> previous events of that subscription carried forward, `FetchOutcome::Failed { why }`.
  `covers = Some(Covered{from, until})` only if at least one read succeeded, else `previous.covers` (637).
- Service thread `"taskdeck-calendars"` (`start`, 715-728): `Feeds { overlay: Arc<RwLock<Overlay>>, pub version: Arc<AtomicU64>, tx }`.
  Loop (753-791): if any enabled -> `fetch_all`; if digest or status differ -> write, `version += 1`, `wake()`.
  Then `recv_timeout(REFRESH_EVERY)`: `Subscriptions(list)` -> forget removed/disabled/changed-url subscriptions' events,
  replace list, `version += 1`, wake, and loop (fetch immediately); `Stop` or disconnected -> exit. `Drop for Feeds`
  sends `Stop` (708-712).
- Who starts it: desktop only when not a client (`sync_handle.is_none()`, main.rs:212-214); server always
  (server_main.rs:286-287, wake is a no-op). Desktop pushes list changes after any command that touches calendars
  (ui.rs:2720-2724); server after each overlay version change (server_main.rs:494).

### What is cached on disk and how
- `subscriptions.json`: pretty JSON array of `Subscription { id: u64, name: String, url: String, color: [u8;4], enabled: bool }`
  (subscriptions.rs:137-147, 505-507). Read: missing or blank -> empty list (490-500).
- `subscribed_cache.json`: compact JSON of `Overlay` (523-525), written via `Board::save_overlay` only when
  `adopt_overlay` returns true (events digest changed) (ui.rs:2748-2749; server_main.rs:478-479). Read at
  `Board::open` (board.rs:613) via `read_overlay` (511-518): unreadable -> `None` -> default; re-sealed and
  `covering(overlay.covers)`.

### Overlay types
- `OverlayEvent { subscription: u64, day: NaiveDate, start: i32, end: i32, #[serde(default)] all_day: bool, #[serde(default)] free: bool, summary: String, #[serde(default)] location: String, #[serde(default)] description: String }`
  (190-210). `LONG_EVENT_MINUTES = 12*60` (233); `is_background` = all_day or minutes >= 720 (247-249);
  `is_busy` = !background && !free && end > start (256-258).
- `FetchOutcome` — `#[serde(tag = "state", rename_all = "snake_case")]`: `Ok { events: usize, problems: Vec<String>, #[serde(default)] title: Option<String> }`,
  `Failed { why: String }` (261-277). JSON e.g. `{"state":"ok","events":3,"problems":[],"title":"..."}`,
  `{"state":"failed","why":"..."}`.
- `FetchStatus { subscription: u64, at: DateTime<Local>, outcome: FetchOutcome }` (336-341).
- `Overlay { #[serde(default)] events: Vec<OverlayEvent>, #[serde(default)] status: Vec<FetchStatus>, #[serde(default, skip_serializing_if = "Option::is_none")] covers: Option<Covered> }`
  (347-368). `Covered { from: NaiveDate, until: NaiveDate }`, `holds(day)` inclusive (116-127).
- `Overlay::sealed(events, status)` (392-422): drops `end < start`, clamps start/end to `0..=planner::DAY_MINUTES`,
  truncates summary/location/description, `take(EVENTS_MAX)` (before sort), sorts by
  `(day, start, end, subscription, summary)`, truncates `why` to 200 chars, `covers: None`.
- `digest()` (431-450): FNV-1a 64 (`FNV_OFFSET = 0xcbf2_9ce4_8422_2325`, `FNV_PRIME = 0x0000_0100_0000_01b3`, 370-379)
  over each event's `subscription` (LE bytes), `day` as `%Y%m%d`, `start`, `end` (LE), `[all_day, free]`, `summary`,
  `location`, `description`. **Excludes `status` and `covers`.**
- `Board::adopt_overlay(fresh)` (board.rs:798-816): same digest -> keep events, copy `status` and `covers`, return
  `false` (version not bumped); else replace and bump version, return `true`.
- `anchors_on(day)` = busy events only (460-466); `forget(subscription)` removes events and status (476-479).

### Auto-rename from the feed's own name
- `placeholder_name(id) = format!("Calendar {id}")` (283-285); `name_from_url` = host without userinfo, port, `www.`
  (294-302); `is_unnamed` = name equals placeholder or `name_from_url(url)` (331-334);
  `better_name(sub, overlay)` = feed's `title` (trimmed, non-empty) from `FetchOutcome::Ok`, else `name_from_url`; only if
  unnamed and different (312-324).
- Desktop: `adopt_calendar_overlay` (ui.rs:2737-2765): after adopting, for every subscription with a `better_name`
  calls `apply_or_report(Command::RenameSubscription { id, name })` (ui.rs:2755-2764) — so on a desktop that
  owns the board this goes through `apply` (and its phone pulse). On a client there is no `calendars`, so this
  function returns early (ui.rs:2738).
- Server: same loop with `board.apply(RenameSubscription ...)` and `pulse.publish` (server_main.rs:482-493).
- `AddSubscription` with empty name uses `name_from_url(&url)` or `placeholder_name(id)` (board.rs:1333-1338).

### Quarantine of a panicking or bad feed
- None exists. No `catch_unwind` anywhere in subscriptions.rs / ics.rs. A feed that fails to fetch or parse only
  produces `FetchOutcome::Failed` for that subscription and keeps its previous events (593-639); it is retried on the
  next 10-minute tick. In release builds any panic aborts the process (`panic_abort::abort_on_panic`, panic_abort.rs:12-19,
  called first in both `main`s: main.rs:13-14, server_main.rs:146-147). ics.rs is documented as unable to panic on input
  (subscriptions.rs:543-544).

---

## 5. Weather (`src/weather.rs`)

### The request (`fetch_weather_once`, weather.rs:287-327)
- URL built with `format!` (weather.rs:291-299), verbatim:
  `https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,apparent_temperature,weather_code,is_day,wind_speed_10m,wind_gusts_10m,wind_direction_10m,relative_humidity_2m,precipitation&hourly=temperature_2m,apparent_temperature,weather_code,is_day,precipitation_probability,precipitation,wind_speed_10m,wind_gusts_10m,wind_direction_10m,uv_index,relative_humidity_2m&daily=weather_code,temperature_2m_max,temperature_2m_min,apparent_temperature_max,apparent_temperature_min,precipitation_sum,precipitation_probability_max,wind_speed_10m_max,wind_gusts_10m_max,uv_index_max,sunrise,sunset&wind_speed_unit=ms&timezone=auto&forecast_days={}`
  with `coordinates[0]`, `coordinates[1]`, `FORECAST_DAYS`.
- Coordinate precision: `[f32; 2]` formatted with `{}` (Rust `Display` for f32 = shortest round-trip
  representation; no rounding applied) (weather.rs:298). Source: `coordinates` from config (`[f32;2]`, default
  `[0.0, 0.0]`, initialization.rs:280-296); written back as two f64 TOML floats (`utilities::float_pair_array`,
  utilities.rs:15-20).
- `FORECAST_DAYS: u32 = 7` (weather.rs:430).
- Header `User-Agent: egui-weather-app` (weather.rs:303); `.error_for_status()` (305). Client:
  `Client::builder().timeout(Duration::from_secs(10))` — no redirect/https-only settings (weather.rs:445-448).
- Parsed into private `WeatherResponse { hourly, current?, daily?, timezone, utc_offset_seconds }` (weather.rs:34-124).
  Desktop grid: `Vec<Vec<WeatherData>>` of 24 buckets by `i % 24`, time formatted `%H:%M` (weather.rs:310-324).

### Cadence (`get_weather`, weather.rs:432-514)
- `REFRESH_INTERVAL = 600 s`, `MAX_RETRIES = 3` (433-434). Per round up to 3 attempts, sleeping `2^attempt` s
  (1, 2, 4 s) after each failure (488-489); on total failure keeps the old data (494-496).
- Waits `recv_timeout(600 s)`; `SetCoordinates(c)` -> immediate refetch with new coordinates (`continue`);
  `Stop` -> exit; `Drop for WeatherService` sends `Stop` (274-278, 498-505).
- Thread is unnamed (`thread::spawn`, weather.rs:444).
- Desktop: started in `main` with config coordinates and the UI wake (main.rs:275); `set_coordinates` when the user
  picks a place, which also persists `coordinates` (ui.rs:6796-6797). Frame loop compares
  `weather_service.version` and rebuilds its cache (ui.rs:7022-7026).
- Server: started with config coordinates and a no-op wake; handle kept in `_weather` (server_main.rs:344).

### Report type (served as `/api/weather`)
- `Report { place: String, latitude: f32, longitude: f32, timezone: String, utc_offset_seconds: i64, fetched_at: u64 /* unix secs */, now: Option<Now>, hours: Vec<Hour>, days: Vec<Day> }`
  (weather.rs:219-235).
- `Hour { t, temp, feels, code, sym, day, pop, mm, wind, gust, dir, uv, humidity }` (148-178);
  `Day { date, code, sym, high, low, feels_high, feels_low, mm, pop, wind, gust, uv, sunrise, sunset }` (180-202);
  `Now { temp, feels, code, sym, day, wind, gust, dir, humidity, mm }` (204-217). `sym` = `sky_symbol(code, is_day)`
  (weather.rs:631); days always use the daylight symbol (372). `place` = `nearest_city(lat, lon)` name or `""`
  (407-409).
- Where held: process-global `static REPORT: Mutex<Option<Arc<Report>>>` (weather.rs:244); `latest_report()`
  (249-251), `publish_report()` (253-255) — set before `version += 1` and `wake()` (467-472). Never cleared once set.
- Served bytes cached per report Arc in `Encoding::report` (`static PACKED`, phone.rs:1330-1348).
- The phone page may also call Open-Meteo itself for its own place (see bridge digest; CSP allows
  `connect-src ... https://api.open-meteo.com`, phone.rs:1216).

---

## 6. Images

### Backdrop constants (`src/phone.rs`)
- `BACKGROUND_TALL = 1560`, `BACKGROUND_WIDE = 720` (phone.rs:1423-1425); `BACKGROUND_QUALITY = 55` (JPEG, 1462);
  `AVIF_QUALITY = 82`, `AVIF_SPEED = 4` (1467-1468); `MAX_UPLOAD_BYTES = 6 * 1024 * 1024` (1517);
  `MAX_UPLOAD_PIXELS = 40 * 1_000_000` (1520); `BACKGROUND_FILE = "phone_background.img"` (1707).
- Crate: `image = { version = "0.25", features = ["avif"] }` (Cargo.toml), locked `0.25.10` (Cargo.lock:1558-1559).

### Decode (`prepare_backdrop_bytes`, phone.rs:1523-1667)
- Refuses empty or `> MAX_UPLOAD_BYTES` (1525-1527).
- `image::Limits`: `max_image_width = 20_000`, `max_image_height = 20_000`, `max_alloc = 40_000_000 * 4` bytes
  (1530-1533).
- `ImageReader::new(Cursor::new(bytes)).with_guessed_format()` then `.limits(limits)` and `.decode()` (1534-1536):
  the format is guessed from the magic bytes; which formats decode is whatever `image` 0.25 default features
  enable (not restricted in this repo).
- `prepare_backdrop(path)` = `fs::read` then the same (1508-1511).

### Crop, process, encode
- Centre crop to aspect 720/1560 (1542-1549), `resize_exact(720, 1560, Lanczos3)` (1550).
- Blur sigma = `LookDials::blur_for(720)` = `720 * min(blur_percent,100) * 0.0002`; applied only if `>= 0.1`
  (1446-1448, 1559-1564).
- Tone map in linear light: `lin / (1 + 4 lin)` per channel, then a scale so the 99.9th-percentile luminance is
  `<= ceiling` where `ceiling = max(min(light_percent,100)/300, 0.01)` (1454-1456, 1579-1598); only darkens.
- JPEG encode at quality 55, re-decoded and corrected up to 4 passes (1603-1626).
- `top` = average colour of the top 1/8 as `#rrggbb` (1630-1638, 1665).
- AVIF: `AvifEncoder::new_with_speed_quality(&mut out, 4, 82)`; `None` on failure or empty (1643-1651).
- `id` = FNV-1a 64 over the JPEG bytes, `format!("{hash:016x}")` (1656-1662).
- `pub struct Backdrop { pub id: String, pub avif: Option<Vec<u8>>, pub jpeg: Vec<u8>, pub top: String }` (1486-1501).

### Storage and state
- Process-globals: `static BACKDROP: Mutex<Option<Arc<Backdrop>>>` (1675); `static BACKDROP_HOME: OnceLock<BackdropHome>`
  with `{ data_dir, config_file, desk: Option<PathBuf> }` (1679-1688) — a `OnceLock`, so only the **first**
  `set_backdrop_home` call takes effect (1724); `static LOOK: Mutex<LookDials>` initialised to
  `BACKGROUND_BLUR_DEFAULT` (11) / `BACKGROUND_LIGHT_DEFAULT` (39) (1693-1696; initialization.rs:162-164).
- Upload (`adopt_background`, 1756-1778): empty body -> delete `phone_background.img`, re-prepare the desk picture
  (or none), return its id; otherwise prepare (`"That file is not a picture this can read."` on failure), then
  write the **received bytes unmodified** with `write_bytes_atomically(data_dir, "phone_background.img", body)`
  (`"Could not keep the picture: {error}"`), then publish.
- Look dials (`/api/look`, 840-851): sets `LOOK`, `remember_dials` writes `background_blur_percent` and
  `background_light_percent` to `userconfig.toml` via `write_config_value` (1789-1794), `remake_backdrop()` (1729-1746)
  keeps the old picture if a remake fails.
- Startup — server: `set_backdrop_home(data, config_file, desk_picture, dials)`; `phone_background.img` if it exists
  wins over the desk's `background` (from `images/`) (server_main.rs:293-332). Desktop: in `tend_phone_server`
  (ui.rs:3393-3410), each start attempt: `set_backdrop_home(...)` and, if no backdrop yet, prepare
  `phone_background.img` or the selected background.
- Served at `GET /bg-<id>.jpg` (auth required): AVIF if the request `Accept` contains `image/avif` and AVIF exists,
  else JPEG; `Vary: Accept`; `Cache-Control: private, max-age=31536000, immutable`; unknown id -> 404
  `"No such picture."` (phone.rs:796-814, 1238).
- Snapshot carries `look: { id, aspect: 720/1560, top }` when a backdrop exists and `dials: { blur, light }` always
  (phone.rs:258-270, 2099-2112).

### `color.rs`
- `ColorScheme { name: String, colors: [[u8;4];6], is_user_configurable: bool }` (color.rs:11-16).
  `RAMP_ALPHA = [74, 88, 104, 120, 136]`, `EVENT_ALPHA = 104` (24-25). Built-ins at ids 0.. :
  `"COLORSCHEME ZERO"` (all transparent, color.rs:38), `"EMBER"`, `"MINE"`, ... (`builtin_schemes`, 61 ff.).
- `install_builtins` runs every start; a user scheme on a reserved id is moved to a free id and the selection
  follows (189-236; main.rs:237-249).
- `read_colorschemes` seeds `{}` if missing via plain `fs::write` (271-287). File `colorschemes.json` = JSON object
  `{"<u32 id>": {"name": ..., "colors": [[r,g,b,a] x6], "is_user_configurable": bool}}`.
- `generate_colorscheme(dirs, name)` (289-383): `dirs.image_path(name)`, `image::load_from_memory` (no limits),
  resize 200x200 Triangle, Lab k-means (6 clusters, 20 iterations, 0.002, seed 42), needs >= 500 opaque pixels,
  alpha 80, name `Scheme from "{name}"`.
- The server reads the selected scheme's colours as the phone palette, default `[[0;4];6]` (server_main.rs:278-281).

---

## 7. Random values and tokens (`src/phone.rs`)
- `TOKEN_LENGTH = 32` (phone.rs:154); `TOKEN_ALPHABET = b"abcdefghijkmnpqrstuvwxyz23456789"` (32 symbols, no
  `l`, `o`, `0`, `1`) (156).
- `generate_token()` = `random_bytes(32)` each mapped `ALPHABET[byte % 32]` (1038-1043) — 5 bits per char, 160 bits.
- `random_bytes(n)` (1049-1077): on `cfg(unix)` reads `/dev/urandom` with `read_exact`; on failure or non-unix,
  fallback: loop of `RandomState::new().build_hasher()` fed `counter: u64`, `SystemTime::now()` nanos (u128) and
  `process::id()`, 8 bytes of `finish()` per round, truncated. Documented as not a CSPRNG (1045-1048). **Windows
  always uses the fallback.**
- Uses: `phone_token` minting (main.rs:51-58; server_main.rs:246-254; renew in ui.rs `renew_phone_token`), and the
  outbox request key per command (sync.rs:501).
- `same_token(a, b)` constant-time compare (phone.rs:1029-1034).
- `encode_token` percent-encodes non-unreserved bytes (phone.rs:1938-1947).

---

## 8. Configuration (`taskdeck_data/userconfig.toml`, `src/initialization.rs`)

Reading: `read_config` creates the file empty if unreadable (initialization.rs:45-57); `parse_config_text` flattens TOML
(dotted keys for tables) or falls back to line parsing (60-135). `get_check_and_set_config` = read, `config_from`,
`write_normalized_config` (rewrites every owned key via `toml_edit`, keeps comments/unknown keys, atomic) (227-232,
405-436). `read_config_only` writes nothing (237-240). Bools via `parse_config_bool`: `true|1|yes|on`
case-insensitive (141-143). Single-key writes: `write_config_value(path, key, value)` with `toml_edit`, atomic (387-396).

| Key | Type | Default / clamp | Meaning | Scope | Ref |
|---|---|---|---|---|---|
| `start_in_fullscreen` | bool | `false` | window starts fullscreen | per-device | 265-268 |
| `coordinates` | `[f32, f32]` | `[0.0, 0.0]`; both finite | weather location (lat, lon) | per-device today (both binaries read their own; phone page shows it) | 280-296 |
| `background` | string | `""` | file name in `images/` for the desk background; also the phone's fallback backdrop | per-device | 277-279 |
| `enable_fps_counter` | bool | `false` | FPS overlay | per-device | 269-272 |
| `window_size_startup` | `[f32, f32]` | `[1280.0, 720.0]`; finite and >= 200 | initial window size (never written at runtime) | per-device | 245-264 |
| `calendar_weeks_to_show` | usize | `100`, clamp 6..=520 | calendar length | per-device | 297-300, 149-150 |
| `selected_monitor_name` | string | `""` | monitor to open on | per-device | 310-312 |
| `selected_colorscheme_id` | u32 | `0`, clamp 0..=200000 | selected scheme in `colorschemes.json`; server uses it for the phone palette | board-like (affects phone palette) | 313-316 |
| `three_day_weather` | bool | `false` | desktop weather column shows three days | per-device | 273-276 |
| `background_image_tint_percent` | u32 | `30`, clamp 1..=100 | desktop tint over background | per-device | 301-304 |
| `background_blur_percent` | u32 | `11`, `min(100)` | blur dial, desktop and phone backdrop; written by `/api/look` | board-like (set from the phone, shared with desk) | 330-334, phone.rs:1792 |
| `background_light_percent` | u32 | `39`, `min(100)` | phone backdrop luminance ceiling; written by `/api/look` | board-like | 335-339, phone.rs:1793 |
| `ui_scale_percent` | u32 | `0` (auto); else clamp 40..=100 | UI scale | per-device | 305-309, 160-179 |
| `phone_server_enabled` | bool | `false` | desktop serves the phone view (server binary ignores it) | per-device | 317-320 |
| `phone_server_port` | u16 | `7373` (`phone::DEFAULT_PORT`); must be >= 1024 | listen port | per-device | 321-325 |
| `phone_token` | string | `""` then minted once and written | the phone/feed secret; server's API key | per-device (per host) | 326-329; main.rs:51-58 |
| `phone_bind_address` | string (IP) | `"127.0.0.1"` (`phone::DEFAULT_BIND`); non-IP -> default | listen address; not on the settings sheet | per-device | 340-343, 184-189 |
| `phone_public_url` | string | `""`; must be `http(s)://host.with.dot`, trailing `/` trimmed, else `""` | address to hand out in links/QR | per-device | 344-347, 198-206 |
| `frame_cap_fps` | u32 | `0` (uncapped); else clamp 15..=360 | render cap | per-device | 348-352, 210-225 |
| `server_url` | string | `""` | `taskdeck-server` URL; non-empty makes the desktop a replica | per-device | 353-356 |
| `server_token` | string | `""` | key sent as `X-TaskDeck-Token` to the server | per-device | 357-360 |

- `Config` struct (initialization.rs:438-495). Desktop destructures all fields, ignores `phone_public_url`
  (main.rs:37).
- Runtime writers on the desktop (`persist_config_value`, ui.rs:6268-6272) touch: `background`,
  `background_image_tint_percent`, `calendar_weeks_to_show`, `coordinates`, `enable_fps_counter`, `frame_cap_fps`,
  `phone_server_enabled`, `phone_server_port`, `phone_token`, `selected_colorscheme_id`, `selected_monitor_name`,
  `server_token`, `server_url`, `start_in_fullscreen`, `three_day_weather`, `ui_scale_percent` (grep of ui.rs).
- `colorschemes.json`: see §6.

---

## 9. Threads and wake-ups

| Thread | Name | Where started | What it does |
|---|---|---|---|
| UI / event loop (desktop) | main | main.rs:302 | winit `run_app`; serves phone requests in `user_event` and each frame |
| Server main loop | main | server_main.rs:445-497 | `rx.recv_timeout(2 s)`; queries via `answer_query`, commands via `board.apply` then `pulse.publish`; polls calendars version |
| Phone workers | `taskdeck-phone-0`, `taskdeck-phone-1` (`WORKERS = 2`) | phone.rs:653-683 | `server.recv()` -> `handle()` |
| Phone pulse | `taskdeck-phone-pulse` | phone.rs:646-649 | `answer_parked`: answers parked `/api/wait` |
| Calendars | `taskdeck-calendars` | subscriptions.rs:722-725 | ICS fetch every 600 s |
| Weather | unnamed | weather.rs:444 | Open-Meteo every 600 s |
| Sync sender | `taskdeck-sync-send` | sync.rs:589-592 | outbox flush |
| Sync listener | `taskdeck-sync-listen` | sync.rs:597-600 | `/api/wait` long poll on the server |

- `pub type Wake = Arc<dyn Fn() + Send + Sync>;` (phone.rs:65). Desktop wake = `proxy.send_event(())` on the winit
  `EventLoopProxy` (main.rs:67-72; also rebuilt per phone server start, ui.rs:3414-3417). Server uses `Arc::new(|| {})`
  everywhere (server_main.rs:287, 344, 349).
- How the desktop UI learns of changes: `App::user_event` -> `TaskApp::serve_phone_requests()` and two
  `request_redraw()` (initialization.rs:1142-1150). `serve_phone_requests` = `drain_sync_events`,
  `adopt_calendar_overlay`, `tend_phone_server`, then executes queued `PhoneRequest`s (ui.rs:3355-3363). The frame
  also calls it (ui.rs:7037) and polls `weather_service.version` (ui.rs:7022-7026). Atomic `version` counters:
  `Feeds.version`, `WeatherService.version`.
- `Pulse` (phone.rs:415-587): `publish(version)`, `interrupt()`, `park(request, seen, deadline)`. Desktop publishes
  after every `apply` (`phone_changed`, ui.rs:3336-3338) and on a colour-scheme repaint (`board.touch()`,
  ui.rs:3342-3345). `PhoneServer::start` publishes the board version first (phone.rs:633).
- Desktop `apply` (ui.rs:2696-2728): `command.stamped(now)`; keep a clone if a client; `board.apply`;
  `summarize_calendar` unless `SetNotes`; `phone_changed`; ghosts if archive touched; push subscriptions to
  `calendars` if touched; `sync.queue(command, created id)` on success.

---

## 10. Process entry points

### Desktop `TaskDeck` (`src/main.rs`)
- Global allocator `MiMalloc` (main.rs:9-10). Release builds: `panic_abort::abort_on_panic()` first (main.rs:13-14;
  hook prints then `std::process::abort()`, panic_abort.rs:12-19). Profile is `panic = "unwind"` (Cargo.toml).
- `pollster::block_on(run())` (17). No command-line flags parsed. `restart_self` re-spawns
  `current_exe()` with the same args, then `flush_pending_saves`, `shutdown_sync`, `exit(0)` (ui.rs:6850-6869).
- Order in `run()`: `EventLoop::new`, `ControlFlow::Wait` (22-25); `AppDirs::resolve()` (30); `claim_data_dir`
  (35); config `get_check_and_set_config` (37); contended warning (43-45); mint `phone_token` if empty (51-58);
  phone channel (63); `wake` (67-72); board (local / replica branches, §1.3) (80-206); calendars service only if not a
  client (212-214); background options; `read_colorschemes` (quarantine on error) (220-230); `install_builtins` + save
  + write `selected_colorscheme_id` (237-249); invalid selection -> 0 (254-256); `TaskApp::new`, `summarize_calendar`,
  `App::new`, `run_app` (295-302).
- Version seed: `clock_version()` = `Utc::now().timestamp_millis()` (main.rs:307-309), applied in every branch.
- Single instance: warned, not stopped (§1.2).
- Shutdown: `ApplicationHandler::exiting` -> `flush_pending_saves()` (saves pending notepad via
  `Command::SetNotes`) then `shutdown_sync()` (joins the sync sender after it files what it holds)
  (initialization.rs:1153-1156; ui.rs:6886-6898). `CloseRequested` -> `event_loop.exit()` (initialization.rs:1049-1054).
  Notepad otherwise autosaves 2 s after the last edit (ui.rs:7039-7050). Nothing runs on a hard kill or panic.
- The phone server (`PhoneServer`) is started/stopped by `tend_phone_server` per `phone_server_enabled`
  (ui.rs:3370-3412); dropping it sets `stopping`, `pulse.interrupt()`, `unblock()` per worker (phone.rs:689-703).

### Server `taskdeck-server` (`src/server_main.rs`)
- `USAGE` text (server_main.rs:32-49). Flags (`parse_args`, 89-121): `--help`/`-h`, `--version`/`-V`
  (prints `taskdeck-server {CARGO_PKG_VERSION} (built {BUILD_DATE})`), `--print-link`, `--bind <IP>`,
  `--port <N>` (N >= `PORT_MIN` 1024). Errors -> stderr, `exit(2)` (148-154). Last repeated flag wins.
- `--print-link`: `AppDirs::locate()`, `read_config_only`, prints `data:`, `zone:`, `key:`/`phone:`/`feed:` lines and a
  QR when stdout and stderr are terminals and `NO_COLOR` unset (`show_qr`, 140-143); no lock, no writes (169-220).
- Run: `AppDirs::resolve()`; `claim_data_dir` -> contended = `exit(1)` (222-230); `Board::unreadable_files` non-empty
  -> `exit(1)` (235-243); config; mint token if empty, `exit(1)` if it cannot be saved (245-254); port/bind overrides
  (255-256); `Board::open`, problems to stderr (258-261); `seed_version(Utc millis)` (264); `adopt_temporaries`,
  `exit(1)` on error (267-274); palette (278-281); calendars (286-287); backdrop (293-332); weather (344); `Pulse`,
  `PhoneServer::start(bind, port, token, tx, no-op wake, pulse, board.version())`, `exit(1)` on error (346-364);
  startup lines to stderr (366-439); main loop (445-497).
- No signal handling; relies on atomic saves for SIGTERM (comment server_main.rs:441-443). No shutdown save step.

---

## What I could not determine
- Whether reqwest's error text stored in `FetchOutcome::Failed { why }` always contains the subscription URL; by
  reqwest's usual `Display` ("error sending request for url (...)") it does, but this was not run. `shorten` truncates
  to 200 chars and does not redact (subscriptions.rs:578-580).
- Which image formats `ImageReader::with_guessed_format()` decodes in this build: `image` 0.25.10 with its default
  features plus `avif` (an encoding feature); the default feature set was not inspected in the registry source.
- Windows behaviour of `File::try_lock` on network shares, and of `sync_directory` (no-op off unix) — only the code
  paths are stated.
- Whether `File::try_lock` (std) is exclusive on every target: assumed from std semantics, not verified here.
- Runtime ordering between the desktop's `exiting` save and a phone request in flight at quit — not traced.
- The full set of `Active`/`Archived` JSON fields (tasks.rs/archive.rs) was not enumerated; only the files that hold
  them and how they are written.
- `ui.rs` was read only in the cited ranges (8,375 lines); other config writes or threads hidden in unread parts were
  searched by grep only (`thread::spawn`, `Builder::new`, `persist_config_value`, `write_config_value`).
