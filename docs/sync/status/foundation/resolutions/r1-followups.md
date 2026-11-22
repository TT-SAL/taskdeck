# Follow-ups from applying round 1 (for the next spec pass)

Things the fixers and checkers reported that no round-1 change covers. The next spec pass (the two-readers
round, or a small fix pass before it) writes them.

1. **device.md: the recovery tool's reinstatement steps** (lead ruling L22): §15.8 and `RecoveryLink` say
   nothing about how the tool computes the missed-root wraps, sends `reinstate` and appends the reinstating
   entry.
2. **engine.md: `TruncateLogBelow` now takes `(seq, floor_chain)`** (R1-DEVICE-ENGINE-11), but several places
   still write the one-argument form: §28.2 "On every device" step 2, §19.7 step 4, the D8 row, and possibly
   others. Every use must give both arguments.
3. **device.md: the order of `history.tdk` and `bootstrap.tdk` when a device joins.** §15.7 (as R1-SEAL-RELAY-28
   wrote it) says the store writes `history.tdk` from the bootstrap snapshot before it persists the bootstrap;
   the list "Persisting the join" in §18.3 still names `bootstrap.tdk` first. One order must hold in both.
4. **relay.md: references to seal clauses whose relay duty moved.** SEAL-MEM-004 lost its relay owner and
   SEAL-PAIR-001's relay part moved to SEAL-PAIR-015 (R1-SEAL-RELAY-5 and -13). Any relay.md text that still
   cites the old ids as relay duties must cite what now states the duty.
5. Settled by the lead, no change needed: DEV-CREATE-005 stands at the end of device.md §18.1 (the change named
   the section and a clause of another section); the sentence R1-SEAL-DEVICE-14 added to seal.md §20.2 stands
   after the exception it builds on.

## For the pass after two-readers round 1

6. **engine.md §28.2 step 2** sets `log_floor = p` even for a device whose `log_floor` is already above `p`
   (lead ruling L27): it must take the larger of the two and keep `floor_chain` in that case.
7. **seal.md §24** needs the test-vector family that the new "ephemeral key" paragraph of §7 refers to: the
   32 bytes drawn for each wrap (TR1-TRUST-2's SEAL-WRAP-004).
8. **`RestoredCopy` and `BasedOnUnchecked`** are shown "until acknowledged" like the continuity warning, but
   have no action, stored flag, facade call or bridge call (lead ruling T4 gave those to the continuity
   warning only).
9. **bridge.md §11 and §21: a default backend** (lead ruling S32): `taskdeck-android` declares
   `default = ["backend-device"]`; building with both backends or with neither stays a `compile_error!`;
   the `echo` flavour builds with `--no-default-features --features backend-echo` and the `full` flavour
   with the default features. Every place that describes how a flavour passes its feature must say so.
10. **relay.md §8.2, the row "token, role 1, on `login-nonce` and `login`"**: its "Valid at the object when"
    cell lists no state condition, while its "Else" cell ("as above, but the generation is not checked") and
    the paragraph and clause RELAY-OBJ-010 that TR2-TRUST-2 added answer `403 relay/revoked` for a row with
    `state = 2` on those routes too. The cell must read: a `devices` row with `dev` and `role = 1` exists;
    `state = 1`.
11. **README.md §"Glossary", the row `notice`** still quotes "changed on Desk since you last synced" as an
    example of the words; since TR2-EDIT-3 the words are the interfaces' (`bridge.md` A-04, `device.md`
    §17.7). Reword the cell to: A persistent per-device message that is not a refusal, such as an overwrite
    notice.
12. **device.md DEV-GEN-011 and DEV-NET-002 disagree about `webpki-roots`** (found by the workspace
    implementer): DEV-NET-002 has the device verify "with `webpki-roots` when the caller passed
    `TrustRoots::Bundled`", and §10.1 names version 1.0.12, but DEV-GEN-011's exact list of non-development
    dependencies leaves the crate out. SYNC_PLAN.md §3.4 approves `webpki-roots` "if `android-shell` chooses
    bundled roots". DEV-GEN-011 must name it with that condition.
13. **device.md §15.8 and the crate table of §4: `taskdeck-recover` depends on `taskdeck-device`** for the
    online steps (`recovery_online::RecoveryLink`); any table or clause that lists the tool's dependencies
    as backup, seal and core only must say so (lead ruling 3 of `briefs/workspace.md`).

## State (2026-11-22 10:15 UTC)

Items 6 to 13 are written as exact changes in `r3-followups-changes.md` (TR3-FOLLOW-1 to -10) and applied:
seal 1, relay 1, engine 2, device 3, bridge 2, README 1. The specs hold 941 clauses. The lead's rulings on
the three open points:

- **T14. `BasedOnUnchecked` counts an entry once**: when it is first applied to the board; a replay of the
  log from a base at `Start` does not count it again. (One sentence in `engine.md` §13.1, written by the
  lead.)
- **T15. Several restored-copy catch-ups before an acknowledgement add up** (saturating), as written.
- **T16. `TrustRoots::Bundled` has nothing to link until `android-shell` chooses bundled roots**; the stage
  that makes the choice adds `webpki-roots` and implements `Bundled` in the same change. No interim refusal.

Nothing is open in this file.
