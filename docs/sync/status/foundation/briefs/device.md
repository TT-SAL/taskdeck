# Brief: `docs/sync/spec/device.md` (owning stage: `device`)

Scope (SYNC_PLAN.md §6.1 item 3): "the facade the two apps use, the on-disk store and log format, transport,
test-only trust (§3.4), the key-store interface, fetcher rules, blobs, backup scheduling". The `device`
stage (§6.7) builds the device runtime in `crates/taskdeck-device`: store, transport, key stores, fetchers,
facade, the headless CLI `taskdeck-devicectl`, and the end-to-end suite. Its first pull request, "device:
facade", is the facade with a local-only mode (core plus plain files, no relay); it unblocks `desktop`
(§6.9), `android` (§6.10) and `release`. `recovery` (§6.8) reads your log format.

## Sources
- The code digest `docs/sync/status/foundation/digests/device-digest.md`.
- SYNC.md §2, §3, §4.1, §4.3 (client rules), §4.7, §4.9, §5.2, §5.6, §6.2, §6.6, §6.7, §7 (the ICS, relay,
  network and theft rows), §8, §9. docs/sync/research/R1 §5, §7, §8, §9; V1 findings 8, 12, 14, 15; V2
  findings 3, 9, 12, 14, 21, 22; V3 findings 12, 15, 17. SYNC_PLAN.md §2, §3.3, §3.4, §6.7, §6.8, §6.9,
  §6.10, §6.11.
- The drafts in `docs/sync/spec/`: `engine.md` (events, effects, persistent records, traits), `seal.md`
  (the sealed-file envelope, key hierarchy, backup envelope), `relay.md` (routes, socket, errors,
  capabilities), `core.md` (intents, read queries, legacy import).

## Must contain
1. **The facade**: the one API the desktop app and the Android bridge call. Every operation with its
   signature and contract, grouped: opening (local-only mode and synced mode; what local-only stores and
   that the wall calendar behaves as today with no sync configured); intents in and results out; the
   zone-aware read queries under the names the UI and the phone page already use (`core.md`); change
   notifications; the notes draft and merge; notices and refusals; sync status and last seen; creating a
   board; pairing in both roles; devices, revoke, replace; banners; alarm evidence and adopting a state by
   fingerprint; the halted view and Skip; restore to a point; purge history; move board and following a
   move; reset trust; the recovery sheet's data (never a file), the recovery-key change; backup
   destinations and status; the shared picture; import-check, "create board from this folder" and "Export
   legacy". For each: which stage's screens use it, what it returns, its errors, whether it needs device
   authentication first (the caller's duty: Windows Hello, Touch ID, BIOMETRIC_STRONG).
2. **Threads and ownership**: the sync engine on one blocking thread (SYNC.md §2.1); how the facade is
   called from the UI thread and from JNI; a single process and one instance per data folder.
3. **The store**: the directory layout (`taskdeck_data/sync/`, app-private storage on Android); for each
   file (`log.tdl`, `checkpoint.tdk`, `base-<seq>.tdk`, `hashes.tdk`, `outbox.tdo`, `overlay.tdk`, page
   markers, notices, anything else) the record layout inside `seal.md`'s sealed-file envelope, when it is
   written, the fsync order against the engine's effects, retention (400 days), what a reader does with a
   torn or unreadable tail; the lock (a contended lock refuses sync; cloud-synced folders are refused);
   "a replica loads `BoardData` exactly as stored" (V2-19); the crash-consistency rule a file layer that
   kills the process at every write boundary must not break.
4. **Local-only mode**: the plain files it keeps (today's formats, or say which), and how a local-only
   board becomes a synced one.
5. **Transport**: HTTPS and WSS over rustls with the platform verifier; `http://` refused; the constant
   User-Agent (give the exact string's form); full-jitter backoff from 1 to 60 s, reset only after 60 s of
   healthy connection, bypassed once on resume; daily budgets; what each relay error and quota signal
   turns into for the engine and the UI ("sync paused until 00:00 UTC"); polling when the relay does not
   push (ruling S9); timeouts; no proxy or certificate options in release builds.
6. **Test-only trust** (SYNC_PLAN.md §3.4): the cargo feature's name, exactly what it enables (a relay on
   loopback, a test certificate), that no default feature set enables it, and how the `release` stage
   proves a built artifact lacks it.
7. **The key-store interface**: the trait; what it stores (the wrapped `L_d` and what else); Windows DPAPI
   (user scope, under `%LOCALAPPDATA%\TaskDeck`, not roaming); the macOS login Keychain; the Android
   Keystore through the bridge (`bridge.md`); an in-memory store for tests; what happens when a key cannot
   be unwrapped ("the device is replaced").
8. **Fetchers**: ICS (https only; at most 3 redirects, followed by hand and re-checked, with no Referer and
   no query carried across origins; the address guard: no IP literals and none of the ranges of SYNC.md §7
   after normalising mapped and translated forms, with the resolved address pinned; 20 s; 8 MiB; ETag with
   the raw body cached so the window can slide (V2-22); no ATTACH or URL fetches; errors that never contain
   the URL; a feed's own name adopted once, by the device that added it; a feed that panics is
   quarantined); weather (Open-Meteo, the home place rounded to 0.01°, the phone's own place never synced).
9. **Blobs**: the shared picture: JPEG only, decoded in Rust under stated limits, re-encoded before any
   WebView or UI sees it (V1-14, V3-12); upload, download and caching.
10. **Backup scheduling** (the formats belong to `seal.md`, the writer and reader to `recovery`): nightly
    files and monthly segments written by whichever desktop is awake, destinations, retention (30 nightly,
    12 monthly, 13 segments), re-reading and checking each file, status and alarm; what a week with no
    desktop awake means (ruling S8); the recovery tool's online steps (topping up through the read token,
    fencing the old board).
11. **`taskdeck-devicectl`**: the headless device's commands, enough to drive every end-to-end scenario.
12. Error catalogue, Limits, Test vectors (store files with known contents; fetcher cases), Interfaces,
    Public API (Rust signatures of `taskdeck-device`, including the facade), Open questions.

## Gaps to close (decide, and say so)
- The facade's shape for change notification (callback, channel, or poll with a version) so that both the
  egui loop (DOCUMENTATION.md §14.1 must stay untouched) and JNI can use it.
- Which facade calls block and for how long.
- The exact split between what the engine persists (as effects) and what the store adds around it.
- Where per-device settings that are not board data live (zone, backup destinations, the phone's place).
- How the desktop learns "what blocks a rules upgrade" and "last seen" from your API.
