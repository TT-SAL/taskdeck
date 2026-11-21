# V2: Sync and data integrity (adversarial verification)

> Working paper from the design run of 2026-11-20 that produced [`SYNC.md`](../../../SYNC.md). It is an agent's report, kept for the detail the design compresses: file and line references, sources, and the reasoning behind each requirement. Where it disagrees with `SYNC.md` or [`SYNC_PLAN.md`](../../../SYNC_PLAN.md), those win. Line numbers refer to `day-planner` at commit `1cfff2f`.

Verdict: **sound with changes**. Issues are numbered as `SYNC.md` cites them (V2-1, V2-2, …). Each was raised against the draft design; `SYNC.md`'s *Verification outcomes* says what changed.

## V2-1 · high · Validation and default rules inside apply are consensus-critical, and nothing versions them

**Scenario.** After M2, a routine release changes one rule: NAME_MAX_CHARS goes from 200 to 300, a sixth severity level is added, the IP-literal/private-host refusal moves into checked_url (as R4 suggests), the notes cap is fixed to count after detab, or a default subscription colour changes. The phone is updated by cable weeks later. Meanwhile it receives Rename{a 250-char name} or AddSubscription{https://10.0.0.5/x.ics} from the desktop.

**Why it fails.** Lowest-schema stamping (design 4.8) decides the stamp from which ops and fields are present. These entries therefore carry the OLD schema, and the old phone applies them under old rules instead of halting. One build accepts the entry, the other refuses it. Replicas diverge, heartbeat hashes disagree, and per 6.4 sync halts on every device until someone picks 'adopt'. 'An op's meaning never changes' cannot hold for bug fixes, and apply is full of rules: checked_name length and str::trim (which follows the toolchain's Unicode tables), the severity/horizon table lengths, NOTES_MAX_CHARS, SUBSCRIPTIONS_MAX, checked_url, the name_from_url/placeholder_name/DEFAULT_COLORS defaults, the 1970-9999 year window, and the MIN_BLOCK/SNAP minutes.

**Required change.** 1) Thin apply: the origin computes every default (the names 'New task' and feed host, colours, horizon/severity seeds, fallback ids) and puts it in the command; apply checks only structural invariants. 2) Add a rules version. Any change to apply semantics or validation bumps it. The writer stamps max(lowest schema, rules version the values need), or activates the new rules with a sequenced RulesUpgrade{v} control entry once every active member's heartbeat reports support. A device halts on rules it does not know. 3) Golden logs with boundary values for every validated field, replayed by every release in CI. 4) Keep Unicode-table-dependent functions out of apply: trim at the origin, or trim ASCII whitespace only.

**Evidence.** board.rs:429-435, 446-460, 979-984, 1165-1166, 1178-1179, 1253-1256, 1321-1340; tasks.rs:29,37; subscriptions.rs:72, 156-180, 283-302; planner.rs:28,32; utilities.rs:135,147 (the cap is checked before detab and one tab becomes four spaces, so state can exceed the cap). [VERIFIED by reading]

## V2-2 · high · The state hash is not stable across versions, so ordinary releases will trip the determinism alarm and halt sync

**Scenario.** A release adds a field to BoardData or Session (a sid, a board setting with a default) or reorders struct fields. The updated desktop and the not-yet-updated phone apply the same entries to the same logical state.

**Why it fails.** The state hash is SHA-256 over canonical JSON (4.5), and heartbeats compare it (4.2, 6.4). serde_json writes fields in declaration order, and writes defaults and None as values unless skip_serializing_if is set; Active.planned_start, for example, is written as null. The two builds therefore hash different bytes. 6.4 then says 'sync halts and offers adopt Desk's state', and after adopting, the next heartbeat mismatches again. The design calls version skew normal (4.8), so this fires on routine releases. The same habit breaks lowest-schema stamping: a new build that writes 'field': null makes every entry need the new schema, and with deny_unknown_fields old devices halt on all of them.

**Required change.** Define a versioned canonical encoding: sorted keys (JCS-style), fields equal to their default omitted, one timestamp format, and unreadable archive lines as base64. Heartbeats carry hash_version and the highest applied schema, and only matching pairs are compared. Add CI golden-hash tests against the last N released builds. Serializers omit new fields at their default, so entries stay at the lowest schema. The hash also covers the replicated sync metadata: device set, epochs, per-device counters and id marks.

**Evidence.** tasks.rs:196-203 (serde(default) without skip_serializing_if); Cargo.toml:45 (serde_json without preserve_order, so field order is declaration order); design 4.2 (deny_unknown_fields), 4.5 (state hash), 6.4 (determinism-bug row). [code VERIFIED; consequence ASSUMED from the design text]

## V2-3 · high · Crash and restored-copy handling duplicates commands and can reissue ids

**Scenario.** (a) Android kills the app after the frame for ctr c has reached the relay but before the sealed frame was persisted. (b) The MacBook's disk dies, and Time Machine restores Application Support/TaskDeck and the login keychain. The same device identity comes back with an old outbox, an old ctr and an old id counter n. (c) Any 409 conflict.

**Why it fails.** (a) On restart the entry is still unsealed, so 4.3 'renumbers it above max(local, last_ctr)' and sends it again: it is applied twice. (c) 4.3 answers a 409 conflict by re-sealing under a fresh counter, so a command that was already sequenced is applied a second time. Only creating commands are idempotent by construction, because they carry ids. Unplan, MoveBlock/MoveBlocks, SetDeadline/SetEstimate/Rename and AddBlock of a since-removed sid silently revert newer edits, and a replayed Unplan wipes blocks added later. (b) apply refuses an id only if it is 'already taken'. An id whose item has since been archived or forgotten is free again, so the restored counter reissues it, and other devices' queued commands for the old item land on the new one.

**Required change.** 1) State the invariant seal, then fsync, then transmit, and test it in the simulator. 2) Put a random 128-bit entry_id in every plaintext. On hello, when last_ctr exceeds the highest ctr recorded locally, fetch this device's own entries in that range and decrypt them. Drop outbox entries whose entry_id is already sequenced, before renumbering anything. 3) On a 409 conflict, fetch the stored frame and treat a matching entry_id as the ack; never re-seal blindly. 4) Replicate per-device high-water marks for minted ids and sids in BoardData. apply requires the new n to exceed the mark; the range check applies only to newly minted ids, never to the original id kept by Restore. Devices advance their counters from the mark after each catch-up. 5) Detect a restored copy at start (the relay's last_ctr or the replicated mark is ahead of local) and catch up fully before sending.

**Evidence.** design 4.3 (client rules, 409 handling), 4.4; board.rs:1104-1114 (Unplan clears whatever exists at apply time), 1037-1042 (Forget frees the id); sync.rs:500-510 (today the key is minted at queue time and survives restarts, the property being lost). [code VERIFIED; failure ASSUMED from the design's stated rules]

## V2-4 · medium · Zone reads remain in apply's validation and in the wire types, and the proposed clippy rule cannot catch them

**Scenario.** A compromised member, or a mistyped date, sends SetDeadline{1970-01-01T00:30:00Z}. The Helsinki desktop reads the local date 1970-01-01 and accepts it; the laptop in New York reads 1969-12-31 and refuses it. The 9999 upper bound behaves the same way. MoveBlock and Create on a routine also still resolve day+minutes in the local zone, purely as validation.

**Why it fails.** sane_instant checks the year with at.date_naive() on a DateTime<Local>. MoveBlock and Create call planner::resolve_on_day (Local.from_local_datetime) for every kind before branching, so a routine move or create is refused only on devices where that local time does not exist. wire::optional_local resolves naive strings while deserializing, so any naive time that reaches a log entry is read in each reader's own zone. Disallowing Local::now and Utc::now (4.5) does not catch date_naive(), hour(), with_timezone(&Local) or DateTime<Local> deserialization. Divergence is detected only by heartbeats, and the design halts sync when that happens.

**Required change.** Core types use DateTime<Utc> (or a newtype), and taskdeck-core sets clippy disallowed-types for chrono::Local and chrono::offset::Local, not only disallowed methods. Check year bounds in UTC. Keep intent types (naive times allowed, compiled at the origin) separate from log command types, which accept RFC 3339 with Z only and reject naive strings and second 60. Give routines their own floating ops, MoveRoutine{start_minutes, minutes} and CreateRoutine{anchor day, minutes}, that never call resolve_on_day in apply. Replace event SetEstimate with an explicit instant-plus-length op.

**Evidence.** board.rs:446-460 (sane_day/sane_instant use the local date), 1047-1056 and 1206-1209 (resolve_on_day called before the routine branch), 396-419 (naive strings resolved during deserialization), 1134-1141 (event SetEstimate reads anchor.hour()); planner.rs:94-96, 296-311. [VERIFIED]

## V2-5 · medium · Stale offline edits silently overwrite newer edits; seen_seq is carried but unused

**Scenario.** The phone is offline Monday to Thursday. On Monday it reflows Tuesday (MoveBlocks) and sets task T due Friday. On Tuesday the laptop moves one of those blocks to 15:00, books another block on T and sets T due Wednesday. On Thursday the phone reconnects, and its entries are sequenced after the laptop's.

**Why it fails.** The last writer wins by arrival order. The laptop's 15:00 move and Wednesday deadline are reverted, with no report on either device. A phone Unplan of T would also delete the block the laptop booked. A halted older device (4.8) and any 48-hour outage cause the same overwrites. MoveBlocks is all-or-nothing, so one concurrently removed block discards a whole reflow, while Unplan clears whatever exists at apply time. Every plaintext already carries seen_seq (4.2), but apply never reads it.

**Required change.** Track last_modified_seq and the author for every item and session in BoardData. Put the origin's based_on_seq into Ctx: its view when the command was compiled, not when it was sealed. When a command overwrites a field another device changed after based_on_seq, apply must deterministically do one of two things: apply and record a 'concurrent overwrite' notice shown on both devices, or refuse. Suggested: refuse for MoveBlocks, Unplan and SetDeadline when based_on_seq is more than about a day old. Compile Unplan into RemoveBlocks{sids seen}. Make MoveBlocks skip missing sids and report how many it skipped.

**Evidence.** design 4.2 (seen_seq in the plaintext), 4.5 (MoveBlocks all-or-nothing), 4.6 (LWW in seq order); board.rs:1104-1114 (Unplan clears every session present at apply time). [code VERIFIED; behaviour ASSUMED from the design]

## V2-6 · medium · Version skew freezes the phone and hides its own edits; nothing gates new features on what the fleet can read

**Scenario.** The desktop updates, and the user uses a new op once. The phone is updated only by cable, so it halts at that seq for weeks.

**Why it fails.** 4.8 halts applying, so the phone shows a board frozen at the halt: new events and edits from the desktop never appear. 'Its own pending edits' are defined by the outbox, so once its own entries are acknowledged they leave the outbox. They cannot be applied to the frozen state either, so they vanish from its own screen. Its edits are compiled against the frozen view and overwrite newer data (issue 5), and it learns of refusals only after updating. Heartbeats already report 'the highest schema the device can read', but nothing uses that to stop writers. A halted device may still take snapshot duty ('any device writes one when the newest is older than 7 days') and still write backups.

**Required change.** Writers must not emit entries that need a schema or rules version above the minimum readable version across active members' latest heartbeats. The UI disables the feature and names the device that blocks it, with an explicit override. A halted device shows its confirmed state plus every own entry not yet applied to it, acknowledged or not, and labels the view 'frozen since <time>'. It either goes read-only or warns before each edit. Halted or lagging devices never take snapshot, compaction or backup duty.

**Evidence.** design 4.2 (beat contents), 4.7 (snapshot duty), 4.8; R1 key fact: older builds silently drop unknown fields (examples/wirecompat.rs). [design text VERIFIED; behaviour ASSUMED]

## V2-7 · medium · Snapshots are used for bootstrap, pairing and compaction without being validated

**Scenario.** A device with a determinism bug, or an older build, writes snapshot S whose state differs from the log, or uploads a corrupted blob whose claimed hash matches it. Later the snapshot holder compacts entries older than 365 days that S covers, and a new phone pairs with a bundle naming S.

**Why it fails.** The snap entry carries a state hash (4.2), but no step requires other devices to compare it with their own state at S. Heartbeats compare only at seqs divisible by 64, while snapshots are taken at arbitrary S. Pairing (5.3 step 5) names 'the snapshot id and hash' without E checking either against its own state. Compaction (4.7) requires only that 'a retained snapshot covers' the entries. After pruning, a bad snapshot becomes the only truth for new and long-absent devices, and the 400-day local logs are the only remaining evidence.

**Required change.** When applying the seq S of any snap entry, every device computes its own state hash and records an ack or a mismatch. A mismatch raises an alarm and marks the snapshot invalid. Compaction and pairing may use only snapshots acknowledged by at least one device other than the writer. The state hash covers BoardData plus the device set, epochs and counters. Devices that are halted or behind never write snapshots.

**Evidence.** design 4.2, 4.7, 5.3 step 5, 6.4. [ASSUMED from the design text]

## V2-8 · medium · Races between epoch and membership control entries break determinism

**Scenario.** (a) The user revokes the lost phone from both the desktop and the laptop, producing two EpochRotated{e+1} entries with different keys. (b) A DeviceAdded is sequenced concurrently with a rotation composed without it. (c) E registers N's signing key at the relay before E's DeviceAdded is sequenced, and N appends at once; or two pairings at the same moment choose the same device_no. (d) A laptop offline since before a rotation returns with entries sealed under e, and they reach a phone paired after the rotation.

**Why it fails.** The design says 'skip' for a bad signature or an unknown author and 'halt' for a newer ver or schema. It does not say what happens to a validly signed frame whose AEAD, unpadding or serde fails. In (a) the losing rotator can decrypt its own frames and nobody else can, so its replica diverges. 'Re-sealed and resubmitted' (5.4) places the resubmitted entries after the author's own later entries, so a Rename can land before its Create. In (b) the added device gets no wrap for e+1. In (c) replicas skip N's first entries as coming from an unknown author, and device_no comes from E's view in the bundle rather than from sequencing. In (d) the new phone may hold only the current epoch root.

**Required change.** Define epoch keys purely by log position. The first EpochRotated{e+1} wins; later ones are refused on every device, their author included, and the author discards the losing key. A validly signed frame that fails AEAD, unpadding or serde is a deterministic refusal everywhere, author included. Resubmission re-seals the author's whole run of entries from the first refused one onward, in their original order. Assign device_no at apply time, as the next free number in sequence order. N reads it from the sequenced DeviceAdded before minting any ids, and the relay accepts N's appends only after that seq. Bundles and snapshots carry every epoch root that a still-acceptable stale-epoch entry could need, or stale-epoch acceptance is limited in time.

**Evidence.** design 4.2 (checks on receipt), 5.3 steps 5-6, 5.4. [ASSUMED from the design text]

## V2-9 · medium · Rollback and fork alarms have no defined response or repair path

**Scenario.** Cloudflare restores the Durable Object from point-in-time recovery after an incident, or a hostile relay rolls back two days. Devices see welcome.head below their stored head, or get 409 gap{last_ctr} for counters whose acknowledgements they hold. Separately, the account is deleted and the board is re-homed without a fence.

**Why it fails.** 5.3 and 7 define alarms, but not that the device stops appending. New appends would be sequenced at seqs where it already holds different entries, which deepens the fork. A 409 gap is not mapped to a rollback. The only repair is a full re-home. A re-home without a fence re-sequences every entry beyond the seed, so every device must roll back to the seed, but devices keep only the newest checkpoint (4.1). Entries authored by a device that cannot move (a dead phone) and held only by others are dropped, because append requires dev == token device (4.3).

**Required change.** On a rollback, a fork, or a gap for which it holds acknowledgements, a device stops sending and stops applying remote entries, keeps editing locally, and shows one clear state. Add a relay 'restore tail' operation that accepts frames at their original seqs when they extend the relay's head along the hash chain; the relay can check the chain and the signatures without keys. The most advanced device can then repair an accidental rollback with the order preserved. Keep local sealed base checkpoints (for example weekly, for the 400-day horizon) so a device can roll back to any seed. Let the re-home seed include other devices' frames up to the initiator's head, with their signatures verified. List any entries that could not be carried over, by author and count.

**Evidence.** design 4.1, 4.3, 5.3, 6.7 step 4. [ASSUMED from the design text]

## V2-10 · medium · The 48-hour silence alarm will fire as a matter of routine

**Scenario.** The MacBook stays shut over a long weekend. Or One UI puts the phone into 'sleeping apps', so its background job does not run for three days. Or a Cloudflare outage lasts 48 hours.

**Why it fails.** Heartbeats are daily entries, and only a running device can send them. A receiver cannot tell a relay that withholds a device's entries from a device that is switched off. Integrity alarms that fire most weeks teach the user to ignore them, and that defeats the fork and rollback detection they are meant to back up.

**Required change.** Show an informational 'last seen' per device. Raise an integrity alarm only for provable withholding: the relay is reachable, this device's own entries echo, and later entries or heartbeats (through counter gaps or seen-heads) show that another device was active during a span whose entries never reached this device. Fork detection stays as designed.

**Evidence.** design 4.2 (beat), 7 (relay-tampering row: '48-hour silence alarm'). [ASSUMED]

## V2-11 · medium · The M2 cutover can strand or lose M1 queues

**Scenario.** On cutover morning the hub was off. The Mac, an M1 sync.rs client, holds outbox.json with commands that use temporary ids of 2^62 and above and address sessions by index. The phone page holds a localStorage queue. The user updates all three devices and runs 'Convert to replicated board'.

**Why it fails.** Step 6 of section 9 has no drain; the drain in step 1 belongs to the move to M1. M2 turns WebView DOM storage off, so the page's queue becomes unreadable and is silently lost. The M2 Mac either replays index-addressed commands against sid-addressed sessions or sets them aside unapplied (sync.rs:243-290). An old PWA cached by its service worker keeps queueing offline indefinitely. Import-check run against a live taskdeck-server can capture an item that is both archived and live, because retire appends the archive row before it saves the items.

**Required change.** Make 'every M1 outbox empty and acknowledged' a hard precondition of Convert, with clients reporting their queue length to the hub. M2 builds refuse to convert or bootstrap while any M1 queue is non-empty. On its first start, the M2 phone reads the page's localStorage queue once, before turning DOM storage off, and replays it through the compile step. Tell the user to remove the PWA. Import-check refuses a folder whose .lock is held, and reports any id that is both live and archived.

**Evidence.** design 9 steps 1 and 5-7, and 12 ('DOM storage off at M2'); phone.html:1471-1478 (page outbox in localStorage); sync.rs:243-290; board.rs:1475-1487. [VERIFIED code]

## V2-12 · medium · Backup contents contradict the storage budget, and restore-to-point has no base state

**Scenario.** After a year, each nightly .tdbk carries the 400-day frame log, about 0.1-0.3 GB by the design's own relay estimate. With 42 copies retained, that goes into free OneDrive or iCloud space (5 GB each).

**Why it fails.** 6.3 assumes about 5 MB per backup, but 6.6 says each backup holds the 400-day frame log, and ciphertext does not compress. The cloud folder fills, uploads fail quietly, and the writer cannot read its own HPKE-sealed backups to notice. Restore to a point in time replays 'the local log to a chosen seq', but devices keep only the newest checkpoint and relay snapshots cover about three weeks, so older targets have no base state.

**Required change.** Back up BoardData and the sync metadata nightly (megabytes), and the frame log as append-only monthly segments shared across backups. Show the backup size and the time of the last successful write, and alarm when a write fails. Re-read each written file and check its structure. Keep local sealed base checkpoints (weekly) across the restore-to-seq horizon.

**Evidence.** design 6.2 (storage row), 6.3, 6.6, 4.1. [design text VERIFIED; sizes ASSUMED]

## V2-13 · medium · Deleted data is not deleted

**Scenario.** The user deletes a task holding private details and then forgets its archive row, which DOC calls 'the one irreversible act'. Or the user types a password into the notepad and deletes it seconds later.

**Why it fails.** The content stays in the Add, Rename and SetNotes entries: 365 days in the relay log, 30 days in DO point-in-time recovery, in up to three retained snapshots, in each device's 400-day local log, in 30 daily and 12 monthly backups, and in the monthly offline USB copies. Restore-to-seq brings forgotten rows back. Even with coalescing, the log keeps one copy of the notes for every 30 s of typing. A device compromised later holds every epoch root and exposes all of it. The design mentions none of this.

**Required change.** Document how long each store keeps data, and make the UI honest about Forget. Offer 'purge history before now'. It writes a snapshot, compacts the relay log and the snapshots before it, and truncates local logs. It also rotates the epoch and drops the old epoch roots, which crypto-shreds the relay copies, and marks older backups for deletion. Optionally keep the notes as a separately keyed blob that is overwritten in place rather than logged.

**Evidence.** board.rs:247-251 (ForgetArchived documented as irreversible); design 4.1, 4.6, 4.7, 6.6. [VERIFIED]

## V2-14 · medium · Poison-pill handling: false halts from OS kills, per-entry fsync, and self-inflicted poison

**Scenario.** Android kills the process during a catch-up batch, or 4,200 entries arrive after two weeks offline. Or an origin enqueues a command whose apply panics.

**Why it fails.** 4.5 persists an 'applying seq N' marker before each entry. On restart a leftover marker looks exactly like an abort caused by entry N. Either OS kills produce false 'entry N crashes this build' halts, or real aborts crash-loop. A persisted marker plus a fsynced log append for every entry means thousands of fsyncs (ASSUMED 5-15 ms each on phone flash), against the '< 10 s' catch-up target. If an origin enqueues a command before its own trial apply, every device halts on it. A notification action taken before the first unlock after a reboot cannot reach the credential-encrypted store.

**Required change.** Halt only after the same N has failed on two or three consecutive starts. Batch log appends and markers per page (256 entries), and bisect on failure. At the origin, compile first, then run a trial apply under catch_unwind, then fsync to the outbox; a command that panicked locally is never enqueued. Queue notification-action intents in device-protected storage until unlock.

**Evidence.** design 4.1, 4.5, 4.9; Cargo.toml:94 (panic = 'abort' today). [timings ASSUMED]

## V2-15 · low · Compaction can strand a device that missed an epoch rotation

**Scenario.** The laptop spends 13 months in a drawer while the epoch rotates. When it returns, tail_start is past its head, so it must bootstrap from the newest snapshot.

**Why it fails.** The rotation's HPKE wraps live in a control entry, and compaction deletes it after 365 days. Snapshots are sealed under the current epoch's snapshot key and carry the epoch wraps inside (4.7). The laptop cannot decrypt the snapshot to learn the key that decrypts it. Its outbox entries carry a seen_seq below everyone's retained log, so the fork check cannot run on them.

**Required change.** Never compact control entries (DeviceAdded, DeviceRevoked, EpochRotated and Fence are tiny), or keep each device's current-epoch wrap outside the snapshot's encryption. When seen_seq is below the receiver's retained log, skip the seen-head check and show a warning rather than an alarm.

**Evidence.** design 4.7 (snapshot contents, compaction), 5.4. [ASSUMED]

## V2-16 · low · Refusals are reported transiently, and benign ones look like errors

**Scenario.** At 08:00 the phone completes T from a reminder in a background job; the desktop completed T at 07:59. The phone's Complete is refused while the app is not on screen.

**Why it fails.** 4.6 reports refusals 'only on the device that sent the command', and nothing persists the report. Today an error window shows it (ui.rs:2837-2841). With offline-first use across devices, the same intent arriving twice (Complete twice, Restore twice) will be the commonest refusal, and it will read as a failure.

**Required change.** Keep a persistent per-device list of changes not applied, giving the command, the reason and the conflicting seq, and surface it as a badge or a notification. When a command finds its target already in the requested end state (Complete or Delete of an id already archived, Restore of a row already live), treat it as a deterministic no-op success.

**Evidence.** ui.rs:2837-2841; design 4.6. [VERIFIED]

## V2-17 · low · The naming hold can delay or reorder the queue

**Scenario.** On the desktop, the user drags out a block and takes 25 s to name it, and meanwhile moves another block.

**Why it fails.** 4.6 holds a Create that is being named for up to 30 s and coalesces it with the following Rename or Forget. Counters are assigned at enqueue, and the relay requires ctr = last_ctr+1. Either every entry behind the held Create waits up to 30 s, which breaks the p95 < 1 s target, or later entries go first and anything that refers to the new id is refused.

**Required change.** Hold a Create only while it is the last entry in the outbox, or assign counters at seal time for entries queued behind a held Create. Never send an entry that refers to an unsent Create.

**Evidence.** design 4.3 (counter assigned at enqueue; relay ctr rule), 4.6 (naming). [ASSUMED]

## V2-18 · low · M1 keeps today's sync hazards for about ten weeks

**Scenario.** Weeks 12 to 21: the phone and the Mac edit the blocks of one task while offline; the hub restarts during a burst of edits; the Mac retries a request it sealed more than five minutes earlier.

**Why it fails.** The hub applies commands with today's Board::apply. Sessions are addressed by index (E11). created is the hub's replay time. phone::Replies lives only in memory, so a retry after a hub restart is applied twice. If retries resend the original sealed request, the 5-minute replay window turns them into 'sealed hub refusals', which section 10 says drop the edit.

**Required change.** Re-seal every M1 attempt with a fresh nonce and time, and treat an expired replay window as 'keep'. Persist the hub's reply cache as a sealed file of 1,024 entries. Stamp created at the origin. Consider bringing sids forward to Phase 2.

**Evidence.** board.rs:1047-1054, 1094-1101, 1420, 1464; phone.rs:287-306; DOCUMENTATION.md 22.4 ('Two things the replay cannot mend'); design 10 (hub). [VERIFIED]

## V2-19 · low · Load-time normalisation changes replicated state outside the log

**Scenario.** A checkpoint or snapshot is loaded through today's Board::from_parts or Board::open path.

**Why it fails.** from_parts runs assign_missing_ids and migrate_legacy_plans. open replaces tabs in the notes and substitutes an error string when the notepad file cannot be read. On a replica, any of these changes that device's state without a log entry.

**Required change.** In replica mode, load BoardData exactly as stored. Run normalisation only in import-check and genesis. On load, assert that normalisation would change nothing, and raise an alarm if it would.

**Evidence.** board.rs:593-595, 644-646; tasks.rs:618-632, 688-699. [VERIFIED]

## V2-20 · low · Clock skew is detected but not bounded

**Scenario.** The laptop's clock is two days fast.

**Why it fails.** Ctx.at is the only clock. Today archived_at caps a client's time at the server's clock plus one day; the design caps it at the same skewed clock, which limits nothing. created and archived_at land two days ahead on every device, which skews the ledger order, the done-day ghosts, Met/Missed, and the age-based ripening of undated tasks. 4.5 only warns.

**Required change.** Keep an offset learned from welcome.relay_now, and stamp Ctx.at as the local clock plus that offset. Show a persistent banner, and refuse to compose edits when the offset exceeds about an hour and has not been confirmed recently.

**Evidence.** board.rs:462-469; tasks.rs:437; design 4.5 (relay time only raises a warning). [VERIFIED]

## V2-21 · low · Identity edge cases: copied folders, found devices and abandoned boards

**Scenario.** The user copies the portable TaskDeck folder and runs both copies under one Windows account. A phone that was revoked as lost turns up again. After a full recovery onto a new board, an old device comes back.

**Why it fails.** DPAPI unwraps the key for the same user, and the folder locks are separate files, so two processes share one identity and keep re-sealing each other's counters on 409. A revoked device that turns up has its outbox stranded. Recovery creates a new board but never fences the old one, so a returning old device keeps syncing to the abandoned board. Nothing states that a relay 401, 403 or 404 must never cause local deletion.

**Required change.** The relay allows one live session per device and raises an alarm on a second. Clients halt after repeated 409s. A device that turns up after revocation offers 're-pair and resubmit my unsent edits'. Recovery holds a key in the device set that can sign only Fence entries, so the old relay can be fenced. No relay response ever deletes local data or keys.

**Evidence.** paths.rs:96-120; design 4.3, 5.4, 5.5. [code VERIFIED]

## V2-22 · low · ETag/304 needs the raw ICS kept so the window can slide

**Scenario.** A feed that rarely changes answers 304 for weeks.

**Why it fails.** The overlay is expanded over a window relative to today: 60 days back and 400 forward. The comment in adopt_overlay explains that the window must slide even when the events do not change. A 304 brings no body to re-expand, so occurrences entering the window are never generated.

**Required change.** Cache the raw ICS body, sealed, together with its ETag, and re-expand it whenever the window moves.

**Evidence.** board.rs:794-816; subscriptions.rs:594; design 3 (ETag). [code VERIFIED]

## Confirmed strengths

- One sequencer with last-writer-wins per command preserves DOC 4.1. Device-minted ids below 2^53 remove the temporary-id remap machinery (sync.rs:403-432, 647, 733-738; ui.rs:2811-2836) and the JavaScript precision collapse. [VERIFIED]
- Every apply arm validates before it mutates (board.rs:951-1399, read arm by arm), so a pure in-memory apply can be all-or-nothing. [VERIFIED]
- Board state has no HashMap, HashSet or randomness. Floats appear only in snap/clamp_block, on integer inputs (board.rs:1055, 1084, 1124, 1136, 1208; planner.rs:260-279), so the planned switch to integer snapping is small. [VERIFIED]
- The design targets the real clock and zone hazards: resolve_on_day (planner.rs:296-311), created = now (board.rs:1420, 1464), Reflow's per-device overlay anchors (board.rs:906) and Vec order (board.rs:888-901). [VERIFIED]
- It also targets the UI-side ones: the drain and re-sort of board.items (ui.rs:2911-2955), and the notepad editing board.notes directly (ui.rs:2152) under a 2 s autosave (ui.rs:7039-7047). [VERIFIED]
- Today the order of archive rows with equal timestamps changes across a reload: record inserts newest-first among equals (archive.rs:741), rewrite reverses (784), and load's stable sort keeps file order (723). The design's total order (archived_at, id, bytes) fixes this. [VERIFIED]
- Undo still works when edits apply locally. The page sends inverse commands (phone.html:4876-4949), its stamp() is UTC to the whole second (4825), and Restore echoes the stored key string (5257), so canonical UTC archive keys keep Undo valid. [VERIFIED]
- Session ids can reuse the existing wire field 'session', because the page only compares and echoes its value (phone.html:4260, 4365, 4909-4931, 4939, 5066-5083). [VERIFIED]
- The bridge allow-list matches what the page sends: exactly 17 ops, with no forget, forget_archived or subscription ops (grep of phone.html). [VERIFIED]
- Fsyncing the outbox before apply returns closes today's window, in which queued commands sit only in a channel until the sender's next pass (sync.rs:648-660). [VERIFIED]
- The hash chain, per-device counters, seen-heads and the echo check together give SUNDR-style fork consistency. Skipped frames stay in the chain, so skipping a frame cannot hide a reorder.
- SetNotes{text, base} with a local draft avoids both lost text and merge-append duplication, and save coalescing addresses the 2 s autosave.
- Compaction deletes only snapshot-covered entries, in daily slices of at most 5,000 rows, which respects the free plan counting deletes as row writes.
- Subscription auto-rename races settle after at most a few renames, because is_unnamed stops once a feed title has been adopted (subscriptions.rs:312-334). [VERIFIED]
- Lowest-schema stamping, with older devices still allowed to send, is the right base, given that old builds silently drop unknown fields. It needs fleet gating and versioned rules on top (issues 1, 2 and 6).

## Notes

Method: I read board.rs, planner.rs, tasks.rs, archive.rs, sync.rs, subscriptions.rs and paths.rs in full or in the relevant parts. In ui.rs I read apply, summarize_calendar and the notepad; in phone.rs the command route and Replies; in phone.html the verbs, Undo, stamp() and the outbox. I did not run cargo (not permitted for this task). [VERIFIED] means I read it in code at the cited lines. How the unbuilt design behaves is ASSUMED from its text.

DETERMINISM AUDIT of today's apply path, with its status under the design:
(1) day+minutes resolved in the applying device's zone in MoveBlock/AddBlock/Create (board.rs:1056, 1085, 1209). Addressed by compiling at the origin, but routines still call it for validation (issue 4).
(2) Naive wire times resolved during deserialization (board.rs:396-419). A gap unless the log's command types reject them (4).
(3) Year-bound validation on the local date (board.rs:446-460). A gap (4).
(4) Event SetEstimate reads anchor.hour() (board.rs:1134-1141). Addressed only if it becomes an explicit instant op (4).
(5) created = now (1420, 1464) and the archived_at default and clamp (466-469). Addressed by Ctx.at, but the clamp no longer limits anything (20).
(6) Reflow reads now_marker, Vec order and the overlay (888-907, 1226-1250). Addressed by MoveBlocks, with the all-or-nothing caveat (5).
(7) Archive I/O and lazy loading inside apply (1263, 1308; archive.rs:754-799). Addressed by the Store and eager loading.
(8) Archive order among equal timestamps changes after a reload (archive.rs:723, 741, 784). Addressed by the total order.
(9) The UI re-sorts board.items (ui.rs:2911-2955). Addressed.
(10) The version counter (924-940). Addressed: it becomes the seq.
(11) The shared next_id, and subscription-id reuse after a reopen (645, 766, 1332). Addressed by device ids plus Phase 0, but monotonic per-device marks are still needed (3).
(12) Restore reusing an id (1274-1281). Addressed by the fallback id.
(13) overlay.forget inside apply (1355, 1393). Addressed: it becomes an effect.
(14) Validation constants and defaults (429-435, 979-984, 1165-1179, 1253, 1321-1340). A gap (1).
(15) str::trim depends on the toolchain's Unicode tables (430). A gap (1).
(16) f32 snapping (planner.rs:260-279). Deterministic on integer inputs, and addressed anyway.
(17) Load-time normalisation (644-646; tasks.rs:618-632, 688-699). A gap (19).
(18) The notes cap is checked before tabs are expanded (1253-1256; utilities.rs:135, 147). Pre-existing. Fix it before M2, or treat it as a rules change (1).
(19) Sessions addressed by index (1047-1054, 1094-1101). Addressed by sids at M2, still open during M1 (18).
Presentation only, not state: importance_score ranking (f32, exp2), the HashMap in bucket_by_deadline_day, the overlay, and weather.

SCENARIOS: what each device shows and stores afterwards (at M2 unless stated).
S1, phone offline 3 days. While offline, the phone shows its confirmed state at S0 plus its own pending entries: new items with ids 3<<40|n and new sids, and T marked done with archived_at D+1 in its Done tab. The laptop shows S0+1 to S1. On reconnect, the phone fetches S0+1 to S1, fsyncs them, applies them and rebases its own entries, which are then sequenced after S1. Every device then stores the same BoardData, including the phone's new items. If the laptop retired T first, the phone's Complete{T} is refused, its D+1 row disappears and the laptop's row stays. The phone's earlier edits to T are refused too, even though they were made earlier in wall time. Phone edits to items the laptop edited but did not retire silently overwrite the laptop's newer values (issue 5). Refusals are shown only on the phone, and only briefly (16). Verdict: converges; overwrites are silent.
S2, concurrent block edits. With sids, a Move against a Remove goes to whichever is sequenced first, and the loser is refused on every device. Two Moves: the later seq wins, with no report. MoveBlocks against a Remove: the whole reflow is refused. Unplan against AddBlock depends on order (5). During M1, sessions are still addressed by index, so a queued move can silently land on the wrong block (18).
S3, revocation while a device is offline. The desktop revokes at the relay, then appends DeviceRevoked+EpochRotated{e+1} at R, wrapped for the desktop, the laptop and the recovery key. When the laptop returns it unwraps R_{e+1}. Its entries sealed under e with seen_seq < R are accepted, and they stay readable to anyone holding R_e. The desktop and laptop then store identical state, and the lost phone keeps everything up to R. Not specified: concurrent rotations, frames that fail AEAD, the order of resubmitted entries, devices added concurrently, and devices without R_e (8). After compaction, a device that missed the rotation is stranded (14).
S4, relay rollback, drop, reorder or fork. A rollback is caught by the stored head. A dropped entry shows as a gap in seq or in a device's counters. A fork shows as a seen-head mismatch once entries cross between the views. A lasting partition shows only as silence (10). No rule says to stop sending, and there is no repair path (9).
S5, bad snapshot. It is caught only if some device bootstraps from it and a later heartbeat comparison at a multiple of 64 follows. Nothing stops compaction or pairing from relying on it (7), and comparing hashes across versions is unreliable (2).
S6, version skew. The old laptop stops at the newer entry k. It shows a frozen board, its acknowledged own edits vanish from its view, its new edits are compiled against the frozen state, and it learns of refusals only after updating (6). Rule fixes cause divergence without halting any device (1).
S7, clock skew, travel and DST. All deterministic: Ctx.at travels in the entry, and instants are resolved once at the origin, so DST is handled once. Skew is bounded only by a warning (20). Travel: the phone compiles instants in New York's zone and the wall shows them in Helsinki time, which is a documented behaviour change. Zone reads remain inside apply (4).
S8, quota, 48-hour outage, suspension. Local editing continues and outboxes grow. Quota exhaustion pauses sync until 00:00 UTC. A 48-hour outage raises false silence alarms (10). Suspension means a re-home (9). Nothing is lost, provided no relay error can delete local data (21).
S9, wipe, reinstall, disk death. A wiped phone stores nothing: its unsent outbox is lost, it re-pairs with a new device_no, and the old identity lingers until revoked. A Mac restored from Time Machine returns as the same identity with rolled-back counters, which leads to duplicates and reused ids (3). If every device is lost, taskdeck-recover builds a new board and the old one is left unfenced (21). Backup sizes are inconsistent (12).
S10, migration. A contended lock blocks the hub and sync, as it should. Writes by an old build to the legacy files are caught by the step-7 watch. The M1 queues are not drained at the M2 cutover (11).
S11, subscriptions. Each device's overlay differs but is only drawn. Reflow is compiled at the origin, so it is deterministic. Auto-rename races settle after at most three renames. Garbage or huge feeds hit the 8 MiB cap, EVENTS_MAX and the catch_unwind quarantine. A 304 response freezes the window (22). Changing checked_url is a change to the shared rules (1).
S12, idempotency. A retry with the identical persisted frame gets the stored seq back. A crash between transmitting and persisting, or a restored copy, duplicates the command (3). In M1, the hub's reply cache is lost on restart (18).
S13, Android. Process death is safe, because the outbox is fsynced before returning and the log is replayed. Crash markers can cause false halts, and per-entry fsync is slow (15). Doze and sleeping-app status delay sync without losing data, but they trigger the silence alarm (10). Reminders fire from local state, which may be stale while offline.
S14, compaction. A device less than 365 days behind catches up from the log. One beyond the compaction floor bootstraps from a snapshot and rebases its pending entries, unless it missed a rotation (14). The snapshot compaction relies on is never validated (7).
S15, deletions. Delete puts an archived row on every device. Forget and ForgetArchived remove data from BoardData deterministically. The content still persists in logs, snapshots, backups and point-in-time recovery, and restore-to-seq brings it back (13).

Highest-value changes: version the apply rules and the canonical hash (issues 1 and 2); reconcile a device's own entries by entry_id, with monotonic id marks (3); validate snapshots before they are used for compaction or pairing (7).
