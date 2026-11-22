# Scenario 1. First edit

Constants used below: `2^40 = 1,099,511,627,776`; `T0 = 1,795,341,600,000`; `ctx(x) = "taskdeck/v1/" ‖ x ‖ 0x00` (seal.md §2, §4.1): `ctx("frame")`, `ctx("entry")`, `ctx("chain")` are 18 bytes each, `ctx("file")` 17.

1. **User (Desk)**: types "Pay rent" in the quick-add field, no due date; desktop calls `Device::submit(Intent::QuickAdd { name: "Pay rent", deadline: None })` (device.md §17.4; core.md §12.3 `quick_add`).
2. **Desk (sync thread)**: `compile` against `Engine::view()` with `CompileCtx { now = T0 (Instant 2026-11-22T10:00:00.000000000Z), zone = ChronoZone(Local) (Europe/Helsinki), device_no = 1 }` (device.md §17.4, DEV-FACADE-001; core.md §13.1). Steps of row `quick_add` (core.md §13.4): trim (G1) → "Pay rent"; ≤ 200 scalars, not empty (G2); deadline none (G3 not applied); live items < 10,000 (cap); mint id: `n = max(minter.next_item, mark(view,1).item + 1) = 41` → `id = 1·2^40 + 41 = 1,099,511,627,817` (core.md §7.2, CORE-ID-004).
   AMBIGUOUS: the world says "Desk has minted item ids up to n = 40"; I read it as `mark(1).item = 40` and `minter.next_item ≤ 41` (core.md §7.2 leaves the minter's stored value to device.md §8.3, which the world does not fix).
   Command: undated → `time_importance = 1`, no `importance` (core.md §13.4 `quick_add`).
3. **Desk**: canonical log command (core.md §4.2, §9.4; keys sorted, `blocks` default `[]` omitted), 77 bytes:

   | Field (key order) | Bytes | Value |
   |---|---|---|
   | `{"id":1099511627817,` | 20 | |
   | `"name":"Pay rent",` | 18 | |
   | `"op":"create_task",` | 19 | |
   | `"time_importance":1}` | 20 | |
   | total | 77 | `{"id":1099511627817,"name":"Pay rent","op":"create_task","time_importance":1}` |

   Minter after compile = origin state (device.md §8.3), 18 bytes: `u16 1 ‖ u64 next_item = 42 ‖ u64 next_block = NB` (NB unchanged, symbolic).
4. **Desk → engine**: `Input { wall_ms = T0, mono_ms, Enqueue { ticket = t1, command, at_ms = T0, origin_state, hold = false } }` (engine.md §9.1; `submit` not `submit_named`, so `hold = false`, device.md §17.4).
5. **Desk (engine)**: §9.1 step 1 checks: joined (device_no 1); not revoked; clock not blocked (skew measured from Desk's socket welcomes, |skew| ≤ 1 h); `|at_ms − wall_ms| = 0 ≤ 60,000` (ENG-ENQ-006). Step 2: no held record. Step 3: trial apply on a copy of the pending view with `Ctx { seq = B + |U| + 1 = 201, device_no = 1, at_ms = T0, based_on = 200 }` → success (mint check `1 = ctx.device_no`, `41 > 40`; cap) (ENG-ENQ-001/002; core.md §9.4).
6. **Desk (engine)**: draws `RAND1[16]` = `entry_id` via `Sealer::new_entry_id` (ENG-ENQ-003, SEAL-PLAIN-002). Builds the record `{ entry_id = RAND1, order = ORD (one more than the largest order ever used), at_ms = T0, based_on = (200, C200), kind = cmd, body = Cmd(77 bytes), origin_state (18 B), via = Append, hold = false, reseals = 0 }` (engine.md §7.4, §9.1 step 4). Emits `Persist { id = p1, ops = [Outbox([Put(record)])] }`; applies the command to the pending view; `view_version += 1` (ENG-PEND-003).
7. **Desk (engine, same `handle`, §4.5 step 6 send loop)**: session `Live`, no stop, not fenced, not revoked, no pause, nothing outstanding (§10.1); `R = [record]`; it has no frame, epoch gate open (§24.2), not held → seal: `next_ctr = 1 + max(a = 121, h = 121, w = 121) = 122` (§10.3, ENG-SEND-006). `Sealer::seal(M_K, 122, entry)` draws `RAND2[24]` = frame nonce (SEAL-GEN-012). Emits `Persist { id = p2, ops = [Outbox([Put(record with ctr 122, frame, frame_hash = H(frame), epoch 1)])] }` (D2).
   AMBIGUOUS: engine.md §10.2 does not say whether the send loop may seal a record in the same `handle` call that enqueued it (before `p1`'s `Persisted`); I take that it does (nothing in §7.12 forbids it; the serial persist queue keeps `p1` before `p2`).
8. **Desk: the frame** (seal.md §8.1, §9.1, §10.1, §10.2).

   Plaintext (`U = 69 + 77 = 146`, `P(146) = 512`):

   | Offset | Field | Width | Value |
   |---|---|---|---|
   | 0 | `entry_id` | 16 | RAND1 |
   | 16 | `kind` | 1 | `0x01` (cmd) |
   | 17 | `at_ms` | 8 | i64 1,795,341,600,000 |
   | 25 | `based_on_seq` | 8 | u64 200 |
   | 33 | `based_on_chain` | 32 | C200 |
   | 65 | `payload_len` | 4 | u32 77 |
   | 69 | `payload` | 77 | the command of step 3 |
   | 146 | `padding` | 366 | `0x00` × 366 |
   | | total | 512 | |

   Frame (670 bytes, the minimum):

   | Offset | Field | Width | Value |
   |---|---|---|---|
   | 0 | `magic` | 4 | `"TDF1"` |
   | 4 | `board_id` | 16 | BID |
   | 20 | `dev` | 16 | DESK |
   | 36 | `ctr` | 8 | u64 122 |
   | 44 | `len` | 4 | u32 558 |
   | 48 | `ver` | 1 | 1 |
   | 49 | `suite` | 1 | 1 |
   | 50 | `epoch` | 4 | u32 1 |
   | 54 | `nonce` | 24 | RAND2 |
   | 78 | `ciphertext` | 528 | `AEAD(K_entry,1; RAND2; AD; plaintext)` (512 + 16 tag) |
   | 606 | `sig` | 64 | `SIG(DESK_SIGN; "taskdeck/v1/frame\0"; frame[0..606])` |

   - `AD` (72 bytes) = `ctx("entry")[18] ‖ frame[0..48] ‖ u8 1 ‖ u8 1 ‖ u32 1` (SEAL-ENV-002).
   - `K_entry,1 = HKDF(salt = BID, ikm = R_1, info = ctx("key-entry") ‖ u32 1, L = 32)` (seal.md §6.5).
   - signed bytes = `ctx("frame")[18] ‖ frame[0..606]` = 624 bytes (SEAL-FRAME-002).
   - `frame_hash = H(frame[0..670])` (§8.2).
9. **Desk (runtime)**: performs `p1`: appends to `outbox.tdo` one sealed record (device.md §7.2 append, §6.4.4): plaintext `u16 1 ‖ 0x01 ‖ vb32(record JSON)`; sealed record `u32 ct_len ‖ RAND3[24] (file nonce) ‖ AEAD(K_file(5); RAND3; ctx("file") ‖ header[55] ‖ u64 i; plaintext)`; `sync(outbox.tdo)`. Then `Persisted{p1}` on the local queue.
   Record JSON (device.md §6.5; canonical key order), `p1`: `{"at_ms":1795341600000,"based_on":{"chain":"<C200 hex>","seq":200},"body":{"cmd":"<base64 of the 77 bytes>"},"entry_id":"<RAND1 hex>","hold":false,"kind":"cmd","order":ORD,"origin_state":"<base64 of 18 bytes>","reseals":0,"via":"append"}`; `p2` adds `"ctr":122`, `"epoch":1`, `"frame":"<base64 670 B>"`, `"frame_hash":"<hex>"` at their sorted places (after `body`, `entry_id`, `epoch` … `frame`, `frame_hash`).
   AMBIGUOUS: the record JSON writes `hold:false` and `reseals:0` (device.md §6.5 "no other field is ever absent") although the canonical encoder omits defaults only by a schema (core.md §4.3), and `OutboxRecord` has no schema table with defaults; I write them.
   AMBIGUOUS: RAND numbering: the frame nonce (RAND2) is drawn inside `handle` before the runtime draws the file nonces of `p1` (RAND3) and `p2` (RAND4); device.md §7.3 does not order the store's draws against the sealer's, I take execution order.
10. **Desk (engine)** on `Persisted{p1}`: `Notify(Enqueued { ticket = t1, entry_id = RAND1, provisional_seq = 201 })` (ENG-ENQ-004, D1) → `submit` returns `Submitted { reply.id = 1,099,511,627,817, entry_id, provisional_seq 201, view_version }`; UI shows "Pay rent" in the pending view, unmarked.
11. **Desk (runtime)**: performs `p2` (append to `outbox.tdo`, RAND4 file nonce, `sync`) → `Persisted{p2}`.
12. **Desk (engine)** on `Persisted{p2}`: `Append { req = r1, frames = [frame] }` (D2, ENG-SEND-003).
13. **Desk → Relay (socket)**: append message, 684 bytes (relay.md §11.3, §9.2; device.md §10.5):

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `0x02` |
    | `req` | 4 | u32 r1' (transport's socket id mapped to r1) |
    | `magic` | 4 | `"TDA1"` |
    | `n` | 1 | 1 |
    | `len_0` | 4 | u32 670 |
    | `frame_0` | 670 | the frame |
14. **Relay (object)**: reads clock `now`; re-reads Desk's device row: role 1, state 1, `gen = 1` = attachment gen (RELAY-SOCK-007). Batch layout ok (§9.2). P1: 670 in range, `"TDF1"`, `len 558 = 670 − 112`; P2: board BID; P3: ctr 122 in range; P4: row DESK exists; P5: dev = token's dev; P6: state 1; P7: `verifyStrict(DESK_SIGN, ctx("frame") ‖ frame[0..606], frame[606..670])` (§3.4, RELAY-SEQ-006).
15. **Relay**: one transaction (§9.4): T1 row unchanged; T2 `122 = 121 + 1`; T4 `app_count < 3,000`; T5 `seq = 201`, `at = max(now, at(200))`, `hash = H(frame)`, `chain = C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ H(frame))` (90-byte input, seal.md §15.1); insert `log(201, DESK, 122, hash, C201, at, frame)`; DESK row `last_ctr 122, last_seq 201, app_day today, app_count + 1`. Rows written: 3 (relay.md §16.1).
16. **Relay → Desk**: `result`, 51 bytes, before any push (RELAY-SOCK-012):

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `0x82` |
    | `req` | 4 | r1' |
    | `magic` | 4 | `"TDR1"` |
    | `n` | 1 | 1 |
    | status | 1 | 0 (`new`) |
    | `seq` | 8 | u64 201 |
    | `chain` | 32 | C201 |
17. **Relay → Desk and Mac** (every ready socket, RELAY-SOCK-008): `push`, 762 bytes:

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
    | entry `at` | 8 | the stored `at` |
    | entry `flen` | 4 | 670 |
    | entry `frame` | 670 | |

    AMBIGUOUS: `floor` and `purge_floor` = 1 assumes no compaction ever ran (a 200-entry board cannot have compacted: RELAY-COMPACT-007); the world does not state the relay's floors.
18. **Desk (engine)** on `AppendResults { r1, [New { 201, C201 }] }`: `ack = (201, C201)`; the `Put` is held for the next `Persist` (§10.4); `201 > L = 200`, echo check deferred.
19. **Desk (engine)** on `Page { req: None }` (push): §12.3 step 1 limits; step 2 `skew = wall_ms − relay_now`; step 3 head 201 ≥ L, no ack above it; step 5 first entry is `L + 1`; step 6 dry run: `C201' = chain_step(C200, 201, frame)`; `classify` with `M_200`, `own_sealed(122) = frame_hash` → no `own_dev_foreign`; steps 1–17 pass (based_on 200 < 201, `C_200` held and equal) → `Accept` (cmd); step 7 `C201' = head_chain`.
20. **Desk**: `Persist { p3, [AppendLog([{201, C201, τ201 = max(τ200, T0), frame}]), SetMarker{201, 201, attempt}, Outbox([Put(record with ack (201, C201))])] }` (§12.3 step 8; the ack `Put` last). Runtime appends to the current log segment (one page record `u16 1 ‖ u64 201 ‖ C201 ‖ i64 τ201 ‖ u32 670 ‖ frame`, device.md §6.4.1), replaces `marker.tdk`, appends to `outbox.tdo`; no history record (not ctl).
21. **Desk** on `Persisted{p3}`: `L = K = 201`, `M_201 = M_200` with DESK `last_ctr 122`, `clock_ms = max(τ, T0)`; board applies 201: not skipped; `parse` → `create_task`; `apply(Ctx { seq 201, device_no 1, at = T0, based_on 200 })`: mint check, cap, insert item, raise mark 1 to 41, no notice (core.md §9.4); own entry → `seen = 201`, `Drop(RAND1)`; `B = 201` (201 not ≡ 0 mod 64: no hash). Echo check: frame hash and `C201` equal (ENG-ECHO-001). `Persist { p4, [Outbox([Drop(RAND1)]), ClearMarker] }` (ENG-RECV-006). Run ended (`more` 0, `L` = head) → rebase (§8.4): pending = confirmed. `Notify(ViewChanged)`.
22. **Mac (engine)**, on the same push: steps of 19 with its own state: dry run `C201 = chain_step(C200, 201, frame)`; `classify(M_200)`: 1 format; 2 board; 3 DESK current; 4 signature; 5 `122 = 121 + 1`; 6 not own dev; 7 author current; 8 (1,1) allowed by rules 1; 9 epoch 1 = E; 10 AEAD opens under `K_entry,1` with the AD of step 8; 11 unpadding (L 512 ≥ 69, U 146 ≤ 512, P(146) = 512, 366 zero bytes, kind 1, at_ms in range); 12 `b = 200 < 201`, `C_200` held, equal; 13 cmd by a current device, current epoch; 14 n/a; 15–16 n/a (cmd); 17 `Accept` (seal.md §14.2). Head chain equals `C201`.
23. **Mac**: `Persist { [AppendLog([201, C201, τ201, frame]), SetMarker{201,201,a}] }`; on `Persisted`: commit `M_201`; apply 201 with `Ctx { 201, device_no 1 (DESK's in M_200), at = T0, based_on 200 }` → item inserted; no notice for Mac; `Persist { [ClearMarker] }`; rebase; `Notify(ViewChanged)`; Mac's UI shows "Pay rent".

**State after.**

| | Desk | Mac | Relay |
|---|---|---|---|
| head | `(201, C201)`, `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ H(frame))` | `(201, C201)` | `(201, C201)` |
| counters | own `last_ctr` 122 | DESK 122 in `M_201` | DESK `last_ctr 122, last_seq 201`, `app_count +1` |
| epoch | 1 | 1 | — |
| outbox | empty (Put, Put, Put, Drop in `outbox.tdo`) | empty | — |
| files | log segment + page 201; `marker.tdk` removed; `outbox.tdo` +4 records | log + page 201 | `log` row 201 |
| board | item `{"created":"2026-11-22T10:00:00.000000000Z","id":1099511627817,"kind":"task","name":"Pay rent","stamps":{"estimate":[201,1],"levels":[201,1],"name":[201,1],"repeat":[201,1],"time":[201,1]},"time_importance":1}`; mark 1 `item = 41` | same | — |

User saw "Pay rent" on Desk at once (pending, provisional seq 201), then confirmed; on Mac after the push. No notice, no refusal.

# Scenario 2. Offline edit replayed

`T0 − 2 h = 1,795,334,400,000`; `T0 − 1 h = 1,795,338,000,000`; `3·2^40 = 3,298,534,883,328`.

1. **User (Phone, T0 − 2 h, no network)**: taps Done on "Call plumber"; the page posts `POST /api/command` `{"op":"complete","id":1099511627783,"at":"2026-11-22T08:00:00Z"}` (bridge.md §6.3, wire time `Exact`).
   AMBIGUOUS: "Phone has been closed since seq 200" — I read "no relay session since 200, the app opened offline for both edits", since an edit needs the app open.
2. **Phone (bridge-io → facade)**: verb `complete` allowed; `submit_json` → `parse_intent` (core.md §12.2) → `compile` with `now = T0 − 2 h`, zone = the shell's `TableZone` (Helsinki), `device_no 3`: missing? no; kind task; `at` Exact → 08:00:00Z; archive < 200,000; `at' = min(at, now + 1 day) = at` (core.md §13.4 `complete`). Command (74 bytes): `{"at":"2026-11-22T08:00:00.000000000Z","id":1099511627783,"op":"complete"}` (39 + 19 + 16). Minter unchanged (no mint).
3. **Phone (engine)**: `Enqueue { at_ms = T0 − 2 h, hold false }`; checks (joined, not revoked, clock: no valid measurement → no block, ENG-CLOCK-002); trial apply `Ctx { 201, 3, T0 − 2 h, 200 }` → ok; `RAND1[16]` entry_id E1; record `{E1, order o1, at_ms T0 − 2 h, based_on (200, C200), cmd}`; `Persist(Outbox[Put])` → `outbox.tdo` append (file nonce RAND2); `Notify(Enqueued{provisional 201})`; reply `{"ok":true,"version":v}` (bridge.md BRIDGE-VERB-002). No seal: session not `Live` (ENG-SESS-001).
4. **User (Phone, T0 − 1 h)**: quick add "Buy stamps"; page posts `{"op":"quick_add","name":"Buy stamps","deadline":null}` (bridge.md §6.3).
5. **Phone**: compile: mint `n = max(next_item, mark(3).item + 1) = 6` → id `3·2^40 + 6 = 3,298,534,883,334`; command (79 bytes) `{"id":3298534883334,"name":"Buy stamps","op":"create_task","time_importance":1}`; origin state `u16 3 ‖ u64 7 ‖ u64 NB`. `Enqueue`, trial apply `Ctx { 202, 3, T0 − 1 h, 200 }` → ok; `RAND3[16]` E2; record `{E2, o2 > o1, at_ms T0 − 1 h, based_on (200, C200)}`; `Persist(Outbox[Put])` (RAND4 file nonce); `Enqueued{provisional 202}`.
   AMBIGUOUS: as in scenario 1, "minted up to n = 5" read as `mark(3).item = 5`.
6. **Phone screen before (offline)**: pending view = confirmed state at 200 + E1 (Call plumber archived: gone from the day, in Done as "Call plumber", finished 08:00 UTC = 10:00 Helsinki) + E2 ("Buy stamps", horizon 1). No "Pay rent". Sync line: unreachable since …, "2 waiting" (`queued 2`, bridge.md §5.4 sync data).
7. **User (T0)**: opens the app with network. Shell: `set_zone`, `set_foreground(true)` → `notify_resume` (bridge.md §14.1; device.md §10.9). If the process was killed meanwhile: `Device::open`, `Start` from the files, mode Synced (device.md §5.4); holds end at `Start`.
8. **Phone (engine)**: `Offline` → `Connect { relay.example, door, 1 }` (engine.md §11.1); no backoff (first after open/resume, device.md §10.7).
9. **Phone (transport)**: stored token gen 1 with `push = true`, not revoked → no login (device.md §10.3); foreground and push → socket `GET wss://relay.example/v1/b/<BID hex>/socket`, `Authorization: Bearer TD1.<92>`, `User-Agent: TaskDeck-Sync/1`. **Relay**: gate (token MAC, role 1), object: row PHONE role 1 state 1 gen 1, board open → `acceptWebSocket`, attachment `{dev, gen 1, features 0, ready false}`, 101.
10. **Phone → Relay**: hello, 21 bytes: `0x01 ‖ session[16] ‖ u32 0` (session drawn at open, device.md §10.4).
11. **Relay → Phone**: welcome, 93 bytes (relay.md §11.3): `0x81 ‖ u64 202 ‖ C202 ‖ u64 last_ctr 18 ‖ u64 last_seq q18 ‖ i64 relay_now ‖ u64 floor 1 ‖ u64 purge_floor 1 ‖ u8 1 ‖ u8 0x01 ‖ u32 appends_left 3,000 ‖ door[5] ‖ u8 0`. (`q18` ≤ 200: the seq of PHONE's 18th frame.)
12. **Phone (engine)**: `Connected{push true}`, then `Welcome`: skew measured; `202 ≥ L = 200`, no fork check possible above L; no ack; not fenced; reconcile: `a = 18`, `h = 18`, `r = 18` → normal, `q18 ≤ L` (§11.3 step 2). Head above `L` → `Read { r, after 200, limit 256 }`, state `Reconciling`.
13. **Phone → Relay**: read, 15 bytes: `0x03 ‖ u32 r' ‖ u64 200 ‖ u16 256`.
14. **Relay → Phone**: page, `0x83 ‖ u32 r' ‖ "TDP1" ‖ 202 ‖ C202 ‖ relay_now ‖ 1 ‖ 1 ‖ flags 0 ‖ u16 2 ‖ (u64 201 ‖ i64 at ‖ u32 f201 ‖ frame201) ‖ (u64 202 ‖ i64 at ‖ u32 f202 ‖ frame202)`.
15. **Phone (engine)**, §12.3: dry run 201: `C201 = chain_step(C200, 201, f201)`, `classify(M_200)` → Accept (DESK ctr 122, epoch 1, based_on (200, C200) checked); 202: `C202 = chain_step(C201, 202, f202)`, `classify(M_201)` → Accept (MAC ctr 61, `based_on_seq = 201` checked against `C201` from the dry run, ENG-SEALER-006); head chain `C202` equal.
    AMBIGUOUS: Mac's rename has `based_on_seq 201` (Mac's head at T0 − 30 min is 201, ENG-ENQ-005); the world does not say, I take it.
16. **Phone**: `Persist { AppendLog([201, C201, τ201, f201], [202, C202, τ202, f202]), SetMarker{201, 202, a} }`; commit; apply 201: Desk's `create_task` "Pay rent" `Ctx{201, 1, T0 − 90 min, 200}`; apply 202: `rename{id 2^40+7, "Call the plumber"}` `Ctx{202, 2, T0 − 30 min, 201}`: item live, `name` stamp was written at a seq ≤ 200 → not "another device's change since 201" → no notice; `name` stamp `(202, 2)`; `Persist{ClearMarker}`.
17. **Phone**: run ended (more 0, L = head) → rebase (§8.4): `U = [E1, E2]`; E1 `Ctx { seq 203, device_no 3, at T0 − 2 h, based_on min(200, 202) = 200 }`: complete succeeds, archive row named "Call the plumber", an overwrite notice (latest stamp `(202, 2)`, 202 > 200, 2 ≠ 3) shown with the pending row only, not recorded (ENG-SM-004); E2 `Ctx { 204, 3, T0 − 1 h, 200 }` ok. Reconcile complete (`q18 ≤ L`, `L` = head) → `Live`.
18. **Phone screen after catch-up**: "Pay rent" appears; Done shows "Call the plumber" (finished 10:00 local); "Buy stamps"; nothing doomed.
19. **Phone (engine, send loop)**: `R = [E1, E2]`, none sealed; seal E1 `ctr = 1 + max(18, 18, 18) = 19`, E2 `ctr 20` (§10.3), both with `based_on (200, C200)` and their own `at_ms` (ENG-ENQ-005); draws `RAND5[24]` (E1 nonce), `RAND6[24]` (E2 nonce); one `Persist(Outbox[Put E1 framed, Put E2 framed])` (file nonce RAND7); then `Append { [f_E1, f_E2] }`.
    AMBIGUOUS: a beat duty may be due (last beat > 24 h ago, engine.md §23.1); duties (step 5) run before the send loop (step 6), so a beat record would join this batch as ctr 21. The world does not give `last_beat_ms`; I assume no beat is due.
20. **Frames**: E1: plaintext `E1 ‖ 0x01 ‖ i64 1,795,334,400,000 ‖ u64 200 ‖ C200 ‖ u32 74 ‖ payload ‖ 0x00 × 369` (U 143 → 512); frame `"TDF1" ‖ BID ‖ PHONE ‖ u64 19 ‖ u32 558 ‖ 1 ‖ 1 ‖ u32 1 ‖ RAND5 ‖ AEAD(K_entry,1; RAND5; ctx("entry") ‖ header ‖ 1 ‖ 1 ‖ u32 1; pt) ‖ SIG(PHONE_SIGN; "taskdeck/v1/frame\0"; frame[0..606])`, 670 bytes. E2: same layout, `ctr 20`, `at_ms 1,795,338,000,000`, `payload_len 79`, padding 364, nonce RAND6, 670 bytes.
21. **Phone → Relay**: append, 1,358 bytes: `0x02 ‖ u32 r' ‖ "TDA1" ‖ u8 2 ‖ u32 670 ‖ f_E1 ‖ u32 670 ‖ f_E2`.
22. **Relay**: P1–P7 for both; transaction: E1 `19 = 18 + 1` → seq 203, `C203 = H(ctx("chain") ‖ C202 ‖ u64 203 ‖ H(f_E1))`; E2 `20 = 19 + 1` → seq 204, `C204`; PHONE row written once: `last_ctr 20, last_seq 204, app_count + 2`. Rows written 5. Result `0x82 ‖ r' ‖ "TDR1" ‖ 2 ‖ (0 ‖ 203 ‖ C203) ‖ (0 ‖ 204 ‖ C204)`, then push `(203, 204)` to Phone, Desk, Mac.
23. **Phone**: acks; push processed (classify 203: PHONE ctr 19, epoch 1, based_on 200 checked → Accept; 204 likewise); apply 203: `complete` `Ctx{203, 3, T0 − 2 h, 200}`: `archived_at = min(08:00:00Z, T0 − 2 h + 1 day) = 2026-11-22T08:00:00.000000000Z`, row `outcome` finished with name "Call the plumber", item removed; overwrite notice on group `item`. Apply 204: item `3·2^40 + 6` created, mark 3 → 6. `seen`, `Drop` E1, E2; `Persist { Notices([Add{kind 1, seq 203, record}]), Outbox([Drop E1, Drop E2]), ClearMarker }` (ENG-RECV-006).
    Notice record (core.md §10.7, canonical): `{"author":3,"group":"item","item":1099511627783,"kind":"overwrite","other":2,"other_seq":202,"seq":203}`; on disk `notices.tdk` op `0x01 ‖ u8 1 ‖ u64 203 ‖ vb32(record)` (device.md §6.4.5).
24. **Desk**: applies 203 and 204 the same way; the notice concerns devices 3 and 2 only → not recorded on Desk (ENG-REF-003). **Mac**: applies them, records the same notice (`other = 2`) in its `notices.tdk`; shows "changed on Phone since you last synced" for "Call the plumber" (core.md §10.2).
25. **"Done" on the renamed task**: on every device the task ends archived, finished, `archived_at` 08:00:00Z, with the name "Call the plumber" (the rename at 202 applied first, the complete at 203 copies the current name, core.md §9.8); not refused, not a benign repeat. Phone's bridge pushes a `notices` event (bridge.md §5.4) with the record; no refusal anywhere.

**State after.**

| | Phone | Desk | Mac | Relay |
|---|---|---|---|---|
| head | `(204, C204)` | `(204, C204)` | `(204, C204)` | `(204, C204)` |
| counters | PHONE 20 | DESK 122, MAC 61, PHONE 20 | same | PHONE `last_ctr 20, last_seq 204` |
| epoch | 1 | 1 | 1 | |
| outbox | empty | empty | empty | |
| notices | 1 (seq 203) | 0 | 1 (seq 203) | |

`C201 … C204` each `H(ctx("chain") ‖ C_{s−1} ‖ u64 s ‖ H(frame_s))`.
