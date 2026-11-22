# Resolutions R1: seal.md against engine.md (15 findings)

Resolver 2 of 9. Inputs read: reviews.md §B, spec/README.md, foundation.md "Decisions" and "Rulings for the specs" (S1 to S31, with the settled S15), SYNC_PLAN.md §2 and §3, the earlier resolution file `r1-seal-relay.md` in full, seal.md §5 to §27, engine.md §1 to §38 where the findings touch them, and the relay.md and device.md passages the changes touch.

Built on `r1-seal-relay.md` (settled; not repeated here). Its changes already resolve parts of these findings, and the changes below give only what must be added or adjusted:
- seal-engine-1: R1-SEAL-RELAY-2 (the §13.6 `Skip` precondition, the §14.6 sentence, the §23 "Skip window" row, §27 item 12) and R1-SEAL-RELAY-16 (RELAY-COMPACT-007).
- seal-engine-2: R1-SEAL-RELAY-8 (seal.md §27 item 18, the conservative reading) and its "For the lead" item 1.
- seal-engine-3: R1-SEAL-RELAY-9 (snapshot `history`, SEAL-BLOB-006, SEAL-BLOB-007, `SnapshotBody.history: Vec<HistoryRecord>`) and R1-SEAL-RELAY-28 (device.md §15.7: the sealer adapter fills and stores the history). Its "For the lead" item 4 asked this resolver to confirm the engine's side or add `history` to `SnapshotMaterial`: confirmed, the engine stays out of it (R1-SEAL-ENGINE-15).
- seal-engine-4: R1-SEAL-RELAY-1 (seal.md §5.4, SEAL-ENC-006, §23 host row).
- seal-engine-5: R1-SEAL-RELAY-6 (§22 `seal/pair_raced` row) and R1-SEAL-RELAY-25 (engine.md §28.4 step 4, ENG-OPS-004).
- seal-engine-9: R1-SEAL-RELAY-3 (the §16.2 registry table) and R1-SEAL-RELAY-16 (relay.md §12.8).
- seal-engine-11: R1-SEAL-RELAY-10 (seal.md §19.2, `FileKind`) and R1-SEAL-RELAY-29 (device.md).
- seal-engine-12: R1-SEAL-RELAY-11 (seal.md §21.1).

New clause ids assigned here (later resolvers continue after them, and after those of `r1-seal-relay.md`):
- seal.md: SEAL-BLOB-008.
- engine.md: ENG-OPS-006, ENG-OPS-007, ENG-SNAP-006.

New error codes: `seal/old_epoch` (seal.md §22), `engine/bad_host` (engine.md §33).

Notation. "Replace A with B" means the exact text A, as it stands in the spec after the changes of `r1-seal-relay.md`, becomes B. Clause texts are given in full. A table row is given in full, in the table's own column order.

## seal.md

### R1-SEAL-ENGINE-1 (seal-engine-1)

Ruling S15 as settled: the Skip floor is checked before any frame is asked for, on every device alike, so whether a device holds a frame never changes the class or the code. R1-SEAL-RELAY-2 put the precondition into §13.6 and a sentence into §14.6; SEAL-RCPT-009 still says "MUST return `NeedFrames` … when a `Skip` item's target frame is not supplied", which contradicts that sentence, and a multi-item `S`-shape entry could give different codes on a device with and one without the frame. No new clause (the proposed SEAL-MEM-028 is not added): SEAL-RCPT-009 is reworded instead, and SEAL-MEM-011 keeps enforcing the precondition.

1. §13.6, the `Skip` row as R1-SEAL-RELAY-2 left it: in its Preconditions cell replace `` `t ≥ max(W.purge_seq, s − 16,384)` (the Skip window, ruling S15); `` with `` `t ≥ max(W.purge_seq, s − 16,384)`, with `s − 16,384` taken as 0 when `s ≤ 16,384` (the Skip window, ruling S15); ``. Nothing else in the row changes.

2. §14.6: after the sentence R1-SEAL-RELAY-2 added (the one beginning `A target below `max(purge_seq, s − 16,384)``), and before SEAL-RCPT-009, add:

   For an `S`-shape entry the preconditions of §13.6 are checked in two passes, on every device alike: first, for each item in order, every precondition that does not need the target frame (the floor, `ctl_seqs`, `skips`); then, for each item in order, the frame's presence (`NeedFrames` when any is missing), its hash and its author. The entry is refused with the first failure in that order, which is the order SEAL-MEM-012 means for this shape.

3. Reword SEAL-RCPT-009 (full text):

   - **SEAL-RCPT-009** (seal) A receiver MUST check, for every `Skip` item of an entry and before it asks for any frame, every precondition of §13.6 that does not need the target frame, and MUST refuse the entry when one fails, whether or not frames were supplied; only when all of them pass MUST it return `NeedFrames`, without changing any state, for the target frames not supplied, and MUST it refuse the entry when a supplied target frame's hash differs from the item's `frame_hash`.

### R1-SEAL-ENGINE-2 (seal-engine-5, seal-engine-8)

A pairing raced by a rotation fails cleanly in both orders, and N's machine can fetch Skip target frames while it verifies the log. Engine side: R1-SEAL-ENGINE-18 (ENG-OPS-006); device side: R1-SEAL-ENGINE-22.

1. §17.8, the bundle table, row `roots`: replace its Constraints cell with:

   1 or 2 objects `{"epoch": integer, "root": hex32}`, ascending by `epoch`: the current epoch of `M_h`, which is `snapshot.epoch`, and the epoch before it when the current epoch is 2 or more; never a later epoch (`engine.md` §28.4, ENG-OPS-006)

   (Type, Required and Meaning cells unchanged.)

2. §17.9, step 6: replace the whole step with:

   6. every entry from `h + 1` to the relay's head is classified in seq order from `M_h` (the snapshot's membership) and `C_h = head.chain`: any `Tamper` aborts (`seal/bundle_invalid`); `Skip` and `Refuse` continue (the entry at `added_seq` is step 7's); `NeedFrames` is resolved by taking each named frame from the entries already read when its seq is above `h`, and otherwise by fetching it from the relay with the bundle's token (`relay.md` `read` with `after = t − 1`, `limit = 1`), checking that it has seq `t` and the named `frame_hash`, and classifying the same entry again with the frames; a named frame the relay does not serve, or serves with another seq or hash, aborts (`seal/bundle_invalid`); `seal/missing_root` aborts (`seal/pair_raced`: a rotation after `h` of which N is not a recipient);

3. §17.9, the paragraph after step 8: replace `A rotation sequenced between `h` and `added_seq` makes the `DeviceAdded` a stale-epoch control entry, which is refused (§11.1); N then aborts with `seal/pair_raced` and the user pairs again (V2-8).` with:

   A rotation sequenced between `h` and `added_seq` races the pairing in either order. If E sealed the `DeviceAdded` before it classified the rotation, the entry is a stale-epoch control entry and is refused everywhere (§11.1): step 7 aborts with `seal/pair_raced`, and E's engine revokes the registered device (`engine.md` §28.4). If E classified the rotation first, its engine never sends the `DeviceAdded` (`engine.md` ENG-OPS-006) and the pairing fails on E with `seal/pair_raced`. Either way N, which is no recipient of that rotation, meets `seal/missing_root` at the first entry after it (step 6) unless it aborted earlier, and the user pairs again (V2-8).

4. §17.10, table **N (new device)**: add these two rows directly after the row `| `AwaitSnapshot` | `Snapshot{parts}` | §17.9 steps 4 and 5 pass | `ReadLog{token, from h + 1}` | `Joining` |`:

   `| `Joining` | `Entries{frames}` | a classification of §17.9 step 6 returns `NeedFrames` for seqs at or below `h` | `FetchFrames{token, seqs}` | `Joining` |`
   `| `Joining` | `Frames{frames}` | each frame has the seq asked for and the `frame_hash` its `Skip` item names | classify the waiting entry again with them and continue §17.9 step 6 | `Joining` |`

   (A frame that fails the guard is "a failed check" of the existing last row and aborts with `seal/bundle_invalid`. `NewEvent` and `NewEffect` of §26 gain the variants `Frames` and `FetchFrames`; their declarations stay `/* §17.10 */`.)

5. §22 Error catalogue: replace the row R1-SEAL-RELAY-6 wrote, `| `seal/pair_raced` | E's DeviceAdded was refused (a rotation raced it) | §17.9 | abort; pair again (E's engine first sends `revoke` for the new device, `engine.md` §28.4) |`, with:

   `| `seal/pair_raced` | a rotation raced the pairing: E's DeviceAdded was refused or never sent, or N meets an entry under an epoch whose root the bundle does not hold | §17.9 steps 6 and 7, `engine.md` §28.4 | abort; pair again (when E's DeviceAdded was sent and refused, E's engine first sends `revoke` for the new device, `engine.md` §28.4) |`

### R1-SEAL-ENGINE-3 (seal-engine-6)

A rejoin opens a snapshot before it has seen the announcing `snap` entry (`engine.md` §19.7). Seal gains a declared mode of §20.5; every declared value is checked later against the signed entry by the engine.

1. §20.5: add this paragraph after the paragraph that begins `Splitting: part `i``, before the clauses:

   **Opening a declared snapshot** (a rejoin, `engine.md` §19.7). A device that has not yet classified the announcing `snap` entry opens a snapshot from what the relay declares about the blob (`relay.md` §10.7): its `sha256`, its `size`, the seq `S`, and its `author`, used as `writer_dev`. The steps are those above, with these differences: step 1 uses the declared `size` and `parts = ceil(size / 1,048,576)`; step 2 the declared `sha256`; step 3 checks `magic`, `ver`, `suite` and `blob_kind = 1`, and reads `epoch` from bytes 7 to 11; step 4 takes the root of that epoch from the caller's roots (none: `seal/missing_root`) and derives `K_snap` from it; step 6 inflates under a cap of 67,108,864 bytes, with no expected length; step 7 checks `S` against the declared seq and does not compare the state hash. The reader returns the body together with the header's epoch and the inflated length. No declared value is trusted: the caller checks the seq, the state hash, `sha256`, `size`, `parts`, `inflated`, `epoch` and the writer against the signed `snap` entry once it has classified it (`engine.md` §19.7 step 5).

2. §20.5: add after SEAL-BLOB-007 (which R1-SEAL-RELAY-9 adds):

   - **SEAL-BLOB-008** (seal) A reader opening a declared snapshot MUST check the parts against the declared `size` and SHA-256 before any decryption, MUST derive the key only from a root it already holds for the epoch in the sealed header, MUST open the AEAD before it parses any field of the body, and MUST inflate under a cap of 67,108,864 bytes.

3. §20.2, in the paragraph R1-SEAL-RELAY-12 rewrote: replace `A reader takes `writer_dev` from the signed entry or the bundle, never from the relay's blob `author` (`relay.md` §10.7).` with:

   A reader takes `writer_dev` from the signed entry or the bundle, never from the relay's blob `author` (`relay.md` §10.7), except in the declared mode of §20.5, where the caller checks it afterwards against the author of the signed `snap` entry.

4. §22 Error catalogue: replace the row `| `seal/missing_root` | the receiver lacks a root for an accepted epoch | step 9 | local fault; `engine.md` decides |` with:

   `| `seal/missing_root` | the receiver lacks a root for an accepted epoch, or for a declared snapshot's epoch | step 9; §20.5 (declared mode); §17.9 step 6 | local fault; `engine.md` decides; a rejoin tries its next candidate; in pairing N aborts with `seal/pair_raced` |`

5. §24 Test vectors, row `seal-blob.json` (as R1-SEAL-RELAY-9 left it): append to its Expected-outputs cell `; declared-mode openings (§20.5) with the returned epoch and inflated length, and their failures`.

6. §26 Public API: after the line `pub fn open_snapshot(parts: &[&[u8]], expected: &BlobRef, s: u64, writer: &DevId, key: &SymKey,` and its continuation line `                     board: &BoardId) -> Result<SnapshotBody, SealError>;`, add:

   ```rust
   pub fn open_snapshot_declared(parts: &[&[u8]], sha256: &[u8; 32], size: u64, s: u64, writer: &DevId,
                                 roots: &dyn Fn(u32) -> Option<&EpochRoot>, board: &BoardId)
                                 -> Result<(SnapshotBody, u32, u64), SealError>;  // §20.5 declared mode: (body, epoch, inflated)
   ```

### R1-SEAL-ENGINE-4 (seal-engine-7)

`seal/bad_epoch` keeps the alarm and now means an epoch from the future only; an epoch older than the previous one, which honest builds produce (a frame sealed and durable before two quick rotations), gets its own code, `seal/old_epoch`, which the author re-seals and which raises no alarm. SEAL-ENV-005 is unchanged.

1. §14.2, row 9: replace its On-failure cell `` `Refuse`, `seal/bad_epoch` `` with:

   `Refuse`, `seal/bad_epoch` when `epoch > E` (at `s = 1`, any epoch other than 1); `seal/old_epoch` when `epoch < E − 1`

2. §9.3, item 3: replace `3. Any other epoch is refused.` with `3. Any other epoch is refused: an epoch above `E` with `seal/bad_epoch`, an older one with `seal/old_epoch` (§14.2 step 9).`

3. §22 Error catalogue: replace the row `| `seal/bad_epoch` | epoch neither current nor previous | step 9, §9.3 | `Refuse`; alarm naming the author |` with these two rows, in this order:

   `| `seal/bad_epoch` | epoch above the current one (at seq 1, not 1) | step 9, §9.3 | `Refuse`; alarm naming the author |`
   `| `seal/old_epoch` | epoch older than the previous one | step 9, §9.3 | `Refuse`; the author re-seals it (`engine.md` §24.3); no alarm |`

### R1-SEAL-ENGINE-5 (seal-engine-9)

The keep list of a compaction also holds every recorded Skip target, so a device that bootstrapped after a target but before its Skip can still fetch it (`engine.md` §20.2). No byte changes.

§16.2, the registry table as R1-SEAL-RELAY-3 left it, row `compact`: replace its "Sent with" cell with:

`keep_seq` ascending: the control entries and the recorded Skip targets (§13.1 `skips`) below `below_seq` that stay (`engine.md` §20.2); `below_seq` at most `head_seq − 16,384` (`relay.md` §12.8)

### R1-SEAL-ENGINE-6 (seal-engine-12)

Nothing further to apply: R1-SEAL-RELAY-11 already replaces the untagged name in §21.1 with `TaskDeck-<date>-<seq>-<tag>.tdbk` (ruling S28) and points to `device.md` §15.3 for names and the tag.

### R1-SEAL-ENGINE-7 (seal-engine-13)

`engine.md` §18.3 (ENG-HASH-001) stays the only definition of the state hash; seal names it and exposes the function its own checks use.

1. §25, `engine.md` block, third bullet: replace the whole bullet (`- Requires: the S7 state-hash composition, …; seal checks recomputed hashes in §17.9, §20.5 and §21.6 by calling it.`) with:

   - Requires: the state hash of ENG-HASH-001 (`engine.md` §18.3), over §13.2's membership bytes. Seal computes it with its own `state_hash` (§26), which reproduces `taskdeck_sync::state_hash` byte for byte and is tested against `engine-hash.json` (`engine.md` §35), for the checks of §17.9, §20.5 and §21.6.

2. §26 Public API: in the block `// ---------- chain and fingerprints (§15)`, after the line `pub fn key_fingerprint(sign_pk: &[u8; 32], hpke_pk: &[u8; 32]) -> String;`, add:

   ```rust
   pub fn state_hash(hash_version: u32, board_hash: &[u8; 32], membership_bytes: &[u8]) -> [u8; 32];  // engine.md §18.3, ENG-HASH-001
   ```

   (`board_hash` is SHA-256 of the canonical board bytes, as in `taskdeck_sync::state_hash`.)

### R1-SEAL-ENGINE-8 (seal-engine-14)

The seal crate writes the `based_on` its caller gives it; which head that is (the board head `B`) is the engine's (ENG-ENQ-005).

1. Reword SEAL-PLAIN-003 (full text):

   - **SEAL-PLAIN-003** (seal) A sealer MUST set `based_on_seq` and `based_on_chain` to the caller's `based_on` unchanged, and MUST refuse with `seal/bad_plaintext` to seal an entry whose `based_on_seq` is 0 and whose `based_on_chain` is not `C_0` (§15.1).

2. §10.1, row `based_on_seq`: replace its "Value or rule" cell with `the seq of the composer's board head when it composed the entry (`engine.md` ENG-ENQ-005); 0 if it held no entry`.

3. §22 Error catalogue: replace the row `| `seal/bad_plaintext` | plaintext layout, padding, kind byte or `at_ms` range wrong | step 11, §10.3 | `Refuse`; alarm naming the author |` with:

   `| `seal/bad_plaintext` | plaintext layout, padding, kind byte or `at_ms` range wrong; when sealing, a `based_on_chain` other than `C_0` for `based_on_seq` 0 | step 11, §10.3; SEAL-PLAIN-003 | `Refuse`; alarm naming the author; when sealing, nothing is sealed (a bug of the caller) |`

### R1-SEAL-ENGINE-9 (seal-engine-15)

The sealed identity file holds the epoch roots a device has learned, its pending rotation roots and the genesis anchor's `R_1` (`engine.md` §6.1 `export_identity`), so an epoch root must be exportable, and only there.

1. Reword SEAL-KEY-009 (full text):

   - **SEAL-KEY-009** (seal) The `seal` crate MUST NOT offer any function that returns a device's or the recovery key's private key bytes, or an epoch root's bytes, except the serialisations used for the sealed identity file (§19.2, kind 7), which MUST be returned only in a zeroizing buffer.

2. §26 Public API: replace

   ```rust
   impl EpochRoot {
       pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError>;
       pub fn from_bytes(bytes: [u8; 32]) -> Self;
       pub fn commitment(&self, board: &BoardId, epoch: u32) -> [u8; 32];       // §6.5
   }
   ```

   with

   ```rust
   impl EpochRoot {
       pub fn generate(rng: &mut dyn Entropy) -> Result<Self, SealError>;
       pub fn from_bytes(bytes: [u8; 32]) -> Self;
       pub fn to_bytes(&self) -> zeroize::Zeroizing<[u8; 32]>;                   // §19.2 kind 7 only (SEAL-KEY-009)
       pub fn commitment(&self, board: &BoardId, epoch: u32) -> [u8; 32];       // §6.5
   }
   ```

## relay.md

### R1-SEAL-ENGINE-10 (seal-engine-9)

The compacting device keeps every recorded Skip target as well as every control entry (`engine.md` §20.2, ENG-COMPACT-003). The relay checks neither; no byte or check changes.

1. §12.8, the paragraph that defines `args`: replace `delete entries with seq below `below_seq` except the `keep_seq`s (the control entries below it),` with `delete entries with seq below `below_seq` except the `keep_seq`s (the control entries and recorded Skip targets below it, `engine.md` §20.2),`.

2. §12.8, the paragraph **Skip targets (ruling S15).** as R1-SEAL-RELAY-16 rewrote it: append at the end of the paragraph:

   Targets of Skips already sequenced stay as well: the compacting device lists them as `keep_seq` with the control entries (`engine.md` §20.2), for the devices that bootstrapped after such a target and must still classify its Skip.

3. §12.2, the table of what the relay can and cannot verify, the row `| for frames carried by an operation, everything of §9.3 and §9.4 | whether a compaction's keep list names every control entry below its bound |`: replace its second cell with `whether a compaction's keep list names every control entry and recorded Skip target below its bound`.

4. §22, `engine.md` block, the bullet that begins `- Requires, of the engine's duties:` (as R1-SEAL-RELAY-16, -17 and -21 left it): replace `lists every control entry below `below_seq` as `keep_seq`` with `lists every control entry and every recorded Skip target below `below_seq` as `keep_seq``.

## engine.md

### R1-SEAL-ENGINE-11 (seal-engine-1, seal-engine-9)

`seal.md` §13.6 now states the floor (R1-SEAL-RELAY-2, R1-SEAL-ENGINE-1); the engine relies on it instead of requesting it, and its compaction also keeps the targets of accepted Skips (R1-SEAL-ENGINE-12).

1. §17.1, step 4: replace `4. `t ≥ max(M_K.purge_seq, L + 1 − 16,384)`: the Skip floor of §17.4.` with `4. `t ≥ max(M_K.purge_seq, L + 1 − 16,384)`, with `L + 1 − 16,384` taken as 0 when `L + 1 ≤ 16,384`: the Skip floor of §17.4.`

2. §17.4: replace the whole paragraph (from `A Skip at seq `s` may target only` to `This rule is a requirement on `seal.md` §13.6 (§36).`) with:

   A Skip at seq `s` may target only `t ≥ F(s) = max(M_{s−1}.purge_seq, s − 16,384)`, with `s − 16,384` taken as 0 when `s ≤ 16,384` (`seal.md` §13.6, ruling S15). Every device computes `F(s)` from the log alone, and the sealer refuses a target below it before it asks for any frame (`seal.md` SEAL-RCPT-009), so the verdict does not depend on whether a device holds the frame. The engine keeps every frame a Skip may still need fetchable: its compaction never deletes an entry at or above `L − 16,384` (§20.2, `relay.md` RELAY-COMPACT-007), so every target a later Skip may name is still on the relay; and it keeps the target of every Skip already accepted (§20.2 step 3), which a device that bootstrapped after the target but before the Skip must still fetch (§17.2).

   (ENG-SKIP-004 is unchanged.)

3. §36, `seal.md` block, second bullet: replace the whole bullet (`- Requires (ruling S15): `seal.md` §13.6's `Skip` precondition gains `t ≥ s − 16,384`, …`) with:

   - Relies on (ruling S15): `seal.md` §13.6's `Skip` precondition `t ≥ max(W.purge_seq, s − 16,384)`, refused with `seal/ctl_target` before any frame is asked for (SEAL-RCPT-009), which is the floor `F(s)` of §17.4; the engine's compaction keeps the frames above it and the target of every accepted Skip (§20.2).

4. §38, item 12: replace the whole item with:

   12. **The Skip floor** `F(s) = max(purge_seq, s − 16,384)`, compaction that keeps the newest 16,384 seqs, and a keep list that holds every accepted Skip's target settle ruling S15 (§17.4, §20.2, `seal.md` §13.6). Decided here.

### R1-SEAL-ENGINE-12 (seal-engine-9)

1. §20.2, step 3: replace `3. **Keep.** Every seq of `M_K.ctl_seqs` from the relay's `floor` to `b − 1`, ascending (V2-15).` with:

   3. **Keep.** Every seq of `M_K.ctl_seqs` and every target `t` of `M_K.skips` from the relay's `floor` to `b − 1`, ascending, without duplicates (V2-15; a device that bootstrapped after a target but before its Skip still fetches the target, §17.2). When that list would exceed 131,072 seqs (`relay.md` §20), `b` is lowered to the largest value for which it does not.

2. Reword ENG-COMPACT-003 (full text):

   - **ENG-COMPACT-003** (engine) The engine MUST list in `keep` every control entry of its membership and every Skip target its membership records, between the relay's floor and `below_seq`.

### R1-SEAL-ENGINE-13 (seal-engine-2)

Conservative reading, as R1-SEAL-RELAY-8 (seal.md §27 item 18) took it: ruling S24 is not applied until the lead rules on a delivery channel (see "For the lead", item 1); a reinstated device pairs again. Only the engine's two pointers change.

1. §36, `seal.md` block, last bullet: replace the whole bullet (`- Question: a device reinstated by the recovery identity cannot read …`) with:

   - Open (ruling S24, with the lead; `seal.md` §27 item 18): a device reinstated by the recovery identity cannot read the entries sealed while it was revoked (no wrap names it), nor the reinstating entry itself, which is sealed under an epoch it lacks, so S24's wraps need a channel outside that entry. Until it is ruled, the device re-pairs (§24.4).

2. §38, item 24: replace the whole item with:

   24. **A reinstated device cannot read what was sealed while it was revoked** (§24.4); default: it re-pairs. Ruling S24 needs a channel outside the reinstating entry and is with the lead (`seal.md` §27 item 18).

### R1-SEAL-ENGINE-14 (seal-engine-6)

The rejoin uses `seal.md` §20.5's declared mode (R1-SEAL-ENGINE-3). The trait's `open_snapshot` return is changed in R1-SEAL-ENGINE-15.

1. §19.7, step 3: replace the whole step with:

   3. For a candidate: fetch the parts; `Sealer::open_snapshot(parts, expected, author, exact: false)`, with `expected.seq`, `blob`, `sha256`, `size` and `parts` from the blob's metadata (its other fields are not read) and `author` the metadata's author (`seal.md` §20.5, declared mode); then: the membership bytes parse, name this board and list this device as current; the state hash recomputed (§18.3) equals the body's. A failure (including the lack of the root of the blob's epoch) moves to the next candidate.

2. §19.7, step 5: replace `its `seq`, `hash`, `sha256`, `size`, `parts` and `epoch` must equal what was opened` with `its author must be the `author` the snapshot was opened with, and its `seq`, `hash`, `sha256`, `size`, `parts`, `inflated` and `epoch` must equal what was opened (the epoch and inflated length that `open_snapshot` returned)`. The rest of the step is unchanged.

3. Reword ENG-BOOT-002 (full text):

   - **ENG-BOOT-002** (engine) A rejoin MUST end only on a snapshot that opened under a root this device holds, whose recomputed state hash matched, whose announcing `snap` entry matched it in author, seq, hash, `sha256`, `size`, `parts`, `inflated` and epoch, and that is usable when the log reaches the relay head.

4. §36, `seal.md` block, third bullet: replace the whole bullet (`- Requires: `seal.md` §20.5 gains a rejoin mode for snapshots …`) with:

   - Relies on: the declared mode of `seal.md` §20.5 (SEAL-BLOB-008) for `open_snapshot(…, exact: false)` (§19.7): the parts are checked against the relay's declared `sha256` and `size`, the epoch is read from the sealed header, the AEAD authenticates before anything else is read, and inflation is capped at 67,108,864 bytes; the declared values and the writer are checked afterwards against the `snap` entry (§19.7 step 5).

### R1-SEAL-ENGINE-15 (seal-engine-3, seal-engine-6)

Ruling S27 on the engine's side: a snapshot body carries the control history (`seal.md` §20.4, R1-SEAL-RELAY-9), which the sealer fills and keeps itself; `SnapshotMaterial` does not change and the engine never sees the history (confirming the reading of R1-SEAL-RELAY-28). And `open_snapshot` returns the header's epoch and the inflated length for the rejoin's later check.

1. §6.1, the trait: replace

   ```rust
       fn open_snapshot(&self, parts: &[&[u8]], expected: &SnapInfo, writer: &Dev, exact: bool)
           -> Result<SnapshotMaterial, SealerError>;
   ```

   with

   ```rust
       fn open_snapshot(&self, parts: &[&[u8]], expected: &SnapInfo, writer: &Dev, exact: bool)
           -> Result<(SnapshotMaterial, u32, u64), SealerError>;   // (material, header epoch, inflated length)
   ```

2. §6.1, **Contracts** table: replace the row `| `seal_snapshot`, `open_snapshot` | `seal.md` §20; `exact = false` is the rejoin mode of §19.7, which takes `inflated` as an upper bound (§36) |` with:

   `| `seal_snapshot`, `open_snapshot` | `seal.md` §20. `seal_snapshot` seals under `m`'s current epoch and puts into the body the sealer's own control history up to `material.seq` (`seal.md` §20.4, ruling S27); the engine never supplies or reads it. `open_snapshot` checks the body's history (`seal.md` §20.5 step 7), keeps it for the runtime (`device.md` §15.7), and returns the material, the sealed header's epoch and the inflated length. With `exact = true` it is `seal.md` §20.5 with every value of `expected`, and the returned epoch and length equal `expected`'s; with `exact = false` it is the declared mode of `seal.md` §20.5 (§19.7) and reads only `expected.seq`, `blob`, `sha256`, `size` and `parts` |`

3. §6.3: add this paragraph directly before the paragraph that begins `The fake has a genesis helper`:

   **Control history.** The fake keeps in memory one history record (the layout of `seal.md` §21.5) for every control entry it accepts: from seq 1 for a fake made by `create`, and from the history of the snapshot it bootstrapped from for one made by `join` or after a rejoin. It puts the records with seq at most `S` into every snapshot body it seals at `S`, and checks a body's history on opening as `seal.md` §20.5 step 7 states. `FakeBundle` carries the history records with the snapshot material.

4. §37 Public API: in the `fake` module line, replace `pub struct FakeBundle { /* roots, snapshot material, added seq */ }` with `pub struct FakeBundle { /* roots, snapshot material and its history records, added seq */ }`.

   (The other callers of `open_snapshot`, §19.9 and §22.3 step 3, take the material from the returned tuple; their text is unchanged.)

### R1-SEAL-ENGINE-16 (seal-engine-4)

A host is at most 255 bytes, `:port` included (`seal.md` §5.4, SEAL-ENC-006, as R1-SEAL-RELAY-1 left them). The engine refuses a bad host from the user before any relay call.

1. §28, the paragraph after `pub struct OpRecord …` (the one that begins `Which platform may start which operation is `device.md`'s`): append this sentence at its end:

   `Move`, `FollowAddress` and `CloseBoard` are refused with `engine/bad_host`, before any effect, when their `host` fails `seal.md` §5.4 (SEAL-ENC-006: at most 255 bytes, `:port` included).

   and add directly after that paragraph:

   - **ENG-OPS-007** (engine) The engine MUST refuse `Operation::Move`, `Operation::FollowAddress` and `Operation::CloseBoard` with `engine/bad_host`, emitting no effect other than the refusal, when their `host` does not match `seal.md` §5.4.

2. §33 Error catalogue: add after the row `| `engine/move_no_fence` | … |`:

   `| `engine/bad_host` | the host the user typed is not a valid host (`seal.md` §5.4) | §28, ENG-OPS-007 | ask again |`

### R1-SEAL-ENGINE-17 (seal-engine-7)

`seal/old_epoch` (R1-SEAL-ENGINE-4) is re-sealable; `seal/bad_epoch` (now future epochs only) is not, and stays in §13.2's alarm list, which does not list `seal/old_epoch`.

1. §24.3, first sentence: replace `An own `cmd` entry refused with `seal/stale_epoch` or `seal/bad_epoch` (sealed under an epoch the position no longer accepts) is **re-sealable**` with `An own `cmd` entry refused with `seal/stale_epoch` or `seal/old_epoch` (sealed under an epoch the position no longer accepts) is **re-sealable**`.

2. Reword ENG-RESEAL-002 (full text):

   - **ENG-RESEAL-002** (engine) The engine MUST NOT re-seal an own entry that was accepted, and MUST NOT re-seal an own entry refused with a code other than `seal/stale_epoch` or `seal/old_epoch`.

### R1-SEAL-ENGINE-18 (seal-engine-5)

A rotation classified after `h` and before the send loop reaches the pairing's `DeviceAdded` would have it sealed under the new epoch, accepted, and N stranded without that root. The engine drops it unsent instead; the bundle never carries a root later than the snapshot's epoch. This adds to R1-SEAL-RELAY-25 (step 4, ENG-OPS-004), which covers a `DeviceAdded` that was sent and refused.

1. §28.4, step 3: replace the whole step with:

   3. When the send loop reaches it: if `M_K.epoch` differs from the epoch of the snapshot uploaded in step 1 (a rotation was classified after `h`), the engine drops the record unsent (`Outbox[Drop]`), sends nothing to the relay, and answers `Notify(PairFailed { ticket, code: "seal/pair_raced" })`; the user pairs again. Otherwise it seals the record and sends `Admin { Register { frame } }`. On success at seq `a`: `Notify(PairRegistered { ticket, head: (h, C_h), snapshot: SnapInfo, added_seq: a })`; the runtime hands the pairing machine the token from the admin answer and, from the sealer, the roots of the snapshot's epoch and of the epoch before it when that epoch is 2 or more, never a later root (`seal.md` §17.8); the machine posts slot D.

2. §28.4: add after ENG-OPS-004 (which R1-SEAL-RELAY-25 adds):

   - **ENG-OPS-006** (engine) The engine MUST NOT seal a pairing's `DeviceAdded` under an epoch other than that of the snapshot it uploaded for the bundle: when `M_K.epoch` differs from it, the engine MUST drop the record unsent and report the pairing failed with `seal/pair_raced`.

3. §36, `seal.md` block, fourth bullet: replace `the token in the `register` answer and the roots the sealer holds.` with `the token in the `register` answer and the roots of the bundle snapshot's epoch and the epoch before it, never a later root (`seal.md` §17.8, ENG-OPS-006).`

### R1-SEAL-ENGINE-19 (seal-engine-10)

Ruling S25: SYNC.md §4.7's two triggers (a week of log clock, or 1,000 entries behind), and at most one snapshot per device per 24 hours of log clock.

1. §19.2: replace the first sentence and condition 1, `At the board apply of each aligned `S`, the snapshot is **due at `S`** for this device when either:` and `1. it knows no `SnapRecord` that is usable (§19.3) with `τ_S − entry_clock < 604,800,000` (seven days), and none that is still waiting for verdicts (no verdict at all) with `τ_S − entry_clock < 172,800,000` (two days); or`, with:

   At the board apply of each aligned `S`, the snapshot is **due at `S`** for this device when no own `snap` record is in its outbox, its newest own `SnapRecord` (author this device), if any, has `τ_S − entry_clock ≥ 86,400,000` (at most one snapshot per 24 hours of log clock, ruling S25), and either:

   1. it knows no `SnapRecord` that is usable (§19.3) with both `τ_S − entry_clock < 604,800,000` (seven days) and `S − info.seq < 1,000` (1,000 entries, SYNC.md §4.7), and none that is still waiting for verdicts (no verdict at all) with `τ_S − entry_clock < 172,800,000` (two days); or

   (Condition 2 and the rest of §19.2 are unchanged.)

2. Reword ENG-SNAP-001 (full text):

   - **ENG-SNAP-001** (engine) The engine MUST decide that a snapshot is due at an aligned seq exactly as §19.2 states: the 24-hour limit and one of its two conditions.

3. §19.2: add after ENG-SNAP-002:

   - **ENG-SNAP-006** (engine) The engine MUST NOT decide a snapshot due, or write one, at an aligned `S` while an own `snap` record is in its outbox or its newest own accepted `snap` entry has an `entry_clock` above `τ_S − 86,400,000`.

4. §34 Limits: add after the row `| verdict grace | 172,800,000 | ms of log clock | ENG-SNAP-001 |`:

   `| snapshot entry trigger | 1,000 | entries behind the aligned seq | ENG-SNAP-001 |`
   `| snapshots per device | 1 | per 86,400,000 ms of log clock | ENG-SNAP-006 |`

5. §38, item 14: replace the whole item with:

   14. **The snapshot duty is ranked by latest beat** with an hour per rank (§19.2, §29.2). A snapshot is due when the newest usable one is a week old by the log clock or 1,000 entries behind, and a device writes at most one per 24 hours of log clock, so an import produces no burst (ruling S25).

6. §1, the fourth bullet: replace `- the rulings S5 to S8, S10, S13 to S15 of `docs/sync/status/foundation.md`, and decision D6 (the traits live in this crate).` with `- the rulings S5 to S8, S10, S13 to S15, S25 and S27 of `docs/sync/status/foundation.md`, and decision D6 (the traits live in this crate).`

### R1-SEAL-ENGINE-20 (seal-engine-11)

The file kinds are listed in `seal.md` §19.2 (R1-SEAL-RELAY-10), kind 1 being the log segments; the engine names them and stops requesting kinds 9 and 10.

1. §7, the first paragraph: replace `(seal file kinds 1 to 8, plus kinds 9 and 10 requested in §36)` with `(the file kinds of `seal.md` §19.2)`.

2. §7, the table: replace the rows

   `| `LogRecord` | 1 `log.tdl` | appended per page |`
   `| identity | 7 | rewritten whole (sealer bytes) |`
   `| `NoticeOp` | 8 | appended; rewritten when compacted |`
   `| `Meta` | 9 (new) | rewritten whole |`
   `| `Marker` | 10 (new) | rewritten whole, or removed |`

   with

   `| `LogRecord` | 1 `log-<first seq>.tdl` (log segments) | appended per page |`
   `| identity | 7 `identity.tdk` | rewritten whole (sealer bytes) |`
   `| `NoticeOp` | 8 `notices.tdk` | appended; rewritten when compacted |`
   `| `Meta` | 9 `meta.tdk` | rewritten whole |`
   `| `Marker` | 10 `marker.tdk` | rewritten whole, or removed |`

3. §36, `device.md` block: delete the first bullet (`- Requires: two new sealed file kinds, 9 (`Meta`, one record) and 10 (`Marker`, one record), added to `seal.md` §19.2 through `device.md` (§7).`).

### R1-SEAL-ENGINE-21 (seal-engine-13)

§36, `seal.md` block, first bullet: replace the whole bullet with:

- Provides to seal: the state-hash composition of §18.3 (ENG-HASH-001; `seal.md` §25 requires it for §17.9, §20.5 and §21.6), and `taskdeck_sync::state_hash` (§37) as its one definition, which `seal.md` §26 `state_hash` and stage `recovery` reproduce byte for byte, tested against `engine-hash.json` (§35).

## device.md

### R1-SEAL-ENGINE-22 (seal-engine-5)

The bundle carries the roots of the snapshot's epoch and the one before it, never a later root (`seal.md` §17.8, `engine.md` ENG-OPS-006).

§18.3, the row of `Notify(PairRegistered { head, snapshot, added_seq })` (as R1-SEAL-RELAY-28 item 2 left it): replace `with the token kept above and the current and previous roots from the sealer adapter` with `with the token kept above and, from the sealer adapter, the roots of the snapshot's epoch and of the epoch before it when that is 2 or more, never a later root (`seal.md` §17.8, `engine.md` ENG-OPS-006)`.

## Rejected

None. Parts of proposals are not adopted, each recorded in its change:
- seal-engine-1: the new clause SEAL-MEM-028 and its §23 row (R1-SEAL-RELAY-2 already added the "Skip window" row against SEAL-MEM-011; rewording SEAL-RCPT-009 removes the contradiction with fewer clauses).
- seal-engine-2: applied only as its conservative reading (R1-SEAL-RELAY-8, R1-SEAL-ENGINE-13); the layout is for the lead (below).
- seal-engine-3: the `history` field on `SnapshotMaterial`, the sealer method `history_bytes` and the history in `Bootstrap::Join`; the sealer fills and keeps the history itself (R1-SEAL-RELAY-9, -28, R1-SEAL-ENGINE-15), so the trait does not change for it.
- seal-engine-6: the proposed id SEAL-BLOB-006, already assigned by R1-SEAL-RELAY-9; the clause is SEAL-BLOB-008.
- seal-engine-5: the proposed id ENG-OPS-004, already assigned by R1-SEAL-RELAY-25; the clause is ENG-OPS-006.
- seal-engine-8: the engine.md §36 note, since §17.2 already covers frames below `log_floor` once a device has joined.

## For the lead

1. **The delivery channel for ruling S24 (seal-engine-2; R1-SEAL-RELAY's "For the lead" item 1).** Both resolvers agree that wraps inside the reinstating entry cannot reach the device: that entry is sealed under the current epoch `E`, which the device lacks. Two designs are on the table:
   - (a) R1-SEAL-RELAY's: the recovery-signed `reinstate` admin operation carries the wraps in its `args` (`dev[16] ‖ u16 n ‖ n × wrap[96]`); the relay stores them on the target's row and returns them in that device's next `login` answer.
   - (b) this finding's: `DeviceReinstated` gains a field `missed` (an epoch, a commitment and a wrap per missed epoch), and an unsigned "reinstatement packet" carries the same wraps. `reinstate` signs the packet's hash (`args = dev[16] ‖ H(packet)[32]`), the relay returns the packet in the target's welcome, and a new sealer method `accept_reinstatement` takes it.

   Recommended: (a), with each wrap's epoch made explicit, as `args = dev[16] ‖ u32 first_epoch ‖ u16 n ‖ n × wrap[96]`. It covers the epochs from `first_epoch` (the one the revoking rotation set up, raised to the epoch in force at `purge_seq` if that is later) up to and including `E`. It needs no wrap of the reinstating entry's own new epoch, because that rotation's wraps already name the reinstated device. It also needs no `missed` field: the device checks each root, oldest first, against the commitment of the rotation that set that epoch, a rotation sealed under the epoch before it and so readable in order (SEAL-KEY-013). That is fewer bytes and one owner per rule. It touches seal.md §16.2 and §12.4, relay.md's `reinstate`, the `devices` row and `login`, and engine.md §24.4 together with a sealer method.

   Meanwhile no layout changes: a reinstated device pairs again (seal.md §27 item 18, engine.md §24.4, §36, §38 item 24, as R1-SEAL-ENGINE-13 writes them).
