# Brief: `docs/sync/spec/bridge.md` (owning stage: `android-shell`; some clauses belong to `android`)

Scope (SYNC_PLAN.md §6.1 item 3): "the phone page's routes and verbs answered natively, every change the
forked page needs (the bridge behind `api()`, the requests it makes outside `api()`, storage, the service
worker), the allow-list, the JNI surface, WebView settings (SYNC.md §7, R1 §6, R2)".
`android-shell` (§6.6, wave 1) builds the Gradle project, the JNI library (strings in, strings out,
`catch_unwind` at every entry, three ABIs), the fork of `src/phone.html` in `android/page/`, the hardened
WebView with its exfiltration harness, Keystore wrap and unwrap, backup exclusions, the WebView version
check and a timing harness; there the bridge answers from the fixtures and an in-memory echo.
`android` (§6.10, wave 3) puts the real device runtime behind the same bridge and adds the sync line,
notices, the notes conflict sheet, "will be refused" marks, jobs, native screens, the photo picker,
location, reminders and the builds. Give each clause the owner that must prove it: `android-shell` for the
shell, page fork, WebView, allow-list and JNI shape; `android` for what needs the real runtime.

## Sources
- The code digest `docs/sync/status/foundation/digests/bridge-digest.md`
  (every request, verb, route, storage key and DOM sink of today's page, with file:line).
- SYNC.md §2.1 (Android row and permissions), §3 (the Android column), §4.9, §5.3, §5.6, §7 (the WebView,
  push, compromised-device and account rows), §8 (Android). docs/sync/research/R2 in full; R1 §6; V1
  findings 9, 14, 19; V3 findings 6, 12, 13, 16, 17, 19 and its "Smaller implementation notes".
  SYNC_PLAN.md §2 (minSdk 26; the page forked; administration from the phone; reminders), §5 (the WebView
  58 versus 133 fact; no emulator in containers), §6.6, §6.10.
- The drafts in `docs/sync/spec/`: `device.md` (the facade the JNI library wraps, the key-store interface),
  `core.md` (intents, the snapshot JSON, `reminders()`), `engine.md` (notices, banners, halts, evidence).

## Must contain
1. **Process model**: one process, one core instance behind a mutex, no `android:process`; the WebView
   renderer is the only other process; what runs with the activity and what runs without it (jobs, alarm
   receivers) and that all of them reach the same JNI library.
2. **The bridge**: the transport (`WebViewCompat.addWebMessageListener` with the asset origin as the only
   allowed origin rule, never `addJavascriptInterface`), the message envelope in both directions (request
   id, route or verb, body; reply; pushed events such as "the board changed", sync status, a refusal or a
   notice), ordering, size limits, what happens to a request when the page reloads, and how `/api/wait`
   becomes a push.
3. **The allow-list**: every route and verb the bridge answers, one row each: the page's 17 board verbs and
   its read routes, the weather route, the Look and background routes, page preferences, location, the
   photo picker; for each the request and reply shapes (equal to today's JSON unless you list the change),
   which facade call answers it, and its owner stage. Then what the bridge MUST refuse: any other verb or
   route, and by name every subscription, device, key, recovery and relay operation (SYNC.md §7).
4. **Every change to the forked page**, as a numbered list a reviewer can check against a diff of
   `android/page/` versus `src/phone.html`: `api()` posts through the bridge; each request made outside
   `api()` (the photo, the sky symbols, Open-Meteo, the manifest, the service worker registration) and how
   it is answered natively or removed; `localStorage` use replaced by bridge preferences with DOM storage
   off (which keys survive as preferences, which caches and queues go away and when: `android-shell` versus
   `android`); the service worker gone; the token gate gone; in `android`: the sync line that cannot be
   silenced, notices and refusals in the notice banner, `set_notes` with a base and the conflict sheet,
   "will be refused" marks, the old offline queue and caches removed. State the rule that no other change is
   made to the page, and that `src/phone.html` itself is not edited.
5. **WebView settings and the hardening** (SYNC.md §7; V1-9): bundled assets only through
   `WebViewAssetLoader` at the asset origin; every other request answered in-process or refused with
   403/404; `setBlockNetworkLoads(true)`; no navigation, file access, content access, multiple windows or
   release debugging; the CSP (give it in full: a script hash instead of `'unsafe-inline'`, `connect-src`
   limited to natively answered origins, `frame-src 'none'`, one Trusted Types policy); the document-start
   script (frames, objects and embeds inert; every `RTC*` constructor removed; `link rel` values limited);
   renderer-crash handling; the minimum WebView version and the start-up check that asks for an update
   (old emulator images carry WebView 58, SYNC_PLAN.md §5).
6. **The exfiltration harness**: the channels it must try (fetch, XHR, sockets, WebRTC including through a
   fresh iframe realm, images, styles, forms, navigation, frames, beacons, prefetch and preconnect, service
   worker, `window.open`) against a local sink that must receive nothing; the fallback if a channel cannot
   be closed (SYNC.md "Rejected or modified", V1-9).
7. **The JNI surface**: every native function: name, parameters and result as strings (JSON shapes),
   threading, `catch_unwind` at each entry and what a caught panic returns, lifecycle (open, close), the
   callbacks from Rust into Kotlin (Keystore wrap and unwrap, scheduling a job or an alarm, posting a
   notification) and their contracts.
8. **Android platform rules**: minSdk 26 unless `android-shell` proves a blocker, target and compile SDK,
   the three ABIs, 16 KB pages; permissions (exactly the list, with the reason for each); the manifest
   rules (`allowBackup="false"`, `dataExtractionRules` and `fullBackupContent` excluding everything, no
   cleartext, exported components, immutable PendingIntents); dependencies (`kotlin-stdlib` and
   `androidx.webkit` only, Gradle dependency verification); Keystore key parameters (AES-256-GCM,
   StrongBox if present, not bound to user authentication) and where the wrapped blob lives; the choice
   between the OS trust store and bundled roots per API level as a decision `android-shell` records.
9. **Native screens and administration from the phone** (SYNC_PLAN.md §2): what the phone shows and does
   (fingerprint, banners, alarm evidence, the halted view, pairing itself, accepting a move code), what
   needs API 30 or later with BIOMETRIC_STRONG and no PIN fallback (adding, revoking and replacing
   devices), and what is desktop-only. The code field for pairing: no suggestions, no learning, no
   clipboard; the relay address may be pasted (ruling S4).
10. **Background work**: the socket while visible; a job every 30 minutes; an expedited job on leaving with
    queued edits (API 31 and later); what is promised (only "fresh on open" until measured).
11. **Reminders**: computed in the core, a chain of exact alarms, reminders under 9 minutes apart folded,
    verify before notifying, never a freshness alarm within 9 minutes before a reminder, actions behind
    authentication from API 31 and opening the app below that, `VISIBILITY_PRIVATE`, what is stored in
    device-protected storage (V2-14, V3-13, V1-19).
12. **Budgets** (SYNC_PLAN.md §6.6): cold start to first painted day within 2.5 s, bridge round trip within
    60 ms at the 95th percentile, universal APK under 25 MB, on the API 36 emulator; how the timing harness
    measures each.
13. Error catalogue, Limits, Test vectors (bridge request and reply pairs answered from `fixtures/`),
    Interfaces, Public API (the Kotlin bridge interface, the JNI function list, the page-side `api()`
    contract), Open questions.

## Gaps to close (decide, and say so)
- The exact bridge message envelope and how binary bodies (the background upload) cross it.
- Which page preferences exist and their keys.
- What `android-shell`'s echo bridge must answer so its instrumented tests mean something before the real
  core exists, and which fixtures it serves.
- What the page shows while the bridge is not yet attached, and after the renderer was recreated.
- How the shared picture reaches the page (decoded and re-encoded in Rust, then served by the asset
  interceptor under which URL).
