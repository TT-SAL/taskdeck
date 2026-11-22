# Resolutions R1: relay.md against device.md (18 findings)

Resolver 9 of 9. Inputs read: reviews.md §B, spec/README.md, foundation.md "Decisions" and "Rulings for the specs" (S1 to S31, with the settled S15 and the notes under the device draft's rulings), SYNC_PLAN.md §2 and §3, the earlier files `r1-seal-relay.md`, `r1-seal-engine.md`, `r1-core-engine.md`, `r1-engine-relay.md`, `r1-device-engine.md`, `r1-device-bridge.md`, `r1-core-device.md`, `r1-seal-device.md` and the lead's `r1-lead.md` (L1 to L15) in full; relay.md §1 to §16 and §19 to §24; device.md §6, §7, §10, §14 to §18, §21, §22, §24, §25; the engine.md passages on `AdminRequest`, `AdminOk`, `RelayRefusal`, `Connect`, `Disconnected` and §11.3 to §11.9. The lead's change file `r1-lead-changes.md` does not exist yet.

Built on the earlier files and the lead's rulings (settled; not repeated here):
- relay-device-4 (the read epoch from the relay): R1-SEAL-RELAY-20 (`read_epoch` in the `relay/stale_epoch` body, RELAY-RTOKEN-004, the §19 row, the `RelayError` extra), R1-SEAL-RELAY-27 (device.md §15.8 step 1, open question 7), R1-DEVICE-ENGINE-15 (the §22 row at 2 tries) and R1-SEAL-DEVICE-5 (the second refusal, `device/recovery_token`, `RecoveryLink::read_token` without `epoch`).
- relay-device-8 (255-byte host): R1-SEAL-RELAY-1, -15, R1-SEAL-DEVICE-1, -7, R1-ENGINE-RELAY-9, R1-DEVICE-ENGINE-18, R1-CORE-DEVICE-7.
- relay-device-9 (S15 at the relay): R1-SEAL-RELAY-16 (check 2, RELAY-COMPACT-007, the "Skip targets" paragraph, the §20 row, both §22 bullets); only relay.md §1 follows here.
- relay-device-10 (S27): R1-SEAL-RELAY-9, -28, R1-DEVICE-ENGINE-1, lead rulings L4 and L9.
- relay-device-11 (S29): R1-DEVICE-ENGINE-14, R1-DEVICE-BRIDGE-1, R1-SEAL-RELAY-10, lead ruling L10 (the file is `own-picture.tdk`; DEV-PIC-005 forbids every upload).
- relay-device-17 (seal kinds, S28 name): R1-SEAL-RELAY-10 and -11, R1-SEAL-DEVICE-2.
- relay-device-18 (S25 day): R1-ENGINE-RELAY-7 (§16.2 recomputed); only relay.md §1 follows here.
- relay-device-2 builds on R1-DEVICE-ENGINE-6 (the `Door` row of device.md §10.5); relay-device-7 builds on R1-DEVICE-ENGINE-9 (the refused-login row of device.md §10.6 and the engine's stop `Closed`); relay-device-3 builds on device.md §18.1 step 7 as R1-DEVICE-ENGINE-2 and R1-DEVICE-BRIDGE-5 left it.

New clause ids assigned here:
- relay.md: RELAY-ADMIN-005, RELAY-REVOKE-005, RELAY-LOGIN-006. Withdrawn: RELAY-TOKEN-003, RELAY-CLIENT-001, RELAY-CLIENT-002, RELAY-CLIENT-003, RELAY-CLIENT-004.
- device.md: DEV-NET-018, DEV-NET-019, DEV-LOGIN-003, DEV-BKP-013, DEV-CREATE-005. Reworded: DEV-NET-005 (owners and text), DEV-NET-008, DEV-BUDGET-001, DEV-PAIR-003, DEV-STORE-001.

New error codes: `device/recovery_truncated` (device.md §21). Changed meaning: `device/create_failed`. New public items: device.md `recovery_online::FenceAnswer`; `RecoveryLink::fence` returns it. Wire changes: the JSON error body gains `last_ctr` and `last_seq` (relay.md §4.2); the `login` answer gains `wraps` and `reinstate`'s `args` are those of lead ruling L1 (R1-RELAY-DEVICE-6). File change: `genesis.tdb` in the identity directory (device.md §6.1); `tokens[].push` may be absent in `device.tdk`.

Notation. "Replace A with B" means the exact text A, as it stands in the spec after the changes of the eight earlier files, becomes B. Clause texts are given in full. A table row is given in full, in the table's own column order. A change that needs edits in two or three specs has the same id under each heading, with the same field names, widths and values.

## relay.md

### R1-RELAY-DEVICE-1 (relay-device-1)

The frame an admin operation carries can get `gap` or `conflict`, which the relay answers as a JSON error; that error now carries the author's counter, so that an author without a log of its own frames (the recovery tool, whose older frames a purge deleted, §24 open question 13) can re-seal once. A purge can also make the tool's guessed counter too low, which is a `conflict` with no stored frame (§9.4 T3c), not a `gap`; both carry the counter. Matching edits: device.md, same id. engine.md does not change: its `RelayRefusal` does not carry the two fields (device.md drops them for the engine, which knows its own counter from its log, `engine.md` §11.4).

1. §4.2, the error-response table: add these two rows directly after the `read_epoch` row (which R1-SEAL-RELAY-20 adds after `head`):

   `| `last_ctr` | integer | optional; present exactly for `relay/gap` and `relay/conflict` refusing a `register` or `fence` request because of the frame it carries (§12.3, §12.6) | 0 to 2^53 − 1 | the stored `last_ctr` of that frame's author row (§7.1), so that an author that holds no log of its own frames learns the counter to use |`
   `| `last_seq` | integer | optional; present exactly when `last_ctr` is | 0 to 2^53 − 1 | the stored `last_seq` of that author row |`

2. §12.3, check 3: replace `(otherwise the operation fails with that outcome's code: `409 relay/conflict`, `409 relay/gap`, or the refusal's)` with `(otherwise the operation fails with that outcome's code: `409 relay/conflict` or `409 relay/gap`, whose error body carries the author row's `last_ctr` and `last_seq` (§4.2, RELAY-ADMIN-005), or the refusal's)`.

3. §12.6, the paragraph that begins `Checks:`: replace `and it passes §9.3 and its §9.4 outcome is `new` or `dup` (else that outcome's code).` with `and it passes §9.3 and its §9.4 outcome is `new` or `dup` (else that outcome's code; a `409 relay/gap` or `409 relay/conflict` carries the author row's `last_ctr` and `last_seq`, §4.2, RELAY-ADMIN-005).`

4. §12.1: add after RELAY-ADMIN-004:

   - **RELAY-ADMIN-005** (relay) When the frame carried by a `register` or `fence` request gets the outcome `gap` or `conflict` (§9.4), the object MUST refuse the request with `409 relay/gap` or `409 relay/conflict` and an error body whose `last_ctr` and `last_seq` are the author row's stored values.

5. §19 Error catalogue: replace the two rows

   `| `relay/conflict` | another frame is stored under this `(dev, ctr)` (409; per frame or JSON) | §9.4 T3b, T3c; §12.7; §12.9 | SYNC.md §4.3 rule 4 (`engine.md`) |`
   `| `relay/gap` | the counter is above the next one (409; per frame or JSON) | §9.4 T3d; §12.7; §12.9 | SYNC.md §4.3 rule 3: fetch own frames first |`

   with

   `| `relay/conflict` | another frame is stored under this `(dev, ctr)`, or none is held any more (409; per frame, or JSON; for the frame of `register` or `fence` with `last_ctr` and `last_seq`) | §9.4 T3b, T3c; §12.3; §12.6; §12.7; §12.9 | SYNC.md §4.3 rule 4 (`engine.md`); the recovery tool re-seals with `last_ctr + 1` (`device.md` §15.8) |`
   `| `relay/gap` | the counter is above the next one (409; per frame, or JSON; for the frame of `register` or `fence` with `last_ctr` and `last_seq`) | §9.4 T3d; §12.3; §12.6; §12.7; §12.9 | SYNC.md §4.3 rule 3: fetch own frames first; the recovery tool re-seals with `last_ctr + 1` (`device.md` §15.8) |`

6. §21 Test vectors, row `relay-admin.json`: replace its Covers cell `nonces, envelope, signer rules, every operation and its repeat` with `nonces, envelope, signer rules, every operation and its repeat; the `last_ctr` and `last_seq` of a `gap` or `conflict` refusing the frame of `register` or `fence``.

7. §22, the **Stage `recovery`.** paragraph (as R1-SEAL-RELAY-20 wrote it): replace `and fences the old board with a `Fence` frame and the `fence` operation over the door (§12.6).` with `and fences the old board with a `Fence` frame and the `fence` operation over the door (§12.6), re-sealing that frame once with the `last_ctr` that a `409 relay/gap` or `409 relay/conflict` answer carries (§4.2, RELAY-ADMIN-005).`

8. §23 Public API: replace the comment `// extra: { until?, index?, head?, read_epoch? }` (as R1-SEAL-RELAY-20 left it) with `// extra: { until?, index?, head?, read_epoch?, last_ctr?, last_seq? }`.

### R1-RELAY-DEVICE-2 (relay-device-9, relay-device-18)

The rulings list names what the spec now applies: the settled S15 (R1-SEAL-RELAY-16), S21 to S23 (the relay draft's own rulings), S25 (§16.2, R1-ENGINE-RELAY-7) and S27 (§13.3, R1-ENGINE-RELAY-8). S24 is not added here: its relay half is R1-RELAY-DEVICE-6, pending the lead's change file ("For the lead", item 1), which adds S24 to this list when it confirms that change.

§1, the fourth bullet of the list after "It realises:": replace `- the rulings S1–S4, S9, S13–S15 of `docs/sync/status/foundation.md`.` with:

- the rulings S1–S4, S9, S13 to S15 (S15 in its settled form: the Skip floor `max(purge_seq, s − 16,384)`, kept fetchable by RELAY-COMPACT-007), S21 to S23, S25 (the typical day of §16.2) and S27 (§13.3) of `docs/sync/status/foundation.md`.

### R1-RELAY-DEVICE-3 (relay-device-12)

A body that arrives too slowly is answered `408 relay/slow` by the gate, before the object runs, and §19 already says "retry with backoff"; §14.5, the table clients classify by, gains the row so that the client treats it as a link failure. Matching edits: device.md, same id. The proposed shorter device body limit and smaller batches are not adopted ("For the lead", item 2).

§14.5, the table: add this row directly after the row `| `503 relay/object_unavailable` | the board's object failed or is over its quota | as unreachable | as unreachable |`:

`| `408 relay/slow` | the request body did not arrive within the gate's deadline (§6.2 step 9): the link is slower than the 64 KiB/s §6.2 assumes, or stalled | as unreachable | as unreachable: the session ends, and the request is sent again after the backoff (a blob upload resumes from `blob-meta`) |`

### R1-RELAY-DEVICE-4 (relay-device-14)

The device keeps the relay address (host and door) in the engine's relay target and one token per host; the token record holds no door. Matching edits: device.md, same id.

§22, `device.md` block, second bullet: replace `- Requires: the device stores each board's relay address (host and door) with its token.` with:

- Requires: the device keeps each board's relay address (host and door) in its relay target and a token per relay host (`device.md` §10.3, §16), and presents the door of that target when it logs in without a token (§10.3).

### R1-RELAY-DEVICE-5 (relay-device-16)

One owner per rule: the client's obligations are `device.md`'s (this spec's §1 non-goals already say so: "the client's transport, backoff, User-Agent, key stores and its own request budget: `device.md`"). The relay keeps the tables of §14.3, §14.5 and §15.1 as the normative tables those device clauses apply, and RELAY-ADDR-002 (the parsing algorithm is this spec's). Matching edits: device.md and engine.md, same id.

1. §5.1: replace the clause RELAY-TOKEN-003 with:

   - **RELAY-TOKEN-003** (withdrawn) Client behaviour, owned by `device.md`: replaced by DEV-NET-005, which also names stage `recovery` as an owner.

2. §14.3: replace the clause RELAY-CLIENT-001 with:

   - **RELAY-CLIENT-001** (withdrawn) Client behaviour, owned by `device.md`: replaced by DEV-BUDGET-001, which counts as this section states.

3. §14.5: add this sentence directly before the clause RELAY-BUDGET-003: `A client classifies every exchange by this table first and acts as its row states (`device.md` §10.6, DEV-NET-011).`

4. §14.5: replace the clause RELAY-CLIENT-002 with:

   - **RELAY-CLIENT-002** (withdrawn) Client behaviour, owned by `device.md`: replaced by DEV-NET-011, whose table (`device.md` §10.6) applies the table of §14.5 first.

   and the clause RELAY-CLIENT-003 with:

   - **RELAY-CLIENT-003** (withdrawn) Client behaviour, owned by `device.md`: replaced by DEV-NET-012.

5. §15.1: replace the clause RELAY-CLIENT-004 with:

   - **RELAY-CLIENT-004** (withdrawn) Client behaviour, owned by `device.md`: replaced by DEV-NET-008.

6. §20 Limits: delete the row `| client request budget | 5,000 | requests per device per UTC day | RELAY-CLIENT-001 |` (the relay enforces no such limit; it is `device.md` §22's row for DEV-BUDGET-001).

7. §22, `device.md` block, first bullet: replace the whole bullet (`- Requires: the transport sends the constant User-Agent, … and opens no socket to the standby (RELAY-CLIENT-004).`) with:

   - Requires: the transport sends the constant User-Agent, protocol pings every 30 seconds, a session id per process (§11.3), uploads at no less than 64 KiB/s (§6.2), and polls `hello` and `read` on a relay with `push: false` and slot C and D while pairing as N; it keeps tokens opaque and only in the `Authorization` header (DEV-NET-005), parses relay addresses (RELAY-ADDR-002), keeps the request budget of §14.3 (DEV-BUDGET-001), classifies every exchange by the table of §14.5 (DEV-NET-011, `device.md` §10.6), never deletes on a relay answer (DEV-NET-012) and opens no socket to a relay that reports `push: false` (DEV-NET-008).

### R1-RELAY-DEVICE-6 (none of this group's findings: lead ruling L1)

Conservative reading of ruling L1's relay half, written because no earlier resolver wrote it and `seal.md` §16.2 now gives `reinstate` the args `dev[16] ‖ u32 first_epoch ‖ u16 n ‖ n × wrap[96]` (R1-SEAL-DEVICE-4), which this spec's fixed 16-byte args and 136-byte cap would refuse at A1, and device.md §10.3 already reads the wraps from the `login` answer (R1-DEVICE-ENGINE-13). The column and field names are this resolver's; where `r1-lead-changes.md` words the same places, the lead's text wins ("For the lead", item 1). Matching edits: device.md, same id.

1. §7.1, `CREATE TABLE IF NOT EXISTS devices`: replace the line `  added_at INTEGER NOT NULL               -- relay clock` with the two lines

   ```sql
     added_at INTEGER NOT NULL,              -- relay clock
     wraps    BLOB                           -- or NULL: u32 first_epoch ‖ u16 n ‖ n × wrap[96] of the latest reinstate (§12.5), until the next revoke
   ```

   (Every insert of a devices row in §10.2, §12.3, §12.7 and §12.12 leaves `wraps` NULL; their texts do not change.)

2. §10.3, the `login` response table: add this row after the row `push`:

   `| `wraps` | object | optional: present exactly while the device's row stores `wraps` (§12.5) | `{"first_epoch": integer, "wraps": [hex96, …]}`: the reinstatement's 1 to 256 wraps of the roots of epochs `first_epoch` onward, in epoch order, exactly as `reinstate` stored them (ruling S24, lead ruling L1); the relay does not interpret them |`

3. §10.3: add after RELAY-LOGIN-005:

   - **RELAY-LOGIN-006** (relay) A successful `login` MUST carry in `wraps` the `first_epoch` and wraps stored on the device's row, unchanged and in their stored order, whenever the row stores them, and MUST omit the field otherwise.

4. §12.1, the per-operation table: replace the row `| `reinstate` | 16 | empty | 136 (fixed) | T1, D | the current recovery identity | §12.5 |` with:

   `| `reinstate` | 22 + 96n | empty | 24,718 | T1, D | the current recovery identity | §12.5 |`

   (120 + 22 + 96 × 256 = 24,718.)

5. §12.4, the paragraph that begins `Effects, in one transaction:`: replace `the target's `state = 2` and `gen` + 1;` with `the target's `state = 2`, `gen` + 1 and `wraps` NULL;`.

6. §12.5: replace the whole paragraph (from `` `args = dev[16]`. Signer: `` to `Response (200): `{}`.`) with:

   `args = dev[16] ‖ u32 first_epoch ‖ u16 n ‖ n × wrap[96]` (`seal.md` §16.2; ruling S24, lead ruling L1): the wraps of the roots of epochs `first_epoch` to `first_epoch + n − 1` to the target, which the relay stores and returns at the target's login without interpreting them. Signer: the current recovery identity only (`seal.md` §13.6). Checks: `first_epoch ≥ 1` and `n` from 1 to 256 (else `400 relay/bad_args`); a role-1 row for `dev` with `state = 2` (else `409 relay/target`); fewer than 16 current role-1 rows (else `409 relay/device_cap`). Effects: `state = 1`, `gen` unchanged, `wraps = u32 first_epoch ‖ u16 n ‖ the n wraps` from `args`, replacing any stored; every `read_tokens` row `state = 2`. The device logs in for a new token, and the login answer carries the wraps (§10.3). Response (200): `{}`.

7. §12.5: add after RELAY-REVOKE-004:

   - **RELAY-REVOKE-005** (relay) `reinstate` MUST store the `first_epoch` and wraps of its arguments on the target's devices row, replacing any stored, and `revoke` MUST set that row's `wraps` to NULL in its transaction.

8. §20 Limits: add after the row `| current devices | 16 | rows | RELAY-REG-004 |`:

   `| wraps per `reinstate` | 1 to 256 | wraps | RELAY-REVOKE-005 |`

## engine.md

### R1-RELAY-DEVICE-5 (relay-device-16)

RELAY-CLIENT-002 is withdrawn (relay.md, same id); the classification is device.md's.

§11, the sentence that begins `The runtime classifies every relay exchange`: replace `(`relay.md` §14.5, RELAY-CLIENT-002)` with `(`relay.md` §14.5 through `device.md` §10.6, DEV-NET-011)`.

## device.md

### R1-RELAY-DEVICE-1 (relay-device-1)

The recovery tool learns its counter from the `fence` refusal (relay.md §4.2, RELAY-ADMIN-005, same id) and re-seals once.

1. §10.6: add this sentence directly after the paragraph that begins `"Next 00:00 UTC" is`:

   The error-body fields `last_ctr` and `last_seq` (`relay.md` §4.2) are parsed with the rest of the body and are not passed to the engine, which knows its own counter from its log (`engine.md` §11.4); only `RecoveryLink::fence` returns them (§15.8, §25).

2. §15.8, step 4: replace the whole step with:

   4. Read the old board's log to its head (as step 2) and take `ctr = 1 +` the highest counter of a frame authored by `dev_R` it saw (1 when none), the current epoch and its root (unwrapped from the newest rotation's wrap to the recovery key), and `based_on` the head. After a purge that guess can be too low, because the purge deleted the recovery identity's older frames (`relay.md` §24 open question 13); step 5 corrects it.

3. §15.8, step 5: replace the whole step with:

   5. Seal a `ctl` entry with one `Fence { host, door, home_gen: current + 1 }` naming the new board's relay (`seal.md` §12.7), authored by the recovery identity, and send it with `fence` (`relay.md` §12.6) with the door and a recovery admin signature. A `409 relay/gap` or `409 relay/conflict` answer carries the recovery identity's `last_ctr` and `last_seq` (`relay.md` §4.2, RELAY-ADMIN-005), which `RecoveryLink::fence` returns as `FenceAnswer::Counter`: the tool reads the log again to its head, re-seals the entry once with `ctr = last_ctr + 1` and `based_on` that head, and sends it with a fresh nonce. A second refusal ends the fencing with the relay's code, shown to the user.

4. §25 Public API: replace the whole `pub mod recovery_online { … }` block (with `read_token` as R1-SEAL-DEVICE-5 left it) with:

   ```rust
   pub mod recovery_online {
       pub struct RecoveryLink { /* transport to one relay, door credential */ }
       pub enum FenceAnswer { Fenced { seq: u64, chain: [u8; 32] },             // the Fence is sequenced
                              Counter { last_ctr: u64, last_seq: u64 } }        // 409 relay/gap or relay/conflict (§15.8 step 5)
       impl RecoveryLink {
           pub fn new(address: &str, recovery: &taskdeck_seal::RecoveryKeys, trust: super::TrustRoots)
               -> Result<Self, super::DeviceError>;
           pub fn read_token(&mut self, backup_token: Option<&str>) -> Result<(), super::DeviceError>;   // §15.8 step 1: the backup's token, else epoch 1, then the refusal's read_epoch
           pub fn read_after(&mut self, after: u64) -> Result<taskdeck_sync::Page, super::DeviceError>;
           pub fn fence(&mut self, frame: Vec<u8>, host: &str, door: [u8; 5], home_gen: u32)
               -> Result<FenceAnswer, super::DeviceError>;
       }
   }
   ```

### R1-RELAY-DEVICE-3 (relay-device-12)

`408 relay/slow` is a transient link failure (relay.md §14.5, same id). The device's body timeout stays as it is: it is longer than the gate's deadline, so the relay's 408 arrives before the device gives up, and a shorter client limit would not make a slow upload succeed.

§10.6, the table: add this row directly after the row `| a response without `X-TD-Relay`, any other status; `503 relay/object_unavailable`; `503 relay/misconfigured` | `Disconnected { unreachable: true }` | as unreachable |`:

`| `408 relay/slow` answering any request (`relay.md` §6.2 step 9, §14.5) | `Disconnected { unreachable: true }`: the session ends and the request is unanswered (`engine.md` ENG-SEND-007); a blob upload resumes after the backoff as DEV-NET-010 states | as unreachable |`

### R1-RELAY-DEVICE-4 (relay-device-14)

The door of a login without a token is the relay target's (`engine.md` §7: `RelayTarget.door` of the `Connect`); `device.tdk` keeps no door (relay.md §22, same id).

1. §10.3, the paragraph that begins `Login: `POST /v1/b/{board}/login/nonce``: replace `credential the stored token (any generation) or, when there is none, `Door <door hex>`;` with `credential the stored token (any generation) or, when there is none, `Door <door hex>` with the door of the `Connect`'s relay target (`RelayTarget.door`: `meta.relay`, or the bootstrap's relay target before the first checkpoint, `engine.md` §7, §7.10);`.

2. §24, `relay.md` block, the `Provides` bullet: replace `, the relay address stored with the token (§16).` with `, the relay address (host and door) kept in the engine's relay target (`engine.md` §7, `meta.relay`) and a token per relay host in `device.tdk` (§16).`

### R1-RELAY-DEVICE-5 (relay-device-16)

The client's obligations are stated here only (relay.md withdraws RELAY-TOKEN-003 and RELAY-CLIENT-001 to RELAY-CLIENT-004, same id); citations of the withdrawn ids point to relay.md's tables instead, and DEV-PAIR-003 no longer restates RELAY-ADDR-002.

1. Reword DEV-NET-005 (owners and text; full text):

   - **DEV-NET-005** (device, recovery) A relay token MUST be treated as an opaque ASCII string, MUST travel only in the `Authorization` header of a request to the relay origin that issued it (`relay.md` §5.1), and MUST NOT appear in a URL, a query, a log, an error or a devicectl output.

2. Reword DEV-NET-008 (full text):

   - **DEV-NET-008** (device) The transport MUST open a socket only when the relay's capability says `push` and the device is in the foreground, and MUST NOT open a socket to a relay that reported `push: false` (`relay.md` §15.1).

3. §10.6, first sentence: replace `Every exchange is classified by `relay.md` §14.5 first (RELAY-CLIENT-002), then by its code:` with `Every exchange is classified by the table of `relay.md` §14.5 first, then by its code (DEV-NET-011):`.

4. §10.6, the paragraph that begins `"Next 00:00 UTC" is`: replace `(RELAY-CLIENT-003, `engine.md` §11.8)` with `(DEV-NET-012, `engine.md` §11.8)`.

5. §10.8, first paragraph: replace `except one `Hello` per 900,000 ms (RELAY-CLIENT-001).` with `except one `Hello` per 900,000 ms (`relay.md` §14.3).`

6. Reword DEV-BUDGET-001 (full text):

   - **DEV-BUDGET-001** (device) The device MUST NOT send more than 5,000 requests to the relay per UTC day of its own clock, counting as §10.8 states, and over the budget MUST send nothing but one `Hello` per 900,000 ms until the next 00:00 UTC (`relay.md` §14.3).

7. Reword DEV-PAIR-003 (full text):

   - **DEV-PAIR-003** (device) A joining device MUST check the code's check characters (SEAL-CODE-002) before it sends any request; its relay address is checked as RELAY-ADDR-002 states.

8. §24, `relay.md` block, the `Provides` bullet (as R1-RELAY-DEVICE-4 leaves its end): replace `relay-address parsing before any request (DEV-PAIR-003),` with `relay-address parsing before any request (RELAY-ADDR-002),` and `the classification of §14.5 (DEV-NET-011),` with `the classification of `relay.md` §14.5 (DEV-NET-011, §10.6),`.

### R1-RELAY-DEVICE-6 (none of this group's findings: lead ruling L1)

The device's hand-over of the reinstatement wraps (R1-DEVICE-ENGINE-13, R1-SEAL-DEVICE-4) names the `login` answer's field (relay.md §10.3 `wraps`, same id). Where `r1-lead-changes.md` words this differently, the lead's text wins.

§10.3, the sentence R1-DEVICE-ENGINE-13 item 1 inserted: replace `When a `login` answer carries the wraps of the epoch roots this identity missed while it was revoked (ruling S24, `relay.md` §10.3),` with `When a `login` answer carries the wraps of the epoch roots this identity missed while it was revoked (its field `wraps`: `first_epoch` and the wraps in epoch order; ruling S24, `relay.md` §10.3, §12.5),`.

### R1-RELAY-DEVICE-7 (relay-device-2)

`Seed` and `SeedTail` get their own rows, as `Door` has (R1-DEVICE-ENGINE-6): `Seed` goes to the new relay with the create secret, and its token is stored at once under the new host; `SeedTail` is the `seed-tail` route, with that token and no nonce or signature (`relay.md` §10.1, §12.7; `engine.md` §11.7).

§10.5, the table: replace the row R1-DEVICE-ENGINE-6 wrote,

`| `Admin { req, op }`, every other `op` | `POST …/admin/nonce` (body: the signer, this device's `dev`), then `POST …/admin/<name>` with the envelope of `relay.md` §12.1, `sig = sign_admin(Signer::Device(keys), op, board, nonce)`; credential `T`, except `Seed` (the create secret) and `SeedTail` (the token `Seed` returned) on the new relay named by the op | `AdminResult { req }` |`

with these three rows, in this order:

`| `Admin { req, op: Seed { host, … } }` | on the new relay, origin `relay_origin(host)` (`seal.md` §5.4): `POST …/admin/nonce` with `Authorization: Create <secret>` (the create secret held for the operation, §17.8) and body this device's `dev` as the signer; then `POST …/admin/seed` with the envelope of `relay.md` §12.1, `sig = sign_admin(Signer::Device(keys), op, board, nonce)`, credential the create secret | `AdminResult { req }`: on 200, the answer's `token` is first stored in `device.tdk` `tokens` under `host` (replace procedure; without `push`, §10.3), then `Ok(AdminOk { door: Some(d), token: true })` with every other field `None`; `next_seq` and `state` are parsed and not passed on; a JSON error as §10.6 |`
`| `Admin { req, op: SeedTail { host, first, frames } }` | `POST …/seed/tail` on the origin `relay_origin(host)`, with the token stored for `host` by `Seed` (`Bearer`) and the body `u64 first ‖ u16 n ‖ n × (u32 len ‖ frame)` (`relay.md` §12.7); no admin nonce and no signature | `AdminResult { req }`: on 200, `Ok(AdminOk { head: Some(head), token: false })` with every other field `None`, `state` parsed and not passed on; a JSON error as §10.6, with `head` (`relay/not_next`) and `index` (a refused frame) in its `RelayRefusal` |`
`| `Admin { req, op }`, every other `op` | on the current relay target: `POST …/admin/nonce` (body: the signer, this device's `dev`), then `POST …/admin/<name>` with the envelope of `relay.md` §12.1, `sig = sign_admin(Signer::Device(keys), op, board, nonce)`; credential `T` | `AdminResult { req }` |`

### R1-RELAY-DEVICE-8 (relay-device-3)

The Genesis item commits to the genesis snapshot's `sha256` (`seal.md` §12.2), and a re-sealed snapshot has a new nonce and so another hash, which the relay also refuses under the same id or a second purpose-3 id (`relay.md` §10.7 check 3, "Repeat"). The sealed bytes are therefore made durable before `create`, and only they are uploaded. They are kept in a plain file of their own, not in `bootstrap.tdk`: a genesis board (at most 62,914,560 bytes) and its snapshot (at most 67,108,864) do not fit one record of `seal.md` §19.1, and the blob is already sealed under epoch 1.

1. §6.1, the layout: add this line directly after the line `      bootstrap.tdk                   kind 17, until the first checkpoint`:

   `      genesis.tdb                     the sealed genesis snapshot of an imported board, until the first checkpoint (§18.1)`

2. §6.1: add this sentence directly after the paragraph that begins `` `<board hex>` and `<dev hex>` are 32 lowercase hex characters. `` (which ends `sealed under `K_file(kind)` of the identity's `L_d`.`):

   `genesis.tdb` is not a sealed file of `seal.md` §19 and has no kind: it holds exactly the sealed blob bytes of the genesis snapshot (`seal.md` §20, `S = 1`), already encrypted under epoch 1, whose `blob`, `sha256`, `size` and `parts` the Genesis item of `frame_1` names (`seal.md` §12.2).

3. Reword DEV-STORE-001 (full text):

   - **DEV-STORE-001** (device) The store MUST keep every file of an identity in that identity's directory with exactly the names, kinds and record counts of the table of §6.1, and `genesis.tdb` only as §6.1 and §18.1 state.

4. §7.2, the table, row **replace** (as R1-DEVICE-ENGINE-14 and R1-DEVICE-BRIDGE-5 left it): replace `` `active.tds`, `pending.tds`, `store.tds`, `` with `` `active.tds`, `pending.tds`, `store.tds`, `genesis.tdb` (its bytes without a header or sealing), ``.

5. §7.2, the table, row **remove**: replace `` `bootstrap.tdk` after the first checkpoint, `` with `` `bootstrap.tdk` and `genesis.tdb` after the first checkpoint, ``.

6. §7.3, the table: replace the row `| `WriteCheckpoint(c)` | replace `checkpoint.tdk`; then, if `bootstrap.tdk` exists, remove it |` with `| `WriteCheckpoint(c)` | replace `checkpoint.tdk`; then, if `bootstrap.tdk` exists, remove it, and then `genesis.tdb` when it exists |`.

7. §7.4, first paragraph: append at its end: ` `genesis.tdb` is not read at open; only `resume_create` reads it (§18.1).`

8. §18.1, step 4: replace `seal its genesis snapshot at `S = 1` (`seal.md` §20, purpose 3).` with `seal its genesis snapshot at `S = 1` (`seal.md` §20, purpose 3) and keep its sealed bytes; they are never sealed again.`

9. §18.1, step 7 (as R1-DEVICE-ENGINE-2 and R1-DEVICE-BRIDGE-5 left it): replace `, and `bootstrap.tdk` (` with `, `genesis.tdb` (the sealed genesis snapshot of step 4, replace procedure; only when there is one), and `bootstrap.tdk` (`.

10. §18.1, step 9: replace the whole step with:

    9. **Token and snapshot.** Write the token into `device.tdk` (without `push`, §10.3); when there is a genesis snapshot, upload exactly the bytes of `genesis.tdb` with that token (purpose 3, `seq = 1`, with the `blob`, `size` and `sha256` the Genesis item names), resuming from `blob-meta` as DEV-NET-010 states.

11. §18.1, the paragraph after step 10: replace `` `resume_create(create_secret)` runs steps 8 to 10 (the keys are in the identity directory; the seed is on paper, not needed again), `` with `` `resume_create(create_secret)` runs steps 8 to 10 (the keys and the sealed genesis snapshot are in the identity directory; the seed is on paper, not needed again; a `genesis.tdb` whose SHA-256 differs from the Genesis item's `sha256` fails it with `device/create_failed`), ``.

12. §18.1: add after DEV-CREATE-004:

    - **DEV-CREATE-005** (device) Creating a board from a genesis board that is not empty MUST make the sealed genesis snapshot durable in `genesis.tdb` before it sends `create`, and MUST upload only those bytes, never a snapshot sealed again.

13. §21 Error catalogue: replace the row `| `device/create_failed` | the relay refused `create` | §18.1 | show the relay code |` with `| `device/create_failed` | the relay refused `create`, or the genesis snapshot kept for it is damaged | §18.1 | show the relay code; for a damaged snapshot, abandon the create and start again |`.

### R1-RELAY-DEVICE-9 (relay-device-5)

Only a login answer carries `push` among the answers that give a device a token (`relay.md` §10.3; not `create` §10.2, `register` §12.3 through the bundle, or `seed` §12.7). A token stored without one has no `push` until the device asks `caps`, which accepts a role-1 token and is answered by the gate (`relay.md` §10.9). No relay change. (The `create` token's "without `push`" is already in §18.1 step 9 as R1-RELAY-DEVICE-8 gives it, and the `seed` token's in §10.5 as R1-RELAY-DEVICE-7 gives it.)

1. §10.3: add this paragraph directly after the paragraph that begins `Login: `POST /v1/b/{board}/login/nonce``:

   **Tokens stored without a login.** The token of `create` (§18.1 step 9), the bundle's token (§18.3) and the token of `Seed` (§10.5) are stored without `push`. A `Connect` whose stored token has no `push` first sends `GET …/caps` with that token and stores the answer's `push` with it (replace procedure on `device.tdk`) before it chooses between the socket and HTTPS; a refused or failed `caps` ends the `Connect` as §10.6 states. A login answer's `push` replaces the stored one.

2. §10.5, the table: replace the row of `Connect { relay }` with:

   `| `Connect { relay }` | wait the backoff (§10.7); login when §10.3 requires it; when the stored token has no `push`, `GET …/caps` with `T` and store its `push` (§10.3); then, when the stored capability says `push` and the device is in the foreground (§10.9): open the socket with `T`, send `hello` (`0x01 ‖ session ‖ u32 0`) and wait for `welcome`; otherwise nothing more | socket: `Connected { push: true }`, then `Welcome` from the `welcome`; HTTPS: `Connected { push: false }` |`

3. §10.5: add after DEV-NET-010:

   - **DEV-NET-018** (device) Before it acts on a `Connect` with a stored token that has no `push`, the transport MUST send `GET …/caps` with that token and MUST store the answer's `push` with the token in `device.tdk`.

4. §16, the `device.tdk` / `device.json` table, row `tokens`: append to its Constraints cell (as R1-DEVICE-ENGINE-18 and R1-SEAL-DEVICE-13 left it) `; `push` absent until the device has learned the relay's capability (§10.3)`.

5. §18.3, paragraph **Persisting the join**: replace `the bundle's token for the host,` with `the bundle's token for the host (without `push`, §10.3),`.

### R1-RELAY-DEVICE-10 (relay-device-6)

The relay closes with 4002 the older socket of a process that said hello on a newer one (`relay.md` §11.3, §11.6: "nothing: it is already on the new socket"). Such a close, and anything else that arrives on a socket the transport has replaced, must not end the new session.

1. §10.6, the table: add this row directly after the row `| socket close 4001 | none (the `displaced` message came first) | |`:

   `| socket close 4002 | none: it closes a socket the transport has already replaced (`relay.md` §11.3) | nothing |`

2. §10.6: add this sentence directly after the table, before the paragraph that begins `"Next 00:00 UTC" is`: `A close, message or error of a socket other than the transport's current one is discarded and produces no event.`

3. §10.6: add after DEV-NET-012:

   - **DEV-NET-019** (device) The transport MUST deliver no event for a close, message or error of a socket other than its current one, and MUST deliver none for a close with code 4002.

### R1-RELAY-DEVICE-11 (relay-device-7)

A refused login already ends the `Connect` with `RelayRefused { req: None, … }` and `Disconnected` (R1-DEVICE-ENGINE-9), which `engine.md` §11.9 handles (revoked, `Closed`, or an hourly retry). What remains are the two benign races of `relay.md` §10.3: a nonce that expired or was pushed out of the 64-row cap (`401 relay/no_nonce`), and another login of the same device in between (`409 relay/bad_gen`). Each restarts the login once instead of raising an alarm.

1. §10.3, the paragraph that begins `Login: `POST /v1/b/{board}/login/nonce``: replace the sentence R1-DEVICE-ENGINE-9 item 1 appended, ` A JSON refusal of `login-nonce` or `login` ends the `Connect` as §10.6 states.`, with:

   A `login` refused with `401 relay/no_nonce` or `409 relay/bad_gen` (an expired or evicted nonce, or another login of this device in between) is restarted once from `login-nonce`; any other JSON refusal of `login-nonce` or `login`, or a second refusal of the restarted login, ends the `Connect` as §10.6 states.

2. §10.6, the table: replace the row R1-DEVICE-ENGINE-9 item 2 added (`| any JSON error answering `login-nonce` or `login` (§10.3), whatever its code | … | `engine.md` §11.9 |`) with:

   `| any JSON error answering `login-nonce` or `login` (§10.3), except a first `401 relay/no_nonce` or `409 relay/bad_gen`, which restarts the login once | `RelayRefused { req: None, refusal }`, then `Disconnected { unreachable: false, paused_until_ms: None }`; the `Connect` counts as a failure (§10.7) | `engine.md` §11.9 |`

3. §10.3: add after DEV-LOGIN-002:

   - **DEV-LOGIN-003** (device) The device MUST restart from `login-nonce`, at most once per `Connect`, a login refused with `401 relay/no_nonce` or `409 relay/bad_gen`, and MUST end the `Connect` as §10.6 states for every other refusal of `login-nonce` or `login` and for a second such refusal.

### R1-RELAY-DEVICE-12 (relay-device-13)

When the backup's seq is below the relay's floor, the page says so (`truncated`, `relay.md` §10.5, RELAY-READ-002): compaction or purge removed entries after `b`, so the chain from `c_b` cannot be recomputed. That is no tampering verdict.

1. §15.8, step 2: replace the whole step with:

   2. Read pages after `b` with that token to the relay head; the chain from `c_b` is recomputed for every entry (`seal.md` §15.1). When a page has `truncated` set (`relay.md` §10.5: entries after `b` were removed by compaction or purge), the chain cannot be recomputed from `b`: the top-up stops with `device/recovery_truncated`, naming the page's `floor`, and the tool restores the backup as it is, or the user picks a newer backup. Otherwise a break, or a head chain that differs, stops the top-up with `device/recovery_chain` and the tool restores the backup as it is.

2. §15.8: add after DEV-BKP-012:

   - **DEV-BKP-013** (device, recovery) A top-up MUST stop with `device/recovery_truncated`, without changing the restored state, when a page it reads has `truncated` set, and MUST NOT report that case as `device/recovery_chain`.

3. §21 Error catalogue: add this row directly after the row `device/recovery_token` (which R1-SEAL-DEVICE-5 adds after `device/recovery_chain`):

   `| `device/recovery_truncated` | a top-up cannot chain from the backup: the relay compacted or purged entries after it | §15.8 | restore the backup as it is, or a newer backup |`

### R1-RELAY-DEVICE-13 (relay-device-15)

§15.4: replace the sentence `Every rotation revokes all read tokens at the relay (`relay.md` §5.2), so a new epoch always means a new token.` with:

The relay revokes every backup-read token at `revoke` (an orphan `revoke` included, lead ruling L5), `reinstate`, `recovery_register` and `purge`, and those of lower epochs when a writer mints for a newer epoch (`relay.md` §5.2); a rotation that none of these accompanies leaves them valid there. A writer therefore mints whenever its stored token's epoch differs from the membership's.

### R1-RELAY-DEVICE-14 (relay-device-4, relay-device-8, relay-device-10, relay-device-11, relay-device-17)

Nothing further to apply.
- relay-device-4: the relay's `read_epoch` in the `relay/stale_epoch` body is R1-SEAL-RELAY-20 (RELAY-RTOKEN-004; the field is named `read_epoch`, not `epoch`); §15.8 step 1 (epoch 1, then once with the refusal's `read_epoch`) and open question 7 are R1-SEAL-RELAY-27; the §22 row (2 tries) is R1-DEVICE-ENGINE-15; the second refusal (`device/recovery_token`, not the proposed `device/recovery_epoch`) and `RecoveryLink::read_token` without `epoch` are R1-SEAL-DEVICE-5. Asking first with epoch 1 never overshoots the relay's `read_epoch`.
- relay-device-8: seal.md §5.4 and SEAL-ENC-006 (255 bytes, no `:443`) are R1-SEAL-RELAY-1 and R1-SEAL-DEVICE-7; relay.md's `fence` cap 262,529 and §22 are R1-SEAL-RELAY-15, its §20 host row R1-ENGINE-RELAY-9, §5.4 step 4 R1-SEAL-DEVICE-7 (§12.6's `400 relay/bad_args` follows from seal.md's rule); device.md's `tokens`, `move_board`, §18.1 step 1, §15.3.1, §6.4.9 and the §22 row are R1-DEVICE-ENGINE-18, R1-CORE-DEVICE-7, R1-SEAL-DEVICE-1 and -7.
- relay-device-10 (ruling S27): the history travels inside the snapshot body (R1-SEAL-RELAY-9), device.md §15.7, §18.3, §24 and open question 6 are R1-SEAL-RELAY-28, the rejoin path DEV-RUN-006 is R1-DEVICE-ENGINE-1; lead rulings L4 and L9 keep the Join layout without history.
- relay-device-11 (ruling S29): kind 19 `own-picture.tdk`, §6.4.14, §14.3, DEV-PIC-005 (never uploaded, never in an intent, frame, blob or backup), the facade rows and `KIND_OWN_PICTURE` are R1-DEVICE-ENGINE-14 and R1-DEVICE-BRIDGE-1 (lead ruling L10); seal.md §19.2 is R1-SEAL-RELAY-10.
- relay-device-17: seal.md §19.2 (kinds 1 to 19, the segments, kind 6 the overlay alone) and `FileKind` are R1-SEAL-RELAY-10 and R1-SEAL-DEVICE-2; §21.1's tagged name is R1-SEAL-RELAY-11.

## Rejected

None as a whole. Parts of proposals are not adopted, each recorded in its change:
- relay-device-1: the presence of `last_ctr` for `seed-tail` and `restore_tail` (their `index` already names the frame, and their sender holds the frames) and for `recovery_register` (it has no frame mode since R1-SEAL-RELAY-17); `last_ctr` and `last_seq` in `engine.md`'s `RelayRefusal` (the engine knows its own counter, `engine.md` §11.4). Added instead: `relay/conflict` carries them too, since a guess made too low after a purge is a `conflict` with no stored frame (§9.4 T3c), not a `gap`.
- relay-device-2: storing the seed's `push` "from `caps` of that host" inside the `Seed` row; R1-RELAY-DEVICE-9's single rule covers all three tokens stored without a login.
- relay-device-3: the variant that extends kind 17 (a board and its snapshot do not fit one record of `seal.md` §19.1); `genesis.tdb` is not a sealed file, so `seal.md` §19.2 gains no kind.
- relay-device-4, -8, -10, -11, -17: already applied by the earlier files (R1-RELAY-DEVICE-14).
- relay-device-5: the relay is not changed (`caps` already accepts a role-1 token).
- relay-device-12: a device body limit tied to the relay's deadline, and append batches of at most 4 frames on slow links (an `engine.md` change); see "For the lead", item 2.
- relay-device-13: the offer to restore a newer backup is the tool's screen; device.md names the choice only.
- relay-device-16: "DEV-PAIR-003 cites RELAY-ADDR-002" is applied by narrowing DEV-PAIR-003 to the code check, which only it states for the device (SEAL-CODE-002 names `seal` alone as owner).

## For the lead

1. **The relay half of ruling L1 had no author (R1-RELAY-DEVICE-6).** `seal.md` §16.2 now gives `reinstate` L1's args (R1-SEAL-DEVICE-4), and device.md §10.3 reads the wraps from the `login` answer (R1-DEVICE-ENGINE-13). But relay.md still had a fixed 16-byte `reinstate` (cap 136) that would refuse those args at A1, and no field in `login`. R1-SEAL-DEVICE flagged this for "the lead's file or the relay–device resolver", and `r1-lead-changes.md` does not exist yet. So the conservative reading is written as the change. It adds a nullable `devices.wraps` column, the `reinstate` args with cap 24,718 and the check `1 ≤ n ≤ 256`, clearing on `revoke`, a `login` answer field `wraps: {"first_epoch", "wraps": [hex96…]}`, RELAY-REVOKE-005, RELAY-LOGIN-006 and a §20 row. Recommended: confirm it, or let your change file replace it where it differs. Two more things are needed. First, add S24 to relay.md §1. Second, `engine.md` §11.7 `AdminRequest::Reinstate { dev }` and `seal.md` §26 `AdminOp::Reinstate` (now `{ dev, first_epoch, wraps }`) disagree. Recommendation: the engine never sends `reinstate` (R1-ENGINE-RELAY-5 leaves a lost reinstatement to the recovery tool), so delete `Reinstate` from `AdminRequest` and `Revoke { dev }, Reinstate { dev: Dev }` becomes `Revoke { dev: Dev }`. This is an engine.md change outside this pair, not written here.
2. **Links slower than 64 KiB/s cannot upload a 1 MiB part (relay-device-12).** The gate's deadline is 10 s plus 1 s per 64 KiB (`relay.md` §6.2), so a 1 MiB snapshot or picture part fails on a sustained 32 KiB/s phone link every time, and so does a large append batch. The change only makes the client treat `408 relay/slow` as transient (backoff, resume). The finding's smaller batches would help appends only, at the cost of an engine rule, and blob parts are fixed at 1 MiB. Recommended: keep the 64 KiB/s floor that relay.md §22 and device.md §24 already state as a client requirement, and record it as a residual for the owner's rehearsal. A per-route grace of 30 s instead of 10 s would also cover links down to about 30 KiB/s for single parts, at no byte cost; it is offered as an option.
