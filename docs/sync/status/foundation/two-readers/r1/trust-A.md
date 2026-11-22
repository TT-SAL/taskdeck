# Group trust, reader A

Notation used throughout. `ctx(x)` = ASCII `taskdeck/v1/x` ‖ `0x00` (seal.md §2, README §5.2); widths: `ctx("frame")`, `ctx("entry")`, `ctx("admin")`, `ctx("epoch")`, `ctx("chain")`, `ctx("login")` 18 bytes; `ctx("ctl")` 16; `ctx("wrap")` 17; `ctx("wrap-aad")` 21; `ctx("key-entry")` 22. `hex(X)` = lowercase hex of X (README §5.4). `ID_CP` = item id of "Call plumber" = 1 × 2^40 + 7 = 1,099,511,627,783. `T0` = 1,795,341,600,000. `R_1` = the epoch-1 root every device holds. `K1` = `K_entry,1` = `HKDF(salt = BID, ikm = R_1, info = ctx("key-entry") ‖ u32 1, L = 32)` (seal.md §6.5). `DEV_R` = `H(ctx("recovery-dev") ‖ BID ‖ RSIGN)[0..16]` (SEAL-KEY-006). `DOOR` = the board's 5-byte door (relay.md §5.3). `TOK_X` = device X's 96-character gen-1 token (relay.md §5.1).

---

## Scenario 4: revocation with rotation

Assumption (scenario, not spec): the content of Phone's queued edit is not given; I take a quick-add "Buy stamps" without due date. Phone's next item number is 6.

### Steps

**Before T0**

1. **Phone** (T0 − 1 h, app open, no network). User quick-adds "Buy stamps". Facade compiles against the pending view (DEV-FACADE-001; core.md §13.4 `quick_add`: undated → `time_importance = 1`), id = 3 × 2^40 + 6 = 3,298,534,883,334. Command bytes (core.md §9.4, canonical encoding §4.2, keys sorted, `blocks` default `[]` omitted), 79 bytes:
   `{"id":3298534883334,"name":"Buy stamps","op":"create_task","time_importance":1}`
2. **Phone**. Engine `Enqueue{at_ms = 1,795,338,000,000}`: refusal checks of ENG §9.1 step 1 pass (not revoked by an accepted entry); trial apply with `ctx = {seq 201, device_no 3, at_ms, based_on 200}` (§9.1 step 3) succeeds. Draws **RAND1[16]** = `entry_id_P` (`Sealer::new_entry_id`, ENG-ENQ-003).
3. **Phone**. `Persist(Outbox[Put(record)])` (§9.1 step 4, D1): `outbox.tdo` append (device.md §7.2), op `0x01 ‖ vb32(record JSON)` (device.md §6.4.4). Record JSON (device.md §6.5, canonical; `ack`, `ctr`, `epoch`, `frame`, `frame_hash`, `op`, `seen` absent):
   `{"at_ms":1795338000000,"based_on":{"chain":"hex(C200)","seq":200},"body":{"cmd":"base64(cmd bytes)"},"entry_id":"hex(RAND1)","hold":false,"kind":"cmd","order":o_P,"origin_state":"base64(u16 3 ‖ u64 7 ‖ u64 nb_P)","reseals":0,"via":"append"}`
   After `Persisted`: `Notify(Enqueued{provisional_seq 201})`; the user sees the task. The record is never sealed: the session is not `Live` (ENG-SESS-001, ENG-SEND-002). The app is then closed (world).
4. **Mac** (T0 − 5 s). User renames `ID_CP` to "Call the plumber"; compile `rename{id, name}` (core.md §13.4 row `rename`, §9.7). Command, 60 bytes:
   `{"id":1099511627783,"name":"Call the plumber","op":"rename"}`
5. **Mac**. `Enqueue{at_ms = 1,795,341,595,000}`, trial apply `ctx = {seq 201, device_no 2, at_ms, based_on 200}` succeeds; draws **RAND2[16]** = `entry_id_M`; `Persist(Outbox[Put])` record as step 3 with `at_ms 1795341595000`, `based_on (200, C200)`, `origin_state` = `u16 2 ‖ u64 10 ‖ u64 nb_M` (device.md §8.3); `Notify(Enqueued{provisional_seq 201})` (ENG §9.1 steps 3–5).
6. **Mac**. Send loop (ENG §10.2): session `Live`, epoch gate open (§24.2), `ctr = next_ctr = 1 + max(60, h, w) = 61` (§10.3). `Sealer::seal(M_200, 61, entry)` under epoch 1 (SEAL-ENV-003). Draws **RAND3[24]** = nonce (SEAL-GEN-012).

   Plaintext (seal.md §10.1), `U = 69 + 60 = 129`, `P(129) = 512` (§10.2):

   | Field | Width | Value |
   |---|---|---|
   | `entry_id` | 16 | RAND2 |
   | `kind` | 1 | 1 (`cmd`) |
   | `at_ms` | 8 | 1,795,341,595,000 |
   | `based_on_seq` | 8 | 200 |
   | `based_on_chain` | 32 | C200 |
   | `payload_len` | 4 | 60 |
   | `payload` | 60 | the rename bytes of step 4 |
   | `padding` | 383 | `0x00` × 383 |
   | total | 512 | |

   Frame `F_M` (seal.md §8.1, §9.1), `len = 30 + 512 + 16 = 558`, total 670:

   | Offset | Field | Width | Value |
   |---|---|---|---|
   | 0 | `magic` | 4 | `"TDF1"` |
   | 4 | `board_id` | 16 | BID |
   | 20 | `dev` | 16 | MAC |
   | 36 | `ctr` | 8 | 61 |
   | 44 | `len` | 4 | 558 |
   | 48 | `ver` | 1 | 1 |
   | 49 | `suite` | 1 | 1 |
   | 50 | `epoch` | 4 | 1 |
   | 54 | `nonce` | 24 | RAND3 |
   | 78 | `ciphertext` | 528 | `AEAD(K1; RAND3; AD_M; plaintext)` |
   | 606 | `sig` | 64 | `SIG(MAC_SIGN; "taskdeck/v1/frame\0"; F_M[0..606])` |

   `AD_M` (SEAL-ENV-002), 72 bytes: `ctx("entry")[18] ‖ F_M[0..48] ‖ u8 1 ‖ u8 1 ‖ u32 1`. Signed bytes (SEAL-FRAME-002): `ctx("frame")[18] ‖ F_M[0..606]`, 624 bytes.
7. **Mac**. `Persist(Outbox[Put(record with ctr 61, frame F_M, frame_hash H(F_M), epoch 1)])` (D2, ENG-SEND-003); on `Persisted`, `Append{req, [F_M]}` (§10.2 step 5). On the socket (device.md §10.5; relay.md §11.3, §9.2), 684 bytes:

   | Field | Width | Value |
   |---|---|---|
   | type | 1 | `0x02` |
   | `req` | 4 | r_M |
   | `magic` | 4 | `"TDA1"` |
   | `n` | 1 | 1 |
   | `len_0` | 4 | 670 |
   | `frame_0` | 670 | F_M |

   The message is in flight until after step 17 (scenario fact).

**T0, Desk revokes**

8. **User** on Desk: Settings → devices → Phone → "Remove" (lost, so no replacement), then device authentication (Windows Hello / Touch ID) immediately before the call (device.md §17.1 "auth", §17.8 `revoke`, DEV-FACADE-003).
9. **Desk**. Facade `revoke(PHONE, false)` → `Op{ticket t, Revoke{dev: PHONE, replace: false}}` (device.md §17.8). Engine checks (ENG §28.5): PHONE is current in `M_K`, not Desk, not the last current device → accepted; Desk is joined, current, not halted, not stopped, no other op (ENG §28).
10. **Desk**. `Persist(WriteMeta(meta with ops += OpRecord))` before the relay call (D10, ENG-OPS-003). OpRecord JSON (device.md §6.5): `{"attempts":0,"op":{"revoke":{"dev":"hex(PHONE)","replace":false}},"stage":1,"started_ms":1795341600000,"ticket":t}`. `meta.tdk` replaced (device.md §7.2). Then `Admin{req, Revoke{dev: PHONE}}` (ENG §28.5 step 1, ENG-OPS-002).
11. **Desk → Relay**. `POST https://relay.example/v1/b/hex(BID)/admin/nonce`, headers `Host`, `User-Agent: TaskDeck-Sync/1`, `Authorization: Bearer TOK_DESK`, `Content-Length: 16`, `Content-Type: application/octet-stream`, `Accept-Encoding: identity` (device.md §10.2, §10.5 last `Admin` row). Body: `signer[16]` = DESK.
12. **Relay**. Gate §6.2 steps 0–7 (route `admin-nonce`, fixed 16, T1 accepted, MAC and board verify), then object: §8.2 token row DESK `role 1`, `state 1`, `gen 1` ✓; §12.1 `admin-nonce`: signer = token's dev, row current, board `open`. Transaction: delete nonces with `at < now − 120,000`; insert `(nonce, purpose 2, DESK, now)` with **RAND4[32]** from `crypto.getRandomValues` (§10.3 step 1 effects, RELAY-ADMIN-001). Response 200 (fields in table order, relay.md §4.2): `{"nonce":"hex(RAND4)","expires":now+120000}`.
13. **Desk**. Signs (seal.md §16.2, `sign_admin`, device.md §10.5), 121 bytes:

    | Field | Width | Value |
    |---|---|---|
    | context | 18 | `ctx("admin")` |
    | `op` | 1 + 6 | `vb8("revoke")` |
    | `board_id` | 16 | BID |
    | `signer` | 16 | DESK |
    | `args_hash` | 32 | `H(PHONE)` (`args` = `dev[16]` = PHONE, 16 bytes) |
    | `relay_nonce` | 32 | RAND4 |

    `sig_rv = SIG(DESK_SIGN; "taskdeck/v1/admin\0"; vb8("revoke") ‖ BID ‖ DESK ‖ H(PHONE) ‖ RAND4)` (SEAL-SIG-003).
14. **Desk → Relay**. `POST …/admin/revoke`, `Authorization: Bearer TOK_DESK`, body 136 bytes (relay.md §12.1, fixed 136):

    | Offset | Field | Width | Value |
    |---|---|---|---|
    | 0 | `signer` | 16 | DESK |
    | 16 | `nonce` | 32 | RAND4 |
    | 48 | `sig` | 64 | sig_rv |
    | 112 | `args_len` | 4 | 16 |
    | 116 | `args` | 16 | PHONE |
    | 132 | `extra_len` | 4 | 0 |

15. **Relay**. Common checks (RELAY-ADMIN-002): A1 lengths, args width 16; A2 board `open`; A3 T1 allowed, signer = token's dev; A4 nonce row `(RAND4, 2, DESK)` fresh, deleted now; A5 DESK row current, a current device other than the target; A6 `verifyStrict(DESK_SIGN, msg, sig_rv)` (RELAY-GEN-009); A7 §12.4: PHONE row role 1 `state 1` ✓, signer ≠ target ✓, a current role-1 row remains (DESK, MAC) ✓. One transaction: PHONE `state = 2`, `gen = 2`, `wraps = NULL`; every `read_tokens` row `state = 2` (RELAY-REVOKE-001, -003, -005; §5.2). After commit: close PHONE's sockets with 4003 (none open). Response 200 `{}`.
16. **Desk**. `AdminResult{Ok}`. Stage 2: `WriteMeta(stage 2)` and enqueue `CtlIntent::Rotate{revoke: [PHONE], reinstate: [], purge_before: None}` with `at_ms = wall_ms = T_R` (T_R ≥ T0), `based_on = (200, C200)`, no trial apply (ENG §9.2, §28.5 step 2); draws **RAND5[16]** = `entry_id_D`; `Persist(WriteMeta, Outbox[Put])`; record body JSON `{"ctl":{"rotate":{"reinstate":[],"revoke":["hex(PHONE)"]}}}`, `via "append"`, `op t`.
    AMBIGUOUS: ENG §28.5 and ENG-OPS-002 say the `revoke` is "sent before" the rotation is enqueued, not that stage 2 waits for its success; I wait for the 200 (which is why `at_ms = T_R` is after the admin round trip). Also open: whether the stage write and the outbox `Put` share one `Persist`.
17. **Desk**. Send loop (§10.2 step 4): `ctr = 1 + max(121, 121, w) = 122`; `Sealer::seal(M_200, 122, entry)` composes the items from `M_200` at this moment (ENG §6.1 `seal`): working copy W revokes PHONE, so the rotation wraps to the current devices of W, DESK and MAC, and to the current recovery identity DEV_R (seal.md §13.6 `EpochRotated`). Draws, in this order: **RAND6[32]** = `R_2` (SEAL-KEY-010); **RAND7[32]**, **RAND8[32]**, **RAND9[32]** = the HPKE ephemeral keys of the three wraps, in ascending recipient-id order; **RAND10[24]** = envelope nonce.
    AMBIGUOUS: neither seal.md §7/§8–§12 nor ENG §6.1 fixes the order of the random draws inside one `seal` call (root, HPKE ephemerals, nonce); this order is my choice.

    `c_2 = H(ctx("epoch")[18] ‖ BID[16] ‖ u32 2 ‖ R_2[32])` (70-byte input, SEAL-KEY-011).

    Each wrap (seal.md §7), for recipient `(id, pk)` ∈ {(DESK, DESK_HPKE), (MAC, MAC_HPKE), (DEV_R, RHPKE)}:
    - `info` (85 bytes): `ctx("wrap")[17] ‖ BID[16] ‖ u32 2 ‖ id[16] ‖ pk[32]`
    - `aad` (61 bytes): `ctx("wrap-aad")[21] ‖ u8 1 ‖ u8 1 ‖ u16 0x0020 ‖ u16 0x0001 ‖ u16 0x0003 ‖ c_2[32]`
    - `(enc, ct) = HPKE(pk; info; aad; R_2)`; wrap (96 bytes): `id[16] ‖ enc[32] ‖ ct[48]`.

    The three wraps are listed ascending by `recipient_id` as unsigned bytes; call them `W_a`, `W_b`, `W_c` in that order (their order depends on the byte values of DESK, MAC, DEV_R).

    `items_bytes` (canonical encoding, 772 bytes; members sorted by key, seal.md §12.4, §12.5, `purge_before` absent):
    `[{"dev":"hex(PHONE)","type":"DeviceRevoked"},{"commit":"hex(c_2)","epoch":2,"type":"EpochRotated","wraps":["hex(W_a)","hex(W_b)","hex(W_c)"]}]`
    (item 1: 65 bytes; item 2: 704 bytes.) Shape R (seal.md §12.1).

    Inner signature (seal.md §11.3), signed bytes 112:

    | Field | Width | Value |
    |---|---|---|
    | context | 16 | `ctx("ctl")` |
    | `board_id` | 16 | BID |
    | `dev` | 16 | DESK |
    | `ctr` | 8 | 122 |
    | `entry_id` | 16 | RAND5 |
    | `at_ms` | 8 | T_R |
    | `H(items_bytes)` | 32 | H(items_bytes) |

    `sig_ctl = SIG(DESK_SIGN; "taskdeck/v1/ctl\0"; BID ‖ DESK ‖ u64 122 ‖ RAND5 ‖ i64 T_R ‖ H(items_bytes))`.

    Payload (919 bytes): `{"items":<items_bytes>,"sig":"hex(sig_ctl)"}`.

    Plaintext, `U = 69 + 919 = 988`, `P(988) = 1024`:

    | Field | Width | Value |
    |---|---|---|
    | `entry_id` | 16 | RAND5 |
    | `kind` | 1 | 2 (`ctl`) |
    | `at_ms` | 8 | T_R |
    | `based_on_seq` | 8 | 200 |
    | `based_on_chain` | 32 | C200 |
    | `payload_len` | 4 | 919 |
    | `payload` | 919 | as above |
    | `padding` | 36 | `0x00` × 36 |
    | total | 1024 | |

    Frame `F_D` (`len = 30 + 1024 + 16 = 1070`, total 1182), sealed under epoch 1 (SEAL-ENV-004):

    | Offset | Field | Width | Value |
    |---|---|---|---|
    | 0 | `magic` | 4 | `"TDF1"` |
    | 4 | `board_id` | 16 | BID |
    | 20 | `dev` | 16 | DESK |
    | 36 | `ctr` | 8 | 122 |
    | 44 | `len` | 4 | 1070 |
    | 48 | `ver` | 1 | 1 |
    | 49 | `suite` | 1 | 1 |
    | 50 | `epoch` | 4 | 1 |
    | 54 | `nonce` | 24 | RAND10 |
    | 78 | `ciphertext` | 1040 | `AEAD(K1; RAND10; ctx("entry") ‖ F_D[0..48] ‖ u8 1 ‖ u8 1 ‖ u32 1; plaintext)` |
    | 1118 | `sig` | 64 | `SIG(DESK_SIGN; "taskdeck/v1/frame\0"; F_D[0..1118])` |

    The sealer keeps `R_2` as a pending root under `entry_id_D` (ENG §6.1 `seal`), so the seal reports a changed secret.
18. **Desk**. One `Persist`: `Outbox[Put(record with ctr 122, frame F_D, frame_hash H(F_D), epoch 1)]`, `WriteIdentity` (identity export with `n_pending = 1`: `RAND5 ‖ u32 2 ‖ R_2`, device.md §6.4.7) (D2, ENG-SEALER-004). On `Persisted`: socket message `0x02 ‖ u32 r_D ‖ "TDA1" ‖ u8 1 ‖ u32 1182 ‖ F_D` (1196 bytes).
19. **Relay**. Socket append (RELAY-SOCK-006/007): re-reads DESK row (current, gen 1 = attachment). §9.3 P1 (1182 in range, `"TDF1"`, `len` 1070 = 1182 − 112), P2 board, P3 ctr 122, P4 row DESK, P5 dev = token's, P6 `state 1`, P7 `verifyStrict(DESK_SIGN, ctx("frame") ‖ F_D[0..1118], F_D[1118..])`. Transaction §9.4: T1 row unchanged; T2 `122 = 121 + 1`; T4 count < 3,000; T5 `seq = 201`, `at = max(now, at_200)`, `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ H(F_D))` (90-byte input, RELAY-STORE-003), insert log row `(201, DESK, 122, H(F_D), C201, at_201, F_D)`, DESK `last_ctr 122`, `last_seq 201`, `app_count + 1`.
20. **Relay → Desk**. `result` (51 bytes): `0x82 ‖ u32 r_D ‖ "TDR1" ‖ u8 1 ‖ u8 0 (new) ‖ u64 201 ‖ C201`. **Relay → Desk, Mac** (every ready socket, author included, RELAY-SOCK-008): `push` (1274 bytes):

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `0x84` |
    | `magic` | 4 | `"TDP1"` |
    | `head_seq` | 8 | 201 |
    | `head_chain` | 32 | C201 |
    | `relay_now` | 8 | now |
    | `floor` | 8 | 1 |
    | `purge_floor` | 8 | 1 |
    | `flags` | 1 | 0 |
    | `n` | 2 | 1 |
    | entry `seq` | 8 | 201 |
    | entry `at` | 8 | at_201 |
    | entry `flen` | 4 | 1182 |
    | entry `frame` | 1182 | F_D |

    AMBIGUOUS: relay.md §10.6/§11.4 say the append is "answered by `result`" and the stored entries are pushed after the commit, but not which of the two messages goes first on the author's socket; I send `result` first.

**Desk processes 201**

21. **Desk**. `AppendResults`: `new (201, C201)` → `ack = (201, C201)`, `Put` in the next `Persist` (ENG §10.4).
22. **Desk**. Push page (ENG §12.3): limits ✓; `relay_now` → clock (§27.1); head checks ✓; contiguity 201 = L + 1; dry run: `C201 = chain_step(C200, 201, F_D)`; classify with `M_200` (seal.md §14.2): step 1–4 ✓ (DESK current, signature); 5 `122 = 121 + 1`; 6 own dev: `own_sealed(122) = H(F_D)` → no flag; 7 current; 8 (1,1) allowed by rules 1; 9 epoch 1 = E; 10 AEAD opens under K1; 11 unpad ✓; 12 `based_on 200 < 201`, `C200` matches; 13 ctl by a device, current epoch; 15 payload canonical, inner signature verifies; 16 shape R; `DeviceRevoked`: author D other than target, PHONE current, a current device remains; `EpochRotated`: `2 = 1 + 1`, wraps name exactly DESK, MAC, DEV_R ascending, no `purge_before`; 17 Accept. Desk is a recipient: opens its wrap with `info`/`aad` rebuilt from its own `board_id`, epoch 2, DESK, DESK_HPKE and `commit` (SEAL-WRAP-002, SEAL-RCPT-007), checks `H(ctx("epoch") ‖ BID ‖ u32 2 ‖ R_2) = c_2` ✓; `keys_changed`.
    AMBIGUOUS: seal.md and ENG §6.1 do not say when a pending root becomes an ordinary root (on the own accepted rotation, or only via the own wrap as for any recipient); I take: the opened root is stored as `(2, R_2)` and the pending entry `RAND5` is removed.
23. **Desk**. One `Persist` (§12.3 step 8): `AppendLog([{seq 201, chain C201, tau T_R, frame F_D}])` (log page record `u16 1 ‖ u64 201 ‖ C201 ‖ i64 T_R ‖ u32 1182 ‖ F_D`, 1236 bytes, device.md §6.4.1), the store's history record for 201 (device.md §8.3, DEV-RUN-003; seal.md §21.5: `u64 201 ‖ DESK ‖ u64 122 ‖ RAND5 ‖ i64 T_R ‖ H(F_D) ‖ vb32(items_bytes) ‖ sig_ctl`, 928 bytes), `SetMarker{201, 201, attempt}`, `WriteIdentity` (roots `(1, R_1), (2, R_2)`, `n_pending 0`).
24. **Desk**. On `Persisted` (§12.3 step 9): `L = K = 201`, `M_201` committed: PHONE `status revoked`, `status_seq 201`, `status_by DESK`; `epoch 2`, `epoch_seq 201`, `epoch_clock_ms = clock_ms = T_R`; commitments += `(2, 201, c_2)`; `ctl_seqs += 201`; DESK `last_ctr 122` (seal.md §13.5, §13.6). No banner on the author device (ENG §14.2). Board applies 201 (ctl, no board effect); the record's `entry_id` matches → `seen = 201`, `Outbox[Drop(RAND5)]`; `B = 201`; `ClearMarker`. Op stage 3: accepted → `Notify(OpDone{t, Ok})`, op removed from `meta.ops` (`WriteMeta`). Rebase (§8.4 case 1). Desk's device list shows Phone "removed".

**Mac processes 201 while its frame is in flight**

25. **Mac**. Push page of step 20: `L = 200`, entry 201 = L + 1; dry run classifies with `M_200` exactly as step 22 (DESK is not Mac: own-dev check not applicable); Mac is a recipient: opens its wrap with `info = ctx("wrap") ‖ BID ‖ u32 2 ‖ MAC ‖ MAC_HPKE`, `aad` as step 17 with `c_2` from the item, checks the root against `c_2` ✓ → learns `R_2`, `keys_changed` (SEAL-RCPT-007, ENG §24.1).
26. **Mac**. One `Persist`: `AppendLog(201)` as step 23, history record 201, `SetMarker{201, 201, a}`, `WriteIdentity` (roots 1 and 2) (D3, D4).
27. **Mac**. On `Persisted`: `M_201` committed (as step 24). Membership effects (ENG §14.1): `DeviceRevoked` → banner; `EpochRotated` → no `KeysRotated` banner (the entry has a revoke item); epoch gate (§24.2): `M_K.epoch 2` > epoch 1 of Mac's sealed, unseen record → Mac seals nothing new until that record is seen (ENG-EPOCH-001). Banner (ENG §14.2), persisted in `Meta` by `WriteMeta`, then `Notify(BannersChanged)`:
   `{"acked":false,"at_ms":T_R,"author":"hex(DESK)","author_fp":"<kfp(DESK_SIGN,DESK_HPKE) text>","author_name":"Desk","detail":"none","id":[201,0],"kind":"device_revoked","seq":201,"subject":"hex(PHONE)","subject_fp":"<kfp(PHONE_SIGN,PHONE_HPKE) text>","subject_name":"Phone"}`
   (`kfp` text: Crockford base32 of `H(ctx("key-fp") ‖ sign_pk ‖ hpke_pk)[0..10]` in four groups, seal.md §6.8.) Board applies 201 (nothing), `B = 201`, `ClearMarker`. Rebase: the rename record is applied on top at provisional seq 202 (§8.2). Mac's screen: banner "Desk (key XXXX…) removed Phone (key YYYY…)"; the rename still shown.

**Mac's frame reaches the relay**

28. **Relay**. Mac's append (step 7) arrives. Re-read MAC row (current, gen 1). P1–P7 for F_M ✓ (the relay does not look at the epoch, relay.md §9.1). T1 ✓; T2 `61 = 60 + 1`; T4 ✓; T5 `seq 202`, `C202 = H(ctx("chain") ‖ C201 ‖ u64 202 ‖ H(F_M))`, log row `(202, MAC, 61, H(F_M), C202, at_202, F_M)`, MAC `last_ctr 61`, `last_seq 202`.
29. **Relay → Mac** `result` `0x82 ‖ u32 r_M ‖ "TDR1" ‖ u8 1 ‖ u8 0 ‖ u64 202 ‖ C202` (51 bytes); **Relay → Desk, Mac** `push` (762 bytes): as step 20 with `head (202, C202)`, entry `(202, at_202, 670, F_M)`.
30. **Mac**. `AppendResults`: `new (202, C202)` → `ack = (202, C202)`, `Put`; `202 > L` so the echo check waits (§10.4, §11.5).
31. **Mac**. Push 202, dry run, classify with `M_201` (`E = 2`): steps 1–5 ✓ (MAC current, `61 = 60 + 1`); 6 `own_sealed(61) = H(F_M)` → no flag; 7 current; 8 ✓; 9 epoch 1 = `E − 1`, `E ≥ 2` → passes; 10 opens under K1 (Mac still holds `R_1`); 11 ✓; 12 `b = 200 < 202`, `C200` matches; 13 `cmd` by a current device, previous epoch allowed for `cmd` (seal.md §11.1); 14 stale-epoch rule (seal.md §13.9, SEAL-MEM-018): kind `cmd` ✓; author a current device ✓; `based_on_seq 200 < M_201.epoch_seq 201` ✓; `M_201.clock_ms − M_201.epoch_clock_ms = T_R − T_R = 0 ≤ 2,592,000,000` ✓; 17 **Accept**. Bookkeeping: MAC `last_ctr 61`, `clock_ms = max(T_R, 1,795,341,595,000) = T_R`.
32. **Mac**. `Persist(AppendLog(202: C202, tau T_R, F_M), SetMarker{202,202,a})`; on `Persisted`: echo check (ENG-ECHO-001): frame at 202 has `frame_hash H(F_M)`, `C202` = ack chain ✓. Board: `parse` the payload (core.md §9.1) ✓; `apply` rename under `Ctx{seq 202, device_no 2, at = 1,795,341,595,000 ms, based_on 200}` (ENG §13.1): name ← "Call the plumber", name stamp ← (202, 2); the old name stamp is Desk's at a seq ≤ 200 ≤ `based_on`, so no overwrite notice (core.md §10.1, §10.2). `seen = 202` → `Outbox[Drop(RAND2)]` (§12.4 step 3); `B = 202`; `ClearMarker`; the epoch gate opens (§24.2). Rebase: the pending view equals the confirmed state. Mac's own frame is thus kept, not re-sealed (ENG §24.3 applies only to a refusal).
33. **Desk**. Push 202: classifies exactly as step 31 (MAC current in `M_201`; stale-epoch rule passes) → Accept; `Persist(AppendLog(202), SetMarker)`; applies the rename at 202 with `ctx.device_no 2`; Desk's name stamp (≤ 200) is not after `based_on 200` → no notice; `B = 202`. Desk shows "Call the plumber".

**T0 + 1 h, Phone returns**

34. **Phone**. User opens the app (network up). `Device::open`: draws session id **RAND11[16]** from `OsEntropy` (device.md §10.4); `Start` with the outbox record of step 3 (ENG-STATE-003); attempt raised (`WriteMeta`, D5). Pending view shows "Buy stamps", status `queued 1` ("· 1 waiting"). `set_foreground(true)` from the shell (DEV-NET-017).
35. **Phone**. Session `Offline` → `Connect{relay.example, DOOR, 1}` (ENG §11.1). Transport (device.md §10.5): first `Connect` after open, no backoff (§10.7); a stored token exists and the engine is not stopped `Revoked` → no login (device.md §10.3 cases 1–4 do not apply); stored capability `push`, foreground → socket upgrade `GET wss://relay.example/v1/b/hex(BID)/socket` with `Host`, `User-Agent: TaskDeck-Sync/1`, `Authorization: Bearer TOK_PHONE` (gen 1) and the upgrade headers (DEV-NET-004).
36. **Relay**. Gate passes (MAC and board verify; generation not checked at the gate, relay.md §6.3). Object §8.2 token role 1: PHONE row exists, `role 1`, `state = 2` → refused `403 relay/revoked`, JSON `{"error":"relay/revoked"}`, `X-TD-Relay: object`, no row written (RELAY-OBJ-003, -004, RELAY-SOCK-001).
37. **Phone**. Transport: `403 relay/revoked` answering a whole request → `RelayRefused{req: None, 403 relay/revoked}` (device.md §10.6). Engine (ENG §11.9 row 2 → §24.4 case 2): revoked with `source: relay`; stop `Revoked{by_log: false}` persisted in `Meta` (`WriteMeta`; `StopRecord` JSON, `alarm` absent: `{"kind":{"revoked":{"by_log":false}},"since_ms":1795345200000+ε}`), no alarm, nothing deleted (ENG-SESS-005, ENG-EPOCH-002). Session → `Offline`.
    AMBIGUOUS: device.md §10.6 gives `RelayRefused` for a refused whole request but, unlike the login row, does not say whether a `Disconnected` follows a refused socket upgrade inside a `Connect`, nor what `req` the refusal carries; I take `req: None` then `Disconnected{unreachable: false}`.
38. **Phone**. What the user sees (ENG §24.4, §30.1): the sync line with the stop "the relay says this device was removed" and the action "re-pair and resubmit my unsent edits" (`StopAction::RePair`); "Buy stamps" still on screen, not marked "will be refused", `queued 1`. The record stays unsealed; no `Append`, `Admin` or blob request (ENG-EPOCH-004). New edits are still accepted (ENG §9.1 step 1: a relay-only revocation does not refuse).
39. **Phone**. Next `Connect` no earlier than 3,600,000 ms of `mono_ms` later (ENG-EPOCH-004): it logs in first (device.md §10.3 case 4): `POST …/login/nonce` body PHONE with `Bearer TOK_PHONE` → relay §10.3 step 1: row role 1 ✓, `state 1` ✗ → `403 relay/revoked`, no nonce drawn, no row written → `RelayRefused{req: None}`, `Disconnected` (device.md §10.6 login row); engine stays revoked. Phone never reads seq 201 (every token route answers `403`), so the revocation stays `source: relay`.

### State at the end

| Holder | State |
|---|---|
| Relay | log 1–202; head `(202, C202)`; DESK `last_ctr 122`, `last_seq 201`, gen 1; MAC `61`, `202`, gen 1; PHONE `state 2`, `gen 2`, `wraps NULL`, `last_ctr 18`; DEV_R role 2 `last_ctr 1`; every `read_tokens` row `state 2`; nonce RAND4 deleted; floors 1; `home_gen 1` |
| Desk | `L = K = B = 202`, chain `C202`; `M_202`: epoch 2 (`epoch_seq 201`, `epoch_clock_ms T_R`), PHONE revoked (`status_seq 201`, `status_by DESK`), DESK `last_ctr 122`, MAC `61`, PHONE `18`, recovery `last_ctr 1`, `ctl_seqs` += 201, commitments `(1,1,c_1), (2,201,c_2)`; identity roots `R_1`, `R_2`, no pending; outbox: Put/Put/Put(ack)/Drop of RAND5, no live record; `history.tdk` += 201; no banner; op finished |
| Mac | same `M_202`, head `(202, C202)`, `B = 202`; identity roots `R_1`, `R_2`; outbox Put/Put/Put(ack)/Drop of RAND2, empty; `history.tdk` += 201; banner `(201, 0)` `DeviceRevoked` unacknowledged |
| Phone | head `(200, C200)`, `B = 200`, `M_200`, epoch 1, own `last_ctr 18`; outbox: one live unsealed `cmd` record RAND1 ("Buy stamps"); `meta.stops = [Revoked{by_log: false}]`; token gen 1 (stale; kept, DEV-NET-012) |
| User saw | Desk: Phone shown removed, operation done; Mac: banner "Desk removed Phone" with both key fingerprints, rename kept as "Call the plumber" (also on Desk); Phone: "the relay says this device was removed" + "re-pair and resubmit my unsent edits", its edit still visible and waiting |

---

## Scenario 5: relay rollback

Facts derived from the world: the relay's rows are those of the moment it held seq 190, so its counters are DESK `last_ctr 115`, MAC `57`, PHONE `17`, DEV_R `1`, every `gen 1`; `compact_floor = purge_floor = purge_done = 1` (relay.md §10.2 create, no compaction or purge). `q_X` = the seq of device X's frame at that counter. Seqs 191–200 hold DESK ctr 116–121, MAC 58–60, PHONE 18. I take none of 191–200 to be a control entry (the world has no membership change); outboxes are empty, so no device holds an acknowledgement.

### Steps

**T0, Desk's session**

1. **Desk**. Session `Offline` → `Connect{relay.example, DOOR, 1}` (ENG §11.1). Transport (device.md §10.5): stored token, capability `push`, foreground → `GET wss://relay.example/v1/b/hex(BID)/socket`, `Authorization: Bearer TOK_DESK`, `User-Agent: TaskDeck-Sync/1`, upgrade headers.
2. **Relay**. Gate ✓; object §8.2: DESK row role 1, `state 1`, `gen 1` = token ✓; board `open`; `acceptWebSocket(server, ["hex(DESK)"])`, attachment `{dev, gen 1, features 0, ready false}`; 101 (RELAY-SOCK-001).
3. **Desk → Relay** hello (21 bytes): `0x01 ‖ SESSION_D[16] ‖ u32 0` (relay.md §11.3; same process, so the session id drawn at its `open`, device.md §10.4). Transport delivers `Connected{push: true}` after the welcome arrives (device.md §10.5).
4. **Relay**. Hello is 21 bytes, no session yet; no other socket of DESK has a session; stores `session`, `features 0`, `ready true`; sends welcome (93 bytes):

   | Field | Width | Value |
   |---|---|---|
   | `type` | 1 | `0x81` |
   | `head_seq` | 8 | 190 |
   | `head_chain` | 32 | C190 |
   | `last_ctr` | 8 | 115 |
   | `last_seq` | 8 | q_D115 |
   | `relay_now` | 8 | ≈ T0 |
   | `floor` | 8 | 1 |
   | `purge_floor` | 8 | 1 |
   | `state` | 1 | 1 |
   | `caps` | 1 | 1 |
   | `appends_left` | 4 | 3,000 − DESK's restored `app_count` (3,000 if its `app_day` is not today) |
   | `door` | 5 | DOOR |
   | `has_fence` | 1 | 0 |

5. **Desk**. Checks on the welcome (ENG §11.2): 1 `relay_now` → skew ≈ 0 (§27.1); 2 `head.seq 190 < L 200` → evidence `relay_behind`; `head.seq ≤ L` and Desk's `C190` equals `head.chain` → no fork; 3 no record has an `ack` (outbox empty); 4 not fenced; 5 reconcile (§11.3): `a = 121`, `r = 115 < a` → evidence `relay_behind` (step 1). The welcome's check failed → session state unchanged (`Connecting`), the stop of §21 (§11.1 last-but-one row).
   AMBIGUOUS: ENG §11.2 step 2 and §11.3 step 1 each raise `relay_behind` from the same welcome; the spec does not say whether that is one alarm or two. I record one alarm carrying both the head and `last_ctr`.
6. **Desk**. Records the evidence (ENG §21.1, ENG-EVID-002): `Persist(WriteMeta(meta with alarms += A, stops += S))`, then `Notify(AlarmRaised{id})`, `Notify(StatusChanged)`.
   - `AlarmRecord` JSON (device.md §6.5; `ack`, `author`, `code`, `seq` absent): `{"evidence":{"relay_behind":{"local_head":{"chain":"hex(C200)","seq":200},"relay_head":{"chain":"hex(C190)","seq":190},"relay_last_ctr":115}},"id":k,"kind":"relay_behind","raised_ms":T0+ε,"resolved":false}`
   - `StopRecord` JSON: `{"alarm":k,"kind":"evidence","since_ms":T0+ε}`
7. **Desk**. Under the evidence stop (ENG §21.2): no `Append`; no read and no persist from the relay; edits still accepted; no duty except the repair. Shows (ENG §30.2 "alarm evidence"): the sync line with the stop "relay is behind this device: relay at 190, this device at 200 (counter 115 vs 121)" and the action `RestoreTail`; it cannot be silenced (ENG-UI-002).
8. **Desk**. Restore tail (ENG §21.3, ENG-EVID-004; duty "any device holding more", §29.3; allowed outside `Live`, ENG-SESS-001, ENG-DUTY-002): condition holds (relay's chain at its head `C190` = Desk's). One request: frames 191–200 (10 ≤ 64; assumed ≤ 1,048,576 bytes), `chain_before = C190`; no control entry inside, so no repairs (§21.3 step 2). `Admin{req, RestoreTail{first_seq 191, chain_before C190, frames F191…F200}}`.
9. **Desk → Relay**. `POST …/admin/nonce`, `Bearer TOK_DESK`, body DESK (16) (device.md §10.5).
10. **Relay**. As scenario 4 step 12: nonce row `(RAND1, 2, DESK, now)` with **RAND1[32]**; `{"nonce":"hex(RAND1)","expires":now+120000}`.
11. **Desk**. `args` (332 bytes, seal.md §16.2 `restore_tail`): `u64 191 ‖ u32 10 ‖ H(F191) ‖ … ‖ H(F200)`. Admin message (127 bytes):

    | Field | Width | Value |
    |---|---|---|
    | context | 18 | `ctx("admin")` |
    | `op` | 1 + 12 | `vb8("restore_tail")` |
    | `board_id` | 16 | BID |
    | `signer` | 16 | DESK |
    | `args_hash` | 32 | `H(args)` |
    | `relay_nonce` | 32 | RAND1 |

    `sig_rt = SIG(DESK_SIGN; "taskdeck/v1/admin\0"; vb8("restore_tail") ‖ BID ‖ DESK ‖ H(args) ‖ RAND1)`.
12. **Desk → Relay**. `POST …/admin/restore_tail`, `Bearer TOK_DESK`, body `524 + Σ len_i` bytes (relay.md §12.1, §12.9):

    | Offset | Field | Width | Value |
    |---|---|---|---|
    | 0 | `signer` | 16 | DESK |
    | 16 | `nonce` | 32 | RAND1 |
    | 48 | `sig` | 64 | sig_rt |
    | 112 | `args_len` | 4 | 332 |
    | 116 | `args` | 332 | as step 11 |
    | 448 | `extra_len` | 4 | `32 + Σ (4 + len_i)` |
    | 452 | `chain_before` | 32 | C190 |
    | 484 | frames | `Σ (4 + len_i)` | `u32 len_191 ‖ F191 ‖ … ‖ u32 len_200 ‖ F200`, from Desk's log unchanged |

13. **Relay**. A1–A6 as scenario 4 step 15 (op `restore_tail`, T1, signer a current device); A7 (relay.md §12.9, RELAY-RESTORE-001): `first_seq 191 = head_seq 190 + 1` ✓; `chain_before = head_chain C190` ✓; for each frame i in order: `H(F_i)` = signed hash; P1–P3; author row exists; `verifyStrict` under its `sign_pk` (DESK_SIGN, MAC_SIGN or PHONE_SIGN); `ctr = last_ctr + 1` counting earlier frames of the request (DESK 116…121, MAC 58…60, PHONE 18) ✓. One transaction: each frame stored as §9.4 T5 without the budget: `C_s = H(ctx("chain") ‖ C_{s−1} ‖ u64 s ‖ H(F_s))` for s = 191…200, which yields `C200`; author rows written once: DESK `121`/`q_D121`, MAC `60`/`q_M60`, PHONE `18`/`q_P18`; no role, state or key change (RELAY-RESTORE-002). After commit: push of entries 191–200 to every ready socket (Desk's), one page, `more` 0. Response 200: `{"head":{"seq":200,"chain":"hex(C200)"}}`.
14. **Desk**. `AdminResult{Ok(head (200, C200))}`: the relay head reaches `L` with Desk's chain and no acknowledgement is outstanding → evidence resolved, stop ends (ENG §21.3 step 5, ENG-EVID-005): `Persist(WriteMeta(alarm k resolved true, stop removed))`, `Notify(StatusChanged)`; the alarm leaves the status. The push of 191–200 holds only entries at or below `L` equal to the log's → ignored (§12.3 step 5).
15. **Desk**. To get back to a reconciled session it sends `Hello`: `GET …/hello`, `Bearer TOK_DESK` (ENG §4.3 `Hello`, device.md §10.5). Relay answer (relay.md §10.4, table order): `{"head":{"seq":200,"chain":"hex(C200)"},"last_ctr":121,"last_seq":q_D121,"appends_left":n,"relay_now":t,"floor":1,"purge_floor":1,"state":"open","door":"hex(DOOR)","push":true}`. Welcome checks pass (head 200 = L, chain equal); reconcile `r = 121 = a`, normal; no read needed; session `Live`.
    AMBIGUOUS: ENG §11.1 has no transition out of `Connecting` after a welcome that raised evidence once the stop has ended (§21.3 step 5 only ends the stop), and no transition to `Live` when a welcome's head already equals `L` (the `Reconciling → Live` row needs a `Page`); I send a `Hello` and go `Live` on its welcome.
    AMBIGUOUS: ENG §11.3 defines `h` as "the largest counter of a record with a frame" but not its value when no record holds a frame (here, and for Mac in step 21); read literally (`h = 0`), `r = a = 121 > h` would trigger the restored-copy path of step 3. I take `h = a` when no record holds a frame, so the reconcile is the normal case.

**T0 + 30 s, the new edit**

16. **Desk**. User quick-adds "Pay rent" (no due date). Compile (core.md §13.4 `quick_add`), id `1 × 2^40 + 41` = 1,099,511,627,817. Command (77 bytes):
    `{"id":1099511627817,"name":"Pay rent","op":"create_task","time_importance":1}`
    `Enqueue{at_ms 1,795,341,630,000}`; trial apply `ctx {seq 201, device_no 1, at_ms, based_on 200}` ✓; draws **RAND2[16]** = entry id; `Persist(Outbox[Put])` (record JSON as scenario 4 step 3, `origin_state = u16 1 ‖ u64 42 ‖ u64 nb_D`); `Notify(Enqueued{provisional_seq 201})`.
    (Had the restore not finished by now, the edit would wait in the outbox, shown pending, and go out once the stop ended, ENG §21.2 item 3.)
17. **Desk**. Send loop: `Live`, no stop; `ctr = 1 + max(121, h, 121) = 122`; seal under epoch 1, draws **RAND3[24]** nonce; plaintext `U = 69 + 77 = 146`, `P = 512`, padding 366; frame `F201` (670 bytes): `"TDF1" ‖ BID ‖ DESK ‖ u64 122 ‖ u32 558 ‖ u8 1 ‖ u8 1 ‖ u32 1 ‖ RAND3 ‖ AEAD(K1; RAND3; ctx("entry") ‖ F201[0..48] ‖ u8 1 ‖ u8 1 ‖ u32 1; plaintext)[528] ‖ SIG(DESK_SIGN; "taskdeck/v1/frame\0"; F201[0..606])`. Plaintext fields: `RAND2 ‖ u8 1 ‖ i64 1,795,341,630,000 ‖ u64 200 ‖ C200 ‖ u32 77 ‖ payload ‖ 0x00 × 366`. `Persist(Outbox[Put(frame)])` (D2), then socket append `0x02 ‖ u32 r ‖ "TDA1" ‖ u8 1 ‖ u32 670 ‖ F201` (684 bytes).
18. **Relay**. P1–P7, T1–T5: `122 = 121 + 1` (the restored row) → `new`, seq 201, `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ H(F201))`; DESK `last_ctr 122`, `last_seq 201`, `app_count + 1`. `result` `0x82 ‖ r ‖ "TDR1" ‖ u8 1 ‖ u8 0 ‖ u64 201 ‖ C201`; `push` `(201, C201)` with entry 201 to Desk (Mac not connected).
19. **Desk**. `ack = (201, C201)`; push 201: classify own frame (own_sealed(122) = H(F201)), Accept; `Persist(AppendLog(201), SetMarker)`; echo ✓; apply `create_task` under `Ctx{201, 1, T0+30 s, 200}`; `seen`, `Drop`; `B = 201`.

**T0 + 60 s, Mac's session**

20. **Mac**. `Connect`: socket with `Bearer TOK_MAC`, hello `0x01 ‖ SESSION_M ‖ u32 0`. Relay checks as steps 2 and 4; welcome: `head (201, C201)`, `last_ctr 60`, `last_seq q_M60` (restored row), `relay_now`, `floor 1`, `purge_floor 1`, `state 1`, `caps 1`, `appends_left`, `DOOR`, `has_fence 0` (93 bytes).
21. **Mac**. Welcome checks: `head.seq 201 > L 200` → no `relay_behind`, no fork check at 201; no acks; reconcile `a = 60 = r` → normal (h as AMBIGUOUS of step 15); `Read` because head > L → session `Reconciling`. Socket read (15 bytes): `0x03 ‖ u32 r ‖ u64 200 ‖ u16 256`.
22. **Relay**. Read (relay.md §10.5): rows `seq > 200`, `≥ purge_floor` → [201]. `page` message `0x83 ‖ u32 r ‖ "TDP1" ‖ u64 201 ‖ C201 ‖ i64 relay_now ‖ u64 1 ‖ u64 1 ‖ u8 0 ‖ u16 1 ‖ u64 201 ‖ i64 at_201 ‖ u32 670 ‖ F201` (762 bytes); no row written (RELAY-READ-003).
23. **Mac**. Page: contiguous; `C201 = chain_step(C200, 201, F201)` = page head chain ✓; classify (DESK current, `122 = 121 + 1`, epoch 1, `based_on 200` with `C200` matching) → Accept; `Persist(AppendLog(201), SetMarker)`; apply the `create_task` (`ctx.device_no 1`); `B = 201`; run ended → rebase; session `Live`. Mac never saw the rollback.

### State at the end

| Holder | State |
|---|---|
| Relay | log 1–201 again, head `(201, C201)`; DESK `122`/201, MAC `60`/`q_M60`, PHONE `18`/`q_P18`, DEV_R `1`/1, every `gen 1`; floors 1; nonce RAND1 deleted; the blobs table is the restored copy's |
| Desk | `L = K = B = 201`, `C201`; `M_201` epoch 1, DESK `last_ctr 122`; outbox: Put/Put/Put(ack)/Drop of RAND2, empty; `meta.alarms` holds alarm k `relay_behind` with `resolved: true`; no stop; session `Live` |
| Mac | `L = B = 201`, `C201`, epoch 1, outbox empty, no alarm, `Live` |
| Phone | unchanged, offline: head `(200, C200)`; its seq-191–200 frame is back on the relay at its original seq |
| User saw | Desk: briefly the stop "relay is behind this device (190 vs 200)" with the action "restore tail", which cleared itself; "Pay rent" added and synced; Mac: "Pay rent" appears, nothing else |

AMBIGUOUS: blobs are outside the rollback repair. The snapshot `SNAP192` was uploaded after seq 192 was sequenced, so the restored copy (to seq 190) very likely lacks it, yet every device's registry still holds the `snap` entry and Mac's acknowledgement in 191–200 that make it "usable" (ENG §19.3). ENG §21.3 and relay.md §12.9 only repair log rows and device rows; nothing says a device re-uploads a blob that the rollback lost, so a later pairing, rejoin or `compact` relying on SNAP192 would meet `relay/no_blob` or `relay/no_snapshot`.
