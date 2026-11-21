# V3: Requirements and feasibility (adversarial verification)

> Working paper from the design run of 2026-11-20 that produced [`SYNC.md`](../../../SYNC.md). It is an agent's report, kept for the detail the design compresses: file and line references, sources, and the reasoning behind each requirement. Where it disagrees with `SYNC.md` or [`SYNC_PLAN.md`](../../../SYNC_PLAN.md), those win. Line numbers refer to `day-planner` at commit `1cfff2f`.

Verdict: **sound with changes**. Issues are numbered as `SYNC.md` cites them (V3-1, V3-2, …). Each was raised against the draft design; `SYNC.md`'s *Verification outcomes* says what changed.

## V3-1 · high · Effort, calendar time and ongoing upkeep are underestimated for a one-person project

**Scenario.** The user plans around M1 at about week 12 and M2 at about week 21. Phase 5 alone is meant to deliver, in 4–4.5 weeks:
- the relay log routes
- the sans-IO engine (confirmed/pending/rebase, counters, echo check, heartbeats, alarms)
- the sealed store
- both desktops and the phone as replicas
- JobScheduler jobs, the Health screen and Samsung onboarding
- ICS on the phone, and the desk-picture blob
- the cutover tool, and deletion of four subsystems
- the standby adapter
- a network simulator running 10,000 schedules over 12 fault classes
- an adversarial relay test harness

**Why it fails.** Bottom-up (ASSUMED), the phases come out at:
- Phase 5: about 8–12 focused weeks.
- Phase 1 (seal crate, relay with tickets/registry/tokens/revoke/budgets/forwarding, DPAPI/Keychain, HPKE backups, recovery CLI, KAT/tamper/fuzz tests): about 4–5.
- Phase 2: about 3–4.
- Phase 3: R2's own +1–2 weeks if Android is new to the developer is missing.

The plan also leaves out:
- rewriting DOCUMENTATION.md (3,492 lines), SERVER.md and MOBILE.md, which this project's documentation culture requires;
- the outside review;
- real-device Samsung testing.

That makes roughly 30–40 focused weeks, not 22.5–25.5, and well over a year at hobby pace. No hours-per-week basis or calendar dates are given.

The end state also carries recurring work that is never estimated:
- monthly updates on three devices, including the adb ritual;
- a quarterly restore drill and a yearly re-home rehearsal;
- the cargo-vet backlog and Gradle verification-metadata refreshes;
- a renewal register.

The brief makes maintenance a first-class cost. A project that stalls between M1 and M2 leaves the user dependent on the desktop hub, with no uptime gain, indefinitely.

**Required change.** - Re-estimate bottom-up with an explicit hours-per-week basis and calendar dates, plus 30–50% contingency.
- Split Phase 5 into 5a (desktops become replicas with a minimal engine) and 5b (the phone).
- Move the simulator at scale, the adversarial relay, the standby adapter, the heartbeat state hashes and the fenced re-home into Phase 7.
- Define a minimal M2 that still meets the four requirements.
- Add a line item for rewriting the documentation.
- State ongoing upkeep in hours per month. Automate the restore drill and the re-home rehearsal in CI so they actually happen.

**Evidence.** - Design §10 table: P0 1, P1 3–3.5, P2 2–2.5, P3 5–6, P4 4–4.5, P5 4–4.5, P6 2, P7 1.5 = 22.5–25.5.
- R1 effort table: readiness 11–17 days plus 3–5 days for the sync client.
- R2: 'add 1–2 weeks if Android is new'.
- wc -l DOCUMENTATION.md = 3492 [VERIFIED].
- git log main..HEAD = 81 commits [VERIFIED]: the developer iterates heavily.

## V3-2 · medium · Lowest-schema stamping still halts an older phone's entire sync, so 'same-day updates are not a necessity' is not true in practice

**Scenario.** The user updates both desktops (easy) and uses a feature that needs a new op or field on the wall calendar. The phone has not yet had its cable update.

**Why it fails.** - By §4.8 the phone stops applying at that seq, and entries must be applied in order. No later entry from any device reaches it until it is reinstalled over adb (developer options, USB debugging, Auto Blocker off, cable).
- With this project's pace of feature work, that becomes a routine loss of phone sync, and it is not counted in the 99.7–99.9% availability figure (§6.5), which covers relay incidents only.
- The beat entries already carry each device's highest readable schema (§4.2), but nothing uses them to gate writers.

**Required change.** - Writers must not emit an op or field whose 'since' schema exceeds the minimum readable schema of active members, taken from their latest beats.
- Hold the feature back in the UI ('available once Galaxy is updated') rather than halting readers.
- Only drop a long-silent member from that minimum with user confirmation.
- Redefine sync availability end to end, so it includes client halts and the phone's update latency.

**Evidence.** - Design §4.8: 'stops applying at that seq … keeps sending'.
- Design §4.2: beat contents include 'the highest schema the device can read'.
- Design §8: Android release steps 4–6, and 'Updating all devices on the same day is a habit, not a necessity'.
- R1 wirecompat [VERIFIED]: older builds silently drop unknown fields, so halting rather than skipping is required.

## V3-3 · medium · The calendar feed is retired at M1 with no replacement in the plan of record, yet M2 is said to meet every requirement

**Scenario.** The user subscribes to /calendar.ics today from Google Calendar, Samsung Calendar or a watch. §9 step 5 tells them to remove that subscription at M1.

**Why it fails.** - The README promises that the feed shows 'your events, due dates, day plan and routines … read-only, and still there when the desktop is off'.
- Both replacements, the CalendarContract export and the desktop .ics export, are in optional Phase 8. Phases 0–7 therefore drop the feature outright, which contradicts '§10 M2: every requirement is met' and the user's 'full functionality'.
- A live Google Calendar (cloud) subscription cannot be provided end-to-end encrypted at EUR 0 at all. The design never says this explicitly to the user.

**Required change.** - Resolve open question 5 before M1.
- If the feed is used, move the Android CalendarContract export (one-way, own account type, as in R2 §6) into Phase 3 or 5, before the feed is turned off. Keep a desktop 'Export .ics…'.
- State plainly that a live Google Calendar web subscription is lost, and get the user's explicit acceptance (or keep the old desktop feed over Tailscale for that one consumer until they decide).

**Evidence.** - README.md:81-86 [VERIFIED].
- DOCUMENTATION.md §21.6.
- Design §3, feed row.
- Design §9 step 5.
- Design §10 Phase 8: 'CalendarContract export; desktop .ics export' (optional).

## V3-4 · medium · Backup sizes contradict the design's own storage figures, so the retention plan does not fit free cloud storage

**Scenario.** Nightly backups are written as specified, each holding the canonical BoardData and 'the 400-day frame log'. 30 daily and 12 monthly copies are kept in OneDrive or iCloud Drive.

**Why it fails.** - §6.2 sizes 365 days of log at about 0.15–0.3 GB. Frames are padded AEAD ciphertext and do not compress, so each backup is about 0.15–0.3 GB, not '≈ 5 MB'.
- 42 copies come to about 6–13 GB. The free tiers of OneDrive and iCloud are 5 GB, and iCloud's is often already used by phone backups.
- This breaks either EUR 0 or the retention scheme, and adds hundreds of MB of upload every night.

**Required change.** - Make the daily backup a snapshot plus the plaintext commands since that snapshot, deflated and then HPKE-sealed (a few MB).
- Keep the full frame log only in the monthly copies on the local second disk and the offline USB.
- Recompute and state the size for each destination.
- Test restore-to-seq from this format.

**Evidence.** - Design §6.2 storage row: '≈ 0.15–0.3 GB steady state (365 days of log …)'.
- Design §6.3: 'Backups (≈ 5 MB each, about 42 kept) fit in existing free OneDrive or iCloud space [A]'.
- Design §6.6: backup contents include 'the 400-day frame log'.
- Design §4.2: Padmé padding plus XChaCha20-Poly1305.

## V3-5 · medium · The 'outside review' of the crypto is load-bearing but has no budget or schedule, and M1 ships without it

**Scenario.** M1 (about week 12) puts taskdeck-seal, pairing, the HPKE wraps and the backups into daily use. The review is promised only 'before M2'.

**Why it fails.** - The review is cited as a mitigation in §5.1, in §7 residual risk 6 and in §11 risk 3, but no phase, reviewer or cost appears anywhere.
- A meaningful independent review of a custom envelope plus pairing protocol typically costs four to five figures (ASSUMED). That conflicts with a EUR 0, cost-sensitive project.
- §10 says M1 'is as safe as the end state, apart from residual risk 5'. That cannot hold while the composition is unreviewed.

**Required change.** Choose one of two paths and write it down:
- (a) Budget and schedule the review: scope, rough cost, and before M1 for the parts M1 uses.
- (b) Drop it from the risk argument and shrink what is novel: a written spec, standard constructions only, test vectors published in the repo, a public request for review, and residual risk 6 restated honestly.

Either way, correct the M1 safety claim.

**Evidence.** - Design §5.1: 'taskdeck-seal gets an outside review before M2'.
- Design §7, residual risk 6.
- Design §11, risk 3.
- Design §10: no deliverable for the review in any phase; the M1 paragraph makes the 'as safe as the end state' claim.

## V3-6 · medium · Android parity at M2 needs page work beyond the '≈ 60 lines' shim: preferences, staleness, async refusals and notes conflicts

**Scenario.** At M2 the WebView runs with DOM storage off, and every answer comes from the in-process core.

**Why it fails.** - **Preferences reset on every launch.** The page keeps these in localStorage (phone.html:1043-1063, 2025, 2740-2811, 3946-3950): the chosen view ('taskdeck-mode'), Don't warn, the phone's own weather place, the weather cache and the sky-warm flag. try/catch only prevents a crash, so 'Use my location' must be tapped again after every cold start.
- **Staleness is never shown.** 'As of' is driven by failed fetches and kept snapshots (keptSince at 1415; banner at 1740-1755). A local snapshot always succeeds, so a phone that has not reached the relay for two days shows no age at all. That breaks the deliberate rule at DOCUMENTATION.md:2817 that staleness cannot be silenced.
- **Async refusals never reach the page.** Refusals are reported only from HTTP replies (1560-1583). At M2 they surface later, when the sequenced entry is applied.
- **Notes conflicts have no UI.** set_notes is sent without a base (4727), and the §4.6 rule (base, refuse, draft, merge) has no merge view on the phone.

**Required change.** Specify and estimate the M2 page work:
- a native key-value store over the bridge for a whitelist of page preferences, or DOM storage limited to non-board keys;
- a sync-status line pushed into the masthead ('synced 14:05' / 'not synced since Tue') that cannot be silenced;
- a bridge event that shows async refusals through the existing notice banner;
- set_notes sending a base, plus a conflict sheet on the phone;
- a 'will be refused' mark on pending rows.

**Evidence.** - phone.html line numbers as cited [VERIFIED by reading].
- DOCUMENTATION.md:2817-2840.
- Design §5.6: 'WebView DOM storage off'.
- Design §4.6 and §4.7.
- Design §10 Phase 3: 'bridge shim (≈ 60 lines of page changes)'.

## V3-7 · medium · Log and backup retention silently changes what Forget and a notes overwrite mean, and widens what one stolen device exposes

**Scenario.** The user types something sensitive into a task name or the notepad, then Forgets the task or deletes the text.

**Why it fails.** - Today Forget is 'for the rare thing you want gone for good', and the notes file is overwritten atomically.
- In the design, every Add, Rename and whole-text SetNotes frame (up to 20,000 characters) is kept:
  - for 400 days in every device's log;
  - for up to 365 days on the relay;
  - in 30 daily and 12 monthly backups, plus an offline USB copy.
- The user is never told about this change in a feature's meaning, and it enlarges what malware or a thief on any one device obtains.

**Required change.** - Document the change in the UI.
- Give Forget real effect: after a covering snapshot, prune the frames that name a forgotten id on the relay and on every device, and keep them out of later backups. Alternatively, use per-item content keys that Forget destroys.
- Prune SetNotes frames older than N days once a snapshot covers them.
- State how long backups keep forgotten data.

**Evidence.** - README.md:107 [VERIFIED].
- src/board.rs:48: NOTES_MAX_CHARS = 20_000 [VERIFIED].
- Design §4.1: devices keep 400 days of frames.
- Design §4.7: compaction only after 365 days.
- Design §6.6: backup contents and retention.

## V3-8 · medium · Build isolation covers release builds only, not the user's day-to-day development builds on the key-holding machine

**Scenario.** The user runs cargo build or a Gradle sync for TaskDeck in the Windows account that runs the wall calendar and holds the DPAPI-wrapped board keys.

**Why it fails.** - A poisoned build.rs or Gradle plugin runs with that account's rights. This is exactly the attack R4 documents: arrayref 0.3.10, which persisted through an HKCU Run key.
- §8 describes isolation (secret-free CI, 'a dedicated non-admin build account') only for releases. Development and test builds are where most compiles happen.
- The lockfile, cooldown and deny gates reduce the risk but do not isolate it.

**Required change.** - Make it a stated rule that no compiles of TaskDeck or its dependencies (cargo, Gradle, cargo-ndk) run in an OS account that holds TaskDeck keys.
- Develop in cloud containers, a VM or a separate local account, against a separate development board, never the real one.
- Apply the cooldown and deny gates to development too.

**Evidence.** - Design §8: Source, CI and Windows paragraphs.
- R4 §4.2: arrayref compromise.
- Cargo.lock:160-161 pins arrayref 0.3.9 [VERIFIED].
- README: 'developed on Windows 11'.

## V3-9 · low · M1 hub traffic: frame caps and chunking are unspecified, whole-board replies outgrow them, and M1 latencies are not stated

**Scenario.** The Mac's whole-board fetch (items, archive and notes) is forwarded through the relay. The archive keeps every row ever written.

**Why it fails.** - Log frames are capped at 256 KiB and tungstenite messages at 2 MiB, but hub request/response frames have no stated cap or chunking. Thousands of archive rows exceed 256 KiB even deflated.
- The §4.9 latency table describes the end state only. At M1 the phone's queue lives in the page and is sent only while the WebView is running.

**Required change.** - Specify a chunked hub frame type with parts of at most 1 MiB, or answer with snapshot plus delta.
- Test with a 10,000-row archive.
- Add an M1 behaviour and latency table, including 'phone edits leave only while the app is open'.

**Evidence.** - Design §4.2 caps.
- Design §10, 'How the hub works': 'Whole-board fetches are deflated and coalesced'.
- DOCUMENTATION.md §21.4 ('ArchiveLog holds every line ever written') and §22.3 (/api/board returns items, archive and notes).

## V3-10 · low · Phase 0's 'http:// refused' breaks today's documented Tailscale setup

**Scenario.** The user follows SERVER.md and sets server_url = "http://100.x.y.z:7373". The Phase 0 hygiene release then refuses it.

**Why it fails.** - Phase 0 is meant to make today's system safer within a week. Instead it stops the desktop syncing, using a configuration SERVER.md documents as valid ('server_url can use either form').

**Required change.** - Until M1, allow http:// only to loopback and the Tailscale ranges (100.64.0.0/10, fd7a:115c:a1e0::/48), with a warning.
- Or rewrite the setting to the https ts.net name automatically.

**Evidence.** - SERVER.md:188 and :369 [VERIFIED].
- src/sync.rs:132-136 accepts http:// [VERIFIED].
- DOCUMENTATION.md §21.7.

## V3-11 · low · The CI plan contradicts its own minutes budget on a private repository

**Scenario.** The repository is private, and every push runs the golden logs on Windows, macOS and Linux.

**Why it fails.** - §8 says 2,000 free minutes cover about one full release a month because macOS minutes count 10×, yet it also runs macOS and Windows jobs on every push.
- Frequent agent pushes would exhaust the minutes within days, and CI then stops (fail-closed, but the supply-chain gates stop with it).

**Required change.** - Run the per-push suite on Linux only.
- Run the Windows and macOS golden logs nightly or on tags.
- Alternatively, make the repository public.
- Answer open question 6 before Phase 0.

**Evidence.** - Design §8, CI paragraph.
- docs.github.com/en/billing/concepts/product-billing/github-actions: 2,000 minutes on Free; macOS ×10, Windows ×2. Third-party reports of 2026 pricing changes are unconfirmed.

## V3-12 · low · 'Synced bytes never reach a platform image decoder' is contradicted by the desk-picture blob shown in the WebView

**Scenario.** The phone uses 'use the desk's' background, which arrives as a sealed blob and is shown by the page as /bg-<hash>.jpg.

**Why it fails.** - Chromium's decoder in the WebView parses bytes authored by another member device. A compromised desktop could send a crafted JPEG (the libwebp class of bug), contrary to the §7 claim.

**Required change.** - Decode the blob in Rust under image::Limits and re-encode it before handing it to the WebView.
- Or amend the claim and residual-risk text.

**Evidence.** - Design §7 ('Synced bytes never reach a platform image decoder') versus §3 ('use the desk's' through one sealed blob).
- phone.html:2394-2396: photo fetch and cache [VERIFIED].

## V3-13 · low · Android background details: the freshness alarm competes with reminders for the Doze budget, the Doze target rests on an unverified assumption, and there are SDK-level gaps

**Scenario.** Phase 6 schedules an hourly allow-while-idle freshness alarm alongside reminder alarms.

**Why it fails.** - In Doze an app gets at most one allow-while-idle alarm per 9 minutes. A freshness alarm can therefore push back a reminder due soon after it.
- The '≤ 60 min' Doze freshness target assumes network access during such alarms (R2 marks this ASSUMED).
- With minSdk 29:
  - setExpedited jobs need API 31;
  - USE_EXACT_ALARM needs API 33;
  - on API 31–32, SCHEDULE_EXACT_ALARM is needed and is missing from §2.1's permission list.

**Required change.** - Never schedule a freshness alarm within 9 minutes before a reminder; sync at reminder firings instead.
- Commit only to 'fresh on open' until the Phase 6 Doze measurement passes.
- Add SCHEDULE_EXACT_ALARM with maxSdkVersion=32, or raise minSdk to 31 or 33 once the phone model is known.

**Evidence.** - R2 key facts (Doze: once per 9 minutes; WorkManager/JobScheduler deferred).
- Design §2.1 permissions, §4.9, and Phase 6.
- R2 recommended SCHEDULE_EXACT_ALARM with maxSdkVersion 32.

## V3-14 · low · The 48-hour silence alarm will fire routinely for devices that are legitimately offline

**Scenario.** The laptop is shut in a bag for a long weekend, or the desktop is off during a trip.

**Why it fails.** - Beats are written only while a device runs. Mere silence therefore says nothing about withholding by the relay.
- False alarms teach the user to ignore the integrity alarms the design relies on.

**Required change.** - Raise the alarm on evidence of withholding, not on silence alone. Examples: this device's own acknowledged appends are not echoed, or the relay's welcome head contradicts another device's signed seen-head.
- Show per-device 'last seen' times instead.

**Evidence.** Design §7, relay-tampering row ('a 48-hour silence alarm'); design §4.2, beat kind.

## V3-15 · low · How the recovery sheet gets printed is unspecified, and the obvious route leaks the key to disk

**Scenario.** Setup asks the user to 'print the sheet twice on a USB-connected printer' from the egui app.

**Why it fails.** - egui and winit have no print API. The natural implementation writes an HTML or PDF file and opens it, which leaves the recovery private key in temp or Downloads folders (possibly synced to OneDrive) and in spooler files.
- Many users have no printer at all.

**Required change.** - Specify the path: show the key full-screen and excluded from capture for hand transcription, or print through an OS print API from memory, and never write it to a file.
- Keep the retype check.

**Evidence.** Design §5.5; §5.6 (WDA_EXCLUDEFROMCAPTURE only for screens).

## V3-16 · low · Distribution runbook inaccuracies

**Scenario.** The user follows §8 on Windows and Android.

**Why it fails.** - SmartScreen judges each new unsigned binary separately, so it warns on every release, not 'once'. Smart App Control, if enabled, blocks unsigned executables outright.
- §2.1's claim that Android ships 'nothing else' is overstated. androidx.webkit pulls in androidx.core and its transitive set, and the rustls-platform-verifier-android artefact must be vendored too (it is a dependency on Android).
- If the user follows R4's advice to turn on Android Advanced Protection, that mode disables 'install unknown apps'. Whether it also blocks adb is unknown.

**Required change.** - Correct the SmartScreen step, and check the Smart App Control state.
- List the full Gradle dependency set in the verification metadata.
- Warn against enabling Android Advanced Protection mode, or test adb install under it on the user's phone.

**Evidence.** - Design §8 ('Accept SmartScreen once') and §2.1.
- crates.io: rustls-platform-verifier 0.7.1 depends on rustls-platform-verifier-android ^0.2.0 on Android [VERIFIED].
- androidauthority.com/android-16-advanced-protection-mode-3518368 and bayton.org Advanced Protection FAQ (installs from unknown sources blocked).

## V3-17 · low · The packaging table does not say what libtaskdeck.so links

**Scenario.** Phase 3 and Phase 5 build the Android cdylib.

**Why it fails.** - §2.3 places the sync engine, sealed store, transport, ICS and weather fetchers, and the backdrop processing in task_deck, which 'ships in' only the desktop builds and the recovery CLI.
- taskdeck-android lists only jni. The phone needs all of the former.

**Required change.** - State that taskdeck-android depends on task_deck with --no-default-features, or create a taskdeck-sync crate.
- Add a CI job, cargo ndk check, from Phase 3 onwards.

**Evidence.** - Design §2.3 table.
- R1 [VERIFIED]: the lib type-checks for aarch64-linux-android once rustls is removed; ring needs the NDK.

## V3-18 · low · The random workers.dev hostname makes both pairing and quota-DoS recovery awkward

**Scenario.** Pairing requires typing the relay host plus a 16-character code on the phone. After a repeated quota attack, §6.4 says to 'rename the Worker'.

**Why it fails.** - A random Worker name on the account subdomain is long and error-prone to type.
- Renaming the Worker changes the URL, and no flow tells devices about it short of the full fenced re-home with a move code typed on each device.

**Required change.** - Either adopt the custom domain from day one (about EUR 10 a year; keeps a stable hostname and enables the WAF gate), or define a signed relay-address entry so a rename reuses the Fence plus move-code flow.
- Let the host, though not the code, be pasted.

**Evidence.** Design §5.3 step 2; §6.1 (random name); §6.4 quota row; §6.7 re-home.

## V3-19 · low · 'Full functionality' leaves desk-only features on Android unconfirmed with the user

**Scenario.** On a day with only the phone, the user wants to add a calendar subscription, search the archive, Forget a row, or change the home place or palette.

**Why it fails.** - §3 marks these as 'as today' or desk-only: subscription management (an optional Phase 8 manager), the archive limited to the Done tab, settings n/a, and the palette taken from the desk.
- At M2 the phone holds a full replica, so these are UI gaps the user has not agreed to.

**Required change.** - List the desk-only features in the requirements sign-off.
- Either add the cheap ones (an archive ledger/search view in the page, Forget with a double confirmation, setters for home place and palette), or get explicit acceptance.

**Evidence.** Design §3 rows; DOCUMENTATION.md §21.8 ('Become a second TaskDeck' deliberately not done).

## Confirmed strengths

- [VERIFIED] The named crates exist on crates.io at the stated versions: ureq 3.5.2, tungstenite 0.30.0, getrandom 0.4.3, hpke 0.15.1, rustls-platform-verifier 0.7.1, jni 0.22.4, ed25519-dalek 3.0.0, x25519-dalek 3.0.0 and chacha20poly1305 0.11.0. A scratch project with them resolves to a single RustCrypto generation: `cargo tree -d` shows only getrandom 0.2/0.4 (ring), syn 2/3 and windows-sys duplicates. A smoke test using XChaCha20-Poly1305 and `verify_strict` compiled and ran (scratchpad/r_verify/depcheck).
- [VERIFIED] rustls-platform-verifier 0.7.1 needs jni ^0.22, which matches jni 0.22.4. On Apple it pulls in security-framework, so the design's [A] that the Keychain crate 'arrives with the platform verifier' is true. On Windows it needs windows-sys <0.62, which is already in Cargo.lock at 0.52–0.61.
- [VERIFIED] tokio enters the tree only through reqwest (`cargo tree -i tokio`), so moving the fetchers to ureq 3 drops tokio and hyper as claimed. ureq 3.5.2 requires rustls ^0.23.46 and offers a platform-verifier feature.
- [VERIFIED] The phone.html claims hold:
- It sends exactly 17 distinct ops.
- Session values are only compared and echoed; sessionAt returns `found.index` (4928-4931).
- There are no bitwise operations on ids, so device ids below 2^53 are safe.
- It has a single innerHTML sink (1073) and no inline event-handler attributes, so a hash-based script-src and Trusted Types are feasible.
- Every localStorage access sits inside try/catch.
- All /api traffic goes through api(), apart from the photo, Open-Meteo and sky-SVG fetches and the service-worker registration at 5698-5699.
- [VERIFIED] The design's code citations are accurate:
- the CSP at phone.rs:1209-1219;
- phone.rs:887-928 forwarding any non-query Command;
- paths.rs:96-120 only warning on a contended lock;
- sync.rs:648-660 filing commands only between drains;
- ui.rs:2152 editing board.notes directly, with the 2 s autosave at ui.rs:7039-7047;
- Cargo.toml:94 `panic = "abort"` and build.rs:11 `Utc::now()`;
- reqwest 0.12.28's error Display appending ' for url (…)' (error.rs:268), stored by subscriptions.rs:545-550, so the leak is real;
- 136 `Local` mentions;
- windows 0.62.2 in Cargo.lock;
- arrayref pinned at 0.3.9.
- [VERIFIED] The §14.4 tie-break jitter is a splitmix64 mix of the id and the day (tasks.rs:487-503). Device-generated ids keep the daily shuffle intact, and the core needs no rand dependency for it.
- [VERIFIED] The official Android developer-verification FAQ (entries updated through 2026-11-10) says ADB installs need no registration and are not subject to the advanced flow's 24-hour wait. The design's adb-only update path therefore survives the 2027 global rollout.
- [VERIFIED] The Cloudflare DO limits page (modified 2026-11-13) confirms: SQLite-only storage on Free, 5 GB per account, 32 MiB maximum received WebSocket message, and up to 100 classes on Free. It still has the 1 GB vs 10 GB and 30 s vs 10 ms ambiguities, which the design correctly treats as uncertain and to be measured.
- The core architecture fits the requirements:
- one sequencer, with offline-first full replicas (about 100% local availability);
- a free plan that fails closed instead of billing;
- no code served by any host;
- authentication before parsing;
- hibernation-only DO usage;
- M1 turning off the tiny_http server, which removes the stalled-body freeze reproduced earlier.
- [VERIFIED] The quota arithmetic is internally consistent: about 220 Worker requests, about 240 DO requests, about 3 GB-s and about 1,500 rows written per typical day, all under 2% of the free limits. The 3-rows-per-append figure matches the schema (log row, UNIQUE index entry, device row).

## Notes

**Method.** I read README.md, DOCUMENTATION.md §1–4 and §21–23, the open items in CODE_REVIEW.md (E/G), SERVER.md §2/§6, and the relevant parts of src/phone.html, phone.rs, sync.rs, paths.rs, tasks.rs, ui.rs and subscriptions.rs.
- I ran `cargo tree` read-only in the worktree. The tree is unmodified.
- I resolved and compiled a scratch dependency set in <scratch>/r_verify/depcheck (target directory: scratchpad/r_verify/depcheck-target).
- I queried the crates.io API and fetched the Cloudflare DO limits page and the Android developer-verification FAQ.

**Still unverified:**
- whether the Workers Free 10 ms CPU cap binds DO invocations;
- network access during allow-while-idle alarms in Doze;
- whether adb works under Android Advanced Protection mode or Samsung Auto Blocker;
- whether `WebSettings.setBlockNetworkLoads(true)` coexists with WebViewAssetLoader interception (ASSUMED yes). Worth testing as a cheap extra layer behind CSP and the 404 interceptor.

**Smaller implementation notes, not raised as issues:**
- Answer `/api/wait` through the async bridge, not a `shouldInterceptRequest` call that blocks a thread.
- Natively answered Open-Meteo responses need Access-Control-Allow-Origin, because the page origin differs.
- Windows Hello from a Win32/egui app needs `IUserConsentVerifierInterop::RequestVerificationForWindowAsync` with the HWND.
- Artefacts downloaded on macOS carry quarantine. Sequoia removed the right-click bypass, so either download with gh or curl, or clear the quarantine attribute.
- App Nap can delay the Mac's 30 s pings past Cloudflare's idle close. That only costs reconnects.
- StrongBox unwrap latency counts against the 0.7 s cold-start target; a TEE-backed key may suffice.
- Keeping a block local while it is being named contradicts the rule that the outbox is fsynced before apply returns.
- README still claims Linux support, but the design gives Linux no key store.

**Ordering.** An alternative worth a sentence in the plan: Phase 0, then 4 (pure, network-free), then 1, then 5a (the desktops become replicas). This gets the desktops uptime-independent sync earlier and skips about 1.5 weeks of throwaway hub code, at the cost of the phone staying on Tailscale longer. The design's choice, getting the phone off Tailscale first, is defensible if the user prefers it.

**Verdict.** No requirement is structurally unmeetable, and the protocol, platform and crate choices hold up. The changes needed are mainly:
- realistic effort, with a smaller M2;
- gating writers by the schema every device can read;
- an in-plan feed replacement (or explicit acceptance of its loss);
- corrected backup sizing;
- an honest crypto-review plan;
- the M2 page work on Android.
