# Spec follow-ups from the requirement map

The requirement map (SYNC.md §3 to §9 against the 942 clauses) found requirements that no clause states and
no stage owns by the plan. The ones below get a clause. For each, read the SYNC.md passage it names and the
spec section, write the clause for the owning stage, and give the map row for it.

1. **SYNC.md §4.5, display order.** Read queries never reorder `board.items`; display order belongs to the
   caller, and the calendar summary breaks ties on (start, id, sid). core.md §14.2 (stage `core`).
2. **SYNC.md §5.4, "reset trust".** Starting a new board from a trusted device's state: device.md §18.6 and
   engine.md §28.7 describe it without a clause.
3. **SYNC.md §5.4, when keys rotate.** On a revocation and on a new recovery key, never on a schedule:
   engine.md §28 states the revocation case only (ENG-OPS-002).
4. **SYNC.md §5.6, the Android Keystore key.** SYNC.md calls `setUnlockedDeviceRequired` optional; bridge.md
   §12.5 does not say whether it is set. Decide the conservative way for a runtime that must open its store
   while the phone is locked (background sync, reminders), and state the reason.
5. **SYNC.md §5.6, capture exclusion on Android.** The screens that show a pairing code or a seed set
   `FLAG_SECURE`; bridge.md says so only in prose about one activity.
6. **SYNC.md §6.2, the import budget.** A 5,000-command import stays under 20% of every daily limit of the
   relay's free tier: relay.md §16.2, next to RELAY-ROWS-002, as a check of the conformance suite.
7. **SYNC.md §6.4, feeds down.** When an ICS host or Open-Meteo cannot be reached, the device keeps the last
   overlay and weather and publishes their age for display: device.md §13 (stage `device`), with the line the
   apps show if the specs fix such lines elsewhere.
8. **SYNC.md §6.7, re-home.** The moving device lists what it did not carry over to the new relay (V2-9):
   engine.md §28.3.
9. **SYNC.md §9 step 6, the first board.** A board created from a legacy folder takes its palette and home
   place from `userconfig.toml`: core.md §15.3 or device.md §18.2, whichever owns the import of that file.
