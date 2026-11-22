# Follow-ups from applying round 1 (for the next spec pass)

Things the fixers and checkers reported that no round-1 change covers. The next spec pass (the two-readers
round, or a small fix pass before it) writes them.

1. **device.md: the recovery tool's reinstatement steps** (lead ruling L22): §15.8 and `RecoveryLink` say
   nothing about how the tool computes the missed-root wraps, sends `reinstate` and appends the reinstating
   entry.
2. **engine.md: `TruncateLogBelow` now takes `(seq, floor_chain)`** (R1-DEVICE-ENGINE-11), but several places
   still write the one-argument form: §28.2 "On every device" step 2, §19.7 step 4, the D8 row, and possibly
   others. Every use must give both arguments.
