# TaskDeck sync specs: common conventions

> **Status: first drafts.** Stage `foundation` has not exited. The six specs in this folder are complete first drafts; they have not yet been through the interface reviews or the two-readers test of SYNC_PLAN.md §6.1, and they are not yet the contract of §3.3. No later stage starts from them until `docs/sync/status/foundation.md` says `final` and this notice is gone.

*The conventions every spec in this folder follows, the notation and encodings they share, and the glossary. This file holds no clauses of its own. It points to [`SYNC.md`](../../../SYNC.md) for the design and to [`SYNC_PLAN.md`](../../../SYNC_PLAN.md) for the plan; it does not restate either.*

## 1. Status and precedence

- Once stage `foundation` merges, the specs in `docs/sync/spec/` are the contract for every later stage (SYNC_PLAN.md §3.3).
- SYNC_PLAN.md overrides SYNC.md; its §2 lists every difference. The specs give the byte-level and behavioural detail that both leave open. A spec agrees with SYNC_PLAN.md and, except where SYNC_PLAN.md §2 changes it, with SYNC.md. A drafter who cannot make a spec agree records the point under "Open questions" with the default taken.
- A stage that finds a spec wrong or ambiguous does not improvise. It opens a spec-change pull request that changes the spec and the affected vectors and adds one line to [`CHANGES.md`](CHANGES.md); merges it; and names the affected stages in its status file and to the owner.
- References use these forms: "SYNC.md §4.2", "SYNC_PLAN.md §6.1", "seal.md §5", and a clause by its id alone ("SEAL-FRAME-003").

## 2. The documents

| File | Scope | Owning stage | Main consumers |
|---|---|---|---|
| `core.md` | Board data, intents, log commands v1 (including the palette and home-place settings), `Ctx`, the origin's compile step, ids and stable block ids, conflict rules, rules versions, canonical encoding and state hash, zone-aware read queries | `core` | `engine`, `device`, `recovery`, `desktop`, `android` |
| `seal.md` | Frame, envelope, signature contexts and their registry, key hierarchy, membership and every control entry, chain, pairing, sealed files, backup envelope | `seal` | `relay`, `engine`, `device`, `recovery` |
| `relay.md` | Every route and socket message, authentication, sequencing and idempotency, caps, budgets, errors, storage schema, compaction and purge, push and plain HTTPS polling | `relay` | `engine` (its model relay), `device`, `recovery` |
| `engine.md` | The replica state machine: confirmed and pending state, outbox, send and reconcile, receipt checks, refusals and halts, checkpoints, snapshots, compaction, evidence, beats, rules upgrades, Skip, timed windows, duties with no always-on device | `engine` | `device`, `desktop`, `android` |
| `device.md` | The facade the two apps use, the on-disk store and log format, transport, test-only trust, the key-store interface, fetcher rules, blobs, backup scheduling | `device` | `desktop`, `android`, `recovery`, `release` |
| `bridge.md` | The phone page's routes and verbs answered natively, every change to the forked page, the allow-list, the JNI surface, WebView settings | `android-shell` | `android` |

The scopes are SYNC_PLAN.md §6.1 item 3. The owning stage maintains the spec after `foundation` merges, through spec-change pull requests.

## 3. Normative language

- The key words MUST and MUST NOT are to be interpreted as described in BCP 14 (RFC 2119, RFC 8174) when, and only when, they appear in all capitals.
- Only MUST and MUST NOT statements are clauses, and every clause has an id (§4). Every MUST or MUST NOT in a spec stands inside a clause.
- SHOULD, SHOULD NOT, RECOMMENDED and MAY are allowed in prose. They carry no id and no test duty.
- Specs do not use SHALL, SHALL NOT, REQUIRED, NOT RECOMMENDED or OPTIONAL; they write MUST, MUST NOT, SHOULD or MAY instead.
- Lowercase "must" has its ordinary English meaning and is not normative. Text outside clauses is explanatory, except tables a clause points to (for example a layout or a limit).

## 4. Clauses

### 4.1 Format

Each clause is one markdown list item whose first line starts exactly like this:

```text
- **CORE-APPLY-012** (core) `apply` MUST refuse ...
```

- **Id:** `<SPEC>-<TOPIC>-<NNN>`.
  - `SPEC` is `CORE`, `SEAL`, `RELAY`, `ENG`, `DEV` or `BRIDGE`, for `core.md`, `seal.md`, `relay.md`, `engine.md`, `device.md` and `bridge.md` in that order. A clause appears only in the spec its prefix names.
  - `TOPIC` is 2 to 8 characters from `A`–`Z` and `0`–`9`, starting with a letter.
  - `NNN` is exactly three decimal digits.
- **Owners:** after the id, one space, then a parenthesised list of owner stages separated by a comma and one space, for example `(seal, relay)`. The owners are the stages whose tests must cite the clause. Allowed names: `core`, `seal`, `relay`, `engine`, `device`, `recovery`, `desktop`, `android-shell`, `android`, `release`, `rc`. A clause has several owners when several stages must each prove it, for example a signature layout that both the Rust clients and the JavaScript relay implement.
- **Withdrawn:** the single word `withdrawn` in place of the owner list retires an id. The line stays in the spec, with the reason and, if any, the id that replaces it.
- **Text:** after the owner list, one space, then the requirement. It contains MUST or MUST NOT. It may continue on indented lines of the same list item. Nested list items inside a clause belong to it and do not start with an id.

The regular expression a tool uses, applied to each line of the six spec files:

```text
^\s*[-*]\s+\*\*((?:CORE|SEAL|RELAY|ENG|DEV|BRIDGE)-[A-Z][A-Z0-9]{1,7}-[0-9]{3})\*\*\s+\(([a-z0-9, -]+)\)\s
```

Group 1 is the id; group 2 is the owner list, or `withdrawn`. Tools read clauses only from the six spec files; this README and `CHANGES.md` contain no clauses, and the example above is not one. A spec does not put a line of this shape inside a code block, a table or a quotation. A spec mentions another clause by its bare id in prose, never in bold at the start of a list item.

### 4.2 Rules

- One requirement per clause.
- Testable as written: a test can be written that fails when the clause is violated.
- Self-contained: the clause names the actor (for example "the relay", "a device", `apply`) and the observable result. It may point to a layout, table or limit in its own spec by section.
- Ids are unique across all six specs.
- Once `foundation` has merged, an id is never reused or renumbered, and its meaning is changed only through `CHANGES.md`. Numbers within a topic need not be contiguous.
- Within `foundation`, drafters number as they write; ids may still change until the stage merges.

### 4.3 How tests cite clauses

- A test cites a clause by the marker `spec: <ID>`, for example `// spec: CORE-APPLY-012`, in a comment on or directly above the test, or by having the id, spelled exactly, in the test's name. One marker names one id; a test may carry several markers. Rust test names cannot contain `-`, so Rust tests use the comment form.
- A citation is a claim that the test fails when the clause is violated.
- `scripts/spec-coverage.sh <stage>` fails if any clause owned by that stage is cited by no test file under that stage's test paths. That script, written later in `foundation`, is the source of truth for which paths count as test files. Withdrawn clauses are not counted.

## 5. Notation and encodings

These apply to every spec. A spec does not redefine them.

### 5.1 Byte layouts

- `‖` is concatenation of byte strings.
- `name[n]` is a field of exactly n bytes.
- `u8`, `u16`, `u32`, `u64` are unsigned integers; `i64` is two's complement.
- Every fixed-width integer on the wire and in every sealed file is big-endian (network byte order).
- Variable-length fields always carry an explicit length prefix, and the clause that defines the field states the prefix's width. There are no NUL-terminated strings except context strings.
- An ASCII literal in quotes in a layout, such as `"TDF1"`, is those bytes with no terminator. In prose a single byte may be written `0x00`; that is notation, not an encoding.

### 5.2 Context strings

- A context string is ASCII of the form `taskdeck/v1/<use>` followed by exactly one `0x00` byte. SYNC.md writes that byte as `\0`.
- Every signature, MAC, hash-chain step, key-derivation info and AEAD associated data starts with a context string that names its use.
- Because of the terminator, no context string is a prefix of another.
- The registry of all context strings is a section of `seal.md`. No other spec introduces one except by adding it to that registry. Labels in SYNC.md that are not in this form (for example `HKDF(s, "ticket")` in SYNC.md §5.3, or the `info` of SYNC.md §5.2) are replaced by registered context strings in `seal.md`.
- The hub contexts `hub-req` and `hub-resp` are dropped (SYNC_PLAN.md §2) and are never registered.

### 5.3 Text and JSON

- Text is UTF-8 without a byte-order mark. Protocol code never normalises Unicode.
- A clause that limits a length says whether it counts bytes or Unicode scalar values.
- JSON is RFC 8259, in UTF-8.
- Parsers of replicated or relayed JSON are strict: unknown fields, duplicate keys, floating-point numbers and trailing data are errors.
- Integers that can reach the phone page stay within −(2^53 − 1) to 2^53 − 1.
- The canonical encoding (sorted keys, omitted defaults and the rest) is defined once, in `core.md`. Other specs refer to it and do not define their own.

### 5.4 Other encodings

- Hexadecimal is lowercase, without a prefix.
- "base64" means RFC 4648 §4 (standard alphabet, with padding) unless a clause says otherwise.
- Instants are UTC. In binary layouts an instant is an `i64` count of milliseconds since 1970-01-01T00:00:00Z, leap seconds ignored. In JSON an instant is an RFC 3339 string ending in `Z`; `core.md` fixes the exact textual form.

### 5.5 Primitives

Used everywhere under these names, without redefinition:

| Name | Meaning |
|---|---|
| SHA-256 | FIPS 180-4 |
| HKDF | HKDF-SHA256 (RFC 5869) |
| AEAD | XChaCha20-Poly1305, 24-byte nonce, 16-byte tag |
| signature | Ed25519 (RFC 8032), verified with strict rules: the `verify_strict` behaviour of ed25519-dalek (canonical S, no small-order public key or R) |
| HPKE | RFC 9180 base mode with DHKEM(X25519, HKDF-SHA256), HKDF-SHA256 and ChaCha20-Poly1305 |

### 5.6 Shared identifiers

| Name | Width | Meaning | Spec that owns first values, genesis and the rest |
|---|---|---|---|
| `board_id` | `[16]` | the board's self-certifying id | `seal.md` |
| `dev` | `[16]` | a device's random id | `seal.md` |
| `entry_id` | `[16]` | random, one per log entry | `seal.md` (layout), `engine.md` (use) |
| `seq` | `u64` | log position, assigned by the relay | `relay.md` |
| `ctr` | `u64` | per-device counter | `relay.md` (acceptance) |
| `epoch` | `u32` | key epoch | `seal.md` |
| `gen` | `u32` | a token generation | `relay.md` |
| `device_no` | small integer | given to a device when its DeviceAdded entry is applied | `seal.md` (membership) |
| item and block ids | integer | `device_no * 2^40 + n`, always below 2^53 | `core.md` |

The chain and membership belong to `seal.md`.

### 5.7 Version markers

Everything in this build is version 1: the protocol path prefix `/v1`; context strings `taskdeck/v1/...`; envelope `ver = 1`; `suite = 1`; rules version 1; canonical encoding v1.

## 6. Sections of every spec

Each spec starts with a level-one title and has these sections, in this order, as level-two headings of the form `## <n>. <Title>`, numbered from 1 without gaps. Scope is `## 1. Scope` and Terms is `## 2. Terms`; the normative sections are 3 to n; the six closing sections take the numbers that follow. Every heading except the normative ones uses exactly the title in bold below. Subsections are numbered `<n>.<m>`, so "seal.md §5.2" names one place.

- **Scope.** What the spec covers, its non-goals, and the SYNC.md and SYNC_PLAN.md sections it realises.
- **Terms.** Only terms not in this README's glossary (§9).
- Normative sections, as many as needed, with titles of the drafter's choice, holding the clauses.
- **Error catalogue.** A table with columns: code, meaning, raised when, what the caller does. Codes are lowercase, `<spec>/<name>`, where `<spec>` is the spec's file name without `.md` (`core`, `seal`, `relay`, `engine`, `device`, `bridge`) and `<name>` is `[a-z][a-z0-9_]*`, for example `seal/bad_signature`.
- **Limits.** A table with columns: name, value, unit, the clause id that enforces it.
- **Test vectors.** The families of vectors the owning stage must publish, their file format (§7) and their location.
- **Interfaces.** What this spec requires from and provides to each other spec, by clause id or section.
- **Public API.** The Rust, JavaScript or Kotlin public types, traits and functions the skeleton must expose, as signatures.
- **Open questions.** Non-normative. Each states the default taken.

The error catalogue, limits and test-vector sections are tables and lists that point to clauses; they hold no clauses themselves. A section with nothing to say keeps its heading and says "None."

## 7. Test vectors

- JSON files, one family per file.
- The top level is an object with a `"spec"` field naming the spec and section (for example `"seal.md §4"`) and a `"cases"` array.
- Each case is an object with a `"name"`, its inputs and its expected outputs.
- Byte strings are lowercase hex.
- Vectors with real cryptographic values are published by stage `seal` under `docs/sync/vectors/`. Golden command logs belong to stage `core`. Every other family lives under its owning stage's paths (SYNC_PLAN.md §4), at the location its spec names.
- In illustrative examples inside a spec, cryptographic outputs may be symbolic, for example `SIG(frame)` or `AEAD(K_entry,e, plain)`. Examples are not vectors.

## 8. Changes

[`CHANGES.md`](CHANGES.md) records every change to a spec after `foundation` merges, and the ambiguities the two-readers test (SYNC_PLAN.md §6.1) left open.

## 9. Glossary

One or two lines per term. "Defined in" names the spec that defines the term normatively; where two are named, each defines the part that falls in its scope (§2), and neither restates the other. There is no hub and no always-on device (SYNC_PLAN.md §2).

| Term | Meaning | Defined in |
|---|---|---|
| board | The shared data (`BoardData`) together with its log, keys and membership; named by `board_id`. | `core.md`, `seal.md` |
| device | One installation of TaskDeck with its own keys and `dev`; a member of a board while not revoked. | `seal.md` |
| member | A device in the current membership. The recovery key acts with authority but is not a member. | `seal.md` |
| relay | The blind server (Worker and Durable Object) that authenticates, sequences, stores and pushes frames, and cannot read them. | `relay.md` |
| standby relay | The same relay core on Node 22 with `node:sqlite`, polled over plain HTTPS; a re-home target and the local relay for tests. | `relay.md` |
| replica | A device's full copy of the board and its log, kept by the engine. | `engine.md` |
| origin | The device where an intent is made and compiled. Only the origin decides policy. | `core.md` |
| intent | A user action as the UI expresses it, possibly with naive local times. Never replicated. | `core.md` |
| log command | A compiled, replicated command in UTC; the only way the board changes. | `core.md` |
| compile step | The origin's conversion of an intent into log commands: defaults, trimming, zone conversion, explicit ids. | `core.md` |
| apply | The pure, deterministic, versioned function that applies a log command to `BoardData` under a `Ctx`. | `core.md` |
| rules version | The version of what `apply` accepts and does; changed only by a sequenced RulesUpgrade. | `core.md`, `engine.md` |
| entry | One element of the log: a frame the relay sequenced at a `seq`. | `seal.md`, `relay.md` |
| frame | The signed byte string a device appends: header, envelope and signature. | `seal.md` |
| envelope | The frame's body: version, suite, epoch, nonce and AEAD ciphertext of the padded plaintext. | `seal.md` |
| kind | The plaintext field that says what an entry carries: `cmd`, `ctl`, `beat` or `snap`. | `seal.md` |
| control entry | An entry of kind `ctl`. It holds an ordered list of control items, applied all or nothing: Genesis, DeviceAdded, DeviceRevoked, DeviceReinstated, EpochRotated, RecoveryKeyChange, Fence, ResetTo, Skip, RulesUpgrade. | `seal.md`, `engine.md` |
| Genesis | The control item in the log's first entry (seq 1), signed by the recovery identity; it makes `board_id` self-certifying and names the first device, epoch 1 and the genesis state. | `seal.md`, `engine.md` |
| seq | The `u64` log position the relay assigns. | `relay.md` |
| ctr | The `u64` per-device counter in each frame; the relay accepts only the next value. | `relay.md` |
| entry_id | The random 16-byte id in each plaintext; makes a re-sealed entry recognisable. | `seal.md`, `engine.md` |
| based_on | The seq and chain value of the author's head when it composed the entry. | `seal.md`, `engine.md` |
| head | The latest seq a device holds, with its chain value. | `seal.md` |
| chain value | C_s, the hash-chain value after entry s (SYNC.md §4.8). | `seal.md` |
| epoch | The `u32` number of the current key generation. | `seal.md` |
| epoch root | R_e, the 32 random bytes from which an epoch's keys derive, wrapped to each member and the recovery key. | `seal.md` |
| membership | The set of devices and their keys, derived from control entries in log order. | `seal.md` |
| device_no | The small integer a device receives when its DeviceAdded entry is applied; the high part of the ids it mints. | `seal.md`, `core.md` |
| recovery seed | 256 random bits, kept only on paper, from which the recovery key derives. | `seal.md` |
| recovery sheet | The paper that holds the recovery seed in its paper encoding, with the relay address and the release-key fingerprint. | `seal.md` |
| recovery key | The recovery HPKE and signing key pair derived from the seed; the root of membership authority. | `seal.md` |
| recovery identity | The recovery signing key as an author of frames: it has a `dev` of its own derived from that key, its own `ctr`, and no token. | `seal.md`, `relay.md` |
| pairing | Adding a new device through a ticket, slots, a SAS and a bundle (SYNC.md §5.3). | `seal.md`, `relay.md` |
| ticket | A single-use, short-lived pairing registration at the relay, derived from the typed code's secret. | `seal.md`, `relay.md` |
| slot | One pairing message (A, B, C or D) posted to a ticket. | `seal.md`, `relay.md` |
| SAS | The six-digit short authentication string the user copies from the existing device into the new one. | `seal.md` |
| bundle | Slot D: what a new device needs to join, sealed to it. | `seal.md` |
| token | A relay bearer token with a role (`device` or `backup-read`) and a `gen`, sent only in the `Authorization` header. | `relay.md` |
| door | A short per-board value the relay's Worker can check without state. It lets a request with no token reach the pre-token routes (pairing as the new device, login, recovery-signed operations) and nothing else. Not a secret that protects data. | `relay.md` |
| relay address | What a person types or pastes to name a board on a relay: the host, then Crockford base32 of `board_id ‖ door`. Shown by the pairing sheet, named by a Fence, printed on the recovery sheet. | `relay.md`, `seal.md` |
| admin operation | A relay operation that needs a fresh admin signature from a member or the recovery key; a token alone never suffices. | `relay.md`, `seal.md` |
| confirmed state | The state reached by applying sequenced entries in seq order. | `engine.md` |
| pending state | The confirmed state with this device's unsequenced entries applied on top; what the UI shows. | `engine.md` |
| outbox | A device's persistent queue of compiled entries not yet acknowledged. | `engine.md`, `device.md` |
| rebase | Recomputing the pending state over a newer confirmed state. | `engine.md` |
| refusal | An entry that every device treats as not applied, reported to its author. | `engine.md` |
| notice | A persistent per-device message that is not a refusal, such as an overwrite notice. | `core.md`, `engine.md` |
| skip (of a frame) | Dropping a frame with an unknown author or a bad signature: no counter value consumed, a tampering alarm raised. Lowercase. | `engine.md` |
| Skip | The control entry `Skip{seq, frame_hash}` that makes every device treat one entry as refused. Capitalised. | `seal.md`, `engine.md` |
| halt | A device stops applying entries; only in the cases `engine.md` lists. | `engine.md` |
| alarm | An integrity alarm; raised only with evidence. | `engine.md` |
| evidence | The data that proves an alarm, shown to the user. | `engine.md` |
| banner | A persistent message on every device for each membership change, kept until acknowledged. | `engine.md` |
| beat | A daily entry of kind `beat`: head, state hash, supported versions, snapshot acks. | `engine.md` |
| snapshot | The sealed `BoardData` at a seq S ≡ 0 mod 64, stored as a blob and announced by a `snap` entry. | `engine.md`, `seal.md` |
| checkpoint | A device's local sealed copy of its state and sync metadata. | `device.md`, `engine.md` |
| base checkpoint | A weekly checkpoint kept 400 days, from which a device replays. | `device.md`, `engine.md` |
| compaction | The relay's deletion of old entries that an acknowledged snapshot covers. Control entries are kept. | `relay.md`, `engine.md` |
| purge | "Purge history before now": truncating every log and base, and rotating the epoch. | `engine.md`, `relay.md` |
| fence | The Fence control entry, and the relay state it sets, that ends a board's use of a relay. | `seal.md`, `relay.md` |
| move code | The code shown on the device that moves the board; typed on each other device before it follows. | `engine.md` |
| restore tail | Re-uploading frames at their original seqs after a relay rollback. | `relay.md`, `engine.md` |
| duty | Work some device must do (snapshot, beat, compaction, backup); done by whichever device is awake. | `engine.md`, `device.md` |
| timed window | A period that spans entries and the clock, such as the 72-hour recovery-key change. | `engine.md` |
| facade | The device runtime's public API, which the desktop app and the Android bridge call. | `device.md` |
| bridge | The channel between the phone page in the WebView and native code; replaces the page's network calls. | `bridge.md` |
| fetcher | A device's HTTP client for ICS feeds or weather. | `device.md` |
| overlay | Events from subscribed ICS feeds, kept per device; never enters `apply`. | `device.md`, `core.md` |
| blob | A sealed binary object stored on the relay in parts, such as a snapshot or the shared picture. | `relay.md`, `seal.md` |
| backup | A file sealed to the recovery key and signed by its writer, holding the board state. | `seal.md`, `device.md` |
| segment | A monthly signed backup file holding that month's frames. | `seal.md`, `device.md` |

## Requirement map

The table that maps every requirement of SYNC.md §3 to §9 to a clause id, a stage of SYNC_PLAN.md, or a row of SYNC_PLAN.md §2 is maintained here. It is the `foundation` gate's requirement map (SYNC_PLAN.md §6.1).

<!-- requirement-map:begin -->
| SYNC.md ref | Requirement | Maps to |
|---|---|---|
| §3 row 1 desktop | Desktops keep the wall layout, UI scale, uncapped loop and idle wind-down; egui unchanged | stage: desktop |
| §3 row 1 Android | Android keeps the agenda as today | plan §2: The phone page |
| §3 row 2 | Ranked tasks with the daily tie-break shuffle are scored by the core on each device and never stored | CORE-READ-003, CORE-READ-002 |
| §3 row 2 Android | Android shows ranked tasks under Tray → All | stage: android |
| §3 row 3 | Calendar grid, agenda, month rail and glance line on every device, drawn over the local replica | CORE-READ-001, DEV-FACADE-004, DEV-THREAD-006 |
| §3 row 4 | New task or event and quick add mint ids on the device | CORE-ID-004, CORE-ID-002, CORE-COMP-004 |
| §3 row 4 Android | Android offers the New sheet and tray quick-add | stage: android |
| §3 row 5 | Planner, estimates, due dates, routines and Reflow compile their times to UTC at the origin | CORE-COMP-003, CORE-COMP-005, CORE-COMP-006 |
| §3 row 5 Android | Android day view offers tap, hold-to-drag and chips | stage: android |
| §3 row 6 | Done, Book and Tomorrow with Undo, Undo being an inverse command; planner equivalents on desktops | stage: android, stage: desktop |
| §3 row 7 desktop | Archive ledger, stats, search, Restore, Forget and ghosts on desktops, in every replica | CORE-SNAP-003, CORE-ID-005, stage: desktop |
| §3 row 7 Android | Android has the Done tab, Restore and ghosts | CORE-SNAP-003, stage: android |
| §3 row 8 | Notepad on every device through SetNotes{base} with a draft and merge view | CORE-CMD-008, DEV-FACADE-005, DEV-FACADE-006 |
| §3 row 9 | Each device asks Open-Meteo about the home place, a board setting at 0.01° | DEV-WX-001, BRIDGE-ROUTE-004 |
| §3 row 9 own place | The phone's own place never syncs | DEV-WX-002, BRIDGE-ROUTE-005 |
| §3 row 10 desktop | Desktops keep backgrounds and Look per device; the palette is a board setting set by SetPalette | CORE-DATA-007, CORE-CMD-002, DEV-PIC-003 |
| §3 row 10 Android | Android shows the desk's palette, its own Look and the desk's picture received as one sealed blob | DEV-PIC-001, DEV-PIC-002, DEV-PIC-004, DEV-PIC-005 |
| §3 row 11 | Each device fetches subscribed ICS feeds itself with ETag, keeping the raw body | DEV-ICS-001, DEV-ICS-005 |
| §3 row 11 overlay | The ICS overlay never enters apply or BoardData | CORE-DATA-007, DEV-ICS-010 |
| §3 row 11 Android | Subscriptions are never managed through the WebView | BRIDGE-ROUTE-008, BRIDGE-ROUTE-003, BRIDGE-SCREEN-001 |
| §3 row 12 | Offline editing on every device, applied at once, shown as confirmed ⊕ pending | ENG-PEND-001, ENG-ENQ-007 |
| §3 row 13 | The /calendar.ics feed is replaced by exports | plan §2: `.ics` feed and exports |
| §3 row 14 desktop | Desktops offer pairing, devices, revocation, sync status, last seen, fingerprint, banners, recovery, backups and restore to a point | stage: desktop |
| §3 row 14 Android | Android offers the same administration, without backups | plan §2: Administration from the phone, BRIDGE-SCREEN-001 |
| §3 row 15 | Android offline reminders with Done, Tomorrow and Snooze from core reminders() and exact alarms | CORE-REM-001, BRIDGE-REM-001, BRIDGE-REM-005, plan §2: Reminders |
| §3 row 7 Android, Phase 8 | Archive search and Forget on Android arrive after cutover | stage: backlog |
| §3 row 11 Android, Phase 8 | A native subscription-management screen on Android arrives after cutover | stage: backlog |
| §3 row 16, Phase 8 | Optional widgets, a tile, a share target and an app lock on Android | stage: backlog |
| §4.1 relay append rows | An append on the relay writes three rows: the log row, its unique-index entry and the device row | RELAY-ROWS-001, RELAY-SEQ-007 |
| §4.1 relay tables | The relay's DO SQLite holds the log, devices, snapshots, blobs, tickets and meta tables as listed | RELAY-STORE-001, RELAY-STORE-002 |
| §4.1 blob parts | Relay blob parts are at most 1 MiB | SEAL-BLOB-001, RELAY-BLOB-002 |
| §4.1 sealed files | Devices keep sealed files in taskdeck_data/sync/ with AD = file kind ‖ dev ‖ board_id | SEAL-FILE-001, SEAL-KEY-014, DEV-STORE-002, DEV-STORE-001 |
| §4.1 log.tdl | log.tdl holds frames with seq and chain value, fsynced per page before applying | ENG-RECV-002, DEV-STORE-006, DEV-FILE-004 |
| §4.1 log retention | Device logs are kept 400 days | ENG-STATE-004, DEV-FILE-011 |
| §4.1 checkpoint | checkpoint.tdk holds BoardData, head, counters, device set and epoch roots, written every 200 entries, every 10 minutes and at exit | ENG-CKPT-001, ENG-CKPT-002 |
| §4.1 bases | Weekly base-<seq>.tdk files are kept 400 days | ENG-CKPT-003, ENG-STATE-004 |
| §4.1 hashes.tdk | hashes.tdk records the state hash at every seq ≡ 0 mod 64 | ENG-HASH-002 |
| §4.1 outbox.tdo | outbox.tdo holds {entry_id, at, based_on, command, ctr?, frame?} and is fsynced before the submit returns | ENG-ENQ-004, DEV-FACADE-002, DEV-CRASH-002, ENG-STATE-003 |
| §4.1 overlay.tdk | overlay.tdk holds the overlay, raw feed bodies and ETags | DEV-STORE-001, DEV-ICS-010 |
| §4.1 load as stored | A replica loads BoardData exactly as stored; normalisation runs only in import-check and genesis | DEV-FILE-010, CORE-DATA-005, CORE-IMP-008 |
| §4.2 frame layout | Frame = "TDF1", board_id, dev, ctr, len, body, sig | SEAL-FRAME-001, SEAL-FRAME-003, SEAL-FRAME-004 |
| §4.2 body layout | Body v1 = ver, suite, epoch, nonce, XChaCha20-Poly1305 ciphertext under K_entry,e with the stated AD | SEAL-ENV-001, SEAL-ENV-002, SEAL-ENV-003 |
| §4.2 plaintext layout | Plaintext = entry_id, kind, at_ms, based_on_seq, based_on_chain, JSON, padding | SEAL-PLAIN-001, SEAL-PLAIN-002, SEAL-PLAIN-003 |
| §4.2 signature contexts | Every signature begins with a NUL-terminated context naming its use, so none can stand in for another | SEAL-CTX-001, SEAL-CTX-002 |
| §4.2 frame signature | The frame signature covers taskdeck/v1/frame\0 ‖ the frame up to sig | SEAL-FRAME-002 |
| §4.2 login signature | The login signature covers context, relay origin, board_id, dev, gen, relay nonce and client nonce; the relay supplies only the nonce | SEAL-SIG-001, SEAL-SIG-002, RELAY-LOGIN-003 |
| §4.2 admin signature | The admin signature covers context, operation, SHA-256(arguments) and relay nonce | SEAL-SIG-003, SEAL-SIG-004, RELAY-ADMIN-003 |
| §4.2 hub signatures | M1 hub request and reply signatures | plan §2: Milestones |
| §4.2 backup signature | The backup signature covers taskdeck/v1/backup\0 ‖ SHA-256(file body) | SEAL-SIG-006 |
| §4.2 kinds | Entry kinds inside the ciphertext are cmd, ctl (with the listed items), beat and snap | SEAL-KIND-001, SEAL-KIND-003, SEAL-CTL-001, SEAL-CTL-003 |
| §4.2 beat | A daily beat carries head, the state hash at the latest seq ≡ 0 mod 64, supported versions and snapshot acks | ENG-BEAT-001, ENG-BEAT-002, SEAL-KIND-005 |
| §4.2 padding | Plaintexts are padded to 512-byte buckets, Padmé above | SEAL-PAD-001 |
| §4.2 caps | Caps of 256 KiB per frame and 1 MiB per blob part | SEAL-FRAME-003, SEAL-PAD-002, SEAL-BLOB-001, RELAY-BLOB-002 |
| §4.2 receipt order | Receipt checks run in order: size, board and author, verify_strict, ver and suite, AEAD, unpad, strict serde, apply | SEAL-RCPT-001, SEAL-ENV-005, SEAL-PAD-003, SEAL-GEN-010 |
| §4.2 strict serde | Log commands parse strictly with deny_unknown_fields | CORE-ENC-005, CORE-CMD-001, ENG-SM-003 |
| §4.2 skipped frames | An unknown author or bad signature is skipped, consuming no counter value, with an alarm | SEAL-RCPT-002, SEAL-MEM-009, ENG-CLASS-002 |
| §4.2 refused frames | A validly signed frame failing a later check before apply is refused on every device, its author's included; no frame can halt a device | SEAL-RCPT-004, SEAL-RCPT-005, ENG-CLASS-004 |
| §4.2 apply refusal | Failing apply is an ordinary refusal reported to the author | ENG-CLASS-005, ENG-REF-001 |
| §4.2 own dev | A frame under this device's dev that it never wrote means "replace this device's keys" | SEAL-RCPT-010 |
| §4.2 based_on tamper | A based_on head at or after the entry's own seq, or off the local chain, is tampering | SEAL-RCPT-011, SEAL-RCPT-012 |
| §4.2 based_on old | A based_on older than the retained log is skipped with a warning | SEAL-RCPT-013, ENG-UI-005 |
| §4.2 halts | Halts happen only on an unknown RulesUpgrade, an entry crashing apply on two consecutive starts, and an epoch-commitment mismatch | ENG-HALT-001, SEAL-RCPT-007, SEAL-RCPT-008, ENG-POISON-003 |
| §4.3 append transport | Appends arrive over the socket or over HTTPS from background jobs, in batches of at most 16 frames | ENG-SEND-001, RELAY-SOCK-006, RELAY-APPEND-001, DEV-NET-014 |
| §4.3 append author | An appended frame's dev must be the authenticated device | RELAY-SEQ-008 |
| §4.3 signature first | The relay checks Ed25519 first, tested with Wycheproof so it is never laxer than the clients | RELAY-SEQ-006, RELAY-GEN-009, RELAY-GEN-010 |
| §4.3 transaction | One transaction then re-reads the device row | RELAY-OBJ-006, RELAY-SEQ-007 |
| §4.3 next counter | ctr = last_ctr + 1 is stored under the next seq and acknowledged only once durably committed | RELAY-SEQ-001, RELAY-OBJ-007, RELAY-SEQ-010 |
| §4.3 lower counter | A lower ctr returns the stored seq if the hash matches and 409 conflict with the stored frame if not | RELAY-SEQ-002 |
| §4.3 higher counter | A higher ctr gets 409 gap | RELAY-SEQ-003 |
| §4.3 one socket | The relay keeps one live socket per device and reports a second one to both ends | RELAY-SOCK-003, ENG-SESS-003 |
| §4.3 rule 1 enqueue | Enqueue compiles, trial-applies under catch_unwind (a panicking command is never queued), adds a random 128-bit entry_id and fsyncs the outbox | ENG-ENQ-001, ENG-ENQ-002, ENG-ENQ-003, ENG-ENQ-004 |
| §4.3 rule 2 send | Counters are assigned at seal time; seal, fsync the frame, transmit; retries resend identical bytes | ENG-SEND-002, ENG-SEND-003, SEAL-FRAME-005, ENG-SEALER-005 |
| §4.3 rule 3 reconcile | On hello, if the relay's last_ctr is ahead, fetch those frames, check this device signed them and drop outbox entries already sequenced | ENG-RECON-001, ENG-RECON-002, ENG-RECON-004 |
| §4.3 rule 3 restored copy | A restored folder or VM catches up fully before sending | ENG-RECON-001, ENG-SEND-007, ENG-UI-004 |
| §4.3 rule 4 conflict | On 409 conflict a matching entry_id is the ack; otherwise warn, re-seal, and stop after three repeats | ENG-RECON-005, ENG-RECON-006 |
| §4.3 rule 5 echo | Acknowledged own entries must return at their seq with the same hash | ENG-ECHO-001, ENG-ECHO-002 |
| §4.3 rule 6 lock | One identity per folder: a contended lock refuses sync | DEV-LOCK-001, DEV-LOCK-002, ENG-SESS-004 |
| §4.3 rule 6 cloud folders | Cloud-synced data folders are refused | DEV-OPEN-001 |
| §4.3 rule 6 no deletion | No relay answer ever deletes local data or keys | ENG-SESS-005, DEV-NET-012, ENG-PERSIST-005 |
| §4.3 old idempotency | (dev, ctr) and entry_id replace X-TaskDeck-Request and the 1,024-reply cache | stage: desktop |
| §4.4 item ids | Item ids are device_no << 40 ‖ n, below 2^53 | CORE-ID-001, CORE-ID-004 |
| §4.4 legacy ids | Legacy ids stay below 2^40 | CORE-IMP-005 |
| §4.4 device_no | device_no is assigned when DeviceAdded is applied: the next free number in log order | SEAL-MEM-013, SEAL-MEM-014 |
| §4.4 register with DeviceAdded | E registers N's key in the same relay call, so N appends only from that seq, after reading its number | RELAY-REG-001, ENG-OPS-001, SEAL-PAIR-012, ENG-BOOT-003 |
| §4.4 high-water marks | BoardData keeps each device's id and sid marks and apply requires newly minted numbers above them | CORE-ID-002, CORE-ID-003, CORE-ID-004 |
| §4.4 restore id | Restore reuses the archived id if free, else its fallback id | CORE-ID-005 |
| §4.4 block sids | Blocks get stable sids carried in the existing session field, which the page only compares and echoes | CORE-INT-003, CORE-SNAP-002, CORE-IMP-006 |
| §4.4 deleted code | CLIENT_ID_FLOOR, Outbox::remap, known and number_from are deleted | stage: desktop |
| §4.4 subscription ids | The subscription-id seeding bug is fixed | plan §2: Phase 0 hygiene, CORE-ID-002 |
| §4.5 signature | apply(&mut BoardData, &LogCommand, &Ctx) returns a change set that a Store writes | CORE-APPLY-003, DEV-LOCAL-001 |
| §4.5 nothing ambient | apply uses no clock, zone, I/O, randomness, floating point, hash-map iteration or Vec-order dependence | CORE-GEN-003, CORE-GEN-006, CORE-CTX-002 |
| §4.5 core dependencies | taskdeck-core has no file or network dependencies; clippy disallows now() and chrono::Local | CORE-GEN-005, CORE-GEN-004 |
| §4.5 thin apply | The origin's compile step decides policy (defaults, fallback ids, trimming); apply checks existence, ranges and lengths | CORE-CMD-006, CORE-COMP-001, CORE-COMP-002 |
| §4.5 rules versions | BoardData.rules is the active version; any change to what apply accepts is a new version; old variants stay, pinned by golden logs | CORE-RULES-001, CORE-RULES-003 |
| §4.5 RulesUpgrade | A version activates only through a sequenced RulesUpgrade{v}, valid only if every active member (beat within 30 days) supports v | SEAL-MEM-019, SEAL-MEM-020, SEAL-MEM-022, ENG-SM-002 |
| §4.5 override | An override needs the author and one other member | SEAL-MEM-021, ENG-RULES-002 |
| §4.5 writers | Writers never emit what the active rules forbid, and the UI names the blocker | CORE-RULES-004, ENG-RULES-001 |
| §4.5 unknown RulesUpgrade | A device meeting an unknown RulesUpgrade halts, shows its state, holds new edits and takes no snapshot, compaction or backup duty | SEAL-RCPT-008, ENG-HALT-005, ENG-HALT-006, ENG-HALT-007 |
| §4.5 time | Ctx.at is the only clock apply sees; it sets created and the archive default and clamp (at most at + 1 day) | CORE-CTX-003, CORE-CMD-005, CORE-COMP-007 |
| §4.5 relay time | Relay time never stamps anything | ENG-CLOCK-001, RELAY-STORE-006, ENG-WINDOW-001 |
| §4.5 clock skew | Skew over 5 minutes against relay_now warns; over an hour blocks composing until the user confirms the clock | ENG-CLOCK-003, ENG-CLOCK-002 |
| §4.5 UTC types | Core types are UTC, with year bounds checked in UTC | CORE-TIME-001 |
| §4.5 naive times | Intents may carry naive local times converted and clamped in the origin's zone; log commands take RFC 3339 with Z only | CORE-INT-002, CORE-COMP-003, CORE-CMD-003, CORE-ENC-007 |
| §4.5 routines and events | MoveRoutine and CreateRoutine float and never call resolve_on_day; events use SetEventSpan{start, minutes} | CORE-CMD-007, CORE-CMD-002 |
| §4.5 device zone | Each device draws in its own zone | CORE-READ-001, DEV-FACADE-004, DEV-SET-002, BRIDGE-ZONE-001 |
| §4.5 plans | Reflow compiles to MoveBlocks and Unplan to RemoveBlocks{sids seen} | CORE-COMP-005, CORE-COMP-006 |
| §4.5 canonical encoding | Canonical encoding v1: sorted keys, defaults omitted, UTC nanosecond timestamps, integers only | CORE-ENC-001, CORE-ENC-002, CORE-ENC-007, CORE-ENC-005 |
| §4.5 archive order | Archive rows are totally ordered (archived_at, id, bytes); unreadable lines kept in base64 | CORE-DATA-003, CORE-DATA-002, CORE-IMP-004 |
| §4.5 state hash | The versioned state hash also covers the device set, epochs, counters and id marks | ENG-HASH-001 |
| §4.5 beat versions | Beats compare only matching state-hash versions | ENG-SNAP-005 |
| §4.5 golden hashes | CI checks golden hashes against earlier releases | CORE-RULES-003 |
| §4.5 ranking | §14.4 ranking is untouched | CORE-READ-003 |
| §4.5 panic unwind | Release builds use panic = "unwind" | stage: foundation |
| §4.5 poison pages | Pages of up to 256 entries are applied under catch_unwind, one marker per page | ENG-POISON-001, ENG-GEN-012 |
| §4.5 hard abort | After a hard abort the page is replayed entry by entry | ENG-POISON-002 |
| §4.5 poison halt | A device halts only if the same entry fails twice in a row, naming its author and offering Skip | ENG-POISON-003, ENG-HALT-003 |
| §4.5 feed panic | A feed that panics is quarantined | DEV-ICS-009 |
| §4.5 determinism gates | Proptest determinism and rebase suites run under two TZ values | CORE-CTX-002, ENG-MODEL-002 |
| §4.5 golden logs | Golden logs per rules version with boundary values are replayed on all three operating systems and the phone | CORE-RULES-003 |
| §4.5 fuzzing | cargo-fuzz on frames, commands plus apply, ics::parse and build_day | CORE-GEN-008, CORE-GEN-007, SEAL-GEN-011 |
| §4.5 poison pills, release profile | Release builds use panic = "unwind", so a panic in a handler is caught | stage: foundation |
| §4.5 display order | Read queries never reorder `board.items`; display order is the caller's, and the calendar summary breaks ties on (start, id, sid) | CORE-READ-005, CORE-READ-006 |
| §4.6 ordering | The last writer wins per command, in seq order, identically everywhere | ENG-RECV-005, CORE-CTX-002 |
| §4.6 overwrite stamps | BoardData records the last (seq, device) of each item and block | CORE-CONF-001 |
| §4.6 overwrite notices | A command overwriting another device's newer change still applies and both devices get a notice | CORE-CONF-002, ENG-REF-003 |
| §4.6 MoveBlocks | MoveBlocks skips blocks changed or gone since based_on | CORE-CONF-003 |
| §4.6 RemoveBlocks | RemoveBlocks removes only blocks its origin saw | CORE-CONF-004, CORE-COMP-006 |
| §4.6 benign repeats | Completing or deleting an archived item, or restoring a live row, is a no-op success | CORE-CONF-005, CORE-APPLY-004 |
| §4.6 notice list | Refusals and notices go to a persistent per-device list | ENG-REF-001, ENG-REF-002, CORE-CONF-006 |
| §4.6 notes base | SetNotes{text, base}: a stale base is refused and the origin keeps a draft with a merge view | CORE-CMD-008, DEV-FACADE-006 |
| §4.6 notes draft | The notepad edits a draft; saves coalesce to one per 30 s while typing, plus blur and exit | DEV-FACADE-005, DEV-FACADE-008 |
| §4.6 notes cap | Rules v1 counts the notes length cap after detab | CORE-CMD-006, CORE-COMP-001 |
| §4.6 naming hold | A Create being named is held only while it is the outbox's last entry | ENG-HOLD-001, ENG-HOLD-004 |
| §4.6 naming merge | Create then Forget never leaves the device; Create then Rename leaves as one Create | ENG-HOLD-003, ENG-HOLD-002 |
| §4.6 Skip author | Skip{seq, frame_hash} is signed by a member other than the author or by the recovery key and never targets a control entry | SEAL-MEM-011, SEAL-RCPT-009, SEAL-MEM-026 |
| §4.6 Skip effect | Every device then treats the entry as refused, replaying from a base if it had applied it | ENG-SKIP-002, ENG-SKIP-001, ENG-SKIP-003 |
| §4.6 read-ahead | Halted devices read ahead for Skip and control entries, so revocations still land | ENG-HALT-003, ENG-CTL-001, ENG-HALT-004 |
| §4.7 view | The view is the confirmed state plus pending entries, rebased per batch; doomed entries are marked | ENG-PEND-001, ENG-PEND-002, ENG-PEND-005, ENG-PEND-004 |
| §4.7 offline | Offline use is unlimited, with a warning at 10,000 queued | ENG-ENQ-007 |
| §4.7 catch-up pages | Catch-up reads pages of at most 256 entries or 1 MiB, one fsync each | RELAY-READ-001, ENG-RECV-001, ENG-RECV-002 |
| §4.7 catch-up speed | Two weeks away (about 4,200 entries) catches up in seconds | DEV-E2E-003 |
| §4.7 snapshot seqs | Snapshots are taken at S ≡ 0 mod 64, where every device records its state hash | ENG-HASH-002, ENG-SNAP-001 |
| §4.7 snapshot acks | Each device that applied S acks the snap entry, or raises a mismatch alarm, in its next beat | ENG-SNAP-005, ENG-BEAT-002 |
| §4.7 snapshot sealing | A snapshot is one AEAD message split into parts of at most 1 MiB, hash-checked before decryption and inflated under a cap | SEAL-BLOB-001, SEAL-BLOB-002, SEAL-BLOB-003, SEAL-BLOB-008 |
| §4.7 snapshot writer | The Windows desktop writes snapshots weekly or every 1,000 entries; any current, unhalted device when the newest acknowledged one is old | plan §2: Always-on device, ENG-SNAP-001, ENG-SNAP-002 |
| §4.7 snapshot retention | The relay keeps the newest three snapshots plus each author's newest | RELAY-BLOB-005, RELAY-BLOB-007 |
| §4.7 compaction | Compaction deletes entries older than 365 days covered by a snapshot acknowledged by another device, in daily slices of at most 5,000 | RELAY-COMPACT-003, RELAY-COMPACT-005, ENG-COMPACT-001, ENG-COMPACT-002 |
| §4.7 compaction keeps | Compaction keeps control entries | ENG-COMPACT-003, RELAY-COMPACT-002 |
| §4.7 bootstrap | A new device bootstraps from a fresh snapshot of the pairing device's state, verified up to the given head | SEAL-PAIR-009, SEAL-PAIR-011, ENG-BOOT-006 |
| §4.7 long absence | A device away more than 365 days uses the newest acknowledged snapshot and is told continuity could not be checked | ENG-BOOT-002, ENG-UI-003, ENG-BOOT-001 |
| §4.7 retention notice | The Forget dialog says deleted text lingers in the relay log, snapshots, device logs and bases, and backups | stage: desktop |
| §4.7 purge | Purge history snapshots, truncates every log and base, rotates the epoch, deletes the old roots and, after confirmation, older backups | ENG-PURGE-002, ENG-PURGE-003, RELAY-PURGE-001, DEV-BKP-009 |
| §4.8 chain | Integrity checks: contiguous seq and the hash chain C_s with a persisted head | SEAL-CHAIN-001, SEAL-CHAIN-002, ENG-RECV-003, RELAY-STORE-003 |
| §4.8 counters | Per-device counters rise by one | SEAL-RCPT-001, SEAL-MEM-008, RELAY-SEQ-001 |
| §4.8 last seen | Settings shows each device's last beat | ENG-UI-001, stage: desktop, stage: android |
| §4.8 evidence | An alarm needs evidence: an unechoed own entry, a relay head behind, a chain mismatch, a counter gap with acks, or proof of missing entries | ENG-EVID-001, ENG-SESS-002, ENG-ECHO-001, ENG-RECON-007 |
| §4.8 response | On evidence the device stops sending and applying, keeps editing locally and shows the evidence | ENG-EVID-003, ENG-EVID-002 |
| §4.8 restore tail device | After a plain relay rollback the most advanced device re-uploads frames at their original seqs | ENG-EVID-004, ENG-EVID-007, ENG-DUTY-001 |
| §4.8 restore tail relay | The relay accepts restored frames only with registered signatures and if they extend its chain | RELAY-RESTORE-001, RELAY-RESTORE-002 |
| §4.8 re-home | A fork or lasting withholding leads to a re-home | ENG-EVID-005, ENG-MOVE-001 |
| §4.8 divergence | On a determinism mismatch only the minority halts | ENG-DIVERGE-001, ENG-DIVERGE-002 |
| §4.8 adoption | Adopting another state requires choosing that device by the fingerprint on its own screen | ENG-DIVERGE-003, ENG-DIVERGE-004 |
| §4.8 versioned path | The relay protocol is versioned by path (/v1) and deploys are additive | RELAY-SHAPE-009, RELAY-SHAPE-010 |
| §4.8 hibernation | Hibernated sockets survive deploys | RELAY-SHAPE-008, RELAY-SOCK-010 |
| §4.9 row 1 | Awake desktops and the visible phone app get pushes over the hibernating socket with 30 s pings, p95 < 1 s | RELAY-SOCK-008, RELAY-SOCK-010, DEV-NET-016, DEV-NET-008 |
| §4.9 row 2 | A laptop after sleep reconnects with backoff bypassed once on resume, hello and replay, within 5 s | DEV-BACKOFF-002, BRIDGE-BG-003 |
| §4.9 row 3 | The phone app draws its local replica at once, then catches up: drawn < 0.7 s, fresh < 2 s p90 | BRIDGE-BUDGET-001, BRIDGE-BUDGET-002, plan §2: Android floor |
| §4.9 row 4 | In the background an expedited job sends queued edits and a JobScheduler job pulls every 30 min | BRIDGE-BG-001, DEV-NET-013, DEV-NET-014 |
| §4.9 row 5 | In Doze the phone uses maintenance windows with "Never sleeping apps" onboarding; only fresh on open is promised | stage: android |
| §4.9 row 6 | A due reminder fires on time even offline: exact alarm, sync of up to 5 s, recompute, notify | BRIDGE-REM-001, BRIDGE-REM-002, plan §2: Reminders |
| §4.9 row 7 | M1 latency with the hub | plan §2: Milestones |
| §4.9 row 5, freshness alarm | The optional hourly freshness alarm is not taken: no alarm but the reminder chain | BRIDGE-BG-002 |
| §5.1 AEAD | Entries, snapshots, blobs and local files use XChaCha20-Poly1305 with random 24-byte nonces (chacha20poly1305 0.11.0) | SEAL-GEN-003, SEAL-GEN-012 |
| §5.1 signatures | Signatures use Ed25519 with verify_strict (ed25519-dalek 3.0.0) | SEAL-GEN-003, SEAL-GEN-010 |
| §5.1 HPKE | Wraps, the pairing bundle and backups use HPKE base mode with DHKEM(X25519, HKDF-SHA256) and ChaCha20-Poly1305 | SEAL-WRAP-001, SEAL-BKP-001, SEAL-GEN-003 |
| §5.1 HPKE hub | M1 hub frames use HPKE | plan §2: Milestones |
| §5.1 KDF and hash | Derivation and hashing use HKDF-SHA256 and SHA-256 | SEAL-GEN-003, ENG-GEN-007, RELAY-GEN-001 |
| §5.1 RNG, wiping, comparison | OS RNG, zeroize and constant-time comparison | SEAL-GEN-004, SEAL-GEN-005, SEAL-GEN-006, SEAL-GEN-007 |
| §5.1 TLS | TLS as defence in depth: rustls on ring with the OS trust store through rustls-platform-verifier | DEV-NET-002, DEV-GEN-011, DEV-NET-001 |
| §5.1 one generation | The crypto crate set has no duplicate crypto crates; ring remains only under rustls | stage: foundation |
| §5.1 one verifier | All devices use one pinned ed25519-dalek so verdicts agree; the relay's check only filters | SEAL-GEN-003, RELAY-GEN-010 |
| §5.1 no paid audit | No paid audit by default | plan §2: Cost |
| §5.1 Wycheproof | CI runs Wycheproof vectors | RELAY-GEN-010, SEAL-GEN-010 |
| §5.1 test vectors | CI runs the RFC 9180 vectors, and the test vectors are published | stage: seal |
| §5.1 key entropy | Key material comes only from getrandom, never from the old non-CSPRNG token fallback | SEAL-GEN-005, DEV-GEN-003, plan §2: Phase 0 hygiene |
| §5.1 one generation | One RustCrypto generation, no second version of a cryptographic crate, ring only under rustls | stage: foundation |
| §5.1 assurance | Standard constructions only; RFC 9180 and seal vectors run in CI and are published; the spec is offered for public review | stage: seal, stage: rc |
| §5.2 recovery seed | The recovery seed is 256 bits, kept on paper only | SEAL-KEY-001, SEAL-KEY-004, DEV-SHEET-001 |
| §5.2 recovery HPKE key | The recovery X25519 HPKE key opens backups and a wrap of every epoch root | SEAL-KEY-002, SEAL-WRAP-001, SEAL-BKP-006 |
| §5.2 recovery signing key | The recovery Ed25519 signing key is the root of membership authority | SEAL-KEY-003, SEAL-MEM-004 |
| §5.2 board_id | board_id = SHA-256(context ‖ recovery signing public key ‖ salt)[0..16] | SEAL-KEY-005, SEAL-KEY-017 |
| §5.2 epoch root | The epoch root R_e is 32 random bytes, HPKE-wrapped to each device and to the recovery key | SEAL-KEY-010, SEAL-WRAP-001, ENG-SEALER-007 |
| §5.2 commitment | The rotation entry carries the commitment c_e | SEAL-KEY-011, SEAL-KEY-013 |
| §5.2 per-kind keys | K_kind,e = HKDF-SHA256(board_id, R_e, info with kind and e) for entry, snap and blob | SEAL-KEY-012 |
| §5.2 device keys | A device's Ed25519 and X25519 keys never leave it | SEAL-KEY-009, SEAL-KEY-008 |
| §5.2 storage key | The storage key L_d is wrapped by DPAPI, Keychain or Keystore | DEV-KEY-001, DEV-KEY-003, DEV-KEY-004, BRIDGE-KEY-001 |
| §5.2 relay token | Relay token = "TD1." role, board, dev, gen and an HMAC under TOKEN_KEY; roles device and backup-read | RELAY-TOKEN-001, SEAL-KEY-015 |
| §5.2 HPKE info | HPKE info binds use, board, epoch or seq, and recipient; the suite id is in every AD | SEAL-WRAP-002, SEAL-CTX-001, SEAL-CTX-003 |
| §5.2 token header | Tokens travel only in the Authorization header | DEV-NET-005 |
| §5.2 token checks | The Worker checks tokens statelessly before any DO call and the DO checks state and generation | RELAY-TOKEN-002, RELAY-OBJ-003, RELAY-OBJ-010, RELAY-TOKEN-004 |
| §5.2 login | Devices obtain tokens, also on a new relay after a re-home, by signing a login challenge | DEV-LOGIN-001, DEV-LOGIN-002, RELAY-LOGIN-003, RELAY-LOGIN-004 |
| §5.2 admin operations | Admin operations need a fresh admin signature from a member or the recovery key; a token alone never suffices | RELAY-ADMIN-001, RELAY-ADMIN-002, RELAY-ADMIN-004 |
| §5.2 backup read tokens | Backup read tokens are minted per writer device and generation, revoked with that device and at every rotation | RELAY-RTOKEN-001, RELAY-RTOKEN-002, RELAY-REVOKE-003, DEV-BKP-006 |
| §5.3 step 1 desktop | Pairing on a desktop authenticates with Windows Hello or Touch ID | DEV-FACADE-003 |
| §5.3 step 1 Android | On Android pairing authenticates with BIOMETRIC_STRONG only, no PIN fallback | BRIDGE-AUTH-001, plan §2: Administration from the phone |
| §5.3 step 2 secret | E draws an 80-bit secret s and shows it as XXXX-XXXX-XXXX-XXXX-CC | SEAL-CODE-001 |
| §5.3 step 2 ticket | E registers a ticket from HKDF(s, "ticket"): single use, 10 minutes, 5 tries | SEAL-PAIR-001, SEAL-PAIR-015, RELAY-PAIR-001, RELAY-PAIR-003 |
| §5.3 step 2 slot A | E posts slot A = H(epk_E ‖ r_E) | SEAL-PAIR-004, RELAY-PAIR-002 |
| §5.3 step 2 capture | The code and host show capture-excluded, never on the wall layout | DEV-PAIR-001, plan §2: Pairing window |
| §5.3 step 2 slot MAC | Slots are MAC'd under HKDF(s, "mac") over context, label, sender role and body; the relay never sees s | SEAL-PAIR-002, SEAL-PAIR-003, RELAY-PAIR-004, RELAY-PAIR-005 |
| §5.3 step 3 code field | On N the code is typed into a field without suggestions, learning or clipboard; the host may be pasted | BRIDGE-CODE-001, BRIDGE-CODE-002, SEAL-CODE-002, stage: desktop |
| §5.3 step 3 slots B and C | N posts slot B with its keys, epk_N and name; E reveals slot C, which N checks against A | SEAL-PAIR-004, SEAL-PAIR-005, RELAY-PAIR-002 |
| §5.3 step 4 SAS | T and the 6-digit SAS are computed as stated | SEAL-PAIR-006 |
| §5.3 step 4 copy-and-enter | The user types E's digits into N, N compares, and the user confirms on E that N accepted | SEAL-PAIR-007, SEAL-PAIR-008 |
| §5.3 step 5 bundle | Slot D, sealed under the bundle key, carries board id, relay, current and previous roots, device set, head, a fresh snapshot with its hash and the recovery public keys | SEAL-PAIR-009 |
| §5.3 step 5 DeviceAdded | E appends DeviceAdded naming exactly the keys in T and registers N's key in the same call | SEAL-PAIR-010, SEAL-SIG-005, RELAY-REG-001, ENG-OPS-001 |
| §5.3 step 6 join | N reads its device_no from the sequenced entry, logs in, loads the snapshot and tail and verifies the chain to the head | SEAL-PAIR-011, SEAL-PAIR-012, ENG-BOOT-003 |
| §5.3 step 6 banner | Every other device shows a new-device banner with the key, until acknowledged | ENG-BANNER-001, ENG-BANNER-002 |
| §5.3 no QR | v1 has no QR code, camera library or deep link | BRIDGE-PERM-001, BRIDGE-DEP-001, BRIDGE-MAN-002, DEV-GEN-011 |
| §5.4 revoke | A member after device authentication, or the recovery key, revokes with a signed admin revoke that drops the key and read tokens at once | RELAY-REVOKE-001, RELAY-REVOKE-003, ENG-OPS-002, DEV-FACADE-003 |
| §5.4 revoke entry | One control entry carries DeviceRevoked and EpochRotated{e+1} with wraps for the remaining devices and recovery key, the commitment and the head | ENG-SEALER-007, SEAL-CTL-001, SEAL-ENV-004, ENG-OPS-009 |
| §5.4 races | The first EpochRotated for e+1 in log order wins; later ones are refused and their keys discarded | SEAL-MEM-015, SEAL-MEM-016, ENG-CTL-002 |
| §5.4 commitment check | Each device checks its unwrapped root against c_{e+1} and halts on a mismatch | SEAL-RCPT-007, SEAL-WRAP-003, SEAL-KEY-013 |
| §5.4 revoked entries | A revoked device's later entries are refused | SEAL-MEM-007, SEAL-KIND-001 |
| §5.4 stale epoch | An epoch-e entry after the rotation is accepted only from a current member, based before the rotation, within 30 days | SEAL-MEM-018, SEAL-ENV-005 |
| §5.4 re-seal | Authors re-seal refused entries with all their later ones, in order | ENG-RESEAL-001, ENG-RESEAL-002, ENG-EPOCH-001 |
| §5.4 replace device | Suspected compromise means Replace device: revoke, pair again as a new device, rotate | ENG-OPS-002, stage: desktop |
| §5.4 reappearing device | A revoked device that reappears offers "re-pair and resubmit my unsent edits" | ENG-EPOCH-002, ENG-EPOCH-003, DEV-PAIR-005, ENG-SESS-007 |
| §5.4 rotation triggers | Rotate on revocation | ENG-OPS-002 |
| §5.4 reset trust | Reset trust starts a new board from a trusted device's state | stage: desktop, plan §2: Administration from the phone |
| §5.4 reset trust | Reset trust starts a new board from a trusted device's confirmed state and points the old board at it with one `Fence` | DEV-CREATE-006, ENG-OPS-010 |
| §5.4 rotation | Keys rotate on a revocation and on a new recovery key, never on a schedule | ENG-OPS-011 |
| §5.5 sheet seed | The sheet holds the seed as 52 Crockford base32 characters plus 4 check characters | SEAL-SHEET-001, SEAL-KEY-004 |
| §5.5 sheet address | The sheet also holds the relay host and the board id | DEV-BKP-011 |
| §5.5 sheet signing fingerprint | The sheet holds the release-signing key's fingerprint | stage: release |
| §5.5 printing | The sheet is printed twice from memory through the OS print dialog, never through a file, or copied by hand | DEV-SHEET-001, stage: desktop |
| §5.5 setup check | Setup finishes only after the whole seed is retyped from paper and a test backup opens with it | DEV-CREATE-002 |
| §5.5 store apart | The owner stores the two copies apart | stage: recovery |
| §5.5 no token | The recovery key has no relay token and no enrolment | SEAL-MEM-026, RELAY-SEQ-009 |
| §5.5 wiped | The seed is typed into a trusted desktop or taskdeck-recover and wiped after use | DEV-SHEET-001, SEAL-SHEET-003 |
| §5.5 powers | The recovery key signs control entries: revoke, reinstate one device, rotate, Skip, Fence, change the recovery key | SEAL-MEM-026, RELAY-REVOKE-004, DEV-BKP-014, DEV-BKP-015 |
| §5.5 new recovery key | A new recovery key needs the current sheet | SEAL-MEM-011 |
| §5.5 lost sheet | If the sheet is lost, one member proposes, every device shows a banner for 72 hours, a different member confirms; any member or the old key can cancel | SEAL-MEM-023, SEAL-MEM-024, ENG-WINDOW-002, ENG-BANNER-002 |
| §5.5 pinned fingerprint | Backup writers pin the recovery fingerprint confirmed on their own screens | DEV-BKP-007, SEAL-BKP-006 |
| §5.5 recover tool start | Total loss: run taskdeck-recover from the offline USB, checked against the sheet's fingerprint | stage: recovery |
| §5.5 recover inputs | The tool takes host and board id from the sheet | SEAL-BKP-004, DEV-BKP-011 |
| §5.5 recover verify | The tool verifies each backup's signature and membership chain back to the self-certifying board id | SEAL-BKP-003, SEAL-BKP-008, SEAL-KEY-017, SEAL-BKP-005 |
| §5.5 recover list | The tool lists verified backups by writer and seq | stage: recovery |
| §5.5 recover top-up | The tool decrypts the chosen backup and tops it up through its read token | DEV-BKP-012, DEV-BKP-013, RELAY-RTOKEN-003 |
| §5.5 recover write and fence | The tool writes a taskdeck_data folder and fences the old board | stage: recovery |
| §5.6 Windows keys | On Windows L_d is DPAPI-wrapped (user scope) in %LOCALAPPDATA%\TaskDeck, not in keyring | DEV-KEY-003 |
| §5.6 sealed store | Data on every OS is a sealed store | SEAL-KEY-014, DEV-STORE-002 |
| §5.6 disk encryption | Desktops also run BitLocker or Device Encryption (Windows) and FileVault (macOS) | stage: release |
| §5.6 macOS keys | On macOS L_d is a login-Keychain item with an ACL tied to the signing identity | DEV-KEY-004 |
| §5.6 Android keys | On Android a Keystore AES-256-GCM key (StrongBox if present), not bound to user authentication | BRIDGE-KEY-004, BRIDGE-KEY-001 |
| §5.6 Android backup exclusion | Android data in app-private storage, with backup and every transfer excluded | BRIDGE-MAN-001 |
| §5.6 DOM storage | DOM storage is off; the page caches nothing | BRIDGE-PREF-002, BRIDGE-WV-003 |
| §5.6 device-protected storage | Reminder times and pending actions live in device-protected storage | BRIDGE-REM-005, BRIDGE-REM-006, BRIDGE-REM-007 |
| §5.6 key lost | A device whose key cannot be unwrapped after a reset is replaced, risking only its unsent outbox | DEV-KEY-006, DEV-KEY-002 |
| §5.6 capture exclusion | Code and seed screens exclude capture | DEV-PAIR-001 |
| §5.6 notifications | Notifications are VISIBILITY_PRIVATE | BRIDGE-REM-004 |
| §5.6 OS disk encryption | The owner turns on BitLocker or Device Encryption on Windows and FileVault on macOS | stage: release |
| §5.6 Android Keystore key | The Keystore key is generated without `setUnlockedDeviceRequired`, so background sync and reminders can open the store while the phone is locked | BRIDGE-KEY-006 |
| §5.6 capture exclusion | The Android screens that show a pairing code, the SAS or a seed set `FLAG_SECURE` | BRIDGE-SCREEN-003 |
| §6.1 provider | Host the relay on Cloudflare Workers Free as one Worker under a random workers.dev name | stage: relay, stage: release |
| §6.1 provider | One SQLite Durable Object per board, created from home | stage: relay |
| §6.1 provider | The object uses only the hibernation API, with no timers, long-polls or alarms | RELAY-SHAPE-007, RELAY-SHAPE-008, RELAY-SOCK-010 |
| §6.1 provider | Clients send a constant, descriptive User-Agent | DEV-NET-004 |
| §6.1 account | The Cloudflare account has a dedicated email address and a passkey | stage: release |
| §6.1 deploy tokens | Deploy tokens are scoped to Workers Scripts:Edit, IP-filtered, valid at most a day and never kept in CI | stage: relay, stage: release |
| §6.1 pairing | The relay host may be pasted during pairing | BRIDGE-CODE-002, RELAY-ADDR-002 |
| §6.1 rename | A relay host rename reuses the fenced re-home | ENG-MOVE-001, ENG-MOVE-003 |
| §6.2 typical day | A typical day stays under 2% of every Free-plan limit | RELAY-ROWS-002 |
| §6.2 M1 forwarding | M1 forwarding adds about 750 requests a day | plan §2: Milestones |
| §6.2 CPU | The CPU cost of an append against Free's 10 ms cap is measured on the real relay | stage: release |
| §6.2 guards | Full-jitter backoff from 1 s to 60 s | DEV-BACKOFF-001, DEV-BACKOFF-002 |
| §6.2 guards | A daily budget of 5,000 relay requests per client | DEV-BUDGET-001, DEV-BUDGET-002 |
| §6.2 guards | A daily budget of 3,000 appends per device | RELAY-BUDGET-001, RELAY-BUDGET-002, ENG-RESULT-002 |
| §6.2 guards | Tests assert the rows each operation writes and reads | RELAY-ROWS-001 |
| §6.2 guards | Quota errors are shown as "sync paused until 00:00 UTC" | DEV-NET-011, ENG-RESULT-002, ENG-UI-001 |
| §6.2 import budget | A 5,000-command import stays under 20% of every daily limit of the free tier, checked by the conformance suite | RELAY-ROWS-003 |
| §6.3 EUR 0 | Use only Workers Free, which fails closed over a limit and never bills | plan §2: Cost |
| §6.3 services | R2, KV and D1 are not used | RELAY-SHAPE-003, RELAY-STORE-001 |
| §6.3 services | FCM is not used | BRIDGE-DEP-001, BRIDGE-PERM-001 |
| §6.3 backups | Backups fit free OneDrive or iCloud space, about 0.5 GB per destination | DEV-BKP-008, plan §2: Cost |
| §6.3 spending | The only optional spending is the domain for the WAF gate and a signing token | plan §2: Cost |
| §6.4 relay outage | During a relay or Cloudflare outage the device keeps full local use and queues edits | ENG-ENQ-007, DEV-NET-015 |
| §6.4 relay outage | The status shows "relay unreachable since <time> · <n> waiting" | ENG-UI-001, stage: desktop, stage: android |
| §6.4 relay outage | Sync resumes by itself after an outage | DEV-BACKOFF-001, DEV-NET-015 |
| §6.4 quota exhausted | Over quota, sync pauses until 00:00 UTC | ENG-RESULT-002, DEV-BUDGET-001, DEV-NET-011 |
| §6.4 quota exhausted | On repeated exhaustion, move to a new Worker name, or to the domain and WAF rule | ENG-MOVE-001, stage: release |
| §6.4 bad deploy | The conformance suite gates relay deploys, and a bad deploy is rolled back | RELAY-TEST-001, stage: release |
| §6.4 relay misbehaves | A relay that loses, rolls back, forks or withholds data raises an alarm with evidence | ENG-EVID-001, ENG-SESS-002, ENG-ECHO-001 |
| §6.4 relay misbehaves | Repair by restoring the tail or by re-homing | ENG-EVID-004, ENG-EVID-005, ENG-MOVE-003 |
| §6.4 account suspended | On account suspension or a Free-plan change, re-home in 1 to 2 hours with nothing lost | ENG-MOVE-003, stage: release |
| §6.4 determinism bug | On divergence the minority halts and adopts a state chosen by fingerprint | ENG-DIVERGE-001, ENG-DIVERGE-002, ENG-DIVERGE-003 |
| §6.4 crashing entry | An entry that crashes apply waits for an update or a Skip | ENG-POISON-003, ENG-HALT-003, ENG-HALT-004 |
| §6.4 device lost | A lost device is revoked and the epoch rotated; its unsent outbox is lost | ENG-OPS-002, ENG-OPS-009, stage: release |
| §6.4 ICS host or Open-Meteo down | The device keeps the last overlay and weather and publishes their age for display | DEV-ICS-011, DEV-WX-003 |
| §6.5 rules upgrades | A phone awaiting an update keeps syncing because rules upgrades wait for every active member | SEAL-MEM-019, SEAL-MEM-020, ENG-RULES-001 |
| §6.5 single sequencer | No hot second sequencer: only one relay sequences the board at a time | RELAY-FENCE-002, RELAY-OBJ-002, ENG-MOVE-004 |
| §6.5 redundancy | Redundancy comes from three replicas with 400-day logs, backups and a rehearsed re-home | ENG-STATE-004, RELAY-COMPACT-003, stage: release |
| §6.5 M1 | During M1, applying edits needs the Windows desktop running | plan §2: Milestones |
| §6.6 nightly | Nightly, or at the first chance after being off, the Windows desktop writes a backup | DEV-BKP-001, DEV-BKP-002, plan §2: Always-on device |
| §6.6 nightly | The backup file is named TaskDeck-<date>-<seq>.tdbk | DEV-BKP-002 |
| §6.6 nightly | A backup holds canonical BoardData, sync metadata, the device set, epoch roots and a backup-read token | SEAL-BKP-002, DEV-BKP-006 |
| §6.6 nightly | A backup is deflated, signed and HPKE-sealed to the recovery key | SEAL-BKP-001, SEAL-SIG-006, SEAL-BKP-006 |
| §6.6 nightly | The writer cannot read its own backup back | SEAL-BKP-007 |
| §6.6 nightly | The Mac writes backups the same way | DEV-BKP-002 |
| §6.6 monthly | Monthly, each writer adds a signed segment of that month's frames | DEV-BKP-002, DEV-BKP-005 |
| §6.6 where | The OneDrive or iCloud Drive folder and a second disk keep 30 nightly files, 12 monthly ones and 13 segments | DEV-BKP-008 |
| §6.6 where | An offline USB stick gets a monthly copy | DEV-BKP-008, stage: release |
| §6.6 where | Each written file is re-read and checked | DEV-BKP-003 |
| §6.6 where | Settings shows the last good backup write | stage: desktop |
| §6.6 where | A failed backup write raises an alarm | DEV-BKP-010 |
| §6.6 drills | CI restores a synthetic board from a synthetic sheet on every release | stage: recovery, stage: release |
| §6.6 drills | Twice a year the owner restores the real board on another account and compares counts and the latest edits | stage: release |
| §6.6 restore to a point | Restore replays from the nearest base to the chosen seq and appends ResetTo{seq, state hash} | ENG-RESET-002, ENG-RESET-003, ENG-CKPT-003 |
| §6.6 restore to a point | Every device recomputes a ResetTo by local replay | ENG-RESET-002 |
| §6.6 restore to a point | Only seqs that every active device still holds are allowed as targets | ENG-RESET-001 |
| §6.7 protocol | The relay protocol /v1 has eight operation families and a conformance suite | RELAY-SHAPE-009, RELAY-SHAPE-010, RELAY-TEST-001 |
| §6.7 portability | relay-core.js runs unchanged on another Cloudflare account, on Workers Paid or on Node 22 with node:sqlite | RELAY-SHAPE-001, RELAY-SHAPE-011 |
| §6.7 portability | The VPS standby runs behind Caddy for TLS and request buffering | stage: relay |
| §6.7 portability | Clients poll the standby over HTTPS | RELAY-STBY-001, RELAY-CAPS-002, DEV-NET-008 |
| §6.7 re-home | The re-home is fenced and never followed automatically | ENG-MOVE-001, ENG-MOVE-004 |
| §6.7 re-home | Deploy the standby before moving | stage: release |
| §6.7 re-home | On a trusted device, "Move board" appends a signed Fence{new relay, g+1} | RELAY-FENCE-001, ENG-MOVE-005, ENG-MOVE-006 |
| §6.7 re-home | The old object is fenced to reads if it still answers | RELAY-FENCE-001, RELAY-FENCE-002 |
| §6.7 re-home | The new relay is seeded with the device set, the newest acknowledged snapshot and the tail with other devices' frames, every signature checked | ENG-MOVE-003, ENG-MOVE-007, RELAY-SEED-002, RELAY-SEED-003 |
| §6.7 re-home | Each other device moves only after the user types the 8-character move code shown on the initiating device | ENG-MOVE-001, ENG-MOVE-002, SEAL-FP-003, SEAL-FP-004 |
| §6.7 re-home | A moved device logs in with its key, checks the new chain against its own log and resubmits its outbox | ENG-EVID-005, ENG-SESS-002, ENG-SEND-007 |
| §6.7 re-home | CI rehearses the re-home on every release | stage: release |
| §6.7 re-home | The owner rehearses the re-home yearly | stage: release |
| §6.7 re-home listing | The moving device reports what it did not carry over to the new relay | ENG-MOVE-009 |
| §7 row 1 updates | No updater | stage: release |
| §7 row 1 updates | Release tags are signed after a diff review | stage: release |
| §7 row 1 updates | Release builds are cache-free and secret-free | stage: release |
| §7 row 1 updates | Artifacts pass strict attestation checks | stage: release |
| §7 row 1 updates | Release artifacts are rebuilt reproducibly | stage: release |
| §7 row 1 updates | The APK is signed offline, so Android accepts updates only from the same certificate | stage: release |
| §7 row 2 dependencies | Builds and tests use --locked | stage: foundation |
| §7 row 2 dependencies | cargo-deny and cargo-audit run on lockfile changes | stage: foundation |
| §7 row 2 dependencies | A 7-day dependency cooldown | stage: foundation |
| §7 row 2 dependencies | Tool installs are locked | stage: foundation |
| §7 row 2 dependencies | Gradle dependency verification | BRIDGE-DEP-001 |
| §7 row 2 dependencies | No npm on personal machines and no compiles where TaskDeck keys live | plan §2: Development machine |
| §7 row 2 dependencies | AI-agent PRs are reviewed like any other | stage: foundation |
| §7 row 3 relay | The relay serves no code | RELAY-SHAPE-004, BRIDGE-ASSET-001 |
| §7 row 3 relay | Clients run the receipt checks of §4.2 on every frame | SEAL-RCPT-001, SEAL-RCPT-004 |
| §7 row 3 relay | A bad frame is refused instead of halting | SEAL-RCPT-005, ENG-HALT-001 |
| §7 row 3 relay | An entry can be removed by a Skip | SEAL-RCPT-009, ENG-SKIP-002 |
| §7 row 3 relay | Decoders are fuzzed | SEAL-GEN-011, CORE-GEN-007, CORE-GEN-008 |
| §7 row 3 relay | Code that processes replicated data runs under catch_unwind | ENG-GEN-012, DEV-GEN-009, BRIDGE-GEN-003 |
| §7 row 3 relay | Synced pictures decode as JPEG only, in Rust under image::Limits | DEV-PIC-001 |
| §7 row 3 relay | Synced pictures are re-encoded before the WebView sees them | DEV-PIC-002, BRIDGE-ASSET-004 |
| §7 row 3 relay | The picture upload path no longer guesses formats | DEV-PIC-003 |
| §7 row 4 ICS | Feeds are fetched over https only | DEV-NET-001 |
| §7 row 4 ICS | At most 3 redirects, each followed by hand and re-checked | DEV-ICS-001 |
| §7 row 4 ICS | A fetch ends within 20 s and reads at most 8 MiB | DEV-ICS-004 |
| §7 row 4 ICS | The feed parser is bounded | CORE-GEN-009, DEV-GEN-006 |
| §7 row 4 ICS | No Referer, cookie or part of a previous URL is sent | DEV-ICS-002, DEV-NET-003 |
| §7 row 4 ICS | A pinning resolver refuses IP literals and private, loopback, link-local, CGNAT, ULA, IPv4-mapped, NAT64, 6to4, 0.0.0.0/8, multicast and broadcast addresses | DEV-ICS-003 |
| §7 row 4 ICS | No ATTACH or URL fetches from a feed body | DEV-ICS-007 |
| §7 row 4 ICS | Fetch errors do not carry the URL | DEV-ICS-006, DEV-GEN-008 |
| §7 row 4 ICS | A feed's own name is adopted once, by the device that added it | DEV-ICS-008 |
| §7 row 5 WebView | Bundled assets only, everything else answered in-process or with 404 or 403 | BRIDGE-ASSET-001, BRIDGE-ASSET-002 |
| §7 row 5 WebView | setBlockNetworkLoads(true), no file access and no release debugging | BRIDGE-WV-003, BRIDGE-WV-004 |
| §7 row 5 WebView | No navigation away from the page | BRIDGE-WV-005 |
| §7 row 5 WebView | Today's CSP with a script hash | BRIDGE-CSP-001, BRIDGE-CSP-002 |
| §7 row 5 WebView | connect-src limited to natively answered origins, and frame-src 'none' | BRIDGE-CSP-001 |
| §7 row 5 WebView | One Trusted Types policy | BRIDGE-TT-001 |
| §7 row 5 WebView | A document-start script makes frames inert, removes every RTC* constructor and limits link rel values | BRIDGE-DOC-001, BRIDGE-DOC-002 |
| §7 row 5 WebView | The bridge offers only the page's 17 board verbs and read routes, never subscription, device, key or relay operations | BRIDGE-VERB-001, BRIDGE-ROUTE-001, BRIDGE-ROUTE-003, BRIDGE-ROUTE-008 |
| §7 row 5 WebView | A Phase 3 exfiltration harness | BRIDGE-HARN-001, BRIDGE-HARN-002 |
| §7 row 5 WebView | Split into a UI app without INTERNET plus a network app if a channel stays open | stage: android-shell |
| §7 row 6 pairing | The pairing code is a ticket, not a key | SEAL-PAIR-001, SEAL-PAIR-015 |
| §7 row 6 pairing | Pairing slots are MAC'd | SEAL-PAIR-002, SEAL-PAIR-003 |
| §7 row 6 pairing | An explicit transcript binds the pairing | SEAL-PAIR-006, SEAL-PAIR-010 |
| §7 row 6 pairing | Codes and digits are copied from one screen and entered on the other | SEAL-CODE-002, SEAL-PAIR-007, SEAL-PAIR-008 |
| §7 row 6 pairing | The pairing code shows only in a capture-excluded window | DEV-PAIR-001, plan §2: Pairing window |
| §7 row 6 pairing | Device authentication before pairing | DEV-FACADE-003, BRIDGE-AUTH-001 |
| §7 row 6 pairing | Key fingerprints in banners | ENG-BANNER-001, SEAL-KEY-016 |
| §7 row 6 pairing | No QR codes, camera or deep links | BRIDGE-PERM-001, BRIDGE-MAN-002 |
| §7 row 7 push | No push in the plan of record | BRIDGE-DEP-001, BRIDGE-PERM-001 |
| §7 row 8 compromised device | Persistent banners with the author's key fingerprint for every control entry | ENG-BANNER-001, ENG-BANNER-002 |
| §7 row 8 compromised device | Snapshots and ResetTo are checked against honest replay | ENG-BOOT-002, ENG-SNAP-005, ENG-RESET-002 |
| §7 row 8 compromised device | The recovery key overrules membership changes | SEAL-MEM-026 |
| §7 row 8 compromised device | "Replace device" | ENG-OPS-002, ENG-OPS-009 |
| §7 row 8 compromised device | BIOMETRIC_STRONG for membership changes on Android | BRIDGE-AUTH-001, plan §2: Administration from the phone |
| §7 row 8 compromised device | State-changing notification actions carry setAuthenticationRequired(true), API 31 and later | BRIDGE-REM-003 |
| §7 row 9 relay theft | End-to-end encryption of entries and blobs | SEAL-ENV-002, SEAL-BLOB-001, RELAY-BLOB-004 |
| §7 row 9 relay theft | Plaintexts are padded | SEAL-PAD-001 |
| §7 row 9 relay theft | Kind and based_on travel inside the ciphertext | SEAL-PLAIN-001 |
| §7 row 9 relay theft | Random ids | SEAL-PLAIN-002, SEAL-BLOB-005, SEAL-KEY-008 |
| §7 row 9 relay theft | A constant User-Agent | DEV-NET-004 |
| §7 row 9 relay theft | The relay keeps no logs | RELAY-SHAPE-005 |
| §7 row 10 network | TLS verified from the OS trust store everywhere | DEV-NET-002, BRIDGE-TRUST-001 |
| §7 row 10 network | http:// is refused | DEV-NET-001, DEV-TRUST-004 |
| §7 row 10 network | Tokens travel only in headers | DEV-NET-005 |
| §7 row 11 backups | Backups are HPKE-sealed to the recovery key | SEAL-BKP-001, SEAL-BKP-006 |
| §7 row 11 backups | Backups carry writer signatures | SEAL-SIG-006, SEAL-BKP-003 |
| §7 row 11 backups | Per-writer backup-read tokens | RELAY-RTOKEN-001, DEV-BKP-006 |
| §7 row 11 backups | The recovery sheet is stored apart from the backups | stage: release |
| §7 row 12 tampering | Signatures and AEAD on every frame | SEAL-FRAME-002, SEAL-ENV-002 |
| §7 row 12 tampering | Clients check counters and seqs | SEAL-RCPT-001, SEAL-MEM-008, ENG-RECON-004 |
| §7 row 12 tampering | The hash chain | SEAL-CHAIN-001, ENG-RECV-003 |
| §7 row 12 tampering | based_on heads | SEAL-RCPT-011, SEAL-RCPT-012 |
| §7 row 12 tampering | Echo checks | ENG-ECHO-001 |
| §7 row 12 tampering | Beats | ENG-BEAT-001, ENG-BEAT-002 |
| §7 row 12 tampering | Restore tail | ENG-EVID-004, RELAY-RESTORE-001 |
| §7 row 12 tampering | A fingerprint (seq plus four words) in Settings | SEAL-FP-001, ENG-UI-001 |
| §7 row 13 account takeover | Passkeys or hardware keys and a dedicated email on every account | stage: release |
| §7 row 13 account takeover | Short-lived scoped tokens | stage: relay, stage: release |
| §7 row 13 account takeover | main is protected | stage: foundation |
| §7 row 13 account takeover | Not distributed on Play | stage: release |
| §7 row 13 account takeover | App data excluded from backup and transfer | BRIDGE-MAN-001 |
| §7 row 13 account takeover | Samsung Remote unlock off | stage: release |
| §7 row 14 DoS | An HMAC gate before any object call | RELAY-TOKEN-002, RELAY-DOOR-002, RELAY-GATE-004 |
| §7 row 14 DoS | The relay hostname is kept out of the repository | stage: relay, stage: release |
| §7 row 14 DoS | No HTML or CORS | RELAY-SHAPE-004 |
| §7 row 14 DoS | Request and append budgets | RELAY-BUDGET-001, DEV-BUDGET-001 |
| §7 row 14 DoS | Quota-aware clients | DEV-NET-011, ENG-RESULT-002 |
| §7 row 14 DoS | Optionally a domain with workers_dev = false and a WAF rule requiring a secret path prefix | stage: release, plan §2: Cost |
| §7 row 15 loss | Three replicas with 400-day logs | ENG-COMPACT-002, RELAY-COMPACT-003 |
| §7 row 15 loss | Nightly backups in two places plus offline copies | DEV-BKP-002, DEV-BKP-008 |
| §7 row 15 loss | Two fully retyped recovery sheets | DEV-CREATE-002 |
| §7 row 15 loss | Restore drills | stage: recovery, stage: release |
| §7 row 15 loss | The signing key kept in two copies | stage: release |
| §7 row 15 loss | A rehearsed re-home | stage: release |
| §7 residual 1 | A compromised device's damage is limited by banners, snapshot checks, the recovery override and restore-to-seq | ENG-BANNER-001, ENG-SNAP-005, SEAL-MEM-026, ENG-RESET-002 |
| §7 residual 6 | Until M2, edits apply only while the hub runs and local copies stay plaintext behind disk encryption | plan §2: Milestones |
| §7 row 2 dependencies, cargo-vet | cargo-vet coverage of the seal and core crates | stage: backlog |
| §7 row 7 push | Push ships only if its gate passes (Phase 9) | stage: backlog |
| §8 development isolation | No cargo, rust-analyzer, Gradle, Android Studio or npm runs in an OS account that holds TaskDeck keys | plan §2: Development machine |
| §8 development isolation | Develop in cloud sessions, a VM or a separate standard account, against a development board | plan §2: Development machine |
| §8 development isolation | Daily accounts are standard users, and administration uses a separate account | stage: release |
| §8 source | main is protected, and every change, AI-agent branches included, arrives through a reviewed PR | stage: foundation |
| §8 source | One dependency change per PR | stage: foundation |
| §8 source | Before git tag -s, review the diff since the last signed tag locally, starting with Cargo.lock, build.rs files and .github | stage: release |
| §8 source | rust-toolchain.toml pins the compiler | stage: foundation |
| §8 source | build.rs takes its date from SOURCE_DATE_EPOCH instead of Utc::now() | stage: foundation |
| §8 CI | CI runs on ephemeral GitHub runners without secrets, with contents: read and SHA-pinned actions | stage: foundation |
| §8 CI | Every push runs the Linux suite: locked tests, two-TZ determinism, golden logs and hashes, deny, audit, vet, node --test, a fuzz smoke run and the unsigned APK | stage: foundation, CORE-CTX-002, CORE-RULES-003 |
| §8 CI | Windows and macOS golden logs run nightly and on tags | stage: release, plan §2: Development machine |
| §8 CI | The release workflow runs only on tag push and neither restores nor saves any cache | stage: release |
| §8 CI | No test job writes a cache a release could read | stage: foundation, stage: release |
| §8 CI | Every artifact is built with actions/attest-build-provenance, with immutable releases | stage: release |
| §8 CI | Tools come from cargo install --locked --version or from verified binaries | stage: foundation |
| §8 verify | Verify every artifact with gh attestation verify and the signer-workflow, source-ref, source-digest and deny-self-hosted-runners flags | stage: release |
| §8 verify | libtaskdeck.so and taskdeck-recover are built reproducibly with a pinned container, --remap-path-prefix and SOURCE_DATE_EPOCH | stage: android, stage: recovery, stage: release |
| §8 verify | One of them is rebuilt locally per release and compared by hash | stage: release |
| §8 Windows | Verify, then unzip to %LOCALAPPDATA%\Programs\TaskDeck, never into OneDrive | stage: release |
| §8 macOS | scripts/mac-bundle.sh signs with the self-signed "TaskDeck Local" identity, kept in its own keychain, locked outside releases and never granted "Always Allow" | stage: release |
| §8 macOS | Clear the quarantine attribute and move the app to ~/Applications | stage: release |
| §8 Android | AGP 9.5, NDK r28 with 16 KB pages, compileSdk 37, targetSdk 36 and cargo-ndk 4.1.2 | BRIDGE-PLAT-001 |
| §8 Android | minSdk 29, raised to 31 or 33 once the phone is known | plan §2: Android floor |
| §8 Android | The cdylib uses panic = "unwind" with catch_unwind at every JNI entry | BRIDGE-GEN-003, BRIDGE-JNI-007 |
| §8 Android | Gradle verification covers the whole dependency set, and the platform verifier's AAR is vendored by checksum | BRIDGE-DEP-001 |
| §8 Android release | Sign with apksigner through PKCS#11 on a touch-to-sign token, or with a key file on an encrypted USB stick as the EUR 0 fallback | stage: release, plan §2: Cost |
| §8 Android release | Check apksigner verify --print-certs against the pinned fingerprint | stage: release |
| §8 Android release | Install with adb install -r, then revoke the debugging authorisations, turn developer options off and Auto Blocker back on | stage: release |
| §8 Android | Test adb before ever enabling Advanced Protection mode | stage: release |
| §8 Android | Register the package and certificate on a free limited-distribution account before January 2027 | stage: release |
| §8 Android | Not on Play | stage: release |
| §8 relay | relay/deploy.sh checks the tag signature and uploads through the Script Upload API with main_module, the DO binding and a new_sqlite_classes migration | stage: relay |
| §8 relay | The upload runs from a throwaway container with the 1-day token | stage: relay, stage: release |
| §8 relay | Preview URLs are off | stage: relay |
| §8 supply chain | Phase 0 refreshes the lockfile to rustls 0.23.46, rustls-webpki 0.103.17, webpki-roots 1.0.12 or later, and tokio past RUSTSEC-2026-0386 | plan §2: Phase 0 hygiene, stage: foundation |
| §8 supply chain | deny.toml allows crates.io only, bans duplicate crypto crates and carries a reviewed ignore for unmaintained chrono | stage: foundation |
| §8 supply chain | cargo-cooldown enforces the cooldown | stage: foundation |
| §8 supply chain | reqwest is replaced with ureq 3, dropping tokio and hyper | DEV-GEN-011 |
| §8 supply chain | Dependencies are updated monthly, or within a week of a relevant advisory | stage: release |
| §8 supply chain | A renewal register tracks the signing keys, the Cloudflare account, the sheets and the drill dates | stage: release |
| §8 CI, vet | The vet step of the Linux suite | stage: backlog |
| §9 step 1 drain | Drain the phone page and client desktops, then quit TaskDeck and any taskdeck-server | plan §2: Migration source |
| §9 step 2 back up | Back up every taskdeck_data folder as a dated zip onto two disks, never to be deleted | stage: release |
| §9 step 3 choose | Choose one board, the server's if in use or else the Windows desktop's; folders are never merged by copying | plan §2: Migration source |
| §9 step 4 dry run | TaskDeck --import-check refuses a folder whose .lock is held | DEV-LOCK-004 |
| §9 step 4 dry run | The dry run reports counts and any id that is both live and archived | CORE-IMP-001, CORE-IMP-003 |
| §9 step 4 dry run | The dry run renumbers duplicate subscription ids and folds ids at or above 2^62 | CORE-IMP-005 |
| §9 step 4 dry run | The dry run checks that every id is below 2^40 | CORE-IMP-005 |
| §9 step 4 dry run | The dry run checks that every instant survives conversion to UTC | CORE-IMP-001, CORE-TIME-001 |
| §9 step 4 dry run | The dry run prints a canonical hash that must repeat | CORE-IMP-002 |
| §9 step 5 M1 | Create the board on the Windows desktop with keys, relay and sheet | DEV-CREATE-001, DEV-CREATE-002, DEV-CREATE-003, plan §2: Milestones |
| §9 step 5 M1 | Pair the Mac, whose first-contact logic sets its own board aside, and the phone, and compare counts | DEV-PAIR-004, stage: release |
| §9 step 5 M1 | Press New key to retire every old token link | stage: desktop, stage: release |
| §9 step 5 M1 | Turn off the phone view and the feed, remove the Google Calendar subscription, remove the old web app and uninstall Tailscale | plan §2: `.ics` feed and exports, stage: release |
| §9 step 6 M2 | Every M1 queue must be empty and acknowledged, and M2 builds refuse to convert or bootstrap otherwise | plan §2: Migration source, plan §2: Milestones |
| §9 step 6 M2 | Update all three devices | stage: release |
| §9 step 6 M2 | On first start the M2 phone replays the old page's localStorage queue through the compile step | plan §2: Migration source |
| §9 step 6 M2 | "Convert to replicated board" re-runs the import checks | DEV-LOCK-004, CORE-IMP-001 |
| §9 step 6 M2 | The conversion writes the genesis, signed by the recovery key | SEAL-MEM-004, DEV-CREATE-005 |
| §9 step 6 M2 | The genesis takes the palette and home place from userconfig.toml | CORE-IMP-007 |
| §9 step 6 M2 | The conversion moves the legacy files untouched to pre-sync-<date>/ | DEV-CREATE-004 |
| §9 step 6 M2 | The conversion deletes the legacy phone token | stage: desktop |
| §9 step 6 M2 | The Mac and the phone bootstrap from the genesis snapshot | DEV-CREATE-005, SEAL-PAIR-011, DEV-PAIR-006 |
| §9 step 6 M2 | Compare fingerprints and counts, and spot-check a few items | stage: release |
| §9 step 7 watch | Watch the legacy folder for 30 days and offer "import changes" as ordinary commands | plan §2: Migration |
| §9 step 8 roll back | An old build opens pre-sync-<date>/ | DEV-CREATE-004 |
| §9 step 8 roll back | "Export legacy" writes day-planner files from the current board | DEV-EXPORT-001 |
| §9 step 3, a second board | Another board's items can come in later as ordinary commands, after a preview | stage: backlog |
| §9 step 6 first board | A board created from a legacy folder takes its palette and home place from that folder's `userconfig.toml` | CORE-IMP-009, DEV-CREATE-007 |
<!-- requirement-map:end -->
