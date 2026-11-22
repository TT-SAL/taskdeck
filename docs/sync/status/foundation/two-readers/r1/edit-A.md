# edit-A

Notation: `ctx(x)` is the ASCII `taskdeck/v1/x` and one `0x00` byte, written in full as `"taskdeck/v1/x\0"` (README §5.2, seal.md §4.1). Integers are big-endian (README §5.1). `K_entry,1 = HKDF(salt = BID; ikm = R_1; info = "taskdeck/v1/key-entry\0" ‖ u32 1; L = 32)` (seal.md §6.5). `K_file(k, d) = HKDF(salt = BID; ikm = L_d of device d; info = "taskdeck/v1/file-key\0" ‖ u8 k ‖ d[16]; L = 32)` (seal.md §6.6). Item id of "Call plumber": `1 × 2^40 + 7 = 1,099,511,627,783`. `T0 = 1,795,341,600,000` ms = 2026-11-22T12:00 Europe/Helsinki (UTC+2).

Layout that every sealed local-file record below uses (seal.md §19.1, §19.3; device.md §6.1, §7.2):

| Field | Width | Value |
|---|---|---|
| `ct_len` | 4 | plaintext length + 16 |
| `nonce` | 24 | the RAND named at the step |
| `ciphertext` | `ct_len` | `AEAD(K_file(kind, dev); nonce; "taskdeck/v1/file\0" ‖ header[55] ‖ u64 i; plaintext)` |

File header (55 bytes, already on disk for every append file named below; written anew with a fresh `file_id` by each replace): `"TDL1"`[4] ‖ `ver` 1 [1] ‖ `suite` 1 [1] ‖ `kind` [1] ‖ `dev` [16] ‖ `BID` [16] ‖ `file_id` [16]. `i` = number of records already in the file (DEV-FILE-003).

## Scenario 1: first edit

1. **User** (Desk), at T0: quick-adds "Pay rent" with no due date. The desktop calls `Device::submit(Intent::QuickAdd { name: "Pay rent", deadline: None })`; `submit`, not `submit_named`, because the create is already named, so `hold = false` (device.md §17.4; core.md §12.3 row `quick_add`).
2. **Desk** (sync thread, call queue): `Engine::device_no()` = 1; compiles against `Engine::view()` (the pending view = the confirmed state at `B = 200`, outbox empty) with `CompileCtx { zone: ChronoZone(Local) = Europe/Helsinki, now = T0, device_no: 1, overlay }` (DEV-FACADE-001, core.md §13.1).
3. **Desk** compile, row `quick_add` (core.md §13.4, §13.3 G1, G2, G3, G5, G7; CORE-COMP-001): trim gives "Pay rent" (8 scalars, not empty); `deadline` absent → none (core.md §12.2 step 4); live items in view below 10,000; mint `n = max(minter.next_item = 41, mark(1).item + 1 = 41) = 41`, id `= 1 × 2^40 + 41 = 1,099,511,627,817`, `minter.next_item := 42` (core.md §7.2, CORE-ID-004). Undated → `time_importance = 1`. Reply `{ id: 1099511627817 }`. Command payload (core.md §4.2, §9.4; `blocks` at its default `[]` is omitted, CORE-ENC-002):

   | Field | Width | Value |
   |---|---|---|
   | `cmd` payload | 77 | `{"id":1099511627817,"name":"Pay rent","op":"create_task","time_importance":1}` |

4. **Desk**: in the same loop turn sends `Enqueue { ticket: t1, command, at_ms: 1,795,341,600,000, origin_state, hold: false }` (DEV-FACADE-001, DEV-RUN-004). `origin_state` (device.md §8.3):

   | Field | Width | Value |
   |---|---|---|
   | `device_no` | 2 | 1 |
   | `next_item` | 8 | 42 |
   | `next_block` | 8 | `NB_D` (Desk's block minter; not fixed by the world) |

5. **Desk** engine, §9.1 step 1 (ENG-ENQ-006): joined (device_no 1); not revoked; composing not clock-blocked (skew far below 3,600,000 ms, §27.2); `|at_ms − wall_ms| ≤ 60,000` → no refusal. Step 2: no held last record.
6. **Desk** engine, §9.1 step 3: trial apply on a copy of the pending view under `catch_unwind`, `Ctx { seq: 201 (= B + |U| + 1 = 200 + 0 + 1), device_no: 1, at_ms: T0, based_on: 200 }`; core.md §8.4: ctx valid (CORE-CTX-001); `create_task` in the rules-1 op table; checks of §9.4: mint check of `id` (high part 1 = `device_no`, `n = 41 >` mark 40, CORE-ID-002), no sids, cap → success, no notices.
7. **Desk** engine, §9.1 step 4: `Sealer::new_entry_id()` draws **RAND1[16]** = `E1` (the entry id, ENG-ENQ-003). Record `r1`; emits `Persist P1 = [Outbox[Put(r1)]]`; puts the trial result into the pending view (item 1,099,511,627,817, kind task, name "Pay rent", `time_importance` 1, `created` `2026-11-22T10:00:00.000000000Z`, every stamp `(201, 1)`; `marks[1].item = 41`); `view_version += 1`. `r1` as record JSON (device.md §6.5, canonical order; `None` fields absent; `o_D` = one more than the largest `order` Desk ever used, not fixed by the world, `d(o_D)` its digit count):

   | Field | Value |
   |---|---|
   | JSON (393 + d(o_D) bytes) | `{"at_ms":1795341600000,"based_on":{"chain":"<hex C200>","seq":200},"body":{"cmd":"<base64 of the 77-byte payload, 104 chars>"},"entry_id":"<hex E1>","hold":false,"kind":"cmd","order":o_D,"origin_state":"<base64 of the 18 bytes, 24 chars>","reseals":0,"via":"append"}` |

8. **Desk** engine, same `handle`, §4.5 step 6, send loop (§10.1, §10.2): session `Live`, no stop, not fenced, not revoked, no pause, no `Append` outstanding → `R = [r1]`; `via = Append`; no resends; epoch gate open (§24.2); `r1` not held → `ctr = next_ctr = 1 + max(a = 121 (DESK.last_ctr in M_200), h = none, w = 121 (relay last_ctr verified at the last reconcile)) = 122` (§10.3, ENG-SEND-006); calls `Sealer::seal(M_K = M_200, 122, entry)`.
9. **Desk** sealer, plaintext (seal.md §10.1, §10.2: `U = 69 + 77 = 146`, `P(146) = 512`):

   | Offset | Field | Width | Value |
   |---|---|---|---|
   | 0 | `entry_id` | 16 | `E1` |
   | 16 | `kind` | 1 | 1 (`cmd`) |
   | 17 | `at_ms` | 8 | 1,795,341,600,000 |
   | 25 | `based_on_seq` | 8 | 200 |
   | 33 | `based_on_chain` | 32 | `C200` |
   | 65 | `payload_len` | 4 | 77 |
   | 69 | `payload` | 77 | step 3 |
   | 146 | `padding` | 366 | `0x00` |
   | | total | 512 | |

10. **Desk** sealer: draws **RAND2[24]** = `N1`, the frame nonce (SEAL-GEN-012). AD (seal.md §9.2, SEAL-ENV-002), 72 bytes:

    | Field | Width | Value |
    |---|---|---|
    | context | 18 | `"taskdeck/v1/entry\0"` |
    | frame header | 48 | `"TDF1"` ‖ `BID` ‖ `DESK` ‖ u64 122 ‖ u32 558 |
    | `ver` | 1 | 1 |
    | `suite` | 1 | 1 |
    | `epoch` | 4 | 1 |

    `ciphertext = AEAD(K_entry,1; N1; AD; plaintext)`, 528 bytes.
11. **Desk** sealer, frame `F201` (seal.md §8.1, §9.1, §8.3):

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
    | 78 | `ciphertext` | 528 | step 10 |
    | 606 | `sig` | 64 | `SIG(DESK signing key; "taskdeck/v1/frame\0"; F201[0..606])` (signed bytes 18 + 606 = 624) |
    | | total | 670 | |

    `frame_hash = H(F201)` =: `FH201` (seal.md §8.2).
12. **Desk** engine: sets `r1.ctr = 122`, `frame = F201`, `frame_hash = FH201`, `epoch = 1`; emits `Persist P2 = [Outbox[Put(r1')]]`, no `WriteIdentity` (no secret changed) (§10.2 step 4, D2); then `ScheduleTick`, `Notify(ViewChanged)`, `Notify(StatusChanged)` (§4.5 steps 8, 9). `r1'` JSON (1,400 + d(o_D) bytes): step 7's members plus, in canonical place, `"ctr":122` after `body`, `"epoch":1` after `entry_id`, `"frame":"<base64 of F201, 896 chars>","frame_hash":"<hex FH201>"` after `epoch`.
13. **Desk** runtime: performs `P1` (device.md §7.3, append procedure §7.2) on `outbox.tdo` (kind 5): one record at index `i_o`, nonce **RAND3[24]**, plaintext `u16 1 ‖ 0x01 ‖ vb32(r1 JSON)` (400 + d bytes); `append`, `sync`; `Persisted(P1)` on the local queue.
14. **Desk** runtime: performs `P2`: record `i_o + 1`, nonce **RAND4[24]**, plaintext `u16 1 ‖ 0x01 ‖ vb32(r1' JSON)` (1,407 + d bytes); `append`, `sync`; `Persisted(P2)`.
15. **Desk** engine, on `Persisted(P1)`: `Notify(Enqueued { t1, E1, provisional_seq: 201 })` (§9.1 step 5, ENG-ENQ-004, D1); `submit` returns `Submitted { reply { id: 1099511627817 }, entry_id: E1, provisional_seq: 201, view_version }` (device.md §17.4).
16. **Desk** UI shows the user "Pay rent" on the board and the sync line "· 1 waiting" (`status.queued = 1`, not doomed) (engine.md §30.1, §30.2).
17. **Desk** engine, on `Persisted(P2)`: emits `Append { req: a1, frames: [F201] }` (§10.2 step 5, ENG-SEND-003). Net thread writes on Desk's socket (device.md §10.5; relay.md §11.3, §9.2):

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `0x02` |
    | `req` | 4 | `r_a1` (transport counter) |
    | `magic` | 4 | `"TDA1"` |
    | `n` | 1 | 1 |
    | `len_0` | 4 | 670 |
    | `frame_0` | 670 | `F201` |
    | | total | 684 |

18. **Relay** (object, woken): reads its clock once, `RN1` (RELAY-OBJ-005); message ≤ 4,194,378 bytes, type `0x02`, batch layout of §9.2 exact (RELAY-APPEND-001).
19. **Relay**: re-reads DESK's `devices` row: `role 1`, `state 1`, `gen 1` = the attachment's `gen` (RELAY-SOCK-007); board `open` (§8.1).
20. **Relay**, phase 1 and 2 (§8.4, §9.3): P1 `len_0` 670 in range, `"TDF1"`, `len` 558 = 670 − 112; P2 `board_id = BID`; P3 `ctr` 122 in 1..2^53 − 1; P4 author row exists; P5 `dev` = the socket token's `DESK`; P6 `state = 1`; P7 `verifyStrict(DESK_SIGN, "taskdeck/v1/frame\0" ‖ F201[0..606], F201[606..670])`, the four steps of §3.4 → true (RELAY-GEN-009).
21. **Relay**, one transaction (§9.4, RELAY-SEQ-007): head `(200, C200)`; T1 row still `state 1`, same `sign_pk`; T2 `122 = 121 + 1`; T4 `count` (DESK's `app_count` if `app_day = 20,779`, else 0) `< 3,000`; T5 `seq = 201`, `at = max(RN1, at of seq 200)`, `hash = FH201`, `C201 = H("taskdeck/v1/chain\0" ‖ C200 ‖ u64 201 ‖ FH201)` (90-byte input, RELAY-STORE-003); writes the `log` row (and its `log_dev_ctr` entry) and DESK's row `last_ctr 122, last_seq 201, app_day 20,779, app_count count + 1`; commits.
22. **Relay** → Desk, result (§11.3, §9.6):

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `0x82` |
    | `req` | 4 | `r_a1` |
    | `magic` | 4 | `"TDR1"` |
    | `n` | 1 | 1 |
    | `status` | 1 | 0 (`new`) |
    | `seq` | 8 | 201 |
    | `chain` | 32 | `C201` |
    | | total | 51 |

23. **Relay** → every ready socket (Desk's and Mac's; Phone has none), push (RELAY-SOCK-008, RELAY-APPEND-002, §10.5):

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `0x84` |
    | `magic` | 4 | `"TDP1"` |
    | `head_seq` | 8 | 201 |
    | `head_chain` | 32 | `C201` |
    | `relay_now` | 8 | `RN1` |
    | `floor` | 8 | 1 |
    | `purge_floor` | 8 | 1 |
    | `flags` | 1 | 0 |
    | `n` | 2 | 1 |
    | entry `seq` | 8 | 201 |
    | entry `at` | 8 | `at201` |
    | entry `flen` | 4 | 670 |
    | entry `frame` | 670 | `F201` |
    | | total | 762 |

    AMBIGUOUS: the order in which Desk's socket receives the `result` (§11.4 "answered by `result`") and the `push` (§11.4 "after any transaction that stored new rows") is not fixed by relay.md §11.4 or §10.6; I take `result` first.
24. **Desk** engine, `AppendResults { a1, [new(201, C201)] }` (§10.4): `r1.ack = (201, C201)`, its `Put` goes into the next `Persist`; 201 > `L` = 200, so no echo check yet; `status.queued = 0`.
25. **Desk** engine, `Page { req: None }` (§12.2: first entry `L + 1`), §12.3: step 1 one entry, 670 bytes; step 2 `relay_now` → skew (§27.1); step 3 head 201 ≥ `L`, no `ack` above 201 (§11.2 steps 2, 3); step 4 not truncated; step 5 first seq 201 = `L + 1`; step 6 `C201 = chain_step(C200, 201, F201)` and `classify { seq 201, prev_chain C200, membership M_200, retained_from ≤ 200, own_sealed(122) = FH201 }`, seal.md §14.2: (1) 670 bytes, `"TDF1"`, `len` 558 = 670 − 112; (2) `BID`; (3) DESK current in `M_200`; (4) strict signature under `DESK_SIGN`; (5) `122 = 121 + 1`; (6) own dev, `own_sealed(122) = H(frame)` → `own_dev_foreign` not set (SEAL-RCPT-010); (7) DESK current; (8) (1, 1) allowed by rules 1; (9) epoch 1 = `E`; (10) opens under `K_entry,1` with the AD of step 10; (11) unpadding §10.3: 512 ≥ 69, `U = 146 ≤ 512`, `P(146) = 512`, bytes 146..512 zero, kind 1, `at_ms` in range; (12) `b = 200 < 201`, `C200` held and equal; (13) `cmd` by a current device in the current epoch; (14), (15), (16) not applicable; (17) `Accept`, no halt. `M_201` = `M_200` with `DESK.last_ctr = 122`, `clock_ms = max(τ200, T0) = T0` (seal.md §13.5). Step 7 `head.chain = C201`.
26. **Desk** engine, §12.3 step 8: `Persist P3 = [AppendLog[{seq 201, chain C201, tau T0, frame F201}], SetMarker{201, 201, A_D}, Outbox[Put(r1'')]]` (`A_D` = Desk's current attempt). Runtime (device.md §7.3): log segment `log-00000000000000000001.tdl` (kind 1) record, nonce **RAND5[24]**, plaintext 724 bytes:

    | Field | Width | Value |
    |---|---|---|
    | `n` | 2 | 1 |
    | `seq` | 8 | 201 |
    | `chain` | 32 | `C201` |
    | `tau` | 8 | 1,795,341,600,000 |
    | `flen` | 4 | 670 |
    | `frame` | 670 | `F201` |

    no history record (not `ctl`, DEV-RUN-003); `marker.tdk` (kind 10) replaced: new header with `file_id` **RAND6[16]**, one record with nonce **RAND7[24]**, plaintext `u64 201 ‖ u64 201 ‖ u64 A_D` (24 bytes; file 55 + 68 = 123 bytes); `outbox.tdo` record `i_o + 2`, nonce **RAND8[24]**, plaintext `u16 1 ‖ 0x01 ‖ vb32(r1'' JSON)`, `r1''` = `r1'` with `"ack":[201,"<hex C201>"]` first (1,479 + d bytes).

    AMBIGUOUS: engine.md §10.4 says the `ack` `Put` goes "in the next `Persist`" but not where among that `Persist`'s ops; I put it after `AppendLog` and `SetMarker`.
27. **Desk** engine, on `Persisted(P3)` (§12.3 step 9, §12.4): `L = K = 201`, `M_K = M_201`; seq 201 not in `skips`; `Accept` `cmd`: `parse` (core.md §9.1 steps 1 to 7) → `apply` under `Ctx { 201, device_no 1, at T0, based_on 200 }` → success, no notices; author is Desk and `E1` is `r1`'s → `r1.seen = 201`, dropped; `B = 201` (not aligned); `registry.last_entry[DESK] = (T0, 201, 200)`; echo check: log frame at 201 has `FH201`, `C201` equals `r1.ack` (ENG-ECHO-001). §12.3 step 10: `more` unset, `L = head` → rebase: `U` empty, pending view = confirmed state, `view_version += 1`. Emits `Persist P4 = [Outbox[Drop(E1)]]` (record `i_o + 3`, nonce **RAND9[24]**, plaintext `u16 1 ‖ 0x02 ‖ E1`, 19 bytes), `Notify(ViewChanged)`, `Notify(StatusChanged)`; `last_synced_ms = wall_ms`.

    AMBIGUOUS: no clause says when `ClearMarker` is emitted after a page has applied cleanly (engine.md §7.11, §15); I leave `marker.tdk` holding `{201, 201, A_D}`.
28. **Mac** engine, `Page { req: None }` from step 23, §12.3 steps 1 to 7 exactly as step 25, except: step 6 `own_sealed` has no entry for DESK, check (6) does nothing (frame not under Mac's dev); classification `Accept`, `M_201` as on Desk.
29. **Mac** engine: `Persist PM1 = [AppendLog[{201, C201, T0, F201}], SetMarker{201, 201, A_M}]`; runtime: log record (layout of step 26) in Mac's current segment, nonce **RAND10[24]**; `marker.tdk` replaced, `file_id` **RAND11[16]**, nonce **RAND12[24]**.
30. **Mac** engine, on `Persisted(PM1)`: `L = K = 201`; `apply` under `Ctx { 201, device_no 1 (DESK in M_200), at T0, based_on 200 }`: mint check high part 1 = `ctx.device_no`, 41 > mark 40; inserts the item, `marks[1].item = 41`; no notices; author not Mac, outbox untouched; `B = 201`; `registry.last_entry[DESK] = (T0, 201, 200)`; run ended → rebase (`U` empty), `view_version += 1`; `Notify(ViewChanged)`, `Notify(StatusChanged)`.
31. **Mac** UI shows the user "Pay rent".

Not fixed by the world: whether `B` changing makes the 600,000 ms checkpoint of engine.md §18.1 case 2 due on Desk or Mac at §4.5 step 7.

**End state.**

| | Desk | Mac | Phone | Relay |
|---|---|---|---|---|
| head | `(201, C201)`, `B = K = L = 201` | `(201, C201)`, `B = K = L = 201` | `(200, C200)` (closed) | head `(201, C201)` |
| counters | DESK 122, MAC 60, PHONE 18, `dev_R` 1 in `M_201` | same | `M_200`: DESK 121 | rows: DESK `last_ctr 122`, `last_seq 201`; others unchanged |
| epoch, rules | 1, 1 | 1, 1 | 1, 1 | `read_epoch` 1 |
| `clock_ms` | `T0` | `T0` | `τ200` | |
| outbox | empty; `outbox.tdo` +4 records (Put, Put+frame, Put+ack, Drop) | empty, unchanged | empty | |
| other files | log +1 record (seq 201); `marker.tdk {201, 201, A_D}`; no notice records | log +1 record; `marker.tdk {201, 201, A_M}` | unchanged | `log` row 201 |
| minter | `(1, 42, NB_D)` in memory and in the `origin_state` of the Puts | unchanged | | |

The user saw "Pay rent" on Desk at once with "· 1 waiting", then synced; "Pay rent" on Mac a moment later. No notice, refusal, alarm or banner anywhere.

## Scenario 2: offline edit replayed

Reading of the scenario: the Phone app ran without network at `T0 − 2 h` and `T0 − 1 h` (every `Connect` ended `Disconnected { unreachable: true }`, device.md §10.6 row 1), its process then ended by `shutdown` (checkpoint at `B = 200`, ENG-CKPT-002), and at `T0` the user launches it anew. Phone's `last_beat_ms` is not fixed by the world; I take no beat as due at `T0` (engine.md §23.1). Given entries: `F201` by Desk (`ctr 122`, `at_ms = T0 − 90 min`, based on `(200, C200)`), `F202` by Mac (`ctr 61`, `at_ms = T0 − 30 min`, based on `(201, C201)`).

1. **User** (Phone), `T0 − 2 h`: marks "Call plumber" done on the phone page; the page sends `POST /api/command` with body `{"op":"complete","id":1099511627783,"at":…}` (bridge.md §6.2 row 3, §6.3).

   AMBIGUOUS: bridge.md §6.3 gives the page's JSON as `{op, id, at}` without fixing the value of `at` (absent, empty, a naive local time or an RFC 3339 time); every reading gives `at' = 2026-11-22T08:00:00.000000000Z` here, because `now` has zero seconds (core.md §12.2 step 4, §13.3 G3, §13.4 `complete`).
2. **Phone** bridge: `op` is one of the 17 verbs; hands the text unchanged to `submit_json` (BRIDGE-VERB-002).
3. **Phone** (sync thread): `parse_intent` → `complete { id, at }`; compile (core.md §13.4 `complete`) against the pending view (`B = 200`, no record): item live; kind task; `at` (G3, G5); archive rows below 200,000; `at' = min(at, now + 1 day)` (or `now`) = `2026-11-22T08:00:00.000000000Z`. No id minted; minter stays `(3, 6, NB_P)`. Payload:

   | Field | Width | Value |
   |---|---|---|
   | `cmd` payload | 74 | `{"at":"2026-11-22T08:00:00.000000000Z","id":1099511627783,"op":"complete"}` |

4. **Phone**: `Enqueue { ticket: ta, command, at_ms: 1,795,334,400,000, origin_state: u16 3 ‖ u64 6 ‖ u64 NB_P, hold: false }` (DEV-FACADE-001, device.md §8.3).
5. **Phone** engine §9.1: step 1 joined, not revoked, no valid skew measurement (no block), `|at_ms − wall_ms| ≤ 60,000`; step 2 no held record; step 3 trial apply, `Ctx { 201, 3, T0 − 2 h, 200 }`: `complete` checks (core.md §9.8): item live, kind task, archive cap → success; the item's latest stamp has `seq ≤ 200 = based_on`, so no overwrite notice.
6. **Phone** engine §9.1 step 4: **RAND1[16]** = `Ea`; record `ra { order o_P, at_ms 1,795,334,400,000, based_on (200, C200), kind cmd, body Cmd(74 bytes), origin_state, via Append, hold false, reseals 0 }`; `Persist Q1 = [Outbox[Put(ra)]]`; pending view: item 1,099,511,627,783 removed, archive row `{archived_at 2026-11-22T08:00:00.000000000Z, name "Call plumber", outcome finished, …}` inserted; `view_version += 1`. Send loop: session not `Live` → nothing sealed (§10.1, ENG-SESS-001).
7. **Phone** runtime: `outbox.tdo` record, nonce **RAND2[24]**, plaintext `u16 1 ‖ 0x01 ‖ vb32(ra JSON)` (`ra` JSON 389 + d(o_P) bytes: `{"at_ms":1795334400000,"based_on":{"chain":"<hex C200>","seq":200},"body":{"cmd":"<base64, 100 chars>"},"entry_id":"<hex Ea>","hold":false,"kind":"cmd","order":o_P,"origin_state":"<base64, 24 chars>","reseals":0,"via":"append"}`); `sync`; `Persisted(Q1)` → `Notify(Enqueued { ta, Ea, 201 })`; the page gets `{"ok":true,"version":v1}`.
8. **Phone** UI: "Call plumber" leaves the task list (it is in the archive as finished); sync line "relay unreachable · 1 waiting" (engine.md §30.1).
9. **User** (Phone), `T0 − 1 h`: quick-adds "Buy stamps": `{"op":"quick_add","name":"Buy stamps","deadline":…}` with no deadline (bridge.md §6.3).
10. **Phone** compile `quick_add` (core.md §13.4): name "Buy stamps"; no deadline; cap; mint `n = max(6, mark(3).item + 1 = 6) = 6`, id `3 × 2^40 + 6 = 3,298,534,883,334`, minter `(3, 7, NB_P)`. Payload:

    | Field | Width | Value |
    |---|---|---|
    | `cmd` payload | 79 | `{"id":3298534883334,"name":"Buy stamps","op":"create_task","time_importance":1}` |

11. **Phone** engine: `Enqueue { tb, command, at_ms: 1,795,338,000,000, origin_state: u16 3 ‖ u64 7 ‖ u64 NB_P, hold: false }`; §9.1 checks pass; trial apply `Ctx { 202 (= 200 + 1 + 1), 3, T0 − 1 h, 200 }`: mint check 3 = `device_no`, 6 > mark 5 → success. **RAND3[16]** = `Eb`; record `rb { order o_P + 1, …, based_on (200, C200) }`; `Persist Q2 = [Outbox[Put(rb)]]`; pending view gains "Buy stamps" (stamps `(202, 3)`).
12. **Phone** runtime: `outbox.tdo` record, nonce **RAND4[24]** (`rb` JSON 397 + d bytes); `Persisted(Q2)` → `Notify(Enqueued { tb, Eb, 202 })`; the page gets `{"ok":true,"version":v2,"id":3298534883334}`. UI: "Buy stamps" shown, "· 2 waiting".
13. **Phone** at `T0`: `Device::open` (device.md §7.4); session id **RAND5[16]** from `OsEntropy` (DEV-NET-006); `Loaded` holds the checkpoint at 200 and the outbox ops `Put(ra)`, `Put(rb)`; facade minter = max of `device.tdk`'s and every `origin_state` = `(3, 7, NB_P)` (device.md §8.3).
14. **Phone** engine `Start` (§7.10, §15.1, §15.2): `attempt := A_P` (one above the stored); outbox rebuilt (ENG-STATE-003); no tail; pending view at `B = 200`: `ra` at provisional 201 (`Ctx { 201, 3, T0 − 2 h, 200 }`), `rb` at 202; `Persist Q3 = [WriteMeta(attempt A_P, prev_session = the old session)]` (replace: `file_id` **RAND6[16]**, nonce **RAND7[24]**); session `Offline` → `Connect { relay: relay.example, door, home_gen 1 }` (§11.1).
15. **Phone** UI before sync (what its screen shows): no "Call plumber" (archived, named "Call plumber"), "Buy stamps" present, no "Pay rent", "· 2 waiting".
16. **Phone** shell: `onStart` → `set_foreground(true)` → `notify_resume`; the transport lets the held `Connect` through with no backoff (first `Connect` after `open`) (device.md §10.7, §10.9, DEV-NET-017).
17. **Phone** transport (device.md §10.5 `Connect`): stored token of generation 1 with `push: true` → no login, no `caps`; `GET /v1/b/<hex BID>/socket` to `wss://relay.example` with `Host`, `User-Agent: TaskDeck-Sync/1`, `Authorization: Bearer <Phone's token>`, the upgrade headers (DEV-NET-004).
18. **Relay** gate (§6.2): steps 0 to 7 pass (route `socket`, no query, no body, `Upgrade: websocket`, Bearer form, token MAC, board, role 1); object (§11.1): PHONE row `role 1`, `state 1`, `gen 1`; board `open` → `acceptWebSocket` tagged `<hex PHONE>`, attachment `{dev, gen 1, features 0, ready false}`, `101`.
19. **Phone** transport → relay, hello (§11.3), 21 bytes: `0x01` [1] ‖ `RAND5` [16] ‖ u32 0 [4]. Engine receives `Connected { push: true }` (no `Hello` effect).
20. **Relay**: hello exact, no session yet; no other PHONE socket; stores `session`, `features 0`, `ready true`; sends welcome:

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `0x81` |
    | `head_seq` | 8 | 202 |
    | `head_chain` | 32 | `C202` |
    | `last_ctr` | 8 | 18 |
    | `last_seq` | 8 | `q_P` (seq of Phone's ctr 18, ≤ 200) |
    | `relay_now` | 8 | `RN2` |
    | `floor` | 8 | 1 |
    | `purge_floor` | 8 | 1 |
    | `state` | 1 | 1 |
    | `caps` | 1 | `0x01` |
    | `appends_left` | 4 | `3,000 − app_count` (3,000 when `app_day ≠ 20,779`) |
    | `door` | 5 | the board's door |
    | `has_fence` | 1 | 0 |
    | | total | 93 |

21. **Phone** engine, `Welcome` (§11.2): (1) skew from `RN2` (§27.1); (2) `head.seq 202 ≥ L = 200`, no chain held at 202; (3) no `ack`; (4) not fenced; (5) reconcile (§11.3) with `a = 18`, `r = 18`, `q = q_P`: normal; nothing to read for its own counters (`q_P ≤ L`). Head above `L` → `Read { req: p1, after: 200, limit: 256 }`; session `Reconciling`.

    AMBIGUOUS: engine.md §11.3 defines `h` as "the largest counter of a record with a frame" and has no value when no record has a frame (as here); read literally as 0, `r = 18 > h` would be step 3, a restored copy. I take `h = a` (step 2, normal).
22. **Phone** transport → relay: read `0x03` [1] ‖ u32 `r_p1` [4] ‖ u64 200 [8] ‖ u16 256 [2] (15 bytes).
23. **Relay**: re-reads PHONE's row (current, `gen 1`, RELAY-SOCK-007); selects seqs > 200 and ≥ `purge_floor` 1: 201, 202 (RELAY-READ-001); answers `0x83` [1] ‖ u32 `r_p1` [4] ‖ page: `"TDP1"` [4] ‖ u64 202 ‖ `C202` [32] ‖ i64 `RN3` ‖ u64 1 ‖ u64 1 ‖ u8 flags 0 ‖ u16 2 ‖ (u64 201 ‖ i64 `at201` ‖ u32 670 ‖ `F201`) ‖ (u64 202 ‖ i64 `at202` ‖ u32 670 ‖ `F202`) (page 1,451 bytes, message 1,456).
24. Frames 201 and 202 (seal.md §8.1, §9.1, §10.1; padding `512 − 69 − payload_len`; every body 558 bytes, every frame 670):

    | Field | Width | `F201` (Desk) | `F202` (Mac) |
    |---|---|---|---|
    | `magic` | 4 | `"TDF1"` | `"TDF1"` |
    | `board_id` | 16 | `BID` | `BID` |
    | `dev` | 16 | `DESK` | `MAC` |
    | `ctr` | 8 | 122 | 61 |
    | `len` | 4 | 558 | 558 |
    | `ver`, `suite`, `epoch` | 1, 1, 4 | 1, 1, 1 | 1, 1, 1 |
    | `nonce` | 24 | Desk's nonce | Mac's nonce |
    | ciphertext | 528 | `AEAD(K_entry,1; nonce; "taskdeck/v1/entry\0" ‖ header[48] ‖ 1 ‖ 1 ‖ u32 1; plaintext)` | same form |
    | plaintext `entry_id` | 16 | Desk's | Mac's |
    | plaintext `kind` | 1 | 1 | 1 |
    | plaintext `at_ms` | 8 | 1,795,336,200,000 | 1,795,339,800,000 |
    | plaintext `based_on_seq` | 8 | 200 | 201 |
    | plaintext `based_on_chain` | 32 | `C200` | `C201` |
    | plaintext `payload_len` | 4 | 77 | 60 |
    | plaintext `payload` | | `{"id":1099511627817,"name":"Pay rent","op":"create_task","time_importance":1}` | `{"id":1099511627783,"name":"Call the plumber","op":"rename"}` |
    | plaintext padding | | 366 × `0x00` | 383 × `0x00` |
    | `sig` | 64 | `SIG(Desk key; "taskdeck/v1/frame\0"; F201[0..606])` | `SIG(Mac key; "taskdeck/v1/frame\0"; F202[0..606])` |

    `C201 = H("taskdeck/v1/chain\0" ‖ C200 ‖ u64 201 ‖ H(F201))`, `C202 = H("taskdeck/v1/chain\0" ‖ C201 ‖ u64 202 ‖ H(F202))` (seal.md §15.1).
25. **Phone** engine, `Page { req: p1 }`, §12.3 steps 1 to 5 (2 entries, 1,340 bytes of frames; skew; head checks; not truncated; 201 = `L + 1`, 202 next); step 6 dry run: `C201 = chain_step(C200, 201, F201)`, classify with `M_200`: seal.md §14.2 (1) to (5) pass (DESK current, `122 = 121 + 1`), (6) not own dev, (7) to (11) pass, (12) `b = 200`, `C200` held and equal, (13) pass → `Accept`, `M_201` (`DESK.last_ctr 122`, `clock_ms = T0 − 90 min`); `C202 = chain_step(C201, 202, F202)`, classify with `M_201`: MAC current, `61 = 60 + 1`, (12) `b = 201 < 202`, `C201` (from this dry run) equal → `Accept`, `M_202` (`MAC.last_ctr 61`, `clock_ms = T0 − 30 min`). Step 7: last entry is 202 = `head.seq`, its chain equals `C202`.
26. **Phone** engine, step 8: `Persist Q4 = [AppendLog[{201, C201, τ 1,795,336,200,000, F201}, {202, C202, τ 1,795,339,800,000, F202}], SetMarker{201, 202, A_P}]`; runtime: one log record, nonce **RAND8[24]**, plaintext `u16 2 ‖ (u64 201 ‖ C201 ‖ i64 τ201 ‖ u32 670 ‖ F201) ‖ (u64 202 ‖ C202 ‖ i64 τ202 ‖ u32 670 ‖ F202)` (1,446 bytes); no history record; `marker.tdk` replaced (`file_id` **RAND9[16]**, nonce **RAND10[24]**, plaintext `u64 201 ‖ u64 202 ‖ u64 A_P`).
27. **Phone** engine, on `Persisted(Q4)`: `L = K = 202`; apply 201 (`create_task` "Pay rent", `Ctx { 201, 1, T0 − 90 min, 200 }`, no notice) and 202 (`rename`, `Ctx { 202, 2, T0 − 30 min, 201 }`: item 1,099,511,627,783 is live in the confirmed state; name "Call the plumber", stamp `name = (202, 2)`; the old name stamp has `seq ≤ 201`, so no notice); `B = 202`; no records; the runtime raises the minter from marks: `mark(3) = 5`, minter stays `(3, 7, NB_P)` (§11.3 end).
28. **Phone** engine, §12.3 step 10: run ended → rebase (§8.4 case 1): `ra` under `Ctx { 203, 3, T0 − 2 h, based_on min(200, 202) = 200 }`: item live, task; `archived_at = min(08:00Z, ctx.at + 1 day) = 2026-11-22T08:00:00.000000000Z`; the row copies the item, now named "Call the plumber"; overwrite notice (group `item`, `other 2`, `other_seq 202`, because the item's latest stamp `(202, 2)` has `202 > 200` and `2 ≠ 3`, core.md §10.1, §10.2) shown with `ra`, never recorded (ENG-SM-004); `rb` under `Ctx { 204, 3, T0 − 1 h, 200 }` → success. Neither doomed. `view_version += 1`. `L` = the relay head and the reconcile is complete → session `Live`.
29. **Phone** UI after catch-up: "Pay rent" appears; "Call plumber" still not in the list; the archive row reads "Call the plumber", finished 10:00 local; the pending complete carries its overwrite notice for display; "· 2 waiting".
30. **Phone** engine, §4.5 step 5 duties: now eligible (§29.1); beat not due (reading above); no snapshot due (no aligned seq applied); compaction: `b ≤ L − 16,384 < floor`, nothing to send (§20.2 step 2).
31. **Phone** engine, step 6 send loop: `R = [ra, rb]`, no frames; epoch gate open; `ra`: `ctr = 1 + max(18, none, 18) = 19`, `Sealer::seal(M_202, 19, ra)`, nonce **RAND11[24]**; `rb`: `ctr = 1 + max(18, 19, 18) = 20`, `seal(M_202, 20, rb)`, nonce **RAND12[24]**; both keep `based_on (200, C200)` and their enqueue `at_ms` (ENG-ENQ-005, SEAL-PLAIN-003). `Persist Q5 = [Outbox[Put(ra'), Put(rb')]]`; runtime: one `outbox.tdo` record, nonce **RAND13[24]**, plaintext `u16 2 ‖ 0x01 ‖ vb32(ra' JSON, 1,395 + d bytes) ‖ 0x01 ‖ vb32(rb' JSON, 1,403 + d bytes)`.
32. Phone's frames (layout of scenario 1 step 11; AD and signature forms as there, with `dev = PHONE`):

    | Field | Width | `F203` (`ra`) | `F204` (`rb`) |
    |---|---|---|---|
    | header | 48 | `"TDF1"` ‖ `BID` ‖ `PHONE` ‖ u64 19 ‖ u32 558 | `"TDF1"` ‖ `BID` ‖ `PHONE` ‖ u64 20 ‖ u32 558 |
    | `ver`, `suite`, `epoch` | 6 | 1, 1, 1 | 1, 1, 1 |
    | `nonce` | 24 | `RAND11` | `RAND12` |
    | ciphertext | 528 | `AEAD(K_entry,1; RAND11; "taskdeck/v1/entry\0" ‖ F203[0..48] ‖ 1 ‖ 1 ‖ u32 1; plaintext)` | `AEAD(K_entry,1; RAND12; "taskdeck/v1/entry\0" ‖ F204[0..48] ‖ 1 ‖ 1 ‖ u32 1; plaintext)` |
    | plaintext `entry_id` | 16 | `Ea` | `Eb` |
    | plaintext `kind` | 1 | 1 | 1 |
    | plaintext `at_ms` | 8 | 1,795,334,400,000 | 1,795,338,000,000 |
    | plaintext `based_on_seq` | 8 | 200 | 200 |
    | plaintext `based_on_chain` | 32 | `C200` | `C200` |
    | plaintext `payload_len` | 4 | 74 | 79 |
    | plaintext `payload` | | step 3 | step 10 |
    | plaintext padding | | 369 × `0x00` | 364 × `0x00` |
    | `sig` | 64 | `SIG(Phone key; "taskdeck/v1/frame\0"; F203[0..606])` | `SIG(Phone key; "taskdeck/v1/frame\0"; F204[0..606])` |
    | total | | 670 | 670 |

33. **Phone** engine, on `Persisted(Q5)`: `Append { req: p2, frames: [F203, F204] }`, one batch (ENG-SEND-001); socket message `0x02` ‖ u32 `r_p2` ‖ `"TDA1"` ‖ u8 2 ‖ u32 670 ‖ `F203` ‖ u32 670 ‖ `F204` (1,358 bytes).
34. **Relay**: clock `RN4`; batch layout valid; PHONE row current, `gen 1`; P1 to P7 for frame 0, then frame 1 (`verifyStrict(PHONE_SIGN, …)` each); transaction: head `(202, C202)`; frame 0: T1, T2 `19 = 18 + 1`, T4, T5 `seq 203`, `C203 = H("taskdeck/v1/chain\0" ‖ C202 ‖ u64 203 ‖ H(F203))`; frame 1: T2 `20 = 19 + 1`, T4 `count + 1 < 3,000`, T5 `seq 204`, `C204 = H("taskdeck/v1/chain\0" ‖ C203 ‖ u64 204 ‖ H(F204))`; PHONE row written once: `last_ctr 20`, `last_seq 204`, `app_count + 2` (RELAY-SEQ-007); commit.
35. **Relay** → Phone, result: `0x82` ‖ u32 `r_p2` ‖ `"TDR1"` ‖ u8 2 ‖ (0 ‖ u64 203 ‖ `C203`) ‖ (0 ‖ u64 204 ‖ `C204`) (92 bytes). **Relay** → Desk, Mac and Phone sockets, push: `0x84` ‖ `"TDP1"` ‖ u64 204 ‖ `C204` ‖ i64 `RN4` ‖ u64 1 ‖ u64 1 ‖ u8 0 ‖ u16 2 ‖ (u64 203 ‖ i64 `at203` ‖ u32 670 ‖ `F203`) ‖ (u64 204 ‖ i64 `at204` ‖ u32 670 ‖ `F204`) (1,452 bytes).

    AMBIGUOUS: as in scenario 1, relay.md §11.4 does not order `result` and `push` on the author's socket; I take `result` first.
36. **Phone** engine, `AppendResults`: `ra.ack = (203, C203)`, `rb.ack = (204, C204)`, Puts deferred to the next `Persist`; both above `L = 202`, no echo yet; `queued = 0`.
37. **Phone** engine, push `Page`, §12.3: dry run `C203`, `C204`; classify 203 with `M_202`: PHONE current, signature, `19 = 18 + 1`, (6) own dev with `own_sealed(19) = H(F203)` → no flag, `based_on 200` with `C200` equal → `Accept`; 204 with `M_203`: `20 = 19 + 1`, `own_sealed(20) = H(F204)` → `Accept`. `τ203 = max(T0 − 30 min, T0 − 2 h) = 1,795,339,800,000`, `τ204 = 1,795,339,800,000`. `Persist Q6 = [AppendLog[{203, C203, τ203, F203}, {204, C204, τ204, F204}], SetMarker{203, 204, A_P}, Outbox[Put(ra''), Put(rb'')]]` (log record nonce **RAND14[24]**; marker `file_id` **RAND15[16]**, nonce **RAND16[24]**; outbox nonce **RAND17[24]**, `ra''`, `rb''` with `"ack":[203,"<hex C203>"]`, `"ack":[204,"<hex C204>"]`).

    AMBIGUOUS: the place of the deferred ack Puts inside the next `Persist` (engine.md §10.4), as in scenario 1.
38. **Phone** engine, on `Persisted(Q6)`: `L = K = 204`; apply 203 (`complete`, `Ctx { 203, 3, T0 − 2 h, 200 }`): item archived as finished with name "Call the plumber", `archived_at 2026-11-22T08:00:00.000000000Z`; one overwrite notice `N1`; `notice_concerns(N1, 3)` true (author) → recorded; `ra.seen = 203`, dropped. Apply 204 (`create_task`, `Ctx { 204, 3, T0 − 1 h, 200 }`): "Buy stamps" live, `marks[3].item = 6`; `rb.seen = 204`, dropped. `B = 204`; echo checks for 203 and 204 pass; rebase (`U` empty). `Persist Q7 = [Notices[Add(N1)], Outbox[Drop(Ea), Drop(Eb)]]` (notices nonce **RAND18[24]**, outbox nonce **RAND19[24]**); after `Persisted(Q7)`, `Notify(NoticesAdded { notices: 1, refusals: 0 })` (ENG-REF-002), `ViewChanged`, `StatusChanged`. `N1` (core.md §10.7) and its op (device.md §6.4.5):

    | Field | Width | Value |
    |---|---|---|
    | `n` | 2 | 1 |
    | op | 1 | `0x01` (`Add`) |
    | `kind` | 1 | 1 (notice) |
    | `seq` | 8 | 203 |
    | record length | 4 | 103 |
    | record | 103 | `{"author":3,"group":"item","item":1099511627783,"kind":"overwrite","other":2,"other_seq":202,"seq":203}` |

    AMBIGUOUS: engine.md §13.3, §13.4 and §7.12 do not fix whether the notice `Add` and the outbox `Drop`s go in one `Persist` or which comes first; I put both in one, notice first.
39. **Desk** engine, the push: §12.3 as step 37 but not own dev; classification `Accept` for 203 and 204 (PHONE current, `19 = 18 + 1`, `20 = 19 + 1`, `C200` held); `Persist [AppendLog[203, 204], SetMarker{203, 204, A_D}]`; apply 203: "Call the plumber" archived as finished; `N1` does not concern `device_no` 1 → no record; apply 204: "Buy stamps" added; `B = 204`; rebase.
40. **Mac** engine, the push: as Desk; apply 203 produces `N1`, `notice_concerns(N1, 2)` true (`other`) → `Persist [Notices[Add(N1)]]` (same op bytes as step 38), then `NoticesAdded { 1, 0 }`; apply 204; `B = 204`; rebase.
41. **User** sees: Phone: "Pay rent" and "Buy stamps" in the list, "Call the plumber" in the archive (finished 10:00), synced, one unread notice (`notices_unread 1`) that its completion overwrote a change made on Mac at seq 202. Mac: "Call the plumber" leaves its list into the archive, "Buy stamps" appears, one unread notice that Phone's entry at 203 overwrote its rename at 202. Desk: the same board changes, no notice.

"Done" on the renamed task, every device: the `complete` at 203 applies after the `rename` at 202, so on Desk, Mac and Phone the item 1,099,511,627,783 is archived as `finished` with name "Call the plumber" and `archived_at 2026-11-22T08:00:00.000000000Z`; nothing is refused; Mac's new name survives in the archive row.

**End state.**

| | Desk | Mac | Phone | Relay |
|---|---|---|---|---|
| head | `(204, C204)`, `B = K = L = 204` | same | same | head `(204, C204)` |
| counters in `M_204` | DESK 122, MAC 61, PHONE 20, `dev_R` 1 | same | same | rows: DESK 122/201, MAC 61/202, PHONE 20/204 |
| epoch, rules, `clock_ms` | 1, 1, `1,795,339,800,000` | same | same | |
| outbox | empty | empty | empty; `outbox.tdo` +4 records (Put ra, Put rb, Put both with frames, Put both with acks) +1 (two Drops) | |
| notices | none added | `notices.tdk` +1 `Add(N1)` | `notices.tdk` +1 `Add(N1)` | |
| other files | log +record (203, 204); `marker.tdk {203, 204, A_D}` | log +records for 201, 202 (earlier) and 203, 204; marker `{203, 204, A_M}` | log +2 records (201–202, 203–204); `marker.tdk {203, 204, A_P}`; `meta.tdk` with `attempt A_P` | `log` rows 203, 204 |
| minter | | | `(3, 7, NB_P)` | |

No refusal record, alarm, stop or banner on any device.
