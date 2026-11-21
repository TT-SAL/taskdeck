# Brief: `docs/sync/spec/core.md` (owning stage: `core`)

Scope (SYNC_PLAN.md §6.1 item 3): "board data, intents, log commands v1 (including board settings for the
palette and the home place), `Ctx`, the origin's compile step, ids and stable block ids, conflict rules,
rules versions, canonical encoding and state hash, read queries that take the device's zone (SYNC.md
§4.4–§4.6)". The `core` stage (§6.2) copies today's pure logic into `crates/taskdeck-core` and builds
`BoardData`, `apply`, intents and the compile step, ids, conflict rules, rules versions, canonical encoding,
read queries, snapshot builders, `reminders()` and the legacy import. Its gate: wasm32 purity, determinism
and rebase property tests under two `TZ` values, transcript and snapshot-JSON equality against `fixtures/`,
golden logs, three budgets.

## Sources
- The code digest: `docs/sync/status/foundation/digests/core-digest.md`
  (facts about today's code with file:line). Check the code yourself where the digest is thin.
- SYNC.md §3, §4.4, §4.5, §4.6, §4.7 (view, retention), §9 (import-check), §12; SYNC_PLAN.md §2, §3.3
  ("Keep read-side names"), §6.2, §6.9 (what the desktop needs), §6.10 (what the phone needs).
- docs/sync/research/R1 in full; V2 findings 1, 2, 3, 4, 5, 16, 17, 19, 20 and its "DETERMINISM AUDIT" and
  scenarios S1, S2, S7, S15.
- `seal.md` if present (the `cmd` payload carries one of your log commands; `device_no` comes from membership).

## Must contain
1. **`BoardData`**: the complete replicated state, field by field: items (tasks, events, routines) with their
   blocks and stable block ids; archive rows and opaque unreadable lines; notes; subscriptions; board
   settings (palette, home place rounded to 0.01°); the active `rules` version; per-device high-water marks
   for item ids and block ids; the last-change stamp (seq, device_no) of every item and block; anything else
   `apply` reads. Say for each field whether today's data has it.
2. **Intents**: what a UI hands the origin (may carry naive local times). One row per intent, covering every
   verb the phone page sends, everything the desktop does, and the new settings.
3. **Log commands v1**: every command, each with its JSON schema in canonical form, strict parsing rules
   (RFC 3339 with `Z` only; unknown fields refused), what `apply` checks (existence, ranges, lengths: thin
   apply, V2-1), what it changes, the change set it returns, its refusal codes, and its benign-repeat cases
   (V2-16). Include at least: creating commands that carry their ids; `MoveBlocks{[(id, sid, start, minutes)]}`
   and `RemoveBlocks{sids seen}`; floating `CreateRoutine` and `MoveRoutine`; `SetEventSpan{start, minutes}`;
   `SetNotes{text, base}`; `SetPalette`; the home-place setting; subscription commands; `Restore` with a
   fallback id; delete, forget and forget-archived.
4. **`Ctx{seq, device_no, at, based_on}`** and the complete list of what `apply` may read. Nothing ambient:
   no clock, zone, I/O, randomness, floating point, hash-map iteration or `Vec`-order dependence.
5. **The compile step**, per intent: defaults (names, colours, seeds), the trimming rule (exactly which
   characters), zone conversion and clamping in the origin's zone, id minting from the marks, Reflow compiled
   to explicit block moves and Unplan to explicit removals, what the origin passes in (its zone, its clock
   reading, its overlay anchors, its `device_no`, its view's `based_on`).
6. **Ids**: item ids `device_no * 2^40 + n`; legacy ids below 2^40; marks and the rule that newly minted
   numbers lie above them; block ids; `Restore` reusing the archived id if free, else the fallback.
7. **Conflict rules**: last writer wins in seq order; stamps; exactly when an overwrite notice is produced
   and for whom; `MoveBlocks` skipping blocks changed or gone since `based_on`; `RemoveBlocks`; notes with a
   base (define the base); benign repeats.
8. **Rules versions**: what rules version 1 is; how `apply` is keyed by `BoardData.rules`; how a version
   changes (the entry point the engine calls when a `RulesUpgrade` control item is applied); golden logs.
9. **Canonical encoding v1** and the canonical board bytes that feed the state hash (ruling S7): sorted
   keys, omitted defaults, the one timestamp form (UTC with nanoseconds), integers only, total orders for
   every collection (archive rows by archived_at, id, bytes), unreadable lines in base64, exactly what is
   hashed. Other specs use this encoding for their own JSON; write it so they can.
10. **Read queries**: the names kept from today (`ranked_ids`, `behind_minutes`, the snapshot JSON, and the
    rest of what the UI and the phone page ask), each with its inputs, the caller's time zone as a parameter
    (say how a zone is passed without a new dependency), the archive bucketed by day, and `reminders()`.
    The snapshot JSON equals today's, field for field, except for changes you list one by one.
11. **Legacy import**: inputs (today's files, and the older `main` build's), what is normalised (only here
    and at genesis, V2-19), the checks of SYNC.md §9 step 4, the report (counts, ids and error kinds only,
    never text from the board), the genesis `BoardData`.
12. Error catalogue, Limits (every constant, with today's value), Test vectors (golden-log file format;
    canonical-encoding vectors; how recorded transcripts map old commands to new ones), Interfaces, Public
    API (Rust signatures for `taskdeck-core`), Open questions.

## Gaps to close (decide, and say so)
- A table of today's commands against the new ones: same meaning, changed, or gone. It decides which recorded
  transcripts the `core` stage must replay unchanged.
- The shapes of the palette and home-place settings, and their commands.
- The notes base, and what a refused `SetNotes` returns so the origin can keep a draft and show a merge.
- How block ids are minted and how the phone page's `session` field carries them.
- Where notices and refusals live (per device, outside `BoardData`) and their record shapes.
- The reminder record, and the defaults it uses.
- The exact textual form of instants in JSON, and of ids (numbers, never strings).
