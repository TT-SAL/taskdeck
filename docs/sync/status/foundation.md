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
| 1a | Toolchain pin, lockfile refresh, `panic = "unwind"` with its release test, `build.rs` date | implemented; attack rounds running | commit `a743020` and the fix after it; `scripts/check-release-panic.sh`, `scripts/check-build-date.sh` |
| 1b | `deny.toml`, cargo-deny, cargo-audit, seven-day cooldown | todo | |
| 1c | CI: path filters at job level, aggregate `gate` job | todo | |
| 2 | Workspace: member crates with every planned dependency pinned; `relay/`, `android/` READMEs | todo | |
| 3a | Spec conventions (`docs/sync/spec/README.md`) | done, read in full by the lead | commit `460aa26` |
| 3b | Spec drafts: core, seal, relay, engine, device, bridge | seal in progress; order: seal, core, relay, engine, device, bridge | |
| 3c | Pairwise interface reviews (seal–relay, seal–engine, core–engine, engine–relay, device–bridge, device–engine) | todo | |
| 3d | Two-readers test, six scenarios, until two rounds agree (at most four) | todo | |
| 3e | Requirement map in `spec/README.md` (SYNC.md §3–§9 to clause, stage or plan §2 row) | todo | |
| 4 | Skeleton crates: public types and traits, toy state machine, fake sealer | todo | |
| 5 | Fixtures as data: boards, legacy `main` formats, snapshot JSON, transcripts | todo | |
| 6 | Tooling: `scripts/gate.sh`, `scripts/spec-coverage.sh`, `scripts/dev-setup.sh` | todo | |
| 7 | Stage exit review, exit pull request, merge | todo | |

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

## Deviations from the plan

None so far.

## Measurements

| What | Value | Where |
|---|---|---|
| Container | 4 CPUs, 15 GB RAM, 60 GB free, Rust 1.97.0, Node 22.23.5, Java 21, Gradle | this session |

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

## Open questions for the owner

None so far.

## What the next stages need to know

To be written at exit.
