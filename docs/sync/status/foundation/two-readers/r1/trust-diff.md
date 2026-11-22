# Two-readers round 1, group `trust`: differences and changes

Readers: `trust-A.md` (A), `trust-B.md` (B). Scenarios 4 (revocation with rotation) and 5 (relay rollback).
Step numbers are the readers' own ("A s4.17" = reader A, scenario 4, step 17). The two agree on every byte
layout they both give: the rename command (60) and the "Pay rent" command (77), Mac's and Desk's `cmd`
plaintexts (512) and frames (670), the AD (72), the admin nonce body (16), the `revoke` admin message (121) and
body (136), the commitment input (70), `info` (85), `aad` (61), the wrap (96), `items_bytes` (772), the inner
signed bytes (112), the payload (919), the `ctl` plaintext (1,024) and frame (1,182), the socket appends (684,
1,196), the results (51), the pushes (1,274, 762), the hello (21) and welcome (93), the read (15), the
`restore_tail` `args` (332) and admin message (127). Every counter, seq, chain input and membership value
agrees. The differences are in what the specs leave open, one length, and a few omissions.

Changes already written in this round that this file builds on: TR1-EDIT-1 (`result` before `push` on the
author's socket), TR1-EDIT-2 and TR1-JOIN-7 (`h` and `a` in §10.3 and §11.3), TR1-EDIT-5 (ENG-SEALER-006),
TR1-JOIN-2 (ENG-OPS-008) and TR1-JOIN-4 (§10.3 "Tokens stored without a login", which sends a refused `caps`
to §10.6).

# Differences

**Scenario 4: revocation with rotation**

1. **Whether stage 2 of `Revoke` waits for the `revoke` answer, and how stage 2 is persisted.**
   A s4.16: "AMBIGUOUS: ENG §28.5 and ENG-OPS-002 say the `revoke` is 'sent before' the rotation is enqueued,
   not that stage 2 waits for its success; I wait for the 200 … Also open: whether the stage write and the
   outbox `Put` share one `Persist`."
   B s4.11: "`AdminResult{req, Ok(…)}`; stage 2: draws `entry_id` RAND5[16]; `Persist(WriteMeta{stage 2},
   Outbox[Put{…}])`", with no doubt raised.
   Spec: engine.md §28.5 steps 1 and 2, ENG-OPS-002 (order only), §28 ("Each stage is persisted before its
   relay call (D10)"; stage 2 has no relay call), §28.8 (only a refused control entry fails a stage). Nothing
   says what the engine does when `revoke` is refused, and a `Start` that repeats stage 1 after the relay
   committed meets `409 relay/target` (relay.md §12.4).
   Verdict: **spec.** TR1-TRUST-1; see "For the lead" 1.

2. **Which devices the rotation wraps to.**
   A s4.17: "working copy W revokes PHONE, so the rotation wraps to the current devices of W, DESK and MAC,
   and to the current recovery identity DEV_R (seal.md §13.6 `EpochRotated`)", with no doubt raised.
   B s4.12: "AMBIGUOUS: engine.md §6.1 `seal` says the wraps go 'to the current devices of `m`', which in
   `M_K` include PHONE; seal.md §13.6 requires exactly the current devices of `W` after the revoke item
   (`seal/ctl_wraps`). I wrap to DESK, MAC and DEV_R only."
   Spec: engine.md §6.1 contract row `seal` ("wraps to the current devices of `m` and its current recovery
   key"); seal.md §13.6 row `EpochRotated` ("`wraps` name exactly the current devices of `W` and `W`'s
   current recovery identity"). Read literally, the engine's contract composes an entry every receiver
   refuses with `seal/ctl_wraps`, and §28.5 step 4 then composes it again forever.
   Verdict: **spec** (the two specs contradict each other). TR1-TRUST-2.

3. **The order of the random draws inside one `seal`, and what HPKE draws.**
   A s4.17: "AMBIGUOUS: neither seal.md §7/§8–§12 nor ENG §6.1 fixes the order of the random draws inside one
   `seal` call (root, HPKE ephemerals, nonce); this order is my choice."
   B s4.12: "AMBIGUOUS: the order of the random draws inside one `seal` call (root, the three HPKE
   encapsulations, the nonce) and the number of bytes HPKE `SealBase` draws through `Entropy` are not fixed
   (seal.md §7, §26; SEAL-GEN-004)."
   Both took `R_2`, then one 32-byte draw per wrap in ascending recipient order, then the nonce.
   Spec: seal.md §7 (`SealBase` with no word on its ephemeral key), §24 (vectors give "the bytes in the order
   they are drawn"), SEAL-GEN-004, SEAL-GEN-012, SEAL-KEY-010; engine.md §6.1 `seal`. The vectors of a
   rotation cannot be reproduced without both facts.
   Verdict: **spec.** TR1-TRUST-2.

4. **The order of `result` and `push` on the author's socket.**
   A s4.20: "AMBIGUOUS: relay.md §10.6/§11.4 say the append is 'answered by `result`' … but not which of the
   two messages goes first on the author's socket; I send `result` first."
   B s4.15: "AMBIGUOUS: the order on the author's socket of the `result` and the `push` of the same commit is
   not fixed (relay.md §10.6, §11.4). I send the result first."
   Spec: relay.md §11.4, RELAY-SOCK-008.
   Verdict: **spec.** Already resolved by TR1-EDIT-1 (RELAY-SOCK-012); nothing to add.

5. **What becomes of the author's pending root when its own rotation is accepted.**
   A s4.22: "AMBIGUOUS: seal.md and ENG §6.1 do not say when a pending root becomes an ordinary root (on the
   own accepted rotation, or only via the own wrap as for any recipient); I take: the opened root is stored as
   `(2, R_2)` and the pending entry `RAND5` is removed." A s4.23: `WriteIdentity` with "`n_pending 0`".
   B s4.27: "opens its own wrap …, R_2 matches `c_2`, `keys_changed`; `Persist(AppendLog[201], SetMarker,
   WriteIdentity)`", without saying what the identity export holds.
   Spec: engine.md §6.1 `seal` ("a fresh root for a rotation, kept as pending under the entry id"),
   `discard_pending` (only for a refused rotation), `export_identity`; device.md §6.4.7 (`pending`: "roots
   drawn for own rotations not yet accepted"); seal.md SEAL-RCPT-007, SEAL-MEM-016. Nothing removes a pending
   root on acceptance, so the bytes of `identity.tdk` after seq 201 differ between the readings, and 16
   accepted rotations would fill `n_pending` (at most 16).
   Verdict: **spec.** TR1-TRUST-3.

6. **What a refused socket upgrade delivers to the engine.**
   A s4.37: "AMBIGUOUS: device.md §10.6 gives `RelayRefused` for a refused whole request but, unlike the login
   row, does not say whether a `Disconnected` follows a refused socket upgrade inside a `Connect`, nor what
   `req` the refusal carries; I take `req: None` then `Disconnected{unreachable: false}`."
   B s4.32: "AMBIGUOUS: device.md §10.6 has no row for a refused socket upgrade: whether `req` is `None` and
   whether a `Disconnected` follows (without one the engine stays `Connecting` and the hourly `Connect` guard
   of engine.md §11.1, which starts from `Offline`, never fires). I deliver both."
   Spec: device.md §10.6 rows "any JSON error answering `login-nonce` or `login`" and "`403 relay/revoked` …
   answering a whole request" (`RelayRefused { req, … }`, no `Disconnected`); §10.3 ("a refused or failed
   `caps` ends the `Connect` as §10.6 states", which has no `caps` row either); relay.md §11.1 ("A refused
   upgrade is an ordinary JSON error response"); engine.md §11.1 (only `Disconnected` leaves `Connecting`
   without a welcome), §11.9.
   Verdict: **spec.** TR1-TRUST-4.

7. **"Comes online": the app opened or a background job.**
   B s4.29: "AMBIGUOUS: 'comes online' can be the app opened (socket) or a background job
   (`background_sync`, HTTPS `Hello`); the outcome is the same `403 relay/revoked`. I take the app opened."
   A s4.34: the user opens the app, without a doubt raised.
   Spec: none; the brief's scenario text.
   Verdict: **format** (a fact of the scenario; both readers took the same reading). No change.

8. **The `attempt` raised at Phone's `Start`.**
   A s4.34: "`Start` with the outbox record of step 3 (ENG-STATE-003); attempt raised (`WriteMeta`, D5)."
   B s4.29: "`Device::open` drew session id RAND11[16] … Engine `Offline`, relay target set → `Connect`",
   with no `attempt` and no `WriteMeta`.
   Spec: engine.md §15.1 ("`meta.attempt` is raised by one, and persisted (D5), at every `Start`"), §15.2
   step 1. Phone's app was closed (the world), so this is a `Start`.
   Verdict: **reader (B).** The text is clear. No change.

9. **The fields of a persisted `StopRecord`.**
   A s4.37: "`StopRecord` JSON, `alarm` absent: `{"kind":{"revoked":{"by_log":false}},"since_ms":…}`"; A
   s5.6: "`{"alarm":k,"kind":"evidence","since_ms":T0+ε}`".
   B s4.33: "the stop `Revoked` with action `RePair` is persisted in `Meta`"; B s5.5: "`stops +
   {RelayBehind evidence stop, since T0, alarm A1, action RestoreTail}`".
   Spec: engine.md §37 `pub struct StopRecord { pub kind: StopKind, pub since_ms: i64, pub alarm:
   Option<u64> }` and `StopKind::Evidence`; device.md §6.5 (the mapping). A `StopRecord` has no action and its
   kind is `Evidence`, not `RelayBehind`; the action is in the status's stop view (§30.1), derived, not stored.
   Verdict: **reader (B).** No change.

10. **Where the history record of seq 201 is written.**
    A s4.23 and s4.26: the history record of 201 inside the page's `Persist`, "(device.md §8.3, DEV-RUN-003 …
    928 bytes)".
    B s4.20 and s4.27: `Persist(AppendLog[…], SetMarker{…}, WriteIdentity)`; its end state: "history.tdk | +
    record of ctl entry 201 (device.md §15.7) | + same".
    Spec: device.md §8.3 ("The store adds exactly these, each in the same `Persist` as the op it names"),
    DEV-RUN-003. B lists the engine's ops; the record is the store's addition to that same `Persist`, and B's
    end state holds it.
    Verdict: **format.**

11. **Timing that no reader can fix.**
    A s4.25 to s4.28: Mac processes the push of 201 before its append reaches the relay. B s4.16 to s4.24: the
    relay sequences 202 first, and Mac then processes the push of 201 and the `result` of 202. A s5.14 has
    Desk handle the `restore_tail` answer before the push of 191 to 200; B s5.13 and s5.14 the push first.
    Spec: the scenario ("reaches the relay just after … 201"); relay.md §12.9 ("After commit: push") and
    §11.4: the answer and the push travel on different connections (as join difference 6).
    Verdict: **format.** Every order gives the same durable writes and outcome: the push of 191 to 200 holds
    only entries equal to Desk's log and is ignored (engine.md §12.3 step 5) whether or not the stop is still
    in force.

12. **Content not fixed by the world, wording, and detail.**
    A s4.1: Phone's queued edit taken as `create_task` "Buy stamps" (79 bytes); B s4 preamble: an unsealed
    `cmd` record without content. A lists `Content-Length` and `Accept-Encoding`; B gives the token's 69-byte
    layout and the masthead text "Sync stopped — open Devices · 1 waiting" (bridge.md A-03); A gives the sync
    line in engine terms. B s5.22: Mac's `Connect` "after its backoff".
    Spec: the scenario; engine.md §30 ("`desktop` and `android` choose the words"); device.md §10.7.
    Verdict: **format.**

**Scenario 5: relay rollback**

13. **One `RelayBehind` alarm or two from the same welcome.**
    A s5.5: "AMBIGUOUS: ENG §11.2 step 2 and §11.3 step 1 each raise `relay_behind` from the same welcome; the
    spec does not say whether that is one alarm or two. I record one alarm carrying both the head and
    `last_ctr`."
    B s5.4: "AMBIGUOUS: engine.md §11.2 lists its checks 'in order' without saying whether a failing check
    ends the list, so step 2 and §11.3 step 1 may give one alarm or two. I record one alarm carrying both
    facts."
    Spec: engine.md §11.2 (steps 2, 3 and 5), §11.3 step 1, §21.1 (`Evidence::RelayBehind { relay_head,
    relay_last_ctr, local_head, ack }`, at most 256 alarms). Nothing says whether the steps after a failing
    one run, whether the reconcile's effects (`registry.restored`, the read) follow evidence, or whether the
    `Hello` every 900,000 ms under the stop (§12.2) raises the same alarm again.
    Verdict: **spec.** TR1-TRUST-5; see "For the lead" 4.

14. **How the restore tail gets its frames.**
    B s5.7: "It reads frames 191–200 from its own log: `ReadLog{req, 191, 200}` → `LocalRead`. AMBIGUOUS: the
    engine's in-memory state (engine.md §7.1) holds no frames, and §21.3 does not say whether the restore tail
    reads the frames with `ReadLog`; I use `ReadLog`."
    A s5.8 and s5.12: "`Admin{req, RestoreTail{first_seq 191, chain_before C190, frames F191…F200}}`", "from
    Desk's log unchanged", with no read effect.
    Spec: engine.md §7.1 (no frames in memory), §4.3 `ReadLog`, §21.3 step 1, ENG-EVID-004, ENG-EFFECT-002.
    Verdict: **spec.** TR1-TRUST-6.

15. **How a session left in `Connecting` by a failed welcome continues once the stop ends.**
    A s5.15: "AMBIGUOUS: ENG §11.1 has no transition out of `Connecting` after a welcome that raised evidence
    once the stop has ended (§21.3 step 5 only ends the stop) … I send a `Hello`".
    B s5.15: "AMBIGUOUS: after an evidence stop ends, engine.md §11.1 and §21.3 do not say how a session left
    in `Connecting` by the failed welcome continues (a new `Hello`, or the earlier welcome re-evaluated). I
    send `Hello`."
    Spec: engine.md §11.1 row "any | `Welcome` | a check of §11.2 fails | the stop of §21 | unchanged"; §12.2
    (`Hello` every 900,000 ms only "while a stop of §21 is in force"); §21.3 step 5; ENG-EVID-005. Read
    literally, the session stays `Connecting` with no effect until the socket drops, and the new edit of
    T0 + 30 s waits.
    Verdict: **spec.** TR1-TRUST-7.

16. **`Live` after a welcome whose head equals `L`.**
    A s5.15: "… and no transition to `Live` when a welcome's head already equals `L` (the `Reconciling →
    Live` row needs a `Page`); I send a `Hello` and go `Live` on its welcome."
    B s5.15: "AMBIGUOUS: when a welcome's head equals L, no page arrives, and engine.md §11.1 moves to `Live`
    only on a `Page`; I take `Live` at once (reconcile complete, L = relay head)."
    Spec: engine.md §11.1 rows `Connecting | Welcome` (next `Reconciling`) and `Reconciling | Page`; §12.2 (no
    read when the head is not above `L`). Read literally, every reconnect with nothing new stays in
    `Reconciling`, where nothing is sealed (ENG-SESS-001).
    Verdict: **spec.** TR1-TRUST-7.

17. **`h` when no record holds a frame.**
    A s5.15: "AMBIGUOUS: ENG §11.3 defines `h` as 'the largest counter of a record with a frame' but not its
    value when no record holds a frame … I take `h = a`".
    B s5.15: "AMBIGUOUS: engine.md §11.3 does not define `h` when no record has a frame; I take `h = a`."
    Spec: engine.md §10.3, §11.3.
    Verdict: **spec.** Already resolved by TR1-EDIT-2 (with TR1-JOIN-7); nothing to add.

18. **Blobs the rollback lost.**
    A, after scenario 5: "AMBIGUOUS: blobs are outside the rollback repair. The snapshot `SNAP192` was uploaded
    after seq 192 was sequenced, so the restored copy (to seq 190) very likely lacks it … nothing says a device
    re-uploads a blob that the rollback lost".
    B s5.26: "AMBIGUOUS: `restore_tail` brings back log rows only; whether the blob `SNAP192` (announced in
    193–200) still exists on the restored relay, and who re-uploads it, is not covered (relay.md §12.9,
    engine.md §21.3)."
    Spec: relay.md §12.9 (log rows and author rows), §7.1 (blob parts live in the object's storage), §12.8
    step 4 (`409 relay/no_snapshot`); engine.md §19.2, §19.3, §19.7 step 2 (candidates from the relay's blob
    list), §20.2, §21.3. The sealed bytes cannot be reproduced (a fresh seal draws a new nonce, and the `snap`
    entry pins `sha256`), so no device can re-upload; nothing says so, and the readers could not tell whether
    a repair is owed.
    Verdict: **spec.** TR1-TRUST-8; see "For the lead" 2.

19. **The `entry_id` of a composed control entry: at enqueue or at seal.**
    B s4.11: "AMBIGUOUS: whether the stage-2 record's `entry_id` is drawn at enqueue (as ENG-ENQ-003 says for
    'each new record') or at seal; engine.md §9.2 gives only `at_ms` and `based_on` for composed entries. I
    draw it at enqueue."
    A s4.16: "draws **RAND5[16]** = `entry_id_D`" at enqueue, with no doubt raised.
    Spec: engine.md ENG-ENQ-003 ("Each new record MUST receive a fresh 128-bit `entry_id` from
    `Sealer::new_entry_id`"), §7.4 (the record carries its `entry_id`; `Drop` names it), §9.2.
    Verdict: **reader (B), clarified.** ENG-ENQ-003 is clear, and an outbox `Put` cannot exist without the id
    its `Drop` names; but §9.2 lists the composed record's other values and not this one. TR1-TRUST-9.

20. **The length of the socket `page` message.**
    A s5.22: "`page` message `0x83 ‖ u32 r ‖ "TDP1" ‖ … ‖ F201` (762 bytes)".
    B s5.25: "`0x83 ‖ u32 r ‖ "TDP1" ‖ … ‖ F201` (766 bytes)".
    Spec: relay.md §11.3 (`page` = `0x83 ‖ u32 req ‖ page`), §10.5 (page `71 + Σ entries`, entry `20 +
    flen`): 1 + 4 + 71 + 20 + 670 = 766. A gave the push's length (762, no `req`).
    Verdict: **reader (A).** No change.

21. **Wording of the evidence shown, and the alarm record.**
    A s5.6: `AlarmRecord` JSON with `ack` absent; A s5.7: "relay is behind this device: relay at 190, this
    device at 200 (counter 115 vs 121)". B s5.5: `evidence RelayBehind{… ack None}`; B s5.6: "relay is at 190
    (chain C190), counter 115 for this device; this device holds 200".
    Spec: device.md §6.5 (`None` is absent); engine.md §30.
    Verdict: **format.**

# Changes

## seal.md

### TR1-TRUST-2 (differences 2, 3)

**§7, after the paragraph that begins "The info binds the use, the board, the epoch and the recipient".**
Insert:

**The ephemeral key.** `SealBase` makes its ephemeral key with RFC 9180's `GenerateKeyPair`: `wrap_root` draws exactly 32 bytes `ikm` from its `Entropy` argument and takes the key pair `DeriveKeyPair(ikm)` (RFC 9180 §7.1.3). It draws nothing else. A test vector (§24) gives those 32 bytes for each wrap, in the order the wraps are made.

**§7, clauses.** After SEAL-WRAP-003, insert:

- **SEAL-WRAP-004** (seal) `wrap_root` MUST draw exactly 32 bytes from its `Entropy` argument, MUST derive HPKE's ephemeral key pair from them with `DeriveKeyPair`, and MUST draw nothing else.

The public API (§26, `wrap_root`), the limits and the error catalogue do not change. The order of the draws
within one entry is the composer's: `engine.md` §6.1 `seal` (ENG-SEALER-008).

## core.md

No change.

## relay.md

### TR1-TRUST-8 (difference 18)

**§12.9, the paragraph that begins "Restore tail does not change any device row's role, state or key".**
After its first sentence ("Restore tail does not change any device row's role, state or key, nor any
floor."), insert:

A restore tail restores log rows and the author rows they need, not blobs: a blob that the rollback lost stays lost, and no device uploads it again, because its sealed bytes, which the announcing `snap` entry pins by `sha256`, cannot be reproduced. A compaction that relies on such a blob is refused with `409 relay/no_snapshot` (§12.8 step 4).

No clause, route or error-catalogue row changes.

## engine.md

### TR1-TRUST-1 (difference 1)

**§28.5, steps 1 and 2.** Replace the two steps with:

1. After the `WriteMeta` of stage 1 (D10): `Admin { Revoke { dev } }` (the relay drops the target's tokens at once, `relay.md` §12.4).
2. When its `AdminResult` is `Ok`, or is the refusal `409 relay/target` (the relay already holds no current row for the target, for example after a `Start` repeated stage 1 whose call had committed): enqueue `CtlIntent::Rotate { revoke: [dev], reinstate: [], purge_before: None }` as §9.2 states, in one `Persist` holding, in order, the `WriteMeta` of stage 2 and the `Outbox` op with the `Put` of that record. Any other refusal ends the operation before any entry: nothing is enqueued, and `Notify(OpDone { ticket, result: Err(code) })` carries the refusal's code.

Steps 3 and 4 do not change.

**§28.5, clauses.** After ENG-OPS-002, insert (ENG-OPS-008 is TR1-JOIN-2's):

- **ENG-OPS-009** (engine) The engine MUST enqueue the rotation of a `Revoke` operation only after the `revoke` admin answer is `Ok` or `409 relay/target`, in one `Persist` with the operation's stage 2, and on any other refusal MUST enqueue nothing and end the operation with the refusal's code.

ENG-OPS-002 and `Operation::Revoke` in §28 and §37 do not change.

### TR1-TRUST-2 (differences 2, 3)

**§6.1, the contract row `seal`.** Replace the row with:

| `seal` | builds the plaintext of `seal.md` §10.1 with the entry's id, kind, `at_ms`, `based_on` and the body's payload; for `Body::Ctl` it composes the items from the intent and `m` at this moment; for a rotation it draws a fresh root, kept as pending under the entry id, computes its commitment, and wraps the root to each current device and to the current recovery identity of the working copy `W` of `seal.md` §13.6, that is `m` as the entry's earlier items leave it (a device the entry revokes gets no wrap, one it reinstates gets one, and after a `RecoveryKeyChange` the new recovery key gets it), once each, ascending by recipient id; then the inner signature; seals under `m`'s current epoch (SEAL-ENV-003) with counter `ctr`; draws its random values in this order: the root (`EpochRoot::generate`), then the wraps in ascending recipient-id order (`wrap_root`, 32 bytes each, `seal.md` SEAL-WRAP-004), then the frame's nonce (`seal_frame`, SEAL-GEN-012); returns the frame, its hash, the epoch and whether its secrets changed |

**§6.1, clauses.** After ENG-SEALER-006 (TR1-EDIT-5), insert:

- **ENG-SEALER-007** (device) When it composes an `EpochRotated`, the sealer adapter MUST wrap the new root to exactly the current devices and the current recovery identity of the working copy `W` of `seal.md` §13.6, as the entry's earlier items leave it, once each, ascending by recipient id.
- **ENG-SEALER-008** (device) Within one `seal` call, the sealer adapter MUST draw its random values in the order of §6.1 `seal`: the epoch root, then one wrap per recipient in ascending recipient-id order, then the frame's nonce.

**§36, the block "`device.md`."** After its first bullet ("Requires: the runtime of §4: …"), insert:

- Requires: the sealer adapter composes rotations, draws its random values and retires its pending roots as §6.1 `seal` and `classify` state (ENG-SEALER-007 to ENG-SEALER-009).

The trait signature `fn seal(&mut self, m: &Self::Membership, ctr: u64, entry: &PlainEntry)` and the fake
sealer (§6.3) do not change.

### TR1-TRUST-3 (difference 5)

**§6.1, the contract row `classify`.** Replace the row with:

| `classify` | `seal.md` §14 over the inputs of §14.1, with `M_{s−1} = input.membership`, the sealer's own roots, its own X25519 key, its build's supported rules and, for `seq = 1`, its genesis anchor; returns `seal.md` §14.4's outcome; while it holds a missed wrap of epoch `n` (`take_missed_wraps`), an accepted `EpochRotated{n}` that does not wrap to this device has that wrap opened against the rotation's `commit` (`seal.md` §7) as SEAL-RCPT-007 opens an own wrap, returning the commitment halt when it does not open or does not match; when it accepts an `EpochRotated{n}` in an entry of this device whose `entry_id` holds a pending root, it opens its own wrap as every recipient does (SEAL-RCPT-007), keeps the opened root as the root of epoch `n`, and removes that pending root in the same call; `keys_changed` is true when it learned a root or removed a pending one; it never panics (SEAL-GEN-011) |

**§6.1, clauses.** After ENG-SEALER-008 (TR1-TRUST-2), insert:

- **ENG-SEALER-009** (device) When it accepts a rotation of this device's own, the sealer adapter MUST remove that entry's pending root in the same `classify` call and keep as the epoch's root the one its own wrap gave, so that the next `export_identity` holds it among the roots and not among the pending roots.

The `discard_pending` row, ENG-SEALER-004 and device.md §6.4.7 do not change (§6.4.7 already says the pending
roots are those "not yet accepted").

### TR1-TRUST-4 (difference 6)

**§11.9, the row "`RelayRefused` `401 relay/unauthorized` with `req: None` in `Connecting` (a refused login),
…".** Replace the row with:

| `RelayRefused` `401 relay/unauthorized` with `req: None` in `Connecting` (a refused login, `caps` or socket upgrade), while `meta.relay` is the relay of `M_K`'s latest accepted `Fence` (`home_host`, `home_door`, `home_gen`) | this device is no member of the board there (§28.7): the stop `Closed` with the action `RePair` (§24.5); no `Connect` while it is in force; it ends at the next `Start`; no alarm |

**§11.9, the row "`RelayRefused` with `req: None` in `Connecting`, any code the rows above do not name".**
Replace the row with:

| `RelayRefused` with `req: None` in `Connecting`, any code the rows above do not name | a refused login, `caps` or socket upgrade: alarm `RelayProtocol` with the code; `Connect` again at most once per 3,600,000 ms of `mono_ms` (§11.1) |

**§11.9, after the table.** Insert:

A refused login, `caps` or socket upgrade reaches the engine as `RelayRefused { req: None, refusal }`, then `Disconnected { unreachable: false, paused_until_ms: None }`, while the session is `Connecting`; the `Disconnected` takes the session to `Offline`, and the guards on `Offline` decide the next `Connect`. (`device.md` §10.6; §11.1.)

**§11.9, clauses: ENG-SESS-007.** Replace the clause with:

- **ENG-SESS-007** (engine) On a login, `caps` or socket upgrade refused with `401 relay/unauthorized` while its relay target is the relay of its latest accepted `Fence`, the engine MUST enter the stop `Closed`, MUST emit no `Connect` while that stop is in force, and MUST offer re-pair and resubmit.

### TR1-TRUST-5 (difference 13)

**§11.2.** Replace the whole section body (the list and the clause) with:

In order:

1. `relay_now` updates the clock measurement (§27.1).
2. If `head.seq < L`: evidence `relay_behind` (the relay holds less than this device's log, V2-9). If `head.seq ≤ L` and the chain this device holds at `head.seq` differs from `head.chain`: evidence `fork`.
3. If any record has `ack.seq > head.seq`: evidence `relay_behind`.
4. If `fenced`: §28.3.
5. The reconcile of §11.3 with `last_ctr` and `last_seq`; when step 2 or step 3 found evidence, only its step 1.

**One alarm per kind.** Every step runs, and a welcome raises at most one alarm of each kind. When step 2, step 3 or §11.3 step 1 finds `relay_behind`, the welcome raises one `RelayBehind` alarm with the evidence `relay_head` = the welcome's head, `relay_last_ctr` = its `last_ctr`, `local_head = (L, C_L)`, and `ack` = the acknowledgement with the largest seq above `head.seq` (absent when none is). A welcome raises no alarm of a kind for which `Meta` holds an unresolved one, so the `Hello` that §12.2 sends every 900,000 ms under a stop adds none. When any step found evidence, §11.3 runs only its step 1: the engine reads nothing, sets no `registry.restored` and takes no `w` from that welcome.

- **ENG-SESS-002** (engine) The engine MUST raise the evidence of §11.2 step 2 or step 3 when a welcome's head is below the log head or below an acknowledged seq, or names a chain value at a seq the device holds that differs from its own.
- **ENG-SESS-008** (engine) A welcome MUST raise at most one alarm of each kind, with the evidence §11.2 "One alarm per kind" gives a `RelayBehind`, and none of a kind for which an unresolved alarm is held; when it found evidence, the engine MUST run of §11.3 only its step 1.

`Evidence::RelayBehind` in §21.1 and the alarm limit of §21.1 do not change.

### TR1-TRUST-6 (difference 14)

**§21.3, "Restore tail", step 1.** Replace the step with:

1. Requests of at most 64 frames and 1,048,576 bytes, from the relay head + 1, in seq order, with `chain_before` equal to this device's chain at the relay head. The engine takes each request's frames from its durable log with `ReadLog { from, to }` over at most 64 seqs (§4.3; the in-memory state holds no frames, §7.1), ends the request before the frame that would take it above 1,048,576 bytes, and sends the frames byte for byte as the log holds them. A `LocalRead` answered `Missing` is the local fault `engine/local_fault` (§21.4), and nothing is sent. A request ends with the first control entry it contains; the next request is sent only after that entry's repairs of step 2 have succeeded, so that a frame of an author the entry registers never reaches the relay before that author's row (`relay.md` §12.9, `relay/unknown_author`).

**§21.3, clauses.** After ENG-EVID-006, insert:

- **ENG-EVID-007** (engine) The engine MUST take the frames of each restore-tail request from its durable log through `ReadLog` and send them unchanged, and MUST send no request whose `ReadLog` was answered `Missing`.

`AdminRequest::RestoreTail` and the `ReadLog` effect do not change.

### TR1-TRUST-7 (differences 15, 16)

**§11.1, the table.** Replace the row "`Connecting` | `Welcome` | the checks of §11.2 pass | …" with the two
rows:

| `Connecting` | `Welcome` | the checks of §11.2 pass | the reconcile of §11.3; `Read` when the head is above `L` | `Reconciling` when the head is above `L`; `Live` when `head.seq = L` (the reconcile then completes at once: `q ≤ head.seq = L`, so §11.3 steps 2 and 3 have nothing left to read) |
| `Connecting` | the evidence stop ends (§21.3 step 5) | | `Hello` | `Connecting` |

The other rows do not change.

**§11.1, clauses.** After ENG-SESS-001, insert (ENG-SESS-008 is TR1-TRUST-5's):

- **ENG-SESS-009** (engine) On a welcome whose checks pass and whose head equals `L`, the engine MUST enter `Live` without a read.
- **ENG-SESS-010** (engine) When an evidence stop ends while its session is `Connecting`, the engine MUST emit `Hello` at once and continue from its welcome as §11.1 states.

### TR1-TRUST-8 (difference 18)

**§21.3, after step 5 of "Restore tail"** (before the paragraph "**Re-home.**"). Insert:

**Blobs.** A restore tail restores log rows and the author rows they need, not blobs: a blob that the rollback lost stays lost, and no device uploads it again, because its sealed bytes, which the announcing `snap` entry pins by `sha256`, cannot be reproduced. The `SnapRecord` of that `snap` entry stays in the registry. A rejoin does not meet the blob (it takes its candidates from the relay's blob list, §19.7 step 2); a compaction that names it is answered `409 relay/no_snapshot` and records the day (§20.2); the next snapshot that §19.2 makes due gives compaction a usable blob again.

No clause changes.

### TR1-TRUST-9 (difference 19)

**§9.2.** Replace the paragraph with:

Control entries, beats and `snap` entries are enqueued by operations (§28) and duties (§29) with a fresh `entry_id` drawn at enqueue (`Sealer::new_entry_id`, ENG-ENQ-003), `at_ms = wall_ms`, `based_on = (B, C_B)` and no trial apply (they do not change the board). A control entry's items are composed when it is sealed (§10.2), from the membership of that moment.

ENG-ENQ-003 does not change.

## device.md

### TR1-TRUST-4 (difference 6)

**§10.6, the table: the row "any JSON error answering `login-nonce` or `login` (§10.3), …".** Replace the row
with:

| any JSON error answering a request of a `Connect`: `login-nonce` or `login` (§10.3), except a first `401 relay/no_nonce` or `409 relay/bad_gen`, which restarts the login once; `GET …/caps` (§10.3); or the socket upgrade (`relay.md` §11.1) | `RelayRefused { req: None, refusal }`, then `Disconnected { unreachable: false, paused_until_ms: None }`; the `Connect` counts as a failure (§10.7) | `engine.md` §11.9 |

**§10.6, the table: the row "`403 relay/fenced`, `403 relay/revoked`, `404 relay/no_board`, `409
relay/seeding` answering a whole request".** Replace the row with:

| `403 relay/fenced`, `403 relay/revoked`, `404 relay/no_board`, `409 relay/seeding` answering a whole request of an effect other than `Connect` | `RelayRefused { req, … }` | `engine.md` §11.9 |

**§10.6, after the paragraph "A close, message or error of a socket other than the transport's current one is
discarded and produces no event."** Insert:

A refused login, `caps` or socket upgrade reaches the engine as `RelayRefused { req: None, refusal }`, then `Disconnected { unreachable: false, paused_until_ms: None }`, while the session is `Connecting`; the `Disconnected` takes the session to `Offline`, and the guards on `Offline` decide the next `Connect`. (`engine.md` §11.1, §11.9.)

The first sentence is the one engine.md §11.9 receives (TR1-TRUST-4 under engine.md); only the closing
references differ.

**§10.6, clauses.** After DEV-NET-019, insert:

- **DEV-NET-020** (device) The transport MUST deliver a JSON refusal of the `caps` request or the socket upgrade of a `Connect` as `RelayRefused { req: None, … }` followed by `Disconnected { unreachable: false, paused_until_ms: None }`, as it delivers a refused login.

DEV-NET-011 and the §10.3 text "a refused or failed `caps` ends the `Connect` as §10.6 states" (as TR1-JOIN-4
writes it) do not change.

## bridge.md

No change.

## For the lead

1. **TR1-TRUST-1: what a refused `revoke` does (difference 1).** §28.5 never said. Recommendation: accept
   as written. Stage 2 waits for the answer, and `409 relay/target` counts as done, because a `Start` after
   the relay committed but before the answer was handled repeats stage 1 and meets exactly that code. If it
   failed the operation, the relay would keep the target locked out while the membership still called it
   current, and every new attempt would fail the same way. Any other refusal ends the operation with nothing
   enqueued. The more conservative alternative is to proceed only on `Ok` and fail on every refusal. Then
   §28.5 needs a way out of that lock-out, such as enqueuing the rotation with no `revoke` when the relay
   answers `relay/target`, which is the same rule written another way.
2. **TR1-TRUST-8: blobs lost to a rollback (difference 18).** The change only writes down what already
   happens: nothing is re-uploaded, and the fallbacks that exist (the rejoin's candidate list, compaction's
   `no_snapshot`) carry on. The cost is that compaction can stall until §19.2 makes the next snapshot due,
   which can be up to seven days or 1,000 entries, because the registry still counts the lost snapshot as
   usable. Recommendation: accept. If a faster recovery is wanted, the alternative is a `lost: bool` on
   `SnapRecord`, set by `409 relay/no_snapshot` or `404 relay/no_blob` for that blob, which §19.3 would
   treat as not usable, so a snapshot falls due at the next aligned seq. That changes `SnapRecord` in §7.3
   and §37, and it is a registry write caused by a relay answer: harmless, since it deletes nothing, but it
   needs a ruling under ENG-SESS-005.
3. **TR1-TRUST-2 and TR1-TRUST-3 give the `device` stage three clauses in engine.md (ENG-SEALER-007 to
   ENG-SEALER-009).** The `Sealer` contract lives in engine.md §6.1, but `taskdeck-device` implements it, so
   the owner is `device`. Before this, the only engine.md clauses that name `device` as an owner were joint
   ones (ENG-EFFECT-001, ENG-PERSIST-001, ENG-PERSIST-004). Recommendation: accept, with the §36 interface
   bullet the change adds. The alternative is to keep the contract text in engine.md and move the three
   clauses to device.md §8 as DEV-RUN clauses.
4. **TR1-TRUST-5: no second alarm of a kind while one is unresolved (difference 13).** The readers only asked
   about one welcome. Without the de-duplication, though, the `Hello` that §12.2 sends every 900,000 ms
   under a stop would add a `RelayBehind` alarm every 15 minutes until the 256-alarm cap. Recommendation:
   accept. The conservative alternative is one alarm per welcome with no cross-welcome rule, plus a note
   that §21.1's "resolved ones are dropped first" bounds the list.
