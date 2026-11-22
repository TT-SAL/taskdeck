# Scenario 3. Pairing

`ctx(x)` = `"taskdeck/v1/" ‖ x ‖ 0x00` (seal.md §4.1). Origin `O = "https://relay.example"` (21 bytes, seal.md §5.4). E = Desk, N = Tab.

1. **User (Desk)**: Devices → Add a device; desktop runs Windows Hello (auth, DEV-FACADE-003); calls `pair_start()` (device.md §17.10). Device checks: engine eligible (current, `Live`, unhalted), no pairing busy.
2. **Desk (pairing machine `Existing`, `Start`)**: draws in order `RAND1[10]` = `s`, `RAND2[32]` = `esk_E`, `RAND3[32]` = `r_E` (SEAL-PAIR-016). Computes:
   - `ticket_key = HKDF(salt = BID; ikm = s; info = ctx("pair-ticket"); L = 32)`
   - `tid = H(ctx("pair-tid") ‖ ticket_key)[0..16]`
   - `mac_key = HKDF(salt = BID; ikm = s; info = ctx("pair-mac"); L = 32)` (seal.md §17.3)
   - `epk_E = X25519(esk_E, 9)`; `commit = H(ctx("pair-commit") ‖ epk_E ‖ r_E)` (§17.5, §17.7)
   - `check = first 10 bits of H(ctx("pair-code") ‖ s)`; code = Crockford(s) (16 chars) ‖ Crockford(check) (2 chars), shown `XXXX-XXXX-XXXX-XXXX-CC` (§17.2)
3. **Desk: slot A** (84 bytes, seal.md §17.4):

   | Field | Width | Value |
   |---|---|---|
   | `label` | 1 | `0x41` |
   | `role` | 1 | `0x45` |
   | `body_len` | 2 | 48 |
   | `dev_E` | 16 | DESK |
   | `commit` | 32 | commit |
   | `mac` | 32 | `MAC(mac_key; ctx("pair") ‖ 0x41 ‖ 0x45 ‖ tid ‖ u16 48 ‖ body)` (HMAC-SHA256) |
4. **Desk → Relay**: `POST /v1/b/<BID hex>/pair`, `Authorization: Bearer <Desk token>`, body `tid[16] ‖ slot A[84]` (100 bytes) (relay.md §10.8 `pair-open`). **Relay**: board open; slot A layout `0x41 0x45 48`, total 84; deletes expired tickets; no ticket `tid`; < 4 tickets; inserts `(tid, creator DESK, expires now + 600,000, tries 0, a)`; 200 `{"expires": now+600000}`; rows written 1.
5. **Desk (UI)**: pairing sheet with the relay address display `relay.example/` + Crockford(BID ‖ door) in groups of 4 (34 characters, relay.md §5.4) and the code; main window excluded from capture (DEV-PAIR-001).
6. **User (Tab)**: installs, opens the app: `Device::open` → `LocalOnly`, empty board (device.md §5.4); page `hello` mode `local`. Opens Pair (`POST /api/screen {"name":"pair"}`); pastes the address, types the code (CodeField, one character at a time), name "Tab"; Join → `callSecret("join", …)` (bridge.md §13.1, §13.3).
7. **Tab (device `join`)**: lock, key store (Android), mode LocalOnly; `parseAddress` → host `relay.example`, BID, door (relay.md §5.4); code normalised, 18 characters, check bits equal (SEAL-CODE-002). `New` machine `CodeEntered`: derives `ticket_key`, `tid`, `mac_key` as step 2; emits `FetchSlot{A}`.
8. **Tab → Relay**: `GET /v1/b/<BID>/pair/<tid hex>/a`, `Authorization: Door <door hex>`, `X-TD-Proof: <ticket_key hex>` (device.md §18.3). **Relay gate**: door matches (constant time), `H(ctx("pair-tid") ‖ ticket_key)[0..16] = tid` (RELAY-GATE-008). Object: ticket unexpired → slot A bytes (`application/octet-stream`).
9. **Tab**: verifies slot A MAC, label A, role E (SEAL-PAIR-003). Draws `RAND4[16]` = TAB, `RAND5[32]` = signing key (→ TAB_SIGN), `RAND6[32]` = X25519 ikm (→ `DeriveKeyPair`, TAB_HPKE), `RAND7[32]` = `esk_N`; `epk_N = X25519(esk_N, 9)` (§17.10 Draws).
10. **Tab: slot B** (157 bytes; body 121):

    | Field | Width | Value |
    |---|---|---|
    | `label`, `role`, `body_len` | 1, 1, 2 | `0x42`, `0x4E`, 121 |
    | `dev_N` | 16 | TAB |
    | `sign_pk_N` | 32 | TAB_SIGN |
    | `hpke_pk_N` | 32 | TAB_HPKE |
    | `epk_N` | 32 | epk_N |
    | `n_rules` | 1 | 1 |
    | rules | 4 | u32 1 |
    | `vb8(name_N)` | 4 | `0x03 "Tab"` |
    | `mac` | 32 | `MAC(mac_key; ctx("pair") ‖ 0x42 ‖ 0x4E ‖ tid ‖ u16 121 ‖ body)` |
11. **Tab → Relay**: `POST …/pair/<tid>/b` with Door and proof, body slot B. **Relay**: ticket exists; `tries = 1` (written); layout `0x42 0x4E`, `body_len 121`, total 157; not a repeat; `a` present, `b` absent → stores `b`; 200 `{}`; after commit sends `slot` `0x87 ‖ tid ‖ 0x42` (18 bytes) to Desk's ready sockets (relay.md §10.8, §11.4).
12. **Desk**: on the `slot` notice `GET …/pair/<tid>/b` with its token (creator) → slot B. Checks §17.6: MAC/label B/role N; layout; TAB, TAB_SIGN, TAB_HPKE differ from every recorded `dev`, `dev_R`, `sign_pk`, `hpke_pk`; rules list contains 1; 3 current devices < 16, `next_device_no = 4 ≤ 8191`.
13. **Desk: slot C** (100 bytes): `0x43 ‖ 0x45 ‖ u16 64 ‖ epk_E[32] ‖ r_E[32] ‖ MAC(mac_key; ctx("pair") ‖ 0x43 ‖ 0x45 ‖ tid ‖ u16 64 ‖ body)`; `POST …/pair/<tid>/c` with its token. Relay: creator check, `b` present, `c` absent → stored; `{}`.
14. **Desk**: `dh = X25519(esk_E, epk_N)` (≠ 0); `T = H(ctx("pair-transcript") ‖ vb16(O) ‖ BID ‖ tid ‖ DESK ‖ slot A[84] ‖ slot B[157] ‖ slot C[100])` (input `28 + 23 + 16 + 16 + 16 + 341 = 440` bytes); `sas = HKDF(salt = s; ikm = dh; info = ctx("sas") ‖ T; L = 8)`; `digits = (u64 BE of sas) mod 1,000,000`, 6 digits (§17.7). `status().pairing = ShowSas{digits}` (device.md §18.3).
15. **Tab**: polls `GET …/pair/<tid>/c` every 2,000 ms (`404 relay/slot_empty` until stored); verifies MAC; `H(ctx("pair-commit") ‖ epk_E ‖ r_E) = commit` (SEAL-PAIR-004); `dh = X25519(esk_N, epk_E)`; same `T`, SAS; `AskSas`.
16. **User**: reads the 6 digits on Desk, types them on Tab (`join_sas`). **Tab**: constant-time equal → `ShowAccepted` (pairing `Accepted`); `FetchSlot{D}` polling.
17. **User (Desk)**: confirms that Tab said accepted → `pair_confirm()` → `UserConfirmed` → `NeedSnapshotAndRegister{device item}` → `PairRegister { ticket, NewDevice { TAB, TAB_SIGN, TAB_HPKE, "Tab", T } }` to the engine (engine.md §28.4; SEAL-PAIR-008).
18. **Desk (engine, §28.4 step 1)**: `h = B = 200`. Material `{ seq 200, chain C200, hash_version 1, state_hash = SH200, board = canonical board bytes at 200, membership = M_200 bytes }`, `SH200 = H(ctx("state") ‖ u32 1 ‖ H(board_200) ‖ H(M_200))` (86-byte input, ENG-HASH-001). `seal_snapshot(M_K, material)`: draws `RAND8[16]` = blob id `PB`, `RAND9[24]` = blob nonce.
    Snapshot body (seal.md §20.4): `"TDSN" ‖ u64 200 ‖ C200 ‖ u32 1 ‖ SH200 ‖ vb32(board_200) ‖ vb32(M_200) ‖ u32 n_hist ‖ history records` with one record per `ctl_seqs` (seq 1 and the `DeviceAdded` entries of Mac and Phone: `n_hist = 3`), raw DEFLATE → `body`; `padded = u32 body_len ‖ body ‖ 0x00…` to `P(4 + body_len)`.
    Sealed blob: `"TDB1" ‖ 1 ‖ 1 ‖ u8 1 ‖ u32 1 ‖ PB ‖ RAND9 ‖ AEAD(K_snap,1; RAND9; ctx("blob") ‖ BID ‖ sealed[0..27] ‖ u64 200 ‖ DESK; padded)`.
19. **Desk → Relay**: `Blob { Upload { purpose 2, seq 200 } }`: `POST /v1/b/<BID>/blobs` body `PB ‖ u8 2 ‖ u64 200 ‖ u64 size ‖ sha256` (65 bytes), then `PUT …/blobs/<PB>/<i>` per part. Relay: lazy deletions; `1 ≤ 200 ≤ head`; quota; row `author DESK`; completion.
20. **Desk (engine, step 2)**: on `BlobResult Uploaded`: enqueue ctl record `{RAND10[16] = entry_id, at_ms = wall_ms = Ta, based_on (200, C200), body Ctl(AddDevice{TAB, TAB_SIGN, TAB_HPKE, "Tab", T, bootstrap_seq 200}), via Register{TAB, TAB_SIGN}}`.
21. **Desk (send loop)**: first unsent record, `via ≠ Append` → sealed alone; `M_K.epoch = 1` = snapshot epoch (ENG-OPS-006); ctr 122; draws `RAND11[24]` nonce.
    Item (canonical, 330 bytes): `{"bootstrap_seq":200,"dev":"<TAB>","hpke_pk":"<TAB_HPKE>","name":"Tab","sign_pk":"<TAB_SIGN>","transcript":"<T>","type":"DeviceAdded"}`; `items_bytes = [item]` (332 bytes).
    Inner signature: `SIG(DESK_SIGN; "taskdeck/v1/ctl\0"; BID ‖ DESK ‖ u64 122 ‖ RAND10 ‖ i64 Ta ‖ H(items_bytes))` (seal.md §11.3).
    Payload (479 bytes): `{"items":[…],"sig":"<128 hex>"}`. Plaintext `RAND10 ‖ 0x02 ‖ i64 Ta ‖ u64 200 ‖ C200 ‖ u32 479 ‖ payload ‖ 0x00 × 476` (U 548 → P 1024). Frame: `"TDF1" ‖ BID ‖ DESK ‖ u64 122 ‖ u32 1070 ‖ 1 ‖ 1 ‖ u32 1 ‖ RAND11 ‖ AEAD(K_entry,1; RAND11; AD; pt)[1040] ‖ SIG(DESK_SIGN; "taskdeck/v1/frame\0"; frame[0..1118])` = 1,182 bytes; `FH = H(frame)`. `Persist(Outbox[Put])` (D2).
22. **Desk → Relay**: `POST …/admin/nonce` body `DESK` (16 B), token → `{"nonce": N1, "expires": …}`. Then `POST …/admin/register`, token, body (1,382 bytes):

    | Field | Width | Value |
    |---|---|---|
    | `signer` | 16 | DESK |
    | `nonce` | 32 | N1 |
    | `sig` | 64 | `SIG(DESK_SIGN; "taskdeck/v1/admin\0"; vb8("register") ‖ BID ‖ DESK ‖ H(args) ‖ N1)` |
    | `args_len` | 4 | 80 |
    | `args` | 80 | `TAB ‖ TAB_SIGN ‖ FH` |
    | `extra_len` | 4 | 1,182 |
    | `extra` | 1,182 | the frame |
23. **Relay**: A1 layout; A2 open; A3 token, signer = token's dev; A4 nonce row deleted; A5 DESK current role 1; A6 `verifyStrict`; A7: TAB and TAB_SIGN in no row; < 16 current, < 256 rows; `H(frame) = FH`; frame dev = signer; P1–P7; T2 `122 = 121 + 1` → seq 201, `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ FH)`; insert TAB row `(role 1, state 1, gen 1, last_ctr 0)`. Response `{"seq":201,"chain":"<C201>","token":"TD1.<base64url(u8 1 ‖ BID ‖ TAB ‖ u32 1 ‖ MAC(TOKEN_KEY; ctx("token") ‖ those 37 bytes))>"}`. Push 201 to Desk and Mac.
24. **Desk**: `AdminResult Ok(seq 201, chain C201, token true)` → ack; `Notify(PairRegistered { head (200, C200), snapshot SnapInfo{seq 200, hash SH200, hash_version 1, blob PB, sha256, size, parts, inflated, epoch 1}, added_seq 201 })`; processes the push: classify 201 Accept (shape A, author current device, keys fresh, `bootstrap_seq 200 < 201`) → TAB record `device_no 4`, `next_device_no 5`, `ctl_seqs += 201`; history record kind 11 appended (DEV-RUN-003); no banner on Desk (author). Runtime: `Registered{h 200, C200, snapshot ref, 201, token, roots [(1, R_1)], relay.example, door}`.
25. **Desk: bundle** (seal.md §17.8), canonical JSON: `{"added_seq":201,"board_id":"<BID>","door":"<door>","head":{"chain":"<C200>","seq":200},"host":"relay.example","roots":[{"epoch":1,"root":"<R_1 hex>"}],"snapshot":{"blob":"<PB>","epoch":1,"hash":"<SH200>","hash_version":1,"inflated":I,"parts":p,"sha256":"<sha>","size":Z},"token":"TD1.<92>"}`.
    `bundle_key = HKDF(salt = s; ikm = dh; info = ctx("pair-bundle-key") ‖ T; L = 32)`; draws `RAND12[24]`; slot D body = `RAND12 ‖ AEAD(bundle_key; RAND12; ctx("pair-bundle") ‖ BID ‖ tid ‖ T; bundle)`; slot D = `0x44 ‖ 0x45 ‖ u16 (40 + len) ‖ body ‖ MAC(mac_key; ctx("pair") ‖ 0x44 ‖ 0x45 ‖ tid ‖ u16 len ‖ body)`; `POST …/pair/<tid>/d` with token; machine `Done`.
    AMBIGUOUS: seal.md §17.8 calls the bundle "JSON (§5.3)", i.e. canonical, but its field table does not say `hash_version` and the like are defaults; I write every field.
26. **Tab**: polls D; §17.9 step 1 MAC; step 2 AEAD open; step 3 strict parse, `board_id`, `host`, `door` equal to the typed address; `FetchSnapshot`: `GET …/blobs/<PB>` and parts with `Authorization: Bearer <bundle token>`; step 4: `open_snapshot` exact with `S = 200`, writer DESK, `K_snap,1` from `R_1`: parts/size, `H = sha256` before decrypt, header, AEAD, padding, inflate cap `I`, body parse, history projection; step 5: `C_S = C200`, `M_200` parses, BID, DESK current, epoch 1 = highest root, `c_1 = H(ctx("epoch") ‖ BID ‖ u32 1 ‖ R_1)` matches, state hash recomputed = SH200; step 6: `GET …/log?after=200` → entry 201, `classify(M_200, C200)` Accept; step 7: 201 by DESK, `based_on_seq 200`, DeviceAdded names TAB, keys, "Tab", T; step 8: `device_no = 4`. `Joined`.
27. **Tab: persist the join** (device.md §18.3): `KeyStore::create(L_d)` (`RAND13[32]` = `L_d`; Keystore wrap → `ld.wrap`); identity dir `id-<BID>-<TAB>/`: `store.tds` (key_store 3), `identity.tdk` (keys, root `(1, R_1)`), `device.tdk` (`device_name "Tab"`, `tokens [{gen 1, host relay.example, token}]` no `push`, `minter {block 1, device_no 4, item 1}`, `history_complete true`), `history.tdk` (the 3 records from the snapshot, before `bootstrap.tdk`, DEV-PAIR-006), `bootstrap.tdk` (`0x02 ‖ u64 200 ‖ C200 ‖ u32 1 ‖ SH200 ‖ vb32(board) ‖ vb32(M_200) ‖ u64 201 ‖ vb8("relay.example") ‖ door ‖ u32 1`); no plain files with data to set aside; `sync/active.tds` (`"TDA1" ‖ 1 ‖ BID ‖ TAB ‖ check`); reopen synced, `Start` with `Bootstrap::Join`.
28. **Tab (engine, §19.8)**: loads board; `Persist{WriteCheckpoint(200), WriteBase(200)}` then `Persist{TruncateLogBelow(201, Some(C200))}`; `log_floor 201`, `retained_from 200`; `Connect`: stored token has no `push` → `GET …/caps` → `push: true` stored (DEV-NET-018); socket, hello; welcome `head (201, C201), last_ctr 0, last_seq 0`; reconcile `a = 0` (no TAB in `M_K`), `r = 0`, `h = 0` (ENG-RECON-009); `Read{after 200}`; page entry 201 classified Accept, applied (no board effect); `B = 201 ≥ added_seq`, device_no 4 → joined. History record 201 appended. Banner `DeviceAdded` (author DESK) on Tab (it is not the author, ENG-BANNER-001). `Live`.
29. **Tab (duty)**: eligible, `last_beat_ms = 0` → beat record enqueued (ENG-BEAT-003, §23.1); sealed ctr 1 (`a = 0`); payload (53 bytes) `{"envelopes":[[1,1]],"hash_versions":[1],"rules":[1]}` (no `state`: no hash record at 192; no `acks`), plaintext kind 3, `based_on (201, C201)`, frame 670 bytes; appended → seq 202, `C202`.
    AMBIGUOUS: the scenario calls "Hello" Tab's first entry; by engine.md §23.1 and ENG-BEAT-003 the beat goes first. I write the beat first.
30. **User (Tab)**: quick add "Hello" (page verb `{"op":"quick_add","name":"Hello","deadline":null}`). Compile: `n = max(1, mark(4).item + 1 = 1) = 1` → id `4·2^40 + 1 = 4,398,046,511,105`; command (74 bytes) `{"id":4398046511105,"name":"Hello","op":"create_task","time_importance":1}`; trial `Ctx{203, 4, Th, 202}`; record `based_on (202, C202)` (if the beat is applied by then; else 201); sealed ctr 2; frame 670 bytes; append → seq 203, `C203`. Every device applies it (mint check `h = 4 = ctx.device_no`, `1 > 0`).
31. **Mac**: applied 201 → banner `DeviceAdded { author DESK, author_name "Desk", author_fp, subject TAB, subject_name "Tab", subject_fp = key fingerprint of (TAB_SIGN, TAB_HPKE) }`, unacknowledged until the user acks it; device list gains Tab (device_no 4, last seen from its beat at 202, then 203); then shows "Hello".

**State after.** Relay: TAB row `gen 1, last_ctr 2, last_seq 203`; DESK `last_ctr 122`; ticket kept until expiry; pairing blob kept ≤ 1 h. All devices: head `(203, C203)`, membership with TAB current `device_no 4`, `next_device_no 5`, epoch 1, `ctl_seqs` + 201. Tab: `log_floor 201`, checkpoint and base at 200, outbox empty. Desk: pairing `Done`. User saw: code and address on Desk, SAS on Desk, "accepted" on Tab, banner on Mac and Tab.

# Scenario 6. Snapshot bootstrap

1. **User (Mac)**: opens the app (desktop). `Device::open` → `Synced`; `Start` with `L = B = 100`, outbox holding one `cmd` record `Q` (a `rename`, `based_on (100, C100)`, `at_ms` 400 days before T0).
   AMBIGUOUS: whether `Q` was ever sealed (it was enqueued while Mac had a head of 100 but no say whether `Live`); I take it unsealed (no `ctr`, no frame).
2. **Mac (engine)**: `Connect`; transport: stored token gen 1 with `push` → socket, hello `0x01 ‖ session ‖ u32 0`.
3. **Relay → Mac**: welcome `0x81 ‖ u64 20000 ‖ C20000 ‖ u64 last_ctr 60 ‖ u64 last_seq q60 ‖ relay_now ‖ u64 floor 3000 ‖ u64 purge_floor 1 ‖ 1 ‖ 1 ‖ appends_left ‖ door ‖ 0`.
   AMBIGUOUS: `floor = max(compact_floor, purge_floor) = 3000` from "no longer holds entries below 3,000"; `q60 ≤ 100`.
4. **Mac**: welcome checks: `20000 ≥ L`; no ack; not fenced; reconcile `a = 60`, `h = 60`, `r = 60` normal; `Read{after 100, limit 256}` (`0x03 ‖ r ‖ u64 100 ‖ u16 256`); `Reconciling`.
5. **Relay → Mac**: page with `floor 3000`, `flags` bit 0 (`more`) and bit 1 (`truncated`: `101 < 3000`) set, entries from the first kept row above 100 (relay.md §10.5).
6. **Mac (engine §12.3 step 4)**: `truncated` and `L + 1 = 101 < 3000` → §19.7; the page is not processed (nothing persisted). Session `Rejoining`; `status.stops = [StopView{Rejoin, since_ms, None, ConfirmRejoin}]` (ENG-BOOT-007).
7. **Mac → User**: "This copy is too far behind the relay to catch up from seq 101; replace its log with a snapshot?" with Confirm (`confirm_rejoin`, device.md §17.7).
8. **User**: confirms → `User(ConfirmRejoin)`.
9. **Mac**: `Persist{WriteBase(base at B = 100 of its current confirmed state)}` (§19.7 step 2; unaligned base allowed, §18.2). `Blob{List}` → `GET /v1/b/<BID>/blobs` with token → complete blobs, purpose then seq descending; candidates: purpose 1, `seq ≥ floor − 1 = 2999`, newest first: `SNAP19968` (author DESK) first.
10. **Mac**: `Blob{Fetch{SNAP19968, parts}}` → `GET …/blobs/<SNAP19968>` (meta: `size`, `parts`, `sha256`, `author DESK`, `seq 19968`), then each part.
11. **Mac: checks, in order** (`open_snapshot(exact = false)`, seal.md §20.5 declared mode): (1) part count/sizes = declared `size`, `parts = ceil(size / 1,048,576)`; (2) `H(parts) = sha256` before decrypting; (3) `"TDB1"`, ver 1, suite 1, `blob_kind 1`, epoch read = 1; (4) root of epoch 1 held → `K_snap,1`; AEAD with `ctx("blob") ‖ BID ‖ sealed[0..27] ‖ u64 19968 ‖ DESK`; (5) padding; (6) inflate, cap 67,108,864; (7) body parses, `S = 19968`, membership parses, history verifies by the projection and agrees with membership. Engine (§19.7 step 3): membership names BID and lists MAC current; `SM::load(board)` ok and `rules = 1 = M.rules`; state hash recomputed `H(ctx("state") ‖ u32 1 ‖ H(board) ‖ H(M_19968))` = the body's.
12. **Mac (step 4)**: confirmed state and `M_19968` from the body; `B = K = L = 19968`; `C19968` from the body; `log_floor 19969`; `retained_from 19968`. `Persist{WriteCheckpoint(19968), WriteBase(19968)}`; the store first appends to `history.tdk` the body's history records above its last seq (DEV-RUN-006; none if the Phone's `DeviceAdded` is ≤ 100, AMBIGUOUS: the world does not say when Phone was added). After its `Persisted`: `Persist{TruncateLogBelow(19969, Some(C19968))}` → `device.tdk` `log_floor 19969`, `floor_chain C19968`, then the segment holding 1–100 removed (DEV-FILE-006). No hash record at 19968 (ENG-HASH-003).
13. **Mac**: `Read{after 19968}` → page 19969–20000 (32 entries, `truncated` 0). Dry run from `M_19968`, `C19968`: each Accept; `based_on_seq` below 19968 → `based_on_unchecked` (warning count); persist, apply. Among them the `snap` entry naming `SNAP19968`: author DESK, `seq 19968`, `hash`, `sha256`, `size`, `parts`, `inflated` and `epoch 1` equal what was opened (§19.7 step 5); Mac gives no verdict (did not apply 19968). Phone's later beat in this range acks it → usable (§19.3). Log reaches head 20000 → rejoin done; `continuity_unchecked = Some(19968)`; `status.warnings = [ContinuityUnchecked{from 19968}]`.
    AMBIGUOUS: that Phone's acknowledging beat lies above 19968 (it must follow the `snap` entry, which follows 19968), so Mac sees it; if not, step 5 tries the next candidate.
14. **Mac**: runtime raises the `IdMinter` from the marks now held (engine.md §11.3; device.md §8.3). Reconcile complete → `Live`.
15. **Mac (send loop)**: `Q` sealed: `ctr = 1 + max(60, 60, 60) = 61`, epoch 1, `based_on (100, C100)` and its old `at_ms` unchanged (ENG-ENQ-005); a beat is due (last beat 400 days ago) and enqueued after `Q` (ctr 62, `state` absent: no hash at 19968). Append batch of 2 → seqs 20001, 20002.
16. **Every device** classifies 20001: Mac current, ctr 61, epoch 1 = E, `based_on 100` below each holder's retained chain → unchecked (seal.md §14.8 step 3); Accept; `apply(Ctx{20001, 2, old at_ms, based_on 100})`: the rename applies if the task is live (overwrite notice if its `name` stamp is another device's change after 100), else refused `core/gone` with a refusal record on Mac.
    AMBIGUOUS: the scenario does not say whether the task is still live at 20000.
17. **Mac → User**: banner/warning "continuity before seq 19968 could not be checked" (`ContinuityUnchecked { from: 19968 }`) until `ack_continuity`.

**State after.** Mac: `log_floor 19969`, checkpoint and bases at 100 and 19968, head `(20002, C20002)`, MAC `last_ctr 62`, epoch 1, outbox empty, `continuity_unchecked 19968`. Relay: floor 3000, MAC `last_ctr 62`.
