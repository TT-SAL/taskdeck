# Group `edit` — reader B

Notation: `ctx(x)` = ASCII `taskdeck/v1/x` ‖ `0x00` (README §5.2). Lengths: `ctx("frame")`, `ctx("entry")`, `ctx("chain")` 18 bytes each; `ctx("key-entry")` 22. Ids: `I7` = 1×2^40+7 = 1,099,511,627,783; `I41` = 1×2^40+41 = 1,099,511,627,817; `P6` = 3×2^40+6 = 3,298,534,883,334. Instants: `T0` = 1,795,341,600,000 ms; `T0−90min` = 1,795,336,200,000; `T0−30min` = 1,795,339,800,000; `T0−2h` = 1,795,334,400,000; `T0−1h` = 1,795,338,000,000. `K1 = K_entry,1 = HKDF(salt=BID, ikm=R_1, info=ctx("key-entry") ‖ u32 1, L=32)` (seal.md §6.5). `H` = SHA-256.

## Scenario 1 — First edit

1. **User** (Desk), at `T0`: quick-add, name "Pay rent", no due date. The facade calls `submit` (not `submit_named`), so `Enqueue.hold = false` (device.md §17.4 table, row `submit_named`).
2. **Desk** `parse_intent` → `quick_add{name:"Pay rent", deadline: none}` (core.md §12.2, §12.3).
3. **Desk** `compile` against the pending view with `now = T0` (core.md §13.4 row `quick_add`): trim (G1); 8 scalars ≤ 200 (G2); not empty; no deadline; cap 10,000 live items OK on view; mint `id = max(minter.next_item, mark(view,1).item + 1) = max(41, 41) = 41` → `I41`; minter `next_item := 42` (CORE-ID-004, §7.2). Not dated → `time_importance = 1`. Reply `{id: I41}`.
4. **Desk** canonical command bytes `CMD1` (core.md §4.2, §9.4; `blocks` at default `[]` omitted, CORE-ENC-002):

   | Field | Width | Value |
   |---|---|---|
   | `CMD1` | 77 | `{"id":1099511627817,"name":"Pay rent","op":"create_task","time_importance":1}` |

5. **Desk** engine §9.1 step 1: joined, not revoked, clock not blocking, `|at_ms − wall_ms| ≤ 60,000` → pass. Step 2: no held last record.
6. **Desk** trial apply on a copy of the pending view, `ctx = {seq: 201, device_no: 1, at: T0, based_on: 200}` (§9.1 step 3; `B = 200`, `|U| = 0`): mint check `I41` (high part 1 = device_no, low 41 > mark 40), cap → success.
7. **Desk** draws `RAND1[16]` = `entry_id` (`Sealer::new_entry_id`, ENG-ENQ-003).
8. **Desk** builds the outbox record (§9.1 step 4, §7.4) and emits `Persist[Outbox[Put(R1)]]`. Record JSON (device.md §6.5 mapping, canonical key order; `None` fields absent):

   ```json
   {"at_ms":1795341600000,"based_on":{"chain":"<hex C200>","seq":200},"body":{"cmd":"<base64 CMD1>"},"entry_id":"<hex RAND1>","hold":false,"kind":"cmd","order":<o+1>,"origin_state":"<base64 IdMinter{device_no:1,next_item:42,next_block:..}>","reseals":0,"via":"append"}
   ```

   AMBIGUOUS: `order` is "one more than the largest `order` ever used" (engine.md §7.4), but with an empty outbox (all earlier records dropped, possibly rewritten) no persisted record states the largest order ever used; the spec does not say where that counter survives a `Start`.
   Durable write: `outbox.tdo` append procedure, plaintext `u16 n=1 ‖ 0x01 ‖ vb32(JSON)` (device.md §6.4.4, §7.2), sealed under `K_file(5)` (seal.md §6.6, §19).
9. **Desk** applies `CMD1` to the pending view, `view_version += 1`; the UI shows "Pay rent" (§8.2).
10. **Desk** on `Persisted`: `Notify(Enqueued{ticket, entry_id: RAND1, provisional_seq: 201})`; `submit` returns (ENG-ENQ-004, D1).
11. **Desk** send loop (§10.1: `Live`, no stop, not fenced, not revoked, no pause, nothing outstanding; §10.2): `R = [R1]`, `via = Append`, no frame, not held → seal. `next_ctr = 1 + max(a=121, h=121, w=121) = 122` (§10.3).
12. **Desk** sealer builds the plaintext (seal.md §10.1, §10.2: `U = 69 + 77 = 146`, `P(146) = 512`):

    | Offset | Field | Width | Value |
    |---|---|---|---|
    | 0 | `entry_id` | 16 | `RAND1` |
    | 16 | `kind` | 1 | 1 (`cmd`) |
    | 17 | `at_ms` | 8 | 1,795,341,600,000 |
    | 25 | `based_on_seq` | 8 | 200 |
    | 33 | `based_on_chain` | 32 | `C200` |
    | 65 | `payload_len` | 4 | 77 |
    | 69 | `payload` | 77 | `CMD1` |
    | 146 | `padding` | 366 | `0x00` × 366 |
    | | total | 512 | |

13. **Desk** draws `RAND2[24]` = envelope nonce (SEAL-GEN-012).
14. **Desk** frame `F201` (seal.md §8.1, §9.1; `len = 30 + 512 + 16 = 558`):

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
    | 54 | `nonce` | 24 | `RAND2` |
    | 78 | `ciphertext` | 528 | `AEAD(K1; RAND2; AD; plaintext[512])` |
    | 606 | `sig` | 64 | `SIG(Desk signing key; "taskdeck/v1/frame\0"; F201[0..606])` |
    | | total | 670 | |

    AD (seal.md §9.2), 72 bytes: `ctx("entry")[18] ‖ F201[0..48] ‖ u8 1 ‖ u8 1 ‖ u32 1`. Signed bytes (§8.3): `ctx("frame")[18] ‖ F201[0..606]` = 624 bytes.
15. **Desk** sets `ctr = 122`, `frame = F201`, `frame_hash = H(F201)`, `epoch = 1` on `R1`; `Persist[Outbox[Put(R1')]]` (D2); the record JSON gains `"ctr":122`, `"epoch":1`, `"frame":"<base64 F201>"`, `"frame_hash":"<hex H(F201)>"` in key order.
16. **Desk** on `Persisted`: `Append{req, frames:[F201]}` (§10.2 step 5). Transport sends over the socket (device.md §10.5):

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `0x02` |
    | `req` | 4 | `r1` (transport counter) |
    | `magic` | 4 | `"TDA1"` |
    | `n` | 1 | 1 |
    | `len_0` | 4 | 670 |
    | `frame_0` | 670 | `F201` |
    | total | 684 | |

17. **Relay** before handling: re-reads Desk's devices row: current, `gen = 1` = attachment's (relay.md §11.4, RELAY-SOCK-007). Board `open`. Batch layout OK (§9.2).
18. **Relay** P1 (670 bytes, `"TDF1"`, `len = 670 − 112`), P2 (`BID`), P3 (`ctr` 122 in range), P4 (row `DESK`), P5 (`dev` = token's), P6 (`state = 1`), P7 `verifyStrict(DESK_SIGN, ctx("frame") ‖ F201[0..606], sig)` (relay.md §9.3).
19. **Relay** transaction (§9.4): T1 row unchanged; T2 `122 = 121 + 1`; T4 `app_count` of today < 3,000; T5 `seq = 201`, `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ H(F201))` (90-byte input, seal.md §15.1); insert `log(201, DESK, 122, H(F201), C201, at = max(now, at_200), F201)` (+ index row `log_dev_ctr`); Desk row `last_ctr = 122`, `last_seq = 201`, `app_day = today`, `app_count += 1`. Commit (RELAY-OBJ-006/007).
20. **Relay** → Desk socket, result first (RELAY-SOCK-012):

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `0x82` |
    | `req` | 4 | `r1` |
    | `magic` | 4 | `"TDR1"` |
    | `n` | 1 | 1 |
    | `status` | 1 | 0 (`new`) |
    | `seq` | 8 | 201 |
    | `chain` | 32 | `C201` |
    | total | 51 | |

21. **Relay** → every ready socket (Desk and Mac), push (§11.4, §10.5):

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `0x84` |
    | `magic` | 4 | `"TDP1"` |
    | `head_seq` | 8 | 201 |
    | `head_chain` | 32 | `C201` |
    | `relay_now` | 8 | relay clock |
    | `floor` | 8 | 1 (`max(compact_floor 1, purge_floor 1)`) |
    | `purge_floor` | 8 | 1 |
    | `flags` | 1 | 0 |
    | `n` | 2 | 1 |
    | entry `seq` | 8 | 201 |
    | entry `at` | 8 | the row's `at` |
    | entry `flen` | 4 | 670 |
    | entry `frame` | 670 | `F201` |
    | total | 762 | |

22. **Desk** `AppendResults`: `new (201, C201)` → `R1.ack = (201, C201)`, its `Put` held for the next `Persist`, placed last (§10.4); `201 > L = 200`, so no echo check yet; `conflicts = 0`.
23. **Desk** push page (§12.2: first entry `L + 1` → processed as a page; §12.3): limits OK; `relay_now` measured; head check (201 > L); not truncated; contiguous; dry run: `C201` computed, `classify` → `Accept` (same checks as step 27, author is itself: own-dev check finds its own record of `ctr 122` with `H(F201)`, no flag); head chain equals page's `C201`.
24. **Desk** `Persist[AppendLog[(201, C201, τ_201 = max(τ_200, T0), F201)], SetMarker{201, 201, attempt}, Outbox[Put(R1 with "ack":[201,"<hex C201>"])]]` (§12.3 step 8, D3). Durable: `log-<first seq>.tdl` page record `u16 1 ‖ u64 201 ‖ C201 ‖ i64 τ ‖ u32 670 ‖ F201` (device.md §6.4.1); `marker.tdk` `u64 201 ‖ u64 201 ‖ u64 attempt`; outbox op.
25. **Desk** on `Persisted`: `L = K = 201`, `M_K` = `M_201` (Desk `last_ctr = 122`, `τ = max(τ, T0)`); echo check of `R1`: log frame hash at 201 = `R1.frame_hash`, `C201` = ack chain → pass (§11.5). Board applies 201: `parse_command(CMD1)` → `apply` with `Ctx{seq 201, device_no 1, at T0, based_on 200}` → item inserted; author is Desk and `entry_id = RAND1` → `R1.seen = 201`, `Drop(R1)` (§12.4 step 3); `B = 201` (201 not aligned: no hash).
26. **Desk** `Persist[Outbox[Drop(RAND1)], ClearMarker]` (§12.4 "After the apply"); rebase (run ended, §8.4 case 1): pending view = confirmed state; `view_version += 1`.
27. **Mac** push page (§12.3) — receipt checks in order (seal.md §14.2): (1) 670 bytes, `"TDF1"`, `len = 558`; (2) `BID`; (3) `DESK` is a current device of `M_200`; (4) strict signature under `DESK_SIGN`; (5) `122 = last_ctr 121 + 1`; (6) not Mac's own dev; (7) current; (8) `(1,1)` allowed by rules 1; (9) epoch 1 = `M_200.epoch`; (10) AEAD opens under `K1` with AD of step 14; (11) unpadding: `L = 512 ≥ 69`, `U = 146 ≤ 512`, `P(146) = 512`, bytes 146..512 zero, `kind = 1`, `at_ms` in range; (12) `based_on`: `200 < 201`, `200 ≥ retained_from`, `C200` equals Mac's `C200`; (13) `cmd` by a current device under the current epoch; (14) n/a; (15)–(16) n/a for `cmd`; (17) `Accept`. Bookkeeping: Desk `last_ctr = 122`, `τ' = max(τ, T0)` (seal.md §13.5).
28. **Mac** head chain check: `C201` = page head chain (§12.3 step 7).
29. **Mac** `Persist[AppendLog[(201, C201, τ_201, F201)], SetMarker{201, 201, attempt}]` (D3).
30. **Mac** on `Persisted`: `L = K = 201`; board apply: `parse_command(CMD1)` (core.md §9.1 seven steps) → `create_task`; `apply` `Ctx{201, d_author = 1, T0, 200}`: §8.4 step 1 ctx OK; step 2 op in rules 1; mint check `I41` (1 = device_no, 41 > mark 40); cap → effects: insert item, raise mark of device 1 to `item = 41` (core.md §9.4). No notices. `B = 201`.
31. **Mac** `Persist[ClearMarker]`; rebase; `view_version += 1`; the UI shows "Pay rent".

Item inserted on both (core.md §6.2, canonical):

```json
{"created":"2026-11-22T10:00:00.000000000Z","id":1099511627817,"kind":"task","name":"Pay rent","stamps":{"estimate":[201,1],"levels":[201,1],"name":[201,1],"repeat":[201,1],"time":[201,1]},"time_importance":1}
```

**State after the last step**

| Holder | Head | Counters | Epoch | Durable |
|---|---|---|---|---|
| Desk | `(201, C201)`, `B = K = L = 201` | Desk `last_ctr` 122 in `M_201`; Mac 60, Phone 18, `dev_R` 1 | 1 | log segment + page 201; outbox `Put(R1)`, `Put(R1')`, `Put(R1 ack)`, `Drop(R1)`; no marker; checkpoint unchanged (no checkpoint rule fired, §18.1); minter `next_item 42` in `origin_state` |
| Mac | `(201, C201)` | same | 1 | log page 201; no marker; outbox empty |
| Relay | head `(201, C201)` | Desk row `last_ctr 122, last_seq 201`, `app_count +1` | — | `log` rows 1..201 |
| Phone | `(200, C200)` | — | 1 | unchanged (offline) |

User saw: "Pay rent" on Desk at once (pending), then confirmed; "Pay rent" appears on Mac after the push. No notice, no banner.

## Scenario 2 — Offline edit replayed

World additions: entry 201 is `F201'` by Desk (`ctr 122`, `at_ms = T0−90min`, `based_on (200, C200)`, payload `CMD1` of scenario 1, 670 bytes); entry 202 is `F202` by Mac (`ctr 61`, `at_ms = T0−30min`, `based_on (201, C201)`, payload `{"id":1099511627783,"name":"Call the plumber","op":"rename"}`, 60 bytes, `P(129) = 512`, 670 bytes). Relay head `(202, C202)`; relay rows: Desk `last_ctr 122`, Mac 61, Phone 18 (`last_seq = q_P ≤ 200`).

1. **User** (Phone, offline), `T0−2h`: marks "Call plumber" done. The page sends `{"op":"complete","id":1099511627783}` (no `at`) through the bridge; `submit_json` → `parse_intent` (core.md §12.2).
2. **Phone** `compile` (core.md §13.4 row `complete`) on its pending view (= confirmed @200): item live, kind task; `at` none → `at' = now = T0−2h`; archive cap OK. Command `CMD_C` (74 bytes):

   | Field | Width | Value |
   |---|---|---|
   | `CMD_C` | 74 | `{"at":"2026-11-22T08:00:00.000000000Z","id":1099511627783,"op":"complete"}` |

3. **Phone** engine §9.1: step 1 passes (an offline device is not refused, §9.4); trial apply `ctx {seq 201, device_no 3, at T0−2h, based_on 200}` → success (no notice: name stamp ≤ 200).
4. **Phone** draws `RAND3[16]` = `entry_id`; record `RC` (`at_ms = 1,795,334,400,000`, `based_on = (200, C200)`, `kind cmd`, `body {"cmd": base64 CMD_C}`, `origin_state` = minter unchanged `next_item 6`, `hold false`, `via append`, `reseals 0`); `Persist[Outbox[Put(RC)]]` → `outbox.tdo` append; pending view: "Call plumber" leaves the list, appears in done rows; on `Persisted` `Notify(Enqueued{provisional_seq 201})` (D1, ENG-ENQ-004).
5. **User** (Phone, offline), `T0−1h`: quick-add "Buy stamps" (`submit`, `hold = false`).
6. **Phone** `compile` `quick_add`: mint `max(minter.next_item 6, mark(view,3).item 5 + 1) = 6` → `P6`; minter `next_item 7`; undated → `time_importance 1`. `CMD_B` (79 bytes): `{"id":3298534883334,"name":"Buy stamps","op":"create_task","time_importance":1}`.
7. **Phone** trial apply `ctx {seq 202, device_no 3, at T0−1h, based_on 200}` (§9.1 step 3: `B + |U| + 1`) → success; draws `RAND4[16]` = `entry_id`; record `RB` (`at_ms = 1,795,338,000,000`, `based_on (200, C200)`, `origin_state` minter `next_item 7`, `order` = `RC.order + 1`); `Persist[Outbox[Put(RB)]]`; pending view shows "Buy stamps"; `Notify(Enqueued{provisional_seq 202})`.
8. **User** opens the app at `T0`. The Android shell calls `set_foreground(true)` (device.md §10.9: at `open` Android is not foreground until then); `notify_resume` → no backoff.
9. **Phone** screen before sync: pending view = confirmed @200 + `RC` + `RB` (§8.2): "Buy stamps" live; "Call plumber" in the done rows; no "Pay rent"; name still "Call plumber".
10. **Phone** session `Offline` → `Connect` (§11.1). Stored token `gen 1` with a stored `push = true` → no login, no `caps` (device.md §10.3). Socket upgrade `GET wss://relay.example/v1/b/<BID hex>/socket`, `Authorization: Bearer TD1.<92>`; relay §11.1 accepts (row current, `gen 1`).
11. **Phone** → Relay hello (relay.md §11.3), 21 bytes: `0x01 ‖ session[16] (drawn at open, device.md §10.4) ‖ u32 0`.
12. **Relay** no other socket of `PHONE` → store session, `ready = true`; send welcome (93 bytes):

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `0x81` |
    | `head_seq` | 8 | 202 |
    | `head_chain` | 32 | `C202` |
    | `last_ctr` | 8 | 18 |
    | `last_seq` | 8 | `q_P` |
    | `relay_now` | 8 | relay clock |
    | `floor` | 8 | 1 |
    | `purge_floor` | 8 | 1 |
    | `state` | 1 | 1 |
    | `caps` | 1 | 1 |
    | `appends_left` | 4 | `3,000 − app_count` (3,000 if `app_day` ≠ today) |
    | `door` | 5 | `HMAC(TOKEN_KEY, ctx("door") ‖ BID)[0..5]` |
    | `has_fence` | 1 | 0 |

13. **Phone** transport: `Connected{push: true}`, then `Welcome`. Engine §11.2: clock measured; `202 > L = 200` (no chain at 202 to compare); no ack; not fenced; §11.3 `a = 18`, `h = 18` (no record has a frame), `r = 18` → case 2 (normal), `q_P ≤ L`. Emit `Read{after: 200, limit: 256}`; state `Reconciling`.
14. **Phone** → Relay read, 15 bytes: `0x03 ‖ u32 r2 ‖ u64 200 ‖ u16 256`.
15. **Relay** re-reads Phone's row (current, gen 1); selects seqs 201, 202 (`> 200`, `≥ purge_floor`); answers `0x83 ‖ u32 r2 ‖ page` with `head (202, C202)`, `floor 1`, `purge_floor 1`, `flags 0`, `n = 2`, entries `u64 201 ‖ i64 at ‖ u32 670 ‖ F201'`, `u64 202 ‖ i64 at ‖ u32 670 ‖ F202`; page body `71 + 2 × 690 = 1,451` bytes.
16. **Phone** §12.3: limits; clock; head; not truncated; contiguous from `L + 1`; dry run: `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ H(F201'))`, classify 201 with `M_200` → checks as scenario 1 step 27, `based_on (200, C200)` equals own → `Accept`; `C202`, classify 202 with `M_201`: author `MAC` current, `ctr 61 = 60 + 1`, epoch 1, `based_on (201, C201)` answered from this dry run (`chain_at`) → equal → `Accept`. Last entry seq = head seq and `C202` = head chain.
17. **Phone** `Persist[AppendLog[201, 202], SetMarker{201, 202, attempt}]`; on `Persisted` `L = K = 202`; board applies 201 (`create_task I41`, `Ctx{201, 1, T0−90min, 200}`; created = `"2026-11-22T08:30:00.000000000Z"`) and 202 (`rename I7`, `Ctx{202, 2, T0−30min, 201}`; name stamp `(202, 2)`; no notice: name stamp was ≤ 201 and the notice would concern Mac and Desk only); `B = 202`; `Persist[ClearMarker]` (no notice records: none concerns device 3; no own entry).
18. **Phone** run ended (`more` unset, head = `L`): rebase (§8.4): `RC` with `ctx {seq 203, device_no 3, at T0−2h, based_on min(200, 202) = 200}`: `I7` live, latest stamp `(202, 2)` is another device's change since 200 → applies with an overwrite notice (shown with the record, not recorded, ENG-SM-004 / §8.2); `RB` with `ctx {204, 3, T0−1h, 200}` → applies. Reconcile complete (`L ≥ q_P`), session `Live`.
19. **Phone** screen after the catch-up: "Pay rent" (from Desk) live; "Buy stamps" live (pending); done rows show "Call the plumber" (the renamed item, archived by the pending `complete`).
20. **Phone** send loop (§10.2): `R = [RC, RB]`, both `via append`, no frame, not held; epoch gate open (no own frame of an older epoch). Seal `RC` with `next_ctr = 1 + max(18, 18, w = 18) = 19`, then `RB` with 20 (§10.3, ENG-SEND-004). Draws: `RAND5[24]` nonce for `RC`, `RAND6[24]` nonce for `RB`.

    Plaintext of `RC` (512 bytes): `entry_id RAND3 ‖ kind 1 ‖ at_ms 1,795,334,400,000 ‖ based_on_seq 200 ‖ C200 ‖ payload_len 74 ‖ CMD_C ‖ 0x00 × 369` (`U = 143`, `P = 512`).
    Plaintext of `RB` (512 bytes): `entry_id RAND4 ‖ kind 1 ‖ at_ms 1,795,338,000,000 ‖ based_on_seq 200 ‖ C200 ‖ payload_len 79 ‖ CMD_B ‖ 0x00 × 364` (`U = 148`).
    (`based_on` is the enqueue head, never changed at sealing, ENG-ENQ-005.)

    Frames (each 670 bytes, layout of scenario 1 step 14):

    | Field | `F203` (`RC`) | `F204` (`RB`) |
    |---|---|---|
    | `magic` [4] | `"TDF1"` | `"TDF1"` |
    | `board_id` [16] | `BID` | `BID` |
    | `dev` [16] | `PHONE` | `PHONE` |
    | `ctr` [8] | 19 | 20 |
    | `len` [4] | 558 | 558 |
    | `ver`, `suite`, `epoch` [1, 1, 4] | 1, 1, 1 | 1, 1, 1 |
    | `nonce` [24] | `RAND5` | `RAND6` |
    | `ciphertext` [528] | `AEAD(K1; RAND5; ctx("entry") ‖ F203[0..48] ‖ 01 01 00000001; ptRC)` | `AEAD(K1; RAND6; ctx("entry") ‖ F204[0..48] ‖ 01 01 00000001; ptRB)` |
    | `sig` [64] | `SIG(Phone key; "taskdeck/v1/frame\0"; F203[0..606])` | `SIG(Phone key; "taskdeck/v1/frame\0"; F204[0..606])` |

21. **Phone** `Persist[Outbox[Put(RC sealed), Put(RB sealed)]]` (one `Persist`, D2); on `Persisted` `Append{req, [F203, F204]}`: one batch (2 ≤ 16 frames, ENG-SEND-001). Socket message `0x02 ‖ u32 r3 ‖ "TDA1" ‖ u8 2 ‖ u32 670 ‖ F203 ‖ u32 670 ‖ F204` = 1,358 bytes.
22. **Relay** re-read Phone row; P1–P7 for each frame; transaction: `F203` T2 `19 = 18 + 1` → T4 → T5 seq 203, `C203`; `F204` `20 = 19 + 1` → seq 204, `C204`; Phone row written once: `last_ctr 20`, `last_seq 204`, `app_count += 2` (RELAY-SEQ-007).
23. **Relay** → Phone `0x82 ‖ r3 ‖ "TDR1" ‖ 2 ‖ (0 ‖ 203 ‖ C203) ‖ (0 ‖ 204 ‖ C204)` (92 bytes); then push `0x84 ‖ page(head 204, C204, n 2, entries 203, 204)` to Phone, Desk and Mac (1,452 bytes).
24. **Phone** results → `RC.ack = (203, C203)`, `RB.ack = (204, C204)`, held for the next `Persist` (§10.4).
25. **Every device** (Phone, Desk, Mac) classifies 203 and 204 → `Accept` (`based_on (200, C200)` equals each one's `C200`; Phone's own-dev check finds its records for `ctr 19, 20`). Phone's `Persist` also carries the two ack `Put`s; echo check passes.
26. **Every device** applies 203: `complete I7`, `Ctx{seq 203, device_no 3, at T0−2h, based_on 200}`: live, task, archive cap → `archived_at = min(T0−2h, T0−2h + 86,400 s) = "2026-11-22T08:00:00.000000000Z"`; row built from the item (name "Call the plumber", outcome `finished`, default, omitted); item removed; overwrite notice on `item`: latest stamp `(202, 2)`, `202 > 200`, `2 ≠ 3`, `2 ≠ 0` (core.md §9.8, §10.1, §10.2).
27. **Every device** applies 204: `create_task P6`, mint check (3 = device_no, 6 > mark 5), item inserted, `created = "2026-11-22T09:00:00.000000000Z"`, mark of device 3 → 6.
28. **Phone** `seen` 203, 204 → `Drop(RAND3)`, `Drop(RAND4)`; `Persist[Notices[Add(notice N1)], Outbox[Drop(RAND3), Drop(RAND4)], ClearMarker]`; `Notify(NoticesAdded)` after it (ENG-REF-002). **Mac** `Persist[Notices[Add(N1)], ClearMarker]` (Mac's `device_no 2` is `other`). **Desk** `Persist[ClearMarker]` (device 1 neither author nor other).

    Notice `N1` (core.md §10.7), carried as `0x01 ‖ kind 1 ‖ u64 203 ‖ vb32(N1)` (device.md §6.4.5):

    ```json
    {"author":3,"group":"item","item":1099511627783,"kind":"overwrite","other":2,"other_seq":202,"seq":203}
    ```

    Archive row inserted on every device:

    ```json
    {"archived_at":"2026-11-22T08:00:00.000000000Z","created":"<I7 created>","id":1099511627783,"kind":"task","name":"Call the plumber"}
    ```

29. **Phone, Mac** show the notice ("changed on Mac since you last synced" on Phone; "Phone overwrote your change" on Mac). No refusal anywhere.

AMBIGUOUS: the notice for `complete` names `other`/`other_seq` from the item's *latest* stamp (core.md §10.2), but the text "changed on Mac" a user reads does not say which group Mac changed (the `group` field is `item`, the overwriting group); whether the UI must also show that the overwritten change was a rename is not stated (core.md §10.2, engine.md §30).
AMBIGUOUS: device.md §10.3 lets a token be stored "without `push`" only for `create`/bundle/seed tokens; for Phone's world token (from an earlier login) the stored `push` is assumed present, so no `caps` request; the world does not say and the spec does not say what is stored when a login answer was lost.

**State after the last step**

| Holder | Head | Counters | Epoch | Durable |
|---|---|---|---|---|
| Phone | `(204, C204)`, `B = 204` | Phone `last_ctr 20`, Desk 122, Mac 61 | 1 | log pages 201–202, 203–204; outbox: two `Put`, two sealed `Put`, two ack `Put`, two `Drop` (no live record); `notices.tdk` + `N1`; minter `next_item 7` |
| Desk | `(204, C204)` | same | 1 | log 203–204; no notice record |
| Mac | `(204, C204)` | same | 1 | log 203–204; `notices.tdk` + `N1` |
| Relay | `(204, C204)` | Phone row `last_ctr 20, last_seq 204` | — | `log` 1..204 |

"Done" on the renamed task: it is done on every device, archived under its new name "Call the plumber" with `archived_at` = `T0−2h`; Mac's rename survives in the row. User saw on Phone: before sync the old name in done; after sync "Pay rent" appears, the done row reads "Call the plumber", the overwrite notice is shown; Mac shows the same notice; Desk shows only the new state.
