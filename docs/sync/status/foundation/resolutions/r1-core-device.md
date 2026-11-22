# Resolutions R1: core.md against device.md (14 findings)

Resolver 7 of 9. Inputs read: reviews.md §B, spec/README.md, foundation.md "Decisions" and "Rulings for the specs" (S1 to S31, with the settled S15, and the notes under the device draft's rulings), SYNC_PLAN.md §2 and §3, the earlier files `r1-seal-relay.md`, `r1-seal-engine.md`, `r1-core-engine.md`, `r1-engine-relay.md`, `r1-device-engine.md`, `r1-device-bridge.md` and the lead's `r1-lead.md` (L1 to L9) in full; core.md §3, §5 to §8, §13.1, §14 to §21; device.md §9, §13, §14, §15.3, §15.7, §15.8, §17.4, §17.5, §18.1, §18.2, §18.7, §21 to §26; seal.md §13.3 and §13.7; today's `src/subscriptions.rs`, `src/planner.rs` and `src/phone.rs` where the findings cite them.

Built on the earlier files and the lead's rulings (settled; not repeated here):
- core-device-2: R1-CORE-ENGINE-10 (core.md §8.3 and the §19 `device.md` bullet) and R1-CORE-ENGINE-23 (device.md §13.1.3, the paragraph "Removed and disabled subscriptions (synced mode)") already apply it. Only a clause and the overlay counter are added here (R1-CORE-DEVICE-6).
- core-device-5 (ruling S27): applied by R1-SEAL-RELAY-9, R1-SEAL-RELAY-28, R1-SEAL-ENGINE-15, R1-DEVICE-ENGINE-1, R1-CORE-ENGINE-11 and the lead's L4 and L9 (the history travels inside the snapshot body; the sealer fills and keeps it; `seal_snapshot` refuses a body over 67,108,864 bytes with a named code). Nothing further (R1-CORE-DEVICE-12).
- core-device-6 (ruling S29): applied by R1-SEAL-RELAY-10, R1-DEVICE-ENGINE-14 and R1-DEVICE-BRIDGE-1. Nothing further (R1-CORE-DEVICE-12).
- core-device-7: the 255-byte host is in seal.md by R1-SEAL-RELAY-1; device.md's `tokens` and `move_board` by R1-DEVICE-ENGINE-18. Only the remaining device.md places follow (R1-CORE-DEVICE-7).
- core-device-8: applied by R1-SEAL-RELAY-20, R1-SEAL-RELAY-27 and R1-DEVICE-ENGINE-15. Nothing further (R1-CORE-DEVICE-12).
- core-device-10 builds on device.md §17.5's snapshot row as R1-DEVICE-BRIDGE-1 item 3 left it.
- core-device-14: applied by R1-CORE-ENGINE-6 (core.md §13.1's `now` comment) and R1-CORE-ENGINE-16. Nothing further (R1-CORE-DEVICE-5).

New clause ids assigned here (later resolvers continue after them, and after those of the earlier files):
- device.md: DEV-ICS-010.
- core.md: none. Reworded: CORE-IMP-005.

New error codes: none. Changed public items: core.md every zone parameter accepts an unsized zone (`&(impl Zone + ?Sized)`, `Z: Zone + ?Sized`); `FetchStatus.at` becomes `chrono::DateTime<chrono::FixedOffset>`; device.md `Published::placements_for` takes an item id. No byte on the wire and no sealed-file layout changes; the serde form of `FetchStatus.at` stays what today's `subscribed_cache.json` holds.

Notation. "Replace A with B" means the exact text A, as it stands in the spec after the changes of the six earlier files, becomes B. Clause texts are given in full. A table row is given in full, in the table's own column order. A change that needs edits in two specs has the same id under both headings.

## core.md

### R1-CORE-DEVICE-1 (core-device-1)

The lead's note under the device draft's rulings: local-only mode mints with `device_no` 1, and the import accepts those ids. More generally the import keeps every device-minted id (high part 1 to 8191, low part at least 1), which local-only mode, "Export legacy" and the recovery tool write into today's files, and sets a mark for each high part it keeps, so `BoardData::check` rule 4 holds and the genesis device (`device_no` 1, `seal.md` §13.7) never mints such an id again. `L` is taken over the legacy space only, so temporaries and duplicates are still renumbered below 2^40. Matching edits: device.md, same id.

1. §7.1: replace `` `h = 0` is the legacy space: ids from today's data, below 2^40, kept unchanged by the import except where §15.3 renumbers them. `` with:

   `h = 0` is the legacy space: ids from today's data, below 2^40, kept unchanged by the import except where §15.3 renumbers them. Ids with `h ≥ 1` in today's files, written there by local-only mode (`device.md` §9.2), "Export legacy" (`device.md` §18.7) or the recovery tool (`device.md` §15.8), are kept unchanged by the import as well (§15.3 step 9.4).

2. §7.2, first paragraph: replace `So no id is ever issued twice, even after Forget, after a refused entry, or from a restored folder whose own counter went back.` with `So no id is ever issued twice, even after Forget, after a refused entry, or from a restored folder whose own counter went back. (Local-only mode stores no marks: it rebuilds them from today's files at each open, `device.md` §9.2.)`

3. §15.3, step 9: replace the whole step (from `9. **Ids.** Let `L` be the largest id below 2^62` to the end of its last line, `` `live_and_archived` (a warning; both are kept, and a later `restore` of such a row uses its fallback; V2-11). ``) with:

   9. **Ids.** Let `L` be the largest id below 2^40 among items, rows and subscriptions (0 when there is none), and `next = L + 1`. Then, each list in file order:
      1. an item whose id is 0, or equal to an earlier item's id, or at least 2^62 (a temporary id, `adopt_temporaries`, board.rs:697-711) gets `next`, and `next` increases;
      2. a row whose id is at least 2^62 gets `next` likewise; a row with id 0 keeps it (a pre-id row);
      3. a subscription whose id is 0, or equal to an earlier subscription's id, or at least 2^62 gets `next` likewise (today's seeding bug can leave two subscriptions with one id, R1 §2);
      4. an item, row or subscription id from 2^40 to 2^62 − 1 whose high part (`id div 2^40`) is from 1 to 8191 and whose low part (`id mod 2^40`) is at least 1 is **device-minted** (§7.1) and is kept unchanged; any other id from 2^40 to 2^62 − 1 (a high part of 8192 or more, which is an id of at least 2^53, or a low part of 0), or any `next` that reaches 2^40: fail `id_range`, listing the ids.
      Every renumbering is reported as `(list, old id, new id)`. Ids held both by a live item and by an archive row are reported as `live_and_archived` (a warning; both are kept, and a later `restore` of such a row uses its fallback; V2-11).

4. §15.3, step 14: replace `the mark of `device_no` 0 is `{item: the largest low part of every id now present, block: the last sid numbered}`, and no other marks;` with:

   the marks, ascending by `device_no`: for `device_no` 0, `{item: the largest low part among the item, row and subscription ids now present whose high part is 0, block: the last sid numbered}`; for each high part `h` from 1 to 8191 that some item, row or subscription id now present has, `{device_no: h, item: the largest low part among those ids with high part h, block: 0}`; no other marks, and a mark whose two numbers are 0 is not stored (§6.10);

   (The step then continues `` `notes_rev = 0`; no picture; … `` unchanged.)

5. Reword CORE-IMP-005 (full text):

   - **CORE-IMP-005** (core) `import` MUST renumber ids exactly as §15.3 step 9 states, MUST report each renumbering, MUST keep every device-minted id of step 9.4 unchanged, and MUST fail with `id_range` exactly when step 9.4 names an id or `next` reaches 2^40.

   (CORE-IMP-006, which binds the marks to step 14, is unchanged.)

6. §18.2 Families: replace the row `| `import.json` | §15 | a folder under `fixtures/` (today's format and `main`'s), the config values | genesis `board_sha256` and the report, or the failure |` with:

   `| `import.json` | §15 | a folder under `fixtures/` (today's format and `main`'s), or the file bytes given in the case; the config values | genesis `board_sha256` and the report, or the failure; cases with device-minted ids (items, rows and subscriptions with ids `2^40 + 3` and `5 × 2^40 + 1`, a duplicate device-minted item id, a temporary id at or above 2^62 next to them) and the marks step 14 gives them; `id_range` for an id with high part 8192 and for the id `3 × 2^40` |`

7. §19, `device.md` block: add after the bullet that begins `- Requires: for the import, the device or desktop reads the legacy files`:

   - Provides: `import` keeps every device-minted id and sets the mark of each high part it keeps (§15.3 steps 9.4 and 14), so the plain files that `device.md`'s local-only mode (§9.2), "Export legacy" (§18.7) and the recovery tool (§15.8) write read back with their ids unchanged.

### R1-CORE-DEVICE-2 (core-device-3)

`FetchStatus.at` cannot stay `DateTime<Local>` in a crate that forbids `chrono::Local` (CORE-GEN-004). It becomes `DateTime<FixedOffset>`, whose `serde` form (RFC 3339 with the offset) is the one today's `DateTime<Local>` writes and also reads, so `subscribed_cache.json` and `overlay.tdk` keep today's format in both directions. The device fills it from its clock and zone. Matching edits: device.md, same id.

1. §14.6, first paragraph: replace `the cities; weather.rs) are copied unchanged.` with:

   the cities; weather.rs) are copied unchanged, except that `FetchStatus.at` is `chrono::DateTime<chrono::FixedOffset>` (today `DateTime<Local>`, subscriptions.rs:337-339, a type CORE-GEN-004 forbids). Its `serde` form is RFC 3339 with the offset, which today's `DateTime<Local>` also writes and reads, so `subscribed_cache.json` keeps today's format in both directions. The caller fills it from its own clock and zone (`device.md` §13.1.2 step 8); none of the copied functions reads a clock (CORE-GEN-003).

2. §20 Public API: replace the line `pub mod overlay;   // OverlayEvent, Overlay, FetchOutcome, FetchStatus, Covered` with `pub mod overlay;   // OverlayEvent, Overlay, FetchOutcome, FetchStatus (at: chrono::DateTime<chrono::FixedOffset>, §14.6), Covered`.

### R1-CORE-DEVICE-3 (core-device-4)

The device's zone is a run-time choice (`ChronoZone<chrono::Local>` on desktops, a `TableZone` on Android) that it holds as `&dyn Zone` (`device.md` §25 `Published::zone()`, `store::write_legacy_files`). `impl Zone` and `Z: Zone` carry an implicit `Sized` bound, so every zone parameter of the core accepts an unsized zone. `Zone` is object-safe as declared (two `&self` methods, no generics), so the trait itself does not change, and no `Send + Sync` supertrait is added: the device keeps its zone as `Box<dyn Zone + Send + Sync>` itself. device.md needs one interface line (same id).

1. §5.2: replace `A device passes `ChronoZone(chrono::Local)` or a `TableZone`; `device.md` chooses.` with `A device passes `ChronoZone(chrono::Local)` or a `TableZone`; `device.md` chooses, at run time, and may pass it as `&dyn Zone`: every zone parameter of this crate is `&(impl Zone + ?Sized)` or a generic `Z: Zone + ?Sized` (§20).`

2. §13.1, the code block: replace the line `pub struct CompileCtx<'a, Z: Zone> {` with `pub struct CompileCtx<'a, Z: Zone + ?Sized> {`, and the line `pub fn compile<Z: Zone>(intent: &Intent, cx: &CompileCtx<Z>, minter: &mut IdMinter) -> Result<Compiled, CompileError>;` with `pub fn compile<Z: Zone + ?Sized>(intent: &Intent, cx: &CompileCtx<Z>, minter: &mut IdMinter) -> Result<Compiled, CompileError>;`. (The other lines of the block, including the `now` comment as R1-CORE-ENGINE-6 left it, are unchanged.)

3. §14.2, first paragraph: replace `a `zone: &impl Zone` parameter wherever today's code reads a local date, hour or minute;` with `a `zone: &(impl Zone + ?Sized)` parameter wherever today's code reads a local date, hour or minute (§5.2);`.

4. §14.3, the code block: replace the line `pub struct SnapshotInput<'a, Z: Zone> {` with `pub struct SnapshotInput<'a, Z: Zone + ?Sized> {`.

5. §20 Public API:
   - module `time`: in the four lines that begin `    pub fn local_day(`, `    pub fn local_minutes(`, `    pub fn resolve_on_day(` and `    pub fn now_marker(`, replace `z: &impl Zone` with `z: &(impl Zone + ?Sized)`;
   - module `compile`: replace `    pub struct CompileCtx<'a, Z: Zone> { /* §13.1 */ }` with `    pub struct CompileCtx<'a, Z: Zone + ?Sized> { /* §13.1 */ }`, and `    pub fn compile<Z: Zone>(intent: &Intent, cx: &CompileCtx<Z>, minter: &mut IdMinter) -> Result<Compiled, CompileError>;` with `    pub fn compile<Z: Zone + ?Sized>(intent: &Intent, cx: &CompileCtx<Z>, minter: &mut IdMinter) -> Result<Compiled, CompileError>;`;
   - module `read`: in each of the six signatures that take `zone: &impl Zone` (`ranked_ids`, `shuffle_seed`, `behind_minutes`, `next_free_start`, `reflow_inputs`, `archive_by_day`), replace `zone: &impl Zone` with `zone: &(impl Zone + ?Sized)`;
   - module `snapshot`: replace `    pub struct SnapshotInput<'a, Z: Zone> { /* §14.3 */ }` with `    pub struct SnapshotInput<'a, Z: Zone + ?Sized> { /* §14.3 */ }`, `    pub fn snapshot<Z: Zone>(input: &SnapshotInput<Z>) -> Snapshot;` with `    pub fn snapshot<Z: Zone + ?Sized>(input: &SnapshotInput<Z>) -> Snapshot;`, and `    pub fn answer_snapshot<Z: Zone>(input: &SnapshotInput<Z>) -> serde_json::Value;` with `    pub fn answer_snapshot<Z: Zone + ?Sized>(input: &SnapshotInput<Z>) -> serde_json::Value;`;
   - the comment line `// ---------- read side (§14), copied modules with the zone parameter`: replace it with `// ---------- read side (§14), copied modules with the zone parameter; every zone parameter, here and in planner, archive, ics and the snapshot builders, is &(impl Zone + ?Sized) or Z: Zone + ?Sized (§5.2)`.

6. §19, `device.md` block: replace the bullet `- Requires: the device supplies the zone (`ChronoZone(chrono::Local)` or a `TableZone`) and its overlay to compile and read calls, and keeps the overlay outside `BoardData`.` with:

   - Requires: the device supplies the zone (`ChronoZone(chrono::Local)` or a `TableZone`, which it may pass as `&dyn Zone`, §5.2) and its overlay to compile and read calls, and keeps the overlay outside `BoardData`.

### R1-CORE-DEVICE-4 (core-device-13)

§19, `device.md` block: replace the bullet `- I assume `device.md` stores the §10.7 records in the sealed file of kind 8 (`seal.md` §19.2) one record per append.` with:

- Relies on: `device.md` §6.4.5 carries each §10.7 record, at most 4,096 bytes, as the record of one `Add` op in the sealed file of kind 8 (`notices.tdk`, `seal.md` §19.2), up to 1,024 ops per sealed record.

### R1-CORE-DEVICE-5 (core-device-14)

Nothing further to apply. R1-CORE-ENGINE-6 already replaces §13.1's comment with `// the origin's own wall clock, in whole milliseconds, never corrected by a relay-derived offset (engine.md §27.3)` and the non-goal's "skew-corrected clock", and R1-CORE-ENGINE-16 deletes engine.md's matching assumption. That wording agrees with device.md §17.4 (`now` = the device clock in whole milliseconds).

## device.md

### R1-CORE-DEVICE-1 (core-device-1)

The import keeps device-minted ids and sets their marks (`core.md` §15.3 steps 9.4 and 14, CORE-IMP-005; same id), so the request to core and its open question go.

1. §9.2, the paragraph after the list: replace `These ids are `2^40 + n`, below 2^53, which today's files hold as plain integers; the import keeps them on the next `open` and at "Create board from this folder" (`core.md` change required, §24). Stamps, `notes_rev` and marks are not stored in today's files and are rebuilt at each `open` by the import; block ids are renumbered by it (`core.md` §15.3 step 10), which no caller observes across an `open`.` with:

   These ids are `2^40 + n`, below 2^53, which today's files hold as plain integers; the import keeps them on the next `open` and at "Create board from this folder" (`core.md` §15.3 step 9.4) and sets the mark of `device_no` 1 to the largest of them (step 14), so the genesis device, which is `device_no` 1 (`seal.md` §13.7), never mints one of them again. Stamps, `notes_rev` and marks are not stored in today's files and are rebuilt at each `open` by the import; block ids are renumbered by it (`core.md` §15.3 step 10), which no caller observes across an `open`. Because the marks are rebuilt from the files, an item or subscription number that was the highest one and has since been forgotten (`forget`, `forget_archived`, `remove_subscription`) may be minted again after a later `open`; nothing in the board refers to it then.

2. §18.7: replace `Today's build opens the folder; the import of this spec reads it back (§24).` with `Today's build opens the folder; the import reads it back with every device-minted id unchanged (`core.md` §15.3 step 9.4).`

3. §24, `core.md` block: replace the first bullet (the one that begins `- Requires: `legacy::import` (§15.3 step 9) keeps an id or sid whose high part is from 1 to 8191`) with:

   - Relies on: `legacy::import` keeps every device-minted id (high part 1 to 8191, low part at least 1) unchanged and sets the mark of each high part it keeps (`core.md` §15.3 steps 9.4 and 14, CORE-IMP-005), so the plain files that local-only mode (§9.2), "Export legacy" (§18.7) and the recovery tool (§15.8) write read back through the import.

4. §24, `core.md` block, the `Assumes` bullet: replace `works on a board whose marks hold only the legacy space (§9.2);` with `works on a board whose marks hold the legacy space and the mark of `device_no` 1 that the import rebuilt from the plain files (§9.2);`.

5. §26, open question 2: replace the whole item with:

   2. **Local-only mode writes today's files** and mints with `device_no` 1 (§9), so that today's build keeps opening the folder and "Create board from this folder" reads one format. The import keeps such ids and sets their mark (`core.md` §15.3 steps 9.4 and 14), as the lead noted under the device draft's rulings (`docs/sync/status/foundation.md`). Decided there.

### R1-CORE-DEVICE-2 (core-device-3)

The device fills `FetchStatus.at`, now `chrono::DateTime<chrono::FixedOffset>` (`core.md` §14.6, same id), from its clock and zone.

1. §13.1.2, step 8: replace `on any failure keep the previous events and record `FetchOutcome::Failed { why }`.` with `on any failure keep the previous events and record `FetchOutcome::Failed { why }`. In both cases `FetchStatus.at` is the device clock (`wall_ms`) as a `chrono::DateTime<chrono::FixedOffset>` whose offset is the device zone's `offset_at` of that instant (`core.md` §14.6), where today's code takes `Local::now()`.`

2. §6.4.10: replace `Overlay: `serde_json` compact bytes of `taskdeck_core::overlay::Overlay` (`core.md` §14.6), at most 16,777,216 bytes.` with `Overlay: `serde_json` compact bytes of `taskdeck_core::overlay::Overlay` (`core.md` §14.6), at most 16,777,216 bytes; `FetchStatus.at` is RFC 3339 with its offset, as in today's `subscribed_cache.json`.`

3. §24, `core.md` block: replace the bullet `- Requires: `Overlay` and `weather::Report` keep `serde` derives so the store can write them (§6.4.10, §6.4.12), and the overlay's `FetchStatus.at` is an instant the device can produce from its clock.` with:

   - Requires: `Overlay` and `weather::Report` keep `serde` derives so the store can write them (§6.4.10, §6.4.12). Relies on: `FetchStatus.at` is a `chrono::DateTime<chrono::FixedOffset>` (`core.md` §14.6), which the device fills as §13.1.2 step 8 states.

### R1-CORE-DEVICE-3 (core-device-4)

Every zone parameter of the core accepts `&dyn Zone` (`core.md` §5.2, §20, same id); §25 does not change.

§24, `core.md` block, the `Provides` bullet: replace `Provides: the zone (`ChronoZone(chrono::Local)` on desktops, a `TableZone` on Android, §16),` with `Provides: the zone (`ChronoZone(chrono::Local)` on desktops, a `TableZone` on Android, §16), passed as `&dyn Zone` (`Published::zone()`, §25), which every zone parameter of the core accepts (`core.md` §20),`.

### R1-CORE-DEVICE-6 (core-device-2)

R1-CORE-ENGINE-23's paragraph in §13.1.3 gets the overlay counter, so the interfaces redraw, and a clause, so the rule has a test.

1. §13.1.3, the paragraph **Removed and disabled subscriptions (synced mode).** (as R1-CORE-ENGINE-23 wrote it): replace `, and writes `overlay.tdk` when the overlay changed.` with `, writes `overlay.tdk` when the overlay changed, and then raises the overlay counter.` (The rest of the paragraph, `A subscription that comes back … by the usual schedule.`, is unchanged.)

2. §13.1.4: add after DEV-ICS-009:

   - **DEV-ICS-010** (device) In synced mode, after each `ViewChanged`, the device MUST remove from its overlay the events and statuses of every subscription id that is absent from the new pending view or has `enabled: false` there, and MUST write `overlay.tdk` and raise the overlay counter when that changed the overlay.

### R1-CORE-DEVICE-7 (core-device-7)

The remaining places where device.md takes a host, after R1-SEAL-RELAY-1 (seal.md §5.4 and SEAL-ENC-006: at most 255 bytes, `:port` included) and R1-DEVICE-ENGINE-18 (`tokens`, `move_board`).

1. §18.1, step 1: replace `` `relay_host` a host of `seal.md` §5.4; `` with `` `relay_host` a host of `seal.md` §5.4 (at most 255 bytes, `:port` included), else `relay/address`; ``.

2. §15.3.1, the table: replace the row `| `relay_host` | string | required | `seal.md` §5.4 host | the writer's relay target |` with `| `relay_host` | string | required | a `seal.md` §5.4 host, 1 to 255 bytes, `:port` included | the writer's relay target |`.

3. §22 Limits: add this row directly after the row `| notes draft | 1,048,576 | bytes | DEV-STORE-005 |`:

   `| host (`device.tdk` `tokens`, `bootstrap.tdk`, backup `meta`) | 1 to 255 | bytes, `:port` included (`seal.md` §5.4, SEAL-ENC-006) | DEV-STORE-005 |`

### R1-CORE-DEVICE-8 (core-device-9)

Conservative reading, pending the lead ("For the lead", item 1): core keeps the genesis caps of ruling S18, and a local-only `open` of a folder over one of them fails with the report, which names the ids; today's build still opens that folder. Nothing in core changes.

1. §9.2, step 2: replace the last bullet, `   - any other failure: `open` fails with `device/legacy_unreadable` and the report's failure kind, and writes nothing.`, with:

   - any other failure (`too_large`, `id_range`, `instant_range`, `field_range`, `cap`, `invariant`): `open` fails with `device/legacy_unreadable` and the import report, whose failure names its kind and the ids involved, and writes nothing. The desktop shows the report, so that the user can repair those items in today's build, which still opens the folder (§26 open question 23).

2. §26: add after item 22:

   23. **A folder over a cap of rules version 1 does not open in local-only mode** (§9.2 step 2). Local-only mode runs the genesis import at every `open`, and the import refuses a board over a cap or field range of `core.md` §17 (ruling S18), for example more than 10,000 live items or a name over 4,096 bytes. Default: `open` fails with `device/legacy_unreadable` and the report, which names the ids, and the user repairs them in today's build. The alternative, a lenient import that only reports such violations for local-only use, is with the lead.

### R1-CORE-DEVICE-9 (core-device-10)

The snapshot's `Look` and `Dials` are built from the published picture and the device's dials with today's formats (phone.rs:260, 1665: `top` is `#rrggbb`, `aspect` is 720 / 1560). Builds on the row as R1-DEVICE-BRIDGE-1 item 3 left it.

§17.5, the table, row `snapshot(from, days, now, ranked)` and `answer_snapshot(…)`: replace its "Wraps" cell with:

`snapshot::snapshot` with this view's overlay, `version`, `look` and `dials`: `look` is `Some(Look { id, aspect: 720.0 / 1560.0, top: format!("#{:02x}{:02x}{:02x}", top[0], top[1], top[2]) })`, `id` and `top` those of the `PictureView` that `picture()` returns (the phone's own picture while it has one, §14.3, else the shared one), and `None` when `picture()` returns `None`; `dials` is `Dials { blur, light }` from `device.tdk`'s `look` (§16)

(The row's first and last cells are unchanged.)

### R1-CORE-DEVICE-10 (core-device-11)

`planner::placements_for` takes one item (`core.md` §14.2, today's `placements_for(item, day)`, planner.rs:258); the facade's wrapper takes its id.

1. §17.5, the table: replace the row `| `placements_for(day)` and the other planner functions that take a day | `planner::…` | D |` with:

   `| `placements_for(item, day)` and the other planner functions that take a day | `planner::placements_for` with the published board's live item of id `item` and the device zone (an empty result when no live item has that id); the others `planner::…` | D |`

2. §25 Public API, `impl Published`: replace `    pub fn placements_for(&self, day: chrono::NaiveDate) -> Vec<taskdeck_core::planner::DayPlacement>;` with `    pub fn placements_for(&self, item: u64, day: chrono::NaiveDate) -> Vec<taskdeck_core::planner::DayPlacement>;   // empty when item is not live`.

### R1-CORE-DEVICE-11 (core-device-12)

§9.3, step 6: replace `` 6. `settings` with a `home` change: write `` with `` 6. `settings`, when the clone's `home` differs from the board's `home` before the apply: write ``. (The rest of the step is unchanged; core's `ChangeSet` stays as it is.)

### R1-CORE-DEVICE-12 (core-device-5, core-device-6, core-device-8)

Nothing further to apply.
- core-device-5 (ruling S27): R1-SEAL-RELAY-9 puts the control history into the snapshot body (seal.md §20.4, SEAL-BLOB-006, SEAL-BLOB-007); R1-SEAL-RELAY-28 rewrites device.md §15.7, removes the history blob from §18.3, writes `history.tdk` from the bootstrap snapshot, deletes §24's bundle request and rewrites open question 6; R1-DEVICE-ENGINE-1 adds the rejoin path (DEV-RUN-006); R1-CORE-ENGINE-11 rewords core.md §17's board-bytes reason; the lead's L4 has `seal_snapshot` refuse a body over 67,108,864 bytes with a named code. The finding's `vb32(history)` in the kind-17 Join layout and its §24 seal bullet are not adopted: the sealer fills and keeps the history (R1-SEAL-ENGINE-15, L4, L9).
- core-device-6 (ruling S29): kind 19, §6.4.14, §14.3, DEV-PIC-005, the facade rows and signatures and `KIND_OWN_PICTURE` are R1-DEVICE-ENGINE-14; the snapshot's look, the look ids and `clear_own_picture`'s dials are R1-DEVICE-BRIDGE-1; seal.md §19.2 is R1-SEAL-RELAY-10.
- core-device-8: relay.md's `read_epoch` in the `relay/stale_epoch` body is R1-SEAL-RELAY-20 (RELAY-RTOKEN-004); device.md §15.8 step 1 (epoch 1, then once with the refusal's `read_epoch`) and open question 7 are R1-SEAL-RELAY-27; the §22 row ("recovery read-token mints | 2") is R1-DEVICE-ENGINE-15. The finding's removal of that row is not adopted (the row states the bound DEV-BKP-011's flow keeps).

## Rejected

None. Parts of proposals are not adopted, each recorded in its change:
- core-device-1: the new sentence in core.md §7.2 that a forgotten id may be minted again in local-only mode; it is a property of local-only mode and goes into device.md §9.2 (one owner), with a pointer in core.md §7.2 (R1-CORE-DEVICE-1).
- core-device-2: the finding's "after every loop turn that published a new board" and the local-only half; R1-CORE-ENGINE-23's paragraph (after each `ViewChanged`, synced mode; local-only mode keeps §9.3 step 7) is kept, and only the counter and DEV-ICS-010 are added (R1-CORE-DEVICE-6). core.md §19 is already reworded by R1-CORE-ENGINE-10.
- core-device-3: parameters `now` and `zone` for copied functions that call `Local::now()`; none of the functions core.md §14.6 copies calls it (the fetch in subscriptions.rs:594-610 stays with the device), so only the type changes (R1-CORE-DEVICE-2).
- core-device-4: `pub trait Zone: Send + Sync`; the device holds its zone as `Box<dyn Zone + Send + Sync>` itself, and a supertrait would add a bound on `ChronoZone<Z>` for no caller of the core (R1-CORE-DEVICE-3).
- core-device-5, core-device-6, core-device-8, core-device-14: already applied by the earlier files (R1-CORE-DEVICE-5, -12).
- core-device-7: the seal.md change (R1-SEAL-RELAY-1) and the `tokens` and `move_board` cells (R1-DEVICE-ENGINE-18); the §22 row's enforcing clause is DEV-STORE-005, whose `vb8` lengths are what the 255 bytes protect.
- core-device-9: the change to foundation.md's "To tell the owner at exit", which is the lead's file, not a spec (see "For the lead", item 1).

## For the lead

1. **Local-only mode refuses a folder over a cap of rules version 1 (core-device-9).** Local-only mode runs the genesis import at every `open` (device.md §9.2), and the import refuses a board over a cap or field range of core.md §17 (ruling S18: 10,000 live items, 50,000 live blocks, 200,000 archive rows, names of 4,096 bytes, URLs of 8,000 bytes, notes of 1 MiB). Such a folder then opens only in today's build. Option (a), written as the change (R1-CORE-DEVICE-8): core keeps its caps; the local-only `open` fails with `device/legacy_unreadable` and the report, which names the ids, and the user repairs the folder in today's build; device.md open question 23 records it. Option (b): a core `import_lenient` that reports the violations and keeps the board, for local-only use only; a genesis still requires the caps. (b) keeps more folders opening, but it adds a second import path and boards that `BoardData::check` refuses inside the running app, and every later `apply` would have to accept them. Recommended: (a), and add "local-only mode refuses a folder over a rules-version-1 cap; the user repairs it in today's build" to foundation.md "To tell the owner at exit", next to S18.
