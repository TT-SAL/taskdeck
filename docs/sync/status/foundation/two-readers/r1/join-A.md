# join — reader A

Symbols used in both scenarios (all from the world or derived as cited):

- `ctx(n)` = ASCII `taskdeck/v1/` ‖ `n` ‖ `0x00` (README §5.2); its width is 13 + |n| bytes. `sk(X)` is the private key whose public key is `X`.
- `DEV_R = H(ctx("recovery-dev") ‖ BID ‖ RSIGN)[0..16]` (seal §6.3). `DOOR = MAC(TOKEN_KEY; ctx("door") ‖ BID)[0..5]` (relay §5.3).
- `R1` the epoch-1 root; `c1 = H(ctx("epoch") ‖ BID ‖ u32 1 ‖ R1)`; `K_entry,1 = HKDF(salt BID; ikm R1; info ctx("key-entry") ‖ u32 1; 32)`; `K_snap,1 = HKDF(BID; R1; ctx("key-snap") ‖ u32 1; 32)` (seal §6.5).
- `F_s` the frame at seq `s`, `FH_s = H(F_s)`, `C_s` the chain value, `τ_s` the log clock after `s`, `M_s` the membership after `s`, `SH_s` the state hash at `s`.
- `ORIGIN` = `"https://relay.example"` (21 bytes; seal §5.4, port 443 never written). Device names "Desk", "Mac", "Phone" (assumed; the world does not name them).
- `a_M`, `a_P`: seqs of the accepted `DeviceAdded` of Mac and Phone (below 192; the world does not fix them). `ctl_seqs` of `M_200` = `[1, a_M, a_P]` (no membership change other than these).
- Relay JSON responses keep the field order of their relay.md tables (relay §4.2); payloads inside frames and the bundle use the canonical encoding (sorted keys, core §4.2).

---

## Scenario 3 — Pairing

Extra symbols: `t1 < t2 < t3 < …` are wall-clock instants after `T0` at which Desk/Tab compose entries (symbolic). `TOK_X` is the stored token of device X. `nowR` is the relay clock at the request being handled.

### Steps

1. **User** (Desk): opens the device list and chooses "Add a device"; the desktop runs device authentication (Windows Hello or Touch ID) immediately before the call, then calls `pair_start()` at `T0`. (seal §17.1 step 1; DEV-FACADE-003; device §17.10, §18.3)

2. **Desk** (facade/engine): check that Desk may pair: joined, `current` in `M_K`, not halted, not stopped, session `Live`, no pairing running; else `engine/not_eligible`, `engine/halted`, `engine/pair_busy`. All pass. (device §17.10; engine §28.4)

3. **Desk** (seal `Existing` machine): event `Start{membership M_200, head (200, C200), relay origin ORIGIN}`, guard "E current and unhalted" holds. Draws, in this order: **RAND1[10]** = `s` (the pairing secret), **RAND2[32]** = `esk_E` (E's ephemeral X25519 secret), **RAND3[32]** = `r_E` (the commitment opening). (seal §17.10 row `Idle`, §17.2, §17.5)
   AMBIGUOUS: seal §17.1/§17.10 say "draw `esk_E`" but do not say how the ephemeral X25519 pair is made (32 raw bytes used as an RFC 7748 scalar, or HPKE `DeriveKeyPair` as §6.4 does for long-term keys); taken: 32 raw bytes, `epk_E = X25519(esk_E, 9)`.

4. **Desk**: compute the code's keys and the code (seal §17.2, §17.3, §17.5):
   - `ticket_key = HKDF(salt BID; ikm s; info ctx("pair-ticket"); 32)`
   - `tid = H(ctx("pair-tid") ‖ ticket_key)[0..16]`
   - `mac_key = HKDF(salt BID; ikm s; info ctx("pair-mac"); 32)`
   - `commit = H(ctx("pair-commit") ‖ epk_E ‖ r_E)`
   - `check` = first 10 bits of `H(ctx("pair-code") ‖ s)`
   - code = Crockford base32 of `s` (16 chars) ‖ Crockford of `check` (2 chars, 5 bits each); display `XXXX-XXXX-XXXX-XXXX-CC`.

   | Hash/HKDF input | Field | Width | Value |
   |---|---|---|---|
   | ticket_key info | context | 24 | `ctx("pair-ticket")` |
   | tid input (53) | context | 21 | `ctx("pair-tid")` |
   | | `ticket_key` | 32 | as above |
   | mac_key info | context | 21 | `ctx("pair-mac")` |
   | commit input (88) | context | 24 | `ctx("pair-commit")` |
   | | `epk_E` | 32 | |
   | | `r_E` | 32 | RAND3 |
   | check input (32) | context | 22 | `ctx("pair-code")` |
   | | `s` | 10 | RAND1 |

5. **Desk**: build slot A (84 bytes). (seal §17.4, §17.5; SEAL-PAIR-002)

   | Field | Width | Value |
   |---|---|---|
   | `label` | 1 | `0x41` |
   | `role` | 1 | `0x45` |
   | `body_len` | 2 | 48 |
   | body: `dev_E` | 16 | `DESK` |
   | body: `commit` | 32 | `commit` |
   | `mac` | 32 | `MAC(mac_key; ctx("pair") ‖ 0x41 ‖ 0x45 ‖ tid[16] ‖ u16 48 ‖ body[48])` (MAC input 17+1+1+16+2+48 = 85 bytes) |

6. **Desk → Relay**: `RegisterTicket{tid, slot A}` = `POST /v1/b/<BID hex>/pair` (`pair-open`), `Authorization: Bearer TOK_DESK`, `User-Agent: TaskDeck-Sync/1`, `Content-Type: application/octet-stream`, `Content-Length: 100`. (device §18.3, §10.2; relay §10.1, §10.8)

   | Body field | Width | Value |
   |---|---|---|
   | `tid` | 16 | `tid` |
   | slot A | 84 | step 5 |

7. **Relay** (gate, relay §6.2 steps 0–10): route `pair-open`, no query, one `Content-Length` = fixed 100, token form `Bearer TD1.…`, token MAC verifies and `board_id = BID`, role 1 accepted, body complete within `10,000 + ceil(100 × 1,000 / 65,536)` ms; one object call.

8. **Relay** (object): token row `DESK` role 1, state 1, `gen = 1` ✓ (relay §8.2); board `open`; slot A is `0x41 ‖ 0x45 ‖ u16 48 ‖ body[48] ‖ mac[32]`; delete expired tickets; no ticket `tid`; fewer than 4 tickets. Insert `tickets(tid, creator DESK, expires = nowR + 600,000, tries 0, a = slot A, b,c,d NULL)`; answer `200 {"expires": nowR + 600,000}` (≈ 1,795,342,200,000). (relay §10.8 `pair-open`)

9. **Desk**: `ShowCode{code}`: `pair_start` returns `PairSheet{address_display, code_display}`; the desktop shows the code `XXXX-XXXX-XXXX-XXXX-CC` and the relay address `relay.example/` + Crockford(`BID ‖ DOOR`) (34 chars, 2 zero pad bits) in groups of 4 joined by `-`, last group 2, on the pairing sheet, main window excluded from capture, never on the wall layout. (seal §17.1 step 2; relay §5.4; device §18.3; DEV-PAIR-001)

10. **Desk**: wait for slot B: on the socket `slot` notice, or every 2,000 ms `GET …/pair/<tid hex>/b` (`404 relay/slot_empty` = wait). (device §18.3)

11. **User** (Tab): fresh install, opens TaskDeck (mode `LocalOnly`, page `hello.mode = "local"`), opens the Pair screen, types or pastes the relay address, types the 18-character code into the code field and the name "Tab", taps Join; the screen calls `callSecret("join", {"address": <text>, "device_name": "Tab"}, <code as UTF-8 bytes>)` and zeroes the arrays. (bridge §5.8, §13.1, §13.3, §11.4; device §17.10 `join`)

12. **Tab** (device `join`): checks the lock, the cloud check, a key store (Android Keystore), mode `LocalOnly`; parses the address by relay §5.4 steps 1–6 → host `relay.example`, `board_id = BID`, `door = DOOR`; normalises the code (seal §5.2), requires 18 characters, decodes `s` (80 bits) and the 10 check bits, recomputes `H(ctx("pair-code") ‖ s)`'s first 10 bits: equal. No request has been sent. (device §18.3; RELAY-ADDR-002; SEAL-CODE-002; DEV-PAIR-003)

13. **Tab** (seal `New` machine): `CodeEntered{code, host, BID, DOOR}` → derive `ticket_key`, `tid`, `mac_key` exactly as step 4 → effect `FetchSlot{tid, ticket_key, A}`. `start` = this event's monotonic time. (seal §17.10, §17.3)

14. **Tab → Relay**: `GET /v1/b/<BID hex>/pair/<tid hex>/a`, `Authorization: Door <DOOR as 10 hex>`, `X-TD-Proof: <ticket_key as 64 hex>`, no body. (device §18.3, §10.2; relay §4.1, §5.6, §10.1)

15. **Relay** (gate): Door form accepted for `slot-get` label `a`; door recomputed `MAC(TOKEN_KEY; ctx("door") ‖ BID)[0..5]` and compared in constant time; proof: `H(ctx("pair-tid") ‖ ticket_key)[0..16] = tid` in constant time (else `403 relay/bad_proof`). Object: ticket exists, `expires > nowR`; `a` present → `200`, `application/octet-stream`, the 84 bytes of slot A; no row written. (relay §6.2 step 7, §10.8; SEAL-PAIR-015; RELAY-PAIR-006)

16. **Tab**: verify slot A before reading its body: total 84 = 36 + 48, label `0x41`, role `0x45`, MAC recomputed over `ctx("pair") ‖ 0x41 ‖ 0x45 ‖ tid ‖ u16 48 ‖ body` and compared in constant time. Reads `dev_E = DESK`, `commit`. (SEAL-PAIR-003; seal §17.10 `AwaitA` guard)

17. **Tab**: generate long-term keys, then the ephemeral key: **RAND4[16]** = `TAB` (its `dev`), **RAND5[32]** = Ed25519 private key (public `TAB_SIGN`), **RAND6[32]** = HPKE `DeriveKeyPair` ikm (public `TAB_HPKE`), all from `OsEntropy`, held zeroizing in memory; **RAND7[32]** = `esk_N`, `epk_N = X25519(esk_N, 9)`. (seal §6.4, §17.1 step 4, §17.10 `AwaitA`; SEAL-GEN-005; device §18.3)
    AMBIGUOUS: the draw order of `dev`, the signing key and the X25519 ikm (seal §6.4 lists them in a table, not as an order) and how `esk_N` is made (as step 3); taken: table order, then `esk_N` as 32 raw bytes.

18. **Tab**: build slot B (157 bytes) and `PostSlot{B}`: `POST /v1/b/<BID>/pair/<tid>/b`, `Door` + `X-TD-Proof`, `Content-Length: 157`; then `FetchSlot{C}`: `GET …/c` every 2,000 ms. (seal §17.4; device §18.3)

    | Field | Width | Value |
    |---|---|---|
    | `label` | 1 | `0x42` |
    | `role` | 1 | `0x4E` |
    | `body_len` | 2 | 121 (= 114 + 4×1 + 3) |
    | `dev_N` | 16 | `TAB` |
    | `sign_pk_N` | 32 | `TAB_SIGN` |
    | `hpke_pk_N` | 32 | `TAB_HPKE` |
    | `epk_N` | 32 | `epk_N` |
    | `n_rules` | 1 | 1 |
    | rules | 4 | u32 1 |
    | `vb8(name_N)` | 1 + 3 | `0x03` ‖ `"Tab"` |
    | `mac` | 32 | `MAC(mac_key; ctx("pair") ‖ 0x42 ‖ 0x4E ‖ tid ‖ u16 121 ‖ body[121])` |

19. **Relay** (`slot-post` b): gate as step 15 (cap 16,076 ≥ 157). Object, in order: board `open`; ticket unexpired; `tries` 0 → 1 (written even if a later step refused); layout B: `0x42`, `0x4E`, `body_len` 121 in 119–242, total 157 = 36 + 121; not a repeat; `a` present, `b` absent → store `b`. After commit, send to every ready socket of `DESK`: `slot` message `0x87 ‖ tid[16] ‖ 0x42` (18 bytes). Answer `200 {}`. (relay §10.8 steps 1–7, §11.3, §11.4 "Slot notice")

20. **Desk → Relay**: on the slot notice, `GET /v1/b/<BID>/pair/<tid>/b` with `Bearer TOK_DESK`; relay: token's `dev` = ticket `creator` → `200` with the 157 bytes. (relay §10.8 `slot-get`; device §18.3)

21. **Desk**: E's checks on slot B, in order: (1) MAC, label B, role N; (2) layout and limits (157 = 36 + 121; `n_rules` 1 in 1–16, list ascending; name "Tab" 3 bytes, no control characters, seal §5.5); (3) `TAB`, `TAB_SIGN`, `TAB_HPKE` differ from every `dev`, `dev_R`, `sign_pk`, `hpke_pk` in `M_200`; (4) N's rules `[1]` contain the active rules 1; (5) 3 current devices < 16 and `next_device_no = 4 ≤ 8191`. All pass. (seal §17.6)

22. **Desk**: build slot C (100 bytes) and `PostSlot{C}`: `POST /v1/b/<BID>/pair/<tid>/c` with `Bearer TOK_DESK`. Relay: board open; ticket unexpired; token `dev` = creator; layout C (`0x43`, `0x45`, 64, total 100); `b` present, `c` absent → store; `200 {}`. (seal §17.4, SEAL-PAIR-005; relay §10.8)

    | Field | Width | Value |
    |---|---|---|
    | `label` | 1 | `0x43` |
    | `role` | 1 | `0x45` |
    | `body_len` | 2 | 64 |
    | `epk_E` | 32 | |
    | `r_E` | 32 | RAND3 |
    | `mac` | 32 | `MAC(mac_key; ctx("pair") ‖ 0x43 ‖ 0x45 ‖ tid ‖ u16 64 ‖ body[64])` |

23. **Desk**: `dh = X25519(esk_E, epk_N)` (all-zero → abort `seal/dh_zero`); transcript `T`, SAS; `ShowSas{digits}` → `status().pairing = ShowSas{digits}`; the desktop shows six digits. (seal §17.7, §17.10 `Posted` row; device §18.3)

    | `T` input (440 bytes) | Width | Value |
    |---|---|---|
    | context | 28 | `ctx("pair-transcript")` |
    | `vb16(relay origin)` | 2 + 21 | `u16 21` ‖ `"https://relay.example"` |
    | `board_id` | 16 | `BID` |
    | `tid` | 16 | |
    | `dev_E` | 16 | `DESK` |
    | slot A | 84 | verbatim, MAC included |
    | slot B | 157 | verbatim |
    | slot C | 100 | verbatim |

    `T = H(above)`; `sas = HKDF(salt s; ikm dh; info ctx("sas") ‖ T  (16 + 32 bytes); 8)`; `digits = (sas as big-endian u64) mod 1,000,000`, six decimal digits with leading zeros.

24. **Tab → Relay**: `GET …/pair/<tid>/c` (Door + proof) → slot C. Tab verifies MAC, label `0x43`, role `0x45` (SEAL-PAIR-003), then `H(ctx("pair-commit") ‖ epk_E ‖ r_E) = commit` of slot A in constant time (SEAL-PAIR-004). `dh = X25519(esk_N, epk_E)`; same `T` (Tab's own relay origin `https://relay.example`) and SAS; `AskSas` → `status().pairing = AskSas`. (seal §17.7, §17.10 `AwaitC`)

25. **User**: reads the six digits on Desk and types them on Tab: `join_sas(digits)` → `SasEntered{digits}`. (seal §17.1 step 7; device §17.10)

26. **Tab**: compares the typed digits with its own SAS in constant time (at most 3 entries): equal → `ShowAccepted` (`status().pairing = Accepted`, "accepted") and `FetchSlot{D}`: `GET …/d` every 2,000 ms (`404 relay/slot_empty` = wait). (SEAL-PAIR-007; seal §17.10 `AwaitSas`; device §18.3)

27. **User** (Desk): sees "accepted" on Tab and confirms on Desk: `pair_confirm()` → `UserConfirmed`. (seal §17.1 step 8; SEAL-PAIR-008; device §17.10)

28. **Desk** (machine → engine): `NeedSnapshotAndRegister{device item}` → the device sends `PairRegister{ticket, device: NewDevice{dev TAB, sign_pk TAB_SIGN, hpke_pk TAB_HPKE, name "Tab", transcript T}}` to the engine. (seal §17.10 `Showing` row; device §18.3; engine §4.2)

29. **Desk** (engine §28.4 step 1): not halted, eligible, no other pairing. `h = B = 200`. Material at 200: `SnapshotMaterial{seq 200, chain C200, hash_version 1, state_hash SH200, board = canonical board bytes at 200, membership = M_200 bytes}`. 200 is not aligned, so `SH200` is computed now (engine §18.3):

    | `SH200` input (86 bytes) | Width | Value |
    |---|---|---|
    | context | 18 | `ctx("state")` |
    | `hash_version` | 4 | u32 1 |
    | board hash | 32 | `H(canonical board bytes at 200)` (core §6.13) |
    | membership hash | 32 | `H(M_200 bytes)` |

    `M_200` canonical bytes (seal §13.2):

    | Field | Width | Value |
    |---|---|---|
    | `magic` | 4 | `"TDM1"` |
    | `version` | 4 | u32 1 |
    | `board_id` | 16 | `BID` |
    | `salt` | 16 | `SALT` |
    | `rules` | 4 | 1 |
    | `epoch` | 4 | 1 |
    | `epoch_seq` | 8 | 1 |
    | `epoch_clock_ms` | 8 | `τ_1` |
    | `clock_ms` | 8 | `τ_200` |
    | `home_gen` | 4 | 1 |
    | `home_seq` | 8 | 0 |
    | `home_host` | 1 | `0x00` (empty vb8) |
    | `home_door` | 5 | 5 zero bytes |
    | `purge_seq` | 8 | 0 |
    | `next_device_no` | 2 | 4 |
    | `n_commit` | 4 | 1, then 44: `u32 1 ‖ u64 1 ‖ c1` |
    | `n_recovery` | 2 | 1, then 104: `DEV_R ‖ RSIGN ‖ RHPKE ‖ u64 since 1 ‖ u64 until 0 ‖ u64 last_ctr 1` |
    | `n_dev` | 2 | 3, then the records of DESK, MAC, PHONE ascending by `dev` bytes |
    | `pending_recovery` | 1 | 0 |
    | `pending_override` | 1 | 0 |
    | `last_reset` | 1 | 0 |
    | `n_skip` | 4 | 0 |
    | `n_ctl` | 4 | 3, then `u64 1 ‖ u64 a_M ‖ u64 a_P` |

    Device record (155 + 5 + 3 + 5 + 1 + |name| bytes): DESK: `dev DESK ‖ sign_pk DESK_SIGN ‖ hpke_pk DESK_HPKE ‖ u16 1 ‖ u8 1 ‖ u64 added_seq 1 ‖ added_by DEV_R ‖ u64 bootstrap 0 ‖ u64 status_seq 1 ‖ status_by DEV_R ‖ u64 last_ctr 121 ‖ i64 beat_ms ‖ rules 0x01 ‖ u32 1 ‖ envelopes 0x01 ‖ 0x01 0x01 ‖ hash_versions 0x01 ‖ u32 1 ‖ vb8 "Desk"`; MAC: `u16 2`, `added_seq a_M`, `added_by` the adder, `bootstrap_seq` from its pairing, `last_ctr 60`, name "Mac"; PHONE: `u16 3`, `added_seq a_P`, `last_ctr 18`, name "Phone"; `beat_ms` each from its latest beat.

30. **Desk** (sealer `seal_snapshot(M_K, material)`): draws **RAND8[16]** = `PBLOB` (blob id, before sealing) and **RAND9[24]** = the blob nonce; puts the control history (records at `1, a_M, a_P` from `history.tdk`) in the body; raw-DEFLATEs, pads, seals under `K_snap,1`. Returns `SealedBlob{id PBLOB, bytes (Z bytes), sha256 D = H(bytes), parts 1, inflated I, epoch 1}`. (seal §20.1–§20.4, SEAL-BLOB-005, SEAL-BLOB-006; engine §6.1; device §15.7)
    Assumed: `Z ≤ 1,048,576`, so `parts = 1`; `Z` and `I` follow from the board's size and are left symbolic.

    Snapshot body (before DEFLATE, I bytes):

    | Field | Width | Value |
    |---|---|---|
    | `magic` | 4 | `"TDSN"` |
    | `S` | 8 | 200 |
    | `chain` | 32 | `C200` |
    | `hash_version` | 4 | 1 |
    | `state_hash` | 32 | `SH200` |
    | `board` | 4 + b | vb32 canonical board bytes at 200 |
    | `membership` | 4 + m | vb32 `M_200` bytes |
    | `history` | 4 + Σ | u32 3, then the history records (seal §21.5: `seq ‖ dev ‖ ctr ‖ entry_id ‖ at_ms ‖ frame_hash ‖ vb32(items) ‖ sig`) for seqs 1, `a_M`, `a_P` |

    Sealed blob (Z bytes):

    | Field | Width | Value |
    |---|---|---|
    | `magic` | 4 | `"TDB1"` |
    | `ver` | 1 | 1 |
    | `suite` | 1 | 1 |
    | `blob_kind` | 1 | 1 |
    | `epoch` | 4 | u32 1 |
    | `blob_id` | 16 | `PBLOB` |
    | `nonce` | 24 | RAND9 |
    | `ciphertext` | `P(4 + L) + 16` | `AEAD(K_snap,1; RAND9; AD; u32 L ‖ DEFLATE(body) ‖ 0x00 × (P(4+L) − 4 − L))`, `L` = deflated length |

    `AD` (84 bytes) = `ctx("blob")[17] ‖ BID[16] ‖ sealed[0..27] ‖ u64 200 ‖ DESK[16]` (writer = E, seal §20.2).

31. **Desk → Relay**: `Blob{Upload{purpose 2, seq 200, blob}}`: `POST /v1/b/<BID>/blobs` (`blob-begin`, `Bearer TOK_DESK`, 65 bytes), then `PUT /v1/b/<BID>/blobs/<PBLOB hex>/0` with the Z sealed bytes. (engine §28.4 step 1; device §10.5; relay §10.7)

    | `blob-begin` field | Width | Value |
    |---|---|---|
    | `id` | 16 | `PBLOB` |
    | `purpose` | 1 | 2 |
    | `seq` | 8 | 200 |
    | `size` | 8 | Z |
    | `sha256` | 32 | D |

32. **Relay**: `blob-begin`: lazy deletions first (incomplete blobs and complete purpose-2 blobs with `at ≤ nowR − 3,600,000`); checks: board open; `size` 67–67,108,864; purpose 2: `1 ≤ 200 ≤ head_seq 200`; no row `PBLOB`; fewer than 3 incomplete and declared bytes + Z ≤ 805,306,368. Insert `blobs(PBLOB, slot = 1 + max slot, purpose 2, seq 200, author DESK, size Z, parts 1, sha256 D, received 0, complete 0, at nowR)`; `200 {"id": "<PBLOB hex>", "parts": 1}`. `blob-put` 0: blob exists, author = token dev, `0 < 1`, length = Z; insert `parts(slot×64 + 0)`; `received = 1 = parts` → `complete = 1`; retention §13.1 for purpose 2: nothing; `200 {"received": 1, "complete": true}`. (relay §10.7, §13.1, §13.2)
    AMBIGUOUS: engine §28.4 step 2 does not say whether the `DeviceAdded` is enqueued only after `BlobResult{Uploaded}` or at once after step 1, nor what an upload failure does; taken: enqueued after the upload succeeded.

33. **Desk** (engine §28.4 step 2, §9.2): enqueue the control record: **RAND10[16]** = `E201` (`Sealer::new_entry_id`); record `{entry_id E201, order o, at_ms t1 (= wall_ms), based_on (200, C200), kind ctl, body Ctl(AddDevice{dev TAB, sign_pk TAB_SIGN, hpke_pk TAB_HPKE, name "Tab", transcript T, bootstrap_seq 200}), via Register{dev TAB, sign_pk TAB_SIGN}, hold false, ctr/frame/ack/seen none, reseals 0}`; `Persist[Outbox(Put)]`. (engine §7.4)

34. **Desk** (send loop, engine §10.2 step 2, §28.4 step 3): the first unacknowledged record has `via ≠ Append` and every earlier one is acknowledged or seen; `M_K.epoch = 1` = the snapshot's epoch (ENG-OPS-006); `ctr = next_ctr = 1 + max(121, 121, 121) = 122` (§10.3); `Sealer::seal(M_200, 122, entry)` composes the item and inner signature and draws **RAND11[24]** = frame nonce. `Persist[Outbox(Put(record with ctr 122, frame F201, frame_hash FH201, epoch 1))]` (D2; no secret changed, so no `WriteIdentity`). (seal §11.3, §12.3, SEAL-PAIR-010; engine §6.1 `seal`)

    `items_bytes` (332 bytes, canonical):
    `[{"bootstrap_seq":200,"dev":"<TAB hex32>","hpke_pk":"<TAB_HPKE hex64>","name":"Tab","sign_pk":"<TAB_SIGN hex64>","transcript":"<T hex64>","type":"DeviceAdded"}]`

    Inner signature `SIG(sk(DESK_SIGN); "taskdeck/v1/ctl\0"; …)` over (112 bytes):

    | Field | Width | Value |
    |---|---|---|
    | context | 16 | `ctx("ctl")` |
    | `board_id` | 16 | `BID` |
    | `dev` | 16 | `DESK` |
    | `ctr` | 8 | 122 |
    | `entry_id` | 16 | `E201` |
    | `at_ms` | 8 | `t1` |
    | `H(items_bytes)` | 32 | |

    `ctl` payload (479 bytes, canonical): `{"items":[{"bootstrap_seq":200,"dev":"…","hpke_pk":"…","name":"Tab","sign_pk":"…","transcript":"…","type":"DeviceAdded"}],"sig":"<inner sig hex128>"}`

    Plaintext (`U = 69 + 479 = 548`, `P(548) = 1024`):

    | Field | Width | Value |
    |---|---|---|
    | `entry_id` | 16 | `E201` |
    | `kind` | 1 | 2 (`ctl`) |
    | `at_ms` | 8 | `t1` |
    | `based_on_seq` | 8 | 200 |
    | `based_on_chain` | 32 | `C200` |
    | `payload_len` | 4 | 479 |
    | `payload` | 479 | above |
    | `padding` | 476 | `0x00` |

    Frame `F201` (1182 bytes):

    | Field | Width | Value |
    |---|---|---|
    | `magic` | 4 | `"TDF1"` |
    | `board_id` | 16 | `BID` |
    | `dev` | 16 | `DESK` |
    | `ctr` | 8 | 122 |
    | `len` | 4 | 1070 |
    | body `ver` | 1 | 1 |
    | body `suite` | 1 | 1 |
    | body `epoch` | 4 | 1 |
    | body `nonce` | 24 | RAND11 |
    | body `ciphertext` | 1040 | `AEAD(K_entry,1; RAND11; AD; plaintext[1024])` |
    | `sig` | 64 | `SIG(sk(DESK_SIGN); "taskdeck/v1/frame\0"; F201[0..1118])` |

    `AD` (72 bytes) = `ctx("entry")[18] ‖ F201[0..48] ‖ u8 1 ‖ u8 1 ‖ u32 1`. (seal §8, §9, §10)

35. **Desk → Relay** (after that `Persisted`): `Admin{Register{frame F201, dev TAB, sign_pk TAB_SIGN}}` → `POST /v1/b/<BID>/admin/nonce`, `Bearer TOK_DESK`, body `DESK` (16 bytes). Relay: signer = token's `dev`; row current; board open; delete nonces with `at < nowR − 120,000`, trim to 63 if needed; **RAND12[32]** = `NA` (relay, `crypto.getRandomValues`); insert `nonces(NA, purpose 2, DESK, nowR)`; `200 {"nonce": "<NA hex>", "expires": nowR + 120,000}`. (engine §10.2 step 2, §11.7; device §10.5; relay §12.1)

36. **Desk → Relay**: `POST /v1/b/<BID>/admin/register`, `Bearer TOK_DESK`, `Content-Length: 1382`. (seal §16.2; relay §12.1, §12.3)

    Admin message signed, `SIG(sk(DESK_SIGN); "taskdeck/v1/admin\0"; …)` (123 bytes):

    | Field | Width | Value |
    |---|---|---|
    | context | 18 | `ctx("admin")` |
    | `op` | 1 + 8 | `0x08` ‖ `"register"` |
    | `board_id` | 16 | `BID` |
    | `signer` | 16 | `DESK` |
    | `args_hash` | 32 | `H(args)` |
    | `relay_nonce` | 32 | `NA` |

    `args` (80) = `TAB[16] ‖ TAB_SIGN[32] ‖ FH201[32]`.

    Request body (1382 bytes):

    | Field | Width | Value |
    |---|---|---|
    | `signer` | 16 | `DESK` |
    | `nonce` | 32 | `NA` |
    | `sig` | 64 | admin signature |
    | `args_len` | 4 | 80 |
    | `args` | 80 | above |
    | `extra_len` | 4 | 1182 |
    | `extra` | 1182 | `F201` |

37. **Relay**: gate (route `admin/register`, cap 262,344, token). Object phase 1–2: parse; await `verifyStrict(DESK_SIGN, admin message, sig)` and `verifyStrict(DESK_SIGN, ctx("frame") ‖ F201[0..1118], F201[1118..1182])`. Phase 3, one transaction: A1 lengths and `args` 80 bytes, `extra` a frame 670–262,144; A2 board open; A3 token credential, `signer = DESK` = token's `dev`; A4 nonce `NA` purpose 2 `dev DESK` fresh → deleted whatever follows; A5 signer row current, role 1; A6 admin signature ✓; A7 `register`: (1) `TAB` is no row's `dev`, `TAB_SIGN` no row's `sign_pk`; (2) current role-1 rows 3 < 16, role-1 rows 3 < 256; (3) `H(F201) = FH201`, frame `dev = DESK` = signer, P1 (1182 in range, `"TDF1"`, `len` 1070 = 1182 − 112), P2 board, P3 `ctr` 122 in 1–2^53−1, P4 row `DESK`, P5 `dev` = token's, P6 state 1, P7 signature ✓; T1 row unchanged; T2 `122 = 121 + 1`; T4 Desk's count today < 3,000; T5 `seq 201`, `at = max(nowR, at_200)`, `chain C201`, insert `log(201, DESK, 122, FH201, C201, at, F201)`, `DESK.last_ctr = 122`, `last_seq = 201`, app count + 1; then insert `devices(TAB, TAB_SIGN, role 1, state 1, gen 1, last_ctr 0, last_seq 0, app_day 0, app_count 0, added_at nowR, wraps NULL)`. Commit. (relay §8.4, §9.3, §9.4, §12.1, §12.3; RELAY-REG-001)

    `C201` input (90 bytes) = `ctx("chain")[18] ‖ C200[32] ‖ u64 201 ‖ FH201[32]` (seal §15.1).

    Token `TOK_TAB` (69 bytes, text `"TD1."` ‖ base64url = 96 chars):

    | Field | Width | Value |
    |---|---|---|
    | `role` | 1 | 1 |
    | `board_id` | 16 | `BID` |
    | `dev` | 16 | `TAB` |
    | `gen` | 4 | 1 |
    | `mac` | 32 | `MAC(TOKEN_KEY; ctx("token") ‖ token[0..37])` (55-byte input) |

    Answer `200 {"seq": 201, "chain": "<C201 hex>", "token": "TD1.<92 chars>"}`. (relay §5.1, §12.3)

38. **Relay** (after commit): push to every ready socket (Desk's and Mac's; Phone has none; Tab has none): `0x84 ‖ page` (1274 bytes). (relay §11.4; RELAY-SOCK-008)

    | Field | Width | Value |
    |---|---|---|
    | type | 1 | `0x84` |
    | `magic` | 4 | `"TDP1"` |
    | `head_seq` | 8 | 201 |
    | `head_chain` | 32 | `C201` |
    | `relay_now` | 8 | `nowR` |
    | `floor` | 8 | 1 |
    | `purge_floor` | 8 | 1 |
    | `flags` | 1 | 0 |
    | `n` | 2 | 1 |
    | entry | 20 + 1182 | `u64 201 ‖ i64 at ‖ u32 1182 ‖ F201` |

39. **Desk** (transport/engine): keeps the token string `TOK_TAB` for the ticket; delivers `AdminResult{Ok(AdminOk{seq 201, chain C201, token true})}`. Engine: record `ack = (201, C201)`, `Put` in the next `Persist`; `Notify(PairRegistered{ticket, head (200, C200), snapshot SnapInfo{seq 200, hash SH200, hash_version 1, blob PBLOB, sha256 D, size Z, parts 1, inflated I, epoch 1}, added_seq 201})`. (engine §10.4, §11.7, §28.4 step 3; device §18.3)

40. **Desk** (device → machine): `Registered{h 200, chain C200, snapshot ref, added_seq 201, token TOK_TAB, roots [(1, R1)], host "relay.example", door DOOR}` (roots of the snapshot's epoch only, since it is 1). Machine: `bundle_key`, bundle, **RAND13[24]** = bundle nonce, slot D; effects `PostSlot{D}`, `Done{TAB}`. (seal §17.8, §17.10 `Registering`; device §18.3)

    Bundle JSON (canonical, `665 + d(I) + d(Z)` bytes, `d(x)` = decimal digits of x):
    `{"added_seq":201,"board_id":"<BID hex>","door":"<DOOR hex10>","head":{"chain":"<C200 hex>","seq":200},"host":"relay.example","roots":[{"epoch":1,"root":"<R1 hex>"}],"snapshot":{"blob":"<PBLOB hex>","epoch":1,"hash":"<SH200 hex>","hash_version":1,"inflated":I,"parts":1,"sha256":"<D hex>","size":Z},"token":"TD1.<92 chars>"}`

    `bundle_key = HKDF(salt s; ikm dh; info ctx("pair-bundle-key")[28] ‖ T[32]; 32)`; `AD` (88) = `ctx("pair-bundle")[24] ‖ BID[16] ‖ tid[16] ‖ T[32]`.

    Slot D (`741 + d(I) + d(Z)` bytes):

    | Field | Width | Value |
    |---|---|---|
    | `label` | 1 | `0x44` |
    | `role` | 1 | `0x45` |
    | `body_len` | 2 | `705 + d(I) + d(Z)` |
    | `nonce` | 24 | RAND13 |
    | `ciphertext` | bundle + 16 | `AEAD(bundle_key; RAND13; AD; bundle JSON)` |
    | `mac` | 32 | `MAC(mac_key; ctx("pair") ‖ 0x44 ‖ 0x45 ‖ tid ‖ u16 body_len ‖ body)` |

41. **Desk → Relay**: `POST /v1/b/<BID>/pair/<tid>/d`, `Bearer TOK_DESK`. Relay: board open; ticket unexpired; creator; layout D (`0x44`, `0x45`, 40–16,040, total = 36 + body_len); `c` present, `d` absent → store; `200 {}`. Desk: `status().pairing = Done`. The ticket stays stored until it expires (no `pair-close` after `Done`). (relay §10.8; device §18.3)

42. **Desk** (push of 201, before or after step 39; nothing orders the HTTP answer and the push): page processing (engine §12.3): limits ✓; `relay_now` measured; head check (201 > L); 201 = L + 1; dry run `C201 = chain_step(C200, 201, F201)`; `classify` with `M_200`, `own_sealed(122) = FH201` → steps 1–17 pass → `Accept`, `own_dev_foreign` false; head chain = `C201` ✓. `Persist[AppendLog[{201, C201, τ_201, F201}], SetMarker{201, 201, attempt}]` + the store's history record for 201 (device §8.3). Commit: `L = K = 201`, `M_201`; `DeviceAdded` banner: none on the author device (engine §14.2); board: nothing; own entry → `seen = 201`, `Outbox[Drop(E201)]`; `B = 201`; echo check: log at 201 has `FH201` and `C201` = ack ✓ (ENG-ECHO-001). (engine §12.3, §12.4, §13.1, §14.1)

43. **Mac** (push of 201): same page processing; `classify(M_200)`: 1 size/magic/len; 2 `BID`; 3 `DESK` current; 4 strict signature under `DESK_SIGN`; 5 `122 = 121 + 1`; 6 not own; 7 current; 8 (1,1) allowed; 9 epoch 1 = E; 10 AEAD opens under `K_entry,1`; 11 unpadding (1024 ≥ 69, U = 548, P(548) = 1024, zeros, kind 2, `at_ms` in range); 12 `based_on 200 < 201`, Mac holds `C200` = `based_on_chain` ✓; 13 kind `ctl` by a current device under the current epoch; 15 payload canonical, inner signature ✓; 16 shape A, `DeviceAdded` preconditions (fresh keys; 3 < 16 current, 3 < 256 records, `next_device_no 4 ≤ 8191`; `bootstrap_seq 200 ≥ purge_seq 0` and `< 201`) → `Accept`. `M_201`: TAB record `device_no 4, current, added_seq = status_seq = 201, added_by = status_by = DESK, bootstrap_seq 200, last_ctr 0, beat_ms t1, rules [1], envelopes [(1,1)], hash_versions [1], name "Tab"`; `next_device_no 5`; DESK `last_ctr 122`; `τ_201 = max(τ_200, t1)`; `ctl_seqs += 201`. Persist `AppendLog` + `SetMarker` + history record for 201. Commit; banner `DeviceAdded{id (201, 0), seq 201, at_ms t1, author DESK, author_name "Desk", author_fp kfp(DESK), subject TAB, subject_name "Tab", subject_fp kfp(TAB), detail None, acked false}` kept in `Meta` (`WriteMeta`); `Notify(BannersChanged)`. (seal §14.2, §13.6, §6.8; engine §14.1, §14.2)
    **Mac shows**: the banner "Desk added Tab" with key `XXXX…` (short form of `kfp = H(ctx("key-fp") ‖ TAB_SIGN ‖ TAB_HPKE)[0..10]`), kept until acknowledged; the device list shows Tab (device 4) with no last beat.

44. **Tab → Relay**: `GET …/pair/<tid>/d` (Door + proof) → slot D bytes. Checks (seal §17.9): (1) MAC, label `0x44`, role `0x45` (`seal/slot_mac`); (2) AEAD opens under `bundle_key` with the AD of step 40 (`seal/bundle_open`); (3) strict canonical JSON, `board_id = BID`, `host = "relay.example"`, `door = DOOR` as typed (`seal/bundle_invalid`). → `FetchSnapshot{TOK_TAB, PBLOB, 1}`.

45. **Tab → Relay**: `GET /v1/b/<BID>/blobs/<PBLOB>` (`blob-meta`), then `GET /v1/b/<BID>/blobs/<PBLOB>/0`, both `Bearer TOK_TAB`. Relay: gate verifies the token's MAC and board; object: row `TAB` role 1, state 1, `gen 1` ✓; meta `200 {"id": "<PBLOB>", "purpose": 2, "seq": 200, "author": "<DESK hex>", "size": Z, "parts": 1, "sha256": "<D hex>", "received": 1, "complete": true}`; part 0: the Z bytes. (device §10.5 `Blob Fetch`, §18.3; relay §10.7)

46. **Tab**: §17.9 step 4 = seal §20.5 with the bundle's values (`S = 200`, writer `DESK`, `sha256 D`, `size Z`, `parts 1`, `inflated I`, `epoch 1`), key `K_snap,1` from the bundle's `R1`: (1) one part of Z bytes; (2) `H(part) = D` in constant time, nothing decrypted before; (3) `"TDB1"`, ver 1, suite 1, `blob_kind 1`, epoch 1; (4) AEAD opens with `AD = ctx("blob") ‖ BID ‖ sealed[0..27] ‖ u64 200 ‖ DESK`; (5) padding (§10.3 with `U = 4 + L`); (6) inflate with cap I, exactly I bytes, no trailing input; (7) body parses with no trailing bytes, `S = 200`, `state_hash = SH200`, membership parses, history projection (records 1, `a_M`, `a_P`: ascending seqs, authorities, counters, inner signatures, shapes, reduced rules; Genesis `board_id` check with Tab's own `BID`) agrees with `M_200`. Step 5: body `chain = C200 = head.chain`; `M_200` names `BID`, lists `DESK` current, current epoch 1 = highest epoch in `roots`; `H(ctx("epoch") ‖ BID ‖ u32 1 ‖ R1) = c1` recorded there; recomputed `SH200` (step 29 table) = `snapshot.hash`. → `ReadLog{TOK_TAB, from 201}`. (seal §17.9 steps 4–5, §20.5; SEAL-KEY-013)

47. **Tab → Relay**: `GET /v1/b/<BID>/log?after=200` (limit default 256), `Bearer TOK_TAB`. Relay: rows `seq > 200` → [201]; page `"TDP1" ‖ head 201 ‖ C201 ‖ relay_now ‖ floor 1 ‖ purge_floor 1 ‖ flags 0 ‖ n 1 ‖ entry 201` (1273 bytes); no write. (device §18.3; relay §10.5)

48. **Tab**: §17.9 step 6: classify 201 from `M_200` with `C_200 = head.chain` (the 17 checks of step 43; `based_on_chain C200` equals Tab's `C200`) → `Accept`; the relay's head is 201, so the run ends. Step 7: entry at `added_seq` 201 accepted, author `DESK`, `based_on_seq 200 = h`, one `DeviceAdded` naming `TAB`, `TAB_SIGN`, `TAB_HPKE`, "Tab", `T` ✓. Step 8: `device_no = 4` from `M_201` (SEAL-PAIR-012). → `Joined{device_no 4, keys, bundle, snapshot}`. (seal §17.9, §17.10 `Joining`)

49. **Tab** (persisting the join, device §18.3, in order): **RAND14[32]** = `L_d` (`OsEntropy`); `KeyStore::create(L_d)` (Android Keystore, which writes `ld.wrap` itself, device §12.5); directory `sync/id-<BID hex>-<TAB hex>/`; then `store.tds`, `identity.tdk`, `device.tdk`, `bootstrap.tdk`, `history.tdk`, each by the replace or append procedure (`write_new(.tmp)`, `sync`, `rename`, `sync_dir`; device §7.2), each sealed file with a fresh `file_id` and each record with a fresh nonce: **RAND15[16]**/**RAND16[24]** identity, **RAND17[16]**/**RAND18[24]** device, **RAND19[16]**/**RAND20[24]** bootstrap, **RAND21[16]**/**RAND22–24[24]** history (3 records). Then set-aside: no plain file holds data (fresh install), nothing moves. Then `active.tds` replaced. Then the facade reopens in synced mode. (device §6.1, §6.2, §6.4, §7.2, §18.3, §18.4; SEAL-PAIR-011; DEV-PAIR-002)

    Sealed file of kind k (seal §19.1, §19.3, §6.6): header (55) `"TDL1" ‖ u8 1 ‖ u8 1 ‖ u8 k ‖ TAB[16] ‖ BID[16] ‖ file_id[16]`; record i: `u32 ct_len ‖ nonce[24] ‖ AEAD(K_file(k); nonce; ctx("file")[17] ‖ header[55] ‖ u64 i; plaintext)`, `K_file(k) = HKDF(salt BID; ikm L_d; info ctx("file-key") ‖ u8 k ‖ TAB; 32)`.

    | File | Plaintext |
    |---|---|
    | `store.tds` (54, plain) | `"TDS1" ‖ u8 1 ‖ BID ‖ TAB ‖ u8 3 (Android Keystore) ‖ i64 created_ms ‖ H(bytes 0..46)[0..8]` |
    | `identity.tdk` (kind 7) | `"TDI1" ‖ vb16(DeviceKeys::to_identity_bytes) ‖ u16 1 ‖ (u32 1 ‖ R1) ‖ u8 0 (pending) ‖ u8 0 (anchor)` |
    | `device.tdk` (kind 12) | `{"device_name":"Tab","history_complete":true,"minter":{"block":B0,"device_no":4,"item":N0},"tokens":[{"gen":G,"host":"relay.example","token":"TD1.…"}]}` (no `push`) |
    | `bootstrap.tdk` (kind 17) | `0x02 ‖ u64 200 ‖ C200 ‖ u32 1 ‖ SH200 ‖ vb32(board) ‖ vb32(M_200) ‖ u64 201 ‖ vb8("relay.example") ‖ DOOR[5] ‖ u32 1` |
    | `history.tdk` (kind 11) | 3 records: the snapshot's history records for 1, `a_M`, `a_P`, byte for byte |
    | `sync/active.tds` (45, plain) | `"TDA1" ‖ u8 1 ‖ BID ‖ TAB ‖ H(bytes 0..37)[0..8]` |

    AMBIGUOUS: `device.tdk.tokens[].gen` is required as "the gen the login returned" (device §10.3, §16), but the bundle's token comes with no `gen` and is opaque (SEAL-KEY-015); taken: `G = 1`.
    AMBIGUOUS: the initial minter (`IdMinter::new` for `device_no` 4) is not given numerically (device §8.3, §18.3; core §7.2); taken: `item 1`, `block 1` as the next numbers.

50. **Tab** (open): draws **RAND25[16]** = session id (`OsEntropy`, never stored); `Loaded{bootstrap Join{material, added_seq 201, relay (relay.example, DOOR, home_gen 1)}, no checkpoint, no meta, log_floor 1}`. The bridge sees the mode change and reloads the WebView; the new page load gets `hello.mode = "synced"`. (device §7.4, §10.4; bridge §5.8, BRIDGE-EVT-005)

51. **Tab** (engine `Start`, §15.2, §19.8, §19.7 step 4): attempt 1 → `WriteMeta` (D5, with `relay = (relay.example, DOOR, 1)`); `SM::load(board)` ✓; `SM::rules = 1 = M_200.rules` ✓; `B = K = L = 200`, `C200`, `M_200`, `log_floor 201`, `retained_from 200`; `Persist[WriteCheckpoint{board_seq 200, board_chain C200, board, M_200, log_floor 201, registry empty}, WriteBase(200)]`, then, after its `Persisted` (D8), `Persist[TruncateLogBelow(201, C200)]` (device.tdk `log_floor 201`, `floor_chain C200`). `WriteCheckpoint` removes `bootstrap.tdk` after replacing `checkpoint.tdk`. Each new file draws a `file_id` and each record a nonce (checkpoint, base, meta, device: RAND26–RAND33). No history record is added: `history.tdk` already holds those seqs (device §8.3). (device §7.3)
    AMBIGUOUS: §19.8 points to §19.7 step 4 ("a checkpoint and a base", then `TruncateLogBelow(S + 1)` listed first); it is unclear whether a base is written at a non-aligned `h = 200` (bases are aligned seqs, §18.2) and in which order the truncate and the checkpoint go (D8 requires the checkpoint first); taken: checkpoint and base at 200 first, truncate after.

52. **Tab → Relay** (`Connect`, no backoff on the first after `open`): the stored token has no `push` → `GET /v1/b/<BID>/caps`, `Bearer TOK_TAB`; the gate answers (`X-TD-Relay: gate`) `200 {"v": 1, "push": true, "max_batch": 16, "max_frame": 262144, "page_entries": 256, "page_bytes": 1048576, "part_bytes": 1048576}`; Tab stores `push: true` with the token in `device.tdk` (replace). **No login**: a token is stored for the host, so none of the four login cases of device §10.3 applies. Tab is in the foreground (the activity is visible, `set_foreground(true)`), so it opens `wss://relay.example/v1/b/<BID hex>/socket`, `Bearer TOK_TAB`; relay: gate; object: row `TAB` valid, board open → `acceptWebSocket(server, ["<TAB hex>"])`, attachment `{dev TAB, gen 1, features 0, ready false}`. (device §10.3, §10.5, §10.9; relay §10.9, §11.1, §11.2; DEV-NET-018)

53. **Tab → Relay**: hello `0x01 ‖ RAND25[16] ‖ u32 0` (21 bytes). Relay: no session yet, 21 bytes; no other socket of `TAB` → store session, `features 0`, `ready true`; send welcome (93 bytes). (relay §11.3)

    | Welcome field | Width | Value |
    |---|---|---|
    | `type` | 1 | `0x81` |
    | `head_seq` | 8 | 201 |
    | `head_chain` | 32 | `C201` |
    | `last_ctr` | 8 | 0 |
    | `last_seq` | 8 | 0 |
    | `relay_now` | 8 | `nowR` |
    | `floor` | 8 | 1 |
    | `purge_floor` | 8 | 1 |
    | `state` | 1 | 1 |
    | `caps` | 1 | `0x01` |
    | `appends_left` | 4 | 3000 |
    | `door` | 5 | `DOOR` |
    | `has_fence` | 1 | 0 |

54. **Tab** (engine): `Connected{push true}`, `Welcome`: skew measured; `head 201 ≥ L 200` (no chain at 201 held); no acks; not fenced; reconcile with `r = 0`, `q = 0`: `a = 0`, `h = 0` → normal. `Read{after 200, limit 256}` → socket `0x03 ‖ u32 req ‖ u64 200 ‖ u16 256` (15 bytes); relay answers `0x83 ‖ u32 req ‖ page` with entry 201. (engine §11.1–§11.3, §12.2; relay §11.4)
    AMBIGUOUS: engine §11.3 defines `a` as this device's `last_ctr` in `M_K` and `h` as the largest counter of a record with a frame; at `K = 200` Tab has no record in `M_K` and no outbox record, so neither value is defined; taken: both 0.

55. **Tab**: processes the page (engine §12.3): classify 201 from `M_200` → `Accept` (as step 48); `Persist[AppendLog[201], SetMarker{201, 201, 1}]` + history record for 201 (device §8.3); commit `L = K = 201`; banner `DeviceAdded` (author Desk, subject Tab) created on Tab too, Tab not being the author (engine §14.2), kept in `Meta`; board: nothing; `B = 201`: Tab has joined (`B ≥ added_seq`, `device_no 4` in `M_201`; ENG-BOOT-003). Read run ends → rebase; session `Live`. Tab shows the board (pending view = confirmed state at 201) and the banner.

56. **Tab** (duties, engine §23.1, §29.1): eligible (joined, current, `Live`, caught up, unhalted, clock not blocked); `wall_ms − last_beat_ms ≥ 86,400,000` with `last_beat_ms` of a fresh `Meta` → beat due. Enqueue: **RAND34[16]** = `EB` entry id; record `{EB, at_ms t2, based_on (201, C201), kind beat, body Beat{rules [1], envelopes [(1,1)], hash_versions [1], state none, acks []}}`; `last_beat_ms = t2`; `Persist[Outbox(Put)]`.
    AMBIGUOUS: the initial `Meta.last_beat_ms` of a new replica is not given (engine §7.8, §23.1); taken: 0, so Tab's first frame is a beat and "Hello" is its second (if no beat were due, "Hello" would be ctr 1 at seq 202).
    AMBIGUOUS: `state` of a beat is "this device's hash record at the largest aligned `S ≤ B`" (engine §23.2); Tab bootstrapped at 200 and holds no record at 192; taken: absent.

57. **Tab** (send loop): `ctr = 1 + max(0, 0, 0) = 1`; seal under epoch 1, **RAND35[24]** nonce; `Persist[Outbox(Put(frame F202 ...))]` (D2); `Append` over the socket. (engine §10.2, §10.3)

    Beat payload (53 bytes): `{"envelopes":[[1,1]],"hash_versions":[1],"rules":[1]}`. Plaintext: `EB[16] ‖ u8 3 ‖ i64 t2 ‖ u64 201 ‖ C201 ‖ u32 53 ‖ payload ‖ 0x00 × 390` (`P(122) = 512`). Frame `F202` (670 bytes): `"TDF1" ‖ BID ‖ TAB ‖ u64 1 ‖ u32 558 ‖ (u8 1 ‖ u8 1 ‖ u32 1 ‖ RAND35 ‖ AEAD(K_entry,1; RAND35; ctx("entry") ‖ F202[0..48] ‖ 1 ‖ 1 ‖ u32 1; plaintext[512]) [528]) ‖ SIG(sk(TAB_SIGN); "taskdeck/v1/frame\0"; F202[0..606])`.

    Socket append (684 bytes): `0x02 ‖ u32 req ‖ "TDA1" ‖ u8 1 ‖ u32 670 ‖ F202`.

58. **Relay**: re-reads `TAB` row (current, `gen 1`); §9.3 P1–P7 (`TAB` row exists, `dev` = token's, state 1, strict signature under `TAB_SIGN`); §9.4 T1, T2 `1 = 0 + 1`, T4 count 0 < 3,000, T5 `seq 202`, `C202 = H(ctx("chain") ‖ C201 ‖ u64 202 ‖ FH202)`, `TAB.last_ctr 1`, `last_seq 202`, `app_day` today, `app_count 1`. Result `0x82 ‖ u32 req ‖ "TDR1" ‖ u8 1 ‖ 0x00 ‖ u64 202 ‖ C202` (51 bytes); push `0x84` page with 202 to Desk, Mac and Tab. (relay §9.3–§9.6, §11.4)

59. **Tab, Desk, Mac**: Tab records `ack (202, C202)`; each classifies 202 from `M_201` → `Accept` (beat), `TAB.beat_ms = max(t1, t2)`, lists unchanged; Tab: `seen 202`, `Drop(EB)`; `B = 202` everywhere; Desk and Mac show Tab's last seen `t2`. (seal §13.10; engine §13.1, §23.3)

60. **User** (Tab): adds a task "Hello" with the page's quick add: the page sends `{"v":1,"k":"req","id":n,"method":"POST","path":"/api/command","body":"{\"op\":\"quick_add\",\"name\":\"Hello\",\"deadline\":…}"}` (no due date: the `deadline` the page sends is empty or null, which is none). The bridge reads `op` = `quick_add` (one of the 17 verbs) and calls `submit_json(body)`. (bridge §5.2, §6.2 row 3, §6.3; core §12.2 step 4)

61. **Tab** (facade `submit_json`, device §17.4): `parse_intent` → `quick_add{name "Hello", deadline none}`; `compile` against `Engine::view()` (B = 202) with zone Europe/Helsinki, `now = t3` (whole ms), `device_no 4`, the overlay and the minter: trim → "Hello", not empty, no deadline, cap ✓, mint item `n = max(1, mark(view, 4).item + 1 = 1) = 1` → `id = 4 × 2^40 + 1 = 4,398,046,511,105`, `minter.next_item = 2`. Command `create_task{id 4398046511105, name "Hello", time_importance 1}`; reply `{id}`. `Enqueue{ticket, command, at_ms t3, origin_state = u16 4 ‖ u64 2 ‖ u64 1 (18 bytes), hold false}`. (core §13.3, §13.4 `quick_add`, §7.2; device §8.3)

62. **Tab** (engine §9.1): no refusal (joined, not revoked, clock not blocked, `|t3 − wall_ms| ≤ 60,000`); trial apply with `ctx {seq 203, device_no 4, at t3, based_on 202}` ✓; **RAND36[16]** = `EH` entry id; record `{EH, order, at_ms t3, based_on (202, C202), kind cmd, body payload, origin_state, via Append, hold false}`; `Persist[Outbox(Put)]`; pending view gets "Hello", `view_version + 1`. On `Persisted`: `Notify(Enqueued{ticket, EH, provisional_seq 203})`; the bridge answers `{"v":1,"k":"res","id":n,"status":200,"body":{"ok":true,"version":V,"id":4398046511105}}` and pushes `changed`. (engine §9.1; bridge §6.3, BRIDGE-VERB-002)

63. **Tab** (send loop): `ctr 2`, **RAND37[24]** nonce; `Persist[Outbox(Put(F203))]`; socket append (684 bytes). (engine §10.2)

    Payload (74 bytes, canonical): `{"id":4398046511105,"name":"Hello","op":"create_task","time_importance":1}` (`blocks` at its default `[]` is omitted). Plaintext (512): `EH[16] ‖ u8 1 ‖ i64 t3 ‖ u64 202 ‖ C202 ‖ u32 74 ‖ payload ‖ 0x00 × 369`. Frame `F203` (670): `"TDF1" ‖ BID ‖ TAB ‖ u64 2 ‖ u32 558 ‖ body(1 ‖ 1 ‖ u32 1 ‖ RAND37 ‖ AEAD(K_entry,1; RAND37; ctx("entry") ‖ F203[0..48] ‖ 1 ‖ 1 ‖ u32 1; plaintext)) ‖ SIG(sk(TAB_SIGN); "taskdeck/v1/frame\0"; F203[0..606])`.

64. **Relay**: as step 58 with `ctr 2 = 1 + 1` → `seq 203`, `C203`, `TAB.last_ctr 2`, `last_seq 203`, `app_count 2`; result `new (203, C203)`; push 203 to Desk, Mac, Tab.

65. **Desk, Mac, Tab**: classify 203 from `M_202` → `Accept` (cmd); `parse_command` ✓; `apply` under `Ctx{seq 203, device_no 4, at t3, based_on 202}`: mint check (high part 4 = `device_no`, `n 1 > mark 0`), caps ✓; insert `{"created":"<t3 as 30-byte instant>","id":4398046511105,"kind":"task","name":"Hello","stamps":{"estimate":[203,4],"levels":[203,4],"name":[203,4],"repeat":[203,4],"time":[203,4]},"time_importance":1}`; add mark `{"block":0,"device_no":4,"item":1}`. Tab: `ack`, `seen 203`, `Drop(EH)`. **Mac shows** "Hello" on its board. (core §9.4, §7.2, §8.1; engine §12.4)

### State after the last step

| | Desk | Mac | Tab | Relay |
|---|---|---|---|---|
| head | 203, `C203` | 203, `C203` | 203, `C203` | `head_seq 203`, `C203` |
| board head `B` | 203 | 203 | 203 | — |
| log | its whole log to 203 | to 203 | `log_floor 201`: 201–203; `retained_from 200` | rows 1–203 |
| counters | Desk 122 | Mac 60 | Tab 2 | rows: DESK `last_ctr 122/last_seq 201`, MAC 60, PHONE 18, `DEV_R` 1, TAB `2/203`, `gen 1`, `app_count 2` |
| epoch | 1 | 1 | 1 (holds `R1` only) | — |
| outbox | empty | empty | empty | — |
| membership | `M_203`: 4 current devices, TAB `device_no 4`, `bootstrap_seq 200`, `next_device_no 5`, `ctl_seqs [1, a_M, a_P, 201]` | same | same | devices rows incl. TAB |
| durable additions | log 201–203, history record 201, outbox Put/Drop E201 | log 201–203, history 201, banner in meta | store.tds, identity, device (token, `push`, minter, `log_floor 201`, `floor_chain C200`), checkpoint and base at 200, log 201–203, history 1, `a_M`, `a_P`, 201, meta (banner), outbox Put/Drop EB, EH, active.tds | ticket `tid` with slots A–D until `expires`; blob `PBLOB` (purpose 2) until a `blob-begin` an hour later deletes it; nonce `NA` deleted |

What the users saw: Desk — the pairing sheet (code, address), the six digits, then "done"; no banner. Tab — the Pair screen, "accepted", then the synced board with a `DeviceAdded` banner, then "Hello". Mac — the banner "Desk added Tab (key XXXX…)", Tab in the device list, then "Hello". Phone — nothing (closed; it holds 200).

---

## Scenario 6 — Snapshot bootstrap

Facts taken for this history (beyond the brief): Mac's frames all precede seq 100, so its counter is 60 at seq 100 and at 200 (it authored nothing after 100); Mac's checkpoint is at `B = 100`; the relay's `compact_floor = 129`, `purge_floor = 1`, so `floor = 129`; no control entry lies in 101–200; the `snap` entry announcing `SNAP192` is at seq `x` (`192 < x ≤ 200`, author Desk) and Phone's beat with `acks [{entry x, ack}]` at seq `y` (`x < y ≤ 200`); the queued edit renames "Call plumber" (id `1 × 2^40 + 7 = 1,099,511,627,783`) to "Call the plumber" (the name is a placeholder), enqueued offline at `t_q ≈ T0 − 400 days = 1,760,781,600,000`, so never sealed.

AMBIGUOUS: the scenario's relay state (a floor of 129 with head 200) cannot arise under the specs: `compact` requires `below_seq ≤ head_seq − 16,384` (RELAY-COMPACT-007, relay §12.8 check 2) and the engine never sends more (ENG-COMPACT-002); the steps below take the scenario's state as given.

### Steps

1. **User** (Mac): opens TaskDeck at `T0`.

2. **Mac** (device `open`): lock, read the store in the order of device §7.4; draws **RAND1[16]** = session id; `Loaded{checkpoint (board_seq 100, C100, M_100, log_floor), chains, tail, outbox [Put(rename record)], notices, meta (relay = relay.example, DOOR, home_gen 1), session RAND1}`. (device §7.4, §10.4)

3. **Mac** (engine `Start`): attempt + 1 → `Persist[WriteMeta]` (D5); confirmed state, `M_100`, `C100` from the checkpoint; outbox rebuilt: the record `{entry_id EQ, at_ms t_q, based_on (100, C100), kind cmd, body rename payload, via Append, ctr/frame none}`; pending view = state at 100 + the rename. (engine §7.10, §8.2, §15.2; ENG-STATE-002, ENG-STATE-003)

4. **Mac → Relay** (`Connect`, no backoff): token `TOK_MAC` (gen 1, `push: true`) stored → desktop in the foreground → `wss://relay.example/v1/b/<BID>/socket`, `Bearer TOK_MAC`. Relay: gate (token form, MAC, board, role 1); object: row `MAC` role 1, state 1, `gen 1` ✓; board open → accept. Mac sends hello `0x01 ‖ RAND1 ‖ u32 0`. (device §10.3, §10.5; relay §11.1, §11.3)

5. **Relay**: welcome (93 bytes): `0x81 ‖ u64 200 ‖ C200 ‖ u64 last_ctr 60 ‖ u64 last_seq q_M ‖ i64 nowR ‖ u64 floor 129 ‖ u64 purge_floor 1 ‖ u8 1 ‖ u8 0x01 ‖ u32 3000 ‖ DOOR ‖ u8 0` (`q_M ≤ 100` the seq of Mac's counter 60). (relay §11.3)

6. **Mac** (engine §11.2, §11.3): skew ≈ 0; `head 200 > L 100` (no chain held at 200); no acks; not fenced; reconcile with `r = 60`, `q = q_M`: `a = 60` (`M_100`), `h` none → `a ≤ r` and the frames up to `q_M` are already in the log → normal. `Read{after 100, limit 256}`: socket `0x03 ‖ u32 req ‖ u64 100 ‖ u16 256` (15 bytes). The welcome's `floor 129` is not acted on here (only a page's `truncated` leads to §19.7).
   AMBIGUOUS: engine §11.3 step 2 requires `a ≤ r ≤ h` with `h` "the largest counter of a record with a frame"; Mac holds no record with a frame, so `h` is undefined and, read as 0, step 3 (`r > h`: restored copy) would fire for every device with an empty outbox; taken: `h = a = 60`, the normal case.

7. **Relay** (socket read): re-reads `MAC`'s row (current, `gen 1`); rows `seq > 100`, `≥ purge_floor 1`: none in 101–128 (deleted by compaction; no kept control entry there), rows 129–200 (72; assumed under 1 MiB). Answer `0x83 ‖ u32 req ‖ page`; no write. (relay §10.5, §11.4)

   | Page field | Width | Value |
   |---|---|---|
   | `magic` | 4 | `"TDP1"` |
   | `head_seq` | 8 | 200 |
   | `head_chain` | 32 | `C200` |
   | `relay_now` | 8 | `nowR` |
   | `floor` | 8 | 129 |
   | `purge_floor` | 8 | 1 |
   | `flags` | 1 | `0x02` (`truncated`: `100 + 1 < 129`; `more` 0; not fenced) |
   | `n` | 2 | 72 |
   | entries | Σ(20 + flen) | `u64 seq ‖ i64 at ‖ u32 flen ‖ frame`, seqs 129–200 |

8. **Mac** (engine §12.3): step 1 limits ✓; step 2 clock; step 3 head ✓; step 4 `truncated` and `L + 1 = 101 < floor 129` → §19.7; the page is not processed further (nothing persisted). **This is how Mac learns it cannot catch up from 101.** (engine §12.3 step 4, §19.7)

9. **Mac** (engine §19.7 step 1): enters `Rejoining`: `status.session = Rejoining`, a stop with action `ConfirmRejoin`; `Notify(StatusChanged)`. Nothing is sent (not `Live`). (engine §30.1)
   **User is asked**: the relay no longer holds the history Mac needs (it was away more than 365 days); confirm to replace Mac's local log by a snapshot (words are the desktop's). Editing still works locally.

10. **User** (Mac): confirms: `confirm_rejoin()` → `User(ConfirmRejoin)`. (device §17.7; engine §30.4)

11. **Mac** (engine §19.7 step 2): `Persist[WriteBase(base at 100: board_seq 100, C100, board, M_100, log_floor, registry)]` (file `base-…100.tdk`, new `file_id` and nonce: RAND2[16], RAND3[24]); `Blob{List}`. (ENG-BOOT-001)

12. **Mac → Relay**: `GET /v1/b/<BID>/blobs`, `Bearer TOK_MAC`. Relay: complete blobs ordered by purpose, seq descending, `at` descending, id; first 64: `200 {"blobs": [{"id": "<SNAP192 hex>", "purpose": 1, "seq": 192, "author": "<DESK hex>", "size": Z, "parts": p, "sha256": "<D hex>", "received": p, "complete": true}, …]}`. (relay §10.7 `blob-list`; device §10.5)

13. **Mac**: candidates = complete purpose-1 snapshots with `seq ≥ floor − 1 = 128`, newest first: `SNAP192` first. `Blob{Fetch{SNAP192, p}}` → `GET /v1/b/<BID>/blobs/<SNAP192>` (`blob-meta`, same object as above), then `GET /v1/b/<BID>/blobs/<SNAP192>/i` for `i = 0 … p − 1`; each answered with the part's bytes (`409 relay/blob_incomplete` cannot occur: complete). (engine §19.7 steps 2–3; device §10.5)

14. **Mac** (sealer `open_snapshot(parts, expected{seq 192, blob SNAP192, sha256 D, size Z, parts p}, writer DESK, exact false)`), the declared mode, in order (seal §20.5; SEAL-BLOB-008):
    1. exactly `p = ceil(Z / 1,048,576)` parts, each 1,048,576 bytes but the last (1–1,048,576), total Z (`seal/blob_size`);
    2. `H(concatenation) = D` in constant time, before any decryption (`seal/blob_hash`);
    3. header `"TDB1"`, ver 1, suite 1, `blob_kind 1`; `epoch` read from bytes 7–11 = 1 (`seal/blob_header`);
    4. root of epoch 1 from Mac's own roots (`R1`) → `K_snap,1`; AEAD opens with `AD = ctx("blob")[17] ‖ BID ‖ sealed[0..27] ‖ u64 192 ‖ DESK[16]` (84 bytes) (`seal/blob_open`);
    5. padding valid with `U = 4 + body_len` (`seal/blob_open`);
    6. inflate under a cap of 67,108,864 bytes, no input left after the stream (`seal/inflate`);
    7. body parses with no trailing bytes (`"TDSN" ‖ u64 192 ‖ C192 ‖ u32 1 ‖ SH192 ‖ vb32(board) ‖ vb32(M_192) ‖ u32 3 ‖ history records 1, a_M, a_P`); `S = 192` = declared seq; membership parses; the history verifies by the projection steps of seal §21.6 with Mac's own `BID` and agrees with `M_192` (devices, keys, statuses, current recovery key, epochs and commitments, `ctl_seqs`); the state hash is not compared.
    Returns `(material, epoch 1, inflated I)`; keeps the history for the store.

15. **Mac** (engine §19.7 step 3): `M_192` bytes parse, name `BID`, list `MAC` current; `SM::load(board)` ✓; `SM::rules(board) = 1 = M_192.rules`; `SH192` recomputed (`H(ctx("state") ‖ u32 1 ‖ H(board) ‖ H(M_192))`, 86-byte input) = the body's `state_hash`. Candidate passes.

16. **Mac** (engine §19.7 step 4; durable writes in this order): `Persist #1` = [store: history records of the body with seq above `history.tdk`'s last (none: Mac already holds 1, `a_M`, `a_P`); `WriteCheckpoint{board_seq 192, board_chain C192, board, M_192, log_floor 193, registry}`; `WriteBase(192)`]; after its `Persisted` (D8), `Persist #2` = [`TruncateLogBelow(193, floor_chain C192)`: first `device.tdk` replaced with `log_floor 193`, `floor_chain C192`, then every segment whose last seq is below 193 removed (all of Mac's old segments)]. State: confirmed state and `M_192` from the snapshot, `B = K = L = 192`, `C192`, `log_floor 193`, `retained_from 192`. Files replaced or created draw a `file_id` and record nonces (RAND4–RAND9). (device §7.3, §8.3, DEV-FILE-006, DEV-RUN-006; engine §7.12 D8)
    AMBIGUOUS: §19.7 step 4 lists `TruncateLogBelow(S + 1)` before "a checkpoint and a base", while D8 makes the truncate wait for a checkpoint at or above 192; taken: checkpoint first.
    AMBIGUOUS: whether Mac records a `HashRecord` at 192 from the snapshot (it did not apply 192; §19.7 says nothing); taken: none (so it gives no verdict and its beat carries no `state`).

17. **Mac → Relay**: `Read{after 192, limit 256}` (socket). Relay: rows 193–200; page `"TDP1" ‖ 200 ‖ C200 ‖ nowR ‖ 129 ‖ 1 ‖ flags 0 ‖ n 8 ‖ entries 193–200` (`193 ≥ floor`, not truncated). (relay §10.5)

18. **Mac** (engine §12.3): contiguity (193 = L + 1, consecutive); dry run from `M_192` and `C192`: for each `s`, `C_s = chain_step(C_{s−1}, s, F_s)` and `classify` (seal §14.2 steps 1–17; the `based_on` check of §14.8 compares entries with `based_on_seq ≥ 192` against Mac's chain and sets `based_on_unchecked` for any below 192); last `C200` = page head chain ✓. `Persist[AppendLog[193..200], SetMarker{193, 200, attempt}]` (no control entry → no history record). Commit; apply 193–200 in order: at `x`, Desk's `snap` → `SnapRecord{entry_seq x, author DESK, info, entry_clock τ_x, verdicts [], own none}` (Mac applied no 192 of its own: no verdict, engine §19.5); at `y`, Phone's beat → `PHONE.beat_ms`, lists, and verdict `(PHONE, Ack, y)` added to `x`'s `SnapRecord`; commands applied; at 200 not aligned. (engine §12.3, §12.4, §13.1, §23.3)

19. **Mac** (engine §19.7 step 5): the `snap` entry at `x` names `SNAP192`: author `DESK` = the author it was opened with; `seq 192`, `hash SH192`, `sha256 D`, `size Z`, `parts p`, `inflated I`, `epoch 1` equal what was opened ✓. `L = 200` = relay head: the snapshot is usable (an `Ack` from Phone ≠ writer Desk, no `Mismatch`) ✓ → rejoin succeeds. (engine §19.3, ENG-BOOT-002)

20. **Mac** (engine §19.7 step 7, §8.4): status warning `ContinuityUnchecked{from: 192}`; read run ended → rebase; session `Live`; `last_synced_ms = wall_ms`. **Mac tells the user**: synced; the board's history before seq 192 could not be checked on this device (continuity unchecked from 192), shown until acknowledged; its pending rename is still shown on top of the state at 200.
    AMBIGUOUS: the warning lasts "until acknowledged" (engine §19.7 step 7), but no `UserAction` (§30.4) or facade call acknowledges a status warning.

21. **Mac** (duties, engine §23.1): eligible; `wall_ms − last_beat_ms ≥ 86,400,000` (≈ 400 days) → beat enqueued: **RAND10[16]** = `EB`; `{EB, at_ms t_b ≈ T0, based_on (200, C200), Beat{rules [1], envelopes [(1,1)], hash_versions [1], state none, acks []}}`; `Persist[Outbox(Put)]`.

22. **Mac** (send loop, engine §10.2): `R` = [rename record (oldest `order`), beat]; no frame sealed under an old epoch (gate open); seal the rename with `ctr = 1 + max(60, 60, 60) = 61`, **RAND11[24]** nonce, keeping `entry_id EQ`, `at_ms t_q` and `based_on (100, C100)` (ENG-ENQ-005); seal the beat with `ctr 62`, **RAND12[24]** nonce; `Persist[Outbox(Put(EQ+F201'), Put(EB+F202'))]` (D2); one `Append` over the socket.

    Rename payload (60 bytes): `{"id":1099511627783,"name":"Call the plumber","op":"rename"}`. Plaintext (512): `EQ[16] ‖ u8 1 ‖ i64 t_q ‖ u64 100 ‖ C100 ‖ u32 60 ‖ payload ‖ 0x00 × 383`. Frame (670): `"TDF1" ‖ BID ‖ MAC ‖ u64 61 ‖ u32 558 ‖ (u8 1 ‖ u8 1 ‖ u32 1 ‖ RAND11 ‖ AEAD(K_entry,1; RAND11; ctx("entry") ‖ frame[0..48] ‖ 1 ‖ 1 ‖ u32 1; plaintext)[528]) ‖ SIG(sk(MAC_SIGN); "taskdeck/v1/frame\0"; frame[0..606])`.

    Beat payload (53 bytes): `{"envelopes":[[1,1]],"hash_versions":[1],"rules":[1]}`; plaintext `EB ‖ u8 3 ‖ i64 t_b ‖ u64 200 ‖ C200 ‖ u32 53 ‖ payload ‖ 0x00 × 390`; frame 670 bytes, `ctr 62`, as above.

    Socket append (1358 bytes): `0x02 ‖ u32 req ‖ "TDA1" ‖ u8 2 ‖ u32 670 ‖ F201' ‖ u32 670 ‖ F202'`.

23. **Relay**: re-reads `MAC`'s row; for each frame P1–P7 (author row `MAC`, `dev` = token's, state 1, strict signature); transaction: T1; T2 `61 = 60 + 1` → T4 → T5 `seq 201`, `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ H(F201'))`; then `62` → `seq 202`, `C202`; `MAC` row written once: `last_ctr 62`, `last_seq 202`, `app_count 2`. Result `0x82 ‖ u32 req ‖ "TDR1" ‖ u8 2 ‖ (0x00 ‖ u64 201 ‖ C201) ‖ (0x00 ‖ u64 202 ‖ C202)` (92 bytes); push 201–202 to every ready socket (Desk, Mac). (relay §9.3–§9.6, §11.4)

24. **Mac**: acks `(201, C201)`, `(202, C202)`; push → classify 201 from `M_200`: own `dev`, `own_sealed(61)` = its hash; `based_on_seq 100 < retained_from 192` → check skipped, flag `based_on_unchecked` (status warning `BasedOnUnchecked{count 1}`, never an alarm); `Accept`; `apply rename` under `Ctx{seq 201, device_no 2, at t_q, based_on 100}`: name set, stamp `name = (201, 2)`; if the item's `name` stamp is another device's change after seq 100, the rename still applies and produces an overwrite notice (recorded on Mac, the author, and shown on the other device), else nothing. 202 accepted (beat). Both `seen`, `Drop(EQ)`, `Drop(EB)`; `B = 202`. (seal §14.8, SEAL-RCPT-013; core §10.1, §10.2; engine §13.4, §12.4)

25. **Desk** (and Phone when it next reads): classify 201: `based_on 100`, Desk holds `C100` (its log keeps the last 16,384 seqs) and it matches → `Accept`; same `apply`, same result; 202 → `MAC.beat_ms = max(…, t_b)`. (seal §14.8; engine §7.13)

### State after the last step

| | Mac | Desk | Relay |
|---|---|---|---|
| head | 202, `C202` | 202, `C202` | `head_seq 202`, `C202`; floor 129, `purge_floor 1` |
| board head `B` | 202 | 202 | — |
| log | `log_floor 193`, records 193–202, `retained_from 192` | its log to 202 | rows ≥ 129 (and kept control entries below), to 202 |
| counters | Mac 62 | Desk unchanged | `MAC.last_ctr 62`, `last_seq 202`, `app_count 2` |
| epoch | 1 | 1 | — |
| outbox | empty | empty | — |
| durable | `device.tdk` `log_floor 193`, `floor_chain C192`; checkpoint at 192 (later ones by cadence); bases at 100 (pre-rejoin) and 192, plus older bases until retention drops them; `history.tdk` unchanged (1, `a_M`, `a_P`); log segments 193–202; outbox Put/Drop of EQ and EB; meta with the session state | log 201–202 | log rows 201–202 |

What the user saw on Mac: the board as of seq 100 with its own rename pending, then the rejoin prompt; after confirming, the board at 200 and then 202 with "Call the plumber", the warning "continuity unchecked from seq 192" (and one entry whose `based_on` could not be checked), and an overwrite notice only if another device had renamed the task after seq 100.
