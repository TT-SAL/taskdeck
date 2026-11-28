# Stage `device`: where the parts did not fit

Every case met while the real core, seal and relay are wired into the engine, or where a spec is wrong, unclear or silent, in the order met. Format: `briefs/README.md`. The lead's judgement of each is in `../device.md` ("Where the parts did not fit").

## f1-1. `seal::Membership` is not `Clone` and exposes too little for the view
- Kind: misfit
- Where: engine.md §6.1 `type Membership: Clone`, §6.2 `MembershipView`; seal.md §26 `Membership`; `crates/taskdeck-device/src/adapt/membership.rs`
- Says / does: the engine needs a `Clone` membership and every field of seal.md §13.1 in the view (epoch_seq, clocks, home, purge_seq, commitments, pending_override, last_reset, skips, ctl_seqs); seal's `Membership` has no `Clone` (its `dup` is crate-private) and public accessors only for board, epoch, rules, devices, current_recovery and device_no.
- Done here: `SealMembership` holds the seal value in an `Arc` with its canonical bytes; a second reader of the §13.2 layout (`Fields::parse`, bounds-checked, run only on bytes seal accepted or wrote) gives the view and `c_e`.
- Proposed fix: seal change: `impl Clone for Membership` (it holds no secret, SEAL-MEM-003) and read accessors for the §13.1 fields, or a public read-only fields struct; then `Fields` goes.
- Evidence: `adapt::seal_sealer::tests::the_view_agrees_with_seal_and_the_bytes_round_trip`

## f1-2. `seal::Receipt` lacks the engine receipt's `author`, `ctr`, `frame_hash` and `keys_changed`
- Kind: misfit
- Where: engine.md §6.1 `Receipt`; seal.md §26 `Receipt { learned_root, .. }`; `SealSealer::classify`
- Says / does: the engine's receipt carries the header's author and counter, the frame hash and whether a secret changed; seal's carries `learned_root` and none of the others.
- Done here: author and counter from `frame_header` (zero when step 1 fails, as the fake), the hash from `frame_hash`; `keys_changed` is true when the adapter stores a root it did not hold (the own wrap's, or a missed wrap's) or removes a pending root.
- Proposed fix: none needed; engine.md §6.1 `classify` could say that `keys_changed` is the adapter's, from `learned_root`, the pending root and the missed wraps.
- Evidence: `adapt::seal_sealer::tests::classify_is_repeatable`, `an_own_rotation_is_pending_until_accepted`

## f1-3. seal has no opener for one own frame, which `open_own` needs
- Kind: seal-fault
- Where: engine.md §6.1 `open_own` (used by §11.3's reconcile, rows "`open_own` returns …" and "`open_own` fails"); seal.md §26; `SealSealer::open_own`
- Says / does: `open_own` opens a frame this device signed under any root it holds; seal.md §26 offers no function that verifies and opens one frame outside `classify`, whose step 5 needs a membership with the author's counter at `ctr − 1`.
- Done here: a frame this process sealed is found by its hash; any other own frame is opened by `classify` on a probe state built from `M_K` (this device current at `ctr − 1`, the frame's epoch as the current one), so seal's own steps 1 to 11 (signature, AEAD under a held root, unpadding) run, and the opened `entry_id` is taken whatever the later steps say.
- Proposed fix: seal change: `open_own_frame(keys: &DeviceKeys, board: &BoardId, roots: &dyn Fn(u32) -> Option<&EpochRoot>, frame: &[u8]) -> Result<Plain, SealError>` (§14.2 steps 1, 2, 4, 8 to 11 for the device's own key); then the probe goes.
- Evidence: `adapt::seal_sealer::tests::open_own_opens_an_own_frame_in_a_new_process`

## f1-4. The Genesis item's state hash needs `M_1`, which seal cannot compute before seq 1 exists
- Kind: spec-silent
- Where: device.md §18.1 step 4 ("the state hash of the genesis board and `M_1`"); seal.md §12.2, §13.3; `SealSealer::create`
- Says / does: the creating device puts `state_hash(board, M_1)` into the Genesis item; `M_1` is what classifying that frame produces, and seal's `Membership::genesis` is crate-private, so seal gives no `M_1` before the frame is sealed.
- Done here: `create` lays `M_1` out by §13.3 (`Fields`), has seal's parser judge it, and hashes it; a test compares it with the `M_1` that `classify` returns and the item's hash with ENG-HASH-001 of it.
- Proposed fix: seal change: a public `genesis_membership(salt, recovery, device, commit, at_ms) -> Membership` (or `Genesis` item → `M_1`); device.md §18.1 step 4 names it.
- Evidence: `adapt::seal_sealer::tests::the_genesis_state_hash_covers_the_m1_classify_produces`

## f1-5. A genesis snapshot cannot be built: its body depends on the frame that names it
- Kind: spec-wrong
- Where: device.md §18.1 step 4 (a non-empty genesis board's snapshot at `S = 1`, purpose 3); seal.md §12.2 `snapshot`, §20.4 (`chain` is `C_S`; `history` one record per seq of `M_S.ctl_seqs`, the first at seq 1, SEAL-BLOB-006)
- Says / does: the body at `S = 1` must hold `C_1` and the history record of seq 1, both of which contain the Genesis frame's hash; the Genesis item in that frame holds the snapshot's `sha256`. `seal_snapshot` refuses a body whose history is not exactly `[1]`.
- Done here: `SealSealer::create` builds only a Genesis without `snapshot` (source `Empty`); the other `CreateRequest` sources wait for the spec.
- Proposed fix: spec change (seal.md §20.4, §20.5, §12.2): a genesis snapshot (`S = 1`, purpose 3) carries `chain` = 32 zero bytes and an empty history, which the reader replaces by `C_1` and the record of seq 1 from the log it holds; or the genesis board travels in a first `cmd`-like import entry instead.
- Evidence: seal.md §20.4 table and `taskdeck_seal::seal_snapshot` (`crates/taskdeck-seal/src/blob.rs`, the `starts_at_1` check)

## f1-6. `rebind`'s origin state: layout and a mismatched `device_no`
- Kind: decision
- Where: engine.md §5.1 `rebind`; device.md §8.3 (origin state `device_no u16 ‖ next_item u64 ‖ next_block u64`); `CoreMachine::rebind`
- Says / does: engine.md does not say what `rebind` answers for an origin state that is not the adapter's encoding, or one minted for another `device_no` than `to_device_no`.
- Done here: both are `SmError { code: "device/record_format" }` and nothing is rebound; core's `core/bad_id` (no number left) passes through.
- Proposed fix: engine.md §5.1 `rebind` row: "an error when `origin_state` is not this adapter's encoding of a minter for `to_device_no`".
- Evidence: `adapt::core_machine::tests::rebind_refuses_an_origin_state_that_is_not_the_new_identitys`

## f1-7. The identity export does not carry the board id
- Kind: spec-silent
- Where: device.md §6.4.7; `SealSealer::open`
- Says / does: kind 7 holds keys, roots, pending roots and the anchor; the anchor names the board only on the creating device. Every later `open` needs the board id (chain zero, sealing, AD) from elsewhere.
- Done here: `SealSealer::open(board_id, identity, history)` takes it from the caller, which is the store (the sealed files bind board and `dev`, seal.md §19).
- Proposed fix: device.md §6.4.7 or §17.2: "the board id comes from the identity directory (`active.tds`)", or a `board_id[16]` field in kind 7.
- Evidence: `adapt::seal_sealer::tests::the_identity_export_round_trips`

## f1-8. The genesis anchor is kept, not dropped when seq 1 is accepted
- Kind: decision
- Where: device.md §6.4.7 (`anchor` "present on the creating device until seq 1 is accepted"); engine.md §6.1 `classify` repeatable, §19.8; `SealSealer::classify`, `forget_roots_below`
- Says / does: dropping the anchor at the acceptance of seq 1 makes that classification unrepeatable, and a crash after the `WriteIdentity` without it and before the checkpoint at 1 is durable leaves a folder whose `Start` with `Bootstrap::Genesis` cannot classify seq 1 again (`engine/bad_genesis`). The anchor holds nothing beyond `R_1`, which `roots` hold anyway.
- Done here: the anchor stays for the identity's life; `forget_roots_below(e)` with `e > 1` drops it with `R_1` (a purge).
- Proposed fix: device.md §6.4.7: "present on the creating device until a purge deletes `R_1`".
- Evidence: `adapt::seal_sealer::tests::forgetting_roots_drops_the_anchor_with_r1`

## f1-9. A 17th pending rotation
- Kind: decision
- Where: device.md §6.4.7 (`n_pending` 0 to 16); `SealSealer::seal`
- Says / does: nothing says what `seal` does when 16 own rotations are pending.
- Done here: `seal` refuses a 17th `Rotate` with `device/pending_roots` before it draws anything.
- Proposed fix: device.md §6.4.7 names the code.
- Evidence: `crates/taskdeck-device/src/adapt/seal_sealer.rs`, `PENDING_FULL`

## f1-10. `seal::SealedBlob` has no inflated length
- Kind: misfit
- Where: engine.md §6.1 `SealedBlob.inflated` (it goes into `SnapInfo.inflated`, which the exact open compares); seal.md §26 `SealedBlob { id, bytes, sha256, parts }`
- Says / does: seal knows the body's length before compression and does not return it.
- Done here: the adapter computes it from the §20.4 layout (`inflated_len`); a test opens the blob with `exact = true` against it.
- Proposed fix: seal change: `seal_snapshot` returns the inflated length (a field of its result).
- Evidence: `adapt::seal_sealer::tests::a_snapshot_seals_and_opens_in_both_modes`

## f1-11. `reset` also fails with `core/cap`
- Kind: spec-unclear
- Where: engine.md §5.1 `reset` ("an error only for a rules version this build lacks"); core.md §11.3, CORE-RULES-005; `CoreMachine::reset`
- Says / does: core's `reset` returns `core/cap` when `notes_rev` is at 2^53 − 1 and cannot be raised.
- Done here: passed through as `SmError { code: "core/cap" }`; the engine treats any `reset` error as `engine/local_fault`.
- Proposed fix: engine.md §5.1 `reset` row: "or `core/cap` when the notes revision cannot be raised (core.md §11.3); both are the local fault".
- Evidence: `adapt::core_machine::tests::reset_fails_at_the_last_notes_revision`

## f1-12. SEAL-FRAME-005 and a counter cleared for re-sealing
- Kind: spec-unclear
- Where: seal.md SEAL-FRAME-005 ("at most once" per author and counter); engine.md §11.3 (frames cleared for re-sealing), ENG-SEALER-005; `SealSealer::seal`
- Says / does: the engine may ask for another entry, or the same entry under a newer epoch, under a counter whose frame it cleared; "at most once" reads as forbidding it.
- Done here: as the fake: the same entry under the same epoch gets the stored bytes; another entry or epoch is sealed anew and replaces the stored sealing (memory only).
- Proposed fix: SEAL-FRAME-005: "… once for one entry and epoch; an entry the engine cleared for re-sealing (engine.md §11.3) is sealed anew".
- Evidence: `adapt::seal_sealer::tests::sealing_the_same_entry_again_returns_the_same_bytes`

## f1-13. Which supported rules the sealer classifies with
- Kind: decision
- Where: engine.md §6.1 `classify` ("its build's supported rules"); seal.md §14.1 `supported_rules`; `SealSealer`
- Says / does: the `Sealer` trait gives the sealer no access to `SM::supported_rules`.
- Done here: the sealer takes `taskdeck_core::apply::SUPPORTED_RULES`, the same build's list.
- Proposed fix: none.
- Evidence: `crates/taskdeck-device/src/adapt/seal_sealer.rs`, `SealSealer::blank`

## f1-14. Roots and SEAL-KEY-013 where no membership is at hand
- Kind: decision
- Where: SEAL-KEY-013; engine.md §6.1 `open_snapshot` (no membership argument); `SealSealer::open`, `open_snapshot`
- Says / does: `open_snapshot` derives `K_snap` with no membership to check the root against; `open` reads roots from the identity file.
- Done here: a root enters the sealer only checked (by `classify`, by `unwrap_root` against `m`'s `c_e`, or `R_1` of its own genesis); roots read back from `identity.tdk` (sealed under `L_d`) are trusted as checked when learned. `seal` and `seal_snapshot` check the root against `m` again.
- Proposed fix: none; device.md §24's SEAL-KEY-013 bullet could say "checked when learned".
- Evidence: `SealSealer::checked_root`

## f1-15. The control history is held whole in memory
- Kind: decision
- Where: device.md §15.7 ("the adapter then drops the body's history"), §6.4.8; engine.md §6.1 `seal_snapshot` (the history up to `S`); `SealSealer::history`
- Says / does: `seal_snapshot` needs every record from seq 1, so a sealer that drops records cannot seal snapshots.
- Done here: the sealer keeps every record (from `open`, from each accepted control entry, from opened snapshots) and `history()` lists them for the store to write those above `history.tdk`'s last seq.
- Proposed fix: none now; F2 may read `history.tdk` on demand instead if memory matters (at most 1,048,576 records).
- Evidence: `adapt::seal_sealer::tests::an_own_rotation_is_pending_until_accepted` (history holds 1 and 2)

## f2-1. The engine's records have no `serde`, so `record_json<T: Serialize>` cannot write them
- Kind: misfit
- Where: device.md §6.5, §25 `store::record_json`; engine.md §37 (`Registry`, `OutboxRecord`, `Meta` and their parts); `crates/taskdeck-device/src/store/rjson.rs`
- Says / does: §6.5 maps the engine's Rust types to canonical JSON and §25 gives `record_json<T: serde::Serialize>`; none of `taskdeck_sync`'s record types implement `Serialize`, and the derive's output (`null` for `None`, externally tagged enums in Rust case) is not §6.5's mapping anyway.
- Done here: the mapping is written out over `serde_json::Value` (trait `Rj`, one impl per type of §37, about 30), encoded and parsed with `taskdeck_core::canon`. `record_json`/`from_record_json` stay generic for the device's own records.
- Proposed fix: device.md §25: name the three functions `registry_json`, `outbox_record_json`, `meta_json` (and their readers) instead of a generic `Serialize` bound; or engine derives behind a feature, with §6.5's attributes.
- Evidence: `store::tests::record_json_is_canonical_and_strict`

## f2-2. §6.5 maps `Option<T>` only as a named field
- Kind: spec-unclear
- Where: device.md §6.5 table, first row
- Says / does: "a field of type `Option<T>` that is `None` is absent"; an `Option` in a tuple, a `Vec` or a tuple variant has no mapping. Today every `Option` of §37's records is a named field.
- Done here: `rjson` maps `Option` only as a field (`Obj::o`, `Fields::o`); an `Option` elsewhere would not compile.
- Proposed fix: §6.5: "`Option<T>` occurs only as a named field; a new record type that needs one elsewhere extends this table."
- Evidence: `store::tests::record_json_is_canonical_and_strict`

## f2-3. When a log segment is closed
- Kind: spec-unclear
- Where: device.md §6.1 last paragraph, DEV-STORE-003
- Says / does: "a segment is closed when it holds 16,384 entries or 67,108,864 bytes"; silent on whether one `AppendLog` may cross the count, and on which bytes count.
- Done here: an `AppendLog` is cut at the entry boundary, so a segment holds at most 16,384 entries; the bytes are the file's length, checked before each page record, so a segment may pass 67,108,864 bytes by one page record. `IdentityStore::append_log`.
- Proposed fix: §6.1: "a page record never crosses a segment's 16,384th entry; a segment whose file has reached 67,108,864 bytes takes no further page."
- Evidence: `store::tests::segments_roll_over_truncate_and_quarantine` (the second segment starts at 16,385)

## f2-4. `device.tdk`'s fields that the store does not own
- Kind: decision
- Where: device.md §16 table; `store::settings::DeviceSettings`
- Says / does: the record has 16 fields; the store reads `device_name`, `log_floor`, `floor_chain`, `minter`, `history_complete`; the rest (tokens, backup, look, requests, ...) are the transport's, the fetchers', the backups'.
- Done here: those five are typed and checked; the others are carried verbatim in `DeviceSettings::other`, checked only by name and by the canonical parser (which refuses a field written at its default). The relay `tokens` are secrets held in plain `serde_json::Value`s, which nothing in the spec forbids (SEAL-FILE-004 names kind 7 only).
- Proposed fix: none; the tasks that own them type them (F3 the tokens and `requests`, in `Zeroizing` for the token).
- Evidence: `store::tests::settings_round_trip_with_defaults`

## f2-5. The `FileLayer` has no lock, and the test key stores write outside it
- Kind: spec-silent
- Where: device.md §7.1 (DEV-FILE-001 "every file system call through its `FileLayer`"), §5.2, §12.6
- Says / does: the lock is a held `File` with `try_lock`, which no `FileLayer` call gives; a key store keeps its own files (§12.5 says so for `AndroidKeyStore` only).
- Done here: `store::take_lock` uses `std::fs` directly; `FileKeyStore` writes `<dir>/<board>-<dev>.testkey` with `std::fs` by the replace steps. Neither is a write boundary of `CrashFiles`.
- Proposed fix: §7.1: "the lock file (§5.2) and the key stores' files (§12) are outside the `FileLayer`."
- Evidence: `store::tests::a_second_open_of_a_locked_folder`, `keystore::tests::file_key_store_round_trips`

## f2-6. No code for a failed store write
- Kind: spec-silent
- Where: device.md §7.3, §7.4, §21
- Says / does: a failed write boundary of a `Persist` delivers `PersistFailed` (no code); a failed write at open (removing `*.tmp`, truncating a torn tail, renaming a quarantined file) has no code at all. `device/write_failed` is "a plain-file write failed".
- Done here: both use `device/write_failed`; the persist error only feeds `PersistFailed`, the open error is returned by `IdentityStore::open`.
- Proposed fix: §21: widen `device/write_failed` to "a write to the data folder failed", raised by `open` too.
- Evidence: `store::tests::a_failed_write_stops_the_store`

## f2-7. The quarantine stamp's zone, and how its age is read
- Kind: spec-silent
- Where: device.md §7.4 (`<name>.corrupt-<YYYYMMDD-HHMMSS>`), §7.5 (removed after 30 days "by the device clock")
- Says / does: the zone of the stamp is not stated (§18.2's `pre-sync-` stamp is local time); nor whether the age is the stamp's or the file's modification time.
- Done here: UTC of the device clock (`now_ms` passed to `open`), and the age is read from the stamp in the name (`identity_dir::stamp`, `stamp_ms`).
- Proposed fix: §7.4: "the stamp is UTC of the device clock; §7.5 reads the age from it."
- Evidence: `store::tests::corrupt_files_at_open` (a 2020 stamp removed, a recent one kept)

## f2-8. Absent files at open other than `checkpoint.tdk` and `meta.tdk`
- Kind: spec-silent
- Where: device.md §7.4, the paragraph after the second table
- Says / does: only the absence of `checkpoint.tdk` and `meta.tdk` is ruled.
- Done here: `device.tdk` and `identity.tdk` absent are `device/store_corrupt` (both are written at creation); `bootstrap.tdk` absent with no checkpoint is `device/store_corrupt` (kind 17); an absent append file (`hashes.tdk`, `outbox.tdo`, `notices.tdk`, `history.tdk`, any segment) is empty, since the append procedure creates it at its first record.
- Proposed fix: §7.4: list those cases.
- Evidence: `store::tests::every_persist_op_through_disk_and_back` (the first reopen has no append file)

## f2-9. A `bootstrap.tdk` left beside a checkpoint
- Kind: decision
- Where: device.md §7.3 `WriteCheckpoint` ("then, if `bootstrap.tdk` exists, remove it"), §7.4 order (bootstrap before checkpoint)
- Says / does: a crash between the checkpoint's rename and the bootstrap's remove leaves both; §7.4 does not say which wins or whether to finish the remove.
- Done here: when `checkpoint.tdk` exists, `bootstrap.tdk` is not read (`Loaded.bootstrap` absent, a damaged one not reported) and is removed by the next `WriteCheckpoint`; no removal at open, so DEV-FILE-011 holds.
- Proposed fix: §7.4: "`bootstrap.tdk` is read only when there is no checkpoint."
- Evidence: `store::tests::every_persist_op_through_disk_and_back`

## f2-10. A gap in the log above the checkpoint
- Kind: spec-silent
- Where: device.md §7.4, DEV-STORE-006
- Says / does: DEV-STORE-006 binds the writer; §7.4 does not say what a reader does with records above the checkpoint that are not consecutive from `board_seq + 1` (a removed or renamed segment).
- Done here: such a tail is `device/store_corrupt` naming kind 1; below the checkpoint a gap (a quarantined segment) is allowed and those seqs are absent from `Loaded.chains`. Within a segment the pages must continue from the name's seq (else `record_format`).
- Proposed fix: §7.4: "the records above the checkpoint's seq must be consecutive from `board_seq + 1`, else `device/store_corrupt`."
- Evidence: `store::tests::segments_roll_over_truncate_and_quarantine`

## f2-11. How the store finds the history records of §8.3
- Kind: decision
- Where: device.md §8.3 rows 1 and 2, DEV-RUN-003, DEV-RUN-006; `IdentityStore::persist(ops, history)`
- Says / does: the store must write the history records of accepted control entries after an `AppendLog` and a snapshot's before the first `WriteCheckpoint`; it does not know which `Persist` follows a snapshot open.
- Done here: `persist` takes the sealer adapter's `history()` (ascending). After an `AppendLog` it appends the records above `history.tdk`'s last seq and at or below the op's last seq; before every `WriteCheckpoint` those at or below its `board_seq`. A snapshot's records (all at or below `S`) thus go before the rejoin's checkpoint, and nothing is written twice.
- Proposed fix: none.
- Evidence: `store::tests::history_records_follow_the_log_and_the_checkpoint`; `adapt::flow_tests::genesis_edit_restart_edit_on_disk` (history.tdk equals the sealer's after the restart)

## f2-12. `KeyLost` cannot show the old device's name
- Kind: spec-wrong
- Where: device.md §12.6 ("shows the old identity's board, `dev`, device name (from `store.tds` and the plaintext headers)"), §5.4 table row `KeyLost`
- Says / does: `store.tds` (§6.2) holds board, `dev`, key store kind and `created_ms`; the sealed headers hold `dev` and `board_id`. The device name is only in `device.tdk`, sealed under `L_d`, which is lost.
- Done here: `FolderState::KeyLost` carries the `StoreDescriptor` only.
- Proposed fix: either drop "device name" from §12.6, or add a `device_name vb8` to `store.tds` (and its length to §6.2, §22).
- Evidence: device.md §6.2 table

## f2-13. `read_identity_dir` has no build id, clock or file layer
- Kind: spec-unclear
- Where: device.md §25 `store::read_identity_dir(dir, l_d) -> (Loaded, Vec<HistoryRecord>)`
- Says / does: no caller is named; it cannot fill `Loaded.build_id`, has no device clock for the quarantine stamp and no `FileLayer`, and reading at open writes (tmp removal, tail truncation, quarantine).
- Done here: implemented over `OsFiles` and `IdentityStore::open`, with the board and `dev` of the directory's `store.tds`, an empty `build_id` and the system clock; documented as writing what §7.4 writes.
- Proposed fix: §25: name its caller, or replace it by `IdentityStore::open`.
- Evidence: `crates/taskdeck-device/src/store.rs` `read_identity_dir`

## h-1. The standby's origin must name its port, which port 0 only learns after the start
- Kind: misfit
- Where: relay.md §17.2 (`TD_RELAY_ORIGIN`, `TD_LISTEN`), §5.4 (the origin of an address is `https://` and its host, with the port); `relay/standby.js` `startStandby` (the origin is read once, by `makeConfig`); device.md §20.1, §19.1 (a relay on `127.0.0.1`, port 0).
- Says / does: a device derives the relay origin it signs in its login from the host of the relay address, which for the standby on loopback is `127.0.0.1:<port>` (§5.4, port written when not 443). The standby checks the login against the origin it was configured with. With `listen: '127.0.0.1:0'` the port is known only after `startStandby` returned, but the origin is needed to call it.
- Done here: `tests/e2e/harness/standby.rs` reserves a free loopback port (bind to port 0, read it, release it), starts the standby on that port with origin `https://127.0.0.1:<port>`, and retries up to five times if the port was taken in between. A small race remains, retried.
- Proposed fix: relay change: when `startStandby` is given `origin` with port `0` (or a `{port}` placeholder, or no origin and an `originFromListen` option), it takes the bound port into the origin after `listen`; or `startStandby` accepts `origin` as a function called with the port. Either removes the race for every test that wants port 0.
- Evidence: `smoke::standby_origin_names_its_own_host`.

## h-2. Decision: the harness stops the standby by closing its standard input
- Kind: decision
- Where: relay.md §17.2 (the standby serves until `SIGTERM`); `tests/e2e/harness/standby.rs`.
- Says / does: `node standby.js` stops on `SIGTERM` and prints nothing, so the port cannot be learned from it. The harness launches `startStandby` through `node --input-type=module -e`, which prints its `url` as one line and stays up until its standard input ends.
- Done here: `Standby::stop` (and drop) closes the child's standard input and waits up to 10 s, then kills it. Std's `Child` has no portable way to send `SIGTERM` without `unsafe` or a dependency, and the end of standard input also stops the standby if the test process itself dies.
- Proposed fix: none.
- Evidence: `smoke::standby_serves_several_requests_and_refuses_an_unknown_route`.

## h-3. §19.2 does not say how a result sits in the answer, nor what `wait` and an unknown `cmd` do
- Kind: spec-unclear
- Where: device.md §19.2 (the Answer line and the table's Result and Arguments columns); §17.3 (`wait_for_change(since_any, timeout_ms)`).
- Says / does: an answer is `{"id", "ok": true, ...result}`, which the harness reads as the result's fields at the top level (`{"id": 3, "ok": true, "wall_ms": …}`); for results the table calls "the status as record JSON", `Submitted`, "the counters", "a ticket" the spec does not say whether they are flattened into the answer or under one key. The table gives `wait` the argument `since` where §17.3 has `since_any`, and the counters as `Changes` (§17.3) whose field names the harness assumes to be the answer's top-level fields (`any`, `view`, …). `{"id": 0, "ok": false, "error": "device/ctl_request"}` is the answer to an invalid line; the answer to a valid line with an unknown `cmd` is not stated (the stand-in used by the harness's tests answers it with its own id and the same code).
- Done here: the typed helpers return the whole answer line and read `canonical`, `state_hash`, `sheet`, `address`, `recovery_fp`, `code`, `wall_ms`, `any` at the top level; `wait` sends `since` and `timeout_ms`. F3 writes the device; if it differs, the helpers of `tests/e2e/harness/ctl.rs` change, not the tests.
- Proposed fix: spec change in §19.2: "a result's fields are written in the answer beside `id` and `ok`; a record is written as its JSON object, `Submitted` and tickets as the fields of their records; `wait`'s answer is the fields of `Changes`; an unknown `cmd` is answered `{"id": <id>, "ok": false, "error": "device/ctl_request"}`".
- Evidence: `smoke::ctl_drives_a_device_by_the_protocol` (against the stand-in; no device exists yet).
