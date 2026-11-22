# Differences

Group `edit`, round 2. A = reader-A/edit.md, B = reader-B/edit.md. Both readers agree on every byte layout,
width, context string, counter, seq and chain value they both give (command 77 and 74 bytes, plaintext 512,
frame 670, AD 72, signed bytes 624, chain input 90, append 684 and 1,358, result 51 and 92, push 762, welcome
93, the notice record and the archive row). The entries below are everything else.

**Scenario 1**

1. **When Desk seals its new record.**
   A (step 7): seals in "the same `handle`, §4.5 step 6 send loop" as the enqueue, emitting `p2` before `p1`'s `Persisted`; "AMBIGUOUS: engine.md §10.2 does not say whether the send loop may seal a record in the same `handle` call that enqueued it (before `p1`'s `Persisted`)".
   B (steps 10–11): "on `Persisted`: `Notify(Enqueued…)`; `submit` returns", then "send loop … seal", i.e. seals in the call that handles `p1`'s `Persisted`.
   Spec: engine.md §4.5 (the send loop is step 6 of every `handle` call), §7.12 (D1 gates only `Notify(Enqueued)`), §10.1, ENG-PERSIST-001 (serial persists).
   Verdict: **reader (B), clarified.** §4.5 runs the send loop after every event and §7.12 names no wait between the enqueue write and the seal, so A's order is the spec's; but no sentence says so, and the order fixes the draw order and the trace. TR2-EDIT-1.

2. **File nonces of the store not named.**
   A (step 9, 11; scenario 2 steps 3, 5, 19): names `RAND3[24]`, `RAND4[24]` (and in scenario 2 `RAND2`, `RAND4`, `RAND7`) as the file nonces of each sealed `outbox.tdo` record.
   B (step 8 "Durable write: `outbox.tdo` append procedure … sealed under `K_file(5)`"): draws no nonce for any file record.
   Spec: device.md DEV-GEN-003 ("every random byte (… nonces) from `taskdeck_seal::OsEntropy`"), §7.2 append procedure; seal.md §19 (sealed file record with its nonce).
   Verdict: **reader (B).** The brief asks for every random value; the file nonce is one. No change.

3. **Order of the sealer's and the store's draws (AMBIGUOUS).**
   A (step 9): "AMBIGUOUS: … the frame nonce (RAND2) is drawn inside `handle` before the runtime draws the file nonces of `p1` (RAND3) and `p2` (RAND4); device.md §7.3 does not order the store's draws against the sealer's, I take execution order."
   B: no such line (and no file nonces, entry 2).
   Spec: engine.md §4.1/§4.5 (effects are appended within `handle` and executed by the runtime after it), device.md §7.3 (a `Persist` is executed op by op on the sync thread).
   Verdict: **reader (A).** The order follows from the model: the sealer draws while `handle` runs, the store draws while it executes the effects that call returned. A's reading is the spec's. No change (and TR2-EDIT-1 fixes which call seals).

4. **`hold` and `reseals` in the record JSON (AMBIGUOUS).**
   A (step 9): writes `"hold":false`, `"reseals":0`; "AMBIGUOUS: … the canonical encoder omits defaults only by a schema (core.md §4.3), and `OutboxRecord` has no schema table with defaults".
   B (step 8): writes them too, no AMBIGUOUS line.
   Spec: device.md §6.5, row "struct with named fields": "a field of type `Option<T>` that is `None` is absent; no other field is ever absent".
   Verdict: **reader (A).** §6.5 governs the record JSON and is explicit. No change.

5. **Where the largest `order` ever used survives (AMBIGUOUS).**
   A (step 6): "`order = ORD` (one more than the largest order ever used)", no value.
   B (step 8): "AMBIGUOUS: `order` is 'one more than the largest `order` ever used' (engine.md §7.4), but with an empty outbox … no persisted record states the largest order ever used; the spec does not say where that counter survives a `Start`."
   Spec: engine.md §7.4 row `order`; §7.8 `Meta` has no such field; §7.10 / ENG-STATE-003 rebuild the outbox from its ops only.
   Verdict: **spec.** "Ever used" needs a durable counter that no record holds; after the outbox empties (here: every earlier record dropped, the file possibly rewritten) the value is not defined. Only the relative order of live records matters (ENG-SEND-001, -004). TR2-EDIT-2.

6. **The minter's stored `next_item` (AMBIGUOUS, both scenarios).**
   A (step 2; scenario 2 step 5): "I read it as `mark(1).item = 40` and `minter.next_item ≤ 41` (core.md §7.2 leaves the minter's stored value to device.md §8.3, which the world does not fix)."
   B (step 3): "`max(41, 41) = 41`".
   Spec: core.md §7.2, CORE-ID-004 (`max(minter.next_item, mark + 1)`).
   Verdict: **format.** A world fact, not a spec question; both mint `n = 41` (and 6 on Phone) and store `next_item = 42` (7).

7. **The relay's floors (AMBIGUOUS).**
   A (step 17): "`floor` and `purge_floor` = 1 assumes no compaction ever ran (… RELAY-COMPACT-007); the world does not state the relay's floors."
   B (step 21): "`floor` 1 (`max(compact_floor 1, purge_floor 1)`)".
   Spec: relay.md RELAY-COMPACT-007; the world ("no purge").
   Verdict: **format.** Both give 1; the doubt is about the world, which A resolves from the spec.

8. **Echo check before or after the board apply.**
   A (step 21): applies 201 to the board, drops the record, then "Echo check: frame hash and `C201` equal (ENG-ECHO-001)".
   B (step 25): "on `Persisted`: … echo check of `R1` … → pass (§11.5). Board applies 201".
   Spec: engine.md ENG-ECHO-001 ("as soon as the log holds that seq"), D3 (the log holds it at the `Persisted` of `AppendLog`, before any apply).
   Verdict: **reader (A).** The clause places the check at the `Persisted` of the log write, ahead of the apply. No outcome changes here (the check passes). No change.

9. **Rows the relay writes.**
   A (step 15): "Rows written: 3 (relay.md §16.1)". B (step 19): "insert `log(…)` (+ index row `log_dev_ctr`); Desk row …".
   Spec: relay.md §9.4, §16.1.
   Verdict: **format.** The same writes, counted differently.

10. **Request id on the socket.**
    A (step 13): "`req` u32 r1' (transport's socket id mapped to r1)". B (step 16): "`r1` (transport counter)".
    Spec: device.md §10.5.
    Verdict: **format.**

11. **When Desk's screen shows "Pay rent".**
    A (step 10): "on `Persisted{p1}` … UI shows 'Pay rent' in the pending view". B (step 9): "applies `CMD1` to the pending view, `view_version += 1`; the UI shows 'Pay rent'" before the `Persisted`.
    Spec: engine.md ENG-PEND-003, §4.5 step 9, §7.12 D1 (only `Notify(Enqueued)` waits); device.md DEV-FACADE-002 (`submit` returns after `Enqueued`).
    Verdict: **format.** A ties the moment to `submit` returning, B to the view's change; both are in the spec and the user sees the same thing at once.

12. **Numbering of random values across scenarios.**
    A restarts at `RAND1` in scenario 2; B continues (`RAND3[16]` is Phone's first `entry_id`).
    Verdict: **format.**

**Scenario 2**

13. **The page's `complete` body.**
    A (step 1): `{"op":"complete","id":1099511627783,"at":"2026-11-22T08:00:00Z"}` (bridge.md §6.3, wire time `Exact`).
    B (step 1–2): `{"op":"complete","id":1099511627783}` "(no `at`)"; "`at` none → `at' = now = T0−2h`".
    Spec: bridge.md §6.3 table row `complete` `{op, id, at}` and "Wire times the page sends" (lead ruling T2). B's form is the notification action `done` (bridge.md, the row at line 1270), another path.
    Verdict: **reader (B).** The text is clear. The compiled command is the same 74 bytes either way. No change.

14. **"Phone has been closed since seq 200" (AMBIGUOUS).**
    A (step 1): "AMBIGUOUS: … I read 'no relay session since 200, the app opened offline for both edits'". B: the same reading ("Phone, offline"), no line.
    Verdict: **format.** A reading of the world; both agree.

15. **Mac's `based_on` for seq 202 (AMBIGUOUS).**
    A (step 15): "AMBIGUOUS: Mac's rename has `based_on_seq 201` …; the world does not say, I take it." B: `based_on (201, C201)` as a world addition.
    Spec: engine.md ENG-ENQ-005.
    Verdict: **format.** Both take the same value; it is a world fact.

16. **A beat due on Phone (AMBIGUOUS).**
    A (step 19): "AMBIGUOUS: a beat duty may be due (last beat > 24 h ago, engine.md §23.1) … The world does not give `last_beat_ms`; I assume no beat is due." B: no beat.
    Spec: engine.md §23.1, ENG-BEAT-001.
    Verdict: **format.** The spec is clear given `last_beat_ms`; the world omits it. See "For the lead".

17. **The words Phone shows for the overwrite notice.**
    A (step 25): "Phone's bridge pushes a `notices` event (bridge.md §5.4) with the record", no words.
    B (step 29): "'changed on Mac since you last synced' on Phone".
    Spec: bridge.md A-04, row "notice `overwrite`, `author` = `self`": `This phone's change to <name(item)> replaced a change made on <dev(other)>`; core.md §10.2: "It is shown on two devices: the author's and the `other` device's ('changed on Desk since you last synced')."
    Verdict: **reader (B), clarified.** bridge.md A-04 fixes the phone's line; core.md §10.2's quoted example, given for both devices, invited B's text. TR2-EDIT-3.

18. **The words Mac (a desktop) shows for the overwrite notice.**
    A (step 24): "shows 'changed on Phone since you last synced' for 'Call the plumber' (core.md §10.2)".
    B (step 29): "'Phone overwrote your change' on Mac".
    Spec: core.md §10.2 (the example only); bridge.md A-04 covers the phone page only; device.md §17.5 `NoticeView` and §17.7 give the desktop the records but no words.
    Verdict: **spec.** No spec gives the desktop's line. TR2-EDIT-3, TR2-EDIT-4; see "For the lead".

19. **Which change the overwrite notice says was overwritten (AMBIGUOUS).**
    B: "AMBIGUOUS: the notice for `complete` names `other`/`other_seq` from the item's *latest* stamp (core.md §10.2), but the text … does not say which group Mac changed …; whether the UI must also show that the overwritten change was a rename is not stated (core.md §10.2, engine.md §30)." A: none.
    Spec: core.md §10.2 and §10.7 (the record names the overwriting group, `item`, and `other`, `other_seq`; no field for the overwritten group); bridge.md A-04 (the line names the item and the device only).
    Verdict: **reader (B).** The record and the line are fixed; nothing else is shown. No change.

20. **Stored `push` with Phone's token (AMBIGUOUS).**
    B: "AMBIGUOUS: device.md §10.3 lets a token be stored 'without `push`' only for `create`/bundle/seed tokens; … the spec does not say what is stored when a login answer was lost." A (step 9): "stored token gen 1 with `push = true` … no login".
    Spec: device.md §10.3: a login answer's token and `push` replace the stored ones together; a token stored without `push` gets it from `caps` at its first `Connect`.
    Verdict: **reader (B).** A lost login answer stores no token either; a device that has synced (the world) holds `push`. No change.

21. **`appends_left` in Phone's welcome.**
    A (step 11): `u32 appends_left 3,000`. B (step 12): "`3,000 − app_count` (3,000 if `app_day` ≠ today)".
    Spec: relay.md §11.3.
    Verdict: **format.** The world does not give Phone's `app_day`; B writes the rule, A its value for a past day.

22. **The ack `Put`s of Phone's two records.**
    A (step 23): "acks; push processed …", the ack `Put`s not placed. B (steps 24–25): held for the next `Persist`, which "also carries the two ack `Put`s".
    Spec: engine.md §10.4, §12.3 step 8.
    Verdict: **format.** A omits a detail it wrote in scenario 1 (step 20); no contrary step.

# Changes

## core.md

### TR2-EDIT-3 (difference 17, 18)

Section §10.2 "Overwrite notices", the paragraph replaced in full by:

A command that changes a field group, a block or (for `complete`, `delete`, `forget`) a whole item whose stamp (for an item, its latest stamp) is another device's change since the origin's view still applies, and `apply` produces one **overwrite notice** for it (SYNC.md §4.6, V2-5; SYNC_PLAN.md §2 takes notices rather than refusals). The notice names the overwriting entry (`seq`, `author = ctx.device_no`), the overwritten change (`other`, `other_seq` from the stamp), the item, the sid for a block, and the group (`name`, `time`, `levels`, `estimate`, `repeat`, `block` or `item`). It concerns two devices, the author's and the `other` device's, and each of them records it (`engine.md` §13.4). The core defines the record (§10.7), not the words a device shows for it: those are the interface's, `bridge.md` A-04 on the phone and `device.md` §17.7 on a desktop, and they differ between the author's device and the `other` device's. A command produces at most one notice per group and block it touches.

(No clause id changes; no table row is touched.)

## engine.md

### TR2-EDIT-1 (difference 1)

Section §10.2 "The send loop": after step 5, before ENG-SEND-001, add the paragraph:

A record takes part in the send loop from the `handle` call that enqueued it on: when §10.1 holds, the loop of that call (§4.5 step 6) seals it as steps 1 to 4 state, without waiting for the `Persisted` of its enqueue write. §7.12 names no dependency between the two, and ENG-PERSIST-001 makes the enqueue write durable before the `Persist` that carries the frame.

and, after ENG-SEND-004, the clause:

- **ENG-SEND-008** (engine) The engine MUST NOT wait for the `Persisted` of the write that enqueued a record before it seals that record: when §10.1 holds, the send loop of the `handle` call that enqueued the record MUST seal it as steps 1 to 4 of §10.2 state.

### TR2-EDIT-2 (difference 5)

Section §7.4 "Outbox records", the table row `order` replaced by:

| `order` | u64 | the record's position: one more than the largest `order` among the records in the outbox when it is enqueued, or 1 when the outbox is empty; it has no counter of its own and is rebuilt at `Start` with the records (ENG-STATE-003) |

and, after ENG-STATE-001, the clause:

- **ENG-STATE-006** (engine) The engine MUST give each newly enqueued record the `order` one more than the largest `order` among the records then in its outbox (at `Start`, the outbox as ENG-STATE-003 rebuilds it), or 1 when the outbox is empty.

## device.md

### TR2-EDIT-4 (difference 18)

Section §17.7 "Notices, banners, alarms, the halted view": after the paragraph "The halted view is the pending view …", add the paragraph:

The desktop app shows each notice and refusal record of `NoticeView` (§17.5) with the line `bridge.md` A-04 gives for that record, with `This device's` in place of `This phone's`, `name(id)` the item's name in quotes (or `#<id>` when the item is not on the board) and `dev(n)` the name of device `n` in `status().engine.devices` (or `device <n>`).

and the clause:

- **DEV-FACADE-010** (desktop) The desktop app MUST show each notice and refusal record with the line of `bridge.md` A-04 for that record, with `This device's` in place of `This phone's`.

## For the lead

1. **The desktop's words for notices (difference 18; TR2-EDIT-3, TR2-EDIT-4).** No spec gives them; both readers invented a line from core.md §10.2's example. Recommendation: adopt TR2-EDIT-4 as written (the conservative reading: the desktop uses the phone's fixed lines, so tests are exact on both platforms) together with TR2-EDIT-3. If the lead rules instead that desktop wording lies outside the sync specs, drop TR2-EDIT-4 and end TR2-EDIT-3's sentence at "… `bridge.md` A-04 on the phone". Either way README.md's glossary row `notice` still quotes "changed on Desk since you last synced" as an example; recommend its fixer reword it to "a persistent per-device message that is not a refusal, such as an overwrite notice" (README is not one of the six headings above).
2. **World facts the brief leaves open (differences 6, 7, 16, 21).** Not spec questions, but they produced four AMBIGUOUS lines in this group. Recommendation: add to "The world": each minter's `next_item` is one more than the `n` it has minted; the relay has never compacted (floors 1); no beat is due on any device in scenarios 1 and 2; no device has appended yet on the day of `T0`.
3. **Ruling T2 (difference 13).** B's `complete` body without `at` goes against the text T2 put into bridge.md §6.3; no change is written, since the text is clear and the compiled command is the same.
