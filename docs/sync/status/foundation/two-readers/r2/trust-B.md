# Group `trust` — reader B

Notation as in `edit.md`: `ctx(x)` = `taskdeck/v1/x` ‖ `0x00`; `H` = SHA-256; `K1 = K_entry,1`; `I7` = 1,099,511,627,783; `I41` = 1,099,511,627,817. Admin message (seal.md §16.2) = `ctx("admin")[18] ‖ vb8(op) ‖ BID ‖ signer[16] ‖ H(args)[32] ‖ relay_nonce[32]`.

## Scenario 4 — Revocation with rotation

World additions: at `T0 − 5 s` Mac enqueued `rename I7 → "Call the plumber"` (`RM`, `at_ms` 1,795,341,595,000, `based_on (200, C200)`), sealed it at `ctr 61` under epoch 1 (frame `FM`, 670 bytes, payload `{"id":1099511627783,"name":"Call the plumber","op":"rename"}`) and sent it on its socket; it reaches the relay after Desk's rotation commits. Phone holds one unsealed outbox record `RP` (`at_ms T0 − 1 h`, `based_on (200, C200)`).

1. **User** (Desk), `T0`: Devices → Phone → "Remove (lost)", no replacement → `Operation::Revoke{dev: PHONE, replace: false}` (engine.md §28.5).
2. **Desk** engine: `PHONE` is a current device of `M_K`, not Desk, not the last current device → accepted; `Persist[WriteMeta(op stage 1)]` (D10, ENG-OPS-003).
3. **Desk** → Relay `POST /v1/b/<BID>/admin/nonce` (Bearer Desk token), body `DESK` (16) → `{"nonce":"<N1>","expires":…}` (relay.md §12.1; nonce row `purpose 2, dev DESK`).
4. **Desk** → Relay `POST …/admin/revoke`, 136 bytes:

   | Field | Width | Value |
   |---|---|---|
   | `signer` | 16 | `DESK` |
   | `nonce` | 32 | `N1` |
   | `sig` | 64 | `SIG(Desk key; "taskdeck/v1/admin\0"; vb8("revoke") ‖ BID ‖ DESK ‖ H(PHONE) ‖ N1)` (121-byte message) |
   | `args_len` | 4 | 16 |
   | `args` | 16 | `PHONE` |
   | `extra_len` | 4 | 0 |

5. **Relay** A1 layout (136 fixed); A2 open; A3 T1, signer = token's dev; A4 nonce found and deleted; A5 `DESK` current role 1; A6 signature; revoke checks: `PHONE` role 1 state 1; signer ≠ target; Desk and Mac stay current. Transaction: `PHONE` `state 2`, `gen 2`, `wraps NULL`; every `read_tokens` row `state 2`. After commit: close Phone's sockets with 4003 (none open). Answer `{}` (relay.md §12.4).
6. **Desk** `AdminResult Ok` → enqueue `CtlIntent::Rotate{revoke [PHONE], reinstate [], purge_before None}` (`at_ms = wall_ms ≈ T0`, `based_on (200, C200)`), draws `RAND1[16]` = `entry_id`; one `Persist[WriteMeta(op stage 2), Outbox[Put(RR)]]` (ENG-OPS-009).
7. **Desk** send loop (`Live`, epoch gate open): seal `RR` with `ctr = 122`. The sealer composes shape R from the membership now (engine.md §9.2): working copy `W` = `M_200` → `DeviceRevoked PHONE` → current devices `{DESK, MAC}`; `EpochRotated{epoch 2}` wraps to `DESK`, `MAC`, `dev_R`. Draws: `RAND2[32]` = `R_2` (SEAL-KEY-010); `RAND3[32]`, `RAND4[32]`, `RAND5[32]` = the HPKE `ikm` of the three wraps, in wrap order (SEAL-WRAP-004); `RAND6[24]` = frame nonce.
   AMBIGUOUS: the order of the draws `R_2`, the three wrap `ikm`s and the nonce within one `seal` of a rotation is fixed nowhere (seal.md §6.5, §7 "in the order the wraps are made", SEAL-GEN-012); and the order in which the wraps are *made* (before or after sorting by `recipient_id`) is not stated.
8. **Desk** `c_2 = H(ctx("epoch")[18] ‖ BID ‖ u32 2 ‖ R_2)` (70-byte input). Each wrap (seal.md §7), recipients ascending by id (`w_a < w_b < w_c` = the sorted order of `DESK`, `MAC`, `dev_R`):

   | Input | Bytes | Value |
   |---|---|---|
   | `info` | 85 | `ctx("wrap")[17] ‖ BID ‖ u32 2 ‖ recipient_id[16] ‖ recipient_pk[32]` (`DESK_HPKE`, `MAC_HPKE`, `RHPKE`) |
   | `aad` | 61 | `ctx("wrap-aad")[21] ‖ 01 ‖ 01 ‖ 0020 ‖ 0001 ‖ 0003 ‖ c_2` |
   | output | 96 | `recipient_id[16] ‖ enc[32] ‖ ct[48]` with `(enc, ct) = HPKE(recipient_pk; info; aad; R_2)` |

9. **Desk** control payload (seal.md §11.3, §12.4, §12.5; canonical key order), 919 bytes:

   ```json
   {"items":[{"dev":"<PHONE hex>","type":"DeviceRevoked"},{"commit":"<c_2 hex>","epoch":2,"type":"EpochRotated","wraps":["<w_a hex192>","<w_b hex192>","<w_c hex192>"]}],"sig":"<SIG(Desk key; \"taskdeck/v1/ctl\\0\"; inner) hex>"}
   ```

   `inner` (112 bytes) = `ctx("ctl")[16] ‖ BID ‖ DESK ‖ u64 122 ‖ RAND1 ‖ i64 at_ms ‖ H(items_bytes)`.
10. **Desk** plaintext (`U = 69 + 919 = 988`, `P = 1024`): `RAND1 ‖ u8 2 ‖ at_ms ‖ u64 200 ‖ C200 ‖ u32 919 ‖ payload ‖ 0x00 × 36`. Frame `FR` sealed under epoch 1 (SEAL-ENV-004), 1,182 bytes:

    | Offset | Field | Width | Value |
    |---|---|---|---|
    | 0 | `magic` | 4 | `"TDF1"` |
    | 4 | `board_id` | 16 | `BID` |
    | 20 | `dev` | 16 | `DESK` |
    | 36 | `ctr` | 8 | 122 |
    | 44 | `len` | 4 | 1,070 |
    | 48 | `ver`, `suite`, `epoch` | 6 | 1, 1, 1 |
    | 54 | `nonce` | 24 | `RAND6` |
    | 78 | `ciphertext` | 1,040 | `AEAD(K1; RAND6; ctx("entry") ‖ FR[0..48] ‖ 01 01 00000001; pt[1024])` |
    | 1,118 | `sig` | 64 | `SIG(Desk key; "taskdeck/v1/frame\0"; FR[0..1118])` |

11. **Desk** `Persist[Outbox[Put(RR sealed)], WriteIdentity]` (identity gains the pending root `RAND1 ‖ u32 2 ‖ R_2`, device.md §6.4.7; D2); then socket append `0x02 ‖ r ‖ "TDA1" ‖ 1 ‖ u32 1182 ‖ FR`.
12. **Relay** P1–P7; T2 `122 = 121 + 1`; seq 201, `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ H(FR))`; result `new (201, C201)` to Desk; push of 201 to Desk and Mac.
13. **Relay** Mac's append `0x02 ‖ r' ‖ "TDA1" ‖ 1 ‖ u32 670 ‖ FM`: Mac row current; P1–P7 (Mac's key); T2 `61 = 60 + 1` → seq 202, `C202`; result `new (202, C202)` to Mac; push of 202 to Desk and Mac. On Mac's socket the order is: push 201, result 202, push 202.
14. **Mac** push 201, classification with `M_200` (seal.md §14.2): steps 1–5 pass (`DESK`, `ctr 122`); 7 current; 8 `(1,1)`; 9 epoch 1 = `E`; 10 opens under `K1`; 11 unpadding (`U 988`, `P 1024`); 12 `based_on (200, C200)` equal; 13 `ctl` by a current device under the current epoch; 15 payload canonical, inner signature verifies under `DESK_SIGN`; 16 shape R; `DeviceRevoked`: author D ≠ target, `PHONE` current, a current device remains; `EpochRotated`: `2 = W.epoch + 1`, wraps name exactly `DESK`, `MAC`, `dev_R` once each ascending, no `purge_before` → `Accept`. Mac is a recipient: `OpenBase` with `info` rebuilt from its own `BID`, epoch 2, `MAC`, `MAC_HPKE`, and `aad` with `c_2` from the item (SEAL-WRAP-002) → `R_2`; `H(ctx("epoch") ‖ BID ‖ u32 2 ‖ R_2) = c_2` → no halt; `keys_changed`.
15. **Mac** `Persist[AppendLog[201], SetMarker{201, 201}, WriteIdentity (root epoch 2)]`; commit: `M_201`: `PHONE` `status revoked`, `status_seq 201`, `status_by DESK`; `epoch 2`, `epoch_seq 201`, `epoch_clock_ms = τ'`; `commitments += (2, 201, c_2)`; `ctl_seqs += 201`; Desk `last_ctr 122`. Membership effects: banner `DeviceRevoked` (author Desk + fingerprint, subject Phone); epoch gate: `RM` (epoch 1, sealed, not seen) blocks new seals (ENG-EPOCH-001); history record of 201. Board: no effect.
16. **Mac** result 202 → `RM.ack = (202, C202)`.
17. **Mac** push 202 with `M_201`, `E = 2`: steps 1–5 (`MAC`, `61 = 60 + 1`); 6 own dev, its record of `ctr 61` with `H(FM)` → no flag; 7 current; 8; 9 epoch 1 = `E − 1`, `E ≥ 2` → continue; 10 opens under `K1` (Mac holds `R_1`); 11; 12 `based_on (200, C200)` equal; 13 `cmd` accepted under the previous epoch (§11.1); 14 stale-epoch rule (§13.9): `cmd`; author current; `200 < epoch_seq 201`; `clock_ms − epoch_clock_ms ≤ 2,592,000,000` → `Accept`.
18. **Mac** persist 202; apply `rename I7` under `Ctx{202, 2, T0 − 5 s, 200}`: name stamp `(202, 2)`; no notice (name stamp ≤ 200). Echo check `RM` passes; `seen 202` → `Drop(RM)`; the epoch gate opens; later Mac seals under epoch 2.
19. **Desk** result 201 → ack; push 201 → its own entry `Accept` (the commitment check opens Desk's own wrap, root equals its pending `R_2`); `Persist[AppendLog, SetMarker, WriteIdentity (R_2 as root of epoch 2, pending removed)]`; membership as step 15; no banner on Desk (author); `seen` → `Drop(RR)`; operation done → `Notify(OpDone{Ok})`. Push 202 → `Accept` as step 17; rename applied; no notice.
20. **Phone**, `T0 + 1 h`: the app opens; `Connect`; stored token `gen 1` with `push` → socket upgrade with `Bearer`. Gate: MAC and board OK. Object: row `PHONE` `state 2` → `403 relay/revoked` JSON (relay.md §8.2, §11.1).
21. **Phone** transport: `RelayRefused{req None, 403 relay/revoked}`, then `Disconnected{unreachable false}` (device.md §10.6). Engine §11.9 → §24.4, `source: relay`: revoked; sends nothing; keeps log, outbox (`RP`), keys, notices; pending view still shows `RP`'s edit.
22. **Phone shows**: "the relay says this device was removed", action "re-pair and resubmit my unsent edits" (engine.md §24.4).
23. **Phone** at most one `Connect` per 3,600,000 ms of `mono_ms`; at each, the runtime logs in first (device.md §10.3 case 4): `POST …/login/nonce` with the stale token → `403 relay/revoked` → the `Connect` ends. Phone never reads 201 and never classifies its revocation.
    AMBIGUOUS: engine.md §24.4 says a revoked device "then only reads", and that a mistaken `relay` source "clears itself when the relay answers again", but relay.md §8.2 answers every request of a revoked row with `403 relay/revoked` (login included, §10.3); so a relay-revoked device can never read the log, never learn `DeviceRevoked` at 201 (source `log`, with the author and seq), and can never see the reason; the spec does not say what it shows beyond "the relay says".
24. **User** (Phone) may choose re-pair and resubmit: `ExportUnsent` → `Notify(Unsent{[RP's command], from_device_no 3, origin_state})` (§24.5); not taken here.

**State after the last step**

| Holder | Head | Counters | Epoch | Durable |
|---|---|---|---|---|
| Desk | `(202, C202)` | Desk 122, Mac 61, Phone 18 (revoked), `dev_R` 1 | 2 (`epoch_seq 201`) | `identity.tdk` roots 1, 2; log 201–202; `history.tdk` + 201; op record cleared |
| Mac | `(202, C202)` | same | 2 | `identity.tdk` roots 1, 2; banner `DeviceRevoked` unacknowledged in `meta.tdk`; outbox empty |
| Relay | `(202, C202)` | `PHONE` `state 2`, `gen 2`; Desk `last_ctr 122`; Mac 61 | — | all backup-read tokens revoked |
| Phone | `(200, C200)` | its `M_200` still shows itself current | 1 | outbox `RP` (unsealed); revoked state (source relay) in `meta.tdk` |

Mac's own frame: sealed under epoch 1 before the rotation, sequenced at 202, accepted by the stale-epoch rule on every device, applied; no re-seal. User saw: Desk the device gone from its list; Mac the banner "Desk removed Phone"; Phone the "removed" screen with re-pair.

## Scenario 5 — Relay rollback

World additions: the relay copy holds `log` 1..190, head `(190, C190)`; rows Desk `last_ctr 115`, Mac 57, Phone 17 (their `last_seq` ≤ 190), every `gen 1`; the blob `SNAP192` (uploaded after seq 192) is not in the copy. Outboxes empty. Frames 191..200 are in every device's log; none is a control entry (membership unchanged).

1. **Desk** socket reconnects at `T0` (`Connect`; token `gen 1` is valid in the copy): upgrade accepted; hello `0x01 ‖ session ‖ u32 0`.
2. **Relay** welcome: `head_seq 190`, `C190`, `last_ctr 115`, `last_seq q_D`, `relay_now`, `floor 1`, `purge_floor 1`, `state 1`, `caps 1`, `appends_left`, door, `has_fence 0` (93 bytes).
3. **Desk** §11.2: step 1 clock; step 2 `190 < L = 200` → `relay_behind`; chain at 190 equals `C190` → no fork; step 3 no ack (outbox empty); step 4 not fenced; step 5 runs only §11.3 step 1: `r = 115 < a = 121` → `relay_behind` (same kind). One alarm (§11.2 "one alarm per kind").
4. **Desk** records the alarm, `Persist[WriteMeta]` before its `Notify` (ENG-EVID-002):

   ```json
   {"evidence":{"relay_behind":{"local_head":{"chain":"<C200>","seq":200},"relay_head":{"chain":"<C190>","seq":190},"relay_last_ctr":115}},"id":<n>,"kind":"relay_behind","raised_ms":<T0>,"resolved":false}
   ```

   (device.md §6.5 mapping of `AlarmRecord`, engine.md §21.1; `ack`, `seq`, `author`, `code` absent.) Evidence stop (§21.2): no `Append`, nothing persisted from the relay, editing continues, no duty. Session stays `Connecting` (§11.1 "any | Welcome | a check fails | unchanged").
5. **Desk shows** one state with the evidence: relay behind — relay holds 190, this device 200 (and the relay's counter 115 against 121); repair under way.
6. **Desk** §21.3 restore tail: evidence `RelayBehind` and the relay's chain at 190 equals its own `C190` → it repairs. `ReadLog{191, 200}` from its durable log (≤ 64 seqs; total frame bytes ≤ 1,048,576 assumed; no control entry, so one request).
7. **Desk** → `POST …/admin/nonce` (Bearer), body `DESK` → nonce `N2`.
8. **Desk** → `POST …/admin/restore_tail` (relay.md §12.1, §12.9; seal.md §16.2):

   | Field | Width | Value |
   |---|---|---|
   | `signer` | 16 | `DESK` |
   | `nonce` | 32 | `N2` |
   | `sig` | 64 | `SIG(Desk key; "taskdeck/v1/admin\0"; vb8("restore_tail") ‖ BID ‖ DESK ‖ H(args) ‖ N2)` (127-byte message) |
   | `args_len` | 4 | 332 |
   | `args` | 332 | `u64 191 ‖ u32 10 ‖ H(F191) ‖ … ‖ H(F200)` |
   | `extra_len` | 4 | `32 + Σ (4 + len_s)` |
   | `extra` | | `C190 ‖ (u32 len_191 ‖ F191) ‖ … ‖ (u32 len_200 ‖ F200)` |

9. **Relay** A1–A6 (signer `DESK` current); `first_seq 191 = head 190 + 1`; `chain_before = C190 = head_chain`; for each frame in order: `H(F_s)` = signed hash; P1–P3; author row exists; `verifyStrict` under the row's key; `ctr` = author's `last_ctr + 1` counting earlier frames (Desk 116..121, Mac 58..60, Phone 18). Transaction: store each as §9.4 T5 without the budget (`seq` 191..200, chains recomputed = `C191..C200`), author rows once (`last_ctr` 121, 60, 18 with their `last_seq`); after commit push 191..200 to ready sockets (Desk); answer `{"head":{"seq":200,"chain":"<C200>"}}`.
10. **Desk** `AdminResult Ok{head (200, C200)}`: relay head = `L` with its chain, no ack to echo → evidence resolved, the stop ends (§21.3 step 5, ENG-EVID-005); `Persist[WriteMeta(alarm resolved true)]`. The push of 191..200 adds nothing (entries ≤ `L` with equal frames are ignored, §12.3 step 5; while the stop held, nothing is read).
11. **Desk** stop ended while `Connecting` → `Hello` at once (ENG-SESS-010) = `GET …/hello` (Bearer) → JSON `{"head":{"seq":200,"chain":"<C200>"},"last_ctr":121,"last_seq":<q'>,"appends_left":…,"relay_now":…,"floor":1,"purge_floor":1,"state":"open","door":"<hex>","push":true}`; checks pass; reconcile `a = h = r = 121`; head = `L` → `Live`.
    AMBIGUOUS: device.md §10.5 maps every `Hello` to `GET …/hello` over HTTPS, while Desk's socket is open and already welcomed; the spec does not say whether the welcome needed after the stop comes from HTTPS `hello` on a live socket or whether the socket must be reopened for a fresh `welcome`.
12. **User** (Desk), `T0 + 30 s`: quick-add "Pay rent". **Desk** compile `create_task{id I41, name "Pay rent", time_importance 1}` (77 bytes, as `edit.md` scenario 1); enqueue (`at_ms` 1,795,341,630,000, `based_on (200, C200)`), draws `RAND1[16]` entry_id; persist; `Notify(Enqueued)`.
13. **Desk** `Live`, no stop → seal `ctr = 1 + max(121, 121, w = 121) = 122`, draws `RAND2[24]` nonce; frame (670 bytes, layout of `edit.md` scenario 1 step 14 with `at_ms` `T0 + 30 s`); persist; socket append.
    (Had the stop still held at `T0 + 30 s`, the edit would be queued and shown, and sent after step 11; ENG-EVID-003.)
14. **Relay** P1–P7, T2 `122 = 121 + 1` → seq 201, `C201`; result `new`; push 201 to Desk.
15. **Desk** ack; push 201 → `Accept`; apply; `Drop`.
16. **Mac** reconnects at `T0 + 60 s`: hello → welcome `head (201, C201)`, `last_ctr 60`, `last_seq` (restored), `floor 1`. §11.2: `201 > L 200`; no ack; §11.3 `a = h = r = 60` → normal; `Read{after 200}` → page 201 → `Accept` (`based_on (200, C200)` equal) → apply "Pay rent"; `Live`. Mac raises no alarm: the relay never showed it less than its log.
17. **Phone** stays offline; on its return the relay holds everything it has (rows restored, `last_ctr 18`).

**State after the last step**

| Holder | Head | Counters | Epoch | Durable |
|---|---|---|---|---|
| Desk | `(201, C201)` | Desk 122, Mac 60, Phone 18, `dev_R` 1 | 1 | log 1..201 (retention); alarm `RelayBehind` resolved in `meta.tdk`; outbox ops of the edit |
| Mac | `(201, C201)` | same | 1 | log 201 added |
| Relay | `(201, C201)` | Desk `last_ctr 122`, Mac 60, Phone 18 | — | log rows 1..201 restored and extended; blob `SNAP192` lost and not re-uploaded (§21.3 "Blobs"); devices' `SnapRecord` of it stays, a later `compact` naming it gets `409 relay/no_snapshot` |
| Phone | `(200, C200)` | | 1 | unchanged |

User saw: Desk a "relay behind" alarm with the two heads, resolved within the repair; "Pay rent" on Desk and then on Mac; Mac and Phone no alarm.
