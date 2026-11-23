# Task B, step 2, `b2-apply`: `apply`, conflict rules, rules versions, reset, rebind, golden logs

Plan: SYNC_PLAN.md §6.2 items 2, 4, 5 and 6. This is the one function that changes replicated state: every
device must compute the same bytes from the same entries. `canon`, `time`, `board`, `ids` (task A) and
`command` (step B1) are in the working copy; build on them. If something there blocks you, make the smallest
change that unblocks you and say so in your report.

## Fill

- `src/apply.rs`: `Ctx`, `Applied`, `ChangeSet`, `Refusal`, `NotesConflict`, `Group`, `Notice`, `SkipReason`,
  `RefusalRecord` (both records with `to_canonical` / `from_canonical`, `Notice::concerns`),
  `SUPPORTED_RULES`, `HASH_VERSIONS`, `apply`, `upgrade_rules`, `reset`.
- `src/ids.rs`: `rebind` (§7.5).

| Read | core.md lines |
|---|---|
| §7 ids, marks, restore, rebinding | 360–400 |
| §8 `Ctx`, result, evaluation order | 402–466 |
| §9 the commands: checks and effects of each | 468–688 |
| §10 conflict rules, notice and refusal records | 690–769 |
| §11 rules versions, `upgrade_rules`, `reset` | 771–808 |
| §6.11–§6.12 archive key, archive order, `check` | 324–342 |
| §16 apply refusals and rules codes; §17 limits | 1206–1224; 1271–1314 |
| §18.2 rows `golden`, `reset.json`, `rebind.json`; §18.3 golden logs | 1337–1339; 1345–1362 |
| §20 `apply`, `ids::rebind` | 1515–1526; 1561–1580 |

Today's behaviour that §9.8 and §9.21 point at: `/home/user/taskdeck/src/archive.rs` lines 180–230
(`Archived::retire`, `to_active`).

## Points the lead has settled

1. `apply` is thin (CORE-CMD-006): it checks ranges, existence, kind, marks and caps as its section lists,
   in that order, and performs the listed effects. It never trims, defaults, snaps, clamps or resolves, and
   it consults no zone. Its only instants are `ctx.at` and the command's.
2. A refusal leaves the board untouched: validate everything before the first mutation (today's arms all do,
   R1 §3), or work on a copy. A benign repeat returns `Applied { benign: true, .. }` with an empty change
   set, no notices, and the bytes unchanged.
3. `core/bad_ctx` first (§8.4 step 1), then `core/rules` (the op table of `board.rules`), then the
   command's checks. Dispatch on `board.rules` even though only version 1 exists: `apply` for rules 1 is a
   function of its own that a later version will sit beside (§11.1).
4. Stamps, overwrite notices and skips follow §10.1–§10.4 exactly. "Another device's change since the
   origin's view" is `s > ctx.based_on && d != ctx.device_no && d != 0`. At most one notice per group and
   block a command touches; notices in the order produced; a benign repeat produces none.
5. `Refusal.target` is the command's `target()`. `Refusal.op` is the command's `op()`.
6. `reset` and `upgrade_rules` are §11.2 and §11.3 as written; both leave their input unchanged on error.
7. `rebind` is §7.5 as written: minting fields first, then referring fields, then re-sort the two arrays §9
   orders by sid; on `core/bad_id` the minter is unchanged.
8. **Golden logs with large start boards (a spec change the lead is making to §18.3).** A case may give
   `start_gen` in place of `start_text`, and then `end_sha256` in place of `end_text`. `start_gen` is
   `{"items": I, "blocks": B, "rows": R}` (integers, absent = 0) and names this board, which the test builds:
   rules 1; `I` items, item `i` (1 ≤ i ≤ I) being
   `{"created":"1970-01-01T00:00:00.000000000Z","id":i,"kind":"task","name":"t","stamps":{"estimate":[1,0],"levels":[1,0],"name":[1,0],"repeat":[1,0],"time":[1,0]}}`;
   `B` blocks with sids 1 to `B`, block `s` belonging to item `((s − 1) div 1000) + 1` (so `B ≤ 1000 × I`), each
   `{"minutes":15,"sid":s,"stamp":[1,0],"start":"1970-01-01T00:00:00.000000000Z"}`; `R` archive rows, row `j`
   (1 ≤ j ≤ R) being `{"archived_at": <the instant j − 1 seconds after 1970-01-01T00:00:00Z>,
   "created":"1970-01-01T00:00:00.000000000Z","id":I+j,"kind":"task","name":"t"}`; and, when `I + R > 0` or
   `B > 0`, the one mark `{"block":B,"device_no":0,"item":I+R}`. Use it for the caps of §17 (10,000 items;
   1,000 blocks per item; 50,000 live blocks; 200,000 rows), each at its value and one above.
9. Golden logs are data that pins rules version 1 for every later build (CORE-RULES-003). Write the cases'
   inputs by hand. For the expectations (`result`, `code`, `notices`, `moved`, `skipped`, `restored_as`,
   `board_sha256`, `end_text`) you may let a test print what the implementation computes, but then read each
   expectation against the spec before you store it: a golden file that records a bug pins the bug. Tests
   cannot write files or read the environment (`common.md`), so print to stdout under `--nocapture` and store
   the output with your own tools.

## Tests and vectors to write

- `tests/vectors/golden/rules-1/*.json` in the format of §18.3, one file per topic (`create`, `blocks`,
  `archive`, `notes`, `subscriptions`, `settings`, `ids`, `conflicts`, `caps`), covering what §18.3's last
  paragraph lists: every check of every op with a refused case, every row of §10.6, every notice kind and
  group, skips for both reasons, and each boundary value it names.
- The test target **`tests/golden.rs`** (the stage gate runs `cargo test --locked -p taskdeck-core --test golden`):
  replays every case and compares every expectation and the end board.
- `tests/vectors/reset.json` and `tests/vectors/rebind.json` with the cases §18.2 names, and their test.
- Property tests (`tests/apply_props.rs`), over boards that pass `check` and commands that `parse_command`
  accepts: `apply` never panics (CORE-GEN-008) and leaves a board that passes `check` (CORE-DATA-006); a
  refusal leaves the canonical bytes unchanged (CORE-APPLY-002); two applies of equal inputs give equal
  results and bytes (CORE-CTX-002, CORE-CONF-007); the archive stays in archive order (CORE-DATA-003).
- Clauses to cite: CORE-ID-002, -003, -005, -006; CORE-CTX-001 to -003; CORE-APPLY-001 to -004; CORE-CMD-004 to
  -009; CORE-CONF-001 to -007; CORE-RULES-001, -002, -003, -005; CORE-DATA-003, -006; CORE-GEN-006, -008, -010.

## Done

The step gate of `common.md` exits 0 in W, and so does `cargo test --locked -p taskdeck-core --test golden`;
no `unimplemented!()` is left in `src/apply.rs` and `src/ids.rs`; you committed in W. Report the commit, what
you built, and every `for_lead` point.
