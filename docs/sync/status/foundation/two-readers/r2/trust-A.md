# Scenario 4. Revocation with rotation

`dev_R = H(ctx("recovery-dev") ‖ BID ‖ RSIGN_pk)[0..16]` (seal.md §6.3). Phone `device_no 3`.

1. **User (Mac, T0 − 5 s)**: renames "Call plumber" to "Call the plumber". Mac compiles `rename{id 1099511627783, name "Call the plumber"}` (canonical `{"id":1099511627783,"name":"Call the plumber","op":"rename"}`, 60 bytes); `Enqueue` with `based_on (200, C200)`, `at_ms T0 − 5,000`; entry_id `M1` (Mac's RAND); persisted; sealed `ctr 61`, epoch 1 (Mac's `M_K` is `M_200`); frame 670 bytes; `Append` sent. It reaches the relay after step 9.
2. **User (Desk, T0)**: Devices → Phone → Remove; Windows Hello (auth); `revoke(PHONE, replace false)` → `Op(Revoke{PHONE, false})` (device.md §17.8). Engine: PHONE current, not this device, not the last current device (engine.md §28.5).
3. **Desk (engine)**: stage 1 `Persist{WriteMeta(ops += OpRecord{ticket, Revoke, stage 1})}` (D10); then `Admin{Revoke{PHONE}}`.
4. **Desk → Relay**: `POST …/admin/nonce` body `DESK` with token → `{"nonce": N1, …}`; `POST …/admin/revoke`, token, body (136 bytes):

   | Field | Width | Value |
   |---|---|---|
   | `signer` | 16 | DESK |
   | `nonce` | 32 | N1 |
   | `sig` | 64 | `SIG(DESK_SIGN; "taskdeck/v1/admin\0"; vb8("revoke") ‖ BID ‖ DESK ‖ H(PHONE) ‖ N1)` (115 + 6 = 121 bytes after the context) |
   | `args_len` | 4 | 16 |
   | `args` | 16 | PHONE |
   | `extra_len` | 4 | 0 |
5. **Relay**: A1–A6 (nonce deleted at A4); target row role 1 state 1; signer ≠ target; one current role-1 row remains. One transaction: PHONE `state 2`, `gen 2`, `wraps` NULL; every `read_tokens` row `state 2`. Rows written `2 + r`. 200 `{}`. No Phone socket to close.
6. **Desk (engine)**: `AdminResult Ok` → stage 2: one `Persist{WriteMeta(stage 2), Outbox([Put(record R)])}` with `R = {RAND1[16] entry_id, at_ms Tr, based_on (200, C200), Ctl(Rotate{revoke [PHONE], reinstate [], purge_before None}), via Append}` (ENG-OPS-009).
7. **Desk (send loop)**: seals `R`, ctr 122. Sealer composes, from `M_200` and the working copy `W`: item 1 `DeviceRevoked{PHONE}`; item 2 `EpochRotated{epoch 2}` wrapping to the current devices of `W` (DESK, MAC) and `dev_R`. Draws in order: `RAND2[32]` = `R_2`; one 32-byte ikm per wrap in ascending recipient-id order of `{DESK, MAC, dev_R}`: `RAND3`, `RAND4`, `RAND5`; then `RAND6[24]` frame nonce (ENG-SEALER-008).
   - `c_2 = H(ctx("epoch") ‖ BID ‖ u32 2 ‖ R_2)`
   - wrap to `(id, pk)`: `HPKE(pk; info = ctx("wrap") ‖ BID ‖ u32 2 ‖ id ‖ pk; aad = ctx("wrap-aad") ‖ 0x01 ‖ 0x01 ‖ 0x0020 ‖ 0x0001 ‖ 0x0003 ‖ c_2; R_2)`, laid out `id[16] ‖ enc[32] ‖ ct[48]` (seal.md §7); `pk` = DESK_HPKE, MAC_HPKE, RHPKE.
   - Items (canonical, 772 bytes): `[{"dev":"<PHONE>","type":"DeviceRevoked"},{"commit":"<c_2>","epoch":2,"type":"EpochRotated","wraps":["<w1>","<w2>","<w3>"]}]`, wraps as 192 hex each, ascending by `recipient_id`.
   - Inner signature `SIG(DESK_SIGN; "taskdeck/v1/ctl\0"; BID ‖ DESK ‖ u64 122 ‖ RAND1 ‖ i64 Tr ‖ H(items_bytes))`; payload `{"items":[…],"sig":"<128 hex>"}`, 919 bytes.
   - Plaintext `RAND1 ‖ 0x02 ‖ i64 Tr ‖ u64 200 ‖ C200 ‖ u32 919 ‖ payload ‖ 0x00 × 36` (U 988, P 1024).
   - Frame (1,182 bytes): `"TDF1" ‖ BID ‖ DESK ‖ u64 122 ‖ u32 1070 ‖ 1 ‖ 1 ‖ u32 1 (sealed under epoch 1, SEAL-ENV-004) ‖ RAND6 ‖ AEAD(K_entry,1; RAND6; ctx("entry") ‖ header ‖ 1 ‖ 1 ‖ u32 1; pt) ‖ SIG(DESK_SIGN; "taskdeck/v1/frame\0"; frame[0..1118])`.
   - The pending root `(RAND1, 2, R_2)` is in the identity export; `Persist{Outbox([Put R framed]), WriteIdentity}` (D2), then `Append`.
8. **Desk → Relay** (socket): append `0x02 ‖ r ‖ "TDA1" ‖ 1 ‖ u32 1182 ‖ frame`.
9. **Relay**: P1–P7, T2 `122 = 121 + 1` → seq 201, `C201 = H(ctx("chain") ‖ C200 ‖ u64 201 ‖ H(frame))`; result `new 201` to Desk, push 201 to Desk and Mac.
10. **Relay** (Mac's append, just after): Mac row current; P1–P7; `61 = 60 + 1` → seq 202, `C202`; result `new 202` to Mac; push 202 to Desk and Mac. (The relay does not see epochs.)
11. **Desk**: classifies 201: Accept, shape R; `DeviceRevoked` by a current device other than the target, ≥ 1 current device left; `EpochRotated` `2 = 1 + 1`, wraps name exactly DESK, MAC, `dev_R`; own rotation → opens its own wrap, checks `c_2`, keeps `R_2` as root 2, removes the pending root (ENG-SEALER-009) → `keys_changed`; page `Persist{AppendLog, SetMarker, WriteIdentity, Outbox(ack Put)}`. `M_201`: PHONE `revoked, status_seq 201, status_by DESK`; `epoch 2, epoch_seq 201, epoch_clock_ms τ201`; commitments `+(2, 201, c_2)`; `ctl_seqs + 201`; history record 201. No banner on Desk (author). Op stage 3: accepted → `OpDone{Ok}`.
12. **Desk**: classifies 202: step 9 `epoch 1 = E − 1` with `E = 2` → continue; step 10 opens under `K_entry,1`; step 14 stale-epoch rule: cmd, author MAC current, `based_on_seq 200 < epoch_seq 201`, `clock_ms − epoch_clock_ms ≤ 2,592,000,000` → Accept (seal.md §13.9). Apply `rename` `Ctx{202, 2, T0 − 5 s, 200}`: no notice (name stamp ≤ 200).
13. **Mac**: on the push of 201: classify Accept; recipient of the rotation → opens its wrap (`info` and `aad` rebuilt from its own BID, epoch 2, MAC, MAC_HPKE, `c_2`), `H(ctx("epoch") ‖ BID ‖ u32 2 ‖ R_2) = c_2` (SEAL-RCPT-007) → learns `R_2`; `Persist{AppendLog[201], SetMarker, WriteIdentity}` (D4); banner `DeviceRevoked{author DESK, subject PHONE}` (no `KeysRotated`: the entry has a revoke item). Epoch gate: Mac's record `M1` (epoch 1, unseen) → no new seal until it is seen (ENG-EPOCH-001).
14. **Mac**: result `new 202` → `M1.ack = (202, C202)`; push 202: classify as step 12 → Accept; apply rename; own entry seen → `Drop(M1)`; echo check ok. Mac's frame is accepted, not re-sealed (it would have been re-sealed under epoch 2 only on `seal/stale_epoch`, §24.3). Epoch gate opens; next seals under epoch 2.
15. **Phone (T0 + 1 h)**: user opens the app; `Start`; one record `P1` in its outbox (made at T0 − 1 h, `based_on (200, C200)`, unsealed). `Connect`: stored token gen 1 with push, not revoked → no login; socket upgrade with `Bearer` token.
16. **Relay**: gate passes (MAC valid); object (§8.2): PHONE row `state 2` → `403 relay/revoked` JSON, no socket.
17. **Phone (transport)**: `RelayRefused{req None, 403 relay/revoked}`, then `Disconnected{unreachable false}` (device.md §10.6). **Engine**: §11.9 → revoked, `source: relay` (§24.4 case 2): stop `Revoked{by_log false}`; sends nothing; keeps log, outbox (`P1`), keys, notices; `Enqueue` still accepted (only a log revocation refuses, §9.1). Reconnects at most once per 3,600,000 ms; each such `Connect` logs in first (device.md §10.3 case 4): `login-nonce` → `403 relay/revoked` → same events.
18. **Phone (screen)**: sync line: "the relay says this phone was removed"; bridge `sync` event `stopped: true`, `queued 1`; Devices screen stop with action `RePair` ("re-pair and resubmit my unsent edits"). It never reads entry 201 (its token is refused), so it does not see `DeviceRevoked` itself.
    AMBIGUOUS: relay.md §8.2 lists `state = 2` before `gen differs`; I take that order, so the stale token (gen 1 ≠ 2) still answers `revoked`, not `token_stale`.

**State after.** Relay: PHONE `state 2, gen 2`; read tokens revoked; head `(202, C202)`; DESK `last_ctr 122`, MAC 61. Desk and Mac: head 202, `M_202` epoch 2 with roots `R_1`, `R_2`, PHONE revoked; Desk's op done; Mac banner unacknowledged. Phone: head 200, epoch 1, stop `Revoked` (source relay), outbox `[P1]`.

# Scenario 5. Relay rollback

Relay rows after the restore: log 1–190; DESK `last_ctr 115`, MAC `57`, PHONE `17`.
AMBIGUOUS: the relay's author rows are those of its older copy (at 190); the world gives only the log, I derive the counters from "Desk six, Mac three, Phone one" of 191–200.

1. **Desk (T0)**: socket reconnects (transport backoff, then upgrade with token, hello `0x01 ‖ session ‖ u32 0`).
2. **Relay → Desk**: welcome `0x81 ‖ u64 190 ‖ C190 ‖ u64 115 ‖ u64 q115 ‖ relay_now ‖ u64 1 ‖ u64 1 ‖ 1 ‖ 1 ‖ appends_left ‖ door ‖ 0`.
3. **Desk (engine, §11.2)**: (1) skew; (2) `190 < L = 200` → `relay_behind`; chain at 190 held = `C190` → no fork; (3) no acknowledged record; (4) not fenced; (5) reconcile step 1 only: `r = 115 < a = 121` → `relay_behind` (same kind). One alarm `RelayBehind{relay_head (190, C190), relay_last_ctr 115, local_head (200, C200), ack None}` persisted in `Meta` (`WriteMeta`) before `Notify(AlarmRaised)` (ENG-EVID-002); evidence stop (§21.2): no `Append`, no read, editing continues, no duties. Session stays `Connecting`. Status: stop `Evidence`, action `RestoreTail`.
4. **Desk (repair, §21.3)**: relay head chain equals its own `C190` → plain rollback. `ReadLog{from 191, to 200}` → frames 191–200 (10 frames, no control entry among them). One request (≤ 64 frames, ≤ 1 MiB).
5. **Desk → Relay**: `POST …/admin/nonce` (DESK) → N2; `POST …/admin/restore_tail`, token:

   | Field | Width | Value |
   |---|---|---|
   | `signer` | 16 | DESK |
   | `nonce` | 32 | N2 |
   | `sig` | 64 | `SIG(DESK_SIGN; "taskdeck/v1/admin\0"; vb8("restore_tail") ‖ BID ‖ DESK ‖ H(args) ‖ N2)` |
   | `args_len` | 4 | 332 |
   | `args` | 332 | `u64 191 ‖ u32 10 ‖ H(f191) ‖ … ‖ H(f200)` |
   | `extra_len` | 4 | `32 + Σ(4 + len_s)` |
   | `extra` | | `C190 ‖ 10 × (u32 len_s ‖ f_s)`, frames byte for byte from the log |
6. **Relay**: A1–A6; `191 = head + 1`; `C190 = head_chain`; per frame in order: hash = signed list; P1–P3; author row exists; `verifyStrict`; `ctr = last_ctr + 1` counting earlier frames of the request (DESK 116–121, MAC 58–60, PHONE 18, interleaved as in the log). Transaction: 10 log rows at their original seqs, chain recomputed `C_s = H(ctx("chain") ‖ C_{s−1} ‖ u64 s ‖ H(f_s))`, three author rows; no budget. Rows written `1 + 20 + 3 = 24`. Response `{"head":{"seq":200,"chain":"<C200>"}}`; push 191–200 to ready sockets (Desk's).
7. **Desk**: answer head `(200, C200)` = `L` with its own chain, no ack to echo → evidence resolved, alarm `resolved`, stop ends (§21.3 step 5, ENG-EVID-005); session `Connecting` → `Hello` at once (ENG-SESS-010). The pushed entries are all ≤ L with equal frames → ignored (§12.3 step 5). `GET …/hello` → `head (200, C200)`, `last_ctr 121`; checks pass; head = L → `Live`.
8. **Desk: evidence shown**: the alarm with its evidence (relay head 190 and `last_ctr 115` against local head 200), while the stop lasted; afterwards resolved.
9. **Blobs**: `SNAP192` was uploaded after seq 192 existed, so the restored relay lacks it; its `snap` entry (in 193–200) is restored, the blob is not (relay.md §12.9). Desk keeps the `SnapRecord`; a later compaction naming it gets `409 relay/no_snapshot`.
   AMBIGUOUS: whether §19.2 makes a new snapshot due: the lost snapshot is still "usable" in Desk's registry, so condition 1 does not fire before 7 days or 1,000 entries.
10. **User (Desk, T0 + 30 s)**: quick add "Pay rent". As scenario 1 steps 2–12, with `now = at_ms = T0 + 30,000 = 1,795,341,630,000`: id `2^40 + 41`, `based_on (200, C200)`, ctr 122, frame 670 bytes; append → relay `122 = 121 + 1` → seq 201, `C201`; result and push.
    AMBIGUOUS: if the restore had not completed by T0 + 30 s, the edit is queued under the stop and sealed after it ends (ENG-EVID-003); the outcome is the same.
11. **Mac (T0 + 60 s)**: reconnects; welcome `head (201, C201), last_ctr 60, last_seq q60` (its three frames restored); `201 ≥ L = 200`; reconcile `a = 60 = r = h` normal; `Read{after 200}` → entry 201 → classify Accept, apply "Pay rent"; `Live`. No alarm on Mac.
12. **Phone**: offline; on return it catches up from 200 normally.

**State after.** Relay: log 1–201, head `(201, C201)`, DESK `last_ctr 122`, MAC 60, PHONE 18; no `SNAP192` blob. Desk: head 201, alarm `RelayBehind` resolved, outbox empty. Mac: head 201. Phone: head 200. User on Desk saw the relay-behind evidence and the restore, then "Pay rent".
