# Resolutions R1: device.md against engine.md (20 findings)

Resolver 5 of 9. Inputs read: reviews.md §B, spec/README.md, foundation.md "Decisions" and "Rulings for the specs" (S1 to S31, with the settled S15), SYNC_PLAN.md §2 and §3, the earlier files `r1-seal-relay.md`, `r1-seal-engine.md`, `r1-core-engine.md`, `r1-engine-relay.md` and the lead's `r1-lead.md` (L1 to L5) in full; device.md in full; engine.md §1 to §11, §14 to §30, §33 to §38; the relay.md passages on login, `door`, §14.5; bridge.md §6.4.4, §11.4 and §20.

Built on the earlier files and the lead's rulings (settled; not repeated here):
- device-engine-1 (ruling S27): the history travels inside the snapshot body and the sealer fills and keeps it (R1-SEAL-RELAY-9, R1-SEAL-RELAY-28, R1-SEAL-ENGINE-15, lead ruling L4). `SnapshotMaterial` does not change and the Join layout gains no history; only the rejoin path of the store is added here (R1-DEVICE-ENGINE-1).
- device-engine-12 (ruling S25) is fully applied by R1-SEAL-ENGINE-19.
- device-engine-13 (ruling S24) is ruled by L1; the engine's rule and the relay and seal bytes are the lead's change file's. Only the device's hand-over of the login answer's wraps is added here (R1-DEVICE-ENGINE-13).
- device-engine-15 is applied by R1-SEAL-RELAY-20 and R1-SEAL-RELAY-27 (the refusal's `read_epoch`, device.md §15.8 step 1, open question 7); only device.md §22's row follows here.
- device-engine-18 is applied by R1-SEAL-ENGINE-20 and R1-CORE-ENGINE-22 (engine.md §7's table names the segments and kinds 7 to 10 and 17, and the sentence on kinds 11, 12 and 18).
- device-engine-19 is applied on the seal and engine side by R1-SEAL-RELAY-1, R1-SEAL-ENGINE-16 and R1-CORE-ENGINE-21 (255 bytes, `engine/bad_host`, ENG-OPS-007, the §34 row); only device.md follows here.
- device-engine-14 (ruling S29): R1-SEAL-RELAY-10 already lists kind 19 `own-picture.tdk` in seal.md §19.2 and `FileKind::OwnPicture`; R1-SEAL-RELAY-29 left device.md's row, layout and facade to a later resolver. They are written here in full (R1-DEVICE-ENGINE-14), since this finding asks for them.
- device-engine-11 changes `TruncateLogBelow`, which §19.7 step 4 (as R1-SEAL-ENGINE-14 and R1-CORE-ENGINE-20 left it) and §28.2 emit; their text is unchanged, the new field is defined once in §7.11.

New clause ids assigned here (later resolvers continue after them, and after those of the earlier files):
- device.md: DEV-RUN-006, DEV-RUN-007, DEV-PIC-005. Withdrawn: DEV-RUN-001.
- engine.md: ENG-MOVE-008 (after ENG-MOVE-005 to -007 of R1-ENGINE-RELAY-2), ENG-PURGE-004, ENG-SESS-007, ENG-STATE-005.

New error codes: none. Removed error code: `device/reopen_needed` (device.md §21). New public types and functions: engine.md `Evidence` and `ForkSource` (§21.1), the `Status` types of §30.1, `Notification::MoveCodeResult`, `Operation::Adopt`, `StopKind::Closed`, `LocalReadResult::Missing`, `Engine::purge_preview`; device.md `set_own_picture`, `clear_own_picture`, `store::KIND_OWN_PICTURE`. No byte on the wire changes; the device's file layouts change in kind 17 (Genesis) and the `device.tdk` record (a new field), and kind 19 is added.

Notation. "Replace A with B" means the exact text A, as it stands in the spec after the changes of the four earlier files, becomes B. Clause texts are given in full. A table row is given in full, in the table's own column order. A change that needs edits in two specs has the same id under both headings.

## engine.md

### R1-DEVICE-ENGINE-2 (device-engine-2, device-engine-16)

The creating device's bootstrap carries its relay target, as a join's does, and `Join` has no `roots_ok` (the pairing machine checked the roots, `seal.md` §17.9). `Config` no longer carries a relay, so the target has one source. Matching edits: device.md, same id.

1. §7.10, the table, row `bootstrap`: replace its Meaning cell with:

   for a new replica, one of `Genesis { board, frame_1, relay }` (the creating device, §19.8) or `Join { material, added_seq, relay }` (a paired device, §19.8); absent otherwise

2. §19.8, the **Genesis** bullet (as R1-CORE-ENGINE-20 item 3 left it): replace `` `Loaded.bootstrap = Genesis { board, frame_1 }`: the creating device's runtime has built the genesis frame with the sealer and created the board at the relay (`relay.md` §10.2). `` with:

   `Loaded.bootstrap = Genesis { board, frame_1, relay }`: the creating device's runtime has built the genesis frame with the sealer and created the board at the relay (`relay.md` §10.2); `relay` is that relay's host and the board's door, with `home_gen = 1` (the genesis membership's, `seal.md` §13.3), and the engine sets `meta.relay` from it.

3. §37 Public API: replace

   ```rust
   pub enum Bootstrap { Genesis { board: Vec<u8>, frame_1: Vec<u8> },
                        Join { material: SnapshotMaterial, added_seq: u64, relay: RelayTarget } }
   ```

   with

   ```rust
   pub enum Bootstrap { Genesis { board: Vec<u8>, frame_1: Vec<u8>, relay: RelayTarget },
                        Join { material: SnapshotMaterial, added_seq: u64, relay: RelayTarget } }
   ```

4. §37 Public API: replace `pub struct Config { pub relay: Option<RelayTarget> }` with `pub struct Config {}                                               // reserved: the relay target comes only from Loaded.meta or Loaded.bootstrap (§7.10)`.

### R1-DEVICE-ENGINE-3 (device-engine-3)

Following an address carries the move code the user typed with it; the engine checks it against the `Fence` it finds on the candidate. The move code is computed from public log values (`seal.md` §15.4) and is not a secret, so it may be persisted in `OpRecord`. Matching edits: device.md, same id.

1. §28, the `Operation` enum: replace the line `    FollowAddress { host: String, door: [u8; 5] },   // follow a move when the old relay is gone` with `    FollowAddress { host: String, door: [u8; 5], code: String }, // follow a move when the old relay is gone; code: the typed move code`.

2. §28.3, **On every other device**, step 4: replace the whole step with:

   4. When the old relay is gone and the device holds no `Fence`: `Operation::FollowAddress { host, door, code }`, `code` being the move code the user typed with the address. While `wall_ms` is below the lockout of ENG-MOVE-002 it ends with `engine/move_locked`. Otherwise the engine reads from the new relay as a candidate, without sending; it follows only if the log there holds an accepted `Fence` at `f` naming exactly that host and door and `Sealer::check_move_code(f, H(Fence frame), code)` accepts `code`. No such `Fence`: the operation ends with `engine/move_no_fence`; a wrong code: it ends with `seal/move_code`, which counts toward the lockout of ENG-MOVE-002 like a wrong `UserAction::MoveCode`. On either failure its target stays the old relay.

### R1-DEVICE-ENGINE-4 (device-engine-4)

The two user actions that can fail get a result: adoption becomes an operation with a ticket (it may wait for a snapshot, so it is long), and a move code is answered in the same `handle` call. Matching edits: device.md and bridge.md, same id.

1. §28, the `Operation` enum: after the line `    CloseBoard { host: String, door: [u8; 5] },      // reset trust: point the old board at the new one` add the line `    Adopt { dev: Dev, typed_fp: String },            // adopt another device's state by its fingerprint (§22.3)`.

2. §28, the paragraph after `pub struct OpRecord …`: replace `with two exceptions: `Skip` is also allowed under a crash halt, and `Move` is also allowed under an evidence stop (it is the re-home of §21.3).` with `with three exceptions: `Skip` is also allowed under a crash halt, `Move` is also allowed under an evidence stop (it is the re-home of §21.3), and `Adopt` is also allowed under the divergence stop (it is that stop's repair, §22.3).`

3. §22.3, the first paragraph: replace `` `UserAction::Adopt { dev, typed_fp }`, where `typed_fp` is what the user read on the other device and typed or compared on this one. `` with `` `Operation::Adopt { dev, typed_fp }` (§28), where `typed_fp` is what the user read on the other device and typed or compared on this one. It reports `OpProgress` and ends with `OpDone`. ``

4. §22.3, step 1: replace `else `engine/fingerprint` and nothing changes.` with `else the operation ends at once, in the same `handle` call, with `OpDone { ticket, result: Err("engine/fingerprint") }`, and nothing changes.`

5. §22.3, step 2: append at its end: ` The operation waits at this stage; §28.8's timeout (`engine/op_timeout`) applies.`

6. §22.3, step 3 (as R1-CORE-ENGINE-20 item 8 left it): replace `raises `BlobFailure` with the writer, the blob id and the code, and nothing changes.` with `raises `BlobFailure` with the writer, the blob id and the code, the operation ends with `Err` of that code, and nothing changes.`

7. §22.3, step 4 (as R1-CORE-ENGINE-20 item 9 left it): replace `else `engine/adopt_membership` and nothing changes.` with `else the operation ends with `Err("engine/adopt_membership")` and nothing changes.`

8. §22.3, step 5 (as R1-CORE-ENGINE-17 item 5 left it): replace `a checkpoint is written; the divergence stop ends.` with `a checkpoint is written; the divergence stop ends; the operation ends with `OpDone { ticket, result: Ok(()) }`.`

9. §28.3, **On every other device**, step 2: replace the whole step with:

   2. `UserAction::MoveCode { text }`: refused with `engine/move_locked` while `wall_ms` is below the lockout, and with `engine/move_no_fence` while no accepted `Fence` awaits a code (no `move_pending`); otherwise `Sealer::check_move_code` against the latest accepted `Fence`: a failure (`seal/move_code`) raises the count and, at 5 failures in a row, locks entry for 3,600,000 ms; a success sets `meta.relay` from the `Fence` item, resets the count, and connects. In the same `handle` call the engine emits `Notify(MoveCodeResult { result })`: `Ok(())` on a success, else `Err` with the code.

10. §28.3: add after ENG-MOVE-007 (which R1-ENGINE-RELAY-2 adds):

    - **ENG-MOVE-008** (engine) The engine MUST answer every `UserAction::MoveCode` with exactly one `Notify(MoveCodeResult)` in the same `handle` call, carrying `Ok(())` when the code was accepted and otherwise the code of §28.3 step 2 that refused it.

11. §30.3, the `Notification` enum: after the line `    MoveReport { carried_through: u64, old_relay_answered: bool },` add the line `    MoveCodeResult { result: Result<(), &'static str> },   // the answer to UserAction::MoveCode (§28.3)`.

12. §30.4, the `UserAction` enum: replace `    ConfirmClock, MoveCode { text: String }, Adopt { dev: Dev, typed_fp: String },` with `    ConfirmClock, MoveCode { text: String },`.

13. §33 Error catalogue: replace the row `| `engine/move_no_fence` | the candidate relay's log holds no matching `Fence` | §28.3 | check the address and the code |` with `| `engine/move_no_fence` | the candidate relay's log holds no matching `Fence`, or no accepted `Fence` awaits a move code | §28.3 | check the address and the code |`.

### R1-DEVICE-ENGINE-5 (device-engine-5)

The reach of a purge, which the lead's ruling under S25 has the desktop show before the owner confirms, is computed once, by the engine. Matching edits: device.md, same id.

1. §28.2: add this paragraph after step 4 of "On the purging device" and before `On **every device**`:

   `Engine::purge_preview()` (§37) returns `(p, τ_{p−1})` exactly as step 1 computes them from the current state, or `Err("engine/nothing_to_purge")` when step 1 would refuse, so the interfaces show the reach of a purge before the user confirms it.

2. §28.2: add after ENG-PURGE-003:

   - **ENG-PURGE-004** (engine) `Engine::purge_preview` MUST return the `p` and `τ_{p−1}` that §28.2 step 1 computes from the engine's current state, and `Err("engine/nothing_to_purge")` exactly when that step would refuse.

3. §37 Public API, `impl<SM: StateMachine, S: Sealer> Engine<SM, S>`: after the line `    pub fn restore_targets(&self) -> Vec<(u64, i64)>;                   // §28.1: (seq, τ)` add the line `    pub fn purge_preview(&self) -> Result<(u64, i64), &'static str>;    // §28.2 step 1: (p, τ_{p−1})`.

### R1-DEVICE-ENGINE-7 (device-engine-7)

A `429 relay/delete_budget` answers an admin call and reaches the engine as `AdminResult` (device.md §10.6, same id). Compaction already handles it (§20.2's table); purge's continuation gains the same rule.

§28.2, "On the purging device", step 3: append at its end: ` An answer `429 relay/delete_budget` (an `AdminResult` with that refusal) counts as that day's call and is repeated on the next UTC day, with no alarm.`

### R1-DEVICE-ENGINE-8 (device-engine-8)

The platform's write refusal pauses writes until the next 00:00 UTC, as `relay.md` §14.5 states, with no alarm. ENG-SESS-006 already binds the engine to every row of §11.9, so no clause is added. Matching edits: device.md, same id.

§11.9, the table: add this row directly after the row `| `RelayRefused` `409 relay/seeding` | retry after 60,000 ms of `mono_ms` |`:

`| `RelayRefused` `507 relay/storage_refused` (with `until`) | the platform refused a write (`relay.md` §14.5): no `Append`, no `Admin` and no blob upload until `wall_ms ≥ until`, then each refused request is sent again; reads go on; `paused_until_ms = until`; no alarm |`

### R1-DEVICE-ENGINE-9 (device-engine-9)

A refused login (`device.md` §10.6, same id) reaches the engine as `RelayRefused { req: None, … }` while the session is `Connecting`, followed by `Disconnected`. A `401 relay/unauthorized` there, on the relay a `Fence` named, means this device is no member of the board on that relay (a board closed by reset trust, or a move that did not carry it): the new stop `Closed` offers re-pair and resubmit (§24.5). Any other refused login is retried at most hourly.

1. §11.9, the table: add these two rows directly after the row R1-DEVICE-ENGINE-8 adds (and before `| `RelayRefused` any other | … |`):

   `| `RelayRefused` `401 relay/unauthorized` with `req: None` in `Connecting` (a refused login), while `meta.relay` is the relay of `M_K`'s latest accepted `Fence` (`home_host`, `home_door`, `home_gen`) | this device is no member of the board there (§28.7): the stop `Closed` with the action `RePair` (§24.5); no `Connect` while it is in force; it ends at the next `Start`; no alarm |`
   `| `RelayRefused` with `req: None` in `Connecting`, any code the rows above do not name | a refused login: alarm `RelayProtocol` with the code; `Connect` again at most once per 3,600,000 ms of `mono_ms` (§11.1) |`

2. §11.1, the table, row `Offline`: replace its Guard cell (as R1-ENGINE-RELAY-6 left it) with:

   a relay target is set, the device is not stopped by `identity_clone` or `Closed` (§11.9), and, while it is revoked (§24.4) or after its last `Connect` ended in a refused login (§11.9), no `Connect` was emitted in the last 3,600,000 ms of `mono_ms`

3. §11.9: add after ENG-SESS-006:

   - **ENG-SESS-007** (engine) On a login refused with `401 relay/unauthorized` while its relay target is the relay of its latest accepted `Fence`, the engine MUST enter the stop `Closed`, MUST emit no `Connect` while that stop is in force, and MUST offer re-pair and resubmit.

4. §24.5, first line: replace `For a revoked device, a commitment halt, a stranded rejoin, or a closed board (§28.7):` with `For a revoked device, a commitment halt, a stranded rejoin, or a closed board (§28.7, the stop `Closed` of §11.9):`

5. §28.7, last sentence: replace `finds it is no member of the board there (`relay/unauthorized` at login), and the engine offers re-pair and resubmit into the new board (§24.5).` with `finds it is no member of the board there (`relay/unauthorized` at login), enters the stop `Closed` (§11.9, ENG-SESS-007), and the engine offers re-pair and resubmit into the new board (§24.5).`

6. §30.1, the table, row `stops`: replace its Meaning cell's `§21, §22, §24.4;` with `§11.9, §21, §22, §24.4;` (the rest of the cell, the list of actions, is unchanged; `Closed` uses `RePair`).

7. §37 Public API: replace

   ```rust
   pub enum StopKind { Evidence, Divergence, ForeignSigner, IdentityClone, Revoked { by_log: bool },
                       LocalFault { code: String }, Stranded, Rebootstrap { after: u64 } }
   ```

   with

   ```rust
   pub enum StopKind { Evidence, Divergence, ForeignSigner, IdentityClone, Revoked { by_log: bool },
                       LocalFault { code: String }, Stranded, Rebootstrap { after: u64 }, Closed }
   ```

### R1-DEVICE-ENGINE-10 (device-engine-10)

`Evidence` and every part of `Status` get Rust definitions, so that `device.md` §6.5's mapping decides the bytes of `meta.tdk` and of devicectl's `status`. An alarm's `seq`, `author` and `code` stay in `AlarmRecord`; `Evidence` holds the rest. Matching edits: device.md, same id.

1. §21.1: directly after the code block that defines `AlarmRecord`, add:

   ```rust
   pub enum ForkSource { Welcome, PageHead, RepeatedEntry, BasedOn }
   pub enum Evidence {
       RelayBehind { relay_head: Head, relay_last_ctr: Option<u64>, local_head: Head, ack: Option<(u64, Hash32)> },
       Fork { local_chain: Hash32, relay_chain: Hash32, source: ForkSource },
       Counter { ctr: u64, expected: u64 },
       Unechoed { ack_frame_hash: Hash32, ack_chain: Hash32, log_frame_hash: Hash32, log_chain: Hash32 },
       Withheld { relay_head: u64, claimed_ctr: Option<u64>, cursor: u64, missing_from: u64, missing_to: u64,
                  counters: bool },
       ForeignSigner { ctr: u64 },
       IdentityClone { session: Option<[u8; 16]>, ctrs: Vec<u64> },
       Mismatch { groups: Vec<(Hash32, Vec<Dev>)> },
       Frame { frame_hash: Option<Hash32> },
       Request { request: String },
       Blob { blob: [u8; 16] },
       RestoredCopy { entries: u32 },
   }
   ```

   and, directly after it, this paragraph and table:

   Each `AlarmKind` carries the `Evidence` variant and fills the `AlarmRecord` fields of its row; a field the row does not name is absent. In `Mismatch`, `groups` are ascending by hash and each group's devices ascending by `dev`. In `Withheld`, `missing_from..=missing_to` is a range of seqs, or of counters when `counters` is true. `Request.request` names the exchange: `hello`, `read`, `append`, `login`, `blob`, or `admin/<op>` with `op` as `relay.md` §12.1 names it.

   | `AlarmKind` | `Evidence` | `seq` | `author` | `code` |
   |---|---|---|---|---|
   | `RelayBehind` | `RelayBehind` | | | |
   | `Fork` | `Fork` | the seq | | |
   | `Counter` | `Counter` | the seq | the frame's author | `seal/counter_replay` or `seal/counter_gap` |
   | `Unechoed` | `Unechoed` | the acknowledged seq | | |
   | `Withheld` | `Withheld` | | | |
   | `ForeignSigner` | `ForeignSigner` | the seq | this device | |
   | `IdentityClone` | `IdentityClone` | | | |
   | `StateMismatch`, `SnapshotMismatch`, `ResetMismatch` | `Mismatch` | the aligned seq, or the reset's target | the snapshot's writer or the reset's author | |
   | `SkippedFrame` | `Frame` with the frame hash | the seq | | the sealer's code |
   | `RefusedFrame` | `Frame` with `None` | the seq | the author | the code |
   | `RelayProtocol` | `Request` | the seq the request named, if any | | the code |
   | `BlobFailure` | `Blob` | | the writer | the code (`seal.md` §20.5) |
   | `RestoredCopy` | `RestoredCopy` | | | |

2. §30.1: directly before the paragraph that begins `` `Engine::pending()` lists the unapplied records ``, add this sentence and code block:

   The table's fields are those of `Status`:

   ```rust
   pub enum SessionState { Offline, Connecting, Reconciling, Live, Rejoining }
   pub struct ClockStatus { pub skew_ms: Option<i64>, pub warning: bool, pub blocked: bool, pub confirmed_until_ms: Option<i64> }
   pub struct SkipOffer { pub seq: u64, pub here: bool }
   pub struct HaltView { pub kind: HaltKind, pub seq: u64, pub author: Dev, pub author_name: String, pub author_fp: String,
                         pub since_ms: i64, pub code: String, pub skip_offer: Option<SkipOffer> }
   pub enum StopAction { RestoreTail, ReHome, Adopt, RePair, ReplaceDevice, UseThisCopy, Restart, ConfirmRejoin, Wait }
   pub struct StopView { pub kind: StopKind, pub since_ms: i64, pub alarm: Option<u64>, pub action: StopAction }
   pub struct DeviceRow { pub dev: Dev, pub name: String, pub key_fp: String, pub device_no: u16, pub status: DeviceStatus,
                          pub is_self: bool, pub last_beat_ms: i64, pub last_entry_ms: Option<i64>, pub rules: Vec<u32>,
                          pub bootstrap_seq: u64 }
   pub struct Blocker { pub dev: Dev, pub name: String, pub key_fp: String, pub last_beat_ms: i64, pub rules: Vec<u32> }
   pub struct VersionStatus { pub version: u32, pub blockers: Vec<Blocker>, pub override_vote: Option<(u32, Dev, u64)> }
   pub struct RulesStatus { pub active: u32, pub versions: Vec<VersionStatus> }
   pub struct RecoveryChangeView { pub proposer: Dev, pub proposer_name: String, pub key_fp: String, pub start_ms: i64,
                                   pub confirm_after_ms: i64, pub can_confirm: bool }
   pub struct MovingView { pub host: String, pub home_gen: u32, pub fence_seq: u64, pub tries_left: u32,
                           pub locked_until_ms: Option<i64> }
   pub struct OpStatus { pub ticket: u64, pub kind: String, pub stage: u8, pub started_ms: i64 }  // kind: the Operation variant in snake_case
   pub enum StatusWarning { RestoredCopy { entries: u32 }, ContinuityUnchecked { from: u64 }, BasedOnUnchecked { count: u32 } }
   pub struct Status {
       pub session: SessionState, pub relay_host: String, pub push: bool, pub last_synced_ms: Option<i64>,
       pub unreachable_since_ms: Option<i64>, pub paused_until_ms: Option<i64>, pub queued: u32,
       pub oldest_queued_ms: Option<i64>, pub doomed: u32, pub queue_large: bool, pub head: Head, pub board_seq: u64,
       pub fingerprint: String, pub clock: ClockStatus, pub halt: Option<HaltView>, pub stops: Vec<StopView>,
       pub alarms: Vec<AlarmRecord>, pub banners: Vec<Banner>, pub devices: Vec<DeviceRow>, pub rules: RulesStatus,
       pub recovery_change: Option<RecoveryChangeView>, pub moving: Option<MovingView>, pub ops: Vec<OpStatus>,
       pub notices_unread: u32, pub refusals_unread: u32, pub warnings: Vec<StatusWarning>,
   }
   ```

   `VersionStatus` is §25.2's (`override_vote` is `M_K.pending_override` when it names `version`); `relay_host` is empty before a relay target is set.

3. §37 Public API: replace

   ```rust
   pub use ui::{Status, PendingMark, NoticeEntry, Notification, UserAction, Banner, BannerKind,
                BannerDetail, AlarmRecord, AlarmKind, Evidence, VersionStatus};       // §14.2, §21.1, §30
   ```

   with

   ```rust
   pub use ui::{Status, SessionState, ClockStatus, SkipOffer, HaltView, StopAction, StopView, DeviceRow, Blocker,
                VersionStatus, RulesStatus, RecoveryChangeView, MovingView, OpStatus, StatusWarning,
                PendingMark, NoticeEntry, Notification, UserAction, Banner, BannerKind, BannerDetail,
                AlarmRecord, AlarmKind, Evidence, ForkSource};                        // §14.2, §21.1, §30
   ```

### R1-DEVICE-ENGINE-11 (device-engine-11)

The log floor and the chain just below it survive a restart: the engine hands `C_{s−1}` with every truncation, the runtime keeps both with its durable floor and presents them in `Loaded`, and the engine takes the larger of its checkpoint's floor and the runtime's. Matching edits: device.md, same id.

1. §7.2, last sentence: replace `` `retained_from` is `log_floor − 1` when the device holds that seq's chain value (from the snapshot, or from the record it just truncated), else `log_floor`. `` with:

   `retained_from` is `log_floor − 1` when the device holds that seq's chain value (from the snapshot, from the record it just truncated, or after a `Start` as §7.10 states), else `log_floor`. Every `TruncateLogBelow` hands the runtime `C_{s−1}` when the engine holds it (§7.11), so the value survives a restart.

2. §7.10, the table: add these two rows directly after the row `chains`:

   `| `log_floor` | the runtime's durable log floor: the `seq` of the latest `TruncateLogBelow` it completed, 1 before any |`
   `| `floor_chain` | the `floor_chain` that `TruncateLogBelow` handed it with that floor, or absent |`

3. §7.10: add this paragraph directly after the table, before ENG-STATE-002:

   At `Start` the engine's `log_floor` is the larger of the checkpoint's `log_floor` (for a bootstrap: 1 at genesis, `h + 1` at a join) and `Loaded.log_floor`, and it uses no `chains` or `tail` record below it except to learn `C_{log_floor−1}`. `retained_from` is `log_floor − 1` when `C_{log_floor−1}` is known, from `Loaded.floor_chain` when `Loaded.log_floor` equals `log_floor`, from the checkpoint's `board_chain` when its `board_seq` is `log_floor − 1`, or from a `chains` record at that seq; otherwise it is `log_floor`.

4. §7.10: add after ENG-STATE-003:

   - **ENG-STATE-005** (engine) At `Start`, the engine MUST take as its log floor the larger of the checkpoint's (or the bootstrap's) and `Loaded.log_floor`, MUST NOT ask the runtime for a log record below it, and MUST set `retained_from` as §7.10 states.

5. §7.11, the table: replace the row `| `TruncateLogBelow` | `seq: u64` | drop every log record below `seq` |` with:

   `| `TruncateLogBelow` | `seq: u64, floor_chain: Option<Hash32>` | drop every log record below `seq`; `floor_chain` is `C_{seq−1}` when the engine holds it (its chain index, or a rejoin snapshot's `C_S`, §19.7), else `None`; the runtime keeps both (`device.md` §7.3) |`

6. §37 Public API, `pub enum PersistOp`: replace `TruncateLogBelow(u64),` with `TruncateLogBelow(u64, Option<Hash32>),`.

7. §37 Public API: replace

   ```rust
   pub struct Loaded { pub checkpoint: Option<Checkpoint>, pub bases: Vec<(u64, Hash32)>, pub chains: Vec<(u64, Hash32)>,
                       pub tail: Vec<LogRecord>, pub hashes: Vec<HashRecord>, pub outbox: Vec<OutboxOp>,
                       pub notices: Vec<NoticeOp>, pub meta: Option<Meta>, pub marker: Option<Marker>,
                       pub build_id: String, pub session: [u8; 16], pub bootstrap: Option<Bootstrap> }
   ```

   with

   ```rust
   pub struct Loaded { pub checkpoint: Option<Checkpoint>, pub bases: Vec<(u64, Hash32)>, pub chains: Vec<(u64, Hash32)>,
                       pub log_floor: u64, pub floor_chain: Option<Hash32>,
                       pub tail: Vec<LogRecord>, pub hashes: Vec<HashRecord>, pub outbox: Vec<OutboxOp>,
                       pub notices: Vec<NoticeOp>, pub meta: Option<Meta>, pub marker: Option<Marker>,
                       pub build_id: String, pub session: [u8; 16], pub bootstrap: Option<Bootstrap> }
   ```

### R1-DEVICE-ENGINE-12 (device-engine-12)

Nothing further to apply. R1-SEAL-ENGINE-19 already applies ruling S25: §19.2 with the 1,000-entry trigger and the one-per-24-hours limit, ENG-SNAP-001 reworded, ENG-SNAP-006, the §34 rows "snapshot entry trigger" and "snapshots per device", §38 item 14 replaced, and S25 in §1.

### R1-DEVICE-ENGINE-16 (device-engine-17)

A local read the runtime cannot serve in full has an answer, and it is the engine's local fault. Matching edits: device.md, same id.

1. §4.2, the events table, row `LocalRead`: replace its Fields cell `` `req: u64, result: LocalReadResult` (records or a base, §37) `` with `` `req: u64, result: LocalReadResult` (records, a base, or `Missing`, §37) ``.

2. §4.3, the effects table: replace the rows

   `| `ReadLog` | `req: u64, from: u64, to: u64` | reads the durable log records `from..=to` and sends `LocalRead` |`
   `| `ReadBase` | `req: u64, seq: u64` | reads the base checkpoint at `seq` and sends `LocalRead` |`

   with

   `| `ReadLog` | `req: u64, from: u64, to: u64` | reads the durable log records `from..=to` and sends `LocalRead`; `Missing` when it cannot read every one of them |`
   `| `ReadBase` | `req: u64, seq: u64` | reads the base checkpoint at `seq` and sends `LocalRead`; `Missing` when it cannot read that base |`

3. §21.4, the parenthesis after `engine/local_fault` (as R1-CORE-ENGINE-20 item 11 left it): replace `or an `SM::reset` error)` with `an `SM::reset` error, or a `LocalRead` answered `Missing`)`. (The parenthesis then reads: `(a panic in a trait method other than `apply`, an inconsistency between two of the device's own records, a checkpoint, base or pairing material of its own whose board does not load, an `SM::reset` error, or a `LocalRead` answered `Missing`)`.)

4. §33 Error catalogue, the row `engine/local_fault` (as R1-CORE-ENGINE-20 item 12 left it): replace its Meaning cell with `a trait method other than `apply` panicked, two own records disagree, an own checkpoint, base or pairing material does not load, `SM::reset` failed, or the runtime could not read a log record or base it holds` and its "Raised when" cell with `ENG-GEN-012, §4.3, §7.10, §19.4, §19.8, §21.4, §28.1`.

5. §37 Public API: replace `pub enum LocalReadResult { Records(Vec<LogRecord>), Base(Checkpoint) }` with `pub enum LocalReadResult { Records(Vec<LogRecord>), Base(Checkpoint), Missing { code: &'static str } }`.

### R1-DEVICE-ENGINE-17 (device-engine-18)

Nothing further to apply. R1-SEAL-ENGINE-20 names the file of every kind in §7's table (kind 1 as the segments `log-<first seq>.tdl`, 7, 8, 9, 10), points the lead sentence to `seal.md` §19.2 and deletes the §36 request for kinds 9 and 10; R1-CORE-ENGINE-22 adds the row for kind 17 and the sentence that kinds 11, 12 and 18 are `device.md`'s. The finding's alternative (kind numbers only) is not adopted, since those changes already make the two specs name the files alike.

### R1-DEVICE-ENGINE-19 (device-engine-20)

One owner per rule: the runtime's effect order and persist order stay the engine's contract clauses (ENG-EFFECT-001, ENG-PERSIST-001, ENG-PERSIST-004, each owned by engine and device); the device keeps only what is its own. Matching edits: device.md, same id.

Reword ENG-GEN-012, owners only (full text):

- **ENG-GEN-012** (engine) Every call of a state-machine or sealer method that can run code derived from replicated data (`apply`, `trial_apply`, `parse`, `load`, `canonical_bytes`, `upgrade`, `reset`, `classify`, `open_snapshot`) MUST run inside `std::panic::catch_unwind`, and a caught panic MUST be handled as §15 states for `apply` and as a local fault (`engine/local_fault`, §21.4) for every other method.

(The device's own calls into the pure crates stay DEV-GEN-009's.)

## device.md

### R1-DEVICE-ENGINE-1 (device-engine-1)

Ruling S27 as the earlier resolutions and the lead's ruling L4 applied it: the sealer fills a snapshot body's control history and keeps it when it opens one (R1-SEAL-RELAY-9, R1-SEAL-RELAY-28, R1-SEAL-ENGINE-15). The finding's `history` field in `SnapshotMaterial` and in the kind-17 Join layout is not adopted: a joining device writes `history.tdk` from the bundle snapshot before `bootstrap.tdk` (§18.3 as R1-SEAL-RELAY-28 left it). What is missing is the rejoin (`engine.md` §19.7), which installs a snapshot without a bootstrap file; the store then takes the history in the persist of the rejoin's checkpoint.

1. §8.3, the table: add this row directly after the row `| history records (kind 11) | after an `AppendLog`: … |`:

   `| history records from an opened snapshot | after the sealer adapter's `open_snapshot` returned a body (a rejoin, `engine.md` §19.7), in the first `Persist` that carries a `WriteCheckpoint`, before that op: the body's history records (`seal.md` §20.4) with seq above the last seq in `history.tdk`, in seq order; the adapter then drops the body's history | a device that rejoined from a snapshot holds the control history from seq 1 (§15.7, ruling S27) |`

2. §8.3: add after DEV-RUN-005:

   - **DEV-RUN-006** (device) After the sealer adapter opened a snapshot, the runtime MUST append to `history.tdk`, in the first `Persist` that carries a `WriteCheckpoint` and before that op, every history record of the snapshot's body whose seq is above the last seq the file holds, in seq order.

### R1-DEVICE-ENGINE-2 (device-engine-2)

The creating device's bootstrap file carries the relay target (`engine.md` §19.8, same id).

1. §6.4.9: replace `` `0x01 ‖ vb32(board) ‖ vb32(frame_1)` for a creating device (`Bootstrap::Genesis`), `` with `` `0x01 ‖ vb32(board) ‖ vb32(frame_1) ‖ vb8(host) ‖ door[5] ‖ home_gen u32` with `home_gen = 1` for a creating device (`Bootstrap::Genesis`, then the relay target), ``.

2. §18.1, step 7: replace `` `bootstrap.tdk` (`Genesis { board, frame_1 }`) `` with `` `bootstrap.tdk` (`Genesis { board, frame_1, relay }`, `relay` being `relay_host`, the door of step 3 and `home_gen = 1`) ``.

### R1-DEVICE-ENGINE-3 (device-engine-3)

`follow_address` hands the typed code to the engine (`engine.md` §28 and §28.3 step 4, same id).

§17.8, the table: replace the row `| `follow_address(address, code)` | D, A | `Op(FollowAddress)` | a ticket | `relay/address`, `engine/move_no_fence` | — | short | all |` with:

`| `follow_address(address, code)` | D, A | `Op(FollowAddress { host, door, code })`, `host` and `door` from `parseAddress` (`relay.md` §5.4) | a ticket | `relay/address`; through `wait_op`: `engine/move_locked`, `engine/move_no_fence`, `seal/move_code` | — | short | all |`

### R1-DEVICE-ENGINE-4 (device-engine-4)

`adopt` becomes an operation with a ticket, and `enter_move_code` returns the engine's `MoveCodeResult` (`engine.md` §22.3, §28.3, §30.3, same id).

1. §17.7, the table: replace the row `| `adopt(dev, typed_fp)` | D, A | `User(Adopt)`: adopting a state by fingerprint (V1-10) | `engine/fingerprint`, `engine/adopt_membership` (through `wait_op`) | auth | short | all |` with:

   `| `adopt(dev, typed_fp)` | D, A | `Op(Adopt { dev, typed_fp })`: adopting a state by fingerprint (V1-10); returns a ticket | `engine/op_busy`; through `wait_op`: `engine/fingerprint`, `engine/adopt_membership`, the open and load codes of `engine.md` §22.3 step 3, `engine/op_timeout` | auth | short | all |`

2. §17.8, the table: replace the row `| `enter_move_code(text)` | D, A | `User(MoveCode)` | `()` | `engine/move_locked`, `seal/move_code` | — | short | all |` with:

   `| `enter_move_code(text)` | D, A | `User(MoveCode)` | `()` when the `Notify(MoveCodeResult)` of the same loop turn is `Ok` | from that result: `engine/move_locked`, `engine/move_no_fence`, `seal/move_code` | — | short | all |`

3. §8.4, the table: add this row directly after the row `| `MoveCode`, `MoveReport` | keeps them for `move_status` (§17.8) |`:

   `| `MoveCodeResult { result }` | answers the waiting `enter_move_code` (§17.8) |`

4. §19.2, the table: replace the row `| `adopt` | `dev` hex16, `fp` | | |` with `| `adopt` | `dev` hex16, `fp` | | ticket |`.

5. §25 Public API: replace `    pub fn adopt(&self, dev: [u8; 16], typed_fp: &str) -> Result<(), DeviceError>;` with `    pub fn adopt(&self, dev: [u8; 16], typed_fp: &str) -> Result<u64, DeviceError>;               // ticket`.

### R1-DEVICE-ENGINE-5 (device-engine-5)

`purge_preview` reads the engine's own computation (`engine.md` §28.2, ENG-PURGE-004, same id).

1. §4.4, the paragraph that begins `` `Published` holds: ``: replace `` `Engine::status()`, `Engine::pending()`, `Engine::notices()`, `` with `` `Engine::status()`, `Engine::pending()`, `Engine::notices()`, `Engine::purge_preview()`, ``.

2. §17.8, the table: replace the row `| `purge_preview()` | D | — | `{ reach_seq, reach_ms }`: the seq the purge would reach and the log-clock time of the entry before it (`engine.md` §28.2 step 1) | `engine/nothing_to_purge` | — | read | desktops |` with:

   `| `purge_preview()` | D | — | `{ reach_seq, reach_ms }` from `Engine::purge_preview()` in the published view (§4.4): the seq the purge would reach and the log-clock time of the entry before it (`engine.md` §28.2 step 1, ENG-PURGE-004) | `engine/nothing_to_purge` | — | read | desktops |`

### R1-DEVICE-ENGINE-6 (device-engine-6)

`Door` is the plain `GET` route of `relay.md` §10.9 with the create secret, not an admin operation.

§10.5, the table: replace the row

`| `Admin { req, op }` | `POST …/admin/nonce` (body: the signer, this device's `dev`), then `POST …/admin/<name>` with the envelope of `relay.md` §12.1, `sig = sign_admin(Signer::Device(keys), op, board, nonce)`; credential `T`, except `Door` and `Seed` (create secret) and `SeedTail` (the token `Seed` returned) on the new relay named by the op | `AdminResult { req }` |`

with these two rows, in this order:

`| `Admin { req, op: Door { host } }` | `GET https://<host>/v1/b/<board>/door` with the create secret held for the operation as credential (`relay.md` §10.9, §5.6); no admin nonce and no signature | `AdminResult { req }`: on 200, `Ok(AdminOk { door: Some(d), token: false })` with every other field `None`; a JSON error as §10.6 |`
`| `Admin { req, op }`, every other `op` | `POST …/admin/nonce` (body: the signer, this device's `dev`), then `POST …/admin/<name>` with the envelope of `relay.md` §12.1, `sig = sign_admin(Signer::Device(keys), op, board, nonce)`; credential `T`, except `Seed` (the create secret) and `SeedTail` (the token `Seed` returned) on the new relay named by the op | `AdminResult { req }` |`

### R1-DEVICE-ENGINE-7 (device-engine-7)

A budget refusal of an admin call reaches the engine as that call's `AdminResult`, where §20.2 and §28.2 handle it (`engine.md` §28.2 step 3, same id).

§10.6, the table: replace the row `| `429 relay/budget` or `429 relay/delete_budget` as JSON | `RelayRefused` with `until` from the body | as above, or nothing for the delete budget |` with these two rows, in this order:

`| `429 relay/budget` as JSON answering `Hello`, `Read` or `Append` | `RelayRefused { req, … }` with `until` from the body | as above |`
`| `429 relay/budget` or `429 relay/delete_budget` as JSON answering an `Admin` | `AdminResult { req, Err(RelayRefusal { status: 429, code, until }) }`, `until` from the body | the operation's result; nothing for the delete budget |`

### R1-DEVICE-ENGINE-8 (device-engine-8)

`engine.md` §11.9 now handles `507 relay/storage_refused` (same id).

1. §24, `engine.md` block: delete the bullet `- Requires: the engine treats `RelayRefused { status: 507, code: "relay/storage_refused", until }` as an append pause until `until`, as it treats `429 relay/budget` (§10.6); `engine.md` §11.9's table has no row for it.`

2. §26, open question 9: replace the whole item with:

   9. **The `507 relay/storage_refused` pause** is passed to the engine as a `RelayRefused` with `until` (§10.6), which `engine.md` §11.9 handles as a pause of writes until `until`, with reads going on and no alarm. Decided there.

### R1-DEVICE-ENGINE-9 (device-engine-9)

A refused login ends the `Connect` with a defined event (`engine.md` §11.9, same id).

1. §10.3, the paragraph that begins `Login: `POST /v1/b/{board}/login/nonce``: append at its end: ` A JSON refusal of `login-nonce` or `login` ends the `Connect` as §10.6 states.`

2. §10.6, the table: add this row directly after the row `| `401 relay/token_stale` | none: login and one retry (§10.3) | nothing |`:

   `| any JSON error answering `login-nonce` or `login` (§10.3), whatever its code | `RelayRefused { req: None, refusal }`, then `Disconnected { unreachable: false, paused_until_ms: None }`; the `Connect` counts as a failure (§10.7) | `engine.md` §11.9 |`

### R1-DEVICE-ENGINE-10 (device-engine-10)

Every type the record JSON maps now has its fields in `engine.md` (`Evidence` in §21.1, `Status` and its parts in §30.1; same id).

1. §6.5, first paragraph: replace `applied recursively to the Rust definitions in `engine.md` §37 and to the field definitions the `engine` stage's skeleton gives the types `engine.md` names without fields (`Evidence`, `Status` parts):` with `applied recursively to the Rust definitions in `engine.md` (§37, with `Evidence` in §21.1 and `Status` and its parts in §30.1):`.

2. §24, `engine.md` block: delete the bullet that begins `- Requires: every type that §6.5 maps (`Registry`, `OutboxRecord`, `Meta` and what they contain,`.

### R1-DEVICE-ENGINE-11 (device-engine-11)

The runtime keeps the floor's chain with the floor and presents both in `Loaded` (`engine.md` §7.10, §7.11, same id).

1. §7.3, the table: replace the row `| `TruncateLogBelow(s)` | replace `device.tdk` with `log_floor = s` (§16); then remove every segment whose last seq is below `s` |` with:

   `| `TruncateLogBelow(s, c)` | replace `device.tdk` with `log_floor = s` and `floor_chain = c`, the field absent when `c` is `None` (§16); then remove every segment whose last seq is below `s` |`

2. Reword DEV-FILE-006 (full text):

   - **DEV-FILE-006** (device) `TruncateLogBelow(s, c)` MUST make `log_floor = s` and `floor_chain = c` durable in `device.tdk` before it removes any segment, and MUST NOT remove a segment that holds any seq at or above `s`.

3. §7.4, the paragraph that begins `` `Loaded` (`engine.md` §7.10) is filled from these files exactly: ``: replace `` `chains` and `tail` from kind 1 (`tail` = the records above `checkpoint.board_seq`); `` with `` `chains` and `tail` from kind 1 (`tail` = the records above `checkpoint.board_seq`); `log_floor` and `floor_chain` from kind 12 (`log_floor` 1 and `floor_chain` absent when `device.tdk` holds none); ``.

4. §8.3, the table: replace the row `| `log_floor` in `device.tdk` | before a `TruncateLogBelow` removes anything | DEV-FILE-006 |` with `| `log_floor` and `floor_chain` in `device.tdk` | before a `TruncateLogBelow` removes anything | DEV-FILE-006 |`.

5. §8.3, the paragraph after the table: replace `Nothing the engine reads at `Start` lives in those files except `log_floor`.` with `Nothing the engine reads at `Start` lives in those files except `log_floor` and `floor_chain`.`

6. §16, the `device.tdk` / `device.json` table: add this row directly after the row `device_name`:

   `| `floor_chain` | `hex32` | optional | synced mode only | `C_{log_floor − 1}` as the engine handed it with the floor (§7.3) |`

### R1-DEVICE-ENGINE-13 (device-engine-13)

Ruling S24 as the lead ruled it (L1): the recovery-signed `reinstate` stores the missed roots' wraps at the relay, which returns them in every `login` answer of that device while they are stored; the sealer verifies each root against its rotation's commitment while it classifies in seq order, and the engine resumes classification (the engine rule, the relay and seal bytes and the field's name are the lead's change file's). The device's part is to hand those wraps to the sealer adapter. Written conservatively, without naming the field (see "For the lead", item 1).

1. §10.3, the paragraph that begins `Login: `POST /v1/b/{board}/login/nonce``: after the sentence that ends `` `push` updates the stored capability. `` insert:

   When a `login` answer carries the wraps of the epoch roots this identity missed while it was revoked (ruling S24, `relay.md` §10.3), the transport hands them to the sealer adapter before it uses the new token; the adapter holds them in memory for classification, which checks each root against the commitment of the rotation that set its epoch (`seal.md`), and the roots reach `identity.tdk` only through the engine's `WriteIdentity` after `keys_changed` (`engine.md` §6.1, D4). A wrap that does not open or a root that does not match is the commitment halt (`engine.md` §16.1).

2. §18.4, first sentence: replace `` `re_pair(address, code, device_name)` for an identity that is revoked, `` with `` `re_pair(address, code, device_name)` for an identity that is revoked and not reinstated (a reinstated identity resumes on its own, ruling S24), ``.

### R1-DEVICE-ENGINE-14 (device-engine-14)

Ruling S29: the phone's own picture, which `bridge.md` §6.4.4 and §20 require. Kind 19 `own-picture.tdk` is already in `seal.md` §19.2 and `FileKind::OwnPicture` (R1-SEAL-RELAY-10). The own picture is never uploaded and never enters the board.

1. §6.1, the layout: add the line `      own-picture.tdk                 kind 19, Android only, present only while the phone has its own picture` directly after the line `      picture.tdk                     kind 13`.

2. §6.1, the kinds table: add after the row of kind 18:

   `| 19 | `own-picture.tdk` | one | the store (§14.3) | §6.4.14 |`

3. §6.1: replace the sentence R1-SEAL-RELAY-29 item 1 wrote (from `Kinds 9 to 18 are this spec's and are listed in `seal.md` §19.2.` to its end, `(`bridge.md` §20).`) with:

   Kinds 9 to 19 are this spec's and are listed in `seal.md` §19.2; kind 19 is the phone's own picture (ruling S29, §14.3).

4. §6.4: add this subsection after §6.4.13 (before DEV-STORE-005):

   #### 6.4.14 Own picture (kind 19)

   The layout of §6.4.11 with `blob`, `epoch`, `parts`, `sha256` and `size` all zero bytes: `clean` the re-encoded JPEG of §14.3 step 3, `blur` and `light` the dials its `look` was made with, `look` and `top` as §6.4.11.

5. Reword DEV-STORE-005 (full text):

   - **DEV-STORE-005** (device, recovery) Every record plaintext of kinds 1 to 19 MUST be laid out exactly as §6.4 states for its kind, and a reader MUST treat any other plaintext as `device/record_format`.

6. §7.2, the table: in the row **replace**, replace `kinds 2, 6, 7, 9, 10, 12 to 18,` with `kinds 2, 6, 7, 9, 10, 12 to 19,`; in the row **remove**, replace `` `draft.tdk` after a save, `` with `` `draft.tdk` after a save, `own-picture.tdk` on `clear_own_picture`, ``.

7. §7.4, first paragraph: replace `` `picture.tdk`, `weather.tdk`, `draft.tdk`. `` with `` `picture.tdk`, `own-picture.tdk`, `weather.tdk`, `draft.tdk`. ``; and in the second table, replace the row `| `overlay.tdk`, `feed-*.tdk`, `picture.tdk`, `weather.tdk`, `draft.tdk` | removed and treated as absent; reported |` with `| `overlay.tdk`, `feed-*.tdk`, `picture.tdk`, `own-picture.tdk`, `weather.tdk`, `draft.tdk` | removed and treated as absent; reported |`.

8. §8.3, the paragraph after the table: replace `` `picture.tdk`, `weather.tdk` and `draft.tdk` on its own, `` with `` `picture.tdk`, `own-picture.tdk`, `weather.tdk` and `draft.tdk` on its own, ``.

9. §14: add this subsection after DEV-PIC-004 (the end of §14.2), before `## 15.`:

   ### 14.3 The phone's own picture (Android)

   The phone may show a picture of its own instead of the board's shared picture (SYNC.md §3: the phone's "own Look", ruling S29). `set_own_picture(bytes)` (§17.12), on the picture thread and then the sync thread, in order:

   1. The mode is synced (`device/needs_sync`); `bytes` is 1 to 6,291,456 bytes (`device/picture_too_large`) and starts with `FF D8 FF` (`device/picture_format`).
   2. Decode with `ImageReader::with_format(ImageFormat::Jpeg)` and the limits of §14.1 step 3, under `catch_unwind`; a failure or panic is `device/picture_decode`.
   3. Centre-crop, resize and encode as §14.2 step 6: the **clean** own picture.
   4. Make its **look** with this device's dials as §14.2 step 7.
   5. Write `own-picture.tdk` (§6.4.14) and publish.

   `clear_own_picture()` removes `own-picture.tdk` and publishes. While `own-picture.tdk` exists, `Published::picture()` returns its look; otherwise the shared picture's (§14.2). `set_look_dials(blur, light)` re-makes the look of the picture shown from its stored clean copy and rewrites its file. The own picture is never sealed as a blob, uploaded, or carried in an intent, a frame, a backup or the board.

   - **DEV-PIC-005** (device) `set_own_picture` MUST accept only a JPEG (first bytes `FF D8 FF`) of 1 to 6,291,456 bytes, MUST decode it only with `ImageFormat::Jpeg` under the limits of §14.1 step 3, MUST keep only its re-encoded clean copy and look, in `own-picture.tdk`, and MUST NOT upload it or put it into any intent, frame, blob or backup.

10. §14.2, the last paragraph: replace `` `set_look_dials(blur, light)` (§17.12) re-makes step 7 from the stored clean picture. `` with `` `set_look_dials(blur, light)` (§17.12) re-makes step 7 from the stored clean picture, or §14.3's look while the phone shows its own picture. ``

11. §17.5, the table: replace the row `| `overlay()`, `weather()`, `picture()` | the per-device overlay, the latest report with its `fetched_ms`, the processed picture (`look` bytes, `id`, `top`) | D, A |` with `| `overlay()`, `weather()`, `picture()` | the per-device overlay, the latest report with its `fetched_ms`, the processed picture (`look` bytes, `id`, `top`): the phone's own picture while it has one (§14.3), else the shared one | D, A |`.

12. §17.12, the table: add these two rows directly after the row `clear_shared_picture()`:

    `| `set_own_picture(bytes)` | A | `()` | `device/picture_too_large`, `device/picture_format`, `device/picture_decode`, `device/needs_sync` | — | long: at most 120,000 ms | Android |`
    `| `clear_own_picture()` | A | `()` | `device/needs_sync` | — | short | Android |`

13. §22 Limits: replace the row `| uploaded picture | 6,291,456 | bytes | DEV-PIC-003 |` with `| uploaded picture; the phone's own picture | 6,291,456 | bytes | DEV-PIC-003, DEV-PIC-005 |`.

14. §23 Test vectors, row `device-files.json`: replace `the file bytes for every kind 1 to 18;` with `the file bytes for every kind 1 to 19;`.

15. §24, `bridge.md` block: replace `- Provides: nothing else to the facade.` with `- Provides: `set_own_picture` and `clear_own_picture` (§14.3, §17.12) for `POST /api/background`; nothing else to the facade.`

16. §25 Public API, `impl Device`, block `// ---------- picture, settings, export (§17.12)`: after the line `    pub fn set_look_dials(&self, blur: u8, light: u8) -> Result<(), DeviceError>;` add the lines

    ```rust
        pub fn set_own_picture(&self, bytes: Vec<u8>) -> Result<(), DeviceError>;          // §14.3, Android
        pub fn clear_own_picture(&self) -> Result<(), DeviceError>;
    ```

17. §25 Public API, `pub mod store`: replace `    pub const KIND_UNSENT: u8 = 18;` with `    pub const KIND_UNSENT: u8 = 18; pub const KIND_OWN_PICTURE: u8 = 19;`.

### R1-DEVICE-ENGINE-15 (device-engine-15)

The recovery tool mints at most twice: epoch 1, then the `read_epoch` the refusal carries (device.md §15.8 step 1 as R1-SEAL-RELAY-27 left it).

§22 Limits: replace the row `| recovery read-token mints | 32 | tries | DEV-BKP-011 |` with `| recovery read-token mints | 2 | tries (epoch 1, then the `read_epoch` of the refusal) | DEV-BKP-011 |`.

### R1-DEVICE-ENGINE-16 (device-engine-17)

A local read the store cannot serve is answered, and the engine's local fault takes over from the device's own freeze (`engine.md` §4.3, §21.4, §37, same id).

1. §8.2, the table: replace the rows

   `| `ReadLog { from, to }` | reads the records from the segments (it keeps an index of each segment's first seq and record offsets); `LocalRead` with them |`
   `| `ReadBase { seq }` | reads `base-<seq>.tdk`; `LocalRead` with the base. A missing or corrupt base: the runtime renames it as §7.4 does, hands no further input to the engine, and every facade write answers `device/reopen_needed` until the facade is opened again, when the base is absent from `Loaded.bases` |`

   with

   `| `ReadLog { from, to }` | reads the records from the segments (it keeps an index of each segment's first seq and record offsets); `LocalRead` with them; when it does not hold every record `from..=to`, or one does not open, `LocalRead { result: Missing { code: "device/store_corrupt" } }` |`
   `| `ReadBase { seq }` | reads `base-<seq>.tdk`; `LocalRead` with the base. A missing or corrupt base: the runtime renames it as §7.4 does (reported as `quarantined`) and delivers `LocalRead { result: Missing { code: "device/store_corrupt" } }`; the engine stops with a local fault until the next `open` (`engine.md` §21.4), when the base is absent from `Loaded.bases` |`

2. §8.2: add after DEV-RUN-002:

   - **DEV-RUN-007** (device) The runtime MUST answer every `ReadLog` and `ReadBase` with one `LocalRead`, and MUST answer one it cannot serve in full with `Missing`.

3. §17.1, the bullet **Errors**: replace `besides `device/closed` (after `shutdown`), `device/reopen_needed` (§8.2) and `device/wrong_mode` (the call is not offered in the current mode), which every write may return.` with `besides `device/closed` (after `shutdown`) and `device/wrong_mode` (the call is not offered in the current mode), which every write may return.`

4. §21 Error catalogue: delete the row `| `device/reopen_needed` | a base could not be read; the runtime stopped | §8.2 | reopen the facade |`.

### R1-DEVICE-ENGINE-18 (device-engine-19)

A host is at most 255 bytes, `:port` included (`seal.md` §5.4 and SEAL-ENC-006 as R1-SEAL-RELAY-1 left them; `engine.md` ENG-OPS-007 and its §34 row as R1-SEAL-ENGINE-16 and R1-CORE-ENGINE-21 left them). `begin_create` (§18.1 step 1), `join`, `re_pair` and `follow_address` (`parseAddress`, `relay.md` §5.4 step 4) already apply that rule; the stored token's host and `move_board` state it.

1. §16, the `device.tdk` / `device.json` table, row `tokens`: replace its Constraints cell with `ascending by `host`, distinct; at most 4; `host` a host of `seal.md` §5.4 (1 to 255 bytes, `:port` included); `token` 96 ASCII characters`.

2. §17.8, the table, row `move_board(host, create_secret)`: replace its "Engine input" cell with `` `Op(Move { host })` once `host` passes `seal.md` §5.4 (at most 255 bytes, `:port` included; else `relay/address`); the device holds the create secret for that operation's `Door` and `Seed` requests and wipes it when the operation ends ``.

### R1-DEVICE-ENGINE-19 (device-engine-20)

The effect-order and persist-order rules have one owner each: `engine.md`'s contract clauses (ENG-EFFECT-001, ENG-PERSIST-001, ENG-PERSIST-004), which name `device` as an owner. The device keeps what only it does (each op's syncs, the crash suite, its own `catch_unwind`). Matching edit: engine.md, same id (ENG-GEN-012's owners).

1. §8.2: replace the clause `- **DEV-RUN-001** (device, engine) The runtime MUST hand every effect other than `Persist` to its executor in the order the engine emitted it, without waiting for any `Persist` (ENG-EFFECT-001).` with:

   - **DEV-RUN-001** (withdrawn) Replaced by ENG-EFFECT-001 (`engine.md` §4.3), which states the same rule for the runtime and names `device` as an owner.

2. Reword DEV-FILE-004 (full text):

   - **DEV-FILE-004** (device) The runtime MUST complete every step of every op of a `Persist` effect, in order, including each `sync` and `sync_dir`, before it delivers that effect's `Persisted`; the order of the effects and of their `Persisted` events is ENG-PERSIST-001's.

3. §24, `engine.md` block, the `Provides` bullet: replace `serial persists with `Persisted` in order (DEV-FILE-004), other effects in order (DEV-RUN-001),` with `serial persists with `Persisted` in order (ENG-PERSIST-001, each op's syncs by DEV-FILE-004), other effects in order (ENG-EFFECT-001), every durable record at the next `Start` (ENG-PERSIST-004, DEV-CRASH-001),`.

## bridge.md

### R1-DEVICE-ENGINE-4 (device-engine-4)

`adopt` returns a ticket (`device.md` §17.7, §25, same id).

§11.4, the `call` table: replace the row `| `adopt` | `{"dev": hex32, "typed_fp": string}` | `null` | `adopt` | auth | android |` with `| `adopt` | `{"dev": hex32, "typed_fp": string}` | ticket | `adopt` | auth | android |`.

## Rejected

None. Parts of proposals are not adopted, each recorded in its change:
- device-engine-1: `history` in `SnapshotMaterial`, the Sealer contract wording, `vb32(history)` in the kind-17 Join layout and the runtime writing `material.history` at Join. The earlier resolutions and the lead's ruling L4 settled that the sealer fills and keeps the history (R1-SEAL-RELAY-9, -28, R1-SEAL-ENGINE-15); only the rejoin path is added (R1-DEVICE-ENGINE-1).
- device-engine-2: removing `Config` altogether; it stays as an empty struct so `Engine::new` keeps its signature (R1-DEVICE-ENGINE-2).
- device-engine-8: the extension of ENG-RESULT-002; ENG-SESS-006 already binds every row of §11.9, so no clause changes (R1-DEVICE-ENGINE-8).
- device-engine-9: the engine rule is narrowed to a login refused while the session is `Connecting`, and a refused login with another code is retried hourly instead of being left until a `Welcome` that never comes (R1-DEVICE-ENGINE-9).
- device-engine-10: the field set of `Evidence` differs from the proposal: the seq, author and code an alarm already carries in `AlarmRecord` are not repeated, `Fork.source` is an enum, and `Seq`, `Count` are replaced by `Frame` and `RestoredCopy` (R1-DEVICE-ENGINE-10).
- device-engine-11: the device does not take `C_{s−1}` from the record before removing it (that record may not exist, as after a rejoin); the engine hands it in `TruncateLogBelow` (R1-DEVICE-ENGINE-11).
- device-engine-12, device-engine-18: already applied by the earlier files (R1-DEVICE-ENGINE-12, -17).
- device-engine-13: the engine's §24.4 rewrite, the seal change and the deletion of engine.md §36's question and §38 item 24 are the lead's (ruling L1 and the lead's change file); only the device's hand-over is written here (R1-DEVICE-ENGINE-13).
- device-engine-14: the seal.md change (kind 19 is already in seal.md §19.2 by R1-SEAL-RELAY-10).
- device-engine-15: the relay.md change (applied by R1-SEAL-RELAY-20).
- device-engine-19: the seal.md and engine.md changes (applied by R1-SEAL-RELAY-1, R1-SEAL-ENGINE-16, R1-CORE-ENGINE-21; the code there is `engine/bad_host`, not `relay/address`); only device.md follows (R1-DEVICE-ENGINE-18).

## For the lead

1. **Ruling L1 names the S24 engine rule ENG-EPOCH-004, but R1-ENGINE-RELAY-6 already gave that id to the revoked device's hourly read-only reconnect.** The two cannot share an id. Option (a): ENG-EPOCH-004 stays the reconnect and the S24 rule takes ENG-EPOCH-005, as R1-ENGINE-RELAY's "For the lead" item 3 recommends. Option (b): the S24 rule takes ENG-EPOCH-004 and the reconnect is renumbered. Recommended: (a), which renumbers nothing. Also: L1 does not say how the device hands the `login` answer's wraps to the sealer. R1-DEVICE-ENGINE-13 writes that hand-over in device.md §10.3 without naming the relay field, and §18.4 now offers re-pair only to a revoked identity that was not reinstated. If the lead's change file (`r1-lead-changes.md`) words device.md's side differently, the lead's text wins. Recommended: keep R1-DEVICE-ENGINE-13, with the field name taken from the lead's relay.md text.
2. **The phone's own picture (ruling S29) is written here, not by the device–bridge resolver** (R1-SEAL-RELAY's "For the lead" item 3 assigned it there). The finding asks for the same change, and leaving device.md without kind 19's row, layout and facade calls until a later resolver would leave the specs inconsistent. Recommended: the device–bridge resolver builds on R1-DEVICE-ENGINE-14 and adds nothing for S29 except what bridge.md needs. Written as the change.
3. **Adoption becomes an operation with a ticket** (R1-DEVICE-ENGINE-4). An adoption can wait for the chosen device's snapshot, so the facade's "short" call could not return its result. As an operation it reports through `OpDone` and `wait_op`, and it is also allowed under the divergence stop. The facade's `adopt` now returns a ticket, which changes a bridge.md row. Recommended: accept. The alternative keeps `UserAction::Adopt` and adds a notification for its result, which leaves the waiting stage without a ticket. Written as the change.
