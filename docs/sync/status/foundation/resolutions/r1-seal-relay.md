# Resolutions R1: seal.md against relay.md (16 findings)

Resolver 1 of 9. Inputs read: reviews.md §B, spec/README.md, foundation.md "Decisions" and "Rulings for the specs" (S1 to S31, with the settled S15), SYNC_PLAN.md §2 and §3, and seal.md and relay.md in full, plus the engine.md and device.md sections the changes touch. No earlier resolver file exists, so every new clause id continues from the highest id of its topic in the spec as it stands.

New clause ids assigned here (later resolvers continue after them):
- seal.md: SEAL-KEY-017, SEAL-PAIR-015, SEAL-BLOB-006, SEAL-BLOB-007.
- relay.md: RELAY-COMPACT-007, RELAY-RECKEY-002, RELAY-RTOKEN-004.
- engine.md: ENG-OPS-004, ENG-OPS-005.

Notation. "Replace A with B" means the exact text A, as it stands in the spec now, becomes B. Clause texts are given in full. A table row is given in full, in the table's own column order.

## seal.md

### R1-SEAL-RELAY-1 (seal-relay-1)

A host is capped at 255 bytes in all, `:port` included, so that it fits every `vb8` that carries it (§13.2 `home_host`, §16.2 `fence`, relay.md's welcome and `state` messages). The relay origin stays at most 263 bytes (`https://` plus 255). No layout changes.

1. §5.4, second bullet. Replace the whole bullet with:

   - A **host** in a Fence or a bundle is the host part alone, with an optional `:port`, at most 255 bytes in all: one to 253 bytes of lowercase ASCII letters, digits, `-` and `.`, forming dot-separated labels of 1 to 63 bytes that neither start nor end with `-`, then optionally `:` and a decimal port from 1 to 65535 without leading zeros. The full regular expression is `^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*(:[1-9][0-9]{0,4})?$`, with the port at most 65535, the part before `:` at most 253 bytes, and the whole value at most 255 bytes, so that it fits the one-byte length of `vb8` (§13.2, §16.2).

2. Reword SEAL-ENC-006 (full text):

   - **SEAL-ENC-006** (seal) A host field (§12.7, §17.8) MUST match the regular expression of §5.4, with at most 253 bytes before any `:`, a port of at most 65535 and at most 255 bytes in all; an encoder MUST refuse to produce, and a parser MUST reject, any other value.

3. §23 Limits: replace the row `| host | 253 | bytes, plus `:port` | SEAL-ENC-006 |` with:

   `| host | 255 | bytes in all, `:port` included (at most 253 before it) | SEAL-ENC-006 |`

   The row `| relay origin | 263 | bytes | SEAL-ENC-005 |` stays.

### R1-SEAL-RELAY-2 (seal-relay-2)

Applies the settled ruling S15: the Skip floor is `F(s) = max(purge_seq, s − 16,384)`.

1. §13.6, the `Skip` row: replace its Preconditions cell with:

   `t < s` and `t ≥ max(W.purge_seq, s − 16,384)` (the Skip window, ruling S15); `t` not in `ctl_seqs` and not in `skips` (`seal/ctl_target`); the frame at `t` is supplied and `H(frame) = frame_hash` (`seal/ctl_target`; `NeedFrames` when not supplied, §14.6); when `A` is a device, the target frame's `dev` differs from `A` (`seal/ctl_author`)

   (Author and Effects cells unchanged.)

2. §14.6: append this sentence to the paragraph (before SEAL-RCPT-009):

   A target below `max(purge_seq, s − 16,384)`, in `ctl_seqs` or already in `skips` refuses the entry (`seal/ctl_target`) before any frame is asked for, so whether a device holds a frame never changes the verdict (ruling S15); every frame a Skip may still name is kept on the relay (`relay.md` RELAY-COMPACT-007).

3. §23 Limits: add, after the row `| Skip targets recorded | 65,535 | entries | SEAL-MEM-002 |`:

   `| Skip window | 16,384 | seqs below the Skip entry's own seq | SEAL-MEM-011 |`

4. §27, open question 12: replace the whole item with:

   12. **Skip needs the target frame** (§14.6). The target is always at or above `F(s) = max(purge_seq, s − 16,384)` (ruling S15), and the relay never compacts the newest 16,384 seqs (`relay.md` RELAY-COMPACT-007), so a device that lacks the frame fetches it from the relay. Only a frame lost with a relay that no longer answers can be missing; the engine then reports an integrity problem without evidence (`engine.md` §17.2). Decided here.

### R1-SEAL-RELAY-3 (seal-relay-3, seal-relay-5, seal-relay-6, seal-relay-7, seal-relay-8, seal-relay-16)

§16.2: the "Sent with" column of the registry records the meanings `relay.md` gives the operations. No byte of any `args` layout changes.

1. Replace the registry table (the one under "**Registry of operations and their `args`.**") with this table; the `op`, `args` layout and Width columns are unchanged:

| `op` | `args` layout | Width | Sent with |
|---|---|---|---|
| `register` | `dev[16] ‖ sign_pk[32] ‖ frame_hash[32]` | 80 | frame mode: the `DeviceAdded` frame appended in the same call, `frame_hash` its hash; seq mode (repairing the relay's rows after a restore tail, `relay.md` §12.9): the entry already stored at the `u64 seq` carried in the unsigned `extra`, `frame_hash` its hash |
| `revoke` | `dev[16]` | 16 | before the `R`-shape entry that revokes it; or alone, for a device that `register` added at the relay but whose `DeviceAdded` clients refused (`engine.md` §28.4) |
| `reinstate` | `dev[16]` | 16 | before the `R`-shape entry that reinstates it |
| `recovery_register` | `dev_R[16] ‖ sign_pk[32] ‖ frame_hash[32]` | 80 | after the `K`-shape entry that installs the new recovery key has been appended as an ordinary entry, sequenced, and classified `Accept` by the sender: the unsigned `extra` carries that entry's `u64 seq`, `frame_hash` is its hash; never with the frame in the same call (`relay.md` §12.12) |
| `fence` | `vb8(host) ‖ door[5] ‖ u32 home_gen` | 10 + n | to the old relay, with the `Fence` frame in the same call (in the unsigned `extra`) or after the `Fence` entry is sequenced |
| `seed` | `u32 home_gen ‖ u64 first_seq ‖ chain_before[32] ‖ u64 last_seq ‖ chain_last[32] ‖ blob_id[16] ‖ u16 n ‖ n × (dev[16] ‖ sign_pk[32] ‖ u8 role ‖ u8 status ‖ u64 last_ctr)` | 102 + 58n | to the new relay, with its create secret. `first_seq` is the first tail seq: `min(S + 1, max(1, purge_seq, last_seq + 1 − 16,384))`, with `S` the seeded snapshot's seq and `purge_seq` that of `M_{last_seq}`, whenever the seeding device holds those frames (`engine.md` §28.3), so that the new relay holds every Skip target §13.6 permits after the move; `chain_before = C_{first_seq − 1}`; `last_seq` is the last tail seq (`first_seq − 1` for no tail) and `chain_last = C_{last_seq}`; the entries are the devices and recovery identities of `M_{last_seq}` with their role and status there, and each `last_ctr` is that author's counter at `first_seq − 1`; `blob_id` is the seeded snapshot's, or 16 zero bytes, and is informational |
| `compact` | `u64 below_seq ‖ u64 snap_entry_seq ‖ u32 n ‖ n × u64 keep_seq` | 20 + 8n | `keep_seq` ascending: the control entries below `below_seq` that stay; `below_seq` at most `head_seq − 16,384` (`relay.md` §12.8) |
| `restore_tail` | `u64 first_seq ‖ u32 n ‖ n × frame_hash[32]` | 12 + 32n | the frames re-uploaded at their original seqs; `chain_before` and the frames travel unsigned in `extra` (`relay.md` §12.9) |
| `purge` | `u64 before_seq ‖ u32 epoch` | 12 | by a device, after the rotation that carries `purge_before` has been sequenced: `before_seq` is that `purge_before`, `epoch` the rotation's new epoch; `extra` is empty |
| `mint_read_token` | `dev[16] ‖ u32 epoch` | 20 | `dev` is the signer: a device that writes backups, or `dev_R` for the recovery tool (ruling S21). `epoch` is the writer's current membership epoch; the recovery tool sends 1 and, when refused with `relay/stale_epoch`, the `read_epoch` that refusal reports (`relay.md` §4.2, §12.11) |
| `revoke_read_token` | `dev[16]` | 16 | |

2. §16.2, the paragraph after the table: replace `and for `register` and `recovery_register` that `frame_hash` is the hash of the frame in the same call.` with `and for `register` and `recovery_register` that `frame_hash` is the hash of the frame carried in the same call or stored at the seq named in `extra`.`

### R1-SEAL-RELAY-4 (seal-relay-3)

1. §13.11, last paragraph: replace `A recovery identity is new from the entry that installs it: its frames are accepted from the next seq, with counter 1. The relay learns its key from the admin operation `recovery_register` (§16.2).` with:

   A recovery identity is new from the entry that installs it: its frames are accepted from the next seq, with counter 1. The relay learns its key from the admin operation `recovery_register` (§16.2), which the entry's sender sends only after it has classified that entry `Accept`, because a `K`-shape entry can be refused everywhere (a lost rotation race, a confirm after a cancel) and the relay cannot see it. Until then the relay still accepts the old recovery identity, whose frames clients refuse from the installing entry on (`seal/revoked_author`).

2. §25, `relay.md` block, fourth bullet: replace `- Requires: `register` appends the `DeviceAdded` frame whose hash its arguments name and returns the new device's first token; `recovery_register` does the same for the new recovery identity; the relay registers neither key without the frame.` with:

   - Requires: `register` in frame mode appends the `DeviceAdded` frame whose hash its arguments name and returns the new device's first token, and in seq mode names such an entry already stored; `recovery_register` names, by seq, a stored `K`-shape entry its sender has classified `Accept`, and never appends a frame (`relay.md` §12.12); the relay registers neither key without such a frame.

### R1-SEAL-RELAY-5 (seal-relay-4)

Narrows the relay's share of the Genesis checks to what a keyless relay can verify (lead's note under S23). Resolved with fewer clauses than proposed: no SEAL-MEM-028 is added, because `relay.md` §10.2 checks 2 to 5 (RELAY-CREATE-001) already state the relay's checks and SEAL-KEY-005, SEAL-KEY-006 and SEAL-GEN-010 give the relay its seal-owned shares.

1. Reword SEAL-MEM-004, owners only (full text):

   - **SEAL-MEM-004** (seal) The entry at seq 1 MUST be accepted only if its frame's `dev` is `dev_R` computed from the anchor's `rsign_pk`, its `ctr` is 1, its signature verifies under `rsign_pk`, `board_id` equals `H(ctx("board") ‖ rsign_pk ‖ salt)[0..16]`, its epoch is 1, it opens under `K_entry,1` from the anchor's `R_1`, `based_on_seq` is 0 with `based_on_chain = C_0`, and its only item is a Genesis item whose `salt` and `recovery.sign_pk` equal the anchor's.

2. §6.2: replace SEAL-KEY-005 with these two clauses (SEAL-KEY-005 keeps its id; SEAL-KEY-017 is new and follows it directly):

   - **SEAL-KEY-005** (seal, relay, recovery) `board_id` MUST equal `H(ctx("board") ‖ rsign_pk ‖ salt)[0..16]` with the genesis recovery signing key and the genesis salt.
   - **SEAL-KEY-017** (seal, recovery) A receiver MUST refuse the entry at seq 1 with `seal/ctl_genesis`, and a backup reader MUST reject the history with `seal/backup_history`, when the Genesis item's `salt` and `recovery.sign_pk` do not give the frame's `board_id` by §6.2.

3. §25, `relay.md` block, second bullet: append this sentence at its end:

   These are the relay's whole share of the checks of seq 1 (`relay.md` §10.2): the epoch, the opening under `K_entry,1`, `based_on` and the Genesis item lie inside `body`, so SEAL-MEM-004 and SEAL-KEY-017 are the clients' alone.

### R1-SEAL-RELAY-6 (seal-relay-5)

§22 Error catalogue: replace the row `| `seal/pair_raced` | E's DeviceAdded was refused (a rotation raced it) | §17.9 | abort; pair again |` with:

`| `seal/pair_raced` | E's DeviceAdded was refused (a rotation raced it) | §17.9 | abort; pair again (E's engine first sends `revoke` for the new device, `engine.md` §28.4) |`

(The `revoke` row of §16.2 is changed by R1-SEAL-RELAY-3.)

### R1-SEAL-RELAY-7 (seal-relay-9)

Records the lead's note under S23: an identical repeat of a slot post is answered again.

§17.11, the limits table: replace the rows

`| tries at the relay | 5 per ticket; what counts as a try is `relay.md`'s |`
`| slots per ticket | one each of A, B, C, D; a second post of the same label is refused by the relay (`relay.md`) |`

with

`| tries at the relay | the fifth slot-B post burns the ticket, so at most 4 are examined; what counts as a try is `relay.md` §10.8's |`
`| slots per ticket | one each of A, B, C, D; an identical repeat of a stored slot is answered again and stores nothing; a post of a stored label with other bytes is refused (`relay.md` §10.8) |`

### R1-SEAL-RELAY-8 (seal-relay-10)

Conservative reading, pending the lead (see "For the lead", item 1): ruling S24 cannot be applied as the finding proposes, because a reinstating entry is sealed under an epoch the revoked device does not hold, so wraps inside it never reach that device. No layout changes here.

§27: add a new item after item 17:

18. **A device the recovery key reinstates cannot yet read what was sealed while it was revoked** (ruling S24 not applied). S24 has the reinstating entry carry the roots the device missed; but that entry is itself sealed under an epoch the device lacks, so any wrap inside it is unreadable to the device, and a channel outside the entry is needed (raised with the lead). Default until it is ruled: `DeviceReinstated` holds only `dev` (§12.4), and a reinstated device pairs again (`engine.md` §24.4).

### R1-SEAL-RELAY-9 (seal-relay-11)

Applies ruling S27: the snapshot a device bootstraps from carries the control history back to Genesis.

1. §20.4 Snapshot body: add a row at the end of the table, after `membership`:

   `| `history` | 4 + Σ | u32 count, then records | `n_hist`, then `n_hist` history records (§21.5), one for each seq in `M_S.ctl_seqs`, ascending, the first at seq 1 (ruling S27) |`

   and add after the table, before "The body is compressed with raw DEFLATE":

   The history lets a device that bootstraps from this snapshot (a pairing's, or a periodic one after compaction or purge) write backups whose history starts at seq 1 (SEAL-BKP-002), although it never received the control entries below `S`. It counts toward the inflated limit of 67,108,864 bytes.

2. §20.5, step 7: replace the whole step with:

   7. for a snapshot: the body parses as §20.4 with no trailing bytes, `S` and the state hash equal the expected values, the membership bytes parse, the history verifies by the projection steps of §21.6 (with the receiver's own `board_id` in place of the sheet's), and the history agrees with `membership` as §21.6 step 11 states (the same devices with the same keys and statuses, the same current recovery key, the same epochs and commitments, the same `ctl_seqs`) (`seal/blob_open`); the caller then recomputes the state hash (`engine.md`).

3. §20.5: add after SEAL-BLOB-005:

   - **SEAL-BLOB-006** (seal) A writer MUST put in a snapshot body's `history` exactly one history record (§21.5) for each seq in its membership's `ctl_seqs`, in seq order, starting at seq 1.
   - **SEAL-BLOB-007** (seal) A reader MUST reject with `seal/blob_open` a snapshot whose history fails a projection step of §21.6 or disagrees with the snapshot's membership as §20.5 step 7 states.

4. §17.8, the paragraph after the bundle table: replace `travel inside the snapshot, whose body holds the canonical membership bytes `M_h` (§20.4) and whose hash the bundle authenticates.` with `travel inside the snapshot, whose body holds the canonical membership bytes `M_h` and the control history back to seq 1 (§20.4, ruling S27), and whose hash the bundle authenticates.`

5. §23 Limits: add after the row `| inflated snapshot | 67,108,864 | bytes | SEAL-BLOB-003 |`:

   `| history records per snapshot | 1,048,576 | records | SEAL-BLOB-006 |`

6. §24 Test vectors, row `seal-blob.json`: replace its Inputs cell with `blob kind, epoch root, `S`, writer, body (a snapshot body with its history), entropy` and its Expected-outputs cell with `sealed bytes, `sha256`, parts; opening failures in the order of §20.5, including a history that fails the projection or disagrees with the membership`.

7. §25, `device.md` block, second bullet: replace `so backups can carry the full history (SEAL-BKP-002).` with `so backups can carry the full history (SEAL-BKP-002) and every snapshot it seals carries it (SEAL-BLOB-006).`

8. §26 Public API: replace

   ```rust
   pub struct SnapshotBody { pub seq: u64, pub chain: Chain, pub hash_version: u32, pub state_hash: [u8; 32],
                             pub board: Vec<u8>, pub membership: Vec<u8> }
   ```

   with

   ```rust
   pub struct SnapshotBody { pub seq: u64, pub chain: Chain, pub hash_version: u32, pub state_hash: [u8; 32],
                             pub board: Vec<u8>, pub membership: Vec<u8>, pub history: Vec<HistoryRecord> }
   ```

   (`open_snapshot` keeps its signature; it now also runs the history checks of §20.5 step 7.)

### R1-SEAL-RELAY-10 (seal-relay-12)

1. §19.2: replace the table and the sentence after it with:

| `kind` | File (SYNC.md §4.1, `device.md` §6.1) | Records |
|---|---|---|
| 1 | `log-<first seq>.tdl`: log segments, frames with seq and chain value | appended |
| 2 | `checkpoint.tdk` | one |
| 3 | `base-<seq>.tdk`: weekly base checkpoints | one |
| 4 | `hashes.tdk`: the state hash at every seq ≡ 0 mod 64 | appended |
| 5 | `outbox.tdo` | appended |
| 6 | `overlay.tdk`: the overlay | one |
| 7 | `identity.tdk`: the device's private keys and the epoch roots it holds | one |
| 8 | `notices.tdk`: notices and refusals | appended |
| 9 | `meta.tdk`: sync metadata | one |
| 10 | `marker.tdk`: a marker, present only while set | one |
| 11 | `history.tdk`: one history record (§21.5) per accepted control entry | appended |
| 12 | `device.tdk`: per-device settings | one |
| 13 | `picture.tdk`: the shared picture as this device shows it | one |
| 14 | `weather.tdk` | one |
| 15 | `draft.tdk`: an unsaved notes draft | one |
| 16 | `feed-<subscription id>.tdk`: one feed's cached body | one |
| 17 | `bootstrap.tdk`: the bootstrap material until the first checkpoint | one |
| 18 | `unsent.tdk`: entries kept during a re-pair | one |
| 19 | `own-picture.tdk`: the phone's own picture, never uploaded (ruling S29) | one |

   Kinds 20 to 255 are reserved; a kind is added only through a change to this table. The contents of every kind are `device.md`'s. A file of kind `k` is sealed under `K_file(k)` (§6.6).

2. §26 Public API: replace `pub enum FileKind { Log = 1, Checkpoint = 2, Base = 3, Hashes = 4, Outbox = 5, Overlay = 6, Identity = 7, Notices = 8 }` with:

   ```rust
   pub enum FileKind { Log = 1, Checkpoint = 2, Base = 3, Hashes = 4, Outbox = 5, Overlay = 6, Identity = 7, Notices = 8,
                       Meta = 9, Marker = 10, History = 11, Device = 12, Picture = 13, Weather = 14, Draft = 15,
                       Feed = 16, Bootstrap = 17, Unsent = 18, OwnPicture = 19 }
   ```

### R1-SEAL-RELAY-11 (seal-relay-13)

§21.1: replace `A nightly backup (`TaskDeck-<date>-<seq>.tdbk`) holds the state at a head;` with `A nightly backup (`TaskDeck-<date>-<seq>-<tag>.tdbk`, ruling S28; names and the writer tag are `device.md` §15.3's) holds the state at a head;`.

### R1-SEAL-RELAY-12 (seal-relay-14)

§20.2, the paragraph after the AD: replace `` `writer_dev` is the device that sealed it, the author of the entry that publishes its hash. `` with:

`writer_dev` is the device that sealed it: the author of the entry that publishes its hash (the `snap` entry, or the `core` command that references a picture); E for a pairing snapshot (§17.9 step 4); and for the genesis snapshot (`S = 1`), whose hash the recovery-authored Genesis entry publishes, the Genesis item's `device.dev`, the first device, which seals and uploads it. A reader takes `writer_dev` from the signed entry or the bundle, never from the relay's blob `author` (`relay.md` §10.7).

### R1-SEAL-RELAY-13 (seal-relay-15)

1. §17.3: replace SEAL-PAIR-001 (owners narrowed; full text):

   - **SEAL-PAIR-001** (seal) `ticket_key`, `tid` and `mac_key` MUST be derived from the decoded code and `board_id` exactly as §17.3 states.

2. §17.11: add after SEAL-PAIR-014 (the topic's next id; it states the one derivation the relay repeats):

   - **SEAL-PAIR-015** (seal, relay) A ticket id MUST be `H(ctx("pair-tid") ‖ ticket_key)[0..16]` for the `ticket_key` of §17.3, and a check of a ticket proof MUST compute exactly that value from the presented `ticket_key`.

   (The constant-time comparison stays SEAL-GEN-007's.)

3. §25, `relay.md` block, third bullet: replace `(SEAL-PAIR-001)` with `(SEAL-PAIR-015)`.

### R1-SEAL-RELAY-14 (seal-relay-16)

A purge is a desktop action (SYNC_PLAN.md §2) and the relay's `purge` takes only a device signer, so only a device may set `purge_before`.

1. §13.6, the `EpochRotated` row: replace its Preconditions cell with:

   `epoch = W.epoch + 1` (`seal/ctl_epoch`); `wraps` name exactly the current devices of `W` and `W`'s current recovery identity, once each, ascending (`seal/ctl_wraps`); `purge_before` is present only when `A` is a device (`seal/ctl_author`); if `purge_before` is present, `W.purge_seq < purge_before ≤ s` (`seal/ctl_target`)

2. §13.12, first paragraph: replace `It never enrols a device, never proposes or confirms a recovery-key change, never resets or upgrades rules, and never authors `cmd`, `beat` or `snap`.` with `It never enrols a device, never proposes or confirms a recovery-key change, never resets or upgrades rules, never sets `purge_before` (a purge is a device's action, `relay.md` §12.10), and never authors `cmd`, `beat` or `snap`.`

## relay.md

### R1-SEAL-RELAY-15 (seal-relay-1)

A host is at most 255 bytes, `:port` included (`seal.md` §5.4, SEAL-ENC-006).

1. §12.1, the per-operation table, row `fence`: replace the Body cap `262,533` with `262,529` (120 + 10 + 255 + 262,144). The row becomes:

   `| `fence` | 10 + host length | empty, or the `Fence` frame | 262,529 | T1, D | a current device or the current recovery identity | §12.6 |`

2. §22, `seal.md` block: replace the bullet that begins `- I assume the hosts carried in `vb8(host)`` (the whole bullet) with:

   - Requires: the hosts carried in `vb8(host)` (`seal.md` §12.7 and §16.2 `fence`, and the welcome and `state` messages here) are at most 255 bytes, `:port` included (`seal.md` §5.4, SEAL-ENC-006); the `fence` body cap of §12.1 (262,529) is computed for 255, and §5.4 parsing step 4 rejects a longer host with `relay/address`.

### R1-SEAL-RELAY-16 (seal-relay-2)

The relay enforces the compaction window of the settled ruling S15 itself.

1. §12.8, check 2: replace `2. With `lo = max(compact_floor, purge_floor)`: `lo < below_seq ≤ head_seq` (else `409 relay/target`).` with:

   2. With `lo = max(compact_floor, purge_floor)`: `lo < below_seq ≤ head_seq − 16,384` (else `409 relay/target`).

2. §12.8: replace the whole paragraph that begins `**Skip targets (ruling S15).**` with:

   **Skip targets (ruling S15).** A Skip at seq `s` may target only `t ≥ max(purge_seq, s − 16,384)` (`seal.md` §13.6). Check 2 keeps the newest 16,384 seqs out of every compaction (RELAY-COMPACT-007), so every target a later Skip may name stays at or above `compact_floor`; purge deletes only below `purge_floor`, which is `purge_seq`; and a `log` row at or above `purge_floor` is otherwise deleted only by the replacement of an unfinished seed (§12.7). A device that lacks a target frame `t` fetches it with `read` (`after = t − 1`, `limit = 1`; the entry returned has seq `t` exactly when the frame is held). On a relay seeded after a re-home, the seed's tail covers the window (§12.7).

3. §12.8: add after RELAY-COMPACT-006:

   - **RELAY-COMPACT-007** (relay) `compact` MUST refuse with `409 relay/target` a `below_seq` above `head_seq − 16,384`, so that no compaction deletes any of the newest 16,384 seqs.

4. §20 Limits: add after the row `| compaction keep list | 131,072 | seqs | RELAY-COMPACT-001 |`:

   `| compaction window | 16,384 | newest seqs never compacted | RELAY-COMPACT-007 |`

5. §22, `seal.md` block: delete the bullet `- I assume `seal.md` §13.6's Skip floor (`t ≥ purge_seq`) is what ruling S15 settles, or that the settled floor is computed from the log so that `engine.md` can bound compaction by it (§12.8 "Skip targets").` and put in its place:

   - Requires: the Skip floor `max(purge_seq, s − 16,384)` of `seal.md` §13.6 (ruling S15), which RELAY-COMPACT-007 keeps fetchable.

6. §22, `engine.md` block, the bullet that begins `- Requires, of the engine's duties:`: replace `the Skip floor of ruling S15 is computed from the log, and compaction sends `below_seq` no higher than that floor (or names every frame between the floor and `below_seq` as `keep_seq`) and no higher than the snapshot acknowledged by a device other than the snapshot's author,` with `compaction sends `below_seq` no higher than `head_seq − 16,384` (RELAY-COMPACT-007) and no higher than the snapshot acknowledged by a device other than the snapshot's author,`.

### R1-SEAL-RELAY-17 (seal-relay-3)

`recovery_register` loses its frame mode: the relay registers a new recovery identity only after the `K`-shape entry is known to be accepted. (The finding's body cap of 216 is not adopted: the fixed length is 120 + 80 + 8 = 208.)

1. §12.1, the per-operation table: replace the row `recovery_register` with:

   `| `recovery_register` | 80 | `u64 seq` (8) | 208 (fixed) | T1, D | the current recovery identity or a current device | §12.12 |`

2. §12.12: replace the whole section text (everything between the heading `### 12.12 `recovery_register`` and the end of RELAY-RECKEY-001) with:

   Registers a new recovery identity once the `K`-shape entry installing it has been sequenced and accepted (`seal.md` §13.11, §16.2), and retires the old one. The relay cannot see whether clients accept that entry (a lost rotation race, or a confirm after a cancel, refuses it everywhere), so it never registers the key in the call that appends the entry: the entry is appended as an ordinary frame (§9: by the recovery identity with the door or a token, or by the confirming device with its token), and its sender sends `recovery_register` only after it has classified the entry `Accept`. Until then the old recovery identity stays current here, although clients refuse its frames from the installing entry on.

   `args = dev_R[16] ‖ sign_pk[32] ‖ frame_hash[32]`; `extra` is `u64 seq`, the seq of the stored `K`-shape entry (8 bytes).

   Checks, in order: `dev_R = H(ctx("recovery-dev") ‖ board_id ‖ sign_pk)[0..16]` (else `400 relay/bad_args`); `dev_R` and `sign_pk` are in no row (else the repeat below, or `409 relay/duplicate_key`); fewer than 64 role-2 rows (else `409 relay/device_cap`); a `log` row at `seq` with `seq ≥ purge_floor` and `hash = frame_hash` exists (else `400 relay/frame_mismatch`).

   Effects, in one transaction: every role-2 row with `state = 1` gets `state = 2` (retired: its later frames are refused at P6); insert the new row (`role = 2`, `state = 1`, `gen = 0`, `last_ctr = 0`, `last_seq = 0`, counters 0, `added_at = now`); every `read_tokens` row `state = 2` (the entry rotates). Response (200): `{"seq": integer, "chain": hex32}`, the named entry's seq and chain. **Repeat:** a row for `dev_R` with `role = 2`, `state = 1` and the same `sign_pk`, with the named `log` row passing the last check, is answered 200 with that entry's seq and chain and no write other than the nonce deletion of A4.

   - **RELAY-RECKEY-001** (relay) `recovery_register` MUST check that `dev_R` derives from `sign_pk` and the board id (SEAL-KEY-006) and that the `log` row at the seq in `extra`, at or above `purge_floor`, has the hash `frame_hash`, and MUST retire the previous current recovery identity and insert the new one in one transaction.
   - **RELAY-RECKEY-002** (relay) `recovery_register` MUST NOT append a frame, and MUST refuse with `400 relay/bad_body` an `extra` that is not exactly 8 bytes.

3. §11.4, paragraph **Push.**: replace `(an append over HTTPS or a socket, `register`, `recovery_register`, `fence` with a frame, `restore_tail`)` with `(an append over HTTPS or a socket, `register` in frame mode, `fence` with a frame, `restore_tail`)`.

4. §5.2, second table: replace the row `| `revoke`, `reinstate`, `recovery_register` or `purge` (each precedes or carries a rotation, SYNC.md §5.2) | every row `state = 2` | every backup-read token is invalid |` with:

   `| `revoke`, `reinstate`, `recovery_register` or `purge` (each precedes, carries or follows a rotation, SYNC.md §5.2) | every row `state = 2` | every backup-read token is invalid |`

5. §16.1, the rows table: delete the row that begins `| `recovery_register` (frame mode) |`, and replace the row `| `recovery_register` (seq mode) | 3 + `r` (nonce, old and new recovery rows) | 8 + 64 |` with `| `recovery_register` | 3 + `r` (nonce, old and new recovery rows) | 8 + 64 |`. The row `| `recovery_register` repeat | 1 (the nonce) | 10 + 64 |` stays.

6. §22, `engine.md` block, the bullet that begins `- Requires, of the engine's duties:`: after `restore tail sends the seq-mode `register` and `recovery_register` and the `revoke` and `reinstate` operations of the restored control entries in log order (§12.9);` insert ` a recovery-key confirmation appends its `K`-shape entry as an ordinary entry and sends `recovery_register` only after classifying it `Accept` (§12.12);`.

(The `Stage recovery` bullet of §22 is replaced by R1-SEAL-RELAY-20.)

### R1-SEAL-RELAY-18 (seal-relay-4)

§22, `seal.md` block: replace the whole bullet that begins `- Requires: SEAL-MEM-004 names `relay` as an owner,` with:

- Requires: the relay's share of the checks of seq 1 is `board_id` from `rsign_pk` and `salt` (SEAL-KEY-005), `dev = dev_R` (SEAL-KEY-006), `ctr = 1` and the strict signature under `rsign_pk` (SEAL-GEN-010), all in §10.2 (RELAY-CREATE-001); SEAL-MEM-004 and SEAL-KEY-017 concern the encrypted body and are the clients' alone.

### R1-SEAL-RELAY-19 (seal-relay-5)

§12.4, first paragraph: replace `` `args = dev[16]`, the target. Sent before the `R`-shape entry that revokes it (`seal.md` §16.2), so the target's next request fails at once (SYNC.md §5.4). `` with:

`args = dev[16]`, the target. Sent before the `R`-shape entry that revokes it (`seal.md` §16.2), so the target's next request fails at once (SYNC.md §5.4); or alone, for a device that `register` added but whose `DeviceAdded` clients refused (a rotation raced the pairing, `engine.md` §28.4), so that its row stops counting as current and its token is refused. The relay cannot tell the two cases apart and treats them alike.

(RELAY-REVOKE-001 to RELAY-REVOKE-003 are unchanged.)

### R1-SEAL-RELAY-20 (seal-relay-6, seal-relay-3)

The recovery tool learns the relay's read epoch from the refusal (lead's note under S28). The finding's added `epoch` field in the `mint_read_token` response is not adopted: the caller knows the epoch it sent.

1. §4.2, the error-response table: add a row after the `head` row:

   `| `read_epoch` | integer | optional; present exactly for `relay/stale_epoch` | 1 to 2^32 − 1 | the board's `read_epoch` (§12.11), so that a recovery tool, which can read nothing before it mints, learns the epoch to mint for |`

2. §12.11, paragraph **`mint_read_token`**: replace `` `epoch ≥ read_epoch` (else `409 relay/stale_epoch`); `` with `` `epoch ≥ read_epoch` (else `409 relay/stale_epoch`, whose error body carries `read_epoch`, §4.2); ``.

3. §12.11: add after RELAY-RTOKEN-003:

   - **RELAY-RTOKEN-004** (relay) A `relay/stale_epoch` refusal MUST carry in its error body the field `read_epoch` set to the board's stored `read_epoch`.

4. §19 Error catalogue: replace the row `| `relay/stale_epoch` | a read token for an epoch below the relay's (409) | §12.11 | mint for the current epoch |` with:

   `| `relay/stale_epoch` | a read token for an epoch below the relay's (409, with `read_epoch`) | §12.11 | mint again with the `read_epoch` the refusal carries |`

5. §23 Public API: replace `  constructor(status, code, extra);      // extra: { until?, index? }` with `  constructor(status, code, extra);      // extra: { until?, index?, head?, read_epoch? }` (`head` was already in §4.2's table and missing here).

6. §22: replace the whole `**Stage `recovery`.**` paragraph with:

   **Stage `recovery`.** Requires: the recovery tool takes host, board id and door from the sheet's relay address (§5.4), obtains nonces and sends recovery-signed operations with the door (§12.1), mints its own backup-read token as `dev_R` to top up a backup (§12.11), asking with epoch 1 and then with the `read_epoch` a `relay/stale_epoch` refusal carries (§4.2), so that it never raises the relay's read epoch; sends `recovery_register` for a `direct` recovery-key change only after the `K`-shape entry it appended is sequenced and classified `Accept` (§12.12); and fences the old board with a `Fence` frame and the `fence` operation over the door (§12.6).

### R1-SEAL-RELAY-21 (seal-relay-7)

After a re-home the seeded tail covers the Skip window, so every Skip target a later Skip may name is on the new relay. The relay cannot check it (the arguments carry no `purge_seq`); it is the seeding device's duty. No byte changes.

1. §12.7, first paragraph: replace `Seeds an empty relay after a re-home: the device set, the newest acknowledged snapshot and up to 30 days of tail including other devices' frames, each signature checked (SYNC.md §6.7, V2-9).` with:

   Seeds an empty relay after a re-home: the device set, the newest acknowledged snapshot and the tail after it, including other devices' frames, each signature checked (SYNC.md §6.7, V2-9). The tail starts low enough that every Skip target `seal.md` §13.6 permits after the move is on the new relay: `first_seq` is `min(S + 1, max(1, purge_seq, last_seq + 1 − 16,384))`, `S` being the snapshot's seq, whenever the seeding device holds those frames (`seal.md` §16.2 `seed`, `engine.md` §28.3). The relay cannot check this, since the arguments carry no `purge_seq`.

2. §16.1: add this sentence after the rows table, before RELAY-ROWS-001:

   A seed tail is outside the append budget (§14.1); covering the Skip window, one re-home can store up to 16,384 frames this way, `2f + a` rows written once.

3. §22, `engine.md` block, the bullet that begins `- Requires, of the engine's duties:`: replace `seeding sends the device set of the membership at the end of the tail with each author's counter before the tail (§12.7),` with `seeding sends a tail from `first_seq = min(S + 1, max(1, purge_seq, last_seq + 1 − 16,384))` (`seal.md` §16.2 `seed`), the device set of the membership at the end of the tail, and each author's counter before the tail (§12.7),`.

### R1-SEAL-RELAY-22 (seal-relay-8, seal-relay-9)

`seal.md` §16.2 and §17.11 now record these meanings (R1-SEAL-RELAY-3, R1-SEAL-RELAY-7), so the assumptions go.

1. §22, `seal.md` block: delete the bullet that begins `- I assume `seal.md` accepts, without a change to its bytes, these meanings`.
2. §22, `seal.md` block: delete the bullet that begins `- I assume `seal.md` §17.11's "a second post of the same label is refused by the relay"`.
3. §24, open question 8: replace `the bytes of `seal.md` §16.2 are unchanged, and §22 asks `seal.md` to record the meanings.` with `the bytes of `seal.md` §16.2 are unchanged, and its "Sent with" column records these meanings. Since R1, `recovery_register` has the seq mode only (§12.12).`

### R1-SEAL-RELAY-23 (seal-relay-15)

1. §22, `seal.md` block, first bullet: replace `the ticket id (SEAL-PAIR-001)` with `the ticket id (SEAL-PAIR-015)`.
2. §22, `seal.md` block, second bullet: replace `SEAL-PAIR-001 by §6.2 step 7.` with `SEAL-PAIR-015 by §6.2 step 7.`

## engine.md

### R1-SEAL-RELAY-24 (seal-relay-3)

A recovery-key confirmation is appended as an ordinary entry; `recovery_register` follows in seq mode once the entry is accepted (`relay.md` §12.12).

1. §7.4 Outbox records (the record table), row `via`: replace `` `Append`, `Register { dev, sign_pk }`, `RecoveryRegister { dev_r, sign_pk }`, `Fence { host, door, home_gen }` (§11.7) `` with `` `Append`, `Register { dev, sign_pk }`, `Fence { host, door, home_gen }` (§11.7) ``.

2. §11.7, `enum AdminRequest`: delete the line `    RecoveryRegister { frame: Vec<u8>, dev_r: Dev, sign_pk: Hash32 },        // relay.md §12.12, frame mode`, and replace the line `    RecoveryRegisterSeq { seq: u64, dev_r: Dev, sign_pk: Hash32, frame_hash: Hash32 },` with `    RecoveryRegisterSeq { seq: u64, dev_r: Dev, sign_pk: Hash32, frame_hash: Hash32 }, // relay.md §12.12, the only mode`.

3. §11.7, the paragraph after the code: replace `A `Register`, `RecoveryRegister` or `Fence` with a frame acknowledges that frame on success` with `A `Register` or `Fence` with a frame acknowledges that frame on success`.

4. §26.1, row `confirm`, Engine cell: replace `sent with `RecoveryRegister` (§11.7);` with `appended as an ordinary entry (`via: Append`); once this device classifies it `Accept`, it sends `RecoveryRegisterSeq { seq, dev_r, sign_pk, frame_hash }` for it (`relay.md` §12.12), again after a lost answer until it is answered, and sends none for a refused entry;`.

5. §28.6: replace `a `K`-shape entry is sent with `RecoveryRegister`, which is stage 2 of the operation.` with `the `K`-shape entry is appended as an ordinary entry (stage 2), and `RecoveryRegisterSeq` for it, once this device has classified it `Accept`, is stage 3.`

6. §28.8 (after ENG-OPS-003): add

   - **ENG-OPS-005** (engine) The engine MUST send `recovery_register` for a recovery-key confirmation only as `RecoveryRegisterSeq`, only after it has classified the `K`-shape entry `Accept`, and MUST NOT send it for a refused entry.

7. Public API (§37): replace

   ```rust
   pub enum Via { Append, Register { dev: Dev, sign_pk: Hash32 }, RecoveryRegister { dev_r: Dev, sign_pk: Hash32 },
                  Fence { host: String, door: [u8; 5], home_gen: u32 } }
   ```

   with

   ```rust
   pub enum Via { Append, Register { dev: Dev, sign_pk: Hash32 },
                  Fence { host: String, door: [u8; 5], home_gen: u32 } }
   ```

### R1-SEAL-RELAY-25 (seal-relay-5)

1. §28.4, step 4: replace the whole step with:

   4. A refusal of the entry (a rotation raced it, `seal/stale_epoch`; a device cap) first sends `Admin { Revoke { dev } }` for the new device, so that the relay drops the row and token `register` created for it (`relay.md` §12.4); a `409 relay/target` answer (already revoked) counts as done. Then it answers `Notify(PairFailed { ticket, code })`, and the user pairs again (`seal.md` `seal/pair_raced`).

2. §28.4: add after ENG-OPS-001:

   - **ENG-OPS-004** (engine) When the `DeviceAdded` of a pairing that it sent with `register` is refused, the engine MUST send the `revoke` admin operation for that device before it reports the pairing failed.

### R1-SEAL-RELAY-26 (seal-relay-7)

1. §28.3, stage 3: replace the whole stage with:

   3. `Admin { Seed { … } }` on the new relay: `S` = the newest usable snapshot this device acknowledged, or wrote and another device acknowledged; `first_seq = min(S + 1, max(1, M_L.purge_seq, L + 1 − 16,384))`, so that every Skip target §17.4 permits after the move is on the new relay (`seal.md` §16.2 `seed`); if this device does not hold every frame from that `first_seq` to `L` (it joined after `L + 1 − 16,384`), `first_seq` is instead the lowest `x + 1 ≤ S + 1` such that it holds every frame from `x + 1` to `L` and a base at or below `x`, and targets below it are reached only as §17.2 states; `last_seq = L`; `chain_before = C_{first_seq − 1}`; `chain_last = C_L`; `blob_id` = that snapshot's; the devices are those of `M_L`, each with its role and status there and its `last_ctr` in `M_{first_seq − 1}` (rebuilt by replay from a base at or below `first_seq − 1`), the revoked ones only when they author a frame at or after `first_seq` (`relay.md` §12.7).

2. §28.3, stage 4: replace `with the frames `S + 1` to `L` from its log,` with `with the frames `first_seq` to `L` from its log,`.

3. Reword ENG-MOVE-003 (full text):

   - **ENG-MOVE-003** (engine) The moving device MUST seed the new relay with the device set of `M_L`, each author's counter at `first_seq − 1`, and the frames from `first_seq` to its log head, each frame unchanged, with `first_seq` computed as §28.3 stage 3 states.

## device.md

### R1-SEAL-RELAY-27 (seal-relay-6)

1. §15.8, step 1: replace the whole step with:

   1. Try the backup's read token on `hello`. When it is refused (`401`, `403`), mint a token for the recovery identity (ruling S21): `admin-nonce` with the door and `signer = dev_R`, then `mint_read_token { dev: dev_R, epoch: 1 }` signed by the recovery key with the door; on `409 relay/stale_epoch` it sends it once more, with a fresh nonce and `epoch` = the `read_epoch` that refusal carries (`relay.md` §4.2, §12.11). It never mints above the relay's `read_epoch`, so it revokes no device's backup-read token.

2. §26, open question 7: replace the whole item with:

   7. **The recovery tool's read-token epoch.** A recovery identity cannot read before it mints, so it asks with epoch 1 and, when refused, with the `read_epoch` the relay's `relay/stale_epoch` refusal carries (`relay.md` §4.2; the lead's note under ruling S28). Decided there.

### R1-SEAL-RELAY-28 (seal-relay-11)

Ruling S27: the history travels in the bootstrap snapshot (`seal.md` §20.4); the optional bundle history blob is dropped.

1. §15.7: replace the whole paragraph under the heading with:

   Every device holds every control entry from seq 1 in `history.tdk` (`history_complete = true`). A device that created the board, or restored it, has them from the start. A device that bootstraps from a snapshot (a pairing's, or a rejoin's) takes the records up to the snapshot's seq from the snapshot's `history` (`seal.md` §20.4, ruling S27), which `seal.md` §20.5 has verified against the snapshot's membership, and records every later accepted control entry itself (§8.3). The sealer adapter fills a snapshot's `history` from `history.tdk` (the records with seq at most the snapshot's) whenever it seals one; when it opens one for a bootstrap, it hands the records to the store, which writes them as `history.tdk`, when it does not already hold them, before it persists the bootstrap.

2. §18.3, the row of `Notify(PairRegistered { head, snapshot, added_seq })`: delete `; when `history_complete` holds, the history blob of §15.7 is uploaded first and named in the bundle` (the cell then ends at `from the sealer adapter`).

3. §18.3, paragraph **Persisting the join**: replace `` `history_complete` true only when the bundle's history blob verified `` with `` `history_complete: true` ``, and replace `` and `history.tdk` from the history blob when there is one; `` with `` and `history.tdk` from the bootstrap snapshot's history (`seal.md` §20.4); ``.

4. §24, `seal.md` block: delete the bullet that begins `- Requires: the pairing bundle (`seal.md` §17.8) gains an optional field `history``.

5. §26, open question 6: replace the whole item with:

   6. **A device that joined by pairing receives the control history** in its bootstrap snapshot (ruling S27, `seal.md` §20.4), so any desktop can write backups however it joined (SYNC.md §6.6). Decided there.

### R1-SEAL-RELAY-29 (seal-relay-12)

1. §6.1: replace the sentence `Kinds 9 to 18 are added to `seal.md` §19.2 by this spec (§24).` with:

   Kinds 9 to 18 are this spec's and are listed in `seal.md` §19.2. Kind 19 (`own-picture.tdk`, one record) is reserved there for the phone's own picture (ruling S29); its row in this table, its line in the layout above and its plaintext come with the facade calls `set_own_picture` and `clear_own_picture` (`bridge.md` §20).

2. §24, `seal.md` block, first bullet: replace the whole bullet with:

   - Requires: the file kinds 1 to 18 of `seal.md` §19.2, kind 1 being the segments `log-<first seq>.tdl` (§6.1), and kind 19 for the phone's own picture (ruling S29), with the matching `FileKind` variants of `seal.md` §26. Kinds 9 and 10 are `engine.md` §36's request; the others are this spec's.

## Rejected

None. Parts of three proposals are not adopted, each recorded in its change: the new clause SEAL-MEM-028 of seal-relay-4 (RELAY-CREATE-001 and SEAL-KEY-005, SEAL-KEY-006 already state the relay's share, so one owner per rule is kept with fewer clauses); the `epoch` field added to the `mint_read_token` response in seal-relay-6 (the caller knows the epoch it sent); the body cap of 216 in seal-relay-3 (the envelope gives 120 + 80 + 8 = 208). seal-relay-10 is answered with its conservative reading because it needs a ruling (below).

## For the lead

1. **S24 has no channel (seal-relay-10).** A `DeviceReinstated` is inside an entry sealed under the current epoch `E`, which the revoked device lacks. So wraps inside it never reach that device, and neither does the root of the new epoch in the same entry. Every design needs a path outside the entry ciphertext. Recommended: the `reinstate` admin operation carries the wraps, as `args = dev[16] ‖ u16 n ‖ n × wrap[96]`, signed by the recovery identity. They cover every epoch from the one the revoking entry set up to and including the reinstating entry's new epoch. The relay stores them in the target's `devices` row (a new column) and returns them in that device's next `login` answer, as a field `wraps`. The device opens them with `unwrap_root`. It verifies them one at a time, oldest first: the revoking entry is sealed under an epoch it holds, so it opens that entry, takes the next commitment from it, checks the next root against that commitment, and so on, finishing at the reinstating entry. This changes relay bytes (the `reinstate` args, a column, the login answer) and seal's registry. Meanwhile: no change to layouts; seal.md §27 gains item 18 (R1-SEAL-RELAY-8), and a reinstated device pairs again, as `engine.md` §24.4 already says.
2. **The seeded tail can be longer than SYNC.md §6.7's "up to 30 days" (seal-relay-7).** To keep S15's guarantee after a re-home, the seed starts at `min(S + 1, max(1, purge_seq, L + 1 − 16,384))`. On a quiet board that can reach back further than 30 days. Recommended: accept it as a small change to SYNC.md §6.7 and note it in `spec/CHANGES.md` (the cost is at most 16,384 frames, written once). Written as the change (R1-SEAL-RELAY-3, -21, -26).
3. **The plaintext and facade of file kind 19 (seal-relay-12).** seal.md numbers the phone's own picture 19 (`own-picture.tdk`, one record). Its device.md table row, layout line and plaintext are left to the device–bridge resolver, together with `set_own_picture` and `clear_own_picture` (S29). Recommended: that resolver adds them to device.md §6.1 and §6.4. Meanwhile device.md §6.1 says so (R1-SEAL-RELAY-29).
4. **How the history reaches the engine's snapshot material (seal-relay-11).** `engine.md`'s `SnapshotMaterial` has no `history` field. The resolution keeps the engine out of it: device.md's sealer adapter fills `SnapshotBody.history` from `history.tdk` when it seals a snapshot, and hands the records to the store when it opens one (R1-SEAL-RELAY-28). Recommended: the seal–engine resolver confirms this, or adds a `history: Vec<u8>` to `SnapshotMaterial`. The history also counts toward the 64 MiB inflated cap; with S18's 60 MiB board cap it has about 4 MiB, far more than a board's control history needs.
5. **An orphan `revoke` invalidates every backup-read token (seal-relay-5).** The `revoke` that removes an orphaned pairing registration also marks every `read_tokens` row revoked (RELAY-REVOKE-003). When a rotation caused the refusal, writers mint again at the new epoch anyway. When a device cap caused it, writers keep an invalid token until the next epoch, and the recovery tool falls back to its own token (S21). Recommended: accept this. The alternative is a relay `revoke` variant that leaves read tokens alone, which costs bytes. Written as accepted.
