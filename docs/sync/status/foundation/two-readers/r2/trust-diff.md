# Differences

Group `trust`, round 2. A = reader-A/trust.md, B = reader-B/trust.md. Both readers agree on every byte layout,
width, context string, counter, seq and chain value they both give (revoke envelope 136 bytes with a 121-byte
signed message; wrap `info` 85, `aad` 61, wrap 96; control payload 919; plaintext U 988 / P 1024; rotation frame
1,182 sealed under epoch 1; seqs 201 (Desk, ctr 122) and 202 (Mac, ctr 61); Mac's frame accepted by the
stale-epoch rule, not re-sealed; Phone refused `403 relay/revoked`; restore_tail args 332 bytes with a 127-byte
signed message; relay rows after restore Desk 121, Mac 60, Phone 18; "Pay rent" at seq 201 ctr 122).

1. **Draw order of a rotation's random values.**
   A (sc. 4 step 7): "Draws in order: `RAND2[32]` = `R_2`; one 32-byte ikm per wrap in ascending recipient-id order of `{DESK, MAC, dev_R}`: `RAND3`, `RAND4`, `RAND5`; then `RAND6[24]` frame nonce (ENG-SEALER-008)."
   B (sc. 4 step 7): same draws, then "AMBIGUOUS: the order of the draws `R_2`, the three wrap `ikm`s and the nonce within one `seal` of a rotation is fixed nowhere (seal.md §6.5, §7 "in the order the wraps are made", SEAL-GEN-012); and the order in which the wraps are *made* (before or after sorting by `recipient_id`) is not stated."
   Spec: engine.md §6.1 `seal` row ("draws its random values in this order: the root …, then the wraps in ascending recipient-id order …, then the frame's nonce") and ENG-SEALER-008. seal.md §7 "The ephemeral key" says only "in the order the wraps are made".
   Verdict: **reader B, clarified.** The order is fixed in engine.md; B looked only in seal.md, whose sentence "in the order the wraps are made" invites the doubt. TR2-TRUST-1 points seal.md §7 at that order.

2. **Which code a revoked row with an old token generation gets.**
   A (sc. 4 step 18): "AMBIGUOUS: relay.md §8.2 lists `state = 2` before `gen differs`; I take that order, so the stale token (gen 1 ≠ 2) still answers `revoked`, not `token_stale`."
   B (sc. 4 step 20): "Object: row `PHONE` `state 2` → `403 relay/revoked` JSON", no remark.
   Spec: relay.md §8.2 table row "token, role 1" and RELAY-OBJ-003 ("answer with the code the table gives when it fails"). The revoke of §12.4 sets both `state 2` and `gen 2`, so Phone's token fails two conditions; nothing says which code wins. With `token_stale` the device would log in (device.md §10.3 case 2) before meeting `revoked`, a different message sequence.
   Verdict: **spec.** TR2-TRUST-2 fixes the precedence (the order the row lists, which both readers followed).

3. **What a device revoked at the relay can read.**
   A (sc. 4 steps 17–18): reconnects once per hour, logs in first, `login-nonce` → `403 relay/revoked`; "It never reads entry 201 (its token is refused), so it does not see `DeviceRevoked` itself."
   B (sc. 4 step 23): same steps, then "AMBIGUOUS: engine.md §24.4 says a revoked device "then only reads", and that a mistaken `relay` source "clears itself when the relay answers again", but relay.md §8.2 answers every request of a revoked row with `403 relay/revoked` (login included, §10.3); so a relay-revoked device can never read the log, never learn `DeviceRevoked` at 201 … the spec does not say what it shows beyond "the relay says"."
   Spec: engine.md §24.4 paragraph "A revoked device sends nothing …" ("then only reads"); relay.md §8.2 (rows "token, role 1" and "on `login-nonce` and `login`": `state = 2` refused there too).
   Verdict: **spec.** The two readers agree on the outcome, but "then only reads" contradicts relay.md §8.2 for `source: relay`, and the spec does not say that the state and its message stay as they are under repeated refusals. TR2-TRUST-3.

4. **Hello after the evidence stop while the socket is open.**
   A (sc. 5 step 7): "session `Connecting` → `Hello` at once (ENG-SESS-010). … `GET …/hello`".
   B (sc. 5 step 11): the same `GET …/hello`, then "AMBIGUOUS: device.md §10.5 maps every `Hello` to `GET …/hello` over HTTPS, while Desk's socket is open and already welcomed; the spec does not say whether the welcome needed after the stop comes from HTTPS `hello` on a live socket or whether the socket must be reopened."
   Spec: device.md §10.5 row `Hello` ("`GET …/hello` with `T`", no socket variant, unlike `Read` and `Append`) and DEV-NET-007 ("exactly the requests of the table"); ENG-SESS-010.
   Verdict: **reader B (plain).** The table gives one request for `Hello` whatever the socket's state, and nothing closes the socket; both readers did the same.

5. **Whether the lost `SNAP192` blob makes a new snapshot due.**
   A (sc. 5 step 9): "AMBIGUOUS: whether §19.2 makes a new snapshot due: the lost snapshot is still "usable" in Desk's registry, so condition 1 does not fire before 7 days or 1,000 entries."
   B (sc. 5 state table): "blob `SNAP192` lost and not re-uploaded (§21.3 "Blobs"); devices' `SnapRecord` of it stays, a later `compact` naming it gets `409 relay/no_snapshot`." No new snapshot either.
   Spec: engine.md §19.2 condition 1 and §19.3 (usable = verdicts only, not blob presence); relay.md §12.9 last paragraph; lead ruling T8.
   Verdict: **reader A (plain).** §19.3 defines "usable" by verdicts alone, so condition 1 does not fire; A wrote that reading. It follows from T8 (see "For the lead").

6. **The relay copy's author rows.**
   A (sc. 5 head): "AMBIGUOUS: the relay's author rows are those of its older copy (at 190); the world gives only the log, I derive the counters from "Desk six, Mac three, Phone one"" → Desk 115, Mac 57, Phone 17.
   B (sc. 5 world additions): the same values, no remark.
   Spec: none; a question about the brief's world.
   Verdict: **format.** Both derived the same values; not a spec question (see "For the lead").

7. **An edit made while the stop still holds.**
   A (sc. 5 step 10): "AMBIGUOUS: if the restore had not completed by T0 + 30 s, the edit is queued under the stop and sealed after it ends (ENG-EVID-003); the outcome is the same."
   B (sc. 5 step 13): "(Had the stop still held at `T0 + 30 s`, the edit would be queued and shown, and sent after step 11; ENG-EVID-003.)"
   Spec: ENG-EVID-003, engine.md §21.2.
   Verdict: **reader A (plain).** The spec covers both timings; the open point is only the world's timing, and the outcome is the same.

8. **Phone's message.**
   A (sc. 4 step 18): "the relay says this phone was removed". B (sc. 4 step 22): "the relay says this device was removed".
   Spec: engine.md §24.4 ("the relay says this device was removed").
   Verdict: **reader A (plain).** A paraphrased the quoted text.

9. **Order of Desk's and Mac's handling of 201 and 202.**
   A: Desk classifies 201 and 202 (steps 11–12), then Mac (13–14). B: Mac first (14–18), then Desk (19).
   Verdict: **format.** The two devices act concurrently; each device's own order is the same in both.

10. **Desk's ack and drop of its rotation record.**
    A (sc. 4 step 11): "page `Persist{AppendLog, SetMarker, WriteIdentity, Outbox(ack Put)}`", no `Drop` named. B (step 19): "result 201 → ack; push 201 → … `Persist[AppendLog, SetMarker, WriteIdentity …]`; … `seen` → `Drop(RR)`".
    Spec: engine.md §10.4, §12.3 step 8 (as edit-diff difference 22).
    Verdict: **format.** Each omits a detail the other gives; no contrary step.

11. **Push and response of `restore_tail`.**
    A (sc. 5 step 6): "Response …; push 191–200 to ready sockets". B (step 9): "after commit push 191..200 …; answer …".
    Spec: relay.md §12.9 ("After commit: push. Response (200)"); RELAY-OBJ-007.
    Verdict: **format.** Both after the commit, on different channels; neither depends on the order.

12. **Details one reader gives and the other omits.**
    `at_ms` of the rotation record: A `Tr`, B "`wall_ms ≈ T0`"; A "Rows written `2 + r`" (revoke) and "`1 + 20 + 3 = 24`" (restore_tail), B none; B "close Phone's sockets with 4003 (none open)", A "No Phone socket to close"; A the bridge `sync` event (`stopped: true`, `queued 1`), B none; Desk's operation "done" (A) / "op record cleared" (B).
    Verdict: **format.**

# Changes

## seal.md

### TR2-TRUST-1 (difference 1)

Section §7, paragraph **The ephemeral key.**, replaced in full by:

**The ephemeral key.** `SealBase` makes its ephemeral key with RFC 9180's `GenerateKeyPair`: `wrap_root` draws exactly 32 bytes `ikm` from its `Entropy` argument and takes the key pair `DeriveKeyPair(ikm)` (RFC 9180 §7.1.3). It draws nothing else. The wraps of one rotation are made in ascending recipient-id order, the order in which they stand in the item (§12.5); so within the one sealing call of a rotation the root is drawn first, then the 32 bytes of each wrap in that order, then the frame's nonce (`engine.md` §6.1 `seal`, ENG-SEALER-008). A test vector (§24) gives those 32 bytes for each wrap, in that order.

(No new clause: the order is the rule of ENG-SEALER-008, owner `device`, lead ruling T9.)

## relay.md

### TR2-TRUST-2 (difference 2)

Section §8.2 "Credentials at the object": after the table, before RELAY-OBJ-003, add the paragraph:

The object checks the conditions of a row in the order the row lists them and answers with the code of the first that fails. So a role-1 token whose row has `state = 2` is answered `403 relay/revoked` whatever its `gen`, on every route, `login-nonce` and `login` included; `401 relay/token_stale` is answered only for a row with `state = 1`.

After RELAY-OBJ-003, add the clause:

- **RELAY-OBJ-010** (relay) When more than one condition of a §8.2 row fails, the object MUST answer with the code of the first failing condition in the order the row lists them, so that a token of a device whose row has `state = 2` is answered `403 relay/revoked` whatever its generation.

## engine.md

### TR2-TRUST-3 (difference 3)

Section §24.4 "A revoked device", the paragraph "A revoked device sends nothing, …" replaced in full by:

A revoked device sends nothing, takes no duty, keeps every record, and shows "this device was removed by <author> at <seq>" (or "the relay says this device was removed") with the action "re-pair and resubmit my unsent edits" (SYNC.md §5.4, V2-21). It reconnects at most once per 3,600,000 ms of `mono_ms` (§11.1) and then reads only: it emits no `Append`, `Admin` or blob upload (§10.1), and the runtime logs in again first, since a revocation at the relay raised its token generation (`relay.md` §5.2, §12.5). While the relay holds the device's row revoked, that login and every read are refused with `403 relay/revoked` (`relay.md` §8.2), and each such refusal leaves the revoked state as it is: a device revoked with `source: relay` reads nothing, does not classify the revoking entry, and keeps `source: relay` and the message "the relay says this device was removed" until one of the cases below ends it. So a mistaken `relay` source clears itself when the relay answers again; a `DeviceReinstated` naming it clears it as well, classified directly or after the missed roots of ruling S24 arrive (below).

## For the lead

1. **Difference 5 follows from ruling T8** ("blobs lost to a relay rollback are not uploaded again; the existing fall-backs apply"): a snapshot whose blob the rollback lost stays usable in every registry (§19.3 counts verdicts only), so §19.2 makes no new snapshot due, and a compaction naming it is refused `409 relay/no_snapshot`. No change written; no ruling needed unless the lead wants a lost blob to make a snapshot due, which would revise T8.
2. **World facts of scenario 5 the brief leaves open (differences 6, 7).** Not spec questions, but they produced two AMBIGUOUS lines. Recommendation: add to scenario 5: "The relay's author rows are those of its copy at seq 190: Desk `last_ctr 115`, Mac 57, Phone 17. The restore completes before `T0 + 30 s`."
