# Engine test vectors

Vector families of `docs/sync/spec/engine.md` §35, in the format of `docs/sync/spec/README.md` §7:
`{"spec": "engine.md §…", "cases": [...]}`, one JSON file per family. Byte strings are lowercase
hex; integers are JSON numbers below 2^53 in magnitude; names and devices are made up.

Every case has a `"name"` and `"clauses"`: the clause ids and section numbers of `engine.md` it
follows from. A case may carry a `"note"` (prose, not checked).

**Absent means undecided.** When the spec does not decide an expected field for a case, the field
is left out of that case, and a loader must not check it. The questions that left them out are
listed at the end of this file.

The seven families below were derived from the spec by the generators in
`docs/sync/status/engine/tools/vectors-b/` (one per family), never from the engine's code.

## `engine-hash.json` (§18.3)

| Field | Meaning |
|---|---|
| `hash_version` | the `u32 hash_version` of the input; 1 in every case (§5.2: only version 1 exists) |
| `board` | optional: canonical toy state bytes (§5.3); when present, `board_hash = SHA-256(board)` (§5.1 `board_hash`) |
| `board_hash` | the 32-byte board hash fed to the state hash |
| `membership` | the membership bytes (any bytes: the state hash does not parse them) |
| `membership_hash` | `SHA-256(membership)`, for convenience |
| `input` | expected: the 86 bytes `ctx("state") ‖ u32 hash_version ‖ board_hash ‖ SHA-256(membership)`, `ctx("state")` = `"taskdeck/v1/state"` and one `0x00` byte (§18.3 table, ENG-HASH-001) |
| `state_hash` | expected: `SHA-256(input)` = `taskdeck_sync::state_hash(hash_version, board_hash, membership)` (§37) |

## `engine-pending.json` (§8.2, §8.3, §8.5, §5.3)

Inputs:

| Field | Meaning |
|---|---|
| `rules2` | construct the toy with `rules2 = true` (§5.3 "Other methods"); needed to load a state with `rules = 2` |
| `confirmed` | the confirmed state's canonical toy bytes (§5.3) |
| `board_seq` | `B` |
| `device_no` | this device's `device_no` in `M_B`, or `null`: a device that has not joined |
| `records` | the outbox records in `order`; each: `order`, `kind` (`cmd`, `ctl`, `beat`, `snap`), `payload` (the toy command's bytes for `cmd`, else `null`), `at_ms`, `based_on_seq` (`based_on.seq`; the chain is irrelevant), `seen` (u64 or `null`) |

Expected:

| Field | Meaning; decided by |
|---|---|
| `view` | the pending view's canonical bytes (§8.2 step 4; ENG-PEND-001); for `device_no: null`, the confirmed state (§8.2 last paragraph) |
| `unapplied` | absent for `device_no: null`. Otherwise one entry per record of `U` (§8.2 step 1: `cmd` records whose `seen` is `null` or above `B`, in `order`); records of other kinds and records seen at or below `B` have none |
| `unapplied[].order` | the record it describes |
| `unapplied[].provisional_seq` | `B + i`, `i` its 1-based position in `U` (§8.2 step 3), doomed records counted |
| `unapplied[].doomed` | `null`, or the refusal's code, or `engine/trial_panic` (§8.2 step 3, ENG-PEND-002, §8.3) |
| `unapplied[].notices` | the toy notice bytes (§5.3, 21 bytes each) that `apply` returned for the pending view, shown with the record and never recorded (ENG-SM-004, §13.4). Empty for a doomed record |

The `Ctx` of each apply is `{seq: B + i, device_no, at_ms, based_on: min(based_on_seq, B)}` (§8.2
step 3). After a panic the view is rebuilt from the confirmed state by re-applying the records
before it that were not doomed.

## `engine-receipts.json` (§13.1 to §13.4, §24.3)

`receipt` (the input):

| Field | Meaning |
|---|---|
| `seq` | the entry's seq |
| `class` | `Skip`, `Tamper`, `Refuse`, `Accept`, `NeedFrames`, or `SealerError` (the `classify` error `seal/missing_root`, §13.1 last row) |
| `code` | the receipt's code, or `null` |
| `kind` | `cmd`, `ctl`, `beat`, `snap`, or `null` (not opened) |
| `own` | the author is this device and the entry's `entry_id` is an outbox record's (§12.4 step 3) |
| `own_dev_foreign`, `based_on_unchecked` | the receipt's flags (seal.md §14.4) |
| `halt` | `null`, `Commitment`, or `UnknownRules` (the sealer's `Halt`) |
| `in_skips` | the seq is a target in `M_K.skips` when the board applies it (§12.4 step 1) |
| `reseals` | the own record's `reseals` before this receipt (§24.3) |
| `first_apply` | the entry is applied to the board for the first time (not a replay from a base at `Start`, §13.1 `based_on_unchecked` row) |
| `parse` | for an accepted `cmd`: `ok` or `fail`; `parse_code` the parser's code on `fail` |
| `apply` | for an accepted `cmd` that parses: `ok`, `benign`, `refused` (`apply_code` its code), `panic` |
| `revoked_in_prev` | for `SealerError`: this device is revoked in `M_{s−1}` |

`expected`:

| Field | Meaning; decided by |
|---|---|
| `frame_persisted` | the frame is in the page's `AppendLog`: false only for `Tamper`, the cut (§12.3 step 6, ENG-CLASS-003). Absent for `NeedFrames` and `SealerError` |
| `membership_changes` | `M_s ≠ M_{s−1}`: true for `Refuse` and `Accept` (the counter is consumed), false for `Skip`, `Tamper`, `NeedFrames`, and a `Refuse` at seq 1 (seal.md §13.5) |
| `board` | `applied` (an accepted `cmd` applied, a benign repeat included), `ctl_items` (§14.1), `held` (the rules halt keeps the board at `seq − 1`, §16.1), `none` |
| `alarm` | `null` or `{kind, evidence, …}`: the `AlarmKind`, the `Evidence` variant, and the `AlarmRecord` fields of §21.1's table: `seq`, `author` (`"author"` = the frame's author, `"this_device"`), `code`, `source` for `Fork`, `frame_hash_some` (whether `Evidence::Frame.frame_hash` is `Some`) |
| `stop` | `null` or `{kind}` (`StopKind`: `Evidence` §21.2, `ForeignSigner` §21.1, `LocalFault` with `code` §21.4). Absent where no new stop is decided |
| `halt` | `null`, `Rules` or `Commitment` (§16.1) |
| `refusal_record` | `null`, or the code of the refusal record appended (§13.3, ENG-REF-001, §24.3) |
| `own_record` | `none` (not an own entry), `dropped` (`seen = s`, `Outbox[Drop]`, §12.4 step 3), `kept_for_reseal` (frame cleared, `reseals + 1`, §24.3), `untouched` |
| `based_on_unchecked_rises` | `meta.based_on_unchecked` rises by 1 (§13.1) |
| `poison` | the `apply` panicked: §15 decides what follows (see `engine-poison.json`) |
| `registry` | for an accepted `beat` or `snap`: the registry is updated (§23.3, §19.5) |
| `need_frames` | for `NeedFrames`: the frames are fetched and the entry classified again (§17.2) |
| `paused` | for `SealerError` on a revoked device: classification pauses without a local fault (§24.4, ENG-EPOCH-005) |

## `engine-poison.json` (§15, §16.1, §16.2, D5, D6)

Inputs: `start` = `{attempt, last_failed ([seq, attempt] or null), marker ([first, last, attempt]
or null), checkpoint_seq (B at Start), log_head (L)}`; `script`, a list of steps: `"start"` (a
`Start`) or `{"page": [seqs]}` (a page of these seqs arrives and is persisted with its marker);
`outcomes`, consumed in order by each apply: `ok`, `panic` (a caught panic), `kill` (an OS kill:
the process dies; the next step is a `"start"`). No checkpoint is written within a script (none of
§18.1's cases arises).

Expected: `events`, in order, each one of:

| Event | Meaning; decided by |
|---|---|
| `{"step": …}` | echo of the script step being run (not an engine effect) |
| `{"attempt": n}` | `WriteMeta` raising `attempt` to `n` (§15.1, D5) |
| `{"last_failed": [x, a]}` | `WriteMeta` setting `last_failed` (§15.2, §15.3, D6) |
| `{"set_marker": [first, last, attempt]}` | `SetMarker` (ENG-POISON-001, §15.2) |
| `{"apply": s, "mode": "page"/"single", "outcome": …}` | the board applies `s` in page or single mode; `clears_last_failed: true` when this clean single apply clears a `last_failed` naming `s` (§15.3 last sentence) |
| `{"clear_marker": true}` | `ClearMarker` after every entry of the marker applied (ENG-POISON-005) |
| `{"halt": x, "code": "engine/crash_halt"}` | the crash halt on `x` (§15.2, §15.3, §16.1); the run stops |

`halt` is the halting seq or `null`. A loader compares the sequence of engine events (the
`step` echoes excluded).

## `engine-duties.json` (§29.1, §29.2, §19.2, §19.3, §23.1)

`kind: "rank"`: `devices` (`dev` 16 bytes, `status` `current`/`revoked`, `beat_ms`) → `order` (the
current devs, `beat_ms` descending, then `dev` ascending as unsigned bytes) and `ranks` (0-based
positions) (§29.2, ENG-DUTY-001).

`kind: "beat"`: `wall_ms`, `last_beat_ms`, `eligible` (§29.1), `unsent_beat` (a beat record not yet
sent), `build` and `record` (`rules`, `envelopes` as `[ver, suite]`, `hash_versions`: the build's
lists and this device's record in `M_K`), `pending_verdicts` (count) → `conditions` (which of
§23.1's conditions 1 to 3 hold: 1 `wall_ms − last_beat_ms ≥ 86,400,000`; 2 the lists differ; 3 a
pending verdict and `wall_ms − last_beat_ms ≥ 3,600,000`) and `due` (eligible, no unsent beat, and
one condition) (ENG-BEAT-001).

`kind: "snapshot"`: `self` (this device's dev), `S`, `tau_S`, `snaps` (the registry's
`SnapRecord`s: `entry_seq`, `author`, `info_seq` = `info.seq`, `entry_clock`, `verdicts` from
beats `[{dev, verdict, beat_seq}]`, `own` = this device's verdict or `null`), `own_snap_in_outbox`,
`owed_mismatch` (§19.2 condition 2) → `due` (§19.2, ENG-SNAP-001, ENG-SNAP-006) and `reasons`,
which lists every one of these that holds: `own_snap_in_outbox`, `within_24_hours` (the newest own
`SnapRecord` has `tau_S − entry_clock < 86,400,000`), `recent_usable_snapshot` (a usable one, §19.3,
with `tau_S − entry_clock < 604,800,000` and `S − info_seq < 1,000`),
`recent_snapshot_waiting_for_verdicts` (one with no verdict at all and `tau_S − entry_clock <
172,800,000`), `condition_1` (neither of the two before), `condition_2`. Usable (§19.3): an `Ack`
from a device other than the writer (this device's `own` counts as `self`'s) and no `Mismatch`.
When present, `write` gives `wall_ms`, `rank`, `eligible`, `snap_accepted_at_or_above_S` →
`earliest_wall_ms` = `tau_S + rank × 3,600,000` and `writes_now` (ENG-SNAP-002).

## `engine-clock.json` (§27.1, §27.2)

Inputs: `measurements` (oldest first; each `wall_ms`, `relay_now`, `mono_ms` at the measurement;
`skew = wall_ms − relay_now`), `confirmation` (`{wall_ms, skew_ms}` = `meta.clock`, or `null`),
`now` (`wall_ms`, `mono_ms` of the moment asked about). Expected: `skew_ms` (the latest
measurement's skew, or `null` when it is more than 86,400,000 ms of `mono_ms` old or there is none),
`warning` (`|skew| > 300,000`), `blocked` (`|skew| > 3,600,000` and no valid confirmation), and,
when a confirmation and a valid measurement exist, `confirmation_valid` (`now.wall_ms − c_wall ≤
604,800,000` and `|skew − c_skew| ≤ 300,000`) (ENG-CLOCK-002, ENG-CLOCK-003). No case sits at exactly
86,400,000 ms of `mono_ms` (see the questions).

## `engine-reconcile.json` (§10.3, §11.2, §11.3, §11.4)

`kind: "welcome"`. Inputs: `a` (this device's `last_ctr` in `M_K`), `has_record` (`M_K` holds a
record of this device; when false `a` is taken as 0, ENG-RECON-009), `records` (outbox records:
`ctr` or `null`, `frame` (holds a frame), `ack` `[seq, chain]` or `null`), `L`, `local_chains`
(this device's `C_s` by seq, as a string key: for `L` and for the welcome's `head.seq` when it is
at most `L`), `welcome` (`head` `{seq, chain}`, `last_ctr` = `r`, `last_seq` = `q`),
`unresolved_alarms` (kinds `Meta` holds unresolved), `a_after` (optional: `a` once the log has
reached `q`). Expected:

| Field | Meaning; decided by |
|---|---|
| `h` | `max(a, largest ctr of a record holding a frame)` (§10.3, ENG-RECON-008) |
| `evidence_found` | the evidence kinds §11.2 steps 2, 3 and §11.3 step 1 find, sorted |
| `alarms` | the alarms raised, at most one per kind and none of an unresolved kind (§11.2 "One alarm per kind", ENG-SESS-008): `RelayBehind` with `relay_head`, `relay_last_ctr`, `local_head` `(L, C_L)`, `ack` (the acknowledgement with the largest seq above `head.seq`, or `null`); `Fork` with `seq` = `head.seq`, `local_chain`, `relay_chain`, `source: "Welcome"` |
| `reconcile` | `full`, or `step_1_only` when any step found evidence (§11.2) |
| `step` | the step of §11.3 that applies (1, 2, 3); with `step_1_only`, 1 when step 1 found `relay_behind`, else `null` |
| `restored` | `registry.restored` set: `[h + 1, r]` for step 3, else `null` (§11.3 step 3; never with evidence) |
| `seal_before_q` | whether anything may be sealed before the log reaches `q` (§11.3 steps 2 and 3, ENG-RECON-001) |
| `withheld` | step 3 only meaningful: `a_after < r` raises `withheld` (§11.3 step 3, ENG-RECON-004) |
| `next_ctr` | once done, `1 + max(a_after, h_after, w)`, `h_after` from `a_after` and the records' frames, `w = r` after step 3 (§10.3, §11.3 step 3) |

`kind: "conflict"`: a `conflict` result for record `x`; `seq` (the result's seq, 0 or not),
`open_own` (`x`: the entry is `x`'s; `other`: another record `y`'s; `none`: no record's; `error`:
`open_own` fails), `conflicts_before`. Expected (§11.4, ENG-RECON-005, ENG-RECON-006):
`acknowledged` (`x`, `y` or `null`), `cleared` (`x` or `null`: the record whose frame is cleared for
re-sealing; absent when `identity_clone` is true), `alarm` (`RestoredCopy`, `RelayProtocol` or
`null`), `rehello` (the reconcile runs again before anything is sealed), `conflicts_after`,
`identity_clone` (the engine stops with `identity_clone`).

`kind: "gap"`: `last_ctr` (`r`), `last_seq`, `records` (`ctr`, `ack`: holds an acknowledgement) →
`relay_behind` (a record with `ctr > r` holds an acknowledgement) or `resend_from_ctr` = `r + 1`
(§11.4, ENG-RECON-007).

## Questions the spec leaves open (cases left out)

1. §13.1 `Skip` row gives the `skipped_frame` alarm the "author `dev` as the frame says"; §21.1's
   table gives `SkippedFrame` no `author` ("a field the row does not name is absent"). The
   receipts vectors leave `author` out of `SkippedFrame`.
2. §12.4 step 3 matches an own entry by its `entry_id`, but `Refuse` returns an `entry_id` only "if
   the entry opened" (seal.md §14.4), and `seal/old_epoch` (always) and `seal/stale_epoch` (at step
   9) refuse before the AEAD opens: how the engine finds the record to re-seal (§24.3) is not
   stated. The receipts vectors take `own` as given.
3. ENG-REF-001 and §13.3 give a refusal record only to an own entry refused for a reason other
   than a re-sealable code, but the §13.1 `Refuse` row ("unless re-sealable") and §24.3 (an own
   `beat`/`snap` that is refused "is dropped"; a `ctl` "fails its operation's stage") speak of own
   `beat`/`snap`/`ctl` entries refused with `seal/stale_epoch` or `seal/old_epoch` as if they
   were not re-sealed, so whether such an entry leaves a refusal record is not decided: those
   cases are left out (only `cmd` is re-sealed, §24.3). For a `beat`/`snap`/`ctl` refused with a
   non-re-sealable code the vectors follow ENG-REF-001; `core.md` §10.7 refusal records describe
   commands, and what `op`/`target` such a record carries is not stated.
4. Whether `based_on_unchecked` counts an accepted entry whose `apply` is refused, or a `beat`,
   `snap` or `ctl` ("applied to the board"): left out.
5. `seal/missing_root` on a revoked device: whether the frame of the paused seq is persisted, and
   which stop (if any) is recorded under case 3 of §24.4.
6. §15.3 step 2: whether the rebuild "from the checkpoint and the log, page-wise" writes markers
   (and which), and how the rest of a page after `x` continues once `x` applied alone. Poison cases
   therefore panic only on the entry right after the checkpoint and at the end of a page.
7. §15.2 steps 2 and 3: the page size of the page-wise replay at `Start` (no case replays more
   than one entry page-wise at `Start`, so its marker covers that one entry whatever the page
   size; a page that arrives in a `{"page": …}` step is one page), and, when the start halts on
   `x` above `B + 1`, whether `B + 1` to
   `x − 1` are applied first.
8. §27.1: whether a measurement exactly 86,400,000 ms of `mono_ms` old has expired ("after").
9. §11.4: when `conflicts` reaches 3, whether `x`'s frame is still cleared ("instead of
   re-sealing").
10. §11.4 `gap`: what is resent when no record holds the counter `r + 1`.
11. §11.3 step 3: a record holding a frame at a counter `≤ h` whose hash differs from the relay's
    is cleared, but that counter is outside `h + 1..=r`, so `own_sealed` supplies this device's
    hash and the relay's frame would be classified `own_dev_foreign` (step 4). Left out.
12. §8.2: `based_on.seq` above `B` (which `min` guards against) cannot arise from §9 (`based_on` is
    `(B, C_B)` at enqueue and `B` never decreases); the pending vectors include it as given input.
13. §8.2 last paragraph: provisional seqs and doomed flags of a device with no `device_no`.
