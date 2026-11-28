# Task D7: a device retired while cut off, the dropped seq above the final head, and the oracle that follows every move

Base: the commit that holds this brief. Read `d-simulator.md`, `d2-simulator-followup.md`,
`d3-gate-seeds.md`, `d4-sim-tests.md`, `d5-check-faults.md` and `d6-decided-invariants.md` in this
directory: their rules still hold (an invariant is read strictly as worded below; you do not read
`crates/taskdeck-sync/src/engine/**`; you change only the files this task lists; the simulator's
own tests build the situations they check by hand, in `src/sim/script.rs` or beside the check,
and none names what a seed produced; no test is skipped, ignored or loosened).

## What changed

**The owner decided (2026-11-25)** two more questions about the gate (the status file's Q7 and
Q8), and the lead made one behaviour change to the engine that the simulator's oracle must
follow. The new text of `engine.md` §31.7 follows; it is the contract for this task. It goes into
the spec in pull requests of their own while you work, so your checkout still shows the old text:
use this one. What changed against D6's text: the oracle's user types move codes; the removed
relay state takes one more case (a dropped seq above the final head); invariant 2 has a new last
bullet (a device **retired** while cut off), invariant 3 names it; invariant 4's last clause
excepts an injected frame above a `Fence`.

### 31.7 The six invariants

Let the **reference log** be the final model relay's log after the settle, or, in a run with a fork, the log of the branch the oracle names. The **oracle** is a fresh `Engine<ToyMachine, FakeSealer>` with the genesis anchor that replays the reference log from seq 1 with no faults; its user types the move code of each accepted `Fence` that awaits one (§28.3 "On every other device" item 2), so that it follows every move in the log, which a relay fenced at a `Fence` would not let it read past (§12.2).

A **removed relay state** is a part of a relay's history that a rollback or fork fault took away from the reference log: in a run with a fork, every branch the oracle does not name; after a rollback, each dropped seq at which the reference log, after the settle, holds a different entry, or holds none because every restore tail that would have brought it back (§21.3) was refused for a frame the relay made up (`relay.md` §12.9), or holds none above its final head while a lower seq that the same rollback dropped holds a different entry (no restore tail can bring back a frame whose chain no longer leads to it). A dropped seq that the reference log holds again, or that a restore tail could have brought back, is not removed: a plain rollback is repaired. An acknowledgement **comes from** a removed relay state when the relay gave it at a seq of that state, on that branch or before that rollback. For these two faults the promise is that an entry is **never lost silently**: a relay that forks, or that rolls back under entries another device then overwrites, can lose entries, and SYNC.md §4.8 answers it with an alarm with evidence and a re-home that lists what it does not carry over (§28.3), not with the entries.

1. **Convergence.** At the end, every device that is `Live`, joined, current and unhalted, with no stop, has the same `B` and the same state hash as the oracle at that seq.
2. **Acknowledged entries.** For every `entry_id` that a relay acknowledged (`new` or `dup`) to a device, at the end one of these holds:
   - the reference log holds it exactly once with class `Accept` (applied, or refused by `apply`), after any number of re-sealable refusals, and every device whose `B` is past that seq has applied it exactly once;
   - the reference log holds it once, finally refused, and its device reported the refusal: with a refusal record, with the `PairFailed` of its pairing, or with the end of its operation; as for a pairing's `DeviceAdded` that a rotation made stale (§28.4 step 4), and for a Skip whose target another device skipped first (§17.1);
   - it was never lost silently: every acknowledgement of it came from a removed relay state, and a device that holds its frame raised an alarm of §21.1 with evidence;
   - its device was **retired** while cut off, a known limit (SYNC.md §11, the owner's decision of 2026-11-25): the user re-paired that device's machine as a new device (§24.5, after `ReplaceReady` of §28.5 step 3), so the old identity runs no more, and before that the device had not met what became of the entry: every acknowledgement of it came from a removed relay state and the device met no relay state of the reference log after the fault that removed it, or the reference log holds it finally refused at a seq the device's log never reached.
3. **Queued edits.** For every `Notify(Enqueued)` with an `entry_id`: at the end, either the reference log holds that `entry_id` exactly once with class `Accept`, or the record is still in the outbox of a device that is halted, stopped or revoked, or it was finally refused with a refusal record on its device, or it was never lost silently, or its device was retired while cut off, as invariant 2 states.
4. **Tampering.** No device's confirmed state at any moment differs from the oracle's at the same `B` (on the branch it followed), except, from a Skip's target `t` up to the Skip at `s` (`t ≤ B < s`), the state of a device that never applied `t` (it crash-halted on `t`, or the Skip was committed to its `M_K` before `t` reached its board, §17.3 step 1): that state is compared with the reference log replayed with `t` left out, which the simulator computes from the oracle's classes without running the engine under test (ENG-SKIP-002, ENG-HALT-004); and every device that received a welcome or page contradicting its own log, or a page from a rolled-back or forked relay state that it would otherwise have applied, raised an alarm of §21.1 with evidence before applying any entry of it; every injected frame raised `SkippedFrame`, except one above a `Fence` that the relay showing it sequenced, which no device takes (§12.3 step 5).
5. **No deletion by the relay.** Every `TruncateLogBelow`, `DeleteBases`, outbox `Drop`, `forget_roots_below` and identity change a device emits is explained by one of: the retention rules of §7.13; an accepted purge rotation (§28.2); a `ConfirmRejoin` (§19.7); a join (§19.8); an entry that reached the board head; a cancelled hold; an accepted epoch rotation or refused own rotation.
6. **Settling.** At `T_f + 7,200,000`, every device is either `Live`, caught up, with no record older than `T_f + 600,000` in its outbox, or halted or stopped with a persisted reason; no request has been outstanding longer than 120,000 ms; and in the last 1,800,000 ms no device emitted an `Append` other than of beats and duty entries.

**The engine's behaviour change (ENG-RECV-007)**, already in this base (`engine.md` §12.2, §12.3
step 5, new text):

In `Reconciling` and `Live`, the engine emits `Read { after: L, limit: 256 }` whenever no read is outstanding and the latest relay head it knows (from a welcome or a page) is above `L`. A push whose first entry is `L + 1` is processed as a page; a push that starts above `L + 1` only raises the known relay head, which causes a read. A relay that sequenced a `Fence` this device accepted, which moves the board to another relay (`M_K`'s home is not the relay the session is on, and the session does not follow an address, §28.3 step 4), is fenced at that `Fence`'s seq `f` (`relay.md` §12.6): its known head counts as at most `f`, so the engine reads it no further, and §12.3 step 5 takes nothing above `f` from it. While a stop of §21 is in force the engine reads nothing and sends `Hello` every 900,000 ms of `mono_ms`, so it can see the relay recover.

5. **Contiguity.** From a relay fenced at `f` (§12.2), entries above `f` are left out, without evidence: they were not accepted from a device (§28.3 stage 3); within a page, the dry run ends after an accepted `Fence` that fences the relay the page came from, and the cut is the entry after it. Ignore entries with seq at or below `L` whose frame equals the log's (a repeated push); an entry at or below `L` with another frame is evidence `fork`. The first remaining entry must have seq `L + 1` and each next one the seq after; otherwise evidence `withheld` (entries above the floor are contiguous on an honest relay, `relay.md` §10.5).

- **ENG-RECV-007** (engine) The engine MUST NOT take into its log an entry above the seq of an accepted `Fence` from the relay that sequenced that `Fence` when the `Fence` moves the board to another relay, and MUST NOT read that relay beyond that seq.

Because of it, the oracle as built today stops at the first accepted `Fence` that moves the board
(the relay it reads is "fenced" there in the engine's eyes) and never reaches the reference log's
final head: on this base, `--test regressions` fails with "the oracle never reached B = …" for
every regression file whose run moves the board. That is the first thing to fix.

## What to do

1. **The oracle follows every move.** When the oracle's engine has accepted a `Fence` that
   awaits a move code (its status shows `move_pending`), the oracle's user types the code
   (`UserAction::MoveCode` with `Sealer::move_code(f, H(Fence frame))`, as a device's user does
   in §28.3 "On every other device" item 2), and the oracle's session goes on with the same rows
   of the reference log (answer the `Connect` the engine then emits as the oracle answers its
   first one). The oracle must reach the reference log's final head in every run, with a move or
   without; its states must stay what they are on a run without a move (a test). Everything else
   about the oracle stays as it is.
2. **A device retired while cut off (invariants 2 and 3).** A device is **retired** when the
   simulated user re-paired its machine as a new device (the user's `unsent` export and re-pair
   that follow `ReplaceReady`, §24.5); its old identity runs no more. An entry of such a device
   passes invariant 2 by the new last bullet exactly when, before the retirement, the device had
   not met what became of the entry:
   - every acknowledgement of it came from a removed relay state, and the device met no relay
     state of the reference log after the fault that removed it (no welcome, page or answer
     from the reference relay, or from the branch the oracle names, after the fault and before
     the retirement); or
   - the reference log holds it finally refused, at a seq the device's log never reached before
     the retirement.
   Nothing wider: a retired device that did meet the reference relay after the fault is held to
   the other bullets; so is one whose log reached the refusal's seq; a device that is not
   retired is never excused by this bullet. Invariant 3 accepts what invariant 2 accepts this
   way. The `gate` seeds that fail today only for this reason (on the lead's engine of
   2026-11-25): 104, 128, 1019, 2041, 2111, 2124, 2155, 2240, 2245, 2277, 2295.
3. **The dropped seq above the final head (removed relay state).** After a rollback, a dropped
   seq at which the reference log holds none, above the reference log's final head, is removed
   when a lower seq that the same rollback dropped holds a different entry in the reference log.
   Nothing wider: a dropped seq above the final head with no such lower seq is held to the
   existing rules (a restore tail could have brought it back, or a made-up frame blocked it).
   `gate` 2272 fails today only for this reason.
4. **Invariant 4, injected frames.** An injected frame above a `Fence` that the relay showing it
   had sequenced (the relay that `Fence` fenced) is taken by no device (ENG-RECV-007), so it is
   not required to raise `SkippedFrame`. Every other injected frame still is, and a device that
   took such a frame into its log would still be a violation (that would be the engine breaking
   ENG-RECV-007): check that no device's log holds it.
5. **Tests.** For each rule, hand-built tests that fail when the rule is removed or widened, as
   D6 did (mutations: remove each rule in turn and see a named test fail; report the list). For
   1: a reference log with one and with two moves (first case of stage 2: the old relay
   sequenced the `Fence`) that the oracle replays to its final head. For 2: each condition
   (retired or not; met the reference relay after the fault or not; log reached the refusal's
   seq or not). For 3: with and without the overwritten lower seq. For 4: the frame above the
   `Fence` taken by no device passes, taken by a device fails.
6. **Run the gate** as D6 did, in release (`TD_SIM_ALL=1`): `gate` at bases 100 (50 seeds), 1000
   (100) and 2000 (300); every named configuration at base 100 with 12 seeds, and `move` and
   `inject` with 60; `--test regressions` with every file in `tests/regress/` (all must pass on
   this base once item 1 is done; you change none of them). Report every seed that still fails,
   with one cause each and the first bad trace line, as D6 did: an engine fault (what the engine
   did against which clause), a check fault, or a spec question.

## Files you may change

`crates/taskdeck-sync/src/sim.rs` (not the splitmix and relay lines), files under
`crates/taskdeck-sync/src/sim/` except `splitmix.rs`, `relay.rs` and `relay/`,
`crates/taskdeck-sync/tests/simulator.rs`, `crates/taskdeck-sync/tests/regressions.rs`, and
`docs/sync/status/engine/api-simulator.md` (keep it short: say what changed, cut what no longer
holds). Not `crates/taskdeck-sync/tests/regress/**`; do not read `crates/taskdeck-sync/src/engine/**`.

## Done

The gate of this task (unit tests, regressions, relay conformance, clippy with `-D warnings`, the
wasm32 check with `--features sim`) passes in your worktree with your own `CARGO_TARGET_DIR`
(clean the crate there before the last run), your work is committed on your branch, and your
report lists what you built, the mutations, the gate runs and every failing seed with its cause.
