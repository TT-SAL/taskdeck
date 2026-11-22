# join — reader B

Notation. `ctx(x)` = ASCII `taskdeck/v1/x` + one `0x00` byte, written in full below as `"taskdeck/v1/x\0"`. `HKDF(salt; ikm; info; L)`. `MAC(k; x)` = HMAC-SHA256. `kfp(sign, hpke)` = Crockford of `H("taskdeck/v1/key-fp\0" ‖ sign ‖ hpke)[0..10]` (seal §6.8). `C_s` chain values; `DOOR` = `HMAC(TOKEN_KEY; "taskdeck/v1/door\0" ‖ BID)[0..5]` (RELAY-DOOR-001). `R_1` the epoch-1 root, `c_1` its commitment. `T_DESK`, `T_MAC` the gen-1 device tokens (96-char text, relay §5.1). `K_entry,1 = HKDF(BID; R_1; "taskdeck/v1/key-entry\0" ‖ u32 1; 32)`, `K_snap,1 = HKDF(BID; R_1; "taskdeck/v1/key-snap\0" ‖ u32 1; 32)` (SEAL-KEY-012). Every sealed local file record is `u32 ct_len ‖ nonce[24] ‖ AEAD(K_file(kind); nonce; "taskdeck/v1/file\0" ‖ header[55] ‖ u64 i; plaintext)` under the 55-byte header `"TDL1" ‖ 1 ‖ 1 ‖ u8 kind ‖ dev ‖ BID ‖ file_id[16]` (seal §19.1, §19.3); `K_file(k) = HKDF(BID; L_d; "taskdeck/v1/file-key\0" ‖ u8 k ‖ dev; 32)` (SEAL-KEY-014). Relay JSON keys are in the relay table's order (relay §4.2, RELAY-ENC-004); replicated JSON is canonical, keys sorted (core §4.2, SEAL-ENC-003).

World assumptions used by both scenarios (the world does not fix them): device names `"Desk"`, `"Mac"`, `"Phone"`; the only control entries are seq 1 (Genesis), `a_M` (DeviceAdded Mac), `a_P` (DeviceAdded Phone), all below 100; no beat of Desk is due during the scenario; the relay's `compact_floor` is 1 in scenario 3. Under these, the canonical membership bytes of a 3-device state are 810 bytes (seal §13.2: 84 + 1 + 15 + (4+44) + (2+104) + (2 + 173 + 172 + 174) + 1 + 1 + 1 + 4 + (4 + 3×8)).

- AMBIGUOUS: engine §11.3 and §10.3 define `h` as "the largest counter of a record with a frame"; when the outbox holds no framed record (the normal idle state of Desk, Mac and Tab here) `h` is undefined, and with `h = 0` the case `r > h` of §11.3 step 3 (restored copy) would fire on every normal welcome. I take `h = max(a, largest framed ctr)`.

## Scenario 3: pairing

Values: `s` = RAND1, `tid`, `ticket_key`, `mac_key` below; head before pairing `(200, C200)`; Desk `last_ctr` 121; `next_device_no` 4.

1. **User** (Desk): opens "add a device", passes device authentication (Windows Hello or Touch ID) and calls `pair_start()` (device §17.10, DEV-FACADE-003, §18.3).
2. **Desk**: starts `pair::Existing` with `Start{M_200, head (200, C200), relay origin "https://relay.example"}`; guard: Desk current and unhalted (seal §17.10 E table, `Idle` row; engine refusals `engine/not_eligible`, `engine/halted`, `engine/pair_busy` not met).
3. **Desk**: draws, in order (seal §17.10 `Idle` row, §17.2, §17.5): `RAND1[10]` = `s` (pairing secret); `RAND2[32]` = `esk_E` (ephemeral X25519 secret; `epk_E` its public key); `RAND3[32]` = `r_E` (commitment opening).
4. **Desk**: computes (seal §17.2, §17.3, §17.5):

   | Value | Input bytes (width) | Output |
   |---|---|---|
   | `check` | `"taskdeck/v1/pair-code\0"`(22) ‖ `s`(10) = 32 | first 10 bits of `H(..)` |
   | code | Crockford(`s`) 16 chars ‖ Crockford(`check`) 2 chars | display `XXXX-XXXX-XXXX-XXXX-CC` |
   | `ticket_key` | | `HKDF(salt=BID; ikm=s; info="taskdeck/v1/pair-ticket\0"(24); L=32)` |
   | `tid` | `"taskdeck/v1/pair-tid\0"`(21) ‖ `ticket_key`(32) = 53 | `H(..)[0..16]` |
   | `mac_key` | | `HKDF(salt=BID; ikm=s; info="taskdeck/v1/pair-mac\0"(21); L=32)` |
   | `commit` | `"taskdeck/v1/pair-commit\0"`(24) ‖ `epk_E`(32) ‖ `r_E`(32) = 88 | `H(..)` |

5. **Desk**: builds slot A (seal §17.4, §17.5; 84 bytes):

   | Field | Width | Value |
   |---|---|---|
   | label | 1 | `0x41` |
   | role | 1 | `0x45` |
   | body_len | 2 | 48 |
   | body: `dev_E` | 16 | `DESK` |
   | body: `commit` | 32 | `commit` |
   | mac | 32 | `MAC(mac_key; "taskdeck/v1/pair\0"(17) ‖ 0x41 ‖ 0x45 ‖ tid(16) ‖ u16 48 ‖ body(48))` (115-byte input) |

6. **Desk → Relay**: `POST https://relay.example/v1/b/<BID hex>/pair` (`pair-open`), headers `Host`, `User-Agent: TaskDeck-Sync/1`, `Authorization: Bearer T_DESK`, `Content-Length: 100`, `Content-Type: application/octet-stream`, `Accept-Encoding: identity` (device §10.2, §18.3 `RegisterTicket`; relay §10.8). Body (100): `tid[16] ‖ slot A[84]`.
7. **Relay**: gate §6.2 steps 0–7 (route `pair-open`, fixed length 100, token MAC and board), body read (step 9), object: token row DESK role 1, state 1, gen 1 (relay §8.2); board open; slot A is `0x41 ‖ 0x45 ‖ u16 48 ‖ 48 ‖ 32`; deletes expired tickets; no ticket `tid`; fewer than 4 tickets. Writes `tickets(tid, creator=DESK, expires=now+600,000, tries=0, a=slot A)` (relay §10.8). Answers 200 `{"expires":<now+600000>}`.
8. **Desk**: `pair_start` returns `PairSheet{address_display, code_display}`; the pairing sheet shows the relay address in display form `relay.example/XXXX-…-XX` (Crockford of `BID ‖ DOOR`, 34 chars, 8 groups of 4 and one of 2; relay §5.4) and the code, with the main window excluded from capture (DEV-PAIR-001, device §18.3 `ShowCode`).
9. **User** (Tab, fresh install): opens TaskDeck, the Pair screen (bridge §13.1); pastes or types the relay address, types the code one character at a time into `CodeField` (bridge §13.3), the name `"Tab"`, taps Join; the shell calls `callSecret join` → `join(address, code, "Tab")` (device §17.10; no authentication needed).
10. **Tab**: checks the lock, the cloud folder, a key store, mode `LocalOnly`; parses the address by relay §5.4 steps 1–6 (host `relay.example`, `board_id = BID`, `door = DOOR`); normalises the code, requires 18 characters and checks the 10 check bits (SEAL-CODE-002, DEV-PAIR-003) — all before any request.
11. **Tab**: `pair::New` `CodeEntered{code, host, BID, DOOR}`; derives `ticket_key`, `tid`, `mac_key` exactly as step 4 (SEAL-PAIR-001); `FetchSlot{tid, ticket_key, A}` (seal §17.10 N table).
12. **Tab → Relay**: `GET …/v1/b/<BID>/pair/<tid hex>/a`, `Authorization: Door <DOOR hex>`, `X-TD-Proof: <ticket_key hex>` (device §18.3; relay §10.8 `slot-get`).
13. **Relay**: gate step 7: door equals `HMAC(TOKEN_KEY; "taskdeck/v1/door\0" ‖ BID)[0..5]` (constant time, RELAY-DOOR-002) and `H("taskdeck/v1/pair-tid\0" ‖ ticket_key)[0..16] = tid` (RELAY-GATE-008, SEAL-PAIR-015); object: ticket present and unexpired; slot `a` present → 200, `application/octet-stream`, the 84 bytes of slot A. No row written (RELAY-PAIR-006).
14. **Tab**: verifies slot A's MAC in constant time with label `0x41`, role `0x45` before reading the body (SEAL-PAIR-003); keeps `dev_E = DESK`, `commit`.
15. **Tab**: generates its long-term keys and ephemeral key (seal §17.1 step 4, §6.4, SEAL-KEY-008), in this order: `RAND4[16]` = `dev` `TAB`; `RAND5[32]` = Ed25519 private key (public `TAB_SIGN`); `RAND6[32]` = HPKE `DeriveKeyPair` ikm (public `TAB_HPKE`); `RAND7[32]` = `esk_N` (public `epk_N`). All kept in memory, zeroizing (device §18.3).
    - AMBIGUOUS: the draw order of the three long-term keys and `esk_N` is not fixed (seal §17.10 "generate long-term keys and `esk_N`"; §6.4 table order taken).
16. **Tab**: builds slot B (seal §17.4; body 121, total 157):

    | Field | Width | Value |
    |---|---|---|
    | label | 1 | `0x42` |
    | role | 1 | `0x4E` |
    | body_len | 2 | 121 |
    | `dev_N` | 16 | `TAB` |
    | `sign_pk_N` | 32 | `TAB_SIGN` |
    | `hpke_pk_N` | 32 | `TAB_HPKE` |
    | `epk_N` | 32 | `epk_N` |
    | `n_rules` | 1 | 1 |
    | rules | 4 | u32 1 |
    | name | 1 + 3 | `vb8("Tab")` = `0x03 "Tab"` |
    | mac | 32 | `MAC(mac_key; "taskdeck/v1/pair\0" ‖ 0x42 ‖ 0x4E ‖ tid ‖ u16 121 ‖ body(121))` |

17. **Tab → Relay**: `POST …/pair/<tid>/b`, `Door` + `X-TD-Proof`, `Content-Length: 157`, body slot B (device §18.3 `PostSlot{B}`).
18. **Relay**: gate as step 13; object: open; ticket unexpired; `tries` 0 → 1 (written even if a later step refuses); layout `0x42 ‖ 0x4E ‖ 119..242`, total = 36 + 121; not a repeat; `a` present, `b` absent → stores `b`. Answers 200 `{}`; after commit sends to every ready socket of DESK the slot notice `0x87 ‖ tid[16] ‖ 0x42` (18 bytes) (relay §10.8, §11.4).
19. **Tab**: `FetchSlot{C}`: `GET …/pair/<tid>/c` with `Door` + proof every 2,000 ms; `404 relay/slot_empty` until C is stored (device §18.3).
20. **Desk**: on the `slot` notice, `GET …/pair/<tid>/b` with `Bearer T_DESK`; relay checks the token's `dev` is the ticket's creator and answers the 157 bytes (relay §10.8 `slot-get`).
21. **Desk**: checks slot B in order (seal §17.6): MAC, label B, role N; layout and limits (`n_rules` 1, ascending, name 1–64 bytes); `TAB`, `TAB_SIGN`, `TAB_HPKE` differ from every `dev`, `dev_R`, `sign_pk`, `hpke_pk` of `M_200`; rules list contains 1; 3 current devices < 16 and `next_device_no` 4 ≤ 8191. All pass.
22. **Desk**: builds slot C (seal §17.4; 100 bytes): `0x43 ‖ 0x45 ‖ u16 64 ‖ epk_E[32] ‖ r_E[32] ‖ MAC(mac_key; "taskdeck/v1/pair\0" ‖ 0x43 ‖ 0x45 ‖ tid ‖ u16 64 ‖ body(64))`; `POST …/pair/<tid>/c` with `Bearer T_DESK` (SEAL-PAIR-005: after B verified).
23. **Relay**: open; ticket unexpired; token `dev` = creator; layout `0x43 ‖ 0x45 ‖ 64`, total 100; `b` present, `c` absent → stores `c`; 200 `{}` (relay §10.8).
24. **Desk**: computes (seal §17.7):
    - `dh = X25519(esk_E, epk_N)`, non-zero (SEAL-GEN-013);
    - `T = H(transcript input)`, input 440 bytes:

      | Field | Width | Value |
      |---|---|---|
      | context | 28 | `"taskdeck/v1/pair-transcript\0"` |
      | origin | 2 + 21 | `vb16("https://relay.example")` |
      | `board_id` | 16 | `BID` |
      | `tid` | 16 | `tid` |
      | `dev_E` | 16 | `DESK` |
      | slot A | 84 | as posted, MAC included |
      | slot B | 157 | as posted |
      | slot C | 100 | as posted |

    - `sas = HKDF(salt=s; ikm=dh; info="taskdeck/v1/sas\0"(16) ‖ T(32); L=8)`; `SAS6` = `u64be(sas) mod 1,000,000`, 6 digits with leading zeros.
25. **Desk**: `ShowSas{SAS6}`: `status().pairing = ShowSas{digits}`; Desk shows the six digits (device §18.3).
26. **Tab**: the poll `GET …/pair/<tid>/c` now answers the 100 bytes of slot C.
27. **Tab**: verifies slot C's MAC (label `0x43`, role `0x45`); checks `H("taskdeck/v1/pair-commit\0" ‖ epk_E ‖ r_E) = commit` in constant time (SEAL-PAIR-004); computes `dh = X25519(esk_N, epk_E)` (non-zero), `T` and `SAS6` as step 24 (its own origin is also `https://relay.example`); `AskSas`: `status().pairing = AskSas`.
28. **User**: reads the six digits on Desk and types them into Tab's SAS field; `join_sas(digits)` → `SasEntered`; Tab compares in constant time (SEAL-PAIR-007), equal on the first try → `ShowAccepted`: Tab shows "accepted" (`status().pairing = Accepted`); `FetchSlot{D}`: Tab polls `GET …/pair/<tid>/d` every 2,000 ms (`404 relay/slot_empty`).
29. **User**: confirms on Desk that Tab accepted: `pair_confirm()` → `UserConfirmed` → `NeedSnapshotAndRegister{device item}` → the device sends `PairRegister{ticket, NewDevice{TAB, TAB_SIGN, TAB_HPKE, "Tab", T}}` to the engine (seal §17.10 `Showing` row, SEAL-PAIR-008, device §18.3).
30. **Desk** (engine §28.4 step 1): session `Live`, current, unhalted, no other pairing → accepted. `h = B = 200`. Material: `{seq: 200, chain: C200, hash_version: 1, state_hash: SH200, board: BOARD200, membership: M200}` with `SH200 = H(state input)` (engine §18.3, 86 bytes):

    | Field | Width | Value |
    |---|---|---|
    | context | 18 | `"taskdeck/v1/state\0"` |
    | `hash_version` | 4 | 1 |
    | board hash | 32 | `H(BOARD200)` (canonical board bytes, core §6.13) |
    | membership hash | 32 | `H(M200)` (810 bytes) |

31. **Desk** (sealer `seal_snapshot(M_K, material)`, seal §20, SEAL-BLOB-005, SEAL-GEN-012): draws `RAND8[16]` = blob id `SNAPP`, then `RAND9[24]` = blob nonce. Snapshot body before compression (seal §20.4; length `INF_P` = 80 + 4 + `BL200` + 4 + 810 + 4 + Σhist):

    | Field | Width | Value |
    |---|---|---|
    | magic | 4 | `"TDSN"` |
    | `S` | 8 | 200 |
    | chain | 32 | `C200` |
    | `hash_version` | 4 | 1 |
    | `state_hash` | 32 | `SH200` |
    | board | 4 + `BL200` | `vb32(BOARD200)` |
    | membership | 4 + 810 | `vb32(M200)` |
    | history | 4 + Σ | `u32 3`, then the history records (seal §21.5) of seqs 1, `a_M`, `a_P`, each `seq ‖ dev ‖ ctr ‖ entry_id ‖ at_ms ‖ frame_hash ‖ vb32(items) ‖ sig` (156 + items bytes) |

    `CB` = raw DEFLATE of the body (length `CL`); padded = `u32 CL ‖ CB ‖ 0x00 × (P(4 + CL) − 4 − CL)` (seal §20.3, §10.2). Sealed bytes (`SZ_P = 67 + P(4 + CL)`, one part assumed: `PARTS_P = ceil(SZ_P / 1,048,576) = 1`):

    | Offset | Field | Width | Value |
    |---|---|---|---|
    | 0 | magic | 4 | `"TDB1"` |
    | 4 | ver | 1 | 1 |
    | 5 | suite | 1 | 1 |
    | 6 | blob_kind | 1 | 1 |
    | 7 | epoch | 4 | 1 |
    | 11 | blob_id | 16 | `SNAPP` |
    | 27 | nonce | 24 | `RAND9` |
    | 51 | ciphertext | `P(4+CL)` + 16 | `AEAD(K_snap,1; RAND9; AD_P; padded)` |

    `AD_P` (84 bytes) = `"taskdeck/v1/blob\0"`(17) ‖ `BID`(16) ‖ sealed[0..27] ‖ `u64 200` ‖ `DESK`(16). `SHA_P = H(sealed bytes)`.
32. **Desk → Relay** (`Blob{Upload{purpose: 2, seq: 200}}`, engine §28.4 step 1, device §10.5): `POST …/blobs` with `Bearer T_DESK`, body (65): `SNAPP[16] ‖ u8 2 ‖ u64 200 ‖ u64 SZ_P ‖ SHA_P[32]`.
33. **Relay**: lazy deletions of §13.2 (incomplete blobs and purpose-2 blobs older than 1 h); checks: open; `67 ≤ SZ_P ≤ 67,108,864`; purpose 2: `1 ≤ 200 ≤ head_seq 200`; no row `SNAPP`; fewer than 3 incomplete and byte quota. Inserts `blobs(SNAPP, slot = max+1, purpose 2, seq 200, author DESK, SZ_P, parts 1, SHA_P, received 0, complete 0, at now)`; 200 `{"id":"<SNAPP>","parts":1}` (relay §10.7).
34. **Desk → Relay**: `PUT …/blobs/<SNAPP>/0`, body the `SZ_P` sealed bytes; relay: author DESK, `i < 1`, length `SZ_P`; inserts `parts(slot×64 + 0)`, `received = 1`, `complete = 1`, retention §13.1 (purpose 2: nothing); 200 `{"received":1,"complete":true}`. Desk gets `BlobResult{Uploaded}`.
35. **Desk** (engine §28.4 step 2, §9.2): enqueues `CtlIntent::AddDevice{dev: TAB, sign_pk: TAB_SIGN, hpke_pk: TAB_HPKE, name: "Tab", transcript: T, bootstrap_seq: 200}`: draws `RAND10[16]` = `entry_id` `EID_A`; record `{entry_id EID_A, order, at_ms T_A (Desk wall_ms), based_on (200, C200), kind ctl, via Register{TAB, TAB_SIGN}, ctr none}`; `Persist[Outbox[Put]]` → one record appended to `outbox.tdo` (plaintext `u16 1 ‖ 0x01 ‖ vb32(record JSON)`, device §6.4.4), nonce `RAND11[24]`.
    - AMBIGUOUS: engine §28.4 lists the upload (step 1) before the enqueue (step 2) but does not say whether the enqueue waits for `BlobResult{Uploaded}`; I take it that it does.
36. **Desk** (send loop, engine §10.2 step 2, ENG-SEND-005): the first unacknowledged record has `via = Register`; seals it with `ctr = 1 + max(121, 121, 121) = 122` (§10.3) under `M_K = M_200`, epoch 1 = the snapshot's epoch (ENG-OPS-006). The sealer composes (engine §6.1 `seal`, seal §11.3, §12.3, SEAL-PAIR-010):
    - items (`items_bytes`, 332 bytes, canonical): `[{"bootstrap_seq":200,"dev":"<TAB hex32>","hpke_pk":"<TAB_HPKE hex64>","name":"Tab","sign_pk":"<TAB_SIGN hex64>","transcript":"<T hex64>","type":"DeviceAdded"}]`
    - inner signature `SIG_I = SIG(DESK signing key; "taskdeck/v1/ctl\0"; inner)`, inner (112 bytes):

      | Field | Width | Value |
      |---|---|---|
      | context | 16 | `"taskdeck/v1/ctl\0"` |
      | `board_id` | 16 | `BID` |
      | `dev` | 16 | `DESK` |
      | `ctr` | 8 | 122 |
      | `entry_id` | 16 | `EID_A` |
      | `at_ms` | 8 | `T_A` |
      | `H(items_bytes)` | 32 | |

    - payload (479 bytes): `{"items":[…as above…],"sig":"<SIG_I hex128>"}`
    - plaintext (`U = 548`, `P(548) = 1024`):

      | Offset | Field | Width | Value |
      |---|---|---|---|
      | 0 | entry_id | 16 | `EID_A` |
      | 16 | kind | 1 | 2 |
      | 17 | at_ms | 8 | `T_A` |
      | 25 | based_on_seq | 8 | 200 |
      | 33 | based_on_chain | 32 | `C200` |
      | 65 | payload_len | 4 | 479 |
      | 69 | payload | 479 | |
      | 548 | padding | 476 | `0x00` |

    - draws `RAND12[24]` = envelope nonce; frame `F201` (1,182 bytes, `len = 30 + 1024 + 16 = 1070`):

      | Offset | Field | Width | Value |
      |---|---|---|---|
      | 0 | magic | 4 | `"TDF1"` |
      | 4 | board_id | 16 | `BID` |
      | 20 | dev | 16 | `DESK` |
      | 36 | ctr | 8 | 122 |
      | 44 | len | 4 | 1070 |
      | 48 | ver | 1 | 1 |
      | 49 | suite | 1 | 1 |
      | 50 | epoch | 4 | 1 |
      | 54 | nonce | 24 | `RAND12` |
      | 78 | ciphertext | 1040 | `AEAD(K_entry,1; RAND12; AD; plaintext)`, `AD` (72) = `"taskdeck/v1/entry\0"` ‖ frame[0..48] ‖ `0x01 0x01` ‖ `u32 1` |
      | 1118 | sig | 64 | `SIG(DESK signing key; "taskdeck/v1/frame\0"; frame[0..1118])` |

    `FH201 = H(F201)`. `Persist[Outbox[Put(record with ctr 122, frame F201, frame_hash FH201, epoch 1)]]` (nonce `RAND13[24]`); the `Admin` waits for its `Persisted` (D2, ENG-SEND-003).
37. **Desk → Relay** (`Admin{Register{frame: F201, dev: TAB, sign_pk: TAB_SIGN}}`, engine §11.7, device §10.5): `POST …/admin/nonce` with `Bearer T_DESK`, body (16) `DESK`. **Relay**: signer is the token's `dev`, row current, board open; deletes expired nonces; draws `RAND14[32]` = admin nonce `AN`; inserts `nonces(AN, purpose 2, DESK, now)`; 200 `{"nonce":"<AN>","expires":<now+120000>}` (relay §12.1).
38. **Desk**: signs `SIG_ADM = SIG(DESK signing key; "taskdeck/v1/admin\0"; admin message)`, message (123 bytes, seal §16.2):

    | Field | Width | Value |
    |---|---|---|
    | context | 18 | `"taskdeck/v1/admin\0"` |
    | op | 1 + 8 | `vb8("register")` |
    | board_id | 16 | `BID` |
    | signer | 16 | `DESK` |
    | args_hash | 32 | `H(args)`, args (80) = `TAB[16] ‖ TAB_SIGN[32] ‖ FH201[32]` |
    | relay_nonce | 32 | `AN` |

    `POST …/admin/register` with `Bearer T_DESK`, body (1,382 bytes, relay §12.1): `DESK[16] ‖ AN[32] ‖ SIG_ADM[64] ‖ u32 80 ‖ args[80] ‖ u32 1182 ‖ F201[1182]`.
39. **Relay**: gate (route `admin` with op `register`, cap 262,344, T1); A1 lengths and args layout; A2 board open; A3 token `dev` = signer; A4 nonce `AN` found (purpose 2, DESK, fresh) and deleted whatever follows; A5 DESK current role 1; A6 `verifyStrict(DESK_SIGN, admin message, SIG_ADM)`; then §12.3: `TAB` and `TAB_SIGN` in no row; 3 current role-1 rows < 16, < 256 rows; `H(F201) = FH201`; frame `dev` = signer; §9.3 P1–P7 (length 1182, `"TDF1"`, `len` = 1182 − 112, board, ctr, author DESK, token's dev, state 1, `verifyStrict(DESK_SIGN, "taskdeck/v1/frame\0" ‖ F201[0..1118], sig)`); §9.4 T1, T2 (`122 = 121 + 1`), T4 (budget). One transaction: `log(seq 201, DESK, 122, hash FH201, chain C201, at, F201)` with `C201 = H("taskdeck/v1/chain\0"(18) ‖ C200(32) ‖ u64 201 ‖ FH201(32))` (90-byte input, SEAL-CHAIN-001); DESK row `last_ctr 122`, `last_seq 201`, `app_count + 1`; inserts `devices(TAB, TAB_SIGN, role 1, state 1, gen 1, last_ctr 0, last_seq 0, counters 0, added_at now)` (RELAY-REG-001). Token for TAB (relay §5.1, 69 bytes): `u8 1 ‖ BID ‖ TAB ‖ u32 1 ‖ MAC(TOKEN_KEY; "taskdeck/v1/token\0" ‖ token[0..37])`, text `T_TAB = "TD1." ‖ base64url` (96 chars). Answers 200 `{"seq":201,"chain":"<C201>","token":"<T_TAB>"}`.
40. **Relay → Desk, Mac** (sockets ready; Tab has none; Phone closed): after commit, push (relay §11.4, RELAY-SOCK-008), 1,274 bytes: `0x84 ‖ "TDP1" ‖ u64 201 ‖ C201 ‖ i64 relay_now ‖ u64 floor 1 ‖ u64 purge_floor 1 ‖ u8 flags 0 ‖ u16 1 ‖ (u64 201 ‖ i64 at ‖ u32 1182 ‖ F201)`.
    - AMBIGUOUS: relay §12.3 says "After commit, push" but not whether the push is sent before or after the HTTPS answer; the two reach Desk in either order (steps 41 and 42 commute).
41. **Desk** (engine §11.7, §10.4, §28.4 step 3): `AdminResult Ok{seq 201, chain C201, token: true}` → record `ack = (201, C201)` (Put in the next Persist); the transport keeps `T_TAB` for the pairing ticket (device §18.3, §24); `Notify(PairRegistered{ticket, head (200, C200), snapshot SnapInfo{seq 200, hash SH200, hash_version 1, blob SNAPP, sha256 SHA_P, size SZ_P, parts 1, inflated INF_P, epoch 1}, added_seq 201})`.
42. **Desk** (push, engine §12.3): `C201 = chain_step(C200, 201, F201)`; classify with `M_200` (seal §14.2): steps 1–5 pass (`ctr 122 = 121 + 1`), own-dev check finds its own `FH201` for ctr 122 (no flag), steps 7–11 pass, `based_on` 200 < 201 and `C200` matches (§14.8), ctl by a current device under `E = 1`, payload and inner signature valid, shape A, DeviceAdded preconditions (fresh keys, caps, `bootstrap_seq 200 ≥ purge_seq 0` and `< 201`) → `Accept`, `M_201` with record TAB (`device_no 4`, current, `added_seq 201`, `added_by DESK`, `bootstrap_seq 200`, `last_ctr 0`, `beat_ms T_A`, rules [1], envelopes [[1,1]], hash_versions [1], name "Tab"), `next_device_no 5`, `ctl_seqs` + 201, DESK `last_ctr 122`, `τ = max(τ, T_A)`. `Persist[AppendLog[(201, C201, τ, F201)], SetMarker{201, 201, attempt}]` plus the store's history record of 201 appended to `history.tdk` (device §8.3, DEV-RUN-003). On `Persisted`: commit; no banner (author device, engine §14.2); apply (nothing on the board); `seen = 201` → `Outbox[Drop(EID_A)]`; echo check: frame hash `FH201` and `C201` equal the ack (ENG-ECHO-001).
43. **Mac** (push): same classification (Mac's `M_200`, `C200`) → `Accept`; `Persist[AppendLog[201…], SetMarker]` + history record 201; commit: banner `DeviceAdded{id (201, 0), seq 201, at_ms T_A, author DESK, author_name "Desk", author_fp kfp(DESK_SIGN, DESK_HPKE), subject TAB, subject_name "Tab", subject_fp kfp(TAB_SIGN, TAB_HPKE)}` persisted in `Meta` (`WriteMeta`); Mac shows the banner and the device list with "Tab", device_no 4, no beat yet (engine §14.1, §14.2, ENG-BANNER-001, §30.2).
44. **Desk** (runtime → machine `Registered{h 200, chain C200, snapshot ref, added_seq 201, token T_TAB, roots [(1, R_1)], host "relay.example", door DOOR}`; only epoch 1 exists, so only its root; device §18.3, ENG-OPS-006): bundle JSON (`BL_J = 665 + d(INF_P) + d(SZ_P)` bytes, `d` = number of decimal digits; seal §17.8, canonical key order):

    `{"added_seq":201,"board_id":"<BID hex32>","door":"<DOOR hex10>","head":{"chain":"<C200 hex64>","seq":200},"host":"relay.example","roots":[{"epoch":1,"root":"<R_1 hex64>"}],"snapshot":{"blob":"<SNAPP hex32>","epoch":1,"hash":"<SH200 hex64>","hash_version":1,"inflated":INF_P,"parts":1,"sha256":"<SHA_P hex64>","size":SZ_P},"token":"<T_TAB>"}`

    `bundle_key = HKDF(salt=s; ikm=dh; info="taskdeck/v1/pair-bundle-key\0"(28) ‖ T(32); L=32)`; `AD_B` (88) = `"taskdeck/v1/pair-bundle\0"`(24) ‖ BID ‖ tid ‖ T; draws `RAND15[24]` = bundle nonce. Slot D (total `76 + BL_J`):

    | Field | Width | Value |
    |---|---|---|
    | label | 1 | `0x44` |
    | role | 1 | `0x45` |
    | body_len | 2 | `40 + BL_J` |
    | nonce | 24 | `RAND15` |
    | ciphertext | `BL_J + 16` | `AEAD(bundle_key; RAND15; AD_B; bundle JSON)` |
    | mac | 32 | `MAC(mac_key; "taskdeck/v1/pair\0" ‖ 0x44 ‖ 0x45 ‖ tid ‖ u16 body_len ‖ body)` |

    `POST …/pair/<tid>/d` with `Bearer T_DESK`.
45. **Relay**: open; ticket unexpired; creator; layout `0x44 ‖ 0x45 ‖ 40..16,040`; `c` present, `d` absent → stores `d`; 200 `{}`.
46. **Desk**: machine `Done{TAB}`; `status().pairing = Done`; the pairing sheet closes. The ticket stays until it expires (no `pair-close` on `Done`, device §18.3).
47. **Tab**: the poll `GET …/pair/<tid>/d` answers slot D's bytes.
48. **Tab** (seal §17.9 steps 1–3): MAC, label D, role E; `AEAD` opens under `bundle_key` with `AD_B`; the JSON parses strictly (canonical, every field, `roots` 1 object of epoch 1, `added_seq 201 > 200`), and `board_id`, `host`, `door` equal `BID`, `relay.example`, `DOOR` from step 10. `FetchSnapshot{T_TAB, SNAPP, 1}`.
49. **Tab → Relay** (device §18.3): `GET …/blobs/<SNAPP>` with `Bearer T_TAB` → relay: TAB row role 1, state 1, gen 1 (§8.2) → 200 `{"id":"<SNAPP>","purpose":2,"seq":200,"author":"<DESK>","size":SZ_P,"parts":1,"sha256":"<SHA_P>","received":1,"complete":true}`; then `GET …/blobs/<SNAPP>/0` → the `SZ_P` bytes.
50. **Tab** (seal §17.9 step 4 = §20.5 with `S = 200`, writer `DESK`, the bundle's values, `K_snap,1` from the bundle's `R_1`): 1 part of `SZ_P` bytes; `H(part) = SHA_P` (constant time, before any decryption); header `"TDB1"`, 1, 1, kind 1, epoch 1; AEAD opens with `AD_P`; padding valid with `U = 4 + CL`; inflate capped at `INF_P`, exactly `INF_P` bytes, no trailing input; body parses with no trailing bytes, `S = 200`, `state_hash = SH200`, membership parses, history (records 1, `a_M`, `a_P`) passes the projection of §21.6 with Tab's `BID` and agrees with the membership (devices, keys, statuses, recovery key, epochs and commitments, `ctl_seqs`).
51. **Tab** (step 5): body `chain = C200 = head.chain`; `M_200` parses, `board_id = BID`, DESK current, current epoch 1 = highest epoch of `roots`; `H("taskdeck/v1/epoch\0" ‖ BID ‖ u32 1 ‖ R_1) = c_1` of `M_200.commitments` (SEAL-KEY-013); recomputed `SH200` (step 30's 86 bytes) equals `snapshot.hash`. `ReadLog{T_TAB, from 201}`.
52. **Tab → Relay**: `GET …/log?after=200` with `Bearer T_TAB` (device §18.3 sends no `limit`; default 256). **Relay**: rows `seq > 200`, `≥ purge_floor 1` → page (71 + 20 + 1,182 = 1,273 bytes): `"TDP1" ‖ u64 201 ‖ C201 ‖ i64 relay_now ‖ u64 1 ‖ u64 1 ‖ u8 0 ‖ u16 1 ‖ (u64 201 ‖ i64 at ‖ u32 1182 ‖ F201)`.
53. **Tab** (step 6): `C201 = H("taskdeck/v1/chain\0" ‖ C200 ‖ u64 201 ‖ H(F201))`; classifies 201 from `M_200` with `retained_from = 200` and `C200` (seal §14) exactly as step 42 (`own_dev_foreign` impossible: author DESK) → `Accept`; the page head `(201, C201)` matches; step 7: the entry at `added_seq` 201 is accepted, authored by DESK, `based_on_seq 200`, one DeviceAdded naming `TAB`, `TAB_SIGN`, `TAB_HPKE`, `"Tab"`, `T`; step 8: `device_no = 4` from `M_201` (SEAL-PAIR-012). `Joined{device_no 4, keys, bundle, snapshot}`. Nothing was persisted before this (SEAL-PAIR-011, DEV-PAIR-002).
54. **Tab** (persisting the join, device §18.3, in order): `RAND16[32]` = `L_d` (OsEntropy, SEAL-GEN-005); `KeyStore::create(L_d)` through `AndroidKeyStore`, associated data `"taskdeck/v1/local-key\0"(22) ‖ BID ‖ TAB` (seal §6.6), which writes `ld.wrap`; the directory `sync/id-<BID hex>-<TAB hex>/`; then, each by replace (`.tmp`, `sync`, `rename`, `sync_dir`), each with a fresh `file_id` and one record nonce:
    - `store.tds` (54): `"TDS1" ‖ 1 ‖ BID ‖ TAB ‖ u8 3 ‖ i64 created_ms ‖ H(bytes 0..46)[0..8]` (device §6.2);
    - `identity.tdk` (kind 7, `RAND17[16]` file_id, `RAND18[24]` nonce): `"TDI1" ‖ vb16(DeviceKeys::to_identity_bytes) ‖ u16 1 ‖ (u32 1 ‖ R_1) ‖ u8 0 ‖ u8 0` (device §6.4.7);
    - `device.tdk` (kind 12, `RAND19[16]`, `RAND20[24]`): `{"device_name":"Tab","history_complete":true,"minter":{"block":MB,"device_no":4,"item":MI},"tokens":[{"gen":1,"host":"relay.example","token":"<T_TAB>"}]}` (device §16; no `push`, §10.3);
    - `bootstrap.tdk` (kind 17, `RAND21[16]`, `RAND22[24]`): `0x02 ‖ u64 200 ‖ C200 ‖ u32 1 ‖ SH200 ‖ vb32(BOARD200) ‖ vb32(M200) ‖ u64 201 ‖ vb8("relay.example") ‖ DOOR[5] ‖ u32 1` (device §6.4.9; `home_gen` of `M_200` is 1);
    - `history.tdk` (kind 11, append procedure, `RAND23[16]` file_id, one record per history entry: `RAND24[24]`, `RAND25[24]`, `RAND26[24]`): the snapshot's records of seqs 1, `a_M`, `a_P`, byte for byte (device §6.4.8, §15.7);
    - no plain file holds data (fresh install): nothing set aside (device §18.4);
    - `sync/active.tds` (45): `"TDA1" ‖ 1 ‖ BID ‖ TAB ‖ H(bytes 0..37)[0..8]`.
    - AMBIGUOUS: `device.tdk.tokens[].gen` is required, but the bundle carries the token only as opaque text (seal §17.8, SEAL-KEY-015); 1 written. Also `IdMinter::new(4)`'s `next_item`/`next_block` values (core §20) are not fixed: `MI`, `MB` symbolic (any value ≤ 1 gives the same id in step 64).
55. **Tab**: the facade reopens in synced mode (device §17.2, §18.3): draws `RAND27[16]` = session id (device §10.4); reads the directory (device §7.4); `Start{Loaded{bootstrap: Join{material at 200, added_seq 201, relay {relay.example, DOOR, 1}}}}` (engine §19.8). Engine: `attempt` raised (`WriteMeta`); loads `BOARD200` (`SM::load`), `SM::rules = 1 = M_200.rules`; `B = K = L = 200`, `C200`, `M_200`, `log_floor 201`, `retained_from 200`; `meta.relay` from the bundle; `Persist[WriteCheckpoint{200, C200, BOARD200, M200, log_floor 201, registry}, WriteBase{same}]` (engine §18.1 case 4, §19.8); the store removes `bootstrap.tdk` after the `WriteCheckpoint` (device §7.3).
    - AMBIGUOUS: engine §19.8 says "loads the snapshot at `h` as in §19.7 step 4", which includes `TruncateLogBelow(S + 1)`; whether a join emits it (setting `device.tdk.log_floor = 201`) or only takes `log_floor = h + 1` from the checkpoint (§7.10) is not said. I take: no `TruncateLogBelow`.
56. **Tab** (`Connect`, device §10.3, §10.5, DEV-NET-018): token present, no `push` → `GET …/caps` with `Bearer T_TAB` → gate answers `{"v":1,"push":true,"max_batch":16,"max_frame":262144,"page_entries":256,"page_bytes":1048576,"part_bytes":1048576}`; `device.tdk` replaced with `push: true` on the token (fresh `file_id`, nonce `RAND28[16]`, `RAND29[24]`). No login: none of the four cases of §10.3 holds (the bundle's token is valid). The activity is visible → foreground → socket `wss://relay.example/v1/b/<BID>/socket` with `Bearer T_TAB`; relay: token row TAB valid, board open, `acceptWebSocket(.., ["<TAB hex>"])` (RELAY-SOCK-001); Tab sends hello (21): `0x01 ‖ RAND27 ‖ u32 0`; relay: no other TAB socket; attachment `{dev TAB, gen 1, session RAND27, features 0, ready true}`; welcome (93): `0x81 ‖ u64 201 ‖ C201 ‖ u64 0 ‖ u64 0 ‖ i64 relay_now ‖ u64 1 ‖ u64 1 ‖ u8 1 ‖ u8 1 ‖ u32 3000 ‖ DOOR ‖ u8 0` (relay §11.3).
57. **Tab** (engine §11.2, §11.3): `Connected{push true}`; welcome: head 201 > `L` 200, no ack; reconcile `a = 0` (no TAB record in `M_200`), `r = 0`: normal; `Read{after 200, limit 256}` → socket `0x03 ‖ u32 req ‖ u64 200 ‖ u16 256` (15 bytes); relay answers `0x83 ‖ u32 req ‖` the page of step 52.
58. **Tab** (engine §12.3, §12.4): dry run classifies 201 as in step 53 → `Accept`; `Persist[AppendLog[(201, C201, τ, F201)], SetMarker{201, 201, attempt}]` → new segment `log-00000000000000000201.tdl` (create procedure) + history record 201 (DEV-RUN-003); commit: `M_201`, banner `DeviceAdded` (author DESK, subject TAB) created (Tab is not the author, engine §14.2); apply 201 (no board change); `B = 201 ≥ added_seq` and `device_no 4`: joined (ENG-BOOT-003); run ended → rebase; session `Live`.
59. **Tab** (duties, engine §4.5 step 5, §23.1): eligible (§29.1: joined, current, `Live`, `L` = relay head); no `last_beat_ms` yet → beat due; enqueues a beat record (`RAND30[16]` entry_id, `at_ms T_B`, `based_on (201, C201)`), `BeatInfo{rules [1], envelopes [(1,1)], hash_versions [1], state none, acks []}`; send loop seals it: `ctr = 1 + max(0, 0, 0) = 1`, payload (53 bytes) `{"envelopes":[[1,1]],"hash_versions":[1],"rules":[1]}`, plaintext `EID ‖ 0x03 ‖ T_B ‖ u64 201 ‖ C201 ‖ u32 53 ‖ payload ‖ 0x00×390` (P = 512), envelope nonce `RAND31[24]`, frame `F202` (670 bytes, header `"TDF1" ‖ BID ‖ TAB ‖ u64 1 ‖ u32 558`), persisted (D2), then `Append` over the socket: `0x02 ‖ u32 req ‖ "TDA1" ‖ u8 1 ‖ u32 670 ‖ F202` (684 bytes).
    - AMBIGUOUS: the initial `Meta.last_beat_ms` of a new replica is not stated (engine §7.8, §23.1); with 0 (taken here) a beat is Tab's first entry and "Hello" its second; Tab's beat `state` (hash record at aligned `S ≤ B`, `S = 192`) does not exist on a device that bootstrapped at 200 — absent taken (§23.2 says absent only for `B < 64`).
60. **Relay**: P1–P7 (author TAB is the token's dev, `verifyStrict(TAB_SIGN, …)`), T1, T2 (`1 = 0 + 1`), T4, T5: `log(202, TAB, 1, H(F202), C202)`, TAB `last_ctr 1`, `last_seq 202`, `app_count 1`; result `0x82 ‖ u32 req ‖ "TDR1" ‖ u8 1 ‖ 0x00 ‖ u64 202 ‖ C202` (51 bytes); push of 202 to Tab, Desk, Mac. Tab acks and, on the push, classifies its own 202 (seen, dropped); Desk and Mac accept the beat (TAB `beat_ms`, lists).
61. **User** (Tab): adds a task "Hello" in the phone page; the page posts `POST /api/command` with the verb `quick_add` (`{op, name, deadline}`, deadline none) through the bridge (bridge §6.2, §6.3) → `submit_json` (device §17.4).
62. **Tab** (device §17.4, core §12.2, §13.4 `quick_add`): parse; compile against the pending view with `now = T_H` (whole ms), `device_no 4`: name `"Hello"` (trimmed, 5 scalars), no deadline, cap passes; mints `id = max(MI, mark(4) + 1 = 1) = 4·2^40 + 1 = 4,398,046,511,105`; command (74 bytes) `{"id":4398046511105,"name":"Hello","op":"create_task","time_importance":1}`; `origin_state = u16 4 ‖ u64 2 ‖ u64 MB` (18 bytes, device §8.3).
63. **Tab** (engine §9.1): `Enqueue{ticket, command, at_ms T_H, origin_state, hold false}`; trial apply with `Ctx{seq B + |U| + 1 = 202 + 0 + 1 = 203, device_no 4, at T_H, based_on 202}` succeeds; `RAND32[16]` = `entry_id EID_H`; `Persist[Outbox[Put{EID_H, at_ms T_H, based_on (202, C202), kind cmd, body, origin_state, via Append}]]`; on `Persisted`: `Notify(Enqueued{ticket, EID_H, provisional_seq 203})`; the bridge answers the page `{"ok": true, "version": N, "id": 4398046511105}` (bridge §6.2 row 3). Tab shows "Hello" (pending view).
64. **Tab** (send loop): seals `EID_H` with `ctr = 1 + max(1, 1, 0) = 2`, epoch 1; plaintext (P = 512):

    | Offset | Field | Width | Value |
    |---|---|---|---|
    | 0 | entry_id | 16 | `EID_H` |
    | 16 | kind | 1 | 1 |
    | 17 | at_ms | 8 | `T_H` |
    | 25 | based_on_seq | 8 | 202 |
    | 33 | based_on_chain | 32 | `C202` |
    | 65 | payload_len | 4 | 74 |
    | 69 | payload | 74 | the command |
    | 143 | padding | 369 | `0x00` |

    envelope nonce `RAND33[24]`; frame `F203` (670): `"TDF1" ‖ BID ‖ TAB ‖ u64 2 ‖ u32 558 ‖ 0x01 ‖ 0x01 ‖ u32 1 ‖ RAND33 ‖ AEAD(K_entry,1; RAND33; "taskdeck/v1/entry\0" ‖ frame[0..48] ‖ 0x01 0x01 ‖ u32 1; plaintext)[528] ‖ SIG(TAB signing key; "taskdeck/v1/frame\0"; frame[0..606])`; `Persist[Outbox[Put(ctr 2, F203)]]`; then `Append` over the socket (684 bytes) as step 59.
65. **Relay**: §9.3 and §9.4 → `log(203, TAB, 2, H(F203), C203 = H("taskdeck/v1/chain\0" ‖ C202 ‖ u64 203 ‖ H(F203)))`; result `new (203, C203)`; push `0x84 ‖ page(head 203, 1 entry)` (762 bytes) to Tab, Desk, Mac.
66. **Tab**: `ack = (203, C203)`; push → classify 203 (own dev, own hash for ctr 2), `Accept`, persist, apply `create_task` with `Ctx{203, 4, T_H, 202}` (mint check passes, mark 4 → 1); `seen = 203`, `Drop(EID_H)`; echo check passes.
67. **Desk, Mac**: classify 203 (TAB current since 202 > 201, `ctr 2 = 1 + 1`, `based_on 202` matches) → `Accept`; apply: "Hello" (id 4,398,046,511,105, created `T_H`, `time_importance 1`, stamps `[203, 4]`) appears. Mac shows: the unacknowledged `DeviceAdded` banner "Desk added Tab" with both key fingerprints, Tab in the device list (device_no 4, last seen from its beat), and the task "Hello".

State at the end:

| Holder | State |
|---|---|
| Relay | log 1..203, head `(203, C203)`; rows DESK `last_ctr 122`/`last_seq 201`, MAC 60, PHONE 18, `dev_R` 1, TAB role 1 state 1 gen 1 `last_ctr 2`/`last_seq 203` `app_count 2`; ticket `tid` (a, b, c, d, tries 1) until `expires`; blob `SNAPP` (purpose 2, seq 200, complete) until a `blob-begin` an hour later deletes it; admin nonce `AN` deleted |
| Desk | `L = K = B = 203`, `C203`; `M_203` epoch 1, Desk `last_ctr 122`, TAB device_no 4 `last_ctr 2`; outbox empty; `history.tdk` + 201; no banner |
| Mac | `L = K = B = 203`, `C203`; same membership; banner `DeviceAdded` (201, 0) unacknowledged; outbox empty |
| Phone | unchanged at 200 (app closed) |
| Tab | `L = K = B = 203`, `C203`, `log_floor 201`, `retained_from 200`; `M_203`; device_no 4, `last_ctr 2`, `next_ctr 3`; epoch 1, root `R_1` in `identity.tdk`; files: `store.tds`, `identity.tdk`, `device.tdk` (token gen 1, push true, minter 4/2/MB), `checkpoint.tdk` and `base-…200.tdk` at 200, `log-…201.tdl` (201, 202, 203), `history.tdk` (1, `a_M`, `a_P`, 201), `outbox.tdo` (Puts and Drops, no live record), `meta.tdk` (banner DeviceAdded 201, `last_beat_ms T_B`), `marker.tdk`; `bootstrap.tdk` removed |

User saw: on Desk the pairing sheet (address, code), then the six digits, then "done"; on Tab "accepted", then the board, the "Desk added Tab" banner and "Hello"; on Mac the banner, Tab in the list and "Hello".

- AMBIGUOUS: when `marker.tdk` is cleared after a page has been applied is not stated (engine §15, §7.11 `ClearMarker`); I leave the marker in place until the next `SetMarker`.

## Scenario 6: snapshot bootstrap

Values: Mac's log `log_floor..100`, `C100`, `M_100` (Mac `last_ctr 60`, last own frame at `q_M ≤ 100`); outbox: one live `Put` `EID_R` `{at_ms TQ ≈ T0 − 400 d = 1,760,781,600,000, based_on (100, C100), kind cmd, body rename, ctr none, frame none}` (the edit was queued while offline, so never sealed, ENG-SESS-001). Relay: head `(200, C200)`, `compact_floor 129`, `purge_floor 1`, `compact_snap SNAP192`; the `snap` entry announcing SNAP192 at `σ` (193 ≤ σ < β), Phone's beat acking it at `β ≤ 200`; SNAP192 complete, purpose 1, seq 192, author DESK, `SZ` bytes, `PARTS` parts, `SHA`. The queued edit renames "Call plumber" (id `2^40 + 7 = 1,099,511,627,783`) to "Call the plumber"; its name stamp is assumed unchanged since a seq ≤ 100.

- AMBIGUOUS: the scenario's relay state cannot arise under the specs: `compact` requires `below_seq ≤ head_seq − 16,384` (RELAY-COMPACT-007; head 200), and SNAP192 can be relied on only through a `snap` entry at `σ > 192` (relay §12.8 check 4); the facts are taken as given.
- AMBIGUOUS: whether the queued edit was sealed before Mac went offline is not fixed by the scenario; taken as unsealed.

1. **User** (Mac): opens TaskDeck. **Mac**: `Device::open` takes the lock, reads the identity directory (device §7.4), draws `RAND1[16]` = session id (device §10.4); `Start{Loaded}`: checkpoint (`B` = 100 assumed), outbox rebuilt with `EID_R` (ENG-STATE-003), `attempt` raised (`WriteMeta`); pending view = state at 100 plus the rename (`Ctx{101, 2, TQ, 100}`), not doomed (engine §8.2).
2. **Mac** (`Connect`, engine §11.1, device §10.5): first `Connect` after open, no backoff; stored `T_MAC` with `push: true`; desktop in foreground → `GET wss://relay.example/v1/b/<BID>/socket`, `Authorization: Bearer T_MAC`, `User-Agent: TaskDeck-Sync/1`, upgrade headers.
3. **Relay**: gate (token MAC, board); object: MAC row role 1, state 1, gen 1 (token never invalidated); board open → `101` (RELAY-SOCK-001).
4. **Mac**: `Connected{push true}`; hello (21): `0x01 ‖ RAND1 ‖ u32 0`.
5. **Relay**: no other MAC socket with a session; attachment `{MAC, 1, RAND1, 0, true}`; welcome (93): `0x81 ‖ u64 200 ‖ C200 ‖ u64 60 ‖ u64 q_M ‖ i64 relay_now ‖ u64 129 ‖ u64 1 ‖ u8 1 ‖ u8 1 ‖ u32 3000 ‖ DOOR ‖ u8 0` (relay §11.3).
6. **Mac** (engine §11.2, §11.3): clock measurement from `relay_now`; `head.seq 200 ≥ L 100`; no ack; not fenced; reconcile `a = 60`, `r = 60`, `h = 60` (see the AMBIGUOUS line at the top): normal; `L = 100 ≥ q_M`. Head above `L` → `Read{after 100, limit 256}`; session `Reconciling`.
7. **Mac → Relay**: socket read `0x03 ‖ u32 req ‖ u64 100 ‖ u16 256` (15 bytes).
8. **Relay** (relay §10.5): rows `seq > 100`, `≥ purge_floor 1`: the compacted rows 101..128 are gone (kept control entries are all ≤ 100), so 129..200 (72 entries, frames well under 1 MiB); `more = 0`; `truncated = 1` (`after + 1 = 101 < floor 129`). Answer `0x83 ‖ u32 req ‖ "TDP1" ‖ u64 200 ‖ C200 ‖ i64 relay_now ‖ u64 129 ‖ u64 1 ‖ u8 0x02 ‖ u16 72 ‖ 72 × (u64 seq ‖ i64 at ‖ u32 flen ‖ frame)` (flags: bit 1 `truncated` set, bits 0 `more` and 2 `fenced` clear).
9. **Mac** (engine §12.3 steps 1–4): limits pass; clock; head checks pass; `truncated` and `L + 1 = 101 < floor 129` → §19.7; the page is dropped unprocessed. Session `Rejoining` (status `session = Rejoining`, engine §30.1).
    - AMBIGUOUS: whether `Rejoining` also adds a `StopView` with action `ConfirmRejoin` to `status.stops` (engine §30.1 lists that action; §19.7 only says "shows it") is not stated.
10. **Mac → User**: shows that the relay no longer holds the history from 101 to 128 and asks to replace this copy's log with a recent snapshot of the board (the words are stage `desktop`'s).
11. **User**: confirms → `confirm_rejoin()` → `User(ConfirmRejoin)` (device §17.7, ENG-BOOT-001).
12. **Mac** (§19.7 step 2): `Persist[WriteBase{board_seq 100, C100, board at 100, M_100, log_floor, registry}]` → `base-00000000000000000100.tdk` (create procedure, `RAND2[16]` file_id, `RAND3[24]` nonce); and `Blob{List}`.
    - AMBIGUOUS: whether the listing waits for that base's `Persisted` is not stated (no row of engine §7.12); taken as not waiting.
13. **Mac → Relay**: `GET https://relay.example/v1/b/<BID>/blobs` with `Bearer T_MAC`. **Relay**: 200 `{"blobs":[{"id":"<SNAP192>","purpose":1,"seq":192,"author":"<DESK>","size":SZ,"parts":PARTS,"sha256":"<SHA>","received":PARTS,"complete":true}, …]}` (purpose ascending, seq descending; relay §10.7).
14. **Mac**: candidates = complete purpose-1 blobs with `seq ≥ floor − 1 = 128`, newest first: SNAP192 first. `Blob{Fetch{SNAP192, PARTS}}`.
15. **Mac → Relay** (device §10.5): `GET …/blobs/<SNAP192>` (blob-meta, same object as above), then `GET …/blobs/<SNAP192>/i` for `i = 0 .. PARTS − 1`; relay answers each part's bytes.
16. **Mac** (`open_snapshot(parts, expected{seq 192, blob SNAP192, sha256 SHA, size SZ, parts PARTS}, writer DESK, exact: false)`, seal §20.5 declared mode, SEAL-BLOB-008), in order: `PARTS = ceil(SZ / 1,048,576)` parts, all but the last 1,048,576 bytes, total `SZ`; `H(concatenation) = SHA` (constant time; nothing decrypted before); header `"TDB1"`, ver 1, suite 1, `blob_kind 1`, epoch read from bytes 7..11 = 1; `K_snap,1` from Mac's `R_1`; AEAD opens with `AD` (84) = `"taskdeck/v1/blob\0" ‖ BID ‖ sealed[0..27] ‖ u64 192 ‖ DESK`; padding valid (`U = 4 + body_len`); inflate under a cap of 67,108,864, no trailing input → `INF` bytes; body `"TDSN" ‖ u64 192 ‖ C192 ‖ u32 1 ‖ SH192 ‖ vb32(BOARD192) ‖ vb32(M192, 810) ‖ u32 3 ‖ 3 history records`, no trailing bytes, `S = 192`; membership parses; history (1, `a_M`, `a_P`) passes the §21.6 projection with Mac's `BID` and agrees with `M192`. Returns `(material, epoch 1, INF)`; the history is kept for the runtime (engine §6.1).
17. **Mac** (§19.7 step 3): `M192` names `BID` and lists MAC current; `SM::load(BOARD192)` succeeds and `SM::rules = 1 = M192.rules`; `H("taskdeck/v1/state\0" ‖ u32 1 ‖ H(BOARD192) ‖ H(M192)) = SH192` (engine §18.3). The candidate passes.
18. **Mac** (§19.7 step 4): in memory `B = K = L = 192`, confirmed state and `M_192` from the snapshot, `C192` from the body, `log_floor 193`, `retained_from 192`. Durable writes, in order: `Persist#1[WriteCheckpoint{192, C192, BOARD192, M192, log_floor 193, registry}, WriteBase{same}]` (`checkpoint.tdk` replace: `RAND4[16]`, `RAND5[24]`; `base-…192.tdk` create: `RAND6[16]`, `RAND7[24]`); the store first appends the snapshot's history records above the last seq of `history.tdk` — none, Mac already holds 1, `a_M`, `a_P` (DEV-RUN-006); after its `Persisted`, `Persist#2[TruncateLogBelow(193, Some(C192))]`: `device.tdk` replaced with `log_floor 193`, `floor_chain C192` (`RAND8[16]`, `RAND9[24]`), then every segment whose last seq is below 193 removed (DEV-FILE-006).
    - AMBIGUOUS: §19.7 step 4 lists `TruncateLogBelow` before "a checkpoint and a base", while D8 (engine §7.12) makes the truncation wait for a durable checkpoint with `board_seq ≥ 192`; I follow D8.
    - AMBIGUOUS: whether Mac records a hash record at 192 from the snapshot (`AppendHashes`) is not stated (ENG-HASH-002 speaks of seqs it applies); taken as no.
19. **Mac**: `Read{after 192, limit 256}` → socket `0x03 ‖ u32 req ‖ u64 192 ‖ u16 256`.
20. **Relay**: rows 193..200 (8 entries), `more 0`, `truncated 0` (`193 ≥ 129`): `0x83 ‖ u32 req ‖ "TDP1" ‖ u64 200 ‖ C200 ‖ i64 relay_now ‖ u64 129 ‖ u64 1 ‖ u8 0 ‖ u16 8 ‖ 8 entries`.
21. **Mac** (engine §12.3 steps 5–7): contiguous from `L + 1 = 193`; for each `s` in 193..200: `C_s = H("taskdeck/v1/chain\0" ‖ C_{s−1} ‖ u64 s ‖ H(frame_s))` from `C192`; classify with `M_{s−1}` (from `M_192`), `retained_from 192`: entries with `based_on_seq < 192` get `based_on_unchecked` (SEAL-RCPT-013), others are checked against `C_b`; all `Accept` (Desk's and Phone's entries, the `snap` entry at `σ`, Phone's beat at `β`). `C200` equals the page head chain.
22. **Mac** (§12.3 step 8): `Persist[AppendLog[193..200 with C_s, τ_s, frames], SetMarker{193, 200, attempt}]` → new segment `log-00000000000000000193.tdl` (create: `RAND10[16]`, `RAND11[24]`); no control entry among them, so no history record.
23. **Mac** (step 9, §12.4): commit `L = K = 200`, `M_200`; apply 193..200 to the board; at `σ`: `SnapRecord{entry_seq σ, author DESK, info, entry_clock τ_σ}`, no own verdict (Mac did not apply 192, engine §19.5); §19.7 step 5: the `snap` entry names SNAP192, its author DESK = the opening author, and `seq 192`, `hash SH192`, `sha256 SHA`, `size SZ`, `parts PARTS`, `inflated INF`, `epoch 1` equal what was opened; at `β`: Phone's beat `acks [{"entry": σ, "verdict": "ack"}]` adds `(PHONE, Ack, β)` to the record; `B = 200`.
24. **Mac** (§12.3 step 10, §19.7 steps 5 and 7, §8.4 case 3): the run ended at the relay head; SNAP192 is usable (an `Ack` from PHONE ≠ writer, no `Mismatch`, §19.3); rejoin done; rebase: the rename applied on the view at 200 with `Ctx{201, 2, TQ, min(100, 200) = 100}` succeeds (no overwrite notice under the stamp assumption); session `Live`; `status.warnings` = `ContinuityUnchecked{from: 192}` (until acknowledged) and `BasedOnUnchecked{count}` (engine §30.1, V2-15). Checkpoint after the bootstrap already written (step 18).
25. **Mac** (duties, §23.1): eligible; `wall_ms − last_beat_ms ≥ 86,400,000` (400 days) → enqueues a beat record after `EID_R` (`RAND12[16]` entry_id, `at_ms T_B`, `based_on (200, C200)`, `BeatInfo{rules [1], envelopes [(1,1)], hash_versions [1], state none, acks []}`), `last_beat_ms = T_B`.
    - AMBIGUOUS: the beat's `state` needs Mac's hash record at 192 (engine §23.2), which per step 18 it lacks; taken as absent.
26. **Mac** (send loop §10.2 steps 3–5): `R = [EID_R, beat]`, both unsealed, `via Append`, epoch gate open; seals in order (ENG-SEND-004):
    - `EID_R`: `ctr = 1 + max(60, 60, 60) = 61`, `M_K = M_200`, epoch 1, `at_ms TQ` and `based_on (100, C100)` unchanged (ENG-ENQ-005). Plaintext (P = 512):

      | Offset | Field | Width | Value |
      |---|---|---|---|
      | 0 | entry_id | 16 | `EID_R` |
      | 16 | kind | 1 | 1 |
      | 17 | at_ms | 8 | `TQ` |
      | 25 | based_on_seq | 8 | 100 |
      | 33 | based_on_chain | 32 | `C100` |
      | 65 | payload_len | 4 | 60 |
      | 69 | payload | 60 | `{"id":1099511627783,"name":"Call the plumber","op":"rename"}` |
      | 129 | padding | 383 | `0x00` |

      nonce `RAND13[24]`; frame `F201` (670): `"TDF1" ‖ BID ‖ MAC ‖ u64 61 ‖ u32 558 ‖ 0x01 ‖ 0x01 ‖ u32 1 ‖ RAND13 ‖ AEAD(K_entry,1; RAND13; "taskdeck/v1/entry\0" ‖ frame[0..48] ‖ 0x01 0x01 ‖ u32 1; plaintext)[528] ‖ SIG(MAC signing key; "taskdeck/v1/frame\0"; frame[0..606])`.
    - beat: `ctr 62`, payload (53) `{"envelopes":[[1,1]],"hash_versions":[1],"rules":[1]}`, plaintext `EID_B ‖ 0x03 ‖ T_B ‖ u64 200 ‖ C200 ‖ u32 53 ‖ payload ‖ 0x00×390`, nonce `RAND14[24]`, frame `F202` (670, header `… ‖ MAC ‖ u64 62 ‖ u32 558`).
    - `Persist[Outbox[Put(EID_R, ctr 61, F201), Put(EID_B, ctr 62, F202)]]` (one record, nonce `RAND15[24]`); on `Persisted`, `Append{frames [F201, F202]}` → socket `0x02 ‖ u32 req ‖ "TDA1" ‖ u8 2 ‖ u32 670 ‖ F201 ‖ u32 670 ‖ F202` (1,358 bytes).
27. **Relay** (§9.3, §9.4): for each frame P1–P7 (MAC is the token's dev, row current, `verifyStrict(MAC_SIGN, …)`); one transaction: T1, T2 (`61 = 60 + 1`, then `62`), T4, T5: `log(201, MAC, 61, H(F201), C201)`, `log(202, MAC, 62, H(F202), C202)`, MAC row written once `last_ctr 62`, `last_seq 202`, `app_count 2`. Result `0x82 ‖ u32 req ‖ "TDR1" ‖ u8 2 ‖ 0x00 ‖ u64 201 ‖ C201 ‖ 0x00 ‖ u64 202 ‖ C202` (87 bytes); push of 201..202 to Mac and Desk (Phone closed).
28. **Mac**: acks `(201, C201)`, `(202, C202)`; on the push classifies 201 (own dev, own hash at 61; `based_on 100 < retained_from 192` → `based_on_unchecked`) and 202 → `Accept`; persists, applies the rename with `Ctx{201, 2, TQ, 100}`, `seen`, `Drop`; echo checks pass.
29. **Desk**: classifies 201 with its full log: `based_on_chain = C100` of its own log (SEAL-RCPT-012) → `Accept`; applies the rename; then the beat (MAC `beat_ms = max(.., T_B)`).
30. **Mac → User**: the board at 202 with "Call the plumber", and the warning that history before 192 could not be checked for continuity (`ContinuityUnchecked{from 192}`), shown until the user acknowledges it.

State at the end:

| Holder | State |
|---|---|
| Relay | log: kept rows ≤ 100, 129..202; head `(202, C202)`; `compact_floor 129`; MAC row `last_ctr 62`, `last_seq 202`, `app_count 2`; SNAP192 kept (`compact_snap`) |
| Mac | `L = K = B = 202`, `C202`; `log_floor 193`, `retained_from 192`; `M_202` epoch 1, MAC `last_ctr 62`, `beat_ms T_B`; files: bases at 100 and 192, `checkpoint.tdk` at 192, `device.tdk` (`log_floor 193`, `floor_chain C192`), segment `log-…193.tdl` (193..202), old segments removed, `history.tdk` (1, `a_M`, `a_P`), outbox with no live record, registry `SnapRecord σ` with PHONE's ack; warnings `ContinuityUnchecked{192}`, `BasedOnUnchecked{count}` |
| Desk | head `(202, C202)`, rename applied |
| Phone | head 200 (closed); gets 201, 202 when it opens |

User saw: the rejoin question, then the board at 202 with the rename, and the continuity warning "not checked before 192".
