# Task B: the expected values of seven vector families

Working copy: `/home/user/wt/b` (branch `wt/b`). `CARGO_TARGET_DIR=/home/user/wt/target-b` (you
will hardly need cargo). Read `docs/sync/status/engine/briefs/README.md` first: its rules hold here.

## Why

`engine.md` §35 lists the vector families stage `engine` publishes. The engine's author (the lead)
writes the tests that load them, and must not be the author of the values they expect. You derive
the expected values **from the spec**, with code of your own where a value must be computed
(Python, standard library only), and never by running, reading or imitating the engine's
implementation. Where the engine and your values later differ, the spec decides, so every case
must be traceable to a sentence of the spec.

## What to write

Seven files under `crates/taskdeck-sync/tests/vectors/`, in the format of
`docs/sync/spec/README.md` §7 (`{"spec": "engine.md §…", "cases": [{"name": …, inputs…,
expected…}]}`; byte strings lowercase hex; integers as JSON numbers, all below 2^53; names made
up). Each case also carries `"clauses"`: the clause ids or section numbers it follows from.

| File | Spec | Each case |
|---|---|---|
| `engine-hash.json` | §18.3 | `hash_version`, `board_hash` (or `board` bytes and its hash), `membership` bytes → `input` (the 86 bytes) and `state_hash`. Real SHA-256. At least 6 cases, among them empty membership bytes and a long one. |
| `engine-pending.json` | §8.2, §8.3, §8.5, §5.3 | a confirmed toy state (canonical bytes of §5.3), `board_seq` (`B`), this device's `device_no` (or null: not joined), the outbox records in `order` (each: `kind`, the toy command's payload bytes, `at_ms`, `based_on_seq`, `seen` or null) → the pending view's canonical bytes, and for each unapplied `cmd` record its provisional seq and `doomed` (null or the code). Cover: no record; successes; a refusal (doomed, view unchanged); a `Panic` (doomed `engine/trial_panic`, view rebuilt from the records before it); a doomed record followed by one that succeeds; records already seen at or below `B` (excluded); `beat`/`ctl`/`snap` records (excluded, and they take no provisional seq); `based_on = min(r.based_on.seq, B)` making a difference to a `Set` notice (notices of the pending view are never recorded); a device with no `device_no`; every toy op at least once; a `Create` whose id depends on the device's mark. |
| `engine-receipts.json` | §13.1, §13.2, §13.3, §13.4, §24.3 | one row of the table of §13.1 made concrete: the receipt (`class`, `code`, `kind`, whether the author is this device and the `entry_id` is a record's, `own_dev_foreign`, `based_on_unchecked`, `halt`, and for an accepted `cmd` whether `parse` fails and what `apply` does: ok, benign, refused with a code, panic) → what the engine does: is the frame persisted; does the membership change; does the board change; the alarm kind and `Evidence` variant if any; the stop if any (kind); the halt if any; is a refusal record appended and with which code; is the own record dropped, kept for re-sealing, or untouched; does `meta.based_on_unchecked` rise. Cover every row, every code of §13.2 and at least four codes outside it, the two re-sealable codes for own `cmd`, own `beat`/`snap`/`ctl` refused, another device's entry refused (no record), an own entry skipped by a Skip (`engine/skipped`). |
| `engine-poison.json` | §15, §16.1, §16.2, D5, D6 | a start state (`attempt`, `last_failed`, marker or none, the checkpoint's `B`, the log's head) and a script of steps (`start`; `page` of seqs; the outcome of applying each seq: `ok` or `panic`; `kill` while a marker is in place) → in order, the markers set and cleared, each `attempt` and `last_failed` written, whether the mode is page or single for each seq, and whether and at which seq the device halts (`engine/crash_halt`). Cover: a clean page; an OS kill in a page (no halt, single mode on the next start, `last_failed` only for a single-entry marker); a deterministic panic (halts within one run after its second attempt); a panic that does not repeat (clears `last_failed`); a single-entry marker left by a kill twice in a row (halt); a kill, then a panic on another entry. |
| `engine-duties.json` | §29.1, §29.2, §19.2, §19.3, §23.1 | three kinds of case, told apart by `"kind"`: `rank` (the devices of a membership view with `dev`, `status`, `beat_ms` → the duty order and each current device's rank; ties on `beat_ms`, revoked devices left out, `dev` compared as unsigned bytes); `beat` (`wall_ms`, `last_beat_ms`, eligible or not, an unsent beat record or not, the build's lists against the device's record in `M_K`, pending verdicts → due or not, and by which condition); `snapshot` (the aligned `S`, `τ_S`, the registry's `SnapRecord`s with their verdicts and `entry_clock`, an own `snap` record in the outbox or not, an owed mismatch snapshot or not → due at `S` or not and why; then `wall_ms`, the rank, eligible, a `snap` entry accepted at or above `S` → writes now or not, and the earliest `wall_ms`). Cover each boundary on both sides (86,400,000; 604,800,000; 172,800,000; 1,000 entries; 3,600,000 per rank), usable against not usable (§19.3), the 24-hour limit. |
| `engine-clock.json` | §27.1, §27.2 | measurements (`wall_ms`, `relay_now`, `mono_ms`), a confirmation or none, and the moment asked about (`wall_ms`, `mono_ms`) → `skew_ms` (or null: no valid measurement), `warning`, `blocked`. Cover both sides of 300,000 and 3,600,000 in both directions, an expired measurement (86,400,000 of `mono_ms`), a valid confirmation, one too old (604,800,000), one whose skew drifted more than 300,000. |
| `engine-reconcile.json` | §10.3, §11.2, §11.3, §11.4, §10.4 | three kinds: `welcome` (`a`, the records with their `ctr`, frame or not, `ack`; `L`; the welcome's `head.seq`, `last_ctr`, `last_seq`, and whether the chain at the head matches → which step of §11.3 applies, `h`, the evidence if any with its fields as §11.2 "One alarm per kind" states, `registry.restored` if set, whether anything may be sealed before the log reaches `q`, and `next_ctr` once done); `conflict` (the conflicting record, the result's `seq`, and what `open_own` returns: this record's id, another record's, no record's, or an error; `conflicts` before → which record is acknowledged, which frame is cleared for re-sealing, the alarm, `conflicts` after, and whether the engine stops with `identity_clone`); `gap` (`last_ctr`, `last_seq`, the records' acks → `relay_behind` or the counter to resend from). |

Also `crates/taskdeck-sync/tests/vectors/README.md`: for each family, the exact meaning of every
field of a case, enough for someone to write a loader without asking; and which spec sentence
decides each expected field.

Keep the scripts that generated the values under `docs/sync/status/engine/tools/vectors-b/` (one
Python file per family, runnable as `python3 <file> > <the json>`, standard library only, no
network, no clock, no randomness beyond fixed seeds), so a later reader can see how each value
follows from the spec. A README line there says how to run them.

Where the spec does not decide an expected value, or two clauses disagree, do not guess: leave the
case out and list the question in your report with the clause ids. That list is as valuable as the
vectors.

## Files you may change

`crates/taskdeck-sync/tests/vectors/engine-{hash,pending,receipts,poison,duties,clock,reconcile}.json`,
`crates/taskdeck-sync/tests/vectors/README.md`, `docs/sync/status/engine/tools/vectors-b/**`.
Nothing else, and no Rust.

## Done means

- every file is valid JSON in the README §7 format (`python3 -m json.tool` accepts each), and
  running each generator again gives the committed file byte for byte;
- `bash scripts/spec-lint.sh` still passes;
- everything is committed on `wt/b`, `git status` is clean.

Report: per family the number of cases and what they cover; the spec questions you met (clause
ids); anything you left out and why.
