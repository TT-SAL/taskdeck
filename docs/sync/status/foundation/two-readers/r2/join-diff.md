# Differences

Scenario 3 (pairing)

1. **Tab's first entry: a beat before "Hello".**
   A (29, 30): "eligible, `last_beat_ms = 0` → beat record enqueued (ENG-BEAT-003, §23.1); sealed ctr 1 … appended → seq 202", then "Hello" at "sealed ctr 2 … append → seq 203", trial `Ctx{203, 4, Th, 202}`; Mac shows Tab "last seen from its beat at 202". A also wrote "AMBIGUOUS: the scenario calls 'Hello' Tab's first entry; by engine.md §23.1 and ENG-BEAT-003 the beat goes first."
   B (43 to 46): no beat; "Hello" at "`ctr = 1 + max(0, 0, 0) = 1`" → seq 202, trial `ctx {202, 4, T_H, 201}`; Mac shows "Tab in the device list with no last beat"; every device ends at `(202, C202)`.
   Spec: engine.md ENG-BEAT-003 (a new replica's `Meta` starts with `last_beat_ms = 0`), §23.1 condition 1 (`wall_ms − last_beat_ms ≥ 86,400,000`), ENG-BEAT-001, §29.1 (Tab is eligible once `Live` and caught up), §4.5 (duties run every `handle` call, step 5).
   Verdict: **reader** (B), **clarified**. The rule is clear; B missed the duty, and the brief's wording ("its first entry … 'Hello'") invited it. TR2-JOIN-1 says in §19.8 that a joined device's first entry is its beat.

2. **Whether Tab shows a `DeviceAdded` banner about its own addition.**
   A (28): "Banner `DeviceAdded` (author DESK) on Tab (it is not the author, ENG-BANNER-001)."
   B (42): "AMBIGUOUS: engine.md §14.2 creates a banner on every device but the author's, which gives Tab a `DeviceAdded` banner about its own addition; whether the joining device itself shows it is not said."
   Spec: engine.md §14.2 "A banner is created on every device except the entry's author device, with one exception: `RecoveryProposed`…"; ENG-BANNER-001.
   Verdict: **reader** (B), **clarified**. The text gives the banner to every device but the author, and Tab is one of them. TR2-JOIN-2 names the subject device explicitly.

3. **Defaults in the bundle JSON.**
   A (25): "AMBIGUOUS: seal.md §17.8 calls the bundle 'JSON (§5.3)', i.e. canonical, but its field table does not say `hash_version` and the like are defaults; I write every field."
   B (32): writes every field without comment. Both JSON bodies are byte-identical.
   Spec: seal.md §5.3 "A field with a stated default is omitted … The tables in this spec state every default; a field without one has none."; §17.8 table: every field "required" and none has a default.
   Verdict: **reader** (A). The spec is clear. No change.

4. **Order of the blob id and nonce draws inside `seal_snapshot`.**
   A (18): "`RAND8[16]` = blob id `PB`, `RAND9[24]` = blob nonce", with no comment.
   B (24): the same draws in the same order, plus "AMBIGUOUS: the order of the two draws inside `seal_snapshot` (blob id, nonce) is not fixed by seal.md §20.1/§20.2 or SEAL-GEN-012."
   Spec: seal.md §20.1 "drawn by the writer before sealing"; SEAL-BLOB-005; SEAL-GEN-012; §26 `seal_snapshot(…, rng: &mut dyn Entropy)`, which draws both values from one `Entropy`. Nothing fixes the order within the call, and the test vectors depend on it.
   Verdict: **spec**. TR2-JOIN-3.

5. **Drawing `L_d` at the join.**
   A (27): "`KeyStore::create(L_d)` (`RAND13[32]` = `L_d`; Keystore wrap → `ld.wrap`)".
   B (38): "`KeyStore::create(L_d)`" with no draw; B's `RAND13[16]` is the entry id of "Hello".
   Spec: seal.md §6.6 "`L_d` is 32 bytes from `OsEntropy`, drawn when a device first stores sealed files for a board"; device.md §18.3 "Persisting the join".
   Verdict: **reader** (B). A draw is missing. No change.

6. **Tab's history record of seq 201.**
   A (28): "History record 201 appended."
   B (42): applies 201 but appends no history record. (At step 31, B does append one for Desk.)
   Spec: device.md DEV-RUN-003 (one history record for every accepted control entry among the appended seqs above the last seq the file holds); §8.3 "records every later accepted control entry itself".
   Verdict: **reader** (B). No change.

7. **Desk's durable write of the `AddDevice` record at enqueue.**
   A (20, 21): "enqueue ctl record {…}", then only after sealing "`Persist(Outbox[Put])` (D2)".
   B (26, 27): "`Persist[Outbox[Put]]`" at enqueue, then "`Persist[Outbox[Put(sealed)]]` (D2)" after sealing.
   Spec: engine.md ENG-PERSIST-001 and §7.12, as TR2-EDIT-1 restates them ("ENG-PERSIST-001 makes the enqueue write durable before the `Persist` that carries the frame").
   Verdict: **reader** (A). A leaves out the enqueue write. No change.

8. **Slot D's length field and its MAC input.**
   A (25): "slot D = `0x44 ‖ 0x45 ‖ u16 (40 + len) ‖ body ‖ MAC(mac_key; ctx("pair") ‖ 0x44 ‖ 0x45 ‖ tid ‖ u16 len ‖ body)`". The header uses the body length but the MAC uses `len`, the bundle's length.
   B (32): "`0x44 ‖ 0x45 ‖ u16 len ‖ body ‖ MAC(… ‖ u16 len ‖ body)`", the same `len` in both.
   Spec: seal.md §17.4, `mac` row "`HMAC(mac_key, ctx("pair") ‖ label ‖ role ‖ tid[16] ‖ u16 body_len ‖ body)`"; slot D body width "40 to 16,040".
   Verdict: **reader** (A). The MAC carries `body_len`, as in the header. No change.

9. **Order on Desk: admin answer and push of 201.**
   A (24): "`AdminResult Ok(…)` → ack; `Notify(PairRegistered …)`; processes the push: classify 201 Accept …".
   B (31): "push 201 → classify own frame `Accept` …; Admin answer → `Notify(PairRegistered{…})`".
   Spec: engine.md §28.4 step 3 ("On success at seq `a`: `Notify(PairRegistered …)`") makes the notify depend on the admin answer alone. The order in which the HTTPS answer and the socket push arrive is not fixed, and no output depends on it.
   Verdict: **format**.

10. **Set-aside of plain files on Tab.**
    A (27): "no plain files with data to set aside".
    B (38): "plain files set aside".
    Spec: device.md §18.4 "Set-aside": only plain files that hold data move; a fresh install holds none. B writes the step generically.
    Verdict: **format**.

Scenario 6 (snapshot bootstrap)

11. **A beat from Mac after the rejoin.**
    A (15): "a beat is due (last beat 400 days ago) and enqueued after `Q` (ctr 62, `state` absent: no hash at 19968). Append batch of 2 → seqs 20001, 20002"; final head `(20002, C20002)`, MAC `last_ctr 62`.
    B (21, 22): only the rename, ctr 61 → seq 20001; final head `(20001, C20001)`, Mac `last_ctr 61`.
    Spec: engine.md §23.1 condition 1; ENG-BEAT-001; §29.1 (Mac is eligible once `Live` at the head); §23.2 (`state` absent when the device bootstrapped from a snapshot at or above the aligned `S`).
    Verdict: **reader** (B). The rule is plain. No change beyond TR2-JOIN-1, which concerns the join.

12. **What the rejoin prompt says.**
    A (7): "'This copy is too far behind the relay to catch up from seq 101; replace its log with a snapshot?' with Confirm".
    B (9): "Mac shows that this device was away longer than the relay keeps history and asks to rebuild from the newest checked snapshot (the rename stays queued)." and "AMBIGUOUS: the wording and the facts the prompt must carry … are not given by engine.md §19.7 step 1 or §30."
    Spec: engine.md §19.7 step 1 gives only the `StopView { kind: Rejoin, …, action: ConfirmRejoin }`; device.md §17.7 gives only the call `confirm_rejoin()`; bridge.md §13.1 "Devices: alarms" gives only the action. No spec gives the words.
    Verdict: **spec**. TR2-JOIN-4. For the lead: point 1.

13. **Whether the user is told before a 400-day-old queued edit is sent.**
    A (15, 16): seals `Q` with its old `at_ms` and `based_on`, with no prompt and no comment.
    B (24): does the same, plus "AMBIGUOUS: a 400-day-old queued edit is accepted and overwrites with `at_ms` 400 days in the past and `based_on 100`; neither engine.md §19.7 ('records in the outbox survive a rejoin') nor core.md says whether the user is warned before such a stale edit is sent."
    Spec: engine.md §19.7, last paragraph; ENG-ENQ-005. Neither says whether the rejoin asks about, marks or holds back surviving records.
    Verdict: **spec**. Both readers already act this way; TR2-JOIN-5 writes it down. For the lead: point 2.

14. **Whether `Q` was sealed before Mac went away.**
    A (1): "AMBIGUOUS: whether `Q` was ever sealed …; I take it unsealed."
    B (world): "one unsealed record `RQ`", with no comment.
    Spec: ENG-ENQ-005 keeps `based_on` and `at_ms`, and the relay's `last_ctr` for Mac is 60, so the frame is the same either way: ctr 61, epoch 1, `based_on (100, C100)`. Only the moment of the nonce draw differs. This is a fact of the world, not of the specs.
    Verdict: **format**. For the lead: point 3.

15. **The welcome's `floor`.**
    A (3): "AMBIGUOUS: `floor = max(compact_floor, purge_floor) = 3000` from 'no longer holds entries below 3,000'".
    B (4): "`floor 3000` (`max(3000, 1)`)".
    Spec: relay.md §2 glossary "floor | `max(compact_floor, purge_floor)`"; welcome table row `floor`. The world's wording describes `compact_floor = 3000`.
    Verdict: **reader** (A). The spec is clear and both readers give the same value. No change.

16. **When Phone's `DeviceAdded` lies (the history records Mac appends).**
    A (12): "none if the Phone's `DeviceAdded` is ≤ 100, AMBIGUOUS: the world does not say when Phone was added".
    B (world): "seqs 1, `a_Mac`, `a_Phone`, all ≤ 100".
    Spec: the world says "Membership has not changed since the board was created", and Mac, which held 1 to 100, held Phone's record. Also, device.md DEV-RUN-006 appends only records above the last seq the file holds, so either case is defined.
    Verdict: **reader** (A). No change.

17. **Whether Mac sees Phone's acknowledgement.**
    A (13): "AMBIGUOUS: that Phone's acknowledging beat lies above 19968 …; if not, step 5 tries the next candidate."
    B (world, 18): the `snap` entry at `s_n` and the ack at `s_b`, with `19968 < s_n < s_b ≤ 20000`.
    Spec: engine.md §19.4 step 4 enqueues the `snap` record only after the upload of the snapshot at `S`, so `s_n > 19968`; an `ack` names `s_n` and so comes after it; the world says Phone acknowledged it.
    Verdict: **reader** (A). No change.

18. **Whether the renamed task is still live at 20000.**
    A (16): "AMBIGUOUS: the scenario does not say whether the task is still live at 20000."
    B (20, 23): gives both branches without the AMBIGUOUS mark.
    Spec: this is a fact of the world, and both readers give the same outcome for each branch (core.md `core/gone` or an overwrite notice).
    Verdict: **format**.

# Changes

## seal.md

### TR2-JOIN-3 (difference 4)

Section §20.1 "Blob ids": after the paragraph's first sentence ("A blob id is 16 bytes from `Entropy`, drawn by the writer before sealing."), insert the sentence:

Within one sealing call (`seal_snapshot`, and every function of §26 that seals a blob), the `blob_id` is the first value drawn from the call's `Entropy` and the nonce of §20.2 the second; nothing else is drawn in between.

and after SEAL-BLOB-005, the clause:

- **SEAL-BLOB-010** (seal) A function that seals a blob MUST draw the `blob_id` (16 bytes) and then the nonce (24 bytes) from its `Entropy` argument, in that order, and MUST draw nothing else between or before them.

(No table row, limit or signature is touched.)

## engine.md

### TR2-JOIN-1 (difference 1)

Section §19.8 "Bootstrap at creation and at pairing", the **Join** bullet: after its last sentence ("Before that, `Enqueue` is refused with `engine/not_joined`."), add:

Its `Meta` starts with `last_beat_ms = 0` (ENG-BEAT-003). So the first time duties are evaluated (§4.5 step 5) after it is eligible (§29.1), §23.1 condition 1 holds and it enqueues a beat. Its first entry is therefore that beat, sealed with counter 1, ahead of any command the user enqueues later.

(No clause id changes; ENG-BEAT-001 and ENG-BEAT-003 already state the rule.)

### TR2-JOIN-2 (difference 2)

Section §14.2 "Banners", the paragraph after the code block, replaced in full by:

A banner is created on every device except the entry's author device. This includes the device that a `DeviceAdded` names: it applies that entry after its join (§19.8), so it shows a `DeviceAdded` banner about its own addition. There is one exception: `RecoveryProposed` is created on every device, the proposer's included. A banner stays until the user acknowledges it on that device (`UserAction::AckBanner`), except `RecoveryProposed`, which cannot be acknowledged while its proposal is pending and is marked acknowledged when the proposal is confirmed or cancelled (SYNC.md §5.3 step 6, §5.5; V1-2). Banners are persisted in `Meta` (at most 256; when full, the oldest acknowledged ones go first, then the oldest).

and ENG-BANNER-001 replaced by:

- **ENG-BANNER-001** (engine) The engine MUST create a banner for every accepted control item of §14.1 that names one, on every device but the author's (the device a `DeviceAdded` names included), with the author's name and key fingerprint.

### TR2-JOIN-5 (difference 13)

Section §19.7 "Rejoin after a long absence", the paragraph "Records in the outbox survive a rejoin; …" replaced in full by:

Records in the outbox survive a rejoin; frames sealed under an epoch that no longer applies are refused and re-sealed (§24.3). The rejoin asks the user nothing about these records and marks none of them. Once it has ended, the pending view rebases them on the snapshot's state and the log after it (§8.4). The send loop then seals and sends them in order, with the `based_on` and `at_ms` they were enqueued with, however old (ENG-ENQ-005). A change of theirs that overwrites a later change yields an overwrite notice (`core.md` §10.2); one that no longer applies is refused, as for any record. The rejoin prompt tells the user that these records stay queued (`device.md` §17.7, `bridge.md` §13.1).

and after ENG-BOOT-007, the clause:

- **ENG-BOOT-008** (engine) The engine MUST keep every outbox record through a rejoin and send it once the rejoin has ended, with the `based_on` and `at_ms` it was enqueued with, and MUST NOT drop a record, hold it back or ask about it because of the rejoin or of its age.

## device.md

### TR2-JOIN-4 (difference 12)

Section §17.7 "Notices, banners, alarms, the halted view": after the paragraph that TR2-EDIT-4 adds (after "The halted view is the pending view …" when TR2-EDIT-4 is dropped), add the paragraph:

While `status().engine.stops` holds the stop of kind `Rejoin` (`engine.md` §19.7 step 1), the app shows the line "This device was away longer than the relay keeps its history. Rebuild it from the newest checked snapshot? Changes not yet sent stay queued and are sent after the rebuild." with the button "Rebuild", which calls `confirm_rejoin()`.

and the clause (after DEV-FACADE-010 of TR2-EDIT-4):

- **DEV-FACADE-011** (desktop) While `status().engine.stops` holds a stop of kind `Rejoin`, the desktop app MUST show the line "This device was away longer than the relay keeps its history. Rebuild it from the newest checked snapshot? Changes not yet sent stay queued and are sent after the rebuild." with a button "Rebuild" that calls `confirm_rejoin()`.

(The `confirm_rejoin()` row of the §17.7 table is unchanged.)

## bridge.md

### TR2-JOIN-4 (difference 12)

Section §13.1 "What the phone does": in the screen table, after the row "Devices: alarms", add the row:

| Devices: rejoin | while `status().engine.stops` holds the stop of kind `Rejoin` (`engine.md` §19.7 step 1), the line "This device was away longer than the relay keeps its history. Rebuild it from the newest checked snapshot? Changes not yet sent stay queued and are sent after the rebuild." | Rebuild (`confirm_rejoin`) | all |

and after BRIDGE-SCREEN-001, the clause:

- **BRIDGE-SCREEN-002** (android) While `status().engine.stops` holds a stop of kind `Rejoin`, the Devices screen MUST show the line "This device was away longer than the relay keeps its history. Rebuild it from the newest checked snapshot? Changes not yet sent stay queued and are sent after the rebuild." with a button "Rebuild" that calls `confirm_rejoin`.

(The `confirm_rejoin` row of the call table is unchanged. The "Devices: alarms" row keeps `ConfirmRejoin` among the stop actions it lists.)

## For the lead

1. **The rejoin prompt's words (difference 12; TR2-JOIN-4).** No spec gives them, so the readers wrote two different prompts. This is the same question as the edit group's point 1 (TR2-EDIT-4): do the sync specs fix the words shown to the user? Recommendation: adopt TR2-JOIN-4 as written. It is the conservative reading: one fixed line on both platforms, which tests can check exactly. If the lead rules that wording lies outside the sync specs, drop TR2-JOIN-4 and the last sentence of TR2-JOIN-5's paragraph ("The rejoin prompt tells the user …").
2. **Stale queued edits after a rejoin (difference 13; TR2-JOIN-5).** Should a device warn the user before it sends an edit queued long ago, or let them review it? This is a product question. Recommendation: adopt TR2-JOIN-5 as written. It is the conservative reading and states what the specs already imply: records are sent unchanged, overwrite notices report what they overwrote, and the prompt of TR2-JOIN-4 says they stay queued. A review step for old records would be a new feature for a later pass.
3. **World facts the brief leaves open (differences 1, 14, 16, 18).** These are not spec questions, but they produced AMBIGUOUS lines. Recommendation: in scenario 3, change "has sent its first entry (the user adds a task 'Hello')" to "has sent its first entries (its beat, then the user adds a task 'Hello')". In scenario 6, add that Mac's queued edit was never sealed, that the renamed task is still live at 20,000, and that Phone's `DeviceAdded` lies at or below seq 100.
