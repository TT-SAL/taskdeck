# Resolutions R1: engine.md against relay.md (19 findings)

Resolver 4 of 9. Inputs read: reviews.md §B, spec/README.md, foundation.md "Decisions" and "Rulings for the specs" (S1 to S31, with the settled S15), SYNC_PLAN.md §2 and §3, the earlier resolution files `r1-seal-relay.md`, `r1-seal-engine.md` and `r1-core-engine.md` in full, engine.md §1 to §13, §17 to §21, §24, §26 to §28, §31, §34, §36 to §38, relay.md §4, §5, §10.5 to §10.7, §11.3, §11.4, §12, §13, §16, §19 to §24, the seal.md §13.1 and §16.2 passages and device.md §10.5.

Built on the earlier files (settled; not repeated here):
- engine-relay-4 is applied by R1-SEAL-RELAY-3, -21 and -26 (the seed's `first_seq = min(S + 1, max(1, purge_seq, L + 1 − 16,384))`, ENG-MOVE-003 reworded, "up to 30 days" gone from relay.md §12.7).
- engine-relay-5 is applied by R1-SEAL-RELAY-2 and -16 (RELAY-COMPACT-007, check 2, the "Skip targets" paragraph, the §20 row, both §22 bullets) and R1-SEAL-ENGINE-10, -11.
- engine-relay-9 is applied on the engine's side by R1-SEAL-ENGINE-19 (ruling S25); only relay.md §16.2 follows here.
- engine-relay-10 is answered by R1-SEAL-RELAY-9, -28 and R1-SEAL-ENGINE-15 (the history travels inside the snapshot body; the sealer fills and keeps it; `SnapshotMaterial` does not change); only a relay.md note follows here.
- engine-relay-11 is applied by R1-SEAL-RELAY-1, -15, R1-SEAL-ENGINE-16 and R1-CORE-ENGINE-21 (255 bytes, `engine/bad_host`, ENG-OPS-007); only a relay.md §20 row follows here.
- engine-relay-12 is applied by R1-SEAL-RELAY-20 and -27 (the error field is named `read_epoch`; the mint response does not echo it).
- engine-relay-19 is applied by R1-SEAL-ENGINE-20 and R1-CORE-ENGINE-22.
- engine-relay-1 and engine-relay-2 build on R1-SEAL-RELAY-24 (no `RecoveryRegister` frame mode; `RecoveryRegisterSeq` after `Accept`) and R1-SEAL-RELAY-26 (stage 3 of §28.3), whose stage 3 text is carried into the full stage list below.
- engine-relay-18 builds on the `compact` registry row as R1-SEAL-RELAY-3 and R1-SEAL-ENGINE-5 left it, and on relay.md §12.8 as R1-SEAL-RELAY-16 and R1-SEAL-ENGINE-10 left it.

New clause ids assigned here (later resolvers continue after them, and after those of the earlier files):
- engine.md: ENG-MOVE-005, ENG-MOVE-006, ENG-MOVE-007, ENG-EPOCH-004.

New error codes: none. New public types: `RecoveryView`, `PendingRecovery` (engine.md §6.2). Wire change: `compact` args gain `blob_id[16]` (seal.md, relay.md, engine.md; see "For the lead", item 2).

Notation. "Replace A with B" means the exact text A, as it stands in the spec after the changes of the three earlier files, becomes B. Clause texts are given in full. A table row is given in full, in the table's own column order.

## seal.md

### R1-ENGINE-RELAY-15 (engine-relay-18)

`compact` names the snapshot blob it relies on, so the relay protects exactly the blob the `snap` entry announces instead of choosing one by seq (an unannounced blob of the same author, left by `engine.md` §19.4 step 5, could otherwise become `compact_snap`). Matching edits: relay.md and engine.md, same id.

§16.2, the registry table: replace the row `compact` (as R1-SEAL-RELAY-3 and R1-SEAL-ENGINE-5 left it) with:

`| `compact` | `u64 below_seq ‖ u64 snap_entry_seq ‖ blob_id[16] ‖ u32 n ‖ n × u64 keep_seq` | 36 + 8n | `blob_id` is the blob the `snap` entry at `snap_entry_seq` announces (its `SnapInfo.blob`), which the relay records as `compact_snap`; `keep_seq` ascending: the control entries and the recorded Skip targets (§13.1 `skips`) below `below_seq` that stay (`engine.md` §20.2); `below_seq` at most `head_seq − 16,384` (`relay.md` §12.8) |`

## relay.md

### R1-ENGINE-RELAY-2 (engine-relay-2, engine-relay-3, engine-relay-7)

The relay side of the move (engine.md §28.3 below, same id). No byte changes.

1. §12.7, the paragraph `The seeding device then uploads the snapshot (`blob-begin`, purpose 1) and the other devices log in with the door when they follow (`seal.md` §15.4; `engine.md`).`: replace it with:

   Once the board is open, the seeding device uploads a snapshot it sealed itself (`blob-begin`, purpose 1, `seq` = the snapshot's `S`) and announces it with a `snap` entry. It never uploads a blob fetched from another relay: §10.7 records the uploader as the blob's `author`, and `seal.md` §20.2 binds the writer into the sealed bytes, so another writer's blob carried here could be neither opened from its `author` (a rejoin, `engine.md` §19.7) nor relied on by `compact` (§12.8 check 4). When the old relay did not sequence the move's `Fence`, the seeding device appends it here as an ordinary entry after the seed; a `fence` operation here would be refused with `409 relay/home_gen`, since the seed stored that `home_gen` (§12.6). The other devices log in with the door when they follow (`seal.md` §15.4; `engine.md`).

2. §22, `engine.md` block, the bullet that begins `- Requires, of the engine's duties:`: replace `the move appends the `Fence` with the `fence` operation, using the new relay's door (§10.9, §12.6);` with:

   the move appends the `Fence` with the `fence` operation on the old relay, using the new relay's door (§10.9, §12.6), and reads the old relay through that `Fence` before it seeds; when the old relay does not answer, or the moving device is under an evidence stop (it then fences the old relay without a frame), the `Fence` is an ordinary append on the new relay after the seed, never a `fence` there; the seeding device uploads only a snapshot it sealed itself (§12.7);

3. §22, the same bullet: replace `the device set of the membership at the end of the tail, and each author's counter before the tail (§12.7),` with `every device and recovery identity of the membership at the end of the tail, and each author's counter before the tail (§12.7),`.

### R1-ENGINE-RELAY-4 (engine-relay-5)

Nothing further to apply. R1-SEAL-RELAY-16 already makes check 2 of §12.8 `lo < below_seq ≤ head_seq − 16,384`, adds RELAY-COMPACT-007 and the §20 row "compaction window", rewrites the "Skip targets (ruling S15)" paragraph without the keep-list alternative, and replaces both §22 bullets; R1-SEAL-ENGINE-10 adds the recorded Skip targets to the keep list.

### R1-ENGINE-RELAY-5 (engine-relay-6, engine-relay-16)

A restore request ends at each control entry, so that a frame of an author the entry registers never reaches the relay before that author's row; a lost purge is repaired like the other device rows; and relay.md stops restating who restores. Matching edits: engine.md, same id.

1. §12.9, first sentence: replace `After a plain relay rollback, the most advanced device re-uploads frames at their original seqs;` with `After a plain relay rollback, any device that holds more than the relay re-uploads frames at their original seqs (who and when: `engine.md` §21.3);`.

2. §12.9: replace the paragraph `Restore tail does not change any device row's role, state or key. A rollback that lost a registration, revocation, reinstatement or recovery-key change is repaired by the restoring device sending `register` or `recovery_register` in seq mode, `revoke` and `reinstate` for the restored control entries, in log order, between restore calls (`engine.md`).` with:

   Restore tail does not change any device row's role, state or key, nor any floor. A rollback that lost a registration, revocation, reinstatement, recovery-key change, fence or purge is repaired by the restoring device: after each restored control entry, and before its next `restore_tail`, it sends what that entry requires, in item order: `register` or `recovery_register` in seq mode, `revoke`, `reinstate` (recovery-signed), `fence` without a frame, or, for a restored rotation that carries `purge_before`, `purge` (`engine.md` §21.3). A `restore_tail` request that contains a control entry ends with it, so that a frame by an author that entry registers is never checked before the author's row exists (`relay/unknown_author`).

3. §22, `engine.md` block, the bullet that begins `- Requires, of the engine's duties:`: replace `restore tail sends the seq-mode `register` and `recovery_register` and the `revoke` and `reinstate` operations of the restored control entries in log order (§12.9);` with:

   restore tail ends each request at its first control entry and, before the next request, sends for that entry, when clients accepted it, the seq-mode `register` or `recovery_register`, `revoke`, `reinstate`, `fence` without a frame, or `purge` for a rotation that carries `purge_before` (§12.9);

   (The text R1-SEAL-RELAY-17 inserted after this phrase stays.)

### R1-ENGINE-RELAY-7 (engine-relay-9)

Ruling S25 restores the 1,000-entry trigger (R1-SEAL-ENGINE-19). The typical day has about 305 entries, so a snapshot falls due about every 3.3 days; the week trigger and the 24-hour cap do not bind. §16.2 is recomputed.

1. §16.2, the first paragraph: replace `one snapshot of 3 MB (3 parts) a week, which replaces an older one.` with `one snapshot of 3 MB (3 parts) every 3.3 days, which replaces an older one (the 1,000-entry trigger of ruling S25 at about 305 entries a day; the week trigger and the one-per-24-hours cap of `engine.md` §19.2 do not bind on this day).`

2. §16.2, the table: replace the row

   `| weekly snapshot (begin, 3 parts, one older snapshot deleted), per day | 4 ÷ 7 | 0.6 | 0.6 | 11 ÷ 7 = 1.6 | 2 |`

   with

   `| snapshot every 3.3 days (begin, 3 parts, one older snapshot deleted), per day | 4 ÷ 3.3 | 1.2 | 1.2 | 11 ÷ 3.3 = 3.3 | 4 |`

3. §16.2, the table: replace the row `| **total** | | **≈ 199** | **≈ 215** | **≈ 1,524** | **≈ 4,217** |` with `| **total** | | **≈ 199** | **≈ 215** | **≈ 1,525** | **≈ 4,219** |`, and the row `| **share** | | **0.20%** | **0.22%** | **1.52%** | **0.08%** |` with `| **share** | | **0.20%** | **0.22%** | **1.53%** | **0.08%** |`.

   (Sums: Worker requests 70 + 96 + 20 + 2 + 1.2 + 10 = 199.2; object requests 70 + 3.5 + 14.2 + 3.5 + 96 + 20 + 2 + 1.2 + 5 = 215.4; rows written 849 + 60 + 613 + 3.3 = 1,525.3; rows read 210 + 210 + 1,698 + 700 + 576 + 120 + 686 + 4 + 15 = 4,219. RELAY-ROWS-002's 2% holds.)

### R1-ENGINE-RELAY-8 (engine-relay-10)

Ruling S27 as the earlier resolutions applied it (the control history inside the snapshot body, filled and kept by the sealer). The relay changes no byte; it records why later devices need the snapshot's history. The finding's engine and seal changes (`history` in `SnapshotMaterial`, the runtime writing it at Join) are not adopted: they would undo R1-SEAL-ENGINE-15 (see "For the lead", item 4).

§13.3, the first paragraph: append at its end:

Control entries below `before_seq` are deleted by `purge` like every other row (§24 open question 13), so a device that bootstraps later receives them only in the control history its bootstrap snapshot carries (`seal.md` §20.4, ruling S27); the blob size limit of §10.7 covers that history.

### R1-ENGINE-RELAY-9 (engine-relay-11)

The host cap is applied by R1-SEAL-RELAY-1 and -15 (seal.md §5.4, SEAL-ENC-006, the `fence` body cap 262,529, the §22 statement; §5.4 parsing step 4 and §12.6's check both apply seal.md §5.4's host rule, which now includes the 255 bytes). Only the limit row is missing.

§20 Limits: add after the row `| relay address | host, `/`, 34 characters | | RELAY-ADDR-001 |`:

`| host | 255 | bytes, `:port` included (`seal.md` §5.4) | RELAY-ADDR-002, RELAY-ADMIN-002 |`

### R1-ENGINE-RELAY-10 (engine-relay-12)

Nothing further to apply. R1-SEAL-RELAY-20 adds the error field `read_epoch` (present exactly for `relay/stale_epoch`), RELAY-RTOKEN-004, the §19 row and the `RelayError` extra; R1-SEAL-RELAY-27 has the recovery tool retry once with it (device.md §15.8, open question 7). The finding's `epoch` field in the `mint_read_token` response was not adopted there (the caller knows the epoch it sent).

### R1-ENGINE-RELAY-15 (engine-relay-18)

Matching edits: seal.md and engine.md, same id.

1. §12.1, the per-operation table: replace the row `| `compact` | 20 + 8n | empty | 1,048,716 | T1 | a current device other than the snapshot's author | §12.8 |` with:

   `| `compact` | 36 + 8n | empty | 1,048,732 | T1 | a current device other than the snapshot's author | §12.8 |`

   (120 + 36 + 8 × 131,072 = 1,048,732.)

2. §12.8, the paragraph that defines `args` (as R1-SEAL-ENGINE-10 left it): replace it with:

   `args = u64 below_seq ‖ u64 snap_entry_seq ‖ blob_id[16] ‖ u32 n ‖ n × u64 keep_seq` (`n` from 0 to 131,072): delete entries with seq below `below_seq` except the `keep_seq`s (the control entries and recorded Skip targets below it, `engine.md` §20.2), relying on the snapshot blob `blob_id` that the `snap` entry at `snap_entry_seq` announces.

3. §12.8, check 4: replace `4. A complete blob of purpose 1 by author `A` with `below_seq ≤ seq < snap_entry_seq` exists; the chosen snapshot is the one with the largest `seq` (then the latest `at`, then the smallest `id`) (else `409 relay/no_snapshot`).` with:

   4. The blob `blob_id` exists, is complete, has purpose 1 and author `A`, and has `below_seq ≤ seq < snap_entry_seq`; it is the chosen snapshot (else `409 relay/no_snapshot`).

4. §13.4: replace `A compaction relies on one complete purpose-1 snapshot at `S ≥ below_seq` (§12.8).` with `A compaction relies on the complete purpose-1 snapshot `blob_id` at `S ≥ below_seq`, which the `snap` entry at `snap_entry_seq` announces (§12.8; the relay cannot read that entry, and the signer states it).`

(RELAY-COMPACT-001 still names seven checks; RELAY-COMPACT-004 is unchanged: `compact_snap` is now `blob_id`.)

## engine.md

### R1-ENGINE-RELAY-1 (engine-relay-1)

The membership view gains what the relay requests the engine composes need: every device's signing key, every recovery identity, and the keys of a pending recovery-key proposal. The view is engine-internal (no wire change).

1. §6.2, the code block: replace the whole block with:

   ```rust
   pub enum DeviceStatus { Current, Revoked }
   pub struct DeviceView { pub dev: Dev, pub sign_pk: Hash32, pub device_no: u16, pub status: DeviceStatus, pub name: String,
                           pub key_fp: String, pub added_seq: u64, pub added_by: Dev, pub bootstrap_seq: u64,
                           pub status_seq: u64, pub status_by: Dev, pub last_ctr: u64, pub beat_ms: i64,
                           pub rules: Vec<u32>, pub envelopes: Vec<(u8, u8)>, pub hash_versions: Vec<u32> }
   pub struct RecoveryView { pub dev_r: Dev, pub sign_pk: Hash32, pub hpke_pk: Hash32, pub since_seq: u64,
                             pub until_seq: u64, pub last_ctr: u64 }
   pub struct PendingRecovery { pub proposer: Dev, pub sign_pk: Hash32, pub hpke_pk: Hash32, pub key_fp: String,
                                pub seq: u64, pub start_ms: i64 }
   pub struct MembershipView {
       pub board_id: [u8; 16], pub rules: u32, pub epoch: u32, pub epoch_seq: u64, pub epoch_clock_ms: i64,
       pub clock_ms: i64, pub purge_seq: u64, pub home_gen: u32, pub home_seq: u64,
       pub home_host: String, pub home_door: [u8; 5], pub devices: Vec<DeviceView>,
       pub recovery_dev: Dev, pub recovery_fp: String, pub recovery: Vec<RecoveryView>,
       pub pending_recovery: Option<PendingRecovery>,
       pub pending_override: Option<(u32, Dev, u64)>,             // rules, voter, seq
       pub last_reset: Option<(u64, u64)>,                        // at_seq, target
       pub skips: Vec<(u64, Hash32)>, pub ctl_seqs: Vec<u64>,
   }
   ```

2. §6.2, the paragraph after the block: replace `Each field is the field of the same name of `seal.md` §13.1; `key_fp` and `recovery_fp` are `seal.md` §6.8's text. `devices` is ascending by `dev`.` with:

   Each field is the field of the same name of `seal.md` §13.1 (`dev_r` is `dev_R`; `recovery` is its `recovery` list, `pending_recovery` its `pending_recovery` with the new key's fingerprint added); `key_fp`, `recovery_fp` and `PendingRecovery.key_fp` are `seal.md` §6.8's text. `recovery_dev` and `recovery_fp` are those of the current entry of `recovery` (`until_seq = 0`). `devices` is ascending by `dev`, `recovery` ascending by `since_seq`.

3. §26.1, row `confirm`, Engine cell (as R1-SEAL-RELAY-24 left it): replace `(`RecoveryChange { Confirm }` with the pending keys;` with `(`RecoveryChange { Confirm }` with `pending_recovery.sign_pk` and `hpke_pk`, §6.2;`, and replace `once this device classifies it `Accept`, it sends `RecoveryRegisterSeq { seq, dev_r, sign_pk, frame_hash }` for it (`relay.md` §12.12)` with `once this device classifies it `Accept`, it sends `RecoveryRegisterSeq { seq, dev_r, sign_pk, frame_hash }` for it (`relay.md` §12.12), with `dev_r` and `sign_pk` those of the `recovery` entry the confirmation installed (§6.2)`.

4. §37 Public API: replace `SnapshotMaterial, SealedBlob, SealerError, Sealer, DeviceStatus, DeviceView, MembershipView};` with:

   ```rust
                   SnapshotMaterial, SealedBlob, SealerError, Sealer, DeviceStatus, DeviceView, RecoveryView,
                   PendingRecovery, MembershipView};
   ```

(The seed entries that use `sign_pk` and `recovery` are in R1-ENGINE-RELAY-2; the restore repairs in R1-ENGINE-RELAY-5.)

### R1-ENGINE-RELAY-2 (engine-relay-2, engine-relay-3, engine-relay-7)

The move, rewritten in one place: the Fence that the old relay sequences is carried into the seed (the mover reads through it first); a Fence the old relay did not sequence goes out as an ordinary append on the new relay; a mover under an evidence stop fences the old relay without a frame; the seed lists every recovery identity with its key; and the snapshot on the new relay is one the mover sealed itself. Matching edits: relay.md, same id. Stage 3's computation of `S` and `first_seq` is R1-SEAL-RELAY-26's, unchanged.

1. §28.3, **On the device that moves the board**: keep stage 1 and replace stages 2 to 7 (stages 3 and 4 as R1-SEAL-RELAY-26 left them) with:

   2. **Fence.** If the current relay answers and this device is under no evidence stop (§21.2): enqueue `CtlIntent::Fence { host, door: d', home_gen: M_K.home_gen + 1 }` with `via: Fence`; it is sent with `Admin { Fence { frame } }`; on success it is sequenced at `f`. The device then reads the old relay (§12.2; it is fenced, so `f` is its final head) until `L ≥ f`, classifying and applying as usual, and only then goes on to stage 3, so that the seeded tail ends at or after the `Fence`. If the current relay answers but this device is under an evidence stop, it cannot carry the old relay's entries from the stop's cut on: it fences the old relay without a frame, `Admin { Fence { host, door: d', home_gen: M_K.home_gen + 1, frame: None } }`, and goes on to stage 3 as for a relay that is gone. If the user says the old relay is gone, it goes on to stage 3 at once. In these last two cases no `Fence` record exists yet; it is enqueued in stage 5. The move's home generation `h` is `M_K.home_gen` once the old relay's `Fence` of this move is accepted (the first case), and `M_K.home_gen + 1` otherwise.
   3. **Seed.** `S` = the newest usable snapshot this device acknowledged, or wrote and another device acknowledged; `first_seq = min(S + 1, max(1, M_L.purge_seq, L + 1 − 16,384))`, so that every Skip target §17.4 permits after the move is on the new relay (`seal.md` §16.2 `seed`); if this device does not hold every frame from that `first_seq` to `L` (it joined after `L + 1 − 16,384`), `first_seq` is instead the lowest `x + 1 ≤ S + 1` such that it holds every frame from `x + 1` to `L` and a base at or below `x`, and targets below it are reached only as §17.2 states. First the device seals a fresh snapshot of its own state at `S`, with itself as writer: the material of §19.4 step 1 from its base at `S`, or from a replay to `S` from a base below it, whose state hash must equal its hash record at `S` (else the local fault `engine/local_fault`), then `Sealer::seal_snapshot(M_K, material)`. It never uploads another writer's blob, nor a blob fetched from another relay: the new relay records the uploader as a blob's `author` (`relay.md` §10.7), and the writer is bound into the sealed bytes (`seal.md` §20.2). Then `Admin { Seed { … } }` on the new relay: `home_gen = h`; `last_seq = L`; `chain_before = C_{first_seq − 1}`; `chain_last = C_L`; `blob_id` = the fresh blob's id; the entries (`SeedDevice`, ascending by `dev`) are every device of `M_L` (`role = 1`; `status` 1 when current, 2 when revoked) and every recovery identity of `M_L.recovery` (`role = 2`; `status` 1 for the current one, whose `until_seq` is 0, and 2 for a retired one), each with its `sign_pk` (§6.2) and with `last_ctr` its counter in `M_{first_seq − 1}` (0 for one added at or after `first_seq`), `M_{first_seq − 1}` being rebuilt by replay from a base at or below `first_seq − 1`; a revoked device or a retired recovery identity is listed only when it authors a frame at or after `first_seq` (`relay.md` §12.7). After a `Start` in this stage, the device seals and seeds again (a new seed replaces an unfinished one, `relay.md` §12.7).
   4. `Admin { SeedTail }` with the frames `first_seq` to `L` from its log, at most 256 frames and 1,048,576 bytes per request, resuming from the head of a `relay/not_next`.
   5. `meta.relay = (host, d', h)`; `Connect`. If the old relay did not sequence this move's `Fence` (the second and third cases of stage 2), the device now enqueues `CtlIntent::Fence { host, door: d', home_gen: h }` with `via: Append`, which the send loop sends as an ordinary `append` on the new relay. It is never sent as a `fence` admin operation there: the new relay was seeded with `home_gen = h` and refuses it with `409 relay/home_gen` (`relay.md` §12.6).
   6. Once the session on the new relay is `Live`: `Blob { Upload { purpose: 1, seq: S, blob } }` with the fresh blob of stage 3 (sealed again, with a new id, after a `Start`), and on success a `snap` record for it, as §19.4 step 4 states (§19.4 step 5 applies). Three failed uploads give the snapshot up, as for the snapshot duty (§19.2); the move does not fail for it.
   7. When its `Fence` is seen in its log at `f` (the old relay's seq in the first case of stage 2, the new relay's otherwise): `Notify(MoveCode { text: Sealer::move_code(f, H(Fence frame)) })`, shown until acknowledged; and `Notify(MoveReport { carried_through: L, old_relay_answered })`, with `L` the seed's `last_seq`: when the old relay did not sequence the `Fence`, entries sequenced there after `L` were not carried (V2-9).

2. Reword ENG-MOVE-003 (full text; it replaces the text R1-SEAL-RELAY-26 gave):

   - **ENG-MOVE-003** (engine) The moving device MUST seed the new relay with every device and recovery identity of `M_L` that §28.3 stage 3 lists, each with its `sign_pk` and its counter at `first_seq − 1`, and the frames from `first_seq` to its log head, each frame unchanged, with `first_seq` computed as §28.3 stage 3 states.

3. §28.3: add after ENG-MOVE-004:

   - **ENG-MOVE-005** (engine) When the old relay did not sequence the move's `Fence`, the moving device MUST send that `Fence` as an `append` to the new relay after the seed, and MUST NOT send it to the new relay as a `fence` admin operation.
   - **ENG-MOVE-006** (engine) When the old relay sequenced the move's `Fence` at `f`, the moving device MUST NOT send `seed` before its log head has reached `f`.
   - **ENG-MOVE-007** (engine) The moving device MUST upload to the new relay only a snapshot blob it sealed itself in §28.3 stage 3, and MUST NOT upload a blob fetched from another relay.

4. §36, `relay.md` block, first `Provides` bullet: replace `the move with the new relay's door and the `fence` operation (§28.3); seeding with the membership at the end of the tail and the counters before it (§28.3);` with:

   the move with the new relay's door and the `fence` operation on the old relay, read through its `Fence` before the seed, or the `Fence` appended on the new relay when the old one did not sequence it (§28.3); seeding with every device and recovery identity of the membership at the end of the tail and the counters before it, and uploading only a snapshot of its own sealing (§28.3);

### R1-ENGINE-RELAY-3 (engine-relay-4)

Nothing further to apply. R1-SEAL-RELAY-26 sets `first_seq = min(S + 1, max(1, M_L.purge_seq, L + 1 − 16,384))` in §28.3 stage 3 (carried unchanged into R1-ENGINE-RELAY-2) and rewords ENG-MOVE-003; R1-SEAL-RELAY-21 replaces relay.md §12.7's "up to 30 days of tail". The tail then covers the Skip window without the finding's lower choice of `S`.

### R1-ENGINE-RELAY-5 (engine-relay-6, engine-relay-16)

Matching edits: relay.md, same id.

1. §21.3, **Restore tail**, step 1: replace the whole step with:

   1. Requests of at most 64 frames and 1,048,576 bytes, from the relay head + 1, in seq order, with `chain_before` equal to this device's chain at the relay head. A request ends with the first control entry it contains; the next request is sent only after that entry's repairs of step 2 have succeeded, so that a frame of an author the entry registers never reaches the relay before that author's row (`relay.md` §12.9, `relay/unknown_author`).

2. §21.3, step 2: replace the whole step with:

   2. After the request that restores a control entry this device classified `Accept`, before the next request, the device repairs the relay for it, in item order (`relay.md` §12.9): `RegisterSeq { seq, dev, sign_pk, frame_hash }` for a `DeviceAdded`, with `sign_pk` the added device's (`DeviceView.sign_pk`, §6.2); `RecoveryRegisterSeq { seq, dev_r, sign_pk, frame_hash }` for a `K`-shape entry, with `dev_r` and `sign_pk` those of the `recovery` entry it installed (§6.2); `Revoke` for each revoked device other than itself; `Fence` without a frame for a `Fence`; and, for a rotation that carries `purge_before = p` with new epoch `n`, `Purge { before_seq: p, epoch: n }`, whose continuation of §28.2 step 3 (`duties.purge`) runs once the stop has ended. A refused control entry needs no repair. A `DeviceReinstated` needs the recovery identity's signature: the device records the alarm `RelayProtocol` with `engine/restore_incomplete` naming the seq, and the user repairs it with the recovery tool.

3. Reword ENG-EVID-004 (full text):

   - **ENG-EVID-004** (engine) A device with `RelayBehind` evidence whose log extends the relay's chain MUST restore the relay's tail from its own log at the original seqs, ending each request at its first control entry and sending, before the next request, the repairs §21.3 step 2 lists for that entry.

4. §36, `relay.md` block, first `Provides` bullet: replace `restore tail with the seq-mode registrations and `revoke` in log order (§21.3);` with `restore tail with requests that end at each control entry, each followed by that entry's seq-mode registration, `revoke`, `fence` or `purge` (§21.3);`.

### R1-ENGINE-RELAY-6 (engine-relay-8)

The session table contradicted §24.4: a revoked device emitted no `Connect`, so a relay-only revocation could never clear itself. A revoked device now reconnects hourly and only reads. Ruling S24's reinstatement is not applied here: it is with the lead (R1-SEAL-RELAY-8, R1-SEAL-ENGINE-13, R1-CORE-ENGINE-19), and the conservative reading (a reinstated device re-pairs) stays; see "For the lead", item 3.

1. §11.1, the table, row `Offline`: replace its Guard cell `a relay target is set, the device is not stopped by `identity_clone` and not revoked` with:

   a relay target is set, the device is not stopped by `identity_clone`, and, while it is revoked (§24.4), no `Connect` was emitted in the last 3,600,000 ms of `mono_ms`

2. §24.4, the paragraph after the list: replace `It reconnects at most once per 3,600,000 ms of `mono_ms`, so a mistaken `relay` source clears itself when the relay answers again;` with:

   It reconnects at most once per 3,600,000 ms of `mono_ms` (§11.1) and then only reads: it emits no `Append`, `Admin` or blob upload (§10.1), and the runtime logs in again first, since a revocation at the relay raised its token generation (`relay.md` §5.2, §12.5). So a mistaken `relay` source clears itself when the relay answers again;

   (The rest of the sentence, `a `DeviceReinstated` naming it that it can still classify clears it as well.`, stays.)

3. §24.4: add after ENG-EPOCH-002:

   - **ENG-EPOCH-004** (engine) A revoked device MUST emit `Connect` at most once per 3,600,000 ms of `mono_ms`, and while revoked MUST emit no `Append`, no `Admin` and no blob upload.

4. §34 Limits: replace the row `| reconnect while revoked | 3,600,000 | ms of `mono_ms` | ENG-EPOCH-002 |` with `| reconnect while revoked | 3,600,000 | ms of `mono_ms` | ENG-EPOCH-004 |`.

### R1-ENGINE-RELAY-11 (engine-relay-13)

`revoke` locks the target out at the relay before the log revokes it, so the engine composes the rotation again until it lands, bounded only by the operation timeout, as relay.md §22 and §24 open question 18 require.

1. §28.5, step 4: replace the whole step with:

   4. Refused with `seal/ctl_epoch` or `seal/ctl_wraps` (another rotation or a membership change raced it): if `dev` is now revoked in `M_K`, done; otherwise compose again from stage 2, after every such race, until the entry is accepted or `dev` is revoked in `M_K`, bounded only by the operation timeout of §28.8 (the relay has already locked the target out, `relay.md` §12.4, so the log must follow). Any other refusal: failed.

2. Reword ENG-OPS-002 (full text):

   - **ENG-OPS-002** (engine) The engine MUST send the `revoke` admin operation before it enqueues the rotation that revokes the device, and MUST compose the rotation again after every race until the target is revoked in its membership or the operation times out (§28.8).

3. §36, `relay.md` block, first `Provides` bullet: replace `` `revoke` before its entry, composed again after a race (§28.5); `` with `` `revoke` before its entry, composed again after every race until the target is revoked (§28.5); ``.

### R1-ENGINE-RELAY-12 (engine-relay-14)

device.md §10.5 already maps the socket's `state` message: "`state` (fenced) as `RelayRefused { req: None, 403 relay/fenced }`", which §11.9's first row handles (stop sending, read to the head, look for the `Fence`). The finding's mapping to a `Welcome` is not adopted (it would give the transport rule a second owner); the engine names the mapping instead.

§4.2, the events table, row `RelayRefused`: replace its "Sent by the runtime when" cell `a whole request was refused (`relay.md` §4.2), or the relay answered `fenced`, `revoked` or a budget` with:

a whole request was refused (`relay.md` §4.2), or the relay answered `fenced`, `revoked` or a budget; a socket `state` message (fenced, `relay.md` §11.3, §11.4) arrives as `RelayRefused { req: None, 403 relay/fenced }` (`device.md` §10.5)

### R1-ENGINE-RELAY-13 (engine-relay-15)

§31.3: replace `§12.3 to §12.10 (`register`, `revoke`, `fence`, `seed` and `seed-tail`, `compact`, `restore_tail`, `purge`, with their own checks)` with `§12.3 to §12.10 and §12.12 (`register`, `revoke`, `reinstate`, `fence`, `seed` and `seed-tail`, `compact`, `restore_tail`, `purge`, `recovery_register`, with their own checks)`.

### R1-ENGINE-RELAY-14 (engine-relay-17)

1. Reword ENG-RECV-001 (full text):

   - **ENG-RECV-001** (engine) The engine MUST read pages of at most 256 entries, after its log head, one read at a time, except the single-frame fetch of §17.2 (`after = t − 1`, `limit = 1`), whose answer it processes only as §17.2 states and never by §12.3, and MUST NOT read while an evidence stop is in force, except for the check of §21.3 step 3 and for a relay it follows under §28.3.

2. §17.2: after `otherwise from the relay (`Read { after: t − 1, limit: 1 }`, whose entry must have seq `t`)`, insert `; the page answering this read is used only here, for that one entry, and is not processed by §12.3 (its head, floors and flags change nothing)`. The sentence then continues `; it checks `H(frame) = hash` and classifies the entry again with the frame supplied.` unchanged.

### R1-ENGINE-RELAY-15 (engine-relay-18)

Matching edits: seal.md and relay.md, same id.

1. §11.7, `enum AdminRequest`: replace the line `    Compact { below_seq: u64, snap_entry_seq: u64, keep: Vec<u64> },         // relay.md §12.8` with `    Compact { below_seq: u64, snap_entry_seq: u64, blob_id: [u8; 16], keep: Vec<u64> }, // relay.md §12.8`.

2. §20.2, step 4: replace `4. `Admin { Compact { below_seq: b, snap_entry_seq: x, keep } }`.` with:

   4. `Admin { Compact { below_seq: b, snap_entry_seq: x, blob_id: info.blob, keep } }`, with `info` the chosen `SnapRecord`'s, so that the relay relies on, and keeps (`relay.md` RELAY-COMPACT-004), exactly the blob the `snap` entry at `x` announces.

### R1-ENGINE-RELAY-16 (engine-relay-19)

Nothing further to apply. R1-SEAL-ENGINE-20 names kinds 1, 7, 8 (`notices.tdk`), 9 and 10 in §7's table, points the lead sentence to `seal.md` §19.2 and deletes the §36 request for kinds 9 and 10; R1-CORE-ENGINE-22 adds the `Bootstrap` row (kind 17 `bootstrap.tdk`) and the sentence naming kinds 11 (`history.tdk`), 12 and 18 as `device.md`'s.

## Rejected

None. Parts of proposals are not adopted, each recorded in its change:
- engine-relay-1: the tuple form of `pending_recovery`; a named struct `PendingRecovery` carries the same six values (R1-ENGINE-RELAY-1).
- engine-relay-3: the addition to ENG-MOVE-003; the rule is its own clause, ENG-MOVE-005 (one requirement per clause).
- engine-relay-4: choosing a lower `S`; R1-SEAL-RELAY-26's `first_seq` already covers the window (R1-ENGINE-RELAY-3).
- engine-relay-8: the S24 part (§24.4 rewrite, the deleted §36 question and §38 item 24, S24 in §1), which would override the conservative reading the earlier resolvers wrote; it is with the lead.
- engine-relay-9: the engine part, already applied by R1-SEAL-ENGINE-19.
- engine-relay-10: `history` in `SnapshotMaterial`, the runtime writing `material.history` at Join and the seal.md field; R1-SEAL-RELAY-9 and R1-SEAL-ENGINE-15 settled that the sealer fills and keeps the history (R1-ENGINE-RELAY-8).
- engine-relay-11: the engine's `engine/runtime_protocol` refusal and `ENG-GEN-009` row; R1-SEAL-ENGINE-16 settled `engine/bad_host` and ENG-OPS-007, and R1-CORE-ENGINE-21 the §34 row.
- engine-relay-12: the `epoch` field of the `mint_read_token` response and the field name `epoch` (R1-SEAL-RELAY-20 named it `read_epoch`).
- engine-relay-14: mapping `state` to a `hello` and a `Welcome`; device.md §10.5 already maps it to `RelayRefused` (R1-ENGINE-RELAY-12).

## For the lead

1. **A move started under an evidence stop (engine-relay-7).** Reading the old relay up to its `Fence` (stage 2) is impossible when the device is stopped by evidence against that relay: frames from the cut on cannot be classified or persisted. Option (a), written as the change (R1-ENGINE-RELAY-2, stage 2): the mover fences the old relay without a frame (`fence` allows an empty `extra`, relay.md §12.6), which stops the old relay's service, and appends the `Fence` on the new relay after the seed, as when the old relay is gone. Other devices then meet `relay/fenced` with no `Fence` in the old log ("re-home needed", engine.md §11.9) and follow with `FollowAddress` and the move code. Option (b): fence the old relay with the frame and seed without it, which leaves the new relay behind the old one's final head and every follower in `RelayBehind`. Recommended: (a).
2. **`compact` gains `blob_id[16]` (engine-relay-18).** This is 16 signed bytes in one admin operation's `args`, in three specs (seal.md §16.2, relay.md §12.1 and §12.8, engine.md §11.7 and §20.2). It makes `compact_snap` the announced blob rather than the largest-seq blob of the same author, which could be an unannounced one left by engine.md §19.4 step 5. The alternative is no wire change and a residual recorded in relay.md §24. Recommended: accept the field. Written as the change.
3. **ENG-EPOCH-004 is now the revoked device's hourly read-only reconnect (engine-relay-8).** R1-CORE-ENGINE's "For the lead" item 2 called ENG-EPOCH-004 the next free id for the S24 rule. That rule pauses classification at `seal/missing_root` while revoked and resumes when the missed roots are learned. When S24's channel is ruled, the rule takes ENG-EPOCH-005. The reconnect fix does not depend on S24, because a relay-only revocation must be able to clear itself either way. Recommended: keep ENG-EPOCH-004 for the reconnect and give the S24 rule ENG-EPOCH-005.
4. **engine-relay-10 asks for `history` in `SnapshotMaterial`, which R1-SEAL-ENGINE-15 decided against.** The earlier decision is kept: the sealer fills a snapshot body's history from `history.tdk` and hands it to the store on opening (device.md §15.7), so the engine's trait does not change. Only a relay.md note is added (R1-ENGINE-RELAY-8). Recommended: keep the earlier decision.
