# R1: Codebase readiness for client-side replay

> Working paper from the design run of 2026-11-20 that produced [`SYNC.md`](../../../SYNC.md). It is an agent's report, kept for the detail the design compresses: file and line references, sources, and the reasoning behind each requirement. Where it disagrees with `SYNC.md` or [`SYNC_PLAN.md`](../../../SYNC_PLAN.md), those win. Line numbers refer to `day-planner` at commit `1cfff2f`.

# R1: Is the codebase ready for every device to replay the log itself?

Scope: the day-planner worktree (`dp/`). I read the code and checked portability with `cargo check` for wasm32 and Android. All experiments ran in a scratch copy (`scratchpad/r1-copy`, target directory `scratchpad/r1-target`), and the worktree was not modified. Each claim is marked VERIFIED or ASSUMED.

## Summary

- **`Board::apply` is not deterministic across devices today.** I applied the same five wire commands at the same instant under `TZ=Europe/Helsinki` and under `TZ=America/New_York`. The two runs stored **different instants**: a session at 09:00Z in one and 16:00Z in the other, and a due date at 17:00+03:00 in one and 17:00−04:00 in the other. Even equal instants were written as different text (VERIFIED, `examples/tzdemo.rs`).
- Five sources of divergence: the local time zone, the applying host's clock (`now`), a per-device derived input (the overlay, which Reflow reads), the item order in the `Vec` (the desktop UI re-sorts it), and disk I/O inside `apply`.
- **Ids:** the counter is deterministic once commands are in a total order. But the temporary-id and remap machinery does not fit an asynchronous, blind log. Ids generated on the device are simpler and safe.
- **Portability:** the pure modules depend only on serde, serde_json and chrono. On Android the whole library type-checks once rustls is removed. ring's C build (it needs the NDK) is the only blocker, and that is a toolchain issue. On wasm32 the real code errors are `reqwest::blocking` and one `cfg` gap in `paths.rs`.
- **The phone page can be reused almost as-is** inside an Android WebView with its assets bundled in the app, if the Rust core runs in the same process and answers the page's `api()` calls.
- **Two pre-existing bugs found:** after a restart a new subscription can reuse an existing subscription's id (VERIFIED), and temporary ids at or above 2^62 collapse to one number when JavaScript parses them (VERIFIED).

## 1. Where `apply` is not deterministic, and the fix for each

| # | Hazard | Evidence | Fix | Size |
|---|---|---|---|---|
| 1 | "day + minutes" is turned into an instant in the **applying** host's local zone (`Local.from_local_datetime`). | `planner.rs:296-311`. Used by MoveBlock (`board.rs:1056`), AddBlock (1085), Create (1209), Reflow (1237), and SetEstimate for an event (1135-1139, which reads `anchor.hour()` locally). Wire times without a zone, such as the phone's `datetime-local` strings, are resolved through `parse_local_input` (`board.rs:409-419`). | **Resolve at the origin.** Commands carry UTC instants, and the origin does the clamping. `apply` then does no time-zone maths. The origin resolves in a replicated board time zone. | 1.5–2 d |
| 2 | `DateTime<Local>` is serialized with the device's UTC offset, so equal instants produce different bytes. | Experiment: `created` was written as `09:00+03:00` in one run and `02:00-04:00` in the other. Affected fields: `tasks.rs:15,180,181,203`, `archive.rs:122-152`, the Command fields. | One canonical serde `with` module that writes UTC (`Z`) and reads any offset. Older builds can still read the files. | 0.5 d |
| 3 | `apply` reads `now`. | `created: now` (`board.rs:1420`, `1464`). The archive time is clamped to now + 1 day and defaults to now (`466-469`). Reflow defaults to `now_marker` (`1231`). `stamped()` covers only Complete, Delete and Reflow (`371-378`). | Put one origin timestamp in the log envelope and make it `apply`'s only clock. This also fixes the queued created-time bug. | 0.5 d |
| 4 | Reflow reads **derived, per-device** data: the overlay's anchors. | `board.rs:906`. The overlay is resolved in the fetcher's zone (`ics.rs:740-749`) over a window based on the fetcher's own date (`subscriptions.rs:594`). | **Compile Reflow at the origin** into explicit session moves. | 0.5–1 d |
| 5 | The order of `items` differs per device. `summarize_calendar` drains `board.items` and rebuilds it sorted by a score that depends on the device and the time. It runs after every desktop apply. | `ui.rs:2911-2955`, called at `ui.rs:2710`. Reflow's sort is stable (`planner.rs:697`) over items in `Vec` order (`board.rs:891`), so equal start times are tie-broken by device order. | Keep the display order in the UI and never mutate the replicated `Vec`. Break ties canonically by (start, id, session). | 0.5–1 d |
| 6 | Disk I/O inside `apply`. | Restore and ForgetArchived lazily load the archive from disk (`board.rs:1263,1308`; `archive.rs:754-756`). See §3. | Keep the archive in memory, loaded up front. | in §3 |
| 7 | Floating point. | `snap`/`clamp_block` use f32 (`planner.rs:260-279`) but only on integer inputs. ASSUMED deterministic on IEEE targets (x86_64 SSE2, aarch64, wasm32). | Rewrite in integer arithmetic anyway. | 1–2 h |
| 8 | The version counter. | It is bumped in `save_*` (`924-940`), in `adopt_overlay` (`814`), and seeded from the clock. | Version = log sequence number. Overlay changes bump a separate per-device view counter. | in §3 |

**Not hazards (VERIFIED by grep):**
- The board state contains no `HashMap`/`HashSet`. `assign_missing_ids` uses a `HashSet` only for membership checks (`tasks.rs:688-699`), and `ics.rs` uses maps only for lookups (`693-695`, `740-748`).
- `apply` uses no randomness.
- The archive sort is stable and its insert order is deterministic (`archive.rs:723`, `741`).

**Presentation only:**
- Ranking uses f32 `exp2` (`tasks.rs:356-374`). ASSUMED: each platform's maths library can differ by one ULP, so the order of near-ties can differ between devices. Scores are never stored, and §14.4 is untouched.

## 2. Ids

**Today:**
- `next_id` (`board.rs:559`, `826-830`) is seeded from **items only** (`board.rs:645`, `766`). Subscriptions draw from the same counter (`1332`).
- Restore reuses the archived id if it is free, and otherwise takes the next one (`1270-1281`).
- A client numbers its own items from `CLIENT_ID_FLOOR` = 2^62 (`sync.rs:74`, `main.rs:165,182`) and keeps them out of the server's range from the outbox (`sync.rs:403-410`).
- When the server's reply carries the real id, `Outbox::remap` re-points queued commands (`sync.rs:425-432`), the sender's `known` map catches late commands (`647`, `733-738`), and the UI renumbers the item and its selection (`ui.rs:2811-2836`).
- `adopt_temporaries` renumbers leftover temporary ids when a replica folder becomes a board of its own (`board.rs:697-711`).

**Does it survive replay?** The counter is deterministic in a total order, so confirmed replicas agree. Four problems remain:
1. A device learns its real id only when its own entry comes back sequenced. It must hold back anything that names an unconfirmed id, so nothing can be pipelined.
2. Ids at or above 2^62 collapse in JavaScript. VERIFIED: Node's `JSON.parse` turns 2^62+1 and 2^62+2 into the same number. A WebView UI cannot show pending items under such ids, and a client desktop serving its own phone page (§22.5) already has this latent problem.
3. Subscription commands are never remapped (`item_id`/`creates_item`, `board.rs:322-341`, `382-387`).
4. **A bug, VERIFIED (`examples/subid.rs`):** after a reopen, a second subscription gets id 1 again (`ids=[1, 1]`), and `RemoveSubscription{1}` removes both.

**Recommendation:** device-generated ids, `id = device_no << 40 | per-device counter`.
- `device_no` (1–4095) is assigned at pairing. Legacy ids stay below 2^40, and every id is below 2^53, so JavaScript handles them.
- Add, QuickAdd, Create and AddSubscription carry the `id`. Restore carries a `fallback_id`.
- `apply` rejects an id that is already in use; the rejection is deterministic.
- This removes the remap, `known`, `number_from` and the temporary range. Keep `adopt_temporaries` for migration.
- Fix the seeding bug either way.
- Size: 1–1.5 days.

## 3. Persistence coupling

**Where `apply` writes today:**
- `save_items` → `oversafe_activesave` (temporary file, fsync, rename, directory fsync; `tasks.rs:725-751`), in nearly every arm.
- `save_notes` (`utilities.rs:106-132`) and `save_subscriptions` (`subscriptions.rs:505-535`).
- `retire` appends the archive row with fsync **before** removing the item (`board.rs:1475-1487`, `archive.rs:802-817`).
- Restore rewrites the archive, then saves the items, with a compensating write on failure (`board.rs:1261-1304`). ForgetArchived rewrites the archive.

Every arm validates before it mutates (checked arm by arm). So an in-memory `apply` is all-or-nothing, and only I/O failures leave changes half-made. On wasm, `std::fs` is a stub (ASSUMED), so today every apply there would mutate memory and then return 500.

**Cleanest split:**
- A pure `BoardData` (Clone, Serialize): items, archive rows plus opaque unreadable lines, notes, subscriptions, board settings.
- `apply(&mut BoardData, &Command, &Ctx) -> Result<Applied, BoardError>`, where `Applied` carries the reply, a change set (items, notes, subs, archive appended or removed) and an optional "forget this subscription's overlay".
- A `Store` performs the writes with the **existing** atomic helpers, in today's order (archive first, then items), and keeps Restore's compensation. Local mode keeps today's files.
- In replica mode the truth is the log:
  - Append each confirmed entry with fsync **before** applying it.
  - Checkpoint the whole `BoardData` periodically as **one** atomically written snapshot with its sequence number.
  - Recover from the snapshot plus a replay.
- Per-file checkpoints cannot give a consistent base to replay from.
- Size: 2–3 days, including porting the 28 board tests.

## 4. Portability (VERIFIED by command)

**wasm32-unknown-unknown:**
- `cargo check --lib --no-default-features` fails in getrandom 0.3.4 ("not supported by default; you may need to enable the 'wasm_js' configuration flag"). It arrives via kmeans_colors → rand 0.9. That is a toolchain/configuration issue in a crate the core does not need.
- With wasm_js forced in the scratch copy, there are 9 genuine errors: `reqwest::blocking`/`redirect` (`weather.rs:10`; `subscriptions.rs:545,593,731,741`; `sync.rs:128,138,159`) and `paths.rs:231` (`user_data_dir` has no arm for this target). Everything else type-checks.
- ASSUMED: on this target, std's fs, thread and net are stubs, and `SystemTime::now`/`Instant::now` panic (`phone.rs:1067`, `phone.rs:336/546`).

**aarch64-linux-android:**
- Only ring 0.17.14 fails (via reqwest rustls-tls). Its build script cannot find `aarch64-linux-android-clang`.
- With the host's clang targeting Android, it then fails on missing bionic headers (`bits/libc-header-start.h`; with `-ffreestanding`, `assert.h`). That is the NDK sysroot.
- **With rustls removed (scratch copy), the whole library type-checks.** No genuine code incompatibility. A real `.so` needs the NDK linker anyway (cargo-ndk), which also fixes ring.

**Core crate (dependencies: serde, serde_json, chrono; optionally chrono-tz):**

| Module | Goes into the core | Moves out |
|---|---|---|
| `board.rs` | Command, Reply, BoardError, BoardData, pure `apply` | `open`, `unreadable_files`, `save_*` |
| `tasks.rs` | types, scoring, migration | read, quarantine, save, `sync_directory` |
| `planner.rs` | all of it | — |
| `archive.rs` | Archived, verdicts, summaries, in-memory log | load, record, take, rewrite, append |
| `ics.rs` | all of it | — |
| `subscriptions.rs` | types, `checked_url`, naming, Overlay (sealed, digest, anchors, forget), an `overlay_from(parsed…)` | file I/O, `fetch_*`, Feeds thread |
| `utilities.rs` | `detab`, `days_in_month` | notepad I/O, `toml_edit` |
| `phone.rs` | Snapshot types, `build_day`/`tray`/`item_detail`/`done_rows`, `with_subscribed`, `calendar_feed`, `answer_query` (palette, look and dials passed in, not globals: `258-270`) | tiny_http, Replies/Pulse, backdrop (image/AVIF), tokens, QR, UdpSocket, gzip |
| `weather.rs` | Report types, `build_report`, `sky_symbol`, cities | fetch thread |

Also move out: tempfile, threads, blocking reqwest, mimalloc (desk-only already), toml, palette/kmeans.

Couplings to cut: `phone::Wake`, which `subscriptions`/`sync`/`weather` use, and `phone::generate_token`/`REQUEST_HEADER` in `sync`.

Size: 2–3 days, mostly mechanical, in a workspace.

## 5. `sync.rs`: what can be reused

| Part | Today | For a sequenced encrypted log |
|---|---|---|
| `Queued` + Outbox (`337-452`) | JSON array; atomic persist with dir fsync; a corrupt file is set aside | **Reuse.** Add the origin stamp and device id; drop `local_id`. |
| Request key (`501`; `phone.rs:147`) | Random, stored with the command, the same on every retry; the server keeps 1024 replies | **Reuse** as the relay's deduplication key (outside the ciphertext), or use (device, counter). |
| Sender (`631-729`) | Blocking; one command at a time; 8 s timeout; 3 s retry; 401/403 and 5xx keep, other 4xx drop and report | Reuse the shape and add batching. A blind relay never judges commands: rejection is found when applying, the same on every device, and the origin reports it (`SyncEvent::Rejected`). |
| Listener (`757-842`) | Long-polls `/api/wait`, fetches the **whole** board, gated by pending edits and `acked_version` | **Replace** with "entries after seq N", then apply, then rebase pending on confirmed. The gate and ack race disappear. |
| First-contact set-aside, `CLIENT_MARKER`, `forget_replica` (`221-330`) | Never overwrite a local board | **Reuse.** The marker records the relay plus board id and key fingerprint. |
| remap, `known`, `renumber`, `number_from`, `Remapped` | Temporary ids | Delete once ids are generated on the device. |

Size: 3–5 days, not counting crypto, relay or keys.

## 6. Phone page: features and seams (for parity)

**Views.** `NEXT_MODE` cycles day → agenda → weather (`phone.html:1913`):
- **Day:** timeline lanes with ghosts and subscribed blocks (laid out by the server), all-day row, now-line, week strip (`1969`), swipe ±1 day (`5478-5506`), tap empty space to create (`5462`), hold-to-drag (350 ms) to move (`4296-4380`).
- **Agenda:** continuous scroll, month rail, "glance" line, months cached (`2059-2700`).
- **Weather:** chart with a plan band, facts, week, place, scrubber (`2699-4050`).

**Sheets:**
- **Item** (`4980-5190`): rename; verbs Done, Book first free slot, Tomorrow, each with a 6 s Undo (`4876-4949`); the block's time and nudges ±15m/±1h/Now; length chips; remove block; due chips plus picker; severity or horizon; estimate; "Block at"; Unplan; event When/For; routine weekdays All/Once; armed Delete.
- **Tray** (`5208-5329`): Unplanned (quick-add with optional due date, ＋book, ✓), All (ranked), Done (Restore), a Reflow button when behind.
- **New:** task, event or routine (`5331`).
- **Notes:** whole-text Save (`4718`).
- **Look** (`4542-4716`): pick, pan/pinch, crop to 1080×2340 JPEG, POST; blur/brightness dials; "use the desk's".

**Offline:**
- An outbox in localStorage with Undo by removing the queued edit. It applies **nothing** locally (`1465-1481`).
- Caches: last 14 day-snapshots (`1278-1318`), months 2 back and 4 forward (`1329-1409`), weather, place.
- Service worker: shell cache-first, photo and sky caches, never `/api/` (`phone_sw.js:12-112`).
- Banners, quiet-offline, the token gate (`5400`), keys ←/→/t/w.

**Seams:**
- `api()` (`1240-1271`) carries `/api/state`, `/api/command` (via `post`, `1500`), `/api/wait` (`5540-5574`), `/api/weather`, `/api/look` and `/api/background`.
- Outside `api()`: the photo (`2355-2408`), the sky SVGs (`3949`), Open-Meteo directly (`3031-3138`), the manifest and service worker.

**WebView reuse (VERIFIED Android facts):**
- `WebViewAssetLoader` serves bundled assets at `https://appassets.androidplatform.net`, a secure origin.
- `WebResourceRequest` exposes no request body. GET routes can therefore be intercepted and answered by the core in the same process, but POST commands and uploads need a JavaScript bridge (`addWebMessageListener`/`addJavascriptInterface`) behind `api()`.
- With the core in-process, `/api/wait` becomes a push, and edits apply locally at once.

## 7. Subscriptions, overlay, feed

**What a device-side fetcher needs:**
- The replicated subscription list. Its URLs are credentials and must stay end-to-end encrypted.
- An https-only client: at most 3 redirects, 20 s timeout, 8 MiB cap (`subscriptions.rs:93-97`, `741-743`).
- `ics::parse` (pure).
- A window around today (60 days back, 400 forward).
- The previous overlay, so a failed fetch keeps its events (`628-633`).
- The device's zone (`ics.rs:740-749`).

**Its output:**
- A per-device `Overlay`, sorted and digested by FNV over events only (`431-450`).
- `covers` (the span actually read, `637`).
- A cache in `subscribed_cache.json`.
- Auto-rename commands (`ui.rs:2757-2765`). These settle, because they are idempotent.

**The "two fetchers" rule (§23.3: two clocks, two copies of a secret, two answers about Tuesday):** a blind relay cannot fetch, so devices must. That is safe **only if** the overlay never feeds `apply` (§1 #4). Optional: the desktop publishes an encrypted overlay blob as a fallback for the phone.

**Feed:** `calendar_feed(items, now)` is pure (`phone.rs:2585-2685`), but a blind relay cannot serve it. It would have to live on a device, for example written into Android's system calendar.

## 8. Weather

- The desktop and server fetch Open-Meteo every 600 s for the `coordinates` in `userconfig.toml`: a per-device setting, never board data (`weather.rs:287-299`, `432+`; `server_main.rs:342`). The report sits in a global static for `/api/weather` (`weather.rs:244-253`).
- The phone uses the desk's report or, failing that, asks Open-Meteo itself for the desk's remembered place or its own geolocation. `buildReport` mirrors the Rust code (`phone.html:2814-2857`, `3037-3138`).
- It depends on the board only for the plan band. No board state depends on weather.

## 9. Board data vs per-device

**Replicated today:** items, archive, notes, subscriptions, the id counter.

**Per-device** (`initialization.rs:439-495`):
- `coordinates`, `background`, the scheme id, tint/blur/light, window/monitor/fullscreen/fps/ui-scale, `calendar_weeks_to_show`, `three_day_weather`.
- The phone-server and server settings, and the tokens.
- `colorschemes.json`, `phone_background.img`, overlay cache, outbox, marker.

**Proposed additions to the replicated board:**
- The board time zone (needed for §1 #1).
- Optionally "home" coordinates and the six palette colours, so the phone matches the wall.

**Keep per-device:** the backgrounds and dials, all tokens and URLs.

Note: on non-Unix systems the 160-bit token falls back to a non-CSPRNG (`phone.rs:1049-1078`). Any key in the new design must come from getrandom/OsRng.

## 10. Tests

- `board.rs` has 28 tests, each on a `tempfile::tempdir` board (`fresh()`, `reopen()`), with clocks from `Local.with_ymd_and_hms`.
- There is **no `[dev-dependencies]` section, and no proptest or quickcheck** (VERIFIED).
- Proposed property test (in the core, with proptest 1.x as a dev-dependency):
  - Random valid command sequences.
  - Replica B = deserialize(snapshot at a random point) plus the rest of the log.
  - Assert equal canonical JSON and equal hashes.
  - Also a rebase test: pending applied over confirmed, with interleavings.
  - Cross-zone: an explicit zone or context per replica once `Local` is out of `apply`, plus a CI step that runs a state-hash binary under two `TZ` values (as `tzdemo` did).
- Size: 1.5–2 days.

## 11. Other hazards

- **Sessions are addressed by index** (MoveBlock, RemoveBlock, Reflow; §22.4, E11). Concurrent removals land edits on the wrong block silently. Stable session ids fix this: 1–2 days, including the page.
- **Notes** are whole-text last-writer-wins, up to 20,000 characters (`board.rs:48`). Desktop autosave is debounced, so the log grows: coalesce and compact.
- **Version skew:** an older build **silently drops unknown fields** (VERIFIED, `examples/wirecompat.rs`). A new `id` or `at` would be ignored and the replica would diverge. Unknown `op`s are refused. Log entries need a schema version, and a device must halt on an entry newer than it understands.
- **Archive keys:** instant equality survives zone changes. Canonical text must keep nanoseconds.
- The **overlay `forget`** inside `apply` touches derived state. Return it as an effect.
- Archive lines that cannot be read are kept as raw bytes (`archive.rs:641`). The migration snapshot must carry them or report them.

## Effort

| Work | Days |
|---|---|
| Determinism fixes (§1) | 3.5–5.5 |
| Persistence split (§3) | 2–3 |
| Ids (§2) | 1–1.5 |
| Core crate (§4) | 2–3 |
| Property tests (§10) | 1.5–2 |
| Session ids (optional) | 1–2 |
| **Readiness subtotal** | **≈11–17** (ASSUMED) |
| Sync client (§5) | +3–5 |

## Recommendation

Do the core work first, independent of which relay is chosen:

1. Canonical UTC serialization.
2. An origin timestamp in the envelope.
3. Commands that carry resolved instants, so `apply` is time-zone-free, plus a replicated board time zone.
4. Reflow compiled at the origin.
5. Device-generated ids (and fix the subscription-id seeding bug now).
6. A pure `BoardData::apply` plus a `Store` that keeps today's atomic writes.
7. The UI stops re-sorting `board.items`.
8. Schema-versioned log entries.

Gate all of it with the proptest determinism suite before any network work.

Then split out `taskdeck-core` (serde, serde_json, chrono only). It builds for Android and wasm without C. Ship Android as a native shell that runs the core in-process with JNI or UniFFI and bundles the **unchanged** `phone.html` through `WebViewAssetLoader`, plus a small JavaScript bridge behind `api()`. That reuses about 5.7k lines of UI, gives full offline use, and means no host ever serves the code. Reuse `sync.rs`'s outbox, keys, set-aside and error mapping; replace its listener and the remap machinery.

---

## Key facts

- **[VERIFIED]** Applying the same wire commands at the same instant under TZ=Europe/Helsinki vs TZ=America/New_York yields different instants (task session 2026-09-04T12:00+03:00 = 09:00Z vs 12:00-04:00 = 16:00Z; quick_add deadline 17:00+03:00 vs 17:00-04:00) and different text for equal instants (created 09:00+03:00 vs 02:00-04:00); output files differ _(evidence: scratchpad/r1-copy/examples/tzdemo.rs run twice; outputs scratchpad/tz-Europe_Helsinki.json vs tz-America_New_York.json, cmp: differ at char 87)_
- **[VERIFIED]** Day+minutes are resolved to instants with Local.from_local_datetime in the applying host's zone, used by MoveBlock, AddBlock, Create, Reflow, SetEstimate(event) and naive wire times _(evidence: planner.rs:296-311; board.rs:1056,1085,1209,1237,1135-1139; board.rs:409-419)_
- **[VERIFIED]** apply reads the host clock: created=now in push_new/push_created, archived_at clamp to now+1d and default now, Reflow default from now_marker; Command::stamped covers only Complete/Delete/Reflow _(evidence: board.rs:1420,1464,466-469,1231,371-378)_
- **[VERIFIED]** Reflow uses the per-device derived overlay as anchors, which is fetched at different times and resolved in the fetcher's local zone _(evidence: board.rs:906; ics.rs:740-749; subscriptions.rs:594,610)_
- **[VERIFIED]** The desktop UI drains and re-sorts board.items by device-local, time-dependent score after every apply, while reflow's stable sort keeps Vec order for equal starts _(evidence: ui.rs:2911-2955 (drain 2913, rebuild 2953-2955), called from ui.rs:2710; planner.rs:697; board.rs:891)_
- **[VERIFIED]** Board state holds no HashMap/HashSet; HashSet in assign_missing_ids and HashMaps in ics.rs are used only for membership/lookup; apply uses no randomness _(evidence: grep of src/*.rs; tasks.rs:688-699; ics.rs:693-695,740-748)_
- **[ASSUMED]** clamp_block/snap use f32 on integer inputs only; deterministic on IEEE targets (x86_64 SSE2, aarch64, wasm32) _(evidence: planner.rs:260-279; IEEE-754 correctly rounded division and round())_
- **[ASSUMED]** Ranking scores use f32 exp2, which may differ by an ulp across platform libms; scores are not stored state _(evidence: tasks.rs:356-374,387-389; board.rs:860-870)_
- **[VERIFIED]** next_id is seeded from items only; after reopening, a new subscription reuses id 1 (ids=[1,1]) and RemoveSubscription{1} removes both _(evidence: board.rs:615-616,645,1332; scratchpad/r1-copy/examples/subid.rs output 'first=Some(1) second=Some(1) ids=[1, 1]' and 'remaining=0')_
- **[VERIFIED]** Temporary client ids (>= 2^62) collapse when parsed by JavaScript: JSON.parse of 4611686018427387905 and ...906 both give 4611686018427388000 _(evidence: node -e JSON.parse output; double spacing at 2^62 is 1024 (python check); sync.rs:74, board.rs:58)_
- **[VERIFIED]** Subscription commands are neither remapped nor treated as creates (item_id/creates_item exclude them), so offline subscription edits on a client carry unremapped temporary ids _(evidence: board.rs:322-341,382-387; sync.rs:425-432; ui.rs:2726)_
- **[VERIFIED]** Older builds silently drop unknown fields in a Command (e.g. a new id/at) but refuse unknown ops _(evidence: scratchpad/r1-copy/examples/wirecompat.rs: 'unknown fields -> Ok(QuickAdd { name: "milk", deadline: None })'; unknown op -> Err(unknown variant))_
- **[VERIFIED]** apply persists inside its arms: save_items/save_notes/save_subscriptions (atomic temp+fsync+rename+dir fsync), archive append before item removal, Restore/ForgetArchived rewrite the archive and lazily load it from disk _(evidence: board.rs:924-940,1263-1315,1475-1487; tasks.rs:725-751; utilities.rs:106-132; subscriptions.rs:505-535; archive.rs:683-817)_
- **[VERIFIED]** Every apply arm validates before mutating, so an in-memory apply can be all-or-nothing _(evidence: board.rs:951-1399 read arm by arm)_
- **[VERIFIED]** wasm32 check of the unmodified lib fails in getrandom 0.3.4 ('wasm32-unknown-unknown targets are not supported by default; ... wasm_js'), pulled only by kmeans_colors -> rand 0.9 _(evidence: cargo check --lib --no-default-features --target wasm32-unknown-unknown; cargo tree -i getrandom@0.3.4)_
- **[VERIFIED]** With getrandom wasm_js forced (scratch copy), wasm32 has 9 genuine errors: reqwest::blocking/redirect in weather.rs/subscriptions.rs/sync.rs and paths.rs:231 user_data_dir; everything else type-checks _(evidence: cargo check output: weather.rs:10, subscriptions.rs:545,593,731,741, sync.rs:128,138,159, paths.rs:231)_
- **[ASSUMED]** On wasm32-unknown-unknown std fs/thread/net are runtime stubs and SystemTime::now/Instant::now panic _(evidence: Rust std platform support for wasm32-unknown-unknown; uses at phone.rs:336,546,1067)_
- **[VERIFIED]** aarch64-linux-android check fails only in ring 0.17.14's build script (no aarch64-linux-android-clang; with host clang, missing bionic headers): a toolchain/NDK issue, not a code incompatibility _(evidence: cargo check --target aarch64-linux-android output; clang wrapper errors: bits/libc-header-start.h, bits/wordsize.h)_
- **[VERIFIED]** With reqwest's rustls-tls removed (scratch copy), the whole lib (no default features) type-checks for aarch64-linux-android _(evidence: cargo check ... --target aarch64-linux-android: 'Finished dev profile' in 36.8s)_
- **[VERIFIED]** Cross-module couplings to cut for a core crate: subscriptions/sync/weather use phone::Wake; sync uses phone::generate_token and REQUEST_HEADER; ics uses board::NAME_MAX_CHARS and utilities::detab/days_in_month _(evidence: subscriptions.rs:48; sync.rs:65,197,501; ics.rs:31,374,1038)_
- **[VERIFIED]** phone::snapshot depends on process globals (backdrop, look dials) besides board data and now _(evidence: phone.rs:258-270,1675-1715)_
- **[VERIFIED]** The phone page routes /api/state, /api/command, /api/wait, /api/weather, /api/look, /api/background through api(); photo, sky SVGs, Open-Meteo, manifest and service worker are fetched outside it _(evidence: phone.html:1240-1271,1500,1694,2396,2829,3127,3949,4660,4684,4715,5553,5699)_
- **[VERIFIED]** The phone's offline outbox applies nothing locally; edits are shown as waiting _(evidence: phone.html:1465-1481,1531-1586)_
- **[VERIFIED]** The phone page loads no external scripts; it is self-contained HTML/JS plus the service worker _(evidence: grep '<script' phone.html: single inline script at line 948)_
- **[VERIFIED]** Android WebResourceRequest exposes method, headers, url, gesture, main-frame, redirect, but no request body; WebViewAssetLoader serves bundled assets at https://appassets.androidplatform.net _(evidence: https://developer.android.com/reference/android/webkit/WebResourceRequest ; https://developer.android.com/reference/androidx/webkit/WebViewAssetLoader)_
- **[VERIFIED]** sync.rs outbox persists Queued{command, local_id, key} atomically with dir fsync; the key is generated per queued command and reused on retries; the server keeps 1024 replies by key _(evidence: sync.rs:337-346,436-451,500-510; phone.rs:141-147,287-406)_
- **[VERIFIED]** The listener long-polls /api/wait and fetches the whole board, gated by pending edits and acked_version _(evidence: sync.rs:757-842)_
- **[VERIFIED]** Subscription fetching is https_only, at most 3 redirects, 20s timeout, 8 MiB cap, a 60-back/400-forward window from the fetcher's local date, and keeps a failed calendar's previous events _(evidence: subscriptions.rs:93-103,545-576,593-639,741-743)_
- **[VERIFIED]** Weather is fetched every 600s from Open-Meteo for the per-device config coordinates and held in a global static for /api/weather; the phone can fetch Open-Meteo itself for the desk's remembered place or its own location _(evidence: weather.rs:244-253,287-299,432-500; server_main.rs:342; phone.html:2814-2857,3031-3138)_
- **[VERIFIED]** userconfig.toml keys are all per-device (coordinates, background, scheme, dials, window, phone server, server url/token); replicated board data is items, archive, notes, subscriptions _(evidence: initialization.rs:439-495; board.rs:473-486)_
- **[VERIFIED]** On non-unix systems the token generator falls back to RandomState+clock+pid, which is not a CSPRNG _(evidence: phone.rs:1036-1078)_
- **[VERIFIED]** There is no [dev-dependencies] section and no proptest/quickcheck; board.rs has 28 tests using tempfile tempdirs and Local.with_ymd_and_hms clocks _(evidence: Cargo.toml (no dev-dependencies); grep -c '#[test]' src/board.rs = 28; board.rs:1496-1514)_
- **[VERIFIED]** Sessions are addressed by index, so concurrent removals can silently retarget a queued move; this is documented as unsolved _(evidence: board.rs:166-176,1044-1102; DOCUMENTATION.md §22.4 (lines 3283-3290); CODE_REVIEW.md E11 (lines 120-127))_
- **[ASSUMED]** Effort for codebase readiness (determinism, persistence split, ids, core crate, property tests, optional session ids) is about 11-17 working days, plus 3-5 days for the sync client _(evidence: Per-item estimates in report sections 1-5, 10, 11)_

## Recommendations

- Make apply time-zone-free: the origin resolves day+minutes into UTC instants and clamps there, and commands carry the instants (Create, MoveBlock, AddBlock, SetEstimate for events). Add a replicated board time zone (IANA) that every device uses when compiling commands and drawing days.
- Serialize every DateTime canonically in UTC (one serde 'with' module that reads any offset and writes Z), so replicas are byte-identical and older builds can still read the files.
- Put one origin timestamp in each log entry's envelope and make it apply's only clock: created times, the archive default and clamp, and Reflow's 'from'. This also fixes the queued created-time bug.
- Compile Reflow at the origin into explicit session moves, so apply never reads the per-device overlay. Break ties canonically by (start, id, session).
- Stop summarize_calendar from re-sorting board.items in place. Keep a display order in the UI, and edit the notepad through a draft rather than the replicated field.
- Switch to device-generated ids (device_no << 40 | counter, all below 2^53 so JavaScript handles them) carried in creating commands, with a fallback id for Restore. Retire the temporary-id and remap machinery.
- Fix the subscription-id bug now, independent of the design: seed next_id from subscription ids as well as items. A reopened board currently reissues id 1, and RemoveSubscription then removes both.
- Split persistence out: a pure BoardData::apply returns a change set, and a Store writes it with the existing atomic helpers. Replicas keep a write-ahead log plus one atomic snapshot file, and load the archive eagerly.
- Version every log entry with a schema number, and halt sync on an entry newer than the device understands: older builds silently drop unknown fields.
- Create taskdeck-core (serde, serde_json, chrono only; pure board, planner, archive, tasks, ics, overlay types, snapshot builder, weather report types). Move fs, threads, reqwest, tiny_http, image, tempfile, qrcode and toml out, and cut the phone::Wake coupling.
- Add proptest 1.x as a dev-dependency only, with a determinism suite: snapshot round trips, the rest of the log applied on a second replica, rebasing pending edits, and a CI run of a state-hash binary under two TZ values.
- For Android, run the core in-process (JNI/UniFFI) and bundle the unchanged phone.html via WebViewAssetLoader. Answer GETs through request interception and POSTs through a JavaScript bridge behind api(), with /api/wait replaced by a push. Build with cargo-ndk, or do networking in Kotlin so the core needs no C toolchain.
- Reuse sync.rs's outbox, request keys, first-contact set-aside, forget_replica and error mapping. Replace its listener with a 'fetch entries after seq N, apply, rebase pending' loop.
- Generate all key material with getrandom/OsRng, never the token generator's RandomState fallback used on non-Unix systems.
- Consider stable session ids (1-2 days) to remove the documented hazard of a queued edit landing on the wrong block.

## Open questions

- Board time-zone semantics: should a 'day' always mean the board's home zone on every device (today's server behaviour), or each device's local zone? This decides what a travelling laptop or phone shows.
- Should calendars be fetched by every device, or should one device (the always-on desktop) publish an encrypted overlay blob that the phone uses as a fallback?
- Is whole-text last-writer-wins acceptable for the notepad across devices, or should SetNotes carry a base hash so concurrent edits are detected?
- Version-skew policy: halt sync on any newer schema entry, or define backward-compatible decoding with deny_unknown_fields-style strictness?
- Should the iCalendar feed survive, which needs a trusted host, be replaced by writing into Android's system calendar, or be dropped?
- Should the phone's background and dials and the desk's palette become replicated board settings, or stay per device?
- How should archive lines that cannot be read (kept as raw bytes today) be carried into the first encrypted snapshot?
- Should session-index addressing be replaced by stable session ids in the same wire revision, since the wire changes anyway?
