# Two readers r5: the follow-ups of the requirement map

Source: `docs/sync/status/foundation/resolutions/map-followups.md`, items 1 to 9. No ruling (L, T or S) is named by these items; V2-9 (item 8) is a verifier finding, read from SYNC.md §6.7 and `engine.md` §28.3. New ids continue after the highest id of their topic in that spec: CORE-READ 004, CORE-IMP 008, DEV-CREATE 005, DEV-ICS 010, DEV-WX 002, ENG-OPS 009, ENG-MOVE 008, BRIDGE-KEY 005, BRIDGE-SCREEN 002, RELAY-ROWS 002.

## core.md

### TR5-FOLLOW-1 (item 1: SYNC.md §4.5, display order)

**Section §14.2 The queries.** Add this row at the end of the table (after the `reminders` row):

| Today | In the core | Zone | Notes |
|---|---|---|---|
| `TaskApp::summarize_calendar` (ui.rs:2911-2955) | not moved: stays in `desktop`, over a copy | | today it drains and re-sorts `board.items`; ties on (start, id, sid) (SYNC.md §4.5, CORE-READ-006) |

Add after the table, as a paragraph and two clauses (after the paragraph "Each is today's function …" and the table, before §14.3):

Display order belongs to the caller (SYNC.md §4.5). No read query sorts, drains or reorders `BoardData.items` or any other list of `BoardData`: each orders what it returns in a list of its own, in the order this table states, and the board keeps its own order (items in ascending id, §6). The desktop's calendar summary, today's `summarize_calendar`, stays in the desktop and orders a copy: its entries ascend by (start, id, sid), where start is the instant the entry is drawn at (a block's start, or an event's own span start), id is the item's id, and sid is the block's sid, an event's own span (no block) sorting before every sid of the same item.

- **CORE-READ-005** (core) A read query MUST NOT sort, drain or reorder `BoardData.items` or any other list held by `BoardData`, and MUST order what it returns in a list of its own, as §14.2 states for it.
- **CORE-READ-006** (desktop) The desktop's calendar summary (today's `summarize_calendar`) MUST order a copy of the items, never `BoardData.items`, and MUST order its entries by ascending (start, id, sid) as §14.2 states.

### TR5-FOLLOW-9 (item 9: SYNC.md §9 step 6, the first board)

**Section §15.3 Steps.** Add after step 7 ("**Home.**"), as a paragraph that is not a step:

The first board created from a legacy folder takes its palette from the scheme that `selected_colorscheme_id` of that folder's `userconfig.toml` names in its `colorschemes.json`, and its home place from that file's `coordinates` (SYNC.md §9 step 6). The core does steps 6 and 7 from `LegacyConfig`; the caller fills `LegacyConfig` from the source folder's own `userconfig.toml` (§15.2, `device.md` §18.2).

**Section §15.4 The report**, after CORE-IMP-008:

- **CORE-IMP-009** (core) `import` MUST set the genesis palette from the scheme under `LegacyConfig.selected_colorscheme_id` (default 0) in `colorschemes`, and the genesis home from `LegacyConfig.coordinates`, exactly as §15.3 steps 6 and 7 state, with the defaults and the warnings `palette_unreadable` and `home_out_of_range` they name, and MUST set `palette_set` and `home_set` in the report accordingly.

## device.md

### TR5-FOLLOW-7 (item 7: SYNC.md §6.4, feeds down)

**Section §13.1.2 One fetch, in order**, step 8: append to the step:

A successful fetch (`200`, or `304` with the cached body) sets the subscription's **last good fetch** to the device clock (`wall_ms`); it is kept in memory and, at `open`, starts from the `fetched_ms` of the subscription's feed body file (the time of its last `200`), absent when there is none. A failed fetch leaves it as it was. The sync thread publishes it with the overlay (`Published::overlay_fetched`, §25).

**Section §13.1.3 After a fetch**: add at the end of the section, before the address guard (§13.1.4), the paragraph:

**Feeds down** (SYNC.md §6.4: "last data kept, labelled with its age"). When an ICS host cannot be reached or answers with a failure, the device keeps the subscription's previous events in the overlay and its last good fetch unchanged, and publishes, per subscription, the time of that last good fetch; when Open-Meteo cannot be reached, the device keeps the last report in `weather.tdk` (or memory, local-only) and in `Published::weather()` with its `fetched_ms`, and `status().weather_age_ms` is `wall_ms − fetched_ms`. The interfaces label kept data with that age; the words are those of stages `desktop` and `android` (§1).

**Section §13.1.4**, after DEV-ICS-010:

- **DEV-ICS-011** (device) After a failed ICS fetch, the device MUST keep the subscription's previous events in the overlay and MUST publish, for every subscription with a last good fetch, the device-clock time of that fetch as `Published::overlay_fetched` states, updating it only after a `200` or a `304`.

**Section §13.2 Weather**, after DEV-WX-002:

- **DEV-WX-003** (device) After a failed weather round, the device MUST keep the last report with its `fetched_ms` in `weather.tdk` (in memory in local-only mode) and in `Published::weather()`, and MUST publish `status().weather_age_ms` as the device clock minus that `fetched_ms`.

**Section §25 Public API**, in `impl Published`, after `pub fn weather(…)`:

```rust
    pub fn overlay_fetched(&self) -> &[(u64, i64)];   // (subscription id, wall_ms of its last good fetch), ascending id; a subscription with none is absent (§13.1.2 step 8)
```

### TR5-FOLLOW-2 (item 2: SYNC.md §5.4, reset trust)

**Section §18.6 Reset trust (desktop).** Add at the start of the section, before "`reset_trust(request)` is …":

Reset trust starts a new board from this device's confirmed state: a new recovery seed and sheet, new device keys, a new `board_id`, and a genesis carrying the current confirmed board bytes unchanged; the old board is then pointed at the new one by one `Fence` (`CloseBoard`), and its devices re-pair into the new board (SYNC.md §5.4).

Add at the end of the section:

- **DEV-CREATE-006** (device) `reset_trust` MUST create the new board by every step of §18.1 with `source = CurrentBoard`, drawing a new recovery seed, new device keys and a new `board_id` and taking the engine's confirmed board bytes unchanged as the genesis board, and MUST NOT activate the new identity (step 10) before the old board's `Op(CloseBoard)` naming the new board's relay and door has ended with `OpDone` `Ok`; on its failure or after 600,000 ms it MUST stop with `device/close_failed` and leave the new board pending.

(The error-catalogue row `device/close_failed` and the facade entry `reset_trust(request)` of §17 and §25 stay as they are.)

### TR5-FOLLOW-9 (item 9: SYNC.md §9 step 6, the first board)

**Section §18.2 "Create board from this folder".** Add before the paragraph "`import_check(folder)` (§17.12) is the dry run …":

The first board created from a legacy folder takes its palette from the scheme that `selected_colorscheme_id` of that folder's `userconfig.toml` names in its `colorschemes.json`, and its home place from that file's `coordinates` (SYNC.md §9 step 6). In §18.1 step 2 the device reads `LegacyConfig` from the source folder's own `userconfig.toml` (this data folder's for `ThisFolder`, the named folder's for `Folder(path)`) with today's parser (initialization.rs:243-362, `core.md` §15.2), reading only and writing nothing to it; `import_check` reads it the same way.

After DEV-CREATE-004:

- **DEV-CREATE-007** (device) For `ThisFolder`, `Folder(path)` and `import_check`, the device MUST fill `LegacyFolder.config` from the `userconfig.toml` of the source folder itself, with today's parser and its clamp, and MUST NOT take `coordinates` or `selected_colorscheme_id` from any other file or folder.

## engine.md

### TR5-FOLLOW-3 (item 3: SYNC.md §5.4, when keys rotate)

**Section §28.6 Recovery-key changes.** Append to the paragraph:

Keys rotate on a revocation (§28.5; a suspected compromise is "Replace device", the same operation), on a purge (§28.2) and on a new recovery key (the `K`-shape entry of §26.1, to which the sealer adds the rotation that wraps to the new key, `seal.md` §12.1), never on a schedule (SYNC.md §5.4).

Add after the paragraph:

- **ENG-OPS-011** (engine) The engine MUST send every recovery-key confirmation as a `K`-shape entry carrying the rotation to the new key, and MUST compose an epoch rotation only within a `Revoke` operation (§28.5), a `Purge` (§28.2) or such a confirmation (§26.1), never on a timer, a schedule or the age of an epoch.

### TR5-FOLLOW-8 (item 8: SYNC.md §6.7, re-home)

**Section §28.3 Move board**, after ENG-MOVE-008:

- **ENG-MOVE-009** (engine) When the moving device sees its move's `Fence` in its log, it MUST emit exactly one `Notify(MoveReport { carried_through, old_relay_answered })`, with `carried_through` the `last_seq` it seeded and `old_relay_answered` true exactly when the old relay sequenced that `Fence`, so that, when it is false, the entries the old relay sequenced after `carried_through` are reported as not carried over (V2-9).

(The `MoveReport { carried_through: u64, old_relay_answered: bool }` notification of the public API and `move_status()` of `device.md` §17.8 stay as they are.)

### TR5-FOLLOW-2 (item 2: SYNC.md §5.4, reset trust)

**Section §28.7 Reset trust.** Replace the first sentence "SYNC.md §5.4: a new board from a trusted device's state." with:

Reset trust starts a new board from this device's confirmed state: a new recovery seed and sheet, new device keys, a new `board_id`, and a genesis carrying the current confirmed board bytes unchanged; the old board is then pointed at the new one by one `Fence` (`CloseBoard`), and its devices re-pair into the new board (SYNC.md §5.4).

Add at the end of the section:

- **ENG-OPS-010** (engine) On `Operation::CloseBoard { host, door }`, the engine MUST enqueue exactly one `CtlIntent::Fence { host, door, home_gen: M_K.home_gen + 1 }` with `via: Fence`, MUST end the operation with `OpDone` `Ok` only once that `Fence` is accepted in its log, and MUST NOT seed, append to or upload to the relay it names.

## bridge.md

### TR5-FOLLOW-4 (item 4: SYNC.md §5.6, the Android Keystore key)

**Section §12.5 The Keystore key.** Add after the first paragraph, before BRIDGE-KEY-004:

The key is not generated with `setUnlockedDeviceRequired(true)`, the option SYNC.md §5.6 leaves open. With it, the Keystore refuses every use of the key while the phone is locked, so `KeyVault.unwrap` would fail inside a `SyncJobService` run and in the verify sync of a reminder firing (§14.2, §15.4), which must open the sealed store while the phone is locked; and a failed unwrap is the "key that cannot be unwrapped" case (`device.md` §12.6), which must not follow from a locked screen. The protection it would add, against an attacker holding the phone locked and able to run code as the app, is accepted as residual; the storage key stays non-exportable in the Keystore (StrongBox when present).

After BRIDGE-KEY-004:

- **BRIDGE-KEY-006** (android-shell) The key vault MUST NOT call `setUnlockedDeviceRequired(true)` when it generates the Keystore key, so that the key can be used while the phone is locked.

### TR5-FOLLOW-5 (item 5: SYNC.md §5.6, capture exclusion on Android)

**Section §12.3 The manifest.** Replace the bullet "`PairActivity` sets `WindowManager.LayoutParams.FLAG_SECURE` on its window (the code and the SAS are on screen; SYNC.md §5.6: code screens exclude capture)." with:

- `PairActivity` and `DevicesActivity` set `WindowManager.LayoutParams.FLAG_SECURE` on their windows (§13.1, BRIDGE-SCREEN-003; SYNC.md §5.6: code and seed screens exclude capture).

**Section §13.1 What the phone does.** Add after the paragraph "Below API 30 …":

The screens that show a pairing code, the SAS or a seed exclude screen capture (SYNC.md §5.6): `PairActivity` (the join and re-pair flows: the typed code and the SAS) and `DevicesActivity` (its "add a device" row shows the relay address, the code and the SAS) set `FLAG_SECURE` on their windows in `onCreate`, before `setContentView`, for their whole lifetime. The phone shows no seed (creating a board and recovery-key changes are desktop-only, above). No other surface (the WebView of `MainActivity`, `HealthActivity`, a notification) shows a pairing code, the SAS or a seed.

After BRIDGE-SCREEN-002:

- **BRIDGE-SCREEN-003** (android) `PairActivity` and `DevicesActivity` MUST set `FLAG_SECURE` on their windows before their first content is shown and keep it for their whole lifetime, and no other activity, the page or a notification MUST show a pairing code, the SAS or a seed.

## relay.md

### TR5-FOLLOW-6 (item 6: SYNC.md §6.2, the import budget)

**Section §16.2 The typical day.** Add after RELAY-ROWS-002:

SYNC.md §6.2's import: 5,000 commands of another board brought in as ordinary commands (SYNC.md §9 step 3) by one device, as 5,000 new `cmd` frames of one role-1 author over HTTPS `append`, 16 frames per request (the last of 8), the worst case for requests; rows are the same over the socket. The append budget (§14.1) splits them over two UTC days of the relay clock (3,000 frames on the first, 2,000 on the second), each day after one `login-nonce` and `login`; the check sums both days and compares the sum with one day's limit.

| Item | Count | Worker requests | Object requests | Rows written | Rows read |
|---|---|---|---|---|---|
| `login-nonce` and `login`, each day | 4 | 4 | 4 | 6 | 138 |
| appends (188 on the first day, 125 on the second) | 313 | 313 | 313 | 2 × 5,000 + 313 = 10,313 | 3 × 313 + 3 × 5,000 = 15,939 |
| **total** | | **317** | **317** | **10,319** | **16,077** |
| free-plan daily limit | | 100,000 | 100,000 | 100,000 | 5,000,000 |
| **share** | | **0.32%** | **0.32%** | **10.32%** | **0.32%** |

- **RELAY-ROWS-003** (relay) The conformance suite MUST store the import of §16.2 (5,000 new frames of one role-1 author, 16 per `append`, over two UTC days of the relay clock as the append budget allows) against the Worker target and MUST fail unless the Worker requests, object requests, rows written and rows read it measures, summed over both days, each stay below 20% of the daily limit of the table.

**Section §20 Limits.** Add after the row `| typical day | 2% of every daily limit | | RELAY-ROWS-002 |`:

| What | Value | Unit | Clause |
|---|---|---|---|
| a 5,000-command import | 20% of every daily limit | | RELAY-ROWS-003 |

## For the lead

1. **Item 6, the import against the append budget.** A 5,000-command import by one device cannot finish in one UTC day: the relay stores at most 3,000 new frames per author per day (§14.1, RELAY-BUDGET-001), and seal carries exactly one log command per `cmd` frame (`seal.md`, item kind 1). SYNC.md §6.2 does not say whether "under 20%" is per day or for the whole import. The change takes the conservative reading: the suite sums both days and compares with one day's limit, a stricter test (rows written, the binding measure, is 10.32%). Recommendation: keep that reading. Whether the device should tell the user that a large import spans two days is a desktop wording question that no item covers.
2. **Item 7, the age line and where the overlay's age lives.** No spec fixes the words for a stale calendar or forecast. The specs do fix sync lines elsewhere ("relay unreachable since 14:05 · 3 waiting", `engine.md` §30.1, `relay.md` §14.5), but `device.md` §1 gives wording to stages `desktop` and `android`. The conservative change fixes no line: it publishes the ages and leaves the words to the apps. Recommendation: if a fixed line is wanted, add one to `engine.md` §30.1's table in the same form, for example "calendar as of 14:05" and "forecast as of 14:05". The overlay's last good fetch is also not persisted after a `304`: at `open` it restarts from the feed body file's `fetched_ms` (the last `200`), so after a restart its age can read older than it is. That is the conservative direction, and it avoids rewriting an 8 MiB body on every `304`. If an exact age across restarts is wanted, add a field to `device.tdk`.
3. **Item 1, the owner.** The follow-up names stage `core`, but `summarize_calendar` is not moved into the core (it is not in §14.2's table; it lives in ui.rs). The change splits the duty: CORE-READ-005 (core) bars every read query from reordering the board, and CORE-READ-006 (desktop) holds the summary's tie-break, following the (core, desktop) precedent of CORE-IMP-003. Recommendation: accept the split. The other choice is to move the summary into the core as a new read query, which changes the desktop's code more.
4. **Item 8, what the move report lists.** `MoveReport` names only the seq cut (`carried_through`, `old_relay_answered`). Two things are also not on the new relay and are not listed: history below the seed's `first_seq`, which the devices still hold, and any blob other than the fresh snapshot, such as the shared picture. The change keeps the existing report, which is the conservative reading of V2-9 ("listing anything not carried over" of the log). Recommendation: decide whether `MoveReport` should also carry `first_seq` and whether the picture is re-uploaded after a move.
5. **Item 5, how long `FLAG_SECURE` lasts.** The change sets it on `DevicesActivity` for its whole lifetime, not only while "add a device" shows the code. That is conservative: no gap between steps and no state to track. It costs screenshots of the device list. Recommendation: keep the whole-lifetime rule.

## Map rows

| §4.5 display order | Read queries never reorder `board.items`; display order is the caller's, and the calendar summary breaks ties on (start, id, sid) | CORE-READ-005, CORE-READ-006 |
| §5.4 reset trust | Reset trust starts a new board from a trusted device's confirmed state and points the old board at it with one `Fence` | DEV-CREATE-006, ENG-OPS-010 |
| §5.4 rotation | Keys rotate on a revocation and on a new recovery key, never on a schedule | ENG-OPS-011 |
| §5.6 Android Keystore key | The Keystore key is generated without `setUnlockedDeviceRequired`, so background sync and reminders can open the store while the phone is locked | BRIDGE-KEY-006 |
| §5.6 capture exclusion | The Android screens that show a pairing code, the SAS or a seed set `FLAG_SECURE` | BRIDGE-SCREEN-003 |
| §6.2 import budget | A 5,000-command import stays under 20% of every daily limit of the free tier, checked by the conformance suite | RELAY-ROWS-003 |
| §6.4 ICS host or Open-Meteo down | The device keeps the last overlay and weather and publishes their age for display | DEV-ICS-011, DEV-WX-003 |
| §6.7 re-home listing | The moving device reports what it did not carry over to the new relay | ENG-MOVE-009 |
| §9 step 6 first board | A board created from a legacy folder takes its palette and home place from that folder's `userconfig.toml` | CORE-IMP-009, DEV-CREATE-007 |
