# trust, reader B

Symbols: `ctx(x)` = ASCII `taskdeck/v1/x` + `0x00`. `DEV_R` = `H(ctx("recovery-dev") ‖ BID ‖ RSIGN)[0..16]` (seal.md §6.3). `T_X` = the stored token text of device X (`"TD1."` ‖ base64url of 69 bytes, relay.md §5.1). `K1` = `K_entry,1` = `HKDF(salt = BID; ikm = R_1; info = "taskdeck/v1/key-entry\0" ‖ u32 1; L = 32)` (seal.md §6.5). `FH_s` = `H(frame at s)`; `C_s` = chain value (seal.md §15.1). `τ_s` = log clock after s. Times: `T0` = 1,795,341,600,000. All socket messages are binary WebSocket messages (relay.md §11.3). `BIDhex` = 32 lowercase hex of BID.

## Scenario 4: revocation with rotation

Extra facts I fix: Phone's queued edit (made at `T0 − 1 h` = 1,795,338,000,000) is a `cmd` record `{entry_id = RAND1[16], at_ms = 1,795,338,000,000, based_on = (200, C200), via Append}`, unsealed (no `ctr`, no frame), durable in Phone's `outbox.tdo` (engine.md §9.1, ENG-SEND-002). `t1` = Desk's `wall_ms` when it enqueues the rotation (≈ `T0` plus one admin round trip). No `read_tokens` row matters (if any exist they are all revoked in step 9).

1. **Mac** (`T0 − 5 s`): the user renames "Call plumber" (id 1,099,511,627,783); the facade compiles `rename` against the pending view with `now` = 1,795,341,595,000 and sends `Enqueue` (device.md §17.4, DEV-FACADE-001; core.md §9.7). Command bytes (canonical, core.md §4.2), 60 bytes:
   `{"id":1099511627783,"name":"Call the plumber","op":"rename"}`
2. **Mac**: trial apply with `ctx = {seq 201, device_no 2, at_ms 1,795,341,595,000, based_on 200}` succeeds; draws `entry_id` = RAND2[16] (`Sealer::new_entry_id`); `Persist(Outbox[Put{entry_id RAND2, order, at_ms 1,795,341,595,000, based_on (200, C200), kind cmd, body, origin_state, via Append, hold false}])`; on `Persisted`, `Notify(Enqueued{provisional_seq 201})` (engine.md §9.1, ENG-ENQ-004).
3. **Mac**: send loop (Live, epoch gate open): `ctr = 1 + max(60, –, 60) = 61` (engine.md §10.3); `seal(M_K, 61, entry)` under epoch 1 (SEAL-ENV-003), nonce RAND3[24]. Frame `F202` (seal.md §8.1, §9.1, §10.1):

   Plaintext (512 bytes):
   | field | width | value |
   |---|---|---|
   | entry_id | 16 | RAND2 |
   | kind | 1 | 1 (`cmd`) |
   | at_ms | 8 | 1,795,341,595,000 |
   | based_on_seq | 8 | 200 |
   | based_on_chain | 32 | C200 |
   | payload_len | 4 | 60 |
   | payload | 60 | the command bytes of step 1 |
   | padding | 383 | `0x00` (`P(129) = 512`, seal.md §10.2) |

   AD (72 bytes, seal.md §9.2): `ctx("entry")` [18] ‖ `F202[0..48]` [48] ‖ ver 1 [1] ‖ suite 1 [1] ‖ u32 epoch 1 [4].

   Frame `F202` (670 bytes):
   | field | width | value |
   |---|---|---|
   | magic | 4 | `"TDF1"` |
   | board_id | 16 | BID |
   | dev | 16 | MAC |
   | ctr | 8 | 61 |
   | len | 4 | 558 |
   | ver | 1 | 1 |
   | suite | 1 | 1 |
   | epoch | 4 | 1 |
   | nonce | 24 | RAND3 |
   | ciphertext | 528 | `AEAD(K1; RAND3; AD; plaintext)` (512 + 16 tag) |
   | sig | 64 | `SIG(MAC signing key; "taskdeck/v1/frame\0"; F202[0..606])` |

   `Persist(Outbox[Put{…, ctr 61, frame F202, frame_hash FH202 = H(F202), epoch 1}])` (D2, ENG-SEND-003).
4. **Mac**: on `Persisted`, `Append{req, [F202]}` on its socket (engine.md §10.2 step 5; device.md §10.5): `0x02 ‖ u32 r ‖ "TDA1" ‖ u8 1 ‖ u32 670 ‖ F202` (684 bytes, relay.md §9.2, §11.3). The message is in flight until after step 15 (scenario fact).
5. **User on Desk** (`T0`): Devices, Phone, "Remove"; the desktop runs device authentication (Windows Hello / Touch ID) and calls `revoke(PHONE, replace = false)`; the facade sends `Op{ticket, Revoke{dev PHONE, replace false}}` and returns the ticket (device.md §17.8 `revoke` row, "auth"; §17.1).
6. **Desk**: checks the target (PHONE current in `M_K`, not self, not the last current device): passes (engine.md §28.5, `engine/revoke_target`); `Persist(WriteMeta{ops: [OpRecord{ticket, Revoke, stage 1, attempts 0, started_ms T0}]})`, then on `Persisted` emits `Admin{req, Revoke{dev PHONE}}` (D10, ENG-OPS-003, ENG-OPS-002).
7. **Desk → Relay**: `POST https://relay.example/v1/b/BIDhex/admin/nonce`, `Authorization: Bearer T_DESK`, `Content-Type: application/octet-stream`, body DESK [16] (device.md §10.5 last Admin row; relay.md §12.1).
8. **Relay**: gate (route, fixed length 16, token MAC and board, relay.md §6.2); object: Desk row role 1, state 1, gen 1 (§8.2); signer = token's dev, row current, board open; in one transaction deletes `nonces` rows older than 120,000 ms and inserts `(RAND4[32], purpose 2, DESK, now)`, RAND4 from `crypto.getRandomValues` (§12.1, §10.3 step 1, RELAY-ADMIN-001). Answer 200 `{"nonce":"<hex RAND4>","expires":<now+120000>}`.
9. **Desk → Relay**: `POST …/admin/revoke`, `Authorization: Bearer T_DESK`, body 136 bytes (relay.md §12.1; seal.md §16.2):

   Admin message signed (121 bytes):
   | field | width | value |
   |---|---|---|
   | context | 18 | `"taskdeck/v1/admin\0"` |
   | op | 7 | `0x06 ‖ "revoke"` (vb8) |
   | board_id | 16 | BID |
   | signer | 16 | DESK |
   | args_hash | 32 | `H(PHONE)` (args = PHONE [16]) |
   | relay_nonce | 32 | RAND4 |

   Request body:
   | field | width | value |
   |---|---|---|
   | signer | 16 | DESK |
   | nonce | 32 | RAND4 |
   | sig | 64 | `SIG(DESK signing key; "taskdeck/v1/admin\0"; op ‖ BID ‖ DESK ‖ H(PHONE) ‖ RAND4)` |
   | args_len | 4 | 16 |
   | args | 16 | PHONE |
   | extra_len | 4 | 0 |
10. **Relay**: gate; object checks the token row; A1 (lengths, args width 16, extra empty), A2 (open), A3 (T1, signer is the token's dev), A4 (nonce RAND4 purpose 2 for DESK, fresh; deleted now), A5 (Desk current role 1, not the target), A6 (`verifyStrict(DESK_SIGN, admin message, sig)`), A7 revoke checks (PHONE row role 1 state 1; signer ≠ target; one current role-1 row remains) (relay.md §12.1, §12.4). One transaction: PHONE row `state = 2`, `gen = 2`, `wraps = NULL`; every `read_tokens` row `state = 2` (RELAY-REVOKE-001, RELAY-REVOKE-003, §5.2). After commit: closes PHONE's sockets with 4003 (none open). Answer 200 `{}`.
11. **Desk**: `AdminResult{req, Ok(AdminOk{all None, token false})}`; stage 2: draws `entry_id` RAND5[16]; `Persist(WriteMeta{stage 2}, Outbox[Put{entry_id RAND5, order, at_ms t1, based_on (200, C200), kind ctl, body Ctl(Rotate{revoke [PHONE], reinstate [], purge_before None}), via Append, op ticket}])` (engine.md §28.5 step 2, §9.2, ENG-ENQ-003).
    AMBIGUOUS: whether the stage-2 record's `entry_id` is drawn at enqueue (as ENG-ENQ-003 says for "each new record") or at seal; engine.md §9.2 gives only `at_ms` and `based_on` for composed entries. I draw it at enqueue.
12. **Desk**: send loop: `ctr = 1 + max(121, –, 121) = 122`; `Sealer::seal(M_K, 122, entry)` composes the items from the intent and `M_K`: `DeviceRevoked{PHONE}`, then `EpochRotated{epoch 2}` (shape R, seal.md §12.1), drawing in this order `R_2` = RAND6[32] (OsEntropy, SEAL-KEY-010), then one HPKE ephemeral key per wrap, RAND7[32], RAND8[32], RAND9[32], in ascending recipient-id order, then the entry nonce RAND10[24] (engine.md §6.1 `seal`).
    AMBIGUOUS: the order of the random draws inside one `seal` call (root, the three HPKE encapsulations, the nonce) and the number of bytes HPKE `SealBase` draws through `Entropy` are not fixed (seal.md §7, §26; SEAL-GEN-004).
    AMBIGUOUS: engine.md §6.1 `seal` says the wraps go "to the current devices of `m`", which in `M_K` include PHONE; seal.md §13.6 requires exactly the current devices of `W` after the revoke item (`seal/ctl_wraps`). I wrap to DESK, MAC and DEV_R only.

    Commitment input (70 bytes): `ctx("epoch")` [18] ‖ BID [16] ‖ u32 2 [4] ‖ R_2 [32]; `c_2 = H(…)` (seal.md §6.5).

    For each recipient X in {DESK, MAC, DEV_R} (pk `DESK_HPKE`, `MAC_HPKE`, `RHPKE`), seal.md §7:
    | info_X (85 bytes) | width | value |
    |---|---|---|
    | context | 17 | `"taskdeck/v1/wrap\0"` |
    | board_id | 16 | BID |
    | epoch | 4 | u32 2 |
    | recipient_id | 16 | X (`DEV_R` for the recovery key) |
    | recipient_pk | 32 | X's HPKE key |

    | aad (61 bytes) | width | value |
    |---|---|---|
    | context | 21 | `"taskdeck/v1/wrap-aad\0"` |
    | ver | 1 | 1 |
    | suite | 1 | 1 |
    | kem | 2 | `0x0020` |
    | kdf | 2 | `0x0001` |
    | aead | 2 | `0x0003` |
    | commit | 32 | c_2 |

    | wrap W_X (96 bytes) | width | value |
    |---|---|---|
    | recipient_id | 16 | X |
    | enc | 32 | from `HPKE(X's HPKE key; info_X; aad; R_2)` |
    | ct | 48 | from the same (32 + 16 tag) |

    `items_bytes` (772 bytes), wraps ascending by recipient id (`W_a`, `W_b`, `W_c` = the three wraps sorted by their first 16 bytes):
    `[{"dev":"<hex PHONE>","type":"DeviceRevoked"},{"commit":"<hex c_2>","epoch":2,"type":"EpochRotated","wraps":["<hex W_a>","<hex W_b>","<hex W_c>"]}]`
    (`purge_before` absent, seal.md §12.5; keys sorted, core.md §4.2.)

    Inner signed bytes (112 bytes, seal.md §11.3):
    | field | width | value |
    |---|---|---|
    | context | 16 | `"taskdeck/v1/ctl\0"` |
    | board_id | 16 | BID |
    | dev | 16 | DESK |
    | ctr | 8 | 122 |
    | entry_id | 16 | RAND5 |
    | at_ms | 8 | t1 |
    | items hash | 32 | `H(items_bytes)` |

    `ISIG = SIG(DESK signing key; "taskdeck/v1/ctl\0"; BID ‖ DESK ‖ u64 122 ‖ RAND5 ‖ i64 t1 ‖ H(items_bytes))`.
    Payload (919 bytes): `{"items":` ‖ items_bytes ‖ `,"sig":"<hex ISIG>"}`.

    Plaintext (1024 bytes):
    | field | width | value |
    |---|---|---|
    | entry_id | 16 | RAND5 |
    | kind | 1 | 2 (`ctl`) |
    | at_ms | 8 | t1 |
    | based_on_seq | 8 | 200 |
    | based_on_chain | 32 | C200 |
    | payload_len | 4 | 919 |
    | payload | 919 | above |
    | padding | 36 | `0x00` (`U = 988`, `P(988) = 1024`) |

    Frame `F201` (1182 bytes), sealed under epoch 1 (SEAL-ENV-004):
    | field | width | value |
    |---|---|---|
    | magic | 4 | `"TDF1"` |
    | board_id | 16 | BID |
    | dev | 16 | DESK |
    | ctr | 8 | 122 |
    | len | 4 | 1070 |
    | ver | 1 | 1 |
    | suite | 1 | 1 |
    | epoch | 4 | 1 |
    | nonce | 24 | RAND10 |
    | ciphertext | 1040 | `AEAD(K1; RAND10; ctx("entry") ‖ F201[0..48] ‖ 0x01 ‖ 0x01 ‖ u32 1; plaintext)` |
    | sig | 64 | `SIG(DESK signing key; "taskdeck/v1/frame\0"; F201[0..1118])` |

    The sealer keeps R_2 as pending under RAND5 (`keys_changed`). `Persist(Outbox[Put{…, ctr 122, frame F201, frame_hash FH201, epoch 1}], WriteIdentity)` (D2, ENG-SEALER-004).
13. **Desk → Relay**: on `Persisted`, socket: `0x02 ‖ u32 r ‖ "TDA1" ‖ u8 1 ‖ u32 1182 ‖ F201` (1196 bytes).
14. **Relay**: re-reads Desk's row (relay.md §11.4, RELAY-SOCK-007); P1 (1182 bytes, `"TDF1"`, len = 1070), P2, P3, P4, P5 (dev = token's), P6, P7 (`verifyStrict(DESK_SIGN, ctx("frame") ‖ F201[0..1118], sig)`) (§9.3); transaction: T1, T2 (122 = 121 + 1), T4 (budget), T5: seq 201, `at = max(now, at_200)`, `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ FH201)` (90-byte input), log row `(201, DESK, 122, FH201, C201, at, F201)`; Desk row `last_ctr 122, last_seq 201, app_count + 1` (§9.4, RELAY-STORE-003).
15. **Relay → Desk, Mac**: result to Desk `0x82 ‖ u32 r ‖ "TDR1" ‖ u8 1 ‖ 0x00 ‖ u64 201 ‖ C201` (51 bytes); push to both ready sockets `0x84 ‖ "TDP1" ‖ u64 201 ‖ C201 ‖ i64 relay_now ‖ u64 floor 1 ‖ u64 purge_floor 1 ‖ u8 flags 0 ‖ u16 1 ‖ u64 201 ‖ i64 at ‖ u32 1182 ‖ F201` (1274 bytes) (§9.6, §10.5, §11.4, RELAY-SOCK-008).
    AMBIGUOUS: the order on the author's socket of the `result` and the `push` of the same commit is not fixed (relay.md §10.6, §11.4). I send the result first.
16. **Relay**: Mac's append (step 4) arrives; P1–P7 pass (Mac current); T2 (61 = 60 + 1); T5: seq 202, `C202 = H(ctx("chain") ‖ C201 ‖ u64 202 ‖ FH202)`; Mac row `last_ctr 61, last_seq 202`. Result to Mac `0x82 ‖ u32 r ‖ "TDR1" ‖ 1 ‖ 0x00 ‖ u64 202 ‖ C202`; push of entry 202 (`0x84 ‖ page`, 762 bytes) to Desk and Mac (§9.4, §11.4). The relay cannot see epochs: a frame under epoch 1 is stored like any other (§9.1).
17. **Mac**: push of 201 arrives as `Page{req None}`; its first entry is `L + 1` = 201, so it is processed as a page (engine.md §12.2): limits, clock (`relay_now`), head (201 > L: no evidence), no truncation, contiguity (engine.md §12.3 steps 1–5).
18. **Mac**: dry run: `C201 = chain_step(C200, 201, F201)`; `classify` with `M_200` (§12.3 step 6). seal.md §14.2: 1 format; 2 board; 3 DESK current; 4 sig under DESK_SIGN; 5 ctr 122 = 121 + 1; 6 not own dev; 7 current; 8 (1, 1) allowed by rules 1; 9 epoch 1 = E; 10 opens under K1 with the AD above; 11 unpadding (1024 ≥ 69, U = 988, P(988) = 1024, 36 zero bytes, kind 2, at_ms in range); 12 `based_on` 200 < 201 and C200 equals its own; 13 `ctl` by a current device under the current epoch; 15 payload is canonical, ISIG verifies under DESK_SIGN; 16 shape R; `DeviceRevoked`: author D ≠ target, PHONE current, one current device remains; `EpochRotated`: 2 = 1 + 1, wraps name exactly DESK, MAC, DEV_R ascending, no `purge_before` → `Accept` (seal.md §12.1, §13.6).
19. **Mac**: as a recipient, opens its wrap: `OpenBase` with `info` = `ctx("wrap") ‖ BID ‖ u32 2 ‖ MAC ‖ MAC_HPKE`, `aad` with `c_2` from the item (SEAL-WRAP-002), gets R_2, checks `H(ctx("epoch") ‖ BID ‖ u32 2 ‖ R_2) = c_2` (SEAL-RCPT-007, SEAL-KEY-013): equal, no halt; `keys_changed`. `M_201`: PHONE `status revoked, status_seq 201, status_by DESK`; `epoch 2, epoch_seq 201, epoch_clock_ms = clock_ms = τ201 = max(τ200, t1) = t1`; commitments `+ (2, 201, c_2)`; DESK `last_ctr 122`; `ctl_seqs + 201` (seal.md §13.5, §13.6).
20. **Mac**: `Persist(AppendLog[(201, C201, t1, F201)], SetMarker{201, 201, attempt}, WriteIdentity)` (§12.3 step 8, D3, D4).
21. **Mac**: on `Persisted`: `L = K = 201`, `M_K = M_201`; membership effects: banner `DeviceRevoked{id (201, 0), seq 201, at_ms t1, author DESK, author_name "Desk", author_fp kfp(DESK), subject PHONE, subject_name, subject_fp, detail None}` (no `KeysRotated`: the entry has a revoke item); epoch gate armed (engine.md §14.1, §14.2, ENG-BANNER-001); banner persisted with `WriteMeta`.
22. **Mac**: board applies 201 (`ctl`: no board effect), `B = 201`; rebase (run ended); `Notify(BannersChanged, StatusChanged, ViewChanged)` (engine.md §12.4, §8.4 case 1).
23. **Mac**: epoch gate: `M_K.epoch` 2 > epoch 1 of its own record (F202, framed, not seen), so it seals nothing new until F202 is seen (engine.md §24.2, ENG-EPOCH-001). Its pending view still shows "Call the plumber".
24. **Mac**: result for F202: `new (202, C202)` → `ack = (202, C202)`, `Put` in the next `Persist`; 202 > L, so the echo check waits (engine.md §10.4).
25. **Mac**: push of 202: dry run `C202 = chain_step(C201, 202, F202)`; classify with `M_201`: steps 1–5 pass (ctr 61 = 60 + 1); 6 `own_sealed(61) = FH202`, no flag; 7 current; 8; 9 epoch 1 = E − 1 with E = 2; 10 opens under K1; 11; 12 `based_on` (200, C200) matches; 13 `cmd` by a current device under the previous epoch is allowed by the table; 14 stale-epoch rule: `cmd`, current device, `based_on_seq 200 < epoch_seq 201`, `clock_ms − epoch_clock_ms = t1 − t1 = 0 ≤ 2,592,000,000` → `Accept` (seal.md §13.9, §14.2, SEAL-MEM-018; engine.md §24.1). `M_202`: MAC `last_ctr 61`, `clock_ms = max(t1, 1,795,341,595,000) = t1`.
26. **Mac**: `Persist(AppendLog[(202, C202, t1, F202)], SetMarker{202, 202, attempt})`; on `Persisted`: `L = K = 202`; echo check: frame hash FH202 and C202 equal the ack (ENG-ECHO-001); apply `rename` under `Ctx{seq 202, device_no 2, at_ms 1,795,341,595,000, based_on 200}`: name "Call the plumber", name stamp `(202, 2)`; the old name stamp has seq ≤ 200, so no overwrite notice (core.md §10.1, §10.2); `seen = 202`, `Outbox[Drop(RAND2)]`; `B = 202`; the epoch gate opens (§12.4, §24.2). Mac's next frame will be sealed under epoch 2 with ctr 62.
27. **Desk**: result `new (201, C201)` → `ack`; push of 201 → the same checks as step 18 with `own_sealed(122) = FH201` (no flag); `Accept`; opens its own wrap (`info` with DESK, DESK_HPKE), R_2 matches `c_2`, `keys_changed`; `Persist(AppendLog[201], SetMarker, WriteIdentity)`; commit: no banner (Desk is the author, ENG-BANNER-001); apply; echo check equal; `seen = 201`, `Outbox[Drop(RAND5)]`; the op reaches stage 3 "accepted: done": `WriteMeta` without the op, `Notify(OpDone{ticket, Ok(())})`; no `ReplaceReady` (`replace` false) (engine.md §28.5 step 3).
28. **Desk**: push of 202: classified as in step 25 (author MAC, stale-epoch rule) → `Accept`; `Persist(AppendLog[202], SetMarker)`; applies the rename (no notice: the item's name stamp predates 200 and the rename is Mac's) (engine.md §12.3, §13.1).
29. **Phone** (`T0 + 1 h` = 1,795,345,200,000): the user opens the app: `MainActivity.onStart` → `set_zone`, `set_foreground(true)` → `notify_resume` (bridge.md §14.1; device.md §10.9). `Device::open` drew session id RAND11[16] (device.md §10.4). Engine `Offline`, relay target set → `Connect{relay}` (engine.md §11.1).
    AMBIGUOUS: "comes online" can be the app opened (socket) or a background job (`background_sync`, HTTPS `Hello`); the outcome is the same `403 relay/revoked`. I take the app opened.
30. **Phone → Relay**: no backoff (after resume); no login (it holds a token, the engine is not stopped `Revoked`, device.md §10.3 cases 1–4); the stored capability says push: `GET wss://relay.example/v1/b/BIDhex/socket` with `Host`, `User-Agent: TaskDeck-Sync/1`, `Authorization: Bearer T_PHONE`, the upgrade headers (device.md §10.2, §10.5). Token bytes inside `T_PHONE` (69 bytes, relay.md §5.1):
    | field | width | value |
    |---|---|---|
    | role | 1 | 1 |
    | board_id | 16 | BID |
    | dev | 16 | PHONE |
    | gen | 4 | 1 |
    | mac | 32 | `MAC(TOKEN_KEY; ctx("token") ‖ token[0..37])` |
31. **Relay**: gate passes (form, MAC, board, role 1; generation not checked at the gate, §6.3); object: PHONE row role 1, `state = 2` → refuses the upgrade with `403 relay/revoked`: HTTP JSON `{"error":"relay/revoked"}`, `Cache-Control: no-store`, `X-TD-Relay: object`, `Content-Type: application/json`; no row written (relay.md §8.2, §11.1, RELAY-OBJ-003, RELAY-OBJ-004).
32. **Phone**: the transport delivers `RelayRefused{req None, 403 relay/revoked}` (device.md §10.6 row "403 relay/revoked answering a whole request") and then `Disconnected{unreachable false}`.
    AMBIGUOUS: device.md §10.6 has no row for a refused socket upgrade: whether `req` is `None` and whether a `Disconnected` follows (without one the engine stays `Connecting` and the hourly `Connect` guard of engine.md §11.1, which starts from `Offline`, never fires). I deliver both.
33. **Phone**: engine: `403 relay/revoked` → revoked with `source: relay` (engine.md §11.9, §24.4 case 2): the stop `Revoked` with action `RePair` is persisted in `Meta`; nothing is deleted (ENG-SESS-005, ENG-EPOCH-002); no banner (no log item), no alarm; session `Offline`; `Notify(StatusChanged)`.
34. **Phone**: shows: the masthead line `Sync stopped — open Devices · 1 waiting` (bridge.md A-03 row 3 and the `queued` suffix; sync data `stopped true`, `queued 1`, bridge.md §5.4); Devices → alarms section: "the relay says this device was removed" with the action `RePair` (opens Pair in re-pair mode) (engine.md §24.4; bridge.md §13.1). Its pending view (state at 200 plus its queued edit) is unchanged; its record stays unsealed in the outbox (ENG-STATE-001).
35. **Phone**: further edits are still accepted (a relay-only revocation does not refuse, engine.md §9.1 step 1) and stay local; nothing is sent (§10.1).
36. **Phone** (`T0 + 2 h` = 1,795,348,800,000, if the app is still open): `Connect` (at most once per 3,600,000 ms of `mono_ms`, ENG-EPOCH-004); login because the engine is stopped `Revoked` (device.md §10.3 case 4): `POST …/login/nonce`, `Authorization: Bearer T_PHONE`, body PHONE [16]; the relay finds the row role 1 (generation not checked on login routes), then `state = 2` → `403 relay/revoked`, no nonce inserted (relay.md §8.2, §10.3 step 1). Transport: `RelayRefused{req None, 403 relay/revoked}`, `Disconnected{unreachable false}` (device.md §10.6); the engine stays revoked (`source: relay`). This repeats hourly; Phone never reads entry 201, so it never learns the revocation from the log.

**State at the end.**

| | Desk | Mac | Phone |
|---|---|---|---|
| L = K = B | 202 | 202 | 200 |
| chain | C202 = H(ctx("chain") ‖ C201 ‖ u64 202 ‖ FH202), C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ FH201) | same | C200 |
| membership | `M_202`: epoch 2 (epoch_seq 201, epoch_clock_ms t1), commitments (1, 1, c_1), (2, 201, c_2); PHONE revoked (status_seq 201, status_by DESK, last_ctr 18); DESK last_ctr 122; MAC last_ctr 61; DEV_R last_ctr 1; clock_ms t1; ctl_seqs + 201 | same | `M_200`, epoch 1, PHONE current |
| roots in identity.tdk | R_1, R_2 | R_1, R_2 | R_1 |
| next ctr / epoch | 123, epoch 2 | 62, epoch 2 | none while revoked (would be 19, epoch 1) |
| outbox (live) | empty (Put, Put with frame, Put with ack, Drop of RAND5) | empty (same ops for RAND2) | one unsealed `cmd` record RAND1 |
| log files | + records 201, 202 | + 201, 202 | unchanged |
| meta | no op; no banner | banner DeviceRevoked (201, 0) unacknowledged | stop `Revoked` (source relay), action RePair |
| history.tdk | + record of ctl entry 201 (device.md §15.7) | + same | unchanged |
| checkpoint | unchanged until its cadence (engine.md §18.1) | same | unchanged |

Relay: log 1–202, head (202, C202); DESK row `state 1, gen 1, last_ctr 122, last_seq 201`; MAC `state 1, gen 1, last_ctr 61, last_seq 202`; PHONE `state 2, gen 2, wraps NULL, last_ctr 18`; DEV_R `role 2, last_ctr 1, last_seq 1`; every `read_tokens` row `state 2`; RAND4's nonce row deleted; floors 1; board `open`.

What the users saw: Desk: the Remove flow with OS authentication, then the operation done; Phone gone from current devices. Mac: banner "Desk removed Phone" (author Desk with key fingerprint, subject Phone) to acknowledge; the rename kept. Phone: `Sync stopped — open Devices · 1 waiting`, "the relay says this device was removed", re-pair and resubmit offered.

## Scenario 5: relay rollback

Extra facts I fix: the relay's restored rows have DESK `last_ctr 115`, MAC `last_ctr 57`, PHONE `last_ctr 17` (their frames in 191–200 carry DESK 116–121, MAC 58–60, PHONE 18), gens 1, floors 1, no fence; `last_seq` values `q_D115`, `q_M57`, `q_P17` ≤ 190. Seqs 191–200 hold no control entry (no membership change in the world). The ten frames total at most 1,048,576 bytes. Desk's process did not restart (same session id `SESS_D`); Mac's socket dropped when the relay was restored (same process, `SESS_M`).

1. **Desk** (`T0`): `Connect{relay}`; token `T_DESK` (gen 1) held, no login; socket upgrade `GET wss://relay.example/v1/b/BIDhex/socket`, `Authorization: Bearer T_DESK` (device.md §10.5); relay: row DESK role 1, state 1, gen 1 → 101, attachment `{dev DESK, gen 1, ready false}` (relay.md §11.1, §11.2).
2. **Desk → Relay**: hello `0x01 ‖ SESS_D[16] ‖ u32 0` (21 bytes) (relay.md §11.3).
3. **Relay → Desk**: no other socket of DESK with a session; stores session, features 0, ready true; welcome (93 bytes):
   | field | width | value |
   |---|---|---|
   | type | 1 | `0x81` |
   | head_seq | 8 | 190 |
   | head_chain | 32 | C190 |
   | last_ctr | 8 | 115 |
   | last_seq | 8 | q_D115 |
   | relay_now | 8 | ≈ T0 |
   | floor | 8 | 1 |
   | purge_floor | 8 | 1 |
   | state | 1 | 1 |
   | caps | 1 | `0x01` |
   | appends_left | 4 | 3,000 − Desk's app_count today |
   | door | 5 | DOOR |
   | has_fence | 1 | 0 |
4. **Desk**: `Connected{push true}`, then `Welcome`. Checks (engine.md §11.2): 1 skew from `relay_now`; 2 `head.seq` 190 < L 200 → evidence `relay_behind`; its own `C190` equals `head_chain` → no `fork`; 3 no record has an ack (outbox empty); 5 reconcile: `r = 115 < a = 121` → `relay_behind` (§11.3 step 1). The session stays `Connecting` (session table, row "a check of §11.2 fails").
   AMBIGUOUS: engine.md §11.2 lists its checks "in order" without saying whether a failing check ends the list, so step 2 and §11.3 step 1 may give one alarm or two. I record one alarm carrying both facts.
5. **Desk**: `Persist(WriteMeta{alarms + AlarmRecord{id A1, kind RelayBehind, raised_ms T0, seq –, author –, code –, evidence RelayBehind{relay_head (190, C190), relay_last_ctr Some(115), local_head (200, C200), ack None}, resolved false}, stops + {RelayBehind evidence stop, since T0, alarm A1, action RestoreTail}})`; on `Persisted`, `Notify(AlarmRaised{A1}, StatusChanged)` (engine.md §21.1, ENG-EVID-002, §21.2). Under the stop: no `Append`, no `Read`, nothing persisted from the relay, `Enqueue` accepted (ENG-EVID-003).
6. **Desk**: shows (engine.md §30.2): the sync line with the stop; the alarm evidence "relay is at 190 (chain C190), counter 115 for this device; this device holds 200" with the action RestoreTail (the desktop's words; `Status.stops`, `Status.alarms`).
7. **Desk**: the relay's chain at its head equals Desk's `C190` (a plain rollback), so Desk takes the restore-tail duty itself (no designated device; eligible apart from the stop, ENG-DUTY-002) (engine.md §21.3, ENG-EVID-004). It reads frames 191–200 from its own log: `ReadLog{req, 191, 200}` → `LocalRead`.
   AMBIGUOUS: the engine's in-memory state (engine.md §7.1) holds no frames, and §21.3 does not say whether the restore tail reads the frames with `ReadLog`; I use `ReadLog`.
8. **Desk**: one request suffices (10 frames ≤ 64, ≤ 1 MiB, no control entry among them): `Admin{req, RestoreTail{first_seq 191, chain_before C190, frames F191 … F200}}` (engine.md §21.3 step 1, §11.7). Exempt from `Live` (ENG-SESS-001).
9. **Desk → Relay**: `POST …/admin/nonce`, `Bearer T_DESK`, body DESK [16]; relay inserts `(RAND1[32], purpose 2, DESK, now)`; answer `{"nonce":"<hex RAND1>","expires":<now+120000>}` (relay.md §12.1).
10. **Desk → Relay**: `POST …/admin/restore_tail`, `Bearer T_DESK` (relay.md §12.1, §12.9; seal.md §16.2):

    `args` (332 bytes):
    | field | width | value |
    |---|---|---|
    | first_seq | 8 | 191 |
    | n | 4 | 10 |
    | frame_hash × 10 | 320 | FH191, …, FH200 |

    Admin message signed (127 bytes): `"taskdeck/v1/admin\0"` [18] ‖ `0x0C ‖ "restore_tail"` [13] ‖ BID [16] ‖ DESK [16] ‖ `H(args)` [32] ‖ RAND1 [32].

    Request body:
    | field | width | value |
    |---|---|---|
    | signer | 16 | DESK |
    | nonce | 32 | RAND1 |
    | sig | 64 | `SIG(DESK signing key; "taskdeck/v1/admin\0"; 0x0C ‖ "restore_tail" ‖ BID ‖ DESK ‖ H(args) ‖ RAND1)` |
    | args_len | 4 | 332 |
    | args | 332 | above |
    | extra_len | 4 | 32 + Σ (4 + len_i) |
    | extra | 32 + Σ (4 + len_i) | C190 ‖ for i = 191..200: `u32 len_i ‖ F_i` |
11. **Relay**: gate (body ≤ 1,051,044); token row; A1–A6 (signer DESK, a current device; nonce RAND1 deleted); A7 restore_tail checks in order: `first_seq 191 = head_seq 190 + 1`; `chain_before = head_chain C190`; for each frame in seq order: `H(F_i) = FH_i`; P1–P3; author row exists (DESK, MAC, PHONE); `verifyStrict`; ctr = the author's `last_ctr` + 1 counting earlier frames of the request (DESK 116…121, MAC 58…60, PHONE 18) (relay.md §12.9, RELAY-RESTORE-001).
12. **Relay**: one transaction: each frame stored as T5 without the budget: seq 191…200, `C_s = H(ctx("chain") ‖ C_{s−1} ‖ u64 s ‖ FH_s)` (equal to the devices' C191…C200), `at = max(now, previous at)`; rows DESK `last_ctr 121, last_seq s_D121`, MAC `60, s_M60`, PHONE `18, s_P18`, each written once; no role, state, key or floor changed (RELAY-RESTORE-002). After commit: push to Desk's ready socket `0x84 ‖ page{head 200, C200, relay_now, floor 1, purge_floor 1, flags 0, n 10, entries 191…200 each u64 seq ‖ i64 at ‖ u32 len ‖ frame}`. Answer 200 `{"head":{"seq":200,"chain":"<hex C200>"}}`.
13. **Desk**: the push: head (200, C200) agrees; every entry is ≤ L with the frame its log holds → ignored as repeated; nothing persisted (engine.md §12.3 step 5, ENG-EVID-003).
14. **Desk**: `AdminResult{Ok(AdminOk{head Some((200, C200))})}`: the relay head is at L with Desk's chain and no acknowledgement is outstanding → evidence resolved, stop ended: `Persist(WriteMeta{A1 resolved, stop removed})`, `Notify(StatusChanged)` (engine.md §21.3 step 5, ENG-EVID-005).
15. **Desk**: emits `Hello`: `GET …/hello`, `Bearer T_DESK`; answer `{"head":{"seq":200,"chain":"<hex C200>"},"last_ctr":121,"last_seq":<s_D121>,"appends_left":<n>,"relay_now":<ms>,"floor":1,"purge_floor":1,"state":"open","door":"<hex DOOR>","push":true}` (relay.md §10.4) → `Welcome`: checks pass (head 200 = L, chain equal); reconcile `a = r = 121`, no framed record → normal; head = L, no read; session `Live` (engine.md §11.1–§11.3).
    AMBIGUOUS: after an evidence stop ends, engine.md §11.1 and §21.3 do not say how a session left in `Connecting` by the failed welcome continues (a new `Hello`, or the earlier welcome re-evaluated). I send `Hello`.
    AMBIGUOUS: when a welcome's head equals L, no page arrives, and engine.md §11.1 moves to `Live` only on a `Page`; I take `Live` at once (reconcile complete, L = relay head).
    AMBIGUOUS: engine.md §11.3 does not define `h` when no record has a frame; I take `h = a`.
16. **User on Desk** (`T0 + 30 s` = 1,795,341,630,000): quick-adds "Pay rent", no due date; `submit` compiles `quick_add` to `create_task{id 1·2^40 + 41, name, time_importance 1}` (core.md §13.4 quick_add row; Desk minted up to 40) with `now` = 1,795,341,630,000 and sends `Enqueue{hold false}` (device.md §17.4). Command bytes (77 bytes):
    `{"id":1099511627817,"name":"Pay rent","op":"create_task","time_importance":1}`
17. **Desk**: trial apply with `ctx = {seq 201, device_no 1, at_ms 1,795,341,630,000, based_on 200}`; `entry_id` RAND2[16]; `Persist(Outbox[Put{RAND2, at_ms 1,795,341,630,000, based_on (200, C200), kind cmd, via Append}])`; on `Persisted`, `Notify(Enqueued{provisional_seq 201})`; the user sees the task at once (engine.md §9.1). (Had the restore not finished yet, this step would be the same and step 18 would wait for the stop to end, ENG-EVID-003.)
18. **Desk**: send loop: `ctr = 1 + max(121, –, 121) = 122`; epoch 1; nonce RAND3[24]. Plaintext (512): entry_id RAND2 [16] ‖ kind 1 [1] ‖ at_ms 1,795,341,630,000 [8] ‖ based_on_seq 200 [8] ‖ C200 [32] ‖ payload_len 77 [4] ‖ payload [77] ‖ padding `0x00` [366]. Frame `F201` (670):
    | field | width | value |
    |---|---|---|
    | magic | 4 | `"TDF1"` |
    | board_id | 16 | BID |
    | dev | 16 | DESK |
    | ctr | 8 | 122 |
    | len | 4 | 558 |
    | ver / suite / epoch | 1 / 1 / 4 | 1 / 1 / 1 |
    | nonce | 24 | RAND3 |
    | ciphertext | 528 | `AEAD(K1; RAND3; ctx("entry") ‖ F201[0..48] ‖ 0x01 ‖ 0x01 ‖ u32 1; plaintext)` |
    | sig | 64 | `SIG(DESK signing key; "taskdeck/v1/frame\0"; F201[0..606])` |

    `Persist(Outbox[Put{…, ctr 122, frame F201, frame_hash FH201, epoch 1}])` (D2).
19. **Desk → Relay**: socket `0x02 ‖ u32 r ‖ "TDA1" ‖ u8 1 ‖ u32 670 ‖ F201` (684 bytes).
20. **Relay**: P1–P7; T1; T2 (122 = restored `last_ctr` 121 + 1); T4; T5: seq 201, `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ FH201)`; DESK row `last_ctr 122, last_seq 201`. Result `0x82 ‖ u32 r ‖ "TDR1" ‖ 1 ‖ 0x00 ‖ u64 201 ‖ C201`; push of 201 to Desk's socket (762 bytes) (relay.md §9.3, §9.4, §11.4).
21. **Desk**: `ack = (201, C201)`; the push: classify with `M_200` (own frame, `own_sealed(122) = FH201`) → `Accept`; `Persist(AppendLog[(201, C201, τ201, F201)], SetMarker{201, 201, attempt})`; apply `create_task` (item and marks raised to 41, stamps (201, 1)); echo check equal; `seen = 201`, `Outbox[Drop(RAND2)]`; `B = 201` (engine.md §10.4, §11.5, §12.3, §12.4).
22. **Mac** (`T0 + 60 s`): `Connect` after its backoff; socket upgrade with `T_MAC` (gen 1); hello `0x01 ‖ SESS_M ‖ u32 0`.
23. **Relay → Mac**: welcome: `0x81 ‖ u64 201 ‖ C201 ‖ u64 last_ctr 60 ‖ u64 last_seq s_M60 ‖ i64 relay_now ‖ u64 1 ‖ u64 1 ‖ 1 ‖ 0x01 ‖ u32 appends_left ‖ DOOR ‖ 0` (93 bytes).
24. **Mac**: checks: head 201 > L 200 (no `relay_behind`, no chain to compare); no acks; reconcile `a = r = 60` → normal; `Read{after 200, limit 256}` → session `Reconciling` (engine.md §11.1–§11.3, §12.2). Socket `0x03 ‖ u32 r ‖ u64 200 ‖ u16 256` (15 bytes).
25. **Relay → Mac**: `0x83 ‖ u32 r ‖ "TDP1" ‖ u64 201 ‖ C201 ‖ i64 relay_now ‖ u64 1 ‖ u64 1 ‖ u8 flags 0 ‖ u16 1 ‖ u64 201 ‖ i64 at ‖ u32 670 ‖ F201` (766 bytes) (relay.md §10.5).
26. **Mac**: page steps 1–5 pass; `C201 = chain_step(C200, 201, F201)`; classify with `M_200`: DESK current, sig, ctr 122 = 121 + 1, epoch 1, opens, `based_on` (200, C200) matches → `Accept`; head chain matches; `Persist(AppendLog[201], SetMarker)`; apply `create_task` "Pay rent"; `B = 201`; run ended: rebase; session `Live` (engine.md §12.3).
    AMBIGUOUS: `restore_tail` brings back log rows only; whether the blob `SNAP192` (announced in 193–200) still exists on the restored relay, and who re-uploads it, is not covered (relay.md §12.9, engine.md §21.3).

**State at the end.**

| | Desk | Mac | Phone (offline) |
|---|---|---|---|
| L = K = B | 201 | 201 | 200 |
| chain | C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ FH201) | C201 | C200 |
| counters (own last_ctr in M) | DESK 122 | DESK 122, MAC 60 | DESK 121, PHONE 18 |
| epoch | 1 | 1 | 1 |
| outbox (live) | empty (Put, Put with frame, Put with ack, Drop) | empty | empty |
| log files | + record 201 | + record 201 | unchanged |
| meta | alarm A1 `RelayBehind` resolved; no stop | unchanged | unchanged |
| checkpoint | unchanged until its cadence | same | unchanged |

Relay: log 1–201 (191–200 restored at their original seqs with the same chain values), head (201, C201); DESK `last_ctr 122, last_seq 201`; MAC `last_ctr 60, last_seq s_M60`; PHONE `last_ctr 18, last_seq s_P18`; no role, state, key or floor changed; nonce RAND1 deleted.

What the users saw: Desk: at `T0` a stopped sync line and the evidence (relay at 190, counter 115; this device at 200) with the repair "restore tail" running, cleared within moments; "Pay rent" appeared at once and synced. Mac: nothing unusual; "Pay rent" appeared after its reconnect. Phone: nothing yet.
