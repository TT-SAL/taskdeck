# TaskDeck sync specs: common conventions

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
| notice | A persistent per-device message that is not a refusal, such as "changed on Desk since you last synced". | `core.md`, `engine.md` |
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
<!-- requirement-map:end -->
