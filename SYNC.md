# TaskDeck on Windows, macOS and Android over a free blind relay: final design

*2026-11-20. The final output of a design workflow: research reports R1–R4, designs D1–D4, critiques C1–C3 and verifiers V1 (security), V2 (sync integrity) and V3 (requirements and feasibility). Nothing here is built yet. **[V]** verified (file:line in the `day-planner` worktree, command output, or a page fetched today); **[A]** assumed; **[U]** uncertain. "DOC §n" is DOCUMENTATION.md on `day-planner`; "V2-3" is verifier 2's issue 3, listed under Verification outcomes.*

> **Building it:** [`SYNC_PLAN.md`](SYNC_PLAN.md) is the plan of record. It goes straight to the end state: there is no M1 hub milestone, no device is assumed to be always on, the `.ics` feed is out of scope, and the effort figures below, counted in unassisted hours, no longer apply. Where the two documents differ, the plan wins; its §2 lists every difference. The full research and verifier reports are in [`docs/sync/research/`](docs/sync/research/).

## Executive summary

**What you get.** Every device keeps the whole board and works fully offline. Windows and the MacBook run today's app; Android gets a small native app around today's phone page, which now applies edits at once, even offline. Devices sync through a free Cloudflare relay that stores only encrypted, signed entries. It can delay sync, but it cannot read your data, alter it unnoticed, or send code to any device. No Tailscale, no server of your own.

**Cost.** EUR 0 a month. Optional: a domain (about EUR 10 a year) against flooding, and a hardware signing key (about EUR 50).

**Uptime and speed.** Viewing and editing never wait for the network. Between running devices a change arrives in under a second. The phone is current about two seconds after opening, and within roughly 30–60 minutes in the background (to be measured). Expect about 9–26 hours a year of delayed sync from Cloudflare incidents, and a pause until midnight UTC if someone floods the relay. Nothing is lost either way.

**What stays risky.** A device that is infected, or stolen while unlocked, sees everything until you revoke it from another device. A poisoned dependency could slip past the checks. Cloudflare and calendar hosts see IP addresses and timing. Losing every device and both paper recovery sheets loses the data, by design. The encryption code is standard and heavily tested but has no paid audit.

**How long.** About 15–19 focused 30-hour weeks to M1: all three devices on the relay, encrypted, without Tailscale, though edits apply only while the Windows desktop runs. About 33–42 to M2, where every device works without the desktop. At 30 hours a week, M1 lands around March–May 2027 and M2 around July–December 2027, the later dates including 30% contingency; at 15 hours a week, about twice as long. Upkeep afterwards: about 6–10 hours a month.

**First three steps.**
1. Phase 0, about a week: refresh the lockfile to close the TLS-trust advisories, stop calendar links leaking through redirects and error text, turn on passkeys and disk encryption, and make a dated encrypted backup.
2. Stop building or opening TaskDeck's source in the Windows account that runs the wall calendar; use a cloud session, a VM or a separate account.
3. Answer the questions that shape M1: your phone and Android version, whether Google Calendar subscribes to the feed today, and whether the desktop stays on overnight.

## 1. Summary and principles

Every device holds the whole board and applies one sequenced command log, encrypted end to end, which one SQLite-backed Cloudflare Durable Object (DO) on Workers Free orders, stores and pushes at EUR 0. Windows and macOS keep the egui app; Android gets a Kotlin shell around a WebView with no network access of its own, showing the unchanged `phone.html` over the same Rust core. The first milestone, **M1**, is a relay hub: the Windows desktop stays the only board owner, reached by the Mac and the phone through the encrypted relay. **M2** makes every device a full replica.

1. Local first: viewing and editing never wait for the network.
2. One sequencer; commands are the only mutation; `apply` is pure and its rules are versioned.
3. Nothing serves code, nothing listens, nothing updates itself.
4. Authenticate before parsing; every signature names its use; clients verify the relay.
5. Every change to membership, keys or history is signed, shown on every device, and can be overruled with the paper recovery key.
6. Fail closed, never bill; every failure is visible, every drill rehearsed.
7. Few, boring, pinned dependencies that one person can maintain.

## 2. Architecture

### 2.1 Components

| Where | Runs | Secrets |
|---|---|---|
| **Windows desktop** (wall) | `TaskDeck.exe`: the egui UI unchanged (DOC §14), core, sync engine on one blocking thread, ICS and weather fetchers; snapshot and backup duty; the M1 hub | DPAPI-wrapped storage key |
| **macOS laptop** | the same source for `aarch64-apple-darwin`, as `TaskDeck.app` with one stable self-signed identity; second backup writer | login-Keychain item |
| **Android phone** | one process: Kotlin shell; WebView with the bundled `phone.html`; `libtaskdeck.so` over JNI; JobScheduler jobs; exact alarms (Phase 6); native Pair, Devices and Health screens | Keystore AES-256-GCM key, StrongBox if present |
| **Cloudflare Workers Free** | Worker `relay`, ≈ 900 lines of dependency-free JavaScript [A]: token check, caps, routing. One SQLite DO per board: verify, sequence, store, push. Nothing outbound | `TOKEN_KEY`; `CREATE_SECRET`, deleted after creation |
| **Elsewhere** | GitHub (source, secret-free CI, attestations); backup folders, a second disk and a USB stick; two paper sheets; a build account or VM; a touch-to-sign token for the APK key | — |

Devices talk only to the relay (wss, or https from background jobs), ICS hosts and Open-Meteo. Android ships `kotlin-stdlib` and `androidx.webkit` 1.18.0 (with its androidx.core dependencies) and nothing else: no Firebase, Play services, WorkManager, CameraX or Compose. Permissions: `INTERNET`, `POST_NOTIFICATIONS`, `USE_EXACT_ALARM` (plus `SCHEDULE_EXACT_ALARM` with `maxSdkVersion="32"`), `RECEIVE_BOOT_COMPLETED`, `USE_BIOMETRIC`; coarse location on demand; calendar access only for the optional export.

Retired at M2: Tailscale, `taskdeck-server`, the tiny_http phone server and its stalled-body freeze, `/calendar.ics`, token links and QR codes, the service worker, temporary-id remapping, the AVIF encoder.

### 2.2 Data flow

```
 Windows (wall)             macOS                  Android
 egui · core · sync         same code              WebView (bundled page, no network)
 sealed log, checkpoints    sealed store           bridge ─ Kotlin ─ JNI ─ core + sync
        │ wss/https                │ wss/https              │ wss (visible) / https (jobs)
        ▼                          ▼                        ▼
 ┌────────────────────────────────────────────────────────────────────────┐
 │ Worker "relay": HMAC token gate · caps · no HTML, CORS or logs         │
 │  └► Durable Object "Board" (SQLite): verify → seq++ → store → push     │
 │  Sees IPs, times, padded sizes, device count. Never keys, text, code.  │
 └────────────────────────────────────────────────────────────────────────┘
```

During M1 the relay also carries sealed requests and replies between the other devices and the Windows hub.

### 2.3 Shared code and packaging

- **`crates/taskdeck-core`** (new; serde, serde_json, chrono 0.4.45): `BoardData`, log commands, the pure versioned `apply`, planner, archive, scoring, `ics.rs`, overlay types, the snapshot builders from `phone.rs`.
- **`crates/taskdeck-seal`** (new): frames, contexts, envelope, keys, chain, pairing, HPKE, backups; `SPEC.md` and test vectors.
- **`task_deck`** (today's package): compile step, sans-IO sync engine, sealed store, transport (ureq 3.5.2; tungstenite 0.30.0 over rustls 0.23.46; rustls-platform-verifier 0.7.1), fetchers, key stores, the egui UI, `taskdeck-recover`.
- **`crates/taskdeck-android`** (new; jni 0.22.4): `libtaskdeck.so` over `task_deck` with `--no-default-features` (V3-17), checked by `cargo ndk check` in CI.
- **`relay/`**: `relay-core.js` behind a storage interface, `worker.js`, `standby.js` (Node 22, `node:sqlite`), a `node:test` suite. **`android/`**: Gradle, copying `src/phone.html` at build time.

## 3. Feature parity

| Feature | Windows, Mac | Android | How |
|---|---|---|---|
| Wall layout, UI scale, uncapped loop, idle wind-down (DOC §14) | ✓ | agenda, as today | egui unchanged |
| Ranked tasks, daily tie-break shuffle (§14.4) | ✓ | Tray → All | core scoring per device, never stored |
| Calendar grid, agenda, month rail, glance line | ✓ | ✓ | today's UIs over the local replica |
| New task or event, quick add | ✓ | New sheet, tray quick-add | device-minted ids (§4.4) |
| Planner (drag, resize, ＋Block, Unplan), estimates, due dates, routines, Reflow | ✓ | day view: tap, hold-to-drag, chips | times compiled to UTC at the origin (§4.5) |
| Done / Book / Tomorrow with Undo | planner equivalents | ✓ | Undo is an inverse command (phone.html:4876-4949 [V]) |
| Archive: ledger, stats, search, Restore, Forget; ghosts | ✓ | Done tab, Restore, ghosts; search and Forget in Phase 8 (V3-19) | in every replica |
| Notepad | ✓ | ✓ | `SetNotes{base}`, draft and merge view (§4.6) |
| Weather, location | ✓ | ✓; the phone's own place never syncs | each device asks Open-Meteo about the home place (a board setting, 0.01°) |
| Palettes; backgrounds and Look | ✓, per device | the desk's palette, its own Look, the desk's picture as one sealed blob | `SetPalette` board setting |
| Subscribed ICS overlay; managing subscriptions | ✓ | overlay ✓; a native management screen in Phase 8, never via the WebView | each device fetches with ETag, keeping the raw body (V2-22); the overlay never enters `apply` |
| Offline editing | ✓ | ✓, applied at once (today "waiting", phone.html:1465-1481 [V]) | confirmed ⊕ pending (§4.7) |
| `/calendar.ics` feed | "Export .ics…" (Phase 5a) | system-calendar export (Phase 5b, if open question 2 says it is used) | a blind relay cannot serve a URL; a live Google Calendar subscription is lost (V3-3) |
| **New:** pairing, devices, revocation, sync status, last seen, fingerprint, banners, recovery, backups, restore to a point | ✓ | ✓ (no backups) | §4–§6 |
| **New:** offline reminders with Done/Tomorrow/Snooze | — | Phase 6 | core `reminders()`, exact alarms |
| **New, optional:** widgets, tile, share target, app lock | — | Phase 8 | |

## 4. Sync protocol (end state, M2)

### 4.1 Storage

On the relay (DO SQLite), an append writes three rows: the log row, its unique-index entry and the device row.

```
log(seq INTEGER PRIMARY KEY, dev, ctr, hash, at, frame, UNIQUE(dev, ctr))
devices(dev PRIMARY KEY, sig_pk, role, gen, last_ctr, state)   -- role: device or recovery
snapshots(id PRIMARY KEY, seq, author, at)
blobs(id, part, data, PRIMARY KEY(id, part))                   -- parts ≤ 1 MiB
tickets(tid PRIMARY KEY, expires, tries, slots)
meta(k PRIMARY KEY, v)                                          -- generation, fence
```

Devices keep sealed files in `taskdeck_data/sync/` (app-private storage on Android), with associated data (AD) = file kind ‖ dev ‖ board_id (V1-12): `log.tdl` (frames with seq and chain value, fsynced per page before applying; 400 days); `checkpoint.tdk` (`BoardData`, head, counters, device set, epoch roots; every 200 entries, every 10 minutes and at exit) plus weekly `base-<seq>.tdk` files kept 400 days (V2-9, V2-12); `hashes.tdk` (the state hash at every seq ≡ 0 mod 64); `outbox.tdo` (`{entry_id, at, based_on, command, ctr?, frame?}`, fsynced before `apply` returns; today commands are filed only between drains, sync.rs:648-660 [V]); and `overlay.tdk` (overlay, raw feed bodies, ETags). A replica loads `BoardData` exactly as stored; normalisation runs only in import-check and genesis (V2-19).

### 4.2 Frame, envelope and signatures

```
Frame:   "TDF1" | board_id[16] | dev[16] | ctr u64 | len u32 | body | sig[64]
Body v1: ver=1 u8 | suite=1 u8 | epoch u32 | nonce[24] | XChaCha20-Poly1305 ciphertext
         key K_entry,e; AD = "taskdeck/v1/entry\0" ‖ frame header ‖ ver ‖ suite ‖ epoch
Plain:   entry_id[16] | kind u8 | at_ms i64 | based_on_seq u64 | based_on_chain[32] | JSON | padding
```

Every signature begins with a NUL-terminated context naming its use, so no signed message can stand in for another (V1-1):

| Use | Signed bytes |
|---|---|
| frame | `taskdeck/v1/frame\0` ‖ the frame up to `sig` |
| login | `taskdeck/v1/login\0` ‖ relay origin ‖ board_id ‖ dev ‖ gen ‖ relay_nonce[32] ‖ client_nonce[32]; the relay supplies only the nonce |
| admin | `taskdeck/v1/admin\0` ‖ operation ‖ SHA-256(arguments) ‖ relay_nonce |
| M1 hub request, reply | `taskdeck/v1/hub-req\0` or `taskdeck/v1/hub-resp\0` ‖ frame |
| backup | `taskdeck/v1/backup\0` ‖ SHA-256(file body) |

- **Kinds**, inside the ciphertext: `cmd`; `ctl` (DeviceAdded, DeviceRevoked, DeviceReinstated, EpochRotated, RecoveryKeyChange, Fence, ResetTo, Skip, RulesUpgrade); `beat` (daily: head, state hash at the latest seq ≡ 0 mod 64, supported versions, snapshot acks); `snap`. Padding: 512-byte buckets, Padmé above. Caps: 256 KiB per frame, 1 MiB per blob part.
- **Receipt checks**, in order: size; board and author (a member at that seq, or the recovery key); `verify_strict`; `ver` and `suite` allowed by the active rules; AEAD under the key the log position defines; unpad; strict serde (log-command types, `deny_unknown_fields`); `apply`.
- **Outcomes**: an unknown author or bad signature is skipped, consuming no counter value, with a tampering alarm. A validly signed frame that fails a later check before `apply` is refused on every device, its author's included, with an alarm naming the author, so no frame can halt a device (V1-1, V2-8); failing `apply` is an ordinary refusal reported to the author. A frame under this device's `dev` that it never wrote means "replace this device's keys" (V1-1). A `based_on` head at or after the entry's own seq, or off the local chain, is tampering; one older than the retained log is skipped with a warning (V1-16, V2-15).
- **Halts** happen in three cases only: an unknown RulesUpgrade, an entry that crashes `apply` on two consecutive starts (§4.5), and an epoch-commitment mismatch (§5.4).

### 4.3 Sequencing and idempotency

**Append** arrives over the socket, or over HTTPS from background jobs, in batches of at most 16 frames. `dev` must be the authenticated device. Ed25519 is checked first (WebCrypto [V], tested with Wycheproof edge cases so the relay is never laxer than the clients, V1-16), then one `transactionSync` re-reads the device row. `ctr = last_ctr + 1` is stored under the next seq and acknowledged once 3 of 5 replicas hold it [V R3]. A lower `ctr` returns the stored seq if the hash matches, and `409 conflict` with the stored frame if not; a higher one gets `409 gap`. The relay keeps one live socket per device and reports a second one to both ends (V2-21).

**Client rules** (V2-3, V2-14):
1. *Enqueue*: compile; trial-apply under `catch_unwind`, so a command that panics locally is never queued; add a random 128-bit `entry_id`; fsync the outbox.
2. *Send*: assign counters at seal time; seal, fsync the frame, transmit; retries resend identical bytes.
3. *Reconcile on hello*: if the relay's `last_ctr` is ahead, fetch those frames, check this device signed them, and drop outbox entries whose `entry_id` is already sequenced; a restored folder or VM catches up fully before sending.
4. *409 conflict*: a matching `entry_id` is the ack; otherwise warn "restored from a copy", re-seal, and stop after three repeats.
5. *Echo check*: acknowledged own entries must return at their seq with the same hash.
6. *One identity per folder*: a contended lock refuses sync (today it only warns, paths.rs:96-120 [V]); cloud-synced data folders are refused. No relay answer ever deletes local data or keys (V2-21).

`(dev, ctr)` and `entry_id` replace `X-TaskDeck-Request` and the 1,024-reply cache.

### 4.4 Ids

- **Item ids** are `device_no << 40 | n`, below 2^53, so JavaScript holds them exactly; today's 2^62 temporary ids collapse in `JSON.parse` [V R1]. Legacy ids stay below 2^40.
- **`device_no`** is assigned when DeviceAdded is applied: the next free number in log order (V2-8). E registers N's key in the same relay call, so N can append only from that seq, after reading its number.
- **High-water marks**: `BoardData` keeps each device's id and sid marks, and `apply` requires newly minted numbers above them, so no id is ever reissued, even after Forget or a restored folder (V2-3). Restore reuses the archived id if it is free, else its fallback id.
- **Blocks** get stable sids in Phase 2, carried in the existing `session` field, which the page only compares and echoes (phone.html:4909-4931 [V]). This closes E11 at M1 (V2-18).
- **Deleted**: `CLIENT_ID_FLOOR`, `Outbox::remap`, `known`, `number_from`. The subscription-id seeding bug [V R1] is fixed in Phase 0.

### 4.5 Deterministic, versioned apply

`apply(&mut BoardData, &LogCommand, &Ctx{seq, device_no, at, based_on}) -> Result<Applied, BoardError>`; a `Store` writes the change set it returns.

- **Nothing ambient**: no clock, time zone, I/O, randomness, floating point, hash-map iteration or `Vec`-order dependence. `taskdeck-core` has no file or network dependencies, and clippy disallows the `now()` functions and the type `chrono::Local` (V2-4).
- **Thin apply (V2-1)**: the origin's compile step decides policy (default names, colours, severity and horizon seeds, fallback ids, and trimming, since `str::trim` follows the toolchain's Unicode tables, board.rs:430 [V]); `apply` checks existence, ranges and lengths.
- **Rules versions (V2-1, V2-6, V3-2)**: `BoardData.rules` is the active version, and any change to what `apply` accepts or does (an op, field, limit, default, fixed check or envelope version) is a new one; old variants stay, pinned by golden logs. A version activates only through a sequenced `RulesUpgrade{v}`, valid only if every active member (a beat within 30 days) supports v; an override needs the author and one other member, so no single device can wedge the rest. Writers never emit what the active rules forbid, and the UI names the blocker ("available once Galaxy is updated"). A device meeting an unknown RulesUpgrade (only after an override) halts, shows its state and own unapplied entries as "frozen since …", holds new edits, and takes no snapshot, compaction or backup duty (V2-6).
- **Time**: `Ctx.at`, the origin's clock at compile time, is the only clock `apply` sees; it sets `created` (fixing the queued created-time bug) and the archive default and clamp (at most at + 1 day). Relay time never stamps anything; skew over 5 minutes against `relay_now` warns, and over an hour blocks composing until the user confirms the clock (V2-20, modified).
- **Zones (V2-4)**: core types are `DateTime<Utc>`, with year bounds checked in UTC (today `sane_instant` uses the local date, board.rs:446-460 [V]). Intents may carry naive local times, which the origin converts and clamps in its own zone; log commands take RFC 3339 with `Z` only. Routines get floating `MoveRoutine` and `CreateRoutine` ops that never call `resolve_on_day`; events use `SetEventSpan{start, minutes}`. Each device draws in its own zone, a deliberate change to DOC §21.3.
- **Plans**: Reflow compiles to `MoveBlocks{[(id, sid, start, minutes)]}` and Unplan to `RemoveBlocks{sids seen}` (V2-5).
- **Canonical encoding v1 (V2-2)**: sorted keys, default-valued fields omitted, UTC timestamps with nanoseconds, integers only, archive rows totally ordered (archived_at, id, bytes), unreadable lines in base64. The versioned state hash also covers the device set, epochs, counters and id marks; beats compare only matching versions, and CI checks golden hashes against the last three releases.
- **Display order** belongs to the UI: `summarize_calendar` stops re-sorting `board.items` (ui.rs:2911-2955 [V]); ties break on (start, id, sid); §14.4 is untouched.
- **Poison pills (V2-14)**: release builds use `panic = "unwind"` (today `"abort"`, Cargo.toml:94 [V]). Pages of up to 256 entries are applied under `catch_unwind`, one marker per page; after a hard abort the page is replayed entry by entry, and a device halts only if the same entry fails twice in a row, naming its author and offering Skip (§4.6). A feed that panics is quarantined.
- **Gates**: proptest determinism and rebase suites under two `TZ` values; golden logs per rules version with boundary values, replayed on all three operating systems and on the phone; cargo-fuzz on frames, commands plus `apply`, `ics::parse` and `build_day`.

### 4.6 Conflicts

- **Ordering**: the last writer wins per command, in seq order, as today, now identically everywhere.
- **Overwrites (V2-5)**: `BoardData` records the last (seq, device) of each item and block. A command overwriting another device's newer change still applies, and both devices get "changed on Desk since you last synced". `MoveBlocks` skips blocks changed or gone since `based_on`; `RemoveBlocks` removes only blocks its origin saw.
- **Benign repeats (V2-16)**: completing or deleting an archived item, or restoring a live row, is a no-op success. Refusals and notices go to a persistent per-device list.
- **Notes**: `SetNotes{text, base}`. A stale base is refused, and the origin keeps a draft with a merge view. The notepad edits a draft, not `board.notes` (ui.rs:2152 [V]); saves coalesce to one per 30 s while typing, plus blur and exit, instead of every 2 s. Rules v1 counts the length cap after detab (today before, board.rs:1253-1256 [V]).
- **Naming (V2-17)**: a Create being named is held only while it is the outbox's last entry. Create then Forget never leaves the device; Create then Rename leaves as one Create.
- **Skip (V1-11)**: `Skip{seq, frame_hash}` is signed by a member other than the entry's author, or by the recovery key, and never targets a control entry. Every device then treats the entry as refused, replaying from a base if it had applied it. Halted devices read ahead for Skip and control entries, so revocations still land.

### 4.7 Offline, catch-up, snapshots, compaction, retention

- **View**: the confirmed state plus this device's pending entries, rebased per batch; doomed entries are marked "will be refused". Offline use is unlimited, with a warning at 10,000 queued.
- **Catch-up**: pages of at most 256 entries or 1 MiB, one fsync each; two weeks away (about 4,200 entries) takes seconds [A R3].
- **Snapshots (V1-10, V2-7)**: taken at S ≡ 0 mod 64, where every device records its state hash; each device that applied S acks the `snap` entry, or raises a mismatch alarm, in its next beat. A snapshot is one AEAD message (AD: blob id, S, writer) split into parts of at most 1 MiB, hash-checked before decryption and inflated under a cap (V1-12, V1-14). The Windows desktop writes one weekly or every 1,000 entries, any current, unhalted device when the newest acknowledged one is a week old; the relay keeps the newest three plus each author's newest.
- **Compaction** deletes entries older than 365 days that a snapshot acknowledged by another device covers, in daily slices of at most 5,000, because deletes count as rows written [V R3]. Control entries are kept (V2-15).
- **Bootstrap**: from a fresh snapshot of the pairing device's state, verified up to the given head. A device away for more than 365 days uses the newest acknowledged snapshot and is told continuity could not be checked.
- **Retention (V2-13, V3-7)**: deleted text lingers in the relay log (365 days, plus 30 days of point-in-time recovery), snapshots, device logs and bases (400 days) and backups, as the Forget dialog says. "Purge history before now" (Phase 7) snapshots, truncates every log and base, rotates the epoch, deletes the old roots everywhere (crypto-shredding the relay's copies) and, after confirmation, older backups.

### 4.8 Integrity checks and responses

- **Checks**: contiguous seq; per-device counters rising by one; a hash chain C_s = SHA-256(`taskdeck/v1/chain\0` ‖ C_{s−1} ‖ s ‖ SHA-256(frame)) with a persisted head; `based_on` heads (SUNDR fork consistency [V R4]); echoes; beats and acks.
- **Last seen, not silence (V2-10, V3-14)**: Settings shows each device's last beat. An alarm needs evidence: an unechoed own entry; a relay head behind the stored one; a chain mismatch at a `based_on` seq; a counter gap the device holds acks for; or proof that another device was active during a span whose entries never arrived.
- **Response (V2-9)**: the device stops sending and applying, keeps editing locally, and shows the evidence. After a plain relay rollback, the most advanced device runs **restore tail**, re-uploading frames at their original seqs; the relay accepts them only with registered signatures and if they extend its chain, which it can check without keys. A fork or lasting withholding means a re-home (§6.7).
- **Determinism mismatch**: only the minority halts, and adopting another state requires choosing that device by the fingerprint on its own screen (V1-10). The relay protocol is versioned by path (`/v1`); deploys are additive, so hibernated sockets survive them [V R3].

### 4.9 Latency targets

| Device state | How | Target |
|---|---|---|
| Desktops awake; phone app visible | push over the hibernating DO socket; free 30 s client pings that do not wake the DO [V R3] | p95 < 1 s |
| Laptop after sleep | reconnect (backoff bypassed once on resume), hello, replay | < 5 s after the network returns |
| Phone app opened | local replica drawn at once, then catch-up | drawn < 0.7 s [A]; fresh < 2 s p90 |
| Phone in background, not idle | expedited job (API 31+) when leaving with queued edits; a JobScheduler job every 30 min | edits out within a minute; pulls ≤ 30 min |
| Phone in Doze or One UI sleep | maintenance windows; an optional hourly freshness alarm, never within 9 minutes before a reminder (V3-13); "Never sleeping apps" onboarding | only "fresh on open" is promised until measured in Phase 6 |
| Reminder due | exact alarm, sync of up to 5 s, recompute, notify | on time, even offline |
| M1 (V3-9) | the hub must run; phone edits leave only while the app is open | < 1 s while the hub runs; otherwise queued, as today |

## 5. Cryptography and keys

### 5.1 Algorithms and crates

| Use | Algorithm | Crate (crates.io, 2026-11-20 [V R4]) |
|---|---|---|
| Entries, snapshots, blobs, local files | XChaCha20-Poly1305, random 24-byte nonces | chacha20poly1305 0.11.0 |
| Signatures | Ed25519, `verify_strict` | ed25519-dalek 3.0.0 |
| Wraps, pairing bundle, backups, M1 hub frames | HPKE base mode (RFC 9180): DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20-Poly1305 | hpke 0.15.1, x25519-dalek 3.0.0 |
| Derivation, hashing | HKDF-SHA256, SHA-256 | hkdf 0.13.0, sha2 0.11.1 |
| Randomness, wiping, comparison | OS RNG, zeroize, constant time | getrandom 0.4.3, zeroize 1.9.2, subtle 2.6 |
| TLS, as defence in depth | rustls on ring, OS trust store | rustls 0.23.46, rustls-platform-verifier 0.7.1 |

- **One RustCrypto generation**: a scratch build of this set has no duplicate crypto crates [V V3]; ring remains only under rustls. Not chosen: age (a second generation plus i18n crates), ring alone (no static X25519 key), MLS (every device holds the plaintext anyway). All devices use one pinned ed25519-dalek, so verdicts agree; the relay's check only filters.
- **Assurance (V3-5)**: no paid audit by default. `SPEC.md` uses standard constructions only, test vectors are published, CI runs Wycheproof and RFC 9180 vectors, and the spec is offered for public review. Key material comes only from getrandom, never from today's non-CSPRNG token fallback on Windows (phone.rs:1038-1078 [V R1]).

### 5.2 Key hierarchy

```
Recovery seed: 256 bits, on paper only (two copies)
  ├ recovery HPKE key (X25519)      backups, and a wrap of every epoch root
  └ recovery signing key (Ed25519)  the root of membership authority (§5.5)
board_id = SHA-256("taskdeck/v1/board\0" ‖ recovery signing public key ‖ salt[16])[0..16]
Epoch root R_e (32 random bytes): HPKE-wrapped to each device and to the recovery key;
  its rotation entry carries c_e = SHA-256("taskdeck/v1/epoch\0" ‖ board_id ‖ e ‖ R_e)
  K_kind,e = HKDF-SHA256(salt = board_id, ikm = R_e, info = "taskdeck/v1/" ‖ kind ‖ e), kind ∈ {entry, snap, blob}
Device d: Ed25519 and X25519 keys that never leave d; storage key L_d wrapped by DPAPI / Keychain / Keystore
Relay token: "TD1." role ‖ board ‖ dev ‖ gen ‖ HMAC-SHA256(TOKEN_KEY, role ‖ board ‖ dev ‖ gen); roles: device, backup-read
```

- **HPKE info** binds the use, board, epoch or seq, and recipient (`taskdeck/v1/wrap\0`, `taskdeck/v1/backup\0`, the hub contexts); the suite id is in every AD (V1-12).
- **Tokens** travel only in the `Authorization` header. The Worker checks them statelessly before any DO call, and the DO checks state and generation. Devices obtain them, also on a new relay after a re-home, by signing a login challenge.
- **Admin operations** (register, revoke, reinstate, fence, seed, compaction, restore tail) need a fresh admin signature from a member or the recovery key; a token alone never suffices (V1-5).
- **Backup read tokens** are minted per writer device and generation, and revoked with that device and at every rotation (V1-2).

### 5.3 Pairing: typed code, explicit transcript, copy-and-enter

E is the existing device, N the new one.
1. **Authenticate on E**: Windows Hello (through the `windows` 0.62 crate already in Cargo.lock [V]) or Touch ID; on Android, BIOMETRIC_STRONG only, with no PIN fallback (V1-5).
2. **Ticket**: E draws an 80-bit secret s, registers H(HKDF(s, "ticket")) (single use, 10 minutes, 5 tries), posts slot A = H(epk_E ‖ r_E), and shows the host and `XXXX-XXXX-XXXX-XXXX-CC` in a capture-excluded window, never on the wall layout (V1-6). Slots are MAC'd under HKDF(s, "mac") over `taskdeck/v1/pair\0` ‖ label ‖ sender role ‖ body; the relay never sees s.
3. **On N** the code is typed into a field without suggestions, learning or clipboard (the host may be pasted). N posts slot B with its long-term X25519 and Ed25519 keys, epk_N and its name; E reveals slot C, (epk_E, r_E), which N checks against A.
4. **SAS**: T = SHA-256(`taskdeck/v1/pair-transcript\0` ‖ relay origin ‖ board_id ‖ ticket id ‖ dev_E ‖ A ‖ B ‖ C), and the SAS is 6 digits of HKDF(X25519(epk_E, epk_N), salt = s, `taskdeck/v1/sas\0` ‖ T). **Copy-and-enter** (V1-6; compare-and-confirm is the error-prone variant, Kainda et al., SOUPS 2009): the user types E's digits into N, N compares, and the user then confirms on E that N accepted.
5. **Bundle**: slot D, sealed under HKDF(shared secret, s, "bundle"), carries the board id, relay, current and previous epoch roots, device set, head, a fresh snapshot of E's state with its hash, and the recovery public keys. E appends DeviceAdded naming exactly the keys in T and registers N's key in the same call.
6. **Join**: N reads its `device_no` from the sequenced entry, logs in, loads the snapshot and tail, and verifies the chain to the head. Every other device shows a banner ("New device 'Galaxy', key 7F3A…, added by 'Desk'. Not you? Revoke") until acknowledged.

An attacker with both the code and the relay gets one 10⁻⁶ guess per attempt, each visible [A R4]. v1 has no QR code, camera library or deep link.

### 5.4 Revocation, rotation, compromise

- **Revoke** (a member after device authentication, or the recovery key): a signed admin revoke drops the device's key and read tokens at once. One control entry then carries DeviceRevoked and EpochRotated{e+1}, with wraps of R_{e+1} for the remaining devices and the recovery key, the commitment c_{e+1}, and the head.
- **Races (V2-8)**: the first EpochRotated for e+1 in log order wins; later ones are refused everywhere and their keys discarded. Each device checks its unwrapped root against c_{e+1} and halts on a mismatch, so no member can give devices different roots (V1-12). A revoked device's later entries are refused; an epoch-e entry sequenced after the rotation is accepted only from a current member, based before the rotation, within 30 days (bundles carry the previous root). Authors re-seal refused entries with all their later ones, in order.
- **Suspected compromise means "Replace device" (V1-3)**: revoke it, pair the machine again as a new device (new keys, storage key, token and `device_no`), and rotate. Cleaning the machine is not enough.
- **Also**: a revoked device that reappears offers "re-pair and resubmit my unsent edits" (V2-21). Rotate on revocation, compromise or a new recovery key, never on a schedule. **Reset trust** starts a new board from a trusted device's state.

### 5.5 Recovery

- **The sheet** holds the recovery seed (52 Crockford base32 characters plus 4 check characters), the relay host, the board id and the release-signing key's fingerprint. It is printed twice from memory through the OS print dialog, never through a file, or copied by hand (V3-15). Setup finishes only after the whole seed is retyped from paper and a test backup opens with it (V1-17). Store the copies apart.
- **Its powers**: no relay token and no enrolment. Typed into a trusted desktop or `taskdeck-recover`, and wiped after use, it signs recovery control entries: revoke any device, reinstate one another device revoked, rotate, Skip, Fence, change the recovery key (V1-5). Alone it can disrupt sync; with a backup it reads everything.
- **A new recovery key (V1-2)** needs the current sheet. If it is lost, one member proposes, every device shows a banner for 72 hours, and a different member confirms; any member or the old key can cancel. Backup writers pin the fingerprint confirmed on their own screens.
- **Total loss**: run `taskdeck-recover` from the offline USB, checked against the sheet's fingerprint. It takes host and board id from the sheet, verifies each backup's signature and membership chain back to the self-certifying board id, lists verified backups by writer and seq (V1-13), decrypts the chosen one, tops it up through its read token, writes a `taskdeck_data` folder, and fences the old board (V2-21). Losing every device and both sheets loses the data, by design.

### 5.6 At rest

| OS | Keys | Data |
|---|---|---|
| Windows | DPAPI-wrapped L_d (`CryptProtectData`, user scope) in `%LOCALAPPDATA%\TaskDeck`; not `keyring`, whose entries roam [C1] | sealed store; BitLocker or Device Encryption (its recovery key is in the Microsoft account) |
| macOS | login-Keychain item (security-framework, via the platform verifier [V V3]), ACL tied to the signing identity | sealed store; FileVault |
| Android | Keystore AES-256-GCM key (StrongBox if present), not bound to user authentication, so jobs and reminders run while locked | app-private storage; backup and every transfer excluded (`allowBackup="false"`, `dataExtractionRules`); DOM storage off from M2 (today the page caches days and its queue, phone.html:1276-1478 [V]); reminder times and pending actions in device-protected storage (V2-14); optional `setUnlockedDeviceRequired` |

Sealing defeats file copies, not malware running as the user, hence §8's development isolation. A device whose key cannot be unwrapped after a reset is replaced, risking only its unsent outbox. Code and seed screens exclude capture; notifications are `VISIBILITY_PRIVATE`.

## 6. Hosting and uptime

### 6.1 Provider

Cloudflare Workers Free, with one Worker under a random `workers.dev` name and one SQLite DO per board, created from home because objects never move [V R3]. The DO uses only the hibernation API, with no timers, long-polls or alarms; a non-hibernating object would burn 83% of the duration quota [V R3]. Clients send a constant, descriptive User-Agent (Error 1010 otherwise [V R3]). The account has a dedicated email address and a passkey; deploy tokens are scoped to Workers Scripts:Edit, IP-filtered, valid for at most a day and never kept in CI. The host may be pasted during pairing; a rename reuses the fenced re-home (V3-18).

### 6.2 Quota math

A typical day (3 devices, 300 edits, 50 phone opens, 48 background syncs) uses about 220 Worker and 240 DO requests of 100,000 each, 3 of 13,000 GB-s, 1,500 of 100,000 rows written and 5,000 of 5 million read: under 2% of every limit. A 5,000-command import stays under 20%; M1 forwarding adds about 750 requests. Storage is 0.15–0.3 GB of 5 GB. An append costs about 1 ms of CPU against Free's 10 ms, if that cap binds DOs at all [U; measured in Phase 1]. Guards: full-jitter backoff from 1 to 60 s; daily budgets (5,000 requests per client, 3,000 appends per device); tests asserting rows per operation; quota errors shown as "sync paused until 00:00 UTC".

### 6.3 EUR 0

Workers Free fails closed: over a limit, calls fail until 00:00 UTC and nothing is billed [V brief]. R2 (it bills overage [V R3]), KV, D1 and FCM are not used. Backups fit free OneDrive or iCloud space: 1–3 MB nightly plus 10–25 MB monthly segments, about 0.5 GB per destination [A] (V2-12, V3-4). The only optional spending is the domain for §7's WAF gate and a signing token.

### 6.4 Failure modes

| Failure | How often | Behaviour |
|---|---|---|
| Regional DO incident; Cloudflare-wide outage | 10–20 a year, 15–120 min; 1–2 a year, hours [A R3] | full local use; "relay unreachable since 14:05 · 3 waiting"; resumes by itself |
| Quota exhausted | rare | paused until 00:00 UTC; if repeated, a new Worker name or the domain and WAF rule |
| Bad deploy; relay loses, rolls back, forks or withholds data | rare | roll back (the conformance suite gates deploys); alarm with evidence, then restore tail or re-home (§4.8) |
| Account suspended, or the Free plan changes | rare | re-home in 1–2 h; nothing is lost |
| Determinism bug, or an entry that crashes `apply` | rare | the minority halts and adopts a state chosen by fingerprint; a crashing entry waits for an update or a Skip |
| Device lost | — | revoke and rotate; its unsent outbox is lost |
| ICS host or Open-Meteo down | common | last data kept, labelled with its age |

### 6.5 Expected availability

Local use: about 100%. Relay: 99.7–99.9%, about 9–26 hours of delay a year, mostly in partial windows [A R3]. End to end, the phone adds its background limits, but a phone awaiting an update keeps syncing, because rules upgrades wait for it (V3-2). There is no hot second sequencer, since two relays with no arbiter split-brain [R3]; redundancy comes from three replicas, 400-day logs, backups and a rehearsed re-home. During M1, applying edits also needs the Windows desktop running, as today.

### 6.6 Backups and restore (V2-12, V3-4, V1-13)

- **Nightly** (or at the first chance after being off), the Windows desktop writes `TaskDeck-<date>-<seq>.tdbk`: canonical `BoardData`, sync metadata, device set, epoch roots and a backup-read token, deflated, signed and HPKE-sealed to the recovery key, so the writer cannot read it back. The Mac does the same. **Monthly**, each adds a signed segment of that month's frames.
- **Where**: the OneDrive or iCloud Drive folder and a second disk keep 30 nightly files, 12 monthly ones and 13 segments; an offline USB stick gets a monthly copy. Each file is re-read and checked; Settings shows the last good write, and a failure raises an alarm.
- **Drills**: CI restores a synthetic board from a synthetic sheet on every release; twice a year you restore the real one on another account and compare counts and the latest edits.
- **Restore to a point in time**: replay from the nearest weekly base to the chosen seq and append `ResetTo{seq, state hash}`. Every device recomputes it by local replay, and only seqs that every active device still holds are allowed (V1-10).

### 6.7 Exit plan

The relay protocol `/v1` has eight operation families (create, login, socket, read, append, blobs, pairing, admin) and a conformance suite. `relay-core.js` runs unchanged on another Cloudflare account, on Workers Paid (USD 5 a month), or on Node 22 with `node:sqlite` on a EUR 1–2/month VPS behind Caddy (TLS and request buffering), polled over HTTPS.

**Re-home**, fenced and never followed automatically: deploy the standby; on a trusted device, "Move board" appends a signed `Fence{new relay, g+1}`, fences the old DO to reads if it still answers, and seeds the new relay with the device set, the newest acknowledged snapshot and up to 30 days of tail, including other devices' frames, each signature checked, listing anything not carried over (V2-9). Each other device moves only after the user types the 8-character move code shown on the initiating device; it logs in with its key, checks the new chain against its own log and resubmits its outbox. CI rehearses this on every release; you rehearse it yearly.

## 7. Security

| Threat | Controls | Residual |
|---|---|---|
| **Malware via updates** | no updater; signed tags after a diff review; cache-free, secret-free release builds; strict attestation checks; reproducible rebuilds; the APK signed offline, and Android accepts updates only from the same certificate [V R2] (§8) | a malicious commit you approve |
| **Malware via dependencies or build tools** | `--locked`; cargo-deny and cargo-audit on lockfile changes; a 7-day cooldown; cargo-vet on the seal and core crates; locked tool installs; Gradle verification; no npm on personal machines; no compiles where TaskDeck keys live (V1-8, V3-8); AI-agent PRs reviewed like any other | an xz-style backdoor that passes review |
| **Malware via the relay** | no code served; the receipt checks of §4.2, refusals instead of halts, Skip; fuzzed decoders; `catch_unwind`. Synced pictures decode as JPEG only, in Rust under `image::Limits`, and are re-encoded before the WebView sees them; the upload path guesses formats today (phone.rs:1534 [V]) (V1-14, V3-12) | a memory-safety bug in a Rust dependency |
| **Malware via ICS feeds** | kept: https only, ≤ 3 redirects, 20 s, 8 MiB, the bounded parser. New: redirects followed by hand and re-checked, without a Referer (reqwest 0.12.28 sends the previous URL with its query: client.rs:312, redirect.rs:293-303 [V]); a pinning resolver refusing IP literals and private, loopback, link-local, CGNAT, ULA, IPv4-mapped, NAT64, 6to4, 0.0.0.0/8, multicast and broadcast addresses (V1-15); no ATTACH or URL fetches; errors without the URL (subscriptions.rs:545-550 leaks it today [V]); a feed's own name adopted once, by the device that added it (ui.rs:2753-2765 renames on every device today [V]) | a contained parser crash or a wrong overlay; publishers see IPs |
| **Malware via the WebView** | bundled assets only, everything else answered in-process or 404/403, `setBlockNetworkLoads(true)`, no navigation, file access or release debugging; today's CSP (phone.rs:1212-1217 [V]) with a script hash, `connect-src` limited to natively answered origins, `frame-src 'none'` and one Trusted Types policy; a document-start script that makes frames inert, removes every `RTC*` constructor and limits `link rel` values (V1-9); a bridge for the page's 17 board verbs and read routes only, never subscription, device, key or relay operations (today `/api/command` takes any Command, phone.rs:887-928 [V]); a Phase 3 exfiltration harness, with a split into a UI app without `INTERNET` plus a network app if a channel stays open | an XSS gets your own undoable board verbs |
| **Malware via pairing** | the code is a ticket, not a key; MAC'd slots; explicit transcript; copy-and-enter; capture-excluded window; device authentication; key fingerprints in banners; no QR, camera or deep links | you enter digits from a screen you did not check |
| **Malware via push** | none in the plan of record. Phase 9 ships only if its gate passes: a constant data-only payload; the credential only on the Windows desktop, as an FCM-only service account; notification delegation off (`firebase_messaging_notification_delegation_enabled=false` [V firebase.google.com]) (V1-19) | if enabled, Google sees timing |
| **A compromised device, or one stolen unlocked** | persistent banners with the author's key fingerprint for every control entry (V1-2); snapshots and ResetTo checked against honest replay; the recovery key overrules membership changes; "Replace device"; BIOMETRIC_STRONG for membership changes on Android; state-changing notification actions with `setAuthenticationRequired(true)` (API 31+ [V developer.android.com]) (V1-19) | whatever it read before revocation |
| **Theft at the relay or Cloudflare** | E2EE; padding; kind and `based_on` inside the ciphertext; random ids; a constant User-Agent; no logs | IPs, timing, padded sizes, device count |
| **Theft on the network** | TLS from the OS trust store everywhere, closing RUSTSEC-2026-0381/-0382 [V R4]; `http://` refused from M1; tokens only in headers | metadata |
| **Theft from backups** | HPKE to the recovery key; writer signatures; per-writer read tokens; the sheet stored apart | sheet and backup stolen together |
| **Relay tampering, rollback, forking, withholding** | signatures and AEAD; counters and seq checked by clients; the hash chain; `based_on` heads; echoes; beats; restore tail; a fingerprint (seq plus four words) in Settings | withholding shows only as staleness until there is evidence |
| **Account takeover** (Cloudflare, GitHub, Google, Microsoft, Apple, Samsung) | passkeys or hardware keys; a dedicated email; short-lived scoped tokens; protected `main`; not on Play; app data excluded from backup and transfer; Samsung Remote unlock off | denial of service, or a remote wipe of one device |
| **DoS and quota exhaustion** | HMAC gate before any DO call; the hostname kept out of the repo; no HTML or CORS; budgets; quota-aware clients; optionally a domain with `workers_dev = false` and a WAF rule requiring a secret path prefix (whether blocked requests count is undocumented [U]) | anyone with the hostname can pause sync until 00:00 UTC |
| **Permanent loss or lockout** | three replicas with 400-day logs; nightly backups in two places plus offline copies; two fully retyped sheets; drills; the signing key in two copies; a rehearsed re-home | losing every device and both sheets |

Residual risks, plainly:
1. **A compromised device** is the largest: malware, or a thief holding it unlocked, sees and can change everything until revoked. Banners, snapshot checks, the recovery override and restore-to-seq limit the damage; nothing undoes what was read.
2. **A poisoned dependency** that survives the lockfile, the cooldown, the audits and review.
3. **Metadata**: Cloudflare sees IP addresses (and so travel), activity times and sizes; ICS hosts and Open-Meteo see device IPs.
4. **Quota DoS** by anyone who learns the hostname.
5. **No paid audit** of our envelope code (§5.1).
6. **Until M2**, edits apply only while the hub runs, and local copies stay plaintext behind disk encryption. M1 is safer than today (no plaintext HTTP, no tokens in URLs) but not as safe as M2 (V3-5).

## 8. Build, distribution and updates

**Development isolation (V1-8, V3-8).** No cargo, rust-analyzer, Gradle, Android Studio or npm runs in an OS account that holds TaskDeck keys. rust-analyzer runs build scripts and proc-macros when a project opens [A], and a poisoned `build.rs` (as in arrayref 0.3.10 [V R4]) could unwrap DPAPI or Keychain secrets. Develop in cloud sessions, a VM or a separate standard account, against a development board. Daily accounts are standard users; administration uses a separate account.

**Source**: `main` is protected, and every change, AI-agent branches included, arrives through a reviewed PR with one dependency change per PR. Before `git tag -s`, review `git diff <last signed tag>..HEAD` locally, starting with Cargo.lock, `build.rs` files and `.github` (V1-4). `rust-toolchain.toml` pins the compiler, and `build.rs:11` uses `SOURCE_DATE_EPOCH` instead of `Utc::now()` [V].

**CI** runs on ephemeral GitHub runners without secrets, with `contents: read` and SHA-pinned actions. Every push runs the Linux suite (locked tests, two-`TZ` determinism, golden logs and hashes, deny, audit, vet, `node --test`, a fuzz smoke run, the unsigned APK); Windows and macOS golden logs run nightly and on tags (V3-11). The release workflow runs only on tag push and neither restores nor saves any cache, and no test job writes a cache a release could read, closing the Cacheract pattern (V1-4). It builds every artifact with `actions/attest-build-provenance` and immutable releases. Tools come from `cargo install --locked --version …` or verified binaries.

**Verify every artifact** with `gh attestation verify <file> --repo O/R --signer-workflow O/R/.github/workflows/release.yml --source-ref refs/tags/vX --source-digest <commit from git verify-tag> --deny-self-hosted-runners`; without those flags only the repository and predicate type are enforced [V cli.github.com manual]. `libtaskdeck.so` and `taskdeck-recover` are built reproducibly (a pinned container, `--remap-path-prefix`, `SOURCE_DATE_EPOCH`), and one of them is rebuilt locally per release and compared by hash.

**Windows**: verify; unzip to `%LOCALAPPDATA%\Programs\TaskDeck`, never into OneDrive. SmartScreen warns on every new unsigned release; Smart App Control, if on, blocks unsigned executables [A V3] (V3-16).

**macOS**: verify; `scripts/mac-bundle.sh` signs with the 20-year self-signed "TaskDeck Local" identity, kept in its own keychain, locked outside releases and never granted "Always Allow" (V1-8). Clear the quarantine attribute and move the app to `~/Applications`.

**Android**:
- AGP 9.5, NDK r28 (16 KB pages), compileSdk 37, targetSdk 36, minSdk 29 (raised to 31 or 33 once the phone is known); cargo-ndk 4.1.2. The cdylib uses `panic = "unwind"` with `catch_unwind` at every JNI entry. Gradle verification covers the whole dependency set; the platform verifier's AAR is vendored by checksum (V3-16).
- Release: verify; sign in the build account with `apksigner` through PKCS#11 on a touch-to-sign token (the EUR 0 fallback is a key file on an encrypted USB stick mounted only there); check `apksigner verify --print-certs` against the pinned fingerprint; turn Auto Blocker off and USB debugging on; `adb install -r`; then revoke the debugging authorisations, turn developer options off and turn Auto Blocker back on.
- Test adb before ever enabling Advanced Protection mode, which blocks installs from unknown sources [V3]. Register the package and certificate on a free limited-distribution account before the global verification rollout from January 2027 [V R4]; adb installs stay exempt [V R2]. Not Play, whose App Signing would hold the key.

**Relay**: `relay/deploy.sh` checks the tag signature and uploads with curl through the Script Upload API (`main_module`, the DO binding, a `new_sqlite_classes` migration [V R4]) from a throwaway container with the 1-day token. Preview URLs are off.

**Supply chain**: Phase 0 refreshes the lockfile to rustls ≥ 0.23.46, rustls-webpki ≥ 0.103.17, webpki-roots ≥ 1.0.12, and tokio past RUSTSEC-2026-0386 [V R4]. `deny.toml` allows crates.io only, bans duplicate crypto crates and carries a reviewed ignore for unmaintained chrono (RUSTSEC-2026-0543); cargo-cooldown enforces the cooldown. Phase 2 replaces reqwest with ureq 3, dropping tokio and hyper [V V3]. Update monthly, or within a week of a relevant advisory; a renewal register tracks the signing keys, the Cloudflare account, the sheets and the drill dates.

## 9. Migration from today's board

1. **Drain**: keep the phone page open until nothing is waiting and let any client desktop reach `↑ 0`, then quit TaskDeck and any `taskdeck-server`.
2. **Back up** every `taskdeck_data/` as a dated zip onto two disks, never to be deleted.
3. **Choose one board**: the server's if one is in use, otherwise the Windows desktop's. Folders are never merged by copying; another board's items can come in later as ordinary commands, after a preview.
4. **Dry run**: `TaskDeck --import-check <folder>` refuses a folder whose `.lock` is held; reports counts and any id that is both live and archived (V2-11); renumbers duplicate subscription ids; folds ids ≥ 2^62 (`adopt_temporaries`, board.rs:697 [V]); checks that every id is below 2^40 and every instant survives conversion to UTC; and prints a canonical hash that must repeat.
5. **M1 (hub)**: on the Windows desktop, choose Settings → SYNC → "Create board" (keys, relay, sheet). Pair the Mac (its first-contact logic sets its own board aside, DOC §22.3) and the phone app, and compare counts. Press **New key** (ui.rs:3597 [V]) to retire every old token link (V1-18). Then turn off the phone view and the feed, remove any Google Calendar subscription to the feed (its URL carries the read/write token, DOCUMENTATION.md:2893 [V]), remove the old web app from the phone's home screen, and uninstall Tailscale.
6. **M2 (cutover, in one sitting)**: every M1 queue must be empty and acknowledged; clients report their queue lengths to the hub, and M2 builds refuse to convert or bootstrap otherwise (V2-11). Update all three devices; on first start the M2 phone replays the page's old localStorage queue through the compile step before DOM storage goes off. "Convert to replicated board" re-runs the checks, writes the genesis (signed by the recovery key; palette and home place from `userconfig.toml`), moves the legacy files untouched to `pre-sync-<date>/`, and deletes their phone token. The Mac and the phone bootstrap from the genesis snapshot; compare fingerprints and counts, and spot-check a few items.
7. **Watch** the legacy folder for 30 days; if an old build writes to it, offer "import changes" as ordinary commands.
8. **Roll back** if needed: an old build opens `pre-sync-<date>/`, and "Export legacy" writes day-planner files from the current board.

## 10. Implementation plan

**Basis**: a focused week is 30 hours of your own time. The estimates are bottom-up [A] and not discounted for AI agents, which shorten coding but not review, device testing or drills. Recalibrate after Phase 1.

| Phase | Deliverables | Focused weeks | Tested by |
|---|---|---|---|
| **0 Hygiene** | lockfile refresh; subscription-id fix; `created` stamped at the origin; getrandom tokens; `http://` only to loopback and Tailscale ranges (100.64.0.0/10, fd7a:115c:a1e0::/48) until M1 (V3-10); `.referer(false)`; errors without URLs; Linux CI with deny and audit; passkeys; disk encryption; the development account; a dated encrypted backup | 1–1.5 | the 264 lib tests plus regressions |
| **1 Seal and relay** | `taskdeck-seal` and `SPEC.md`; the relay (create, tickets, registry, login, signed admin operations, revoke, sockets, budgets, forwarding); the OS key stores | 3.5–4 | known-answer and Wycheproof vectors, tamper tests, fuzzing, `node --test` on a staging Worker |
| **2 Hub on the desktops** | `phone::route()` lifted out of the tiny_http handler; the hub (§10.1); `Remote::Tunnel` for the Mac; stable sids (E11); Settings → SYNC behind Hello or Touch ID; ureq fetchers with the SSRF guard | 3.5–4.5 | mock-relay, pairing-MITM, replay and revocation tests |
| **3 Android app** | Gradle, cargo-ndk, JNI; the hardened WebView and bridge; the exfiltration harness; photo picker; coarse location; Keystore; backup exclusions; Pair, Devices and Health screens; signing and install runbook | 6.5–8.5, incl. 1–2 if Android is new | emulator: origins, exfiltration, backup; phone: offline, process death, reboot |
| **M1: relay hub** | **all three devices on the EUR 0 relay, encrypted, with a native phone app and no Tailscale; edits apply while the Windows desktop runs** | **14.5–18.5 in total** | a week of daily use |
| **4 Deterministic core** | `taskdeck-core` and everything in §4.4–§4.6: pure `apply` and `Store`, UTC and log-command types, device ids and marks, `entry_id`, thin apply, rules versions, canonical hashing, overwrite notices, notes drafts, unwinding panics, trial apply | 6–7.5 | proptest under two `TZ`; golden logs and hashes; the 28 board tests; fuzzing |
| **5a Desktop replicas** | relay log routes (append, read, snapshots, compaction, restore tail); the sans-IO engine with reconciliation, echoes, beats and evidence-based alarms; snapshot acks; epoch rules; Skip; banners; the sealed store; "Export .ics…"; the cutover tool; hub, tiny_http, feed and `taskdeck-server` deleted | 5.5–7 | a deterministic network simulator (1,000 fault schedules per CI run); the cutover rehearsed on a copy of the real board |
| **5r Recovery and backups** | recovery seed and signer; the sheet; recovery-key change; signed backups and segments; `taskdeck-recover`; the CI restore drill | 2.5–3 | every backup kind restored; forged-backup and stolen-token tests |
| **5b Phone replica** | core in-process; DOM storage off, with a native store for page preferences; an unsilenceable sync line; refusals in the notice banner; notes base and conflict sheet; "will be refused" marks (V3-6); JobScheduler jobs; Health screen; ICS with the raw-body cache; the desk picture through Rust; M1 queue import; system-calendar export if needed (+1–1.5) | 4–5.5 | emulator and phone: offline, Doze, process death, a 10,000-row archive |
| **M2: full replicas** | **every device works and syncs without the desktop; all four requirements met** | **32.5–41.5 in total** | |
| **6 Reminders** | core `reminders()`; a chain of exact alarms, folding those under 9 minutes apart [V R2]; verify before notifying; the Doze-budget rule; Done, Tomorrow and Snooze behind authentication | 2–2.5 | forced Doze, standby buckets, reboot, zone and DST changes |
| **7 Operations and hardening** | the simulator at 10,000 schedules; an adversarial relay (drop, reorder, replay, fork, rollback, truncate) that must raise alarms; the standby adapter, fenced re-home and its CI rehearsal; purge history; restore to a point in time; alerts; the cargo-vet backlog | 6–7 | the drills |
| **Documentation** | DOCUMENTATION.md (3,492 lines [V]), SERVER.md, MOBILE.md and README rewritten along the way | 1.5–2.5 | |
| **8 Optional** | widgets, tile, share target; phone extras (archive search, Forget, subscriptions, home place, palette); in-app QR scanning; a home-zone display | about 1 each | |
| **9 Optional push** | FCM tickles behind the gate in §7 | 1 | timing in and out of Doze |

**Totals**: Phases 0–7 and the documentation come to **42–53.5 focused weeks**, or 55–70 with 30% contingency. Starting 2026-11-23 at 30 hours a week, M1 lands around March–May 2027 and M2 around July–December 2027 (the later dates include the contingency); at 15 hours a week, about twice as long. Upkeep afterwards is about 6–10 hours a month: monthly dependency updates and installs on three devices, vet and Gradle metadata, platform changes, and the twice-yearly drill [A].

### 10.1 How the M1 hub works (V1-7, V2-18, V3-9)

- **Requests** are HPKE-sealed to the hub's X25519 key and signed by the device, each carrying a per-device counter that both sides persist, instead of a time-based replay window.
- **The hub** checks the sender against its per-device allow-list and answers through `phone::route()` with today's `Board::apply`, files, clock and zone (DOC §21.3 unchanged), with `created` stamped at the origin and blocks addressed by sid. Its tiny_http server is off.
- **Replies** are sealed to the requester and signed, and carry the request's SHA-256, the board version and the hub's boot id. A client rejects a reply that matches no outstanding request or whose version is below one already seen. The hub persists its last 1,024 replies per device in a sealed file, so a retry after a restart gets the same answer; today's cache lives only in memory (phone.rs:287-406 [V]).
- **Revocation** needs no rekey, because sealing is pairwise. Change notices are signed and content-free. Whole-board answers are deflated and sent in numbered parts of at most 1 MiB, at most every 5 s, tested with a 10,000-row archive.
- **While the hub is off**, the Mac and the phone show their last state and queue edits, as today. The M1-only code (forwarding, the hub pump, `Remote::Tunnel`; about 2 weeks) is deleted at M2.

## 11. Risks and open questions

**Risks**
1. **Duration** (§10): value arrives at week 1, at M1 and at M2; Phases 8–9 can be cut, and M1 is a stable place to pause.
2. **Determinism bugs**: caught by the gates and beats, repaired by adopting a state chosen by fingerprint, and kept off M1's critical path.
3. **Our own crypto composition**, unaudited (§5.1).
4. **Cloudflare**: the DO incident rate rose in late 2026 [V R3], and the free tier could change; the exit plan is rehearsed in CI and yearly.
5. **Android churn**: developer verification in 2027; One UI's sleeping lists; whether Auto Blocker or Advanced Protection blocks adb [U]; whether `about:blank` iframes get the document-start script [U].
6. **chrono is unmaintained** (RUSTSEC-2026-0543): a reviewed ignore now, jiff later.

**Open questions**
1. Which phone, One UI version and Android version? This sets minSdk and the install procedure.
2. Is `/calendar.ics` subscribed from Google Calendar or the phone's calendar today? If so, Phase 5b includes the system-calendar export, and a live Google Calendar URL subscription cannot survive end-to-end encryption at EUR 0 (V3-3).
3. Are device-local zones acceptable, so that a travelling device shows local times?
4. Is the Windows desktop on overnight? The hub, snapshots and backups depend on it.
5. Does a `taskdeck-server` folder hold a newer board?
6. Is the repository public or private? This sets the CI minutes.
7. Is background freshness within 60 minutes enough, or is Google FCM acceptable later?
8. Are overwrite notices enough, or should stale `MoveBlocks`, `RemoveBlocks` and `SetDeadline` from a device offline for more than a day be refused (V2-5)?
9. Is a paid crypto review worth four to five figures [A]? A domain at about EUR 10 a year? A touch-to-sign token at about EUR 50?
10. Which phone extras (archive search, Forget, subscriptions, home place, palette) must exist before M2 counts as full functionality (V3-19)?
11. What should the reminder defaults and lock-screen text be?
12. How many hours a week can you give this?

**Still to measure**: DO CPU against the 10 ms cap; whether WAF-blocked requests count toward the quota; wake-from-hibernation latency; WebView cold start and StrongBox unwrap time; network access during allow-while-idle alarms in Doze; whether `setBlockNetworkLoads(true)` coexists with WebViewAssetLoader.

**Left out deliberately**: hot standby, automatic failover, mailbox relays, leaderless or CRDT designs (split-brain risk, or breaking DOC §4.1); anything that listens; web, PWA or iOS clients; a plaintext feed; FCM, Firebase and Play services; MLS and post-quantum wraps in v1 (the suite byte leaves room for X-Wing); Compose, Tauri or egui on Android; Play and auto-updaters; R2, KV, D1, workers-rs; any change to DOC §14.

## 12. Decisions log

| Decision | Alternatives (proposed by) | Why this won |
|---|---|---|
| D4's protocol, D1's Android surface, D3's reuse and pairing, D2's re-home | D1, D3 or D4 alone | fewest sync defects, smallest attack surface, cheapest M1 |
| First milestone is a relay hub | core first, phone on Tailscale until Android parity (D1, D3, D4, V3); today's server on a VPS (brief) | no Tailscale anywhere about 4 weeks sooner, for about 2 weeks of throwaway code; a VPS would hold plaintext and serve pages |
| Rules versions activated by a fleet-gated RulesUpgrade; thin apply | lowest-schema stamping (synthesis); per-entry rules stamps (V2-1); `MinSchema` (D4) | limits and defaults are consensus-critical; gating stops routine halts; no single device can wedge the rest |
| The recovery seed as the root of membership authority, never enrolling | a powerless sheet (synthesis); recovery signer and token (D2) | hostile revocations and recovery-key swaps can be overruled (V1-2, V1-5) |
| Typed code, explicit transcript, copy-and-enter | compare-and-confirm (synthesis); QR with CameraX and ZXing (D2, D4) | the user cannot just tap "match"; no camera dependency |
| Device-minted ids, replicated marks, `entry_id` reconciliation | temporary ids and remapping (D3) | JavaScript-safe, pipelined, no reissued ids after a restore |
| Device-local zones | a replicated board zone (D1, D2) | `apply` is zone-free either way; matches native calendars |
| No push in the plan of record | relay-sent FCM (D2); the credential on every device (D4) | whoever holds it can display notifications [V C1] |
| HPKE on one RustCrypto generation; our own recovery tool, offline | age (D2, D4); ring only (D3) | one generation, a static X25519 key, no code in cloud folders |
| M1 frames sealed pairwise, with persisted counters | a shared key and a 5-minute window (synthesis) | no rekey on revocation; no out-of-context replies |
| Notes: base hash, refuse, draft, merge | merge-append (D2); last writer wins (D3) | neither duplicates nor loses text |
| One sequencer; fenced re-home with move codes | a mailbox DO (D2); an auto-followed move (D4) | no split brain; one device cannot redirect the others |
| Relay in dependency-free JavaScript | workers-rs; TypeScript with npm (R3) | first-class path, no npm supply chain, Wasm cold start unmeasured |

## Verification outcomes

All three verifiers returned **sound with changes**; none found a critical issue. Every high finding is applied; every medium is applied or partly applied, with the rest as open questions; the lows are applied.

### V1: security attack (4 high, 8 medium, 7 low)

No critical path: nothing delivers code, and a relay alone cannot read or forge data. The theme was per-device authority: one compromised device could change keys, membership and history, and keep footholds after revocation.

| # | Finding | Outcome |
|---|---|---|
| 1 high | Login challenge was a signing oracle | per-use contexts; fixed login message; signed bad frames refused (§4.2) |
| 2 high | One device could swap the recovery key | old sheet, or two members and 72 h; banners; pinned fingerprints; per-writer tokens (§5.5) |
| 3 high | Rotation kept compromised keys | "Replace device" (§5.4) |
| 4 high | Loose attestation checks; shared caches | strict flags; cache-free releases; diff review; reproducible rebuilds (§8) |
| 5 | One device or token could lock out the rest | signed admin operations; recovery override; BIOMETRIC_STRONG (§5) |
| 6 | Vague SAS transcript, compare-and-confirm, code on the wall | explicit transcript; copy-and-enter; capture-excluded window (§5.3) |
| 7 | M1 replies unbound; volatile replay window | pairwise sealing; persisted counters and replies (§10.1) |
| 8 | Same-user code defeats key stores | development isolation; token signing (§8) |
| 9 | WebView exfiltration channels | inert frames; no `RTC*`; harness; fallback split (§7) |
| 10 | Snapshots, ResetTo, adoption unchecked | acks; local replay; adoption by fingerprint (§4.7, §4.8, §6.6) |
| 11 | A poison entry blocks revocation | Skip (§4.6) |
| 12 | Crypto spec gaps | epoch commitment; HPKE info; snapshot, file and token binding (§4.1, §4.7, §5.2) |
| 13–19 low | unsigned backups; decoders reached before checks; ICS Referer leak, publisher names and SSRF gaps; trusted relay fields; a weak sheet check; the legacy token; push delegation and lock-screen actions | all applied (§4.2, §4.3, §4.7, §5.5, §7, §9) |

### V2: sync and data integrity (3 high, 11 medium, 8 low)

V2 confirmed the base and audited 19 determinism hazards in today's `apply`. Its themes: consensus-critical rules, version skew, duplicates after crashes and restores, and responses to alarms.

| # | Finding | Outcome |
|---|---|---|
| 1 high | Validation and defaults unversioned | thin apply; rules versions; boundary golden logs (§4.5) |
| 2 high | State hash unstable across versions | canonical encoding v1; versions in beats (§4.5) |
| 3 high | Crashes and restores duplicate commands, reissue ids | seal, fsync, send; `entry_id` reconciliation; id marks (§4.3, §4.4) |
| 4 | Zone reads left in validation and wire types | UTC types; log-command types; routine and span ops (§4.5) |
| 5 | Stale offline edits overwrite silently | seen-block removal; skips; notices; refusal is open question 8 |
| 6 | Version skew freezes the phone | fleet gating; halted-device rules (§4.5) |
| 7 | Snapshots unvalidated | acks at aligned S (§4.7) |
| 8 | Control-entry races | first rotation wins; ordered re-seal; `device_no` at apply (§4.4, §5.4) |
| 9 | No rollback or fork response | stop and show; restore tail; weekly bases (§4.8, §6.7) |
| 10 | Silence alarm fires routinely | last seen plus evidence (§4.8) |
| 11 | Cutover strands M1 queues | preconditions; queue import; lock check (§9) |
| 12 | Backup size; no restore base | small nightly files plus segments; weekly bases (§6.6) |
| 13 | Deleted data persists | retention stated; purge (§4.7) |
| 14 | Poison-pill handling | page markers; two-start rule; trial apply (§4.5) |
| 15–22 low | compaction stranding a device; transient refusals; the naming hold; M1's inherited hazards; load-time normalisation; unbounded clock skew; identity edge cases; 304 freezing the overlay | all applied, V2-20 modified (§4.1, §4.3, §4.5–§4.7, §5, §10.1) |

### V3: requirements and feasibility (1 high, 7 medium, 11 low)

V3 compiled the crate set, confirmed the citations, the quota arithmetic and the adb exemption, and found no requirement unmeetable. Its central finding was effort.

| # | Finding | Outcome |
|---|---|---|
| 1 high | Effort, calendar and upkeep underestimated | 30-hour basis, contingency, dates; Phase 5 split; hardening moved to Phase 7; documentation line; upkeep; automated drills (§10) |
| 2 | Older phones halt on new schema | fleet-gated RulesUpgrade (§4.5, §6.5) |
| 3 | Feed retired without replacement | exports in 5a and 5b; the loss stated (§3) |
| 4 | Backups exceed free storage | resized (§6.3, §6.6) |
| 5 | Outside review unbudgeted | spec, vectors, public review; M1 claim corrected (§5.1, §7) |
| 6 | Android page work under-scoped | itemised in 5b (§10) |
| 7 | Retention changes Forget | as V2-13 |
| 8 | Development builds not isolated | as V1-8 |
| 9–18 low | M1 frame caps and latency; `http://` refusal breaking Tailscale setups; CI minutes; the desk picture reaching WebView decoders; Doze budget and SDK gaps; the silence alarm; printing the seed; runbook errors; what `libtaskdeck.so` links; the random hostname | all applied (§2, §4.9, §5.5, §6.1, §7, §8, §10) |
| 19 low | Desk-only features on Android | Phase 8; open question 10 |

### Rejected or modified verifier findings

- **V2-20, stamping with a relay-learned clock offset: modified.** A hostile relay could then shift `created` and archive times on every device. Relay time never stamps; an hour of skew blocks composing until the user confirms the clock.
- **V2-13, notes as a separately keyed blob overwritten in place: not adopted.** Notes would become the one mutation outside the command log, against the ground rules; retention limits and "Purge history" give real deletion instead.
- **V2-5, refusing stale edits by age: deferred** to open question 8. The structural fixes and notices are adopted; a deterministic age would rest on origin clocks, and refusal would drop legitimate offline work.
- **V1-12, a separate raise-only `MinCryptoVer` control: folded into RulesUpgrade.** A separately signed floor would be another lever for wedging devices, the reason `MinSchema` was dropped; the fleet gate now covers envelope versions.
- **V1-4, a local rebuild per release: scoped** to `libtaskdeck.so` and `taskdeck-recover`, which can be made reproducible; bit-for-bit MSVC and Apple builds across machines are a project of their own [A].
- **V1-9, the two-app split: kept as the fallback**, since it doubles the APKs, signing and IPC; it becomes the plan if the Phase 3 harness finds a channel that cannot be closed.
- **V1-5, no PIN fallback: Android only.** The desktops keep Windows Hello and Touch ID with their usual fallbacks; they stay at home, and the Windows Hello PIN is TPM-bound [A].
- **V3-5, a paid review: not budgeted by default**, as it conflicts with EUR 0; the spec-and-vectors path is adopted, and a paid review is open question 9.
- **V3-18, a custom domain from day one: optional**, to keep EUR 0; renames go through the fenced re-home.
