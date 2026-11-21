# Stage `foundation`: status

**State: in progress** (wave 0). Lead session started 2026-11-21 on branch `claude/focused-volta-tbqufb`.
This file is the stage's memory (SYNC_PLAN.md §3.5). A fresh session resumes from it alone:
read SYNC_PLAN.md, then this file, then continue at the first task below that is not `done`.

## How this stage is run

- Workflows of Opus agents (`model: 'opus'`), at most two agents at a time, one workflow at a time.
- Agents write; the lead runs every gate and reads CI itself. Agents never commit or push.
- Every implementation task goes through the task loop of SYNC_PLAN.md §7 (implement, attack through
  three lenses, fix, until two dry rounds). Spec work uses drafters, pairwise interface reviews and
  the two-readers test (§6.1).
- Pull requests go from `claude/focused-volta-tbqufb` into `day-planner`, merged with a merge commit
  so the branch stays valid for the next one. The exit pull request is the one that sets this file
  to `final`.

## Tasks

| # | Task (SYNC_PLAN.md §6.1) | State | Evidence |
|---|---|---|---|
| 0 | Status file, draft pull request | done | this file; draft pull request TT-SAL/taskdeck#1 |
| 1a | Toolchain pin, lockfile refresh, `panic = "unwind"` with its release test, `build.rs` date | implemented, with the panic hook of D11. Three loops so far: 5 rounds (14 findings), 5 rounds (9), 3 rounds (5); every finding fixed or ruled on. No finding against the product changes themselves since the hook landed; all recent ones are about which hypothetical edits the checks detect, now a closed list (D14). Not yet two dry rounds: the loop continues after the usage window resets | commits `a743020` onwards; `scripts/check-release-panic.sh`, `scripts/check-build-date.sh`; full CI matrix green on `eef1ba0` |
| 1b | `deny.toml`, cargo-deny, cargo-audit, seven-day cooldown | todo | |
| 1c | CI: path filters at job level, aggregate `gate` job | todo | |
| 2 | Workspace: member crates with every planned dependency pinned; `relay/`, `android/` READMEs | todo | |
| 3a | Spec conventions (`docs/sync/spec/README.md`) | done, read in full by the lead | commit `460aa26` |
| 3b | Spec drafts: core, seal, relay, engine, device, bridge | seal drafted (2,038 lines, 155 clauses; the lead read §3–§10, §14, §17); core drafted (1,622 lines, 93 clauses; the lead read the report and §14.3); relay drafted and finished (1,846 lines, 166 clauses; typical day recomputed at 199 Worker requests, 215 object requests, 1,524 rows written, 4,217 rows read, all under 2%); engine drafted (2,390 lines, 182 clauses, with the outcome of each of V2's fifteen scenarios); device drafted (2,085 lines, 146 clauses); bridge drafted (1,655 lines, 110 clauses). All six are first drafts: 11,635 lines, 852 clauses | `docs/sync/spec/*.md` |
| 3c | Pairwise interface reviews (seal–relay, seal–engine, core–engine, engine–relay, device–bridge, device–engine) | todo | |
| 3d | Two-readers test, six scenarios, until two rounds agree (at most four) | todo | |
| 3e | Requirement map in `spec/README.md` (SYNC.md §3–§9 to clause, stage or plan §2 row) | todo | |
| 4 | Skeleton crates: public types and traits, toy state machine, fake sealer | todo | |
| 5 | Fixtures as data: boards, legacy `main` formats, snapshot JSON, transcripts | todo | |
| 6 | Tooling: `scripts/gate.sh`, `scripts/spec-coverage.sh`, `scripts/dev-setup.sh` | todo | |
| 7 | Stage exit review, exit pull request, merge | todo | |

## Next steps (as of 2026-11-21 19:10 UTC)

In this order; a fresh session picks up at the first one not done.

1. **Interface reviews of the six spec drafts**: nine pairs (the plan's six plus core–device,
   seal–device, relay–device), each reviewer also checking that the rulings S1 to S31 are reflected;
   then one resolver writes the exact change per spec; then one fixer per spec applies it. Repeat
   until a round brings nothing of substance.
2. **Task 1a's attack loop** again from round 1, with the closed mutation list of D14, until two
   dry rounds. The task's brief and the lead's seven rulings are summarised in D11, D12 and D14.
3. **Task 1b** (deny.toml, cargo-deny, cargo-audit, the cooldown script, `scripts/dev-setup.sh`,
   `scripts/supply-chain.sh`), then the lead decides on any advisory left in the lockfile, then its
   attack loop.
4. **Task 1c** (CI by path, per-stage jobs with hooks, the `gate` job named `gate` only on
   pull-request runs), pushed early so real CI runs can be read; then its attack loop.
5. **Merge the baseline pull request** (TT-SAL/taskdeck#1) once 1a to 1c have converged and CI is
   green; open the next one for specs, workspace, fixtures and tooling.
6. **Task 2** (workspace crates and pinned dependencies), **task 5** (fixtures), **task 6**
   (tooling), each with its attack loop.
7. **Two-readers test** on the reviewed specs (six scenarios, two rounds without a difference or four
   rounds), then the **requirement map** in `spec/README.md`, then **task 4** (skeleton crates from
   the specs' Public API sections, toy state machine, fake sealer).
8. **Stage exit review**, status `final`, exit pull request, merge; tell the owner to start wave 1.

Usage: the account's five-hour window allows about 60 agent runs. Each workflow is sized to about
20 runs, and no workflow is started once a window is about four-fifths used (D13).

## Decisions (with reasons)

- **D1. Agent model.** `model: 'opus'` (Opus 5.5), as the owner's start prompt says.
- **D2. Pull requests.** Several, merged in order with merge commits (baseline first), instead of one
  long-lived one: the CI restructure is best proven by a real pull-request run early, and
  `day-planner` gains the `gate` check the owner may protect it with (§1.4).
- **D3. Cooldown without a new tool.** The crates.io sparse index publishes a `pubtime` for every
  version (checked 2026-11-21), so the seven-day cooldown is a script of our own over `Cargo.lock`.
  `cargo-cooldown` is not on §3.4's pre-approved list and is not needed.
- **D4. Toolchain pin: Rust 1.97.0.** It is the toolchain the cloud containers ship, so stages
  start without a download; today's code is tested on it here.
- **D5. Fixed-width integers in every wire and file layout are big-endian.** SYNC.md gives layouts
  without a byte order; HPKE (RFC 9180) and the relay's `DataView` default agree on network order.
- **D7. tokio moves forward, to 1.53.3.** RUSTSEC-2026-0386 is patched in 1.51.6 (an older line) and
  from 1.53.3. A refresh does not downgrade, so the lowest patched release above today's 1.52.3 is
  taken. Cargo also re-pointed the `windows-sys` edge of six crates whose own versions did not move;
  those entries were put back, so the lockfile diff is the four crates and nothing else.
- **D8. `build.rs` names its inputs.** Printing `rerun-if-env-changed=SOURCE_DATE_EPOCH` switches
  off cargo's default (rerun when any package file changes). Instead of scanning the directory to
  imitate it, the script lists `build.rs`, `resources.rc`, `icon.ico`, `Cargo.toml`, `Cargo.lock`
  and `src`. `BUILD_DATE` is refreshed when the app's own inputs change, no longer when an unrelated
  file does. Short enough to review before signing a tag (SYNC.md §8), and later directories such
  as `fixtures/` and `android/` do not trigger rebuilds of the app.
- **D9. The release-panic test is a probe binary.** `cargo test --release` always unwinds, so it
  proves nothing about the profile. `crates/panic-probe` is built with `cargo build --release` and
  must catch a panic; `scripts/check-release-panic.sh` runs it. Proven to fail under `abort`.
- **D10. The fingerprint word list is BIP-39 English** (2,048 words), kept as
  `docs/sync/spec/wordlist.txt`. The lead fetched the official file and compared SHA-256
  `2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda`.
- **D11. Today's binaries get a panic hook that aborts, in release builds.** Attack round 5 showed
  what `panic = "unwind"` costs today's app: a panic on a background thread no longer ends the
  process, so the server's systemd unit (`Restart=always`) no longer recovers it. The plan requires
  the unwinding profile and also "every merge keeps today's app working", so `main.rs` and
  `server_main.rs` install a hook that prints as before and then aborts, only in builds without
  debug assertions (debug builds unwound before too). `desktop` removes or narrows it when the new
  runtime's `catch_unwind` guards need unwinding inside the app. See "Deviations".
- **D12. Findings against the two check scripts count only if CI or the documented local use would
  be misled.** Rounds 2 to 5 kept finding exotic local setups (a target directory shared between
  checkouts, hostile environment variables); the scripts grew with each. The property they guard is
  what matters.
- **D13. Usage-aware workflows.** The account's five-hour limit cut one long workflow in the
  middle (15:15 UTC, 2026-11-21): 18 agents had finished and were committed, 109 calls failed. Since
  then: workflows of about 15 to 20 agent runs, routine agents (attackers, fixers, implementers) at
  effort `high`, spec drafters at the session's effort, an attack loop that stops at once if an
  agent fails (so a failed agent never counts as a clean result), and no more than about 80% of a
  window's budget planned.
- **D14. The checks of task 1a guard a closed list of mutations.** After the hook landed, eleven attack
  rounds found nothing wrong with the product changes, and one small gap after another in how many
  hypothetical edits the two check scripts and the hook's test would notice; three fixes of the hook's
  test in a row each opened a new gap. The lead fixed the list: the profile's panic setting; the probe
  panicking; `build.rs` using local time, accepting one of eight named kinds of invalid
  `SOURCE_DATE_EPOCH`, or dropping or narrowing a listed rerun input; a `main()` not starting with the
  guarded hook call or naming the hook twice; the hook not printing as the default hook does, or not
  aborting. The hook's test reads the two main files only and runs in the debug profile. Anything else
  is left to review.
- **D6. Generic traits live in the engine crate.** `taskdeck-sync` defines the state-machine and
  sealer traits plus the toy implementations; `taskdeck-device` adapts the real core and seal to
  them (§6.7 task 2). The engine then depends on neither and compiles for wasm32 on its own.

## Rulings for the specs

SYNC.md leaves these open and more than one spec touches each, so the lead fixed them before the
drafters started. They are inputs to the drafts; the pairwise reviews and the two-readers test
may still overturn one, and then the change is recorded here and in `spec/CHANGES.md`.

- **S1. Byte order.** Big-endian everywhere (D5).
- **S2. Genesis.** The log's first entry (seq 1) is a control entry holding a `Genesis` item,
  authored and signed by the recovery identity (SYNC.md §9 step 6, V1-5, V1-13). It carries the
  salt and the recovery public keys (so `board_id` certifies itself), the first device, epoch 1
  with its wraps and commitment, rules version 1, and the hash of the genesis state plus, for an
  imported board, the id of the genesis snapshot blob. `seal.md` owns the layout, `engine.md` the
  bootstrap from it, `core.md` the genesis `BoardData`, `relay.md` the create route that stores it.
- **S3. The recovery identity as an author.** It has a `dev` id of its own, derived from its
  signing key, and its own counter; the relay lists it with role `recovery`. It holds no token.
  The relay accepts a frame it authored from any connection that passed the Worker gate, because
  the frame's signature is what authenticates it.
- **S4. The Worker gate and the door.** SYNC_PLAN.md §6.4 requires unauthenticated requests to be
  refused before the Durable Object runs, yet a new device in pairing, a device logging in on a
  re-homed relay and the recovery tool hold no token. So every request must carry one of: a token
  whose HMAC verifies (any generation; the object then checks state and generation); the create
  secret, for `create` only; or the board's **door**, for the pre-token routes only (pairing as
  the new device, login, recovery-signed reads and admin operations). The door is
  `HMAC-SHA256(TOKEN_KEY, "taskdeck/v1/door\0" ‖ board_id)[0..5]`: stateless for the Worker,
  fixed for the life of a deployment, and not a secret that protects data. The **relay address**
  a person types or pastes is the host plus Crockford base32 of `board_id ‖ door` (21 bytes, 34
  characters). It is what the pairing sheet shows beside the code, what a Fence names, and what the
  recovery sheet prints as "relay host and board id". A leaked door lets a stranger reach the
  object's pre-token routes and nothing else: the same exposure as a revoked device's stale token.
- **S5. A control entry holds an ordered list of control items**, applied all or nothing (revoke
  and rotate travel in one entry, SYNC.md §5.4).
- **S6. Time windows are decided from the log.** Every validity rule that involves time (72 hours
  for a recovery-key change, 30 days for a stale-epoch entry, 30 days for an active member) is
  computed from `at_ms` fields of sequenced entries, never from the relay's clock or the applying
  device's, so all devices decide alike. Local clocks decide only when a device offers or takes
  an action.
- **S7. State hash.** `SHA-256("taskdeck/v1/state\0" ‖ u32 hash_version ‖ SHA-256(canonical board)
  ‖ SHA-256(canonical membership))`. `core.md` defines the canonical board bytes, `seal.md` the
  canonical membership bytes (device set, epochs, per-device counters), `engine.md` the
  composition and when it is taken.
- **S8. No always-on device.** Any current, unhalted device, the phone included, can take every
  duty (snapshots, acknowledgements, compaction, beats, restore tail, upgrade proposals). Backups
  are written by whichever desktop is awake. A week with only the phone awake loses nothing and
  blocks nothing except backups.
- **S9. Push or poll is a capability the relay states.** The Worker pushes over a socket; the
  standby answers plain HTTPS and clients poll `read`. Routes and bodies are otherwise the same.
- **S10. Each spec ends with a Public API section**, and the skeleton crates are written from
  those. `engine.md` owns the state-machine and sealer traits (D6).

Rulings on what the seal draft raised (2026-11-21):

- **S11. The pairing bundle does not repeat the device set and the recovery public keys.** They
  travel in the fresh snapshot's canonical membership bytes, which the bundle authenticates by the
  snapshot's hash. Slot D stays under 16 KB.
- **S12. Every control payload carries an inner signature over its items.** A backup reader can then
  verify the membership history back to the board id from signatures alone, after a purge has
  deleted old epoch roots. It costs 64 bytes per control entry and one context string (`ctl`).
- **S13. A purge is signalled by `purge_before` on `EpochRotated`.** `engine.md` and `relay.md` adopt it.
- **S14. Revoking the last current device is refused, from the recovery identity too.** A board with
  no device cannot be read by anyone but the recovery key; a total compromise is answered by
  restoring to a new board and fencing the old one.
- **S15 (see the settled form below). Skip must classify the same on every device.** Whether a device still holds the target
  frame must not change the verdict. The specs must define a floor computed from the log below which
  a Skip target is refused everywhere, keep every frame above that floor fetchable from the relay
  (compaction respects it), and have a device that lacks the frame fetch it. Open until the
  seal–engine review settles the exact rule.

Rulings on what the core draft raised (2026-11-21):

- **S16. Stamps are per field group.** Five per item (name, time, levels, estimate, repeat) and one
  per block, instead of SYNC.md's one per item, so unrelated concurrent edits raise no false
  "changed since you last synced" notice.
- **S17. The notes base is a revision counter** (`notes_rev`), not a hash. It refuses a stale edit
  just the same, works for two queued edits from one device, and keeps hashing out of the core.
- **S18. Rules version 1 has caps SYNC.md does not name:** 10,000 live items, 50,000 live blocks,
  1,000 blocks per item, 200,000 archive rows, 60 MiB of board bytes, so a snapshot always fits its
  64 MiB limit. The import check reports a board that exceeds one. Told to the owner at exit.
- **S19. Reminder lead times are parameters of `reminders()`.** The defaults the spec names (events
  10 minutes before, blocks and due times on time, routines off) are what `android` starts with and
  may expose as a device setting. SYNC.md's open question 11 stays with the owner, without blocking.
- **S20. The legacy import stops on an unreadable primary file** (items, notes or subscriptions)
  instead of importing a partial board; unreadable archive lines are carried as opaque bytes.
- The snapshot JSON keeps today's names and shape. `core.md` §14.3 lists every difference: the
  inputs that were process globals, deterministic tie-breaks, block ids in place of indexes,
  canonical UTC text, and one new field, `notes_base`.

Rulings on what the relay draft raised (2026-11-21):

- **S21. The recovery tool may hold a read-only token it minted itself.** A recovery-signed
  `mint_read_token` gives the recovery tool a backup-read token for topping up a restore when the
  token inside the backup has been revoked by a rotation. "No relay token" (SYNC.md §5.5, ruling S3)
  means the sheet carries none and the recovery identity never appends by token.
- **S22. The create secret gates the create family**: `create`, and seeding a moved board on a new
  relay. A new relay has no door for the board until the seed has run.
- **S23. The relay counts appends, the client counts requests.** The 3,000-appends budget lives in
  the author's row and costs no extra write; the 5,000-requests budget is kept by the client, since
  counting requests at the relay would write a row per request. A member that floods the relay is
  a compromised device and is answered by revoking it.
- For the seal–relay review to settle: narrow the relay's share of the Genesis checks to what it can
  verify without keys; cap a host at 255 bytes so it fits its one-byte length; record that an
  identical repeat of a pairing slot post is answered again rather than refused.

Rulings on what the engine draft raised (2026-11-21):

- **S15, settled. The Skip floor is `F(s) = max(purge_seq, s − 16,384)`.** A Skip at seq `s` may
  target only an entry at or above that floor, on every device alike, and compaction keeps the newest
  16,384 seqs, so the target frame is always fetchable. `seal.md` §13.6 takes the precondition.
- **S24. A reinstating entry carries the roots the reinstated device missed.** The recovery identity
  holds a wrap of every epoch root, so when it reinstates a device it wraps to that device every root
  from the revocation to the reinstating entry. The device can then read and apply what was sealed
  while it was out, and need not pair again (SYNC.md §5.5: the recovery key overrules a hostile
  revocation).
- **S25. Snapshots keep SYNC.md §4.7's two triggers:** the newest acknowledged snapshot is a week old
  by the log clock, or 1,000 entries behind the head. To keep an import from producing a burst, a
  device writes at most one snapshot per 24 hours of log clock. (The draft had dropped the entry
  trigger.)
- A determinism mismatch is a "divergence stop" for the minority and for both sides of a tie: stop
  sending, keep applying, show both fingerprints. It is not a fourth halt (SYNC.md §4.2 names three).
- "Purge history before now" reaches as far as the most lagging current device's last reported head,
  so no device is stranded; `desktop` shows the seq it will reach before the owner confirms.
- A device away more than 365 days rejoins only after the user confirms, because rejoining replaces
  its local log.
- `taskdeck-sync` needs `sha2` and `zeroize` at run time (state hash, identity export); the
  workspace task declares them.

Rulings on what the device draft raised (2026-11-21):

- **S26. `unsafe` is allowed in platform FFI, and only there.** SYNC_PLAN.md §3.4 says "no `unsafe`"
  in its cryptography rule. DPAPI and a write-through rename on Windows exist only as `unsafe`
  functions of the pre-approved `windows` crate, and the JNI entry points need it too. So: none in
  core, seal, sync, backup or recover (`#![forbid(unsafe_code)]`); in `taskdeck-device` one module
  per operating system, in `taskdeck-android` the JNI entry points, each block with its safety
  argument written beside it. Told to the owner at exit.
- **S27. Any desktop can write backups, however it joined.** A paired device lacks the control
  entries below its bootstrap seq, so by the draft it could not write a verifiable backup, and a
  later-paired Mac would not be the second writer SYNC.md §6.6 expects. The snapshot a new device
  bootstraps from therefore carries the control history back to Genesis (it is small). The
  seal–engine and device reviews apply it.
- **S28. A backup's file name carries a writer tag** (`TaskDeck-<date>-<seq>-<tag>.tdbk`), so two
  desktops writing into one synced folder never overwrite each other. A small change to SYNC.md §6.6.
- The recovery tool learns the current read epoch from the relay's answer instead of guessing it;
  the relay–device review applies it. Local-only mode mints ids with device number 1, and the import
  accepts them; the core–device review applies it. A desktop on a polling relay asks every 30
  seconds, about 2,880 requests a day, inside its budget.
- Three review pairs are added to the plan's six, because the device draft asks for changes in all
  three: core–device, seal–device and relay–device.

Rulings on what the bridge draft raised (2026-11-21):

- **S29. The phone keeps its own picture.** SYNC.md §3 gives the phone "its own Look". The
  device–bridge review adds the two facade calls and the per-device file kind it needs.
- **S30. Every call on the phone that needs device authentication needs API 30 or later.**
  BIOMETRIC_STRONG without a PIN fallback is available through the framework only from API 30, and
  `androidx.biometric` is not a pre-approved dependency. Below API 30 the phone shows and the desktop
  acts, which is what SYNC_PLAN.md §2 already says for adding, revoking and replacing devices.
- **S31. The app declares `ACCESS_NETWORK_STATE`** besides the permissions of SYNC.md §2.1: a job with
  a network constraint cannot be scheduled without it. It is a normal permission with no prompt.
  Calendar access is gone with the export. Told to the owner at exit.
- The page's caches go in `android`, as SYNC_PLAN.md §6.10 says, so the first frame waits for one
  bridge round trip; the cold-start budget checks that. The minimum WebView version (120) and bundled
  roots on API 26 to 33 are the spec's defaults for `android-shell` to confirm (§6.6 task 7).

## Deviations from the plan

- **`foundation` edits three files under `src/`**, which SYNC_PLAN.md §4 gives to `desktop`:
  `src/main.rs`, `src/server_main.rs` and a small new module for the panic hook (D11). Without it
  the plan's own `panic = "unwind"` would break "every merge keeps today's app working" for the
  server. Nothing else in `src/` is touched. `desktop` should note it in its status file when it
  starts.

## Measurements

| What | Value | Where |
|---|---|---|
| Container | 4 CPUs, 15 GB RAM, 60 GB free, Rust 1.97.0, Node 22.23.5, Java 21, Gradle | this session |
| Today's headless tests in the container | 264 passed, 1 ignored (`codec_bake_off`, already ignored); 5 min 53 s cold, about 3 min of it tests | `cargo test --locked --no-default-features --lib` |
| Release build of `taskdeck-server` with `panic = "unwind"` and fat LTO | builds in 4 min 3 s; 9,240,728 bytes; `--version` prints the build date | container, commit `c7b3d07` |
| Full CI matrix with the pin, the refresh and the unwinding profile | green: Linux headless, desktop on Windows, macOS and Ubuntu, emulators at API 26 and 36 | pull-request run on `c7b3d07` |

## Facts found along the way

- Versions named in SYNC.md §2.3 and §5.1 all exist on crates.io. Youngest on 2026-11-21:
  `zeroize` 1.9.2 (2026-11-17, inside the cooldown until 2026-11-24). `Cargo.lock` has 1.9.0 today.
- `origin/main` (the older build, commit `03554d6`) is fetchable: `git fetch origin main`.
- `Cargo.lock` today: rustls 0.23.43, rustls-webpki 0.103.14, webpki-roots 1.0.9, tokio 1.52.3.

- Under `panic = "abort"` a panic on any background thread killed the app at once. With `unwind`
  only that thread dies (weather, calendars, the phone workers, the sync sender or listener) and
  the process carries on without it; a panic on the main thread exits with 101 after running
  destructors. `src/` has no `catch_unwind`, no panic hook and no `lock().unwrap()` outside tests.
  Debug builds always behaved this way. `desktop` should decide whether the app wants a panic hook.
- CI accepts `rust-toolchain.toml` as it is: the Linux job passed on the pinned toolchain with the
  old toolchain step still in place (run of commit `a743020`).

- The root manifest is a workspace root with `exclude = ["target"]`. A Cargo package anywhere else
  in the checkout must be a workspace member, be listed in `exclude`, or carry its own `[workspace]`
  table (as `cargo fuzz init` writes). A tree unpacked under `target/` builds on its own.

## Open questions for the owner

None so far.

## What the next stages need to know

To be written at exit.
