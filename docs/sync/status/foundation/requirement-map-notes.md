# Notes from the requirement map (2026-11-22)

Two mappers walked SYNC.md §3 to §9 against an index of all 942 clauses; a checker who did not write the map
compared every fourth row's first clause with the spec text (91 rows, 11 corrected) and added 6 missing rows.
The map has 493 rows before the last spec pass: 369 cite a clause, 119 a stage, 40 a row of SYNC_PLAN.md §2,
7 the plan's backlog (§6.13). `tools/build-map.py` assembles and checks it.

## Where the specs differ from SYNC.md in detail

The mappers noted these while mapping. Most follow from the plan's §2, from the lead's rulings S1 to S32 or
from the interface reviews; none was checked again here. The specs are the contract (SYNC_PLAN.md §3.3); a
stage that finds one of these wrong raises a spec change.

- §4.1 relay tables: SYNC.md lists log, devices, snapshots, blobs(id, part, data), tickets and meta. relay.md §7.1 (RELAY-STORE-001, 'exactly the tables') has other tables, such as nonces, read_tokens and separate parts rows, and RELAY-STORE-003 adds a chain column to log.
- §4.2 outcomes: SYNC.md says an unknown author or bad signature is skipped 'with a tampering alarm'. seal.md classifies steps 1-4 as Skip (SEAL-RCPT-002) and only a step-5 failure as Tamper (SEAL-RCPT-003). For Skip the engine raises skipped_frame without stopping (ENG-CLASS-002).
- §4.2 signature contexts: SYNC.md's table has frame, login, admin, hub and backup. SEAL-CTX-002 adds the ctl inner signature (SEAL-KIND-003), and the hub contexts are dropped (plan §2 Milestones).
- §4.3: SYNC.md says (dev, ctr) and entry_id replace the 1,024-reply cache. BRIDGE-VERB-003 keeps a 1,024-key held-reply cache for page verbs on the bridge. That cache is local, not on the relay, but the wording clashes.
- §4.5 golden hashes: SYNC.md checks them 'against the last three releases'. CORE-RULES-003 requires every later build to replay every released version's golden logs.
- §4.7 snapshots: SYNC.md has the Windows desktop write weekly or every 1,000 entries, with a fallback after a week. ENG-SNAP-001 and ENG-SNAP-002 use a 24-hour limit, ranks and no designated device, in line with plan §2 Always-on device.
- §4.7 compaction: SYNC.md says entries older than 365 days. ENG-COMPACT-002 uses 366 days of local clock, RELAY-COMPACT-003 uses 365 days of relay clock, and RELAY-COMPACT-007 also keeps the newest 16,384 seqs.
- §4.7 purge: SYNC.md deletes the old roots everywhere at purge. ENG-PURGE-003 deletes them only 30 days after the purge's rotation.
- §4.9 row 5: SYNC.md offers an optional hourly freshness alarm. BRIDGE-BG-002 forbids any alarm other than the reminder chain.
- §5.1 and §5.3 pairing bundle: §5.1 lists the bundle under HPKE, while §5.3 seals it under HKDF(shared secret, s, 'bundle'). SEAL-PAIR-009 seals it with bundle_key, an AEAD, not HPKE.
- §5.2 relay token: SYNC.md has HMAC-SHA256(TOKEN_KEY, role ‖ board ‖ dev ‖ gen). RELAY-TOKEN-001 has HMAC(TOKEN_KEY, ctx('token') ‖ token[0..37]), with a context prefix.
- §5.3 slot MAC: SYNC.md MACs context ‖ label ‖ sender role ‖ body. SEAL-PAIR-002 also covers tid and body_len.
- §5.5 recovery powers: SYNC.md says the recovery key can 'revoke any device'. seal.md (decided item 9, near line 2147) refuses a revocation that would leave no current device, even from the recovery identity, and RELAY-REVOKE-002 refuses it too.
- §3 row 11 and plan §2: bridge.md §13.1 lists managing subscriptions, the palette and the home place as desktop-only. SYNC.md §3 plans a native subscription screen on Android in Phase 8.
- The §4.1 checkpoint cadence (200 entries, 10 minutes, exit) and the weekly bases are mapped to ENG-CKPT-001 and ENG-CKPT-003 through their §18.1 and §18.2 cases. The exact numbers were not checked against engine.md.
- Log retention: SYNC.md §6.5 and §7 row 15 say '400-day logs', but RELAY-COMPACT-003 lets the relay compact entries older than 365 days and ENG-COMPACT-002 allows compaction outside a 366-day window. The rows cite those clauses, but the figures differ.
- Trust store: §7 row 10 says 'TLS from the OS trust store everywhere', but DEV-NET-002, DEV-GEN-011 and BRIDGE-TRUST-001 allow bundled webpki-roots (TrustRoots::Bundled) on some Android API levels. This follows from plan §2 Android floor (minSdk 26).
- Document-start script: §7 row 5 says it 'limits link rel values', but BRIDGE-DOC-002 makes creating any link (or base) element throw, which is stricter.
- Re-home rehearsal: §6.7 says CI rehearses the re-home on every release, but SYNC_PLAN §6.11 puts the move-board rehearsal in nightly.yml. Mapped to stage: release.
- Backup writers: §6.6 names the Windows desktop (and the Mac) as nightly writers. Plan §2 Always-on device assumes no device is always on. DEV-BKP-007 limits recovery-fingerprint pinning, and so backup writing, to desktops. Mapped with plan §2: Always-on device.
- Restore to a point: §6.6 says 'nearest weekly base'. ENG-CKPT-003 defines bases by three conditions in engine.md §18.2, and the index does not say whether these give weekly bases. Not checked against the spec.
- plan §2: Cost is used for the EUR 0 rules even though that row says 'Unchanged'. It restates that EUR 0 options are the default, and that makes the key-file fallback for APK signing the default.
- M1 and M2 migration steps in §9 map to plan §2: Milestones and plan §2: Migration source. Their surviving parts map to DEV-CREATE-001 to 005, DEV-PAIR-004, DEV-LOCK-004 and CORE-IMP-*. Owner steps map to stage: release (cutover runbook).

## Requirements that needed a ruling

Nine get a new clause (`resolutions/map-followups.md`). The others map to a stage or to the backlog by the
lead's ruling: the release profile's `panic = "unwind"` and the single RustCrypto generation are stage
`foundation`'s gates; vectors in CI are stage `seal`'s and the public review stage `rc`'s; disk encryption is
an owner step in stage `release`'s runbook; the optional hourly freshness alarm is not taken (BRIDGE-BG-002).
Backlog (SYNC_PLAN.md §6.13): the phone extras of Phase 8, push, cargo-vet. Two items are mapped to the
backlog although §6.13 does not name them, and the owner is told: the optional app lock on Android, and
bringing in another board's items later as ordinary commands.
