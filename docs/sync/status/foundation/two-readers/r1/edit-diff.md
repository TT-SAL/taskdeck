# Two-readers round 1, group `edit`: differences and changes

Readers: `edit-A.md` (A), `edit-B.md` (B). Scenarios 1 (first edit) and 2 (offline edit replayed). Step numbers
are the readers' own ("A s1.8" = reader A, scenario 1, step 8). The two agree on every byte layout: plaintext,
AD, frame, signature input, chain input, append, result, push, hello, welcome, read, log, marker, outbox and
notice records, and every counter, seq and chain value. The differences are in what the specs leave open and in
two page bodies.

# Differences

1. **Quick-add on the desktop: `submit` or `submit_named`.**
   A s1.1: "`submit`, not `submit_named`, because the create is already named, so `hold = false`".
   B s1.1: "AMBIGUOUS: whether the desktop's quick-add uses `submit` (`hold = false`) or `submit_named` (`hold = true`, naming hold of engine.md §9.3) … Taken: `submit`".
   Spec: device.md §17.4, the row `submit_named(intent)`: "with the naming hold … for a create the user is about to name"; engine.md §9.3.
   Verdict: **reader (B), clarified.** The row is clear enough: a quick-add carries the name the user typed, so it is not a create "the user is about to name". The row does not say which intents use which call, though, and B's doubt is reasonable. TR1-EDIT-6 says so.

2. **Sealing in the same `handle` call as the `Enqueue`, before the first `Put` is durable.**
   A s1.8: seals `r1` "in the same `handle`, §4.5 step 6, send loop", with no doubt raised.
   B s1.6: "AMBIGUOUS: whether the send loop may seal a record in the same `handle` call as its `Enqueue`, before the `Persisted` of its first `Put` … Taken: yes".
   Spec: engine.md §4.5 (step 6, the send loop, runs on every call; ENG-MODEL-005), §4.4 (the `Persist` effects form one serial queue, ENG-PERSIST-001), §7.12 D1 (gates only `Notify(Enqueued)`) and D2 (gates only the `Append`), §10.2 step 1 (`R` is every record with no `ack` and no `seen`).
   Verdict: **reader (B).** The text is clear: nothing in §7.12 holds back the seal, and the serial queue makes the frame's `Put` durable after the record's first `Put`. No change.

3. **When the engine handles `Persisted(P1)` relative to the store writing `P2`.**
   A s1.13 to s1.17: the store writes P1, then P2; the engine then handles `Persisted(P1)` (`Notify(Enqueued)`, the user sees the edit) and then `Persisted(P2)` (`Append`).
   B s1.10 to s1.14: the store writes p1; `Notify(Enqueued)`; the user sees it; the store writes p2; `Append`.
   Spec: engine.md §4.4. The runtime writes the `Persist` effects one after another and reports them in order, and the engine thread handles each `Persisted` when it arrives.
   Verdict: **format.** The durable writes come in the same order and the effects have the same dependencies. Whether the engine's handling of `Persisted(P1)` comes before or after the store finishes P2 is thread timing, which no reader can fix.

4. **The value of `h` in `next_ctr` when no record has a frame.**
   A s1.8: "`ctr = next_ctr = 1 + max(a = 121 …, h = none, w = 121 …) = 122`"; A s2.31: "`ctr = 1 + max(18, none, 18) = 19`".
   B s1.6: "`ctr = next_ctr = 1 + max(a = 121, h = 121, w = 121) = 122`"; B s2.32: "`ctr = 1 + max(18, 18, 18) = 19`".
   Spec: engine.md §10.3: "`h` the largest `ctr` of a record that has a frame". Nothing says what `h` is when no record has one.
   Verdict: **spec.** The counter comes out the same, but the readers gave `h` different values, and the same definition decides the reconcile (difference 5). TR1-EDIT-2.

5. **`h` in the reconcile on hello when no record has a frame.**
   A s2.21: "AMBIGUOUS: engine.md §11.3 defines `h` as 'the largest counter of a record with a frame' and has no value when no record has a frame (as here); read literally as 0, `r = 18 > h` would be step 3, a restored copy. I take `h = a` (step 2, normal)."
   B s2.23: "AMBIGUOUS: engine.md §11.3 defines `h` as … Taken: `h = a = 18` when no record holds a frame, so case 2".
   Spec: engine.md §11.3 opening sentence and steps 2 and 3.
   Verdict: **spec.** Read literally, every reconnect with an empty or unsealed outbox looks like a restored copy. The case where the device has classified a frame of its own whose record still holds that frame is also wrong when read literally: such a record sits at a counter at or below `a`, so `h < a = r` gives step 3. The fix is `h = max(a, largest framed counter)`. TR1-EDIT-2.

6. **The order of `result` and `push` on the author's socket.**
   A s1.23 and A s2.35: "AMBIGUOUS: the order in which Desk's socket receives the `result` … and the `push` … is not fixed by relay.md §11.4 or §10.6; I take `result` first."
   B s1.20 and B s2.37: "AMBIGUOUS: the order of `result` and `push` on the author's socket … Taken: `result` first, then `push`."
   Spec: relay.md §11.4 ("answered by `result`"; "**Push.** After any transaction that stored new `log` rows … the author's included"), §10.6, RELAY-SOCK-008, RELAY-APPEND-002.
   Verdict: **spec.** The order changes the engine's path. With `push` first, the record is seen and dropped (engine.md §12.4 step 3) before its `ack` arrives, and the `result` then names a record that no longer exists. TR1-EDIT-1.

7. **Where the acknowledgement `Put` goes in "the next `Persist`".**
   A s1.26: "AMBIGUOUS: engine.md §10.4 says the `ack` `Put` goes 'in the next `Persist`' but not where among that `Persist`'s ops; I put it after `AppendLog` and `SetMarker`." A s2.37 repeats it.
   B s1.22: "AMBIGUOUS: where 'the next Persist' puts the acknowledgement `Put` among the page's ops (engine.md §10.4 versus §12.3 step 8, which lists only `AppendLog`, `SetMarker` and `WriteIdentity`). Taken: in `p3`, after `SetMarker`."
   Spec: engine.md §10.4 row `new`, `dup`; §12.3 step 8.
   Verdict: **spec.** TR1-EDIT-3.

8. **When `ClearMarker` is emitted.**
   A s1.27: "AMBIGUOUS: no clause says when `ClearMarker` is emitted after a page has applied cleanly (engine.md §7.11, §15); I leave `marker.tdk` holding `{201, 201, A_D}`."
   B s1.24: "AMBIGUOUS: when `ClearMarker` is emitted after a page applied cleanly (engine.md §7.11 lists it, §15 never says when). Taken: never here".
   Spec: engine.md §7.11 (the op exists), §15, §15.2 step 2 (at `Start`, a marker with `m.last > B` makes the apply go one entry at a time), ENG-POISON-001 to ENG-POISON-004.
   Verdict: **spec.** The op is defined and never used. A marker left after a clean apply is read at the next `Start` as a hard abort inside that page. That costs a single-entry replay, and for a one-entry page it sets `last_failed` (§15.2 step 2). Two kills in a row before a checkpoint can then halt on an entry that never failed. TR1-EDIT-4; see "For the lead" 1.

9. **Notice records and outbox `Drop`s after an apply: one `Persist` or two, and their order.**
   A s2.38: "`Persist Q7 = [Notices[Add(N1)], Outbox[Drop(Ea), Drop(Eb)]]` … AMBIGUOUS: engine.md §13.3, §13.4 and §7.12 do not fix whether the notice `Add` and the outbox `Drop`s go in one `Persist` or which comes first; I put both in one, notice first."
   B s2.40: "`Persist { pf, [Notices([Add …]), Outbox([Drop(E2), Drop(E3)])] }` (D3)", with no doubt raised.
   Spec: engine.md §7.12 D3 (both wait for the page's `AppendLog`; neither waits for the other), §12.4 step 3, §13.3, §13.4, ENG-REF-002.
   Verdict: **spec.** The readers agree on the result, but the text fixes neither the grouping nor the order of durable writes. TR1-EDIT-4 fixes both, together with the `ClearMarker` of difference 8.

10. **The `at` the phone page sends in `complete`.**
    A s2.1: body `{"op":"complete","id":1099511627783,"at":…}`, "AMBIGUOUS: bridge.md §6.3 gives the page's JSON as `{op, id, at}` without fixing the value of `at` (absent, empty, a naive local time or an RFC 3339 time)".
    B s2.1: body `{"op":"complete","id":1099511627783,"at":"2026-11-22T10:00"}`, "AMBIGUOUS: what `at` today's page puts in `complete` … Taken: the phone's local wall time to the minute, naive".
    Spec: bridge.md §6.3, which defers to "today's command JSON (`board::Command`, digest §2)", and the row `complete` | `{op, id, at}`. core.md §12.2 step 4 accepts four forms. Today's page sends `at = stamp()` = `new Date().toISOString().replace(/\.\d+Z$/, 'Z')` (src/phone.html:4825, 4877), here `"2026-11-22T08:00:00Z"`, which is `WireTime::Exact`.
    Verdict: **spec.** The specs point to a digest the readers do not have. Both bodies differ from the page's, and in general the readings give different `archived_at` values: a naive time to the minute drops the seconds that the exact instant keeps. Here they coincide only because `now` has zero seconds. TR1-EDIT-7.

11. **The `deadline` the phone page sends in `quick_add` with no due date.**
    A s2.9: `{"op":"quick_add","name":"Buy stamps","deadline":…}` "with no deadline".
    B s2.8: `{"op":"quick_add","name":"Buy stamps","deadline":""}`.
    Spec: bridge.md §6.3 row `quick_add` | `{op, name, deadline}`; core.md §12.2 step 4 (absent, `null` or empty is none). Today's page sends `deadline: null` (src/phone.html:5300, 5303).
    Verdict: **spec**, for the same reason as difference 10. The two bodies differ, and neither is the page's. TR1-EDIT-7.

12. **`chain_at` for a seq computed earlier in the same dry run.**
    A s2.25: entry 202 is checked with "(12) `b = 201 < 202`, `C201` (from this dry run) equal → `Accept`", with no doubt raised.
    B s2.27: "AMBIGUOUS: whether `chain_at` in `ClassifyInput` (engine.md §6.1, ENG-SEALER-001) answers for a seq computed earlier in the same dry run but not yet persisted (here `C201` for entry 202's `based_on`). Taken: yes; otherwise 202 would read as `based_on_unchecked` or a fork."
    Spec: engine.md §6.1 `ClassifyInput.chain_at`, ENG-SEALER-001 (which fixes `membership`, `prev_chain` and `retained_from` but not `chain_at`), §7.1 (the chain index runs "from `retained_from` to `L`"), §12.3 step 6; seal.md §14.8.
    Verdict: **spec.** The chain index covers only up to `L`, and a page's entries are above `L` until step 9. TR1-EDIT-5.

13. **How the Phone app ran while offline.**
    A s2 preamble: "Reading of the scenario: the Phone app ran without network at `T0 − 2 h` and `T0 − 1 h` (every `Connect` ended `Disconnected { unreachable: true }`…), its process then ended by `shutdown` …, and at `T0` the user launches it anew."
    B s2 preamble: "AMBIGUOUS (world): how the Phone app ran offline. Taken: the user opened the app without network for each edit … and at `T0` a new process opens the board".
    Spec: none. The open point is a fact of the scenario.
    Verdict: **format.** Both take the same reading, and it does not concern the specs. No change.

14. **Phone's `last_beat_ms`.**
    A s2 preamble: "Phone's `last_beat_ms` is not fixed by the world; I take no beat as due at `T0`".
    B s2.31: "AMBIGUOUS (world): Phone's `last_beat_ms` is not given. If `wall_ms − last_beat_ms ≥ 86,400,000` (§23.1), a beat record is enqueued here … Taken: no beat due."
    Spec: engine.md §23.1, which is clear once the value is known.
    Verdict: **format** (a fact of the scenario). No change.

15. **The order of the relay's own steps before the append checks.**
    A s1.18 and s1.19: reads its clock, checks size, type and layout, then re-reads the row and checks the board state.
    B s1.16: size, type, `ready`, the row re-read, the board state, the layout, then reads the clock.
    Spec: relay.md RELAY-OBJ-005 ("once per request or socket message", at no fixed point), §11.4, RELAY-SOCK-007, RELAY-APPEND-001.
    Verdict: **format.** No outcome depends on when the clock is read, and B's `ready` check is satisfied in both readings.

16. **Sync-line wording.**
    A s2.8: "relay unreachable · 1 waiting"; A s2.15: "· 2 waiting".
    B s2.7: "relay unreachable since …· 1 waiting"; B s2.16: "connecting · 2 waiting".
    Spec: engine.md §30: "The engine gives the data; `desktop` and `android` choose the words".
    Verdict: **format.**

17. **Effects and state one reader names and the other leaves out.**
    A names `ScheduleTick` and `Notify(StatusChanged)` after the seal, the `registry.last_entry` updates, the minter taken from `origin_state` at `open`, and the duty checks of §4.5 step 5. B names the WebSocket pings, `Changes.view`, the `log` row count at the relay, and the item's full JSON.
    Spec: as each reader cites. None of these items contradicts the other reader.
    Verdict: **format.**

18. **Notation.**
    B s1.10 and s1.13 write the outbox `Put`'s length as "length 4" and "u32 len(J1')". A writes `vb32(…)`. A writes "DESK signing key" where B writes `SIG(DESK_SIGN; …)`.
    Spec: device.md §6.4.4 (`vb32` is the 4-byte length prefix), seal.md §8.3.
    Verdict: **format.**

## relay.md

### TR1-EDIT-1 (difference 6)

**§11.4, the paragraph "Push."** Replace it with:

**Push.** After any transaction that stored new `log` rows (an append over HTTPS or a socket, `register` in frame mode, `fence` with a frame, `restore_tail`), the object sends `push` messages carrying exactly the new entries in seq order to each socket with `ready = true`, of every device, the author's included. The messages are split as pages are (§10.5: at most 256 entries and 1 MiB of frames per message, `more` set on all but the last). The head fields are the head after the commit, and `truncated` is 0. For an append over the socket, the object sends that append's `result` on the author's socket before any of those `push` messages. A send that throws is ignored: the client catches up by reading.

**§11.4, clauses.** After RELAY-SOCK-008, insert:

- **RELAY-SOCK-012** (relay) After a socket append commits, the object MUST send that append's `result` on the author's socket before the `push` messages that carry the entries it stored.

## engine.md

### TR1-EDIT-2 (differences 4, 5)

**§10.3.** Replace the paragraph before ENG-SEND-006 with:

`next_ctr = 1 + max(a, h, w)`. `a` is this device's `last_ctr` in `M_K`. `h` is the larger of `a` and the largest `ctr` of a record that has a frame, and `h = a` when no record has a frame. `w` is the relay's `last_ctr` for this device once a reconcile has verified it (§11.3). The first frame of a device has counter 1.

**§11.3.** Replace the sentence that begins "Let `a` be this device's `last_ctr`" with:

Let `a` be this device's `last_ctr` in `M_K`, `h` as §10.3 defines it (the larger of `a` and the largest counter of a record with a frame, so `h = a` when no record has a frame), and `r = last_ctr` and `q = last_seq` from the welcome.

**§11.3, clauses.** After ENG-RECON-004, insert:

- **ENG-RECON-008** (engine) In the reconcile of §11.3, the engine MUST take `h` to be the larger of `a` and the largest counter of a record that holds a frame, so that a welcome with `r = a` is the normal case of step 2 whether or not any record holds a frame.

ENG-SEND-006 does not change.

### TR1-EDIT-3 (difference 7)

**§10.4, the table row `new`, `dup` `(seq, chain)`.** Replace the row with:

| `new`, `dup` `(seq, chain)` | `ack = (seq, chain)`; `Put` the record in the next `Persist` the engine emits, in one `Outbox` op holding the acknowledged records in frame order, placed after every other op of that `Persist` (no effect waits for it); if `seq ≤ L`, run the echo check now (§11.5) |

**§12.3, step 8.** Replace the step with:

8. **Persist.** One `Persist` holding, in order: `AppendLog` of the entries before the cut (seq, `C_s`, `τ_s`, frame); `SetMarker { first, last, attempt }` over them; `WriteIdentity` when a classification reported `keys_changed`; and, last, the `Outbox` op of the acknowledgement `Put`s that §10.4 left for the next `Persist`, when there are any.

### TR1-EDIT-4 (differences 8, 9)

**§12.4.** After the paragraph "The board's apply of a page runs under the page's marker and `catch_unwind` (§15).", insert:

**After the apply.** When the board has applied, without a panic, every entry that the current marker covers (a page's entries in page mode, one entry in single mode, §15), the engine emits one `Persist` holding, in order: a `Notices` op with an `Add` for each notice and refusal record those entries produced (§13.3, §13.4), in seq order, and within one seq in the order `apply` returned them; an `Outbox` op with the `Drop` of each record that step 3 dropped, in seq order; and `ClearMarker`. When a run produces no record and no `Drop`, the `Persist` holds only `ClearMarker`. Any `Notify(NoticesAdded)` for those records waits for that `Persisted` (ENG-REF-002). A panic, or a halt that holds the board, leaves the marker in place for §15.

**§12.4, clauses.** After ENG-RECV-005, insert:

- **ENG-RECV-006** (engine) After the board has applied, without a panic, every entry that a marker covers, the engine MUST write in one `Persist`, in this order, the notice and refusal records those entries produced, the outbox `Drop`s they caused, and `ClearMarker`.

**§15, clauses.** After ENG-POISON-004, insert:

- **ENG-POISON-005** (engine) The engine MUST emit `ClearMarker` only after the board has applied, without a panic, every entry that the marker covers, and only in the `Persist` of §12.4 "After the apply".

The D3 row of §7.12, the `ClearMarker` row of §7.11 and `PersistOp::ClearMarker` in §37 do not change.

### TR1-EDIT-5 (difference 12)

**§12.3, step 6.** Replace the step with:

6. **Dry run.** For each entry in seq order: `C_s = chain_step(C_{s−1}, s, frame)`; `classify` with `M_{s−1}` from the previous step (or `M_L` for the first), and with `chain_at(b)` answering `C_b` for every `b` from `retained_from` to `s − 1`: from the chain index for `b ≤ L`, and from this dry run's earlier steps for `L < b < s`; it answers `None` for every other `b`. A `NeedFrames` result pauses the page until §17.2 supplies the frames, and then the same entry is classified again. A `Tamper` class, or a frame at or below `L` that differs (step 5), is evidence: the **cut** is that entry, and no entry from the cut on is persisted (§21). A receipt with a rules or commitment halt ends the dry run after that entry: the cut is the next one, and nothing after it is evidence (§16.3).

**§6.1, clauses.** After ENG-SEALER-005, insert:

- **ENG-SEALER-006** (engine) When it classifies the entry at seq `s`, the engine MUST supply a `chain_at` that returns `C_b` for every `b` from `retained_from` to `s − 1`, including the values computed earlier in the same dry run (§12.3 step 6), and `None` for every other `b`.

## device.md

### TR1-EDIT-6 (difference 1)

**§17.4, the table row `submit_named(intent)`.** Replace the row with:

| `submit_named(intent)` | D, A | as `submit`, with the naming hold (`Enqueue.hold = true`, `engine.md` §9.3), for a create that the screen will follow with the name the user is about to type (a `rename` or a `forget`); an intent that already carries the name the user typed, such as `quick_add`, goes through `submit` | as `submit` | — | short | all |

## bridge.md

### TR1-EDIT-7 (differences 10, 11)

**§6.3.** After the paragraph that begins "`session` in requests and replies is a sid", insert:

**Wire times the page sends.** The page sends `at` of `complete` and `delete` as the instant of the tap in UTC, RFC 3339 with whole seconds and `Z` (today's `stamp()`, `new Date().toISOString()` with its fraction removed, phone.html:4825), for example `"2026-11-22T08:00:00Z"`, which `core.md` §12.2 step 4 reads as `WireTime::Exact`. It sends `deadline` of `quick_add` as `null` when the user gave no due date (phone.html:5300), and otherwise as the value of its date-time field, `YYYY-MM-DDTHH:MM` local (`WireTime::Naive`). The members of every body are in the order the table lists them, and the request key travels in the `X-TaskDeck-Request` header (`headers.request_key`, §5.2), not in the body.

No clause changes: BRIDGE-VERB-002 already passes the body unchanged, and the fork changes nothing here.

## For the lead

1. **`ClearMarker` after a clean apply (TR1-EDIT-4, difference 8).** No text says when the marker is removed.
   Left in place, it turns every restart before the next checkpoint into a single-entry replay of the last
   page. For a one-entry marker it also sets `last_failed`, so two kills in a row before a checkpoint can
   halt on an entry that applied cleanly (§15.2 step 2). Recommendation: accept TR1-EDIT-4. It puts
   `ClearMarker` last in the `Persist` that already carries the run's notices and `Drop`s, so it costs a
   `Persist` of its own only when a run produces neither (Mac in scenario 1). The alternative is to state
   that a marker is never cleared, to drop `ClearMarker` from §7.11 and §37, and to have §15.2 step 2 ignore
   a marker whose run applied cleanly. That needs a durable "applied" flag, which costs the same write.
2. **The page's values in TR1-EDIT-7 come from today's page (src/phone.html), not from the specs.** bridge.md
   §6.3 points to a digest the readers do not get. TR1-EDIT-7 writes down what the page sends, with the line
   numbers bridge.md already uses for phone.html. Recommendation: accept. If the lead would rather keep the
   specs free of page facts, the conservative alternative is to give the example values only
   (`"at":"2026-11-22T08:00:00Z"`, `"deadline":null`) and keep the pointer to the digest.
