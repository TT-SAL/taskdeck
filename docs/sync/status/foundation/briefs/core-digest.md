# Brief: code digest for `core.md` (a fact sheet, not a spec)

Write ONE file: `docs/sync/status/foundation/digests/core-digest.md`.
Change nothing in the repository. No git state changes. Do not compile.

Purpose: the drafter of `docs/sync/spec/core.md` must specify a pure, deterministic board core that keeps
today's behaviour wherever the design does not change it. They will not have time to read 600 KB of Rust.
You read today's code and write down the facts, exactly, each with `file:line`. Facts only: no design, no
opinions, no summary of SYNC.md. Quote constants, field names, serde attributes and wire names verbatim.
Read the code itself (src/board.rs, tasks.rs, planner.rs, archive.rs, phone.rs (snapshot side), ics.rs,
subscriptions.rs (types), utilities.rs, weather.rs (report types), initialization.rs (config keys),
color.rs (palette)), in ranges; use Grep to find things. DOCUMENTATION.md §6, §7, §16–§18, §21.3–§21.4
explain intent; the code wins where they differ, and say where they differ.

Sections the digest must have:
1. **Data model.** Every persisted struct and enum (`Active`, sessions/blocks, `Recurrence`, `Archived`,
   notes, subscriptions, anything else a board holds): each field, its type, serde attributes, default,
   meaning; the files they are stored in (names, formats, one example record each, taken from tests or
   written by you from the serde shapes); how unreadable archive lines are kept; what "legacy" rows and
   fields exist and how today's code migrates them (`migrate_legacy_plans`, `assign_missing_ids`,
   `adopt_temporaries`, legacy-row tests in archive.rs).
2. **Commands.** Every `Command` variant: wire `op` name, fields with types and serde attributes, whether
   it is a query or a mutation, `Reply` shape, which `BoardError` it can return. Then `Board::apply` arm by
   arm: every validation (with the constant's name and value), every default it invents (names, colours,
   seeds), how it assigns ids, exactly what it mutates, and every read of the clock, the local zone, the
   overlay, or disk. List every constant that limits anything (`NAME_MAX_CHARS`, `NOTES_MAX_CHARS`,
   `SUBSCRIPTIONS_MAX`, block minimum and snap minutes, severity and horizon tables, the year window, ...).
3. **The 17 verbs the phone page sends** (grep `src/phone.html` for the ops it posts) and how each maps to
   a `Command`. Also what the desktop UI sends that the page does not (grep `Command::` in src/ui.rs).
4. **Read side.** `ranked_ids`, `behind_minutes`, `importance_score` and the daily tie-break shuffle,
   `phone::snapshot` and everything it calls (`build_day`, `build_tray`, `item_detail`, `done_rows`,
   `with_subscribed`, `answer_query`): signatures, inputs that are process globals today, and the exact JSON
   shape returned (every struct, field name, type, serde attribute), so the new core can return the same
   JSON. Note every place the local zone or `now` is read.
5. **Planner.** The pure planning logic: reflow (inputs, ordering, tie-breaks), clamp and snap, block
   placement, `resolve_on_day`, budget and estimate arithmetic; where floats are used.
6. **Archive.** Row shape and key, ordering rules (including among equal timestamps), verdicts and
   summaries, the ledger, restore and forget, ghosts.
7. **ICS, overlay, weather types.** Parser limits and entry points; overlay types and digest; weather
   report types the snapshot uses.
8. **Settings that the design makes board settings.** Where the palette (six colours?) and the home place
   (coordinates) live today (`userconfig.toml`, `colorschemes.json`), their exact shapes and defaults.
9. **Tests worth porting.** The board, planner, archive and tasks tests by name with one line each on
   what they pin down.

Be complete rather than brief: this is a reference. 600 to 1,200 lines is fine. End with a list of
anything you could not determine.
Return a short text summary (what the digest covers, what is missing).
