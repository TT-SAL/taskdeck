# TaskDeck sync spec: device

*The device runtime that the desktop app and the Android bridge call: the facade and its threads, the data folder and its lock, the sealed store and its log format, local-only mode over today's plain files, transport to the relay, test-only trust, the key-store interface, the ICS and weather fetchers, blobs and the shared picture, backup scheduling and the recovery tool's online steps, per-device settings, the headless `taskdeck-devicectl` and the end-to-end suite. Owning stage: `device`. Conventions, notation and the glossary are in [`README.md`](README.md); this spec does not restate them.*

## 1. Scope

This spec fixes everything `crates/taskdeck-device/` does around the pure crates: which files it keeps and their bytes, the order in which it makes them durable, every request it sends to the relay, ICS hosts and Open-Meteo, how it holds keys, and the one API (the **facade**) through which stage `desktop` (`src/`) and stage `android` (`crates/taskdeck-android/`) reach the board. It realises:

- SYNC.md §2.1 (the sync engine on one blocking thread, the fetchers, the secrets column), §2.3 (the runtime's crates), §3 (the rows for notepad drafts, weather and the home place, the shared picture, subscriptions, offline editing, sync status, last seen, backups), §4.1 (the device files, their associated data, fsync before `apply` returns, "a replica loads `BoardData` exactly as stored"), §4.3 client rule 6 (one identity per folder, cloud-synced folders refused, no relay answer deletes local data), §4.7 (catch-up pages durable before they are applied), §4.9 (backoff bypassed on resume, pings, background jobs), §5.2 (tokens only in headers, login), §5.3 step 1 (device authentication before pairing), §5.5 (the sheet printed from memory, never through a file; backup writers pin the recovery fingerprint), §5.6 (DPAPI, the login Keychain, the Android Keystore; a key that cannot be unwrapped means a replaced device), §6.2 (full-jitter backoff, the client budget, "sync paused until 00:00 UTC"), §6.4 (the ICS and Open-Meteo rows), §6.6 (nightly files and monthly segments, destinations, retention, re-reading, status and alarm), §7 (the ICS, relay, network and theft rows), §8 (no proxy or certificate options in release builds), §9 steps 4, 6 and 8 (import check, create board from a folder, "Export legacy");
- R1 §5 (what of `sync.rs` is reused: the atomic outbox file, set-aside on first contact), §7 (what a device-side fetcher needs), §8 (weather), §9 (board data against per-device data);
- the verifier findings V1-8 (no key material reachable from development tooling: the release artifacts carry no test trust), V1-12 (file AD per kind, device and board), V1-14 and V3-12 (JPEG only, decoded under limits and re-encoded), V1-15 (hand-followed redirects, no Referer, the address guard, a feed's name adopted once), V2-3 (seal, fsync, send), V2-9 and V2-12 (weekly bases kept 400 days, small nightly backups plus segments), V2-14 (per-page fsync and markers), V2-19, V2-21 (one session per process, nothing deleted on a relay answer), V2-22 (the raw ICS body kept so the window can slide), V3-15 (printing without a file), V3-17 (what `libtaskdeck.so` links);
- SYNC_PLAN.md §2 (no hub, no always-on device, no `.ics` feed or export, minSdk 26, limited administration from the phone, no legacy-folder watch, separate crates), §3.3, §3.4 (security rules and test-only trust), §6.1 item 3, §6.7 (the `device` stage and its gate), §6.8 (what `recovery` reads), §6.9, §6.10 and §6.11 (what `desktop`, `android` and `release` need);
- the rulings S1, S3, S4, S8, S9, S13, S19, S21, S23, S24, S25, S26, S27, S28, S29 and S30 of `docs/sync/status/foundation.md`, and decision D6.

Non-goals, each owned elsewhere:

- the replica state machine, its records as values and the order of its effects: `engine.md` (this spec gives those records their bytes and their files);
- frames, the sealed-file envelope, file kinds 1 to 8, keys, pairing machines, the backup envelope and its reader: `seal.md`;
- board data, intents, the compile step, read queries, the legacy import: `core.md`;
- routes, socket messages, tokens and their checks: `relay.md`;
- the screens and their wording, display order, the notepad editor, the print dialog: stages `desktop` and `android`;
- the JNI surface, the phone page's routes and the WebView: `bridge.md` (stage `android-shell`);
- the backup file's layout, writer and reader, the sheet's printed page and the offline recovery tool: stage `recovery` (SYNC_PLAN.md §6.8), from `seal.md` §18 and §21 and this spec's §15.

There is no hub and no always-on device (SYNC_PLAN.md §2). Every duty falls to whichever device is awake; backups wait for a desktop (ruling S8).

## 2. Terms

Terms used here that README §9 does not define.

| Term | Meaning |
|---|---|
| runtime | The code of `taskdeck-device` that owns one engine, turns its effects into I/O and I/O into its events (`engine.md` §2). |
| facade | The type `Device` (§17, §25): the only API `desktop` and `android` call. |
| data folder | The folder `taskdeck_data` that today's `AppDirs::resolve` chooses (DOCUMENTATION.md §4.1), or `<filesDir>/taskdeck_data` on Android. |
| identity | One device identity on one board: a `(board_id, dev)` pair with its keys, its storage key `L_d` and its identity directory (§6.1). |
| identity directory | `taskdeck_data/sync/id-<board hex>-<dev hex>/`, holding every sealed file of one identity. |
| active identity | The identity named by `sync/active.tds` (§6.2); the one the facade opens. |
| mode | `LocalOnly`, `Synced` or `KeyLost` (§5.4). |
| plain files | Today's files in the data folder: `read_at_startup.json`, `archived.jsonl`, `notepad_text.json`, `subscriptions.json`, `subscribed_cache.json`, `colorschemes.json`, `userconfig.toml` (digest §2, §8). |
| store | The part of the runtime that reads and writes the identity directory (§6, §7). |
| file layer | The trait through which the store performs every file system call (§7.1); the crash tests replace it. |
| write boundary | One call of the file layer that changes the disk: `write`, `sync`, `rename`, `remove`, `create_dir`, `sync_dir`, `truncate` (§7.1). |
| record JSON | The canonical encoding (`core.md` §4) of an engine record by the mapping of §6.5. |
| loop turn | One pass of the sync thread's loop (§4.3). |
| published view | The immutable value the sync thread publishes after a loop turn that changed what the interfaces show (§4.4). |
| change counters | The counters of §17.3 that tell a caller what changed. |
| foreground | A platform state in which the app is visible to the user (§10.9): a desktop app that is running, or the Android activity between `onStart` and `onStop`. |
| background run | A bounded sync run started by an Android job (§10.9). |
| fetcher | A device's HTTP client for ICS feeds (§13.1) or Open-Meteo (§13.2) (README §9). |
| address guard | The rules of §13.1.4 that refuse private, loopback and translated addresses and pin the address a fetch connects to. |
| destination | A folder a desktop writes backups to (§15.2). |
| writer tag | The first 4 bytes of a backup writer's `dev` in lowercase hex (8 characters), used in backup file names (§15.3). |
| device authentication | Windows Hello, Touch ID, or Android BIOMETRIC_STRONG, run by the calling app before a facade call that §17 marks "auth" (SYNC.md §5.3 step 1, SYNC_PLAN.md §2). |

## 3. Rules for every implementation

These hold for all code in `crates/taskdeck-device/` except its tests, benches and the `taskdeck-devicectl` binary's argument parsing.

- **DEV-GEN-001** (device) The `taskdeck-device` crate MUST declare `#![deny(unsafe_code)]` at its root, and MUST allow `unsafe` only in the module `platform::windows` (§12.3, §7.1; ruling S26), whose every `unsafe` block MUST carry a `// SAFETY:` comment and call only `CryptProtectData`, `CryptUnprotectData`, `LocalFree` and `MoveFileExW` of the `windows` crate.
- **DEV-GEN-002** (device) The crate MUST compute SHA-256, HKDF, HMAC, AEAD, signatures and HPKE only through `taskdeck-seal` or the `sha2` crate, and MUST NOT contain its own implementation of any hash, MAC, cipher, signature or random generator.
- **DEV-GEN-003** (device) The crate MUST draw every random byte (session ids, `file_id`s, the jitter of §10.7, blob ids, nonces) from `taskdeck_seal::OsEntropy`, except the deterministic test hooks behind the feature `test-trust` (§11.1 item 8).
- **DEV-GEN-004** (device) Every value that holds `L_d`, a file key, an identity export, a recovery seed, a pairing code, a create secret, a relay token or a sheet text MUST be held in a `zeroize::Zeroizing` or `ZeroizeOnDrop` type, and MUST NOT implement or be passed to `Debug`, `Display` or `serde::Serialize`, except the token's serialisation inside the sealed device state (§16).
- **DEV-GEN-005** (device) Every comparison of a secret value the crate performs itself (a create secret it retyped, a retyped sheet seed, a typed move code) MUST be constant-time through `subtle::ConstantTimeEq` or the `taskdeck-seal` function that owns it.
- **DEV-GEN-006** (device) Every parser of the crate (sealed record plaintexts, relay responses, ICS bodies before `ics::parse`, image headers, `active.tds`, `store.tds`, devicectl requests) MUST check every length and count against its limit in §22 and against the bytes remaining before it allocates or reads that many bytes.
- **DEV-GEN-007** (device) The crate MUST NOT parse any byte of a sealed record, a frame body, a blob or a backup before `taskdeck-seal` has authenticated it, except the fixed header fields that `seal.md` §19.4, §20.5 and §21.6 name.
- **DEV-GEN-008** (device) No error, status, notice, alarm, log line, panic message or devicectl output the crate produces MUST contain a board text (item name, notes, subscription name), a subscription URL or any part of one, a token, a key, a code, a seed or a create secret; only codes, seqs, ids, counts, device names, key fingerprints, hosts of the relay and of Open-Meteo, and HTTP status numbers; the only exceptions are devicectl's `sheet` and `pair_start` answers (DEV-CTL-002), which exist only with `test-trust`.
- **DEV-GEN-009** (device) Every call into `taskdeck-core`, `taskdeck-seal`, `taskdeck-sync` or the `image` crate that processes data from a file, the relay, a feed or Open-Meteo MUST run inside `std::panic::catch_unwind`, and a caught panic MUST be handled as the section that makes the call states, never by unwinding out of the sync thread or a facade call.
- **DEV-GEN-010** (device) The crate MUST NOT write to standard output or standard error, and MUST NOT write any log file; diagnostics reach the caller only through the facade (§17).
- **DEV-GEN-011** (device) The crate's non-development dependencies MUST be exactly `taskdeck-core`, `taskdeck-seal`, `taskdeck-sync`, `taskdeck-backup`, `serde`, `serde_json`, `chrono`, `zeroize`, `subtle`, `sha2`, `base64`, `flate2`, `image` (features `jpeg` and `png` only), `ureq`, `tungstenite`, `rustls`, `rustls-platform-verifier`, `url`, `webpki-roots` 1.0.12 if and only if stage `android-shell` chooses bundled roots for an API level (SYNC_PLAN.md §3.4, §6.6 item 7; §10.1), and, per target, `windows` (Windows) and `security-framework` (macOS), at the workspace's pinned versions.

## 4. Crates, threads and ownership

### 4.1 Where the code lives

| Crate | Owner | What it holds | Links |
|---|---|---|---|
| `taskdeck-device` | `device` | store, transport, key-store trait and the desktop key stores, fetchers, blobs, backup scheduling, the facade, the adapters that implement `taskdeck_sync::StateMachine` over `taskdeck-core` and `taskdeck_sync::Sealer` over `taskdeck-seal` (decision D6) | core, seal, sync, backup |
| `taskdeck-devicectl` | `device` | a binary target of `taskdeck-device`, `required-features = ["test-trust"]` (§19) | `taskdeck-device` |
| `taskdeck-recover` | `recovery` | the recovery tool (SYNC_PLAN.md §6.8): the backup reader, the replay and the online steps of §15.8 | backup, seal, core, and `taskdeck-device` with default features for `recovery_online::RecoveryLink` (§15.8, §25; lead ruling 3 of the workspace brief) |
| `tests/e2e/` | `device` | the end-to-end suite (§20) | the standby relay, devicectl |
| `taskdeck-android` | `android-shell`, then `android` (SYNC_PLAN.md §4) | the JNI library: the facade behind JNI, the Keystore key store over the bridge (§12.5) | `taskdeck-device` with default features (V3-17) |
| `src/` | `desktop` | `TaskApp` on the facade | `taskdeck-device` with default features |

`libtaskdeck.so` links `taskdeck-device` and everything it links, and no code of the root package (SYNC_PLAN.md §2, V3-17).

### 4.2 Threads

One facade owns one data folder and runs these threads, each named:

| Thread | Name | Started | Does | Never does |
|---|---|---|---|---|
| sync | `taskdeck-sync` | at `open`, in every mode | owns the engine (synced mode) or the local-only board (§9); runs the compile step; performs every store write and read; publishes views | network I/O, image decoding, waiting on a lock held by another thread for longer than one channel send |
| net | `taskdeck-net` | synced mode, at the first `Connect` | owns the socket: writes socket messages, reads with a 200 ms read timeout, sends protocol pings | file I/O |
| https | `taskdeck-https` | synced mode, at the first HTTPS request | performs HTTPS requests one at a time, in the order the sync thread queued them | file I/O |
| calendars | `taskdeck-calendars` | when the board has a subscription | ICS fetches (§13.1), one at a time | file I/O; it hands results to the sync thread |
| weather | `taskdeck-weather` | at `open` | Open-Meteo fetches (§13.2) | file I/O |
| picture | `taskdeck-picture` | on demand | decoding and re-encoding pictures (§14) | network or file I/O |
| backup | `taskdeck-backup` | desktop, synced mode | writing and re-reading backup files in destinations (§15) | relay I/O |

The sync thread is "the sync engine on one blocking thread" of SYNC.md §2.1. It blocks only on its own file I/O and on its input channel. All other threads communicate with it through one multi-producer channel of `Input` values (§8.1) and never touch the engine or the store.

- **DEV-THREAD-001** (device) The engine, the local-only board and every file of the data folder's `sync/` directory and of the plain files MUST be accessed only from the sync thread, except the backup thread's writes into destinations, the facade's read of published views (§4.4), and the key store's own calls, which run on the threads §12.1 names and of which only `AndroidKeyStore` touches a file under `sync/` (`ld.wrap`, §12.5).
- **DEV-THREAD-002** (device) The sync thread MUST NOT perform network I/O, decode an image or wait for another thread's result, other than receiving from its input channel.
- **DEV-THREAD-003** (device) Every thread the crate starts MUST be named as the table of §4.2 states, and every one of them MUST be joined by `shutdown` (§17.2) before it returns.

### 4.3 The loop of the sync thread

Each loop turn, in order:

1. Receive one input: first from the **local queue** (inputs the sync thread produced for itself: `Persisted`, `PersistFailed`, `LocalRead`), else from the **call queue** (facade requests), else from the **event queue** (transport, fetcher, picture, backup and timer inputs). The three queues are drained in that priority; within one queue, in arrival order. When all three are empty, wait on the channel until the next scheduled tick (§8.4).
2. Hand the input to the engine (or the local-only board), or perform the facade request (§17).
3. Perform the effects (§8.2): `Persist` effects synchronously, each followed by its `Persisted` placed on the local queue; every other effect handed to its thread.
4. If what the interfaces show changed, publish a new view and raise the change counters (§4.4).

A `Page` with up to 256 entries is one input; its persist and its apply are separate turns (`engine.md` §12.3 steps 8 and 9), so a facade request waits for at most one page persist and one page apply.

- **DEV-THREAD-004** (device) The sync thread MUST take its next input by the priority of §4.3 step 1, and MUST serve every facade request already in the call queue before it takes another event-queue input.
- **DEV-THREAD-005** (device) The sync thread MUST hand at most one event-queue input to the engine per loop turn.

### 4.4 Published views

After a loop turn that changed the pending view, the status, the notices, the banners, the overlay, the weather report or the picture, the sync thread builds a `Published` value (§25) and swaps it into a `Mutex<Arc<Published>>`. A facade read locks that mutex only to clone the `Arc`, so a read never waits for the sync thread.

`Published` holds: the pending `BoardData` (`Engine::view`, or the local-only board), the engine's `view_version` (or the local-only counter), `Engine::status()`, `Engine::pending()`, `Engine::notices()`, `Engine::purge_preview()`, the overlay, the weather report with the place it was fetched for, the weather place of §16 (`weather_source` and `own_place`), the processed picture, the device status of §17.3, the identity list of §17.2 (`identities()`), and the change counters. The pending `BoardData` is shared by `Arc` with the engine's adapter state where the adapter keeps it in an `Arc`; it is cloned at most once per published view.

- **DEV-THREAD-006** (device) A facade read MUST answer from the latest published view and MUST NOT send anything to the sync thread or wait for it.
- **DEV-THREAD-007** (device) The sync thread MUST publish a new view in the same loop turn in which the change became visible to the engine, and MUST publish at most one view per loop turn.

### 4.5 Calls from the UI thread and from JNI

`Device` is `Clone + Send + Sync`. Every method may be called from any thread. Reads (§17) answer from the published view. Writes send a request with a one-shot reply channel to the call queue and block the calling thread until the reply arrives; the time a write blocks is the wait of §4.3 plus the request's own work, and §17's tables give the expected bound for each. The egui loop calls writes from its UI thread, as today's `TaskApp` calls `board.apply` (DOCUMENTATION.md §14.1 is unchanged: no new thread, timer or redraw rule is added to the render loop; the waker of §17.3 sends the same `EventLoopProxy` event today's sync and fetch threads send). The Android bridge calls writes from a worker thread, never from the Android main thread (`bridge.md`).

- **DEV-THREAD-008** (device, desktop, android) A caller MUST NOT call a facade method marked "long" in §17 from a thread that draws the user interface; `desktop` calls them from a worker thread of its own, `android` from a bridge worker thread.

### 4.6 One process and one instance per data folder

A data folder is used by at most one facade at a time, in one process. The lock of §5.2 enforces it across processes; within a process, `open` refuses a second facade for a data folder whose canonical path an open facade already holds (`device/folder_in_use`).

- **DEV-THREAD-009** (device) `open` MUST refuse with `device/folder_in_use` a data folder whose canonical path is held by another open `Device` in the same process.

## 5. The data folder, opening and modes

### 5.1 The data folder

The caller passes the path of the data folder; the device does not choose it. The desktop keeps today's `AppDirs::resolve` (DOCUMENTATION.md §4.1, digest §1.1) and passes `dirs.data`; the Android shell passes `<Context.getFilesDir()>/taskdeck_data` (app-private storage, SYNC.md §4.1). The folder holds the plain files of local-only mode (§9), `userconfig.toml` and `colorschemes.json` (desktop settings, read and written by `desktop` as today), the lock file `.lock`, and the directory `sync/` (§6.1).

### 5.2 The lock

At `open` the device creates the data folder if it is missing, opens `taskdeck_data/.lock` with create, write and no truncation, and calls the standard library's exclusive `File::try_lock` (today's `claim_data_dir`, paths.rs:96-120). The result is one of:

| Result | When | Local-only mode | Synced mode, create, join |
|---|---|---|---|
| `Held` | `try_lock` succeeded | open | open |
| `Contended` | `TryLockError::WouldBlock` | open, with the warning `device/folder_locked` in the open report (today's behaviour, DOCUMENTATION.md §4.1) | refused with `device/folder_locked` |
| `Unsupported` | the open failed, or `TryLockError::Error` (the file system does not lock) | open, with the warning `device/folder_unlockable` | refused with `device/folder_unlockable` |

The lock file is never written and stays empty. The facade holds the file, and so the lock, until `shutdown` returns.

- **DEV-LOCK-001** (device) `open` MUST take the exclusive lock of §5.2 on `taskdeck_data/.lock` before it reads or writes any other file of the data folder, and MUST hold it until `shutdown` returns.
- **DEV-LOCK-002** (device) `open` in synced mode, `begin_create` and `join` MUST refuse with `device/folder_locked` when the lock is contended and with `device/folder_unlockable` when it cannot be taken, and MUST NOT then read or write any file of `sync/` (SYNC.md §4.3 rule 6).
- **DEV-LOCK-003** (device) `open` in local-only mode MUST proceed when the lock is contended or unsupported, and MUST report the matching warning.
- **DEV-LOCK-004** (device) `import_check` and `begin_create` from a folder MUST refuse with `device/folder_locked` a source folder whose `.lock` another process holds (SYNC.md §9 step 4), testing it with `try_lock` and releasing it at once when it was free.

### 5.3 Cloud-synced folders

A data folder inside a folder that a cloud service synchronises would let the service copy the identity to another machine and roll files back (SYNC.md §4.3 rule 6). The check, on the canonical path of the data folder (symbolic links resolved), refuses it when any of these holds:

1. a path component equals, ignoring ASCII case, `OneDrive`, `iCloud Drive`, `iCloudDrive`, `Mobile Documents`, `CloudStorage`, `Dropbox`, `Google Drive`, `GoogleDrive`, `My Drive`, `Box`, `Box Sync`, `pCloud Drive`, `Nextcloud`, `ownCloud`, `MEGA`, `MEGAsync`, `Sync`, `SynologyDrive`, `Tresorit`, or begins, ignoring ASCII case, with `OneDrive - ` or `Dropbox (`;
2. on Windows: the path lies under the value of `OneDrive`, `OneDriveConsumer` or `OneDriveCommercial` in the process environment, when that value is non-empty;
3. on macOS: the path lies under `~/Library/Mobile Documents` or `~/Library/CloudStorage`, or under `~/Desktop` or `~/Documents` while `~/Library/Mobile Documents/com~apple~CloudDocs/Desktop` exists (Desktop and Documents in iCloud).

On Android the check is skipped: the folder is app-private storage.

- **DEV-OPEN-001** (device) `open` in synced mode, `begin_create` and `join` MUST refuse with `device/cloud_folder` a data folder for which any rule of §5.3 holds, and MUST NOT then read or write any file of `sync/`.

### 5.4 Modes

`open` decides the mode, in order:

1. Take the lock (§5.2).
2. Read `sync/active.tds` (§6.2). Absent: the mode is **`LocalOnly`** (§9): an empty data folder is an empty local board, and no file is written before the first change. When `sync/pending.tds` exists, the open report carries `PendingCreate` (§18.1).
3. Present: the lock must be `Held` and the cloud check must pass (§5.2, §5.3); read `store.tds` of the named identity directory and check that it names the same `board_id` and `dev`; else `device/store_descriptor`.
4. Load `L_d` from the key store named by `store.tds` (§12); the key store in `OpenOptions` must have that `kind` (else `device/store_descriptor`): `Unavailable` (the store is locked, or the bridge is not ready): `open` fails with `device/keystore_unavailable` and changes nothing; `NotFound` or `CannotUnwrap`: the mode is **`KeyLost`** (§12.6).
5. Read every file of the identity directory (§7.4), build `Loaded` (`engine.md` §7.10), and send `Start`. The mode is **`Synced`**.

| Mode | What the facade offers | Files written |
|---|---|---|
| `LocalOnly` | every board intent and read query, the fetchers, import check, create board (from this folder or empty), join (pair as a new device), export legacy, `identities` and `forget_identity` (§17.2) | the plain files (§9) |
| `Synced` | everything of §17 | the identity directory; never the plain files, except "Export legacy" into another folder |
| `KeyLost` | the reason, the old identity's `board_id`, `dev` and relay address; `join`; `begin_create`; `identities`; `forget_identity`, also of the active identity (§17.2) | nothing until the user acts |

- **DEV-OPEN-002** (device) `open` MUST decide the mode by the five steps of §5.4 in order.
- **DEV-OPEN-003** (device) In local-only mode the device MUST NOT open any network connection other than the fetchers' (§13), and MUST NOT create the `sync/` directory except as `begin_create` or `join` does.
- **DEV-OPEN-004** (device) A data folder with no plain files and no `sync/active.tds` MUST open as an empty local-only board, and `open` MUST NOT write any file for it other than `.lock`.

## 6. The store: layout and encodings

### 6.1 Layout

```text
taskdeck_data/
  .lock                               the lock (§5.2)
  read_at_startup.json, archived.jsonl, notepad_text.json, subscriptions.json,
  subscribed_cache.json, feeds/       local-only mode only (§9)
  device.json                         local-only per-device settings (§16)
  userconfig.toml, colorschemes.json  desktop settings (stage desktop), as today
  pre-sync-<YYYYMMDD-HHMMSS>/         plain files moved aside by "create board from this folder" (§18.2)
  local-<YYYYMMDD-HHMMSS>/            plain files set aside by a join (§18.4)
  sync/
    active.tds                        which identity is open (§6.2)
    pending.tds                       an identity being created, not yet active (§18.1)
    id-<board hex>-<dev hex>/         one identity directory per identity (§2)
      store.tds                       its descriptor (§6.2)
      identity.tdk                    kind 7
      device.tdk                      kind 12
      bootstrap.tdk                   kind 17, until the first checkpoint
      genesis.tdb                     the sealed genesis snapshot of an imported board, until the first checkpoint (§18.1)
      log-<first seq>.tdl             kind 1, log segments
      checkpoint.tdk                  kind 2
      base-<seq>.tdk                  kind 3, one per base
      hashes.tdk                      kind 4
      outbox.tdo                      kind 5
      notices.tdk                     kind 8
      meta.tdk                        kind 9
      marker.tdk                      kind 10, present only while a marker is set
      history.tdk                     kind 11
      overlay.tdk                     kind 6
      feed-<subscription id>.tdk      kind 16, one per subscription with a cached body
      picture.tdk                     kind 13
      own-picture.tdk                 kind 19, Android only, present only while the phone has its own picture
      weather.tdk                     kind 14
      draft.tdk                       kind 15, present only while a notes draft is unsaved
      unsent.tdk                      kind 18, present only during a re-pair (§18.4)
      ld.wrap                         Android only: the Keystore wrap of L_d, written by AndroidKeyStore, not by the store (§12.5)
      *.tmp                           a replacement being written (§7.2)
```

`<board hex>` and `<dev hex>` are 32 lowercase hex characters. `<first seq>` and `<seq>` are the seq in decimal, zero-padded to 20 digits, so file names sort by seq. `<subscription id>` is decimal without padding. Every `.tdk`, `.tdl` and `.tdo` file is a sealed file of `seal.md` §19 sealed under `K_file(kind)` of the identity's `L_d`.

`genesis.tdb` is not a sealed file of `seal.md` §19 and has no kind: it holds exactly the sealed blob bytes of the genesis snapshot (`seal.md` §20, `S = 1`), already encrypted under epoch 1, whose `blob`, `sha256`, `size` and `parts` the Genesis item of `frame_1` names (`seal.md` §12.2).

| Kind | File | Records | Written by | Plaintext |
|---|---|---|---|---|
| 1 | `log-<first seq>.tdl` | appended, one per page | `AppendLog` | §6.4.1 |
| 2 | `checkpoint.tdk` | one | `WriteCheckpoint` | §6.4.2 |
| 3 | `base-<seq>.tdk` | one | `WriteBase` | §6.4.2 |
| 4 | `hashes.tdk` | appended, one per op | `AppendHashes` | §6.4.3 |
| 5 | `outbox.tdo` | appended, one per op (more when §6.4 splits it); rewritten | `Outbox`, `RewriteOutbox` | §6.4.4 |
| 6 | `overlay.tdk` | one | the store (§13.1) | §6.4.10 |
| 7 | `identity.tdk` | one | `WriteIdentity` | §6.4.7 |
| 8 | `notices.tdk` | appended, one per op; rewritten | `Notices`, `RewriteNotices` | §6.4.5 |
| 9 | `meta.tdk` | one | `WriteMeta` | §6.4.6 |
| 10 | `marker.tdk` | one | `SetMarker`; `ClearMarker` removes the file | §6.4.6 |
| 11 | `history.tdk` | appended, one per control entry | the store (§8.3) | §6.4.8 |
| 12 | `device.tdk` | one | the store (§16) | §16 |
| 13 | `picture.tdk` | one | the store (§14.2) | §6.4.11 |
| 14 | `weather.tdk` | one | the store (§13.2) | §6.4.12 |
| 15 | `draft.tdk` | one | the store (§17.6) | §6.4.13 |
| 16 | `feed-<id>.tdk` | one | the store (§13.1) | §6.4.10 |
| 17 | `bootstrap.tdk` | one | the store (§18) | §6.4.9 |
| 18 | `unsent.tdk` | one | the store (§18.4) | §18.4 |
| 19 | `own-picture.tdk` | one | the store (§14.3) | §6.4.14 |

Kinds 9 to 19 are this spec's and are listed in `seal.md` §19.2; kind 19 is the phone's own picture (ruling S29, §14.3).

SYNC.md §4.1 names one `log.tdl`; this spec splits the log into segments so that truncating it below a seq deletes whole files instead of rewriting a 400-day log (§7.3). A segment is closed when it holds 16,384 entries or 67,108,864 bytes; the next page starts a new segment named after its first seq.

- **DEV-STORE-001** (device) The store MUST keep every file of an identity in that identity's directory with exactly the names, kinds and record counts of the table of §6.1, and `genesis.tdb` only as §6.1 and §18.1 state.
- **DEV-STORE-002** (device) The store MUST seal every record of a file of kind `k` under `K_file(k)` with the header and AD of `seal.md` §19, with the identity's own `dev` and `board_id` in the header.
- **DEV-STORE-003** (device) The store MUST start a new log segment when the current one holds 16,384 entries or 67,108,864 bytes, and MUST name every segment after the seq of its first entry.

### 6.2 The two plain descriptors

`sync/active.tds` (45 bytes; `sync/pending.tds` has the same layout with magic `"TDP1"`):

| Offset | Field | Width | Type | Value |
|---|---|---|---|---|
| 0 | `magic` | 4 | ASCII | `"TDA1"` |
| 4 | `ver` | 1 | u8 | 1 |
| 5 | `board_id` | 16 | bytes | the active identity's board |
| 21 | `dev` | 16 | bytes | its `dev` |
| 37 | `check` | 8 | bytes | `SHA-256(bytes 0..37)[0..8]` |
| | **total** | **45** | | |

`store.tds` in each identity directory (54 bytes):

| Offset | Field | Width | Type | Value |
|---|---|---|---|---|
| 0 | `magic` | 4 | ASCII | `"TDS1"` |
| 4 | `ver` | 1 | u8 | 1 |
| 5 | `board_id` | 16 | bytes | |
| 21 | `dev` | 16 | bytes | |
| 37 | `key_store` | 1 | u8 | the key store holding `L_d`: 1 DPAPI, 2 macOS Keychain, 3 Android Keystore, 4 test file store, 5 test memory store (§12) |
| 38 | `created_ms` | 8 | i64 | the device clock when the identity was created; display only |
| 46 | `check` | 8 | bytes | `SHA-256(bytes 0..46)[0..8]` |
| | **total** | **54** | | |

Neither file holds a secret: `board_id` and `dev` are in every sealed header in plaintext. A file of another length, magic, version or check is `device/store_descriptor`.

- **DEV-STORE-004** (device) `active.tds` and `store.tds` MUST be laid out exactly as the tables of §6.2, and a reader MUST refuse with `device/store_descriptor` any other bytes.

### 6.3 Records and their sizes

Every plaintext below is the plaintext of one sealed record (`seal.md` §19.1). Integers are big-endian, `vbN` as `seal.md` §5.1. A plaintext of another length or layout is `device/record_format`, handled as §7.4 states.

### 6.4 Plaintexts

No record plaintext is larger than 67,108,864 bytes, the limit of `seal.md` §19.1 (`ct_len` at most 67,108,880 with the tag). An op of kind 1 or 5 that would exceed it is written as consecutive records, each ending before the entry or op that would make it exceed the limit (§6.4.1, §6.4.4).

#### 6.4.1 Log page (kind 1)

| Field | Width | Type | Value |
|---|---|---|---|
| `n` | 2 | u16 | 1 to 256 |
| entries | `Σ (52 + flen)` | | `n` × entry |

Entry: `seq u64 ‖ chain[32] ‖ tau i64 ‖ flen u32 ‖ frame[flen]` (52 + `flen` bytes): the fields of `LogRecord` (`engine.md` §7.2), `flen` from 670 to 262,144. The seqs are consecutive within a record, the first seq of each record is one more than the last seq of the record before it (in the same segment, or the last seq of the previous segment), and the first seq of a segment equals the number in its name. An `AppendLog` op is written as consecutive records, each of at most 256 entries and at most 67,108,864 plaintext bytes (§6.4); 256 entries of the largest frames (2 + 256 × 262,196 = 67,122,178 bytes) exceed the latter, so such a record holds fewer.

#### 6.4.2 Checkpoint and base (kinds 2 and 3)

| Field | Width | Type | Value |
|---|---|---|---|
| `magic` | 4 | ASCII | `"TDC1"` |
| `board_seq` | 8 | u64 | `B` |
| `board_chain` | 32 | bytes | `C_B` |
| `log_floor` | 8 | u64 | |
| `board` | 4 + b | vb32 | `SM::canonical_bytes` of the confirmed state: the canonical board bytes (`core.md` §6.13), at most 62,914,560 bytes |
| `membership` | 4 + m | vb32 | `M_B` canonical bytes (`seal.md` §13.2), at most 2,097,152 bytes |
| `registry` | 4 + r | vb32 | the record JSON (§6.5) of the `Registry`, at most 1,048,576 bytes |
| | **64 + b + m + r** | | |

A base is the same layout; its file name's seq equals its `board_seq`.

#### 6.4.3 Hash records (kind 4)

`n u16 ‖ n × (seq u64 ‖ hash_version u32 ‖ hash[32])`: 2 + 44 `n` bytes, `n` from 1 to 1,024, in the op's order.

#### 6.4.4 Outbox ops (kind 5)

`n u16 ‖ n × op`, `n` from 1 to 1,024. An op is `0x01 ‖ vb32(record JSON of OutboxRecord)` for `Put`, or `0x02 ‖ entry_id[16]` for `Drop`. A `RewriteOutbox` writes a new file whose records hold one `Put` per live record, in `order`, at most 1,024 and at most 67,108,864 plaintext bytes per sealed record; an `Outbox` op is split the same way (§6.4). The record JSON of one `OutboxRecord` is at most 1,048,576 bytes.

#### 6.4.5 Notice ops (kind 8)

`n u16 ‖ n × op`, `n` from 1 to 1,024. `Add`: `0x01 ‖ kind u8 (1 notice, 2 refusal) ‖ seq u64 ‖ vb32(record)`, the record being the canonical bytes of `core.md` §10.7 (at most 4,096 bytes). `Dismiss`: `0x02 ‖ seq u64 ‖ index u32`. A `RewriteNotices` writes a new file with the given ops in order, at most 1,024 per sealed record.

#### 6.4.6 Meta and marker (kinds 9 and 10)

Meta: the record JSON (§6.5) of `Meta`, at most 4,194,304 bytes. Marker: `first u64 ‖ last u64 ‖ attempt u64` (24 bytes).

#### 6.4.7 Identity (kind 7)

The sealer adapter's export (`Sealer::export_identity`, `engine.md` §6.1), held in `Zeroizing` buffers throughout:

| Field | Width | Type | Value |
|---|---|---|---|
| `magic` | 4 | ASCII | `"TDI1"` |
| `keys` | 2 + k | vb16 | `DeviceKeys::to_identity_bytes` (`seal.md` §26): 80 bytes, so `k = 80` |
| `n_roots` | 2 | u16 | 0 to 65,535 |
| roots | 36 × `n_roots` | | `epoch u32 ‖ root[32]`, strictly ascending by epoch |
| `n_pending` | 1 | u8 | 0 to 16 |
| pending | 52 × `n_pending` | | `entry_id[16] ‖ epoch u32 ‖ root[32]`: roots drawn for own rotations not yet accepted |
| `has_anchor` | 1 | u8 | 0 or 1 |
| anchor | 0 or 80 | | `rsign_pk[32] ‖ salt[16] ‖ root1[32]` (`GenesisAnchor`), present on the creating device until seq 1 is accepted |
| | **10 + k + 36 `n_roots` + 52 `n_pending` + 80 `has_anchor`** | | |

#### 6.4.8 History record (kind 11)

The history record of `seal.md` §21.5, byte for byte (156 bytes plus the items): `seq ‖ dev ‖ ctr ‖ entry_id ‖ at_ms ‖ frame_hash ‖ vb32(items) ‖ sig`. One sealed record per accepted control entry, in strictly ascending seq. It is built by `taskdeck_seal::HistoryRecord::from_accepted`, written as its `to_bytes()`, and read with `HistoryRecord::from_bytes` (`seal.md` §26); a record that `from_bytes` refuses is `device/record_format`.

#### 6.4.9 Bootstrap (kind 17)

`0x01 ‖ vb32(board) ‖ vb32(frame_1) ‖ vb8(host) ‖ door[5] ‖ home_gen u32` with `home_gen = 1` for a creating device (`Bootstrap::Genesis`, then the relay target), or `0x02 ‖ seq u64 ‖ chain[32] ‖ hash_version u32 ‖ state_hash[32] ‖ vb32(board) ‖ vb32(membership) ‖ added_seq u64 ‖ vb8(host) ‖ door[5] ‖ home_gen u32` for a joining device (`Bootstrap::Join`, `SnapshotMaterial` then the relay target). In both forms `host` is a host of `seal.md` §5.4 (1 to 255 bytes, `:port` included, never `:443`; SEAL-ENC-006), so it always fits its `vb8`; any other value is `device/record_format`.

#### 6.4.10 Overlay and feed bodies (kinds 6 and 16)

Overlay: `serde_json` compact bytes of `taskdeck_core::overlay::Overlay` (`core.md` §14.6), at most 16,777,216 bytes; `FetchStatus.at` is RFC 3339 with its offset, as in today's `subscribed_cache.json`. Feed body: `sub_id u64 ‖ url_hash[32] ‖ vb16(etag) ‖ vb16(last_modified) ‖ fetched_ms i64 ‖ vb32(body)`, `url_hash = SHA-256(url bytes)`, `etag` and `last_modified` at most 1,024 bytes of visible ASCII each (empty when absent), `body` at most 8,388,608 bytes.

#### 6.4.11 Picture (kind 13)

| Field | Width | Type | Value |
|---|---|---|---|
| `blob` | 16 | bytes | the `PictureRef` it came from (`core.md` §6.9) |
| `epoch` | 4 | u32 | |
| `parts` | 4 | u32 | |
| `sha256` | 32 | bytes | |
| `size` | 8 | u64 | |
| `clean` | 4 + c | vb32 | the re-encoded JPEG of §14.2 step 6, at most 4,194,304 bytes |
| `blur`, `light` | 2 | u8, u8 | the look dials the `look` bytes were made with (0 to 100 each) |
| `look` | 4 + l | vb32 | the JPEG the interfaces show (§14.2 step 7), at most 4,194,304 bytes |
| `top` | 3 | bytes | the average RGB of the top eighth of `look` |
| | **77 + c + l** | | |

#### 6.4.12 Weather (kind 14)

`fetched_ms i64 ‖ source u8 (1 home, 2 own place) ‖ lat i32 ‖ lon i32 ‖ vb32(report)`: `lat` and `lon` in hundredths of a degree, `report` the `serde_json` compact bytes of `taskdeck_core::weather::Report`, at most 1,048,576 bytes.

#### 6.4.13 Draft (kind 15)

`base_rev u64 ‖ edited_ms i64 ‖ vb32(text)`: the `notes_rev` the draft was started from, the device clock at the last edit, and the draft's UTF-8 text, at most 1,048,576 bytes.

#### 6.4.14 Own picture (kind 19)

The layout of §6.4.11 with `blob`, `epoch`, `parts`, `sha256` and `size` all zero bytes: `clean` the re-encoded JPEG of §14.3 step 3, `blur` and `light` the dials its `look` was made with, `look` and `top` as §6.4.11.

- **DEV-STORE-005** (device, recovery) Every record plaintext of kinds 1 to 19 MUST be laid out exactly as §6.4 states for its kind, and a reader MUST treat any other plaintext as `device/record_format`.
- **DEV-STORE-006** (device) Log page records MUST hold consecutive seqs in ascending order, continuing without a gap from the previous record and across segments.

### 6.5 Record JSON

`Registry`, `OutboxRecord` and `Meta` are engine types (`engine.md` §37). Their bytes are the canonical encoding (`core.md` §4.2) of the JSON value this mapping gives, applied recursively to the Rust definitions in `engine.md` (§37, with `Evidence` in §21.1 and `Status` and its parts in §30.1):

| Rust | JSON |
|---|---|
| struct with named fields | object: one member per field, key = the field's name; a field of type `Option<T>` that is `None` is absent; no other field is ever absent |
| tuple and tuple struct | array of the elements in order |
| `enum` unit variant | string: the variant's name in `snake_case` |
| `enum` variant with named fields | object with one member: key = the variant's name in `snake_case`, value = the object of its fields |
| `enum` tuple variant | object with one member: key = the variant's name in `snake_case`, value = the single element, or the array of its elements when it has two or more |
| `bool` | `true` or `false` |
| `u8`, `u16`, `u32`, `u64`, `i64` | integer; a value outside −(2^53 − 1) to 2^53 − 1 cannot be written (`device/record_range`, a local fault) |
| `String`, `&'static str` | string |
| `[u8; N]` (including `Dev`, `Hash32`) | `hexN` (`core.md` §4.4) |
| `Vec<u8>` | `base64` (`core.md` §4.4) |
| `Vec<T>` | array in the vector's order |

The parser is the canonical parser of `core.md` §4.5 with nesting depth at most 32; a parsed value that does not map back to the Rust type is `device/record_format`. `Body::Ctl` holds a `CtlIntent`, mapped like any enum; `Zeroizing` values never occur in these records.

- **DEV-STORE-007** (device) The store MUST encode `Registry`, `OutboxRecord` and `Meta` by the mapping of §6.5 in the canonical encoding, and MUST refuse with `device/record_format` bytes that the canonical parser refuses or that do not map to the type.

## 7. The store: writing, reading and crash consistency

### 7.1 The file layer

The store performs every file system call through a `FileLayer` (§25) whose methods are the write boundaries: `create_dir`, `write_new(path, bytes)` (create or truncate, write all), `append(path, bytes)`, `sync(path)` (`File::sync_all`; on macOS this is `F_FULLFSYNC`), `rename(from, to)`, `remove(path)`, `truncate(path, len)`, `sync_dir(dir)`, and the reads `read`, `list`, `exists`. `OsFiles` is the real layer:

- on Linux and Android, `sync_dir` opens the directory and calls `sync_all` on it;
- on macOS, likewise;
- on Windows, `rename` calls `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH` (the one `unsafe` call of the store, `platform::windows`), and `sync_dir` does nothing, because the write-through rename is durable on its own.

The crash tests use `CrashFiles`, which counts write boundaries and aborts the process (`std::process::abort`) on the k-th (§7.6).

- **DEV-FILE-001** (device) The store MUST perform every file system call through its `FileLayer`, and `OsFiles` MUST implement `sync`, `rename` and `sync_dir` as §7.1 states for each platform.

### 7.2 The write procedures

| Procedure | Used for | Steps, in order |
|---|---|---|
| **append** | kinds 1, 4, 5, 8, 11 | if the file does not exist: `write_new(<name>.tmp, header)`, `sync(<name>.tmp)`, `rename(<name>.tmp, <name>)`, `sync_dir`; then `append(<name>, sealed records)`, `sync(<name>)` |
| **replace** | kinds 2, 6, 7, 9, 10, 12 to 19, `active.tds`, `pending.tds`, `store.tds`, `genesis.tdb` (its bytes without a header or sealing), and the rewrites of kinds 5 and 8 | `write_new(<name>.tmp, header ‖ sealed records)` with a fresh `file_id`, `sync(<name>.tmp)`, `rename(<name>.tmp, <name>)`, `sync_dir` |
| **create** | kind 3, and a new log segment | as replace, with the new file's name; a log segment's header and first page are written in the one `write_new` |
| **remove** | `ClearMarker`, `DeleteBases`, segments below the floor, `draft.tdk` after a save, `own-picture.tdk` on `clear_own_picture`, `bootstrap.tdk` and `genesis.tdb` after the first checkpoint, `unsent.tdk` after its resubmit report, `pending.tds` after an activation, `active.tds` when `forget_identity` removes the active identity (§17.2) | `remove(<name>)`, `sync_dir` |

The store keeps, for each open append file, its header and the number of records in it, and seals the next record with that index (`seal.md` §19.3, SEAL-FILE-002).

- **DEV-FILE-002** (device) The store MUST write each file kind only by the procedure §7.2 assigns to it, with the steps in the order the table gives.
- **DEV-FILE-003** (device) The store MUST draw a new `file_id` for every replace and create, and MUST seal every appended record with the index equal to the number of records already in the file.

### 7.3 Executing a `Persist` effect

A `Persist { id, ops }` (`engine.md` §7.11) is executed on the sync thread, op by op in order:

| Op | Store action |
|---|---|
| `AppendLog(records)` | append to the current segment, or create a new one (DEV-STORE-003); then the history records of §8.3 for these seqs |
| `SetMarker(m)` | replace `marker.tdk` |
| `ClearMarker` | remove `marker.tdk` (nothing when absent) |
| `WriteCheckpoint(c)` | replace `checkpoint.tdk`; then, if `bootstrap.tdk` exists, remove it, and then `genesis.tdb` when it exists |
| `WriteBase(c)` | create `base-<c.board_seq>.tdk` |
| `DeleteBases(seqs)` | remove each `base-<seq>.tdk` |
| `TruncateLogBelow(s, c)` | replace `device.tdk` with `log_floor = s` and `floor_chain = c`, the field absent when `c` is `None` (§16); then remove every segment whose last seq is below `s` |
| `AppendHashes(records)` | append to `hashes.tdk` |
| `Outbox(ops)` | append to `outbox.tdo` |
| `RewriteOutbox(records)` | replace `device.tdk` with `minter` raised to cover every `origin_state` in the outbox file (§8.3); then replace `outbox.tdo` |
| `Notices(ops)` | append to `notices.tdk` |
| `RewriteNotices(ops)` | replace `notices.tdk` |
| `WriteMeta(meta)` | replace `meta.tdk` |
| `WriteIdentity(bytes)` | replace `identity.tdk` |

When every op's last step has returned, the store puts `Persisted { id }` on the local queue. When any step fails, it stops, performs no later step of this or any later `Persist`, puts `PersistFailed { id }` on the local queue, and refuses every later write until the next `open` (ENG-PERSIST-003).

- **DEV-FILE-004** (device) The runtime MUST complete every step of every op of a `Persist` effect, in order, including each `sync` and `sync_dir`, before it delivers that effect's `Persisted`; the order of the effects and of their `Persisted` events is ENG-PERSIST-001's.
- **DEV-FILE-005** (device) On the first failed write boundary of a `Persist`, the runtime MUST deliver `PersistFailed` for that effect and MUST perform no further write to the identity directory until the facade is opened again.
- **DEV-FILE-006** (device) `TruncateLogBelow(s, c)` MUST make `log_floor = s` and `floor_chain = c` durable in `device.tdk` before it removes any segment, and MUST NOT remove a segment that holds any seq at or above `s`.

### 7.4 Reading at open

The store reads the identity directory in this order: `store.tds`; `device.tdk`; `identity.tdk`; `bootstrap.tdk`; `checkpoint.tdk`; every `base-*.tdk` (header and `board_seq` and `board_chain` only, for `Loaded.bases`); the log segments in name order; `hashes.tdk`; `outbox.tdo`; `notices.tdk`; `meta.tdk`; `marker.tdk`; `history.tdk`; then `overlay.tdk`, the `feed-*.tdk` files, `picture.tdk`, `own-picture.tdk`, `weather.tdk`, `draft.tdk`. First it removes every `*.tmp` file of the directory: a replace that did not reach its rename left the old file current. `ld.wrap` (Android) is the key store's (§12.5): the store neither opens it nor treats it as corrupt, and a leftover `ld.wrap.tmp` is removed with the other `*.tmp` files. `genesis.tdb` is not read at open; only `resume_create` reads it (§18.1).

Each file is opened with `taskdeck_seal::open_file` (`seal.md` §19.4). Its outcome:

| Outcome (`seal.md` §19.4) | Append kinds 1, 4, 5, 8, 11 | Replace kinds |
|---|---|---|
| clean | use every record | use the record |
| `seal/file_torn_tail` | truncate the file to `valid_bytes` (`truncate`, `sync`), use the records before it, add `torn_tail { kind }` to the open report | the file has one record: as `file_corrupt` |
| `seal/file_bad_tail` | as torn, and add `bad_tail { kind }` | as `file_corrupt` |
| `seal/file_corrupt`, `seal/file_header`, `device/record_format` | the policy of the next table | the policy of the next table |

A torn or bad tail on an append file is the write of a crash that did not reach its `sync`, so its `Persisted` was never delivered (DEV-FILE-004) and nothing durable is lost. A bad tail can also be damage to the last record; the report says so, and the engine's counters and echoes catch what it held (`engine.md` §11).

| File | On a corrupt file |
|---|---|
| `store.tds`, `identity.tdk`, `device.tdk`, `checkpoint.tdk` (or `bootstrap.tdk` when there is no checkpoint), a log segment above the checkpoint's seq, `outbox.tdo`, `meta.tdk`, `history.tdk`, `unsent.tdk` | `open` fails with `device/store_corrupt` naming the kind; nothing is written; the facade offers `join` as a new device (§18.4), keeping the old directory |
| a base, or a log segment wholly at or below the checkpoint's seq | the file is renamed to `<name>.corrupt-<YYYYMMDD-HHMMSS>` and left out of `Loaded`; reported as `quarantined { kind }` |
| `hashes.tdk`, `notices.tdk`, `marker.tdk` | renamed as above and treated as empty; reported |
| `overlay.tdk`, `feed-*.tdk`, `picture.tdk`, `own-picture.tdk`, `weather.tdk`, `draft.tdk` | removed and treated as absent; reported |

`checkpoint.tdk` and `meta.tdk` may be absent only while `bootstrap.tdk` exists or before the first checkpoint of a new identity; otherwise their absence is `device/store_corrupt`. The log records below `log_floor` of `device.tdk` are dropped from `Loaded`, and segments wholly below it are removed (the end of an interrupted `TruncateLogBelow`).

`Loaded` (`engine.md` §7.10) is filled from these files exactly: `checkpoint` from kind 2; `bases` from kind 3; `chains` and `tail` from kind 1 (`tail` = the records above `checkpoint.board_seq`); `log_floor` and `floor_chain` from kind 12 (`log_floor` 1 and `floor_chain` absent when `device.tdk` holds none); `hashes` from kind 4; `outbox` from kind 5 in file order; `notices` from kind 8 in file order; `meta` from kind 9; `marker` from kind 10; `build_id` from the caller (`OpenOptions`, §17.2); `session` 16 fresh bytes (§10.4); `bootstrap` from kind 17.

The confirmed `BoardData` is loaded from the checkpoint's board bytes with `BoardData::from_canonical_bytes`, exactly as stored: no id assignment, plan migration, detab or default substitution (V2-19, `core.md` §6.14).

- **DEV-FILE-007** (device) At open, the store MUST remove leftover `*.tmp` files before it opens any sealed file, and MUST treat each file's outcome as the tables of §7.4 state.
- **DEV-FILE-008** (device) The store MUST truncate a torn or bad tail of an append file to the last record that opened before it appends to that file again.
- **DEV-FILE-009** (device) The store MUST build `Loaded` from the files exactly as §7.4 maps them, MUST drop log records below `log_floor`, and MUST NOT present a log record, outbox op or notice op that did not open.
- **DEV-FILE-010** (device) A replica MUST load its `BoardData` only with `BoardData::from_canonical_bytes` from a checkpoint, a base, a snapshot or a bootstrap file, and MUST NOT normalise it in any way (V2-19).

### 7.5 Retention

The engine decides what to delete (`engine.md` §7.13); the store deletes only on its ops. The store adds: a quarantined file (`*.corrupt-*`) is removed when it is more than 2,592,000,000 ms (30 days) old by the device clock at an `open`; `feed-<id>.tdk` is removed when the board no longer has a subscription `id` or its URL hash changed (§13.1); `history.tdk` is never truncated, through purges included (`seal.md` §25).

- **DEV-FILE-011** (device) The store MUST NOT delete or truncate any file of an identity directory except by an engine op, the rules of §7.4, the three cases of §7.5, and the removal of a whole identity directory by `forget_identity` or `abandon_create` (§17.2, §18.1), and MUST NOT delete records of `history.tdk` in any other way.

### 7.6 Crash consistency

The rule that a file layer that kills the process at every write boundary must not break:

1. **Open.** After a kill at any write boundary, the next `open` succeeds and reaches the same mode.
2. **Durable means durable.** Every record whose `Persisted` was delivered before the kill is in `Loaded` (ENG-PERSIST-004); in particular every edit whose `submit` returned (§17.4) is in the outbox or already applied.
3. **Nothing half-made.** No file is read with a record that did not open; every replace leaves either the old or the new file.
4. **Same end.** Once the restarted device reconnects and the relay is honest, its state hash at the relay head equals that of a run without the kill, and no command is applied twice (`engine.md` §31.7 invariants 2 and 3).

The crash suite runs a scripted workload (enqueue, catch-up of 300 entries, a checkpoint, a base, a snapshot upload, a rotation, `RewriteOutbox`, `TruncateLogBelow`) with `CrashFiles`, once for every k from 1 to the number of write boundaries of the workload, on Linux, Windows and macOS.

- **DEV-CRASH-001** (device) For every write boundary k of the crash workload of §7.6, a run killed at k MUST reopen in the same mode, MUST present every record whose `Persisted` was delivered before the kill, and MUST reach the state hash of the uninterrupted run after it reconnects.
- **DEV-CRASH-002** (device) A `submit` that returned success before a kill MUST find its edit in the outbox or applied after the restart, and no `submit` MUST be applied twice.

## 8. The runtime: effects, events and what the store adds

### 8.1 Inputs the runtime delivers

The runtime builds every `Input` (`engine.md` §4.2) with `wall_ms` = the device's clock (`SystemTime::now`, milliseconds since the epoch) and `mono_ms` = `Instant::now` minus the `Instant` taken at `open`, in milliseconds, both read on the sync thread when the input is handed to the engine.

| Event | Delivered when |
|---|---|
| `Start` | once, after §7.4, before any other input |
| `Shutdown` | at `shutdown` (§17.2) |
| `Tick` | when the sync thread's wait reaches the deadline of the latest `ScheduleTick`, and once after every facade request that changed the foreground state |
| `Persisted`, `PersistFailed` | §7.3 |
| `LocalRead` | after the store answered a `ReadLog` or `ReadBase`, on the local queue |
| `Enqueue`, `ReleaseHold`, `Op`, `PairRegister`, `Resubmit`, `User` | from facade requests (§17) |
| `Connected`, `Welcome`, `Page`, `AppendResults`, `AdminResult`, `BlobResult`, `RelayRefused`, `Second`, `Displaced`, `Disconnected` | from the transport (§10.5) |

### 8.2 Effects the runtime performs

| Effect | Runtime |
|---|---|
| `Persist` | §7.3, on the sync thread |
| `ReadLog { from, to }` | reads the records from the segments (it keeps an index of each segment's first seq and record offsets); `LocalRead` with them; when it does not hold every record `from..=to`, or one does not open, `LocalRead { result: Missing { code: "device/store_corrupt" } }` |
| `ReadBase { seq }` | reads `base-<seq>.tdk`; `LocalRead` with the base. A missing or corrupt base: the runtime renames it as §7.4 does (reported as `quarantined`) and delivers `LocalRead { result: Missing { code: "device/store_corrupt" } }`; the engine stops with a local fault until the next `open` (`engine.md` §21.4), when the base is absent from `Loaded.bases` |
| `Connect`, `Disconnect`, `Hello`, `Read`, `Append`, `Admin`, `Blob` | handed to the transport (§10.5) in emission order (ENG-EFFECT-001) |
| `ScheduleTick { at_mono_ms }` | replaces the sync thread's tick deadline |
| `Notify(n)` | §8.4 |

- **DEV-RUN-001** (withdrawn) Replaced by ENG-EFFECT-001 (`engine.md` §4.3), which states the same rule for the runtime and names `device` as an owner.
- **DEV-RUN-002** (device) The runtime MUST deliver a `Tick` no later than 50 ms after the `mono_ms` of the latest `ScheduleTick` while the sync thread is otherwise idle.
- **DEV-RUN-007** (device) The runtime MUST answer every `ReadLog` and `ReadBase` with one `LocalRead`, and MUST answer one it cannot serve in full with `Missing`.

### 8.3 What the store adds around the engine's records

The engine persists its own records as effects (`engine.md` §7). The store adds exactly these, each in the same `Persist` as the op it names, before that `Persisted` is delivered:

| Addition | Written | Why |
|---|---|---|
| history records (kind 11) | after an `AppendLog`: one record for each accepted control entry among the appended seqs, built by `taskdeck_seal::HistoryRecord::from_accepted(seq, frame, opened)` from the sealer adapter's classification (it keeps the frame and the `Opened` of every `Accept` of kind `ctl` from the dry run); only seqs above the last seq in `history.tdk`, so a re-classification after `Start` adds nothing twice | backups carry the full membership history (`seal.md` §21.5, SEAL-BKP-002); the log loses it below `log_floor` |
| history records from an opened snapshot | after the sealer adapter's `open_snapshot` returned a body (a rejoin, `engine.md` §19.7), in the first `Persist` that carries a `WriteCheckpoint`, before that op: the body's history records (`seal.md` §20.4) with seq above the last seq in `history.tdk`, in seq order; the adapter then drops the body's history | a device that rejoined from a snapshot holds the control history from seq 1 (§15.7, ruling S27) |
| `log_floor` and `floor_chain` in `device.tdk` | before a `TruncateLogBelow` removes anything | DEV-FILE-006 |
| `minter` in `device.tdk` | before a `RewriteOutbox`: the element-wise maximum of the stored `minter` and the `IdMinter` of every `Put` in `outbox.tdo` | so that numbers minted for records that left the outbox are never minted again (`core.md` §7.2) |
| removal of `bootstrap.tdk` | after the first `WriteCheckpoint` | the checkpoint now holds the state |

Outside `Persist` effects, the store writes `device.tdk`, `overlay.tdk`, the feed files, `picture.tdk`, `own-picture.tdk`, `weather.tdk` and `draft.tdk` on its own, from the sync thread, by the replace procedure. Nothing the engine reads at `Start` lives in those files except `log_floor` and `floor_chain`.

**The origin state.** `Enqueue.origin_state` (`engine.md` §9.1) is the `IdMinter` after the compile step advanced it: `device_no u16 ‖ next_item u64 ‖ next_block u64` (18 bytes). At `open`, the facade's minter is the element-wise maximum of `device.tdk`'s `minter` and every `Put`'s `origin_state` in `outbox.tdo`, in file order; the core's minting rule then also takes `mark + 1` from the view (`core.md` §7.2), which raises it after a catch-up that showed higher marks (`engine.md` §11.3).

- **DEV-RUN-003** (device, recovery) After each `AppendLog`, in the same `Persist` effect and before its `Persisted`, the runtime MUST append to `history.tdk` one history record (`seal.md` §21.5) for every accepted control entry among the appended seqs that is above the last seq the file holds.
- **DEV-RUN-004** (device) The runtime MUST persist in each `Enqueue` the minter state after compiling as `origin_state`, laid out as §8.3 states, and MUST never mint from a minter below the maximum of §8.3.
- **DEV-RUN-005** (device) Before a `RewriteOutbox` drops any `Put` from `outbox.tdo`, the runtime MUST make durable in `device.tdk` a minter at least as high as every `origin_state` the file holds.
- **DEV-RUN-006** (device) After the sealer adapter opened a snapshot, the runtime MUST append to `history.tdk`, in the first `Persist` that carries a `WriteCheckpoint` and before that op, every history record of the snapshot's body whose seq is above the last seq the file holds, in seq order.

### 8.4 Notifications

The runtime turns each `Notify` (`engine.md` §30.3) into a facade effect:

| Notification | Runtime |
|---|---|
| `Enqueued`, `EnqueueRefused` | answers the waiting `submit` (§17.4) |
| `ViewChanged`, `StatusChanged`, `NoticesAdded`, `BannersChanged`, `AlarmRaised` | raise the matching change counters (§17.3) |
| `OpProgress`, `OpDone` | update the operation's state and answer a waiting `wait_op` (§17.8) |
| `PairRegistered`, `PairFailed` | feeds the existing-device pairing machine (§18.3) |
| `MoveCode`, `MoveReport` | keeps them for `move_status` (§17.8) |
| `MoveCodeResult { result }` | answers the waiting `enter_move_code` (§17.8) |
| `Unsent` | keeps them for `re_pair` (§18.4) |
| `ResubmitReport` | answers `re_pair` |
| `ReplaceReady` | raises `status` |
| `PurgeBackups { before_ms }` | records the offer; `delete_backups_before` acts on it (§15.6) |
| `ReuploadPicture { epoch }` | a desktop that holds the clean picture re-uploads it (§14.1) |
| `RecoveryKeyChanged { key_fp }` | sets `backup.pin_needed = key_fp` (§15.5) |
| `ReadyToExit` | ends `shutdown` (§17.2) |

## 9. Local-only mode

### 9.1 What it is

Local-only mode is the core over today's plain files with no relay (SYNC_PLAN.md §6.7 item 1). With no sync configured the desktop's wall calendar behaves as today: the same files, in the same formats, written in the same order, so that today's build and the older `main` build can still open the folder (SYNC_PLAN.md §3.2: "every merge keeps today's app working"), and "Create board from this folder" (§18.2) and "Export legacy" (§18.7) read and write one format.

The sync thread holds the board as a `BoardData` and a local counter `local_seq`, starting at 1 at each `open`. There is no engine, no outbox, no notice list and no sealed file.

### 9.2 Loading

At `open`, in order:

1. Read the plain files as `LegacyFolder` (`core.md` §15.2): `read_at_startup.json`, `archived.jsonl`, `notepad_text.json`, `subscriptions.json`, `colorschemes.json`, and `LegacyConfig` from `userconfig.toml` with today's parser (read only, never normalised here).
2. Run `taskdeck_core::legacy::import`. On a failure:
   - `items_unreadable`: rename `read_at_startup.json` to `read_at_startup.json.corrupt-<YYYYMMDD-HHMMSS>` (today's `quarantine_corrupt_file`, tasks.rs:705-723), report it, and import again without it;
   - `notes_unreadable`: rename `notepad_text.json` likewise, report it, import again without it;
   - `subscriptions_unreadable`: leave the file in place (as today, board.rs:597-610), report it, import again without it, and mark subscriptions **frozen**: the subscription intents are refused with `device/subscriptions_frozen` and `subscriptions.json` is never written, until the next `open`;
   - any other failure (`too_large`, `id_range`, `instant_range`, `field_range`, `cap`, `invariant`): `open` fails with `device/legacy_unreadable` and the import report, whose failure names its kind and the ids involved, and writes nothing. The desktop shows the report, so that the user can repair those items in today's build, which still opens the folder (§26 open question 23).
3. Read `subscribed_cache.json` as today (`read_overlay`, subscriptions.rs:511-518): unreadable means an empty overlay.
4. The import's renumberings, warnings and quarantines go into the open report (§17.2); the report holds kinds, ids and counts only (CORE-IMP-003).

Ids minted in local-only mode use `device_no` 1: the compile step runs with `CompileCtx.device_no = 1` and an `IdMinter` for 1, and `apply` with `Ctx { seq: local_seq + 1, device_no: 1, at: now, based_on: local_seq }`, after which `local_seq` is raised by one. These ids are `2^40 + n`, below 2^53, which today's files hold as plain integers; the import keeps them on the next `open` and at "Create board from this folder" (`core.md` §15.3 step 9.4) and sets the mark of `device_no` 1 to the largest of them (step 14), so the genesis device, which is `device_no` 1 (`seal.md` §13.7), never mints one of them again. Stamps, `notes_rev` and marks are not stored in today's files and are rebuilt at each `open` by the import; block ids are renumbered by it (`core.md` §15.3 step 10), which no caller observes across an `open`. Because the marks are rebuilt from the files, an item or subscription number that was the highest one and has since been forgotten (`forget`, `forget_archived`, `remove_subscription`) may be minted again after a later `open`; nothing in the board refers to it then.

### 9.3 Writing: the local Store

A local-only `submit` runs, on the sync thread: parse (or take) the intent; `compile` against the board with the device's zone and the overlay; `apply` to a clone of the board under `catch_unwind`; on success write the plain files from the change set; then replace the board with the clone and publish. The write order is today's (R1 §3, board.rs:1475-1487, 1261-1315; `core.md` §8.3):

1. `archive_inserted` (complete, delete): append each inserted row as one JSON line to `archived.jsonl` (`archive::append`: serialise, append, `sync_all`).
2. `archive_removed` (restore, forget archived): rewrite `archived.jsonl` whole (`ArchiveLog::rewrite`: unreadable lines first, verbatim, each followed by `0x0A`, then every row oldest first in the archive order, one JSON line each; temporary file, `sync_all`, rename, directory sync).
3. `items`: rewrite `read_at_startup.json` (`oversafe_activesave`: pretty JSON array of today's `Active`, in ascending id).
4. `notes`: rewrite `notepad_text.json` (pretty JSON string).
5. `subscriptions`: rewrite `subscriptions.json` (pretty JSON array of today's `Subscription`), unless frozen.
6. `settings`, when the clone's `home` differs from the board's `home` before the apply: write `coordinates = [lat / 100, lon / 100]` to `userconfig.toml` with today's single-key writer (`write_config_value`). A palette change writes nothing: in local-only mode the palette comes from `colorschemes.json` and `selected_colorscheme_id`, which `desktop` manages as today. `set_picture` is refused before compile with `device/needs_sync`.
7. `overlay_forget`: drop those subscriptions' events and statuses from the overlay and rewrite `subscribed_cache.json` (compact JSON) when the overlay changed.

If a write fails, the board is not replaced, the files written so far stay as they are (as today: an archive row appended before the items rewrite failed is found again by the next `open`'s import as a row, so the item shows as completed), and `submit` returns `device/write_failed`.

**Mapping to today's types.** `Item` to `Active`: `id`, `name`, `importance`, `time_importance`, `duration_minutes` and `recurrence` (`days`, `anchor`, `start_minutes`, `minutes`) unchanged; `created` and `deadline` as `DateTime<Local>` in the device's zone, written by `chrono`'s `serde` (RFC 3339 with the local offset and up to nine fraction digits, so the instant round-trips); `is_event = (kind == event)`; `sessions` = the blocks in ascending sid, each `{start, minutes}`; `planned_start` absent. `ArchiveRow` to `Archived` likewise, with `inactivated = archived_at` and `outcome`. `Subscription` unchanged.

- **DEV-LOCAL-001** (device) In local-only mode the device MUST write the plain files from each change set in the order of §9.3 steps 1 to 7, each by today's atomic or append procedure, before `submit` returns.
- **DEV-LOCAL-002** (device) In local-only mode the device MUST compile with `device_no` 1 and apply with the `Ctx` of §9.2, and MUST raise `local_seq` by one per applied command.
- **DEV-LOCAL-003** (device) The plain files written in local-only mode MUST parse with today's types (`Active`, `Archived`, `Subscription`) and MUST give, through `legacy::import`, a board with the same items, rows, notes, subscriptions and home place.
- **DEV-LOCAL-004** (device) At a local-only `open`, the device MUST handle an import failure exactly as §9.2 step 2 states for its kind.
- **DEV-LOCAL-005** (device) A local-only `submit` whose plain-file write fails MUST leave the in-memory board unchanged and MUST return `device/write_failed`.

### 9.4 How a local-only board becomes a synced one

Two ways, both through §18:

- **Create board from this folder** (desktop): the import of the folder's plain files is the genesis board (`core.md` §15.1); after the relay accepted the genesis and the identity is durable, the plain files are moved, untouched, to `pre-sync-<YYYYMMDD-HHMMSS>/` (SYNC.md §9 step 6, §18.2).
- **Join an existing board** (any device): the plain files, if any holds data (§18.4), are moved to `local-<YYYYMMDD-HHMMSS>/` before the joined identity is made active, so a local board is never overwritten (today's first-contact rule, DOCUMENTATION.md §22.3).

The folder watch of SYNC.md §9 step 7 is dropped (SYNC_PLAN.md §2).

## 10. Transport

### 10.1 Stack and origins

HTTPS requests use `ureq` 3.5.2; the socket uses `tungstenite` 0.30.0. Both run over `rustls` 0.23.46 with the ring provider and `rustls-platform-verifier` 0.7.1, TLS 1.2 and 1.3, the provider's default cipher suites (SYNC.md §2.3, §5.1). The relay origin is `taskdeck_seal::relay_origin(host)` of the stored relay target's host (`seal.md` §5.4: `https://` ‖ host, since a host never carries `:443`); the socket URL is `wss://<host>/v1/b/<board hex>/socket`.

On Android the platform verifier is initialised by the shell before `open` (`bridge.md`); when `android-shell` decides that an API level verifies with bundled roots instead (SYNC_PLAN.md §6.6 item 7), the shell passes `TrustRoots::Bundled` in `OpenOptions` and the device builds its verifier from `webpki-roots` 1.0.12 for that run. No other trust configuration exists in a release build.

- **DEV-NET-001** (device) The device MUST send every relay request over TLS to the origin `https://<host>` (or `wss://<host>`) of its stored relay target, and MUST refuse with `device/insecure_url`, before any connection, a relay host, ICS URL or redirect target whose scheme is not `https` (SYNC.md §7, the network row), except the loopback URLs that `test-trust` allows (§11.1 item 1).
- **DEV-NET-002** (device) The device MUST verify every server certificate with `rustls-platform-verifier`, or with `webpki-roots` when the caller passed `TrustRoots::Bundled`, and MUST NOT offer, read or honour any option, file or environment variable that adds a trust anchor, disables verification or sets a proxy, except the feature `test-trust` of §11.
- **DEV-NET-003** (device) Every HTTP client the device builds MUST have no proxy (`ureq` configured with no proxy and not reading `HTTPS_PROXY`, `ALL_PROXY` or `NO_PROXY`), MUST NOT follow redirects on relay requests, MUST NOT send cookies, and MUST NOT send a `Referer` header.

### 10.2 Headers

Every relay request carries exactly: `Host`; `User-Agent: TaskDeck-Sync/1`; `Authorization` with the credential of §10.5 in the forms of `relay.md` §5.6; `Content-Length` (and `Content-Type: application/octet-stream`) when it has a body; `X-TD-Proof` on the pairing routes that need it; `Accept-Encoding: identity`. The socket upgrade carries `Host`, `User-Agent`, `Authorization` and the WebSocket upgrade headers. ICS requests carry `User-Agent: TaskDeck-Calendar/1`; Open-Meteo requests `User-Agent: TaskDeck-Weather/1`. The three strings are constants: they hold no version, platform or device name, so they say nothing a stranger could use to tell devices apart, and they satisfy the relay's need for a descriptive agent (SYNC.md §6.1).

- **DEV-NET-004** (device) Every relay request and socket upgrade MUST carry `User-Agent: TaskDeck-Sync/1` and no header other than those §10.2 lists.
- **DEV-NET-005** (device, recovery) A relay token MUST be treated as an opaque ASCII string, MUST travel only in the `Authorization` header of a request to the relay origin that issued it (`relay.md` §5.1), and MUST NOT appear in a URL, a query, a log, an error or a devicectl output.

### 10.3 Tokens and login

The device keeps one token per relay host in `device.tdk` (§16), with the `gen` the login returned. It logs in (`relay.md` §10.3, `seal.md` §16.1):

1. when a `Connect` finds no token for the target host;
2. when a request is answered `401 relay/token_stale`, or the socket is closed with 4003 while the device is not revoked;
3. after a re-home, on the new relay, with the door.
4. at every `Connect` while the engine is stopped `Revoked` (`engine.md` §24.4, its status's `stops`), even with a stored token, so that the answer carries the wraps of a reinstatement (ruling S24); `403 relay/revoked` ends the `Connect` as §10.6 states.

Login: `POST /v1/b/{board}/login/nonce` with body `dev`, credential the stored token (any generation) or, when there is none, `Door <door hex>` with the door of the `Connect`'s relay target (`RelayTarget.door`: `meta.relay`, or the bootstrap's relay target before the first checkpoint, `engine.md` §7, §7.10); then `taskdeck_seal::sign_login(keys, origin, board, gen, nonce)` with `origin = taskdeck_seal::relay_origin(host)` of the device's own relay target, never from the relay; then `POST …/login` with `dev ‖ gen ‖ nonce ‖ client_nonce ‖ sig`. The answer's token and `gen` replace the stored ones in `device.tdk` (replace procedure) before the token is used; `push` updates the stored capability. When a `login` answer carries the wraps of the epoch roots this identity missed while it was revoked (its field `wraps`: `first_epoch` and the wraps in epoch order; ruling S24, `relay.md` §10.3, §12.5), the runtime sends `MissedRoots { first_epoch, wraps }` to the engine (`engine.md` §4.2) right after that login, before any other input, each wrap decoded from its 192 hex characters, and the engine hands them to the sealer adapter (`Sealer::take_missed_wraps`, `engine.md` §6.1, §24.4), which holds them in memory for classification, which opens each wrap with `unwrap_root` once its membership records that epoch's commitment, oldest epoch first, and checks the root against it (`seal.md` §7, SEAL-WRAP-002, SEAL-KEY-013), and the roots reach `identity.tdk` only through the engine's `WriteIdentity` after `keys_changed` (`engine.md` §6.1, D4). A wrap that does not open or a root that does not match is the commitment halt (`engine.md` §16.1). A request answered `token_stale` is sent once more with the new token; a second `token_stale` for the same request is handed to the engine as `RelayRefused`. A `login` refused with `401 relay/no_nonce` or `409 relay/bad_gen` (an expired or evicted nonce, or another login of this device in between) is restarted once from `login-nonce`; any other JSON refusal of `login-nonce` or `login`, or a second refusal of the restarted login, ends the `Connect` as §10.6 states.

**Tokens stored without a login.** The token of `create` (§18.1 step 9), the bundle's token (§18.3) and the token of `Seed` (§10.5) are stored without `push` and with `gen = 1`, the generation `relay.md` §5.2 gives every token that `create`, `register` and `seed` return. A `Connect` whose stored token has no `push` first sends `GET …/caps` with that token and stores the answer's `push` with it (replace procedure on `device.tdk`) before it chooses between the socket and HTTPS; a refused or failed `caps` ends the `Connect` as §10.6 states. A login answer's `push` replaces the stored one.

- **DEV-LOGIN-001** (device) The device MUST sign every login with the relay origin that `taskdeck_seal::relay_origin` builds from its own stored relay target's host, and MUST make the new token durable in `device.tdk` before it sends any request with it.
- **DEV-LOGIN-002** (device) The device MUST log in only in the four cases of §10.3, and MUST retry a request refused with `relay/token_stale` at most once after a successful login.
- **DEV-LOGIN-003** (device) The device MUST restart from `login-nonce`, at most once per `Connect`, a login refused with `401 relay/no_nonce` or `409 relay/bad_gen`, and MUST end the `Connect` as §10.6 states for every other refusal of `login-nonce` or `login` and for a second such refusal.
- **DEV-LOGIN-004** (device) The device MUST store a token it received from `create`, from a pairing bundle or from `seed` with `gen = 1`.

### 10.4 Session id

At each `open` in synced mode the device draws a 16-byte session id from `OsEntropy`. It is `Loaded.session` and the `session` of every socket hello of this process (`relay.md` §11.3, `engine.md` §11.6). It is never stored.

- **DEV-NET-006** (device) The device MUST draw one fresh session id per `open`, MUST send that same id in every socket hello of the process, and MUST NOT persist it.

### 10.5 Effects and the requests they make

`board` is the board id in 32 hex; `T` the stored token; `D` the door of the target. "Socket" means the socket is open and has received its welcome.

| Effect | Requests | Answer delivered |
|---|---|---|
| `Connect { relay }` | wait the backoff (§10.7); login when §10.3 requires it; when the stored token has no `push`, `GET …/caps` with `T` and store its `push` (§10.3); then, when the stored capability says `push` and the device is in the foreground (§10.9): open the socket with `T`, send `hello` (`0x01 ‖ session ‖ u32 0`) and wait for `welcome`; otherwise nothing more | `MissedRoots { first_epoch, wraps }` first when a login of this `Connect` answered `wraps` (§10.3); then socket: `Connected { push: true }`, then `Welcome` from the `welcome`; HTTPS: `Connected { push: false }` |
| `Disconnect` | close the socket with 1000; answers of requests already sent are discarded | nothing |
| `Hello` | `GET …/hello` with `T` | `Welcome` from the JSON (`fenced` = `state == "fenced"`; `fence` from the `fence` object) |
| `Read { req, after, limit }` | socket: `0x03 ‖ u32 r ‖ u64 after ‖ u16 limit`; else `GET …/log?after=<after>&limit=<limit>` with `T` | `Page { req }` from the page (`relay.md` §10.5) |
| `Append { req, frames }` | the batch `"TDA1" ‖ u8 n ‖ n × (u32 len ‖ frame)` (`relay.md` §9.2); socket: `0x02 ‖ u32 r ‖ batch`; else `POST …/append` with `T` | `AppendResults { req }` from the results body (`relay.md` §9.6) |
| `Admin { req, op: Door { host } }` | `GET https://<host>/v1/b/<board>/door` with the create secret held for the operation as credential (`relay.md` §10.9, §5.6); no admin nonce and no signature | `AdminResult { req }`: on 200, `Ok(AdminOk { door: Some(d), token: false })` with every other field `None`; a JSON error as §10.6 |
| `Admin { req, op: Seed { host, … } }` | on the new relay, origin `relay_origin(host)` (`seal.md` §5.4): `POST …/admin/nonce` with `Authorization: Create <secret>` (the create secret held for the operation, §17.8) and body this device's `dev` as the signer; then `POST …/admin/seed` with the envelope of `relay.md` §12.1, `sig = sign_admin(Signer::Device(keys), op, board, nonce)`, credential the create secret | `AdminResult { req }`: on 200, the answer's `token` is first stored in `device.tdk` `tokens` under `host` (replace procedure; without `push`, §10.3), then `Ok(AdminOk { door: Some(d), token: true })` with every other field `None`; `next_seq` and `state` are parsed and not passed on; a JSON error as §10.6 |
| `Admin { req, op: SeedTail { host, first, frames } }` | `POST …/seed/tail` on the origin `relay_origin(host)`, with the token stored for `host` by `Seed` (`Bearer`) and the body `u64 first ‖ u16 n ‖ n × (u32 len ‖ frame)` (`relay.md` §12.7); no admin nonce and no signature | `AdminResult { req }`: on 200, `Ok(AdminOk { head: Some(head), token: false })` with every other field `None`, `state` parsed and not passed on; a JSON error as §10.6, with `head` (`relay/not_next`) and `index` (a refused frame) in its `RelayRefusal` |
| `Admin { req, op }`, every other `op` | on the current relay target: `POST …/admin/nonce` (body: the signer, this device's `dev`), then `POST …/admin/<name>` with the envelope of `relay.md` §12.1, `sig = sign_admin(Signer::Device(keys), op, board, nonce)`; credential `T` | `AdminResult { req }` |
| `Blob { Upload }` | `POST …/blobs` (begin), then `PUT …/blobs/<id>/<i>` for each part not yet stored; after an interruption, `GET …/blobs/<id>` and the missing parts only | `BlobResult { Uploaded }` |
| `Blob { Fetch { id, parts } }` | `GET …/blobs/<id>`, then `GET …/blobs/<id>/<i>` for `i` from 0 to `parts − 1` | `BlobResult { Parts }` |
| `Blob { List }`, `Blob { Meta }` | `GET …/blobs`, `GET …/blobs/<id>` | `BlobResult { List }`, `BlobResult { Meta }` |

`r` is a socket request id the transport draws from its own counter and maps back to `req`. A socket `push` is delivered as `Page { req: None }`; `second` as `Second`; `displaced` as `Displaced`; `slot` to the pairing machine (§18.3); `state` (fenced) as `RelayRefused { req: None, 403 relay/fenced }`.

Every relay response body is parsed strictly before it becomes an event: binary bodies by their layout (`relay.md` §9.6, §10.5, §11.3), JSON by the tables of `relay.md` with unknown fields, missing required fields, a `null` or a float refused (README §5.3). A body that fails is delivered as `RelayRefused { status: 0, code: "device/bad_answer" }` for that request.

- **DEV-NET-007** (device) The transport MUST perform each effect with exactly the requests of the table of §10.5, in that order, and MUST deliver exactly the event the table names for its answer.
- **DEV-NET-008** (device) The transport MUST open a socket only when the relay's capability says `push` and the device is in the foreground, and MUST NOT open a socket to a relay that reported `push: false` (`relay.md` §15.1).
- **DEV-NET-009** (device) The transport MUST parse every relay answer strictly by its layout or JSON table before building an event from it, and MUST deliver `device/bad_answer` for one that fails.
- **DEV-NET-010** (device) The transport MUST resume an interrupted blob upload from `blob-meta`, sending only the parts the relay lacks, and MUST send each part's identical bytes on every retry.
- **DEV-NET-018** (device) Before it acts on a `Connect` with a stored token that has no `push`, the transport MUST send `GET …/caps` with that token and MUST store the answer's `push` with the token in `device.tdk`.

### 10.6 Errors and quota signals

Every exchange is classified by the table of `relay.md` §14.5 first, then by its code (DEV-NET-011):

| The transport observes | Event to the engine | The interfaces show |
|---|---|---|
| no response: DNS, TCP or TLS failure, or a timeout of §10.10 | `Disconnected { unreachable: true }` | "relay unreachable since 14:05 · 3 waiting" (`engine.md` §30.1) |
| a response without `X-TD-Relay`, status 429 | `Disconnected { unreachable: false, paused_until_ms: Some(next 00:00 UTC of the device clock) }` | "sync paused until 00:00 UTC" |
| a response without `X-TD-Relay`, any other status; `503 relay/object_unavailable`; `503 relay/misconfigured` | `Disconnected { unreachable: true }` | as unreachable |
| `408 relay/slow` answering any request (`relay.md` §6.2 step 9, §14.5) | `Disconnected { unreachable: true }`: the session ends and the request is unanswered (`engine.md` ENG-SEND-007); a blob upload resumes after the backoff as DEV-NET-010 states | as unreachable |
| `507 relay/storage_refused` | `RelayRefused { status: 507, code, until: Some(next 00:00 UTC) }` | "sync paused until 00:00 UTC" |
| `429 relay/budget` as a frame result | inside `AppendResults` (`engine.md` §10.4) | "daily sync limit reached until 00:00 UTC" |
| `429 relay/budget` as JSON answering `Hello`, `Read` or `Append` | `RelayRefused { req, … }` with `until` from the body | as above |
| `429 relay/budget` or `429 relay/delete_budget` as JSON answering an `Admin` | `AdminResult { req, Err(RelayRefusal { status: 429, code, until }) }`, `until` from the body | the operation's result; nothing for the delete budget |
| `401 relay/token_stale` | none: login and one retry (§10.3) | nothing |
| any JSON error answering a request of a `Connect`: `login-nonce` or `login` (§10.3), except a first `401 relay/no_nonce` or `409 relay/bad_gen`, which restarts the login once; `GET …/caps` (§10.3); or the socket upgrade (`relay.md` §11.1) | `RelayRefused { req: None, refusal }`, then `Disconnected { unreachable: false, paused_until_ms: None }`; the `Connect` counts as a failure (§10.7) | `engine.md` §11.9 |
| `403 relay/fenced`, `403 relay/revoked`, `404 relay/no_board`, `409 relay/seeding` answering a whole request of an effect other than `Connect` | `RelayRefused { req, … }` | `engine.md` §11.9 |
| any other JSON error answering an `Admin` or a `Blob` | `AdminResult` or `BlobResult` with `Err(RelayRefusal)` | the operation's result |
| any other JSON error answering `Hello`, `Read` or `Append` | `RelayRefused { req, … }` | `engine.md` §11.9 |
| socket close 4001 | none (the `displaced` message came first) | |
| socket close 4002 | none: it closes a socket the transport has already replaced (`relay.md` §11.3) | nothing |
| socket close 4003 | login, then `Disconnected { unreachable: false }` | |
| socket close 4004 or 1009 | `Disconnected { unreachable: false }` | |
| any other socket close or socket error | `Disconnected { unreachable: true }` | |

A close, message or error of a socket other than the transport's current one is discarded and produces no event.

A refused login, `caps` or socket upgrade reaches the engine as `RelayRefused { req: None, refusal }`, then `Disconnected { unreachable: false, paused_until_ms: None }`, while the session is `Connecting`; the `Disconnected` takes the session to `Offline`, and the guards on `Offline` decide the next `Connect`. (`engine.md` §11.1, §11.9.)

"Next 00:00 UTC" is `(floor(wall_ms / 86,400,000) + 1) × 86,400,000`. No row of this table deletes or forgets anything (DEV-NET-012, `engine.md` §11.8); only a successful login replaces a token.

The error-body fields `last_ctr` and `last_seq` (`relay.md` §4.2) are parsed with the rest of the body and are not passed to the engine, which knows its own counter from its log (`engine.md` §11.4); only `RecoveryLink::fence` returns them (§15.8, §25).

- **DEV-NET-011** (device) The transport MUST turn every relay exchange into exactly the event of the table of §10.6.
- **DEV-NET-012** (device) The device MUST NOT delete a local file, a record, a key or a token because of any relay answer, close or silence, except that a successful login replaces the stored token.
- **DEV-NET-019** (device) The transport MUST deliver no event for a close, message or error of a socket other than its current one, and MUST deliver none for a close with code 4002.
- **DEV-NET-020** (device) The transport MUST deliver a JSON refusal of the `caps` request or the socket upgrade of a `Connect` as `RelayRefused { req: None, … }` followed by `Disconnected { unreachable: false, paused_until_ms: None }`, as it delivers a refused login.

### 10.7 Backoff

The transport waits before it acts on a `Connect`:

- no wait for the first `Connect` after `open`, after a reset, and once after `notify_resume` (SYNC.md §4.9: "backoff bypassed once on resume");
- after the `k`-th consecutive failure (`k ≥ 1`), a wait drawn uniformly from the integers 1,000 to `min(60,000, 1,000 × 2^(k − 1))` ms (full jitter, SYNC.md §6.2).

A **failure** is a session that ended (`Disconnected`) without a welcome, or less than 60,000 ms after its welcome. `k` returns to 0 only when a session has stayed open for 60,000 ms after its welcome (a **healthy** session), or at `open`. While the device is revoked or the board is missing at the relay, the engine itself limits reconnects to one per 3,600,000 ms (`engine.md` §11.9, §24.4).

- **DEV-BACKOFF-001** (device) Before acting on a `Connect` after the `k`-th consecutive failure, the transport MUST wait a uniformly drawn number of milliseconds from 1,000 to `min(60,000, 1,000 × 2^(k − 1))`.
- **DEV-BACKOFF-002** (device) The transport MUST reset its failure count only after a session stayed open 60,000 ms past its welcome, or at `open`, and MUST skip the wait exactly once after each `notify_resume`.

### 10.8 Budgets

The device keeps the client's request budget (`relay.md` §14.3, ruling S23): at most 5,000 requests per UTC day of its own clock, counting every HTTPS request to the relay and every socket message it sends, and not counting protocol pings or `ping` texts. The count `(utc_day, count)` is written to `device.tdk` every 100 requests and at `shutdown`; at `open`, when the stored day is today's, the count starts at the stored value plus 100. At 5,000, the transport answers every relay effect with `Disconnected { unreachable: false, paused_until_ms: Some(next 00:00 UTC) }` without sending, except one `Hello` per 900,000 ms (`relay.md` §14.3). The status shows the count while it is above 4,000.

The relay's own budgets (3,000 appends per device, 5,000 deletions per board) reach the engine through §10.6.

- **DEV-BUDGET-001** (device) The device MUST NOT send more than 5,000 requests to the relay per UTC day of its own clock, counting as §10.8 states, and over the budget MUST send nothing but one `Hello` per 900,000 ms until the next 00:00 UTC (`relay.md` §14.3).
- **DEV-BUDGET-002** (device) The request count MUST survive a restart as §10.8 states, starting at the stored count plus 100 on the same UTC day.

### 10.9 Foreground, background and polling

| Platform state | Relay pushes (Worker) | Relay does not push (standby, ruling S9) |
|---|---|---|
| desktop app running | one socket; reads and appends over it | HTTPS; the engine sends `Hello` every 30,000 ms while `Live` (`engine.md` §11.1) |
| Android, activity visible | one socket | HTTPS polling as above |
| Android, not visible, no job | the transport closes the socket, delivers `Disconnected { unreachable: false }`, and holds every `Connect` until a background run or the foreground | the same |
| Android background run | HTTPS only, for at most its budget | HTTPS only |

`set_foreground(bool)` (§17.9) tells the device the state. At `open` the state is foreground on desktops and not foreground on Android (`Platform::Android`), until the first `set_foreground(true)`. On `true` it calls `notify_resume` and lets the next `Connect` through; on `false` on Android it closes the socket. A **background run** is `background_sync(budget_ms)` (§17.9), called by the Android job every 30 minutes and by the expedited job when the app leaves the foreground with queued edits (API 31 and later): it lets one `Connect` through over HTTPS, and returns when the engine's status is `Live` with nothing queued and no request outstanding, or when `budget_ms` (at most 600,000) has passed, whichever is first; then it holds `Connect` again. Doze and standby buckets only delay jobs; they show as "last seen" on the other devices (`engine.md` §30.2).

- **DEV-NET-013** (device, android) On Android, the device MUST keep no socket and send no relay request while the app is not in the foreground, except during a background run.
- **DEV-NET-014** (device) A background run MUST use HTTPS only and MUST return no later than its budget, at most 600,000 ms.
- **DEV-NET-017** (device) At `open`, the device MUST take the foreground state as true on desktops and as false on Android, and on Android MUST keep it false until a `set_foreground(true)`.

### 10.10 Timeouts

| What | Limit |
|---|---|
| DNS, TCP and TLS handshake | 10,000 ms |
| HTTPS response headers after the request is sent | 20,000 ms |
| HTTPS body, either direction | 20,000 + ceil(bytes × 1,000 / 65,536) ms, so uploads run at no less than 64 KiB/s (`relay.md` §6.2) |
| socket upgrade and `welcome` after `hello` | 20,000 ms |
| socket protocol ping | every 30,000 ms (`relay.md` §11.5) |
| socket silence (no frame, pong included) | 75,000 ms, then close as unreachable |

A timeout is a failure of §10.6's first row.

- **DEV-NET-015** (device) The transport MUST apply every limit of §10.10 and MUST treat its expiry as `Disconnected { unreachable: true }`.
- **DEV-NET-016** (device) While a socket is open, the transport MUST send a WebSocket protocol ping every 30,000 ms.

## 11. Test-only trust

### 11.1 The feature

`taskdeck-device` has one cargo feature for test-only trust, **`test-trust`**. No default feature set of any crate in the workspace enables it, and no crate a release artifact is built from depends on `taskdeck-device` with it (SYNC_PLAN.md §3.4). It enables exactly:

1. relay targets and ICS URLs with the scheme `http` whose host is `127.0.0.1`, `[::1]` or `localhost`, for the standby relay and the fetcher test server on loopback (`relay.md` §15.2);
2. `TrustRoots::Extra(pem)`: one additional trust anchor, a test certificate, added to the platform verifier's roots;
3. the address guard's exception for loopback addresses when the URL's host is one of the three loopback names of item 1 (§13.1.4);
4. the test key stores `FileKeyStore` and `MemoryKeyStore` (§12.6);
5. `CrashFiles` (§7.6) and an injectable clock (`OpenOptions.clock`);
6. the binary `taskdeck-devicectl` (§19), whose target has `required-features = ["test-trust"]`;
7. the constant `TEST_TRUST_MARKER = "taskdeck-test-trust-enabled-6f1c2a"`, which `open` passes to `std::hint::black_box` so that every build with the feature contains the string;
8. the test hooks of the end-to-end suite (§20.2): a state-machine wrapper that advertises supported rules `[1, 2]` with version 2 equal to version 1, a hook that corrupts the board bytes handed to `seal_snapshot`, a hook that makes the ICS parse panic for one URL, a test resolver for the address guard, and seeded jitter.

Without the feature none of these items exists in the compiled crate: the code is behind `#[cfg(feature = "test-trust")]`, not a runtime switch.

### 11.2 How `release` proves an artifact lacks it

1. **At build time**, the release workflow runs `cargo tree -e features --locked` with the exact package, target and feature arguments of each release build, and fails when the output contains `taskdeck-device feature "test-trust"`, `taskdeck-seal feature "test-entropy"` or `taskdeck-sync feature "sim"`.
2. **On the artifact**, a script searches every release binary (the Windows and macOS apps, `libtaskdeck.so` for each ABI inside the APK, `taskdeck-recover`) for the byte strings `taskdeck-test-trust-enabled-6f1c2a` and `taskdeck-test-entropy-enabled-3b9e5d` (`taskdeck_seal::TEST_ENTROPY_MARKER`, `seal.md` SEAL-GEN-014), and fails when one is found.
3. **Positive controls**: the same script run on a `taskdeck-devicectl` binary must find `taskdeck-test-trust-enabled-6f1c2a`, and run on the test binary of `taskdeck-seal` built with `test-entropy` (`cargo test -p taskdeck-seal --features test-entropy --no-run`) must find `taskdeck-test-entropy-enabled-3b9e5d`, so that a broken search cannot pass.

- **DEV-TRUST-001** (device) Every item of §11.1 MUST be compiled only with the feature `test-trust`, and the default feature set of `taskdeck-device` MUST NOT include it.
- **DEV-TRUST-002** (device) A build with `test-trust` MUST contain the byte string `taskdeck-test-trust-enabled-6f1c2a`, and a build without it MUST NOT.
- **DEV-TRUST-003** (release) The release workflow MUST fail when the feature resolution of any release build includes `test-trust`, `test-entropy` or `sim`, and when any release binary contains the marker of §11.1 item 7 or `taskdeck_seal::TEST_ENTROPY_MARKER` (§11.2 item 2), with both positive controls of §11.2 item 3 passing in the same run.
- **DEV-TRUST-004** (device) Without `test-trust`, the device MUST refuse an `http` URL for every host, loopback included.

## 12. Key stores

### 12.1 What a key store holds

A key store holds exactly one secret per identity: the identity's storage key `L_d`, 32 bytes drawn from `OsEntropy` when the identity is created (`seal.md` §6.6). Everything else secret (the device keys, epoch roots, the relay token) lives in sealed files under keys derived from `L_d` (`identity.tdk`, `device.tdk`). The **associated data** of an identity is `AD_L = ctx("local-key") ‖ board_id ‖ dev` (22 + 16 + 16 = 54 bytes: `ctx("local-key")` is the 21 characters of `taskdeck/v1/local-key` and one `0x00`), passed wherever the store accepts associated data or entropy.

```rust
pub trait KeyStore: Send + Sync {
    fn kind(&self) -> u8;                                                   // the store.tds byte (§6.2)
    fn create(&self, board: &[u8; 16], dev: &[u8; 16], key: &zeroize::Zeroizing<[u8; 32]>) -> Result<(), KeyStoreError>;
    fn load(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<zeroize::Zeroizing<[u8; 32]>, KeyStoreError>;
    fn delete(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<(), KeyStoreError>;
}
pub enum KeyStoreError { NotFound, CannotUnwrap, Unavailable, Failed }
```

| Error | Meaning | `open` does |
|---|---|---|
| `NotFound` | no entry for this identity | `KeyLost` |
| `CannotUnwrap` | an entry exists but does not open (another user, another machine, a reset Keystore key) | `KeyLost` |
| `Unavailable` | the store cannot answer now (a locked keychain, a bridge not ready) | fails with `device/keystore_unavailable`; nothing changes |
| `Failed` | any other error | fails with `device/keystore_failed`; nothing changes |

**Threads.** `Device::open` calls `load` on its calling thread, before it starts the sync thread. Every other key-store call runs on the sync thread: `create` (§18.1 step 7, §18.3), `delete` (`forget_identity`, `abandon_create`), and any `load` the running facade makes when it activates another identity (§17.2). A key store therefore works from any thread; `AndroidKeyStore` attaches each calling thread to the JVM itself (`bridge.md` §11.5, BRIDGE-KEY-005).

- **DEV-KEY-001** (device) A key store MUST hold only `L_d` for each identity, and the device MUST NOT store `L_d` anywhere but in the key store and in memory.
- **DEV-KEY-002** (device) `create` MUST make the key-store entry durable before the identity directory's first sealed file is written, and `open` MUST map each `load` error as the table of §12.1 states.

### 12.2 Which store each platform uses

| Platform | Store | `kind` |
|---|---|---|
| Windows | `DpapiKeyStore` (§12.3) | 1 |
| macOS | `KeychainKeyStore` (§12.4) | 2 |
| Android | `AndroidKeyStore` through the bridge (§12.5) | 3 |
| Linux, release builds | none: `begin_create` and `join` answer `device/no_keystore`; local-only mode works as today | |
| tests (`test-trust`) | `FileKeyStore`, `MemoryKeyStore` (§12.6) | 4, 5 |

### 12.3 Windows: DPAPI

`create`: `CryptProtectData` with `pDataIn` = `L_d`, no description, `pOptionalEntropy` = `AD_L`, no prompt structure, flags `CRYPTPROTECT_UI_FORBIDDEN` (user scope: `CRYPTPROTECT_LOCAL_MACHINE` is never set); the output blob is written to `%LOCALAPPDATA%\TaskDeck\keys\<board hex>-<dev hex>.ldk` by the replace procedure (§7.2), creating the directories. `%LOCALAPPDATA%` is read from the environment and must be a non-empty absolute path (else `Unavailable`); it is local and does not roam, unlike the `keyring` crate's entries (SYNC.md §5.6). `load`: read the file (missing: `NotFound`), `CryptUnprotectData` with the same entropy and flags; failure or an output of other than 32 bytes is `CannotUnwrap`. Output buffers are copied into a `Zeroizing` value, overwritten with zeros, and freed with `LocalFree`. `delete` removes the file.

- **DEV-KEY-003** (device) On Windows, `L_d` MUST be protected with `CryptProtectData` in user scope with `AD_L` as optional entropy and `CRYPTPROTECT_UI_FORBIDDEN`, and the blob MUST be stored under `%LOCALAPPDATA%\TaskDeck\keys\`, never in a roaming profile or in the data folder.

### 12.4 macOS: the login Keychain

`create`: a generic password item in the default (login) keychain through `security-framework`, service `TaskDeck storage key`, account `<board hex>.<dev hex>`, data `L_d`. The item is created by the signed app, so its access control trusts the app's signing identity ("TaskDeck Local", SYNC.md §8) and no other application (SYNC.md §5.6). `load`: `errSecItemNotFound` is `NotFound`; `errSecInteractionNotAllowed` and `errSecAuthFailed` are `Unavailable`; data of other than 32 bytes is `CannotUnwrap`. The Keychain takes no associated data, so `AD_L` is not used here. CI tests run against a temporary keychain made the default for the test process.

- **DEV-KEY-004** (device) On macOS, `L_d` MUST be stored as a generic password item of the login keychain with the service and account of §12.4, created by the app itself.

### 12.5 Android: the Keystore through the bridge

`AndroidKeyStore`, in `taskdeck-android`, implements the trait (`kind() = 3`) by calling the shell's key vault, which holds an AES-256-GCM key of the Android Keystore under the alias `taskdeck.ld.<board hex>.<dev hex>`, **not** bound to user authentication, so that jobs and reminders run while the phone is locked (SYNC.md §5.6). The vault wraps `L_d` with `AD_L` as associated data into 60 bytes, `iv[12] ‖ ciphertext[32] ‖ tag[16]`, which `AndroidKeyStore` keeps as `ld.wrap` in the identity directory. It writes and removes that file itself, outside the store and its file layer (DEV-THREAD-001): `create` first creates `sync/` and the identity directory when they are missing (`create_dir`, then `sync_dir` of its parent, for each), then writes `ld.wrap.tmp`, `sync`, `rename` to `ld.wrap`, `sync_dir`; `load` reads it; `delete` removes the alias, then the file, a missing file being no error. A missing alias or file is `NotFound`; a wrap that does not open (`AEADBadTagException`, `KeyPermanentlyInvalidatedException`) is `CannotUnwrap`; a Keystore not yet available is `Unavailable`. The key's parameters, the vault's calls, the status mapping and the threading are `bridge.md`'s (§11.5, §12.5; BRIDGE-KEY-001, BRIDGE-KEY-004, BRIDGE-KEY-005); the app's backup and transfer exclusions are the shell's (`bridge.md` §12.3).

- **DEV-KEY-005** (withdrawn) Replaced by BRIDGE-KEY-001 and BRIDGE-KEY-004 (`bridge.md` §11.5, §12.5), which state the Android Keystore wrap of `L_d` for the stages that build it.

### 12.6 Test stores and a key that cannot be unwrapped

With `test-trust`: `FileKeyStore` keeps `L_d` in plain 32-byte files `<dir>/<board hex>-<dev hex>.testkey` (for devicectl, whose processes restart); `MemoryKeyStore` keeps them in a process map.

**"The device is replaced."** When `L_d` cannot be loaded (`NotFound`, `CannotUnwrap`), nothing of the identity can be read: not its outbox, not its keys (SYNC.md §5.6). The facade opens in `KeyLost` mode, shows the old identity's board, `dev`, device name (from `store.tds` and the plaintext headers) and asks the user to pair this machine again as a new device and to revoke the old identity from another device (SYNC.md §5.4). The old identity directory and key-store entry are kept until the user calls `forget_identity` (§17.7); its unsent outbox is lost.

- **DEV-KEY-006** (device) In `KeyLost` mode the device MUST NOT delete the old identity directory or its key-store entry, except on `forget_identity`.
- **DEV-KEY-007** (device) The test key stores MUST exist only with the feature `test-trust`.

## 13. Fetchers

### 13.1 ICS feeds

Every device fetches the board's subscriptions itself (SYNC.md §3, R1 §7); the overlay is per device, never enters `apply`, and lives in `overlay.tdk` (synced) or `subscribed_cache.json` (local-only). The raw body of each feed is kept with its validators so that the window can slide on a `304` (V2-22): `feed-<id>.tdk` (synced) or `taskdeck_data/feeds/feed-<id>.bin` (local-only, the same plaintext layout as §6.4.10, unsealed, written by the replace procedure).

#### 13.1.1 Cadence

The calendars thread fetches every enabled subscription of the published board, one at a time in ascending id: at start, then every 600,000 ms, and at once when the set of enabled subscriptions or a URL changes (today's `REFRESH_EVERY` and wake-up, subscriptions.rs:715-791). On Android it fetches while the app is in the foreground, and in a background run only when the last fetch is older than 21,600,000 ms (six hours). The window is 60 days back and 400 forward of today in the device's zone (`WINDOW_BACK_DAYS`, `WINDOW_FORWARD_DAYS`).

#### 13.1.2 One fetch, in order

1. Parse the subscription's URL with the `url` crate; the scheme must be `https` (`device/insecure_url`); a URL with userinfo keeps it only for its own origin (an `Authorization: Basic` header to that origin, dropped on any redirect to another origin).
2. Check the host (§13.1.4) and resolve it; check every address; pin the first allowed one.
3. Send `GET` with `User-Agent: TaskDeck-Calendar/1`, `Accept: text/calendar, text/plain;q=0.9, */*;q=0.5`, `Accept-Encoding: identity`, and `If-None-Match: <etag>` and `If-Modified-Since: <last_modified>` when the cached body is for the same URL hash; no `Referer`, no cookies.
4. On `301`, `302`, `303`, `307` or `308` with a `Location`: resolve it against the current URL; it must be `https` and pass steps 1 and 2 again (a new resolution, a new pin); the next request carries only the headers of step 3 for the new URL, with no query or credential of the previous URL; at most 3 redirects (a 4th is `device/ics_redirects`). Any other `3xx` is `device/ics_status`.
5. `304`: reuse the cached body. `200`: a `Content-Length` above 8,388,608 is `device/ics_too_large`; read at most 8,388,609 bytes; more is `device/ics_too_large`. Any other status is `device/ics_status` with the number.
6. The whole fetch, redirects included, must finish within 20,000 ms (`device/ics_timeout`).
7. Decode with `String::from_utf8_lossy` and run `taskdeck_core::ics::parse(text, from, until, Limits::default(), zone)` under `catch_unwind`.
8. On success, build the subscription's events and `FetchOutcome::Ok` as today (subscriptions.rs:593-639); store the body and validators (only after a `200`); on any failure keep the previous events and record `FetchOutcome::Failed { why }`. In both cases `FetchStatus.at` is the device clock (`wall_ms`) as a `chrono::DateTime<chrono::FixedOffset>` whose offset is the device zone's `offset_at` of that instant (`core.md` §14.6), where today's code takes `Local::now()`. A successful fetch (`200`, or `304` with the cached body) sets the subscription's **last good fetch** to the device clock (`wall_ms`); it is kept in memory and, at `open`, starts from the `fetched_ms` of the subscription's feed body file (the time of its last `200`), absent when there is none. A failed fetch leaves it as it was. The sync thread publishes it with the overlay (`Published::overlay_fetched`, §25).

`why` is one fixed text per code, with no URL, host, header or library message in it:

| Code | `why` |
|---|---|
| `device/insecure_url` | `the address is not https` |
| `device/ics_address` | `that calendar's address is not allowed` |
| `device/ics_dns` | `that calendar's host could not be found` |
| `device/ics_connect` | `that calendar could not be reached` |
| `device/ics_tls` | `that calendar's certificate was not accepted` |
| `device/ics_timeout` | `that calendar took too long to answer` |
| `device/ics_redirects` | `that calendar redirected too many times` |
| `device/ics_status` | `the calendar answered <status>` |
| `device/ics_too_large` | `that calendar is too large to read` |
| `device/ics_panic` | `this calendar could not be read and was set aside` |

The fetcher never dereferences a URL found inside a feed (`ATTACH`, `URL`, `IMAGE` and every other property are text to the parser, which performs no I/O).

#### 13.1.3 After a fetch

The sync thread adopts the new overlay as today (`adopt_overlay`: events digest unchanged keeps the events and copies status and coverage), writes `overlay.tdk` (or `subscribed_cache.json`) when the digest changed, and raises the overlay counter. A feed body file is removed when its subscription is gone from the board or its URL hash changed (§7.5).

**Removed and disabled subscriptions (synced mode).** `apply` runs inside the engine, which does not pass the change set on (`engine.md` §5.1), so after each `ViewChanged` the sync thread drops from the overlay the events and statuses of every subscription id that is absent from the new pending view or has `enabled: false` there, which is the set `overlay_forget` names in local-only mode (`core.md` §8.3, §9.3 item 7), writes `overlay.tdk` when the overlay changed, and then raises the overlay counter. A subscription that comes back (a refused removal, or enabled again) is fetched again by the usual schedule.

**The feed's own name, adopted once, by the device that added it** (V1-15, SYNC.md §7). `device.tdk` holds `added_here`, the subscription ids this device's `add_subscription` intents created (from their `Enqueued` reply), and `named`, the ids whose name it has already adopted. After a successful fetch of a subscription in `added_here` and not in `named`: if the subscription `is_unnamed` and `better_name` gives a title, the device submits `rename_subscription { id, name }` with the title trimmed, control characters (`U+0000`–`U+001F`, `U+007F`–`U+009F`) removed and cut to 200 scalars, and adds the id to `named` whatever the outcome. No other device and no later fetch renames.

**Quarantine.** A panic caught in step 7 quarantines the subscription: `device.tdk` records `(id, url_hash)` in `quarantined`; the calendars thread skips it while the record stands; its status is `device/ics_panic` and its previous events stay. The record is cleared when the subscription's URL changes, when it is disabled and enabled again, or when it is removed (SYNC.md §4.5: "a feed that panics is quarantined").

**Feeds down** (SYNC.md §6.4: "last data kept, labelled with its age"). When an ICS host cannot be reached or answers with a failure, the device keeps the subscription's previous events in the overlay and its last good fetch unchanged, and publishes, per subscription, the time of that last good fetch; when Open-Meteo cannot be reached, the device keeps the last report in `weather.tdk` (or memory, local-only) and in `Published::weather()` with its `fetched_ms`, and `status().weather_age_ms` is `wall_ms − fetched_ms`. The interfaces label kept data with that age; the words are those of stages `desktop` and `android` (§1).

#### 13.1.4 The address guard

**The host.** A host that is an IP literal is refused (`device/ics_address`): a bracketed IPv6 literal; a host whose last dot-separated label consists only of decimal digits, or of `0x` or `0X` followed by hexadecimal digits (so `127.1`, `2130706433`, `0x7f.1` are literals); and the names `localhost` and every name ending in `.localhost`.

**The addresses.** The host is resolved with the operating system's resolver; when any resolved address is refused, the whole fetch is refused (so a name cannot mix an allowed address with a private one). Each IPv6 address is first normalised: an IPv4-mapped address (`::ffff:0:0/96`) or an IPv4-compatible address (`::/96`, except `::` and `::1`) is replaced by its embedded IPv4 address, which is then checked as IPv4. Then these are refused:

| Family | Refused ranges |
|---|---|
| IPv4 | `0.0.0.0/8`, `10.0.0.0/8`, `100.64.0.0/10` (CGNAT), `127.0.0.0/8`, `169.254.0.0/16`, `172.16.0.0/12`, `192.0.0.0/24`, `192.0.2.0/24`, `192.88.99.0/24`, `192.168.0.0/16`, `198.18.0.0/15`, `198.51.100.0/24`, `203.0.113.0/24`, `224.0.0.0/4` (multicast), `240.0.0.0/4` (reserved, with `255.255.255.255` broadcast) |
| IPv6 | `::/128`, `::1/128`, `64:ff9b::/96` and `64:ff9b:1::/48` (NAT64), `100::/64`, `2001::/23` (Teredo and the other protocol assignments), `2001:db8::/32`, `2002::/16` (6to4), `fc00::/7` (ULA), `fe80::/10`, `fec0::/10`, `ff00::/8` (multicast) |

The connection goes to the first allowed address in resolver order, and TLS uses the URL's host for SNI and certificate checks; the address is not resolved again for that request (pinning against rebinding). With `test-trust`, the loopback names of §11.1 item 1 are allowed with loopback addresses.

- **DEV-ICS-001** (device) An ICS fetch MUST follow the eight steps of §13.1.2 in order, with at most 3 redirects, each followed by hand and checked again by steps 1 and 2.
- **DEV-ICS-002** (device) An ICS request MUST NOT carry a `Referer` header, a cookie, or any query, path or credential of a previous URL.
- **DEV-ICS-003** (device) The fetcher MUST refuse a host that is an IP literal by §13.1.4, MUST refuse the fetch when any resolved address, after normalising mapped and compatible forms, lies in a range of §13.1.4, and MUST connect only to the address it checked.
- **DEV-ICS-004** (device) An ICS fetch MUST end within 20,000 ms and MUST refuse a body above 8,388,608 bytes, reading at most 8,388,609 bytes.
- **DEV-ICS-005** (device) After a `304`, the fetcher MUST re-expand the cached body for the current window (V2-22), and MUST send `If-None-Match` and `If-Modified-Since` only with a cached body of the same URL hash.
- **DEV-ICS-006** (device) Every `FetchOutcome::Failed` text MUST be the fixed text of §13.1.2 for its code, and MUST NOT contain the URL or any part of it.
- **DEV-ICS-007** (device) The fetcher MUST NOT request any URL that appears inside a feed body.
- **DEV-ICS-008** (device) A device MUST submit a rename from a feed's own title only for a subscription it added itself, at most once per subscription, and with the title cleaned as §13.1.3 states.
- **DEV-ICS-009** (device) A subscription whose parse panicked MUST NOT be fetched again until one of the three clearing events of §13.1.3, and its previous events MUST stay in the overlay.
- **DEV-ICS-010** (device) In synced mode, after each `ViewChanged`, the device MUST remove from its overlay the events and statuses of every subscription id that is absent from the new pending view or has `enabled: false` there, and MUST write `overlay.tdk` and raise the overlay counter when that changed the overlay.
- **DEV-ICS-011** (device) After a failed ICS fetch, the device MUST keep the subscription's previous events in the overlay and MUST publish, for every subscription with a last good fetch, the device-clock time of that fetch as `Published::overlay_fetched` states, updating it only after a `200` or a `304`.

### 13.2 Weather

The weather thread asks Open-Meteo about one place (SYNC.md §3, R1 §8):

- **Which place.** On every device the board's home place (`BoardData.home`, hundredths of a degree). On Android the user may choose the phone's own place instead (coarse location, on demand): it is kept in `device.tdk` as `own_place` in hundredths of a degree, as the shell rounded it half away from zero and passed it to `set_weather_place` (`bridge.md` §6.4.5 step 3, BRIDGE-ROUTE-004), and never enters the board, a command, a log entry or a backup (SYNC.md §3: "the phone's own place never syncs").
- **The request.** Today's URL (weather.rs:291-299, digest §5) with `latitude` and `longitude` written as `lat / 100` and `lon / 100` with exactly two decimals (`format!("{:.2}")` of the integer divided in decimal, e.g. `5237` gives `52.37`, `-12` gives `-0.12`), `forecast_days=7`; `User-Agent: TaskDeck-Weather/1`; host `api.open-meteo.com` through the address guard; no redirect followed (a `3xx` is a failure); 10,000 ms; body at most 1,048,576 bytes.
- **Cadence.** At start, every 600,000 ms, and at once when the place changes; up to 3 attempts per round, waiting 1, 2 and 4 seconds after failures (today's `get_weather`, weather.rs:432-514). On Android only in the foreground, and in a background run when the last report is older than 3,600,000 ms.
- **Result.** `weather::build_report` (`core.md` §14.6), written to `weather.tdk` (synced) or kept in memory (local-only, as today), in both cases with the place's source (§6.4.12), and published; a failed round keeps the last report, which the interfaces label with its age (SYNC.md §6.4).

- **DEV-WX-001** (device) The weather request MUST use the place's coordinates with exactly two decimals and MUST NOT send any other location, identifier or board data.
- **DEV-WX-002** (device) The phone's own place MUST be kept only in `device.tdk` (or `device.json` in local-only mode, §16) and memory, and MUST NOT appear in any intent, command, frame, blob or backup.
- **DEV-WX-003** (device) After a failed weather round, the device MUST keep the last report with its `fetched_ms` in `weather.tdk` (in memory in local-only mode) and in `Published::weather()`, and MUST publish `status().weather_age_ms` as the device clock minus that `fetched_ms`.

## 14. Blobs and the shared picture

Snapshot blobs (purposes 1 to 3) are the engine's: it emits `Blob` effects, the transport performs them (§10.5), and `taskdeck-seal` seals and opens them (`seal.md` §20, `engine.md` §19). This section is the shared picture (purpose 4), which the board names by `PictureRef` (`core.md` §6.9) and the device handles end to end. Only desktops upload it (SYNC.md §3); every device downloads it.

### 14.1 Upload (desktop)

`set_shared_picture(bytes)` (§17.12), on the picture thread and then the sync thread, in order:

1. `bytes` is 1 to 6,291,456 bytes (today's `MAX_UPLOAD_BYTES`), else `device/picture_too_large`.
2. The format by its first bytes: JPEG (`FF D8 FF`) or PNG (`89 50 4E 47 0D 0A 1A 0A`); anything else is `device/picture_format`. The user's own file may be PNG; nothing else is decoded.
3. Decode with `image::ImageReader::with_format` of that format (never `with_guessed_format`) and `image::Limits { max_image_width: 20,000, max_image_height: 20,000, max_alloc: 160,000,000 }` (today's limits, phone.rs:1530-1533); a failure is `device/picture_decode`.
4. Centre-crop to the aspect 720 : 1560 and resize to exactly 720 × 1560 with Lanczos3 (today's crop, phone.rs:1542-1550).
5. Encode as baseline JPEG at quality 85 with the `image` crate's encoder (no EXIF or other metadata): the **clean** picture.
6. Seal it as a blob of kind 2 under `K_blob,e` of the current epoch with this device as writer (`taskdeck_seal::seal_blob`); upload it with purpose 4 and seq 0 (`relay.md` §10.7), resuming as DEV-NET-010 states.
7. Submit the intent `set_picture { picture: { blob, epoch, parts, sha256, size } }`; when it is enqueued, write `picture.tdk` with the clean picture and its look (§14.2 step 7).

After a purge (`Notify(ReuploadPicture { epoch })`), a desktop whose `picture.tdk` matches the board's current `picture` repeats steps 6 and 7 with its clean picture under the new epoch; a device without it does nothing, and the board keeps a picture that devices joining later cannot open (they show none).

### 14.2 Download and processing (every device)

When the published board's `picture` differs from `picture.tdk`'s reference, the device, in order:

1. `GET …/blobs/<blob>`: the blob is complete, and its `size` and `parts` equal the reference's (`device/picture_blob` otherwise).
2. Fetch every part; check, before any decryption, that their concatenation has the reference's `size` and `sha256` (SEAL-BLOB-002).
3. `taskdeck_seal::open_blob` with a `BlobRef` built from the reference (`id` its `blob`, its `sha256`, `size`, `parts` and `epoch`, and `inflated: 0`, which `open_blob` does not read, `seal.md` §20.5), the blob-meta's `author` as the writer, and `K_blob,e` of the reference's epoch. The writer is bound by the AEAD's AD (`seal.md` §20.2, which admits the relay's author for a kind-2 blob), so a relay that names another author only makes the open fail. A missing root, a failed open or a failed hash is `device/picture_blob`.
4. The opened bytes are 4 to 4,194,304 bytes and start with `FF D8 FF`; else `device/picture_format`.
5. Decode with `ImageReader::with_format(ImageFormat::Jpeg)` and `image::Limits { max_image_width: 4,096, max_image_height: 4,096, max_alloc: 67,108,864 }`, under `catch_unwind` (V1-14, V3-12); a failure or panic is `device/picture_decode`.
6. Centre-crop and resize to 720 × 1560 as §14.1 step 4, and encode at quality 90: the **clean** picture of this device.
7. Make the **look** from the clean picture with this device's dials (`device.tdk` `look`, default blur 11 and light 39, today's `BACKGROUND_BLUR_DEFAULT` and `BACKGROUND_LIGHT_DEFAULT`): today's blur and tone map (phone.rs:1559-1626) and baseline JPEG at quality 55 (`BACKGROUND_QUALITY`), and `top`, the average colour of the top eighth (phone.rs:1630-1638). No AVIF is made (SYNC.md §2.1).
8. Write `picture.tdk` and publish. The WebView and every user interface see only the `look` bytes, never the opened blob (`bridge.md` serves them as today's `/bg-<id>.jpg`, with `id` the lowercase hex of the first 8 bytes of `SHA-256(look)`).

A failure keeps the previous picture, if any, sets the picture status to the code, and retries after 3,600,000 ms or when the reference changes. `set_look_dials(blur, light)` (§17.12) stores the dials in `device.tdk`'s `look`, re-makes step 7 from the stored clean picture (or §14.3's look while the phone shows its own picture), rewrites that picture's file, publishes, and then returns the id (step 8) of the look now shown, or none when no picture is shown.

- **DEV-PIC-001** (device) A device MUST decode a synced picture only as JPEG, with `ImageFormat::Jpeg` and the limits of §14.2 step 5, and only after the checks of steps 1 to 4 have passed.
- **DEV-PIC-002** (device) A device MUST re-encode every synced picture as §14.2 steps 6 and 7 state before any user interface or WebView receives it, and MUST NOT pass the opened blob bytes to either.
- **DEV-PIC-003** (device) A desktop MUST decode an uploaded picture only as JPEG or PNG, chosen by its first bytes, under the limits of §14.1 step 3, and MUST upload only the re-encoded JPEG of §14.1 step 5.
- **DEV-PIC-004** (device) A device MUST check the parts' size and SHA-256 against the board's `PictureRef` before it decrypts them.

### 14.3 The phone's own picture (Android)

The phone may show a picture of its own instead of the board's shared picture (SYNC.md §3: the phone's "own Look", ruling S29). `set_own_picture(bytes)` (§17.12), on the picture thread and then the sync thread, in order:

1. The mode is synced (`device/needs_sync`); `bytes` is 1 to 6,291,456 bytes (`device/picture_too_large`) and starts with `FF D8 FF` (`device/picture_format`).
2. Decode with `ImageReader::with_format(ImageFormat::Jpeg)` and the limits of §14.1 step 3, under `catch_unwind`; a failure or panic is `device/picture_decode`.
3. Centre-crop, resize and encode as §14.2 step 6: the **clean** own picture.
4. Make its **look** with this device's dials as §14.2 step 7.
5. Write `own-picture.tdk` (§6.4.14) and publish.

`clear_own_picture()` removes `own-picture.tdk`; when the `blur` and `light` of `picture.tdk` differ from `device.tdk`'s `look`, it re-makes that picture's look as §14.2 step 7 and rewrites `picture.tdk`; then it publishes. `set_own_picture` and `clear_own_picture` each return, after the publish, the id (§14.2 step 8) of the look now shown: the own picture's after `set_own_picture`, the shared picture's or none after `clear_own_picture`. The publish raises `Changes.picture` (DEV-NOTIFY-001). While `own-picture.tdk` exists, `Published::picture()` returns its look; otherwise the shared picture's (§14.2). `set_look_dials(blur, light)` re-makes the look of the picture shown from its stored clean copy and rewrites its file. The own picture is never sealed as a blob, uploaded, or carried in an intent, a frame, a backup or the board.

- **DEV-PIC-005** (device) `set_own_picture` MUST accept only a JPEG (first bytes `FF D8 FF`) of 1 to 6,291,456 bytes, MUST decode it only with `ImageFormat::Jpeg` under the limits of §14.1 step 3, MUST keep only its re-encoded clean copy and look, in `own-picture.tdk`, and MUST NOT upload it or put it into any intent, frame, blob or backup.

## 15. Backups and the recovery tool's online steps

The backup file's layout, sealing and reading are `seal.md` §21; the writer and reader functions and the offline recovery tool are stage `recovery`'s (SYNC_PLAN.md §6.8). This section is what `device` adds (SYNC_PLAN.md §6.7 item 11, after `recovery` merges): who writes, when, where, under which names, what is kept, how each file is checked, what the user sees, and the recovery tool's online steps.

### 15.1 Who writes

A device writes backups only while all of these hold (ruling S8: "backups are written by whichever desktop is awake"):

1. it is a desktop (Windows or macOS) in synced mode;
2. it has joined and is `current` in its membership, with no halt and no stop of `engine.md` §21, §22 or §24.4 in force;
3. its `history.tdk` is complete from seq 1 (`device.tdk` `history_complete`, §15.7);
4. the recovery fingerprint pinned on this device (`device.tdk` `pinned_recovery_fp`) equals the membership's current recovery key fingerprint (SEAL-BKP-006);
5. at least one destination is configured;
6. it holds a backup-read token for the current epoch, or can mint one now (§15.4).

When one fails, the backup status names the first failing condition (`device/backup_not_desktop`, `device/backup_stopped`, `device/backup_history_incomplete`, `device/backup_unpinned`, `device/backup_no_destination`, `device/backup_no_token`). Being offline does not stop a nightly file: it holds the confirmed state, which needs no relay, and the last read token.

A week in which no desktop is awake writes no backup and blocks nothing else (ruling S8, `engine.md` §29.4). When a desktop wakes, its nightly file is due at once (§15.2) and its next segment covers every frame since its previous one, the week's included, from its own log; the point-in-time history is therefore complete, and only the nightly files of the days slept through are missing.

### 15.2 When

The backup thread evaluates the schedule 300,000 ms after `open`, then every 600,000 ms, and at `backup_now`:

- **Nightly** is due when `last_nightly_day` (a local date in `device.tdk`) is before today's local date and either the local time is 03:00 or later, or `last_nightly_day` is before yesterday ("at the first chance after being off", SYNC.md §6.6). It holds the confirmed state at `B` (`Engine::confirmed`, `Engine::membership_bytes`).
- **Segment** is due when the UTC month (`YYYY-MM` of the device clock) differs from `last_segment.month` and a start base `x` below the newest base's seq exists. The start base is the base at `last_segment.seq` (for a writer's first segment, its oldest base); when that base is missing (a purge removed it) or its membership does not list this device as current (a paired device's oldest base is at its bootstrap seq `h`, below the `added_seq` that adds it; or the device was revoked there), `x` is the oldest later base whose membership lists this device as current, and the status records the skipped seqs as a gap. A segment covers the seqs from `x + 1` to the newest base's seq; its `membership` is the base's at `x` and its `chain_before` that base's `board_chain`, so both come from a base file and need no replay, and the writer is a current device of that membership, as `seal.md` §21.6 step 10 requires of a segment.

A file written successfully to at least one destination sets `last_nightly_day` or `last_segment` (replace procedure on `device.tdk`).

### 15.3 What is written, and under which names

Each file is `taskdeck_seal::write_backup` (`seal.md` §21) with:

| Input | Nightly | Segment |
|---|---|---|
| `seq` | `B` | the last seq it holds |
| `history` | the records of `history.tdk` with seq at most `B` | the records with seq at most `first_seq − 1` |
| `content` | `Nightly { membership: M_B, board, head: (B, C_B), hash_version: 1, state_hash }`, `state_hash` from `taskdeck_sync::state_hash` | `Segment { membership, first_seq, chain_before, frames }`, frames read from the log |
| `roots` | every root `identity.tdk` holds; `write_backup` writes those whose commitment the file's membership records and leaves out the others (`seal.md` §21.5) | the same |
| `read_token` | the token of §15.4 | the same |
| `meta` | §15.3.1 | §15.3.1 |
| `pinned_recovery` | the pinned recovery public key | the same |

A segment whose frames would make the file larger than 209,715,200 bytes (200 MiB) is split into consecutive segments at that size.

**File names** (`<tag>` is the writer tag, §2):

- nightly: `TaskDeck-<YYYYMMDD>-<seq>-<tag>.tdbk`, the date being today's local date and `<seq>` decimal;
- segment: `TaskDeck-<YYYYMM>-seg-<first seq>-<last seq>-<tag>.tdbk`, the month being the UTC month of writing.

SYNC.md §6.6 names nightly files `TaskDeck-<date>-<seq>.tdbk`; the writer tag is added so that two desktops writing to one synchronised folder never collide.

#### 15.3.1 The `meta` JSON

Canonical encoding (`core.md` §4), at most 1,048,576 bytes; `seal.md` §21.5 calls it the sync metadata and leaves it to this spec. The reader treats it as informational: nothing in it is trusted for verification (`seal.md` §21.6 verifies from the history and signatures alone).

| Field | Type | Presence | Constraints | Meaning |
|---|---|---|---|---|
| `app` | string | required | 1 to 64 bytes | the writer's `build_id` |
| `device_no` | `int[1..8191]` | required | | the writer's `device_no` |
| `home_gen` | `u53` | required | | the membership's home generation |
| `kind` | string | required | `"nightly"` or `"segment"` | |
| `log_floor` | `u53` | required | | the writer's `log_floor` |
| `relay_door` | `hex5` | required | | the board's door at writing |
| `relay_host` | string | required | a `seal.md` §5.4 host, 1 to 255 bytes, `:port` included | the writer's relay target |
| `version` | `int[1..1]` | required | | 1 |
| `writer_name` | string | required | `seal.md` §5.5 | the writer's device name |
| `written_ms` | `u53` | required | | the writer's clock |
| `zone_offset_s` | `int[-86399..86399]` | required | | the writer's UTC offset at writing; display only |

#### 15.3.2 Writing and checking one file

In each destination, on the backup thread: write the bytes to `.<name>.tmp`, `sync`, rename to `<name>` (write-through on Windows), `sync_dir`; then read the whole file back, check that its SHA-256 equals that of the bytes written, and check its header and writer signature with `taskdeck_seal::verify_backup_file` (`seal.md` §26, SEAL-BKP-009: §21.6 steps 1, 2 and 5) against this device's signing key. A failure at any step removes the temporary or final file and records the destination's failure. The writer cannot read its own file back (it is sealed to the recovery key), so this is the check SYNC.md §6.6 asks for.

- **DEV-BKP-001** (device) A device MUST write a backup only while every condition of §15.1 holds, and MUST report the first failing condition otherwise.
- **DEV-BKP-002** (device) A device MUST write a nightly backup and a segment exactly when §15.2 makes them due, with the inputs of §15.3 and the names of §15.3.
- **DEV-BKP-003** (device) After writing a backup file, the device MUST read it back from the destination, compare its SHA-256 with the bytes written and verify its header and signature, and MUST count the write as good only when both pass.
- **DEV-BKP-004** (device, recovery) The `meta` of a backup MUST be the canonical JSON record of §15.3.1, and a reader MUST NOT use any of its fields for verification.
- **DEV-BKP-005** (device) A segment's `membership` and `chain_before` MUST be taken from the base at its first seq − 1, whose membership MUST list the writer as a current device, and its frames MUST be the consecutive log frames from its first to its last seq.

### 15.4 The backup-read token

Each writer mints its own backup-read token (`relay.md` §12.11, V1-2): before a backup, when `device.tdk` has no `read_token` or its epoch differs from the membership's current epoch, the device sends `mint_read_token { dev: own, epoch: current }` (signer this device, credential its token) and stores the answer. The relay revokes every backup-read token at `revoke` (an orphan `revoke` included, lead ruling L5), `reinstate`, `recovery_register` and `purge`, and those of lower epochs when a writer mints for a newer epoch (`relay.md` §5.2); a rotation that none of these accompanies leaves them valid there. A writer therefore mints whenever its stored token's epoch differs from the membership's. Offline, a nightly file carries the last token minted; a writer that never minted one waits (`device/backup_no_token`).

- **DEV-BKP-006** (device) A writer MUST mint a backup-read token for the current epoch before the first backup of each epoch it can reach the relay in, and MUST NOT put a role-1 token in a backup.

### 15.5 The pinned recovery fingerprint

The creating desktop pins the recovery key it created (§18.1). Another desktop pins it when the user types or compares the fingerprint printed on the recovery sheet (`pin_recovery_fingerprint(typed)`, §17.11): the typed text, normalised as `seal.md` §5.2, must equal the membership's current recovery key fingerprint (`device/fingerprint_mismatch` otherwise). On `Notify(RecoveryKeyChanged)` after a recovery-key change, `pin_needed` is set and backups stop with `device/backup_unpinned` until the user pins the new fingerprint on this device's own screen (SYNC.md §5.5, SEAL-BKP-006).

- **DEV-BKP-007** (device, desktop) A desktop MUST pin a recovery fingerprint only from the user's own confirmation on its screen, compared with the membership's current recovery key, and MUST stop writing backups from a recovery-key change until the new fingerprint is pinned.

### 15.6 Destinations, retention and status

A destination is `{ path, kind, label }`: `kind` is `folder` (the OneDrive or iCloud Drive folder, a second disk) or `offline` (a USB stick, present only now and then). At most 4. `path` must be absolute, must not lie inside the data folder, and must be a writable directory when it is set (`device/destination_invalid`); a cloud-synced folder is allowed here, since the files are sealed.

| Kind | Receives | Kept |
|---|---|---|
| `folder` | every nightly file and every segment | of this writer's files (names with its tag): the newest 30 nightly files by date then seq; the earliest nightly file of each of the 12 most recent local months that have one ("monthly"); the newest 13 segments by last seq. Every other file of this writer is deleted after a good write |
| `offline` | when its path exists: the earliest nightly file of each month and every segment, copied from the newest good copy or written anew | nothing is deleted |

The device never deletes, renames or reads a file in a destination whose name does not match this writer's patterns.

After a purge, `Notify(PurgeBackups { before_ms })` is shown to the user; `delete_backups_before(before_ms)` (§17.11) then deletes, in every destination present, this writer's files whose `written_ms` (header bytes 95 to 103, `seal.md` §21.2) is below `before_ms`. Nothing is deleted without that call (SYNC.md §4.7).

**Status** (§17.11): per destination `last_good_ms`, `last_attempt_ms`, `last_error` (a code), the number of this writer's files and their bytes; overall the reason of §15.1 when backups are off, `last_nightly_day`, `last_segment`. **Alarms**, shown on this desktop only and kept until resolved: `backup_failed` when the latest attempt at a destination failed (cleared by the next good write there); `backup_stale` when §15.1 holds and no destination has a good nightly file from the last 172,800,000 ms.

- **DEV-BKP-008** (device) A device MUST keep in each `folder` destination exactly the files of its own that the table of §15.6 keeps, MUST delete none in an `offline` destination, and MUST NOT touch any file whose name does not match its own patterns.
- **DEV-BKP-009** (device) A device MUST delete backups after a purge only on `delete_backups_before`, and only its own files older than the given instant by their header's `written_ms`.
- **DEV-BKP-010** (device) A device MUST raise `backup_failed` when an attempt at a destination fails and `backup_stale` when no good nightly file is 172,800,000 ms old or younger while §15.1 holds.

### 15.7 History for a device that joined later

Every device holds every control entry from seq 1 in `history.tdk` (`history_complete = true`). A device that created the board, or restored it, has them from the start. A device that bootstraps from a snapshot (a pairing's, or a rejoin's) takes the records up to the snapshot's seq from the snapshot's `history` (`seal.md` §20.4, ruling S27), which `seal.md` §20.5 has verified against the snapshot's membership, and records every later accepted control entry itself (§8.3). The sealer adapter fills a snapshot's `history` from `history.tdk` (the records with seq at most the snapshot's) whenever it seals one. When it opens one for a bootstrap, it hands the records to the store, which writes those it does not already hold to `history.tdk` before it persists the bootstrap: for a pairing, before `bootstrap.tdk` (§18.3, DEV-PAIR-006); for a rejoin (`engine.md` §19.7), in the first `Persist` that carries a `WriteCheckpoint`, before that op (DEV-RUN-006).

### 15.8 The recovery tool's online steps

`taskdeck-recover` (stage `recovery`) runs offline until these steps, which use `taskdeck_device::recovery_online::RecoveryLink` (§25), so the tool's crate depends on `taskdeck-device` (§4.1), over the transport of §10, with the relay address from the sheet (never from the backup, SEAL-BKP-004), the door as credential where no token exists, and the recovery identity (`dev_R`, the recovery signing key) as signer:

**Topping up** a verified backup at seq `b` with chain `c_b`:

1. Try the backup's read token on `hello`, unless the backup carries none (an empty `read_token`, `seal.md` §21.5). When it is refused (`401`, `403`), or the backup carries none, mint a token for the recovery identity (ruling S21): `admin-nonce` with the door and `signer = dev_R`, then `mint_read_token { dev: dev_R, epoch: 1 }` signed by the recovery key with the door; on `409 relay/stale_epoch` it sends it once more, with a fresh nonce and `epoch` = the `read_epoch` that refusal carries (`relay.md` §4.2, §12.11). It never mints above the relay's `read_epoch`, so it revokes no device's backup-read token. A second refusal (a rotation between the two requests) ends the top-up with `device/recovery_token`, and the tool restores the backup as it is.
2. Read pages after `b` with that token to the relay head; the chain from `c_b` is recomputed for every entry (`seal.md` §15.1). When a page has `truncated` set (`relay.md` §10.5: entries after `b` were removed by compaction or purge), the chain cannot be recomputed from `b`: the top-up stops with `device/recovery_truncated`, naming the page's `floor`, and the tool restores the backup as it is, or the user picks a newer backup. Otherwise a break, or a head chain that differs, stops the top-up with `device/recovery_chain` and the tool restores the backup as it is.
3. Hand the frames to the recovery stage's replay (classification with the backup's roots and the roots the recovery key unwraps from later rotations, then `apply`); the tool writes the result as a local-only data folder (§9.3's writer), which the user opens in a desktop and turns into a new board with "Create board from this folder" (§18.2).

**Fencing the old board**, once the new board exists and its relay address is known:

4. Read the old board's log to its head (as step 2) and take `ctr = 1 +` the highest counter of a frame authored by `dev_R` it saw (1 when none), the current epoch and its root (unwrapped from the newest rotation's wrap to the recovery key), and `based_on` the head. After a purge that guess can be too low, because the purge deleted the recovery identity's older frames (`relay.md` §24 open question 13); step 5 corrects it.
5. Seal a `ctl` entry with one `Fence { host, door, home_gen: current + 1 }` naming the new board's relay (`seal.md` §12.7), authored by the recovery identity, and send it with `fence` (`relay.md` §12.6) with the door and a recovery admin signature. A `409 relay/gap` or `409 relay/conflict` answer carries the recovery identity's `last_ctr` and `last_seq` (`relay.md` §4.2, RELAY-ADMIN-005), which `RecoveryLink::fence` returns as `FenceAnswer::Counter`: the tool reads the log again to its head, re-seals the entry once with `ctr = last_ctr + 1` and `based_on` that head, and sends it with a fresh nonce. A second refusal ends the fencing with the relay's code, shown to the user.

Devices of the old board that follow the Fence find they are no members of the new board and are offered re-pair and resubmit (`engine.md` §28.7, V2-21).

**Reinstating a revoked device** (ruling S24; lead rulings L1, L20, L23), on the board the sheet names, once a top-up of a verified backup to the relay head `h` (steps 1 to 3, except that step 3 writes no folder) has given the membership `M_h`, its epoch `E = M_h.epoch`, and the roots of steps 2 and 3:

6. **Offer.** The tool offers to reinstate a device `dev` only when `M_h` records `dev` as revoked with `status_by` a device, not a recovery identity (`seal.md` §13.6, `DeviceReinstated`); its revoking entry, the entry at `dev`'s `status_seq`, is at or above `M_h.purge_seq` (lead ruling L20); `M_h` has fewer than 16 current devices; and `n = E − first_epoch + 1` is at most 256, where `first_epoch` is the `epoch` of the `EpochRotated` item of the revoking entry (`seal.md` §12.1 shape R). Otherwise it says why with `device/recovery_reinstate` and sends nothing: the device pairs again, or the owner restores to a new board (ruling S14).
7. **Missed roots.** For each epoch `e` from `first_epoch` to `E`, in order, the tool takes `R_e` from the backup's roots, or else unwraps it (`unwrap_root` with `Recipient::Recovery`) from the wrap to `dev_R` in the `EpochRotated{e}` item, read from the backup's verified history (`seal.md` §21.6) for a rotation at or below `b` and from the entries read after `b` otherwise. It checks `R_e` against `c_e` as `M_h` records it (`EpochRoot::commitment`, `seal.md` §6.5, §13.8), then wraps it to `dev` with `wrap_root(R_e, board, e, c_e, dev, hpke_pk)`, `hpke_pk` being `dev`'s key in `M_h` (SEAL-WRAP-001). A root it cannot get this way (a rotation wrapped only to an earlier recovery key) ends the reinstatement with `device/recovery_reinstate`, and the user may pick a newer backup. A wrap that does not open, or a root that differs from `c_e`, ends it with `seal/commitment`.
8. **`reinstate`.** `admin-nonce` with the door and `signer = dev_R`, then `reinstate` (`relay.md` §12.5) with `AdminOp::Reinstate { dev, first_epoch, wraps }` (`seal.md` §16.2), the `n` wraps of step 7 in epoch order, signed by the recovery key, with the door (`RecoveryLink::reinstate`). A refusal ends the reinstatement with the relay's code, shown to the user; the relay has changed nothing.
9. **The reinstating entry.** The tool draws `R_{E+1}` (SEAL-GEN-005) and computes `c_{E+1}` (`seal.md` §6.5). It then seals one `ctl` entry authored by the recovery identity under epoch `E` with `K_entry,E`: `based_on` the head `h`; `ctr = 1 +` the highest counter of an entry authored by `dev_R` in the backup's history and in the entries it read (1 when none); and the items `DeviceReinstated { dev }`, then `EpochRotated { epoch: E + 1, commit: c_{E+1}, wraps }`, with a wrap of `R_{E+1}` to every device current after the `DeviceReinstated` (`dev` included) and to the current recovery key (`seal.md` §12.1 shape R, §12.5). It sends the frame with `append` and the door (`relay.md` §9.3, RELAY-SEQ-009; `RecoveryLink::append`). On `gap`, it re-seals the entry once with `ctr = last_ctr + 1` and sends it again. `new` or `dup` gives the entry's seq `s` (step 10). `conflict`, a second `gap` or a refusal leads to step 11.
10. **Check.** The tool mints a read token again as step 1 states, because `reinstate` revoked every backup-read token (RELAY-REVOKE-004). It reads the entries after `h` up to `s` and classifies them in seq order from `M_h` with the replay of step 3. If it classifies the entry at `s` `Accept`, the reinstatement is done: the device receives the wraps at its next login and reads what it missed (`engine.md` §24.4). If it refuses that entry (an entry sequenced between `h` and `s`, such as another rotation, made it invalid, `seal.md` §13.8), step 11 follows.
11. **Undo.** The tool sends `revoke { dev }` (`relay.md` §12.4), signed by the recovery key, with the door (`RecoveryLink::revoke`), for a device that `reinstate` returned to current but whose reinstating entry was not sequenced or was refused by clients. This returns the device's row to revoked and clears its stored wraps (RELAY-REVOKE-005), so the relay agrees with the membership again. After the first undo, the tool reads to the relay head as step 10 does and starts again at step 6, with that head as `h`. After a second undo, or when `revoke` is refused, it ends with `device/recovery_reinstate`, giving the relay's code when `revoke` was refused. A device that already received the wraps is revoked again by its own classification (`engine.md` §24.4, lead ruling L23).

- **DEV-BKP-011** (device, recovery) The recovery tool's online steps MUST take the relay host, board id and door only from the sheet's relay address, and MUST sign every admin request with the recovery identity.
- **DEV-BKP-012** (device, recovery) A top-up MUST recompute the chain from the backup's head chain for every entry it reads, and MUST stop without changing the restored state when the chain breaks.
- **DEV-BKP-013** (device, recovery) A top-up MUST stop with `device/recovery_truncated`, without changing the restored state, when a page it reads has `truncated` set, and MUST NOT report that case as `device/recovery_chain`.
- **DEV-BKP-014** (recovery) The recovery tool MUST offer to reinstate a device only when every condition of §15.8 step 6 holds in the membership at the relay head it verified, and MUST send no request for that device otherwise.
- **DEV-BKP-015** (recovery) The recovery tool MUST send `reinstate` with the wraps of the roots of epochs `first_epoch` to `E`, in epoch order, each root checked against its commitment before it is wrapped to the target's `hpke_pk`. It MUST send it before it appends the reinstating entry, which is sealed under `E` and whose rotation to `E + 1` wraps to the target.
- **DEV-BKP-016** (recovery) When the reinstating entry is not sequenced, or the tool's classification refuses it, the recovery tool MUST send `revoke` for the target before it ends or starts again, and MUST NOT start again more than once.

## 16. Per-device settings

Settings that are not board data live on the device (R1 §9). In synced mode they are the record of `device.tdk` (kind 12), sealed; in local-only mode the fields that apply are kept in the plain file `taskdeck_data/device.json` (the same JSON, written by today's atomic procedure, created on the first write; it holds no secret). Desktop window settings (window size, monitor, scale, frame cap, the desk background, its tint) stay in `userconfig.toml`, `desktop`'s as today.

The time zone is not a setting: the desktop uses the operating system's zone at each call (`ChronoZone(chrono::Local)`, `core.md` §5.2), and the Android shell hands the device a `TableZone` built from the platform's rules (`set_zone`, §17.9) at start and on every zone change; the device keeps it in memory.

**`device.tdk` / `device.json`** (canonical encoding, at most 1,048,576 bytes):

| Field | Type | Presence | Constraints | Meaning |
|---|---|---|---|---|
| `added_here` | array of `id` | default `[]` | ascending, distinct; at most 20 | subscriptions this device added (§13.1.3) |
| `backup` | object | optional | below; desktops only | backup settings and state (§15) |
| `device_name` | string | required | `seal.md` §5.5 | this device's name, sent in pairing slot B |
| `floor_chain` | `hex32` | optional | synced mode only | `C_{log_floor − 1}` as the engine handed it with the floor (§7.3) |
| `history_complete` | boolean | default `false` | | §15.7 |
| `log_floor` | `u53` | default `1` | | the store's truncation floor (§7.3) |
| `look` | object `{"blur": int[0..100], "light": int[0..100]}` | default `{"blur":11,"light":39}` | | the picture dials (§14.2) |
| `minter` | object `{"block": u53, "device_no": int[1..8191], "item": u53}` | optional | | the minter floor (§8.3); written at a board's creation (§18.1 step 7) and at a join (§18.3) as `IdMinter::new(device_no)`, that is `{"block": 1, "device_no": <device_no>, "item": 1}` (`core.md` §7.2, CORE-ID-007) |
| `named` | array of `id` | default `[]` | ascending, distinct; at most 20 | subscriptions whose feed name was adopted |
| `own_place` | object `{"lat": int[-9000..9000], "lon": int[-18000..18000]}` | optional | Android only; present exactly while `weather_source` is `"own"` | the phone's own place (§13.2) |
| `quarantined` | array of `{"id": id, "url_hash": hex32}` | default `[]` | ascending by `id`; at most 20 | §13.1.3 |
| `reminders` | object `{"block": int[0..1440], "due": int[0..1440], "event": int[0..1440]}` | default `{"block":0,"due":0,"event":10}` | Android | reminder lead minutes (ruling S19) |
| `requests` | object `{"count": int[0..1000000], "day": u53}` | optional | | the request budget (§10.8) |
| `tokens` | array of `{"gen": int[0..4294967295], "host": string, "push": boolean, "token": string}` | default `[]` | ascending by `host`, distinct; at most 4; `host` a host of `seal.md` §5.4 (1 to 255 bytes, `:port` included); `token` ASCII, 1 to 512 bytes (`seal.md` §6.7, SEAL-KEY-015); `push` absent until the device has learned the relay's capability (§10.3) | relay tokens and capabilities per host (§10.3); sealed file only |
| `weather_source` | string | default `"home"` | `"home"` or `"own"` | §13.2 |

**`backup`**:

| Field | Type | Presence | Constraints | Meaning |
|---|---|---|---|---|
| `destinations` | array of `{"kind": "folder"/"offline", "label": string, "path": string}` | default `[]` | at most 4; `label` 1 to 64 bytes; `path` 1 to 4,096 bytes | §15.6 |
| `last_nightly_day` | `date` | optional | | §15.2 |
| `last_segment` | `{"month": string "YYYY-MM", "seq": u53}` | optional | | §15.2 |
| `pin_needed` | string | optional | a key fingerprint | §15.5 |
| `pinned_recovery_fp` | string | optional | a key fingerprint (`seal.md` §6.8) | §15.5 |
| `pinned_recovery_pk` | `{"hpke_pk": hex32, "sign_pk": hex32}` | optional | | the pinned public key |
| `read_token` | `{"epoch": int[1..4294967295], "token": string}` | optional | `token` ASCII, 1 to 512 bytes (`seal.md` §6.7) | §15.4 |
| `status` | array of `{"bytes": u53, "files": u53, "last_attempt_ms": u53, "last_error": string, "last_good_ms": u53, "path": string}` | default `[]` | one per destination, same order; `last_error` absent when none | §15.6 |

The plain `device.json` never holds `tokens`, `backup` or `minter`; a local-only device has none of them.

- **DEV-SET-001** (device) The device MUST keep its per-device settings only in the record of §16, sealed in `device.tdk` in synced mode, and MUST NOT write `tokens`, `backup` or `minter` to the plain `device.json`.
- **DEV-SET-002** (device) The device MUST take its time zone only from the operating system (desktops) or the `TableZone` the shell handed it (Android), and MUST NOT store a zone.

## 17. The facade

### 17.1 How the tables read

Every call of `Device` is listed once. The columns:

- **Screens**: the stages whose code calls it: **D** `desktop`, **A** `android` (through the bridge or a native screen), **R** `release` (runbooks, rehearsal scripts), **T** tests and devicectl only.
- **Returns**: the success value (§25 gives the types).
- **Errors**: the codes of §21 it may return (a code written without a `<spec>/` prefix is a `device/` code), besides `device/closed` (after `shutdown`) and `device/wrong_mode` (the call is not offered in the current mode), which every write may return.
- **Auth**: "auth" means the caller must have run device authentication (Windows Hello or Touch ID on a desktop, with their usual fallbacks; BIOMETRIC_STRONG with no PIN fallback on Android, SYNC.md §5.3, V1-5) immediately before the call; the device cannot check it, so the duty is the caller's (DEV-FACADE-003 on desktops, BRIDGE-AUTH-001 on Android). "—" means none.
- **Blocks**: **read**: answers from the published view and never waits (DEV-THREAD-006); **short**: waits for the sync thread, at most one loop turn plus its own fsyncs (on the reference hardware of §20.4, under 300 ms at the 99th percentile); **long**: waits for network or user steps, up to the stated bound, and must be called off the drawing thread (DEV-THREAD-008).
- **Platforms**: where it is offered (SYNC_PLAN.md §2). Elsewhere it answers `device/not_on_this_platform`. "API 30" means Android with API level 30 or later; below it the call answers `device/needs_api30`. On Android every call marked "auth" is offered only from API 30 (ruling S30), and its row says so.

### 17.2 Opening, modes and shutdown

```rust
pub struct OpenOptions {
    pub data_dir: PathBuf, pub platform: Platform, pub key_store: Arc<dyn KeyStore>,
    pub zone: ZoneSource, pub trust: TrustRoots, pub build_id: String, pub default_device_name: String,
    #[cfg(feature = "test-trust")] pub clock: Option<Arc<dyn TestClock>>,
    #[cfg(feature = "test-trust")] pub files: Option<Arc<dyn FileLayer>>,
}
pub enum Platform { Windows, MacOs, Linux, Android { api: u32 } }
```

| Call | Screens | Returns | Errors | Auth | Blocks | Platforms |
|---|---|---|---|---|---|---|
| `Device::open(options)` | D, A, T | `(Device, OpenReport)`: the mode, warnings (`folder_locked`, `folder_unlockable`, `torn_tail`, `bad_tail`, `quarantined`, import warnings and quarantines), the local-only import report, the identity (board, dev, relay address, device name) or the `KeyLost` identity, a pending create (§18.1) | `folder_in_use`, `folder_locked` and `folder_unlockable` (synced), `cloud_folder`, `store_descriptor`, `store_corrupt`, `keystore_unavailable`, `keystore_failed`, `legacy_unreadable` | — | long: reads every file; at most 150 ms for a checkpoint of the 10,000-row board plus 1 ms per log entry above it, on the reference hardware | all |
| `mode()` | D, A | `Mode` | — | — | read | all |
| `shutdown()` | D, A, T | `()` once the engine reported `ReadyToExit` (final checkpoint, outbox and meta durable, ENG-CKPT-002), the notes draft was flushed (§17.6), `device.tdk` was written, and every thread was joined | — | — | long: at most 10,000 ms, after which the threads are left to the process exit and `device/shutdown_timeout` is returned | all |
| `identities()` | D, A | `Vec<StoredIdentity>`: one entry per identity directory under `sync/` whose `store.tds` is valid, except the one `pending.tds` names, from its `store.tds` (§6.2), with `active` true for the one `active.tds` names; read at `open` and after every call that adds or removes a directory, published with the view, a change raising `Changes.status` | — | — | read | all |
| `forget_identity(board, dev)` | D, A | `()`: for a non-active identity, removes its key-store entry, then its identity directory (every file, `store.tds` last, then the directory, then `sync_dir` of `sync/`); in `KeyLost` mode it may also name the active identity: it first removes `sync/active.tds` (remove procedure), then does the same, and the facade continues in `LocalOnly` mode (§5.4 step 2, DEV-FACADE-009) | `identity_active` (the active identity, outside `KeyLost` mode), `keystore_failed` | auth | short | desktops; API 30 |

`shutdown` replaces today's `exiting` handler steps (`flush_pending_saves`, `shutdown_sync`, digest §10). A process killed instead loses nothing durable (§7.6).

The active identity changes inside the running facade: `CreateFlow::finish` and `resume_create` (§18.1 step 10), the activation of a `join` or `re_pair` (§18.3, §18.4), the activation of `reset_trust` (§18.6), and `forget_identity` of the active identity in `KeyLost` mode (§17.2) "reopen" the facade only in this sense. The caller keeps its `Device` and learns of the change through `mode()`, `status()` and the change counters.

- **DEV-FACADE-009** (device) When a call of §17.2's list activates another identity or deactivates the active one, the facade MUST keep every existing `Device` handle valid, MUST make `mode()` and `DeviceStatus.mode` report the new mode from the loop turn that activates it, and MUST raise `Changes.status` and `Changes.view` in that loop turn.

### 17.3 Change notification

The facade keeps monotonically increasing `u64` change counters, each raised when its part of the published view changes:

```rust
pub struct Changes { pub any: u64, pub view: u64, pub status: u64, pub notices: u64, pub banners: u64,
                     pub overlay: u64, pub weather: u64, pub picture: u64, pub backup: u64,
                     pub pairing: u64, pub ops: u64 }
```

`any` is raised whenever any other counter is. Three ways to learn of changes, which the caller may combine:

| Call | Screens | Returns | Blocks |
|---|---|---|---|
| `set_waker(waker: Arc<dyn Fn() + Send + Sync>)` | D | `()`; the waker is called on the sync thread after every publish that raised `any`, at most once per loop turn; it must return at once | read |
| `changes()` | D, A | `Changes` | read |
| `wait_for_change(since_any: u64, timeout_ms: u32)` | A, T | `Changes` as soon as `any > since_any`, or at the timeout (at most 60,000 ms) | long |

The desktop's waker is `proxy.send_event(())` on its winit `EventLoopProxy`, as today's sync, calendar and phone threads wake it (digest §9); its frame then compares counters, exactly as it compares `weather_service.version` today. DOCUMENTATION.md §14.1 is untouched. The bridge blocks a worker thread in `wait_for_change` and forwards a change to the page (`bridge.md`); JNI never calls a Java method from a Rust thread for this.

- **DEV-NOTIFY-001** (device) The facade MUST raise each change counter of §17.3 in the loop turn that publishes the change it counts, MUST raise `any` with it, and MUST never lower a counter.
- **DEV-NOTIFY-002** (device) The facade MUST call the waker at most once per loop turn, from the sync thread, only after the publish, and MUST NOT hold any lock while it calls it.
- **DEV-NOTIFY-003** (device) `wait_for_change` MUST return no later than 10 ms after a publish that raised `any` above `since_any`, and no later than its timeout.

### 17.4 Intents in, results out

```rust
pub struct Submitted { pub reply: Reply, pub entry_id: Option<[u8; 16]>, pub provisional_seq: Option<u64>, pub view_version: u64 }
```

| Call | Screens | Returns | Errors | Auth | Blocks | Platforms |
|---|---|---|---|---|---|---|
| `submit(intent: Intent)` | D, A | `Submitted`: the compile step's `Reply` (`core.md` §13.1), the record's id and provisional seq (synced), the view version that includes it | every `core/` compile code with its status and message (`CompileError`); the engine's `EnqueueRefused` codes (`engine/trial_panic`, `engine/not_joined`, `engine/revoked`, `engine/clock_unconfirmed`, the `apply` refusal codes); `device/needs_sync` (`set_picture` in local-only mode); `device/subscriptions_frozen`; `device/write_failed` | — | short | all |
| `submit_named(intent)` | D, A | as `submit`, with the naming hold (`Enqueue.hold = true`, `engine.md` §9.3), for a create that the screen will follow with the name the user is about to type (a `rename` or a `forget`); an intent that already carries the name the user typed, such as `quick_add`, goes through `submit` | as `submit` | — | short | all |
| `release_hold()` | D, A | `()` | — | — | short | all |
| `submit_json(text: &str)` | A | as `submit`, after `parse_intent` (`core.md` §12.2); the bridge renders the page's reply | `core/intent_json` and as `submit` | — | short | all |

A `submit` in synced mode runs on the sync thread, with no other engine input in between (`engine.md` §9.1): `parse` (for `submit_json`), `compile` against `Engine::view()` with the device zone, `now` = the device clock in whole milliseconds, `device_no` from the engine, the overlay and the minter; before the device has a `device_no` (`Engine::device_no()` is `None`) it returns `engine/not_joined` without compiling; on `Ok(Compiled { command: None, .. })` it returns the reply at once; otherwise it sends `Enqueue { ticket, command, at_ms: now, origin_state, hold }` and returns when `Notify(Enqueued)` arrives, which the engine emits only after the record is durable (ENG-ENQ-004, SYNC.md §4.1: "fsynced before `apply` returns"). In local-only mode it runs §9.3.

- **DEV-FACADE-001** (device) `submit` MUST compile against the engine's pending view and send the `Enqueue` in the same loop turn, with `at_ms` equal to the compile step's `now` and `origin_state` the minter after compiling.
- **DEV-FACADE-002** (device) `submit` MUST return success only after the engine's `Enqueued` for its ticket (synced) or after the plain-file writes of §9.3 (local-only), and MUST return the engine's or the compile step's code otherwise.
- **DEV-FACADE-003** (desktop) The desktop app MUST run device authentication (Windows Hello or Touch ID) immediately before every facade call that §17 marks "auth"; on Android the same duty is BRIDGE-AUTH-001's.

### 17.5 Read queries

`view()` returns `Arc<Published>` (§4.4). `Published` answers the queries the UI and the phone page already ask, under their names (SYNC_PLAN.md §3.3, `core.md` §14.2), with the device's zone supplied by the facade:

| `Published` method | Wraps (`core.md` §14) | Screens |
|---|---|---|
| `board()` | the pending `BoardData` | D |
| `version()` | `view_version` (synced) or the local-only counter, raised on every change | D, A |
| `ranked_ids(now)`, `shuffle_seed(now)` | `read::ranked_ids`, `read::shuffle_seed` | D, A |
| `behind_minutes(day, now)`, `next_free_start(day)`, `reflow_inputs(day)` | `read::…` | D |
| `placements_for(item, day)` and the other planner functions that take a day | `planner::placements_for` with the published board's live item of id `item` and the device zone (an empty result when no live item has that id); the others `planner::…` | D |
| `archive_newest_first()`, `archive_by_day(from, days)`, `group_by_month(rows)` | `read::…`, `archive::…` | D, A |
| `snapshot(from, days, now, ranked)` and `answer_snapshot(…)` | `snapshot::snapshot` with this view's overlay, `version`, `look` and `dials`: `look` is `Some(Look { id, aspect: 720.0 / 1560.0, top: format!("#{:02x}{:02x}{:02x}", top[0], top[1], top[2]) })`, `id` and `top` those of the `PictureView` that `picture()` returns (the phone's own picture while it has one, §14.3, else the shared one), and `None` when `picture()` returns `None`; `dials` is `Dials { blur, light }` from `device.tdk`'s `look` (§16) | A |
| `reminders(from, until)` | `reminders::reminders` with the leads of `device.tdk` | A |
| `overlay()`, `weather()`, `picture()` | the per-device overlay, the latest report with its `fetched_ms` and the `WeatherSource` it was fetched for (`weather.tdk`'s `source`, §6.4.12, or the in-memory round's in local-only mode), the processed picture (`look` bytes, `id`, `top`): the phone's own picture while it has one (§14.3), else the shared one | D, A |
| `pending_marks()` | `Engine::pending()`: "will be refused" marks (V3-6) | D, A |
| `notices()` | `Engine::notices()` in its order, each `NoticeEntry`'s `seq`, `index` and `dismissed` copied unchanged and its `record` decoded as `core.md` §10.7 (a `Notice` or a `RefusalRecord` by its `kind`); empty in local-only mode | D, A |
| `status()` | §17.8 | D, A |

Every method is a read (DEV-THREAD-006); `now` is the caller's clock, as in today's calls. The UI keeps its own display order and never reorders replicated data (SYNC.md §4.5).

- **DEV-FACADE-004** (device) Every read query of `Published` MUST pass the device's current zone to the core function it wraps and MUST return exactly that function's result for the published board.

### 17.6 The notes draft and its merge

The notepad edits a draft, not `board.notes` (SYNC.md §4.6). The facade holds the draft:

| Call | Screens | Returns | Errors | Blocks |
|---|---|---|---|---|
| `notes_draft()` | D, A | `Option<Draft { text, base_rev, edited_ms }>` | — | read |
| `set_notes_draft(text)` | D, A | `()`: replaces the draft (starting it at the current `notes_rev` when there was none) | `core/notes_long` (after detab) | short |
| `flush_notes_draft()` | D, A | `Submitted`: submits `set_notes { text, base: base_rev }` now | as `submit` | short |
| `notes_merge()` | D, A | `Option<NotesMerge { mine, theirs, base_rev, current_rev }>`: present while the newest refusal of this device's `set_notes` is `core/notes_stale` and the draft is unsaved | — | read |
| `resolve_notes(text)` | D, A | `Submitted`: submits `set_notes { text, base: current notes_rev }` and replaces the draft | as `submit` | short |
| `discard_notes_draft()` | D, A | `()` | — | short |

**Saving.** A draft is flushed 30,000 ms after its first unflushed change (so typing produces at most one save per 30 s), on `flush_notes_draft` (the app calls it when the notepad loses focus), and at `shutdown`. The draft stays until its `set_notes` entry is applied to the confirmed state; then `draft.tdk` is removed. A refusal with `core/notes_stale` (another device saved first, `core.md` §10.5) keeps the draft and opens the merge view; it neither duplicates nor loses text (SYNC.md §12). `draft.tdk` is written at each flush and at each refusal, so a draft survives a restart. In local-only mode there is no conflict; the draft is saved on the same schedule.

**A `set_notes` from `submit`.** In synced mode, a `set_notes` that reaches `submit`, `submit_named` or `submit_json` (the phone page's notes sheet sends it as a verb, `bridge.md` §6.3) is a flush: once the compile step has succeeded and before the `Enqueue`, the facade sets its draft to the compiled command's text, with `base_rev` the `base` the intent carried (the pending view's `notes_rev` when it carried none) and `edited_ms` the compile step's `now`, and writes `draft.tdk`. The draft then has no unflushed change, and it is kept, offered in `notes_merge` and removed exactly as a flushed draft is.

- **DEV-FACADE-005** (device) The facade MUST submit a notes draft at most once per 30,000 ms while it changes, plus on `flush_notes_draft` and at `shutdown`, and MUST keep the draft until its entry is applied or the user discards it.
- **DEV-FACADE-006** (device) After a `core/notes_stale` refusal of its own `set_notes`, the facade MUST offer `notes_merge` with the draft and the board's current notes, and MUST NOT submit the draft again until `resolve_notes` or `discard_notes_draft`.
- **DEV-FACADE-008** (device) In synced mode, before it sends the `Enqueue` of a `set_notes` compiled from `submit`, `submit_named` or `submit_json`, the facade MUST make that command's text its notes draft with the `base_rev` and `edited_ms` of §17.6, MUST write `draft.tdk`, and MUST then treat the draft as flushed by that submit.

### 17.7 Notices, banners, alarms, the halted view

| Call | Screens | Engine input | Errors | Auth | Blocks | Platforms |
|---|---|---|---|---|---|---|
| `dismiss_notice(seq, index)` | D, A | `User(DismissNotice { seq, index })`, with the `seq` and `index` of a `NoticeView` (§17.5) | — | — | short | all |
| `ack_banner(id)` | D, A | `User(AckBanner)` | — | — | short | all |
| `dismiss_alarm(id)` | D, A | `User(DismissAlarm)` (ignored for a stopping alarm, ENG-UI-002) | — | — | short | all |
| `confirm_clock()` | D, A | `User(ConfirmClock)` | — | — | short | all |
| `confirm_rejoin()` | D, A | `User(ConfirmRejoin)` | — | — | short | all |
| `ack_continuity()` | D, A | `User(AckContinuity)`: the user has read `ContinuityUnchecked` (`engine.md` §19.7 step 7) | — | — | short | all |
| `ack_restored_copy()` | D, A | `User(AckRestoredCopy)`: the user has read `RestoredCopy` (`engine.md` §11.3 step 3) | — | — | short | all |
| `ack_based_on_unchecked()` | D, A | `User(AckBasedOnUnchecked)`: the user has read `BasedOnUnchecked` (`engine.md` §13.1) | — | — | short | all |
| `use_this_copy()` | D, A | `User(UseThisCopy)` | — | auth | short | desktops; API 30 |
| `adopt(dev, typed_fp)` | D, A | `Op(Adopt { dev, typed_fp })`: adopting a state by fingerprint (V1-10); returns a ticket | `engine/op_busy`; through `wait_op`: `engine/fingerprint`, `engine/adopt_membership`, the open and load codes of `engine.md` §22.3 step 3, `engine/op_timeout` | auth | short | desktops; API 30 |
| `skip(seq)` | D, A | `Op(Skip { seq })`: the halted view's Skip | `engine/skip_target`, `engine/op_busy` | auth | short | desktops; API 30 |

The halted view is the pending view with `status().engine.halt` (frozen since, the entry's author, the Skip offer); alarm evidence is `status().engine.alarms` with each stop's `action`.

The desktop app shows each notice and refusal record of `NoticeView` (§17.5) with the line `bridge.md` A-04 gives for that record, with `This device's` in place of `This phone's`, `name(id)` the item's name in quotes (or `#<id>` when the item is not on the board) and `dev(n)` the name of device `n` in `status().engine.devices` (or `device <n>`).

While `status().engine.stops` holds the stop of kind `Rejoin` (`engine.md` §19.7 step 1), the app shows the line "This device was away longer than the relay keeps its history. Rebuild it from the newest checked snapshot? Changes not yet sent stay queued and are sent after the rebuild." with the button "Rebuild", which calls `confirm_rejoin()`.

- **DEV-FACADE-010** (desktop) The desktop app MUST show each notice and refusal record with the line of `bridge.md` A-04 for that record, with `This device's` in place of `This phone's`.
- **DEV-FACADE-011** (desktop) While `status().engine.stops` holds a stop of kind `Rejoin`, the desktop app MUST show the line "This device was away longer than the relay keeps its history. Rebuild it from the newest checked snapshot? Changes not yet sent stay queued and are sent after the rebuild." with a button "Rebuild" that calls `confirm_rejoin()`.

### 17.8 Sync status, devices and operations

`status()` (read) returns `DeviceStatus`: the engine's `Status` (`engine.md` §30.1: session, last synced, unreachable since, paused until, queued, doomed, head, fingerprint, clock, halt, stops, alarms, banners, devices with their last beat and last entry as "last seen", rules with each newer version's blockers, the recovery-key change, a move, operations, unread counts, warnings), plus the device's own: the mode, the relay address, whether a socket is open, today's request count, the overlay's per-subscription statuses, the weather's age, the picture status, the backup status (§15.6), the pairing state (§18.3), and `device_name`.

**What blocks a rules upgrade** is `status().engine.rules.versions[i].blockers` (ENG-RULES-001); **last seen** is `status().engine.devices[i].last_beat_ms` and `last_entry_ms`, never an alarm (V2-10).

| Call | Screens | Engine input | Returns | Errors | Auth | Blocks | Platforms |
|---|---|---|---|---|---|---|---|
| `revoke(dev, replace)` | D, A | `Op(Revoke)` | a ticket | `engine/revoke_target`, `engine/op_busy` | auth | short | desktops; API 30 |
| `restore_targets()` | D | — | `Engine::restore_targets()`, the `(seq, τ)` the user may restore to | — | — | read | desktops |
| `restore_to(seq)` | D | `Op(RestoreTo)` | a ticket | `engine/op_busy`, and the target rules (ENG-RESET-001) as `device/restore_target` | auth | short | desktops |
| `purge_preview()` | D | — | `{ reach_seq, reach_ms }` from `Engine::purge_preview()` in the published view (§4.4): the seq the purge would reach and the log-clock time of the entry before it (`engine.md` §28.2 step 1, ENG-PURGE-004) | `engine/nothing_to_purge` | — | read | desktops |
| `purge()` | D | `Op(Purge)` | a ticket | `engine/op_busy`, `engine/nothing_to_purge` | auth | short | desktops |
| `rules_override(version)` | D | `Op(RulesOverride)` | a ticket | | auth | short | desktops |
| `move_board(host, create_secret)` | D | `Op(Move { host })`, `host` normalised as `relay.md` §5.4 step 4 (letters to lowercase, a trailing `:443` removed), once it passes `seal.md` §5.4 (at most 255 bytes, `:port` included, never `:443`; else `relay/address`); the device holds the create secret for that operation's `Door` and `Seed` requests and wipes it when the operation ends | a ticket | `relay/address`, `engine/op_busy` | auth | short | desktops |
| `move_status()` | D | — | the move code (`Notify(MoveCode)`) and `MoveReport` | — | — | read | desktops |
| `enter_move_code(text)` | D, A | `User(MoveCode)` | `()` when the `Notify(MoveCodeResult)` of the same loop turn is `Ok` | from that result: `engine/move_locked`, `engine/move_no_fence`, `seal/move_code` | — | short | all |
| `follow_address(address, code)` | D, A | `Op(FollowAddress { host, door, code })`, `host` and `door` from `parseAddress` (`relay.md` §5.4) | a ticket | `relay/address`; through `wait_op`: `engine/move_locked`, `engine/move_no_fence`, `seal/move_code` | — | short | all |
| `wait_op(ticket, timeout_ms)` | D, A, T | — | the operation's `OpDone` result, or `None` at the timeout (at most 60,000 ms) | — | — | long | all |
| `ops()` | D, A | — | operations in progress with their stage | — | — | read | all |

Operations run in the engine; their stages survive a restart (ENG-OPS-003). The platform column is SYNC_PLAN.md §2: restore to a point, purge, reset trust and recovery-key changes are desktop-only; the phone adds, revokes and replaces devices only from API 30.

- **DEV-FACADE-007** (device) The facade MUST refuse every call outside the platforms its row of §17 lists, with `device/not_on_this_platform` or, on Android below API 30, `device/needs_api30`, before it sends anything to the engine.

### 17.9 Platform events

| Call | Screens | Does | Blocks |
|---|---|---|---|
| `set_foreground(visible)` | D, A | §10.9; the state at `open` is `true` on desktops and `false` on Android (DEV-NET-017) | short |
| `notify_resume()` | D (wake from sleep, network change), A | the next `Connect` skips the backoff once (§10.7) | short |
| `background_sync(budget_ms)` | A | a background run (§10.9); returns `{ synced: bool, queued: u32, head: u64 }` | long, at most `budget_ms` ≤ 600,000 |
| `set_zone(zone: TableZone)` | A | replaces the zone; republishes the view | short |

### 17.10 Pairing, creating a board, the recovery sheet

These are flows (§18). Their calls:

| Call | Screens | Returns | Errors | Auth | Blocks | Platforms |
|---|---|---|---|---|---|---|
| `begin_create(request)` | D | `CreateFlow` (§18.1) | `no_keystore`, `folder_locked`, `cloud_folder`, `relay/address`, `legacy_unreadable`, `folder_locked` (source) | auth | long: imports the source | desktops |
| `CreateFlow::sheet()` | D | `SheetData`: the seed's 14 groups (zeroizing), the relay address, the recovery key fingerprint, the release-signing key fingerprint, the date | — | — | read | desktops |
| `CreateFlow::confirm_sheet(typed)` | D | `()` | `seal/sheet_*` codes, `device/sheet_mismatch`, `device/sheet_test_failed` | — | short | desktops |
| `CreateFlow::finish()` | D | `CreatedBoard { address }`; the facade reopens in synced mode | `relay/*` codes of `create`, `device/create_failed` | — | long: at most 120,000 ms | desktops |
| `resume_create(create_secret)` | D | as `finish` (§18.1 step 10) | as `finish` | auth | long | desktops |
| `abandon_create()` | D | `()` | — | — | short | desktops |
| `pair_start()` | D, A | `PairSheet { address_display, code_display }` (§18.3) | `engine/not_eligible`, `engine/halted`, `engine/pair_busy` | auth | long: at most 20,000 ms | desktops; API 30 |
| `pair_confirm()` | D, A | `()`: the user confirmed on this device that the new one accepted the digits | `device/pair_state` | — | short | desktops; API 30 |
| `pair_cancel()` | D, A | `()` | — | — | short | all |
| `join(address, code, device_name)` | D, A | `()`; progress in `status().pairing` | `relay/address`, `seal/code_format`, `seal/code_check`, `no_keystore`, `folder_locked`, `cloud_folder` | — | short | all |
| `join_sas(digits)` | D, A | `()`; `status().pairing` says "accepted" or "does not match" | `seal/sas_mismatch` after the third | — | short | all |
| `re_pair(address, code, device_name)` | D, A | as `join`, for a revoked, halted, stranded or closed identity, with its unsent commands resubmitted after the join (§18.4) | as `join` | — | short | all |
| `begin_recovery_change()` | D | `RecoveryChangeFlow` (§18.5) | `engine/op_busy` | auth | short | desktops |
| `RecoveryChangeFlow::confirm_current_sheet(typed)` | D | `()` | `seal/sheet_*` codes, `device/sheet_mismatch` | — | short | desktops |
| `RecoveryChangeFlow::confirm_sheet(typed)`, `finish()` | D | `()`; `finish` returns the seq of the accepted entry | as `CreateFlow::confirm_sheet`; for `finish` the relay's codes of the append, and the code of a refused entry | — | long | desktops |
| `reset_trust(request)` | D | `CreateFlow` with the current confirmed board as source (§18.6) | as `begin_create` | auth | long | desktops |

The recovery sheet's data exists only inside a `CreateFlow` or `RecoveryChangeFlow` (`sheet()`), in zeroizing memory, never in a file: the desktop prints it through the operating system's print dialog from memory, or shows it full screen with capture excluded (V3-15). After the flow ends the seed is gone; the relay address and the fingerprints stay available in `status()`.

- **DEV-SHEET-001** (device) The facade MUST hold a recovery seed only in zeroizing memory inside a create or recovery-change flow, MUST NOT write it to any file, and MUST erase it when the flow finishes or is dropped.

### 17.11 Backups

| Call | Screens | Returns | Errors | Auth | Blocks | Platforms |
|---|---|---|---|---|---|---|
| `backup_destinations()` | D | the destinations | — | — | read | desktops |
| `set_backup_destinations(list)` | D | `()` | `device/destination_invalid` | auth | short | desktops |
| `backup_status()` | D | the status of §15.6 | — | — | read | desktops |
| `backup_now()` | D | `()`: evaluates the schedule now, writing what is due | — | — | short | desktops |
| `pin_recovery_fingerprint(typed)` | D | `()` | `device/fingerprint_mismatch` | auth | short | desktops |
| `delete_backups_before(before_ms)` | D | the number of files deleted | — | auth | long: at most 60,000 ms | desktops |

### 17.12 The shared picture, import and export

| Call | Screens | Returns | Errors | Auth | Blocks | Platforms |
|---|---|---|---|---|---|---|
| `set_shared_picture(bytes)` | D | `Submitted` of the `set_picture` | `device/picture_too_large`, `device/picture_format`, `device/picture_decode`, `device/needs_sync`, the blob codes of `relay.md` §10.7 | — | long: at most 120,000 ms | desktops |
| `clear_shared_picture()` | D | `Submitted` of `set_picture` with no picture | as `submit` | — | short | desktops |
| `set_own_picture(bytes)` | A | `Option<String>`: the id of the own picture's new look, returned after it is published (§14.3) | `device/picture_too_large`, `device/picture_format`, `device/picture_decode`, `device/needs_sync` | — | long: at most 120,000 ms | Android |
| `clear_own_picture()` | A | `Option<String>`: the id of the shared picture's look now shown, or `None` when the board has none, returned after the publish (§14.3) | `device/needs_sync` | — | short | Android |
| `set_look_dials(blur, light)` | D, A | `Option<String>`: the id of the look now shown (§14.2 step 8), returned after the new look is published; `None` when no picture is shown | — | — | long: at most 5,000 ms | all |
| `weather_place()` | A | `(WeatherSource, Option<Home>)`: `weather_source` and `own_place` of `device.tdk` (or `device.json`), from the published view | — | — | read | Android |
| `set_weather_place(source, own_place)` | A | `()`: `Home` clears `own_place` and sets `weather_source` to `"home"`; `Own` stores the given place and sets `"own"`; the weather thread then fetches at once (§13.2) | `device/bad_place` (`Own` with no place, or a place outside −9,000 to 9,000 and −18,000 to 18,000) | — | short | Android |
| `set_reminder_leads(event, block, due)` | A | `()` | — | — | short | Android |
| `taskdeck_device::import_check(folder)` | D (`TaskDeck --import-check <folder>`) | `ImportCheck { report, genesis_sha256 }`: the report of `core.md` §15.4 and SHA-256 of the genesis canonical bytes in lowercase hex | `device/folder_locked`, `device/legacy_unreadable` | — | long | desktops |
| `export_legacy(target)` | D | `()`: writes today's files from the pending view into `target` (§18.7) | `device/export_target`, `device/write_failed` | — | long | desktops |

`import_check` is a free function: it needs no open facade and writes nothing.

## 18. Flows

### 18.1 Creating a board (desktop)

`begin_create(CreateRequest { relay_host, create_secret, source, device_name })`, with `source` one of `Empty` (`BoardData::empty()`), `ThisFolder` (the plain files of this data folder), `Folder(path)` (another folder's plain files) or `CurrentBoard` (reset trust, §18.6). In order; each failure leaves the data folder as it was:

1. **Checks.** A desktop; a key store; the lock `Held`; the cloud check (§5.3); mode `LocalOnly` or `KeyLost` (or `Synced` for `CurrentBoard`); `relay_host`, after the normalisation of `relay.md` §5.4 step 4 (letters to lowercase, a trailing `:443` removed), a host of `seal.md` §5.4 (at most 255 bytes, `:port` included), else `relay/address`; the normalised host is the one stored and used; `create_secret` 64 lowercase hex characters, held zeroizing.
2. **Genesis board.** `ThisFolder` or `Folder`: the source's `.lock` is free (DEV-LOCK-004), its files are read, and `legacy::import` runs (`core.md` §15); a failure is `device/legacy_unreadable` with the report. `CurrentBoard`: the engine's confirmed board bytes, unchanged.
3. **Door.** `GET /v1/b/<board>/door` needs the board id, so the keys come first: draw the recovery seed (`RecoverySeed::generate`), the salt, this device's keys, `L_d`, and `R_1`; compute `board_id` (`seal.md` §6.2) and `dev_R`; then ask the relay for the door with the create secret (`relay.md` §10.9). The relay address is `relay_host` and Crockford of `board_id ‖ door`.
4. **Genesis frame.** When the genesis board is not `BoardData::empty()`, seal its genesis snapshot at `S = 1` (`seal.md` §20, purpose 3) and keep its sealed bytes; they are never sealed again. Build seq 1 with the sealer adapter: the `Genesis` item (`seal.md` §12.2) naming the first device, epoch 1 with its wraps and commitment, rules version 1, the state hash of the genesis board and `M_1`, and the genesis snapshot's blob id when there is one; signed by the recovery identity with `ctr = 1`.
5. **Sheet.** `sheet()` returns `SheetData` (§17.10). The desktop prints it twice through the OS print dialog from memory, or shows it full screen with capture excluded (SYNC.md §5.5, V3-15).
6. **Retype.** `confirm_sheet(typed)`: `RecoverySeed::from_sheet(typed)` (`seal.md` §18.2), then a constant-time comparison with the held seed (`device/sheet_mismatch`); the user may try any number of times. Then the **test backup**: a nightly backup of the genesis state is written in memory to the recovery key, with no read token (`read_token: None`, `seal.md` §21.5: the board holds none yet), and read back with `read_backup` and the retyped seed's keys (V1-17); a failure is `device/sheet_test_failed`.
7. **Persist, inactive.** `KeyStore::create(L_d)`; create the identity directory (when the key store has not, §12.5) with `store.tds`, `identity.tdk` (keys, `R_1`, the anchor), `device.tdk` (`device_name`, `history_complete: true`, `minter` for `device_no` 1, `pinned_recovery_fp` and `pinned_recovery_pk` of the new recovery key), `genesis.tdb` (the sealed genesis snapshot of step 4, replace procedure; only when there is one), and `bootstrap.tdk` (`Genesis { board, frame_1, relay }`, `relay` being `relay_host`, the door of step 3 and `home_gen = 1`); then write `sync/pending.tds` (the layout of `active.tds` with magic `"TDP1"`) naming it. Nothing is active yet.
8. **Create at the relay.** `finish()`: `POST …/create` (`relay.md` §10.2) with the create secret. A lost answer is retried: the relay's "Repeat" answers the same create with a new token.
9. **Token and snapshot.** Write the token into `device.tdk` (without `push`, §10.3); when there is a genesis snapshot, upload exactly the bytes of `genesis.tdb` with that token (purpose 3, `seq = 1`, with the `blob`, `size` and `sha256` the Genesis item names), resuming from `blob-meta` as DEV-NET-010 states.
10. **Activate.** Replace `sync/active.tds` with the new identity and remove `pending.tds`; for `ThisFolder`, move the plain files aside (§18.2); for `CurrentBoard`, §18.6; then reopen the facade in synced mode: `Start` carries `Bootstrap::Genesis` (`engine.md` §19.8). The seed and the create secret are erased.

A crash between steps 7 and 10 leaves `pending.tds`: the next `open` opens the old mode and reports the pending create; `resume_create(create_secret)` runs steps 8 to 10 (the keys and the sealed genesis snapshot are in the identity directory; the seed is on paper, not needed again; a `genesis.tdb` whose SHA-256 differs from the Genesis item's `sha256` fails it with `device/create_failed`), and `abandon_create()` removes the pending identity directory, its key-store entry and `pending.tds`.

- **DEV-CREATE-001** (device) Creating a board MUST make the identity directory and its key-store entry durable, and MUST write `pending.tds`, before it sends `create` to the relay.
- **DEV-CREATE-002** (device) Creating a board MUST NOT send `create` before the user retyped the whole sheet, the decoded seed matched, and a test backup opened with it.
- **DEV-CREATE-003** (device) The active identity MUST change only by the replace of `sync/active.tds` in step 10 (or §18.4), after the relay accepted the genesis and the token is durable.
- **DEV-CREATE-005** (device) Creating a board from a genesis board that is not empty MUST make the sealed genesis snapshot durable in `genesis.tdb` before it sends `create`, and MUST upload only those bytes, never a snapshot sealed again.

### 18.2 "Create board from this folder"

`source = ThisFolder` (SYNC.md §9 step 6). After step 10 has written `active.tds`, the plain files are moved, untouched, into `taskdeck_data/pre-sync-<YYYYMMDD-HHMMSS>/` (local time): `read_at_startup.json`, `archived.jsonl`, `notepad_text.json`, `subscriptions.json`, `subscribed_cache.json`, `device.json` and `feeds/`, each by one `rename`, then `sync_dir`; `userconfig.toml` and `colorschemes.json` are copied there (they stay in use by the desktop), so an old build opening `pre-sync-…` finds a complete folder (SYNC.md §9 step 8). On a rename failure every earlier rename is undone in reverse order and the facade reports `device/set_aside_failed`; the board is created and active, and the plain files are no longer read (the mode is synced). Deleting the phone token is `desktop`'s (SYNC.md §9 step 6).

The first board created from a legacy folder takes its palette from the scheme that `selected_colorscheme_id` of that folder's `userconfig.toml` names in its `colorschemes.json`, and its home place from that file's `coordinates` (SYNC.md §9 step 6). In §18.1 step 2 the device reads `LegacyConfig` from the source folder's own `userconfig.toml` (this data folder's for `ThisFolder`, the named folder's for `Folder(path)`) with today's parser (initialization.rs:243-362, `core.md` §15.2), reading only and writing nothing to it; `import_check` reads it the same way.

`import_check(folder)` (§17.12) is the dry run: the same steps 1 and 2 with nothing written, the report and the SHA-256 of the genesis bytes, which must repeat on a second run (SYNC.md §9 step 4).

- **DEV-CREATE-004** (device) After creating a board from this folder, the device MUST move the plain files of §18.2 into `pre-sync-<stamp>/` unchanged, and MUST undo every move when one fails.
- **DEV-CREATE-007** (device) For `ThisFolder`, `Folder(path)` and `import_check`, the device MUST fill `LegacyFolder.config` from the `userconfig.toml` of the source folder itself, with today's parser and its clamp, and MUST NOT take `coordinates` or `selected_colorscheme_id` from any other file or folder.

### 18.3 Pairing

**On the existing device (E)** — `pair_start()` after device authentication (desktops; Android from API 30). The device runs `taskdeck_seal::pair::Existing` (`seal.md` §17.10) and performs its effects:

| Machine effect | Device |
|---|---|
| `RegisterTicket { tid, slot A }` | `POST …/pair` with the token (`relay.md` §10.8) |
| `ShowCode { code }` | returned by `pair_start` as `code_display` (`XXXX-XXXX-XXXX-XXXX-CC`), with the relay address's display form; the desktop shows them on the pairing sheet with capture excluded on the main window (SYNC_PLAN.md §2), never on the wall layout |
| (waiting for B) | on the socket's `slot` notice, or every 2,000 ms over HTTPS, `GET …/pair/<tid>/b`; `404 relay/slot_empty` means wait |
| `PostSlot { C }` | `POST …/pair/<tid>/c` |
| `ShowSas { digits }` | `status().pairing = ShowSas { digits }` |
| user confirms | `pair_confirm()` → `UserConfirmed` |
| `NeedSnapshotAndRegister { device item }` | `PairRegister { ticket, device }` to the engine (`engine.md` §28.4); the transport keeps the token string of the `register` answer for this ticket |
| `Notify(PairRegistered { head, snapshot, added_seq })` | `Registered { h, chain, snapshot ref, added_seq, token, roots, host, door }` to the machine, with the token kept above and, from the sealer adapter, the roots of the snapshot's epoch and of the epoch before it when that is 2 or more, never a later root (`seal.md` §17.8, `engine.md` ENG-OPS-006) |
| `PostSlot { D }` | `POST …/pair/<tid>/d` |
| `Done`, `Abort { code }` | `status().pairing = Done` or `Aborted { code }`; on abort, `DELETE …/pair/<tid>` |

The machine aborts 600,000 ms after its start (SEAL-PAIR-013). A device that crashes while registering leaves a `DeviceAdded` for a device that never joins: it shows in every device list with no last beat, and the user revokes it (decided here, §26).

**On the new device (N)** — `join(address, code, device_name)`; N needs no device authentication (it holds nothing yet). Checks first: the lock, the cloud check, a key store, mode `LocalOnly` or `KeyLost`; `parseAddress` (`relay.md` §5.4) and the code's check characters (`seal.md` §17.2) before any request (RELAY-ADDR-002, SEAL-CODE-002). Then `taskdeck_seal::pair::New`:

| Machine effect | Device |
|---|---|
| `FetchSlot { tid, ticket_key, A }`, `{ C }`, `{ D }` | `GET …/pair/<tid>/<label>` with `Door` and `X-TD-Proof: <ticket_key hex>`; `slot_empty` means poll again after 2,000 ms |
| `PostSlot { B }` | `POST …/pair/<tid>/b` with `Door` and the proof; N's keys are generated in memory and kept zeroizing |
| `AskSas` | `status().pairing = AskSas`; `join_sas(digits)` → `SasEntered` |
| `ShowAccepted` | `status().pairing = Accepted` (the user then confirms on E) |
| `FetchSnapshot { token, blob, parts }` | `GET …/blobs/<blob>` and its parts with the bundle's token |
| `ReadLog { token, from }` | `GET …/log?after=<from − 1>` pages to the head with that token |
| `Joined { device_no, keys, bundle, snapshot }` | persist (below) |
| `Abort { code }` | `status().pairing = Aborted { code }`; every secret erased |

**Persisting the join**, in order: `KeyStore::create(L_d)`; the identity directory (created when the key store has not, §12.5) with `store.tds`, `identity.tdk` (keys and the bundle's roots), `device.tdk` (`device_name`, the bundle's token for the host (without `push`, §10.3), `minter` for its `device_no`, `history_complete: true`), `history.tdk` from the bootstrap snapshot's history (`seal.md` §20.4, §15.7), and `bootstrap.tdk` (`Join` with the snapshot material, `added_seq` and the relay target); then the plain files are set aside (§18.4); then `active.tds` is replaced; then the facade reopens in synced mode with `Bootstrap::Join` (`engine.md` §19.8). A crash before `active.tds` leaves an inactive directory, ignored by `open`, and a `DeviceAdded` that the user pairs again over.

- **DEV-PAIR-001** (device, desktop) The desktop MUST exclude the main window from screen capture while the pairing sheet shows the code or the sheet shows a seed, and MUST NOT show either on the wall layout.
- **DEV-PAIR-002** (device) The device MUST perform the effects of both pairing machines exactly as the two tables of §18.3 state, and MUST NOT persist any key, root or token of a joining device before the machine emitted `Joined`.
- **DEV-PAIR-003** (device) A joining device MUST check the code's check characters (SEAL-CODE-002) before it sends any request; its relay address is checked as RELAY-ADDR-002 states.
- **DEV-PAIR-006** (device) A joining device MUST make `history.tdk`, holding the bootstrap snapshot's history, durable before it writes `bootstrap.tdk`.

### 18.4 Re-pair and resubmit, and the set-aside of plain files

`re_pair(address, code, device_name)` for an identity that is revoked and not reinstated (a reinstated identity resumes on its own, ruling S24), crash-halted beyond repair, under the commitment halt, stranded, or whose board was closed (`engine.md` §24.5): first `User(ExportUnsent)`; the `Notify(Unsent { commands, from_device_no, origin_state })` is written to `unsent.tdk` in the **old** identity directory (kind 18: `from_device_no u16 ‖ vb32(origin_state) ‖ n u32 ‖ n × vb32(command payload)`), so it survives a restart; then `join` as above. When the new engine has joined (ENG-BOOT-003), the runtime sends `Resubmit { ticket, commands, from_device_no, origin_state }` (each command parsed with `parse_command`), with `origin_state` the **new** identity's current minter (the `origin_state` of its newest `cmd` outbox record, else `IdMinter::new` for its own `device_no` in the adapter's encoding), never the old identity's from `unsent.tdk`, which is kept but not used (`engine.md` §24.5, `core.md` §7.5); it returns the `ResubmitReport`, and from then on the facade's minter is the `origin_state` of the newest resubmitted record; `unsent.tdk` is removed after the report is durable. The old identity directory stays until `forget_identity`.

**Set-aside.** Before a join makes a new identity active, the plain files that hold data (any of `read_at_startup.json`, `archived.jsonl`, `notepad_text.json`, `subscriptions.json` whose content is not empty after trimming White_Space and not `[]` or `""`, today's rule, sync.rs:243-290) are moved to `local-<YYYYMMDD-HHMMSS>/` with `subscribed_cache.json`, `device.json` and `feeds/`, undoing every move on a failure (`device/set_aside_failed`, and the join is not activated). The open report of the synced mode names the folder, so the user knows their local board was kept.

- **DEV-PAIR-004** (device) A join MUST move every plain file that holds data out of the data folder's top level before it makes the new identity active, and MUST NOT make it active when the move failed.
- **DEV-PAIR-005** (device) Re-pairing MUST make the old identity's unsent commands durable in `unsent.tdk` before it starts the join, and MUST resubmit them only after the new identity has joined.

### 18.5 The recovery-key change (desktop)

`begin_recovery_change()` (auth) returns a `RecoveryChangeFlow`. The recovery key changes only with the current sheet (`seal.md` §13.11): `confirm_current_sheet(typed)` takes the current sheet as typed (`RecoverySeed::from_sheet`, `seal.md` §18.2) and requires its recovery keys to be those of the membership's current recovery record (`device/sheet_mismatch`). Then the flow draws a new seed, with `sheet()` and `confirm_sheet(typed)` as in §18.1 steps 5 and 6 (the test backup is sealed to the new key); the new sheet's relay address is the board's current one. `finish()` seals the `K`-shape entry (`seal.md` §12.1) with the sealer adapter, signed by the current recovery identity: `RecoveryKeyChange` with the new public keys, then `EpochRotated` to `E + 1` with a fresh root wrapped to every current device and to the new recovery key, under epoch `E` with its root, `based_on` this device's head and `ctr` one above the current recovery record's `last_ctr`. It appends the frame with the door (`relay.md` §9.3, RELAY-SEQ-009), as the recovery tool appends its entries (§15.8 step 9, `RecoveryLink::append`, one re-seal on `gap`), and, once this device has classified the entry `Accept`, sends `recovery_register` for it (`relay.md` §12.12), and none for a refused entry (a lost rotation race), which fails the call with its code. It erases both seeds when it returns, and `RecoveryChangeFlow` erases them when dropped (DEV-SHEET-001). After the change, each backup writer pins the new fingerprint on its own screen (§15.5). A lost sheet does not change the recovery key: it is answered by Reset trust (§18.6).

### 18.6 Reset trust (desktop)

Reset trust starts a new board from this device's confirmed state: a new recovery seed and sheet, new device keys, a new `board_id`, and a genesis carrying the current confirmed board bytes unchanged; the old board is then pointed at the new one by one `Fence` (`CloseBoard`), and its devices re-pair into the new board (SYNC.md §5.4).

`reset_trust(request)` is `begin_create` with `source = CurrentBoard`: a new recovery seed and sheet, new device keys, a new `board_id` on the relay `request.relay_host`, and the current confirmed board bytes as an imported genesis (SYNC.md §5.4, `engine.md` §28.7). Between steps 9 and 10 of §18.1, while the old identity is still active, the device sends `Op(CloseBoard { host, door })` naming the new board's relay and waits for it (`wait_op`, at most 600,000 ms; a failure stops the flow before activation with `device/close_failed`, leaving the new board pending). Then step 10 activates the new identity. The old identity directory stays until `forget_identity`. Devices of the old board follow the `Fence`, find they are not members, and re-pair into the new board (§18.4).

- **DEV-CREATE-006** (device) `reset_trust` MUST create the new board by every step of §18.1 with `source = CurrentBoard`, drawing a new recovery seed, new device keys and a new `board_id` and taking the engine's confirmed board bytes unchanged as the genesis board, and MUST NOT activate the new identity (step 10) before the old board's `Op(CloseBoard)` naming the new board's relay and door has ended with `OpDone` `Ok`; on its failure or after 600,000 ms it MUST stop with `device/close_failed` and leave the new board pending.

### 18.7 "Export legacy"

`export_legacy(target)` writes the pending view as today's files into `target`, which must be an absolute path to a directory that does not exist or is empty, outside the data folder (`device/export_target`): `read_at_startup.json`, `archived.jsonl`, `notepad_text.json` and `subscriptions.json` by the mapping of §9.3; `colorschemes.json` holding one scheme under the key `"200000"`, `{"name": "Board palette", "colors": palette, "is_user_configurable": true}`, unless the palette is six `[0,0,0,0]` (then `{}`); and `userconfig.toml` with `coordinates = [lat / 100, lon / 100]` and `selected_colorscheme_id = 200000` (or 0). Each file by today's atomic procedure. Ids minted by devices (`device_no × 2^40 + n`) are written as they are; block ids are dropped (sessions carry none). Today's build opens the folder; the import reads it back with every device-minted id unchanged (`core.md` §15.3 step 9.4).

- **DEV-EXPORT-001** (device) `export_legacy` MUST write only into an empty or new directory outside the data folder, and the files it writes MUST open in today's build and give, through `legacy::import`, the exported board's items, rows, notes, subscriptions, palette and home place.

## 19. `taskdeck-devicectl`

### 19.1 What it is

A headless device for tests: one process, one facade, one data folder, built only with `test-trust` (§11.1 item 6). It is the device every end-to-end scenario drives, on Linux in containers and CI, against the standby relay over loopback and, in CI, the Worker under `wrangler dev`.

```text
taskdeck-devicectl run --data <dir> --keys <dir> [--platform windows|macos|linux|android:<api>]
                       [--name <device name>] [--zone utc|fixed:<seconds>|table:<file>]
                       [--trust-pem <file>] [--clock manual|system] [--crash-at <k>] [--build-id <text>]
taskdeck-devicectl import-check <folder>
```

`run` opens the facade with `FileKeyStore` in `--keys`, the given platform (default `linux`, which for devicectl is treated as a desktop for every platform gate, so desktop-only flows can be driven headless), zone (default `utc`), an extra trust anchor (`TrustRoots::Extra`), a manual clock (default `manual`, starting at 2026-01-05T12:00:00Z, moved only by the `clock` command, used as both `wall_ms` and the compile clock; `mono_ms` advances with it) and, with `--crash-at k`, `CrashFiles` aborting at the k-th write boundary. It then reads requests from standard input, one JSON object per line, and writes one JSON object per line to standard output: an answer per request, in order, and event lines between them.

### 19.2 Protocol

Request: `{"id": u53, "cmd": string, ...arguments}`. Answer: `{"id": u53, "ok": true, ...result}` or `{"id": u53, "ok": false, "error": code}`. Event: `{"event": "changed", "changes": {…§17.3 counters…}}`, written after each publish, and `{"event": "exit", "code": int}` before exit. Lines are at most 16,777,216 bytes; byte strings are lowercase hex; integers within ±(2^53 − 1); JSON is parsed strictly (README §5.3), an invalid line being answered `{"id": 0, "ok": false, "error": "device/ctl_request"}`.

| `cmd` | Arguments | Facade call | Result |
|---|---|---|---|
| `status` | | `status()` | the status as record JSON (§6.5) |
| `snapshot` | `from` date, `days` | `view().answer_snapshot` | `{"snapshot": …}` |
| `board` | | `view().board()` | `{"canonical": hex, "state_hash": hex32}` (the board bytes and the state hash with the current membership) |
| `submit`, `submit_named` | `intent`: the intent JSON (`core.md` §12.2) | `submit_json` | `Submitted` |
| `release_hold` | | `release_hold` | |
| `notices`, `dismiss_notice` | `seq`, `index` | | |
| `ack_banner`, `dismiss_alarm` | `id` | | |
| `confirm_clock`, `confirm_rejoin`, `use_this_copy`, `ack_continuity`, `ack_restored_copy`, `ack_based_on_unchecked` | | | |
| `adopt` | `dev` hex16, `fp` | | ticket |
| `skip`, `restore_to` | `seq` | | ticket |
| `restore_targets`, `purge_preview`, `purge` | | | |
| `revoke` | `dev`, `replace` | | ticket |
| `rules_override` | `version` | | ticket |
| `move_board` | `host`, `create_secret` hex32 | | ticket |
| `move_status`, `enter_move_code` | `text` | | |
| `follow_address` | `address`, `code` | | ticket |
| `wait_op` | `ticket`, `timeout_ms` | | the result |
| `create` | `relay`, `create_secret`, `source`: `"empty"`, `"this"`, or `{"folder": path}`, `name` | `begin_create` | |
| `sheet` | | `CreateFlow::sheet` or `RecoveryChangeFlow::sheet` | `{"sheet": text, "address": text, "recovery_fp": text}`: **test only**, the one place the seed leaves a flow |
| `confirm_sheet` | `text` | | |
| `finish_create`, `resume_create`, `abandon_create` | `create_secret` (resume) | | `{"address": …}` |
| `pair_start` | | | `{"code": …, "address": …}` (test only) |
| `pair_status`, `pair_confirm`, `pair_cancel` | | | `{"state": …, "sas"?: digits}` |
| `join`, `re_pair` | `address`, `code`, `name` | | |
| `join_sas` | `digits` | | |
| `begin_recovery_change`, `finish_recovery_change` | | | `{"seq": …}` (finish) |
| `confirm_current_sheet` | `text` | `RecoveryChangeFlow::confirm_current_sheet` | |
| `reset_trust` | `relay`, `create_secret`, `name` | | |
| `backup_now`, `backup_status`, `pin_fp` | `text` (pin) | | |
| `set_destinations` | `destinations` | | |
| `delete_backups_before` | `before_ms` | | |
| `set_picture` | `file` path | `set_shared_picture` | |
| `set_foreground` | `visible` | | |
| `resume` | | `notify_resume` | |
| `background_sync` | `budget_ms` | | the outcome |
| `fetch_now` | | the calendars and weather threads fetch at once | |
| `export_legacy` | `target` | | |
| `wait` | `since`, `timeout_ms` | `wait_for_change` | the counters |
| `clock` | `set_ms` or `advance_ms` | the manual clock | `{"wall_ms": …}` |
| `shutdown` | | `shutdown` | then exits with 0 |
| `kill` | | `std::process::abort()` at once | none |

- **DEV-CTL-001** (device) `taskdeck-devicectl` MUST exist only with the feature `test-trust`, and MUST answer every request of the table of §19.2 by exactly the facade call the table names.
- **DEV-CTL-002** (device) `taskdeck-devicectl` MUST write a seed or a pairing code to its output only in the answers of `sheet` and `pair_start`, and MUST write no token, key or create secret.

## 20. The end-to-end suite

### 20.1 Shape

`tests/e2e/` holds a Rust test harness that starts the standby relay (`relay/standby.js`, `node`, on `127.0.0.1` with a test certificate) or, in CI, the Worker under `wrangler dev`, then several `taskdeck-devicectl` processes, and drives them by §19.2. A **hostile proxy** of the harness may sit between the devices and the relay; it can, on command: drop one entry from a page; swap two entries of a page; serve an earlier page again for a later read; serve a different frame at one seq to one device (fork); answer every request from a copy of the relay's state taken earlier (roll back); serve the log without its newest `k` entries (truncate); refuse every request (outage); and answer 429 without `X-TD-Relay` (quota).

### 20.2 Scenarios

Each of V2's fifteen scenarios (`engine.md` §32) is a test; the numbers are V2's.

| Test | Devices | What it drives | It passes when |
|---|---|---|---|
| S1 offline phone | desk, laptop, phone (`android:31`) | the phone edits for three days of manual clock while disconnected, the laptop edits the same items | all three converge to one state hash; the notices and refusals of `engine.md` §32 S1 are recorded on the devices they concern |
| S2 concurrent block edits | desk, phone | concurrent `move_block`, `reflow`, `remove_block` on one day | converged; `skipped` notices on the author |
| S3 revocation offline | desk, laptop, phone | the desk revokes the phone while the laptop is offline; the laptop returns | the laptop applies the rotation; its old-epoch frames are accepted or re-sealed in order; the phone is told it was removed |
| S4 rollback, drop, reorder, fork | desk, laptop | each proxy fault in turn | each fault raises the alarm of §20.3 on every device it reached, nothing beyond the evidence is applied, and restore tail repairs the rollback |
| S5 bad snapshot | desk, laptop, phone | one device runs with the snapshot-corrupting hook (§11.1 item 8) and writes a snapshot of a wrong state | `SnapshotMismatch` on the others; the snapshot is never used |
| S6 version skew | desk, phone | the desk runs with the rules-2 wrapper (§11.1 item 8), the phone without it | the desk names the phone as the blocker; with an override the phone halts with `Rules` and resumes after an update |
| S7 clock skew | desk, laptop | the laptop's clock is moved by 10 minutes, then by 2 hours | warning, then `engine/clock_unconfirmed` until `confirm_clock` |
| S8 quota and outage | desk, phone | the proxy answers quota, then an outage for 48 hours of manual clock | local edits continue; "paused until" and "unreachable since" in status; no alarm; convergence after |
| S9 restored copy | desk, laptop | the laptop's data folder and keys are copied back to an earlier copy and restarted | no command applied twice, no id reissued (`engine.md` §32 S9) |
| S10 migration | desk | `create` from a legacy fixture folder (`fixtures/`), then pair a laptop and a phone | the genesis hash equals `import-check`'s; all three converge |
| S11 subscriptions | desk, phone | a local ICS server serves one feed that the panic hook (§11.1 item 8) makes the parser panic on, and one good feed | the bad feed is quarantined on each device, the good one's events are shown; no rename on the phone |
| S12 idempotency | desk | a device is killed after sending an append and before its answer, many times | every entry once in the log |
| S13 process death | phone | `kill` at random moments during edits and catch-up | §7.6 holds; no halt |
| S14 compaction | desk, laptop, phone | 400 days of manual clock with daily activity, the phone away 380 days | the phone rejoins after `confirm_rejoin`, with `continuity_unchecked` |
| S15 deletions | desk, laptop | `purge`, then `delete_backups_before` | every log and base truncated, the relay purged, backups deleted only after the call |

Besides these: pairing both ways (desktop as E, phone as N; and the phone as E at `android:30`), revocation with replacement, restore to a point, move board with the move code, reset trust, the recovery-key change with the current sheet, nightly backups and segments with retention over 400 days of manual clock, the recovery tool's top-up and fence against the standby, and "Export legacy" read back by `import_check`.

### 20.3 The hostile relay must always raise an alarm

| Proxy fault | Alarm (`engine.md` §21.1) |
|---|---|
| drop an entry from a page | `Withheld` |
| swap two entries | `Withheld` or `Fork` |
| replay an earlier page for a later read | none when the entries match the log (`engine.md` §12.3 step 5); `Fork` when they differ |
| fork one seq for one device | `Fork` |
| roll back | `RelayBehind` |
| truncate the newest `k` entries | `RelayBehind` |

- **DEV-E2E-001** (device) The end-to-end suite MUST contain one passing test per row of §20.2 against the standby relay, and in CI against the Worker under `wrangler dev`.
- **DEV-E2E-002** (device) For every proxy fault of §20.3 that changes what a device receives, the suite MUST show the alarm of that row raised on every device that received the fault, with nothing beyond the evidence persisted or applied.

### 20.4 Budgets and platform tests

- **Catch-up**: 4,200 entries (two weeks away, SYNC.md §4.7) read, made durable and applied within 3,000 ms on the CI Linux runner, against the standby on loopback, median of five runs (SYNC_PLAN.md §6.7 gate). This runner is the "reference hardware" of §17.1.
- **Fetchers**: a local HTTPS server (test certificate, `test-trust`) for redirects (3 allowed, the 4th refused, a cross-origin redirect without the query or credential, no `Referer`), every refused range of §13.1.4 through a test resolver, IP literals, an oversized body with and without `Content-Length`, a slow body (the 20 s limit), `304` with the window moved, error texts without the URL.
- **Key stores**: DPAPI on the Windows runner (a second user cannot unwrap: a test that runs `CryptUnprotectData` with another entropy and expects `CannotUnwrap`); the Keychain on the macOS runner with a temporary keychain.
- **Crash consistency**: §7.6 on Linux, Windows and macOS.

- **DEV-E2E-003** (device) Catch-up of 4,200 entries MUST complete within 3,000 ms as §20.4 measures it.

## 21. Error catalogue

Codes the device returns from facade calls, puts in the open report, in statuses and in overlay statuses. Codes of `core.md` §16, `seal.md` §22, `relay.md` §19 and `engine.md` §33 pass through unchanged where §17 says so.

| Code | Meaning | Raised when | What the caller does |
|---|---|---|---|
| `device/closed` | the facade was shut down | any call after `shutdown` | open again |
| `device/wrong_mode` | the call is not offered in this mode | §5.4, §17 | hide the control |
| `device/shutdown_timeout` | threads did not end within 10 s | §17.2 | exit the process |
| `device/folder_in_use` | another `Device` in this process holds the folder | DEV-THREAD-009 | use the open one |
| `device/folder_locked` | another process holds the folder's lock | §5.2, DEV-LOCK-002, DEV-LOCK-004 | show today's warning; close the other copy |
| `device/folder_unlockable` | the file system cannot lock | §5.2 | move the data folder to a local disk |
| `device/cloud_folder` | the data folder is cloud-synchronised | §5.3 | move the data folder |
| `device/store_descriptor` | `active.tds`, `pending.tds` or `store.tds` is invalid | §6.2 | report; offer `join` as a new device |
| `device/store_corrupt` | a required sealed file does not open | §7.4 | report; offer `join` as a new device |
| `device/record_format` | a record plaintext has the wrong layout | §6.3, §6.5 | as the file's row in §7.4 |
| `device/record_range` | an engine record holds an integer the encoding cannot carry | §6.5 | a local fault: report the bug |
| `device/keystore_unavailable` | the key store cannot answer now | §12.1 | retry later (unlock the keychain; wait for the shell) |
| `device/keystore_failed` | the key store failed otherwise | §12.1 | report |
| `device/no_keystore` | this platform has no key store in release builds | §12.2 | local-only mode only |
| `device/legacy_unreadable` | the plain files cannot be imported | §9.2, §18.1 | show the import report |
| `device/subscriptions_frozen` | `subscriptions.json` was unreadable at open | §9.2 | fix or remove the file, reopen |
| `device/needs_sync` | the call needs a synced board | §9.3, §17.12 | create or join a board |
| `device/bad_place` | `set_weather_place` with `Own` and no place, or a place out of range | §17.12 | the caller has a bug |
| `device/write_failed` | a plain-file write failed | §9.3, §18.7 | show it; the change was not made |
| `device/identity_active` | `forget_identity` named the active identity outside `KeyLost` mode | §17.2 | choose another identity |
| `device/insecure_url` | a URL is not `https` | DEV-NET-001, §13.1 | ask for an `https` address |
| `device/bad_answer` | a relay answer failed strict parsing | §10.5 | the engine's `RelayProtocol` alarm |
| `device/ics_address` | a feed's host or address is refused | §13.1.4 | shown as the feed's status |
| `device/ics_dns` | a feed's host does not resolve | §13.1.2 | shown as the feed's status |
| `device/ics_connect` | a feed's host cannot be reached | §13.1.2 | shown; retried next round |
| `device/ics_tls` | a feed's certificate was refused | §13.1.2 | shown |
| `device/ics_timeout` | a feed took more than 20 s | §13.1.2 | shown; retried |
| `device/ics_redirects` | a fourth redirect | §13.1.2 | shown |
| `device/ics_status` | a feed answered a status other than 200 or 304 | §13.1.2 | shown with the number |
| `device/ics_too_large` | a feed is above 8 MiB | §13.1.2 | shown |
| `device/ics_panic` | a feed's parse panicked; quarantined | §13.1.3 | shown until the URL changes |
| `device/picture_too_large` | an uploaded picture is above 6 MiB | §14.1 | choose a smaller file |
| `device/picture_format` | not a JPEG (or PNG for an upload) | §14.1, §14.2 | choose another file; for a synced one, shown as the picture status |
| `device/picture_decode` | the picture does not decode within the limits | §14.1, §14.2 | as above |
| `device/picture_blob` | the picture's blob is missing, incomplete or does not open | §14.2 | retried after an hour |
| `device/backup_not_desktop` | backups are written by desktops only | §15.1 | none |
| `device/backup_stopped` | the device is halted, stopped or not current | §15.1 | resolve the stop |
| `device/backup_history_incomplete` | the device lacks the control history from seq 1 | §15.1, §15.7 | another desktop writes backups |
| `device/backup_unpinned` | the recovery fingerprint is not pinned, or changed | §15.1, §15.5 | confirm the fingerprint from the sheet |
| `device/backup_no_destination` | no destination configured | §15.1 | add one |
| `device/backup_no_token` | no backup-read token yet | §15.1, §15.4 | connect once |
| `device/backup_failed` | the latest write to a destination failed | §15.6 | check the destination |
| `device/backup_stale` | no good nightly file for 48 hours | §15.6 | check the destinations |
| `device/destination_invalid` | a destination path is not usable | §15.6 | choose another |
| `device/fingerprint_mismatch` | the typed fingerprint is not the membership's recovery key | §15.5 | compare with the sheet again |
| `device/recovery_chain` | a top-up found a chain break | §15.8 | restore the backup as it is |
| `device/recovery_token` | the relay refused the recovery identity's read token twice | §15.8 | restore the backup as it is |
| `device/recovery_truncated` | a top-up cannot chain from the backup: the relay compacted or purged entries after it | §15.8 | restore the backup as it is, or a newer backup |
| `device/recovery_reinstate` | the recovery tool cannot reinstate this device: it was not revoked by a device, its revoking entry lies below the purge, 16 devices are current, more than 256 epochs were missed, a missed root cannot be obtained, or a second attempt failed | §15.8 | pair the device again, or restore to a new board; for a missing root, pick a newer backup |
| `device/not_on_this_platform` | the call is not offered on this platform | DEV-FACADE-007 | hide the control |
| `device/needs_api30` | Android below API 30 | DEV-FACADE-007 | use a desktop |
| `device/restore_target` | the seq is not an offered restore target | §17.8 | choose from `restore_targets` |
| `device/sheet_mismatch` | the retyped sheet is not the seed, or the current sheet typed for a recovery-key change is not the board's current recovery key | §18.1, §18.5 | retype |
| `device/sheet_test_failed` | the test backup did not open | §18.1 | report the bug; do not finish |
| `device/create_failed` | the relay refused `create`, or the genesis snapshot kept for it is damaged | §18.1 | show the relay code; for a damaged snapshot, abandon the create and start again |
| `device/pair_state` | a pairing call out of order | §17.10 | follow the screen |
| `device/set_aside_failed` | moving the plain files failed | §18.2, §18.4 | show it; for a join, nothing was activated |
| `device/close_failed` | the old board's `CloseBoard` failed | §18.6 | retry; the new board stays pending |
| `device/export_target` | the export folder is not empty, not absolute, or inside the data folder | §18.7 | choose another |
| `device/ctl_request` | a devicectl request line is invalid | §19.2 | a test bug |
| `device/torn_tail` | warning: a torn tail was truncated | §7.4 | none |
| `device/bad_tail` | warning: a last record did not open and was truncated | §7.4 | none; possibly disk damage |
| `device/quarantined` | warning: a damaged optional file was set aside | §7.4 | none |

## 22. Limits

| Name | Value | Unit | Enforced by |
|---|---|---|---|
| descriptors `active.tds`, `pending.tds`, `store.tds` | 45, 45, 54 | bytes exactly | DEV-STORE-004 |
| entries per log page record; any record plaintext | 1 to 256; 67,108,864 | entries; bytes | DEV-STORE-005 |
| log segment | 16,384 entries or 67,108,864 bytes | | DEV-STORE-003 |
| checkpoint `board`, `membership`, `registry` | 62,914,560; 2,097,152; 1,048,576 | bytes | DEV-STORE-005 |
| hash and notice ops per record | 1,024 | ops | DEV-STORE-005 |
| outbox ops per record; one `OutboxRecord` JSON | 1,024; 1,048,576 | ops; bytes | DEV-STORE-005 |
| notice or refusal record | 4,096 | bytes | DEV-STORE-005 |
| `meta.tdk` JSON | 4,194,304 | bytes | DEV-STORE-005 |
| record JSON nesting | 32 | levels | DEV-STORE-007 |
| identity roots; pending roots | 65,535; 16 | entries | DEV-STORE-005 |
| `overlay.tdk` | 16,777,216 | bytes | DEV-STORE-005 |
| feed body; ETag and `Last-Modified` | 8,388,608; 1,024 | bytes | DEV-ICS-004, DEV-STORE-005 |
| picture `clean` and `look` | 4,194,304 each | bytes | DEV-STORE-005 |
| weather report | 1,048,576 | bytes | DEV-STORE-005 |
| notes draft | 1,048,576 | bytes | DEV-STORE-005 |
| host (`device.tdk` `tokens`, `bootstrap.tdk`, backup `meta`) | 1 to 255 | bytes, `:port` included (`seal.md` §5.4, SEAL-ENC-006) | DEV-STORE-005 |
| `device.tdk` / `device.json` | 1,048,576 | bytes | DEV-SET-001 |
| quarantined file age before removal | 2,592,000,000 | ms | DEV-FILE-011 |
| Tick lateness while idle | 50 | ms | DEV-RUN-002 |
| backoff | 1,000 to `min(60,000, 1,000 × 2^(k − 1))` | ms | DEV-BACKOFF-001 |
| healthy session | 60,000 | ms after the welcome | DEV-BACKOFF-002 |
| client requests | 5,000 | per UTC day | DEV-BUDGET-001 |
| hello while over the budget | 1 per 900,000 | ms | DEV-BUDGET-001 |
| restart margin of the request count | 100 | requests | DEV-BUDGET-002 |
| TLS handshake | 10,000 | ms | DEV-NET-015 |
| HTTPS headers | 20,000 | ms | DEV-NET-015 |
| HTTPS body | 20,000 + ceil(bytes × 1,000 / 65,536) | ms | DEV-NET-015 |
| socket welcome | 20,000 | ms | DEV-NET-015 |
| socket ping; silence | 30,000; 75,000 | ms | DEV-NET-016, DEV-NET-015 |
| background run | 600,000 | ms at most | DEV-NET-014 |
| pairing slot poll | 2,000 | ms | DEV-PAIR-002 |
| ICS redirects | 3 | redirects | DEV-ICS-001 |
| ICS fetch | 20,000 | ms | DEV-ICS-004 |
| ICS cadence; Android background | 600,000; 21,600,000 | ms | §13.1.1 |
| subscription title adopted | 200 | scalars | DEV-ICS-008 |
| weather request; body | 10,000; 1,048,576 | ms; bytes | DEV-WX-001 |
| uploaded picture; the phone's own picture | 6,291,456 | bytes | DEV-PIC-003, DEV-PIC-005 |
| upload decode | 20,000 × 20,000 pixels, 160,000,000 bytes | | DEV-PIC-003 |
| synced picture | 4,194,304 bytes; 4,096 × 4,096 pixels; 67,108,864 bytes allocated | | DEV-PIC-001 |
| shared picture size | 720 × 1,560 | pixels | DEV-PIC-002 |
| picture retry | 3,600,000 | ms | §14.2 |
| backup destinations | 4 | destinations | DEV-BKP-008 |
| nightly kept; monthly kept; segments kept | 30; 12; 13 | files | DEV-BKP-008 |
| segment file | 209,715,200 | bytes | DEV-BKP-002 |
| backup stale | 172,800,000 | ms | DEV-BKP-010 |
| backup schedule | 300,000 after open, then every 600,000 | ms | DEV-BKP-002 |
| backup `meta` | 1,048,576 | bytes | DEV-BKP-004 |
| recovery read-token mints | 2 | tries (epoch 1, then the `read_epoch` of the refusal) | DEV-BKP-011 |
| wraps per reinstatement | 1 to 256 | wraps (epochs `first_epoch` to `E`) | DEV-BKP-014 |
| reinstatement attempts | 2 | tries (one more after an undo) | DEV-BKP-016 |
| notes save | 1 per 30,000 | ms while typing | DEV-FACADE-005 |
| `wait_for_change`, `wait_op` | 60,000 | ms at most | DEV-NOTIFY-003 |
| shutdown | 10,000 | ms | §17.2 |
| devicectl line | 16,777,216 | bytes | DEV-CTL-001 |
| catch-up of 4,200 entries | 3,000 | ms | DEV-E2E-003 |

## 23. Test vectors

Stage `device` publishes these families under `crates/taskdeck-device/tests/vectors/`, one JSON file each, in the format of README §7. Sealed-file bytes use `seal`'s published primitives with the entropy given as an input, as `seal.md` §24 does, so the bytes are deterministic.

| File | Spec | Inputs | Expected outputs |
|---|---|---|---|
| `device-descriptors.json` | §6.2 | board, dev, key-store byte, `created_ms` | `active.tds`, `pending.tds` and `store.tds` bytes; invalid ones and their code |
| `device-files.json` | §6.4, §7.2, §7.4 | `L_d`, board, dev, kind, `file_id`, record plaintexts, entropy | the file bytes for every kind 1 to 19; reads of each with a torn tail, a bad tail and a corrupt record, and the open outcome of §7.4 |
| `device-records.json` | §6.4, §6.5 | engine records as values (`LogRecord`, `Checkpoint`, `Registry`, `OutboxRecord`, `Meta`, `Marker`, `NoticeOp`, `HashRecord`), the identity export, a history record | plaintext bytes; malformed inputs and `device/record_format` |
| `device-loaded.json` | §7.4 | a directory listing with file contents (as hex), including leftover `.tmp` files and a `log_floor` | the `Loaded` value and the open report |
| `device-crash.json` | §7.6 | the crash workload, `k` | the write-boundary trace up to `k` and the recovered `Loaded` |
| `device-legacy-files.json` | §9.3, §18.7 | `BoardData` values, a zone | today's file bytes (`read_at_startup.json`, `archived.jsonl`, `notepad_text.json`, `subscriptions.json`, `colorschemes.json`, `userconfig.toml`) and the board that `legacy::import` gives back |
| `device-backoff.json` | §10.7 | `k`, seeded jitter draws | the wait in ms |
| `device-classify.json` | §10.6 | relay answers (status, `X-TD-Relay`, body), socket closes | the engine event |
| `device-addrguard.json` | §13.1.4 | hosts, resolved addresses (both families, mapped, compatible, NAT64, 6to4, Teredo) | allowed or refused, and the pinned address |
| `device-ics.json` | §13.1.2 | scripted server exchanges (redirect chains, `304`, sizes, statuses) | the requests sent (headers listed) and the outcome with its `why` |
| `device-weather.json` | §13.2 | home places | the request URL |
| `device-backup-names.json` | §15.3, §15.6 | writer tags, dates, seqs, a destination's file listing | file names; the files kept and deleted |
| `device-state.json` | §16 | device settings as values | `device.tdk` / `device.json` JSON bytes; invalid ones |
| `device-ctl.json` | §19.2 | request lines | answer lines, for the commands that need no relay |

## 24. Interfaces

**`seal.md`.**
- Requires: the file kinds 1 to 18 of `seal.md` §19.2, kind 1 being the segments `log-<first seq>.tdl` (§6.1), and kind 19 for the phone's own picture (ruling S29), with the matching `FileKind` variants of `seal.md` §26. Kinds 9 and 10 are `engine.md` §36's request; the others are this spec's.
- Relies on: `taskdeck_seal::verify_backup_file(bytes, writer_sign_pk) -> Result<BackupHeader, SealError>` (`seal.md` §21.6, §26, SEAL-BKP-009), which runs §21.6 steps 1, 2 and 5 only, so a writer checks what it wrote without the recovery key (§15.3.2).
- Requires: the sealer adapter of `taskdeck-device` composes the identity export of §6.4.7 from `DeviceKeys::to_identity_bytes`, the roots and the anchor; `seal.md` §19.2's description of kind 7 ("its private keys and the epoch roots it holds") is that layout. The adapter and the transport (§10) carry the duties `seal.md` §25 names for them (SEAL-FRAME-005, SEAL-ENV-003, SEAL-ENV-004, SEAL-SIG-005, SEAL-MEM-016, and the caller's halves of SEAL-WRAP-003, SEAL-SIG-002, SEAL-KEY-012 (each function of `seal.md` §26 that takes a `SymKey` is given the key of its own kind) and SEAL-KEY-013 (`derive_epoch_key` and `open_snapshot_declared` are given only roots checked against the commitment the membership records)), and the runtime carries its halves of SEAL-PAIR-008 (it sends `UserConfirmed` only when the user has confirmed on E) and SEAL-PAIR-013 (it feeds both pairing machines monotonic time and ticks, §18.3); stage `device` tests them.
- Assumes: `StorageKey::from_bytes` and `open_file`'s `FileTail { Torn, Bad }` with `valid_bytes` are what §7.4 uses; `sign_login` takes the origin from the caller (SEAL-SIG-002); `seal_frame` and `ctl_payload` with `Signer::Recovery` compose the recovery-authored `Fence` of §15.8; `open_blob` takes the writer as an argument (§14.2 step 3).
- Relies on: `taskdeck_seal::TEST_ENTROPY_MARKER` (`seal.md` SEAL-GEN-014), which a build with `test-entropy` contains and `release` searches for together with the `test-trust` marker (§11.2, DEV-TRUST-003).
- Provides: the contents, write and fsync order of every sealed file, torn and bad tails (§7), the key stores that wrap `L_d` with `ctx("local-key") ‖ board_id ‖ dev` (§12), history records kept for life (§8.3, DEV-FILE-011), backup scheduling, destinations, names and `meta` (§15), the pinned recovery fingerprint (§15.5), release builds without `test-entropy` (§11.2).

**`engine.md`.**
- Requires: `AdminOk.token` is a boolean, so the transport keeps the `register` answer's token string for the pairing machine (§18.3); the engine never sees it (`engine.md` §11.7).
- Requires: no single `AppendLog` op carries more than 256 records except as §6.4.1 splits them; `Loaded.session` is per process (§10.4).
- Assumes: `Engine::view()` and `Engine::confirmed()` are cheap enough to read once per published view (§4.4); `Engine::membership_bytes()` is `M_B`; `taskdeck_sync::state_hash` is the one composition.
- Provides: the runtime of `engine.md` §4 and §36: serial persists with `Persisted` in order (ENG-PERSIST-001, each op's syncs by DEV-FILE-004), other effects in order (ENG-EFFECT-001), every durable record at the next `Start` (ENG-PERSIST-004, DEV-CRASH-001), `Start` with every record (§7.4), a session id per process, the backoff, login and tokens, polling while visible, ticks (DEV-RUN-002), the compile step against the pending view with `at_ms` the compile clock and the minter in `origin_state` (DEV-FACADE-001, DEV-RUN-004), the minter raised from marks (§8.3), the history records (DEV-RUN-003), the pinned-fingerprint prompt after `RecoveryKeyChanged`, the purge's backup offer and the picture's re-upload (§8.4), the platform gates (DEV-FACADE-007), and the files kinds 9 and 10.
- Provides (ruling S24): a login at every `Connect` while the engine is stopped `Revoked`, and `MissedRoots { first_epoch, wraps }` right after each login whose answer carries `wraps` (§10.3; `engine.md` §24.4).

**`core.md`.**
- Relies on: `legacy::import` keeps every device-minted id (high part 1 to 8191, low part at least 1) unchanged and sets the mark of each high part it keeps (`core.md` §15.3 steps 9.4 and 14, CORE-IMP-005), so the plain files that local-only mode (§9.2), "Export legacy" (§18.7) and the recovery tool (§15.8) write read back through the import.
- Requires: `Overlay` and `weather::Report` keep `serde` derives so the store can write them (§6.4.10, §6.4.12). Relies on: `FetchStatus.at` is a `chrono::DateTime<chrono::FixedOffset>` (`core.md` §14.6), which the device fills as §13.1.2 step 8 states.
- Assumes: `compile` with `CompileCtx.device_no = 1` and an `IdMinter` for 1 works on a board whose marks hold the legacy space and the mark of `device_no` 1 that the import rebuilt from the plain files (§9.2); `Applied.changes` is complete (CORE-APPLY-003), so the local Store writes exactly what changed.
- Provides: the zone (`ChronoZone(chrono::Local)` on desktops, a `TableZone` on Android, §16), passed as `&dyn Zone` (`Published::zone()`, §25), which every zone parameter of the core accepts (`core.md` §20), the overlay to compile and read calls, the minter persisted with the outbox record (§8.3), the local-only `Store` over today's files (§9.3), checkpoints and bases loaded with `from_canonical_bytes` (DEV-FILE-010), notice and refusal records one per op in kind 8 (§6.4.5), and the `.lock` refusal for the import (DEV-LOCK-004).

**`relay.md`.**
- Requires: the routes, bodies and errors of `relay.md` §9 to §14 as written; the device sends exactly the requests of §10.5.
- Provides: the constant User-Agent, protocol pings every 30 s, a session id per process, uploads at 64 KiB/s or more (§10.10), polling on a relay without push, opaque tokens only in headers (DEV-NET-005), relay-address parsing before any request (RELAY-ADDR-002), the client budget (DEV-BUDGET-001), the classification of `relay.md` §14.5 (DEV-NET-011, §10.6), nothing deleted on an answer (DEV-NET-012), no socket to the standby (DEV-NET-008), the relay address (host and door) kept in the engine's relay target (`engine.md` §7, `meta.relay`) and a token per relay host in `device.tdk` (§16).
- Requires (ruling S24): the `login` answer's field `wraps`, present exactly while the device's row stores the wraps of a `reinstate` (`relay.md` §10.3, RELAY-LOGIN-006).

**`bridge.md`.**
- Requires: the bridge calls facade writes from worker threads (DEV-THREAD-008); learns of changes with `wait_for_change` (§17.3); answers the page's verbs with `submit_json` and renders the reply; answers `/api/state` with `answer_snapshot`; serves the picture's `look` bytes as `/bg-<id>.jpg`; implements `AndroidKeyStore` (§12.5), which writes `ld.wrap` itself and runs on the threads of §12.1 (BRIDGE-KEY-001, BRIDGE-KEY-004, BRIDGE-KEY-005); runs device authentication before "auth" calls (BRIDGE-AUTH-001); calls `set_zone` at start and on every zone change, `set_foreground` on `onStart` and `onStop`, `notify_resume` on a network change, and `background_sync` from its jobs; initialises the platform verifier and passes `TrustRoots` (§10.1).
- Provides: `set_own_picture`, `clear_own_picture` and `set_look_dials`, each returning the id of the look now shown (§14.3, §17.12); `NoticeView` with `seq`, `index` and `dismissed` (§17.5, §25); a `set_notes` from `submit_json` kept as the draft (DEV-FACADE-008); `weather_place()` and the report's source (§17.12, §25); `identities()` (§17.2); the mode changed in place (DEV-FACADE-009); Android starting out of the foreground (DEV-NET-017); the key-store threads (§12.1).

**Stage `recovery`.** Requires: `RecoveryLink` (§15.8, §25) for the online steps; the local-only writer (§9.3) to write a restored folder; `history.tdk`, the log segments and checkpoints in the layouts of §6.4 when the tool reads a device's own folder (DEV-STORE-005 names `recovery` as an owner). Requires (ruling S24, lead ruling L22): `reinstate` (`relay.md` §12.5, RELAY-REVOKE-004, RELAY-REVOKE-005), `revoke` signed by the recovery identity (`relay.md` §12.4), and `append` with the door for a frame of `dev_R` (`relay.md` §9.3, RELAY-SEQ-009); the wraps of `seal.md` §7 (SEAL-WRAP-001) and the `reinstate` arguments of `seal.md` §16.2 (SEAL-SIG-004). Provides: the backup writer and reader through `seal`; the offline replay of a top-up; the reinstatement of a device that a device revoked (§15.8 steps 6 to 11); the sheet's printed page.

**Stage `desktop`.** Requires: authentication before "auth" calls (DEV-FACADE-003); capture exclusion of the main window while a code or a seed is shown (DEV-PAIR-001); the waker (§17.3); `TaskDeck --import-check <folder>` printing `import_check`'s report and hash; printing the sheet from memory through the OS dialog; deleting the phone token after "Create board from this folder"; calling long calls off the UI thread.

**Stage `android`.** Requires: BIOMETRIC_STRONG before "auth" calls (BRIDGE-AUTH-001); the jobs that call `background_sync`; the native screens over `status()`; reminders from `Published::reminders`.

**Stage `release`.** Requires: DEV-TRUST-003 in the release workflow.

## 25. Public API

The `taskdeck-device` crate. `#![deny(unsafe_code)]` with the one exception of DEV-GEN-001. Signatures are Rust; the skeleton must compile with exactly these names. Types of other crates are named by their paths.

```rust
// ---------- opening (§5, §17.2)
pub struct Device { /* Arc<Inner>; Clone + Send + Sync */ }
pub enum Platform { Windows, MacOs, Linux, Android { api: u32 } }
pub enum ZoneSource { System, Table(taskdeck_core::time::TableZone) }
pub enum TrustRoots { Platform, Bundled, #[cfg(feature = "test-trust")] Extra(Vec<u8>) }
pub struct OpenOptions {
    pub data_dir: std::path::PathBuf, pub platform: Platform, pub key_store: std::sync::Arc<dyn KeyStore>,
    pub zone: ZoneSource, pub trust: TrustRoots, pub build_id: String, pub default_device_name: String,
    #[cfg(feature = "test-trust")] pub clock: Option<std::sync::Arc<dyn TestClock>>,
    #[cfg(feature = "test-trust")] pub files: Option<std::sync::Arc<dyn FileLayer>>,
}
pub enum Mode { LocalOnly, Synced, KeyLost }
pub struct IdentityInfo { pub board_id: [u8; 16], pub dev: [u8; 16], pub device_name: String,
                          pub relay_address: Option<String>, pub key_fp: Option<String> }
pub struct StoredIdentity { pub board_id: [u8; 16], pub dev: [u8; 16], pub key_store: u8, pub created_ms: i64,
                            pub active: bool }                                  // §17.2, from store.tds
pub enum OpenWarning { FolderLocked, FolderUnlockable, TornTail { kind: u8 }, BadTail { kind: u8 },
                       Quarantined { kind: u8, file: String }, SetAside { folder: String }, PendingCreate }
pub struct OpenReport { pub mode: Mode, pub warnings: Vec<OpenWarning>,
                        pub import: Option<taskdeck_core::legacy::ImportReport>, pub identity: Option<IdentityInfo> }
pub struct DeviceError { pub code: &'static str, pub status: Option<u16>, pub message: Option<String> }
impl Device {
    pub fn open(options: OpenOptions) -> Result<(Device, OpenReport), DeviceError>;
    pub fn mode(&self) -> Mode;
    pub fn shutdown(&self) -> Result<(), DeviceError>;
    pub fn forget_identity(&self, board: [u8; 16], dev: [u8; 16]) -> Result<(), DeviceError>;
    pub fn identities(&self) -> Vec<StoredIdentity>;                                  // read

    // ---------- change notification (§17.3)
    pub fn set_waker(&self, waker: std::sync::Arc<dyn Fn() + Send + Sync>);
    pub fn changes(&self) -> Changes;
    pub fn wait_for_change(&self, since_any: u64, timeout_ms: u32) -> Changes;

    // ---------- intents (§17.4)
    pub fn submit(&self, intent: taskdeck_core::intent::Intent) -> Result<Submitted, DeviceError>;
    pub fn submit_named(&self, intent: taskdeck_core::intent::Intent) -> Result<Submitted, DeviceError>;
    pub fn release_hold(&self) -> Result<(), DeviceError>;
    pub fn submit_json(&self, text: &str) -> Result<Submitted, DeviceError>;

    // ---------- reads (§17.5)
    pub fn view(&self) -> std::sync::Arc<Published>;
    pub fn status(&self) -> DeviceStatus;

    // ---------- notes (§17.6)
    pub fn notes_draft(&self) -> Option<Draft>;
    pub fn set_notes_draft(&self, text: String) -> Result<(), DeviceError>;
    pub fn flush_notes_draft(&self) -> Result<Option<Submitted>, DeviceError>;
    pub fn notes_merge(&self) -> Option<NotesMerge>;
    pub fn resolve_notes(&self, text: String) -> Result<Submitted, DeviceError>;
    pub fn discard_notes_draft(&self) -> Result<(), DeviceError>;

    // ---------- notices, banners, alarms, halted view (§17.7)
    pub fn dismiss_notice(&self, seq: u64, index: u32) -> Result<(), DeviceError>;
    pub fn ack_banner(&self, id: (u64, u8)) -> Result<(), DeviceError>;
    pub fn dismiss_alarm(&self, id: u64) -> Result<(), DeviceError>;
    pub fn confirm_clock(&self) -> Result<(), DeviceError>;
    pub fn confirm_rejoin(&self) -> Result<(), DeviceError>;
    pub fn ack_continuity(&self) -> Result<(), DeviceError>;
    pub fn ack_restored_copy(&self) -> Result<(), DeviceError>;
    pub fn ack_based_on_unchecked(&self) -> Result<(), DeviceError>;
    pub fn use_this_copy(&self) -> Result<(), DeviceError>;
    pub fn adopt(&self, dev: [u8; 16], typed_fp: &str) -> Result<u64, DeviceError>;               // ticket
    pub fn skip(&self, seq: u64) -> Result<u64, DeviceError>;                       // ticket

    // ---------- devices and operations (§17.8)
    pub fn revoke(&self, dev: [u8; 16], replace: bool) -> Result<u64, DeviceError>;
    pub fn restore_targets(&self) -> Vec<(u64, i64)>;
    pub fn restore_to(&self, seq: u64) -> Result<u64, DeviceError>;
    pub fn purge_preview(&self) -> Result<PurgePreview, DeviceError>;
    pub fn purge(&self) -> Result<u64, DeviceError>;
    pub fn rules_override(&self, version: u32) -> Result<u64, DeviceError>;
    pub fn move_board(&self, host: &str, create_secret: zeroize::Zeroizing<String>) -> Result<u64, DeviceError>;
    pub fn move_status(&self) -> MoveStatus;
    pub fn enter_move_code(&self, text: &str) -> Result<(), DeviceError>;
    pub fn follow_address(&self, address: &str, code: &str) -> Result<u64, DeviceError>;
    pub fn wait_op(&self, ticket: u64, timeout_ms: u32) -> Option<Result<(), &'static str>>;
    pub fn ops(&self) -> Vec<OpView>;

    // ---------- platform (§17.9)
    pub fn set_foreground(&self, visible: bool) -> Result<(), DeviceError>;
    pub fn notify_resume(&self) -> Result<(), DeviceError>;
    pub fn background_sync(&self, budget_ms: u32) -> Result<BackgroundOutcome, DeviceError>;
    pub fn set_zone(&self, zone: taskdeck_core::time::TableZone) -> Result<(), DeviceError>;

    // ---------- flows (§17.10, §18)
    pub fn begin_create(&self, request: CreateRequest) -> Result<CreateFlow, DeviceError>;
    pub fn resume_create(&self, create_secret: zeroize::Zeroizing<String>) -> Result<CreatedBoard, DeviceError>;
    pub fn abandon_create(&self) -> Result<(), DeviceError>;
    pub fn reset_trust(&self, request: CreateRequest) -> Result<CreateFlow, DeviceError>;
    pub fn pair_start(&self) -> Result<PairSheet, DeviceError>;
    pub fn pair_confirm(&self) -> Result<(), DeviceError>;
    pub fn pair_cancel(&self) -> Result<(), DeviceError>;
    pub fn join(&self, address: &str, code: zeroize::Zeroizing<String>, device_name: &str) -> Result<(), DeviceError>;
    pub fn join_sas(&self, digits: &str) -> Result<(), DeviceError>;
    pub fn re_pair(&self, address: &str, code: zeroize::Zeroizing<String>, device_name: &str) -> Result<(), DeviceError>;
    pub fn begin_recovery_change(&self) -> Result<RecoveryChangeFlow, DeviceError>;

    // ---------- backups (§17.11)
    pub fn backup_destinations(&self) -> Vec<Destination>;
    pub fn set_backup_destinations(&self, list: Vec<Destination>) -> Result<(), DeviceError>;
    pub fn backup_status(&self) -> BackupStatus;
    pub fn backup_now(&self) -> Result<(), DeviceError>;
    pub fn pin_recovery_fingerprint(&self, typed: &str) -> Result<(), DeviceError>;
    pub fn delete_backups_before(&self, before_ms: i64) -> Result<u32, DeviceError>;

    // ---------- picture, settings, export (§17.12)
    pub fn set_shared_picture(&self, bytes: Vec<u8>) -> Result<Submitted, DeviceError>;
    pub fn clear_shared_picture(&self) -> Result<Submitted, DeviceError>;
    pub fn set_look_dials(&self, blur: u8, light: u8) -> Result<Option<String>, DeviceError>;   // the id of the look shown
    pub fn set_own_picture(&self, bytes: Vec<u8>) -> Result<Option<String>, DeviceError>;   // §14.3, Android
    pub fn clear_own_picture(&self) -> Result<Option<String>, DeviceError>;
    pub fn weather_place(&self) -> (WeatherSource, Option<taskdeck_core::board::Home>);   // read
    pub fn set_weather_place(&self, source: WeatherSource, own_place: Option<taskdeck_core::board::Home>) -> Result<(), DeviceError>;
    pub fn set_reminder_leads(&self, leads: taskdeck_core::reminders::ReminderDefaults) -> Result<(), DeviceError>;
    pub fn export_legacy(&self, target: &std::path::Path) -> Result<(), DeviceError>;
}
pub fn import_check(folder: &std::path::Path) -> Result<ImportCheck, DeviceError>;
pub struct ImportCheck { pub report: taskdeck_core::legacy::ImportReport, pub genesis_sha256: String }

// ---------- change counters, results, views (§17.3 to §17.8)
pub struct Changes { pub any: u64, pub view: u64, pub status: u64, pub notices: u64, pub banners: u64,
                     pub overlay: u64, pub weather: u64, pub picture: u64, pub backup: u64,
                     pub pairing: u64, pub ops: u64 }
pub struct Submitted { pub reply: taskdeck_core::compile::Reply, pub entry_id: Option<[u8; 16]>,
                       pub provisional_seq: Option<u64>, pub view_version: u64 }
pub struct Published { /* §4.4 */ }
impl Published {
    pub fn board(&self) -> &taskdeck_core::board::BoardData;
    pub fn version(&self) -> u64;
    pub fn zone(&self) -> &dyn taskdeck_core::time::Zone;
    pub fn ranked_ids(&self, now: taskdeck_core::time::Instant) -> Vec<u64>;
    pub fn shuffle_seed(&self, now: taskdeck_core::time::Instant) -> u64;
    pub fn behind_minutes(&self, day: chrono::NaiveDate, now: taskdeck_core::time::Instant) -> i32;
    pub fn next_free_start(&self, day: chrono::NaiveDate) -> i32;
    pub fn reflow_inputs(&self, day: chrono::NaiveDate)
        -> (Vec<taskdeck_core::planner::Flowable<(u64, u64)>>, Vec<(i32, i32)>);
    pub fn placements_for(&self, item: u64, day: chrono::NaiveDate) -> Vec<taskdeck_core::planner::DayPlacement>;   // empty when item is not live
    pub fn archive_newest_first(&self) -> Vec<&taskdeck_core::board::ArchiveRow>;
    pub fn archive_by_day(&self, from: chrono::NaiveDate, days: u32)
        -> Vec<Vec<(&taskdeck_core::board::ArchiveRow, &taskdeck_core::board::RowBlock)>>;
    pub fn snapshot(&self, from: chrono::NaiveDate, days: u32, now: taskdeck_core::time::Instant,
                    ranked: Option<Vec<u64>>) -> taskdeck_core::snapshot::Snapshot;
    pub fn answer_snapshot(&self, from: chrono::NaiveDate, days: u32, now: taskdeck_core::time::Instant)
        -> serde_json::Value;
    pub fn reminders(&self, from: taskdeck_core::time::Instant, until: taskdeck_core::time::Instant)
        -> Vec<taskdeck_core::reminders::Reminder>;
    pub fn overlay(&self) -> &taskdeck_core::overlay::Overlay;
    pub fn weather(&self) -> Option<(&taskdeck_core::weather::Report, i64, WeatherSource)>;   // (report, fetched_ms, the place it is for)
    pub fn overlay_fetched(&self) -> &[(u64, i64)];   // (subscription id, wall_ms of its last good fetch), ascending id; a subscription with none is absent (§13.1.2 step 8)
    pub fn picture(&self) -> Option<PictureView>;
    pub fn pending_marks(&self) -> &[taskdeck_sync::PendingMark];
    pub fn notices(&self) -> &[NoticeView];
}
pub struct PictureView { pub id: String, pub jpeg: std::sync::Arc<Vec<u8>>, pub top: [u8; 3], pub aspect_milli: u32 }
pub struct NoticeView { pub seq: u64, pub index: u32, pub dismissed: bool, pub record: NoticeRecord }   // engine.md NoticeEntry
pub enum NoticeRecord { Notice(taskdeck_core::apply::Notice), Refusal(taskdeck_core::apply::RefusalRecord) }
pub struct DeviceStatus { pub engine: Option<taskdeck_sync::Status>, pub mode: Mode, pub relay_address: Option<String>,
                          pub socket_open: bool, pub requests_today: u32, pub weather_age_ms: Option<i64>,
                          pub picture: PictureStatus, pub backup: BackupStatus, pub pairing: PairState,
                          pub device_name: String }
pub enum PictureStatus { None, Ready, Failed { code: &'static str } }
pub struct Draft { pub text: String, pub base_rev: u64, pub edited_ms: i64 }
pub struct NotesMerge { pub mine: String, pub theirs: String, pub base_rev: u64, pub current_rev: u64 }
pub struct PurgePreview { pub reach_seq: u64, pub reach_ms: i64 }
pub struct MoveStatus { pub code: Option<String>, pub carried_through: Option<u64>, pub old_relay_answered: Option<bool> }
pub struct OpView { pub ticket: u64, pub kind: &'static str, pub stage: u8, pub started_ms: i64 }
pub struct BackgroundOutcome { pub synced: bool, pub queued: u32, pub head: u64 }
pub enum WeatherSource { Home, Own }

// ---------- flows (§18)
pub enum CreateSource { Empty, ThisFolder, Folder(std::path::PathBuf), CurrentBoard }
pub struct CreateRequest { pub relay_host: String, pub create_secret: zeroize::Zeroizing<String>,
                           pub source: CreateSource, pub device_name: String }
pub struct CreateFlow { /* holds the seed, keys and genesis; ZeroizeOnDrop */ }
impl CreateFlow {
    pub fn sheet(&self) -> SheetData;
    pub fn import_report(&self) -> Option<&taskdeck_core::legacy::ImportReport>;
    pub fn confirm_sheet(&mut self, typed: &str) -> Result<(), DeviceError>;
    pub fn finish(self) -> Result<CreatedBoard, DeviceError>;
}
pub struct SheetData { pub seed_groups: zeroize::Zeroizing<String>, pub relay_address: String,
                       pub recovery_fp: String, pub release_fp: String, pub date: chrono::NaiveDate }
pub struct CreatedBoard { pub address: String }
pub struct RecoveryChangeFlow { /* ZeroizeOnDrop */ }
impl RecoveryChangeFlow {
    pub fn confirm_current_sheet(&mut self, typed: &str) -> Result<(), DeviceError>;
    pub fn sheet(&self) -> SheetData;
    pub fn confirm_sheet(&mut self, typed: &str) -> Result<(), DeviceError>;
    pub fn finish(self) -> Result<u64, DeviceError>;                                 // seq of the accepted K-shape entry
}
pub struct PairSheet { pub address_display: String, pub code_display: zeroize::Zeroizing<String> }
pub enum PairState { Idle, ShowingCode, ShowSas { digits: String }, Registering, AskSas, Accepted,
                     Joining, Done, Aborted { code: &'static str } }

// ---------- backups (§15)
pub enum DestinationKind { Folder, Offline }
pub struct Destination { pub path: std::path::PathBuf, pub kind: DestinationKind, pub label: String }
pub struct DestinationStatus { pub path: std::path::PathBuf, pub last_good_ms: Option<i64>, pub last_attempt_ms: Option<i64>,
                               pub last_error: Option<&'static str>, pub files: u32, pub bytes: u64 }
pub struct BackupStatus { pub off_reason: Option<&'static str>, pub destinations: Vec<DestinationStatus>,
                          pub last_nightly_day: Option<chrono::NaiveDate>, pub last_segment: Option<(String, u64)>,
                          pub alarms: Vec<&'static str>, pub pin_needed: Option<String> }

// ---------- key stores (§12)
pub trait KeyStore: Send + Sync {
    fn kind(&self) -> u8;
    fn create(&self, board: &[u8; 16], dev: &[u8; 16], key: &zeroize::Zeroizing<[u8; 32]>) -> Result<(), KeyStoreError>;
    fn load(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<zeroize::Zeroizing<[u8; 32]>, KeyStoreError>;
    fn delete(&self, board: &[u8; 16], dev: &[u8; 16]) -> Result<(), KeyStoreError>;
}
pub enum KeyStoreError { NotFound, CannotUnwrap, Unavailable, Failed }
#[cfg(windows)] pub struct DpapiKeyStore;
#[cfg(windows)] impl KeyStore for DpapiKeyStore {}
#[cfg(target_os = "macos")] pub struct KeychainKeyStore;
#[cfg(target_os = "macos")] impl KeyStore for KeychainKeyStore {}
#[cfg(feature = "test-trust")] pub struct FileKeyStore { pub dir: std::path::PathBuf }
#[cfg(feature = "test-trust")] impl KeyStore for FileKeyStore {}
#[cfg(feature = "test-trust")] pub struct MemoryKeyStore { /* map */ }
#[cfg(feature = "test-trust")] impl KeyStore for MemoryKeyStore {}
// AndroidKeyStore lives in taskdeck-android (§12.5) and implements KeyStore there (bridge.md §21).

// ---------- the store and its file layer (§6, §7)
pub trait FileLayer: Send + Sync {
    fn create_dir(&self, path: &std::path::Path) -> std::io::Result<()>;
    fn write_new(&self, path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()>;
    fn append(&self, path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()>;
    fn sync(&self, path: &std::path::Path) -> std::io::Result<()>;
    fn rename(&self, from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()>;
    fn remove(&self, path: &std::path::Path) -> std::io::Result<()>;
    fn truncate(&self, path: &std::path::Path, len: u64) -> std::io::Result<()>;
    fn sync_dir(&self, dir: &std::path::Path) -> std::io::Result<()>;
    fn read(&self, path: &std::path::Path) -> std::io::Result<Vec<u8>>;
    fn list(&self, dir: &std::path::Path) -> std::io::Result<Vec<std::ffi::OsString>>;
    fn exists(&self, path: &std::path::Path) -> bool;
}
pub struct OsFiles;
impl FileLayer for OsFiles {}
#[cfg(feature = "test-trust")] pub struct CrashFiles { pub inner: OsFiles, pub crash_at: u64 }
#[cfg(feature = "test-trust")] impl FileLayer for CrashFiles {}
pub mod store {
    // The file kinds of §6.1 are taskdeck_seal::FileKind (seal.md §19.2, §26): the store passes those variants to
    // FileSealer::create and open_file. These constants are their numbers, never defined a second time.
    use taskdeck_seal::FileKind;
    pub const KIND_META: u8 = FileKind::Meta as u8;           pub const KIND_MARKER: u8 = FileKind::Marker as u8;
    pub const KIND_HISTORY: u8 = FileKind::History as u8;     pub const KIND_DEVICE: u8 = FileKind::Device as u8;
    pub const KIND_PICTURE: u8 = FileKind::Picture as u8;     pub const KIND_WEATHER: u8 = FileKind::Weather as u8;
    pub const KIND_DRAFT: u8 = FileKind::Draft as u8;         pub const KIND_FEED: u8 = FileKind::Feed as u8;
    pub const KIND_BOOTSTRAP: u8 = FileKind::Bootstrap as u8; pub const KIND_UNSENT: u8 = FileKind::Unsent as u8;
    pub const KIND_OWN_PICTURE: u8 = FileKind::OwnPicture as u8;
    pub fn encode_log_page(records: &[taskdeck_sync::LogRecord]) -> Vec<u8>;              // §6.4.1
    pub fn decode_log_page(bytes: &[u8]) -> Result<Vec<taskdeck_sync::LogRecord>, super::DeviceError>;
    pub fn encode_checkpoint(c: &taskdeck_sync::Checkpoint) -> Result<Vec<u8>, super::DeviceError>;   // §6.4.2
    pub fn decode_checkpoint(bytes: &[u8]) -> Result<taskdeck_sync::Checkpoint, super::DeviceError>;
    pub fn record_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, super::DeviceError>;        // §6.5
    pub fn from_record_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, super::DeviceError>;
    pub fn read_identity_dir(dir: &std::path::Path, l_d: &zeroize::Zeroizing<[u8; 32]>)
        -> Result<(taskdeck_sync::Loaded, Vec<taskdeck_seal::HistoryRecord>), super::DeviceError>;   // Loaded, history records
    pub fn write_legacy_files(dir: &std::path::Path, board: &taskdeck_core::board::BoardData,
                              zone: &dyn taskdeck_core::time::Zone) -> Result<(), super::DeviceError>;   // §9.3, §18.7
}

// ---------- adapters (decision D6, §4.1)
pub struct CoreMachine { /* over taskdeck-core */ }
pub struct SealSealer { /* over taskdeck-seal */ }
// impl taskdeck_sync::StateMachine for CoreMachine and impl taskdeck_sync::Sealer for SealSealer are stage device's
// to write; the skeleton carries the two structs without them.

// ---------- recovery tool online steps (§15.8)
pub mod recovery_online {
    pub struct RecoveryLink { /* transport to one relay, door credential */ }
    pub enum FenceAnswer { Fenced { seq: u64, chain: [u8; 32] },             // the Fence is sequenced
                           Counter { last_ctr: u64, last_seq: u64 } }        // 409 relay/gap or relay/conflict (§15.8 step 5)
    pub enum AppendAnswer { Stored { seq: u64, chain: [u8; 32] },            // new or dup (§15.8 step 9)
                            Gap { last_ctr: u64, last_seq: u64 },            // re-seal once (§15.8 step 9)
                            Conflict { seq: u64 } }                          // §15.8 step 11
    impl RecoveryLink {
        pub fn new(address: &str, recovery: &taskdeck_seal::RecoveryKeys, trust: super::TrustRoots)
            -> Result<Self, super::DeviceError>;
        pub fn read_token(&mut self, backup_token: Option<&str>) -> Result<(), super::DeviceError>;   // §15.8 step 1: the backup's token, else epoch 1, then the refusal's read_epoch
        pub fn read_after(&mut self, after: u64) -> Result<taskdeck_sync::Page, super::DeviceError>;
        pub fn fence(&mut self, frame: Vec<u8>, host: &str, door: [u8; 5], home_gen: u32)
            -> Result<FenceAnswer, super::DeviceError>;
        pub fn reinstate(&mut self, dev: [u8; 16], first_epoch: u32, wraps: Vec<[u8; 96]>)
            -> Result<(), super::DeviceError>;                               // §15.8 step 8: admin-nonce, then reinstate, with the door
        pub fn append(&mut self, frame: Vec<u8>) -> Result<AppendAnswer, super::DeviceError>;   // §15.8 step 9: one frame of dev_R with the door; a refused result is the error
        pub fn revoke(&mut self, dev: [u8; 16]) -> Result<(), super::DeviceError>;              // §15.8 step 11: admin-nonce, then revoke, with the door
    }
}

// ---------- test-only (§11)
#[cfg(feature = "test-trust")] pub const TEST_TRUST_MARKER: &str = "taskdeck-test-trust-enabled-6f1c2a";
#[cfg(feature = "test-trust")] pub trait TestClock: Send + Sync { fn wall_ms(&self) -> i64; fn mono_ms(&self) -> u64; }
```

## 26. Open questions

Non-normative. Each states the default this spec takes.

1. **`unsafe` on Windows.** Ruled S26: `unsafe` is allowed in platform FFI and only there; in `taskdeck-device` that is the module `platform::windows`, four functions, each block commented (DEV-GEN-001).
2. **Local-only mode writes today's files** and mints with `device_no` 1 (§9), so that today's build keeps opening the folder and "Create board from this folder" reads one format. The import keeps such ids and sets their mark (`core.md` §15.3 steps 9.4 and 14), as the lead noted under the device draft's rulings (`docs/sync/status/foundation.md`). Decided there.
3. **The log is split into segments** of 16,384 entries (§6.1). Decided here; SYNC.md §4.1 names one `log.tdl`, which a 400-day truncation would have to rewrite whole.
4. **Backup file names carry a writer tag** (§15.3). Ruled S28, so that two desktops writing to one synced folder never overwrite each other's files.
5. **Segments end at bases** (§15.2), so a segment's membership and chain come from a base file without replay. Decided here (SYNC.md: "that month's frames").
6. **A device that joined by pairing receives the control history** in its bootstrap snapshot (ruling S27, `seal.md` §20.4), so any desktop can write backups however it joined (SYNC.md §6.6). Decided there.
7. **The recovery tool's read-token epoch.** A recovery identity cannot read before it mints, so it asks with epoch 1 and, when refused, with the `read_epoch` the relay's `relay/stale_epoch` refusal carries (`relay.md` §4.2; the lead's note under ruling S28). Decided there.
8. **A crash on E while registering a pairing** leaves a `DeviceAdded` for a device that never joins; it appears with no last beat and the user revokes it (§18.3). Decided here (SYNC.md is silent); persisting the pairing machine would keep its secrets on disk.
9. **The `507 relay/storage_refused` pause** is passed to the engine as a `RelayRefused` with `until` (§10.6), which `engine.md` §11.9 handles as a pause of writes until `until`, with reads going on and no alarm. Decided there.
10. **Polling while visible on a relay without push** is the engine's 30-second `hello`, also on a desktop that runs all day: about 2,880 requests, inside the 5,000 budget. Decided here (ruling S9 leaves the rate to the device).
11. **The client's request count is written every 100 requests** and restarts with a margin of 100 (§10.8), rather than on every request. Decided here.
12. **User-Agent strings** are `TaskDeck-Sync/1`, `TaskDeck-Calendar/1` and `TaskDeck-Weather/1`, with no version (§10.2). Decided here (SYNC.md: "constant, descriptive"); today's ICS agent `TaskDeck/<version>` is dropped because it told publishers which build each device ran.
13. **ICS userinfo** is sent as Basic credentials to the URL's own origin only (§13.1.2). Decided here, keeping today's behaviour for such feeds while closing the cross-origin leak.
14. **The address guard refuses the whole fetch** when any resolved address is private, rather than filtering (§13.1.4). Decided here, against DNS answers that mix addresses.
15. **The shared picture's clean copy is 720 × 1560 JPEG at quality 85**, and each device applies its own look (§14). Decided here; today's phone made one look on the desk.
16. **Linux has no key store** in release builds, so a Linux desktop runs local-only (§12.2). Decided here: SYNC.md names no Linux client; CI's Linux desktop build keeps working.
17. **The phone fetches feeds and weather in background runs only when stale** (6 hours, 1 hour, §13). Decided here, to keep jobs short.
18. **Reminder leads are a device setting** with the defaults of ruling S19 (§16). Decided here; their screen is `android`'s.
19. **The notes draft survives a restart** in `draft.tdk` (§17.6). Decided here (SYNC.md: "saves coalesce to one per 30 s").
20. **Weather in local-only mode is not stored**, as today. Decided here.
21. **The time zone is never stored** (§16): desktops read the operating system's at each call, the phone's shell hands a `TableZone` over. Decided here (`core.md` open question 20 leaves the choice to this spec).
22. **devicectl's platform `linux` passes every desktop gate** (§19.1), so the whole desktop surface can be driven in a container. Decided here; the gates themselves are unit-tested with each platform value.
23. **A folder over a cap of rules version 1 does not open in local-only mode** (§9.2 step 2). Local-only mode runs the genesis import at every `open`, and the import refuses a board over a cap or field range of `core.md` §17 (ruling S18), for example more than 10,000 live items or a name over 4,096 bytes. Default: `open` fails with `device/legacy_unreadable` and the report, which names the ids, and the user repairs them in today's build. The alternative, a lenient import that only reports such violations for local-only use, is with the lead.
24. **A link slower than 64 KiB/s cannot upload a 1 MiB part** (`relay.md` §6.2): the relay answers `408 relay/slow`, which the transport treats as unreachable and sends again after the backoff (§10.6), so such an upload never completes over that link. Uploads at 64 KiB/s or more stay a client requirement (§10.10, §24); the owner's rehearsal measures real phone links (SYNC.md §11 "Still to measure"). Default kept.
25. **The reinstating entry's counter can be too low** (§15.8 step 9). The tool takes the recovery identity's counter from the backup's history and the entries it read. A frame of `dev_R` that clients refused is in neither, and after a purge it is not on the relay either. An `append` answers a counter that is too low with `conflict`, which carries no `last_ctr` (`relay.md` §9.6). Default: the tool undoes (step 11) and ends with the relay's code, and the owner pairs the device again.

