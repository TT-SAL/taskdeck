# Lead rulings on the round-1 resolutions ("For the lead" items)

Binding on the fixers. Where a ruling differs from the conservative reading a resolver wrote into its
changes, the ruling wins and the "lead changes" file (`r1-lead-changes.md`) gives the exact text.

## L1. Ruling S24 gets a channel: the `reinstate` admin operation carries the missed roots

(R1-SEAL-RELAY "For the lead" 1; R1-SEAL-ENGINE 1; R1-CORE-ENGINE 2.)

A reinstated device cannot open the reinstating entry, so the wraps cannot travel inside it. Decision:
design (a), with the first epoch explicit.

- The recovery-signed `reinstate` admin operation has `args = dev[16] ‖ u32 first_epoch ‖ u16 n ‖ n × wrap[96]`.
  The wraps are of the roots of epochs `first_epoch` to `first_epoch + n − 1`, in order, each to the
  reinstated device's HPKE key. `first_epoch` is the epoch that the rotation in the revoking entry set up,
  or the epoch in force at `purge_seq` if that is later. The last wrapped epoch is `E`, the epoch the
  reinstating entry is sealed under. The reinstating entry's own new epoch `E + 1` is not included: its
  rotation already wraps to the reinstated device like to every other recipient.
- `1 ≤ n ≤ 256`. If more than 256 epochs lie between, the recovery tool does not offer reinstatement for
  that device; the owner restores to a new board (ruling S14's remedy).
- The relay does not interpret the wraps. It stores them on the target's `devices` row, replaces them on a
  later `reinstate` of the same device, clears them when the device is revoked again, and returns them in
  every `login` answer of that device while they are stored.
- The device verifies each root against the commitment of the rotation that set that epoch, oldest first,
  reading the log in order (each rotation entry is sealed under the epoch before it). A wrap that does not
  open, or a root that does not match its commitment, is the commitment halt for that device.
- The engine rule the core–engine resolver describes, with the id ENG-EPOCH-005 (ENG-EPOCH-004 is the revoked
  device's hourly read-only reconnect, R1-ENGINE-RELAY, see L8): a revoked device keeps its log and
  pauses classification at the first entry that returns `seal/missing_root`; when the sealer learns the
  missed roots it clears the revoked state, then classifies and applies in seq order from the paused seq
  before it seals anything. A reinstated device does not pair again.
- A device revoked before a purge cannot replay across it: it bootstraps from a snapshot at or after
  `purge_seq`, as a device away more than 365 days does.
- The status file's S24 is reworded to this.

## L2. The seeded tail may be longer than 30 days

(R1-SEAL-RELAY "For the lead" 2.) Accepted as the resolver wrote it: the seed starts early enough to keep
the Skip window of ruling S15 on the new relay. It is a deliberate difference from SYNC.md §6.7's "up to
30 days of tail". The spec that owns the seed records it under "Open questions" with the default taken
(README §1), and the lead lists it for the owner in the status file. It gets no row in `spec/CHANGES.md`:
that file records changes after `foundation` merges and what the two-readers test leaves open.

## L3. A far-future `at_ms` is refused by seal

(R1-CORE-ENGINE "For the lead" 1.) Option (a), as written: `seal.md` §10.1 narrows `at_ms` to
0 to 253,402,300,799,999; an entry outside it is refused at classification on every device with
`seal/bad_plaintext`.

## L4. A snapshot body over 64 MiB is refused with a named code

(R1-SEAL-RELAY "For the lead" 4; R1-CORE-ENGINE 3.) Accepted: the sealer fills and keeps the control
history itself, the engine's snapshot material does not change, and `seal_snapshot` refuses a body over
67,108,864 bytes with a named error code instead of leaving the case undefined. The board cap of S18 stays.

## L5. An orphan `revoke` also invalidates the backup-read tokens

(R1-SEAL-RELAY "For the lead" 5.) Accepted as written.

## L6. A move started under an evidence stop fences the old relay without a frame

(R1-ENGINE-RELAY "For the lead" 1.) Option (a), as written: the mover fences the old relay with an empty
`extra`, seeds the new relay, and appends the `Fence` there; the others meet `relay/fenced` without a
`Fence` in the old log and follow with the relay address and the move code.

## L7. `compact` names the snapshot blob

(R1-ENGINE-RELAY "For the lead" 2.) Accepted as written: `compact`'s `args` gain `blob_id[16]`, in
seal.md, relay.md and engine.md alike.

## L8. Clause ids for the two epoch rules

(R1-ENGINE-RELAY "For the lead" 3.) ENG-EPOCH-004 stays the revoked device's hourly read-only reconnect.
The S24 rule of L1 is ENG-EPOCH-005.

## L9. The engine's snapshot material carries no history

(R1-ENGINE-RELAY "For the lead" 4.) The earlier decision stands (L4): the sealer fills and keeps it.

## L10. The phone's own picture is written where the device–engine resolver put it

(R1-DEVICE-ENGINE "For the lead" 2.) Accepted: kind 19's row, layout and facade calls come from
R1-DEVICE-ENGINE-14; the device–bridge file adds only what bridge.md needs.

## L11. Adoption is an operation with a ticket

(R1-DEVICE-ENGINE "For the lead" 3.) Accepted as written.

## L12. A mode change reloads the phone page

(R1-DEVICE-BRIDGE "For the lead" 1.) Option (a), as written: one clause, no change to the forked page.

## L13. `forget_identity` of the active identity in `KeyLost` mode, and `identities()`

(R1-DEVICE-BRIDGE "For the lead" 2.) Accepted as written.

## L14. `set_look_dials` is a long call

(R1-DEVICE-BRIDGE "For the lead" 3.) Accepted as written, on the condition the resolver states: it adds
no thread, timer or redraw rule to the desktop's render loop (DOCUMENTATION.md §14 is off-limits). A
desktop caller uses the worker thread it already has for long calls.

## L15. Local-only mode refuses a folder over a cap of rules version 1

(R1-CORE-DEVICE "For the lead" 1.) Option (a), as written: one import path; the open fails with
`device/legacy_unreadable` and a report that names the ids. The lead adds it to the status file's list for
the owner, next to S18.

## L16. Seal's and the relay's halves of L1, as the last two resolvers wrote them

(R1-SEAL-DEVICE "For the lead" 1; R1-RELAY-DEVICE 1.) Confirmed: R1-SEAL-DEVICE-4 (the `reinstate` args,
`AdminOp::Reinstate { dev, first_epoch, wraps }`, the limit of 256, opening each wrap by SEAL-WRAP-002 with
the commitment from the device's own membership, oldest epoch first, a failure being the commitment halt)
and R1-RELAY-DEVICE-6 (the nullable `devices.wraps` column, the args cap, `1 ≤ n ≤ 256`, clearing on
`revoke`, the `login` answer field `wraps`, RELAY-REVOKE-005, RELAY-LOGIN-006). The lead's change file
does not reword them; it adds what is still missing and removes what L1 overrules:
- relay.md §1 lists ruling S24;
- engine.md: `Reinstate` leaves `AdminRequest` (the engine never sends it; reinstatement is the recovery
  flow's), so `Revoke { dev }, Reinstate { dev: Dev }` becomes `Revoke { dev: Dev }`; §24.4 and the clause
  ENG-EPOCH-005 as L1 says; the conservative "a reinstated device pairs again" text of R1-SEAL-RELAY-8 and
  R1-SEAL-ENGINE-13, and the open-question items they added, are removed or marked as ruled;
- device.md: the hand-over of R1-DEVICE-ENGINE-13 names the `login` answer's field `wraps`.

## L17. A host never carries `:443`

(R1-SEAL-DEVICE "For the lead" 2.) Option (a), as written.

## L18. A backup's `read_token` may be empty

(R1-SEAL-DEVICE "For the lead" 3.) Accepted as written.

## L19. Slow links

(R1-RELAY-DEVICE "For the lead" 2.) As written: the client treats `408 relay/slow` as transient, and the
64 KiB/s floor stays a stated client requirement. The relay's deadline constants do not change now: there
is no measurement yet. The owning specs record the residual under "Open questions", and the lead lists it
for the owner's rehearsal (SYNC.md §11 "Still to measure").
