# Two-readers round 1, group `join`: differences and changes

Readers: `join-A.md` (A), `join-B.md` (B). Scenarios 3 (pairing) and 6 (snapshot bootstrap). Step numbers are
the readers' own ("A s3.23" = reader A, scenario 3, step 23). The two agree on every byte layout they both
give: the code, `ticket_key`, `tid`, `mac_key`, `commit`, slots A to D, the transcript (440 bytes), the SAS,
the bundle (`665 + d(I) + d(Z)` bytes), the snapshot body and sealed blob, the `DeviceAdded` items (332),
inner signature (112), payload (479), plaintext (1,024) and frame (1,182), the admin message (123) and
`register` body (1,382), the token (69), the push (1,274), the log page (1,273), the hello, welcome and read,
Tab's beat and "Hello" frames (670 each), the rejoin's pages, `open_snapshot` checks, the rename and beat
frames and the socket append (1,358). Every counter, seq and chain value agrees. The differences are in what
the specs leave open, in two lengths, and in a few omissions.

Changes already written in this round that this file builds on: TR1-EDIT-2 (`h` in §10.3 and §11.3),
TR1-EDIT-4 (`ClearMarker`), TR1-EDIT-7 (the page's `deadline`), TR1-FOLLOW-2 (`TruncateLogBelow(seq,
floor_chain)`, §19.7 step 4) and TR1-FOLLOW-3 (`history.tdk` before `bootstrap.tdk`).

# Differences

**Scenario 3: pairing**

1. **How an ephemeral X25519 key pair is made.**
   A s3.3: "AMBIGUOUS: seal §17.1/§17.10 say 'draw `esk_E`' but do not say how the ephemeral X25519 pair is
   made (32 raw bytes used as an RFC 7748 scalar, or HPKE `DeriveKeyPair` as §6.4 does for long-term keys);
   taken: 32 raw bytes, `epk_E = X25519(esk_E, 9)`." A s3.17 repeats it for `esk_N`.
   B s3.3: "`RAND2[32]` = `esk_E` (ephemeral X25519 secret; `epk_E` its public key)", no doubt raised.
   Spec: seal.md §17.1 steps 2 and 4, §17.7 (`dh = X25519(esk_E, epk_N)`), §17.10 rows `Idle` (E) and
   `AwaitA` (N). §6.4 makes the long-term X25519 key with `DeriveKeyPair`; nothing says how `esk`/`epk` are
   made, and the vectors of §29 (`seal-pairing.json`) take "ephemeral keys" as inputs and the entropy in draw
   order, so two implementations that differ here produce different slots A to C.
   Verdict: **spec.** TR1-JOIN-1.

2. **The order in which N draws its long-term keys and `esk_N`.**
   A s3.17: "AMBIGUOUS: the draw order of `dev`, the signing key and the X25519 ikm (seal §6.4 lists them in a
   table, not as an order) and how `esk_N` is made (as step 3); taken: table order, then `esk_N`".
   B s3.15: "AMBIGUOUS: the draw order of the three long-term keys and `esk_N` is not fixed (seal §17.10
   'generate long-term keys and `esk_N`'; §6.4 table order taken)."
   Spec: seal.md §6.4 (a table), §17.10 row `AwaitA`; §29 ("the bytes in the order they are drawn"). E's
   order is given (§17.10 row `Idle`: "draw `s`, `esk_E`, `r_E`"); N's is not.
   Verdict: **spec.** The readers agree, but the vectors depend on it. TR1-JOIN-1.

3. **The length of slot A's MAC input.**
   A s3.5: "MAC input 17+1+1+16+2+48 = 85 bytes".
   B s3.5: "`MAC(mac_key; "taskdeck/v1/pair\0"(17) ‖ 0x41 ‖ 0x45 ‖ tid(16) ‖ u16 48 ‖ body(48))` (115-byte
   input)".
   Spec: seal.md §17.4, the `mac` row: `ctx("pair") ‖ label ‖ role ‖ tid[16] ‖ u16 body_len ‖ body`, which is
   17 + 1 + 1 + 16 + 2 + 48 = 85 bytes. B's own field list sums to 85.
   Verdict: **reader (B).** An arithmetic slip; no change.

4. **Random values of the store's writes on Desk.**
   A s3.33 to s3.35: `RAND10` entry id, `RAND11` frame nonce, `RAND12` the relay's admin nonce; the outbox
   records' nonces are not named.
   B s3.35 to s3.37: `RAND10` entry id, `RAND11[24]` the outbox record's nonce, `RAND12` frame nonce,
   `RAND13[24]` the second outbox record's nonce, `RAND14` admin nonce.
   Spec: device.md §6.3 and seal.md §19.3 (every sealed record has a fresh 24-byte nonce from `Entropy`);
   the brief asks for every draw. Neither reader names the nonces of Desk's and Mac's log-segment and
   history records for 201, nor Mac's `meta.tdk` rewrite.
   Verdict: **reader (A)** for the outbox nonces that B names; the shared omissions change no byte and no
   order. No change.

5. **Whether the `DeviceAdded` is enqueued only after the snapshot upload succeeded.**
   A s3.32: "AMBIGUOUS: engine §28.4 step 2 does not say whether the `DeviceAdded` is enqueued only after
   `BlobResult{Uploaded}` or at once after step 1, nor what an upload failure does; taken: enqueued after the
   upload succeeded."
   B s3.35: "AMBIGUOUS: engine §28.4 lists the upload (step 1) before the enqueue (step 2) but does not say
   whether the enqueue waits for `BlobResult{Uploaded}`; I take it that it does."
   Spec: engine.md §28.4 steps 1 and 2. For a `snap` entry ENG-SNAP-004 makes the announcement wait for the
   upload; for a pairing nothing does, and a failed upload has no outcome.
   Verdict: **spec.** TR1-JOIN-2.

6. **The `register` answer and the push of 201 reach Desk in either order.**
   A s3.42: "Desk (push of 201, before or after step 39; nothing orders the HTTP answer and the push)".
   B s3.40: "AMBIGUOUS: relay §12.3 says 'After commit, push' but not whether the push is sent before or
   after the HTTPS answer; the two reach Desk in either order (steps 41 and 42 commute)."
   Spec: relay.md §12.3 ("After commit, push"); engine.md §11.7 ("A `Register` or `Fence` with a frame
   acknowledges that frame on success"), §10.4 row `new`/`dup`, §12.4 step 3 (a seen own entry is dropped
   once applied), §28.4 step 3. The two travel on different connections, so no relay rule can order them
   (TR1-EDIT-1 orders only a socket append's `result` and `push`). When the push comes first, Desk applies
   201 and drops the record (`seen = 201`, `Drop(E201)`); the answer then names a record that no longer
   exists, and nothing says whether the engine records the acknowledgement, runs the echo check, or still
   emits `PairRegistered`.
   Verdict: **spec.** TR1-JOIN-3.

7. **The order of `bootstrap.tdk` and `history.tdk` when Tab persists the join.**
   A s3.49: "then `store.tds`, `identity.tdk`, `device.tdk`, `bootstrap.tdk`, `history.tdk`".
   B s3.54: the same order (`bootstrap.tdk`, then `history.tdk`).
   Spec: device.md §18.3 "Persisting the join" lists `bootstrap.tdk` before `history.tdk`; §15.7 says the
   store writes `history.tdk` "before it persists the bootstrap". Both readers followed §18.3.
   Verdict: **spec** (the two sections contradict each other). Already resolved by TR1-FOLLOW-3 (history
   first, DEV-PAIR-006); nothing to add.

8. **The `gen` stored with the bundle's token.**
   A s3.49: "AMBIGUOUS: `device.tdk.tokens[].gen` is required as 'the gen the login returned' (device §10.3,
   §16), but the bundle's token comes with no `gen` and is opaque (SEAL-KEY-015); taken: `G = 1`."
   B s3.54: "AMBIGUOUS: `device.tdk.tokens[].gen` is required, but the bundle carries the token only as
   opaque text (seal §17.8, SEAL-KEY-015); 1 written."
   Spec: device.md §10.3 (first sentence, and "Tokens stored without a login"), §16 row `tokens` (`gen`
   required); relay.md §5.2 (tokens of `create`, `register` and `seed` have `gen = 1`), §12.3 ("N's token
   with `gen = 1`"). device.md never says what it stores for a token it did not get from a login.
   Verdict: **spec.** TR1-JOIN-4.

9. **The minter a joining device starts with.**
   A s3.49: "AMBIGUOUS: the initial minter (`IdMinter::new` for `device_no` 4) is not given numerically
   (device §8.3, §18.3; core §7.2); taken: `item 1`, `block 1` as the next numbers." A s3.61:
   `origin_state = u16 4 ‖ u64 2 ‖ u64 1`.
   B s3.54: "Also `IdMinter::new(4)`'s `next_item`/`next_block` values (core §20) are not fixed: `MI`, `MB`
   symbolic". B s3.62: `origin_state = u16 4 ‖ u64 2 ‖ u64 MB`.
   Spec: core.md §7.2 (the minting rule), §20 `IdMinter::new(device_no)`; device.md §16 row `minter`, §18.1
   step 7, §18.3. The minted id is the same under both readings (`mark + 1 = 1` wins), but `device.tdk`'s
   `minter` and every `origin_state` differ.
   Verdict: **spec.** TR1-JOIN-5.

10. **The minter held in Tab's `device.tdk` at the end.**
    A, end state: "device (token, `push`, minter, `log_floor 201`, `floor_chain C200`)", the minter as written
    at the join.
    B, end state: "`device.tdk` (token gen 1, push true, minter 4/2/MB)".
    Spec: device.md §8.3, row "`minter` in `device.tdk`": written "before a `RewriteOutbox`" only; the
    minter after each compile travels in the outbox record's `origin_state` (DEV-RUN-004). No
    `RewriteOutbox` happens in the scenario, so `device.tdk` keeps the join's minter.
    Verdict: **reader (B).** No change.

11. **Whether a join emits `TruncateLogBelow`, and whether it writes a base at a seq that is not aligned.**
    A s3.51: "`Persist[WriteCheckpoint{…}, WriteBase(200)]`, then, after its `Persisted` (D8),
    `Persist[TruncateLogBelow(201, C200)]` (device.tdk `log_floor 201`, `floor_chain C200`)", and
    "AMBIGUOUS: §19.8 points to §19.7 step 4 …; it is unclear whether a base is written at a non-aligned
    `h = 200` (bases are aligned seqs, §18.2) and in which order the truncate and the checkpoint go".
    B s3.55: "`Persist[WriteCheckpoint{200, …}, WriteBase{same}]`", and "AMBIGUOUS: engine §19.8 says 'loads
    the snapshot at `h` as in §19.7 step 4', which includes `TruncateLogBelow(S + 1)`; whether a join emits
    it … or only takes `log_floor = h + 1` from the checkpoint (§7.10) is not said. I take: no
    `TruncateLogBelow`."
    Spec: engine.md §19.8 "Join" ("as in §19.7 step 4"), §19.7 step 4, ENG-PERSIST-005 (which allows
    `TruncateLogBelow` only for §7.13, §19.7 and §28.2), §7.10 (`retained_from` from `Loaded.floor_chain`),
    §18.2 (a base is "a checkpoint taken when `B` equals an aligned seq"). §19.8 and ENG-PERSIST-005
    contradict each other. The outcome differs: without the truncate, `device.tdk` holds no `floor_chain`,
    and after Tab's next checkpoint a restart finds no `C_200`, so `retained_from` falls to 201.
    Verdict: **spec.** TR1-JOIN-6, together with difference 21.

12. **`a` in the reconcile when `M_K` holds no record of this device.**
    A s3.54: "AMBIGUOUS: engine §11.3 defines `a` as this device's `last_ctr` in `M_K` and `h` as the largest
    counter of a record with a frame; at `K = 200` Tab has no record in `M_K` and no outbox record, so
    neither value is defined; taken: both 0."
    B s3.57: "reconcile `a = 0` (no TAB record in `M_200`), `r = 0`: normal", no doubt raised.
    Spec: engine.md §11.3 and §10.3 as TR1-EDIT-2 rewrites them: `h` is now defined from `a`, but `a` is
    still "this device's `last_ctr` in `M_K`", which a joined device lacks until it has classified its
    `DeviceAdded`.
    Verdict: **spec.** TR1-JOIN-7, on top of TR1-EDIT-2.

13. **`h` when no record holds a frame.**
    B, preamble: "AMBIGUOUS: engine §11.3 and §10.3 define `h` as 'the largest counter of a record with a
    frame'; when the outbox holds no framed record … `h` is undefined … I take `h = max(a, largest framed
    ctr)`."
    A s3.54 (above) and A s6.6: "Mac holds no record with a frame, so `h` is undefined and, read as 0, step 3
    (`r > h`: restored copy) would fire for every device with an empty outbox; taken: `h = a = 60`".
    Spec: engine.md §10.3, §11.3.
    Verdict: **spec.** Already resolved by TR1-EDIT-2 (`h = max(a, largest framed counter)`); nothing to add.

14. **The `last_beat_ms` of a new replica.**
    A s3.56: "AMBIGUOUS: the initial `Meta.last_beat_ms` of a new replica is not given (engine §7.8, §23.1);
    taken: 0, so Tab's first frame is a beat and 'Hello' is its second (if no beat were due, 'Hello' would be
    ctr 1 at seq 202)."
    B s3.59: "AMBIGUOUS: the initial `Meta.last_beat_ms` of a new replica is not stated (engine §7.8,
    §23.1); with 0 (taken here) a beat is Tab's first entry and 'Hello' its second".
    Spec: engine.md §7.8 row `last_beat_ms`, §23.1 condition 1, §7.10 (`meta` "absent before the first").
    The readers agree; the other reading moves "Hello" to seq 202 with counter 1.
    Verdict: **spec.** TR1-JOIN-8.

15. **A beat's `state` on a device that bootstrapped after the aligned seq.**
    A s3.56: "AMBIGUOUS: `state` of a beat is 'this device's hash record at the largest aligned `S ≤ B`'
    (engine §23.2); Tab bootstrapped at 200 and holds no record at 192; taken: absent." A s6.16 gives the
    same reason for Mac's beat (`state none`).
    B s3.59: "Tab's beat `state` (hash record at aligned `S ≤ B`, `S = 192`) does not exist on a device that
    bootstrapped at 200 — absent taken (§23.2 says absent only for `B < 64`)." B s6.25: "AMBIGUOUS: the beat's
    `state` needs Mac's hash record at 192 (engine §23.2), which per step 18 it lacks; taken as absent."
    Spec: engine.md §23.2, ENG-BEAT-002.
    Verdict: **spec.** TR1-JOIN-9, together with difference 22.

16. **When `marker.tdk` is cleared.**
    B, after scenario 3: "AMBIGUOUS: when `marker.tdk` is cleared after a page has been applied is not stated
    (engine §15, §7.11 `ClearMarker`); I leave the marker in place until the next `SetMarker`."
    A: no marker clearing written.
    Spec: engine.md §7.11, §15.
    Verdict: **spec.** Already resolved by TR1-EDIT-4 (ENG-RECV-006, ENG-POISON-005); nothing to add.

17. **Wording and detail in scenario 3.**
    A names the eligibility refusals before `pair_start` (s3.2), the 2,000 ms poll for slot B besides the
    socket notice (s3.10), the `callSecret` arguments (s3.11), the attachment `ready false` before the hello
    (s3.52), the banner's `detail None, acked false` (s3.43), the item JSON and stamps of "Hello" (s3.65)
    and the page's request envelope (s3.60, `deadline` "empty or null"). B names `Host` and
    `Accept-Encoding` (s3.6), the key store's associated data (s3.54), the bundle's `roots` and `added_seq`
    checks (s3.48), the push size 762 (s3.65) and the membership's 810 bytes. The relay address display ("8
    groups of 4 and one of 2" against "groups of 4 …, last group 2"), the RAND numbering and the order of
    "Answer 200" and the slot notice in one step differ only in words.
    Spec: as each reader cites; none of these contradicts the other reader. The `deadline` value is fixed
    by TR1-EDIT-7 (`null`).
    Verdict: **format.**

**Scenario 6: snapshot bootstrap**

18. **The scenario's own facts.**
    A, preamble: "AMBIGUOUS: the scenario's relay state (a floor of 129 with head 200) cannot arise under the
    specs: `compact` requires `below_seq ≤ head_seq − 16,384` (RELAY-COMPACT-007 …)". B, preamble: the same,
    plus "SNAP192 can be relied on only through a `snap` entry at `σ > 192`", and "AMBIGUOUS: whether the
    queued edit was sealed before Mac went offline is not fixed by the scenario; taken as unsealed." A takes
    it unsealed too ("so never sealed").
    Spec: relay.md §12.8 check 2, RELAY-COMPACT-007; engine.md ENG-SESS-001.
    Verdict: **format** (facts of the scenario; both take the same reading). See "For the lead" 4.

19. **Whether `Rejoining` puts a stop with the action `ConfirmRejoin` in `status.stops`.**
    A s6.9: "enters `Rejoining`: `status.session = Rejoining`, a stop with action `ConfirmRejoin`;
    `Notify(StatusChanged)`."
    B s6.9: "AMBIGUOUS: whether `Rejoining` also adds a `StopView` with action `ConfirmRejoin` to
    `status.stops` (engine §30.1 lists that action; §19.7 only says 'shows it') is not stated."
    Spec: engine.md §19.7 step 1 ("enters `Rejoining`, shows it"), §30.1 rows `session` and `stops` (sources
    "§11.9, §21, §22, §24.4"; `StopAction::ConfirmRejoin`), §37 `StopKind` (no kind fits a rejoin);
    bridge.md §13.1 row "Devices: alarms" (offers `confirm_rejoin` as "the stop's action"). The Android
    screen can offer the confirmation only through a stop, and no `StopKind` can carry it.
    Verdict: **spec.** TR1-JOIN-10.

20. **Whether the blob listing waits for the pre-rejoin base.**
    B s6.12: "AMBIGUOUS: whether the listing waits for that base's `Persisted` is not stated (no row of engine
    §7.12); taken as not waiting."
    A s6.11: "`Persist[WriteBase(base at 100 …)]`; `Blob{List}`", no doubt raised.
    Spec: engine.md §7.12 (no row makes a `Blob` effect wait for a `WriteBase`), ENG-PERSIST-001 (the
    `Persist` effects form one serial queue, so the base is durable before any later `Persist`, the
    truncation included).
    Verdict: **reader (B).** The text is clear, and B's reading is the one it gives. No change.

21. **The order of the checkpoint, the base and `TruncateLogBelow` in §19.7 step 4.**
    A s6.16: "AMBIGUOUS: §19.7 step 4 lists `TruncateLogBelow(S + 1)` before 'a checkpoint and a base', while
    D8 makes the truncate wait for a checkpoint at or above 192; taken: checkpoint first." A s3.51 raises the
    same point for the join.
    B s6.18: "AMBIGUOUS: §19.7 step 4 lists `TruncateLogBelow` before 'a checkpoint and a base', while D8
    (engine §7.12) makes the truncation wait for a durable checkpoint with `board_seq ≥ 192`; I follow D8."
    Spec: engine.md §19.7 step 4 (as TR1-FOLLOW-2 rewrites it, still listing the truncate first), §7.12 D8.
    Verdict: **spec.** The readers agree, but the step names the ops in the order D8 forbids and does not say
    whether the checkpoint and the base share one `Persist`. TR1-JOIN-6.

22. **Whether a bootstrap records a hash record at the snapshot's seq.**
    A s6.16: "AMBIGUOUS: whether Mac records a `HashRecord` at 192 from the snapshot (it did not apply 192;
    §19.7 says nothing); taken: none (so it gives no verdict and its beat carries no `state`)."
    B s6.18: "AMBIGUOUS: whether Mac records a hash record at 192 from the snapshot (`AppendHashes`) is not
    stated (ENG-HASH-002 speaks of seqs it applies); taken as no."
    Spec: engine.md §18.3, ENG-HASH-002 ("at every seq … that it applies"), §19.5 ("A device that did not
    apply `S` itself (it bootstrapped later) gives no verdict"), §19.7 step 4 (lists the writes, not
    hashes). The answer follows from §19.5 only by inference: a hash record at 192 would make Mac judge
    Desk's snapshot by the hash it was handed.
    Verdict: **spec.** TR1-JOIN-9.

23. **How `ContinuityUnchecked` is acknowledged.**
    A s6.20: "status warning `ContinuityUnchecked{from: 192}` … AMBIGUOUS: the warning lasts 'until
    acknowledged' (engine §19.7 step 7), but no `UserAction` (§30.4) or facade call acknowledges a status
    warning."
    B s6.24 and s6.30: "`status.warnings` = `ContinuityUnchecked{from: 192}` (until acknowledged)", "shown until
    the user acknowledges it", with no action named.
    Spec: engine.md §19.7 step 7 ("the status carries the notice `continuity_unchecked { from: S }` until
    acknowledged"), §30.1 row `warnings` ("informational"), §30.4 (no such action), §7.8 (no `Meta` field
    holds it, so it does not survive a restart either); device.md §17.7; bridge.md §11.4, §13.1.
    Verdict: **spec.** TR1-JOIN-11.

24. **The length of the socket `result` for two frames.**
    A s6.23: "`0x82 ‖ u32 req ‖ "TDR1" ‖ u8 2 ‖ (0x00 ‖ u64 201 ‖ C201) ‖ (0x00 ‖ u64 202 ‖ C202)` (92 bytes)".
    B s6.27: the same fields, "(87 bytes)".
    Spec: relay.md §9.6 (`magic` 4, `n` 1, each `new` result 41) and the socket's result message (type 1,
    `req` 4): 1 + 4 + 4 + 1 + 2 × 41 = 92. B's own one-result message in s3.60 is 51 bytes, consistent with 92.
    Verdict: **reader (B).** No change.

25. **Wording and detail in scenario 6.**
    A writes the prompt as "away more than 365 days", B as "no longer holds the history from 101 to 128";
    B counts `BasedOnUnchecked` already from the entries 193 to 200, A only for Mac's own entry 201 (the
    world does not fix the `based_on` of 193 to 200); B names the rebase `Ctx{201, 2, TQ, 100}` and session
    `Reconciling`; A names `last_synced_ms`, the overwrite notice that would follow another device's rename,
    and the blob listing's order.
    Spec: engine.md §30 ("`desktop` and `android` choose the words"), §19.7, §8.4.
    Verdict: **format.**

# Changes

## seal.md

### TR1-JOIN-1 (differences 1, 2)

**§17.7, the code block.** Replace it with:

```text
esk_X = 32 bytes from Entropy (X = E or N), used as an X25519 scalar as RFC 7748 §5 decodes it
epk_X = X25519(esk_X, 9)                                             (the base point u = 9)
dh  = X25519(esk_E, epk_N) on E, X25519(esk_N, epk_E) on N         (all-zero output aborts, SEAL-GEN-013)
T   = H(ctx("pair-transcript") ‖ vb16(relay origin) ‖ board_id[16] ‖ tid[16] ‖ dev_E[16] ‖ slot A ‖ slot B ‖ slot C)
sas = HKDF(salt = s, ikm = dh, info = ctx("sas") ‖ T, L = 8)
digits = (sas read as a big-endian u64) mod 1,000,000, written as 6 decimal digits with leading zeros
```

**§17.7, after the paragraph that begins "Slots A, B and C enter `T` verbatim".** Insert:

The ephemeral keys are not HPKE keys: `epk_X` is the RFC 7748 X25519 function of the raw 32 bytes and the base
point, with no `DeriveKeyPair` (§6.4 uses `DeriveKeyPair` only for a device's long-term X25519 key).

**§17.10, after the line "`start` is the time of `Start` on E and of `CodeEntered` on N."** Insert:

**Draws.** Each machine draws from the `Entropy` it is given, in this order. E, on `Start`: `s` (10 bytes),
`esk_E` (32), `r_E` (32). N, on `Slot{A}` after the MAC passes: `dev_N` (16), the signing key (32), the X25519
`ikm` (32) of §6.4, then `esk_N` (32). No other value is drawn before slot B is posted.

**§17.10, clauses.** §17.10 has none; directly after the new paragraph "Draws.", insert:

- **SEAL-PAIR-016** (seal) Each pairing machine MUST draw its random values in the order of §17.10 "Draws": E `s`, `esk_E`, `r_E`; N `dev_N`, the signing key, the X25519 `ikm`, `esk_N`.
- **SEAL-PAIR-017** (seal) A pairing's ephemeral secret key MUST be 32 bytes from `Entropy` used as an RFC 7748 X25519 scalar, and its public key MUST be `X25519(esk, 9)`.

## core.md

### TR1-JOIN-5 (difference 9)

**§7.2, after the paragraph that begins "The origin mints with an `IdMinter`".** Insert:

`IdMinter::new(device_no)` (§20) is `{ device_no, next_item: 1, next_block: 1 }`: the minter a device starts
with when it creates a board or joins one, so its first numbers are `mark + 1` of the view.

**§7.2, clauses.** After CORE-ID-004, insert:

- **CORE-ID-007** (core) `IdMinter::new(device_no)` MUST return a minter with that `device_no`, `next_item = 1` and `next_block = 1`.

The signature `impl IdMinter { pub fn new(device_no: u16) -> Self; … }` in §20 does not change.

## engine.md

### TR1-JOIN-2 (difference 5)

**§28.4, step 2.** Replace the step with:

2. When the `BlobResult` of that upload arrives with `Uploaded`, enqueue `CtlIntent::AddDevice { dev, sign_pk, hpke_pk, name, transcript, bootstrap_seq: h }` with `based_on = (h, C_h)` and `via: Register { dev, sign_pk }`. A `BlobResult` with a `RelayRefusal` ends the pairing before any entry: nothing is enqueued, and the engine answers `Notify(PairFailed { ticket, code })` with the refusal's code.

**§28.4, clauses.** After ENG-OPS-006, insert:

- **ENG-OPS-008** (engine) The engine MUST enqueue a pairing's `DeviceAdded` only after the `BlobResult` `Uploaded` of that pairing's snapshot, and MUST enqueue nothing and report the pairing failed with the refusal's code when the upload is refused.

### TR1-JOIN-3 (difference 6)

**§11.7, after the paragraph that begins "`Door`, `Seed` and `SeedTail` go to the new relay".** Insert:

**An answer after the push.** The admin answer and the `push` of the seq it names travel on different
connections (`relay.md` §12.3), so the push can arrive first: the engine then classifies the frame as its own,
applies it and drops the record (§12.4). When a `Register` or `Fence` answer with `seq` and `chain` arrives and
its record has left the outbox, the engine records no `ack`. It checks at once that its log at `seq` holds a
frame under this device's `dev` at the counter it sealed for that record, and that its `C_seq` equals the
answer's `chain`; a difference is evidence `unechoed` (§11.5). The rest of the answer is handled as when the
record is still in the outbox: for a `Register`, `Notify(PairRegistered …)` and the token as §28.4 step 3
states.

**§11.5, clauses.** After ENG-ECHO-001, insert:

- **ENG-ECHO-002** (engine) When a `Register` or `Fence` answer arrives after its record has left the outbox, the engine MUST compare the answer's `seq` and `chain` with the frame and chain value its log holds at that seq, as §11.7 states, MUST raise `unechoed` on any difference, and MUST otherwise handle the answer as if the record were present.

### TR1-JOIN-6 (differences 11, 21)

**§7.11, clauses: ENG-PERSIST-005.** Replace the clause with:

- **ENG-PERSIST-005** (engine) The engine MUST emit `TruncateLogBelow`, `DeleteBases` and `RewriteOutbox` only in the cases §7.13, §19.7 (after the user's confirmation), §19.8 (a join, whose log is empty) and §28.2 name, and never because of a relay answer (SYNC.md §4.3 rule 6, V2-21).

**§18.2, after the numbered list (before "Bases are kept 400 days").** Insert:

Two more bases are written whether or not their seq is aligned: the base of the state before a rejoin
(§19.7 step 2) and the base at the snapshot's seq of a bootstrap (§19.7 step 4, §19.8). Retention treats them
like any base (§7.13).

**§19.7, step 4** (as TR1-FOLLOW-2 writes it). Replace the step with:

4. On the first candidate that passes: the confirmed state and `M_S` from the snapshot, `B = K = L = S`, `C_S` from the body, `log_floor = S + 1`, `retained_from = S`. Then one `Persist` holding `WriteCheckpoint` and `WriteBase`, both at `S` (the store writes the snapshot's history records ahead of the checkpoint, `device.md` §15.7, §8.3); and, after its `Persisted` (D8), a `Persist` holding `TruncateLogBelow(S + 1, Some(C_S))` with `C_S` from the body (allowed by the user's confirmation, ENG-PERSIST-005). No hash record is written at `S` (§18.3). Then read from `S + 1`; the read need not wait for either `Persisted`, because the page's own `Persist` follows both in the serial queue (ENG-PERSIST-001).

**§19.8, the item "Join".** Replace the item with:

- **Join** (`seal.md` §17.9). `Loaded.bootstrap = Join { material, added_seq, relay }`: the pairing machine has checked the bundle, the snapshot and the log to the relay's head. The engine loads the snapshot at `h` as §19.7 step 4 states, with `S = h`: the same state, the same `Persist` of `WriteCheckpoint` and `WriteBase` at `h` whether or not `h` is aligned (§18.2), and after its `Persisted` the same `TruncateLogBelow(h + 1, Some(C_h))`, which deletes nothing (the log is empty) and makes the runtime keep `C_h` as `floor_chain` (`device.md` §7.3). It needs no user confirmation. A material whose board `SM::load` refuses, or whose `SM::rules` differs from `M_h.rules`, is the local fault `engine/local_fault`, and the user pairs again. The engine sets the relay target from the bundle's host and door, and reads from `h + 1`; it has joined once `B ≥ added_seq` and `M_B` gives it a `device_no` (`seal.md` SEAL-PAIR-012). Before that, `Enqueue` is refused with `engine/not_joined`.

**§19.8, clauses.** After ENG-BOOT-005, insert:

- **ENG-BOOT-006** (engine) On a bootstrap from a snapshot at `S` (a rejoin, §19.7 step 4, or a join, §19.8), the engine MUST write the checkpoint and the base at `S` in one `Persist`, and MUST emit `TruncateLogBelow(S + 1, Some(C_S))` only after that `Persist`'s `Persisted`.

**§31.7, invariant 5 "No deletion by the relay."** Replace it with:

5. **No deletion by the relay.** Every `TruncateLogBelow`, `DeleteBases`, outbox `Drop`, `forget_roots_below` and identity change a device emits is explained by one of: the retention rules of §7.13; an accepted purge rotation (§28.2); a `ConfirmRejoin` (§19.7); a join (§19.8); an entry that reached the board head; a cancelled hold; an accepted epoch rotation or refused own rotation.

The D8 row of §7.12 (as TR1-FOLLOW-2 writes it) and `PersistOp::TruncateLogBelow(u64, Option<Hash32>)` in §37
do not change.

### TR1-JOIN-7 (difference 12)

Applied after TR1-EDIT-2.

**§10.3.** Replace the paragraph before ENG-SEND-006 (as TR1-EDIT-2 writes it) with:

`next_ctr = 1 + max(a, h, w)`. `a` is this device's `last_ctr` in `M_K`, and 0 while `M_K` holds no record of
this device (a device that joined holds none until it has classified its `DeviceAdded`, §19.8). `h` is the
larger of `a` and the largest `ctr` of a record that has a frame, and `h = a` when no record has a frame. `w`
is the relay's `last_ctr` for this device once a reconcile has verified it (§11.3). The first frame of a device
has counter 1.

**§11.3.** Replace the sentence that TR1-EDIT-2 writes ("Let `a` be this device's `last_ctr` in `M_K`, `h` as
§10.3 defines it …") with:

Let `a` be this device's `last_ctr` in `M_K` (0 while `M_K` holds no record of this device, §10.3), `h` as
§10.3 defines it (the larger of `a` and the largest counter of a record with a frame, so `h = a` when no record
has a frame), and `r = last_ctr` and `q = last_seq` from the welcome.

**§11.3, clauses.** After ENG-RECON-008 (TR1-EDIT-2), insert:

- **ENG-RECON-009** (engine) While `M_K` holds no record of this device, the engine MUST take `a = 0` in §10.3 and §11.3.

### TR1-JOIN-8 (difference 14)

**§7.8, the row `last_beat_ms`.** Replace the row with:

| `last_beat_ms` | i64 | `wall_ms` of the last beat this device composed; 0 in the first `Meta` of a new replica (genesis or join, §19.8), so its first beat is due as soon as it is eligible (§23.1) |

**§23.1, clauses.** After ENG-BEAT-002, insert:

- **ENG-BEAT-003** (engine) The engine MUST start a new replica's `Meta` with `last_beat_ms = 0`.

`Meta.last_beat_ms: i64` in §37 does not change.

### TR1-JOIN-9 (differences 15, 22)

**§18.3, after the paragraph that begins "The state and membership are taken at the same seq".** Insert:

A bootstrap from a snapshot at `S` (§19.7 step 4, §19.8) records no hash at `S`, aligned or not: the device did
not apply the entry at `S`, so it has no state hash of its own there and gives no verdict on a `snap` entry for
`S` (§19.5). Its first hash record is at the first aligned seq it applies.

**§18.3, clauses.** After ENG-HASH-002, insert:

- **ENG-HASH-003** (engine) The engine MUST NOT write a hash record at the seq of a snapshot it bootstrapped from.

**§23.2.** Replace the paragraph before ENG-BEAT-001 with:

`BeatInfo` (`seal.md` §11.4): `rules = SM::supported_rules()`, `envelopes = Sealer::supported_envelopes()`, `hash_versions = SM::hash_versions()`, `state` = this device's hash record at the largest aligned `S ≤ B`, absent when it holds no hash record at that `S` (`B < 64`, or the device bootstrapped from a snapshot at a seq at or above `S`, §18.3), `acks` = `pending_verdicts` (at most 64, ascending, distinct). The beat's head is its `based_on = (B, C_B)`. `pending_verdicts` are removed when the beat is seen accepted.

ENG-BEAT-002 does not change.

### TR1-JOIN-10 (difference 19)

**§19.7, step 1.** Replace the step with:

1. The engine enters `Rejoining` and shows it: while `session` is `Rejoining`, `status.stops` carries one `StopView { kind: Rejoin, since_ms, alarm: None, action: ConfirmRejoin }`, with `since_ms` the `wall_ms` at which it entered. That view is derived from the session; it is no `StopRecord` in `Meta`, and it holds back nothing that `Rejoining` does not already hold back (§10.1, §11.1). The engine waits for `UserAction::ConfirmRejoin`: it will replace its log, and the user decides that, not the relay (ENG-SESS-005).

**§30.1, the row `stops`.** Replace the row with:

| `stops` | list of `{ kind, since_ms, alarm: Option<u64>, action }` | §11.9, §19.7 step 1, §21, §22, §24.4; `action` is one of `RestoreTail`, `ReHome`, `Adopt`, `RePair`, `ReplaceDevice`, `UseThisCopy`, `Restart`, `ConfirmRejoin`, `Wait`; `ConfirmRejoin` only with the kind `Rejoin`, while `session` is `Rejoining` |

**§37, `StopKind`.** Replace the definition with:

```rust
pub enum StopKind { Evidence, Divergence, ForeignSigner, IdentityClone, Revoked { by_log: bool },
                    LocalFault { code: String }, Stranded, Rebootstrap { after: u64 }, Closed, Rejoin }
```

**§19.7, clauses.** After ENG-BOOT-002, insert:

- **ENG-BOOT-007** (engine) While its session is `Rejoining`, the engine MUST show in `status.stops` exactly one stop of kind `Rejoin` with the action `ConfirmRejoin`, and MUST NOT persist it as a `StopRecord`.

bridge.md §13.1 row "Devices: alarms" already offers `confirm_rejoin` as the stop's action `ConfirmRejoin`;
it does not change.

### TR1-JOIN-11 (difference 23)

**§7.8, after the row `move_tries`.** Insert:

| `continuity_unchecked` | `Option<u64>` | the `S` of the last rejoin, until the user acknowledges it (§19.7 step 7) |

**§19.7, step 7.** Replace the step with:

7. On success the engine sets `meta.continuity_unchecked = Some(S)`, persisted in the next `WriteMeta`. While it is set, `status.warnings` carries `ContinuityUnchecked { from: S }` (SYNC.md §4.7). `UserAction::AckContinuity` sets it to `None`, persisted in the next `WriteMeta`.

**§30.4.** Replace the code block with:

```rust
pub enum UserAction {
    AckBanner { id: (u64, u8) }, DismissNotice { seq: u64, index: u32 }, DismissAlarm { id: u64 },
    ConfirmClock, MoveCode { text: String },
    ConfirmRejoin, UseThisCopy, ExportUnsent, AckContinuity,
}
```

**§30.4, clauses.** After ENG-UI-002, insert:

- **ENG-UI-003** (engine) The engine MUST show `ContinuityUnchecked { from: S }` in `status.warnings` from the success of a rejoin at `S` until `UserAction::AckContinuity`, across restarts, and at no other time.

**§37, `Meta`.** Replace the definition with:

```rust
pub struct Meta { pub attempt: u64, pub last_failed: Option<(u64, u64)>, pub build_id: String,
                  pub halt: Option<HaltRecord>, pub stops: Vec<StopRecord>, pub alarms: Vec<AlarmRecord>,
                  pub banners: Vec<Banner>, pub relay: RelayTarget, pub prev_session: Option<[u8; 16]>,
                  pub conflicts: u32, pub clock: Option<(i64, i64)>, pub last_beat_ms: i64,
                  pub duties: DutyState, pub ops: Vec<OpRecord>, pub move_tries: (u32, i64),
                  pub continuity_unchecked: Option<u64> }
```

`StatusWarning::ContinuityUnchecked { from: u64 }` and the §30.1 row `warnings` do not change. `Meta` is
encoded by the mapping of `device.md` §6.5, which needs no change for an `Option<u64>` field.

## device.md

### TR1-JOIN-4 (difference 8)

**§10.3, the paragraph "Tokens stored without a login."** Replace it with:

**Tokens stored without a login.** The token of `create` (§18.1 step 9), the bundle's token (§18.3) and the token of `Seed` (§10.5) are stored without `push` and with `gen = 1`, the generation `relay.md` §5.2 gives every token that `create`, `register` and `seed` return. A `Connect` whose stored token has no `push` first sends `GET …/caps` with that token and stores the answer's `push` with it (replace procedure on `device.tdk`) before it chooses between the socket and HTTPS; a refused or failed `caps` ends the `Connect` as §10.6 states. A login answer's `push` replaces the stored one.

**§10.3, clauses.** After DEV-LOGIN-003, insert:

- **DEV-LOGIN-004** (device) The device MUST store a token it received from `create`, from a pairing bundle or from `seed` with `gen = 1`.

### TR1-JOIN-5 (difference 9)

**§16, the row `minter`.** Replace the row with:

| `minter` | object `{"block": u53, "device_no": int[1..8191], "item": u53}` | optional | | the minter floor (§8.3); written at a board's creation (§18.1 step 7) and at a join (§18.3) as `IdMinter::new(device_no)`, that is `{"block": 1, "device_no": <device_no>, "item": 1}` (`core.md` §7.2, CORE-ID-007) |

### TR1-JOIN-11 (difference 23)

**§17.7, after the row `confirm_rejoin()`.** Insert:

| `ack_continuity()` | D, A | `User(AckContinuity)`: the user has read `ContinuityUnchecked` (`engine.md` §19.7 step 7) | — | — | short | all |

**§19.2, the row `confirm_clock`, `confirm_rejoin`, `use_this_copy`.** Replace the row with:

| `confirm_clock`, `confirm_rejoin`, `use_this_copy`, `ack_continuity` | | | |

**§25, the facade, after `pub fn confirm_rejoin(&self) -> Result<(), DeviceError>;`.** Insert:

```rust
    pub fn ack_continuity(&self) -> Result<(), DeviceError>;
```

## bridge.md

### TR1-JOIN-11 (difference 23)

**§11.4, the `call` table, after the row `confirm_rejoin`.** Insert:

| `ack_continuity` | `{}` | `null` | `ack_continuity` | — | android |

**§13.1, the table, after the row "Devices: the clock".** Insert:

| Devices: continuity | `ContinuityUnchecked { from }` of `status().warnings`, with its seq | Acknowledge (`ack_continuity`) | all |

BRIDGE-SCREEN-001 does not change: it already requires exactly the actions of §13.1's table.

## For the lead

1. **TR1-JOIN-10 adds `StopKind::Rejoin` to engine.md's public API (difference 19).** The specs already
   give `StopAction::ConfirmRejoin`, and bridge.md §13.1 offers `confirm_rejoin` only as a stop's action,
   but no `StopKind` can carry it. Recommendation: accept. The stop is derived from `session` and never
   persisted, so a restart before the user confirms meets the truncated page again and shows the stop
   again. Alternative: no stop at all. That would drop `ConfirmRejoin` from `StopAction`, have the screens
   offer `confirm_rejoin` while `session = Rejoining`, and change bridge.md §13.1 rows "Devices" and
   "Devices: alarms" to match.
2. **TR1-JOIN-11 adds a user action, a `Meta` field, a facade call and a bridge call (difference 23).**
   "Until acknowledged" (SYNC.md §4.7) had no action, and the warning did not survive a restart.
   Recommendation: accept. Conservative alternative: keep SYNC.md's words and acknowledge through the
   existing `dismiss_alarm`, which needs an alarm kind for the warning instead of the new call. Outside this
   scenario, `RestoredCopy` ("the status carries the notice", engine.md §11.3 step 3) and `BasedOnUnchecked`
   have the same gap: nothing says when they leave `warnings`. A later pass should settle them.
3. **TR1-JOIN-6 lets a join emit `TruncateLogBelow` (difference 11).** §19.8 ("as in §19.7 step 4") and
   ENG-PERSIST-005 contradicted each other. The change follows §19.8 and widens ENG-PERSIST-005 and invariant 5
   of §31.7 to a join. Without it, Tab's `floor_chain` is never stored, and after its next checkpoint a
   restart can no longer check entries whose `based_on` is `h`. Recommendation: accept. Conservative
   alternative (reader B's reading): no truncate at a join, and §19.8 says "as in §19.7 step 4, without the
   `TruncateLogBelow`". Then §7.10 needs another source for `C_h` after the bootstrap checkpoint is
   replaced.
4. **Scenario 6's world cannot arise (difference 18).** Both readers found that the relay cannot hold a
   floor of 129 with head 200, because `compact` requires `below_seq ≤ head_seq − 16,384`
   (RELAY-COMPACT-007). No spec change is needed. For round 2, the brief could keep the scenario consistent
   by moving the numbers, for example head 20,000, floor 3,000, newest snapshot at 19,968, and Mac last at
   seq 100.
