# R2: Native Android client

> Working paper from the design run of 2026-11-20 that produced [`SYNC.md`](../../../SYNC.md). It is an agent's report, kept for the detail the design compresses: file and line references, sources, and the reasoning behind each requirement. Where it disagrees with `SYNC.md` or [`SYNC_PLAN.md`](../../../SYNC_PLAN.md), those win. Line numbers refer to `day-planner` at commit `1cfff2f`.

# R2 — Native Android client for TaskDeck (research, 2026-11-20)

## Summary

**Recommended: option (c).** Build a small Kotlin shell around a locked-down WebView. The WebView loads the existing `phone.html` from the APK. The Rust core (board, `apply`, snapshot builder, ICS, weather, sync, crypto) is linked into the same process as a `cdylib`, reached through a narrow JNI API that passes strings in and out.

- The page keeps the rule that it "draws and never decides". The only change is that the deciding code now runs on the phone: it is the same Rust that runs on the server today.
- Everything Android-specific is native Kotlin around the core: reminders, push, widgets, calendar export, pairing and key storage.
- Push is FCM data "tickles" with no content. Sync does not depend on them for correctness.
- Distribution uses the user's own signing key.

**Effort:** about 6–8 focused weeks for parity plus the native extras. This sits on top of prerequisites owned by other workstreams: a deterministic core, the relay protocol, and a Rust sync engine.

## 1. Scope: parity, and what changes on a device

**Parity means everything in DOCUMENTATION §21.**
- Views: the agenda with its month rail, the day timeline with hold-to-drag, and the weather view with the plan band.
- The tray with its Unplanned/All/Done tabs.
- The sheet verbs (Done, Book, Tomorrow, armed Delete), each with Undo, and the chip pickers.
- Reflow, Notes, and Look (dials, crop and upload).
- The masthead glance line and the offline behaviour.

**Code facts that make reuse cheap (all VERIFIED):**
- `phone.html` is 5,704 lines: about 835 of CSS and about 4,750 of JS.
- Every `/api/*` call goes through `api()` (phone.html:1240). The only other fetches are the photo cache (2396), Open-Meteo (3127) and the weather SVGs (3949).
- There is a single `innerHTML` sink: the `html:` attribute of `el()` (1073). It is fed constant strings or text passed through the `xml()` escaper (3393).
- The snapshot functions are pure functions of board data and a time: `phone::snapshot` (phone.rs:218), `build_day` (2391), `build_tray` (2509) and `calendar_feed` (2585).
- In chrono 0.4.45 (the version in Cargo.lock), Android tzdata is read by an inlined module ([docs.rs source](https://docs.rs/crate/chrono/0.4.45/source/src/offset/local/unix.rs)).

**What changes meaning on the phone:**
- **Offline edits apply locally** ("confirmed + pending", as on the desktop). Today they only show as "waiting".
- **The `/calendar.ics` feed cannot live on a blind relay.** It is replaced by a CalendarContract export (§6).
- **The desk's place, the palette, the Look dials and the background crop become synced settings or blobs.** The phone re-derives the backdrop with the same `prepare_backdrop_bytes`, with AVIF turned off because the image never travels.
- **Weather is fetched by the phone's Rust `weather.rs`.**

**What native adds:**
- Reminders that fire with no network, with Done, Snooze and Tomorrow actions.
- Widgets, and events in Samsung Calendar.
- A share target and a quick-add tile.
- Native ICS fetching, which has no CORS restriction.
- Biometric app lock.
- No code served by any host.

## 2. Architecture options

| | Effort to parity* | UX | Background | Security | Build (Win/mac) | APK arm64 (ASSUMED) | Reuse |
|---|---|---|---|---|---|---|---|
| **(a) Kotlin + Compose + UniFFI 0.32** | 12–16 wk | best | full | best (no WebView) | AS + cargo-ndk + bindgen | ~8–11 MB | core 100%, page 0% |
| **(b) Tauri 2 mobile** | 7–10 wk | = (c) | awkward | good; big framework | tauri-cli + AS + NDK | ~10–15 MB | page ~95% |
| **(c) Kotlin shell + WebView + JNI** | **6–8 wk** | very good | full | good if locked down | AS + cargo-ndk | **~8–10 MB** | page ~90–95%, core 100% |
| **(d) egui/eframe** | 12+ wk | poor | needs Kotlin anyway | good | cargo-apk/xbuild | large (wgpu) | ui.rs not reusable |

\*Includes the native extras (§4–6), excludes core and relay prerequisites.

**(a) Compose**
- Gives the most native polish.
- But it means rewriting about 4.7k lines of tuned interaction: agenda paging, scroll anchoring, hold-to-drag, the SVG weather chart, the crop UI.
- The JS-side decisions (`nextFreeSlot`, `bookLength`, `glanceFor`) would be re-implemented in Kotlin, a second copy that can drift.
- It leaves a second UI codebase to maintain forever.

**(b) Tauri**
- Reuses the page, but its runtime lives with the Activity.
- WorkManager workers, the FCM service and alarm receivers run without it, so you still write Kotlin plugins *and* a second Rust entry point.
- You get Tauri's useful 20% (WebView, bundled assets, Rust in-process) together with its generated Gradle project and a large dependency tree.

**(d) egui**
- `ui.rs` is a wall-calendar layout (§14.6, UI scale), so a phone UI would be new code.
- Soft-keyboard and IME support on native Android is still weak; a third-party [egui_virtual_keyboard](https://lib.rs/crates/egui_virtual_keyboard) exists as a workaround.
- Background work would still need JNI and Kotlin.

**(e) Other options**
- Flutter + flutter_rust_bridge 2.12: adds Dart and a runtime, and the UI is a rewrite.
- Slint: Android backend via android-activity; the free license requires attribution.
- Capacitor: brings an npm supply chain.
- None of these beats (c).

**Why (c) wins.**
- It is the only option where parity is mostly plumbing, and the plumbing is simple. `api()` becomes a message to native code, which calls the same Rust handlers `phone.rs` serves today.
- Background capability is fully native.
- Native code is the only thing that can reach the data. The WebView itself has no network (§7).
- The WebView renderer runs sandboxed in its own process, and Chromium supplies IME and accessibility.
- Insets: the page already uses `viewport-fit=cover` and `env(safe-area-inset-*)`. WebView forwards these insets from M136 for fullscreen WebViews, and M139 adds keyboard (IME) resizing ([Chromium insets doc](https://chromium.googlesource.com/chromium/src.git/+/main/android_webview/docs/insets.md)).

**FFI choice.**
- Use `jni` 0.22.4 with a JSON-string API. The existing wire shapes (Command JSON, Snapshot JSON) pass straight through, and Kotlin needs only the built-in `org.json`.
- UniFFI 0.32.2 is the alternative if the typed surface grows. It pulls in JNA, which needs version 5.17 or later for 16 KB pages (UNCERTAIN; verify the packaged `.so` alignment).

## 3. Android platform constraints (Nov 2026)

**Doze** ([doc](https://developer.android.com/training/monitoring-device-state/doze-standby))
- Network is suspended. Jobs, WorkManager, standard alarms and `setExact` are deferred to maintenance windows.
- `set(Exact)AndAllowWhileIdle` fires at most **once per 9 minutes per app**.
- `setAlarmClock` exits Doze shortly before it fires.

**Standby buckets** ([limits](https://developer.android.com/topic/performance/power/power-details))
- Alarms: working set 10/h, frequent 2/h, rare 1/h, restricted 1/day.
- Regular jobs in the rare bucket get 10 min per 24 h.
- From Android 16, jobs started while visible or alongside a foreground service count against the quota ([A16](https://developer.android.com/about/versions/16/behavior-changes-all)).
- An app running a foreground service has unrestricted network.

**WorkManager**
- Periodic work has a **15-minute minimum** ([doc](https://developer.android.com/develop/background-work/services/alarms)).
- Expedited work gets a brief exemption right after a high-priority FCM message ([FCM](https://firebase.google.com/docs/cloud-messaging/android-message-priority)).

**Foreground services** ([types](https://developer.android.com/develop/background-work/services/fgs/service-types), [timeouts](https://developer.android.com/develop/background-work/services/fgs/timeout))
- Since Android 14, every foreground service must declare a type.
- From Android 15, `dataSync` is capped at 6 h per 24 h.
- From Android 15, apps can no longer start a `dataSync` service from `BOOT_COMPLETED`.
- `specialUse` is free-form, but its stated use case is reviewed only by Play.
- `systemExempted` is allowed for apps that hold an exact-alarm permission.

**A persistent background WebSocket** is possible only with a foreground service and its permanent notification. For a sideloaded app that is technically viable (`specialUse`). It costs battery for keepalives through carrier NAT, and Samsung's sleeping lists still bite. The platform's answer is FCM's shared connection. **Not recommended.** Keep a WebSocket open only while the app is visible.

**Exact alarms** ([A14](https://developer.android.com/about/versions/14/changes/schedule-exact-alarms))
- `SCHEDULE_EXACT_ALARM` is denied by default for new installs targeting 33+ on Android 14+, and the user or system can revoke it.
- `USE_EXACT_ALARM` is granted at install, cannot be revoked, and is meant for calendar and alarm apps. Play restricts publishing it to apps that qualify, which does not apply to a sideloaded APK.
- Exact alarms are exempt from foreground-service start restrictions.
- All alarms are cleared on reboot.

**`POST_NOTIFICATIONS`** is a runtime permission from Android 13, and notifications are off for new installs until granted ([doc](https://developer.android.com/develop/ui/views/notifications/notification-permission)).

**Android 17 (API 37)** — changes for apps targeting it ([doc](https://developer.android.com/about/versions/17/behavior-changes-17)):
- Certificate Transparency is on by default, and ECH is used.
- New `ACCESS_LOCAL_NETWORK` permission.
- Native libraries loaded with `System.load()` must be read-only.
- A memory cap on widget (RemoteViews) bitmaps.
- `FLAG_SECURE` is the only way to opt out of Content Capture.
- **16 KB pages:** NDK r28+ aligns by default and AGP must be ≥ 8.5.1 ([doc](https://developer.android.com/guide/practices/page-sizes)). AGP 9.5 ships NDK 28.2 and supports API 37 ([AGP](https://developer.android.com/build/releases/gradle-plugin)).

**Samsung One UI** ([Samsung dev](https://developer.samsung.com/mobile/app-management.html))
- *Sleeping*: unused about 3 days and hurting system health. Jobs, alarms and foreground services are restricted through the standby bucket.
- *Deep sleeping*: unused about 16 days. Nothing runs in the background, notifications included.
- *Never sleeping*: the user's manual exemption list (Device care → Battery → Background usage limits).
- The app can deep-link to these lists with action `com.samsung.android.sm.ACTION_OPEN_CHECKABLE_LISTACTIVITY`, package `com.samsung.android.lool`, extra `activity_type` = 2.
- Also set App info → Battery → Unrestricted.
- A sideloaded app may request `ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`. Play restricts this, but Play policy does not apply here.
- **Auto Blocker** (One UI 6.1.1+) blocks installs from unauthorized sources; Samsung's advice is to turn it off briefly to install ([Samsung](https://www.samsung.com/uk/support/mobile-devices/protect-your-galaxy-device-with-the-new-auto-blocker-feature/)). Whether it blocks `adb` is UNCERTAIN.

## 4. Prompt background sync: push

**FCM.** It is no-cost ([pricing](https://firebase.google.com/pricing)). Sending works like this:
- POST to `https://fcm.googleapis.com/v1/projects/<id>/messages:send` with a Bearer token for scope `firebase.messaging` ([doc](https://firebase.google.com/docs/cloud-messaging/send/v1-api)).
- Inside the Worker or Durable Object, get the token with an RS256 JWT signed by the service-account key through WebCrypto ([workers-jwt](https://github.com/sagi/workers-jwt)), or with about 60 lines by hand.
- Cache the token for about an hour. Each push is one subrequest.

**FCM changed in 2026.**
- *AndroidConfigV2* (preview) splits messages into `remote_notification` (urgent, user-visible) and `background_sync` (may be batched or delayed) ([doc](https://firebase.google.com/docs/cloud-messaging/send/v2-api)).
- The Android Doze page calls v1 high priority deprecated.
- High-priority messages that never lead to a notification get deprioritized.
- Design for the slow path: a push is a *hint*, never a dependency.

**Proposed flow**
1. After a commit, the Durable Object coalesces pushes (at most one per device about every 10 s).
2. It sends a data message with a constant payload such as `{"s":"1"}`, `collapse_key:"sync"`, a TTL of about 1 h, and normal priority (`background_sync` once GA).
3. On the phone, `onMessageReceived` enqueues expedited work that runs `core.sync_once()`.
4. Then it reconciles alarms, widgets and the calendar export.

**Safety nets**
- A WebSocket to the hibernating Durable Object while the app is visible.
- Periodic WorkManager sync every 60 minutes, network required.
- **Verify-before-notify** at alarm time (§5).

**What Google learns.** It learns the Firebase installation ID and token (kept until deleted via the API; [privacy](https://firebase.google.com/support/privacy)), the package, and message timing, size and collapse key. That tells it *when* something changed, but nothing about content.

**A leaked push credential** only lets someone send tickles. Push content is never trusted, so this is harmless.

**Alternatives**
- **UnifiedPush/ntfy:** needs a distributor app with its own connection, and the ntfy operator sees timing. It is one more moving part (the burden the user disliked with Tailscale). Keep it only as a de-Googled fallback.
- **No push:** fine for viewing, since opening the app syncs in about a second. But widgets, the system calendar and reminders can be hours stale in Doze. Offer it as a privacy toggle that falls back to periodic sync.

## 5. Reminders with no network

- **Rust computes the schedule.** It reads the local board (confirmed + pending) through `reminders(from, to)`. Each entry has a stable key, an absolute instant (resolved in the board's zone), a title and body, an item id and a kind.
- **Defaults:**
  - events and booked sessions: at start, or a lead time;
  - due markers: deadline minus a lead time;
  - routines: off.
- **One alarm at a time, as a chain.** Use `setExactAndAllowWhileIdle(RTC_WAKEUP)` with `USE_EXACT_ALARM`, plus `SCHEDULE_EXACT_ALARM` with `maxSdkVersion="32"`. Fold reminders less than 9 minutes apart into one firing, because of the Doze cap.
- **When the alarm fires:**
  1. The receiver (not exported) uses `goAsync()` to make a best-effort sync of 5 s or less. Network access under the temporary allowlist is ASSUMED; test with `dumpsys deviceidle force-idle`.
  2. It recomputes from the local board.
  3. It posts or skips the notification.
  4. It schedules the next alarm.
- **Reschedule on:** start, every board version change, `BOOT_COMPLETED`, `TIME_SET`, `TIMEZONE_CHANGED`, `MY_PACKAGE_REPLACED` and permission-state changes.
- **Channels:** Reminders and Due (high importance), and Sync problems (low).
- **Actions:** Done (`Complete`, stamped at origin), Tomorrow (`MoveBlock` +1 day) and Snooze (a local re-alarm only). All use immutable PendingIntents.
- **Lock screen:** `VISIBILITY_PRIVATE` with a generic public version.
- **Before first unlock after a reboot**, the board's encrypted storage is unreadable, so reminders wait for unlock. Storing titles in device-protected storage would leak them; offer it only as an opt-in.
- **Health screen:** shows notifications enabled, `canScheduleExactAlarms`, ignoring battery optimizations, `getAppStandbyBucket()`, and the Samsung Never-sleeping deep link.

## 6. System integration

**CalendarContract export (opt-in)**
- Create calendars under its own account type (stub authenticator), as ICSx⁵-style apps do; `ACCOUNT_TYPE_LOCAL` is the fallback ([doc](https://developer.android.com/reference/android/provider/CalendarContract.Calendars)).
- One calendar per feed layer: Events, Due, Plan, and Routines (off). Users can toggle each in Samsung Calendar.
- Set `CAL_ACCESS_READ` so the export stays one-way, like the feed.
- Diff by a stable key in `SYNC_DATA1`. Upsert in batches over a window of −30 to +180 days.
- Write no Reminders rows, to avoid double alerts.
- Needs READ_ and WRITE_CALENDAR.
- **Privacy cost:** any app holding READ_CALENDAR can read these events. Whether Samsung Calendar lists these calendars is UNCERTAIN; test on the device.

**Widgets**
- A "Today" widget (glance line plus the next 3–5 rows) and a tray widget.
- Show absolute times; a `Chronometer` countdown avoids per-minute updates.
- Glance 1.2.0 (stable 2026-08-26) works, or plain RemoteViews avoids the Compose toolchain.
- Android 17 caps widget bitmap memory, so use no photo or a downsampled one.

**Quick add**
- Share target: `ACTION_SEND text/plain` opens a QuickAdd sheet. The shared text is untrusted and is never auto-applied.
- A Quick Settings `TileService` and a static "New task" shortcut.

**Inside the WebView**
- Implement `onShowFileChooser` with the Photo Picker; no storage permission is needed.
- Route "Use my location" over the bridge to `LocationManager.getCurrentLocation` with coarse location, which needs no Play services.
- Implement `onRenderProcessGone` so a crashed renderer is recreated.

## 7. Security on Android

**Keys**
- Wrap the board key with a non-exportable Android Keystore key: AES-256-GCM, StrongBox if present with fallback otherwise.
- Do **not** bind it to user authentication, because background sync and reminders need it while the phone is locked.
- Store the wrapped blob in `noBackupFilesDir`.
- Biometric is only an optional UI app-lock (BiometricPrompt, STRONG or device credential).
- `EncryptedSharedPreferences` is deprecated, so use Keystore directly.

**Backups**
- Set `allowBackup="false"` *and* `dataExtractionRules` excluding everything under cloud-backup, device-transfer and cross-platform-transfer. On some OEMs, `allowBackup=false` alone does not stop device-to-device transfer for targetSdk 31+ ([doc](https://developer.android.com/identity/data/autobackup)).
- Add `fullBackupContent` excludes for Android 11 and lower.
- The WebView's localStorage lives in the app directory and is covered by these rules.

**WebView lockdown**
- Load only `https://appassets.androidplatform.net/...` through `WebViewAssetLoader`.
- In `shouldInterceptRequest`, answer **404 or 403 for everything else**. Unmatched paths otherwise fall through to the network ([doc](https://developer.android.com/develop/ui/views/layout/webapps/load-local-content)).
- `shouldOverrideUrlLoading` always returns true, so the WebView never navigates.
- `allowFileAccess` and `allowContentAccess` off; mixed content never allowed; no multiple windows; debugging off in release.
- The bridge is `WebViewCompat.addWebMessageListener` with `allowedOriginRules` set to the asset origin. Do not use `addJavascriptInterface`, which is exposed to every frame without an origin check ([doc](https://developer.android.com/privacy-and-security/risks/insecure-webview-native-bridges)).
- Content Security Policy: `connect-src 'none'`, with the inline script pinned by a build-time hash instead of `'unsafe-inline'`.
- Worst case, an XSS from a hostile ICS feed gets the same verbs a token holder has today and cannot exfiltrate anything.

**Network**
- The network security config forbids cleartext and trusts system CAs only.
- Rust uses rustls with webpki-roots and https-only, and ignores user-installed CAs.
- Because the payload is end-to-end encrypted and authenticated, skip certificate pinning; Cloudflare certificate rotation would make it brittle.

**Components and processes**
- Export only the launcher, share activity, tile, widget receiver and boot/time receivers. Validate the action on each.
- All PendingIntents immutable.
- **Single process.** Do not use `android:process`, and keep one Mutex-guarded core singleton. This is how §4.1's "one writer" invariant holds on the phone.

**Screen**
- Optional "hide from screenshots" via `FLAG_SECURE`. On Android 17 this also blocks Content Capture.
- An optional milder setting: turn off the recents screenshot only.

**Supply chain**
- Gradle dependency verification metadata, pinned versions, a small set of dependencies, and `cargo deny` checks.

## 8. QR pairing

- **Recommended: CameraX 1.7.0 with ZXing core 3.5.4.** ZXing core is pure Java under Apache-2.0, with no native code, no network and no Play services.
- **zxing-cpp 3.1.1** is faster but native; check its 16 KB alignment.
- **Avoid ML Kit.** It sends device and app info, a per-installation ID and performance metrics to Google ([disclosure](https://developers.google.com/ml-kit/android-data-disclosure)). Google Code Scanner would also route the key through Play services.
- **Never pair through the system camera app or a custom-scheme deep link.** Another app can register the same scheme and intercept the key.
- Show a short confirmation code on both screens after pairing.

## 9. Distribution and updates

**Developer verification** ([FAQ](https://developer.android.com/developer-verification/guides/faq.md.txt), [blog](https://android-developers.googleblog.com/2026/06/android-developer-verification.html))
- Enforced from 2026-09-30 in Brazil, Indonesia, Singapore and Thailand, and only for installs from participating stores. Sideloads are unaffected for now.
- Global rollout is planned for 2027, on certified devices running Android 8+.
- **`adb` installs are exempt.**
- The "advanced flow" is a one-time setup: developer options, an anti-coercion check, a restart, a 24-hour wait, then biometric confirmation. After that, unregistered apps install with a warning, enabled for 7 days or indefinitely. Updates also need it.
- **Limited distribution accounts** are free, need no government ID, and require a Google account with 2-step verification plus a payments profile ([guide](https://developer.android.com/developer-verification/guides/limited-distribution.md.txt)). The device limit is 20 in the FAQ but 50 in the guide.

**Options**
1. **Own key, then `adb install -r`, or a plain sideload.**
   - Android allows an update only if the certificate matches ([doc](https://developer.android.com/studio/publish/app-signing)). Nobody — GitHub, the relay, a mirror — can push code to the phone without the user's offline signing key.
   - Keep Auto Blocker on between updates.
2. **Register the package and certificate with a Limited-Distribution account before 2027**, so normal installs keep working after enforcement.
3. **Play internal testing** (up to 100 testers): auto-updates, but Play App Signing means **Google holds the signing key**, plus a $25 fee and ID verification. Not recommended for this threat model.
4. **Obtainium or a self-hosted F-Droid repo** for update notifications only. Signature continuity still guards the install.
5. **Play Protect** scans sideloaded apps. Its enhanced fraud protection targets SMS, accessibility and notification-listener permissions, none of which TaskDeck requests.

## 10. ICS subscriptions on the phone

- Native code has no CORS. `subscriptions.rs` runs unchanged in a WorkManager worker: blocking reqwest with rustls, https-only, at most 3 redirects, an 8 MiB cap and a 20 s timeout.
- **Cut the cadence.** It is 600 s today (subscriptions.rs:97). Use 1–3 h in the background, plus on open if older than 30 minutes.
- **Add conditional GET and gzip.** There is no ETag handling today. Both make repeat fetches nearly free.
- **Option for the relay agents:** share the derived overlay through the relay as an encrypted blob, so the phone fetches only when it is stale. The overlay is still never turned into board items.

## Recommendation

**Stack**

*Android*
- AGP 9.5.x, NDK r28 (16 KB by default).
- compileSdk 37; targetSdk 37 (36 until tested); **minSdk 29**, the Android 10 floor where the WebView still gets updates.
- `androidx.webkit` 1.18.0, WorkManager 2.12.0, CameraX 1.7.0, ZXing core 3.5.4.
- Firebase BoM 35.2.0 (messaging 26.0.2).
- Glance 1.2.0 or plain RemoteViews.
- Kotlin, with no Compose for the main UI.

*Rust*
- `jni` 0.22.4, cargo-ndk 4.1.2.
- Targets `aarch64-linux-android`, plus `x86_64` for the emulator.
- rustls with ring and webpki-roots.

**Module layout**
```
crates/taskdeck-core     board, tasks, archive, planner, ics, subscriptions, weather,
                         snapshot (split out of phone.rs), sync+crypto (R1/R3)
crates/taskdeck-android  cdylib: open(dir,zone), handle(route,body), version(),
                         sync_once(), live_start/stop(), reminders(), widget_model(),
                         calendar_rows(), refresh_subscriptions()
android/app              MainActivity (WebView+bridge), Pairing, Onboarding/Health,
                         QuickAdd(share+tile), SyncWorker/OutboxWorker/PeriodicWorker,
                         PushService, ReminderScheduler+AlarmReceiver+BootReceiver,
                         Widget, CalendarExporter(+stub authenticator), KeyWrap
                         assets/: phone.html copied from src/ at build (single source)
```

**Effort**
| Work | Weeks |
|---|---|
| Crate split + JNI + Gradle cargo task | 1 |
| Hardened WebView + bridge + page transport shim (picker, location, insets) | 1.5–2 |
| Sync glue, push, workers | 1–1.5 |
| Pairing + Keystore | 1 |
| Reminders | 1–1.5 |
| Widgets, share target, tile | 1 |
| Calendar export | 1–1.5 |
| **Total** | **≈7–9.5** |

That comes to about 6–8 weeks for someone already comfortable with Android; add 1–2 weeks if Android is new. A Compose UI adds about 6–8 weeks.

**Delivery order**
1. Shell, core and sync: replaces the PWA.
2. Reminders.
3. Widgets and tile.
4. Calendar export.

---

## Key facts

- **[VERIFIED]** phone.html is 5,704 lines (~835 CSS, ~4,750 JS); all /api/* traffic goes through api() at line 1240; the only other fetches are the photo cache (2396), Open-Meteo (3127) and weather SVGs (3949) _(evidence: wc -l and grep on src/phone.html in the dp worktree; phone.html:1240, 2396, 3127, 3949)_
- **[VERIFIED]** phone.html has a single innerHTML sink: el()'s 'html' attribute (line 1073), used with constant strings or xml()-escaped text (xml() at line 3393) _(evidence: grep innerHTML src/phone.html -> 1 hit at 1073; uses at 3604, 3755, 3759-3761; escaper at 3393)_
- **[VERIFIED]** phone::snapshot (phone.rs:218), build_day (2391), build_tray (2509) and calendar_feed (2585) are functions of board data, palette and a time, so the same layout code can run on the phone _(evidence: src/phone.rs:218-312, 2391, 2509, 2585)_
- **[VERIFIED]** Subscriptions refresh every 600 s, cap at 8 MiB, time out after 20 s, and have no conditional-GET/ETag handling _(evidence: src/subscriptions.rs:93-97; grep for etag/if-none-match/last-modified returns nothing)_
- **[VERIFIED]** chrono 0.4.45 reads Android tzdata through an inlined tz_data module, so chrono Local works on Android _(evidence: https://docs.rs/crate/chrono/0.4.45/source/src/offset/local/unix.rs; Cargo.lock chrono 0.4.45)_
- **[ASSUMED]** The library is meant to build without winit/android-activity via --no-default-features ('desk' feature gating); an actual aarch64-linux-android build has not been tried _(evidence: Cargo.toml [features] comments; DOCUMENTATION.md §22.1; cargo not run for Android)_
- **[VERIFIED]** Android developer verification is enforced from 2026-09-30 only for installs from participating stores in Brazil, Indonesia, Singapore and Thailand, on certified devices running Android 8+; global rollout is planned for 2027; sideloads outside participating stores are unaffected for now _(evidence: https://developer.android.com/developer-verification ; https://developer.android.com/developer-verification/guides/faq.md.txt (updated 2026-07-15 and 2026-11-10))_
- **[VERIFIED]** adb installs are exempt from verification; the advanced flow is a one-time setup (developer options, coaching check, restart, 24 h wait, biometric/PIN) and can be enabled for 7 days or indefinitely; unregistered apps can only be updated while it is enabled or via adb _(evidence: https://developer.android.com/developer-verification/guides/faq.md.txt)_
- **[UNCERTAIN]** Limited distribution accounts are free, need no government ID, and require a Google account with 2SV plus a payments profile; the device limit conflicts between sources (20 in the FAQ and blog, 50 in the guide) _(evidence: https://developer.android.com/developer-verification/guides/limited-distribution.md.txt (50) vs FAQ (20))_
- **[VERIFIED]** SCHEDULE_EXACT_ALARM is denied by default for new installs targeting 33+ on Android 14+; USE_EXACT_ALARM is granted at install, cannot be revoked and is intended for calendar/alarm apps; Play restricts publishing it to qualifying apps; setExact, setExactAndAllowWhileIdle and setAlarmClock all require one of the two _(evidence: https://developer.android.com/about/versions/14/changes/schedule-exact-alarms ; https://developer.android.com/develop/background-work/services/alarms)_
- **[VERIFIED]** Exact alarms are exempt from foreground-service launch restrictions; all alarms are cancelled on shutdown, so a BOOT_COMPLETED receiver must reschedule _(evidence: https://developer.android.com/develop/background-work/services/alarms)_
- **[VERIFIED]** In Doze, setAndAllowWhileIdle/setExactAndAllowWhileIdle fire at most once per 9 minutes per app; setAlarmClock alarms make the system exit Doze; WorkManager/JobScheduler do not run in Doze _(evidence: https://developer.android.com/training/monitoring-device-state/doze-standby)_
- **[VERIFIED]** Standby-bucket alarm limits are working set 10/h, frequent 2/h, rare 1/h, restricted 1/day; an app running a foreground service has unrestricted network; high-priority FCM has no execution limits in Doze _(evidence: https://developer.android.com/topic/performance/power/power-details)_
- **[VERIFIED]** WorkManager periodic work has a 15-minute minimum interval _(evidence: https://developer.android.com/develop/background-work/services/alarms (use-cases section))_
- **[VERIFIED]** For apps targeting Android 15+, dataSync and mediaProcessing foreground services are capped at 6 h per 24 h, and dataSync cannot be started from BOOT_COMPLETED; specialUse is free-form but its use case is reviewed by Play; systemExempted is allowed for apps holding SCHEDULE_EXACT_ALARM or USE_EXACT_ALARM _(evidence: https://developer.android.com/develop/background-work/services/fgs/timeout ; https://developer.android.com/develop/background-work/services/fgs/service-types)_
- **[VERIFIED]** From Android 16, jobs started while the app is visible or while a foreground service runs count against the job runtime quota _(evidence: https://developer.android.com/about/versions/16/behavior-changes-all)_
- **[VERIFIED]** For targetSdk 37 (Android 17): Certificate Transparency is on by default, ECH is used, ACCESS_LOCAL_NETWORK is required for LAN access, System.load() needs read-only native files, RemoteViews bitmap memory is capped, and FLAG_SECURE is the only way to disable Content Capture _(evidence: https://developer.android.com/about/versions/17/behavior-changes-17)_
- **[VERIFIED]** NDK r28+ builds 16 KB-aligned by default; AGP >= 8.5.1 is required; Play blocks updates without 16 KB support from 2027-02-01; Android 16 runs 4 KB-aligned apps in a compat mode that shows a dialog _(evidence: https://developer.android.com/guide/practices/page-sizes ; https://developer.android.com/about/versions/16/behavior-changes-all)_
- **[VERIFIED]** AGP 9.5 supports up to API 37 and defaults to NDK 28.2.13676358 and JDK 17 _(evidence: https://developer.android.com/build/releases/gradle-plugin)_
- **[VERIFIED]** Current versions: androidx.webkit 1.18.0 (2026-10-21), WorkManager 2.12.0 (minSdk 24), Glance 1.2.0 (2026-08-26), CameraX 1.7.0 (2026-11-18), Firebase BoM 35.2.0 with firebase-messaging 26.0.2 (minSdk 24), ZXing core 3.5.4, zxing-cpp android 3.1.1, uniffi 0.32.2, jni 0.22.4, cargo-ndk 4.1.2 _(evidence: androidx release pages for webkit, work, glance and camera; https://firebase.google.com/support/release-notes/android ; Maven Central metadata; crates.io API)_
- **[VERIFIED]** FCM is no-cost on both plans; HTTP v1 sends go to fcm.googleapis.com/v1/projects/<id>/messages:send with an OAuth token for scope firebase.messaging _(evidence: https://firebase.google.com/pricing ; https://firebase.google.com/docs/cloud-messaging/send/v1-api)_
- **[VERIFIED]** FCM AndroidConfigV2 (preview, 2026) splits messages into RemoteNotification and BackgroundSync (which may be batched or delayed); Android's Doze page calls v1 high priority deprecated; high-priority messages that do not produce notifications get deprioritized _(evidence: https://firebase.google.com/docs/cloud-messaging/send/v2-api ; https://developer.android.com/training/monitoring-device-state/doze-standby ; https://firebase.google.com/docs/cloud-messaging/android-message-priority)_
- **[VERIFIED]** FCM processes Firebase installation IDs to route messages and keeps them until the customer deletes them via the API _(evidence: https://firebase.google.com/support/privacy)_
- **[ASSUMED]** The FCM service-account JWT (RS256) can be signed with WebCrypto inside Cloudflare Workers (community library @sagi.io/workers-jwt) _(evidence: https://github.com/sagi/workers-jwt ; not tested; Workers free-plan CPU cost not measured)_
- **[VERIFIED]** Samsung sleeping apps: unused ~3 days and hurting system health, restricted via the standby bucket (jobs, alarms, FGS); deep sleeping: unused ~16 days, no background activity or notifications; Never sleeping is the user's manual exemption; deep-link action com.samsung.android.sm.ACTION_OPEN_CHECKABLE_LISTACTIVITY with extra activity_type=2 _(evidence: https://developer.samsung.com/mobile/app-management.html)_
- **[UNCERTAIN]** Samsung Auto Blocker (One UI 6.1.1+) blocks app installs from unauthorized sources; Samsung advises turning it off temporarily to install; whether it blocks adb is not documented _(evidence: https://www.samsung.com/uk/support/mobile-devices/protect-your-galaxy-device-with-the-new-auto-blocker-feature/)_
- **[VERIFIED]** WebViewAssetLoader falls back to the internet for unmatched paths, so an app must answer unmatched requests itself; file:// and the allowFileAccessFromFileURLs settings are discouraged _(evidence: https://developer.android.com/develop/ui/views/layout/webapps/load-local-content)_
- **[VERIFIED]** addJavascriptInterface exposes the object to every frame with no origin check; message-channel bridges need explicit origin checks (addWebMessageListener takes allowedOriginRules) _(evidence: https://developer.android.com/privacy-and-security/risks/insecure-webview-native-bridges ; https://developer.android.com/jetpack/androidx/releases/webkit)_
- **[VERIFIED]** Android WebView forwards safe-area insets to CSS env() from M136 for fullscreen WebViews and adds IME (keyboard) insets in M139; phone.html already uses viewport-fit=cover and env(safe-area-inset-*) _(evidence: https://chromium.googlesource.com/chromium/src.git/+/main/android_webview/docs/insets.md ; phone.html:5, 69-70)_
- **[VERIFIED]** For targetSdk 31+, allowBackup=false may not disable device-to-device transfer on some OEMs, so dataExtractionRules (cloud-backup, device-transfer and, from Android 16 QPR2, cross-platform-transfer) must exclude data explicitly _(evidence: https://developer.android.com/identity/data/autobackup)_
- **[VERIFIED]** ML Kit (bundled models included) sends device info, app package and version, a per-installation identifier and performance diagnostics to Google over HTTPS _(evidence: https://developers.google.com/ml-kit/android-data-disclosure)_
- **[VERIFIED]** Android allows an update only if the new APK's certificates match the installed app's; Play App Signing is required for new Play apps, and Google then manages (and may back up) the app signing key _(evidence: https://developer.android.com/studio/publish/app-signing)_
- **[VERIFIED]** From 2026-08-31, Play requires new apps and updates to target API 36; internal testing allows up to 100 testers; personal accounts created after 2023-11-13 need a 12-tester, 14-day closed test before production _(evidence: https://support.google.com/googleplay/android-developer/answer/11926878 ; https://support.google.com/googleplay/android-developer/answer/14151465)_
- **[VERIFIED]** Play Protect enhanced fraud protection targets internet-sideloaded apps that request SMS/OTP-style permissions; it was rolled out in Malaysia in April 2026 after pilots in other regions _(evidence: https://blog.google/intl/en-my/company-news/technology/google-launches-enhanced-protection-in-google/)_
- **[VERIFIED]** Durable Object WebSocket hibernation answers protocol pings without waking the object; setWebSocketAutoResponse handles application-level pings without duration charges _(evidence: https://developers.cloudflare.com/durable-objects/best-practices/websockets)_
- **[UNCERTAIN]** JNA (used by UniFFI's Kotlin bindings) gained 16 KB page support in 5.17.0 _(evidence: Secondary: Mozilla bug 1981506 and JNA issue #1647 via search; JNA release notes not checked)_
- **[ASSUMED]** A sideloaded app gets network access for a few seconds when an allow-while-idle exact alarm fires in Doze (temporary power allowlist), which makes verify-before-notify workable _(evidence: AOSP behaviour from memory; not confirmed in the docs fetched; test with adb shell dumpsys deviceidle force-idle)_
- **[ASSUMED]** Effort for option (c): ~7–9.5 focused weeks including native extras (about 6–8 for someone comfortable with Android), excluding the core-determinism and relay prerequisites; a Compose UI rewrite adds ~6–8 weeks _(evidence: Estimate from the code inventory (phone.html 5.7k lines, phone.rs handlers) and the component breakdown in the report)_
- **[ASSUMED]** APK size for option (c), arm64 only, is ~8–10 MB (Rust .so without the AVIF encoder ~3–5 MB plus AndroidX, Firebase, CameraX and ZXing after R8) _(evidence: Estimate; the Linux server binary is 8.5 MB including rav1e and tiny_http (session fact); not built for Android)_

## Recommendations

- Build option (c): a minimal Kotlin shell hosting a hardened WebView that loads src/phone.html from APK assets via WebViewAssetLoader, with the Rust core in-process as a cdylib behind a narrow string-in/string-out JNI API (jni 0.22.4). Do not use Tauri, egui or a Compose rewrite for the main UI.
- Keep every decision in Rust: the snapshot layout (phone::snapshot/build_day), the reminder schedule, the widget model and the calendar-export rows. Kotlin only schedules and displays. Over time, move JS-side decisions such as glanceFor and nextFreeSlot into Rust so widgets and notifications share them.
- Run a single Android process with one Mutex-guarded core singleton and no android:process attributes; this is how the 'one writer' invariant (DOCUMENTATION §4.1) holds on the phone.
- Change api() in phone.html so that inside the app it posts to a WebMessageListener bridge restricted to the https://appassets.androidplatform.net origin. Answer 403/404 for every other request in shouldInterceptRequest, use connect-src 'none' with a build-time script hash, never navigate, and turn off file/content access. Route the file picker (Photo Picker) and 'Use my location' (LocationManager, coarse) through native code.
- Push: from the Durable Object, send coalesced FCM data tickles over the HTTP v1 endpoint with a constant payload, collapse_key, a short TTL and normal priority (background_sync once AndroidConfigV2 is GA), signed with a service-account JWT through WebCrypto. Never trust push content. Back it with a foreground WebSocket while the app is visible, hourly periodic WorkManager sync and verify-before-notify. Offer a no-push privacy toggle.
- Reminders: Rust computes them from confirmed plus pending state. Use a chain of single setExactAndAllowWhileIdle alarms with USE_EXACT_ALARM (plus SCHEDULE_EXACT_ALARM with maxSdkVersion 32), and fold reminders less than 9 minutes apart into one firing. At alarm time, sync briefly, recompute, then post. Reschedule on boot, time or zone change, package replacement and every board version change. Notification actions are Done, Tomorrow and Snooze; lock-screen notifications use VISIBILITY_PRIVATE.
- Add a Samsung onboarding and health screen: POST_NOTIFICATIONS, Unrestricted battery, ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS (fine for a sideloaded app), a deep link to Never sleeping apps, and readouts of canScheduleExactAlarms and getAppStandbyBucket.
- Security: wrap the board key with a Keystore AES-256-GCM key (StrongBox if available, no user-auth binding) kept in noBackupFilesDir. Set allowBackup=false and dataExtractionRules excluding all transfer types. Use a network security config with no cleartext and system CAs only, immutable PendingIntents and the minimum of exported components. Offer FLAG_SECURE and a biometric app lock as options. Enable Gradle dependency verification and cargo-deny.
- Pairing: scan in-app with CameraX 1.7.0 and ZXing core 3.5.4 (no ML Kit, no Play services code scanner), never through the system camera or a custom-scheme deep link, and show a confirmation code on both screens.
- Distribution: sign with the user's own key (25+ year validity, backed up offline) and install or update with adb or a direct sideload, keeping Samsung Auto Blocker on between updates. Register the package and certificate with a free Limited Distribution account before the 2027 global verification rollout. Avoid Play internal testing, because Play App Signing hands the signing key to Google.
- ICS on the phone: run subscriptions.rs in a WorkManager worker every 1–3 h, plus on open if older than 30 minutes; add ETag/If-Modified-Since and gzip. The relay agents could alternatively share the derived overlay as an encrypted blob so the phone fetches only when it is stale.
- Stack: AGP 9.5.x, NDK r28 (16 KB by default), compileSdk 37, targetSdk 37 (36 until tested), minSdk 29; androidx.webkit 1.18.0, WorkManager 2.12.0, CameraX 1.7.0, ZXing 3.5.4, Firebase BoM 35.2.0, Glance 1.2.0 or RemoteViews; cargo-ndk 4.1.2 targeting aarch64-linux-android (plus x86_64 for the emulator); rustls with ring and webpki-roots. Ship in phases: shell, core and sync first (replaces the PWA), then reminders, then widgets and tile, then calendar export.

## Open questions

- Which exact phone model and One UI/Android version is it? That decides minSdk and whether Samsung Auto Blocker blocks adb or wireless-debugging installs.
- Does the user accept FCM's metadata exposure to Google (installation ID, timing and size of tickles, no content)? If not, ship the no-push mode (foreground WebSocket plus periodic sync) or ntfy/UnifiedPush.
- Should TaskDeck events be exported to the system calendar (Samsung Calendar, Galaxy Watch), given that any app with READ_CALENDAR can then read them? Does Samsung Calendar list calendars from a custom account type or from ACCOUNT_TYPE_LOCAL? This needs a device test.
- Will a browser or PWA path remain for any device? If so, phone.html must keep both transports (fetch and bridge); if not, its service worker, token gate and outbox code can be removed from the app build.
- Board time-zone policy for multi-writer apply (R1): a fixed board zone using chrono-tz, or the device zone? Reminders and snapshots on a travelling phone depend on it.
- Is network actually available when an allow-while-idle exact alarm fires in deep Doze on One UI? This decides how much verify-before-notify can be relied on; test with dumpsys deviceidle.
- When does FCM AndroidConfigV2 reach GA, and when is v1 high priority actually retired? Either affects the push message format.
- Should reminder titles be stored in device-protected storage so reminders fire before the first unlock after a reboot (a privacy trade-off)?
- Default for screenshot protection (FLAG_SECURE) and lock-screen notification content.
- If UniFFI is chosen instead of plain JNI: is the packaged libjnidispatch.so from JNA ≥5.17 16 KB-aligned in the final APK?
- Should the derived ICS overlay be fetched per device or shared through the relay as an encrypted blob (decision for R1/R3)?
- Is the user willing to register a free Limited Distribution account (Google account with 2SV and payments profile) before the 2027 enforcement, or will they rely on adb or the advanced flow indefinitely?
