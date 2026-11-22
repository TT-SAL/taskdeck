# Group `edit`, reader B

Notation for both scenarios. `ctx(x)` = ASCII `taskdeck/v1/x` ‖ `0x00` (seal.md §2, README §5.2); its width is 13 + len(x): `ctx("frame")`, `ctx("entry")`, `ctx("chain")`, `ctx("token")`, `ctx("state")` 18 bytes, `ctx("file")` 17, `ctx("key-entry")` 22. `D` = 2^40 = 1,099,511,627,776. `R_1` = the epoch-1 root every device holds. `K1` = `K_entry,1` = `HKDF(salt = BID, ikm = R_1, info = ctx("key-entry") ‖ u32 1, L = 32)` (seal.md §6.5, SEAL-KEY-012; info 26 bytes). `Kf(k)` = `K_file(k)` of the device's own `L_d` = `HKDF(salt = BID, ikm = L_d, info = ctx("file-key") ‖ u8 k ‖ dev[16], L = 32)` (seal.md §6.6). A sealed-file record is `u32 ct_len ‖ nonce[24] ‖ AEAD(Kf(k); nonce; ctx("file") ‖ header[55] ‖ u64 i; plaintext)` with `ct_len` = plaintext + 16 (seal.md §19.1, §19.3); `i` = number of records already in the file (DEV-FILE-003). Record JSON of `OutboxRecord` is the canonical encoding of the mapping of device.md §6.5 (`Option` None absent, `Vec<u8>` base64, `[u8; N]` hex, unit variants snake_case). Hex values below are written `hex(X)` when X is symbolic. RAND numbers are per scenario, in draw order on each device.

---

## Scenario 1 — First edit

1. **User** (Desk, at `T0` = 1,795,341,600,000 ms): types "Pay rent" in quick-add, no due date, and confirms. The desktop calls `Device::submit(Intent::QuickAdd { name: "Pay rent", deadline: None })` (device.md §17.4; core.md §12.3 row `quick_add`).
   AMBIGUOUS: whether the desktop's quick-add uses `submit` (`hold = false`) or `submit_named` (`hold = true`, naming hold of engine.md §9.3); device.md §17.4 says `submit_named` is "for a create the user is about to name", and quick-add carries its name already. Taken: `submit`, `hold = false`. With `hold = true` nothing would be sealed for up to 30,000 ms of `mono_ms` (ENG-HOLD-001).
2. **Desk** (facade, sync thread, one loop turn with no other engine input, DEV-FACADE-001, engine.md §9.1): `Engine::device_no()` = 1, so it compiles. `compile(QuickAdd, CompileCtx { view: Engine::view() (= confirmed state at B = 200, outbox empty), zone: ChronoZone(Local) = Europe/Helsinki, now: T0, device_no: 1, overlay }, minter)` (core.md §13.1):
   1. G1 trim: "Pay rent" unchanged; G2: 8 scalars ≤ 200, not empty (CORE-COMP-001, §13.3);
   2. `deadline` none (G3 not applied, no zone used);
   3. cap: live items in view < 10,000 (`core/cap_items` not raised);
   4. mint `id`: `n = max(minter.next_item = 41, mark(view, 1).item + 1 = 41) = 41`; `id = 1 × 2^40 + 41 = 1,099,511,627,817`; `minter.next_item = 42` (core.md §7.2, CORE-ID-004, G7 after every check, CORE-COMP-004);
   5. command: `create_task { id: 1099511627817, name: "Pay rent", time_importance: 1 }` (undated → `time_importance = 1`); reply `{ id: 1099511627817 }`.
3. **Desk** (facade): `Enqueue { ticket: t1, command, at_ms: 1795341600000, origin_state: OS1, hold: false }` (device.md §17.4, §8.3, DEV-RUN-004). `OS1` (18 bytes):

   | Field | Width | Value |
   |---|---|---|
   | `device_no` | 2 | `0001` |
   | `next_item` | 8 | `000000000000002a` (42) |
   | `next_block` | 8 | `NB_D` (Desk's block minter; untouched) |

4. **Desk** (engine, §9.1 step 1): not refused: joined; not revoked; clock not blocked (skew of the last measurement within 3,600,000, ENG-CLOCK-002); `|at_ms − wall_ms| ≤ 60,000`. Step 2: no held record. Step 3 trial apply on a copy of the pending view with `Ctx { seq: B + |U| + 1 = 201, device_no: 1, at_ms: T0, based_on: 200 }` (ENG-ENQ-002): `apply` §8.4 (core.md): (1) ctx: device_no 1 in 1..8191, seq 201 in 2..2^53−1, based_on 200 < 201 (CORE-CTX-001); (2) `create_task` in the op table of rules 1 (CORE-RULES-001); (3) §9.4 checks: mint check of `id` (high part 1 = `ctx.device_no`, low 41 > mark 40, CORE-ID-002), no `sid`, cap; (4) effects. Succeeds.
5. **Desk** (engine, §9.1 step 4): `Sealer::new_entry_id()` → **RAND1[16]** = `E1`, the entry id (ENG-ENQ-003, SEAL-PLAIN-002). Builds the record and emits `Persist { id: p1, ops: [Outbox([Put(r1)])] }`; applies the command to the pending view (provisional seq 201), `view_version += 1` (ENG-PEND-003). Record JSON `J1` (engine.md §7.4, §37 `OutboxRecord`; device.md §6.5), key order as canonical encoding (core.md §4.2):

   ```
   {"at_ms":1795341600000,"based_on":{"chain":"hex(C200)","seq":200},"body":{"cmd":"eyJpZCI6MTA5OTUxMTYyNzgxNywibmFtZSI6IlBheSByZW50Iiwib3AiOiJjcmVhdGVfdGFzayIsInRpbWVfaW1wb3J0YW5jZSI6MX0="},"entry_id":"hex(E1)","hold":false,"kind":"cmd","order":ORD_D,"origin_state":"base64(OS1)","reseals":0,"via":"append"}
   ```

   `ORD_D` = one more than the largest `order` Desk ever used (world does not fix it). The `cmd` body is base64 of the 77-byte payload `PAY` of step 9.
6. **Desk** (same `handle` call, engine §4.5 step 6, send loop §10.2): session `Live`, no stop, not fenced, not revoked, no pause, no outstanding `Append`. `R = [r1]`; no resends; epoch gate open (§24.2: no own frame under an older epoch); `r1` has no frame and is not held: `ctr = next_ctr = 1 + max(a = 121, h = 121, w = 121) = 122` (§10.3, ENG-SEND-006; `a` = Desk's `last_ctr` in `M_200`); `Sealer::seal(M_200, 122, PlainEntry { E1, T0, (200, C200), Body::Cmd(PAY) })` (engine.md §6.1).
   AMBIGUOUS: whether the send loop may seal a record in the same `handle` call as its `Enqueue`, before the `Persisted` of its first `Put` (D1 of §7.12 gates only `Notify(Enqueued)`; D2 gates only the `Append`). Taken: yes, the loop runs at step 6 of every call (ENG-MODEL-005); the two `Persist` effects are serial (ENG-PERSIST-001).
7. **Desk** (sealer, seal.md §10, §9, §8): plaintext `PT1` (SEAL-PLAIN-001, SEAL-PAD-001), `U = 69 + 77 = 146`, `P(146) = 512` (§10.2 rule 1):

   | Offset | Field | Width | Value |
   |---|---|---|---|
   | 0 | `entry_id` | 16 | `E1` |
   | 16 | `kind` | 1 | `01` (cmd) |
   | 17 | `at_ms` | 8 | `000001a202b2c500` (1,795,341,600,000) |
   | 25 | `based_on_seq` | 8 | `00000000000000c8` (200) |
   | 33 | `based_on_chain` | 32 | `C200` |
   | 65 | `payload_len` | 4 | `0000004d` (77) |
   | 69 | `payload` | 77 | `PAY` = `{"id":1099511627817,"name":"Pay rent","op":"create_task","time_importance":1}` |
   | 146 | `padding` | 366 | `00` × 366 |
   | | total | 512 | |

8. **Desk** (sealer): draws **RAND2[24]** = `N1`, the envelope's AEAD nonce (SEAL-GEN-012). Header `H1` (48 bytes) = `"TDF1"` ‖ `BID` ‖ `DESK` ‖ `000000000000007a` (ctr 122) ‖ `0000022e` (len 558). `AD1` = `ctx("entry")`[18] ‖ `H1`[48] ‖ `01` ‖ `01` ‖ `00000001` = 72 bytes (SEAL-ENV-002). Ciphertext = `AEAD(K1; N1; AD1; PT1)`, 528 bytes. Signature `S1 = SIG(DESK_SIGN; "taskdeck/v1/frame\0"; H1 ‖ body)`, signed input 18 + 606 = 624 bytes (§8.3, SEAL-FRAME-002). Frame `F` (670 bytes, §8.1, §9.1, SEAL-FRAME-001, SEAL-ENV-001):

   | Offset | Field | Width | Value |
   |---|---|---|---|
   | 0 | `magic` | 4 | `"TDF1"` |
   | 4 | `board_id` | 16 | `BID` |
   | 20 | `dev` | 16 | `DESK` |
   | 36 | `ctr` | 8 | 122 |
   | 44 | `len` | 4 | 558 |
   | 48 | `ver` | 1 | 1 |
   | 49 | `suite` | 1 | 1 |
   | 50 | `epoch` | 4 | 1 |
   | 54 | `nonce` | 24 | `N1` |
   | 78 | `ciphertext` | 528 | `AEAD(K1; N1; AD1; PT1)` (512 + 16 tag) |
   | 606 | `sig` | 64 | `S1` |
   | | total | 670 | |

   `FH201 = H(F)` (670-byte input, §8.2). The sealer returns `Sealed { frame: F, frame_hash: FH201, epoch: 1, keys_changed: false }`.
9. **Desk** (engine): sets `r1.ctr = 122`, `frame = F`, `frame_hash = FH201`, `epoch = 1`; emits `Persist { id: p2, ops: [Outbox([Put(r1')])] }` (§10.2 step 4, ENG-SEND-003). `J1'` = `J1` with four members added, in key order:

   ```
   {"at_ms":1795341600000,"based_on":{"chain":"hex(C200)","seq":200},"body":{"cmd":"…as J1…"},"ctr":122,"entry_id":"hex(E1)","epoch":1,"frame":"base64(F)","frame_hash":"hex(FH201)","hold":false,"kind":"cmd","order":ORD_D,"origin_state":"base64(OS1)","reseals":0,"via":"append"}
   ```

10. **Desk** (store, p1; device.md §7.3, §7.2 append procedure): appends one sealed record to `outbox.tdo` (kind 5), record index `i_o`; nonce **RAND3[24]**; plaintext (device.md §6.4.4):

    | Field | Width | Value |
    |---|---|---|
    | `n` | 2 | 1 |
    | op tag | 1 | `01` (Put) |
    | length | 4 | byte length of `J1` |
    | `J1` | len(J1) | above |

    then `sync(outbox.tdo)`; `Persisted { p1 }` (DEV-FILE-004).
11. **Desk** (engine, on `Persisted p1`): `Notify(Enqueued { ticket: t1, entry_id: Some(E1), provisional_seq: Some(201) })` (§9.1 step 5, ENG-ENQ-004). **Desk** (facade): `submit` returns `Submitted { reply: { id: 1099511627817 }, entry_id: Some(E1), provisional_seq: Some(201), view_version }`; `Changes.view` raised (device.md §8.4, §17.3).
12. **User** (Desk): sees "Pay rent" in the board (pending view); sync line "· 1 waiting" (`queued = 1`, `oldest_queued_ms = T0`, engine.md §30.1).
13. **Desk** (store, p2): appends to `outbox.tdo` the record `i_o + 1`, nonce **RAND4[24]**, plaintext `0001 ‖ 01 ‖ u32 len(J1') ‖ J1'`; `sync`; `Persisted { p2 }`.
14. **Desk** (engine, on `Persisted p2`): `Append { req: q1, frames: [F] }` (§10.2 step 5, D2).
15. **Desk → Relay** (socket, transport device.md §10.5): message `A1` (684 bytes; relay.md §11.3, §9.2):

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `02` |
    | `req` | 4 | `r1` (transport's socket request id, mapped to `q1`) |
    | `magic` | 4 | `"TDA1"` |
    | `n` | 1 | 1 |
    | `len_0` | 4 | `0000029e` (670) |
    | `frame_0` | 670 | `F` |

16. **Relay** (object): message ≤ 4,194,378 bytes (RELAY-SOCK-009); type `0x02` known; attachment `ready = true`; re-reads Desk's `devices` row: `state = 1`, `gen = 1` = attachment's `gen` (RELAY-SOCK-007). Board state `open` (RELAY-OBJ-001). Batch layout: magic `"TDA1"`, `n = 1` in 1..16, length prefix within the message, no trailing bytes (RELAY-APPEND-001). Reads `now()` once: `NOW1` (RELAY-OBJ-005).
17. **Relay** (§9.3, before the transaction, RELAY-SEQ-006), frame 0: P1 `len_0` 670 in 670..262,144, `"TDF1"`, `len` 558 = 670 − 112; P2 `board_id` = `BID`; P3 `ctr` 122 in 1..2^53−1; P4 `devices` row for `DESK` exists; P5 `DESK` = the socket token's `dev`; P6 `state = 1`; P7 `verifyStrict(DESK_SIGN, ctx("frame") ‖ F[0..606], F[606..670])`, the four steps of §3.4 (RELAY-GEN-009). All pass.
18. **Relay** (§9.4, one transaction, RELAY-SEQ-007): reads head `(200, C200)`; T1 re-read `DESK` row: `state = 1`, same `sign_pk`; T2 `122 = last_ctr 121 + 1`; T4 `count = app_count` if `app_day` = 20,779 (UTC day of `NOW1`) else 0, `count < 3,000`; T5: `seq = 201`; `at = AT201 = max(NOW1, at of row 200)` (RELAY-STORE-004); `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ FH201)` (90-byte input; RELAY-STORE-003, SEAL-CHAIN-001); writes, then commits:

    | Table | Row |
    |---|---|
    | `log` (+ index `log_dev_ctr`) | `seq 201, dev DESK, ctr 122, hash FH201, chain C201, at AT201, frame F` |
    | `devices` | `DESK`: `last_ctr 122, last_seq 201, app_day 20779, app_count count + 1`; other columns unchanged |

    3 rows written (relay.md §7.1). Head becomes `(201, C201)`.
19. **Relay → Desk** (socket, after commit, RELAY-OBJ-007, RELAY-SOCK-006): `result` (51 bytes; §11.3, §9.6):

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `82` |
    | `req` | 4 | `r1` |
    | `magic` | 4 | `"TDR1"` |
    | `n` | 1 | 1 |
    | status | 1 | `00` (new) |
    | `seq` | 8 | `00000000000000c9` (201) |
    | `chain` | 32 | `C201` |

20. **Relay → Desk, Relay → Mac** (push to every socket with `ready = true`, the author's included; RELAY-APPEND-002, RELAY-SOCK-008; Phone has no socket): `push` `P201` (762 bytes; §11.3, §10.5):

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `84` |
    | `magic` | 4 | `"TDP1"` |
    | `head_seq` | 8 | 201 |
    | `head_chain` | 32 | `C201` |
    | `relay_now` | 8 | `NOW1` |
    | `floor` | 8 | 1 (`max(compact_floor 1, purge_floor 1)`; no compaction or purge in the world) |
    | `purge_floor` | 8 | 1 |
    | `flags` | 1 | `00` (`more` 0, `truncated` 0, `fenced` 0) |
    | `n` | 2 | 1 |
    | entry `seq` | 8 | 201 |
    | entry `at` | 8 | `AT201` |
    | entry `flen` | 4 | 670 |
    | entry `frame` | 670 | `F` |

    AMBIGUOUS: the order of `result` and `push` on the author's socket (relay.md §10.6 "after the transaction commits, every stored entry is pushed"; §11.4 "answered by `result`"). Taken: `result` first, then `push`.
21. **Desk** (transport → engine): `AppendResults { req: q1, results: [New { seq: 201, chain: C201 }] }`. **Desk** (engine §10.4): `r1.ack = (201, C201)`; the `Put` goes in the next `Persist`; `201 > L = 200`, so no echo check yet (ENG-RESULT-001).
22. **Desk** (transport → engine): `Page { req: None, page }` from `P201`; its first entry is `L + 1 = 201`, so it is processed as a page (§12.2). §12.3: (1) 1 entry ≤ 256, frame 670 in range, total ≤ 1,048,576; (2) `skew = wall_ms − NOW1` recorded (§27.1); (3) `head.seq 201 ≥ L 200`, no chain held at 201; no record has `ack.seq > 201` (§11.2 steps 2, 3); (4) `truncated` 0; (5) contiguous from `L + 1`; (6) dry run: `C201' = chain_step(C200, 201, F)`; `classify(seq 201, F, prev_chain C200, M_200, chain_at, retained_from, own_sealed(122) = FH201)` → the 17 steps of seal.md §14.2 (as Mac's, step 26), with step 6: `dev = DESK` is own and `own_sealed` holds `FH201` for ctr 122, so `own_dev_foreign` is not set (SEAL-RCPT-010) → `Accept`; (7) last entry is `head.seq`: `C201' = head_chain C201`; (8) `Persist { id: p3, ops: [AppendLog([LogRecord { seq 201, chain C201, tau τ201, frame F }]), SetMarker({ first 201, last 201, attempt a_D }), Outbox([Put(r1'')])] }` where `τ201 = max(τ200, T0) = T0` and `r1''` = `r1'` with `"ack":[201,"hex(C201)"]` as its first member (ENG-RECV-002).
    AMBIGUOUS: where "the next Persist" puts the acknowledgement `Put` among the page's ops (engine.md §10.4 versus §12.3 step 8, which lists only `AppendLog`, `SetMarker` and `WriteIdentity`). Taken: in `p3`, after `SetMarker`.
23. **Desk** (store, p3): appends to the current log segment (kind 1, no new segment: it holds < 16,384 entries; DEV-STORE-003) one record, nonce **RAND5[24]**, plaintext (device.md §6.4.1) of 724 bytes:

    | Field | Width | Value |
    |---|---|---|
    | `n` | 2 | 1 |
    | `seq` | 8 | 201 |
    | `chain` | 32 | `C201` |
    | `tau` | 8 | `000001a202b2c500` (T0) |
    | `flen` | 4 | 670 |
    | `frame` | 670 | `F` |

    `sync`; no history record (not `ctl`, DEV-RUN-003). Replaces `marker.tdk` (kind 10): new header with `file_id` **RAND6[16]**, one record with nonce **RAND7[24]**, plaintext `u64 201 ‖ u64 201 ‖ u64 a_D` (24 bytes); `write_new(.tmp)`, `sync`, `rename`, `sync_dir` (device.md §7.2 replace, §6.4.6). Appends to `outbox.tdo` record `i_o + 2`, nonce **RAND8[24]**, plaintext `0001 ‖ 01 ‖ vb32(J1'')`. `Persisted { p3 }`.
24. **Desk** (engine, on `Persisted p3`, §12.3 step 9): `L = K = 201`, `M_K = M_201` (Desk `last_ctr = 122`, `clock_ms = T0`; §13.5 `Accept` row). Board applies 201 (§12.4, §13.1 `Accept, cmd`): `SM::parse(PAY)` (core.md §9.1 steps 1–7: 77 ≤ 257,979; JSON; no duplicate key; `op` = `create_task`; schema; canonical; no cross-field rule) → `apply(board, create_task, Ctx { seq 201, device_no 1, at_ms T0, based_on 200 })` (ENG-SM-001), checks as step 4 → inserts the item, raises `marks[1].item` to 41; no notices. Own entry: `r1.seen = 201`; echo check: log frame at 201 has hash `FH201` = `r1.frame_hash`, `C201` = `ack.chain` (ENG-ECHO-001) → no `unechoed`. `B = 201` (not aligned). Emits `Persist { id: p4, ops: [Outbox([Drop(E1)])] }` (§12.4 step 3, ENG-STATE-001). Run ended (`more` 0, `L` = head): rebase (§8.4 case 1); pending view = confirmed; `Notify(ViewChanged)`, `Notify(StatusChanged)`. Item now in `BoardData.items` (core.md §6.2):

    ```
    {"created":"2026-11-22T10:00:00.000000000Z","id":1099511627817,"kind":"task","name":"Pay rent","stamps":{"estimate":[201,1],"levels":[201,1],"name":[201,1],"repeat":[201,1],"time":[201,1]},"time_importance":1}
    ```

    AMBIGUOUS: when `ClearMarker` is emitted after a page applied cleanly (engine.md §7.11 lists it, §15 never says when). Taken: never here; `marker.tdk` keeps `{201, 201, a_D}` until the next `SetMarker`, and at the next `Start` it is not above the checkpoint only once a checkpoint reaches 201.
25. **Desk** (store, p4): appends to `outbox.tdo` record `i_o + 3`, nonce **RAND9[24]**, plaintext `0001 ‖ 02 ‖ E1` (19 bytes); `sync`; `Persisted { p4 }`. **User** (Desk): sync line shows "synced" at `wall_ms` (`last_synced_ms`), `queued = 0`.
26. **Mac** (transport → engine): `Page { req: None }` from `P201`; `L = 200`, first entry `L + 1`. §12.3, in order: (1) limits as step 22; (2) `relay_now` measurement; (3) `head.seq 201 ≥ L`, no chain held at 201, no acks above 201; (4) `truncated` 0; (5) contiguous; (6) `C201 = chain_step(C200, 201, F)` (SEAL-CHAIN-002), then `classify` with `M_200` (ENG-SEALER-001), seal.md §14.2:
    1. total 670 in 670..262,144, magic `"TDF1"`, `len` 558 = 670 − 112 (SEAL-FRAME-003);
    2. `board_id = BID`;
    3. `DESK` is a current device of `M_200` (§13.4);
    4. `S1` verifies strictly under `DESK_SIGN` over `ctx("frame") ‖ F[0..606]` (SEAL-GEN-010);
    5. `ctr 122 = last_ctr 121 + 1`;
    6. own-dev: `DESK ≠ MAC`, no flag;
    7. `DESK` current;
    8. `(1, 1)` allowed by rules 1 (§14.3);
    9. epoch 1 = `E` = `M_200.epoch`;
    10. AEAD opens under `K1` with `AD1` rebuilt from the frame's header (§9.2);
    11. unpadding §10.3: `L = 512 ≥ 69`; `U = 146 ≤ 512`; `P(146) = 512 = L`; bytes 146..512 all `00`; kind 1, `at_ms` in range;
    12. `based_on`: `b = 200 < 201`, `b ≥ retained_from`, `C200` equals Mac's `C200` (§14.8);
    13. `cmd` by a current device under the current epoch (§11.1);
    14. not under `E − 1`;
    15. `cmd`: no payload parse here (SEAL-KIND-002);
    16. not `ctl`;
    17. `Accept`, no halt.
    (7) `C201 = head_chain`; (8) `Persist { id: m1, ops: [AppendLog([{201, C201, T0, F}]), SetMarker({201, 201, a_M})] }`.
27. **Mac** (store, m1): log segment record, nonce **RAND10[24]**, the 724-byte plaintext of step 23; `marker.tdk` replaced, `file_id` **RAND11[16]**, record nonce **RAND12[24]**, plaintext `u64 201 ‖ u64 201 ‖ u64 a_M`. `Persisted { m1 }`.
28. **Mac** (engine, on `Persisted m1`): `L = K = 201`, `M_201` (Desk `last_ctr` 122, `clock_ms` T0). Board: `parse` (core.md §9.1, seven steps, as step 24) → `apply` with `Ctx { seq 201, device_no 1 (Desk's in M_200), at_ms T0, based_on 200 }`: §8.4 (1) ctx valid, (2) op in rules-1 table, (3) mint check of `id` (high 1 = device_no, 41 > mark 40), no sids, cap < 10,000 items and blocks ≤ 50,000; (4) inserts the item of step 24 byte for byte, `marks[1].item = 41`. No notice (nothing concerns Mac), no outbox record of Mac's. `B = 201`. Rebase; `Notify(ViewChanged)`.
29. **User** (Mac): "Pay rent" appears in Mac's board; nothing else changes.

Both sockets also carry WebSocket protocol pings every 30,000 ms (device.md §10.10, relay.md §11.5); they reach no engine.

**State after step 29.**

| Holder | State |
|---|---|
| Desk | `L = K = B = 201`, `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ FH201)`; `M_201`: epoch 1, `epoch_seq` 1, rules 1, `clock_ms` T0, `last_ctr` Desk 122, Mac 60, Phone 18, recovery 1; outbox empty (`outbox.tdo` holds 4 more ops: Put `J1`, Put `J1'`, Put `J1''`, Drop `E1`); log segment +1 record (201); `marker.tdk` `{201, 201, a_D}`; `notices.tdk`, `meta.tdk`, `identity.tdk`, checkpoint unchanged (a checkpoint follows by §18.1 case 2 when 600,000 ms have passed since the last one); minter `next_item` 42; board holds "Pay rent", `marks[1].item = 41` |
| Mac | `L = K = B = 201`, `C201`, `M_201` as Desk's; outbox empty; log +1 record; `marker.tdk` `{201, 201, a_M}`; board equal to Desk's (same canonical bytes) |
| Phone | unchanged: `L = B = 200`, app closed |
| Relay | head `(201, C201)`; `log` row 201; `DESK` row `last_ctr 122, last_seq 201, app_day 20779, app_count +1`; no other row |
| User | Desk: "Pay rent" at once, "1 waiting" then "synced"; Mac: "Pay rent" appears |

---

## Scenario 2 — Offline edit replayed

World additions taken from the scenario: seq 201 = Desk's frame `F201` (ctr 122, `at_ms` = T0 − 90 min = 1,795,336,200,000, based_on `(200, C200)`, payload `create_task {id 1099511627817, "Pay rent", time_importance 1}`, 670 bytes); seq 202 = Mac's frame `F202` (ctr 61, `at_ms` = T0 − 30 min = 1,795,339,800,000, based_on `(201, C201)`, payload `{"id":1099511627783,"name":"Call the plumber","op":"rename"}`, 60 bytes, `U = 129`, 670 bytes). Desk and Mac received both by push and applied them as in scenario 1; the rename stamped `name = (202, 2)` with no notice (the old name stamp `(s, 1)` has `s ≤ 200 < 201`). "Call plumber" = `1 × 2^40 + 7 = 1,099,511,627,783`.

AMBIGUOUS (world): how the Phone app ran offline. Taken: the user opened the app without network for each edit (activity visible; each `Connect` ended `Disconnected { unreachable: true }`, device.md §10.6 row 1), and at `T0` a new process opens the board (`Device::open`, `Start`).

### At `T0 − 2 h` (1,795,334,400,000): "done"

1. **User** (Phone): taps done on "Call plumber". **Phone** (page → bridge): `POST /api/command`, body `{"op":"complete","id":1099511627783,"at":"2026-11-22T10:00"}` (bridge.md §6.3 row `complete`, `{op, id, at}`); `op` is one of the 17 verbs; the body is passed unchanged to `submit_json` (BRIDGE-VERB-002).
   AMBIGUOUS: what `at` today's page puts in `complete` (bridge.md §6.3 "today's command JSON", core.md §12.2 step 4 accepts exact, naive or none). Taken: the phone's local wall time to the minute, naive; Europe/Helsinki is UTC+2 on this date, so it resolves to `2026-11-22T08:00:00.000000000Z`, the same instant as `now`. An exact RFC 3339 value or an absent `at` gives the same instant here.
2. **Phone** (facade, sync thread): `parse_intent` (core.md §12.2: ≤ 84,096 bytes; object with `op` `complete`; `id` u64; `at` → `WireTime::Naive(2026-11-22T10:00)`). `compile` against `Engine::view()` (= confirmed at 200), `now = 1,795,334,400,000`, `device_no = 3`, zone a `TableZone` for Europe/Helsinki (device.md §16 / bridge.md §12.7): missing? no (live); kind task (not `core/finish_kind`); `at` G3: `resolve_on_day(2026-11-22, 600, zone) = 08:00:00Z`; G5 in range; archive rows < 200,000; `at' = min(at, now + 1 day) = at`. Command `complete { id: 1099511627783, at: 2026-11-22T08:00:00.000000000Z }`, payload `PC` (74 bytes) = `{"at":"2026-11-22T08:00:00.000000000Z","id":1099511627783,"op":"complete"}`. Mints nothing; minter unchanged.
3. **Phone** (facade): `Enqueue { ticket, command, at_ms: 1795334400000, origin_state: OSa = 0003 ‖ u64 6 ‖ u64 NB_P, hold: false }` (DEV-RUN-004; Phone minted up to n = 5, so `next_item` = 6).
4. **Phone** (engine §9.1): step 1 not refused (no valid skew measurement or an old one: no block, §27.1); trial apply with `Ctx { seq 201, device_no 3, at_ms T0−2h, based_on 200 }`: §9.8 checks (live item; task; archive cap) pass; the item's latest stamp `(s, 1)`, `s ≤ 200`, is not another device's change since 200 → no notice. Step 4: **RAND1[16]** = `E2` (entry id). `Persist { pa, [Outbox([Put(ra)])] }`; pending view: "Call plumber" archived as finished, `archived_at` 08:00Z; `view_version += 1`. `Ja`:

   ```
   {"at_ms":1795334400000,"based_on":{"chain":"hex(C200)","seq":200},"body":{"cmd":"eyJhdCI6IjIwMjYtMTEtMjJUMDg6MDA6MDAuMDAwMDAwMDAwWiIsImlkIjoxMDk5NTExNjI3NzgzLCJvcCI6ImNvbXBsZXRlIn0="},"entry_id":"hex(E2)","hold":false,"kind":"cmd","order":ORD_P,"origin_state":"base64(OSa)","reseals":0,"via":"append"}
   ```

5. **Phone** (engine): send loop does nothing: session not `Live` (ENG-SESS-001).
6. **Phone** (store, pa): appends to `outbox.tdo` (kind 5) one record, nonce **RAND2[24]**, plaintext `0001 ‖ 01 ‖ vb32(Ja)`; `sync`; `Persisted`. **Phone** (engine): `Notify(Enqueued { entry_id: E2, provisional_seq: 201 })`; the bridge answers `{"ok":true,"version":N}`.
7. **User** (Phone): the task shows as done; sync line "relay unreachable since …· 1 waiting" (`unreachable_since_ms`, `queued = 1`).

### At `T0 − 1 h` (1,795,338,000,000): "Buy stamps"

8. **Phone** (page → bridge): `POST /api/command`, body `{"op":"quick_add","name":"Buy stamps","deadline":""}` (bridge.md §6.3 `{op, name, deadline}`; an empty `deadline` is none, core.md §12.2 step 4) → `submit_json`.
9. **Phone** (facade): `compile` on the pending view (confirmed 200 + `ra`): name "Buy stamps"; no deadline; cap; mint `n = max(6, mark(3).item + 1 = 6) = 6`, `id = 3 × 2^40 + 6 = 3,298,534,883,334`, `next_item = 7`. Command `create_task { id: 3298534883334, name: "Buy stamps", time_importance: 1 }`, payload `PB` (79 bytes) = `{"id":3298534883334,"name":"Buy stamps","op":"create_task","time_importance":1}`. `Enqueue { at_ms: 1795338000000, origin_state: OSb = 0003 ‖ u64 7 ‖ u64 NB_P, hold: false }`.
10. **Phone** (engine): trial apply `Ctx { seq 202, device_no 3, at_ms T0−1h, based_on 200 }` (mint check 6 > mark 5; cap) passes; **RAND3[16]** = `E3`; `Persist { pb, [Outbox([Put(rb)])] }` with `Jb` = `{"at_ms":1795338000000,"based_on":{"chain":"hex(C200)","seq":200},"body":{"cmd":"eyJpZCI6MzI5ODUzNDg4MzMzNCwibmFtZSI6IkJ1eSBzdGFtcHMiLCJvcCI6ImNyZWF0ZV90YXNrIiwidGltZV9pbXBvcnRhbmNlIjoxfQ=="},"entry_id":"hex(E3)","hold":false,"kind":"cmd","order":ORD_P+1,"origin_state":"base64(OSb)","reseals":0,"via":"append"}`.
11. **Phone** (store, pb): `outbox.tdo` record, nonce **RAND4[24]**, plaintext `0001 ‖ 01 ‖ vb32(Jb)`; `sync`; `Persisted`; `Notify(Enqueued { entry_id: E3, provisional_seq: 202 })`; reply `{"ok":true,"version":N+1}` (the page's `quick_add` uses no reply field).
12. **User** (Phone): "Buy stamps" shown; "· 2 waiting".

### At `T0`: the app opens with a network

13. **Phone** (device `open`, device.md §7.4): reads the identity directory; draws the session id **RAND5[16]** = `SID` from `OsEntropy` (§10.4, DEV-NET-006); `Loaded { checkpoint, tail, outbox: [Put(ra), Put(rb)], meta, …, session: SID }`; foreground false (DEV-NET-017).
14. **Phone** (engine, `Start`, §7.10, §15.2): rebuilds confirmed state at `B = 200`, `M_200`; outbox `[ra, rb]` (ENG-STATE-003); raises `meta.attempt` to `a_P`, stores the old session as `prev_session`: `Persist { [WriteMeta(meta)] }` (D5) → `meta.tdk` replaced, `file_id` **RAND6[16]**, record nonce **RAND7[24]**. Pending view: `ra` with `Ctx { 201, 3, T0−2h, 200 }`, `rb` with `Ctx { 202, 3, T0−1h, 200 }` (§8.2). Session `Offline` → emits `Connect { relay: (relay.example, door, 1) }`.
15. **Phone** (shell): `onStart` → `set_foreground(true)` → `notify_resume`; the held `Connect` is let through with no backoff (first after `open`, device.md §10.7, §10.9).
16. **User** (Phone, before): sees the board at 200 with its own two edits: "Call plumber" done (in the archive, 10:00), "Buy stamps" live, no "Pay rent"; sync line "connecting · 2 waiting" (`queued 2`, `oldest_queued_ms` T0−2h).
17. **Phone → Relay** (transport, `Connect`): the stored token has `gen = 1` and a stored `push = true`, the engine is not stopped `Revoked`: no login, no `caps` (device.md §10.3, §10.5). Socket upgrade `GET wss://relay.example/v1/b/hex(BID)/socket` with `Host: relay.example`, `User-Agent: TaskDeck-Sync/1`, `Authorization: Bearer TD1.<92 base64url chars>` and the WebSocket upgrade headers (DEV-NET-004). The token's 69 bytes (opaque to Phone; relay.md §5.1): `01 ‖ BID ‖ PHONE ‖ u32 1 ‖ MAC(TOKEN_KEY; ctx("token") ‖ token[0..37])`.
18. **Relay** (gate §6.2): 0 configuration; 1 `GET /v1/b/{board}/socket`; 2 no query; 3 no body; 4 length 0; 5 `Upgrade: websocket`; 6 `Bearer` form; 7 96 characters, `TD1.`, decodes to 69 bytes, role 1, `board_id = BID`, MAC verified in constant time (RELAY-TOKEN-002); 10 object call. **Relay** (object §11.1): `devices` row `PHONE` role 1, `state 1`, `gen 1` = token's (§8.2); board `open`; `acceptWebSocket(server, ["hex(PHONE)"])`; attachment `{dev: hex(PHONE), gen: 1, features: 0, ready: false}`; `101`.
19. **Phone → Relay**: hello (21 bytes): `01 ‖ SID ‖ 00000000` (relay.md §11.3).
20. **Relay** (object): attachment has no session, length 21; no other socket tagged `PHONE` has a session; stores `session = SID`, `features = 0`, `ready = true`; reads `now()` = `NOW2`.
21. **Relay → Phone**: welcome (93 bytes):

    | Field | Width | Value |
    |---|---|---|
    | `type` | 1 | `81` |
    | `head_seq` | 8 | 202 |
    | `head_chain` | 32 | `C202` |
    | `last_ctr` | 8 | 18 |
    | `last_seq` | 8 | `Q18` (the seq of Phone's ctr 18, ≤ 200) |
    | `relay_now` | 8 | `NOW2` |
    | `floor` | 8 | 1 |
    | `purge_floor` | 8 | 1 |
    | `state` | 1 | 1 |
    | `caps` | 1 | `01` |
    | `appends_left` | 4 | `3000 − app_count` (3,000 when Phone's `app_day` ≠ 20,779) |
    | `door` | 5 | `DOOR` |
    | `has_fence` | 1 | 0 |

22. **Phone** (transport → engine): `Connected { push: true }` (no effect, §11.1), then `Welcome { head (202, C202), last_ctr Some(18), last_seq Some(Q18), relay_now NOW2, floor 1, purge_floor 1, fenced false, fence None, appends_left, push true }`.
23. **Phone** (engine §11.2): (1) `skew = wall_ms − NOW2`, `|skew| ≤ 300,000` → no warning; (2) `head.seq 202 ≥ L 200`, no chain held at 202 to compare; (3) no record has an ack; (4) not fenced; (5) reconcile §11.3 with `a = 18` (`M_200`), `r = 18`, `q = Q18`: no record has a frame.
    AMBIGUOUS: engine.md §11.3 defines `h` as "the largest counter of a record with a frame" and says nothing for an outbox with no sealed record; read literally (`h` absent or 0), `r = 18 > h` would start the restored-copy path of step 3. Taken: `h = a = 18` when no record holds a frame, so case 2 (`a ≤ r ≤ h`): normal; `Q18 ≤ L`, so nothing to read before sealing; `w = 18` (§10.3). State `Reconciling`; `head.seq > L` → `Read { req: q2, after: 200, limit: 256 }` (§12.2).
24. **Phone → Relay**: read (15 bytes): `03 ‖ u32 r2 ‖ u64 200 ‖ u16 256`.
25. **Relay** (object §11.4): re-reads `PHONE`'s row: current, `gen 1` (RELAY-SOCK-007); read §10.5: rows `seq > 200`, `seq ≥ purge_floor 1`, ≤ 256, frames ≤ 1,048,576 → 201, 202; writes nothing (RELAY-READ-003).
26. **Relay → Phone**: page message (1,456 bytes) = `83 ‖ u32 r2 ‖ "TDP1" ‖ u64 202 ‖ C202 ‖ i64 NOW2' ‖ u64 1 ‖ u64 1 ‖ 00 ‖ u16 2 ‖ (u64 201 ‖ i64 AT201 ‖ u32 670 ‖ F201) ‖ (u64 202 ‖ i64 AT202 ‖ u32 670 ‖ F202)`.
27. **Phone** (engine §12.3): (1) 2 entries, frames 670, total 1,340; (2) measurement; (3) head 202 ≥ L; (4) `truncated` 0 (`201 ≥ floor 1`); (5) first seq 201 = `L + 1`, 202 next; (6) dry run:
    - `C201 = chain_step(C200, 201, F201)`; `classify(201, F201, C200, M_200)`: the 17 steps of seal.md §14.2 as scenario 1 step 26 (author `DESK` current, strict signature under `DESK_SIGN`, ctr 122 = 121 + 1, not own dev, (1,1), epoch 1, AEAD under `K1`, unpadding (`U = 146`), `based_on` (200, `C200`) matches, cmd by a current device) → `Accept`, `M_201`;
    - `C202 = chain_step(C201, 202, F202)`; `classify(202, F202, C201, M_201)`: author `MAC` current, strict signature under `MAC_SIGN`, ctr 61 = 60 + 1, `based_on` (201, `C201`) equal to the `C201` just computed, unpadding (`U = 129`) → `Accept`, `M_202`.
    AMBIGUOUS: whether `chain_at` in `ClassifyInput` (engine.md §6.1, ENG-SEALER-001) answers for a seq computed earlier in the same dry run but not yet persisted (here `C201` for entry 202's `based_on`). Taken: yes; otherwise 202 would read as `based_on_unchecked` or a fork.
    (7) last entry 202 = `head.seq`, `C202` = `head_chain`; (8) `Persist { pc, [AppendLog([{201, C201, τ201 = T0−90min, F201}, {202, C202, τ202 = T0−30min, F202}]), SetMarker({201, 202, a_P})] }`.
28. **Phone** (store, pc): log segment record, nonce **RAND8[24]**, plaintext `0002 ‖ (u64 201 ‖ C201 ‖ i64 1795336200000 ‖ u32 670 ‖ F201) ‖ (u64 202 ‖ C202 ‖ i64 1795339800000 ‖ u32 670 ‖ F202)` (1,446 bytes); `marker.tdk` replaced, `file_id` **RAND9[16]**, nonce **RAND10[24]**, plaintext `u64 201 ‖ u64 202 ‖ u64 a_P`. `Persisted`.
29. **Phone** (engine, commit §12.3 step 9, §12.4): `L = K = 202`, `M_202`. Applies 201: `create_task` "Pay rent" with `Ctx { 201, 1, T0−90min, 200 }` (no notice). Applies 202: `rename` with `Ctx { 202, 2, T0−30min, 201 }`: item live; `name` "Call the plumber", stamp `name = (202, 2)`; old stamp `(s, 1)`, `s ≤ 201` → no notice. Neither is Phone's. `B = 202`. Run ended → rebase (§8.4 case 1): `ra` with `Ctx { seq 203, device_no 3, at_ms T0−2h, based_on min(200, 202) = 200 }`: the item's latest stamp `(202, 2)` is another device's change since 200 (`202 > 200`, `2 ≠ 3`, `2 ≠ 0`; core.md §10.1) → applies with an overwrite notice (group `item`), shown with the record, never recorded (ENG-SM-004); the row is built from the renamed item; `rb` with `Ctx { 204, 3, T0−1h, 200 }`: applies. `view_version += 1`. `L = 202 =` relay head and the reconcile is complete → `Live` (§11.1).
30. **User** (Phone, after the catch-up): "Pay rent" appears; the done row now reads "Call the plumber" (finished 10:00 local), with the pending overwrite mark; "Buy stamps" live; "· 2 waiting".
31. **Phone** (engine, same call, §4.5 step 5 duties): no beat enqueued.
    AMBIGUOUS (world): Phone's `last_beat_ms` is not given. If `wall_ms − last_beat_ms ≥ 86,400,000` (§23.1), a beat record is enqueued here, after `rb` in `order`, and sealed with ctr 21 into the same batch. Taken: no beat due.
32. **Phone** (engine, step 6, send loop §10.2): `R = [ra, rb]`; no resends; epoch gate open; seals in `order` (ENG-SEND-004): `ra` with `ctr = 1 + max(18, 18, 18) = 19`, `rb` with ctr 20; `Sealer::seal(M_202, 19, {E2, T0−2h, (200, C200), Cmd(PC)})` and `seal(M_202, 20, {E3, T0−1h, (200, C200), Cmd(PB)})`, both under epoch 1 (SEAL-ENV-003), `based_on` unchanged from enqueue (ENG-ENQ-005, SEAL-PLAIN-003). Nonces **RAND11[24]** = `N19`, **RAND12[24]** = `N20`.
    - Plaintext `PT19` (512 bytes): `E2 ‖ 01 ‖ 000001a20244e800 ‖ 00000000000000c8 ‖ C200 ‖ 0000004a ‖ PC[74] ‖ 00 × 369` (`U = 143`, `P = 512`).
    - Plaintext `PT20` (512 bytes): `E3 ‖ 01 ‖ 000001a2027bd680 ‖ 00000000000000c8 ‖ C200 ‖ 0000004f ‖ PB[79] ‖ 00 × 364` (`U = 148`).
    - Frame `F19` (670 bytes): header `H19 = "TDF1" ‖ BID ‖ PHONE ‖ 0000000000000013 ‖ 0000022e`; body `01 ‖ 01 ‖ 00000001 ‖ N19 ‖ AEAD(K1; N19; ctx("entry") ‖ H19 ‖ 01 ‖ 01 ‖ 00000001; PT19)` (528); `sig = SIG(PHONE_SIGN; "taskdeck/v1/frame\0"; F19[0..606])`. `FH203 = H(F19)`.
    - Frame `F20` (670 bytes): header `H20 = "TDF1" ‖ BID ‖ PHONE ‖ 0000000000000014 ‖ 0000022e`; body `01 ‖ 01 ‖ 00000001 ‖ N20 ‖ AEAD(K1; N20; ctx("entry") ‖ H20 ‖ 01 ‖ 01 ‖ 00000001; PT20)`; `sig = SIG(PHONE_SIGN; "taskdeck/v1/frame\0"; F20[0..606])`. `FH204 = H(F20)`.
    One `Persist { pd, [Outbox([Put(ra'), Put(rb')])] }` (§10.2 step 4); `ra'` = `Ja` plus `"ctr":19`, `"epoch":1`, `"frame":"base64(F19)"`, `"frame_hash":"hex(FH203)"` in key order; `rb'` likewise with 20, `F20`, `FH204`.
33. **Phone** (store, pd): one `outbox.tdo` record, nonce **RAND13[24]**, plaintext `0002 ‖ 01 ‖ vb32(Ja') ‖ 01 ‖ vb32(Jb')`; `sync`; `Persisted`. **Phone** (engine): `Append { req: q3, frames: [F19, F20] }` (one batch: 2 ≤ 16 frames, batch of 5 + 2 × 674 = 1,353 ≤ 4,194,373 bytes, ENG-SEND-001).
34. **Phone → Relay**: append message (1,358 bytes): `02 ‖ u32 r3 ‖ "TDA1" ‖ 02 ‖ 0000029e ‖ F19 ‖ 0000029e ‖ F20`.
35. **Relay** (object): size, type; re-read `PHONE` row (current, gen 1); board `open`; batch layout. `now()` = `NOW3`. P1–P7 for `F19`, then for `F20` (author `PHONE` = token's dev; `verifyStrict(PHONE_SIGN, …)` each). Transaction, head `(202, C202)`: `F19`: T1; T2 `19 = 18 + 1`; T4 `count < 3,000`; T5 `seq 203`, `at AT203 = max(NOW3, AT202)`, `C203 = H(ctx("chain") ‖ C202 ‖ u64 203 ‖ FH203)` → `new`. `F20`: T1; T2 `20 = 19 + 1` (the batch's running value); T4 `count + 1 < 3,000`; T5 `seq 204`, `C204 = H(ctx("chain") ‖ C203 ‖ u64 204 ‖ FH204)` → `new`. Writes `log` rows 203 and 204 (with index rows) and the `PHONE` row once (`last_ctr 20, last_seq 204, app_day 20779, app_count count + 2`): 5 rows. Commit.
36. **Relay → Phone**: result (92 bytes) `82 ‖ u32 r3 ‖ "TDR1" ‖ 02 ‖ 00 ‖ u64 203 ‖ C203 ‖ 00 ‖ u64 204 ‖ C204`.
37. **Relay → Phone, Desk, Mac** (push, every ready socket): `84 ‖ "TDP1" ‖ u64 204 ‖ C204 ‖ i64 NOW3 ‖ u64 1 ‖ u64 1 ‖ 00 ‖ u16 2 ‖ (u64 203 ‖ i64 AT203 ‖ u32 670 ‖ F19) ‖ (u64 204 ‖ i64 AT204 ‖ u32 670 ‖ F20)` (1,452 bytes). AMBIGUOUS: order of `result` and `push` on Phone's socket, as scenario 1 step 20; taken `result` first.
38. **Phone** (engine §10.4): `ra.ack = (203, C203)`, `rb.ack = (204, C204)`; Puts in the next `Persist`; both above `L`, echo later.
39. **Phone** (engine §12.3, push as page, first entry 203 = `L + 1`): limits; measurement; head 204 ≥ L; contiguous; dry run: `C203 = chain_step(C202, 203, F19)`, `classify` with `M_202`: author `PHONE` current; signature; ctr 19 = 18 + 1; own dev with `own_sealed(19) = FH203` → no `own_dev_foreign`; `based_on` 200 with `C200` held (≥ `retained_from`) → match; unpadding `U = 143` → `Accept`; `C204`, `classify` with `M_203`: ctr 20 = 19 + 1, `own_sealed(20) = FH204`, `U = 148` → `Accept`. Head chain `C204` matches. `Persist { pe, [AppendLog([{203, C203, τ = T0−30min, F19}, {204, C204, T0−30min, F20}]), SetMarker({203, 204, a_P}), Outbox([Put(ra''), Put(rb'')])] }` (`τ203 = τ204 = max(T0−30min, at_ms) = T0−30min`; `ra''`, `rb''` carry `"ack":[203,"hex(C203)"]` and `"ack":[204,"hex(C204)"]`). Store: log nonce **RAND14[24]**; marker `file_id` **RAND15[16]**, nonce **RAND16[24]**; outbox nonce **RAND17[24]**.
40. **Phone** (engine, commit): `M_204` (Phone `last_ctr` 20; `clock_ms` stays T0−30min). Applies 203: `parse(PC)`; `apply(complete, Ctx { 203, 3, T0−2h, 200 })`: item live (§9.8 step 1); kind task; archive < 200,000. Effects: `archived_at = min(08:00:00Z, ctx.at + 1 day) = 2026-11-22T08:00:00.000000000Z`; row copied from the item as it now stands (name "Call the plumber"; `created`, levels, blocks as held; outcome `finished`, omitted as default); row inserted at its archive-order position; item removed; overwrite notice (core.md §10.2):

    ```
    {"author":3,"group":"item","item":1099511627783,"kind":"overwrite","other":2,"other_seq":202,"seq":203}
    ```

    (103 bytes; core.md §10.7). It concerns Phone (`author` 3) → recorded (§13.4, ENG-REF-003). Own entry: `ra.seen = 203`; echo check `FH203`, `C203` (ENG-ECHO-001); `Drop(E2)`. Applies 204: `create_task` "Buy stamps" with `Ctx { 204, 3, T0−1h, 200 }`: mint check (high 3, 6 > mark 5); item inserted with `created = 2026-11-22T09:00:00.000000000Z`, stamps `[204,3]`; `marks[3].item = 6`. `rb.seen = 204`; echo check; `Drop(E3)`. `B = 204`. `Persist { pf, [Notices([Add { kind: Notice, seq: 203, record }]), Outbox([Drop(E2), Drop(E3)])] }` (D3). Rebase: pending = confirmed.
41. **Phone** (store, pf): `notices.tdk` (kind 8) one record, nonce **RAND18[24]**, plaintext (device.md §6.4.5) 119 bytes:

    | Field | Width | Value |
    |---|---|---|
    | `n` | 2 | 1 |
    | op | 1 | `01` (Add) |
    | kind | 1 | `01` (notice) |
    | `seq` | 8 | 203 |
    | length | 4 | 103 |
    | record | 103 | the notice above |

    `outbox.tdo` record, nonce **RAND19[24]**, plaintext `0002 ‖ 02 ‖ E2 ‖ 02 ‖ E3`. `Persisted` → `Notify(NoticesAdded { notices: 1, refusals: 0 })` (ENG-REF-002), `ViewChanged`, `StatusChanged`.
42. **User** (Phone, after): "Pay rent" and "Buy stamps" live; "Call the plumber" in the archive as done; "synced", nothing waiting; one unread notice: Phone's "done" overwrote Mac's change at seq 202.
43. **Mac** (on the push, engine §12.3): checks as scenario 1 step 26 for 203 and 204 with `M_202`, `M_203` (author `PHONE` current; strict signature under `PHONE_SIGN`; ctr 19 = 18 + 1, then 20; not own dev; epoch 1; AEAD; unpadding; `based_on` 200 with Mac's `C200`); `Persist { AppendLog([203, 204]), SetMarker({203, 204, a_M}) }` (log nonce, marker `file_id` and nonce: Mac's RAND1[24], RAND2[16], RAND3[24]); applies 203 with `Ctx { 203, 3, T0−2h, 200 }` → the same row and the same notice (CORE-CONF-007); it concerns Mac (`other` 2) → `Persist { Notices([Add { Notice, 203, record }]) }` (Mac's RAND4[24]); applies 204. `B = 204`. **User** (Mac): "Call the plumber" leaves the live board into the archive as done; a notice "changed on Phone since you last synced"; "Buy stamps" appears.
44. **Desk** (on the push): the same checks and applies; the notice does not concern Desk (`author` 3, `other` 2) → no record. `Persist { AppendLog([203, 204]), SetMarker({203, 204, a_D}) }` only. **User** (Desk): the task leaves the board into the archive as done; "Buy stamps" appears; no notice.

No refusal arises on any device (no `seal/` or `core/` refusal: both of Phone's entries are accepted and applied); nothing is recorded at the relay beyond the rows of step 35.

**State after step 44.**

| Holder | State |
|---|---|
| Phone | `L = K = B = 204`, `C204 = H(ctx("chain") ‖ C203 ‖ u64 204 ‖ FH204)`, `C203 = H(ctx("chain") ‖ C202 ‖ u64 203 ‖ FH203)`; `M_204`: epoch 1, rules 1, `clock_ms` T0−30min (1,795,339,800,000), `last_ctr` Desk 122, Mac 61, Phone 20, recovery 1; registry `last_entry` for Phone `(T0−1h, 204, 200)`; outbox empty (`outbox.tdo`: Put `Ja`, Put `Jb`, one record with Put `Ja'` + Put `Jb'`, one with Put `Ja''` + Put `Jb''`, one with Drop `E2` + Drop `E3`); log +2 records (201–202, 203–204); `marker.tdk` `{203, 204, a_P}`; `meta.tdk` with `attempt a_P`, `prev_session`; `notices.tdk` +1 notice (seq 203); minter `next_item` 7; board: "Pay rent" (1·2^40+41), "Buy stamps" (3·2^40+6), archive row "Call the plumber" finished at 08:00Z, `marks[1].item 41`, `marks[3].item 6` |
| Mac | `L = B = 204`, `C204`, `M_204` as Phone's; outbox empty; `notices.tdk` +1 notice (seq 203); board equal to Phone's |
| Desk | `L = B = 204`, `C204`, `M_204`; outbox empty; no new notice; board equal |
| Relay | head `(204, C204)`; `log` rows 201–204; `PHONE` row `last_ctr 20, last_seq 204, app_count +2`; `DESK` 122/201; `MAC` 61/202 |
| User | Phone: before, its two edits on the 200 board; after, the merged board, "synced", one notice. Mac: the notice. Desk: no notice |
