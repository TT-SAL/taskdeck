# Task S7b: the last small code changes

Read `docs/sync/status/seal/briefs/common.md` first. Paths below are relative to the working copy.
Tasks S1 to S6 are done and every module is built. These are the code changes still owed: rulings
of the lead that `seal.md` already states and the code does not yet do, and two things the passes
of earlier tasks asked for. Task S7 (the gate's completion) follows; its verify and security passes
read your changes too. Keep each change small and give each a test that fails without it.

## Read (one batch)

- `docs/sync/spec/seal.md` lines 1037-1105 (§16 login and admin signatures, the registry), 1262-1321
  (§17.10 the machines, §17.11), 1471-1506 (§20.4, §20.5), 1547-1555 (§21.3).
- The code: `src/blob.rs` (`open_snapshot`, `verify_body`, `seal_snapshot`), `src/pair.rs` (both
  machines' `on`, how each stores the slots it has accepted), `src/signatures.rs` (`sign_admin`,
  `admin_message`, `AdminOp`), `src/backup.rs` (`write_backup`), `src/prim.rs` (`deflate_split`,
  `compress_piece`, `inflate_raw`), and their unit tests.

## Build

1. **`open_snapshot` also recomputes the state hash** from the body's own `board` and `membership`
   under the expected version and requires it to equal the expected hash (`seal/blob_open`; ruling
   Q57 as completed, §20.5 step 7): a body whose board does not give its signed hash is refused
   whoever the caller is. It keeps comparing the body's `S`, `hash_version` and `state_hash` with
   the expected values. The test of task S6b that shows such a body opening turns round, and
   `seal-blob.json` gains the invalid case; no other case changes its result. The declared mode
   stays as it is.
2. **A pairing machine ignores a slot that is byte for byte one it has already accepted** (ruling
   Q60 as completed, §17.10: a relay answers a repeated read with the stored slot, and a runtime
   that polls and listens can deliver one twice). Any other slot its state does not await still
   aborts with `seal/slot_mac`. Tests for E (slot B again while showing, registering and done) and
   for N (each of A, C and D again in every later state), and that a repeat with one byte changed
   aborts. A machine that is `Aborted` still ignores everything.
3. **`seal_snapshot` refuses, with `seal/blob_open`,** a body with a history record above its `S`
   and a membership of another board than the one it seals for (ruling Q56; §20.4 says so), before
   it draws.
4. **`sign_admin` checks the relay nonce's length before anything else** (ruling Q18:
   `seal/challenge_length` first, also when the signer's board is wrong).
5. **`sign_admin` and `admin_message` refuse what the registry's words exclude** (ruling Q15,
   `seal/bad_payload`): `mint_read_token` for a `dev` that is not the signer; `reinstate` by a
   device signer, or with `first_epoch` below 2; `recovery_register` whose `dev` is not the `dev_R`
   of its `sign_pk`; a `seed` with more than 320 records. Read the registry in §16.2 for the exact
   words; where they do not exclude a value, do not refuse it, and say so in your report.
6. **`write_backup` proves its stream before it seals** (the defect task S5's security pass found
   shows only on some data, and a backup that does not read back is found only at a restore): the
   compressed head followed by an empty final stored block (`01 00 00 FF FF`) inflates to exactly
   the head, and the compressed tail inflates to exactly the tail; otherwise nothing is written
   (`seal/backup_payload`). The bytes from `n_roots` to the end of `read_token` do not pass through
   the decompressor. Compare while inflating, without a second copy of the payload. With it, a
   property test in the crate's unit tests: `deflate_split` read back by `inflate_raw` for random
   mixes of compressible and incompressible bytes (runs of both, in random proportions), with heads
   up to a few hundred kilobytes.

## Not in this task

The tamper, context and parser matrices, the fuzz targets, the budget job and the clause coverage:
task S7. No signature of §26 changes. Two low findings stay as they are and are not yours: the
lookup of a typed character in `base32`, and `seal_frame` under epoch 0.
