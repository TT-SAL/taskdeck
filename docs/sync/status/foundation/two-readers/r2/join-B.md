# Group `join` — reader B

Notation as in `edit.md`: `ctx(x)` = `taskdeck/v1/x` ‖ `0x00`; `H` = SHA-256; `K1 = K_entry,1`; `R_1` the epoch-1 root. Origin `O` = `"https://relay.example"` (21 bytes, seal.md §5.4). `door` = `HMAC(TOKEN_KEY, ctx("door") ‖ BID)[0..5]` (relay.md §5.3).

## Scenario 3 — Pairing

1. **User** (Desk), `T0`: Devices → "Add a device"; Desk authenticates the user (device.md §18.3 `pair_start`, seal.md §17.1 step 1).
2. **Desk** pairing machine `Start{membership, head (200, C200), O}`: Desk current and unhalted (seal.md §17.10). Draws, in order (SEAL-PAIR-016): `RAND1[10]` = `s`; `RAND2[32]` = `esk_E`; `RAND3[32]` = `r_E`.
3. **Desk** computes (seal.md §17.2, §17.3, §17.5, §17.7):
   - `check` = first 10 bits of `H(ctx("pair-code")[22] ‖ s)`; code = Crockford(`s`) (16 chars) ‖ Crockford(`check`) (2 chars), shown `XXXX-XXXX-XXXX-XXXX-CC`;
   - `ticket_key = HKDF(salt=BID, ikm=s, info=ctx("pair-ticket")[24], L=32)`;
   - `tid = H(ctx("pair-tid")[21] ‖ ticket_key)[0..16]`;
   - `mac_key = HKDF(salt=BID, ikm=s, info=ctx("pair-mac")[21], L=32)`;
   - `epk_E = X25519(esk_E, 9)`; `commit = H(ctx("pair-commit")[24] ‖ epk_E ‖ r_E)` (88-byte input).
4. **Desk** slot A (seal.md §17.4), 84 bytes:

   | Field | Width | Value |
   |---|---|---|
   | `label` | 1 | `0x41` |
   | `role` | 1 | `0x45` |
   | `body_len` | 2 | 48 |
   | `dev_E` | 16 | `DESK` |
   | `commit` | 32 | `commit` |
   | `mac` | 32 | `MAC(mac_key; ctx("pair")[17] ‖ 0x41 ‖ 0x45 ‖ tid ‖ u16 48 ‖ body)` (HMAC input 85 bytes) |

5. **Desk** → Relay `POST /v1/b/<BID hex>/pair`, `Authorization: Bearer <Desk token>`, body `tid[16] ‖ slot A[84]` (100 bytes) (relay.md §10.8 `pair-open`; device.md §18.3).
6. **Relay** gate (token MAC, board); object: token row current gen 1; board open; slot A layout `41 45 0030 …`; expired tickets deleted; no ticket `tid`; fewer than 4 → insert `(tid, creator DESK, expires = now + 600,000, tries 0, a)`; answer `{"expires":<now+600000>}`.
7. **Desk** shows the pairing sheet (capture excluded): the code and the relay address display `relay.example/` ‖ Crockford(`BID ‖ door`) in groups of 4, last of 2 (relay.md §5.4, DEV-PAIR-001).
8. **User** (Tab, fresh install): "Join a board"; types the address, the code, the name "Tab" (`join(address, code, "Tab")`, device.md §18.3).
9. **Tab** checks lock, cloud folder, key store; parses the address (relay.md §5.4 steps 1–6) → `BID`, `door`; normalises the code: 18 characters, check bits match (SEAL-CODE-002); derives `ticket_key`, `tid`, `mac_key` as step 3. Machine `CodeEntered` → `FetchSlot{A}`.
10. **Tab** → Relay `GET /v1/b/<BID>/pair/<tid hex>/a`, `Authorization: Door <door hex>`, `X-TD-Proof: <ticket_key hex>`.
11. **Relay** gate: door equal (constant time), proof `H(ctx("pair-tid") ‖ ticket_key)[0..16] = tid` (RELAY-GATE-008); object: ticket unexpired → slot A bytes.
12. **Tab** verifies the MAC of A, label `0x41`, role `0x45` (SEAL-PAIR-003). Draws, in order: `RAND4[16]` = `dev_N` = `TAB`; `RAND5[32]` = Ed25519 private key (public `TAB_SIGN`); `RAND6[32]` = X25519 `ikm` (`DeriveKeyPair`, public `TAB_HPKE`); `RAND7[32]` = `esk_N`; `epk_N = X25519(esk_N, 9)`.
13. **Tab** slot B, 157 bytes (body 121 = 114 + 4 × 1 + 3):

    | Field | Width | Value |
    |---|---|---|
    | `label` | 1 | `0x42` |
    | `role` | 1 | `0x4E` |
    | `body_len` | 2 | 121 |
    | `dev_N` | 16 | `TAB` |
    | `sign_pk_N` | 32 | `TAB_SIGN` |
    | `hpke_pk_N` | 32 | `TAB_HPKE` |
    | `epk_N` | 32 | `epk_N` |
    | `n_rules` | 1 | 1 |
    | rules | 4 | `u32 1` |
    | `name_N` | 1 + 3 | `vb8("Tab")` |
    | `mac` | 32 | `MAC(mac_key; ctx("pair") ‖ 0x42 ‖ 0x4E ‖ tid ‖ u16 121 ‖ body)` |

14. **Tab** → Relay `POST …/pair/<tid>/b` with door and proof, body slot B.
15. **Relay** board open; ticket exists; label `b`: `tries := 1` (written whatever follows); layout `42 4E`, 121 within 119–242, total 157; not a repeat; `a` present, `b` absent → store `b`; answer `{}`; after commit `0x87 ‖ tid ‖ 0x42` (18 bytes) to Desk's ready socket (relay.md §10.8, §11.4).
16. **Desk** on the slot notice: `GET …/pair/<tid>/b` with its token (creator) → slot B bytes.
17. **Desk** §17.6 checks in order: MAC/label/role; layout; `TAB`, `TAB_SIGN`, `TAB_HPKE` differ from every recorded `dev`, `dev_R`, `sign_pk`, `hpke_pk`; rules list `[1]` holds active rules 1; 3 current devices < 16, `next_device_no = 4 ≤ 8191`.
18. **Desk** slot C (100 bytes): `0x43 ‖ 0x45 ‖ u16 64 ‖ epk_E[32] ‖ r_E[32] ‖ MAC(mac_key; ctx("pair") ‖ 0x43 ‖ 0x45 ‖ tid ‖ u16 64 ‖ body)`; `POST …/pair/<tid>/c` with token; Relay: creator check, `b` present, `c` absent → store; `{}`.
19. **Desk** `dh = X25519(esk_E, epk_N)` (non-zero); transcript (seal.md §17.7), 440-byte input:

    | Field | Width | Value |
    |---|---|---|
    | context | 28 | `ctx("pair-transcript")` |
    | origin | 2 + 21 | `vb16("https://relay.example")` |
    | `board_id` | 16 | `BID` |
    | `tid` | 16 | `tid` |
    | `dev_E` | 16 | `DESK` |
    | slot A | 84 | as posted |
    | slot B | 157 | as posted |
    | slot C | 100 | as posted |

    `T = H(that)`; `sas = HKDF(salt=s, ikm=dh, info=ctx("sas")[16] ‖ T, L=8)`; digits = `u64(sas) mod 1,000,000`, six digits. Desk shows `ShowSas{digits}`.
20. **Tab** polls `GET …/pair/<tid>/c` (door + proof; `404 relay/slot_empty` → retry after 2,000 ms) → slot C; MAC/label/role; `H(ctx("pair-commit") ‖ epk_E ‖ r_E) = commit` (constant time); `dh = X25519(esk_N, epk_E)`; same `T`, `sas`; `AskSas`.
21. **User** types the six digits shown on Desk into Tab; **Tab** compares in constant time (≤ 3 tries) → equal → shows "accepted"; starts polling `GET …/pair/<tid>/d`.
22. **User** confirms on Desk that Tab accepted (`pair_confirm` → `UserConfirmed`); machine → `NeedSnapshotAndRegister{device item}`; device → engine `PairRegister{ticket, device}` (device.md §18.3).
23. **Desk** engine §28.4: not halted, eligible, no other pairing. Step 1: `h = B = 200`; material `{seq 200, chain C200, hash_version 1, state_hash SH200, board, membership M_200}` with `SH200 = H(ctx("state")[18] ‖ u32 1 ‖ H(board bytes @200) ‖ H(M_200 bytes))` (86-byte input, engine.md §18.3).
24. **Desk** `seal_snapshot(M_K, material)` under epoch 1. Draws `RAND8[16]` = `blob_id` (seal.md §20.1), `RAND9[24]` = blob nonce.
    AMBIGUOUS: the order of the two draws inside `seal_snapshot` (blob id, nonce) is not fixed by seal.md §20.1/§20.2 or SEAL-GEN-012.

    Snapshot body before compression (seal.md §20.4):

    | Field | Width | Value |
    |---|---|---|
    | `magic` | 4 | `"TDSN"` |
    | `S` | 8 | 200 |
    | `chain` | 32 | `C200` |
    | `hash_version` | 4 | 1 |
    | `state_hash` | 32 | `SH200` |
    | `board` | 4 + b | `vb32(canonical board bytes @200)` |
    | `membership` | 4 + m | `vb32(M_200)` (seal.md §13.2) |
    | `history` | 4 + Σ | `u32 3`, then the history records of seqs 1, `a_Mac`, `a_Phone` (`ctl_seqs`) |

    `inflated` = its length `n_i`. Raw DEFLATE → `z`; padded `u32 |z| ‖ z ‖ 0x00 × (P(4 + |z|) − 4 − |z|)` (§20.3). Sealed blob (§20.2): `"TDB1" ‖ u8 1 ‖ u8 1 ‖ u8 1 (snapshot) ‖ u32 1 ‖ RAND8 ‖ RAND9 ‖ AEAD(K_snap,1; RAND9; AD; padded)`, `AD = ctx("blob")[17] ‖ BID ‖ sealed[0..27] ‖ u64 200 ‖ DESK` (84 bytes), `K_snap,1 = HKDF(salt=BID, ikm=R_1, info=ctx("key-snap") ‖ u32 1, L=32)`. `size = 67 + P(4 + |z|)`, `parts = ceil(size / 1,048,576)`, `sha256 = H(sealed)`.
25. **Desk** `Blob{Upload{purpose 2, seq 200}}`: `POST …/blobs` body (65 bytes) `RAND8 ‖ u8 2 ‖ u64 200 ‖ u64 size ‖ sha256`. **Relay** lazy deletions; open; size in range; purpose 2: `1 ≤ 200 ≤ head 200`; id new; quota → row (`slot`, `author DESK`, `received 0`); `{"id":"<hex>","parts":p}`. Then `PUT …/blobs/<id>/<i>` for `i` 0..p−1 (exact lengths); last part sets `complete = 1` (relay.md §10.7). `BlobResult{Uploaded}`.
26. **Desk** step 2: enqueue `CtlIntent::AddDevice{dev TAB, sign_pk TAB_SIGN, hpke_pk TAB_HPKE, name "Tab", transcript T, bootstrap_seq 200}`, `based_on (200, C200)`, `via Register{TAB, TAB_SIGN}`, `at_ms = wall_ms` (`T_A`); draws `RAND10[16]` = `entry_id`; `Persist[Outbox[Put]]` (engine.md §9.2).
27. **Desk** send loop: first unacked record has `via ≠ Append` → seal alone (§10.2 step 2). §28.4 step 3: `M_K.epoch 1` = snapshot epoch → proceed. `ctr = 1 + max(121, 121, 121) = 122`. Items composed (shape A); control payload (seal.md §11.3, §12.3), 479 bytes:

    ```json
    {"items":[{"bootstrap_seq":200,"dev":"<TAB hex>","hpke_pk":"<TAB_HPKE hex>","name":"Tab","sign_pk":"<TAB_SIGN hex>","transcript":"<T hex>","type":"DeviceAdded"}],"sig":"<SIG(Desk key; \"taskdeck/v1/ctl\\0\"; inner)>"}
    ```

    `inner` (112 bytes) = `ctx("ctl")[16] ‖ BID ‖ DESK ‖ u64 122 ‖ RAND10 ‖ i64 T_A ‖ H(items_bytes)`, `items_bytes` = canonical encoding of the `items` array.
    Plaintext: `RAND10 ‖ kind 2 ‖ T_A ‖ u64 200 ‖ C200 ‖ u32 479 ‖ payload ‖ 0x00 × 476` (`U = 548`, `P = 1024`). Draws `RAND11[24]` = nonce. Frame `FA`: `"TDF1" ‖ BID ‖ DESK ‖ u64 122 ‖ u32 1070 ‖ 01 ‖ 01 ‖ u32 1 ‖ RAND11 ‖ AEAD(K1; RAND11; ctx("entry") ‖ FA[0..48] ‖ 01 01 00000001; pt)[1040] ‖ SIG(Desk; ctx("frame"); FA[0..1118])` — 1,182 bytes. `Persist[Outbox[Put(sealed)]]` (D2).
28. **Desk** → Relay `POST …/admin/nonce` (token), body `DESK` (16) → `{"nonce":"<N1>","expires":…}` (nonce row purpose 2).
29. **Desk** → Relay `POST …/admin/register` (token), 1,382 bytes (relay.md §12.1):

    | Field | Width | Value |
    |---|---|---|
    | `signer` | 16 | `DESK` |
    | `nonce` | 32 | `N1` |
    | `sig` | 64 | `SIG(Desk key; "taskdeck/v1/admin\0"; vb8("register") ‖ BID ‖ DESK ‖ H(args) ‖ N1)` (123-byte message) |
    | `args_len` | 4 | 80 |
    | `args` | 80 | `TAB ‖ TAB_SIGN ‖ H(FA)` |
    | `extra_len` | 4 | 1,182 |
    | `extra` | 1,182 | `FA` |

30. **Relay** A1 layout; A2 open; A3 T1, signer = token's dev; A4 nonce found, deleted; A5 `DESK` current role 1; A6 admin signature; register checks: `TAB`/`TAB_SIGN` in no row; 3 current < 16; `H(FA) = args.frame_hash`; frame `dev` = signer; P1–P7; T2 `122 = 121 + 1` → `new`. Transaction: append seq 201, `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ H(FA))`; Desk row `last_ctr 122`; insert `TAB` row (`role 1, state 1, gen 1, last_ctr 0, last_seq 0`). Answer `{"seq":201,"chain":"<C201>","token":"TD1.<TAB gen 1>"}`; push 201 to Desk and Mac (RELAY-REG-001).
31. **Desk** push 201 → classify own frame `Accept` (shape A; DeviceAdded preconditions: fresh keys, caps, `bootstrap_seq 200 ≥ purge_seq 0` and `< 201`); membership: `TAB` `device_no 4`, `added_seq 201`, `added_by DESK`, `bootstrap_seq 200`, `beat_ms T_A`; `next_device_no 5`; `ctl_seqs += 201`; history record of 201 appended to `history.tdk` with `AppendLog` (device.md §7.3); record seen → `Drop`. Admin answer → `Notify(PairRegistered{head (200, C200), snapshot SnapInfo, added_seq 201})`; transport hands the token to the machine; sealer hands root `R_1` (epoch 1 only: no epoch before).
32. **Desk** machine `Registered` → bundle JSON (seal.md §17.8, canonical key order):

    ```json
    {"added_seq":201,"board_id":"<BID hex>","door":"<door hex>","head":{"chain":"<C200 hex>","seq":200},"host":"relay.example","roots":[{"epoch":1,"root":"<R_1 hex>"}],"snapshot":{"blob":"<RAND8 hex>","epoch":1,"hash":"<SH200 hex>","hash_version":1,"inflated":n_i,"parts":p,"sha256":"<hex>","size":size},"token":"TD1.<92>"}
    ```

    `bundle_key = HKDF(salt=s, ikm=dh, info=ctx("pair-bundle-key")[28] ‖ T, L=32)`; draws `RAND12[24]` = bundle nonce; slot D body = `RAND12 ‖ AEAD(bundle_key; RAND12; ctx("pair-bundle")[24] ‖ BID ‖ tid ‖ T; bundle)` (AD 88 bytes); slot D = `0x44 ‖ 0x45 ‖ u16 len ‖ body ‖ MAC(mac_key; ctx("pair") ‖ 0x44 ‖ 0x45 ‖ tid ‖ u16 len ‖ body)`; `POST …/pair/<tid>/d` (token); Relay stores `d`. Desk machine `Done{TAB}`.
33. **Mac** push 201 → `Accept` (checks of seal.md §14.2 1–16: inner signature, shape A, item preconditions); membership commit; banner `DeviceAdded` (author Desk, its key fingerprint `H(ctx("key-fp") ‖ DESK_SIGN ‖ DESK_HPKE)[0..10]`, subject "Tab", subject fingerprint); history record. **Mac shows**: banner "Desk added Tab (key XXXX…)" until acknowledged; Tab in the device list with no last beat.
34. **Tab** `GET …/pair/<tid>/d` → slot D. §17.9 checks: (1) MAC/label `D`/role `E`; (2) AEAD opens with `bundle_key`; (3) strict JSON, `board_id`, `host`, `door` = what was typed.
35. **Tab** (bundle token, `Bearer`) `GET …/blobs/<RAND8>` (meta), then parts 0..p−1. Step 4, §20.5 with `S = 200`, writer `DESK`, bundle `sha256`, `size`, `parts`, `inflated`, epoch 1, `K_snap,1` from the bundle's `R_1`: part count/sizes; `H(parts) = sha256` before any decryption; header `TDB1 1 1 1`, epoch 1; AEAD with the AD of step 24; padding; inflate capped at `n_i`; body parses, `S = 200`, `state_hash = SH200`, membership parses, history verifies and agrees.
36. **Tab** step 5: body `C_S = C200 = head.chain`; `M_200` names `BID`, `DESK` current; current epoch 1 = highest in `roots`; `H(ctx("epoch") ‖ BID ‖ u32 1 ‖ R_1) = c_1`; recomputed `SH200` equals `snapshot.hash`.
37. **Tab** `GET …/log?after=200` (bundle token) → page `n = 1`, entry 201 (`FA`), head `(201, C201)`. Step 6: `C201` chain step; classify from `M_200`, `C_h = C200`: `based_on (200, C200)` equals → `Accept`. Step 7: entry 201 accepted, author `DESK`, `based_on_seq 200`, one `DeviceAdded` naming `TAB`, `TAB_SIGN`, `TAB_HPKE`, "Tab", `T`. Step 8: `device_no = 4` from `M_201`. Machine `Joined`.
38. **Tab** persists the join in order (device.md §18.3): `KeyStore::create(L_d)`; identity directory `id-<BID>-<TAB>/` with `store.tds`, `identity.tdk` (keys, root epoch 1), `device.tdk` (name "Tab", token for `relay.example` with `gen 1`, no `push`; `minter` = `IdMinter::new(4)`, `history_complete true`), `history.tdk` (snapshot history, DEV-PAIR-006), `bootstrap.tdk` = `0x02 ‖ u64 200 ‖ C200 ‖ u32 1 ‖ SH200 ‖ vb32(board) ‖ vb32(M_200) ‖ u64 201 ‖ vb8("relay.example") ‖ door ‖ u32 1`; plain files set aside; `active.tds` replaced; reopen in synced mode.
39. **Tab** engine `Start` with `Bootstrap::Join` (engine.md §19.8): loads the board, `SM::rules = M_200.rules = 1`; `B = K = L = 200`; `Persist[WriteCheckpoint@200, WriteBase@200]`; after `Persisted`, `Persist[TruncateLogBelow(201, Some(C200))]` (deletes nothing); relay target from the bundle.
40. **Tab** `Connect`: stored token has no `push` → `GET …/caps` (Bearer) → `{"v":1,"push":true,"max_batch":16,…}`, stored; foreground → socket, hello `0x01 ‖ session ‖ u32 0`.
41. **Relay** welcome: `head (201, C201)`, `last_ctr 0`, `last_seq 0`, `floor 1`, `purge_floor 1`, `state 1`, door.
42. **Tab** §11.2–§11.3: `a = 0` (`M_K` holds no record of Tab, ENG-RECON-009), `h = 0`, `r = 0` → normal. `Read{after 200}` → page 201 → `Accept` (as step 37); `B = 201 ≥ added_seq` and `M_201` gives `device_no 4`: joined (ENG-BOOT-003). `Live`.
    AMBIGUOUS: engine.md §14.2 creates a banner on every device but the author's, which gives Tab a `DeviceAdded` banner about its own addition; whether the joining device itself shows it is not said.
43. **User** (Tab), at `T_H`: quick-add "Hello". **Tab** compile: mint `max(1, mark(view,4).item 0 + 1) = 1` → `4 × 2^40 + 1 = 4,398,046,511,105`; command `{"id":4398046511105,"name":"Hello","op":"create_task","time_importance":1}` (74 bytes). Trial apply `ctx {202, 4, T_H, 201}`; draws `RAND13[16]` entry_id; `Persist[Outbox[Put]]`; `Notify(Enqueued)`.
44. **Tab** send loop: `ctr = 1 + max(0, 0, 0) = 1`; plaintext `RAND13 ‖ 1 ‖ T_H ‖ u64 201 ‖ C201 ‖ u32 74 ‖ cmd ‖ 0x00 × 369` (512); draws `RAND14[24]` nonce; frame `"TDF1" ‖ BID ‖ TAB ‖ u64 1 ‖ u32 558 ‖ 01 01 00000001 ‖ RAND14 ‖ AEAD(K1; …)[528] ‖ SIG(Tab key; ctx("frame"); [0..606])` (670 bytes); persist; socket append `0x02 ‖ r ‖ "TDA1" ‖ 1 ‖ u32 670 ‖ frame`.
45. **Relay** P1–P7 (`TAB` row exists since step 30, current); T2 `1 = 0 + 1`; seq 202, `C202`; `TAB` row `last_ctr 1, last_seq 202`; result `new (202, C202)` to Tab, push to Desk, Mac, Tab.
46. **Desk, Mac, Tab** classify 202: author `TAB` current in `M_201` (added at 201, current from 202, SEAL-MEM-007), `ctr 1 = 0 + 1` → `Accept`; apply `create_task` under `Ctx{202, 4, T_H, 201}` (mint check: high part 4, 1 > mark 0) → "Hello" on every device; Tab drops its record.

**State after the last step**

| Holder | Head | Counters | Epoch | Durable |
|---|---|---|---|---|
| Desk | `(202, C202)` | Desk 122, Mac 60, Phone 18, Tab 1, `dev_R` 1; `next_device_no 5` | 1 | log 201–202; `history.tdk` + 201; pairing machine done; ticket left to expire |
| Mac | `(202, C202)` | same | 1 | log 201–202; banner `DeviceAdded` (unacknowledged) in `meta.tdk` |
| Tab | `(202, C202)`, `log_floor 201` | same; `device_no 4` | 1 | checkpoint/base @200, log 201–202, `identity.tdk` (R_1), `device.tdk` (token gen 1, push true), minter `next_item 2` |
| Relay | `(202, C202)` | rows `TAB (gen 1, last_ctr 1)`, Desk 122 | — | blob `RAND8` purpose 2 seq 200 complete; ticket `tid` with A–D until expiry |
| Phone | `(200, C200)` | — | 1 | offline, unchanged |

User saw: Desk the code and address, then six digits, then a confirm prompt and "done"; Tab the SAS prompt, "accepted", then the board with "Hello"; Mac the `DeviceAdded` banner by Desk naming Tab, and "Hello".

## Scenario 6 — Snapshot bootstrap

World: Mac holds `B = K = L = 100`, `C100`, `M_100` (devices Desk 1, Mac 2, Phone 3; epoch 1; Mac `last_ctr 60`); its outbox holds one unsealed record `RQ` (`rename` of item `X` to name `N`, `at_ms = T0 − 400 d`, `based_on (100, C100)`, `entry_id E_Q`). Relay: head `(20000, C20000)`, `compact_floor 3000`, `purge_floor 1`; log rows: control entries below 3000 (seqs 1, `a_Mac`, `a_Phone`, all ≤ 100) and every row 3000..20000; Mac row `last_ctr 60`, `last_seq q_M ≤ 100`. Snapshot blob `SNAP19968` (purpose 1, seq 19968, author `DESK`), announced by Desk's `snap` entry at `s_n` (19969 ≤ `s_n` ≤ 20000), acked `ack` by Phone's beat at `s_b` (`s_n < s_b ≤ 20000`).

1. **User** opens TaskDeck on Mac at `T0` (desktop: foreground).
2. **Mac** `open` → engine `Start`: checkpoint, tail, outbox (`RQ`), meta. Pending view = confirmed @100 + `RQ` (rename shown).
3. **Mac** `Connect` (no backoff at `open`): stored token `gen 1` with `push` → socket upgrade; Relay accepts (row current, gen 1); hello `0x01 ‖ session ‖ u32 0`.
4. **Relay** welcome (93 bytes): `head_seq 20000`, `C20000`, `last_ctr 60`, `last_seq q_M`, `relay_now`, `floor 3000` (`max(3000, 1)`), `purge_floor 1`, `state 1`, `caps 1`, `appends_left`, door, `has_fence 0`.
5. **Mac** §11.2: clock; `20000 > L`; no acks; not fenced; §11.3 `a = 60`, `h = 60` (no frame), `r = 60` → normal. `Read{after 100, limit 256}`; `Reconciling`.
6. **Mac** → `0x03 ‖ r ‖ u64 100 ‖ u16 256`.
7. **Relay** selects rows `> 100`, `≥ purge_floor 1`: the first is 3000 (rows 101–2999 compacted, no kept row there); up to 256 entries / 1 MiB; `flags = more | truncated` (`after + 1 = 101 < floor 3000`); answers `0x83 ‖ r ‖ page`.
8. **Mac** §12.3 step 1–3 pass; step 4: `truncated` and `L + 1 = 101 < floor 3000` → §19.7; the page is not persisted.
9. **Mac** §19.7 step 1: session `Rejoining`; `status.stops` = `[StopView{kind Rejoin, since_ms T0, alarm None, action ConfirmRejoin}]` (not a `StopRecord`). **Mac shows** that this device was away longer than the relay keeps history and asks to rebuild from the newest checked snapshot (the rename stays queued).
   AMBIGUOUS: the wording and the facts the prompt must carry (how far behind, what is replaced, that local history is kept as a base) are not given by engine.md §19.7 step 1 or §30.
10. **User** confirms → `UserAction::ConfirmRejoin`.
11. **Mac** step 2: `Persist[WriteBase@100]` (base of its current confirmed state); `Blob{List}` → `GET …/blobs` (token) → `{"blobs":[…]}` (complete blobs by purpose, seq desc). Candidates: purpose 1, complete, `seq ≥ floor − 1 = 2999`, newest first: `SNAP19968` first.
12. **Mac** step 3: `Blob{Fetch{SNAP19968, parts p}}` → `GET …/blobs/<id>` (meta: `purpose 1, seq 19968, author DESK, size, parts p, sha256`), then `GET …/blobs/<id>/<i>` for `i` 0..p−1.
13. **Mac** `open_snapshot(parts, expected {seq 19968, blob, sha256, size, parts}, author DESK, exact false)` — declared mode (seal.md §20.5), in order: (1) p parts, sizes sum to `size`; (2) `H(concat) = sha256` before any decryption; (3) `magic "TDB1"`, `ver 1`, `suite 1`, `blob_kind 1`, read `epoch = 1` from bytes 7–11; (4) root `R_1` held → `K_snap,1`; AEAD opens with `AD = ctx("blob") ‖ BID ‖ sealed[0..27] ‖ u64 19968 ‖ DESK`; (5) padding (`U = 4 + body_len`); (6) inflate under 67,108,864; (7) body parses with no trailing bytes, `S = 19968`, membership bytes parse, history (records of seqs 1, `a_Mac`, `a_Phone`) verifies and agrees with `M_19968`. Returns body, epoch 1, inflated length `n_i`.
14. **Mac** engine checks: `M_19968` names `BID` and lists `MAC` current; `SM::load(board)` OK and `SM::rules = M_19968.rules = 1`; recomputed `H(ctx("state") ‖ u32 1 ‖ H(board) ‖ H(M_19968))` equals the body's `state_hash`.
15. **Mac** step 4: confirmed state and `M_19968` from the snapshot; `B = K = L = 19968`; `C19968` from the body; `log_floor 19969`; `retained_from 19968`. `Persist[WriteCheckpoint@19968, WriteBase@19968]` (the store writes the snapshot's history records to `history.tdk` ahead of the checkpoint, device.md §8.3, §15.7). No hash record at 19968 (ENG-HASH-003).
16. **Mac** after that `Persisted` (D8): `Persist[TruncateLogBelow(19969, Some(C19968))]` → `device.tdk` gets `log_floor 19969`, `floor_chain C19968`, then every segment whose last seq < 19969 (its log 1..100) is removed (device.md §7.3, DEV-FILE-006).
17. **Mac** `Read{after 19968}` → `0x03 ‖ r ‖ u64 19968 ‖ u16 256`; **Relay** page: 32 entries 19969..20000, `flags 0`.
18. **Mac** dry run 19969..20000 with `M_19968`: chain steps from `C19968`; `based_on` with `b < 19968` set `based_on_unchecked` (seal.md §14.8 step 3); `b ≥ 19968` compared. At `s_n`: Desk's `snap` entry, payload (seal.md §11.5, canonical key order) `{"blob":"<SNAP19968 hex>","epoch":1,"hash":"<SH19968>","hash_version":1,"inflated":n_i,"parts":p,"seq":19968,"sha256":"<hex>","size":size}`; §19.7 step 5: author `DESK` = opened author; `seq 19968`, `hash`, `sha256`, `size`, `parts`, `inflated n_i`, `epoch 1` equal → match. At `s_b`: Phone's beat with `acks [{"entry": s_n, "verdict": "ack"}]` → `SnapRecord` verdict `(PHONE, Ack, s_b)` (§23.3). Head chain `C20000` equals page head.
19. **Mac** `Persist[AppendLog[19969..20000], SetMarker{19969, 20000, attempt}]`; on `Persisted` apply each; `Persist[Notices[…concerning device 2…], ClearMarker]`; no aligned seq in 19969..20000 (next is 20032): no hash record, no verdict on `s_n` (Mac did not apply 19968, §19.5).
20. **Mac** at the relay head: the snapshot is usable (Ack from Phone ≠ writer, no Mismatch, §19.3) → rejoin ends; `meta.continuity_unchecked = Some(19968)` in the next `WriteMeta`; `status.warnings` = `ContinuityUnchecked{from 19968}`. Rebase: `RQ` re-applied with `ctx {seq 20001, device_no 2, at T0 − 400 d, based_on min(100, 20000) = 100}` (rename succeeds if `X` is live; otherwise doomed `core/gone`, §8.3). Reconcile complete → `Live`. The runtime raises the minter from the new marks (§11.3 end).
21. **Mac** send loop: `ctr = 1 + max(60, 60, 60) = 61`; plaintext `E_Q ‖ 1 ‖ at_ms (T0 − 400 d) ‖ u64 100 ‖ C100 ‖ payload_len ‖ rename ‖ padding` (`based_on` unchanged since enqueue, ENG-ENQ-005); draws `RAND1[24]` nonce; frame under epoch 1 (`dev MAC`, `ctr 61`); persist; append.
22. **Relay** P1–P7, T2 `61 = 60 + 1` → seq 20001, `C20001`; result `new`; push to every ready socket.
23. **Mac** classifies its own 20001: `b = 100 < retained_from 19968` → `based_on_unchecked`; `Accept`; apply `rename X` under `Ctx{20001, 2, T0 − 400 d, 100}`: name stamp `(20001, 2)`; an overwrite notice when `X`'s name stamp is `(s, d)` with `s > 100`, `d ∉ {2, 0}` (recorded on Mac and on `d`); `seen`, `Drop(E_Q)`.
24. **Desk, Phone** classify 20001 when they are online: Accept (their `based_on` check passes if they still hold `C100`, else `based_on_unchecked`); apply the rename.
    AMBIGUOUS: a 400-day-old queued edit is accepted and overwrites with `at_ms` 400 days in the past and `based_on 100`; neither engine.md §19.7 ("records in the outbox survive a rejoin") nor core.md says whether the user is warned before such a stale edit is sent.
25. **User** acknowledges continuity (`UserAction::AckContinuity`) → `continuity_unchecked None`, warning gone. Until then Mac shows "continuity from seq 19968 not checked".

**State after the last step**

| Holder | Head | Counters | Epoch | Durable |
|---|---|---|---|---|
| Mac | `(20001, C20001)`, `log_floor 19969`, `retained_from 19968` | Mac `last_ctr 61`; Desk 14,000, Phone 5,939, `dev_R` 1 (from `M_19968` and the tail) | 1 | checkpoint/base @19968, base @100 (both kept: the newest two bases, §7.13), log segment 19969..20001, `history.tdk` from the snapshot, `device.tdk` `log_floor 19969 floor_chain C19968`, registry `SnapRecord(s_n)` with Phone's Ack, outbox empty, `meta.continuity_unchecked` until acked |
| Relay | `(20001, C20001)` | Mac row `last_ctr 61` | — | unchanged otherwise |
| Desk, Phone | `(20001, C20001)` once online | | 1 | rename applied |

What Mac told the user: the rejoin prompt; after it, the warning "Continuity unchecked from seq 19968" until acknowledged; the queued rename reached the board.
