# Task S6b: three small changes to the API of snapshots and backups

Read `docs/sync/status/seal/briefs/common.md` first. Paths below are relative to the working copy.
Tasks S1 to S5 are done and merged. Task S6 (pairing) is implemented and committed but not yet
verified: its verify and security passes run after this task and will read your changes too. Change
`src/pair.rs` only as item 1 needs. The lead ruled the three changes after the second check of the
spec change (rulings Q57 to Q59); `seal.md` §20.5, §21.6 and §26 already say them.

## Read (one batch)

- `docs/sync/spec/seal.md` lines 1490-1506 (§20.5), 1607-1634 (§21.6), 2092-2131 (§26: blobs,
  snapshots, backups).
- The code: `src/blob.rs`, `src/backup.rs` (`agrees`, `read_backup`, `HistoryRecord`,
  `VerifiedBackup`), the place in `src/pair.rs` that calls `open_snapshot`, and every test and vector
  module that calls `open_snapshot` or reads a `VerifiedBackup` (grep for both names under
  `crates/taskdeck-seal`).

## Build

1. **`open_snapshot` takes the expected state hash and its version** (§26: `state_hash: &[u8; 32]`,
   `hash_version: u32`, after `s`). Step 7 of §20.5 as it now reads: the body's `S`, `hash_version`
   and `state_hash` equal the expected values (`seal/blob_open`). The crate no longer recomputes the
   state hash from the body's board and membership in `open_snapshot`, and no longer checks the
   version against the membership's rules there: the engine recomputes the hash from the board it
   loads (a body whose board does not give its signed hash is the engine's to find, with its own
   alarm), and the signed entry's version was checked when the entry was classified. The declared
   mode compares neither value, as before. `src/pair.rs` hands `open_snapshot` the bundle's `hash`
   and `hash_version`; if it compared them itself after the call, that comparison goes.
   Tests: a body whose state hash differs from the expected one; one whose version differs; a body
   whose board does not give its own state hash now opens (say so in the test: the engine finds
   it). A test of the removed checks goes with them. Vectors: every `open_snapshot` case of
   `seal-blob.json` gains the two expected values as inputs, and one invalid case is added for each
   of the two comparisons; no other case changes its result.
2. **The agreement of a membership with its history names the board id** (§21.6 step 11, §20.5
   step 7): `agrees` also compares the two board ids. Through bytes this cannot be reached today
   (the parser ties the board id to the salt, the first recovery key and every recovery record's
   id), so the test builds the differing state directly.
3. **`read_backup` returns the verified history**, and a history record shows what its readers
   need (§26): `VerifiedBackup` gains `pub history: Vec<HistoryRecord>` (the verified records, in
   order), and `HistoryRecord` gains `dev()`, `ctr()`, `frame_hash()` and
   `items() -> Result<Vec<CtlItem>, SealError>` (`seal/backup_history` when the items do not parse,
   which a record that was only read with `from_bytes` may do). `device.md` §15.8 reads all of these
   from "the backup's verified history". Tests: `read_backup` returns records equal, byte for byte
   (`to_bytes`), to those the file was written with; the accessors against a record made by
   `from_accepted`; `items()` on a record whose items are no canonical item list. Vectors: the
   successful reads of `seal-backup.json` gain the history as an output (each record's bytes).

## Clauses the tests cite

SEAL-BLOB-007 (step 7's comparisons), SEAL-BKP-003 (step 11's agreement), SEAL-BKP-010 (a record's
fields). A citation claims the test fails when the clause is violated.

## Not in this task

Anything else: no other signature changes, `seal_snapshot`'s further refusals are task S7's.
