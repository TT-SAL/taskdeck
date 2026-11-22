# Resolutions R1: device.md against bridge.md (16 findings)

Resolver 6 of 9. Inputs read: reviews.md §B, spec/README.md, foundation.md "Decisions" and "Rulings for the specs" (S1 to S31, with the settled S15), SYNC_PLAN.md §2 and §3, the earlier files `r1-seal-relay.md`, `r1-seal-engine.md`, `r1-core-engine.md`, `r1-engine-relay.md`, `r1-device-engine.md` and the lead's `r1-lead.md` (L1 to L5); device.md §1 to §8, §10, §12 to §18, §21 to §26; bridge.md §1 to §7, §11 to §14, §17, §18, §20 to §22; engine.md §37 (`NoticeEntry`, `UserAction`).

Built on the earlier files (settled; not repeated here):
- device-bridge-1 (ruling S29): kind 19 `own-picture.tdk`, its row, layout and plaintext (§6.4.14), §14.3, DEV-PIC-005, the two facade rows and signatures, `KIND_OWN_PICTURE`, the §22 limit row, the §7.2 and §7.4 rows, seal.md §19.2 are R1-DEVICE-ENGINE-14 and R1-SEAL-RELAY-10. Only what is still missing follows (R1-DEVICE-BRIDGE-1): the snapshot's look, the shared look's dials after a clear, the returned look id, and bridge.md.
- device-bridge-11 (ruling S27) is applied by R1-SEAL-RELAY-9, R1-SEAL-RELAY-28 and R1-DEVICE-ENGINE-1 (the history travels in the snapshot body; §15.7, §18.3, §24 and open question 6 rewritten). The Join layout gains no history (earlier decision kept). S27 joins §1 in R1-DEVICE-BRIDGE-10.
- device-bridge-12 is applied by R1-SEAL-RELAY-20, R1-SEAL-RELAY-27 and R1-DEVICE-ENGINE-15 (`read_epoch` in the refusal body, §15.8 step 1, open question 7, the §22 row at 2 tries).
- device-bridge-6 builds on R1-DEVICE-ENGINE-4 (the `adopt` rows of device.md §17.7 and bridge.md §11.4).

New clause ids assigned here (later resolvers continue after them, and after those of the earlier files):
- device.md: DEV-FACADE-008, DEV-FACADE-009, DEV-NET-017. Withdrawn: DEV-KEY-005.
- bridge.md: BRIDGE-PROC-008, BRIDGE-EVT-005, BRIDGE-KEY-005.

Reworded clauses: DEV-GEN-001, DEV-THREAD-001, DEV-FILE-011, DEV-FACADE-003, BRIDGE-MSG-013.

New error code: `device/bad_place` (device.md §21). Changed meaning: `device/identity_active`. New public items: device.md `Device::weather_place`, `Device::identities`, `StoredIdentity`, `NoticeRecord`; `NoticeView` becomes a struct; `Published::weather` gains the source; `set_look_dials`, `set_own_picture`, `clear_own_picture` return the look id; bridge.md `CoreState.Open` loses its mode. No byte on the wire and no sealed-file layout changes.

Notation. "Replace A with B" means the exact text A, as it stands in the spec after the changes of the five earlier files, becomes B. Clause texts are given in full. A table row is given in full, in the table's own column order. A change that needs edits in two specs has the same id under both headings.

## device.md

### R1-DEVICE-BRIDGE-1 (device-bridge-1, device-bridge-9)

Ruling S29, completed on top of R1-DEVICE-ENGINE-14: the snapshot's `look` follows the picture shown, clearing the own picture shows the shared one with the current dials, and every call that changes the picture shown returns, after the publish, the id of the look now shown, which the bridge's replies carry. Matching edits: bridge.md, same id.

1. §14.2, the last paragraph (as R1-DEVICE-ENGINE-14 item 10 left it): replace `` `set_look_dials(blur, light)` (§17.12) re-makes step 7 from the stored clean picture, or §14.3's look while the phone shows its own picture. `` with:

   `set_look_dials(blur, light)` (§17.12) stores the dials in `device.tdk`'s `look`, re-makes step 7 from the stored clean picture (or §14.3's look while the phone shows its own picture), rewrites that picture's file, publishes, and then returns the id (step 8) of the look now shown, or none when no picture is shown.

2. §14.3 (as R1-DEVICE-ENGINE-14 item 9 wrote it), the paragraph after the five steps: replace `` `clear_own_picture()` removes `own-picture.tdk` and publishes. `` with:

   `clear_own_picture()` removes `own-picture.tdk`; when the `blur` and `light` of `picture.tdk` differ from `device.tdk`'s `look`, it re-makes that picture's look as §14.2 step 7 and rewrites `picture.tdk`; then it publishes. `set_own_picture` and `clear_own_picture` each return, after the publish, the id (§14.2 step 8) of the look now shown: the own picture's after `set_own_picture`, the shared picture's or none after `clear_own_picture`. The publish raises `Changes.picture` (DEV-NOTIFY-001).

3. §17.5, the table, row `snapshot(from, days, now, ranked)` and `answer_snapshot(…)`: replace `` the picture's `Look { id, aspect: 720/1560, top }` and the dials `` with `` the `Look { id, aspect: 720/1560, top }` of `picture()` (the phone's own picture while it has one, §14.3, else the shared one) and the dials ``.

4. §17.12, the table: replace the row `| `set_look_dials(blur, light)` | D, A | `()` | — | — | short | all |` with:

   `| `set_look_dials(blur, light)` | D, A | `Option<String>`: the id of the look now shown (§14.2 step 8), returned after the new look is published; `None` when no picture is shown | — | — | long: at most 5,000 ms | all |`

5. §17.12, the table: replace the two rows R1-DEVICE-ENGINE-14 item 12 added with:

   `| `set_own_picture(bytes)` | A | `Option<String>`: the id of the own picture's new look, returned after it is published (§14.3) | `device/picture_too_large`, `device/picture_format`, `device/picture_decode`, `device/needs_sync` | — | long: at most 120,000 ms | Android |`
   `| `clear_own_picture()` | A | `Option<String>`: the id of the shared picture's look now shown, or `None` when the board has none, returned after the publish (§14.3) | `device/needs_sync` | — | short | Android |`

6. §25 Public API, `impl Device`: replace `    pub fn set_look_dials(&self, blur: u8, light: u8) -> Result<(), DeviceError>;` with `    pub fn set_look_dials(&self, blur: u8, light: u8) -> Result<Option<String>, DeviceError>;   // the id of the look shown`, and replace the two lines R1-DEVICE-ENGINE-14 item 16 added with:

   ```rust
       pub fn set_own_picture(&self, bytes: Vec<u8>) -> Result<Option<String>, DeviceError>;   // §14.3, Android
       pub fn clear_own_picture(&self) -> Result<Option<String>, DeviceError>;
   ```

7. §24, `bridge.md` block: replace the bullet R1-DEVICE-ENGINE-14 item 15 wrote (`- Provides: `set_own_picture` and `clear_own_picture` (§14.3, §17.12) for `POST /api/background`; nothing else to the facade.`) with:

   - Provides: `set_own_picture`, `clear_own_picture` and `set_look_dials`, each returning the id of the look now shown (§14.3, §17.12); `NoticeView` with `seq`, `index` and `dismissed` (§17.5, §25); a `set_notes` from `submit_json` kept as the draft (DEV-FACADE-008); `weather_place()` and the report's source (§17.12, §25); `identities()` (§17.2); the mode changed in place (DEV-FACADE-009); Android starting out of the foreground (DEV-NET-017); the key-store threads (§12.1).

   (If the lead drops one of R1-DEVICE-BRIDGE-2, -3, -4, -7, -8 or -9, the fixer drops its part of this bullet.)

### R1-DEVICE-BRIDGE-2 (device-bridge-2)

A published notice keeps the engine's `seq`, `index` and `dismissed`, so the bridge sends exactly the pair `dismiss_notice` takes and leaves dismissed records out. Matching edits: bridge.md, same id.

1. §17.5, the table: replace the row `| `notices()` | `Engine::notices()` decoded as `core.md` §10.7 records | D, A |` with:

   `| `notices()` | `Engine::notices()` in its order, each `NoticeEntry`'s `seq`, `index` and `dismissed` copied unchanged and its `record` decoded as `core.md` §10.7 (a `Notice` or a `RefusalRecord` by its `kind`); empty in local-only mode | D, A |`

2. §17.7, the table: replace the row `| `dismiss_notice(seq, index)` | D, A | `User(DismissNotice)` | — | — | short | all |` with:

   `| `dismiss_notice(seq, index)` | D, A | `User(DismissNotice { seq, index })`, with the `seq` and `index` of a `NoticeView` (§17.5) | — | — | short | all |`

3. §25 Public API: replace `pub enum NoticeView { Notice(taskdeck_core::apply::Notice), Refusal(taskdeck_core::apply::RefusalRecord) }` with:

   ```rust
   pub struct NoticeView { pub seq: u64, pub index: u32, pub dismissed: bool, pub record: NoticeRecord }   // engine.md NoticeEntry
   pub enum NoticeRecord { Notice(taskdeck_core::apply::Notice), Refusal(taskdeck_core::apply::RefusalRecord) }
   ```

### R1-DEVICE-BRIDGE-3 (device-bridge-3)

A `set_notes` that the page sends as a verb becomes the facade's draft, as if it had been flushed, so `notes_merge`, `resolve_notes` and `discard_notes_draft` work unchanged for it and `mine` is the refused text. Matching edits: bridge.md, same id.

1. §17.6: add this paragraph after the paragraph that begins `**Saving.**`, before DEV-FACADE-005:

   **A `set_notes` from `submit`.** In synced mode, a `set_notes` that reaches `submit`, `submit_named` or `submit_json` (the phone page's notes sheet sends it as a verb, `bridge.md` §6.3) is a flush: once the compile step has succeeded and before the `Enqueue`, the facade sets its draft to the compiled command's text, with `base_rev` the `base` the intent carried (the pending view's `notes_rev` when it carried none) and `edited_ms` the compile step's `now`, and writes `draft.tdk`. The draft then has no unflushed change, and it is kept, offered in `notes_merge` and removed exactly as a flushed draft is.

2. §17.6: add after DEV-FACADE-006:

   - **DEV-FACADE-008** (device) In synced mode, before it sends the `Enqueue` of a `set_notes` compiled from `submit`, `submit_named` or `submit_json`, the facade MUST make that command's text its notes draft with the `base_rev` and `edited_ms` of §17.6, MUST write `draft.tdk`, and MUST then treat the draft as flushed by that submit.

### R1-DEVICE-BRIDGE-4 (device-bridge-4)

The facade gives the weather place back, choosing the home place forgets the phone's own place, and the published report says which place it was fetched for. Matching edits: bridge.md, same id. (§13.2's rounding sentence is reworded by R1-DEVICE-BRIDGE-5.)

1. §4.4, the paragraph that begins `` `Published` holds: ``: replace `the weather report, the processed picture,` with `the weather report with the place it was fetched for, the weather place of §16 (`weather_source` and `own_place`), the processed picture,`.

2. §13.2, the bullet **Result.**: replace `` written to `weather.tdk` (synced) or kept in memory (local-only, as today), and published; `` with `` written to `weather.tdk` (synced) or kept in memory (local-only, as today), in both cases with the place's source (§6.4.12), and published; ``.

3. §16, the `device.tdk` / `device.json` table: replace the row `` | `own_place` | object `{"lat": int[-9000..9000], "lon": int[-18000..18000]}` | optional | Android only | the phone's own place (§13.2) | `` with:

   `| `own_place` | object `{"lat": int[-9000..9000], "lon": int[-18000..18000]}` | optional | Android only; present exactly while `weather_source` is `"own"` | the phone's own place (§13.2) |`

4. §17.5, the table, row `overlay()`, `weather()`, `picture()` (as R1-DEVICE-ENGINE-14 item 11 left it): replace `` the latest report with its `fetched_ms`, `` with `` the latest report with its `fetched_ms` and the `WeatherSource` it was fetched for (`weather.tdk`'s `source`, §6.4.12, or the in-memory round's in local-only mode), ``.

5. §17.12, the table: replace the row `| `set_weather_place(source, own_place)` | A | `()` | — | — | short | Android |` with these two rows, in this order:

   `| `weather_place()` | A | `(WeatherSource, Option<Home>)`: `weather_source` and `own_place` of `device.tdk` (or `device.json`), from the published view | — | — | read | Android |`
   `| `set_weather_place(source, own_place)` | A | `()`: `Home` clears `own_place` and sets `weather_source` to `"home"`; `Own` stores the given place and sets `"own"`; the weather thread then fetches at once (§13.2) | `device/bad_place` (`Own` with no place, or a place outside −9,000 to 9,000 and −18,000 to 18,000) | — | short | Android |`

6. §21 Error catalogue: add this row directly after the row `device/needs_sync`:

   `| `device/bad_place` | `set_weather_place` with `Own` and no place, or a place out of range | §17.12 | the caller has a bug |`

7. §25 Public API, `impl Device`, block `// ---------- picture, settings, export (§17.12)`: directly before the line that begins `    pub fn set_weather_place(` add the line `    pub fn weather_place(&self) -> (WeatherSource, Option<taskdeck_core::board::Home>);   // read`.

8. §25 Public API, `impl Published`: replace `    pub fn weather(&self) -> Option<(&taskdeck_core::weather::Report, i64)>;      // (report, fetched_ms)` with `    pub fn weather(&self) -> Option<(&taskdeck_core::weather::Report, i64, WeatherSource)>;   // (report, fetched_ms, the place it is for)`.

### R1-DEVICE-BRIDGE-5 (device-bridge-5, device-bridge-13, device-bridge-15)

The Android key store's Android-side rules have one owner, `bridge.md`: `AndroidKeyStore` writes `ld.wrap` itself, creating the directories it needs, outside the store; the threads each key-store method runs on are stated here once; DEV-KEY-005 and the Android half of DEV-FACADE-003 give way to BRIDGE-KEY-001, BRIDGE-KEY-004 and BRIDGE-AUTH-001; the own place's rounding is the shell's. Matching edits: bridge.md, same id.

1. §6.1, the layout: replace the line `      ld.wrap                         Android only: the Keystore wrap of L_d (§12.5)` with `      ld.wrap                         Android only: the Keystore wrap of L_d, written by AndroidKeyStore, not by the store (§12.5)`.

2. §7.2, the table, row **replace**: replace `` `store.tds`, `ld.wrap`, and the rewrites of kinds 5 and 8 `` with `` `store.tds`, and the rewrites of kinds 5 and 8 ``.

3. §7.4, first paragraph: replace `First it removes every `*.tmp` file of the directory: a replace that did not reach its rename left the old file current.` with:

   First it removes every `*.tmp` file of the directory: a replace that did not reach its rename left the old file current. `ld.wrap` (Android) is the key store's (§12.5): the store neither opens it nor treats it as corrupt, and a leftover `ld.wrap.tmp` is removed with the other `*.tmp` files.

4. §4.2: reword DEV-THREAD-001 (full text):

   - **DEV-THREAD-001** (device) The engine, the local-only board and every file of the data folder's `sync/` directory and of the plain files MUST be accessed only from the sync thread, except the backup thread's writes into destinations, the facade's read of published views (§4.4), and the key store's own calls, which run on the threads §12.1 names and of which only `AndroidKeyStore` touches a file under `sync/` (`ld.wrap`, §12.5).

5. §12.1: add this paragraph directly after the error table and before DEV-KEY-001:

   **Threads.** `Device::open` calls `load` on its calling thread, before it starts the sync thread. Every other key-store call runs on the sync thread: `create` (§18.1 step 7, §18.3), `delete` (`forget_identity`, `abandon_create`), and any `load` the running facade makes when it activates another identity (§17.2). A key store therefore works from any thread; `AndroidKeyStore` attaches each calling thread to the JVM itself (`bridge.md` §11.5, BRIDGE-KEY-005).

6. §12.5: replace the whole paragraph under the heading and the clause DEV-KEY-005 with:

   `AndroidKeyStore`, in `taskdeck-android`, implements the trait (`kind() = 3`) by calling the shell's key vault, which holds an AES-256-GCM key of the Android Keystore under the alias `taskdeck.ld.<board hex>.<dev hex>`, **not** bound to user authentication, so that jobs and reminders run while the phone is locked (SYNC.md §5.6). The vault wraps `L_d` with `AD_L` as associated data into 60 bytes, `iv[12] ‖ ciphertext[32] ‖ tag[16]`, which `AndroidKeyStore` keeps as `ld.wrap` in the identity directory. It writes and removes that file itself, outside the store and its file layer (DEV-THREAD-001): `create` first creates `sync/` and the identity directory when they are missing (`create_dir`, then `sync_dir` of its parent, for each), then writes `ld.wrap.tmp`, `sync`, `rename` to `ld.wrap`, `sync_dir`; `load` reads it; `delete` removes the alias, then the file, a missing file being no error. A missing alias or file is `NotFound`; a wrap that does not open (`AEADBadTagException`, `KeyPermanentlyInvalidatedException`) is `CannotUnwrap`; a Keystore not yet available is `Unavailable`. The key's parameters, the vault's calls, the status mapping and the threading are `bridge.md`'s (§11.5, §12.5; BRIDGE-KEY-001, BRIDGE-KEY-004, BRIDGE-KEY-005); the app's backup and transfer exclusions are the shell's (`bridge.md` §12.3).

   - **DEV-KEY-005** (withdrawn) Replaced by BRIDGE-KEY-001 and BRIDGE-KEY-004 (`bridge.md` §11.5, §12.5), which state the Android Keystore wrap of `L_d` for the stages that build it.

7. §13.2, the bullet **Which place.**: replace `rounded half away from zero from the platform's value,` with `as the shell rounded it half away from zero and passed it to `set_weather_place` (`bridge.md` §6.4.5 step 3, BRIDGE-ROUTE-004),`.

8. §17.1, the bullet **Auth**: replace `BIOMETRIC_STRONG with no PIN fallback on Android, SYNC.md §5.3, V1-5) immediately before the call; the device cannot check it, so the duty is the caller's (DEV-FACADE-003).` with `BIOMETRIC_STRONG with no PIN fallback on Android, SYNC.md §5.3, V1-5) immediately before the call; the device cannot check it, so the duty is the caller's (DEV-FACADE-003 on desktops, BRIDGE-AUTH-001 on Android).`

9. §17.4: reword DEV-FACADE-003 (full text):

   - **DEV-FACADE-003** (desktop) The desktop app MUST run device authentication (Windows Hello or Touch ID) immediately before every facade call that §17 marks "auth"; on Android the same duty is BRIDGE-AUTH-001's.

10. §18.1, step 7: replace `` `KeyStore::create(L_d)`; create the identity directory with `` with `` `KeyStore::create(L_d)`; create the identity directory (when the key store has not, §12.5) with ``.

11. §18.3, paragraph **Persisting the join**: replace `` `KeyStore::create(L_d)`; the identity directory with `` with `` `KeyStore::create(L_d)`; the identity directory (created when the key store has not, §12.5) with ``.

12. §24, `bridge.md` block, the `Requires` bullet: replace `implements the Keystore wrap and unwrap of §12.5 for `AndroidKeyStore`;` with `implements `AndroidKeyStore` (§12.5), which writes `ld.wrap` itself and runs on the threads of §12.1 (BRIDGE-KEY-001, BRIDGE-KEY-004, BRIDGE-KEY-005); runs device authentication before "auth" calls (BRIDGE-AUTH-001);`.

13. §24, **Stage `android`.**: replace `Requires: BIOMETRIC_STRONG before "auth" calls;` with `Requires: BIOMETRIC_STRONG before "auth" calls (BRIDGE-AUTH-001);`.

### R1-DEVICE-BRIDGE-6 (device-bridge-6)

Ruling S30: every call marked "auth" is offered on the phone only from API 30, so the facade's own gate (DEV-FACADE-007) refuses it below. `forget_identity`'s row is given in full by R1-DEVICE-BRIDGE-8, with the same Platforms value. Matching edits: bridge.md, same id.

1. §17.1, the bullet **Platforms**: append at its end: ` On Android every call marked "auth" is offered only from API 30 (ruling S30), and its row says so.`

2. §17.7, the table: replace the row `| `use_this_copy()` | D, A | `User(UseThisCopy)` | — | auth | short | all |` with `| `use_this_copy()` | D, A | `User(UseThisCopy)` | — | auth | short | desktops; API 30 |`.

3. §17.7, the table: replace the row R1-DEVICE-ENGINE-4 item 1 wrote for `adopt` with:

   `| `adopt(dev, typed_fp)` | D, A | `Op(Adopt { dev, typed_fp })`: adopting a state by fingerprint (V1-10); returns a ticket | `engine/op_busy`; through `wait_op`: `engine/fingerprint`, `engine/adopt_membership`, the open and load codes of `engine.md` §22.3 step 3, `engine/op_timeout` | auth | short | desktops; API 30 |`

4. §17.7, the table: replace the row `| `skip(seq)` | D, A | `Op(Skip { seq })`: the halted view's Skip | `engine/skip_target`, `engine/op_busy` | auth | short | all |` with `| `skip(seq)` | D, A | `Op(Skip { seq })`: the halted view's Skip | `engine/skip_target`, `engine/op_busy` | auth | short | desktops; API 30 |`.

### R1-DEVICE-BRIDGE-7 (device-bridge-7, device-bridge-8)

A flow that changes the active identity changes the mode in place: every `Device` handle stays valid and `mode()`, the status and the change counters show the new mode at once. (device-bridge-8 needs no change here: `device/keystore_unavailable` stays a transient failure, §12.1.) Matching edits: bridge.md, same id.

1. §17.2: add this paragraph and clause after the paragraph that begins `` `shutdown` replaces today's `exiting` handler steps ``:

   The active identity changes inside the running facade: `CreateFlow::finish` and `resume_create` (§18.1 step 10), the activation of a `join` or `re_pair` (§18.3, §18.4), the activation of `reset_trust` (§18.6), and `forget_identity` of the active identity in `KeyLost` mode (§17.2) "reopen" the facade only in this sense. The caller keeps its `Device` and learns of the change through `mode()`, `status()` and the change counters.

   - **DEV-FACADE-009** (device) When a call of §17.2's list activates another identity or deactivates the active one, the facade MUST keep every existing `Device` handle valid, MUST make `mode()` and `DeviceStatus.mode` report the new mode from the loop turn that activates it, and MUST raise `Changes.status` and `Changes.view` in that loop turn.

### R1-DEVICE-BRIDGE-8 (device-bridge-10)

`forget_identity` may remove the lost identity in `KeyLost` mode (the facade then continues local-only), and a new read lists the identity directories, so the phone and the desktop learn which `(board, dev)` to forget. Matching edits: bridge.md, same id.

1. §5.4, the mode table: replace the row `| `LocalOnly` | every board intent and read query, the fetchers, import check, create board (from this folder or empty), join (pair as a new device), export legacy | the plain files (§9) |` with:

   `| `LocalOnly` | every board intent and read query, the fetchers, import check, create board (from this folder or empty), join (pair as a new device), export legacy, `identities` and `forget_identity` (§17.2) | the plain files (§9) |`

   and the row `| `KeyLost` | the reason, the old identity's `board_id`, `dev` and relay address; `join`; `begin_create`; `forget_identity` | nothing until the user acts |` with:

   `| `KeyLost` | the reason, the old identity's `board_id`, `dev` and relay address; `join`; `begin_create`; `identities`; `forget_identity`, also of the active identity (§17.2) | nothing until the user acts |`

2. §4.4, the paragraph that begins `` `Published` holds: ``: replace `the device status of §17.3, and the change counters.` with `the device status of §17.3, the identity list of §17.2 (`identities()`), and the change counters.`

3. §7.2, the table, row **remove** (as R1-DEVICE-ENGINE-14 item 6 left it): replace `` `pending.tds` after an activation `` with `` `pending.tds` after an activation, `active.tds` when `forget_identity` removes the active identity (§17.2) ``.

4. §7.5: reword DEV-FILE-011 (full text):

   - **DEV-FILE-011** (device) The store MUST NOT delete or truncate any file of an identity directory except by an engine op, the rules of §7.4, the three cases of §7.5, and the removal of a whole identity directory by `forget_identity` or `abandon_create` (§17.2, §18.1), and MUST NOT delete records of `history.tdk` in any other way.

5. §17.2, the table: replace the row `| `forget_identity(board, dev)` | D, A | `()`: removes a non-active identity directory and its key-store entry | `identity_active`, `keystore_failed` | auth | short | all |` with these two rows, in this order:

   `| `identities()` | D, A | `Vec<StoredIdentity>`: one entry per identity directory under `sync/` whose `store.tds` is valid, except the one `pending.tds` names, from its `store.tds` (§6.2), with `active` true for the one `active.tds` names; read at `open` and after every call that adds or removes a directory, published with the view, a change raising `Changes.status` | — | — | read | all |`
   `| `forget_identity(board, dev)` | D, A | `()`: for a non-active identity, removes its key-store entry, then its identity directory (every file, `store.tds` last, then the directory, then `sync_dir` of `sync/`); in `KeyLost` mode it may also name the active identity: it first removes `sync/active.tds` (remove procedure), then does the same, and the facade continues in `LocalOnly` mode (§5.4 step 2, DEV-FACADE-009) | `identity_active` (the active identity, outside `KeyLost` mode), `keystore_failed` | auth | short | desktops; API 30 |`

6. §21 Error catalogue: replace the row `| `device/identity_active` | `forget_identity` named the active identity | §17.2 | choose another identity |` with `| `device/identity_active` | `forget_identity` named the active identity outside `KeyLost` mode | §17.2 | choose another identity |`.

7. §25 Public API: after the line `                          pub relay_address: Option<String>, pub key_fp: Option<String> }` (the end of `IdentityInfo`) add:

   ```rust
   pub struct StoredIdentity { pub board_id: [u8; 16], pub dev: [u8; 16], pub key_store: u8, pub created_ms: i64,
                               pub active: bool }                                  // §17.2, from store.tds
   ```

   and in `impl Device`, after the line `    pub fn forget_identity(&self, board: [u8; 16], dev: [u8; 16]) -> Result<(), DeviceError>;` add the line `    pub fn identities(&self) -> Vec<StoredIdentity>;                                  // read`.

### R1-DEVICE-BRIDGE-9 (device-bridge-14)

An Android facade starts out of the foreground, so a core opened by a job or a receiver keeps no socket.

1. §10.9: add this sentence directly after `` `set_foreground(bool)` (§17.9) tells the device the state. ``: `At `open` the state is foreground on desktops and not foreground on Android (`Platform::Android`), until the first `set_foreground(true)`.`

2. §10.9: add after DEV-NET-014:

   - **DEV-NET-017** (device) At `open`, the device MUST take the foreground state as true on desktops and as false on Android, and on Android MUST keep it false until a `set_foreground(true)`.

3. §17.9, the table: replace the row `| `set_foreground(visible)` | D (always `true` at open), A | §10.9 | short |` with `| `set_foreground(visible)` | D, A | §10.9; the state at `open` is `true` on desktops and `false` on Android (DEV-NET-017) | short |`.

### R1-DEVICE-BRIDGE-10 (device-bridge-16)

A wrong section reference and crate owner, a mode that does not exist, the rulings the spec applies, and the two open questions the lead ruled. Matching edits: bridge.md, same id (its §1 rulings and a removed code).

1. §1, last bullet of the list: replace `- the rulings S1, S3, S4, S8, S9, S13, S21, S23 and S25 of `docs/sync/status/foundation.md`, and decision D6.` with `- the rulings S1, S3, S4, S8, S9, S13, S19, S21, S23, S24, S25, S26, S27, S28, S29 and S30 of `docs/sync/status/foundation.md`, and decision D6.`

2. §2, the table: replace the row `| mode | `NoBoard`, `LocalOnly` or `Synced` (§5.3). |` with `| mode | `LocalOnly`, `Synced` or `KeyLost` (§5.4). |`.

3. §4.1, the table: replace the row `| `taskdeck-android` | `android` | the JNI library: the facade behind JNI, the Keystore key store over the bridge (§12.4) | `taskdeck-device` with default features (V3-17) |` with:

   `| `taskdeck-android` | `android-shell`, then `android` (SYNC_PLAN.md §4) | the JNI library: the facade behind JNI, the Keystore key store over the bridge (§12.5) | `taskdeck-device` with default features (V3-17) |`

4. §3: reword DEV-GEN-001 (full text):

   - **DEV-GEN-001** (device) The `taskdeck-device` crate MUST declare `#![deny(unsafe_code)]` at its root, and MUST allow `unsafe` only in the module `platform::windows` (§12.3, §7.1; ruling S26), whose every `unsafe` block MUST carry a `// SAFETY:` comment and call only `CryptProtectData`, `CryptUnprotectData`, `LocalFree` and `MoveFileExW` of the `windows` crate.

5. §26, open question 1: replace the whole item with:

   1. **`unsafe` on Windows.** Ruled S26: `unsafe` is allowed in platform FFI and only there; in `taskdeck-device` that is the module `platform::windows`, four functions, each block commented (DEV-GEN-001).

6. §26, open question 4: replace the whole item with:

   4. **Backup file names carry a writer tag** (§15.3). Ruled S28, so that two desktops writing to one synced folder never overwrite each other's files.

### R1-DEVICE-BRIDGE-11 (device-bridge-11)

Nothing further to apply. Ruling S27 is applied by R1-SEAL-RELAY-9 (seal.md §20.4 `history`, SEAL-BLOB-006, SEAL-BLOB-007), R1-SEAL-RELAY-28 (device.md §15.7 rewritten, the §18.3 history-blob cell deleted, `history.tdk` written from the bootstrap snapshot, the §24 bundle request deleted, open question 6 rewritten) and R1-DEVICE-ENGINE-1 (the rejoin path, DEV-RUN-006). The finding's `vb32(history)` in the kind-17 Join layout is not adopted: the earlier resolvers settled that the sealer fills and keeps the history (lead ruling L4). S27 is listed in §1 by R1-DEVICE-BRIDGE-10.

### R1-DEVICE-BRIDGE-12 (device-bridge-12)

Nothing further to apply. The lead's note under S28 is applied by R1-SEAL-RELAY-20 (relay.md §4.2 `read_epoch`, §12.11, RELAY-RTOKEN-004), R1-SEAL-RELAY-27 (device.md §15.8 step 1: epoch 1, then once with the refusal's `read_epoch`; open question 7) and R1-DEVICE-ENGINE-15 (the §22 row "recovery read-token mints | 2"). The finding's removal of that row is not adopted: the earlier row states the bound DEV-BKP-011's flow keeps.

## bridge.md

### R1-DEVICE-BRIDGE-1 (device-bridge-1, device-bridge-9)

The replies of `POST /api/look` and `POST /api/background` carry the look id the facade returns after it published the new look (`device.md` §14.2, §14.3, §17.12, same id).

1. §6.2, the route table, row 5: replace its Facade cell `` `set_own_picture`, `clear_own_picture` (§20) `` with `` `set_own_picture`, `clear_own_picture` (`device.md` §14.3) ``.

2. §6.4.3: replace `The reply's `id` is the id of the picture now shown (§7.4) or `null`, and `kept` is whether a picture is shown.` with `The reply's `id` is the value `set_look_dials` returned (the id of the look now shown, §7.4, or `null`), and `kept` is `true` exactly when it is not `null`.`

3. §6.4.4, the table: replace the reply cell `` `{"ok": true, "id": <id of the shared picture's look, or null>}` `` with `` `{"ok": true, "id": <the id clear_own_picture returned, or null>}` `` and the reply cell `` `{"ok": true, "id": <id of the new look>}` `` with `` `{"ok": true, "id": <the id set_own_picture returned>}` ``.

4. §20, `device.md` block: replace the whole bullet that begins `- Requires, new: `Device::set_own_picture(bytes: Vec<u8>) -> Result<(), DeviceError>`` with:

   - Requires: `Device::set_own_picture(bytes)` and `clear_own_picture()` (`device.md` §14.3, §17.12, DEV-PIC-005; ruling S29) and `set_look_dials`, each returning `Result<Option<String>, DeviceError>`, the id of the look now shown after it is published; `Published::picture()` and the snapshot's `look` are the own picture's while the phone has one.

5. §21 Public API, `impl PageFacade`: replace

   ```rust
       pub fn set_look_dials(&self, blur: u8, light: u8) -> Result<(), taskdeck_device::DeviceError>;
       pub fn set_own_picture(&self, bytes: Vec<u8>) -> Result<(), taskdeck_device::DeviceError>;
       pub fn clear_own_picture(&self) -> Result<(), taskdeck_device::DeviceError>;
   ```

   with

   ```rust
       pub fn set_look_dials(&self, blur: u8, light: u8) -> Result<Option<String>, taskdeck_device::DeviceError>;
       pub fn set_own_picture(&self, bytes: Vec<u8>) -> Result<Option<String>, taskdeck_device::DeviceError>;
       pub fn clear_own_picture(&self) -> Result<Option<String>, taskdeck_device::DeviceError>;
   ```

6. §22, open question 1: replace the whole item with:

   1. **The phone's own picture.** Ruled S29: the phone keeps a picture of its own, never synced, processed in Rust like the shared one (`device.md` §14.3, file kind 19 `own-picture.tdk`).

### R1-DEVICE-BRIDGE-2 (device-bridge-2)

The notices event sends the `seq` and `index` of the facade's `NoticeView` and leaves dismissed records out (`device.md` §17.5, §25, same id).

1. §5.4, the notices data table: replace the row `` | `records` | array | required | every record of `Published::notices()` that is not dismissed, in its order, each `{"seq": integer, "index": integer, "record": <the canonical record of core.md §10.7 as a JSON object>, "type": "notice" \| "refusal"}` | `` with:

   `| `records` | array | required | every `NoticeView` of `Published::notices()` whose `dismissed` is false, in its order, each `{"seq": integer, "index": integer, "record": <the canonical record of core.md §10.7 as a JSON object>, "type": "notice" \| "refusal"}`: `seq` and `index` the `NoticeView`'s, `type` from its `NoticeRecord` variant |`

2. §5.4: replace the sentence `` `index` is the position of the record among the records of the same `seq` in `notices()`, counting from 0 (an assumption about `device.md`, §20). `` with `` `seq` and `index` are the engine's own (`engine.md` `NoticeEntry`, copied by `device.md` §17.5), the pair `dismiss_notice` takes. ``

3. §20, `device.md` block: replace the bullet `` - Requires: `Published::notices()` lists records in an order in which "the position among the records of the same seq" is the `index` `dismiss_notice` takes (§5.4). `` with `` - Requires: `NoticeView` carries `seq`, `index` and `dismissed` (`device.md` §17.5, §25), and `dismiss_notice` takes that `seq` and `index` (§5.4). ``

### R1-DEVICE-BRIDGE-3 (device-bridge-3)

The phone's notes save is the facade's draft (`device.md` DEV-FACADE-008, same id), so `GET /api/notes_merge` works for it.

§20, `device.md` block: replace the bullet that begins `` - Requires: `notes_merge()` is also present after a `core/notes_stale` refusal `` with:

- Requires: a `set_notes` the page sends through `submit_json` becomes the facade's notes draft (`device.md` §17.6, DEV-FACADE-008), so that after a `core/notes_stale` refusal `notes_merge()` offers its text as `mine` until `resolve_notes` or `discard_notes_draft` (A-06).

### R1-DEVICE-BRIDGE-4 (device-bridge-4)

`for` follows the place the report was fetched for, and `here` follows the stored own place (`device.md` §17.5, §17.12, §25, same id).

1. §6.4.2, the table: replace the row `` | `for` | `"here"` when `weather_place()` is `(Own, Some(place))`, else `"desk"` | `` with `` | `for` | `"here"` when the third element of `Published::weather()` (the place the report was fetched for) is `Own`, else `"desk"` | ``.

2. §6.4.2, the table, row `here`: replace `when the own place is set, else `null`;` with `when `weather_place().1` is `Some`, else `null`;`.

3. §20, `device.md` block: replace the bullet `- Requires, new: a read `Device::weather_place() -> (WeatherSource, Option<taskdeck_core::board::Home>)` from `device.tdk` (§6.4.2).` with `- Requires: `Device::weather_place()`, and the report's source as the third element of `Published::weather()` (`device.md` §17.12, §25; §6.4.2); `set_weather_place` with `home` clears the own place.`

### R1-DEVICE-BRIDGE-5 (device-bridge-5, device-bridge-13, device-bridge-15)

`AndroidKeyStore` creates the directories it needs and writes `ld.wrap` itself, and attaches whichever thread calls it (`device.md` §12.1, §12.5, DEV-THREAD-001, same id).

1. §11.5, the trait table: replace the row `` | `create(board, dev, key)` | `KeyVault.wrap(alias, AD_L, key)`; on status 0 writes the 60 returned bytes to `<data_dir>/sync/id-<board hex>-<dev hex>/ld.wrap` by `device.md`'s replace procedure (`ld.wrap.tmp`, `sync`, `rename`, `sync_dir`) | `` with:

   `| `create(board, dev, key)` | `KeyVault.wrap(alias, AD_L, key)`; on status 0 creates `<data_dir>/sync/` and `<data_dir>/sync/id-<board hex>-<dev hex>/` when they are missing (`create_dir`, then `sync_dir` of its parent, for each), then writes the 60 returned bytes as `ld.wrap` in that directory: `ld.wrap.tmp`, `sync`, `rename`, `sync_dir` |`

   and the row `` | `delete(board, dev)` | `KeyVault.delete(alias)`, then removes `ld.wrap` | `` with `` | `delete(board, dev)` | `KeyVault.delete(alias)`, then removes `ld.wrap` and calls `sync_dir`; a missing `ld.wrap` is not an error | ``.

2. §11.5: replace the paragraph that begins `**Threading.**` with:

   **Threading.** `Device::open` calls `load` on its calling thread (`lifecycle`, or a job or receiver worker); `create`, `delete` and any other `load` run on the device's sync thread (`device.md` §12.1). Every trait method first attaches its thread to the JVM with `JavaVM::attach_current_thread_as_daemon` when it is not attached. After each call Rust checks for a pending Java exception, clears it, and maps it to `Failed`.

3. §11.5: add after BRIDGE-KEY-003:

   - **BRIDGE-KEY-005** (android) Every trait method of `AndroidKeyStore` MUST attach its calling thread to the JVM as a daemon thread when it is not attached, before its first JNI call.

4. §20, `device.md` block: replace the bullet that begins `` - Requires: `AndroidKeyStore` (in `taskdeck-android`) reads and writes `ld.wrap` `` with `` - Provides: `AndroidKeyStore`, which writes and removes `ld.wrap` itself, outside the store, on the threads `device.md` §12.1 names (`device.md` §12.5, DEV-THREAD-001; BRIDGE-KEY-001, BRIDGE-KEY-004, BRIDGE-KEY-005). ``

5. §20, `device.md` block, the `Assumes` bullet: replace `` ; `Device::open` calls `KeyStore::load` on the calling thread. `` with `.`

6. §20, `device.md` block, the first `Provides` bullet: replace `BIOMETRIC_STRONG before "auth" calls (BRIDGE-AUTH-001, DEV-FACADE-003).` with `BIOMETRIC_STRONG before "auth" calls (BRIDGE-AUTH-001, which `device.md` §17.1 names for Android).`

### R1-DEVICE-BRIDGE-6 (device-bridge-6)

Ruling S30 in the call table: the four "auth" calls `device.md` offered on every platform are API 30 on the phone (`device.md` §17.1, §17.2, §17.7, same id; `forget_identity`'s row is R1-DEVICE-BRIDGE-8's).

1. §11.4, the `call` table: replace the row `| `use_this_copy` | `{}` | `null` | `use_this_copy` | auth | android |` with `| `use_this_copy` | `{}` | `null` | `use_this_copy` | auth, API 30 | android |`; the row R1-DEVICE-ENGINE-4 wrote for `adopt` with `` | `adopt` | `{"dev": hex32, "typed_fp": string}` | ticket | `adopt` | auth, API 30 | android | ``; and the row `| `skip` | `{"seq": int}` | ticket (int) | `skip` | auth | android |` with `| `skip` | `{"seq": int}` | ticket (int) | `skip` | auth, API 30 | android |`.

2. §22, open question 7: replace the whole item with:

   7. **Every "auth" call on the phone needs API 30** (§13.2). Ruled S30: the framework offers BIOMETRIC_STRONG without a PIN fallback only from API 30, and the app may use no `androidx.biometric` (§12.4); `device.md` §17 marks these calls "desktops; API 30", so the facade refuses them below.

### R1-DEVICE-BRIDGE-7 (device-bridge-7, device-bridge-8)

The hello's mode is the facade's mode when the hello is sent, a mode change reloads the page, and a Keystore that is not ready yet does not fail the process for good (`device.md` §17.2, DEV-FACADE-009, §12.1, same id).

1. §4.3, first paragraph: replace `` holding the state `Closed`, `Opening`, `Open(mode)` or `Failed(code)`, `` with `` holding the state `Closed`, `Opening`, `Open` or `Failed(code)`, ``, and append at the paragraph's end: ` `Open` carries no mode: the mode is the facade's and changes while the core is open (`device.md` DEV-FACADE-009), so the shell reads it from `DeviceStatus.mode` whenever it needs it (§5.8).`

2. §4.3, step 4: replace the whole step with:

   4. Set `Opening`; call `Native.initPlatform(context)` once per process (§11.2); build the configuration of §11.3; call `Native.open(config)`. On success set `Open`. On a failure with `device/keystore_unavailable` (the Keystore is not ready yet, `device.md` §12.1) set `Closed` and return `Failed(code)` for this call only, so that the next `ensureOpen` opens again; on any other failure set `Failed(code)`.

3. §4.3: add after BRIDGE-PROC-005:

   - **BRIDGE-PROC-008** (android) After `Native.open` failed with `device/keystore_unavailable`, `CoreHost.ensureOpen` MUST return `Failed` with that code and MUST leave the state `Closed`, so that the next call opens again; after any other failure it MUST keep the state `Failed(code)`.

4. §5.4, the events table, row `hello`: replace its "Sent when" cell with `after the load's `ready`, as soon as `CoreHost.ensureOpen` has returned `Open` or `Failed` (§5.8)`.

5. §5.8: replace `The shell sends `hello`:` and the table's first four rows (`Open(LocalOnly)`, `Open(Synced)`, `Open(KeyLost)`, `Failed(code)`) with:

   The shell sends `hello`, its `mode` read from `DeviceStatus.mode` when it is sent:

   | Core host state | `hello.mode` | What the page shows |
   |---|---|---|
   | `Open`, `DeviceStatus.mode` `LocalOnly` | `local` | the board (an empty local board before pairing); in `android` the sync line says the phone is not paired |
   | `Open`, `DeviceStatus.mode` `Synced` | `synced` | the board |
   | `Open`, `DeviceStatus.mode` `KeyLost` | `key_lost` | the panel: "This phone can no longer unlock its copy of the board. Pair it again to carry on." with a button "Open Devices" (`POST /api/screen`, §6.4.7) |
   | `Failed(code)`, as `ensureOpen` returned it | `failed` | the panel: "TaskDeck could not open the board (<code>). Close the app and open it again." |

   (the rows `Locked` and echo flavour stay as they are).

6. §5.8: add this paragraph after the paragraph that begins `When 20,000 ms pass without `hello``, before BRIDGE-MSG-013:

   When the mode changes while a page load is current (a join or re-pair finished, a lost identity forgotten, `device.md` DEV-FACADE-009), the watcher sees `Changes.status` move and `DeviceStatus.mode` differ from the `mode` that load's `hello` carried, and reloads the WebView (`WebView.reload()`); the new page load boots with a fresh `hello` (§5.6).

7. §5.8: reword BRIDGE-MSG-013 (full text) and add a clause after BRIDGE-MSG-014:

   - **BRIDGE-MSG-013** (android-shell) The shell MUST start loading the page before `CoreHost.ensureOpen` returns, and MUST send `hello` to the current page load within 50 ms of `ensureOpen` returning `Open` or `Failed`.

   - **BRIDGE-EVT-005** (android) When `DeviceStatus.mode` differs from the `mode` of the current page load's `hello`, the watcher MUST reload the WebView.

8. §21 Public API, Kotlin: replace `    data class Open(val mode: String) : CoreState; data class Failed(val code: String) : CoreState }` with `    object Open : CoreState; data class Failed(val code: String) : CoreState }`.

### R1-DEVICE-BRIDGE-8 (device-bridge-10)

The phone lists the identity directories and forgets the lost one, or an old one (`device.md` §5.4, §17.2, §25, same id).

1. §11.4, the `call` table: replace the row `| `forget_identity` | `{"board": hex32, "dev": hex32}` | `null` | `forget_identity` | auth | android |` with these two rows, in this order:

   `| `identities` | `{}` | array of `{"board_id": hex32, "dev": hex32, "key_store": int, "created_ms": int, "active": bool}` | `identities()` | — | android |`
   `| `forget_identity` | `{"board": hex32, "dev": hex32}` | `null` | `forget_identity` | auth, API 30 | android |`

2. §13.1, the table: replace the row `| Devices: a lost key | the old identity (key lost, `device.md` §12.6) | Forget the old copy (`forget_identity`) | API 30 and auth |` with these two rows, in this order:

   `| Devices: a lost key | the old identity (the `identities()` entry with `active`, key lost, `device.md` §12.6) | Forget the old copy (`forget_identity`; the phone then opens as an unpaired phone, `device.md` §17.2) | API 30 and auth |`
   `| Devices: old copies | every `identities()` entry that is not `active` (board, device id, created) | Forget (`forget_identity`) | API 30 and auth |`

### R1-DEVICE-BRIDGE-10 (device-bridge-16)

The rulings bridge.md applies, and a device code that no longer exists (R1-DEVICE-ENGINE-16 removed `device/reopen_needed`).

1. §1, the bullet that begins `- the rulings S4`: replace `S19 (reminder leads), S26 (`unsafe` only in the JNI entry points) of` with `S19 (reminder leads), S26 (`unsafe` only in the JNI entry points), S29 (the phone's own picture), S30 (device authentication on the phone needs API 30), S31 (`ACCESS_NETWORK_STATE`) of`.

2. §6.5, the table: replace the row `| `device/closed`, `device/reopen_needed`, `device/wrong_mode` | 503 | TaskDeck's board is not open. Close the app and open it again. |` with `| `device/closed`, `device/wrong_mode` | 503 | TaskDeck's board is not open. Close the app and open it again. |`.

## Rejected

None. Parts of proposals are not adopted, each recorded in its change:
- device-bridge-1: the new clause DEV-PIC-006 (DEV-PIC-005, from R1-DEVICE-ENGINE-14, already forbids the own picture in any intent, frame, blob or backup); the bound of 10,000 ms for `set_own_picture` (R1-DEVICE-ENGINE-14 set 120,000 ms; kept); the seal.md change (R1-SEAL-RELAY-10).
- device-bridge-4: rounding moved to R1-DEVICE-BRIDGE-5 (it is the same sentence of §13.2 that device-bridge-15 rewords); the case of `Own` with no place gets the new code `device/bad_place`.
- device-bridge-7: besides the hello read at send time, the page reloads on a mode change (BRIDGE-EVT-005), so the current load does not keep the key-lost panel over a working board.
- device-bridge-10: `identities()` returns `StoredIdentity` (from `store.tds`, the active one flagged) instead of `IdentityInfo`, whose device name and fingerprint cannot be read without each identity's `L_d`; `forget_identity` of the active identity removes `active.tds` first, so a crash never leaves `active.tds` naming a half-removed directory.
- device-bridge-11, device-bridge-12: already applied by the earlier files (R1-DEVICE-BRIDGE-11, -12).
- device-bridge-15: DEV-FACADE-003 keeps owner `desktop` only; its Android half is BRIDGE-AUTH-001 (R1-DEVICE-BRIDGE-5).

## For the lead

1. **A mode change reloads the phone page (R1-DEVICE-BRIDGE-7, BRIDGE-EVT-005).** Reading `hello.mode` at send time fixes later page loads, but the load that is current when a re-pair or a forget finishes would keep the key-lost panel (or the "not paired" sync line) until the user restarts the app. Option (a), written as the change: the watcher reloads the WebView when `DeviceStatus.mode` differs from the mode its hello carried. Option (b): the page reacts to the `sync` event's `mode` itself, which is a new change to the fork (S-12). Recommended: (a), one clause, no fork change.
2. **`forget_identity` of the active identity in `KeyLost` mode, and the new read `identities()` (R1-DEVICE-BRIDGE-8).** Without them the phone's only cleanup action always fails and old identity directories and Keystore aliases are never removed. The facade then continues local-only, as an unpaired phone. Recommended: accept. Written as the change.
3. **`set_look_dials` becomes a "long" call (R1-DEVICE-BRIDGE-1).** To return the id of the new look it waits for the picture thread (at most 5,000 ms), so under DEV-THREAD-008 the desktop must call it from its own worker thread, as it already does for the other long calls; DOCUMENTATION.md §14.1 gains no thread, timer or redraw rule. The alternative keeps it short and returns nothing, leaving the bridge's reply to carry the old look's id. Recommended: accept. Written as the change.
