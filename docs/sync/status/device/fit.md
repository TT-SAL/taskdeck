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
