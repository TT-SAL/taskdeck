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
