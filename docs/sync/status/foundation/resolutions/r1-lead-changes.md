# Lead changes R1: what the rulings L1 to L19 still need

Last resolver of round 1. Inputs read: `r1-lead.md` (L1 to L19) in full; the "For the lead" sections of the nine resolver files; the changes of those files that touch a ruling's subject (R1-SEAL-RELAY-1, -3, -8, -9, -21, -26, -29; R1-SEAL-ENGINE-3, -13, -15, -18, -19; R1-CORE-ENGINE-11; R1-ENGINE-RELAY-2, -5, -6; R1-DEVICE-ENGINE-13, -14; R1-DEVICE-BRIDGE-1; R1-SEAL-DEVICE-4, -7; R1-RELAY-DEVICE-2, -3, -6, -9); and the spec passages they name (seal.md §7, §14.5, §20.4, §20.5, §22, §23, §26; relay.md §1, §10.3, §12.4, §12.5, §16.1, §20, §24; engine.md §4.2, §6.1, §6.3, §11.7, §16.1, §19.4, §19.7, §24.4, §28.4, §36 to §38; device.md §10.3, §10.5, §10.10, §15.8, §24, §26; bridge.md's revoked and re-pair texts).

The fixers apply this file last, on top of the nine. "Replace A with B" means the exact text A, as it stands after the nine files, becomes B. Clause texts are given in full; table rows in full, in the table's own column order. A change that touches several specs has the same id under each heading, with identical wording for the shared part (the input `MissedRoots { first_epoch: u32, wraps: Vec<[u8; 96]> }`, the sealer method `take_missed_wraps`, the relay field `wraps`).

New clause ids: seal.md SEAL-BLOB-009; engine.md ENG-EPOCH-005 (lead ruling L8). Reworded: device.md DEV-LOGIN-002.
New error code: `seal/snapshot_large` (seal.md §22).
New public items: engine.md `Input::MissedRoots { first_epoch: u32, wraps: Vec<[u8; 96]> }`, `Sealer::take_missed_wraps`; removed: `AdminRequest::Reinstate`.
Open questions added: engine.md 26, relay.md 26, device.md 24 (after R1-CORE-DEVICE's 23).

## seal.md

### R1-LEAD-1 (L1, L16)

Seal's half of L1 is R1-SEAL-DEVICE-4, confirmed by L16 and not reworded here. One sentence of its §7 paragraph is false in L1's purge case (the first wrapped epoch raised to the epoch in force at `purge_seq`): the device's membership then holds no commitment for that epoch, and SEAL-WRAP-002 binds the wrap to `c_e`, so the device cannot open the wraps. The sentence gains that case. The open-question item R1-SEAL-RELAY-8 added (§27 item 18, "a reinstated device pairs again") is already replaced by R1-SEAL-DEVICE-4 item 8; nothing else in seal.md says a reinstated device pairs again.

§7, the paragraph **Wraps delivered outside an entry.** that R1-SEAL-DEVICE-4 item 1 adds: replace `the commitment of the first wrapped epoch is already in its membership (it classified the revoking rotation, which is sealed under an epoch it held), and each later one enters it` with `the commitment of the first wrapped epoch is already in its membership (it classified the revoking rotation, which is sealed under an epoch it held), unless `first_epoch` was raised to the epoch in force at `purge_seq`: then its membership records no commitment for that epoch, it opens none of the wraps, and it cannot replay across the purge (`engine.md` §24.4); each later one enters it`.

### R1-LEAD-2 (L4)

A snapshot body over 64 MiB is refused when it is sealed, with a named code, instead of being undefined. No resolver wrote the code (R1-CORE-DEVICE's "Built on" list assumes it). The board cap of ruling S18 stays (`core.md` §17, as R1-CORE-ENGINE-11 words it). Matching edits: engine.md, same id.

1. §20.4, after the table: replace the sentence `Its uncompressed length is at most 67,108,864 bytes.` with:

   Its uncompressed length is at most 67,108,864 bytes: `seal_snapshot` refuses a longer body, or one whose sealed bytes would exceed the sealed-blob maximum of 67,108,864 bytes (§23), with `seal/snapshot_large`, and produces no sealed bytes (SEAL-BLOB-009).

2. §20.5: add after SEAL-BLOB-008 (which R1-SEAL-ENGINE-3 adds after SEAL-BLOB-007):

   - **SEAL-BLOB-009** (seal) A writer MUST refuse with `seal/snapshot_large`, and MUST produce no sealed bytes, when a snapshot body laid out as §20.4 exceeds 67,108,864 bytes before compression or its sealed blob would exceed 67,108,864 bytes.

3. §22 Error catalogue: add this row directly after the row `| `seal/blob_open` | blob or snapshot body fails to open or parse | §20.5 | alarm naming the writer |`:

   `| `seal/snapshot_large` | a snapshot body over 67,108,864 bytes before compression, or a sealed snapshot over 67,108,864 bytes | §20.4 | nothing is sealed; `engine.md` decides (§19.4, §28.4) |`

   (`SealError` has one variant per code of §22, §26; it gains this one.)

4. §23 Limits: replace the row `| inflated snapshot | 67,108,864 | bytes | SEAL-BLOB-003 |` with `| inflated snapshot | 67,108,864 | bytes | SEAL-BLOB-003, SEAL-BLOB-009 |`, and the row `| sealed blob, maximum | 67,108,864 | bytes | SEAL-BLOB-001 |` with `| sealed blob, maximum | 67,108,864 | bytes | SEAL-BLOB-001, SEAL-BLOB-009 |`.

## relay.md

### R1-LEAD-1 (L1, L16)

The relay's half of L1 is R1-RELAY-DEVICE-6, confirmed by L16 and not reworded here (the nullable `devices.wraps` column, the `reinstate` args and checks, the cap of 24,718, `wraps` NULL on `revoke`, the `login` answer field `wraps`, RELAY-REVOKE-005, RELAY-LOGIN-006, the §20 row). Only §1 is missing.

§1, the fourth bullet as R1-RELAY-DEVICE-2 left it: replace `S21 to S23, S25 (the typical day of §16.2) and S27 (§13.3)` with `S21 to S23, S24 (the `reinstate` wraps, stored on the target's row and returned by `login`: §12.5, §10.3), S25 (the typical day of §16.2) and S27 (§13.3)`.

### R1-LEAD-3 (L19)

The slow-link residual is recorded where its constants and its client requirement live. Matching edits: device.md, same id. The relay's deadline constants do not change.

§24 Open questions: add after item 25:

26. **Links slower than 64 KiB/s** (§6.2): the body deadline is 10,000 ms plus 1,000 ms per 65,536 bytes, so a 1,048,576-byte blob part, or a large append batch, sent over a sustained link slower than 64 KiB/s is answered `408 relay/slow` every time. Clients treat that answer as transient (§14.5), and 64 KiB/s stays a stated client requirement (§22, `device.md` §10.10). The constants stay until the owner's rehearsal measures real phone links (SYNC.md §11 "Still to measure"). Default kept.

## engine.md

### R1-LEAD-1 (L1, L8, L16)

Ruling S24's engine half, with the id ENG-EPOCH-005 (L8; ENG-EPOCH-004 stays the hourly read-only reconnect of R1-ENGINE-RELAY-6). The wraps reach the engine as an input, because the engine must know when to resume classification, and the engine hands them to the sealer through a new trait method; the sealer opens each wrap once its membership records that epoch's commitment (`seal.md` §7 as R1-SEAL-DEVICE-4 and R1-LEAD-1 word it). This removes the conservative text of R1-SEAL-ENGINE-13 (§36 and §38 item 24) and replaces it. `Reinstate` leaves `AdminRequest` (L16): the engine never sends it. Matching edits: device.md (the login case and the input), same id.

1. §1, the fourth bullet as R1-SEAL-ENGINE-19 item 6 left it: replace `S13 to S15, S25 and S27` with `S13 to S15, S24, S25 and S27`.

2. §4.2, the inputs table: add this row directly after the row `| `Connected` | `push: bool` | a session to the relay is open (socket, or HTTPS reachability) |`:

   `| `MissedRoots` | `first_epoch: u32, wraps: Vec<[u8; 96]>` | a `login` answer carried `wraps` (`relay.md` §10.3; ruling S24): sent right after that login, before any other input; accepted in every session state and changes none (§24.4) |`

3. §6.1, the trait: after the line `    fn forget_roots_below(&mut self, epoch: u32) -> bool;` add the lines

   ```rust
       fn take_missed_wraps(&mut self, m: &Self::Membership, first_epoch: u32, wraps: &[[u8; 96]])
           -> Result<bool, SealerError>;                                // §24.4, ruling S24
   ```

4. §6.1, **Contracts** table: replace the row `| `classify` | `seal.md` §14 over the inputs of §14.1, with `M_{s−1} = input.membership`, the sealer's own roots, its own X25519 key, its build's supported rules and, for `seq = 1`, its genesis anchor; returns `seal.md` §14.4's outcome; `keys_changed` is true when it learned a root; it never panics (SEAL-GEN-011) |` with:

   `| `classify` | `seal.md` §14 over the inputs of §14.1, with `M_{s−1} = input.membership`, the sealer's own roots, its own X25519 key, its build's supported rules and, for `seq = 1`, its genesis anchor; returns `seal.md` §14.4's outcome; while it holds a missed wrap of epoch `n` (`take_missed_wraps`), an accepted `EpochRotated{n}` that does not wrap to this device has that wrap opened against the rotation's `commit` (`seal.md` §7) as SEAL-RCPT-007 opens an own wrap, returning the commitment halt when it does not open or does not match; `keys_changed` is true when it learned a root; it never panics (SEAL-GEN-011) |`

5. §6.1, **Contracts** table: add this row directly after the row `| `forget_roots_below` | deletes every epoch root below `epoch` (purge, §28.2); true when a secret changed |`:

   `| `take_missed_wraps` | ruling S24 (§24.4): keeps in memory the wraps of the roots of epochs `first_epoch` to `first_epoch + wraps.len() − 1` that a `login` answer carried, replacing any it holds, and opens at once, by `seal.md` §7 (SEAL-WRAP-002, SEAL-KEY-013), the wrap of each of those epochs whose root it lacks and whose commitment `m` records; `classify` opens the others as it accepts the rotations that set their epochs; true when it learned a root (a changed secret, ENG-SEALER-004); `seal/commitment` when such a wrap does not open or its root does not match the commitment (SEAL-WRAP-003); it never persists the wraps; the fake sealer opens them with its wrap layout (§6.3) |`

6. §11.7, `pub enum AdminRequest`: replace the line `    Revoke { dev: Dev }, Reinstate { dev: Dev },                              // relay.md §12.4, §12.5` with `    Revoke { dev: Dev },                                                      // relay.md §12.4; reinstate (§12.5) is the recovery tool's`.

7. §16.1, the halts table: replace the row `| `Commitment { seq, author }` | the sealer returns `Commitment` with an accepted rotation (`seal.md` §14.5) | applies up to `seq` | stops after `seq` | never: the device re-pairs and resubmits (§24.5) |` with:

   `| `Commitment { seq, author }` | the sealer returns `Commitment` with an accepted rotation (`seal.md` §14.5), also for a missed wrap of that rotation's epoch (§24.4); or `take_missed_wraps` returns `seal/commitment` (§24.4), with `seq = K` and `author` the current recovery identity of `M_K` | applies up to `seq` | stops after `seq` | never: the device re-pairs and resubmits (§24.5) |`

8. §24.4, the paragraph after the list (as R1-ENGINE-RELAY-6 item 2 left it): replace its last clause `a `DeviceReinstated` naming it that it can still classify clears it as well.` with `a `DeviceReinstated` naming it clears it as well, classified directly or after the missed roots of ruling S24 arrive (below).`

9. §24.4: add after ENG-EPOCH-004 (which R1-ENGINE-RELAY-6 item 3 adds) this paragraph and clause:

   **Reinstatement (ruling S24).** The recovery tool reinstates a revoked device with the `reinstate` admin operation, whose `args` carry the wraps, to the device's `hpke_pk`, of the roots of epochs `first_epoch` to `E` (`seal.md` §16.2: `first_epoch` is the epoch the revoking entry's rotation set up, or the epoch in force at `purge_seq` when that is later; `E` is the epoch the reinstating entry is sealed under), and then with the `R`-shape entry whose `DeviceReinstated` names it and whose rotation to `E + 1` wraps to it like to every other recipient. The relay stores the wraps on the device's row and returns them in every `login` answer of that device until it is revoked again (`relay.md` §10.3 `wraps`, §12.5). The runtime logs in at every `Connect` while this device is revoked and sends `MissedRoots { first_epoch, wraps }` after each login whose answer carries them (`device.md` §10.3). While revoked, the device keeps its log and pauses classification at the first entry for which `classify` returns `seal/missing_root` (the paused seq, `K + 1`), without the local fault `engine/missing_root`. On `MissedRoots` while revoked, the engine calls `Sealer::take_missed_wraps(M_K, first_epoch, wraps)`; a `MissedRoots` with no wrap, more than 256 wraps or `first_epoch = 0` is `engine/runtime_protocol` and is ignored, and one that arrives while the device is not revoked is ignored (the relay returns the wraps until the next `revoke`). When the sealer reports a learned root, the engine persists the identity (D4), clears the revoked state (the stop `Revoked` and its banner action), and classifies and applies in seq order from the paused seq, from its log and then from the relay, before it seals or appends anything: the root of each later epoch is checked against the commitment of the rotation that set it as that rotation is classified (each rotation is sealed under the epoch before it; §6.1 `classify`), and the reinstating entry's own rotation gives `R_{E+1}`. A wrap that does not open, or a root that does not match its commitment, is the commitment halt (§16.1). A reinstated device does not pair again. If `M_K` still records this device as revoked when classification reaches the relay's head (a `reinstate` whose entry never landed), the device is revoked again with `source: log` and the revoking entry's seq and author, and pauses at the next `seal/missing_root`. A device revoked before a purge cannot replay across it: its membership holds no commitment of `first_epoch`, so the sealer learns no root from the wraps; when its reads reach the relay's floor it rejoins from a snapshot at or after `purge_seq` by §19.7, as a device away more than 365 days does, and without the root of a candidate's epoch it ends in the stop `stranded` and is offered re-pair and resubmit (§24.5).

   - **ENG-EPOCH-005** (engine) A device revoked in its membership MUST keep its log and MUST pause classification at the first entry for which the sealer returns `seal/missing_root`; on `MissedRoots` while revoked it MUST pass the wraps to `Sealer::take_missed_wraps` with `M_K`, and once the sealer reports a learned root it MUST persist the identity, clear the revoked state, and classify and apply in seq order from the paused seq up to the relay's head before it seals or appends anything.

10. §36, `seal.md` block: replace the bullet R1-SEAL-ENGINE-13 item 1 wrote (`- Open (ruling S24, with the lead; `seal.md` §27 item 18): …`, to its end `Until it is ruled, the device re-pairs (§24.4).`) with:

    - Requires (ruling S24, lead ruling L1): the `reinstate` operation's `args = dev[16] ‖ u32 first_epoch ‖ u16 n ‖ n × wrap[96]` (`seal.md` §16.2), signed by the recovery identity and never sent by the engine, and the opening of a wrap delivered outside an entry (`seal.md` §7, SEAL-WRAP-002, SEAL-WRAP-003), which the sealer implements in `take_missed_wraps` and `classify` (§6.1, §24.4).

11. §36, `relay.md` block: add at its end:

    - Requires (ruling S24): `reinstate` stores its wraps on the target's devices row and `revoke` clears them (RELAY-REVOKE-005), and every `login` answer of that device carries them in `wraps` while they are stored (RELAY-LOGIN-006).

12. §36, `device.md` block: add at its end:

    - Requires (ruling S24): the runtime logs in at every `Connect` while this device is revoked, and sends `MissedRoots { first_epoch, wraps }` right after each login whose answer carries `wraps` (`device.md` §10.3).

13. §37 Public API, `pub enum Input`: replace the line `    Connected { push: bool }, Welcome(Welcome), Page { req: Option<u64>, page: Page },` with the two lines

    ```rust
        Connected { push: bool }, MissedRoots { first_epoch: u32, wraps: Vec<[u8; 96]> },
        Welcome(Welcome), Page { req: Option<u64>, page: Page },
    ```

14. §38, item 24 (as R1-SEAL-ENGINE-13 item 2 left it): replace the whole item with:

    24. **A reinstated device reads what was sealed while it was revoked** (§24.4; ruling S24, lead ruling L1): the wraps of the roots it missed reach it in its `login` answer, outside the reinstating entry it cannot open, and it resumes classification at the paused seq; it does not pair again. A device revoked before a purge cannot replay across it and rejoins as §19.7 states. Decided here.

### R1-LEAD-2 (L4)

What the engine does when `seal_snapshot` refuses with `seal/snapshot_large` (seal.md, same id).

1. §6.1, **Contracts** table, the row of `seal_snapshot`, `open_snapshot` as R1-SEAL-ENGINE-15 item 2 left it: replace `the engine never supplies or reads it.` with:

   the engine never supplies or reads it. It refuses a body over 67,108,864 bytes, or one whose sealed bytes would exceed 67,108,864, with `seal/snapshot_large` (`seal.md` SEAL-BLOB-009): §19.4 then uploads and announces nothing, §28.4 reports the pairing failed, and any other caller fails its operation with that code (`OpDone`, §28).

2. §19.4, step 2: replace `2. `Sealer::seal_snapshot(M_K, material)`, which seals under `M_K`'s current epoch.` with `2. `Sealer::seal_snapshot(M_K, material)`, which seals under `M_K`'s current epoch. A `SealerError` (`seal/snapshot_large`: the body would exceed 67,108,864 bytes, `seal.md` SEAL-BLOB-009) ends the attempt: nothing is uploaded or announced, and the duty stays as it was.`

3. §28.4, step 1: replace `` `Sealer::seal_snapshot(M_K, material)`; `Blob { Upload { purpose: 2, seq: h } }`. `` with `` `Sealer::seal_snapshot(M_K, material)`; `Blob { Upload { purpose: 2, seq: h } }`. A `SealerError` from `seal_snapshot` (`seal/snapshot_large`) ends the pairing before any upload or entry: `Notify(PairFailed { ticket, code })` with that code. ``

### R1-LEAD-4 (L2)

The longer seeded tail is a deliberate difference from SYNC.md §6.7, recorded by the spec that owns the seed's composition (ENG-MOVE-003, §28.3 stage 3). No row in `spec/CHANGES.md` (none of the nine files wrote one).

§38 Open questions: add after item 25:

26. **The seeded tail can reach back further than 30 days** (§28.3 stage 3): `first_seq = min(S + 1, max(1, M_L.purge_seq, L + 1 − 16,384))` keeps on the new relay every Skip target ruling S15 permits after the move, at a cost of at most 16,384 frames written once; on a quiet board that reaches back further than SYNC.md §6.7's "up to 30 days of tail". Default: the longer tail (lead ruling L2), a deliberate difference from SYNC.md §6.7.

## device.md

### R1-LEAD-1 (L1, L16)

The device hands the `login` answer's `wraps` to the engine as `MissedRoots` (engine.md, same id), not straight to the sealer adapter, because the engine must know when to resume classification; the adapter still holds them in memory (`Sealer::take_missed_wraps`). The field's name is already in the hand-over (R1-RELAY-DEVICE-6's device.md part), and §18.4 is R1-DEVICE-ENGINE-13 item 2; neither is reworded. A fourth login case makes a revoked device log in at every `Connect`, so that a process that restarts after a reinstatement with a still-valid token does not miss the wraps.

1. §10.3, the list after `It logs in (`relay.md` §10.3, `seal.md` §16.1):`: add after item 3:

   4. at every `Connect` while the engine is stopped `Revoked` (`engine.md` §24.4, its status's `stops`), even with a stored token, so that the answer carries the wraps of a reinstatement (ruling S24); `403 relay/revoked` ends the `Connect` as §10.6 states.

2. Reword DEV-LOGIN-002 (full text):

   - **DEV-LOGIN-002** (device) The device MUST log in only in the four cases of §10.3, and MUST retry a request refused with `relay/token_stale` at most once after a successful login.

3. §10.3, the sentence R1-DEVICE-ENGINE-13 item 1 inserted (with the opening R1-RELAY-DEVICE-6 gave it): replace `the transport hands them to the sealer adapter before it uses the new token; the adapter holds them in memory for classification,` with `the runtime sends `MissedRoots { first_epoch, wraps }` to the engine (`engine.md` §4.2) right after that login, before any other input, each wrap decoded from its 192 hex characters, and the engine hands them to the sealer adapter (`Sealer::take_missed_wraps`, `engine.md` §6.1, §24.4), which holds them in memory for classification,`.

4. §10.5, the table, the row of `Connect { relay }` as R1-RELAY-DEVICE-9 item 2 gives it: replace its last cell `` socket: `Connected { push: true }`, then `Welcome` from the `welcome`; HTTPS: `Connected { push: false }` `` with `` `MissedRoots { first_epoch, wraps }` first when a login of this `Connect` answered `wraps` (§10.3); then socket: `Connected { push: true }`, then `Welcome` from the `welcome`; HTTPS: `Connected { push: false }` ``.

5. §24, `engine.md` block: add at its end:

   - Provides (ruling S24): a login at every `Connect` while the engine is stopped `Revoked`, and `MissedRoots { first_epoch, wraps }` right after each login whose answer carries `wraps` (§10.3; `engine.md` §24.4).

6. §24, `relay.md` block: add at its end:

   - Requires (ruling S24): the `login` answer's field `wraps`, present exactly while the device's row stores the wraps of a `reinstate` (`relay.md` §10.3, RELAY-LOGIN-006).

### R1-LEAD-3 (L19)

relay.md, same id. The transport's handling is R1-RELAY-DEVICE-3 (`408 relay/slow` as unreachable, §10.6).

§26 Open questions: add after item 23 (which R1-CORE-DEVICE-8 adds):

24. **A link slower than 64 KiB/s cannot upload a 1 MiB part** (`relay.md` §6.2): the relay answers `408 relay/slow`, which the transport treats as unreachable and sends again after the backoff (§10.6), so such an upload never completes over that link. Uploads at 64 KiB/s or more stay a client requirement (§10.10, §24); the owner's rehearsal measures real phone links (SYNC.md §11 "Still to measure"). Default kept.

## Already written

- L1, seal.md: R1-SEAL-DEVICE-4 (the `reinstate` args and §16.2 row, `AdminOp::Reinstate { dev, first_epoch, wraps }`, the §23 limit of 256, SEAL-WRAP-002 and SEAL-WRAP-003, the §25 bullet, §27 item 18 replacing R1-SEAL-RELAY-8's); only the purge case is added (R1-LEAD-1).
- L1, relay.md: R1-RELAY-DEVICE-6 (the `devices.wraps` column, args cap 24,718, `1 ≤ n ≤ 256` with `relay/bad_args`, `relay/target` and `relay/device_cap` unchanged, `wraps` NULL on `revoke`, the `login` field, RELAY-REVOKE-005, RELAY-LOGIN-006, the §20 row). §16.1's row `revoke`, `reinstate` (`2 + r`) needs no change: the wraps are written in the target's devices row, which both operations already write.
- L1, device.md: the field name in the hand-over is R1-RELAY-DEVICE-6 (device.md part); §18.4 is R1-DEVICE-ENGINE-13 item 2; §1 lists S24 by R1-DEVICE-BRIDGE-10.
- L1, bridge.md: no change; nothing there says a reinstated phone pairs again (the `engine/revoked` message "Open Devices to pair it again" is the action offered to a revoked phone, which re-pair still is).
- L3: R1-CORE-ENGINE-1, R1-CORE-ENGINE-3, R1-CORE-ENGINE-13.
- L5: R1-SEAL-RELAY-3 (`revoke` row), R1-SEAL-RELAY-6, RELAY-REVOKE-003 as it stands, R1-SEAL-RELAY-25 (ENG-OPS-004).
- L6: R1-ENGINE-RELAY-2 (stage 2).
- L7: R1-ENGINE-RELAY-15 (seal.md, relay.md, engine.md).
- L8: R1-ENGINE-RELAY-6 (ENG-EPOCH-004 and the §34 row); ENG-EPOCH-005 is R1-LEAD-1.
- L9: R1-SEAL-ENGINE-15, R1-SEAL-RELAY-28, R1-ENGINE-RELAY-8.
- L10: R1-DEVICE-ENGINE-14 (which replaces R1-SEAL-RELAY-29's sentence) and R1-DEVICE-BRIDGE-1.
- L11: R1-DEVICE-ENGINE-4 (device.md, engine.md, bridge.md).
- L12: R1-DEVICE-BRIDGE-7 (BRIDGE-EVT-005).
- L13: R1-DEVICE-BRIDGE-8.
- L14: R1-DEVICE-BRIDGE-1; it adds no thread, timer or redraw rule to the desktop's render loop.
- L15: R1-CORE-DEVICE-8 and device.md open question 23.
- L17: R1-SEAL-DEVICE-7 (seal.md, relay.md §5.4 step 4, device.md).
- L18: R1-SEAL-DEVICE-13.
- L19, the transient `408 relay/slow`: R1-RELAY-DEVICE-3 (relay.md §14.5, device.md §10.6); the open questions are R1-LEAD-3.

## Id conflicts

- ENG-MOVE-003: R1-ENGINE-RELAY-2 item 2 rewords it on top of R1-SEAL-RELAY-26 item 3. The later text keeps everything the earlier added (`M_L`, the counter at `first_seq − 1`, the frames from `first_seq`, `first_seq` as §28.3 stage 3 states) and adds the recovery identities and `sign_pk`. No merge needed.
- SEAL-ENC-006: R1-SEAL-DEVICE-7 item 3 rewords it on top of R1-SEAL-RELAY-1 item 2. The later text keeps the 253-byte part before `:`, the port limit, the 255 bytes in all and the encoder and parser duties, and adds "not 443". No merge needed.
- No new clause id here collides: SEAL-BLOB-009 follows SEAL-BLOB-008 (R1-SEAL-ENGINE-3); ENG-EPOCH-005 follows ENG-EPOCH-004 (R1-ENGINE-RELAY-6).

## For the lead

1. **L1's purge case cannot work as worded.** A wrap's `aad` binds the commitment `c_e` (SEAL-WRAP-002), and a device revoked before a purge has no `c_e` for the epoch in force at `purge_seq`: the rotation that set it is purged, and the only other copy is inside snapshots sealed under epochs whose roots it cannot verify or open. So the wraps from that `first_epoch` serve nothing, and the device ends stranded and re-pairs (§19.7 step 6). Written as the change (conservative, L1 literally): R1-LEAD-1 says so in seal.md §7 and engine.md §24.4. Recommended: the recovery tool does not offer reinstatement to a device whose revoking entry lies below `purge_seq` (the owner re-pairs it), and the clause "or the epoch in force at `purge_seq` when that is later" leaves the `reinstate` row of seal.md §16.2 and the engine text.
2. **A re-home drops stored wraps.** `seed` writes device rows with `wraps` NULL (R1-RELAY-DEVICE-6 item 1), and a `reinstate` sent again on the new relay is refused with `409 relay/target`, because the seeded row is already current. A device reinstated but not yet logged in when the board moves never receives its wraps and must re-pair. Written as the change: nothing (the device stays revoked and is offered re-pair, ENG-EPOCH-002). Recommended: record it as a residual in relay.md §24 in round 2, or let `seed` carry each current device's stored wraps.
3. **The recovery tool's reinstatement has no steps.** device.md §15.8 lists only top-up and fencing, and `RecoveryLink` has no method that sends `reinstate` (`engine.md`'s `engine/restore_incomplete` row already tells the user to "run the recovery tool's reinstate"). Composing L1's wraps (the epochs, the 256 limit, the order before the `R`-shape entry) therefore has no owner text beyond seal.md §16.2's row. Written as the change: nothing. Recommended: round 2 adds "Reinstating a device" to device.md §15.8 with a `RecoveryLink::reinstate(dev, first_epoch, wraps, frame)` method and its clause.
4. **A `reinstate` whose entry never lands.** L1 clears the revoked state once the roots are learned. If the `R`-shape entry never lands, the device holds the roots but `M_K` still lists it revoked. Written as the change: once classification reaches the relay's head with `M_K` still recording it revoked, the device is revoked again and pauses at the next `seal/missing_root` (R1-LEAD-1, engine.md §24.4). Recommended: confirm.
