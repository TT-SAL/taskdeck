# Resolutions R1: seal.md against device.md (17 findings)

Resolver 8 of 9. Inputs read: reviews.md §B, spec/README.md, foundation.md "Decisions" and "Rulings for the specs" (S1 to S31, with the settled S15 and the notes under the device draft's rulings), SYNC_PLAN.md §2 and §3, the earlier files `r1-seal-relay.md`, `r1-seal-engine.md`, `r1-core-engine.md`, `r1-engine-relay.md`, `r1-device-engine.md`, `r1-device-bridge.md`, `r1-core-device.md` and the lead's `r1-lead.md` (L1 to L9) in full; seal.md §3 to §7, §11 to §14, §16, §17.7 to §17.9, §19 to §27; device.md §6 to §8, §10.1 to §10.3, §11, §12.1, §14, §15, §16, §18.1, §18.3, §18.4, §21 to §26; relay.md §5.4; engine.md §19.7 to §19.9. The lead's change file `r1-lead-changes.md`, which `r1-lead.md` names, does not exist yet.

Built on the earlier files and the lead's rulings (settled; not repeated here):
- seal-device-1: seal.md §5.4, SEAL-ENC-006 and the §23 row are R1-SEAL-RELAY-1; device.md `tokens` and `move_board` are R1-DEVICE-ENGINE-18; `relay_host`, §18.1 step 1 and the §22 host row are R1-CORE-DEVICE-7. Only §6.4.9 follows (R1-SEAL-DEVICE-1).
- seal-device-2: seal.md §19.2 and `FileKind` (kinds 1 to 19) are R1-SEAL-RELAY-10; device.md §6.1's sentence and §24's first seal bullet are R1-SEAL-RELAY-29 and R1-DEVICE-ENGINE-14 item 3; `KIND_OWN_PICTURE` is R1-DEVICE-ENGINE-14 item 17. Only the single owner of the kind numbers follows (R1-SEAL-DEVICE-2).
- seal-device-3 (S27), seal-device-5 (S29), seal-device-11 (S15), seal-device-12 (S28): applied in full by earlier files (R1-SEAL-DEVICE-3).
- seal-device-4 (S24): ruled by L1 (the wraps travel in the `reinstate` admin operation, not in `DeviceReinstated`); the device's hand-over is R1-DEVICE-ENGINE-13. Only seal's side of opening those wraps, and L1's exact `args`, follow (R1-SEAL-DEVICE-4).
- seal-device-6: R1-SEAL-RELAY-20 (relay.md `read_epoch` in the refusal, RELAY-RTOKEN-004), R1-SEAL-RELAY-27 (device.md §15.8 step 1, open question 7) and R1-DEVICE-ENGINE-15 (the §22 row at 2 tries). Only the second refusal and the `RecoveryLink` signature follow (R1-SEAL-DEVICE-5).

New clause ids assigned here (the last resolver continues after them, and after those of the earlier files):
- seal.md: SEAL-GEN-014, SEAL-BKP-009, SEAL-BKP-010.
- device.md: none. Reworded: DEV-BKP-005, DEV-LOGIN-001, DEV-TRUST-003.
- seal.md reworded: SEAL-ENC-006 (again, on top of R1-SEAL-RELAY-1), SEAL-KEY-015, SEAL-WRAP-002, SEAL-WRAP-003.

New error codes: `seal/host_format`, `seal/not_control` (seal.md §22); `device/recovery_token` (device.md §21). New public items: seal.md `relay_origin`, `BackupHeader`, `verify_backup_file`, `impl HistoryRecord`, `FixedEntropy::new`, `TEST_ENTROPY_MARKER`, `#[repr(u8)]` on `FileKind`; `AdminOp::Reinstate` gains L1's fields; `BackupInput.read_token` and `VerifiedBackup.read_token` become `Option<String>`. device.md `RecoveryLink::read_token` loses its `epoch` parameter; `read_identity_dir` returns `HistoryRecord` values; the `store::KIND_*` constants are derived from `FileKind`. Wire and file changes: a host may no longer carry `:443`; the `reinstate` args are L1's; a backup's `read_token` may be empty (test backup only).

Notation. "Replace A with B" means the exact text A, as it stands in the spec after the changes of the seven earlier files, becomes B. Clause texts are given in full. A table row is given in full, in the table's own column order. A change that needs edits in two or three specs has the same id under each heading.

## seal.md

### R1-SEAL-DEVICE-2 (seal-device-2)

The kind numbers have one owner, `FileKind` (seal.md §26); device.md's constants are derived from it (same id under device.md).

§26 Public API: directly before the line `pub enum FileKind { Log = 1, Checkpoint = 2, Base = 3, Hashes = 4, Outbox = 5, Overlay = 6, Identity = 7, Notices = 8,` (the enum as R1-SEAL-RELAY-10 wrote it) add the line `#[repr(u8)]`. The enum itself is unchanged.

### R1-SEAL-DEVICE-3 (seal-device-3, seal-device-5, seal-device-11, seal-device-12)

Nothing further to apply.
- seal-device-3 (ruling S27): R1-SEAL-RELAY-9 adds the snapshot body's `history` (§20.4), the history check of §20.5 step 7, SEAL-BLOB-006 and SEAL-BLOB-007, and the §17.8 sentence, without a bundle field; R1-SEAL-RELAY-28 rewrites device.md §15.7, removes the history blob from §18.3, writes `history.tdk` from the bootstrap snapshot, deletes §24's bundle bullet and rewrites open question 6; R1-SEAL-ENGINE-15 keeps the engine out (the sealer fills and keeps the history, lead ruling L4); R1-DEVICE-ENGINE-1 adds the rejoin path (DEV-RUN-006), which writes the snapshot's history records in the `Persist` of the rejoin's checkpoint, before any `AppendLog` above `S` (`engine.md` §19.7 step 4), so a rejoined device's `history.tdk` has no hole. §17.9 step 4 runs §20.5, which now checks the history, so step 5 needs no sentence. With every path writing the full history, `history_complete` stays as the guard of §15.1 condition 3 and needs no rewording.
- seal-device-5 (ruling S29): kind 19 and `FileKind::OwnPicture` are R1-SEAL-RELAY-10; the device's row, layout, §14.3, DEV-PIC-005, the §7.2 and §7.4 rows and the facade calls are R1-DEVICE-ENGINE-14 and R1-DEVICE-BRIDGE-1.
- seal-device-11 (ruling S15): R1-SEAL-RELAY-2 puts `t ≥ max(W.purge_seq, s − 16,384)` into §13.6, the §14.6 sentence, the §23 row "Skip window" and §27 item 12; R1-SEAL-ENGINE-1 adds "with `s − 16,384` taken as 0 when `s ≤ 16,384`" and rewords SEAL-RCPT-009. device.md maps `engine/skip_target` already.
- seal-device-12 (ruling S28): R1-SEAL-RELAY-11 replaces §21.1's name with `TaskDeck-<date>-<seq>-<tag>.tdbk` and points to `device.md` §15.3 for names and the tag. The finding's "state no name at all" is not adopted: the earlier text names the form once, with its owner.

### R1-SEAL-DEVICE-4 (seal-device-4)

Ruling S24 as the lead ruled it (L1). The finding's `roots` field on `DeviceReinstated`, its §13.6 precondition, its limit of 1,024 and `Receipt.learned_roots: Vec<…>` are not adopted: a reinstated device cannot open the reinstating entry, so L1 puts the wraps into the recovery-signed `reinstate` operation, which the relay returns at the device's login. The roots then arrive outside any entry, one epoch at a time, so `Receipt.learned_root` (the root an entry's own wraps teach) stays single. What seal must still say is how such a wrap is opened and checked, and L1's `args`. Where `r1-lead-changes.md` gives text for the same place (the §16.2 row, `AdminOp`, §27 item 18), the lead's text wins (see "For the lead", item 1).

1. §7: add this paragraph directly after the sentence `In JSON a wrap is a 192-character hex string of these 96 bytes.`, before SEAL-WRAP-001:

   **Wraps delivered outside an entry.** A device that the recovery identity reinstates (ruling S24) receives, outside the log, the wraps of the roots of the epochs it missed: the recovery identity signs them in the `reinstate` operation (§16.2), and the relay returns them at the device's login (`relay.md`; `device.md` §10.3). The device opens the wrap of epoch `e` by SEAL-WRAP-002, with `c_e` as its own membership records it, and only once its membership records `c_e`, oldest epoch first: the commitment of the first wrapped epoch is already in its membership (it classified the revoking rotation, which is sealed under an epoch it held), and each later one enters it when the device classifies, in seq order, the rotation that set that epoch, which is sealed under the epoch before it. It uses a root only after it matched `c_e` (SEAL-KEY-013).

2. Reword SEAL-WRAP-002 (full text):

   - **SEAL-WRAP-002** (seal, recovery) Unwrapping MUST call HPKE `OpenBase` with the `info` and `aad` rebuilt from the receiver's own `board_id`, the epoch named by the item, the receiver's own id and public key, and the commitment `c_e` from the same item (for a wrap delivered outside an entry, §7: the epoch it is delivered for and `c_e` as the receiver's membership records it), never from values taken from the wrap.

3. Reword SEAL-WRAP-003 (full text):

   - **SEAL-WRAP-003** (seal) A device MUST treat a failure to open its own wrap, or an opened root whose commitment differs from `c_e`, as the commitment halt of §14.5, for a wrap delivered outside an entry (§7) as for one inside a rotation.

4. §16.2, the registry table as R1-SEAL-RELAY-3 left it: replace the row that begins `| `reinstate` | `dev[16]` | 16 |` with:

   `| `reinstate` | `dev[16] ‖ u32 first_epoch ‖ u16 n ‖ n × wrap[96]` | 22 + 96n | signed by the current recovery identity, before the `R`-shape entry that reinstates `dev`: the wraps (§7) of the roots of epochs `first_epoch` to `first_epoch + n − 1`, in order, each to the target's `hpke_pk`, with `1 ≤ n ≤ 256`; `first_epoch` is the epoch the rotation in the revoking entry set up, or the epoch in force at `purge_seq` when that is later; the last wrapped epoch is the one the reinstating entry is sealed under, whose own rotation wraps to the target as to every recipient (ruling S24, lead ruling L1); the relay stores the wraps and returns them at the target's login (`relay.md`) |`

5. §23 Limits: add, directly after the row `| wraps per rotation | 2 to 17 | wraps | SEAL-CTL-003 |`:

   `| wraps per reinstatement | 1 to 256 | wraps | SEAL-SIG-004 |`

6. §25, `device.md` block: add this bullet after the bullet that begins `- Requires: every accepted control entry's history record`:

   - Requires: the sealer adapter opens, with `unwrap_root`, the wraps a `login` answer carries for a reinstated device (`device.md` §10.3), as §7 states (SEAL-WRAP-002, SEAL-WRAP-003, SEAL-KEY-013): epoch by epoch, oldest first, each once its membership records that epoch's commitment. `Receipt.learned_root` reports only a root that an entry's own wraps teach.

7. §26 Public API, `pub enum AdminOp`: replace `Reinstate(DevId),` with `Reinstate { dev: DevId, first_epoch: u32, wraps: Vec<[u8; 96]> },`.

8. §27: replace item 18 (as R1-SEAL-RELAY-8 wrote it) with:

   18. **A device the recovery key reinstates reads what was sealed while it was revoked** (ruling S24, lead ruling L1). The wraps of the roots it missed travel in the recovery-signed `reinstate` operation (§16.2), outside the reinstating entry, which the device cannot open; the relay returns them at the device's login, and the device opens each by §7 once its membership records that epoch's commitment. More than 256 missed epochs: the recovery tool does not offer reinstatement, and the owner restores to a new board. Decided there.

### R1-SEAL-DEVICE-6 (seal-device-7)

A segment's projection ends at `first_seq − 1`, so its writer must be current there; the writer chooses the start (device.md §15.2 and DEV-BKP-005, same id). No byte changes.

§21.6, step 10: replace `10. `writer_dev` is a current device of the projection with `writer_sign_pk`, and `recipient` is the projection's current recovery identity (`seal/backup_writer`).` with:

10. `writer_dev` is a current device of the projection with `writer_sign_pk`, and `recipient` is the projection's current recovery identity (`seal/backup_writer`). For a segment the projection ends at `first_seq − 1`, so its writer must be a current device at that seq; a writer chooses the start of its segments so that it is (`device.md` §15.2, DEV-BKP-005).

### R1-SEAL-DEVICE-7 (seal-device-8)

Each relay origin has exactly one host: a host never carries `:443`, so the relay origin is `https://` followed by the host, built by one seal function. Matching edits: relay.md (the order of its parse step) and device.md, same id.

1. §5.4, the second bullet as R1-SEAL-RELAY-1 left it: replace `then optionally `:` and a decimal port from 1 to 65535 without leading zeros.` with `then optionally `:` and a decimal port from 1 to 65535 without leading zeros, other than 443: the default port is never written, so that each relay origin has exactly one host.`, and replace `with the port at most 65535, the part before `:` at most 253 bytes,` with `with the port at most 65535 and not 443, the part before `:` at most 253 bytes,`.

2. §5.4, the third bullet: replace `- The relay origin of a host `h` is `https://h`, except that a port of 443 is never written.` with `- The relay origin of a host `h` is `https://` ‖ `h` (`relay_origin`, §26).`

3. Reword SEAL-ENC-006 (full text; it replaces the text of R1-SEAL-RELAY-1):

   - **SEAL-ENC-006** (seal) A host field (§12.7, §17.8) MUST match the regular expression of §5.4, with at most 253 bytes before any `:`, a port of at most 65535 that is not 443, and at most 255 bytes in all; an encoder MUST refuse to produce, and a parser MUST reject, any other value.

4. §22 Error catalogue: add this row directly after the row `| `seal/challenge_length` | … |`:

   `| `seal/host_format` | a host is not a host of §5.4 (a port of 443 included) | `relay_origin`, §5.4 | refuse the host; ask the user again |`

5. §26 Public API: in the block `// ---------- encodings (§5)`, after the closing `}` of `pub mod base32 { … }`, add:

   ```rust
   pub fn relay_origin(host: &str) -> Result<String, SealError>;   // §5.4: "https://" ‖ host; seal/host_format when host fails SEAL-ENC-006
   ```

### R1-SEAL-DEVICE-8 (seal-device-9)

A writer checks its own backup without the recovery key through a seal function (device.md §15.3.2, same id).

1. §21.6: add this paragraph directly after the paragraph that begins `The checks that need non-control entries`, before SEAL-BKP-001:

   **Checking a file one wrote.** `verify_backup_file` (§26) runs steps 1, 2 and 5 only, under the writer's own signing key, so that a writer checks the file it wrote without the recovery key (`device.md` §15.3.2). It also requires the header's `writer_sign_pk` to equal that key (`seal/backup_signature`), returns the header fields, and opens nothing.

2. §21.6: add after SEAL-BKP-008:

   - **SEAL-BKP-009** (seal) `verify_backup_file` MUST run exactly steps 1, 2 and 5 of §21.6, in that order, MUST reject with `seal/backup_signature` a file whose header's `writer_sign_pk` differs from the given key, and MUST NOT open, inflate or parse any byte of `ct`.

3. §24 Test vectors, row `seal-backup.json`: append to its Expected-outputs cell `; `verify_backup_file` results for a good file, a re-signed file, a file whose header names another `writer_sign_pk`, and a truncated one`.

4. §26 Public API, block `// ---------- backups (§21)`: after the line `pub fn read_backup(bytes: &[u8], sheet_board: &BoardId, recovery: &RecoveryKeys) -> Result<VerifiedBackup, SealError>;` add:

   ```rust
   pub struct BackupHeader { pub file_type: u8, pub board: BoardId, pub writer_dev: DevId, pub writer_sign_pk: [u8; 32],
                             pub recipient: DevId, pub seq: u64, pub written_ms: i64, pub ct_len: u64 }   // §21.2
   pub fn verify_backup_file(bytes: &[u8], writer_sign_pk: &[u8; 32]) -> Result<BackupHeader, SealError>;  // §21.6 steps 1, 2, 5; SEAL-BKP-009
   ```

### R1-SEAL-DEVICE-9 (seal-device-10)

The history record has one codec, seal's, and is built by seal from an accepted control entry, so the device never parses a `ctl` payload (device.md §6.4.8, §8.3, §25, same id).

1. §21.5: add this paragraph directly after the history-record table, before the paragraph that begins `The history holds one record for every accepted control entry`:

   A writer builds each record from an accepted `ctl` entry with `HistoryRecord::from_accepted` (§26), keeps its bytes as `to_bytes` gives them, and reads them back with `from_bytes`, which rejects bytes that are not exactly one record of this table with `seal/backup_history`.

2. §21.6: add after SEAL-BKP-009 (R1-SEAL-DEVICE-8):

   - **SEAL-BKP-010** (seal) `HistoryRecord::from_accepted` MUST return the record of §21.5 with `seq` the given seq, `dev` and `ctr` from the frame header, `entry_id` and `at_ms` from the opened plaintext, `frame_hash = H(frame)`, `items` the canonical encoding of the payload's `items` and `sig` its inner signature (§11.3), and MUST refuse with `seal/not_control` an opened entry whose kind is not `ctl`.

3. §22 Error catalogue: add this row directly after the row `| `seal/backup_unpinned` | … |`:

   `| `seal/not_control` | `HistoryRecord::from_accepted` was given an entry that is not of kind `ctl` | §21.5, SEAL-BKP-010 | a bug of the caller |`

4. §24 Test vectors, row `seal-backup.json`: append to its Expected-outputs cell (after the text R1-SEAL-DEVICE-8 appends) `; history records built by `from_accepted` from accepted `ctl` entries, their bytes, and `from_bytes` failures`.

5. §26 Public API, block `// ---------- backups (§21)`: replace `pub struct HistoryRecord { /* §21.5 */ }` with:

   ```rust
   pub struct HistoryRecord { /* §21.5 */ }
   impl HistoryRecord {
       pub fn from_accepted(seq: u64, frame: &[u8], opened: &Opened) -> Result<Self, SealError>;  // SEAL-BKP-010
       pub fn to_bytes(&self) -> Vec<u8>;                                                         // §21.5 layout
       pub fn from_bytes(bytes: &[u8]) -> Result<Self, SealError>;                                // seal/backup_history
       pub fn seq(&self) -> u64;
   }
   ```

### R1-SEAL-DEVICE-10 (seal-device-13)

Seal defines the marker its `test-entropy` feature puts into a build, so `release` can search for it (device.md §11.2, DEV-TRUST-003, same id).

1. §3: add after SEAL-GEN-013:

   - **SEAL-GEN-014** (seal) A build of the `seal` crate with the cargo feature `test-entropy` MUST contain the byte string `taskdeck-test-entropy-enabled-3b9e5d` (`TEST_ENTROPY_MARKER`, which `FixedEntropy::new` passes to `std::hint::black_box`), and a build without the feature MUST NOT.

2. §25, `device.md` block: replace the bullet `- Requires: release builds never enable the `test-entropy` feature (SEAL-GEN-005).` with:

   - Requires: release builds never enable the `test-entropy` feature (SEAL-GEN-005), which stage `release` checks by the feature resolution and by searching every artifact for `TEST_ENTROPY_MARKER` (SEAL-GEN-014; `device.md` §11.2, DEV-TRUST-003).

3. §26 Public API: replace the line `#[cfg(feature = "test-entropy")] pub struct FixedEntropy { /* replays given bytes */ }` with:

   ```rust
   #[cfg(feature = "test-entropy")] pub struct FixedEntropy { /* replays given bytes */ }
   #[cfg(feature = "test-entropy")] impl FixedEntropy { pub fn new(bytes: Vec<u8>) -> Self; }   // passes TEST_ENTROPY_MARKER to std::hint::black_box
   #[cfg(feature = "test-entropy")] pub const TEST_ENTROPY_MARKER: &str = "taskdeck-test-entropy-enabled-3b9e5d";  // SEAL-GEN-014
   ```

### R1-SEAL-DEVICE-13 (seal-device-16)

A backup's `read_token` may be empty, meaning none, so the create-time test backup (device.md §18.1 step 6), written before any token exists, needs no placeholder. Matching edits: device.md, same id.

1. §21.5, the payload table: replace the row `| `read_token` | 2 + n | vb16 | both | the writer's backup-read token (§6.7), per writer and generation (V1-2) |` with:

   `| `read_token` | 2 + n | vb16 | both | the writer's backup-read token (§6.7), per writer and generation (V1-2); `n = 0` means none, written only by the test backup of a board being created, which holds no token yet (`device.md` §18.1 step 6); a reader treats it as absent |`

2. Reword SEAL-KEY-015 (full text):

   - **SEAL-KEY-015** (seal) The `seal` crate MUST treat a relay token as an opaque ASCII string of 1 to 512 bytes, the empty `read_token` of §21.5 standing for no token, and MUST NOT derive any key or identifier from it.

3. §26 Public API: in `pub struct BackupInput<'a>`, replace `pub read_token: String,` with `pub read_token: Option<String>,` (`None` is written with length 0, §21.5); in `pub struct VerifiedBackup`, replace `pub read_token: String,` with `pub read_token: Option<String>,`.

### R1-SEAL-DEVICE-14 (seal-device-17)

`open_blob` does not read `inflated`, and a kind-2 blob is opened with the relay's author, since the board's picture reference names no writer. Matching edits: device.md §14.2 step 3, same id.

1. §20.2, the paragraph after the AD, as R1-SEAL-RELAY-12 and R1-SEAL-ENGINE-3 left its last sentence (`… except in the declared mode of §20.5, where the caller checks it afterwards against the author of the signed `snap` entry.`): append:

   A blob of kind 2 is the other exception: the board's reference (`core.md` §6.9) names no writer, so the reader takes the relay's `author` (`device.md` §14.2 step 3); the `sha256` that the signed command carries fixes the sealed bytes, so a wrong `author` only makes the open fail.

2. §20.5: add this sentence directly after step 7, before the paragraph that begins `Splitting:`:

   For a blob of kind 2, steps 6 and 7 do not apply and `expected.inflated` is not read.

3. §26 Public API: replace the line `pub fn open_blob(parts: &[&[u8]], expected: &BlobRef, writer: &DevId, key: &SymKey, board: &BoardId) -> Result<Vec<u8>, SealError>;` with:

   ```rust
   pub fn open_blob(parts: &[&[u8]], expected: &BlobRef, writer: &DevId, key: &SymKey, board: &BoardId) -> Result<Vec<u8>, SealError>;
                                                                    // kind 2; expected.inflated is not read (callers pass 0), §20.5
   ```

## relay.md

### R1-SEAL-DEVICE-7 (seal-device-8)

A host never carries `:443` (`seal.md` §5.4, SEAL-ENC-006 as R1-SEAL-DEVICE-7 rewords it), so the parse removes a typed `:443` before it checks the host.

§5.4, the parsing list, step 4: replace `4. Map `A`–`Z` in the host to `a`–`z`; the host must then match `seal.md` §5.4's host rule, and a port of 443 is removed.` with:

4. Map `A`–`Z` in the host to `a`–`z` and remove a trailing `:443`; the host must then match `seal.md` §5.4's host rule, which admits no port 443.

(The sentence `` `host` is the host form of `seal.md` §5.4 (lowercase, optional `:port`, 443 never written). The relay origin of the address is `https://` ‖ `host`. `` already agrees and is unchanged.)

## device.md

### R1-SEAL-DEVICE-1 (seal-device-1)

The hosts in the bootstrap file fit their `vb8` (`seal.md` §5.4 and SEAL-ENC-006 as R1-SEAL-RELAY-1 and R1-SEAL-DEVICE-7 leave them).

§6.4.9 (as R1-DEVICE-ENGINE-2 left it): append at the end of the paragraph:

In both forms `host` is a host of `seal.md` §5.4 (1 to 255 bytes, `:port` included, never `:443`; SEAL-ENC-006), so it always fits its `vb8`; any other value is `device/record_format`.

### R1-SEAL-DEVICE-2 (seal-device-2)

The store's kind constants are `FileKind`'s numbers, defined once in `seal.md` §19.2 and §26 (same id under seal.md). R1-DEVICE-ENGINE-14's constant `KIND_OWN_PICTURE` is kept; only its definition changes.

§25 Public API, `pub mod store`: replace the four lines

```rust
    pub const KIND_META: u8 = 9;  pub const KIND_MARKER: u8 = 10; pub const KIND_HISTORY: u8 = 11;
    pub const KIND_DEVICE: u8 = 12; pub const KIND_PICTURE: u8 = 13; pub const KIND_WEATHER: u8 = 14;
    pub const KIND_DRAFT: u8 = 15; pub const KIND_FEED: u8 = 16; pub const KIND_BOOTSTRAP: u8 = 17;
    pub const KIND_UNSENT: u8 = 18; pub const KIND_OWN_PICTURE: u8 = 19;
```

(the last as R1-DEVICE-ENGINE-14 item 17 left it) with

```rust
    // The file kinds of §6.1 are taskdeck_seal::FileKind (seal.md §19.2, §26): the store passes those variants to
    // FileSealer::create and open_file. These constants are their numbers, never defined a second time.
    use taskdeck_seal::FileKind;
    pub const KIND_META: u8 = FileKind::Meta as u8;           pub const KIND_MARKER: u8 = FileKind::Marker as u8;
    pub const KIND_HISTORY: u8 = FileKind::History as u8;     pub const KIND_DEVICE: u8 = FileKind::Device as u8;
    pub const KIND_PICTURE: u8 = FileKind::Picture as u8;     pub const KIND_WEATHER: u8 = FileKind::Weather as u8;
    pub const KIND_DRAFT: u8 = FileKind::Draft as u8;         pub const KIND_FEED: u8 = FileKind::Feed as u8;
    pub const KIND_BOOTSTRAP: u8 = FileKind::Bootstrap as u8; pub const KIND_UNSENT: u8 = FileKind::Unsent as u8;
    pub const KIND_OWN_PICTURE: u8 = FileKind::OwnPicture as u8;
```

### R1-SEAL-DEVICE-4 (seal-device-4)

The device's hand-over of the reinstatement wraps (R1-DEVICE-ENGINE-13) points to the seal rule that opens them (`seal.md` §7, SEAL-WRAP-002, SEAL-WRAP-003, same id). Where `r1-lead-changes.md` words this paragraph differently, the lead's text wins.

§10.3, the sentence R1-DEVICE-ENGINE-13 item 1 inserted: replace `which checks each root against the commitment of the rotation that set its epoch (`seal.md`),` with `which opens each wrap with `unwrap_root` once its membership records that epoch's commitment, oldest epoch first, and checks the root against it (`seal.md` §7, SEAL-WRAP-002, SEAL-KEY-013),`.

### R1-SEAL-DEVICE-5 (seal-device-6)

The recovery tool mints at most twice (epoch 1, then the refusal's `read_epoch`, R1-SEAL-RELAY-27); a second refusal now has an outcome, and `RecoveryLink::read_token` loses the epoch it no longer takes. relay.md needs nothing further (R1-SEAL-RELAY-20). (The start of the same step is changed by R1-SEAL-DEVICE-13 item 4; the two edits touch different sentences.)

1. §15.8, step 1 (as R1-SEAL-RELAY-27 wrote it): replace `It never mints above the relay's `read_epoch`, so it revokes no device's backup-read token.` with:

   It never mints above the relay's `read_epoch`, so it revokes no device's backup-read token. A second refusal (a rotation between the two requests) ends the top-up with `device/recovery_token`, and the tool restores the backup as it is.

2. §21 Error catalogue: add this row directly after the row `| `device/recovery_chain` | a top-up found a chain break | §15.8 | restore the backup as it is |`:

   `| `device/recovery_token` | the relay refused the recovery identity's read token twice | §15.8 | restore the backup as it is |`

3. §25 Public API, `pub mod recovery_online`: replace `        pub fn read_token(&mut self, backup_token: Option<&str>, epoch: u32) -> Result<(), super::DeviceError>;` with `        pub fn read_token(&mut self, backup_token: Option<&str>) -> Result<(), super::DeviceError>;   // §15.8 step 1: the backup's token, else epoch 1, then the refusal's read_epoch`.

### R1-SEAL-DEVICE-6 (seal-device-7)

A writer starts a segment only after a base whose membership lists it as current, so the reader's step 10 (`seal.md` §21.6, same id) accepts it.

1. §15.2, the bullet **Segment**: replace the whole bullet with:

   - **Segment** is due when the UTC month (`YYYY-MM` of the device clock) differs from `last_segment.month` and a start base `x` below the newest base's seq exists. The start base is the base at `last_segment.seq` (for a writer's first segment, its oldest base); when that base is missing (a purge removed it) or its membership does not list this device as current (a paired device's oldest base is at its bootstrap seq `h`, below the `added_seq` that adds it; or the device was revoked there), `x` is the oldest later base whose membership lists this device as current, and the status records the skipped seqs as a gap. A segment covers the seqs from `x + 1` to the newest base's seq; its `membership` is the base's at `x` and its `chain_before` that base's `board_chain`, so both come from a base file and need no replay, and the writer is a current device of that membership, as `seal.md` §21.6 step 10 requires of a segment.

2. Reword DEV-BKP-005 (full text):

   - **DEV-BKP-005** (device) A segment's `membership` and `chain_before` MUST be taken from the base at its first seq − 1, whose membership MUST list the writer as a current device, and its frames MUST be the consecutive log frames from its first to its last seq.

### R1-SEAL-DEVICE-7 (seal-device-8)

The device builds every relay origin with `taskdeck_seal::relay_origin` and stores only normalised hosts, which never carry `:443` (`seal.md` §5.4, SEAL-ENC-006; `relay.md` §5.4 step 4; same id).

1. §10.1: replace `The relay origin is `https://<host>` built from the stored relay target's host (`seal.md` §5.4);` with `The relay origin is `taskdeck_seal::relay_origin(host)` of the stored relay target's host (`seal.md` §5.4: `https://` ‖ host, since a host never carries `:443`);`.

2. §10.3, the paragraph that begins `Login: `POST /v1/b/{board}/login/nonce``: replace `with `origin = "https://" ‖ host` from the device's own relay target, never from the relay;` with `with `origin = taskdeck_seal::relay_origin(host)` of the device's own relay target, never from the relay;`.

3. Reword DEV-LOGIN-001 (full text):

   - **DEV-LOGIN-001** (device) The device MUST sign every login with the relay origin that `taskdeck_seal::relay_origin` builds from its own stored relay target's host, and MUST make the new token durable in `device.tdk` before it sends any request with it.

4. §18.1, step 1 (as R1-CORE-DEVICE-7 left it): replace `` `relay_host` a host of `seal.md` §5.4 (at most 255 bytes, `:port` included), else `relay/address`; `` with `` `relay_host`, after the normalisation of `relay.md` §5.4 step 4 (letters to lowercase, a trailing `:443` removed), a host of `seal.md` §5.4 (at most 255 bytes, `:port` included), else `relay/address`; the normalised host is the one stored and used; ``.

5. §17.8, the table, row `move_board(host, create_secret)`, its "Engine input" cell as R1-DEVICE-ENGINE-18 left it: replace `` `Op(Move { host })` once `host` passes `seal.md` §5.4 (at most 255 bytes, `:port` included; else `relay/address`); `` with `` `Op(Move { host })`, `host` normalised as `relay.md` §5.4 step 4 (letters to lowercase, a trailing `:443` removed), once it passes `seal.md` §5.4 (at most 255 bytes, `:port` included, never `:443`; else `relay/address`); ``. (The rest of the cell is unchanged.)

### R1-SEAL-DEVICE-8 (seal-device-9)

`verify_backup_file` is in `seal.md` §26 (same id), so the request becomes a reliance.

1. §15.3.2: replace `with `taskdeck_seal::verify_backup_file` (§24; `seal.md` §21.6 steps 1, 2 and 5)` with `with `taskdeck_seal::verify_backup_file` (`seal.md` §26, SEAL-BKP-009: §21.6 steps 1, 2 and 5)`.

2. §24, `seal.md` block: replace the bullet that begins `- Requires: a public `verify_backup_file(bytes, writer_sign_pk) -> Result<BackupHeader, SealError>`` (the whole bullet) with:

   - Relies on: `taskdeck_seal::verify_backup_file(bytes, writer_sign_pk) -> Result<BackupHeader, SealError>` (`seal.md` §21.6, §26, SEAL-BKP-009), which runs §21.6 steps 1, 2 and 5 only, so a writer checks what it wrote without the recovery key (§15.3.2).

### R1-SEAL-DEVICE-9 (seal-device-10)

History records are built and encoded only by seal's `HistoryRecord` (`seal.md` §21.5, §26, SEAL-BKP-010, same id).

1. §6.4.8: append at the end of the paragraph:

   It is built by `taskdeck_seal::HistoryRecord::from_accepted`, written as its `to_bytes()`, and read with `HistoryRecord::from_bytes` (`seal.md` §26); a record that `from_bytes` refuses is `device/record_format`.

2. §8.3, the table, row `history records (kind 11)`: in its Written cell replace `from the sealer adapter's classification (it keeps the opened items, inner signature, `entry_id`, `at_ms`, `ctr` and author of every `Accept` of kind `ctl` from the dry run);` with `built by `taskdeck_seal::HistoryRecord::from_accepted(seq, frame, opened)` from the sealer adapter's classification (it keeps the frame and the `Opened` of every `Accept` of kind `ctl` from the dry run);`. (The rest of the cell, and the row R1-DEVICE-ENGINE-1 added after it, are unchanged.)

3. §25 Public API, `pub mod store`: replace `        -> Result<(taskdeck_sync::Loaded, Vec<Vec<u8>>), super::DeviceError>;              // Loaded, history records` with `        -> Result<(taskdeck_sync::Loaded, Vec<taskdeck_seal::HistoryRecord>), super::DeviceError>;   // Loaded, history records`.

### R1-SEAL-DEVICE-10 (seal-device-13)

`release` searches for the marker `seal.md` now defines (SEAL-GEN-014, `TEST_ENTROPY_MARKER`, same id).

1. §11.2, item 2: replace `and the `seal` stage's own `test-entropy` marker (`seal.md` SEAL-GEN-005), and fails when one is found.` with `and `taskdeck-test-entropy-enabled-3b9e5d` (`taskdeck_seal::TEST_ENTROPY_MARKER`, `seal.md` SEAL-GEN-014), and fails when one is found.`

2. §11.2, item 3: replace the whole item with:

   3. **Positive controls**: the same script run on a `taskdeck-devicectl` binary must find `taskdeck-test-trust-enabled-6f1c2a`, and run on the test binary of `taskdeck-seal` built with `test-entropy` (`cargo test -p taskdeck-seal --features test-entropy --no-run`) must find `taskdeck-test-entropy-enabled-3b9e5d`, so that a broken search cannot pass.

3. Reword DEV-TRUST-003 (full text):

   - **DEV-TRUST-003** (release) The release workflow MUST fail when the feature resolution of any release build includes `test-trust`, `test-entropy` or `sim`, and when any release binary contains the marker of §11.1 item 7 or `taskdeck_seal::TEST_ENTROPY_MARKER` (§11.2 item 2), with both positive controls of §11.2 item 3 passing in the same run.

4. §24, `seal.md` block: replace the bullet `- Assumes: SEAL-GEN-005's "a build without `test-entropy`" is checked by `release` together with `test-trust` (DEV-TRUST-003), with a marker string `seal` defines.` with:

   - Relies on: `taskdeck_seal::TEST_ENTROPY_MARKER` (`seal.md` SEAL-GEN-014), which a build with `test-entropy` contains and `release` searches for together with the `test-trust` marker (§11.2, DEV-TRUST-003).

### R1-SEAL-DEVICE-11 (seal-device-14)

No sealed record plaintext exceeds `seal.md` §19.1's 67,108,864 bytes: a log page record is bounded by bytes as well as by count, and so is an outbox record, whose 1,024 ops of up to 1,048,576 bytes each could also exceed it. Kind 8 cannot (1,024 × 4,110 bytes).

1. §6.4: add this paragraph directly under the heading `### 6.4 Plaintexts`, before `#### 6.4.1 Log page (kind 1)`:

   No record plaintext is larger than 67,108,864 bytes, the limit of `seal.md` §19.1 (`ct_len` at most 67,108,880 with the tag). An op of kind 1 or 5 that would exceed it is written as consecutive records, each ending before the entry or op that would make it exceed the limit (§6.4.1, §6.4.4).

2. §6.4.1: replace `An `AppendLog` op with more than 256 records is written as consecutive records of at most 256 entries.` with `An `AppendLog` op is written as consecutive records, each of at most 256 entries and at most 67,108,864 plaintext bytes (§6.4); 256 entries of the largest frames (2 + 256 × 262,196 = 67,122,178 bytes) exceed the latter, so such a record holds fewer.`

3. §6.4.4: replace `A `RewriteOutbox` writes a new file whose records hold one `Put` per live record, in `order`, at most 1,024 per sealed record.` with `A `RewriteOutbox` writes a new file whose records hold one `Put` per live record, in `order`, at most 1,024 and at most 67,108,864 plaintext bytes per sealed record; an `Outbox` op is split the same way (§6.4).`

4. §6.1, the kinds table: replace the row `| 5 | `outbox.tdo` | appended, one per op; rewritten | `Outbox`, `RewriteOutbox` | §6.4.4 |` with `| 5 | `outbox.tdo` | appended, one per op (more when §6.4 splits it); rewritten | `Outbox`, `RewriteOutbox` | §6.4.4 |`.

5. §22 Limits: replace the row `| entries per log page record | 1 to 256 | entries | DEV-STORE-005 |` with `| entries per log page record; any record plaintext | 1 to 256; 67,108,864 | entries; bytes | DEV-STORE-005 |`.

### R1-SEAL-DEVICE-12 (seal-device-15)

§12.1: replace `(18 + 16 + 16 = 50 bytes)` with `(22 + 16 + 16 = 54 bytes: `ctx("local-key")` is the 21 characters of `taskdeck/v1/local-key` and one `0x00`)`.

### R1-SEAL-DEVICE-13 (seal-device-16)

A token is opaque, 1 to 512 bytes, wherever the device stores it, and the create-time test backup carries none (`seal.md` §21.5, SEAL-KEY-015, same id).

1. §16, the `device.tdk` / `device.json` table, row `tokens`, its Constraints cell as R1-DEVICE-ENGINE-18 left it: replace `` `token` 96 ASCII characters `` with `` `token` ASCII, 1 to 512 bytes (`seal.md` §6.7, SEAL-KEY-015) ``.

2. §16, the `backup` table, row `read_token`: replace `| `read_token` | `{"epoch": int[1..4294967295], "token": string}` | optional | | §15.4 |` with `| `read_token` | `{"epoch": int[1..4294967295], "token": string}` | optional | `token` ASCII, 1 to 512 bytes (`seal.md` §6.7) | §15.4 |`.

3. §18.1, step 6: replace `a nightly backup of the genesis state is written in memory to the recovery key and read back` with `a nightly backup of the genesis state is written in memory to the recovery key, with no read token (`read_token: None`, `seal.md` §21.5: the board holds none yet), and read back`.

4. §15.8, step 1 (as R1-SEAL-RELAY-27 wrote it): replace `Try the backup's read token on `hello`. When it is refused (`401`, `403`), mint` with `Try the backup's read token on `hello`, unless the backup carries none (an empty `read_token`, `seal.md` §21.5). When it is refused (`401`, `403`), or the backup carries none, mint`.

### R1-SEAL-DEVICE-14 (seal-device-17)

The picture reference is turned into the `BlobRef` that `open_blob` takes, with `inflated: 0`, which `open_blob` does not read; taking the relay's `author` as writer is now what `seal.md` §20.2 allows for a kind-2 blob (same id).

§14.2, step 3: replace `` `taskdeck_seal::open_blob` with the reference, `` with `` `taskdeck_seal::open_blob` with a `BlobRef` built from the reference (`id` its `blob`, its `sha256`, `size`, `parts` and `epoch`, and `inflated: 0`, which `open_blob` does not read, `seal.md` §20.5), ``, and, in the same step, replace `The writer is bound by the AEAD's AD (`seal.md` §20.2),` with `The writer is bound by the AEAD's AD (`seal.md` §20.2, which admits the relay's author for a kind-2 blob),`.

## Rejected

None as a whole. Parts of proposals are not adopted, each recorded in its change:
- seal-device-4: the `roots` field of `DeviceReinstated`, its §13.6 precondition, the clause on unwrapping the item's roots, the limit of 1,024 and `Receipt.learned_roots: Vec<(u32, EpochRoot)>`. Lead ruling L1 moves the wraps into the `reinstate` operation, because the reinstated device cannot open the reinstating entry (R1-SEAL-DEVICE-4).
- seal-device-2: replacing device.md's `store::KIND_*` constants by `FileKind`. R1-DEVICE-ENGINE-14 kept them and added one, so they stay, now derived from `FileKind` (R1-SEAL-DEVICE-2).
- seal-device-3: the third bullet of device.md §24, the history-blob upload and open question 6, already removed or rewritten by R1-SEAL-RELAY-28; the DEV-RUN-003 addition, covered by DEV-RUN-006 (R1-DEVICE-ENGINE-1).
- seal-device-6: the relay.md change, already R1-SEAL-RELAY-20 (with the field name `read_epoch`).
- seal-device-9: the clause text "MUST NOT decrypt" alone; SEAL-BKP-009 also forbids inflating or parsing `ct`.
- seal-device-12: stating no name at all in seal.md §21.1 (R1-SEAL-RELAY-11 names the form once and points to its owner).
- seal-device-17: the separate type `PlainBlobRef`; the finding's alternative (state that `inflated` is not read) changes fewer signatures.

## For the lead

1. **Seal's side of ruling L1 (seal-device-4), written without `r1-lead-changes.md`.** That file does not exist yet. R1-SEAL-DEVICE-4 applies L1 to seal.md exactly: the `reinstate` args `dev[16] ‖ u32 first_epoch ‖ u16 n ‖ n × wrap[96]`, `AdminOp::Reinstate { dev, first_epoch, wraps }`, the §23 limit of 256, and §27 item 18 marked as ruled. It also adds what L1 leaves to seal: how such a wrap is opened. It is opened by SEAL-WRAP-002, with `c_e` from the device's own membership, oldest epoch first, once each commitment is recorded, and a failure is the commitment halt (SEAL-WRAP-003). It does not change `Receipt.learned_root`, which the finding wanted to make a list. Under L1 the roots arrive at login, not inside an entry, so the sealer adapter opens them with the existing `unwrap_root` and no seal signature changes. Recommended: keep this, and let the lead's change file win wherever it words the same places (§16.2 row, `AdminOp`, §27 item 18). The relay's half (storing the wraps on the `devices` row and returning them in `login`) is still for the lead's file or the relay–device resolver.
2. **A host may no longer carry `:443` (seal-device-8).** Today seal's regex accepts `relay.example:443` while relay.md says "443 never written", so two spellings of one relay exist. They sign different login origins and give different pairing transcripts. R1-SEAL-DEVICE-7 makes `:443` invalid in a host (SEAL-ENC-006). It adds `relay_origin(host)` with the code `seal/host_format`, and makes relay.md §5.4 step 4 remove a typed `:443` before it checks the host. The device normalises typed hosts the same way. Option (b) would keep `:443` valid and have every comparer and origin builder drop it, which needs a normalisation in four places instead of one. Recommended: accept (a), as written.
3. **A backup's `read_token` may be empty (seal-device-16).** The create-time test backup (device.md §18.1 step 6) is written before any relay token exists. R1-SEAL-DEVICE-13 lets `read_token` have length 0, meaning none (`Option<String>` in `BackupInput` and `VerifiedBackup`). The recovery tool then goes straight to minting. The alternative is a placeholder token in the test backup, which seal would have to accept as a real token. Recommended: accept the empty form, as written.
