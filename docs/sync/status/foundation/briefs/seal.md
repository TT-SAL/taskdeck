# Brief: `docs/sync/spec/seal.md` (owning stage: `seal`)

Scope (SYNC_PLAN.md §6.1 item 3): "frame, envelope, signature contexts, key hierarchy, membership and every
control entry (including the recovery-key change and its two-member path), chain, pairing, sealed files,
backup envelope (§4.2, §4.8, §5)". The `seal` stage builds all of it "as pure functions and sans-IO state
machines" (§6.3) and publishes vectors under `docs/sync/vectors/`. `relay` verifies frame, login and admin
signatures from your layouts in JavaScript (§6.4). `recovery` builds the backup reader and writer and the
recovery sheet from you (§6.8). `engine` drives your membership machine through the sealer trait.

## Sources
SYNC.md §4.1, §4.2, §4.3, §4.7, §4.8, §5 (all), §6.6, §6.7, §7, §9 and "Rejected or modified verifier
findings". docs/sync/research/R4 §5–§8. docs/sync/research/V1 in full (findings 1, 2, 3, 5, 6, 10, 11, 12,
13, 14, 16, 17 change this spec). V2 findings 3, 7, 8, 9, 15, 21. SYNC_PLAN.md §2, §3.4, §6.3, §6.4, §6.8.

## Must contain
1. **Context-string registry.** Every context string in the system, its use, and which spec uses it:
   frame, login, admin, backup, entry (AEAD AD), chain, board, epoch, wrap, pair, pair-transcript, sas,
   state (S7), door (S4), the recovery identity's dev derivation (S3), the HKDF infos for per-kind keys,
   ticket / mac / bundle derivations in pairing, sealed-file and snapshot/blob AD, the recovery-seed
   derivations, the fingerprint. The M1 hub contexts are dropped; say so. Other specs may only use strings
   listed here.
2. **Frame**: `"TDF1" | board_id[16] | dev[16] | ctr u64 | len u32 | body | sig[64]`, the 256 KiB cap, the
   exact signed bytes, what `len` counts.
3. **Body v1**: `ver | suite | epoch | nonce[24] | ciphertext`, the exact AD, key selection by log position.
4. **Plaintext**: `entry_id[16] | kind u8 | at_ms i64 | based_on_seq u64 | based_on_chain[32] | payload | padding`.
   Fix how the payload's length is carried, the padding rule exactly (512-byte buckets up to a stated
   threshold, Padmé above, with the formula), the pad bytes, and how unpadding validates them.
5. **Kinds and payloads** (kind numbers; JSON schemas in canonical encoding, which `core.md` defines):
   `cmd` (opaque to seal: one core log command); `ctl` (an ordered list of control items, S5); `beat`
   (head, state hash at the latest seq ≡ 0 mod 64, supported rules, envelope and hash versions, snapshot
   acknowledgements or mismatches); `snap` (S, state hash, blob id, SHA-256 of the sealed bytes, sizes, part
   count). Control items, each with its full schema: `Genesis` (S2), `DeviceAdded`, `DeviceRevoked`,
   `DeviceReinstated`, `EpochRotated`, `RecoveryKeyChange` (direct with the current sheet; propose,
   confirm, cancel without it), `Fence`, `ResetTo`, `Skip`, `RulesUpgrade` (with its override form).
6. **Key hierarchy**: recovery seed (256 bits) and exactly how the recovery HPKE key and the recovery
   signing key derive from it; `board_id`; the recovery identity's `dev` (S3); device keys (Ed25519 and
   X25519, generated from the OS RNG only); epoch roots, the commitment `c_e`, `K_kind,e` for entry, snap and
   blob; the per-device storage key `L_d` and the per-file-kind subkeys. The relay token is an opaque bearer
   string to clients: its layout belongs to `relay.md`; say that.
7. **HPKE wraps**: exact `info`, `aad`, recipient, and the byte layout of a wrap, for devices and for the
   recovery key; what binds use, board, epoch and recipient (V1-12).
8. **Membership state machine**: the state (device set with dev, keys, name, role, `device_no`, status and the
   seq of each change; recovery keys; epochs and commitments; a pending recovery-key change; fence; active
   rules version; per-device counters) and a transition table for every control item: who may author it
   (a member, a member other than X, the recovery identity), preconditions, effects, refusal reason.
   Cover: `device_no` assigned at apply time as the next free number in log order; the first
   `EpochRotated{e+1}` wins and later ones are refused everywhere; the commitment check and the halt on
   mismatch; revoked devices' later entries; a stale-epoch entry accepted only from a current member, based
   before the rotation, within 30 days (S6); reinstating a device another device revoked; `Skip` never
   targets a control entry and is signed by a member other than the author or by the recovery identity;
   `RulesUpgrade` valid only if every active member supports it (active = a beat within 30 days, S6) and its
   two-member override; the recovery identity's powers and limits (it never enrols a device); the
   72-hour recovery-key change (S6). Define the canonical membership bytes that feed the state hash (S7).
9. **Receipt checks**, in order, each with its outcome class: skipped with a tampering alarm and no counter
   consumed (unknown author, bad signature); refused on every device, the author's included, with an alarm
   naming the author (validly signed, fails a later check before `apply`); the three halts; "a frame under
   this device's dev that it never wrote" (V1-1); `based_on` at or after the entry's own seq or off the local
   chain is tampering; older than the retained log is skipped with a warning (V1-16, V2-15).
   `engine.md` owns what the replica then does; you own the classification and its inputs.
10. **Hash chain**: `C_0`, `C_s`, the head, the fingerprint (seq plus four words). Choose the word list,
    name its source and licence, give the exact selection rule from `C_s`, and write the list into the
    repository as `docs/sync/spec/wordlist.txt` only if you can fetch it from its official source and state
    its SHA-256 (you may create that one extra file). Also the key fingerprint shown in banners.
11. **Signatures**: the login message, the admin message (the registry of operation names and exactly how
    arguments are encoded and hashed), the backup signature; refusal of any challenge that is not exactly
    32 bytes (V1-1).
12. **Pairing**, both roles, complete: the code (80 bits as 16 Crockford base32 characters plus 2 check
    characters; define the check), ticket and key derivations from it, slots A to D with byte layouts and
    MAC inputs (label and sender role in every MAC), the transcript `T`, the SAS (exactly how 6 digits come
    from the bytes), copy-and-enter order, the bundle (key, AEAD, schema: board id, relay address (S4),
    current and previous epoch roots, device set, head, a fresh snapshot of E's state with its hash, the
    recovery public keys, the new device's first token), the registration E performs, every abort condition,
    try counts and timeouts.
13. **Recovery sheet encoding**: 52 Crockford base32 characters plus 4 check characters, grouping, the
    check algorithm, case and look-alike handling on input, what else the sheet prints (S4).
14. **Sealed local files**: the envelope every local file uses (header, nonce, AD = file kind ‖ dev ‖
    board_id, key), how appends to a log file are sealed, the registry of file kinds. The contents of
    each file belong to `device.md`.
15. **Snapshots and blobs**: one AEAD message split into parts of at most 1 MiB; the blob id (note: an id that
    is the hash of the sealed bytes cannot also be inside their AD; resolve this exactly); the AD; the
    hash check before decryption; the inflate cap (V1-12, V1-14).
16. **Backup envelope** (`.tdbk` and the monthly segment): layout, what is signed, what is HPKE-sealed to the
    recovery key, the payload schema (canonical board, sync metadata, device set, epoch roots, backup-read
    token, the membership history a reader needs), and the exact order in which a reader authenticates
    before it parses, back to the self-certifying board id (V1-13). A writer must not be able to read its own
    backup back.
17. Error catalogue, Limits, Test vectors (families: frame, envelope, padding, key derivation, wrap,
    commitment, chain, membership transitions, pairing transcript and SAS, sheet encoding, sealed file,
    snapshot, backup; plus the known-answer suites of SYNC_PLAN.md §6.3), Interfaces, Public API (the Rust
    types and functions the `taskdeck-seal` crate exposes: pure functions and sans-IO state machines; no
    I/O, no clock, randomness passed in or drawn only where the spec says), Open questions.

## Gaps to close (decide, and say so)
- The `ctr` and `dev` of recovery-authored frames; whether control entries consume the author's counter like
  any entry (they do unless you find a reason).
- Under which epoch the entry carrying `EpochRotated{e+1}` is sealed, and from which seq the new epoch applies.
- What exactly a device signs when it "registers N's key in the same call" as `DeviceAdded`.
- How `Fence` names the new relay (S4) and the generation, and how the 8-character move code is derived and
  checked on the other devices.
- How `ResetTo{seq, state hash}` and `Skip{seq, frame_hash}` identify their targets.
- Anything a receiver needs to verify a frame at a log position that the frame alone does not carry.
