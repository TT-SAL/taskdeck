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

## f3-1. The manual clock starts ten months before the standby's clock, and the clock guard blocks composing
- Kind: misfit
- Where: device.md §19.1 (`--clock manual`, "starting at 2026-01-05T12:00:00Z"), engine.md §27.2 (ENG-CLOCK-002), relay/standby.js (`now = Date.now`)
- Says / does: the standby stamps `relay_now` with the real clock, the device's manual clock starts at 2026-01-05; the skew is months, so the engine refuses every `Enqueue` with `engine/clock_unconfirmed` and composes no beat. `confirm_clock` does not help: the confirmation lasts only while the skew stays within five minutes of the confirmed one, and every `clock advance_ms` moves it.
- Done here: the end-to-end tests set each device's manual clock to the real time right after it starts (`clock set_ms`), and `set_ms` moves `wall_ms` alone, not `mono_ms`, so that the jump is not months of monotonic time (`ctl::ManualClock`). A test's steps (30 s per poll) keep the skew under an hour.
- Proposed fix: device.md §19.1: "`--clock manual` starts at the host's clock when `run` starts (or at `--clock manual:<ms>`); `clock set_ms` sets the wall clock and leaves the monotonic clock; `advance_ms` moves both." Or the harness gives the standby a clock the test drives (`startStandby({ now })` exists).
- Evidence: `tests/e2e/first_sync.rs` before the change: `{"error":"engine/clock_unconfirmed","id":16,"ok":false}` answering the first `submit`

## f3-2. Decision: with `test-trust`, a loopback host is reached over `http`, chosen by the host
- Kind: decision
- Where: device.md §11.1 item 1, DEV-NET-001, DEV-TRUST-004; `RelayTarget { host, door, home_gen }` (engine.md §37) carries no scheme; `net::http::origin_for`, `create::relay_host`
- Says / does: §11.1 allows `http` relay targets on `127.0.0.1`, `[::1]` or `localhost`; a stored relay target has only a host, so nothing records which scheme a board was created with.
- Done here: with the feature, a host whose name is `127.0.0.1` or `localhost` is reached over `http`, every other over `https`; without it, every host over `https`. `begin_create` takes `https://host`, a bare host, or (with the feature, loopback only) `http://host`; any other `scheme://` is `device/insecure_url`. `[::1]` cannot be a host of `seal.md` §5.4 (no brackets), so it is not offered. The login origin stays `relay_origin(host)` = `https://<host>`, which is what the standby signs (h-1).
- Proposed fix: device.md §11.1 item 1: "the scheme is not stored: with the feature, a relay target whose host is `127.0.0.1` or `localhost` (with any port) is reached over `http`; `[::1]` is not a host of `seal.md` §5.4."
- Evidence: `net::http::tests::origins`; `first_sync::one_device_round_trip`

## f3-3. `board` in devicectl: the state hash exists only for the confirmed state
- Kind: spec-unclear
- Where: device.md §19.2 row `board` ("`view().board()`", "the board bytes and the state hash with the current membership")
- Says / does: `view().board()` is the pending view (§4.4), which has no membership seq to hash with; engine.md §18 defines the state hash of a confirmed state and `M_B`.
- Done here: `board` answers the confirmed board's canonical bytes and `state_hash(1, board_hash, M_B)`, from the published view (`Published::confirmed_state`, a new read beside `board()`); in local-only mode it is `device/needs_sync`.
- Proposed fix: §19.2: "`board` | `view().confirmed_state()` | the confirmed board's bytes and the state hash of it and `M_B`"; add `Published::confirmed_state(&self) -> Option<(&[u8], [u8; 32])>` to §25.
- Evidence: `first_sync::one_device_round_trip` (equal before and after the restart)

## f3-4. Decision: the https thread starts at the first HTTPS request in any mode
- Kind: decision
- Where: device.md §4.2 (`taskdeck-https`: "synced mode, at the first HTTPS request"), §18.1 steps 3 and 8, §18.3 (the joining device's requests), DEV-THREAD-002
- Says / does: a create and a join send requests before the device is in synced mode, and the sync thread may not do network I/O.
- Done here: one `taskdeck-https` thread per facade, started at the first request of any mode (`runtime::Net::submit`), joined by `shutdown`. Engine effects, the create flow's `door` and `create` (waited for on the caller's thread) and both pairing machines' requests (answered to the sync thread's queue) all go through it, one at a time in queue order.
- Proposed fix: §4.2 table, `https` row, Started: "at the first HTTPS request, in any mode".
- Evidence: `first_sync::one_device_round_trip`

## f3-5. Decision: a token the transport learns is made durable by the sync thread while the transport waits
- Kind: decision
- Where: device.md §10.3 (DEV-LOGIN-001: durable before use), §4.2 (`https` never does file I/O; the sync thread never waits for another thread)
- Says / does: the login runs on the https thread, which may not write `device.tdk`; the token must be durable before the next request uses it.
- Done here: the transport hands the token to the sync thread (`net::Back::StoreToken`) and blocks until the sync thread has replaced `device.tdk` and acknowledged; the same for the `push` learned from `caps`. The tokens are typed in `DeviceSettings.tokens` (`store::RelayToken`, the token `Zeroizing`, `Debug` without it; the record's JSON value is wiped after encoding and decoding), closing f2-4 for `tokens`.
- Proposed fix: §10.3: "the transport hands a token to the sync thread and sends nothing with it before the sync thread has made it durable."
- Evidence: `store::tests::settings_tokens_round_trip`; `first_sync::one_device_round_trip` (the create token, then `caps` storing `push: false`)

## f3-6. Shortcuts of this milestone in the transport and the runtime
- Kind: decision
- Where: device.md §10.5 to §10.8, §11.1, §19.2; `net`, `runtime`, `ctl`
- Says / does: the stage's plan defers the socket, backoff and budgets (status file, decision 3).
- Done here (each a shortcut to undo later): no socket thread: `Connect` always ends in `Connected { push: false }` and every relay is polled over HTTPS, a push relay too; a `Connect` after a failed one waits a fixed 1,000 ms instead of the backoff of §10.7; no request budget (§10.8; `requests_today` is 0); `MissedRoots` is never sent (a login's `wraps` is parsed by presence and dropped: no reinstatement yet); a blob upload sends every part and does not resume from `blob-meta` (DEV-NET-010); admin operations other than `register` and `revoke` are answered `device/not_built`; devicectl answers every command of §19.2 other than the seventeen F3 built with `device/ctl_request`, writes one `changed` event per change it sees (a thread waiting on the counters, so close publishes coalesce), maps only part of the engine's `Status` in `status`, and refuses `--trust-pem`, `--crash-at` and `--zone table:`; `DeviceStatus.backup`, `.picture`, `.weather_age_ms` are empty; `release_fp` is empty (no release key yet); the local-only board is not loaded (`view().board()` is empty there).
- Proposed fix: none (the plan's later tasks).
- Evidence: the code comments at each place (`net/mod.rs` module doc, `ctl.rs` module doc)

## f3-7. Codes the spec does not name
- Kind: spec-silent
- Where: device.md §21; §18.1 step 1 (a create secret not of 64 lowercase hex), §17.10 `join_sas`, §18.3 (a pairing request with no answer)
- Says / does: these failures have no code.
- Done here: `device/create_secret` (a malformed create secret, before any request), `device/sas_format` (digits that are not six ASCII digits after removing spaces and hyphens; costs no attempt), `device/relay_unreachable` (a pairing or create request with no answer, as `Aborted { code }`), `device/relay_refused` (a relay refusal of a pairing request whose code is not one of the pairing routes'; `PairState::Aborted` holds a `&'static str`), `device/not_built` (above).
- Proposed fix: §21: add `device/create_secret`, `device/sas_format`, `device/relay_unreachable`; give `PairState::Aborted` a `String` code so a relay code passes through as it is.
- Evidence: `pairing::code_of`, `create::begin`

## f3-8. Decision: a write's reply is sent after the turn's publish
- Kind: decision
- Where: device.md §4.3 (steps 2 to 4), §4.5, DEV-THREAD-006, DEV-THREAD-007, DEV-FACADE-002 ("the view version that includes it")
- Says / does: §4.3 publishes at the end of the turn and §4.5 has a write block until its reply; nothing orders the two. A caller whose `pair_start` had returned read `status().pairing = Idle`, and a `submit`'s `view_version` could be ahead of `view()`.
- Done here: every reply of a facade request (and the `Enqueued` answer of a `submit`) is queued during the turn and sent after the turn's publish (`runtime::later`, `Runtime::deferred`), so a read made after a write returned sees what the write changed.
- Proposed fix: §4.3 step 4: "…then send the replies of the requests this turn served"; DEV-THREAD-007: "and MUST send a request's reply only after the publish of the turn that served it".
- Evidence: `first_sync::first_sync` failed 2 runs in 3 (`left: "idle"`, `right: "showing_code"` after `pair_start`) before the change, and passed 10 in 10 after

## f3-9. Decision (CHANGES.md, pairing runtime, points 1 to 6)
- Kind: decision
- Where: `docs/sync/spec/CHANGES.md` row 2026-11-23 "Left open by stage `seal` for the pairing runtime"; device.md §8.4, §18.3; seal.md §17.10, §17.11, SEAL-PAIR-013; `pairing.rs`
- Says / does: six points left to `device`.
- Done here: (1) a `PairFailed` for the pairing's ticket sets `status().pairing = Aborted { code }` with the engine's code whatever E's machine state, `Done` included (the machine, if not ended, is cancelled first): the user never sees success on E for a device that was not added. (2) N's `ReadLog { from }` is performed as `read` pages from `after = from − 1` (limit 256) until `more` is clear, then one `Entries` with every entry; `FetchFrames { seqs }` as one `read` with `after = t − 1, limit = 1` per seq, in order, then one `Frames`. (3) A ticket is deleted (`DELETE …/pair/<tid>`) only when E's machine emits `DeleteTicket`; `Abort` alone deletes nothing. (4) After an abort that follows `Registered`, the runtime does nothing more: the device that never joins shows in the device list with no last beat and the user revokes it, as after a crash while registering (§18.3). (5) Each machine gets `Tick { now }` with the device's monotonic time every 1,000 ms while it is neither `Done` nor `Aborted`. (6) `join_sas` removes spaces and hyphens and hands on exactly six ASCII digits; anything else is refused with `device/sas_format` before the machine sees it, so it costs no attempt.
- Proposed fix: device.md §18.3 and §8.4: state the six as above.
- Evidence: `first_sync::first_sync`

## f3-10. E's `RegisterFailed` takes a seal code; the engine reports a pairing's failure with its own
- Kind: misfit
- Where: seal.md §17.10, `ExistingEvent::RegisterFailed { code: SealError }`; engine.md §30.3 `Notification::PairFailed { ticket, code: &'static str }` (codes `engine/…`, `relay/…`, `seal/…`); device.md §8.4
- Says / does: the engine's code cannot be made a `SealError`, so `PairFailed` cannot be handed to the machine as `RegisterFailed`.
- Done here: on `PairFailed` the runtime cancels the machine (`Cancel`: `DeleteTicket`, `Abort { seal/pair_cancelled }`) and then shows the engine's code (`Aborted { code }`, decision f3-9 point 1).
- Proposed fix: seal.md §26: `RegisterFailed { code: &'static str }` (the machine passes it to `Abort` unchanged), or device.md §18.3 states the cancel as above.
- Evidence: `pairing::Runtime::pair_failed`

## f3-11. `pair_start` checks the engine's refusals itself, and returns once the ticket is registered
- Kind: decision
- Where: device.md §17.10 `pair_start` (errors `engine/not_eligible`, `engine/halted`, `engine/pair_busy`; long, 20,000 ms), §18.3; engine.md §28.4 (refusals of `PairRegister`)
- Says / does: the engine sees the pairing only at `PairRegister`, after the user confirmed on E; `pair_start`'s errors are the engine's refusals of that input.
- Done here: `pair_start` refuses before `Existing::start` with `engine/halted` (a halt), `engine/not_eligible` (no `device_no`, a stop, or a session other than `Live`) and `engine/pair_busy` (a pairing not ended); then it answers once the relay has answered `pair-open`, so that the code shown is one the new device can use (a refusal there aborts the machine and is returned).
- Proposed fix: §17.10: "`pair_start` returns after the relay stored the ticket; its refusals are checked against the engine's status before the machine starts."
- Evidence: `first_sync::first_sync`

## f3-12. `PairState` has no state for the new device before it is asked for the digits
- Kind: spec-unclear
- Where: device.md §18.3 tables, §25 `PairState`
- Says / does: N's table names `AskSas`, `Accepted`, `Joining`, `Done`, `Aborted`; between `join` and `AskSas` (reading slot A, posting B, waiting for C) and while it reads slot D, the snapshot and the log nothing is named.
- Done here: `Joining` from `join` until `AskSas`, again from `Accepted`'s slot D on, and until the engine has applied the `DeviceAdded` and has a `device_no` (then `Done`, engine.md §19.8). devicectl's `pair_status` answers `{"state": snake_case name, "sas"?: digits}` and, for an aborted pairing, `"reason": code`.
- Proposed fix: §18.3: say so, or add `PairState::Connecting`; §19.2 `pair_status`: add `reason`.
- Evidence: `first_sync::first_sync`

## f3-13. The store writes a joining device's history before its bootstrap; the sealer builds its identity from the bundle
- Kind: decision
- Where: device.md §18.3 "Persisting the join", DEV-PAIR-006, SEAL-KEY-013; `store::NewIdentity`, `SealSealer::join_identity` (F1's open item "a joining device's constructor from a pairing bundle")
- Says / does: §18.3 lists `history.tdk` among the identity directory's files but `create_identity` wrote none; the identity bytes of a joiner had no constructor.
- Done here: `NewIdentity.history` (ascending `(seq, HistoryRecord::to_bytes)`), appended to `history.tdk` after `device.tdk` and before `bootstrap.tdk`; empty for a creating device. `SealSealer::join_identity(board, keys, roots, membership)` checks every bundle root against the commitment the snapshot's membership records for its epoch (a root without one, or a mismatch, is `seal/bundle_invalid`) and lays out `identity.tdk` with no pending root and no anchor. `create_identity` also writes `pending.tds` for a join, which `activate` then removes; the plain files of a local-only board are not set aside (§18.4 is not built).
- Proposed fix: §18.3: "…`history.tdk` (written before `bootstrap.tdk`) … `pending.tds` as for a create, removed by the activation."
- Evidence: `first_sync::first_sync` (the joined device restarts from its files and holds the board with the same state hash)
