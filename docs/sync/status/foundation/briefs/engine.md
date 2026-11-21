# Brief: `docs/sync/spec/engine.md` (owning stage: `engine`)

Scope (SYNC_PLAN.md §6.1 item 3): "the replica state machine: confirmed and pending state, outbox, send and
reconcile, receipt checks, refusals and halts, checkpoints, snapshots, compaction, integrity evidence, beats,
rules upgrades, Skip, timed windows such as the recovery-key change, duties with no always-on device
(SYNC.md §4.3, §4.7, §4.8)". The `engine` stage (§6.5) builds the replica engine with no I/O of its own,
generic over the state machine and the sealer: it consumes events (user intent, relay message, timer,
storage result) and emits effects (persist, send, notify). Then a seeded, deterministic simulator with a
model relay and faults. Its gate: 1,000 seeds per CI run holding six invariants; the crate compiles for
wasm32 as proof it does no I/O. `device` (§6.7) wires the real core and seal into it.

## Sources
SYNC.md §4 (all), §5.3, §5.4, §5.5, §6.4, §6.6, §6.7, §9 and "Rejected or modified verifier findings".
docs/sync/research/V2 in full, including the fifteen scenarios S1–S15 at the end (each becomes an end-to-end
test in `device`; your spec must give each a defined outcome). V1 findings 1, 10, 11, 16. V3 findings 2, 6,
14. SYNC_PLAN.md §2 (no always-on device; no hub), §6.5, §6.7, §6.9, §6.10.
The drafts in `docs/sync/spec/`: `seal.md` (frames, kinds, control items, membership machine, receipt-check
classes, chain), `core.md` (`apply`, `Ctx`, canonical bytes, rules), `relay.md` (routes, socket messages,
errors) when present.

## Must contain
1. **The model.** `Engine<SM, S>` generic over a state machine and a sealer (decision D6: both traits are
   defined by the engine crate; the toy state machine and the fake sealer live there too). Sans-IO and
   deterministic: time, randomness, storage results and network messages arrive as events; everything the
   engine wants done leaves as effects. Give the complete event and effect enumerations with their fields,
   and the rule for what the runtime must do with each effect and in what order.
2. **The two traits**, as exact Rust signatures with the contract of every method: the state machine
   (apply under a `Ctx`, trial apply, canonical bytes, load, hash, rules version and its upgrade entry
   point) and the sealer (seal an entry, verify and open a frame at a log position with the receipt-check
   class as the result, apply control items to membership, epoch state, chain step, canonical membership
   bytes, own identity). Then the toy state machine and the fake sealer, specified exactly enough that two
   people write the same ones.
3. **Persistent records the engine needs** (abstractly: `device.md` owns files): the log with seq and
   chain value, the checkpoint, weekly bases, the hash table at seq ≡ 0 mod 64, the outbox records
   `{entry_id, at, based_on, command, ctr?, frame?}`, page markers, the notice and refusal lists, the head.
   For each persist effect, what must be durable before which later effect.
4. **Enqueue** (SYNC.md §4.3 rule 1): compile is the origin's (core); trial apply so a command that panics
   locally is never queued; a random 128-bit `entry_id`; durable before the call returns; the naming hold
   (V2-17: a Create is held only while it is the outbox's last entry; Create then Forget never leaves;
   Create then Rename leaves as one Create).
5. **Send** (rule 2): counters assigned at seal time; seal, make the frame durable, transmit; retries
   resend identical bytes; batches of at most 16.
6. **Reconcile on hello** (rule 3), **409 handling** (rule 4), **echo check** (rule 5), one identity per
   folder and "no relay answer ever deletes local data or keys" (rule 6, V2-21).
7. **Receive**: pages of at most 256 entries or 1 MiB made durable before they are applied; the receipt
   checks and their classes (from `seal.md`) mapped to what the replica does: skip, refuse everywhere,
   ordinary refusal reported to the author, halt; rebase of pending entries per batch; marking doomed
   entries "will be refused"; the persistent per-device notice and refusal lists.
8. **Poison pills** (V2-14): pages applied under `catch_unwind` with one marker per page; after a hard
   abort, replay entry by entry; halt only when the same entry fails on two consecutive starts, naming its
   author and offering Skip.
9. **Halts**: exactly the three of SYNC.md §4.2; what a halted device shows (confirmed state plus its own
   unapplied entries, "frozen since"), that it holds new edits, takes no duty, and reads ahead for Skip
   and control entries so revocations still land (V1-11, V2-6).
10. **Checkpoints, bases, hashes**: every 200 entries, every 10 minutes and at exit; weekly bases kept 400
    days; the state hash at every seq ≡ 0 mod 64 (ruling S7: you own the composition).
11. **Snapshots** (V1-10, V2-7): taken at S ≡ 0 mod 64; who takes one and when with no always-on device
    (ruling S8; state the rule so two devices rarely duplicate work and it never matters if they do);
    acknowledgement or mismatch in the next beat; which snapshots bootstrap and compaction may use;
    bootstrap from the pairing bundle and for a device away more than 365 days.
12. **Compaction duty**: when a device asks the relay to compact, what it must have verified first, what
    it names (control entries are kept, V2-15).
13. **Integrity evidence** (SYNC.md §4.8, V2-9, V2-10): the evidence kinds; last seen is not an alarm; the
    response (stop sending and applying, keep editing locally, show the evidence); restore tail by the
    most advanced device; when a re-home is the answer.
14. **Determinism mismatch**: only the minority halts; adoption only of a device chosen by the fingerprint
    on its own screen (V1-10).
15. **Beats**: when (daily while awake), contents, what receivers derive (last seen, supported versions).
16. **Epoch transitions**: applying a rotation; re-sealing refused entries together with all later ones,
    in order (V2-8); stale-epoch entries; a revoked device that reappears ("re-pair and resubmit").
17. **Rules upgrades** (V2-1, V2-6, V3-2): writers never emit what the active rules forbid; what the
    engine tells the UI about the blocker; the unknown-RulesUpgrade halt.
18. **Timed windows** (ruling S6): the 72-hour recovery-key change (propose, banner, confirm by a
    different member, cancel), the 30-day windows, pairing tickets; which are decided from the log and
    which only prompt a device to act; behaviour when devices are asleep for the whole window.
19. **The clock-skew guard** (V2-20 as modified): over 5 minutes against `relay_now` warns; over an hour
    blocks composing until the user confirms the clock; relay time never stamps anything.
20. **Operations**: restore to a point (`ResetTo`, recomputed by every device by local replay, only to seqs
    every active device still holds), purge history, move board (Fence, the move code, following a move),
    device replacement, reset trust.
21. **Offline**: unlimited, with a warning at 10,000 queued; the banner texts' data, not their wording.
22. **The simulator**: seeded and deterministic; the model relay follows `relay.md`; the fault list of
    SYNC_PLAN.md §6.5; the six invariants stated as checkable properties; how a failing seed becomes a
    regression test; the trace format.
23. Error catalogue, Limits, Test vectors (simulator traces; receipt-check tables), Interfaces, Public API
    (Rust signatures of `taskdeck-sync`), Open questions.

## Gaps to close (decide, and say so)
- Exactly how the pending view is computed and when "doomed" is known.
- What "current" and "unhalted" mean for taking a duty, and the tie-break that keeps two devices from both
  writing a snapshot in the common case.
- What a week of phone-only use looks like (ruling S8): which duties the phone takes, which wait.
- How the engine learns `relay_now`, and what "the user confirmed the clock" resets.
- The data behind every banner and notice the desktop and the phone must show (so `desktop` and `android`
  can build the screens from your Public API).
- The fifteen V2 scenarios: a table with the expected outcome of each under your rules.
