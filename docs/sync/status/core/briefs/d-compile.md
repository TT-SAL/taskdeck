# Task D, `d-compile`: intents and the compile step

Plan: SYNC_PLAN.md §6.2 item 3: "defaults, trimming, zone conversion, Reflow compiled to explicit block moves,
Unplan to explicit removals, floating routine commands, the palette and home-place settings". All policy
lives here; `apply` (task B) only checks and performs. Tasks A, B and C are in the working copy.

## Fill

- `src/intent.rs`: `WireTime`, `Intent` (one variant per row of §12.3), `parse_intent`.
- `src/compile.rs`: `CompileCtx`, `Compiled`, `Reply`, `compile`, `checked_url` (`trim` and `detab` are there).
- `src/read.rs`: `reflow_inputs` only (§13.4 steps 1 and 2); the other read queries are task E.

| Read | core.md lines |
|---|---|
| §12 intents: parsing today's wire shapes, the table of intents | 810–869 |
| §13.1–§13.3 inputs, what only the origin decides, the rules G1 to G10 | 871–905 |
| §13.4 the table: steps, command and reply per intent; `checked_url`; Reflow | 907–960 |
| §13.5 today's commands against the new ones (what may differ from today) | 962–989 |
| §7.2 minting; §11.2 last paragraphs (the op table of `view.rules`) | 366–373; 787–794 |
| §16 intent and compile errors: code, status and message of each | 1226–1263 |
| §18.2 rows `intents.json`, `compile.json` | 1340–1341 |
| §20 `intent`, `compile`, `read::reflow_inputs` | 1601–1615; 1754–1764 |

Today's code that this reproduces, under `/home/user/taskdeck/src/`: `board.rs` 111–280 (the `Command`
enum: the wire shapes), 392–419 (`wire::optional_local`, `parse_local_input`), 421–515 (the checks and the
error texts), 888–908 (`reflow_inputs`), 943–1400 (the arms: today's policy per command, in today's order),
1226–1250 (Reflow); `subscriptions.rs` 60–100 (limits, `DEFAULT_COLORS`), 156–180 (`checked_url`), 283–302
(`name_from_url`, already in `overlay`).

## Points the lead has settled

1. `parse_intent` reads today's shapes with `serde` derive on private wire structs (unknown fields ignored,
   today's defaults), after the 84,096-byte check. Every failure is `core/intent_json`, status 400. The wire
   times follow §12.2 steps 4 and 5 exactly; a naive time is carried as `WireTime::Naive` and resolved only
   in `compile` (CORE-INT-002). `session` is a sid (CORE-INT-003).
2. `compile` runs each row of §13.4 in the order written and returns the first failure with the code, the
   status and the message of §16, character for character (they are today's texts; the recorded transcripts
   compare them). Mint on a copy of the minter and store it back only on `Ok` (CORE-COMP-004).
3. Every check `apply` would make is made on `view` first (G6), so a command `compile` returns is accepted by
   `apply` on that view (CORE-COMP-008): prove it with a property test over random intents and boards.
4. `compile` may use `f64` for `set_home` only (`(x * 100.0).round()`, which rounds half away from zero).
5. Reflow follows the five steps under the table; ties are settled by ascending `(id, sid)` (§13.5).
6. `compile` never emits an op outside the op table of `view.rules` (CORE-RULES-004).
7. The error exception of CORE-GEN-010 covers only `core/intent_json`'s message, which may quote the field text.

## Tests and vectors to write

- `tests/vectors/intents.json` (every op of §12.3 in the page's and the desktop's JSON shapes, every wire-time
  form, the failures) and `tests/vectors/compile.json` (intent text, view board text, zone as §18.1 writes it,
  `now`, `device_no`, minter, overlay anchors → command text and reply, or code, status and message; the
  minter after). One case per row of §13.4 and per failure step of each row; zone cases with a `table` zone
  across a gap and a fold. Expected values from the spec's table, worked by hand.
- A test target `tests/compile.rs` that checks every case, plus the property tests: `parse_intent` and
  `compile` never panic (CORE-GEN-007); CORE-COMP-008 as above; the minter is unchanged on error.
- Clauses to cite: CORE-INT-001 to -003, CORE-COMP-001 to -008, CORE-RULES-004, CORE-ID-004, CORE-TIME-003.

## Done

The step gate of `common.md` exits 0 in W; no `unimplemented!()` is left in `intent.rs`, `compile.rs` and
`read::reflow_inputs`; you committed in W. Report the commit, what you built, and every `for_lead` point.
