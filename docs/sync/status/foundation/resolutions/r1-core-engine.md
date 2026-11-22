# Resolutions R1: core.md against engine.md (17 findings)

Resolver 3 of 9. Inputs read: reviews.md §B, spec/README.md, foundation.md "Decisions" and "Rulings for the specs" (S1 to S31, with the settled S15), SYNC_PLAN.md §2 and §3, the earlier resolution files `r1-seal-relay.md` and `r1-seal-engine.md` in full, core.md §1 to §11, §13.1, §16 to §21, engine.md §1 to §13, §15 to §19, §21, §22, §24, §27, §28, §30, §33 to §38, and the seal.md and device.md passages the changes touch.

Built on the earlier files (settled; not repeated here):
- core-engine-7 is fully applied by R1-SEAL-ENGINE-19 (ruling S25).
- core-engine-8 is the S24 question the lead already has (R1-SEAL-RELAY-8, R1-SEAL-ENGINE-13, and their "For the lead" item 1).
- core-engine-15 is applied by R1-SEAL-ENGINE-16 (`engine/bad_host`, ENG-OPS-007); only a §34 row is added here.
- core-engine-16 builds on R1-SEAL-ENGINE-20 (the §7 table rows, the deleted §36 device bullet).
- core-engine-17 is answered by R1-SEAL-RELAY-9, R1-SEAL-RELAY-28 and R1-SEAL-ENGINE-15 (the history travels inside the snapshot body; the sealer fills and keeps it); only core's limit wording follows here.
- core-engine-11 and core-engine-14 edit engine.md §19.7 step 3 as R1-SEAL-ENGINE-14 rewrote it.

New clause ids assigned here (later resolvers continue after them, and after those of the earlier files):
- core.md: CORE-ID-006, CORE-RULES-005.
- engine.md: ENG-REF-004.

New error codes: none. New trait method: `StateMachine::refusal_bytes` (engine.md §5.1). New core functions: `apply::reset`, `ids::rebind`.

Notation. "Replace A with B" means the exact text A, as it stands in the spec after the changes of `r1-seal-relay.md` and `r1-seal-engine.md`, becomes B. Clause texts are given in full. A table row is given in full, in the table's own column order.

## seal.md

### R1-CORE-ENGINE-1 (core-engine-2)

The plaintext's `at_ms` ends at the last millisecond of 9999, the end of `core.md` §5.1's instant range, so every opened entry's `at_ms` becomes `Ctx.at` and no device meets an instant it cannot represent. An entry above it is refused at classification, on every device alike, with the existing code `seal/bad_plaintext` (an alarm naming the author, as for any `at_ms` out of range). No byte layout changes; only the accepted range narrows.

1. §10.1, the layout table: replace the row

   `| 17 | `at_ms` | 8 | i64 | the author's clock when it composed the entry; 0 to 2^53 − 1 |`

   with

   `| 17 | `at_ms` | 8 | i64 | the author's clock when it composed the entry; 0 to 253,402,300,799,999 (9999-12-31T23:59:59.999Z, the end of `core.md` §5.1's instant range) |`

2. §10.3, step 5: replace `5. `kind` must be 1, 2, 3 or 4; `at_ms` must be from 0 to 2^53 − 1.` with:

   5. `kind` must be 1, 2, 3 or 4; `at_ms` must be from 0 to 253,402,300,799,999.

3. §23 Limits: add, after the row `| payload, maximum | 257,979 | bytes | SEAL-PAD-002 |`:

   `| `at_ms` | 0 to 253,402,300,799,999 | ms since 1970-01-01T00:00:00Z (to 9999-12-31T23:59:59.999Z) | SEAL-PAD-003 |`

4. §24 Test vectors, row `seal-padding.json`: replace its Inputs cell with:

   `payload_len` values (including 0, 443, 444, 8123, 8124, 257,979, 257,980) and malformed plaintexts, including `at_ms` 253,402,300,799,999 (accepted) and 253,402,300,800,000 (refused)

   (The §22 row `seal/bad_plaintext`, as R1-SEAL-ENGINE-8 left it, already names an `at_ms` out of range and is unchanged.)

## core.md

### R1-CORE-ENGINE-2 (core-engine-1)

A refusal record may carry the two engine codes the engine records (`engine.md` §13.3): `engine/skipped` and `engine/reseal_limit`. No other engine code reaches a refusal record (an own entry refused because its author was revoked carries `seal/revoked_author`, a seal code), so none is added.

§10.7, the **Refusal** table: replace the row

`| `code` | string | required | a code of §16 or of `seal.md` §22 | why |`

with

`| `code` | string | required | a code of §16, a code of `seal.md` §22, or `engine/skipped` or `engine/reseal_limit` (`engine.md` §33) | why |`

(CORE-CONF-006 and `RefusalRecord::from_canonical` are unchanged: they now accept those two codes through this row.)

### R1-CORE-ENGINE-3 (core-engine-2)

With `seal.md` §10.1 bounding `at_ms` (R1-CORE-ENGINE-1), `Instant::from_at_ms` succeeds for every opened entry. "Plus one day" saturates at the end of the range, so the archive clamp is defined for an `at` within a day of it. `core/bad_ctx` stays a fault of the caller.

1. §5.1: replace

   An instant from a plaintext's `at_ms` (`seal.md` §10.1, 0 to 2^53 − 1) is `seconds = at_ms div 1000`, `nanos = (at_ms mod 1000) × 1,000,000`. "Plus one day" adds 86,400 seconds.

   with

   An instant from a plaintext's `at_ms` (`seal.md` §10.1: 0 to 253,402,300,799,999, which is 9999-12-31T23:59:59.999Z, so the `at_ms` of every opened entry converts) is `seconds = at_ms div 1000`, `nanos = (at_ms mod 1000) × 1,000,000`. "Plus one day" adds 86,400 seconds; a sum after 9999-12-31T23:59:59.999999999Z is that instant (`Instant::MAX`).

2. §8.1, the `Ctx` table: replace the row

   `| `at` | the plaintext's `at_ms` (§5.1): the origin's clock when it compiled the command | | `created` of a created item; the archive clamp of `complete` and `delete` |`

   with

   `| `at` | the plaintext's `at_ms` (§5.1): the origin's clock when it compiled the command | within §5.1's range; every opened entry's `at_ms` converts (`seal.md` §10.1) | `created` of a created item; the archive clamp of `complete` and `delete` |`

3. Reword CORE-CMD-005 (full text):

   - **CORE-CMD-005** (core) `apply` of `complete` and `delete` MUST set `archived_at` to `min(at, ctx.at + 86,400 s)`, the sum taken as `Instant::MAX` when it would be later (§5.1).

4. §18.3, the last sentence but one of the paragraph after the JSON (it begins `` `code` is present for ``): replace `` `based_on` equal to a stamp's seq and one below it. `` with `` `based_on` equal to a stamp's seq and one below it, and a `complete` whose `ctx.at` is less than a day before the end of the instant range (the saturating clamp). ``

(The §16 row `core/bad_ctx` and CORE-CTX-001 are unchanged.)

### R1-CORE-ENGINE-4 (core-engine-3)

`SM::reset` gets its core function. The rules stay those of `current`: a `ResetTo` never changes the membership (`engine.md` ENG-RESET-003), and `BoardData.rules` mirrors the membership's `rules` (§11.2).

1. Add a new subsection after CORE-RULES-004 (the end of §11.2), before `## 12. Intents`:

   ### 11.3 Reset

   A `ResetTo` (`seal.md` §13.6, `engine.md` §28.1) installs an earlier state of the board and leaves the membership unchanged. `reset(target, current)` computes the board it installs; `target` is the board at the reset's target seq and `current` the board just before the `ResetTo` entry. In order:

   1. `target.rules ≤ current.rules`, else `core/unknown_rules`;
   2. the result `b` is `target` with every field kept, except:
      - `marks`: one mark for each `device_no` that has a mark in `target` or in `current`, its `item` and its `block` each the larger of the two boards' values (an absent mark counts as 0), ascending by `device_no`;
      - `notes_rev = max(target.notes_rev, current.notes_rev) + 1`;
   3. when `b.rules < current.rules`: `upgrade_rules(&mut b, current.rules)` (§11.2); its error is returned.

   The marks keep every number minted since the target from being minted again (§7.2); the raised revision makes every `set_notes` based on a revision of either board stale (§10.5); and `b.rules` mirrors the unchanged membership. `reset` produces no notices and, like `apply`, is a function of its arguments alone (CORE-CTX-002).

   - **CORE-RULES-005** (core) `reset` MUST return `target` unchanged except that its marks are the elementwise maximum of both boards' marks, its `notes_rev` is one above the larger of the two, and its rules version is raised to `current.rules` by `upgrade_rules`, and MUST return `core/unknown_rules` when `target.rules` exceeds `current.rules`.

2. §16, the **Rules** table: replace the row `| `core/unknown_rules` | `upgrade_rules` to a version that is not newer or not supported | §11.2 | engine: halt with the rules halt (`seal.md` §14.5, `engine.md`) |` with:

   `| `core/unknown_rules` | `upgrade_rules` to a version that is not newer or not supported; `reset` with a target newer than the current board | §11.2, §11.3 | engine: halt with the rules halt (`seal.md` §14.5, `engine.md`); from `reset`, the local fault `engine/local_fault` (`engine.md` §28.1) |`

3. §18.2 Families: add after the row that begins `| `golden/rules-1/*.json` |`:

   `| `reset.json` | §11.3 | target and current board texts | the result board text and its `board_sha256`, or the code; cases with a mark in only one board, with each board's `notes_rev` the larger, and with equal rules |`

4. §19, `engine.md` block: replace the last bullet, `- Provides: §8 to §11 as pure functions; `compile` for the trial apply at enqueue.`, with (this one text also covers R1-CORE-ENGINE-2 and R1-CORE-ENGINE-5):

   - Provides: §8 to §11 as pure functions, including `reset` (§11.3) behind `SM::reset` and `ids::rebind` (§7.5) behind `SM::rebind`; `RefusalRecord::to_canonical` (§10.7) behind `SM::refusal_bytes`; `compile` for the trial apply at enqueue.

5. §20 Public API, module `apply`: after the line `    pub fn upgrade_rules(board: &mut BoardData, v: u32) -> Result<(), super::CoreError>;` add:

   ```rust
       pub fn reset(target: &BoardData, current: &BoardData) -> Result<BoardData, super::CoreError>;   // §11.3
   ```

(CORE-GEN-006 gains `reset` in R1-CORE-ENGINE-5.)

### R1-CORE-ENGINE-5 (core-engine-4, core-engine-9)

`SM::rebind` gets its core function, which mints from the **new** identity's current minter against its pending view, with an exact list of the fields that mint and of those that refer.

1. Add a new subsection after CORE-ID-005 (the end of §7.4), before `## 8. `Ctx` and `apply``:

   ### 7.5 Rebinding

   When a device pairs again as a new identity, the commands its old identity left unsent are resubmitted under the new `device_no` (`engine.md` §24.5). `rebind(commands, from_device_no, view, minter)` rewrites them; `minter` is the new identity's current `IdMinter` (its `device_no` is the new one) and `view` its pending state. Two maps start empty: the item map (item and subscription ids) and the block map (sids). For each command in order:

   1. **Minting fields**, in this order: `create_task`: `id`, then each `blocks[].sid` in array order; `create_event` and `create_routine`: `id`; `add_block`: `sid`; `restore`: `fallback`; `add_subscription`: `id`. Each value whose high part is `from_device_no` is replaced by a new number, `minter.mint_item(view)` for `id` and `fallback`, `minter.mint_block(view)` for a sid, and the pair is added to the item map or the block map. A value with another high part is kept.
   2. **Referring fields.** Every value equal to a key of the matching map is replaced by its value. Item-space references: the `id` of `rename`, `complete`, `delete`, `forget`, `move_block`, `add_block`, `set_estimate`, `set_deadline`, `set_severity`, `set_horizon`, `set_repeat`, `move_routine`, `set_event_span`, `restore`, `forget_archived`, `remove_subscription`, `rename_subscription`, `set_subscription_color` and `set_subscription_enabled`; `moves[].id` of `move_blocks`; `blocks[].id` of `remove_blocks`. Sid references: the `sid` of `move_block`; `moves[].sid` of `move_blocks`; `blocks[].sid` of `remove_blocks`.
   3. The arrays that §9 orders by sid (`create_task.blocks`, `remove_blocks.blocks`) are sorted again ascending by sid. Every other value is kept, so each returned command parses (§9.1).

   An id the old identity minted in an entry that was accepted is not in either map, so references to it are kept. `rebind` returns `core/bad_id` when the minter has no number left (§7.1); the minter is then unchanged.

   - **CORE-ID-006** (core) `rebind` MUST replace, in command order, every value a minting field of §7.5 holds with high part `from_device_no` by a number from `minter` (`mint_item` for item-space values, `mint_block` for sids), MUST replace every referring field of §7.5 that equals a value replaced so far in the same space by its replacement, and MUST leave every other value unchanged.

2. Reword CORE-GEN-006 (full text):

   - **CORE-GEN-006** (core) The functions `apply`, `upgrade_rules`, `reset`, `ids::rebind`, the canonical encoder and parser, `BoardData::check` and the id functions of §7 MUST NOT use `f32` or `f64` values, MUST NOT iterate a `HashMap` or `HashSet`, and MUST NOT depend on the order of any collection other than the orders this spec defines.

3. §18.2 Families: add after the row `reset.json` (R1-CORE-ENGINE-4):

   `| `rebind.json` | §7.5 | command texts, `from_device_no`, view board text, minter | the rebound command texts and the minter after, or `core/bad_id`; cases for every minting and referring field, a reference to an accepted id, and a re-sorted `remove_blocks` |`

4. §19, `engine.md` block: add after the bullet that begins `- Requires: rebase re-applies the pending log commands`:

   - Requires: a resubmit after a re-pair (`engine.md` §24.5) rebinds the commands with `ids::rebind` (§7.5) from the new identity's current minter against its pending view, and applies them with the resubmitting device's `at_ms` and head, so their `created` instants, archive clamps and overwrite notices follow that clock and head.

5. §20 Public API, module `ids`: after the `impl IdMinter { … }` block (it ends `pub fn mint_block(&mut self, view: &super::board::BoardData) -> Option<u64>; }`) add:

   ```rust
       pub fn rebind(commands: &[super::command::LogCommand], from_device_no: u16, view: &super::board::BoardData,
                     minter: &mut IdMinter) -> Result<Vec<super::command::LogCommand>, super::CoreError>;   // §7.5
   ```

### R1-CORE-ENGINE-6 (core-engine-5)

The compile clock is the origin's own clock (`engine.md` §27.3, ENG-CLOCK-001).

1. §13.1, the code block: replace `    pub now: Instant,            // the origin's (skew-corrected) clock, in whole milliseconds` with `    pub now: Instant,            // the origin's own wall clock, in whole milliseconds, never corrected by a relay-derived offset (engine.md §27.3)`.
2. §1, the first non-goal bullet: replace `the skew-corrected clock` with `the clock-skew guard`.

### R1-CORE-ENGINE-7 (core-engine-6)

§19, `engine.md` block: replace the bullet `- Requires: the engine records notices only from applies to the confirmed state, keeps a notice for its own `device_no` as `author` or `other`, and a refusal of its own entries, as the records of §10.7 in the sealed notices file.` with:

- Requires: the engine records notices only from applies to the confirmed state, keeps a notice for its own `device_no` as `author` or `other`, and a refusal of its own entries, as the records of §10.7 in the sealed notices file, and records each seq's records once however often it applies the entry, a Skip replay or an adoption replacing those it changes (`engine.md` §13.4).

### R1-CORE-ENGINE-8 (core-engine-11)

§16, the **Parse and data codes** table: replace the row `| `core/invariant` | board bytes parse but break a rule of §6.12 (the rule number is reported) | `from_canonical_bytes`; import step 15 | engine: raise an integrity alarm and do not use the bytes (`engine.md`); import: report a failure |` with:

`| `core/invariant` | board bytes parse but break a rule of §6.12 (the rule number is reported) | `from_canonical_bytes`; import step 15 | engine: a snapshot whose board fails is passed over with the alarm `BlobFailure` (`engine.md` §19.7, §19.9, §22.3); its own checkpoint or base is the local fault `engine/local_fault` (`engine.md` §7.10); import: report a failure |`

### R1-CORE-ENGINE-9 (core-engine-12)

§10.5: replace `The origin still holds its draft (the refused command's `text` is in its outbox entry), so it can show its draft` with `The origin still holds its draft (the device keeps the unsaved draft, `device.md` §17.6; the outbox record is dropped when its refusal is recorded, `engine.md` §7.4), so it can show its draft`.

### R1-CORE-ENGINE-10 (core-engine-13)

In synced mode the engine does not pass the change set on; the device derives the same set from successive pending views (`device.md` §13.1.3, R1-CORE-ENGINE-23).

1. §8.3, the paragraph after the code: append at its end:

   In synced mode `apply` runs inside the engine, which does not pass the change set on (`engine.md` §5.1 `Applied`); the device then drops the overlay of every subscription that is absent or disabled in the new pending view (`device.md` §13.1.3), which is the same set.

2. §19, `device.md` block: replace the bullet `- Requires: the local-only `Store` writes today's files from the `ChangeSet`, archive changes before item changes (R1 §3), and drops overlay events for every id in `overlay_forget`.` with:

   - Requires: the local-only `Store` writes today's files from the `ChangeSet`, archive changes before item changes (R1 §3), and drops overlay events for every id in `overlay_forget`; in synced mode the device drops the overlay of every subscription absent or disabled in the new pending view (`device.md` §13.1.3).

### R1-CORE-ENGINE-11 (core-engine-17)

Ruling S27 as R1-SEAL-RELAY-9 applies it: the control history travels inside the snapshot body, so the board cap's reason names it. (The finding's separate history blob is not adopted: the earlier resolutions settled the history inside the body.)

§17 Limits: replace the row `| canonical board bytes | 62,914,560 | bytes (60 MiB, so a snapshot body with membership stays under 64 MiB) | CORE-DATA-004 |` with:

`| canonical board bytes | 62,914,560 | bytes (60 MiB, so that a snapshot body, which holds the board, the membership and the control history, `seal.md` §20.4, stays under 64 MiB inflated) | CORE-DATA-004 |`

## engine.md

### R1-CORE-ENGINE-12 (core-engine-1, core-engine-10)

The engine encodes refusal records through the state machine, as it does notices; `op` and `target` come from the command when it parses and are absent otherwise, which is what `core.md` §10.7 says (no core change for core-engine-10: an own entry's outbox record holds the same payload as its frame, so it cannot fill `op` when that payload does not parse); and the only engine codes a refusal record carries are `engine/skipped` and `engine/reseal_limit` (`core.md` §10.7, R1-CORE-ENGINE-2).

1. §5.1, the trait: after the line `    fn notice_bytes(&self, notice: &Self::Notice) -> Vec<u8>;` add:

   ```rust
       fn refusal_bytes(&self, seq: u64, author: u16, refusal: &SmRefusal) -> Vec<u8>;
   ```

2. §5.1, **Contracts** table: add after the row that begins `| `notice_bytes` |`:

   `| `refusal_bytes` | the canonical bytes of the refusal record of `core.md` §10.7 (`RefusalRecord::to_canonical`) with this `seq` and `author` and the refusal's `code`, `op`, `target`, `base` and `current` |`

3. §5.3, paragraph **Other methods.**: replace `A notice's bytes are `u64 seq ‖ u16 author ‖ u16 other ‖ u64 other_seq ‖ u8 key` (21 bytes); it concerns `author` and `other`.` with:

   A notice's bytes are `u64 seq ‖ u16 author ‖ u16 other ‖ u64 other_seq ‖ u8 key` (21 bytes); it concerns `author` and `other`. A refusal's bytes (`refusal_bytes`) are `u64 seq ‖ u16 author ‖ vb8(code) ‖ vb8(op) ‖ u8 has_target ‖ u64 target`, with `op` empty when absent, and `has_target = 1` with the target, or `has_target = 0` with eight zero bytes.

4. §7.7: replace `` `Add { record: Vec<u8> }` (a notice or refusal record of `core.md` §10.7, canonical bytes, with its kind) `` with `` `Add { record: Vec<u8> }` (a notice or refusal record of `core.md` §10.7, canonical bytes from `SM::notice_bytes` or `SM::refusal_bytes`, with its kind) ``.

5. §13.3: replace the whole paragraph (from `When an entry authored by this device reaches the board head refused,` to `leave no record (V2-16).`) with:

   When an entry authored by this device reaches the board head refused, by the sealer (a code that is not re-sealable), by a parse failure, by `apply`, or by a Skip, the engine appends a refusal record (`core.md` §10.7), with the bytes of `SM::refusal_bytes` (§5.1): `seq`, `author` (its own `device_no`), `code`, `op` and `target` (`SM::op_name` and `SM::target` of the command: the accepted entry's parsed payload, or, for an entry the sealer refused, the command its outbox record holds; both absent when that payload does not parse), and `base` and `current` (a `set_notes` conflict). Its `code` is a code of `core.md` §16 or `seal.md` §22, or one of the two engine codes `engine/skipped` (§17.3) and `engine/reseal_limit` (§24.3), the only engine codes a refusal record carries (`core.md` §10.7). A revoked device keeps its refusal records with its other records (§24.4, ENG-EPOCH-002); an own entry refused because its author was revoked carries the sealer's code (`seal/revoked_author`). Benign repeats (`core.md` §10.6) are successes and leave no record (V2-16).

6. §36, `core.md` block: replace the bullet `- Requires: a refusal record's `code` may be an `engine/` code (`engine/skipped`, `engine/reseal_limit`), widening `core.md` §10.7's "a code of §16 or of `seal.md` §22".` with:

   - Relies on: `core.md` §10.7 admits `engine/skipped` and `engine/reseal_limit` as refusal codes, besides the codes of `core.md` §16 and `seal.md` §22; the adapter's `SM::refusal_bytes` is `RefusalRecord::to_canonical` (`core.md` §20).

### R1-CORE-ENGINE-13 (core-engine-2)

`seal.md` §10.1 bounds `at_ms` by core's instant range (R1-CORE-ENGINE-1), so the adapter's conversion cannot fail and no refusal path is needed.

§36, `core.md` block: replace the bullet `- Requires: `Ctx.at` is `Instant::from_at_ms(at_ms)`; the adapter refuses an `at_ms` it cannot convert as `core/bad_ctx`.` with:

- Relies on: `Ctx.at` is `Instant::from_at_ms(at_ms)` (`core.md` §5.1), which succeeds for every opened entry because `seal.md` §10.1 bounds `at_ms` by 253,402,300,799,999, the last millisecond of 9999; the pending view and the trial apply use `at_ms` values the facade took from a `CompileCtx.now` within that range (§9.1).

### R1-CORE-ENGINE-14 (core-engine-3)

`SM::reset` is `core.md` §11.3 (`reset`, CORE-RULES-005, R1-CORE-ENGINE-4) and keeps `current`'s rules version, as the toy already does.

1. §5.1, the trait: replace `    fn reset(&self, target: &Self::State, current: &Self::State) -> Self::State;` with `    fn reset(&self, target: &Self::State, current: &Self::State) -> Result<Self::State, SmError>;`.

2. §5.1, **Contracts** table: replace the row `| `reset` | the state a `ResetTo` installs: `target`'s data with the id high-water marks of `current` kept (elementwise maximum) and any revision counter of `current` raised above both (§28.1) |` with:

   `| `reset` | the state a `ResetTo` installs (§28.1), `core.md` §11.3: `target`'s data with the id high-water marks of both kept (elementwise maximum), any revision counter raised one above both, and the rules version of `current`; an error only for a rules version this build lacks, which is the local fault `engine/local_fault` |`

3. §5.3, paragraph **Other methods.**: replace `` `reset(target, current)` is `target` with `rules = current.rules` and every mark the maximum of the two. `` with `` `reset(target, current)` is `Ok` of `target` with `rules = current.rules` and every mark the maximum of the two; it never fails. ``

4. §28.1, **Every device**, step 3: replace `3. Otherwise: confirmed state `= SM::reset(state_t, state_{r−1})`; a checkpoint; a rebase; banner `Reset`.` with:

   3. Otherwise: confirmed state `= SM::reset(state_t, state_{r−1})` (`core.md` §11.3; an error is the local fault `engine/local_fault`, §21.4, and the entry is not applied); a checkpoint; a rebase; banner `Reset`.

5. §36, `core.md` block: replace the bullet that begins `- Requires: the adapter's `SM::reset` (§5.1) is a core function` (the whole bullet) with:

   - Relies on: `core.md` §11.3 `reset` (CORE-RULES-005) behind `SM::reset` (§5.1): the target's board with `marks` the elementwise maximum of target and current, `notes_rev` one above both, and the rules version raised to the current one (a `ResetTo` never changes the membership, ENG-RESET-003), so no id is reissued, no notes base matches by coincidence, and `BoardData.rules` still mirrors `M.rules` after a reset (§28.1).

(The §33 row `engine/local_fault` gains `SM::reset` in R1-CORE-ENGINE-20.)

### R1-CORE-ENGINE-15 (core-engine-4, core-engine-9)

A resubmit rebinds from the new identity's current origin state against its pending view (`core.md` §7.5, R1-CORE-ENGINE-5), and stamps each resubmitted record with the `Resubmit`'s `wall_ms` and the board head, since `Unsent` carries payloads only.

1. §4.2, the events table: replace the row `| `Resubmit` | `ticket: u64, commands: Vec<C>, from_device_no: u16, origin_state: Vec<u8>` | a re-paired identity imports an old identity's unsent commands (§24.5) |` with:

   `| `Resubmit` | `ticket: u64, commands: Vec<C>, from_device_no: u16, origin_state: Vec<u8>` | a re-paired identity imports an old identity's unsent commands, with its own current origin state (§24.5) |`

2. §5.1, the trait: replace the two lines

   ```rust
       fn rebind(&self, commands: &[Self::Command], from_device_no: u16, to_device_no: u16,
                 origin_state: &[u8]) -> Result<(Vec<Self::Command>, Vec<u8>), SmError>;
   ```

   with

   ```rust
       fn rebind(&self, commands: &[Self::Command], from_device_no: u16, to_device_no: u16,
                 view: &Self::State, origin_state: &[u8]) -> Result<(Vec<Self::Command>, Vec<u8>), SmError>;
   ```

3. §5.1, **Contracts** table: replace the row `| `rebind` | rewrites commands minted by `from_device_no` for `to_device_no`: every id the commands create with high part `from_device_no` is re-minted from `origin_state`, and every later reference to it is rewritten; returns the commands and the new origin state (§24.5) |` with:

   `| `rebind` | `core.md` §7.5: rewrites commands minted by `from_device_no` for `to_device_no`; every id and sid the commands mint with high part `from_device_no` is minted anew from `origin_state`, the new identity's current origin state, against `view`, its pending view, and every later reference to it is rewritten; returns the commands and the origin state after those mints (§24.5) |`

4. §5.3, paragraph **Other methods.**: replace `` `rebind` maps, in order, every `Create` whose id has high part `from_device_no` to `to_device_no × 2^40 + next_n` (then `next_n += 1`), and rewrites `Name` and `Forget` ids by that mapping. `` with:

   `rebind` maps, in order, every `Create` whose id has high part `from_device_no` to `to_device_no × 2^40 + n`, with `n = max(next_n, m + 1)` and `m` the mark of `to_device_no` in `view` (0 when absent), then sets `next_n = n + 1`; it rewrites `Name` and `Forget` ids by that mapping and returns the commands with the final `next_n`.

5. Reword ENG-ENQ-005 (full text):

   - **ENG-ENQ-005** (engine) A record MUST carry `based_on = (B, C_B)` at enqueue and the `at_ms` of its `Enqueue` (for a resubmitted command, the `wall_ms` of the `Resubmit`, §24.5), and the engine MUST NOT change either when it seals or re-seals it.

6. §24.5, step 3: replace the whole step with:

   3. Once the new engine has joined (ENG-BOOT-003), the runtime sends `Resubmit { ticket, commands, from_device_no, origin_state }`, where `origin_state` is the new identity's current origin state (the `origin_state` of its newest `cmd` outbox record, or the initial one for its own `device_no` when it has none, `device.md` §18.4), never the old identity's. The engine calls `SM::rebind(commands, from_device_no, d, view, origin_state)` with `d` its own `device_no` and `view` its pending view; a `rebind` error refuses every command with that code. Then it runs §9.1 steps 3 to 5 for each rebound command in order, with `at_ms` the `wall_ms` of the `Resubmit` input and `based_on = (B, C_B)` in the trial apply's `Ctx` and in the record, and with the origin state `rebind` returned as each record's `origin_state`. The commands' own instants (`complete.at` and the like) keep their compiled values; their original `at_ms` and `based_on` are not known (`Unsent` carries payloads only, §30.3). It answers `Notify(ResubmitReport { ticket, queued, refused: Vec<(index, code)> })`.

7. Reword ENG-EPOCH-003 (full text):

   - **ENG-EPOCH-003** (engine) The engine MUST resubmit imported commands in their original order, rebound by `SM::rebind` to its own `device_no` from the new identity's current origin state, each through the trial apply of §9.1 with `at_ms` the `wall_ms` of the `Resubmit` and `based_on = (B, C_B)`, and MUST report every command it refused.

8. §30.3: replace `` `Unsent.commands` are the commands' encoded payloads (`SM::encode`). `` with:

   `Unsent.commands` are the commands' encoded payloads (`SM::encode`). They carry no `at_ms` or `based_on`: a resubmitted command is stamped with the resubmitting device's clock and head (§24.5). `Unsent.origin_state` is the old identity's and is not used for the resubmit.

9. §36, `core.md` block: replace the bullet that begins `- Requires: a core function behind `SM::rebind` (§24.5)` (the whole bullet) with:

   - Relies on: `core.md` §7.5 `ids::rebind` (CORE-ID-006) behind `SM::rebind` (§24.5): every id and sid the commands mint with the old `device_no` is minted anew from the new identity's current minter against its pending view, and every later reference is rewritten.

### R1-CORE-ENGINE-16 (core-engine-5)

`core.md` §13.1 now names the origin's own clock (R1-CORE-ENGINE-6). §36, `core.md` block: delete the bullet `- Assumes: `core.md` §13.1's "(skew-corrected) clock" is the device's own clock after the user confirmed it (§27); no relay offset is ever added (§27.3).`

### R1-CORE-ENGINE-17 (core-engine-6)

Each seq's notice and refusal records are recorded once, however often the engine applies the entry. The key is derived from the notices file itself (no new `Meta` field: records are appended in apply order, so a seq at or below the highest recorded seq was already recorded or produced none). The two replays that change results replace the records they change.

1. §13.4: replace the paragraph `Every notice `apply` returns for the confirmed state and that `notice_concerns` this device's `device_no` is appended as a notice record (`core.md` §10.7); notices from the pending view are never recorded (ENG-SM-004). The user dismisses records (`UserAction::DismissNotice`), which appends `Dismiss`. Records are kept per §7.13.` with:

   Every notice `apply` returns for the confirmed state and that `notice_concerns` this device's `device_no` is appended as a notice record (`core.md` §10.7); notices from the pending view are never recorded (ENG-SM-004). The user dismisses records (`UserAction::DismissNotice`), which appends `Dismiss`. Records are kept per §7.13.

   **Each seq once.** The engine applies some entries more than once: at `Start` above an older checkpoint (ENG-STATE-002), after a caught panic (§15.3), and in the replays of §17.3, §19.9, §22.3 and §28.1. It appends notice and refusal records for a seq `s` only when `s` is above `notices_through`, the highest `seq` among the records it holds (computed from `Loaded.notices` at `Start`, raised by every record it appends); entries at or below it are applied without recording. A replay that only computes a state (§28.1 stage 1 and step 1, §22.3 step 4, §28.3 stage 3) records nothing. Two replays change what earlier applies produced and replace those records: a Skip replay (§17.3 step 3) from its target `t`, and an adoption (§22.3 step 5) from `S' + 1`. Each ends with one `RewriteNotices` that keeps every record with a seq below that first seq, then holds, in seq order, the records the replay produced for the seqs from it to the replay's end, with a `Dismiss` for each new record whose bytes equal those of a dismissed old record of the same seq (each old record matched at most once, in file order).

2. Reword ENG-REF-003 (full text):

   - **ENG-REF-003** (engine) The engine MUST record exactly the notices from confirmed applies that concern its own `device_no`, in the order `apply` returned them, and MUST record each seq's notice and refusal records once: none for a seq at or below the highest seq of the records it holds, except by the replacement of ENG-REF-004.

3. §13.4: add after ENG-REF-003:

   - **ENG-REF-004** (engine) After a Skip replay or an adoption, the engine MUST replace, in one `RewriteNotices`, every notice and refusal record whose seq is at or above the replay's first seq (the Skip's target, or `S' + 1`) by the records the replay produces, and MUST keep a new record dismissed exactly when a dismissed old record of the same seq has equal bytes.

4. §17.3, step 3: replace `rewrites the hash records of every aligned seq from `t` to `s` (`AppendHashes`, the newer record winning, §7.5); writes a checkpoint; and rebases.` with `rewrites the hash records of every aligned seq from `t` to `s` (`AppendHashes`, the newer record winning, §7.5); replaces its notice and refusal records from `t` on by those of the replay (§13.4, ENG-REF-004); writes a checkpoint; and rebases.`

5. §22.3, step 5: replace `5. The confirmed state becomes the snapshot's board at `S'`; the board is replayed from `S' + 1` to `K` from the log; hash records from `S'` are rewritten; a checkpoint is written; the divergence stop ends.` with:

   5. The confirmed state becomes the snapshot's board at `S'`; the board is replayed from `S' + 1` to `K` from the log; hash records from `S'` are rewritten; notice and refusal records from `S' + 1` are replaced by those of the replay (§13.4, ENG-REF-004); a checkpoint is written; the divergence stop ends.

### R1-CORE-ENGINE-18 (core-engine-7)

Nothing further to apply: R1-SEAL-ENGINE-19 already applies ruling S25 (§19.2 with both triggers and the 24-hour limit, ENG-SNAP-001 reworded, ENG-SNAP-006, the §34 rows "snapshot entry trigger" and "snapshots per device", §38 item 14, and S25 in §1).

### R1-CORE-ENGINE-19 (core-engine-8)

Nothing further to apply. Ruling S24 has no delivery channel yet: a reinstating entry is sealed under an epoch the reinstated device lacks. R1-SEAL-RELAY-8 and R1-SEAL-ENGINE-13 wrote the conservative reading (a reinstated device pairs again; §24.4, §36, §38 item 24), and the channel is with the lead ("For the lead", item 2 below adds what the engine needs once it is ruled).

### R1-CORE-ENGINE-20 (core-engine-11, core-engine-14)

A board that fails `SM::load`, or whose rules version is not the membership's, has one defined outcome on every path: a snapshot is passed over with `BlobFailure` (or `engine/adopt_membership` for an adoption), the genesis is `engine/bad_genesis`, and the device's own checkpoint, base or pairing material is the local fault `engine/local_fault`.

1. §7.10: add this paragraph directly before ENG-STATE-002:

   A checkpoint, or a base read later (`ReadBase`), whose board `SM::load` refuses is the local fault `engine/local_fault` (§21.4); the engine applies nothing from it.

2. §19.7, step 3, as R1-SEAL-ENGINE-14 wrote it: replace `then: the membership bytes parse, name this board and list this device as current; the state hash recomputed (§18.3) equals the body's. A failure (including the lack of the root of the blob's epoch) moves to the next candidate.` with:

   then: the membership bytes parse, name this board and list this device as current; the board loads (`SM::load`) and `SM::rules` of it equals the membership's `rules`; the state hash recomputed (§18.3) equals the body's. A failure (including the lack of the root of the blob's epoch) moves to the next candidate; a failed `SM::load` also raises the alarm `BlobFailure` with the writer, the blob id and the load's code.

3. §19.8, the **Genesis** bullet: replace `The engine loads `board`, classifies seq 1 (the sealer uses its genesis anchor), which must be `Accept` with a `Genesis` item, checks that the item's `state_hash` equals the state hash of `board` and `M_1`, and persists `AppendLog[1]`, a checkpoint and a base at 1. Otherwise it stops with `engine/bad_genesis`.` with:

   The engine loads `board` (`SM::load`), classifies seq 1 (the sealer uses its genesis anchor), which must be `Accept` with a `Genesis` item, checks that `SM::rules(board) = M_1.rules` and that the item's `state_hash` equals the state hash of `board` and `M_1`, and persists `AppendLog[1]`, a checkpoint and a base at 1. Otherwise (a failed load included) it stops with `engine/bad_genesis`.

4. §19.8, the **Join** bullet: replace `The engine loads the snapshot at `h` as in §19.7 step 4 (no user confirmation: the log was empty),` with `The engine loads the snapshot at `h` as in §19.7 step 4 (no user confirmation: the log was empty; a material whose board `SM::load` refuses, or whose `SM::rules` differs from `M_h.rules`, is the local fault `engine/local_fault`, and the user pairs again),`.

5. Reword ENG-BOOT-004 (full text):

   - **ENG-BOOT-004** (engine) At genesis, the engine MUST check the `Genesis` item's state hash against the state hash it computes from the genesis state and `M_1`, and the genesis state's rules version (`SM::rules`) against `M_1.rules`, and MUST refuse to start the board when either differs.

6. §19.9: replace `It then opens it with `exact = true` and the `snap` entry's `SnapInfo`, checks that the membership bytes equal its own `M_S`, loads the board at `S`, sets `B = S` and applies the classified entries after it.` with:

   It then opens it with `exact = true` and the `snap` entry's `SnapInfo`, checks that the membership bytes equal its own `M_S`, loads the board at `S` (`SM::load`) and checks `SM::rules(board) = M_S.rules`, sets `B = S` and applies the classified entries after it. A snapshot that fails to open or to load, or fails either check, is passed over (alarm `BlobFailure` with the writer, the blob id and the code of a failed open or load), and the device waits for the next usable snapshot.

7. Reword ENG-BOOT-005 (full text):

   - **ENG-BOOT-005** (engine) A device that cannot replay to a Skip or `ResetTo` target MUST NOT apply the entry that needs it, and MUST resume only from a usable snapshot above that entry whose membership bytes equal its own and whose board loads with that membership's rules version.

8. §22.3, step 3: replace `3. Fetch and open it with `exact = true` and its `SnapInfo`; recompute its state hash (§18.3).` with:

   3. Fetch and open it with `exact = true` and its `SnapInfo`, load its board (`SM::load`) and recompute its state hash (§18.3); a failure to open or to load raises `BlobFailure` with the writer, the blob id and the code, and nothing changes.

9. §22.3, step 4: replace `its membership bytes must equal the snapshot's; else `engine/adopt_membership` and nothing changes.` with `its membership bytes must equal the snapshot's, and `SM::rules` of the snapshot's board must equal `M_{S'}.rules`; else `engine/adopt_membership` and nothing changes.`

10. Reword ENG-DIVERGE-004 (full text):

    - **ENG-DIVERGE-004** (engine) The engine MUST refuse an adoption whose snapshot's membership bytes differ from its own membership at the same seq, or whose board's rules version differs from that membership's `rules`.

11. §21.4: replace `` `engine/local_fault` (a panic in a trait method other than `apply`, or an inconsistency between two of the device's own records) `` with `` `engine/local_fault` (a panic in a trait method other than `apply`, an inconsistency between two of the device's own records, a checkpoint, base or pairing material of its own whose board does not load, or an `SM::reset` error) ``.

12. §33 Error catalogue: replace these three rows

    `| `engine/local_fault` | a trait method other than `apply` panicked, or two own records disagree | ENG-GEN-012, §19.4, §21.4 | stop; restart; report |`
    `| `engine/bad_genesis` | the genesis entry or state does not match | §19.8 | the board cannot be used; report |`
    `| `engine/adopt_membership` | the snapshot's membership differs from this device's | §22.3 | update, or re-pair |`

    with

    `| `engine/local_fault` | a trait method other than `apply` panicked, two own records disagree, an own checkpoint, base or pairing material does not load, or `SM::reset` failed | ENG-GEN-012, §7.10, §19.4, §19.8, §21.4, §28.1 | stop; restart; report |`
    `| `engine/bad_genesis` | the genesis entry or state does not match (state hash or rules version), or the genesis board does not load | §19.8 | the board cannot be used; report |`
    `| `engine/adopt_membership` | the snapshot's membership, or its board's rules version, differs from this device's | §22.3 | update, or re-pair |`

    (each row stays in its current position in the table).

### R1-CORE-ENGINE-21 (core-engine-15)

R1-SEAL-ENGINE-16 already added the check (`engine/bad_host`, ENG-OPS-007, at most 255 bytes by `seal.md` §5.4 as R1-SEAL-RELAY-1 left it). Only the limit row is missing.

§34 Limits: add after the row `| seed-tail request | 256 frames, 1,048,576 bytes | | ENG-MOVE-003 |`:

`| host in `Move`, `FollowAddress`, `CloseBoard` | 255 | bytes, `:port` included (`seal.md` §5.4) | ENG-OPS-007 |`

### R1-CORE-ENGINE-22 (core-engine-16)

Adds to R1-SEAL-ENGINE-20: the engine's table names the bootstrap file it reads, and says which kinds it neither writes nor reads.

1. §7, the table as R1-SEAL-ENGINE-20 left it: add after the row `| `Marker` | 10 `marker.tdk` | rewritten whole, or removed |`:

   `| `Bootstrap` (§7.10) | 17 `bootstrap.tdk` | written by the runtime before the first `Start` of a new replica; removed by it after the first checkpoint (`device.md` §6.1) |`

2. §7: add this sentence directly after the table, before `### 7.1 In-memory state`:

   The engine writes and reads no other kind: the control history for backups (`history.tdk`, kind 11, `device.md` §15.7), the per-device settings (`device.tdk`, kind 12) and the unsent commands of a re-pair (`unsent.tdk`, kind 18, `device.md` §18.4) are `device.md`'s.

3. §36, `device.md` block, the bullet that begins `- Requires: backups read the confirmed state`: replace `` `device.md` keeps them in their own file. `` with `` `device.md` keeps them in `history.tdk` (kind 11, `device.md` §15.7). ``

## device.md

### R1-CORE-ENGINE-23 (core-engine-13)

In synced mode the engine passes no change set on, so the device derives `overlay_forget` from the pending view (`core.md` §8.3, R1-CORE-ENGINE-10).

§13.1.3: add this paragraph after the paragraph that begins `The sync thread adopts the new overlay as today`:

**Removed and disabled subscriptions (synced mode).** `apply` runs inside the engine, which does not pass the change set on (`engine.md` §5.1), so after each `ViewChanged` the sync thread drops from the overlay the events and statuses of every subscription id that is absent from the new pending view or has `enabled: false` there, which is the set `overlay_forget` names in local-only mode (`core.md` §8.3, §9.3 item 7), and writes `overlay.tdk` when the overlay changed. A subscription that comes back (a refused removal, or enabled again) is fetched again by the usual schedule.

### R1-CORE-ENGINE-24 (core-engine-4)

The resubmit mints from the new identity's minter (`engine.md` §24.5, `core.md` §7.5, R1-CORE-ENGINE-5 and -15).

§18.4: replace `When the new engine has joined (ENG-BOOT-003), the runtime sends `Resubmit { ticket, commands, from_device_no, origin_state }` (each command parsed with `parse_command`) and returns the `ResubmitReport`;` with:

When the new engine has joined (ENG-BOOT-003), the runtime sends `Resubmit { ticket, commands, from_device_no, origin_state }` (each command parsed with `parse_command`), with `origin_state` the **new** identity's current minter (the `origin_state` of its newest `cmd` outbox record, else `IdMinter::new` for its own `device_no` in the adapter's encoding), never the old identity's from `unsent.tdk`, which is kept but not used (`engine.md` §24.5, `core.md` §7.5); it returns the `ResubmitReport`, and from then on the facade's minter is the `origin_state` of the newest resubmitted record;

## Rejected

None. Parts of proposals are not adopted, each recorded in its change:
- core-engine-1: the code `engine/revoked` in refusal records (R1-CORE-ENGINE-2, -12). An own entry refused because its author was revoked already carries `seal/revoked_author`; §13.3's sentence meant that a revoked device keeps its records, and is reworded so.
- core-engine-2: making `core/bad_ctx` an ordinary refusal, `Ctx::from_at_ms` and the CORE-CTX-001 change. Narrowing `seal.md` §10.1's `at_ms` range (R1-CORE-ENGINE-1) refuses such entries once, at classification, on every device, with an existing code and no new function (see "For the lead", item 1).
- core-engine-3: calling `upgrade_rules` once per version. One call to `current.rules` is used, because §11.2 defines each version's migration as a pure function of the board, and a log can activate a later version without the ones between.
- core-engine-4: dropping `to_device_no` from `SM::rebind` (the toy's origin state does not carry a `device_no`, so the parameter stays).
- core-engine-6: the new `Meta` field `notices_through` (it is derived from the notices file, so `Meta` and its writes do not change).
- core-engine-8: ENG-EPOCH-004 and the §24.4 rewrite, which would override the conservative reading R1-SEAL-RELAY-8 and R1-SEAL-ENGINE-13 wrote (with the lead: "For the lead", item 2).
- core-engine-10: the core change. The engine's wording was the problem: an own entry's outbox record holds the same payload as its frame, so it cannot supply `op` when that payload does not parse (R1-CORE-ENGINE-12).
- core-engine-17: the separate history blob and the history in `Bootstrap::Join`. The earlier resolutions put the history inside the snapshot body (R1-SEAL-RELAY-9, -28, R1-SEAL-ENGINE-15); only core's limit wording follows (R1-CORE-ENGINE-11, and "For the lead", item 3).

## For the lead

1. **Where a far-future `at_ms` is refused (core-engine-2).** Option (a), written as R1-CORE-ENGINE-1, -3 and -13: `seal.md` §10.1 narrows `at_ms` to 0 to 253,402,300,799,999, the end of core's instant range. Such an entry is then refused at classification on every device with `seal/bad_plaintext`, and an alarm names the author. No byte layout changes, and core needs no refusal path. Option (b), the finding's: seal keeps 2^53 − 1, the adapter refuses with `core/bad_ctx`, and core redefines that code as an ordinary refusal. That costs a new function and makes the code mean two things. Recommended: (a). An honest clock never reaches year 10,000, and one owner (seal's plaintext check) decides it.
2. **S24, once the channel is ruled (core-engine-8; R1-SEAL-ENGINE's "For the lead", item 1).** Nothing here overrides the conservative reading. Whichever channel the lead picks, the engine then also needs this rule. While revoked, classification pauses at the first entry that returns `seal/missing_root`, and the log is kept. When the sealer learns the missed roots (`keys_changed`, D4), the engine clears the revoked state, then classifies and applies in seq order from the paused seq before it seals anything. The finding proposes this as ENG-EPOCH-004, the next free id of that topic. Recommended: rule channel (a) as R1-SEAL-ENGINE recommends, and add this rule with that ruling.
3. **The history inside a 64 MiB snapshot body (core-engine-17).** Core's 60 MiB board cap leaves about 4 MiB for the membership and the control history (R1-SEAL-RELAY's "For the lead", item 4). `seal.md` allows up to 1,048,576 history records per snapshot (R1-SEAL-RELAY-9), which far exceeds that. A board with a large history and a full board would therefore fail to seal a snapshot (and to pair) with the 64 MiB inflated limit. Real histories are small (a control entry is a few KB at most, and beats are not control entries). Recommended: accept, and have `seal_snapshot` refuse a body over 67,108,864 bytes with a named code, so the failure is reported rather than undefined. The alternative is to lower the board cap, which is a rules-version change (S18). Meanwhile core's limit wording names the history (R1-CORE-ENGINE-11).
