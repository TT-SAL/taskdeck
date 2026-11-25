# TaskDeck sync: the build plan

*2026-11-21. How to build the design in [`SYNC.md`](SYNC.md) with parallel cloud sessions, each led by Fable 5.2 and running workflows of Opus 5.5 agents. Where this plan and `SYNC.md` disagree, this plan wins; §2 lists every difference. Wave 0 (`foundation`) is merged. Revised 2026-11-22 for the remaining waves: a task is written once, checked once and fixed once (§3.1, §3.3, §3.5, §6.3, §6.12, §7), so that tokens go to building.*

The plan goes straight to the end state: every device a full replica, syncing through a free blind relay. There is no interim milestone. Today's app keeps working on `day-planner` at every merge, and the owner's running copy is untouched until a one-sitting cutover (§1.4).

## 1. For the owner

### 1.1 One-time setup

1. **Make `day-planner` the default branch** (GitHub: Settings, General, Default branch). New cloud sessions then start on the code this plan describes. Without it, pick `day-planner` by hand for every session.
2. Nothing else yet. No Cloudflare account, no keys and no phone are needed before §1.4.

### 1.2 The sequence

Each stage is one cloud session. Stages in the same wave run at the same time, each in its own container. A container runs only two agents at once (§5), so parallel sessions are where the speed comes from.

| Wave | Start these sessions | When |
|---|---|---|
| 0 | `foundation` | now |
| 1 | `core`, `seal`, `relay`, `engine`, `android-shell` | `foundation` is merged |
| 2 | `device`, `recovery` | `core`, `seal`, `relay` and `engine` are merged |
| 3 | `desktop`, `release` | `device`'s first pull request, titled "device: facade", is merged |
| 3 | `android` | the same, and `android-shell` is merged |
| 4 | `rc` | every earlier stage is merged |
| 5 | cutover, by you (§1.4) | `rc` names a release candidate |

A stage is "merged" when its exit pull request is merged: the one whose status file says `final`. Each session ends by telling you which sessions to start next. Running fewer sessions at once is fine: take the stages in the order listed. Never run two sessions on the same stage.

### 1.3 Starting a session

Start a cloud session on this repository with Fable 5.2 and paste this, with the stage name filled in:

```
ultracode. You are the lead for stage <NAME> of SYNC_PLAN.md. Read SYNC_PLAN.md in full
before anything else, then the documents it lists for your stage. Run the work as workflows
whose agents use model 'opus'. Follow the plan's rules (section 3) without exception. You may
open pull requests from your branch into day-planner and merge them yourself once CI is green
and the plan's conditions are met. Ask me only about what section 3.5 lists; otherwise decide,
record the decision in your status file, and keep going.
```

To continue a stage in a fresh session, add: `Resume from docs/sync/status/<NAME>.md.`

### 1.4 What only you can do

| When | What | Time |
|---|---|---|
| After wave 0 | Optional: protect `day-planner` so it only accepts pull requests whose `gate` check passed | 5 min |
| After wave 3 | Dress rehearsal with made-up data: deploy a test relay to a Cloudflare account of your own, install the rehearsal builds on your phone and both computers, pair them, try it, and run the measurement script. Runbook: `docs/sync/runbooks/rehearsal.md` | 1 h |
| Wave 5 | Cutover: create the release signing key, sign and push the release tag, deploy the real relay, install the signed builds, turn your current board into the synced board, pair the Mac and the phone, print and retype the two recovery sheets. Runbook: `docs/sync/runbooks/cutover.md` | 2 h |
| The week after | Use it daily; note how stale the phone gets in the background; run one restore drill | — |

Your real board, your keys, your recovery sheets and your Cloudflare login never enter a cloud session or this repository. Sessions work on made-up boards only.

## 2. What changed since SYNC.md

Decisions the owner made on 2026-11-21, and their consequences. Sessions do not re-ask these.

| Topic | SYNC.md | This plan |
|---|---|---|
| Milestones | M1 relay hub first, then M2 | **No M1.** Dropped: the hub (§10.1), `Remote::Tunnel`, lifting `phone::route()`, relay forwarding, the `hub-req` and `hub-resp` signature contexts, the M1 queue import |
| Always-on device | The Windows desktop carries snapshot, backup and hub duty | **No device is assumed to be on.** Every duty falls to whichever device is awake; a week of phone-only use must be safe |
| `.ics` feed and exports | Export in phase 5a, system-calendar export in 5b | **Out of scope.** Nothing subscribes to the feed. A manual export is backlog (§6.13) |
| Android floor | minSdk 29, raised once the phone is known | **minSdk 26 (Android 8.0)** unless `android-shell` proves a blocker; ABIs `arm64-v8a`, `armeabi-v7a`, and `x86_64` for the emulator; speed budgets (§6.6) |
| The phone page | Shown unchanged | **Forked** to `android/page/` and changed only as `spec/bridge.md` lists. `src/phone.html` stays frozen with the old server until `desktop` deletes both |
| Administration from the phone | Marked ✓ on Android | The phone shows the fingerprint, banners, alarm evidence and the halted view, pairs itself, and accepts a move code. Adding, revoking and replacing devices needs API 30 or later (BIOMETRIC_STRONG). Restore to a point, purge history, reset trust and recovery-key changes are desktop-only |
| Reminders | Phase 6, after M2 | Part of the `android` stage, after the replica works; not a cutover gate |
| Pairing window | A separate capture-excluded window | Capture exclusion is applied to the main window while the pairing sheet is open. No second window; the render loop (DOC §14.1) is untouched |
| Migration | Watch the legacy folder for 30 days; "Export legacy" | The watch is dropped. "Export legacy" stays, as the way back |
| Migration source | Possibly a server folder and M1 queues | One standalone desktop folder, possibly written by the older `main` build |
| Packaging | Engine, store and transport inside the root package | Separate crates (§4), so stages own disjoint paths and the phone links no desktop code |
| Phase 0 hygiene | Fix the old server's leaks first | Only the lockfile refresh survives as its own task. The calendar-link leaks and the weak token generator are fixed by replacing that code (`device`, `seal`) |
| Development machine | A separate account or VM | Cloud sessions. CI is the Windows, macOS and Android test bench |
| Cost | EUR 0, optional domain and signing token | Unchanged. The EUR 0 options are the default; nothing in the plan may require spending |
| Effort table (§10) | 42–53 focused weeks of unassisted work | Superseded by this plan |
| Lost recovery sheet | one member proposes, every device shows a banner for 72 hours, another member confirms | **Dropped** (the owner, 2026-11-23): the recovery key changes only with the current sheet, and a lost sheet means Reset trust. One compromised device could run the old path alone and at once |

Defaults taken for SYNC.md's open questions: device-local time zones (3); overwrite notices, not refusals, for stale edits (8); no paid crypto review (9); phone extras after cutover (10); no push (7).

## 3. Rules for every session

### 3.1 Orchestration

- **The lead orchestrates and verifies; agents write.** Every `agent()` call sets `model: 'opus'`. The lead runs each gate itself and reads CI itself. An agent's claim that something passes is not evidence.
- **Two agents run at a time** in one container. Plan workflows for that: one writer at a time in a given crate; a verifier may run beside the writer of another task. Agents that compile at the same time each set their own `CARGO_TARGET_DIR`.
- **Work in small tasks, each ending green and pushed.** A container can be reclaimed; anything unpushed is lost.
- **Use the shapes in §7.** One agent writes a task; one agent that did not write it checks it once; one pass fixes what that found. There is no loop until dry. A security pass follows only where §7 names one.
- **Spend tokens on building.** Agents never read a spec in full: they use the clause index (`python3 docs/sync/status/foundation/tools/clause-index.py docs/sync/spec <file>` writes every clause, one line each, under its section heading) and the sections their task names. The lead integrates once per finished workflow, not after every agent. Each stage records in its status file the tokens each task took.

### 3.2 Repository

- **Own only your stage's paths** (§4). After a stage's exit pull request merges, a later stage that needs a change in its paths makes it through a small pull request of its own and notes it in both status files.
- **Three files are shared:** `Cargo.lock`, the root `Cargo.toml` and `.github/workflows/ci.yml`. Change them in a small pull request of their own, merged before the work that needs it. One new dependency per pull request, except in `foundation`.
- **Stay current:** merge `day-planner` into your branch before each workflow.
- **Open a draft pull request early.** Pushes to a pull request run the full CI matrix; other pushes run only the Linux job unless the commit message contains `[ci full]`.
- **Every merge keeps today's app working.** Old code paths are deleted only by `desktop`, at the end.
- **This repository is public.** Never commit a secret, a relay address, a token, an account name or anything from a real board.

### 3.3 Engineering

- **The specs in `docs/sync/spec/` are the contract** once `foundation` merges. A stage that finds a spec wrong or ambiguous does not improvise: it opens a spec-change pull request (spec, vectors, and a line in `docs/sync/spec/CHANGES.md`), merges it, and names the affected stages in its status file and to the owner.
- **Every MUST clause has an id and a test.** `scripts/spec-coverage.sh <stage>` fails if a clause owned by the stage appears in no test.
- **No new checker or scanner scripts.** A stage writes none beyond those this plan names; the ones under `scripts/` stay as `foundation` left them unless a gate is wrong. A scanner guards the ordinary forms only: it is there to catch an honest mistake, and review covers the rest.
- **Never weaken a gate to pass it.** No skipped, ignored or loosened tests, no `unimplemented!()` left in owned paths. Tests are deleted only together with the code they test. If a gate is wrong, change it in its own pull request and say why.
- **Budgets are limits.** Measure in a dedicated CI job, as the median of five runs, and record the baseline in the status file. A baseline that misses the limit is a question for the owner, not a reason to edit the limit. After that, a result more than 25% worse than the baseline fails.
- **Keep read-side names.** The new core answers the questions the UI and the phone page already ask (`ranked_ids`, `behind_minutes`, the snapshot JSON) under the same names, with the device's time zone passed in.
- **DOCUMENTATION.md §14 is off-limits:** the render loop, the single-file `ui.rs`, the hand-tuned widget numbers, the daily shuffle, plans staying out of the calendar grid, the UI scale. New desktop screens go in `ui.rs` like the rest.

### 3.4 Security

- **Dependencies.** Pre-approved: the crates named in SYNC.md §2.3 and §5.1 at those versions; anything already in `Cargo.lock` (among them `windows`, `objc2` and its framework crates, `flate2`, `image`); `security-framework`; `webpki-roots` if `android-shell` chooses bundled roots; the platform verifier's Android component, vendored by checksum. For tests and tooling only: `proptest`, `libfuzzer-sys`, `cargo-fuzz`, `cargo-deny`, `cargo-audit`, `cargo-ndk`, JUnit and `androidx.test`, `wrangler` (pinned, used only in cloud sessions and CI). The relay has no runtime dependencies. Anything else goes to the owner with a written reason.
- **Cryptography:** compose the named primitives exactly as `spec/seal.md` says. No hand-written primitives, no `unsafe`, secrets zeroized, comparisons constant-time, every size checked before allocation, nothing parsed before it is authenticated.
- **Test-only trust** (a relay on loopback, a test certificate) sits behind a cargo feature that release builds never enable; `release` proves the artifacts lack it.
- **Agents never deploy, never push a release tag, never touch a production relay.** Staging deploys happen only in the owner's rehearsal.
- **Install tools from their official sources** at pinned versions, with checksums where published.

### 3.5 Status, hand-off, and when to ask

- **`docs/sync/status/<stage>.md`** is the stage's memory: tasks done, the tokens each took, decisions with reasons, deviations, measurements, the low findings left as they are (§7), open questions for the owner. Update it with every task. A fresh session must be able to resume from it alone.
- **Exit:** the stage's gate is green, the status file says `final`, and the exit pull request's body lists what was built, what was measured, and what the next stages need to know. Then merge and tell the owner which sessions to start.
- **Ask the owner only for:** spending money; adding a service, or a dependency §3.4 does not pre-approve; weakening a MUST or a budget; removing a feature; touching DOC §14; anything this plan marks as the owner's.

## 4. Layout and ownership

Where two stages are listed, ownership passes when the first one merges.

| Path | Stage | Contents |
|---|---|---|
| `docs/sync/spec/`, `fixtures/`, `scripts/`, `.github/workflows/ci.yml`, workspace manifests, `build.rs` | `foundation` | contracts, made-up boards and recorded transcripts, gates, CI |
| `crates/taskdeck-core/` | `core` | pure board: data, commands, deterministic `apply`, planner, archive, scoring, ICS parser, snapshot builders, legacy import; fuzz targets |
| `crates/taskdeck-seal/`, `docs/sync/vectors/` | `seal` | frames, envelope, keys, membership, chain, pairing, sealed files; test vectors; fuzz targets |
| `relay/` | `relay` | `relay-core.js`, the Worker and Durable Object, the Node standby, the conformance suite |
| `crates/taskdeck-sync/` | `engine`, then `device` | sans-IO replica engine and its simulator |
| `android/` (including `android/page/`), `crates/taskdeck-android/` | `android-shell`, then `android` | Kotlin shell, hardened WebView, the forked phone page, bridge, JNI library |
| `crates/taskdeck-device/`, `tests/e2e/` | `device` | the device runtime: store, transport, key stores, fetchers, facade, headless CLI |
| `crates/taskdeck-backup/`, `crates/taskdeck-recover/` | `recovery`, then `device` | backup format, writer, reader; the offline recovery tool |
| `src/` (the root package) | `desktop` | the egui app on the new runtime; removal of the old server, phone server, `src/phone.html` and `sync.rs` |
| `.github/workflows/release.yml` and `nightly.yml`, `scripts/release/`, `scripts/mac-bundle.sh`, `docs/sync/runbooks/`, the top-level docs | `release` | release pipeline, nightly jobs, runbooks, documentation |
| any path | `rc` | fixes only: one per pull request, each with its reproduction as a regression test |

Reference material for every stage: [`SYNC.md`](SYNC.md); `docs/sync/research/` (R1 code seams with file and line, R2 Android, R3 hosting and relay, R4 threats and crypto; V1, V2 and V3 are the verifiers' findings in full, cited in SYNC.md as "V2-3" and so on); DOCUMENTATION.md §4.1, §14, §16–§23.

## 5. What is already known

Checked on 2026-11-21 in a cloud container like the ones the stages will use.

- **Container:** 4 CPUs, 15 GB RAM, about 58 GB free disk, unrestricted network. Node 22 (with `node:sqlite`, no flag needed), Java 21, Gradle and Xvfb are installed; the Android SDK, the NDK and `cargo-ndk` are not, and their download hosts are reachable. There is no hardware virtualisation, so no usable Android emulator, and no Docker daemon.
- **Workflows:** at most two agents run at once (the cap is CPUs minus 2).
- **CI works from a session.** A cloud session can push workflow files. The repository is public, so GitHub's runners are free.
- **The emulator boots in CI** in one to two minutes at API 26 and API 36 (`.github/workflows/ci.yml`).
- **Old emulator images carry an old WebView that cannot be updated:** version 58 at API 26, against 133 at API 36. Real Android 8 phones update WebView through the Play Store. So WebView tests run on recent images, everything else also runs at API 26, and the app checks the WebView version at start and asks for an update if it is too old.
- **Cloudflare's local runtime works in the container.** `wrangler dev` 4.166.0 ran a SQLite Durable Object with `transactionSync`, hibernating sockets and push to connected sockets, with no account.
- **Today's code is green everywhere.** The first full CI run took 12 minutes: the headless build and its 264 library tests on Linux; the windowed app and its library tests on Windows, macOS and Linux (280 tests on Windows).
- **The lockfile carries three advisories to refresh away:** `webpki-roots` 1.0.9 (RUSTSEC-2026-0381), `rustls-webpki` 0.103.14 (RUSTSEC-2026-0382), `tokio` 1.52.3 (RUSTSEC-2026-0386).
- **Release builds abort on panic today** (`panic = "abort"`, `Cargo.toml` line 94). In a workspace that setting is global, and the design's `catch_unwind` guards need `"unwind"`.

## 6. Stages

Each stage lists what to read first, what to build, and the gate. "Gate" means two things: `scripts/gate.sh <stage>` exits 0 for everything a container can run, and the stage's CI jobs are green for the rest. The script prints the items only CI can check.

### 6.1 `foundation` (wave 0)

Read: all of SYNC.md; R1, R3, R4, V1 and V2 in full; R2 and V3 for `spec/bridge.md` and the tooling.

1. **Baseline.**
   - Pin the toolchain in `rust-toolchain.toml`, with clippy and the wasm32, Android and Apple-silicon targets.
   - Refresh the lockfile with `cargo update -p` for `rustls`, `rustls-webpki`, `webpki-roots` and `tokio` only. No blanket update: the desktop's look depends on the pinned egui, wgpu and winit.
   - Set `panic = "unwind"` in the release profile, with a test that builds in release and checks a panic is caught.
   - Make `build.rs` take its date from `SOURCE_DATE_EPOCH` when set.
   - Add `deny.toml`, `cargo-deny`, `cargo-audit` and the seven-day dependency cooldown.
   - Filter CI jobs by path at job level, and add an aggregate job named `gate` that always runs, fails if any job failed or was cancelled, and treats skipped jobs as passed.
2. **Workspace.** Keep the root package; add the crates in §4 as members with every planned dependency declared at its pinned version, so later stages rarely touch `Cargo.lock`. Add `relay/` and `android/` with a README each.
3. **Contracts.** Write the specs, each with numbered MUST clauses, an error catalogue, limits, and the format of its test vectors:
   - `spec/core.md`: board data, intents, log commands v1 (including board settings for the palette and the home place), `Ctx`, the origin's compile step, ids and stable block ids, conflict rules, rules versions, canonical encoding and state hash, read queries that take the device's zone (SYNC.md §4.4–§4.6).
   - `spec/seal.md`: frame, envelope, signature contexts, key hierarchy, membership and every control entry (including the recovery-key change and its two-member path), chain, pairing, sealed files, backup envelope (§4.2, §4.8, §5).
   - `spec/relay.md`: every route and socket message, authentication, sequencing and idempotency, caps, budgets, errors, the storage schema, compaction and purge, push and plain HTTPS polling (§4.1, §4.3, §6).
   - `spec/engine.md`: the replica state machine: confirmed and pending state, outbox, send and reconcile, receipt checks, refusals and halts, checkpoints, snapshots, compaction, integrity evidence, beats, rules upgrades, Skip, timed windows such as the recovery-key change, duties with no always-on device (§4.3, §4.7, §4.8).
   - `spec/device.md`: the facade the two apps use, the on-disk store and log format, transport, test-only trust (§3.4), the key-store interface, fetcher rules, blobs, backup scheduling.
   - `spec/bridge.md`: the phone page's routes and verbs answered natively, every change the forked page needs (the bridge behind `api()`, the requests it makes outside `api()`, storage, the service worker), the allow-list, the JNI surface, WebView settings (§7, R1 §6, R2).
4. **Skeletons.** Public types and traits from the specs compile in every crate. Bodies may be `unimplemented!()` here and nowhere later. Include a toy state machine and a fake sealer so `engine` can start without `core` and `seal`.
5. **Fixtures, as data.** A generator built on today's `Board` code, run with a pinned clock and `TZ`, writes under `fixtures/`:
   - made-up boards: small, 10,000 archive rows, daylight-saving edges, ids near every limit, unreadable archive lines;
   - the same in the older `main` build's file formats (build that branch's code, or write them from DOCUMENTATION.md's tables and the legacy-row tests in `archive.rs`), with a `userconfig.toml`;
   - for each board, the JSON today's `phone::snapshot` returns;
   - transcripts: seeded command sequences with the state today's `Board` reaches after each command.

   Later stages test against this data, never against the old code, so `desktop` can delete that code freely.
6. **Tooling.** `scripts/gate.sh`, `scripts/spec-coverage.sh`, `scripts/dev-setup.sh <stage>` (installs what a stage needs in a fresh container: Android SDK and NDK, `cargo-ndk`, `cargo-deny`, `wrangler`).

Workflow: one drafter per spec; then pairwise interface reviews (seal with relay, seal with engine, core with engine, engine with relay, device with bridge, device with engine); then the two-readers test. In it, two agents, given only the specs, each write out the message sequence and the byte layout (field order, widths, context strings, with cryptographic outputs left symbolic) for six scenarios: first edit, offline edit replayed, pairing, revocation with rotation, relay rollback, snapshot bootstrap. Any difference is an ambiguity to fix. Stop after two rounds with no difference, or after four rounds, recording what remains in `CHANGES.md`.

Gate: every CI job green, including the desktop matrix; skeletons compile on Windows, macOS, Linux and for the Android targets; a table in `spec/README.md` maps every requirement in SYNC.md §3–§9 to a clause id, a stage in this plan, or a row of §2 above.

### 6.2 `core` (wave 1)

Read: `spec/core.md`; R1 in full; V2-1 to V2-5, V2-16, V2-17, V2-19. Copy from `src/`; do not edit `src/`.

1. Copy the pure logic in: types, planner, archive, scoring, ICS parser, overlay types, weather report types. No file, network, thread or clock access anywhere in the crate.
2. `BoardData` and `apply(&mut BoardData, &LogCommand, &Ctx)` returning a change set. UTC types, canonical serialisation.
3. Intents and the compile step: defaults, trimming, zone conversion, Reflow compiled to explicit block moves, Unplan to explicit removals, floating routine commands, the palette and home-place settings. Log commands deserialize strictly.
4. Device-minted ids with high-water marks, stable block ids, Restore with a fallback id.
5. Conflict rules: last-change stamps, overwrite notices, benign repeats, notes with a base, block moves that skip what changed.
6. Rules versions with golden logs; canonical encoding v1; the state hash.
7. Read queries and snapshot builders for the UI and the phone page, generic over the caller's time zone, with the archive bucketed by day; `reminders()`.
8. Legacy import: old files in (including the palette and home place from `userconfig.toml`), a report and a genesis `BoardData` out. The report contains counts, ids and error kinds only, never text from the board.
9. Fuzz targets under `crates/taskdeck-core/fuzz/` for commands with `apply`, the ICS parser and the snapshot builder.

Gate:
- purity: the crate compiles for `wasm32-unknown-unknown`; clippy forbids clocks, `chrono::Local`, file, network and thread APIs;
- determinism property test: random command sequences, a second replica started from a snapshot at a random point, run under two `TZ` values, identical canonical bytes;
- rebase property test: pending edits replayed over confirmed ones;
- transcript test: replaying `fixtures/`' recorded sequences gives the recorded states, for the commands whose meaning did not change;
- snapshot JSON equal to the fixtures';
- golden logs and hashes;
- budgets (§3.3) with the 10,000-row fixture: 31-day snapshot within 40 ms, 5,000 entries applied within 1 s, checkpoint loaded within 150 ms.

### 6.3 `seal` (wave 1)

Read: `spec/seal.md`; R4; all of V1.

Build SYNC.md §4.2, §4.8 and §5 as pure functions and sans-IO state machines: frames and signatures with per-use contexts; the entry envelope with padding; the recovery seed and its paper encoding; epoch roots, derivation and HPKE wraps; the membership state machine for every control entry, including races, the recovery key's powers and the recovery-key change; the hash chain and `based_on` checks; both roles of pairing; login and admin signatures; sealed local files; the backup envelope. Publish vectors under `docs/sync/vectors/`. Add fuzz targets under `crates/taskdeck-seal/fuzz/`.

Gate:
- known answers: RFC 8032, RFC 9180 for the chosen suite, RFC 5869, the XChaCha20-Poly1305 draft vectors, Wycheproof for Ed25519 and the AEAD;
- tamper matrix: flipping any byte of any frame, wrap, bundle or file is rejected;
- context confusion: no signature or ciphertext made for one use verifies for another;
- a relay-in-the-middle pairing test that succeeds only by guessing the six digits;
- parsers never panic under property tests; `#![forbid(unsafe_code)]`;
- compiles for the Android targets; budget: 256 entries verified and opened within 100 ms.

Security passes (§7): two for every task of this crate. What they try: protocol misuse; parsing and resource exhaustion; membership and rollback logic; pairing.

### 6.4 `relay` (wave 1)

Read: `spec/relay.md`; R3; V1-1, V1-5, V1-16; SYNC.md §4.1, §4.3, §6.

1. `relay-core.js` behind a storage interface: create, login, socket, read, append, snapshots and blobs, pairing tickets, signed admin operations, compaction, purge, restore tail, fence.
2. `worker.js`: the token gate before any Durable Object call; the object with SQLite storage and hibernating sockets; no HTML, no CORS, no logs.
3. `standby.js`: the same core on Node 22 with `node:sqlite`, served over plain HTTPS polling. It doubles as the local relay for every other stage's tests.
4. The conformance suite (`node --test`), run against the core in process, the standby over HTTP, and the Worker under `wrangler dev`. It signs its own test frames from `spec/seal.md`'s layout; `device` later adds `seal`'s published vectors.
5. `deploy.sh` for the owner, and a README.

Gate: the suite green on all three targets; a test asserts rows written per operation and that a typical day (SYNC.md §6.2) stays under 2% of every free-plan limit; unauthenticated, oversized and slow requests are refused before the Durable Object runs; replays are refused by the object without a write; `relay/package.json` has no runtime dependencies.

### 6.5 `engine` (wave 1)

Read: `spec/engine.md`; V2 in full; SYNC.md §4.3, §4.7, §4.8, §5.4.

Build the replica engine with no I/O of its own, generic over the state machine and the sealer: it consumes events (user intent, relay message, timer, storage result) and emits effects (persist, send, notify). It covers confirmed and pending state with rebase, the enqueue and send rules, reconciliation, receipt checks, refusal against halt, page-wise apply with poison-pill handling, Skip, checkpoints and hashes, snapshot duty and acknowledgements, bootstrap, integrity evidence and the stop-and-show response, epoch transitions and re-sealing, rules-upgrade gating, timed windows, beats, the clock-skew guard, restore to a point, purge, and following a move of the board.

Then the simulator: seeded and deterministic, with a model relay that follows `spec/relay.md` and faults for loss, duplication, reordering, delay, disconnection, a device crash at every persist boundary, clock skew, and a relay that rolls back, forks or withholds.

Gate: 1,000 seeds per CI run with these invariants; every failing seed becomes a regression test.
1. Devices that are honest and connected converge to one state hash.
2. An acknowledged entry is never lost and is applied exactly once everywhere. A relay that forks, or rolls back under entries another device then overwrites, can lose entries; for those faults the promise is that the loss is never silent: a device that holds the entry raises an alarm with evidence (`engine.md` §31.7; the owner's decision of 2026-11-25).
3. A queued edit survives any crash and is sent exactly once.
4. Tampering always raises an alarm with evidence and never changes state.
5. No relay answer deletes local data or keys.
6. When faults stop, the system settles.

The crate compiles for `wasm32-unknown-unknown` as proof it does no I/O.

### 6.6 `android-shell` (wave 1)

Read: `spec/bridge.md`; R2; R1 §6; SYNC.md §7 (the WebView row) and §8 (Android).

1. Toolchain in the container and in CI; a Gradle project whose app depends on `kotlin-stdlib` and `androidx.webkit` only; Gradle dependency verification.
2. The JNI library: strings in, strings out, `catch_unwind` at every entry, built for the three ABIs.
3. Fork `src/phone.html` to `android/page/` and change it as `spec/bridge.md` lists: `api()` posts through the bridge; the photo, the sky symbols and the weather are answered natively; the service worker is gone; page preferences go through the bridge, with DOM storage off. In this stage the bridge answers from the fixtures and an in-memory echo.
4. The hardened WebView around it (SYNC.md §7), and the exfiltration harness: script inside the page tries every way out (fetch, XHR, sockets, WebRTC, images, styles, forms, navigation, frames, beacons, prefetch, service worker) against a local sink that must receive nothing.
5. Keystore wrap and unwrap; backup and transfer exclusions; a start-up check of the WebView version.
6. A timing harness for cold start, first painted day and bridge round trip.
7. Decide the floor: minSdk 26 stands unless something here proves it cannot. Decide per API level whether the OS trust store or bundled roots verify the relay's certificate. Record both decisions.

Gate: unit tests; instrumented tests in CI, WebView tests on a recent image, the rest also at API 26; the harness's sink stays empty; the `armeabi-v7a` build links; budgets (§3.3) on the API 36 emulator: cold start to first painted day within 2.5 s, bridge round trip within 60 ms at the 95th percentile, universal APK under 25 MB.

### 6.7 `device` (wave 2)

Read: `spec/device.md`, `spec/engine.md`; R1 §5, §7, §8; V2's scenarios; SYNC.md §4.1, §5.6, §7 (the ICS row).

1. **First pull request, "device: facade":** the facade from `spec/device.md`, including the zone-aware read queries, with a local-only mode (core plus plain files, no relay). This unblocks wave 3. Merge it as soon as it is green.
2. Wire the real core and the real seal into the engine. One command end to end through the standby relay before anything else, and `seal`'s vectors added to the relay's suite; send spec changes back now, while they are cheap.
3. The sealed store with its fsync order, tested by a file layer that kills the process at every write boundary.
4. Transport: HTTPS and WSS over rustls with the platform verifier, backoff with jitter, budgets, a constant User-Agent.
5. Key stores: the interface, Windows DPAPI, macOS Keychain (tested in CI with a temporary keychain), an in-memory one for tests.
6. Fetchers: ICS with hand-followed redirects, no Referer, the address guard, ETag with the raw body cached, errors that never contain the URL; weather.
7. Blobs: the shared picture, JPEG only, decoded under limits and re-encoded.
8. Operations, wired through: move board, purge history, restore to a point, restore tail, device replacement, the recovery-key change window.
9. `taskdeck-devicectl`, a headless device for tests.
10. End-to-end suite: several device processes against the standby relay and, in CI, the Worker under `wrangler dev`. Each of V2's fifteen scenarios is a test. A hostile proxy that drops, reorders, replays, forks, rolls back and truncates must always produce an alarm.
11. Once `recovery` is merged: nightly backups and monthly segments scheduled in the runtime by any desktop that is awake; the recovery tool's online steps (topping up through the read token, fencing the old board).

Gate: the end-to-end and hostile-relay suites; crash-consistency tests; key-store tests on the Windows and macOS runners; fetcher tests against a local server for redirects, private addresses and oversized bodies; budget: catch-up of 4,200 entries within 3 s.

### 6.8 `recovery` (wave 2)

Read: `spec/seal.md` (backup envelope), `spec/device.md` (log format); SYNC.md §5.5, §6.6; V1-2, V1-13, V1-17; V2-12.

Build everything that works offline: the backup format with its writer, reader and verification chain back to the board id; nightly files and monthly segments as functions over a state and a log; the recovery sheet's layout and retype check; the recovery signer's operations; `taskdeck-recover`, which restores a folder from a sheet and a backup. The online steps and the scheduling are `device`'s task 11.

Gate: every backup kind restores to an identical state hash; forged, truncated and re-signed backups are refused; a restore drill on a made-up board runs in CI.

### 6.9 `desktop` (wave 3)

Read: `spec/device.md`; DOCUMENTATION.md §14, §16–§19, §22; SYNC.md §3, §5.3–§5.5, §6.6, §9; R1 §1 (item 5), §9. One writer at a time: this stage is serial.

1. Put `TaskApp` on the facade in local-only mode. With no sync configured, the wall calendar behaves exactly as today. The UI keeps its own display order and never reorders replicated data. Times convert to local at the edge. The notepad edits a draft. The palette and the home place become board settings.
2. Settings, SYNC, part one: create board; pair with the typed code and the six digits, with capture excluded while the sheet is open; the device list; revoke and replace; sync status and last seen; the fingerprint; banners for every membership change; the notices list; the notes merge view; the clock-confirm prompt; what blocks a rules upgrade. Windows Hello or Touch ID before membership changes.
3. Settings, SYNC, part two (needs all of `device` and `recovery` merged): alarm evidence and adopting a state by fingerprint; the halted view and Skip; restore to a point; purge history, and the retention wording in the Forget dialog; move board; reset trust; the recovery sheet, printed through the OS dialog and never through a file; the recovery-key change, with the current sheet; backup destinations and status; uploading the shared picture.
4. Import and cutover: `TaskDeck --import-check <folder>`; "Create board from this folder"; the old files moved aside untouched; "Export legacy".
5. Remove what is replaced, with its tests: the `taskdeck-server` binary, the phone server, `src/phone.html` and the feed, tokens and QR codes, `sync.rs`, the old fetch threads, the fixture generator, and their dependencies.
6. Try the app under Xvfb in the container for screenshots. Useful if it works; not a gate.

Gate: every remaining UI test green on Windows, macOS and Linux; a headless test drives create, pair, edit, revoke and recover through the same code paths the settings screens call; the import check passes on every legacy fixture and its report contains no board text; no reference to the removed modules remains.

### 6.10 `android` (wave 3)

Read: `spec/bridge.md`, `spec/device.md`; R2; SYNC.md §3, §4.9, §5.6, §7.

1. The JNI library over the device runtime; the Keystore-backed key store; the sealed store in app-private storage.
2. The bridge answers from the real core. In the page: the sync line; notices; the notes conflict sheet; marks on edits that will be refused; the old offline queue and caches removed.
3. Sync while visible over a socket; a job every 30 minutes; an expedited job on leaving with queued edits (API 31 and later).
4. Native screens: Pair; Devices, with the fingerprint, banners and, from API 30, add, revoke and replace; alarm evidence and the halted view; entering a move code; Health (battery, standby bucket and alarm readouts; the Samsung "never sleeping apps" walk-through).
5. Photo picker, coarse location, the shared picture through Rust.
6. Reminders: computed in the core, scheduled as a chain of exact alarms, verified before they notify. Done, Tomorrow and Snooze act behind authentication from API 31; below that they open the app.
7. Builds: a reproducible `.so`; an unsigned release APK as a CI artifact; a rehearsal APK under its own application id, signed with a throwaway key.

Gate: instrumented tests for offline edit, process death, reboot, forced Doze, a zone change, and a 10,000-row archive; the exfiltration harness still empty; the budgets from §6.6 hold with the real core; a three-device end-to-end test in CI with the emulator as one device.

### 6.11 `release` (wave 3)

Read: SYNC.md §8, §6.6, §6.7, §11 ("Still to measure"); V1-4; V3 in full.

1. The release workflow. It publishes only for a `v*` tag signed by the owner: cache-free builds of the Windows and macOS apps (with `scripts/mac-bundle.sh`), the unsigned APK and the relay bundle, each with a provenance attestation. A dry-run mode, started from a pull request, builds and verifies everything and publishes nothing. Scripts that verify an artifact the way SYNC.md §8 says, and that prove it lacks the test-only trust feature.
2. `nightly.yml`: the simulator at 10,000 seeds; the fuzz targets, on a nightly toolchain used nowhere else; the move-board rehearsal against the standby relay.
3. Runbooks in `docs/sync/runbooks/`: rehearsal (with a script that takes SYNC.md §11's open measurements on the real relay and phone), cutover, installing and updating each platform, signing the APK, deploying the relay, a lost device, recovery, moving the board, the restore drill. Written for the owner, tested by an agent following them literally in a container wherever the steps allow.
4. Documentation: DOCUMENTATION.md §21–§23 rewritten for the new system; README; SERVER.md and MOBILE.md marked as history; CODE_REVIEW.md brought up to date.

Gate: the dry run produces every artifact and each verifies; the nightly jobs run green once; every runbook step that can run in a container has been run.

### 6.12 `rc` (wave 4)

Read everything in `docs/sync/status/`.

1. The whole system in CI: the Windows and macOS apps' headless paths, the emulator app and the Worker under `wrangler dev`, through pair, edit, offline, revoke, restore.
2. One whole-system security pass: three agents, one for each aim: get code onto a device, read the data, lose or corrupt the data. A finding counts only with a reproduction. Fix what is of high or medium severity, one pull request per fix; list the low findings in the status file. An aim gets a second pass only if its first found something of high severity.
3. Requirements check against the owner's four: full function on all three devices, a free server, uptime, protection from malware and theft. Every row of SYNC.md §3 and §7 is marked done, deferred to §6.13, or failed.
4. Name the release candidate: the commit, and the hashes the release dry run produced for it. The owner reviews, signs and pushes the tag; agents do not.

### 6.13 After cutover (backlog, not planned here)

Phone extras (archive search, Forget, managing subscriptions, home place, palette); a manual `.ics` export; widgets, a tile and a share target; push; a system-calendar export; a home-zone display; `cargo-vet` coverage.

## 7. Workflow shapes

**The task shape**, for every implementation task. A task is written once, checked once and fixed once; tokens go to building, not to repeated checking.

1. **Implement.** One agent writes the code and the tests that cite its clauses, and runs the task's gate.
2. **Verify, once.** One agent that did not write the code checks it against the task's brief and the spec, and reproduces whatever it reports. A finding without a reproduction does not count.
3. **Fix, once.** One pass fixes the findings of high and medium severity, each with its reproduction as a test.
4. **The lead** runs the gate, commits, pushes and reads CI.

There is no loop until dry. A second verify pass, with its own fix pass, runs only if the first found something of high severity; nothing runs a third time. Keep these properties when you adapt the shape: the writer does not judge its own work, a finding needs a reproduction that is checked before anyone acts on it, and a failed agent never counts as a clean result.

**Severity.** High: a rule of §3.4 or a MUST of a spec is broken, or today's app stops working. Medium: the work does something other than its brief or its spec says, in ordinary use. Low: everything else, robustness outside ordinary use included. Only high and medium findings are fixed. Low findings are listed in the status file, not chased.

**Security pass.** After the fix, one agent tries to break the task, and only where the task touches cryptography, authentication, pairing, relay credentials, the Android bridge or the WebView, or the parsing of untrusted input. Every task of the seal crate and the relay's authentication get two such passes, by two agents. The same rules hold: reproduce, fix high and medium once, list low.

**Once per stage.** One hygiene pass (secrets, anything from a real board, stray files, dependencies §3.4 does not approve), and the exit review below.

```js
export const meta = {
  name: 'task',
  description: 'Implement one plan task, verify it once, fix once, then the security passes it needs',
  phases: [
    { title: 'Implement', model: 'opus' },
    { title: 'Verify', model: 'opus' },
    { title: 'Fix', model: 'opus' },
  ],
}
// args: { id, brief, paths, gate, security }   security: 0, 1, or 2 (the seal crate, the relay's authentication)
const FINDINGS = {
  type: 'object',
  properties: { findings: { type: 'array', items: {
    type: 'object',
    properties: { title: { type: 'string' }, severity: { type: 'string', enum: ['high', 'medium', 'low'] },
                  clause: { type: 'string' }, repro: { type: 'string' } },
    required: ['title', 'severity', 'repro'],
  } } },
  required: ['findings'],
}
const RULES = `Report only what you reproduced, with the failing test or command as text. Do not edit tracked files: ` +
  `run with CARGO_TARGET_DIR=target/check and keep scratch files under that directory. Do not read a spec in full: ` +
  `use the clause index and the sections the brief names.`
const lows = []
// One pass by an agent that did not write the code, then one fix pass for what is high or medium.
const pass = async (label, task) => {
  const r = await agent(`${task} ${RULES}`, { label, phase: 'Verify', model: 'opus', schema: FINDINGS })
  if (r === null) return null                       // a failed agent never counts as a clean result
  lows.push(...r.findings.filter(f => f.severity === 'low'))
  const serious = r.findings.filter(f => f.severity !== 'low')
  if (serious.length === 0) return { high: false }
  const fixed = await agent(`For task ${args.id}: first add each reproduction below as a test and run it. ` +
    `Name and drop any that does not fail. Then fix the rest, keeping \`${args.gate}\` green.\n` +
    JSON.stringify(serious, null, 2), { label: `fix:${label}`, phase: 'Fix', model: 'opus' })
  return fixed === null ? null : { high: serious.some(f => f.severity === 'high') }
}

phase('Implement')
const built = await agent(`${args.brief}\nChange only: ${args.paths}.\nWrite the tests that cite the clauses you implement. ` +
  `Done means \`${args.gate}\` exits 0. Never weaken or skip a test; delete one only together with the code it tests.`,
  { label: `impl:${args.id}`, model: 'opus' })
if (built === null) return { id: args.id, done: false }

const verify = `You did not write task ${args.id}. Check it once against its brief and the spec clauses it names: ${args.brief}`
const first = await pass(`verify:${args.id}`, verify)
if (first === null) return { id: args.id, done: false }
if (first.high && await pass(`verify-2:${args.id}`, verify) === null) return { id: args.id, done: false }
for (let n = 1; n <= (args.security || 0); n++) {
  const broke = await pass(`security-${n}:${args.id}`, `You did not write task ${args.id} (${args.brief}). Try to break ` +
    `it: its cryptography, authentication, pairing, credentials, bridge and parsing of untrusted input.`)
  if (broke === null) return { id: args.id, done: false }
}
return { id: args.id, done: true, lows }
```

After it returns, the lead runs the gate, commits, pushes and reads CI, and writes the low findings and the task's tokens into the status file. `done: false` means an agent failed: run the task again.

**Other shapes.**
- *Spec work:* drafters, pairwise interface reviews, the two-readers test (§6.1).
- *Porting:* copy the code, then test it against the recorded transcripts, before changing behaviour.
- *Choosing between designs* inside a stage: two or three independent attempts, judged against the spec and the gate, the best one kept.
- *Stage exit:* one agent that did no work in the stage reads the status file, the diff and the spec's clause index, and lists what is missing. What it finds of high or medium severity is the next work; the rest is listed.

## 8. Where SYNC.md's phases went

| SYNC.md phase | Stage |
|---|---|
| 0 Hygiene | `foundation` (lockfile, CI); the rest replaced by `seal` and `device` |
| 1 Seal and relay | `seal`, `relay` |
| 2 Hub on the desktops | dropped (§2), except stable block ids (`core`) and the fetchers (`device`) |
| 3 Android app | `android-shell` |
| 4 Deterministic core | `core` |
| 5a Desktop replicas | `engine`, `device`, `desktop` |
| 5r Recovery and backups | `recovery`, `device` |
| 5b Phone replica | `android` |
| 6 Reminders | `android` |
| 7 Operations and hardening | `engine` and `device` (operations), `release` (nightly simulator, fuzzing, rehearsals), `rc` (whole-system security pass) |
| Documentation | `release` |
| 8, 9 Optional | §6.13 |
